//! UNX-F0463 · 桥展开多总线扫描（只读递归）（CoRun·Unxreal · UNX-A1-B05 · PCI 深化之二）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0463`
//!
//! **判据原文**：PCI-to-PCI 桥逐条展开（桥数=n 记账），子总线扫描深度
//! 限 8 层（超限红）。
//!
//! ## 职责定位
//!
//! 从总线 0 出发，对每个头类型 0x01（PCI-to-PCI 桥）逐条解析二级总线
//! 域（primary/secondary/subordinate + IO/内存/可预取内存三窗），按
//! 桥登记账逐桥落账（桥数=n 记账），并以显式栈（防栈溢出）递归下钻
//! 子总线。全程只读配置空间（trait 注入口，无 data_write 面），零真实
//! 端口 IO、零墙钟（逻辑步 u64 计数）、全确定性。子总线扫描深度限
//! 8 层，第 9 层超限即红色立案且不再下钻（桥本身仍逐条登记，记账
//! 完整性不因超限破缺）。
//!
//! ## 设计要点
//!
//! 1. **显式栈递归**：递归下钻不用调用栈——`WalkFrame` 显式栈承载（当前
//!    总线/深度/上游桥窗口/祖先链），弹出按（总线升序，深度次序）取最小帧；
//!    栈容量 256 封顶，打满红案不再入帧（不中止整体扫描）。
//! 2. **深度计数码**：子总线深度 = 桥深度 + 1，上限 8 层（`DEPTH_LIMIT`），
//!    第 9 层超限即 `DEPTH_LIMIT_RED` 红案不入栈不下钻——超限红不等于
//!    扫描中止，只砍下钻（桥仍登记，账全）。
//! 3. **桥数=n 记账**：每桥头类型 0x01 即登记 `BridgeRecord` 并计数；
//!    守恒式 `bridge_count == bridges.len()` 且
//!    `edges + 环红 + 双亲红 == 桥数` 闭式对拍，破缺即红。
//! 4. **环检测**：帧内祖先链 `path` 命中子总线即 `ANCESTOR_CYCLE` 红
//!    （不入边不入帧）；子总线被第二桥认领即 `MULTI_PARENT` 红（首认领有效）。
//! 5. **窗口一致性**：子设备 BAR 必须落上游桥声明窗内（IO/MEM/可预取三窗
//!    独立校验，越窗逐条 `WINDOW_VIOLATION` 红）；根总线（深度 0）跳过。
//! 6. **重叠 subordinate 区间**：同父桥 `[secondary, subordinate]` 两两相交
//!    即 `SUBORDINATE_OVERLAP` 红（兄弟必须不相交——嵌套属跨层合法）。
//! 7. **枚举去重账**：`seen` 升序键表，同一 bdf 二次到达即 `DUP_BDF` 红
//!    拒登记（守恒式依赖此约束）。
//! 8. **扫描序确定性**：帧弹出序（总线/深度）、设备/功能内层升序、账本
//!    终局三键排序（总线/设备/功能）三重保证。
//! 9. **拓扑导出双形态**：缩进树（显式栈 DFS 层次渲染）+ 边清单（逐边
//!    父→子 + 桥 bdf + subordinate 上界）。
//! 10. **增量重扫对账**：两轮桥账按 bdf 键两指针归并——新桥/消失桥/
//!     窗口漂移三分账（`RESCAN_*` 三码闭集）。
//! 11. **异常账**：周期超时入异常账计数，预算熔断转 Aborted；
//!     悬空桥（subordinate 区间零在账设备/桥）终局补红立案。
//! 12. **零 panic 面**：所有输入不可信，错误全走 Err + 中文诊断；
//!     no_std + alloc，绝不 use std；禁浮点禁墙钟（逻辑步 u64）。
//! 13. **诊断码独占 0x7Cxx 段**：unx462（0x7Bxx）相邻段，grep 同目录
//!     核对无诊断码占用（0x463x 已被 unx574 T574 段占用——以 grep 为准）。

#![forbid(unsafe_op_in_unsafe_fn)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码闭集（独占 0x7Cxx 段）
// ---------------------------------------------------------------------------

/// 桥展开扫描诊断码。独占 `0x7Cxx` 段（unx462 占 0x7Bxx；0x463x 已被
/// unx574 T574 段占用——grep 同目录核对后选定本段，无撞）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BwCode(pub u16);

impl BwCode {
    /// 子总线扫描深度超 8 层上限（红色立案，不再下钻）。
    pub const DEPTH_LIMIT_RED: BwCode = BwCode(0x7C01);
    /// 配置周期超时（注入口显性报错）。
    pub const CYCLE_TIMEOUT: BwCode = BwCode(0x7C02);
    /// 非法头：全零 ghost（vendor 0x0000）或保留头类型。
    pub const ILLEGAL_HEADER: BwCode = BwCode(0x7C03);
    /// 桥头域非法（subordinate < secondary / 自环 / 窗口倒挂）。
    pub const BRIDGE_HEADER_BAD: BwCode = BwCode(0x7C04);
    /// 子设备地址越桥声明窗口。
    pub const WINDOW_VIOLATION: BwCode = BwCode(0x7C05);
    /// 同父桥 subordinate 区间重叠。
    pub const SUBORDINATE_OVERLAP: BwCode = BwCode(0x7C06);
    /// 环：子总线上溯回指祖先。
    pub const ANCESTOR_CYCLE: BwCode = BwCode(0x7C07);
    /// 同一 bdf 重复登记（去重账拒绝二次登记）。
    pub const DUP_BDF: BwCode = BwCode(0x7C08);
    /// 显式栈容量打满（帧不再入栈）。
    pub const STACK_CAP_EXHAUSTED: BwCode = BwCode(0x7C09);
    /// 地址位域溢出（设备 >31 / 功能 >7）。
    pub const ADDR_FIELD_OVERFLOW: BwCode = BwCode(0x7C0A);
    /// 寄存器偏移非 4 字节对齐。
    pub const OFFSET_UNALIGNED: BwCode = BwCode(0x7C0B);
    /// 寄存器偏移越界（>0xFC，256 字节配置空间）。
    pub const OFFSET_OUT_OF_RANGE: BwCode = BwCode(0x7C0C);
    /// 状态机非法迁移（非闭式转移边）。
    pub const STATE_INVALID_TRANSITION: BwCode = BwCode(0x7C0D);
    /// 增量重扫：新桥（上轮无此桥）。
    pub const RESCAN_NEW_BRIDGE: BwCode = BwCode(0x7C0E);
    /// 增量重扫：消失桥（本轮无此桥）。
    pub const RESCAN_LOST_BRIDGE: BwCode = BwCode(0x7C0F);
    /// 增量重扫：窗口漂移（同桥 bdf、窗口域变值）。
    pub const RESCAN_WINDOW_DRIFT: BwCode = BwCode(0x7C10);
    /// 超时预算熔断（扫描中止）。
    pub const TIMEOUT_BUDGET_ABORT: BwCode = BwCode(0x7C11);
    /// 悬空桥（subordinate 区间内零在账设备/桥）。
    pub const DANGLING_BRIDGE: BwCode = BwCode(0x7C12);
    /// 桥数记账守恒破缺。
    pub const LEDGER_CONSERVATION_BROKEN: BwCode = BwCode(0x7C13);
    /// primary 与上游总线号不符。
    pub const PRIMARY_MISMATCH: BwCode = BwCode(0x7C14);
    /// 子总线被第二座桥认领（首认领有效）。
    pub const MULTI_PARENT: BwCode = BwCode(0x7C15);
    /// CF8 使能位（bit31）未置——地址非配置周期形态。
    pub const ENABLE_BIT_CLEAR: BwCode = BwCode(0x7C16);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            BwCode::DEPTH_LIMIT_RED => "子总线扫描深度超限：第 9 层起红色立案且不再下钻（上限 8 层）".into(),
            BwCode::CYCLE_TIMEOUT => "配置周期超时：注入口未在确定性界内返回".into(),
            BwCode::ILLEGAL_HEADER => "非法头：全零 ghost 或保留头类型（非 0x00/0x01/0x02）".into(),
            BwCode::BRIDGE_HEADER_BAD => "桥头域非法：subordinate < secondary 或自环或窗口基址 > 限界".into(),
            BwCode::WINDOW_VIOLATION => "窗口一致性破缺：子设备 BAR 地址落在上游桥声明窗口之外".into(),
            BwCode::SUBORDINATE_OVERLAP => "同父桥 subordinate 区间重叠：兄弟桥总线区间必须不相交".into(),
            BwCode::ANCESTOR_CYCLE => "拓扑环：子总线上溯回指祖先总线".into(),
            BwCode::DUP_BDF => "枚举去重账冲突：同一 bdf 二次到达，拒绝重复登记".into(),
            BwCode::STACK_CAP_EXHAUSTED => "显式栈容量打满：新帧不再入栈（防栈溢出封顶）".into(),
            BwCode::ADDR_FIELD_OVERFLOW => "地址位域溢出：设备 >31 或功能 >7".into(),
            BwCode::OFFSET_UNALIGNED => "寄存器偏移非 4 字节对齐：配置空间按双字访问".into(),
            BwCode::OFFSET_OUT_OF_RANGE => "寄存器偏移越界：256 字节配置空间上限 0xFC".into(),
            BwCode::STATE_INVALID_TRANSITION => "状态机非法迁移：非闭式转移边".into(),
            BwCode::RESCAN_NEW_BRIDGE => "增量重扫对账：出现上轮不存在的桥".into(),
            BwCode::RESCAN_LOST_BRIDGE => "增量重扫对账：上轮在账的桥本轮消失".into(),
            BwCode::RESCAN_WINDOW_DRIFT => "增量重扫对账：同桥 bdf 窗口域变值".into(),
            BwCode::TIMEOUT_BUDGET_ABORT => "超时预算熔断：扫描状态机转入 Aborted".into(),
            BwCode::DANGLING_BRIDGE => "悬空桥：subordinate 区间内零在账设备与零在账桥".into(),
            BwCode::LEDGER_CONSERVATION_BROKEN => {
                "桥数记账守恒破缺：桥数 ≠ 登记数或边数 + 环红 + 双亲红 ≠ 桥数".into()
            }
            BwCode::PRIMARY_MISMATCH => "primary 与上游总线号不符：桥头声明与扫描路径分叉".into(),
            BwCode::MULTI_PARENT => "子总线被第二座桥认领：首认领有效，后到立案".into(),
            BwCode::ENABLE_BIT_CLEAR => "CF8 使能位（bit31）未置：地址非配置周期形态".into(),
            BwCode(_) => "未知 UNX 桥展开扫描域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、常量与 BDF 三元组
// ---------------------------------------------------------------------------

/// 配置周期地址使能位（bit31）。
pub const CFG_ENABLE_BIT: u32 = 0x8000_0000;
/// 配置空间 256 字节总长。
pub const CFG_SPACE_BYTES: usize = 256;
/// 单总线设备槽位数。
pub const BUS_DEVICE_SLOTS: usize = 32;
/// 单设备功能位数。
pub const DEV_FUNC_SLOTS: usize = 8;
/// 子总线扫描深度上限（判据数：8 层，第 9 层超限红）。
pub const DEPTH_LIMIT: u8 = 8;
/// 显式栈容量封顶（防栈溢出硬上界）。
pub const STACK_CAP: usize = 256;
/// 超时预算（累计超时超此数熔断转 Aborted）。
pub const TIMEOUT_BUDGET: u32 = 64;
/// 头类型：一般设备。
pub const HEADER_TYPE_GENERAL: u8 = 0x00;
/// 头类型：PCI-to-PCI 桥。
pub const HEADER_TYPE_BRIDGE: u8 = 0x01;
/// 头类型：CardBus 卡。
pub const HEADER_TYPE_CARDBUS: u8 = 0x02;
/// vendor 无效面（空功能槽）。
pub const VENDOR_INVALID: u16 = 0xFFFF;

/// BDF（总线/设备/功能）三元组（多总线全域版——总线 0..255）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BwBdf {
    /// 总线号 0..255。
    pub bus: u8,
    /// 设备号 0..31。
    pub dev: u8,
    /// 功能号 0..7。
    pub func: u8,
}

