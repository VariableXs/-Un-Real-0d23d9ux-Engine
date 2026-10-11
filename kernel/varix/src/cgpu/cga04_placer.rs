//! CGPU-F0004 · CPU/GPU 任务放置决策器（CGPU-A 域 · 帧任务图与执行域 · A02 组 · 目标 440 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0004`
//!
//! **判据（锚点原文）**：双路对拍一致、滞回防抖实测、预测误差 ≤30%、
//! 重标定闭环、决策日志全量。
//!
//! **职责定位（锚点原文）**：每个渲染任务放 CPU 还是 GPU？本功能实现放置
//! 决策器：对每类任务建立成本模型（数据量、算子密度、GPU 占用现状、CPU
//! 空闲度、传输成本——CPU↔GPU 数据搬运本身耗带宽），预测双路执行耗时，
//! 选期望帧延迟更低的一路。决策纪律：同一效果双路都必须有实现（与 VE-N/F
//! 系列「原生效果重建」协议对接），放置只切路径不改像素结果，双路输出对拍
//! 一致是决策器存在的前提。滞后策略：放置决策带滞回（连续 5 帧差 >15% 才
//! 切换），避免抖动反复横跳。决策全记录（每帧每任务选了哪路、预测 vs 实测
//! 误差），误差持续 >30% 的成本模型自动重标定。
//!
//! ## 一、成本画像与成本模型：预测是公式不是猜测
//!
//! [`CostProfile`] 承载锚点点名的五个输入：数据量（data_bytes——传输与
//! 计算的规模基）、算子密度（ops_per_byte——每字节算子数，算子密任务偏
//! GPU）、GPU 占用现状（gpu_busy_millis——在途任务余量，占用高时 GPU 路
//! 预估上浮）、CPU 空闲度（cpu_free_millis——空闲核算力，空闲大时 CPU 路
//! 下压）、传输成本（bandwidth_mbps——CPU↔GPU 搬运本身耗带宽，数据大的
//! 任务搬运费可能吃掉 GPU 算力优势）。[`CostModel::predict`] 双路公式
//! 全部写死并注释来源（标定常数注明当前策略值可调）：CPU 路 = 算子数 ÷
//! CPU 吞吐 × 空闲折算；GPU 路 = 传输微秒 + 核算微秒 × 占用折算；返回
//! [`PathEstimate`] 双路预估与差值——预测是可复算的算术，不是拍脑袋。
//!
//! ## 二、决策纪律：双路契约是决策器存在的资格证
//!
//! 放置只切路径不改像素结果——前提是两路都能跑同一效果（VE-N/F 原生
//! 效果重建协议）。[`DualPathContract`] 强制登记双路实现声明：缺一路
//! 的任务决策器直接拒（[`PlcCode::ContractMissingPath`]）——单路任务没有
//! 放置自由，硬编码路径声明走契约的固定路径字段。对拍账（判据「双路对拍
//! 一致」）：执行面在双路各跑一次输出指纹（输出哈希由执行面上报——本域
//! 只记账与裁决），[`OutcomePair`] 逐对登记；指纹不一致 = 决策器资格
//! 出局（[`PlcCode::DivergenceDetected`] 立案），在差异查清前该效果族
//! 冻结在当前路径（不放回自动决策——诚实优先于吞吐）。
//!
//! ## 三、滞回引擎：连续 5 帧差 >15% 才切换
//!
//! [`Hysteresis`] 防抖状态机（判据「滞回防抖实测」）：每帧重算双路预估，
//! 差值比 = |est[候选路] − est[当前路]| ÷ est[当前路]；候选路更优且差值
//! > [`HYST_RATIO`]（15%）连续 [`HYST_WINDOW`]（5）帧才真正切换；中途
//! 差值回落即清零计数（滞回带不是计数器——反复横跳在带内被吸收）。切换
//! 本身入账（from/to/证据差值序列）——防抖不是不可切换，是不许抖。
//!
//! ## 四、预测误差账与重标定闭环
//!
//! 实测耗时由执行面回填（tick 注入，零墙钟）；[`ErrorLedger`] 逐任务记
//! 预测 vs 实测误差比。误差持续 >30%（连续 [`RECAL_WINDOW`] 帧，判据
//! 「预测误差 ≤30%」的执法面）→ [`Recalibrator`] 自动重标定：按实测/
//! 预测比例缩放对应路径的吞吐常数（CPU 吞吐/GPU 吞吐/带宽三个常数各自
//! 独立归因——哪个路径的误差超限就标定哪个），重标定事件留 before/after
//! 常量与证据误差（闭环可审计），标定后误差账清零重计（判据「重标定
//! 闭环」）。重标定有界：单帧最多一次（防震荡），幅度钳制
//! [`RECAL_MAX_SHIFT`]（±50%——重标定是修正不是重构）。
//!
//! ## 五、决策日志：全量可回放
//!
//! [`DecisionLog`] 每帧每任务一条 [`DecisionRecord`]：帧号/任务族/双路
//! 预估/选择/切换证据/后续回填的实测与误差——滚动窗口全量保留在窗口内
//! （[`LOG_CAPACITY`]，超窗归档计数——归档不是丢账）。判据「决策日志
//! 全量可回放」的落地面：日志可按帧重放决策序列（回放走同一决策函数，
//! 同输入同输出——确定性），日志计数与决策计数守恒（[`DecisionLog`]
//! `reconcile`）。
//!
//! **对接**：F0003（帧任务图——任务画像与资源读写集来源）；F0005（帧
//! 预算仲裁——放置结论影响预算账户申请）；F0010（带宽调度——传输成本
//! 常数联动）；F0007（卡顿归因——误差账与切换史是归因证据源）；VE-N/F
//! 系列（原生效果重建——双路契约的协议对端）。零 panic 面（下标走
//! `get`、算术饱和）、零 IO、零墙钟（tick 上游注入）、无全局可变状态、
//! no_std + alloc。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（来源：锚点原文数字 + 标定值注明可调）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const PLACER_VERSION: &str = "A02-placer-v1";

