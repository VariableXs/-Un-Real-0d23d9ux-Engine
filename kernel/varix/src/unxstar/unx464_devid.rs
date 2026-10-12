//! UNX-F0464 · 设备身份账（vendor/device/revision）（CoRun·Unxreal · UNX-A1-B04 · PCI 深化之四）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0464`
//!
//! **判据 J1**：有效设备身份三元组（vendor/device/revision）账齐（100% 覆盖
//! 有效清单），值域核过。
//!
//! ## 要点一：三元组账本与采集状态机——三读一致性才入账
//!
//! 每个功能位（bdf：bus/device/function）的 vendor、device、revision 三个
//! 字段各读三次：三读同值才入账；任一漂移立案重采（上限
//! [`RESAMPLE_CAP`] 次），超限不入账——身份是账，不是猜。
//! PCI 配置读取经可注入 trait [`CfgReader`]（默认确定性构造口
//! [`DeterministicCfgBank`]），零硬访真机端口——采集与传输解耦。
//!
//! ## 要点二：vendor 0xFFFF 无效设备过滤显性
//!
//! 配置读回 0xFFFF 是 PCI 规范的"无设备"语义（全 1 回读）：该功能位
//! 显性记入无效清单（[`InvalidKind::NotPresent`]），**不计入覆盖分母**——
//! 分母 = 有效功能位集合（清单总数 - 无效位），分子 = 已入账三元组数。
//! vendor 高位预留段（高字节 0xF0..=0xFF）尾值核失败立案
//! [`DvCode::VENDOR_RESERVED_TAIL`]，同样不入分子。
//!
//! ## 要点三：值域核 + class/subclass 联动账
//!
//! vendor/device/revision 各走值域核闭函数：device 0x0000 保留值立案
//! （[`DvCode::DEVICE_RESERVED_ZERO`]）、revision 值域 0x00~0xFF（u8 全域，
//! 0x00 显性标注合法但罕见）。class/subclass 联动账从配置 0x08 双字寄存器
//! 同源提取（revision 是该寄存器低字节——三元组与联动账一体两面），
//! subclass 对 class 的合法域按 PCI SIG 类编码闭表核，越域立案
//! （[`DvCode::CLASS_LINKAGE_CONFLICT`]）。
//!
//! ## 要点四：100% 覆盖对账 + 增量重扫 diff + 双形态导出
//!
//! 覆盖 < 100% 立案缺漏清单（[`DvCode::COVERAGE_GAP`]）；重复 bdf 入账
//! 幂等（同值幂等通过，异值立案 [`DvCode::DUP_BDF_IDENTITY_DRIFT`]）；
//! 增量重扫三分账（新身份/身份漂移/消失）；身份清单导出 kv + TLV 双形态
//! （TLV 带闭集 tag 与往返解码自核）。vendor 名录表写死 PCI SIG 注册库
//! 常见条目，未知名录显性 `Unknown` 不臆造。
//!
//! ## 要点五：零 panic 面 + 诊断码独占 0xCBxx 段
//!
//! 与 unx463 桥走查（0x7Cxx）/unx461·462（0x7Bxx）/CGPU 册各段互不重叠；
//! 全文件禁 unwrap/expect/panic——所有输入不可信（配置空间可被改坏、
//! 清单可被喂坏），错误一律 Err + 中文诊断；逻辑 tick u64，禁 IO/墙钟，
//! 全确定性，禁浮点。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码闭集（独占 0xCBxx 段——建库前已 grep 全库确认 0xCB 段无主）
// ---------------------------------------------------------------------------

/// F0464 设备身份账诊断码。独占 `0xCB01..=0xCB13`，与 0x7Bxx（F0461/462）、
/// 0x7Cxx（F0463）、0x7Axx/0x7Dxx~0x7Fxx（CGPU 册）互不重叠。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DvCode(pub u16);

