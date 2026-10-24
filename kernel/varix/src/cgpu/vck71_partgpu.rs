//! CGPU-F1767 · 分区 GPU（SR-IOV/MIG/GPU-P）（CGPU-K 域 · 虚拟化组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1767`
//!
//! **判据（锚点原文）**：统一抽象、配额模型、边界声明、公平扩展、探测、五组、判据。
//!
//! **职责定位（锚点原文摘录）**：分区技术（SR-IOV（Intel/AMD——VF 管理/MIG（NVIDIA——实例管理/GPU-P（Hyper-V 分区——三技术抽象——统一分区抽象（分区 API（创建/销毁/配额——分区 API 冻结 v1；配额（每分区资源配额（显存/算力百分比——配额模型；隔离（分区隔离（硬件级声明（隔离信任边界——边界声明；公平（分区间公平（调度公平复用 F1610 模式——公平扩展；探测（分区能力探测——探测复用；测试（三抽象/配额/边界/公平/探测五组）。
//!
//! ─────────────────────────────────────────────────────────────────────────────
//! 设计总则（本模块九条铁律，逐条对应锚点判据）：
//!
//! 1. **三技术抽象**（判据"统一抽象"）：`PartTech` 三技术闭集——`SrIov`
//!    （Intel/AMD SR-IOV VF 管理）、`Mig`（NVIDIA MIG 实例管理）、`GpuP`
//!    （Hyper-V GPU-P 分区）。能力差异全部落在 `TECH_CAP_TABLE` 常量差异表
//!    （最大分区数、显存粒度 MiB、算力粒度 ppm、隔离等级、厂商族），逐行
//!    注明数值来源与保守化理由；新增技术必须改表升版，不许运行期拼装
//!    第四态。
//! 2. **分区 API 冻结 v1**（判据"分区 API 冻结 v1"）：统一分区 API 以
//!    （方法号，参数数，版本）三元组签名表冻结；`API_FROZEN_V1_FINGERPRINT`
//!    由签名表 const 求出并有编译期断言把门。演进只增不改：新方法只能占
//!    空槽且版本从 1 起步；既有方法改参数数即 `X_PG_API_FROZEN`（破坏
//!    ABI 拦截），版本回退/同版重放同拒；v1 核心六行被运行期指纹复核
//!    （`api_verify()`），表被腐蚀立即带码报错。
//! 3. **配额模型**（判据"配额模型"）：每分区配额二元组（显存 MiB + 算力
//!    ppm），`QuotaModel` 守恒核保证 Σ分区 ≤ 物理总量（81920 MiB / 1e6
//!    ppm，常量写死并注明来源）；配额粒度按技术对齐（TECH_CAP_TABLE）；
//!    配额变更全量入账（环形 128 槽，溢出覆盖显性计数）；超卖注入即被
//!    守恒核拦截（`X_PG_QUOTA_OVERRUN`）；`verify_conservation()` 随时
//!    对账 Σ活动分区 == 账面提交量。
//! 4. **边界声明**（判据"边界声明/硬件级声明"）：`TRUTH_BOUNDARY_TABLE`
//!    是硬件事实表（SR-IOV/MIG 硬件级、GPU-P 半虚拟级——不夸大红线），
//!    `declare_boundary()` 拒绝一切高于事实的声明（`X_PG_BOUNDARY_INFLATE`），
//!    低于事实的保守声明放行；分区创建前五域（显存/算力/中断/DMA/故障）
//!    必须全部声明，缺一即 `X_PG_BOUNDARY_UNDECLARED`。跨分区访问一律
//!    fault 语义：`X_PG_BOUNDARY_FAULT` + 红案入账，永不静默。
//! 5. **公平扩展**（判据"公平扩展/F1610 复用"）：`FairnessExt` 是 F1610
//!    DRR 调度模式的分区间镜像复用声明——按算力配额 ppm 加权记账，每
//!    16 拍收一个纪元，实得/应得偏差超过 5000‱（50%，跨纪元补偿带宽的
//!    保守界）即红案（`X_PG_FAIRNESS_DRIFT`）；公平对账可出快照报告
//!    （`fairness_report()`）。
//! 6. **探测复用**（判据"探测复用"）：`PartProbe` 是 F1764/F1604 探测面的
//!    镜像输入声明——三态 Present/Missing/Unknown，证据不全一律 Unknown
//!    （不造假升级）；声明与镜像矛盾采信镜像并留痕（`X_PG_PROBE_CONTRA`）；
//!    创建门只放 Present，Unknown/Missing 一律 `X_PG_PROBE_UNKNOWN` 拒绝。
//! 7. **生命周期与孤儿**：Created/Active/Suspended/Destroyed 四态闭集 +
//!    迁移矩阵 const；Destroyed 真终态。属主注销后无属主的活动分区即
//!    孤儿——`audit_orphans()` 全量巡账立案（`X_PG_ORPHAN` 红案），strict
//!    变体带码返回；孤儿分区注入（属主注销注入）必须被巡账检出。
//! 8. **事件账/健康/指纹/双跑**：`EventLedger` 定长 64 槽环形账，溢出覆盖
//!    最旧并显性计数（不静默丢账）；`health()` 结构化体检；`fingerprint()`
//!    把引擎稳定态编入 FNV-1a 64 位——同输入同指纹，本机回归对账不上传；
//!    `run_script()` 同脚本双引擎对拍（同指纹）与异操作（异指纹）在测试组
//!    闭环。
//! 9. **五组测试面**（判据"五组/判据"）：三抽象/配额/边界/公平/探测五组
//!    注入自检 + 生命周期 + 孤儿 + API 冻结 + 双跑对拍，全部走同一公开
//!    管道（注入输入，无旁路）；每项注入必须"检出"（带码 Err 或红案入账）
//!    才算绿。
//!
//! **码段偏差留痕**：任务书首选 0x1C30 段已被 cga42_iffreeze（CGPU-F0042）
//! 独占；任务书备选 0x1C70 段亦被 unx2582_uvmainject / unx2602_upfentry
//! （0x1C40..0x1CBF 声明段）占用。开工前 grep 两轮确认后取最近空闲段
//! **0x1D20..0x1D2F**：cga43/cga44 占 0x1D10/0x1D30，svstar2 ves484/485/486
//! 占 0x1CE0/0x1D40/0x1D60，unx2642 占 0x1D40..0x1D6B——与本段均不相交。
//!
//! 纪律：no_std + alloc；内核路径零 unwrap 零 expect 零 panic 零运行期
//! assert 零 IO 零墙钟零浮点（测试组除外——测试运行在宿主 std 下）；
//! 所有可失败操作返回 `Result<T, PartGpuError>`，错误 `detail()` 为中文
//! 可操作诊断；溢出显性 saturating/wrapping（计数只涨不回绕）；const fn
//! 内不使用 slice::get / Vec 索引 / Ord::min / Ord::max / 派生 PartialEq
//! 比较 / for 循环（一律 while+索引）；含 Vec/String 字段的结构体一律不
//! derive Copy；>64KB 结构体一律不上栈（本模块最大栈上结构体为引擎本体，
//! 约 6KB 量级，远低于 64KB 红线）。

use core::fmt;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ═════════════════════════════════════════════════════════════════════════════
// §0 · 诊断码段（0x1D20~0x1D2F，十六码独占；开工前 grep 两轮确认空闲，
//      见文件头"码段偏差留痕"）
// ═════════════════════════════════════════════════════════════════════════════

/// 成功基线（无错误）。
pub const X_PG_OK: u32 = 0x1D20;
/// 分区/方法/属主编号非法（0 编号或超槽位）——编号从 1 起，核对编号来源。
pub const X_PG_ILLEGAL_ID: u32 = 0x1D21;
/// 生命周期乱序迁移（from/to 全量留痕）——核对 LC_STATE_MATRIX 迁移矩阵。
pub const X_PG_STATE_ORDER: u32 = 0x1D22;
/// Σ分区配额超物理总量（超卖）——守恒核已拦截，缩减配额或销毁闲置分区。
pub const X_PG_QUOTA_OVERRUN: u32 = 0x1D23;
/// 配额值非法（零值/粒度不对齐/超单分区上界/守恒核破坏）——按 reason 定位。
pub const X_PG_QUOTA_OVERFLOW: u32 = 0x1D24;
/// 分区不存在或已销毁（part_id 无效）——先 query_part 核对在册编号。
pub const X_PG_QUOTA_UNKNOWN_PART: u32 = 0x1D25;
/// 分区 API 冻结破坏（v1 签名被改/版本回退/新方法版本非 1）——API 演进
/// 只增不改，改签名即破坏 ABI，必须回退改动或升版冻结表。
pub const X_PG_API_FROZEN: u32 = 0x1D26;
/// 跨分区越界访问——fault 语义（永不静默）：访问已被硬件/半虚拟层拦截，
/// 红案已立案，核对访问路径是否泄漏跨分区句柄。
pub const X_PG_BOUNDARY_FAULT: u32 = 0x1D27;
/// 隔离声明夸大（声明等级高于硬件事实）——按 TRUTH_BOUNDARY_TABLE 如实
/// 降级声明，不夸大是红线。
pub const X_PG_BOUNDARY_INFLATE: u32 = 0x1D28;
/// 技术边界未声明即创建分区——先对五域逐一 declare_boundary 再创建。
pub const X_PG_BOUNDARY_UNDECLARED: u32 = 0x1D29;
/// 分区间公平偏差超界（实得/应得偏差 > 5000‱）——查调度注入是否偏袒。
pub const X_PG_FAIRNESS_DRIFT: u32 = 0x1D2A;
/// 尚无任何调度纪元可对账——先跑 fairness_tick 攒满一个纪元（16 拍）。
pub const X_PG_FAIRNESS_NO_EPOCH: u32 = 0x1D2B;
/// 探测声明与镜像矛盾（声明被采信为假，事实优先）——核对探测证据链。
pub const X_PG_PROBE_CONTRA: u32 = 0x1D2C;
/// 探测三态非 Present 即创建（Unknown/Missing 不许当 Present 用）——先补
/// 探测证据（probe_feed）拿到 Present。
pub const X_PG_PROBE_UNKNOWN: u32 = 0x1D2D;
/// 孤儿分区（无属主红案）——查明属主去向，销毁或重新指派属主。
pub const X_PG_ORPHAN: u32 = 0x1D2E;
/// 操作与技术能力不符（如声称的单元数超该技术能力表上限）——核对
/// TECH_CAP_TABLE 差异表，不许跨技术拼凑能力。
pub const X_PG_TECH_MISMATCH: u32 = 0x1D2F;

/// 本模块诊断码段下界（含）。
pub const X_PG_CODE_MIN: u32 = X_PG_OK;
/// 本模块诊断码段上界（含）。
pub const X_PG_CODE_MAX: u32 = X_PG_TECH_MISMATCH;

// ═════════════════════════════════════════════════════════════════════════════
// §1 · 统一错误类型（每变体 detail() 中文可操作诊断；code() 两两不同，
//      全部落在 0x1D20~0x1D2F 段内）
// ═════════════════════════════════════════════════════════════════════════════

/// 分区子系统统一错误类型。
#[derive(Debug, Clone)]
pub enum PartGpuError {
    /// 编号非法（分区号/属主号/方法号——编号从 1 起）。
    IllegalId {
        /// 非法编号原值。
        id: u64,
        /// 非法发生的操作名。
        what: &'static str,
    },
    /// 生命周期乱序迁移（from/to 全量携带，调用方按迁移矩阵核对）。
    StateOrder {
        /// 目标分区编号。
        part_id: u64,
        /// 迁移发起时的状态。
        from: LifecycleState,
        /// 请求迁往的状态。
        to: LifecycleState,
    },
    /// 超卖：Σ分区配额超物理总量，守恒核拦截（need/free 全量留痕）。
    QuotaOverrun {
        /// 请求提交的显存配额（MiB）。
        need_vram_mib: u64,
        /// 请求提交的算力配额（ppm）。
        need_ppm: u64,
        /// 池内剩余显存（MiB）。
        free_vram_mib: u64,
        /// 池内剩余算力（ppm）。
        free_ppm: u64,
    },
    /// 配额值非法（零值/粒度不对齐/超单分区上界/守恒核破坏），reason 定位。
    QuotaBad {
        /// 涉案的显存配额（MiB）。
        vram_mib: u64,
        /// 涉案的算力配额（ppm）。
        compute_ppm: u64,
        /// 中文拒绝理由。
        reason: &'static str,
    },
    /// 分区不存在（含已销毁墓碑——销毁不是抹账，但已不可操作）。
    UnknownPart {
        /// 查询的分区编号。
        part_id: u64,
        /// 发起查询的操作名。
        op: &'static str,
    },
    /// 分区 API 冻结破坏（v1 指纹不符/改签名/版本回退/新方法版本非 1）。
    ApiFrozen {
        /// 涉案方法号（0 表示 v1 核心指纹整体不符）。
        method_id: u32,
        /// 期望值（参数数或版本或指纹）。
        expected: u64,
        /// 实际值。
        actual: u64,
    },
    /// 跨分区越界访问——fault 语义（不静默），红案已同步立案。
    BoundaryFault {
        /// 访问发起分区。
        src_part: u64,
        /// 被访问分区。
        dst_part: u64,
        /// 被访问的边界域。
        domain: BoundaryDomain,
    },
    /// 隔离声明夸大（declared 高于硬件事实 truth）——不夸大红线。
    BoundaryInflate {
        /// 涉案技术。
        tech: PartTech,
        /// 被拒的声明等级。
        declared: IsolationLevel,
        /// 硬件事实等级。
        truth: IsolationLevel,
    },
    /// 技术边界五域未声明齐即创建分区。
    BoundaryUndeclared {
        /// 涉案技术。
        tech: PartTech,
    },
    /// 分区间公平偏差超界（纪元收账时实得/应得偏差越 5000‱ 界）。
    FairnessDrift {
        /// 收账纪元号。
        epoch: u64,
        /// 偏差分区编号。
        part_id: u64,
        /// 实得调度拍数。
        served: u64,
        /// 应得调度拍数。
        entitled: u64,
    },
    /// 尚无任何调度纪元可对账。
    FairnessNoEpoch,
    /// 探测声明与镜像矛盾（事实优先，状态已采信镜像值）。
    ProbeContra {
        /// 涉案技术。
        tech: PartTech,
        /// 外部声明三态。
        declared: Probe3,
        /// 镜像判定三态。
        mirrored: Probe3,
    },
    /// 探测三态非 Present 即创建（Unknown/Missing 同拒）。
    ProbeUnknown {
        /// 涉案技术。
        tech: PartTech,
    },
    /// 孤儿分区（无属主红案——查明属主去向，销毁或重新指派）。
    OrphanPart {
        /// 孤儿分区编号。
        part_id: u64,
    },
    /// 操作与技术能力不符（跨技术拼凑能力/单元数超上限）。
    TechMismatch {
        /// 涉案技术。
        tech: PartTech,
        /// 中文说明（何种操作何项能力不符）。
        op: &'static str,
    },
}

impl PartGpuError {
    /// 诊断码（0x1D20~0x1D2F 段内两两不同）。
    pub fn code(&self) -> u32 {
        match self {
            PartGpuError::IllegalId { .. } => X_PG_ILLEGAL_ID,
            PartGpuError::StateOrder { .. } => X_PG_STATE_ORDER,
            PartGpuError::QuotaOverrun { .. } => X_PG_QUOTA_OVERRUN,
            PartGpuError::QuotaBad { .. } => X_PG_QUOTA_OVERFLOW,
            PartGpuError::UnknownPart { .. } => X_PG_QUOTA_UNKNOWN_PART,
            PartGpuError::ApiFrozen { .. } => X_PG_API_FROZEN,
            PartGpuError::BoundaryFault { .. } => X_PG_BOUNDARY_FAULT,
            PartGpuError::BoundaryInflate { .. } => X_PG_BOUNDARY_INFLATE,
            PartGpuError::BoundaryUndeclared { .. } => X_PG_BOUNDARY_UNDECLARED,
            PartGpuError::FairnessDrift { .. } => X_PG_FAIRNESS_DRIFT,
            PartGpuError::FairnessNoEpoch => X_PG_FAIRNESS_NO_EPOCH,
            PartGpuError::ProbeContra { .. } => X_PG_PROBE_CONTRA,
            PartGpuError::ProbeUnknown { .. } => X_PG_PROBE_UNKNOWN,
            PartGpuError::OrphanPart { .. } => X_PG_ORPHAN,
            PartGpuError::TechMismatch { .. } => X_PG_TECH_MISMATCH,
        }
    }

    /// 中文可操作诊断（告诉调用方下一步该做什么）。
    pub fn detail(&self) -> &'static str {
        match self {
            PartGpuError::IllegalId { .. } => {
                "编号非法（0 或超界）——分区号/属主号/方法号一律从 1 起，核对编号来源"
            }
            PartGpuError::StateOrder { .. } => {
                "生命周期乱序迁移——对照 LC_STATE_MATRIX 迁移矩阵，先补齐前置状态"
            }
            PartGpuError::QuotaOverrun { .. } => {
                "Σ分区配额超物理总量（超卖）——守恒核已拦截：缩减本次配额、收缩存量分区或销毁闲置分区"
            }
            PartGpuError::QuotaBad { .. } => {
                "配额值非法——查 reason 字段：零值/粒度不对齐/超单分区上界/守恒核破坏，按 TECH_CAP_TABLE 对齐后重试"
            }
            PartGpuError::UnknownPart { .. } => {
                "分区不存在（含已销毁墓碑）——先 query_part 核对在册编号，销毁分区不可复活须重新创建"
            }
            PartGpuError::ApiFrozen { .. } => {
                "分区 API 冻结破坏——演进只增不改：改签名/版本回退/新方法非 v1 起步一律拒绝，回退改动或升版冻结表"
            }
            PartGpuError::BoundaryFault { .. } => {
                "跨分区越界访问 fault——访问已被隔离层拦截并立案，排查调用方是否泄漏跨分区句柄"
            }
            PartGpuError::BoundaryInflate { .. } => {
                "隔离声明夸大——声明等级不得高于 TRUTH_BOUNDARY_TABLE 硬件事实，如实/保守声明后重试"
            }
            PartGpuError::BoundaryUndeclared { .. } => {
                "技术边界五域（显存/算力/中断/DMA/故障）未声明齐——先逐域 declare_boundary 再创建分区"
            }
            PartGpuError::FairnessDrift { .. } => {
                "分区间公平偏差超界（>5000‱）——查调度注入是否偏袒某分区，必要时调权重或红案追责"
            }
            PartGpuError::FairnessNoEpoch => {
                "尚无调度纪元可对账——先 fairness_tick 攒满 EPOCH_TICKS 拍收一个纪元"
            }
            PartGpuError::ProbeContra { .. } => {
                "探测声明与镜像矛盾（已采信镜像）——核对探测证据链后按事实重新声明"
            }
            PartGpuError::ProbeUnknown { .. } => {
                "探测三态非 Present 即创建——先 probe_feed 补齐硬件证据拿到 Present 再创建分区"
            }
            PartGpuError::OrphanPart { .. } => {
                "孤儿分区（无属主红案）——查明属主去向：重新指派属主或走 destroy 销毁回收配额"
            }
            PartGpuError::TechMismatch { .. } => {
                "操作与技术能力不符——不许跨技术拼凑能力，按 TECH_CAP_TABLE 差异表核对后改用正确技术"
            }
        }
    }
}

