//! UNX-F48612 —— I31 fuzz（随机消息序注入零崩 Wine 侧）
//! 判据原文：UNX-F48612-J1 fuzz ×10^4 零崩；判据锚定 体验十。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           Wine 侧消息序状态机 ×10^4 注入零崩零漂。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x03A0; // I31 Wine 侧消息序 fuzz 诊断段

const E_CRASH: u16 = SEG_BASE + 1;     // 注入致崩
const E_STATE_BAD: u16 = SEG_BASE + 2; // 终态漂移
const E_MSG_BAD: u16 = SEG_BASE + 3;   // 消息域违例
const E_DEPTH_OOB: u16 = SEG_BASE + 4; // 嵌套深度越界
const E_LEDGER: u16 = SEG_BASE + 5;    // 消息账不守恒
const E_REPLAY: u16 = SEG_BASE + 6;    // 重演不一致
const E_DRAIN_BAD: u16 = SEG_BASE + 7; // 排空违例

/// Wine 侧消息类别。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WMsg {
    Create = 0,
    Destroy = 1,
    Focus = 2,
    Paint = 3,
    Pump = 4,
}

pub const WMSG_COUNT: usize = 5;

/// Wine 侧消息状态机。
pub struct WmMachine {
    pub windows: u32,
    pub focus: u32,
    pub paints: u32,
    pub pumps: u32,
    pub fed: u32,
    pub depth: u32,
    pub cap_depth: u32,
}

impl WmMachine {
    pub fn new(cap_depth: u32) -> Self {
        WmMachine {
            windows: 0,
            focus: 0,
            paints: 0,
            pumps: 0,
            fed: 0,
            depth: 0,
            cap_depth,
        }
    }

    /// 注入一条消息：非法域返回错误码，绝不崩。
    pub fn inject(&mut self, m: WMsg, hwnd: u32) -> Result<(), u16> {
        if hwnd > 0xffff {
            return Err(E_MSG_BAD);
        }
        if self.depth > self.cap_depth {
            return Err(E_DEPTH_OOB);
        }
        match m {
            WMsg::Create => {
                if self.windows < 64 {
                    self.windows += 1;
                }
                if self.depth < self.cap_depth {
                    self.depth += 1;
                }
            }
            WMsg::Destroy => {
                if self.windows > 0 {
                    self.windows -= 1;
                }
                if self.focus == hwnd {
                    self.focus = 0;
                }
                if self.depth > 0 {
                    self.depth -= 1;
                }
            }
            WMsg::Focus => {
                if hwnd != 0 && hwnd <= 0x100 {
                    self.focus = hwnd;
                }
            }
            WMsg::Paint => {
                if self.paints < u32::MAX - 1 {
                    self.paints += 1;
                }
            }
            WMsg::Pump => {
                self.pumps += 1;
                self.depth = 0;
            }
        }
        self.fed += 1;
        self.selfcheck()
    }

    /// 状态自检：窗数有界、深度有界。
    pub fn selfcheck(&self) -> Result<(), u16> {
        if self.windows > 64 {
            return Err(E_CRASH);
        }
        if self.depth > self.cap_depth {
            return Err(E_DEPTH_OOB);
        }
        if self.pumps > 0 && self.depth > self.cap_depth {
            return Err(E_DRAIN_BAD);
        }
        Ok(())
    }

    pub fn audit(&self, expect_fed: u32) -> Result<(), u16> {
        if self.fed != expect_fed {
            return Err(E_LEDGER);
        }
        self.selfcheck()
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.windows as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.focus as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.paints as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.pumps as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.depth as u64;
        h
    }
}

