//! Evidence for crashes that leave no log line: a step marker the code
//! updates as it goes, and a handler for fatal signals that writes the
//! signal number and the current step to boot.log with nothing but write(2).
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

pub const STEPS: [&str; 14] = [
    "start",
    "config",
    "framebuffer open",
    "terminal",
    "input devices open",
    "backlight probe",
    "mixer open",
    "mixer load",
    "output port switch",
    "volume",
    "audio open + library index",
    "first frame",
    "running",
    "exit",
];

static STEP: AtomicUsize = AtomicUsize::new(0);
static LOG_FD: AtomicI32 = AtomicI32::new(-1);

pub fn step(i: usize) {
    STEP.store(i, Ordering::SeqCst);
}

extern "C" fn on_signal(sig: libc::c_int) {
    let fd = LOG_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        let mut buf = [0u8; 96];
        let mut n = 0;
        let mut put = |s: &[u8]| {
            for &b in s {
                if n < buf.len() {
                    buf[n] = b;
                    n += 1;
                }
            }
        };
        put(b"FATAL: signal ");
        let mut digits = [0u8; 4];
        let mut v = sig.max(0) as usize;
        let mut d = 0;
        loop {
            digits[d] = b'0' + (v % 10) as u8;
            d += 1;
            v /= 10;
            if v == 0 || d == digits.len() {
                break;
            }
        }
        for i in (0..d).rev() {
            put(&digits[i..i + 1]);
        }
        put(b" during step: ");
        put(STEPS[STEP.load(Ordering::SeqCst).min(STEPS.len() - 1)].as_bytes());
        put(b"\n");
        // SAFETY: write(2) is async-signal-safe; the buffer is on our stack.
        unsafe { libc::write(fd, buf.as_ptr() as *const _, n) };
    }
    // SAFETY: _exit is async-signal-safe.
    unsafe { libc::_exit(128 + sig) };
}

/// Installs the handler; `fd` must stay open for the life of the process.
pub fn install(fd: i32) {
    LOG_FD.store(fd, Ordering::SeqCst);
    for sig in [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGABRT, libc::SIGTRAP] {
        // SAFETY: installing a plain handler function.
        unsafe { libc::signal(sig, on_signal as libc::sighandler_t) };
    }
}
