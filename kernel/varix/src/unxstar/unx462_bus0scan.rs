//! UNX-F0462 · 总线 0 扫描（32 设备 × 8 功能）（CoRun·Unxreal · UNX-A1-B04 · PCI 深化之一）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0462`
//!
//! **判据原文**：总线 0 全槽位扫描 32×8=256 功能位探齐，vendor 0xFFFF
//! 无效跳过计数显性。
//!
//! ## 职责定位
//!
//! 对 PCI 总线 0 的 32 个设备槽位 × 8 个功能位共 256 个功能槽做全量
//! 配置空间扫描，落账设备清单（vendor/device/class 三元组 + 修订 ID）、
//! 头部类型分派、BAR 摘要、能力指针链；无效功能（vendor 0xFFFF）在
//! 设备级/功能级两级显性计数，不静默跳过。配置空间访问走可注入抽象
//! 读取口（trait 注入），默认实现返回确定性构造数据——本模块零真实
//! 端口 IO、零墙钟（逻辑 tick u64 注入）、全确定性。
//!
//! ## 设计要点
//!
//! 1. **CF8/CFC 两阶段**：地址编解码是纯函数（bit31 使能位 + 总线 16..23
//!    + 设备 11..15 + 功能 8..10 + 寄存器 2..7 位域），编码→锁存（CF8）→
//!    读写（CFC）两阶段显性，不合并成一次魔法调用。
//! 2. **256 槽探齐纪律**：多功能位（功能 0 头类型 bit7）清位只做冲突
//!    检测账，不砍探针——256 个功能位个个探到，探齐数与守恒式对拍。
//! 3. **两级无效计数**：功能级（vendor 0xFFFF 的功能槽数）与设备级
//!    （功能 0 即无效的整槽数）分开记账——前者是拓扑噪声账，后者是
//!    空槽位账，混记即丢信息。
//! 4. **异常账**：配置周期超时（注入口 Err）与非法返回（全零 ghost
//!    设备）分账累积，超时预算熔断把状态机打到 Aborted——异常是账
//!    不是中断。
//! 5. **零 panic 面**：所有输入不可信（注入口可返回任何位型），全部
//!    错误走 Err + 中文诊断；no_std + alloc，绝不 use std。
//! 6. **诊断码独占 0x7Bxx 段**：与 DSDT 册（0x2Exx）等互不重叠。

#![forbid(unsafe_op_in_unsafe_fn)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use alloc::format;

// ---------------------------------------------------------------------------
// 一、诊断码闭集（独占 0x7Bxx 段）
// ---------------------------------------------------------------------------

/// 总线 0 扫描诊断码。独占 `0x7Bxx` 段（同目录已占 0x2Exx/0x2Fxx/
/// 0x94xx/0x95xx 等，本段经 grep 核对无撞）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BxCode(pub u16);

impl BxCode {
    /// 地址位域溢出（总线 >255 / 设备 >31 / 功能 >7）。
    pub const ADDR_FIELD_OVERFLOW: BxCode = BxCode(0x7B01);
    /// 寄存器偏移非 4 字节对齐。
    pub const OFFSET_UNALIGNED: BxCode = BxCode(0x7B02);
    /// 寄存器偏移越界（>0xFC，256 字节配置空间）。
    pub const OFFSET_OUT_OF_RANGE: BxCode = BxCode(0x7B03);
    /// CF8 使能位（bit31）未置——地址非配置周期形态。
    pub const ENABLE_BIT_CLEAR: BxCode = BxCode(0x7B04);
    /// 配置周期超时（注入口显性报错）。
    pub const CYCLE_TIMEOUT: BxCode = BxCode(0x7B05);
    /// 非法返回：全零（vendor 0x0000 ghost 设备）。
    pub const ILLEGAL_RETURN_ALL_ZERO: BxCode = BxCode(0x7B06);
    /// 多功能位冲突：功能 >0 有效但功能 0 头类型 bit7 清位。
    pub const MF_BIT_CONFLICT: BxCode = BxCode(0x7B07);
    /// 未知头部类型（非 0x00/0x01/0x02）。
    pub const UNKNOWN_HEADER_TYPE: BxCode = BxCode(0x7B08);
    /// BAR 位型畸形（IO 位与内存类型位互斥面被破坏）。
    pub const BAR_MALFORMED: BxCode = BxCode(0x7B09);
    /// 能力指针非法（越界 / 非对齐）。
    pub const CAP_PTR_BAD: BxCode = BxCode(0x7B0A);
    /// 能力指针链成环（同偏移二访）。
    pub const CAP_CHAIN_LOOP: BxCode = BxCode(0x7B0B);
    /// 能力链步数超上限（防环计数器打满）。
    pub const CAP_STEP_CAP: BxCode = BxCode(0x7B0C);
    /// 超时预算熔断（扫描中止）。
    pub const TIMEOUT_BUDGET_ABORT: BxCode = BxCode(0x7B0D);
    /// 账本守恒破缺（分项和 ≠ 探针总数 ≠ 256）。
    pub const LEDGER_CONSERVATION_BROKEN: BxCode = BxCode(0x7B0E);
    /// 重扫对账不一致。
    pub const RESCAN_MISMATCH: BxCode = BxCode(0x7B0F);
    /// 状态机非法迁移。
    pub const STATE_INVALID_TRANSITION: BxCode = BxCode(0x7B10);
    /// BAR 尺寸探测失败（写 1s 读回非掩码形态）。
    pub const SIZE_PROBE_FAIL: BxCode = BxCode(0x7B11);
    /// 头寄存器读取失败（功能 0 已判在但头周期超时）。
    pub const HEADER_FETCH_FAIL: BxCode = BxCode(0x7B12);
    /// 越界总线请求（本模块只扫总线 0）。
    pub const BUS0_ONLY: BxCode = BxCode(0x7B13);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            BxCode::ADDR_FIELD_OVERFLOW => {
                "地址位域溢出：总线/设备/功能任一超位宽（总线≤255 设备≤31 功能≤7）".into()
            }
            BxCode::OFFSET_UNALIGNED => "寄存器偏移非 4 字节对齐：配置空间按双字访问".into(),
            BxCode::OFFSET_OUT_OF_RANGE => "寄存器偏移越界：256 字节配置空间上限 0xFC".into(),
            BxCode::ENABLE_BIT_CLEAR => "CF8 使能位（bit31）未置：地址非配置周期形态".into(),
            BxCode::CYCLE_TIMEOUT => "配置周期超时：注入口未在确定性界内返回".into(),
            BxCode::ILLEGAL_RETURN_ALL_ZERO => "非法返回全零：vendor 0x0000 ghost 设备立案".into(),
            BxCode::MF_BIT_CONFLICT => {
                "多功能位冲突：功能 >0 有效但功能 0 头类型 bit7 清位".into()
            }
            BxCode::UNKNOWN_HEADER_TYPE => "未知头部类型：低 7 位非 0x00/0x01/0x02".into(),
            BxCode::BAR_MALFORMED => "BAR 位型畸形：IO 位与内存类型位互斥面被破坏".into(),
            BxCode::CAP_PTR_BAD => "能力指针非法：越界或非 4 字节对齐".into(),
            BxCode::CAP_CHAIN_LOOP => "能力指针链成环：同一偏移二次到达".into(),
            BxCode::CAP_STEP_CAP => "能力链步数超上限：防环计数器打满".into(),
            BxCode::TIMEOUT_BUDGET_ABORT => "超时预算熔断：扫描状态机转入 Aborted".into(),
            BxCode::LEDGER_CONSERVATION_BROKEN => {
                "账本守恒破缺：在位+无效+ghost+超时 ≠ 探针总数 ≠ 256".into()
            }
            BxCode::RESCAN_MISMATCH => "重扫对账不一致：两轮清单分叉".into(),
            BxCode::STATE_INVALID_TRANSITION => "状态机非法迁移：非闭式转移边".into(),
            BxCode::SIZE_PROBE_FAIL => "BAR 尺寸探测失败：写 1s 读回非掩码形态".into(),
            BxCode::HEADER_FETCH_FAIL => "头寄存器读取失败：功能 0 已判在但头周期超时".into(),
            BxCode::BUS0_ONLY => "越界总线请求：本模块只扫总线 0".into(),
            BxCode(_) => "未知 UNX 总线 0 扫描域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、CF8/CFC 地址编解码纯函数（两阶段之一：位域编解码）
// ---------------------------------------------------------------------------

/// 配置周期地址使能位（bit31）。
pub const CFG_ENABLE_BIT: u32 = 0x8000_0000;
/// 配置空间 256 字节总长。
pub const CFG_SPACE_BYTES: usize = 256;
/// 配置空间双字总数（256 / 4）。
pub const CFG_SPACE_WORDS: usize = 64;
/// 单总线设备槽位数。
pub const BUS_DEVICE_SLOTS: usize = 32;
/// 单设备功能位数。
pub const DEV_FUNC_SLOTS: usize = 8;
/// 全总线功能槽位数（判据数：32×8=256）。
pub const TOTAL_FUNC_SLOTS: usize = BUS_DEVICE_SLOTS * DEV_FUNC_SLOTS;

/// BDF（总线/设备/功能）三元组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bdf {
    /// 总线号（本模块恒 0）。
    pub bus: u16,
    /// 设备号 0..31。
    pub dev: u8,
    /// 功能号 0..7。
    pub func: u8,
}

impl Bdf {
    /// 总线 0 上的 BDF 构造。
    pub const fn bus0(dev: u8, func: u8) -> Bdf {
        Bdf { bus: 0, dev, func }
    }

    /// 槽位线性编号（dev*8+func，0..256——账本下标锚）。
    pub const fn slot(self) -> usize {
        (self.dev as usize) * DEV_FUNC_SLOTS + (self.func as usize)
    }

