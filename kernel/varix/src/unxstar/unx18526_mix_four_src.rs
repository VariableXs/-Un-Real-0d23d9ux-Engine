//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18526 · 混合场景探针（5 例四源叠加） · 0x7FF40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据锚点：UNX-F18526-J1 5 例四源叠加终值与优先级链一致断言全过
//! ─────────────────────────────────────────────────────────────────────
//! 本模块结构清单：
//!   - 数据结构：MixSource 四源枚举（条件掩码/ActCtx/声明/兼容值）、
//!     ScenarioSrc（四源 Option 值束）、MixTrace（终值+胜出层审计行）、
//!     MixLedger（5 例总账）；
//!   - 状态机：MixState 四态（Unassembled→Assembled→Resolved→
//!     Matched/Deviated），迁移闭集 4 条合法、其余 12 组合显式拒；
//!   - 账本：5 行冻结裁决表（四源值→期望终值+期望胜出层）、解析轨迹账
//!     （winner/final 双列）、注入检出账；
//!   - 错误路径：MixCode 十二码闭集，code()=SEG_BASE+n，中文 detail；
//!   - 注入核：三类违约（缺源 / 场景号越界 / 批内场景号重复）逐类
//!     10 轮断言被显式拒绝，漏检恒零。
//! 设计要点：
//!   - 优先级链冻结：条件掩码(P0) > ActCtx(P1) > 声明(P2) > 兼容值(P3)；
//!     链语义 = 自高向低取第一个非让位值；全让位 → 基值兜底 0x00F0；
//!   - 让位语义 = 该层值为 0（显式弃权），与「缺源 = None」严格区分：
//!     缺源是账簿违约（拒），让位是合法语义（降层）；
//!   - 五例覆盖链全路径：P0 胜 / P1 胜 / P2 胜 / P3 胜 / 基值兜底，
//!     胜出层与终值双列冻结在裁决表；
//!   - 常量推导：0x7FF40 = 0x7FF00 + 0x40（A13 批第 6 卡独占窗）；
//!     基值 0x00F0 推导：四层让位兜底值取非零标记位（F=Fallthrough）；
//!   - no_std + alloc；零 unwrap/expect/panic/裸越界下标/浮点/IO。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FF40，A13 批第 6 卡独占区，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7FF40;

// ── 冻结常量 ────────────────────────────────────────────────────
/// 混合场景数（冻结：5，J1 主锚点）。
pub const SCEN_COUNT: usize = 5;
/// 四源层数（冻结：4）。
pub const SRC_LAYERS: usize = 4;
/// 全让位基值兜底（推导：F=Fallthrough 标记位 0xF0，非零可辨）。
pub const BASE_FALLBACK: u32 = 0x00F0;
/// 判据迭代轮数（推导：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类数（缺源/越界/重复 三类）。
pub const INJECT_CLASSES: usize = 3;
/// 胜出层编码：基值兜底（全让位）。
pub const PRIO_BASE: u8 = 4;

// ── FNV-1a 64 位指纹 ────────────────────────────────────────────
/// FNV-1a（裁决表与账本指纹唯一哈希来源）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

// ── 四源枚举（优先级闭集） ──────────────────────────────────────
/// 四源层（优先级自高向低：P0 > P1 > P2 > P3）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MixSource {
    /// P0 · 条件掩码（最高）
    CondMask,
    /// P1 · ActCtx 覆写
    ActCtx,
    /// P2 · supportedOS 声明
    Declaration,
    /// P3 · 兼容值（最低）
    CompatValue,
}

impl MixSource {
    /// 层号（= 优先级编码，0..4）。
    pub fn prio(self) -> u8 {
        match self {
            MixSource::CondMask => 0,
            MixSource::ActCtx => 1,
            MixSource::Declaration => 2,
            MixSource::CompatValue => 3,
        }
    }
}