impl DvCode {
    /// 配置读面错误：CfgReader 返回 Err（读通道本身坏）。
    pub const CFG_READ_ERR: DvCode = DvCode(0xCB01);
    /// 配置偏移非法：非偶对齐或越过 0x3F 常规头界。
    pub const CFG_OFFSET_BAD: DvCode = DvCode(0xCB02);
    /// bdf 越界：bus/device/function 超出规范值域。
    pub const BDF_OUT_OF_RANGE: DvCode = DvCode(0xCB03);
    /// vendor 三读不一致（漂移立案重采）。
    pub const VENDOR_READ_DRIFT: DvCode = DvCode(0xCB04);
    /// device 三读不一致（漂移立案重采）。
    pub const DEVICE_READ_DRIFT: DvCode = DvCode(0xCB05);
    /// revision 三读不一致（漂移立案重采）。
    pub const REVISION_READ_DRIFT: DvCode = DvCode(0xCB06);
    /// class 双字三读不一致（联动账与三元组同源，一并三读）。
    pub const CLASS_READ_DRIFT: DvCode = DvCode(0xCB07);
    /// vendor 高位预留段：高字节 0xF0..=0xFF（尾值核失败）。
    pub const VENDOR_RESERVED_TAIL: DvCode = DvCode(0xCB08);
    /// device 0x0000 保留值核失败。
    pub const DEVICE_RESERVED_ZERO: DvCode = DvCode(0xCB09);
    /// 漂移重采超限（RESAMPLE_CAP 次仍未三读一致），放弃入账。
    pub const RESAMPLE_EXHAUSTED: DvCode = DvCode(0xCB0A);
    /// 重复 bdf 幂等冲突：同 bdf 二次入账但三元组异值。
    pub const DUP_BDF_IDENTITY_DRIFT: DvCode = DvCode(0xCB0B);
    /// 覆盖 < 100%：缺漏清单立案。
    pub const COVERAGE_GAP: DvCode = DvCode(0xCB0C);
    /// class/subclass 联动账越域（subclass 不在 class 合法域闭表内）。
    pub const CLASS_LINKAGE_CONFLICT: DvCode = DvCode(0xCB0D);
    /// 导出/解析格式坏：TLV tag 未知、长度越界、kv 字段缺失。
    pub const EXPORT_MALFORMED: DvCode = DvCode(0xCB0E);
    /// 增量重扫身份漂移：同 bdf 三元组变更。
    pub const RESCAN_IDENTITY_DRIFT: DvCode = DvCode(0xCB0F);
    /// 增量重扫消失：基线有账本次无账。
    pub const RESCAN_LOST: DvCode = DvCode(0xCB10);
    /// 功能位清单为空：分母为零无从对账。
    pub const SLOT_LIST_EMPTY: DvCode = DvCode(0xCB11);
    /// 逻辑 tick 回退：时间轴必须单调不减。
    pub const TICK_RETROGRADE: DvCode = DvCode(0xCB12);
    /// vendor 名录表自检失败：表内出现未升序或空名。
    pub const VENDOR_TABLE_SELF_BAD: DvCode = DvCode(0xCB13);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因（中文诊断，闭集穷举）。
    pub fn reason(self) -> String {
        match self {
            DvCode::CFG_READ_ERR => "配置读面错误：CfgReader 读通道返回 Err".into(),
            DvCode::CFG_OFFSET_BAD => "配置偏移非法：非偶对齐或越过常规头 0x3F 界".into(),
            DvCode::BDF_OUT_OF_RANGE => "bdf 越界：bus/device/function 超出规范值域".into(),
            DvCode::VENDOR_READ_DRIFT => "vendor 三读不一致：身份漂移，立案重采".into(),
            DvCode::DEVICE_READ_DRIFT => "device 三读不一致：身份漂移，立案重采".into(),
            DvCode::REVISION_READ_DRIFT => "revision 三读不一致：身份漂移，立案重采".into(),
            DvCode::CLASS_READ_DRIFT => "class 双字三读不一致：联动账同源读漂移".into(),
            DvCode::VENDOR_RESERVED_TAIL => "vendor 高位预留段：高字节 0xF0..0xFF 尾值核失败".into(),
            DvCode::DEVICE_RESERVED_ZERO => "device 0x0000 保留值：值域核失败".into(),
            DvCode::RESAMPLE_EXHAUSTED => "漂移重采超限：重采上限次后仍三读不一致，放弃入账".into(),
            DvCode::DUP_BDF_IDENTITY_DRIFT => "重复 bdf 幂等冲突：同 bdf 二次入账三元组异值".into(),
            DvCode::COVERAGE_GAP => "覆盖 < 100%：有效功能位存在缺漏，立案缺漏清单".into(),
            DvCode::CLASS_LINKAGE_CONFLICT => "class/subclass 联动越域：subclass 不在 class 合法域闭表".into(),
            DvCode::EXPORT_MALFORMED => "导出/解析格式坏：TLV tag 未知或长度越界或 kv 字段缺失".into(),
            DvCode::RESCAN_IDENTITY_DRIFT => "增量重扫身份漂移：同 bdf 三元组变更".into(),
            DvCode::RESCAN_LOST => "增量重扫消失：基线在册身份本次无账".into(),
            DvCode::SLOT_LIST_EMPTY => "功能位清单为空：覆盖分母为零，无从对账".into(),
            DvCode::TICK_RETROGRADE => "逻辑 tick 回退：时间轴必须单调不减".into(),
            DvCode::VENDOR_TABLE_SELF_BAD => "vendor 名录表自检失败：条目未升序或名称为空".into(),
            DvCode(_) => "未知 UNX 设备身份账域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、bdf 与值域常量——功能位是账本的钥匙
// ---------------------------------------------------------------------------

/// PCI 总线数上限（0x00..=0xFF）。
pub const BUS_MAX: u8 = 0xFF;
/// 每总线设备数上限（0x00..=0x1F）。
pub const DEV_MAX: u8 = 0x1F;
/// 每设备功能数上限（0x00..=0x07）。
pub const FUNC_MAX: u8 = 0x07;
/// 常规配置头最大偏移（0x00..=0x3F）。
pub const CFG_HEADER_MAX_OFFSET: u8 = 0x3F;
/// vendor 寄存器偏移。
pub const CFG_OFF_VENDOR: u8 = 0x00;
/// device 寄存器偏移。
pub const CFG_OFF_DEVICE: u8 = 0x02;
/// revision/class 双字寄存器偏移。
pub const CFG_OFF_REV_CLASS: u8 = 0x08;
/// bus0 全功能位清单长度：32 设备 × 8 功能 = 256 功能位。
pub const BUS0_SLOT_COUNT: usize = (DEV_MAX as usize + 1) * (FUNC_MAX as usize + 1);
/// 漂移重采上限。
pub const RESAMPLE_CAP: u8 = 3;

/// 功能位坐标（bus/device/function），账本键。序按 bus > dev > func 升序，
/// 与物理走查顺序一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bdf {
    /// 总线号。
    pub bus: u8,
    /// 设备号。
    pub dev: u8,
    /// 功能号。
    pub func: u8,
}

impl Bdf {
    /// 构造。
    pub const fn new(bus: u8, dev: u8, func: u8) -> Self {
        Bdf { bus, dev, func }
    }

    /// 规范值域核：bus 0x00..=0xFF / dev 0x00..=0x1F / func 0x00..=0x07。
    pub const fn in_domain(self) -> bool {
        self.dev <= DEV_MAX && self.func <= FUNC_MAX
    }

    /// 规范书写形态 `bb:dd.f`（16 进制大写，账面唯一形态）。
    pub fn to_text(self) -> String {
        format!(
            "{:02X}:{:02X}.{:X}",
            self.bus as u16, self.dev as u16, self.func as u16
        )
    }
}

/// bus0 全功能位清单：dev 0x00..=0x1F × func 0x00..=0x07 = 256 功能位。
///
/// 这是判据"输入的 256 功能位清单"的标准生成口——清单由调用方喂入，
/// 状态机对清单逐条采集，本函数只负责按规范序穷举。
pub fn full_bus0_slots() -> Vec<Bdf> {
    let mut out = vec![];
    let mut dev: u16 = 0;
    while dev <= DEV_MAX as u16 {
        let mut func: u16 = 0;
        while func <= FUNC_MAX as u16 {
            out.push(Bdf::new(0, dev as u8, func as u8));
            func += 1;
        }
        dev += 1;
    }
    out
}

/// 指定 bus 的全功能位清单（同 256 结构）。
pub fn full_bus_slots(bus: u8) -> Vec<Bdf> {
    let mut out = vec![];
    let mut dev: u16 = 0;
    while dev <= DEV_MAX as u16 {
        let mut func: u16 = 0;
        while func <= FUNC_MAX as u16 {
            out.push(Bdf::new(bus, dev as u8, func as u8));
            func += 1;
        }
        dev += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 三、设备身份三元组 + 值域核闭函数
// ---------------------------------------------------------------------------

/// 设备身份三元组：vendor/device/revision。
///
/// vendor 16 位 / device 16 位 / revision 8 位，与 PCI 配置空间寄存器
/// 位宽一一对应；三元组同入账同引用，任何一半漂移即非同一身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceIdentity {
    /// 供应商 ID。
    pub vendor: u16,
    /// 设备 ID。
    pub device: u16,
    /// 修订版本。
    pub revision: u8,
}

impl DeviceIdentity {
    /// 构造。
    pub const fn new(vendor: u16, device: u16, revision: u8) -> Self {
        DeviceIdentity { vendor, device, revision }
    }

    /// 账面形态 `vendor=0xVVVV device=0xDDDD revision=0xRR`。
    pub fn to_text(self) -> String {
        format!(
            "vendor=0x{:04X} device=0x{:04X} revision=0x{:02X}",
            self.vendor as u32, self.device as u32, self.revision as u32
        )
    }
}

/// class/subclass/prog-if 联动账（自配置 0x08 双字同源提取）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClassIdentity {
    /// 基类。
    pub class: u8,
    /// 子类。
    pub subclass: u8,
    /// 编程接口。
    pub prog_if: u8,
}

impl ClassIdentity {
    /// 构造。
    pub const fn new(class: u8, subclass: u8, prog_if: u8) -> Self {
        ClassIdentity { class, subclass, prog_if }
    }

    /// 账面形态 `class=0xCC subclass=0xSS prog_if=0xPP`。
    pub fn to_text(self) -> String {
        format!(
            "class=0x{:02X} subclass=0x{:02X} prog_if=0x{:02X}",
            self.class as u32, self.subclass as u32, self.prog_if as u32
        )
    }
}

/// vendor 值域三态：有效 / 预留尾段 / 无设备。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VendorDomain {
    /// 有效 vendor（含未知名录——名录查询另行走 [`vendor_name`]）。
    Ok,
    /// 高位预留段：高字节 0xF0..=0xFF（0xFFFF 另归 NotPresent）。
    ReservedTail,
    /// 无设备：读回 0xFFFF（PCI 全 1 回读语义）。
    NotPresent,
}

/// vendor 值域核：0xFFFF → 无设备；高字节 0xF0..=0xFF → 预留尾段；
/// 其余有效。尾值核先于无设备核会让 0xFFFF 落错桶——0xFFFF 判定在前。
pub const fn vendor_domain_classify(vendor: u16) -> VendorDomain {
    if vendor == 0xFFFF {
        return VendorDomain::NotPresent;
    }
    let hi = (vendor >> 8) as u8;
    if hi >= 0xF0 {
        return VendorDomain::ReservedTail;
    }
    VendorDomain::Ok
}

/// device 值域核：0x0000 是 PCI SIG 保留值，其余有效。
pub const fn device_domain_ok(device: u16) -> bool {
    device != 0x0000
}

/// revision 值域核：0x00..=0xFF 为 u8 全域恒真——显性函数不是空壳，
/// 语义是" revision 无预留空洞，0x00 合法但罕见，账面照常入账不标异常"。
/// 返回值为 true 恒定，调用方仍须走核（值域核三联的第三联在册）。
pub const fn revision_in_domain(_revision: u8) -> bool {
    true
}

/// revision 0x00 显性标注：合法但罕见（多见于占位实现），账面记
/// `revision_zero_note=true` 供审计，不算异常不算立案。
pub const fn revision_is_zero_note(revision: u8) -> bool {
    revision == 0x00
}

// ---------------------------------------------------------------------------
// 四、class/subclass 联动账闭表（来源：PCI SIG 类编码注册库）
// ---------------------------------------------------------------------------

/// PCI SIG 基类闭集：0x00..=0x13 连续段 + 0xFF（未分类/旧设备）。
pub const CLASS_CODE_SET: [u8; 21] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A,
    0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13, 0xFF,
];

/// class 人话名录（与 CLASS_CODE_SET 一一对应，PCI SIG 注册库口径）。
pub const CLASS_NAME_SET: [&str; 21] = [
    "Unclassified",
    "Mass Storage",
    "Network",
    "Display",
    "Multimedia",
    "Memory",
    "Bridge",
    "Simple Comm",
    "Base Peripheral",
    "Input",
    "Docking",
    "Processor",
    "Serial Bus",
    "Wireless",
    "Intelligent IO",
    "Satellite",
    "Encryption",
    "Signal Proc",
    "Proc Accelerator",
    "Non-Essential",
    "Unassigned",
];

