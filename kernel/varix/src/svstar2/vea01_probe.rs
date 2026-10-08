//! VE-F0001 · 虚拟显卡探测仲裁器（VE-A 域 · 内核图形抽象层 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0001`
//!
//! **判据（锚点原文）**：三类识别、虚拟化声明、能力诚实、回退软渲、判据；
//! 探测含驱动版本指纹（同 GPU 不同驱动版本能力差异显式区分）；仲裁含多 GPU
//! 拓扑记录（哪张卡在哪条总线上可查）；探测结果与 V01 协商的口径对齐断言；
//! 虚拟显卡的能力虚报检测含已知虚拟化平台特征库（VMware/HyperV/KVM 各自的
//! 报文特征在册）。
//!
//! **设计要点**：
//! - 三类识别 = 物理 GPU / 虚拟 GPU / 软渲染。第四类 `Unknown` 不是第四类设备，
//!   而是"三类都没认出来"的**显式失败态**——规格要求"探测失败→软渲染回退"，
//!   所以 Unknown 必须可判别，不能塞进软渲染里冒充识别成功；
//! - 虚拟化平台识别靠**在册特征库**逐条匹配并累加证据权重。强证据
//!   （CPUID 品牌串 / backdoor 端口）一票定案；弱证据需多条叠加；
//! - 能力诚实：驱动自报强度与实测强度交叉比对，差值超阈值判虚报。
//!   **虚报只标记降级不放行**——直接拒绝会让画面全屏，实测中虚报极普遍；
//! - 驱动版本指纹 + 能力指纹 + 报告指纹三级，同机同果 ⇒ 同指纹，
//!   换驱动版本 ⇒ 指纹变（规格点名"同 GPU 不同驱动版本能力差异显式区分"）；
//! - 仲裁规则逐条留痕可审计，冲突按四级稳定决胜，**绝不随机**——
//!   同机同解是"可追溯"的前提；
//! - V01 口径对齐断言 A1~A5 是与显示协商侧的**契约**，不通过就拒绝交付
//!   半真报告（宁可早崩，不让上层拿到自相矛盾的结论）。
//!
//! **落位纪律**：VE 全部活在用户态服务（锚点铁律 6：内核态只留合成裁决与安全），
//! 本模块属 `svstar2`（service star II），不侵入内核态。
//!
//! 确定性算法、零 IO、可序列化：同样的输入必得同样的输出。时间用逻辑 tick
//! 注入，不用墙钟——保证回归可复现、对拍可重现。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、三类识别（判据 1）
// ---------------------------------------------------------------------------

/// 适配器类别。第四类 `Unknown` 是显式失败态，不是设备类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterClass {
    /// 物理 GPU
    Physical,
    /// 虚拟 GPU
    Virtual,
    /// 软渲染（llvmpipe / WARP / GDI Generic 等，无硬件）
    Software,
    /// 三类证据均未命中——显式失败
    Unknown,
}

impl AdapterClass {
    /// 人话标签（读屏与日志用）。
    pub fn label(self) -> &'static str {
        match self {
            AdapterClass::Physical => "物理显卡",
            AdapterClass::Virtual => "虚拟显卡",
            AdapterClass::Software => "软件渲染",
            AdapterClass::Unknown => "未知设备",
        }
    }
}

/// 虚拟化平台在册档案。规格判据点名 VMware / HyperV / KVM 三家。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hypervisor {
    VMware,
    HyperV,
    Kvm,
    Xen,
    Parallels,
    VirtualBox,
    Qemu,
    None,
}