/// 四源全量（优先级遍历用，自高向低）。
pub const ALL_SOURCES: [MixSource; SRC_LAYERS] = [
    MixSource::CondMask,
    MixSource::ActCtx,
    MixSource::Declaration,
    MixSource::CompatValue,
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MixCode {
    /// 缺源（四源叠加要求全层在账，None 即拒）
    MissingSource,
    /// 场景号越界（0 或 >5）
    IdOutOfRange,
    /// 批内场景号重复
    DupScen,
    /// 状态机非法迁移
    StateIllegalTransit,
    /// 终值与裁决表偏差
    DeviationFound,
    /// 双跑不一致
    NotReproducible,
    /// 胜出层与期望不符（链语义破坏）
    WinnerMismatch,
    /// 裁决表缺行
    TableMissing,
    /// 全让位但基值兜底未生效
    AllAbstainedWithoutBase,
    /// 优先级链序破坏（层序遍历错乱）
    ChainBroken,
    /// 账本空（无轨迹落账）
    LedgerEmpty,
    /// 注入漏检
    InjectMissed,
}

impl MixCode {
    /// 诊断码 = SEG_BASE + 枚举序（0x7FF40..0x7FF80）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            MixCode::MissingSource => "缺源：四源叠加要求全层在账（None 拒）",
            MixCode::IdOutOfRange => "场景号越界：合法闭集 1..=5",
            MixCode::DupScen => "批内场景号重复：批账拒收",
            MixCode::StateIllegalTransit => "状态机非法迁移：越级/回退全拒",
            MixCode::DeviationFound => "终值偏差：与裁决表不等",
            MixCode::NotReproducible => "双跑不一致：确定性红线",
            MixCode::WinnerMismatch => "胜出层不符：优先级链语义破坏",
            MixCode::TableMissing => "裁决表缺行：sid 不在 5 行闭集",
            MixCode::AllAbstainedWithoutBase => "全让位但基值兜底未生效",
            MixCode::ChainBroken => "优先级链序破坏：层序遍历错乱",
            MixCode::LedgerEmpty => "账本空：解析轨迹零落账",
            MixCode::InjectMissed => "注入漏检：检出 < 注入，验收红线",
        }
    }
    /// 全码闭集（12 条）。
    pub const ALL: [MixCode; 12] = [
        MixCode::MissingSource,
        MixCode::IdOutOfRange,
        MixCode::DupScen,
        MixCode::StateIllegalTransit,
        MixCode::DeviationFound,
        MixCode::NotReproducible,
        MixCode::WinnerMismatch,
        MixCode::TableMissing,
        MixCode::AllAbstainedWithoutBase,
        MixCode::ChainBroken,
        MixCode::LedgerEmpty,
        MixCode::InjectMissed,
    ];
}

/// u32 诊断码反查中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < MixCode::ALL.len() {
        if let Some(c) = MixCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FF40 段闭集内"
}

// ── 场景数据束 ──────────────────────────────────────────────────
/// 四源值束（None=缺源[拒]，Some(0)=让位[降层]，Some(v)=出值）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioSrc {
    pub sid: u32,
    pub cond: Option<u32>,
    pub actx: Option<u32>,
    pub decl: Option<u32>,
    pub compat: Option<u32>,
}

/// 四源在账完整性闸：任一层 None 即 MissingSource。
pub fn require_all_srcs(s: &ScenarioSrc) -> Result<(), u32> {
    if s.cond.is_none() {
        return Err(MixCode::MissingSource.code());
    }
    if s.actx.is_none() {
        return Err(MixCode::MissingSource.code());
    }
    if s.decl.is_none() {
        return Err(MixCode::MissingSource.code());
    }
    if s.compat.is_none() {
        return Err(MixCode::MissingSource.code());
    }
    Ok(())
}

/// 优先级链解析：自 P0 向 P3 取第一个非让位值；全让位 → 基值兜底。
/// 返回 (终值, 胜出层)；层编码 0..3，全让位 = PRIO_BASE(4)。
pub fn resolve_chain(s: &ScenarioSrc) -> Result<(u32, u8), u32> {
    require_all_srcs(s)?;
    let layers = [s.cond, s.actx, s.decl, s.compat];
    let mut p = 0;
    while p < ALL_SOURCES.len() {
        // 链序闸：层号必须与枚举 prio 一致（链语义自证）。
        let src = match ALL_SOURCES.get(p) {
            Some(x) => *x,
            None => return Err(MixCode::ChainBroken.code()),
        };
        if src.prio() != p as u8 {
            return Err(MixCode::ChainBroken.code());
        }
        let v = match layers.get(p) {
            Some(x) => *x,
            None => return Err(MixCode::MissingSource.code()),
        };
        match v {
            Some(val) => {
                if val != 0 {
                    return Ok((val, p as u8));
                }
                // 0 = 让位，继续降层。
            }
            None => return Err(MixCode::MissingSource.code()),
        }
        p += 1;
    }
    // 全让位 → 基值兜底。
    Ok((BASE_FALLBACK, PRIO_BASE))
}