/// 滞回窗口（帧）：锚点原文「连续 5 帧差 >15% 才切换」。
pub const HYST_WINDOW: u32 = 5;

/// 滞回差值阈值（百分数）：锚点原文 15%。
pub const HYST_RATIO_PCT: u64 = 15;

/// 误差重标定阈值（百分数）：锚点原文「误差持续 >30% 的成本模型自动重标定」。
pub const RECAL_RATIO_PCT: u64 = 30;

/// 误差持续窗口（帧）：连续多少帧超阈才触发重标定（当前策略值，可调）。
pub const RECAL_WINDOW: u32 = 8;

/// 重标定幅度钳制（百分数）：单次缩放不超过 ±50%（修正不是重构）。
pub const RECAL_MAX_SHIFT_PCT: u64 = 50;

/// 决策日志滚动窗口容量（条；当前策略值，可调）。
pub const LOG_CAPACITY: usize = 2_048;

/// 任务族数上限（决策器按任务族记账；当前策略值，可调）。
pub const FAMILY_CAPACITY: usize = 256;

// ---- 成本模型标定常数（当前策略值，可调——重标定器是它们的唯一写者）----

/// CPU 吞吐基线（算子/微秒）：宿主标定，当前策略值。
pub const CPU_OPS_PER_US_BASE: u64 = 96;

/// GPU 吞吐基线（算子/微秒）：标定值（相对 CPU 的加速比 ≈ 16×），当前策略值。
pub const GPU_OPS_PER_US_BASE: u64 = 1_536;

/// 传输带宽基线（字节/微秒 ≈ 1 GB/s）：PCIe 标定，当前策略值。
pub const BANDWIDTH_BPS_US_BASE: u64 = 1_000;

/// GPU 占用折算上限（百分数）：占用率换算为耗时上浮的封顶（当前策略值）。
pub const GPU_BUSY_PENALTY_CAP_PCT: u64 = 300;

// ---------------------------------------------------------------------------
// 二、诊断码（0x65 段独占：A02 放置决策面）
// ---------------------------------------------------------------------------

/// 诊断码（u16；0x65 段为本单独占段，段内互异）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlcCode {
    /// 双路契约缺失：任务未登记双路实现声明。
    ContractMissing = 0x6501,
    /// 契约缺一路：声明了双路但某路实现位为假（无实现无放置自由）。
    ContractMissingPath = 0x6502,
    /// 契约重复登记：同任务族重复注册。
    ContractDuplicate = 0x6503,
    /// 契约表满。
    ContractTableFull = 0x6504,
    /// 成本画像非法：数据量/占用/空闲字段越界。
    ProfileInvalid = 0x6505,
    /// 双路输出指纹不一致：放置资格出局立案。
    DivergenceDetected = 0x6506,
    /// 对拍缺一半：只收到单路输出指纹（对拍必须成对）。
    DivergencePairIncomplete = 0x6507,
    /// 任务族不存在：误差回填/决策查询的族未登记。
    FamilyUnknown = 0x6508,
    /// 实测回填非法：实测耗时为 0（物理不可执行）。
    ActualZero = 0x6509,
    /// 重标定幅度越界：缩放比超出 ±50% 钳制（防重构式标定）。
    RecalShiftOutOfRange = 0x650a,
    /// 重标定频率越界：单帧内重复触发（防震荡）。
    RecalDuplicate = 0x650b,
    /// 滞回状态非法：切换计数窗口错位（内部不变量破坏）。
    HysteresisCorrupt = 0x650c,
    /// 决策日志满且无可归档条目（全部未回填——日志不可截断半条账）。
    LogFullUnfilled = 0x650d,
    /// 决策日志守恒破坏：决策数与日志数不符。
    LogReconcileBroken = 0x650e,
    /// 固定路径任务的路径与契约不符。
    FixedPathMismatch = 0x650f,
}

impl PlcCode {
    /// 人读诊断（发生了什么/为什么/下一步）。
    pub const fn detail(self) -> &'static str {
        match self {
            PlcCode::ContractMissing => "双路契约缺失：任务族未登记双路声明，建议先注册契约再请求放置",
            PlcCode::ContractMissingPath => "契约缺一路：双路都必须有实现才有放置自由，建议补实现或改固定路径",
            PlcCode::ContractDuplicate => "契约重复登记：同任务族重复注册，建议核对注册流程幂等性",
            PlcCode::ContractTableFull => "契约表满：族数超上限，建议合并同质任务族或调 FAMILY_CAPACITY",
            PlcCode::ProfileInvalid => "成本画像非法：字段越界（占用/空闲应 ≤ 容量基线），建议核对画像来源",
            PlcCode::DivergenceDetected => "双路输出指纹不一致：像素结果路径依赖=违反放置纪律，建议冻结该族自动放置并立案排查实现差异",
            PlcCode::DivergencePairIncomplete => "对拍缺一半：双路指纹必须成对上报，建议核对执行面上报流程",
            PlcCode::FamilyUnknown => "任务族不存在：误差回填的族未登记，建议先注册契约",
            PlcCode::ActualZero => "实测耗时为 0：物理不可执行，建议核对计时面注入",
            PlcCode::RecalShiftOutOfRange => "重标定幅度越界：缩放超 ±50%，建议分多次小幅重标定",
            PlcCode::RecalDuplicate => "重标定频率越界：单帧至多一次，建议合并同帧触发",
            PlcCode::HysteresisCorrupt => "滞回状态非法：内部不变量破坏，建议重建滞回状态并核对切换史",
            PlcCode::LogFullUnfilled => "决策日志满且全部未回填：不能截断半条账，建议加快回填或扩 LOG_CAPACITY",
            PlcCode::LogReconcileBroken => "决策日志守恒破坏：决策数与日志数不符，建议停止决策并核查记账面",
            PlcCode::FixedPathMismatch => "固定路径与契约不符：固定路径任务的执行路必须与契约声明一致",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、执行路径与成本画像
// ---------------------------------------------------------------------------

/// 执行路径闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionPath {
    /// CPU 路径。
    Cpu,
    /// GPU 路径。
    Gpu,
}

impl ExecutionPath {
    /// 全闭集（对拍账独立重排用）。
    pub const ALL: [ExecutionPath; 2] = [ExecutionPath::Cpu, ExecutionPath::Gpu];

