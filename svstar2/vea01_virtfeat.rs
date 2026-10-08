//! VE-F0001 续 · 虚拟化平台特征库（判据：VMware/HyperV/KVM 报文特征在册）
//!
//! 纯数据 + 纯匹配，零 IO、零时钟、零随机。每条特征带出处说明，
//! 便于日后对拍时逐条复核（规格对拍红线：参考实现 vs VE 输出）。

use super::vea01_probe::Hypervisor;

/// 单条在册特征。
pub struct HypervisorSignature {
    /// 特征键，用于取证留痕
    pub key: &'static str,
    /// 报文特征串。与探测输入的 raw_signatures 做包含匹配
    pub pattern: &'static str,
    /// 该特征指向的平台
    pub platform: Hypervisor,
    /// 出处：为什么这个串能证明平台身份
    pub source: &'static str,
    /// 证据权重 0.0~1.0。强指纹权重高，弱指纹需多条叠加
    pub weight: f32,
}

/// 平台定案所需的总证据权重门槛。
pub const HYPERVISOR_VERDICT_FLOOR: f32 = 0.50;

/// 强证据线：达到即一票定案。
pub const HYPERVISOR_STRONG_EVIDENCE: f32 = 0.85;

/// 已知虚拟化平台特征库（在册）。
///
/// 覆盖规格点名的三家（VMware / HyperV / KVM），另附 Xen / Parallels /
/// VirtualBox / QEMU——多认几家用不上，但漏认一家会让那台机器退到软渲染白屏。
pub const HYPERVISOR_SIGNATURES: &[HypervisorSignature] = &[
    // ---------- VMware（规格判据点名） ----------
    HypervisorSignature {
        key: "vmware-svga-name",
        pattern: "VMware SVGA 3D",
        platform: Hypervisor::VMware,
        source: "VMware Tools 安装的 SVGA 驱动名，Workstation/Fusion 桌面虚拟机的确定标志",
        weight: 0.55,
    },
    HypervisorSignature {
        key: "vmware-vmmouse",
        pattern: "VMware Virtual",
        platform: Hypervisor::VMware,
        source: "虚拟 PCI 设备名含 \"VMware Virtual\"，VMware-tools 装的虚拟硬件族",
        weight: 0.50,
    },
    HypervisorSignature {
        key: "vmware-pci-vendor",
        pattern: "0x15ad",
        platform: Hypervisor::VMware,
        source: "PCI vendor ID 0x15AD = VMware Inc.，SVGA/网卡/存储全族共用",
        weight: 0.75,
    },
    HypervisorSignature {
        key: "vmware-backdoor-port",
        pattern: "VMXH",
        platform: Hypervisor::VMware,
        source: "VMware backdoor I/O 端口标识 IOH，Guest 内核用它上抛宿主调用",
        weight: 0.90,
    },
    // ---------- Hyper-V（规格判据点名） ----------
    HypervisorSignature {
        key: "hyperv-dxgi-adapter",
        pattern: "Microsoft Basic Render Driver",
        platform: Hypervisor::HyperV,
        source: "Hyper-V 合成显示适配器的 DXGI 适配器名，Hyper-V 客户端与 RDP 会话共用",
        weight: 0.50,
    },
    HypervisorSignature {
        key: "hyperv-synthetic",
        pattern: "Hyper-V",
        platform: Hypervisor::HyperV,
        source: "Hyper-V 合成设备族名前缀，vid_1414(MSFT) 下的设备全族",
        weight: 0.45,
    },
    HypervisorSignature {
        key: "hyperv-pci-vendor",
        pattern: "0x1414",
        platform: Hypervisor::HyperV,
        source: "PCI vendor ID 0x1414 = Microsoft，Hyper-V 合成设备的标准归属",
        weight: 0.70,
    },
    HypervisorSignature {
        key: "hyperv-tlfs",
        pattern: "TLFS",
        platform: Hypervisor::HyperV,
        source: "Hypervisor 顶级文件系统签名，Guest 读 ACPI 表时可见",
        weight: 0.85,
    },
    // ---------- KVM（规格判据点名） ----------
    HypervisorSignature {
        key: "kvm-vga-name",
        pattern: "QEMU Virtual Video Controller",
        platform: Hypervisor::Kvm,
        source: "KVM/QEMU 虚拟 VGA 默认设备名，virtio-gpu 未启用时的标准名",
        weight: 0.50,
    },
    HypervisorSignature {
        key: "kvm-virtio-gpu",
        pattern: "virtio-gpu",
        platform: Hypervisor::Kvm,
        source: "virtio-gpu 半虚拟化设备名，KVM 下最常见的显示路径",
        weight: 0.60,
    },
    HypervisorSignature {
        key: "kvm-pci-vendor",
        pattern: "0x1af4",
        platform: Hypervisor::Kvm,
        source: "PCI vendor ID 0x1AF4 = Red Hat virtio 全族（网卡/块/显示），KVM 侧证据",
        weight: 0.70,
    },
    HypervisorSignature {
        key: "kvm-cpuid-hypervisor",
        pattern: "KVMKVMKVM",
        platform: Hypervisor::Kvm,
        source: "CPUID leaf 0x40000000 的 hypervisor 品牌串，KVM 的判定铁证",
        weight: 0.95,
    },
    // ---------- Xen ----------
    HypervisorSignature {
        key: "xen-cpuid",
        pattern: "XenVMMXenVMM",
        platform: Hypervisor::Xen,
        source: "CPUID hypervisor 品牌串，Xen 的等价于 KVMKVMKVM 的铁证",
        weight: 0.95,
    },
    HypervisorSignature {
        key: "xen-vga",
        pattern: "Xen Virtual Video",
        platform: Hypervisor::Xen,
        source: "Xen 的虚拟 VGA 设备名",
        weight: 0.50,
    },
    // ---------- Parallels ----------
    HypervisorSignature {
        key: "parallels-pci-vendor",
        pattern: "0x1ab8",
        platform: Hypervisor::Parallels,
        source: "PCI vendor ID 0x1AB8 = Parallels，Parallels Desktop 虚拟硬件族",
        weight: 0.70,
    },
    HypervisorSignature {
        key: "parallels-name",
        pattern: "Parallels",
        platform: Hypervisor::Parallels,
        source: "Parallels 虚拟设备名",
        weight: 0.45,
    },
    // ---------- VirtualBox ----------
    HypervisorSignature {
        key: "vbox-name",
        pattern: "VirtualBox Graphics Adapter",
        platform: Hypervisor::VirtualBox,
        source: "VirtualBox 的虚拟显示设备名",
        weight: 0.55,
    },
    HypervisorSignature {
        key: "vbox-vga",
        pattern: "VBoxSVGA",
        platform: Hypervisor::VirtualBox,
        source: "VirtualBox 的 VBoxSVGA 显示驱动标识",
        weight: 0.60,
    },
    // ---------- QEMU（无 KVM 加速，纯 TCG 仿真） ----------
    HypervisorSignature {
        key: "qemu-bochs",
        pattern: "Bochs VBE",
        platform: Hypervisor::Qemu,
        source: "QEMU 默认 stdvga 的 Bochs VBE 扩展名，无 KVM 时常见",
        weight: 0.45,
    },
    HypervisorSignature {
        key: "qemu-stdvga",
        pattern: "QEMU VGA",
        platform: Hypervisor::Qemu,
        source: "QEMU 标准 VGA 设备名",
        weight: 0.40,
    },
];