impl BwBdf {
    /// 构造（界外即拒——构造器不 panic）。
    pub const fn new(bus: u8, dev: u8, func: u8) -> Result<BwBdf, BwCode> {
        if dev > 31 || func > 7 {
            return Err(BwCode::ADDR_FIELD_OVERFLOW);
        }
        Ok(BwBdf { bus, dev, func })
    }

    /// 三键排序键：总线升序 / 设备升序 / 功能升序（单一 u32 全序）。
    pub const fn key(self) -> u32 {
        ((self.bus as u32) << 12) | ((self.dev as u32) << 3) | (self.func as u32)
    }

    /// 由键还原 bdf（键域闭式可逆）。
    pub const fn from_key(key: u32) -> BwBdf {
        BwBdf {
            bus: ((key >> 12) & 0xFF) as u8,
            dev: ((key >> 3) & 0x1F) as u8,
            func: (key & 0x07) as u8,
        }
    }

    /// bdf 人话形（"01:08.0"）。
    pub fn render(self) -> String {
        format!("{:02X}:{:02X}.{}", self.bus, self.dev, self.func)
    }
}

/// 由三分量直接取键（免构造，纯函数）。
pub const fn bdf_key(bus: u8, dev: u8, func: u8) -> u32 {
    ((bus as u32) << 12) | ((dev as u32) << 3) | (func as u32)
}

// ---------------------------------------------------------------------------
// 三、地址编解码与配置空间访问口（只读——无 data_write 面）
// ---------------------------------------------------------------------------

/// 配置周期地址编码（纯函数；使能位 + 总线 16..23 + 设备 11..15 +
/// 功能 8..10 + 寄存器 2..7，低 2 位恒 0——双字对齐硬件约束）。
pub const fn encode_cfg(bus: u8, dev: u8, func: u8, off: u8) -> Result<u32, BwCode> {
    if dev > 31 || func > 7 {
        return Err(BwCode::ADDR_FIELD_OVERFLOW);
    }
    if (off as usize) >= CFG_SPACE_BYTES {
        return Err(BwCode::OFFSET_OUT_OF_RANGE);
    }
    if off & 0x03 != 0 {
        return Err(BwCode::OFFSET_UNALIGNED);
    }
    Ok(CFG_ENABLE_BIT
        | ((bus as u32) & 0xFF) << 16
        | ((dev as u32) & 0x1F) << 11
        | ((func as u32) & 0x07) << 8
        | (off as u32 & 0xFC))
}

/// 配置周期地址解码（编码恒等逆——对拍面）。
pub const fn decode_cfg(addr: u32) -> Result<(u8, u8, u8, u8), BwCode> {
    if addr & CFG_ENABLE_BIT == 0 {
        return Err(BwCode::ENABLE_BIT_CLEAR);
    }
    Ok((
        ((addr >> 16) & 0xFF) as u8,
        ((addr >> 11) & 0x1F) as u8,
        ((addr >> 8) & 0x07) as u8,
        (addr & 0xFC) as u8,
    ))
}

/// PCI 配置空间只读访问口（可注入抽象——不做真实端口 IO）。
///
/// 两阶段纪律显性：先 `addr_latch`（对应 CF8 写），后 `data_read`
/// （对应 CFC 读）。本模块只读递归——trait 面不设 data_write，
/// 实现方自管锁存态，扫描器只按序调用不窥内部。
pub trait BridgeConfigPort {
    /// 阶段一：锁存配置周期地址（CF8 写等价）。
    fn addr_latch(&mut self, addr: u32) -> Result<(), BwCode>;
    /// 阶段二：读数据周期（CFC 读等价，对最近锁存地址）。
    fn data_read(&mut self) -> Result<u32, BwCode>;
}

/// 两阶段读单步：编码 → 锁存 → 读数据。任何一步 Err 即透传。
pub fn bw_read_u32(
    port: &mut dyn BridgeConfigPort,
    bdf: BwBdf,
    off: u8,
) -> Result<u32, BwCode> {
    let addr = encode_cfg(bdf.bus, bdf.dev, bdf.func, off)?;
    port.addr_latch(addr)?;
    port.data_read()
}

/// 两阶段双字内 16 位半字读（低半/高半按偏移位 1 选择）。
pub fn bw_read_u16(
    port: &mut dyn BridgeConfigPort,
    bdf: BwBdf,
    off: u8,
) -> Result<u16, BwCode> {
    if off & 0x01 != 0 {
        return Err(BwCode::OFFSET_UNALIGNED);
    }
    let word = bw_read_u32(port, bdf, off & 0xFC)?;
    if off & 0x02 != 0 {
        Ok((word >> 16) as u16)
    } else {
        Ok(word as u16)
    }
}

/// 两阶段双字内单字节读。
pub fn bw_read_u8(port: &mut dyn BridgeConfigPort, bdf: BwBdf, off: u8) -> Result<u8, BwCode> {
    let word = bw_read_u32(port, bdf, off & 0xFC)?;
    let shift = ((off & 0x03) as u32) * 8;
    Ok((word >> shift) as u8)
}

// ---------------------------------------------------------------------------
// 四、PCI-to-PCI 桥头解析（头类型 0x01 二级总线域 + 三窗）
// ---------------------------------------------------------------------------

/// IO 窗口基址合成（低字节寄存器 bit7..4 → 地址 15..12，高 16 位
/// 寄存器 → 地址 31..16；低 4 位为 32 位窗口标志位域，不参与地址）。
pub const fn io_base_compose(lo: u8, hi: u16) -> u32 {
    ((lo as u32 & 0xF0) << 8) | ((hi as u32) << 16)
}

/// IO 窗口限界合成（4KB 粒度：低 12 位补 F）。
pub const fn io_limit_compose(lo: u8, hi: u16) -> u32 {
    ((lo as u32 & 0xF0) << 8) | 0x0FFF | ((hi as u32) << 16)
}

/// 内存窗口基址合成（1MB 粒度：寄存器 bit15..4 → 地址 31..20）。
pub const fn mem_base_compose(v: u16) -> u32 {
    ((v as u32) & 0xFFF0) << 16
}

/// 内存窗口限界合成（1MB 粒度：低 20 位补 F）。
pub const fn mem_limit_compose(v: u16) -> u32 {
    (((v as u32) & 0xFFF0) << 16) | 0x000F_FFFF
}

/// 可预取内存窗口 64 位基址合成（高 32 位寄存器 → 地址 63..32）。
pub const fn pf_base_compose(v: u16, hi: u32) -> u64 {
    (((v as u64) & 0xFFF0) << 16) | ((hi as u64) << 32)
}

/// 可预取内存窗口 64 位限界合成。
pub const fn pf_limit_compose(v: u16, hi: u32) -> u64 {
    (((v as u64) & 0xFFF0) << 16) | 0x000F_FFFF | ((hi as u64) << 32)
}

/// PCI-to-PCI 桥头域账（头类型 0x01 专用解析产物）。
///
/// 字段为合成后的真实地址形态（寄存器原值不保——粒度补位闭式
/// 可逆），窗口语义见各组合纯函数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeHeader {
    /// 一级总线号（@0x18）——应等于上游扫描总线。
    pub primary: u8,
    /// 二级总线号（@0x19）——下钻目标。
    pub secondary: u8,
    /// 从属总线号上界（@0x1A）——子树最大总线号。
    pub subordinate: u8,
    /// 二级状态寄存器（@0x1E）。
    pub secondary_status: u16,
    /// IO 窗口基址（32 位合成形态）。
    pub io_base: u32,
    /// IO 窗口限界（4KB 粒度补位形态）。
    pub io_limit: u32,
    /// IO 窗口是否 32 位形态（高 16 位寄存器非零）。
    pub io_32bit: bool,
    /// 内存窗口基址（32 位合成形态）。
    pub mem_base: u32,
    /// 内存窗口限界（1MB 粒度补位形态）。
    pub mem_limit: u32,
    /// 可预取内存窗口基址（64 位合成形态）。
    pub pfb_base: u64,
    /// 可预取内存窗口限界（64 位合成形态）。
    pub pfb_limit: u64,
    /// 可预取窗口是否 64 位形态（高 32 位寄存器非零）。
    pub pfb_64bit: bool,
}