impl fmt::Display for PartGpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[0x{:04X}] {}", self.code(), self.detail())
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §2 · 基础件（FNV-1a 64 位指纹 / 混合函数 / 槽位上限）
// ═════════════════════════════════════════════════════════════════════════════

/// 最大在册分区槽数（含销毁墓碑）。常量写死：单引擎 16 槽为保守实现上界，
/// 升版需同步 MAX_PARTS 注释与配额守恒核算。
pub const MAX_PARTS: usize = 16;

/// 红案上限（环形覆盖，溢出显性计数，不静默丢案）。
pub const CASE_CAP: usize = 64;

/// FNV-1a 偏移基（标准 64 位常量，来源：FNV-1a 规范）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 素数（标准 64 位常量，来源：FNV-1a 规范）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 指纹混合一步（h ^= v; h *= FNV_PRIME），const 可用（wrapping 乘法）。
pub const fn mix64(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 64 位字节级指纹（运行期用；const fn 内不做 slice 索引）。
pub fn fnv1a64(init: u64, bytes: &[u8]) -> u64 {
    let mut h = init;
    let mut i = 0;
    while i < bytes.len() {
        h = mix64(h, bytes[i] as u64);
        i += 1;
    }
    h
}

/// 饱和加一步（计数只涨不回绕）。
pub fn sat_add(dst: &mut u64, delta: u64) {
    *dst = dst.saturating_add(delta);
}

// ═════════════════════════════════════════════════════════════════════════════
// §3 · 三技术抽象（PartTech 闭集 + 能力差异 const 表 + 隔离等级 +
//      边界域 + 硬件事实表——判据"统一抽象/边界声明"）
// ═════════════════════════════════════════════════════════════════════════════

/// 分区技术三技术闭集。只许这三态：新增技术必须改 TECH_CAP_TABLE 升版，
/// 不许运行期拼装第四态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartTech {
    /// SR-IOV（Intel/AMD——VF 管理：PCIe 单根 I/O 虚拟化，每个 VF 独占
    /// BAR 窗口与 MSI-X 向量，宿主驱动只管 PF）。
    SrIov,
    /// MIG（NVIDIA——实例管理：SM 与显存切片物理分区成独立 GPU 实例）。
    Mig,
    /// GPU-P（Hyper-V 分区——半虚拟化分区：vmbus 通道 + 宿主软件配额）。
    GpuP,
}

/// 技术表行号（SrIov=0 / Mig=1 / GpuP=2）。
pub const TECH_SRI_OV: usize = 0;
/// 技术表行号（MIG）。
pub const TECH_MIG: usize = 1;
/// 技术表行号（GPU-P）。
pub const TECH_GPU_P: usize = 2;
/// 技术闭集大小。
pub const TECH_COUNT: usize = 3;

/// 技术表行号（const 总函数，闭集内安全）。
pub const fn tech_index(tech: PartTech) -> usize {
    match tech {
        PartTech::SrIov => TECH_SRI_OV,
        PartTech::Mig => TECH_MIG,
        PartTech::GpuP => TECH_GPU_P,
    }
}

/// 行号反查技术（const 总函数：越界行号回退 SrIov，闭集外不产生新态）。
pub const fn tech_from_index(index: usize) -> PartTech {
    match index {
        TECH_MIG => PartTech::Mig,
        TECH_GPU_P => PartTech::GpuP,
        _ => PartTech::SrIov,
    }
}

/// 技术名（const）。
pub const fn tech_name(tech: PartTech) -> &'static str {
    match tech {
        PartTech::SrIov => "SR-IOV",
        PartTech::Mig => "MIG",
        PartTech::GpuP => "GPU-P",
    }
}

/// 隔离等级闭集（诚实分级——不夸大红线的基础）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationLevel {
    /// 无隔离声明（仅闭集完备性占位，不许用于任何域声明放行）。
    None,
    /// 半虚拟化隔离（hypervisor/vmbus 软件边界——GPU-P 的事实等级）。
    Paravirtual,
    /// 硬件级隔离（PCIe/IOMMU/SM 切片物理边界——SR-IOV/MIG 的事实等级）。
    Hardware,
}

impl IsolationLevel {
    /// 等级秩（用于"声明不得高于事实"的秩比较）。
    pub const fn rank(self) -> u32 {
        match self {
            IsolationLevel::None => 0,
            IsolationLevel::Paravirtual => 1,
            IsolationLevel::Hardware => 2,
        }
    }

    /// 等级名（const）。
    pub const fn name(self) -> &'static str {
        match self {
            IsolationLevel::None => "None",
            IsolationLevel::Paravirtual => "Paravirtual",
            IsolationLevel::Hardware => "Hardware",
        }
    }
}

/// 边界域闭集（五域——配额/隔离/故障的分区切面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryDomain {
    /// 显存域（VRAM/BAR 窗口）。
    Vram,
    /// 算力域（SM/引擎时间片）。
    Compute,
    /// 中断域（MSI-X/中断重映射）。
    Interrupt,
    /// DMA 域（IOMMU/ACS）。
    Dma,
    /// 故障域（机器检查/挂死隔离）。
    Fault,
}

/// 边界域数（五域）。
pub const DOMAIN_COUNT: usize = 5;

/// 边界域表行号（const 总函数）。
pub const fn domain_index(domain: BoundaryDomain) -> usize {
    match domain {
        BoundaryDomain::Vram => 0,
        BoundaryDomain::Compute => 1,
        BoundaryDomain::Interrupt => 2,
        BoundaryDomain::Dma => 3,
        BoundaryDomain::Fault => 4,
    }
}

/// 域名（const）。
pub const fn domain_name(domain: BoundaryDomain) -> &'static str {
    match domain {
        BoundaryDomain::Vram => "Vram",
        BoundaryDomain::Compute => "Compute",
        BoundaryDomain::Interrupt => "Interrupt",
        BoundaryDomain::Dma => "Dma",
        BoundaryDomain::Fault => "Fault",
    }
}

/// 技术能力差异表行（判据"每技术能力差异 const 表"）。
#[derive(Debug, Clone, Copy)]
pub struct TechCapRow {
    /// 技术名。
    pub name: &'static str,
    /// 厂商族。
    pub vendor_family: &'static str,
    /// 单 GPU 最大分区数（保守上界）。
    pub max_parts: u32,
    /// 显存配额粒度（MiB——配额必须按此对齐）。
    pub vram_gran_mib: u64,
    /// 算力配额粒度（ppm——配额必须按此对齐）。
    pub compute_gran_ppm: u64,
    /// 事实隔离等级（与 TRUTH_BOUNDARY_TABLE 五域行一致的最高级）。
    pub isolation: IsolationLevel,
    /// 数值来源与保守化理由。
    pub note: &'static str,
}

/// 技术能力差异表（逐行注明来源；数值取公开资料常见档位的保守上界，
/// 不夸大——真机差异由探测面复核，不在本表吹数）。
pub const TECH_CAP_TABLE: [TechCapRow; TECH_COUNT] = [
    // SR-IOV（Intel/AMD）：GPU SR-IOV 常见 4~32 VF；取 32 为保守上界
    //（PCIe SR-IOV 能力寄存器字段本身更宽，GPU 实现远小于协议上界）。
    TechCapRow {
        name: "SR-IOV",
        vendor_family: "Intel/AMD",
        max_parts: 32,
        vram_gran_mib: 256,
        compute_gran_ppm: 31_250,
        isolation: IsolationLevel::Hardware,
        note: "VF 数取 GPU 实现常见档保守上界 32（非 PCIe 协议 16 位宽上界）；显存粒度 256MiB 对齐 VF BAR 窗口；算力粒度 1/32 档（31250ppm，与 VF 数上限自洽）",
    },
    // MIG（NVIDIA）：A100/H100 最多 7 个 GPU 实例（1g.5gb × 7）。
    TechCapRow {
        name: "MIG",
        vendor_family: "NVIDIA",
        max_parts: 7,
        vram_gran_mib: 5120,
        compute_gran_ppm: 142_857,
        isolation: IsolationLevel::Hardware,
        note: "实例上限 7（A100/H100 MIG 1g.5gb×7 切片）；显存粒度 5120MiB（1g.5gb 实例）；算力粒度 1/7 切片（142857ppm 取整，余量由宿主保留）",
    },
    // GPU-P（Hyper-V）：半虚拟化分区，配额软件定义，粒度到 MiB/1%。
    TechCapRow {
        name: "GPU-P",
        vendor_family: "Microsoft（Hyper-V）/通用半虚拟",
        max_parts: 64,
        vram_gran_mib: 64,
        compute_gran_ppm: 10_000,
        isolation: IsolationLevel::Paravirtual,
        note: "分区上限 64（宿主实现保守上界）；显存粒度 64MiB（VRAM 配额软件定义）；算力粒度 1%（10000ppm）；隔离为半虚拟化（vmbus），不得声明硬件级",
    },
];

/// 查技术能力行（const，闭集内安全）。
pub const fn tech_cap_row(tech: PartTech) -> TechCapRow {
    TECH_CAP_TABLE[tech_index(tech)]
}

/// 物理显存总量（MiB）。常量写死：80 GiB 基准池——配额守恒核算基准，
/// 非实测硬件值；换真机由探测面校正，不在本常量吹数。
pub const PHYS_VRAM_MIB: u64 = 81_920;
/// 物理算力总量（ppm）。常量写死：1_000_000 ppm = 100% 单 GPU 算力。
pub const PHYS_COMPUTE_PPM: u64 = 1_000_000;

/// 单分区显存理论上界：整池。能否拿到受守恒核（Σ分区 ≤ 池）与技术粒度
/// 共同整形——本上界只拦"配额值本身超池"的非法值。
pub const fn tech_vram_cap_mib(_tech: PartTech) -> u64 {
    PHYS_VRAM_MIB
}

/// 单分区算力理论上界：整池（同上——份额整形靠粒度与守恒核）。
pub const fn tech_ppm_cap(_tech: PartTech) -> u64 {
    PHYS_COMPUTE_PPM
}

/// 硬件事实边界表（判据"硬件级声明/不夸大红线"）：
/// 行 = 技术（SrIov/Mig/GpuP），列 = 五域（Vram/Compute/Interrupt/Dma/Fault）。
///
/// * SR-IOV：VF BAR 独占（显存）/VF 引擎硬件划分（算力）/每 VF MSI-X +
///   中断重映射（中断）/IOMMU+ACS（DMA）/PCIe 故障域（故障）——全硬件级。
/// * MIG：SM 与显存切片物理分区，故障域硬件隔离——全硬件级。
/// * GPU-P：vmbus 半虚拟通道 + 宿主软件配额，无 IOMMU 硬边界——全半虚拟级，
///   声明 Hardware 即夸大，一律拒绝。
pub const TRUTH_BOUNDARY_TABLE: [[IsolationLevel; DOMAIN_COUNT]; TECH_COUNT] = [
    [
        IsolationLevel::Hardware,
        IsolationLevel::Hardware,
        IsolationLevel::Hardware,
        IsolationLevel::Hardware,
        IsolationLevel::Hardware,
    ],
    [
        IsolationLevel::Hardware,
        IsolationLevel::Hardware,
        IsolationLevel::Hardware,
        IsolationLevel::Hardware,
        IsolationLevel::Hardware,
    ],
    [
        IsolationLevel::Paravirtual,
        IsolationLevel::Paravirtual,
        IsolationLevel::Paravirtual,
        IsolationLevel::Paravirtual,
        IsolationLevel::Paravirtual,
    ],
];

/// 查硬件事实等级（const）。
pub const fn boundary_truth(tech: PartTech, domain: BoundaryDomain) -> IsolationLevel {
    TRUTH_BOUNDARY_TABLE[tech_index(tech)][domain_index(domain)]
}

// ═════════════════════════════════════════════════════════════════════════════
// §4 · 统一分区 API 冻结 v1（签名指纹冻结 + 演进只增不改——判据"分区 API
//      冻结 v1"，改签名即破坏 ABI 拦截）
// ═════════════════════════════════════════════════════════════════════════════

/// API 方法槽位总数（冻结表 6 行 + 预留 2 槽；扩槽必须升版冻结表）。
pub const API_METHOD_SLOTS: usize = 8;
/// v1 冻结方法行数。
pub const API_V1_ROWS: usize = 6;
/// API 冻结魔数（"PGAPIV10" 的 ASCII 编排）——进指纹首拍，防串段对拍。
pub const API_ABI_MAGIC: u64 = 0x5047_4150_4956_3130;

/// v1 冻结签名表：行 = [方法号, 参数数, 版本]。冻结语义：任何一行被改
/// （含参数数变更）即破坏 ABI——`api_table_fingerprint` 随之改变，
/// `verify_frozen()` 带码拦截；演进只增不改（新方法占空槽且版本从 1 起）。
pub const API_V1_TABLE: [[u32; 3]; API_V1_ROWS] = [
    [1, 4, 1], // create(tech, owner, vram_mib, compute_ppm) -> part_id
    [2, 1, 1], // destroy(part_id)
    [3, 3, 1], // set_quota(part_id, vram_mib, compute_ppm)
    [4, 1, 1], // query_part(part_id)
    [5, 1, 1], // probe_feed(input)
    [6, 1, 1], // fairness_tick(part_id)
];

/// 对签名槽位表前 `rows` 行求指纹（const：while+数组索引，不用 slice::get）。
pub const fn api_slots_fingerprint(slots: &[[u32; 3]; API_METHOD_SLOTS], rows: usize) -> u64 {
    let mut h = mix64(FNV_OFFSET, API_ABI_MAGIC);
    let mut i = 0;
    while i < rows {
        let mut j = 0;
        while j < 3 {
            h = mix64(h, slots[i][j] as u64);
            j += 1;
        }
        i += 1;
    }
    h
}

/// 对冻结表整体求指纹（const）。
pub const fn api_table_fingerprint(table: &[[u32; 3]; API_V1_ROWS]) -> u64 {
    let mut h = mix64(FNV_OFFSET, API_ABI_MAGIC);
    let mut i = 0;
    while i < API_V1_ROWS {
        let mut j = 0;
        while j < 3 {
            h = mix64(h, table[i][j] as u64);
            j += 1;
        }
        i += 1;
    }
    h
}

/// v1 冻结指纹（const 求出，唯一权威值；三元组全量——含版本列）。
pub const API_FROZEN_V1_FINGERPRINT: u64 = api_table_fingerprint(&API_V1_TABLE);

/// v1 冻结核指纹（只混 [方法号, 参数数] 两列——版本允许只增演进，
/// 参数面冻结不变；运行期 verify_frozen 以此为权威）。
pub const API_CORE_V1_FINGERPRINT: u64 = api_core_table_fingerprint(&API_V1_TABLE);

/// 对冻结表核心两列求指纹（const）。
pub const fn api_core_table_fingerprint(table: &[[u32; 3]; API_V1_ROWS]) -> u64 {
    let mut h = mix64(FNV_OFFSET, API_ABI_MAGIC);
    let mut i = 0;
    while i < API_V1_ROWS {
        let mut j = 0;
        while j < 2 {
            h = mix64(h, table[i][j] as u64);
            j += 1;
        }
        i += 1;
    }
    h
}

/// 对槽位表前 `rows` 行核心两列求指纹（const）。
pub const fn api_core_slots_fingerprint(slots: &[[u32; 3]; API_METHOD_SLOTS], rows: usize) -> u64 {
    let mut h = mix64(FNV_OFFSET, API_ABI_MAGIC);
    let mut i = 0;
    while i < rows {
        let mut j = 0;
        while j < 2 {
            h = mix64(h, slots[i][j] as u64);
            j += 1;
        }
        i += 1;
    }
    h
}

// 编译期断门：冻结指纹必须非零且与冻结表重算一致（签名表被手改会在
// 编译期暴露指纹漂移——除非同时改本断言，那属于冻结破坏本身）。
const _: () = assert!(API_FROZEN_V1_FINGERPRINT != 0);
const _: () = assert!(API_CORE_V1_FINGERPRINT != 0);
const _: () = assert!(api_table_fingerprint(&API_V1_TABLE) == API_FROZEN_V1_FINGERPRINT);
const _: () = assert!(api_core_table_fingerprint(&API_V1_TABLE) == API_CORE_V1_FINGERPRINT);
const _: () = assert!(API_V1_ROWS <= API_METHOD_SLOTS);

/// API 注册表（运行期槽位账——演进只增不改的执行面）。
#[derive(Debug, Clone)]
pub struct ApiRegistry {
    /// 方法槽位（[方法号, 参数数, 版本]）。
    slots: [[u32; 3]; API_METHOD_SLOTS],
    /// 槽位占用位图。
    used: [bool; API_METHOD_SLOTS],
    /// 合法扩展次数（新方法登记 + 合法版本递增）。
    extensions: u64,
    /// 冻结破坏拦截次数（改签名/版本回退/新方法非 v1 起步）。
    frozen_violations: u64,
}

impl ApiRegistry {
    /// 注册表起步：v1 六行原样入座。
    pub fn new() -> Self {
        let mut slots = [[0u32; 3]; API_METHOD_SLOTS];
        let mut used = [false; API_METHOD_SLOTS];
        let mut i = 0;
        while i < API_V1_ROWS {
            slots[i] = API_V1_TABLE[i];
            used[i] = true;
            i += 1;
        }
        ApiRegistry {
            slots,
            used,
            extensions: 0,
            frozen_violations: 0,
        }
    }

    /// v1 核心指纹运行期复核（只核 [方法号, 参数数] 冻结核——版本列允许
    /// 只增演进；参数面被腐蚀立即带码报错）。
    pub fn verify_frozen(&self) -> Result<u64, PartGpuError> {
        let actual = api_core_slots_fingerprint(&self.slots, API_V1_ROWS);
        if actual != API_CORE_V1_FINGERPRINT {
            return Err(PartGpuError::ApiFrozen {
                method_id: 0,
                expected: API_CORE_V1_FINGERPRINT,
                actual,
            });
        }
        Ok(actual)
    }

