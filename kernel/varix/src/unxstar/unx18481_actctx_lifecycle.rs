//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18481 · ActCtx 覆写面总纲（生命周期冻结） · 0x7F400 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18481-J1 覆写生命周期（创建→生效→查询→撤销）四段冻结 v1，
//!       跳段注入 10/10 拒绝
//! 本模块结构清单：
//!   1. 冻结常量层：账本容量 12 / 生命周期四段闭集 / 判据轮数 10（推导注释）；
//!   2. 诊断码闭集 LcCode（code()=SEG_BASE+n，detail() 中文闭集逐码非空）；
//!   3. 数据结构：
//!      - 数组账本 OvrLedger：[Option<OvrRec>; 12] 定容记录表；
//!      - 位面 PhasePlane：u16 位图，第 i 位=槽 i 的"已到四段末"标记；
//!      - 状态机 LifePhase：Created→Active→Queried→Revoked 单向四态；
//!   4. 错误路径：跳段（create→query / create→revoke / activate→revoke
//!      / revoke 后再 query）四类显式拒绝码 + 重复创建 + 幽灵 id + 表满；
//!   5. 注入核：跳段 / 重复创建 / 幽灵 id / 撤销后复活 四类坏样本，
//!      逐类 10/10 断言被显式拒绝，计数守恒（注入=检出+漏检，漏检必 0）；
//!   6. 黄金全链 golden_run：8 条正例全链走通→账目守恒→双跑逐字段一致；
//!   7. run_unx18481_checks：14 条判据（断言性质：守恒/单调/幂等/确定性/
//!      非法必拒），运行期文本走 add_dyn（≤8 槽纪律）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段基址（0x7F400，G1 工单 A09 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F400;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 覆写账本容量（写死 12）：四段生命周期 × 3 个并发样板位，12=4×3 便于
/// 按 4 对折校验段位分布；扩容须换版 v2。
pub const OVR_CAP: usize = 12;
/// 生命周期段数（冻结 v1：创建→生效→查询→撤销，恰为 4 段）。
pub const LIFE_SEGS: u8 = 4;
/// 覆写 id 域上界（冻结：1..=96，0 保留给"账本自身"哨兵）。
pub const OVR_ID_MAX: u32 = 96;
/// 判据轮数（写死 10：判据原文"跳段注入 10/10 拒绝"取 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 账本版本号（冻结 v1）。
pub const LEDGER_VER_V1: u32 = 1;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LcCode {
    /// 幽灵 id：账本查无此覆写
    GhostId,
    /// 重复创建：同 id 已在场（生命周期只许建一次）
    DupCreate,
    /// 表满：12 槽用尽
    LedgerFull,
    /// 跳段-查询：未生效即查询（Created→Queried 越级）
    SkipToQuery,
    /// 跳段-撤销：未生效即撤销（Created→Revoked 越级）
    SkipToRevoke,
    /// 跳段-撤销：已生效未查询即撤销（Active→Revoked 越级）
    RevokeBeforeQuery,
    /// 撤销后复活：已撤销态再查询（单向状态机倒退）
    QueryAfterRevoke,
    /// 撤销二次：已撤销态再撤销（重复终态）
    RevokeTwice,
    /// id 越界：超出 1..=96 闭集或撞保留字 0
    IdOutOfRange,
    /// 守恒破坏：入账数≠存活+撤销+拒绝
    ConservationBroken,
    /// 段位分布破坏：存活记录数与位面计数不符
    PhasePlaneMismatch,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl LcCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            LcCode::GhostId => "幽灵 id：账本查无此覆写",
            LcCode::DupCreate => "重复创建：同 id 已在场，v1 生命周期只许建一次",
            LcCode::LedgerFull => "表满：12 槽用尽，须升版扩容",
            LcCode::SkipToQuery => "跳段-查询：未生效即查询，须先 activate",
            LcCode::SkipToRevoke => "跳段-撤销：未生效即撤销，须先 activate",
            LcCode::RevokeBeforeQuery => "跳段-撤销：已生效未查询即撤销，须先过查询段",
            LcCode::QueryAfterRevoke => "撤销后复活：已撤销态再查询，单向机倒退",
            LcCode::RevokeTwice => "撤销二次：终态重复进入，v1 拒绝",
            LcCode::IdOutOfRange => "id 越界：超出 1..=96 闭集或撞保留字 0",
            LcCode::ConservationBroken => "守恒破坏：入账数≠存活+撤销+拒绝",
            LcCode::PhasePlaneMismatch => "段位分布破坏：位面计数与账本不符",
            LcCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [LcCode; 12] = [
        LcCode::GhostId,
        LcCode::DupCreate,
        LcCode::LedgerFull,
        LcCode::SkipToQuery,
        LcCode::SkipToRevoke,
        LcCode::RevokeBeforeQuery,
        LcCode::QueryAfterRevoke,
        LcCode::RevokeTwice,
        LcCode::IdOutOfRange,
        LcCode::ConservationBroken,
        LcCode::PhasePlaneMismatch,
        LcCode::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < LcCode::ALL.len() {
        if let Some(c) = LcCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F400 段闭集内"
}

// ── 生命周期状态机（四段单向） ─────────────────────────────────────
/// 四段生命周期（冻结 v1，只许单向推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LifePhase {
    /// 段 0：创建（登记，未生效）。
    Created,
    /// 段 1：生效（activate 后，覆写开始压过下级源）。
    Active,
    /// 段 2：已查询（首个 query 推入此段，后续 query 幂等停留）。
    Queried,
    /// 段 3：撤销（终态，只进不出）。
    Revoked,
}

impl LifePhase {
    /// 段序号（冻结映射，0..=3）。
    pub fn ord(self) -> u8 {
        match self {
            LifePhase::Created => 0,
            LifePhase::Active => 1,
            LifePhase::Queried => 2,
            LifePhase::Revoked => 3,
        }
    }
}

// ── 覆写记录（六维载荷打包在 18488 深化，本卡只留指纹槽） ──────────
/// 单条覆写记录：id + 持有者 + 段位 + 查询计数 + 指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OvrRec {
    /// 覆写 id（1..=96）。
    pub id: u32,
    /// 持有者进程句柄（绑定语义在 18482 深化，此处保留指纹位）。
    pub holder: u32,
    /// 当前段位。
    pub phase: LifePhase,
    /// 查询计数（幂等停留时递增，u16 足够：判据上限 96 条记录）。
    pub query_ticks: u16,
    /// 载荷指纹（六维压缩值，写死由 id×段数推导）。
    pub digest: u32,
}

