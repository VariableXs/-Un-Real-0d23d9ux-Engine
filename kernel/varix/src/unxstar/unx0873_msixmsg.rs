//! unx0873_msixmsg —— UNX-F0873 · MSI-X 消息地址/数据编码器。
//! 锚点（CoRun Unxreal 主册）：UNX-F0873 · MSI-X 消息地址/数据编码器。
//! 判据原文摘录（卡面验收行）：MSI-X 消息地址/数据编码器
//!           ——地址/数据位域按 SDM 编码、向量越界与模式非法全拦截、
//!           编码账逐笔留痕且守恒。
//! 诊断段 0x78A0（本单独占，0x78A1~0x78AF 闭集）。
//!
//! 设计纪律：
//! - 消息地址（SDM Vol.3 §10.11）：基址 0xFEE0_0000；[19:12] 目标 APIC ID，
//!   [3] 重定向提示 RH，[2] 目标模式 DM（0=物理 1=逻辑）。
//!   高 32 位（64 位地址面）为 0（x2APIC 面不收）。
//! - 消息数据：[7:0] 向量，[10:8] 投递模式（0=Fixed 1=Lowest 2=SMI 4=NMI
//!   5=INIT 6=Startup），[14] 电平（Fixed 面），[15] 触发（0=边沿 1=电平）。
//! - 拦截面：向量 <16 或 >255（16~255 才是可用向量带）、地址基址错位、
//!   目标 ID > 255、保留投递模式（3/7）、电平位与 Fixed 模式联动非法
//!   （NMI/INIT/SMI 面触发位必须 0）。
//! - 状态机：Idle → DestSet → ModeSet → DataDone → Committed（提交态不可
//!   复写，重写须先 Reset）。
//! - 编码账：每笔提交一行（定容 96，溢出计数），守恒：提交数 = 在账 + 溢出。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 模块版本。
pub const XM_VERSION: &str = "unx0873_msixmsg-v1";

/// 诊断段基址（0x78A0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x78A0;

/// XcCode 闭集（0x78A1~0x78AF）。
pub const XC_VEC_RANGE: u16 = SEG_BASE + 1; // 向量越界（<16 或 >255）
pub const XC_ADDR_BASE: u16 = SEG_BASE + 2; // 地址基址错位（非 0xFEE0_0000 面）
pub const XC_DEST_RANGE: u16 = SEG_BASE + 3; // 目标 APIC ID 越界（>255）
pub const XC_MODE_RSVD: u16 = SEG_BASE + 4; // 保留投递模式（3/7）
pub const XC_TRIG_ILLEGAL: u16 = SEG_BASE + 5; // 触发位与模式联动非法
pub const XC_STATE: u16 = SEG_BASE + 6; // 状态机违序（未授权的写入序）
pub const XC_REWRITE: u16 = SEG_BASE + 7; // 提交态复写（未 Reset）
pub const XC_LED_FULL: u16 = SEG_BASE + 8; // 账溢出（计数不丢）
pub const XC_CONSERVE: u16 = SEG_BASE + 9; // 守恒破

/// 消息地址基址。
pub const ADDR_BASE: u32 = 0xFEE0_0000;
/// 可用向量下界（0~15 为异常/保留带）。
pub const VEC_LO: u32 = 16;
/// 可用向量上界（含）。
pub const VEC_HI: u32 = 255;
/// 账定容。
pub const LED_CAP: usize = 96;

/// 投递模式闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeliveryMode {
    /// Fixed（0）。
    Fixed,
    /// Lowest Priority（1）。
    Lowest,
    /// SMI（2）。
    Smi,
    /// NMI（4）。
    Nmi,
    /// INIT（5）。
    Init,
    /// Startup（6）。
    Startup,
}

impl DeliveryMode {
    /// 三位编码。
    pub fn bits(&self) -> u32 {
        match self {
            DeliveryMode::Fixed => 0,
            DeliveryMode::Lowest => 1,
            DeliveryMode::Smi => 2,
            DeliveryMode::Nmi => 4,
            DeliveryMode::Init => 5,
            DeliveryMode::Startup => 6,
        }
    }
    /// 三位原值 → 闭集（3/7 即保留）。
    pub fn from_bits(b: u32) -> Result<DeliveryMode, u16> {
        match b {
            0 => Ok(DeliveryMode::Fixed),
            1 => Ok(DeliveryMode::Lowest),
            2 => Ok(DeliveryMode::Smi),
            4 => Ok(DeliveryMode::Nmi),
            5 => Ok(DeliveryMode::Init),
            6 => Ok(DeliveryMode::Startup),
            _ => Err(XC_MODE_RSVD),
        }
    }
    /// 触发位是否允许置位（仅 Fixed/Lowest 电平面允许）。
    pub fn level_trigger_ok(&self) -> bool {
        matches!(self, DeliveryMode::Fixed | DeliveryMode::Lowest)
    }
}

