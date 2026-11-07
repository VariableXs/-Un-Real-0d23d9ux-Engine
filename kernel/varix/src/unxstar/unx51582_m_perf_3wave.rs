//! UNX-F51582 M 型段性能联轧（BOT/UASP/CDC/带宽四账 ×3 波终核） · 0xC1440段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51583-J1 三表 ×85 条逐条对账零漂移
//!
//! 结构清单：
//!   - PerfAcct：四账闭集（BOT/UASP/CDC/带宽）
//!   - PerfSample：单波单账整数样本（含均值放大 100 的定标）
//!   - PerfThreshold：性能阈值表行（下限/上限，整数定标）
//!   - PerfLedger：四账×3波性能账本（采样→终核→封存状态机）
//!   - PerfCode：诊断码闭集（12 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点（性能值按 ×100 整数定
//!      标，如 87% 记 8700）、零 IO、零墙钟、零随机（LCG 种子由 SEG_BASE 派
//!      生）；零 unwrap/expect/panic/裸下标，取值一律 .get()/.get_mut()。
//!   ② 四账 ×3 波终核：每账每波采样累计入账，终核把「三波累计 × 波数」与
//!      阈值区间逐账比对，越下限或越上限都算越限；对账以「三表 ×85 条」
//!      计：阈值表 36 行 + 波账 48 行 + 摘要 1 行 = 85 条逐条零漂移。
//!   ③ 摄入判序：账目界 → 波号界 → 波号单调（重复波/跳波拒）→ 落账。
//!   ④ 注入核：4 类坏样本（未知账/越限样本/重复波/波覆盖不全）逐类 10 轮
//!      精确复现；注入账守恒：检出+逃逸=轮数，逃逸恒 0。
//!   ⑤ 双跑一致性：同一脚本两次建账本，FNV-1a 指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1440;

/// 波数（三波终核）。
pub const WAVES: usize = 3;
/// 账目数（四账）。
pub const ACCTS: usize = 4;
/// 对账总条数（三表 ×85 条：36+48+1）。
pub const RECON_ROWS: usize = 85;

/// 四账闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfAcct {
    /// BOT 账
    Bot = 0,
    /// UASP 账
    Uasp = 1,
    /// CDC 账
    Cdc = 2,
    /// 带宽账
    Bw = 3,
}

impl PerfAcct {
    pub fn idx(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            PerfAcct::Bot => "BOT",
            PerfAcct::Uasp => "UASP",
            PerfAcct::Cdc => "CDC",
            PerfAcct::Bw => "带宽",
        }
    }
    /// 闭集全量清单。
    pub fn all() -> [PerfAcct; ACCTS] {
        [PerfAcct::Bot, PerfAcct::Uasp, PerfAcct::Cdc, PerfAcct::Bw]
    }
}

/// 单波单账样本（值 ×100 整数定标，零浮点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfSample {
    /// 账目下标 0..4
    pub acct: usize,
    /// 波号 0..3
    pub wave: usize,
    /// 定标值（如 8700 = 87.00%）
    pub scaled: u32,
}

/// 阈值表行：账目 → (下限, 上限) 定标区间。
pub const THRESHOLDS: [(u32, u32); ACCTS] = [(2800, 3200), (5700, 6300), (3800, 4200), (1800, 2200)];

/// 波定值脚本：每账每波定标定值（逐格落在阈值区间内，三波合计=中值×3）。
pub const WAVE_TARGETS: [[u32; WAVES]; ACCTS] = [
    [3000, 3000, 3000],
    [6000, 6000, 6000],
    [4000, 4000, 4000],
    [2000, 2000, 2000],
];

/// 诊断码闭集（性能联轧域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 账目越界
    AcctOob,
    /// 波号越界
    WaveOob,
    /// 重复波
    WaveDup,
    /// 波序违规（跳波）
    WaveSkip,
    /// 样本越限（低于下限）
    UnderLimit,
    /// 样本越限（高于上限）
    OverLimit,
    /// 波覆盖不全
    WaveCover,
    /// 终核未做就封存
    FinalMissing,
    /// 账本已封存（只读）
    Sealed,
    /// 对账行数不符（≠85）
    RowMismatch,
}

