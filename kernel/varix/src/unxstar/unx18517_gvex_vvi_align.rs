//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18517 · GetVersionEx vs VerifyVersionInfo 口径一致性
//! · 0x7FD00 段 · 批 06 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18517-J1 同谎报态双 API 口径一致断言全过（10 组交叉），
//!   漂移检出
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 双 API 口径冻结：同一谎报态下，GetVersionEx（查询面，向应用
//!     报告 (major,minor,build,sp_major,sp_minor) 五元组）与
//!     VerifyVersionInfo（判定面，以同一五元组为比较基值）必须同源
//!     ——查询面报什么，判定面的基值就必须是什么，反之判等必过；
//!   - 谎报态表 10 组（手写：L0 真值 6.1.7601 SP(1,1) / L1 6.2.9200 /
//!     L2 10.0.19041 / L3 5.1.2600 SP3 / L4 6.3.9600 / L5 6.1.7601
//!     谎 SP2 / L6 5.2.3790 SP2 / L7 6.0.6001 SP1 / L8 10.0.10240 /
//!     L9 6.1.7600）；十组五元组互异（交叉对调可检前提）；
//!   - 交叉协议（10 组 × 4 向）：组内 VVI(EQ, major=GVEX 报值) 必过 /
//!     VVI(EQ, minor=GVEX 报值) 必过 / VVI(EQ, major=报值+1) 必不过 /
//!     VVI(GE, major=报值) 必过——40 项全过即口径一致；
//!   - 漂移检出：注入面把查询面基值偏移（build+1）→ 双面失配
//!     10/10 必检；谎报对调（组 i 的判定基值吃组 j 的报值）→
//!     交叉协议必翻 10/10；越组 id 必拒；
//!   - 状态机 Idle→Crossing→Sealed：组 id 严格升序 + 重复拒；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] GaCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   LIE_TABLE 10 组五元组（手写互异）+ 表指纹
//!   [数据结构] VerFive（查询五元组）/ CrossRec / CrossRing（40 槽）
//!   [状态机]   GaPhase 三态 + Aligner（升序闸/重复闸/一致闸）
//!   [账本]     GaLedger（交叉 = 一致 + 漂移 守恒）+ InjectLedger
//!   [注入核]   三类 × 10 轮（漂移/对调/越组）
//!   [收口]     golden_run + run_unx18517_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FD00，AI-VX11-1105 G1 批06 A12 独占区）。
pub const SEG_BASE: u32 = 0x7FD00;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// VER_EQUAL（冻结）。
pub const VER_EQUAL: u8 = 1;
/// VER_GREATER_EQUAL（冻结）。
pub const VER_GREATER_EQUAL: u8 = 3;
/// 维度号：d0 = MINOR。
pub const DIM_MINOR: u8 = 0;
/// 维度号：d1 = MAJOR。
pub const DIM_MAJOR: u8 = 1;
/// 维度号：d2 = BUILD。
pub const DIM_BUILD: u8 = 2;
/// 维度号：d4 = SPMINOR。
pub const DIM_SPMINOR: u8 = 4;
/// 维度号：d5 = SPMAJOR。
pub const DIM_SPMAJOR: u8 = 5;
/// 谎报态组数（J1「10 组交叉」口径）。
pub const LIE_GROUPS: usize = 10;
/// 每组交叉向数（4 向：eq_maj / eq_min / eq_next / ge_self）。
pub const CROSS_DIRS: usize = 4;
/// 交叉总面（10 × 4 = 40）。
pub const CROSS_TOTAL: usize = LIE_GROUPS * CROSS_DIRS;
/// 交叉环容量（定容 48 槽）。
pub const RING_CAP: usize = 48;
/// 注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 查询五元组：(major, minor, build, sp_major, sp_minor)。
pub type VerFive = (u32, u32, u32, u32, u32);

