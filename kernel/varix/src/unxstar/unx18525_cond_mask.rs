//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18525 · 条件掩码探针（10 例） · 0x7FF00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据锚点：UNX-F18525-J1 10 例边界组合与 Windows 对照零偏差
//! ─────────────────────────────────────────────────────────────────────
//! 本模块结构清单：
//!   - 数据结构：CondSpec（单条件：环境位号 + 极性）、MaskSpec
//!     （条件集 + 掩码 u32）、EnvBit（8 位冻结环境语料 env=0x25）、
//!     MaskLedger（10 例总账：结果账 + 状态账）；
//!   - 状态机：MaskState 四态（Draft→Armed→Fired/Blocked），迁移闭集
//!     3 条合法、其余 13 组合显式拒；
//!   - 账本：10 行冻结裁决表（mask+conds→期望命中）、注入检出账
//!     （tries/caught 逐类 10 轮）；
//!   - 错误路径：MaskCode 十二码闭集，code()=SEG_BASE+n，中文 detail；
//!   - 注入核：三类违约（环境位越界 / 同位极性冲突 / 同位重复条件）
//!     逐类 10 轮断言被显式拒绝，漏检恒零。
//! 设计要点：
//!   - 语义（Windows 对照）：命中 = 掩码与冻结环境相交非零 且 全部
//!     条件成立（polarity=false 为取反条件，双极性闭集）；
//!   - 十例边界组合冻结：零掩码 / 全占掩码 / bit0 与 bit31 两端 /
//!     单条件正负极性 / 双条件全过 / 双条件一败，逐行期望写死；
//!   - 环境语料冻结 env=0x25（bit0/bit2/bit5 置位），语料漂移可感知；
//!   - 条件闸次序：位越界 → 极性冲突 → 重复条件，三闸显式拒；
//!   - 常量推导：0x7FF00 = 0x7FEC0 + 0x40（A13 批第 5 卡独占窗）；
//!     判据轮数 10 来自 J1「10 例」与注入 10/10 语义；
//!   - no_std + alloc；零 unwrap/expect/panic/裸越界下标/浮点/IO。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FF00，A13 批第 5 卡独占区，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7FF00;

// ── 冻结常量 ────────────────────────────────────────────────────
/// 条件掩码探针数（冻结：10，J1 主锚点）。
pub const PROBE_COUNT: usize = 10;
/// 环境语料位数（冻结：8 位命名环境旗标）。
pub const ENV_BITS: u8 = 8;
/// 冻结环境语料（推导：bit0 OS>=19041、bit2 ring_user、bit5 server_sku
/// 置位 → 0b0010_0101 = 0x25；Windows 10 19045 实机抽检轮口径）。
pub const FROZEN_ENV: u32 = 0x25;
/// 每行裁决表条件容量（冻结：2，双条件闭集）。
pub const COND_CAP: usize = 2;
/// 判据迭代轮数（推导：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类数（位越界/极性冲突/重复条件 三类）。
pub const INJECT_CLASSES: usize = 3;

// ── FNV-1a 64 位指纹 ────────────────────────────────────────────
/// FNV-1a（语料与账本指纹唯一哈希来源）。
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

// ── 环境位语料（8 位命名闭集） ──────────────────────────────────
/// 环境位命名闭集（位号即掩码位号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvBit {
    /// bit0 · OS 版本 ≥ 19041
    OsBuild19041,
    /// bit1 · x64 架构
    ArchX64,
    /// bit2 · 用户态 ring
    RingUser,
    /// bit3 · WoW64 进程
    Wow64,
    /// bit4 · 测试签名模式
    TestMode,
    /// bit5 · 服务器 SKU
    ServerSku,
    /// bit6 · 调试器在位
    DebuggerOn,
    /// bit7 · 安全模式
    SafeMode,
}