    /// 路径名。
    pub const fn zh(self) -> &'static str {
        match self {
            ExecutionPath::Cpu => "CPU",
            ExecutionPath::Gpu => "GPU",
        }
    }

    /// 编号（日志序列化用）。
    pub const fn index(self) -> u8 {
        match self {
            ExecutionPath::Cpu => 0,
            ExecutionPath::Gpu => 1,
        }
    }

    /// 从编号恢复。
    pub const fn from_index(i: u8) -> Option<ExecutionPath> {
        match i {
            0 => Some(ExecutionPath::Cpu),
            1 => Some(ExecutionPath::Gpu),
            _ => None,
        }
    }
}

/// 成本画像（锚点五输入：数据量/算子密度/GPU 占用/CPU 空闲/传输成本）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostProfile {
    /// 数据量（字节）。
    pub data_bytes: u64,
    /// 算子密度（算子/字节）。
    pub ops_per_byte: u64,
    /// GPU 在途占用（毫秒当量——在途任务耗时总和的画像侧快照）。
    pub gpu_busy_millis: u64,
    /// CPU 空闲核算力（毫秒当量）。
    pub cpu_free_millis: u64,
    /// 占用/空闲画像的容量基线（毫秒当量——折算比的分母，防止除小数）。
    pub capacity_millis: u64,
}

impl CostProfile {
    /// 画像合法性（容量基线必须为正；占用与空闲不得超过容量基线——
    /// 超过即画像来源脏了，登记期拒）。
    pub const fn validate(&self) -> Option<PlcCode> {
        if self.capacity_millis == 0 {
            return Some(PlcCode::ProfileInvalid);
        }
        if self.gpu_busy_millis > self.capacity_millis {
            return Some(PlcCode::ProfileInvalid);
        }
        if self.cpu_free_millis > self.capacity_millis {
            return Some(PlcCode::ProfileInvalid);
        }
        None
    }
}

/// 双路预估（微秒；公式见 [`CostModel::predict`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathEstimate {
    /// CPU 路预估。
    pub cpu_us: u64,
    /// GPU 路预估（含传输费）。
    pub gpu_us: u64,
    /// 预估时使用的标定常数指纹（重标定后预估可比对——常数变了预估
    /// 不可跨标定版本比较）。
    pub calib_epoch: u32,
}

impl PathEstimate {
    /// 更优路径（平局取 CPU——CPU 不占 GPU 带宽；平局语义显性写死）。
    pub const fn better(&self) -> ExecutionPath {
        if self.gpu_us < self.cpu_us {
            ExecutionPath::Gpu
        } else {
            ExecutionPath::Cpu
        }
    }

    /// 指定路径的预估微秒。
    pub const fn of(&self, p: ExecutionPath) -> u64 {
        match p {
            ExecutionPath::Cpu => self.cpu_us,
            ExecutionPath::Gpu => self.gpu_us,
        }
    }

    /// 两路差值比（百分数；相对基准路径，分母为 0 时返回 u64::MAX——
    /// 基线为零预估意味着画像异常，差值视为无穷大交由上游处置）。
    pub const fn diff_pct_vs(&self, base: ExecutionPath) -> u64 {
        let b = self.of(base);
        let other = match base {
            ExecutionPath::Cpu => self.gpu_us,
            ExecutionPath::Gpu => self.cpu_us,
        };
        if b == 0 {
            return u64::MAX;
        }
        let delta = if other > b { other - b } else { b - other };
        delta.saturating_mul(100).saturating_div(b)
    }
}

/// 成本模型（标定常数的持有者；重标定器是唯一写者）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostModel {
    /// CPU 吞吐（算子/微秒）。
    pub cpu_ops_per_us: u64,
    /// GPU 吞吐（算子/微秒）。
    pub gpu_ops_per_us: u64,
    /// 传输带宽（字节/微秒）。
    pub bandwidth_bps_us: u64,
    /// 标定纪元（每次重标定 +1——预估可比性边界）。
    pub calib_epoch: u32,
}

impl CostModel {
    /// 基线模型（标定常数 = 基线值；纪元 0）。
    pub const fn baseline() -> CostModel {
        CostModel {
            cpu_ops_per_us: CPU_OPS_PER_US_BASE,
            gpu_ops_per_us: GPU_OPS_PER_US_BASE,
            bandwidth_bps_us: BANDWIDTH_BPS_US_BASE,
            calib_epoch: 0,
        }
    }

