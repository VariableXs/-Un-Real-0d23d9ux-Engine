//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18521 · 探针库总纲（50 例分层结构冻结） · 0x7FE00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据锚点：UNX-F18521-J1 50 例分层（兼容值 10/声明 15/ActCtx 10/
//! 条件掩码 10/混合 5）冻结 v1，勾稽 50/50 零缺漏
//! ─────────────────────────────────────────────────────────────────────
//! 本模块结构清单：
//!   - 数据结构：ProbeDef（探针定义：id/层归属/标题 FNV 指纹）、
//!     ProbeSlot（定义 + 状态 + 运行账）、ProbeLedger（50 槽总账）；
//!   - 状态机：ProbeState 五态（Registered→Frozen→Running→Passed/
//!     Deviated），迁移闭集恰 6 条合法、19 条非法一律显式拒；
//!   - 账本：分层数组账（五层槽位/基址闭集）、勾稽账（50/50）、
//!     注入检出账（tries/caught 逐类 10 轮）；
//!   - 错误路径：LedgerCode 十二码闭集，code() = SEG_BASE + n，
//!     detail() 中文闭集，detail_of(u32) 反查；
//!   - 注入核：三类坏账本（id 重复 / 层计数错 / id 缺口）逐类
//!     10 轮断言被 validate() 显式拒绝，漏检恒零。
//! 设计要点：
//!   - 分层冻结 v1：五层槽位（10/15/10/10/5）与 id 区间写死，任何
//!     重建必须逐槽逐字段相等——冻结语义 = 结构指纹不变；
//!   - 勾稽守恒：Σ五层计数 = 50 = 总槽位；id 连续无重复无缺口；
//!   - 状态机纪律：仅 Frozen 可进 Running；Deviated/Passed 回归必须
//!     先经 Frozen（显式重冻结），越级/回退/复活全拒；
//!   - 常量推导：0x7FE00 = 0x7A000 + 0x5E00，0x40 对齐，AI-VX11-1105
//!     A13 批独占区；判据轮数 10 来自 J1「10/10」语义；
//!   - no_std + alloc；内核路径零 unwrap/expect/panic/裸下标/浮点/IO。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FE00，AI-VX11-1105 批 A13 独占区，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7FE00;

// ── 总纲冻结常量 ────────────────────────────────────────────────
/// 探针总数（冻结：50，来自 J1「勾稽 50/50」）。
pub const TOTAL_PROBES: usize = 50;
/// 五层槽位（冻结：兼容值 10 / 声明 15 / ActCtx 10 / 条件掩码 10 / 混合 5）。
pub const TIER_SLOTS: [usize; 5] = [10, 15, 10, 10, 5];
/// 五层 id 基址（连续区间：1-10 / 11-25 / 26-35 / 36-45 / 46-50）。
pub const TIER_BASE_ID: [u32; 5] = [1, 11, 26, 36, 46];
/// 冻结版本 v1（结构指纹升级只允许 (1,0,0)→(1,0,1)，禁止跳版本）。
pub const FROZEN_VER: (u32, u32, u32) = (1, 0, 0);
/// 判据迭代轮数（推导：J1 注入语义「逐类断言被显式拒绝」按 10/10 计）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类数（推导：三类坏账本 = 重复/计数/缺口）。
pub const INJECT_CLASSES: usize = 3;
/// 五态状态机合法迁移条数（推导：迁移表枚举恰 6 条）。
pub const LEGAL_TRANSITS: usize = 6;
/// 五态全组合数（推导：5×5 = 25，非法 = 25 − 6 = 19）。
pub const STATE_COMBOS: usize = 25;
pub const ILLEGAL_TRANSITS: usize = STATE_COMBOS - LEGAL_TRANSITS;

// ── FNV-1a 64 位指纹（确定性，无随机源） ────────────────────────
/// FNV-1a：探针标题/结构指纹的唯一哈希来源（往返判据复算用）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    // 偏移基与素数取 FNV 标准常数；wrapping 防溢出，内核路径零 panic。
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerCode {
    /// 总账空（零槽位建账）
    EmptyLedger,
    /// 探针 id 重复（闭集内两槽同 id）
    DupProbeId,
    /// 层计数与冻结表不符（10/15/10/10/5 之外）
    TierCountMismatch,
    /// id 缺口（跳号/断号）
    IdGap,
    /// id 越界（0 或 >50）
    IdOutOfRange,
    /// 未冻结即运行（结构冻结 v1 纪律）
    NotFrozenV1,
    /// 状态机非法迁移（越级/回退/复活）
    StateIllegalTransit,
    /// 勾稽破坏（Σ层计数 ≠ 50）
    ReconcileBroken,
    /// 层归属未知（id 落不进任何层区间）
    TierUnknown,
    /// 空槽被引用（定义缺失）
    SlotEmpty,
    /// 运行账溢出（饱和上限，账不丢）
    RunOverflow,
    /// 注入漏检（检出 < 注入，红线）
    InjectMissed,
}

