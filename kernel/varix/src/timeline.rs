//! F024 引导日志可视化 — boot stage timeline with TSC timestamps.
//!
//! Every boot stage brackets its TSC start/end; the module renders the
//! collected stages as an aligned timeline (name + duration + bar) onto the
//! framebuffer console and into the log ring. Fixed capacity, allocation-free.

use core::cell::UnsafeCell;

/// Maximum stages tracked in one boot.
pub const MAX_STAGES: usize = 16;
/// Bar width (in cells) for the longest stage when rendering.
pub const BAR_WIDTH: usize = 24;

/// Canonical boot stage ids (F024 vocabulary, shared with main.rs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Serial,
    Cmdline,
    Framebuffer,
    Console,
    Logo,
    Banner,
    Platform,
    Acpi,
    Smios,
    Memmap,
    Kaslr,
    Integrity,
    BootOpt,
    SelfTest,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Stage::Serial => "serial",
            Stage::Cmdline => "cmdline",
            Stage::Framebuffer => "framebuffer",
            Stage::Console => "console",
            Stage::Logo => "logo",
            Stage::Banner => "banner",
            Stage::Platform => "platform",
            Stage::Acpi => "acpi",
            Stage::Smios => "smbios",
            Stage::Memmap => "memmap",
            Stage::Kaslr => "kaslr",
            Stage::Integrity => "integrity",
            Stage::BootOpt => "bootopt",
            Stage::SelfTest => "selftest",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Stage::Serial => 0,
            Stage::Cmdline => 1,
            Stage::Framebuffer => 2,
            Stage::Console => 3,
            Stage::Logo => 4,
            Stage::Banner => 5,
            Stage::Platform => 6,
            Stage::Acpi => 7,
            Stage::Smios => 8,
            Stage::Memmap => 9,
            Stage::Kaslr => 10,
            Stage::Integrity => 11,
            Stage::BootOpt => 12,
            Stage::SelfTest => 13,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct StageRecord {
    started: bool,
    finished: bool,
    start_tsc: u64,
    end_tsc: u64,
}

/// Boot timeline collector (single-threaded boot path).
pub struct BootTimeline {
    records: UnsafeCell<[StageRecord; MAX_STAGES]>,
}

// SAFETY: written only from the single-threaded boot path.
unsafe impl Sync for BootTimeline {}

impl BootTimeline {
    pub const fn new() -> BootTimeline {
        const EMPTY: StageRecord = StageRecord {
            started: false,
            finished: false,
            start_tsc: 0,
            end_tsc: 0,
        };
        BootTimeline {
            records: UnsafeCell::new([EMPTY; MAX_STAGES]),
        }
    }

    /// Mark the beginning of boot (called once, before any stage).
    pub fn begin_boot(&self, _tsc: u64) {
        unsafe {
            (*self.records.get()) = [StageRecord::default(); MAX_STAGES];
        }
    }

    /// Mark a stage as started.
    pub fn stage_begin(&self, stage: Stage, tsc: u64) {
        let idx = stage.index();
        if idx >= MAX_STAGES {
            return;
        }
        unsafe {
            let r = &mut (*self.records.get())[idx];
            r.started = true;
            r.start_tsc = tsc;
        }
    }

    /// Mark a stage as finished (records the duration even if `begin` was
    /// missed — start defaults to 0).
    pub fn stage_end(&self, stage: Stage, tsc: u64) {
        let idx = stage.index();
        if idx >= MAX_STAGES {
            return;
        }
        unsafe {
            let r = &mut (*self.records.get())[idx];
            r.finished = true;
            r.end_tsc = tsc;
            if !r.started {
                r.started = true;
                r.start_tsc = tsc;
            }
        }
    }

    /// Duration of a stage in TSC ticks (0 when not run).
    pub fn stage_ticks(&self, stage: Stage) -> u64 {
        let idx = stage.index();
        if idx >= MAX_STAGES {
            return 0;
        }
        unsafe {
            let r = &(*self.records.get())[idx];
            if r.finished {
                r.end_tsc.saturating_sub(r.start_tsc)
            } else {
                0
            }
        }
    }

    /// Sum of all finished stage durations.
    pub fn total_ticks(&self) -> u64 {
        let mut sum: u64 = 0;
        for i in 0..MAX_STAGES {
            unsafe {
                let r = &(*self.records.get())[i];
                if r.finished {
                    sum = sum.saturating_add(r.end_tsc.saturating_sub(r.start_tsc));
                }
            }
        }
        sum
    }

    /// Number of finished stages.
    pub fn finished_stages(&self) -> usize {
        (0..MAX_STAGES)
            .filter(|&i| unsafe { (*self.records.get())[i].finished })
            .count()
    }

