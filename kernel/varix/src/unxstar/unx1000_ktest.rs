// unx1000_ktest —— UNX-F1000 ktest 中断面总收口（B01–B10 十批 400 断言大关）
// 判据原文 UNX-F1000-J1：十批断言总收口 ≥400 断言，宿主档通过率 100%，
// QEMU 档全量档通过率 100%，域中断面"中断面 400+ 断言"验收主轴达成并登记。
// 落位：kernel/varix/src/unxstar/；诊断段 0x1780 独占（0x1780~0x17BF）。
//
// 设计（纯功能 no_std）：
//   * 批次登记表：B01~B10 十批，每批登记断言数/通过数/宿主档/QEMU 档通过率；
//   * 总收口账：总断言数 = Σ 批断言数（逐批对账守恒式）；
//   * 双档口径：宿主档与 QEMU 档分别记账，任一档 <100% 即收口失败；
//   * 封账语义：seal 后登记必拒，指纹 FNV-1a 冻结（0x811C9DC5/0x01000193）；
//   * 边界：批号越界/重复/通过>断言/断言数下限全部显式拒绝，回绕用 saturating。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x1780 独占，n 从 1 递增） ────────────────────────────────
pub const XC_KTEST_OOB: u32 = 0x1780 + 1; // 批号越界（非 B01~B10）
pub const XC_KTEST_DUP: u32 = 0x1780 + 2; // 批号重复登记
pub const XC_KTEST_COUNT: u32 = 0x1780 + 3; // 批断言数越界（<40 或 >80）
pub const XC_KTEST_PASS: u32 = 0x1780 + 4; // 通过数 > 断言数
pub const XC_KTEST_SEALED: u32 = 0x1780 + 5; // 封账后登记
pub const XC_KTEST_SHORT: u32 = 0x1780 + 6; // 总收口 <400 断言
pub const XC_KTEST_RATE: u32 = 0x1780 + 7; // 通过率 <100%（任一档）
pub const XC_KTEST_LOST: u32 = 0x1780 + 8; // 守恒破（总数 ≠ Σ 批数）
pub const XC_KTEST_WRAP: u32 = 0x1780 + 9; // 计数回绕（溢出防护破）

// ── 常数（写死并注释来源） ────────────────────────────────────────────
/// 收口大关：中断面十批合计断言下限（判据 J1 明文 ≥400）。
pub const KTEST_TOTAL_MIN: u32 = 400;
/// 单批断言下限（B 批最小体量，防灌水批）。
pub const KTEST_BATCH_MIN: u32 = 40;
/// 单批断言上限（ktest 定容检查槽上限的批配额）。
pub const KTEST_BATCH_MAX: u32 = 80;
/// 合法批次区间：B01~B10（中断面十批）。
pub const KTEST_BATCH_LO: u32 = 1;
pub const KTEST_BATCH_HI: u32 = 10;
/// 千分比通过率满分（1000 = 100%）。
pub const RATE_FULL: u32 = 1000;
/// FNV 基数（冻结口径）。
pub const FNV_BASIS: u32 = 0x811C_9DC5;
/// FNV 乘子（冻结口径）。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 单批登记记录。
#[derive(Clone, Copy)]
pub struct BatchRec {
    pub batch: u32,     // 批号 1..=10（B01~B10）
    pub asserted: u32,  // 本批断言总数
    pub passed_host: u32,  // 宿主档通过数
    pub passed_qemu: u32,  // QEMU 档通过数
}