impl OvrRec {
    /// 空哨兵（保留字 id=0 标记空槽）。
    pub const fn empty() -> OvrRec {
        OvrRec {
            id: 0,
            holder: 0,
            phase: LifePhase::Created,
            query_ticks: 0,
            digest: 0,
        }
    }
    /// 空槽判定。
    pub fn is_empty(&self) -> bool {
        self.id == 0
    }
}

// ── 位面：槽位"已达四段末"标记（u16 位图，第 i 位=槽 i） ───────────
/// 位面账：12 槽用 u16 恰好装下（2^12=4096 ≥ 12）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PhasePlane(pub u16);

impl PhasePlane {
    /// 空位面。
    pub const fn new() -> PhasePlane {
        PhasePlane(0)
    }
    /// 置位：槽 slot 进入终态时调用（slot < 12）。
    pub fn set(&mut self, slot: usize) {
        if slot < OVR_CAP {
            self.0 |= 1u16 << slot;
        }
    }
    /// 取位。
    pub fn get(self, slot: usize) -> bool {
        if slot < OVR_CAP {
            (self.0 >> slot) & 1u16 == 1u16
        } else {
            false
        }
    }
    /// 清位（撤销不清除终态标记——终态是账面事实）。
    pub fn count(self) -> u32 {
        let mut n = 0u32;
        let mut b = self.0;
        while b != 0 {
            if b & 1u16 == 1u16 {
                n = n.saturating_add(1);
            }
            b >>= 1;
        }
        n
    }
}

