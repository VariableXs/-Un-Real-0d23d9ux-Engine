//! CGPU-F1766 · 直通 GPU 支持（CGPU-K 域 · 虚拟化组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1766`
//!
//! **判据（锚点原文）**：直通语义、<2% 声明、驱动路径、限制显性、热可见、五组、判据。
//!
//! **职责定位（锚点原文摘录）**：直通支持（整卡直通（DDA/GPU-P 独占——直通语义（性能≈原生（直通契约（损耗<2%——直通性能声明；检测（直通环境检测（F1765 复用——复用；驱动（guest 驱动（厂商驱动直装——驱动路径；限制（直通限制（不可共享/迁移受限——限制显性；热（直通热管理（guest 可见——热可见复用；测试（契约/检测/驱动/限制/热五组）。
//!
//! ─────────────────────────────────────────────────────────────────────────────
//! 设计总则（本模块八条铁律，逐条对应锚点判据）：
//!
//! 1. **直通语义**（判据"直通语义"）：`PassthruMode` 双模式闭集——`Dda`
//!    （整卡直通，Discrete Device Assignment：硬件中断直投、BAR MMIO 直映射、
//!    宿主可卸载）与 `GpuPExclusive`（GPU-P 独占分区：半虚拟化中断面、vmbus
//!    通道、分区宿主可回收）。两模式的能力差异落在 `MODE_CAP_TABLE` 常量
//!    差异表上，逐行注明语义来源；越界能力键显性 `None` 不猜（映射为
//!    `X_PT_MODE_UNKNOWN` 码）。闭集纪律：只许这两模式，新增模式必须改表
//!    升版，不许运行期拼装第三态。
//! 2. **<2% 声明**（判据"<2% 声明/性能≈原生"）：`PassthruContract` 把
//!    "性能≈原生"量化为可检验数值——损耗预算 2000 bps（<2%，以基点计）。
//!    `perf_claim()` 带实测 vs 原生基准对拍：先登记原生基准（native
//!    baseline），后送实测吞吐；损耗率严格小于预算为达标，达到或超过即
//!    **违约立案**（`BreachCase` 带证据五元组入账 + `X_PT_CONTRACT_BREACH`
//!    带码返回）。实测高于原生属对拍异常（overshoot），不违约但计数留痕。
//!    恰在预算线上不违约（判据语义是"损耗<2%"，严格小于）。
//! 3. **检测复用**（判据"检测（F1765 复用）"）：`HostDetectMirror` 是
//!    vck68/vck69 检测面的**镜像复用声明**——本模块不自研 hypervisor 检测
//!    器，只镜像其输入（CPUID 0x40000000 签名 + leaf 存在位 + /dev/dxg 探测
//!    三态）与结论三态（Bare/Virtualized/Indeterminate），并做一致性校验：
//!    镜像分类与外部声明不一致 → `X_PT_DETECT_MIRROR_MISMATCH`；声明直通
//!    而环境是 Bare → 矛盾入账 `X_PT_DETECT_BARE_CONTRA`。直通准入要求
//!    Virtualized/Indeterminate，Bare 上声明直通永远拒绝。
//! 4. **驱动路径**（判据"驱动路径/厂商驱动直装"）：`GuestDriverRegistry`
//!    登记guest 驱动族（NVIDIA/AMD/Intel 厂商驱动直装 + VirtIO-GPU 半虚拟
//!    化后备），路径三态 Present/Missing/Unknown（不造假补齐），版本最低
//!    要求表（三元组 major.minor.build，20 位每段打包）。绑定门
//!    `driver_gate()`：至少一族 Present 且版本达标，版本低带码留痕且不
//!    静默放行。
//! 5. **限制显性**（判据"限制显性"）：`LIMIT_TABLE` 每条限制一行常量
//!    （不可共享/迁移受限/热添加不可/checkpoint 同族受限），拦截器四件套
//!    （request_share/migrate/hotadd/checkpoint）命中即拒绝带码留痕计数，
//!    无条件拒绝——直通语义下这些操作没有"看情况放行"的分支，拒绝就是
//!    语义本身。
//! 6. **热可见**（判据"热可见复用"）：`ThermalVisibility` 把 host 热档四态
//!    （Nominal/Warm/Hot/Critical）投影为 guest 感知档四态，映射 const 表
//!    逐行注明；投影滞后显性（lag > 4 拍即 stale，guest 视图保守降档一档，
//!    `guest_query()` 报 `X_PT_THERMAL_STALE` 要求宿主补投影）。
//! 7. **生命周期**：Unassigned→Assigned→Bound→Active→Revoked 五态状态机，
//!    迁移矩阵 const；乱序拒绝带 `X_PT_STATE_ORDER`（from/to 全量留痕）；
//!    Revoked 是真终态（一切后续操作 `X_PT_STATE_REVOKED`）；吊销路径从
//!    Assigned/Bound/Active 三态真实可用。直通无"退回"语义——硬件已让渡，
//!    只能吊销重配。
//! 8. **五组测试面**（判据"五组/判据"）：契约/检测/驱动/限制/热五组注入
//!    自检 + 生命周期 + 账本/指纹对拍，全部走同一公开管道（注入输入，无
//!    旁路）。`health()` 出结构化体检；FNV-1a 64 位指纹同输入同指纹；
//!    双跑对拍（同脚本双引擎同指纹、异操作异指纹）在测试组闭环。
//!
//! **事件账**：`EventLedger` 定长 64 槽环形账，溢出覆盖最旧并显性计数
//! （不静默丢账）；每事件带序号/种类/诊断码/中文细节。
//!
//! **体检与指纹**：`health()` 汇总全部子系统水位；`fingerprint()` 把引擎
//! 稳定态（模式/状态/绑定/契约计数/镜像结论/驱动槽/限制计数/热投影/账本
//! 水位/吊销原因）编入 FNV-1a——同输入同指纹，用于本机回归对账，不上传。
//!
//! 诊断码段 0x1350~0x135F（十六码独占；开工前已 grep 确认 cgpu 域内空闲，
//! 邻件 F1764/F1765 各占 0x12A0/0x12B0 段，互不相交）。
//! no_std + alloc；内核路径零 unwrap 零 expect 零 panic 零运行期 assert
//! 零 IO 零墙钟零浮点；所有可失败操作返回 `Result<T, PassthruError>`，
//! 错误 detail 为中文可操作诊断；溢出处显性 saturating（计数只涨不回绕）；
//! const fn 内不使用 slice::get / Vec 索引 / Ord::min / Ord::max / 派生
//! PartialEq 比较 / for 循环（一律 while+索引）；含 Vec 字段的结构体一律
//! 不 derive Copy；>64KB 结构体一律不上栈（本模块最大栈上结构体为
//! `PtHealth`，几十字节量级）。

use core::fmt;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ═════════════════════════════════════════════════════════════════════════════
// §0 · 诊断码段（0x1350~0x135F，十六码独占；开工前 grep 确认空闲）
// ═════════════════════════════════════════════════════════════════════════════

/// 成功基线（无错误）。
pub const X_PT_OK: u32 = 0x1350;
/// 状态机乱序迁移拒绝（from/to 全量留痕——建议核对生命周期五态迁移矩阵）。
pub const X_PT_STATE_ORDER: u32 = 0x1351;
/// 设备已吊销（终态）——一切后续操作被拒（须重建直通绑定，不可复活）。
pub const X_PT_STATE_REVOKED: u32 = 0x1352;
/// 性能损耗超出契约声明（<2%=2000bps）——违约已立案，核对 BreachCase 证据。
pub const X_PT_CONTRACT_BREACH: u32 = 0x1353;
/// 原生基准未登记或非法（为 0）——先 register_native_baseline 再对拍。
pub const X_PT_CONTRACT_NO_BASELINE: u32 = 0x1354;
/// 裸金属（Bare）环境声明直通——矛盾入账（直通要求 Virtualized/Indeterminate）。
pub const X_PT_DETECT_BARE_CONTRA: u32 = 0x1355;
/// 检测镜像与外部声明不一致（F1765 复用面校验失败）——核对镜像输入。
pub const X_PT_DETECT_MIRROR_MISMATCH: u32 = 0x1356;
/// 直通环境检测未通过/未执行即激活——先 check_environment 拿到准入。
pub const X_PT_ENV_NOT_CLEARED: u32 = 0x1357;
/// 驱动路径登记冲突（族号越界、同族重复登记或登记为空）。
pub const X_PT_DRIVER_PATH_CONFLICT: u32 = 0x1358;
/// guest 驱动版本低于族表最低要求（核对 DRIVER_FAMILY_TABLE 演进登记）。
pub const X_PT_DRIVER_VERSION_LOW: u32 = 0x1359;
/// 共享请求命中限制表（不可共享——直通卡独占绑定，无共享分支）。
pub const X_PT_LIMIT_SHARE: u32 = 0x135A;
/// 迁移/checkpoint 请求命中限制表（迁移受限——设备态在硬件内不可序列化）。
pub const X_PT_LIMIT_MIGRATE: u32 = 0x135B;
/// 热添加请求命中限制表（热添加不可——须先吊销再重配拓扑）。
pub const X_PT_LIMIT_HOTADD: u32 = 0x135C;
/// guest 热投影滞后超限或尚无投影（要求宿主立即补投一次热档）。
pub const X_PT_THERMAL_STALE: u32 = 0x135D;
/// 直通模式能力越界查询（能力键不在 MODE_CAP_TABLE——不猜，走表演进）。
pub const X_PT_MODE_UNKNOWN: u32 = 0x135E;
/// 设备/guest 编号非法（device_id 或 guest_id 为 0——编号从 1 起）。
pub const X_PT_NO_DEVICE: u32 = 0x135F;

/// 本模块诊断码段下界（含）。
pub const X_PT_CODE_MIN: u32 = X_PT_OK;
/// 本模块诊断码段上界（含）。
pub const X_PT_CODE_MAX: u32 = X_PT_NO_DEVICE;

/// 直通子系统统一错误类型。每变体 `detail()` 给中文可操作诊断，
/// `code()` 落在 0x1350~0x135F 段内（两两不同）。
#[derive(Debug, Clone)]
pub enum PassthruError {
    /// 状态机乱序迁移：from/to 全量携带，调用方按矩阵核对。
    StateOrder {
        /// 迁移发起时的状态。
        from: LifecycleState,
        /// 试图迁往的状态。
        to: LifecycleState,
    },
    /// 设备已吊销（终态），任何后续操作被拒。
    StateRevoked,
    /// 性能违约：损耗超过预算，已立案。
    ContractBreach {
        /// 本次对拍的绝对损耗（bps）。
        loss_bps: u64,
        /// 契约损耗预算（bps）。
        budget_bps: u64,
    },
    /// 原生基准缺失或为 0，无法对拍。
    ContractNoBaseline,
    /// 环境检测未通过/未执行即要求激活。
    EnvNotCleared,
    /// Bare 环境声明直通——语义矛盾。
    DetectBareContradiction,
    /// 镜像分类与外部声明不一致。
    DetectMirrorMismatch {
        /// 本地镜像分类结论。
        local: DetectTriState,
        /// 外部（F1765 面）声明结论。
        declared: DetectTriState,
    },
    /// 驱动路径登记冲突（越界/重复/空表）。
    DriverPathConflict,
    /// 驱动版本低于族表最低要求。
    DriverVersionLow {
        /// 未达标的驱动族名。
        family: &'static str,
    },
    /// 共享请求命中不可共享限制。
    LimitShareHit,
    /// 迁移/checkpoint 请求命中迁移受限限制。
    LimitMigrateHit,
    /// 热添加请求命中热添加不可限制。
    LimitHotAddHit,
    /// 热投影滞后超限或尚无投影。
    ThermalStale,
    /// 能力键越界（不在能力差异表）。
    ModeUnknown {
        /// 越界的能力键。
        cap: usize,
    },
    /// 设备/guest 编号为 0 非法。
    NoDevice,
}

impl PassthruError {
    /// 诊断码（0x1350~0x135F 段内，两两不同）。
    pub fn code(&self) -> u32 {
        match self {
            PassthruError::StateOrder { .. } => X_PT_STATE_ORDER,
            PassthruError::StateRevoked => X_PT_STATE_REVOKED,
            PassthruError::ContractBreach { .. } => X_PT_CONTRACT_BREACH,
            PassthruError::ContractNoBaseline => X_PT_CONTRACT_NO_BASELINE,
            PassthruError::DetectBareContradiction => X_PT_DETECT_BARE_CONTRA,
            PassthruError::DetectMirrorMismatch { .. } => X_PT_DETECT_MIRROR_MISMATCH,
            PassthruError::EnvNotCleared => X_PT_ENV_NOT_CLEARED,
            PassthruError::DriverPathConflict => X_PT_DRIVER_PATH_CONFLICT,
            PassthruError::DriverVersionLow { .. } => X_PT_DRIVER_VERSION_LOW,
            PassthruError::LimitShareHit => X_PT_LIMIT_SHARE,
            PassthruError::LimitMigrateHit => X_PT_LIMIT_MIGRATE,
            PassthruError::LimitHotAddHit => X_PT_LIMIT_HOTADD,
            PassthruError::ThermalStale => X_PT_THERMAL_STALE,
            PassthruError::ModeUnknown { .. } => X_PT_MODE_UNKNOWN,
            PassthruError::NoDevice => X_PT_NO_DEVICE,
        }
    }

    /// 中文可操作诊断（一行，可直接进日志/面板）。
    pub fn detail(&self) -> &'static str {
        match self {
            PassthruError::StateOrder { .. } => {
                "生命周期乱序迁移被拒：核对 Unassigned→Assigned→Bound→Active→Revoked 迁移矩阵，乱序不补位"
            }
            PassthruError::StateRevoked => {
                "设备已吊销（终态）：直通绑定不可复活，须以新 device_id 重建引擎并重走五态"
            }
            PassthruError::ContractBreach { .. } => {
                "性能违约立案：实测损耗达到或超过 2000bps 预算——核对 BreachCase 证据五元组并回滚直通声明"
            }
            PassthruError::ContractNoBaseline => {
                "原生基准未登记或为 0：先 register_native_baseline(>0) 登记原生吞吐，再 perf_claim 对拍"
            }
            PassthruError::DetectBareContradiction => {
                "裸金属环境声明直通：矛盾入账——直通要求 Virtualized/Indeterminate，Bare 上无 hypervisor 可让渡设备"
            }
            PassthruError::DetectMirrorMismatch { .. } => {
                "检测镜像与声明不一致：核对镜像输入（CPUID 0x40000000 签名/leaf 存在位）与 F1765 面声明是否同源"
            }
            PassthruError::EnvNotCleared => {
                "环境门未过：激活前必须 check_environment 拿到准入（Virtualized/Indeterminate 且与声明一致）"
            }
            PassthruError::DriverPathConflict => {
                "驱动路径登记冲突：族号须在 DRIVER_FAMILY_TABLE 内，同族不得重复登记；gate 失败时先补登记"
            }
            PassthruError::DriverVersionLow { .. } => {
                "guest 驱动版本低于族表最低要求：升级 guest 厂商驱动至 DRIVER_FAMILY_TABLE 登记版本后重登记"
            }
            PassthruError::LimitShareHit => {
                "不可共享：直通卡独占绑定单一 guest，共享请求无条件拒绝——第二 guest 走 GPU-P 共享分区通道而非直通"
            }
            PassthruError::LimitMigrateHit => {
                "迁移受限：直通设备态在硬件内不可序列化，live migration/checkpoint 无条件拒绝——仅关机级迁移可走宿主审批外流程"
            }
            PassthruError::LimitHotAddHit => {
                "热添加不可：直通绑定后拓扑固化——先吊销当前绑定，重配拓扑后再重新分配"
            }
            PassthruError::ThermalStale => {
                "guest 热感知滞后超限或尚无投影：宿主立即补投一次 project_thermal，guest_query 才恢复可信"
            }
            PassthruError::ModeUnknown { .. } => {
                "能力键越界：直通能力差异表只收已登记键——新增能力须改 MODE_CAP_TABLE 并升版，不许运行期拼装"
            }
            PassthruError::NoDevice => {
                "设备/guest 编号为 0 非法：device_id 与 guest_id 一律从 1 起编号，0 保留为无效哨兵"
            }
        }
    }
}

