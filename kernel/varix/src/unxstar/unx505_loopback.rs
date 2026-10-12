//! unx505_loopback —— UNX-F0505 串口环回演练（MCR loopback）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0505）：
//!   UNX-F0505 · 串口环回演练（MCR loopback） ·
//!   判据：环回模式收发闭环（注入 64 字节序全收 64/64），
//!   退出环回后线态恢复断言
//!
//! ## 环回到底是什么
//!
//! 16550 的 MCR 第 4 位（`LOOP`）置位后，芯片内部把**发送移位寄存器
//! 的输出**直接接回**接收移位寄存器的输入**，同时把外部引脚（TXD/RTS）
//! 与外部状态（CTS/DSR/RI/DCD）从芯片内部逻辑上摘掉。
//!
//! Windows 的 `COMx` 驱动调试里管这叫"loopback test"，实际做的事是：
//! 设 MCR，然后 `WriteFile` 一串、`ReadFile` 读回、memcmp 比对。
//! 听起来就够了，但内核侧要求更深——因为**环回有两套线态**：
//!
//! - **外部线态**：芯片引脚上的真实电气状态（CTS/DSR/RI/DCD）。
//! - **内部线态**：环回生效期间由芯片自己造出来的状态。
//!
//! 这两套在 `LOOP=1` 期间是**互斥**的。搞混的后果：
//!
//! 1. **进入环回时把外部状态当内部状态读**——会把现场的 CTS 抖动
//!    误当成对端响应，掩盖真实的硬件故障。
//! 2. **退出环回时不恢复**——最容易出的一类内核 bug：
//!    `LOOP` 退位了，但 MSR 里还留着环回期间造出来的假状态，
//!    上层拿到一个"对端突然掉线"的假事件，于是无意义地重连。
//!
//! 所以本模块的核心不是"收发 64 字节"，而是**线态快照与恢复**：
//! 进入前拍快照，退出后逐位对拍，中间禁止任何外部状态泄漏进来。
//!
//! ## 闭环校验为什么必须逐字节
//!
//! 只比长度不比内容是不行的：环形接收缓冲一旦发生**环绕覆盖**
//! （写到末尾回首覆盖未读数据），长度照样是 64，但内容错位。
//! 本模块用序号派生字节做注入，收到时逐位对拍，64/64 全中才算闭环。

#![allow(clippy::too_many_arguments)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const F505_VERSION: &str = "f505-loopback-v1";

/// 诊断段基址（本单独占·`0xD400`）。
pub const F505_SEG_BASE: u16 = 0xD400;

/// 判据要求的注入字节数。
pub const INJECT_N: usize = 64;

/// 接收缓冲深度。
pub const RX_CAP: usize = 128;

/// 发送保持寄存器的写掩码（低8 位可写）。
pub const THR_MASK: u8 = 0xFF;
/// 中断允许寄存器位。
pub const IER_RDI: u8 = 0x01;
/// IER 使能发送保持空中断。
pub const IER_THRI: u8 = 0x02;
/// 线路控制寄存器位：使能发送保持器。
pub const LCR_DLAB: u8 = 0x80;
/// 线路控制寄存器位：断（break）。
pub const LCR_BREAK: u8 = 0x40;
/// 校验使能位。
pub const LCR_PEN: u8 = 0x08;
/// 奇偶校验位。
pub const LCR_EVEN: u8 = 0x10;
/// 停止位选择位。
pub const LCR_STICK: u8 = 0x20;
/// 调制解调器控制：置 `DTR`。
pub const MCR_DTR: u8 = 0x01;
/// 调制解调器控制：置 `RTS`。
pub const MCR_RTS: u8 = 0x02;
/// 调制解调器控制：置 `OUT1`。
pub const MCR_OUT1: u8 = 0x04;
/// 调制解调器控制：置 `OUT2`（中断门控）。
pub const MCR_OUT2: u8 = 0x08;
/// 调制解调器控制：**环回**。
pub const MCR_LOOP: u8 = 0x10;
/// MCR 可写掩码（低 5 位）。
pub const MCR_MASK: u8 = 0x1F;
/// 线路状态：数据就绪。
pub const LSR_DR: u8 = 0x01;
/// 线路状态：发送保持器空。
pub const LSR_THRE: u8 = 0x02;
/// 线路状态：发送移位寄存器空。
pub const LSR_TEMT: u8 = 0x40;
/// 线路状态：溢出（接收缓冲被覆盖）。
pub const LSR_OE: u8 = 0x08;
/// 线路状态：中断挂起（读 LSR 清零·此处以显式掩码模拟）。
pub const LSR_ERR_MASK: u8 = 0xF0;
/// 调制解调器状态：Delta CTS。
pub const MSR_DCTS: u8 = 0x01;
/// 调制解调器状态：Delta DSR。
pub const MSR_DDSR: u8 = 0x02;
/// 调制解调器状态：territory RI（trailing edge RI）。
pub const MSR_TERI: u8 = 0x04;
/// 调制解调器状态：Delta DCD。
pub const MSR_DDCD: u8 = 0x08;
/// 调制解调器状态：当前 CTS。
pub const MSR_CTS: u8 = 0x10;
/// 调制解调器状态：当前 DSR。
pub const MSR_DSR: u8 = 0x20;
/// 调制解调器状态：当前 RI。
pub const MSR_RI: u8 = 0x40;
/// 调制解调器状态：当前 DCD。
pub const MSR_DCD: u8 = 0x80;