/// 编码请求行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MsgReq {
    /// 目标 APIC ID（0~255）。
    pub dest: u8,
    /// 重定向提示 RH。
    pub rh: bool,
    /// 目标模式 DM：false=物理 true=逻辑。
    pub logical: bool,
    /// 向量（16~255）。
    pub vector: u8,
    /// 投递模式。
    pub mode: DeliveryMode,
    /// 触发：false=边沿 true=电平。
    pub level: bool,
}

/// 编码产物：(地址, 数据)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MsgEnc {
    /// 消息地址（低 32 位面）。
    pub addr: u32,
    /// 消息数据。
    pub data: u32,
}

/// 账行：(请求摘要, 产物, 序号)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EncRow {
    /// 向量。
    pub vector: u8,
    /// 目标。
    pub dest: u8,
    /// 模式位。
    pub mode_bits: u32,
    /// 产物地址。
    pub addr: u32,
    /// 产物数据。
    pub data: u32,
    /// 序号。
    pub seq: u64,
}

/// 编码器状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncState {
    /// 空闲。
    Idle,
    /// 目标已设。
    DestSet,
    /// 模式已设。
    ModeSet,
    /// 数据已就绪。
    DataDone,
    /// 已提交。
    Committed,
}

/// MSI-X 消息编码器。
pub struct MsgEncoder {
    state: EncState,
    /// 暂存目标（DestSet 后生效）。
    pend_dest: Option<(u8, bool, bool)>, // (dest, rh, logical)
    /// 暂存模式。
    pend_mode: Option<DeliveryMode>,
    /// 暂存向量/触发。
    pend_vec: Option<(u8, bool)>,
    /// 提交账。
    rows: Vec<EncRow>,
    overflow: usize,
    seq: u64,
    /// 拦截计数。
    rejects: u64,
}

impl MsgEncoder {
    pub fn new() -> MsgEncoder {
        MsgEncoder {
            state: EncState::Idle,
            pend_dest: None,
            pend_mode: None,
            pend_vec: None,
            rows: Vec::new(),
            overflow: 0,
            seq: 0,
            rejects: 0,
        }
    }

    fn reject(&mut self, _code: u16) {
        self.rejects += 1;
    }

    /// 步一：设目标（Idle/DestSet 面合法）。
    pub fn set_dest(&mut self, dest: u8, rh: bool, logical: bool) -> Result<(), u16> {
        if self.state == EncState::Committed {
            self.reject(XC_REWRITE);
            return Err(XC_REWRITE);
        }
        if self.state != EncState::Idle && self.state != EncState::DestSet {
            self.reject(XC_STATE);
            return Err(XC_STATE);
        }
        self.state = EncState::DestSet;
        self.pend_dest = Some((dest, rh, logical));
        Ok(())
    }

    /// 步二：设投递模式。
    pub fn set_mode(&mut self, mode: DeliveryMode) -> Result<(), u16> {
        if self.state != EncState::DestSet {
            self.reject(XC_STATE);
            return Err(XC_STATE);
        }
        self.state = EncState::ModeSet;
        self.pend_mode = Some(mode);
        Ok(())
    }

    /// 步三：设向量与触发 → 数据就绪。
    pub fn set_vector(&mut self, vector: u8, level: bool) -> Result<(), u16> {
        if self.state != EncState::ModeSet {
            self.reject(XC_STATE);
            return Err(XC_STATE);
        }
        let v = vector as u32;
        if v < VEC_LO || v > VEC_HI {
            self.reject(XC_VEC_RANGE);
            return Err(XC_VEC_RANGE);
        }
        self.state = EncState::DataDone;
        self.pend_vec = Some((vector, level));
        Ok(())
    }

    /// 步四：提交编码（触发位联动 + 地址基址校验 + 落账）。
    pub fn commit(&mut self) -> Result<MsgEnc, u16> {
        if self.state != EncState::DataDone {
            self.reject(XC_STATE);
            return Err(XC_STATE);
        }
        let (dest, rh, logical) = match self.pend_dest {
            Some(t) => t,
            None => {
                self.reject(XC_STATE);
                return Err(XC_STATE);
            }
        };
        let mode = match self.pend_mode {
            Some(m) => m,
            None => {
                self.reject(XC_STATE);
                return Err(XC_STATE);
            }
        };
        let (vector, level) = match self.pend_vec {
            Some(t) => t,
            None => {
                self.reject(XC_STATE);
                return Err(XC_STATE);
            }
        };
        // 触发位联动：非 Fixed/Lowest 面电平触发非法
        if level && !mode.level_trigger_ok() {
            self.reject(XC_TRIG_ILLEGAL);
            return Err(XC_TRIG_ILLEGAL);
        }
        // 地址组装：基址 | dest<<12 | rh<<3 | dm<<2
        let addr = ADDR_BASE
            | ((dest as u32) << 12)
            | ((rh as u32) << 3)
            | ((logical as u32) << 2);
        if addr & 0xFFF0_0F00 != ADDR_BASE & 0xFFF0_0F00 {
            self.reject(XC_ADDR_BASE);
            return Err(XC_ADDR_BASE);
        }
        // 数据组装：vector | mode<<8 | level<<14 | trigger<<15
        let data = (vector as u32)
            | (mode.bits() << 8)
            | ((level as u32) << 15)
            | ((level as u32) << 14);
        let enc = MsgEnc { addr, data };
        self.seq += 1;
        if self.rows.len() < LED_CAP {
            self.rows.push(EncRow {
                vector,
                dest,
                mode_bits: mode.bits(),
                addr,
                data,
                seq: self.seq,
            });
        } else {
            self.overflow += 1;
        }
        self.state = EncState::Committed;
        Ok(enc)
    }

