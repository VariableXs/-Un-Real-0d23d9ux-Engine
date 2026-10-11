//! VE-F0201 · virtio-gpu 设备探测与初始化（VE-B 域 · GPU 驱动矩阵 · 目标 460 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0201`
//!
//! **规格原文**：B 域第一个驱动：走通 A 域冻结接口的完整实现。探测流程：PCI
//! 枚举命中 virtio 商标（vendor 0x1AF4，device 0x1050 现代版/0x0380 旧版）→
//! 读设备能力列表（virtio PCI capabilities：common cfg/isr cfg/device cfg/
//! notify 区域）→ 版本协商（VIRTIO_F_VERSION_1 必需，缺即拒绝并报三要素）→
//! 读 device cfg 拿扫描出口数与最大尺寸。初始化序：reset→ack 状态位→特性
//! 协商→队列分配（controlq + cursorq）→ DRIVER_OK。全流程只走 virtio 标准
//! 寄存器语义，零"试试看"写。与 A 域对接：产出 AdapterReport（VE-F0001 格式）
//! 注册为主适配器候选。判据：QEMU 识别并 DRIVER_OK、旧版设备显性拒绝、能力
//! 解析完整、注册报告过 A 域 schema、初始化 ≤100ms。
//!
//! **设计要点**：
//! - 本文件是**类型契约层**：设备快照（PCI 枚举的一手资料）、能力列表原始
//!   字节、virtio 状态位/特征位、拒绝三要素（发生了什么/为什么/下一步）；
//! - 拒绝必带 `next`（修正建议）——异常零静默铁律，拒绝不给路 = 把 AI 和
//!   用户一起堵死；
//! - 旧版设备（0x0380）**显性拒绝**而非静默降级：旧版没有现代能力布局，
//!   硬初始化是"试试看"写，正是规格点名禁止的路径；
//! - 确定性、零 IO、可序列化：设备状态来自快照注入（PCI 配置空间的一手
//!   读数），不摸真硬件——QEMU 判据靠对拍快照回归覆盖。

use crate::svstar2::vea01_probe::fnv1a64;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、virtio 商标与身份（判据：QEMU 识别、旧版显性拒绝）
// ---------------------------------------------------------------------------

/// virtio 委员会 PCI 厂商标识。
pub const VENDOR_VIRTIO: u32 = 0x1AF4;
/// virtio-gpu 现代版设备 ID（virtio 1.0+ 布局）。
pub const DEVICE_MODERN: u32 = 0x1050;
/// virtio-gpu 旧版（legacy/transitional）设备 ID。
pub const DEVICE_LEGACY: u32 = 0x0380;

/// PCI 能力列表里 virtio 厂商能力的 cap_vndr 值。
pub const CAP_VENDOR_VIRTIO: u8 = 0x09;

/// virtio PCI 能力小类（cfg_type）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapKind {
    /// 公共配置（设备状态/特征/队列选择，virtio_pci_common_cfg）
    Common,
    /// 通知区域（队列门铃）
    Notify,
    /// ISR 状态
    Isr,
    /// 设备专有配置（virtio-gpu 的 scanout/显示信息在这）
    Device,
}

impl CapKind {
    pub fn from_cfg_type(v: u8) -> Option<CapKind> {
        match v {
            1 => Some(CapKind::Common),
            2 => Some(CapKind::Notify),
            3 => Some(CapKind::Isr),
            4 => Some(CapKind::Device),
            _ => None,
        }
    }

