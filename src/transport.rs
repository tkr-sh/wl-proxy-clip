use {
    crate::{
        Res,
        clipboard::prepare,
        config::IDLE_POLL_INTERVAL,
        fd::{pending_bytes, readable},
        filter::{self, Verdict},
        state::Connection,
    },
    std::{
        io::{self, Read, Write},
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
            unix::net::UnixStream,
        },
    },
    uds::UnixStreamExt,
    wlproxy::proto::{self, Packet},
};

const MAX_FDS_PER_MESSAGE: usize = 28;

struct FdReader<'a> {
    socket: &'a UnixStream,
    fds:    &'a mut Vec<RawFd>,
}

impl Read for FdReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut fd_buffer = [0; MAX_FDS_PER_MESSAGE];
        let (bytes, fd_count) = self.socket.recv_fds(buffer, &mut fd_buffer)?;
        if let Some(received) = fd_buffer.get(..fd_count) {
            self.fds.extend(received);
        }
        Ok(bytes)
    }
}

struct FdWriter<'a> {
    socket: &'a UnixStream,
    fds:    Vec<RawFd>,
}

impl Write for FdWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let fds = std::mem::take(&mut self.fds);
        if fds.is_empty() {
            (&*self.socket).write(buffer)
        } else {
            self.socket.send_fds(buffer, &fds)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Send a message together with any file descriptors received so far, then
/// close our copies.
///
/// File descriptors travel with the next message, as in wlproxy: libwayland
/// queues them in order without tying them to a specific message. A message
/// carrying an fd is therefore never dropped.
fn send(socket: &UnixStream, packet: &Packet, fds: &mut Vec<RawFd>) -> Res<()> {
    proto::write_packet(
        &mut FdWriter {
            socket,
            fds: fds.clone(),
        },
        packet,
    )?;
    for fd in fds.drain(..) {
        // SAFETY: the fd was received through SCM_RIGHTS and is owned by us.
        drop(unsafe { OwnedFd::from_raw_fd(fd) });
    }
    Ok(())
}

pub(crate) fn forward_requests(
    client: &UnixStream,
    compositor: &UnixStream,
    connection: &Connection,
) -> Res<()> {
    let mut pending_fds = Vec::new();

    while let Some(packet) = proto::read_packet(&mut FdReader {
        socket: client,
        fds:    &mut pending_fds,
    })? {
        let verdict = {
            let mut state = connection.state();
            filter::request(&mut state, &packet)?
        };
        if verdict == Verdict::Forward {
            send(compositor, &packet, &mut pending_fds)?;
        }
    }
    Ok(())
}

pub(crate) fn forward_events(
    compositor: &UnixStream,
    client: &UnixStream,
    connection: &Connection,
) -> Res<()> {
    let mut pending_fds = Vec::new();

    loop {
        // Inject only when the compositor has nothing in flight, so injected
        // file descriptors arrive after all of its own
        if pending_bytes(compositor) == 0 && pending_fds.is_empty() {
            for injection in prepare(connection) {
                let fds = injection.fd.iter().map(AsRawFd::as_raw_fd).collect();
                proto::write_packet(
                    &mut FdWriter {
                        socket: client,
                        fds,
                    },
                    &injection.packet,
                )?;
            }
            if !readable(compositor.as_raw_fd(), IDLE_POLL_INTERVAL) {
                continue;
            }
        }

        let Some(packet) = proto::read_packet(&mut FdReader {
            socket: compositor,
            fds:    &mut pending_fds,
        })?
        else {
            return Ok(());
        };

        let verdict = {
            let mut state = connection.state();
            filter::event(&mut state, &packet)?
        };
        if verdict == Verdict::Forward {
            send(client, &packet, &mut pending_fds)?;
        }
    }
}