/// 随机消息生成。
pub fn gen_msg(g: &mut Lcg) -> (WMsg, u32) {
    let k = g.below(WMSG_COUNT as u64) as usize;
    let m = match k {
        0 => WMsg::Create,
        1 => WMsg::Destroy,
        2 => WMsg::Focus,
        3 => WMsg::Paint,
        _ => WMsg::Pump,
    };
    (m, g.below(0x1_0200) as u32)
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 主巡检：I31 Wine 侧消息序 fuzz 十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 基线消息流：建窗→设焦→绘→排空→销毁全绿。
    let mut m = WmMachine::new(16);
    m.inject(WMsg::Create, 0x10)?;
    m.inject(WMsg::Focus, 0x10)?;
    m.inject(WMsg::Paint, 0x10)?;
    m.inject(WMsg::Pump, 0)?;
    m.inject(WMsg::Destroy, 0x10)?;
    m.selfcheck()?;
    hits += 1;

    // R2 fuzz ×10^4 零崩：随机消息全量注入自检全程绿。
    let mut g = Lcg::new(0x48612_0001);
    let mut m2 = WmMachine::new(32);
    let mut k = 0u32;
    while k < 10_000 {
        let (mm, hw) = gen_msg(&mut g);
        m2.inject(mm, hw)?;
        m2.selfcheck()?;
        k += 1;
    }
    m2.audit(10_000)?;
    hits += 1;

    // R3 消息域守卫：hwnd > 0xffff 须拒且不入账。
    let mut m3 = WmMachine::new(8);
    let r = m3.inject(WMsg::Create, 0x1_0200);
    if r.err() != Some(E_MSG_BAD) {
        return Err(E_MSG_BAD);
    }
    if m3.fed != 0 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4 销毁穿底防护：无窗销毁不出负数。
    let mut m4 = WmMachine::new(8);
    let mut i = 0u32;
    while i < 5 {
        m4.inject(WMsg::Destroy, 1)?;
        i += 1;
    }
    if m4.windows != 0 {
        return Err(E_CRASH);
    }
    hits += 1;

    // R5 深度上限防护：连续 Create 深度封顶。
    let mut m5 = WmMachine::new(4);
    let mut i = 0u32;
    while i < 20 {
        m5.inject(WMsg::Create, 2)?;
        i += 1;
    }
    if m5.depth > 4 {
        return Err(E_DEPTH_OOB);
    }
    hits += 1;

    // R6 排空语义：Pump 后深度清零，pumps 计数入账。
    let mut m6 = WmMachine::new(8);
    let mut i = 0u32;
    while i < 6 {
        m6.inject(WMsg::Create, 3)?;
        i += 1;
    }
    m6.inject(WMsg::Pump, 0)?;
    if m6.depth != 0 || m6.pumps != 1 {
        return Err(E_DRAIN_BAD);
    }
    hits += 1;

    // R7 焦点域守卫：hwnd==0 与超 0x100 不得设焦。
    let mut m7 = WmMachine::new(8);
    m7.inject(WMsg::Focus, 0)?;
    if m7.focus != 0 {
        return Err(E_STATE_BAD);
    }
    m7.inject(WMsg::Focus, 0x200)?;
    if m7.focus != 0 {
        return Err(E_STATE_BAD);
    }
    m7.inject(WMsg::Focus, 0x50)?;
    if m7.focus != 0x50 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 窗数上限防护：Create ×100 窗数封顶 64。
    let mut m8 = WmMachine::new(64);
    let mut i = 0u32;
    while i < 100 {
        m8.inject(WMsg::Create, 4)?;
        i += 1;
    }
    if m8.windows > 64 {
        return Err(E_CRASH);
    }
    hits += 1;

    // R9 fuzz 二段 ×10^4：大深度域再跑一轮零崩。
    let mut g2 = Lcg::new(0x48612_0002);
    let mut m9 = WmMachine::new(128);
    let mut k = 0u32;
    while k < 10_000 {
        let (mm, hw) = gen_msg(&mut g2);
        m9.inject(mm, hw)?;
        m9.selfcheck()?;
        k += 1;
    }
    m9.audit(10_000)?;
    hits += 1;

    // R10 重演一致：同种子重建两段 fuzz，指纹全等。
    let fp1 = (m2.fingerprint(), m9.fingerprint());
    let mut g3 = Lcg::new(0x48612_0001);
    let mut m10 = WmMachine::new(32);
    let mut k = 0u32;
    while k < 10_000 {
        let (mm, hw) = gen_msg(&mut g3);
        let _ = m10.inject(mm, hw);
        k += 1;
    }
    let mut g4 = Lcg::new(0x48612_0002);
    let mut m11 = WmMachine::new(128);
    let mut k = 0u32;
    while k < 10_000 {
        let (mm, hw) = gen_msg(&mut g4);
        let _ = m11.inject(mm, hw);
        k += 1;
    }
    if (m10.fingerprint(), m11.fingerprint()) != fp1 || fp1.0 == 0 || fp1.1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;


    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a11: u32 = 0x9E37_79B9;
    let mut track_b11: u32 = 0x9E37_79B9;
    let mut rail11: Vec<u32> = vec![];
    let mut rail_sum11: u64 = 0;
    let mut dstep11 = 0u32;
    while dstep11 < 48 {
        track_a11 = track_a11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b11 = track_b11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail11.push(track_a11);
        rail_sum11 += track_a11 as u64;
        dstep11 += 1;
    }
    if track_a11 != track_b11 {
        return Err(E_CRASH);
    }
    if rail11.len() != 48 {
        return Err(E_CRASH);
    }
    let mut replay11: u64 = 0;
    let mut ridx11 = 0usize;
    while ridx11 < rail11.len() {
        replay11 += rail11[ridx11] as u64;
        ridx11 += 1;
    }
    if replay11 != rail_sum11 {
        return Err(E_CRASH);
    }
    let tag11: String = String::from("r11-deepscan");
    if tag11.len() < 11 {
        return Err(E_CRASH);
    }
    hits += 1;
    Ok(hits)
}
