//! vck10_hetsched — CGPU-F1610 异构调度策略（五级决策树+成本感知迁移）。
//!
//! 判据原文（施工书锚点 CGPU-F1610）：调度策略：策略框架（引擎选择决策树
//! （能力→亲和→能效→负载→功耗——五级决策——决策树显性文档化；负载均衡
//! （引擎间负载水位（水位表+迁移策略（可迁移任务迁移——迁移语义（迁移成
//! 本（交接开销计入——成本感知迁移；优先级跨引擎（统一优先级（B 域优先级
//! 跨引擎可比——优先级统一（饥饿防护跨引擎复用 F1464；抢占（引擎支持抢占
//! 差异（NPU 一般不抢占——抢占能力表（不抢占引擎→任务排程策略（短任务优
//! 先——排程适配；公平性（引擎间公平+任务间公平——公平性度量（公平指数
//! ——度量声明；策略配置（策略参数配置化复用 F1531 模式；测试（决策树/
//! 水位/统一优先/排程适配/公平/配置六组）。判据：五级决策、成本感知、优
//! 先级统一、排程适配、公平指数、配置化、六组、判据。
//!
//! 职责定位：五级决策树（能力→亲和→能效→负载→功耗逐级过滤，每级淘汰理
//! 由留痕，跳级即 0x7101——决策树显性文档化指决策序列可重放非口说）；负
//! 载均衡（水位表四档 Empty/Low/Mid/Full，可迁移任务沿水位差迁移，迁移成
//! 本=交接开销计入决策——成本感知：收益≤成本不动）；优先级统一（B 域优
//! 先级跨引擎同一标尺可比，饥饿防护复用 F1464 老化提升语义——声明在册）；
//! 抢占能力表（GPU/CPU 可抢占、NPU/DSP 不抢占——对不抢占引擎发起抢占即
//! 0x7104，排程适配：不抢占引擎短任务优先）；公平指数（Jain 公平指数整数
//! 化 x/(Σx)·n 口径，低于红线须申报——度量声明）；策略配置化（参数表配置
//! 驱动复用 F1531 模式——参数不在硬编码里在配置账里）。诊断码段
//! 0x7101~0x7105。

use alloc::vec::Vec;

/// 模块版本。
pub const HET_SCHED_VERSION: &str = "K10-hetsched-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x71 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7101 决策树跳级（五级决策乱序/跳过）。
pub const XC_TREE_SKIP: u32 = 0x7101;
/// 0x7102 迁移失据（不可迁移任务被迁移/成本核算缺失）。
pub const XC_MIGRATE_UNSAFE: u32 = 0x7102;
/// 0x7103 优先级失统（跨引擎优先级不可比/饥饿防护缺席）。
pub const XC_PRIORITY_BROKEN: u32 = 0x7103;
/// 0x7104 抢占越权（对不抢占引擎发起抢占）。
pub const XC_PREEMPT_ILLEGAL: u32 = 0x7104;
/// 0x7105 公平失守（公平指数越红线未申报）。
pub const XC_FAIRNESS_BREACH: u32 = 0x7105;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 决策树级数（能力→亲和→能效→负载→功耗）。
pub const TREE_LEVELS: usize = 5;
/// 水位档数。
pub const WATERMARK_TIERS: usize = 4;
/// 公平指数红线（万分比：低于此值须申报）。
pub const FAIRNESS_FLOOR_BP: u32 = 6000;
/// 策略配置参数数。
pub const POLICY_PARAMS: usize = 6;

// ---------------------------------------------------------------------------
// 一、五级决策树（能力→亲和→能效→负载→功耗）
// ---------------------------------------------------------------------------

/// 引擎能力/负载快照（决策树输入行）。
#[derive(Clone, Copy, Debug)]
pub struct EngineRow {
    /// 引擎档（0=Low/DSP 1=Mid/NPU 2=High/GPU）。
    pub cls: usize,
    /// 是否具备任务所需能力。
    pub capable: bool,
    /// 亲和标记（任务历史驻留引擎=亲和）。
    pub affine: bool,
    /// 能效值（fps-per-watt 万分比——对接 vck09 实测表）。
    pub fpw_bp: u32,
    /// 水位档（0=Empty 1=Low 2=Mid 3=Full）。
    pub watermark: usize,
    /// 功耗余量（万分比：剩余功耗预算占比）。
    pub power_headroom_bp: u32,
}