/// subclass 对 class 的合法域闭表（重点类枚举；未列 class 全域合法）。
/// 每项：(class, 合法 subclass 列表)。来源 PCI SIG 注册库。
const SUBCLASS_DOMAIN_TABLE: [(u8, &[u8]); 8] = [
    // 0x01 大容量存储：SCSI/IDE/Floppy/IPI/RAID/ATA/Serial ATA/SAS/Other
    (0x01, &[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x80]),
    // 0x02 网络控：Ethernet/TokenRing/FDDI/ATM/ISDN/WorldFip/PICMG/InfiniBand/Other
    (0x02, &[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x80]),
    // 0x03 显示控：VGA/XGA/3D/Other
    (0x03, &[0x00, 0x01, 0x02, 0x80]),
    // 0x04 多媒体：Video/Audio/Telephony/Other
    (0x04, &[0x00, 0x01, 0x02, 0x03, 0x80]),
    // 0x05 内存控：RAM/Flash/Other
    (0x05, &[0x00, 0x01, 0x80]),
    // 0x06 桥：Host/ISA/EISA/MCA/PCI/PCMCIA/NuBus/CardBus/RaceWay/半透 PCI/InfiniBand/Other
    (0x06, &[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x80]),
    // 0x0C 串行总线：FireWire/ACCESS/SSA/USB/uWireless/PCIe/InfiniBand/Host
    (0x0C, &[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x80]),
    // 0xFF 未分类：VGA 兼容旧设备只有 0x00/0x01
    (0xFF, &[0x00, 0x01]),
];

/// 基类在闭集内核：0x00..=0x13 或 0xFF。
pub const fn class_in_set(class: u8) -> bool {
    if class == 0xFF {
        return true;
    }
    class <= 0x13
}

/// subclass 对 class 的合法域核：闭表列出者按表核，未列者全域合法。
pub fn subclass_in_domain(class: u8, subclass: u8) -> bool {
    let mut i = 0;
    while i < SUBCLASS_DOMAIN_TABLE.len() {
        let row = SUBCLASS_DOMAIN_TABLE[i];
        if row.0 == class {
            let mut j = 0;
            while j < row.1.len() {
                if let Some(&s) = row.1.get(j) {
                    if s == subclass {
                        return true;
                    }
                }
                j += 1;
            }
            return false;
        }
        i += 1;
    }
    true
}

/// class 联动账一体核：class 闭集 + subclass 合法域双关。
pub fn class_linkage_ok(ci: ClassIdentity) -> bool {
    class_in_set(ci.class) && subclass_in_domain(ci.class, ci.subclass)
}

/// class 人话名（闭表穷举，未在表内给 "Unassigned"——不臆造）。
pub fn class_name(class: u8) -> &'static str {
    let mut i = 0;
    while i < CLASS_CODE_SET.len() {
        if let Some(&c) = CLASS_CODE_SET.get(i) {
            if c == class {
                if let Some(&n) = CLASS_NAME_SET.get(i) {
                    return n;
                }
            }
        }
        i += 1;
    }
    "Unassigned"
}

// ---------------------------------------------------------------------------
// 五、vendor 名录表（来源：PCI SIG 注册库常见条目；未知显性 Unknown 不臆造）
// ---------------------------------------------------------------------------

/// vendor 名录闭表：升序排列（自检核序），`&str` 为 PCI SIG 注册库登记名。
/// 20+ 常见条目写死在此——账面直读，不查外部。
pub const VENDOR_NAME_TABLE: [(u16, &str); 26] = [
    (0x1002, "AMD/ATI"),
    (0x1022, "AMD"),
    (0x1028, "Dell"),
    (0x104C, "Texas Instruments"),
    (0x106B, "Apple"),
    (0x10DE, "NVIDIA"),
    (0x10EC, "Realtek"),
    (0x11AB, "Marvell"),
    (0x1234, "Technical Corp"),
    (0x126F, "Silicon Motion"),
    (0x1414, "Microsoft"),
    (0x144D, "Samsung"),
    (0x14C3, "MediaTek"),
    (0x14E4, "Broadcom"),
    (0x15AD, "VMware"),
    (0x15B7, "SanDisk"),
    (0x168C, "Qualcomm Atheros"),
    (0x1912, "Renesas"),
    (0x19E5, "Huawei"),
    (0x1ACE, "Enmotus"),
    (0x1AF4, "Red Hat"),
    (0x1B21, "ASMedia"),
    (0x1B36, "Red Hat"),
    (0x1B73, "Realtek"),
    (0x1D0F, "Amazon"),
    (0x8086, "Intel"),
];

/// 名录表自检：升序 + 名称非空——表自身可被改坏，先自证清白再服人。
pub fn vendor_table_self_check() -> Result<(), DvCode> {
    let mut i = 1;
    while i < VENDOR_NAME_TABLE.len() {
        let prev = match VENDOR_NAME_TABLE.get(i - 1) {
            Some(p) => *p,
            None => return Err(DvCode::VENDOR_TABLE_SELF_BAD),
        };
        let cur = match VENDOR_NAME_TABLE.get(i) {
            Some(c) => *c,
            None => return Err(DvCode::VENDOR_TABLE_SELF_BAD),
        };
        if prev.0 >= cur.0 || cur.1.is_empty() || prev.1.is_empty() {
            return Err(DvCode::VENDOR_TABLE_SELF_BAD);
        }
        i += 1;
    }
    Ok(())
}

/// vendor 人话名：名录在册回登记名；不在册显性 `Unknown`（不臆造）。
/// 预留尾段与 0xFFFF 不在此函数职责内（采集端已滤）。
pub fn vendor_name(vendor: u16) -> &'static str {
    let mut lo = 0usize;
    let mut hi = VENDOR_NAME_TABLE.len();
    // 二分：表已自检升序。
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let entry = match VENDOR_NAME_TABLE.get(mid) {
            Some(e) => *e,
            None => return "Unknown",
        };
        if entry.0 == vendor {
            return entry.1;
        }
        if entry.0 < vendor {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    "Unknown"
}

// ---------------------------------------------------------------------------
// 六、PCI 配置读取口——可注入 trait + 确定性构造口
// ---------------------------------------------------------------------------

/// PCI 配置读取口（可注入）：采集状态机只认此 trait，零硬访真机端口。
/// 实现方负责真实 CF8/CFC 或 ECAM 映射；测试与推演喂确定性账。
pub trait CfgReader {
    /// 读一个 16 位配置字（offset 须偶对齐）。
    /// 无设备语义由实现方表达：真机回全 1（0xFFFF），确定性账同构。
    fn read_cfg_word(&mut self, bdf: Bdf, offset: u8) -> Result<u16, DvCode>;
}

/// 确定性构造口：以 (bus, dev, func, offset) → word 的账面银行实现
/// [`CfgReader`]。未登记槽位回 0xFFFF（真机无设备全 1 回读同构）——
/// 确定性即"账上有什么就读到什么"。
#[derive(Debug, Clone, Default)]
pub struct DeterministicCfgBank {
    words: BTreeMap<(u8, u8, u8, u8), u16>,
}

impl DeterministicCfgBank {
    /// 空银行。
    pub fn new() -> Self {
        DeterministicCfgBank { words: BTreeMap::new() }
    }

    /// 登记一个配置字。
    pub fn set_word(&mut self, bdf: Bdf, offset: u8, word: u16) {
        self.words.insert((bdf.bus, bdf.dev, bdf.func, offset), word);
    }

    /// 便捷口：按三元组登记 vendor/device/rev+class 三个寄存器。
    pub fn set_identity(
        &mut self,
        bdf: Bdf,
        vendor: u16,
        device: u16,
        revision: u8,
        prog_if: u8,
        subclass: u8,
        class: u8,
    ) {
        self.set_word(bdf, CFG_OFF_VENDOR, vendor);
        self.set_word(bdf, CFG_OFF_DEVICE, device);
        // 0x08 双字按两个 word 登记：低 word = revision|prog_if<<8，
        // 高 word = subclass|class<<8（PCI SIG 寄存器布局 LE 同构）。
        let low = (revision as u16) | ((prog_if as u16) << 8);
        let high = (subclass as u16) | ((class as u16) << 8);
        self.set_word(bdf, CFG_OFF_REV_CLASS, low);
        self.set_word(bdf, CFG_OFF_REV_CLASS + 2, high);
    }

    /// 便捷口：登记一个"无设备"槽（等价于什么都不登记——显性写出意图）。
    pub fn set_absent(&mut self, bdf: Bdf) {
        self.set_word(bdf, CFG_OFF_VENDOR, 0xFFFF);
    }
}

impl CfgReader for DeterministicCfgBank {
    fn read_cfg_word(&mut self, bdf: Bdf, offset: u8) -> Result<u16, DvCode> {
        // 偏移须偶对齐且不越常规头界。
        if offset & 0x01 != 0 || offset > CFG_HEADER_MAX_OFFSET {
            return Err(DvCode::CFG_OFFSET_BAD);
        }
        match self.words.get(&(bdf.bus, bdf.dev, bdf.func, offset)) {
            Some(&w) => Ok(w),
            None => Ok(0xFFFF), // 未登记 = 无设备：全 1 回读同构。
        }
    }
}

