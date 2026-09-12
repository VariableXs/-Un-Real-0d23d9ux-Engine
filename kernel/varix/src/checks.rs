//! Shared self-test plumbing for the per-domain `自检` features
//! (F275/F300/F325/F350/F375/F400/F425/F450/F475/F500) and the kernel-wide
//! closed loop (F475).
//!
//! Every domain builds a `CheckSet` locally (no allocator, fixed capacity)
//! and hands it to the aggregator. Rendering stays byte-oriented so the
//! console, the serial log and the QEMU headless assertions (F477) all read
//! the same text.

/// Maximum checks recorded by one domain self-test.
pub const MAX_CHECKS: usize = 64;

/// One check result — name, verdict and a short failure detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    pub passed: bool,
    pub detail: &'static str,
}

/// Fixed-capacity result set returned by a domain self-test.
#[derive(Clone, Copy, Debug)]
pub struct CheckSet {
    checks: [Option<Check>; MAX_CHECKS],
    count: usize,
    /// Results dropped because the set was already full. Tracked separately from
    /// `count` so that a域 producing *exactly* MAX_CHECKS checks is not
    /// misreported as truncated.
    dropped: usize,
    /// Domain tag, e.g. `"power"`.
    pub domain: &'static str,
}

impl CheckSet {
    pub const fn new(domain: &'static str) -> CheckSet {
        CheckSet {
            checks: [None; MAX_CHECKS],
            count: 0,
            dropped: 0,
            domain,
        }
    }

    /// Record one result (no-op when full — the tally stays honest because
    /// `truncated()` reports the overflow).
    pub fn add(&mut self, name: &'static str, passed: bool, detail: &'static str) {
        if self.count >= MAX_CHECKS {
            self.dropped += 1;
            return;
        }
        self.checks[self.count] = Some(Check { name, passed, detail });
        self.count += 1;
    }

    /// Record a passing check with no detail.
    pub fn ok(&mut self, name: &'static str) {
        self.add(name, true, "");
    }

    /// Record a failing check.
    pub fn fail(&mut self, name: &'static str, detail: &'static str) {
        self.add(name, false, detail);
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn get(&self, index: usize) -> Option<Check> {
        if index < self.count {
            self.checks[index]
        } else {
            None
        }
    }

    /// True when more results were produced than fit (i.e. at least one was
    /// dropped). A set that is merely *full* is not truncated.
    pub fn truncated(&self) -> bool {
        self.dropped > 0
    }

    /// How many results were dropped past the capacity.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn all_passed(&self) -> bool {
        (0..self.count).all(|i| self.get(i).map(|c| c.passed).unwrap_or(true))
    }

    pub fn tally(&self) -> (usize, usize) {
        let mut passed = 0usize;
        for i in 0..self.count {
            if let Some(c) = self.get(i) {
                if c.passed {
                    passed += 1;
                }
            }
        }
        (passed, self.count - passed)
    }

    /// Render `domain PASS 12/12` style line plus failures into `out`.
    /// Returns bytes written.
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        push_str(out, &mut n, self.domain);
        push_str(out, &mut n, if self.all_passed() { " PASS " } else { " FAIL " });
        let (passed, failed) = self.tally();
        push_usize(out, &mut n, passed);
        push_str(out, &mut n, "/");
        push_usize(out, &mut n, passed + failed);
        push_str(out, &mut n, "\n");
        for i in 0..self.count {
            if let Some(c) = self.get(i) {
                if !c.passed {
                    push_str(out, &mut n, "  - ");
                    push_str(out, &mut n, c.name);
                    push_str(out, &mut n, ": ");
                    push_str(out, &mut n, c.detail);
                    push_str(out, &mut n, "\n");
                }
            }
        }
        n
    }
}

/// Copy `s` into `out` at `n`, clamped.
pub fn push_str(out: &mut [u8], n: &mut usize, s: &str) {
    for &b in s.as_bytes() {
        if *n < out.len() {
            out[*n] = b;
            *n += 1;
        }
    }
}

/// Decimal rendering without a formatter.
pub fn push_usize(out: &mut [u8], n: &mut usize, mut v: usize) {
    if v == 0 {
        push_str(out, n, "0");
        return;
    }
    let mut digits = [0u8; 20];
    let mut w = 0usize;
    while v > 0 && w < digits.len() {
        digits[w] = b'0' + (v % 10) as u8;
        v /= 10;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        if *n < out.len() {
            out[*n] = digits[w];
            *n += 1;
        }
    }
}

