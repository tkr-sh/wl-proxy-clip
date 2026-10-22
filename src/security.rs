use {
    crate::Res,
    std::{
        env,
        fs,
        io,
        os::{
            fd::{AsFd, OwnedFd},
            unix::net::{UnixListener, UnixStream},
        },
        path::{Path, PathBuf},
        process,
        sync::atomic::{AtomicU64, Ordering},
    },
    wayland_client::{
        Connection,
        Dispatch,
        Proxy,
        QueueHandle,
        globals::{GlobalListContents, registry_queue_init},
        protocol::wl_registry,
    },
    wayland_protocols::wp::security_context::v1::client::{
        wp_security_context_manager_v1::WpSecurityContextManagerV1,
        wp_security_context_v1::WpSecurityContextV1,
    },
};

const SANDBOX_ENGINE: &str = "sh.tkr.wl-proxy-clip";
const SECURITY_CONTEXT_ENV: &str = "WL_PROXY_CLIP_SECURITY_CONTEXT";

static NEXT_LISTENER: AtomicU64 = AtomicU64::new(0);

pub(crate) enum Upstream {
    Direct(PathBuf),
    Tagged(SecurityContext),
}

impl Upstream {
    pub(crate) fn connect(&self) -> io::Result<UnixStream> {
        match self {
            Self::Direct(path) => UnixStream::connect(path),
            Self::Tagged(context) => UnixStream::connect(&context.socket),
        }
    }

    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Direct(path) => path.display().to_string(),
            Self::Tagged(context) => format!("{} (security context)", context.socket.display()),
        }
    }
}

/// A registered security context and the resources that keep it alive.
pub(crate) struct SecurityContext {
    socket: PathBuf,
    _close: OwnedFd,
}

impl Drop for SecurityContext {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
    }
}

#[derive(Default)]
struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpSecurityContextManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WpSecurityContextManagerV1,
        _event: <WpSecurityContextManagerV1 as Proxy>::Event,
        _data: &(),
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpSecurityContextV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WpSecurityContextV1,
        _event: <WpSecurityContextV1 as Proxy>::Event,
        _data: &(),
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
    }
}

/// If the launcher already registered a security context, it can pass the
/// listening socket through `WL_PROXY_CLIP_SECURITY_CONTEXT`. Otherwise
/// register a context with `instance_id = name` through the compositor.
pub(crate) fn upstream(compositor: &Path, name: &str) -> Res<Upstream> {
    if let Some(path) = env::var_os(SECURITY_CONTEXT_ENV) {
        return Ok(Upstream::Direct(PathBuf::from(path)));
    }

    match register(compositor, name) {
        Ok(Some(context)) => Ok(Upstream::Tagged(context)),
        Ok(None) => {
            eprintln!(
                "wl-proxy-clip[{name}]: compositor does not support security contexts: \
                 using the compositor socket directly"
            );
            Ok(Upstream::Direct(compositor.to_path_buf()))
        },
        Err(error) => Err(error),
    }
}

/// Returns `Ok(None)` when the compositor does not advertise
/// `wp_security_context_manager_v1`.
fn register(compositor: &Path, name: &str) -> Res<Option<SecurityContext>> {
    let socket = UnixStream::connect(compositor)
        .map_err(|error| format!("failed to connect to {}: {error}", compositor.display()))?;
    let connection = Connection::from_socket(socket)
        .map_err(|error| format!("failed to initialize Wayland connection: {error}"))?;
    let (globals, mut queue) = registry_queue_init::<State>(&connection)
        .map_err(|error| format!("failed to read the registry: {error}"))?;

    let has_manager = globals.contents().with_list(|globals| {
        globals
            .iter()
            .any(|global| global.interface == "wp_security_context_manager_v1")
    });
    if !has_manager {
        return Ok(None);
    }

    let manager: WpSecurityContextManagerV1 = globals
        .bind(&queue.handle(), 1..=1, ())
        .map_err(|error| format!("failed to bind wp_security_context_manager_v1: {error}"))?;

    let (listener, socket) = bind_listener()
        .map_err(|error| format!("failed to create security context listener: {error}"))?;
    let (close_read, close_write) = crate::fd::pipe()
        .map_err(|error| format!("failed to create security context close pipe: {error}"))?;

    let context =
        manager.create_listener(listener.as_fd(), close_read.as_fd(), &queue.handle(), ());
    context.set_sandbox_engine(SANDBOX_ENGINE.to_owned());
    context.set_app_id(name.to_owned());
    context.set_instance_id(name.to_owned());
    context.commit();

    queue
        .roundtrip(&mut State)
        .map_err(|error| format!("failed to register security context: {error}"))?;

    if let Some(error) = connection.protocol_error() {
        return Err(format!("failed to register security context: {error}"));
    }

    drop(listener);
    drop(close_read);

    Ok(Some(SecurityContext {
        socket,
        _close: close_write,
    }))
}

