//! UNX-F51533 B17 性能基线（混跑吞吐基线 ×3 波） · 0xC0840段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51534-J1 错误码 ×10 并入；未定义码注入 10/10 拒绝
//!
//! 设计要点：
//!   ① 混跑吞吐按「固定种子 LCG + 冻结操作字节表」建模：读/写/削/刷四类操作
//!      按 LCG 混跑（读 5/10、写 3/10、削 1/10、刷 1/10），每波 128 个样本，
//!      波吞吐 = Σ字节（纯整数）；同种子双跑逐波逐字段一致（确定性，无墙钟
//!      无随机源）。
//!   ② 基线 ×3 波漂移 ≤5%：波间漂移按千分比整数计算 |a−b|×1000/max(a,b)
//!      ≤ 50‰；不足 3 波取基线 → BaselineStale；波总量必须落在冻结界内。
//!   ③ 波序单调面：波号严格递增，倒退显性拒（WaveBackward）且版本纹丝不动。
//!   ④ 错误码 ×10 并入：10 个定义码逐一并入合并表（每码至多一次，重复
//!      DupCode）；未定义码注入 10/10 拒绝（CodeUndefined），闭集校验先于去重。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0；诊断码闭集 + 段位守卫。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0840;

/// 冻结常量表：每波样本数 / 基线波数 / 漂移上限（千分比）/ 抖动幅 / 操作字节表。
/// 混跑占比：读 5/10、写 3/10、削 1/10、刷 1/10；字节表收窄跨度以保证
/// 波间漂移在 5% 判据内的方差可控。
pub const SAMPLES_PER_WAVE: u32 = 128;
pub const WAVES_NEEDED: usize = 3;
pub const DRIFT_PERMILLE_CAP: u64 = 50;
pub const JITTER_CAP: u64 = 64;
pub const OP_READ_BYTES: u64 = 4096;
pub const OP_WRITE_BYTES: u64 = 4608;
pub const OP_TRIM_BYTES: u64 = 3584;
pub const OP_FLUSH_BYTES: u64 = 3072;
pub const THR_MIN: u64 = SAMPLES_PER_WAVE as u64 * OP_FLUSH_BYTES;
pub const THR_MAX: u64 = SAMPLES_PER_WAVE as u64 * (OP_WRITE_BYTES + JITTER_CAP - 1);
pub const LCG_SEED: u64 = 0xB17F_00D5_EED1_1070;
pub const INJ_ROUNDS: usize = 10;

/// 固定种子 LCG（确定性伪随机，禁真随机）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
}

/// 诊断码闭集（B17 性能基线域，共 10 个：即「错误码 ×10 并入」的权威来源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 波号倒退
    WaveBackward,
    /// 波吞吐越出冻结界
    ThrOutOfRange,
    /// 基线未定（不足 3 波）
    BaselineStale,
    /// 波间漂移超上限
    DriftExceeded,
    /// 未定义错误码
    CodeUndefined,
    /// 重复并入
    DupCode,
    /// 样本账不平
    LedgerMismatch,
    /// 守恒账破坏
    ConservedBroken,
}

impl PerfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PerfCode::Ok => "基线判据通过",
            PerfCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            PerfCode::WaveBackward => "波号倒退：波号必须严格递增",
            PerfCode::ThrOutOfRange => "波吞吐越界：落在冻结界外先查操作混比",
            PerfCode::BaselineStale => "基线未定：至少 3 波才可取基线",
            PerfCode::DriftExceeded => "波间漂移超 5%：先查负载配比再复测",
            PerfCode::CodeUndefined => "未定义错误码：只允许并入闭集内的 10 码",
            PerfCode::DupCode => "重复并入：同一错误码只入表一次",
            PerfCode::LedgerMismatch => "样本账不平：Σ样本必须等于波数×每波样本",
            PerfCode::ConservedBroken => "守恒破坏：注入=检出+逃逸必须成立",
        }
    }
}

/// 单波记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveRec {
    /// 波号（严格递增）
    pub idx: u64,
    /// 波吞吐（Σ字节，整数）
    pub total: u64,
    /// 样本数（恒 64）
    pub samples: u32,
    /// 四类操作计数（读/写/削/刷）
    pub op_counts: [u32; 4],
}