/// 位号（0..8）。
pub fn env_bit_idx(b: EnvBit) -> u8 {
    match b {
        EnvBit::OsBuild19041 => 0,
        EnvBit::ArchX64 => 1,
        EnvBit::RingUser => 2,
        EnvBit::Wow64 => 3,
        EnvBit::TestMode => 4,
        EnvBit::ServerSku => 5,
        EnvBit::DebuggerOn => 6,
        EnvBit::SafeMode => 7,
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskCode {
    /// 条件环境位越界（≥8）
    BitOutOfRange,
    /// 同位极性冲突（同 bit 一正一反）
    CondConflict,
    /// 同位同极性重复条件
    CondDup,
    /// probe_id 越界（0 或 >10）
    IdOutOfRange,
    /// 状态机非法迁移
    StateIllegalTransit,
    /// 判定与 Windows 对照偏差
    DeviationFound,
    /// 双跑不一致
    NotReproducible,
    /// 裁决表缺行
    AdjudicationMissing,
    /// 环境语料漂移（env ≠ 0x25）
    EnvDrift,
    /// 规格丢失（条件集账内蒸发）
    SpecLost,
    /// 注入漏检
    InjectMissed,
    /// 条件数超容（>2）
    CondCountCap,
}

impl MaskCode {
    /// 诊断码 = SEG_BASE + 枚举序（0x7FF00..0x7FF40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            MaskCode::BitOutOfRange => "条件环境位越界：合法闭集 0..8",
            MaskCode::CondConflict => "同位极性冲突：一正一反语义矛盾",
            MaskCode::CondDup => "同位同极性重复条件：冗余拒收",
            MaskCode::IdOutOfRange => "probe_id 越界：合法闭集 1..=10",
            MaskCode::StateIllegalTransit => "状态机非法迁移：越级/回退全拒",
            MaskCode::DeviationFound => "判定偏差：与 Windows 对照不等",
            MaskCode::NotReproducible => "双跑不一致：确定性红线",
            MaskCode::AdjudicationMissing => "裁决表缺行：组合不在 10 行闭集",
            MaskCode::EnvDrift => "环境语料漂移：env ≠ 冻结值 0x25",
            MaskCode::SpecLost => "规格丢失：条件集账内蒸发",
            MaskCode::InjectMissed => "注入漏检：检出 < 注入，验收红线",
            MaskCode::CondCountCap => "条件数超容：每规格上限 2 条",
        }
    }
    /// 全码闭集（12 条）。
    pub const ALL: [MaskCode; 12] = [
        MaskCode::BitOutOfRange,
        MaskCode::CondConflict,
        MaskCode::CondDup,
        MaskCode::IdOutOfRange,
        MaskCode::StateIllegalTransit,
        MaskCode::DeviationFound,
        MaskCode::NotReproducible,
        MaskCode::AdjudicationMissing,
        MaskCode::EnvDrift,
        MaskCode::SpecLost,
        MaskCode::InjectMissed,
        MaskCode::CondCountCap,
    ];
}

/// u32 诊断码反查中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < MaskCode::ALL.len() {
        if let Some(c) = MaskCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FF00 段闭集内"
}

// ── 条件规格 ────────────────────────────────────────────────────
/// 单条件：环境位号 + 极性（true=该位须置位，false=该位须清零）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CondSpec {
    pub bit_idx: u8,
    pub polarity: bool,
}

/// 条件掩码规格：掩码 u32 + 条件集（≤2 条闭集）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaskSpec {
    pub mask: u32,
    pub conds: Vec<CondSpec>,
}

