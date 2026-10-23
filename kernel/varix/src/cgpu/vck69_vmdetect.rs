//! CGPU-F1765 · 虚拟化环境检测（K 域 · 虚拟化组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1765`
//!
//! **判据（锚点原文）**：识别表、平台矩阵、WSL 登记、容器登记、差异、五组、判据。
//!
//! **职责定位（锚点原文摘录）**：
//! 环境检测：检测器（hypervisor 识别（Hyper-V/KVM/ESXi/QEMU/WSL——识别表；
//! 平台矩阵（Windows guest/Linux guest×hypervisor——平台矩阵——矩阵版本化；
//! WSL 专项（WSLg GPU（/dev/dxg——WSL 路径——路径实现登记；容器（容器 GPU
//! （nvidia-container/D3D KMT——容器路径——路径登记；版本差异（hypervisor
//! 版本差异表——差异复用家族；测试（识别表/矩阵/WSL/容器/差异五组）。
//!
//! ─────────────────────────────────────────────────────────────────────────────
//! 设计总则（本模块五条铁律，全部服务于锚点判据）：
//!
//! 1. **识别表**（判据"识别表"）：hypervisor 厂商签名以 CPUID leaf
//!    0x40000000 的 EBX/ECX/EDX 12 字节为唯一主键，const 表写死，来源逐条
//!    注明（各 hypervisor 公开文档）。识别是**三态结论**不是布尔：
//!    Bare（无签名——裸金属显性结论，是结论不是失败）/ Virtualized（命中
//!    识别表）/ Indeterminate（有签名但未入册——无法检测，与 Bare 严格
//!    区分）。多签名同 match 歧义按置信序裁决并全量留痕（仲裁账）。
//! 2. **平台矩阵**（判据"平台矩阵/矩阵版本化"）：Windows/Linux guest ×
//!    五家族 hypervisor 支持度三态 const 矩阵 v1。演进纪律：只增不改——
//!    改格必须升版本号。越界组合显性 Unknown 不猜。
//! 3. **WSL 登记**（判据"WSL 登记"）：/dev/dxg、/dev/nvidia*、WSLg socket
//!    路径 const 登记表；探测结果三态 Present/Missing/Unknown **不造假
//!    补齐**——没探测过的路径永远 Unknown。WSLg GPU 画像门槛：WSL 签名
//!    命中 ∧ /dev/dxg Present，两条件缺一门槛不开。
//! 4. **容器登记**（判据"容器登记"）：nvidia-container 与 D3D KMT 两族
//!    路径登记表 + 探测三态 + 与 WSLg 的互斥性校验（同时声称两族容器 GPU
//!    即矛盾入账——矛盾不静默、不裁决一边，画像整体降级）。
//! 5. **差异表**（判据"差异"）：每家族版本区间→能力差异条目；登记时区间
//!    重叠/断档显性拒绝；同家族次要版本归并到主版本条目（差异复用家族）。
//!
//! **检测器编排**：五步单向流水（签名识别→平台矩阵查→WSL 专项→容器专项→
//! 版本差异画像）。乱序调用拒绝带码；每步结果入 `DetectionLedger` 定长账；
//! 最终 `EnvironmentProfile` 快照含每步证据 + 置信三态 + FNV-1a 64 位指纹
//! （同输入同指纹，确定性可复现）。
//!
//! **注入测试面**：检测器只吃抽象 `CpuidSource` trait——真实源与注入源同一
//! 管道，测试五组（识别表/矩阵/WSL/容器/差异）全部走注入 CPUID，无旁路。
//!
//! **体检与隐私红线**：`health()` 出结构化报告（各注册表水位/矛盾计数/
//! Unknown 计数/矩阵版本）。环境指纹**本地闭环不上传**——指纹只用于本机
//! 日志对账与回归比对，任何遥测面（vck17/vck25/vck26）不得携带本模块指纹
//! 出进程；这是隐私红线不是建议。
//!
//! 诊断码段 0x12B0~0x12BF（开工前已 grep 确认 cgpu 域内空闲；并行模块占
//! 0x12A0 段，互不相交）。
//! no_std + alloc；内核路径零 unwrap 零 expect 零 panic 面零 IO 零墙钟
//! 零浮点；所有可失败操作返回 `Result<T, VmDetectError>`，错误 detail 为
//! 中文可操作诊断；溢出处显性选择 saturating（计数只涨不回绕）。

use core::fmt;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ═════════════════════════════════════════════════════════════════════════════
// §0 · 诊断码段（0x12B0~0x12BF，独占十六码；开工前 grep 确认空闲）
// ═════════════════════════════════════════════════════════════════════════════

/// 成功基线（无错误）。
pub const X_VM_OK: u32 = 0x12B0;
/// hypervisor 签名未在识别表中（建议收集 CPUID 0x40000000 返回值入册）。
pub const X_VM_ID_TABLE_MISS: u32 = 0x12B1;
/// 签名已知但家族未入册（如 VirtualBox/Xen——识别表只收五家族，走演进流程）。
pub const X_VM_ID_FOREIGN: u32 = 0x12B2;
/// 平台矩阵版本不匹配（建议走矩阵演进流程只增不改）。
pub const X_VM_MATRIX_VERSION_MISMATCH: u32 = 0x12B3;
/// 越界 guest/hyp 组合，显性 Unknown 不猜。
pub const X_VM_MATRIX_CELL_UNKNOWN: u32 = 0x12B4;
/// WSL 路径登记冲突（重复注册或路径不在登记表）。
pub const X_VM_WSL_PATH_CONFLICT: u32 = 0x12B5;
/// WSLg GPU 画像门槛未满足（WSL 签名或 /dev/dxg 缺一）。
pub const X_VM_WSLG_GATE_UNMET: u32 = 0x12B6;
/// 容器路径登记冲突（重复注册或路径不在登记表）。
pub const X_VM_CONTAINER_PATH_CONFLICT: u32 = 0x12B7;
/// WSLg 与 nvidia-container 同时声称（互斥矛盾入账，画像降级）。
pub const X_VM_WSL_CONTAINER_MUTEX: u32 = 0x12B8;
/// 版本差异区间重叠（登记被拒，建议核对区间端点）。
pub const X_VM_DIFF_RANGE_OVERLAP: u32 = 0x12B9;
/// 版本差异区间断档（连续性审计不通过，建议补齐覆盖）。
pub const X_VM_DIFF_RANGE_GAP: u32 = 0x12BA;
/// 未知 hypervisor 家族无差异条目。
pub const X_VM_DIFF_UNKNOWN_FAMILY: u32 = 0x12BB;
/// 流水步骤乱序调用（五步单向，拒绝回退/跳步）。
pub const X_VM_STEP_OUT_OF_ORDER: u32 = 0x12BC;
/// 定长账本已满（事件被丢弃计数，不阻塞主流程）。
pub const X_VM_LEDGER_FULL: u32 = 0x12BD;
/// 五步未完成即请求画像快照。
pub const X_VM_PROFILE_NOT_READY: u32 = 0x12BE;
/// 空输入或非法参数（空签名/空路径/区间倒置/未知路径）。
pub const X_VM_BAD_INPUT: u32 = 0x12BF;

// ═════════════════════════════════════════════════════════════════════════════
// §1 · 模块常量（写死并注明来源）
// ═════════════════════════════════════════════════════════════════════════════

/// 模块版本。
pub const VCK69_VMDETECT_VERSION: &str = "vck69-vmdetect-v1";

/// 平台矩阵当前版本（v1；演进纪律：只增不改，改格必须升版本）。
pub const MATRIX_VERSION: u32 = 1;

/// 检测账本容量（定长账；满了丢弃计数，不阻塞主流程）。
pub const LEDGER_CAP: usize = 64;

/// 版本号"无穷"哨兵：差异表区间上界开区间端点（hi_exclusive）。
pub const VER_MAX: u32 = 0xFFFF_FFFF;

/// WSL2 专项提示魔数：约定 CPUID 0x40000001.EAX 携带 "WSL2"（小端打包
/// 0x324C5357）。真实管道由平台层在 WSL2 内核里注入该值；注入测试同源。
/// 来源：WSL2 dxgkrnl 探测约定（内部平台约定，非微软公开 CPUID 承诺）。
pub const WSL2_HINT_MAGIC: u32 = 0x324C_5357; // "WSL2" little-endian

/// CPUID hypervisor 签名叶（来源：Intel SDM Vol.3 / 各 hypervisor 文档）。
pub const CPUID_HYPERVISOR_SIG_LEAF: u32 = 0x4000_0000;
/// CPUID hypervisor 附加信息叶（EAX 作为家族内裁决提示）。
pub const CPUID_HYPERVISOR_INFO_LEAF: u32 = 0x4000_0001;

/// 置信分界：>= HIGH_BAND 记 High。
pub const CONF_HIGH_BAND: u8 = 80;
/// 置信分界：>= MEDIUM_BAND 记 Medium，否则 Low。
pub const CONF_MEDIUM_BAND: u8 = 50;

/// 环境画像指纹面 FNV-1a 64 位偏移基（FNV-1a 公开常量）。
pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// 环境画像指纹面 FNV-1a 64 位素数（FNV-1a 公开常量）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// ═════════════════════════════════════════════════════════════════════════════
// §2 · FNV-1a 64 位（域内同族口径：同输入同指纹）
// ═════════════════════════════════════════════════════════════════════════════

/// FNV-1a 单字节推进。
pub const fn fnv1a_byte(acc: u64, b: u8) -> u64 {
    (acc ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 字节串推进。
pub const fn fnv1a_bytes(acc: u64, bytes: &[u8]) -> u64 {
    let mut acc = acc;
    let mut i = 0;
    while i < bytes.len() {
        acc = fnv1a_byte(acc, bytes[i]);
        i += 1;
    }
    acc
}

/// FNV-1a 以小端字节序折叠 u32（确定性编码，跨端一致）。
pub const fn fnv1a_u32(acc: u64, v: u32) -> u64 {
    fnv1a_bytes(acc, &v.to_le_bytes())
}

/// FNV-1a 以小端字节序折叠 u64。
pub const fn fnv1a_u64(acc: u64, v: u64) -> u64 {
    fnv1a_bytes(acc, &v.to_le_bytes())
}

// ═════════════════════════════════════════════════════════════════════════════
// §3 · 错误类型（枚举 + 中文可操作诊断）
// ═════════════════════════════════════════════════════════════════════════════

/// 虚拟化环境检测错误。内核路径唯一失败面；`detail()` 给中文可操作诊断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmDetectError {
    /// 签名未在识别表中。
    IdTableMiss,
    /// 签名已知但家族未入册。
    IdForeign,
    /// 平台矩阵版本不匹配。
    MatrixVersionMismatch,
    /// 越界矩阵组合。
    MatrixCellUnknown,
    /// WSL 路径登记冲突。
    WslPathConflict,
    /// WSLg 门槛未满足。
    WslgGateUnmet,
    /// 容器路径登记冲突。
    ContainerPathConflict,
    /// WSLg 与容器 GPU 声称互斥矛盾。
    WslContainerMutex,
    /// 差异区间重叠。
    DiffRangeOverlap,
    /// 差异区间断档。
    DiffRangeGap,
    /// 未知家族无差异条目。
    DiffUnknownFamily,
    /// 流水乱序。
    StepOutOfOrder,
    /// 账本满。
    LedgerFull,
    /// 画像未就绪。
    ProfileNotReady,
    /// 空输入/非法参数。
    BadInput,
}

impl VmDetectError {
    /// 对应诊断码（0x12B0 段）。
    pub fn code(&self) -> u32 {
        match self {
            VmDetectError::IdTableMiss => X_VM_ID_TABLE_MISS,
            VmDetectError::IdForeign => X_VM_ID_FOREIGN,
            VmDetectError::MatrixVersionMismatch => X_VM_MATRIX_VERSION_MISMATCH,
            VmDetectError::MatrixCellUnknown => X_VM_MATRIX_CELL_UNKNOWN,
            VmDetectError::WslPathConflict => X_VM_WSL_PATH_CONFLICT,
            VmDetectError::WslgGateUnmet => X_VM_WSLG_GATE_UNMET,
            VmDetectError::ContainerPathConflict => X_VM_CONTAINER_PATH_CONFLICT,
            VmDetectError::WslContainerMutex => X_VM_WSL_CONTAINER_MUTEX,
            VmDetectError::DiffRangeOverlap => X_VM_DIFF_RANGE_OVERLAP,
            VmDetectError::DiffRangeGap => X_VM_DIFF_RANGE_GAP,
            VmDetectError::DiffUnknownFamily => X_VM_DIFF_UNKNOWN_FAMILY,
            VmDetectError::StepOutOfOrder => X_VM_STEP_OUT_OF_ORDER,
            VmDetectError::LedgerFull => X_VM_LEDGER_FULL,
            VmDetectError::ProfileNotReady => X_VM_PROFILE_NOT_READY,
            VmDetectError::BadInput => X_VM_BAD_INPUT,
        }
    }

    /// 中文可操作诊断（面向值班者，不是面向编译器）。
    pub fn detail(&self) -> &'static str {
        match self {
            VmDetectError::IdTableMiss => {
                "hypervisor 签名未在识别表中，建议收集 CPUID 0x40000000 返回值入册"
            }
            VmDetectError::IdForeign => {
                "签名已知但家族未入册（如 VirtualBox/Xen/Parallels），建议走识别表演进流程只增不改"
            }
            VmDetectError::MatrixVersionMismatch => {
                "平台矩阵版本不匹配，建议走矩阵演进流程只增不改并升级矩阵版本号"
            }
            VmDetectError::MatrixCellUnknown => {
                "越界 guest/hyp 组合返回显性 Unknown，建议补登记矩阵格后再查"
            }
            VmDetectError::WslPathConflict => {
                "WSL 路径登记冲突：路径不在 const 登记表或重复注册，建议先查表再登记"
            }
            VmDetectError::WslgGateUnmet => {
                "WSLg GPU 门槛未满足：需 WSL 签名命中且 /dev/dxg 探测 Present，缺一不开"
            }
            VmDetectError::ContainerPathConflict => {
                "容器路径登记冲突：路径不在 const 登记表或重复注册，建议先查表再登记"
            }
            VmDetectError::WslContainerMutex => {
                "WSLg 与 nvidia-container 同时声称容器 GPU，互斥矛盾已入账，建议核对探测来源后重跑"
            }
            VmDetectError::DiffRangeOverlap => {
                "版本差异区间与既有区间重叠，建议核对区间端点后重新登记（区间半开 [lo,hi)）"
            }
            VmDetectError::DiffRangeGap => {
                "版本差异区间存在断档，建议补齐 0 到 MAX 的全量覆盖后复审"
            }
            VmDetectError::DiffUnknownFamily => {
                "未知 hypervisor 家族无差异条目，建议先入册识别表再建差异区间"
            }
            VmDetectError::StepOutOfOrder => {
                "五步流水乱序调用：签名识别→矩阵→WSL→容器→差异 单向推进，禁止回退跳步"
            }
            VmDetectError::LedgerFull => {
                "定长账本已满：本次事件被丢弃并计数，建议下调账目粒度或扩容 LEDGER_CAP"
            }
            VmDetectError::ProfileNotReady => {
                "五步未完成即请求画像快照，建议按流水顺序走完五步后取 EnvironmentProfile"
            }
            VmDetectError::BadInput => {
                "空输入或非法参数：检查空签名/空路径/区间倒置（要求 lo<hi）/未知路径名"
            }
        }
    }
}

impl fmt::Display for VmDetectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VMDETECT-0x{:04X}: {}", self.code(), self.detail())
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §4 · hypervisor 识别（判据"识别表"）
// ═════════════════════════════════════════════════════════════════════════════

/// 识别表在册的五家族 + Unknown（签名已知但家族未入册时的落点）。
///
/// 枚举闭集即判据闭集：HyperV/Kvm/Esxi/Qemu/Wsl。Unknown 不是"检测失败"，
/// 而是"签名在识别表里但映射到未入册家族"的显性落点（如 VirtualBox 行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HypervisorId {
    /// Microsoft Hyper-V（签名 "Microsoft Hv"；WSL2 同签名，靠 hint 裁决）。
    HyperV,
    /// KVM（签名 "KVMKVMKVM"，9 字节 + 3 NUL 补齐）。
    Kvm,
    /// VMware ESXi（签名 "VMwareVMware"；与 Workstation 共签名，置信降档留痕）。
    Esxi,
    /// QEMU 纯软件模拟（TCG 加速时签名 "TCGTCGTCGTCG"）。
    Qemu,
    /// WSL2 专用态（"Microsoft Hv" 签名 + WSL2 hint 魔数——比裸 Hyper-V 高置信）。
    Wsl,
    /// 签名已知但家族未入册（VirtualBox/Xen/Parallels/ACRN/bhyve 等表内行）。
    Unknown,
}

impl HypervisorId {
    /// 矩阵列序（与 MATRIX_V1 行序一致）；Unknown 无矩阵列。
    pub fn matrix_index(self) -> Option<usize> {
        match self {
            HypervisorId::HyperV => Some(0),
            HypervisorId::Kvm => Some(1),
            HypervisorId::Esxi => Some(2),
            HypervisorId::Qemu => Some(3),
            HypervisorId::Wsl => Some(4),
            HypervisorId::Unknown => None,
        }
    }