// ---------------------------------------------------------------------------
// 七、身份采集状态机——三读一致性才入账
// ---------------------------------------------------------------------------

/// 采集状态机相位闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbePhase {
    /// 空闲（未开读）。
    Idle,
    /// vendor 三采中。
    VendorRd,
    /// device 三采中。
    DeviceRd,
    /// revision/class 双字三采中。
    RevisionRd,
    /// 已入账（终态）。
    Committed,
    /// 无设备（终态，显性过滤桶）。
    NotPresent,
    /// 预留尾段（终态，立案桶）。
    ReservedTail,
    /// 重采等待（漂移立案后回到下一相位）。
    ResampleWait,
    /// 重采超限（终态，放弃桶）。
    Exhausted,
}

/// 单步推进结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// 未到终态，继续 step。
    Continue,
    /// 三读一致入账：身份 + 联动账。
    Committed(DeviceIdentity, ClassIdentity),
    /// 无设备：显性过滤（不计覆盖分母）。
    NotPresent,
    /// 预留尾段：立案（不计覆盖分母）。
    ReservedTail(DvCode),
    /// 单步漂移立案（重采中）。
    Drift(DvCode),
    /// 重采超限终态。
    Exhausted(DvCode),
}

/// 三读样本罐（容量 3）。
#[derive(Debug, Clone, Copy)]
struct ReadJar {
    samples: [u32; 3],
    n: u8,
}

impl ReadJar {
    const fn empty() -> Self {
        ReadJar { samples: [0; 3], n: 0 }
    }

    fn push(&mut self, v: u32) {
        match self.n {
            0 => self.samples[0] = v,
            1 => self.samples[1] = v,
            2 => self.samples[2] = v,
            _ => {} // 计数受控恒 <=3，越界分支显性吞掉（零 panic 面）。
        }
        if self.n < 3 {
            self.n += 1;
        }
    }

    fn full(&self) -> bool {
        self.n >= 3
    }

    /// 三读一致 → Some(一致值)；不一致 → None。
    fn agree(&self) -> Option<u32> {
        if !self.full() {
            return None;
        }
        let a = self.samples[0];
        if a == self.samples[1] && a == self.samples[2] {
            Some(a)
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.n = 0;
        self.samples = [0; 3];
    }
}

/// 单功能位采集状态机。
///
/// 流程：Idle → vendor 三读 → device 三读 → revision/class 双字三读 →
/// 入账；任一相位三读不一致 → 立案 + 重采（整相位从头三读，上限
/// [`RESAMPLE_CAP`]）；vendor 首读 0xFFFF → 无设备终态（显性过滤）。
/// 已核一致值（vendor/device）在相位推进时转存 `saved_*`——重采只
/// 重采漂移相位，已核值不随罐清零而丢。
#[derive(Debug, Clone)]
pub struct ProbeSm {
    slot: Bdf,
    phase: ProbePhase,
    vendor_jar: ReadJar,
    device_jar: ReadJar,
    rev_jar: ReadJar,
    class_jar: ReadJar,
    /// 已核一致的 vendor（VendorRd 相位转存）。
    saved_vendor: u16,
    /// 已核一致的 device（DeviceRd 相位转存）。
    saved_device: u16,
    /// 重采恢复相位（ResampleWait → 回到的相位）。
    resume_phase: Option<ProbePhase>,
    resample_used: u8,
    tick_birth: u64,
    tick_last: u64,
}

impl ProbeSm {
    /// 新状态机（对某功能位）。
    pub fn new(slot: Bdf) -> Self {
        ProbeSm {
            slot,
            phase: ProbePhase::Idle,
            vendor_jar: ReadJar::empty(),
            device_jar: ReadJar::empty(),
            rev_jar: ReadJar::empty(),
            class_jar: ReadJar::empty(),
            saved_vendor: 0,
            saved_device: 0,
            resume_phase: None,
            resample_used: 0,
            tick_birth: 0,
            tick_last: 0,
        }
    }

    /// 当前相位。
    pub fn phase(&self) -> ProbePhase {
        self.phase
    }

    /// 已用重采次数。
    pub fn resample_used(&self) -> u8 {
        self.resample_used
    }

    /// bdf 只读视图。
    pub fn slot(&self) -> Bdf {
        self.slot
    }

    /// 单步推进：读一个字、喂罐、判一致。tick 单调核由调用方保证
    /// （本函数收到 tick < tick_last 即退化为不加戳——不 panic）。
    pub fn step(&mut self, reader: &mut dyn CfgReader, tick: u64) -> StepOutcome {
        // tick 回退不 panic：如实落戳（时间轴异常由编排层另案，诊断
        // 码 TICK_RETROGRADE 在册备用）。
        self.tick_last = tick;
        match self.phase {
            ProbePhase::Idle => {
                self.tick_birth = tick;
                self.phase = ProbePhase::VendorRd;
                StepOutcome::Continue
            }
            ProbePhase::VendorRd => {
                let word = match reader.read_cfg_word(self.slot, CFG_OFF_VENDOR) {
                    Ok(w) => w,
                    Err(code) => return self.exhaust_on(code),
                };
                if word == 0xFFFF {
                    // 无设备：显性过滤桶（不计覆盖分母）。
                    self.phase = ProbePhase::NotPresent;
                    return StepOutcome::NotPresent;
                }
                self.vendor_jar.push(word as u32);
                if self.vendor_jar.full() {
                    match self.vendor_jar.agree() {
                        Some(v) => {
                            let vendor = v as u16;
                            if let VendorDomain::ReservedTail = vendor_domain_classify(vendor) {
                                // 预留尾段：立案终态（不计分母）。
                                self.phase = ProbePhase::ReservedTail;
                                return StepOutcome::ReservedTail(DvCode::VENDOR_RESERVED_TAIL);
                            }
                            self.saved_vendor = vendor;
                            self.vendor_jar.reset();
                            self.phase = ProbePhase::DeviceRd;
                        }
                        None => return self.on_drift(DvCode::VENDOR_READ_DRIFT, ProbePhase::VendorRd),
                    }
                }
                StepOutcome::Continue
            }
            ProbePhase::DeviceRd => {
                let word = match reader.read_cfg_word(self.slot, CFG_OFF_DEVICE) {
                    Ok(w) => w,
                    Err(code) => return self.exhaust_on(code),
                };
                self.device_jar.push(word as u32);
                if self.device_jar.full() {
                    match self.device_jar.agree() {
                        Some(d) => {
                            let device = d as u16;
                            if !device_domain_ok(device) {
                                // device 0x0000 保留值：立案终态（不计分母）。
                                self.phase = ProbePhase::Exhausted;
                                return StepOutcome::Exhausted(DvCode::DEVICE_RESERVED_ZERO);
                            }
                            self.saved_device = device;
                            self.device_jar.reset();
                            self.phase = ProbePhase::RevisionRd;
                        }
                        None => return self.on_drift(DvCode::DEVICE_READ_DRIFT, ProbePhase::DeviceRd),
                    }
                }
                StepOutcome::Continue
            }
            ProbePhase::RevisionRd => {
                // revision 与 class 同源：低 word(0x08)=rev|prog_if<<8，
                // 高 word(0x0A)=subclass|class<<8——四值同采三读。
                let low = match reader.read_cfg_word(self.slot, CFG_OFF_REV_CLASS) {
                    Ok(w) => w,
                    Err(code) => return self.exhaust_on(code),
                };
                let high = match reader.read_cfg_word(self.slot, CFG_OFF_REV_CLASS + 2) {
                    Ok(w) => w,
                    Err(code) => return self.exhaust_on(code),
                };
                let rev = (low & 0xFF) as u32;
                let prog_if = ((low >> 8) & 0xFF) as u32;
                let subclass = (high & 0xFF) as u32;
                let class = ((high >> 8) & 0xFF) as u32;
                self.rev_jar.push(rev);
                self.class_jar.push(
                    (class << 16) | (subclass << 8) | prog_if,
                );
                if self.rev_jar.full() && self.class_jar.full() {
                    match (self.rev_jar.agree(), self.class_jar.agree()) {
                        (Some(r), Some(c)) => {
                            let revision = r as u8;
                            let prog_if = (c & 0xFF) as u8;
                            let subclass = ((c >> 8) & 0xFF) as u8;
                            let class = ((c >> 16) & 0xFF) as u8;
                            let ci = ClassIdentity::new(class, subclass, prog_if);
                            if !class_linkage_ok(ci) {
                                self.phase = ProbePhase::Exhausted;
                                return StepOutcome::Exhausted(DvCode::CLASS_LINKAGE_CONFLICT);
                            }
                            // 采集全毕：从罐中复原 vendor/device（已核有效）。
                            self.phase = ProbePhase::Committed;
                            let vendor = self.committed_vendor();
                            let device = self.committed_device();
                            return StepOutcome::Committed(
                                DeviceIdentity::new(vendor, device, revision),
                                ci,
                            );
                        }
                        (Some(_), None) => {
                            return self.on_drift(DvCode::CLASS_READ_DRIFT, ProbePhase::RevisionRd)
                        }
                        (None, Some(_)) => {
                            return self.on_drift(DvCode::REVISION_READ_DRIFT, ProbePhase::RevisionRd)
                        }
                        (None, None) => {
                            // 双罐皆不一致：按 revision 桶立案（同批重采全相位）。
                            return self.on_drift(DvCode::REVISION_READ_DRIFT, ProbePhase::RevisionRd)
                        }
                    }
                }
                StepOutcome::Continue
            }
            // 终态：重复 step 无害（幂等停机）。
            ProbePhase::Committed => StepOutcome::Continue,
            ProbePhase::NotPresent => StepOutcome::NotPresent,
            ProbePhase::ReservedTail => StepOutcome::ReservedTail(DvCode::VENDOR_RESERVED_TAIL),
            ProbePhase::ResampleWait => {
                let resume = match self.resume_phase {
                    Some(p) => p,
                    None => ProbePhase::VendorRd,
                };
                self.phase = resume;
                StepOutcome::Continue
            }
            ProbePhase::Exhausted => StepOutcome::Exhausted(DvCode::RESAMPLE_EXHAUSTED),
        }
    }