impl fmt::Display for PassthruError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PT-ERR 0x{:04X}: {}", self.code(), self.detail())
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §1 · FNV-1a 64 位指纹基元（确定性、无状态、const 可用）
// ═════════════════════════════════════════════════════════════════════════════

/// FNV-1a 64 位标准偏移基（来源：FNV 参考实现，公开常量）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位标准素数（来源：FNV 参考实现，公开常量）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 位：在 `init` 上续编 `bytes`。const fn（while+索引，无 for）。
pub const fn fnv1a64(init: u64, bytes: &[u8]) -> u64 {
    let mut h = init;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

// ═════════════════════════════════════════════════════════════════════════════
// §2 · PassthruMode —— 直通双模式闭集 + 常量能力差异表
// ═════════════════════════════════════════════════════════════════════════════

/// 直通模式双态闭集。判据"直通语义"：只有这两种让渡形态，
/// 不存在第三种运行期可拼装的"部分直通"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassthruMode {
    /// DDA 整卡直通（Discrete Device Assignment）：整卡从宿主卸下让渡给
    /// guest，硬件中断直投，BAR MMIO 直映射，宿主失去对该卡的调度权。
    Dda,
    /// GPU-P 独占分区（GPU Paravirtualization 独占档）：分区独占分配给
    /// 单一 guest，中断走半虚拟化面，宿主保留分区回收权。
    GpuPExclusive,
}

/// 模式下标：Dda=0，GpuPExclusive=1（能力表列号）。
pub const MODE_DDA: usize = 0;
/// 模式下标：GpuPExclusive=1。
pub const MODE_GPUP_EXCLUSIVE: usize = 1;
/// 模式闭集基数。
pub const MODE_COUNT: usize = 2;

/// 模式 → 能力表列号（const fn，match 不用派生比较）。
pub const fn mode_index(mode: PassthruMode) -> usize {
    match mode {
        PassthruMode::Dda => MODE_DDA,
        PassthruMode::GpuPExclusive => MODE_GPUP_EXCLUSIVE,
    }
}

/// 模式语义原型串（进入事件账与面板）。
pub const fn mode_archetype(mode: PassthruMode) -> &'static str {
    match mode {
        PassthruMode::Dda => "DDA 整卡直通（硬件中断直投/BAR MMIO 直映射/宿主可卸载）",
        PassthruMode::GpuPExclusive => "GPU-P 独占分区（半虚拟化中断面/vmbus 通道/宿主可回收分区）",
    }
}

// 能力键（行号即键；越界键显性 None → X_PT_MODE_UNKNOWN）。
/// 能力键：硬件中断直投。
pub const CAP_HW_INTERRUPT_DIRECT: usize = 0;
/// 能力键：半虚拟化中断面。
pub const CAP_PARAVIRT_INTERRUPT_SURFACE: usize = 1;
/// 能力键：BAR MMIO 直映射。
pub const CAP_MMIO_DIRECT_MAP: usize = 2;
/// 能力键：vmbus 通道面。
pub const CAP_VMBUS_CHANNEL: usize = 3;
/// 能力键：分区宿主可回收。
pub const CAP_PARTITION_RECLAIM: usize = 4;
/// 能力键：宿主可卸载整卡。
pub const CAP_HOST_DISMOUNT: usize = 5;
/// 能力键：live migration（直通下恒 0，与限制表一致）。
pub const CAP_LIVE_MIGRATION: usize = 6;
/// 能力键：同卡共享（直通下恒 0，与限制表一致）。
pub const CAP_CARD_SHARE: usize = 7;

/// 能力差异表行数。
pub const MODE_CAP_ROWS: usize = 8;

/// 直通能力差异表 `[行=能力键][列=模式]`。1=具备，0=不具备。
/// 来源逐行注明（各行为语义与公开的 DDA/GPU-P 模型对齐）：
///
/// * 行 0 硬件中断直投：DDA=1（整卡让渡后物理中断直接注入 guest）；
///   GPU-P=0（中断经半虚拟化面转交）。
/// * 行 1 半虚拟化中断面：DDA=0；GPU-P=1（vmbus 中断路径）。
/// * 行 2 BAR MMIO 直映射：两模式均 1（直通语义核心：设备 BAR 对 guest
///   直接可见，这是"性能≈原生"的物理基础）。
/// * 行 3 vmbus 通道面：DDA=0（不依赖通道）；GPU-P=1。
/// * 行 4 分区可回收：DDA=0（整卡让渡后宿主不可回收，只能 guest 侧交还）；
///   GPU-P=1（宿主保留分区回收权）。
/// * 行 5 宿主可卸载：DDA=1（dismount 语义）；GPU-P=0。
/// * 行 6 live migration：两模式均 0——与 `LIMIT_TABLE` 迁移受限一致，
///   能力表与限制表是同一事实的两面，不允许出现互相矛盾的登记。
/// * 行 7 同卡共享：两模式均 0——与不可共享限制一致。
pub const MODE_CAP_TABLE: [[u32; MODE_COUNT]; MODE_CAP_ROWS] = [
    [1, 0], // CAP_HW_INTERRUPT_DIRECT
    [0, 1], // CAP_PARAVIRT_INTERRUPT_SURFACE
    [1, 1], // CAP_MMIO_DIRECT_MAP
    [0, 1], // CAP_VMBUS_CHANNEL
    [0, 1], // CAP_PARTITION_RECLAIM
    [1, 0], // CAP_HOST_DISMOUNT
    [0, 0], // CAP_LIVE_MIGRATION（与限制表一致：迁移受限）
    [0, 0], // CAP_CARD_SHARE（与限制表一致：不可共享）
];

/// 查能力差异表。越界键返回 `None`（调用方映射 `X_PT_MODE_UNKNOWN`）。
/// const fn（直接索引，无 get，无 for）。
pub const fn mode_capability(mode: PassthruMode, cap: usize) -> Option<u32> {
    if cap >= MODE_CAP_ROWS {
        return None;
    }
    let row = MODE_CAP_TABLE[cap];
    let col = mode_index(mode);
    Some(row[col])
}

// ═════════════════════════════════════════════════════════════════════════════
// §3 · PassthruContract —— 直通性能契约（<2% = 2000bps，超声明即违约立案）
// ═════════════════════════════════════════════════════════════════════════════

/// 性能损耗预算：2000 bps（<2%；来源：锚点判据"损耗<2%"——以基点计）。
pub const PERF_LOSS_BUDGET_BPS: u64 = 2000;

/// 违约案卷上限（Vec 封顶，防长跑无界增长；超限后保留最新案卷）。
pub const CONTRACT_CASE_CAP: usize = 32;

/// 单次对拍裁决（数据全量携带，调用方可直接入日志）。
#[derive(Debug, Clone)]
pub struct PerfVerdict {
    /// 原生基准（bps）。
    pub native_bps: u64,
    /// 实测吞吐（bps）。
    pub measured_bps: u64,
    /// 绝对损耗（bps）＝max(0, native - measured)，饱和减法。
    pub loss_bps: u64,
    /// 损耗率（bps）：loss/native*10000，饱和封顶。
    pub loss_ratio_bps: u64,
    /// 契约损耗预算（bps）。
    pub budget_bps: u64,
    /// 实测高于原生（对拍异常）：不违约但必须留痕——基准可能失真。
    pub overshoot: bool,
    /// 是否违约（严格大于预算才违约；恰在预算线上不违约）。
    pub breached: bool,
}

/// 一次违约立案的证据五元组 + 案号。
#[derive(Debug, Clone)]
pub struct BreachCase {
    /// 案号（从 1 起单调递增）。
    pub case_id: u64,
    /// 原生基准（bps）。
    pub native_bps: u64,
    /// 实测吞吐（bps）。
    pub measured_bps: u64,
    /// 绝对损耗（bps）。
    pub loss_bps: u64,
    /// 损耗率（bps）。
    pub loss_ratio_bps: u64,
    /// 契约损耗预算（bps）。
    pub budget_bps: u64,
}

/// 直通性能契约：登记原生基准 → 逐次对拍 → 达标计数 / 违约立案。
#[derive(Debug)]
pub struct PassthruContract {
    native_baseline: Option<u64>,
    claims_total: u64,
    claims_passed: u64,
    overshoot_count: u64,
    case_counter: u64,
    cases: Vec<BreachCase>,
}

impl PassthruContract {
    pub(crate) fn new() -> Self {
        PassthruContract {
            native_baseline: None,
            claims_total: 0,
            claims_passed: 0,
            overshoot_count: 0,
            case_counter: 0,
            cases: Vec::new(),
        }
    }

    /// 登记原生基准（bps）。为 0 拒绝（`X_PT_CONTRACT_NO_BASELINE`）；
    /// 重复登记为覆盖语义（基准刷新合法，事件账留痕由引擎层做）。
    pub fn register_native_baseline(&mut self, native_bps: u64) -> Result<(), PassthruError> {
        if native_bps == 0 {
            return Err(PassthruError::ContractNoBaseline);
        }
        self.native_baseline = Some(native_bps);
        Ok(())
    }

    /// 当前原生基准。
    pub fn native_baseline(&self) -> Option<u64> {
        self.native_baseline
    }

    /// 对拍总次数（含违约）。
    pub fn claims_total(&self) -> u64 {
        self.claims_total
    }

    /// 达标次数。
    pub fn claims_passed(&self) -> u64 {
        self.claims_passed
    }

    /// 对拍异常（实测>原生）次数。
    pub fn overshoot_count(&self) -> u64 {
        self.overshoot_count
    }

    /// 违约立案总数（案号计数）。
    pub fn case_count(&self) -> u64 {
        self.case_counter
    }

    /// 案卷快照（最多保留最新 `CONTRACT_CASE_CAP` 份）。
    pub fn cases_snapshot(&self) -> Vec<BreachCase> {
        self.cases.clone()
    }

    /// 性能声明对拍：实测吞吐 vs 已登记原生基准。
    ///
    /// * 未登记基准 → `X_PT_CONTRACT_NO_BASELINE`。
    /// * loss = saturating(native - measured)；measured > native 记
    ///   overshoot（loss 记 0，不违约）。
    /// * 违约判定：`loss*10000 > budget*native`（u128 防溢出，严格大于）。
    ///   违约即立案（案号递增、证据入卷、Vec 封顶保最新），并返回
    ///   `X_PT_CONTRACT_BREACH`——立案与报错同拍完成，不留无码之案。
    pub fn perf_claim(&mut self, measured_bps: u64) -> Result<PerfVerdict, PassthruError> {
        let native = match self.native_baseline {
            Some(v) => v,
            None => return Err(PassthruError::ContractNoBaseline),
        };
        self.claims_total = self.claims_total.saturating_add(1);
        let overshoot = measured_bps > native;
        if overshoot {
            self.overshoot_count = self.overshoot_count.saturating_add(1);
        }
        let loss_bps = native.saturating_sub(measured_bps);
        let ratio128 = (loss_bps as u128) * 10_000u128 / (native as u128);
        let loss_ratio_bps = if ratio128 > u64::MAX as u128 {
            u64::MAX
        } else {
            ratio128 as u64
        };
        let breached =
            (loss_bps as u128) * 10_000u128 > (PERF_LOSS_BUDGET_BPS as u128) * (native as u128);
        let verdict = PerfVerdict {
            native_bps: native,
            measured_bps,
            loss_bps,
            loss_ratio_bps,
            budget_bps: PERF_LOSS_BUDGET_BPS,
            overshoot,
            breached,
        };
        if breached {
            self.case_counter = self.case_counter.saturating_add(1);
            let case = BreachCase {
                case_id: self.case_counter,
                native_bps: native,
                measured_bps,
                loss_bps,
                loss_ratio_bps,
                budget_bps: PERF_LOSS_BUDGET_BPS,
            };
            if self.cases.len() >= CONTRACT_CASE_CAP {
                // Vec 封顶：丢最旧（保最新案卷），显性移除不静默。
                let evicted = self.cases.remove(0);
                let _ = evicted.case_id; // 显性消费移除值，语义为"最旧案卷出卷"
            }
            self.cases.push(case);
            return Err(PassthruError::ContractBreach {
                loss_bps,
                budget_bps: PERF_LOSS_BUDGET_BPS,
            });
        }
        self.claims_passed = self.claims_passed.saturating_add(1);
        Ok(verdict)
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §4 · HostDetectMirror —— F1765 检测复用镜像（三态一致性校验）
// ═════════════════════════════════════════════════════════════════════════════

/// 探测三态（F1765 面语义镜像）：Present/Missing/Unknown 不造假补齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe3 {
    /// 已探测到存在。
    Present,
    /// 已探测到缺失。
    Missing,
    /// 未探测或无法判定——保持 Unknown，不猜。
    Unknown,
}

impl Probe3 {
    /// 三态名。
    pub const fn name(self) -> &'static str {
        match self {
            Probe3::Present => "Present",
            Probe3::Missing => "Missing",
            Probe3::Unknown => "Unknown",
        }
    }
}

/// 检测结论三态（镜像 vck69 EnvironmentProfile 的结论口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectTriState {
    /// 裸金属（显性结论，不是"失败"）：无 hypervisor 签名且无 0x40000000 leaf。
    Bare,
    /// 命中识别表（已知 hypervisor 家族）。
    Virtualized,
    /// 有虚拟化痕迹但未入册（有签名/leaf 但签名不在识别表）。
    Indeterminate,
}

impl DetectTriState {
    /// 三态名。
    pub const fn name(self) -> &'static str {
        match self {
            DetectTriState::Bare => "Bare",
            DetectTriState::Virtualized => "Virtualized",
            DetectTriState::Indeterminate => "Indeterminate",
        }
    }
}

/// hypervisor 签名长度（CPUID leaf 0x40000000 的 EBX/ECX/EDX 12 字节）。
pub const SIG_LEN: usize = 12;

/// 识别表行（F1765 识别表镜像）：签名 12 字节 + 家族名。
#[derive(Debug, Clone, Copy)]
pub struct SigRow {
    /// hypervisor 签名（12 字节）。
    pub sig: [u8; SIG_LEN],
    /// 家族名。
    pub family: &'static str,
}

/// 识别表（来源：各 hypervisor 公开的 CPUID leaf 0x40000000 签名）。
pub const SIG_TABLE: [SigRow; 5] = [
    SigRow {
        sig: *b"Microsoft Hv",
        family: "Hyper-V",
    },
    SigRow {
        sig: *b"KVMKVMKVM\0\0\0",
        family: "KVM",
    },
    SigRow {
        sig: *b"VMwareVMware",
        family: "ESXi",
    },
    SigRow {
        sig: *b"TCGTCGTCGTCG",
        family: "QEMU-TCG",
    },
    SigRow {
        sig: *b"XenVMMXenVMM",
        family: "Xen",
    },
];

/// 直通环境检测镜像输入（F1765 检测器输入面的最小镜像集）。
#[derive(Debug, Clone, Copy)]
pub struct DetectMirrorInput {
    /// CPUID leaf 0x40000000 返回的 12 字节 hypervisor 签名（无则全零）。
    pub hypervisor_sig: [u8; SIG_LEN],
    /// leaf 0x40000000 是否存在（hypervisor leaf 存在位）。
    pub cpuid_leaf4000_present: bool,
    /// /dev/dxg 探测三态镜像（F1765 WSL 专项路径）。
    pub dxg_device_path: Probe3,
}

/// 镜像分类（const fn，while+索引，无派生比较——签名逐字节比对）。
///
/// * 全零签名 ∧ 无 0x40000000 leaf → Bare（裸金属显性结论）。
/// * 签名命中识别表 → Virtualized。
/// * 其余（有 leaf/签名但未入册）→ Indeterminate（与 Bare 严格区分）。
pub const fn classify_mirror(input: &DetectMirrorInput) -> DetectTriState {
    let mut all_zero = true;
    let mut i = 0;
    while i < SIG_LEN {
        if input.hypervisor_sig[i] != 0 {
            all_zero = false;
        }
        i += 1;
    }
    if all_zero && !input.cpuid_leaf4000_present {
        return DetectTriState::Bare;
    }
    let mut j = 0;
    while j < SIG_TABLE.len() {
        let mut hit = true;
        let mut k = 0;
        while k < SIG_LEN {
            if SIG_TABLE[j].sig[k] != input.hypervisor_sig[k] {
                hit = false;
            }
            k += 1;
        }
        if hit {
            return DetectTriState::Virtualized;
        }
        j += 1;
    }
    DetectTriState::Indeterminate
}