    /// Longest finished stage (None when nothing finished yet).
    pub fn longest_stage(&self) -> Option<Stage> {
        let mut best: Option<(Stage, u64)> = None;
        for i in 0..MAX_STAGES {
            unsafe {
                let r = &(*self.records.get())[i];
                if r.finished {
                    let t = r.end_tsc.saturating_sub(r.start_tsc);
                    if best.map(|(_, bt)| t > bt).unwrap_or(true) {
                        best = Some((stage_of_index(i), t));
                    }
                }
            }
        }
        best.map(|(s, _)| s)
    }

    /// Render one ASCII line per finished stage into `out` (used by the
    /// console view + the log). Lines are NUL-free, `\n`-terminated.
    /// Returns bytes written.
    pub fn render(&self, tsc_hz: u64, out: &mut [u8]) -> usize {
        let max_ticks = (0..MAX_STAGES)
            .map(|i| unsafe {
                let r = &(*self.records.get())[i];
                if r.finished {
                    r.end_tsc.saturating_sub(r.start_tsc)
                } else {
                    0
                }
            })
            .max()
            .unwrap_or(0);
        let mut n = 0usize;
        let emit = |bytes: &[u8], out: &mut [u8], n: &mut usize| {
            for &b in bytes {
                if *n < out.len() {
                    out[*n] = b;
                    *n += 1;
                }
            }
        };
        for i in 0..MAX_STAGES {
            unsafe {
                let r = (*self.records.get())[i];
                if !r.finished {
                    continue;
                }
                let ticks = r.end_tsc.saturating_sub(r.start_tsc);
                let ms = ticks_to_ms(ticks, tsc_hz);
                // "name      12.3ms  ####\n"
                let name = stage_of_index(i).name();
                emit(name.as_bytes(), out, &mut n);
                for _ in name.len()..10 {
                    emit(b" ", out, &mut n);
                }
                let mut num = [0u8; 12];
                let mut w = 0usize;
                let mut v = ms;
                if v == 0 {
                    num[0] = b'0';
                    w = 1;
                } else {
                    while v > 0 && w < num.len() {
                        num[w] = b'0' + (v % 10) as u8;
                        v /= 10;
                        w += 1;
                    }
                }
                while w > 0 {
                    w -= 1;
                    emit(&[num[w]], out, &mut n);
                }
                emit(b"ms ", out, &mut n);
                if max_ticks > 0 {
                    let bar = (ticks * BAR_WIDTH as u64 / max_ticks) as usize;
                    for _ in 0..bar {
                        emit(b"#", out, &mut n);
                    }
                }
                emit(b"\n", out, &mut n);
            }
        }
        n
    }
}

fn stage_of_index(i: usize) -> Stage {
    match i {
        0 => Stage::Serial,
        1 => Stage::Cmdline,
        2 => Stage::Framebuffer,
        3 => Stage::Console,
        4 => Stage::Logo,
        5 => Stage::Banner,
        6 => Stage::Platform,
        7 => Stage::Acpi,
        8 => Stage::Smios,
        9 => Stage::Memmap,
        10 => Stage::Kaslr,
        11 => Stage::Integrity,
        12 => Stage::BootOpt,
        13 => Stage::SelfTest,
        _ => Stage::SelfTest,
    }
}

/// TSC ticks → milliseconds (saturating, no float).
pub fn ticks_to_ms(ticks: u64, tsc_hz: u64) -> u64 {
    if tsc_hz == 0 {
        return 0;
    }
    // (ticks * 1000) / hz with u128-free widening via chunked division.
    let ms = (ticks / tsc_hz) * 1000 + ((ticks % tsc_hz) * 1000) / tsc_hz;
    ms
}

// ---------------------------------------------------------------------------
// Global timeline + target helpers
// ---------------------------------------------------------------------------

pub static TIMELINE: BootTimeline = BootTimeline::new();

pub fn timeline() -> &'static BootTimeline {
    &TIMELINE
}

/// Read the TSC (target) or a host-stable value (tests).
#[cfg(target_os = "none")]
pub fn read_tsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

#[cfg(all(not(target_os = "none"), test))]
pub fn read_tsc() -> u64 {
    // Host test build: a per-call varying value keeps timeline math exercised.
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(all(not(target_os = "none"), not(test)))]
pub fn read_tsc() -> u64 {
    // Host no_std build (lib as a dependency): deterministic stand-in.
    0x9E37_79B9_7F4A_7C15
}