    /// 开账 tick 只读视图。
    pub fn tick_birth(&self) -> u64 {
        self.tick_birth
    }

    /// 最近 tick 只读视图。
    pub fn tick_last(&self) -> u64 {
        self.tick_last
    }

    /// 已核一致的 vendor 只读视图（相位未推进到位前为 0 占位非有效值）。
    pub fn saved_vendor(&self) -> u16 {
        self.saved_vendor
    }

    /// 已核一致的 device 只读视图（同上）。
    pub fn saved_device(&self) -> u16 {
        self.saved_device
    }

    /// vendor/device 已核值读取（Commit 复原口——saved 槽只读）。
    fn committed_vendor(&self) -> u16 {
        self.saved_vendor
    }

    fn committed_device(&self) -> u16 {
        self.saved_device
    }

    /// 漂移处理：立案 + 重采或超限。
    fn on_drift(&mut self, code: DvCode, resume: ProbePhase) -> StepOutcome {
        if self.resample_used >= RESAMPLE_CAP {
            self.phase = ProbePhase::Exhausted;
            return StepOutcome::Exhausted(DvCode::RESAMPLE_EXHAUSTED);
        }
        self.resample_used += 1;
        self.vendor_jar.reset();
        self.device_jar.reset();
        self.rev_jar.reset();
        self.class_jar.reset();
        self.phase = ProbePhase::ResampleWait;
        self.resume_phase = Some(resume);
        StepOutcome::Drift(code)
    }

    /// 读通道坏：直接超限终态（确定性通道不随机错，错了就是通道账坏）。
    fn exhaust_on(&mut self, code: DvCode) -> StepOutcome {
        self.phase = ProbePhase::Exhausted;
        StepOutcome::Exhausted(code)
    }
}

// ---------------------------------------------------------------------------
// 八、采集编排——对 256 功能位清单全流程扫账
// ---------------------------------------------------------------------------

/// 单功能位采集终账（编排层输出）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotVerdict {
    /// 入账（身份 + 联动账）。
    Admitted,
    /// 无设备（显性过滤，不计分母）。
    NotPresent,
    /// 预留尾段（立案，不计分母）。
    ReservedTail,
    /// device 保留值（立案，不计分母）。
    ReservedDeviceZero,
    /// 联动越域（立案，不计分母）。
    LinkageConflict,
    /// 重采超限（立案，缺漏桶）。
    Exhausted(DvCode),
    /// 读通道坏（立案，缺漏桶）。
    ReadErr(DvCode),
}

/// 全流程扫账报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepReport {
    /// 清单功能位总数（覆盖名单基准）。
    pub slots_total: usize,
    /// 入账数（覆盖分子）。
    pub admitted: usize,
    /// 无设备数（显性过滤，扣除分母）。
    pub not_present: usize,
    /// 预留尾段立案数。
    pub reserved_tail: usize,
    /// device 保留值立案数。
    pub reserved_zero: usize,
    /// 联动越域立案数。
    pub linkage_conflict: usize,
    /// 漂移立案事件数（含重采成功者）。
    pub drift_events: usize,
    /// 重采超限数。
    pub exhausted: usize,
    /// 读通道错误数。
    pub read_err: usize,
    /// 扫账结束 tick。
    pub tick_end: u64,
}

impl SweepReport {
    /// 有效功能位分母：清单总数 - 无设备 - 预留尾段（预留段不是有效设备）。
    pub fn denominator(&self) -> usize {
        self.slots_total - self.not_present - self.reserved_tail
    }

    /// 覆盖分子：已入账三元组数。
    pub fn numerator(&self) -> usize {
        self.admitted
    }

    /// 覆盖是否 100%。
    pub fn complete(&self) -> bool {
        self.numerator() == self.denominator()
    }
}

/// 对功能位清单全流程扫账：逐位建状态机步进至终态，按终态分桶落账。
///
/// tick 预算：每 step 消耗 1 tick，无墙钟——确定性推进。任一功能位
/// 有限步内必达终态（单功能位至多 3 读 × 4 寄存器 + 重采上限），故
/// 无死循环；预算上限 `tick_budget` 仅供防御性截断（超预算按读错误桶）。
pub fn sweep_identity(
    slots: &[Bdf],
    reader: &mut dyn CfgReader,
    ledger: &mut DeviceLedger,
    tick_budget: u64,
) -> Result<SweepReport, DvCode> {
    if slots.is_empty() {
        return Err(DvCode::SLOT_LIST_EMPTY);
    }
    let mut report = SweepReport {
        slots_total: slots.len(),
        admitted: 0,
        not_present: 0,
        reserved_tail: 0,
        reserved_zero: 0,
        linkage_conflict: 0,
        drift_events: 0,
        exhausted: 0,
        read_err: 0,
        tick_end: 0,
    };
    let mut tick: u64 = 0;
    let mut i = 0;
    while i < slots.len() {
        let slot = match slots.get(i) {
            Some(&s) => s,
            None => break,
        };
        if !slot.in_domain() {
            // bdf 越界：立案缺漏桶（越界位不可能是有效设备）。
            ledger.add_gap(slot);
            report.read_err += 1;
            i += 1;
            continue;
        }
        let mut sm = ProbeSm::new(slot);
        let mut verdict: Option<SlotVerdict> = None;
        let mut guard: u64 = 0;
        while verdict.is_none() {
            if tick >= tick_budget {
                // 防御截断：预算尽，按读通道案记（立案 + 缺漏桶）。
                ledger.add_case(slot, DvCode::CFG_READ_ERR, tick);
                ledger.add_gap(slot);
                report.read_err += 1;
                break;
            }
            guard += 1;
            if guard > 4096 {
                // 单功能位步数守卫：正常至多 ~30 步，越界即状态机账坏。
                ledger.add_case(slot, DvCode::RESAMPLE_EXHAUSTED, tick);
                ledger.add_gap(slot);
                report.exhausted += 1;
                break;
            }
            tick += 1;
            match sm.step(reader, tick) {
                StepOutcome::Continue => {}
                StepOutcome::Committed(id, ci) => {
                    match ledger.admit(slot, id, ci, tick) {
                        Ok(_) => {
                            report.admitted += 1;
                            verdict = Some(SlotVerdict::Admitted);
                        }
                        Err(DvCode::DUP_BDF_IDENTITY_DRIFT) => {
                            // 幂等冲突已立案入账本，此处计分母不变。
                            verdict = Some(SlotVerdict::Admitted);
                        }
                        Err(code) => {
                            report.read_err += 1;
                            verdict = Some(SlotVerdict::ReadErr(code));
                        }
                    }
                }
                StepOutcome::NotPresent => {
                    ledger.add_invalid(slot, InvalidKind::NotPresent);
                    report.not_present += 1;
                    verdict = Some(SlotVerdict::NotPresent);
                }
                StepOutcome::ReservedTail(code) => {
                    ledger.add_case(slot, code, tick);
                    ledger.add_invalid(slot, InvalidKind::VendorReserved);
                    report.reserved_tail += 1;
                    verdict = Some(SlotVerdict::ReservedTail);
                }
                StepOutcome::Drift(code) => {
                    ledger.add_case(slot, code, tick);
                    report.drift_events += 1;
                }
                StepOutcome::Exhausted(code) => {
                    ledger.add_case(slot, code, tick);
                    ledger.add_gap(slot);
                    match code {
                        DvCode::DEVICE_RESERVED_ZERO => {
                            report.reserved_zero += 1;
                            verdict = Some(SlotVerdict::ReservedDeviceZero);
                        }
                        DvCode::CLASS_LINKAGE_CONFLICT => {
                            report.linkage_conflict += 1;
                            verdict = Some(SlotVerdict::LinkageConflict);
                        }
                        DvCode::CFG_READ_ERR | DvCode::CFG_OFFSET_BAD => {
                            report.read_err += 1;
                            verdict = Some(SlotVerdict::ReadErr(code));
                        }
                        _ => {
                            report.exhausted += 1;
                            verdict = Some(SlotVerdict::Exhausted(code));
                        }
                    }
                }
            }
        }
        i += 1;
    }
    report.tick_end = tick;
    // 覆盖对账：<100% 立案缺漏清单。
    if !report.complete() {
        ledger.add_coverage_case(DvCode::COVERAGE_GAP, report.denominator(), report.numerator());
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// 九、设备身份账本——入账幂等 + 立案 + 覆盖对账
// ---------------------------------------------------------------------------

/// 无效槽种类（显性过滤桶——均不计覆盖分母）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidKind {
    /// 无设备：vendor 读回 0xFFFF。
    NotPresent,
    /// vendor 高位预留段。
    VendorReserved,
}

/// 单条身份账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 设备身份三元组。
    pub identity: DeviceIdentity,
    /// class/subclass 联动账。
    pub class: ClassIdentity,
    /// 首次入账 tick。
    pub first_tick: u64,
    /// 最近复核 tick。
    pub last_tick: u64,
    /// revision 0x00 显性标注（合法但罕见，审计用）。
    pub revision_zero_note: bool,
    /// vendor 人话名快照（Unknown 显性）。
    pub vendor_label: String,
}

