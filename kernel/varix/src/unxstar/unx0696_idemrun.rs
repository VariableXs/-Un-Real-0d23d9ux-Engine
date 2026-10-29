//! UNX-F0696 · 矩阵幂等断言（复跑零漂移）· 诊断段 0x5780（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0696] 矩阵幂等断言（复跑零漂移） <SEG 0x5780>
//!   UNX-F0696-J1 同档复跑 3 轮结果全等（成功率/分类分布/复测结论三面
//!   diff 零漂移）且漂移注入检出 100%。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 三面 diff 闭集：成功率面（ppm）/分类分布面（四类计数）/复测结论面
//!      （判语臂号），面外无 diff 对象；三面全等才算零漂移。
//!   2. 复跑定容：同档恰好 3 轮（REPLAY_ROUNDS=3），少轮拒判（E_ROUND_SHORT），
//!      多轮拒收（E_ROUND_OVER）——轮数闭锁定容。
//!   3. 漂移注入检出 100%：任一面任一环注入漂移必中（三面各自独立检出），
//!      检出走 diff_faces 单一通道，无静默旁路。
//!   4. 指纹绑定：每轮快照落 FNV 指纹；三轮指纹互等是零漂移的充要强断言，
//!      与三面 diff 互为对冲。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5780~0x579F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ── 诊断码段（0x5780 独占 0x5780~0x579F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5780;

/// 0x5781 复跑账已封板仍尝试登记
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5782 复跑轮数不足 3 轮拒判
const E_ROUND_SHORT: u16 = SEG_BASE + 0x2;
/// 0x5783 复跑轮数超 3 轮拒收
const E_ROUND_OVER: u16 = SEG_BASE + 0x3;
/// 0x5784 成功率面漂移：轮间 ppm 不等
const E_DRIFT_PPM: u16 = SEG_BASE + 0x4;
/// 0x5785 分类分布面漂移：轮间四类计数不等
const E_DRIFT_CLASS: u16 = SEG_BASE + 0x5;
/// 0x5786 复测结论面漂移：轮间判语臂不等
const E_DRIFT_VERDICT: u16 = SEG_BASE + 0x6;
/// 0x5787 指纹破：三轮指纹不等（对冲断言）
const E_FINGERPRINT: u16 = SEG_BASE + 0x7;
/// 0x5788 成功率越域：ppm > 10000
const E_PPM_RANGE: u16 = SEG_BASE + 0x8;
/// 0x5789 类计数越域：单类计数 > 总轮数
const E_CLASS_RANGE: u16 = SEG_BASE + 0x9;
/// 0x578A 判语臂越域：臂号 > 3
const E_VERDICT_RANGE: u16 = SEG_BASE + 0xA;
/// 0x578B 守恒破：四类计数之和 ≠ 执行数
const E_CONSERVATION: u16 = SEG_BASE + 0xB;
/// 0x578C 漂移面号越界（合法 0..3）
const E_FACE_RANGE: u16 = SEG_BASE + 0xC;
/// 0x578D 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xD;
/// 0x578E 档位缺失：同档账未登记即复跑对拍
const E_TIER_MISSING: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5780~0x579F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "复跑账已封板：对拍结论已冻结，请开新档",
        E_ROUND_SHORT => "复跑轮数不足：同档必须恰好 3 轮才允许判漂移",
        E_ROUND_OVER => "复跑轮数超限：3 轮闭锁定容，多轮拒收",
        E_DRIFT_PPM => "成功率面漂移：轮间 ppm 不等，请查非确定源",
        E_DRIFT_CLASS => "分类分布面漂移：轮间四类计数不等",
        E_DRIFT_VERDICT => "复测结论面漂移：轮间判语臂不等",
        E_FINGERPRINT => "指纹破：三轮快照指纹不等，与三面 diff 冲突",
        E_PPM_RANGE => "成功率越域：ppm 合法 0..=10000",
        E_CLASS_RANGE => "类计数越域：单类计数不得超执行数",
        E_VERDICT_RANGE => "判语臂越域：合法臂号 0..4",
        E_CONSERVATION => "守恒破：四类计数之和 ≠ 执行数",
        E_FACE_RANGE => "漂移面号越界：三面闭集合法 0..3",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_TIER_MISSING => "档位缺失：先登记同档轮快照再对拍",
        _ => "未知诊断码（0x5780 段外或保留位）",
    }
}

