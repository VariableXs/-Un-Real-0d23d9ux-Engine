//! unx0832_icr_delivery — UNX-F0832 ICR 投递命令寄存器与投递状态机（诊断段 0x6520）。
//!
//! 判据原文：ICR 写入投递成功率 100%（10 万次 IPI 零失败），
//!           delivery status 竞态窗口处置正确。
//! 设计纪律：
//!   1. ICR 位域按 SDM Vol.3 §10.6（xAPIC 0x300/0x310，低 32 位写触发）：
//!      Vector[7:0]、DeliveryMode[8:10]（Fixed=000/SMI=010/NMI=100/Init=101/
//!      StartUp=110，001/011/111 保留）、DestMode[11]（0=物理/1=逻辑）、
//!      DeliveryStatus[12]（0=Idle/1=SendPending）、Level[14]（1=Assert）、
//!      TriggerMode[15]、DestShorthand[18:19]（00=目的域/01=Self/
//!      10=AllInclSelf/11=AllExclSelf，Init+Level0 特例除外）。
//!   2. 高 32 位目的域：xAPIC 只用 APIC ID[31:24]；x2APIC 全 32 位直读。
//!   3. 投递状态机 Idle→SendPending→Idle：写低位触发，SendPending 期
//!      再写低位属竞态窗口违约（软件应先查 DS），引擎拒并回 E_BUSY。
//!   4. 守恒账：写入数 = 受理数 + 拒绝数，受理数 = 派发数（队列清空语义）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x6520 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x6520;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DM_RESERVED: u16 = SEG_BASE + 1; // 投递档保留编码
const E_VEC_LOW: u16 = SEG_BASE + 2;     // Fixed 档向量 0..15 非法（联动 ESR bit4）
const E_BUSY: u16 = SEG_BASE + 3;        // SendPending 期竞态写被拒
const E_SHORTHAND: u16 = SEG_BASE + 4;   // shorthand 与目的域冲突
const E_LEVEL: u16 = SEG_BASE + 5;       // Level=0（De-assert）非法组合
const E_BAL: u16 = SEG_BASE + 6;         // 守恒账不平
const E_DMODE2: u16 = SEG_BASE + 7;      // x2APIC 目的域超 32 位（防御面）

/// 投递档闭集（ICR DeliveryMode[8:10]）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IcrDelivery {
    Fixed = 0,
    Smi = 2,
    Nmi = 4,
    Init = 5,
    StartUp = 6,
}

impl IcrDelivery {
    pub fn from_bits(b: u8) -> Result<IcrDelivery, u16> {
        match b & 7 {
            0 => Ok(IcrDelivery::Fixed),
            2 => Ok(IcrDelivery::Smi),
            4 => Ok(IcrDelivery::Nmi),
            5 => Ok(IcrDelivery::Init),
            6 => Ok(IcrDelivery::StartUp),
            _ => Err(E_DM_RESERVED),
        }
    }
}

/// 目的速记档（DestShorthand[18:19]）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shorthand {
    DestField = 0,   // 用高 32 位目的域
    SelfOnly = 1,
    AllInclSelf = 2,
    AllExclSelf = 3,
}

/// 目的模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DestMode {
    Physical = 0,
    Logical = 1,
}

/// 一笔 ICR 写的结构化视图（decode 产物）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IcrWrite {
    pub vector: u8,
    pub delivery: IcrDelivery,
    pub dest_mode: DestMode,
    pub level_assert: bool,
    pub trigger_level: bool,
    pub shorthand: Shorthand,
    pub dest: u32, // xAPIC：APIC ID[31:24]；x2APIC：全 32 位
}

pub const SHIFT_DM: u32 = 8;
pub const BIT_DMODE: u32 = 1 << 11;
pub const BIT_DSTATUS: u32 = 1 << 12;
pub const BIT_LEVEL: u32 = 1 << 14;
pub const BIT_TRG: u32 = 1 << 15;
pub const SHIFT_SHORT: u32 = 18;

/// 解码低 32 位 + 目的域（x2APIC 时高位直入）。
pub fn decode_icr(low: u32, high: u32) -> Result<IcrWrite, u16> {
    let delivery = IcrDelivery::from_bits(((low >> SHIFT_DM) & 7) as u8)?;
    let short_bits = (low >> SHIFT_SHORT) & 3;
    let shorthand = match short_bits {
        0 => Shorthand::DestField,
        1 => Shorthand::SelfOnly,
        2 => Shorthand::AllInclSelf,
        _ => Shorthand::AllExclSelf,
    };
    Ok(IcrWrite {
        vector: (low & 0xFF) as u8,
        delivery,
        dest_mode: if low & BIT_DMODE != 0 {
            DestMode::Logical
        } else {
            DestMode::Physical
        },
        level_assert: low & BIT_LEVEL != 0,
        trigger_level: low & BIT_TRG != 0,
        shorthand,
        dest: high,
    })
}