impl LedgerCode {
    /// 诊断码 = SEG_BASE + 枚举序（闭集：0x7FE00..0x7FE40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断明细（闭集，每码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            LedgerCode::EmptyLedger => "总账空：零槽位不得建账",
            LedgerCode::DupProbeId => "探针 id 重复：闭集内两槽同 id，账本作废",
            LedgerCode::TierCountMismatch => "层计数错：与冻结表 10/15/10/10/5 不符",
            LedgerCode::IdGap => "id 缺口：跳号/断号，勾稽零缺漏红线",
            LedgerCode::IdOutOfRange => "id 越界：合法闭集 1..=50",
            LedgerCode::NotFrozenV1 => "未冻结即运行：结构冻结 v1 纪律",
            LedgerCode::StateIllegalTransit => "状态机非法迁移：越级/回退/复活全拒",
            LedgerCode::ReconcileBroken => "勾稽破坏：Σ层计数 ≠ 50",
            LedgerCode::TierUnknown => "层归属未知：id 不落任何层区间",
            LedgerCode::SlotEmpty => "空槽被引用：探针定义缺失",
            LedgerCode::RunOverflow => "运行账饱和：u32 上限，账不丢只截断",
            LedgerCode::InjectMissed => "注入漏检：检出 < 注入，验收红线",
        }
    }
    /// 全码闭集（12 条，与枚举一一对应）。
    pub const ALL: [LedgerCode; 12] = [
        LedgerCode::EmptyLedger,
        LedgerCode::DupProbeId,
        LedgerCode::TierCountMismatch,
        LedgerCode::IdGap,
        LedgerCode::IdOutOfRange,
        LedgerCode::NotFrozenV1,
        LedgerCode::StateIllegalTransit,
        LedgerCode::ReconcileBroken,
        LedgerCode::TierUnknown,
        LedgerCode::SlotEmpty,
        LedgerCode::RunOverflow,
        LedgerCode::InjectMissed,
    ];
}

/// u32 诊断码反查中文明细（闭集外返回统一提示）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < LedgerCode::ALL.len() {
        if let Some(c) = LedgerCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FE00 段闭集内"
}

// ── 探针层（五层闭集） ──────────────────────────────────────────
/// 探针五层：兼容值 / supportedOS 声明 / ActCtx 覆写 / 条件掩码 / 混合。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeTier {
    /// 层0 · 兼容值口径（10 例，id 1..=10）
    CompatValue,
    /// 层1 · supportedOS 声明（15 例，id 11..=25）
    Declaration,
    /// 层2 · ActCtx 覆写（10 例，id 26..=35）
    ActCtx,
    /// 层3 · 条件掩码（10 例，id 36..=45）
    CondMask,
    /// 层4 · 混合四源（5 例，id 46..=50）
    Mixed,
}

impl ProbeTier {
    /// 层下标（0..5，冻结表 TIER_SLOTS/TIER_BASE_ID 的行号）。
    pub fn idx(self) -> usize {
        match self {
            ProbeTier::CompatValue => 0,
            ProbeTier::Declaration => 1,
            ProbeTier::ActCtx => 2,
            ProbeTier::CondMask => 3,
            ProbeTier::Mixed => 4,
        }
    }
    /// 层槽位（冻结表取值，越界返回 0 保底）。
    pub fn slots(self) -> usize {
        match TIER_SLOTS.get(self.idx()) {
            Some(n) => *n,
            None => 0,
        }
    }
    /// 层 id 基址（冻结表取值，越界返回 0 保底）。
    pub fn base_id(self) -> u32 {
        match TIER_BASE_ID.get(self.idx()) {
            Some(v) => *v,
            None => 0,
        }
    }
    /// id → 层归属（落不进任何层区间返回 None，用于越界闸）。
    pub fn from_id(id: u32) -> Option<ProbeTier> {
        let tiers = [
            ProbeTier::CompatValue,
            ProbeTier::Declaration,
            ProbeTier::ActCtx,
            ProbeTier::CondMask,
            ProbeTier::Mixed,
        ];
        let mut i = 0;
        while i < tiers.len() {
            let t = match tiers.get(i) {
                Some(x) => *x,
                None => break,
            };
            let base = t.base_id();
            let n = t.slots() as u32;
            if n > 0 && id >= base && id < base.saturating_add(n) {
                return Some(t);
            }
            i += 1;
        }
        None
    }
}

/// 五层闭集全量（判据遍历用）。
pub const ALL_TIERS: [ProbeTier; 5] = [
    ProbeTier::CompatValue,
    ProbeTier::Declaration,
    ProbeTier::ActCtx,
    ProbeTier::CondMask,
    ProbeTier::Mixed,
];

// ── 探针状态机（五态，迁移闭集） ────────────────────────────────
/// 探针生命周期五态：Registered→Frozen→Running→Passed/Deviated。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeState {
    /// 已登记未冻结
    Registered,
    /// 已冻结（可运行）
    Frozen,
    /// 运行中
    Running,
    /// 本轮全绿
    Passed,
    /// 本轮有偏差
    Deviated,
}

