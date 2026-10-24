//! UNX-F1024 · HPET 比较器分配器（CoRun·Unxreal · UNX-A1-F10 · 定时器深化之四）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F1024`
//!
//! **判据 J1**：UNX-F1024-J1 分配正确（并发 32 请求零双占），回收配平
//! （10 万次分配回收零泄漏），优先级纪律（tick/超时器/高精用途预留位）。
//!
//! ## 要点一：比较器池——容量闭集 4..=33 与 HPET 能力账语义一致
//!
//! 池容量是闭集 `[4, 33]`：下界 4 与 HPET 能力账最小比较器数语义一致，
//! 上界 33 覆盖能力寄存器 num-comparators 字段（bits 13:8，读值 +1）的
//! 最大映射 32 并留一个直构裕位。每槽状态机四态闭集
//! `Free → Allocated → Free`（分配/回收）、`Free ↔ Reserved`（预留位
//! 进出）、`Free/Allocated → Broken → Free`（隔离与修复）；任何越出
//! 迁移矩阵的跃迁一律拒绝并落 STATE_ILLEGAL 诊断。
//!
//! ## 要点二：分配器状态机——请求→选槽→锁定→登记
//!
//! 用途三型闭集 `Tick / Timeout / HighPrec`，优先级 `tick(0) < 超时(1)
//! < 高精(2)`；选槽次序闭集：Tick 升序首适配、Timeout 降序首适配（错峰
//! 让低槽给 tick）、HighPrec 升序首适配并附最优槽降级审计。预留位语义：
//! Reserved 槽对一切授权封闭（判据原文：高精型只能分 Reserved 之外最优
//! 槽）；高精请求在 Free 耗尽时唯一出路是"预留兑现"——先把一个 Reserved
//! 槽显式转 Free 再按升序选中，兑现动作落账可审计。
//!
//! ## 要点三：并发 32 请求零双占
//!
//! 确定性交错调度模拟 32 路并发：单步批量收请求、按 `(优先级, 请求号)`
//! 升序逐个过分配状态机；每次授权后做双核对——池侧位图逐槽核对"位=状
//! 态=单 owner"恰好单占，模拟侧 slot→owner 映射核对无二次命中。任何
//! 双占即红（SLOT_DOUBLE_OCCUPY），判据不允许出现。
//!
//! ## 要点四：回收配平——10 万次分配回收零泄漏
//!
//! LCG 确定性序列驱动 10 万次混合操作（分配/回收/重复回收/坏槽隔离/
//! 修复/预留位进出），in-flight 计数入出配平；泄漏显性闭式
//! `leaked = 分配总数 - 回收总数 - 在用实数`，终态全 Free、in-flight
//! 归零、leaked 必须为零。重复回收拒绝账独立成册（Free 槽再回收即
//! DOUBLE_FREE，代次过期即 GEN_STALE，owner 不符即 OWNER_MISMATCH）。
//!
//! ## 要点五：优先级纪律与降级立案 + 零 panic 面
//!
//! 服务序按逻辑 tick 分批审计：同批内优先级必须非降序，违例即
//! PRIORITY_INVERSION；高精请求未得全池首槽（0 号，最优比较器语义）
//! 即"降级分配"，逐案立案（DowngradeCase）供预留审计。本模块零 panic
//! 面：禁 unwrap/expect/panic!、禁浮点、禁墙钟（逻辑 tick u64）、禁
//! 真实 MMIO（比较器为语义模型）、全确定性（LCG）；诊断码独占
//! `0x4DA0` 段（全域 grep 确认：邻段 0x4cd0=F1021、0x4d40=F1022、
//! 0x9D=F1023，互不重叠）。


use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::format;
// ---------------------------------------------------------------------------
// 一、常量与诊断码闭集（独占 0x4DA0 段）
// ---------------------------------------------------------------------------

/// 池容量闭集下界（与 HPET 能力账最小比较器数语义一致）。
pub const POOL_CAP_MIN: usize = 4;
/// 池容量闭集上界（能力账字段映射最大 32 + 直构裕位 1）。
pub const POOL_CAP_MAX: usize = 33;
/// 诊断段基址（0x4DA0 段 · 本模块独占）。
pub const DIAG_SEG_BASE: u16 = 0x4DA0;
/// 诊断段顶（含守卫码 0x4DAC，段内余量留扩展）。
pub const DIAG_SEG_TOP: u16 = 0x4DAF;
/// 服务序账滚动窗口上限（防无界增长，滚动淘汰最旧）。
pub const SERVE_LOG_CAP: usize = 4096;
/// 降级立案滚动账上限（总数另计于 downgrade_total）。
pub const DOWNGRADE_CASE_CAP: usize = 256;

/// HPET 比较器分配诊断码。独占 `0x4DA0` 段，十三码闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HaCode(pub u16);

impl HaCode {
    /// 池容量越出闭集 4..=33。
    pub const CAP_OUT_OF_RANGE: HaCode = HaCode(0x4DA0);
    /// 槽位双占（一位图位/一槽出现多 owner 迹象）。
    pub const SLOT_DOUBLE_OCCUPY: HaCode = HaCode(0x4DA1);
    /// 重复回收（Free 槽被再次回收）。
    pub const DOUBLE_FREE: HaCode = HaCode(0x4DA2);
    /// 回收者不符（owner 号对不上）。
    pub const OWNER_MISMATCH: HaCode = HaCode(0x4DA3);
    /// 池耗尽（Free 非预留槽不足，且高精兑现无 Reserved 可兑）。
    pub const POOL_EXHAUSTED: HaCode = HaCode(0x4DA4);
    /// 预留位越权（Reserved 槽被试图授予/回收/隔离）。
    pub const RESERVE_VIOLATION: HaCode = HaCode(0x4DA5);
    /// 状态机非法迁移（越出迁移矩阵/槽下标指向未分配语义）。
    pub const STATE_ILLEGAL: HaCode = HaCode(0x4DA6);
    /// 坏槽被使用（Broken 槽参与回收/修复语义之外的操作）。
    pub const SLOT_BROKEN_USE: HaCode = HaCode(0x4DA7);
    /// 泄漏账不平（闭式 leaked != 0）。
    pub const LEAK_DETECTED: HaCode = HaCode(0x4DA8);
    /// 优先级纪律破坏（同批服务序出现降序）。
    pub const PRIORITY_INVERSION: HaCode = HaCode(0x4DA9);
    /// 代次过期（旧句柄对已易主的槽回收）。
    pub const GEN_STALE: HaCode = HaCode(0x4DAA);
    /// 槽下标越界（超出池容量闭集）。
    pub const SLOT_INDEX_OOB: HaCode = HaCode(0x4DAB);
    /// 诊断码越出 0x4DA0 段（段位守卫）。
    pub const SEGMENT_GUARD: HaCode = HaCode(0x4DAC);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因（可操作建议一体）。
    pub fn reason(self) -> String {
        match self {
            HaCode::CAP_OUT_OF_RANGE => {
                "池容量越出闭集 4..=33：核对 HPET 能力账比较器数字段".into()
            }
            HaCode::SLOT_DOUBLE_OCCUPY => {
                "槽位双占：位图/owner 核对出现多占，立即冻结分配并复盘授权链".into()
            }
            HaCode::DOUBLE_FREE => {
                "重复回收：该槽已是 Free 且有回收史，查调用方句柄生命周期".into()
            }
            HaCode::OWNER_MISMATCH => {
                "回收者不符：owner 号与登记不符，查跨组件句柄传递".into()
            }
            HaCode::POOL_EXHAUSTED => {
                "池耗尽：Free 非预留槽为零且预留兑现无源，降级或等待回收".into()
            }
            HaCode::RESERVE_VIOLATION => {
                "预留位越权：Reserved 槽只许显式转 Free 后参与分配".into()
            }
            HaCode::STATE_ILLEGAL => {
                "状态机非法迁移：核对四态迁移矩阵与槽下标有效性".into()
            }
            HaCode::SLOT_BROKEN_USE => {
                "坏槽被使用：Broken 槽只许修复后复用，先走 repair 通道".into()
            }
            HaCode::LEAK_DETECTED => {
                "泄漏账不平：闭式 分配-回收-在用 != 0，核对强制隔离出账路径".into()
            }
            HaCode::PRIORITY_INVERSION => {
                "优先级纪律破坏：同批服务序出现降序，核对调度批量排序".into()
            }
            HaCode::GEN_STALE => {
                "代次过期：旧句柄对已易主槽位回收，用最新授权代次重试".into()
            }
            HaCode::SLOT_INDEX_OOB => {
                "槽下标越界：超出池容量闭集，核对槽号来源".into()
            }
            HaCode::SEGMENT_GUARD => {
                "诊断码越出 0x4DA0 段：核对本模块段基址与闭集".into()
            }
            HaCode(_) => "未知 UNX HPET 比较器分配域诊断码".into(),
        }
    }
}

/// 段位守卫：诊断码必须落 `0x4DA0..=0x4DAF`。
pub const fn ha_segment_ok(code: u16) -> bool {
    code >= DIAG_SEG_BASE && code <= DIAG_SEG_TOP
}