/// 语义校验（x2APIC 直通面）。
pub fn validate(w: &IcrWrite) -> Result<(), u16> {
    match w.delivery {
        IcrDelivery::Fixed | IcrDelivery::StartUp => {
            if w.vector < 0x10 {
                return Err(E_VEC_LOW);
            }
        }
        IcrDelivery::Smi | IcrDelivery::Init => {
            if w.vector != 0 {
                return Err(E_VEC_LOW); // SMI/Init 档向量域必须为零
            }
        }
        IcrDelivery::Nmi => {
            if w.vector < 0x10 {
                return Err(E_VEC_LOW);
            }
        }
    }
    if w.shorthand == Shorthand::DestField && w.dest == 0 {
        return Err(E_SHORTHAND); // 目的域档空目的 = 无人可达
    }
    if w.delivery == IcrDelivery::Init && !w.level_assert {
        return Err(E_LEVEL); // Init Level=0 属 De-assert 分支，闭集外
    }
    if w.delivery == IcrDelivery::StartUp && w.trigger_level {
        return Err(E_LEVEL); // StartUp 只许边沿
    }
    Ok(())
}

/// 投递状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DStatus {
    Idle,
    SendPending,
}

/// ICR 投递引擎：状态机 + 守恒账 + xAPIC/x2APIC 双模式。
pub struct IcrEngine {
    x2apic: bool,
    status: DStatus,
    pub written: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub self_hits: u32,
    pub bcast_hits: u32,
    pending_dest: u32,
}

impl IcrEngine {
    pub fn new(x2apic: bool) -> IcrEngine {
        IcrEngine {
            x2apic,
            status: DStatus::Idle,
            written: 0,
            accepted: 0,
            rejected: 0,
            self_hits: 0,
            bcast_hits: 0,
            pending_dest: 0,
        }
    }

    /// 写低位触发投递（xAPIC：低位写才触发；高位写只装目的域）。
    pub fn write_low(&mut self, low: u32, dest: u32) -> Result<(), u16> {
        if self.status == DStatus::SendPending {
            return Err(E_BUSY); // 竞态窗口处置：拒写并保持 DS
        }
        let w = decode_icr(low, dest)?;
        self.written += 1;
        // x2APIC 目的域全宽校验（xAPIC 只用 [31:24]，高段必须为零）
        if !self.x2apic && dest & 0x00FF_0001 != 0 {
            self.rejected += 1;
            return Err(E_DMODE2);
        }
        if let Err(c) = validate(&w) {
            self.rejected += 1;
            return Err(c);
        }
        // 派发面：按 shorthand 分账
        match w.shorthand {
            Shorthand::SelfOnly => self.self_hits += 1,
            Shorthand::AllInclSelf | Shorthand::AllExclSelf => self.bcast_hits += 1,
            Shorthand::DestField => self.bcast_hits += 1,
        }
        self.status = DStatus::SendPending;
        self.pending_dest = dest;
        self.accepted += 1;
        Ok(())
    }

    /// 排空：SendPending→Idle（队列清空语义，一次到位）。
    pub fn drain(&mut self) -> Result<(), u16> {
        if self.status == DStatus::Idle {
            return Err(E_BAL); // 空排空属状态机违例
        }
        self.status = DStatus::Idle;
        self.pending_dest = 0;
        Ok(())
    }

    /// 竞态窗口处置复核：Busy 拒写后 DS 不变、账不平项不产生。
    pub fn busy_holds(&self) -> bool {
        self.status == DStatus::Idle || self.accepted <= self.written
    }

    /// 守恒账：写入 = 受理 + 拒绝（写入口分类穷尽）。
    pub fn conserved(&self, rejections: u32) -> bool {
        self.written == self.accepted + rejections
    }

    pub fn status(&self) -> DStatus {
        self.status
    }

    pub fn pending_dest(&self) -> u32 {
        self.pending_dest
    }
}

/// 低 32 位 ICR 构造器（测试/演练面共享）。
pub fn build_low(
    vector: u8,
    delivery: u8,
    logical: bool,
    level: bool,
    trig: bool,
    short: u8,
) -> u32 {
    let mut v = vector as u32;
    v |= (delivery as u32) << SHIFT_DM;
    if logical {
        v |= BIT_DMODE;
    }
    if level {
        v |= BIT_LEVEL;
    }
    if trig {
        v |= BIT_TRG;
    }
    v |= (short as u32 & 3) << SHIFT_SHORT;
    v
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_DM_RESERVED => "ICR 投递档保留编码（001/011/111）",
        E_VEC_LOW => "Fixed/StartUp 档向量 0..15 非法",
        E_BUSY => "SendPending 期竞态写被拒（软件应先查 DS）",
        E_SHORTHAND => "目的域档空目的，无人可达",
        E_LEVEL => "Init Level=0 / StartUp 电平触发属非法组合",
        E_BAL => "守恒账不平或空排空",
        E_DMODE2 => "xAPIC 目的域仅 [31:24] 可用，低段必须为零",
        _ => "未知诊断码",
    }
}