/// 迁移闭集：合法恰 LEGAL_TRANSITS(6) 条，其余 ILLEGAL_TRANSITS(19) 全拒。
///
/// 合法集合推导：
///   ① Registered→Frozen（登记后冻结，唯一入 Frozen 通道）
///   ② Frozen→Running（冻结后才可跑）
///   ③ Running→Passed（本轮全绿）
///   ④ Running→Deviated（本轮有偏差）
///   ⑤ Deviated→Frozen（修复后重冻结，回归通道）
///   ⑥ Passed→Frozen（回归复检重冻结，F18530 对接）
pub fn can_transit(from: ProbeState, to: ProbeState) -> bool {
    match (from, to) {
        (ProbeState::Registered, ProbeState::Frozen) => true,
        (ProbeState::Frozen, ProbeState::Running) => true,
        (ProbeState::Running, ProbeState::Passed) => true,
        (ProbeState::Running, ProbeState::Deviated) => true,
        (ProbeState::Deviated, ProbeState::Frozen) => true,
        (ProbeState::Passed, ProbeState::Frozen) => true,
        _ => false,
    }
}

// ── 数据结构：定义与槽位 ────────────────────────────────────────
/// 探针定义（冻结结构：id + 层 + 标题指纹三列）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeDef {
    /// 全局唯一 id（1..=50 连续闭集）。
    pub id: u32,
    /// 层归属（必须与 id 区间一致，错位即 TierUnknown）。
    pub tier: ProbeTier,
    /// 标题 FNV-1a 指纹（标题语料变更即指纹变，冻结可感知）。
    pub title_tag: u64,
}

/// 探针槽位（定义 + 状态 + 运行账三元组）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeSlot {
    /// 冻结定义。
    pub def: ProbeDef,
    /// 当前状态（五态之一）。
    pub state: ProbeState,
    /// 累计运行次数（saturating，账不丢）。
    pub runs: u32,
    /// 累计全绿次数。
    pub green: u32,
    /// 累计偏差次数。
    pub red: u32,
}

/// 探针库总账（50 槽 + 冻结旗 + 版本三元组）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeLedger {
    /// 50 个槽位（id 连续序排列，validate 强制）。
    pub slots: Vec<ProbeSlot>,
    /// 结构冻结旗（v1 冻结后槽位序列不可再改）。
    pub frozen: bool,
    /// 冻结版本三元组（当前恒 (1,0,0)）。
    pub ver: (u32, u32, u32),
}

// ── 标题语料（50 例确定性生成） ─────────────────────────────────
/// 按层确定性生成第 id 号探针标题（语料指纹 = FNV(title)）。
pub fn title_of(id: u32) -> String {
    match ProbeTier::from_id(id) {
        Some(ProbeTier::CompatValue) => format!("probe-cv-{:02}", id),
        Some(ProbeTier::Declaration) => format!("probe-decl-{:02}", id),
        Some(ProbeTier::ActCtx) => format!("probe-actx-{:02}", id),
        Some(ProbeTier::CondMask) => format!("probe-cm-{:02}", id),
        Some(ProbeTier::Mixed) => format!("probe-mix-{:02}", id),
        None => format!("probe-orphan-{:02}", id),
    }
}

impl ProbeLedger {
    /// 建账（50 槽 Registered，逐槽 title 指纹在位）。
    pub fn build() -> Result<ProbeLedger, u32> {
        let mut led = ProbeLedger {
            slots: Vec::new(),
            frozen: false,
            ver: FROZEN_VER,
        };
        let mut id = 1u32;
        while id <= TOTAL_PROBES as u32 {
            let tier = match ProbeTier::from_id(id) {
                Some(t) => t,
                None => return Err(LedgerCode::IdOutOfRange.code()),
            };
            let title = title_of(id);
            let def = ProbeDef {
                id,
                tier,
                title_tag: fnv1a(title.as_bytes()),
            };
            led.slots.push(ProbeSlot {
                def,
                state: ProbeState::Registered,
                runs: 0,
                green: 0,
                red: 0,
            });
            id += 1;
        }
        led.validate()?;
        Ok(led)
    }

    /// 三重验证：槽位规模 / 重复 id / 连续 id 与层归属一致性。
    pub fn validate(&self) -> Result<(), u32> {
        if self.slots.is_empty() {
            return Err(LedgerCode::EmptyLedger.code());
        }
        if self.slots.len() != TOTAL_PROBES {
            return Err(LedgerCode::TierCountMismatch.code());
        }
        // 第一遍：重复 id 扫描（50² 规模下确定、无堆开销）。
        let mut i = 0;
        while i < self.slots.len() {
            let id_i = match self.slots.get(i) {
                Some(s) => s.def.id,
                None => return Err(LedgerCode::SlotEmpty.code()),
            };
            let mut j = i + 1;
            while j < self.slots.len() {
                let id_j = match self.slots.get(j) {
                    Some(s) => s.def.id,
                    None => return Err(LedgerCode::SlotEmpty.code()),
                };
                if id_i == id_j {
                    return Err(LedgerCode::DupProbeId.code());
                }
                j += 1;
            }
            i += 1;
        }
        // 第二遍：id 连续无缺口 + 层归属与 id 区间一致。
        let mut k = 0;
        while k < self.slots.len() {
            let s = match self.slots.get(k) {
                Some(x) => x,
                None => return Err(LedgerCode::SlotEmpty.code()),
            };
            let want = (k as u32).saturating_add(1);
            if s.def.id != want {
                return Err(LedgerCode::IdGap.code());
            }
            match ProbeTier::from_id(s.def.id) {
                Some(t) => {
                    if t != s.def.tier {
                        return Err(LedgerCode::TierUnknown.code());
                    }
                }
                None => return Err(LedgerCode::IdOutOfRange.code()),
            }
            k += 1;
        }
        Ok(())
    }