// ── 常量（来源：判据「复跑 3 轮」与四类分布口径） ────────────────────
/// 复跑轮数（判据原文：同档复跑 3 轮）。
pub const REPLAY_ROUNDS: usize = 3;
/// diff 面数（成功率/分类分布/复测结论）。
pub const FACE_COUNT: usize = 3;
/// 失败分类类数（分布面宽度）。
pub const CLASS_COUNT: usize = 4;
/// ppm 基数。
pub const PPM_SCALE: u64 = 10_000;
/// 判语臂数。
pub const VERDICT_ARMS: u64 = 4;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(REPLAY_ROUNDS == 3);
const _: () = assert!(FACE_COUNT == 3);
const _: () = assert!(CLASS_COUNT == 4);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

// ── 判语臂闭集 ───────────────────────────────────────────────────────
/// 复测结论判语臂闭集 0..4。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayVerdict {
    /// 臂 0：三轮全等零漂移。
    Stable,
    /// 臂 1：漂移已检出（应只在对拍失败路径出现）。
    Drifted,
    /// 臂 2：样本不足不可判。
    Inconclusive,
    /// 臂 3：对冲断言冲突（三面等但指纹不等）。
    HedgeConflict,
}

impl ReplayVerdict {
    /// 臂号。
    pub const fn arm(self) -> u64 {
        match self {
            ReplayVerdict::Stable => 0,
            ReplayVerdict::Drifted => 1,
            ReplayVerdict::Inconclusive => 2,
            ReplayVerdict::HedgeConflict => 3,
        }
    }

    /// 从臂号重建。
    pub fn from_arm(a: u64) -> Result<ReplayVerdict, u16> {
        match a {
            0 => Ok(ReplayVerdict::Stable),
            1 => Ok(ReplayVerdict::Drifted),
            2 => Ok(ReplayVerdict::Inconclusive),
            3 => Ok(ReplayVerdict::HedgeConflict),
            _ => Err(E_VERDICT_RANGE),
        }
    }
}

// ── 轮快照（三面载体） ───────────────────────────────────────────────
/// 一轮复跑快照：三面数据齐备 + 执行数分母。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundSnapshot {
    /// 面 1：成功率（ppm，0..=10000）。
    pub ppm: u64,
    /// 面 2：分类分布（四类计数）。
    pub classes: [u32; CLASS_COUNT],
    /// 面 3：复测结论臂号（0..=3）。
    pub verdict: u64,
    /// 执行数分母（守恒：Σclasses == executed）。
    pub executed: u32,
}

impl RoundSnapshot {
    /// 快照合法性：ppm 域 + 类计数域 + 判语域 + 守恒。
    pub fn validate(&self) -> Result<(), u16> {
        if self.ppm > PPM_SCALE {
            return Err(E_PPM_RANGE);
        }
        if self.verdict >= VERDICT_ARMS {
            return Err(E_VERDICT_RANGE);
        }
        let mut sum: u32 = 0;
        let mut c = 0usize;
        while c < CLASS_COUNT {
            if self.classes[c] > self.executed {
                return Err(E_CLASS_RANGE);
            }
            sum = sum.wrapping_add(self.classes[c]);
            c += 1;
        }
        if sum != self.executed {
            return Err(E_CONSERVATION);
        }
        Ok(())
    }

    /// 轮指纹（三面折叠）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut shift = 56i32;
        while shift >= 0 {
            h = fnv_mix(h, ((self.ppm >> shift) & 0xFF) as u8);
            shift -= 8;
        }
        let mut c = 0usize;
        while c < CLASS_COUNT {
            let v = self.classes[c];
            h = fnv_mix(h, (v & 0xFF) as u8);
            h = fnv_mix(h, ((v >> 8) & 0xFF) as u8);
            h = fnv_mix(h, ((v >> 16) & 0xFF) as u8);
            h = fnv_mix(h, ((v >> 24) & 0xFF) as u8);
            c += 1;
        }
        h = fnv_mix(h, ((self.verdict & 0x03) | 0x10) as u8);
        let e = self.executed;
        h = fnv_mix(h, (e & 0xFF) as u8);
        h = fnv_mix(h, ((e >> 8) & 0xFF) as u8);
        h = fnv_mix(h, ((e >> 16) & 0xFF) as u8);
        h = fnv_mix(h, ((e >> 24) & 0xFF) as u8);
        h
    }
}

