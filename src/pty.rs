//! Pseudo-terminal wrapper: spawns a child on a PTY, forwards keystrokes
//! untouched, and passes the child's output through a `Filter` before it
//! reaches the real terminal.

use std::ffi::CString;
use std::io;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};

use nix::errno::Errno;
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::pty::{ForkptyResult, Winsize, forkpty};
use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
use nix::sys::termios::{self, SetArg, Termios};
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd;

/// Transforms the child's output stream before it is shown.
pub trait Filter {
    /// Consume `input`, appending whatever may be emitted now to `out`.
    fn feed(&mut self, input: &[u8], out: &mut Vec<u8>);
    /// Emit anything still held back.
    fn flush(&mut self, out: &mut Vec<u8>);
    /// True when `flush` would emit something.
    fn pending(&self) -> bool;
}

/// Passes output through unchanged.
pub struct Passthrough;

impl Filter for Passthrough {
    fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) {
        out.extend_from_slice(input);
    }
    fn flush(&mut self, _out: &mut Vec<u8>) {}
    fn pending(&self) -> bool {
        false
    }
}

static WINCH: AtomicBool = AtomicBool::new(false);

extern "C" fn on_winch(_: libc::c_int) {
    WINCH.store(true, Ordering::SeqCst);
}

nix::ioctl_read_bad!(tiocgwinsz, libc::TIOCGWINSZ, Winsize);
nix::ioctl_write_ptr_bad!(tiocswinsz, libc::TIOCSWINSZ, Winsize);

fn current_winsize() -> Winsize {
    let mut ws = Winsize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
    // Safety: TIOCGWINSZ writes a Winsize into the pointer we pass.
    let _ = unsafe { tiocgwinsz(libc::STDOUT_FILENO, &mut ws) };
    ws
}

/// Puts stdin into raw mode and restores the original settings on drop.
struct RawMode {
    saved: Termios,
}

impl RawMode {
    fn enable() -> Option<Self> {
        let stdin = io::stdin();
        let saved = termios::tcgetattr(stdin.as_fd()).ok()?;
        let mut raw = saved.clone();
        termios::cfmakeraw(&mut raw);
        termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &raw).ok()?;
        Some(Self { saved })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = termios::tcsetattr(io::stdin().as_fd(), SetArg::TCSANOW, &self.saved);
    }
}

fn write_all<F: AsFd>(fd: &F, mut buf: &[u8]) -> nix::Result<()> {
    while !buf.is_empty() {
        match unistd::write(fd, buf) {
            Ok(n) => buf = &buf[n..],
            Err(Errno::EINTR) | Err(Errno::EAGAIN) => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Spawns `argv` on a fresh PTY and shuttles bytes until it exits.
/// Returns the child's exit code.
pub fn run(argv: &[String], filter: &mut dyn Filter) -> Result<i32, Box<dyn std::error::Error>> {
    let ws = current_winsize();
    let term = termios::tcgetattr(io::stdin().as_fd()).ok();

    // Safety: the child branch only calls async-signal-safe functions before exec.
    let (child, master) = match unsafe { forkpty(&ws, term.as_ref()) }? {
        ForkptyResult::Child => exec_child(argv),
        ForkptyResult::Parent { child, master } => (child, master),
    };

    let _raw = RawMode::enable();
    install_winch()?;

    let code = shuttle(&master, filter);
    drop(master);

    let status = loop {
        match waitpid(child, None) {
            Ok(WaitStatus::Exited(_, c)) => break c,
            Ok(WaitStatus::Signaled(_, sig, _)) => break 128 + sig as i32,
            Ok(_) | Err(Errno::EINTR) => continue,
            Err(_) => break 1,
        }
    };
    code?;
    Ok(status)
}

fn exec_child(argv: &[String]) -> ! {
    let cargs: Vec<CString> = argv
        .iter()
        .map(|a| CString::new(a.as_str()).unwrap_or_default())
        .collect();
    // Safety: plain setenv before exec; single-threaded child.
    unsafe { libc::setenv(c"ANONTERM".as_ptr(), c"1".as_ptr(), 1) };
    let _ = unistd::execvp(&cargs[0], &cargs);
    // Only reached if exec failed; avoid running parent destructors.
    unsafe { libc::_exit(127) }
}

fn install_winch() -> nix::Result<()> {
    let action = SigAction::new(SigHandler::Handler(on_winch), SaFlags::empty(), SigSet::empty());
    // Safety: handler only touches an atomic.
    unsafe { sigaction(Signal::SIGWINCH, &action) }.map(|_| ())
}

fn resize(master: &OwnedFd) {
    let ws = current_winsize();
    // Safety: TIOCSWINSZ reads a Winsize from the pointer we pass.
    let _ = unsafe { tiocswinsz(master.as_fd().as_raw_fd(), &ws) };
}


fn shuttle(master: &OwnedFd, filter: &mut dyn Filter) -> nix::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut buf = [0u8; 65536];
    let mut out = Vec::with_capacity(65536);
    let mut stdin_open = true;

    loop {
        if WINCH.swap(false, Ordering::SeqCst) {
            resize(master);
        }

        let mut fds = Vec::with_capacity(2);
        fds.push(PollFd::new(master.as_fd(), PollFlags::POLLIN));
        if stdin_open {
            fds.push(PollFd::new(stdin.as_fd(), PollFlags::POLLIN));
        }
        let timeout = if filter.pending() { PollTimeout::from(5u8) } else { PollTimeout::NONE };

        match poll(&mut fds, timeout) {
            Err(Errno::EINTR) => continue,
            Err(e) => return Err(e),
            Ok(0) => {
                filter.flush(&mut out);
                write_all(&stdout, &out)?;
                out.clear();
                continue;
            }
            Ok(_) => {}
        }

        let master_ready = fds[0]
            .revents()
            .is_some_and(|r| r.intersects(PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR));
        let stdin_ready = fds
            .get(1)
            .and_then(|f| f.revents())
            .is_some_and(|r| r.intersects(PollFlags::POLLIN | PollFlags::POLLHUP));

        if stdin_ready {
            match unistd::read(&stdin, &mut buf) {
                Ok(0) | Err(Errno::EIO) => stdin_open = false,
                Ok(n) => write_all(master, &buf[..n])?,
                Err(Errno::EINTR) | Err(Errno::EAGAIN) => {}
                Err(e) => return Err(e),
            }
        }

        if master_ready {
            match unistd::read(master, &mut buf) {
                Ok(0) | Err(Errno::EIO) => break,
                Ok(n) => {
                    filter.feed(&buf[..n], &mut out);
                    write_all(&stdout, &out)?;
                    out.clear();
                }
                Err(Errno::EINTR) | Err(Errno::EAGAIN) => {}
                Err(e) => return Err(e),
            }
        }
    }

    filter.flush(&mut out);
    write_all(&stdout, &out)?;
    Ok(())
}
