//! Put the SSH tty in raw mode so keys pass through to the inner PTY.

use std::os::fd::BorrowedFd;

use nix::sys::termios::{cfmakeraw, tcgetattr, tcsetattr, SetArg, Termios};

pub struct RawTty {
    orig: Termios,
}

impl RawTty {
    pub fn enter() -> std::io::Result<Self> {
        let fd = unsafe { BorrowedFd::borrow_raw(libc::STDIN_FILENO) };
        let orig = tcgetattr(fd).map_err(std::io::Error::other)?;
        let mut raw = orig.clone();
        cfmakeraw(&mut raw);
        tcsetattr(fd, SetArg::TCSANOW, &raw).map_err(std::io::Error::other)?;
        Ok(Self { orig })
    }
}

impl Drop for RawTty {
    fn drop(&mut self) {
        let fd = unsafe { BorrowedFd::borrow_raw(libc::STDIN_FILENO) };
        let _ = tcsetattr(fd, SetArg::TCSANOW, &self.orig);
    }
}