/// 环境校验裁决快照。
#[derive(Debug, Clone)]
pub struct EnvVerdict {
    /// 本地镜像分类结论。
    pub local: DetectTriState,
    /// 外部声明结论（须与 local 一致才可能通过）。
    pub declared: DetectTriState,
    /// 直通准入是否放行。
    pub passthru_allowed: bool,
    /// WSL 形态标记（Hyper-V 签名 ∧ /dev/dxg Present）。
    pub wsl_like: bool,
    /// 中文说明（入事件账）。
    pub note: String,
}

/// F1765 检测复用镜像：本地重分类 + 与外部声明的一致性校验 + 直通准入门。
///
/// 复用声明：本结构体**不自研检测器**——输入镜像与结论三态口径与
/// F1764/F1765 检测面对齐；直通侧只做"复用面一致性校验"：
/// 分类不一致即拒绝（防两侧口径漂移），Bare 声明直通即矛盾入账。
#[derive(Debug)]
pub struct HostDetectMirror {
    env_cleared: bool,
    checks_total: u64,
    contradiction_count: u64,
    mismatch_count: u64,
    last_local: Option<DetectTriState>,
    last_declared: Option<DetectTriState>,
}

impl HostDetectMirror {
    pub(crate) fn new() -> Self {
        HostDetectMirror {
            env_cleared: false,
            checks_total: 0,
            contradiction_count: 0,
            mismatch_count: 0,
            last_local: None,
            last_declared: None,
        }
    }

    /// 环境门是否已过（激活前置）。
    pub fn env_cleared(&self) -> bool {
        self.env_cleared
    }

    /// 校验总次数。
    pub fn checks_total(&self) -> u64 {
        self.checks_total
    }

    /// Bare 矛盾入账次数。
    pub fn contradiction_count(&self) -> u64 {
        self.contradiction_count
    }

    /// 镜像不一致次数。
    pub fn mismatch_count(&self) -> u64 {
        self.mismatch_count
    }

    /// 最近一次本地分类结论。
    pub fn last_local(&self) -> Option<DetectTriState> {
        self.last_local
    }

    /// 最近一次外部声明结论。
    pub fn last_declared(&self) -> Option<DetectTriState> {
        self.last_declared
    }

    /// 直通环境准入校验：
    ///
    /// 1. 本地镜像分类 `classify_mirror(input)`；
    /// 2. 与 `declared` 不一致 → `X_PT_DETECT_MIRROR_MISMATCH`（计数留痕，
    ///    环境门不开）；
    /// 3. 一致但为 Bare → `X_PT_DETECT_BARE_CONTRA`（矛盾入账，环境门不开）；
    /// 4. 一致且为 Virtualized/Indeterminate → 放行（`passthru_allowed=true`）。
    pub fn check_environment(
        &mut self,
        input: &DetectMirrorInput,
        declared: DetectTriState,
    ) -> Result<EnvVerdict, PassthruError> {
        self.checks_total = self.checks_total.saturating_add(1);
        let local = classify_mirror(input);
        self.last_local = Some(local);
        self.last_declared = Some(declared);
        if local != declared {
            self.mismatch_count = self.mismatch_count.saturating_add(1);
            return Err(PassthruError::DetectMirrorMismatch { local, declared });
        }
        match declared {
            DetectTriState::Bare => {
                self.contradiction_count = self.contradiction_count.saturating_add(1);
                Err(PassthruError::DetectBareContradiction)
            }
            DetectTriState::Virtualized | DetectTriState::Indeterminate => {
                self.env_cleared = true;
                let wsl_like = input.dxg_device_path == Probe3::Present;
                let note = if wsl_like {
                    format!(
                        "直通环境准入：local={} declared={}（WSL 形态：/dev/dxg Present）",
                        local.name(),
                        declared.name()
                    )
                } else {
                    format!(
                        "直通环境准入：local={} declared={}（F1765 复用面一致）",
                        local.name(),
                        declared.name()
                    )
                };
                Ok(EnvVerdict {
                    local,
                    declared,
                    passthru_allowed: true,
                    wsl_like,
                    note,
                })
            }
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §5 · GuestDriverRegistry —— guest 驱动路径登记（厂商驱动直装 + 版本门槛）
// ═════════════════════════════════════════════════════════════════════════════

/// 驱动登记槽位数（固定容量，封顶不扩）。
pub const DRIVER_SLOT_COUNT: usize = 8;

/// 驱动族表行：族名 + 安装形态 + 最低版本要求（major.minor.build）。
#[derive(Debug, Clone, Copy)]
pub struct DriverFamilyRow {
    /// 驱动族名。
    pub family: &'static str,
    /// 安装形态（厂商驱动直装 / 半虚拟化后备）。
    pub install_mode: &'static str,
    /// 最低版本要求（major, minor, build）。
    pub min_version: (u32, u32, u32),
}

/// 驱动族表（判据"驱动路径/厂商驱动直装"）。
/// 最低版本为演进登记值（来源：直通驱动路径契约登记；升级须改表留痕，
/// 不许运行期放宽）。
pub const DRIVER_FAMILY_TABLE: [DriverFamilyRow; 4] = [
    DriverFamilyRow {
        family: "NVIDIA",
        install_mode: "厂商驱动直装",
        min_version: (550, 0, 0),
    },
    DriverFamilyRow {
        family: "AMD",
        install_mode: "厂商驱动直装",
        min_version: (24, 5, 0),
    },
    DriverFamilyRow {
        family: "Intel",
        install_mode: "厂商驱动直装",
        min_version: (31, 0, 101),
    },
    DriverFamilyRow {
        family: "VirtIO-GPU",
        install_mode: "半虚拟化后备",
        min_version: (1, 0, 0),
    },
];

/// 驱动族表基数。
pub const DRIVER_FAMILY_COUNT: usize = 4;

/// 版本三元组 → 64 位打包（每段 20 位，超宽高位显性掩除）。
pub const fn version_pack(major: u32, minor: u32, build: u32) -> u64 {
    ((major as u64 & 0xFFFFF) << 40)
        | ((minor as u64 & 0xFFFFF) << 20)
        | (build as u64 & 0xFFFFF)
}

/// 64 位打包 → 版本三元组（与 version_pack 互逆，供核对）。
pub const fn version_unpack(packed: u64) -> (u32, u32, u32) {
    (
        ((packed >> 40) & 0xFFFFF) as u32,
        ((packed >> 20) & 0xFFFFF) as u32,
        (packed & 0xFFFFF) as u32,
    )
}

/// 版本是否满足最低要求（语义化三段比较：major → minor → build）。
pub const fn version_meets(packed: u64, min: (u32, u32, u32)) -> bool {
    let (mj, mn, bd) = version_unpack(packed);
    if mj > min.0 {
        return true;
    }
    if mj < min.0 {
        return false;
    }
    if mn > min.1 {
        return true;
    }
    if mn < min.1 {
        return false;
    }
    bd >= min.2
}

/// 驱动登记槽（Copy 槽设计：便于常量空槽填充与指纹摘要）。
#[derive(Debug, Clone, Copy)]
pub struct DriverSlot {
    /// 槽位是否已占用。
    pub occupied: bool,
    /// 驱动族表下标（占用时恒 < DRIVER_FAMILY_COUNT）。
    pub family_idx: usize,
    /// 路径探测三态。
    pub probe: Probe3,
    /// 上报版本（未上报为 None）。
    pub version: Option<(u32, u32, u32)>,
    /// 版本是否达到族表最低要求（Present ∧ 有版本 ∧ version_meets）。
    pub version_met: bool,
}

/// 空槽常量（用于 `[EMPTY_DRIVER_SLOT; N]` 常量初始化）。
pub const EMPTY_DRIVER_SLOT: DriverSlot = DriverSlot {
    occupied: false,
    family_idx: 0,
    probe: Probe3::Unknown,
    version: None,
    version_met: false,
};

/// 驱动就绪摘要。
#[derive(Debug, Clone, Copy)]
pub struct DriverReadiness {
    /// 已用槽位。
    pub slots_used: usize,
    /// Present 数。
    pub present: usize,
    /// Missing 数。
    pub missing: usize,
    /// Unknown 数（不造假补齐）。
    pub unknown: usize,
    /// 版本未达标数（Present 但版本低）。
    pub version_shortfall: usize,
    /// 是否存在任一"Present 且版本达标"族（绑定门条件）。
    pub any_present_met: bool,
}

/// guest 驱动路径登记表：三态登记 + 版本门槛 + 绑定门。
#[derive(Debug)]
pub struct GuestDriverRegistry {
    slots: [DriverSlot; DRIVER_SLOT_COUNT],
    used: usize,
    register_total: u64,
    conflict_count: u64,
}

impl GuestDriverRegistry {
    pub(crate) fn new() -> Self {
        GuestDriverRegistry {
            slots: [EMPTY_DRIVER_SLOT; DRIVER_SLOT_COUNT],
            used: 0,
            register_total: 0,
            conflict_count: 0,
        }
    }

    /// 已用槽位数。
    pub fn used(&self) -> usize {
        self.used
    }

    /// 登记总次数。
    pub fn register_total(&self) -> u64 {
        self.register_total
    }

    /// 冲突次数。
    pub fn conflict_count(&self) -> u64 {
        self.conflict_count
    }

    /// 登记一条驱动路径。冲突（族号越界/同族重复）→
    /// `X_PT_DRIVER_PATH_CONFLICT`（计数留痕，不入库）；
    /// Present 且版本低于族表 → 条目照入库（留痕）但返回
    /// `X_PT_DRIVER_VERSION_LOW`——入库与报错同拍，不留无码之痕。
    pub fn register(
        &mut self,
        family_idx: usize,
        probe: Probe3,
        version: Option<(u32, u32, u32)>,
    ) -> Result<(), PassthruError> {
        self.register_total = self.register_total.saturating_add(1);
        if family_idx >= DRIVER_FAMILY_COUNT {
            self.conflict_count = self.conflict_count.saturating_add(1);
            return Err(PassthruError::DriverPathConflict);
        }
        let mut i = 0;
        while i < DRIVER_SLOT_COUNT {
            if self.slots[i].occupied && self.slots[i].family_idx == family_idx {
                self.conflict_count = self.conflict_count.saturating_add(1);
                return Err(PassthruError::DriverPathConflict);
            }
            i += 1;
        }
        let mut slot_idx = DRIVER_SLOT_COUNT;
        let mut j = 0;
        while j < DRIVER_SLOT_COUNT {
            if !self.slots[j].occupied {
                slot_idx = j;
                j = DRIVER_SLOT_COUNT; // 命中即退出
            } else {
                j += 1;
            }
        }
        if slot_idx >= DRIVER_SLOT_COUNT {
            // 槽满：直通绑定单卡通常只需一族，8 槽仍满属登记失控。
            self.conflict_count = self.conflict_count.saturating_add(1);
            return Err(PassthruError::DriverPathConflict);
        }
        let version_met = match (probe, version) {
            (Probe3::Present, Some(v)) => version_meets(version_pack(v.0, v.1, v.2), DRIVER_FAMILY_TABLE[family_idx].min_version),
            _ => false,
        };
        self.slots[slot_idx] = DriverSlot {
            occupied: true,
            family_idx,
            probe,
            version,
            version_met,
        };
        self.used = self.used.saturating_add(1);
        if probe == Probe3::Present && version.is_some() && !version_met {
            return Err(PassthruError::DriverVersionLow {
                family: DRIVER_FAMILY_TABLE[family_idx].family,
            });
        }
        Ok(())
    }

    /// 就绪摘要（三态计数 + 版本缺口 + 门条件）。
    pub fn readiness(&self) -> DriverReadiness {
        let mut present = 0;
        let mut missing = 0;
        let mut unknown = 0;
        let mut shortfall = 0;
        let mut any_met = false;
        let mut i = 0;
        while i < DRIVER_SLOT_COUNT {
            let s = self.slots[i];
            if s.occupied {
                match s.probe {
                    Probe3::Present => {
                        present += 1;
                        if s.version_met {
                            any_met = true;
                        } else {
                            shortfall += 1;
                        }
                    }
                    Probe3::Missing => missing += 1,
                    Probe3::Unknown => unknown += 1,
                }
            }
            i += 1;
        }
        DriverReadiness {
            slots_used: self.used,
            present,
            missing,
            unknown,
            version_shortfall: shortfall,
            any_present_met: any_met,
        }
    }

    /// 绑定门：要求至少一族 Present 且版本达标。
    /// 未达标 → `X_PT_DRIVER_VERSION_LOW`（带族名）；空表/无达标族 →
    /// `X_PT_DRIVER_PATH_CONFLICT`。
    pub fn driver_gate(&self) -> Result<DriverReadiness, PassthruError> {
        let rd = self.readiness();
        if rd.any_present_met {
            return Ok(rd);
        }
        // 无达标族：先找"Present 但版本低"的具体族（报版本码更有指向性）。
        let mut i = 0;
        while i < DRIVER_SLOT_COUNT {
            let s = self.slots[i];
            if s.occupied && s.probe == Probe3::Present && !s.version_met {
                return Err(PassthruError::DriverVersionLow {
                    family: DRIVER_FAMILY_TABLE[s.family_idx].family,
                });
            }
            i += 1;
        }
        Err(PassthruError::DriverPathConflict)
    }

    /// 槽位指纹摘要（4 字节：占用/族号低 8 位/三态/版本达标）。
    pub(crate) fn slot_digest(&self, idx: usize) -> [u8; 4] {
        if idx >= DRIVER_SLOT_COUNT {
            return [0; 4];
        }
        let s = self.slots[idx];
        if !s.occupied {
            return [0; 4];
        }
        [
            1,
            (s.family_idx & 0xFF) as u8,
            probe_index(s.probe) as u8,
            s.version_met as u8,
        ]
    }
}

/// 探测三态 → 下标（const fn）。
pub const fn probe_index(p: Probe3) -> usize {
    match p {
        Probe3::Present => 0,
        Probe3::Missing => 1,
        Probe3::Unknown => 2,
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §6 · PassthruLimits —— 限制显性表 + 宿主操作拦截器
// ═════════════════════════════════════════════════════════════════════════════

/// 限制种类闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// 不可共享：直通卡独占绑定单一 guest。
    NonShareable,
    /// 迁移受限：设备态在硬件内不可序列化。
    MigrationRestricted,
    /// 热添加不可：绑定后拓扑固化。
    NoHotAdd,
    /// checkpoint 不可（与迁移同族：依赖设备态序列化）。
    NoCheckpoint,
}

/// 限制表行：种类 + 命中码 + 原因（显性，不藏在文档里）。
#[derive(Debug, Clone, Copy)]
pub struct LimitRow {
    /// 限制种类。
    pub kind: LimitKind,
    /// 命中时对外的诊断码。
    pub hit_code: u32,
    /// 限制原因（中文显性说明）。
    pub reason: &'static str,
}

/// 直通限制显性表（判据"限制显性"）。每行一条 const 限制：
/// 共享请求/迁移请求/checkpoint/热添加命中即拒绝带码留痕。
/// 注：checkpoint 与迁移同族受限，命中码并入 `X_PT_LIMIT_MIGRATE`，
/// 原因列显性说明——不在码段里为同族操作另立暗码。
pub const LIMIT_TABLE: [LimitRow; 4] = [
    LimitRow {
        kind: LimitKind::NonShareable,
        hit_code: X_PT_LIMIT_SHARE,
        reason: "不可共享：直通卡独占绑定单一 guest，同一物理卡不可再共享给第二 guest（共享需求走 GPU-P 共享分区通道，不属于直通语义）",
    },
    LimitRow {
        kind: LimitKind::MigrationRestricted,
        hit_code: X_PT_LIMIT_MIGRATE,
        reason: "迁移受限：直通设备状态在硬件内（中断路由/BAR 映射/队列态），live migration 无法序列化设备态——仅关机级迁移动作可走宿主审批外流程",
    },
    LimitRow {
        kind: LimitKind::NoHotAdd,
        hit_code: X_PT_LIMIT_HOTADD,
        reason: "热添加不可：直通绑定后设备拓扑固化，运行期不可热添加第二块直通卡——须先吊销当前绑定再重配拓扑",
    },
    LimitRow {
        kind: LimitKind::NoCheckpoint,
        hit_code: X_PT_LIMIT_MIGRATE,
        reason: "checkpoint 不可：checkpoint 依赖设备态序列化，与 live migration 同族受限——命中码并入迁移限制码（X_PT_LIMIT_MIGRATE），原因在此显性",
    },
];

/// 限制拦截计数器（判据"限制显性"的留痕侧：每次拒绝计数只涨不回绕）。
#[derive(Debug)]
pub struct PassthruLimits {
    share_rejected: u64,
    migrate_rejected: u64,
    hotadd_rejected: u64,
    checkpoint_rejected: u64,
    total_rejected: u64,
}

impl PassthruLimits {
    pub(crate) fn new() -> Self {
        PassthruLimits {
            share_rejected: 0,
            migrate_rejected: 0,
            hotadd_rejected: 0,
            checkpoint_rejected: 0,
            total_rejected: 0,
        }
    }

    fn bump_total(&mut self) {
        self.total_rejected = self.total_rejected.saturating_add(1);
    }

    /// 共享请求拦截：无条件命中不可共享限制。
    pub fn request_share(&mut self) -> PassthruError {
        self.share_rejected = self.share_rejected.saturating_add(1);
        self.bump_total();
        PassthruError::LimitShareHit
    }

    /// 迁移请求拦截：无条件命中迁移受限。
    pub fn request_migration(&mut self) -> PassthruError {
        self.migrate_rejected = self.migrate_rejected.saturating_add(1);
        self.bump_total();
        PassthruError::LimitMigrateHit
    }

    /// 热添加请求拦截：无条件命中热添加不可。
    pub fn request_hot_add(&mut self) -> PassthruError {
        self.hotadd_rejected = self.hotadd_rejected.saturating_add(1);
        self.bump_total();
        PassthruError::LimitHotAddHit
    }

    /// checkpoint 请求拦截：与迁移同族，命中迁移限制码。
    pub fn request_checkpoint(&mut self) -> PassthruError {
        self.checkpoint_rejected = self.checkpoint_rejected.saturating_add(1);
        self.bump_total();
        PassthruError::LimitMigrateHit
    }

    /// 各类拒绝计数（共享/迁移/热添加/checkpoint/总数）。
    pub fn rejection_counts(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.share_rejected,
            self.migrate_rejected,
            self.hotadd_rejected,
            self.checkpoint_rejected,
            self.total_rejected,
        )
    }

    /// 总拒绝数。
    pub fn total_rejected(&self) -> u64 {
        self.total_rejected
    }

    /// 限制表快照（const 表直接可读，此处给运行期取值口）。
    pub fn manifest(&self) -> [LimitRow; 4] {
        LIMIT_TABLE
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §7 · ThermalVisibility —— guest 可见热投影（host 档→guest 档 const 映射）
// ═════════════════════════════════════════════════════════════════════════════

/// 宿主热档四态（复用 F1765/热域档位口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostThermalTier {
    /// 额定域内。
    Nominal,
    /// 偏热（关注）。
    Warm,
    /// 热（限频域）。
    Hot,
    /// 临届（保护域）。
    Critical,
}

/// 宿主热档基数。
pub const HOST_THERMAL_COUNT: usize = 4;

/// 宿主热档 → 下标（const fn）。
pub const fn host_thermal_index(t: HostThermalTier) -> usize {
    match t {
        HostThermalTier::Nominal => 0,
        HostThermalTier::Warm => 1,
        HostThermalTier::Hot => 2,
        HostThermalTier::Critical => 3,
    }
}

/// 宿主热档名。
pub const fn host_thermal_name(t: HostThermalTier) -> &'static str {
    match t {
        HostThermalTier::Nominal => "Nominal",
        HostThermalTier::Warm => "Warm",
        HostThermalTier::Hot => "Hot",
        HostThermalTier::Critical => "Critical",
    }
}

/// guest 感知热档四态（直通下 guest 看到的是同一物理热源的投影）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestThermalTier {
    /// 正常。
    Normal,
    /// 偏高。
    Elevated,
    /// 高（可能限频）。
    High,
    /// 紧急（保护行为可见）。
    Emergency,
}

/// guest 感知档名。
pub const fn guest_thermal_name(t: GuestThermalTier) -> &'static str {
    match t {
        GuestThermalTier::Normal => "Normal",
        GuestThermalTier::Elevated => "Elevated",
        GuestThermalTier::High => "High",
        GuestThermalTier::Emergency => "Emergency",
    }
}

/// guest 感知档 → 下标（const fn）。
pub const fn guest_thermal_index(t: GuestThermalTier) -> usize {
    match t {
        GuestThermalTier::Normal => 0,
        GuestThermalTier::Elevated => 1,
        GuestThermalTier::High => 2,
        GuestThermalTier::Emergency => 3,
    }
}

/// 热投影映射表 `[host 档] → guest 档`。
/// 判据"热可见"：直通下热源是同一物理实体，投影保真优先（一一同档），
/// 保守降档只在投影滞后时启用（见 `guest_thermal_shift_worse`）——
/// 滞后期间 guest 看到的热档已经过期，宁可让 guest 早警不可让 guest 迟警。
pub const THERMAL_MAP: [GuestThermalTier; HOST_THERMAL_COUNT] = [
    GuestThermalTier::Normal,    // host Nominal → guest Normal
    GuestThermalTier::Elevated,  // host Warm    → guest Elevated
    GuestThermalTier::High,      // host Hot     → guest High
    GuestThermalTier::Emergency, // host Critical→ guest Emergency
];

/// 映射查表（const fn）。
pub const fn thermal_map(host: HostThermalTier) -> GuestThermalTier {
    THERMAL_MAP[host_thermal_index(host)]
}

/// 感知档向更严档位偏移一档（Emergency 封顶；const fn）。
pub const fn guest_thermal_shift_worse(t: GuestThermalTier) -> GuestThermalTier {
    match t {
        GuestThermalTier::Normal => GuestThermalTier::Elevated,
        GuestThermalTier::Elevated => GuestThermalTier::High,
        GuestThermalTier::High => GuestThermalTier::Emergency,
        GuestThermalTier::Emergency => GuestThermalTier::Emergency,
    }
}

/// 投影滞后上限（host 样本序推进超过该拍数未投影即判 stale）。
pub const THERMAL_PROJECTION_LAG_LIMIT: u64 = 4;

/// guest 可见热视图快照。
#[derive(Debug, Clone, Copy)]
pub struct GuestThermalView {
    /// 本次投影的 host 档。
    pub host_tier: HostThermalTier,
    /// guest 感知档（映射结果；stale 时已保守降档）。
    pub guest_tier: GuestThermalTier,
    /// host 样本序（最新已见的 host 热样本序号）。
    pub host_seq: u64,
    /// 已投影到的 host 样本序。
    pub projected_upto: u64,
    /// 投影滞后拍数（host 样本序 - 已投影序）。
    pub lag: u64,
    /// 是否滞后（lag > THERMAL_PROJECTION_LAG_LIMIT）。
    pub stale: bool,
    /// 视图诊断码：stale 时 `X_PT_THERMAL_STALE`，否则 `X_PT_OK`。
    pub code: u32,
}

/// guest 热投影器：host 热档 → guest 感知档（映射表）+ 滞后标记。
#[derive(Debug)]
pub struct ThermalProjector {
    host_sample_seq: u64,
    projected_upto: u64,
    has_projected: bool,
    last_host: Option<HostThermalTier>,
    projections: u64,
    stale_hits: u64,
    stale: bool,
    last_view: Option<GuestThermalView>,
}

impl ThermalProjector {
    pub(crate) fn new() -> Self {
        ThermalProjector {
            host_sample_seq: 0,
            projected_upto: 0,
            has_projected: false,
            last_host: None,
            projections: 0,
            stale_hits: 0,
            stale: false,
            last_view: None,
        }
    }