    /// 判 BDF 是否落在总线 0 全槽位界内。
    pub const fn in_bus0_range(self) -> bool {
        self.bus == 0 && self.dev < BUS_DEVICE_SLOTS as u8 && self.func < DEV_FUNC_SLOTS as u8
    }
}

/// CF8 地址编码（纯函数，两阶段之一）。
///
/// 位域：bit31 使能 | bit16..23 总线 | bit11..15 设备 | bit8..10 功能
/// | bit2..7 寄存器（低 2 位恒 0——双字对齐硬件约束）。任一位域超宽
/// 或偏移非对齐/越界→对应诊断码。
pub const fn encode_cf8(bus: u16, dev: u8, func: u8, off: u8) -> Result<u32, BxCode> {
    if bus > 0x00FF {
        return Err(BxCode::ADDR_FIELD_OVERFLOW);
    }
    if dev > 31 {
        return Err(BxCode::ADDR_FIELD_OVERFLOW);
    }
    if func > 7 {
        return Err(BxCode::ADDR_FIELD_OVERFLOW);
    }
    if (off as usize) >= CFG_SPACE_BYTES {
        return Err(BxCode::OFFSET_OUT_OF_RANGE);
    }
    if off & 0x03 != 0 {
        return Err(BxCode::OFFSET_UNALIGNED);
    }
    Ok(CFG_ENABLE_BIT
        | ((bus as u32) & 0xFF) << 16
        | ((dev as u32) & 0x1F) << 11
        | ((func as u32) & 0x07) << 8
        | (off as u32 & 0xFC))
}

/// CF8 地址解码（纯函数，编码恒等逆——对拍面）。
///
/// 使能位未置→ENABLE_BIT_CLEAR；位域照原样拆出（解码侧不再重验
/// 越界——编码侧已闸，解码只认形态）。
pub const fn decode_cf8(addr: u32) -> Result<(u16, u8, u8, u8), BxCode> {
    if addr & CFG_ENABLE_BIT == 0 {
        return Err(BxCode::ENABLE_BIT_CLEAR);
    }
    let bus = ((addr >> 16) & 0xFF) as u16;
    let dev = ((addr >> 11) & 0x1F) as u8;
    let func = ((addr >> 8) & 0x07) as u8;
    let off = (addr & 0xFC) as u8;
    Ok((bus, dev, func, off))
}

/// 编解码恒等对拍（纯函数：encode → decode 须还原输入）。
pub fn codec_roundtrip(bus: u16, dev: u8, func: u8, off: u8) -> Result<bool, BxCode> {
    let addr = encode_cf8(bus, dev, func, off)?;
    let (b2, d2, f2, o2) = decode_cf8(addr)?;
    Ok(b2 == bus && d2 == dev && f2 == func && o2 == (off & 0xFC))
}

// ---------------------------------------------------------------------------
// 三、配置空间访问抽象口（两阶段之二：锁存 + 数据周期）
// ---------------------------------------------------------------------------

/// PCI 配置空间访问口（可注入抽象——不做真实端口 IO）。
///
/// 两阶段纪律显性：先 `addr_latch`（对应 CF8 写），后 `data_read`/
/// `data_write`（对应 CFC 读/写）。实现方自管锁存态，本模块只按序
/// 调用不窥内部。
pub trait ConfigPort {
    /// 阶段一：锁存配置周期地址（CF8 写等价）。
    fn addr_latch(&mut self, addr: u32) -> Result<(), BxCode>;
    /// 阶段二：读数据周期（CFC 读等价，对最近锁存地址）。
    fn data_read(&mut self) -> Result<u32, BxCode>;
    /// 阶段二'：写数据周期（CFC 写等价——BAR 尺寸探测专用）。
    fn data_write(&mut self, val: u32) -> Result<(), BxCode>;
}

/// 两阶段读单步：编码 → 锁存 → 读数据。任何一步 Err 即透传。
pub fn cfg_read_u32(port: &mut dyn ConfigPort, bdf: Bdf, off: u8) -> Result<u32, BxCode> {
    if bdf.bus != 0 {
        return Err(BxCode::BUS0_ONLY);
    }
    let addr = encode_cf8(bdf.bus, bdf.dev, bdf.func, off)?;
    port.addr_latch(addr)?;
    port.data_read()
}

/// 两阶段双字内 16 位半字读（低半/高半按偏移位 1 选择）。
pub fn cfg_read_u16(port: &mut dyn ConfigPort, bdf: Bdf, off: u8) -> Result<u16, BxCode> {
    if off & 0x01 != 0 {
        return Err(BxCode::OFFSET_UNALIGNED);
    }
    let word = cfg_read_u32(port, bdf, off & 0xFC)?;
    if off & 0x02 != 0 {
        Ok((word >> 16) as u16)
    } else {
        Ok(word as u16)
    }
}

/// 两阶段双字内单字节读。
pub fn cfg_read_u8(port: &mut dyn ConfigPort, bdf: Bdf, off: u8) -> Result<u8, BxCode> {
    let word = cfg_read_u32(port, bdf, off & 0xFC)?;
    let shift = ((off & 0x03) as u32) * 8;
    Ok((word >> shift) as u8)
}

// ---------------------------------------------------------------------------
// 四、确定性构造总线拓扑（默认注入口的数据源——全确定性零 IO）
// ---------------------------------------------------------------------------

/// 单条 BAR 的确定性规格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtBar {
    /// IO 空间位（bit1 形态；false 为内存 BAR）。
    pub io: bool,
    /// 基址（IO 取低 16 位形态，内存取 32 位页基址形态）。
    pub base: u32,
    /// 尺寸（字节，2 的幂；探测掩码由它闭式推出）。
    pub size: u32,
    /// 内存 BAR：64 位形态（与下一号 BAR 成对）。
    pub mem64: bool,
    /// 内存 BAR：可预取位（bit3）。
    pub prefetchable: bool,
}

impl VirtBar {
    /// IO BAR 规格速造。
    pub const fn io(base: u32, size: u32) -> VirtBar {
        VirtBar { io: true, base, size, mem64: false, prefetchable: false }
    }

    /// 32 位内存 BAR 规格速造。
    pub const fn mem32(base: u32, size: u32) -> VirtBar {
        VirtBar { io: false, base, size, mem64: false, prefetchable: false }
    }

    /// 64 位内存 BAR 规格速造（低双字形态，高双字恒零）。
    pub const fn mem64(base: u32, size: u32) -> VirtBar {
        VirtBar { io: false, base, size, mem64: true, prefetchable: false }
    }
}

/// 单功能确定性规格。
#[derive(Debug, Clone)]
pub struct VirtFuncSpec {
    /// 功能号 0..7。
    pub func: u8,
    /// vendor ID。
    pub vendor: u16,
    /// device ID。
    pub device: u16,
    /// 修订 ID。
    pub revision: u8,
    /// 类码三元组 [base, sub, prog-if]。
    pub class: [u8; 3],
    /// 头部类型低 7 位（0x00/0x01/0x02）。
    pub header_type: u8,
    /// 多功能位（功能 0 才有语义）。
    pub mf: bool,
    /// BAR 列表（最多 6 条，下标即 BAR 号）。
    pub bars: [Option<VirtBar>; 6],
    /// 能力 ID 列表（确定性落在 0x80 起、每条 4 字节）。
    pub caps: Vec<u8>,
    /// ghost 形态：整个配置空间返回全零（非法返回账演练点）。
    pub ghost_zero: bool,
    /// 桥窗口（头类型 0x01 才有语义）。
    pub bridge: Option<BridgeWindow>,
    /// CardBus 卡基址（头类型 0x02 才有语义）。
    pub cardbus_base: u32,
}

/// 桥窗口规格（头类型 0x01 的二级总线与窗口域）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeWindow {
    /// 一级总线号。
    pub primary: u8,
    /// 二级总线号。
    pub secondary: u8,
    /// 从属总线号。
    pub subordinate: u8,
    /// IO 窗口基址（16 位形态）。
    pub io_base: u16,
    /// IO 窗口限界。
    pub io_limit: u16,
    /// 内存窗口基址（16 位形态，1MB 粒度）。
    pub mem_base: u16,
    /// 内存窗口限界。
    pub mem_limit: u16,
}

impl VirtFuncSpec {
    /// 一般设备规格速造（头类型 0x00）。
    pub fn general(func: u8, vendor: u16, device: u16, revision: u8, class: [u8; 3]) -> VirtFuncSpec {
        VirtFuncSpec {
            func,
            vendor,
            device,
            revision,
            class,
            header_type: 0x00,
            mf: false,
            bars: [None; 6],
            caps: Vec::new(),
            ghost_zero: false,
            bridge: None,
            cardbus_base: 0,
        }
    }

    /// 桥规格速造（头类型 0x01）。
    pub fn bridge_spec(func: u8, vendor: u16, device: u16, revision: u8, class: [u8; 3], w: BridgeWindow) -> VirtFuncSpec {
        VirtFuncSpec {
            func,
            vendor,
            device,
            revision,
            class,
            header_type: 0x01,
            mf: false,
            bars: [None; 6],
            caps: Vec::new(),
            ghost_zero: false,
            bridge: Some(w),
            cardbus_base: 0,
        }
    }

    /// CardBus 规格速造（头类型 0x02）。
    pub fn cardbus_spec(func: u8, vendor: u16, device: u16, revision: u8, class: [u8; 3], cbar: u32) -> VirtFuncSpec {
        VirtFuncSpec {
            func,
            vendor,
            device,
            revision,
            class,
            header_type: 0x02,
            mf: false,
            bars: [None; 6],
            caps: Vec::new(),
            ghost_zero: false,
            bridge: None,
            cardbus_base: cbar,
        }
    }

    /// 设多功能位（链式速造）。
    pub fn with_mf(mut self) -> VirtFuncSpec {
        self.mf = true;
        self
    }

    /// 设 BAR（链式速造，bar_no 0..6 界外即忽略——速造器不 panic）。
    pub fn with_bar(mut self, bar_no: usize, bar: VirtBar) -> VirtFuncSpec {
        if bar_no < 6 {
            self.bars[bar_no] = Some(bar);
        }
        self
    }