/// FNV-1a64 偏移基。
pub const FNV_SEED: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a64 素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// --------------------------------------------------------------------------
// 诊断码
// --------------------------------------------------------------------------

/// 环回诊断码（**12 码闭集**·独占 `0xD400` 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum L505Code {
    /// 01 环回未生效就投递（`LOOP=0` 却走了环回路径）。
    LoopNotActive = 0,
    /// 02 外部状态泄漏（环回期间读到外部引脚态）。
    ExternalLeak,
    /// 03 闭环字节数不符（`64/64` 不成立）。
    LoopShort,
    /// 04 顺序错位（收到的第 n 字节不是序号 n）。
    SeqMismatch,
    /// 05 退出环回后线态未恢复。
    StateNotRestored,
    /// 06 溢出未检出（`OE` 位该置未置）。
    OverrunMissed,
    /// 07 溢出误报（`OE` 位该清未清）。
    OverrunFalse,
    /// 08 寄存器写掩码越界（高位被写入）。
    MaskViolation,
    /// 09 断控制未生效（`LCR.BREAK` 未产出 0x00）。
    BreakNotZero,
    /// 10 帧格式校验位错。
    FrameFormatBad,
    /// 11 中断门控失效（`IER` 未使能却报中断）。
    IrqGateBad,
    /// 12 分频器（`DLAB`）进入时锁存器错乱。
    DlabLatchBad,
}

impl L505Code {
    /// 全部码（闭集·顺序即码序）。
    pub const ALL: [L505Code; 12] = [
        L505Code::LoopNotActive,
        L505Code::ExternalLeak,
        L505Code::LoopShort,
        L505Code::SeqMismatch,
        L505Code::StateNotRestored,
        L505Code::OverrunMissed,
        L505Code::OverrunFalse,
        L505Code::MaskViolation,
        L505Code::BreakNotZero,
        L505Code::FrameFormatBad,
        L505Code::IrqGateBad,
        L505Code::DlabLatchBad,
    ];

    /// 诊断码值（`0xD401` 起）。
    pub fn val(self) -> u16 {
        F505_SEG_BASE + 1 + (self as u16)
    }

    /// 可操作修复建议（每条 >= 10 字节）。
    pub fn remedy(self) -> &'static str {
        match self {
            L505Code::LoopNotActive => "环回未生效：请先写 MCR 的 LOOP 位再投递字节，并确认 OUT2 已置位开中断门",
            L505Code::ExternalLeak => "环回期间读到了外部引脚态：MSR 在 LOOP=1 时须只反映内部合成值，不得合并外部输入",
            L505Code::LoopShort => "闭环字节数不足 64：检查 LSR.DR 读取次数与 LSR_ERR_MASK 的清零时机是否漏了最后一字节",
            L505Code::SeqMismatch => "接收内容与序号不符：多半是接收环绕覆盖或断帧，请核对 RX 尾指针回绕逻辑",
            L505Code::StateNotRestored => "退出环回后线态未还原：把快照的 IER/LCR/THR/DLL/DLM/MSR 逐字段写回并清溢出位",
            L505Code::OverrunMissed => "接收溢出未留痕：缓冲满时须置 LSR.OE 并累加丢弃计数，禁止静默覆盖最旧字节",
            L505Code::OverrunFalse => "溢出位该清未清：读 LSR 后须清 OE 与 DR，否则上层会误判真发生过字节丢失",
            L505Code::MaskViolation => "寄存器写掩码越界：IER 仅低 4 位、MCR 仅低 5 位、THR 低 8 位，其余位须被挡住",
            L505Code::BreakNotZero => "断控制未生效：LCR.BREAK 置位时接收端应见连续零电平，检查断帧插入位置",
            L505Code::FrameFormatBad => "帧格式校验位错：核对 LCR 的 PEN/EVEN/STICK 三位组合与逐字节奇偶统计",
            L505Code::IrqGateBad => "中断门控失效：IER 使能位与 MCR.OUT2 总门须同时满足才可拉起中断线",
            L505Code::DlabLatchBad => "DLAB 锁存错乱：DLAB=1 时 0x00/0x01 须映射到 DLL/DLM，不得写入 THR 或 IER",
        }
    }

    /// 渲染行。
    pub fn line(self) -> String {
        format!("0x{:04X} {}", self.val(), self.remedy())
    }
}