/// 闭集守卫：十三码全部落段（本模块自检用）。
pub fn ha_codes_in_segment() -> bool {
    let all = [
        HaCode::CAP_OUT_OF_RANGE,
        HaCode::SLOT_DOUBLE_OCCUPY,
        HaCode::DOUBLE_FREE,
        HaCode::OWNER_MISMATCH,
        HaCode::POOL_EXHAUSTED,
        HaCode::RESERVE_VIOLATION,
        HaCode::STATE_ILLEGAL,
        HaCode::SLOT_BROKEN_USE,
        HaCode::LEAK_DETECTED,
        HaCode::PRIORITY_INVERSION,
        HaCode::GEN_STALE,
        HaCode::SLOT_INDEX_OOB,
        HaCode::SEGMENT_GUARD,
    ];
    let mut ok = true;
    for c in all.iter() {
        if !ha_segment_ok(c.0) {
            ok = false;
        }
    }
    ok
}

// ---------------------------------------------------------------------------
// 二、用途三型与优先级闭集
// ---------------------------------------------------------------------------

/// 用途三型闭集 + 优先级：`Tick(0) < Timeout(1) < HighPrec(2)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    /// 内核 tick 源（最高频、最低延迟诉求，占最低槽）。
    Tick,
    /// 超时器群（错峰占高端槽，让低槽给 tick）。
    Timeout,
    /// 高精用途（性能敏感，期望全池首槽，未得即降级立案）。
    HighPrec,
}

impl UseKind {
    /// 优先级秩（闭集：0/1/2，小者优先服务）。
    pub const fn rank(self) -> u8 {
        match self {
            UseKind::Tick => 0,
            UseKind::Timeout => 1,
            UseKind::HighPrec => 2,
        }
    }

    /// 选槽次序闭集映射。
    pub const fn select_order(self) -> SelectOrder {
        match self {
            UseKind::Tick => SelectOrder::AscFirstFit,
            UseKind::Timeout => SelectOrder::DescFirstFit,
            UseKind::HighPrec => SelectOrder::AscFirstFitBestAudit,
        }
    }
}

/// 用途型数（闭集大小；索引映射经 rank() 直读，不另设 from-index 表）。
pub const USE_KIND_COUNT: usize = 3;

/// 选槽次序闭集（人话名经导出面直书，不另设 name 表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectOrder {
    /// 升序首适配（Tick：从 0 号起取首个 Free）。
    AscFirstFit,
    /// 降序首适配（Timeout：从最高槽起取首个 Free，错峰）。
    DescFirstFit,
    /// 升序首适配 + 最优槽降级审计（HighPrec）。
    AscFirstFitBestAudit,
}

// ---------------------------------------------------------------------------
// 三、槽位状态机——四态闭集与迁移矩阵
// ---------------------------------------------------------------------------

/// 槽位四态闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    /// 空闲：可被授予（授予时转 Allocated）。
    Free,
    /// 在用：持单 owner，回收时转 Free。
    Allocated,
    /// 预留位：对一切授权封闭，只许显式转 Free（预留兑现）或解除。
    Reserved,
    /// 坏槽：隔离不复分，只许修复转 Free。
    Broken,
}

impl SlotState {
    /// 数组下标（统计账用）。
    pub const fn index(self) -> usize {
        match self {
            SlotState::Free => 0,
            SlotState::Allocated => 1,
            SlotState::Reserved => 2,
            SlotState::Broken => 3,
        }
    }
}

/// 状态总数（统计账维度）。
pub const SLOT_STATE_COUNT: usize = 4;

/// 迁移矩阵：合法迁移闭集。
///
/// * `Free → Allocated`（授权）
/// * `Allocated → Free`（回收）
/// * `Free → Reserved`（预留入位）
/// * `Reserved → Free`（预留解除/兑现）
/// * `Free → Broken`（静置检出）
/// * `Allocated → Broken`（强制隔离，随回收出账）
/// * `Broken → Free`（修复）
pub const fn transition_legal(from: SlotState, to: SlotState) -> bool {
    match (from, to) {
        (SlotState::Free, SlotState::Allocated) => true,
        (SlotState::Allocated, SlotState::Free) => true,
        (SlotState::Free, SlotState::Reserved) => true,
        (SlotState::Reserved, SlotState::Free) => true,
        (SlotState::Free, SlotState::Broken) => true,
        (SlotState::Allocated, SlotState::Broken) => true,
        (SlotState::Broken, SlotState::Free) => true,
        _ => false,
    }
}

/// 迁移拒绝人话（诊断辅助）。
pub fn transition_reject_reason(from: SlotState, to: SlotState) -> String {
    if transition_legal(from, to) {
        format!("迁移合法：{}->{}", from.index(), to.index())
    } else {
        format!("迁移 {}->{} 越出迁移矩阵：核对四态闭集", from.index(), to.index())
    }
}

// ---------------------------------------------------------------------------
// 四、请求 / 授权 / 降级案 —— 分配状态机的数据面
// ---------------------------------------------------------------------------

/// 分配请求（调用方递交，字段不可信须池侧校验）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocRequest {
    /// 请求号（全局单调，服务序 tie-break）。
    pub req_id: u64,
    /// owner 号（回收时须出示一致）。
    pub owner: u64,
    /// 用途三型。
    pub use_kind: UseKind,
    /// 逻辑 tick（禁墙钟；服务序按 tick 分批审计）。
    pub tick: u64,
}

/// 授权凭据（回收凭证：slot + owner + gen 三元组）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocGrant {
    /// 对应请求号。
    pub req_id: u64,
    /// 授予槽号。
    pub slot: usize,
    /// 持有 owner（回收时须出示一致）。
    pub owner: u64,
    /// 授权代次（槽代次快照，回收时须一致防旧句柄复用）。
    pub gen: u64,
    /// 用途三型回执。
    pub use_kind: UseKind,
    /// 授权时逻辑 tick。
    pub tick: u64,
    /// 是否高精降级分配（true 即已立案）。
    pub downgraded: bool,
    /// 是否经预留兑现路径（Reserved 显式转 Free 后授予）。
    pub via_reserve_release: bool,
}

/// 高精降级立案（req_id/期望最优槽 0 号/实授槽/立案 tick）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DowngradeCase {
    pub req_id: u64,
    pub expected_best: usize,
    pub granted: usize,
    pub tick: u64,
}

/// 配平快照（grants_in/frees_out 入出账；inflight 为逐槽扫描实数；
/// leaked = grants_in - frees_out - inflight 必须为零；all_free 终态裁定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BalanceSnap {
    pub grants_in: u64,
    pub frees_out: u64,
    pub inflight: u64,
    pub leaked: u64,
    pub all_free: bool,
    /// 四态逐槽计数（Free/Allocated/Reserved/Broken）。
    pub per_state: [u64; SLOT_STATE_COUNT],
}

/// 优先级纪律快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisciplineSnap {
    /// 服务序账长（滚动窗口内）。
    pub serve_log_len: usize,
    /// 同批降序违例数（必须为零）。
    pub inversions: u64,
    /// 降级立案总数（滚动账外另计总数）。
    pub downgrade_total: u64,
    /// tick/超时被预留位挡下的次数累计。
    pub reserve_blocks: u64,
    /// 预留兑现次数累计。
    pub reserve_redeems: u64,
}

// ---------------------------------------------------------------------------
// 五、LCG 确定性随机（全确定性纪律，禁真随机/浮点）
// ---------------------------------------------------------------------------

/// 64 位 LCG（Knuth 常数），确定性可再生。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    const MUL: u64 = 6_364_136_223_846_793_005;
    const INC: u64 = 1_442_695_040_888_963_407;

    /// 播种（异或混合防零种子退化，确定性可再生）。
    pub fn new(seed: u64) -> Self {
        Lcg {
            state: seed
                .wrapping_mul(Self::MUL)
                .wrapping_add(Self::INC)
                .wrapping_add(0xF102_4C0D_E),
        }
    }

    /// 下一原始 u64。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(Self::MUL)
            .wrapping_add(Self::INC);
        self.state
    }

    /// `[0, n)` 均匀取（n=0 得 0，调用方自行保证非空域）。
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next_u64() % n
        }
    }

    /// `[lo, hi]` 闭区间取（hi<lo 时得 lo）。
    pub fn next_range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            lo
        } else {
            lo + self.next_below(hi - lo + 1)
        }
    }
}

// ---------------------------------------------------------------------------
// 六、槽位记录与比较器池
// ---------------------------------------------------------------------------

/// 单槽记录（语义模型，无 MMIO）。
#[derive(Debug, Clone)]
pub struct Slot {
    /// 槽号（0 起）。
    pub index: usize,
    /// 四态。
    pub state: SlotState,
    /// 在用 owner（Allocated 时必 Some）。
    pub owner: Option<u64>,
    /// 在用用途（Allocated 时必 Some）。
    pub use_kind: Option<UseKind>,
    /// 槽代次（每次离开 Allocated 或修复时 +1，防旧句柄复用）。
    pub gen: u64,
    /// 最近授权逻辑 tick。
    pub alloc_tick: u64,
    /// 累计授权次数。
    pub alloc_count: u64,
    /// 累计正常回收次数。
    pub free_count: u64,
    /// 隔离原因（Broken 时 Some）。
    pub broken_reason: Option<String>,
}

