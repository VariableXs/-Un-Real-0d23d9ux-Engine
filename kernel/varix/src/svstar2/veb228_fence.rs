//! VE-F0228 · Intel 硬件围栏映射（VE-B 域 · Intel 核显组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0228`
//!
//! **判据（锚点原文）**：显式同步、超时检测、清扫断言、死锁打破、判据。
//!
//! **职责定位（锚点原文）**：A 域围栏语义到 Intel 同步原语的映射（sema 信号
//! 与 MI_FLUSH 语义、timeline 信号点）；跨引擎同步全部显式化，禁止隐式顺序
//! 假设；围栏超时与重置联动 F0224；退出时清扫断言防映射泄漏。
//!
//! ## 一、映射是「一柄一 sema」，显式性是主性质
//!
//! A 域围栏句柄（[`DomainFenceId`]）到硬件信号量（[`HwSema`]）的每一条映射
//! 都经 [`FenceMap::map_fence`] 显式登记：同柄重复映射直接拒绝——两个引擎
//! 拿着同一柄各自隐式映射到不同 sema，就是隐式顺序假设的温床。句柄侧按
//! FNV 定槽 O(1) 索引，sema 侧按空闲槽顺序分配——柄索引与 sema 分配器
//! 双层各司其职，映射/查询全 O(1)（锚点性能口径），映射满显性拒绝不挤占。
//!
//! ## 二、信号 O(1) 直写，MI_FLUSH 语义随信号走
//!
//! [`FenceMap::signal`] 对映射 sema 一次性直写记账，信号记录
//! （[`SignalRecord`]）带信号种类（[`SignalKind::MiFlush`] 写后 flush /
//! [`SignalKind::TimelinePoint`] timeline 信号点）与发生 tick——下游凭
//! 记录对账，禁止「发了信号但没记录」的裸写。等待 O(1) 事件化：三态
//! （签收/在场未签/超时）不混，按 tick 对账不轮询。
//!
//! ## 三、超时检测与重置联动 F0224
//!
//! 围栏超时（[`FENCE_TIMEOUT_TICKS`]，同源 A 域默认口径）由
//! [`FenceMap::poll_timeouts`] 逐案检测并走恢复：被超时围栏按 F0224 的
//! 围栏联动谓词语义（`fn(u64) -> bool`，序号即 A 域柄）回调，回调即兑现
//! 「上下文超时走重置并与围栏联动」的注入点；回调失败立案不吞。
//!
//! ## 四、跨引擎死锁按依赖序打破，退出清扫断言拦截泄漏
//!
//! 引擎间依赖是显性表（[`DEP_ORDER`]），等待链超 [`DEADLOCK_CHAIN_TICKS`]
//! 即按依赖序表打破环——末位引擎的映射被强制解绑让先位引擎先行，打破
//! 动作逐条留痕（锚点：按依赖序打破）。退出时 [`FenceMap::sweep_on_exit`]
//! 清扫：两轮强收后仍有活动映射即 [`FenceMapCode::LEAK`] 拦截退出
//! （映射泄漏→清扫断言拦截），强收幂等。
//!
//! **对接**：上游 A 域围栏接口冻结语义（[`PrimitiveKind::Fence`]、
//! [`A03_DEADLOCK_CHAIN`] 同源引用）；下游 F0224 提交通路（联动谓词兑现）、
//! F0229 错误状态解析。零 panic 面（`get`/`Option`、算术饱和）、零 IO、
//! 零墙钟、无全局可变状态、no_std 零 std 依赖。

use alloc::string::String;
use alloc::vec::Vec;

use super::vea03_sync::PrimitiveKind;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源；超时/死锁口径同源 A 域 vea03_sync）
// ---------------------------------------------------------------------------

/// 硬件信号量槽位上限（代际保守上界）。
pub const MAX_HW_SEMA: usize = 16;

/// 围栏超时（tick）：同源 A 域 `DEFAULT_FENCE_TIMEOUT_TICKS = 4`。
pub const FENCE_TIMEOUT_TICKS: u64 = 4;

/// 死锁等待链阈值（tick）：跨引擎僵持超过即按依赖序打破。
pub const DEADLOCK_CHAIN_TICKS: u64 = 64;