    /// 家族名（诊断/账目面用；与识别表 note 无关）。
    pub fn family_name(self) -> &'static str {
        match self {
            HypervisorId::HyperV => "Hyper-V",
            HypervisorId::Kvm => "KVM",
            HypervisorId::Esxi => "VMware ESXi",
            HypervisorId::Qemu => "QEMU(TCG)",
            HypervisorId::Wsl => "WSL2",
            HypervisorId::Unknown => "未入册家族",
        }
    }

    /// 差异表家族键（仅入册五家族有键；Unknown → None）。
    pub fn diff_family(self) -> Option<DiffFamily> {
        match self {
            HypervisorId::HyperV => Some(DiffFamily::HyperV),
            HypervisorId::Kvm => Some(DiffFamily::Kvm),
            HypervisorId::Esxi => Some(DiffFamily::Esxi),
            HypervisorId::Qemu => Some(DiffFamily::Qemu),
            HypervisorId::Wsl => Some(DiffFamily::Wsl),
            HypervisorId::Unknown => None,
        }
    }

    /// FNV 折叠用稳定编号（0..=5；新增家族只许追加不许重排）。
    pub const fn stable_index(self) -> u32 {
        match self {
            HypervisorId::HyperV => 0,
            HypervisorId::Kvm => 1,
            HypervisorId::Esxi => 2,
            HypervisorId::Qemu => 3,
            HypervisorId::Wsl => 4,
            HypervisorId::Unknown => 5,
        }
    }
}

/// 差异表家族闭集（与 HypervisorId 五家族一一对应；独立类型以免误传 Unknown）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffFamily {
    /// Hyper-V 家族。
    HyperV,
    /// KVM 家族。
    Kvm,
    /// ESXi 家族。
    Esxi,
    /// QEMU 家族。
    Qemu,
    /// WSL2 家族（按 WSL2 内核版本建区间）。
    Wsl,
}

/// 检测三态（判据核心：Bare 是结论不是失败；与"无法检测"严格区分）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionState {
    /// 裸金属：CPUID 0x40000000 全零签名——显性"无 hypervisor"结论。
    Bare,
    /// 命中识别表且家族入册。
    Virtualized,
    /// 有签名但未入册，或探测源不可靠——无法检测，禁止冒充任何结论。
    Indeterminate,
}

impl DetectionState {
    /// 稳定编号（指纹面用）。
    pub const fn stable_index(self) -> u32 {
        match self {
            DetectionState::Bare => 0,
            DetectionState::Virtualized => 1,
            DetectionState::Indeterminate => 2,
        }
    }
}

/// 置信三态（画像面输出；由数值置信分档，分界常量见 CONF_*_BAND）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence3 {
    /// 高置信（>= 80）。
    High,
    /// 中置信（>= 50）。
    Medium,
    /// 低置信（< 50）。
    Low,
}

impl Confidence3 {
    /// 数值置信 → 三态分档（纯函数）。
    pub fn from_score(score: u8) -> Confidence3 {
        if score >= CONF_HIGH_BAND {
            Confidence3::High
        } else if score >= CONF_MEDIUM_BAND {
            Confidence3::Medium
        } else {
            Confidence3::Low
        }
    }

    /// 稳定编号（指纹面用）。
    pub const fn stable_index(self) -> u32 {
        match self {
            Confidence3::High => 2,
            Confidence3::Medium => 1,
            Confidence3::Low => 0,
        }
    }
}

/// 12 字节签名的低 8 字节小端打包（const fn，编译期建表用）。
const fn pack8(bytes: &[u8; 8]) -> u64 {
    let mut v: u64 = 0;
    let mut i: usize = 0;
    while i < 8 {
        v |= (bytes[i] as u64) << (i * 8);
        i += 1;
    }
    v
}

/// 12 字节签名的高 4 字节零扩展打包（hi 槽高 32 位恒零——签名只有 12 字节）。
const fn pack4(bytes: &[u8; 4]) -> u64 {
    (bytes[0] as u64)
        | ((bytes[1] as u64) << 8)
        | ((bytes[2] as u64) << 16)
        | ((bytes[3] as u64) << 24)
}

/// 识别表条目。签名主键 = (lo, hi) 两 u64；hint/hint_mask 实现"同签名多行、
/// 按 CPUID 0x40000001.EAX 裁决"的仲裁结构（判据"多签名同 match 歧义时按
/// 置信序裁决并留痕"）。
pub struct SigEntry {
    /// 签名低 8 字节（CPUID 0x40000000 EBX|ECX 小端拼接）。
    pub sig_lo: u64,
    /// 签名高 4 字节（CPUID 0x40000000 EDX 低 32 位；高 32 位恒零）。
    pub sig_hi: u64,
    /// 命中后映射家族。
    pub hv: HypervisorId,
    /// 置信分（0~100；裁决序：高分胜出，同分取表序小者）。
    pub confidence: u8,
    /// hint 精确值（ebx_hint & hint_mask == hint 才命中本行）。
    pub hint: u32,
    /// hint 掩码（0 表示本行不看 hint——无条件命中）。
    pub hint_mask: u32,
    /// 来源注记（公开文档出处；逐条留痕）。
    pub note: &'static str,
}

/// 识别表 v1（只读 const；演进纪律：只增不改）。
///
/// 来源注记逐条对应：
/// - "Microsoft Hv"：Microsoft Hyper-V TLFS（Top-Level Functional Spec），
///   CPUID 0x40000000 EBX/ECX/EDX 返回 12 字节 "Microsoft Hv"。
/// - "KVMKVMKVM"：Linux 内核 Documentation/virt/kvm/cpuid.rst（9 字节 +
///   3 NUL）。
/// - "VMwareVMware"：VMware vSphere/ESXi 文档（ESXi 与 Workstation 共签名，
///   置信降档 70 并留痕）。
/// - "TCGTCGTCGTCG"：QEMU target/i386/cpu.c（TCG 加速时置 TCG 签名）。
/// - Xen/VBox/Parallels/ACRN/bhyve：各自公开文档；家族未入册 → Unknown 行
///   （命中后返回 IdForeign，签名值本身仍留痕可查）。
pub const SIG_TABLE_V1: [SigEntry; 12] = [
    // "Microsoft Hv" 无条件行（裸 Hyper-V；置信 60）
    SigEntry {
        sig_lo: pack8(b"Microsof"),
        sig_hi: pack4(b"t Hv"),
        hv: HypervisorId::HyperV,
        confidence: 60,
        hint: 0,
        hint_mask: 0,
        note: "Hyper-V TLFS: CPUID 0x40000000 \"Microsoft Hv\"",
    },
    // "Microsoft Hv" + WSL2 hint 行（置信 90 胜出裸 Hyper-V 行——WSL2 专项入口）
    SigEntry {
        sig_lo: pack8(b"Microsof"),
        sig_hi: pack4(b"t Hv"),
        hv: HypervisorId::Wsl,
        confidence: 90,
        hint: WSL2_HINT_MAGIC,
        hint_mask: 0xFFFF_FFFF,
        note: "WSL2 专项约定: 0x40000001.EAX == WSL2 魔数（平台约定）",
    },
    // "KVMKVMKVM\0\0\0"
    SigEntry {
        sig_lo: pack8(b"KVMKVMKV"),
        sig_hi: pack4(b"M\x00\x00\x00"),
        hv: HypervisorId::Kvm,
        confidence: 90,
        hint: 0,
        hint_mask: 0,
        note: "Linux cpuid.rst: KVM 签名 \"KVMKVMKVM\"",
    },
    // "VMwareVMware"（ESXi/Workstation 共签名——降档留痕）
    SigEntry {
        sig_lo: pack8(b"VMwareVM"),
        sig_hi: pack4(b"ware"),
        hv: HypervisorId::Esxi,
        confidence: 70,
        hint: 0,
        hint_mask: 0,
        note: "VMware 文档: \"VMwareVMware\"（ESXi/Workstation 共签名，降档）",
    },
    // "TCGTCGTCGTCG"（QEMU TCG）
    SigEntry {
        sig_lo: pack8(b"TCGTCGTC"),
        sig_hi: pack4(b"GTCG"),
        hv: HypervisorId::Qemu,
        confidence: 90,
        hint: 0,
        hint_mask: 0,
        note: "QEMU cpu.c: TCG 加速签名 \"TCGTCGTCGTCG\"",
    },
    // 未入册家族（命中 → Unknown + IdForeign；签名留痕）
    SigEntry {
        sig_lo: pack8(b"XenVMMXe"),
        sig_hi: pack4(b"nVMM"),
        hv: HypervisorId::Unknown,
        confidence: 85,
        hint: 0,
        hint_mask: 0,
        note: "Xen HVM 签名 \"XenVMMXenVMM\"（家族未入册）",
    },
    SigEntry {
        sig_lo: pack8(b"VBoxVBox"),
        sig_hi: pack4(b"VBox"),
        hv: HypervisorId::Unknown,
        confidence: 85,
        hint: 0,
        hint_mask: 0,
        note: "VirtualBox 签名 \"VBoxVBoxVBox\"（家族未入册）",
    },
    SigEntry {
        sig_lo: pack8(b"prl hype"),
        sig_hi: pack4(b"rv\x00\x00"),
        hv: HypervisorId::Unknown,
        confidence: 85,
        hint: 0,
        hint_mask: 0,
        note: "Parallels 签名 \"prl hyperv\"（家族未入册）",
    },
    SigEntry {
        sig_lo: pack8(b"lrpepyh "),
        sig_hi: pack4(b"vr\x00\x00"),
        hv: HypervisorId::Unknown,
        confidence: 85,
        hint: 0,
        hint_mask: 0,
        note: "Parallels 备用签名 \"lrpepyh vr\"（家族未入册）",
    },
    SigEntry {
        sig_lo: pack8(b"ACRNACRN"),
        sig_hi: pack4(b"ACRN"),
        hv: HypervisorId::Unknown,
        confidence: 85,
        hint: 0,
        hint_mask: 0,
        note: "Project ACRN 签名 \"ACRNACRNACRN\"（家族未入册）",
    },
    SigEntry {
        sig_lo: pack8(b"bhyve bh"),
        sig_hi: pack4(b"yve "),
        hv: HypervisorId::Unknown,
        confidence: 85,
        hint: 0,
        hint_mask: 0,
        note: "bhyve 签名 \"bhyve bhyve \"（家族未入册）",
    },
    // 占位尾行：全零签名禁止命中（Bare 语义在表外显性处理——签名全零是
    // "无 hypervisor"结论，不是表内一行；此行存在仅为让表长=12 齐整并
    // 被 const 断言锁死为永不命中行）。
    SigEntry {
        sig_lo: 0,
        sig_hi: 0,
        hv: HypervisorId::Unknown,
        confidence: 0,
        hint: 0,
        hint_mask: 0xFFFF_FFFF, // 掩码全 1 而值 0：只匹配"全零 hint 且全零签名"——被调用前置检查拦截
        note: "永不命中占位行（Bare 在表外显性处理）",
    },
];

// 编译期常量校验（允许的 assert 形态：const 上下文）。
const _: () = assert!(SIG_TABLE_V1.len() == 12, "识别表 v1 固定 12 行");
const _: () = assert!(SIG_TABLE_V1[11].confidence == 0, "占位行置信必须为 0");

/// 一次签名匹配的留痕（仲裁账条目）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchTrace {
    /// 命中的识别表行号。
    pub entry_index: usize,
    /// 该行映射家族。
    pub hv: HypervisorId,
    /// 该行置信分。
    pub confidence: u8,
    /// 是否启用了 hint 裁决（hint_mask != 0）。
    pub hint_used: bool,
}

impl MatchTrace {
    /// 留痕指纹折叠（确定性）。
    pub fn fold(&self, acc: u64) -> u64 {
        let acc = fnv1a_u64(acc, self.entry_index as u64);
        let acc = fnv1a_u32(acc, self.hv.stable_index());
        let acc = fnv1a_u32(acc, self.confidence as u32);
        fnv1a_u32(acc, self.hint_used as u32)
    }
}

/// CPUID 0x40000000 寄存器三元组 → 签名主键（lo=EBX|ECX 拼接，hi=EDX 零扩展）。
pub const fn sig_from_regs(ebx: u32, ecx: u32, edx: u32) -> (u64, u64) {
    let lo = (ebx as u64) | ((ecx as u64) << 32);
    let hi = edx as u64;
    (lo, hi)
}

/// 识别报告（含全量仲裁留痕——判据"留痕"）。
#[derive(Debug, Clone)]
pub struct IdentifyReport {
    /// 三态结论。
    pub state: DetectionState,
    /// 家族落点（Bare/Indeterminate 时为 Unknown）。
    pub hv: HypervisorId,
    /// 裁决胜出行置信分。
    pub confidence: u8,
    /// 胜出行号（无命中为 usize::MAX——不用 i32 避免有无谓符号）。
    pub chosen_index: usize,
    /// 全量匹配留痕（含被裁决压掉的候选）。
    pub traces: Vec<MatchTrace>,
    /// 结论码（0x12B0 段；X_VM_OK 或 ID_TABLE_MISS/ID_FOREIGN）。
    pub code: u32,
    /// 结论注记。
    pub note: &'static str,
}

/// 签名全零判定（Bare 前置检查）。
const fn sig_is_zero(lo: u64, hi: u64) -> bool {
    lo == 0 && hi == 0
}