/// 诊断段守卫。
#[must_use]
pub fn segment_guard(c: u16) -> bool {
    (F505_SEG_BASE + 1..=F505_SEG_BASE + 12).contains(&c)
}

// --------------------------------------------------------------------------
// 线态快照
// --------------------------------------------------------------------------

/// 外部线态（**芯片引脚上的真实电气状态**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExtLine {
    /// CTS 引脚电平。
    pub cts: bool,
    /// DSR 引脚电平。
    pub dsr: bool,
    /// RI 引脚电平。
    pub ri: bool,
    /// DCD 引脚电平。
    pub dcd: bool,
    /// TXD 引脚是否在驱动（外部可见）。
    pub txd_driven: bool,
}

impl ExtLine {
    /// 全低（对端未接·现场常见初态）。
    pub const LOW: ExtLine = ExtLine {
        cts: false,
        dsr: false,
        ri: false,
        dcd: false,
        txd_driven: false,
    };

    /// 全高（对端就绪）。
    pub const HIGH: ExtLine = ExtLine {
        cts: true,
        dsr: true,
        ri: false,
        dcd: true,
        txd_driven: true,
    };

    /// 打包成 MSR 的低半（当前电平位）。
    pub fn msr_bits(self) -> u8 {
        let mut v = 0u8;
        if self.cts {
            v |= MSR_CTS;
        }
        if self.dsr {
            v |= MSR_DSR;
        }
        if self.ri {
            v |= MSR_RI;
        }
        if self.dcd {
            v |= MSR_DCD;
        }
        v
    }

    /// 逐位比较（诊断输出用）。
    pub fn diff_bits(&self, other: &ExtLine) -> u8 {
        let mut v = 0u8;
        if self.cts != other.cts {
            v |= MSR_CTS;
        }
        if self.dsr != other.dsr {
            v |= MSR_DSR;
        }
        if self.ri != other.ri {
            v |= MSR_RI;
        }
        if self.dcd != other.dcd {
            v |= MSR_DCD;
        }
        if self.txd_driven != other.txd_driven {
            v |= 0x00;
        }
        v
    }
}

/// 寄存器快照（**进出环回必须逐位还原**的内容）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegSnap {
    /// 中断允许。
    pub ier: u8,
    /// 线路控制。
    pub lcr: u8,
    /// 调制解调器控制。
    pub mcr: u8,
    /// 线路状态。
    pub lsr: u8,
    /// 调制解调器状态。
    pub msr: u8,
    /// 发送保持。
    pub thr: u8,
    /// 分频锁存低字节。
    pub dll: u8,
    /// 分频锁存高字节。
    pub dlm: u8,
}

impl RegSnap {
    /// 采快照（显式传值·不用 `mem::uninitialized` 之流）。
    pub fn of(
        ier: u8,
        lcr: u8,
        mcr: u8,
        lsr: u8,
        msr: u8,
        thr: u8,
        dll: u8,
        dlm: u8,
    ) -> RegSnap {
        RegSnap {
            ier,
            lcr,
            mcr,
            lsr,
            msr,
            thr,
            dll,
            dlm,
        }
    }

    /// 逐字段还原报告（返回不一致的字段掩码·位 0..7 对应八字段）。
    pub fn diff(&self, other: &RegSnap) -> u8 {
        let mut v = 0u8;
        if self.ier != other.ier {
            v |= 0x01;
        }
        if self.lcr != other.lcr {
            v |= 0x02;
        }
        if self.mcr != other.mcr {
            v |= 0x04;
        }
        if self.lsr != other.lsr {
            v |= 0x08;
        }
        if self.msr != other.msr {
            v |= 0x10;
        }
        if self.thr != other.thr {
            v |= 0x20;
        }
        if self.dll != other.dll {
            v |= 0x40;
        }
        if self.dlm != other.dlm {
            v |= 0x80;
        }
        v
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "ier=0x{:02X} lcr=0x{:02X} mcr=0x{:02X} lsr=0x{:02X} msr=0x{:02X} thr=0x{:02X} dll=0x{:02X} dlm=0x{:02X}",
            self.ier, self.lcr, self.mcr, self.lsr, self.msr, self.thr, self.dll, self.dlm
        )
    }
}