    /// 勾稽：Σ五层计数 == 50，返回总勾稽数（零缺漏）。
    pub fn reconcile(&self) -> Result<usize, u32> {
        let mut total = 0usize;
        let mut ti = 0;
        while ti < ALL_TIERS.len() {
            let t = match ALL_TIERS.get(ti) {
                Some(x) => *x,
                None => return Err(LedgerCode::TierUnknown.code()),
            };
            total = total.saturating_add(self.count_tier(t));
            ti += 1;
        }
        if total != TOTAL_PROBES {
            return Err(LedgerCode::ReconcileBroken.code());
        }
        Ok(total)
    }

    /// 统计某层在账槽位数。
    pub fn count_tier(&self, tier: ProbeTier) -> usize {
        let mut n = 0usize;
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.def.tier == tier {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 分层计数快照（与 TIER_SLOTS 冻结表逐层对拍）。
    pub fn tier_counts(&self) -> [usize; 5] {
        let mut out = [0usize; 5];
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                let idx = s.def.tier.idx();
                if let Some(cell) = out.get_mut(idx) {
                    *cell = cell.saturating_add(1);
                }
            }
            i += 1;
        }
        out
    }

    /// 结构冻结（幂等：重复冻结零副作用；冻结前强制 validate）。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.frozen {
            return Ok(());
        }
        self.validate()?;
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get_mut(i) {
                if s.state == ProbeState::Registered {
                    s.state = ProbeState::Frozen;
                }
            }
            i += 1;
        }
        self.frozen = true;
        Ok(())
    }

    /// 运行一枚探针（仅 Frozen 可跑；deviated=true 记偏差终态）。
    pub fn run_probe(&mut self, id: u32, deviated: bool) -> Result<ProbeState, u32> {
        if self.frozen == false {
            return Err(LedgerCode::NotFrozenV1.code());
        }
        if id == 0 || id > TOTAL_PROBES as u32 {
            return Err(LedgerCode::IdOutOfRange.code());
        }
        let idx = (id - 1) as usize;
        let st = match self.slots.get(idx) {
            Some(s) => s.state,
            None => return Err(LedgerCode::SlotEmpty.code()),
        };
        // 迁移闸一：当前态必须能合法进 Running（仅 Frozen）。
        if !can_transit(st, ProbeState::Running) {
            return Err(LedgerCode::StateIllegalTransit.code());
        }
        // 迁移闸二：Running 的终态闭集内（Passed/Deviated 恒合法）。
        let fin = if deviated {
            ProbeState::Deviated
        } else {
            ProbeState::Passed
        };
        if !can_transit(ProbeState::Running, fin) {
            return Err(LedgerCode::StateIllegalTransit.code());
        }
        let slot = match self.slots.get_mut(idx) {
            Some(s) => s,
            None => return Err(LedgerCode::SlotEmpty.code()),
        };
        slot.runs = slot.runs.saturating_add(1);
        if deviated {
            slot.red = slot.red.saturating_add(1);
        } else {
            slot.green = slot.green.saturating_add(1);
        }
        slot.state = fin;
        Ok(fin)
    }

    /// 显式状态回写（仅注入核用于验证非法迁移被拒；合法路径才落账）。
    pub fn force_state(&mut self, id: u32, to: ProbeState) -> Result<(), u32> {
        if id == 0 || id > TOTAL_PROBES as u32 {
            return Err(LedgerCode::IdOutOfRange.code());
        }
        let idx = (id - 1) as usize;
        let st = match self.slots.get(idx) {
            Some(s) => s.state,
            None => return Err(LedgerCode::SlotEmpty.code()),
        };
        if !can_transit(st, to) {
            return Err(LedgerCode::StateIllegalTransit.code());
        }
        if let Some(s) = self.slots.get_mut(idx) {
            s.state = to;
        }
        Ok(())
    }

    /// 运行账守恒：Σruns == Σgreen + Σred（逐槽恒等，全局也对）。
    pub fn runs_conserve(&self) -> bool {
        let mut runs = 0u32;
        let mut green = 0u32;
        let mut red = 0u32;
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.runs != s.green.saturating_add(s.red) {
                    return false;
                }
                runs = runs.saturating_add(s.runs);
                green = green.saturating_add(s.green);
                red = red.saturating_add(s.red);
            }
            i += 1;
        }
        runs == green.saturating_add(red)
    }

    /// 结构指纹（全槽 def 三列 + 冻结旗 + 版本拼 FNV；冻结语义可复算）。
    pub fn struct_fnv(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                bytes.push((s.def.id & 0xFF) as u8);
                bytes.push((s.def.id >> 8) as u8);
                bytes.push(s.def.tier.idx() as u8);
                let tag = s.def.title_tag;
                bytes.push((tag & 0xFF) as u8);
                bytes.push(((tag >> 8) & 0xFF) as u8);
                bytes.push(((tag >> 16) & 0xFF) as u8);
                bytes.push(((tag >> 24) & 0xFF) as u8);
            }
            i += 1;
        }
        if self.frozen {
            bytes.push(0xF0);
        }
        bytes.push(FROZEN_VER.0 as u8);
        bytes.push(FROZEN_VER.1 as u8);
        bytes.push(FROZEN_VER.2 as u8);
        fnv1a(&bytes)
    }
}