// ── 5 行冻结裁决表 ──────────────────────────────────────────────
/// 裁决表行（sid, 四源值, 期望终值, 期望胜出层）。
///
/// 期望推导（链序 P0>PT1>P2>P3，0=让位）：
///   S1 0x11/0x22/0x33/0x44 → P0 胜，终值 0x11
///   S2 0x00/0x22/0x33/0x44 → P1 胜，终值 0x22
///   S3 0x00/0x00/0x33/0x44 → P2 胜，终值 0x33
///   S4 0x00/0x00/0x00/0x44 → P3 胜，终值 0x44
///   S5 0x00/0x00/0x00/0x00 → 全让位，基值 0x00F0（层=4）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixRow {
    pub sid: u32,
    pub cond: u32,
    pub actx: u32,
    pub decl: u32,
    pub compat: u32,
    pub expected_val: u32,
    pub expected_prio: u8,
}

/// 5 行冻结裁决表（J1 主锚点）。
pub const MIX_TABLE: [MixRow; SCEN_COUNT] = [
    MixRow { sid: 1, cond: 0x11, actx: 0x22, decl: 0x33, compat: 0x44, expected_val: 0x11, expected_prio: 0 },
    MixRow { sid: 2, cond: 0x00, actx: 0x22, decl: 0x33, compat: 0x44, expected_val: 0x22, expected_prio: 1 },
    MixRow { sid: 3, cond: 0x00, actx: 0x00, decl: 0x33, compat: 0x44, expected_val: 0x33, expected_prio: 2 },
    MixRow { sid: 4, cond: 0x00, actx: 0x00, decl: 0x00, compat: 0x44, expected_val: 0x44, expected_prio: 3 },
    MixRow { sid: 5, cond: 0x00, actx: 0x00, decl: 0x00, compat: 0x00, expected_val: BASE_FALLBACK, expected_prio: PRIO_BASE },
];

/// 行 → 四源值束（Some 全在账）。
pub fn row_to_src(row: &MixRow) -> ScenarioSrc {
    ScenarioSrc {
        sid: row.sid,
        cond: Some(row.cond),
        actx: Some(row.actx),
        decl: Some(row.decl),
        compat: Some(row.compat),
    }
}

/// 按场景号查裁决表（越界 Err）。
pub fn mix_row(sid: u32) -> Result<MixRow, u32> {
    if sid == 0 || sid > SCEN_COUNT as u32 {
        return Err(MixCode::IdOutOfRange.code());
    }
    match MIX_TABLE.get((sid - 1) as usize) {
        Some(r) => Ok(*r),
        None => Err(MixCode::TableMissing.code()),
    }
}

// ── 状态机（四态） ──────────────────────────────────────────────
/// 单场景生命周期四态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MixState {
    /// 未组装（四源未在账）
    Unassembled,
    /// 已组装（四源在账，待解析）
    Assembled,
    /// 已解析（终值+胜出层落账）
    Resolved,
    /// 与裁决表一致
    Matched,
    /// 与裁决表偏差
    Deviated,
}

/// 迁移闭集：合法恰 4 条，其余 16 组合显式拒。
pub fn can_transit(from: MixState, to: MixState) -> bool {
    match (from, to) {
        (MixState::Unassembled, MixState::Assembled) => true,
        (MixState::Assembled, MixState::Resolved) => true,
        (MixState::Resolved, MixState::Matched) => true,
        (MixState::Resolved, MixState::Deviated) => true,
        _ => false,
    }
}

// ── 账本 ────────────────────────────────────────────────────────
/// 解析轨迹审计行（sid, 胜出层, 终值 双列）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MixTrace {
    pub sid: u32,
    pub winner_prio: u8,
    pub final_val: u32,
    pub deviated: bool,
}

/// 混合场景总账。
#[derive(Clone, Debug)]
pub struct MixLedger {
    pub traces: Vec<MixTrace>,
    pub states: [MixState; SCEN_COUNT],
    pub transit_rejects: u32,
}