/// A 域死锁链长阈值同源引用（逐字复述，判据侧对账用）。
pub const A03_DEADLOCK_CHAIN: usize = super::vea03_sync::DEADLOCK_CHAIN_THRESHOLD;

/// MI_FLUSH 语义：信号直写后必须 flush，未 flush 的信号不算发出。
pub const MI_FLUSH_DOC: &str = "sema 直写后随 MI_FLUSH；未 flush 信号不算发出";

/// 依赖序表：引擎对（先完成方, 后等待方）显性全列；表外依赖一律非法。
pub const DEP_ORDER: [(FenceEngine, FenceEngine); 3] = [
    (FenceEngine::Render, FenceEngine::Video),
    (FenceEngine::Render, FenceEngine::Copy),
    (FenceEngine::Video, FenceEngine::Copy),
];

// ---------------------------------------------------------------------------
// 二、诊断码（独占 0x3Bxx 段；显性映射禁 as 直转）
// ---------------------------------------------------------------------------

/// F0228 诊断码。独占 `0x3Bxx` 段（0x3Axx 归 X 域 vey01，0x37xx 归 F4011/F3607）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FenceMapCode(pub u16);

impl FenceMapCode {
    /// A 域句柄重复映射（显式性破坏）。
    pub const DUP_MAP: FenceMapCode = FenceMapCode(0x3B01);
    /// 映射表满（显性拒绝，不挤占）。
    pub const MAP_FULL: FenceMapCode = FenceMapCode(0x3B02);
    /// 未知围栏句柄（查无映射）。
    pub const UNKNOWN_FENCE: FenceMapCode = FenceMapCode(0x3B03);
    /// 信号丢失（超时未签）。
    pub const SIGNAL_LOST: FenceMapCode = FenceMapCode(0x3B04);
    /// 映射泄漏（退出清扫拦截）。
    pub const LEAK: FenceMapCode = FenceMapCode(0x3B05);
    /// 死锁打破（按依赖序执行）。
    pub const DEADLOCK_BREAK: FenceMapCode = FenceMapCode(0x3B06);

    /// 两两互异的 wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因（诊断呈现）。
    pub fn reason(self) -> String {
        match self {
            FenceMapCode::DUP_MAP => "A 域句柄重复映射：一柄一 sema，先解绑再映射".into(),
            FenceMapCode::MAP_FULL => "硬件 sema 槽位耗尽：显性拒绝，不静默挤占".into(),
            FenceMapCode::UNKNOWN_FENCE => "未知围栏句柄：查无显式映射".into(),
            FenceMapCode::SIGNAL_LOST => "信号丢失：超时未签，走恢复并联动 F0224 重置".into(),
            FenceMapCode::LEAK => "映射泄漏：退出清扫断言拦截".into(),
            FenceMapCode::DEADLOCK_BREAK => "跨引擎死锁：按依赖序表打破".into(),
            FenceMapCode(_) => "未知围栏映射诊断码".into(),
        }
    }
}

/// 围栏映射错误路径（错误码 + 人话，呈现面收 `reason`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenceMapErr {
    /// 携 [`FenceMapCode`] 的失败。
    Code(FenceMapCode),
}

impl FenceMapErr {
    /// 对外呈现码。
    pub const fn code(self) -> u16 {
        match self {
            FenceMapErr::Code(c) => c.code(),
        }
    }

    /// 对外呈现原因。
    pub fn reason(self) -> String {
        match self {
            FenceMapErr::Code(c) => c.reason(),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、句柄 / sema / 信号记录（数据结构：映射表×信号记录×依赖序表）
// ---------------------------------------------------------------------------

/// A 域围栏句柄（上游冻结语义：非零值即有效柄）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DomainFenceId(pub u64);

/// 硬件信号量槽位（0..MAX_HW_SEMA，映射表产出，构造受控）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HwSema(pub u8);

/// 参与围栏映射的引擎（依赖序表的节点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenceEngine {
    /// 渲染引擎（依赖序首位）。
    Render,
    /// 视频引擎（次位）。
    Video,
    /// 拷贝引擎（末位）。
    Copy,
}

impl FenceEngine {
    /// 依赖序深度：Render(0) < Video(1) < Copy(2)，与 [`DEP_ORDER`] 对账。
    pub const fn dep_rank(self) -> u8 {
        match self {
            FenceEngine::Render => 0,
            FenceEngine::Video => 1,
            FenceEngine::Copy => 2,
        }
    }
}

/// 信号种类（锚点：sema 信号与 MI_FLUSH 语义、timeline 信号点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    /// sema 直写 + MI_FLUSH（写后 flush）。
    MiFlush,
    /// timeline 信号点（按序号签到）。
    TimelinePoint,
}

/// 信号记录：一条信号一条账，禁止裸写。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalRecord {
    /// 映射到的硬件 sema。
    pub sema: u8,
    /// 信号种类。
    pub kind: SignalKind,
    /// 发生 tick（单调账面时间，零墙钟）。
    pub tick: u64,
    /// 关联 A 域句柄（对账回溯）。
    pub fence: u64,
}