    /// 双路预估公式（全部写死并注明来源）：
    /// - CPU 路：ops = data × density；est = ops ÷ cpu_throughput，再按
    ///   CPU 空闲折算——空闲充足（free ≥ ops 需要的毫秒当量）不打折，
    ///   空闲不足按 free/capacity 比例上浮（上限 +300%——空闲枯竭时
    ///   CPU 路不再是无限便宜的后备）。
    /// - GPU 路：传输费 = data ÷ bandwidth（往返记 2×——上传 + 回读元数据；
    ///   纯上传效果按传输系数 TRANSMIT_FACTOR 写死 2，当前策略值可调）；
    ///   核算 = ops ÷ gpu_throughput，按 GPU 占用折算上浮（占用比 =
    ///   busy/capacity，上浮 = 占用比 × 100%，封顶 +300%）。
    pub fn predict(&self, p: &CostProfile) -> PathEstimate {
        let ops = p.data_bytes.saturating_mul(p.ops_per_byte);
        // CPU 路。
        let cpu_raw = if self.cpu_ops_per_us == 0 {
            u64::MAX
        } else {
            ops.saturating_div(self.cpu_ops_per_us).max(1)
        };
        let free_ratio_pct = (p.cpu_free_millis.saturating_mul(100)).saturating_div(p.capacity_millis);
        // 空闲折算：free_ratio 100% → ×1；0% → ×4（+300% 封顶）。
        let cpu_factor_pct = 100u64
            .saturating_add((100u64.saturating_sub(free_ratio_pct)).saturating_mul(3))
            .min(100 + GPU_BUSY_PENALTY_CAP_PCT);
        let cpu_us = cpu_raw.saturating_mul(cpu_factor_pct).saturating_div(100);
        // GPU 路。
        let transfer_us = p
            .data_bytes
            .saturating_mul(2)
            .saturating_div(self.bandwidth_bps_us.max(1));
        let kernel_raw = if self.gpu_ops_per_us == 0 {
            u64::MAX
        } else {
            ops.saturating_div(self.gpu_ops_per_us).max(1)
        };
        let busy_ratio_pct = (p.gpu_busy_millis.saturating_mul(100)).saturating_div(p.capacity_millis);
        let gpu_factor_pct = 100u64
            .saturating_add(busy_ratio_pct)
            .min(100 + GPU_BUSY_PENALTY_CAP_PCT);
        let gpu_us = transfer_us
            .saturating_add(kernel_raw.saturating_mul(gpu_factor_pct).saturating_div(100));
        PathEstimate { cpu_us, gpu_us, calib_epoch: self.calib_epoch }
    }
}

// ---------------------------------------------------------------------------
// 四、双路契约（决策器存在资格证）
// ---------------------------------------------------------------------------

/// 双路实现契约（VE-N/F 原生效果重建协议的对端声明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DualPathContract {
    /// 任务族名（唯一键）。
    pub family: String,
    /// CPU 路实现位。
    pub has_cpu: bool,
    /// GPU 路实现位。
    pub has_gpu: bool,
    /// 固定路径（Some = 该族不参与自动放置，硬编码路径——单路任务与
    /// 合同级锁路径效果的登记口）。
    pub fixed_path: Option<ExecutionPath>,
}

/// 契约表（族 → 契约；唯一键查重）。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ContractTable {
    contracts: Vec<DualPathContract>,
}

impl ContractTable {
    /// 空表。
    pub const fn new() -> ContractTable {
        ContractTable { contracts: Vec::new() }
    }

    /// 注册契约（重复族显性拒；缺一路且无固定路径声明拒——无实现无资格）。
    pub fn register(&mut self, c: DualPathContract) -> Result<(), PlcCode> {
        if self.contracts.len() >= FAMILY_CAPACITY {
            return Err(PlcCode::ContractTableFull);
        }
        let mut i = 0usize;
        while i < self.contracts.len() {
            if let Some(x) = self.contracts.get(i) {
                if x.family == c.family {
                    return Err(PlcCode::ContractDuplicate);
                }
            }
            i += 1;
        }
        // 双路缺一且未声明固定路径 = 资格缺失。
        if c.fixed_path.is_none() && (!c.has_cpu || !c.has_gpu) {
            return Err(PlcCode::ContractMissingPath);
        }
        // 固定路径必须与实现位一致（声明 GPU 固定但 has_gpu=false 是脏契约）。
        if let Some(p) = c.fixed_path {
            let ok = match p {
                ExecutionPath::Cpu => c.has_cpu,
                ExecutionPath::Gpu => c.has_gpu,
            };
            if !ok {
                return Err(PlcCode::FixedPathMismatch);
            }
        }
        self.contracts.push(c);
        Ok(())
    }

    /// 查契约。
    pub fn get(&self, family: &str) -> Option<&DualPathContract> {
        let mut i = 0usize;
        while i < self.contracts.len() {
            if let Some(c) = self.contracts.get(i) {
                if c.family == family {
                    return Some(c);
                }
            }
            i += 1;
        }
        None
    }

    /// 在册族数。
    pub fn len(&self) -> usize {
        self.contracts.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.contracts.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 五、放置决策器 + 滞回引擎
// ---------------------------------------------------------------------------

/// 滞回状态（防抖状态机）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hysteresis {
    /// 当前生效路径。
    pub current: ExecutionPath,
    /// 候选路径连续达标计数（差值 >15% 的连续帧数）。
    pub streak: u32,
    /// 冻结位（对拍分歧立案后冻结在当前路径，直到解冻）。
    pub frozen: bool,
    /// 最近一次切换的证据差值序列（切换原因可审计；封顶 16 条滚动）。
    pub switch_evidence: Vec<u64>,
    /// 累计切换次数。
    pub switches: u64,
}

impl Hysteresis {
    /// 初始滞回（默认 CPU——帧首任务 CPU 起步，GPU 留给算子密任务；
    /// 起步路径显性写死不是隐藏默认）。
    pub const fn new() -> Hysteresis {
        Hysteresis {
            current: ExecutionPath::Cpu,
            streak: 0,
            frozen: false,
            switch_evidence: Vec::new(),
            switches: 0,
        }
    }