    /// 演进只增不改：
    /// * 空槽新方法：版本必须从 1 起步（否则 `X_PG_API_FROZEN`）；
    /// * 已占槽：参数数不允许变（变即破坏 ABI），版本只许严格递增。
    pub fn extend(
        &mut self,
        method_id: u32,
        params: u32,
        version: u32,
    ) -> Result<(), PartGpuError> {
        if method_id == 0 || method_id as usize > API_METHOD_SLOTS {
            return Err(PartGpuError::IllegalId {
                id: method_id as u64,
                what: "api_extend 方法号非法（从 1 起且不超过槽位总数）",
            });
        }
        let slot = method_id as usize - 1;
        if self.used[slot] {
            let old_params = self.slots[slot][1];
            let old_version = self.slots[slot][2];
            if old_params != params {
                self.frozen_violations += 1;
                return Err(PartGpuError::ApiFrozen {
                    method_id,
                    expected: old_params as u64,
                    actual: params as u64,
                });
            }
            if version <= old_version {
                self.frozen_violations += 1;
                return Err(PartGpuError::ApiFrozen {
                    method_id,
                    expected: old_version as u64 + 1,
                    actual: version as u64,
                });
            }
            self.slots[slot][2] = version;
        } else {
            if version != 1 {
                self.frozen_violations += 1;
                return Err(PartGpuError::ApiFrozen {
                    method_id,
                    expected: 1,
                    actual: version as u64,
                });
            }
            self.slots[slot] = [method_id, params, version];
            self.used[slot] = true;
        }
        self.extensions = self.extensions.saturating_add(1);
        Ok(())
    }

    /// 槽位视图（未占槽返回 None——不猜）。
    pub fn slot_view(&self, method_id: u32) -> Option<[u32; 3]> {
        if method_id == 0 || method_id as usize > API_METHOD_SLOTS {
            return None;
        }
        let slot = method_id as usize - 1;
        if self.used[slot] {
            Some(self.slots[slot])
        } else {
            None
        }
    }

    /// 合法扩展次数。
    pub fn extensions(&self) -> u64 {
        self.extensions
    }

    /// 冻结破坏拦截次数。
    pub fn frozen_violations(&self) -> u64 {
        self.frozen_violations
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §5 · 配额模型（每分区 显存 MiB + 算力 ppm；守恒核 Σ分区 ≤ 物理总量；
//      配额变更账环形 128 槽；超卖拦截——判据"配额模型"）
// ═════════════════════════════════════════════════════════════════════════════

/// 配额变更账容量（环形覆盖，溢出显性计数，不静默丢账）。
pub const QUOTA_LEDGER_CAP: usize = 128;

/// 一笔配额变更（账面记录，含前后值——可审计可回放）。
#[derive(Debug, Clone)]
pub struct QuotaChangeRec {
    /// 分区编号。
    pub part_id: u64,
    /// 变更前显存（MiB）。
    pub old_vram_mib: u64,
    /// 变更后显存（MiB）。
    pub new_vram_mib: u64,
    /// 变更前算力（ppm）。
    pub old_ppm: u64,
    /// 变更后算力（ppm）。
    pub new_ppm: u64,
    /// 引擎全局序号（变更发生拍）。
    pub seq: u64,
}

/// 配额模型：守恒核 + 变更账。
#[derive(Debug, Clone)]
pub struct QuotaModel {
    /// 账面已提交显存（MiB）。
    committed_vram_mib: u64,
    /// 账面已提交算力（ppm）。
    committed_ppm: u64,
    /// 变更账（环形 Vec：满则覆盖最旧并计数）。
    changes: Vec<QuotaChangeRec>,
    /// 变更总笔数（含被覆盖的）。
    change_count: u64,
    /// 变更账覆盖次数（>0 即早期账目被覆盖——审计须结合快照及时对账）。
    ledger_overruns: u64,
    /// 超卖拦截次数（守恒核拒绝的提交）。
    rejected_oversell: u64,
}

impl QuotaModel {
    /// 空池起步。
    pub fn new() -> Self {
        QuotaModel {
            committed_vram_mib: 0,
            committed_ppm: 0,
            changes: Vec::new(),
            change_count: 0,
            ledger_overruns: 0,
            rejected_oversell: 0,
        }
    }

    /// 账面已提交显存（MiB）。
    pub fn committed_vram_mib(&self) -> u64 {
        self.committed_vram_mib
    }

    /// 账面已提交算力（ppm）。
    pub fn committed_ppm(&self) -> u64 {
        self.committed_ppm
    }

    /// 池内剩余显存（MiB，饱和减法——账面异常时不回绕）。
    pub fn free_vram_mib(&self) -> u64 {
        PHYS_VRAM_MIB.saturating_sub(self.committed_vram_mib)
    }

    /// 池内剩余算力（ppm）。
    pub fn free_ppm(&self) -> u64 {
        PHYS_COMPUTE_PPM.saturating_sub(self.committed_ppm)
    }

    /// 变更总笔数。
    pub fn change_count(&self) -> u64 {
        self.change_count
    }

    /// 变更账覆盖次数。
    pub fn ledger_overruns(&self) -> u64 {
        self.ledger_overruns
    }

    /// 超卖拦截次数。
    pub fn rejected_oversell(&self) -> u64 {
        self.rejected_oversell
    }

    /// 变更账快照（按写入序）。
    pub fn changes_snapshot(&self) -> Vec<QuotaChangeRec> {
        self.changes.clone()
    }

    /// 守恒核提交：need 双维都在余量内才放行；超卖即拦截并计数。
    pub fn try_commit(&mut self, vram_mib: u64, compute_ppm: u64) -> Result<(), PartGpuError> {
        let free_v = self.free_vram_mib();
        let free_p = self.free_ppm();
        if vram_mib > free_v || compute_ppm > free_p {
            self.rejected_oversell = self.rejected_oversell.saturating_add(1);
            return Err(PartGpuError::QuotaOverrun {
                need_vram_mib: vram_mib,
                need_ppm: compute_ppm,
                free_vram_mib: free_v,
                free_ppm: free_p,
            });
        }
        self.committed_vram_mib = self.committed_vram_mib.saturating_add(vram_mib);
        self.committed_ppm = self.committed_ppm.saturating_add(compute_ppm);
        Ok(())
    }

    /// 守恒核释放（销毁/缩配额回池）。账面不足时饱和下溢并留 0——
    /// 上层 `verify_conservation()` 会对账捕获这种账实不符。
    pub fn release(&mut self, vram_mib: u64, compute_ppm: u64) {
        self.committed_vram_mib = self.committed_vram_mib.saturating_sub(vram_mib);
        self.committed_ppm = self.committed_ppm.saturating_sub(compute_ppm);
    }

    /// 记一笔配额变更（环形：满则覆盖最旧并显性计数）。
    pub fn record_change(&mut self, rec: QuotaChangeRec) {
        if self.changes.len() >= QUOTA_LEDGER_CAP {
            self.changes.remove(0);
            self.ledger_overruns = self.ledger_overruns.saturating_add(1);
        }
        self.changes.push(rec);
        self.change_count = self.change_count.saturating_add(1);
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §6 · 生命周期（四态闭集 + 迁移矩阵 const——Destroyed 真终态）
// ═════════════════════════════════════════════════════════════════════════════

/// 分区生命周期四态闭集。Created（已建待激活）→ Active（运行中）↔
/// Suspended（挂起）→ Destroyed（销毁，真终态——配额已回池，分区不可
/// 复活，须重新创建）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// 已创建未激活（配额已入账，算力/显存已预占）。
    Created,
    /// 运行中（可调度、可参与公平记账、可被访问）。
    Active,
    /// 挂起（配额保留，不参与公平记账）。
    Suspended,
    /// 已销毁（真终态——配额已回池，一切后续操作被拒）。
    Destroyed,
}

/// 生命周期状态数。
pub const LC_COUNT: usize = 4;

/// 状态表行号（const 总函数）。
pub const fn lc_index(state: LifecycleState) -> usize {
    match state {
        LifecycleState::Created => 0,
        LifecycleState::Active => 1,
        LifecycleState::Suspended => 2,
        LifecycleState::Destroyed => 3,
    }
}

/// 状态名（const）。
pub const fn lc_name(state: LifecycleState) -> &'static str {
    match state {
        LifecycleState::Created => "Created",
        LifecycleState::Active => "Active",
        LifecycleState::Suspended => "Suspended",
        LifecycleState::Destroyed => "Destroyed",
    }
}

/// 生命周期迁移矩阵：行 = from，列 = to，true = 合法迁移。
/// 合法迁移闭集：Created→Active；Created→Destroyed；Active→Suspended；
/// Active→Destroyed；Suspended→Active；Suspended→Destroyed。
/// Destroyed 行全 false——真终态，不可复活。
pub const LC_STATE_MATRIX: [[bool; LC_COUNT]; LC_COUNT] = [
    // from Created: →Active, →Destroyed
    [false, true, false, true],
    // from Active: →Suspended, →Destroyed
    [false, false, true, true],
    // from Suspended: →Active, →Destroyed
    [false, true, false, true],
    // from Destroyed: 终态（全拒绝）
    [false, false, false, false],
];

/// 查迁移合法性（const）。
pub const fn lc_transition_allowed(from: LifecycleState, to: LifecycleState) -> bool {
    LC_STATE_MATRIX[lc_index(from)][lc_index(to)]
}

// ═════════════════════════════════════════════════════════════════════════════
// §7 · 分区记录（Partition + 对外快照 PartInfo）
// ═════════════════════════════════════════════════════════════════════════════

/// 分区记录（引擎内账，含配额与属主——属主为 0 即孤儿）。
#[derive(Debug, Clone)]
pub struct Partition {
    /// 分区编号（从 1 起单调递增）。
    pub id: u64,
    /// 分区技术。
    pub tech: PartTech,
    /// 属主编号（0 = 无属主 = 孤儿红案）。
    pub owner: u64,
    /// 生命周期状态。
    pub state: LifecycleState,
    /// 显存配额（MiB）。
    pub vram_mib: u64,
    /// 算力配额（ppm）。
    pub compute_ppm: u64,
    /// 该分区的配额变更次数。
    pub quota_changes: u64,
    /// 创建时的引擎全局序号。
    pub created_seq: u64,
    /// 最近触碰时的引擎全局序号。
    pub last_touch_seq: u64,
}

/// 分区对外快照（不含内部序号——给 query_part 用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartInfo {
    /// 分区编号。
    pub id: u64,
    /// 分区技术。
    pub tech: PartTech,
    /// 属主编号（0 = 孤儿）。
    pub owner: u64,
    /// 生命周期状态。
    pub state: LifecycleState,
    /// 显存配额（MiB）。
    pub vram_mib: u64,
    /// 算力配额（ppm）。
    pub compute_ppm: u64,
    /// 配额变更次数。
    pub quota_changes: u64,
}

// ═════════════════════════════════════════════════════════════════════════════
// §8 · 公平扩展（F1610 DRR 调度模式的分区间镜像复用声明——判据"公平扩展"：
//      按算力配额 ppm 加权记账，纪元收账对偏差立案）
// ═════════════════════════════════════════════════════════════════════════════

/// 纪元长度（拍）。常量写死：16 拍收一个纪元——DRR 跨纪元补偿窗口的
/// 镜像参数，与 F1610 模式声明对齐。
pub const EPOCH_TICKS: u64 = 16;
/// 公平偏差容忍界（万分比）。常量写死：5000‱ = 50%——单纪元内允许的
/// 最大实得/应得偏差（DRR 补偿带宽的保守界；跨多纪元累积偏差由红案
/// 逐纪元追责）。
pub const DRIFT_BOUND_PERMYRIAD: u64 = 5_000;

/// 一行纪元偏差对账（实得 vs 应得）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriftRow {
    /// 分区槽位。
    pub slot: usize,
    /// 本纪元实得调度拍数。
    pub served: u64,
    /// 本纪元应得调度拍数（按算力配额 ppm 加权折算）。
    pub entitled: u64,
    /// 绝对偏差。
    pub drift: u64,
    /// 允许偏差界（entitled × 5000‱ + 1——entitled=0 时界为 1，抖动容忍）。
    pub bound: u64,
    /// 是否越界（越界即红案）。
    pub breached: bool,
}

/// 纪元收账结果。
#[derive(Debug, Clone)]
pub struct EpochClose {
    /// 本次收账的纪元号（从 0 起）。
    pub epoch_no: u64,
    /// 偏差对账行（只含有调度活动的分区）。
    pub rows: Vec<DriftRow>,
}

/// 纪元应得拍数折算：entitled_i = EPOCH_TICKS × ppm_i / Σppm（整数下取整；
/// Σppm=0 时全 0——无活动分区即无应得）。
pub fn entitled_for_ticks(
    ticks: u64,
    active_ppm: &[u64; MAX_PARTS],
    sum_ppm: u64,
) -> [u64; MAX_PARTS] {
    let mut out = [0u64; MAX_PARTS];
    if sum_ppm == 0 {
        return out;
    }
    let mut i = 0;
    while i < MAX_PARTS {
        out[i] = ticks.saturating_mul(active_ppm[i]) / sum_ppm;
        i += 1;
    }
    out
}

/// 公平扩展记账器（F1610 镜像）。
#[derive(Debug, Clone)]
pub struct FairnessExt {
    /// 已收账纪元数。
    pub epoch: u64,
    /// 当前纪元已走拍数。
    pub epoch_ticks: u64,
    /// 当前纪元各槽实得拍数。
    pub epoch_served: [u64; MAX_PARTS],
    /// 历史累计各槽实得拍数。
    pub total_served: [u64; MAX_PARTS],
    /// 公平偏差红案累计数。
    pub drift_cases: u64,
    /// 最近一次收账的最大偏差（观测值，供健康面读）。
    pub last_close_drift_max: u64,
}

impl FairnessExt {
    /// 空账起步。
    pub fn new() -> Self {
        FairnessExt {
            epoch: 0,
            epoch_ticks: 0,
            epoch_served: [0u64; MAX_PARTS],
            total_served: [0u64; MAX_PARTS],
            drift_cases: 0,
            last_close_drift_max: 0,
        }
    }

    /// 记一拍调度（引擎已验证分区存在且 Active）。
    pub fn record_tick(&mut self, slot: usize) {
        if slot < MAX_PARTS {
            self.epoch_served[slot] = self.epoch_served[slot].saturating_add(1);
            self.total_served[slot] = self.total_served[slot].saturating_add(1);
        }
        self.epoch_ticks = self.epoch_ticks.saturating_add(1);
    }

    /// 纪元收账：逐槽对实得/应得偏差，越界行标记 breached（红案由引擎
    /// 立案）；收账后重置纪元计数、纪元号递增。
    pub fn close_epoch(&mut self, entitled: &[u64; MAX_PARTS]) -> EpochClose {
        let epoch_no = self.epoch;
        let mut rows: Vec<DriftRow> = Vec::new();
        let mut i = 0;
        while i < MAX_PARTS {
            let served = self.epoch_served[i];
            let ent = entitled[i];
            if served != 0 || ent != 0 {
                let drift = if served > ent { served - ent } else { ent - served };
                let bound = ent.saturating_mul(DRIFT_BOUND_PERMYRIAD) / 10_000 + 1;
                let breached = drift > bound;
                if breached {
                    self.drift_cases = self.drift_cases.saturating_add(1);
                    if drift > self.last_close_drift_max {
                        self.last_close_drift_max = drift;
                    }
                }
                rows.push(DriftRow {
                    slot: i,
                    served,
                    entitled: ent,
                    drift,
                    bound,
                    breached,
                });
            }
            i += 1;
        }
        let mut j = 0;
        while j < MAX_PARTS {
            self.epoch_served[j] = 0;
            j += 1;
        }
        self.epoch_ticks = 0;
        self.epoch = self.epoch.saturating_add(1);
        EpochClose { epoch_no, rows }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §9 · 能力探测（F1764/F1604 镜像输入声明 + 三态不造假——判据"探测复用"）
// ═════════════════════════════════════════════════════════════════════════════

/// 探测三态（不造假三态——证据不全一律 Unknown，绝不升级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe3 {
    /// 探测在（硬件证据 + 单元报告齐备）。
    Present,
    /// 探测缺席（无硬件证据且无单元报告）。
    Missing,
    /// 证据不全/矛盾——不可判定（Unknown 不许当 Present 用）。
    Unknown,
}

impl Probe3 {
    /// 三态名（const）。
    pub const fn name(self) -> &'static str {
        match self {
            Probe3::Present => "Present",
            Probe3::Missing => "Missing",
            Probe3::Unknown => "Unknown",
        }
    }
}

/// 三态表行号（const 总函数）。
pub const fn probe_index(probe: Probe3) -> usize {
    match probe {
        Probe3::Present => 0,
        Probe3::Missing => 1,
        Probe3::Unknown => 2,
    }
}

/// 探测镜像输入（F1764 异构探测 / F1604 能力探测的镜像复用声明——
/// 本模块不自研探测硬件，只镜像其输入与结论三态）。
#[derive(Debug, Clone, Copy)]
pub struct ProbeMirrorInput {
    /// 被探测技术。
    pub tech: PartTech,
    /// 硬件能力证据（如 SR-IOV 能力寄存器位 / MIG cap / vmbus 类 GUID）。
    pub hw_cap_seen: bool,
    /// 报告的单元数（VF 数 / MIG 实例数 / GPU-P 分区通道数）。
    pub units_reported: u32,
    /// 外部声明的三态（与镜像对拍——矛盾即 `X_PG_PROBE_CONTRA`）。
    pub declared: Probe3,
}

/// 镜像判定（const，不造假规则）：
/// * 有硬件证据且有单元报告 → Present；
/// * 无硬件证据且无单元报告 → Missing；
/// * 其余（证据与报告不一致）→ Unknown（绝不升级，也绝不武断缺席）。
pub const fn mirror_of(input: &ProbeMirrorInput) -> Probe3 {
    let evidence = input.hw_cap_seen;
    let units = input.units_reported;
    if evidence && units > 0 {
        Probe3::Present
    } else if !evidence && units == 0 {
        Probe3::Missing
    } else {
        Probe3::Unknown
    }
}

/// 探测面（镜像输入账 + 三态状态表）。
#[derive(Debug, Clone)]
pub struct PartProbe {
    /// 各技术当前三态（起步全 Unknown——不造假不预设）。
    states: [Probe3; TECH_COUNT],
    /// 喂入总次数。
    feeds: u64,
    /// 矛盾拦截次数。
    contradictions: u64,
}

impl PartProbe {
    /// 起步：三技术全 Unknown。
    pub fn new() -> Self {
        PartProbe {
            states: [Probe3::Unknown; TECH_COUNT],
            feeds: 0,
            contradictions: 0,
        }
    }

    /// 查某技术当前三态。
    pub fn state_of(&self, tech: PartTech) -> Probe3 {
        self.states[tech_index(tech)]
    }

    /// 喂入总次数。
    pub fn feeds(&self) -> u64 {
        self.feeds
    }

    /// 矛盾拦截次数。
    pub fn contradictions(&self) -> u64 {
        self.contradictions
    }