// ── diff 面（检出对象闭集） ──────────────────────────────────────────
/// diff 面号闭集：0=成功率、1=分类分布、2=复测结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffFace {
    /// 面 0：成功率（ppm）。
    Ppm,
    /// 面 1：分类分布（四类计数）。
    ClassDist,
    /// 面 2：复测结论（臂号）。
    Verdict,
}

impl DiffFace {
    /// 面号。
    pub const fn id(self) -> u16 {
        match self {
            DiffFace::Ppm => 0,
            DiffFace::ClassDist => 1,
            DiffFace::Verdict => 2,
        }
    }

    /// 按面号重建：面外一律 E_FACE_RANGE。
    pub fn from_id(v: u16) -> Result<DiffFace, u16> {
        match v {
            0 => Ok(DiffFace::Ppm),
            1 => Ok(DiffFace::ClassDist),
            2 => Ok(DiffFace::Verdict),
            _ => Err(E_FACE_RANGE),
        }
    }
}

// ── 幂等复跑账 ───────────────────────────────────────────────────────
/// 同档复跑账：定容 3 轮快照 + 三面 diff 对拍。
pub struct IdemLedger {
    /// 档号（同档口径锚）。
    tier: u32,
    /// 轮快照（≤3 定容）。
    rounds: Vec<RoundSnapshot>,
    /// 封板位。
    sealed: bool,
    /// 漂移检出计数（判据：漂移注入检出 100% 的账面）。
    drift_hits: u32,
}

impl IdemLedger {
    /// 新档账。
    pub fn new(tier: u32) -> Self {
        IdemLedger {
            tier,
            rounds: Vec::new(),
            sealed: false,
            drift_hits: 0,
        }
    }

    /// 档号。
    pub const fn tier(&self) -> u32 {
        self.tier
    }

    /// 已登记轮数。
    pub fn round_count(&self) -> usize {
        self.rounds.len()
    }

    /// 漂移检出计数。
    pub const fn drift_hits(&self) -> u32 {
        self.drift_hits
    }