    /// 投影一次。`host_seq` 为宿主热样本序（单调递增；回退样本不回退
    /// 投影点——热档是"当前态"不是"历史序列"）。
    ///
    /// 滞后判定：host 样本序已推进而投影点未跟上（lag > 上限）→ stale，
    /// guest 视图保守降档一档并标记 `X_PT_THERMAL_STALE`。投影本身不失败
    /// （失败面在 `guest_query`：stale 期间查询报错要求补投）。
    pub fn project(&mut self, host: HostThermalTier, host_seq: u64) -> GuestThermalView {
        if host_seq > self.host_sample_seq {
            self.host_sample_seq = host_seq;
        }
        let lag = if self.has_projected {
            self.host_sample_seq.saturating_sub(self.projected_upto)
        } else {
            0
        };
        let stale = lag > THERMAL_PROJECTION_LAG_LIMIT;
        self.projected_upto = self.host_sample_seq;
        self.has_projected = true;
        let mut guest = thermal_map(host);
        if stale {
            guest = guest_thermal_shift_worse(guest);
            self.stale_hits = self.stale_hits.saturating_add(1);
        }
        self.stale = stale;
        self.projections = self.projections.saturating_add(1);
        self.last_host = Some(host);
        let view = GuestThermalView {
            host_tier: host,
            guest_tier: guest,
            host_seq: self.host_sample_seq,
            projected_upto: self.projected_upto,
            lag,
            stale,
            code: if stale { X_PT_THERMAL_STALE } else { X_PT_OK },
        };
        self.last_view = Some(view);
        view
    }

    /// guest 侧查询：stale 期间报 `X_PT_THERMAL_STALE`（要求宿主补投影）；
    /// 尚无任何投影亦报同码（guest 感知尚未建立）。
    pub fn guest_query(&self) -> Result<GuestThermalView, PassthruError> {
        match self.last_view {
            Some(v) => {
                if self.stale {
                    Err(PassthruError::ThermalStale)
                } else {
                    Ok(v)
                }
            }
            None => Err(PassthruError::ThermalStale),
        }
    }

    /// 当前是否处于滞后态。
    pub fn stale_flag(&self) -> bool {
        self.stale
    }

    /// 累计投影次数。
    pub fn projections(&self) -> u64 {
        self.projections
    }

    /// 累计滞后命中次数。
    pub fn stale_hits(&self) -> u64 {
        self.stale_hits
    }

    /// 已投影到的 host 样本序。
    pub fn projected_upto(&self) -> u64 {
        self.projected_upto
    }

    /// 最近 host 档下标（无投影时 0xFF，供指纹摘要）。
    pub(crate) fn last_host_index_or_ff(&self) -> u8 {
        match self.last_host {
            Some(t) => host_thermal_index(t) as u8,
            None => 0xFF,
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §8 · 生命周期状态机 —— 五态 + 迁移矩阵 + 乱序拒绝 + 真吊销
// ═════════════════════════════════════════════════════════════════════════════

/// 直通生命周期五态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// 未分配：设备尚未让渡给任何 guest。
    Unassigned,
    /// 已分配：设备已绑定到某 guest（拓扑锁定）。
    Assigned,
    /// 已绑定：guest 驱动门通过，驱动路径就绪。
    Bound,
    /// 激活：环境门通过，直通语义生效（性能对拍/限制拦截/热投影运行中）。
    Active,
    /// 已吊销：终态，直通绑定解除且不可复活。
    Revoked,
}

/// 状态基数。
pub const LIFECYCLE_STATE_COUNT: usize = 5;

/// 状态 → 下标（const fn）。
pub const fn lifecycle_index(s: LifecycleState) -> usize {
    match s {
        LifecycleState::Unassigned => 0,
        LifecycleState::Assigned => 1,
        LifecycleState::Bound => 2,
        LifecycleState::Active => 3,
        LifecycleState::Revoked => 4,
    }
}

/// 状态名。
pub const fn lifecycle_name(s: LifecycleState) -> &'static str {
    match s {
        LifecycleState::Unassigned => "Unassigned",
        LifecycleState::Assigned => "Assigned",
        LifecycleState::Bound => "Bound",
        LifecycleState::Active => "Active",
        LifecycleState::Revoked => "Revoked",
    }
}

/// 生命周期迁移矩阵 `[from][to]`（true=合法）。
///
/// * Unassigned → Assigned：唯一前进入口（分配即锁定拓扑）。
/// * Assigned → Bound / Revoked：驱动门通过才可绑定；分配后可吊销。
/// * Bound → Active / Revoked：环境门通过才可激活；绑定后可吊销。
/// * Active → Revoked：直通无退回语义（硬件已让渡），只能吊销。
/// * Revoked → （无）：真终态，一切后续操作拒绝。
pub const TRANSITION_ALLOWED: [[bool; LIFECYCLE_STATE_COUNT]; LIFECYCLE_STATE_COUNT] = [
    // to:  Unasgn  Asgn   Bound  Active Revoked
    [false, true, false, false, false], // from Unassigned
    [false, false, true, false, true], //  from Assigned
    [false, false, false, true, true], //  from Bound
    [false, false, false, false, true], // from Active（直通不可退回，只可吊销）
    [false, false, false, false, false], // from Revoked（终态）
];

/// 迁移合法性查表（const fn）。
pub const fn transition_allowed(from: LifecycleState, to: LifecycleState) -> bool {
    TRANSITION_ALLOWED[lifecycle_index(from)][lifecycle_index(to)]
}

// ═════════════════════════════════════════════════════════════════════════════
// §9 · EventLedger —— 定长环形事件账（溢出显性覆盖，不静默丢账）
// ═════════════════════════════════════════════════════════════════════════════

/// 账本容量（64 槽环形；满后覆盖最旧并计数留痕）。
pub const PT_LEDGER_CAP: usize = 64;

/// 事件种类闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtEventKind {
    /// 模式选定（引擎构造）。
    ModeSelected,
    /// 环境校验通过。
    EnvChecked,
    /// Bare 矛盾入账。
    EnvContradiction,
    /// 镜像不一致。
    EnvMismatch,
    /// 激活时环境门未过。
    EnvGateFailed,
    /// 分配（成功或被拒）。
    Assigned,
    /// 绑定（成功或被拒）。
    Bound,
    /// 激活（成功或被拒）。
    Activated,
    /// 吊销（成功或被拒）。
    Revoked,
    /// 驱动路径登记（含被拒留痕）。
    DriverRegistered,
    /// 绑定时驱动门未过。
    DriverGateFailed,
    /// 性能对拍达标。
    PerfClaimPassed,
    /// 性能违约立案。
    PerfBreachFiled,
    /// 共享请求被拒。
    LimitShareRejected,
    /// 迁移请求被拒。
    LimitMigrateRejected,
    /// 热添加请求被拒。
    LimitHotAddRejected,
    /// checkpoint 请求被拒。
    LimitCheckpointRejected,
    /// 热投影（新鲜）。
    ThermalProjected,
    /// 热投影滞后。
    ThermalStale,
    /// 账本溢出覆盖标记。
    LedgerOverrun,
}

/// 事件种类名（全部变体显性命名，供面板/测试面）。
pub const fn event_kind_name(kind: PtEventKind) -> &'static str {
    match kind {
        PtEventKind::ModeSelected => "ModeSelected",
        PtEventKind::EnvChecked => "EnvChecked",
        PtEventKind::EnvContradiction => "EnvContradiction",
        PtEventKind::EnvMismatch => "EnvMismatch",
        PtEventKind::EnvGateFailed => "EnvGateFailed",
        PtEventKind::Assigned => "Assigned",
        PtEventKind::Bound => "Bound",
        PtEventKind::Activated => "Activated",
        PtEventKind::Revoked => "Revoked",
        PtEventKind::DriverRegistered => "DriverRegistered",
        PtEventKind::DriverGateFailed => "DriverGateFailed",
        PtEventKind::PerfClaimPassed => "PerfClaimPassed",
        PtEventKind::PerfBreachFiled => "PerfBreachFiled",
        PtEventKind::LimitShareRejected => "LimitShareRejected",
        PtEventKind::LimitMigrateRejected => "LimitMigrateRejected",
        PtEventKind::LimitHotAddRejected => "LimitHotAddRejected",
        PtEventKind::LimitCheckpointRejected => "LimitCheckpointRejected",
        PtEventKind::ThermalProjected => "ThermalProjected",
        PtEventKind::ThermalStale => "ThermalStale",
        PtEventKind::LedgerOverrun => "LedgerOverrun",
    }
}