    /// 追加能力（链式速造）。
    pub fn with_cap(mut self, cap_id: u8) -> VirtFuncSpec {
        self.caps.push(cap_id);
        self
    }
}

/// 单设备确定性规格（设备号 + 功能集）。
#[derive(Debug, Clone)]
pub struct VirtDevice {
    /// 设备号 0..31。
    pub dev: u8,
    /// 功能规格集。
    pub funcs: Vec<VirtFuncSpec>,
}

/// 确定性总线 0 拓扑（默认注入口数据源——覆盖头类型 0x00/0x01/0x02、
/// 多功能位、ghost 全零、MF 冲突四类账演练点）。
#[derive(Debug, Clone)]
pub struct VirtualBus0 {
    /// 设备规格表（按设备号可查）。
    pub devices: Vec<VirtDevice>,
    /// 周期超时演练位（true 的设备号所有周期报超时）。
    pub timeout_devices: [bool; BUS_DEVICE_SLOTS],
}

impl VirtualBus0 {
    /// 空总线（全 32 槽无设备——全 FFFF 面）。
    pub fn empty() -> VirtualBus0 {
        VirtualBus0 { devices: Vec::new(), timeout_devices: [false; BUS_DEVICE_SLOTS] }
    }

    /// 追加设备规格（设备号界外即忽略——构造器不 panic）。
    pub fn with_device(mut self, d: VirtDevice) -> VirtualBus0 {
        if d.dev < BUS_DEVICE_SLOTS as u8 {
            self.devices.push(d);
        }
        self
    }

    /// 设周期超时演练位（链式）。
    pub fn with_timeout(mut self, dev: u8) -> VirtualBus0 {
        if (dev as usize) < BUS_DEVICE_SLOTS {
            self.timeout_devices[dev as usize] = true;
        }
        self
    }

    /// 查设备规格。
    pub fn find_dev(&self, dev: u8) -> Option<&VirtDevice> {
        if dev >= BUS_DEVICE_SLOTS as u8 {
            return None;
        }
        let mut i = 0usize;
        while i < self.devices.len() {
            if self.devices[i].dev == dev {
                return Some(&self.devices[i]);
            }
            i += 1;
        }
        None
    }

    /// 查功能规格。
    pub fn find_func(&self, dev: u8, func: u8) -> Option<&VirtFuncSpec> {
        let d = self.find_dev(dev)?;
        let mut i = 0usize;
        while i < d.funcs.len() {
            if d.funcs[i].func == func {
                return Some(&d.funcs[i]);
            }
            i += 1;
        }
        None
    }

    /// 标准演示拓扑（确定性——判据演练面全点位覆盖）：
    /// - dev 0 func 0：host bridge（类 0x060000，无 BAR）
    /// - dev 2 func 0：VGA（MF=1，带能力链 0x10→0x05，MEM BAR ×2）
    /// - dev 3 func 0：NIC（MEM32 + IO BAR 各一，MSI 能力）
    /// - dev 8 func 0：PCI-PCI 桥（头类型 0x01）
    /// - dev 9 func 0：CardBus 桥（头类型 0x02）
    /// - dev 12 func 0：ghost 全零设备（非法返回账）
    /// - dev 13：func 0 有效但 MF 清位、func 1 也有效（MF 冲突账）
    /// - dev 20：全部周期超时（异常账熔断演练点）
    /// - dev 31：PIIX4 形多功能四功能（0/2/3/7 在位）
    pub fn standard() -> VirtualBus0 {
        let host = VirtFuncSpec::general(0, 0x8086, 0x1237, 0x02, [0x06, 0x00, 0x00]);
        let vga = VirtFuncSpec::general(0, 0x1234, 0x1111, 0x11, [0x03, 0x00, 0x00])
            .with_mf()
            .with_bar(0, VirtBar::mem32(0xFD00_0000, 0x0100_0000))
            .with_bar(1, VirtBar::mem32(0xFEE0_0000, 0x0000_1000))
            .with_cap(0x10)
            .with_cap(0x05);
        let nic = VirtFuncSpec::general(0, 0x8086, 0x100E, 0x03, [0x02, 0x00, 0x00])
            .with_bar(0, VirtBar::mem32(0xFEB0_0000, 0x0002_0000))
            .with_bar(1, VirtBar::io(0xC000, 0x40))
            .with_cap(0x01);
        let bridge = VirtFuncSpec::bridge_spec(
            0,
            0x8086,
            0x2418,
            0x01,
            [0x06, 0x04, 0x00],
            BridgeWindow {
                primary: 0,
                secondary: 1,
                subordinate: 1,
                io_base: 0xD000,
                io_limit: 0xDFFF,
                mem_base: 0xF000,
                mem_limit: 0xFBFF,
            },
        );
        let cardbus = VirtFuncSpec::cardbus_spec(0, 0x104C, 0xAC50, 0x01, [0x06, 0x07, 0x00], 0x8800_0000);
        let ghost = VirtFuncSpec::general(0, 0x0000, 0x0000, 0x00, [0x00, 0x00, 0x00]);
        let ghost = VirtFuncSpec { ghost_zero: true, ..ghost };
        let mf_conflict0 = VirtFuncSpec::general(0, 0x10EC, 0x8168, 0x0C, [0x02, 0x00, 0x02]);
        let mf_conflict1 = VirtFuncSpec::general(1, 0x10EC, 0x8169, 0x0C, [0x02, 0x00, 0x02]);
        let piix0 = VirtFuncSpec::general(0, 0x8086, 0x7000, 0x00, [0x06, 0x01, 0x00]).with_mf();
        let piix2 = VirtFuncSpec::general(2, 0x8086, 0x7010, 0x00, [0x01, 0x01, 0x80])
            .with_bar(4, VirtBar::io(0xFFA0, 0x10));
        let piix3 = VirtFuncSpec::general(3, 0x8086, 0x7112, 0x00, [0x0C, 0x03, 0x00]);
        let piix7 = VirtFuncSpec::general(7, 0x8086, 0x7113, 0x02, [0x06, 0x80, 0x00]);
        VirtualBus0::empty()
            .with_device(VirtDevice { dev: 0, funcs: vec![host] })
            .with_device(VirtDevice { dev: 2, funcs: vec![vga] })
            .with_device(VirtDevice { dev: 3, funcs: vec![nic] })
            .with_device(VirtDevice { dev: 8, funcs: vec![bridge] })
            .with_device(VirtDevice { dev: 9, funcs: vec![cardbus] })
            .with_device(VirtDevice { dev: 12, funcs: vec![ghost] })
            .with_device(VirtDevice { dev: 13, funcs: vec![mf_conflict0, mf_conflict1] })
            .with_device(VirtDevice {
                dev: 31,
                funcs: vec![piix0, piix2, piix3, piix7],
            })
            .with_timeout(20)
    }
}

/// 能力区确定性偏移闭式：第 i 条能力落 `0x80 + i*4`。
pub const fn cap_area_base() -> u8 {
    0x80
}

/// 求功能规格中第 idx 条能力的配置偏移（链式闭式）。
pub const fn cap_offset(idx: usize) -> Option<u8> {
    let off = cap_area_base() as usize + idx * 4;
    if off + 4 <= CFG_SPACE_BYTES {
        Some(off as u8)
    } else {
        None
    }
}

/// 单功能配置空间单双字确定性构造（纯函数——默认注入口数据源）。
pub fn virt_word(spec: &VirtFuncSpec, word: usize) -> u32 {
    if word >= CFG_SPACE_WORDS {
        return 0xFFFF_FFFF;
    }
    if spec.ghost_zero {
        return 0x0000_0000;
    }
    let off = word * 4;
    match word {
        0 => (spec.vendor as u32) | ((spec.device as u32) << 16),
        1 => {
            // 状态/命令：状态低位半字——bit4 能力列表在位。
            let status: u16 = if spec.caps.is_empty() { 0x0000 } else { 0x0010 };
            (status as u32) | 0x0007_0000 // 命令字：IO+MEM+BUS MASTER 形态
        }
        2 => {
            // 修订/ProgIF/子类/基类。
            (spec.revision as u32)
                | ((spec.class[2] as u32) << 8)
                | ((spec.class[1] as u32) << 16)
                | ((spec.class[0] as u32) << 24)
        }
        3 => {
            // BIST/头类型/延迟/缓存行。
            let hdr = if spec.mf { spec.header_type | 0x80 } else { spec.header_type & 0x7F };
            0x0000_0000 | ((hdr as u32) << 16)
        }
        _ => {
            if let Some(v) = virt_ext_word(spec, off) {
                v
            } else {
                0x0000_0000
            }
        }
    }
}

/// 扩展区（BAR/桥/CardBus/能力）双字构造（0x10 起——word 0..3 之外面）。
fn virt_ext_word(spec: &VirtFuncSpec, off: usize) -> Option<u32> {
    // 能力区。
    if off >= cap_area_base() as usize {
        let idx = (off - cap_area_base() as usize) / 4;
        let next_idx = idx + 1;
        if (idx as usize) < spec.caps.len() {
            let next: u8 = if next_idx < spec.caps.len() {
                match cap_offset(next_idx) {
                    Some(o) => o,
                    None => 0,
                }
            } else {
                0
            };
            return Some((spec.caps[idx] as u32) | ((next as u32) << 8));
        }
        return Some(0x0000_0000);
    }
    match spec.header_type & 0x7F {
        0x00 => virt_ext_type0(spec, off),
        0x01 => virt_ext_type1(spec, off),
        0x02 => virt_ext_type2(spec, off),
        _ => Some(0x0000_0000),
    }
}