/// 漂移立案记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftCase {
    /// 案发功能位。
    pub slot: Bdf,
    /// 诊断码。
    pub code: DvCode,
    /// 案发 tick。
    pub tick: u64,
    /// 案情补充（如覆盖缺案的分母/分子）。
    pub note: String,
}

/// 设备身份账本。
///
/// 三本账：
/// 1. `entries`——有效三元组正账（bdf 键幂等）；
/// 2. `invalid`——无效槽显性过滤桶（不计覆盖分母）；
/// 3. `cases`——漂移/越域/缺漏立案簿（审计全留痕）。
#[derive(Debug, Clone, Default)]
pub struct DeviceLedger {
    entries: BTreeMap<Bdf, LedgerEntry>,
    invalid: Vec<(Bdf, InvalidKind)>,
    cases: Vec<DriftCase>,
    gaps: Vec<Bdf>,
    /// 覆盖对账快照（最近一次）。
    last_coverage: Option<(usize, usize)>,
}

impl DeviceLedger {
    /// 空账本。
    pub fn new() -> Self {
        DeviceLedger {
            entries: BTreeMap::new(),
            invalid: Vec::new(),
            cases: Vec::new(),
            gaps: Vec::new(),
            last_coverage: None,
        }
    }

    /// 正账条数（覆盖分子口径）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 正账是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 无效桶条数。
    pub fn invalid_len(&self) -> usize {
        self.invalid.len()
    }

    /// 立案簿条数。
    pub fn cases_len(&self) -> usize {
        self.cases.len()
    }

    /// 缺漏清单（只读）。
    pub fn gaps(&self) -> &[Bdf] {
        &self.gaps
    }

    /// 立案簿（只读）。
    pub fn cases(&self) -> &[DriftCase] {
        &self.cases
    }

    /// 正账只读视图：按 bdf 升序遍历。
    pub fn iter_entries(&self) -> alloc::collections::btree_map::Iter<'_, Bdf, LedgerEntry> {
        self.entries.iter()
    }

    /// 查某功能位的账。
    pub fn entry(&self, slot: Bdf) -> Option<&LedgerEntry> {
        self.entries.get(&slot)
    }

    /// 入账：bdf 越界拒；同 bdf 同值幂等（刷新 last_tick）；同 bdf 异值
    /// 立案 DUP_BDF_IDENTITY_DRIFT（账面保留首账——首账优先，冲突只记案）。
    pub fn admit(
        &mut self,
        slot: Bdf,
        identity: DeviceIdentity,
        class: ClassIdentity,
        tick: u64,
    ) -> Result<(), DvCode> {
        if !slot.in_domain() {
            return Err(DvCode::BDF_OUT_OF_RANGE);
        }
        if let Some(existing) = self.entries.get(&slot) {
            if existing.identity == identity {
                // 幂等：同 bdf 同值——只刷新复核 tick。
                if let Some(e) = self.entries.get_mut(&slot) {
                    e.last_tick = tick;
                }
                return Ok(());
            }
            self.cases.push(DriftCase {
                slot,
                code: DvCode::DUP_BDF_IDENTITY_DRIFT,
                tick,
                note: format!(
                    "首账 {} 复账 {}",
                    existing.identity.to_text(),
                    identity.to_text()
                ),
            });
            return Err(DvCode::DUP_BDF_IDENTITY_DRIFT);
        }
        let label = vendor_name(identity.vendor);
        self.entries.insert(
            slot,
            LedgerEntry {
                identity,
                class,
                first_tick: tick,
                last_tick: tick,
                revision_zero_note: revision_is_zero_note(identity.revision),
                vendor_label: label.to_string(),
            },
        );
        Ok(())
    }

    /// 无效槽显性入桶。
    pub fn add_invalid(&mut self, slot: Bdf, kind: InvalidKind) {
        self.invalid.push((slot, kind));
    }

    /// 无效桶只读视图。
    pub fn invalid_slots(&self) -> &[(Bdf, InvalidKind)] {
        &self.invalid
    }

    /// 立案。
    pub fn add_case(&mut self, slot: Bdf, code: DvCode, tick: u64) {
        self.cases.push(DriftCase {
            slot,
            code,
            tick,
            note: code.reason(),
        });
    }

    /// 覆盖缺案立案（带分母/分子留痕）。
    pub fn add_coverage_case(&mut self, code: DvCode, denominator: usize, numerator: usize) {
        self.last_coverage = Some((denominator, numerator));
        self.cases.push(DriftCase {
            slot: Bdf::new(0, 0, 0),
            code,
            tick: 0,
            note: format!("覆盖分母={} 分子={}", denominator as u32, numerator as u32),
        });
    }

    /// 缺漏桶登记。
    pub fn add_gap(&mut self, slot: Bdf) {
        self.gaps.push(slot);
    }

    /// 覆盖快照。
    pub fn coverage(&self) -> Option<(usize, usize)> {
        self.last_coverage
    }

    /// 覆盖对账：给定有效清单，分子 = 已入账 ∩ 清单，分母 = 清单长度；
    /// 返回缺漏清单。<100% → Err(COVERAGE_GAP) + 缺漏清单在案。
    pub fn reconcile(&mut self, valid_slots: &[Bdf]) -> Result<Vec<Bdf>, DvCode> {
        if valid_slots.is_empty() {
            return Err(DvCode::SLOT_LIST_EMPTY);
        }
        let mut missing = Vec::new();
        let mut denominator = 0usize;
        let mut numerator = 0usize;
        let mut i = 0;
        while i < valid_slots.len() {
            if let Some(&slot) = valid_slots.get(i) {
                denominator += 1;
                if self.entries.contains_key(&slot) {
                    numerator += 1;
                } else {
                    missing.push(slot);
                }
            }
            i += 1;
        }
        self.last_coverage = Some((denominator, numerator));
        if numerator < denominator {
            self.gaps = missing.clone();
            self.cases.push(DriftCase {
                slot: Bdf::new(0, 0, 0),
                code: DvCode::COVERAGE_GAP,
                tick: 0,
                note: format!(
                    "缺漏 {} 位：分母={} 分子={}",
                    missing.len() as u32, denominator as u32, numerator as u32
                ),
            });
            return Err(DvCode::COVERAGE_GAP);
        }
        self.gaps.clear();
        Ok(missing)
    }
}

// ---------------------------------------------------------------------------
// 十、身份清单导出——kv + TLV 双形态（往返自核）
// ---------------------------------------------------------------------------