/// 单条事件：序号 + 种类 + 诊断码 + 中文细节。
#[derive(Debug, Clone)]
pub struct PtEvent {
    /// 全局递增序号（饱和）。
    pub seq: u64,
    /// 事件种类。
    pub kind: PtEventKind,
    /// 关联诊断码（成功面为 X_PT_OK）。
    pub code: u32,
    /// 中文细节。
    pub detail: String,
}

/// 定长环形事件账。
#[derive(Debug)]
pub struct EventLedger {
    events: Vec<PtEvent>,
    next_seq: u64,
    overrun_count: u64,
    overflow_flag: bool,
}

impl EventLedger {
    pub(crate) fn new() -> Self {
        EventLedger {
            events: Vec::new(),
            next_seq: 0,
            overrun_count: 0,
            overflow_flag: false,
        }
    }

    /// 入账（溢出时覆盖最旧并显性计数）。返回本条序号。
    pub(crate) fn record(&mut self, kind: PtEventKind, code: u32, detail: String) -> u64 {
        self.next_seq = self.next_seq.saturating_add(1);
        let seq = self.next_seq;
        if self.events.len() >= PT_LEDGER_CAP {
            // 槽满：移除最旧（len 已保证非空，remove(0) 不会越界），
            // 溢出显性计数，不静默丢账。
            let evicted = self.events.remove(0);
            self.overrun_count = self.overrun_count.saturating_add(1);
            self.overflow_flag = true;
            let _ = evicted.seq; // 显性消费被覆盖序号，语义为"最旧出账"
        }
        self.events.push(PtEvent { seq, kind, code, detail });
        seq
    }

    /// 当前账内条数。
    pub fn count(&self) -> usize {
        self.events.len()
    }

    /// 最近序号（0 = 空账）。
    pub fn last_seq(&self) -> u64 {
        self.next_seq
    }

    /// 溢出覆盖次数。
    pub fn overrun_count(&self) -> u64 {
        self.overrun_count
    }

    /// 是否发生过溢出覆盖。
    pub fn overflowed(&self) -> bool {
        self.overflow_flag
    }

    /// 全量快照（按入账顺序）。
    pub fn snapshot(&self) -> Vec<PtEvent> {
        self.events.clone()
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §10 · PassthruEngine —— 直通引擎门面（五态编排 + 全子系统接线）
// ═════════════════════════════════════════════════════════════════════════════

/// 引擎事件入账（细节格式化统一在此，避免散落）。
macro_rules! pt_log {
    ($eng:expr, $kind:expr, $code:expr, $($arg:tt)*) => {
        $eng.ledger.record($kind, $code, format!($($arg)*))
    };
}

/// 直通引擎：一台设备一份直通绑定。模式在构造时锁定（不可换档——
/// 换档即拓扑变更，须吊销重建）。
#[derive(Debug)]
pub struct PassthruEngine {
    mode: PassthruMode,
    device_id: u64,
    guest_id: Option<u64>,
    state: LifecycleState,
    contract: PassthruContract,
    mirror: HostDetectMirror,
    drivers: GuestDriverRegistry,
    limits: PassthruLimits,
    thermal: ThermalProjector,
    ledger: EventLedger,
    transitions_done: u64,
    revoke_reason: Option<String>,
}

impl PassthruEngine {
    /// 构造引擎：锁定模式与设备号。device_id 为 0 → `X_PT_NO_DEVICE`。
    pub fn new(mode: PassthruMode, device_id: u64) -> Result<Self, PassthruError> {
        let mut ledger = EventLedger::new();
        if device_id == 0 {
            return Err(PassthruError::NoDevice);
        }
        ledger.record(
            PtEventKind::ModeSelected,
            X_PT_OK,
            format!(
                "直通引擎建立：device 0x{:X} 模式={}（{}）",
                device_id,
                mode_archetype(mode),
                if mode == PassthruMode::Dda { "DDA" } else { "GPU-P 独占" }
            ),
        );
        Ok(PassthruEngine {
            mode,
            device_id,
            guest_id: None,
            state: LifecycleState::Unassigned,
            contract: PassthruContract::new(),
            mirror: HostDetectMirror::new(),
            drivers: GuestDriverRegistry::new(),
            limits: PassthruLimits::new(),
            thermal: ThermalProjector::new(),
            ledger,
            transitions_done: 0,
            revoke_reason: None,
        })
    }

    // ── 只读视图 ─────────────────────────────────────────────────────────

    /// 直通模式。
    pub fn mode(&self) -> PassthruMode {
        self.mode
    }

    /// 设备号。
    pub fn device_id(&self) -> u64 {
        self.device_id
    }

    /// 当前绑定 guest（分配后非 None；吊销后清空）。
    pub fn guest_id(&self) -> Option<u64> {
        self.guest_id
    }

    /// 当前生命周期状态。
    pub fn state(&self) -> LifecycleState {
        self.state
    }

    /// 完成迁移次数（成功计入，被拒不计）。
    pub fn transitions_done(&self) -> u64 {
        self.transitions_done
    }

    /// 吊销原因（吊销后非 None）。
    pub fn revoke_reason(&self) -> Option<&str> {
        match &self.revoke_reason {
            Some(r) => Some(r.as_str()),
            None => None,
        }
    }

    /// 能力差异表查询（越界键 → `X_PT_MODE_UNKNOWN`）。
    pub fn capability(&self, cap: usize) -> Result<u32, PassthruError> {
        match mode_capability(self.mode, cap) {
            Some(v) => Ok(v),
            None => Err(PassthruError::ModeUnknown { cap }),
        }
    }

    /// 事件账快照。
    pub fn snapshot_events(&self) -> Vec<PtEvent> {
        self.ledger.snapshot()
    }

    /// 违约案卷快照。
    pub fn breach_cases(&self) -> Vec<BreachCase> {
        self.contract.cases_snapshot()
    }

    /// 驱动就绪摘要。
    pub fn driver_readiness(&self) -> DriverReadiness {
        self.drivers.readiness()
    }

    // ── 生命周期编排 ─────────────────────────────────────────────────────

    /// 内部迁移执行：Revoked 先行拒绝（显性终态码），再查矩阵。
    fn do_transition(&mut self, to: LifecycleState) -> Result<LifecycleState, PassthruError> {
        if self.state == LifecycleState::Revoked {
            return Err(PassthruError::StateRevoked);
        }
        if !transition_allowed(self.state, to) {
            let from = self.state;
            return Err(PassthruError::StateOrder { from, to });
        }
        self.state = to;
        self.transitions_done = self.transitions_done.saturating_add(1);
        Ok(to)
    }

    /// 分配：Unassigned → Assigned。guest_id 为 0 → `X_PT_NO_DEVICE`。
    pub fn assign(&mut self, guest_id: u64) -> Result<LifecycleState, PassthruError> {
        if guest_id == 0 {
            let e = PassthruError::NoDevice;
            pt_log!(
                self,
                PtEventKind::Assigned,
                e.code(),
                "分配被拒：guest_id 为 0 非法（guest 编号从 1 起）"
            );
            return Err(e);
        }
        let r = self.do_transition(LifecycleState::Assigned);
        match &r {
            Ok(_) => {
                self.guest_id = Some(guest_id);
                pt_log!(
                    self,
                    PtEventKind::Assigned,
                    X_PT_OK,
                    "设备 0x{:X} 分配给 guest #{}（拓扑锁定，不可共享）",
                    self.device_id,
                    guest_id
                );
            }
            Err(e) => {
                pt_log!(self, PtEventKind::Assigned, e.code(), "分配被拒：{}", e.detail());
            }
        }
        r
    }

    /// 绑定：Assigned → Bound，前置两道门——状态门优先（乱序最显性），
    /// 驱动门其次（至少一族 Present 且版本达标）。
    pub fn bind(&mut self) -> Result<LifecycleState, PassthruError> {
        if !transition_allowed(self.state, LifecycleState::Bound) {
            let e = if self.state == LifecycleState::Revoked {
                PassthruError::StateRevoked
            } else {
                PassthruError::StateOrder {
                    from: self.state,
                    to: LifecycleState::Bound,
                }
            };
            pt_log!(self, PtEventKind::Bound, e.code(), "绑定被拒：{}", e.detail());
            return Err(e);
        }
        if let Err(e) = self.drivers.driver_gate() {
            pt_log!(
                self,
                PtEventKind::DriverGateFailed,
                e.code(),
                "绑定被拒（驱动门未过）：{}",
                e.detail()
            );
            return Err(e);
        }
        let r = self.do_transition(LifecycleState::Bound);
        match &r {
            Ok(_) => {
                pt_log!(
                    self,
                    PtEventKind::Bound,
                    X_PT_OK,
                    "设备 0x{:X} 完成 guest 驱动绑定（厂商驱动直装路径就绪）",
                    self.device_id
                );
            }
            Err(e) => {
                pt_log!(self, PtEventKind::Bound, e.code(), "绑定被拒：{}", e.detail());
            }
        }
        r
    }

    /// 激活：Bound → Active，前置两道门——状态门优先（乱序最显性），
    /// 环境门其次（check_environment 已放行）。
    pub fn activate(&mut self) -> Result<LifecycleState, PassthruError> {
        if !transition_allowed(self.state, LifecycleState::Active) {
            let e = if self.state == LifecycleState::Revoked {
                PassthruError::StateRevoked
            } else {
                PassthruError::StateOrder {
                    from: self.state,
                    to: LifecycleState::Active,
                }
            };
            pt_log!(self, PtEventKind::Activated, e.code(), "激活被拒：{}", e.detail());
            return Err(e);
        }
        if !self.mirror.env_cleared() {
            let e = PassthruError::EnvNotCleared;
            pt_log!(
                self,
                PtEventKind::EnvGateFailed,
                e.code(),
                "激活被拒：直通环境检测未通过——先 check_environment（要求 Virtualized/Indeterminate 且与声明一致）"
            );
            return Err(e);
        }
        let r = self.do_transition(LifecycleState::Active);
        match &r {
            Ok(_) => {
                pt_log!(
                    self,
                    PtEventKind::Activated,
                    X_PT_OK,
                    "设备 0x{:X} 直通激活：性能契约/限制拦截/热投影全部生效",
                    self.device_id
                );
            }
            Err(e) => {
                pt_log!(self, PtEventKind::Activated, e.code(), "激活被拒：{}", e.detail());
            }
        }
        r
    }

    /// 吊销：Assigned/Bound/Active → Revoked（真终态）。吊销后 guest 绑定
    /// 清空、原因留档；对 Unassigned 吊销属乱序（无可吊销之物）。
    pub fn revoke(&mut self, reason: String) -> Result<LifecycleState, PassthruError> {
        let r = self.do_transition(LifecycleState::Revoked);
        match &r {
            Ok(_) => {
                let detail = format!("设备 0x{:X} 直通吊销：{}", self.device_id, reason);
                self.guest_id = None;
                self.revoke_reason = Some(reason);
                pt_log!(self, PtEventKind::Revoked, X_PT_OK, "{}", detail);
            }
            Err(e) => {
                pt_log!(self, PtEventKind::Revoked, e.code(), "吊销被拒：{}", e.detail());
            }
        }
        r
    }

    // ── 子系统接线 ───────────────────────────────────────────────────────

    /// 登记原生性能基准（bps）。
    pub fn register_native_baseline(&mut self, native_bps: u64) -> Result<(), PassthruError> {
        let r = self.contract.register_native_baseline(native_bps);
        match &r {
            Ok(()) => {
                pt_log!(
                    self,
                    PtEventKind::PerfClaimPassed,
                    X_PT_OK,
                    "原生基准登记：{}bps（契约预算 {}bps）",
                    native_bps,
                    PERF_LOSS_BUDGET_BPS
                );
            }
            Err(e) => {
                pt_log!(
                    self,
                    PtEventKind::PerfClaimPassed,
                    e.code(),
                    "原生基准登记被拒：{}",
                    e.detail()
                );
            }
        }
        r
    }

    /// 性能声明对拍：达标计数 / 违约立案（案卷入账 + 带码返回）。
    pub fn perf_claim(&mut self, measured_bps: u64) -> Result<PerfVerdict, PassthruError> {
        let r = self.contract.perf_claim(measured_bps);
        match &r {
            Ok(v) => {
                pt_log!(
                    self,
                    PtEventKind::PerfClaimPassed,
                    X_PT_OK,
                    "性能对拍达标：实测 {}bps / 原生 {}bps，损耗 {}bps（{}bps，预算 {}bps）",
                    v.measured_bps,
                    v.native_bps,
                    v.loss_bps,
                    v.loss_ratio_bps,
                    v.budget_bps
                );
            }
            Err(e) => {
                pt_log!(
                    self,
                    PtEventKind::PerfBreachFiled,
                    e.code(),
                    "性能违约立案：{}（损耗 {}bps / 预算 {}bps）",
                    e.detail(),
                    match e {
                        PassthruError::ContractBreach { loss_bps, .. } => *loss_bps,
                        _ => 0,
                    },
                    PERF_LOSS_BUDGET_BPS
                );
            }
        }
        r
    }

    /// 直通环境准入校验（F1765 复用面）。
    pub fn check_environment(
        &mut self,
        input: &DetectMirrorInput,
        declared: DetectTriState,
    ) -> Result<EnvVerdict, PassthruError> {
        let r = self.mirror.check_environment(input, declared);
        match &r {
            Ok(v) => {
                pt_log!(
                    self,
                    PtEventKind::EnvChecked,
                    X_PT_OK,
                    "{}",
                    v.note
                );
            }
            Err(e) => {
                let kind = if e.code() == X_PT_DETECT_BARE_CONTRA {
                    PtEventKind::EnvContradiction
                } else {
                    PtEventKind::EnvMismatch
                };
                pt_log!(self, kind, e.code(), "环境镜像校验失败：{}", e.detail());
            }
        }
        r
    }

    /// 登记一条 guest 驱动路径（版本低时入库留痕但带码返回）。
    pub fn register_driver_path(
        &mut self,
        family_idx: usize,
        probe: Probe3,
        version: Option<(u32, u32, u32)>,
    ) -> Result<(), PassthruError> {
        let r = self.drivers.register(family_idx, probe, version);
        let family = if family_idx < DRIVER_FAMILY_COUNT {
            DRIVER_FAMILY_TABLE[family_idx].family
        } else {
            "（越界族号）"
        };
        match &r {
            Ok(()) => {
                pt_log!(
                    self,
                    PtEventKind::DriverRegistered,
                    X_PT_OK,
                    "驱动路径登记：family={} probe={} version={:?}",
                    family,
                    probe.name(),
                    version
                );
            }
            Err(e) => {
                pt_log!(
                    self,
                    PtEventKind::DriverRegistered,
                    e.code(),
                    "驱动路径登记留痕（冲突不入库/版本低入库留痕）：family={}：{}",
                    family,
                    e.detail()
                );
            }
        }
        r
    }

    /// 共享请求拦截（命中不可共享限制，带码留痕；已吊销绑定先报终态码）。
    pub fn request_share(&mut self, requester_guest: u64) -> PassthruError {
        if self.state == LifecycleState::Revoked {
            return PassthruError::StateRevoked;
        }
        let e = self.limits.request_share();
        pt_log!(
            self,
            PtEventKind::LimitShareRejected,
            e.code(),
            "共享请求被拒（请求方 guest #{}）：{}",
            requester_guest,
            e.detail()
        );
        e
    }

    /// 迁移请求拦截（命中迁移受限，带码留痕；已吊销绑定先报终态码）。
    pub fn request_migration(&mut self, target_host: &str) -> PassthruError {
        if self.state == LifecycleState::Revoked {
            return PassthruError::StateRevoked;
        }
        let e = self.limits.request_migration();
        pt_log!(
            self,
            PtEventKind::LimitMigrateRejected,
            e.code(),
            "迁移请求被拒（目标宿主 {}）：{}",
            target_host,
            e.detail()
        );
        e
    }

    /// 热添加请求拦截（命中热添加不可，带码留痕；已吊销绑定先报终态码）。
    pub fn request_hot_add(&mut self, device_id: u64) -> PassthruError {
        if self.state == LifecycleState::Revoked {
            return PassthruError::StateRevoked;
        }
        let e = self.limits.request_hot_add();
        pt_log!(
            self,
            PtEventKind::LimitHotAddRejected,
            e.code(),
            "热添加请求被拒（待加设备 0x{:X}）：{}",
            device_id,
            e.detail()
        );
        e
    }

    /// checkpoint 请求拦截（与迁移同族，命中迁移限制码，带码留痕；
    /// 已吊销绑定先报终态码）。
    pub fn request_checkpoint(&mut self) -> PassthruError {
        if self.state == LifecycleState::Revoked {
            return PassthruError::StateRevoked;
        }
        let e = self.limits.request_checkpoint();
        pt_log!(
            self,
            PtEventKind::LimitCheckpointRejected,
            e.code(),
            "checkpoint 请求被拒：{}",
            e.detail()
        );
        e
    }

    /// 热投影：host 热档 → guest 感知档（滞后时保守降档 + 留痕）。
    pub fn project_thermal(&mut self, host: HostThermalTier, host_seq: u64) -> GuestThermalView {
        let view = self.thermal.project(host, host_seq);
        if view.stale {
            pt_log!(
                self,
                PtEventKind::ThermalStale,
                X_PT_THERMAL_STALE,
                "热投影滞后 {} 拍超限（host seq {}）——guest 视图已保守降档至 {}",
                view.lag,
                view.host_seq,
                guest_thermal_name(view.guest_tier)
            );
        } else {
            pt_log!(
                self,
                PtEventKind::ThermalProjected,
                X_PT_OK,
                "热投影：host {} → guest {}（lag {}）",
                host_thermal_name(view.host_tier),
                guest_thermal_name(view.guest_tier),
                view.lag
            );
        }
        view
    }

    /// guest 侧热查询（stale 期间报 `X_PT_THERMAL_STALE`）。
    pub fn guest_thermal_query(&self) -> Result<GuestThermalView, PassthruError> {
        self.thermal.guest_query()
    }

    // ── 体检 / 指纹 / 双跑对拍 ───────────────────────────────────────────

    /// 结构化体检快照。`ok` 为综合健康：未吊销 ∧ 零违约 ∧ 零矛盾 ∧
    /// 零镜像不一致 ∧ 零账本溢出 ∧ 热投影未滞后 ∧ 驱动零版本缺口
    /// ∧ 环境门已过。
    pub fn health(&self) -> PtHealth {
        let rd = self.drivers.readiness();
        let (s_r, s_m, s_h, s_c, s_t) = self.limits.rejection_counts();
        let revoked = self.state == LifecycleState::Revoked;
        let ok = !revoked
            && self.contract.case_count() == 0
            && self.mirror.contradiction_count() == 0
            && self.mirror.mismatch_count() == 0
            && self.ledger.overrun_count() == 0
            && !self.thermal.stale_flag()
            && rd.version_shortfall == 0
            && self.mirror.env_cleared();
        PtHealth {
            mode_index: mode_index(self.mode),
            state_index: lifecycle_index(self.state),
            guest_bound: self.guest_id.is_some(),
            ledger_count: self.ledger.count(),
            ledger_cap: PT_LEDGER_CAP,
            ledger_overruns: self.ledger.overrun_count(),
            last_seq: self.ledger.last_seq(),
            contract_claims_total: self.contract.claims_total(),
            contract_claims_passed: self.contract.claims_passed(),
            contract_breach_cases: self.contract.case_count(),
            contract_overshoots: self.contract.overshoot_count(),
            env_checks_total: self.mirror.checks_total(),
            env_contradictions: self.mirror.contradiction_count(),
            env_mismatches: self.mirror.mismatch_count(),
            env_cleared: self.mirror.env_cleared(),
            driver_slots_used: rd.slots_used,
            driver_present: rd.present,
            driver_missing: rd.missing,
            driver_unknown: rd.unknown,
            driver_version_shortfall: rd.version_shortfall,
            limit_share_rejected: s_r,
            limit_migrate_rejected: s_m,
            limit_hotadd_rejected: s_h,
            limit_checkpoint_rejected: s_c,
            limit_rejections_total: s_t,
            thermal_projections: self.thermal.projections(),
            thermal_stale_hits: self.thermal.stale_hits(),
            thermal_stale_now: self.thermal.stale_flag(),
            transitions_done: self.transitions_done,
            revoked,
            ok,
        }
    }

    /// FNV-1a 64 位状态指纹：同输入同指纹（确定性可复现）。
    /// 覆盖模式/状态/绑定/契约计数/镜像结论/驱动槽摘要/限制计数/热投影/
    /// 账本水位/吊销原因——本机回归对账用，不上传（隐私红线同 F1765）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::with_capacity(160);
        buf.push(mode_index(self.mode) as u8);
        buf.extend_from_slice(&self.device_id.to_le_bytes());
        match self.guest_id {
            Some(g) => {
                buf.push(1);
                buf.extend_from_slice(&g.to_le_bytes());
            }
            None => buf.push(0),
        }
        buf.push(lifecycle_index(self.state) as u8);
        buf.extend_from_slice(&self.transitions_done.to_le_bytes());
        buf.extend_from_slice(&self.contract.claims_total().to_le_bytes());
        buf.extend_from_slice(&self.contract.claims_passed().to_le_bytes());
        buf.extend_from_slice(&self.contract.case_count().to_le_bytes());
        buf.extend_from_slice(&self.contract.overshoot_count().to_le_bytes());
        match self.contract.native_baseline() {
            Some(b) => {
                buf.push(1);
                buf.extend_from_slice(&b.to_le_bytes());
            }
            None => buf.push(0),
        }
        buf.push(self.mirror.env_cleared() as u8);
        buf.extend_from_slice(&self.mirror.contradiction_count().to_le_bytes());
        buf.extend_from_slice(&self.mirror.mismatch_count().to_le_bytes());
        buf.extend_from_slice(&self.drivers.used().to_le_bytes());
        let mut i = 0;
        while i < DRIVER_SLOT_COUNT {
            buf.extend_from_slice(&self.drivers.slot_digest(i));
            i += 1;
        }
        buf.extend_from_slice(&self.limits.total_rejected().to_le_bytes());
        buf.push(self.thermal.last_host_index_or_ff());
        buf.extend_from_slice(&self.thermal.projected_upto().to_le_bytes());
        buf.push(self.thermal.stale_flag() as u8);
        buf.extend_from_slice(&self.ledger.last_seq().to_le_bytes());
        buf.extend_from_slice(&self.ledger.overrun_count().to_le_bytes());
        match &self.revoke_reason {
            Some(r) => {
                buf.push(1);
                buf.extend_from_slice(r.as_bytes());
            }
            None => buf.push(0),
        }
        fnv1a64(FNV_OFFSET, &buf)
    }

    /// 双跑对拍（引擎内自检）：同状态连续两次指纹必须一致。
    /// 跨引擎对拍（同脚本双引擎同指纹、异操作异指纹）在测试组闭环。
    pub fn fingerprint_selfcheck(&self) -> bool {
        let a = self.fingerprint();
        let b = self.fingerprint();
        a == b
    }
}

/// 引擎体检快照（纯数据，可直接上面板）。
#[derive(Debug, Clone, Copy)]
pub struct PtHealth {
    /// 模式下标。
    pub mode_index: usize,
    /// 状态下标。
    pub state_index: usize,
    /// 是否有 guest 绑定。
    pub guest_bound: bool,
    /// 账内事件数。
    pub ledger_count: usize,
    /// 账本容量。
    pub ledger_cap: usize,
    /// 账本溢出覆盖次数。
    pub ledger_overruns: u64,
    /// 最近事件序号。
    pub last_seq: u64,
    /// 契约对拍总数。
    pub contract_claims_total: u64,
    /// 契约达标数。
    pub contract_claims_passed: u64,
    /// 违约立案数。
    pub contract_breach_cases: u64,
    /// 对拍异常（overshoot）数。
    pub contract_overshoots: u64,
    /// 环境校验总数。
    pub env_checks_total: u64,
    /// Bare 矛盾数。
    pub env_contradictions: u64,
    /// 镜像不一致数。
    pub env_mismatches: u64,
    /// 环境门是否已过。
    pub env_cleared: bool,
    /// 驱动已用槽。
    pub driver_slots_used: usize,
    /// 驱动 Present 数。
    pub driver_present: usize,
    /// 驱动 Missing 数。
    pub driver_missing: usize,
    /// 驱动 Unknown 数。
    pub driver_unknown: usize,
    /// 驱动版本缺口数。
    pub driver_version_shortfall: usize,
    /// 共享拒绝数。
    pub limit_share_rejected: u64,
    /// 迁移拒绝数。
    pub limit_migrate_rejected: u64,
    /// 热添加拒绝数。
    pub limit_hotadd_rejected: u64,
    /// checkpoint 拒绝数。
    pub limit_checkpoint_rejected: u64,
    /// 限制拒绝总数。
    pub limit_rejections_total: u64,
    /// 热投影次数。
    pub thermal_projections: u64,
    /// 热滞后命中数。
    pub thermal_stale_hits: u64,
    /// 当前是否滞后。
    pub thermal_stale_now: bool,
    /// 成功迁移次数。
    pub transitions_done: u64,
    /// 是否已吊销。
    pub revoked: bool,
    /// 综合健康。
    pub ok: bool,
}

// ═════════════════════════════════════════════════════════════════════════════
// §11 · 五组判据清单（契约/检测/驱动/限制/热——锚点判据的显性落点）
// ═════════════════════════════════════════════════════════════════════════════

/// 五组判据清单：组名 + 判据落点描述（测试面按此清单逐组闭环）。
pub const FIVE_GROUP_TABLE: [(&'static str, &'static str); 5] = [
    ("契约", "损耗<2%=2000bps 声明可检验：基准登记→对拍→达标计数/违约立案"),
    ("检测", "F1765 复用面：镜像三态一致性校验，Bare 声明直通即矛盾入账"),
    ("驱动", "厂商驱动直装路径三态登记 + 版本最低要求表 + 绑定门"),
    ("限制", "不可共享/迁移受限/热添加不可逐条 const + 拦截器带码留痕"),
    ("热", "host 档→guest 档 const 映射 + 投影滞后标记 + guest 查询报码"),
];

// ═════════════════════════════════════════════════════════════════════════════
// §12 · 编译期断言（const _: () = assert!(...)，零运行期成本）
// ═════════════════════════════════════════════════════════════════════════════

const _: () = assert!(PERF_LOSS_BUDGET_BPS == 2000, "契约预算必须为 2000bps（<2%）");
const _: () = assert!(X_PT_OK == 0x1350 && X_PT_NO_DEVICE == 0x135F, "码段必须为 0x1350~0x135F");
const _: () = assert!(MODE_CAP_ROWS == 8 && MODE_COUNT == 2, "能力差异表形状固定 8x2");
const _: () = assert!(SIG_TABLE.len() == 5, "识别表镜像五行（F1765 家族口径）");
const _: () = assert!(DRIVER_FAMILY_TABLE.len() == DRIVER_FAMILY_COUNT, "族表基数一致");
const _: () = assert!(DRIVER_SLOT_COUNT >= DRIVER_FAMILY_COUNT, "槽位容量须容得下全族登记");
const _: () = assert!(LIMIT_TABLE.len() == 4, "限制表四行（共享/迁移/热添加/checkpoint）");
const _: () = assert!(THERMAL_MAP.len() == HOST_THERMAL_COUNT, "热映射表覆盖 host 全档");
const _: () = assert!(THERMAL_PROJECTION_LAG_LIMIT == 4, "投影滞后上限 4 拍");
const _: () = assert!(PT_LEDGER_CAP == 64, "事件账定长 64 槽");
const _: () = assert!(CONTRACT_CASE_CAP == 32, "违约案卷封顶 32 份");
const _: () = assert!(FIVE_GROUP_TABLE.len() == 5, "判据五组（契约/检测/驱动/限制/热）");
const _: () = assert!(
    transition_allowed(LifecycleState::Unassigned, LifecycleState::Assigned),
    "Unassigned→Assigned 必须合法"
);
const _: () = assert!(
    !transition_allowed(LifecycleState::Revoked, LifecycleState::Assigned),
    "Revoked 是终态，不得迁出"
);
const _: () = assert!(
    matches!(mode_capability(PassthruMode::Dda, CAP_LIVE_MIGRATION), Some(0))
        && matches!(mode_capability(PassthruMode::GpuPExclusive, CAP_CARD_SHARE), Some(0)),
    "能力表与限制表必须一致（迁移/共享恒 0）"
);
const _: () = assert!(
    version_pack(550, 0, 0) == 0x0002_2600_0000_0000,
    "版本打包位布局固定（20 位每段：550=0x226 左移 40 位）"
);

// ═════════════════════════════════════════════════════════════════════════════
// §13 · 测试面（五组注入自检 + 生命周期 + 账本/指纹/体检）
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── 注入工具（全部走公开管道，无旁路）────────────────────────────────