impl Slot {
    /// 新建 Free 槽。
    pub fn new(index: usize) -> Self {
        Slot {
            index,
            state: SlotState::Free,
            owner: None,
            use_kind: None,
            gen: 0,
            alloc_tick: 0,
            alloc_count: 0,
            free_count: 0,
            broken_reason: None,
        }
    }
}

/// HPET 比较器池（比较器资源管理语义模型）。
///
/// 不变量（任何公开操作后必须成立，`assert_no_double_occupy` 可核）：
/// * 位图第 i 位为 1 ⟺ 槽 i 状态为 Allocated；
/// * 槽 i 为 Allocated ⟺ owner/use_kind 均 Some 且互斥单占；
/// * Reserved/Broken 槽永不入授权选择面。
#[derive(Debug, Clone)]
pub struct HpetComparatorPool {
    cap: usize,
    slots: Vec<Slot>,
    bitmap: u64,
    logic_tick: u64,
    // —— 入出账 ——
    grants_in: u64,
    frees_out: u64,
    peak_inflight: u64,
    // —— 拒绝账 ——
    reject_double_free: u64,
    reject_owner_mismatch: u64,
    reject_gen_stale: u64,
    reject_state_illegal: u64,
    reject_oob: u64,
    double_occupy_hits: u64,
    occupy_checks: u64,
    exhaust_hits: u64,
    // —— 预留位账 ——
    reserve_marks: u64,
    unreserve_ops: u64,
    reserve_redeems: u64,
    reserve_blocks: u64,
    // —— 健康账 ——
    broken_marks: u64,
    forced_isolations: u64,
    repairs: u64,
    // —— 优先级/降级审计 ——
    serve_log: Vec<(u64, u8)>,
    downgrade_total: u64,
    downgrade_cases: Vec<DowngradeCase>,
    grant_kind_counts: [u64; USE_KIND_COUNT],
}

impl HpetComparatorPool {
    /// 直构：容量闭集 `4..=33`，越界即 Err（CAP_OUT_OF_RANGE）。
    pub fn new(cap: usize) -> Result<Self, HaCode> {
        if cap < POOL_CAP_MIN || cap > POOL_CAP_MAX {
            return Err(HaCode::CAP_OUT_OF_RANGE);
        }
        let mut slots = Vec::new();
        for i in 0..cap {
            slots.push(Slot::new(i));
        }
        Ok(HpetComparatorPool {
            cap,
            slots,
            bitmap: 0,
            logic_tick: 0,
            grants_in: 0,
            frees_out: 0,
            peak_inflight: 0,
            reject_double_free: 0,
            reject_owner_mismatch: 0,
            reject_gen_stale: 0,
            reject_state_illegal: 0,
            reject_oob: 0,
            double_occupy_hits: 0,
            occupy_checks: 0,
            exhaust_hits: 0,
            reserve_marks: 0,
            unreserve_ops: 0,
            reserve_redeems: 0,
            reserve_blocks: 0,
            broken_marks: 0,
            forced_isolations: 0,
            repairs: 0,
            serve_log: Vec::new(),
            downgrade_total: 0,
            downgrade_cases: Vec::new(),
            grant_kind_counts: [0; USE_KIND_COUNT],
        })
    }

    /// 能力账口径构造：raw 为 HPET 能力寄存器读值，比较器数 =
    /// `((raw >> 8) & 0x1F) + 1`（bits 13:8 字段语义，最大 32）；
    /// 映射值须落闭集 `4..=33`（能力账上限 32，33 仅直构可达）。
    pub fn from_capability(raw: u64) -> Result<Self, HaCode> {
        let cmp_n = (((raw >> 8) & 0x1F).wrapping_add(1)) as usize;
        HpetComparatorPool::new(cmp_n)
    }

    /// 池容量。
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 逻辑 tick（只读）。
    pub fn logic_tick(&self) -> u64 {
        self.logic_tick
    }

    /// 槽状态读取（越界 Err(SLOT_INDEX_OOB)）。
    pub fn state_of(&self, slot: usize) -> Result<SlotState, HaCode> {
        match self.slots.get(slot) {
            Some(s) => Ok(s.state),
            None => Err(HaCode::SLOT_INDEX_OOB),
        }
    }

    /// 槽只读引用（越界 None）。
    pub fn slot(&self, slot: usize) -> Option<&Slot> {
        self.slots.get(slot)
    }

    /// 位图原值（审计导出用）。
    pub fn bitmap_raw(&self) -> u64 {
        self.bitmap
    }

    // -- 统计面 ------------------------------------------------------------

    /// 四态逐槽计数。
    pub fn count_by_state(&self) -> [u64; SLOT_STATE_COUNT] {
        let mut out = [0u64; SLOT_STATE_COUNT];
        for s in self.slots.iter() {
            out[s.state.index()] = out[s.state.index()].wrapping_add(1);
        }
        out
    }

    /// 在用实数（逐槽扫描 Allocated 计数——不信任计数器，显性闭式要求实数）。
    pub fn inflight(&self) -> u64 {
        self.count_by_state()[SlotState::Allocated.index()]
    }

    /// 用途型授权计数（Tick/Timeout/HighPrec）。
    pub fn grant_kind_counts(&self) -> [u64; USE_KIND_COUNT] {
        self.grant_kind_counts
    }

    /// 入账总数。
    pub fn grants_in_total(&self) -> u64 {
        self.grants_in
    }

    /// 出账总数（含强制隔离出账）。
    pub fn frees_out_total(&self) -> u64 {
        self.frees_out
    }

    /// 在用峰值。
    pub fn peak_inflight(&self) -> u64 {
        self.peak_inflight
    }

    /// 拒绝账五元组（重复回收/owner 不符/代次过期/非法迁移/下标越界）。
    pub fn reject_ledger(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.reject_double_free,
            self.reject_owner_mismatch,
            self.reject_gen_stale,
            self.reject_state_illegal,
            self.reject_oob,
        )
    }

    /// 双占命中数与核对次数（命中必须恒零）。
    pub fn double_occupy_ledger(&self) -> (u64, u64) {
        (self.double_occupy_hits, self.occupy_checks)
    }

    /// 池耗尽命中数。
    pub fn exhaust_hits(&self) -> u64 {
        self.exhaust_hits
    }

    /// 预留位账（入位/解除/兑现/挡下）。
    pub fn reserve_ledger(&self) -> (u64, u64, u64, u64) {
        (
            self.reserve_marks,
            self.unreserve_ops,
            self.reserve_redeems,
            self.reserve_blocks,
        )
    }

    /// 健康账（隔离标记/强制隔离/修复）。
    pub fn health_ledger(&self) -> (u64, u64, u64) {
        (self.broken_marks, self.forced_isolations, self.repairs)
    }

    /// 降级立案总数与滚动案卷长。
    pub fn downgrade_ledger(&self) -> (u64, usize) {
        (self.downgrade_total, self.downgrade_cases.len())
    }

    /// 降级案卷只读。
    pub fn downgrade_cases(&self) -> &[DowngradeCase] {
        &self.downgrade_cases
    }

    /// 服务序账只读（tick, rank）序列。
    pub fn serve_log(&self) -> &[(u64, u8)] {
        &self.serve_log
    }

    // -- 位图基元 ----------------------------------------------------------

    fn bit_get(&self, i: usize) -> bool {
        i < 64 && (self.bitmap >> i) & 1 == 1
    }

    fn bit_set(&mut self, i: usize) {
        if i < 64 {
            self.bitmap |= 1u64 << i;
        }
    }

    fn bit_clear(&mut self, i: usize) {
        if i < 64 {
            self.bitmap &= !(1u64 << i);
        }
    }
}

// ---------------------------------------------------------------------------
// 七、选槽次序与碎片统计
// ---------------------------------------------------------------------------

impl HpetComparatorPool {
    /// 按选槽次序闭集展开槽号序列（升序/降序；Broken/Reserved 保留在
    /// 序列中由选择面过滤——序列只定方向，不定可用性）。
    pub fn ordered_indices(&self, order: SelectOrder) -> Vec<usize> {
        let mut v = Vec::new();
        match order {
            SelectOrder::AscFirstFit | SelectOrder::AscFirstFitBestAudit => {
                let mut i = 0usize;
                while i < self.cap {
                    v.push(i);
                    i = i.wrapping_add(1);
                }
            }
            SelectOrder::DescFirstFit => {
                let mut i = self.cap;
                while i > 0 {
                    i = i.wrapping_sub(1);
                    v.push(i);
                }
            }
        }
        v
    }

    /// Free 首槽下标（Reserved/Broken 排除——"Reserved 之外"选择面）。
    pub fn best_free_slot_excluding_reserved(&self) -> Option<usize> {
        for s in self.slots.iter() {
            if s.state == SlotState::Free {
                return Some(s.index);
            }
        }
        None
    }

    /// Reserved 首槽（预留兑现候选，取最低号=最优质量）。
    pub fn lowest_reserved_slot(&self) -> Option<usize> {
        for s in self.slots.iter() {
            if s.state == SlotState::Reserved {
                return Some(s.index);
            }
        }
        None
    }