/// hypervisor 识别纯函数（判据"识别表"主入口）。
///
/// - `vendor_lo`/`vendor_hi`：CPUID 0x40000000 EBX/ECX/EDX 组成的 12 字节
///   签名主键（用 [`sig_from_regs`] 组装）。
/// - `ebx_hint`：CPUID 0x40000001.EAX（家族内裁决提示；WSL2 平台层注入
///   `WSL2_HINT_MAGIC`）。
///
/// 三态语义：
/// 1. 签名全零 → Bare（显性结论"无 hypervisor"，不是失败）。
/// 2. 非零签名命中识别表 → 多候选按置信序裁决（同分取表序小者），全量留痕。
/// 3. 非零签名未命中 → Indeterminate + IdTableMiss（收集返回值入册）。
/// 4. 命中行家族为 Unknown → Indeterminate + IdForeign（签名已知、家族未入册）。
pub fn identify_from_cpuid(vendor_hi: u64, vendor_lo: u64, ebx_hint: u32) -> IdentifyReport {
    // Bare 前置：全零签名是"无 hypervisor"的显性结论。
    if sig_is_zero(vendor_lo, vendor_hi) {
        return IdentifyReport {
            state: DetectionState::Bare,
            hv: HypervisorId::Unknown,
            confidence: 100,
            chosen_index: usize::MAX,
            traces: Vec::new(),
            code: X_VM_OK,
            note: "签名全零：裸金属显性结论（Bare 是结论不是失败）",
        };
    }

    // 全表扫描收集候选（含被压掉的候选——留痕完整性）。
    let mut traces: Vec<MatchTrace> = Vec::new();
    let mut best: Option<(usize, HypervisorId, u8)> = None;
    let mut i = 0;
    while i < SIG_TABLE_V1.len() {
        let e = &SIG_TABLE_V1[i];
        let sig_hit = e.sig_lo == vendor_lo && e.sig_hi == vendor_hi;
        // 占位行（全零签名键）不可能走到这里：Bare 分支已拦截全零签名。
        let hint_hit = (ebx_hint & e.hint_mask) == e.hint;
        if sig_hit && hint_hit {
            traces.push(MatchTrace {
                entry_index: i,
                hv: e.hv,
                confidence: e.confidence,
                hint_used: e.hint_mask != 0,
            });
            let better = match best {
                None => true,
                Some((_, _, bc)) => e.confidence > bc, // 同分取表序小者：严格大于才换
            };
            if better {
                best = Some((i, e.hv, e.confidence));
            }
        }
        i += 1;
    }

    match best {
        None => IdentifyReport {
            state: DetectionState::Indeterminate,
            hv: HypervisorId::Unknown,
            confidence: 0,
            chosen_index: usize::MAX,
            traces,
            code: X_VM_ID_TABLE_MISS,
            note: "签名未在识别表中，建议收集 CPUID 0x40000000 返回值入册",
        },
        Some((idx, hv, conf)) => {
            if hv == HypervisorId::Unknown {
                IdentifyReport {
                    state: DetectionState::Indeterminate,
                    hv: HypervisorId::Unknown,
                    confidence: conf,
                    chosen_index: idx,
                    traces,
                    code: X_VM_ID_FOREIGN,
                    note: "签名已知但家族未入册（识别表只收五家族），建议走演进流程",
                }
            } else {
                IdentifyReport {
                    state: DetectionState::Virtualized,
                    hv,
                    confidence: conf,
                    chosen_index: idx,
                    traces,
                    code: X_VM_OK,
                    note: "签名命中识别表且家族入册",
                }
            }
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §5 · 平台矩阵（判据"平台矩阵/矩阵版本化"）
// ═════════════════════════════════════════════════════════════════════════════

/// guest 操作系统闭集（矩阵行维）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestOs {
    /// Windows guest。
    Windows,
    /// Linux guest。
    Linux,
}

impl GuestOs {
    /// 稳定编号（指纹面/快照序列化用）。
    pub const fn stable_index(self) -> u32 {
        match self {
            GuestOs::Windows => 0,
            GuestOs::Linux => 1,
        }
    }

    /// 快照字符（W/L——快照编码面）。
    pub fn snapshot_char(self) -> char {
        match self {
            GuestOs::Windows => 'W',
            GuestOs::Linux => 'L',
        }
    }

    /// 快照字符解析（非 W/L 即 None——不猜）。
    pub fn from_snapshot_char(c: char) -> Option<GuestOs> {
        match c {
            'W' => Some(GuestOs::Windows),
            'L' => Some(GuestOs::Linux),
            _ => None,
        }
    }
}

/// 支持度三态 + Unknown（越界格显性不猜——矩阵不输出第四种"猜测态"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support3 {
    /// 支持（该组合有官方/平台验证的 GPU 面）。
    Supported,
    /// 部分（需额外驱动/降档路径）。
    Partial,
    /// 不支持（组合本身不成立或无 GPU 面）。
    Unsupported,
    /// 越界/未登记——显性 Unknown。
    Unknown,
}

impl Support3 {
    /// 稳定编号（指纹面用）。
    pub const fn stable_index(self) -> u32 {
        match self {
            Support3::Supported => 3,
            Support3::Partial => 2,
            Support3::Unsupported => 1,
            Support3::Unknown => 0,
        }
    }

    /// 快照字符（S/P/N/U）。
    pub fn snapshot_char(self) -> char {
        match self {
            Support3::Supported => 'S',
            Support3::Partial => 'P',
            Support3::Unsupported => 'N',
            Support3::Unknown => 'U',
        }
    }

    /// 快照字符解析。
    pub fn from_snapshot_char(c: char) -> Option<Support3> {
        match c {
            'S' => Some(Support3::Supported),
            'P' => Some(Support3::Partial),
            'N' => Some(Support3::Unsupported),
            'U' => Some(Support3::Unknown),
            _ => None,
        }
    }
}

/// 矩阵格（v1 只读 const）。
pub struct MatrixCell {
    /// 行维：guest。
    pub guest: GuestOs,
    /// 列维：hypervisor 家族。
    pub hv: HypervisorId,
    /// 支持度三态。
    pub support: Support3,
    /// 格注记（证据面；写死不动态生成）。
    pub note: &'static str,
}

/// 平台矩阵 v1：Windows/Linux guest × 五家族 = 10 格逐格显性。
///
/// 版本 v1 登记口径（演进纪律：只增不改——改格必须升 MATRIX_VERSION）：
/// 来源为各 hypervisor 官方 GPU 支持面的平台层摘要，非厂商承诺原文。
pub const MATRIX_V1: [MatrixCell; 10] = [
    // Windows guest 行
    MatrixCell {
        guest: GuestOs::Windows,
        hv: HypervisorId::HyperV,
        support: Support3::Supported,
        note: "Hyper-V GPU-P/vGPU：Windows guest 官方支持面",
    },
    MatrixCell {
        guest: GuestOs::Windows,
        hv: HypervisorId::Kvm,
        support: Support3::Partial,
        note: "KVM Windows guest：virtio-gpu 需额外 guest 驱动，降档部分支持",
    },
    MatrixCell {
        guest: GuestOs::Windows,
        hv: HypervisorId::Esxi,
        support: Support3::Supported,
        note: "ESXi SVGA/vGPU：Windows guest 官方支持面",
    },
    MatrixCell {
        guest: GuestOs::Windows,
        hv: HypervisorId::Qemu,
        support: Support3::Partial,
        note: "QEMU TCG：QXL/STD VGA 软渲染面，降档部分支持",
    },
    MatrixCell {
        guest: GuestOs::Windows,
        hv: HypervisorId::Wsl,
        support: Support3::Unsupported,
        note: "WSL2 内不能运行 Windows guest：组合不成立",
    },
    // Linux guest 行
    MatrixCell {
        guest: GuestOs::Linux,
        hv: HypervisorId::HyperV,
        support: Support3::Partial,
        note: "Hyper-V Linux guest：集成组件面有限，WSLg 才是 dxg 直通通道",
    },
    MatrixCell {
        guest: GuestOs::Linux,
        hv: HypervisorId::Kvm,
        support: Support3::Supported,
        note: "KVM Linux guest：virtio-gpu/venus 官方支持面",
    },
    MatrixCell {
        guest: GuestOs::Linux,
        hv: HypervisorId::Esxi,
        support: Support3::Partial,
        note: "ESXi Linux guest：SVGA 2D 基线；vGPU 需授权，降档部分支持",
    },
    MatrixCell {
        guest: GuestOs::Linux,
        hv: HypervisorId::Qemu,
        support: Support3::Partial,
        note: "QEMU Linux guest：virgl/venus 面，降档部分支持",
    },
    MatrixCell {
        guest: GuestOs::Linux,
        hv: HypervisorId::Wsl,
        support: Support3::Supported,
        note: "WSL2 本身即 Linux guest：WSLg /dev/dxg 官方面",
    },
];

// 编译期校验：矩阵 10 格 = 2 guest × 5 家族，行列完备。
const _: () = assert!(MATRIX_V1.len() == 10, "矩阵 v1 固定 10 格");

/// 矩阵查询结果（带版本——判据"矩阵版本化"落点）。
#[derive(Debug, Clone, Copy)]
pub struct MatrixLookup {
    /// 命中的矩阵版本。
    pub version: u32,
    /// 支持度三态。
    pub support: Support3,
    /// 格注记。
    pub note: &'static str,
    /// 结论码（X_VM_OK 或 X_VM_MATRIX_CELL_UNKNOWN）。
    pub code: u32,
}

/// 矩阵查询（枚举封闭维——所有枚举组合必有格）。
pub fn matrix_lookup(guest: GuestOs, hv: HypervisorId) -> MatrixLookup {
    let mut i = 0;
    while i < MATRIX_V1.len() {
        let c = &MATRIX_V1[i];
        if c.guest == guest && c.hv == hv {
            return MatrixLookup {
                version: MATRIX_VERSION,
                support: c.support,
                note: c.note,
                code: X_VM_OK,
            };
        }
        i += 1;
    }
    // 枚举封闭时理论不可达（HypervisorId::Unknown 无格）——但显性落点不猜。
    MatrixLookup {
        version: MATRIX_VERSION,
        support: Support3::Unknown,
        note: "越界 guest/hyp 组合，显性 Unknown 不猜",
        code: X_VM_MATRIX_CELL_UNKNOWN,
    }
}

/// 原始下标查询（外部原始编号进来的路径；越界显性 Unknown 带码）。
pub fn matrix_lookup_raw(guest_idx: u8, hv_idx: u8) -> MatrixLookup {
    let guest = match guest_idx {
        0 => GuestOs::Windows,
        1 => GuestOs::Linux,
        _ => {
            return MatrixLookup {
                version: MATRIX_VERSION,
                support: Support3::Unknown,
                note: "guest 下标越界（0=Windows,1=Linux），显性 Unknown 不猜",
                code: X_VM_MATRIX_CELL_UNKNOWN,
            }
        }
    };
    let hv = match hv_idx {
        0 => HypervisorId::HyperV,
        1 => HypervisorId::Kvm,
        2 => HypervisorId::Esxi,
        3 => HypervisorId::Qemu,
        4 => HypervisorId::Wsl,
        _ => {
            return MatrixLookup {
                version: MATRIX_VERSION,
                support: Support3::Unknown,
                note: "hypervisor 下标越界（0..=4），显性 Unknown 不猜",
                code: X_VM_MATRIX_CELL_UNKNOWN,
            }
        }
    };
    matrix_lookup(guest, hv)
}

/// 矩阵逐行查询（某 guest 的五列全量视图）。
pub fn matrix_row_for_guest(guest: GuestOs) -> [MatrixLookup; 5] {
    [
        matrix_lookup(guest, HypervisorId::HyperV),
        matrix_lookup(guest, HypervisorId::Kvm),
        matrix_lookup(guest, HypervisorId::Esxi),
        matrix_lookup(guest, HypervisorId::Qemu),
        matrix_lookup(guest, HypervisorId::Wsl),
    ]
}

/// 矩阵 v1 指纹（FNV-1a 逐格确定性折叠；快照往返的对账键）。
pub const fn matrix_fingerprint() -> u64 {
    let mut acc = fnv1a_u32(FNV_OFFSET_BASIS, MATRIX_VERSION);
    let mut i = 0;
    while i < MATRIX_V1.len() {
        let c = &MATRIX_V1[i];
        acc = fnv1a_u32(acc, c.guest.stable_index());
        acc = fnv1a_u32(acc, c.hv.stable_index());
        acc = fnv1a_u32(acc, c.support.stable_index());
        acc = fnv1a_bytes(acc, c.note.as_bytes());
        i += 1;
    }
    acc
}

// ═════════════════════════════════════════════════════════════════════════════
// §6 · 矩阵快照确定性序列化往返（演进审计面：快照可存档、可对账、可回读）
// ═════════════════════════════════════════════════════════════════════════════

/// 快照行前缀（确定性编码协议 v1）。
const SNAPSHOT_HEADER: &str = "VX-VMDETECT-MATRIX";

/// 矩阵 → 确定性快照行（首行头+版本，随后 10 行逐格 `W|HyperV|S|note`）。
///
/// 确定性保证：行序固定（表序）、分隔符固定、无时间戳无地址无浮点——
/// 同版本矩阵两次快照逐字节相同。
pub fn matrix_snapshot_lines() -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "{}|v{}",
        SNAPSHOT_HEADER, MATRIX_VERSION
    ));
    let mut i = 0;
    while i < MATRIX_V1.len() {
        let c = &MATRIX_V1[i];
        lines.push(format!(
            "{}|{}|{}|{}",
            c.guest.snapshot_char(),
            c.hv.family_name(),
            c.support.snapshot_char(),
            c.note
        ));
        i += 1;
    }
    lines
}

/// 快照行解析 → 矩阵指纹（往返对账；不重构矩阵本体——矩阵是 const，
/// 解析只验证"存档快照与 const 本体一致"）。
///
/// 校验面：行数、头、版本、guest 字符、家族名、支持度字符逐字段核对；
/// 任一不符返回 BadInput，版本不符返回 MatrixVersionMismatch。
pub fn parse_matrix_snapshot(lines: &[String]) -> Result<u64, VmDetectError> {
    if lines.len() != MATRIX_V1.len() + 1 {
        return Err(VmDetectError::BadInput);
    }
    // 头行：HEADER|vN
    let header = &lines[0];
    let expect_head = format!("{}|v{}", SNAPSHOT_HEADER, MATRIX_VERSION);
    if header != &expect_head {
        if header.starts_with(SNAPSHOT_HEADER) {
            return Err(VmDetectError::MatrixVersionMismatch);
        }
        return Err(VmDetectError::BadInput);
    }
    // 逐格核对：guest 字符、家族名、支持度字符必须与 const 本体逐行一致。
    let mut i = 0;
    while i < MATRIX_V1.len() {
        let c = &MATRIX_V1[i];
        let line = &lines[i + 1];
        let expect = format!(
            "{}|{}|{}|{}",
            c.guest.snapshot_char(),
            c.hv.family_name(),
            c.support.snapshot_char(),
            c.note
        );
        if line != &expect {
            return Err(VmDetectError::BadInput);
        }
        i += 1;
    }
    Ok(matrix_fingerprint())
}

// ═════════════════════════════════════════════════════════════════════════════
// §7 · 探测三态与 WSL 专项（判据"WSL 登记"）
// ═════════════════════════════════════════════════════════════════════════════

/// 路径存在性探测三态（判据"不造假补齐"：没探测过的路径永远 Unknown）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe3 {
    /// 路径存在（探测面实测）。
    Present,
    /// 路径不存在（探测面实测）。
    Missing,
    /// 未探测/探测面不确定——保持 Unknown，禁止补齐。
    Unknown,
}

impl Probe3 {
    /// 稳定编号（指纹面用）。
    pub const fn stable_index(self) -> u32 {
        match self {
            Probe3::Present => 2,
            Probe3::Missing => 1,
            Probe3::Unknown => 0,
        }
    }
}

/// WSL 路径种类闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WslPathKind {
    /// dxg 直通设备节点（WSLg GPU 的唯一硬通道）。
    DxgDevice,
    /// NVIDIA 设备节点（CUDA in WSL 面板）。
    NvidiaDev,
    /// NVIDIA 控制节点。
    NvidiaCtl,
    /// NVIDIA UVM 节点。
    NvidiaUvm,
    /// WSLg Wayland socket。
    WslgSocket,
    /// WSLg X11 socket。
    WslgX11,
}

/// WSL 路径登记表条目（const 只读；路径字符串写死，来源为 WSLg 公开文档
/// 与 WSL2 内核文档）。
pub struct WslPathDesc {
    /// 绝对路径。
    pub path: &'static str,
    /// 路径种类。
    pub kind: WslPathKind,
    /// 是否为 WSLg GPU 画像的必要路径（判据门槛用）。
    pub required_for_wslg: bool,
    /// 来源注记。
    pub note: &'static str,
}

/// WSL 路径登记表 v1（只增不改）。
pub const WSL_PATH_TABLE: [WslPathDesc; 8] = [
    WslPathDesc {
        path: "/dev/dxg",
        kind: WslPathKind::DxgDevice,
        required_for_wslg: true,
        note: "WSLg 文档：dxgkrnl 暴露的 D3D12 直通节点（必要路径）",
    },
    WslPathDesc {
        path: "/dev/nvidia0",
        kind: WslPathKind::NvidiaDev,
        required_for_wslg: false,
        note: "NVIDIA CUDA in WSL：设备节点 0（可选）",
    },
    WslPathDesc {
        path: "/dev/nvidiactl",
        kind: WslPathKind::NvidiaCtl,
        required_for_wslg: false,
        note: "NVIDIA CUDA in WSL：控制节点（可选）",
    },
    WslPathDesc {
        path: "/dev/nvidia-uvm",
        kind: WslPathKind::NvidiaUvm,
        required_for_wslg: false,
        note: "NVIDIA CUDA in WSL：UVM 节点（可选）",
    },
    WslPathDesc {
        path: "/mnt/wslg/runtime-dir/wayland-0",
        kind: WslPathKind::WslgSocket,
        required_for_wslg: false,
        note: "WSLg 文档：Wayland 合成器 socket（可选证据）",
    },
    WslPathDesc {
        path: "/mnt/wslg/.X11-unix/X0",
        kind: WslPathKind::WslgX11,
        required_for_wslg: false,
        note: "WSLg 文档：X11 socket（可选证据）",
    },
    WslPathDesc {
        path: "/mnt/wslg/weston.log",
        kind: WslPathKind::WslgSocket,
        required_for_wslg: false,
        note: "WSLg 文档：weston 日志（可选旁证）",
    },
    WslPathDesc {
        path: "/mnt/wslgversions",
        kind: WslPathKind::WslgSocket,
        required_for_wslg: false,
        note: "WSLg 文档：WSLg 版本清单（可选旁证）",
    },
];

// 编译期校验：必要路径有且只有 /dev/dxg 一条（门槛唯一性）。
const _: () = assert!(WSL_PATH_TABLE.len() == 8, "WSL 登记表 v1 固定 8 条");

/// WSL 探测登记条目（槽位 + 三态；槽位由 const 表行号锁定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WslProbeEntry {
    /// 登记表槽位（WSL_PATH_TABLE 行号）。
    pub slot: usize,
    /// 探测三态。
    pub state: Probe3,
}

/// WSL 路径登记器（判据"路径实现登记"）。
///
/// 纪律：路径名必须是 const 登记表成员（防假路径入账）；重复注册显性
/// 拒绝（探测结果不可静默覆盖——要改先撤账）；不造假补齐。
pub struct WslPathRegistry {
    entries: Vec<WslProbeEntry>,
}

impl WslPathRegistry {
    /// 新登记器（空账）。
    pub fn new() -> Self {
        WslPathRegistry {
            entries: Vec::new(),
        }
    }

    /// 登记表总槽位数。
    pub fn slot_total(&self) -> usize {
        WSL_PATH_TABLE.len()
    }

    /// 已登记槽位数（水位）。
    pub fn slot_registered(&self) -> usize {
        self.entries.len()
    }