impl BridgeHeader {
    /// 桥声明窗口集（基址限界双零视为窗口未声明——None）。
    pub fn windows(&self) -> BridgeWindowSet {
        let io = if self.io_base == 0 && self.io_limit == 0 {
            None
        } else {
            Some((self.io_base, self.io_limit))
        };
        let mem = if self.mem_base == 0 && self.mem_limit == 0 {
            None
        } else {
            Some((self.mem_base, self.mem_limit))
        };
        let pfb = if self.pfb_base == 0 && self.pfb_limit == 0 {
            None
        } else {
            Some((self.pfb_base, self.pfb_limit))
        };
        BridgeWindowSet { io, mem, pfb }
    }

    /// 桥头域人话摘要（区间 + 三窗形态）。
    pub fn render(&self) -> String {
        format!(
            "区间 [{},{}] IO[{:04X},{:04X}]{} MEM[{:08X},{:08X}] PFB{}",
            self.secondary,
            self.subordinate,
            self.io_base,
            self.io_limit,
            if self.io_32bit { "(32位)" } else { "(16位)" },
            self.mem_base,
            self.mem_limit,
            if self.pfb_64bit { "[64位]" } else { "" }
        )
    }
}

/// 从配置空间读全桥头域（只读十步序列：0x18 三字节 + 0x1E +
/// 0x1C/0x1D + 0x30/0x32 + 0x20 + 0x24 + 0x28/0x2C）。
///
/// 任一周期 Err 即透传（超时由调用侧入异常账）；字段合成走本节
/// 纯函数，不做二次校验（校验在 `validate_bridge_header`）。
pub fn read_bridge_header(
    port: &mut dyn BridgeConfigPort,
    bdf: BwBdf,
) -> Result<BridgeHeader, BwCode> {
    let primary = bw_read_u8(port, bdf, 0x18)?;
    let secondary = bw_read_u8(port, bdf, 0x19)?;
    let subordinate = bw_read_u8(port, bdf, 0x1A)?;
    let secondary_status = bw_read_u16(port, bdf, 0x1E)?;
    let io_base_lo = bw_read_u8(port, bdf, 0x1C)?;
    let io_limit_lo = bw_read_u8(port, bdf, 0x1D)?;
    let io_base_hi = bw_read_u16(port, bdf, 0x30)?;
    let io_limit_hi = bw_read_u16(port, bdf, 0x32)?;
    let mem_raw = bw_read_u32(port, bdf, 0x20)?;
    let mem_base_v = mem_raw as u16;
    let mem_limit_v = (mem_raw >> 16) as u16;
    let pf_raw = bw_read_u32(port, bdf, 0x24)?;
    let pfb_base_v = pf_raw as u16;
    let pfb_limit_v = (pf_raw >> 16) as u16;
    let pfb_base_hi = bw_read_u32(port, bdf, 0x28)?;
    let pfb_limit_hi = bw_read_u32(port, bdf, 0x2C)?;
    Ok(BridgeHeader {
        primary,
        secondary,
        subordinate,
        secondary_status,
        io_base: io_base_compose(io_base_lo, io_base_hi),
        io_limit: io_limit_compose(io_limit_lo, io_limit_hi),
        io_32bit: io_base_hi != 0 || io_limit_hi != 0,
        mem_base: mem_base_compose(mem_base_v),
        mem_limit: mem_limit_compose(mem_limit_v),
        pfb_base: pf_base_compose(pfb_base_v, pfb_base_hi),
        pfb_limit: pf_limit_compose(pfb_limit_v, pfb_limit_hi),
        pfb_64bit: pfb_base_hi != 0 || pfb_limit_hi != 0,
    })
}

/// 桥头域合法性校验（纯函数）。
///
/// 规则：① secondary 为零——桥指向总线 0 属非法回环面（总线 0
/// 只能是根，不允许被桥认领）；② subordinate < secondary——区间
/// 倒挂；③ secondary == primary——自环；④ 三窗任一 基址 > 限界
/// ——窗口倒挂。任一命中即对应诊断码。
pub fn validate_bridge_header(h: &BridgeHeader) -> Result<(), BwCode> {
    if h.secondary == 0 {
        return Err(BwCode::BRIDGE_HEADER_BAD);
    }
    if h.subordinate < h.secondary {
        return Err(BwCode::BRIDGE_HEADER_BAD);
    }
    if h.secondary == h.primary {
        return Err(BwCode::BRIDGE_HEADER_BAD);
    }
    if h.io_base > h.io_limit {
        return Err(BwCode::BRIDGE_HEADER_BAD);
    }
    if h.mem_base > h.mem_limit {
        return Err(BwCode::BRIDGE_HEADER_BAD);
    }
    if h.pfb_base > h.pfb_limit {
        return Err(BwCode::BRIDGE_HEADER_BAD);
    }
    Ok(())
}

/// 区间相交纯函数（闭区间两两判交）。
pub const fn intervals_intersect(a_lo: u8, a_hi: u8, b_lo: u8, b_hi: u8) -> bool {
    a_lo <= b_hi && b_lo <= a_hi
}

// ---------------------------------------------------------------------------
// 五、窗口集与 BAR 越窗校验（窗口一致性判据锚）
// ---------------------------------------------------------------------------

/// 桥声明窗口集（IO/内存/可预取三窗独立，None = 未声明）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeWindowSet {
    /// IO 窗口（基址，限界）。
    pub io: Option<(u32, u32)>,
    /// 内存窗口（基址，限界）。
    pub mem: Option<(u32, u32)>,
    /// 可预取内存窗口（基址，限界，64 位面）。
    pub pfb: Option<(u64, u64)>,
}

impl BridgeWindowSet {
    /// 全空窗口（根总线帧专用——无上游桥，跳过窗口校验）。
    pub const NONE: BridgeWindowSet = BridgeWindowSet { io: None, mem: None, pfb: None };
}

/// 地址是否落窗（纯函数）。
pub const fn addr_in_win32(addr: u32, base: u32, limit: u32) -> bool {
    addr >= base && addr <= limit
}

/// 64 位地址是否落窗（纯函数）。
pub const fn addr_in_win64(addr: u64, base: u64, limit: u64) -> bool {
    addr >= base && addr <= limit
}

/// 单条 BAR 越窗命中账（校验产物，立案在调用侧）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinHit {
    /// BAR 号 0..5。
    pub bar_no: u8,
    /// 是否 IO BAR。
    pub io: bool,
    /// 是否可预取内存 BAR。
    pub prefetchable: bool,
    /// 去位型后的地址。
    pub addr: u32,
    /// 64 位形态（类型位 10）。
    pub mem64: bool,
}

/// BAR 原值数组对窗口集的越窗校验（纯函数，只产命中账不立案）。
///
/// 规则：① 原值零——未实现 BAR，跳过；② IO BAR（bit0=1）取
/// 低 32 位地址对 IO 窗；③ 内存非预取对内存窗；④ 可预取内存
/// 对可预取窗（未声明则退对内存窗）；⑤ 窗未声明而 BAR 在用——
/// 同样命中（设备用了桥没给的窗）。命中即越窗，逐条留痕。
pub fn bar_window_hits(bars: &[u32; 6], win: &BridgeWindowSet) -> Vec<WinHit> {
    let mut hits: Vec<WinHit> = Vec::new();
    for (i, &raw) in bars.iter().enumerate() {
        if raw == 0 {
            continue;
        }
        let is_io = raw & 0x01 != 0;
        let mem64 = !is_io && (raw >> 1) & 0x03 == 0b10;
        let prefetchable = !is_io && (raw & 0x08 != 0);
        if is_io {
            let addr = raw & 0xFFFF_FFFC;
            let hit = match win.io {
                Some((b, l)) => !addr_in_win32(addr, b, l),
                None => true,
            };
            if hit {
                hits.push(WinHit {
                    bar_no: i as u8,
                    io: true,
                    prefetchable: false,
                    addr,
                    mem64: false,
                });
            }
        } else {
            let addr = raw & 0xFFFF_FFF0;
            let hit = if prefetchable {
                match win.pfb {
                    Some((b, l)) => !addr_in_win64(addr as u64, b, l),
                    None => match win.mem {
                        Some((b, l)) => !addr_in_win32(addr, b, l),
                        None => true,
                    },
                }
            } else {
                match win.mem {
                    Some((b, l)) => !addr_in_win32(addr, b, l),
                    None => true,
                }
            };
            if hit {
                hits.push(WinHit {
                    bar_no: i as u8,
                    io: false,
                    prefetchable,
                    addr,
                    mem64,
                });
            }
        }
    }
    hits
}

// ---------------------------------------------------------------------------
// 六、确定性多总线拓扑（默认注入口数据源——全确定性零 IO）
// ---------------------------------------------------------------------------

/// 确定性桥定义（头类型 0x01 的二级总线域寄存器原值形态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtBridgeDef {
    /// 一级总线号寄存器原值。
    pub primary: u8,
    /// 二级总线号寄存器原值。
    pub secondary: u8,
    /// 从属总线号寄存器原值。
    pub subordinate: u8,
    /// IO 基址低字节寄存器。
    pub io_base_lo: u8,
    /// IO 限界低字节寄存器。
    pub io_limit_lo: u8,
    /// IO 基址高 16 位寄存器。
    pub io_base_hi: u16,
    /// IO 限界高 16 位寄存器。
    pub io_limit_hi: u16,
    /// 内存基址寄存器（16 位，1MB 粒度）。
    pub mem_base: u16,
    /// 内存限界寄存器（16 位，1MB 粒度）。
    pub mem_limit: u16,
    /// 可预取基址寄存器（16 位）。
    pub pfb_base: u16,
    /// 可预取限界寄存器（16 位）。
    pub pfb_limit: u16,
    /// 可预取基址高 32 位寄存器。
    pub pfb_base_hi: u32,
    /// 可预取限界高 32 位寄存器。
    pub pfb_limit_hi: u32,
}

impl VirtBridgeDef {
    /// 闭区间桥速造（三窗全零——窗口未声明形态）。
    pub const fn closed(primary: u8, secondary: u8, subordinate: u8) -> VirtBridgeDef {
        VirtBridgeDef { primary, secondary, subordinate, io_base_lo: 0, io_limit_lo: 0, io_base_hi: 0, io_limit_hi: 0, mem_base: 0, mem_limit: 0, pfb_base: 0, pfb_limit: 0, pfb_base_hi: 0, pfb_limit_hi: 0 }
    }