    /// Free 连续段数（碎片统计基元：0..cap 扫描，非 Free 即断段）。
    pub fn free_runs(&self) -> usize {
        let mut runs = 0usize;
        let mut prev_free = false;
        for s in self.slots.iter() {
            let now_free = s.state == SlotState::Free;
            if now_free && !prev_free {
                runs = runs.wrapping_add(1);
            }
            prev_free = now_free;
        }
        runs
    }

    /// 碎片率（千分比闭式）：`free_runs * 1000 / free_count`，
    /// free_count 为零时定义为 0（无空闲无碎片可言）。
    pub fn fragmentation_per_mille(&self) -> u64 {
        let fc = self.count_by_state()[SlotState::Free.index()];
        if fc == 0 {
            0
        } else {
            (self.free_runs() as u64)
                .wrapping_mul(1000)
                .wrapping_div(fc)
        }
    }

    /// 断言核对：位图逐槽核对"位=状态=单 owner"恰好单占。
    ///
    /// 双占即红：任何位/状态/owner 不一致 → Err(SLOT_DOUBLE_OCCUPY)。
    pub fn assert_no_double_occupy(&mut self) -> Result<(), HaCode> {
        self.occupy_checks = self.occupy_checks.wrapping_add(1);
        for s in self.slots.iter() {
            let bit = self.bit_get(s.index);
            let expect = s.state == SlotState::Allocated;
            if bit != expect {
                self.double_occupy_hits = self.double_occupy_hits.wrapping_add(1);
                return Err(HaCode::SLOT_DOUBLE_OCCUPY);
            }
            if s.state == SlotState::Allocated && s.owner.is_none() {
                self.double_occupy_hits = self.double_occupy_hits.wrapping_add(1);
                return Err(HaCode::SLOT_DOUBLE_OCCUPY);
            }
            if s.state != SlotState::Allocated && s.owner.is_some() {
                self.double_occupy_hits = self.double_occupy_hits.wrapping_add(1);
                return Err(HaCode::SLOT_DOUBLE_OCCUPY);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 八、分配状态机——请求 → 选槽 → 锁定 → 登记
// ---------------------------------------------------------------------------

impl HpetComparatorPool {
    /// 分配状态机。
    ///
    /// 步骤闭集：
    /// 1. **请求**：逻辑 tick 单调吸收；用途型合法；
    /// 2. **选槽**：按用途型选槽次序闭集扫描 Free 面（Reserved/Broken
    ///    恒排除；tick/超时扫过 Reserved 即计入预留挡下账）；
    /// 3. **锁定**：迁移矩阵核 `Free→Allocated`，owner/use_kind 登记，
    ///    位图置位——非 Free 槽到不了这里（双占结构性不可能）；
    /// 4. **登记**：入账 +1、峰值刷新、用途计数、服务序入账、
    ///    高精降级审计（实授 > 全池首槽即立案）。
    ///
    /// Free 耗尽时：高精型走"预留兑现"（最低号 Reserved 显式转 Free 后
    /// 选中，via_reserve_release=true）；仍无源 → Err(POOL_EXHAUSTED)。
    pub fn allocate(&mut self, req: &AllocRequest) -> Result<AllocGrant, HaCode> {
        // —— 步骤 1：请求吸收 ——
        if req.tick > self.logic_tick {
            self.logic_tick = req.tick;
        }

        // —— 步骤 2：选槽（次序闭集） ——
        let order = req.use_kind.select_order();
        let mut reserved_skipped: u64 = 0;
        let mut chosen: Option<usize> = None;
        for i in self.ordered_indices(order) {
            match self.slots.get(i) {
                Some(s) => match s.state {
                    SlotState::Free => {
                        chosen = Some(i);
                        break;
                    }
                    SlotState::Reserved => {
                        // Reserved 对一切授权封闭；扫过即挡下落账。
                        reserved_skipped = reserved_skipped.wrapping_add(1);
                    }
                    SlotState::Allocated | SlotState::Broken => {}
                },
                None => {}
            }
        }

        // 预留挡下账：tick/超时对预留位的让行计数（高精走兑现通道另计）。
        if reserved_skipped > 0 {
            self.reserve_blocks = self.reserve_blocks.wrapping_add(reserved_skipped);
        }

        // 高精预留兑现：Free 耗尽且存在 Reserved → 最低号 Reserved 显式
        // 转 Free 再选（兑现即出预留账；授予槽此刻已在 Reserved 之外）。
        let mut via_reserve_release = false;
        if chosen.is_none() && req.use_kind == UseKind::HighPrec {
            if let Some(ri) = self.lowest_reserved_slot() {
                match self.unreserve_slot(ri) {
                    Ok(()) => {
                        self.reserve_redeems = self.reserve_redeems.wrapping_add(1);
                        via_reserve_release = true;
                        chosen = Some(ri);
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        let slot_idx = match chosen {
            Some(i) => i,
            None => {
                self.exhaust_hits = self.exhaust_hits.wrapping_add(1);
                return Err(HaCode::POOL_EXHAUSTED);
            }
        };

        // —— 步骤 3：锁定（迁移矩阵核 + 单占登记） ——
        let gen = match self.slots.get(slot_idx) {
            Some(s) => {
                if !transition_legal(s.state, SlotState::Allocated) {
                    self.reject_state_illegal = self.reject_state_illegal.wrapping_add(1);
                    return Err(HaCode::STATE_ILLEGAL);
                }
                s.gen
            }
            None => {
                self.reject_oob = self.reject_oob.wrapping_add(1);
                return Err(HaCode::SLOT_INDEX_OOB);
            }
        };
        {
            let s = match self.slots.get_mut(slot_idx) {
                Some(s) => s,
                None => {
                    self.reject_oob = self.reject_oob.wrapping_add(1);
                    return Err(HaCode::SLOT_INDEX_OOB);
                }
            };
            s.state = SlotState::Allocated;
            s.owner = Some(req.owner);
            s.use_kind = Some(req.use_kind);
            s.alloc_tick = req.tick;
            s.alloc_count = s.alloc_count.wrapping_add(1);
        }
        self.bit_set(slot_idx);

        // —— 步骤 4：登记 ——
        self.grants_in = self.grants_in.wrapping_add(1);
        let ki = req.use_kind.rank() as usize;
        if ki < USE_KIND_COUNT {
            self.grant_kind_counts[ki] = self.grant_kind_counts[ki].wrapping_add(1);
        }
        let inflight_now = self.inflight();
        if inflight_now > self.peak_inflight {
            self.peak_inflight = inflight_now;
        }
        self.serve_log_push(req.tick, req.use_kind.rank());

        // 高精降级审计：理想最优槽 = 全池首槽 0 号（最优比较器语义）；
        // 实授高于 0 号即"降级分配"，逐案立案（滚动案卷 + 总数）。
        let downgraded = req.use_kind == UseKind::HighPrec && slot_idx > 0;
        if downgraded {
            self.downgrade_total = self.downgrade_total.wrapping_add(1);
            self.downgrade_cases.push(DowngradeCase {
                req_id: req.req_id,
                expected_best: 0,
                granted: slot_idx,
                tick: req.tick,
            });
            if self.downgrade_cases.len() > DOWNGRADE_CASE_CAP {
                self.downgrade_cases.remove(0);
            }
        }

        Ok(AllocGrant {
            req_id: req.req_id,
            slot: slot_idx,
            owner: req.owner,
            gen,
            use_kind: req.use_kind,
            tick: req.tick,
            downgraded,
            via_reserve_release,
        })
    }

    /// 服务序入账（滚动窗口 SERVE_LOG_CAP）。
    fn serve_log_push(&mut self, tick: u64, rank: u8) {
        self.serve_log.push((tick, rank));
        if self.serve_log.len() > SERVE_LOG_CAP {
            self.serve_log.remove(0);
        }
    }
}

// ---------------------------------------------------------------------------
// 九、回收状态机——三元组核验 + 拒绝账
// ---------------------------------------------------------------------------

impl HpetComparatorPool {
    /// 回收状态机：出示 `(slot, owner, gen)` 三元组。
    ///
    /// 拒绝路径闭集（各自独立落账）：
    /// * 槽号越界 → SLOT_INDEX_OOB（reject_oob）；
    /// * 槽在 Free 且有回收史 → DOUBLE_FREE（reject_double_free）；
    /// * 槽在 Free 但从无回收史 → STATE_ILLEGAL（reject_state_illegal）；
    /// * 槽在 Reserved → RESERVE_VIOLATION（越权回收预留位）；
    /// * 槽在 Broken → SLOT_BROKEN_USE；
    /// * owner 不符 → OWNER_MISMATCH（reject_owner_mismatch）；
    /// * 代次过期 → GEN_STALE（reject_gen_stale）。
    ///
    /// 成功：`Allocated → Free`，gen +1（旧句柄从此失配），出账 +1。
    pub fn release(&mut self, slot: usize, owner: u64, gen: u64) -> Result<(), HaCode> {
        let st = match self.slots.get(slot) {
            Some(s) => s.state,
            None => {
                self.reject_oob = self.reject_oob.wrapping_add(1);
                return Err(HaCode::SLOT_INDEX_OOB);
            }
        };
        match st {
            SlotState::Allocated => {
                let s = match self.slots.get_mut(slot) {
                    Some(s) => s,
                    None => {
                        self.reject_oob = self.reject_oob.wrapping_add(1);
                        return Err(HaCode::SLOT_INDEX_OOB);
                    }
                };
                match s.owner {
                    Some(o) if o == owner => {}
                    _ => {
                        self.reject_owner_mismatch =
                            self.reject_owner_mismatch.wrapping_add(1);
                        return Err(HaCode::OWNER_MISMATCH);
                    }
                }
                if s.gen != gen {
                    self.reject_gen_stale = self.reject_gen_stale.wrapping_add(1);
                    return Err(HaCode::GEN_STALE);
                }
                if !transition_legal(SlotState::Allocated, SlotState::Free) {
                    self.reject_state_illegal = self.reject_state_illegal.wrapping_add(1);
                    return Err(HaCode::STATE_ILLEGAL);
                }
                s.state = SlotState::Free;
                s.owner = None;
                s.use_kind = None;
                s.free_count = s.free_count.wrapping_add(1);
                s.gen = s.gen.wrapping_add(1);
                self.bit_clear(slot);
                self.frees_out = self.frees_out.wrapping_add(1);
                let inflight_now = self.inflight();
                if inflight_now > self.peak_inflight {
                    self.peak_inflight = inflight_now;
                }
                Ok(())
            }
            SlotState::Free => {
                if self.slots[slot].free_count > 0 {
                    self.reject_double_free = self.reject_double_free.wrapping_add(1);
                    Err(HaCode::DOUBLE_FREE)
                } else {
                    self.reject_state_illegal = self.reject_state_illegal.wrapping_add(1);
                    Err(HaCode::STATE_ILLEGAL)
                }
            }
            SlotState::Reserved => Err(HaCode::RESERVE_VIOLATION),
            SlotState::Broken => Err(HaCode::SLOT_BROKEN_USE),
        }
    }
}

// ---------------------------------------------------------------------------
// 十、预留位管理与槽位健康账
// ---------------------------------------------------------------------------

impl HpetComparatorPool {
    /// 预留入位：从最高号 Free 向下标记 `count` 个槽为 Reserved
    /// （高槽入预留，低槽留给 tick——与选槽错峰纪律一致）。
    /// Free 不足即整体不入（all-or-nothing），Err(POOL_EXHAUSTED)。
    pub fn reserve_slots(&mut self, count: usize) -> Result<usize, HaCode> {
        if count == 0 {
            return Ok(0);
        }
        let free_now = self.count_by_state()[SlotState::Free.index()] as usize;
        if free_now < count {
            return Err(HaCode::POOL_EXHAUSTED);
        }
        let mut picked: Vec<usize> = Vec::new();
        let mut i = self.cap;
        while i > 0 && picked.len() < count {
            i = i.wrapping_sub(1);
            if let Some(s) = self.slots.get(i) {
                if s.state == SlotState::Free {
                    picked.push(i);
                }
            }
        }
        for idx in picked.iter() {
            if let Some(s) = self.slots.get_mut(*idx) {
                if transition_legal(s.state, SlotState::Reserved) {
                    s.state = SlotState::Reserved;
                    self.reserve_marks = self.reserve_marks.wrapping_add(1);
                }
            }
        }
        Ok(picked.len())
    }

    /// 预留解除：单槽 `Reserved → Free`。
    pub fn unreserve_slot(&mut self, slot: usize) -> Result<(), HaCode> {
        let s = match self.slots.get_mut(slot) {
            Some(s) => s,
            None => {
                self.reject_oob = self.reject_oob.wrapping_add(1);
                return Err(HaCode::SLOT_INDEX_OOB);
            }
        };
        if s.state != SlotState::Reserved {
            return Err(HaCode::RESERVE_VIOLATION);
        }
        if !transition_legal(SlotState::Reserved, SlotState::Free) {
            return Err(HaCode::STATE_ILLEGAL);
        }
        s.state = SlotState::Free;
        self.unreserve_ops = self.unreserve_ops.wrapping_add(1);
        Ok(())
    }

    /// 预留全解除（收官清池用），返回解除数。
    pub fn unreserve_all(&mut self) -> usize {
        let mut n = 0usize;
        let mut idx = 0usize;
        while idx < self.cap {
            if self.unreserve_slot(idx).is_ok() {
                n = n.wrapping_add(1);
            }
            idx = idx.wrapping_add(1);
        }
        n
    }

    /// 坏槽隔离：
    /// * Free → Broken（静置检出）；
    /// * Allocated → Broken（强制隔离：owner 清空、位图清位、gen +1、
    ///   **随回收出账 frees_out +1**——强制隔离即回收语义，保闭式配平）；
    /// * Reserved → 越权拒绝；Broken 重复隔离 → STATE_ILLEGAL。
    pub fn mark_broken(&mut self, slot: usize, reason: String) -> Result<(), HaCode> {
        let st = match self.slots.get(slot) {
            Some(s) => s.state,
            None => {
                self.reject_oob = self.reject_oob.wrapping_add(1);
                return Err(HaCode::SLOT_INDEX_OOB);
            }
        };
        match st {
            SlotState::Free | SlotState::Allocated => {
                let was_allocated = st == SlotState::Allocated;
                let s = match self.slots.get_mut(slot) {
                    Some(s) => s,
                    None => return Err(HaCode::SLOT_INDEX_OOB),
                };
                if !transition_legal(st, SlotState::Broken) {
                    self.reject_state_illegal = self.reject_state_illegal.wrapping_add(1);
                    return Err(HaCode::STATE_ILLEGAL);
                }
                s.state = SlotState::Broken;
                s.broken_reason = Some(reason);
                s.owner = None;
                s.use_kind = None;
                s.gen = s.gen.wrapping_add(1);
                self.bit_clear(slot);
                self.broken_marks = self.broken_marks.wrapping_add(1);
                if was_allocated {
                    // 强制隔离即回收语义：出账 +1，闭式 leaked 恒平。
                    self.frees_out = self.frees_out.wrapping_add(1);
                    self.forced_isolations = self.forced_isolations.wrapping_add(1);
                }
                Ok(())
            }
            SlotState::Reserved => Err(HaCode::RESERVE_VIOLATION),
            SlotState::Broken => {
                self.reject_state_illegal = self.reject_state_illegal.wrapping_add(1);
                Err(HaCode::STATE_ILLEGAL)
            }
        }
    }

    /// 修复：`Broken → Free`，gen +1（隔离期旧句柄永久失配）。
    pub fn repair_slot(&mut self, slot: usize) -> Result<(), HaCode> {
        let s = match self.slots.get_mut(slot) {
            Some(s) => s,
            None => {
                self.reject_oob = self.reject_oob.wrapping_add(1);
                return Err(HaCode::SLOT_INDEX_OOB);
            }
        };
        if s.state != SlotState::Broken {
            return Err(HaCode::SLOT_BROKEN_USE);
        }
        if !transition_legal(SlotState::Broken, SlotState::Free) {
            return Err(HaCode::STATE_ILLEGAL);
        }
        s.state = SlotState::Free;
        s.broken_reason = None;
        s.gen = s.gen.wrapping_add(1);
        self.repairs = self.repairs.wrapping_add(1);
        Ok(())
    }

    /// 坏槽清单（隔离不复分核对用）。
    pub fn broken_indices(&self) -> Vec<usize> {
        let mut v = Vec::new();
        for s in self.slots.iter() {
            if s.state == SlotState::Broken {
                v.push(s.index);
            }
        }
        v
    }

    /// 预留位清单。
    pub fn reserved_indices(&self) -> Vec<usize> {
        let mut v = Vec::new();
        for s in self.slots.iter() {
            if s.state == SlotState::Reserved {
                v.push(s.index);
            }
        }
        v
    }
}

// ---------------------------------------------------------------------------
// 十一、配平核算与泄漏显性闭式
// ---------------------------------------------------------------------------

impl HpetComparatorPool {
    /// 泄漏显性闭式：`leaked = 分配入账 - 回收出账 - 在用实数`。
    ///
    /// 每一授权入账一次，每一次离开 Allocated（正常回收或强制隔离）
    /// 出账一次，故该闭式恒为零；非零即账面损坏 → LEAK_DETECTED。
    pub fn leak_closed_form(&self) -> u64 {
        let inflight_now = self.inflight();
        self.grants_in
            .wrapping_sub(self.frees_out)
            .wrapping_sub(inflight_now)
    }

    /// 配平核算快照；`leaked != 0` 即 Err(LEAK_DETECTED)。
    pub fn balance_check(&self) -> Result<BalanceSnap, HaCode> {
        let per_state = self.count_by_state();
        let inflight_now = per_state[SlotState::Allocated.index()];
        let leaked = self
            .grants_in
            .wrapping_sub(self.frees_out)
            .wrapping_sub(inflight_now);
        let mut all_free = true;
        for s in self.slots.iter() {
            if s.state != SlotState::Free {
                all_free = false;
            }
        }
        let snap = BalanceSnap {
            grants_in: self.grants_in,
            frees_out: self.frees_out,
            inflight: inflight_now,
            leaked,
            all_free,
            per_state,
        };
        if leaked != 0 {
            return Err(HaCode::LEAK_DETECTED);
        }
        Ok(snap)
    }

    /// 收官清池：清空全部在用（出示登记三元组）、解除全部预留、
    /// 修复全部坏槽——终态全 Free 前置通道。
    ///
    /// 返回 (正常回收数, 吸收异常数, 解除预留数, 修复数)。
    pub fn drain_all(&mut self) -> (u64, u64, usize, usize) {
        let mut freed = 0u64;
        let mut absorbed = 0u64;
        let mut idx = 0usize;
        while idx < self.cap {
            let (owner, gen, allocated) = match self.slots.get(idx) {
                Some(s) => (
                    s.owner,
                    s.gen,
                    s.state == SlotState::Allocated,
                ),
                None => (None, 0, false),
            };
            if allocated {
                let o = match owner {
                    Some(o) => o,
                    None => 0,
                };
                match self.release(idx, o, gen) {
                    Ok(()) => freed = freed.wrapping_add(1),
                    Err(_) => absorbed = absorbed.wrapping_add(1),
                }
            }
            idx = idx.wrapping_add(1);
        }
        let unres = self.unreserve_all();
        let broken = self.broken_indices();
        let mut repaired = 0usize;
        for b in broken.iter() {
            if self.repair_slot(*b).is_ok() {
                repaired = repaired.wrapping_add(1);
            }
        }
        (freed, absorbed, unres, repaired)
    }

    /// 优先级纪律审计：服务序账按逻辑 tick 分批，同批内优先级
    /// 必须非降序；违例数非零即纪律破坏。
    pub fn priority_discipline_check(&self) -> Result<DisciplineSnap, HaCode> {
        let mut inversions = 0u64;
        let n = self.serve_log.len();
        let mut i = 1usize;
        while i < n {
            let (t0, r0) = self.serve_log[i - 1];
            let (t1, r1) = self.serve_log[i];
            if t0 == t1 && r0 > r1 {
                inversions = inversions.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        let snap = DisciplineSnap {
            serve_log_len: n,
            inversions,
            downgrade_total: self.downgrade_total,
            reserve_blocks: self.reserve_blocks,
            reserve_redeems: self.reserve_redeems,
        };
        if inversions != 0 {
            return Err(HaCode::PRIORITY_INVERSION);
        }
        Ok(snap)
    }
}

// ---------------------------------------------------------------------------
// 十二、并发 32 路确定性交错（判据 J1-1：零双占）
// ---------------------------------------------------------------------------

/// 并发操演报告（lanes 恒 32；double_occupy_hits/discipline_inversions
/// 必须为零——双占即红；final_snap 为终态配平快照）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcurrencyReport {
    pub lanes: usize,
    pub steps: u64,
    pub grants: u64,
    pub frees: u64,
    pub double_occupy_hits: u64,
    /// 池侧/模拟侧单占核对次数。
    pub occupy_checks: u64,
    pub map_checks: u64,
    /// 池耗尽（合法拒绝）与其他拒绝。
    pub exhaust_hits: u64,
    pub other_rejects: u64,
    /// 预留入位/解除/兑现/挡下。
    pub reserve_marks: u64,
    pub unreserve_ops: u64,
    pub reserve_redeems: u64,
    pub reserve_blocks: u64,
    pub downgrades: u64,
    pub final_snap: Option<BalanceSnap>,
    pub discipline_inversions: u64,
}

const CONC_LANES: usize = 32;

/// 路号 → 用途型（确定性映射：lane%3 闭集轮转）。
pub fn concurrency_lane_kind(lane: usize) -> UseKind {
    match lane % USE_KIND_COUNT {
        0 => UseKind::Tick,
        1 => UseKind::Timeout,
        _ => UseKind::HighPrec,
    }
}

/// 并发 32 路确定性交错操演（全确定性，LCG 只决定时序不改语义）：
/// 每步先到期回收，再按 `(优先级, 请求号)` 升序批量服务就绪请求
/// （同批服务序即优先级纪律）；每次授权后池侧位图逐槽核对 + 模拟侧
/// slot→(owner,gen) 映射核对双保险；周期性预留位/坏槽操演压满
/// 分配/拒绝路径。判据线：双占命中为零且终态全 Free 泄漏闭式为零。
pub fn run_concurrency_32(seed: u64, steps: u64) -> Result<ConcurrencyReport, HaCode> {
    // 32 路争用 8 槽小池：饱和争用下 free==0 为常态，压满双占核对、
    // 预留挡下与高精兑现全部路径（容量 8 落闭集 4..=33 内）。
    let mut pool = HpetComparatorPool::new(8)?;
    let mut rng = Lcg::new(seed);
    let mut holding: Vec<Option<AllocGrant>> = vec![None; CONC_LANES];
    let mut cooldown: Vec<u64> = vec![0; CONC_LANES];
    let mut slot_map: Vec<Option<(u64, u64)>> = vec![None; POOL_CAP_MAX];
    let mut req_seq: u64 = 0;
    let mut reserve_pending = false;

    let mut grants = 0u64;
    let mut frees = 0u64;
    let mut map_checks = 0u64;
    let mut exhaust_hits = 0u64;
    let mut other_rejects = 0u64;

    let mut step: u64 = 0;
    while step < steps {
        // —— 一、到期回收 ——
        let mut lane = 0usize;
        while lane < CONC_LANES {
            if let Some(g) = holding[lane] {
                if cooldown[lane] == 0 {
                    match pool.release(g.slot, g.owner, g.gen) {
                        Ok(()) => {
                            frees = frees.wrapping_add(1);
                            map_checks = map_checks.wrapping_add(1);
                            match slot_map.get(g.slot) {
                                Some(Some((o, gn))) if *o == g.owner && *gn == g.gen => {}
                                _ => return Err(HaCode::SLOT_DOUBLE_OCCUPY),
                            }
                            slot_map[g.slot] = None;
                            holding[lane] = None;
                            cooldown[lane] = rng.next_range(1, 6);
                        }
                        Err(_e) => {
                            // 强制隔离等异常路径：句柄作废吸收，lane 空手。
                            holding[lane] = None;
                            cooldown[lane] = rng.next_range(1, 6);
                        }
                    }
                } else {
                    cooldown[lane] = cooldown[lane].wrapping_sub(1);
                }
            }
            if holding[lane].is_none() && cooldown[lane] > 0 {
                cooldown[lane] = cooldown[lane].wrapping_sub(1);
            }
            lane = lane.wrapping_add(1);
        }

        // —— 一·五、预留位操演（97 步排期、释放后空窗即入位；41 步全解除） ——
        if step % 97 == 0 {
            reserve_pending = true;
        }
        let free_now = pool.count_by_state()[SlotState::Free.index()] as usize;
        if reserve_pending && free_now >= 2 {
            let _ = pool.reserve_slots(2);
            reserve_pending = false;
        }
        if step % 41 == 0 {
            let _ = pool.unreserve_all();
        }

        // —— 二、批量就绪请求（优先级纪律：按 (rank, req_id) 升序） ——
        let mut ready: Vec<(u8, u64, usize)> = Vec::new();
        let mut lane = 0usize;
        while lane < CONC_LANES {
            if holding[lane].is_none() && cooldown[lane] == 0 {
                let kind = concurrency_lane_kind(lane);
                ready.push((kind.rank(), req_seq, lane));
                req_seq = req_seq.wrapping_add(1);
            }
            lane = lane.wrapping_add(1);
        }
        ready.sort_by_key(|x| (x.0, x.1));
        for (_rank, rid, lane) in ready.iter() {
            let kind = concurrency_lane_kind(*lane);
            let req = AllocRequest {
                req_id: *rid,
                owner: 1000u64.wrapping_add(*lane as u64),
                use_kind: kind,
                tick: step,
            };
            match pool.allocate(&req) {
                Ok(g) => {
                    // 双占核对（模拟侧）：槽必无主方可登记。
                    map_checks = map_checks.wrapping_add(1);
                    match slot_map.get(g.slot) {
                        Some(None) => {}
                        _ => return Err(HaCode::SLOT_DOUBLE_OCCUPY),
                    }
                    slot_map[g.slot] = Some((g.owner, g.gen));
                    // 双占核对（池侧）：位图逐槽恰好单占。
                    pool.assert_no_double_occupy()?;
                    holding[*lane] = Some(g);
                    cooldown[*lane] = rng.next_range(2, 9);
                    grants = grants.wrapping_add(1);
                }
                Err(HaCode::POOL_EXHAUSTED) => {
                    exhaust_hits = exhaust_hits.wrapping_add(1);
                    cooldown[*lane] = rng.next_range(1, 5);
                }
                Err(_e) => {
                    other_rejects = other_rejects.wrapping_add(1);
                    cooldown[*lane] = rng.next_range(1, 4);
                }
            }
        }

        // —— 四、坏槽操演（静置检出 → 修复，只动 Free 槽） ——
        if step % 613 == 0 {
            let mut free_idx: Vec<usize> = Vec::new();
            let mut i = 0usize;
            while i < pool.cap() {
                if pool.state_of(i) == Ok(SlotState::Free) {
                    free_idx.push(i);
                }
                i = i.wrapping_add(1);
            }
            if free_idx.len() > 4 {
                let pick = rng.next_below(free_idx.len() as u64) as usize;
                let target = free_idx[pick];
                let _ = pool.mark_broken(target, format!("静置检出-步{}", step));
            }
        }
        if step % 617 == 0 {
            let broken = pool.broken_indices();
            if let Some(first) = broken.first() {
                let _ = pool.repair_slot(*first);
            }
        }

        step = step.wrapping_add(1);
    }

    // —— 收官清池：终态全 Free 前置 ——
    let mut lane = 0usize;
    while lane < CONC_LANES {
        if let Some(g) = holding[lane] {
            match pool.release(g.slot, g.owner, g.gen) {
                Ok(()) => frees = frees.wrapping_add(1),
                Err(_) => {}
            }
            holding[lane] = None;
        }
        lane = lane.wrapping_add(1);
    }
    let _ = pool.drain_all();
    let final_snap = pool.balance_check()?;
    let disc = pool.priority_discipline_check()?;
    let (dh, oc) = pool.double_occupy_ledger();
    let (rm, uo, rr, rb) = pool.reserve_ledger();
    let (dt, _dl) = pool.downgrade_ledger();

    Ok(ConcurrencyReport {
        lanes: CONC_LANES,
        steps,
        grants,
        frees,
        double_occupy_hits: dh,
        occupy_checks: oc,
        map_checks,
        exhaust_hits,
        other_rejects,
        reserve_marks: rm,
        unreserve_ops: uo,
        reserve_redeems: rr,
        reserve_blocks: rb,
        downgrades: dt,
        final_snap: Some(final_snap),
        discipline_inversions: disc.inversions,
    })
}

// ---------------------------------------------------------------------------
// 十三、10 万次分配回收配平（判据 J1-2：零泄漏）
// ---------------------------------------------------------------------------

/// 配平操演报告（grants_in/frees_out 入出账；frees 为正常回收、
/// forced_isolations 为强制隔离出账；leaked 必须为零、all_free 必须
/// 为真、final_inflight 必须为零）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceReport {
    pub iters: u64,
    pub grants: u64,
    pub frees: u64,
    pub forced_isolations: u64,
    /// 拒绝账（重复回收/owner 不符/代次过期/非法迁移/越界）。
    pub reject_double_free: u64,
    pub reject_owner_mismatch: u64,
    pub reject_gen_stale: u64,
    pub reject_state_illegal: u64,
    pub reject_oob: u64,
    pub exhaust_hits: u64,
    /// 预留入位/解除/兑现/挡下。
    pub reserve_marks: u64,
    pub unreserve_ops: u64,
    pub reserve_redeems: u64,
    pub reserve_blocks: u64,
    /// 隔离标记/修复次数。
    pub broken_marks: u64,
    pub repairs: u64,
    pub peak_inflight: u64,
    pub final_inflight: u64,
    pub leaked: u64,
    pub all_free: bool,
    /// 终态碎片率（千分比）与四态计数。
    pub frag_per_mille: u64,
    pub final_per_state: [u64; SLOT_STATE_COUNT],
}

/// 10 万次（或指定次数）分配 + 回收 LCG 混合操演（全确定性）。操作
/// 分布：55% 分配、35% 回收、3% 重复回收操演（喂满拒绝路径）、2%
/// 坏槽隔离 + 2% 修复（含 Allocated 强制隔离出账）、2% 预留入位 +
/// 1% 解除。配平语义：终态显性闭式 `leaked = 分配 - 回收 - 在用 == 0`
/// 且全 Free 零泄漏。
pub fn run_balance_100k(seed: u64, iters: u64) -> Result<BalanceReport, HaCode> {
    let mut pool = HpetComparatorPool::new(8)?; // 小池高压：拒收路径高频
    let mut rng = Lcg::new(seed);
    let mut live: Vec<AllocGrant> = Vec::new();
    let mut graveyard: Vec<AllocGrant> = Vec::new();

    let mut exhaust_hits = 0u64;

    let mut i: u64 = 0;
    while i < iters {
        let r = rng.next_below(100);
        if r < 55 || live.is_empty() {
            // —— 分配 ——
            let kind = match rng.next_below(10) {
                0..=3 => UseKind::Tick,
                4..=7 => UseKind::Timeout,
                _ => UseKind::HighPrec,
            };
            let req = AllocRequest {
                req_id: i,
                owner: rng.next_range(1, 64),
                use_kind: kind,
                tick: i,
            };
            match pool.allocate(&req) {
                Ok(_g) => {
                    live.push(_g);
                }
                Err(HaCode::POOL_EXHAUSTED) => exhaust_hits = exhaust_hits.wrapping_add(1),
                Err(_e) => {}
            }
        } else if r < 90 {
            // —— 回收（随机取一个在用句柄） ——
            let pick = rng.next_below(live.len() as u64) as usize;
            let g = live.swap_remove(pick);
            match pool.release(g.slot, g.owner, g.gen) {
                Ok(()) => {
                    graveyard.push(g);
                    if graveyard.len() > 8 {
                        graveyard.remove(0);
                    }
                }
                Err(_e) => {}
            }
        } else if r < 93 {
            // —— 重复回收操演：墓园句柄再回收（必被拒，落账） ——
            if let Some(g) = graveyard.last().copied() {
                let _ = pool.release(g.slot, g.owner, g.gen);
            }
        } else if r < 95 {
            // —— 坏槽隔离（Free 静置检出 / Allocated 强制隔离出账） ——
            let target = rng.next_below(pool.cap() as u64) as usize;
            let reason = match rng.next_below(3) {
                0 => "能力位自检失配".to_string(),
                1 => "比较值写入无回读".to_string(),
                _ => "中断投递超时".to_string(),
            };
            let _ = pool.mark_broken(target, reason);
        } else if r < 97 {
            // —— 修复 ——
            let broken = pool.broken_indices();
            if let Some(first) = broken.first() {
                let _ = pool.repair_slot(*first);
            }
        } else if r < 99 {
            // —— 预留入位（有空富余才入，全或无） ——
            let free_now = pool.count_by_state()[SlotState::Free.index()] as usize;
            if free_now > 3 {
                let n = rng.next_range(1, 2) as usize;
                let _ = pool.reserve_slots(n);
            }
        } else {
            // —— 预留解除（解除最低号一个） ——
            if let Some(ri) = pool.lowest_reserved_slot() {
                let _ = pool.unreserve_slot(ri);
            }
        }

        i = i.wrapping_add(1);
    }

    // —— 收官清池 ——
    let (freed_drain, _absorbed, _unres_drain, _repaired_drain) = pool.drain_all();
    let _ = freed_drain;
    let snap = pool.balance_check()?;
    let (rdf, rom, rgs, rsi, roob) = pool.reject_ledger();
    let (rm, uo, rr, rb) = pool.reserve_ledger();
    let (bm, fi, rp) = pool.health_ledger();
    let inflight_final = snap.inflight;

    Ok(BalanceReport {
        iters,
        grants: snap.grants_in,
        frees: snap.frees_out.wrapping_sub(fi),
        forced_isolations: fi,
        reject_double_free: rdf,
        reject_owner_mismatch: rom,
        reject_gen_stale: rgs,
        reject_state_illegal: rsi,
        reject_oob: roob,
        exhaust_hits,
        reserve_marks: rm,
        unreserve_ops: uo,
        reserve_redeems: rr,
        reserve_blocks: rb,
        broken_marks: bm,
        repairs: rp,
        peak_inflight: pool.peak_inflight(),
        final_inflight: inflight_final,
        leaked: snap.leaked,
        all_free: snap.all_free,
        frag_per_mille: pool.fragmentation_per_mille(),
        final_per_state: snap.per_state,
    })
}

// ---------------------------------------------------------------------------
// 十四、kv 导出
// ---------------------------------------------------------------------------

impl HpetComparatorPool {
    /// 结构化 kv 导出（逐键值对，审计/引导报告通用格式）。
    pub fn export_kv(&self) -> Vec<(String, String)> {
        let per_state = self.count_by_state();
        let (dh, oc) = self.double_occupy_ledger();
        let (rdf, rom, rgs, rsi, roob) = self.reject_ledger();
        let (rm, uo, rr, rb) = self.reserve_ledger();
        let (bm, fi, rp) = self.health_ledger();
        let (dt, dcl) = self.downgrade_ledger();
        let kc = self.grant_kind_counts();
        let mut kv: Vec<(String, String)> = Vec::new();
        kv.push(("unx1024.module".into(), "hpet-comparator-allocator".into()));
        kv.push(("unx1024.diag_seg".into(), format!("0x{:04X}", DIAG_SEG_BASE)));
        kv.push(("unx1024.cap".into(), self.cap.to_string()));
        kv.push(("unx1024.cap_closed_set".into(), format!("{}..={}", POOL_CAP_MIN, POOL_CAP_MAX)));
        kv.push(("unx1024.logic_tick".into(), self.logic_tick.to_string()));
        kv.push(("unx1024.state.free".into(), per_state[0].to_string()));
        kv.push(("unx1024.state.allocated".into(), per_state[1].to_string()));
        kv.push(("unx1024.state.reserved".into(), per_state[2].to_string()));
        kv.push(("unx1024.state.broken".into(), per_state[3].to_string()));
        kv.push(("unx1024.bitmap".into(), format!("0x{:016X}", self.bitmap)));
        kv.push(("unx1024.grants_in".into(), self.grants_in.to_string()));
        kv.push(("unx1024.frees_out".into(), self.frees_out.to_string()));
        kv.push(("unx1024.inflight".into(), self.inflight().to_string()));
        kv.push(("unx1024.peak_inflight".into(), self.peak_inflight.to_string()));
        kv.push(("unx1024.leak_closed_form".into(), self.leak_closed_form().to_string()));
        kv.push(("unx1024.grant.tick".into(), kc[0].to_string()));
        kv.push(("unx1024.grant.timeout".into(), kc[1].to_string()));
        kv.push(("unx1024.grant.highprec".into(), kc[2].to_string()));
        kv.push(("unx1024.reject.double_free".into(), rdf.to_string()));
        kv.push(("unx1024.reject.owner_mismatch".into(), rom.to_string()));
        kv.push(("unx1024.reject.gen_stale".into(), rgs.to_string()));
        kv.push(("unx1024.reject.state_illegal".into(), rsi.to_string()));
        kv.push(("unx1024.reject.index_oob".into(), roob.to_string()));
        kv.push(("unx1024.double_occupy.hits".into(), dh.to_string()));
        kv.push(("unx1024.double_occupy.checks".into(), oc.to_string()));
        kv.push(("unx1024.exhaust.hits".into(), self.exhaust_hits.to_string()));
        kv.push(("unx1024.reserve.marks".into(), rm.to_string()));
        kv.push(("unx1024.reserve.unreserve".into(), uo.to_string()));
        kv.push(("unx1024.reserve.redeems".into(), rr.to_string()));
        kv.push(("unx1024.reserve.blocks".into(), rb.to_string()));
        kv.push(("unx1024.health.broken_marks".into(), bm.to_string()));
        kv.push(("unx1024.health.forced_isolations".into(), fi.to_string()));
        kv.push(("unx1024.health.repairs".into(), rp.to_string()));
        kv.push(("unx1024.frag.free_runs".into(), self.free_runs().to_string()));
        kv.push(("unx1024.frag.per_mille".into(), self.fragmentation_per_mille().to_string()));
        kv.push(("unx1024.serve_log.len".into(), self.serve_log.len().to_string()));
        kv.push(("unx1024.downgrade.total".into(), dt.to_string()));
        kv.push(("unx1024.downgrade.cases".into(), dcl.to_string()));
        kv
    }

    /// kv 导出扁平串（`k=v` 行序拼接）。
    pub fn export_kv_string(&self) -> String {
        let kv = self.export_kv();
        let mut out = String::new();
        for (k, v) in kv.iter() {
            out.push_str(k);
            out.push('=');
            out.push_str(v);
            out.push('\n');
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 十五、判据 J1 自证总装
// ---------------------------------------------------------------------------

/// 单条判据证据项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct J1Item {
    /// 判据编号（J1-1/J1-2/J1-3 细分）。
    pub code: String,
    /// 是否达标。
    pub pass: bool,
    /// 人话说明（含关键数值）。
    pub note: String,
}

/// 判据 J1 自证总报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct J1Evidence {
    /// 证据项序列。
    pub items: Vec<J1Item>,
    /// 总裁定（全部达标才为真）。
    pub pass: bool,
    /// kv 总导出。
    pub kv: Vec<(String, String)>,
}

/// 判据 UNX-F1024-J1 自证总装（并发 32 零双占 + 10 万次配平零泄漏 +
/// 优先级纪律与预留位审计），全确定性：同种子必同报告。
pub fn run_j1_evidence(seed: u64) -> Result<J1Evidence, HaCode> {
    let mut items: Vec<J1Item> = Vec::new();

    // —— 容量闭集守卫（先于一切操演） ——
    let cap_lo = HpetComparatorPool::new(POOL_CAP_MIN - 1).is_err();
    let cap_hi = HpetComparatorPool::new(POOL_CAP_MAX + 1).is_err();
    let cap_in = HpetComparatorPool::new(POOL_CAP_MAX).is_ok();
    let cap_feat = HpetComparatorPool::from_capability((31u64 << 8) | 0xFFFF).is_ok();
    items.push(J1Item {
        code: "J1-0".into(),
        pass: cap_lo && cap_hi && cap_in && cap_feat && ha_codes_in_segment(),
        note: format!(
            "容量闭集 {}..={} 守卫（3拒/34拒/33收/能力账0x1F00+1=32收）+ 0x{:04X} 段十三码守卫",
            POOL_CAP_MIN, POOL_CAP_MAX, DIAG_SEG_BASE
        ),
    });

    // —— J1-1：并发 32 请求零双占 ——
    let conc = run_concurrency_32(seed, 4096)?;
    let c1 = conc.double_occupy_hits == 0 && conc.lanes == 32;
    items.push(J1Item {
        code: "J1-1a".into(),
        pass: c1,
        note: format!(
            "并发 32 路 × {} 步交错：授权 {} 回收 {}，双占命中 {}（须零），单占核对池侧 {} 次/模拟侧 {} 次",
            conc.steps, conc.grants, conc.frees, conc.double_occupy_hits,
            conc.occupy_checks, conc.map_checks
        ),
    });
    let c2 = match &conc.final_snap {
        Some(s) => s.all_free && s.leaked == 0 && s.inflight == 0,
        None => false,
    };
    items.push(J1Item {
        code: "J1-1b".into(),
        pass: c2,
        note: "并发操演终态：全 Free + in-flight 归零 + 泄漏闭式为零".into(),
    });

    // —— J1-2：10 万次分配回收配平 ——
    let bal = run_balance_100k(seed ^ 0x5EED_F102, 100_000)?;
    let b1 = bal.iters == 100_000
        && bal.leaked == 0
        && bal.all_free
        && bal.final_inflight == 0;
    items.push(J1Item {
        code: "J1-2a".into(),
        pass: b1,
        note: format!(
            "10 万次混合操演：分配 {} 回收 {}（强制隔离出账 {}），闭式 leaked={}，终态全 Free={}",
            bal.grants, bal.frees, bal.forced_isolations, bal.leaked, bal.all_free
        ),
    });
    let b2 = bal.reject_double_free > 0 && bal.reject_gen_stale > 0;
    items.push(J1Item {
        code: "J1-2b".into(),
        pass: b2,
        note: format!(
            "重复回收拒绝账实运转：DOUBLE_FREE {} / GEN_STALE {} / OWNER_MISMATCH {} / 非法迁移 {} / 越界 {}",
            bal.reject_double_free, bal.reject_gen_stale,
            bal.reject_owner_mismatch, bal.reject_state_illegal, bal.reject_oob
        ),
    });
    let b3 = bal.broken_marks > 0 && bal.repairs >= bal.broken_marks && bal.final_per_state[SlotState::Broken.index()] == 0;
    items.push(J1Item {
        code: "J1-2c".into(),
        pass: b3,
        note: format!(
            "坏槽健康账：隔离 {}（含强制 {}）修复 {}，终态 Broken 槽 {}（隔离不复分）",
            bal.broken_marks, bal.forced_isolations, bal.repairs,
            bal.final_per_state[SlotState::Broken.index()]
        ),
    });

    // —— J1-3：优先级纪律（tick < 超时 < 高精 + 预留位） ——
    let d1 = conc.discipline_inversions == 0;
    items.push(J1Item {
        code: "J1-3a".into(),
        pass: d1,
        note: format!(
            "服务序按 (rank, req_id) 升序分批：违例 {}（须零）",
            conc.discipline_inversions
        ),
    });
    let d2 = conc.reserve_blocks > 0 && conc.reserve_redeems > 0;
    items.push(J1Item {
        code: "J1-3b".into(),
        pass: d2,
        note: format!(
            "预留位纪律：Reserved 挡下 tick/超时 {} 次，高精预留兑现 {} 次（授予面恒在 Reserved 之外）",
            conc.reserve_blocks, conc.reserve_redeems
        ),
    });
    let d3 = conc.downgrades > 0;
    items.push(J1Item {
        code: "J1-3c".into(),
        pass: d3,
        note: format!(
            "高精降级立案：{} 案（实授高于全池首槽即立案，滚动案卷审可溯）",
            conc.downgrades
        ),
    });

    let mut pass = true;
    for it in items.iter() {
        if !it.pass {
            pass = false;
        }
    }

    // kv 总导出：并发池终态 + 报告面
    let mut kv: Vec<(String, String)> = Vec::new();
    kv.push(("unx1024.j1.pass".into(), if pass { "true".into() } else { "false".into() }));
    kv.push(("unx1024.j1.conc.double_occupy".into(), conc.double_occupy_hits.to_string()));
    kv.push(("unx1024.j1.bal.leaked".into(), bal.leaked.to_string()));
    kv.push(("unx1024.j1.disc.inversions".into(), conc.discipline_inversions.to_string()));
    kv.push(("unx1024.j1.bal.frag_per_mille".into(), bal.frag_per_mille.to_string()));

    Ok(J1Evidence { items, pass, kv })
}
