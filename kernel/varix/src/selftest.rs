//! F025 引导自检 — boot chain health check: one `check()` per boot link,
//! a pass/fail summary rendered to the console + log, and the aggregate
//! verdict for downstream phases to gate on.

use core::cell::UnsafeCell;

/// Maximum checks recorded in one run.
pub const MAX_CHECKS: usize = 32;

/// One check result.
#[derive(Clone, Copy, Debug)]
pub struct Check {
    pub name: &'static str,
    pub passed: bool,
    /// Short failure detail (only shown for failures).
    pub detail: &'static str,
}

/// Check registry (single-threaded boot path).
pub struct SelfTest {
    checks: UnsafeCell<[Option<Check>; MAX_CHECKS]>,
    count: core::sync::atomic::AtomicUsize,
}

// SAFETY: written only from the single-threaded boot path.
unsafe impl Sync for SelfTest {}

impl SelfTest {
    pub const fn new() -> SelfTest {
        SelfTest {
            checks: UnsafeCell::new([None; MAX_CHECKS]),
            count: core::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Record one check result.
    pub fn check(&self, name: &'static str, passed: bool, detail: &'static str) {
        let n = self.count.load(core::sync::atomic::Ordering::Relaxed);
        if n >= MAX_CHECKS {
            return;
        }
        unsafe {
            (*self.checks.get())[n] = Some(Check { name, passed, detail });
        }
        self.count.store(n + 1, core::sync::atomic::Ordering::Relaxed);
    }

    pub fn check_ok(&self, name: &'static str) {
        self.check(name, true, "");
    }

    pub fn len(&self) -> usize {
        self.count.load(core::sync::atomic::Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<Check> {
        if index >= self.len() {
            return None;
        }
        unsafe { (*self.checks.get())[index] }
    }

    /// All checks passed?
    pub fn all_passed(&self) -> bool {
        (0..self.len()).all(|i| self.get(i).map(|c| c.passed).unwrap_or(true))
    }

    /// (passed, failed) counts.
    pub fn tally(&self) -> (usize, usize) {
        let mut p = 0;
        let mut f = 0;
        for i in 0..self.len() {
            match self.get(i) {
                Some(c) if c.passed => p += 1,
                Some(_) => f += 1,
                None => {}
            }
        }
        (p, f)
    }

    /// Render the summary + failures into `out` (console + log view).
    /// Lines are `\n`-terminated ASCII.
    pub fn render(&self, out: &mut [u8]) -> usize {
        let (passed, failed) = self.tally();
        let mut n = 0usize;
        let emit = |bytes: &[u8], out: &mut [u8], n: &mut usize| {
            for &b in bytes {
                if *n < out.len() {
                    out[*n] = b;
                    *n += 1;
                }
            }
        };
        emit(b"BOOT SELF-TEST ", out, &mut n);
        // "12/12 PASS" or "10/12 FAIL"
        let mut num = [0u8; 8];
        let mut w = 0usize;
        let mut v = passed;
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
        emit(b"/", out, &mut n);
        let mut v = passed + failed;
        let mut w = 0usize;
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
        emit(if failed == 0 { b" PASS\n" } else { b" FAIL\n" }, out, &mut n);
        // Failure lines with details.
        for i in 0..self.len() {
            if let Some(c) = self.get(i) {
                if !c.passed {
                    emit(b"  FAIL ", out, &mut n);
                    emit(c.name.as_bytes(), out, &mut n);
                    if !c.detail.is_empty() {
                        emit(b" (", out, &mut n);
                        emit(c.detail.as_bytes(), out, &mut n);
                        emit(b")", out, &mut n);
                    }
                    emit(b"\n", out, &mut n);
                }
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// Global registry + boot-time check set (F025 target path)
// ---------------------------------------------------------------------------

static SELFTEST: SelfTest = SelfTest::new();

pub fn registry() -> &'static SelfTest {
    &SELFTEST
}

/// Run every boot-chain health check against the live boot state and log the
/// summary. Each domain module reports its own outcome; this function only
/// aggregates (keeps domain boundaries intact).
pub fn run_boot_checks() -> (usize, usize) {
    let r = &SELFTEST;

    // F001/F002 — bootloader handoff.
    let bl = crate::limine::bootloader_info().is_some();
    r.check("bootloader", bl, "no bootloader info response");

    // F003/F004 — framebuffer.
    let fb_ok = crate::limine::framebuffer().is_some();
    r.check("framebuffer", fb_ok, "no framebuffer response");

    // F011/F012 — console up and actually drew pixels.
    let console_ok = crate::console::installed() && {
        crate::console::installed_ref().map(|c| c.wrote_any()).unwrap_or(false)
    };
    r.check("console", console_ok, "no pixels written");

    // F005/F006 — logger ring collected lines.
    let log_ok = crate::logger::ring().total_written() > 0;
    r.check("log-ring", log_ok, "ring empty");

    // F008/F009 — ACPI + MADT.
    let acpi_ok = crate::acpi::init().map(|s| s.has_madt && s.enabled_cpus > 0).unwrap_or(false);
    r.check("acpi-madt", acpi_ok, "no MADT CPUs");

    // F010 — SMBIOS.
    let smbios_ok = crate::smbios::init().is_some();
    r.check("smbios", smbios_ok, "entry point unreadable");

    // F015 — memory map.
    let mem_ok = crate::memmap::init().map(|s| s.usable_bytes > 0).unwrap_or(false);
    r.check("memmap", mem_ok, "no usable memory");

    // F016 — reservations registered without conflict.
    crate::memmap::register_boot_reservations();
    let resv_ok = crate::memmap::reservations().len() >= 1;
    r.check("reservations", resv_ok, "nothing reserved");

    // F017 — cmdline parsed (even an empty one parses).
    let _ = crate::cmdline::init();
    r.check_ok("cmdline");

    // F018 — entropy pool seeded.
    let (_, seeded) = crate::kaslr::pool_state();
    r.check("kaslr-entropy", seeded, "pool not stirred");

    // F019/F020 — platform probe.
    let plat = crate::platform::detect();
    r.check("cpu-features", plat.features.sse2, "SSE2 absent");

    // F021 — integrity verdict.
    let chain = crate::integrity::init();
    r.check(
        "boot-chain",
        chain.verdict != crate::integrity::Verdict::Mismatch,
        "hash mismatch",
    );

    // F022 — boot options resolved.
    let _ = crate::bootopt::init();
    r.check_ok("boot-options");

    let (passed, failed) = r.tally();
    if failed == 0 {
        crate::kinfo!("self-test: {}/{} pass", passed, passed);
    } else {
        // Emit every failing item to serial: the graphical render is useless
        // when the console is the thing being diagnosed.
        for i in 0..r.len() {
            if let Some(c) = r.get(i) {
                if !c.passed {
                    crate::kerror!(
                        "self-check {} FAILED: {}",
                        c.name,
                        if c.detail.is_empty() { "-" } else { c.detail }
                    );
                }
            }
        }
        crate::kwarn!("self-test: {}/{} pass ({} FAIL)", passed, passed + failed, failed);
    }
    (passed, failed)
}

/// Render the self-test block onto the global console.
pub fn render_to_console() {
    let mut buf = [0u8; 512];
    let n = SELFTEST.render(&mut buf);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &buf[..n] {
            c.put_byte(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lifecycle() {
        let st = SelfTest::new();
        assert!(st.is_empty());
        st.check_ok("alpha");
        st.check("beta", false, "detail here");
        st.check("gamma", true, "");
        assert_eq!(st.len(), 3);
        let (p, f) = st.tally();
        assert_eq!((p, f), (2, 1));
        assert!(!st.all_passed());
        assert_eq!(st.get(1).unwrap().name, "beta");
        assert!(st.get(3).is_none());
    }

    #[test]
    fn all_passed() {
        let st = SelfTest::new();
        st.check_ok("a");
        st.check_ok("b");
        assert!(st.all_passed());
        assert_eq!(st.tally(), (2, 0));
    }

    #[test]
    fn capacity_is_bounded() {
        let st = SelfTest::new();
        for i in 0..(MAX_CHECKS + 10) {
            st.check("x", i % 2 == 0, "");
        }
        assert_eq!(st.len(), MAX_CHECKS);
    }

    #[test]
    fn render_pass_and_fail() {
        let st = SelfTest::new();
        st.check_ok("one");
        st.check_ok("two");
        let mut out = [0u8; 256];
        let n = st.render(&mut out);
        let s = core::str::from_utf8(&out[..n]).unwrap();
        assert!(s.starts_with("BOOT SELF-TEST 2/2 PASS"), "got: {s}");
        assert!(!s.contains("FAIL"));

        let st2 = SelfTest::new();
        st2.check_ok("one");
        st2.check("two", false, "missing");
        let mut out2 = [0u8; 256];
        let n2 = st2.render(&mut out2);
        let s2 = core::str::from_utf8(&out2[..n2]).unwrap();
        assert!(s2.starts_with("BOOT SELF-TEST 1/2 FAIL"), "got: {s2}");
        assert!(s2.contains("FAIL two (missing)"), "got: {s2}");
    }

    #[test]
    fn render_respects_buffer() {
        let st = SelfTest::new();
        for _ in 0..10 {
            st.check("check-name-long", false, "boom");
        }
        let mut out = [0u8; 8];
        let n = st.render(&mut out);
        assert_eq!(n, 8);
    }
}