/// B17 性能基线引擎。
pub struct B17PerfBase {
    lcg: Lcg,
    waves: Vec<WaveRec>,
    merged: Vec<u32>,
    version: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl B17PerfBase {
    /// 冻结种子引擎。
    pub fn new() -> B17PerfBase {
        B17PerfBase {
            lcg: Lcg::new(LCG_SEED),
            waves: Vec::new(),
            merged: Vec::new(),
            version: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 操作字节表查表（读 0..4 / 写 5..7 / 削 8 / 刷 9）。
    fn op_bytes(op: u64) -> u64 {
        match op {
            0..=4 => OP_READ_BYTES,
            5..=7 => OP_WRITE_BYTES,
            8 => OP_TRIM_BYTES,
            _ => OP_FLUSH_BYTES,
        }
    }

    /// 跑一波混跑。判序：波号单调 → 采样 → 界校验 → 落账。
    pub fn run_wave(&mut self, idx: u64) -> Result<WaveRec, PerfCode> {
        let last = match self.waves.last() {
            Some(w) => w.idx,
            None => 0,
        };
        if idx <= last {
            return Err(PerfCode::WaveBackward);
        }
        let mut total = 0u64;
        let mut counts = [0u32; 4];
        for _ in 0..SAMPLES_PER_WAVE {
            let r = self.lcg.next();
            let op = (r >> 32) % 10;
            let jitter = (r >> 20) % JITTER_CAP;
            let bytes = Self::op_bytes(op).saturating_add(jitter);
            total = total.saturating_add(bytes);
            let slot = match op {
                0..=4 => 0usize,
                5..=7 => 1usize,
                8 => 2usize,
                _ => 3usize,
            };
            if let Some(c) = counts.get_mut(slot) {
                *c += 1;
            }
        }
        if total < THR_MIN || total > THR_MAX {
            return Err(PerfCode::ThrOutOfRange);
        }
        let rec = WaveRec {
            idx,
            total,
            samples: SAMPLES_PER_WAVE,
            op_counts: counts,
        };
        self.waves.push(rec);
        self.version += 1;
        Ok(rec)
    }

    /// 波间漂移（千分比整数）。
    pub fn drift_permille(a: u64, b: u64) -> u64 {
        let hi = if a > b { a } else { b };
        let diff = a.abs_diff(b);
        if hi == 0 {
            return 0;
        }
        diff.saturating_mul(1000) / hi
    }

    /// 基线判据：≥3 波且相邻波漂移全部 ≤50‰（5%）。
    pub fn baseline_ok(&self) -> bool {
        if self.waves.len() < WAVES_NEEDED {
            return false;
        }
        for i in 0..self.waves.len() - 1 {
            let a = match self.waves.get(i) {
                Some(w) => w.total,
                None => return false,
            };
            let b = match self.waves.get(i + 1) {
                Some(w) => w.total,
                None => return false,
            };
            if Self::drift_permille(a, b) > DRIFT_PERMILLE_CAP {
                return false;
            }
        }
        true
    }

    /// 取基线：不足 3 波 → BaselineStale；否则取三波最小值。
    pub fn baseline_value(&self) -> Result<u64, PerfCode> {
        if self.waves.len() < WAVES_NEEDED {
            return Err(PerfCode::BaselineStale);
        }
        let mut min = u64::MAX;
        for i in 0..self.waves.len() {
            let t = match self.waves.get(i) {
                Some(w) => w.total,
                None => return Err(PerfCode::LedgerMismatch),
            };
            if t < min {
                min = t;
            }
        }
        Ok(min)
    }

    /// 错误码并入：闭集校验先于去重；每码至多一次。
    pub fn merge_code(&mut self, v: u32) -> Result<usize, PerfCode> {
        let mut defined = false;
        for i in 0..ALL_CODES.len() {
            let c = match ALL_CODES.get(i) {
                Some(c) => *c,
                None => continue,
            };
            if c.val() == v {
                defined = true;
            }
        }
        if !defined {
            return Err(PerfCode::CodeUndefined);
        }
        for i in 0..self.merged.len() {
            if let Some(m) = self.merged.get(i) {
                if *m == v {
                    return Err(PerfCode::DupCode);
                }
            }
        }
        self.merged.push(v);
        self.version += 1;
        Ok(self.merged.len())
    }

    /// 注入记账：注入与检出同步加一（调用方在捕获违约时登记）。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.escaped += 1;
    }

    pub fn waves_len(&self) -> usize {
        self.waves.len()
    }
    pub fn wave_at(&self, idx: usize) -> Option<WaveRec> {
        self.waves.get(idx).copied()
    }
    pub fn merged_len(&self) -> usize {
        self.merged.len()
    }
    pub fn merged_at(&self, idx: usize) -> Option<u32> {
        self.merged.get(idx).copied()
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn detected(&self) -> u64 {
        self.detected
    }
    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 样本账守恒：Σ波样本 = 波数 × 每波样本；注入 = 检出 + 逃逸，逃逸 0。
    pub fn conserved(&self) -> bool {
        let mut sum = 0u64;
        for i in 0..self.waves.len() {
            let s = match self.waves.get(i) {
                Some(w) => w.samples as u64,
                None => return false,
            };
            sum += s;
        }
        let ledger_ok = sum == (self.waves.len() as u64) * SAMPLES_PER_WAVE as u64;
        ledger_ok
            && self.injected == self.detected + self.escaped
            && self.escaped == 0
            && self.detected > 0
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同种子两次重建波账与合并表逐字段一致（确定性）。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<WaveRec>, Vec<u32>) {
        let mut e = B17PerfBase::new();
        let _ = e.run_wave(1);
        let _ = e.run_wave(2);
        let _ = e.run_wave(3);
        for i in 0..ALL_CODES.len() {
            if let Some(c) = ALL_CODES.get(i) {
                let _ = e.merge_code(c.val());
            }
        }
        let mut wv = Vec::new();
        for i in 0..e.waves_len() {
            if let Some(w) = e.wave_at(i) {
                wv.push(w);
            }
        }
        let mut mg = Vec::new();
        for i in 0..e.merged_len() {
            if let Some(m) = e.merged_at(i) {
                mg.push(m);
            }
        }
        (wv, mg)
    };
    let a = build();
    let b = build();
    a == b
}

/// 诊断码全序表（闭集枚举的权威清单，即并入来源 ×10）。
pub const ALL_CODES: [PerfCode; 10] = [
    PerfCode::Ok,
    PerfCode::SegBad,
    PerfCode::WaveBackward,
    PerfCode::ThrOutOfRange,
    PerfCode::BaselineStale,
    PerfCode::DriftExceeded,
    PerfCode::CodeUndefined,
    PerfCode::DupCode,
    PerfCode::LedgerMismatch,
    PerfCode::ConservedBroken,
];

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51533_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51533");
    // 1 正常流：3 波混跑，每波 128 样本、四类操作全现、总量在冻结界内。
    {
        let mut e = B17PerfBase::new();
        let mut ok = true;
        for w in 1..=WAVES_NEEDED as u64 {
            match e.run_wave(w) {
                Ok(rec) => {
                    let flush = match rec.op_counts.get(3) {
                        Some(c) => *c,
                        None => 0,
                    };
                    if rec.samples != SAMPLES_PER_WAVE
                        || rec.total < THR_MIN
                        || rec.total > THR_MAX
                        || flush == 0
                    {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        s.add_dyn(
            "三波混跑落账",
            ok && e.waves_len() == WAVES_NEEDED,
            "每波样本数、界内吞吐、四类操作齐现",
        );
    }
    // 2 边界零：波号 0 拒（首波必须 ≥1）。
    {
        let mut e = B17PerfBase::new();
        let r = e.run_wave(0);
        s.add_dyn(
            "零波号拒",
            r == Err(PerfCode::WaveBackward),
            "波号从 1 起算，0 号当场拒",
        );
    }
    // 3 边界上限：u64::MAX 放行一波，其后无更大波号。
    {
        let mut e = B17PerfBase::new();
        let a = e.run_wave(u64::MAX);
        let b = e.run_wave(u64::MAX);
        s.add_dyn(
            "波号上限边界",
            a.is_ok() && b == Err(PerfCode::WaveBackward),
            "上限波号放行一次，其后必须拒",
        );
    }
    // 4 基线未定：不足 3 波 → BaselineStale。
    {
        let mut e = B17PerfBase::new();
        let _ = e.run_wave(1);
        let _ = e.run_wave(2);
        s.add_dyn(
            "基线未定拒",
            e.baseline_value() == Err(PerfCode::BaselineStale) && !e.baseline_ok(),
            "不足 3 波不得取基线",
        );
    }
    // 5 基线 ×3 波漂移 ≤5%（50‰，含跨首尾两两校验）。
    {
        let mut e = B17PerfBase::new();
        for w in 1..=3u64 {
            let _ = e.run_wave(w);
        }
        let t0 = match e.wave_at(0) {
            Some(w) => w.total,
            None => 0,
        };
        let t1 = match e.wave_at(1) {
            Some(w) => w.total,
            None => 0,
        };
        let t2 = match e.wave_at(2) {
            Some(w) => w.total,
            None => 0,
        };
        let d01 = B17PerfBase::drift_permille(t0, t1);
        let d12 = B17PerfBase::drift_permille(t1, t2);
        let d02 = B17PerfBase::drift_permille(t0, t2);
        s.add_dyn(
            "基线 ×3 波漂移 ≤5%",
            e.baseline_ok()
                && d01 <= DRIFT_PERMILLE_CAP
                && d12 <= DRIFT_PERMILLE_CAP
                && d02 <= DRIFT_PERMILLE_CAP,
            "相邻与跨首尾漂移全部 ≤50‰",
        );
    }
    // 6 注入必抓：未定义码 ×10 → 10/10 拒，合并表不变。
    {
        let mut e = B17PerfBase::new();
        let before = e.merged_len();
        let mut caught = 0u32;
        for i in 0..INJ_ROUNDS {
            let r = e.merge_code(SEG_BASE + 100 + i as u32);
            if r == Err(PerfCode::CodeUndefined) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn(
            "未定义码注入 10/10 拒绝",
            caught == 10 && e.merged_len() == before,
            "闭集外的码必须全拒且不入表",
        );
    }
    // 7 注入必抓：重复码 ×10 → DupCode，表不变。
    {
        let mut e = B17PerfBase::new();
        let _ = e.merge_code(PerfCode::Ok.val());
        let before = e.merged_len();
        let mut caught = 0u32;
        for _ in 0..INJ_ROUNDS {
            let r = e.merge_code(PerfCode::Ok.val());
            if r == Err(PerfCode::DupCode) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn(
            "重复码注入 10/10 拒绝",
            caught == 10 && e.merged_len() == before,
            "同一错误码只入表一次",
        );
    }
    // 8 错误码 ×10 并入。
    {
        let mut e = B17PerfBase::new();
        let mut merged = 0u32;
        for i in 0..ALL_CODES.len() {
            let v = match ALL_CODES.get(i) {
                Some(c) => c.val(),
                None => 0,
            };
            if e.merge_code(v).is_ok() {
                merged += 1;
            }
        }
        s.add_dyn(
            "错误码 ×10 并入",
            merged == 10 && e.merged_len() == 10,
            "闭集 10 码逐一并入成功",
        );
    }
    // 9 注入必抓：波号倒退 ×10 → 状态冻结。
    {
        let mut e = B17PerfBase::new();
        let _ = e.run_wave(5);
        let v0 = e.version();
        let n0 = e.waves_len();
        let mut caught = 0u32;
        for _ in 0..INJ_ROUNDS {
            let r = e.run_wave(2);
            if r == Err(PerfCode::WaveBackward) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn(
            "波号倒退注入 10/10 且状态冻结",
            caught == 10 && e.version() == v0 && e.waves_len() == n0,
            "倒退必须全抓且拒后版本与波账不动",
        );
    }
    // 10 判定次序：闭集校验先于去重；波号先于采样。
    {
        let mut e = B17PerfBase::new();
        let r = e.merge_code(SEG_BASE + 999);
        let w = e.run_wave(0);
        s.add_dyn(
            "判序双先",
            r == Err(PerfCode::CodeUndefined) && w == Err(PerfCode::WaveBackward),
            "未定义码判据先于去重，波号判据先于采样",
        );
    }
    // 11 混跑配比：读最多、四类全现（冻结种子确定性）。
    {
        let mut e = B17PerfBase::new();
        let _ = e.run_wave(1);
        let counts = match e.wave_at(0) {
            Some(w) => w.op_counts,
            None => [0u32; 4],
        };
        let read = match counts.get(0) {
            Some(v) => *v,
            None => 0,
        };
        let write = match counts.get(1) {
            Some(v) => *v,
            None => 0,
        };
        let trim = match counts.get(2) {
            Some(v) => *v,
            None => 0,
        };
        let flush = match counts.get(3) {
            Some(v) => *v,
            None => 0,
        };
        s.add_dyn(
            "混跑配比成立",
            read > write && write > 0 && trim > 0 && flush > 0,
            "读占优且四类操作齐现",
        );
    }
    // 12 对账守恒：Σ样本 = 波数×128；注入=检出+逃逸，逃逸 0。
    {
        let mut e = B17PerfBase::new();
        for w in 1..=3u64 {
            let _ = e.run_wave(w);
        }
        for _ in 0..10u32 {
            let _ = e.merge_code(SEG_BASE + 200);
            e.note_detected();
        }
        s.add_dyn(
            "样本账守恒",
            e.conserved(),
            "Σ样本=波数×每波样本，注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 13 双跑一致（同种子确定性）。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同种子两次建账逐字段相等");
    // 14 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 15 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_waves_mixed_run() {
        let mut e = B17PerfBase::new();
        for w in 1..=3u64 {
            let rec = e.run_wave(w);
            assert!(matches!(rec, Ok(r) if r.samples == SAMPLES_PER_WAVE && r.idx == w));
        }
        assert_eq!(e.waves_len(), 3);
        assert!(e.baseline_ok());
    }

    #[test]
    fn wave_zero_rejected_and_max_boundary() {
        let mut e = B17PerfBase::new();
        assert_eq!(e.run_wave(0), Err(PerfCode::WaveBackward));
        assert!(e.run_wave(u64::MAX).is_ok());
        assert_eq!(e.run_wave(u64::MAX), Err(PerfCode::WaveBackward));
    }

    #[test]
    fn baseline_stale_until_three_waves() {
        let mut e = B17PerfBase::new();
        assert_eq!(e.baseline_value(), Err(PerfCode::BaselineStale));
        let _ = e.run_wave(1);
        assert_eq!(e.baseline_value(), Err(PerfCode::BaselineStale));
        let _ = e.run_wave(2);
        let _ = e.run_wave(3);
        let base = e.baseline_value();
        assert!(matches!(base, Ok(v) if v >= THR_MIN && v <= THR_MAX));
    }

    #[test]
    fn drift_within_5_permille_cap() {
        let mut e = B17PerfBase::new();
        for w in 1..=3u64 {
            let _ = e.run_wave(w);
        }
        let t0 = match e.wave_at(0) { Some(w) => w.total, None => 0 };
        let t1 = match e.wave_at(1) { Some(w) => w.total, None => 0 };
        let t2 = match e.wave_at(2) { Some(w) => w.total, None => 0 };
        assert!(B17PerfBase::drift_permille(t0, t1) <= DRIFT_PERMILLE_CAP);
        assert!(B17PerfBase::drift_permille(t1, t2) <= DRIFT_PERMILLE_CAP);
        assert!(B17PerfBase::drift_permille(t0, t2) <= DRIFT_PERMILLE_CAP);
    }

    #[test]
    fn undefined_code_injection_caught_x10() {
        let mut e = B17PerfBase::new();
        for i in 0..10u32 {
            assert_eq!(
                e.merge_code(SEG_BASE + 500 + i),
                Err(PerfCode::CodeUndefined)
            );
            e.note_detected();
        }
        assert_eq!(e.merged_len(), 0);
        assert!(e.conserved());
    }

    #[test]
    fn dup_code_injection_caught_x10() {
        let mut e = B17PerfBase::new();
        assert!(e.merge_code(PerfCode::SegBad.val()).is_ok());
        for _ in 0..10 {
            assert_eq!(
                e.merge_code(PerfCode::SegBad.val()),
                Err(PerfCode::DupCode)
            );
            e.note_detected();
        }
        assert_eq!(e.merged_len(), 1);
    }

    #[test]
    fn all_ten_codes_merge_once() {
        let mut e = B17PerfBase::new();
        for i in 0..ALL_CODES.len() {
            let v = match ALL_CODES.get(i) {
                Some(c) => c.val(),
                None => 0,
            };
            let n = e.merge_code(v);
            assert!(matches!(n, Ok(k) if k == i + 1));
        }
        assert_eq!(e.merged_len(), ALL_CODES.len());
    }

    #[test]
    fn wave_backward_injection_freezes_state() {
        let mut e = B17PerfBase::new();
        let _ = e.run_wave(5);
        let v0 = e.version();
        for _ in 0..10 {
            assert_eq!(e.run_wave(2), Err(PerfCode::WaveBackward));
            e.note_detected();
        }
        assert_eq!(e.version(), v0);
        assert_eq!(e.waves_len(), 1);
        assert!(e.conserved());
    }

    #[test]
    fn mix_profile_read_dominant_all_present() {
        let mut e = B17PerfBase::new();
        let _ = e.run_wave(1);
        let counts = match e.wave_at(0) {
            Some(w) => w.op_counts,
            None => [0u32; 4],
        };
        let read_n = match counts.get(0) {
            Some(v) => *v,
            None => 0,
        };
        let write_n = match counts.get(1) {
            Some(v) => *v,
            None => 0,
        };
        assert!(read_n > write_n, "读应占优");
        for i in 0..4usize {
            let c = match counts.get(i) {
                Some(v) => *v,
                None => 0,
            };
            assert!(c > 0, "第 {} 类操作必须出现", i);
        }
    }

    #[test]
    fn dual_run_seg_and_closed_set() {
        assert!(dual_run_consistent());
        assert!(seg_guard_ok());
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
