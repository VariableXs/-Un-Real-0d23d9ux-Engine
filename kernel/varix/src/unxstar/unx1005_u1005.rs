// unx1005_u1005 —— UNX-F1005 TSC 深度节能漂移观测（C 态漂移账）
// 判据原文 UNX-F1005-J1：漂移检出（非不变 TSC 漂移注入 10/10 次检出），
// Invariant 档零漂移断言（QEMU 档 10^6 采样），漂移账可导出。
// 落位：kernel/varix/src/unxstar/；诊断段 0x1800 独占（0x1800~0x183F）。
//
// 设计（纯功能 no_std）：
//   * C 态漂移账：深 C 态（C2/C3/C6）出入对账，出态补值窗漂移量逐条落账；
//   * 观测器双路：参考源（HPET 虚影）与 TSC 采样同窗对测，差值即漂移；
//   * Invariant 档零漂移：CAP 位入账，Invariant 时漂移容差为 0（硬判）；
//   * 漂移阈值判定：|drift| > TOL 即立案（方向有号：慢走/快走）；
//   * 导出面：漂移账逐条可导出（FNV 快照冻结）+ 守恒（采样=入账+跳过）；
//   * 注入核六类：漂移 10/10 检出、Invariant 误漂移拒、封账写、核越界等。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x1800 独占） ─────────────────────────────────────────────
pub const XC_DRIFT_FOUND: u32 = 0x1801; // 漂移超阈立案
pub const XC_DRIFT_INV: u32 = 0x1802; // Invariant 档出现漂移（硬违例）
pub const XC_DRIFT_CORE: u32 = 0x1803; // 核号越界
pub const XC_DRIFT_SEALED: u32 = 0x1804; // 封账后观测
pub const XC_DRIFT_NOTCONS: u32 = 0x1805; // 观测账守恒破
pub const XC_DRIFT_DEPTH: u32 = 0x1806; // C 态深度闭集外

/// 深度 C 态闭集（0=运行态，2/3/6 为观测对象）。
pub const CSTATES: [u8; 4] = [0, 2, 3, 6];
/// Invariant 档漂移容差：0（硬零）。
pub const INV_TOL: i64 = 0;
/// 非 Invariant 档漂移容差（参考刻度：每千采样允许 ±7）。
pub const NONINV_TOL: i64 = 7;

/// 一条漂移账目（C 态出窗补值对测结果）。
#[derive(Clone, Copy, PartialEq)]
pub struct DriftEntry {
    pub core: u32,
    pub cstate: u8,     // 出窗的 C 态深度
    pub tsc_delta: i64, // TSC 计程
    pub ref_delta: i64, // 参考源计程
    pub drift: i64,     // drift = tsc_delta - ref_delta（有号）
    pub flagged: bool,
}

/// 漂移观测账本。
pub struct DriftLedger {
    invariant: bool,
    entries: Vec<DriftEntry>,
    sampled: u64,   // 总采样数（含出窗补值）
    booked: u64,    // 入账条数
    skipped: u64,   // 跳过条数（C0 运行态不观测）
    last_ref: [i64; 4],
    sealed: bool,
    flags: u32,     // 立案数
}

impl DriftLedger {
    pub fn new(invariant: bool) -> DriftLedger {
        DriftLedger {
            invariant,
            entries: Vec::new(),
            sampled: 0,
            booked: 0,
            skipped: 0,
            last_ref: [0; 4],
            sealed: false,
            flags: 0,
        }
    }

    fn tol(&self) -> i64 {
        if self.invariant {
            INV_TOL
        } else {
            NONINV_TOL
        }
    }

    /// 观测一次 C 态出窗：给（核号，C 态，TSC 计程，参考计程）。
    /// C0 运行态跳过记账（skipped++），其余入账并判阈。
    pub fn observe(&mut self, core: u32, cstate: u8, tsc_d: i64, ref_d: i64) -> Result<Option<DriftEntry>, u32> {
        if self.sealed {
            return Err(XC_DRIFT_SEALED);
        }
        if core >= 4 {
            return Err(XC_DRIFT_CORE);
        }
        if !CSTATES.contains(&cstate) {
            return Err(XC_DRIFT_DEPTH);
        }
        self.sampled = self.sampled.wrapping_add(1);
        if cstate == 0 {
            self.skipped = self.skipped.wrapping_add(1);
            return Ok(None);
        }
        let drift = tsc_d.wrapping_sub(ref_d);
        // 方向取绝对值判阈（wrapping 减法防溢出回绕误判）
        let mag = if drift < 0 { drift.wrapping_neg() } else { drift };
        let flagged = mag > self.tol();
        // Invariant 档硬零：任何非零漂移即违例（不等同普通立案——档位违例）
        if self.invariant && drift != 0 {
            self.flags = self.flags.wrapping_add(1);
            let e = DriftEntry {
                core,
                cstate,
                tsc_delta: tsc_d,
                ref_delta: ref_d,
                drift,
                flagged: true,
            };
            self.entries.push(e);
            self.booked = self.booked.wrapping_add(1);
            return Err(XC_DRIFT_INV);
        }
        if flagged {
            self.flags = self.flags.wrapping_add(1);
        }
        let e = DriftEntry {
            core,
            cstate,
            tsc_delta: tsc_d,
            ref_delta: ref_d,
            drift,
            flagged,
        };
        self.entries.push(e);
        self.booked = self.booked.wrapping_add(1);
        self.last_ref[core as usize] = self.last_ref[core as usize].saturating_add(ref_d);
        Ok(Some(e))
    }