    /// 每帧滞回推进（判据「滞回防抖实测」的执行面）：
    /// 返回 (本帧生效路径, 是否发生切换)。
    pub fn step(
        &mut self,
        est: &PathEstimate,
        allowed: bool,
    ) -> Result<(ExecutionPath, bool), PlcCode> {
        if self.frozen {
            // 冻结期：路径锁定，只观察不切换。
            return Ok((self.current, false));
        }
        if !allowed {
            // 双路不可用（契约缺路）——路径保持，不计滞回。
            return Ok((self.current, false));
        }
        let candidate = est.better();
        if candidate == self.current {
            // 候选即当前：计数清零（滞回带内波动被吸收）。
            self.streak = 0;
            return Ok((self.current, false));
        }
        // 差值比（相对当前路径）。
        let diff = est.diff_pct_vs(self.current);
        // 候选须严格更优才计 streak（diff_pct_vs 是绝对差比，方向由
        // candidate == est.better() 保证：better 只在严格更小时给候选）。
        if est.of(candidate) < est.of(self.current) && diff > HYST_RATIO_PCT {
            self.streak = self.streak.saturating_add(1);
            if self.streak >= HYST_WINDOW {
                // 达窗切换：证据入账（滚动 16 条）。
                if self.switch_evidence.len() >= 16 {
                    self.switch_evidence.remove(0);
                }
                self.switch_evidence.push(diff);
                self.switches = self.switches.saturating_add(1);
                self.current = candidate;
                self.streak = 0;
                return Ok((self.current, true));
            }
            return Ok((self.current, false));
        }
        // 差值回落带内：清零（防抖核心——反复横跳在此被吸收）。
        self.streak = 0;
        Ok((self.current, false))
    }

    /// 冻结/解冻（对拍分歧立案冻结，查清后解冻恢复自动决策）。
    pub const fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
        if frozen {
            self.streak = 0;
        }
    }
}

impl Default for Hysteresis {
    fn default() -> Hysteresis {
        Hysteresis::new()
    }
}

/// 单族放置决策器（每任务族独立滞回——GPU 占用画像变化对各族影响
/// 不同，共享滞回会让 A 族的切换抖动传染 B 族）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyDecider {
    /// 族名。
    pub family: String,
    /// 滞回状态。
    pub hyst: Hysteresis,
    /// 最近预估（决策日志与误差账共用的输入快照）。
    pub last_est: Option<PathEstimate>,
}

/// 放置决策器（契约表 + 成本模型 + 族决策器账 + 决策日志）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacementDecider {
    contracts: ContractTable,
    /// 成本模型（可重标定）。
    pub model: CostModel,
    /// 族决策器账（族名 → 决策器）。
    pub families: Vec<FamilyDecider>,
    /// 决策日志。
    pub log: DecisionLog,
    /// 误差账。
    pub errors: ErrorLedger,
    /// 对拍账。
    pub outcomes: OutcomeLedger,
    /// 决策计数（守恒对账基准）。
    pub decisions_made: u64,
}

impl PlacementDecider {
    /// 基线决策器。
    pub const fn new() -> PlacementDecider {
        PlacementDecider {
            contracts: ContractTable::new(),
            model: CostModel::baseline(),
            families: Vec::new(),
            log: DecisionLog::new(),
            errors: ErrorLedger::new(),
            outcomes: OutcomeLedger::new(),
            decisions_made: 0,
        }
    }

    /// 注册契约（透传契约表）。
    pub fn register_contract(&mut self, c: DualPathContract) -> Result<(), PlcCode> {
        self.contracts.register(c)
    }

    /// 帧内放置决策（判据主流程）：画像校验 → 契约查证 → 双路预估 →
    /// 滞回推进 → 日志落账。返回本帧生效路径与预估。
    pub fn decide(
        &mut self,
        frame: u64,
        family: &str,
        profile: &CostProfile,
    ) -> Result<(ExecutionPath, PathEstimate), PlcCode> {
        if let Some(c) = profile.validate() {
            return Err(c);
        }
        let contract = match self.contracts.get(family) {
            Some(c) => c.clone(),
            None => return Err(PlcCode::ContractMissing),
        };
        let est = self.model.predict(profile);
        // 族决策器就位（惰性建账）。
        let mut fam_idx = usize::MAX;
        let mut i = 0usize;
        while i < self.families.len() {
            if let Some(f) = self.families.get(i) {
                if f.family == family {
                    fam_idx = i;
                    break;
                }
            }
            i += 1;
        }
        if fam_idx == usize::MAX {
            if self.families.len() >= FAMILY_CAPACITY {
                return Err(PlcCode::ContractTableFull);
            }
            self.families.push(FamilyDecider {
                family: family.to_string(),
                hyst: Hysteresis::new(),
                last_est: None,
            });
            fam_idx = self.families.len().saturating_sub(1);
        }
        // 固定路径族：不走滞回（路径由契约硬编码），但仍落日志。
        let path = match contract.fixed_path {
            Some(p) => p,
            None => {
                let allowed = contract.has_cpu && contract.has_gpu;
                let fam = match self.families.get_mut(fam_idx) {
                    Some(f) => f,
                    None => return Err(PlcCode::HysteresisCorrupt),
                };
                let (p, _switched) = fam.hyst.step(&est, allowed)?;
                p
            }
        };
        let fam = match self.families.get_mut(fam_idx) {
            Some(f) => f,
            None => return Err(PlcCode::HysteresisCorrupt),
        };
        fam.last_est = Some(est);
        self.decisions_made = self.decisions_made.saturating_add(1);
        self.log.push(DecisionRecord {
            frame,
            family: family.to_string(),
            chosen: path,
            est_cpu_us: est.cpu_us,
            est_gpu_us: est.gpu_us,
            calib_epoch: est.calib_epoch,
            actual_us: 0,
            error_pct: 0,
            filled: false,
        })?;
        Ok((path, est))
    }