/// TLV tag 闭集。
pub mod tlv {
    /// 正账项头：8 字节 bus,dev,func,vendor_lo,vendor_hi,dev_lo,dev_hi,rev。
    pub const TAG_ENTRY: u8 = 0x01;
    /// 联动账：4 字节 class,subclass,prog_if,reserved(0)。
    pub const TAG_CLASS: u8 = 0x02;
    /// vendor 人话名：ASCII 变长。
    pub const TAG_VENDOR_LABEL: u8 = 0x03;
    /// 正账项尾：0 字节。
    pub const TAG_ENTRY_END: u8 = 0x04;
    /// 无效槽：4 字节 bus,dev,func,kind(0=NotPresent,1=VendorReserved)。
    pub const TAG_INVALID: u8 = 0x05;
    /// 账尾：0 字节。
    pub const TAG_LEDGER_END: u8 = 0x06;
}

/// kv 形态导出：一行一条 `k=v` 空格分隔，段首 `#` 注释。
pub fn export_kv(ledger: &DeviceLedger) -> String {
    let mut out = String::new();
    out.push_str("# UNX-F0464 设备身份账 kv 形态\n");
    out.push_str(&format!("# entries={} invalid={} cases={}\n",
        ledger.len() as u32, ledger.invalid_len() as u32, ledger.cases_len() as u32));
    for (slot, entry) in ledger.iter_entries() {
        out.push_str(&format!(
            "bdf={} {} {} vendor_name={} first_tick={} last_tick={}\n",
            slot.to_text(),
            entry.identity.to_text(),
            entry.class.to_text(),
            entry.vendor_label,
            entry.first_tick as u32,
            entry.last_tick as u32,
        ));
    }
    out.push_str("# 无效槽（显性过滤，不计覆盖分母）\n");
    for (slot, kind) in ledger.invalid_slots() {
        let kind_text = match kind {
            InvalidKind::NotPresent => "not_present",
            InvalidKind::VendorReserved => "vendor_reserved",
        };
        out.push_str(&format!("invalid_bdf={} kind={}\n", slot.to_text(), kind_text));
    }
    out
}

/// TLV 形态导出：`[tag, len_lo, len_hi, value...]` 序列。
pub fn export_tlv(ledger: &DeviceLedger) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let push_tlv = |out: &mut Vec<u8>, tag: u8, value: &[u8]| {
        out.push(tag);
        let len = value.len() as u16;
        out.push((len & 0xFF) as u8);
        out.push(((len >> 8) & 0xFF) as u8);
        let mut i = 0;
        while i < value.len() {
            if let Some(&b) = value.get(i) {
                out.push(b);
            }
            i += 1;
        }
    };
    for (slot, entry) in ledger.iter_entries() {
        let mut head: Vec<u8> = Vec::new();
        head.push(slot.bus);
        head.push(slot.dev);
        head.push(slot.func);
        head.push((entry.identity.vendor & 0xFF) as u8);
        head.push(((entry.identity.vendor >> 8) & 0xFF) as u8);
        head.push((entry.identity.device & 0xFF) as u8);
        head.push(((entry.identity.device >> 8) & 0xFF) as u8);
        head.push(entry.identity.revision);
        push_tlv(&mut out, tlv::TAG_ENTRY, &head);
        let class_val = [entry.class.class, entry.class.subclass, entry.class.prog_if, 0];
        push_tlv(&mut out, tlv::TAG_CLASS, &class_val);
        push_tlv(&mut out, tlv::TAG_VENDOR_LABEL, entry.vendor_label.as_bytes());
        push_tlv(&mut out, tlv::TAG_ENTRY_END, &[]);
    }
    for (slot, kind) in ledger.invalid_slots() {
        let kind_byte = match kind {
            InvalidKind::NotPresent => 0u8,
            InvalidKind::VendorReserved => 1u8,
        };
        let val = [slot.bus, slot.dev, slot.func, kind_byte];
        push_tlv(&mut out, tlv::TAG_INVALID, &val);
    }
    push_tlv(&mut out, tlv::TAG_LEDGER_END, &[]);
    out
}

/// TLV 解码回账（往返自核）：tag 未知/长度越界/字段残缺 → EXPORT_MALFORMED。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodedLedger {
    /// 正账项（bdf → 身份/联动账/vendor 名）。
    pub entries: Vec<(Bdf, DeviceIdentity, ClassIdentity, String)>,
    /// 无效槽。
    pub invalid: Vec<(Bdf, InvalidKind)>,
}

