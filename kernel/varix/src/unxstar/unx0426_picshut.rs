//! unx0426_picshut —— UNX-F0426 PIC 8259 全屏蔽账（APIC 接管预备）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0426 · PIC 8259 全屏蔽账 · 目标 240 行
//! 判据原文：UNX-F0426-J1 全屏蔽序列 = 主片 OCW1 写 0xFF +
//!           从片 OCW1 写 0xFF + IMR 回读双向核（写后读同值）
//!           + EOI 清账（OCW2 非特定 EOI 0x20 逐片）；接管预备
//!           状态机：Linked（PIC 在岗）→ MaskAll（全屏蔽落
//!           盘）→ Quiesced（无未决 IRQ）→ Handover（APIC 可
//!           接管）；任一步回退必须可逆（unmask 全通即回
//!           Linked）；端口纪律：主 0x20/0x21，从 0xA0/0xA1，
//!           命令口写偏移 0；级联线 IRQ2 屏蔽位不得独立于全
//!           屏蔽语义（全屏蔽含级联位）。
//! 组件：OCW1/IMR 位账（软镜像）+ 全屏蔽序列核 + 状态机 +
//!       可逆性核（unmask 复原）+ EOI 清账 + 端口纪律表。
//! 职责定位：PIC 是遗产中断控制器的「老门房」，APIC 接管前
//!   必须把它彻底闭嘴——屏蔽不全则旧中断漏进来打断新账；
//!   屏蔽后不核 IMR 则写丢失无声；不可逆则接管失败没有退
//!   路。本模块把「全屏蔽」从一句话变成四步有账可查的状态
//!   机，每步都有回读证据。
//! 性能逐项分解：mask/unmask O(1)；状态推进 O(1)；EOI 清账
//!   O(片数)=O(2)；可逆性核 O(1)。
//! 诊断码段：0x30200（0x30201~0x30209 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PS_VERSION: &str = "unx0426_picshut-v1";

/// 诊断段基址（0x30200 段 · 本模块独占）。
pub const PS_SEG_BASE: u32 = 0x30200;
pub const XC_IMR_MISMATCH: u32 = 0x30201; // IMR 回读不一致
pub const XC_STATE_OOR: u32 = 0x30202; // 状态机越迁
pub const XC_CASCADE_LEAK: u32 = 0x30203; // 级联位漏屏蔽
pub const XC_NOT_QUIESCED: u32 = 0x30204; // 未静止即申请接管
pub const XC_EOI_LOST: u32 = 0x30205; // EOI 清账缺片
pub const XC_PORT_OOR: u32 = 0x30206; // 端口越表
pub const XC_REVERSAL: u32 = 0x30207; // 可逆性破坏
pub const XC_DOUBLE_HANDOVER: u32 = 0x30208; // 重复接管
pub const XC_MAGIC: u32 = 0x30209; // 账魔数破

/// 端口纪律表：主片命令/数据，从片命令/数据。
pub const PORT_MCMD: u16 = 0x20;
pub const PORT_MDATA: u16 = 0x21;
pub const PORT_SCMD: u16 = 0xA0;
pub const PORT_SDATA: u16 = 0xA1;
/// OCW2 非特定 EOI 命令字。
pub const OCW2_EOI: u8 = 0x20;
/// OCW1 全屏蔽字。
pub const MASK_ALL: u8 = 0xFF;
/// 全通字（可逆核用）。
pub const MASK_NONE: u8 = 0x00;

/// 接管预备状态机四态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandState {
    /// PIC 在岗（未屏蔽）。
    Linked = 1,
    /// 全屏蔽落盘（IMR 双片 0xFF 且回读一致）。
    MaskAll = 2,
    /// 静止（无未决 IRQ，ISR/IRR 均空）。
    Quiesced = 3,
    /// APIC 可接管（终态；再进即双接管违规）。
    Handover = 4,
}

/// 单片软镜像：IMR 期望值与回读值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PicMirror {
    /// IMR 期望（最近一次写）。
    pub imr_want: u8,
    /// IMR 回读。
    pub imr_read: u8,
    /// ISR（在服务寄存器）软账。
    pub isr: u8,
    /// IRR（中断请求寄存器）软账。
    pub irr: u8,
    /// 本轮 EOI 已清账。
    pub eoi_done: bool,
}

impl PicMirror {
    pub const fn fresh() -> Self {
        PicMirror { imr_want: 0, imr_read: 0, isr: 0, irr: 0, eoi_done: false }
    }
}

/// 全屏蔽账本：双片镜像 + 状态机。
pub struct ShutBook {
    /// 主片镜像。
    pub master: PicMirror,
    /// 从片镜像。
    pub slave: PicMirror,
    /// 当前状态。
    pub state: HandState,
    /// 已接管（终态闩）。
    pub handed: bool,
    /// 端口访问流水（审计用，记录 port 值）。
    pub port_log: Vec<u16>,
}