// ── 注入核：三类坏账本 ──────────────────────────────────────────
/// 类①：id 重复（第 8 槽塞 id 7，且缺 id 8）。
pub fn bad_ledger_dup_id() -> Result<ProbeLedger, u32> {
    let mut led = ProbeLedger {
        slots: Vec::new(),
        frozen: false,
        ver: FROZEN_VER,
    };
    let mut id = 1u32;
    while id <= TOTAL_PROBES as u32 {
        // id 8 位置换成 id 7 → 7 重复、8 缺失。
        let eff = if id == 8 { 7 } else { id };
        let tier = match ProbeTier::from_id(eff) {
            Some(t) => t,
            None => return Err(LedgerCode::IdOutOfRange.code()),
        };
        let title = title_of(eff);
        led.slots.push(ProbeSlot {
            def: ProbeDef {
                id: eff,
                tier,
                title_tag: fnv1a(title.as_bytes()),
            },
            state: ProbeState::Registered,
            runs: 0,
            green: 0,
            red: 0,
        });
        id += 1;
    }
    Ok(led)
}

/// 类②：层计数错（兼容值层 11 例，挤占声明层首槽 id 11）。
pub fn bad_ledger_tier_count() -> Result<ProbeLedger, u32> {
    let mut led = ProbeLedger {
        slots: Vec::new(),
        frozen: false,
        ver: FROZEN_VER,
    };
    let mut id = 1u32;
    while id <= TOTAL_PROBES as u32 {
        // id 11 的层归属错标为兼容值层 → 兼容值 11 / 声明 14。
        let tier = if id == 11 {
            ProbeTier::CompatValue
        } else {
            match ProbeTier::from_id(id) {
                Some(t) => t,
                None => return Err(LedgerCode::IdOutOfRange.code()),
            }
        };
        let title = title_of(id);
        led.slots.push(ProbeSlot {
            def: ProbeDef {
                id,
                tier,
                title_tag: fnv1a(title.as_bytes()),
            },
            state: ProbeState::Registered,
            runs: 0,
            green: 0,
            red: 0,
        });
        id += 1;
    }
    Ok(led)
}

/// 类③：id 缺口（丢 id 30，尾部补 id 51 越界值，缺口+越界双违约）。
///
/// 注意：坏账本构造器允许越界 id 入账（层归属以 Mixed 兜底占位），
/// 违约必须由 validate() 的 IdGap 闸显式拒绝——构造期提前返回会让
/// 注入核失去「坏样本逐类断言被拒」的靶子。
pub fn bad_ledger_id_gap() -> Result<ProbeLedger, u32> {
    let mut led = ProbeLedger {
        slots: Vec::new(),
        frozen: false,
        ver: FROZEN_VER,
    };
    let mut id = 1u32;
    while id <= TOTAL_PROBES as u32 {
        let eff = if id == 30 { 51 } else { id };
        let tier = match ProbeTier::from_id(eff) {
            Some(t) => t,
            None => ProbeTier::Mixed,
        };
        let title = title_of(eff);
        led.slots.push(ProbeSlot {
            def: ProbeDef {
                id: eff,
                tier,
                title_tag: fnv1a(title.as_bytes()),
            },
            state: ProbeState::Registered,
            runs: 0,
            green: 0,
            red: 0,
        });
        id += 1;
    }
    Ok(led)
}

// ── 注入检出账 ──────────────────────────────────────────────────
/// 注入检出账：tries/caught 逐类记账，漏检 = tries − caught 必须为 0。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    /// 每类注入次数。
    pub tries: [u32; INJECT_CLASSES],
    /// 每类显式拒绝次数。
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    /// 空账（三类全零）。
    pub fn new() -> InjectLedger {
        InjectLedger {
            tries: [0; INJECT_CLASSES],
            caught: [0; INJECT_CLASSES],
        }
    }
    /// 记一轮：slot 槽位（0..3），hit=是否被显式拒绝。
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
    /// 全抓断言：每类都试过且 caught == tries（漏检恒零）。
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
    /// 总漏检数（判据魔数来源：三类漏检之和恒 0）。
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

