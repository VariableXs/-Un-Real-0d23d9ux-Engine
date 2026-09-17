//! 三方配额服务（任务56 · AI-K）——CPU 分配矩阵 / 内存水位回收 / GPU 通道抽象。
//!
//! 总案 §阶段4 施工步骤 1~3：
//! 1. **CPU 配额服务**：三方（Variable 前台 Front / 引擎 Engine / Wine）分配矩阵
//!    + 权重 + 保底（min_permil 份额下限）。对接方式＝**只写调度器既有 slice 字段
//!    （slice_total/slice_left）**：engine::on_tick 按 slice 消费、pick 换出时按
//!    slice_total 重置，同优先级 RR 每轮份额 ∝ slice_total——调制份额走的就是
//!    tick 机制本身，不存在旁路通道。
//! 2. **内存水位与压力回收**：三档（Normal/Pressure/Critical）滞回水位（升档即时、
//!    降档需越过阈值+margin）→ 分级回收（级1 ramcache 裁剪 → 级2 引擎页 →
//!    级3 Wine 前缀缓存）→ 压力解除按逆序归还。危急时用户提示属 UI 侧
//!    （总案步骤 4 设置页/通知），内核侧只出水位事件与回收动作。
//! 3. **GPU 通道分层**：`GpuChannel` trait 冻结（reserve/release/submit 记账 +
//!    三方时间片预留），当前唯一实现 `SoftGpuChannel`＝**软件渲染兜底**（双落点
//!    声明：此处 + docs/双域-任务59-GPU-Vulkan立项评估报告-2026-09-17.md）。
//!    未来 Vulkan 后端实现同一 trait 即插，软件渲染路径零改动。
//!
//! 保底语义两层：份额层＝min_permil（本模块，单核时间片模型下的可验证保证）；
//! 物理核层＝`sched::isolate_for`（Isolated 类核预留，关键线程可叠加使用）。
//!
//! 分配策略文档：docs/双域-任务56-配额分配策略-2026-09-17.md（为什么不这样分）。

use crate::sched::engine::{ThreadState, ThreadTable, MAX_THREADS};

pub const PERMIL: u32 = 1000;
/// 基准时间片（engine 的 DEFAULT_SLICE_TICKS，10 tick）。
pub const QUOTA_BASE_SLICE: u32 = crate::sched::engine::DEFAULT_SLICE_TICKS;
/// 目标份额→时间片换算放大系数：party 总片 = target_permil × SLICE_SCALE / 1000。
/// 取 4×基准：默认矩阵（500/350/150‰）→ 20/14/6 tick，粒度 25‰/tick，兼顾
/// 交互延迟（单轮上限仍受 MAX_SLICE_CAP 约束）与换算精度。
pub const SLICE_SCALE: u32 = QUOTA_BASE_SLICE * 4;
/// 单线程单轮时间片安全上限（交互延迟界：任何线程单轮 ≤ 80 tick）。
pub const MAX_SLICE_CAP: u32 = QUOTA_BASE_SLICE * 8;

// ---------------------------------------------------------------------------
// 三方与配额矩阵
// ---------------------------------------------------------------------------

/// 双域三方。Front＝Variable 前台（人看得到的），Engine＝隐形 Windows 引擎，
/// Wine＝兼容层进程。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Party {
    Front = 0,
    Engine = 1,
    Wine = 2,
}

pub const PARTY_COUNT: usize = 3;
/// assign 表里的「未指派」哨兵（未指派线程跟随 default_party）。
pub const NO_PARTY: u8 = u8::MAX;

impl Party {
    pub const ALL: [Party; PARTY_COUNT] = [Party::Front, Party::Engine, Party::Wine];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Party::Front => "front",
            Party::Engine => "engine",
            Party::Wine => "wine",
        }
    }

    pub fn from_index(i: u8) -> Option<Party> {
        Party::ALL.iter().copied().find(|p| p.index() == i as usize)
    }
}

/// 单方配额：min_permil＝保底份额（千分比，Σmin ≤ 1000）；weight＝超出保底
/// 部分的分配权重（比例保底求解用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartyQuota {
    pub min_permil: u32,
    pub weight: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuotaError {
    /// Σmin > 1000：保底承诺超出总量，矩阵无效。
    SumExceeds,
    /// weight 必须 ≥ 1（0 权重会让比例求解退化）。
    BadWeight,
    /// min_permil > 1000。
    BadPermil,
    /// tid 超出 MAX_THREADS。
    BadTid,
}

/// CPU 三方分配矩阵。默认：Front 400‰/w5、Engine 350‰/w3、Wine 150‰/w1
/// （论证见分配策略文档）。openness：`assign_tid` 提供 tid 级（≈进程级）覆写，
/// `set_quota` 允许运行时重配矩阵（校验 Σmin ≤ 1000）。
pub struct CpuQuota {
    quotas: [PartyQuota; PARTY_COUNT],
    /// tid → Party（NO_PARTY = 跟随 default_party）。
    assign: [u8; MAX_THREADS],
    default_party: Party,
}

impl CpuQuota {
    pub const fn new() -> CpuQuota {
        CpuQuota {
            quotas: [
                PartyQuota { min_permil: 400, weight: 5 },
                PartyQuota { min_permil: 350, weight: 3 },
                PartyQuota { min_permil: 150, weight: 1 },
            ],
            assign: [NO_PARTY; MAX_THREADS],
            default_party: Party::Front,
        }
    }