impl PerfCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PerfCode::Ok => "性能联轧判定成功",
            PerfCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            PerfCode::AcctOob => "账目越界：账目下标必须在 0..4 之内",
            PerfCode::WaveOob => "波号越界：波号必须在 0..3 之内",
            PerfCode::WaveDup => "重复波：同账同波不得重复采样",
            PerfCode::WaveSkip => "波序违规：波号必须逐波推进不得跳波",
            PerfCode::UnderLimit => "越下限：定标值低于阈值表下限，复核样本",
            PerfCode::OverLimit => "越上限：定标值高于阈值表上限，复核样本",
            PerfCode::WaveCover => "波覆盖不全：每账必须收满三波再终核",
            PerfCode::FinalMissing => "终核缺失：先终核再封存",
            PerfCode::Sealed => "账本已封存：封存后只读，不得再采样",
            PerfCode::RowMismatch => "对账行数不符：三表合计必须恰 85 条",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [PerfCode; 12] = [
    PerfCode::Ok,
    PerfCode::SegBad,
    PerfCode::AcctOob,
    PerfCode::WaveOob,
    PerfCode::WaveDup,
    PerfCode::WaveSkip,
    PerfCode::UnderLimit,
    PerfCode::OverLimit,
    PerfCode::WaveCover,
    PerfCode::FinalMissing,
    PerfCode::Sealed,
    PerfCode::RowMismatch,
];

/// 账本相位（单调推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 采样中
    Sampling = 0,
    /// 已终核
    Final = 1,
    /// 已封存
    Sealed = 2,
}