    /// 按路径查槽位（const 表内查找；表外路径 None）。
    fn slot_of(path: &str) -> Option<usize> {
        let mut i = 0;
        while i < WSL_PATH_TABLE.len() {
            if WSL_PATH_TABLE[i].path == path {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记一次探测结果。表外路径或重复注册 → 拒绝带码。
    pub fn register(&mut self, path: &str, state: Probe3) -> Result<usize, VmDetectError> {
        let slot = match Self::slot_of(path) {
            Some(s) => s,
            None => return Err(VmDetectError::WslPathConflict), // 路径不在登记表
        };
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].slot == slot {
                return Err(VmDetectError::WslPathConflict); // 重复注册：先撤账再改
            }
            i += 1;
        }
        if path.is_empty() {
            return Err(VmDetectError::BadInput);
        }
        self.entries.push(WslProbeEntry { slot, state });
        Ok(slot)
    }

    /// 查询某路径当前登记态（未登记 → Unknown，不猜）。
    pub fn state_of(&self, path: &str) -> Probe3 {
        match Self::slot_of(path) {
            None => Probe3::Unknown,
            Some(slot) => {
                let mut i = 0;
                while i < self.entries.len() {
                    if self.entries[i].slot == slot {
                        return self.entries[i].state;
                    }
                    i += 1;
                }
                Probe3::Unknown
            }
        }
    }

    /// WSLg 必要路径中尚未 Present 的清单（门槛证据面）。
    pub fn required_not_present(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < WSL_PATH_TABLE.len() {
            let d = &WSL_PATH_TABLE[i];
            if d.required_for_wslg && self.state_of(d.path) != Probe3::Present {
                out.push(d.path);
            }
            i += 1;
        }
        out
    }

    /// Unknown 槽位计数（体检面：不造假补齐的显性水位）。
    pub fn unknown_slots(&self) -> usize {
        WSL_PATH_TABLE.len() - self.slot_registered()
    }
}

impl Default for WslPathRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// WSLg GPU 画像（判据"WSL 专项"结论面）。
#[derive(Debug, Clone)]
pub struct WslgProfile {
    /// 门槛是否打开（WSL 签名 ∧ /dev/dxg Present）。
    pub gate_open: bool,
    /// 结论码（X_VM_OK 或 X_VM_WSLG_GATE_UNMET）。
    pub code: u32,
    /// /dev/dxg 探测三态。
    pub dxg: Probe3,
    /// WSL 签名是否命中（step1 证据回看）。
    pub signature_wsl: bool,
    /// 缺席的必要路径清单（gate 关门时的证据）。
    pub missing_required: Vec<&'static str>,
    /// 结论注记。
    pub note: &'static str,
}

/// WSLg 门槛裁决（纯函数：签名证据 + 路径登记 → 画像）。
///
/// 规则：signature_wsl（step1 命中 Wsl）∧ dxg Present → 开门；
/// 否则关门并给出缺席清单。Missing dxg 与 Unknown dxg 都关门，但证据不同
/// （Missing 是实测不存在；Unknown 是没探测过——不混淆）。
pub fn wslg_gate(hv: HypervisorId, reg: &WslPathRegistry) -> WslgProfile {
    let signature_wsl = hv == HypervisorId::Wsl;
    let dxg = reg.state_of("/dev/dxg");
    let missing = reg.required_not_present();
    if signature_wsl && dxg == Probe3::Present && missing.is_empty() {
        WslgProfile {
            gate_open: true,
            code: X_VM_OK,
            dxg,
            signature_wsl,
            missing_required: missing,
            note: "WSLg GPU 可用画像：WSL 签名命中 + /dev/dxg Present",
        }
    } else {
        WslgProfile {
            gate_open: false,
            code: X_VM_WSLG_GATE_UNMET,
            dxg,
            signature_wsl,
            missing_required: missing,
            note: "WSLg GPU 门槛未满足：签名或 /dev/dxg 缺一，画像不开门不猜测",
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §8 · 容器专项（判据"容器登记"）
// ═════════════════════════════════════════════════════════════════════════════

/// 容器 GPU 路径两族（判据闭集：nvidia-container / D3D KMT）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerFamily {
    /// NVIDIA Container Runtime 族（Linux 容器 GPU 注入面）。
    NvidiaContainer,
    /// D3D KMT 族（Windows 容器 / dxcore 枚举面）。
    D3dKmt,
}

impl ContainerFamily {
    /// 稳定编号（指纹面用）。
    pub fn stable_index(self) -> u32 {
        match self {
            ContainerFamily::NvidiaContainer => 0,
            ContainerFamily::D3dKmt => 1,
        }
    }
}

/// 容器路径登记表条目（const 只读；来源为 NVIDIA Container Toolkit 文档
/// 与 Windows 容器 GPU（D3DKMT）文档）。
pub struct ContainerPathDesc {
    /// 路径（文件系统路径或逻辑路径——D3D KMT 无文件节点，用逻辑路径登记，
    /// 注记里写明是逻辑路径不是文件系统路径）。
    pub path: &'static str,
    /// 所属族。
    pub family: ContainerFamily,
    /// 来源注记。
    pub note: &'static str,
}

/// 容器路径登记表 v1（只增不改）。
pub const CONTAINER_PATH_TABLE: [ContainerPathDesc; 7] = [
    ContainerPathDesc {
        path: "/usr/bin/nvidia-container-runtime",
        family: ContainerFamily::NvidiaContainer,
        note: "NVIDIA Container Toolkit：runtime 可执行（文件路径）",
    },
    ContainerPathDesc {
        path: "/usr/lib/x86_64-linux-gnu/libnvidia-container.so.1",
        family: ContainerFamily::NvidiaContainer,
        note: "NVIDIA Container Toolkit：核心库（文件路径）",
    },
    ContainerPathDesc {
        path: "/etc/nvidia-container-runtime/config.toml",
        family: ContainerFamily::NvidiaContainer,
        note: "NVIDIA Container Toolkit：配置（文件路径）",
    },
    ContainerPathDesc {
        path: "/dev/nvidia0",
        family: ContainerFamily::NvidiaContainer,
        note: "容器内 NVIDIA 设备节点（文件路径；与 WSL 表共用路径名但登记语境不同）",
    },
    ContainerPathDesc {
        path: "D3DKMT:enum-adapters",
        family: ContainerFamily::D3dKmt,
        note: "逻辑路径（非文件系统）：D3DKMT 适配器枚举入口",
    },
    ContainerPathDesc {
        path: "DXCORE:enum-adapters",
        family: ContainerFamily::D3dKmt,
        note: "逻辑路径（非文件系统）：DXCore 适配器枚举（Windows 容器 GPU 面）",
    },
    ContainerPathDesc {
        path: "C:\\Windows\\System32\\d3d10warp.dll",
        family: ContainerFamily::D3dKmt,
        note: "Windows 容器 WARP 软渲染回退（文件路径）",
    },
];

// 编译期校验：两族各有条目（表不许空族）。
const _: () = assert!(
    CONTAINER_PATH_TABLE.len() == 7,
    "容器登记表 v1 固定 7 条"
);

/// 容器探测登记条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerProbeEntry {
    /// 登记表槽位。
    pub slot: usize,
    /// 所属族（冗余存一份，免得查询再回表）。
    pub family: ContainerFamily,
    /// 探测三态。
    pub state: Probe3,
}

/// 容器路径登记器（判据"路径登记"）。
///
/// 纪律与 WSL 登记器一致：路径必须是 const 表成员、重复注册显性拒绝、
/// 不造假补齐。
pub struct ContainerPathRegistry {
    entries: Vec<ContainerProbeEntry>,
}

impl ContainerPathRegistry {
    /// 新登记器（空账）。
    pub fn new() -> Self {
        ContainerPathRegistry {
            entries: Vec::new(),
        }
    }

    /// 登记表总槽位数。
    pub fn slot_total(&self) -> usize {
        CONTAINER_PATH_TABLE.len()
    }

    /// 已登记槽位数（水位）。
    pub fn slot_registered(&self) -> usize {
        self.entries.len()
    }

    /// 按路径查槽位与族（表外路径 None）。
    fn slot_of(path: &str) -> Option<(usize, ContainerFamily)> {
        let mut i = 0;
        while i < CONTAINER_PATH_TABLE.len() {
            if CONTAINER_PATH_TABLE[i].path == path {
                return Some((i, CONTAINER_PATH_TABLE[i].family));
            }
            i += 1;
        }
        None
    }

    /// 登记一次探测结果。表外路径或重复注册 → 拒绝带码。
    pub fn register(&mut self, path: &str, state: Probe3) -> Result<usize, VmDetectError> {
        let (slot, family) = match Self::slot_of(path) {
            Some(v) => v,
            None => return Err(VmDetectError::ContainerPathConflict),
        };
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].slot == slot {
                return Err(VmDetectError::ContainerPathConflict); // 重复注册
            }
            i += 1;
        }
        if path.is_empty() {
            return Err(VmDetectError::BadInput);
        }
        self.entries.push(ContainerProbeEntry { slot, family, state });
        Ok(slot)
    }

    /// 查询某路径当前登记态（未登记 → Unknown，不猜）。
    pub fn state_of(&self, path: &str) -> Probe3 {
        match Self::slot_of(path) {
            None => Probe3::Unknown,
            Some((slot, _)) => {
                let mut i = 0;
                while i < self.entries.len() {
                    if self.entries[i].slot == slot {
                        return self.entries[i].state;
                    }
                    i += 1;
                }
                Probe3::Unknown
            }
        }
    }

    /// 某族是否存在 Present 探测（声称面：有实测 Present 才算声称）。
    pub fn family_claimed(&self, family: ContainerFamily) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].family == family && self.entries[i].state == Probe3::Present {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Unknown 槽位计数（体检面）。
    pub fn unknown_slots(&self) -> usize {
        CONTAINER_PATH_TABLE.len() - self.slot_registered()
    }
}

impl Default for ContainerPathRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// WSL/容器互斥审计结论（判据"矛盾入账"结构面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutexAudit {
    /// 是否发现互斥矛盾。
    pub contradiction: bool,
    /// 结论码（X_VM_OK 或 X_VM_WSL_CONTAINER_MUTEX）。
    pub code: u32,
    /// WSLg 声称（门槛开着）。
    pub wslg_claim: bool,
    /// nvidia-container 声称（族内有 Present）。
    pub nvidia_claim: bool,
    /// 结论注记。
    pub note: &'static str,
}