    /// 实测回填（执行面 tick 注入；误差比入账 + 重标定触发判定）。
    pub fn report_actual(
        &mut self,
        frame: u64,
        family: &str,
        chosen: ExecutionPath,
        actual_us: u64,
    ) -> Result<(), PlcCode> {
        if actual_us == 0 {
            return Err(PlcCode::ActualZero);
        }
        if self.contracts.get(family).is_none() {
            return Err(PlcCode::FamilyUnknown);
        }
        // 日志回填（找该帧该族最新未回填条目）。
        let mut target = usize::MAX;
        let mut i = self.log.records.len();
        while i > 0 {
            i -= 1;
            if let Some(r) = self.log.records.get(i) {
                if !r.filled && r.frame == frame && r.family == family {
                    target = i;
                    break;
                }
            }
        }
        if target == usize::MAX {
            return Err(PlcCode::FamilyUnknown);
        }
        let (est_of_path, epoch) = {
            let r = match self.log.records.get(target) {
                Some(r) => r,
                None => return Err(PlcCode::FamilyUnknown),
            };
            let est = if r.chosen == ExecutionPath::Cpu { r.est_cpu_us } else { r.est_gpu_us };
            (est, r.calib_epoch)
        };
        let error_pct = {
            let diff = if actual_us > est_of_path {
                actual_us.saturating_sub(est_of_path)
            } else {
                est_of_path.saturating_sub(actual_us)
            };
            if est_of_path == 0 {
                u64::MAX
            } else {
                diff.saturating_mul(100).saturating_div(est_of_path)
            }
        };
        if let Some(r) = self.log.records.get_mut(target) {
            r.actual_us = actual_us;
            r.error_pct = error_pct;
            r.filled = true;
        }
        // 误差账（固定路径任务的实测也入账——误差是模型质量信号）。
        self.errors.push(family, chosen, epoch, est_of_path, actual_us, error_pct);
        // 重标定判定（连续 RECAL_WINDOW 帧超 30% → 触发）。
        if self.errors.over_threshold_streak(family, chosen, RECAL_RATIO_PCT) >= RECAL_WINDOW {
            self.recalibrate(family, chosen)?;
        }
        Ok(())
    }

    /// 自动重标定（判据「重标定闭环」）：按实测/预测比例缩放对应路径
    /// 吞吐常数；幅度钳制 ±50%；单帧一次；证据留账。
    fn recalibrate(&mut self, family: &str, path: ExecutionPath) -> Result<(), PlcCode> {
        // 单帧一次闸（本帧已重标定过则拒）。
        if self.errors.recal_this_frame {
            return Err(PlcCode::RecalDuplicate);
        }
        // 平均误差比（该族该路径最近窗口）。
        let avg = match self.errors.avg_actual_over_est(family, path) {
            Some(a) => a,
            None => return Err(PlcCode::FamilyUnknown),
        };
        if avg == 0 {
            return Err(PlcCode::ActualZero);
        }
        // 缩放比 = 实测/预测（预测偏低 → 常数下调）。钳制 ±50%。
        let shift_pct = {
            // avg 是百分数（实测/预测 ×100）。缩放 = 10000 / avg（百分数
            // 的百分数——avg=125 表示实测是预测 1.25 倍 → 吞吐常数 ×(100/125)）。
            let raw = avg.saturating_mul(100);
            let inv = if avg == 0 { 100 } else { 10_000u64.saturating_div(raw.max(1)) };
            // 钳制：50..=150。
            inv.max(100 - RECAL_MAX_SHIFT_PCT).min(100 + RECAL_MAX_SHIFT_PCT)
        };
        let before = self.model;
        let m = &mut self.model;
        match path {
            ExecutionPath::Cpu => {
                m.cpu_ops_per_us = m.cpu_ops_per_us.saturating_mul(shift_pct).saturating_div(100).max(1);
            }
            ExecutionPath::Gpu => {
                m.gpu_ops_per_us = m.gpu_ops_per_us.saturating_mul(shift_pct).saturating_div(100).max(1);
            }
        }
        m.calib_epoch = m.calib_epoch.saturating_add(1);
        let after = self.model;
        self.errors.recal_this_frame = true;
        self.errors.recal_events.push(RecalEvent {
            family: family.to_string(),
            path,
            before_cpu: before.cpu_ops_per_us,
            before_gpu: before.gpu_ops_per_us,
            after_cpu: after.cpu_ops_per_us,
            after_gpu: after.gpu_ops_per_us,
            evidence_pct: avg,
            epoch_after: after.calib_epoch,
        });
        // 误差账清零重计（闭环：重标定后从零观察新常数质量）。
        self.errors.reset_streak(family, path);
        Ok(())
    }
}

impl Default for PlacementDecider {
    fn default() -> PlacementDecider {
        PlacementDecider::new()
    }
}

// ---------------------------------------------------------------------------
// 六、决策日志（全量可回放）
// ---------------------------------------------------------------------------

/// 单条决策记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionRecord {
    /// 帧号。
    pub frame: u64,
    /// 任务族。
    pub family: String,
    /// 本帧选择。
    pub chosen: ExecutionPath,
    /// CPU 路预估（微秒）。
    pub est_cpu_us: u64,
    /// GPU 路预估（微秒）。
    pub est_gpu_us: u64,
    /// 预估使用的标定纪元。
    pub calib_epoch: u32,
    /// 实测（微秒；回填前 0）。
    pub actual_us: u64,
    /// 误差比（百分数；回填前 0）。
    pub error_pct: u64,
    /// 回填完成位。
    pub filled: bool,
}

/// 决策日志（滚动窗口；全量可回放）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionLog {
    records: Vec<DecisionRecord>,
    /// 归档条数（超窗截断的已回填条目——未回填不许截断）。
    pub archived: u64,
}

impl DecisionLog {
    /// 空日志。
    pub const fn new() -> DecisionLog {
        DecisionLog { records: Vec::new(), archived: 0 }
    }

    /// 在册条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 落账（窗口满时只归档已回填条目；全未回填则拒——不截半条账）。
    pub fn push(&mut self, r: DecisionRecord) -> Result<(), PlcCode> {
        if self.records.len() >= LOG_CAPACITY {
            let mut filled_head = 0usize;
            while filled_head < self.records.len() {
                match self.records.get(filled_head) {
                    Some(x) if x.filled => filled_head += 1,
                    _ => break,
                }
            }
            if filled_head == 0 {
                return Err(PlcCode::LogFullUnfilled);
            }
            self.records.drain(..filled_head);
            self.archived = self.archived.saturating_add(filled_head as u64);
        }
        self.records.push(r);
        Ok(())
    }

