//! F006 分级日志宏 — trace/info/warn/error unified kernel-wide logging.
//! F007 环形日志缓冲 — crash-readable log ring.
//!
//! Layout per line: `[LEVEL ] message` (level padded to 5). Every line is
//! appended to the ring buffer (F007) and mirrored to the serial port; once
//! the framebuffer console is up, lines are also mirrored there.
//!
//! The ring uses atomics (no locks: boot is single threaded) and keeps
//! monotonic sequence numbers so a crash dump can tell whether lines were
//! lost when the ring wrapped.

use core::cell::UnsafeCell;
use core::fmt;
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Off = 5,
}

impl Level {
    pub const fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "trace",
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
            Level::Off => "off",
        }
    }

    pub const fn tag(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => " INFO",
            Level::Warn => " WARN",
            Level::Error => "ERROR",
            Level::Off => " OFF ",
        }
    }

    pub fn parse(s: &str) -> Option<Level> {
        match s {
            "trace" => Some(Level::Trace),
            "debug" => Some(Level::Debug),
            "info" => Some(Level::Info),
            "warn" | "warning" => Some(Level::Warn),
            "error" => Some(Level::Error),
            "off" | "none" => Some(Level::Off),
            _ => None,
        }
    }

    fn from_u8(v: u8) -> Level {
        match v {
            0 => Level::Trace,
            1 => Level::Debug,
            2 => Level::Info,
            3 => Level::Warn,
            4 => Level::Error,
            _ => Level::Off,
        }
    }
}

// ---------------------------------------------------------------------------
// F007 — log ring
// ---------------------------------------------------------------------------

pub const RING_CAPACITY: usize = 64 * 1024;

/// Fixed-size byte ring with monotonic sequence numbers.
pub struct RingLog {
    buf: UnsafeCell<[u8; RING_CAPACITY]>,
    /// Bytes ever written (monotonic).
    written: AtomicU32,
    /// Read position for snapshot consumers.
    read: AtomicU32,
}

unsafe impl Sync for RingLog {}

impl RingLog {
    pub const fn new() -> RingLog {
        RingLog {
            buf: UnsafeCell::new([0; RING_CAPACITY]),
            written: AtomicU32::new(0),
            read: AtomicU32::new(0),
        }
    }

    /// Append raw bytes; when the ring wraps, earlier content is lost but
    /// the `written` counter keeps the truth visible to crash analysis.
    pub fn append(&self, bytes: &[u8]) {
        unsafe {
            let buf = &mut *self.buf.get();
            let mut pos = self.written.load(Ordering::Relaxed);
            for &b in bytes {
                buf[(pos as usize) % RING_CAPACITY] = b;
                pos += 1;
            }
            self.written.store(pos, Ordering::Relaxed);
        }
    }

    pub fn total_written(&self) -> u32 {
        self.written.load(Ordering::Relaxed)
    }

    /// Copy out at most `out.len()` bytes of ring content, oldest first,
    /// starting at the current read cursor. Returns bytes copied.
    /// The cursor is clamped to the oldest still-retained byte, so bytes the
    /// ring has already overwritten after a wrap are never served back.
    pub fn read_snapshot(&self, out: &mut [u8]) -> usize {
        let total = self.written.load(Ordering::Relaxed) as usize;
        let mut read = self.read.load(Ordering::Relaxed) as usize;
        if read > total {
            read = total;
        }
        let oldest = total.saturating_sub(RING_CAPACITY);
        if read < oldest {
            read = oldest;
        }
        let available = total - read;
        let n = available.min(out.len());
        unsafe {
            let buf = &*self.buf.get();
            for i in 0..n {
                out[i] = buf[(read + i) % RING_CAPACITY];
            }
        }
        self.read.store((read + n) as u32, Ordering::Relaxed);
        n
    }

    /// Drain everything written so far (oldest first) into `out`.
    /// Reads past a wrap start at the oldest retained byte — pre-wrap
    /// stale bytes are never served back.
    pub fn drain(&self, out: &mut [u8]) -> usize {
        let total = self.written.load(Ordering::Relaxed) as usize;
        let mut read = self.read.load(Ordering::Relaxed) as usize;
        if read > total {
            return 0;
        }
        let oldest = total.saturating_sub(RING_CAPACITY);
        if read < oldest {
            read = oldest;
        }
        let n = (total - read).min(out.len());
        unsafe {
            let buf = &*self.buf.get();
            for i in 0..n {
                out[i] = buf[(read + i) % RING_CAPACITY];
            }
        }
        self.read.store((read + n) as u32, Ordering::Relaxed);
        n
    }
}

static RING: RingLog = RingLog::new();
static LEVEL: AtomicU32 = AtomicU32::new(Level::Info as u32);

/// Global level control (used by the logger and selftest).
pub fn set_level(level: Level) {
    LEVEL.store(level as u32, Ordering::Relaxed);
}

pub fn current_level() -> Level {
    Level::from_u8(LEVEL.load(Ordering::Relaxed) as u8)
}

/// Apply `log=<level>` from the kernel command line (F006 + F017).
/// Unknown values are ignored (default level stays).
pub fn apply_cmdline(cmdline: &str) {
    for token in cmdline.split_whitespace() {
        if let Some(value) = token.strip_prefix("log=") {
            if let Some(level) = Level::parse(value) {
                set_level(level);
            }
        }
    }
}

/// Install the serial port (call once at the very start of boot).
pub fn early_init() {
    crate::serial::init();
}

// ---------------------------------------------------------------------------
// Emit path
// ---------------------------------------------------------------------------

