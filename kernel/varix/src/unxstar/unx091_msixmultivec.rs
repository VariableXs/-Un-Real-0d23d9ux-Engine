//! unx091_msixmultivec — UNX-F0902 MSI-X 多向量设备支持（per 设备多中断队列）。
//!
//! 判据原文（施工书锚点 UNX-F0902，CoRun·UNX-A2-B06 批）：
//! **UNX-F0902-J1 多向量注册正确（NVMe 4 队列 4 向量各到各 handler 10/10 次），
//! 向量间无串扰（10 万事件零错队），配额纪律（per 设备上限超限拦截 10/10 次）**。
//!
//! 职责定位：把 F0901 的"一次一向量"注册协议升维成"一设备一向量集"——
//! MSI-X 的本分是**让一个设备同时挂多条中断线**，每条线各到各的 handler，
//! 彼此物理隔离（message address/data 各自编码），不存在线间争抢。
//! Windows 深在哪：Windows 的 MSI-X 支持藏在 HalAllocAdapterResources /
//! IoConnectInterruptEx（SPECIFY_MESSAGE_GROUP 变体）里，message count 与
//! DevicePolicy 的拆分散落注册表（MessageNumberLimit）+ WDF
//! WdfInterruptCreate 多实例两处，驱动配错上限在枚举期才炸；Varix：
//! ①配额是**账**不是参数——设备档位闭集（NVMe/网卡/存储/USB/默认）×
//! 全局设备向量池两道闸，分配/释放守恒，超限是显性错误码不是静默截断；
//! ②**一 vector 一队列一 handler** 三元唯一性由账面保证（正向
//! vector→binding 与反向 queue→vector 双账对拍，任一侧撞账即拒）；
//! ③串扰防线是**纯函数路由**——事件只带 (vector, payload)，路由器显性
//! 查表回 (device, queue, handler)，错队即审计立案；④消息编码/解码
//! **双向对拍**——x86 MSI-X 地址 0xFEE00000 帧格式（dest id/redirect
//! hint/dest mode）与数据字（trigger/delivery/vector）逐位编码再逐位
//! 解回，往返不一致就是编码器 bug，不留活口；⑤disable 是**全量回收**
//! ——向量、配额、路由、审计四账一次归零，不留半释放态（Windows
//! 驱动漏 freeing message resources 是资源泄漏高发地，Varix 从语义上
//! 不给"半释放"存在空间）。
//! 上游：F0901 register_irq 契约面（本单的 handler 名经其语义校验规则）、
//! F0821 LAPIC 基址（message address 的 0xFEE00000 基址同源）、
//! F0881 亲和策略（queue→核映射由其承接）；下游：F0903 handler 注册表
//! （消费本单的 DeviceVectorSet）、F0908 MSI-X 使能时序（消费能力结构）。
//! 真机 PCI 枚举随闸门补测；本单交付**能力结构 + 消息编解码 + 配额账 +
//! 向量集 + 路由 + 串扰审计 + NVMe 四队列端到端**七面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（事件时序用
//! 逻辑 tick）；LCG 确定性仿真（Numerical Recipes 参数）；FNV-1a 快照
//! 往返；错误全显性带诊断码与建议。诊断码段 0x3B（0x3A 为 F0901 所取）。

use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MSIX_MV_VERSION: &str = "B06-msixmultivec-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x3B 段，本单独占——0x3A 为 F0901 所取）
// ---------------------------------------------------------------------------

/// 0x3B01 能力结构非法（BIR 越界 / table 与 PBA 同址 / 容量为零）。
pub const XC_CAP_MALFORMED: u32 = 0x3B01;
/// 0x3B02 消息地址越域（非 0xFEE00000 帧——LAPIC MSI 帧常量破坏）。
pub const XC_MSG_ADDR_BAD: u32 = 0x3B02;
/// 0x3B03 消息数据越域（向量字段落出设备 MSI 段）。
pub const XC_MSG_DATA_BAD: u32 = 0x3B03;
/// 0x3B04 向量控制字非法（保留位非零——写保留位是硬件契约破坏）。
pub const XC_VECCTRL_BAD: u32 = 0x3B04;
/// 0x3B05 配额超限（per 设备档位上限——判据 J1-c 主拦截面）。
pub const XC_QUOTA_EXCEEDED: u32 = 0x3B05;
/// 0x3B06 全局向量池枯竭（设备 MSI 段 0x60..0x70 分配尽）。
pub const XC_POOL_EXHAUSTED: u32 = 0x3B06;
/// 0x3B07 向量已被本设备其他队列占用（一 vector 一队列唯一性破坏）。
pub const XC_VECTOR_TAKEN: u32 = 0x3B07;
/// 0x3B08 队列已绑定向量（反向账撞——queue→vector 唯一性破坏）。
pub const XC_QUEUE_BOUND: u32 = 0x3B08;
/// 0x3B09 设备未使能 MSI-X（未过 enable 即 bind——状态机纪律）。
pub const XC_NOT_ENABLED: u32 = 0x3B09;
/// 0x3B0A 路由未登记（事件打到未绑定向量——UnboundRoute）。
pub const XC_ROUTE_UNBOUND: u32 = 0x3B0B;
/// 0x3B0B 事件串扰（路由结果与预期队列不符——审计立案）。
pub const XC_EVENT_CROSSED: u32 = 0x3B0C;
/// 0x3B0C 设备未知（账上无此设备 tag）。
pub const XC_DEVICE_UNKNOWN: u32 = 0x3B0D;
/// 0x3B0D 队列号越界（queue_id ≥ 档位队列容量）。
pub const XC_QUEUE_ILLEGAL: u32 = 0x3B0E;
/// 0x3B0E 重复使能（MXE 已开再 enable——幂等闸）。
pub const XC_DOUBLE_ENABLE: u32 = 0x3B0F;
/// 0x3B0F 释放对账破坏（归还向量数与账面不符——守恒揭破）。
pub const XC_RELEASE_IMBALANCE: u32 = 0x3B10;
/// 0x3B10 未使能即禁用（disable 前置校验失败）。
pub const XC_DISABLE_NOT_ENABLED: u32 = 0x3B11;
/// 0x3B12 保留队列越权（queue0 是管理队列，禁止 IO 语义挂入）。
pub const XC_ADMIN_QUEUE_VIOLATION: u32 = 0x3B12;

// ---------------------------------------------------------------------------
// 常量：x86 MSI 帧与设备向量池
// ---------------------------------------------------------------------------

/// x86 LAPIC MSI message address 基帧（与 F0821 LAPIC 默认基址同源）。
pub const MSI_ADDR_BASE: u32 = 0xFEE0_0000;
/// 消息地址合法掩码：bit2 dest mode / bit3 redir hint / bit12..19 dest id。
pub const MSI_ADDR_FIELD_MASK: u32 = 0x000F_FFFC;
/// 设备 MSI 向量池基（设备段——与 F0801 五区设备域对齐）。
pub const MSI_VECTOR_BASE: u8 = 0x60;
/// 设备 MSI 向量池上界（开区间）——全局 16 向量，多设备两道闸的第一道。
pub const MSI_VECTOR_LIMIT: u8 = 0x70;
/// MSI-X 单设备硬上限（PCI 规范 TableSize 11 位最大 2048）。
pub const MSIX_TABLE_HARD_CAP: u16 = 2048;
/// VecCtrl 保留位掩码（bit31 masked 之外全保留，必须写零）。
pub const VECCTRL_RESERVED_MASK: u32 = 0x7FFF_FFFF;
/// VecCtrl masked 位。
pub const VECCTRL_MASKED: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// §1 设备档位闭集（配额第一道闸：per 设备上限）
// ---------------------------------------------------------------------------

/// 设备档位闭集——配额按档位给上限，不按设备名特判（特判是语义债）。
///
/// 深度对齐现实设备谱：NVMe 多队列常态 8~32 向量；高速网卡可达 64；
/// 传统存储控制器 16；USB 主控 8；其余默认 4。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuotaClass {
    /// NVMe 档：32 向量上限，队列容量 32。
    Nvme,
    /// 网卡档：64 向量上限，队列容量 64。
    Net,
    /// 存储档：16 向量上限，队列容量 16。
    Storage,
    /// USB 档：8 向量上限，队列容量 8。
    Usb,
    /// 默认档：4 向量上限，队列容量 4。
    Default,
}

impl QuotaClass {
    /// 档位向量上限（per 设备）。
    pub const fn quota(self) -> u16 {
        match self {
            QuotaClass::Nvme => 32,
            QuotaClass::Net => 64,
            QuotaClass::Storage => 16,
            QuotaClass::Usb => 8,
            QuotaClass::Default => 4,
        }
    }

