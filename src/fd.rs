use std::{
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::net::UnixStream,
    },
    time::Duration,
};

pub(crate) fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: `fds` points to two valid file descriptor slots.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `pipe2` initialized both descriptors and passed ownership to us.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

pub(crate) fn readable(fd: RawFd, timeout: Duration) -> bool {
    let mut poll_fd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let timeout_ms = libc::c_int::try_from(timeout.as_millis()).unwrap_or(libc::c_int::MAX);
    // SAFETY: `poll_fd` is valid for the duration of the call.
    unsafe { libc::poll(&mut poll_fd, 1, timeout_ms) > 0 }
}

pub(crate) fn pending_bytes(socket: &UnixStream) -> libc::c_int {
    let mut bytes = 0;
    // SAFETY: `FIONREAD` writes an integer through the provided pointer.
    unsafe { libc::ioctl(socket.as_raw_fd(), libc::FIONREAD, &mut bytes) };
    bytes
}