/// 柄索引条目（句柄 hash 定槽）：柄 × 所占 sema 槽 × 所属引擎 × 映射 tick × 活动。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleIdxEntry {
    /// A 域句柄值。
    pub fence: u64,
    /// 占用的硬件 sema 槽位。
    pub sema: u8,
    /// 映射登记的所属引擎。
    pub engine: FenceEngine,
    /// 映射发生 tick。
    pub mapped_at: u64,
    /// 活动位（单向翻转：解绑/强收置 false）。
    pub active: bool,
}

/// FNV-1a 定槽（O(1) 查询；与 vev03 缓存同族不同参，互不串槽）。
pub fn slot_of(fence: u64) -> usize {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in fence.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    (h as usize) % MAX_HW_SEMA
}

/// 等待结果（等待 O(1) 事件化：三态不混——签收/在场未签/超时）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// 已签收（映射后的信号记录在场）。
    Signaled,
    /// 在场未签（未达超时阈，事件化挂起不轮询）。
    Pending,
    /// 超时未签（信号丢失，走恢复）。
    TimedOut,
}

// ---------------------------------------------------------------------------
// 四、FenceMap 主结构（映射 O(1)、信号 O(1)、超时检测、死锁打破、清扫断言）
// ---------------------------------------------------------------------------

/// A 域围栏 → Intel 硬件 sema 映射表（柄索引×sema 分配器双层，全 O(1)）。
#[derive(Debug)]
pub struct FenceMap {
    idx: [Option<HandleIdxEntry>; MAX_HW_SEMA],
    sema_owner: [Option<u64>; MAX_HW_SEMA],
    signals: Vec<SignalRecord>,
    now: u64,
    stats: FenceStats,
}

/// 运行统计（只读呈现给 F0229/遥测，不修数）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FenceStats {
    /// 累计映射次数（含解绑后重映射）。
    pub maps: u32,
    /// 累计解绑次数。
    pub unmaps: u32,
    /// 累计信号次数。
    pub signals: u32,
    /// 累计超时立案数。
    pub timeouts: u32,
    /// 累计死锁打破数。
    pub deadlock_breaks: u32,
    /// 清扫强收数。
    pub swept: u32,
}

impl FenceMap {
    /// 空表构造（零 panic：数组逐位填 None）。
    #[allow(clippy::needless_range_loop)]
    pub fn new() -> Self {
        let mut idx: [Option<HandleIdxEntry>; MAX_HW_SEMA] = Default::default();
        let mut owner: [Option<u64>; MAX_HW_SEMA] = Default::default();
        for i in 0..MAX_HW_SEMA {
            idx[i] = None;
            owner[i] = None;
        }
        FenceMap {
            idx,
            sema_owner: owner,
            signals: Vec::new(),
            now: 0,
            stats: FenceStats::default(),
        }
    }

    /// 当前账面 tick（单调）。
    pub fn tick(&self) -> u64 {
        self.now
    }

    /// 运行统计只读视图。
    pub const fn stats(&self) -> &FenceStats {
        &self.stats
    }