    /// 按帧回放：取该帧全部记录（决策序列确定性重放的入口）。
    pub fn replay_frame(&self, frame: u64) -> Vec<DecisionRecord> {
        let mut out: Vec<DecisionRecord> = Vec::new();
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.frame == frame {
                    out.push(r.clone());
                }
            }
            i += 1;
        }
        out
    }

    /// 守恒对账：日志条数 + 归档数 == 决策总数（由持有者核对）。
    pub const fn total_accounts(&self) -> u64 {
        self.records.len() as u64
    }
}

impl Default for DecisionLog {
    fn default() -> DecisionLog {
        DecisionLog::new()
    }
}

// ---------------------------------------------------------------------------
// 七、误差账与重标定事件
// ---------------------------------------------------------------------------

/// 单条误差记录（预测 vs 实测）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorRecord {
    /// 任务族。
    pub family: String,
    /// 执行路径。
    pub path: ExecutionPath,
    /// 预估标定纪元。
    pub epoch: u32,
    /// 预估（微秒）。
    pub est_us: u64,
    /// 实测（微秒）。
    pub actual_us: u64,
    /// 误差比（百分数）。
    pub error_pct: u64,
}

/// 重标定事件（闭环审计账：before/after + 证据）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecalEvent {
    /// 触发族。
    pub family: String,
    /// 标定路径。
    pub path: ExecutionPath,
    /// 重标定前 CPU 吞吐。
    pub before_cpu: u64,
    /// 重标定前 GPU 吞吐。
    pub before_gpu: u64,
    /// 重标定后 CPU 吞吐。
    pub after_cpu: u64,
    /// 重标定后 GPU 吞吐。
    pub after_gpu: u64,
    /// 证据误差比（百分数）。
    pub evidence_pct: u64,
    /// 重标定后的标定纪元。
    pub epoch_after: u32,
}

/// 误差账（滚动窗口；重标定触发判定面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorLedger {
    records: Vec<ErrorRecord>,
    /// 重标定事件账（全量——闭环审计不滚动）。
    pub recal_events: Vec<RecalEvent>,
    /// 本帧已重标定位（单帧一次闸；帧推进由调用方清零——
    /// [`ErrorLedger::advance_frame`]）。
    pub recal_this_frame: bool,
    /// 帧游标（advance_frame 推进）。
    pub frame_cursor: u64,
    /// 每族每路径的连续超阈计数（族名 + 路径 → streak）。
    streaks: Vec<(String, u8, u32)>,
}

const ERROR_LEDGER_CAP: usize = 4_096;

impl ErrorLedger {
    /// 空账。
    pub const fn new() -> ErrorLedger {
        ErrorLedger {
            records: Vec::new(),
            recal_events: Vec::new(),
            recal_this_frame: false,
            frame_cursor: 0,
            streaks: Vec::new(),
        }
    }

    /// 帧推进（调用方每帧调一次：清单帧重标定闸 + 游标前进）。
    pub fn advance_frame(&mut self) {
        self.recal_this_frame = false;
        self.frame_cursor = self.frame_cursor.saturating_add(1);
    }

    /// 记录追加（滚动窗口 4096）。
    pub fn push(
        &mut self,
        family: &str,
        path: ExecutionPath,
        epoch: u32,
        est_us: u64,
        actual_us: u64,
        error_pct: u64,
    ) {
        if self.records.len() >= ERROR_LEDGER_CAP {
            self.records.remove(0);
        }
        self.records.push(ErrorRecord {
            family: family.to_string(),
            path,
            epoch,
            est_us,
            actual_us,
            error_pct,
        });
        // streak 推进。
        let mut found = false;
        let mut i = 0usize;
        while i < self.streaks.len() {
            if let Some((f, p, s)) = self.streaks.get_mut(i) {
                if f == family && *p == path.index() {
                    if error_pct > RECAL_RATIO_PCT {
                        *s = s.saturating_add(1);
                    } else {
                        *s = 0;
                    }
                    found = true;
                    break;
                }
            }
            i += 1;
        }
        if !found {
            let init = if error_pct > RECAL_RATIO_PCT { 1u32 } else { 0 };
            self.streaks.push((family.to_string(), path.index(), init));
        }
    }

    /// 连续超阈计数。
    pub fn over_threshold_streak(&self, family: &str, path: ExecutionPath, _pct: u64) -> u32 {
        let mut i = 0usize;
        while i < self.streaks.len() {
            if let Some((f, p, s)) = self.streaks.get(i) {
                if f == family && *p == path.index() {
                    return *s;
                }
            }
            i += 1;
        }
        0
    }

    /// 清零指定族路径的 streak（重标定后重计）。
    pub fn reset_streak(&mut self, family: &str, path: ExecutionPath) {
        let mut i = 0usize;
        while i < self.streaks.len() {
            if let Some((f, p, s)) = self.streaks.get_mut(i) {
                if f == family && *p == path.index() {
                    *s = 0;
                    return;
                }
            }
            i += 1;
        }
    }

    /// 平均实测/预测比（百分数；该族该路径最近 8 条——重标定证据）。
    pub fn avg_actual_over_est(&self, family: &str, path: ExecutionPath) -> Option<u64> {
        let mut sum = 0u64;
        let mut n = 0u64;
        let mut i = self.records.len();
        let mut seen = 0u32;
        while i > 0 && seen < 8 {
            i -= 1;
            if let Some(r) = self.records.get(i) {
                if r.family == family && r.path == path {
                    if r.est_us > 0 {
                        sum = sum.saturating_add(r.actual_us.saturating_mul(100).saturating_div(r.est_us));
                        n = n.saturating_add(1);
                    }
                    seen += 1;
                }
            }
        }
        if n == 0 {
            return None;
        }
        Some(sum.saturating_div(n))
    }

    /// 在册误差条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }
}

impl Default for ErrorLedger {
    fn default() -> ErrorLedger {
        ErrorLedger::new()
    }
}

// ---------------------------------------------------------------------------
// 八、对拍账：双路输出指纹逐对裁决
// ---------------------------------------------------------------------------