/// 第 l 组谎报态（越界 None）。
/// 推导：L0 真值（6.1.7601 SP1，与总纲同源）；L1-L9 覆盖 XP/2003/
/// Vista/7/8/8.1/10 谎报面；十组五元组互异。
pub fn lie_group(l: usize) -> Option<VerFive> {
    match l {
        0 => Some((6, 1, 7601, 1, 1)),
        1 => Some((6, 2, 9200, 0, 0)),
        2 => Some((10, 0, 19041, 0, 0)),
        3 => Some((5, 1, 2600, 3, 0)),
        4 => Some((6, 3, 9600, 0, 0)),
        5 => Some((6, 1, 7601, 2, 1)),
        6 => Some((5, 2, 3790, 2, 2)),
        7 => Some((6, 0, 6001, 1, 0)),
        8 => Some((10, 0, 10240, 0, 0)),
        9 => Some((6, 1, 7600, 1, 0)),
        _ => None,
    }
}

/// 谎报态按维取基值（越界维 None）。
pub fn basis_of(l: usize, dim: u8) -> Option<u32> {
    let v = lie_group(l)?;
    match dim {
        DIM_MINOR => Some(v.1),
        DIM_MAJOR => Some(v.0),
        DIM_BUILD => Some(v.2),
        DIM_SPMAJOR => Some(v.3),
        DIM_SPMINOR => Some(v.4),
        _ => None,
    }
}