/// 一般设备扩展双字（BAR 0x10..0x24 / 子系统 0x2C / CAP 指针 0x34 / 中断 0x3C）。
fn virt_ext_type0(spec: &VirtFuncSpec, off: usize) -> Option<u32> {
    if (0x10..0x28).contains(&off) && off % 4 == 0 {
        let bar_no = (off - 0x10) / 4;
        if bar_no < 6 {
            if let Some(b) = spec.bars[bar_no] {
                return Some(bar_raw_value(b));
            }
        }
        return Some(0x0000_0000);
    }
    match off {
        0x2C => Some((0x1AF4u32) | ((spec.device as u32) << 16)), // 子系统 vendor/device（QEMU 形）
        0x34 => {
            // CAP 指针。
            match spec.caps.first() {
                Some(_) => Some(cap_area_base() as u32),
                None => Some(0x0000_0000),
            }
        }
        0x3C => Some(0x0000_010A), // 中断引脚 A / 中断线 10。
        _ => Some(0x0000_0000),
    }
}

/// 桥扩展双字（BAR 0x10..0x18 / 总线 0x18 / IO 0x1C / 内存 0x20 /
/// 预取 0x24 / CAP 指针 0x34 / 中断 0x3C）。
fn virt_ext_type1(spec: &VirtFuncSpec, off: usize) -> Option<u32> {
    let w = match spec.bridge {
        Some(w) => w,
        None => {
            return Some(0x0000_0000);
        }
    };
    if off == 0x10 || off == 0x14 {
        return Some(0x0000_0000); // 桥 BAR 留空。
    }
    match off {
        0x18 => {
            Some((w.primary as u32) | ((w.secondary as u32) << 8) | ((w.subordinate as u32) << 16))
        }
        0x1C => {
            // IO 窗口：低字节基/限 + 高 16 位上基/上限（32 位窗口形态）。
            Some(
                ((w.io_base & 0xF0) as u32)
                    | (((w.io_limit & 0xF0) as u32) << 8)
                    | (0x0001u32 << 16)
                    | (0x0001u32 << 24),
            )
        }
        0x20 => {
            Some(((w.mem_base & 0xFFF0) as u32) | (((w.mem_limit & 0xFFF0) as u32) << 16))
        }
        0x24 => Some(0x0000_0000), // 预取窗留空。
        0x34 => {
            match spec.caps.first() {
                Some(_) => Some(cap_area_base() as u32),
                None => Some(0x0000_0000),
            }
        }
        0x3C => Some(0x0000_0100),
        _ => Some(0x0000_0000),
    }
}

/// CardBus 扩展双字（CBAR 0x10 / CAP 指针 0x14 / 桥控 0x3C）。
fn virt_ext_type2(spec: &VirtFuncSpec, off: usize) -> Option<u32> {
    match off {
        0x10 => Some(spec.cardbus_base),
        0x14 => {
            match spec.caps.first() {
                Some(_) => Some(cap_area_base() as u32),
                None => Some(0x0000_0000),
            }
        }
        0x3C => Some(0x0000_0100),
        _ => Some(0x0000_0000),
    }
}

/// BAR 寄存器原始值位型（内存：bit0=0、bit2..3 类型、bit3 预取；
/// IO：bit0=1、基址低 16 位形态）。
pub fn bar_raw_value(b: VirtBar) -> u32 {
    if b.io {
        (b.base & 0xFFFF) | 0x0000_0001
    } else {
        let type_bits: u32 = if b.mem64 { 0b10 } else { 0b00 };
        (b.base & 0xFFFF_FFF0) | (b.prefetchable as u32) << 3 | (type_bits << 1)
    }
}

/// BAR 尺寸掩码闭式（探测期写全 1 后读回的确定性期望值）：
/// 尺寸 2^n → 掩码 = !(size-1) | 保留位型位。
pub fn bar_size_mask(b: VirtBar) -> u32 {
    let size_mask = b.size.wrapping_sub(1);
    if b.io {
        // IO：低 16 位掩码 + 高 16 位全 1（规范读回形态）。
        (!size_mask & 0x0000_FFF0) | 0xFFFF_0000 | 0x0000_0001
    } else {
        let type_bits: u32 = if b.mem64 { 0b10 } else { 0b00 };
        (!size_mask & 0xFFFF_FFF0) | (b.prefetchable as u32) << 3 | (type_bits << 1)
    }
}

// ---------------------------------------------------------------------------
// 五、默认注入口：确定性配置口（CF8/CFC 两阶段行为模型）
// ---------------------------------------------------------------------------

/// 默认配置口（确定性构造数据——判据全链可离线演练）。
///
/// 行为模型：① 锁存态自管（addr_latch 存最近地址）；② 读按拓扑构造
/// 双字（不在拓扑的功能槽恒 0xFFFF_FFFF——vendor 0xFFFF 面）；③ 写
/// 全 1 进入探测臂，同地址随后读回尺寸掩码（BAR 尺寸探测模型）；
/// ④ 超时演练位按设备号报 CYCLE_TIMEOUT。
pub struct DeterministicPort {
    /// 拓扑源。
    bus: VirtualBus0,
    /// 最近锁存地址。
    latched: Option<u32>,
    /// BAR 尺寸探测臂（锁存了写全 1 的地址）。
    probe_arm: Option<u32>,
    /// 累计周期数（确定性审计面）。
    cycle_count: u64,
}

impl DeterministicPort {
    /// 以给定拓扑造口。
    pub fn new(bus: VirtualBus0) -> DeterministicPort {
        DeterministicPort { bus, latched: None, probe_arm: None, cycle_count: 0 }
    }

    /// 标准演示拓扑造口。
    pub fn standard() -> DeterministicPort {
        DeterministicPort::new(VirtualBus0::standard())
    }

    /// 累计周期数（审计面只读）。
    pub fn cycles(&self) -> u64 {
        self.cycle_count
    }
}

impl ConfigPort for DeterministicPort {
    fn addr_latch(&mut self, addr: u32) -> Result<(), BxCode> {
        // 编码形态闸：使能位必须在（锁存非配置形态地址即拒）。
        let _ = decode_cf8(addr)?;
        self.latched = Some(addr);
        self.probe_arm = None;
        self.cycle_count += 1;
        Ok(())
    }

    fn data_read(&mut self) -> Result<u32, BxCode> {
        let addr = match self.latched {
            Some(a) => a,
            None => return Err(BxCode::ENABLE_BIT_CLEAR),
        };
        self.cycle_count += 1;
        let (bus, dev, _func, off) = match decode_cf8(addr) {
            Ok(t) => t,
            Err(e) => return Err(e),
        };
        if bus != 0 {
            // 非总线 0：确定性模型只有总线 0——恒 FFFF（规范无设备形态）。
            return Ok(0xFFFF_FFFF);
        }
        if (dev as usize) < BUS_DEVICE_SLOTS && self.bus.timeout_devices[dev as usize] {
            return Err(BxCode::CYCLE_TIMEOUT);
        }
        if self.probe_arm == Some(addr) {
            // 尺寸探测读回：写全 1 后同地址读 → 掩码。
            if let Some(spec) = self.bus.find_func(dev, _func) {
                if !spec.ghost_zero {
                    if off >= 0x10 && off < 0x28 && (off - 0x10) % 4 == 0 {
                        let bar_no = (off - 0x10) / 4;
                        if bar_no < 6 {
                            if let Some(b) = spec.bars[bar_no as usize] {
                                return Ok(bar_size_mask(b));
                            }
                        }
                    }
                }
            }
            return Ok(0x0000_0000);
        }
        match self.bus.find_func(dev, _func) {
            Some(spec) => Ok(virt_word(spec, (off / 4) as usize)),
            None => Ok(0xFFFF_FFFF),
        }
    }

    fn data_write(&mut self, val: u32) -> Result<(), BxCode> {
        let addr = match self.latched {
            Some(a) => a,
            None => return Err(BxCode::ENABLE_BIT_CLEAR),
        };
        self.cycle_count += 1;
        if val == 0xFFFF_FFFF {
            // 探测臂 armed（BAR 尺寸探测协议第一步）。
            self.probe_arm = Some(addr);
        } else {
            self.probe_arm = None;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 六、设备清单账（vendor/device/class 三元组 + 修订 ID）
// ---------------------------------------------------------------------------

/// 设备清单账条目（判据「设备清单账」锚）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevIdentity {
    /// vendor ID（16 位）。
    pub vendor: u16,
    /// device ID（16 位）。
    pub device: u16,
    /// 修订 ID（8 位）。
    pub revision: u8,
    /// 基类。
    pub base_class: u8,
    /// 子类。
    pub sub_class: u8,
    /// 编程接口。
    pub prog_if: u8,
    /// 头部类型低 7 位。
    pub header_type: u8,
    /// 多功能位（功能 0 头类型 bit7）。
    pub mf_bit: bool,
}

/// 功能 0 头类型双字的多功能位提取（bit7 of byte @0x0E）。
pub const fn mf_bit_of_header_word(word3: u32) -> bool {
    (word3 >> 23) & 0x01 != 0
}

/// 状态/命令双字 → 能力列表在位（状态 bit4）。
pub const fn cap_list_present(status_cmd: u32) -> bool {
    status_cmd & 0x0010 != 0
}

// ---------------------------------------------------------------------------
// 七、BAR 摘要解析（MEM/IO 类型位）
// ---------------------------------------------------------------------------

/// BAR 类型摘要。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
    /// IO 空间（bit0=1）。
    Io,
    /// 32 位内存（类型位 00）。
    Mem32,
    /// 64 位内存低双字（类型位 10；高双字由下一号 BAR 承载）。
    Mem64Low,
    /// 64 位内存高双字（扫描账内不独立立案——并入低双字）。
    Mem64High,
    /// 保留类型位（01/11——畸形立案）。
    Reserved,
}

/// 单 BAR 摘要账。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarSummary {
    /// BAR 号（0..6）。
    pub bar_no: u8,
    /// 配置偏移（0x10 + 4*bar_no）。
    pub offset: u8,
    /// 原始寄存器值。
    pub raw: u32,
    /// 类型摘要。
    pub kind: BarKind,
    /// 可预取（仅内存 BAR 有语义）。
    pub prefetchable: bool,
    /// 去位型后的基址（IO 已剥低 2 位；MEM 已剥低 4 位与类型位）。
    pub base: u32,
    /// 64 位对的下一号 BAR 号（非 64 位为 None）。
    pub next_pair: Option<u8>,
}