/// TLV 解析。入口先核账尾存在（结构性完整），再逐项解。
pub fn decode_tlv(bytes: &[u8]) -> Result<DecodedLedger, DvCode> {
    let mut out = DecodedLedger::default();
    // 结构预检：账尾 tag 必须在案（否则流被截）。
    let mut has_end = false;
    let mut pos = 0usize;
    while pos + 3 <= bytes.len() {
        let tag = match bytes.get(pos) {
            Some(&t) => t,
            None => return Err(DvCode::EXPORT_MALFORMED),
        };
        let lo = match bytes.get(pos + 1) {
            Some(&t) => t as usize,
            None => return Err(DvCode::EXPORT_MALFORMED),
        };
        let hi = match bytes.get(pos + 2) {
            Some(&t) => t as usize,
            None => return Err(DvCode::EXPORT_MALFORMED),
        };
        let len = lo | (hi << 8);
        let vstart = pos + 3;
        let vend = match vstart.checked_add(len) {
            Some(v) => v,
            None => return Err(DvCode::EXPORT_MALFORMED),
        };
        if vend > bytes.len() {
            return Err(DvCode::EXPORT_MALFORMED);
        }
        if tag == tlv::TAG_LEDGER_END {
            if len != 0 {
                return Err(DvCode::EXPORT_MALFORMED);
            }
            has_end = true;
        }
        pos = vend;
    }
    if !has_end {
        return Err(DvCode::EXPORT_MALFORMED);
    }
    // 逐项解。
    let mut pos = 0usize;
    // 项内暂存（entry 头先到，随后 class/label/end）。
    let mut pend_slot: Option<Bdf> = None;
    let mut pend_identity: Option<DeviceIdentity> = None;
    let mut pend_class: Option<ClassIdentity> = None;
    let mut pend_label: Option<String> = None;
    while pos + 3 <= bytes.len() {
        let tag = match bytes.get(pos) {
            Some(&t) => t,
            None => return Err(DvCode::EXPORT_MALFORMED),
        };
        let lo = match bytes.get(pos + 1) {
            Some(&t) => t as usize,
            None => return Err(DvCode::EXPORT_MALFORMED),
        };
        let hi = match bytes.get(pos + 2) {
            Some(&t) => t as usize,
            None => return Err(DvCode::EXPORT_MALFORMED),
        };
        let len = lo | (hi << 8);
        let vstart = pos + 3;
        let vend = vstart + len;
        if vend > bytes.len() {
            return Err(DvCode::EXPORT_MALFORMED);
        }
        match tag {
            tlv::TAG_ENTRY => {
                if len != 8 {
                    return Err(DvCode::EXPORT_MALFORMED);
                }
                let g = |i: usize| -> Result<u8, DvCode> {
                    match bytes.get(vstart + i) {
                        Some(&b) => Ok(b),
                        None => Err(DvCode::EXPORT_MALFORMED),
                    }
                };
                let bus = g(0)?;
                let dev = g(1)?;
                let func = g(2)?;
                let v_lo = g(3)? as u16;
                let v_hi = g(4)? as u16;
                let d_lo = g(5)? as u16;
                let d_hi = g(6)? as u16;
                let rev = g(7)?;
                pend_slot = Some(Bdf::new(bus, dev, func));
                pend_identity = Some(DeviceIdentity::new(
                    v_lo | (v_hi << 8),
                    d_lo | (d_hi << 8),
                    rev,
                ));
            }
            tlv::TAG_CLASS => {
                if len != 4 {
                    return Err(DvCode::EXPORT_MALFORMED);
                }
                let class = match bytes.get(vstart) {
                    Some(&b) => b,
                    None => return Err(DvCode::EXPORT_MALFORMED),
                };
                let subclass = match bytes.get(vstart + 1) {
                    Some(&b) => b,
                    None => return Err(DvCode::EXPORT_MALFORMED),
                };
                let prog_if = match bytes.get(vstart + 2) {
                    Some(&b) => b,
                    None => return Err(DvCode::EXPORT_MALFORMED),
                };
                pend_class = Some(ClassIdentity::new(class, subclass, prog_if));
            }
            tlv::TAG_VENDOR_LABEL => {
                let mut s = String::new();
                let mut i = 0;
                while i < len {
                    if let Some(&b) = bytes.get(vstart + i) {
                        s.push(b as char);
                    }
                    i += 1;
                }
                pend_label = Some(s);
            }
            tlv::TAG_ENTRY_END => {
                if len != 0 {
                    return Err(DvCode::EXPORT_MALFORMED);
                }
                match (pend_slot, pend_identity, pend_class, pend_label.take()) {
                    (Some(slot), Some(id), Some(ci), label) => {
                        out.entries.push((slot, id, ci, label.unwrap_or_else(|| "Unknown".to_string())));
                    }
                    _ => return Err(DvCode::EXPORT_MALFORMED),
                }
                pend_slot = None;
                pend_identity = None;
                pend_class = None;
            }
            tlv::TAG_INVALID => {
                if len != 4 {
                    return Err(DvCode::EXPORT_MALFORMED);
                }
                let bus = match bytes.get(vstart) {
                    Some(&b) => b,
                    None => return Err(DvCode::EXPORT_MALFORMED),
                };
                let dev = match bytes.get(vstart + 1) {
                    Some(&b) => b,
                    None => return Err(DvCode::EXPORT_MALFORMED),
                };
                let func = match bytes.get(vstart + 2) {
                    Some(&b) => b,
                    None => return Err(DvCode::EXPORT_MALFORMED),
                };
                let kind = match bytes.get(vstart + 3) {
                    Some(0) => InvalidKind::NotPresent,
                    Some(1) => InvalidKind::VendorReserved,
                    _ => return Err(DvCode::EXPORT_MALFORMED),
                };
                out.invalid.push((Bdf::new(bus, dev, func), kind));
            }
            tlv::TAG_LEDGER_END => {
                if len != 0 {
                    return Err(DvCode::EXPORT_MALFORMED);
                }
                // 账尾：流可以到此为止（后面若还有内容按预检亦必须仍是
                // 合法 TLV——循环自然继续）。
            }
            _ => return Err(DvCode::EXPORT_MALFORMED),
        }
        pos = vend;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 十一、增量重扫 diff——新身份 / 身份漂移 / 消失 三分账
// ---------------------------------------------------------------------------

/// 重扫三分账报告。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffReport {
    /// 新身份：基线无账本次有账。
    pub added: Vec<(Bdf, DeviceIdentity)>,
    /// 身份漂移：同 bdf 三元组变更（立案 RESCAN_IDENTITY_DRIFT）。
    pub drifted: Vec<(Bdf, DeviceIdentity, DeviceIdentity)>,
    /// 消失：基线有账本次无账（立案 RESCAN_LOST）。
    pub lost: Vec<(Bdf, DeviceIdentity)>,
    /// 联动账漂移：身份同但 class/subclass/prog_if 变（同案入 drift 簿）。
    pub class_drifted: Vec<(Bdf, ClassIdentity, ClassIdentity)>,
}

impl DiffReport {
    /// 三账是否全平。
    pub fn clean(&self) -> bool {
        self.added.is_empty() && self.drifted.is_empty() && self.lost.is_empty()
            && self.class_drifted.is_empty()
    }
}

/// 增量重扫 diff：基线账 vs 本次账，新身份/身份漂移/消失三分账。
/// 漂移与消失同步写入 `base` 账本立案簿（审计留痕）。
pub fn rescan_diff(
    base: &mut DeviceLedger,
    rescan: &DeviceLedger,
    tick: u64,
) -> DiffReport {
    let mut report = DiffReport::default();
    // 一分：新身份 + 身份/联动漂移（以本次账为主视角）。
    for (slot, entry) in rescan.iter_entries() {
        match base.entry(*slot) {
            None => {
                report.added.push((*slot, entry.identity));
            }
            Some(old) => {
                if old.identity != entry.identity {
                    report.drifted.push((*slot, old.identity, entry.identity));
                    base.add_case(*slot, DvCode::RESCAN_IDENTITY_DRIFT, tick);
                } else if old.class != entry.class {
                    report.class_drifted.push((*slot, old.class, entry.class));
                    base.add_case(*slot, DvCode::RESCAN_IDENTITY_DRIFT, tick);
                }
            }
        }
    }
    // 二分：消失（以基线为主视角）。
    let mut lost_slots: Vec<(Bdf, DeviceIdentity)> = Vec::new();
    for (slot, entry) in base.iter_entries() {
        if rescan.entry(*slot).is_none() {
            lost_slots.push((*slot, entry.identity));
        }
    }
    let mut i = 0;
    while i < lost_slots.len() {
        if let Some((slot, id)) = lost_slots.get(i) {
            report.lost.push((*slot, *id));
            base.add_case(*slot, DvCode::RESCAN_LOST, tick);
        }
        i += 1;
    }
    report
}

// ---------------------------------------------------------------------------
// 十二、账务总核——判据 J1 闭环口
// ---------------------------------------------------------------------------

/// 账务总核：判据 J1 闭环——正账齐 + 覆盖 100% + 名录表自检过 + 无未结缺漏。
///
/// 规则：① 名录表自检先行（表坏则账不可信）；② 功能位清单对账
/// （分子 = 已入账 ∩ 清单，分母 = 清单长度）；③ 覆盖 < 100% →
/// Err(COVERAGE_GAP) + 缺漏清单；④ 全过 → Ok(总核快照)。
pub fn audit_identity_ledger(
    ledger: &mut DeviceLedger,
    valid_slots: &[Bdf],
) -> Result<(usize, usize), DvCode> {
    vendor_table_self_check()?;
    let missing = ledger.reconcile(valid_slots)?;
    let denominator = valid_slots.len();
    let numerator = ledger.len();
    let _ = missing; // reconcile 全平时 missing 为空。
    Ok((denominator, numerator))
}

// ---------------------------------------------------------------------------
// 十三、自证样例数据面——确定性银行的标准装机单（供推演复用，非测试）
// ---------------------------------------------------------------------------

/// 标准装机单：往确定性银行登记一组典型真机拓扑（vendor 名录在册者
/// 为主），供推演与采集复用——确定性即"同一装机单永远同一账"。
pub fn install_reference_topology(bank: &mut DeterministicCfgBank) {
    // 00:00.0 Intel 主桥（class 06/00）。
    bank.set_identity(Bdf::new(0, 0x00, 0x0), 0x8086, 0x1237, 0x02, 0x00, 0x00, 0x06);
    // 00:01.0 Intel IDE（class 01/01）。
    bank.set_identity(Bdf::new(0, 0x01, 0x0), 0x8086, 0x7010, 0x00, 0x00, 0x01, 0x01);
    // 00:02.0 Technical Corp VGA（class 03/00）。
    bank.set_identity(Bdf::new(0, 0x02, 0x0), 0x1234, 0x1111, 0x02, 0x00, 0x00, 0x03);
    // 00:03.0 Realtek 以太网（class 02/00）。
    bank.set_identity(Bdf::new(0, 0x03, 0x0), 0x10EC, 0x8139, 0x20, 0x00, 0x00, 0x02);
    // 00:04.0 Red Hat virtio 盘（class 01/00）。
    bank.set_identity(Bdf::new(0, 0x04, 0x0), 0x1AF4, 0x1001, 0x00, 0x00, 0x00, 0x01);
    // 00:05.0 Red Hat QEMU USB（class 0C/03）。
    bank.set_identity(Bdf::new(0, 0x05, 0x0), 0x1B36, 0x000D, 0x01, 0x00, 0x03, 0x0C);
    // 00:1F.3 Broadcom 未知名设备——名录 Unknown 显性。
    bank.set_identity(Bdf::new(0, 0x1F, 0x3), 0x14E4, 0x4331, 0x03, 0x00, 0x80, 0x02);
    // 01:00.0 NVIDIA 3D 显卡（class 03/02）。
    bank.set_identity(Bdf::new(1, 0x00, 0x0), 0x10DE, 0x1B80, 0xA1, 0x00, 0x02, 0x03);
    // 02:00.0 AMD CPU 平台（class 06/00）。
    bank.set_identity(Bdf::new(2, 0x00, 0x0), 0x1022, 0x1450, 0x00, 0x00, 0x00, 0x06);
    // 其余功能位不登记 = 无设备（全 1 回读语义，显性过滤桶）。
}

// ---------------------------------------------------------------------------
// 尾注：判据对照
// ---------------------------------------------------------------------------
// J1 "有效设备身份三元组（vendor/device/revision）账齐（100% 覆盖有效
//     清单）" —— DeviceLedger.reconcile / audit_identity_ledger /
//     SweepReport.complete 三口径对账；覆盖 < 100% 立案 COVERAGE_GAP
//     + 缺漏清单（gaps）。
// J1 "值域核过" —— vendor_domain_classify（0xFFFF 无效 + F0..FF 尾值
//     核）/ device_domain_ok（0x0000 保留）/ revision_in_domain
//     （0x00..0xFF 全域显性）+ class_linkage_ok 联动核。