    /// 重配一方配额；Σmin（含他方现存值）不得超过 1000。
    pub fn set_quota(&mut self, p: Party, min_permil: u32, weight: u32) -> Result<(), QuotaError> {
        if min_permil > PERMIL {
            return Err(QuotaError::BadPermil);
        }
        if weight == 0 {
            return Err(QuotaError::BadWeight);
        }
        let others: u32 = (0..PARTY_COUNT)
            .filter(|&i| i != p.index())
            .map(|i| self.quotas[i].min_permil)
            .sum();
        if others.saturating_add(min_permil) > PERMIL {
            return Err(QuotaError::SumExceeds);
        }
        self.quotas[p.index()] = PartyQuota { min_permil, weight };
        Ok(())
    }

    pub fn quota(&self, p: Party) -> PartyQuota {
        self.quotas[p.index()]
    }

    /// tid 级覆写（总案 openness：「矩阵进程级可覆写」的落地入口）。
    pub fn assign_tid(&mut self, tid: u32, p: Party) -> Result<(), QuotaError> {
        if tid as usize >= MAX_THREADS {
            return Err(QuotaError::BadTid);
        }
        self.assign[tid as usize] = p as u8;
        Ok(())
    }

    /// 撤销覆写，回到 default_party。
    pub fn clear_tid(&mut self, tid: u32) -> Result<(), QuotaError> {
        if tid as usize >= MAX_THREADS {
            return Err(QuotaError::BadTid);
        }
        self.assign[tid as usize] = NO_PARTY;
        Ok(())
    }

    pub fn set_default_party(&mut self, p: Party) {
        self.default_party = p;
    }

    pub fn party_of(&self, tid: u32) -> Party {
        match tid as usize >= MAX_THREADS {
            true => self.default_party,
            false => match Party::from_index(self.assign[tid as usize]) {
                Some(p) => p,
                None => self.default_party,
            },
        }
    }

    /// 比例保底求解（proportional share with minimums）：
    /// 1. 自然份额 s_i = w_i / Σw × 1000；
    /// 2. 低于自身 min 的一方先落定为 min（fixed）；
    /// 3. 其余各方在剩余容量（1000 − Σfixed_min）内按权重再分；重复至收敛
 ///    （最多 3 轮，每轮至多固化一方）。
    /// 结果 Σ ≤ 1000 且每方 ≥ min（构造保证：set_quota 已限 Σmin ≤ 1000）。
    pub fn targets(&self) -> [u32; PARTY_COUNT] {
        let q = &self.quotas;
        let wsum: u32 = q.iter().map(|x| x.weight).sum();
        let mut target = [0u32; PARTY_COUNT];
        if wsum == 0 {
            // set_quota 禁 weight=0，此处纯兜底：退化均分。
            return [333, 333, 334];
        }
        let mut fixed = [false; PARTY_COUNT];
        loop {
            let fixed_min: u32 = (0..PARTY_COUNT)
                .filter(|&i| fixed[i])
                .map(|i| q[i].min_permil)
                .sum();
            let rem_cap = PERMIL.saturating_sub(fixed_min);
            let rem_w: u32 = (0..PARTY_COUNT)
                .filter(|&i| !fixed[i])
                .map(|i| q[i].weight)
                .sum();
            for i in 0..PARTY_COUNT {
                if !fixed[i] {
                    target[i] = if rem_w == 0 { 0 } else { rem_cap * q[i].weight / rem_w };
                }
            }
            match (0..PARTY_COUNT).find(|&i| !fixed[i] && target[i] < q[i].min_permil) {
                Some(i) => fixed[i] = true,
                None => break,
            }
        }
        for i in 0..PARTY_COUNT {
            if fixed[i] {
                target[i] = q[i].min_permil;
            }
        }
        target
    }

    /// 某方 m 个活动线程时，单线程单轮时间片。
    /// party 总片 = target_permil × SLICE_SCALE / 1000，按成员均分（≥1），
    /// 夹在 [1, MAX_SLICE_CAP]。
    pub fn party_slice(&self, p: Party, members: u32) -> u32 {
        let target = self.targets()[p.index()];
        let total = target * SLICE_SCALE / PERMIL;
        let per = (total / members.max(1)).min(MAX_SLICE_CAP);
        per.max(1)
    }

    /// 把矩阵应用到线程表——**只写既有 slice 字段**（slice_total/slice_left），
    /// 由 engine::on_tick / pick 的既有语义消费：同优先级 RR 每轮份额 ∝
    /// slice_total。不触碰队列、类、优先级，不存在旁路调度路径。
    /// （RealTime/Isolated 类本就不吃 slice，写入无害。）
    pub fn apply_to_table(&self, table: &mut ThreadTable) {
        // 先按 party 清点活动线程数（份额按成员均分）。
        let mut members = [0u32; PARTY_COUNT];
        for i in 0..MAX_THREADS {
            let t = &table.threads()[i];
            if t.state != ThreadState::Unused {
                members[self.party_of(t.tid).index()] += 1;
            }
        }
        for i in 0..MAX_THREADS {
            let tid = i as u32;
            let (party, m) = {
                let t = &table.threads()[i];
                if t.state == ThreadState::Unused {
                    continue;
                }
                (self.party_of(tid), members[self.party_of(tid).index()])
            };
            let slice = self.party_slice(party, m);
            if let Some(t) = table.thread_mut(tid) {
                t.slice_total = slice;
                // 已在途的片只许缩短不许膨胀：新目标即刻生效。
                t.slice_left = t.slice_left.min(slice);
            }
        }
    }
}

impl Default for CpuQuota {
    fn default() -> CpuQuota {
        CpuQuota::new()
    }
}

// ---------------------------------------------------------------------------
// 内存水位与分级回收
// ---------------------------------------------------------------------------

/// 水位三档（总案步骤 2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WatermarkLevel {
    Normal = 0,
    Pressure = 1,
    Critical = 2,
}