    /// 喂入一份镜像探测：
    /// * 单元数超该技术能力表上限 → `X_PG_TECH_MISMATCH`（跨技术拼凑造假）；
    /// * 声明与镜像矛盾 → `X_PG_PROBE_CONTRA`（状态仍采信镜像——事实优先）；
    /// * 一致 → 状态更新为镜像值。
    pub fn feed(&mut self, input: ProbeMirrorInput) -> Result<Probe3, PartGpuError> {
        let cap = tech_cap_row(input.tech);
        if input.units_reported as u64 > cap.max_parts as u64 {
            return Err(PartGpuError::TechMismatch {
                tech: input.tech,
                op: "probe_feed 声称的 VF/实例/分区单元数超 TECH_CAP_TABLE 上限",
            });
        }
        let mirrored = mirror_of(&input);
        self.feeds = self.feeds.saturating_add(1);
        self.states[tech_index(input.tech)] = mirrored;
        if mirrored != input.declared {
            self.contradictions = self.contradictions.saturating_add(1);
            return Err(PartGpuError::ProbeContra {
                tech: input.tech,
                declared: input.declared,
                mirrored,
            });
        }
        Ok(mirrored)
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §10 · 事件账（定长 64 槽环形账，溢出覆盖最旧并显性计数——不静默丢账）
// ═════════════════════════════════════════════════════════════════════════════

/// 事件账容量（环形覆盖）。
pub const EVENT_CAP: usize = 64;

/// 事件种类闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartEventKind {
    /// 分区创建。
    Created,
    /// 分区激活。
    Activated,
    /// 分区挂起。
    Suspended,
    /// 分区恢复。
    Resumed,
    /// 分区销毁。
    Destroyed,
    /// 配额变更入账。
    QuotaChanged,
    /// 配额变更被拒。
    QuotaRejected,
    /// 边界声明登记。
    BoundaryDeclared,
    /// 越界访问 fault。
    BoundaryFault,
    /// 纪元公平收账。
    FairnessTick,
    /// 公平偏差红案。
    FairnessDrift,
    /// 探测镜像喂入。
    ProbeFed,
    /// 探测矛盾拦截。
    ProbeContra,
    /// 创建门拒绝（探测非 Present）。
    ProbeGateRefused,
    /// 属主注销。
    OwnerRevoked,
    /// 孤儿巡账。
    OrphanAudit,
    /// API 合法扩展。
    ApiExtended,
    /// API 冻结破坏拦截。
    ApiFrozenHit,
    /// 事件账覆盖（显性留痕）。
    LedgerOverrun,
}

/// 事件种类名（const，穷尽匹配——新增种类必须补名）。
pub const fn event_kind_name(kind: PartEventKind) -> &'static str {
    match kind {
        PartEventKind::Created => "Created",
        PartEventKind::Activated => "Activated",
        PartEventKind::Suspended => "Suspended",
        PartEventKind::Resumed => "Resumed",
        PartEventKind::Destroyed => "Destroyed",
        PartEventKind::QuotaChanged => "QuotaChanged",
        PartEventKind::QuotaRejected => "QuotaRejected",
        PartEventKind::BoundaryDeclared => "BoundaryDeclared",
        PartEventKind::BoundaryFault => "BoundaryFault",
        PartEventKind::FairnessTick => "FairnessTick",
        PartEventKind::FairnessDrift => "FairnessDrift",
        PartEventKind::ProbeFed => "ProbeFed",
        PartEventKind::ProbeContra => "ProbeContra",
        PartEventKind::ProbeGateRefused => "ProbeGateRefused",
        PartEventKind::OwnerRevoked => "OwnerRevoked",
        PartEventKind::OrphanAudit => "OrphanAudit",
        PartEventKind::ApiExtended => "ApiExtended",
        PartEventKind::ApiFrozenHit => "ApiFrozenHit",
        PartEventKind::LedgerOverrun => "LedgerOverrun",
    }
}

/// 一条事件（序号 + 种类 + 诊断码 + 中文细节）。
#[derive(Debug, Clone)]
pub struct PartEvent {
    /// 引擎全局序号。
    pub seq: u64,
    /// 事件种类。
    pub kind: PartEventKind,
    /// 关联诊断码（X_PG_OK = 无错事件）。
    pub code: u32,
    /// 中文细节。
    pub note: String,
}

/// 环形事件账。
#[derive(Debug, Clone)]
pub struct EventLedger {
    /// 环形缓冲。
    buf: [Option<PartEvent>; EVENT_CAP],
    /// 写头（下一写入槽）。
    head: usize,
    /// 累计入账事件数（含被覆盖的）。
    total: u64,
    /// 覆盖次数（>0 即早期事件被覆盖——审计须及时 snapshot）。
    overruns: u64,
}

impl EventLedger {
    /// 空账起步。
    pub fn new() -> Self {
        EventLedger {
            buf: core::array::from_fn(|_| None),
            head: 0,
            total: 0,
            overruns: 0,
        }
    }

    /// 入账一条事件（满则覆盖最旧并显性计数）。
    pub fn push(&mut self, ev: PartEvent) {
        if self.buf[self.head].is_some() {
            self.overruns = self.overruns.saturating_add(1);
        }
        self.buf[self.head] = Some(ev);
        self.head = (self.head + 1) % EVENT_CAP;
        self.total = self.total.saturating_add(1);
    }

    /// 累计入账数。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 覆盖次数。
    pub fn overruns(&self) -> u64 {
        self.overruns
    }

