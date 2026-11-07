//! Button and wheel events from the kernel (`/dev/input/event*`).
//!
//! On the F20, event0 is the rotary encoder (reported as left/right keys) and
//! event1 the buttons; every device present is opened so other revisions
//! work too. Records are read raw: a 32-bit kernel writes 16-byte events
//! (timeval, type, code, value) whatever the C library's time_t is.
use std::ffi::CString;
use std::time::Duration;

const EV_KEY: u16 = 1;
const RECORD: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct Raw {
    pub code: u16,
    /// 1 pressed, 0 released, 2 kernel autorepeat.
    pub value: i32,
}

pub struct Input {
    fds: Vec<libc::c_int>,
    pending: Vec<u8>,
}

impl Input {
    pub fn open() -> Input {
        let mut fds = Vec::new();
        for i in 0..8 {
            let p = CString::new(format!("/dev/input/event{i}")).unwrap();
            // SAFETY: open(2) on a C string.
            let fd = unsafe { libc::open(p.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC) };
            if fd >= 0 {
                fds.push(fd);
            }
        }
        Input {
            fds,
            pending: Vec::new(),
        }
    }

    pub fn device_count(&self) -> usize {
        self.fds.len()
    }

    /// Blocks up to `timeout` and returns the key events that arrived.
    pub fn wait(&mut self, timeout: Duration) -> Vec<Raw> {
        let mut out = Vec::new();
        if self.fds.is_empty() {
            std::thread::sleep(timeout);
            return out;
        }
        let mut polls: Vec<libc::pollfd> = self
            .fds
            .iter()
            .map(|&fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        let ms = timeout.as_millis().min(i32::MAX as u128) as libc::c_int;
        // SAFETY: `polls` is a valid array of pollfd for its length.
        let n = unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as _, ms) };
        if n <= 0 {
            return out;
        }
        let mut dead = Vec::new();
        for (i, p) in polls.iter().enumerate() {
            if p.revents & libc::POLLIN != 0 {
                let mut buf = [0u8; RECORD * 32];
                loop {
                    // SAFETY: reading into a stack buffer of the stated size.
                    let r = unsafe { libc::read(p.fd, buf.as_mut_ptr() as *mut _, buf.len()) };
                    if r <= 0 {
                        break;
                    }
                    self.pending.extend_from_slice(&buf[..r as usize]);
                    for rec in self.pending.chunks_exact(RECORD) {
                        let ty = u16::from_le_bytes([rec[8], rec[9]]);
                        let code = u16::from_le_bytes([rec[10], rec[11]]);
                        let value = i32::from_le_bytes([rec[12], rec[13], rec[14], rec[15]]);
                        if ty == EV_KEY {
                            out.push(Raw { code, value });
                        }
                    }
                    let rem = self.pending.len() % RECORD;
                    let keep = self.pending[self.pending.len() - rem..].to_vec();
                    self.pending = keep;
                }
            } else if p.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                dead.push(i);
            }
        }
        for i in dead.into_iter().rev() {
            // SAFETY: closing an fd we opened.
            unsafe { libc::close(self.fds[i]) };
            self.fds.remove(i);
        }
        out
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        for &fd in &self.fds {
            // SAFETY: closing fds we opened.
            unsafe { libc::close(fd) };
        }
    }
}