impl WatermarkLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            WatermarkLevel::Normal => "normal",
            WatermarkLevel::Pressure => "pressure",
            WatermarkLevel::Critical => "critical",
        }
    }
}

/// 回收级（顺序即管线序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReclaimStageId {
    RamCache = 0,
    EnginePages = 1,
    WineCache = 2,
}

impl ReclaimStageId {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ReclaimStageId::RamCache => "ram",
            ReclaimStageId::EnginePages => "engine",
            ReclaimStageId::WineCache => "wine",
        }
    }
}

/// free/total ≤ 此值 → Pressure。
pub const PRESSURE_AT_PERMIL: u32 = 250;
/// free/total ≤ 此值 → Critical。
pub const CRITICAL_AT_PERMIL: u32 = 100;
/// 降档滞回带：回升需越过「上一档阈值 + margin」才降档，防抖动。
pub const HYSTERESIS_MARGIN_PERMIL: u32 = 50;

/// 回收管线的一级。回收＝借走（压力期占用），归还＝压力解除后原路退回。
/// trait 即总案「冻结」契约：只含 id/标识 + reclaim/restore 两个动作，
/// 未来接真 ramcache/Engine/Wine 实现时不得增改方法（只允许新增独立 trait）。
pub trait ReclaimStage {
    fn id(&self) -> ReclaimStageId;
    fn as_str(&self) -> &'static str;
    /// 请求回收 pages 页，返回实际回收数（可以不足——容量/策略自决）。
    fn reclaim(&mut self, pages: u64) -> u64;
    /// 归还 pages 页，返回实际归还数。
    fn restore(&mut self, pages: u64) -> u64;
}

/// 内存水位服务。`owed` 记录向各级借走未还的页数（归还验证的账本）。
pub struct MemWatermark {
    level: WatermarkLevel,
    owed: [u64; 3],
    transitions: u32,
    reclaimed_pages: u64,
    restored_pages: u64,
}

impl MemWatermark {
    pub const fn new() -> MemWatermark {
        MemWatermark {
            level: WatermarkLevel::Normal,
            owed: [0; 3],
            transitions: 0,
            reclaimed_pages: 0,
            restored_pages: 0,
        }
    }

    pub fn level(&self) -> WatermarkLevel {
        self.level
    }

    pub fn owed(&self, id: ReclaimStageId) -> u64 {
        self.owed[id.index()]
    }

    pub fn transitions(&self) -> u32 {
        self.transitions
    }

    pub fn reclaimed_pages(&self) -> u64 {
        self.reclaimed_pages
    }

    pub fn restored_pages(&self) -> u64 {
        self.restored_pages
    }

    /// 目标水位：把 free 拉出滞回带（Pressure/Critical 同一目标，差异只在
    /// 允许动用几级回收）。
    fn target_free(&self, total_pages: u64) -> u64 {
        total_pages * (PRESSURE_AT_PERMIL + HYSTERESIS_MARGIN_PERMIL) as u64 / PERMIL as u64
    }

    /// 水位评估（滞回：升档即时；降档需 ≥ PRESSURE_AT + MARGIN；滞回带内维持）。
    pub fn evaluate(&mut self, free_pages: u64, total_pages: u64) -> WatermarkLevel {
        let ratio = if total_pages == 0 {
            PERMIL
        } else {
            ((free_pages * PERMIL as u64) / total_pages).min(PERMIL as u64) as u32
        };
        let next = if ratio <= CRITICAL_AT_PERMIL {
            WatermarkLevel::Critical
        } else if ratio <= PRESSURE_AT_PERMIL {
            WatermarkLevel::Pressure
        } else if ratio >= PRESSURE_AT_PERMIL + HYSTERESIS_MARGIN_PERMIL {
            WatermarkLevel::Normal
        } else {
            self.level
        };
        if next != self.level {
            self.transitions += 1;
        }
        self.level = next;
        next
    }

    /// 水位服务主入口：评估 → 按档动作。
    /// - Normal：逆序归还（Wine → Engine → RamCache）全部欠账；
    /// - Pressure：只级1（ramcache 裁剪），目标拉出滞回带；
    /// - Critical：级1→级2→级3 顺序回收，目标拉出滞回带。
    /// 调用方按管线序传入 stages（[RamCache, EnginePages, WineCache]）；
    /// 回收产生的 free 增量由调用方回账后再次调用（幂等收敛）。
    pub fn service(
        &mut self,
        free_pages: u64,
        total_pages: u64,
        stages: &mut [&mut dyn ReclaimStage],
    ) -> WatermarkLevel {
        let lv = self.evaluate(free_pages, total_pages);
        match lv {
            WatermarkLevel::Normal => {
                for i in (0..stages.len()).rev() {
                    let id = stages[i].id().index();
                    let owed = self.owed[id];
                    if owed > 0 {
                        let got = stages[i].restore(owed);
                        self.owed[id] -= got;
                        self.restored_pages += got;
                    }
                }
            }
            WatermarkLevel::Pressure => {
                let need = self.target_free(total_pages).saturating_sub(free_pages);
                if need > 0 {
                    if let Some(s0) = stages.first_mut() {
                        let id = s0.id().index();
                        let got = s0.reclaim(need);
                        self.owed[id] += got;
                        self.reclaimed_pages += got;
                    }
                }
            }
            WatermarkLevel::Critical => {
                let need = self.target_free(total_pages).saturating_sub(free_pages);
                let mut left = need;
                for stage in stages.iter_mut() {
                    if left == 0 {
                        break;
                    }
                    let id = stage.id().index();
                    let got = stage.reclaim(left);
                    self.owed[id] += got;
                    self.reclaimed_pages += got;
                    left -= got;
                }
            }
        }
        lv
    }
}