/// Write one formatted log line to ring + serial (+ console when ready).
pub fn log(level: Level, args: fmt::Arguments) {
    if level < current_level() {
        return;
    }
    let mut line = [0u8; 512];

    // `[LEVEL ] `
    line[0] = b'[';
    line[1..6].copy_from_slice(level.tag().as_bytes());
    line[6] = b']';
    line[7] = b' ';
    let mut len = 8usize;

    let mut w = LineWriter { buf: &mut line, pos: &mut len };
    fmt::write(&mut w, args).ok();
    if len < line.len() {
        line[len] = b'\n';
        len += 1;
    }

    RING.append(&line[..len]);
    crate::serial::write_bytes(&line[..len]);
    crate::console::mirror_bytes(&line[..len]);
}

struct LineWriter<'a> {
    buf: &'a mut [u8; 512],
    pos: &'a mut usize,
}

impl fmt::Write for LineWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            if *self.pos >= self.buf.len() {
                return Err(fmt::Error); // truncate long lines instead of overflowing
            }
            self.buf[*self.pos] = b;
            *self.pos += 1;
        }
        Ok(())
    }
}

/// Direct access to the crash-readable ring (F007).
pub fn ring() -> &'static RingLog {
    &RING
}

/// Unified kernel-wide logging macros (F006).
#[macro_export]
macro_rules! ktrace {
    ($($arg:tt)*) => {
        $crate::logger::log($crate::logger::Level::Trace, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! kdebug {
    ($($arg:tt)*) => {
        $crate::logger::log($crate::logger::Level::Debug, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! kinfo {
    ($($arg:tt)*) => {
        $crate::logger::log($crate::logger::Level::Info, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! kwarn {
    ($($arg:tt)*) => {
        $crate::logger::log($crate::logger::Level::Warn, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! kerror {
    ($($arg:tt)*) => {
        $crate::logger::log($crate::logger::Level::Error, format_args!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests mutate the global LEVEL/RING — serialize them.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn level_parse_and_order() {
        let _g = SERIAL.lock().unwrap();
        assert!(Level::Trace < Level::Debug);
        assert!(Level::Debug < Level::Info);
        assert!(Level::Info < Level::Warn);
        assert!(Level::Warn < Level::Error);
        assert_eq!(Level::parse("trace"), Some(Level::Trace));
        assert_eq!(Level::parse("warning"), Some(Level::Warn));
        assert_eq!(Level::parse("off"), Some(Level::Off));
        assert_eq!(Level::parse("verbose"), None);
        assert_eq!(Level::Error.tag(), "ERROR");
        assert_eq!(Level::Info.tag(), " INFO");
    }

    #[test]
    fn cmdline_level_filter() {
        let _g = SERIAL.lock().unwrap();
        apply_cmdline("root=varix log=trace kaslr=off");
        assert_eq!(current_level(), Level::Trace);
        apply_cmdline("log=warn");
        assert_eq!(current_level(), Level::Warn);
        apply_cmdline("log=bogus");
        assert_eq!(current_level(), Level::Warn); // unknown values ignored
        set_level(Level::Info);
    }

    #[test]
    fn ring_wraparound_keeps_newest() {
        // Independent ring (not the global one) to keep tests isolated.
        let ring = RingLog::new();
        let chunk = [b'x'; 1000];
        for _ in 0..((RING_CAPACITY / 1000) + 3) {
            ring.append(&chunk);
        }
        let total = ring.total_written() as usize;
        assert_eq!(total % 1000, 0);
        assert!(total > RING_CAPACITY); // wrapped at least once
        let mut out = [0u8; 64];
        let n = ring.drain(&mut out);
        assert_eq!(n, 64);
        // Oldest retained byte after wrap = total - capacity; its position
        // in the ring is (total - capacity) % capacity, and all bytes are
        // 'x', so we verify the count semantics instead:
        assert_eq!(out[0], b'x');
        // After draining 64 retained bytes, capacity - 64 remain readable —
        // the pre-wrap 2464 stale bytes are never served back.
        let mut big = [0u8; RING_CAPACITY + 100];
        let drained = ring.drain(&mut big);
        assert_eq!(drained, RING_CAPACITY - 64);
        // Everything read now — the next drain returns 0.
        let drained2 = ring.drain(&mut big);
        assert_eq!(drained2, 0);
    }

    #[test]
    fn ring_preserves_order() {
        let ring = RingLog::new();
        ring.append(b"hello ");
        ring.append(b"world");
        let mut out = [0u8; 32];
        let n = ring.drain(&mut out);
        assert_eq!(&out[..n], b"hello world");
    }

    #[test]
    fn log_respects_level() {
        let _g = SERIAL.lock().unwrap();
        set_level(Level::Warn);
        // Discard whatever earlier tests left in the global ring so the
        // drain below only sees this test's output.
        let mut sink = [0u8; RING_CAPACITY];
        let _ = RING.drain(&mut sink);
        let before = RING.total_written();
        log(Level::Trace, format_args!("ignored"));
        log(Level::Debug, format_args!("ignored too"));
        assert_eq!(RING.total_written(), before);
        log(Level::Error, format_args!("boom {}", 42));
        assert!(RING.total_written() > before);
        let mut out = [0u8; 128];
        let n = RING.drain(&mut out);
        let s = core::str::from_utf8(&out[..n]).unwrap_or("");
        assert!(s.contains("[ERROR] boom 42"), "got: {s}");
        set_level(Level::Info);
    }

    #[test]
    fn long_line_truncates_without_panic() {
        let _g = SERIAL.lock().unwrap();
        let before = RING.total_written();
        let long: String = std::format!("{}", "a".repeat(1000));
        log(Level::Error, format_args!("{}", long));
        let written = RING.total_written() - before;
        assert!(written as usize <= 512, "written={written}");
    }
}