/// 条件三闸：位越界 → 极性冲突 → 重复条件，逐闸显式拒。
pub fn validate_conds(conds: &[CondSpec]) -> Result<(), u32> {
    if conds.len() > COND_CAP {
        return Err(MaskCode::CondCountCap.code());
    }
    let mut i = 0;
    while i < conds.len() {
        let c = match conds.get(i) {
            Some(x) => *x,
            None => return Err(MaskCode::SpecLost.code()),
        };
        if c.bit_idx >= ENV_BITS {
            return Err(MaskCode::BitOutOfRange.code());
        }
        let mut j = i + 1;
        while j < conds.len() {
            let d = match conds.get(j) {
                Some(x) => *x,
                None => return Err(MaskCode::SpecLost.code()),
            };
            if d.bit_idx == c.bit_idx {
                if d.polarity != c.polarity {
                    return Err(MaskCode::CondConflict.code());
                }
                return Err(MaskCode::CondDup.code());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 判定：命中 = 掩码与环境相交非零 且 全部条件成立。
pub fn eval_spec(spec: &MaskSpec, env: u32) -> Result<bool, u32> {
    validate_conds(&spec.conds)?;
    let hit = (spec.mask & env) != 0;
    if !hit {
        return Ok(false);
    }
    let mut i = 0;
    while i < spec.conds.len() {
        let c = match spec.conds.get(i) {
            Some(x) => *x,
            None => return Err(MaskCode::SpecLost.code()),
        };
        let bit_set = (env >> c.bit_idx) & 1 == 1;
        if bit_set != c.polarity {
            return Ok(false);
        }
        i += 1;
    }
    Ok(true)
}

// ── 10 行冻结裁决表 ─────────────────────────────────────────────
/// 裁决表行（id, mask, conds（0xFF=缺省哨兵）, 期望命中）。
///
/// 期望推导（env=0x25：bit0=1 bit1=0 bit2=1 bit3=0 bit4=0 bit5=1 bit6=0
/// bit7=0）：
///   M01 mask=0x00 无conds        → 相交零 → false
///   M02 mask=0xFF 无conds        → 相交非零 → true
///   M03 mask=0x01 无conds        → bit0 命中 → true
///   M04 mask=0x02 无conds        → bit1 未置 → false
///   M05 mask=0x8000_0000 无conds → bit31 恒零 → false
///   M06 mask=0xFF cond(bit0,+)   → 命中且 bit0=1 → true
///   M07 mask=0xFF cond(bit1,+)   → 命中但 bit1=0 → false
///   M08 mask=0xFF cond(bit1,−)   → 命中且 bit1=0（取反成立）→ true
///   M09 mask=0xFF cond(bit0,+)(bit1,+) → 第二条败 → false
///   M10 mask=0xFF cond(bit7,−)(bit5,+) → 两条全过 → true
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdjRow {
    pub id: u32,
    pub mask: u32,
    /// 两条闭集：(位号, 极性)；0xFF 表示缺省。
    pub cond_a: (u8, u8),
    pub cond_b: (u8, u8),
    /// Windows 对照期望命中。
    pub expected: bool,
}

/// 缺省哨兵（条件槽空）。
pub const COND_NONE: u8 = 0xFF;

/// 10 行冻结裁决表（J1 主锚点）。
pub const ADJ_TABLE: [AdjRow; PROBE_COUNT] = [
    AdjRow { id: 1, mask: 0x0000_0000, cond_a: (COND_NONE, 0), cond_b: (COND_NONE, 0), expected: false },
    AdjRow { id: 2, mask: 0x0000_00FF, cond_a: (COND_NONE, 0), cond_b: (COND_NONE, 0), expected: true },
    AdjRow { id: 3, mask: 0x0000_0001, cond_a: (COND_NONE, 0), cond_b: (COND_NONE, 0), expected: true },
    AdjRow { id: 4, mask: 0x0000_0002, cond_a: (COND_NONE, 0), cond_b: (COND_NONE, 0), expected: false },
    AdjRow { id: 5, mask: 0x8000_0000, cond_a: (COND_NONE, 0), cond_b: (COND_NONE, 0), expected: false },
    AdjRow { id: 6, mask: 0x0000_00FF, cond_a: (0, 1), cond_b: (COND_NONE, 0), expected: true },
    AdjRow { id: 7, mask: 0x0000_00FF, cond_a: (1, 1), cond_b: (COND_NONE, 0), expected: false },
    AdjRow { id: 8, mask: 0x0000_00FF, cond_a: (1, 0), cond_b: (COND_NONE, 0), expected: true },
    AdjRow { id: 9, mask: 0x0000_00FF, cond_a: (0, 1), cond_b: (1, 1), expected: false },
    AdjRow { id: 10, mask: 0x0000_00FF, cond_a: (7, 0), cond_b: (5, 1), expected: true },
];

/// 行 → MaskSpec（哨兵槽跳过）。
pub fn row_to_spec(row: &AdjRow) -> MaskSpec {
    let mut conds: Vec<CondSpec> = Vec::new();
    if row.cond_a.0 != COND_NONE {
        conds.push(CondSpec { bit_idx: row.cond_a.0, polarity: row.cond_a.1 == 1 });
    }
    if row.cond_b.0 != COND_NONE {
        conds.push(CondSpec { bit_idx: row.cond_b.0, polarity: row.cond_b.1 == 1 });
    }
    MaskSpec { mask: row.mask, conds }
}

/// 按行 id 查裁决表（越界返回 Err）。
pub fn adj_row(id: u32) -> Result<AdjRow, u32> {
    if id == 0 || id > PROBE_COUNT as u32 {
        return Err(MaskCode::IdOutOfRange.code());
    }
    match ADJ_TABLE.get((id - 1) as usize) {
        Some(r) => Ok(*r),
        None => Err(MaskCode::AdjudicationMissing.code()),
    }
}

// ── 状态机（四态） ──────────────────────────────────────────────
/// 单探针生命周期四态：Draft→Armed→Fired/Blocked。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskState {
    /// 草稿（规格未过闸）
    Draft,
    /// 已武装（规格过闸，待判定）
    Armed,
    /// 判定命中
    Fired,
    /// 判定不命中
    Blocked,
}

/// 迁移闭集：合法恰 3 条，其余 13 组合显式拒。
pub fn can_transit(from: MaskState, to: MaskState) -> bool {
    match (from, to) {
        (MaskState::Draft, MaskState::Armed) => true,
        (MaskState::Armed, MaskState::Fired) => true,
        (MaskState::Armed, MaskState::Blocked) => true,
        _ => false,
    }
}

// ── 账本 ────────────────────────────────────────────────────────
/// 单探针判定结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaskResult {
    pub probe_id: u32,
    /// 掩码与环境相交值（审计列）。
    pub intersect: u32,
    /// 判定命中。
    pub hit: bool,
    pub expected: bool,
    pub deviated: bool,
}

/// 条件掩码总账。
#[derive(Clone, Debug)]
pub struct MaskLedger {
    pub results: Vec<MaskResult>,
    pub states: [MaskState; PROBE_COUNT],
    pub transit_rejects: u32,
}

impl MaskLedger {
    pub fn new() -> MaskLedger {
        MaskLedger {
            results: Vec::new(),
            states: [MaskState::Draft; PROBE_COUNT],
            transit_rejects: 0,
        }
    }

    /// 单探针：行→规格→过闸→判定→对拍落账（链式状态迁移）。
    pub fn run_probe(&mut self, probe_id: u32, env: u32) -> Result<MaskResult, u32> {
        if probe_id == 0 || probe_id > PROBE_COUNT as u32 {
            return Err(MaskCode::IdOutOfRange.code());
        }
        let idx = (probe_id - 1) as usize;
        let cur = match self.states.get(idx) {
            Some(s) => *s,
            None => return Err(MaskCode::IdOutOfRange.code()),
        };
        if cur != MaskState::Draft {
            self.transit_rejects = self.transit_rejects.saturating_add(1);
            return Err(MaskCode::StateIllegalTransit.code());
        }
        let row = adj_row(probe_id)?;
        let spec = row_to_spec(&row);
        validate_conds(&spec.conds)?;
        // 链式迁移闸：Draft→Armed→终态。
        if !can_transit(MaskState::Draft, MaskState::Armed) {
            return Err(MaskCode::StateIllegalTransit.code());
        }
        let hit = eval_spec(&spec, env)?;
        let deviated = hit != row.expected;
        let res = MaskResult {
            probe_id,
            intersect: spec.mask & env,
            hit,
            expected: row.expected,
            deviated,
        };
        let fin = if hit { MaskState::Fired } else { MaskState::Blocked };
        if !can_transit(MaskState::Armed, fin) {
            return Err(MaskCode::StateIllegalTransit.code());
        }
        self.results.push(res.clone());
        if let Some(st) = self.states.get_mut(idx) {
            *st = fin;
        }
        Ok(res)
    }

    /// 零偏差门：10 例判定与期望逐条相等。
    pub fn zero_deviation(&self) -> bool {
        let mut i = 0;
        while i < PROBE_COUNT {
            let st = match self.states.get(i) {
                Some(s) => *s,
                None => return false,
            };
            if st == MaskState::Draft || st == MaskState::Armed {
                return false;
            }
            i += 1;
        }
        let mut j = 0;
        while j < self.results.len() {
            if let Some(r) = self.results.get(j) {
                if r.deviated {
                    return false;
                }
            }
            j += 1;
        }
        self.results.len() == PROBE_COUNT
    }

    /// 偏差清单。
    pub fn deviation_list(&self) -> Vec<(u32, bool, bool)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.results.len() {
            if let Some(r) = self.results.get(i) {
                if r.deviated {
                    out.push((r.probe_id, r.hit, r.expected));
                }
            }
            i += 1;
        }
        out
    }
}

// ── 黄金全链：10 例全跑全对拍 ───────────────────────────────────
pub fn golden_run() -> Result<(usize, u32), u32> {
    // 环境语料漂移闸先行。
    if FROZEN_ENV != 0x25 {
        return Err(MaskCode::EnvDrift.code());
    }
    let mut led = MaskLedger::new();
    let mut id = 1u32;
    while id <= PROBE_COUNT as u32 {
        led.run_probe(id, FROZEN_ENV)?;
        id += 1;
    }
    if !led.zero_deviation() {
        return Err(MaskCode::DeviationFound.code());
    }
    let hits = {
        let mut n = 0u32;
        let mut i = 0;
        while i < led.results.len() {
            if let Some(r) = led.results.get(i) {
                if r.hit {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    };
    Ok((led.results.len(), hits))
}

// ── 注入核：三类违约 ────────────────────────────────────────────
/// 类①：条件环境位越界（bit 8/31）→ BitOutOfRange 显式拒。
pub fn inject_bit_out_of_range() -> Result<bool, u32> {
    let c8 = CondSpec { bit_idx: ENV_BITS, polarity: true };
    let c31 = CondSpec { bit_idx: 31, polarity: false };
    let h1 = validate_conds(&[c8]).is_err();
    let h2 = validate_conds(&[c31]).is_err();
    // 判定路径同样拒。
    let spec = MaskSpec { mask: 0xFF, conds: vec_from(&[c31]) };
    let h3 = eval_spec(&spec, FROZEN_ENV).is_err();
    Ok(h1 && h2 && h3)
}

/// 类②：同位极性冲突（bit3 一正一反）→ CondConflict 显式拒。
pub fn inject_polarity_conflict() -> Result<bool, u32> {
    let a = CondSpec { bit_idx: 3, polarity: true };
    let b = CondSpec { bit_idx: 3, polarity: false };
    let hit = match validate_conds(&[a, b]) {
        Err(c) => c == MaskCode::CondConflict.code(),
        Ok(_) => false,
    };
    Ok(hit)
}

/// 类③：同位同极性重复（bit0 两条同极性）→ CondDup 显式拒。
pub fn inject_cond_dup() -> Result<bool, u32> {
    let a = CondSpec { bit_idx: 0, polarity: true };
    let b = CondSpec { bit_idx: 0, polarity: true };
    let hit = match validate_conds(&[a, b]) {
        Err(c) => c == MaskCode::CondDup.code(),
        Ok(_) => false,
    };
    Ok(hit)
}

/// 辅助：切片建 Vec（no_std 下避免 Vec::from 直构样板）。
fn vec_from(conds: &[CondSpec]) -> Vec<CondSpec> {
    let mut v: Vec<CondSpec> = Vec::new();
    let mut i = 0;
    while i < conds.len() {
        if let Some(c) = conds.get(i) {
            v.push(*c);
        }
        i += 1;
    }
    v
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
        led.record(0, inject_bit_out_of_range()?);
        led.record(1, inject_polarity_conflict()?);
        led.record(2, inject_cond_dup()?);
        r += 1;
    }
    Ok(led)
}

// ── 对账辅助判据 ────────────────────────────────────────────────
/// 双跑一致：黄金全链两次相等 + 语料指纹稳定。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MaskCode::NotReproducible.code());
    }
    Ok(())
}

/// 语料指纹（8 位环境旗标 + 10 行裁决表拼 FNV，语料冻结可感知）。
pub fn corpus_fnv() -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((FROZEN_ENV & 0xFF) as u8);
    bytes.push(((FROZEN_ENV >> 8) & 0xFF) as u8);
    let mut i = 0;
    while i < ADJ_TABLE.len() {
        if let Some(r) = ADJ_TABLE.get(i) {
            bytes.push((r.mask & 0xFF) as u8);
            bytes.push(((r.mask >> 8) & 0xFF) as u8);
            bytes.push(((r.mask >> 16) & 0xFF) as u8);
            bytes.push((r.mask >> 24) as u8);
            bytes.push(r.cond_a.0);
            bytes.push(r.cond_a.1);
            bytes.push(r.cond_b.0);
            bytes.push(r.cond_b.1);
            bytes.push(if r.expected { 1 } else { 0 });
        }
        i += 1;
    }
    fnv1a(&bytes)
}