fn bind_listener() -> io::Result<(UnixListener, PathBuf)> {
    let socket = listener_path();
    match fs::remove_file(&socket) {
        Ok(()) => {},
        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&socket)?;
    Ok((listener, socket))
}

fn listener_path() -> PathBuf {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR").map_or_else(env::temp_dir, PathBuf::from);
    let sequence = NEXT_LISTENER.fetch_add(1, Ordering::Relaxed);
    runtime_dir.join(format!(
        "wl-proxy-clip-{}-{sequence}.security",
        process::id()
    ))
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        std::{
            io::{self, Cursor, Read, Write},
            os::{
                fd::{AsRawFd, FromRawFd, RawFd},
                unix::net::UnixStream,
            },
            sync::{Arc, Mutex, mpsc},
            thread,
            time::Duration,
        },
        uds::UnixStreamExt,
        wlproxy::proto::{self, Packet},
    };

    struct FdReader<'a> {
        stream: &'a UnixStream,
        fds:    &'a mut Vec<RawFd>,
    }

    impl Read for FdReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let mut fd_buffer = [0; 8];
            let (bytes, count) = self.stream.recv_fds(buffer, &mut fd_buffer)?;
            self.fds
                .extend_from_slice(fd_buffer.get(..count).unwrap_or_default());
            Ok(bytes)
        }
    }

    fn read_packet(stream: &UnixStream, fds: &mut Vec<RawFd>) -> Packet {
        proto::read_packet(&mut FdReader { stream, fds })
            .unwrap()
            .expect("connection closed")
    }

    fn write_packet(stream: &UnixStream, id: u32, opcode: u16, body: Vec<u8>) {
        proto::write_packet(&mut &*stream, &Packet { id, opcode, body }).unwrap();
    }

    fn first_uint(body: &[u8]) -> u32 {
        proto::read_arg_uint(&mut Cursor::new(body)).unwrap()
    }

    fn sync_reply(stream: &UnixStream, sync: &Packet) {
        write_packet(
            stream,
            first_uint(&sync.body),
            0,
            0_u32.to_ne_bytes().to_vec(),
        );
    }

    fn global_body(name: u32, interface: &str, version: u32) -> Vec<u8> {
        let mut body = Vec::new();
        proto::write_arg_uint(&mut body, name).unwrap();
        proto::write_arg_string(&mut body, interface).unwrap();
        proto::write_arg_uint(&mut body, version).unwrap();
        body
    }

    fn last_uint(body: &[u8]) -> u32 {
        let start = body.len().checked_sub(4).unwrap();
        u32::from_ne_bytes(body.get(start..).unwrap().try_into().unwrap())
    }

    #[test]
    fn reads_external_security_context_from_environment() {
        // SAFETY: no other test reads this environment variable.
        unsafe { env::set_var(SECURITY_CONTEXT_ENV, "/tmp/launcher-context") };
        let upstream = upstream(Path::new("/tmp/compositor"), "vm1").unwrap();
        assert!(
            matches!(upstream, Upstream::Direct(path) if path == Path::new("/tmp/launcher-context"))
        );
        // SAFETY: see above.
        unsafe { env::remove_var(SECURITY_CONTEXT_ENV) };
    }

    #[test]
    fn registers_security_context_with_instance_id() {
        let directory = std::env::temp_dir().join(format!("wl-proxy-clip-test-{}", process::id()));
        let _ = std::fs::remove_file(&directory);
        let listener = UnixListener::bind(&directory).unwrap();

        let instance_ids = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&instance_ids);
        let (release_tx, release_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let stream = listener.accept().unwrap().0;
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut fds = Vec::new();

            let registry = read_packet(&stream, &mut fds);
            assert_eq!((registry.id, registry.opcode), (1, 1));
            let registry_id = first_uint(&registry.body);

            write_packet(
                &stream,
                registry_id,
                0,
                global_body(7, "wp_security_context_manager_v1", 1),
            );

            let sync = read_packet(&stream, &mut fds);
            assert_eq!((sync.id, sync.opcode), (1, 0));
            sync_reply(&stream, &sync);

            let bind = read_packet(&stream, &mut fds);
            assert_eq!((bind.id, bind.opcode), (registry_id, 0));
            let manager_id = last_uint(&bind.body);

            let create = read_packet(&stream, &mut fds);
            assert_eq!((create.id, create.opcode), (manager_id, 1));
            assert_eq!(fds.len(), 2);
            let context_id = first_uint(&create.body);

            let set_engine = read_packet(&stream, &mut fds);
            assert_eq!((set_engine.id, set_engine.opcode), (context_id, 1));
            let mut cursor = Cursor::new(set_engine.body.as_slice());
            let engine = proto::read_arg_string(&mut cursor).unwrap().unwrap();
            assert_eq!(engine, SANDBOX_ENGINE);

            let set_app = read_packet(&stream, &mut fds);
            assert_eq!((set_app.id, set_app.opcode), (context_id, 2));
            let mut cursor = Cursor::new(set_app.body.as_slice());
            let app_id = proto::read_arg_string(&mut cursor).unwrap().unwrap();
            assert_eq!(app_id, "vm1");

            let set_instance = read_packet(&stream, &mut fds);
            assert_eq!((set_instance.id, set_instance.opcode), (context_id, 3));
            let mut cursor = Cursor::new(set_instance.body.as_slice());
            let instance = proto::read_arg_string(&mut cursor).unwrap().unwrap();
            seen.lock().unwrap().push(instance);

            let commit = read_packet(&stream, &mut fds);
            assert_eq!((commit.id, commit.opcode), (context_id, 4));

            let sync = read_packet(&stream, &mut fds);
            assert_eq!((sync.id, sync.opcode), (1, 0));
            sync_reply(&stream, &sync);

            release_rx.recv().unwrap();
        });

        let context = register(&directory, "vm1").unwrap().expect("context");
        release_tx.send(()).unwrap();
        server.join().unwrap();
        assert_eq!(instance_ids.lock().unwrap().as_slice(), ["vm1"]);
        assert!(context.socket.exists());
        let _ = std::fs::remove_file(&directory);
    }

    #[test]
    fn missing_manager_returns_none() {
        let directory =
            std::env::temp_dir().join(format!("wl-proxy-clip-test-none-{}", process::id()));
        let _ = std::fs::remove_file(&directory);
        let listener = UnixListener::bind(&directory).unwrap();

        let (release_tx, release_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let stream = listener.accept().unwrap().0;
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut fds = Vec::new();
            let registry = read_packet(&stream, &mut fds);
            let registry_id = first_uint(&registry.body);
            write_packet(&stream, registry_id, 0, global_body(7, "wl_compositor", 6));
            let sync = read_packet(&stream, &mut fds);
            sync_reply(&stream, &sync);
            release_rx.recv().unwrap();
        });

        assert!(register(&directory, "vm1").unwrap().is_none());
        release_tx.send(()).unwrap();
        server.join().unwrap();
        let _ = std::fs::remove_file(&directory);
    }

    #[test]
    fn listener_paths_are_unique() {
        assert_ne!(listener_path(), listener_path());
    }

    #[test]
    fn received_listener_fd_can_be_used() {
        let (listener, address) = bind_listener().unwrap();
        let (sender, receiver) = UnixStream::pair().unwrap();
        sender.send_fds(b"x", &[listener.as_raw_fd()]).unwrap();

        let mut buffer = [0; 8];
        let mut fds = [0; 4];
        let (bytes, count) = receiver.recv_fds(&mut buffer, &mut fds).unwrap();
        assert_eq!(bytes, 1);
        assert_eq!(count, 1);

        // SAFETY: the descriptor was received through SCM_RIGHTS and is owned
        // by this process.
        let received = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        let received = UnixListener::from(received);
        received.set_nonblocking(true).unwrap();

        let mut client = UnixStream::connect(&address).unwrap();
        let (mut server, _) = received.accept().unwrap();
        client.write_all(b"hello").unwrap();
        let mut buffer = [0; 5];
        server.read_exact(&mut buffer).unwrap();
        assert_eq!(&buffer, b"hello");
        let _ = std::fs::remove_file(&address);
    }
}