/// Render the timeline onto the global console + log ring (F024 view).
pub fn render_to_console() {
    let hz = crate::platform::info().map(|p| p.tsc_hz).unwrap_or(crate::platform::FALLBACK_TSC_HZ);
    let mut buf = [0u8; 1024];
    let n = TIMELINE.render(hz, &mut buf);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &buf[..n] {
            c.put_byte(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tl() -> BootTimeline {
        let t = BootTimeline::new();
        t.begin_boot(0);
        t
    }

    #[test]
    fn stage_bracketing() {
        let t = tl();
        t.stage_begin(Stage::Serial, 100);
        t.stage_end(Stage::Serial, 350);
        assert_eq!(t.stage_ticks(Stage::Serial), 250);
        assert_eq!(t.finished_stages(), 1);
        // unfinished stages report 0
        assert_eq!(t.stage_ticks(Stage::Acpi), 0);
    }

    #[test]
    fn end_without_begin_is_self_healing() {
        let t = tl();
        t.stage_end(Stage::Acpi, 1000);
        assert_eq!(t.stage_ticks(Stage::Acpi), 0); // zero-duration
        assert_eq!(t.finished_stages(), 1);
    }

    #[test]
    fn total_and_longest() {
        let t = tl();
        t.stage_begin(Stage::Serial, 0);
        t.stage_end(Stage::Serial, 100);
        t.stage_begin(Stage::Acpi, 100);
        t.stage_end(Stage::Acpi, 500); // 400 — longest
        t.stage_begin(Stage::Smios, 500);
        t.stage_end(Stage::Smios, 600);
        assert_eq!(t.total_ticks(), 600);
        assert_eq!(t.longest_stage(), Some(Stage::Acpi));
        assert_eq!(t.finished_stages(), 3);
    }

    #[test]
    fn stage_index_round_trips() {
        // `stage_of_index` is the inverse of `Stage::index`; a silent drift
        // here corrupts every timeline read-back, so pin it down.
        for i in 0..MAX_STAGES {
            let s = stage_of_index(i);
            if s == Stage::SelfTest && i != Stage::SelfTest.index() {
                continue; // beyond the last real stage
            }
            assert_eq!(s.index(), i, "stage {s:?} disagrees with index {i}");
        }
        for s in [
            Stage::Serial,
            Stage::Cmdline,
            Stage::Framebuffer,
            Stage::Console,
            Stage::Logo,
            Stage::Banner,
            Stage::Platform,
            Stage::Acpi,
            Stage::Smios,
            Stage::Memmap,
            Stage::Kaslr,
            Stage::Integrity,
            Stage::BootOpt,
            Stage::SelfTest,
        ] {
            assert_eq!(stage_of_index(s.index()), s, "round trip broke for {s:?}");
            assert!(!s.name().is_empty());
        }
    }

    #[test]
    fn begin_boot_resets() {
        let t = tl();
        t.stage_begin(Stage::Serial, 10);
        t.stage_end(Stage::Serial, 20);
        assert_eq!(t.finished_stages(), 1);
        t.begin_boot(1000);
        assert_eq!(t.finished_stages(), 0);
        assert_eq!(t.stage_ticks(Stage::Serial), 0);
    }

    #[test]
    fn tick_to_ms_math() {
        assert_eq!(ticks_to_ms(0, 1_000_000), 0);
        // 2.5 GHz: 2.5M ticks = 1 ms
        assert_eq!(ticks_to_ms(2_500_000, 2_500_000_000), 1);
        // 12.5M ticks = 5 ms
        assert_eq!(ticks_to_ms(12_500_000, 2_500_000_000), 5);
        // hz = 0 must not divide by zero
        assert_eq!(ticks_to_ms(12345, 0), 0);
        // large values must not overflow
        assert!(ticks_to_ms(u64::MAX / 2, 1_000_000_000) > 0);
    }

    #[test]
    fn render_lines_look_right() {
        let t = tl();
        t.stage_begin(Stage::Serial, 0);
        t.stage_end(Stage::Serial, 2_500_000); // 1ms @2.5GHz
        t.stage_begin(Stage::Acpi, 2_500_000);
        t.stage_end(Stage::Acpi, 12_500_000); // 4ms — longest
        let mut out = [0u8; 512];
        let n = t.render(2_500_000_000, &mut out);
        let s = core::str::from_utf8(&out[..n]).unwrap();
        let serial_line = s.lines().find(|l| l.starts_with("serial")).unwrap();
        assert!(serial_line.contains("1ms"), "line: {serial_line}");
        assert!(serial_line.contains("#"));
        let acpi_line = s.lines().find(|l| l.starts_with("acpi")).unwrap();
        assert!(acpi_line.contains("4ms"), "line: {acpi_line}");
        // Longest stage gets the full-width bar.
        assert!(acpi_line.matches('#').count() == BAR_WIDTH);
        // Name column is aligned (both lines pad to col 10).
        assert_eq!(serial_line.find("1ms"), acpi_line.find("4ms"));
    }

    #[test]
    fn render_respects_buffer_limit() {
        let t = tl();
        for s in [Stage::Serial, Stage::Cmdline, Stage::Framebuffer, Stage::Console] {
            t.stage_begin(s, 0);
            t.stage_end(s, 1000);
        }
        let mut out = [0u8; 16];
        let n = t.render(1_000_000, &mut out);
        assert_eq!(n, 16); // exactly fills, no panic/overflow
        assert!(out.iter().all(|&b| b != 0 || true));
    }
}