/// 注入 10/10：三类坏账本逐类断言被 validate() 以**预期诊断码**拒绝。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类①：dup id → 必须以 DupProbeId 拒。
        let d = bad_ledger_dup_id()?;
        let hit_d = match d.validate() {
            Err(c) => c == LedgerCode::DupProbeId.code(),
            Ok(_) => false,
        };
        led.record(0, hit_d);
        // 类②：层计数错 → 必须以 TierUnknown 拒（层归属与 id 区间错位）。
        let t = bad_ledger_tier_count()?;
        let hit_t = match t.validate() {
            Err(c) => c == LedgerCode::TierUnknown.code(),
            Ok(_) => false,
        };
        led.record(1, hit_t);
        // 类③：id 缺口（30→51）→ 必须以 IdGap 拒。
        let g = bad_ledger_id_gap()?;
        let hit_g = match g.validate() {
            Err(c) => c == LedgerCode::IdGap.code(),
            Ok(_) => false,
        };
        led.record(2, hit_g);
        r += 1;
    }
    Ok(led)
}

// ── 状态机注入：非法迁移三类全拒 ────────────────────────────────
/// 越级（Registered→Passed）/ 回退（Passed→Registered）/
/// 复活（Deviated→Passed）三违约，各跑 rounds 轮，必须全拒。
pub fn inject_state_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut l1 = ProbeLedger::build()?;
        // 越级：Registered 直接到 Passed。
        let h1 = match l1.force_state(1, ProbeState::Passed) {
            Err(c) => c == LedgerCode::StateIllegalTransit.code(),
            Ok(_) => false,
        };
        led.record(0, h1);
        // 回退：Passed 回 Registered（绿→登记，零回退红线）。
        let mut l2 = ProbeLedger::build()?;
        l2.freeze()?;
        l2.run_probe(2, false)?;
        let h2 = match l2.force_state(2, ProbeState::Registered) {
            Err(c) => c == LedgerCode::StateIllegalTransit.code(),
            Ok(_) => false,
        };
        led.record(1, h2);
        // 复活：Deviated 直接到 Passed（偏差未修不许洗绿）。
        let mut l3 = ProbeLedger::build()?;
        l3.freeze()?;
        l3.run_probe(3, true)?;
        let h3 = match l3.force_state(3, ProbeState::Passed) {
            Err(c) => c == LedgerCode::StateIllegalTransit.code(),
            Ok(_) => false,
        };
        led.record(2, h3);
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：建账→冻结→50 例全绿→勾稽→守恒，返回 (勾稽总数, 全绿数)。
pub fn golden_run() -> Result<(usize, u32), u32> {
    let mut led = ProbeLedger::build()?;
    led.reconcile()?;
    led.freeze()?;
    let mut greened = 0u32;
    let mut id = 1u32;
    while id <= TOTAL_PROBES as u32 {
        match led.run_probe(id, false) {
            Ok(ProbeState::Passed) => greened = greened.saturating_add(1),
            _ => return Err(LedgerCode::ReconcileBroken.code()),
        }
        id += 1;
    }
    if greened != TOTAL_PROBES as u32 {
        return Err(LedgerCode::ReconcileBroken.code());
    }
    if !led.runs_conserve() {
        return Err(LedgerCode::ReconcileBroken.code());
    }
    let total = led.reconcile()?;
    Ok((total, greened))
}

/// 双跑一致：两次黄金全链 + 两次结构指纹逐位相等（确定性红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LedgerCode::ReconcileBroken.code());
    }
    let la = ProbeLedger::build()?;
    let lb = ProbeLedger::build()?;
    if la.struct_fnv() != lb.struct_fnv() {
        return Err(LedgerCode::ReconcileBroken.code());
    }
    Ok(())
}

/// 段位守卫：12 码全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < LedgerCode::ALL.len() {
        if let Some(c) = LedgerCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 标题指纹往返：title_of → FNV → 与槽位登记的 title_tag 逐槽相等。