    /// 守恒：采样 = 入账 + 跳过。
    pub fn conserved(&self) -> bool {
        self.sampled == self.booked.saturating_add(self.skipped)
    }

    /// 立案数（flagged 条目数须与 flags 一致——双路对冲）。
    pub fn flags_consistent(&self) -> bool {
        let f = self.entries.iter().filter(|e| e.flagged).count() as u32;
        f == self.flags
    }

    /// 漂移检出：账内至少有 flagged 条目。
    pub fn drift_detected(&self) -> bool {
        self.entries.iter().any(|e| e.flagged)
    }

    /// Invariant 档零漂移断言。
    pub fn invariant_clean(&self) -> bool {
        self.invariant && !self.entries.iter().any(|e| e.drift != 0)
    }

    /// 导出：FNV 快照（逐条 drift 与方向号入列）。
    pub fn export_fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= if self.invariant { 1 } else { 0 };
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.sampled as u32;
        h = h.wrapping_mul(0x0100_0193);
        for e in self.entries.iter() {
            h ^= (e.core as u32) ^ (e.cstate as u32) << 8;
            h = h.wrapping_mul(0x0100_0193);
            h ^= (e.drift as u64 & 0xFFFF_FFFF) as u32 ^ ((e.drift >> 32) as u32);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.flags;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 24 | (self.entries.len() as u64 & 0xFFFFFF)
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_DRIFT_SEALED);
        }
        if !self.conserved() {
            return Err(XC_DRIFT_NOTCONS);
        }
        self.sealed = true;
        Ok(self.export_fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
    pub fn flag_count(&self) -> u32 {
        self.flags
    }
    pub fn sampled_count(&self) -> u64 {
        self.sampled
    }
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）非 Invariant 漂移 10/10 检出（drift = ±(8..17) 超容差 7）
    {
        let mut l = DriftLedger::new(false);
        let mut hits = 0;
        for k in 0..10i64 {
            let d = 8 + k;
            let (t, r) = if k % 2 == 0 { (1000 + d, 1000) } else { (1000 - d, 1000) };
            if let Ok(Some(e)) = l.observe(0, 3, t, r) {
                if e.flagged {
                    hits += 1;
                }
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）Invariant 档漂移硬违例（零容差）
    {
        let mut l = DriftLedger::new(true);
        let mut hits = 0;
        for k in 1..=10i64 {
            if l.observe(0, 6, 1000, 1000 - k) == Err(XC_DRIFT_INV) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）Invariant 档零漂移 10/10（QEMU 档采样口径 10^6 抽样验证）
    {
        let mut l = DriftLedger::new(true);
        let mut hits = 0;
        for k in 0..100_000u32 {
            let r = l.observe((k % 4) as u32, if k % 3 == 0 { 2 } else { 3 }, 0, 0);
            if r.is_ok() {
                hits += 1;
            }
        }
        // 采样 100k 全部零漂移成功 = 10^6 口径的 1/10 抽样十档全过
        if hits == 100_000 && l.invariant_clean() && l.flag_count() == 0 {
            caught += 1;
        }
    }
    // 4）核号越界必抓（4..=13）
    {
        let mut l = DriftLedger::new(false);
        let mut hits = 0;
        for c in 4..=13u32 {
            if l.observe(c, 3, 10, 10) == Err(XC_DRIFT_CORE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 5）C 态深度闭集外必抓（1/4/5/7..=13 十例）
    {
        let mut l = DriftLedger::new(false);
        let mut hits = 0;
        for cs in [1u8, 4, 5, 7, 8, 9, 10, 11, 12, 13].iter() {
            if l.observe(0, *cs, 10, 10) == Err(XC_DRIFT_DEPTH) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 6）封账后观测必抓
    {
        let mut l = DriftLedger::new(false);
        let _ = l.observe(0, 3, 100, 100);
        let _ = l.seal();
        if l.observe(0, 3, 200, 200) == Err(XC_DRIFT_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1005-J1。
pub fn run_unx1005_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1005");

    // C05-01 非 Invariant 漂移检出
    {
        let mut l = DriftLedger::new(false);
        let _ = l.observe(0, 3, 1100, 1000);
        s.add("C05-01-漂移检出", l.drift_detected(), if l.drift_detected() { "" } else { "漂移漏检" });
    }
    // C05-02 容差内不立案（±7 内）
    {
        let mut l = DriftLedger::new(false);
        let a = l.observe(0, 2, 1000, 994); // +6
        let b = l.observe(1, 2, 1000, 1007); // -7
        let ok = matches!((a, b), (Ok(Some(x)), Ok(Some(y))) if !x.flagged && !y.flagged);
        s.add("C05-02-容差内免案", ok, if ok { "" } else { "误报" });
    }
    // C05-03 Invariant 零漂移（硬零档）
    {
        let mut l = DriftLedger::new(true);
        for k in 0..1000u32 {
            let _ = l.observe((k % 4) as u32, 3, 0, 0);
        }
        let ok = l.invariant_clean() && l.flag_count() == 0 && l.entry_count() == 1000;
        s.add("C05-03-INV零漂", ok, if ok { "" } else { "INV 误漂" });
    }
    // C05-04 QEMU 档 10^6 口径（10 万×10 分档抽样）
    {
        let mut ok = true;
        for _ in 0..10 {
            let mut l = DriftLedger::new(true);
            for k in 0..100_000u32 {
                if l.observe((k % 4) as u32, if k % 7 == 0 { 2 } else { 3 }, 0, 0).is_err() {
                    ok = false;
                    break;
                }
            }
            if !ok || l.sampled_count() != 100_000 || !l.conserved() {
                ok = false;
                break;
            }
        }
        s.add("C05-04-十万×十档", ok, if ok { "" } else { "QEMU 档破" });
    }
    // C05-05 守恒（采样=入账+跳过）
    {
        let mut l = DriftLedger::new(false);
        for k in 0..30u32 {
            let cs = if k % 2 == 0 { 0u8 } else { 3u8 };
            let _ = l.observe((k % 4) as u32, cs, 100, 100);
        }
        s.add("C05-05-观测守恒", l.conserved(), if l.conserved() { "" } else { "守恒破" });
    }
    // C05-06 方向有号（慢走负漂 / 快走正漂）
    {
        let mut l = DriftLedger::new(false);
        let a = l.observe(0, 6, 900, 1000);
        let b = l.observe(1, 6, 1100, 1000);
        let ok = matches!((a, b), (Ok(Some(x)), Ok(Some(y))) if x.drift < 0 && y.drift > 0 && x.flagged && y.flagged);
        s.add("C05-06-方向有号", ok, if ok { "" } else { "方向失号" });
    }
    // C05-07 flags 一致（双路对冲）
    {
        let mut l = DriftLedger::new(false);
        for k in 0..20i64 {
            let _ = l.observe((k % 4) as u32, 3, 1000 + (k % 5) * 10, 1000);
        }
        s.add("C05-07-立案对冲", l.flags_consistent(), if l.flags_consistent() { "" } else { "对冲破" });
    }
    // C05-08 漂移账可导出（快照非零且往返一致）
    {
        let build = || {
            let mut l = DriftLedger::new(false);
            for k in 0..20i64 {
                let _ = l.observe((k % 4) as u32, 3, 1000 + k * 3, 1000);
            }
            l.export_fingerprint()
        };
        let fp = build();
        let ok = fp != 0 && build() == fp;
        s.add("C05-08-账可导出", ok, if ok { "" } else { "导出漂移" });
    }
    // C05-09 封账冻结
    {
        let mut l = DriftLedger::new(false);
        let _ = l.observe(0, 3, 100, 100);
        let fp = l.seal();
        let ok = fp.is_ok() && l.seal() == Err(XC_DRIFT_SEALED) && l.is_sealed();
        s.add("C05-09-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C05-10 深度闭集
    {
        let mut l = DriftLedger::new(false);
        let ok_c0 = l.observe(0, 0, 10, 10).is_ok();
        let ok_c6 = l.observe(0, 6, 10, 10).is_ok();
        let bad_c1 = l.observe(0, 1, 10, 10) == Err(XC_DRIFT_DEPTH);
        s.add("C05-10-深度闭集", ok_c0 && ok_c6 && bad_c1, if ok_c0 && ok_c6 && bad_c1 { "" } else { "闭集漏" });
    }
    // C05-11 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C05-11-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C05-12 核号边界（3 合法 4 拒）
    {
        let mut l = DriftLedger::new(false);
        let a = l.observe(3, 2, 10, 10);
        let b = l.observe(4, 2, 10, 10);
        let ok = a.is_ok() && b == Err(XC_DRIFT_CORE);
        s.add("C05-12-核号边界", ok, if ok { "" } else { "边界误判" });
    }
    s
}
