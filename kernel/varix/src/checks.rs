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

/// Maximum domains in the kernel checkup registry. WP-301 后 96 域恰满容量，
/// register 超容静默丢域（KernelCheckup 无 truncated 预警）比恰满更危险——
/// 按"不够即扩"纪律扩容，WP-403 撞 128 余量 2 后 WP-404 八域前扩到 144，
/// STAR I 分工阶段（AI-K1 收口实测）注册面已达 258 域 > 144——既有 114 个
/// 注册被静默丢弃，AI-K1 追加 B 性能域 17 域（F041~F057，robust.rs）后
/// 共 275，按同一纪律扩到 288（余量 13 供后续收尾域）；AI-K2 追加 18 域后
/// 逼近 288 上沿，AI-C1 追加 A 兼容域 20 域（F001~F020）→ 294+，按同一
/// 「不够即扩」纪律扩到 320（余量供后续分队收口）。
pub const MAX_DOMAINS: usize = 320;

/// 每域聚合摘要（register 时从 CheckSet 提取）。CheckSet 全量值拷贝入
/// `[Option<CheckSet>; MAX_DOMAINS]` 会让 KernelCheckup 达 ~870KB——栈上
/// 构造（run_kernel_checkup / aggregate_checksets 的局部量 + 返回槽三份）
/// 直接爆测试线程栈（STATUS_STACK_OVERFLOW 实锤，MAX_DOMAINS=288 后暴露）。
/// 失败明细的 name/detail 恒为 'static 字面量，以引用保留前 8 条；超出
/// render 中如实标注（不静默吞）。
#[derive(Clone, Copy, Debug)]
struct DomainRecord {
    domain: &'static str,
    passed: u16,
    failed: u16,
    dropped: u32,
    fail_name: [&'static str; 8],
    fail_detail: [&'static str; 8],
    fail_n: usize,
}

/// 每域保留的失败明细条数。
const FAIL_DETAIL_KEEP: usize = 8;

/// Aggregate result of `run_kernel_checkup()`.
#[derive(Clone, Copy, Debug)]
pub struct KernelCheckup {
    records: [Option<DomainRecord>; MAX_DOMAINS],
    count: usize,
}

impl KernelCheckup {
    pub const fn new() -> KernelCheckup {
        KernelCheckup {
            records: [None; MAX_DOMAINS],
            count: 0,
        }
    }

    /// Register one domain's result. 提取摘要而非拷贝整个 CheckSet——
    /// 275+ 域 × ~3KB/域的值数组在栈上不可承受（见 DomainRecord 注释）。
    pub fn register(&mut self, set: CheckSet) {
        if self.count < MAX_DOMAINS {
            let mut r = DomainRecord {
                domain: set.domain,
                passed: 0,
                failed: 0,
                dropped: set.dropped as u32,
                fail_name: [""; FAIL_DETAIL_KEEP],
                fail_detail: [""; FAIL_DETAIL_KEEP],
                fail_n: 0,
            };
            for i in 0..set.count {
                if let Some(c) = set.checks[i] {
                    if c.passed {
                        r.passed += 1;
                    } else {
                        r.failed += 1;
                        if r.fail_n < FAIL_DETAIL_KEEP {
                            r.fail_name[r.fail_n] = c.name;
                            r.fail_detail[r.fail_n] = c.detail;
                            r.fail_n += 1;
                        }
                    }
                }
            }
            self.records[self.count] = Some(r);
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn tally(&self) -> (usize, usize) {
        let mut passed = 0usize;
        let mut failed = 0usize;
        for i in 0..self.count {
            if let Some(r) = self.records[i] {
                passed += r.passed as usize;
                failed += r.failed as usize;
            }
        }
        (passed, failed)
    }

    pub fn all_passed(&self) -> bool {
        (0..self.count).all(|i| self.records[i].map(|r| r.failed == 0).unwrap_or(true))
    }

    /// Render every domain line into `out`（格式与 CheckSet::render 一致：
    /// `domain PASS p/n` / `domain FAIL p/n` + `  - name: detail` 失败行；
    /// 超出 FAIL_DETAIL_KEEP 的失败与被 drop 的检查如实标注，不留诊断盲区）。
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if n >= out.len() {
                break;
            }
            if let Some(r) = self.records[i] {
                push_str(out, &mut n, r.domain);
                push_str(out, &mut n, if r.failed == 0 { " PASS " } else { " FAIL " });
                push_usize(out, &mut n, r.passed as usize);
                push_str(out, &mut n, "/");
                push_usize(out, &mut n, (r.passed + r.failed) as usize);
                push_str(out, &mut n, "\n");
                for k in 0..r.fail_n {
                    push_str(out, &mut n, "  - ");
                    push_str(out, &mut n, r.fail_name[k]);
                    push_str(out, &mut n, ": ");
                    push_str(out, &mut n, r.fail_detail[k]);
                    push_str(out, &mut n, "\n");
                }
                if r.failed as usize > r.fail_n {
                    push_str(out, &mut n, "  - ... and ");
                    push_usize(out, &mut n, r.failed as usize - r.fail_n);
                    push_str(out, &mut n, " more failed checks\n");
                }
                if r.dropped > 0 {
                    push_str(out, &mut n, "  - truncated: ");
                    push_usize(out, &mut n, r.dropped as usize);
                    push_str(out, &mut n, " results dropped\n");
                }
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