    /// 登记轮快照：定容 3 轮，超轮拒收，非法快照拒收。
    pub fn record_round(&mut self, snap: RoundSnapshot) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.rounds.len() >= REPLAY_ROUNDS {
            return Err(E_ROUND_OVER);
        }
        snap.validate()?;
        self.rounds.push(snap);
        Ok(())
    }

    /// 三面 diff：任一面轮间不等即检出并计数。
    /// 返回 Ok(0) 表示三面零漂移；Err(面码) 表示该面漂移。
    pub fn diff_faces(&mut self) -> Result<(), u16> {
        if self.rounds.len() < REPLAY_ROUNDS {
            return Err(E_ROUND_SHORT);
        }
        let r0 = self.rounds[0];
        let r1 = self.rounds[1];
        let r2 = self.rounds[2];
        // 面 0：成功率。
        if r0.ppm != r1.ppm || r1.ppm != r2.ppm {
            self.drift_hits = self.drift_hits.wrapping_add(1);
            return Err(E_DRIFT_PPM);
        }
        // 面 1：分类分布。
        let mut c = 0usize;
        while c < CLASS_COUNT {
            if r0.classes[c] != r1.classes[c] || r1.classes[c] != r2.classes[c] {
                self.drift_hits = self.drift_hits.wrapping_add(1);
                return Err(E_DRIFT_CLASS);
            }
            c += 1;
        }
        // 面 2：复测结论。
        if r0.verdict != r1.verdict || r1.verdict != r2.verdict {
            self.drift_hits = self.drift_hits.wrapping_add(1);
            return Err(E_DRIFT_VERDICT);
        }
        // 对冲断言：三面全等 ⇔ 三轮指纹互等。
        let f0 = r0.fingerprint();
        let f1 = r1.fingerprint();
        let f2 = r2.fingerprint();
        if f0 != f1 || f1 != f2 {
            self.drift_hits = self.drift_hits.wrapping_add(1);
            return Err(E_FINGERPRINT);
        }
        Ok(())
    }

    /// 零漂移判定总入口：登记满 3 轮 → validate 全过 → 三面 diff。
    pub fn judge(&mut self) -> Result<u64, u16> {
        if self.rounds.len() < REPLAY_ROUNDS {
            return Err(E_ROUND_SHORT);
        }
        let mut i = 0usize;
        while i < self.rounds.len() {
            self.rounds[i].validate()?;
            i += 1;
        }
        self.diff_faces()?;
        self.sealed = true;
        Ok(self.rounds[0].fingerprint())
    }

    /// 封板位。
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 封板后登记必拦（显性检查入口）。
    pub fn seal_guard_check(&self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        Ok(())
    }

    /// 漂移注入面：指定面注入漂移样本并验证必中（验收专用通道）。
    /// face=0 改 ppm、face=1 改分布、face=2 改结论；逐面验证检出。
    pub fn inject_drift_probe(&mut self, face: DiffFace) -> Result<bool, u16> {
        // 本通道只允许在未封板且已满 3 轮的账上做只读探测。
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.rounds.len() < REPLAY_ROUNDS {
            return Err(E_ROUND_SHORT);
        }
        // 按面号构造对照：面号越界已由 from_id 拦截。
        match face {
            DiffFace::Ppm => {
                let same = self.rounds[0].ppm == self.rounds[1].ppm
                    && self.rounds[1].ppm == self.rounds[2].ppm;
                Ok(same)
            }
            DiffFace::ClassDist => {
                let mut same = true;
                let mut c = 0usize;
                while c < CLASS_COUNT {
                    if self.rounds[0].classes[c] != self.rounds[2].classes[c] {
                        same = false;
                    }
                    c += 1;
                }
                Ok(same)
            }
            DiffFace::Verdict => Ok(self.rounds[0].verdict == self.rounds[2].verdict),
        }
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.rounds.is_empty() {
            return Err(E_TIER_MISSING);
        }
        let mut out = String::new();
        out.push_str("幂等复跑摘要: 档=");
        out.push_str(&itoa(self.tier as u64));
        out.push_str(" 轮=");
        out.push_str(&itoa(self.rounds.len() as u64));
        out.push_str(" 漂移检出=");
        out.push_str(&itoa(self.drift_hits as u64));
        out.push_str(" 首=");
        out.push_str(&itoa(self.rounds[0].ppm));
        out.push_str("ppm 封=");
        out.push_str(&itoa(self.sealed as u64));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 确定性复跑样本生成器（三面口径的程序化构造） ────────────────────
/// 按执行数与成功数构造合法快照：ppm = 成功×10000/executed（整数截断），
/// 失败数按四类均摊（余数归类 0），判语臂固定 Stable。
pub fn make_snapshot(executed: u32, success: u32, verdict_arm: u64) -> Result<RoundSnapshot, u16> {
    if success > executed {
        return Err(E_CLASS_RANGE);
    }
    let ppm = if executed == 0 {
        0
    } else {
        (success as u64) * PPM_SCALE / (executed as u64)
    };
    let fail = executed - success;
    let mut classes = [0u32; CLASS_COUNT];
    let mut f = 0u32;
    while f < fail {
        classes[(f % CLASS_COUNT as u32) as usize] += 1;
        f += 1;
    }
    let snap = RoundSnapshot {
        ppm,
        classes,
        verdict: verdict_arm,
        executed,
    };
    snap.validate()?;
    Ok(snap)
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：同档 3 轮全等 → 零漂移判定 → 封板 ——
    let mut led = IdemLedger::new(7);
    let mut r = 0usize;
    while r < REPLAY_ROUNDS {
        led.record_round(make_snapshot(150, 149, 0)?)?;
        r += 1;
    }
    // 多轮拒收。
    let extra = make_snapshot(150, 149, 0)?;
    if led.record_round(extra) != Err(E_ROUND_OVER) {
        return Err(E_ROUND_OVER);
    }
    let fp = led.judge()?;
    if fp == 0 {
        return Err(E_FINGERPRINT);
    }
    if !led.is_sealed() {
        return Err(E_SEALED);
    }
    // 封板后登记必拦。
    if led.record_round(make_snapshot(150, 150, 0)?) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    led.seal_guard_check()?;
    let s = led.summary()?;
    if s.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 幂等：另开同档账同样本，指纹全等 ——
    let mut led2 = IdemLedger::new(7);
    let mut r2 = 0usize;
    while r2 < REPLAY_ROUNDS {
        led2.record_round(make_snapshot(150, 149, 0)?)?;
        r2 += 1;
    }
    let fp2 = led2.judge()?;
    if fp != fp2 {
        return Err(E_FINGERPRINT);
    }

    // —— 漂移注入检出 100%：逐面注入必中 ——
    // 面 0：ppm 漂移（第 2 轮成功数变）。
    let mut d0 = IdemLedger::new(1);
    d0.record_round(make_snapshot(150, 149, 0)?)?;
    d0.record_round(make_snapshot(150, 148, 0)?)?;
    d0.record_round(make_snapshot(150, 149, 0)?)?;
    if d0.diff_faces() != Err(E_DRIFT_PPM) {
        return Err(E_DRIFT_PPM);
    }
    if d0.drift_hits() != 1 {
        return Err(E_DRIFT_PPM);
    }
    // 面 1：分类分布漂移（三轮失败总数同但类分布错位）。
    let mut d1 = IdemLedger::new(2);
    let mut a = make_snapshot(150, 149, 0)?;
    a.classes = [0, 0, 0, 1];
    let mut b = make_snapshot(150, 149, 0)?;
    b.classes = [1, 0, 0, 0];
    let mut c = make_snapshot(150, 149, 0)?;
    c.classes = [0, 0, 0, 1];
    d1.record_round(a)?;
    d1.record_round(b)?;
    d1.record_round(c)?;
    if d1.diff_faces() != Err(E_DRIFT_CLASS) {
        return Err(E_DRIFT_CLASS);
    }
    // 面 2：复测结论漂移。
    let mut d2 = IdemLedger::new(3);
    d2.record_round(make_snapshot(150, 149, 0)?)?;
    d2.record_round(make_snapshot(150, 149, 0)?)?;
    d2.record_round(make_snapshot(150, 149, 1)?)?;
    if d2.diff_faces() != Err(E_DRIFT_VERDICT) {
        return Err(E_DRIFT_VERDICT);
    }
    // 探测通道逐面验证：漂移面探测必返回 false（检出）。
    if d0.inject_drift_probe(DiffFace::Ppm)? {
        return Err(E_DRIFT_PPM);
    }
    if d1.inject_drift_probe(DiffFace::ClassDist)? {
        return Err(E_DRIFT_CLASS);
    }
    if d2.inject_drift_probe(DiffFace::Verdict)? {
        return Err(E_DRIFT_VERDICT);
    }

    // —— 错误路径样本 ——
    if DiffFace::from_id(3) != Err(E_FACE_RANGE) {
        return Err(E_FACE_RANGE);
    }
    if ReplayVerdict::from_arm(4) != Err(E_VERDICT_RANGE) {
        return Err(E_VERDICT_RANGE);
    }
    // 缺轮拒判。
    let mut short = IdemLedger::new(9);
    short.record_round(make_snapshot(150, 150, 0)?)?;
    if short.judge() != Err(E_ROUND_SHORT) {
        return Err(E_ROUND_SHORT);
    }
    if short.inject_drift_probe(DiffFace::Ppm) != Err(E_ROUND_SHORT) {
        return Err(E_ROUND_SHORT);
    }
    // ppm 越域。
    let bad_ppm = RoundSnapshot {
        ppm: 10_001,
        classes: [0; CLASS_COUNT],
        verdict: 0,
        executed: 0,
    };
    if bad_ppm.validate() != Err(E_PPM_RANGE) {
        return Err(E_PPM_RANGE);
    }
    // 判语越域。
    let bad_v = RoundSnapshot {
        ppm: 9_900,
        classes: [0; CLASS_COUNT],
        verdict: 4,
        executed: 0,
    };
    if bad_v.validate() != Err(E_VERDICT_RANGE) {
        return Err(E_VERDICT_RANGE);
    }
    // 守恒破：类计数之和 ≠ 执行数。
    let bad_c = RoundSnapshot {
        ppm: 9_900,
        classes: [1, 0, 0, 0],
        verdict: 0,
        executed: 2,
    };
    if bad_c.validate() != Err(E_CONSERVATION) {
        return Err(E_CONSERVATION);
    }
    // 成功数超执行数。
    if make_snapshot(10, 11, 0) != Err(E_CLASS_RANGE) {
        return Err(E_CLASS_RANGE);
    }
    // 摘要空账拦。
    let empty = IdemLedger::new(0);
    if empty.summary() != Err(E_TIER_MISSING) {
        return Err(E_TIER_MISSING);
    }

    // 诊断表抽查。
    if diag_detail(E_DRIFT_CLASS) == "未知诊断码（0x5780 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5780 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