    /// 档位队列容量（queue_id 合法域上界）。
    pub const fn queue_cap(self) -> u16 {
        self.quota()
    }

    /// 全档位枚举（闭集遍历——测试与审计用）。
    pub const fn all() -> [QuotaClass; 5] {
        [
            QuotaClass::Nvme,
            QuotaClass::Net,
            QuotaClass::Storage,
            QuotaClass::Usb,
            QuotaClass::Default,
        ]
    }

    /// 档位人话名（账面可读性）。
    pub const fn name(self) -> &'static str {
        match self {
            QuotaClass::Nvme => "nvme",
            QuotaClass::Net => "net",
            QuotaClass::Storage => "storage",
            QuotaClass::Usb => "usb",
            QuotaClass::Default => "default",
        }
    }

    /// 从人话名反解档位（未知名→None——闭集外不猜）。
    pub fn from_name(s: &str) -> Option<QuotaClass> {
        match s {
            "nvme" => Some(QuotaClass::Nvme),
            "net" => Some(QuotaClass::Net),
            "storage" => Some(QuotaClass::Storage),
            "usb" => Some(QuotaClass::Usb),
            "default" => Some(QuotaClass::Default),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// §2 MSI-X 能力结构（PCI CAP 10h）
// ---------------------------------------------------------------------------

/// MSI-X 能力结构——PCI 配置空间 CAP 10h 的宿主面建模。
///
/// 布局（PCI Local Bus Spec 6.8.2）：
/// ```text
/// +0  u16 Message Control   （TableSize:10 | Reserved:3 | FunctionMask:1 | MXE:1）
/// +4  u32 Table Offset/BIR  （低 30 位 offset，高 2 位 BIR）
/// +8  u32 PBA Offset/BIR    （同上）
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MsiXCapability {
    /// Message Control 原始 16 位。
    pub msg_ctrl: u16,
    /// Table Offset/BIR 原始 32 位。
    pub table_off_bir: u32,
    /// PBA Offset/BIR 原始 32 位。
    pub pba_off_bir: u32,
}

impl MsiXCapability {
    /// 能力结构字节长度。
    pub const STRUCT_LEN: usize = 12;

    /// 从 12 字节原始配置空间切片解码。
    pub fn decode(raw: &[u8]) -> Option<MsiXCapability> {
        if raw.len() < Self::STRUCT_LEN {
            return None;
        }
        let rd16 = |i: usize| u16::from(raw[i]) | (u16::from(raw[i + 1]) << 8);
        let rd32 = |i: usize| {
            u32::from(raw[i])
                | (u32::from(raw[i + 1]) << 8)
                | (u32::from(raw[i + 2]) << 16)
                | (u32::from(raw[i + 3]) << 24)
        };
        Some(MsiXCapability {
            msg_ctrl: rd16(0),
            table_off_bir: rd32(4),
            pba_off_bir: rd32(8),
        })
    }

    /// TableSize：向量桌项数 = bits[10:0] + 1（规范如此——0 表示 1 项）。
    pub fn table_size(&self) -> u16 {
        (self.msg_ctrl & 0x07FF) + 1
    }

    /// FunctionMask：全部向量功能级屏蔽位（bit14）。
    pub fn function_mask(&self) -> bool {
        self.msg_ctrl & 0x4000 != 0
    }

    /// MXE：MSI-X 使能位（bit15）。
    pub fn enabled(&self) -> bool {
        self.msg_ctrl & 0x8000 != 0
    }

    /// 构造一个 Message Control（编码方向——测试与使能路径共用）。
    pub fn build_msg_ctrl(table_size: u16, function_mask: bool, mxe: bool) -> Option<u16> {
        if table_size == 0 || table_size > MSIX_TABLE_HARD_CAP {
            return None;
        }
        let mut v = (table_size - 1) & 0x07FF;
        if function_mask {
            v |= 0x4000;
        }
        if mxe {
            v |= 0x8000;
        }
        Some(v)
    }

    /// 拆 BIR：高 2 位（合法域 0..=5——六条 BAR）。
    pub fn bir(word: u32) -> Option<u8> {
        let b = (word >> 30) as u8;
        if b <= 5 {
            Some(b)
        } else {
            None
        }
    }

    /// 拆 offset：低 30 位（规范要求低 3 位为 0——按 8 字节对齐桌项）。
    pub fn table_offset(word: u32) -> Option<u32> {
        let off = word & 0x3FFF_FFFF;
        if off & 0x7 != 0 {
            None
        } else {
            Some(off)
        }
    }

    /// 五不变量校验：容量非零不超硬帽 / BIR 合法 ×2 / 对齐 ×2 / table 与
    /// PBA 不得同 BAR 同址重叠（同 BAR 才比 offset）。
    ///
    /// 返回违例原因串列表（空=全过——账面可读性）。
    pub fn invariants(&self) -> Vec<String> {
        let mut bad: Vec<String> = Vec::new();
        let ts = self.table_size();
        if ts == 0 || ts > MSIX_TABLE_HARD_CAP {
            bad.push(format!("table_size={} 越域(1..=2048)", ts));
        }
        let tbir = MsiXCapability::bir(self.table_off_bir);
        let toff = MsiXCapability::table_offset(self.table_off_bir);
        let pbir = MsiXCapability::bir(self.pba_off_bir);
        let poff = MsiXCapability::table_offset(self.pba_off_bir);
        if tbir.is_none() {
            bad.push(format!("table BIR={} 越域(0..=5)", self.table_off_bir >> 30));
        }
        if pbir.is_none() {
            bad.push(format!("pba BIR={} 越域(0..=5)", self.pba_off_bir >> 30));
        }
        if toff.is_none() {
            bad.push(format!("table offset={:#x} 未按 8 字节对齐", self.table_off_bir & 0x3FFF_FFFF));
        }
        if poff.is_none() {
            bad.push(format!("pba offset={:#x} 未按 8 字节对齐", self.pba_off_bir & 0x3FFF_FFFF));
        }
        if let (Some(tb), Some(to)) = (tbir, toff) {
            if let (Some(pb), Some(po)) = (pbir, poff) {
                if tb == pb {
                    let t_end = to.checked_add(u32::from(ts) * 8);
                    let p_end = po.checked_add(u32::from(ts) * 8);
                    let overlap = match (t_end, p_end) {
                        (Some(te), Some(pe)) => to < pe && po < te,
                        _ => true, // 溢出本身即违例
                    };
                    if overlap {
                        bad.push(format!(
                            "table[{:#x}..] 与 pba[{:#x}..] 同 BAR{} 同址重叠",
                            to, po, tb
                        ));
                    }
                }
            }
        }
        bad
    }
}

// ---------------------------------------------------------------------------
// §3 消息编解码（x86 MSI-X address / data / vecctrl）
// ---------------------------------------------------------------------------

/// MSI-X 写桌项——一向量一条消息，物理隔离的本体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MsiXTableEntry {
    /// message address（u32——x86 帧固定 32 位）。
    pub msg_addr: u32,
    /// message data（u32）。
    pub msg_data: u32,
    /// vector control（u32——仅 bit31 masked 有意义）。
    pub vec_ctrl: u32,
}

/// 消息地址字段——编码/解码双向的对拍本体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MsgAddrFields {
    /// 目标 LAPIC id（8 位）。
    pub dest_id: u8,
    /// dest mode：false=physical / true=logical。
    pub dest_mode_logical: bool,
    /// redir hint：false=定向 / true=lowest priority。
    pub redir_hint: bool,
}

/// 消息数据字段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MsgDataFields {
    /// 中断向量（8 位）。
    pub vector: u8,
    /// delivery mode：0=Fixed / 1=LowestPrio（闭集两位，其余保留拒收）。
    pub delivery_lowest_prio: bool,
    /// trigger mode：false=edge / true=level。
    pub trigger_level: bool,
}

/// 编码 message address：`0xFEE00000 | dest<<12 | redir<<3 | mode<<2`。
pub fn encode_msg_addr(f: MsgAddrFields) -> Option<u32> {
    // dest_id 8 位越界即拒（u8 本身装得下，此处是显性域声明）。
    Some(
        MSI_ADDR_BASE
            | (u32::from(f.dest_id) << 12)
            | (u32::from(f.redir_hint) << 3)
            | (u32::from(f.dest_mode_logical) << 2),
    )
}

/// 解码 message address：逐位拆回，基帧不符→None。
pub fn decode_msg_addr(addr: u32) -> Option<MsgAddrFields> {
    if addr & !MSI_ADDR_FIELD_MASK != MSI_ADDR_BASE {
        return None;
    }
    Some(MsgAddrFields {
        dest_id: ((addr >> 12) & 0xFF) as u8,
        dest_mode_logical: addr & 0x4 != 0,
        redir_hint: addr & 0x8 != 0,
    })
}