pub fn title_tag_roundtrip() -> Result<(), u32> {
    let led = ProbeLedger::build()?;
    let mut i = 0;
    while i < led.slots.len() {
        let s = match led.slots.get(i) {
            Some(x) => x,
            None => return Err(LedgerCode::SlotEmpty.code()),
        };
        let tag = fnv1a(title_of(s.def.id).as_bytes());
        if tag != s.def.title_tag {
            return Err(LedgerCode::ReconcileBroken.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 判据收口 ────────────────────────────────────────────────────
/// UNX-F18521 自检判据（16 条 ≤ 20 上限；动态明细 ≤ 8 槽）。
pub fn run_unx18521_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18521 探针库总纲（50 例分层结构冻结）");

    // 判据1 段位守卫：12 码全落 0x7FE00 窗口。
    s.add(
        "J3-段位守卫",
        segment_guard(),
        "0x7FE00 窗口内 12 码全落位",
    );

    // 判据2 勾稽 50/50（J1 主锚点，动态明细给勾稽数）。
    let recon = golden_run();
    let (r_ok, r_total) = match &recon {
        Ok((t, g)) => (*t == TOTAL_PROBES && *g == TOTAL_PROBES as u32, format!("勾稽={} 全绿={}", t, g)),
        Err(c) => (false, format!("黄金链断:{}", detail_of(*c))),
    };
    s.add_dyn("J1-勾稽50/50零缺漏", r_ok, &r_total);

    // 判据3 分层计数与冻结表逐层相等。
    let tier_ok = match ProbeLedger::build() {
        Ok(l) => {
            let c = l.tier_counts();
            let mut eq = true;
            let mut i = 0;
            while i < 5 {
                let want = match TIER_SLOTS.get(i) {
                    Some(w) => *w,
                    None => 0,
                };
                let got = match c.get(i) {
                    Some(v) => *v,
                    None => 0,
                };
                if got != want {
                    eq = false;
                }
                i += 1;
            }
            eq
        }
        Err(_) => false,
    };
    s.add("J1-分层计数10/15/10/10/5", tier_ok, "五层计数与冻结表逐层相等");

    // 判据4 冻结 v1 全量在位（冻结后 50 槽全 Frozen 且 ver=(1,0,0)）。
    let frozen_ok = {
        let mut l = match ProbeLedger::build() {
            Ok(x) => x,
            Err(_) => ProbeLedger { slots: Vec::new(), frozen: false, ver: (0, 0, 0) },
        };
        let fr = l.freeze().is_ok();
        let mut all_frozen = fr;
        let mut i = 0;
        while i < l.slots.len() {
            if let Some(sl) = l.slots.get(i) {
                if sl.state != ProbeState::Frozen {
                    all_frozen = false;
                }
            }
            i += 1;
        }
        all_frozen && l.ver == FROZEN_VER
    };
    s.add("J1-冻结v1全量在位", frozen_ok, "50 槽 Registered→Frozen 全迁移，ver=(1,0,0)");

    // 判据5 id 连续无重复无缺口（正账 validate 必须过）。
    s.add(
        "J1-id连续无重复无缺口",
        ProbeLedger::build().is_ok(),
        "正账 validate 三重闸全过",
    );

    // 判据6 状态机迁移闭集：合法恰 6 条。
    let mut legal_n = 0usize;
    let states = [
        ProbeState::Registered,
        ProbeState::Frozen,
        ProbeState::Running,
        ProbeState::Passed,
        ProbeState::Deviated,
    ];
    let mut a = 0;
    while a < states.len() {
        let mut b = 0;
        while b < states.len() {
            let fa = match states.get(a) {
                Some(x) => *x,
                None => break,
            };
            let fb = match states.get(b) {
                Some(x) => *x,
                None => break,
            };
            if can_transit(fa, fb) {
                legal_n = legal_n.saturating_add(1);
            }
            b += 1;
        }
        a += 1;
    }
    s.add(
        "J2-迁移闭集合法恰6条",
        legal_n == LEGAL_TRANSITS,
        "5×5 组合枚举：合法 6 / 非法 19",
    );

    // 判据7 非法迁移全拒（越级/回退/复活 10 轮 3/3 全抓）。
    let state_inj = inject_state_rounds(JUDGE_ROUNDS);
    let state_ok = match &state_inj {
        Ok(l) => l.all_caught() && l.missed_total() == 0,
        Err(_) => false,
    };
    s.add_dyn(
        "J2-非法迁移注入10/10",
        state_ok,
        &format!("越级/回退/复活 三类 {}", match &state_inj { Ok(l) => format!("caught={:?}", l.caught), Err(c) => format!("断:{}", detail_of(*c)) }),
    );

    // 判据8/9/10 三类坏账本注入（动态明细 ≤8 槽：三类各一条）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J2-注入①id重复10/10", match &inj { Ok(l) => l.caught[0] == l.tries[0] && l.tries[0] == JUDGE_ROUNDS, Err(_) => false }, &format!(" DupProbeId 码={:04X}", LedgerCode::DupProbeId.code()));
    s.add_dyn("J2-注入②层计数10/10", match &inj { Ok(l) => l.caught[1] == l.tries[1] && l.tries[1] == JUDGE_ROUNDS, Err(_) => false }, &format!(" TierUnknown 码={:04X}", LedgerCode::TierUnknown.code()));
    s.add_dyn("J2-注入③id缺口10/10", match &inj { Ok(l) => l.caught[2] == l.tries[2] && l.tries[2] == JUDGE_ROUNDS, Err(_) => false }, &format!(" IdGap 码={:04X}", LedgerCode::IdGap.code()));

    // 判据11 漏检恒零（注入次数 = 检出 + 漏检，漏检 = 0）。
    s.add(
        "J2-漏检恒零",
        match &inj { Ok(l) => l.missed_total() == 0, Err(_) => false },
        "tries = caught + missed，missed=0",
    );

    // 判据12 双跑一致（黄金链 + 结构指纹两次相等）。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑 + struct_fnv 两跑同值");

    // 判据13 计数守恒：Σruns = Σgreen + Σred。
    let conserve = match ProbeLedger::build() {
        Ok(mut l) => {
            let _ = l.freeze();
            let mut ok = true;
            let mut id = 1u32;
            while id <= TOTAL_PROBES as u32 {
                // 逢 7 偏差：7/14/21/28/35/42/49 共 7 例偏差、43 例绿。
                let dev = id % 7 == 0;
                if l.run_probe(id, dev).is_err() {
                    ok = false;
                }
                id += 1;
            }
            ok && l.runs_conserve()
        }
        Err(_) => false,
    };
    s.add("J2-计数守恒runs=green+red", conserve, "逐槽恒等 + 全局恒等（混跑 7 偏差样本）");

    // 判据14 文案闭集：12 码 detail 全非空。
    let mut detail_full = true;
    let mut di = 0;
    while di < LedgerCode::ALL.len() {
        if let Some(c) = LedgerCode::ALL.get(di) {
            if c.detail().len() == 0 || detail_of(c.code()).len() == 0 {
                detail_full = false;
            }
        }
        di += 1;
    }
    s.add("J3-文案闭集非空", detail_full, "12 码 detail() 与 detail_of() 双查非空");

    // 判据15 标题指纹往返。
    s.add("J3-标题指纹往返", title_tag_roundtrip().is_ok(), "title_of→FNV 与登记 title_tag 逐槽相等");

    // 判据16 收口：判据条数 ≤ 20 且本判据在账（账只增不减）。
    s.add(
        "J3-判据条数≤20收口",
        s.len() < 20,
        "本模块判据上限纪律（含本条）",
    );

    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_master_all_checks_green() {
        let set = run_unx18521_checks();
        assert!(set.all_passed(), "UNX-F18521 全判据应全绿: {}", set.red_items_text());
    }

    #[test]
    fn t02_build_and_validate_ok() {
        let led = ProbeLedger::build();
        assert!(led.is_ok(), "正账建账必须过");
        let l = led.unwrap_or(ProbeLedger { slots: Vec::new(), frozen: false, ver: (0, 0, 0) });
        assert_eq!(l.slots.len(), 50);
        assert!(l.validate().is_ok());
    }

    #[test]
    fn t03_tier_counts_frozen_table() {
        let l = ProbeLedger::build().unwrap_or(ProbeLedger { slots: Vec::new(), frozen: false, ver: (0, 0, 0) });
        let c = l.tier_counts();
        assert_eq!(c, [10, 15, 10, 10, 5]);
        assert_eq!(l.reconcile().unwrap_or(0), 50);
    }

    #[test]
    fn t04_injection_three_classes_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed_total(), 0);
        assert_eq!(inj.caught[0], 10);
        assert_eq!(inj.caught[1], 10);
        assert_eq!(inj.caught[2], 10);
    }

    #[test]
    fn t05_state_machine_illegal_rejected() {
        let inj = inject_state_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught(), "越级/回退/复活必须全抓");
    }

    #[test]
    fn t06_boundary_run_before_freeze_rejected() {
        let mut l = ProbeLedger::build().unwrap_or(ProbeLedger { slots: Vec::new(), frozen: false, ver: (0, 0, 0) });
        // 未冻结即运行 → NotFrozenV1。
        assert!(l.run_probe(1, false).is_err());
        // 越界 id：0 与 51。
        let _ = l.freeze();
        assert!(l.run_probe(0, false).is_err());
        assert!(l.run_probe(51, false).is_err());
    }

    #[test]
    fn t07_double_run_deterministic() {
        assert!(repro_check().is_ok());
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
    }

    #[test]
    fn t08_segment_and_detail_closed_set() {
        assert!(segment_guard());
        let mut i = 0;
        while i < LedgerCode::ALL.len() {
            if let Some(c) = LedgerCode::ALL.get(i) {
                assert_eq!(detail_of(c.code()), c.detail());
                assert!(c.detail().len() > 0);
            }
            i += 1;
        }
        assert_eq!(detail_of(0xDEAD), "未知诊断码：不在 0x7FE00 段闭集内");
    }

    #[test]
    fn t09_struct_fnv_stable_and_tier_lookup() {
        let a = ProbeLedger::build().unwrap_or(ProbeLedger { slots: Vec::new(), frozen: false, ver: (0, 0, 0) });
        let b = ProbeLedger::build().unwrap_or(ProbeLedger { slots: Vec::new(), frozen: false, ver: (0, 0, 0) });
        assert_eq!(a.struct_fnv(), b.struct_fnv());
        // 层归属边界：1=兼容值首 / 10=兼容值尾 / 11=声明首 / 25=声明尾 /
        // 26=ActCtx首 / 50=混合尾 / 0 与 51 越界。
        assert_eq!(ProbeTier::from_id(1), Some(ProbeTier::CompatValue));
        assert_eq!(ProbeTier::from_id(10), Some(ProbeTier::CompatValue));
        assert_eq!(ProbeTier::from_id(11), Some(ProbeTier::Declaration));
        assert_eq!(ProbeTier::from_id(25), Some(ProbeTier::Declaration));
        assert_eq!(ProbeTier::from_id(26), Some(ProbeTier::ActCtx));
        assert_eq!(ProbeTier::from_id(50), Some(ProbeTier::Mixed));
        assert_eq!(ProbeTier::from_id(0), None);
        assert_eq!(ProbeTier::from_id(51), None);
    }

    #[test]
    fn t10_title_roundtrip_and_freeze_idempotent() {
        assert!(title_tag_roundtrip().is_ok());
        let mut l = ProbeLedger::build().unwrap_or(ProbeLedger { slots: Vec::new(), frozen: false, ver: (0, 0, 0) });
        let _ = l.freeze();
        let snap = l.struct_fnv();
        // 幂等：二次冻结零副作用。
        let _ = l.freeze();
        assert_eq!(l.struct_fnv(), snap);
        assert!(l.frozen);
    }
}