// ── 数组账本 + 状态机推进 ──────────────────────────────────────────
/// 覆写生命周期账本（定容 12 槽数组 + 位面 + 计数守恒）。
pub struct OvrLedger {
    /// 定容记录数组（空槽以 id=0 哨兵标记）。
    pub slots: [OvrRec; OVR_CAP],
    /// 终态位面。
    pub plane: PhasePlane,
    /// 已创建总数（含被拒后不入账的失败不计）。
    pub created_total: u32,
    /// 已撤销总数。
    pub revoked_total: u32,
    /// 被拒操作总数（错误路径入账）。
    pub rejected_total: u32,
    /// 拒因账（诊断码流水，只增不减）。
    pub reject_log: Vec<u32>,
}

impl OvrLedger {
    /// 新账本（12 空槽 + 空位面）。
    pub fn new() -> OvrLedger {
        OvrLedger {
            slots: [OvrRec::empty(); OVR_CAP],
            plane: PhasePlane::new(),
            created_total: 0,
            revoked_total: 0,
            rejected_total: 0,
            reject_log: Vec::new(),
        }
    }

    /// 线性扫 id 定槽（None=幽灵 id）。
    fn find_slot(&self, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < OVR_CAP {
            if let Some(r) = self.slots.get(i) {
                if !r.is_empty() && r.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 线性扫首个空槽（None=表满）。
    fn free_slot(&self) -> Option<usize> {
        let mut i = 0;
        while i < OVR_CAP {
            if let Some(r) = self.slots.get(i) {
                if r.is_empty() {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 段 0：创建。非法 id / 重复创建 / 表满三类显式拒绝。
    pub fn create(&mut self, id: u32, holder: u32) -> Result<(), u32> {
        if id == 0 || id > OVR_ID_MAX {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(LcCode::IdOutOfRange.code());
            return Err(LcCode::IdOutOfRange.code());
        }
        if self.find_slot(id).is_some() {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(LcCode::DupCreate.code());
            return Err(LcCode::DupCreate.code());
        }
        let slot = match self.free_slot() {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(LcCode::LedgerFull.code());
                return Err(LcCode::LedgerFull.code());
            }
        };
        // 指纹推导：id×段数+holder 低 4 位（写死公式，双跑可复算）。
        let digest = id.wrapping_mul(LIFE_SEGS as u32).wrapping_add(holder & 0xF);
        let rec = OvrRec {
            id,
            holder,
            phase: LifePhase::Created,
            query_ticks: 0,
            digest,
        };
        if let Some(r) = self.slots.get_mut(slot) {
            *r = rec;
        }
        self.created_total = self.created_total.saturating_add(1);
        Ok(())
    }

    /// 段 0→1：生效。仅 Created 态可推；越级查询/撤销在各自入口拦截。
    pub fn activate(&mut self, id: u32) -> Result<(), u32> {
        let slot = match self.find_slot(id) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(LcCode::GhostId.code());
                return Err(LcCode::GhostId.code());
            }
        };
        // 先取段位（块内取值，块外改账，避免 E0506 双借用）。
        let cur = match self.slots.get(slot) {
            Some(r) => r.phase,
            None => LifePhase::Revoked,
        };
        if cur != LifePhase::Created {
            // 已生效/已查询/已撤销均不可再 activate（幂等拒绝，非跳段）。
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(LcCode::DupCreate.code());
            return Err(LcCode::DupCreate.code());
        }
        if let Some(r) = self.slots.get_mut(slot) {
            r.phase = LifePhase::Active;
        }
        Ok(())
    }

    /// 段 1→2：查询。首个 query 推 Active→Queried，后续幂等停留并计数。
    pub fn query(&mut self, id: u32) -> Result<u32, u32> {
        let slot = match self.find_slot(id) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(LcCode::GhostId.code());
                return Err(LcCode::GhostId.code());
            }
        };
        let cur = match self.slots.get(slot) {
            Some(r) => r.phase,
            None => LifePhase::Revoked,
        };
        match cur {
            LifePhase::Created => {
                // 跳段：未生效即查询。
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(LcCode::SkipToQuery.code());
                Err(LcCode::SkipToQuery.code())
            }
            LifePhase::Active => {
                // 正常推进：Active→Queried，返回指纹。
                let d = match self.slots.get(slot) {
                    Some(r) => r.digest,
                    None => 0,
                };
                if let Some(r) = self.slots.get_mut(slot) {
                    r.phase = LifePhase::Queried;
                    r.query_ticks = r.query_ticks.saturating_add(1);
                }
                Ok(d)
            }
            LifePhase::Queried => {
                // 幂等停留：只计数不改段。
                let d = match self.slots.get(slot) {
                    Some(r) => r.digest,
                    None => 0,
                };
                if let Some(r) = self.slots.get_mut(slot) {
                    r.query_ticks = r.query_ticks.saturating_add(1);
                }
                Ok(d)
            }
            LifePhase::Revoked => {
                // 单向机倒退：撤销后查询必拒。
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(LcCode::QueryAfterRevoke.code());
                Err(LcCode::QueryAfterRevoke.code())
            }
        }
    }

    /// 段 2→3：撤销。仅 Queried 态可推（严格四段链，少一段即跳段）。
    pub fn revoke(&mut self, id: u32) -> Result<(), u32> {
        let slot = match self.find_slot(id) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(LcCode::GhostId.code());
                return Err(LcCode::GhostId.code());
            }
        };
        let cur = match self.slots.get(slot) {
            Some(r) => r.phase,
            None => LifePhase::Revoked,
        };
        let new_phase = match cur {
            LifePhase::Created => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(LcCode::SkipToRevoke.code());
                return Err(LcCode::SkipToRevoke.code());
            }
            LifePhase::Active => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(LcCode::RevokeBeforeQuery.code());
                return Err(LcCode::RevokeBeforeQuery.code());
            }
            LifePhase::Queried => LifePhase::Revoked,
            LifePhase::Revoked => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(LcCode::RevokeTwice.code());
                return Err(LcCode::RevokeTwice.code());
            }
        };
        if let Some(r) = self.slots.get_mut(slot) {
            r.phase = new_phase;
        }
        self.revoked_total = self.revoked_total.saturating_add(1);
        // 终态置位面（撤销是唯一入终态路径）。
        self.plane.set(slot);
        Ok(())
    }

    /// 只读查询：当前段位（不推状态机，审计用）。
    pub fn peek_phase(&self, id: u32) -> Option<LifePhase> {
        let slot = self.find_slot(id)?;
        match self.slots.get(slot) {
            Some(r) => Some(r.phase),
            None => None,
        }
    }

    /// 存活计数（Created/Active/Queried 三态之和）。
    pub fn alive_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < OVR_CAP {
            if let Some(r) = self.slots.get(i) {
                if !r.is_empty() && r.phase != LifePhase::Revoked {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 计数守恒：created = alive + revoked。破坏返回显式码。
    pub fn conserve(&self) -> Result<(), u32> {
        let alive = self.alive_count();
        if self.created_total != alive.saturating_add(self.revoked_total) {
            return Err(LcCode::ConservationBroken.code());
        }
        Ok(())
    }

    /// 位面对账：位面计数 = 已撤销数。破坏返回显式码。
    pub fn plane_audit(&self) -> Result<(), u32> {
        if self.plane.count() != self.revoked_total {
            return Err(LcCode::PhasePlaneMismatch.code());
        }
        Ok(())
    }
}

// ── 黄金全链：8 正例四段走通 → 守恒 → 位面对账 ─────────────────────
/// 黄金全链：8 条覆写走满四段，账目与位面全对账。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut led = OvrLedger::new();
    // 8 条正例：id 从 1 起（0 为保留字），holder 交错防耦合。
    let mut i = 0u32;
    while i < 8 {
        let id = 1 + i;
        let holder = 100 + i;
        led.create(id, holder)?;
        led.activate(id)?;
        let d1 = led.query(id)?;
        let d2 = led.query(id)?; // 幂等二查
        if d1 != d2 {
            return Err(LcCode::NotReproducible.code());
        }
        led.revoke(id)?;
        i = i.saturating_add(1);
    }
    led.conserve()?;
    led.plane_audit()?;
    if led.created_total != 8 || led.revoked_total != 8 {
        return Err(LcCode::ConservationBroken.code());
    }
    Ok((led.created_total, led.revoked_total))
}

// ── 注入检出账（注入=检出+漏检，漏检必 0） ─────────────────────────
/// 四槽注入账：tries/caught 对齐。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    /// 记一轮（slot < 4 才入账）。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    /// 全抓判定：每槽 tries>0 且 caught==tries。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 守恒：Σtries = Σcaught + Σmissed，missed 恒 0。
    pub fn missed_total(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < 4 {
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

/// 注入核：四类坏样本逐类 10/10 断言被显式拒绝。
///   槽 0：跳段（create→query / create→revoke / activate→revoke 三姿势）
///   槽 1：重复创建
///   槽 2：幽灵 id 查询
///   槽 3：撤销后复活（再查询）
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：跳段三姿势，同账本内逐个验。
        let mut l0 = OvrLedger::new();
        l0.create(1, 10)?;
        let hit0a = match l0.query(1) {
            Err(c) if c == LcCode::SkipToQuery.code() => true,
            _ => false,
        };
        let mut l1 = OvrLedger::new();
        l1.create(2, 10)?;
        let hit0b = match l1.revoke(2) {
            Err(c) if c == LcCode::SkipToRevoke.code() => true,
            _ => false,
        };
        let mut l2 = OvrLedger::new();
        l2.create(3, 10)?;
        let _ = l2.activate(3);
        let hit0c = match l2.revoke(3) {
            Err(c) if c == LcCode::RevokeBeforeQuery.code() => true,
            _ => false,
        };
        led.record(0, hit0a && hit0b && hit0c);
        // 槽 1：重复创建。
        let mut l3 = OvrLedger::new();
        let _ = l3.create(4, 10);
        let hit1 = match l3.create(4, 11) {
            Err(c) if c == LcCode::DupCreate.code() => true,
            _ => false,
        };
        led.record(1, hit1);
        // 槽 2：幽灵 id 查询。
        let mut l4 = OvrLedger::new();
        let hit2 = match l4.query(77) {
            Err(c) if c == LcCode::GhostId.code() => true,
            _ => false,
        };
        led.record(2, hit2);
        // 槽 3：撤销后复活（撤销成功 → 再查询必拒）。
        let mut l5 = OvrLedger::new();
        let _ = l5.create(5, 10);
        let _ = l5.activate(5);
        let _ = l5.query(5);
        let _ = l5.revoke(5);
        let hit3 = match l5.query(5) {
            Err(c) if c == LcCode::QueryAfterRevoke.code() => true,
            _ => false,
        };
        led.record(3, hit3);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────────
/// 探针 A：正例四段链全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = OvrLedger::new();
    led.create(9, 900)?;
    led.activate(9)?;
    let _ = led.query(9)?;
    led.revoke(9)?;
    Ok(())
}

/// 探针 B：越级查询必拒（跳段）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = OvrLedger::new();
    led.create(8, 800)?;
    led.query(8).map(|_| ())
}

/// 探针 C：幽灵 id 必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = OvrLedger::new();
    led.activate(666)
}