/// 原始 BAR 值 → 摘要（纯函数；畸形类型位立案在 kind=Reserved，
/// 调用侧据此计 BAR_MALFORMED）。
pub const fn parse_bar_raw(bar_no: u8, raw: u32) -> BarSummary {
    let offset = 0x10u8 + bar_no * 4;
    let io = raw & 0x01 != 0;
    let prefetchable = !io && (raw & 0x08 != 0);
    let kind = if io {
        BarKind::Io
    } else {
        match (raw >> 1) & 0x03 {
            0b00 => BarKind::Mem32,
            0b10 => BarKind::Mem64Low,
            _ => BarKind::Reserved,
        }
    };
    let base = if io { raw & 0xFFFF_FFFC } else { raw & 0xFFFF_FFF0 };
    let next_pair = if matches!(kind, BarKind::Mem64Low) {
        if bar_no < 5 {
            Some(bar_no + 1)
        } else {
            None
        }
    } else {
        None
    };
    BarSummary { bar_no, offset, raw, kind, prefetchable, base, next_pair }
}

/// BAR 探测账（含尺寸闭式——写全 1 读回掩码 → size = !mask+1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarLedger {
    /// 摘要列表。
    pub bars: Vec<BarSummary>,
    /// 尺寸列表（与 bars 一一对应；探测失败为 None）。
    pub sizes: Vec<Option<u32>>,
    /// 畸形计数（Reserved 类型 / 64 位对越界）。
    pub malformed: u32,
}

impl BarLedger {
    /// 空账。
    pub fn new() -> BarLedger {
        BarLedger { bars: Vec::new(), sizes: Vec::new(), malformed: 0 }
    }
}

/// BAR 尺寸探测两步协议（纯注入口协议，确定性）：
/// ① 读原值；② 写全 1；③ 读回掩码；④ 复写原值；⑤ size = !mask+1。
pub fn probe_bar_size(
    port: &mut dyn ConfigPort,
    bdf: Bdf,
    off: u8,
) -> Result<u32, BxCode> {
    let orig = cfg_read_u32(port, bdf, off)?;
    if orig == 0x0000_0000 || orig == 0xFFFF_FFFF {
        // 未实现 BAR：尺寸恒零（不算失败——实现了才有尺寸）。
        return Ok(0);
    }
    port.addr_latch(encode_cf8(bdf.bus, bdf.dev, bdf.func, off)?)?;
    port.data_write(0xFFFF_FFFF)?;
    let mask = port.data_read()?;
    // 复写原值（探测协议收尾——不留探测臂）。
    port.addr_latch(encode_cf8(bdf.bus, bdf.dev, bdf.func, off)?)?;
    port.data_write(orig)?;
    let size = (!mask).wrapping_add(1);
    if size <= 1 {
        return Err(BxCode::SIZE_PROBE_FAIL);
    }
    Ok(size)
}

/// 功能 BAR 全量解析（按头类型定 BAR 数：0x00→6、0x01→2、0x02→1）。
pub fn collect_bars(
    port: &mut dyn ConfigPort,
    bdf: Bdf,
    header_type: u8,
) -> Result<BarLedger, BxCode> {
    let bar_count: usize = match header_type & 0x7F {
        0x00 => 6,
        0x01 => 2,
        0x02 => 1,
        _ => return Err(BxCode::UNKNOWN_HEADER_TYPE),
    };
    let mut ledger = BarLedger::new();
    let mut bar_no: usize = 0;
    while bar_no < bar_count {
        let off = 0x10u8 + (bar_no as u8) * 4;
        let raw = cfg_read_u32(port, bdf, off)?;
        if raw == 0x0000_0000 {
            // 未实现 BAR：不留账（规范形态）。
            bar_no += 1;
            continue;
        }
        if raw == 0xFFFF_FFFF {
            // 读周期打回 FFFF：按未实现处理且不立案（在位功能的上
            // 半 BAR 缺席合法——如 64 位对高双字）。
            bar_no += 1;
            continue;
        }
        let summary = parse_bar_raw(bar_no as u8, raw);
        if matches!(summary.kind, BarKind::Reserved) {
            ledger.malformed += 1;
            bar_no += 1;
            continue;
        }
        // 64 位对：高双字并入（低双字形态成立才读高双字）。
        if let Some(next_no) = summary.next_pair {
            if (next_no as usize) >= bar_count {
                ledger.malformed += 1;
                bar_no += 1;
                continue;
            }
            let hi_off = 0x10u8 + (next_no as u8) * 4;
            let hi = cfg_read_u32(port, bdf, hi_off)?;
            if hi != 0xFFFF_FFFF && hi != 0x0000_0000 {
                // 高双字非零：确定性模型内不立 64 位全值账，只记对位
                // 存在（确定性数据源高双字恒零——非零面留给注入）。
            }
            bar_no = next_no as usize + 1;
        } else {
            bar_no += 1;
        }
        let size = match probe_bar_size(port, bdf, off) {
            Ok(s) => Some(s),
            Err(BxCode::SIZE_PROBE_FAIL) => None,
            Err(e) => return Err(e),
        };
        ledger.bars.push(summary);
        ledger.sizes.push(size);
    }
    Ok(ledger)
}

// ---------------------------------------------------------------------------
// 八、能力指针链走查（防环计数上限）
// ---------------------------------------------------------------------------

/// 能力链步数上限（防环计数器——同链步数超限即 CAP_STEP_CAP）。
pub const CAP_STEP_LIMIT: usize = 48;

/// 单能力条目账。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapEntry {
    /// 能力 ID。
    pub cap_id: u8,
    /// 配置偏移。
    pub offset: u8,
    /// 链内下一偏移（0x00 = 链尾）。
    pub next: u8,
}

/// 能力链走查账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapWalk {
    /// 逐条能力账。
    pub caps: Vec<CapEntry>,
    /// 实际步数。
    pub steps: usize,
    /// 走查是否完整到链尾（false = 环/步数帽/坏指针提前终止）。
    pub complete: bool,
    /// 终止原因（完整为 None）。
    pub aborted_by: Option<BxCode>,
}

/// 能力链走查单点（防环三闸：指针界闸 + 同偏移重访闸 + 步数帽闸）。
pub fn walk_caps(
    port: &mut dyn ConfigPort,
    bdf: Bdf,
    cap_ptr_off: u8,
) -> Result<CapWalk, BxCode> {
    let mut visited = [false; CFG_SPACE_WORDS];
    let mut caps: Vec<CapEntry> = Vec::new();
    let mut steps: usize = 0;
    let mut ptr = cap_ptr_off;
    loop {
        if steps >= CAP_STEP_LIMIT {
            return Ok(CapWalk {
                caps,
                steps,
                complete: false,
                aborted_by: Some(BxCode::CAP_STEP_CAP),
            });
        }
        if (ptr as usize) < 0x40
            || (ptr as usize) >= CFG_SPACE_BYTES
            || ptr & 0x03 != 0
        {
            return Ok(CapWalk {
                caps,
                steps,
                complete: false,
                aborted_by: Some(BxCode::CAP_PTR_BAD),
            });
        }
        let slot = (ptr / 4) as usize;
        if slot < CFG_SPACE_WORDS && visited[slot] {
            return Ok(CapWalk {
                caps,
                steps,
                complete: false,
                aborted_by: Some(BxCode::CAP_CHAIN_LOOP),
            });
        }
        if slot < CFG_SPACE_WORDS {
            visited[slot] = true;
        }
        let raw = cfg_read_u32(port, bdf, ptr)?;
        let cap_id = (raw & 0xFF) as u8;
        let next = ((raw >> 8) & 0xFF) as u8;
        caps.push(CapEntry { cap_id, offset: ptr, next });
        steps += 1;
        if next == 0 {
            return Ok(CapWalk { caps, steps, complete: true, aborted_by: None });
        }
        ptr = next;
    }
}

/// 按头类型取能力指针配置偏移（0x00/0x01→0x34、0x02→0x14）。
pub const fn cap_ptr_offset(header_type: u8) -> Option<u8> {
    match header_type & 0x7F {
        0x00 | 0x01 => Some(0x34),
        0x02 => Some(0x14),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 九、扫描进度状态机（Idle/Scanning/Complete/Aborted）
// ---------------------------------------------------------------------------

/// 扫描状态机状态闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanState {
    /// 空闲（起步形态）。
    Idle,
    /// 扫描中（256 槽逐个探）。
    Scanning,
    /// 完成（256 槽探齐且未熔断）。
    Complete,
    /// 中止（超时预算熔断）。
    Aborted,
}

/// 扫描进度状态机（逻辑 tick u64 注入——零墙钟）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanProgress {
    /// 当前状态。
    pub state: ScanState,
    /// 已探设备数（0..32）。
    pub devices_done: u32,
    /// 已探功能数（0..256）。
    pub funcs_done: u32,
    /// 起步逻辑 tick。
    pub start_tick: u64,
    /// 最近推进逻辑 tick。
    pub last_tick: u64,
}

impl ScanProgress {
    /// 空闲起步形态。
    pub fn new(start_tick: u64) -> ScanProgress {
        ScanProgress {
            state: ScanState::Idle,
            devices_done: 0,
            funcs_done: 0,
            start_tick,
            last_tick: start_tick,
        }
    }