impl MixLedger {
    pub fn new() -> MixLedger {
        MixLedger {
            traces: Vec::new(),
            states: [MixState::Unassembled; SCEN_COUNT],
            transit_rejects: 0,
        }
    }

    /// 单场景：查表→组装→链解析→双列对拍→落账。
    pub fn run_scenario(&mut self, sid: u32) -> Result<MixTrace, u32> {
        if sid == 0 || sid > SCEN_COUNT as u32 {
            return Err(MixCode::IdOutOfRange.code());
        }
        let idx = (sid - 1) as usize;
        let cur = match self.states.get(idx) {
            Some(s) => *s,
            None => return Err(MixCode::IdOutOfRange.code()),
        };
        if cur != MixState::Unassembled {
            self.transit_rejects = self.transit_rejects.saturating_add(1);
            return Err(MixCode::StateIllegalTransit.code());
        }
        let row = mix_row(sid)?;
        let src = row_to_src(&row);
        // 链式迁移闸：Unassembled→Assembled→Resolved→终态。
        if !can_transit(MixState::Unassembled, MixState::Assembled)
            || !can_transit(MixState::Assembled, MixState::Resolved)
        {
            return Err(MixCode::StateIllegalTransit.code());
        }
        let (final_val, winner) = resolve_chain(&src)?;
        let deviated = final_val != row.expected_val || winner != row.expected_prio;
        if !can_transit(MixState::Resolved, if deviated { MixState::Deviated } else { MixState::Matched })
        {
            return Err(MixCode::StateIllegalTransit.code());
        }
        let trace = MixTrace {
            sid,
            winner_prio: winner,
            final_val,
            deviated,
        };
        self.traces.push(trace.clone());
        let fin = if deviated { MixState::Deviated } else { MixState::Matched };
        if let Some(st) = self.states.get_mut(idx) {
            *st = fin;
        }
        Ok(trace)
    }

    /// 全绿门：5 例终值与胜出层双列全一致。
    pub fn all_matched(&self) -> bool {
        let mut i = 0;
        while i < SCEN_COUNT {
            let st = match self.states.get(i) {
                Some(s) => *s,
                None => return false,
            };
            if st != MixState::Matched {
                return false;
            }
            i += 1;
        }
        self.traces.len() == SCEN_COUNT
    }

    /// 轨迹账非空闸。
    pub fn has_traces(&self) -> bool {
        self.traces.len() == SCEN_COUNT
    }
}

// ── 黄金全链：5 例全跑双列对拍 ──────────────────────────────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut led = MixLedger::new();
    let mut sid = 1u32;
    while sid <= SCEN_COUNT as u32 {
        led.run_scenario(sid)?;
        sid += 1;
    }
    if !led.all_matched() {
        return Err(MixCode::DeviationFound.code());
    }
    if !led.has_traces() {
        return Err(MixCode::LedgerEmpty.code());
    }
    // 胜出层分布守恒：5 例恰好覆盖层 0/1/2/3/4 各一。
    let mut seen = [0u32; 5];
    let mut i = 0;
    while i < led.traces.len() {
        if let Some(t) = led.traces.get(i) {
            let slot = t.winner_prio as usize;
            if slot >= 5 {
                return Err(MixCode::ChainBroken.code());
            }
            if let Some(c) = seen.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
        i += 1;
    }
    let mut k = 0;
    while k < 5 {
        let c = match seen.get(k) {
            Some(v) => *v,
            None => 0,
        };
        if c != 1 {
            return Err(MixCode::WinnerMismatch.code());
        }
        k += 1;
    }
    Ok((led.traces.len() as u32, BASE_FALLBACK))
}

// ── 注入核：三类违约 ────────────────────────────────────────────
/// 类①：缺源（actx=None）→ MissingSource 显式拒（与让位 0 严格区分）。
pub fn inject_missing_src() -> Result<bool, u32> {
    let s = ScenarioSrc { sid: 1, cond: Some(0x11), actx: None, decl: Some(0x33), compat: Some(0x44) };
    let hit = match resolve_chain(&s) {
        Err(c) => c == MixCode::MissingSource.code(),
        Ok(_) => false,
    };
    // 对照：让位 0 合法（S2 行为证）。
    let abstain = ScenarioSrc { sid: 2, cond: Some(0), actx: Some(0x22), decl: Some(0x33), compat: Some(0x44) };
    let ok_abstain = resolve_chain(&abstain) == Ok((0x22, 1));
    Ok(hit && ok_abstain)
}