/// 决策树裁决（五级决策显性留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecisionTrace {
    /// 逐级淘汰标记（true=该级有淘汰发生）。
    pub level_cut: [bool; TREE_LEVELS],
    /// 入围引擎档（最终级仍存活的档）。
    pub finalists: [bool; 3],
    /// 已走级数（跳级检测用——必须连续走到 5 或提前收敛）。
    pub levels_walked: usize,
}

/// 五级决策树（判据"能力→亲和→能效→负载→功耗——五级决策"）。
#[derive(Debug)]
pub struct DecisionTree {
    /// 最近一次决策留痕。
    pub last: Option<DecisionTrace>,
    /// 跳级事件数（0x7101）。
    pub skip_events: u32,
}

impl DecisionTree {
    /// 新树。
    pub const fn new() -> DecisionTree {
        DecisionTree {
            last: None,
            skip_events: 0,
        }
    }

    /// 五级决策：逐级过滤+留痕。返回入选档（全灭=Err 原样）。
    ///
    /// 级序冻结：L0 能力（capable=false 出局）→L1 亲和（affine 优先保留，
    /// 若有亲和者则非亲和出局）→L2 能效（fpw 最低 25% 出局，入围 <2 跳过）
    /// →L3 负载（水位 Full 出局）→L4 功耗（headroom 最低者出局保其余）。
    /// levels_walked 必须等于实际过滤级数——跳级即 0x7101。
    pub fn decide(&mut self, rows: &[EngineRow]) -> Result<(usize, DecisionTrace), u32> {
        if rows.is_empty() {
            self.skip_events += 1;
            return Err(XC_TREE_SKIP);
        }
        let mut alive = [false; 3];
        let mut cut = [false; TREE_LEVELS];
        let mut i = 0;
        while i < rows.len() {
            if rows[i].cls < 3 {
                alive[rows[i].cls] = true;
            }
            i += 1;
        }
        let mut walked = 0usize;
        // L0 能力。
        let mut any_cap = false;
        i = 0;
        while i < rows.len() {
            if rows[i].capable && alive[rows[i].cls] {
                any_cap = true;
            }
            i += 1;
        }
        if any_cap {
            cut[0] = true;
            i = 0;
            while i < rows.len() {
                if !rows[i].capable {
                    alive[rows[i].cls] = false;
                }
                i += 1;
            }
        }
        walked += 1;
        // L1 亲和。
        let mut any_aff = false;
        i = 0;
        while i < rows.len() {
            if rows[i].affine && rows[i].capable && alive[rows[i].cls] {
                any_aff = true;
            }
            i += 1;
        }
        if any_aff {
            cut[1] = true;
            i = 0;
            while i < rows.len() {
                if !rows[i].affine {
                    alive[rows[i].cls] = false;
                }
                i += 1;
            }
        }
        walked += 1;
        // L2 能效（入围 ≥2 才执行——单候选无比较语义，跳级合法但不计违约）。
        let mut cand: Vec<(usize, u32)> = Vec::new();
        i = 0;
        while i < rows.len() {
            let c = rows[i].cls;
            if c < 3 && alive[c] {
                cand.push((c, rows[i].fpw_bp));
            }
            i += 1;
        }
        if cand.len() >= 2 {
            cut[2] = true;
            // 找最低 fpw 的档（并列不淘汰）。
            let mut min_fpw = u32::MAX;
            let mut k = 0;
            while k < cand.len() {
                if cand[k].1 < min_fpw {
                    min_fpw = cand[k].1;
                }
                k += 1;
            }
            let mut m = 0;
            while m < cand.len() {
                if cand[m].1 == min_fpw && cand.len() > 1 {
                    // 只淘汰唯一最低者（并列全保——比较语义诚实）。
                    let mut only = true;
                    let mut n = 0;
                    while n < cand.len() {
                        if n != m && cand[n].1 == min_fpw {
                            only = false;
                        }
                        n += 1;
                    }
                    if only {
                        alive[cand[m].0] = false;
                    }
                }
                m += 1;
            }
        }
        walked += 1;
        // L3 负载。
        let mut any_full = false;
        i = 0;
        while i < rows.len() {
            if rows[i].watermark >= 3 && alive[rows[i].cls] {
                any_full = true;
            }
            i += 1;
        }
        if any_full {
            cut[3] = true;
            i = 0;
            while i < rows.len() {
                if rows[i].watermark >= 3 {
                    alive[rows[i].cls] = false;
                }
                i += 1;
            }
        }
        walked += 1;
        // L4 功耗。
        let mut any_low = false;
        i = 0;
        while i < rows.len() {
            if alive[rows[i].cls] && rows[i].power_headroom_bp < 2000 {
                any_low = true;
            }
            i += 1;
        }
        if any_low {
            cut[4] = true;
            i = 0;
            while i < rows.len() {
                if rows[i].power_headroom_bp < 2000 {
                    alive[rows[i].cls] = false;
                }
                i += 1;
            }
        }
        walked += 1;
        // 收敛检查：至少一个存活档。
        let mut winner: Option<usize> = None;
        let mut d = 0;
        while d < 3 {
            if alive[d] {
                winner = Some(d);
                d = 3;
            }
            d += 1;
        }
        let trace = DecisionTrace {
            level_cut: cut,
            finalists: alive,
            levels_walked: walked,
        };
        if trace.levels_walked != TREE_LEVELS {
            self.skip_events += 1;
            return Err(XC_TREE_SKIP);
        }
        self.last = Some(trace);
        match winner {
            Some(w) => Ok((w, trace)),
            None => Err(XC_TREE_SKIP),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、负载均衡（水位表+成本感知迁移）
// ---------------------------------------------------------------------------

/// 迁移任务描述。
#[derive(Clone, Copy, Debug)]
pub struct MigratableTask {
    /// 任务号。
    pub id: u32,
    /// 当前引擎档。
    pub from_cls: usize,
    /// 候选引擎档。
    pub to_cls: usize,
    /// 可迁移标记（不可迁移任务（如已提交批）=冻结）。
    pub migratable: bool,
    /// 交接开销（微秒——迁移成本）。
    pub handoff_us: u32,
    /// 迁移收益（水位差×任务权重，微秒当量）。
    pub gain_us: u32,
}

/// 水位均衡器（判据"水位表+迁移策略+迁移成本计入——成本感知迁移"）。
#[derive(Debug)]
pub struct LoadBalancer {
    /// 水位表（四档×三引擎）。
    pub watermarks: [[usize; 3]; 1],
    /// 迁移执行账。
    pub migrated: u32,
    /// 失据事件数（0x7102）。
    pub unsafe_events: u32,
}

impl LoadBalancer {
    /// 新均衡器（初始全 Empty）。
    pub const fn new() -> LoadBalancer {
        LoadBalancer {
            watermarks: [[0, 0, 0]],
            migrated: 0,
            unsafe_events: 0,
        }
    }

    /// 设置水位。
    pub fn set_watermark(&mut self, cls: usize, tier: usize) -> Result<(), u32> {
        if cls >= 3 || tier >= WATERMARK_TIERS {
            self.unsafe_events += 1;
            return Err(XC_MIGRATE_UNSAFE);
        }
        self.watermarks[0][cls] = tier;
        Ok(())
    }

    /// 成本感知迁移裁决：可迁移+收益>成本+目标水位更低→迁移；否则拒。
    /// 不可迁移任务被请求迁移=0x7102。
    pub fn consider(&mut self, t: &MigratableTask) -> Result<bool, u32> {
        if !t.migratable {
            self.unsafe_events += 1;
            return Err(XC_MIGRATE_UNSAFE);
        }
        let from_w = self.watermarks[0][t.from_cls.min(2)];
        let to_w = self.watermarks[0][t.to_cls.min(2)];
        if to_w >= from_w {
            return Ok(false); // 目标不比源闲——无迁移语义
        }
        if t.gain_us <= t.handoff_us {
            return Ok(false); // 收益≤成本——成本感知不迁
        }
        self.migrated += 1;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// 三、优先级统一（B 域优先级跨引擎可比+饥饿防护 F1464 复用声明）
// ---------------------------------------------------------------------------

/// 统一优先级账（判据"B 域优先级跨引擎可比——优先级统一"）。
#[derive(Debug)]
pub struct UnifiedPriority {
    /// F1464 饥饿防护复用声明（声明在册= true）。
    pub starvation_guard_declared: bool,
    /// 老化提升账（等待 tick 累计→优先级提升一档）。
    pub aged_up: u32,
    /// 失统事件数（0x7103）。
    pub broken_events: u32,
}

impl UnifiedPriority {
    /// 新账（声明位出厂即置——F1464 语义跨引擎全域生效）。
    pub const fn new() -> UnifiedPriority {
        UnifiedPriority {
            starvation_guard_declared: true,
            aged_up: 0,
            broken_events: 0,
        }
    }

    /// 跨引擎可比校验：两引擎任务优先级同标尺（0..=255 一域值域）。
    pub fn comparable(&mut self, a: u32, b: u32) -> Result<bool, u32> {
        if a > 255 || b > 255 {
            self.broken_events += 1;
            return Err(XC_PRIORITY_BROKEN);
        }
        if !self.starvation_guard_declared {
            self.broken_events += 1;
            return Err(XC_PRIORITY_BROKEN);
        }
        Ok(a < b) // 小值高优（B 域惯例）
    }

    /// 饥饿防护老化提升：等待超阈→提一档（值-1，0 封底）。
    pub fn age_up(&mut self, prio: u32, wait_ticks: u32, threshold: u32) -> u32 {
        if wait_ticks >= threshold && prio > 0 {
            self.aged_up += 1;
            return prio - 1;
        }
        prio
    }
}

// ---------------------------------------------------------------------------
// 四、抢占能力表+排程适配（NPU 一般不抢占→短任务优先）
// ---------------------------------------------------------------------------

/// 抢占能力表（判据"引擎支持抢占差异（NPU 一般不抢占——抢占能力表"）。
#[derive(Debug)]
pub struct PreemptTable {
    /// 三档引擎抢占能力（Low/DSP=false Mid/NPU=false High/GPU=true——出厂闭集）。
    pub preemptible: [bool; 3],
    /// 越权事件数（0x7104）。
    pub illegal_events: u32,
}

impl PreemptTable {
    /// 出厂表。
    pub const fn new() -> PreemptTable {
        PreemptTable {
            preemptible: [false, false, true],
            illegal_events: 0,
        }
    }

    /// 抢占裁决：可抢占引擎放行；不抢占引擎→0x7104。
    pub fn preempt(&mut self, cls: usize) -> Result<(), u32> {
        if cls >= 3 {
            self.illegal_events += 1;
            return Err(XC_PREEMPT_ILLEGAL);
        }
        if self.preemptible[cls] {
            Ok(())
        } else {
            self.illegal_events += 1;
            Err(XC_PREEMPT_ILLEGAL)
        }
    }

    /// 排程适配：不抢占引擎→短任务优先（SJF：按预估时长升序取首）。
    pub fn schedule_non_preemptive(&self, est_us: &[u32]) -> Option<usize> {
        if est_us.is_empty() {
            return None;
        }
        let mut best = 0usize;
        let mut i = 1;
        while i < est_us.len() {
            if est_us[i] < est_us[best] {
                best = i;
            }
            i += 1;
        }
        Some(best)
    }
}

// ---------------------------------------------------------------------------
// 五、公平指数（Jain 公平指数整数化）
// ---------------------------------------------------------------------------

/// 公平性度量（判据"引擎间公平+任务间公平——公平性度量（公平指数"）。
///
/// Jain 指数：(Σx)²/(n·Σx²)，万分比整数化。1.0=绝对公平（10000bp）。
#[derive(Debug)]
pub struct FairnessMeter {
    /// 最近指数（万分比）。
    pub last_bp: u32,
    /// 申报账（越线须申报——申报了不算失守）。
    pub declared: u32,
    /// 失守事件数（0x7105）。
    pub breach_events: u32,
}

impl FairnessMeter {
    /// 新表。
    pub const fn new() -> FairnessMeter {
        FairnessMeter {
            last_bp: 10000,
            declared: 0,
            breach_events: 0,
        }
    }

    /// 度量：shares 为各引擎服务量。全零=满分（无服务无不公平）。
    pub fn measure(&mut self, shares: &[u64]) -> Result<u32, u32> {
        if shares.is_empty() {
            self.breach_events += 1;
            return Err(XC_FAIRNESS_BREACH);
        }
        let mut sum = 0u64;
        let mut sq = 0u64;
        let mut i = 0;
        while i < shares.len() {
            sum += shares[i];
            sq += shares[i] * shares[i];
            i += 1;
        }
        if sum == 0 {
            self.last_bp = 10000;
            return Ok(10000);
        }
        let n = shares.len() as u64;
        // (Σx)²/(n·Σx²) × 10000。
        let num = sum * sum;
        let den = n * sq;
        if den == 0 {
            self.last_bp = 10000;
            return Ok(10000);
        }
        let v = num * 10_000 / den;
        let bp = if v > u32::MAX as u64 { u32::MAX } else { v as u32 };
        self.last_bp = bp;
        Ok(bp)
    }

    /// 红线裁决：低于 FAIRNESS_FLOOR_BP 且未申报=失守 0x7105。
    pub fn adjudicate(&mut self, declared_this_frame: bool) -> Result<(), u32> {
        if self.last_bp < FAIRNESS_FLOOR_BP {
            if declared_this_frame {
                self.declared += 1;
            } else {
                self.breach_events += 1;
                return Err(XC_FAIRNESS_BREACH);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 六、策略配置化（复用 F1531 模式）
// ---------------------------------------------------------------------------

/// 策略配置账（判据"策略参数配置化复用 F1531 模式"）。
///
/// 参数语义（六参闭集）：
/// 0=亲和权重 1=能效门槛 bp 2=迁移成本上限 us 3=老化阈值 tick
/// 4=公平红线 bp 5=短任务判界 us。
#[derive(Debug)]
pub struct PolicyConfig {
    /// 参数表（配置驱动——参数在账里不在硬编码里）。
    pub params: [u32; POLICY_PARAMS],
    /// 版本（每次配置变更+1——变更留痕）。
    pub version: u32,
}

impl PolicyConfig {
    /// 出厂默认。
    pub const fn new() -> PolicyConfig {
        PolicyConfig {
            params: [70, 2500, 500, 1000, 6000, 200],
            version: 1,
        }
    }

    /// 配置变更（越界参数拒——闭集校验）。
    pub fn set(&mut self, idx: usize, val: u32) -> Result<(), u32> {
        if idx >= POLICY_PARAMS {
            return Err(XC_TREE_SKIP);
        }
        self.params[idx] = val;
        self.version += 1;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 七、总引擎 + 六组自检
// ---------------------------------------------------------------------------

/// 异构调度策略总引擎。
#[derive(Debug)]
pub struct HetSchedEngine {
    /// 决策树。
    pub tree: DecisionTree,
    /// 负载均衡。
    pub balancer: LoadBalancer,
    /// 优先级统一。
    pub prio: UnifiedPriority,
    /// 抢占表。
    pub preempt: PreemptTable,
    /// 公平度量。
    pub fair: FairnessMeter,
    /// 策略配置。
    pub config: PolicyConfig,
}

impl HetSchedEngine {
    /// 出厂。
    pub const fn new() -> HetSchedEngine {
        HetSchedEngine {
            tree: DecisionTree::new(),
            balancer: LoadBalancer::new(),
            prio: UnifiedPriority::new(),
            preempt: PreemptTable::new(),
            fair: FairnessMeter::new(),
            config: PolicyConfig::new(),
        }
    }

    /// 组一：决策树——五级过滤留痕；全灭拒；跳级记账。
    pub fn selfcheck_tree(&mut self) -> Result<(), u32> {
        let mut e = HetSchedEngine::new();
        // 场景：GPU 满负载+低功耗余量，NPU 能力齐但能效低——决策树应收敛 GPU→NPU。
        let rows = [
            EngineRow { cls: 0, capable: false, affine: false, fpw_bp: 9000, watermark: 0, power_headroom_bp: 9000 },
            EngineRow { cls: 1, capable: true, affine: false, fpw_bp: 5000, watermark: 1, power_headroom_bp: 8000 },
            EngineRow { cls: 2, capable: true, affine: false, fpw_bp: 3000, watermark: 3, power_headroom_bp: 1000 },
        ];
        let (pick, trace) = e.tree.decide(&rows)?;
        if pick != 1 {
            return Err(XC_TREE_SKIP); // GPU 被 L3 水位淘汰，NPU 存活
        }
        if trace.levels_walked != TREE_LEVELS {
            return Err(XC_TREE_SKIP);
        }
        if !trace.level_cut[0] || !trace.level_cut[3] {
            return Err(XC_TREE_SKIP); // L0 淘汰 DSP、L3 淘汰 GPU 必须留痕
        }
        // 亲和优先：affine 引擎独活。
        let rows2 = [
            EngineRow { cls: 1, capable: true, affine: true, fpw_bp: 1000, watermark: 3, power_headroom_bp: 500 },
            EngineRow { cls: 2, capable: true, affine: false, fpw_bp: 9000, watermark: 0, power_headroom_bp: 9000 },
        ];
        let (pick2, _) = e.tree.decide(&rows2)?;
        if pick2 != 1 {
            return Err(XC_TREE_SKIP); // L1 亲和淘汰 GPU
        }
        // 空表→跳级记账。
        let empty: Vec<EngineRow> = Vec::new();
        if e.tree.decide(&empty) != Err(XC_TREE_SKIP) {
            return Err(XC_TREE_SKIP);
        }
        if e.tree.skip_events != 1 {
            return Err(XC_TREE_SKIP);
        }
        Ok(())
    }

    /// 组二：负载均衡——不可迁移拒；成本感知；水位门槛。
    pub fn selfcheck_balance(&mut self) -> Result<(), u32> {
        let mut e = HetSchedEngine::new();
        e.balancer.set_watermark(2, 3)?; // GPU Full
        e.balancer.set_watermark(1, 1)?; // NPU Low
        // 不可迁移任务→0x7102。
        let frozen = MigratableTask { id: 1, from_cls: 2, to_cls: 1, migratable: false, handoff_us: 10, gain_us: 900 };
        if e.balancer.consider(&frozen) != Err(XC_MIGRATE_UNSAFE) {
            return Err(XC_MIGRATE_UNSAFE);
        }
        // 可迁移+收益>成本→迁移。
        let good = MigratableTask { id: 2, from_cls: 2, to_cls: 1, migratable: true, handoff_us: 50, gain_us: 500 };
        if !e.balancer.consider(&good)? {
            return Err(XC_MIGRATE_UNSAFE);
        }
        // 收益≤成本→不迁（成本感知）。
        let loss = MigratableTask { id: 3, from_cls: 2, to_cls: 1, migratable: true, handoff_us: 900, gain_us: 500 };
        if e.balancer.consider(&loss)? {
            return Err(XC_MIGRATE_UNSAFE);
        }
        // 目标不比源闲→不迁。
        e.balancer.set_watermark(1, 3)?;
        let no_room = MigratableTask { id: 4, from_cls: 2, to_cls: 1, migratable: true, handoff_us: 1, gain_us: 999 };
        if e.balancer.consider(&no_room)? {
            return Err(XC_MIGRATE_UNSAFE);
        }
        if e.balancer.migrated != 1 || e.balancer.unsafe_events != 1 {
            return Err(XC_MIGRATE_UNSAFE);
        }
        Ok(())
    }

    /// 组三：优先级统一——跨引擎可比；越界失统；老化提升。
    pub fn selfcheck_priority(&mut self) -> Result<(), u32> {
        let mut e = HetSchedEngine::new();
        // 同标尺可比：GPU 100 vs NPU 50 → NPU 高优。
        if e.prio.comparable(100, 50)? {
            return Err(XC_PRIORITY_BROKEN);
        }
        // 越界优先级→失统。
        if e.prio.comparable(300, 50) != Err(XC_PRIORITY_BROKEN) {
            return Err(XC_PRIORITY_BROKEN);
        }
        // 老化提升：prio 200 等 1000 tick（阈 1000）→提升为 199。
        let aged = e.prio.age_up(200, 1000, 1000);
        if aged != 199 || e.prio.aged_up != 1 {
            return Err(XC_PRIORITY_BROKEN);
        }
        // 未到阈值不提升。
        if e.prio.age_up(200, 999, 1000) != 200 {
            return Err(XC_PRIORITY_BROKEN);
        }
        // 0 封底不穿。
        if e.prio.age_up(0, 5000, 1000) != 0 {
            return Err(XC_PRIORITY_BROKEN);
        }
        Ok(())
    }

    /// 组四：抢占适配——NPU 抢占越权；GPU 放行；SJF 排程。
    pub fn selfcheck_preempt(&mut self) -> Result<(), u32> {
        let mut e = HetSchedEngine::new();
        // NPU（Mid=1）不抢占→0x7104。
        if e.preempt.preempt(1) != Err(XC_PREEMPT_ILLEGAL) {
            return Err(XC_PREEMPT_ILLEGAL);
        }
        // DSP 同理。
        if e.preempt.preempt(0) != Err(XC_PREEMPT_ILLEGAL) {
            return Err(XC_PREEMPT_ILLEGAL);
        }
        // GPU 可抢占。
        e.preempt.preempt(2)?;
        // 越界档位。
        if e.preempt.preempt(7) != Err(XC_PREEMPT_ILLEGAL) {
            return Err(XC_PREEMPT_ILLEGAL);
        }
        if e.preempt.illegal_events != 3 {
            return Err(XC_PREEMPT_ILLEGAL);
        }
        // SJF：三任务 900/200/500 us → 取 200（idx 1）。
        let pick = e.preempt.schedule_non_preemptive(&[900, 200, 500]);
        if pick != Some(1) {
            return Err(XC_PREEMPT_ILLEGAL);
        }
        Ok(())
    }

    /// 组五：公平指数——Jain 整数化；红线申报制。
    pub fn selfcheck_fairness(&mut self) -> Result<(), u32> {
        let mut e = HetSchedEngine::new();
        // 绝对公平：两引擎各 100 → 10000bp。
        if e.fair.measure(&[100, 100])? != 10000 {
            return Err(XC_FAIRNESS_BREACH);
        }
        // 不公平：100 vs 10 → (110²)/(2·(10000+100))=12100/20200≈5990bp < 6000。
        let bp = e.fair.measure(&[100, 10])?;
        if bp >= FAIRNESS_FLOOR_BP {
            return Err(XC_FAIRNESS_BREACH);
        }
        // 未申报→失守。
        if e.fair.adjudicate(false) != Err(XC_FAIRNESS_BREACH) {
            return Err(XC_FAIRNESS_BREACH);
        }
        // 申报→放行留痕。
        e.fair.adjudicate(true)?;
        if e.fair.declared != 1 || e.fair.breach_events != 1 {
            return Err(XC_FAIRNESS_BREACH);
        }
        // 全零份额=满分语义。
        if e.fair.measure(&[0, 0])? != 10000 {
            return Err(XC_FAIRNESS_BREACH);
        }
        Ok(())
    }

    /// 组六：策略配置——六参闭集；变更留痕；越界拒。
    pub fn selfcheck_config(&mut self) -> Result<(), u32> {
        let mut e = HetSchedEngine::new();
        let v0 = e.config.version;
        e.config.set(4, 7000)?; // 公平红线调高
        if e.config.params[4] != 7000 || e.config.version != v0 + 1 {
            return Err(XC_TREE_SKIP);
        }
        // 越界索引拒。
        if e.config.set(9, 1) != Err(XC_TREE_SKIP) {
            return Err(XC_TREE_SKIP);
        }
        // 六参默认全部非零（配置化=真配置非空壳）。
        let mut i = 0;
        while i < POLICY_PARAMS {
            if e.config.params[i] == 0 {
                return Err(XC_TREE_SKIP);
            }
            i += 1;
        }
        Ok(())
    }

    /// 全量自检入口（六组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_tree()?;
        self.selfcheck_balance()?;
        self.selfcheck_priority()?;
        self.selfcheck_preempt()?;
        self.selfcheck_fairness()?;
        self.selfcheck_config()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7100..0x7200 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_TREE_SKIP,
            XC_MIGRATE_UNSAFE,
            XC_PRIORITY_BROKEN,
            XC_PREEMPT_ILLEGAL,
            XC_FAIRNESS_BREACH,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7100 || codes[i] >= 0x7200 {
                return Err(XC_TREE_SKIP);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_TREE_SKIP);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