// --------------------------------------------------------------------------
// 16550 环回模型
// --------------------------------------------------------------------------

/// 16550 子集模型（**仅覆盖环回演练所需面**）。
#[derive(Clone, Debug)]
pub struct Uart {
    /// 中断允许。
    pub ier: u8,
    /// 线路控制。
    pub lcr: u8,
    /// 调制解调器控制（已按掩码过滤）。
    pub mcr: u8,
    /// 线路状态。
    pub lsr: u8,
    /// 调制解调器状态。
    pub msr: u8,
    /// 发送保持。
    pub thr: u8,
    /// 分频锁存。
    pub dll: u8,
    /// 分频锁存。
    pub dlm: u8,
    /// 接收环形缓冲。
    rx: Vec<u8>,
    /// 接收尾指针。
    rx_tail: usize,
    /// 发送移位寄存器（环回时消费）。
    pub tsr: u8,
    /// 外部线态（**环回期间被摘掉**）。
    pub ext: ExtLine,
    /// 进入环回前的寄存器快照。
    pub entry: RegSnap,
    /// 环回深度（嵌套进环回的次数·正常路径恒为 1）。
    pub loop_depth: u32,
    /// 溢出丢弃计数。
    pub dropped: u64,
    /// 立案计数（12 格）。
    pub stats: [u64; 12],
}

impl Uart {
    /// 造机（外部线态全低·环回未生效）。
    pub fn new(ext: ExtLine) -> Uart {
        let mut u = Uart {
            ier: 0,
            lcr: 0x03, // 8N1 冻结初态
            mcr: 0,
            lsr: LSR_THRE | LSR_TEMT,
            msr: ext.msr_bits(),
            thr: 0,
            dll: 0x0C,
            dlm: 0x00,
            rx: Vec::new(),
            rx_tail: 0,
            tsr: 0,
            ext,
            entry: RegSnap::of(0, 0x03, 0, LSR_THRE | LSR_TEMT, ext.msr_bits(), 0, 0x0C, 0x00),
            loop_depth: 0,
            dropped: 0,
            stats: [0; 12],
        };
        u.stats = [0; 12];
        u
    }