/// 类②：场景号越界（0 与 6）→ IdOutOfRange 显式拒。
pub fn inject_id_range() -> Result<bool, u32> {
    let mut led = MixLedger::new();
    let h0 = led.run_scenario(0).is_err();
    let h6 = led.run_scenario(SCEN_COUNT as u32 + 1).is_err();
    Ok(h0 && h6)
}

/// 类③：批内场景号重复 → DupScen 显式拒。
pub fn inject_dup_sid() -> Result<bool, u32> {
    let batch = [1u32, 2, 3, 3, 4];
    let hit = match validate_batch(&batch) {
        Err(c) => c == MixCode::DupScen.code(),
        Ok(_) => false,
    };
    Ok(hit)
}

/// 批闸：场景号闭集内且无重复（乱序允许，重复拒）。
pub fn validate_batch(batch: &[u32]) -> Result<(), u32> {
    if batch.len() == 0 {
        return Err(MixCode::LedgerEmpty.code());
    }
    let mut i = 0;
    while i < batch.len() {
        let a = match batch.get(i) {
            Some(x) => *x,
            None => return Err(MixCode::LedgerEmpty.code()),
        };
        if a == 0 || a > SCEN_COUNT as u32 {
            return Err(MixCode::IdOutOfRange.code());
        }
        let mut j = i + 1;
        while j < batch.len() {
            let b = match batch.get(j) {
                Some(x) => *x,
                None => break,
            };
            if a == b {
                return Err(MixCode::DupScen.code());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 注入检出账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed_total(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 注入 10/10：三类违约逐类断言被显式拒绝。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        led.record(0, inject_missing_src()?);
        led.record(1, inject_id_range()?);
        led.record(2, inject_dup_sid()?);
        r += 1;
    }
    Ok(led)
}

// ── 对账辅助判据 ────────────────────────────────────────────────
/// 双跑一致：黄金全链两次相等 + 裁决表指纹稳定。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MixCode::NotReproducible.code());
    }
    if corpus_fnv() != corpus_fnv() {
        return Err(MixCode::NotReproducible.code());
    }
    Ok(())
}

/// 裁决表指纹（5 行四源值+期望双列拼 FNV）。
pub fn corpus_fnv() -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < MIX_TABLE.len() {
        if let Some(r) = MIX_TABLE.get(i) {
            bytes.push((r.cond & 0xFF) as u8);
            bytes.push((r.actx & 0xFF) as u8);
            bytes.push((r.decl & 0xFF) as u8);
            bytes.push((r.compat & 0xFF) as u8);
            bytes.push((r.expected_val & 0xFF) as u8);
            bytes.push(r.expected_prio);
        }
        i += 1;
    }
    bytes.push((BASE_FALLBACK & 0xFF) as u8);
    fnv1a(&bytes)
}

/// 链序自证：ALL_SOURCES 层号严格 0..4 单调（链语义可复算）。
pub fn chain_order_selfcheck() -> bool {
    let mut p = 0;
    while p < ALL_SOURCES.len() {
        let src = match ALL_SOURCES.get(p) {
            Some(x) => *x,
            None => return false,
        };
        if src.prio() != p as u8 {
            return false;
        }
        p += 1;
    }
    true
}

/// 段位守卫：12 码全落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MixCode::ALL.len() {
        if let Some(c) = MixCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────