impl Default for MemWatermark {
    fn default() -> MemWatermark {
        MemWatermark::new()
    }
}

// ---------------------------------------------------------------------------
// GPU 通道抽象（trait 冻结）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpuError {
    /// Σ 三方预留 > 1000‰。
    Overcommit,
    /// permil > 1000。
    BadPermil,
}

/// GPU 通道抽象——总案步骤 3「trait 冻结」契约：
/// reserve/release/submit(记账) + reserved/submitted(读数) + kind(后端标识)。
/// 未来 Vulkan 后端实现本 trait 即插；**不得增改方法**（扩展走新 trait）。
pub trait GpuChannel {
    /// 后端标识：当前唯一实现为 "soft"（软件渲染兜底）。
    fn kind(&self) -> &'static str;
    /// 三方时间片预留（千分比），Σ ≤ 1000。
    fn reserve(&mut self, party: Party, permil: u32) -> Result<(), GpuError>;
    /// 撤销预留，返回被撤销的量。
    fn release(&mut self, party: Party) -> u32;
    fn reserved(&self, party: Party) -> u32;
    fn total_reserved(&self) -> u32;
    /// 提交记账（单位由后端自定义：软件渲染=帧，Vulkan=命令缓冲）。
    fn submit(&mut self, party: Party, units: u64);
    fn submitted(&self, party: Party) -> u64;
}

/// 软件渲染兜底实现（当前唯一后端）。纯记账：预留校验 + 提交计数，
/// 不干预既有软件渲染路径（回归不破的根源＝它根本不在渲染路径上）。
#[derive(Debug)]
pub struct SoftGpuChannel {
    reserved: [u32; PARTY_COUNT],
    submitted: [u64; PARTY_COUNT],
}

impl SoftGpuChannel {
    pub const fn new() -> SoftGpuChannel {
        SoftGpuChannel { reserved: [0; PARTY_COUNT], submitted: [0; PARTY_COUNT] }
    }
}

impl Default for SoftGpuChannel {
    fn default() -> SoftGpuChannel {
        SoftGpuChannel::new()
    }
}

impl GpuChannel for SoftGpuChannel {
    fn kind(&self) -> &'static str {
        "soft"
    }

    fn reserve(&mut self, party: Party, permil: u32) -> Result<(), GpuError> {
        if permil > PERMIL {
            return Err(GpuError::BadPermil);
        }
        let others: u32 = (0..PARTY_COUNT)
            .filter(|&i| i != party.index())
            .map(|i| self.reserved[i])
            .sum();
        if others.saturating_add(permil) > PERMIL {
            return Err(GpuError::Overcommit);
        }
        self.reserved[party.index()] = permil;
        Ok(())
    }

    fn release(&mut self, party: Party) -> u32 {
        core::mem::replace(&mut self.reserved[party.index()], 0)
    }

    fn reserved(&self, party: Party) -> u32 {
        self.reserved[party.index()]
    }

    fn total_reserved(&self) -> u32 {
        self.reserved.iter().sum()
    }

    fn submit(&mut self, party: Party, units: u64) {
        self.submitted[party.index()] += units;
    }

    fn submitted(&self, party: Party) -> u64 {
        self.submitted[party.index()]
    }
}

// ---------------------------------------------------------------------------
// 探针（target）
// ---------------------------------------------------------------------------

/// 内核启动链实机探针：纯计算（局部线程表 + mock 管线 + 软通道），
/// 不碰全局 SCHED、不依赖块设备。返回全绿与否；逐行 kinfo 留串口证据。
pub mod target {
    use super::*;
    use crate::sched::engine::{PRIO_NORMAL, SchedClass};

    /// 回收级 mock（宿主测试与内核探针共用）。
    pub(crate) struct MockStage {
        id: ReclaimStageId,
        cap: u64,
        given: u64,
        reclaim_calls: u32,
        restored: u64,
    }

    impl MockStage {
        pub(crate) fn new(id: ReclaimStageId, cap: u64) -> MockStage {
            MockStage { id, cap, given: 0, reclaim_calls: 0, restored: 0 }
        }

        pub(crate) fn given(&self) -> u64 {
            self.given
        }

        pub(crate) fn reclaim_calls(&self) -> u32 {
            self.reclaim_calls
        }

        pub(crate) fn restored(&self) -> u64 {
            self.restored
        }
    }

    impl ReclaimStage for MockStage {
        fn id(&self) -> ReclaimStageId {
            self.id
        }

        fn as_str(&self) -> &'static str {
            self.id.as_str()
        }

        fn reclaim(&mut self, pages: u64) -> u64 {
            self.reclaim_calls += 1;
            let got = pages.min(self.cap - self.given);
            self.given += got;
            got
        }