impl Hypervisor {
    pub fn label(self) -> &'static str {
        match self {
            Hypervisor::VMware => "VMware",
            Hypervisor::HyperV => "Hyper-V",
            Hypervisor::Kvm => "KVM",
            Hypervisor::Xen => "Xen",
            Hypervisor::Parallels => "Parallels",
            Hypervisor::VirtualBox => "VirtualBox",
            Hypervisor::Qemu => "QEMU",
            Hypervisor::None => "无虚拟化",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、能力诚实（判据 3）
// ---------------------------------------------------------------------------

/// 能力申报的可信等级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityTrust {
    /// 厂商 ID + 驱动版本 + 实测三者一致
    Verified,
    /// 驱动自报，未交叉验证
    Reported,
    /// 检测到虚报（已标记降级）
    Inflated,
    /// 驱动没报，靠降级推定
    Unknown,
}

/// 单条能力申报记录——能力虚报检测的最小取证单元。
#[derive(Clone, Debug)]
pub struct CapabilityClaim {
    /// 能力键
    pub key: &'static str,
    /// 驱动自报强度 0.0~1.0
    pub claimed: f32,
    /// 实测强度；`None` = 无实测
    pub measured: Option<f32>,
    /// 虚报判定
    pub trust: CapabilityTrust,
    /// 虚报取证：凭什么判定它虚报
    pub evidence: &'static str,
}

// ---------------------------------------------------------------------------
// 三、探测输入与结果
// ---------------------------------------------------------------------------

/// 上游总线类型。多 GPU 拓扑记录的一环。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusKind {
    Pcie,
    Virtio,
    Usb4,
    Platform,
    Unknown,
}

impl BusKind {
    pub fn label(self) -> &'static str {
        match self {
            BusKind::Pcie => "PCIe",
            BusKind::Virtio => "virtio",
            BusKind::Usb4 => "USB4",
            BusKind::Platform => "platform",
            BusKind::Unknown => "未知总线",
        }
    }

    /// 总线拓扑深度（0 = 挂在 CPU 直连根总线上）。
    pub fn depth(self) -> u32 {
        match self {
            BusKind::Virtio | BusKind::Platform => 0,
            BusKind::Pcie => 1,
            BusKind::Usb4 => 3,
            BusKind::Unknown => 9,
        }
    }

    /// 上游设备标识；根设备为 `None`。
    pub fn parent(self) -> Option<&'static str> {
        match self {
            BusKind::Pcie | BusKind::Platform | BusKind::Unknown => None,
            BusKind::Virtio => Some("virtio-pci-backend"),
            BusKind::Usb4 => Some("usb4-host-router"),
        }
    }
}