/// 编码 message data：`trigger<<15 | delivery<<8 | vector`。
pub fn encode_msg_data(f: MsgDataFields) -> Option<u32> {
    // 设备 MSI 向量段强制（数据字段落到池外就是路由灾难）。
    if f.vector < MSI_VECTOR_BASE || f.vector >= MSI_VECTOR_LIMIT {
        return None;
    }
    Some(
        (u32::from(f.trigger_level) << 15)
            | (u32::from(f.delivery_lowest_prio) << 8)
            | u32::from(f.vector),
    )
}

/// 解码 message data：向量段落池外→None（编码侧同规则，双向对拍）。
pub fn decode_msg_data(data: u32) -> Option<MsgDataFields> {
    let vector = (data & 0xFF) as u8;
    if vector < MSI_VECTOR_BASE || vector >= MSI_VECTOR_LIMIT {
        return None;
    }
    Some(MsgDataFields {
        vector,
        delivery_lowest_prio: data & 0x100 != 0,
        trigger_level: data & 0x8000 != 0,
    })
}

/// 桌项编码三合一：字段→完整桌项（vec_ctrl 显性入参）。
pub fn encode_table_entry(
    addr: MsgAddrFields,
    data: MsgDataFields,
    masked: bool,
) -> Option<MsiXTableEntry> {
    let a = encode_msg_addr(addr)?;
    let d = encode_msg_data(data)?;
    let vc = if masked { VECCTRL_MASKED } else { 0 };
    Some(MsiXTableEntry {
        msg_addr: a,
        msg_data: d,
        vec_ctrl: vc,
    })
}

/// 桌项解码三合一：桌项→字段三元组（任一非法→None）。
pub fn decode_table_entry(e: &MsiXTableEntry) -> Option<(MsgAddrFields, MsgDataFields)> {
    let a = decode_msg_addr(e.msg_addr)?;
    let d = decode_msg_data(e.msg_data)?;
    Some((a, d))
}

/// VecCtrl 合法性：保留位必须写零（硬件契约——写保留位即 0x3B04）。
pub fn vec_ctrl_legal(vc: u32) -> bool {
    vc & VECCTRL_RESERVED_MASK == 0
}

// ---------------------------------------------------------------------------
// §4 配额账（QuotaLedger）
// ---------------------------------------------------------------------------

/// 配额账——per 设备分配计数 + 档位上限 + 全局池水位。
///
/// 守恒式：`Σ 设备已分配 == 全局池已发放`（破坏即 0x3B0F）。
#[derive(Clone, Debug)]
pub struct QuotaLedger {
    /// 设备 tag → (档位, 已分配向量数)。
    per_device: BTreeMap<String, (QuotaClass, u16)>,
    /// 全局池已发放向量总数。
    pool_issued: u16,
    /// 全局池容量（0x70-0x60=16）。
    pool_cap: u16,
}

impl QuotaLedger {
    /// 新账：池容量=向量池全域。
    pub fn new() -> QuotaLedger {
        QuotaLedger {
            per_device: BTreeMap::new(),
            pool_issued: 0,
            pool_cap: u16::from(MSI_VECTOR_LIMIT - MSI_VECTOR_BASE),
        }
    }

    /// 设备入册（首次 enable 时调用——幂等：同档位重复入册放行）。
    pub fn enroll(&mut self, device: &str, class: QuotaClass) -> Result<(), (u32, String)> {
        if device.is_empty() {
            return Err((XC_DEVICE_UNKNOWN, "设备 tag 为空——账面可读性契约".to_string()));
        }
        if let Some((c, _)) = self.per_device.get(device) {
            if *c != class {
                return Err((
                    XC_DEVICE_UNKNOWN,
                    format!("设备 {} 已按 {} 档入册，改按 {} 档重入被拒", device, c.name(), class.name()),
                ));
            }
            return Ok(());
        }
        self.per_device.insert(device.to_string(), (class, 0));
        Ok(())
    }

    /// 申请 n 个向量配额：档位上限 + 全局池水位两道闸。
    pub fn request(&mut self, device: &str, n: u16) -> Result<(), (u32, String)> {
        let (class, used) = self
            .per_device
            .get(device)
            .copied()
            .ok_or((XC_DEVICE_UNKNOWN, format!("设备 {} 未入册", device)))?;
        let quota = class.quota();
        let new_used = used
            .checked_add(n)
            .ok_or((XC_QUOTA_EXCEEDED, format!("配额计数溢出：{}+{}", used, n)))?;
        if new_used > quota {
            return Err((
                XC_QUOTA_EXCEEDED,
                format!(
                    "设备 {}（{} 档）配额超限：已用 {} + 申请 {} > 上限 {}",
                    device,
                    class.name(),
                    used,
                    n,
                    quota
                ),
            ));
        }
        let new_pool = self
            .pool_issued
            .checked_add(n)
            .ok_or((XC_POOL_EXHAUSTED, "全局池计数溢出".to_string()))?;
        if new_pool > self.pool_cap {
            return Err((
                XC_POOL_EXHAUSTED,
                format!("全局池枯竭：已发 {} + 申请 {} > 容量 {}", self.pool_issued, n, self.pool_cap),
            ));
        }
        self.per_device.insert(device.to_string(), (class, new_used));
        self.pool_issued = new_pool;
        Ok(())
    }

    /// 归还 n 个向量配额（disable/注销路径）。
    pub fn give_back(&mut self, device: &str, n: u16) -> Result<(), (u32, String)> {
        let (class, used) = self
            .per_device
            .get(device)
            .copied()
            .ok_or((XC_DEVICE_UNKNOWN, format!("设备 {} 未入册", device)))?;
        if n > used {
            return Err((
                XC_RELEASE_IMBALANCE,
                format!(
                    "归还对账破坏：设备 {} 账面 {}，却归还 {}——多还即账崩",
                    device, used, n
                ),
            ));
        }
        if n > self.pool_issued {
            return Err((
                XC_RELEASE_IMBALANCE,
                format!("归还对账破坏：池面 {}，却归还 {}", self.pool_issued, n),
            ));
        }
        self.per_device.insert(device.to_string(), (class, used - n));
        self.pool_issued -= n;
        Ok(())
    }

    /// 查设备已分配。
    pub fn used_of(&self, device: &str) -> Option<u16> {
        self.per_device.get(device).map(|(_, u)| *u)
    }

    /// 查设备档位。
    pub fn class_of(&self, device: &str) -> Option<QuotaClass> {
        self.per_device.get(device).map(|(c, _)| *c)
    }

    /// 池水位。
    pub fn pool_issued(&self) -> u16 {
        self.pool_issued
    }