    /// 带内存窗速造（IO/可预取窗全零）。
    pub const fn with_mem(
        primary: u8,
        secondary: u8,
        subordinate: u8,
        mem_base: u16,
        mem_limit: u16,
    ) -> VirtBridgeDef {
        VirtBridgeDef { primary, secondary, subordinate, io_base_lo: 0, io_limit_lo: 0, io_base_hi: 0, io_limit_hi: 0, mem_base, mem_limit, pfb_base: 0, pfb_limit: 0, pfb_base_hi: 0, pfb_limit_hi: 0 }
    }

    /// 带 IO + 内存窗速造。
    pub const fn with_io_mem(
        primary: u8,
        secondary: u8,
        subordinate: u8,
        io_base_lo: u8,
        io_limit_lo: u8,
        mem_base: u16,
        mem_limit: u16,
    ) -> VirtBridgeDef {
        VirtBridgeDef { primary, secondary, subordinate, io_base_lo, io_limit_lo, io_base_hi: 0, io_limit_hi: 0, mem_base, mem_limit, pfb_base: 0, pfb_limit: 0, pfb_base_hi: 0, pfb_limit_hi: 0 }
    }
}

/// 确定性功能规格（配置空间字生成源）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtFunc {
    /// 功能号 0..7。
    pub func: u8,
    /// vendor ID。
    pub vendor: u16,
    /// device ID。
    pub device: u16,
    /// 修订 ID。
    pub revision: u8,
    /// 类码三元组 [基类, 子类, 编程接口]。
    pub class: [u8; 3],
    /// 头类型低 7 位。
    pub header_type: u8,
    /// BAR 原值（头类型 0x00 六条 / 0x01 前两条 / 0x02 首条）。
    pub bars: [u32; 6],
    /// 桥域（头类型 0x01 才有语义）。
    pub bridge: Option<VirtBridgeDef>,
}

impl VirtFunc {
    /// 一般设备规格速造（头类型 0x00；bars 超六条截断——速造器不 panic）。
    pub fn endpoint(func: u8, vendor: u16, device: u16, class: [u8; 3], bars: &[u32]) -> VirtFunc {
        let mut bar_arr = [0u32; 6];
        for (i, &v) in bars.iter().take(6).enumerate() {
            bar_arr[i] = v;
        }
        VirtFunc {
            func,
            vendor,
            device,
            revision: 0x01,
            class,
            header_type: HEADER_TYPE_GENERAL,
            bars: bar_arr,
            bridge: None,
        }
    }

    /// 桥规格速造（头类型 0x01）。
    pub fn bridge_func(
        func: u8,
        vendor: u16,
        device: u16,
        class: [u8; 3],
        def: VirtBridgeDef,
    ) -> VirtFunc {
        VirtFunc {
            func,
            vendor,
            device,
            revision: 0x01,
            class,
            header_type: HEADER_TYPE_BRIDGE,
            bars: [0u32; 6],
            bridge: Some(def),
        }
    }

    /// CardBus 规格速造（头类型 0x02，首 BAR 为卡基址）。
    pub fn cardbus_func(func: u8, vendor: u16, device: u16, class: [u8; 3], bar0: u32) -> VirtFunc {
        let mut bars = [0u32; 6];
        bars[0] = bar0;
        VirtFunc {
            func,
            vendor,
            device,
            revision: 0x01,
            class,
            header_type: HEADER_TYPE_CARDBUS,
            bars,
            bridge: None,
        }
    }
}

/// 确定性设备规格（设备号 + 功能集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtDev {
    /// 设备号 0..31。
    pub dev: u8,
    /// 功能规格集。
    pub funcs: Vec<VirtFunc>,
}

/// 确定性总线规格（总线号 + 设备集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtBus {
    /// 总线号。
    pub bus: u8,
    /// 设备规格集。
    pub devs: Vec<VirtDev>,
}

/// 确定性多总线拓扑（默认注入口数据源）。
///
/// `standard()` 覆盖判据全链演练点：正常桥/悬空桥/同父区间重叠/
/// 双亲认领/越窗设备/在窗设备/8 层深度链 + 第 9 层超限红/周期
/// 超时/CardBus——红账六类 + 异常账两类全可离线复现。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualTopo {
    /// 总线规格表（按总线号可查）。
    pub buses: Vec<VirtBus>,
    /// 周期超时演练面（这些 bdf 键的所有配置周期报超时）。
    pub timeouts: Vec<u32>,
}

impl VirtualTopo {
    /// 空拓扑。
    pub fn empty() -> VirtualTopo {
        VirtualTopo { buses: Vec::new(), timeouts: Vec::new() }
    }

    /// 追加功能规格（总线/设备不存在即沿途建；设备号界外即忽略）。
    pub fn add_func(&mut self, bus: u8, dev: u8, f: VirtFunc) {
        if dev > 31 {
            return;
        }
        match self.buses.iter_mut().find(|b| b.bus == bus) {
            None => {
                self.buses.push(VirtBus { bus, devs: vec![VirtDev { dev, funcs: vec![f] }] });
            }
            Some(vb) => match vb.devs.iter_mut().find(|d| d.dev == dev) {
                Some(vd) => vd.funcs.push(f),
                None => vb.devs.push(VirtDev { dev, funcs: vec![f] }),
            },
        }
    }

    /// 设周期超时演练位（链式）。
    pub fn with_timeout(mut self, bus: u8, dev: u8, func: u8) -> VirtualTopo {
        self.timeouts.push(bdf_key(bus, dev, func));
        self
    }

    /// 查功能规格。
    pub fn find_func(&self, bus: u8, dev: u8, func: u8) -> Option<&VirtFunc> {
        let vb = self.buses.iter().find(|b| b.bus == bus)?;
        let vd = vb.devs.iter().find(|d| d.dev == dev)?;
        vd.funcs.iter().find(|f| f.func == func)
    }

    /// 标准演示拓扑（判据全链演练面）。
    ///
    /// 布局：
    /// - 总线 0：dev 0x00 主桥端点；dev 0x08 根桥 A→[1..9]（IO+MEM 窗）；
    ///   dev 0x0C 根桥 B→[10..10]（总线 10 不在拓扑——悬空桥演练点）。
    /// - 总线 1：dev 0x00 桥→[2..2]；dev 0x05 网卡（BAR 在窗）；
    ///   dev 0x06 越窗设备（双 BAR 全越窗）；dev 0x10 深度链首桥→[3..9]；
    ///   dev 0x18 重叠桥→[3..3]（区间相交 + 双亲认领——双红演练点）。
    /// - 总线 2：dev 0x00 端点（BAR 在窗）；dev 0x08 CardBus；dev 0x1F 超时。
    /// - 总线 3..9：深度链桥（逐层深度 2..8）；总线 9 的桥→[11..11] 超限红。
    /// - 总线 10/11：不在拓扑（悬空 + 超限不下钻双演练点）。
    pub fn standard() -> VirtualTopo {
        // 标准演示拓扑统一桥 vendor/device/class 速造器。
        const BCLS: [u8; 3] = [0x06, 0x04, 0x00];
        fn br(func: u8, def: VirtBridgeDef) -> VirtFunc {
            VirtFunc::bridge_func(func, 0x8086, 0x244E, BCLS, def)
        }
        let mut topo = VirtualTopo::empty();
        // 总线 0（深度 0）：主桥端点 + 根桥 A→[1..9]（IO+MEM 窗）+ 根桥 B→[10..10]（悬空演练点）。
        topo.add_func(0, 0x00, VirtFunc::endpoint(0, 0x8086, 0x1237, [0x06, 0x00, 0x00], &[]));
        topo.add_func(0, 0x08, br(0, VirtBridgeDef::with_io_mem(0, 1, 9, 0x11, 0x11, 0xC000, 0xC0F0)));
        topo.add_func(0, 0x0C, br(0, VirtBridgeDef::closed(0, 10, 10)));
        // 总线 1（深度 1，窗口 = 根桥 A：IO [0x1000,0x1FFF]，MEM [0xC0000000,0xC0FFFFFF]）。
        topo.add_func(1, 0x00, br(0, VirtBridgeDef::with_io_mem(1, 2, 2, 0x21, 0x2F, 0xC100, 0xC1F0)));
        topo.add_func(1, 0x05, VirtFunc::endpoint(0, 0x10EC, 0x8168, [0x02, 0x00, 0x00], &[0x0000_1041, 0xC000_4000]));
        topo.add_func(1, 0x06, VirtFunc::endpoint(0, 0x1234, 0x6001, [0x01, 0x06, 0x01], &[0x0000_9001, 0xE000_0000]));
        topo.add_func(1, 0x10, br(0, VirtBridgeDef::with_mem(1, 3, 9, 0xC200, 0xC2F0)));
        topo.add_func(1, 0x18, br(0, VirtBridgeDef::closed(1, 3, 3))); // 重叠 + 双亲双红演练点
        // 总线 2（深度 2，窗口 = 总线 1 dev 0x00：IO [0x2000,0x2FFF]，MEM [0xC1000000,0xC1FFFFFF]）。
        topo.add_func(2, 0x00, VirtFunc::endpoint(0, 0x10EC, 0x8139, [0x02, 0x00, 0x00], &[0xC100_8000]));
        topo.add_func(2, 0x08, VirtFunc::cardbus_func(0, 0x104C, 0xAC51, [0x06, 0x07, 0x00], 0xC100_4000));
        // 总线 3..9：深度链桥（逐层深度 2..8；每层 MEM 窗错开互不重叠）。
        topo.add_func(3, 0x10, br(0, VirtBridgeDef::with_mem(3, 4, 9, 0xC400, 0xC4F0)));
        topo.add_func(4, 0x10, br(0, VirtBridgeDef::with_mem(4, 5, 9, 0xC500, 0xC5F0)));
        topo.add_func(5, 0x10, br(0, VirtBridgeDef::with_mem(5, 6, 9, 0xC600, 0xC6F0)));
        topo.add_func(6, 0x10, br(0, VirtBridgeDef::with_mem(6, 7, 9, 0xC700, 0xC7F0)));
        topo.add_func(7, 0x10, br(0, VirtBridgeDef::with_mem(7, 8, 9, 0xC800, 0xC8F0)));
        topo.add_func(8, 0x10, br(0, VirtBridgeDef::with_mem(8, 9, 9, 0xC900, 0xC9F0)));
        // 总线 9（深度 8）：其桥指向总线 11——子总线深度 9 超限红。
        topo.add_func(9, 0x10, br(0, VirtBridgeDef::closed(9, 11, 11)));
        // 周期超时演练位：总线 2 dev 0x1F。
        topo.with_timeout(2, 0x1F, 0)
    }
}