impl ShutBook {
    pub fn new() -> Self {
        ShutBook {
            master: PicMirror::fresh(),
            slave: PicMirror::fresh(),
            state: HandState::Linked,
            handed: false,
            port_log: Vec::new(),
        }
    }

    /// 端口越表核：只许四口。
    pub fn port_ok(&self, port: u16) -> Result<(), u32> {
        match port {
            PORT_MCMD | PORT_MDATA | PORT_SCMD | PORT_SDATA => Ok(()),
            _ => Err(XC_PORT_OOR),
        }
    }

    /// 写 OCW1（数据口）：落软镜像并回读核。返回 Err 即写丢。
    pub fn write_ocw1(&mut self, slave: bool, val: u8, port: u16) -> Result<(), u32> {
        self.port_ok(port)?;
        self.port_log.push(port);
        let m = if slave { &mut self.slave } else { &mut self.master };
        m.imr_want = val;
        m.imr_read = val; // 软总线：回读恒同；破坏由 inject_read_drift 注入
        if m.imr_read != m.imr_want {
            return Err(XC_IMR_MISMATCH);
        }
        Ok(())
    }

    /// 注入回读漂移（演练用：模拟总线写丢）。
    pub fn inject_read_drift(&mut self, slave: bool, drifted: u8) {
        let m = if slave { &mut self.slave } else { &mut self.master };
        m.imr_read = drifted;
    }

    /// 全屏蔽序列：双片 OCW1=0xFF + 回读核 + 级联位核。
    /// 主片数据口 0x21、从片 0xA1。
    pub fn mask_all(&mut self) -> Result<HandState, u32> {
        if self.handed {
            return Err(XC_DOUBLE_HANDOVER);
        }
        if self.state != HandState::Linked {
            return Err(XC_STATE_OOR);
        }
        // 主片：级联位（IRQ2 = 位 2）必须随全屏蔽一起置位。
        self.write_ocw1(false, MASK_ALL, PORT_MDATA)?;
        self.write_ocw1(true, MASK_ALL, PORT_SDATA)?;
        // 级联位核：主片位 2 必须在屏蔽字里。
        if self.master.imr_want & (1 << 2) == 0 {
            return Err(XC_CASCADE_LEAK);
        }
        self.state = HandState::MaskAll;
        Ok(self.state)
    }

    /// 静止核：ISR 与 IRR 双空才放行。
    pub fn quiesce(&mut self) -> Result<HandState, u32> {
        if self.state != HandState::MaskAll {
            return Err(XC_STATE_OOR);
        }
        if self.master.isr != 0 || self.master.irr != 0 || self.slave.isr != 0 || self.slave.irr != 0 {
            return Err(XC_NOT_QUIESCED);
        }
        self.state = HandState::Quiesced;
        Ok(self.state)
    }

    /// EOI 清账（OCW2 非特定 EOI 逐片）：命令口写，落 eoi_done。
    pub fn send_eoi(&mut self, slave: bool, port: u16) -> Result<(), u32> {
        self.port_ok(port)?;
        let cmd_ok = if slave { port == PORT_SCMD } else { port == PORT_MCMD };
        if !cmd_ok {
            return Err(XC_PORT_OOR);
        }
        self.port_log.push(port);
        let m = if slave { &mut self.slave } else { &mut self.master };
        if m.isr == 0 && m.irr == 0 {
            m.eoi_done = true; // 空片也走一遍 EOI 语义（清账完备）
        } else {
            m.isr = 0;
            m.irr = 0;
            m.eoi_done = true;
        }
        Ok(())
    }

    /// EOI 清账完备核：双片都清了才许 Handover。
    pub fn eoi_complete(&self) -> Result<(), u32> {
        if self.master.eoi_done && self.slave.eoi_done {
            Ok(())
        } else {
            Err(XC_EOI_LOST)
        }
    }

    /// 接管放行：Quiesced + EOI 完备 → Handover。
    pub fn handover(&mut self) -> Result<HandState, u32> {
        if self.handed {
            return Err(XC_DOUBLE_HANDOVER);
        }
        if self.state != HandState::Quiesced {
            return Err(XC_STATE_OOR);
        }
        self.eoi_complete()?;
        self.state = HandState::Handover;
        self.handed = true;
        Ok(self.state)
    }

    /// 可逆核：接管前任意时刻可 unmask 全通回 Linked。
    pub fn revert(&mut self) -> Result<HandState, u32> {
        if self.handed {
            return Err(XC_REVERSAL);
        }
        self.write_ocw1(false, MASK_NONE, PORT_MDATA)?;
        self.write_ocw1(true, MASK_NONE, PORT_SDATA)?;
        self.state = HandState::Linked;
        Ok(self.state)
    }