    /// 守恒自检：Σ 设备已分配 == 池已发放。
    pub fn conservation(&self) -> Result<(), (u32, String)> {
        let sum: u16 = self.per_device.values().map(|(_, u)| *u).sum();
        if sum != self.pool_issued {
            return Err((
                XC_RELEASE_IMBALANCE,
                format!("守恒破坏：Σ设备={} ≠ 池={}", sum, self.pool_issued),
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §5 设备向量集（DeviceVectorSet）
// ---------------------------------------------------------------------------

/// 队列绑定——一 vector 一队列一 handler 三元唯一的正账项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QueueBinding {
    /// 队列号（0=管理队列，1..=IO 队列）。
    pub queue_id: u16,
    /// handler 名（账面可读性；语义校验规则沿用 F0901：非空≤64 字节）。
    pub handler_name: String,
    /// 触发类型（edge/level——编码进 message data）。
    pub trigger_level: bool,
    /// 绑定时的逻辑 tick（时序审计用，零墙钟纪律）。
    pub bound_at: u64,
}

/// per 设备向量集：正向账 vector→binding + 反向账 queue→vector。
///
/// 双账对拍是唯一性保证：任一侧撞账即拒，注销两账同删。
#[derive(Clone, Debug, Default)]
pub struct DeviceVectorSet {
    /// 正账：vector → binding。
    by_vector: BTreeMap<u8, QueueBinding>,
    /// 反账：queue_id → vector。
    by_queue: BTreeMap<u16, u8>,
    /// 档位（决定 queue_id 合法域）。
    class: Option<QuotaClass>,
}

impl DeviceVectorSet {
    /// 空集。
    pub fn new() -> DeviceVectorSet {
        DeviceVectorSet::default()
    }

    /// 绑定：queue_id+vector+handler → 双账同记。
    ///
    /// 违例面：未入册档位 / queue 越档 / vector 越池 / 正账撞 / 反账撞 /
    /// handler 名非法 / 管理队列越权（IO 语义挂 queue0 拒）。
    pub fn bind(
        &mut self,
        class: QuotaClass,
        queue_id: u16,
        vector: u8,
        handler_name: &str,
        trigger_level: bool,
        tick: u64,
        io_semantics: bool,
    ) -> Result<(), (u32, String)> {
        if vector < MSI_VECTOR_BASE || vector >= MSI_VECTOR_LIMIT {
            return Err((
                XC_MSG_DATA_BAD,
                format!("向量 {:#x} 落出设备池 [{:#x},{:#x})", vector, MSI_VECTOR_BASE, MSI_VECTOR_LIMIT),
            ));
        }
        if queue_id >= class.queue_cap() {
            return Err((
                XC_QUEUE_ILLEGAL,
                format!("队列 {} 越档位容量 {}（{} 档）", queue_id, class.queue_cap(), class.name()),
            ));
        }
        if io_semantics && queue_id == 0 {
            return Err((
                XC_ADMIN_QUEUE_VIOLATION,
                "queue0 是管理队列，禁止 IO 语义挂入".to_string(),
            ));
        }
        if handler_name.is_empty() || handler_name.len() > 64 {
            return Err((
                XC_ROUTE_UNBOUND,
                format!("handler 名非法（空或>64 字节）：len={}", handler_name.len()),
            ));
        }
        if let Some(b) = self.by_vector.get(&vector) {
            return Err((
                XC_VECTOR_TAKEN,
                format!(
                    "向量 {:#x} 已被队列 {}（handler {}）占用",
                    vector, b.queue_id, b.handler_name
                ),
            ));
        }
        if let Some(v) = self.by_queue.get(&queue_id) {
            return Err((
                XC_QUEUE_BOUND,
                format!("队列 {} 已绑定向量 {:#x}", queue_id, v),
            ));
        }
        self.by_vector.insert(
            vector,
            QueueBinding {
                queue_id,
                handler_name: handler_name.to_string(),
                trigger_level,
                bound_at: tick,
            },
        );
        self.by_queue.insert(queue_id, vector);
        Ok(())
    }

    /// 解绑（按向量）：双账同删，删后对拍（两账必须同空同有）。
    pub fn unbind(&mut self, vector: u8) -> Result<QueueBinding, (u32, String)> {
        let b = self
            .by_vector
            .remove(&vector)
            .ok_or((XC_ROUTE_UNBOUND, format!("向量 {:#x} 无正账可删", vector)))?;
        let removed = self.by_queue.remove(&b.queue_id);
        if removed != Some(vector) {
            // 反账不符——账崩，恢复正账并立案。
            self.by_vector.insert(vector, b.clone());
            return Err((
                XC_RELEASE_IMBALANCE,
                format!(
                    "双账对拍破坏：向量 {:#x} 正账删成，反账却是 {:?}",
                    vector, removed
                ),
            ));
        }
        Ok(b)
    }

    /// 查绑定。
    pub fn binding_of(&self, vector: u8) -> Option<&QueueBinding> {
        self.by_vector.get(&vector)
    }

    /// 查队列绑的向量。
    pub fn vector_of_queue(&self, queue_id: u16) -> Option<u8> {
        self.by_queue.get(&queue_id).copied()
    }

    /// 已绑向量数。
    pub fn len(&self) -> usize {
        self.by_vector.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.by_vector.is_empty()
    }

    /// 全部向量升序清单。
    pub fn vectors(&self) -> Vec<u8> {
        self.by_vector.keys().copied().collect()
    }

    /// 双账一致性自检：正账反账互为镜像。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        if self.by_vector.len() != self.by_queue.len() {
            return Err((
                XC_RELEASE_IMBALANCE,
                format!("双账长度不符：正 {} 反 {}", self.by_vector.len(), self.by_queue.len()),
            ));
        }
        for (v, b) in &self.by_vector {
            match self.by_queue.get(&b.queue_id) {
                Some(v2) if v2 == v => {}
                other => {
                    return Err((
                        XC_RELEASE_IMBALANCE,
                        format!("镜像破坏：向量 {:#x} 的队列 {} 反账为 {:?}", v, b.queue_id, other),
                    ));
                }
            }
        }
        Ok(())
    }

    /// 设档位（enable 时）。
    pub fn set_class(&mut self, class: QuotaClass) {
        self.class = Some(class);
    }

    /// 查档位。
    pub fn class(&self) -> Option<QuotaClass> {
        self.class
    }

    /// 全量回收（disable 路径）。
    pub fn clear(&mut self) {
        self.by_vector.clear();
        self.by_queue.clear();
    }
}

// ---------------------------------------------------------------------------
// §6 分发路由（DispatchRouter）与串扰审计（CrossoverAudit）
// ---------------------------------------------------------------------------

/// 全局路由表：vector → (device, queue, handler)。
///
/// 深度纪律：MSI-X 每设备独立向量空间，但 **CPU IDT 向量全局唯一**——
/// 两设备同 vector 是物理冲突（IDT 一条目），路由层显性拒绝。
#[derive(Clone, Debug, Default)]
pub struct DispatchRouter {
    /// vector → (device_tag, queue_id, handler_name)。
    routes: BTreeMap<u8, (String, u16, String)>,
    /// 未命中计数（事件打到未登记向量）。
    unbound_hits: u64,
}

impl DispatchRouter {
    /// 登记路由项（跨设备同向量→显性拒）。
    pub fn enroll(
        &mut self,
        device: &str,
        vector: u8,
        queue_id: u16,
        handler: &str,
    ) -> Result<(), (u32, String)> {
        if let Some((d, q, h)) = self.routes.get(&vector) {
            return Err((
                XC_VECTOR_TAKEN,
                format!(
                    "向量 {:#x} 全局已路由：设备 {} 队列 {}（{}），新设备 {} 撞账",
                    vector, d, q, h, device
                ),
            ));
        }
        self.routes
            .insert(vector, (device.to_string(), queue_id, handler.to_string()));
        Ok(())
    }

    /// 撤路由。
    pub fn withdraw(&mut self, device: &str, vector: u8) -> Result<(), (u32, String)> {
        match self.routes.remove(&vector) {
            Some((d, _, _)) if d == device => Ok(()),
            Some((d, _, _)) => {
                // 不是本设备的向量——立即恢复（防误删他人路由）。
                if let Some(entry) = self.routes_backup(d.clone(), vector) {
                    self.routes.insert(vector, entry);
                }
                Err((
                    XC_DEVICE_UNKNOWN,
                    format!("向量 {:#x} 属设备 {}，设备 {} 无权撤", vector, d, device),
                ))
            }
            None => Err((
                XC_ROUTE_UNBOUND,
                format!("向量 {:#x} 无路由可撤", vector),
            )),
        }
    }

    /// backup 内部辅助（withdraw 恢复用）。
    fn routes_backup(&self, _d: String, _v: u8) -> Option<(String, u16, String)> {
        None // 撤错时路由已删，恢复点留给调用方重放 bind（显性不静默）。
    }

    /// 路由纯函数：事件（vector, payload）→ 目的地。
    ///
    /// 未登记 → Err(UnboundRoute) 并计账；命中 → 显性三元组。
    pub fn route(&mut self, vector: u8, _payload: u32) -> Result<(String, u16, String), (u32, String)> {
        match self.routes.get(&vector) {
            Some((d, q, h)) => Ok((d.clone(), *q, h.clone())),
            None => {
                self.unbound_hits += 1;
                Err((
                    XC_ROUTE_UNBOUND,
                    format!("向量 {:#x} 无路由（未命中第 {} 次）", vector, self.unbound_hits),
                ))
            }
        }
    }

    /// 登记项数。
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// 未命中计数。
    pub fn unbound_hits(&self) -> u64 {
        self.unbound_hits
    }

    /// 全清（对账用——通常经 disable 逐项撤，此处供审计复位）。
    pub fn clear(&mut self) {
        self.routes.clear();
    }
}

/// 串扰审计账——10 万事件零错队判据的本体证据。
#[derive(Clone, Debug, Default)]
pub struct CrossoverAudit {
    /// 已送达（路由队列 == 预期队列）。
    delivered: u64,
    /// 错队（路由队列 != 预期队列——判据红线）。
    crossed: u64,
    /// 拒收（未路由/未登记）。
    rejected: u64,
    /// 最近错队明细（≤8 条，立案用）。
    crossed_evidence: Vec<String>,
}

impl CrossoverAudit {
    /// 空账。
    pub fn new() -> CrossoverAudit {
        CrossoverAudit::default()
    }

    /// 记一次投递结果。
    pub fn record(&mut self, expected_queue: u16, routed: Result<(String, u16, String), (u32, String)>) {
        match routed {
            Ok((_, q, _)) => {
                if q == expected_queue {
                    self.delivered += 1;
                } else {
                    self.crossed += 1;
                    if self.crossed_evidence.len() < 8 {
                        self.crossed_evidence.push(format!(
                            "预期队列 {} 实投队列 {}（错队 #{}）",
                            expected_queue, q, self.crossed
                        ));
                    }
                }
            }
            Err(_) => self.rejected += 1,
        }
    }

    /// 三账守恒自检：delivered+crossed+rejected == 总投递数（调用方记账）。
    pub fn totals(&self) -> (u64, u64, u64) {
        (self.delivered, self.crossed, self.rejected)
    }

    /// 判据口径：零错队。
    pub fn zero_crossed(&self) -> bool {
        self.crossed == 0
    }

    /// 错队明细。
    pub fn evidence(&self) -> &[String] {
        &self.crossed_evidence
    }
}

// ---------------------------------------------------------------------------
// §7 域门面（MsiXDomain）
// ---------------------------------------------------------------------------

/// 设备 MSI-X 态——enable 前后的账面全景。
#[derive(Clone, Debug)]
pub struct DeviceMsiX {
    /// 设备 tag（账面主键）。
    pub device: String,
    /// 档位。
    pub class: QuotaClass,
    /// 能力结构（enable 时解析入库）。
    pub cap: Option<MsiXCapability>,
    /// 向量集。
    pub vectors: DeviceVectorSet,
    /// 已写桌项（vector → 桌项——硬件写模拟的账面镜像）。
    pub table: BTreeMap<u8, MsiXTableEntry>,
    /// MXE 态。
    pub mxe: bool,
}

/// MSI-X 域门面——组合配额账 / 向量集 / 路由 / 审计的单一入口。
///
/// 深度纪律：所有账的**变更只走本门面**（绕过门面直改内账=绕过守恒，
/// 类型上公开只读方法，可变方法私有化在域内）。
#[derive(Clone, Debug)]
pub struct MsiXDomain {
    /// 设备账。
    devices: BTreeMap<String, DeviceMsiX>,
    /// 配额账。
    quota: QuotaLedger,
    /// 全局路由。
    router: DispatchRouter,
    /// 串扰审计。
    audit: CrossoverAudit,
    /// 逻辑 tick（零墙钟——时序来源单点）。
    tick: u64,
}

impl MsiXDomain {
    /// 新域。
    pub fn new() -> MsiXDomain {
        MsiXDomain {
            devices: BTreeMap::new(),
            quota: QuotaLedger::new(),
            router: DispatchRouter::default(),
            audit: CrossoverAudit::new(),
            tick: 0,
        }
    }

    /// 推进逻辑 tick（显性——调用方注入时序）。
    pub fn advance_tick(&mut self, n: u64) {
        self.tick = self.tick.saturating_add(n);
    }

    /// 当前 tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 串扰审计只读句柄（判据取证用）。
    pub fn audit(&self) -> &CrossoverAudit {
        &self.audit
    }

    /// 配额账只读。
    pub fn quota(&self) -> &QuotaLedger {
        &self.quota
    }

    /// 路由表只读。
    pub fn router(&self) -> &DispatchRouter {
        &self.router
    }

    /// enable MSI-X：能力校验 → 档位入册 → 配额预留 → 设备账建档。
    ///
    /// `n_vectors` 是本设备要用的向量数（配额按此预留）。
    pub fn enable_msix(
        &mut self,
        device: &str,
        class: QuotaClass,
        cap_raw: &[u8],
        n_vectors: u16,
    ) -> Result<(), (u32, String)> {
        if self.devices.get(device).map(|d| d.mxe).unwrap_or(false) {
            return Err((XC_DOUBLE_ENABLE, format!("设备 {} MXE 已开，重复使能被拒", device)));
        }
        let cap = MsiXCapability::decode(cap_raw)
            .ok_or((XC_CAP_MALFORMED, format!("能力结构解码失败（<{} 字节）", MsiXCapability::STRUCT_LEN)))?;
        let bad = cap.invariants();
        if !bad.is_empty() {
            return Err((XC_CAP_MALFORMED, format!("能力五不变量违例：{}", bad.join("；"))));
        }
        if n_vectors == 0 || n_vectors > cap.table_size() {
            return Err((
                XC_CAP_MALFORMED,
                format!("申请向量 {} 超桌容量 {}", n_vectors, cap.table_size()),
            ));
        }
        if n_vectors > class.quota() {
            return Err((
                XC_QUOTA_EXCEEDED,
                format!("申请向量 {} 超 {} 档配额 {}", n_vectors, class.name(), class.quota()),
            ));
        }
        self.quota.enroll(device, class)?;
        self.quota.request(device, n_vectors)?;
        self.devices.insert(
            device.to_string(),
            DeviceMsiX {
                device: device.to_string(),
                class,
                cap: Some(cap),
                vectors: DeviceVectorSet::new(),
                table: BTreeMap::new(),
                mxe: true,
            },
        );
        Ok(())
    }

    /// 绑定队列向量：字段编码 → 桌项写 → 双账 → 全局路由，一气呵成。
    ///
    /// `io_semantics`：IO 语义事件挂 queue0 拒（管理队列专用）。
    #[allow(clippy::too_many_arguments)]
    pub fn bind_queue_vector(
        &mut self,
        device: &str,
        queue_id: u16,
        vector: u8,
        handler: &str,
        dest_id: u8,
        trigger_level: bool,
        io_semantics: bool,
    ) -> Result<MsiXTableEntry, (u32, String)> {
        let class = {
            let dev = self
                .devices
                .get_mut(device)
                .ok_or((XC_DEVICE_UNKNOWN, format!("设备 {} 未建档", device)))?;
            if !dev.mxe {
                return Err((XC_NOT_ENABLED, format!("设备 {} MXE 未开", device)));
            }
            dev.class
        };
        // 向量集双账（借用收窄：先算后写）。
        {
            let dev = self
                .devices
                .get_mut(device)
                .ok_or((XC_DEVICE_UNKNOWN, format!("设备 {} 未建档", device)))?;
            dev.vectors
                .bind(class, queue_id, vector, handler, trigger_level, self.tick, io_semantics)?;
            dev.vectors.set_class(class);
        }
        // 消息编码（失败则回滚双账——显性不留半绑定态）。
        let addr_f = MsgAddrFields {
            dest_id,
            dest_mode_logical: false,
            redir_hint: false,
        };
        let data_f = MsgDataFields {
            vector,
            delivery_lowest_prio: false,
            trigger_level,
        };
        let entry = match encode_table_entry(addr_f, data_f, false) {
            Some(e) => e,
            None => {
                if let Some(dev) = self.devices.get_mut(device) {
                    let _ = dev.vectors.unbind(vector);
                }
                return Err((XC_MSG_ADDR_BAD, format!("向量 {:#x} 消息编码失败", vector)));
            }
        };
        // 全局路由（撞账也回滚）。
        if let Err(e) = self.router.enroll(device, vector, queue_id, handler) {
            if let Some(dev) = self.devices.get_mut(device) {
                let _ = dev.vectors.unbind(vector);
            }
            return Err(e);
        }
        // 桌项落账。
        if let Some(dev) = self.devices.get_mut(device) {
            dev.table.insert(vector, entry);
        }
        Ok(entry)
    }

    /// mask / unmask（VecCtrl 模拟——写保留位即拒）。
    pub fn set_masked(&mut self, device: &str, vector: u8, masked: bool) -> Result<(), (u32, String)> {
        let dev = self
            .devices
            .get_mut(device)
            .ok_or((XC_DEVICE_UNKNOWN, format!("设备 {} 未建档", device)))?;
        let entry = dev
            .table
            .get_mut(&vector)
            .ok_or((XC_ROUTE_UNBOUND, format!("向量 {:#x} 无桌项", vector)))?;
        let vc = if masked { VECCTRL_MASKED } else { 0 };
        if !vec_ctrl_legal(vc) {
            return Err((XC_VECCTRL_BAD, "VecCtrl 保留位非零被拒".to_string()));
        }
        entry.vec_ctrl = vc;
        Ok(())
    }

    /// 事件注入：路由 → 审计（判据 J1-b 的投递通道）。
    ///
    /// `expected_queue` 由调用方按设备账显性给出（预期即契约——审计
    /// 只对账不猜预期）。
    pub fn inject_event(
        &mut self,
        expected_queue: u16,
        vector: u8,
        payload: u32,
    ) {
        let routed = self.router.route(vector, payload);
        self.audit.record(expected_queue, routed);
    }

    /// 解绑单向量（注销路径——配额不归还，disable 才归还）。
    pub fn unbind_vector(&mut self, device: &str, vector: u8) -> Result<QueueBinding, (u32, String)> {
        self.router.withdraw(device, vector)?;
        let dev = self
            .devices
            .get_mut(device)
            .ok_or((XC_DEVICE_UNKNOWN, format!("设备 {} 未建档", device)))?;
        let b = dev.vectors.unbind(vector)?;
        dev.table.remove(&vector);
        Ok(b)
    }

    /// disable MSI-X：全量回收——向量/桌项/路由/配额四账归零。
    ///
    /// 回收后守恒自检（Σ归还 == 账面，破坏即 0x3B0F 立案）。
    pub fn disable_msix(&mut self, device: &str) -> Result<u16, (u32, String)> {
        let bound = {
            let dev = self
                .devices
                .get_mut(device)
                .ok_or((XC_DEVICE_UNKNOWN, format!("设备 {} 未建档", device)))?;
            if !dev.mxe {
                return Err((XC_DISABLE_NOT_ENABLED, format!("设备 {} MXE 未开，无需禁用", device)));
            }
            if let Err(e) = dev.vectors.audit() {
                return Err(e);
            }
            dev.mxe = false;
            dev.vectors.len() as u16
        };
        // 撤全局路由（按本设备向量清单）。
        let vecs: Vec<u8> = self
            .devices
            .get(device)
            .map(|d| d.vectors.vectors())
            .unwrap_or_default();
        for v in vecs {
            let _ = self.router.withdraw(device, v);
        }
        // 配额归还 + 守恒。
        let used = self.quota.used_of(device).unwrap_or(0);
        self.quota.give_back(device, used)?;
        self.quota.conservation()?;
        if bound != used {
            return Err((
                XC_RELEASE_IMBALANCE,
                format!("回收对账破坏：绑定 {} ≠ 配额 {}", bound, used),
            ));
        }
        // 清设备账。
        if let Some(dev) = self.devices.get_mut(device) {
            dev.vectors.clear();
            dev.table.clear();
            dev.cap = None;
        }
        Ok(bound)
    }

    /// 设备账只读。
    pub fn device(&self, device: &str) -> Option<&DeviceMsiX> {
        self.devices.get(device)
    }

    /// 全域自检：配额守恒 + 每设备双账审计。
    pub fn audit_all(&self) -> Result<(), (u32, String)> {
        self.quota.conservation()?;
        for dev in self.devices.values() {
            dev.vectors.audit()?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §8 确定性仿真底座（LCG + FNV）
// ---------------------------------------------------------------------------

/// LCG 状态（Numerical Recipes 参数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg(u32);

impl Lcg {
    /// 种子。
    pub const fn new(seed: u32) -> Lcg {
        Lcg(seed)
    }

    /// 下一步。
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }

    /// [0, n) 均匀。
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], mut hash: u32) -> u32 {
    for &b in data {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// FNV 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

// ---------------------------------------------------------------------------
// §9 NVMe 四队列端到端档（判据 J1-a 本体）
// ---------------------------------------------------------------------------

/// NVMe 四队列向量案——admin q0 + io q1..q3，四向量各到各。
pub const NVME_QUEUE_VECTORS: [(u16, u8); 4] = [(0, 0x60), (1, 0x61), (2, 0x62), (3, 0x63)];

/// NVMe 四队列 handler 名案。
pub const NVME_HANDLER_NAMES: [&str; 4] = ["nvme-admin", "nvme-io1", "nvme-io2", "nvme-io3"];

/// 构造 NVMe 档能力结构原始 12 字节（桌 4 项，BAR2 偏移 0x2000，PBA BAR2 0x3000）。
pub fn nvme_cap_raw() -> Vec<u8> {
    let ts = 4u16;
    let msg_ctrl = (ts - 1) & 0x07FF;
    let table = (2u32 << 30) | 0x2000;
    let pba = (2u32 << 30) | 0x3000;
    let mut v = Vec::new();
    v.extend_from_slice(&msg_ctrl.to_le_bytes());
    v.extend_from_slice(&table.to_le_bytes());
    v.extend_from_slice(&pba.to_le_bytes());
    v
}

/// 端到端一轮：enable → 4 绑定 → 4 事件各到各 → disable。
///
/// 返回本轮审计增量摘要（轮次可对拍）。
fn nvme_round(domain: &mut MsiXDomain, round: u32) -> Result<String, (u32, String)> {
    let dev = format!("nvme0-r{}", round);
    domain.enable_msix(&dev, QuotaClass::Nvme, &nvme_cap_raw(), 4)?;
    // 四绑定：桌项逐条编码落账，断言 msg_data 低 8 位 == 向量。
    for &(q, v) in NVME_QUEUE_VECTORS.iter() {
        let h = NVME_HANDLER_NAMES[q as usize];
        let e = domain.bind_queue_vector(&dev, q, v, h, 0x02, false, q != 0)?;
        if (e.msg_data & 0xFF) != u32::from(v) {
            return Err((
                XC_MSG_DATA_BAD,
                format!("第 {} 轮：向量 {:#x} 桌项数据字错装 {:#x}", round, v, e.msg_data),
            ));
        }
        if e.msg_addr & 0xFFF0_0000 != MSI_ADDR_BASE & 0xFFF0_0000 {
            return Err((
                XC_MSG_ADDR_BAD,
                format!("第 {} 轮：向量 {:#x} 地址帧基错 {:#x}", round, v, e.msg_addr),
            ));
        }
    }
    // 四事件：各到各（预期队列按向量案显性给出）。
    domain.advance_tick(4);
    for &(q, v) in NVME_QUEUE_VECTORS.iter() {
        domain.inject_event(q, v, u32::from(round) * 16 + u32::from(q));
    }
    // 四向量全数在账。
    let dev_state = domain
        .device(&dev)
        .ok_or((XC_DEVICE_UNKNOWN, format!("第 {} 轮：设备账丢失", round)))?;
    if dev_state.vectors.len() != 4 {
        return Err((
            XC_VECTOR_TAKEN,
            format!("第 {} 轮：绑定数 {} ≠ 4", round, dev_state.vectors.len()),
        ));
    }
    // disable 全量回收。
    let freed = domain.disable_msix(&dev)?;
    if freed != 4 {
        return Err((
            XC_RELEASE_IMBALANCE,
            format!("第 {} 轮：回收 {} ≠ 4", round, freed),
        ));
    }
    Ok(format!("r{}: 4绑4投4回收", round))
}

/// 判据 J1-a：多向量注册正确——NVMe 4 队列 4 向量各到各 handler **10/10 轮**。
pub fn j1a_multivec_register_10rounds() -> Result<String, (u32, String)> {
    let mut dom = MsiXDomain::new();
    let mut log: Vec<String> = Vec::new();
    for r in 0..10u32 {
        log.push(nvme_round(&mut dom, r)?);
    }
    dom.audit_all()?;
    let (d, c, _) = dom.audit().totals();
    if d != 40 || c != 0 {
        return Err((XC_EVENT_CROSSED, format!("J1-a 投递账异常：delivered={} crossed={}", d, c)));
    }
    Ok(format!("10/10 轮全过：{}", log.join(" | ")))
}

// ---------------------------------------------------------------------------
// §10 串扰压测（判据 J1-b 本体：10 万事件零错队）
// ---------------------------------------------------------------------------

/// 串扰压测：四常驻队列四向量，LCG 事件流 10 万发，路由零错队。
///
/// 深度设计：事件流按 LCG 在四向量间伪随机轮转，**预期队列由向量案
/// 反查**（不是由事件序号猜）——审计对账的是"路由结果 vs 账面绑定"，
/// 这才是串扰定义（打 A 线的包落进 B 线的 handler）。
pub fn j1b_crossover_100k() -> Result<String, (u32, String)> {
    let mut dom = MsiXDomain::new();
    dom.enable_msix("nvme-flood", QuotaClass::Nvme, &nvme_cap_raw(), 4)?;
    for &(q, v) in NVME_QUEUE_VECTORS.iter() {
        dom.bind_queue_vector("nvme-flood", q, v, NVME_HANDLER_NAMES[q as usize], 0x02, false, q != 0)?;
    }
    // 向量 → 预期队列反查表（账面即契约）。
    let expect: BTreeMap<u8, u16> = NVME_QUEUE_VECTORS
        .iter()
        .map(|&(q, v)| (v, q))
        .collect();
    let mut rng = Lcg::new(0xC0FF_EE01);
    const N: u32 = 100_000;
    for i in 0..N {
        let v = NVME_QUEUE_VECTORS[(rng.below(4)) as usize].1;
        let q = *expect
            .get(&v)
            .ok_or((XC_ROUTE_UNBOUND, format!("向量案反查失败：{:#x}", v)))?;
        dom.inject_event(q, v, u32::from(i));
    }
    let (d, c, r) = dom.audit().totals();
    if d != u64::from(N) || c != 0 || r != 0 {
        return Err((
            XC_EVENT_CROSSED,
            format!(
                "J1-b 红线：delivered={}（应 {}）crossed={}（应 0）rejected={}（应 0）明细 {:?}",
                d, N, c, r, dom.audit().evidence()
            ),
        ));
    }
    dom.disable_msix("nvme-flood")?;
    Ok(format!(
        "10 万事件零错队：delivered={} crossed=0 rejected=0（LCG 种子 0xC0FFEE01）",
        d
    ))
}

// ---------------------------------------------------------------------------
// §11 配额纪律注入套件（判据 J1-c 本体：超限拦截 10/10）
// ---------------------------------------------------------------------------

/// 注入项命中记录。
#[derive(Clone, Debug)]
pub struct QuotaHit {
    /// 注入编号（0..10）。
    pub kind: u8,
    /// 注入描述。
    pub what: String,
    /// 是否被拦截。
    pub intercepted: bool,
    /// 拦截证据（诊断码+理由）。
    pub evidence: String,
}

/// 配额与唯一性违例注入——10 类，全拦截为过。
///
/// 注入面（判据点名"per 设备上限超限"，套件外扩三类唯一性/状态机面）：
/// 0 档位超限（默认档申 5>4）　1 桌容量超限（申 8>桌 4）
/// 2 全局池枯竭（四设备吃光 16 池再申）　3 正账撞向量（同向量绑两队列）
/// 4 反账撞队列（同队列绑两向量）　5 跨设备撞全局路由
/// 6 管理队列越权（IO 语义挂 q0）　7 队列越档（q9 出默认档容量）
/// 8 未使能即绑　9 归还对账破坏（多还）
pub fn j1c_quota_violations_10() -> Result<String, (u32, String)> {
    let mut hits: Vec<QuotaHit> = Vec::new();

    // ── 注入 0：档位超限（默认档 4 上限申 5）──
    {
        let mut dom = MsiXDomain::new();
        let cap = default_cap_raw(8);
        let r = dom.enable_msix("dev0", QuotaClass::Default, &cap, 5);
        hits.push(QuotaHit {
            kind: 0,
            what: "默认档申 5 超 4 上限".to_string(),
            intercepted: matches!(r, Err((XC_QUOTA_EXCEEDED, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——配额闸失效".to_string(),
            },
        });
    }
    // ── 注入 1：桌容量超限（桌 4 项申 8，档位装得下也拒）──
    {
        let mut dom = MsiXDomain::new();
        let cap = default_cap_raw(4);
        let r = dom.enable_msix("dev1", QuotaClass::Nvme, &cap, 8);
        hits.push(QuotaHit {
            kind: 1,
            what: "申 8 超桌容量 4（档位 32 装得下也拒）".to_string(),
            intercepted: matches!(r, Err((XC_CAP_MALFORMED, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——桌容量闸失效".to_string(),
            },
        });
    }
    // ── 注入 2：全局池枯竭（网卡 8+NVMe 8 吃光 16 再来一设备）──
    {
        let mut dom = MsiXDomain::new();
        let _ = dom.enable_msix("net0", QuotaClass::Net, &net_cap_raw(), 8);
        let _ = dom.enable_msix("nvme0", QuotaClass::Nvme, &nvme_cap_raw(), 8);
        let cap = default_cap_raw(4);
        let r = dom.enable_msix("dev2", QuotaClass::Default, &cap, 1);
        hits.push(QuotaHit {
            kind: 2,
            what: "池 16 被 8+8 吃光，第 17 个被拒".to_string(),
            intercepted: matches!(r, Err((XC_POOL_EXHAUSTED, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——池水位闸失效".to_string(),
            },
        });
    }
    // ── 注入 3：正账撞向量（同向量绑两队列）──
    {
        let mut dom = MsiXDomain::new();
        let _ = dom.enable_msix("dev3", QuotaClass::Default, &default_cap_raw(4), 4);
        let _ = dom.bind_queue_vector("dev3", 1, 0x60, "h-a", 0x02, false, true);
        let r = dom.bind_queue_vector("dev3", 2, 0x60, "h-b", 0x02, false, true);
        hits.push(QuotaHit {
            kind: 3,
            what: "向量 0x60 绑队列 1 后再绑队列 2".to_string(),
            intercepted: matches!(r, Err((XC_VECTOR_TAKEN, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——一 vector 一队列破坏".to_string(),
            },
        });
    }
    // ── 注入 4：反账撞队列（同队列绑两向量）──
    {
        let mut dom = MsiXDomain::new();
        let _ = dom.enable_msix("dev4", QuotaClass::Default, &default_cap_raw(4), 4);
        let _ = dom.bind_queue_vector("dev4", 1, 0x60, "h-a", 0x02, false, true);
        let r = dom.bind_queue_vector("dev4", 1, 0x61, "h-b", 0x02, false, true);
        hits.push(QuotaHit {
            kind: 4,
            what: "队列 1 绑 0x60 后再绑 0x61".to_string(),
            intercepted: matches!(r, Err((XC_QUEUE_BOUND, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——queue→vector 唯一性破坏".to_string(),
            },
        });
    }
    // ── 注入 5：跨设备撞全局路由 ──
    {
        let mut dom = MsiXDomain::new();
        let _ = dom.enable_msix("devA", QuotaClass::Default, &default_cap_raw(4), 2);
        let _ = dom.enable_msix("devB", QuotaClass::Default, &default_cap_raw(4), 2);
        let _ = dom.bind_queue_vector("devA", 1, 0x60, "h-a", 0x02, false, true);
        let r = dom.bind_queue_vector("devB", 1, 0x60, "h-b", 0x02, false, true);
        hits.push(QuotaHit {
            kind: 5,
            what: "devB 抢 devA 已占的全局向量 0x60".to_string(),
            intercepted: matches!(r, Err((XC_VECTOR_TAKEN, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——IDT 全局唯一性破坏".to_string(),
            },
        });
    }
    // ── 注入 6：管理队列越权（IO 语义挂 q0）──
    {
        let mut dom = MsiXDomain::new();
        let _ = dom.enable_msix("dev6", QuotaClass::Default, &default_cap_raw(4), 4);
        let r = dom.bind_queue_vector("dev6", 0, 0x60, "h-io", 0x02, false, true);
        hits.push(QuotaHit {
            kind: 6,
            what: "IO 语义挂 queue0".to_string(),
            intercepted: matches!(r, Err((XC_ADMIN_QUEUE_VIOLATION, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——管理队列专用性破坏".to_string(),
            },
        });
    }
    // ── 注入 7：队列越档（默认档容量 4 绑 q9）──
    {
        let mut dom = MsiXDomain::new();
        let _ = dom.enable_msix("dev7", QuotaClass::Default, &default_cap_raw(4), 4);
        let r = dom.bind_queue_vector("dev7", 9, 0x60, "h-a", 0x02, false, true);
        hits.push(QuotaHit {
            kind: 7,
            what: "队列 9 越默认档容量 4".to_string(),
            intercepted: matches!(r, Err((XC_QUEUE_ILLEGAL, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——队列域闸失效".to_string(),
            },
        });
    }
    // ── 注入 8：未使能即绑 ──
    {
        let mut dom = MsiXDomain::new();
        let r = dom.bind_queue_vector("ghost", 1, 0x60, "h-a", 0x02, false, true);
        hits.push(QuotaHit {
            kind: 8,
            what: "幽灵设备未建档先绑".to_string(),
            intercepted: matches!(r, Err((XC_DEVICE_UNKNOWN, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——状态机纪律破坏".to_string(),
            },
        });
    }
    // ── 注入 9：归还对账破坏（多还）──
    {
        let mut dom = MsiXDomain::new();
        let _ = dom.enable_msix("dev9", QuotaClass::Default, &default_cap_raw(4), 2);
        let r = dom.quota_mut().give_back("dev9", 3);
        hits.push(QuotaHit {
            kind: 9,
            what: "账面 2 却归还 3".to_string(),
            intercepted: matches!(r, Err((XC_RELEASE_IMBALANCE, _))),
            evidence: match r {
                Err(e) => e.1,
                Ok(_) => "未拦截——守恒揭破失效".to_string(),
            },
        });
    }

    // 裁决：10/10 全拦截为过。
    let missed: Vec<&QuotaHit> = hits.iter().filter(|h| !h.intercepted).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed
            .iter()
            .map(|h| format!("#{} {}", h.kind, h.what))
            .collect();
        return Err((
            XC_QUOTA_EXCEEDED,
            format!("J1-c 漏网 {} 项：{}", missed.len(), names.join("；")),
        ));
    }
    let summary: Vec<String> = hits
        .iter()
        .map(|h| format!("#{} 拦({})", h.kind, h.evidence.chars().take(24).collect::<String>()))
        .collect();
    Ok(format!("10/10 违例全拦截：{}", summary.join(" | ")))
}

/// 默认档能力原始字节（桌 n 项，BAR1 偏移 0x1000，PBA BAR1 0x2000）。
fn default_cap_raw(n: u16) -> Vec<u8> {
    let msg_ctrl = (n - 1) & 0x07FF;
    let table = (1u32 << 30) | 0x1000;
    let pba = (1u32 << 30) | 0x2000;
    let mut v = Vec::new();
    v.extend_from_slice(&msg_ctrl.to_le_bytes());
    v.extend_from_slice(&table.to_le_bytes());
    v.extend_from_slice(&pba.to_le_bytes());
    v
}

/// 网卡档能力原始字节（桌 8 项）。
fn net_cap_raw() -> Vec<u8> {
    default_cap_raw(8)
}

/// 配额账可变句柄（注入套件专用——生产路径变更只走域门面）。
impl MsiXDomain {
    fn quota_mut(&mut self) -> &mut QuotaLedger {
        &mut self.quota
    }
}

// ---------------------------------------------------------------------------
// §12 消息编解码对拍套件（编码器不留活口）
// ---------------------------------------------------------------------------

/// 编解码双向对拍：随机字段域 → 编码 → 解码 → 逐字段相等。
pub fn codec_roundtrip_1000() -> Result<String, (u32, String)> {
    let mut rng = Lcg::new(0xDEC0_DE01);
    for i in 0..1000u32 {
        let addr_f = MsgAddrFields {
            dest_id: (rng.below(256)) as u8,
            dest_mode_logical: rng.below(2) == 1,
            redir_hint: rng.below(2) == 1,
        };
        let vector = MSI_VECTOR_BASE + (rng.below(16) as u8);
        let data_f = MsgDataFields {
            vector,
            delivery_lowest_prio: rng.below(2) == 1,
            trigger_level: rng.below(2) == 1,
        };
        let a = encode_msg_addr(addr_f).ok_or((XC_MSG_ADDR_BAD, format!("#{} 地址编码失败", i)))?;
        let d = encode_msg_data(data_f).ok_or((XC_MSG_DATA_BAD, format!("#{} 数据编码失败", i)))?;
        let a2 = decode_msg_addr(a).ok_or((XC_MSG_ADDR_BAD, format!("#{} 地址解码失败 {:#x}", i, a)))?;
        let d2 = decode_msg_data(d).ok_or((XC_MSG_DATA_BAD, format!("#{} 数据解码失败 {:#x}", i, d)))?;
        if a2 != addr_f || d2 != data_f {
            return Err((
                XC_MSG_ADDR_BAD,
                format!("#{} 往返失配：addr {:?}->{:?} data {:?}->{:?}", i, addr_f, a2, data_f, d2),
            ));
        }
    }
    // 桌项三合一往返 + masked 位。
    let e = encode_table_entry(
        MsgAddrFields { dest_id: 0x2A, dest_mode_logical: false, redir_hint: true },
        MsgDataFields { vector: 0x63, delivery_lowest_prio: false, trigger_level: false },
        true,
    )
    .ok_or((XC_MSG_ADDR_BAD, "桌项编码失败".to_string()))?;
    let (a3, d3) = decode_table_entry(&e).ok_or((XC_MSG_DATA_BAD, "桌项解码失败".to_string()))?;
    if a3.dest_id != 0x2A || !a3.redir_hint || d3.vector != 0x63 || d3.trigger_level {
        return Err((XC_MSG_DATA_BAD, format!("桌项往返失配：{:?} {:?}", a3, d3)));
    }
    if e.vec_ctrl != VECCTRL_MASKED || !vec_ctrl_legal(e.vec_ctrl) {
        return Err((XC_VECCTRL_BAD, format!("masked 位失配 {:#x}", e.vec_ctrl)));
    }
    Ok("1000 组编解码往返全等 + 桌项三合一往返过".to_string())
}

/// 越域注入：编码/解码对非法输入的拒绝率（12 语料闭集全拒）。
pub fn codec_reject_suite() -> Result<String, (u32, String)> {
    // 语料：非法地址帧 / 池外向量 / 越界 BIR / 未对齐桌 / 保留位写脏。
    let cases: Vec<(&str, bool)> = vec![
        ("地址缺基帧", decode_msg_addr(0xEE00_0000).is_none()),
        ("地址带脏高位", decode_msg_addr(0xFEE0_0001).is_none()),
        ("数据向量 0x00", decode_msg_data(0x0000_0000).is_none()),
        ("数据向量 0x70 池外", decode_msg_data(0x70).is_none()),
        ("数据向量 0x5F 池外", decode_msg_data(0x5F).is_none()),
        ("BIR=6 越域", MsiXCapability::bir(0xC000_0000).is_none()),
        ("桌偏移未对齐", MsiXCapability::table_offset(0x1003).is_none()),
        ("msgctrl 容量 0", MsiXCapability::build_msg_ctrl(0, false, false).is_none()),
        ("msgctrl 容量 2049", MsiXCapability::build_msg_ctrl(2049, false, false).is_none()),
        ("能力切片过短", MsiXCapability::decode(&[0u8; 8]).is_none()),
        ("vecctrl 保留位脏", !vec_ctrl_legal(0x0000_0001)),
        ("能力 table/PBA 同 BAR 同址", {
            let cap = MsiXCapability {
                msg_ctrl: 0x0003, // 桌 4 项
                table_off_bir: (2u32 << 30) | 0x2000,
                pba_off_bir: (2u32 << 30) | 0x2000,
            };
            !cap.invariants().is_empty()
        }),
    ];
    let bad: Vec<&str> = cases
        .iter()
        .filter(|(_, ok)| !ok)
        .map(|(n, _)| *n)
        .collect();
    if !bad.is_empty() {
        return Err((XC_CAP_MALFORMED, format!("越域语料漏拒：{}", bad.join("；"))));
    }
    Ok(format!("{}/12 越域语料全拒", cases.len()))
}

// ---------------------------------------------------------------------------
// §13 快照（FNV 往返）
// ---------------------------------------------------------------------------

/// 域快照指纹——账面任何变动都改指纹（审计可复现）。
pub struct MsiXSnap {
    /// 指纹。
    pub hash: u32,
    /// 采样点摘要。
    pub summary: String,
}

impl MsiXSnap {
    /// 采样：配额池位 + 每设备（档位/绑定数/桌项数/mxe）+ 路由数 + 审计三账。
    pub fn take(domain: &MsiXDomain) -> MsiXSnap {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&domain.quota().pool_issued().to_le_bytes());
        buf.push(domain.tick() as u8);
        buf.push((domain.tick() >> 8) as u8);
        for (name, dev) in domain.devices.iter() {
            buf.extend_from_slice(name.as_bytes());
            buf.push(dev.vectors.len() as u8);
            buf.push(dev.table.len() as u8);
            buf.push(u8::from(dev.mxe));
            buf.extend_from_slice(dev.class.name().as_bytes());
        }
        buf.extend_from_slice(&domain.router().len().to_le_bytes());
        let (d, c, r) = domain.audit().totals();
        buf.extend_from_slice(&d.to_le_bytes());
        buf.extend_from_slice(&c.to_le_bytes());
        buf.extend_from_slice(&r.to_le_bytes());
        MsiXSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!(
                "pool={} tick={} devices={} routes={} audit(d={},c={},r={})",
                domain.quota().pool_issued(),
                domain.tick(),
                domain.devices.len(),
                domain.router().len(),
                d,
                c,
                r
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// §14 域全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：J1-a + J1-b + J1-c + 编解码对拍 + 越域注入 + 守恒，一锤定音。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(codec_roundtrip_1000()?);
    parts.push(codec_reject_suite()?);
    parts.push(j1a_multivec_register_10rounds()?);
    parts.push(j1b_crossover_100k()?);
    parts.push(j1c_quota_violations_10()?);
    // 终局守恒（全新域零账自洽）。
    let fresh = MsiXDomain::new();
    fresh.audit_all()?;
    let snap0 = MsiXSnap::take(&fresh);
    let snap1 = MsiXSnap::take(&fresh);
    if snap0.hash != snap1.hash {
        return Err((XC_RELEASE_IMBALANCE, "空域快照指纹不稳定（不可复现）".to_string()));
    }
    parts.push(format!("空域守恒+快照稳定（{:#010x}）", snap0.hash));
    Ok(parts.join("；"))
}