/// 配置空间字合成（确定性拓扑 → 双字流）。
///
/// 未在拓扑的功能槽恒 0xFFFF_FFFF（vendor 无效面）；字序号 wi
/// 即配置偏移 wi*4。
pub fn virt_word(topo: &VirtualTopo, bus: u8, dev: u8, func: u8, wi: usize) -> u32 {
    let f = match topo.find_func(bus, dev, func) {
        Some(f) => f,
        None => return 0xFFFF_FFFF,
    };
    match wi {
        0 => (f.vendor as u32) | ((f.device as u32) << 16),
        1 => 0x0210_0007, // 命令 0x0007 + 状态 0x0210（能力表在位位）
        2 => {
            (f.revision as u32)
                | ((f.class[2] as u32) << 8)
                | ((f.class[1] as u32) << 16)
                | ((f.class[0] as u32) << 24)
        }
        3 => (f.header_type as u32) << 16,
        _ => virt_ext_word(f, wi),
    }
}

/// 配置空间扩展字合成（0x10 起按头类型分派）。
fn virt_ext_word(f: &VirtFunc, wi: usize) -> u32 {
    match f.header_type {
        HEADER_TYPE_GENERAL => {
            if wi >= 4 && wi < 10 {
                f.bars[wi - 4]
            } else {
                0
            }
        }
        HEADER_TYPE_BRIDGE => {
            let def = match f.bridge {
                Some(d) => d,
                None => return 0,
            };
            match wi {
                4 | 5 => f.bars[wi - 4],
                6 => {
                    (def.primary as u32)
                        | ((def.secondary as u32) << 8)
                        | ((def.subordinate as u32) << 16)
                }
                7 => (def.io_base_lo as u32) | ((def.io_limit_lo as u32) << 8),
                8 => (def.mem_base as u32) | ((def.mem_limit as u32) << 16),
                9 => (def.pfb_base as u32) | ((def.pfb_limit as u32) << 16),
                10 => def.pfb_base_hi,
                11 => def.pfb_limit_hi,
                12 => (def.io_base_hi as u32) | ((def.io_limit_hi as u32) << 16),
                _ => 0,
            }
        }
        HEADER_TYPE_CARDBUS => {
            if wi == 4 {
                f.bars[0]
            } else {
                0
            }
        }
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// 七、默认注入口：确定性只读配置口（CF8/CFC 两阶段行为模型）
// ---------------------------------------------------------------------------

/// 默认配置口（确定性构造数据——判据全链可离线演练）。
///
/// 行为模型：① 锁存态自管；② 读按拓扑合成双字（不在拓扑的功能
/// 槽恒 0xFFFF_FFFF）；③ 超时演练位按 bdf 键报 CYCLE_TIMEOUT；
/// ④ 无写面——只读递归判据锚。
pub struct DeterministicBridgePort {
    /// 拓扑源。
    topo: VirtualTopo,
    /// 最近锁存地址。
    latched: Option<u32>,
    /// 累计周期数（确定性审计面）。
    cycle_count: u64,
}

impl DeterministicBridgePort {
    /// 以给定拓扑造口。
    pub fn new(topo: VirtualTopo) -> DeterministicBridgePort {
        DeterministicBridgePort { topo, latched: None, cycle_count: 0 }
    }

    /// 标准演示拓扑造口。
    pub fn standard() -> DeterministicBridgePort {
        DeterministicBridgePort::new(VirtualTopo::standard())
    }

    /// 累计周期数（审计面只读）。
    pub fn cycles(&self) -> u64 {
        self.cycle_count
    }
}

impl BridgeConfigPort for DeterministicBridgePort {
    fn addr_latch(&mut self, addr: u32) -> Result<(), BwCode> {
        // 编码形态闸：使能位必须在（锁存非配置形态地址即拒）。
        match decode_cfg(addr) {
            Ok(_) => {}
            Err(e) => return Err(e),
        }
        self.latched = Some(addr);
        self.cycle_count += 1;
        Ok(())
    }

    fn data_read(&mut self) -> Result<u32, BwCode> {
        let addr = match self.latched {
            Some(a) => a,
            None => return Err(BwCode::ENABLE_BIT_CLEAR),
        };
        self.cycle_count += 1;
        let (bus, dev, func, off) = match decode_cfg(addr) {
            Ok(t) => t,
            Err(e) => return Err(e),
        };
        if self.topo.timeouts.contains(&bdf_key(bus, dev, func)) {
            return Err(BwCode::CYCLE_TIMEOUT);
        }
        Ok(virt_word(&self.topo, bus, dev, func, (off / 4) as usize))
    }
}

// ---------------------------------------------------------------------------
// 八、登记账：功能记录 / 桥记录 / 拓扑边 / 红账 / 异常账
// ---------------------------------------------------------------------------

/// 在账功能记录（端点与 CardBus——桥走独立账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FuncRecord {
    /// 位置 bdf。
    pub bdf: BwBdf,
    /// vendor ID。
    pub vendor: u16,
    /// device ID。
    pub device: u16,
    /// 修订 ID。
    pub revision: u8,
    /// 类码三元组。
    pub class: [u8; 3],
    /// 头类型低 7 位（0x00 / 0x02——0x01 走桥账）。
    pub header_type: u8,
    /// BAR 原值（6 条，未实现为 0）。
    pub bars: [u32; 6],
}

/// 在账桥记录（判据「桥数=n 记账」锚——逐桥一条）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeRecord {
    /// 桥自身位置 bdf。
    pub bdf: BwBdf,
    /// 上游总线号（扫描路径事实）。
    pub parent_bus: u8,
    /// 桥头域全量。
    pub header: BridgeHeader,
    /// 桥所在深度（根总线 0 层）。
    pub depth: u8,
}

impl BridgeRecord {
    /// 桥人话摘要（位置 + 区间 + 深度）。
    pub fn render(&self) -> String {
        format!(
            "桥 {}（深度 {}，区间 [{},{}]，上游总线 {}）",
            self.bdf.render(),
            self.depth,
            self.header.secondary,
            self.header.subordinate,
            self.parent_bus
        )
    }
}

/// 拓扑边（父总线 → 子总线，由桥认领产生）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopoEdge {
    /// 父总线号。
    pub parent_bus: u8,
    /// 子总线号（桥 secondary）。
    pub child_bus: u8,
    /// 认领桥的 bdf 键。
    pub bridge_key: u32,
    /// 从属总线号上界（桥 subordinate）。
    pub subordinate: u8,
}

/// 红色立案账条目（判据违例/异常形态统一载体——判据「超限红」锚）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedEntry {
    /// 诊断码。
    pub code: BwCode,
    /// 立案位置（尽可能精确到 bdf；总线级红以 dev/func 零占位）。
    pub at: BwBdf,
    /// 人话详情。
    pub detail: String,
}

impl RedEntry {
    /// 红账条目人话行。
    pub fn render(&self) -> String {
        format!("[{:04X}] @{} {}", self.code.code(), self.at.render(), self.detail)
    }
}

/// 异常账条目（配置周期超时等运行异常——异常是账不是中断）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcEntry {
    /// 诊断码。
    pub code: BwCode,
    /// 异常位置。
    pub at: BwBdf,
    /// 人话详情。
    pub detail: String,
}

// ---------------------------------------------------------------------------
// 九、显式栈状态机（递归下钻的确定性载体）
// ---------------------------------------------------------------------------

/// 扫描状态机状态（闭式转移集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkState {
    /// 初始态：栈已备好根帧，未开始。
    Idle,
    /// 扫描中。
    Walking,
    /// 挂起（可恢复——显式栈状态在，恢复即续扫）。
    Suspended,
    /// 正常完成（栈空，终局补检已过）。
    Done,
    /// 异常中止（超时预算熔断）。
    Aborted,
}

/// 闭式转移边判定（纯函数）：Idle→Walking；Walking→Suspended/
/// Done/Aborted；Suspended→Walking；Done/Aborted 终态不可出。
pub const fn state_transition_ok(from: WalkState, to: WalkState) -> bool {
    match (from, to) {
        (WalkState::Idle, WalkState::Walking) => true,
        (WalkState::Walking, WalkState::Suspended) => true,
        (WalkState::Walking, WalkState::Done) => true,
        (WalkState::Walking, WalkState::Aborted) => true,
        (WalkState::Suspended, WalkState::Walking) => true,
        _ => false,
    }
}

/// 显式栈帧（一层子总线扫描的完整上下文）。
///
/// 调用栈递归被显式栈替代——`path` 祖先链承载环检测上下文，
/// `window` 承载窗口一致性校验上下文，`depth` 承载深度限计数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkFrame {
    /// 当前扫描总线号。
    pub bus: u8,
    /// 当前深度（根总线 0，每过一桥 +1；上限 DEPTH_LIMIT）。
    pub depth: u8,
    /// 上游桥声明窗口集（深度 0 恒 NONE）。
    pub window: BridgeWindowSet,
    /// 祖先链（根总线到当前总线，含两端——环检测面）。
    pub path: Vec<u8>,
}

// ---------------------------------------------------------------------------
// 十、桥展开扫描器（主状态机 + 三键排序账本）
// ---------------------------------------------------------------------------

/// 桥展开扫描器。
///
/// 持有显式栈与全部账本；`run` 逐帧弹出-扫描-入帧直至栈空或熔断。
/// 扫描序确定性：帧弹出按（总线升序，深度次序）取最小帧；总线内
/// 设备/功能双层升序；账本终局按 bdf 三键排序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeWalker {
    /// 状态机状态。
    state: WalkState,
    /// 显式栈（防栈溢出——调用栈零递归）。
    stack: Vec<WalkFrame>,
    /// 桥登记账（终局按 bdf 键排序）。
    bridges: Vec<BridgeRecord>,
    /// 功能登记账（终局按 bdf 键排序）。
    funcs: Vec<FuncRecord>,
    /// 拓扑边清单（终局按 父总线/子总线 排序）。
    edges: Vec<TopoEdge>,
    /// 枚举去重账（升序 bdf 键——同一 bdf 只登记一次）。
    seen: Vec<u32>,
    /// 子总线认领账（升序总线号——首认领有效）。
    claimed_buses: Vec<u8>,
    /// 红色立案账。
    reds: Vec<RedEntry>,
    /// 异常账（周期超时等）。
    exceptions: Vec<ExcEntry>,
    /// 累计超时数。
    timeout_count: u32,
    /// 桥数=n 记账计数器（与 bridges.len() 守恒对拍）。
    bridge_count: u32,
    /// 逻辑步计数（确定性 tick，替代墙钟）。
    step_count: u64,
    /// 达到的最大深度。
    max_depth: u8,
    /// 无效功能槽计数（vendor 0xFFFF——显性不静默）。
    invalid_func_slots: u64,
    /// 全零 ghost 槽计数（vendor 0x0000）。
    ghost_slots: u64,
    /// CardBus 功能计数。
    cardbus_count: u32,
}