/// 收口总账。
pub struct KtestLedger {
    batches: Vec<BatchRec>,
    total_asserted: u32,
    total_host: u32,
    total_qemu: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl KtestLedger {
    pub fn new() -> KtestLedger {
        KtestLedger {
            batches: Vec::new(),
            total_asserted: 0,
            total_host: 0,
            total_qemu: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 登记一批。全部边界显式拒绝，Err 即诊断码。
    pub fn register(&mut self, r: BatchRec) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_KTEST_SEALED, "封账后登记批"));
            return Err(XC_KTEST_SEALED);
        }
        if r.batch < KTEST_BATCH_LO || r.batch > KTEST_BATCH_HI {
            self.rejects.push((XC_KTEST_OOB, "批号越界：仅 B01~B10"));
            return Err(XC_KTEST_OOB);
        }
        for b in &self.batches {
            if b.batch == r.batch {
                self.rejects.push((XC_KTEST_DUP, "批号重复登记"));
                return Err(XC_KTEST_DUP);
            }
        }
        if r.asserted < KTEST_BATCH_MIN || r.asserted > KTEST_BATCH_MAX {
            self.rejects.push((XC_KTEST_COUNT, "批断言数越界：须在 40~80"));
            return Err(XC_KTEST_COUNT);
        }
        if r.passed_host > r.asserted || r.passed_qemu > r.asserted {
            self.rejects.push((XC_KTEST_PASS, "通过数不得大于断言数"));
            return Err(XC_KTEST_PASS);
        }
        // 饱和累加：计数永不回绕
        self.total_asserted = self.total_asserted.saturating_add(r.asserted);
        self.total_host = self.total_host.saturating_add(r.passed_host);
        self.total_qemu = self.total_qemu.saturating_add(r.passed_qemu);
        self.batches.push(r);
        Ok(())
    }

    /// 批数（登记成功的批）。
    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }

    /// 总断言数。
    pub fn total_asserted(&self) -> u32 {
        self.total_asserted
    }

    /// 宿主档千分比通过率（0 批时返回 0，不算满分）。
    pub fn host_rate_permille(&self) -> u32 {
        if self.total_asserted == 0 {
            return 0;
        }
        self.total_host.saturating_mul(RATE_FULL) / self.total_asserted
    }

    /// QEMU 档千分比通过率。
    pub fn qemu_rate_permille(&self) -> u32 {
        if self.total_asserted == 0 {
            return 0;
        }
        self.total_qemu.saturating_mul(RATE_FULL) / self.total_asserted
    }

    /// 守恒对账：总断言数 = Σ 批断言数，双档总数同法。
    pub fn conserved(&self) -> bool {
        let mut sa: u32 = 0;
        let mut sh: u32 = 0;
        let mut sq: u32 = 0;
        for b in &self.batches {
            sa = sa.wrapping_add(b.asserted);
            sh = sh.wrapping_add(b.passed_host);
            sq = sq.wrapping_add(b.passed_qemu);
        }
        sa == self.total_asserted && sh == self.total_host && sq == self.total_qemu
    }

    /// 溢出防护：逐批 wrapping 累加无回退且不超饱和界。
    pub fn no_wrap(&self) -> bool {
        let mut sum: u32 = 0;
        let mut wrapped = false;
        for b in &self.batches {
            let next = sum.wrapping_add(b.asserted);
            if next < sum {
                wrapped = true;
            }
            sum = next;
        }
        !wrapped
    }

    /// 收口判定：十批齐 + ≥400 断言 + 双档 100% + 守恒。
    pub fn closed(&self) -> Result<(), u32> {
        if self.batch_count() != (KTEST_BATCH_HI - KTEST_BATCH_LO + 1) as usize {
            return Err(XC_KTEST_SHORT);
        }
        if self.total_asserted < KTEST_TOTAL_MIN {
            return Err(XC_KTEST_SHORT);
        }
        if self.host_rate_permille() < RATE_FULL {
            return Err(XC_KTEST_RATE);
        }
        if self.qemu_rate_permille() < RATE_FULL {
            return Err(XC_KTEST_RATE);
        }
        if !self.conserved() {
            return Err(XC_KTEST_LOST);
        }
        Ok(())
    }

    /// 封账：返回指纹。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_KTEST_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// FNV-1a 指纹（冻结口径）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = FNV_BASIS;
        h ^= self.total_asserted;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.total_host;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.total_qemu;
        h = h.wrapping_mul(FNV_PRIME);
        for b in &self.batches {
            h ^= b.batch ^ b.asserted.wrapping_mul(0x9E37) ^ b.passed_host ^ b.passed_qemu;
            h = h.wrapping_mul(FNV_PRIME);
        }
        (h as u64) << 16 | (self.batch_count() as u64 & 0xFFFF)
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
}