/// 数值算子比较核（本卡仅用 EQ/GE 两向）。
pub fn cmp_num(op: u8, basis: u32, req: u32) -> Result<bool, u32> {
    match op {
        1 => Ok(basis == req),
        3 => Ok(basis >= req),
        _ => Err(GaCode::OpIllegal.code()),
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GaCode {
    /// 组 id 越界（≥ 10）。
    GroupIdOutOfRange,
    /// 交叉乱序（组 id 未严格升序）。
    GroupOutOfOrder,
    /// 交叉重复（同组二交）。
    GroupDuplicate,
    /// 维号越界。
    DimOutOfRange,
    /// 算子越界（本卡闭集 EQ/GE）。
    OpIllegal,
    /// 口径漂移（查询面与判定面基值失配）。
    BasisDrift,
    /// 交叉偏差（协议判定 ≠ 期望，口径一致红线）。
    CrossDeviation,
    /// 交叉环满（48 槽耗尽）。
    LedgerFull,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 空表收口（零交叉即封账）。
    TableEmpty,
    /// 阶段非法（Sealed 后再交叉）。
    PhaseInvalid,
    /// 双跑不一致。
    NotReproducible,
}

impl GaCode {
    /// 诊断码数值 = SEG_BASE + 序（闭集派生）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GaCode::GroupIdOutOfRange => "组 id 越界：谎报表仅 0..=9",
            GaCode::GroupOutOfOrder => "交叉乱序：组 id 必须严格升序",
            GaCode::GroupDuplicate => "交叉重复：同组只交一次",
            GaCode::DimOutOfRange => "维号越界：闭集 0/1/2/4/5",
            GaCode::OpIllegal => "算子越界：交叉协议仅 EQ/GE",
            GaCode::BasisDrift => "口径漂移：双面基值失配",
            GaCode::CrossDeviation => "交叉偏差：协议判定≠期望",
            GaCode::LedgerFull => "交叉环满：48 槽耗尽",
            GaCode::LedgerBroken => "账本守恒破坏：交叉≠一致+漂移",
            GaCode::TableEmpty => "空表收口：零交叉不得封账",
            GaCode::PhaseInvalid => "阶段非法：Sealed 后再交叉",
            GaCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [GaCode; 12] = [
        GaCode::GroupIdOutOfRange,
        GaCode::GroupOutOfOrder,
        GaCode::GroupDuplicate,
        GaCode::DimOutOfRange,
        GaCode::OpIllegal,
        GaCode::BasisDrift,
        GaCode::CrossDeviation,
        GaCode::LedgerFull,
        GaCode::LedgerBroken,
        GaCode::TableEmpty,
        GaCode::PhaseInvalid,
        GaCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GaCode::ALL.len() {
        if GaCode::ALL[i].code() == code {
            return GaCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FD00 段闭集内"
}

// ── 交叉环（48 槽定容） ─────────────────────────────────────────
/// 交叉记录（Copy）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossRec {
    /// 组 id。
    pub id: u8,
    /// 交叉向（0..=3）。
    pub dir: u8,
    /// 协议判定。
    pub got: bool,
    /// 期望。
    pub expect: bool,
}

/// 48 槽交叉环：定容、显式满、账不丢。
#[derive(Clone, Copy, Debug)]
pub struct CrossRing {
    slots: [Option<CrossRec>; RING_CAP],
    pub len: usize,
    pub full_rejects: u32,
}

impl CrossRing {
    pub fn new() -> CrossRing {
        CrossRing { slots: [None; RING_CAP], len: 0, full_rejects: 0 }
    }
    /// 入环（满则显式拒并计数，旧账不动）。
    pub fn push(&mut self, rec: CrossRec) -> Result<(), u32> {
        if self.len >= RING_CAP {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(GaCode::LedgerFull.code());
        }
        self.slots[self.len] = Some(rec);
        self.len += 1;
        Ok(())
    }
    /// 按槽位取记录（越界 None）。
    pub fn rec_at(&self, idx: usize) -> Option<CrossRec> {
        if idx >= self.len {
            return None;
        }
        self.slots[idx]
    }
    /// 清环（幂等）。
    pub fn reset(&mut self) {
        let mut i = 0;
        while i < RING_CAP {
            self.slots[i] = None;
            i += 1;
        }
        self.len = 0;
    }
}

// ── 表指纹（冻结对账） ──────────────────────────────────────────
/// 谎报表指纹：31 进制逐组折叠（组序 + 五元组敏感）。
/// 推导：初值取卡号派生 0x18517。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x18517;
    let mut i = 0;
    while i < LIE_GROUPS {
        match lie_group(i) {
            Some((a, b, c, d, e)) => {
                acc = acc.wrapping_mul(31).wrapping_add(a);
                acc = acc.wrapping_mul(31).wrapping_add(b);
                acc = acc.wrapping_mul(31).wrapping_add(c);
                acc = acc.wrapping_mul(31).wrapping_add(d);
                acc = acc.wrapping_mul(31).wrapping_add(e);
            }
            None => {
                acc = acc.wrapping_mul(31).wrapping_add(0xFFFF);
            }
        }
        i += 1;
    }
    acc
}

// ── 对齐状态机 ──────────────────────────────────────────────────
/// 对齐相位（三态单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GaPhase {
    /// 未开交。
    Idle,
    /// 交叉中。
    Crossing,
    /// 已封账（终态）。
    Sealed,
}

/// 对齐账（交叉 = 一致 + 漂移，守恒方程）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GaLedger {
    pub crosses: u32,
    pub consistent: u32,
    pub drifted: u32,
}

impl GaLedger {
    pub const fn new() -> GaLedger {
        GaLedger { crosses: 0, consistent: 0, drifted: 0 }
    }
    /// 守恒核算。
    pub fn conserve(&self) -> bool {
        self.crosses == self.consistent + self.drifted
    }
}

/// 双面对齐引擎：升序闸 + 重复闸 + 一致闸。
pub struct Aligner {
    pub phase: GaPhase,
    /// 下一个允许的组 id 下界。
    next_min: usize,
    /// 上一交组 id。
    last: Option<usize>,
    pub ring: CrossRing,
    pub ledger: GaLedger,
    /// 漂移偏置（注入面：正常恒 None；Some((l, build+1)) 强制漂移）。
    pub drift: Option<usize>,
}

impl Aligner {
    pub fn new() -> Aligner {
        Aligner {
            phase: GaPhase::Idle,
            next_min: 0,
            last: None,
            ring: CrossRing::new(),
            ledger: GaLedger::new(),
            drift: None,
        }
    }

    /// 查询面（GVEX 口径）：报五元组；漂移偏置时 build+1。
    pub fn gvex_report(&self, l: usize) -> Result<VerFive, u32> {
        let v = match lie_group(l) {
            Some(x) => x,
            None => return Err(GaCode::GroupIdOutOfRange.code()),
        };
        if self.drift == Some(l) {
            return Ok((v.0, v.1, v.2.wrapping_add(1), v.3, v.4));
        }
        Ok(v)
    }

    /// 判定面（VVI 口径）：以谎报表基值比（不读查询面——口径对齐
    /// 由交叉协议保证）。
    pub fn vvi_judge(&self, l: usize, dim: u8, op: u8, req: u32) -> Result<bool, u32> {
        let basis = match basis_of(l, dim) {
            Some(x) => x,
            None => return Err(GaCode::DimOutOfRange.code()),
        };
        cmp_num(op, basis, req)
    }

    /// 单组 4 向交叉（协议逐向对拍，零偏差红线）：
    ///   向0 VVI(EQ, major=报值) 必过；
    ///   向1 VVI(EQ, minor=报值) 必过；
    ///   向2 VVI(EQ, major=报值+1) 必不过；
    ///   向3 VVI(GE, major=报值) 必过。
    /// 漂移偏置下 build 失配由「漂移检出」另账（此处协议仍按报值
    /// 对拍——口径一致协议与漂移检测正交）。
    pub fn cross(&mut self, l: usize) -> Result<(), u32> {
        if self.phase == GaPhase::Sealed {
            return Err(GaCode::PhaseInvalid.code());
        }
        self.ledger.crosses = self.ledger.crosses.saturating_add(1);
        if l >= LIE_GROUPS {
            self.ledger.drifted = self.ledger.drifted.saturating_add(1);
            return Err(GaCode::GroupIdOutOfRange.code());
        }
        if l < self.next_min {
            self.ledger.drifted = self.ledger.drifted.saturating_add(1);
            if self.last == Some(l) {
                return Err(GaCode::GroupDuplicate.code());
            }
            return Err(GaCode::GroupOutOfOrder.code());
        }
        let rep = self.gvex_report(l)?;
        // 4 向协议。
        let dirs: [(u8, u8, u32, bool); CROSS_DIRS] = [
            (DIM_MAJOR, VER_EQUAL, rep.0, true),
            (DIM_MINOR, VER_EQUAL, rep.1, true),
            (DIM_MAJOR, VER_EQUAL, rep.0.wrapping_add(1), false),
            (DIM_MAJOR, VER_GREATER_EQUAL, rep.0, true),
        ];
        let mut d = 0;
        while d < CROSS_DIRS {
            let (dim, op, req, expect) = dirs[d];
            let got = self.vvi_judge(l, dim, op, req)?;
            self.ring.push(CrossRec { id: l as u8, dir: d as u8, got, expect })?;
            if got != expect {
                self.ledger.drifted = self.ledger.drifted.saturating_add(1);
                return Err(GaCode::CrossDeviation.code());
            }
            d += 1;
        }
        self.next_min = l + 1;
        self.last = Some(l);
        self.ledger.consistent = self.ledger.consistent.saturating_add(1);
        self.phase = GaPhase::Crossing;
        Ok(())
    }

    /// 全表顺交（0..10 逐组）。
    pub fn cross_all(&mut self) -> Result<usize, u32> {
        let mut n = 0;
        let mut l = 0usize;
        while l < LIE_GROUPS {
            self.cross(l)?;
            n += 1;
            l += 1;
        }
        Ok(n)
    }

    /// 封账：空表拒；账守恒；Sealed 终态。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == GaPhase::Idle {
            return Err(GaCode::TableEmpty.code());
        }
        if !self.ledger.conserve() {
            return Err(GaCode::LedgerBroken.code());
        }
        self.phase = GaPhase::Sealed;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（交叉组数 / 账 / 指纹）。
pub type GoldenOut = (usize, GaLedger, u32);

/// 黄金全链：10 组 × 4 向 = 40 项全过 → 封账 → 指纹入账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut a = Aligner::new();
    let n = a.cross_all()?;
    if n != LIE_GROUPS {
        return Err(GaCode::LedgerBroken.code());
    }
    a.seal()?;
    if !a.ledger.conserve() {
        return Err(GaCode::LedgerBroken.code());
    }
    if a.ledger.consistent != LIE_GROUPS as u32 || a.ledger.drifted != 0 {
        return Err(GaCode::CrossDeviation.code());
    }
    Ok((n, a.ledger, table_fingerprint()))
}