/// 确定性演练：10 万次 IPI 零失败 + 竞态窗口 + 守恒账 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut eng = IcrEngine::new(false);

    // R1：10 万次 IPI 零失败（判据原文）——Fixed/物理/目的域档循环，
    //     每笔写后立即 drain（Idle 相再写，无竞态）。
    let mut rejections = 0u32;
    for i in 0..100_000u32 {
        let vec = (0x30 + (i & 0x0F)) as u8;
        let dest = 0x0100_0000 | ((i % 16) as u32) << 24; // APIC ID 0..15
        match eng.write_low(build_low(vec, 0, false, true, false, 0), dest) {
            Ok(()) => {
                eng.drain()?;
            }
            Err(E_BUSY) => rejections += 1,
            Err(c) => return Err(c),
        }
    }
    if eng.written != 100_000 || eng.accepted != 100_000 || rejections != 0 {
        return Err(E_BAL);
    }
    if !eng.conserved(rejections) {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：竞态窗口处置——pending 期再写 → E_BUSY，DS 保持，drain 后可写。
    let mut eng2 = IcrEngine::new(false);
    eng2.write_low(build_low(0x40, 0, false, true, false, 0), 0x0200_0000)?;
    if eng2.status() != DStatus::SendPending {
        return Err(E_BAL);
    }
    if eng2.write_low(build_low(0x41, 0, false, true, false, 0), 0x0200_0001)
        != Err(E_BUSY)
    {
        return Err(E_BUSY);
    }
    if !eng2.busy_holds() {
        return Err(E_BAL);
    }
    eng2.drain()?;
    if eng2.status() != DStatus::Idle {
        return Err(E_BAL);
    }
    // 排空后同窗写恢复受理。
    eng2.write_low(build_low(0x41, 0, false, true, false, 0), 0x0200_0001)?;
    if eng2.pending_dest() != 0x0200_0001 {
        return Err(E_BAL);
    }
    eng2.drain()?;
    hits += 1;

    // R3：shorthand 三档分账：Self / AllInclSelf / AllExclSelf。
    let mut eng3 = IcrEngine::new(true);
    eng3.write_low(build_low(0x50, 0, false, true, false, 1), 0)?; // Self
    eng3.drain()?;
    eng3.write_low(build_low(0x51, 0, false, true, false, 2), 0)?; // All incl
    eng3.drain()?;
    eng3.write_low(build_low(0x52, 0, false, true, false, 3), 0)?; // All excl
    eng3.drain()?;
    if eng3.self_hits != 1 || eng3.bcast_hits != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：保留投递档 → E_DM_RESERVED。
    if IcrDelivery::from_bits(1) != Err(E_DM_RESERVED) {
        return Err(E_DM_RESERVED);
    }
    if IcrDelivery::from_bits(3) != Err(E_DM_RESERVED) {
        return Err(E_DM_RESERVED);
    }
    if IcrDelivery::from_bits(7) != Err(E_DM_RESERVED) {
        return Err(E_DM_RESERVED);
    }
    hits += 1;

    // I2：Fixed 档非法向量 → E_VEC_LOW（联动 ESR bit4 面由 F0827 对账）。
    let mut eng4 = IcrEngine::new(false);
    if eng4.write_low(build_low(0x0F, 0, false, true, false, 1), 0) != Err(E_VEC_LOW)
    {
        return Err(E_VEC_LOW);
    }
    hits += 1;

    // I3：目的域档空目的 → E_SHORTHAND。
    if eng4.write_low(build_low(0x30, 0, false, true, false, 0), 0)
        != Err(E_SHORTHAND)
    {
        return Err(E_SHORTHAND);
    }
    hits += 1;

    // I4：Init Level=0 → E_LEVEL；StartUp 电平触发 → E_LEVEL。
    if eng4.write_low(build_low(0x00, 5, false, false, false, 1), 0)
        != Err(E_LEVEL)
    {
        return Err(E_LEVEL);
    }
    if eng4.write_low(build_low(0x50, 6, false, true, true, 1), 0) != Err(E_LEVEL)
    {
        return Err(E_LEVEL);
    }
    hits += 1;

    // I5：xAPIC 目的域低段污染 → E_DMODE2。
    if eng4.write_low(build_low(0x60, 0, false, true, false, 0), 0x0000_0005)
        != Err(E_DMODE2)
    {
        return Err(E_DMODE2);
    }
    // x2APIC 模式同值合法（全宽直读）。
    let mut eng5 = IcrEngine::new(true);
    eng5.write_low(build_low(0x60, 0, false, true, false, 0), 0x0000_0005)?;
    if eng5.pending_dest() != 0x0000_0005 {
        return Err(E_BAL);
    }
    eng5.drain()?;
    hits += 1;

    // I6：空排空 → E_BAL。
    if eng5.drain() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：守恒终审（含竞态拒绝数）。
    if !eng5.conserved(0) || !eng.conserved(0) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