// ── 双跑确定性对拍 ─────────────────────────────────────────────────
/// 双跑：同一黄金链两次逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LcCode::NotReproducible.code());
    }
    Ok(())
}

// ── 段位守卫 ───────────────────────────────────────────────────────
/// 段位守卫：SEG_BASE 对齐且全部诊断码落段内 0x40 窗口。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < LcCode::ALL.len() {
        if let Some(c) = LcCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 容量耗尽探测：连建 13 条（12 槽+1），第 13 条必返表满且前 12 条账不丢。
pub fn capacity_probe() -> Result<(u32, u32), u32> {
    let mut led = OvrLedger::new();
    let mut filled = 0u32;
    let mut i = 1u32;
    while i <= 13u32 {
        match led.create(i, i) {
            Ok(()) => filled = filled.saturating_add(1),
            Err(c) if c == LcCode::LedgerFull.code() => {}
            Err(_) => return Err(LcCode::ConservationBroken.code()),
        }
        i = i.saturating_add(1);
    }
    if filled != OVR_CAP as u32 {
        return Err(LcCode::ConservationBroken.code());
    }
    led.conserve()?;
    Ok((filled, led.rejected_total))
}

/// id 域边界探测：0 与 97 必拒（0 保留 / 97 越界），1 与 96 必收。
pub fn id_domain_probe() -> Result<(), u32> {
    let mut led = OvrLedger::new();
    match led.create(0, 1) {
        Err(c) if c == LcCode::IdOutOfRange.code() => {}
        _ => return Err(LcCode::IdOutOfRange.code()),
    }
    match led.create(97, 1) {
        Err(c) if c == LcCode::IdOutOfRange.code() => {}
        _ => return Err(LcCode::IdOutOfRange.code()),
    }
    led.create(1, 1)?;
    led.create(OVR_ID_MAX, 1)?;
    Ok(())
}