/// Lowercase hex rendering (minimal width, no `0x` prefix).
pub fn push_hex_u64(out: &mut [u8], n: &mut usize, mut v: u64) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    if v == 0 {
        push_str(out, n, "0");
        return;
    }
    let mut digits = [0u8; 16];
    let mut w = 0usize;
    while v > 0 && w < digits.len() {
        digits[w] = HEX[(v & 0xF) as usize];
        v >>= 4;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        if *n < out.len() {
            out[*n] = digits[w];
            *n += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// F475 — kernel-wide closed loop: every domain registers here, one verdict.
// ---------------------------------------------------------------------------

/// Maximum domains in the kernel checkup registry.
pub const MAX_DOMAINS: usize = 96;

/// Aggregate result of `run_kernel_checkup()`.
#[derive(Clone, Copy, Debug)]
pub struct KernelCheckup {
    sets: [Option<CheckSet>; MAX_DOMAINS],
    count: usize,
}

impl KernelCheckup {
    pub const fn new() -> KernelCheckup {
        KernelCheckup {
            sets: [None; MAX_DOMAINS],
            count: 0,
        }
    }

    pub fn register(&mut self, set: CheckSet) {
        if self.count < MAX_DOMAINS {
            self.sets[self.count] = Some(set);
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, index: usize) -> Option<CheckSet> {
        if index < self.count {
            self.sets[index]
        } else {
            None
        }
    }

    pub fn tally(&self) -> (usize, usize) {
        let mut passed = 0usize;
        let mut total = 0usize;
        for i in 0..self.count {
            if let Some(s) = self.get(i) {
                let (p, f) = s.tally();
                passed += p;
                total += p + f;
            }
        }
        (passed, total - passed)
    }

    pub fn all_passed(&self) -> bool {
        (0..self.count).all(|i| self.get(i).map(|s| s.all_passed()).unwrap_or(true))
    }

    /// Render every domain line into `out`.
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(s) = self.get(i) {
                if n >= out.len() {
                    break;
                }
                let written = s.render(&mut out[n..]);
                n += written;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_records_and_tallies() {
        let mut s = CheckSet::new("power");
        s.ok("acpi");
        s.ok("battery");
        s.fail("thermal", "no sensor");
        assert_eq!(s.len(), 3);
        assert_eq!(s.tally(), (2, 1));
        assert!(!s.all_passed());
        assert!(!s.truncated());
        assert_eq!(s.get(2).unwrap().detail, "no sensor");
        assert!(s.get(3).is_none());
    }

    #[test]
    fn set_renders_failures_only() {
        let mut s = CheckSet::new("audio");
        s.ok("hda");
        s.fail("usb", "no controller");
        let mut buf = [0u8; 256];
        let n = s.render(&mut buf);
        let text = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(text.starts_with("audio FAIL 1/2\n"));
        assert!(text.contains("- usb: no controller"));
        assert!(!text.contains("hda"));
    }

    #[test]
    fn set_capacity_is_bounded() {
        let mut s = CheckSet::new("x");
        for _ in 0..MAX_CHECKS + 4 {
            s.ok("y");
        }
        assert_eq!(s.len(), MAX_CHECKS);
        assert!(s.truncated());
        assert!(s.all_passed());
    }

    #[test]
    fn exactly_full_is_not_truncated() {
        // 正好 MAX_CHECKS 项不是"被截断"；只有真的丢掉了才算。
        let mut s = CheckSet::new("x");
        for _ in 0..MAX_CHECKS {
            s.ok("y");
        }
        assert_eq!(s.len(), MAX_CHECKS);
        assert!(!s.truncated());
        assert_eq!(s.dropped(), 0);

        s.ok("z"); // 第 MAX_CHECKS+1 项被丢弃
        assert_eq!(s.len(), MAX_CHECKS);
        assert!(s.truncated());
        assert_eq!(s.dropped(), 1);
    }

    #[test]
    fn numbers_render_as_decimal() {
        let mut buf = [0u8; 32];
        let mut n = 0;
        push_usize(&mut buf, &mut n, 0);
        push_str(&mut buf, &mut n, " ");
        push_usize(&mut buf, &mut n, 1234);
        assert_eq!(core::str::from_utf8(&buf[..n]).unwrap(), "0 1234");
    }

    #[test]
    fn kernel_checkup_aggregates() {
        let mut k = KernelCheckup::new();
        let mut a = CheckSet::new("power");
        a.ok("s3");
        a.ok("s4");
        let mut b = CheckSet::new("audio");
        b.ok("hda");
        b.fail("mixer", "clipping");
        k.register(a);
        k.register(b);
        assert_eq!(k.len(), 2);
        assert_eq!(k.tally(), (3, 1));
        assert!(!k.all_passed());
        let mut buf = [0u8; 512];
        let n = k.render(&mut buf);
        let text = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(text.contains("power PASS 2/2"));
        assert!(text.contains("audio FAIL 1/2"));
    }
}