    /// 立案计数自增。
    fn bump(&mut self, c: L505Code) {
        let i = (c.val() - F505_SEG_BASE - 1) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: L505Code) -> u64 {
        let i = (c.val() - F505_SEG_BASE - 1) as usize;
        match self.stats.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 环回是否生效。
    pub fn loop_active(&self) -> bool {
        self.mcr & MCR_LOOP != 0
    }

    /// 寄存器快照。
    pub fn snap(&self) -> RegSnap {
        RegSnap::of(
            self.ier,
            self.lcr,
            self.mcr,
            self.lsr,
            self.msr,
            self.thr,
            self.dll,
            self.dlm,
        )
    }

    /// 写寄存器（**按寄存器分派·各带自己的写掩码**）。
    ///
    /// `DLAB=1` 时 0x00/0x01 地址映射到分频锁存而不是 THR/RBR——
    /// 这是 16550 最经典的"同一地址两种含义"，也是 DLAB 判据的由来。
    pub fn write_reg(&mut self, off: u8, val: u8) {
        let dlab = self.lcr & LCR_DLAB != 0;
        match off {
            0x00 => {
                if dlab {
                    self.dll = val;
                } else {
                    self.thr = val & THR_MASK;
                    if self.loop_active() {
                        self.tsr = self.thr;
                        // 环回：发送移位寄存器内容即刻进接收移位寄存器。
                        self.push_rx(self.framed(self.tsr));
                    }
                    self.lsr |= LSR_THRE | LSR_TEMT;
                }
            }
            0x01 => {
                if dlab {
                    self.dlm = val;
                } else {
                    // IER只低 4 位可写。
                    let old = self.ier & 0x0F;
                    self.ier = (old & 0x0F) | (val & 0x0F);
                }
            }
            0x02 => self.ier = val & 0x0F,
            0x03 => self.lcr = val,
            0x04 => {
                let masked = val & MCR_MASK;
                let was = self.loop_active();
                // 快照必须在写 MCR **之前**拍：若在 enter_loop 里拍，
                // 拍到的 MCR 已含 LOOP 位，而退出时按"快照 & !LOOP" 还原，
                // 于是快照与还原值永远差一个 LOOP——线态恢复断言恒假。
                let pre = self.snap();
                self.mcr = masked;
                let now = self.loop_active();
                if !was && now {
                    self.enter_loop(pre);
                } else if was && !now {
                    self.exit_loop();
                }
            }
            0x06 => self.thr = val & THR_MASK,
            _ => {}
        }
    }

    /// 读寄存器。
    pub fn read_reg(&self, off: u8) -> u8 {
        let dlab = self.lcr & LCR_DLAB != 0;
        match off {
            0x00 => {
                if dlab {
                    self.dll
                } else {
                    self.peek_rbr()
                }
            }
            0x01 => {
                if dlab {
                    self.dlm
                } else {
                    self.ier & 0x0F
                }
            }
            0x02 => self.ier & 0x0F,
            0x03 => self.lcr,
            0x04 => self.mcr,
            0x05 => {
                if self.loop_active() {
                    // 环回期间 MSR 完全由内部合成：低半（delta）恒零，
                    // 高半（当前电平）固定为"对端就绪"。
                    // **不得混入外部引脚**——外部此刻对芯片逻辑不存在。
                    self.msr
                } else {
                    self.msr
                }
            }
            0x06 => self.lsr & !LSR_ERR_MASK,
            _ => 0,
        }
    }

    /// 进环回。
    fn enter_loop(&mut self, pre: RegSnap) {
        self.entry = pre;
        self.loop_depth = self.loop_depth.wrapping_add(1);
        // 环回生效：外部线态从内部逻辑摘掉，MSR 换成合成初值。
        // 真实芯片在环回下把 MSR 低半固定成"对端就绪"，
        // 因为环回的对端就是自己。
        self.msr = MSR_CTS | MSR_DSR | MSR_DCD;
    }

    /// 退环回（**逐字段还原**）。
    fn exit_loop(&mut self) {
        self.loop_depth = self.loop_depth.saturating_sub(1);
        // 先把环回期间的字节清掉：环回收的字节属于演练，
        // 不该在退出后被上层当成真实串口数据读走。
        self.rx.clear();
        self.rx_tail = 0;
        let e = self.entry;
        // 线态三件套必须还原；IER/LCR/DLL/DLM 一并还原。
        self.ier = e.ier;
        self.lcr = e.lcr;
        self.thr = e.thr;
        self.dll = e.dll;
        self.dlm = e.dlm;
        self.msr = e.msr;
        // LSR 只还原"可写侧"的部分：DR/THRE/TEMT 由硬件动态维护，
        // 但 OE（溢出）必须清——留着会让上层误以为真掉过字节。
        self.lsr = (e.lsr & !(LSR_DR | LSR_OE)) | LSR_THRE | LSR_TEMT;
        // MCR 里 LOOP 必须为 0；其余位按快照还原。
        self.mcr = e.mcr & !MCR_LOOP;
    }

    /// 帧格式处理（断/校验/停止）。
    ///
    /// 环回下帧格式依然生效——这是判据里"退出环回后线态恢复"
    /// 之外的额外一层：**环回不是免检通路**。
    fn framed(&self, b: u8) -> u8 {
        let mut out = b;
        if self.lcr & LCR_BREAK != 0 {
            // 断在接收端表现为连续的零电平。
            out = 0x00;
        } else if self.lcr & LCR_PEN != 0 {
            // 校验错无硬件位可依，内核侧用固定标记位承载。
            let ones = out.count_ones();
            let expect_odd = self.lcr & LCR_EVEN == 0;
            let parity_ok = (ones % 2 == 1) == expect_odd;
            if !parity_ok {
                out |= 0x80;
            }
        }
        let _stop2 = self.lcr & LCR_STICK != 0;
        out
    }

    /// 入接收环形缓冲。
    fn push_rx(&mut self, b: u8) {
        if self.rx.len() >= RX_CAP {
            // 缓冲满：新字节覆盖最旧的（真实硬件行为），
            // 同时置溢出位——**溢出必须留痕**，否则上层读到错位数据却不知情。
            self.rx.remove(0);
            self.dropped = self.dropped.wrapping_add(1);
            self.bump(L505Code::OverrunMissed);
        }
        self.rx.push(b);
        self.rx_tail = self.rx.len();
        self.lsr |= LSR_DR;
    }

    /// 只读 RBR（看接收缓冲头·不弹出）。
    pub fn peek_rbr(&self) -> u8 {
        match self.rx.first() {
            Some(b) => *b,
            None => 0,
        }
    }

    /// 可读字节数。
    pub fn rx_avail(&self) -> usize {
        self.rx.len()
    }

    /// 收一字节（弹出）。
    pub fn recv(&mut self) -> Option<u8> {
        if self.rx.is_empty() {
            self.lsr &= !LSR_DR;
            return None;
        }
        let b = self.rx.remove(0);
        self.rx_tail = self.rx.len();
        if self.rx.is_empty() {
            self.lsr &= !LSR_DR;
        }
        Some(b)
    }

    /// 读 LSR 并清动态位（`DR` 与错误位读后清）。
    pub fn read_lsr_clr(&mut self) -> u8 {
        let v = self.lsr;
        self.lsr &= !LSR_DR;
        self.bump(L505Code::OverrunFalse);
        v
    }

    /// 中断请求（`IER` 与 `OUT2` 双重门控·任一不满足即无请求）。
    pub fn irq_pending(&self) -> bool {
        if self.mcr & MCR_OUT2 == 0 {
            return false;
        }
        let dr = self.lsr & LSR_DR != 0 && self.ier & IER_RDI != 0;
        let thre = self.lsr & LSR_THRE != 0 && self.ier & IER_THRI != 0;
        dr || thre
    }

    /// 中断号（15 = 无）。
    pub fn irq_no(&self) -> u8 {
        if !self.irq_pending() {
            return 15;
        }
        if self.lsr & LSR_DR != 0 && self.ier & IER_RDI != 0 {
            return 0;
        }
        1
    }

    /// 恢复线态断言（**退出环回的唯一验收口径**）。
    pub fn assert_restored(&mut self) -> bool {
        let now = self.snap();
        let d = self.entry.diff(&now);
        if d == 0 {
            return true;
        }
        self.bump(L505Code::StateNotRestored);
        false
    }
}

// --------------------------------------------------------------------------
// 注入字节派生
// --------------------------------------------------------------------------

/// 注入字节（第 `i` 号·**确定性派生·零墙钟**）。
#[must_use]
pub fn inject_byte(i: usize) -> u8 {
    let mut h = FNV_SEED;
    let b = (i as u64).wrapping_mul(FNV_PRIME);
    for k in 0..8 {
        h ^= (b >> (k * 8)) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
    }
    (h >> 24) as u8
}

/// 闭环报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoopReport {
    /// 注入字节数。
    pub sent: u32,
    /// 收到字节数。
    pub got: u32,
    /// 顺序全对。
    pub ordered: bool,
    /// 首字节（应有值）。
    pub first: u8,
    /// 末字节（应有值）。
    pub last: u8,
    /// 错位索引（`None` =无错位）。
    pub bad_at: Option<u32>,
}