/// UNX-F18526 自检判据（14 条 ≤ 20 上限；动态明细 ≤ 8 槽）。
pub fn run_unx18526_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18526 混合场景探针（5 例四源叠加）");

    s.add("J3-段位守卫", segment_guard(), "0x7FF40 窗口内 12 码全落位");

    // J1 主锚点：5 例四源叠加终值与优先级链一致。
    let gold = golden_run();
    let (g_ok, g_txt) = match &gold {
        Ok((n, base)) => (*n == SCEN_COUNT as u32 && *base == BASE_FALLBACK, format!("轨迹={} 基值={:04X}", n, base)),
        Err(c) => (false, format!("黄金链断:{}", detail_of(*c))),
    };
    s.add_dyn("J1-5例链终值一致", g_ok, &g_txt);

    // 胜出层分布守恒（0/1/2/3/4 各恰一例）。
    s.add("J1-胜出层全路径覆盖", gold.is_ok(), "P0/P1/P2/P3/基值 五路各一例");

    // 链序自证。
    s.add("J2-链序自证0..4", chain_order_selfcheck(), "ALL_SOURCES 层号严格单调");

    // 让位与缺源语义区分（S2 让位降层 vs None 拒）。
    let abstain_sem = {
        let s2 = row_to_src(&mix_row(2).unwrap_or(MixRow { sid: 2, cond: 0, actx: 0, decl: 0, compat: 0, expected_val: 0, expected_prio: 0 }));
        let r2 = resolve_chain(&s2);
        r2 == Ok((0x22, 1))
    };
    s.add("J2-让位0与缺源None区分", abstain_sem, "S2 让位降层合法，None 恒拒");

    let inj = inject_rounds(JUDGE_ROUNDS);
    s.add(
        "J2-注入①缺源10/10",
        match &inj { Ok(l) => l.caught[0] == l.tries[0] && l.tries[0] == JUDGE_ROUNDS, Err(_) => false },
        "actx=None MissingSource 拒",
    );
    s.add(
        "J2-注入②越界10/10",
        match &inj { Ok(l) => l.caught[1] == l.tries[1] && l.tries[1] == JUDGE_ROUNDS, Err(_) => false },
        "sid 0/6 全拒",
    );
    s.add(
        "J2-注入③批内重复10/10",
        match &inj { Ok(l) => l.caught[2] == l.tries[2] && l.tries[2] == JUDGE_ROUNDS, Err(_) => false },
        "dup sid DupScen 拒（乱序允许）",
    );

    s.add(
        "J2-漏检恒零",
        match &inj { Ok(l) => l.missed_total() == 0, Err(_) => false },
        "tries = caught + missed，missed=0",
    );

    // 边界：空批 / 全让位基值兜底。
    let boundary = validate_batch(&[]).is_err()
        && resolve_chain(&ScenarioSrc { sid: 5, cond: Some(0), actx: Some(0), decl: Some(0), compat: Some(0) })
            == Ok((BASE_FALLBACK, PRIO_BASE));
    s.add("J3-空批与基值兜底边界", boundary, "空批拒 + 全让位 → 0x00F0 兜底");

    // 状态机：迁移闭集合法恰 4 条。
    let states = [
        MixState::Unassembled,
        MixState::Assembled,
        MixState::Resolved,
        MixState::Matched,
        MixState::Deviated,
    ];
    let mut legal_n = 0usize;
    let mut a = 0;
    while a < states.len() {
        let mut b = 0;
        while b < states.len() {
            let fa = match states.get(a) { Some(x) => *x, None => break };
            let fb = match states.get(b) { Some(x) => *x, None => break };
            if can_transit(fa, fb) {
                legal_n = legal_n.saturating_add(1);
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J2-迁移闭集合法恰4条", legal_n == 4, "5 态 25 组合枚举：合法 4 / 非法 21");

    // 已判场景重跑拒。
    let rerun_ok = {
        let mut led = MixLedger::new();
        let first = led.run_scenario(1).is_ok();
        let second = match led.run_scenario(1) {
            Err(c) => c == MixCode::StateIllegalTransit.code(),
            Ok(_) => false,
        };
        first && second
    };
    s.add("J2-已判场景重跑拒", rerun_ok, "Matched 后再 run_scenario 显式拒");

    // 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run + 裁决表指纹两跑同值");

    // 裁决表指纹非零。
    s.add("J3-裁决表指纹非零", corpus_fnv() != 0, "5 行四源值+期望拼 FNV 稳定");

    // 文案闭集。
    let mut detail_full = true;
    let mut di = 0;
    while di < MixCode::ALL.len() {
        if let Some(c) = MixCode::ALL.get(di) {
            if c.detail().len() == 0 || detail_of(c.code()).len() == 0 {
                detail_full = false;
            }
        }
        di += 1;
    }
    s.add("J3-文案闭集非空", detail_full, "12 码双查非空");

    // 收口。
    s.add("J3-判据条数≤20收口", s.len() < 20, "本模块判据上限纪律（含本条）");

    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_all_checks_green() {
        let set = run_unx18526_checks();
        assert!(set.all_passed(), "UNX-F18526 全判据应全绿: {}", set.red_items_text());
    }

    #[test]
    fn t02_golden_chain_full_path() {
        let g = golden_run();
        assert!(g.is_ok());
        assert_eq!(g.unwrap_or((0, 0)), (5, BASE_FALLBACK));
    }

    #[test]
    fn t03_priority_chain_each_layer() {
        // P0 胜。
        let s1 = ScenarioSrc { sid: 1, cond: Some(0x11), actx: Some(0x22), decl: Some(0x33), compat: Some(0x44) };
        assert_eq!(resolve_chain(&s1), Ok((0x11, 0)));
        // P1 胜（P0 让位）。
        let s2 = ScenarioSrc { sid: 2, cond: Some(0), actx: Some(0x22), decl: Some(0x33), compat: Some(0x44) };
        assert_eq!(resolve_chain(&s2), Ok((0x22, 1)));
        // P2 胜。
        let s3 = ScenarioSrc { sid: 3, cond: Some(0), actx: Some(0), decl: Some(0x33), compat: Some(0x44) };
        assert_eq!(resolve_chain(&s3), Ok((0x33, 2)));
        // P3 胜。
        let s4 = ScenarioSrc { sid: 4, cond: Some(0), actx: Some(0), decl: Some(0), compat: Some(0x44) };
        assert_eq!(resolve_chain(&s4), Ok((0x44, 3)));
        // 基值兜底。
        let s5 = ScenarioSrc { sid: 5, cond: Some(0), actx: Some(0), decl: Some(0), compat: Some(0) };
        assert_eq!(resolve_chain(&s5), Ok((BASE_FALLBACK, PRIO_BASE)));
    }

    #[test]
    fn t04_missing_vs_abstain() {
        // None = 缺源拒。
        let miss = ScenarioSrc { sid: 1, cond: None, actx: Some(0x22), decl: Some(0x33), compat: Some(0x44) };
        assert!(resolve_chain(&miss).is_err());
        // Some(0) = 让位合法。
        let abs = ScenarioSrc { sid: 1, cond: Some(0), actx: Some(0x22), decl: Some(0x33), compat: Some(0x44) };
        assert!(resolve_chain(&abs).is_ok());
    }

    #[test]
    fn t05_injection_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed_total(), 0);
    }

    #[test]
    fn t06_batch_gates() {
        // 乱序合法。
        assert!(validate_batch(&[3, 1, 5, 2, 4]).is_ok());
        // 重复拒。
        assert!(match validate_batch(&[1, 1]) {
            Err(c) => c == MixCode::DupScen.code(),
            Ok(_) => false,
        });
        // 越界拒。
        assert!(validate_batch(&[0]).is_err());
        assert!(validate_batch(&[6]).is_err());
        // 空批拒。
        assert!(validate_batch(&[]).is_err());
    }

    #[test]
    fn t07_state_machine_closed_set() {
        assert!(can_transit(MixState::Unassembled, MixState::Assembled));
        assert!(can_transit(MixState::Assembled, MixState::Resolved));
        assert!(can_transit(MixState::Resolved, MixState::Matched));
        assert!(can_transit(MixState::Resolved, MixState::Deviated));
        assert!(!can_transit(MixState::Unassembled, MixState::Resolved));
        assert!(!can_transit(MixState::Matched, MixState::Assembled));
    }

    #[test]
    fn t08_double_run_deterministic() {
        assert!(repro_check().is_ok());
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        assert_eq!(corpus_fnv(), corpus_fnv());
    }

    #[test]
    fn t09_row_lookup_bounds() {
        assert!(mix_row(0).is_err());
        assert!(mix_row(6).is_err());
        assert!(mix_row(1).is_ok());
        assert!(mix_row(5).is_ok());
    }

    #[test]
    fn t10_segment_and_detail() {
        assert!(segment_guard());
        assert!(chain_order_selfcheck());
        let mut i = 0;
        while i < MixCode::ALL.len() {
            if let Some(c) = MixCode::ALL.get(i) {
                assert_eq!(detail_of(c.code()), c.detail());
                assert!(c.detail().len() > 0);
            }
            i += 1;
        }
        assert_eq!(detail_of(0xDEAD), "未知诊断码：不在 0x7FF40 段闭集内");
    }
}