// ── 判据收口 ───────────────────────────────────────────────────────
/// 判据产出（14 条，断言性质命名），供 run_unx18481_checks 与测试共用。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1-1 黄金全链（正常流）。
    let g = golden_run();
    v.push((
        "J1-黄金全链-四段走通",
        g.is_ok(),
        format!("{:?}", g),
    ));
    // J1-2 跳段注入 10/10（四槽全抓）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-跳段注入10/10",
        inj_ok,
        format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // J1-3 注入守恒（漏检=0）。
    let miss = match &inj {
        Ok(l) => l.missed_total(),
        Err(_) => 1,
    };
    v.push((
        "J1-注入守恒漏检零",
        miss == 0,
        format!("missed={}", miss),
    ));
    // J2-4 计数守恒。
    let mut led = OvrLedger::new();
    let _ = led.create(3, 3);
    let conserve_ok = led.conserve().is_ok();
    v.push((
        "J2-计数守恒-创建即守恒",
        conserve_ok,
        String::from("created=alive+revoked"),
    ));
    // J2-5 位面对账。
    let mut led2 = OvrLedger::new();
    let _ = led2.create(2, 2);
    let _ = led2.activate(2);
    let _ = led2.query(2);
    let _ = led2.revoke(2);
    let plane_ok = led2.plane_audit().is_ok();
    v.push((
        "J2-位面对账-终态计数一致",
        plane_ok,
        String::from("plane.count=revoked"),
    ));
    // J3-6 幂等查询（双查同值）。
    let mut led3 = OvrLedger::new();
    let _ = led3.create(5, 5);
    let _ = led3.activate(5);
    let q1 = led3.query(5);
    let q2 = led3.query(5);
    let idem = q1 == q2;
    v.push((
        "J3-查询幂等-双查同值",
        idem,
        format!("{:?}/{:?}", q1, q2),
    ));
    // J3-7 单调：撤销后账本撤销数只增（再撤销被拒不改账）。
    let rv_before = led2.revoked_total;
    let _ = led2.revoke(2); // 应被 RevokeTwice 拒
    let mono = led2.revoked_total == rv_before;
    v.push((
        "J3-撤销单调-终态不可再入",
        mono,
        format!("before={} after={}", rv_before, led2.revoked_total),
    ));
    // J4-8 容量耗尽。
    let cap = capacity_probe();
    v.push((
        "J4-容量耗尽-12满13拒",
        cap.is_ok(),
        format!("{:?}", cap),
    ));
    // J4-9 id 域边界（0/97 拒，1/96 收）。
    v.push((
        "J4-id域边界",
        id_domain_probe().is_ok(),
        String::from("0与97拒 1与96收"),
    ));
    // J4-10 探针三序。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_err();
    let pc = probe_seq_c().is_err();
    v.push((
        "J4-探针三序",
        pa && pb && pc,
        format!("a={} b!={} c!={}", pa, pb, pc),
    ));
    // J5-11 双跑一致。
    v.push((
        "J5-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // J5-12 段位守卫。
    v.push((
        "J5-段位守卫",
        segment_guard(),
        String::from("0x7F400 窗口闭集"),
    ));
    // J5-13 文案闭集：逐码非空。
    let mut det_ok = true;
    let mut k = 0;
    while k < LcCode::ALL.len() {
        if let Some(c) = LcCode::ALL.get(k) {
            if detail_of(c.code()) != c.detail() || c.detail().len() == 0 {
                det_ok = false;
            }
        }
        k += 1;
    }
    v.push((
        "J5-诊断文案闭集非空",
        det_ok,
        format!("codes={}", LcCode::ALL.len()),
    ));
    // J5-14 收口：判据条数冻结 14。
    v.push((
        "J5-判据收口",
        v.len() + 1 == 14,
        format!("n={}", v.len() + 1),
    ));
    v
}

/// 探针入口：F18481 ActCtx 覆写生命周期总纲自检（14 条）。
pub fn run_unx18481_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18481 actctx_lifecycle");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i = i.saturating_add(1);
    }
    s
}