    /// 闭式迁移表驱动的状态转移（非法边 → STATE_INVALID_TRANSITION）。
    ///
    /// 合法边：Idle→Scanning、Scanning→Complete、Scanning→Aborted、
    /// Complete→Scanning（重扫入口）、Aborted→Scanning（预算重置后
    /// 重扫入口）。其余全拒。
    pub fn transition(&mut self, to: ScanState, tick: u64) -> Result<(), BxCode> {
        let legal = matches!(
            (self.state, to),
            (ScanState::Idle, ScanState::Scanning)
                | (ScanState::Scanning, ScanState::Complete)
                | (ScanState::Scanning, ScanState::Aborted)
                | (ScanState::Complete, ScanState::Scanning)
                | (ScanState::Aborted, ScanState::Scanning)
        );
        if !legal {
            return Err(BxCode::STATE_INVALID_TRANSITION);
        }
        self.state = to;
        self.last_tick = tick;
        Ok(())
    }

    /// 推进一格（一功能探毕——tick 自增注入）。
    pub fn advance_one(&mut self, tick: u64) {
        self.funcs_done = (self.funcs_done + 1).min(TOTAL_FUNC_SLOTS as u32);
        if self.funcs_done % DEV_FUNC_SLOTS as u32 == 0 {
            self.devices_done = (self.devices_done + 1).min(BUS_DEVICE_SLOTS as u32);
        }
        self.last_tick = tick;
    }

    /// 探齐判定（funcs_done == 256 且设备 32）。
    pub const fn probed_full(&self) -> bool {
        self.funcs_done == TOTAL_FUNC_SLOTS as u32 && self.devices_done == BUS_DEVICE_SLOTS as u32
    }
}

// ---------------------------------------------------------------------------
// 十、功能槽账与扫描账本（256 功能位账 + 两级无效计数）
// ---------------------------------------------------------------------------

/// 功能槽判定形态闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuncState {
    /// 未探（中止残留面）。
    NotProbed,
    /// 在位有效（vendor ≠ 0xFFFF 且非法面未命中）。
    Present,
    /// 无效（vendor 0xFFFF——功能级无效账）。
    Invalid,
    /// 非法返回（全零 ghost）。
    Ghost,
    /// 周期超时。
    Timeout,
}

/// 单功能槽账。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FuncSlot {
    /// 判定形态。
    pub state: FuncState,
    /// 设备清单账（仅 Present 有值）。
    pub identity: Option<DevIdentity>,
    /// BAR 数（仅 Present 且头类型分派成功有语义）。
    pub bar_count: u8,
    /// 能力数。
    pub cap_count: u8,
    /// 探测时逻辑 tick。
    pub probe_tick: u64,
}

impl FuncSlot {
    /// 空槽（未探）。
    pub const fn blank() -> FuncSlot {
        FuncSlot { state: FuncState::NotProbed, identity: None, bar_count: 0, cap_count: 0, probe_tick: 0 }
    }
}

/// 扫描账本（判据核心账：256 功能位 + 两级无效计数显性）。
#[derive(Debug, Clone)]
pub struct ScanLedger {
    /// 256 功能槽账（下标 = Bdf::slot()）。
    pub slots: [FuncSlot; TOTAL_FUNC_SLOTS],
    /// 功能级无效计数（vendor 0xFFFF 功能槽数——含设备级空槽衍生）。
    pub func_invalid_count: u32,
    /// 设备级无效计数（功能 0 即 0xFFFF 的整槽数——空设备槽）。
    pub dev_absent_count: u32,
    /// 非法返回（ghost 全零）计数。
    pub ghost_count: u32,
    /// 周期超时计数。
    pub timeout_count: u32,
    /// 在位有效功能数。
    pub present_func_count: u32,
    /// 探针总数（完成态恒 256——判据锚数）。
    pub probed: u32,
    /// MF 冲突计数（功能 >0 有效但功能 0 MF 清位）。
    pub mf_conflict_count: u32,
    /// 各设备功能 0 的多功能位快照（None = 功能 0 不在位）。
    pub dev_mf: [Option<bool>; BUS_DEVICE_SLOTS],
}

impl ScanLedger {
    /// 空账本（起步形态——256 槽全 NotProbed）。
    pub fn new() -> ScanLedger {
        ScanLedger {
            slots: core::array::from_fn(|_| FuncSlot::blank()),
            func_invalid_count: 0,
            dev_absent_count: 0,
            ghost_count: 0,
            timeout_count: 0,
            present_func_count: 0,
            probed: 0,
            mf_conflict_count: 0,
            dev_mf: [None; BUS_DEVICE_SLOTS],
        }
    }

    /// 判据锚数：探齐数恒等 256（守恒式第一项）。
    pub const fn probed_expected() -> u32 {
        TOTAL_FUNC_SLOTS as u32
    }

    /// 守恒式对拍：探针总数 == 256 且 在位+功能级无效+ghost+超时 ==
    /// 探针总数（分项和闭合——任何缺口即破缺）。
    pub const fn conserved(&self) -> bool {
        let probed = self.probed;
        let parts = self.present_func_count
            + self.func_invalid_count
            + self.ghost_count
            + self.timeout_count;
        probed == ScanLedger::probed_expected() && parts == probed
    }

    /// 守恒对拍显性化（破缺 → LEDGER_CONSERVATION_BROKEN）。
    pub fn conservation_check(&self) -> Result<(), BxCode> {
        if self.conserved() {
            Ok(())
        } else {
            Err(BxCode::LEDGER_CONSERVATION_BROKEN)
        }
    }