    /// 各能力小类的最小合法长度（字节）。短于它 = 布局不完整，拒绝。
    pub fn min_len(self) -> u32 {
        match self {
            // common cfg 至少要能覆盖到 queue_used_event（0x38 处才完整，
            // 现代布局最低 0x2C 即含 queue_notify_data 前的全部字段）
            CapKind::Common => 0x2C,
            CapKind::Notify => 4,
            CapKind::Isr => 1,
            // device cfg 至少含 events_read + scanout 数量与一档尺寸
            CapKind::Device => 24,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CapKind::Common => "common cfg",
            CapKind::Notify => "notify 区域",
            CapKind::Isr => "isr cfg",
            CapKind::Device => "device cfg",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、virtio 状态位与特征位
// ---------------------------------------------------------------------------

/// 设备状态位（virtio_pci_common_cfg.device_status 的标准语义）。
pub const STATUS_ACKNOWLEDGE: u8 = 0x01;
pub const STATUS_DRIVER: u8 = 0x02;
pub const STATUS_DRIVER_OK: u8 = 0x04;
pub const STATUS_FEATURES_OK: u8 = 0x08;
pub const STATUS_FAILED: u8 = 0x80;

/// VIRTIO_F_VERSION_1 的特征位号。现代版布局的强制门槛。
pub const F_VERSION_1: u32 = 32;

/// virtio-gpu 设备专有特征位（位 0 起算，落在 offered_features 低 32 位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GpuFeatures {
    /// VIRTIO_GPU_F_VIRGL：3D 加速支持
    pub virgl: bool,
    /// VIRTIO_GPU_F_EDID：显示器 EDID 查询支持
    pub edid: bool,
    /// VIRTIO_GPU_F_RESOURCE_UUID
    pub resource_uuid: bool,
    /// VIRTIO_GPU_F_RESOURCE_BLOB
    pub resource_blob: bool,
    /// VIRTIO_GPU_F_CONTEXT_INIT
    pub context_init: bool,
}

impl GpuFeatures {
    /// 从 offered_features 低 32 位解析设备专有特征。
    pub fn parse(offered: u64) -> GpuFeatures {
        let low = offered as u32;
        GpuFeatures {
            virgl: low & (1 << 0) != 0,
            edid: low & (1 << 1) != 0,
            resource_uuid: low & (1 << 2) != 0,
            resource_blob: low & (1 << 3) != 0,
            context_init: low & (1 << 4) != 0,
        }
    }

    /// 读屏可达的特征摘要（规格：探测结果读屏可达）。
    pub fn describe(&self) -> String {
        let mut v: Vec<&str> = Vec::new();
        if self.virgl {
            v.push("3D加速");
        }
        if self.edid {
            v.push("EDID");
        }
        if self.resource_uuid {
            v.push("资源UUID");
        }
        if self.resource_blob {
            v.push("Blob资源");
        }
        if self.context_init {
            v.push("上下文");
        }
        if v.is_empty() {
            "无扩展特征（纯 2D 显示）".to_string()
        } else {
            format!("扩展特征：{}", v.join("、"))
        }
    }
}

// ---------------------------------------------------------------------------
// 三、能力列表原始快照（判据：能力解析完整）
// ---------------------------------------------------------------------------

/// 一条 PCI 配置空间能力快照（探测阶段原样读出，解析交给初始化器）。
#[derive(Clone, Debug)]
pub struct PciCapRaw {
    /// cap_vndr；virtio 厂商能力必须为 0x09
    pub vndr: u8,
    /// 链表下一跳偏移
    pub next: u8,
    /// 本能力结构总长
    pub len: u8,
    /// 能力小类（cfg_type 原值，解析失败要原样报给用户）
    pub cfg_type: u8,
    /// 映射的 BAR 号
    pub bar: u8,
    /// BAR 内偏移
    pub offset: u32,
    /// 区域长度
    pub length: u32,
    /// notify 专有：notify_off_multiplier（仅 cfg_type=2 时有值）
    pub notify_off_multiplier: Option<u32>,
}

impl PciCapRaw {
    pub fn new(cfg_type: u8, bar: u8, offset: u32, length: u32) -> PciCapRaw {
        PciCapRaw {
            vndr: CAP_VENDOR_VIRTIO,
            next: 0,
            len: 16,
            cfg_type,
            bar,
            offset,
            length,
            notify_off_multiplier: None,
        }
    }

    /// 带 notify 乘数的 notify 能力。
    pub fn notify(bar: u8, offset: u32, length: u32, multiplier: u32) -> PciCapRaw {
        PciCapRaw {
            vndr: CAP_VENDOR_VIRTIO,
            next: 0,
            len: 20,
            cfg_type: 2,
            bar,
            offset,
            length,
            notify_off_multiplier: Some(multiplier),
        }
    }
}

// ---------------------------------------------------------------------------
// 四、device cfg 快照（判据：扫描出口数与最大尺寸）
// ---------------------------------------------------------------------------

/// virtio-gpu 最大 scanout 数（virtio 规格定值 VIRTIO_GPU_MAX_SCANOUTS）。
pub const MAX_SCANOUTS: u32 = 16;
/// 最大边长上限（对拍基准的钳制线；4K 红线由验收矩阵另测）。
pub const MAX_DIMENSION: u32 = 32768;

/// device cfg 的显示能力快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayCfg {
    /// 扫描出口数
    pub scanouts: u32,
    /// 最大像素宽度
    pub max_width: u32,
    /// 最大像素高度
    pub max_height: u32,
}

impl DisplayCfg {
    /// QEMU 默认 virtio-gpu 显示能力（对拍基准输入）。
    pub fn qemu_default() -> DisplayCfg {
        DisplayCfg {
            scanouts: 1,
            max_width: 3840,
            max_height: 2160,
        }
    }