/// 软件渲染适配器在册特征（第三类识别）。
/// 这类"适配器"没有硬件，靠 CPU 出帧，必须被识别出来并被仲裁器选为回退目标。
pub const SOFTWARE_RENDER_SIGNATURES: &[&str] = &[
    "llvmpipe",
    "softpipe",
    "swrast",
    "Microsoft Basic Render",
    "Software Adapter",
    "GDI Generic",
    "Direct3D 11 WARP",
    "Apple Software Renderer",
    "Mesa OffScreen",
];

/// 物理 GPU 名称在册特征（辅助分类，处理厂商 ID 读不到的情况）。
pub const PHYSICAL_NAME_SIGNATURES: &[&str] = &[
    "GeForce", "Radeon", "RX ", "Arc ", "Iris", "UHD Graphics", "Iris Xe", "Apple M", "Apple GPU",
    "NVIDIA", "AMD Radeon",
];

/// 物理 GPU 厂商在册表（vendor_id → 厂商名）。
pub const PHYSICAL_VENDOR_TABLE: &[(u32, &str)] = &[
    (0x10DE, "NVIDIA"),
    (0x1002, "AMD"),
    (0x1022, "AMD"),
    (0x8086, "Intel"),
    (0x106B, "Apple"),
    (0x14E4, "Broadcom"),
    (0x1A03, "ASPEED"),
    (0x1234, "QEMU/Bochs（软仿真，无加速）"),
    (0x80EE, "VirtualBox"),
];

/// 规格判据自检——三家点名平台是否都在册。缺一即漏判据。
pub fn check_required_hypervisors_in_registry() -> (bool, Vec<Hypervisor>) {
    let required = [Hypervisor::VMware, Hypervisor::HyperV, Hypervisor::Kvm];
    let missing: Vec<Hypervisor> = required
        .iter()
        .copied()
        .filter(|h| !HYPERVISOR_SIGNATURES.iter().any(|s| s.platform == *h))
        .collect();
    (missing.is_empty(), missing)
}