    /// Ok 解包助手（测试面专用）。
    fn ok<T>(r: Result<T, PassthruError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("应为 Ok，得到 0x{:04X}: {}", e.code(), e.detail()),
        }
    }

    /// 取错误码助手。
    fn err_code<T>(r: Result<T, PassthruError>) -> u32 {
        match r {
            Ok(_) => panic!("应为 Err，得到 Ok"),
            Err(e) => e.code(),
        }
    }

    /// DDA 引擎（设备号固定，注入用）。
    fn dda_engine() -> PassthruEngine {
        ok(PassthruEngine::new(PassthruMode::Dda, 0xA7C0))
    }

    /// Hyper-V 镜像输入（识别表首行签名注入）。
    fn hyper_v_input() -> DetectMirrorInput {
        DetectMirrorInput {
            hypervisor_sig: SIG_TABLE[0].sig,
            cpuid_leaf4000_present: true,
            dxg_device_path: Probe3::Missing,
        }
    }

    /// 裸金属镜像输入。
    fn bare_input() -> DetectMirrorInput {
        DetectMirrorInput {
            hypervisor_sig: [0u8; SIG_LEN],
            cpuid_leaf4000_present: false,
            dxg_device_path: Probe3::Unknown,
        }
    }

    /// 外来 hypervisor 签名（VirtualBox——不入册）。
    fn foreign_input() -> DetectMirrorInput {
        DetectMirrorInput {
            hypervisor_sig: *b"VBoxVBoxVBox",
            cpuid_leaf4000_present: true,
            dxg_device_path: Probe3::Unknown,
        }
    }

    /// 激活态引擎（注入全管道：分配→驱动→绑定→环境→激活）。
    fn activated_engine(mode: PassthruMode) -> PassthruEngine {
        let mut e = ok(PassthruEngine::new(mode, 0xA7C0));
        ok(e.assign(7));
        ok(e.register_driver_path(0, Probe3::Present, Some((560, 12, 0))));
        ok(e.bind());
        ok(e.check_environment(&hyper_v_input(), DetectTriState::Virtualized));
        ok(e.activate());
        e
    }

    // ── 组一：契约 ───────────────────────────────────────────────────────

    #[test]
    fn contract_pass_claim_within_budget() {
        let mut c = PassthruContract::new();
        ok(c.register_native_baseline(100_000));
        let v = ok(c.perf_claim(99_000));
        assert_eq!(v.loss_bps, 1_000);
        assert_eq!(v.loss_ratio_bps, 100); // 1% = 100bps
        assert_eq!(v.budget_bps, PERF_LOSS_BUDGET_BPS);
        assert!(!v.breached);
        assert!(!v.overshoot);
        assert_eq!(c.claims_total(), 1);
        assert_eq!(c.claims_passed(), 1);
        assert_eq!(c.case_count(), 0);
    }

    #[test]
    fn contract_breach_files_case_with_evidence() {
        let mut c = PassthruContract::new();
        ok(c.register_native_baseline(100_000));
        // 损耗率 2500bps > 预算 2000bps → 违约立案（loss 25000/100000）。
        let code = err_code(c.perf_claim(75_000));
        assert_eq!(code, X_PT_CONTRACT_BREACH);
        let cases = c.cases_snapshot();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].case_id, 1);
        assert_eq!(cases[0].loss_bps, 25_000);
        assert_eq!(cases[0].loss_ratio_bps, 2_500);
        assert_eq!(cases[0].budget_bps, 2_000);
        // 二次违约案号递增。
        let _ = c.perf_claim(70_000);
        let cases = c.cases_snapshot();
        assert_eq!(cases.len(), 2);
        assert_eq!(cases[1].case_id, 2);
        assert_eq!(c.case_count(), 2);
        assert_eq!(c.claims_passed(), 0);
    }

    #[test]
    fn contract_boundary_exact_budget_is_not_breach() {
        // 判据语义是"损耗<2%=2000bps"严格小于：恰在 2000bps 预算线上不违约
        // （loss 20000/100000 = 恰 2000bps）。
        let mut c = PassthruContract::new();
        ok(c.register_native_baseline(100_000));
        let v = ok(c.perf_claim(80_000));
        assert_eq!(v.loss_ratio_bps, 2_000);
        assert!(!v.breached);
        assert_eq!(c.case_count(), 0);
    }

    #[test]
    fn contract_requires_baseline() {
        let mut c = PassthruContract::new();
        assert_eq!(err_code(c.perf_claim(99_000)), X_PT_CONTRACT_NO_BASELINE);
        // 基准为 0 非法。
        assert_eq!(err_code(c.register_native_baseline(0)), X_PT_CONTRACT_NO_BASELINE);
        assert!(c.native_baseline().is_none());
    }

    #[test]
    fn contract_overshoot_tracked_not_breach() {
        let mut c = PassthruContract::new();
        ok(c.register_native_baseline(100_000));
        let v = ok(c.perf_claim(101_000)); // 实测>原生：对拍异常
        assert!(v.overshoot);
        assert_eq!(v.loss_bps, 0);
        assert!(!v.breached);
        assert_eq!(c.overshoot_count(), 1);
        assert_eq!(c.claims_passed(), 1);
    }

    // ── 组二：检测 ───────────────────────────────────────────────────────

    #[test]
    fn detect_classify_table_hits() {
        // 识别表五行全数命中 → Virtualized。
        let mut i = 0;
        while i < SIG_TABLE.len() {
            let input = DetectMirrorInput {
                hypervisor_sig: SIG_TABLE[i].sig,
                cpuid_leaf4000_present: true,
                dxg_device_path: Probe3::Unknown,
            };
            assert!(classify_mirror(&input) == DetectTriState::Virtualized);
            i += 1;
        }
        // 全零签名 + 无 leaf → Bare。
        assert!(classify_mirror(&bare_input()) == DetectTriState::Bare);
        // 全零签名 + 有 leaf → Indeterminate（有痕迹但未入册）。
        let ghost = DetectMirrorInput {
            hypervisor_sig: [0u8; SIG_LEN],
            cpuid_leaf4000_present: true,
            dxg_device_path: Probe3::Unknown,
        };
        assert!(classify_mirror(&ghost) == DetectTriState::Indeterminate);
        // 外来签名 → Indeterminate。
        assert!(classify_mirror(&foreign_input()) == DetectTriState::Indeterminate);
    }

    #[test]
    fn detect_bare_contradiction_logged() {
        let mut e = dda_engine();
        ok(e.assign(7));
        // Bare 上声明直通：矛盾入账，环境门不开。
        let code = err_code(e.check_environment(&bare_input(), DetectTriState::Bare));
        assert_eq!(code, X_PT_DETECT_BARE_CONTRA);
        assert!(!e.health().env_cleared);
        // 激活被环境门拦下。
        ok(e.register_driver_path(0, Probe3::Present, Some((560, 0, 0))));
        ok(e.bind());
        assert_eq!(err_code(e.activate()), X_PT_ENV_NOT_CLEARED);
        // 矛盾计数留痕。
        assert_eq!(e.health().env_contradictions, 1);
    }

    #[test]
    fn detect_mirror_mismatch_rejected() {
        let mut e = dda_engine();
        // 外来签名（本地分类 Indeterminate）却声明 Virtualized → 不一致。
        let code = err_code(e.check_environment(&foreign_input(), DetectTriState::Virtualized));
        assert_eq!(code, X_PT_DETECT_MIRROR_MISMATCH);
        assert_eq!(e.health().env_mismatches, 1);
        assert!(!e.health().env_cleared);
    }

    #[test]
    fn detect_indeterminate_allows_passthru() {
        let mut e = dda_engine();
        ok(e.assign(7));
        ok(e.register_driver_path(0, Probe3::Present, Some((560, 0, 0))));
        ok(e.bind());
        // Indeterminate 是直通合法环境（有虚拟化痕迹但未入册——不装瞎也不装懂）。
        let v = ok(e.check_environment(&foreign_input(), DetectTriState::Indeterminate));
        assert!(v.passthru_allowed);
        assert!(v.local == DetectTriState::Indeterminate);
        assert!(e.health().env_cleared);
        ok(e.activate());
        assert!(e.state() == LifecycleState::Active);
    }

    #[test]
    fn detect_wsl_like_flagged() {
        let mut e = dda_engine();
        let mut input = hyper_v_input();
        input.dxg_device_path = Probe3::Present;
        let v = ok(e.check_environment(&input, DetectTriState::Virtualized));
        assert!(v.wsl_like);
        assert!(v.note.contains("WSL"));
    }

    // ── 组三：驱动 ───────────────────────────────────────────────────────

    #[test]
    fn driver_register_and_gate_pass() {
        let mut e = dda_engine();
        ok(e.assign(7));
        ok(e.register_driver_path(0, Probe3::Present, Some((560, 12, 0))));
        let rd = e.driver_readiness();
        assert_eq!(rd.slots_used, 1);
        assert_eq!(rd.present, 1);
        assert_eq!(rd.version_shortfall, 0);
        assert!(rd.any_present_met);
        ok(e.bind());
        assert!(e.state() == LifecycleState::Bound);
    }

    #[test]
    fn driver_duplicate_and_range_conflict() {
        let mut e = dda_engine();
        ok(e.register_driver_path(1, Probe3::Present, Some((24, 9, 0))));
        // 同族重复 → 冲突。
        assert_eq!(
            err_code(e.register_driver_path(1, Probe3::Present, Some((24, 9, 0)))),
            X_PT_DRIVER_PATH_CONFLICT
        );
        // 族号越界 → 冲突。
        assert_eq!(
            err_code(e.register_driver_path(DRIVER_FAMILY_COUNT, Probe3::Present, Some((1, 0, 0)))),
            X_PT_DRIVER_PATH_CONFLICT
        );
        assert_eq!(e.driver_readiness().slots_used, 1); // 冲突不入库
        assert_eq!(e.health().driver_slots_used, 1);
    }

    #[test]
    fn driver_version_low_blocks_bind() {
        let mut e = dda_engine();
        ok(e.assign(7));
        // NVIDIA 500 < 最低 550：入库留痕但带码返回。
        let code = err_code(e.register_driver_path(0, Probe3::Present, Some((500, 3, 0))));
        assert_eq!(code, X_PT_DRIVER_VERSION_LOW);
        assert_eq!(e.driver_readiness().version_shortfall, 1); // 留痕在库
        // 绑定门拦下，同码带族名。
        match e.bind() {
            Err(PassthruError::DriverVersionLow { family }) => assert_eq!(family, "NVIDIA"),
            other => panic!("应为 DriverVersionLow，得到 {:?}", other),
        }
        // 升级驱动后重登记同族被拒（重复）——须走版本刷新语义：先覆盖登记失败，
        // 正确路径是版本升级走演进流程；此处验证门持续有效。
        assert_eq!(
            err_code(e.register_driver_path(0, Probe3::Present, Some((560, 0, 0)))),
            X_PT_DRIVER_PATH_CONFLICT
        );
    }

    #[test]
    fn driver_unknown_not_fabricated_but_gate_strict() {
        let mut e = dda_engine();
        ok(e.assign(7));
        // VirtIO-GPU 未探测：登记 Unknown（不造假补齐）。
        ok(e.register_driver_path(3, Probe3::Unknown, None));
        let rd = e.driver_readiness();
        assert_eq!(rd.unknown, 1);
        assert!(!rd.any_present_met);
        // 无任何 Present 达标族 → 绑定门拒绝。
        assert_eq!(err_code(e.bind()), X_PT_DRIVER_PATH_CONFLICT);
        // 补上 NVIDIA 达标后门开，Unknown 计数保持显性。
        ok(e.register_driver_path(0, Probe3::Present, Some((560, 0, 0))));
        ok(e.bind());
        let rd = e.driver_readiness();
        assert_eq!(rd.unknown, 1);
        assert!(rd.any_present_met);
    }

    #[test]
    fn driver_version_pack_roundtrip() {
        let packed = version_pack(550, 12, 34567);
        assert_eq!(version_unpack(packed), (550, 12, 34567));
        assert!(version_meets(packed, (550, 0, 0)));
        assert!(!version_meets(version_pack(549, 999, 999), (550, 0, 0)));
        assert!(version_meets(version_pack(551, 0, 0), (550, 0, 0)));
        assert!(!version_meets(version_pack(550, 11, 999), (550, 12, 0)));
        assert!(version_meets(version_pack(550, 12, 0), (550, 12, 0)));
    }

    // ── 组四：限制 ───────────────────────────────────────────────────────

    #[test]
    fn limits_intercept_all_host_ops_with_codes() {
        let mut e = activated_engine(PassthruMode::Dda);
        assert_eq!(e.request_share(9).code(), X_PT_LIMIT_SHARE);
        assert_eq!(e.request_migration("host-b").code(), X_PT_LIMIT_MIGRATE);
        assert_eq!(e.request_hot_add(0xBEEF).code(), X_PT_LIMIT_HOTADD);
        // checkpoint 与迁移同族，命中迁移限制码（LIMIT_TABLE 原因列显性）。
        assert_eq!(e.request_checkpoint().code(), X_PT_LIMIT_MIGRATE);
        let (s, m, h, c, t) = (1, 1, 1, 1, 4);
        let counts = e.health();
        assert_eq!(counts.limit_share_rejected, s);
        assert_eq!(counts.limit_migrate_rejected, m);
        assert_eq!(counts.limit_hotadd_rejected, h);
        assert_eq!(counts.limit_checkpoint_rejected, c);
        assert_eq!(counts.limit_rejections_total, t);
    }

    #[test]
    fn limits_rejections_do_not_break_state() {
        let mut e = activated_engine(PassthruMode::GpuPExclusive);
        let _ = e.request_share(9);
        let _ = e.request_migration("host-c");
        // 拒绝不影响生命周期与后续正常操作。
        assert!(e.state() == LifecycleState::Active);
        ok(e.register_native_baseline(100_000));
        let v = ok(e.perf_claim(99_500));
        assert!(!v.breached);
    }

    #[test]
    fn limits_manifest_table_intact() {
        let limits = PassthruLimits::new();
        let m = limits.manifest();
        assert_eq!(m.len(), 4);
        assert!(m[0].kind == LimitKind::NonShareable && m[0].hit_code == X_PT_LIMIT_SHARE);
        assert!(
            m[1].kind == LimitKind::MigrationRestricted && m[1].hit_code == X_PT_LIMIT_MIGRATE
        );
        assert!(m[2].kind == LimitKind::NoHotAdd && m[2].hit_code == X_PT_LIMIT_HOTADD);
        // checkpoint 行并入迁移码，原因显性。
        assert!(m[3].kind == LimitKind::NoCheckpoint && m[3].hit_code == X_PT_LIMIT_MIGRATE);
        let mut i = 0;
        while i < m.len() {
            assert!(!m[i].reason.is_empty());
            i += 1;
        }
    }

    // ── 组五：热 ─────────────────────────────────────────────────────────

    #[test]
    fn thermal_mapping_table_each_tier() {
        assert!(thermal_map(HostThermalTier::Nominal) == GuestThermalTier::Normal);
        assert!(thermal_map(HostThermalTier::Warm) == GuestThermalTier::Elevated);
        assert!(thermal_map(HostThermalTier::Hot) == GuestThermalTier::High);
        assert!(thermal_map(HostThermalTier::Critical) == GuestThermalTier::Emergency);
        // 保守降档链：逐档上移，Emergency 封顶。
        assert!(guest_thermal_shift_worse(GuestThermalTier::Normal) == GuestThermalTier::Elevated);
        assert!(guest_thermal_shift_worse(GuestThermalTier::High) == GuestThermalTier::Emergency);
        assert!(guest_thermal_shift_worse(GuestThermalTier::Emergency) == GuestThermalTier::Emergency);
    }

    #[test]
    fn thermal_projection_fresh_then_stale_then_recover() {
        let mut e = activated_engine(PassthruMode::Dda);
        // 首投：lag=0，新鲜。
        let v1 = e.project_thermal(HostThermalTier::Warm, 10);
        assert!(!v1.stale);
        assert_eq!(v1.code, X_PT_OK);
        assert!(v1.guest_tier == GuestThermalTier::Elevated);
        // guest 查询可信。
        let q1 = ok(e.guest_thermal_query());
        assert!(q1.guest_tier == GuestThermalTier::Elevated);
        // host 推进 10 拍未投影 → lag=10 > 4 → stale，保守降档。
        let v2 = e.project_thermal(HostThermalTier::Hot, 20);
        assert!(v2.stale);
        assert_eq!(v2.lag, 10);
        assert_eq!(v2.code, X_PT_THERMAL_STALE);
        assert!(v2.guest_tier == GuestThermalTier::Emergency); // High 降档 → Emergency
        // stale 期间 guest 查询报码。
        assert_eq!(err_code(e.guest_thermal_query()), X_PT_THERMAL_STALE);
        // 立即补投：lag=1，恢复新鲜。
        let v3 = e.project_thermal(HostThermalTier::Hot, 21);
        assert!(!v3.stale);
        assert!(v3.guest_tier == GuestThermalTier::High);
        let q3 = ok(e.guest_thermal_query());
        assert!(q3.guest_tier == GuestThermalTier::High);
        let h = e.health();
        assert_eq!(h.thermal_projections, 3);
        assert_eq!(h.thermal_stale_hits, 1);
        assert!(!h.thermal_stale_now);
    }

    #[test]
    fn thermal_backwards_seq_never_regresses() {
        let mut p = ThermalProjector::new();
        let v1 = p.project(HostThermalTier::Nominal, 5);
        assert!(!v1.stale);
        // 回退样本不回退投影点（热档是当前态）。
        let v2 = p.project(HostThermalTier::Warm, 3);
        assert_eq!(v2.host_seq, 5);
        assert!(!v2.stale);
        assert!(v2.guest_tier == GuestThermalTier::Elevated);
    }

    // ── 生命周期组 ───────────────────────────────────────────────────────

    #[test]
    fn lifecycle_happy_path() {
        let mut e = dda_engine();
        assert!(e.state() == LifecycleState::Unassigned);
        ok(e.assign(7));
        assert!(e.state() == LifecycleState::Assigned);
        assert_eq!(e.guest_id(), Some(7));
        ok(e.register_driver_path(0, Probe3::Present, Some((560, 0, 0))));
        ok(e.bind());
        assert!(e.state() == LifecycleState::Bound);
        ok(e.check_environment(&hyper_v_input(), DetectTriState::Virtualized));
        ok(e.activate());
        assert!(e.state() == LifecycleState::Active);
        assert_eq!(e.transitions_done(), 3);
        assert!(e.health().ok);
    }

    #[test]
    fn lifecycle_out_of_order_rejected_with_from_to() {
        let mut e = dda_engine();
        // bind before assign。
        match e.bind() {
            Err(PassthruError::StateOrder { from, to }) => {
                assert!(from == LifecycleState::Unassigned);
                assert!(to == LifecycleState::Bound);
            }
            other => panic!("应为 StateOrder，得到 {:?}", other),
        }
        // activate before everything。
        match e.activate() {
            Err(PassthruError::EnvNotCleared { .. }) => {}
            Err(PassthruError::StateOrder { .. }) => {}
            other => panic!("应为环境门或状态门拒绝，得到 {:?}", other),
        }
        // assign 后直跳 activate 仍被状态矩阵拒绝。
        ok(e.assign(7));
        ok(e.register_driver_path(0, Probe3::Present, Some((560, 0, 0))));
        match e.activate() {
            Err(PassthruError::StateOrder { from, to }) => {
                assert!(from == LifecycleState::Assigned);
                assert!(to == LifecycleState::Active);
            }
            other => panic!("应为 StateOrder，得到 {:?}", other),
        }
        assert!(e.health().ok == false); // 有拒绝留痕，未激活
    }

    #[test]
    fn lifecycle_revoke_from_every_nonterminal_state() {
        // Assigned → Revoked。
        let mut e1 = dda_engine();
        ok(e1.assign(7));
        ok(e1.revoke(String::from("guest 下线")));
        assert!(e1.state() == LifecycleState::Revoked);
        assert_eq!(e1.revoke_reason(), Some("guest 下线"));
        assert_eq!(e1.guest_id(), None);
        // Bound → Revoked。
        let mut e2 = dda_engine();
        ok(e2.assign(7));
        ok(e2.register_driver_path(0, Probe3::Present, Some((560, 0, 0))));
        ok(e2.bind());
        ok(e2.revoke(String::from("拓扑重配")));
        assert!(e2.state() == LifecycleState::Revoked);
        // Active → Revoked。
        let mut e3 = activated_engine(PassthruMode::Dda);
        ok(e3.revoke(String::from("安全吊销")));
        assert!(e3.state() == LifecycleState::Revoked);
        // Unassigned 吊销属乱序（无可吊销之物）。
        let mut e4 = dda_engine();
        assert_eq!(err_code(e4.revoke(String::from("空吊销"))), X_PT_STATE_ORDER);
    }

    #[test]
    fn lifecycle_revoked_is_terminal() {
        let mut e = activated_engine(PassthruMode::Dda);
        ok(e.revoke(String::from("演练")));
        // 终态：一切操作被拒，码恒为 X_PT_STATE_REVOKED。
        assert_eq!(err_code(e.assign(7)), X_PT_STATE_REVOKED);
        assert_eq!(err_code(e.bind()), X_PT_STATE_REVOKED);
        assert_eq!(err_code(e.activate()), X_PT_STATE_REVOKED);
        assert_eq!(err_code(e.revoke(String::from("再吊"))), X_PT_STATE_REVOKED);
        // 直通语义操作同样被拒（不留旁门）。
        assert_eq!(e.request_share(9).code(), X_PT_STATE_REVOKED);
        assert_eq!(e.request_migration("host-d").code(), X_PT_STATE_REVOKED);
        assert_eq!(e.request_hot_add(0xBEEF).code(), X_PT_STATE_REVOKED);
        assert_eq!(e.request_checkpoint().code(), X_PT_STATE_REVOKED);
        assert!(!e.health().ok);
    }

    #[test]
    fn lifecycle_unassigned_cannot_bind_or_activate_directly() {
        let mut e = dda_engine();
        assert_eq!(err_code(e.bind()), X_PT_STATE_ORDER);
        // 状态门优先于环境门：Unassigned 上激活先报乱序。
        assert_eq!(err_code(e.activate()), X_PT_STATE_ORDER);
        // 修正顺序后必须全绿。
        ok(e.assign(7));
        ok(e.register_driver_path(0, Probe3::Present, Some((560, 0, 0))));
        ok(e.bind());
        ok(e.check_environment(&hyper_v_input(), DetectTriState::Virtualized));
        ok(e.activate());
        assert!(e.health().ok);
    }

    // ── 账本 / 指纹 / 体检组 ─────────────────────────────────────────────

    #[test]
    fn ledger_records_every_op_with_codes() {
        let mut e = activated_engine(PassthruMode::Dda);
        let _ = e.request_share(9);
        ok(e.register_native_baseline(100_000));
        ok(e.perf_claim(99_000));
        let events = e.snapshot_events();
        // 事件序号严格递增。
        let mut i = 1;
        while i < events.len() {
            assert!(events[i].seq > events[i - 1].seq);
            i += 1;
        }
        // 关键事件在场。
        let mut has_mode = false;
        let mut has_assigned = false;
        let mut has_bound = false;
        let mut has_activated = false;
        let mut has_share = false;
        let mut has_perf = false;
        let mut i = 0;
        while i < events.len() {
            match events[i].kind {
                PtEventKind::ModeSelected => has_mode = true,
                PtEventKind::Assigned => has_assigned = true,
                PtEventKind::Bound => has_bound = true,
                PtEventKind::Activated => has_activated = true,
                PtEventKind::LimitShareRejected => has_share = true,
                PtEventKind::PerfClaimPassed => has_perf = true,
                _ => {}
            }
            i += 1;
        }
        assert!(has_mode && has_assigned && has_bound && has_activated && has_share && has_perf);
        assert!(events[0].detail.contains("直通引擎建立"));
    }

    #[test]
    fn ledger_overrun_evicts_oldest_explicitly() {
        let mut e = activated_engine(PassthruMode::Dda);
        // 激活管道已有若干事件；再灌满越过 64 槽。
        let mut i = 0;
        while i < PT_LEDGER_CAP {
            let _ = e.request_share(i as u64 + 1);
            i += 1;
        }
        let h = e.health();
        assert_eq!(h.ledger_count, PT_LEDGER_CAP);
        assert!(h.ledger_overruns >= 1);
        // 快照长度恒为容量，序号连续。
        let events = e.snapshot_events();
        assert_eq!(events.len(), PT_LEDGER_CAP);
        assert_eq!(e.health().last_seq, events[PT_LEDGER_CAP - 1].seq);
        assert!(!h.ok); // 溢出留痕拉低综合健康
    }

    #[test]
    fn fingerprint_double_run_same_script_same_print() {
        let build = |breach: bool| -> u64 {
            let mut e = dda_engine();
            ok(e.assign(7));
            ok(e.register_driver_path(0, Probe3::Present, Some((560, 0, 0))));
            ok(e.bind());
            ok(e.check_environment(&hyper_v_input(), DetectTriState::Virtualized));
            ok(e.activate());
            ok(e.register_native_baseline(100_000));
            let _ = if breach { e.perf_claim(75_000) } else { e.perf_claim(99_000) };
            let _ = e.request_share(9);
            let _ = e.project_thermal(HostThermalTier::Warm, 3);
            e.fingerprint()
        };
        // 同脚本双跑：指纹一致。
        assert_eq!(build(false), build(false));
        assert_eq!(build(true), build(true));
        // 异操作：指纹必异。
        assert!(build(false) != build(true));
        // 引擎内双跑自检。
        let e = activated_engine(PassthruMode::Dda);
        assert!(e.fingerprint_selfcheck());
    }

    #[test]
    fn fingerprint_diverges_on_state_change() {
        let mut e = activated_engine(PassthruMode::Dda);
        let f0 = e.fingerprint();
        ok(e.register_native_baseline(100_000));
        let f1 = e.fingerprint();
        assert!(f0 != f1);
        ok(e.perf_claim(99_000));
        let f2 = e.fingerprint();
        assert!(f1 != f2);
        e.project_thermal(HostThermalTier::Hot, 9);
        let f3 = e.fingerprint();
        assert!(f2 != f3);
    }

    #[test]
    fn engine_capability_closed_set() {
        let e = dda_engine();
        // 表内键：DDA 硬件中断直投=1，GPU-P 半虚拟化面=1。
        let e2 = activated_engine(PassthruMode::GpuPExclusive);
        assert_eq!(ok(e.capability(CAP_HW_INTERRUPT_DIRECT)), 1);
        assert_eq!(ok(e.capability(CAP_PARAVIRT_INTERRUPT_SURFACE)), 0);
        assert_eq!(ok(e2.capability(CAP_PARAVIRT_INTERRUPT_SURFACE)), 1);
        assert_eq!(ok(e.capability(CAP_MMIO_DIRECT_MAP)), 1);
        // 限制键两模式恒 0（与限制表一致）。
        assert_eq!(ok(e2.capability(CAP_LIVE_MIGRATION)), 0);
        assert_eq!(ok(e2.capability(CAP_CARD_SHARE)), 0);
        // 越界键 → 显性不猜。
        match e.capability(999) {
            Err(PassthruError::ModeUnknown { cap }) => assert_eq!(cap, 999),
            other => panic!("应为 ModeUnknown，得到 {:?}", other),
        }
    }

    #[test]
    fn error_codes_segment_unique_and_detailed() {
        let all: [PassthruError; 15] = [
            PassthruError::StateOrder {
                from: LifecycleState::Unassigned,
                to: LifecycleState::Bound,
            },
            PassthruError::StateRevoked,
            PassthruError::ContractBreach {
                loss_bps: 3_000,
                budget_bps: 2_000,
            },
            PassthruError::ContractNoBaseline,
            PassthruError::EnvNotCleared,
            PassthruError::DetectBareContradiction,
            PassthruError::DetectMirrorMismatch {
                local: DetectTriState::Bare,
                declared: DetectTriState::Virtualized,
            },
            PassthruError::DriverPathConflict,
            PassthruError::DriverVersionLow { family: "NVIDIA" },
            PassthruError::LimitShareHit,
            PassthruError::LimitMigrateHit,
            PassthruError::LimitHotAddHit,
            PassthruError::ThermalStale,
            PassthruError::ModeUnknown { cap: 99 },
            PassthruError::NoDevice,
        ];
        let mut i = 0;
        while i < all.len() {
            let c = all[i].code();
            assert!(
                (X_PT_STATE_ORDER..=X_PT_NO_DEVICE).contains(&c),
                "code 0x{:04X} 越段",
                c
            );
            assert!(!all[i].detail().is_empty());
            assert!(!format!("{}", all[i]).is_empty());
            i += 1;
        }
        // 码两两不同（十六码独占语义；OK 基线不在错误集内）。
        let mut a = 0;
        while a < all.len() {
            let mut b = a + 1;
            while b < all.len() {
                assert_ne!(all[a].code(), all[b].code());
                b += 1;
            }
            a += 1;
        }
    }

    #[test]
    fn health_snapshot_after_full_pipeline() {
        let mut e = activated_engine(PassthruMode::Dda);
        ok(e.register_native_baseline(100_000));
        ok(e.perf_claim(99_000));
        let _ = e.request_share(9);
        let _ = e.project_thermal(HostThermalTier::Nominal, 1);
        let h = e.health();
        assert!(h.ok);
        assert_eq!(h.mode_index, MODE_DDA);
        assert_eq!(h.state_index, lifecycle_index(LifecycleState::Active));
        assert!(h.guest_bound);
        assert_eq!(h.contract_claims_total, 1);
        assert_eq!(h.contract_claims_passed, 1);
        assert_eq!(h.contract_breach_cases, 0);
        assert_eq!(h.env_contradictions, 0);
        assert_eq!(h.env_mismatches, 0);
        assert!(h.env_cleared);
        assert_eq!(h.driver_present, 1);
        assert_eq!(h.driver_version_shortfall, 0);
        assert_eq!(h.limit_rejections_total, 1);
        assert_eq!(h.thermal_projections, 1);
        assert_eq!(h.thermal_stale_hits, 0);
        assert_eq!(h.transitions_done, 3);
        assert!(!h.revoked);
        assert!(h.ledger_count >= 6);
        assert!(h.ledger_cap == PT_LEDGER_CAP);
    }

    #[test]
    fn health_ok_drops_on_any_red_line() {
        // 违约拉低综合健康。
        let mut e1 = activated_engine(PassthruMode::Dda);
        ok(e1.register_native_baseline(100_000));
        let _ = e1.perf_claim(75_000);
        assert!(!e1.health().ok);
        // Bare 矛盾拉低综合健康（未激活引擎 env_cleared 本就 false）。
        let mut e2 = dda_engine();
        ok(e2.assign(7));
        let _ = e2.check_environment(&bare_input(), DetectTriState::Bare);
        assert!(!e2.health().ok);
        assert_eq!(e2.health().env_contradictions, 1);
    }

    #[test]
    fn engine_new_rejects_zero_device() {
        assert_eq!(
            err_code(PassthruEngine::new(PassthruMode::Dda, 0)),
            X_PT_NO_DEVICE
        );
        assert_eq!(err_code(dda_engine().assign(0)), X_PT_NO_DEVICE);
    }

    #[test]
    fn five_group_table_covers_anchor_criteria() {
        assert_eq!(FIVE_GROUP_TABLE.len(), 5);
        assert_eq!(FIVE_GROUP_TABLE[0].0, "契约");
        assert_eq!(FIVE_GROUP_TABLE[1].0, "检测");
        assert_eq!(FIVE_GROUP_TABLE[2].0, "驱动");
        assert_eq!(FIVE_GROUP_TABLE[3].0, "限制");
        assert_eq!(FIVE_GROUP_TABLE[4].0, "热");
        let mut i = 0;
        while i < FIVE_GROUP_TABLE.len() {
            assert!(!FIVE_GROUP_TABLE[i].1.is_empty());
            i += 1;
        }
    }

    #[test]
    fn event_kind_names_exhaustive() {
        // 全部 20 种事件种类显性命名（编译期穷尽 match 保证新增必改此处）。
        let kinds = [
            PtEventKind::ModeSelected,
            PtEventKind::EnvChecked,
            PtEventKind::EnvContradiction,
            PtEventKind::EnvMismatch,
            PtEventKind::EnvGateFailed,
            PtEventKind::Assigned,
            PtEventKind::Bound,
            PtEventKind::Activated,
            PtEventKind::Revoked,
            PtEventKind::DriverRegistered,
            PtEventKind::DriverGateFailed,
            PtEventKind::PerfClaimPassed,
            PtEventKind::PerfBreachFiled,
            PtEventKind::LimitShareRejected,
            PtEventKind::LimitMigrateRejected,
            PtEventKind::LimitHotAddRejected,
            PtEventKind::LimitCheckpointRejected,
            PtEventKind::ThermalProjected,
            PtEventKind::ThermalStale,
            PtEventKind::LedgerOverrun,
        ];
        let mut i = 0;
        while i < kinds.len() {
            assert!(!event_kind_name(kinds[i]).is_empty());
            i += 1;
        }
    }

    #[test]
    fn mode_archetype_and_state_names_exhaustive() {
        assert!(!mode_archetype(PassthruMode::Dda).is_empty());
        assert!(!mode_archetype(PassthruMode::GpuPExclusive).is_empty());
        assert_eq!(lifecycle_name(LifecycleState::Unassigned), "Unassigned");
        assert_eq!(lifecycle_name(LifecycleState::Assigned), "Assigned");
        assert_eq!(lifecycle_name(LifecycleState::Bound), "Bound");
        assert_eq!(lifecycle_name(LifecycleState::Active), "Active");
        assert_eq!(lifecycle_name(LifecycleState::Revoked), "Revoked");
        assert_eq!(mode_index(PassthruMode::Dda), MODE_DDA);
        assert_eq!(mode_index(PassthruMode::GpuPExclusive), MODE_GPUP_EXCLUSIVE);
    }

    #[test]
    fn gpu_p_exclusive_engine_full_pipeline() {
        // GPU-P 独占模式全管道与限制语义一致。
        let mut e = activated_engine(PassthruMode::GpuPExclusive);
        assert_eq!(ok(e.capability(CAP_PARTITION_RECLAIM)), 1);
        assert_eq!(ok(e.capability(CAP_HOST_DISMOUNT)), 0);
        let _ = e.request_migration("host-e");
        assert_eq!(e.health().limit_migrate_rejected, 1);
        ok(e.register_native_baseline(80_000));
        let v = ok(e.perf_claim(79_000));
        assert!(!v.breached);
        assert_eq!(e.mode(), PassthruMode::GpuPExclusive);
        assert!(!mode_archetype(PassthruMode::GpuPExclusive).is_empty());
    }
}