// ── 测试（8 个：正常流/四段链/注入/容量/双跑/守恒/段位/收口） ──────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_golden_chain() {
        // 正常流：黄金全链 8 例四段走通且账目守恒。
        assert!(golden_run().is_ok(), "黄金全链应全绿");
    }

    #[test]
    fn full_lifecycle_chain_order() {
        // 判定次序：四段必须严格链式，任何越级都被显式拒绝。
        let mut led = OvrLedger::new();
        assert!(led.create(1, 1).is_ok());
        // Created 态撤销 = 跳段。
        let e1 = led.revoke(1);
        assert!(e1.err() == Some(LcCode::SkipToRevoke.code()), "create→revoke 跳段必拒");
        // Created 态查询 = 跳段。
        let e2 = led.query(1);
        assert!(e2.err() == Some(LcCode::SkipToQuery.code()), "create→query 跳段必拒");
        assert!(led.activate(1).is_ok());
        // Active 态撤销 = 越过查询段。
        let e3 = led.revoke(1);
        assert!(e3.err() == Some(LcCode::RevokeBeforeQuery.code()), "activate→revoke 跳段必拒");
        assert!(led.query(1).is_ok());
        assert!(led.revoke(1).is_ok());
        // 终态后一切操作被拒。
        assert!(led.query(1).err() == Some(LcCode::QueryAfterRevoke.code()));
        assert!(led.revoke(1).err() == Some(LcCode::RevokeTwice.code()));
    }

    #[test]
    fn injection_all_classes_caught() {
        // 注入必抓：四类坏样本 10/10 全抓。
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.is_ok(), "注入核不应报错");
        if let Ok(l) = led {
            assert!(l.all_caught(), "四槽必须全抓");
            assert!(l.missed_total() == 0, "漏检必须为 0");
            assert!(l.tries[0] == JUDGE_ROUNDS, "跳段槽应满轮");
        }
    }

    #[test]
    fn dup_and_ghost_rejected() {
        // 注入：重复创建与幽灵 id。
        let mut led = OvrLedger::new();
        assert!(led.create(7, 7).is_ok());
        assert!(led.create(7, 8).err() == Some(LcCode::DupCreate.code()));
        assert!(led.query(99).err() == Some(LcCode::GhostId.code()));
        assert!(led.revoke(99).err() == Some(LcCode::GhostId.code()));
    }

    #[test]
    fn capacity_exhaustion_boundary() {
        // 容量边界：12 满、13 拒，账不丢。
        let cap = capacity_probe();
        assert!(cap.is_ok(), "容量探测应全绿");
        if let Ok((filled, rejected)) = cap {
            assert!(filled == OVR_CAP as u32);
            assert!(rejected >= 1, "第 13 条应入拒因账");
        }
    }

    #[test]
    fn id_domain_four_bounds() {
        // 四类边界：0（下界外）/1（下界）/96（上界）/97（上界外）。
        assert!(id_domain_probe().is_ok());
    }

    #[test]
    fn double_run_deterministic() {
        // 双跑一致：同输入两次逐字段相等。
        assert!(repro_check().is_ok(), "双跑应逐字段一致");
    }

    #[test]
    fn segment_guard_and_checkset() {
        // 段位守卫 + 判据收口全绿。
        assert!(segment_guard(), "段位守卫应过");
        let set = run_unx18481_checks();
        assert!(set.all_passed(), "14 条判据应全绿");
        assert!(set.len() == 14, "判据条数应冻结 14");
    }
}