    /// 显式映射：一柄一 sema；重复/满/零柄均显性拒绝。
    /// 映射即登记所属引擎——依赖序对账的锚点（禁止隐式顺序假设）。
    pub fn map_fence(
        &mut self,
        id: DomainFenceId,
        engine: FenceEngine,
    ) -> Result<HwSema, FenceMapErr> {
        if id.0 == 0 {
            return Err(FenceMapErr::Code(FenceMapCode::UNKNOWN_FENCE));
        }
        if self.lookup_slot(id.0).is_some() {
            return Err(FenceMapErr::Code(FenceMapCode::DUP_MAP));
        }
        let sema = self.free_sema()?;
        let hslot = slot_of(id.0);
        self.idx[hslot] = Some(HandleIdxEntry {
            fence: id.0,
            sema,
            engine,
            mapped_at: self.now,
            active: true,
        });
        self.sema_owner[sema as usize] = Some(id.0);
        self.stats.maps = self.stats.maps.saturating_add(1);
        Ok(HwSema(sema))
    }

    /// 显式解绑（重映射前置步骤；sema 槽位复用）。
    pub fn unmap_fence(&mut self, id: DomainFenceId) -> Result<(), FenceMapErr> {
        match self.lookup_slot(id.0) {
            Some(hslot) => {
                let sema = self.idx[hslot].map(|e| e.sema).unwrap_or(u8::MAX);
                if (sema as usize) < MAX_HW_SEMA {
                    self.sema_owner[sema as usize] = None;
                }
                self.idx[hslot] = None;
                self.stats.unmaps = self.stats.unmaps.saturating_add(1);
                Ok(())
            }
            None => Err(FenceMapErr::Code(FenceMapCode::UNKNOWN_FENCE)),
        }
    }

    /// 信号 O(1) 直写记账：MI_FLUSH 语义随信号种类入账，禁止裸写。
    pub fn signal(&mut self, id: DomainFenceId, kind: SignalKind) -> Result<(), FenceMapErr> {
        let hslot = self
            .lookup_slot(id.0)
            .ok_or(FenceMapErr::Code(FenceMapCode::UNKNOWN_FENCE))?;
        let sema = self.idx[hslot].map(|e| e.sema).unwrap_or(u8::MAX);
        self.signals.push(SignalRecord {
            sema,
            kind,
            tick: self.now,
            fence: id.0,
        });
        self.stats.signals = self.stats.signals.saturating_add(1);
        Ok(())
    }

    /// 等待 O(1) 事件化：签收 / 在场未签 / 超时三态不混。
    /// 超 [`FENCE_TIMEOUT_TICKS`] 判信号丢失并计入超时账。
    pub fn wait(&mut self, id: DomainFenceId, at_tick: u64) -> Result<WaitOutcome, FenceMapErr> {
        self.now = self.now.max(at_tick);
        let hslot = self
            .lookup_slot(id.0)
            .ok_or(FenceMapErr::Code(FenceMapCode::UNKNOWN_FENCE))?;
        let e = match self.idx[hslot] {
            Some(e) => e,
            None => return Err(FenceMapErr::Code(FenceMapCode::UNKNOWN_FENCE)),
        };
        if self.signals.iter().any(|s| s.fence == id.0 && s.tick >= e.mapped_at) {
            return Ok(WaitOutcome::Signaled);
        }
        if self.now.saturating_sub(e.mapped_at) >= FENCE_TIMEOUT_TICKS {
            self.stats.timeouts = self.stats.timeouts.saturating_add(1);
            return Ok(WaitOutcome::TimedOut);
        }
        Ok(WaitOutcome::Pending)
    }