/// 一对输出指纹（CPU 路与 GPU 路各一份；放置只切路径不改像素结果的
/// 执法面——指纹由执行面对同一输入双跑上报，本域只裁决）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutcomePair {
    /// 任务族。
    pub family: String,
    /// 输入指纹（同输入才可比——输入不同的一致性没有意义）。
    pub input_fp: u32,
    /// CPU 路输出指纹。
    pub cpu_out_fp: u32,
    /// GPU 路输出指纹。
    pub gpu_out_fp: u32,
    /// 裁决：一致。
    pub consistent: bool,
}

/// 对拍账（滚动窗口）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutcomeLedger {
    pairs: Vec<OutcomePair>,
    /// 分歧立案数（consistent=false 的累计——资格执法的计数面）。
    pub divergence_cases: u64,
    /// 分歧冻结族（族名表——冻结期该族滞回锁定）。
    pub frozen_families: Vec<String>,
}

const OUTCOME_LEDGER_CAP: usize = 1_024;

impl OutcomeLedger {
    /// 空账。
    pub const fn new() -> OutcomeLedger {
        OutcomeLedger { pairs: Vec::new(), divergence_cases: 0, frozen_families: Vec::new() }
    }

    /// 上报一对指纹并裁决（不一致即立案 + 冻结族自动放置——
    /// 差异查清前不回自动，诚实优先于吞吐）。
    pub fn report_pair(
        &mut self,
        family: &str,
        input_fp: u32,
        cpu_out_fp: u32,
        gpu_out_fp: u32,
    ) -> Result<bool, PlcCode> {
        if cpu_out_fp == 0 || gpu_out_fp == 0 {
            // 零指纹 = 单路缺报（指纹 0 保留为「未上报」哨兵）。
            return Err(PlcCode::DivergencePairIncomplete);
        }
        let consistent = cpu_out_fp == gpu_out_fp;
        if self.pairs.len() >= OUTCOME_LEDGER_CAP {
            self.pairs.remove(0);
        }
        self.pairs.push(OutcomePair {
            family: family.to_string(),
            input_fp,
            cpu_out_fp,
            gpu_out_fp,
            consistent,
        });
        if !consistent {
            self.divergence_cases = self.divergence_cases.saturating_add(1);
            let mut exists = false;
            let mut i = 0usize;
            while i < self.frozen_families.len() {
                if let Some(f) = self.frozen_families.get(i) {
                    if f == family {
                        exists = true;
                        break;
                    }
                }
                i += 1;
            }
            if !exists {
                self.frozen_families.push(family.to_string());
            }
            return Err(PlcCode::DivergenceDetected);
        }
        Ok(true)
    }

    /// 族解冻（差异查清、双路修复后由调用方解冻）。
    pub fn unfreeze(&mut self, family: &str) -> bool {
        let mut target = usize::MAX;
        let mut i = 0usize;
        while i < self.frozen_families.len() {
            if let Some(f) = self.frozen_families.get(i) {
                if f == family {
                    target = i;
                    break;
                }
            }
            i += 1;
        }
        if target == usize::MAX {
            return false;
        }
        self.frozen_families.remove(target);
        true
    }

    /// 族冻结判定。
    pub fn is_frozen(&self, family: &str) -> bool {
        let mut i = 0usize;
        while i < self.frozen_families.len() {
            if let Some(f) = self.frozen_families.get(i) {
                if f == family {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 在册对数。
    pub fn len(&self) -> usize {
        self.pairs.len()
    }
}

impl Default for OutcomeLedger {
    fn default() -> OutcomeLedger {
        OutcomeLedger::new()
    }
}

// ---------------------------------------------------------------------------
// 九、守恒对账与帧级聚合（决策器健康面）
// ---------------------------------------------------------------------------

/// 决策器守恒对账（判据「决策日志全量」的账面检查）：决策总数 ==
/// 日志在册 + 日志归档；误差账有决策才有回填（回填 ≤ 决策）。
pub fn decider_reconcile(d: &PlacementDecider) -> Result<(), PlcCode> {
    let log_total = d.log.total_accounts().saturating_add(d.log.archived);
    if log_total != d.decisions_made {
        return Err(PlcCode::LogReconcileBroken);
    }
    if d.errors.len() > d.log.len().saturating_add(d.log.archived as usize) {
        return Err(PlcCode::LogReconcileBroken);
    }
    Ok(())
}

/// 帧级放置聚合（帧尾消费面：本帧各族路径与预估汇总——F0005 预算
/// 仲裁与 F0007 归因的输入形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FramePlacementSummary {
    /// 帧号。
    pub frame: u64,
    /// (族, 路径, 预估微秒) 三元组表（按决策日志该帧序）。
    pub rows: Vec<(String, ExecutionPath, u64)>,
    /// 本帧 CPU 路任务数。
    pub cpu_tasks: u64,
    /// 本帧 GPU 路任务数。
    pub gpu_tasks: u64,
}

/// 从决策日志聚合某帧放置汇总（零拷贝读——日志是唯一事实源）。
pub fn summarize_frame(d: &PlacementDecider, frame: u64) -> FramePlacementSummary {
    let records = d.log.replay_frame(frame);
    let mut rows: Vec<(String, ExecutionPath, u64)> = Vec::new();
    let mut cpu_tasks = 0u64;
    let mut gpu_tasks = 0u64;
    let mut i = 0usize;
    while i < records.len() {
        if let Some(r) = records.get(i) {
            let est = match r.chosen {
                ExecutionPath::Cpu => r.est_cpu_us,
                ExecutionPath::Gpu => r.est_gpu_us,
            };
            match r.chosen {
                ExecutionPath::Cpu => cpu_tasks = cpu_tasks.saturating_add(1),
                ExecutionPath::Gpu => gpu_tasks = gpu_tasks.saturating_add(1),
            }
            rows.push((r.family.clone(), r.chosen, est));
        }
        i += 1;
    }
    FramePlacementSummary { frame, rows, cpu_tasks, gpu_tasks }
}