    /// 设备级在位数（功能 0 在位的设备槽数）。
    pub fn dev_present_count(&self) -> u32 {
        let mut n = 0u32;
        let mut d = 0usize;
        while d < BUS_DEVICE_SLOTS {
            match self.dev_mf[d] {
                Some(_) => n += 1,
                None => {}
            }
            d += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 十一、异常账（配置周期超时 / 非法返回——分账累积，容量封顶不静默）
// ---------------------------------------------------------------------------

/// 异常账条目容量上限（超出截断并显性标注）。
pub const MAX_ANOMALY_ENTRIES: usize = 64;

/// 异常账条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnomalyEntry {
    /// 异常诊断码（超时 / 非法返回闭集内）。
    pub code: BxCode,
    /// BDF 槽位。
    pub bdf: Bdf,
    /// 发生时逻辑 tick。
    pub tick: u64,
}

/// 异常账（判据「异常账（配置周期超时/非法返回）」锚）。
#[derive(Debug, Clone)]
pub struct AnomalyLedger {
    /// 条目列表（容量封顶 MAX_ANOMALY_ENTRIES）。
    pub entries: Vec<AnomalyEntry>,
    /// 实数（含溢出未入账者——不静默）。
    pub count: u64,
    /// 是否截断。
    pub truncated: bool,
}

impl AnomalyLedger {
    /// 空账。
    pub fn new() -> AnomalyLedger {
        AnomalyLedger { entries: Vec::new(), count: 0, truncated: false }
    }

    /// 记一条（封顶后只计数不丢账实数——截断显性）。
    pub fn record(&mut self, code: BxCode, bdf: Bdf, tick: u64) {
        self.count += 1;
        if self.entries.len() < MAX_ANOMALY_ENTRIES {
            self.entries.push(AnomalyEntry { code, bdf, tick });
        } else {
            self.truncated = true;
        }
    }

    /// 超时异常子账实数。
    pub fn timeout_total(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].code == BxCode::CYCLE_TIMEOUT {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 非法返回子账实数。
    pub fn illegal_total(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].code == BxCode::ILLEGAL_RETURN_ALL_ZERO {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 十二、扫描执行器（256 槽全探 + 头部分派 + 两级无效计数落账）
// ---------------------------------------------------------------------------

/// 超时预算（累计超时达此数熔断——状态机转 Aborted）。
pub const TIMEOUT_ABORT_BUDGET: u32 = 32;

/// 扫描诊断项容量上限（报告形态与 DSDT 册同构）。
pub const MAX_SCAN_DIAGS: usize = 16;

/// 扫描输出（账本 + 进度 + 诊断 + 异常账）。
#[derive(Debug, Clone)]
pub struct ScanOutput {
    /// 扫描账本。
    pub ledger: ScanLedger,
    /// 进度状态机终态。
    pub progress: ScanProgress,
    /// BAR 账 per 功能槽（下标同账本；非在位槽为 None）。
    pub bar_ledgers: Vec<Option<BarLedger>>,
    /// 能力链账 per 功能槽（下标同账本）。
    pub cap_walks: Vec<Option<CapWalk>>,
    /// 异常账。
    pub anomalies: AnomalyLedger,
    /// 诊断项（按发生序）。
    pub diags: [Option<BxCode>; MAX_SCAN_DIAGS],
    /// 诊断项实数。
    pub diag_count: usize,
    /// 诊断截断标注。
    pub diag_truncated: bool,
}

impl ScanOutput {
    /// 追加诊断（封顶截断显性）。
    fn push_diag(&mut self, code: BxCode) {
        if self.diag_count < MAX_SCAN_DIAGS {
            self.diags[self.diag_count] = Some(code);
        } else {
            self.diag_truncated = true;
        }
        self.diag_count += 1;
    }

    /// 健康位：完成态 + 守恒成立 + 零诊断。
    pub fn healthy(&self) -> bool {
        self.progress.state == ScanState::Complete && self.ledger.conserved() && self.diag_count == 0
    }
}

/// 读设备清单账四双字（vendor/状态/类码/头类型）并合成 DevIdentity。
///
/// 任一周期 Err（超时）→ HEADER_FETCH_FAIL（功能 0 判在后的头周期
/// 失败属异常账——不静默吞成无效）。
fn read_identity(port: &mut dyn ConfigPort, bdf: Bdf, tick: u64, out: &mut ScanOutput) -> Result<DevIdentity, BxCode> {
    let w0 = match cfg_read_u32(port, bdf, 0x00) {
        Ok(v) => v,
        Err(e) => {
            out.anomalies.record(e, bdf, tick);
            return Err(BxCode::HEADER_FETCH_FAIL);
        }
    };
    let w1 = match cfg_read_u32(port, bdf, 0x04) {
        Ok(v) => v,
        Err(e) => {
            out.anomalies.record(e, bdf, tick);
            return Err(BxCode::HEADER_FETCH_FAIL);
        }
    };
    let w2 = match cfg_read_u32(port, bdf, 0x08) {
        Ok(v) => v,
        Err(e) => {
            out.anomalies.record(e, bdf, tick);
            return Err(BxCode::HEADER_FETCH_FAIL);
        }
    };
    let w3 = match cfg_read_u32(port, bdf, 0x0C) {
        Ok(v) => v,
        Err(e) => {
            out.anomalies.record(e, bdf, tick);
            return Err(BxCode::HEADER_FETCH_FAIL);
        }
    };
    let _ = w1; // 状态/命令位型由分派阶段用（能力在位位）。
    Ok(DevIdentity {
        vendor: (w0 & 0xFFFF) as u16,
        device: ((w0 >> 16) & 0xFFFF) as u16,
        revision: (w2 & 0xFF) as u8,
        base_class: ((w2 >> 24) & 0xFF) as u8,
        sub_class: ((w2 >> 16) & 0xFF) as u8,
        prog_if: ((w2 >> 8) & 0xFF) as u8,
        header_type: ((w3 >> 16) & 0x7F) as u8,
        mf_bit: mf_bit_of_header_word(w3),
    })
}

/// 头类型分派（0x00/0x01/0x02 闭集——其余 UNKNOWN_HEADER_TYPE）。
pub const fn dispatch_header(header_type: u8) -> Result<u8, BxCode> {
    match header_type & 0x7F {
        0x00 => Ok(0x00),
        0x01 => Ok(0x01),
        0x02 => Ok(0x02),
        _ => Err(BxCode::UNKNOWN_HEADER_TYPE),
    }
}

/// 总线 0 全槽位扫描（判据执行主入口）。
///
/// 规则：① 32 设备 × 8 功能全 256 槽逐个探（多功能位只做冲突账，
/// 不砍探针——探齐纪律）；② vendor 0xFFFF → 功能级无效账 +1，且功
/// 能 0 命中时设备级无效账 +1；③ 全零返回 → ghost 非法返回账 +1；
/// ④ 周期超时 → 异常账 +1，累计达预算 → 状态机 Aborted、诊断
/// TIMEOUT_BUDGET_ABORT；⑤ 在位功能做头部分派 + BAR 解析 + 能力链
/// 走查落账；⑥ 终态 Complete 前跑守恒对拍（破缺 → 诊断）。
pub fn scan_bus0(port: &mut dyn ConfigPort, start_tick: u64) -> ScanOutput {
    let mut out = ScanOutput {
        ledger: ScanLedger::new(),
        progress: ScanProgress::new(start_tick),
        bar_ledgers: vec![None; TOTAL_FUNC_SLOTS],
        cap_walks: vec![None; TOTAL_FUNC_SLOTS],
        anomalies: AnomalyLedger::new(),
        diags: [None; MAX_SCAN_DIAGS],
        diag_count: 0,
        diag_truncated: false,
    };
    let mut tick = start_tick;
    // Idle → Scanning。
    if let Err(e) = out.progress.transition(ScanState::Scanning, tick) {
        out.push_diag(e);
        return out;
    }
    let mut dev: usize = 0;
    'dev_loop: while dev < BUS_DEVICE_SLOTS {
        let mut func: usize = 0;
        while func < DEV_FUNC_SLOTS {
            tick = tick.wrapping_add(1);
            let bdf = Bdf::bus0(dev as u8, func as u8);
            let w0 = match cfg_read_u32(port, bdf, 0x00) {
                Ok(v) => v,
                Err(e) => {
                    // 周期超时：异常账 + 功能槽 Timeout。
                    out.anomalies.record(e, bdf, tick);
                    out.ledger.timeout_count += 1;
                    out.ledger.slots[bdf.slot()] = FuncSlot {
                        state: FuncState::Timeout,
                        identity: None,
                        bar_count: 0,
                        cap_count: 0,
                        probe_tick: tick,
                    };
                    out.progress.advance_one(tick);
                    if out.ledger.timeout_count >= TIMEOUT_ABORT_BUDGET {
                        out.push_diag(BxCode::TIMEOUT_BUDGET_ABORT);
                        let _ = out.progress.transition(ScanState::Aborted, tick);
                        return out;
                    }
                    func += 1;
                    continue;
                }
            };
            if w0 == 0xFFFF_FFFF {
                // 功能级无效：vendor 0xFFFF 显性计数。
                out.ledger.func_invalid_count += 1;
                out.ledger.slots[bdf.slot()] = FuncSlot {
                    state: FuncState::Invalid,
                    identity: None,
                    bar_count: 0,
                    cap_count: 0,
                    probe_tick: tick,
                };
                if func == 0 {
                    // 设备级无效：功能 0 即空 → 整槽空账。
                    out.ledger.dev_absent_count += 1;
                }
                out.progress.advance_one(tick);
                func += 1;
                continue;
            }
            if w0 == 0x0000_0000 {
                // 非法返回：全零 ghost（vendor 0x0000）。
                out.ledger.ghost_count += 1;
                out.anomalies.record(BxCode::ILLEGAL_RETURN_ALL_ZERO, bdf, tick);
                out.ledger.slots[bdf.slot()] = FuncSlot {
                    state: FuncState::Ghost,
                    identity: None,
                    bar_count: 0,
                    cap_count: 0,
                    probe_tick: tick,
                };
                out.push_diag(BxCode::ILLEGAL_RETURN_ALL_ZERO);
                out.progress.advance_one(tick);
                func += 1;
                continue;
            }
            // 在位：读清单账四双字。
            let identity = match read_identity(port, bdf, tick, &mut out) {
                Ok(id) => id,
                Err(e) => {
                    out.push_diag(e);
                    out.ledger.slots[bdf.slot()] = FuncSlot {
                        state: FuncState::Timeout,
                        identity: None,
                        bar_count: 0,
                        cap_count: 0,
                        probe_tick: tick,
                    };
                    out.ledger.timeout_count += 1;
                    out.progress.advance_one(tick);
                    if out.ledger.timeout_count >= TIMEOUT_ABORT_BUDGET {
                        out.push_diag(BxCode::TIMEOUT_BUDGET_ABORT);
                        let _ = out.progress.transition(ScanState::Aborted, tick);
                        return out;
                    }
                    func += 1;
                    continue;
                }
            };
            // 多功能位账：功能 0 快照；功能 >0 与快照冲突 → 诊断。
            if func == 0 {
                out.ledger.dev_mf[dev] = Some(identity.mf_bit);
            } else {
                match out.ledger.dev_mf[dev] {
                    Some(false) => {
                        out.ledger.mf_conflict_count += 1;
                        out.push_diag(BxCode::MF_BIT_CONFLICT);
                    }
                    _ => {}
                }
            }
            // 头部分派。
            let hdr = match dispatch_header(identity.header_type) {
                Ok(h) => h,
                Err(e) => {
                    out.push_diag(e);
                    out.ledger.slots[bdf.slot()] = FuncSlot {
                        state: FuncState::Present,
                        identity: Some(identity),
                        bar_count: 0,
                        cap_count: 0,
                        probe_tick: tick,
                    };
                    out.ledger.present_func_count += 1;
                    out.progress.advance_one(tick);
                    func += 1;
                    continue;
                }
            };
            // BAR 解析（失败不中止全扫——槽账降级记零 BAR）。
            let bar_ledger = match collect_bars(port, bdf, hdr) {
                Ok(bl) => Some(bl),
                Err(e) => {
                    out.push_diag(e);
                    None
                }
            };
            // 能力链走查（状态 bit4 在位才走）。
            let mut cap_count: u8 = 0;
            let status_cmd = match cfg_read_u32(port, bdf, 0x04) {
                Ok(v) => v,
                Err(e) => {
                    out.anomalies.record(e, bdf, tick);
                    0
                }
            };
            if cap_list_present(status_cmd) {
                if let Some(cpo) = cap_ptr_offset(hdr) {
                    // 先读指针字段取链首值（低字节按 4 对齐截取——0x34
                    // 是指针字段位置，不是链首能力本体）。
                    match cfg_read_u32(port, bdf, cpo) {
                        Ok(ptr_word) => {
                            let head = ((ptr_word & 0xFF) & 0xFC) as u8;
                            if head == 0 {
                                // 在位位立但指针为零：空链落账（规范冲突
                                // 面留诊断——CAP_PTR_BAD）。
                                out.push_diag(BxCode::CAP_PTR_BAD);
                            } else {
                                match walk_caps(port, bdf, head) {
                                    Ok(walk) => {
                                        cap_count = walk.caps.len() as u8;
                                        if let Some(ab) = walk.aborted_by {
                                            out.push_diag(ab);
                                        }
                                        out.cap_walks[bdf.slot()] = Some(walk);
                                    }
                                    Err(e) => out.push_diag(e),
                                }
                            }
                        }
                        Err(e) => out.push_diag(e),
                    }
                }
            }
            let bar_count = match &bar_ledger {
                Some(bl) => bl.bars.len() as u8,
                None => 0,
            };
            out.bar_ledgers[bdf.slot()] = bar_ledger;
            out.ledger.slots[bdf.slot()] = FuncSlot {
                state: FuncState::Present,
                identity: Some(identity),
                bar_count,
                cap_count,
                probe_tick: tick,
            };
            out.ledger.present_func_count += 1;
            out.progress.advance_one(tick);
            func += 1;
        }
        dev += 1;
        if out.progress.state == ScanState::Aborted {
            break 'dev_loop;
        }
    }
    // 终态：探齐 → Complete + 守恒对拍；未齐（理论上仅中止路径）→
    // 保持 Aborted。
    if out.progress.probed_full() {
        if let Err(e) = out.progress.transition(ScanState::Complete, tick) {
            out.push_diag(e);
            return out;
        }
        out.ledger.probed = out.progress.funcs_done;
        if let Err(e) = out.ledger.conservation_check() {
            out.push_diag(e);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 十三、重扫与账本守恒对账
// ---------------------------------------------------------------------------

/// 单槽分歧形态闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotDiff {
    /// 形态分叉（Present vs Invalid 等）。
    StateMismatch {
        /// 首扫形态。
        first: FuncState,
        /// 重扫形态。
        second: FuncState,
    },
    /// 双在位但清单账分叉（vendor/device/class/修订任一异）。
    IdentityMismatch {
        /// 首扫清单。
        first: DevIdentity,
        /// 重扫清单。
        second: DevIdentity,
    },
}

/// 槽位分歧定位（设备号 + 功能号）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotDiffAt {
    /// 设备号。
    pub dev: u8,
    /// 功能号。
    pub func: u8,
    /// 分歧形态。
    pub diff: SlotDiff,
}

/// 重扫对账账。
#[derive(Debug, Clone)]
pub struct RescanCompare {
    /// 分歧定位列表（容量封顶 MAX_ANOMALY_ENTRIES）。
    pub diffs: Vec<SlotDiffAt>,
    /// 分歧总数（含溢出未入账——不静默）。
    pub mismatch_count: u64,
    /// 两轮守恒均成立。
    pub both_conserved: bool,
    /// 探齐数两轮一致（恒 256）。
    pub probed_equal: bool,
    /// 对账结论（零分歧且守恒且探齐 → 一致）。
    pub mismatch: bool,
}

impl RescanCompare {
    /// 对账一致判定。
    pub const fn consistent(&self) -> bool {
        !self.mismatch
    }
}

/// 双轮账本守恒对账（判据「重扫与账本守恒对账」锚）。
///
/// 规则：① 双方守恒式先各自过闸；② 256 槽逐槽比形态与清单账；
/// 分歧入账封顶截断显性；③ 任一分歧 → RESCAN_MISMATCH 结论。
pub fn rescan_reconcile(first: &ScanLedger, second: &ScanLedger) -> RescanCompare {
    let mut cmp = RescanCompare {
        diffs: Vec::new(),
        mismatch_count: 0,
        both_conserved: first.conserved() && second.conserved(),
        probed_equal: first.probed == second.probed,
        mismatch: false,
    };
    if !cmp.both_conserved || !cmp.probed_equal {
        cmp.mismatch = true;
        return cmp;
    }
    let mut slot = 0usize;
    while slot < TOTAL_FUNC_SLOTS {
        let a = &first.slots[slot];
        let b = &second.slots[slot];
        if a.state != b.state {
            cmp.mismatch_count += 1;
            if cmp.diffs.len() < MAX_ANOMALY_ENTRIES {
                cmp.diffs.push(SlotDiffAt {
                    dev: (slot / DEV_FUNC_SLOTS) as u8,
                    func: (slot % DEV_FUNC_SLOTS) as u8,
                    diff: SlotDiff::StateMismatch { first: a.state, second: b.state },
                });
            }
            slot += 1;
            continue;
        }
        if a.state == FuncState::Present {
            match (a.identity, b.identity) {
                (Some(ia), Some(ib)) => {
                    if ia != ib {
                        cmp.mismatch_count += 1;
                        if cmp.diffs.len() < MAX_ANOMALY_ENTRIES {
                            cmp.diffs.push(SlotDiffAt {
                                dev: (slot / DEV_FUNC_SLOTS) as u8,
                                func: (slot % DEV_FUNC_SLOTS) as u8,
                                diff: SlotDiff::IdentityMismatch { first: ia, second: ib },
                            });
                        }
                    }
                }
                _ => {
                    // Present 槽无清单账：账内自破缺，立案。
                    cmp.mismatch_count += 1;
                }
            }
        }
        slot += 1;
    }
    cmp.mismatch = cmp.mismatch_count > 0;
    cmp
}

/// 重扫一步化：给定注入口与起点 tick，先取首扫账本再扫第二轮并对账
/// （同口同拓扑 → 确定性下应全同——分歧即拓扑漂移立案）。
pub fn rescan_bus0(port: &mut dyn ConfigPort, start_tick: u64) -> (ScanOutput, ScanOutput, RescanCompare) {
    let first = scan_bus0(port, start_tick);
    let second_tick = first.progress.last_tick.wrapping_add(1);
    let second = scan_bus0(port, second_tick);
    let cmp = rescan_reconcile(&first.ledger, &second.ledger);
    (first, second, cmp)
}

// ---------------------------------------------------------------------------
// 十四、清单导出与比对格式
// ---------------------------------------------------------------------------

/// 在位功能清单导出（判据「清单导出比对格式」锚——kv 行闭式）。
///
/// 行型：`dev.d.f vendor=0xVVVV device=0xDDDD class=0xCCSSPP rev=0xRR
/// hdr=0xH bars=N caps=N mf=0|1`；头行 `# varix-bus0-scan-report` +
/// 汇总行（探齐/在位/两级无效/ghost/超时/MF 冲突）。
pub fn export_ledger_kv(ledger: &ScanLedger) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push("# varix-bus0-scan-report".to_string());
    out.push(format!("probed={}", ledger.probed));
    out.push(format!("present_funcs={}", ledger.present_func_count));
    out.push(format!("present_devices={}", ledger.dev_present_count()));
    out.push(format!("func_invalid={}", ledger.func_invalid_count));
    out.push(format!("dev_absent={}", ledger.dev_absent_count));
    out.push(format!("ghost={}", ledger.ghost_count));
    out.push(format!("timeout={}", ledger.timeout_count));
    out.push(format!("mf_conflicts={}", ledger.mf_conflict_count));
    out.push(format!("conserved={}", ledger.conserved()));
    let mut slot = 0usize;
    while slot < TOTAL_FUNC_SLOTS {
        let s = &ledger.slots[slot];
        if s.state == FuncState::Present {
            if let Some(id) = s.identity {
                let dev = (slot / DEV_FUNC_SLOTS) as u8;
                let func = (slot % DEV_FUNC_SLOTS) as u8;
                out.push(format!(
                    "dev.{}.{} vendor=0x{:04X} device=0x{:04X} class=0x{:02X}{:02X}{:02X} rev=0x{:02X} hdr=0x{:02X} bars={} caps={} mf={}",
                    dev,
                    func,
                    id.vendor,
                    id.device,
                    id.base_class,
                    id.sub_class,
                    id.prog_if,
                    id.revision,
                    id.header_type,
                    s.bar_count,
                    s.cap_count,
                    if id.mf_bit { 1 } else { 0 }
                ));
            }
        }
        slot += 1;
    }
    out
}

/// 导出比对行集（重扫对账结论的可 diff 化渲染）。
pub fn export_compare_kv(cmp: &RescanCompare) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push("# varix-bus0-rescan-compare".to_string());
    out.push(format!("consistent={}", cmp.consistent()));
    out.push(format!("mismatch_count={}", cmp.mismatch_count));
    out.push(format!("both_conserved={}", cmp.both_conserved));
    out.push(format!("probed_equal={}", cmp.probed_equal));
    let mut i = 0usize;
    while i < cmp.diffs.len() {
        let d = &cmp.diffs[i];
        let kind = match &d.diff {
            SlotDiff::StateMismatch { .. } => "state",
            SlotDiff::IdentityMismatch { .. } => "identity",
        };
        out.push(format!("diff.{}=dev.{}.{} {}", i, d.dev, d.func, kind));
        i += 1;
    }
    out
}

/// 扫描输出全量导出（账本 kv + 诊断逐行 + 异常账逐行——单格式闭环）。
pub fn export_output_kv(out: &ScanOutput) -> Vec<String> {
    let mut lines = export_ledger_kv(&out.ledger);
    lines.push(format!("state={:?}", out.progress.state));
    lines.push(format!("diag_count={}", out.diag_count));
    lines.push(format!("diag_truncated={}", out.diag_truncated));
    let mut i = 0usize;
    while i < out.diag_count && i < MAX_SCAN_DIAGS {
        if let Some(code) = out.diags[i] {
            lines.push(format!("diag.{}=0x{:04X}", i, code.code()));
        }
        i += 1;
    }
    lines.push(format!("anomaly_count={}", out.anomalies.count));
    lines.push(format!("anomaly_truncated={}", out.anomalies.truncated));
    lines.push(format!("anomaly_timeout={}", out.anomalies.timeout_total()));
    lines.push(format!("anomaly_illegal={}", out.anomalies.illegal_total()));
    let mut i = 0usize;
    while i < out.anomalies.entries.len() {
        let a = &out.anomalies.entries[i];
        lines.push(format!(
            "anomaly.{}=0x{:04X} dev.{}.{} tick={}",
            i,
            a.code.code(),
            a.bdf.dev,
            a.bdf.func,
            a.tick
        ));
        i += 1;
    }
    lines
}

/// 导出行数闸（行数闭式对拍锚——确定性渲染守恒）。
pub fn export_ledger_expected_len(ledger: &ScanLedger) -> usize {
    // 头 1 + 汇总 9 + 在位逐行。
    10 + ledger.present_func_count as usize
}

// ---------------------------------------------------------------------------
// 十五、判据对拍常量（逐字落账——非测试面，纯账目锚）
// ---------------------------------------------------------------------------

/// 判据原文逐字锚（总线 0 全槽位扫描探齐纪律）。
pub const CRITERION_FULL_PROBE_NOTE: &str =
    "总线 0 全槽位扫描 32×8=256 功能位探齐——多功能位只做冲突账不砍探针";

/// 判据原文逐字锚（vendor 0xFFFF 两级无效计数显性）。
pub const CRITERION_INVALID_COUNT_NOTE: &str =
    "vendor 0xFFFF 无效跳过计数显性——功能级 func_invalid_count 与设备级 dev_absent_count 两级分账";