impl BridgeWalker {
    /// 新扫描器（Idle 态，栈备根帧——总线 0 深度 0 无窗）。
    pub fn new() -> BridgeWalker {
        let root = WalkFrame {
            bus: 0,
            depth: 0,
            window: BridgeWindowSet::NONE,
            path: vec![0u8],
        };
        BridgeWalker {
            state: WalkState::Idle,
            stack: vec![root],
            bridges: Vec::new(),
            funcs: Vec::new(),
            edges: Vec::new(),
            seen: Vec::new(),
            claimed_buses: Vec::new(),
            reds: Vec::new(),
            exceptions: Vec::new(),
            timeout_count: 0,
            bridge_count: 0,
            step_count: 0,
            max_depth: 0,
            invalid_func_slots: 0,
            ghost_slots: 0,
            cardbus_count: 0,
        }
    }

    // -- 状态机 --

    /// 当前状态。
    pub fn state(&self) -> WalkState {
        self.state
    }

    /// 状态迁移（闭式边闸——非法迁移即 Err，状态不动）。
    fn transition(&mut self, to: WalkState) -> Result<(), BwCode> {
        if state_transition_ok(self.state, to) {
            self.state = to;
            Ok(())
        } else {
            Err(BwCode::STATE_INVALID_TRANSITION)
        }
    }

    /// 开始扫描（Idle → Walking）。
    pub fn begin(&mut self) -> Result<(), BwCode> {
        self.transition(WalkState::Walking)
    }

    /// 挂起（Walking → Suspended——显式栈状态在，可恢复）。
    pub fn suspend(&mut self) -> Result<(), BwCode> {
        self.transition(WalkState::Suspended)
    }

    /// 恢复（Suspended → Walking）。
    pub fn resume(&mut self) -> Result<(), BwCode> {
        self.transition(WalkState::Walking)
    }

    // -- 只读访问面 --

    /// 桥登记账。
    pub fn bridges(&self) -> &[BridgeRecord] {
        &self.bridges
    }

    /// 功能登记账。
    pub fn funcs(&self) -> &[FuncRecord] {
        &self.funcs
    }

    /// 拓扑边清单。
    pub fn edges(&self) -> &[TopoEdge] {
        &self.edges
    }

    /// 红色立案账。
    pub fn reds(&self) -> &[RedEntry] {
        &self.reds
    }

    /// 异常账。
    pub fn exceptions(&self) -> &[ExcEntry] {
        &self.exceptions
    }

    /// 桥数=n 记账读数。
    pub fn bridge_count(&self) -> u32 {
        self.bridge_count
    }

    /// 逻辑步计数。
    pub fn step_count(&self) -> u64 {
        self.step_count
    }

    /// 达到的最大深度。
    pub fn max_depth(&self) -> u8 {
        self.max_depth
    }

    /// 无效功能槽计数。
    pub fn invalid_func_slots(&self) -> u64 {
        self.invalid_func_slots
    }

    /// 显式栈深（审计面）。
    pub fn stack_depth(&self) -> usize {
        self.stack.len()
    }

    /// bdf 是否已登记（去重账查询——二分）。
    pub fn registered(&self, key: u32) -> bool {
        self.seen.binary_search(&key).is_ok()
    }

    /// 按 bdf 键查桥（账已终局排序——线性定位足用）。
    pub fn find_bridge_by_key(&self, key: u32) -> Option<&BridgeRecord> {
        self.bridges.iter().find(|b| b.bdf.key() == key)
    }

    /// 按诊断码数红。
    pub fn count_red(&self, code: BwCode) -> usize {
        self.reds.iter().filter(|r| r.code == code).count()
    }

    // -- 内部账务 --

    /// 推红（立案统一入口——详情必附人话）。
    fn red(&mut self, code: BwCode, at: BwBdf, detail: String) {
        self.reds.push(RedEntry { code, at, detail });
    }

    /// 记超时（异常账 + 预算熔断闸）。
    fn note_timeout(&mut self, at: BwBdf, off: u8) -> Result<(), BwCode> {
        self.timeout_count += 1;
        self.exceptions.push(ExcEntry {
            code: BwCode::CYCLE_TIMEOUT,
            at,
            detail: format!("偏移 0x{:02X} 配置周期超时（第 {} 次）", off, self.timeout_count),
        });
        if self.timeout_count > TIMEOUT_BUDGET {
            self.red(
                BwCode::TIMEOUT_BUDGET_ABORT,
                at,
                format!("累计超时 {} 次超预算 {}——扫描中止", self.timeout_count, TIMEOUT_BUDGET),
            );
            self.transition(WalkState::Aborted)?;
            return Err(BwCode::TIMEOUT_BUDGET_ABORT);
        }
        Ok(())
    }

    /// 弹出最小帧（总线升序，深度次序——扫描序确定性锚）。
    fn pop_first_frame(&mut self) -> Option<WalkFrame> {
        if self.stack.is_empty() {
            return None;
        }
        let mut best = 0usize;
        for i in 1..self.stack.len() {
            if (self.stack[i].bus, self.stack[i].depth)
                < (self.stack[best].bus, self.stack[best].depth)
            {
                best = i;
            }
        }
        Some(self.stack.remove(best))
    }

    /// 终局排序（三键确定性：桥/功能按 bdf 键，边按 父→子）。
    fn sort_ledgers(&mut self) {
        self.bridges.sort_by_key(|b| b.bdf.key());
        self.funcs.sort_by_key(|f| f.bdf.key());
        self.edges.sort_by_key(|e| ((e.parent_bus as u32) << 8) | (e.child_bus as u32));
    }

    /// 账本守恒判定（纯查账——桥数=n 记账锚）。
    ///
    /// 三式：① 桥计数器 == 桥账长度；② 去重账长度 == 桥账 + 功能账
    /// （每个在账 bdf 恰记一次）；③ 边数 + 环红 + 双亲红 == 桥数
    /// （每桥恰一出口：合法入边或环红或双亲红）。
    pub fn conservation_ok(&self) -> bool {
        if self.bridge_count != self.bridges.len() as u32 {
            return false;
        }
        if self.seen.len() != self.bridges.len() + self.funcs.len() {
            return false;
        }
        self.edges.len() + self.count_red(BwCode::ANCESTOR_CYCLE) + self.count_red(BwCode::MULTI_PARENT)
            == self.bridges.len()
    }

    // -- 主循环 --

    /// 全量扫描（须 Walking 态——Idle 先 begin，Suspended 先 resume）。
    ///
    /// 逐帧弹出-扫描-入帧直至栈空（Done）或超时熔断（Aborted）；
    /// 栈空后跑终局补检（悬空桥 + 守恒 + 排序），返回汇总报告。
    /// 返回 Err 仅当注入口出现非超时形态故障（端口滥用）。
    pub fn run(&mut self, port: &mut dyn BridgeConfigPort) -> Result<WalkReport, BwCode> {
        if self.state != WalkState::Walking {
            return Err(BwCode::STATE_INVALID_TRANSITION);
        }
        while !self.stack.is_empty() {
            if self.state != WalkState::Walking {
                break;
            }
            self.step_count += 1;
            let frame = match self.pop_first_frame() {
                Some(f) => f,
                None => break,
            };
            match self.scan_bus(port, &frame) {
                Ok(()) => {}
                // 熔断：状态机已被 note_timeout 打到 Aborted——收账出报告。
                Err(BwCode::TIMEOUT_BUDGET_ABORT) => {}
                Err(e) => return Err(e),
            }
        }
        if self.state == WalkState::Walking {
            self.finish_pass();
            self.transition(WalkState::Done)?;
        } else {
            // 异常终局也要排序落账（确定性不因中止破缺）。
            self.sort_ledgers();
        }
        Ok(self.report())
    }