/// 探测引擎输入——来自总线枚举的一手资料。全部可序列化，探测本身零 IO。
#[derive(Clone, Debug)]
pub struct AdapterProbeInput {
    /// PCI 槽位地址，如 "0000:01:00.0"
    pub slot: String,
    pub bus: BusKind,
    /// 厂商 ID；0 = 未读到
    pub vendor_id: u32,
    /// 设备 ID
    pub device_id: u32,
    /// 设备名（驱动自报）
    pub name: String,
    /// 驱动版本全串——规格要求同卡不同驱动显式区分
    pub driver_version: String,
    /// 驱动暴露的能力表（键值对，值 0.0~1.0）
    pub caps: Vec<(&'static str, f32)>,
    /// 交叉验证实测（无则空表）
    pub measured_caps: Vec<(&'static str, f32)>,
    /// 探测时抓到的原始报文特征串
    pub raw_signatures: Vec<String>,
    /// 独占显存字节数；0 = 共享内存
    pub dedicated_memory_bytes: u64,
    /// 是否允许直通
    pub passthrough_capable: bool,
}

impl AdapterProbeInput {
    /// 构造一个物理 GPU 输入（测试与对拍基准用）。
    pub fn physical(slot: &str, name: &str, driver: &str) -> Self {
        AdapterProbeInput {
            slot: slot.to_string(),
            bus: BusKind::Pcie,
            vendor_id: 0x10DE,
            device_id: 0x2484,
            name: name.to_string(),
            driver_version: driver.to_string(),
            caps: vec![("compute", 1.0), ("hdr", 0.9)],
            measured_caps: vec![("compute", 1.0), ("hdr", 0.88)],
            raw_signatures: vec!["pcie".to_string(), "nv".to_string()],
            dedicated_memory_bytes: 8 * 1024 * 1024 * 1024,
            passthrough_capable: true,
        }
    }

    /// 构造一个 KVM 虚拟显卡输入。
    pub fn virtual_kvm(slot: &str) -> Self {
        AdapterProbeInput {
            slot: slot.to_string(),
            bus: BusKind::Virtio,
            vendor_id: 0x1AF4,
            device_id: 0x1050,
            name: "virtio-gpu".to_string(),
            driver_version: "virtio-1.0".to_string(),
            caps: vec![("compute", 0.3)],
            measured_caps: vec![("compute", 0.3)],
            raw_signatures: vec![
                "KVMKVMKVM".to_string(),
                "virtio-gpu".to_string(),
                "0x1af4".to_string(),
            ],
            dedicated_memory_bytes: 0,
            passthrough_capable: false,
        }
    }

    /// 构造一个软件渲染输入。
    pub fn software(slot: &str, name: &str) -> Self {
        AdapterProbeInput {
            slot: slot.to_string(),
            bus: BusKind::Unknown,
            vendor_id: 0x1234,
            device_id: 0x1111,
            name: name.to_string(),
            driver_version: "0.1".to_string(),
            caps: vec![],
            measured_caps: vec![],
            raw_signatures: vec!["bochs".to_string()],
            dedicated_memory_bytes: 0,
            passthrough_capable: false,
        }
    }

    /// 查能力表。`pub(crate)`：`vea01_engine::judge_capabilities` 跨模块调用，
    /// 但不扩公开面（内核外部无需直接读单条能力）。
    pub(crate) fn cap(&self, key: &str) -> Option<f32> {
        self.caps.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }

    /// 查实测表。跨模块用，同 `cap`。
    pub(crate) fn measured(&self, key: &str) -> Option<f32> {
        self.measured_caps
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
    }
}

/// 拓扑记录——规格判据点名项：多 GPU 拓扑记录可查。
#[derive(Clone, Debug)]
pub struct AdapterTopology {
    pub slot: String,
    pub bus: BusKind,
    pub depth: u32,
    pub parent_slot: Option<&'static str>,
    /// 同一物理封装内的其他 slot（多屏单卡场景）
    pub sibling_slots: Vec<String>,
}

/// 单个适配器的完整探测结论。
#[derive(Clone, Debug)]
pub struct AdapterProbeResult {
    pub slot: String,
    pub bus: BusKind,
    pub adapter_class: AdapterClass,
    pub hypervisor: Hypervisor,
    /// 命中虚拟化特征库的具体特征键
    pub hypervisor_signatures: Vec<&'static str>,
    /// 能力诚实记录，逐条
    pub capabilities: Vec<CapabilityClaim>,
    /// 综合可信度 0.0~1.0
    pub trust_score: f32,
    /// 是否存在虚报能力
    pub inflated: bool,
    /// 驱动版本指纹——同卡不同驱动 ⇒ 不同指纹
    pub driver_fingerprint: u64,
    /// 能力摘要指纹
    pub capability_fingerprint: u64,
    /// 是否降级
    pub degraded: bool,
    /// 降级原因链（人话）
    pub degrade_reasons: Vec<String>,
    pub topology: AdapterTopology,
    /// 探测耗时（确定性伪测量，按工作量模型算，不用真实时钟）
    pub probe_cost_us: u32,
}

// ---------------------------------------------------------------------------
// 指纹：同机同果可追溯（规格原文点名）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位。选它不选 MD5/SHA：规格只要"同果同指纹"的等价类划分，
/// 不要求密码学强度；而探测在启动热路径上跑，必须快且零依赖。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 能力表规范化后散列——键排序，保证与插入顺序无关。
/// 跨模块用（`vea01_engine::probe_adapter` 算能力指纹），故公开。
pub fn canon_caps_hash(caps: &[(&'static str, f32)]) -> u64 {
    let mut keys: Vec<&str> = caps.iter().map(|(k, _)| *k).collect();
    keys.sort_unstable();
    let mut buf: Vec<u8> = Vec::new();
    for k in keys {
        buf.extend_from_slice(k.as_bytes());
        buf.push(b'=');
        let v = caps.iter().find(|(kk, _)| *kk == k).map(|(_, v)| *v).unwrap_or(0.0);
        buf.extend_from_slice(format!("{:.3}", v).as_bytes());
        buf.push(b',');
    }
    fnv1a64(&buf)
}