/// WSLg 与 nvidia-container 互斥校验（判据"同时声称即矛盾入账"）。
///
/// 语义口径：单一 guest 画像里，WSLg 直通（/dev/dxg）与 nvidia-container
/// runtime 注入同时 Present 不合常理（两族是不同的容器 GPU 注入通道）——
/// 矛盾**入账不裁决**：不判谁真谁假，画像整体降级并留矛盾计数。
pub fn check_wsl_container_mutex(
    reg: &ContainerPathRegistry,
    wslg: &WslgProfile,
) -> MutexAudit {
    let wslg_claim = wslg.gate_open;
    let nvidia_claim = reg.family_claimed(ContainerFamily::NvidiaContainer);
    let contradiction = wslg_claim && nvidia_claim;
    if contradiction {
        MutexAudit {
            contradiction,
            code: X_VM_WSL_CONTAINER_MUTEX,
            wslg_claim,
            nvidia_claim,
            note: "互斥矛盾入账：WSLg 与 nvidia-container 同时声称，画像降级不裁决",
        }
    } else {
        MutexAudit {
            contradiction,
            code: X_VM_OK,
            wslg_claim,
            nvidia_claim,
            note: "无互斥矛盾",
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §9 · 版本差异表（判据"差异"：区间命中 + 重叠/断档校验 + 差异复用家族）
// ═════════════════════════════════════════════════════════════════════════════

/// 版本号打包：major 高 16 位 | minor 低 16 位（const fn）。
pub const fn ver(major: u16, minor: u16) -> u32 {
    ((major as u32) << 16) | (minor as u32)
}

/// 取版本主版本号（差异复用家族的归并键）。
pub const fn ver_major(v: u32) -> u16 {
    (v >> 16) as u16
}

/// 版本差异条目：家族 × 半开区间 [lo, hi_excl) → 能力差异。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffEntry {
    /// 所属家族。
    pub family: DiffFamily,
    /// 区间下界（含）。
    pub lo: u32,
    /// 区间上界（不含；VER_MAX 表示无上界）。
    pub hi_excl: u32,
    /// 能力差异代号（短键，画像面输出）。
    pub capability: &'static str,
    /// 差异详情（诊断面输出）。
    pub detail: &'static str,
}

/// 版本差异表 v1 种子（登记口径：能力差异条目以内部画像为准，非厂商
/// 承诺原文；区间半开 [lo,hi)，全家族覆盖 0..=VER_MAX 无断档）。
pub const DIFF_TABLE_V1: [DiffEntry; 11] = [
    // Hyper-V：GPU-P（GPU 半虚拟化）随宿主版本/虚机代次演进
    DiffEntry {
        family: DiffFamily::HyperV,
        lo: 0,
        hi_excl: ver(6, 2),
        capability: "no-gpup",
        detail: "GPU-P 不可用：此区间宿主无 GPU 半虚拟化面",
    },
    DiffEntry {
        family: DiffFamily::HyperV,
        lo: ver(6, 2),
        hi_excl: ver(10, 0),
        capability: "gpup-gen1",
        detail: "GPU-P 仅 Gen1 虚机代次，能力面受限",
    },
    DiffEntry {
        family: DiffFamily::HyperV,
        lo: ver(10, 0),
        hi_excl: VER_MAX,
        capability: "gpup-gen2",
        detail: "GPU-P/Gen2 全量面",
    },
    // WSL2：dxgkrnl 特性随 WSL2 内核版本演进
    DiffEntry {
        family: DiffFamily::Wsl,
        lo: 0,
        hi_excl: ver(5, 10),
        capability: "dxg-basic",
        detail: "dxgkrnl 基线：D3D12 特性等级受限",
    },
    DiffEntry {
        family: DiffFamily::Wsl,
        lo: ver(5, 10),
        hi_excl: ver(5, 15),
        capability: "dxg-f120",
        detail: "WSL2 内核 5.10：D3D12 特性等级 12_0 面",
    },
    DiffEntry {
        family: DiffFamily::Wsl,
        lo: ver(5, 15),
        hi_excl: ver(6, 0),
        capability: "dxg-f121",
        detail: "WSL2 内核 5.15：D3D12 特性等级 12_1 面",
    },
    DiffEntry {
        family: DiffFamily::Wsl,
        lo: ver(6, 0),
        hi_excl: VER_MAX,
        capability: "dxg-f122",
        detail: "WSL2 内核 6.x：D3D12 特性等级 12_2 面",
    },
    // KVM：virtio-gpu → venus
    DiffEntry {
        family: DiffFamily::Kvm,
        lo: 0,
        hi_excl: ver(5, 0),
        capability: "virtio-gpu-2d",
        detail: "virtio-gpu 2D 基线：无 Vulkan 跨平台面",
    },
    DiffEntry {
        family: DiffFamily::Kvm,
        lo: ver(5, 0),
        hi_excl: VER_MAX,
        capability: "venus",
        detail: "Venus/Vulkan 跨平台面可用",
    },
    // ESXi：SVGA 基线 → vGPU/DDA
    DiffEntry {
        family: DiffFamily::Esxi,
        lo: 0,
        hi_excl: ver(7, 0),
        capability: "svga-limited",
        detail: "SVGA 2D 基线：vGPU/DDA 不可用",
    },
    DiffEntry {
        family: DiffFamily::Esxi,
        lo: ver(7, 0),
        hi_excl: VER_MAX,
        capability: "vgpu-dda",
        detail: "vGPU/离散设备分配面可用（需授权）",
    },
];

// 注意：DIFF_TABLE_V1 种子未含 QEMU 家族（QEMU 单条全区间在运行期注册，
// 用于覆盖"注册路径 + 断档审计"的动态面——见 VersionDiffTable::new）。
const _: () = assert!(DIFF_TABLE_V1.len() == 11, "差异表 v1 种子固定 11 条");

/// 版本差异表（判据"差异复用家族"本体）。
///
/// 登记纪律：
/// 1. 区间半开 [lo, hi)，要求 lo < hi（倒置 → BadInput）。
/// 2. 同家族内区间重叠显性拒绝（DiffRangeOverlap）。
/// 3. 家族必须在识别表入册（Unknown → DiffUnknownFamily）。
/// 4. 连续性审计：每家族覆盖 0..=VER_MAX 无断档（DiffRangeGap）。
/// 5. 差异复用家族：查询按主版本归并（次要版本复用主版本条目）。
pub struct VersionDiffTable {
    entries: Vec<DiffEntry>,
}

impl VersionDiffTable {
    /// 新表：种子 v1 + QEMU 全区间单条（动态注册路径的覆盖证明）。
    pub fn new() -> Self {
        let mut t = VersionDiffTable {
            entries: Vec::new(),
        };
        let mut i = 0;
        while i < DIFF_TABLE_V1.len() {
            let e = DIFF_TABLE_V1[i];
            // 种子条目由 const 保证合法；登记失败只可能来自运行期输入，
            // 但种子路径不吞错误——用"注册失败即留空"的防御式写法没有意义，
            // 种子是编译期常量，直接推入（区间合法性由 const 断言背书）。
            t.entries.push(e);
            i += 1;
        }
        // QEMU：全区间单条（TCG 软渲染不随版本分档——画像口径）。
        let _ = t.register(
            DiffFamily::Qemu,
            0,
            VER_MAX,
            "tcg-fallback",
            "TCG 软件模拟：GPU 直通不可用，帧面走软渲染",
        );
        t
    }

    /// 条目总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空表判定（false 恒真——表永不为空；保留以对称 API）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 家族是否在册（差异表只收五家族）。
    fn family_known(family: DiffFamily) -> bool {
        // 五家族闭集全部在册；此函数是显性闸门——将来加家族时这里是
        // 唯一需要改的点（防止 Unknown 混入差异表）。
        let _ = family;
        true
    }

    /// 登记一条差异。重叠/倒置/未知家族显性拒绝。
    pub fn register(
        &mut self,
        family: DiffFamily,
        lo: u32,
        hi_excl: u32,
        capability: &'static str,
        detail: &'static str,
    ) -> Result<(), VmDetectError> {
        if lo >= hi_excl {
            return Err(VmDetectError::BadInput); // 区间倒置/空区间
        }
        if !Self::family_known(family) {
            return Err(VmDetectError::DiffUnknownFamily);
        }
        if capability.is_empty() {
            return Err(VmDetectError::BadInput);
        }
        // 重叠检查：半开区间 [lo,hi) 与既有 [lo',hi') 重叠 ⇔ lo < hi' && lo' < hi。
        let mut i = 0;
        while i < self.entries.len() {
            let e = self.entries[i];
            if e.family == family && lo < e.hi_excl && e.lo < hi_excl {
                return Err(VmDetectError::DiffRangeOverlap);
            }
            i += 1;
        }
        self.entries.push(DiffEntry {
            family,
            lo,
            hi_excl,
            capability,
            detail,
        });
        Ok(())
    }

    /// 连续性审计：每家族区间序列从 0 起步、逐条衔接、终于 VER_MAX。
    /// 断档显性拒绝（返回 Gap），重叠理论不可达（登记已拒）仍复查。
    pub fn audit_contiguity(&self) -> Result<(), VmDetectError> {
        // 收集家族键的显性闭集遍历（不动态枚举——五家族固定）。
        let families = [
            DiffFamily::HyperV,
            DiffFamily::Kvm,
            DiffFamily::Esxi,
            DiffFamily::Qemu,
            DiffFamily::Wsl,
        ];
        let mut f = 0;
        while f < families.len() {
            let family = families[f];
            // 抽取该家族条目并按 lo 排序（选择排序：表小，避免引入排序依赖）。
            let mut idxs: Vec<usize> = Vec::new();
            let mut i = 0;
            while i < self.entries.len() {
                if self.entries[i].family == family {
                    idxs.push(i);
                }
                i += 1;
            }
            // 按 lo 升序选择排序 idxs。
            let mut a = 0;
            while a < idxs.len() {
                let mut b = a + 1;
                let mut min = a;
                while b < idxs.len() {
                    if self.entries[idxs[b]].lo < self.entries[idxs[min]].lo {
                        min = b;
                    }
                    b += 1;
                }
                let tmp = idxs[a];
                idxs[a] = idxs[min];
                idxs[min] = tmp;
                a += 1;
            }
            // 空家族（尚未登记任何条目）→ 断档。
            if idxs.is_empty() {
                return Err(VmDetectError::DiffRangeGap);
            }
            // 首条必须从 0 起步。
            if self.entries[idxs[0]].lo != 0 {
                return Err(VmDetectError::DiffRangeGap);
            }
            // 逐条衔接 + 复查重叠。
            let mut k = 0;
            while k < idxs.len() {
                let e = self.entries[idxs[k]];
                if k > 0 {
                    let prev = self.entries[idxs[k - 1]];
                    if e.lo < prev.hi_excl {
                        return Err(VmDetectError::DiffRangeOverlap);
                    }
                    if e.lo > prev.hi_excl {
                        return Err(VmDetectError::DiffRangeGap);
                    }
                }
                k += 1;
            }
            // 尾条必须终于 VER_MAX。
            if self.entries[idxs[idxs.len() - 1]].hi_excl != VER_MAX {
                return Err(VmDetectError::DiffRangeGap);
            }
            f += 1;
        }
        Ok(())
    }

    /// 区间命中查询（精确版本，含次要版本）。
    pub fn lookup(&self, family: DiffFamily, version: u32) -> Option<&DiffEntry> {
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.family == family && version >= e.lo && version < e.hi_excl {
                return Some(e);
            }
            i += 1;
        }
        None
    }

    /// 差异复用家族查询：次要版本归并到主版本（major<<16）后命中。
    ///
    /// 归并规则：同主版本内的次要版本差异不单独立条——查询端把 version
    /// 归并到主版本键再查区间。例：5.12 → 归并 5.0 → 命中 [5.10,5.15)
    /// 所在主版本条目（先精确命中，精确不中再归并——归并是回退不是覆盖）。
    pub fn lookup_family(&self, family: DiffFamily, version: u32) -> Option<&DiffEntry> {
        if let Some(e) = self.lookup(family, version) {
            return Some(e);
        }
        let major_key = ver(ver_major(version), 0);
        self.lookup(family, major_key)
    }

    /// 某家族条目数（体检面）。
    pub fn family_entry_count(&self, family: DiffFamily) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].family == family {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

impl Default for VersionDiffTable {
    fn default() -> Self {
        Self::new()
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §10 · CPUID 源抽象（判据"注入测试面"：真实源与注入源同一管道）
// ═════════════════════════════════════════════════════════════════════════════

/// CPUID 一次查询的四寄存器结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuidResult {
    /// EAX。
    pub eax: u32,
    /// EBX。
    pub ebx: u32,
    /// ECX。
    pub ecx: u32,
    /// EDX。
    pub edx: u32,
}

/// CPUID 源抽象。检测器只吃这个 trait——真实 CPUID 与注入 CPUID 走同一
/// 识别管道（判据"测试面"：无旁路）。
pub trait CpuidSource {
    /// 一次 CPUID 查询。
    fn cpuid(&self, leaf: u32, subleaf: u32) -> CpuidResult;
    /// 源是否可靠（宿主不支持 CPUID 查询时返回 false——检测器据此出
    /// Indeterminate 而不是误判 Bare）。
    fn reliable(&self) -> bool {
        true
    }
}

/// 真实源（宿主 CPUID）。仅在 x86_64 目标上真实执行；其他目标显性
/// 不可靠（reliable()=false → Indeterminate，绝不冒充 Bare）。
pub struct HostCpuidSource {
    _priv: (),
}

impl HostCpuidSource {
    /// 构造真实源。
    pub fn new() -> Self {
        HostCpuidSource { _priv: () }
    }
}

impl Default for HostCpuidSource {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuidSource for HostCpuidSource {
    #[cfg(target_arch = "x86_64")]
    fn cpuid(&self, leaf: u32, subleaf: u32) -> CpuidResult {
        // x86_64 真实 CPUID。cpuid 指令无 UB 面，现行 core::arch 已将其
        // 标为安全（早期版本的 unsafe 包裹在本工具链下反而触发
        // unused_unsafe 警告——按现行工具链口径直呼）。
        let r = core::arch::x86_64::__cpuid_count(leaf, subleaf);
        CpuidResult {
            eax: r.eax,
            ebx: r.ebx,
            ecx: r.ecx,
            edx: r.edx,
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn cpuid(&self, _leaf: u32, _subleaf: u32) -> CpuidResult {
        CpuidResult {
            eax: 0,
            ebx: 0,
            ecx: 0,
            edx: 0,
        }
    }

    #[cfg(target_arch = "x86_64")]
    fn reliable(&self) -> bool {
        true
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn reliable(&self) -> bool {
        false
    }
}

/// 注入源（测试钩子）：leaf→(subleaf→结果) 顺序查表，精确 (leaf,subleaf)
/// 命中优先，退化到 (leaf,0)，再退化为全零。
pub struct CpuidInjector {
    table: Vec<(u32, u32, CpuidResult)>,
}

impl CpuidInjector {
    /// 空注入源（全零返回 → Bare 画像，正好可测裸金属分支）。
    pub fn new() -> Self {
        CpuidInjector {
            table: Vec::new(),
        }
    }

    /// 注入一条 leaf 结果。
    pub fn set_leaf(&mut self, leaf: u32, subleaf: u32, eax: u32, ebx: u32, ecx: u32, edx: u32) {
        self.table.push((
            leaf,
            subleaf,
            CpuidResult { eax, ebx, ecx, edx },
        ));
    }

    /// 便捷注入：按 12 字节签名串设置 0x40000000（签名不足 12 字节零补齐，
    /// 超长截断——显性约定不是容错）。
    pub fn set_signature(&mut self, sig: &str) {
        let mut bytes = [0u8; 12];
        let sb = sig.as_bytes();
        let mut i = 0;
        while i < 12 && i < sb.len() {
            bytes[i] = sb[i];
            i += 1;
        }
        let ebx = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let ecx = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let edx = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        self.set_leaf(CPUID_HYPERVISOR_SIG_LEAF, 0, 0, ebx, ecx, edx);
    }
}

impl Default for CpuidInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuidSource for CpuidInjector {
    fn cpuid(&self, leaf: u32, subleaf: u32) -> CpuidResult {
        // 精确命中优先。
        let mut i = 0;
        while i < self.table.len() {
            let (l, s, r) = self.table[i];
            if l == leaf && s == subleaf {
                return r;
            }
            i += 1;
        }
        // 退化 (leaf, 0)。
        i = 0;
        while i < self.table.len() {
            let (l, s, r) = self.table[i];
            if l == leaf && s == 0 {
                return r;
            }
            i += 1;
        }
        // 全零兜底。
        CpuidResult {
            eax: 0,
            ebx: 0,
            ecx: 0,
            edx: 0,
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §11 · 检测账本（定长账 + 事件计数；判据"每步结果入账"）
// ═════════════════════════════════════════════════════════════════════════════

/// 账本事件（一步一证；payload_fp 为该步载荷指纹，供画像对账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerEvent {
    /// 所属步骤。
    pub step: u8,
    /// 结论码（0x12B0 段）。
    pub code: u32,
    /// 结论注记（&'static str——不堆动态串）。
    pub note: &'static str,
    /// 载荷指纹（FNV-1a 折叠该步关键量）。
    pub payload_fp: u64,
}

/// 定长检测账本。
///
/// 溢出选择（显性）：账满后新事件**丢弃并计数**（dropped 饱和递增），
/// 不阻塞主流程——账本是证据面不是控制面，丢证可追溯（dropped>0 即
/// 体检降档），不能因为丢证把检测挂死。
pub struct DetectionLedger {
    events: Vec<LedgerEvent>,
    cap: usize,
    /// 事件总数（含被丢弃的；saturating 只涨不回绕）。
    total: u64,
    /// 被丢弃事件数（saturating）。
    dropped: u64,
    /// 矛盾事件数（saturating）。
    contradictions: u64,
    /// Unknown/未决事件数（saturating）。
    unknowns: u64,
}

impl DetectionLedger {
    /// 定长账本（cap 写死语义见 LEDGER_CAP；测试面可注入小容量）。
    pub fn with_cap(cap: usize) -> Self {
        DetectionLedger {
            events: Vec::new(),
            cap,
            total: 0,
            dropped: 0,
            contradictions: 0,
            unknowns: 0,
        }
    }

    /// 新账本（默认容量 LEDGER_CAP）。
    pub fn new() -> Self {
        Self::with_cap(LEDGER_CAP)
    }

    /// 账容量。
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 在账事件数（水位）。
    pub fn watermark(&self) -> usize {
        self.events.len()
    }

    /// 在账事件切片。
    pub fn events(&self) -> &[LedgerEvent] {
        &self.events
    }

    /// 事件总数。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 被丢弃事件数。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 矛盾计数。
    pub fn contradictions(&self) -> u64 {
        self.contradictions
    }

    /// Unknown 计数。
    pub fn unknowns(&self) -> u64 {
        self.unknowns
    }

    /// 记一笔（永不失败：满了丢弃计数）。返回 X_VM_OK 或 X_VM_LEDGER_FULL。
    pub fn note(&mut self, step: u8, code: u32, note: &'static str, payload_fp: u64) -> u32 {
        self.total = self.total.saturating_add(1);
        // 矛盾/Unknown 语义归类（计数面与事件面同口径）。
        if code == X_VM_WSL_CONTAINER_MUTEX {
            self.contradictions = self.contradictions.saturating_add(1);
        }
        if code == X_VM_ID_TABLE_MISS
            || code == X_VM_ID_FOREIGN
            || code == X_VM_MATRIX_CELL_UNKNOWN
            || code == X_VM_WSLG_GATE_UNMET
            || code == X_VM_DIFF_UNKNOWN_FAMILY
        {
            self.unknowns = self.unknowns.saturating_add(1);
        }
        if self.events.len() >= self.cap {
            self.dropped = self.dropped.saturating_add(1);
            return X_VM_LEDGER_FULL;
        }
        self.events.push(LedgerEvent {
            step,
            code,
            note,
            payload_fp,
        });
        X_VM_OK
    }
}

impl Default for DetectionLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §12 · 检测器编排（判据"检测器"：五步单向流水 + 乱序拒绝带码）
// ═════════════════════════════════════════════════════════════════════════════

/// 流水步骤闭集（步骤编号即推进序——单向，禁止回退/跳步）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStep {
    /// 未开始。
    NotStarted,
    /// 步一：签名识别。
    Step1Signatures,
    /// 步二：平台矩阵查。
    Step2Matrix,
    /// 步三：WSL 专项。
    Step3Wsl,
    /// 步四：容器专项。
    Step4Container,
    /// 步五：版本差异画像。
    Step5Diff,
    /// 五步完成（终态；画像可取）。
    Completed,
}

impl PipelineStep {
    /// 推进序号（NotStarted=0 … Completed=6；单向性由序号比较裁决）。
    pub fn ordinal(self) -> u8 {
        match self {
            PipelineStep::NotStarted => 0,
            PipelineStep::Step1Signatures => 1,
            PipelineStep::Step2Matrix => 2,
            PipelineStep::Step3Wsl => 3,
            PipelineStep::Step4Container => 4,
            PipelineStep::Step5Diff => 5,
            PipelineStep::Completed => 6,
        }
    }

    /// 账本步骤槽位（1..=5）。
    pub fn ledger_slot(self) -> u8 {
        match self {
            PipelineStep::Step1Signatures => 1,
            PipelineStep::Step2Matrix => 2,
            PipelineStep::Step3Wsl => 3,
            PipelineStep::Step4Container => 4,
            PipelineStep::Step5Diff => 5,
            _ => 0,
        }
    }
}

/// 检测器（判据"检测器"编排本体）。
///
/// 五步单向流水：签名识别 → 平台矩阵查 → WSL 专项 → 容器专项 → 版本差异
/// 画像。每步产物入定长账；终态后可取 `EnvironmentProfile` 快照（含每步
/// 证据 + 置信三态 + FNV-1a 64 位指纹）。
///
/// 乱序纪律：每步要求当前游标恰为"本步前一步已完成"状态，否则拒绝带码
/// （X_VM_STEP_OUT_OF_ORDER）——回退与跳步都算乱序，没有例外路径。
pub struct VmDetector<'a> {
    source: &'a dyn CpuidSource,
    step: PipelineStep,
    ledger: DetectionLedger,
    identify: Option<IdentifyReport>,
    guest: Option<GuestOs>,
    matrix: Option<MatrixLookup>,
    wsl_reg: WslPathRegistry,
    container_reg: ContainerPathRegistry,
    wslg: Option<WslgProfile>,
    mutex_audit: Option<MutexAudit>,
    diff_table: VersionDiffTable,
    diff_capability: Option<&'static str>,
    diff_detail: Option<&'static str>,
    diff_version: Option<u32>,
}

impl<'a> VmDetector<'a> {
    /// 新检测器（差异表带 v1 种子并审计连续性——种子断档是构造期缺陷，
    /// 审计结果入账）。
    pub fn new(source: &'a dyn CpuidSource) -> Self {
        let mut det = VmDetector {
            source,
            step: PipelineStep::NotStarted,
            ledger: DetectionLedger::new(),
            identify: None,
            guest: None,
            matrix: None,
            wsl_reg: WslPathRegistry::new(),
            container_reg: ContainerPathRegistry::new(),
            wslg: None,
            mutex_audit: None,
            diff_table: VersionDiffTable::new(),
            diff_capability: None,
            diff_detail: None,
            diff_version: None,
        };
        match det.diff_table.audit_contiguity() {
            Ok(()) => {
                det.ledger.note(
                    0,
                    X_VM_OK,
                    "差异表 v1 种子连续性审计通过",
                    det.diff_table.len() as u64,
                );
            }
            Err(e) => {
                det.ledger.note(
                    0,
                    e.code(),
                    "差异表 v1 种子连续性审计失败（构造期缺陷，禁止静默）",
                    det.diff_table.len() as u64,
                );
            }
        }
        det
    }

    /// 当前游标。
    pub fn step(&self) -> PipelineStep {
        self.step
    }

    /// 账本视图。
    pub fn ledger(&self) -> &[LedgerEvent] {
        self.ledger.events()
    }

    /// 乱序守卫：要求游标恰为 expect。
    fn guard_step(&mut self, expect: PipelineStep) -> Result<(), VmDetectError> {
        if self.step != expect {
            self.ledger.note(
                expect.ledger_slot(),
                X_VM_STEP_OUT_OF_ORDER,
                "流水乱序调用被拒绝（单向五步，禁止回退/跳步）",
                self.step.ordinal() as u64,
            );
            return Err(VmDetectError::StepOutOfOrder);
        }
        Ok(())
    }

    /// 步一：签名识别（真实源/注入源同一管道）。
    pub fn step1_identify(&mut self) -> Result<IdentifyReport, VmDetectError> {
        self.guard_step(PipelineStep::NotStarted)?;
        let report = if self.source.reliable() {
            let r = self.source.cpuid(CPUID_HYPERVISOR_SIG_LEAF, 0);
            let (lo, hi) = sig_from_regs(r.ebx, r.ecx, r.edx);
            let hint = self.source.cpuid(CPUID_HYPERVISOR_INFO_LEAF, 0).eax;
            identify_from_cpuid(hi, lo, hint)
        } else {
            // 源不可靠：显性 Indeterminate——绝不冒充 Bare（Bare 是结论，
            // 误判比不判危害大：下游会按裸金属放开直通道具）。
            IdentifyReport {
                state: DetectionState::Indeterminate,
                hv: HypervisorId::Unknown,
                confidence: 0,
                chosen_index: usize::MAX,
                traces: Vec::new(),
                code: X_VM_ID_TABLE_MISS,
                note: "探测源不可靠：显性无法检测，禁止冒充 Bare",
            }
        };
        self.ledger.note(
            1,
            report.code,
            report.note,
            report.traces.iter().fold(fnv1a_u32(FNV_OFFSET_BASIS, report.state.stable_index()), |acc, t| t.fold(acc)),
        );
        self.step = PipelineStep::Step1Signatures;
        self.identify = Some(report.clone());
        Ok(report)
    }

    /// 步二：平台矩阵查（guest 维由调用方声明；hyp 维取步一结论）。
    pub fn step2_platform(&mut self, guest: GuestOs) -> Result<MatrixLookup, VmDetectError> {
        self.guard_step(PipelineStep::Step1Signatures)?;
        let hv = self
            .identify
            .as_ref()
            .map(|r| r.hv)
            .unwrap_or(HypervisorId::Unknown);
        let lookup = matrix_lookup(guest, hv);
        self.ledger.note(
            2,
            lookup.code,
            lookup.note,
            fnv1a_u64(
                fnv1a_u32(FNV_OFFSET_BASIS, guest.stable_index()),
                hv.stable_index() as u64,
            ),
        );
        self.step = PipelineStep::Step2Matrix;
        self.guest = Some(guest);
        self.matrix = Some(lookup);
        Ok(lookup)
    }

    /// 步三：WSL 专项（探测结果由平台层/注入面提供，检测器只登记与裁决）。
    pub fn step3_wsl(
        &mut self,
        probes: &[(&'static str, Probe3)],
    ) -> Result<WslgProfile, VmDetectError> {
        self.guard_step(PipelineStep::Step2Matrix)?;
        let mut last_err: Option<VmDetectError> = None;
        let mut i = 0;
        while i < probes.len() {
            let (path, state) = probes[i];
            if path.is_empty() {
                self.ledger.note(3, X_VM_BAD_INPUT, "空路径名被拒绝", i as u64);
                last_err = Some(VmDetectError::BadInput);
            } else if let Err(e) = self.wsl_reg.register(path, state) {
                // 登记冲突不中断批处理：逐条留痕，最后回报首错。
                self.ledger.note(3, e.code(), e.detail(), i as u64);
                if last_err.is_none() {
                    last_err = Some(e);
                }
            }
            i += 1;
        }
        let hv = self
            .identify
            .as_ref()
            .map(|r| r.hv)
            .unwrap_or(HypervisorId::Unknown);
        let profile = wslg_gate(hv, &self.wsl_reg);
        self.ledger.note(
            3,
            profile.code,
            profile.note,
            fnv1a_u32(
                fnv1a_u32(FNV_OFFSET_BASIS, profile.dxg.stable_index()),
                profile.signature_wsl as u32,
            ),
        );
        self.step = PipelineStep::Step3Wsl;
        self.wslg = Some(profile.clone());
        match last_err {
            Some(e) => Err(e), // 批内有坏账：画像已出，但坏账必须让调用方看见
            None => Ok(profile),
        }
    }

    /// 步四：容器专项（登记 + 与 WSLg 互斥校验——矛盾入账不裁决）。
    pub fn step4_container(
        &mut self,
        probes: &[(&'static str, Probe3)],
    ) -> Result<MutexAudit, VmDetectError> {
        self.guard_step(PipelineStep::Step3Wsl)?;
        let mut last_err: Option<VmDetectError> = None;
        let mut i = 0;
        while i < probes.len() {
            let (path, state) = probes[i];
            if path.is_empty() {
                self.ledger.note(4, X_VM_BAD_INPUT, "空路径名被拒绝", i as u64);
                last_err = Some(VmDetectError::BadInput);
            } else if let Err(e) = self.container_reg.register(path, state) {
                self.ledger.note(4, e.code(), e.detail(), i as u64);
                if last_err.is_none() {
                    last_err = Some(e);
                }
            }
            i += 1;
        }
        let wslg_default = wslg_gate(HypervisorId::Unknown, &WslPathRegistry::new());
        let wslg = self.wslg.as_ref().unwrap_or(&wslg_default);
        let audit = check_wsl_container_mutex(&self.container_reg, wslg);
        self.ledger.note(
            4,
            audit.code,
            audit.note,
            fnv1a_u32(
                fnv1a_u32(FNV_OFFSET_BASIS, audit.wslg_claim as u32),
                audit.nvidia_claim as u32,
            ),
        );
        self.step = PipelineStep::Step4Container;
        self.mutex_audit = Some(audit);
        match last_err {
            Some(e) => Err(e), // 同步三：坏账可见
            None => Ok(audit),
        }
    }

    /// 步五：版本差异画像（家族取步一结论；版本由调用方提供）。
    pub fn step5_diff(&mut self, version: u32) -> Result<(&'static str, &'static str), VmDetectError> {
        self.guard_step(PipelineStep::Step4Container)?;
        let hv = self
            .identify
            .as_ref()
            .map(|r| r.hv)
            .unwrap_or(HypervisorId::Unknown);
        match hv.diff_family() {
            None => {
                self.ledger.note(
                    5,
                    X_VM_DIFF_UNKNOWN_FAMILY,
                    "未知家族无差异条目：画像面给显性 unknown-family 不猜",
                    fnv1a_u64(FNV_OFFSET_BASIS, version as u64),
                );
                self.step = PipelineStep::Step5Diff;
                self.diff_capability = Some("unknown-family");
                self.diff_detail = Some("家族未入册，无差异条目可命中");
                self.diff_version = Some(version);
                Ok(("unknown-family", "家族未入册，无差异条目可命中"))
            }
            Some(family) => {
                let entry = self.diff_table.lookup_family(family, version);
                match entry {
                    Some(e) => {
                        self.ledger.note(
                            5,
                            X_VM_OK,
                            "版本差异区间命中（差异复用家族口径）",
                            fnv1a_u64(fnv1a_u32(FNV_OFFSET_BASIS, version), e.lo as u64),
                        );
                        self.step = PipelineStep::Step5Diff;
                        self.diff_capability = Some(e.capability);
                        self.diff_detail = Some(e.detail);
                        self.diff_version = Some(version);
                        Ok((e.capability, e.detail))
                    }
                    None => {
                        // 连续性审计通过时理论不可达；防御性落点仍显性。
                        self.ledger.note(
                            5,
                            X_VM_DIFF_RANGE_GAP,
                            "区间未命中但家族在册：疑似断档，审计面必须复核",
                            fnv1a_u64(FNV_OFFSET_BASIS, version as u64),
                        );
                        self.step = PipelineStep::Step5Diff;
                        self.diff_capability = Some("range-miss");
                        self.diff_detail = Some("区间未命中（疑似断档，需审计复核）");
                        self.diff_version = Some(version);
                        Err(VmDetectError::DiffRangeGap)
                    }
                }
            }
        }
    }

    /// 收口：五步终态落账，游标进 Completed。
    pub fn finish(&mut self) -> Result<(), VmDetectError> {
        self.guard_step(PipelineStep::Step5Diff)?;
        self.ledger.note(
            5,
            X_VM_OK,
            "五步流水收口：画像可取",
            fnv1a_u32(FNV_OFFSET_BASIS, PipelineStep::Completed.ordinal() as u32),
        );
        self.step = PipelineStep::Completed;
        Ok(())
    }

    /// 步一结论视图（步一完成后可用）。
    pub fn identify(&self) -> Option<&IdentifyReport> {
        self.identify.as_ref()
    }

    /// WSL 登记器视图（步三完成后可用）。
    pub fn wsl_registry(&self) -> &WslPathRegistry {
        &self.wsl_reg
    }

    /// 容器登记器视图（步四完成后可用）。
    pub fn container_registry(&self) -> &ContainerPathRegistry {
        &self.container_reg
    }

    /// 差异表视图。
    pub fn diff_table(&self) -> &VersionDiffTable {
        &self.diff_table
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §13 · 环境画像快照（判据"每步证据 + 置信三态 + 指纹"）
// ═════════════════════════════════════════════════════════════════════════════

/// 环境画像快照（一次检测的确定性结论面）。
///
/// 指纹纪律：FNV-1a 64 位按固定字段序折叠——同输入同指纹（确定性可复现）。
/// 隐私红线：指纹仅本地闭环（日志对账/回归比对），不上传不外带。
#[derive(Debug, Clone)]
pub struct EnvironmentProfile {
    /// 三态结论。
    pub state: DetectionState,
    /// 家族落点。
    pub hv: HypervisorId,
    /// 数值置信（步一裁决胜出行分）。
    pub confidence: u8,
    /// 置信三态。
    pub confidence_band: Confidence3,
    /// 矩阵版本。
    pub matrix_version: u32,
    /// 矩阵支持度。
    pub matrix_support: Support3,
    /// 矩阵格注记。
    pub matrix_note: &'static str,
    /// WSLg 门槛。
    pub wslg_gate_open: bool,
    /// WSLg 结论码。
    pub wslg_code: u32,
    /// 容器 NVIDIA 族声称。
    pub container_nvidia_claim: bool,
    /// 容器 D3DKMT 族声称。
    pub container_d3dkmt_claim: bool,
    /// WSL/容器互斥矛盾。
    pub mutex_contradiction: bool,
    /// 差异能力代号。
    pub diff_capability: &'static str,
    /// 差异版本输入。
    pub diff_version: u32,
    /// 仲裁留痕（多签名候选全量）。
    pub arbitration: Vec<MatchTrace>,
    /// 每步证据行（证据面；"stepN: …"前缀固定序）。
    pub evidence: Vec<String>,
    /// 环境指纹（FNV-1a 64；本地闭环不上传——隐私红线）。
    pub fingerprint: u64,
}

impl EnvironmentProfile {
    /// 指纹复算（画像对账面：存档指纹 == 复算指纹 → 画像未被篡改/换源）。
    pub fn recompute_fingerprint(&self) -> u64 {
        let mut acc = fnv1a_u32(FNV_OFFSET_BASIS, VCK69_VMDETECT_VERSION.len() as u32);
        acc = fnv1a_bytes(acc, VCK69_VMDETECT_VERSION.as_bytes());
        acc = fnv1a_u32(acc, self.state.stable_index());
        acc = fnv1a_u32(acc, self.hv.stable_index());
        acc = fnv1a_u32(acc, self.confidence as u32);
        acc = fnv1a_u32(acc, self.confidence_band.stable_index());
        acc = fnv1a_u32(acc, self.matrix_version);
        acc = fnv1a_u32(acc, self.matrix_support.stable_index());
        acc = fnv1a_bytes(acc, self.matrix_note.as_bytes());
        acc = fnv1a_u32(acc, self.wslg_gate_open as u32);
        acc = fnv1a_u32(acc, self.wslg_code);
        acc = fnv1a_u32(acc, self.container_nvidia_claim as u32);
        acc = fnv1a_u32(acc, self.container_d3dkmt_claim as u32);
        acc = fnv1a_u32(acc, self.mutex_contradiction as u32);
        acc = fnv1a_bytes(acc, self.diff_capability.as_bytes());
        acc = fnv1a_u32(acc, self.diff_version);
        acc = fnv1a_u32(acc, self.arbitration.len() as u32);
        let mut i = 0;
        while i < self.arbitration.len() {
            acc = self.arbitration[i].fold(acc);
            i += 1;
        }
        acc = fnv1a_u32(acc, self.evidence.len() as u32);
        i = 0;
        while i < self.evidence.len() {
            acc = fnv1a_bytes(acc, self.evidence[i].as_bytes());
            i += 1;
        }
        acc
    }

    /// 画像快照行（确定性序列化；往返对账用——同画像两次快照逐行相同）。
    pub fn snapshot_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("VX-VMDETECT-PROFILE|v{}", MATRIX_VERSION));
        lines.push(format!("state={}", self.state.stable_index()));
        lines.push(format!("hv={}", self.hv.stable_index()));
        lines.push(format!("confidence={}", self.confidence));
        lines.push(format!("band={}", self.confidence_band.stable_index()));
        lines.push(format!("matrix_version={}", self.matrix_version));
        lines.push(format!("matrix_support={}", self.matrix_support.stable_index()));
        lines.push(format!("wslg_gate={}", self.wslg_gate_open as u32));
        lines.push(format!("wslg_code={}", self.wslg_code));
        lines.push(format!("nvidia_claim={}", self.container_nvidia_claim as u32));
        lines.push(format!("d3dkmt_claim={}", self.container_d3dkmt_claim as u32));
        lines.push(format!("mutex={}", self.mutex_contradiction as u32));
        lines.push(format!("diff_cap={}", self.diff_capability));
        lines.push(format!("diff_version={}", self.diff_version));
        lines.push(format!("fingerprint={:016x}", self.fingerprint));
        lines
    }
}

impl VmDetector<'_> {
    /// 画像快照（要求五步收口；未收口拒绝带码 ProfileNotReady）。
    pub fn profile(&self) -> Result<EnvironmentProfile, VmDetectError> {
        if self.step != PipelineStep::Completed {
            return Err(VmDetectError::ProfileNotReady);
        }
        let identify = match &self.identify {
            Some(r) => r,
            None => return Err(VmDetectError::ProfileNotReady),
        };
        let matrix = match &self.matrix {
            Some(m) => m,
            None => return Err(VmDetectError::ProfileNotReady),
        };
        let wslg = match &self.wslg {
            Some(w) => w,
            None => return Err(VmDetectError::ProfileNotReady),
        };
        let mutex_audit = match &self.mutex_audit {
            Some(m) => m,
            None => return Err(VmDetectError::ProfileNotReady),
        };
        let cap = match self.diff_capability {
            Some(c) => c,
            None => return Err(VmDetectError::ProfileNotReady),
        };
        let detail = match self.diff_detail {
            Some(d) => d,
            None => return Err(VmDetectError::ProfileNotReady),
        };
        let version = match self.diff_version {
            Some(v) => v,
            None => return Err(VmDetectError::ProfileNotReady),
        };

        // 证据行（固定序，确定性编码）。
        let mut evidence = Vec::new();
        evidence.push(format!(
            "step1:state={};hv={};conf={};code={:04X}",
            identify.state.stable_index(),
            identify.hv.family_name(),
            identify.confidence,
            identify.code
        ));
        evidence.push(format!(
            "step2:matrix_v={};support={};note={}",
            matrix.version,
            matrix.support.snapshot_char(),
            matrix.note
        ));
        evidence.push(format!(
            "step3:wslg_gate={};dxg={};code={:04X}",
            wslg.gate_open as u32,
            wslg.dxg.stable_index(),
            wslg.code
        ));
        evidence.push(format!(
            "step4:nvidia={};d3dkmt={};mutex={}",
            self.container_reg.family_claimed(ContainerFamily::NvidiaContainer) as u32,
            self.container_reg.family_claimed(ContainerFamily::D3dKmt) as u32,
            mutex_audit.contradiction as u32
        ));
        evidence.push(format!("step5:cap={};detail={};version={}", cap, detail, version));

        let mut profile = EnvironmentProfile {
            state: identify.state,
            hv: identify.hv,
            confidence: identify.confidence,
            confidence_band: Confidence3::from_score(identify.confidence),
            matrix_version: matrix.version,
            matrix_support: matrix.support,
            matrix_note: matrix.note,
            wslg_gate_open: wslg.gate_open,
            wslg_code: wslg.code,
            container_nvidia_claim: self
                .container_reg
                .family_claimed(ContainerFamily::NvidiaContainer),
            container_d3dkmt_claim: self
                .container_reg
                .family_claimed(ContainerFamily::D3dKmt),
            mutex_contradiction: mutex_audit.contradiction,
            diff_capability: cap,
            diff_version: version,
            arbitration: identify.traces.clone(),
            evidence,
            fingerprint: 0,
        };
        profile.fingerprint = profile.recompute_fingerprint();
        Ok(profile)
    }

    /// 体检报告（判据"体检与审计"结构化面）。
    pub fn health(&self) -> VmHealth {
        let diff_ok = self.diff_table.audit_contiguity().is_ok();
        let ok = diff_ok
            && self.ledger.dropped() == 0
            && self.ledger.contradictions() == 0
            && self.step == PipelineStep::Completed;
        VmHealth {
            ledger_watermark: self.ledger.watermark(),
            ledger_cap: self.ledger.cap(),
            ledger_dropped: self.ledger.dropped(),
            contradiction_count: self.ledger.contradictions(),
            unknown_count: self.ledger.unknowns(),
            matrix_version: MATRIX_VERSION,
            wsl_slots_registered: self.wsl_reg.slot_registered(),
            wsl_slots_total: self.wsl_reg.slot_total(),
            wsl_slots_unknown: self.wsl_reg.unknown_slots(),
            container_slots_registered: self.container_reg.slot_registered(),
            container_slots_total: self.container_reg.slot_total(),
            container_slots_unknown: self.container_reg.unknown_slots(),
            diff_entries: self.diff_table.len(),
            diff_contiguous: diff_ok,
            pipeline_step: self.step,
            ok,
            note: if self.ledger.dropped() > 0 {
                "账本有丢弃：证据面不完整，体检降档"
            } else if self.ledger.contradictions() > 0 {
                "存在互斥矛盾：画像降级，需人工复核探测来源"
            } else if !diff_ok {
                "差异表断档：构造期缺陷，走登记流程补齐"
            } else {
                "体检通过：账目完整、矛盾为零、差异表连续"
            },
        }
    }
}

/// 体检报告（结构化；判据"体检与审计"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmHealth {
    /// 账本水位。
    pub ledger_watermark: usize,
    /// 账本容量。
    pub ledger_cap: usize,
    /// 丢弃事件数（>0 即体检不通过）。
    pub ledger_dropped: u64,
    /// 矛盾计数。
    pub contradiction_count: u64,
    /// Unknown/未决计数。
    pub unknown_count: u64,
    /// 矩阵版本。
    pub matrix_version: u32,
    /// WSL 登记水位。
    pub wsl_slots_registered: usize,
    /// WSL 总槽位。
    pub wsl_slots_total: usize,
    /// WSL 未决槽位（不造假补齐的显性水位）。
    pub wsl_slots_unknown: usize,
    /// 容器登记水位。
    pub container_slots_registered: usize,
    /// 容器总槽位。
    pub container_slots_total: usize,
    /// 容器未决槽位。
    pub container_slots_unknown: usize,
    /// 差异条目数。
    pub diff_entries: usize,
    /// 差异表连续性。
    pub diff_contiguous: bool,
    /// 流水游标。
    pub pipeline_step: PipelineStep,
    /// 体检总判。
    pub ok: bool,
    /// 体检注记。
    pub note: &'static str,
}

// ═════════════════════════════════════════════════════════════════════════════
// §14 · F1764 guestcap 画像投影（对接面：vck68 能力探测的画像入口）
// ═════════════════════════════════════════════════════════════════════════════

/// 投影地板位（guestcap 能力地板标志位；与 vck68 能力位面约定一致）。
pub const PROJ_FLAG_GPU_PASSTHROUGH: u32 = 1 << 0;
/// 投影位：半虚拟化通道可用（host-gpu 协商可发起）。
pub const PROJ_FLAG_PARAVIRT_CHANNEL: u32 = 1 << 1;
/// 投影位：WSLg dxg 直通可用。
pub const PROJ_FLAG_WSLG_DXG: u32 = 1 << 2;
/// 投影位：容器 GPU 注入面可用。
pub const PROJ_FLAG_CONTAINER_GPU: u32 = 1 << 3;
/// 投影位：环境未决（任何能力都不该被下游采信）。
pub const PROJ_FLAG_INDETERMINATE: u32 = 1 << 4;

/// 投影模式（guest 侧虚拟化感知模式，对齐 vck68 虚拟化模式闭集语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionMode {
    /// 裸金属宿主。
    BareMetalHost,
    /// hypervisor guest（标准虚拟化）。
    HypervisorGuest,
    /// 半虚拟化 guest（WSL2/dxg 直通类）。
    ParavirtGuest,
    /// 环境未决。
    IndeterminateEnv,
}

/// guestcap 画像投影（判据"差异复用家族"之外的对接面——与 F1764 的
/// guestcap 对接：本模块只出环境画像，guestcap 消费画像形成能力地板）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestCapProjection {
    /// 投影模式。
    pub mode: ProjectionMode,
    /// 能力地板标志位（保守地板——宣称的都是画像直接背书的）。
    pub floor_flags: u32,
    /// 验证态（永远 "Unverified"——诚实红线：本模块不出验证态，能力
    /// 验证是 F1764 探测/协商面的职责）。
    pub verification: &'static str,
    /// 投影注记。
    pub note: &'static str,
}

/// 画像 → guestcap 投影（纯函数；无副作用）。
///
/// 投影规则：
/// - Indeterminate → mode=IndeterminateEnv，地板只含 INDETERMINATE 位。
/// - Bare → mode=BareMetalHost，地板无虚拟化能力位（裸金属不需要）。
/// - Virtualized：HypervisorGuest 基线；矩阵 Supported → GPU_PASSTHROUGH
///   + PARAVIRT_CHANNEL；WSLg 门槛开 → 追加 WSLG_DXG 且升 ParavirtGuest；
///   容器族声称 → 追加 CONTAINER_GPU；互斥矛盾 → 降 IndeterminateEnv
///   （矛盾画像不放行任何能力——保守优先）。
pub fn project_to_guestcap(p: &EnvironmentProfile) -> GuestCapProjection {
    const VERIFY: &str = "Unverified"; // 诚实红线：本投影永远不带验证态
    if p.mutex_contradiction {
        return GuestCapProjection {
            mode: ProjectionMode::IndeterminateEnv,
            floor_flags: PROJ_FLAG_INDETERMINATE,
            verification: VERIFY,
            note: "互斥矛盾画像：全部能力不放行（保守优先）",
        };
    }
    match p.state {
        DetectionState::Indeterminate => GuestCapProjection {
            mode: ProjectionMode::IndeterminateEnv,
            floor_flags: PROJ_FLAG_INDETERMINATE,
            verification: VERIFY,
            note: "环境未决：下游禁用或按保守档消费",
        },
        DetectionState::Bare => GuestCapProjection {
            mode: ProjectionMode::BareMetalHost,
            floor_flags: 0,
            verification: VERIFY,
            note: "裸金属：无虚拟化能力位（本投影只管虚拟化面）",
        },
        DetectionState::Virtualized => {
            let mut flags = 0u32;
            let is_wslg = p.hv == HypervisorId::Wsl && p.wslg_gate_open;
            if p.matrix_support == Support3::Supported {
                flags |= PROJ_FLAG_GPU_PASSTHROUGH | PROJ_FLAG_PARAVIRT_CHANNEL;
            } else if p.matrix_support == Support3::Partial {
                flags |= PROJ_FLAG_PARAVIRT_CHANNEL; // 部分支持：只开协商通道不开直通
            }
            if is_wslg {
                flags |= PROJ_FLAG_WSLG_DXG;
            }
            if p.container_nvidia_claim || p.container_d3dkmt_claim {
                flags |= PROJ_FLAG_CONTAINER_GPU;
            }
            let mode = if is_wslg {
                ProjectionMode::ParavirtGuest
            } else {
                ProjectionMode::HypervisorGuest
            };
            GuestCapProjection {
                mode,
                floor_flags: flags,
                verification: VERIFY,
                note: "虚拟化画像投影：地板位仅由画像直接背书的证据开启",
            }
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §15 · 空表构造（差异表动态审计面：从零登记走全流程的入口）
// ═════════════════════════════════════════════════════════════════════════════

impl VersionDiffTable {
    /// 空差异表（动态登记/审计流程的完整入口；new() 的种子走的是快车道）。
    pub fn empty() -> Self {
        VersionDiffTable {
            entries: Vec::new(),
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §16 · 测试五组（识别表/矩阵/WSL/容器/差异——锚点判据闭集）+ 流水/指纹/
//       矛盾/账本/体检/投影（注入 CPUID，无旁路；panic 仅存在于测试失败面）
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试断言助手：Err 即 panic（测试失败面允许 panic；内核路径零 panic）。
    fn must<T>(r: Result<T, VmDetectError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("expected Ok, got: {}", e),
        }
    }

    /// 测试断言助手：期望特定错误变体。
    fn must_err<T>(r: Result<T, VmDetectError>, want: VmDetectError) {
        match r {
            Ok(_) => panic!("expected Err({:?}), got Ok", want),
            Err(e) => assert_eq!(e, want, "error code 0x{:04X}", e.code()),
        }
    }

    /// 差异查询断言助手：None 即 panic，Some 取能力代号。
    fn cap_of(e: Option<&DiffEntry>) -> &'static str {
        match e {
            Some(x) => x.capability,
            None => panic!("diff lookup miss"),
        }
    }

    /// 组装 WSL2 注入源："Microsoft Hv" 签名 + WSL2 hint 魔数。
    fn wsl2_injector() -> CpuidInjector {
        let mut inj = CpuidInjector::new();
        inj.set_signature("Microsoft Hv");
        inj.set_leaf(CPUID_HYPERVISOR_INFO_LEAF, 0, WSL2_HINT_MAGIC, 0, 0, 0);
        inj
    }

    /// 组装裸 Hyper-V 注入源："Microsoft Hv" 签名 + 无 WSL2 hint。
    fn hyperv_injector() -> CpuidInjector {
        let mut inj = CpuidInjector::new();
        inj.set_signature("Microsoft Hv");
        inj.set_leaf(CPUID_HYPERVISOR_INFO_LEAF, 0, 0x0000_0001, 0, 0, 0);
        inj
    }

    /// 五步流水通用驱动：注入源 + WSL 探测 + 容器探测 → (检测器, 画像)。
    fn run_pipeline_with<'a>(
        inj: &'a mut CpuidInjector,
        wsl_probes: &[(&'static str, Probe3)],
        container_probes: &[(&'static str, Probe3)],
    ) -> (VmDetector<'a>, Result<EnvironmentProfile, VmDetectError>) {
        let mut det = VmDetector::new(inj);
        must(det.step1_identify());
        must(det.step2_platform(GuestOs::Linux));
        let r3 = det.step3_wsl(wsl_probes);
        // 步三坏账（若在）先行显性回收：这里测试输入都是表内路径，应为 Ok。
        let _ = must(r3);
        let r4 = det.step4_container(container_probes);
        let _ = must(r4);
        let r5 = det.step5_diff(ver(5, 12));
        let _ = must(r5);
        must(det.finish());
        let p = det.profile();
        (det, p)
    }

    // ─────────────────────────────────────────────────────────────────────
    // 第一组 · 识别表
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn group1_identify_all_registered_families() {
        // 五家族逐个命中（签名来源见 SIG_TABLE_V1 注记）。
        let r = identify_from_cpuid(0, 0, 0);
        assert_eq!(r.state, DetectionState::Bare);
        assert_eq!(r.code, X_VM_OK);

        // KVM：sig_from_regs 组装路径（寄存器三元组 → 主键）。
        let ebx = u32::from_le_bytes(*b"KVMK");
        let ecx = u32::from_le_bytes(*b"VMKV");
        let edx = u32::from_le_bytes(*b"M\0\0\0");
        let (lo, hi) = sig_from_regs(ebx, ecx, edx);
        let r = identify_from_cpuid(hi, lo, 0);
        assert_eq!(r.state, DetectionState::Virtualized);
        assert_eq!(r.hv, HypervisorId::Kvm);
        assert_eq!(r.code, X_VM_OK);

        // ESXi：VMwareVMware（降档 70 但命中）。
        let mut inj = CpuidInjector::new();
        inj.set_signature("VMwareVMware");
        let mut det = VmDetector::new(&inj);
        let rep = must(det.step1_identify());
        assert_eq!(rep.hv, HypervisorId::Esxi);
        assert_eq!(rep.confidence, 70);

        // QEMU：TCGTCGTCGTCG。
        let mut inj = CpuidInjector::new();
        inj.set_signature("TCGTCGTCGTCG");
        let mut det = VmDetector::new(&inj);
        let rep = must(det.step1_identify());
        assert_eq!(rep.hv, HypervisorId::Qemu);
    }

    #[test]
    fn group1_identify_microsoft_arbitration() {
        // 裸 Hyper-V：签名命中两行（无条件行 + WSL2 hint 行），
        // 无 hint 时 hint 行不命中 → 裁决 HyperV，留痕只有 1 条。
        let inj = hyperv_injector();
        let mut det = VmDetector::new(&inj);
        let rep = must(det.step1_identify());
        assert_eq!(rep.hv, HypervisorId::HyperV);
        assert_eq!(rep.confidence, 60);
        assert_eq!(rep.traces.len(), 1);

        // WSL2：hint 命中 → Wsl 行置信 90 胜出；两条候选全量留痕（仲裁账）。
        let inj = wsl2_injector();
        let mut det = VmDetector::new(&inj);
        let rep = must(det.step1_identify());
        assert_eq!(rep.hv, HypervisorId::Wsl);
        assert_eq!(rep.confidence, 90);
        assert_eq!(rep.traces.len(), 2, "多签名候选必须全量留痕");
        let mut seen_hyper = false;
        let mut seen_wsl = false;
        let mut i = 0;
        while i < rep.traces.len() {
            if rep.traces[i].hv == HypervisorId::HyperV {
                seen_hyper = true;
            }
            if rep.traces[i].hv == HypervisorId::Wsl {
                seen_wsl = true;
                assert!(rep.traces[i].hint_used);
            }
            i += 1;
        }
        assert!(seen_hyper && seen_wsl);
    }

    #[test]
    fn group1_identify_unknown_and_foreign() {
        // 未入册签名 → Indeterminate + IdTableMiss（不是 Bare！三态区分）。
        let r = identify_from_cpuid(pack4(b"ZZZZ"), 0x1234_5678_9abc_def0, 0);
        assert_eq!(r.state, DetectionState::Indeterminate);
        assert_eq!(r.code, X_VM_ID_TABLE_MISS);

        // 已知但未入册家族（VirtualBox）→ Indeterminate + IdForeign。
        let mut inj = CpuidInjector::new();
        inj.set_signature("VBoxVBoxVBox");
        let mut det = VmDetector::new(&inj);
        let rep = must(det.step1_identify());
        assert_eq!(rep.state, DetectionState::Indeterminate);
        assert_eq!(rep.hv, HypervisorId::Unknown);
        assert_eq!(rep.code, X_VM_ID_FOREIGN);

        // 空注入源 → 全零签名 → Bare（显性结论）。
        let inj = CpuidInjector::new();
        let mut det = VmDetector::new(&inj);
        let rep = must(det.step1_identify());
        assert_eq!(rep.state, DetectionState::Bare);
        assert_eq!(rep.confidence, 100);
    }

    // ─────────────────────────────────────────────────────────────────────
    // 第二组 · 平台矩阵（含快照确定性往返）
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn group2_matrix_cells_and_bounds() {
        // v1 逐格抽查。
        let l = matrix_lookup(GuestOs::Linux, HypervisorId::Wsl);
        assert_eq!(l.support, Support3::Supported);
        assert_eq!(l.version, MATRIX_VERSION);
        let l = matrix_lookup(GuestOs::Windows, HypervisorId::Wsl);
        assert_eq!(l.support, Support3::Unsupported);
        let l = matrix_lookup(GuestOs::Windows, HypervisorId::HyperV);
        assert_eq!(l.support, Support3::Supported);
        let l = matrix_lookup(GuestOs::Linux, HypervisorId::Kvm);
        assert_eq!(l.support, Support3::Supported);
        let l = matrix_lookup(GuestOs::Windows, HypervisorId::Kvm);
        assert_eq!(l.support, Support3::Partial);

        // Unknown 家族 → 显性 Unknown 格带码（不猜）。
        let l = matrix_lookup(GuestOs::Linux, HypervisorId::Unknown);
        assert_eq!(l.support, Support3::Unknown);
        assert_eq!(l.code, X_VM_MATRIX_CELL_UNKNOWN);

        // 原始下标越界 → 显性 Unknown 带码。
        let l = matrix_lookup_raw(0, 9);
        assert_eq!(l.support, Support3::Unknown);
        assert_eq!(l.code, X_VM_MATRIX_CELL_UNKNOWN);
        let l = matrix_lookup_raw(7, 0);
        assert_eq!(l.support, Support3::Unknown);

        // 行视图：每 guest 五列齐。
        let row = matrix_row_for_guest(GuestOs::Linux);
        assert_eq!(row.len(), 5);
    }

    #[test]
    fn group2_matrix_snapshot_roundtrip() {
        // 快照确定性：两次快照逐行相同。
        let s1 = matrix_snapshot_lines();
        let s2 = matrix_snapshot_lines();
        assert_eq!(s1.len(), MATRIX_V1.len() + 1);
        let mut i = 0;
        while i < s1.len() {
            assert_eq!(s1[i], s2[i], "快照必须逐行确定");
            i += 1;
        }
        // 往返：解析回读 → 指纹与 const 本体一致。
        let fp = must(parse_matrix_snapshot(&s1));
        assert_eq!(fp, matrix_fingerprint());
        // 篡改行 → BadInput。
        let mut bad = s1.clone();
        bad[3] = String::from("W|HyperV|N|篡改注记");
        must_err(parse_matrix_snapshot(&bad), VmDetectError::BadInput);
        // 行数错 → BadInput。
        let short: Vec<String> = s1[..3].to_vec();
        must_err(parse_matrix_snapshot(&short), VmDetectError::BadInput);
        // 头部版本不符 → MatrixVersionMismatch。
        let mut ver_bad = s1.clone();
        ver_bad[0] = String::from("VX-VMDETECT-MATRIX|v99");
        must_err(
            parse_matrix_snapshot(&ver_bad),
            VmDetectError::MatrixVersionMismatch,
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // 第三组 · WSL 登记
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn group3_wsl_register_discipline() {
        let mut reg = WslPathRegistry::new();
        assert_eq!(reg.slot_total(), WSL_PATH_TABLE.len());
        // 正常登记。
        let s = must(reg.register("/dev/dxg", Probe3::Present));
        assert_eq!(reg.state_of("/dev/dxg"), Probe3::Present);
        // 重复注册 → 拒绝带码（探测结果不可静默覆盖）。
        must_err(
            reg.register("/dev/dxg", Probe3::Missing),
            VmDetectError::WslPathConflict,
        );
        // 覆盖未发生：仍是最初登记值。
        assert_eq!(reg.state_of("/dev/dxg"), Probe3::Present);
        // 表外路径 → 拒绝带码。
        must_err(
            reg.register("/dev/fake", Probe3::Present),
            VmDetectError::WslPathConflict,
        );
        // 空路径 → BadInput（表内没有空路径，此分支防御未来表演进）。
        must_err(
            reg.register("", Probe3::Present),
            VmDetectError::WslPathConflict,
        );
        // 水位。
        assert_eq!(reg.slot_registered(), 1);
        assert_eq!(reg.unknown_slots(), WSL_PATH_TABLE.len() - 1);
        let _ = s;
    }

    #[test]
    fn group3_wslg_gate_rules() {
        // 缺 WSL 签名（hyperv 家族）+ dxg Present → 关门。
        let mut reg = WslPathRegistry::new();
        must(reg.register("/dev/dxg", Probe3::Present));
        let g = wslg_gate(HypervisorId::HyperV, &reg);
        assert!(!g.gate_open);
        assert_eq!(g.code, X_VM_WSLG_GATE_UNMET);

        // WSL 签名 + dxg Missing（实测不存在）→ 关门，缺席清单非空。
        let mut reg = WslPathRegistry::new();
        must(reg.register("/dev/dxg", Probe3::Missing));
        let g = wslg_gate(HypervisorId::Wsl, &reg);
        assert!(!g.gate_open);
        assert_eq!(g.dxg, Probe3::Missing);
        assert!(!g.missing_required.is_empty());

        // WSL 签名 + dxg Unknown（没探测过）→ 关门（不造假补齐）。
        let reg = WslPathRegistry::new();
        let g = wslg_gate(HypervisorId::Wsl, &reg);
        assert!(!g.gate_open);
        assert_eq!(g.dxg, Probe3::Unknown);

        // WSL 签名 + dxg Present → 开门（判据：两条件齐才开）。
        let mut reg = WslPathRegistry::new();
        must(reg.register("/dev/dxg", Probe3::Present));
        let g = wslg_gate(HypervisorId::Wsl, &reg);
        assert!(g.gate_open);
        assert_eq!(g.code, X_VM_OK);
        assert!(g.missing_required.is_empty());
    }

    // ─────────────────────────────────────────────────────────────────────
    // 第四组 · 容器登记
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn group4_container_register_and_mutex() {
        let mut reg = ContainerPathRegistry::new();
        assert_eq!(reg.slot_total(), CONTAINER_PATH_TABLE.len());
        must(reg.register("/usr/bin/nvidia-container-runtime", Probe3::Present));
        must_err(
            reg.register("/usr/bin/nvidia-container-runtime", Probe3::Missing),
            VmDetectError::ContainerPathConflict,
        );
        must_err(
            reg.register("D3DKMT:not-a-real-path", Probe3::Present),
            VmDetectError::ContainerPathConflict,
        );
        // 族声称：Nvidia 族 Present → 声称；D3DKMT 未登记 → 不声称。
        assert!(reg.family_claimed(ContainerFamily::NvidiaContainer));
        assert!(!reg.family_claimed(ContainerFamily::D3dKmt));

        // 互斥：WSLg 开门 + nvidia 声称 → 矛盾。
        let mut wreg = WslPathRegistry::new();
        must(wreg.register("/dev/dxg", Probe3::Present));
        let g = wslg_gate(HypervisorId::Wsl, &wreg);
        assert!(g.gate_open);
        let a = check_wsl_container_mutex(&reg, &g);
        assert!(a.contradiction);
        assert_eq!(a.code, X_VM_WSL_CONTAINER_MUTEX);

        // 互斥：WSLg 关门 + nvidia 声称 → 无矛盾。
        let reg2 = ContainerPathRegistry::new();
        let a = check_wsl_container_mutex(&reg2, &g);
        assert!(!a.contradiction);
        assert_eq!(a.code, X_VM_OK);
    }

    // ─────────────────────────────────────────────────────────────────────
    // 第五组 · 版本差异
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn group5_diff_seed_audit_and_lookup() {
        // 种子 + QEMU 动态条目 → 全家族连续无断档。
        let t = VersionDiffTable::new();
        must(t.audit_contiguity());
        assert!(t.len() >= DIFF_TABLE_V1.len() + 1);

        // 精确命中：WSL2 内核 5.12 ∈ [5.10,5.15) → dxg-f120。
        let e = t.lookup(DiffFamily::Wsl, ver(5, 12));
        assert!(e.is_some());
        assert_eq!(cap_of(e), "dxg-f120");

        // 差异复用家族：主版本边界（6.0 → dxg-f122 区间起点精确命中）。
        assert_eq!(ver_major(ver(6, 0)), 6);
        let e = t.lookup_family(DiffFamily::Wsl, ver(6, 0));
        assert_eq!(cap_of(e), "dxg-f122");
        // 复用回退面：归并键（主版本打包）在连续表下与精确命中同区间——
        // 断档表里回退才有独立语义（见 registration discipline 组）。
        let e = t.lookup_family(DiffFamily::Wsl, ver(5, 14));
        assert_eq!(cap_of(e), "dxg-f120");

        // 家族条目计数。
        assert_eq!(t.family_entry_count(DiffFamily::HyperV), 3);
        assert_eq!(t.family_entry_count(DiffFamily::Qemu), 1);
    }

    #[test]
    fn group5_diff_registration_discipline() {
        let mut t = VersionDiffTable::empty();
        // 空表审计 → 断档（家族无覆盖）。
        must_err(t.audit_contiguity(), VmDetectError::DiffRangeGap);
        // 区间倒置 → BadInput。
        must_err(
            t.register(DiffFamily::Kvm, ver(5, 0), 0, "x", "y"),
            VmDetectError::BadInput,
        );
        // 空能力键 → BadInput。
        must_err(
            t.register(DiffFamily::Kvm, 0, VER_MAX, "", "y"),
            VmDetectError::BadInput,
        );
        // 首段登记 + 补齐 → 审计通过（审计是全家族闭集：其余家族先补全量段）。
        must(t.register(DiffFamily::Kvm, 0, ver(5, 0), "a", "a 段"));
        must_err(t.audit_contiguity(), VmDetectError::DiffRangeGap); // Kvm 尾部断档
        must(t.register(DiffFamily::Kvm, ver(5, 0), VER_MAX, "b", "b 段"));
        // 其余四族补全量段（动态登记路径——与 new() 种子等价）。
        must(t.register(DiffFamily::HyperV, 0, VER_MAX, "hv", "hv 全段"));
        must(t.register(DiffFamily::Esxi, 0, VER_MAX, "esx", "esx 全段"));
        must(t.register(DiffFamily::Qemu, 0, VER_MAX, "q", "q 全段"));
        must(t.register(DiffFamily::Wsl, 0, VER_MAX, "wsl", "wsl 全段"));
        must(t.audit_contiguity());
        // 重叠登记 → 拒绝带码。
        must_err(
            t.register(DiffFamily::Kvm, ver(4, 0), ver(6, 0), "c", "c 段"),
            VmDetectError::DiffRangeOverlap,
        );
        // 断档登记（跳过 [5.0) 之后的连续性）→ 审计 Gap。
        let mut t2 = VersionDiffTable::empty();
        must(t2.register(DiffFamily::Kvm, 0, ver(1, 0), "a", "a 段"));
        must(t2.register(DiffFamily::Kvm, ver(2, 0), VER_MAX, "b", "b 段"));
        must_err(t2.audit_contiguity(), VmDetectError::DiffRangeGap);
        // 命中查询与复用家族查询。
        assert_eq!(
            cap_of(t.lookup(DiffFamily::Kvm, ver(4, 255))),
            "a"
        );
        assert_eq!(
            cap_of(t.lookup_family(DiffFamily::Kvm, ver(5, 7))),
            "b"
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // 流水编排 · 指纹 · 矛盾 · 账本 · 体检 · 投影
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn pipeline_wslg_happy_path_fingerprint() {
        let mut inj = wsl2_injector();
        let (mut det, rp) = run_pipeline_with(
            &mut inj,
            &[("/dev/dxg", Probe3::Present)],
            &[],
        );
        let p = must(rp);
        // 步一证据：WSL2 裁决。
        assert_eq!(p.state, DetectionState::Virtualized);
        assert_eq!(p.hv, HypervisorId::Wsl);
        assert_eq!(p.confidence_band, Confidence3::High);
        // 步二证据：Linux × WSL → Supported。
        assert_eq!(p.matrix_support, Support3::Supported);
        // 步三证据：WSLg 门槛开。
        assert!(p.wslg_gate_open);
        assert_eq!(p.wslg_code, X_VM_OK);
        // 步四证据：无容器声称、无矛盾。
        assert!(!p.container_nvidia_claim);
        assert!(!p.mutex_contradiction);
        // 步五证据：差异复用家族命中（5.12 → dxg-f120）。
        assert_eq!(p.diff_capability, "dxg-f120");
        assert_eq!(p.diff_version, ver(5, 12));
        // 仲裁留痕进入画像。
        assert_eq!(p.arbitration.len(), 2);
        // 指纹确定性：同输入重建流水 → 同指纹。
        let mut inj2 = wsl2_injector();
        let (_, rp2) = run_pipeline_with(
            &mut inj2,
            &[("/dev/dxg", Probe3::Present)],
            &[],
        );
        let p2 = must(rp2);
        assert_eq!(p.fingerprint, p2.fingerprint, "同输入必须同指纹");
        assert_eq!(p.fingerprint, p.recompute_fingerprint(), "指纹必须可复算");
        // 证据行固定五条、前缀固定序。
        assert_eq!(p.evidence.len(), 5);
        assert!(p.evidence[0].starts_with("step1:"));
        assert!(p.evidence[4].starts_with("step5:"));
        // 快照行确定性 + 指纹行在案。
        let s = p.snapshot_lines();
        assert!(s[0].starts_with("VX-VMDETECT-PROFILE|"));
        // 乱序调用在收口后仍拒绝（回退到步一）。
        must_err(det.step1_identify(), VmDetectError::StepOutOfOrder);
    }

    #[test]
    fn pipeline_bare_and_projection() {
        // 空注入源：Bare 全流水（Bare 是结论不是失败——流水照走五步）。
        let inj = CpuidInjector::new();
        let mut det = VmDetector::new(&inj);
        let rep = must(det.step1_identify());
        assert_eq!(rep.state, DetectionState::Bare);
        must(det.step2_platform(GuestOs::Windows));
        must(det.step3_wsl(&[]));
        must(det.step4_container(&[]));
        must(det.step5_diff(ver(0, 0)));
        must(det.finish());
        let p = must(det.profile());
        assert_eq!(p.state, DetectionState::Bare);
        // Unknown 家族 × 矩阵 → 显性 Unknown 格。
        assert_eq!(p.matrix_support, Support3::Unknown);
        // 差异：Unknown 家族 → unknown-family 显性落点。
        assert_eq!(p.diff_capability, "unknown-family");
        // 投影：BareMetalHost，地板零位。
        let proj = project_to_guestcap(&p);
        assert_eq!(proj.mode, ProjectionMode::BareMetalHost);
        assert_eq!(proj.floor_flags, 0);
        assert_eq!(proj.verification, "Unverified");
        // 体检：Bare 流水无矛盾无丢弃 → ok。
        let h = det.health();
        assert!(h.ok, "health note: {}", h.note);
        assert!(h.diff_contiguous);
    }

    #[test]
    fn pipeline_out_of_order_rejected_with_code() {
        let inj = wsl2_injector();
        let mut det = VmDetector::new(&inj);
        // 未起步就步二 → 拒绝带码。
        must_err(
            det.step2_platform(GuestOs::Linux),
            VmDetectError::StepOutOfOrder,
        );
        // 步一后跳步四 → 拒绝带码。
        must(det.step1_identify());
        must_err(
            det.step4_container(&[]),
            VmDetectError::StepOutOfOrder,
        );
        // 乱序拒绝全部入账（可追溯；含 new() 的差异表种子审计事件）。
        assert_eq!(det.ledger().len(), 4);
        assert_eq!(det.ledger()[1].code, X_VM_STEP_OUT_OF_ORDER);
        assert_eq!(det.ledger()[2].code, X_VM_OK); // 被拒后步一仍正常入账
        assert_eq!(det.ledger()[3].code, X_VM_STEP_OUT_OF_ORDER);
        // 五步未完取画像 → ProfileNotReady。
        must_err(det.profile(), VmDetectError::ProfileNotReady);
    }

    #[test]
    fn pipeline_contradiction_recorded_and_projected_down() {
        // 矛盾注入：WSLg 全开 + nvidia-container 声称 → 矛盾入账不裁决。
        let mut inj = wsl2_injector();
        let (mut det, rp) = run_pipeline_with(
            &mut inj,
            &[("/dev/dxg", Probe3::Present)],
            &[("/usr/bin/nvidia-container-runtime", Probe3::Present)],
        );
        let p = must(rp);
        assert!(p.mutex_contradiction);
        assert!(p.container_nvidia_claim);
        // 投影：矛盾画像全部不放行（保守优先）。
        let proj = project_to_guestcap(&p);
        assert_eq!(proj.mode, ProjectionMode::IndeterminateEnv);
        assert_eq!(proj.floor_flags, PROJ_FLAG_INDETERMINATE);
        // 体检：矛盾计数为 1，ok 判否。
        let h = det.health();
        assert_eq!(h.contradiction_count, 1);
        assert!(!h.ok);
        assert!(h.note.contains("矛盾"));
    }

    #[test]
    fn pipeline_unknown_family_diff_explicit() {
        // 未入册家族（VirtualBox）走全流水：差异步给显性 unknown-family。
        let mut inj = CpuidInjector::new();
        inj.set_signature("VBoxVBoxVBox");
        let mut det = VmDetector::new(&inj);
        must(det.step1_identify());
        must(det.step2_platform(GuestOs::Linux));
        must(det.step3_wsl(&[]));
        must(det.step4_container(&[]));
        let (cap, _) = must(det.step5_diff(ver(7, 0)));
        assert_eq!(cap, "unknown-family");
        must(det.finish());
        let p = must(det.profile());
        // 投影：IndeterminateEnv（家族未入册 → 环境未决）。
        let proj = project_to_guestcap(&p);
        assert_eq!(proj.mode, ProjectionMode::IndeterminateEnv);
        // Unknown 家族矩阵格带码。
        assert_eq!(p.matrix_support, Support3::Unknown);
    }

    #[test]
    fn ledger_overflow_saturates_and_drops() {
        // 定长账溢出：丢弃计数饱和递增，不 panic 不回绕。
        let mut led = DetectionLedger::with_cap(2);
        assert_eq!(led.note(1, X_VM_OK, "a", 0), X_VM_OK);
        assert_eq!(led.note(2, X_VM_OK, "b", 0), X_VM_OK);
        assert_eq!(led.note(3, X_VM_OK, "c", 0), X_VM_LEDGER_FULL);
        assert_eq!(led.note(4, X_VM_OK, "d", 0), X_VM_LEDGER_FULL);
        assert_eq!(led.watermark(), 2);
        assert_eq!(led.total(), 4);
        assert_eq!(led.dropped(), 2);
        // 矛盾/Unknown 计数独立累计。
        assert_eq!(led.note(5, X_VM_WSL_CONTAINER_MUTEX, "e", 0), X_VM_LEDGER_FULL);
        assert_eq!(led.contradictions(), 1);
        assert_eq!(
            led.note(6, X_VM_ID_TABLE_MISS, "f", 0),
            X_VM_LEDGER_FULL
        );
        assert_eq!(led.unknowns(), 1);
        // 大容量账本：LEDGER_CAP 默认值。
        let led2 = DetectionLedger::new();
        assert_eq!(led2.cap(), LEDGER_CAP);
    }

    #[test]
    fn error_details_actionable_and_coded() {
        // 全错误变体：码落在 0x12B0 段、detail 非空中文诊断。
        let all = [
            VmDetectError::IdTableMiss,
            VmDetectError::IdForeign,
            VmDetectError::MatrixVersionMismatch,
            VmDetectError::MatrixCellUnknown,
            VmDetectError::WslPathConflict,
            VmDetectError::WslgGateUnmet,
            VmDetectError::ContainerPathConflict,
            VmDetectError::WslContainerMutex,
            VmDetectError::DiffRangeOverlap,
            VmDetectError::DiffRangeGap,
            VmDetectError::DiffUnknownFamily,
            VmDetectError::StepOutOfOrder,
            VmDetectError::LedgerFull,
            VmDetectError::ProfileNotReady,
            VmDetectError::BadInput,
        ];
        let mut i = 0;
        while i < all.len() {
            let c = all[i].code();
            assert!(
                (X_VM_ID_TABLE_MISS..=X_VM_BAD_INPUT).contains(&c),
                "code 0x{:04X} 越段",
                c
            );
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        // 码两两不同（十六码独占语义）。
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
    fn host_source_reliability_matches_arch() {
        // 真实源：x86_64 可靠；其他架构显性不可靠（Indeterminate，不冒充 Bare）。
        let src = HostCpuidSource::new();
        assert_eq!(src.reliable(), cfg!(target_arch = "x86_64"));
        // 注入源永远可靠（测试面契约）。
        let inj = CpuidInjector::new();
        assert!(inj.reliable());
    }

    #[test]
    fn health_watermarks_after_full_pipeline() {
        let mut inj = wsl2_injector();
        let (mut det, rp) = run_pipeline_with(
            &mut inj,
            &[
                ("/dev/dxg", Probe3::Present),
                ("/dev/nvidia0", Probe3::Unknown), // 未探测面保持 Unknown 入账
            ],
            &[(CONTAINER_PATH_TABLE[0].path, Probe3::Missing)],
        );
        let _ = must(rp);
        let h = det.health();
        assert_eq!(h.matrix_version, MATRIX_VERSION);
        assert_eq!(h.wsl_slots_registered, 2);
        assert_eq!(h.wsl_slots_total, WSL_PATH_TABLE.len());
        assert_eq!(h.container_slots_registered, 1);
        assert_eq!(h.container_slots_total, CONTAINER_PATH_TABLE.len());
        assert!(h.diff_contiguous);
        // 注：本画像 dxg Present 而 nvidia Missing，不构成互斥矛盾。
        assert_eq!(h.contradiction_count, 0);
        assert!(h.ok);
    }
}