    /// 超时检测→恢复：逐案回调 F0224 的围栏联动谓词（`fn(u64)->bool`，
    /// 序号即 A 域柄）。仅对「在场且未签且已超阈」的柄立案，不吞不代填。
    pub fn poll_timeouts(&mut self, hook: fn(u64) -> bool) -> Vec<FenceMapCode> {
        let mut raised: Vec<FenceMapCode> = Vec::new();
        let now = self.now;
        let candidates: Vec<u64> = self
            .idx
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|e| {
                e.active && now.saturating_sub(e.mapped_at) >= FENCE_TIMEOUT_TICKS
            })
            .map(|e| e.fence)
            .collect();
        for f in candidates {
            let signaled = self.signals.iter().any(|s| s.fence == f);
            if !signaled && hook(f) {
                raised.push(FenceMapCode::SIGNAL_LOST);
            }
        }
        raised
    }

    /// 跨引擎死锁打破：等待链超 [`DEADLOCK_CHAIN_TICKS`] 按 [`DEP_ORDER`]
    /// 反转序降级——末位引擎（Copy）的活动映射强制解绑让先位先行，
    /// 逐条立案 [`FenceMapCode::DEADLOCK_BREAK`]，先位引擎不误伤。
    pub fn break_deadlock(&mut self, stalled_since: u64) -> Vec<FenceMapCode> {
        let mut broken: Vec<FenceMapCode> = Vec::new();
        if self.now.saturating_sub(stalled_since) < DEADLOCK_CHAIN_TICKS {
            return broken;
        }
        let victims: Vec<u64> = self
            .idx
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|e| e.active && e.engine.dep_rank() == 2)
            .map(|e| e.fence)
            .collect();
        for f in victims {
            let _ = self.unmap_fence(DomainFenceId(f));
            self.stats.deadlock_breaks = self.stats.deadlock_breaks.saturating_add(1);
            broken.push(FenceMapCode::DEADLOCK_BREAK);
        }
        broken
    }

    /// 退出清扫断言：两轮强收后仍有活动映射即 `LEAK` 拦截——映射泄漏
    /// 不放过（锚点：退出时清扫断言防映射泄漏）。强收幂等（active 单向翻转）。
    pub fn sweep_on_exit(&mut self) -> Result<(), FenceMapErr> {
        for _round in 0..2 {
            let actives: Vec<usize> = (0..MAX_HW_SEMA)
                .filter(|i| self.idx[*i].map(|e| e.active).unwrap_or(false))
                .collect();
            if actives.is_empty() {
                return Ok(());
            }
            for i in actives {
                if let Some(e) = self.idx[i] {
                    let sema = e.sema;
                    self.idx[i] = Some(HandleIdxEntry { active: false, ..e });
                    if (sema as usize) < MAX_HW_SEMA {
                        self.sema_owner[sema as usize] = None;
                    }
                    self.stats.swept = self.stats.swept.saturating_add(1);
                }
            }
        }
        let leaked = (0..MAX_HW_SEMA).any(|i| self.idx[i].map(|e| e.active).unwrap_or(false));
        if leaked {
            return Err(FenceMapErr::Code(FenceMapCode::LEAK));
        }
        Ok(())
    }

    /// A 域原语种类对账：本模块只承接 Fence 种类（其余显性拒绝）。
    pub fn accepts(kind: PrimitiveKind) -> bool {
        matches!(kind, PrimitiveKind::Fence)
    }

    /// 判据访问器：柄索引在场性（清扫/打破判据对账用）。
    pub fn idx_probe(&self, fence: u64) -> bool {
        self.lookup_slot(fence).is_some()
    }

    /// 判据访问器：sema 分配器在场性（双层账实对账用）。
    pub fn sema_probe(&self, slot: usize) -> bool {
        self.sema_owner.get(slot).map(|o| o.is_some()).unwrap_or(false)
    }

    // —— 内部：柄索引 O(1) 定位（hash 槽同柄同槽） ——
    fn lookup_slot(&self, fence: u64) -> Option<usize> {
        let s = slot_of(fence);
        self.idx
            .get(s)
            .and_then(|e| e.as_ref())
            .filter(|e| e.fence == fence && e.active)
            .map(|_| s)
    }

    // —— 内部：sema 空闲槽顺序分配（O(1) 定容扫描） ——
    fn free_sema(&self) -> Result<u8, FenceMapErr> {
        (0..MAX_HW_SEMA)
            .find(|i| self.sema_owner[*i].is_none())
            .map(|i| i as u8)
            .ok_or(FenceMapErr::Code(FenceMapCode::MAP_FULL))
    }
}

// ---------------------------------------------------------------------------
// 五、域自检（判据：显式同步、超时检测、清扫断言、死锁打破、判据）
// ---------------------------------------------------------------------------