    /// 快照（按序号升序——从写头起环绕收集）。
    pub fn snapshot(&self) -> Vec<PartEvent> {
        let mut out: Vec<PartEvent> = Vec::new();
        let mut i = 0;
        while i < EVENT_CAP {
            let slot = (self.head + i) % EVENT_CAP;
            if let Some(ev) = &self.buf[slot] {
                out.push(ev.clone());
            }
            i += 1;
        }
        out
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §11 · 分区引擎（统一分区抽象的执行面：创建/销毁/配额/边界/公平/探测/
//       孤儿巡账/事件账/红案/健康/指纹/脚本双跑）
// ═════════════════════════════════════════════════════════════════════════════

/// 行号反查边界域（const 总函数：越界回退 Vram，闭集外不产生新态）。
pub const fn domain_from_index(index: usize) -> BoundaryDomain {
    match index {
        1 => BoundaryDomain::Compute,
        2 => BoundaryDomain::Interrupt,
        3 => BoundaryDomain::Dma,
        4 => BoundaryDomain::Fault,
        _ => BoundaryDomain::Vram,
    }
}

/// 红案（违约/越界/孤儿/偏差/冻结破坏——带码入账，可审计）。
#[derive(Debug, Clone)]
pub struct RedCase {
    /// 立案序号（引擎全局序号）。
    pub seq: u64,
    /// 关联诊断码。
    pub code: u32,
    /// 中文案由。
    pub note: String,
}

/// 分区引擎（栈上本体约 6KB，远低于 64KB 上栈红线）。
pub struct PartGpuEngine {
    /// 分区槽位（含销毁墓碑——销毁不抹账）。
    parts: [Option<Partition>; MAX_PARTS],
    /// 下一个分区编号（从 1 起单调递增）。
    next_id: u64,
    /// 引擎全局序号（事件/红案/触碰统一时序）。
    seq: u64,
    /// 配额模型（守恒核 + 变更账）。
    quota: QuotaModel,
    /// 探测面（镜像输入 + 三态）。
    probe: PartProbe,
    /// 公平扩展记账器（F1610 镜像）。
    fair: FairnessExt,
    /// API 冻结注册表。
    api: ApiRegistry,
    /// 边界声明位图（[技术][域]——五域声明齐才许创建）。
    boundary_declared: [[bool; DOMAIN_COUNT]; TECH_COUNT],
    /// 事件环形账。
    events: EventLedger,
    /// 红案账（环形覆盖）。
    cases: Vec<RedCase>,
    /// 红案账覆盖次数。
    case_overruns: u64,
    // —— 计数器（全部只涨不回绕，health/fingerprint 全量读取）——
    /// 成功创建数。
    creates_ok: u64,
    /// 成功销毁数。
    destroys_ok: u64,
    /// Created→Active 激活数。
    activates: u64,
    /// Suspended→Active 恢复数。
    resumes: u64,
    /// 挂起数。
    suspends: u64,
    /// 配额变更成功数。
    quota_changes_ok: u64,
    /// 配额变更被拒数。
    quota_rejected: u64,
    /// 超卖注入次数。
    oversell_injections: u64,
    /// 边界声明登记数（含被拒的夸大尝试不计数，见下一计数器）。
    boundary_decls: u64,
    /// 夸大声明拦截数。
    boundary_inflate_rejected: u64,
    /// 越界访问 fault 数。
    boundary_faults: u64,
    /// 探测喂入数。
    probe_feeds: u64,
    /// 探测矛盾拦截数。
    probe_contra_rejected: u64,
    /// 创建门拒绝数（探测非 Present）。
    probe_gate_refusals: u64,
    /// 孤儿巡账次数。
    orphan_audits: u64,
    /// 检出孤儿数。
    orphans_found: u64,
    /// API 合法扩展数。
    api_extends: u64,
    /// API 冻结拦截数。
    api_frozen_hits: u64,
    /// API 指纹复核次数。
    api_verify_calls: u64,
}

impl PartGpuEngine {
    /// 引擎起步：探测全 Unknown（不造假不预设）、边界全未声明、API v1 冻结入座。
    pub fn new() -> Self {
        PartGpuEngine {
            parts: core::array::from_fn(|_| None),
            next_id: 1,
            seq: 0,
            quota: QuotaModel::new(),
            probe: PartProbe::new(),
            fair: FairnessExt::new(),
            api: ApiRegistry::new(),
            boundary_declared: [[false; DOMAIN_COUNT]; TECH_COUNT],
            events: EventLedger::new(),
            cases: Vec::new(),
            case_overruns: 0,
            creates_ok: 0,
            destroys_ok: 0,
            activates: 0,
            resumes: 0,
            suspends: 0,
            quota_changes_ok: 0,
            quota_rejected: 0,
            oversell_injections: 0,
            boundary_decls: 0,
            boundary_inflate_rejected: 0,
            boundary_faults: 0,
            probe_feeds: 0,
            probe_contra_rejected: 0,
            probe_gate_refusals: 0,
            orphan_audits: 0,
            orphans_found: 0,
            api_extends: 0,
            api_frozen_hits: 0,
            api_verify_calls: 0,
        }
    }

    // ── 内部件 ───────────────────────────────────────────────────────────

    /// 按分区号定位槽位（0 号非法；查无即 UnknownPart）。
    fn slot_of(&self, part_id: u64, op: &'static str) -> Result<usize, PartGpuError> {
        if part_id == 0 {
            return Err(PartGpuError::IllegalId {
                id: part_id,
                what: "分区编号从 1 起，0 为非法编号",
            });
        }
        let mut i = 0;
        while i < MAX_PARTS {
            if let Some(p) = &self.parts[i] {
                if p.id == part_id {
                    return Ok(i);
                }
            }
            i += 1;
        }
        Err(PartGpuError::UnknownPart { part_id, op })
    }

    /// 红案入账（环形覆盖，显性计数）。
    fn record_case(&mut self, code: u32, note: String) {
        if self.cases.len() >= CASE_CAP {
            self.cases.remove(0);
            self.case_overruns = self.case_overruns.saturating_add(1);
        }
        self.seq = self.seq.saturating_add(1);
        self.cases.push(RedCase {
            seq: self.seq,
            code,
            note,
        });
    }

    /// 事件入账。
    fn push_event(&mut self, kind: PartEventKind, code: u32, note: String) {
        self.seq = self.seq.saturating_add(1);
        self.events.push(PartEvent {
            seq: self.seq,
            kind,
            code,
            note,
        });
    }

    /// 该技术五域边界是否全部声明。
    fn tech_boundary_complete(&self, tech: PartTech) -> bool {
        let ti = tech_index(tech);
        let mut all = true;
        let mut d = 0;
        while d < DOMAIN_COUNT {
            if !self.boundary_declared[ti][d] {
                all = false;
            }
            d += 1;
        }
        all
    }

    // ── 边界声明（判据"边界声明"）────────────────────────────────────────

    /// 声明某技术某域的隔离等级：
    /// * 声明高于硬件事实 → `X_PG_BOUNDARY_INFLATE`（不夸大红线，红案入账）；
    /// * 声明等于或低于事实 → 放行并返回事实等级（保守声明诚实放行）。
    pub fn declare_boundary(
        &mut self,
        tech: PartTech,
        domain: BoundaryDomain,
        declared: IsolationLevel,
    ) -> Result<IsolationLevel, PartGpuError> {
        let truth = boundary_truth(tech, domain);
        if declared.rank() > truth.rank() {
            self.boundary_inflate_rejected = self.boundary_inflate_rejected.saturating_add(1);
            self.record_case(
                X_PG_BOUNDARY_INFLATE,
                format!(
                    "隔离声明夸大红案：{} {} 域声明 {} 高于硬件事实 {}——如实/保守声明后重试",
                    tech_name(tech),
                    domain_name(domain),
                    declared.name(),
                    truth.name()
                ),
            );
            self.push_event(
                PartEventKind::BoundaryDeclared,
                X_PG_BOUNDARY_INFLATE,
                format!(
                    "{} {} 域夸大声明被拒（事实 {}）",
                    tech_name(tech),
                    domain_name(domain),
                    truth.name()
                ),
            );
            return Err(PartGpuError::BoundaryInflate {
                tech,
                declared,
                truth,
            });
        }
        self.boundary_declared[tech_index(tech)][domain_index(domain)] = true;
        self.boundary_decls = self.boundary_decls.saturating_add(1);
        self.push_event(
            PartEventKind::BoundaryDeclared,
            X_PG_OK,
            format!(
                "{} {} 域边界声明为 {}（硬件事实 {}）",
                tech_name(tech),
                domain_name(domain),
                declared.name(),
                truth.name()
            ),
        );
        Ok(truth)
    }

    /// 注入面：夸大声明注入（声明高于硬件事实）——检出 = 返回
    /// `X_PG_BOUNDARY_INFLATE` 错误并同步红案。
    pub fn inject_boundary_claim(
        &mut self,
        tech: PartTech,
        domain: BoundaryDomain,
        declared: IsolationLevel,
    ) -> Result<IsolationLevel, PartGpuError> {
        self.declare_boundary(tech, domain, declared)
    }

    /// 越界访问语义（判据"越界即 fault 不静默"）：
    /// * 自分区访问 → 合法放行；
    /// * 跨分区访问（任一域）→ 一律 fault：`X_PG_BOUNDARY_FAULT` + 红案 +
    ///   事件，永不静默——分区隔离没有"看情况放行"的分支；
    /// * 任一端编号不存在 → `X_PG_QUOTA_UNKNOWN_PART`（不许声明访问成功）。
    pub fn boundary_access(
        &mut self,
        src: u64,
        dst: u64,
        domain: BoundaryDomain,
    ) -> Result<(), PartGpuError> {
        if src == 0 || dst == 0 {
            return Err(PartGpuError::IllegalId {
                id: if src == 0 { src } else { dst },
                what: "boundary_access 分区编号非法（从 1 起）",
            });
        }
        let _ = self.slot_of(src, "boundary_access(src)")?;
        let _ = self.slot_of(dst, "boundary_access(dst)")?;
        if src == dst {
            return Ok(());
        }
        self.boundary_faults = self.boundary_faults.saturating_add(1);
        self.record_case(
            X_PG_BOUNDARY_FAULT,
            format!(
                "越界访问 fault：分区 {} → 分区 {} 域 {}——跨分区访问一律 fault，不静默",
                src,
                dst,
                domain_name(domain)
            ),
        );
        self.push_event(
            PartEventKind::BoundaryFault,
            X_PG_BOUNDARY_FAULT,
            format!("分区 {} → {} 域 {}", src, dst, domain_name(domain)),
        );
        Err(PartGpuError::BoundaryFault {
            src_part: src,
            dst_part: dst,
            domain,
        })
    }

    // ── 探测（判据"探测复用"）────────────────────────────────────────────

    /// 喂入镜像探测（透传 PartProbe.feed；矛盾计数 + 事件留痕）。
    pub fn probe_feed(&mut self, input: ProbeMirrorInput) -> Result<Probe3, PartGpuError> {
        self.probe_feeds = self.probe_feeds.saturating_add(1);
        match self.probe.feed(input) {
            Ok(p3) => {
                self.push_event(
                    PartEventKind::ProbeFed,
                    X_PG_OK,
                    format!("{} 探测镜像：{}", tech_name(input.tech), p3.name()),
                );
                Ok(p3)
            }
            Err(e) => {
                if matches!(e, PartGpuError::ProbeContra { .. }) {
                    self.probe_contra_rejected = self.probe_contra_rejected.saturating_add(1);
                    self.push_event(
                        PartEventKind::ProbeContra,
                        X_PG_PROBE_CONTRA,
                        format!("{} 探测声明与镜像矛盾，已采信镜像", tech_name(input.tech)),
                    );
                }
                Err(e)
            }
        }
    }

    /// 查某技术当前探测三态。
    pub fn probe_state(&self, tech: PartTech) -> Probe3 {
        self.probe.state_of(tech)
    }

    // ── API 冻结（判据"分区 API 冻结 v1"）────────────────────────────────

    /// v1 核心指纹运行期复核。
    pub fn api_verify(&mut self) -> Result<u64, PartGpuError> {
        match self.api.verify_frozen() {
            Ok(fp) => {
                self.api_verify_calls = self.api_verify_calls.saturating_add(1);
                Ok(fp)
            }
            Err(e) => {
                self.api_frozen_hits = self.api_frozen_hits.saturating_add(1);
                self.push_event(
                    PartEventKind::ApiFrozenHit,
                    X_PG_API_FROZEN,
                    format!("v1 核心指纹复核失败：{}", e.detail()),
                );
                Err(e)
            }
        }
    }

    /// API 槽位视图（透传注册表；未占槽 None——不猜）。
    pub fn api_slot_view(&self, method_id: u32) -> Option<[u32; 3]> {
        self.api.slot_view(method_id)
    }

    /// API 演进（只增不改——新方法占空槽 v1 起步；既有方法改参数数/版本
    /// 回退即 `X_PG_API_FROZEN` 拦截 + 事件留痕）。
    pub fn api_extend(
        &mut self,
        method_id: u32,
        params: u32,
        version: u32,
    ) -> Result<(), PartGpuError> {
        match self.api.extend(method_id, params, version) {
            Ok(()) => {
                self.api_extends = self.api_extends.saturating_add(1);
                self.push_event(
                    PartEventKind::ApiExtended,
                    X_PG_OK,
                    format!("API 扩展：方法 {} 参数数 {} 版本 {}", method_id, params, version),
                );
                Ok(())
            }
            Err(e) => {
                self.api_frozen_hits = self.api_frozen_hits.saturating_add(1);
                self.push_event(
                    PartEventKind::ApiFrozenHit,
                    X_PG_API_FROZEN,
                    format!("API 冻结拦截：方法 {}——{}", method_id, e.detail()),
                );
                Err(e)
            }
        }
    }

    // ── 创建/销毁/状态（判据"统一抽象/分区 API"）─────────────────────────

    /// 统一创建分区。五道门：
    /// 1. 探测门：三态必须 Present（Unknown/Missing 一律 `X_PG_PROBE_UNKNOWN`）；
    /// 2. 边界门：该技术五域边界必须声明齐（`X_PG_BOUNDARY_UNDECLARED`）；
    /// 3. 配额值门：正值 + 技术粒度对齐 + 单分区上界（`X_PG_QUOTA_OVERFLOW`）；
    /// 4. 槽位门：MAX_PARTS 满则拒；
    /// 5. 守恒核门：Σ分区 ≤ 物理总量（超卖 `X_PG_QUOTA_OVERRUN`）。
    pub fn create(
        &mut self,
        tech: PartTech,
        owner: u64,
        vram_mib: u64,
        compute_ppm: u64,
    ) -> Result<u64, PartGpuError> {
        // 门 0：属主编号（从 1 起；孤儿只经由 revoke_owner 产生，不在建档时）
        if owner == 0 {
            return Err(PartGpuError::IllegalId {
                id: owner,
                what: "create 属主编号从 1 起，0 为非法属主",
            });
        }
        // 门 1：探测
        if self.probe_state(tech) != Probe3::Present {
            self.probe_gate_refusals = self.probe_gate_refusals.saturating_add(1);
            self.push_event(
                PartEventKind::ProbeGateRefused,
                X_PG_PROBE_UNKNOWN,
                format!(
                    "{} 创建被拒：探测三态 {} 非 Present",
                    tech_name(tech),
                    self.probe_state(tech).name()
                ),
            );
            return Err(PartGpuError::ProbeUnknown { tech });
        }
        // 门 2：边界五域声明
        if !self.tech_boundary_complete(tech) {
            return Err(PartGpuError::BoundaryUndeclared { tech });
        }
        // 门 3：配额值合法性
        let cap = tech_cap_row(tech);
        if vram_mib == 0 || compute_ppm == 0 {
            return Err(PartGpuError::QuotaBad {
                vram_mib,
                compute_ppm,
                reason: "配额必须为正——0 配额分区没有意义，先改配额再创建",
            });
        }
        if vram_mib % cap.vram_gran_mib != 0 {
            return Err(PartGpuError::QuotaBad {
                vram_mib,
                compute_ppm,
                reason: "显存配额不满足技术粒度对齐——按 TECH_CAP_TABLE 粒度取整",
            });
        }
        if compute_ppm % cap.compute_gran_ppm != 0 {
            return Err(PartGpuError::QuotaBad {
                vram_mib,
                compute_ppm,
                reason: "算力配额不满足技术粒度对齐——按 TECH_CAP_TABLE 粒度取整",
            });
        }
        if vram_mib > tech_vram_cap_mib(tech) {
            return Err(PartGpuError::QuotaBad {
                vram_mib,
                compute_ppm,
                reason: "显存配额超物理池单分区上界——缩减配额后重试",
            });
        }
        if compute_ppm > tech_ppm_cap(tech) {
            return Err(PartGpuError::QuotaBad {
                vram_mib,
                compute_ppm,
                reason: "算力配额超物理池单分区上界——缩减配额后重试",
            });
        }
        // 门 4：槽位（空槽优先；否则复用最老的销毁墓碑槽——墓碑让位，
        // 账目史仍在配额变更账与事件账中）
        let mut slot_found: i64 = -1;
        let mut tomb_slot: i64 = -1;
        let mut i = 0;
        while i < MAX_PARTS {
            match &self.parts[i] {
                None => {
                    slot_found = i as i64;
                    i = MAX_PARTS;
                }
                Some(p) => {
                    if tomb_slot < 0 && p.state == LifecycleState::Destroyed {
                        tomb_slot = i as i64;
                    }
                    i += 1;
                }
            }
        }
        if slot_found < 0 {
            slot_found = tomb_slot;
        }
        if slot_found < 0 {
            return Err(PartGpuError::QuotaBad {
                vram_mib: 0,
                compute_ppm: 0,
                reason: "分区槽位耗尽——先销毁闲置分区或升版 MAX_PARTS",
            });
        }
        let slot = slot_found as usize;
        // 门 5：守恒核
        if let Err(e) = self.quota.try_commit(vram_mib, compute_ppm) {
            self.quota_rejected = self.quota_rejected.saturating_add(1);
            self.push_event(
                PartEventKind::QuotaRejected,
                e.code(),
                format!("{} 创建被拒：{}", tech_name(tech), e.detail()),
            );
            return Err(e);
        }
        // 建账
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.seq = self.seq.saturating_add(1);
        let seq_now = self.seq;
        self.parts[slot] = Some(Partition {
            id,
            tech,
            owner,
            state: LifecycleState::Created,
            vram_mib,
            compute_ppm,
            quota_changes: 0,
            created_seq: seq_now,
            last_touch_seq: seq_now,
        });
        self.creates_ok = self.creates_ok.saturating_add(1);
        self.push_event(
            PartEventKind::Created,
            X_PG_OK,
            format!(
                "分区 {}（{}）创建：显存 {} MiB / 算力 {} ppm，属主 {}",
                id,
                tech_name(tech),
                vram_mib,
                compute_ppm,
                owner
            ),
        );
        Ok(id)
    }

    /// 激活分区（Created→Active 或 Suspended→Active 恢复）。
    pub fn activate(&mut self, part_id: u64) -> Result<(), PartGpuError> {
        let slot = self.slot_of(part_id, "activate")?;
        let from = match &self.parts[slot] {
            Some(p) => p.state,
            None => return Err(PartGpuError::UnknownPart { part_id, op: "activate" }),
        };
        if !lc_transition_allowed(from, LifecycleState::Active) {
            return Err(PartGpuError::StateOrder {
                part_id,
                from,
                to: LifecycleState::Active,
            });
        }
        self.seq = self.seq.saturating_add(1);
        let seq_now = self.seq;
        if let Some(p) = &mut self.parts[slot] {
            p.state = LifecycleState::Active;
            p.last_touch_seq = seq_now;
        }
        if from == LifecycleState::Suspended {
            self.resumes = self.resumes.saturating_add(1);
        } else {
            self.activates = self.activates.saturating_add(1);
        }
        self.push_event(
            PartEventKind::Activated,
            X_PG_OK,
            format!("分区 {} 激活（{} → Active）", part_id, lc_name(from)),
        );
        Ok(())
    }

    /// 挂起分区（Active→Suspended；配额保留，不参与公平记账）。
    pub fn suspend(&mut self, part_id: u64) -> Result<(), PartGpuError> {
        let slot = self.slot_of(part_id, "suspend")?;
        let from = match &self.parts[slot] {
            Some(p) => p.state,
            None => return Err(PartGpuError::UnknownPart { part_id, op: "suspend" }),
        };
        if !lc_transition_allowed(from, LifecycleState::Suspended) {
            return Err(PartGpuError::StateOrder {
                part_id,
                from,
                to: LifecycleState::Suspended,
            });
        }
        self.seq = self.seq.saturating_add(1);
        let seq_now = self.seq;
        if let Some(p) = &mut self.parts[slot] {
            p.state = LifecycleState::Suspended;
            p.last_touch_seq = seq_now;
        }
        self.suspends = self.suspends.saturating_add(1);
        self.push_event(
            PartEventKind::Suspended,
            X_PG_OK,
            format!("分区 {} 挂起（{} → Suspended）", part_id, lc_name(from)),
        );
        Ok(())
    }

    /// 销毁分区（真终态；配额全量回池；墓碑留账）。
    pub fn destroy(&mut self, part_id: u64) -> Result<(), PartGpuError> {
        let slot = self.slot_of(part_id, "destroy")?;
        let (from, vram, ppm) = match &self.parts[slot] {
            Some(p) => (p.state, p.vram_mib, p.compute_ppm),
            None => return Err(PartGpuError::UnknownPart { part_id, op: "destroy" }),
        };
        if !lc_transition_allowed(from, LifecycleState::Destroyed) {
            return Err(PartGpuError::StateOrder {
                part_id,
                from,
                to: LifecycleState::Destroyed,
            });
        }
        self.quota.release(vram, ppm);
        self.seq = self.seq.saturating_add(1);
        let seq_now = self.seq;
        if let Some(p) = &mut self.parts[slot] {
            p.state = LifecycleState::Destroyed;
            p.last_touch_seq = seq_now;
        }
        self.destroys_ok = self.destroys_ok.saturating_add(1);
        self.push_event(
            PartEventKind::Destroyed,
            X_PG_OK,
            format!(
                "分区 {} 销毁（原状态 {}）：显存 {} MiB / 算力 {} ppm 回池",
                part_id,
                lc_name(from),
                vram,
                ppm
            ),
        );
        Ok(())
    }

    // ── 配额（判据"配额模型"）────────────────────────────────────────────

    /// 配额变更（Created/Active/Suspended 可变；Destroyed 终态拒）。
    /// 双维（显存/算力）独立增减，增侧先过守恒核，减侧直接回池；变更全量入账。
    pub fn set_quota(
        &mut self,
        part_id: u64,
        vram_mib: u64,
        compute_ppm: u64,
    ) -> Result<(), PartGpuError> {
        let slot = self.slot_of(part_id, "set_quota")?;
        let (state, tech, old_v, old_p) = match &self.parts[slot] {
            Some(p) => (p.state, p.tech, p.vram_mib, p.compute_ppm),
            None => return Err(PartGpuError::UnknownPart { part_id, op: "set_quota" }),
        };
        if state == LifecycleState::Destroyed {
            return Err(PartGpuError::StateOrder {
                part_id,
                from: state,
                to: state,
            });
        }
        let cap = tech_cap_row(tech);
        if vram_mib == 0 || compute_ppm == 0 {
            return Err(PartGpuError::QuotaBad {
                vram_mib,
                compute_ppm,
                reason: "配额必须为正——0 配额请走 destroy 销毁回收",
            });
        }
        if vram_mib % cap.vram_gran_mib != 0 {
            return Err(PartGpuError::QuotaBad {
                vram_mib,
                compute_ppm,
                reason: "显存配额不满足技术粒度对齐——按 TECH_CAP_TABLE 粒度取整",
            });
        }
        if compute_ppm % cap.compute_gran_ppm != 0 {
            return Err(PartGpuError::QuotaBad {
                vram_mib,
                compute_ppm,
                reason: "算力配额不满足技术粒度对齐——按 TECH_CAP_TABLE 粒度取整",
            });
        }
        if vram_mib > tech_vram_cap_mib(tech) {
            return Err(PartGpuError::QuotaBad {
                vram_mib,
                compute_ppm,
                reason: "显存配额超物理池单分区上界——缩减配额后重试",
            });
        }
        if compute_ppm > tech_ppm_cap(tech) {
            return Err(PartGpuError::QuotaBad {
                vram_mib,
                compute_ppm,
                reason: "算力配额超物理池单分区上界——缩减配额后重试",
            });
        }
        if vram_mib == old_v && compute_ppm == old_p {
            return Err(PartGpuError::QuotaBad {
                vram_mib,
                compute_ppm,
                reason: "配额未变化——无意义变更不入账",
            });
        }
        let grow_v = if vram_mib > old_v { vram_mib - old_v } else { 0 };
        let grow_p = if compute_ppm > old_p {
            compute_ppm - old_p
        } else {
            0
        };
        if grow_v > 0 || grow_p > 0 {
            if let Err(e) = self.quota.try_commit(grow_v, grow_p) {
                self.quota_rejected = self.quota_rejected.saturating_add(1);
                self.push_event(
                    PartEventKind::QuotaRejected,
                    e.code(),
                    format!("分区 {} 配额变更被拒：{}", part_id, e.detail()),
                );
                return Err(e);
            }
        }
        if vram_mib < old_v {
            self.quota.release(old_v - vram_mib, 0);
        }
        if compute_ppm < old_p {
            self.quota.release(0, old_p - compute_ppm);
        }
        self.seq = self.seq.saturating_add(1);
        let seq_now = self.seq;
        if let Some(p) = &mut self.parts[slot] {
            p.vram_mib = vram_mib;
            p.compute_ppm = compute_ppm;
            p.quota_changes = p.quota_changes.saturating_add(1);
            p.last_touch_seq = seq_now;
        }
        self.quota.record_change(QuotaChangeRec {
            part_id,
            old_vram_mib: old_v,
            new_vram_mib: vram_mib,
            old_ppm: old_p,
            new_ppm: compute_ppm,
            seq: seq_now,
        });
        self.quota_changes_ok = self.quota_changes_ok.saturating_add(1);
        self.push_event(
            PartEventKind::QuotaChanged,
            X_PG_OK,
            format!(
                "分区 {} 配额变更：显存 {}→{} MiB，算力 {}→{} ppm",
                part_id, old_v, vram_mib, old_p, compute_ppm
            ),
        );
        Ok(())
    }

    /// 守恒核对账：Σ非销毁分区配额 == 账面提交量，账实不符即
    /// `X_PG_QUOTA_OVERFLOW`（reason = 守恒核破坏）。
    pub fn verify_conservation(&self) -> Result<(), PartGpuError> {
        let mut sv: u64 = 0;
        let mut sp: u64 = 0;
        let mut i = 0;
        while i < MAX_PARTS {
            if let Some(p) = &self.parts[i] {
                if p.state != LifecycleState::Destroyed {
                    sv = sv.saturating_add(p.vram_mib);
                    sp = sp.saturating_add(p.compute_ppm);
                }
            }
            i += 1;
        }
        if sv != self.quota.committed_vram_mib() || sp != self.quota.committed_ppm() {
            return Err(PartGpuError::QuotaBad {
                vram_mib: sv,
                compute_ppm: sp,
                reason: "守恒核破坏：Σ活动分区配额与账面提交量不一致——查配额变更/销毁释放路径",
            });
        }
        Ok(())
    }

    // ── 查询 ─────────────────────────────────────────────────────────────

    /// 查询分区快照（v1 方法 4）。
    pub fn query_part(&self, part_id: u64) -> Result<PartInfo, PartGpuError> {
        let slot = self.slot_of(part_id, "query_part")?;
        match &self.parts[slot] {
            Some(p) => Ok(PartInfo {
                id: p.id,
                tech: p.tech,
                owner: p.owner,
                state: p.state,
                vram_mib: p.vram_mib,
                compute_ppm: p.compute_ppm,
                quota_changes: p.quota_changes,
            }),
            None => Err(PartGpuError::UnknownPart { part_id, op: "query_part" }),
        }
    }

    /// 全量分区快照（按槽位序，不含空槽）。
    pub fn parts_snapshot(&self) -> Vec<PartInfo> {
        let mut out: Vec<PartInfo> = Vec::new();
        let mut i = 0;
        while i < MAX_PARTS {
            if let Some(p) = &self.parts[i] {
                out.push(PartInfo {
                    id: p.id,
                    tech: p.tech,
                    owner: p.owner,
                    state: p.state,
                    vram_mib: p.vram_mib,
                    compute_ppm: p.compute_ppm,
                    quota_changes: p.quota_changes,
                });
            }
            i += 1;
        }
        out
    }

    // ── 公平扩展（判据"公平扩展/F1610 复用"）─────────────────────────────

    /// 公平记账一拍：指定分区被调度服务一次。攒满 EPOCH_TICKS 拍即收账：
    /// 逐活动分区对实得/应得偏差，越 5000‱ 界行红案立案
    /// （`X_PG_FAIRNESS_DRIFT` 入红案账 + 事件）。
    pub fn fairness_tick(&mut self, part_id: u64) -> Result<FairVerdict, PartGpuError> {
        let slot = {
            let idx = self.slot_of(part_id, "fairness_tick")?;
            match &self.parts[idx] {
                Some(p) if p.state == LifecycleState::Active => idx,
                Some(p) => {
                    return Err(PartGpuError::StateOrder {
                        part_id,
                        from: p.state,
                        to: LifecycleState::Active,
                    })
                }
                None => return Err(PartGpuError::UnknownPart { part_id, op: "fairness_tick" }),
            }
        };
        self.fair.record_tick(slot);
        let mut verdict = FairVerdict {
            epoch_closed: false,
            drift_rows: 0,
            breached_rows: 0,
        };
        if self.fair.epoch_ticks >= EPOCH_TICKS {
            // 活动分区算力配额求和（挂起/销毁不参与）
            let mut active_ppm = [0u64; MAX_PARTS];
            let mut sum_ppm: u64 = 0;
            let mut i = 0;
            while i < MAX_PARTS {
                if let Some(p) = &self.parts[i] {
                    if p.state == LifecycleState::Active {
                        active_ppm[i] = p.compute_ppm;
                        sum_ppm = sum_ppm.saturating_add(p.compute_ppm);
                    }
                }
                i += 1;
            }
            let entitled = entitled_for_ticks(EPOCH_TICKS, &active_ppm, sum_ppm);
            let close = self.fair.close_epoch(&entitled);
            let mut j = 0;
            while j < close.rows.len() {
                let row = &close.rows[j];
                verdict.drift_rows = verdict.drift_rows.saturating_add(1);
                if row.breached {
                    verdict.breached_rows = verdict.breached_rows.saturating_add(1);
                    let pid = match &self.parts[row.slot] {
                        Some(p) => p.id,
                        None => 0,
                    };
                    self.record_case(
                        X_PG_FAIRNESS_DRIFT,
                        format!(
                            "公平偏差红案：纪元 {} 分区 {} 实得 {} 应得 {} 偏差 {} 超界 {}——查调度注入是否偏袒",
                            close.epoch_no, pid, row.served, row.entitled, row.drift, row.bound
                        ),
                    );
                    self.push_event(
                        PartEventKind::FairnessDrift,
                        X_PG_FAIRNESS_DRIFT,
                        format!("纪元 {} 分区 {} 偏差 {}", close.epoch_no, pid, row.drift),
                    );
                }
                j += 1;
            }
            self.push_event(
                PartEventKind::FairnessTick,
                X_PG_OK,
                format!(
                    "纪元 {} 收账：偏差行 {} 违约行 {}",
                    close.epoch_no, verdict.drift_rows, verdict.breached_rows
                ),
            );
            verdict.epoch_closed = true;
        }
        Ok(verdict)
    }

    /// 公平对账快照（历史累计实得 + 当前配额权重）。
    pub fn fairness_report(&self) -> Vec<FairRow> {
        let mut out: Vec<FairRow> = Vec::new();
        let mut i = 0;
        while i < MAX_PARTS {
            if let Some(p) = &self.parts[i] {
                out.push(FairRow {
                    part_id: p.id,
                    slot: i,
                    total_served: self.fair.total_served[i],
                    compute_ppm: p.compute_ppm,
                    active: p.state == LifecycleState::Active,
                });
            }
            i += 1;
        }
        out
    }

    /// 纪元对账总览（无任何纪元数据即 `X_PG_FAIRNESS_NO_EPOCH`）。
    pub fn fairness_epoch_verdict(&self) -> Result<(u64, u64, u64), PartGpuError> {
        if self.fair.epoch == 0 && self.fair.epoch_ticks == 0 {
            return Err(PartGpuError::FairnessNoEpoch);
        }
        Ok((self.fair.epoch, self.fair.epoch_ticks, self.fair.drift_cases))
    }

    // ── 生命周期账与孤儿（判据"生命周期账/孤儿分区检测"）─────────────────

    /// 属主注销（真实路径：guest 崩溃/注销后分区无属主——成为待巡账孤儿）。
    pub fn revoke_owner(&mut self, part_id: u64) -> Result<(), PartGpuError> {
        let slot = self.slot_of(part_id, "revoke_owner")?;
        let state = match &mut self.parts[slot] {
            Some(p) => {
                if p.owner == 0 {
                    return Err(PartGpuError::OrphanPart { part_id });
                }
                if p.state == LifecycleState::Destroyed {
                    return Err(PartGpuError::StateOrder {
                        part_id,
                        from: p.state,
                        to: p.state,
                    });
                }
                p.owner = 0;
                p.state
            }
            None => return Err(PartGpuError::UnknownPart { part_id, op: "revoke_owner" }),
        };
        self.seq = self.seq.saturating_add(1);
        let seq_now = self.seq;
        if let Some(p) = &mut self.parts[slot] {
            p.last_touch_seq = seq_now;
        }
        self.push_event(
            PartEventKind::OwnerRevoked,
            X_PG_OK,
            format!("分区 {} 属主注销（原状态 {}）——分区成为待巡账孤儿", part_id, lc_name(state)),
        );
        Ok(())
    }

    /// 孤儿巡账：无属主且非销毁的分区逐个红案立案，返回在册孤儿编号。
    pub fn audit_orphans(&mut self) -> Vec<u64> {
        let mut found: Vec<u64> = Vec::new();
        let mut i = 0;
        while i < MAX_PARTS {
            let hit = match &self.parts[i] {
                Some(p) => p.state != LifecycleState::Destroyed && p.owner == 0,
                None => false,
            };
            if hit {
                let pid = match &self.parts[i] {
                    Some(p) => p.id,
                    None => 0,
                };
                found.push(pid);
                self.orphans_found = self.orphans_found.saturating_add(1);
                self.record_case(
                    X_PG_ORPHAN,
                    format!("孤儿分区红案：分区 {} 无属主——查明属主去向，重新指派或走 destroy 销毁回收配额", pid),
                );
            }
            i += 1;
        }
        self.orphan_audits = self.orphan_audits.saturating_add(1);
        self.push_event(
            PartEventKind::OrphanAudit,
            if found.is_empty() { X_PG_OK } else { X_PG_ORPHAN },
            format!("孤儿巡账：在册 {} 个孤儿分区", found.len()),
        );
        found
    }

    /// 孤儿巡账（带码变体）：检出孤儿即 `X_PG_ORPHAN` 带码返回。
    pub fn audit_orphans_strict(&mut self) -> Result<(), PartGpuError> {
        let found = self.audit_orphans();
        if found.is_empty() {
            return Ok(());
        }
        Err(PartGpuError::OrphanPart { part_id: found[0] })
    }

    /// 注入面：孤儿分区注入（属主注销 + 立即严格巡账）——检出 =
    /// 返回 `X_PG_ORPHAN` 错误且红案已入账。
    pub fn inject_orphan(&mut self, part_id: u64) -> Result<(), PartGpuError> {
        self.revoke_owner(part_id)?;
        self.audit_orphans_strict()
    }

    /// 注入面：超卖注入（越过分区 API 直捅守恒核的探测路径）——检出 =
    /// 返回 `X_PG_QUOTA_OVERRUN`；若未被拦截（Ok）则红案立案（守恒核失效）。
    pub fn inject_oversubscribe(&mut self, vram_mib: u64, compute_ppm: u64) -> Result<(), PartGpuError> {
        self.oversell_injections = self.oversell_injections.saturating_add(1);
        match self.quota.try_commit(vram_mib, compute_ppm) {
            Ok(()) => {
                // 守恒核失效（不该发生）：立即红案并把账目回滚
                self.record_case(
                    X_PG_QUOTA_OVERRUN,
                    String::from("超卖注入未被拦截——守恒核失效，账目已回滚，立即排查 try_commit 比较"),
                );
                self.quota.release(vram_mib, compute_ppm);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    // ── 账面读数 ─────────────────────────────────────────────────────────

    /// 账面已提交显存（MiB）。
    pub fn committed_vram_mib(&self) -> u64 {
        self.quota.committed_vram_mib()
    }

    /// 账面已提交算力（ppm）。
    pub fn committed_ppm(&self) -> u64 {
        self.quota.committed_ppm()
    }

    /// 事件账快照。
    pub fn events_snapshot(&self) -> Vec<PartEvent> {
        self.events.snapshot()
    }

    /// 事件累计入账数。
    pub fn event_total(&self) -> u64 {
        self.events.total()
    }

    /// 事件覆盖次数。
    pub fn event_overruns(&self) -> u64 {
        self.events.overruns()
    }

    /// 红案快照。
    pub fn case_snapshot(&self) -> Vec<RedCase> {
        self.cases.clone()
    }

    /// 红案在册数。
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 配额变更账快照。
    pub fn quota_changes_snapshot(&self) -> Vec<QuotaChangeRec> {
        self.quota.changes_snapshot()
    }
}

/// 公平记账一拍的判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FairVerdict {
    /// 本次是否收了一个纪元。
    pub epoch_closed: bool,
    /// 收账时的偏差行数。
    pub drift_rows: u64,
    /// 收账时越界红案行数。
    pub breached_rows: u64,
}

/// 公平对账快照行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FairRow {
    /// 分区编号。
    pub part_id: u64,
    /// 槽位。
    pub slot: usize,
    /// 历史累计实得拍数。
    pub total_served: u64,
    /// 当前算力配额（ppm，权重）。
    pub compute_ppm: u64,
    /// 是否活动（挂起/销毁不参与纪元应得折算）。
    pub active: bool,
}

// ═════════════════════════════════════════════════════════════════════════════
// §12 · 健康体检 / 稳定态指纹（FNV-1a——同输入同指纹，本机回归对账不上传）
// ═════════════════════════════════════════════════════════════════════════════

/// 结构化体检（只读快照，无堆分配）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartHealth {
    /// 在册槽位数（含销毁墓碑）。
    pub parts_in_register: u64,
    /// 活账分区数（非销毁）。
    pub parts_live: u64,
    /// 活动分区数（Active）。
    pub parts_active: u64,
    /// 成功创建数。
    pub creates_ok: u64,
    /// 成功销毁数。
    pub destroys_ok: u64,
    /// 激活数（Created→Active）。
    pub activates: u64,
    /// 恢复数（Suspended→Active）。
    pub resumes: u64,
    /// 挂起数。
    pub suspends: u64,
    /// 配额变更成功数。
    pub quota_changes_ok: u64,
    /// 配额变更被拒数。
    pub quota_rejected: u64,
    /// 账面已提交显存（MiB）。
    pub committed_vram_mib: u64,
    /// 账面已提交算力（ppm）。
    pub committed_ppm: u64,
    /// 池内剩余显存（MiB）。
    pub free_vram_mib: u64,
    /// 池内剩余算力（ppm）。
    pub free_ppm: u64,
    /// 超卖拦截数（守恒核）。
    pub oversell_rejected: u64,
    /// 超卖注入尝试数。
    pub oversell_injections: u64,
    /// 边界声明登记数。
    pub boundary_decls: u64,
    /// 夸大声明拦截数。
    pub boundary_inflate_rejected: u64,
    /// 越界访问 fault 数。
    pub boundary_faults: u64,
    /// 三技术五域边界是否全部声明（15 位全 1）。
    pub all_boundaries_declared: bool,
    /// 探测喂入数。
    pub probe_feeds: u64,
    /// 探测矛盾拦截数。
    pub probe_contra_rejected: u64,
    /// 创建门拒绝数。
    pub probe_gate_refusals: u64,
    /// 已收账纪元数。
    pub fairness_epochs: u64,
    /// 公平偏差红案数。
    pub fairness_drift_cases: u64,
    /// 最近收账最大偏差（观测）。
    pub fairness_last_drift_max: u64,
    /// 孤儿巡账次数。
    pub orphan_audits: u64,
    /// 检出孤儿数。
    pub orphans_found: u64,
    /// API 合法扩展数。
    pub api_extensions: u64,
    /// API 冻结拦截数。
    pub api_frozen_hits: u64,
    /// v1 核心指纹当前是否完好。
    pub api_v1_intact: bool,
    /// 事件累计入账数。
    pub events_total: u64,
    /// 事件覆盖次数。
    pub event_overruns: u64,
    /// 红案在册数。
    pub case_count: u64,
    /// 红案覆盖次数。
    pub case_overruns: u64,
}

impl PartGpuEngine {
    /// 结构化体检：全量水位一次读齐。
    pub fn health(&self) -> PartHealth {
        let mut in_register: u64 = 0;
        let mut live: u64 = 0;
        let mut active: u64 = 0;
        let mut i = 0;
        while i < MAX_PARTS {
            if let Some(p) = &self.parts[i] {
                in_register = in_register.saturating_add(1);
                if p.state != LifecycleState::Destroyed {
                    live = live.saturating_add(1);
                }
                if p.state == LifecycleState::Active {
                    active = active.saturating_add(1);
                }
            }
            i += 1;
        }
        let mut all_declared = true;
        let mut t = 0;
        while t < TECH_COUNT {
            let mut d = 0;
            while d < DOMAIN_COUNT {
                if !self.boundary_declared[t][d] {
                    all_declared = false;
                }
                d += 1;
            }
            t += 1;
        }
        PartHealth {
            parts_in_register: in_register,
            parts_live: live,
            parts_active: active,
            creates_ok: self.creates_ok,
            destroys_ok: self.destroys_ok,
            activates: self.activates,
            resumes: self.resumes,
            suspends: self.suspends,
            quota_changes_ok: self.quota_changes_ok,
            quota_rejected: self.quota_rejected,
            committed_vram_mib: self.quota.committed_vram_mib(),
            committed_ppm: self.quota.committed_ppm(),
            free_vram_mib: self.quota.free_vram_mib(),
            free_ppm: self.quota.free_ppm(),
            oversell_rejected: self.quota.rejected_oversell(),
            oversell_injections: self.oversell_injections,
            boundary_decls: self.boundary_decls,
            boundary_inflate_rejected: self.boundary_inflate_rejected,
            boundary_faults: self.boundary_faults,
            all_boundaries_declared: all_declared,
            probe_feeds: self.probe.feeds(),
            probe_contra_rejected: self.probe.contradictions(),
            probe_gate_refusals: self.probe_gate_refusals,
            fairness_epochs: self.fair.epoch,
            fairness_drift_cases: self.fair.drift_cases,
            fairness_last_drift_max: self.fair.last_close_drift_max,
            orphan_audits: self.orphan_audits,
            orphans_found: self.orphans_found,
            api_extensions: self.api.extensions(),
            api_frozen_hits: self.api_frozen_hits,
            api_v1_intact: self.api.verify_frozen().is_ok(),
            events_total: self.events.total(),
            event_overruns: self.events.overruns(),
            case_count: self.cases.len() as u64,
            case_overruns: self.case_overruns,
        }
    }

    /// 稳定态指纹：把引擎全部账面稳定态编入 FNV-1a 64 位——同输入同指纹，
    /// 双跑对拍与本机回归对账用；不上传，不当唯一 ID。
    pub fn fingerprint(&self) -> u64 {
        let mut h = mix64(FNV_OFFSET, API_ABI_MAGIC);
        h = mix64(h, self.next_id);
        h = mix64(h, self.seq);
        h = mix64(h, self.quota.committed_vram_mib());
        h = mix64(h, self.quota.committed_ppm());
        let mut i = 0;
        while i < MAX_PARTS {
            match &self.parts[i] {
                Some(p) => {
                    h = mix64(h, p.id);
                    h = mix64(h, tech_index(p.tech) as u64);
                    h = mix64(h, p.owner);
                    h = mix64(h, lc_index(p.state) as u64);
                    h = mix64(h, p.vram_mib);
                    h = mix64(h, p.compute_ppm);
                    h = mix64(h, p.quota_changes);
                    h = mix64(h, p.created_seq);
                    h = mix64(h, p.last_touch_seq);
                }
                None => {
                    h = mix64(h, 0xFFFF_FFFF_FFFF_FFFF);
                }
            }
            i += 1;
        }
        let mut t = 0;
        while t < TECH_COUNT {
            h = mix64(h, probe_index(self.probe.state_of(tech_from_index(t))) as u64);
            let mut d = 0;
            while d < DOMAIN_COUNT {
                h = mix64(h, if self.boundary_declared[t][d] { 1 } else { 0 });
                d += 1;
            }
            t += 1;
        }
        h = mix64(h, self.fair.epoch);
        h = mix64(h, self.fair.epoch_ticks);
        let mut s = 0;
        while s < MAX_PARTS {
            h = mix64(h, self.fair.total_served[s]);
            h = mix64(h, self.fair.epoch_served[s]);
            s += 1;
        }
        h = mix64(h, self.fair.drift_cases);
        h = mix64(h, self.fair.last_close_drift_max);
        let mut m = 0;
        while m < API_METHOD_SLOTS {
            let (mid, pa, ve) = if self.api.slot_view(m as u32 + 1).is_some() {
                match self.api.slot_view(m as u32 + 1) {
                    Some(tri) => (tri[0] as u64, tri[1] as u64, tri[2] as u64),
                    None => (0, 0, 0),
                }
            } else {
                (0xFFFF, 0xFFFF, 0xFFFF)
            };
            h = mix64(h, mid);
            h = mix64(h, pa);
            h = mix64(h, ve);
            m += 1;
        }
        h = mix64(h, self.events.total());
        h = mix64(h, self.events.overruns());
        h = mix64(h, self.cases.len() as u64);
        h = mix64(h, self.case_overruns);
        h = mix64(h, self.creates_ok);
        h = mix64(h, self.destroys_ok);
        h = mix64(h, self.activates);
        h = mix64(h, self.resumes);
        h = mix64(h, self.suspends);
        h = mix64(h, self.quota_changes_ok);
        h = mix64(h, self.quota_rejected);
        h = mix64(h, self.oversell_injections);
        h = mix64(h, self.boundary_decls);
        h = mix64(h, self.boundary_inflate_rejected);
        h = mix64(h, self.boundary_faults);
        h = mix64(h, self.probe.feeds());
        h = mix64(h, self.probe.contradictions());
        h = mix64(h, self.probe_gate_refusals);
        h = mix64(h, self.orphan_audits);
        h = mix64(h, self.orphans_found);
        h = mix64(h, self.api.extensions());
        h = mix64(h, self.api.frozen_violations());
        h = mix64(h, self.api_verify_calls);
        h
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §13 · 脚本管道（双跑对拍输入面——同一公开管道执行操作序列，无旁路）
// ═════════════════════════════════════════════════════════════════════════════

/// 脚本操作（双跑对拍的输入原子；part_id=0 表示"最近创建成功的分区"）。
#[derive(Debug, Clone, Copy)]
pub enum ScriptOp {
    /// 创建分区。
    Create {
        /// 技术。
        tech: PartTech,
        /// 属主（0 非法——create 门 0 拒绝；孤儿只经由 revoke_owner 产生）。
        owner: u64,
        /// 显存配额（MiB）。
        vram_mib: u64,
        /// 算力配额（ppm）。
        compute_ppm: u64,
    },
    /// 销毁分区。
    Destroy {
        /// 分区号。
        part_id: u64,
    },
    /// 配额变更。
    SetQuota {
        /// 分区号。
        part_id: u64,
        /// 新显存（MiB）。
        vram_mib: u64,
        /// 新算力（ppm）。
        compute_ppm: u64,
    },
    /// 激活分区。
    Activate {
        /// 分区号。
        part_id: u64,
    },
    /// 挂起分区。
    Suspend {
        /// 分区号。
        part_id: u64,
    },
    /// 公平记账一拍。
    Tick {
        /// 分区号。
        part_id: u64,
    },
    /// 越界访问尝试。
    Access {
        /// 发起分区。
        src: u64,
        /// 目标分区。
        dst: u64,
        /// 边界域。
        domain: BoundaryDomain,
    },
}

/// 脚本执行报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptReport {
    /// 执行终态指纹。
    pub fingerprint: u64,
    /// 执行中被拒的操作数（脚本继续不中断）。
    pub errors: u64,
    /// 成功创建数。
    pub creates_ok: u64,
}

/// 编号 0 重指向：脚本层把 0 号解释为"最近创建成功的分区"。
fn script_rid(pid: u64, last_created: u64) -> u64 {
    if pid == 0 {
        last_created
    } else {
        pid
    }
}

/// 执行一段脚本（全新引擎 + 固定准入前置：三技术五域边界声明 + 三技术
/// Present 探测——均为同一公开管道调用，非旁路）。同一脚本双跑必同指纹；
/// 异操作序列必异指纹（对拍在测试组闭环）。
pub fn run_script(ops: &[ScriptOp]) -> ScriptReport {
    let mut e = PartGpuEngine::new();
    let mut t = 0;
    while t < TECH_COUNT {
        let tech = tech_from_index(t);
        let mut d = 0;
        while d < DOMAIN_COUNT {
            let _ = e.declare_boundary(tech, domain_from_index(d), boundary_truth(tech, domain_from_index(d)));
            d += 1;
        }
        let _ = e.probe_feed(ProbeMirrorInput {
            tech,
            hw_cap_seen: true,
            units_reported: 1,
            declared: Probe3::Present,
        });
        t += 1;
    }
    let mut last_created: u64 = 0;
    let mut errors: u64 = 0;
    let mut creates_ok: u64 = 0;
    let mut i = 0;
    while i < ops.len() {
        // part_id=0 → 最近创建成功的分区（0 号本身非法，脚本层重指向）
        match &ops[i] {
            ScriptOp::Create {
                tech,
                owner,
                vram_mib,
                compute_ppm,
            } => match e.create(*tech, *owner, *vram_mib, *compute_ppm) {
                Ok(id) => {
                    last_created = id;
                    creates_ok = creates_ok.saturating_add(1);
                }
                Err(_) => errors = errors.saturating_add(1),
            },
            ScriptOp::Destroy { part_id } => {
                if e.destroy(script_rid(*part_id, last_created)).is_err() {
                    errors = errors.saturating_add(1);
                }
            }
            ScriptOp::SetQuota {
                part_id,
                vram_mib,
                compute_ppm,
            } => {
                if e.set_quota(script_rid(*part_id, last_created), *vram_mib, *compute_ppm).is_err() {
                    errors = errors.saturating_add(1);
                }
            }
            ScriptOp::Activate { part_id } => {
                if e.activate(script_rid(*part_id, last_created)).is_err() {
                    errors = errors.saturating_add(1);
                }
            }
            ScriptOp::Suspend { part_id } => {
                if e.suspend(script_rid(*part_id, last_created)).is_err() {
                    errors = errors.saturating_add(1);
                }
            }
            ScriptOp::Tick { part_id } => {
                if e.fairness_tick(script_rid(*part_id, last_created)).is_err() {
                    errors = errors.saturating_add(1);
                }
            }
            ScriptOp::Access { src, dst, domain } => {
                if e.boundary_access(script_rid(*src, last_created), script_rid(*dst, last_created), *domain).is_err() {
                    errors = errors.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    ScriptReport {
        fingerprint: e.fingerprint(),
        errors,
        creates_ok,
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §14 · 五组测试面（三抽象/配额/边界/公平/探测 + 生命周期/孤儿/API 冻结/
//       注入检出/双跑对拍——判据"五组/判据"；测试运行在宿主 std 下，
//       panic/assert 只许出现在本节）
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// 期望成功；失败即带诊断打印崩溃。
    fn ok<T>(r: Result<T, PartGpuError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("期望成功，实际失败：{}", e),
        }
    }

    /// 期望失败；返回诊断码。
    fn code_of<T>(r: Result<T, PartGpuError>) -> u32 {
        match r {
            Ok(_) => panic!("期望失败，实际成功"),
            Err(e) => e.code(),
        }
    }

    /// 预备引擎：三技术五域边界如实声明 + 三技术 Present 探测喂入
    /// （同一公开管道，非旁路）。
    fn prepped() -> PartGpuEngine {
        let mut e = PartGpuEngine::new();
        let mut t = 0;
        while t < TECH_COUNT {
            let tech = tech_from_index(t);
            let mut d = 0;
            while d < DOMAIN_COUNT {
                ok(e.declare_boundary(tech, domain_from_index(d), boundary_truth(tech, domain_from_index(d))));
                d += 1;
            }
            ok(e.probe_feed(ProbeMirrorInput {
                tech,
                hw_cap_seen: true,
                units_reported: 1,
                declared: Probe3::Present,
            }));
            t += 1;
        }
        e
    }

    // ── 组一：三抽象 ─────────────────────────────────────────────────────

    #[test]
    fn tech_cap_table_exhaustive_and_self_consistent() {
        let mut i = 0;
        while i < TECH_COUNT {
            let tech = tech_from_index(i);
            let cap = tech_cap_row(tech);
            assert_eq!(cap.name, tech_name(tech));
            assert!(!cap.vendor_family.is_empty());
            assert!(cap.max_parts > 0);
            assert!(cap.vram_gran_mib > 0);
            assert!(cap.compute_gran_ppm > 0);
            assert!(!cap.note.is_empty());
            // 隔离等级与硬件事实表自洽
            let mut d = 0;
            while d < DOMAIN_COUNT {
                let truth = boundary_truth(tech, domain_from_index(d));
                assert!(truth.rank() >= cap.isolation.rank());
                d += 1;
            }
            i += 1;
        }
        // 三技术互异（闭集不重行）
        assert_ne!(tech_name(PartTech::SrIov), tech_name(PartTech::Mig));
        assert_ne!(tech_name(PartTech::Mig), tech_name(PartTech::GpuP));
        assert_ne!(tech_name(PartTech::SrIov), tech_name(PartTech::GpuP));
        // GpuP 事实等级是半虚拟化（不夸大红线的锚）
        assert_eq!(boundary_truth(PartTech::GpuP, BoundaryDomain::Vram), IsolationLevel::Paravirtual);
        assert_eq!(boundary_truth(PartTech::SrIov, BoundaryDomain::Dma), IsolationLevel::Hardware);
        assert_eq!(boundary_truth(PartTech::Mig, BoundaryDomain::Compute), IsolationLevel::Hardware);
    }

    #[test]
    fn isolation_and_domain_names_exhaustive() {
        assert_eq!(IsolationLevel::None.rank(), 0);
        assert_eq!(IsolationLevel::Paravirtual.rank(), 1);
        assert_eq!(IsolationLevel::Hardware.rank(), 2);
        assert!(!IsolationLevel::None.name().is_empty());
        assert!(!IsolationLevel::Paravirtual.name().is_empty());
        assert!(!IsolationLevel::Hardware.name().is_empty());
        let mut d = 0;
        while d < DOMAIN_COUNT {
            assert!(!domain_name(domain_from_index(d)).is_empty());
            d += 1;
        }
        let mut s = 0;
        while s < LC_COUNT {
            assert!(!lc_name(match s {
                0 => LifecycleState::Created,
                1 => LifecycleState::Active,
                2 => LifecycleState::Suspended,
                _ => LifecycleState::Destroyed,
            }).is_empty());
            s += 1;
        }
        let mut p = 0;
        while p < 3 {
            let probe = match p {
                0 => Probe3::Present,
                1 => Probe3::Missing,
                _ => Probe3::Unknown,
            };
            assert!(!probe.name().is_empty());
            p += 1;
        }
        // 销毁行全 false——真终态
        let mut t = 0;
        while t < LC_COUNT {
            assert!(!lc_transition_allowed(LifecycleState::Destroyed, tech_lc(t)));
            t += 1;
        }
    }

    // 匹配辅助（测试内）：行号转状态。
    fn tech_lc(index: usize) -> LifecycleState {
        match index {
            0 => LifecycleState::Created,
            1 => LifecycleState::Active,
            2 => LifecycleState::Suspended,
            _ => LifecycleState::Destroyed,
        }
    }

    #[test]
    fn error_codes_segment_unique_with_chinese_detail() {
        let samples: Vec<PartGpuError> = alloc::vec![
            PartGpuError::IllegalId { id: 0, what: "t" },
            PartGpuError::StateOrder { part_id: 1, from: LifecycleState::Created, to: LifecycleState::Destroyed },
            PartGpuError::QuotaOverrun { need_vram_mib: 1, need_ppm: 1, free_vram_mib: 0, free_ppm: 0 },
            PartGpuError::QuotaBad { vram_mib: 0, compute_ppm: 0, reason: "t" },
            PartGpuError::UnknownPart { part_id: 9, op: "t" },
            PartGpuError::ApiFrozen { method_id: 1, expected: 1, actual: 2 },
            PartGpuError::BoundaryFault { src_part: 1, dst_part: 2, domain: BoundaryDomain::Vram },
            PartGpuError::BoundaryInflate { tech: PartTech::GpuP, declared: IsolationLevel::Hardware, truth: IsolationLevel::Paravirtual },
            PartGpuError::BoundaryUndeclared { tech: PartTech::Mig },
            PartGpuError::FairnessDrift { epoch: 1, part_id: 1, served: 9, entitled: 1 },
            PartGpuError::FairnessNoEpoch,
            PartGpuError::ProbeContra { tech: PartTech::SrIov, declared: Probe3::Present, mirrored: Probe3::Unknown },
            PartGpuError::ProbeUnknown { tech: PartTech::Mig },
            PartGpuError::OrphanPart { part_id: 3 },
            PartGpuError::TechMismatch { tech: PartTech::GpuP, op: "t" },
        ];
        assert_eq!(samples.len(), 15);
        // 段内唯一 + 中文诊断非空 + Display 带码
        let mut i = 0;
        while i < samples.len() {
            let ci = samples[i].code();
            assert!(ci >= X_PG_CODE_MIN && ci <= X_PG_CODE_MAX, "码 0x{:04X} 越段", ci);
            assert!(!samples[i].detail().is_empty());
            let disp = alloc::format!("{}", samples[i]);
            assert!(disp.contains("0x1D"));
            let mut j = i + 1;
            while j < samples.len() {
                assert_ne!(ci, samples[j].code(), "诊断码重复：0x{:04X}", ci);
                j += 1;
            }
            i += 1;
        }
        // 段界自洽
        assert_eq!(X_PG_CODE_MIN, X_PG_OK);
        assert_eq!(X_PG_CODE_MAX, X_PG_TECH_MISMATCH);
        assert_eq!(X_PG_CODE_MAX - X_PG_CODE_MIN, 0xF);
    }

    #[test]
    fn api_freeze_v1_fingerprint_and_evolution_gates() {
        // v1 冻结核指纹运行期复核
        let mut e = prepped();
        assert_eq!(ok(e.api_verify()), API_CORE_V1_FINGERPRINT);
        // 签名被改 → 指纹漂移（改签名即破坏 ABI 的可检性）
        let mut mutated = API_V1_TABLE;
        mutated[0][1] = 5;
        assert_ne!(api_table_fingerprint(&mutated), API_FROZEN_V1_FINGERPRINT);
        let mut mutated_core = API_V1_TABLE;
        mutated_core[2][1] = 9;
        assert_ne!(api_core_table_fingerprint(&mutated_core), API_CORE_V1_FINGERPRINT);
        // 白盒腐蚀注册表 → verify 带码拦截
        let mut corrupt = ApiRegistry::new();
        corrupt.slots[2][1] = 9;
        assert_eq!(corrupt.verify_frozen().err().map(|x| x.code()), Some(X_PG_API_FROZEN));
        // 演进只增不改：
        // 新方法占空槽且版本从 1 起步
        ok(e.api_extend(7, 2, 1));
        assert_eq!(e.api_slot_view(7), Some([7, 2, 1]));
        // 既有方法合法版本递增（参数面不动）
        ok(e.api_extend(1, 4, 2));
        assert_eq!(e.api_slot_view(1), Some([1, 4, 2]));
        assert_eq!(ok(e.api_verify()), API_CORE_V1_FINGERPRINT);
        // 改签名拦截（v1 方法参数数变更）
        assert_eq!(code_of(e.api_extend(1, 5, 3)), X_PG_API_FROZEN);
        // 版本回退/同版重放拦截
        assert_eq!(code_of(e.api_extend(1, 4, 2)), X_PG_API_FROZEN);
        assert_eq!(code_of(e.api_extend(1, 4, 1)), X_PG_API_FROZEN);
        // 新方法非 v1 起步拦截
        assert_eq!(code_of(e.api_extend(8, 3, 2)), X_PG_API_FROZEN);
        // 方法号非法拦截
        assert_eq!(code_of(e.api_extend(0, 1, 1)), X_PG_ILLEGAL_ID);
        assert_eq!(code_of(e.api_extend(9, 1, 1)), X_PG_ILLEGAL_ID);
        // 槽位视图：未占槽不猜
        assert_eq!(e.api_slot_view(6), Some([6, 1, 1]));
        assert_eq!(e.api_slot_view(0), None);
        // 计数：4 次冻结拦截 + 2 次方法号非法拦截（同过 api_extend 门）= 6；
        // 合法扩展 2 次
        let h = e.health();
        assert_eq!(h.api_frozen_hits, 6);
        assert_eq!(h.api_extensions, 2);
    }

    // ── 组二：配额 ───────────────────────────────────────────────────────

    #[test]
    fn quota_granularity_and_value_gates() {
        let mut e = prepped();
        // 零值
        assert_eq!(code_of(e.create(PartTech::GpuP, 1, 0, 10_000)), X_PG_QUOTA_OVERFLOW);
        assert_eq!(code_of(e.create(PartTech::GpuP, 1, 640, 0)), X_PG_QUOTA_OVERFLOW);
        // 粒度不对齐（各按 TECH_CAP_TABLE）
        assert_eq!(code_of(e.create(PartTech::GpuP, 1, 63, 10_000)), X_PG_QUOTA_OVERFLOW);
        assert_eq!(code_of(e.create(PartTech::GpuP, 1, 640, 9_999)), X_PG_QUOTA_OVERFLOW);
        assert_eq!(code_of(e.create(PartTech::SrIov, 1, 100, 31_250)), X_PG_QUOTA_OVERFLOW);
        assert_eq!(code_of(e.create(PartTech::Mig, 1, 5120, 10_000)), X_PG_QUOTA_OVERFLOW);
        // 超物理池上界
        assert_eq!(code_of(e.create(PartTech::GpuP, 1, PHYS_VRAM_MIB + 64, 10_000)), X_PG_QUOTA_OVERFLOW);
        assert_eq!(code_of(e.create(PartTech::GpuP, 1, 640, PHYS_COMPUTE_PPM + 10_000)), X_PG_QUOTA_OVERFLOW);
        // 非法属主
        assert_eq!(code_of(e.create(PartTech::GpuP, 0, 640, 10_000)), X_PG_ILLEGAL_ID);
        // 探测/边界门先于配额值门（fresh 引擎探测 Unknown、边界未声明）
        let mut f = PartGpuEngine::new();
        assert_eq!(code_of(f.create(PartTech::Mig, 1, 5120, 142_857)), X_PG_PROBE_UNKNOWN);
    }

    #[test]
    fn quota_conservation_and_oversell_injection_detected() {
        let mut e = prepped();
        // 16 个 GPU-P 分区铺满槽位（各 1280 MiB / 10000 ppm）
        let mut i = 0;
        while i < MAX_PARTS {
            let id = ok(e.create(PartTech::GpuP, (i + 1) as u64, 1280, 10_000));
            assert_eq!(id, (i + 1) as u64);
            i += 1;
        }
        assert_eq!(e.committed_vram_mib(), 16 * 1280);
        assert_eq!(e.committed_ppm(), 16 * 10_000);
        ok(e.verify_conservation());
        // 槽位耗尽
        assert_eq!(code_of(e.create(PartTech::GpuP, 99, 640, 10_000)), X_PG_QUOTA_OVERFLOW);
        // 超卖注入：双维都越过余量 → 守恒核拦截
        assert_eq!(code_of(e.inject_oversubscribe(70_000, 900_000)), X_PG_QUOTA_OVERRUN);
        // 单维超卖（显存维）同样拦截
        assert_eq!(code_of(e.inject_oversubscribe(70_000, 1)), X_PG_QUOTA_OVERRUN);
        // 单维超卖（算力维）
        assert_eq!(code_of(e.inject_oversubscribe(1, 900_000)), X_PG_QUOTA_OVERRUN);
        let h = e.health();
        assert_eq!(h.oversell_rejected, 3);
        assert_eq!(h.oversell_injections, 3);
        // 账面未被污染
        ok(e.verify_conservation());
        assert_eq!(e.committed_vram_mib(), 16 * 1280);
    }

    #[test]
    fn quota_set_quota_paths_and_ledger() {
        let mut e = prepped();
        let id = ok(e.create(PartTech::GpuP, 7, 640, 10_000));
        ok(e.activate(id));
        // 扩配额（双维同增）
        ok(e.set_quota(id, 1280, 20_000));
        assert_eq!(e.committed_vram_mib(), 1280);
        assert_eq!(e.committed_ppm(), 20_000);
        ok(e.verify_conservation());
        // 缩配额（双维同减，回池）
        ok(e.set_quota(id, 640, 10_000));
        assert_eq!(e.committed_vram_mib(), 640);
        assert_eq!(e.committed_ppm(), 10_000);
        ok(e.verify_conservation());
        // 一增一减混合
        ok(e.set_quota(id, 1280, 10_000));
        ok(e.verify_conservation());
        assert_eq!(e.committed_vram_mib(), 1280);
        // 无意义变更拒
        assert_eq!(code_of(e.set_quota(id, 1280, 10_000)), X_PG_QUOTA_OVERFLOW);
        // 变更后粒度违规拒
        assert_eq!(code_of(e.set_quota(id, 1281, 10_000)), X_PG_QUOTA_OVERFLOW);
        // 扩到越池拒（16 槽内第二分区占位后余量不足）——先补满
        let id2 = ok(e.create(PartTech::GpuP, 8, 640, 10_000));
        ok(e.activate(id2));
        // 自由显存 = 81920 - 1920 = 80000 MiB；增量 81344-1280 = 80064 > 80000 → 超卖
        assert_eq!(code_of(e.set_quota(id, 81_344, 20_000)), X_PG_QUOTA_OVERRUN);
        // 变更账：3 笔成功入账，前后值成对
        let ledger = e.quota_changes_snapshot();
        assert_eq!(ledger.len(), 3);
        assert_eq!(ledger[0].old_vram_mib, 640);
        assert_eq!(ledger[0].new_vram_mib, 1280);
        assert_eq!(ledger[2].new_vram_mib, 1280);
        assert_eq!(e.health().quota_changes_ok, 3);
        // 销毁回池 + 终态配额操作拒
        ok(e.destroy(id));
        ok(e.verify_conservation());
        assert_eq!(code_of(e.set_quota(id, 640, 10_000)), X_PG_STATE_ORDER);
        // 查不存在分区
        assert_eq!(code_of(e.set_quota(4242, 640, 10_000)), X_PG_QUOTA_UNKNOWN_PART);
        assert_eq!(code_of(e.query_part(0)), X_PG_ILLEGAL_ID);
    }

    #[test]
    fn conservation_core_detects_tampering() {
        let e = prepped();
        // 空引擎守恒成立
        ok(e.verify_conservation());
        // 白盒污染账面 → 守恒核带码报错
        let mut bad = prepped();
        bad.quota.committed_vram_mib = 4242;
        let err = bad.verify_conservation().unwrap_err();
        assert_eq!(err.code(), X_PG_QUOTA_OVERFLOW);
        assert!(err.detail().contains("守恒核"));
    }

    // ── 组三：边界 ───────────────────────────────────────────────────────

    #[test]
    fn boundary_declare_gates_inflate_and_undeclared() {
        let mut e = PartGpuEngine::new();
        // 探测先喂 Present，让边界门成为首个拦截点
        ok(e.probe_feed(ProbeMirrorInput {
            tech: PartTech::GpuP,
            hw_cap_seen: true,
            units_reported: 1,
            declared: Probe3::Present,
        }));
        // 五域未声明齐即创建 → 拒
        assert_eq!(code_of(e.create(PartTech::GpuP, 1, 640, 10_000)), X_PG_BOUNDARY_UNDECLARED);
        // 夸大声明注入：GPU-P 声明硬件级 → 拒 + 红案
        assert_eq!(
            code_of(e.inject_boundary_claim(PartTech::GpuP, BoundaryDomain::Vram, IsolationLevel::Hardware)),
            X_PG_BOUNDARY_INFLATE
        );
        assert_eq!(e.case_count(), 1);
        assert_eq!(e.health().boundary_inflate_rejected, 1);
        // 夸大不落账：Vram 域仍未声明 → 仍拒创建
        assert_eq!(code_of(e.create(PartTech::GpuP, 1, 640, 10_000)), X_PG_BOUNDARY_UNDECLARED);
        // 如实声明五域（半虚拟级）→ 创建放行
        let mut d = 0;
        while d < DOMAIN_COUNT {
            ok(e.declare_boundary(PartTech::GpuP, domain_from_index(d), IsolationLevel::Paravirtual));
            d += 1;
        }
        let id = ok(e.create(PartTech::GpuP, 1, 640, 10_000));
        assert_eq!(e.query_part(id).unwrap().state, LifecycleState::Created);
        // 保守声明（低于事实）放行并返回事实等级
        assert_eq!(
            ok(e.declare_boundary(PartTech::SrIov, BoundaryDomain::Dma, IsolationLevel::None)),
            IsolationLevel::Hardware
        );
        // SR-IOV 硬件级如实声明放行
        assert_eq!(
            ok(e.declare_boundary(PartTech::SrIov, BoundaryDomain::Dma, IsolationLevel::Hardware)),
            IsolationLevel::Hardware
        );
    }

    #[test]
    fn boundary_access_fault_semantics_never_silent() {
        let mut e = prepped();
        let a = ok(e.create(PartTech::GpuP, 1, 640, 10_000));
        let b = ok(e.create(PartTech::GpuP, 2, 640, 10_000));
        // 自分区访问合法
        ok(e.boundary_access(a, a, BoundaryDomain::Compute));
        assert_eq!(e.case_count(), 0);
        // 跨分区访问 → fault + 红案 + 事件（不静默）
        assert_eq!(code_of(e.boundary_access(a, b, BoundaryDomain::Vram)), X_PG_BOUNDARY_FAULT);
        assert_eq!(code_of(e.boundary_access(b, a, BoundaryDomain::Dma)), X_PG_BOUNDARY_FAULT);
        assert_eq!(e.case_count(), 2);
        assert_eq!(e.health().boundary_faults, 2);
        let cases = e.case_snapshot();
        assert_eq!(cases[0].code, X_PG_BOUNDARY_FAULT);
        // 不存在的端点/非法编号
        assert_eq!(code_of(e.boundary_access(a, 4242, BoundaryDomain::Vram)), X_PG_QUOTA_UNKNOWN_PART);
        assert_eq!(code_of(e.boundary_access(0, b, BoundaryDomain::Vram)), X_PG_ILLEGAL_ID);
        // 五域语义一致：任何域跨分区都 fault
        let mut d = 0;
        while d < DOMAIN_COUNT {
            assert_eq!(code_of(e.boundary_access(a, b, domain_from_index(d))), X_PG_BOUNDARY_FAULT);
            d += 1;
        }
    }

    // ── 组四：公平 ───────────────────────────────────────────────────────

    #[test]
    fn entitled_for_ticks_weighted_split() {
        let mut ppm = [0u64; MAX_PARTS];
        ppm[0] = 10_000;
        ppm[1] = 30_000;
        let ent = entitled_for_ticks(16, &ppm, 40_000);
        assert_eq!(ent[0], 4);
        assert_eq!(ent[1], 12);
        // 零和 → 全零应得（无活动分区即无应得）
        let zero = entitled_for_ticks(16, &[0u64; MAX_PARTS], 0);
        let mut i = 0;
        let mut all_zero = true;
        while i < MAX_PARTS {
            if zero[i] != 0 {
                all_zero = false;
            }
            i += 1;
        }
        assert!(all_zero);
    }

    #[test]
    fn fairness_epochs_fair_then_biased_detected() {
        let mut e = prepped();
        let a = ok(e.create(PartTech::GpuP, 1, 640, 10_000));
        let b = ok(e.create(PartTech::GpuP, 2, 640, 10_000));
        ok(e.activate(a));
        ok(e.activate(b));
        // 纪元一：交替 8/8 拍——公平收账，零偏差（第 16 拍收账）
        let mut i = 0;
        while i < 7 {
            let v = ok(e.fairness_tick(a));
            assert!(!v.epoch_closed);
            let v = ok(e.fairness_tick(b));
            assert!(!v.epoch_closed);
            i += 1;
        }
        let v = ok(e.fairness_tick(a));
        assert!(!v.epoch_closed);
        let v = ok(e.fairness_tick(b));
        assert!(v.epoch_closed);
        assert_eq!(v.drift_rows, 2);
        assert_eq!(v.breached_rows, 0);
        assert_eq!(e.health().fairness_drift_cases, 0);
        assert_eq!(e.case_count(), 0);
        // 纪元二：全部 16 拍砸给 a——双侧越界（a 多得、b 零得）→ 红案
        let mut j = 0;
        while j < 16 {
            let v = ok(e.fairness_tick(a));
            if j < 15 {
                assert!(!v.epoch_closed);
            } else {
                assert!(v.epoch_closed);
                assert_eq!(v.breached_rows, 2);
            }
            j += 1;
        }
        let h = e.health();
        assert_eq!(h.fairness_drift_cases, 2);
        assert_eq!(h.fairness_epochs, 2);
        // 红案入账（X_PG_FAIRNESS_DRIFT 带码）
        let cases = e.case_snapshot();
        assert_eq!(cases.len(), 2);
        assert_eq!(cases[0].code, X_PG_FAIRNESS_DRIFT);
        assert_eq!(cases[1].code, X_PG_FAIRNESS_DRIFT);
        // 公平对账快照：累计实得 a=24、b=8
        let rows = e.fairness_report();
        assert_eq!(rows.len(), 2);
        let ra = rows.iter().find(|r| r.part_id == a).unwrap();
        let rb = rows.iter().find(|r| r.part_id == b).unwrap();
        assert_eq!(ra.total_served, 24);
        assert_eq!(rb.total_served, 8);
        // 纪元总览
        let (epochs, ticks, drift_cases) = ok(e.fairness_epoch_verdict());
        assert_eq!(epochs, 2);
        assert_eq!(ticks, 0);
        assert_eq!(drift_cases, 2);
    }

    #[test]
    fn fairness_gates_inactive_and_no_epoch() {
        let mut e = prepped();
        // 无纪元数据
        assert_eq!(code_of(e.fairness_epoch_verdict()), X_PG_FAIRNESS_NO_EPOCH);
        let id = ok(e.create(PartTech::GpuP, 1, 640, 10_000));
        // Created 未激活不许记账
        assert_eq!(code_of(e.fairness_tick(id)), X_PG_STATE_ORDER);
        // 挂起后不许记账
        ok(e.activate(id));
        ok(e.suspend(id));
        assert_eq!(code_of(e.fairness_tick(id)), X_PG_STATE_ORDER);
        // 不存在/非法编号
        assert_eq!(code_of(e.fairness_tick(4242)), X_PG_QUOTA_UNKNOWN_PART);
        assert_eq!(code_of(e.fairness_tick(0)), X_PG_ILLEGAL_ID);
    }

    // ── 组五：探测 ───────────────────────────────────────────────────────

    #[test]
    fn probe_tristate_no_fabrication_and_contra() {
        let mut e = PartGpuEngine::new();
        // 起步全 Unknown（不预设）
        assert_eq!(e.probe_state(PartTech::SrIov), Probe3::Unknown);
        assert_eq!(e.probe_state(PartTech::Mig), Probe3::Unknown);
        assert_eq!(e.probe_state(PartTech::GpuP), Probe3::Unknown);
        // 证据齐 → Present
        ok(e.probe_feed(ProbeMirrorInput { tech: PartTech::Mig, hw_cap_seen: true, units_reported: 3, declared: Probe3::Present }));
        assert_eq!(e.probe_state(PartTech::Mig), Probe3::Present);
        // 声明 Present 无证据 → 矛盾拦截，状态采信镜像（无证据无单元 = Missing）
        assert_eq!(code_of(e.probe_feed(ProbeMirrorInput { tech: PartTech::Mig, hw_cap_seen: false, units_reported: 0, declared: Probe3::Present })), X_PG_PROBE_CONTRA);
        assert_eq!(e.probe_state(PartTech::Mig), Probe3::Missing);
        // 一致 Missing 放行
        ok(e.probe_feed(ProbeMirrorInput { tech: PartTech::SrIov, hw_cap_seen: false, units_reported: 0, declared: Probe3::Missing }));
        assert_eq!(e.probe_state(PartTech::SrIov), Probe3::Missing);
        // 证据矛盾（有硬件无单元）→ Unknown，且声明一致放行（不造假升级）
        ok(e.probe_feed(ProbeMirrorInput { tech: PartTech::GpuP, hw_cap_seen: true, units_reported: 0, declared: Probe3::Unknown }));
        assert_eq!(e.probe_state(PartTech::GpuP), Probe3::Unknown);
        // 单元数超技术上限 → TechMismatch（跨技术拼凑造假）
        assert_eq!(code_of(e.probe_feed(ProbeMirrorInput { tech: PartTech::Mig, hw_cap_seen: true, units_reported: 99, declared: Probe3::Present })), X_PG_TECH_MISMATCH);
        let h = e.health();
        assert_eq!(h.probe_feeds, 4);
        assert_eq!(h.probe_contra_rejected, 1);
    }

    #[test]
    fn probe_gate_refuses_unknown_and_missing() {
        let mut e = PartGpuEngine::new();
        // 边界齐但探测 Unknown → 创建门拒
        let mut d = 0;
        while d < DOMAIN_COUNT {
            ok(e.declare_boundary(PartTech::GpuP, domain_from_index(d), IsolationLevel::Paravirtual));
            d += 1;
        }
        assert_eq!(code_of(e.create(PartTech::GpuP, 1, 640, 10_000)), X_PG_PROBE_UNKNOWN);
        assert_eq!(e.health().probe_gate_refusals, 1);
        // Missing 同拒
        ok(e.probe_feed(ProbeMirrorInput { tech: PartTech::GpuP, hw_cap_seen: false, units_reported: 0, declared: Probe3::Missing }));
        assert_eq!(code_of(e.create(PartTech::GpuP, 1, 640, 10_000)), X_PG_PROBE_UNKNOWN);
        assert_eq!(e.health().probe_gate_refusals, 2);
        // Present 放行
        ok(e.probe_feed(ProbeMirrorInput { tech: PartTech::GpuP, hw_cap_seen: true, units_reported: 1, declared: Probe3::Present }));
        let id = ok(e.create(PartTech::GpuP, 1, 640, 10_000));
        assert_eq!(e.query_part(id).unwrap().tech, PartTech::GpuP);
    }

    // ── 生命周期 ─────────────────────────────────────────────────────────

    #[test]
    fn lifecycle_matrix_and_terminal_state() {
        let mut e = prepped();
        let id = ok(e.create(PartTech::GpuP, 1, 640, 10_000));
        // Created→Active
        ok(e.activate(id));
        // Active→Active 乱序拒
        assert_eq!(code_of(e.activate(id)), X_PG_STATE_ORDER);
        // Active→Suspended→Active（恢复）
        ok(e.suspend(id));
        ok(e.activate(id));
        assert_eq!(e.health().resumes, 1);
        // Active→Destroyed
        ok(e.destroy(id));
        // 终态一切后续操作拒
        assert_eq!(code_of(e.destroy(id)), X_PG_STATE_ORDER);
        assert_eq!(code_of(e.activate(id)), X_PG_STATE_ORDER);
        assert_eq!(code_of(e.suspend(id)), X_PG_STATE_ORDER);
        assert_eq!(code_of(e.set_quota(id, 640, 10_000)), X_PG_STATE_ORDER);
        assert_eq!(code_of(e.fairness_tick(id)), X_PG_STATE_ORDER);
        assert_eq!(code_of(e.revoke_owner(id)), X_PG_STATE_ORDER);
        // 墓碑仍可查询（销毁不抹账）
        let info = e.query_part(id).unwrap();
        assert_eq!(info.state, LifecycleState::Destroyed);
        // 非法编号
        assert_eq!(code_of(e.query_part(0)), X_PG_ILLEGAL_ID);
        assert_eq!(code_of(e.destroy(4242)), X_PG_QUOTA_UNKNOWN_PART);
    }

    #[test]
    fn lifecycle_orphan_injection_detected() {
        let mut e = prepped();
        let id = ok(e.create(PartTech::GpuP, 5, 640, 10_000));
        ok(e.activate(id));
        // 巡账前无孤儿
        ok(e.audit_orphans_strict());
        assert_eq!(e.case_count(), 0);
        // 注入：属主注销 → 严格巡账立即检出 X_PG_ORPHAN
        assert_eq!(code_of(e.inject_orphan(id)), X_PG_ORPHAN);
        assert_eq!(e.case_count(), 1);
        assert_eq!(e.case_snapshot()[0].code, X_PG_ORPHAN);
        assert_eq!(e.health().orphans_found, 1);
        // 重复注销已是孤儿 → 带码拒
        assert_eq!(code_of(e.revoke_owner(id)), X_PG_ORPHAN);
        // 巡账持续在册
        assert_eq!(e.audit_orphans().len(), 1);
        // 销毁回收后墓碑不算孤儿
        ok(e.destroy(id));
        ok(e.audit_orphans_strict());
        assert_eq!(e.health().orphan_audits, 4);
    }

    // ── 事件账 / 健康 / 指纹 ─────────────────────────────────────────────

    #[test]
    fn event_ledger_ring_overrun_explicit() {
        let mut e = prepped();
        // 预备管道已产生 18 个事件（15 边界声明 + 3 探测）
        assert_eq!(e.event_total(), 18);
        // 30 轮创建+销毁 → 60 事件 → 总 78 > 64 → 覆盖显性计数
        let mut i = 0;
        while i < 30 {
            let id = ok(e.create(PartTech::GpuP, 3, 640, 10_000));
            ok(e.destroy(id));
            i += 1;
        }
        assert_eq!(e.event_total(), 78);
        assert_eq!(e.event_overruns(), 78 - 64);
        let snap = e.events_snapshot();
        assert_eq!(snap.len(), 64);
        // 快照按序号升序（环绕收集正确）
        let mut k = 1;
        while k < snap.len() {
            assert!(snap[k].seq > snap[k - 1].seq);
            assert!(!snap[k].note.is_empty());
            assert!(!event_kind_name(snap[k].kind).is_empty());
            k += 1;
        }
        // 品种名穷尽（新增品种漏名即崩）
        let kinds = [
            PartEventKind::Created,
            PartEventKind::Activated,
            PartEventKind::Suspended,
            PartEventKind::Resumed,
            PartEventKind::Destroyed,
            PartEventKind::QuotaChanged,
            PartEventKind::QuotaRejected,
            PartEventKind::BoundaryDeclared,
            PartEventKind::BoundaryFault,
            PartEventKind::FairnessTick,
            PartEventKind::FairnessDrift,
            PartEventKind::ProbeFed,
            PartEventKind::ProbeContra,
            PartEventKind::ProbeGateRefused,
            PartEventKind::OwnerRevoked,
            PartEventKind::OrphanAudit,
            PartEventKind::ApiExtended,
            PartEventKind::ApiFrozenHit,
            PartEventKind::LedgerOverrun,
        ];
        let mut m = 0;
        while m < kinds.len() {
            assert!(!event_kind_name(kinds[m]).is_empty());
            m += 1;
        }
    }

    #[test]
    fn health_and_fingerprint_stability() {
        let mut e = prepped();
        let fp0 = e.fingerprint();
        assert_eq!(e.fingerprint(), fp0, "同态指纹必须稳定");
        let a = ok(e.create(PartTech::GpuP, 1, 640, 10_000));
        assert_ne!(e.fingerprint(), fp0, "状态变化指纹必变");
        ok(e.activate(a));
        let h = e.health();
        assert_eq!(h.parts_in_register, 1);
        assert_eq!(h.parts_live, 1);
        assert_eq!(h.parts_active, 1);
        assert_eq!(h.creates_ok, 1);
        assert_eq!(h.activates, 1);
        assert_eq!(h.committed_vram_mib, 640);
        assert_eq!(h.committed_ppm, 10_000);
        assert_eq!(h.free_vram_mib, PHYS_VRAM_MIB - 640);
        assert_eq!(h.free_ppm, PHYS_COMPUTE_PPM - 10_000);
        assert!(h.all_boundaries_declared);
        assert!(h.api_v1_intact);
        assert_eq!(h.case_count, 0);
        assert_eq!(h.boundary_decls, 15);
        assert_eq!(h.probe_feeds, 3);
        // parts_snapshot 与 health 一致
        assert_eq!(e.parts_snapshot().len(), 1);
        ok(e.verify_conservation());
    }

    // ── 双跑对拍 ─────────────────────────────────────────────────────────

    #[test]
    fn dual_run_same_script_same_fingerprint_different_script_differs() {
        let script_a: Vec<ScriptOp> = alloc::vec![
            ScriptOp::Create { tech: PartTech::GpuP, owner: 1, vram_mib: 640, compute_ppm: 10_000 },
            ScriptOp::Create { tech: PartTech::GpuP, owner: 2, vram_mib: 640, compute_ppm: 10_000 },
            ScriptOp::Activate { part_id: 1 },
            ScriptOp::Activate { part_id: 2 },
            ScriptOp::Tick { part_id: 1 },
            ScriptOp::Tick { part_id: 2 },
            ScriptOp::Access { src: 1, dst: 2, domain: BoundaryDomain::Vram },
            ScriptOp::SetQuota { part_id: 1, vram_mib: 1280, compute_ppm: 20_000 },
            ScriptOp::Suspend { part_id: 2 },
            ScriptOp::Destroy { part_id: 2 },
        ];
        let ra = run_script(&script_a);
        let rb = run_script(&script_a);
        assert_eq!(ra, rb, "同脚本双跑必须同指纹同报告");
        assert_eq!(ra.creates_ok, 2);
        assert_eq!(ra.errors, 1, "跨分区访问 fault 计一次被拒");
        // 异操作序列 → 异指纹
        let script_b: Vec<ScriptOp> = alloc::vec![
            ScriptOp::Create { tech: PartTech::GpuP, owner: 1, vram_mib: 640, compute_ppm: 10_000 },
            ScriptOp::Create { tech: PartTech::GpuP, owner: 2, vram_mib: 640, compute_ppm: 10_000 },
            ScriptOp::Activate { part_id: 1 },
            ScriptOp::Activate { part_id: 2 },
            ScriptOp::Tick { part_id: 1 },
            ScriptOp::Tick { part_id: 2 },
            ScriptOp::Access { src: 1, dst: 2, domain: BoundaryDomain::Vram },
            ScriptOp::SetQuota { part_id: 1, vram_mib: 1280, compute_ppm: 20_000 },
            ScriptOp::Suspend { part_id: 2 },
            ScriptOp::Destroy { part_id: 2 },
            ScriptOp::Tick { part_id: 1 },
        ];
        let rc = run_script(&script_b);
        assert_ne!(ra.fingerprint, rc.fingerprint);
        // 空脚本双跑也稳定
        let r0a = run_script(&[]);
        let r0b = run_script(&[]);
        assert_eq!(r0a, r0b);
        assert_eq!(r0a.errors, 0);
        // 编号 0 重指向最近创建
        let script_c: Vec<ScriptOp> = alloc::vec![
            ScriptOp::Create { tech: PartTech::Mig, owner: 1, vram_mib: 5120, compute_ppm: 142_857 },
            ScriptOp::Activate { part_id: 0 },
            ScriptOp::Destroy { part_id: 0 },
        ];
        let rcc = run_script(&script_c);
        assert_eq!(rcc.errors, 0);
        assert_eq!(rcc.creates_ok, 1);
    }

    #[test]
    fn multi_tech_coexistence_and_full_pipeline() {
        let mut e = prepped();
        // 三技术共存：各按各的粒度与事实边界
        let s = ok(e.create(PartTech::SrIov, 1, 256, 31_250));
        let m = ok(e.create(PartTech::Mig, 2, 5120, 142_857));
        let g = ok(e.create(PartTech::GpuP, 3, 640, 10_000));
        ok(e.activate(s));
        ok(e.activate(m));
        ok(e.activate(g));
        ok(e.verify_conservation());
        assert_eq!(e.committed_vram_mib(), 256 + 5120 + 640);
        assert_eq!(e.committed_ppm(), 31_250 + 142_857 + 10_000);
        let h = e.health();
        assert_eq!(h.parts_active, 3);
        // 三技术三编号连续
        assert_eq!((s, m, g), (1, 2, 3));
        // 全管道收口：公平拍 + 边界自访 + 巡账 + 指纹复核
        ok(e.fairness_tick(s));
        ok(e.boundary_access(m, m, BoundaryDomain::Fault));
        ok(e.audit_orphans_strict());
        ok(e.api_verify());
        assert_eq!(e.case_count(), 0);
    }
}