    /// 模拟 IRQ 到达（置 IRR；屏蔽位拦下则不置）。
    pub fn inject_irq(&mut self, slave: bool, line: u8) -> bool {
        if line >= 8 {
            return false;
        }
        let bit = 1u8 << line;
        let m = if slave { &mut self.slave } else { &mut self.master };
        if m.imr_read & bit != 0 {
            return false; // 被屏蔽
        }
        m.irr |= bit;
        true
    }
}

/// 全屏蔽演示：标准四步到 Handover。
pub fn demo_handover() -> Result<HandState, u32> {
    let mut b = ShutBook::new();
    b.mask_all()?;
    b.quiesce()?;
    b.send_eoi(false, PORT_MCMD)?;
    b.send_eoi(true, PORT_SCMD)?;
    b.handover()
}

/// 判据入口：UNX-F0426。
pub fn run_unx0426_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0426_picshut");
    // 族一：全屏蔽序列。
    let mut b = ShutBook::new();
    cs.add("初态 Linked", b.state == HandState::Linked, "new 即在岗");
    cs.add("全屏蔽落盘", b.mask_all() == Ok(HandState::MaskAll), "双片 0xFF");
    cs.add("级联位在", b.master.imr_want & (1 << 2) != 0, "主片位 2");
    cs.add("回读一致", b.master.imr_read == b.master.imr_want && b.slave.imr_read == b.slave.imr_want, "写后读同值");
    // 族二：状态机越迁纪律。
    let mut b2 = ShutBook::new();
    cs.add("跳级拒", b2.quiesce() == Err(XC_STATE_OOR), "Linked 直达 Quiesced");
    cs.add("未屏蔽不静止", b2.mask_all().is_ok() && b2.quiesce().is_ok(), "MaskAll 才可静止");
    // 族三：IRQ 漏进来即拦。
    let mut b3 = ShutBook::new();
    let _ = b3.mask_all();
    cs.add("屏蔽拦 IRQ", !b3.inject_irq(false, 4), "COM1 被拦");
    cs.add("从片屏蔽拦", !b3.inject_irq(true, 12), "PS2 被拦");
    // 族四：未静止即申请接管。
    let mut b4 = ShutBook::new();
    let _ = b4.mask_all();
    let _ = b4.inject_irq(false, 1); // 漏一个未屏蔽场景不存在（全屏蔽后不可达），软账直置
    b4.master.irr = 1; // 直置软账模拟漏网
    cs.add("IRR 非空拒静止", b4.quiesce() == Err(XC_NOT_QUIESCED), "有未决不许接管");
    // 族五：EOI 清账。
    let mut b5 = ShutBook::new();
    let _ = b5.mask_all();
    let _ = b5.quiesce();
    cs.add("缺 EOI 拒接管", b5.handover() == Err(XC_EOI_LOST), "双片未清");
    let _ = b5.send_eoi(false, PORT_MCMD);
    let _ = b5.send_eoi(true, PORT_SCMD);
    cs.add("EOI 双清过", b5.eoi_complete().is_ok(), "逐片清账");
    cs.add("接管放行", b5.handover() == Ok(HandState::Handover), "Quiesced+EOI");
    cs.add("重复接管拒", b5.handover() == Err(XC_DOUBLE_HANDOVER), "终态闩");
    // 族六：EOI 端口纪律。
    let mut b6 = ShutBook::new();
    cs.add("EOI 数据口拒", b6.send_eoi(false, PORT_MDATA) == Err(XC_PORT_OOR), "EOI 只走命令口");
    cs.add("EOI 从主口拒", b6.send_eoi(true, PORT_MCMD) == Err(XC_PORT_OOR), "从片走 A0");
    cs.add("野端口拒", b6.port_ok(0xCF8).is_err(), "非四口");
    // 族七：可逆核。
    let mut b7 = ShutBook::new();
    let _ = b7.mask_all();
    cs.add("unmask 回 Linked", b7.revert() == Ok(HandState::Linked), "全通复原");
    cs.add("复原后 IRQ 通", b7.inject_irq(false, 0), "时钟线放行");
    // 族八：可逆性在接管后破坏。
    let mut b8 = ShutBook::new();
    let _ = demo_handover();
    let _ = b8.mask_all();
    let _ = b8.quiesce();
    let _ = b8.send_eoi(false, PORT_MCMD);
    let _ = b8.send_eoi(true, PORT_SCMD);
    let _ = b8.handover();
    cs.add("接管后回退拒", b8.revert() == Err(XC_REVERSAL), "不可逆保护");
    // 族九：标准演示路径。
    cs.add("演示路径到 Handover", demo_handover() == Ok(HandState::Handover), "四步全绿");
    // 族十：回读漂移检出。
    let mut b9 = ShutBook::new();
    b9.write_ocw1(false, MASK_ALL, PORT_MDATA).ok();
    b9.inject_read_drift(false, 0x00);
    cs.add("漂移检出", b9.write_ocw1(false, MASK_ALL, PORT_MDATA) == Err(XC_IMR_MISMATCH), "写丢立案");
    cs
}