    /// 单总线扫描（32 设备 × 8 功能全探齐——探齐纪律同 unx462）。
    fn scan_bus(&mut self, port: &mut dyn BridgeConfigPort, frame: &WalkFrame) -> Result<(), BwCode> {
        for dev in 0..(BUS_DEVICE_SLOTS as u8) {
            for func in 0..(DEV_FUNC_SLOTS as u8) {
                if self.state != WalkState::Walking {
                    return Ok(());
                }
                self.step_count += 1;
                let bdf = BwBdf { bus: frame.bus, dev, func };
                // 第一步：vendor 判在（超时入异常账——预算熔断即收）。
                let w0 = match bw_read_u32(port, bdf, 0x00) {
                    Ok(v) => v,
                    Err(BwCode::CYCLE_TIMEOUT) => {
                        self.note_timeout(bdf, 0x00)?;
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                let vendor = w0 as u16;
                if vendor == VENDOR_INVALID {
                    // 空功能槽：显性计数不静默。
                    self.invalid_func_slots += 1;
                    continue;
                }
                if vendor == 0x0000 {
                    // 全零 ghost：vendor 0x0000 非法返回面。
                    self.ghost_slots += 1;
                    self.red(
                        BwCode::ILLEGAL_HEADER,
                        bdf,
                        "全零 ghost 功能槽：vendor 0x0000 非法返回".to_string(),
                    );
                    continue;
                }
                // 去重账：同一 bdf 只登记一次。
                let key = bdf.key();
                if self.registered(key) {
                    self.red(
                        BwCode::DUP_BDF,
                        bdf,
                        format!("bdf 键 0x{:04X} 已在账——二次到达拒绝登记", key),
                    );
                    continue;
                }
                // 第二步：头类型读。
                let w3 = match bw_read_u32(port, bdf, 0x0C) {
                    Ok(v) => v,
                    Err(BwCode::CYCLE_TIMEOUT) => {
                        self.note_timeout(bdf, 0x0C)?;
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                let hdr_type = ((w3 >> 16) & 0x7F) as u8;
                // 第三步：revision/class 读。
                let w2 = match bw_read_u32(port, bdf, 0x08) {
                    Ok(v) => v,
                    Err(BwCode::CYCLE_TIMEOUT) => {
                        self.note_timeout(bdf, 0x08)?;
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                let revision = w2 as u8;
                let class = [
                    ((w2 >> 24) & 0xFF) as u8,
                    ((w2 >> 16) & 0xFF) as u8,
                    ((w2 >> 8) & 0xFF) as u8,
                ];
                match hdr_type {
                    HEADER_TYPE_BRIDGE => {
                        self.bridge_flow(port, frame, bdf)?;
                    }
                    HEADER_TYPE_GENERAL => {
                        self.endpoint_flow(port, frame, bdf, vendor, (w0 >> 16) as u16, revision, class)?;
                    }
                    HEADER_TYPE_CARDBUS => {
                        // CardBus：登记账不递归（卡总线扫描不在本册判据面）。
                        self.cardbus_count += 1;
                        let rec = FuncRecord {
                            bdf,
                            vendor,
                            device: (w0 >> 16) as u16,
                            revision,
                            class,
                            header_type: HEADER_TYPE_CARDBUS,
                            bars: [0u32; 6],
                        };
                        self.seen.push(key);
                        self.funcs.push(rec);
                    }
                    _ => {
                        self.red(
                            BwCode::ILLEGAL_HEADER,
                            bdf,
                            format!("保留头类型 0x{:02X}——不入账", hdr_type),
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// 桥流程：读头 → 校验 → 登记 → 区间重叠/双亲/环/深度四闸 → 入帧。
    fn bridge_flow(
        &mut self,
        port: &mut dyn BridgeConfigPort,
        frame: &WalkFrame,
        bdf: BwBdf,
    ) -> Result<(), BwCode> {
        // 桥头域全量读取（超时入异常账——读不完不登记）。
        let header = match read_bridge_header(port, bdf) {
            Ok(h) => h,
            Err(BwCode::CYCLE_TIMEOUT) => {
                self.note_timeout(bdf, 0x18)?;
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        // 桥头域合法性闸。
        if let Err(c) = validate_bridge_header(&header) {
            self.red(c, bdf, format!("桥头域校验不过：{}（{}）", c.reason(), header.render()));
            return Ok(());
        }
        // primary 一致性：声明应等于扫描路径事实（红但继续登记——账全）。
        if header.primary != frame.bus {
            self.red(
                BwCode::PRIMARY_MISMATCH,
                bdf,
                format!("primary={} 上游总线={}——桥头声明与扫描路径分叉", header.primary, frame.bus),
            );
        }
        // 登记（去重账已在上游闸——此处必新）。
        let key = bdf.key();
        self.seen.push(key);
        self.bridge_count += 1;
        let rec = BridgeRecord { bdf, parent_bus: frame.bus, header, depth: frame.depth };
        // 同父兄弟桥区间重叠闸（登记前把命中兄弟的区间值拷出，避免借用冲突）。
        let overlap_with = self
            .bridges
            .iter()
            .find(|prev| {
                prev.parent_bus == frame.bus
                    && intervals_intersect(
                        prev.header.secondary,
                        prev.header.subordinate,
                        header.secondary,
                        header.subordinate,
                    )
            })
            .map(|prev| (prev.header.secondary, prev.header.subordinate));
        self.bridges.push(rec);
        if let Some((plo, phi)) = overlap_with {
            self.red(
                BwCode::SUBORDINATE_OVERLAP,
                bdf,
                format!(
                    "同父桥区间重叠：[{},{}] 与已入账 [{},{}]（上游总线 {}）",
                    header.secondary, header.subordinate, plo, phi, frame.bus
                ),
            );
        }
        // 认领闸：首认领有效。
        let child = header.secondary;
        if self.claimed_buses.contains(&child) {
            self.red(BwCode::MULTI_PARENT, bdf, format!("子总线 {} 已被前桥认领——本桥认领立案不入边", child));
            return Ok(());
        }
        // 环闸：子总线上溯不得回指祖先（祖先链含当前总线）。
        if frame.path.contains(&child) {
            self.red(BwCode::ANCESTOR_CYCLE, bdf, format!("子总线 {} 命中祖先链——拓扑环立案不入边不下钻", child));
            return Ok(());
        }
        // 认领 + 入边。
        self.claimed_buses.push(child);
        self.edges.push(TopoEdge {
            parent_bus: frame.bus,
            child_bus: child,
            bridge_key: key,
            subordinate: header.subordinate,
        });
        // 深度闸：子总线深度超 8 层即红——红色立案不继续下钻
        // （判据锚：桥本身已逐条登记，记账完整性不因超限破缺）。
        // saturating：帧深度构造上有界（≤8），此处再设防杜绝任何溢出面。
        let child_depth = frame.depth.saturating_add(1);
        if child_depth > DEPTH_LIMIT {
            self.red(
                BwCode::DEPTH_LIMIT_RED,
                bdf,
                format!("子总线 {} 深度 {} 超 8 层上限——红色立案不再下钻（桥已登记）", child, child_depth),
            );
            return Ok(());
        }
        // 栈容量闸（防栈溢出封顶）。
        if self.stack.len() >= STACK_CAP {
            self.red(BwCode::STACK_CAP_EXHAUSTED, bdf, format!("显式栈已满 {} 帧——子总线 {} 不入栈", STACK_CAP, child));
            return Ok(());
        }
        // 入帧（深度计数推进 + 窗口/祖先链继承）。
        if child_depth > self.max_depth {
            self.max_depth = child_depth;
        }
        let mut path = frame.path.clone();
        path.push(child);
        self.stack.push(WalkFrame { bus: child, depth: child_depth, window: header.windows(), path });
        Ok(())
    }

    /// 端点流程：登记 + BAR 窗口一致性校验（深度 > 0 才有上游窗）。
    fn endpoint_flow(
        &mut self,
        port: &mut dyn BridgeConfigPort,
        frame: &WalkFrame,
        bdf: BwBdf,
        vendor: u16,
        device: u16,
        revision: u8,
        class: [u8; 3],
    ) -> Result<(), BwCode> {
        // 六条 BAR 只读采集（超时入异常账——采到几条记几条）。
        let mut bars = [0u32; 6];
        for (i, bar) in bars.iter_mut().enumerate() {
            let off = (0x10 + i * 4) as u8;
            match bw_read_u32(port, bdf, off) {
                Ok(v) => *bar = v,
                Err(BwCode::CYCLE_TIMEOUT) => {
                    self.note_timeout(bdf, off)?;
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        let key = bdf.key();
        self.seen.push(key);
        self.funcs.push(FuncRecord {
            bdf,
            vendor,
            device,
            revision,
            class,
            header_type: HEADER_TYPE_GENERAL,
            bars,
        });
        // 窗口一致性：仅桥后子总线（深度 > 0）校验；根总线无上游窗。
        if frame.depth > 0 {
            let hits = bar_window_hits(&bars, &frame.window);
            for h in hits {
                let kind = if h.io { "IO" } else if h.prefetchable { "可预取MEM" } else { "MEM" };
                self.red(
                    BwCode::WINDOW_VIOLATION,
                    bdf,
                    format!("BAR{}（{}）地址 0x{:08X} 越上游桥声明窗口（深度 {}）", h.bar_no, kind, h.addr, frame.depth),
                );
            }
        }
        Ok(())
    }

    /// 终局补检：悬空桥立案 + 账本排序 + 守恒对拍。
    fn finish_pass(&mut self) {
        self.sort_ledgers();
        // 悬空桥：subordinate 区间内零在账端点/功能且零其他桥。
        let mut dangling: Vec<(BwBdf, u8, u8)> = Vec::new();
        for (bi, br) in self.bridges.iter().enumerate() {
            let (lo, hi) = (br.header.secondary, br.header.subordinate);
            let func_served = self.funcs.iter().any(|f| f.bdf.bus >= lo && f.bdf.bus <= hi);
            let bridge_served = self
                .bridges
                .iter()
                .enumerate()
                .any(|(bj, b)| bj != bi && b.bdf.bus >= lo && b.bdf.bus <= hi);
            if !func_served && !bridge_served {
                dangling.push((br.bdf, lo, hi));
            }
        }
        for (bdf, lo, hi) in dangling {
            self.red(
                BwCode::DANGLING_BRIDGE,
                bdf,
                format!("悬空桥：区间 [{},{}] 内零在账功能与零其他桥", lo, hi),
            );
        }
        // 守恒对拍：破缺即红（账面自身完整性判据）。
        if !self.conservation_ok() {
            let at = BwBdf { bus: 0, dev: 0, func: 0 };
            self.red(
                BwCode::LEDGER_CONSERVATION_BROKEN,
                at,
                format!(
                    "桥计数 {} / 桥账 {} / 去重账 {} / 边 {} + 环红 {} + 双亲红 {}",
                    self.bridge_count, self.bridges.len(), self.seen.len(), self.edges.len(),
                    self.count_red(BwCode::ANCESTOR_CYCLE), self.count_red(BwCode::MULTI_PARENT)
                ),
            );
        }
    }

    /// 汇总报告（快照——不持有借用）。
    pub fn report(&self) -> WalkReport {
        WalkReport {
            state: self.state,
            bridge_count: self.bridge_count,
            func_count: self.funcs.len() as u32,
            edge_count: self.edges.len() as u32,
            red_count: self.reds.len() as u32,
            timeout_count: self.timeout_count,
            invalid_func_slots: self.invalid_func_slots,
            ghost_slots: self.ghost_slots,
            cardbus_count: self.cardbus_count,
            max_depth_reached: self.max_depth,
            step_count: self.step_count,
            conservation_ok: self.conservation_ok(),
        }
    }
}

// ---------------------------------------------------------------------------
// 十一、汇总报告（判据读数快照）
// ---------------------------------------------------------------------------

/// 扫描汇总报告（判据读数快照——桥数/深度/红账/守恒全显性）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkReport {
    /// 终局状态。
    pub state: WalkState,
    /// 桥数=n 记账读数（判据锚一）。
    pub bridge_count: u32,
    /// 在账功能数（端点 + CardBus）。
    pub func_count: u32,
    /// 拓扑边数。
    pub edge_count: u32,
    /// 红账条数。
    pub red_count: u32,
    /// 累计超时数。
    pub timeout_count: u32,
    /// 无效功能槽数（vendor 0xFFFF）。
    pub invalid_func_slots: u64,
    /// 全零 ghost 槽数（vendor 0x0000）。
    pub ghost_slots: u64,
    /// CardBus 功能数。
    pub cardbus_count: u32,
    /// 达到的最大深度（判据锚二：深度限 8 层的实测读数）。
    pub max_depth_reached: u8,
    /// 逻辑步计数。
    pub step_count: u64,
    /// 账本守恒是否通过。
    pub conservation_ok: bool,
}

impl WalkReport {
    /// 报告人话多行渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("状态：{:?}\n", self.state));
        s.push_str(&format!("桥数=n 记账：{} 座桥\n", self.bridge_count));
        s.push_str(&format!("在账功能：{}（CardBus {}）\n", self.func_count, self.cardbus_count));
        s.push_str(&format!("拓扑边：{} 条\n", self.edge_count));
        s.push_str(&format!("最大深度：{} 层（上限 {} 层）\n", self.max_depth_reached, DEPTH_LIMIT));
        s.push_str(&format!(
            "红账 {} 条 / 超时 {} 次 / 无效槽 {} / ghost {}\n",
            self.red_count, self.timeout_count, self.invalid_func_slots, self.ghost_slots
        ));
        s.push_str(&format!(
            "守恒：{}（逻辑步 {}）\n",
            if self.conservation_ok { "通过" } else { "破缺" },
            self.step_count
        ));
        s
    }
}

// ---------------------------------------------------------------------------
// 十二、拓扑导出（缩进树 + 边清单双形态）
// ---------------------------------------------------------------------------

impl BridgeWalker {
    /// 缩进树导出（父总线→子总线层次形态——DFS 显式栈防递归）。
    ///
    /// 行格式：`<缩进>总线 N [深度 d] <- 桥 bb:dd.f [sec..sub]`；
    /// 每总线只渲染一次（双亲立案的总线不重复渲染）。
    pub fn export_indent_tree(&self) -> String {
        let mut out = String::new();
        // 邻接表：父总线 → 子边下标表（每父按子总线升序）。
        let mut adj: Vec<(u8, Vec<usize>)> = Vec::new();
        for ei in 0..self.edges.len() {
            let parent = self.edges[ei].parent_bus;
            match adj.iter_mut().find(|a| a.0 == parent) {
                Some(a) => a.1.push(ei),
                None => adj.push((parent, vec![ei])),
            }
        }
        for a in adj.iter_mut() {
            a.1.sort_by_key(|&idx| self.edges[idx].child_bus);
        }
        // 显式栈 DFS：帧 = (总线号，深度，经桥键)；子边逆序入栈 → 弹出即子总线升序。
        let mut visited: Vec<u8> = Vec::new();
        let mut dfs: Vec<(u8, usize, u32)> = vec![(0u8, 0usize, 0u32)];
        while let Some((bus, depth, via_key)) = dfs.pop() {
            if visited.contains(&bus) {
                continue;
            }
            visited.push(bus);
            let mut line = String::new();
            for _ in 0..depth {
                line.push_str("  ");
            }
            if via_key == 0 && depth == 0 {
                line.push_str(&format!("总线 {} [深度 {}]（根）", bus, depth));
            } else {
                let vb = BwBdf::from_key(via_key);
                line.push_str(&format!("总线 {} [深度 {}] <- 桥 {}", bus, depth, vb.render()));
            }
            out.push_str(&line);
            out.push('\n');
            let kids: Vec<usize> = match adj.iter().find(|a| a.0 == bus) {
                Some(a) => a.1.clone(),
                None => Vec::new(),
            };
            for &idx in kids.iter().rev() {
                let e = self.edges[idx];
                if !visited.contains(&e.child_bus) {
                    dfs.push((e.child_bus, depth + 1, e.bridge_key));
                }
            }
        }
        out
    }

    /// 边清单导出（逐边一行——父→子 + 认领桥 + subordinate 上界）。
    pub fn export_edge_list(&self) -> String {
        let mut out = String::new();
        if self.edges.is_empty() {
            out.push_str("（无边：单总线拓扑）\n");
            return out;
        }
        for (i, e) in self.edges.iter().enumerate() {
            let vb = BwBdf::from_key(e.bridge_key);
            out.push_str(&format!(
                "#{} 总线 {} -> 总线 {}（桥 {}，subordinate={}）\n",
                i + 1,
                e.parent_bus,
                e.child_bus,
                vb.render(),
                e.subordinate
            ));
        }
        out
    }

    /// 扫描序审计：账本是否三键升序（终局排序后应恒真）。
    pub fn scan_order_sorted(&self) -> bool {
        let bridges_sorted = self
            .bridges
            .windows(2)
            .all(|w| w[0].bdf.key() <= w[1].bdf.key());
        let funcs_sorted = self.funcs.windows(2).all(|w| w[0].bdf.key() <= w[1].bdf.key());
        bridges_sorted && funcs_sorted
    }
}

// ---------------------------------------------------------------------------
// 十三、增量重扫对账（新桥 / 消失桥 / 窗口漂移三分账）
// ---------------------------------------------------------------------------

/// 重扫对账条目（差分单条——桥键 + 人话）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescanEntry {
    /// 诊断码（新桥/消失桥/漂移闭集三选一）。
    pub code: BwCode,
    /// 桥 bdf 键。
    pub bridge_key: u32,
    /// 人话详情。
    pub detail: String,
}

/// 重扫对账报告（三分账——新桥/消失桥/窗口漂移分列，不混记）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescanReport {
    /// 新桥（本轮在、上轮无）。
    pub new_bridges: Vec<RescanEntry>,
    /// 消失桥（上轮在、本轮无）。
    pub lost_bridges: Vec<RescanEntry>,
    /// 窗口漂移（同桥 bdf、桥头域变值）。
    pub drifted: Vec<RescanEntry>,
}

impl RescanReport {
    /// 对账干净（三分账全空）。
    pub fn clean(&self) -> bool {
        self.new_bridges.is_empty() && self.lost_bridges.is_empty() && self.drifted.is_empty()
    }

    /// 差分总数。
    pub fn total(&self) -> usize {
        self.new_bridges.len() + self.lost_bridges.len() + self.drifted.len()
    }    /// 人话多行渲染。
    pub fn render(&self) -> String {
        fn dump(out: &mut String, tag: &str, list: &[RescanEntry]) {
            for e in list {
                out.push_str(&format!(
                    "[{:04X}] {} {}：{}\n",
                    e.code.code(),
                    tag,
                    BwBdf::from_key(e.bridge_key).render(),
                    e.detail
                ));
            }
        }
        let mut s = String::new();
        if self.clean() {
            s.push_str("重扫对账：干净（零差分）\n");
            return s;
        }
        dump(&mut s, "新桥", &self.new_bridges);
        dump(&mut s, "消失桥", &self.lost_bridges);
        dump(&mut s, "窗口漂移", &self.drifted);
        s
    }
}

/// 增量重扫对账（纯函数——两轮桥账按 bdf 键两指针归并）。
///
/// 前提：双方已按 bdf 键升序（扫描器终局排序保证）。同一桥键
/// 桥头域变值即窗口漂移（secondary/subordinate/三窗任一动）。
pub fn reconcile_rescan(prev: &[BridgeRecord], cur: &[BridgeRecord]) -> RescanReport {
    let mut report = RescanReport {
        new_bridges: Vec::new(),
        lost_bridges: Vec::new(),
        drifted: Vec::new(),
    };
    let mut i = 0usize;
    let mut j = 0usize;
    while i < prev.len() && j < cur.len() {
        let pk = prev[i].bdf.key();
        let ck = cur[j].bdf.key();
        if pk == ck {
            if prev[i].header != cur[j].header {
                report.drifted.push(RescanEntry {
                    code: BwCode::RESCAN_WINDOW_DRIFT,
                    bridge_key: ck,
                    detail: format!(
                        "上轮 {} / 本轮 {}",
                        prev[i].header.render(),
                        cur[j].header.render()
                    ),
                });
            }
            i += 1;
            j += 1;
        } else if pk < ck {
            report.lost_bridges.push(RescanEntry {
                code: BwCode::RESCAN_LOST_BRIDGE,
                bridge_key: pk,
                detail: format!("上轮 {} 后本轮消失", prev[i].header.render()),
            });
            i += 1;
        } else {
            report.new_bridges.push(RescanEntry {
                code: BwCode::RESCAN_NEW_BRIDGE,
                bridge_key: ck,
                detail: format!("本轮新增 {}", cur[j].header.render()),
            });
            j += 1;
        }
    }
    while i < prev.len() {
        let p = &prev[i];
        report.lost_bridges.push(RescanEntry {
            code: BwCode::RESCAN_LOST_BRIDGE,
            bridge_key: p.bdf.key(),
            detail: format!("上轮 {} 后本轮消失", p.header.render()),
        });
        i += 1;
    }
    while j < cur.len() {
        let c = &cur[j];
        report.new_bridges.push(RescanEntry {
            code: BwCode::RESCAN_NEW_BRIDGE,
            bridge_key: c.bdf.key(),
            detail: format!("本轮新增 {}", c.header.render()),
        });
        j += 1;
    }
    report
}

impl BridgeWalker {
    /// 以本扫描器桥账为「本轮」、外部账为「上轮」做对账。
    pub fn reconcile_against_previous(&self, prev: &[BridgeRecord]) -> RescanReport {
        reconcile_rescan(prev, &self.bridges)
    }
}

// ---------------------------------------------------------------------------
// 十四、全链速造口（默认拓扑一把跑——判据全链单调用可复现）
// ---------------------------------------------------------------------------

/// 以标准演示拓扑全链跑一遍（造口 + begin + run 单调用闭环）。
///
/// 返回（扫描器，报告，端口周期数）三元组——账本在扫描器内，
/// 报告为快照，周期数为确定性审计面。
pub fn run_standard() -> Result<(BridgeWalker, WalkReport, u64), BwCode> {
    let mut port = DeterministicBridgePort::standard();
    let mut walker = BridgeWalker::new();
    walker.begin()?;
    let report = walker.run(&mut port)?;
    let cycles = port.cycles();
    Ok((walker, report, cycles))
}