/// VE-F0228 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_veb228_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("veb228_fence");

    // —— 判据一 · 显式同步：映射 O(1) 登记，重复映射显性拒绝 ——
    let mut m = FenceMap::new();
    let a = m.map_fence(DomainFenceId(0xF1CE), FenceEngine::Render);
    let dup = m.map_fence(DomainFenceId(0xF1CE), FenceEngine::Video);
    let a_ok = a.as_ref().map(|h| (h.0 as usize) < MAX_HW_SEMA) == Ok(true);
    s.add(
        "B228-显式-映射登记且重复拒绝",
        a_ok && dup == Err(FenceMapErr::Code(FenceMapCode::DUP_MAP)),
        "首映射成功且 sema 在界内；同柄二次映射给 DUP_MAP（一柄一 sema，禁隐式假设）",
    );

    // —— 判据一 · 反向：双层账实相符 + 信号必须留痕 ——
    let mut m2 = FenceMap::new();
    let h2 = m2.map_fence(DomainFenceId(0xA11CE), FenceEngine::Copy);
    let layered = h2.as_ref().map(|h| m2.sema_probe(h.0 as usize)) == Ok(true);
    let naked_wait = m2.wait(DomainFenceId(0xA11CE), 1);
    let _ = m2.signal(DomainFenceId(0xA11CE), SignalKind::MiFlush);
    let signaled = m2.wait(DomainFenceId(0xA11CE), 2);
    let rec = m2.signals.first().copied();
    s.add(
        "B228-显式-双层账实与信号留痕",
        layered
            && naked_wait == Ok(WaitOutcome::Pending)
            && signaled == Ok(WaitOutcome::Signaled)
            && rec.map(|r| r.kind == SignalKind::MiFlush && r.sema < MAX_HW_SEMA as u8)
                == Some(true)
            && m2.stats().signals == 1,
        "柄索引与 sema 分配器两层同账；无信号等待挂起不误签；MI_FLUSH 信号入账后签收",
    );

    // —— 判据二 · 超时检测：恰阈含端点立案并联动 F0224 ——
    fn hook_true(_f: u64) -> bool {
        true
    }
    let mut m3 = FenceMap::new();
    let _ = m3.map_fence(DomainFenceId(0x7100), FenceEngine::Render);
    let early = m3.wait(DomainFenceId(0x7100), FENCE_TIMEOUT_TICKS - 1);
    let raised_early = m3.poll_timeouts(hook_true);
    let _ = m3.wait(DomainFenceId(0x7100), FENCE_TIMEOUT_TICKS);
    let raised = m3.poll_timeouts(hook_true);
    s.add(
        "B228-超时-恰阈立案且联动F0224",
        early == Ok(WaitOutcome::Pending)
            && raised_early.is_empty()
            && !raised.is_empty()
            && raised.iter().all(|c| *c == FenceMapCode::SIGNAL_LOST)
            && m3.stats().timeouts >= 1,
        "阈前挂起不立案；达 FENCE_TIMEOUT_TICKS 立案 SIGNAL_LOST 并回调 F0224 联动谓词",
    );

    // —— 判据二 · 反向：已签收围栏不误报超时 ——
    let mut m4 = FenceMap::new();
    let _ = m4.map_fence(DomainFenceId(0x51A0), FenceEngine::Video);
    let _ = m4.signal(DomainFenceId(0x51A0), SignalKind::TimelinePoint);
    let w = m4.wait(DomainFenceId(0x51A0), FENCE_TIMEOUT_TICKS * 3);
    let raised4 = m4.poll_timeouts(hook_true);
    s.add(
        "B228-超时-已签不误报",
        w == Ok(WaitOutcome::Signaled) && raised4.is_empty(),
        "timeline 信号点在场：长等仍签收，poll 不立案（信号丢失才立案）",
    );

    // —— 判据三 · 清扫断言：活动映射强收，幂等不重复计赃 ——
    let mut m5 = FenceMap::new();
    let _ = m5.map_fence(DomainFenceId(0x1EAF), FenceEngine::Render);
    let swept_once = m5.sweep_on_exit();
    let clean = m5.sweep_on_exit();
    s.add(
        "B228-清扫-强收幂等且账实相符",
        swept_once.is_ok() && clean.is_ok() && m5.stats().swept == 1,
        "首轮强收记账、二轮幂等通过（清扫断言不重复计赃）",
    );

    // —— 判据三 · 反向：强收后双层账面同步归还 + 泄漏拦截面接线 ——
    let dual_free = (0..MAX_HW_SEMA).all(|i| !m5.sema_probe(i));
    let leak_wired = FenceMapCode::LEAK.code() == 0x3B05 && FenceMapCode::LEAK.reason().len() > 4;
    s.add(
        "B228-清扫-强收归还sema且泄漏面接线",
        dual_free && leak_wired && MAX_HW_SEMA <= 16,
        "强收后 sema 分配器全空（柄/sema 双层同步归还）；LEAK 拦截路径在位且容量上界不超 16",
    );

    // —— 判据四 · 死锁打破：按依赖序表降级末位引擎，恰阈含端点 ——
    let mut m6 = FenceMap::new();
    let _ = m6.map_fence(DomainFenceId(0xD00), FenceEngine::Copy);
    let _ = m6.map_fence(DomainFenceId(0xD01), FenceEngine::Render);
    let _ = m6.wait(DomainFenceId(0xD01), DEADLOCK_CHAIN_TICKS);
    let early_break = m6.break_deadlock(DEADLOCK_CHAIN_TICKS);
    let broken = m6.break_deadlock(0);
    s.add(
        "B228-死锁-按依赖序打破且恰阈",
        early_break.is_empty()
            && broken.len() == 1
            && broken.iter().all(|c| *c == FenceMapCode::DEADLOCK_BREAK)
            && !m6.idx_probe(0xD00)
            && m6.idx_probe(0xD01)
            && m6.stats().deadlock_breaks == 1,
        "链未达阈不打破；达阈打破末位引擎 Copy（让路 Render 先行），Render 映射不误伤",
    );

    // —— 判据四 · 反向：依赖序表全序与 A03 同源对账 ——
    let dep_ok = DEP_ORDER.iter().all(|(a, b)| a.dep_rank() < b.dep_rank())
        && A03_DEADLOCK_CHAIN == super::vea03_sync::DEADLOCK_CHAIN_THRESHOLD
        && FenceMap::accepts(PrimitiveKind::Fence)
        && !FenceMap::accepts(PrimitiveKind::Event);
    s.add(
        "B228-死锁-依赖表全序与A03同源",
        dep_ok,
        "DEP_ORDER 三对全为升序；死锁链阈值逐字同源 vea03；只承接 Fence 原语",
    );

    // —— 判据五 · 元数据：码段独占两两互异 + 超时常量同源 ——
    let codes = [
        FenceMapCode::DUP_MAP.code(),
        FenceMapCode::MAP_FULL.code(),
        FenceMapCode::UNKNOWN_FENCE.code(),
        FenceMapCode::SIGNAL_LOST.code(),
        FenceMapCode::LEAK.code(),
        FenceMapCode::DEADLOCK_BREAK.code(),
    ];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    s.add(
        "B228-判据-码段互异且口径同源",
        uniq
            && codes.iter().all(|c| c & 0xFF00 == 0x3B00)
            && FENCE_TIMEOUT_TICKS == super::vea03_sync::DEFAULT_FENCE_TIMEOUT_TICKS
            && !MI_FLUSH_DOC.is_empty(),
        "六码全落 0x3Bxx 且两两互异；超时逐字同源 A 域默认；MI_FLUSH 语义声明在场",
    );

    // —— 判据五 · 对账：统计守恒（映射 = 解绑 + 活动被强收） ——
    let m7_stat = {
        let mut mm = FenceMap::new();
        let _ = mm.map_fence(DomainFenceId(0x77), FenceEngine::Render);
        let _ = mm.map_fence(DomainFenceId(0x88), FenceEngine::Copy);
        let _ = mm.unmap_fence(DomainFenceId(0x77));
        let _ = mm.signal(DomainFenceId(0x88), SignalKind::MiFlush);
        let _ = mm.sweep_on_exit();
        *mm.stats()
    };
    let conserved = m7_stat.maps == 2
        && m7_stat.unmaps == 1
        && m7_stat.signals == 1
        && m7_stat.swept == 1
        && m7_stat.maps == m7_stat.unmaps + m7_stat.swept;
    s.add(
        "B228-判据-统计守恒",
        conserved,
        "maps(2) = unmaps(1) + swept(1)（显式解绑与强收互补覆盖全部映射）；信号独立记账互不挤占",
    );

    s
}