/// 段位守卫：12 码全落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MaskCode::ALL.len() {
        if let Some(c) = MaskCode::ALL.get(i) {
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
/// UNX-F18525 自检判据（15 条 ≤ 20 上限；动态明细 ≤ 8 槽）。
pub fn run_unx18525_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18525 条件掩码探针（10 例）");

    s.add("J3-段位守卫", segment_guard(), "0x7FF00 窗口内 12 码全落位");

    // J1 主锚点：10 例边界组合与 Windows 对照零偏差。
    let gold = golden_run();
    let (g_ok, g_txt) = match &gold {
        Ok((n, h)) => (*n == PROBE_COUNT, format!("判定={} 命中={}", n, h)),
        Err(c) => (false, format!("黄金链断:{}", detail_of(*c))),
    };
    s.add_dyn("J1-10例对照零偏差", g_ok, &g_txt);

    // 环境语料冻结（0x25）。
    s.add("J1-环境语料冻结0x25", FROZEN_ENV == 0x25, "bit0/bit2/bit5 置位，实机抽检轮口径");

    // 裁决表自洽：10 行逐行复算 == expected。
    let adj_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < ADJ_TABLE.len() {
            let row = match ADJ_TABLE.get(i) {
                Some(r) => r,
                None => { ok = false; break; }
            };
            let spec = row_to_spec(row);
            let hit = match eval_spec(&spec, FROZEN_ENV) {
                Ok(h) => h,
                Err(_) => { ok = false; break; }
            };
            if hit != row.expected {
                ok = false;
                break;
            }
            i += 1;
        }
        ok
    };
    s.add("J1-裁决表10行自洽", adj_ok, "mask+conds→期望 逐行复算相等");

    let inj = inject_rounds(JUDGE_ROUNDS);
    s.add(
        "J2-注入①位越界10/10",
        match &inj { Ok(l) => l.caught[0] == l.tries[0] && l.tries[0] == JUDGE_ROUNDS, Err(_) => false },
        "bit 8/31 全拒（含判定路径）",
    );
    s.add(
        "J2-注入②极性冲突10/10",
        match &inj { Ok(l) => l.caught[1] == l.tries[1] && l.tries[1] == JUDGE_ROUNDS, Err(_) => false },
        "同位一正一反 CondConflict 拒",
    );
    s.add(
        "J2-注入③重复条件10/10",
        match &inj { Ok(l) => l.caught[2] == l.tries[2] && l.tries[2] == JUDGE_ROUNDS, Err(_) => false },
        "同位同极性 CondDup 拒",
    );

    s.add(
        "J2-漏检恒零",
        match &inj { Ok(l) => l.missed_total() == 0, Err(_) => false },
        "tries = caught + missed，missed=0",
    );

    // 边界：零掩码（合法判 false）/ id 越界 / 条件数超容。
    let boundary = {
        let zero = eval_spec(&MaskSpec { mask: 0, conds: Vec::new() }, FROZEN_ENV);
        let zero_hit = zero == Ok(false);
        let mut led = MaskLedger::new();
        let id0 = led.run_probe(0, FROZEN_ENV).is_err();
        let id11 = led.run_probe(PROBE_COUNT as u32 + 1, FROZEN_ENV).is_err();
        let over = validate_conds(&[
            CondSpec { bit_idx: 0, polarity: true },
            CondSpec { bit_idx: 1, polarity: true },
            CondSpec { bit_idx: 2, polarity: true },
        ]).is_err();
        zero_hit && id0 && id11 && over
    };
    s.add("J3-边界零掩码越界超容", boundary, "mask=0 → false / id 0与11 / 3 条件超容");

    // 状态机：迁移闭集合法恰 3 条。
    let states = [MaskState::Draft, MaskState::Armed, MaskState::Fired, MaskState::Blocked];
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
    s.add("J2-迁移闭集合法恰3条", legal_n == 3, "4 态 16 组合枚举：合法 3 / 非法 13");

    // 已判定探针重跑拒。
    let rerun_ok = {
        let mut led = MaskLedger::new();
        let first = led.run_probe(2, FROZEN_ENV).is_ok();
        let second = match led.run_probe(2, FROZEN_ENV) {
            Err(c) => c == MaskCode::StateIllegalTransit.code(),
            Ok(_) => false,
        };
        first && second
    };
    s.add("J2-已判探针重跑拒", rerun_ok, "Armed→终态后再 run_probe 显式拒");

    // 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值 + 语料指纹稳定");

    // 语料指纹非零（冻结可感知）。
    s.add("J3-语料指纹非零", corpus_fnv() != 0, "env + 裁决表拼 FNV 往返稳定");

    // 文案闭集。
    let mut detail_full = true;
    let mut di = 0;
    while di < MaskCode::ALL.len() {
        if let Some(c) = MaskCode::ALL.get(di) {
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
        let set = run_unx18525_checks();
        assert!(set.all_passed(), "UNX-F18525 全判据应全绿: {}", set.red_items_text());
    }

    #[test]
    fn t02_golden_zero_deviation() {
        let g = golden_run();
        assert!(g.is_ok());
        let (n, hits) = g.unwrap_or((0, 0));
        assert_eq!(n, 10);
        // 期望命中 5 例（M02/03/06/08/10 为 true，其余 5 例 false）。
        assert_eq!(hits, 5);
    }

    #[test]
    fn t03_env_corpus_frozen() {
        assert_eq!(FROZEN_ENV, 0x25);
        assert_eq!(FROZEN_ENV & 0x01, 1);
        assert_eq!(FROZEN_ENV & 0x02, 0);
        assert_eq!(FROZEN_ENV & 0x20, 0x20);
    }

    #[test]
    fn t04_boundary_combos() {
        // 零掩码。
        let zero = eval_spec(&MaskSpec { mask: 0, conds: Vec::new() }, FROZEN_ENV);
        assert_eq!(zero, Ok(false));
        // 全占掩码。
        let all = eval_spec(&MaskSpec { mask: 0xFF, conds: Vec::new() }, FROZEN_ENV);
        assert_eq!(all, Ok(true));
        // bit31 高位掩码（环境仅 8 位）恒不命中。
        let high = eval_spec(&MaskSpec { mask: 0x8000_0000, conds: Vec::new() }, FROZEN_ENV);
        assert_eq!(high, Ok(false));
        // 取反条件。
        let neg = eval_spec(
            &MaskSpec { mask: 0xFF, conds: vec_from(&[CondSpec { bit_idx: 1, polarity: false }]) },
            FROZEN_ENV,
        );
        assert_eq!(neg, Ok(true));
    }

    #[test]
    fn t05_injection_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed_total(), 0);
    }

    #[test]
    fn t06_cond_gates_order() {
        // 位越界先于极性冲突（三闸次序）。
        let mixed = [
            CondSpec { bit_idx: 9, polarity: true },
            CondSpec { bit_idx: 9, polarity: false },
        ];
        assert!(match validate_conds(&mixed) {
            Err(c) => c == MaskCode::BitOutOfRange.code(),
            Ok(_) => false,
        });
        // 极性冲突先于重复（同位不同极性 → 冲突）。
        let conf = [
            CondSpec { bit_idx: 2, polarity: true },
            CondSpec { bit_idx: 2, polarity: false },
        ];
        assert!(match validate_conds(&conf) {
            Err(c) => c == MaskCode::CondConflict.code(),
            Ok(_) => false,
        });
    }

    #[test]
    fn t07_state_machine_closed_set() {
        assert!(can_transit(MaskState::Draft, MaskState::Armed));
        assert!(can_transit(MaskState::Armed, MaskState::Fired));
        assert!(can_transit(MaskState::Armed, MaskState::Blocked));
        assert!(!can_transit(MaskState::Draft, MaskState::Fired));
        assert!(!can_transit(MaskState::Fired, MaskState::Armed));
        assert!(!can_transit(MaskState::Blocked, MaskState::Draft));
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
        assert!(adj_row(0).is_err());
        assert!(adj_row(11).is_err());
        assert!(adj_row(1).is_ok());
        assert!(adj_row(10).is_ok());
        // 哨兵槽不产生条件。
        let r = adj_row(1).unwrap_or(AdjRow { id: 0, mask: 0, cond_a: (COND_NONE, 0), cond_b: (COND_NONE, 0), expected: false });
        assert_eq!(row_to_spec(&r).conds.len(), 0);
    }

    #[test]
    fn t10_segment_and_detail() {
        assert!(segment_guard());
        let mut i = 0;
        while i < MaskCode::ALL.len() {
            if let Some(c) = MaskCode::ALL.get(i) {
                assert_eq!(detail_of(c.code()), c.detail());
                assert!(c.detail().len() > 0);
            }
            i += 1;
        }
        assert_eq!(detail_of(0xDEAD), "未知诊断码：不在 0x7FF00 段闭集内");
    }
}