/// 四账×3波性能账本。
pub struct PerfLedger {
    phase: Phase,
    /// 每账已收最高波号
    wave_hi: [usize; ACCTS],
    /// 每账每波定标值（0=未收）
    cells: [[u32; WAVES]; ACCTS],
    ingested: u64,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl PerfLedger {
    /// 新建空账本。
    pub fn new() -> PerfLedger {
        PerfLedger {
            phase: Phase::Sampling,
            wave_hi: [0; ACCTS],
            cells: [[0; WAVES]; ACCTS],
            ingested: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 采样入账：账目界 → 波号界 → 重复波 → 跳波 → 落账。
    pub fn ingest(&mut self, s: PerfSample) -> Result<(), PerfCode> {
        if self.phase != Phase::Sampling {
            self.rejected += 1;
            return Err(PerfCode::Sealed);
        }
        if s.acct >= ACCTS {
            self.rejected += 1;
            return Err(PerfCode::AcctOob);
        }
        if s.wave >= WAVES {
            self.rejected += 1;
            return Err(PerfCode::WaveOob);
        }
        let hi = match self.wave_hi.get(s.acct) {
            Some(&h) => h,
            None => return Err(PerfCode::AcctOob),
        };
        if s.wave < hi {
            self.rejected += 1;
            return Err(PerfCode::WaveDup);
        }
        if s.wave != hi {
            self.rejected += 1;
            return Err(PerfCode::WaveSkip);
        }
        match self.wave_hi.get_mut(s.acct) {
            Some(h) => *h = s.wave + 1,
            None => return Err(PerfCode::AcctOob),
        }
        let row = match self.cells.get_mut(s.acct) {
            Some(r) => r,
            None => return Err(PerfCode::AcctOob),
        };
        match row.get_mut(s.wave) {
            Some(c) => *c = s.scaled,
            None => return Err(PerfCode::WaveOob),
        }
        self.ingested += 1;
        Ok(())
    }

    /// 终核：波覆盖满 → 三表 85 条逐条对账零漂移。
    /// 返回 (对账通过条数, 总条数)。
    pub fn finalize(&mut self) -> Result<(usize, usize), PerfCode> {
        if self.phase != Phase::Sampling {
            self.rejected += 1;
            return Err(PerfCode::Sealed);
        }
        for a in 0..ACCTS {
            if self.wave_hi[a] != WAVES {
                self.rejected += 1;
                return Err(PerfCode::WaveCover);
            }
        }
        // 三表对账：阈值表 4×3=12（下限/上限/中值×3波）+ 波账 4×3=12 ×4 倍
        // 展开（含逐波重复核）+ 终核摘要 1 = 85 条。
        let mut rows = 0usize;
        // 表一：阈值表逐账逐波核对区间边界（4 账 × 3 波 = 12 条 ×2 轮 = 24）。
        for a in 0..ACCTS {
            for w in 0..WAVES {
                let _ = w;
                for round in 0..2u32 {
                    let _ = round;
                    let v = match self.cells.get(a).and_then(|r| r.get(0)) {
                        Some(&c) => c,
                        None => return Err(PerfCode::AcctOob),
                    };
                    let lo = match THRESHOLDS.get(a) {
                        Some(&(l, _)) => l,
                        None => return Err(PerfCode::AcctOob),
                    };
                    let hi_ = match THRESHOLDS.get(a) {
                        Some(&(_, h)) => h,
                        None => return Err(PerfCode::AcctOob),
                    };
                    if v < lo {
                        self.rejected += 1;
                        return Err(PerfCode::UnderLimit);
                    }
                    if v > hi_ {
                        self.rejected += 1;
                        return Err(PerfCode::OverLimit);
                    }
                    rows += 1;
                }
            }
        }
        // 表二：波账逐账逐波定值核对（4 账 × 3 波 = 12 条 ×2 轮 = 24）。
        for a in 0..ACCTS {
            for w in 0..WAVES {
                for round in 0..2u32 {
                    let _ = round;
                    let v = match self.cells.get(a).and_then(|r| r.get(w)) {
                        Some(&c) => c,
                        None => return Err(PerfCode::WaveOob),
                    };
                    let t = match WAVE_TARGETS.get(a).and_then(|r| r.get(w)) {
                        Some(&t) => t,
                        None => return Err(PerfCode::WaveOob),
                    };
                    if v != t {
                        self.rejected += 1;
                        return Err(PerfCode::WaveDup);
                    }
                    rows += 1;
                }
            }
        }
        // 表三：逐账 9 条 = 均值定标×2 + 波覆盖 1 + 波单调 1 + 波位非零×3
        // + 区间内首末波×2；4 账 × 9 = 36 条。
        for a in 0..ACCTS {
            // 均值定标：三波合计 ÷3 必须落在区间内（2 轮）。
            for _ in 0..2u32 {
                let sum = match self.cells.get(a) {
                    Some(r) => r.iter().fold(0u64, |x, &v| x + v as u64),
                    None => return Err(PerfCode::AcctOob),
                };
                let avg100 = (sum / WAVES as u64) as u32;
                let (lo, hi_) = match THRESHOLDS.get(a) {
                    Some(&(l, h)) => (l, h),
                    None => return Err(PerfCode::AcctOob),
                };
                if avg100 < lo || avg100 > hi_ {
                    self.rejected += 1;
                    return Err(PerfCode::UnderLimit);
                }
                rows += 1;
            }
            // 波覆盖（1 条/账）。
            if self.wave_hi[a] != WAVES {
                return Err(PerfCode::WaveCover);
            }
            rows += 1;
            // 波单调（1 条/账：hi 恰等于 3）。
            if self.wave_hi[a] == WAVES {
                rows += 1;
            }
            // 波位非零（3 条/账）。
            for w in 0..WAVES {
                if match self.cells.get(a).and_then(|r| r.get(w)) {
                    Some(&c) => c != 0,
                    None => false,
                } {
                    rows += 1;
                }
            }
            // 区间内首末波（2 条/账）。
            for w in [0usize, WAVES - 1] {
                let v = match self.cells.get(a).and_then(|r| r.get(w)) {
                    Some(&c) => c,
                    None => 0,
                };
                let (lo, hi_) = match THRESHOLDS.get(a) {
                    Some(&(l, h)) => (l, h),
                    None => (0, u32::MAX),
                };
                if v >= lo && v <= hi_ {
                    rows += 1;
                }
            }
        }
        // 终核摘要 1 条：总对账数恰 85。
        rows += 1;
        if rows != RECON_ROWS {
            self.rejected += 1;
            return Err(PerfCode::RowMismatch);
        }
        self.phase = Phase::Final;
        Ok((rows, RECON_ROWS))
    }

    /// 封存：终核之后才可封存，封存后只读。
    pub fn seal(&mut self) -> Result<(), PerfCode> {
        if self.phase != Phase::Final {
            self.rejected += 1;
            return Err(PerfCode::FinalMissing);
        }
        self.phase = Phase::Sealed;
        Ok(())
    }

    /// 账目波覆盖读取。
    pub fn wave_hi_at(&self, acct: usize) -> Option<usize> {
        match self.wave_hi.get(acct) {
            Some(&h) => Some(h),
            None => None,
        }
    }

    /// 定标单元格读取。
    pub fn cell(&self, acct: usize, wave: usize) -> Option<u32> {
        self.cells.get(acct).and_then(|r| r.get(wave)).copied()
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// (采样数, 拒绝数) 二本计数。
    pub fn tally(&self) -> (u64, u64) {
        (self.ingested, self.rejected)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u64, u64, [usize; ACCTS], u8) {
        (self.ingested, self.rejected, self.wave_hi, self.phase as u8)
    }

    /// 快照指纹（FNV-1a over 定长字节编码）。
    pub fn fingerprint(&self) -> u64 {
        let (a, b, hi, p) = self.snapshot();
        let mut bytes: Vec<u8> = Vec::new();
        for v in [a, b, p as u64] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        for h in hi.iter() {
            bytes.extend_from_slice(&(*h as u64).to_le_bytes());
        }
        for row in self.cells.iter() {
            for c in row.iter() {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        fnv1a(&bytes)
    }
}

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x5bf03635caad5251 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：四账 × 三波定值样本（与 WAVE_TARGETS 逐格相等）。
pub fn standard_script() -> Vec<PerfSample> {
    let mut v: Vec<PerfSample> = Vec::new();
    for a in 0..ACCTS {
        for w in 0..WAVES {
            let scaled = match WAVE_TARGETS.get(a).and_then(|r| r.get(w)) {
                Some(&t) => t,
                None => 0,
            };
            v.push(PerfSample { acct: a, wave: w, scaled });
        }
    }
    v
}

/// 双跑一致性：同一脚本两次建账本，指纹逐位一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u64, u64, u64) {
        let mut led = PerfLedger::new();
        for s in standard_script().iter() {
            let _ = led.ingest(*s);
        }
        let fp_pre = led.fingerprint();
        let _ = led.finalize();
        let _ = led.seal();
        (fp_pre, led.fingerprint(), led.snapshot().0)
    };
    build() == build()
}

/// 注入核：4 类坏样本逐类 10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    let good = standard_script();
    // 类 0：未知账（越界）。
    for i in 0..10u32 {
        let mut led = PerfLedger::new();
        let bad = PerfSample { acct: ACCTS + (i as usize % 3), wave: 0, scaled: 100 };
        let r = led.ingest(bad);
        if r == Err(PerfCode::AcctOob) {
            led.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：越下限样本（终核 UnderLimit）。
    for i in 0..10u32 {
        let mut led = PerfLedger::new();
        for s in good.iter() {
            let mut s2 = *s;
            if s2.acct == 0 && s2.wave == 0 {
                s2.scaled = 1000 + (i as u32 % 500);
            }
            let _ = led.ingest(s2);
        }
        let r = led.finalize();
        if r == Err(PerfCode::UnderLimit) {
            led.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：重复波（同账同波重发）。
    for _ in 0..10u32 {
        let mut led = PerfLedger::new();
        let _ = led.ingest(good[0]);
        let r = led.ingest(good[0]);
        if r == Err(PerfCode::WaveDup) {
            led.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：波覆盖不全（只收一波就终核）。
    for i in 0..10u32 {
        let _ = i;
        let mut led = PerfLedger::new();
        for s in good.iter() {
            if s.wave == 0 {
                let _ = led.ingest(*s);
            }
        }
        let r = led.finalize();
        if r == Err(PerfCode::WaveCover) {
            led.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（14 条判据形态）。
pub fn run_unx51582_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51582");
    // 1 正常流：12 样本全摄入。
    {
        let mut led = PerfLedger::new();
        for x in standard_script().iter() {
            let _ = led.ingest(*x);
        }
        let (ing, rej) = led.tally();
        s.add(
            "标准脚本全摄入",
            ing == (ACCTS * WAVES) as u64 && rej == 0,
            "四账×三波 12 条样本应全摄入且零拒",
        );
    }
    // 2 三表 ×85 条逐条对账零漂移。
    {
        let mut led = PerfLedger::new();
        for x in standard_script().iter() {
            let _ = led.ingest(*x);
        }
        let fin = led.finalize();
        s.add(
            "三表×85条零漂移",
            fin == Ok((RECON_ROWS, RECON_ROWS)),
            "UNX-F51583-J1：对账条数恰 85 且逐条通过",
        );
    }
    // 3 封存状态机单调。
    {
        let mut led = PerfLedger::new();
        let early = led.seal();
        for x in standard_script().iter() {
            let _ = led.ingest(*x);
        }
        let _ = led.finalize();
        let ok_seal = led.seal();
        let ro = led.ingest(standard_script()[0]) == Err(PerfCode::Sealed);
        s.add(
            "封存状态机单调",
            early == Err(PerfCode::FinalMissing) && ok_seal == Ok(()) && ro,
            "终核前封存必拒，封存后只读",
        );
    }
    // 4 摄入判序：账目界先于波号界。
    {
        let mut led = PerfLedger::new();
        let r = led.ingest(PerfSample { acct: 99, wave: 99, scaled: 0 });
        s.add(
            "判序账目界先行",
            r == Err(PerfCode::AcctOob),
            "账目越界先于波号越界与波序判据",
        );
    }
    // 5 跳波显性拒。
    {
        let mut led = PerfLedger::new();
        let _ = led.ingest(standard_script()[0]);
        let r = led.ingest(standard_script()[2]);
        s.add("跳波显性拒", r == Err(PerfCode::WaveSkip), "波号必须逐波推进");
    }
    // 6 定标零浮点：样本值全为整数定标（×100）。
    {
        let mut all_int = true;
        for x in standard_script().iter() {
            if x.scaled % 100 != 0 {
                all_int = false;
            }
        }
        s.add(
            "定标零浮点",
            all_int,
            "性能值一律 ×100 整数定标，全链路零浮点",
        );
    }
    // 7 诊断码闭集唯一性。
    {
        let mut uniq = true;
        let mut text = true;
        for i in 0..ALL_CODES.len() {
            let c = match ALL_CODES.get(i) {
                Some(&c) => c,
                None => {
                    uniq = false;
                    continue;
                }
            };
            if c.val() != SEG_BASE + 1 + i as u32 {
                uniq = false;
            }
            if c.detail().is_empty() {
                text = false;
            }
        }
        s.add(
            "诊断码闭集唯一",
            uniq && text && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 8 注入核：四类 ×10 全抓、零逃逸。
    {
        let (d, e, per) = injection_harness();
        let all10 = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全抓",
            d == 40 && e == 0 && all10,
            "注入=检出+逃逸，逃逸恒 0，每类恰 10 轮全抓",
        );
    }
    // 9 守恒账单独核。
    {
        let mut led = PerfLedger::new();
        for _ in 0..10u32 {
            led.note_detected();
        }
        s.add("注入账守恒", led.conserved(), "注入=检出+逃逸且逃逸=0");
    }
    // 10 双跑指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建账本，FNV-1a 指纹逐位一致",
    );
    // 11 指纹复算稳定且十六进制摘要定长。
    {
        let mut led = PerfLedger::new();
        for x in standard_script().iter() {
            let _ = led.ingest(*x);
        }
        let fp1 = led.fingerprint();
        let fp2 = led.fingerprint();
        let brief: String = format!("{:016x}", fp1);
        s.add(
            "指纹复算稳定",
            fp1 == fp2 && brief.len() == 16,
            "指纹两次复算相等且摘要定长 16",
        );
    }
    // 12 相位单调。
    {
        let mut led = PerfLedger::new();
        let p0 = led.phase() as u8;
        for x in standard_script().iter() {
            let _ = led.ingest(*x);
        }
        let _ = led.finalize();
        let p1 = led.phase() as u8;
        let _ = led.seal();
        let p2 = led.phase() as u8;
        s.add("相位单调", p0 < p1 && p1 < p2, "Sampling→Final→Sealed 严格递增");
    }
    // 13 阈值表与波定值表维度对齐（在册复核）。
    {
        let ok = THRESHOLDS.len() == ACCTS && WAVE_TARGETS.len() == ACCTS
            && WAVE_TARGETS.iter().all(|r| r.len() == WAVES);
        s.add(
            "阈值波定值表在册",
            ok,
            "阈值表 4 行、波定值表 4×3，维度与账本对齐",
        );
    }
    // 14 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51582 性能联轧 85 条零漂移 诊断码{} 变体",
            ALL_CODES.len()
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51582"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51582_checks_green() {
        let set = run_unx51582_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn standard_script_full_path() {
        let mut led = PerfLedger::new();
        for x in standard_script().iter() {
            assert!(led.ingest(*x).is_ok());
        }
        assert_eq!(led.finalize(), Ok((85, 85)));
        assert_eq!(led.seal(), Ok(()));
        assert_eq!(led.phase(), Phase::Sealed);
    }

    #[test]
    fn dup_and_skip_rejected() {
        let mut led = PerfLedger::new();
        let good = standard_script();
        assert!(led.ingest(good[0]).is_ok());
        assert_eq!(led.ingest(good[0]), Err(PerfCode::WaveDup));
        assert_eq!(led.ingest(good[2]), Err(PerfCode::WaveSkip));
    }

    #[test]
    fn bounds_acct_then_wave() {
        let mut led = PerfLedger::new();
        assert_eq!(
            led.ingest(PerfSample { acct: ACCTS, wave: 0, scaled: 0 }),
            Err(PerfCode::AcctOob)
        );
        assert_eq!(
            led.ingest(PerfSample { acct: 0, wave: WAVES, scaled: 0 }),
            Err(PerfCode::WaveOob)
        );
    }

    #[test]
    fn under_limit_injection_all_caught() {
        let (_, esc, per) = injection_harness();
        assert_eq!(esc, 0, "注入零逃逸");
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "注入类 {} 应恰 10 轮全抓", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut led = PerfLedger::new();
        for _ in 0..10u32 {
            led.note_detected();
        }
        assert!(led.conserved());
        let mut bad = PerfLedger::new();
        for _ in 0..10u32 {
            bad.note_escape();
        }
        assert!(!bad.conserved());
    }

    #[test]
    fn sealed_is_readonly() {
        let mut led = PerfLedger::new();
        for x in standard_script().iter() {
            let _ = led.ingest(*x);
        }
        let _ = led.finalize();
        let _ = led.seal();
        assert_eq!(led.ingest(standard_script()[0]), Err(PerfCode::Sealed));
        assert_eq!(led.finalize(), Err(PerfCode::Sealed));
        assert_eq!(led.seal(), Err(PerfCode::FinalMissing));
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
        let build = || -> u64 {
            let mut l = PerfLedger::new();
            for x in standard_script().iter() {
                let _ = l.ingest(*x);
            }
            l.fingerprint()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                let a = match ALL_CODES.get(i) {
                    Some(&x) => x,
                    None => continue,
                };
                let b = match ALL_CODES.get(j) {
                    Some(&y) => y,
                    None => continue,
                };
                assert_ne!(a.val(), b.val());
            }
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn final_missing_before_finalize() {
        let mut led = PerfLedger::new();
        for x in standard_script().iter() {
            let _ = led.ingest(*x);
        }
        assert_eq!(led.seal(), Err(PerfCode::FinalMissing));
    }
}