        fn restore(&mut self, pages: u64) -> u64 {
            let got = pages.min(self.given);
            self.given -= got;
            self.restored += got;
            got
        }
    }

    fn party_share_permil(table: &ThreadTable, tids: &[u32]) -> u64 {
        let sum: u64 = tids.iter().map(|t| table.threads()[*t as usize].runtime_ticks).sum();
        sum
    }

    // 探针专用线程表放 .bss（const 初始化）：ThreadTable ≈115KB，绝不能在引导栈上
    // 物化——`Box::new(ThreadTable::new())` 的栈中转同样会撑爆 64KB 引导栈
    // （实机静默崩溃坑；宿主测试栈 8MB 不受影响，仍可 Box）。沿 sched::SCHED 同款
    // static 模式，A/B 各承载一步避免跨步重置。
    static PROBE_TABLE_A: crate::cpu::sync::SpinProtected<ThreadTable> =
        crate::cpu::sync::SpinProtected::new(ThreadTable::new());
    static PROBE_TABLE_B: crate::cpu::sync::SpinProtected<ThreadTable> =
        crate::cpu::sync::SpinProtected::new(ThreadTable::new());

    /// 探针主体。四步：①矩阵饱和份额+保底 ②单方独占压测 ③水位演练
    /// ④GPU 通道预留/记账。
    pub fn quota_probe() -> bool {
        let saved_current = crate::sched::engine::current_tid();
        let mut all_ok = true;

        // ① 三方全占满：份额 ≈ targets（±80‰），每方 ≥ min − 80‰（保底不丢）。
        let mut quota = CpuQuota::new();
        quota.assign_tid(11, Party::Engine).unwrap();
        quota.assign_tid(12, Party::Wine).unwrap();
        let targets = quota.targets();
        let floors = [
            quota.quota(Party::Front).min_permil,
            quota.quota(Party::Engine).min_permil,
            quota.quota(Party::Wine).min_permil,
        ];
        let step1 = {
            let mut table = PROBE_TABLE_A.lock();
            for tid in [10u32, 11, 12] {
                table.spawn_on(tid, PRIO_NORMAL, SchedClass::Interactive, 0, 0);
            }
            quota.apply_to_table(&mut table);
            for _ in 0..480 {
                let _ = table.on_tick(0);
            }
            let rt = [
                party_share_permil(&table, &[10]),
                party_share_permil(&table, &[11]),
                party_share_permil(&table, &[12]),
            ];
            let total: u64 = rt[0] + rt[1] + rt[2];
            let shares = [
                rt[0] * 1000 / total.max(1),
                rt[1] * 1000 / total.max(1),
                rt[2] * 1000 / total.max(1),
            ];
            let mut ok = true;
            for i in 0..PARTY_COUNT {
                if shares[i] + 80 < floors[i] as u64 || shares[i] > targets[i] as u64 + 80 {
                    ok = false;
                }
            }
            crate::kinfo!(
                "quota: matrix shares front={}/1000 engine={}/1000 wine={}/1000 \
                 targets={}/{}/{} verdict={}",
                shares[0], shares[1], shares[2],
                targets[0], targets[1], targets[2],
                if ok { "ok" } else { "FAIL" }
            );
            ok
        };
        all_ok &= step1;

        // ② 单方独占压测：Wine×2 抢跑，Front/Engine 交互不卡顿
        //    （最大连续等待 ≤ 40 tick）且份额仍达保底。
        let step2 = {
            let mut table = PROBE_TABLE_B.lock();
            for tid in [20u32, 21, 22, 23] {
                table.spawn_on(tid, PRIO_NORMAL, SchedClass::Interactive, 0, 0);
            }
            let mut q = CpuQuota::new();
            let _ = q.assign_tid(20, Party::Wine);
            let _ = q.assign_tid(21, Party::Wine);
            let _ = q.assign_tid(22, Party::Front);
            let _ = q.assign_tid(23, Party::Engine);
            q.apply_to_table(&mut table);
            let mut prev = [0u64; 24];
            let mut last_run = [0usize; 24];
            let mut max_gap = [0usize; 24];
            for tick in 1..=480usize {
                let _ = table.on_tick(0);
                for tid in [20u32, 21, 22, 23] {
                    let now = table.threads()[tid as usize].runtime_ticks;
                    if now > prev[tid as usize] {
                        let gap = tick - last_run[tid as usize];
                        if gap > max_gap[tid as usize] {
                            max_gap[tid as usize] = gap;
                        }
                        last_run[tid as usize] = tick;
                        prev[tid as usize] = now;
                    }
                }
            }
            let f = party_share_permil(&table, &[22]);
            let e = party_share_permil(&table, &[23]);
            let w = party_share_permil(&table, &[20, 21]);
            let total = f + e + w;
            let (fs, es, ws) = (f * 1000 / total.max(1), e * 1000 / total.max(1), w * 1000 / total.max(1));
            let gap_f = max_gap[22];
            let gap_e = max_gap[23];
            let ok = gap_f <= 40 && gap_e <= 40 && fs + 80 >= 400 && es + 80 >= 350;
            crate::kinfo!(
                "quota: monopoly front={}/1000 engine={}/1000 wine={}/1000 \
                 max-gap front={} engine={} (bound 40) verdict={}",
                fs, es, ws, gap_f, gap_e,
                if ok { "ok" } else { "FAIL" }
            );
            ok
        };
        all_ok &= step2;

        // ③ 水位演练：Pressure 只级1；Critical 级1→级2→3 顺序；回升逆序归还。
        let step3 = {
            let mut ram = MockStage::new(ReclaimStageId::RamCache, 150);
            let mut engine = MockStage::new(ReclaimStageId::EnginePages, 100);
            let mut wine = MockStage::new(ReclaimStageId::WineCache, 100);
            let mut wm = MemWatermark::new();
            let mut ok = true;
            let reclaimed_before_restore;
            {
                let mut stages: [&mut dyn ReclaimStage; 3] = [&mut ram, &mut engine, &mut wine];
                // Normal：不动。
                ok &= wm.service(500, 1000, &mut stages[..]) == WatermarkLevel::Normal
                    && wm.owed(ReclaimStageId::RamCache) == 0;
                // Pressure（200‰）：只级1 动（need = 300−200 = 100）。
                ok &= wm.service(200, 1000, &mut stages[..]) == WatermarkLevel::Pressure
                    && wm.owed(ReclaimStageId::RamCache) == 100
                    && wm.owed(ReclaimStageId::EnginePages) == 0
                    && wm.owed(ReclaimStageId::WineCache) == 0;
                // Critical（80‰）：need = 300−80 = 220 → 级1 剩 50 → 级2 100 → 级3 70。
                // 份额算术唯一确定顺序：级3 拿 70（非 100）当且仅当级2 先于它拿满 100。
                ok &= wm.service(80, 1000, &mut stages[..]) == WatermarkLevel::Critical
                    && wm.owed(ReclaimStageId::RamCache) == 150
                    && wm.owed(ReclaimStageId::EnginePages) == 100
                    && wm.owed(ReclaimStageId::WineCache) == 70;
                reclaimed_before_restore = wm.reclaimed_pages();
                // 回升（400‰ ≥ 300）：逆序归还，欠账清零，归还量 = 回收量。
                ok &= wm.service(400, 1000, &mut stages[..]) == WatermarkLevel::Normal
                    && wm.owed(ReclaimStageId::RamCache) == 0
                    && wm.owed(ReclaimStageId::EnginePages) == 0
                    && wm.owed(ReclaimStageId::WineCache) == 0
                    && wm.restored_pages() == reclaimed_before_restore;
            }
            // mock 直查（trait 对象借用已结束）：调用次数证明 Pressure 只动级1、
            // Critical 三级各一次；given 清零 + 归还量逐级对账。
            ok &= ram.reclaim_calls() == 2
                && engine.reclaim_calls() == 1
                && wine.reclaim_calls() == 1
                && ram.given() == 0
                && engine.given() == 0
                && wine.given() == 0
                && ram.restored() == 150
                && engine.restored() == 100
                && wine.restored() == 70;
            crate::kinfo!(
                "quota: watermark drill normal/pressure/critical/normal \
                 reclaimed={} restored={} order=[ram,engine,wine] verdict={}",
                reclaimed_before_restore,
                wm.restored_pages(),
                if ok { "ok" } else { "FAIL" }
            );

            // 滞回带：280‰ 维持 Normal；240‰ 升 Pressure；270‰ 带内维持；
            // 310‰ 才降 Normal。
            let mut wm2 = MemWatermark::new();
            let ok2 = wm2.evaluate(280, 1000) == WatermarkLevel::Normal
                && wm2.evaluate(240, 1000) == WatermarkLevel::Pressure
                && wm2.evaluate(270, 1000) == WatermarkLevel::Pressure
                && wm2.evaluate(310, 1000) == WatermarkLevel::Normal;
            crate::kinfo!(
                "quota: hysteresis 280-hold/240-up/270-hold/310-down verdict={}",
                if ok2 { "ok" } else { "FAIL" }
            );
            ok &= ok2;
            ok
        };
        all_ok &= step3;

        // ④ GPU 通道：三方预留 Σ≤1000；超额明确拒绝；记账计数。
        let step4 = {
            let mut gpu = SoftGpuChannel::new();
            let r1 = gpu.reserve(Party::Front, 400);
            let r2 = gpu.reserve(Party::Engine, 350);
            let r3 = gpu.reserve(Party::Wine, 150);
            let over = gpu.reserve(Party::Wine, 400);
            gpu.submit(Party::Front, 60);
            gpu.submit(Party::Front, 30);
            gpu.submit(Party::Engine, 10);
            let mut ok = r1.is_ok()
                && r2.is_ok()
                && r3.is_ok()
                && over == Err(GpuError::Overcommit)
                && gpu.total_reserved() == 900
                && gpu.reserved(Party::Front) == 400
                && gpu.submitted(Party::Front) == 90
                && gpu.submitted(Party::Engine) == 10
                && gpu.kind() == "soft";
            let _ = gpu.release(Party::Wine);
            ok &= gpu.total_reserved() == 750;
            crate::kinfo!(
                "quota: gpu soft-channel reserve 400/350/150 overcommit-rejected \
                 submit=90/10/0 verdict={}",
                if ok { "ok" } else { "FAIL" }
            );
            ok
        };
        all_ok &= step4;

        // 恢复内核级 current 视图（探针 pick 会改写 engine 全局 current）。
        crate::sched::engine::set_current(saved_current);
        if all_ok {
            crate::kinfo!("QUOTA PROBE PASS");
        } else {
            crate::kinfo!("quota: probe verdict=FAIL");
        }
        all_ok
    }
}