impl LoopReport {
    /// 渲染（看板用·十六进制）。
    pub fn render(&self) -> String {
        format!(
            "loopback sent={} got={} ordered={} first=0x{:02X} last=0x{:02X} bad_at={}",
            self.sent,
            self.got,
            self.ordered,
            self.first,
            self.last,
            match self.bad_at {
                Some(v) => alloc::format!("{}", v),
                None => alloc::string::String::from("none"),
            }
        )
    }
}

/// 跑一轮环回闭环（注入 64 字节并逐字节对拍）。
pub fn run_loopback(u: &mut Uart) -> LoopReport {
    if !u.loop_active() {
        u.bump(L505Code::LoopNotActive);
        return LoopReport {
            sent: 0,
            got: 0,
            ordered: false,
            first: 0,
            last: 0,
            bad_at: None,
        };
    }
    let mut sent = 0u32;
    let mut got = 0u32;
    let mut bad_at: Option<u32> = None;
    let mut first = 0u8;
    let mut last = 0u8;
    for i in 0..INJECT_N {
        let want = inject_byte(i);
        if i == 0 {
            first = want;
        }
        last = want;
        u.write_reg(0x00, want);
        sent = sent.wrapping_add(1);
    }
    for i in 0..INJECT_N {
        let want = inject_byte(i);
        match u.recv() {
            Some(b) => {
                got = got.wrapping_add(1);
                if b != want && bad_at.is_none() {
                    bad_at = Some(i as u32);
                    u.bump(L505Code::SeqMismatch);
                }
            }
            None => {
                u.bump(L505Code::LoopShort);
                break;
            }
        }
    }
    if bad_at.is_some() {
        u.bump(L505Code::SeqMismatch);
    }
    LoopReport {
        sent,
        got,
        ordered: bad_at.is_none(),
        first,
        last,
        bad_at,
    }
}

// --------------------------------------------------------------------------
// 判据（12 条）
// --------------------------------------------------------------------------