    /// 边界校验：越界即拒绝（带修正建议，异常零静默）。
    pub fn validate(&self) -> Result<(), Rejection> {
        if self.scanouts == 0 {
            return Err(Rejection {
                code: "E_CFG_INVALID",
                what: "device cfg 声明 0 个扫描出口".to_string(),
                why: "没有 scanout 就没有可显示画面，图形栈无从落地".to_string(),
                next: "检查 QEMU/虚拟机监控器的 virtio-gpu 配置（disable-run=off）后重探"
                    .to_string(),
            });
        }
        if self.scanouts > MAX_SCANOUTS {
            return Err(Rejection {
                code: "E_CFG_INVALID",
                what: format!(
                    "device cfg 声明 {} 个扫描出口，超过规格上限 {}",
                    self.scanouts, MAX_SCANOUTS
                ),
                why: "virtio 规格定值 MAX_SCANOUTS=16，超限说明读到的不是合法 device cfg"
                    .to_string(),
                next: "复核 device cfg 的 bar/offset/length 三元组是否命中真实区域".to_string(),
            });
        }
        if self.max_width == 0
            || self.max_height == 0
            || self.max_width > MAX_DIMENSION
            || self.max_height > MAX_DIMENSION
        {
            return Err(Rejection {
                code: "E_CFG_INVALID",
                what: format!(
                    "device cfg 最大尺寸 {}x{} 越界（合法 1..={}）",
                    self.max_width, self.max_height, MAX_DIMENSION
                ),
                why: "尺寸为 0 或超上限都说明能力读数不可信".to_string(),
                next: "重新读 device cfg；连续越界按设备缺陷走软渲回退".to_string(),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、拒绝三要素（判据：旧版显性拒绝、缺 VERSION_1 报三要素）
// ---------------------------------------------------------------------------

/// 拒绝三要素：发生了什么 / 为什么 / 下一步怎么办。
///
/// 异常零静默铁律的载体：任何拒绝都必须同时给全三项，
/// 只报 code 不给路的拒绝在评审按缺陷处理。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    /// 机器可读错误码
    pub code: &'static str,
    /// 发生了什么（人话）
    pub what: String,
    /// 为什么（根因）
    pub why: String,
    /// 下一步怎么办（修正建议）
    pub next: String,
}

impl Rejection {
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 六、设备快照（探测输入的一手资料）
// ---------------------------------------------------------------------------

/// virtio-gpu 设备快照——PCI 枚举阶段读到的全部一手资料。
///
/// 零 IO：初始化器只消费快照，不摸真硬件；QEMU 判据由对拍快照覆盖。
#[derive(Clone, Debug)]
pub struct VirtioGpuDeviceSnapshot {
    /// PCI 槽位地址
    pub slot: String,
    pub vendor_id: u32,
    pub device_id: u32,
    /// 驱动版本串（虚拟化环境下随宿主版本走，进指纹）
    pub driver_version: String,
    /// 原始能力列表（按 PCI 配置空间顺序）
    pub caps_raw: Vec<PciCapRaw>,
    /// 64 位设备特征字（协商输入）
    pub offered_features: u64,
    /// device cfg 读数
    pub display: DisplayCfg,
    /// 各 BAR 的尺寸（字节）；0 = 该 BAR 不存在
    pub bar_sizes: [u32; 6],
    /// controlq / cursorq 的设备报告队列深度
    pub queue_sizes: [u16; 2],
}

impl VirtioGpuDeviceSnapshot {
    /// QEMU 标准 virtio-gpu 现代版对拍基准快照。
    pub fn qemu_modern(slot: &str) -> VirtioGpuDeviceSnapshot {
        VirtioGpuDeviceSnapshot {
            slot: slot.to_string(),
            vendor_id: VENDOR_VIRTIO,
            device_id: DEVICE_MODERN,
            driver_version: "qemu-virtio-1.0".to_string(),
            caps_raw: vec![
                PciCapRaw::new(1, 4, 0x0000, 0x0038), // common cfg
                PciCapRaw::notify(4, 0x1000, 0x1000, 1), // notify
                PciCapRaw::new(3, 4, 0x2000, 0x0001), // isr
                PciCapRaw::new(4, 4, 0x3000, 0x0100), // device cfg
            ],
            offered_features: (1u64 << F_VERSION_1) | (1 << 0) | (1 << 1), // V1 + virgl + edid
            display: DisplayCfg::qemu_default(),
            bar_sizes: [0, 0, 0, 0, 0x4000, 0],
            queue_sizes: [64, 64],
        }
    }

    /// 旧版设备快照（判据：显性拒绝的靶子）。
    pub fn legacy(slot: &str) -> VirtioGpuDeviceSnapshot {
        VirtioGpuDeviceSnapshot {
            device_id: DEVICE_LEGACY,
            driver_version: "legacy-0.9".to_string(),
            caps_raw: Vec::new(),
            offered_features: 0,
            ..VirtioGpuDeviceSnapshot::qemu_modern(slot)
        }
    }

    /// 是否命中 virtio 商标（vendor + device 双判）。
    pub fn is_virtio_gpu(&self) -> bool {
        self.vendor_id == VENDOR_VIRTIO
            && (self.device_id == DEVICE_MODERN || self.device_id == DEVICE_LEGACY)
    }

    /// 身份判定的第一道闸：vendor 或 device 不命中时给出对路的三要素。
    pub fn identity_rejection(&self) -> Option<Rejection> {
        if self.vendor_id != VENDOR_VIRTIO {
            return Some(Rejection {
                code: "E_NOT_VIRTIO",
                what: format!(
                    "设备 {} 的 vendor 0x{:04X} 不是 virtio 商标 0x{:04X}",
                    self.slot, self.vendor_id, VENDOR_VIRTIO
                ),
                why: "本驱动只服务 virtio-gpu，别的厂商交给各自的驱动矩阵条目"
                    .to_string(),
                next: "该设备由 VE-B 域其他驱动条目认领，不走本初始化器".to_string(),
            });
        }
        if self.device_id != DEVICE_MODERN && self.device_id != DEVICE_LEGACY {
            return Some(Rejection {
                code: "E_NOT_VIRTIO_GPU",
                what: format!(
                    "virtio 设备 {} 的 device 0x{:04X} 不是 virtio-gpu（0x1050/0x0380）",
                    self.slot, self.device_id
                ),
                why: "virtio 家族里 net/blk/scsi 等各有专属设备号，混认会写坏别的设备"
                    .to_string(),
                next: "该设备交给对应 virtio 驱动条目；本驱动只认 0x1050/0x0380".to_string(),
            });
        }
        None
    }

    /// 快照指纹：同机同果可追溯（与 VE-F0001 的指纹纪律同源）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(self.slot.as_bytes());
        buf.extend_from_slice(&self.vendor_id.to_le_bytes());
        buf.extend_from_slice(&self.device_id.to_le_bytes());
        buf.extend_from_slice(self.driver_version.as_bytes());
        buf.extend_from_slice(&self.offered_features.to_le_bytes());
        buf.extend_from_slice(&self.display.scanouts.to_le_bytes());
        buf.extend_from_slice(&self.display.max_width.to_le_bytes());
        buf.extend_from_slice(&self.display.max_height.to_le_bytes());
        for q in self.queue_sizes.iter() {
            buf.extend_from_slice(&q.to_le_bytes());
        }
        for c in self.caps_raw.iter() {
            buf.extend_from_slice(&[
                c.vndr, c.next, c.len, c.cfg_type, c.bar,
            ]);
            buf.extend_from_slice(&c.offset.to_le_bytes());
            buf.extend_from_slice(&c.length.to_le_bytes());
            if let Some(m) = c.notify_off_multiplier {
                buf.extend_from_slice(&m.to_le_bytes());
            }
        }
        fnv1a64(&buf)
    }
}