// ---------------------------------------------------------------------------
// 宿主测试（cargo ktest）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::target::MockStage;
    use crate::sched::engine::{PRIO_NORMAL, SchedClass};

    fn spawn3(table: &mut ThreadTable, tids: [u32; 3]) {
        for tid in tids {
            assert!(table.spawn_on(tid, PRIO_NORMAL, SchedClass::Interactive, 0, 0));
        }
    }

    #[test]
    fn quota_targets_default_floor_solve() {
        let q = CpuQuota::new();
        // 自然份额 556/333/111：Engine/Wine 低于保底 → 固化；Front 吃剩余容量。
        assert_eq!(q.targets(), [500, 350, 150]);
        assert!(q.targets()[0] >= 400 && q.targets()[1] >= 350 && q.targets()[2] >= 150);
    }

    #[test]
    fn quota_targets_all_mins_exact() {
        let mut q = CpuQuota::new();
        // Σmin = 1000：全部固化，targets = mins（权重不再起作用）。
        q.set_quota(Party::Front, 400, 9).unwrap();
        q.set_quota(Party::Engine, 400, 1).unwrap();
        q.set_quota(Party::Wine, 200, 1).unwrap();
        assert_eq!(q.targets(), [400, 400, 200]);
    }

    #[test]
    fn quota_set_quota_validation() {
        let mut q = CpuQuota::new();
        // Σmin 超 1000。
        assert_eq!(q.set_quota(Party::Front, 800, 5), Err(QuotaError::SumExceeds));
        // weight 0。
        assert_eq!(q.set_quota(Party::Front, 100, 0), Err(QuotaError::BadWeight));
        // permil 超 1000。
        assert_eq!(q.set_quota(Party::Front, 1001, 5), Err(QuotaError::BadPermil));
        // 合法改动生效。
        q.set_quota(Party::Front, 500, 5).unwrap();
        assert_eq!(q.quota(Party::Front).min_permil, 500);
    }

    #[test]
    fn quota_assign_override_and_clear() {
        let mut q = CpuQuota::new();
        let mut table = Box::new(ThreadTable::new());
        assert!(table.spawn_on(5, PRIO_NORMAL, SchedClass::Interactive, 0, 0));
        // 未指派 → default_party（Front）。
        assert_eq!(q.party_of(5), Party::Front);
        // 覆写 → Wine；apply 后片 = Wine 目标换算。
        q.assign_tid(5, Party::Wine).unwrap();
        assert_eq!(q.party_of(5), Party::Wine);
        q.apply_to_table(&mut table);
        let expected = (150 * SLICE_SCALE / PERMIL).clamp(1, MAX_SLICE_CAP);
        assert_eq!(table.thread(5).unwrap().slice_total, expected);
        // 撤销覆写 → 回 default。
        q.clear_tid(5).unwrap();
        assert_eq!(q.party_of(5), Party::Front);
        // tid 越界。
        assert_eq!(q.assign_tid(MAX_THREADS as u32, Party::Wine), Err(QuotaError::BadTid));
    }

    #[test]
    fn quota_saturated_shares_hold_floor() {
        let quota = CpuQuota::new();
        let mut table = Box::new(ThreadTable::new());
        spawn3(&mut table, [10, 11, 12]);
        let mut quota = quota; // 移动为可变绑定供指派
        quota.assign_tid(11, Party::Engine).unwrap();
        quota.assign_tid(12, Party::Wine).unwrap();
        quota.apply_to_table(&mut table);
        // 份额换算抽查：party_slice(Front,1)=20（500‰×40/1000）。
        assert_eq!(table.thread(10).unwrap().slice_total, 20);
        assert_eq!(table.thread(11).unwrap().slice_total, 14);
        assert_eq!(table.thread(12).unwrap().slice_total, 6);
        for _ in 0..480 {
            let _ = table.on_tick(0);
        }
        let rt = [
            table.threads()[10].runtime_ticks,
            table.threads()[11].runtime_ticks,
            table.threads()[12].runtime_ticks,
        ];
        let total = rt[0] + rt[1] + rt[2];
        let shares = [rt[0] * 1000 / total, rt[1] * 1000 / total, rt[2] * 1000 / total];
        for (i, min) in [400u32, 350, 150].iter().enumerate() {
            assert!(
                shares[i] + 80 >= *min as u64,
                "party {} share {}/1000 below floor {}",
                i,
                shares[i],
                min
            );
            assert!(
                shares[i] <= quota.targets()[i] as u64 + 80,
                "party {} share {}/1000 above target+80 ({})",
                i,
                shares[i],
                quota.targets()[i]
            );
        }
    }

    #[test]
    fn quota_monopoly_others_not_starved() {
        let mut q = CpuQuota::new();
        let mut table = Box::new(ThreadTable::new());
        for tid in [20u32, 21, 22, 23] {
            assert!(table.spawn_on(tid, PRIO_NORMAL, SchedClass::Interactive, 0, 0));
        }
        q.assign_tid(20, Party::Wine).unwrap();
        q.assign_tid(21, Party::Wine).unwrap();
        q.assign_tid(22, Party::Front).unwrap();
        q.assign_tid(23, Party::Engine).unwrap();
        q.apply_to_table(&mut table);
        // Wine 2 成员均分 6 → 各 3。
        assert_eq!(table.thread(20).unwrap().slice_total, 3);
        assert_eq!(table.thread(21).unwrap().slice_total, 3);
        let mut prev = [0u64; 24];
        let mut last_run = [0usize; 24];
        let mut max_gap = [0usize; 24];
        for tick in 1..=480usize {
            let _ = table.on_tick(0);
            for tid in [20u32, 21, 22, 23] {
                let now = table.threads()[tid as usize].runtime_ticks;
                if now > prev[tid as usize] {
                    let gap = tick - last_run[tid as usize];
                    max_gap[tid as usize] = max_gap[tid as usize].max(gap);
                    last_run[tid as usize] = tick;
                    prev[tid as usize] = now;
                }
            }
        }
        // 交互不卡顿：Front/Engine 最大连续等待有界。
        assert!(max_gap[22] <= 40, "front gap {}", max_gap[22]);
        assert!(max_gap[23] <= 40, "engine gap {}", max_gap[23]);
        // 份额保底仍成立。
        let f = table.threads()[22].runtime_ticks;
        let e = table.threads()[23].runtime_ticks;
        let w = table.threads()[20].runtime_ticks + table.threads()[21].runtime_ticks;
        let total = f + e + w;
        assert!(f * 1000 / total + 80 >= 400);
        assert!(e * 1000 / total + 80 >= 350);
        assert!(w * 1000 / total + 100 >= 150);
    }

    #[test]
    fn quota_multi_thread_party_split_slice() {
        let mut q = CpuQuota::new();
        let mut table = Box::new(ThreadTable::new());
        for tid in [1u32, 2, 3, 4] {
            assert!(table.spawn_on(tid, PRIO_NORMAL, SchedClass::Normal, 0, 0));
        }
        q.assign_tid(3, Party::Wine).unwrap();
        q.assign_tid(4, Party::Wine).unwrap();
        // tid 1/2 未指派 → Front（2 成员）→ 20/2 = 10。
        q.apply_to_table(&mut table);
        assert_eq!(table.thread(1).unwrap().slice_total, 10);
        assert_eq!(table.thread(3).unwrap().slice_total, 3);
        assert_eq!(table.thread(4).unwrap().slice_total, 3);
    }

    #[test]
    fn watermark_pressure_ramcache_only() {
        let mut ram = MockStage::new(ReclaimStageId::RamCache, 150);
        let mut engine = MockStage::new(ReclaimStageId::EnginePages, 100);
        let mut wine = MockStage::new(ReclaimStageId::WineCache, 100);
        let mut wm = MemWatermark::new();
        {
            let mut stages: [&mut dyn ReclaimStage; 3] = [&mut ram, &mut engine, &mut wine];
            // 正常水位：零动作。
            assert_eq!(wm.service(500, 1000, &mut stages[..]), WatermarkLevel::Normal);
            assert_eq!(wm.owed(ReclaimStageId::RamCache), 0);
            // 压力水位：只级1，且量 = 目标 − free = 300 − 200 = 100。
            assert_eq!(wm.service(200, 1000, &mut stages[..]), WatermarkLevel::Pressure);
            assert_eq!(wm.owed(ReclaimStageId::RamCache), 100);
            assert_eq!(wm.owed(ReclaimStageId::EnginePages), 0);
            assert_eq!(wm.owed(ReclaimStageId::WineCache), 0);
        }
        // mock 直查：级2/级3 零调用。
        assert_eq!(ram.given(), 100);
        assert_eq!(engine.reclaim_calls(), 0);
        assert_eq!(wine.reclaim_calls(), 0);
    }

        #[test]
    fn watermark_critical_order_and_restore() {
        let mut ram = MockStage::new(ReclaimStageId::RamCache, 150);
        let mut engine = MockStage::new(ReclaimStageId::EnginePages, 100);
        let mut wine = MockStage::new(ReclaimStageId::WineCache, 100);
        let mut wm = MemWatermark::new();
        let reclaimed_before_restore;
        {
            let mut stages: [&mut dyn ReclaimStage; 3] = [&mut ram, &mut engine, &mut wine];
            let _ = wm.service(200, 1000, &mut stages[..]); // 先压到 Pressure，级1 已给 100
            // 危急：220 缺口 → 50+100+70，算术唯一确定顺序 [ram, engine, wine]。
            assert_eq!(wm.service(80, 1000, &mut stages[..]), WatermarkLevel::Critical);
            assert_eq!(wm.owed(ReclaimStageId::RamCache), 150);
            assert_eq!(wm.owed(ReclaimStageId::EnginePages), 100);
            assert_eq!(wm.owed(ReclaimStageId::WineCache), 70);
            reclaimed_before_restore = wm.reclaimed_pages();
            // 回升归还：逆序、账目两清。
            assert_eq!(wm.service(400, 1000, &mut stages[..]), WatermarkLevel::Normal);
            assert_eq!(wm.owed(ReclaimStageId::RamCache), 0);
            assert_eq!(wm.owed(ReclaimStageId::EnginePages), 0);
            assert_eq!(wm.owed(ReclaimStageId::WineCache), 0);
        }
        assert_eq!(wm.restored_pages(), reclaimed_before_restore);
        assert_eq!(ram.restored(), 150);
        assert_eq!(engine.restored(), 100);
        assert_eq!(wine.restored(), 70);
        assert_eq!(ram.given(), 0);
        assert_eq!(engine.given(), 0);
        assert_eq!(wine.given(), 0);
    }

        #[test]
    fn watermark_hysteresis_band() {
        let mut wm = MemWatermark::new();
        // 滞回带 [250, 300)：维持原档。
        assert_eq!(wm.evaluate(280, 1000), WatermarkLevel::Normal);
        // ≤250 即升 Pressure（升档即时）。
        assert_eq!(wm.evaluate(240, 1000), WatermarkLevel::Pressure);
        // 带内维持 Pressure。
        assert_eq!(wm.evaluate(270, 1000), WatermarkLevel::Pressure);
        // ≥300 才降 Normal。
        assert_eq!(wm.evaluate(310, 1000), WatermarkLevel::Normal);
        assert_eq!(wm.transitions(), 2);
        // 极端：0 总页 → 满额比，Normal；0 free → Critical。
        let mut wm2 = MemWatermark::new();
        assert_eq!(wm2.evaluate(0, 0), WatermarkLevel::Normal);
        assert_eq!(wm2.evaluate(0, 1000), WatermarkLevel::Critical);
    }

    #[test]
    fn gpu_soft_channel_reserve_and_accounting() {
        let mut gpu = SoftGpuChannel::new();
        assert_eq!(gpu.kind(), "soft");
        assert!(gpu.reserve(Party::Front, 400).is_ok());
        assert!(gpu.reserve(Party::Engine, 350).is_ok());
        assert!(gpu.reserve(Party::Wine, 150).is_ok());
        // Σ=900，再要 400（750+400=1150>1000）→ Overcommit（拒绝且不落账）。
        assert_eq!(gpu.reserve(Party::Wine, 400), Err(GpuError::Overcommit));
        assert_eq!(gpu.reserve(Party::Front, 1001), Err(GpuError::BadPermil));
        assert_eq!(gpu.total_reserved(), 900);
        gpu.submit(Party::Front, 60);
        gpu.submit(Party::Front, 30);
        gpu.submit(Party::Engine, 10);
        assert_eq!(gpu.submitted(Party::Front), 90);
        assert_eq!(gpu.submitted(Party::Engine), 10);
        assert_eq!(gpu.submitted(Party::Wine), 0);
        // 释放后可再分。
        assert_eq!(gpu.release(Party::Engine), 350);
        assert!(gpu.reserve(Party::Wine, 400).is_ok());
        assert_eq!(gpu.total_reserved(), 800);
    }
}