    /// 复位（提交态回 Idle，重新起笔）。
    pub fn reset(&mut self) {
        self.state = EncState::Idle;
        self.pend_dest = None;
        self.pend_mode = None;
        self.pend_vec = None;
    }

    /// 当前状态。
    pub fn state(&self) -> EncState {
        self.state
    }

    /// 账行读取。
    pub fn row(&self, i: usize) -> Result<EncRow, u16> {
        if i >= self.rows.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.rows[i])
    }

    /// 提交总数。
    pub fn committed(&self) -> u64 {
        self.rows.len() as u64 + self.overflow as u64
    }

    /// 拦截总数。
    pub fn rejects(&self) -> u64 {
        self.rejects
    }

    /// 守恒：提交数（在账+溢出）= 提交序号；拦截独立计数不混账。
    pub fn conserve(&self) -> bool {
        self.rows.len() as u64 + self.overflow as u64 == self.seq
            && self.committed() >= self.rows.len() as u64
    }
}

/// 便捷一次成型：单请求直接编码（内部分步走状态机）。
pub fn encode_once(req: MsgReq) -> Result<MsgEnc, u16> {
    let mut e = MsgEncoder::new();
    e.set_dest(req.dest, req.rh, req.logical)?;
    e.set_mode(req.mode)?;
    e.set_vector(req.vector, req.level)?;
    e.commit()
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 正常编码：Fixed 边沿，向量 0x40，目标 2 物理
    let enc = encode_once(MsgReq {
        dest: 2,
        rh: false,
        logical: false,
        vector: 0x40,
        mode: DeliveryMode::Fixed,
        level: false,
    })?;
    if enc.addr != 0xFEE0_2000 || enc.data != 0x40 {
        return Err(XC_ADDR_BASE);
    }
    // 2. 逻辑面 + 电平触发：dest=5, Lowest, level
    let enc2 = encode_once(MsgReq {
        dest: 5,
        rh: true,
        logical: true,
        vector: 0x51,
        mode: DeliveryMode::Lowest,
        level: true,
    })?;
    if enc2.addr != 0xFEE0_500C || enc2.data != 0xC151 {
        return Err(XC_ADDR_BASE);
    }
    // 3. NMI 面电平触发拒（触发位联动）
    if encode_once(MsgReq {
        dest: 1,
        rh: false,
        logical: false,
        vector: 0x60,
        mode: DeliveryMode::Nmi,
        level: true,
    })
    .is_ok()
    {
        return Err(XC_TRIG_ILLEGAL);
    }
    // 4. 向量越界拒：15 与 0（下界）
    if encode_once(MsgReq {
        dest: 1,
        rh: false,
        logical: false,
        vector: 15,
        mode: DeliveryMode::Fixed,
        level: false,
    })
    .is_ok()
    {
        return Err(XC_VEC_RANGE);
    }
    // 5. 状态机违序：跳步（Idle 直接 commit）
    let mut e = MsgEncoder::new();
    if e.commit().is_ok() {
        return Err(XC_STATE);
    }
    // 6. 提交态复写拒 + Reset 复位后可再编
    e.set_dest(1, false, false)?;
    e.set_mode(DeliveryMode::Fixed)?;
    e.set_vector(0x40, false)?;
    e.commit()?;
    if e.state() != EncState::Committed {
        return Err(XC_STATE);
    }
    if e.set_dest(2, false, false).is_ok() {
        return Err(XC_REWRITE);
    }
    e.reset();
    if e.state() != EncState::Idle {
        return Err(XC_CONSERVE);
    }
    e.set_dest(3, false, true)?;
    e.set_mode(DeliveryMode::Nmi)?;
    e.set_vector(0x61, false)?;
    let enc3 = e.commit()?;
    if enc3.data != (0x61 | (4 << 8)) {
        return Err(XC_CONSERVE);
    }
    // 7. 账守恒：本编码器两笔提交，第二笔（NMI 向量 0x61）在账可查
    if e.row(1)?.vector != 0x61 || e.committed() != 2 || !e.conserve() {
        return Err(XC_CONSERVE);
    }
    Ok(0)
}