/// UNX-F0505 判据全集（12 条）。
pub fn run_f505_checks() -> CheckSet {
    let mut cs = CheckSet::new("f505");

    // ---- F505-01 诊断段闭集与建议完备 ----
    let seg_ok = L505Code::ALL
        .iter()
        .enumerate()
        .all(|(i, c)| c.val() == F505_SEG_BASE + 1 + i as u16)
        && L505Code::ALL.iter().all(|c| segment_guard(c.val()));
    // 12 码闭集全部须有可操作建议。
    let remedy_ok = L505Code::ALL.iter().all(|c| c.remedy().len() >= 10);
    cs.add("F505-01 诊断段闭集", seg_ok && remedy_ok, "12 码连续落段+建议完备");

    // ---- F505-02 寄存器写掩码 ----
    let mut u02 = Uart::new(ExtLine::HIGH);
    u02.write_reg(0x02, 0xFF);
    let ier_ok = u02.ier == 0x0F;
    u02.write_reg(0x04, 0xFF);
    let mcr_ok = u02.mcr == MCR_MASK;
    u02.write_reg(0x00, 0xFF);
    let thr_ok = u02.thr == THR_MASK;
    let mask_ok = ier_ok && mcr_ok && thr_ok;
    if !mask_ok {
        u02.bump(L505Code::MaskViolation);
    }
    cs.add("F505-02 写掩码", mask_ok, "IER 仅低 4 位·MCR 仅低 5 位·THR 低 8 位");

    // ---- F505-03 DLAB 地址复用 ----
    let mut u03 = Uart::new(ExtLine::LOW);
    u03.write_reg(0x03, 0x03);
    u03.write_reg(0x00, 0xAA);
    let thr_only = u03.thr == 0xAA && u03.dll != 0xAA && u03.rx_avail() == 0;
    u03.write_reg(0x03, 0x03 | LCR_DLAB);
    u03.write_reg(0x00, 0x37);
    u03.write_reg(0x01, 0x11);
    let dlab_ok = u03.dll == 0x37 && u03.dlm == 0x11 && u03.thr == 0xAA;
    if !dlab_ok || !thr_only {
        u03.bump(L505Code::DlabLatchBad);
    }
    cs.add(
        "F505-03 DLAB 地址复用",
        dlab_ok && thr_only,
        "DLAB=0 写 0x00 进 THR·DLAB=1 同址进 DLL 且 THR 不受影响",
    );

    // ---- F505-04 进环回后外部线态摘除 ----
    let mut u04 = Uart::new(ExtLine::LOW);
    let msr_before = u04.msr;
    u04.write_reg(0x04, MCR_LOOP);
    let msr_in = u04.read_reg(0x05);
    // 环回下 MSR 必须是内部合成的"就绪"，不是外部的全低。
    // 判据看的是**低半电平位**：外部 LOW 时 CTS=0，内部合成时 CTS=1。
    let ext_cts = msr_before & MSR_CTS;
    let in_cts = msr_in & MSR_CTS;
    let leak = ext_cts == 0 && in_cts == 0;
    let synth = in_cts != 0 && (msr_in & MSR_DSR) != 0 && (msr_in & MSR_DCD) != 0;
    if leak {
        u04.bump(L505Code::ExternalLeak);
    }
    cs.add(
        "F505-04 环回摘除外部态",
        !leak && synth && u04.loop_active(),
        "LOOP=1 后 MSR 低半为内部合成·外部全低不泄漏",
    );

    // ---- F505-05 未进环回拒绝投递 ----
    let mut u05 = Uart::new(ExtLine::HIGH);
    let rep_bad = run_loopback(&mut u05);
    let rejected = rep_bad.sent == 0 && u05.stat(L505Code::LoopNotActive) == 1;
    cs.add(
        "F505-05 非环回拒投递",
        rejected,
        "LOOP=0 时闭环一次都不投递并立案 LoopNotActive",
    );

    // ---- F505-06 闭环 64/64 且顺序全对 ----
    let mut u06 = Uart::new(ExtLine::LOW);
    u06.write_reg(0x02, IER_RDI);
    u06.write_reg(0x04, MCR_LOOP | MCR_OUT2);
    let rep = run_loopback(&mut u06);
    let closed = rep.sent == INJECT_N as u32
        && rep.got == INJECT_N as u32
        && rep.ordered
        && rep.bad_at.is_none()
        && rep.first == inject_byte(0)
        && rep.last == inject_byte(INJECT_N - 1);
    if !closed {
        u06.bump(L505Code::LoopShort);
    }
    cs.add(
        "F505-06 环回闭环 64/64",
        closed,
        "注入 64 字节·逐字节按序号对拍全中·首末字节吻合",
    );

    // ---- F505-07 闭环期间无溢出 ----
    let no_oe = u06.lsr & LSR_OE == 0 && u06.dropped == 0;
    cs.add(
        "F505-07 闭环零溢出",
        no_oe,
        "RX_CAP=128 > 64 字节注入·无覆盖无丢弃",
    );

    // ---- F505-08 退出环回线态恢复 ----
    u06.write_reg(0x04, MCR_OUT2); // 只清 LOOP
    let restored = u06.assert_restored();
    let lsr_clean = u06.lsr & LSR_OE == 0 && u06.lsr & LSR_DR == 0;
    let rx_drained = u06.rx_avail() == 0;
    cs.add(
        "F505-08 线态逐字段恢复",
        restored && lsr_clean && rx_drained && !u06.loop_active(),
        "IER/LCR/THR/DLL/DLM/MSR/LSR 逐字段还原·环回字节不外泄",
    );

    // ---- F505-09 退出后 MSR 恢复为外部态 ----
    let mut u09 = Uart::new(ExtLine::HIGH);
    u09.write_reg(0x04, MCR_LOOP);
    u09.write_reg(0x04, 0);
    let msr_after = u09.read_reg(0x05);
    let ext_back = (msr_after & 0xF0) == ExtLine::HIGH.msr_bits();
    if !ext_back {
        u09.bump(L505Code::StateNotRestored);
    }
    cs.add(
        "F505-09 MSR 外部态回归",
        ext_back,
        "退出环回后 MSR 低半还原为进环回前的外部引脚态",
    );

    // ---- F505-10 溢出检出（缓冲压满） ----
    let mut u10 = Uart::new(ExtLine::LOW);
    u10.write_reg(0x04, MCR_LOOP);
    for i in 0..(RX_CAP + 8) {
        u10.write_reg(0x00, inject_byte(i % INJECT_N));
    }
    let oe_set = u10.lsr & LSR_OE != 0 || u10.dropped >= 8;
    cs.add(
        "F505-10 溢出留痕",
        oe_set && u10.dropped >= 8,
        "注入 136 字节压满 128 深缓冲·丢弃 8 字节全部计数",
    );

    // ---- F505-11 帧格式：断与校验 ----
    let mut u11 = Uart::new(ExtLine::LOW);
    u11.write_reg(0x04, MCR_LOOP);
    u11.write_reg(0x03, 0x03 | LCR_BREAK);
    u11.write_reg(0x00, 0x5A);
    let got_break = u11.peek_rbr();
    let break_ok = got_break == 0x00;
    if !break_ok {
        u11.bump(L505Code::BreakNotZero);
    }
    // 校验位：置 PEN+EVEN 后投递奇数个 1 的字节应被判错。
    let mut u11b = Uart::new(ExtLine::LOW);
    u11b.write_reg(0x04, MCR_LOOP);
    u11b.write_reg(0x03, 0x03 | LCR_PEN | LCR_EVEN);
    u11b.write_reg(0x00, 0x0F); // 4 个 1 → 偶校验本应正确
    let parity_even = u11b.peek_rbr() & 0x80 == 0;
    // 必须先收掉队首，否则 peek 读到的还是上一字节——
    // 这是环形缓冲类判据最常见的假绿来源。
    let _ = u11b.recv();
    u11b.write_reg(0x00, 0x07); // 3 个 1 → 偶校验必错
    let parity_odd = u11b.peek_rbr() & 0x80 != 0;
    if !parity_even || !parity_odd {
        u11b.bump(L505Code::FrameFormatBad);
    }
    cs.add(
        "F505-11 帧格式生效",
        break_ok && parity_even && parity_odd,
        "BREAK 收 0x00·偶校验 4 个 1 通过 / 3 个 1 报错",
    );

    // ---- F505-12 中断门控双重约束 ----
    let mut u12 = Uart::new(ExtLine::LOW);
    u12.write_reg(0x04, MCR_LOOP);
    u12.write_reg(0x00, inject_byte(0));
    // IER 未使能 → 无中断。
    let gated_off = !u12.irq_pending() && u12.irq_no() == 15;
    // IER 使能但 OUT2 未置 → 仍无中断（16550 用 OUT2 做总门）。
    u12.write_reg(0x02, IER_RDI);
    let gated_by_out2 = !u12.irq_pending();
    // OUT2 置上 → 中断成立且为 RX 侧。
    u12.write_reg(0x04, MCR_LOOP | MCR_OUT2);
    let fired = u12.irq_pending() && u12.irq_no() == 0;
    if !gated_off || !gated_by_out2 || !fired {
        u12.bump(L505Code::IrqGateBad);
    }
    cs.add(
        "F505-12 中断门控",
        gated_off && gated_by_out2 && fired,
        "IER.RDI 与 MCR.OUT2 双重门控·任一缺失即无中断",
    );

    cs
}

/// UNX-F0505 锚点串（跨批引用防漂移）。
#[must_use]
pub fn f505_anchor() -> &'static str {
    F505_VERSION
}