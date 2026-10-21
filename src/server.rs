use {
    crate::{
        Res,
        config::Settings,
        state::{Command, Connection},
        transport,
    },
    std::{
        env,
        fs,
        io::{BufRead, BufReader, ErrorKind},
        net::Shutdown,
        os::unix::net::{UnixListener, UnixStream},
        path::{Path, PathBuf},
        sync::{Arc, Mutex, Weak},
        thread,
    },
};

type Connections = Arc<Mutex<Vec<Weak<Connection>>>>;

pub fn run(name: &str, downstream: &Path, control: &Path, settings: Settings) -> Res<()> {
    let upstream = host_compositor_socket()?;
    let connections: Connections = Arc::default();

    serve_control(listen(control)?, Arc::clone(&connections));

    eprintln!(
        "wl-proxy-clip[{name}]: {} -> {}",
        downstream.display(),
        upstream.display()
    );
    let listener = listen(downstream)?;

    for client in listener.incoming() {
        let client = match client {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("accept failed: {error}");
                continue;
            },
        };
        let compositor = match UnixStream::connect(&upstream) {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("failed to connect to compositor: {error}");
                continue;
            },
        };

        let connection = Arc::new(Connection::new(settings));
        connections
            .lock()
            .expect("connection list lock poisoned")
            .push(Arc::downgrade(&connection));

        for direction in [Direction::Requests, Direction::Events] {
            let client = match client.try_clone() {
                Ok(stream) => stream,
                Err(error) => {
                    eprintln!("failed to clone client socket: {error}");
                    continue;
                },
            };
            let compositor = match compositor.try_clone() {
                Ok(stream) => stream,
                Err(error) => {
                    eprintln!("failed to clone compositor socket: {error}");
                    continue;
                },
            };
            spawn_worker(
                client,
                compositor,
                Arc::clone(&connection),
                name.to_owned(),
                direction,
            );
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Direction {
    Requests,
    Events,
}

impl Direction {
    fn forward(
        self,
        client: &UnixStream,
        compositor: &UnixStream,
        connection: &Connection,
    ) -> Res<()> {
        match self {
            Self::Requests => transport::forward_requests(client, compositor, connection),
            Self::Events => transport::forward_events(compositor, client, connection),
        }
    }
}


fn listen(path: &Path) -> Res<UnixListener> {
    match fs::remove_file(path) {
        Ok(()) => {},
        Err(error) if error.kind() == ErrorKind::NotFound => {},
        Err(error) => {
            return Err(format!("failed to remove {}: {error}", path.display()));
        },
    }
    UnixListener::bind(path).map_err(|error| format!("failed to bind {}: {error}", path.display()))
}

fn serve_control(listener: UnixListener, connections: Connections) {
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let command = match line.trim() {
                    "paste" => Command::Paste,
                    "copy" => Command::Copy,
                    other => {
                        eprintln!("unknown command: {other}");
                        break;
                    },
                };

                let mut connections = connections.lock().expect("connection list lock poisoned");
                connections.retain(|connection| connection.strong_count() > 0);
                for connection in connections.iter().filter_map(Weak::upgrade) {
                    connection.enqueue(command);
                }
            }
        }
    });
}

fn spawn_worker(
    client: UnixStream,
    compositor: UnixStream,
    connection: Arc<Connection>,
    name: String,
    direction: Direction,
) {
    thread::spawn(move || {
        if let Err(error) = direction.forward(&client, &compositor, &connection) {
            eprintln!("wl-proxy-clip[{name}]: connection closed: {error}");
        }
        let _ = client.shutdown(Shutdown::Both);
        let _ = compositor.shutdown(Shutdown::Both);
    });
}


fn host_compositor_socket() -> Res<PathBuf> {
    let runtime_dir =
        env::var("XDG_RUNTIME_DIR").map_err(|_| "XDG_RUNTIME_DIR is not set".to_owned())?;
    let display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_owned());
    Ok(PathBuf::from(runtime_dir).join(display))
}