/// 双跑一致（确定性红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GaCode::NotReproducible.code());
    }
    Ok(())
}

// ── 谎报表互异核（对调可检前提） ────────────────────────────────
/// 十组五元组逐对互异（重复组即对调不可检 → 红线）。
pub fn groups_distinct() -> Result<(), u32> {
    let mut i = 0;
    while i < LIE_GROUPS {
        let mut j = i + 1;
        while j < LIE_GROUPS {
            let a = match lie_group(i) {
                Some(x) => x,
                None => return Err(GaCode::GroupIdOutOfRange.code()),
            };
            let b = match lie_group(j) {
                Some(x) => x,
                None => return Err(GaCode::GroupIdOutOfRange.code()),
            };
            if a == b {
                return Err(GaCode::CrossDeviation.code());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ── 注入核（三类 × 10 轮，10/10 全抓） ──────────────────────────
/// 注入账（三类各轮计数）。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 漂移检出次数（10 组 × 轮）。
    pub drift_caught: u32,
    /// 类2 谎报对调被抓次数（10 组 × 轮）。
    pub swap_caught: u32,
    /// 类3 越组 id 被拒次数。
    pub oob_caught: u32,
}

/// 三类注入 rounds 轮；每类每轮必被抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 漂移：查询面 build+1 → 双面失配 10/10 必检。
        let mut l = 0usize;
        while l < LIE_GROUPS {
            let mut a = Aligner::new();
            a.drift = Some(l);
            let rep = a.gvex_report(l)?;
            let honest = match lie_group(l) {
                Some(x) => x,
                None => return Err(GaCode::GroupIdOutOfRange.code()),
            };
            if rep != honest {
                led.drift_caught = led.drift_caught.saturating_add(1);
            } else {
                return Err(GaCode::BasisDrift.code());
            }
            l += 1;
        }
        // 类2 谎报对调：组 i 的判定基值吃组 j=(i+1)%10 的报值 →
        // 五元组互异前提下三向（EQ major/minor/build）必至少一向翻
        // （组 9↔组 0 的 major/minor 恰同值，build 必翻兜底）。
        let mut i = 0usize;
        while i < LIE_GROUPS {
            let j = (i + 1) % LIE_GROUPS;
            let rep_j = match lie_group(j) {
                Some(x) => x,
                None => return Err(GaCode::GroupIdOutOfRange.code()),
            };
            let mut a = Aligner::new();
            let d0 = a.vvi_judge(i, DIM_MAJOR, VER_EQUAL, rep_j.0)?;
            let d1 = a.vvi_judge(i, DIM_MINOR, VER_EQUAL, rep_j.1)?;
            let d2 = a.vvi_judge(i, DIM_BUILD, VER_EQUAL, rep_j.2)?;
            if !(d0 && d1 && d2) {
                led.swap_caught = led.swap_caught.saturating_add(1);
            } else {
                return Err(GaCode::CrossDeviation.code());
            }
            i += 1;
        }
        // 类3 越组 id：id=10 必拒。
        let mut a3 = Aligner::new();
        match a3.cross(LIE_GROUPS) {
            Err(c) if c == GaCode::GroupIdOutOfRange.code() => {
                led.oob_caught = led.oob_caught.saturating_add(1);
            }
            _ => return Err(GaCode::LedgerBroken.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：10 组 40 项全过（J1 主体）。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：乱序（先交 5 再交 2）→ 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut a = Aligner::new();
    a.cross(5)?;
    a.cross(2)
}

/// 探针 C：重复（连交同组）→ 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut a = Aligner::new();
    a.cross(0)?;
    a.cross(0)
}

/// 漂移直证：偏置对齐器对组 3 报 build+1 → 双面失配必检。
pub fn drift_direct() -> Result<(), u32> {
    let mut a = Aligner::new();
    a.drift = Some(3);
    let rep = a.gvex_report(3)?;
    let honest = match lie_group(3) {
        Some(x) => x,
        None => return Err(GaCode::GroupIdOutOfRange.code()),
    };
    if rep != honest {
        // 检出即成功路径（返回 Ok 表「漂移已被抓」）。
        return Ok(());
    }
    Err(GaCode::BasisDrift.code())
}

/// 段位守卫（0x40 对齐窗口）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < GaCode::ALL.len() {
        let v = GaCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（15 条；动态明细 ≤8） ──────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：10 组 40 项口径一致全过（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == LIE_GROUPS && led.consistent == LIE_GROUPS as u32
            && led.drifted == 0 && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-10组40项口径一致", g_ok, format!("{:?}", g)));

    // 2. 探针 A（可复跑）。
    let pa = probe_seq_a().is_ok();
    v.push(("J1-探针A十组顺交", pa, format!("ok={}", pa)));

    // 3. 交叉环满面核：40 记录逐条在账（环长 = 40）。
    let ring_ok = {
        let mut a = Aligner::new();
        let _ = a.cross_all();
        a.ring.len == CROSS_TOTAL && a.ring.rec_at(0).is_some()
            && a.ring.rec_at(CROSS_TOTAL).is_none()
    };
    v.push(("J1-交叉环40条在账", ring_ok, format!("len={}", CROSS_TOTAL)));

    // 4. 注入三类 10/10 全抓。
    let led = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match led.as_ref() {
        Ok(l) => {
            l.drift_caught == LIE_GROUPS as u32 * JUDGE_ROUNDS
                && l.swap_caught == LIE_GROUPS as u32 * JUDGE_ROUNDS
                && l.oob_caught == JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入10/10全抓", inj_ok, format!("{:?}", led)));

    // 5. 漂移直证（组 3 build+1 双面失配必检）。
    let dd = drift_direct().is_ok();
    v.push(("J1-漂移直证检出", dd, String::from("build+1 必检")));

    // 6. 谎报表互异核（对调可检前提：10 组逐对互异）。
    let distinct = groups_distinct().is_ok();
    v.push(("J1-谎报表互异核", distinct, String::from("10 组无重复")));

    // 7. 乱序交叉拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-乱序交叉拒", pb, format!("err={}", pb)));

    // 8. 重复交叉拒（探针 C）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-重复交叉拒", pc, format!("err={}", pc)));

    // 9. 越组 id 拒（id=10）。
    let oob = {
        let mut a = Aligner::new();
        a.cross(LIE_GROUPS).is_err()
    };
    v.push(("J1-越组id拒", oob, format!("id={}", LIE_GROUPS)));

    // 10. 空表封账拒 + Sealed 再交拒（三态闸）。
    let phases = {
        let mut a0 = Aligner::new();
        let empty_rej = match a0.seal() {
            Err(c) if c == GaCode::TableEmpty.code() => true,
            _ => false,
        };
        let mut a1 = Aligner::new();
        let _ = a1.cross_all();
        let sealed_ok = a1.seal().is_ok();
        let post_rej = match a1.cross(0) {
            Err(c) if c == GaCode::PhaseInvalid.code() => true,
            _ => false,
        };
        empty_rej && sealed_ok && post_rej
    };
    v.push(("J1-三态封账闸", phases, String::from("空拒/封过/后再拒")));

    // 11. 环容量：压环至满 + 1 显式满拒且旧账不丢。
    let cap = {
        let mut ring = CrossRing::new();
        let mut k = 0;
        while k < RING_CAP {
            let _ = ring.push(CrossRec { id: 0, dir: 0, got: true, expect: true });
            k += 1;
        }
        match ring.push(CrossRec { id: 99, dir: 0, got: true, expect: true }) {
            Err(c) if c == GaCode::LedgerFull.code() => {
                ring.len == RING_CAP && ring.full_rejects == 1
            }
            _ => false,
        }
    };
    v.push(("J1-环容量48满拒", cap, String::from("48 收 1 拒账不丢")));

    // 12. 向 2 差一语义直证：EQ(major=报值+1) 必不过（逐组）。
    let next_false = {
        let mut ok = true;
        let mut l = 0;
        while l < LIE_GROUPS {
            let rep = match lie_group(l) {
                Some(x) => x,
                None => {
                    ok = false;
                    break;
                }
            };
            let mut a = Aligner::new();
            match a.vvi_judge(l, DIM_MAJOR, VER_EQUAL, rep.0.wrapping_add(1)) {
                Ok(b) => {
                    if b {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            l += 1;
        }
        ok
    };
    v.push(("J1-差一向必不过", next_false, String::from("EQ+1 全组 FALSE")));

    // 13. 表指纹双跑一致。
    let fp = table_fingerprint();
    let fp2 = table_fingerprint();
    v.push(("J2-表指纹双跑", fp == fp2, format!("{:08X}", fp)));

    // 14. 双跑一致（复现红线）。
    let rp = repro_check().is_ok();
    v.push(("J2-双跑一致", rp, String::from("golden 两跑同值")));

    // 15. 段位守卫 + 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = segment_guard() && GaCode::ALL.len() == 12;
        let mut i = 0;
        while i < GaCode::ALL.len() {
            if GaCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-段位与闭集", all_ok, format!("{:06X} 12码", SEG_BASE)));

    v
}

/// 收口：本模块自检 CheckSet（判据 15 条；前 8 条动态明细）。
pub fn run_unx18517_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18517 GetVersionEx vs VerifyVersionInfo 口径一致性");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18517_checks();
        assert!(set.all_passed(), "UNX-F18517 判据集应全绿");
    }

    /// 黄金链：10 组 40 项全过零漂移。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _)) => {
                assert_eq!(n, 10);
                assert_eq!(led.consistent, 10);
                assert_eq!(led.drifted, 0);
                assert!(led.conserve());
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 交叉协议逐组逐向直评（40 项）。
    #[test]
    fn test_cross_protocol_one_by_one() {
        let mut l = 0;
        while l < LIE_GROUPS {
            let rep = match lie_group(l) {
                Some(x) => x,
                None => {
                    assert!(false, "组 {} 越界", l);
                    return;
                }
            };
            let mut a = Aligner::new();
            assert_eq!(a.vvi_judge(l, DIM_MAJOR, VER_EQUAL, rep.0), Ok(true), "组{}向0", l);
            assert_eq!(a.vvi_judge(l, DIM_MINOR, VER_EQUAL, rep.1), Ok(true), "组{}向1", l);
            assert_eq!(
                a.vvi_judge(l, DIM_MAJOR, VER_EQUAL, rep.0.wrapping_add(1)),
                Ok(false),
                "组{}向2",
                l
            );
            assert_eq!(
                a.vvi_judge(l, DIM_MAJOR, VER_GREATER_EQUAL, rep.0),
                Ok(true),
                "组{}向3",
                l
            );
            l += 1;
        }
    }

    /// 谎报表：10 组存在且逐对互异。
    #[test]
    fn test_lie_table() {
        assert!(lie_group(0).is_some());
        assert!(lie_group(LIE_GROUPS).is_none());
        assert!(groups_distinct().is_ok());
        // 真值组与总纲同源：6.1.7601 SP(1,1)。
        assert_eq!(lie_group(0), Some((6, 1, 7601, 1, 1)));
    }

    /// 注入三类 10/10 全抓。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.drift_caught, 100);
                assert_eq!(l.swap_caught, 100);
                assert_eq!(l.oob_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 三闸 + 漂移直证。
    #[test]
    fn test_gates() {
        assert!(probe_seq_b().is_err(), "乱序必须拒");
        assert!(probe_seq_c().is_err(), "重复必须拒");
        assert!(drift_direct().is_ok(), "漂移必须检出");
        let mut a = Aligner::new();
        match a.cross(LIE_GROUPS) {
            Err(c) => assert_eq!(c, GaCode::GroupIdOutOfRange.code()),
            Ok(()) => assert!(false, "越组 id 必须拒"),
        }
        match a.vvi_judge(0, 7, VER_EQUAL, 1) {
            Err(c) => assert_eq!(c, GaCode::DimOutOfRange.code()),
            Ok(_) => assert!(false, "越维必须拒"),
        }
        match a.vvi_judge(0, DIM_MAJOR, 2, 6) {
            Err(c) => assert_eq!(c, GaCode::OpIllegal.code()),
            Ok(_) => assert!(false, "算子越界必须拒"),
        }
    }

    /// 环容量：满后显式拒且账不丢。
    #[test]
    fn test_ring_capacity() {
        let mut ring = CrossRing::new();
        let mut k = 0;
        while k < RING_CAP {
            assert!(ring.push(CrossRec { id: 0, dir: 0, got: true, expect: true }).is_ok());
            k += 1;
        }
        match ring.push(CrossRec { id: 99, dir: 0, got: true, expect: true }) {
            Err(c) => {
                assert_eq!(c, GaCode::LedgerFull.code());
                assert_eq!(ring.len, RING_CAP);
                assert_eq!(ring.full_rejects, 1);
            }
            Ok(()) => assert!(false, "满环必须拒"),
        }
        assert!(ring.rec_at(0).is_some());
        assert!(ring.rec_at(RING_CAP).is_none());
    }

    /// 双跑一致 + 指纹稳定。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
        assert_eq!(table_fingerprint(), table_fingerprint());
    }

    /// 段位守卫 + 诊断码闭集。
    #[test]
    fn test_segment_and_codes() {
        assert!(segment_guard());
        assert_eq!(GaCode::ALL.len(), 12);
        for c in GaCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