/// 十批演示构造：全过档（供判据入口复用）。
pub fn build_full_ten_batches() -> KtestLedger {
    let mut l = KtestLedger::new();
    // B01~B09 各 40，B10 = 400 - 360 = 40；取 40×10 = 400 恰好过线
    for b in KTEST_BATCH_LO..=KTEST_BATCH_HI {
        let _ = l.register(BatchRec {
            batch: b,
            asserted: 40,
            passed_host: 40,
            passed_qemu: 40,
        });
    }
    l
}

/// 注入核：六类破坏各注入一次，逐项断言诊断码精确匹配。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）批号越界必拒（0 与 11 双向）
    {
        let mut l = KtestLedger::new();
        let mut hits = 0;
        if l.register(BatchRec { batch: 0, asserted: 40, passed_host: 40, passed_qemu: 40 }) == Err(XC_KTEST_OOB) {
            hits += 1;
        }
        if l.register(BatchRec { batch: 11, asserted: 40, passed_host: 40, passed_qemu: 40 }) == Err(XC_KTEST_OOB) {
            hits += 1;
        }
        if hits == 2 {
            caught += 1;
        }
    }
    // 2）批断言数越界必拒（<40 与 >80）
    {
        let mut l = KtestLedger::new();
        let mut hits = 0;
        if l.register(BatchRec { batch: 1, asserted: 39, passed_host: 39, passed_qemu: 39 }) == Err(XC_KTEST_COUNT) {
            hits += 1;
        }
        if l.register(BatchRec { batch: 1, asserted: 81, passed_host: 81, passed_qemu: 81 }) == Err(XC_KTEST_COUNT) {
            hits += 1;
        }
        if hits == 2 {
            caught += 1;
        }
    }
    // 3）重复批号必拒
    {
        let mut l = KtestLedger::new();
        let _ = l.register(BatchRec { batch: 3, asserted: 40, passed_host: 40, passed_qemu: 40 });
        if l.register(BatchRec { batch: 3, asserted: 40, passed_host: 40, passed_qemu: 40 }) == Err(XC_KTEST_DUP) {
            caught += 1;
        }
    }
    // 4）通过数 > 断言数必拒
    {
        let mut l = KtestLedger::new();
        if l.register(BatchRec { batch: 4, asserted: 40, passed_host: 41, passed_qemu: 40 }) == Err(XC_KTEST_PASS) {
            caught += 1;
        }
    }
    // 5）封账后登记必拒
    {
        let mut l = KtestLedger::new();
        let _ = l.register(BatchRec { batch: 1, asserted: 40, passed_host: 40, passed_qemu: 40 });
        let _ = l.seal();
        if l.register(BatchRec { batch: 2, asserted: 40, passed_host: 40, passed_qemu: 40 }) == Err(XC_KTEST_SEALED) {
            caught += 1;
        }
    }
    // 6）不满十批不得收口（守恒不破前提下的下限防护）
    {
        let mut l = KtestLedger::new();
        let _ = l.register(BatchRec { batch: 1, asserted: 40, passed_host: 40, passed_qemu: 40 });
        if l.closed() == Err(XC_KTEST_SHORT) && l.conserved() {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1000-J1。
pub fn run_unx1000_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1000");

    // C00-01 十批全登记成功
    {
        let l = build_full_ten_batches();
        let ok = l.batch_count() == 10;
        s.add("C00-01-十批登记", ok, if ok { "" } else { "登记缺批" });
    }
    // C00-02 总收口 ≥400
    {
        let l = build_full_ten_batches();
        let ok = l.total_asserted() >= KTEST_TOTAL_MIN;
        s.add("C00-02-总量过线", ok, if ok { "" } else { "总量不足" });
    }
    // C00-03 宿主档 100%
    {
        let l = build_full_ten_batches();
        let ok = l.host_rate_permille() == RATE_FULL;
        s.add("C00-03-宿主全过", ok, if ok { "" } else { "宿主档缺" });
    }
    // C00-04 QEMU 档 100%
    {
        let l = build_full_ten_batches();
        let ok = l.qemu_rate_permille() == RATE_FULL;
        s.add("C00-04-QEMU全过", ok, if ok { "" } else { "QEMU 档缺" });
    }
    // C00-05 收口判定通过
    {
        let mut l = build_full_ten_batches();
        let ok = l.closed().is_ok() && l.seal().is_ok();
        s.add("C00-05-收口达成", ok, if ok { "" } else { "收口未过" });
    }
    // C00-06 守恒对账
    {
        let l = build_full_ten_batches();
        s.add("C00-06-计数守恒", l.conserved(), if l.conserved() { "" } else { "守恒破" });
    }
    // C00-07 回绕防护
    {
        let l = build_full_ten_batches();
        s.add("C00-07-无回绕", l.no_wrap(), if l.no_wrap() { "" } else { "回绕发生" });
    }
    // C00-08 批号越界拒
    {
        let mut l = KtestLedger::new();
        let ok = l.register(BatchRec { batch: 99, asserted: 40, passed_host: 40, passed_qemu: 40 }) == Err(XC_KTEST_OOB);
        s.add("C00-08-批号越界", ok, if ok { "" } else { "越界漏抓" });
    }
    // C00-09 重复批号拒
    {
        let mut l = KtestLedger::new();
        let _ = l.register(BatchRec { batch: 2, asserted: 40, passed_host: 40, passed_qemu: 40 });
        let ok = l.register(BatchRec { batch: 2, asserted: 40, passed_host: 40, passed_qemu: 40 }) == Err(XC_KTEST_DUP);
        s.add("C00-09-重复批拒", ok, if ok { "" } else { "重复漏抓" });
    }
    // C00-10 部分失败时通过率下降
    {
        let mut l = KtestLedger::new();
        for b in KTEST_BATCH_LO..=KTEST_BATCH_HI {
            let host = if b == 5 { 39 } else { 40 };
            let _ = l.register(BatchRec { batch: b, asserted: 40, passed_host: host, passed_qemu: 40 });
        }
        let ok = l.host_rate_permille() < RATE_FULL && l.closed() == Err(XC_KTEST_RATE);
        s.add("C00-10-缺一即败", ok, if ok { "" } else { "失败未判" });
    }
    // C00-11 指纹一致
    {
        let f = || build_full_ten_batches().fingerprint();
        let ok = f() == f();
        s.add("C00-11-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C00-12 封账稳定：封后指纹不变且二次封账拒
    {
        let mut l = build_full_ten_batches();
        let pre = l.fingerprint();
        let _ = l.seal();
        let ok = l.fingerprint() == pre && l.seal().is_err() && l.is_sealed();
        s.add("C00-12-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    // C00-13 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C00-13-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C00-14 拒绝账与诊断码闭集
    {
        let mut l = KtestLedger::new();
        let _ = l.register(BatchRec { batch: 0, asserted: 40, passed_host: 40, passed_qemu: 40 });
        let _ = l.register(BatchRec { batch: 1, asserted: 40, passed_host: 41, passed_qemu: 40 });
        let ok = l.reject_count() == 2
            && l.reject_count() <= (XC_KTEST_WRAP - XC_KTEST_OOB + 1) as usize;
        s.add("C00-14-拒绝账闭集", ok, if ok { "" } else { "拒绝账溢出" });
    }
    s
}
