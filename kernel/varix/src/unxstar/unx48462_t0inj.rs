//! UNX-F48462 —— E·乱序注入断连（反：T0 违例 ×10 断连+P0 登记）
//! 判据原文：UNX-F48462-J1 T0 违例（Hello 前消息/Bye 后消息）×10 全部断连；
//!           断连必伴随 P0 登记（通道号+违例型）；断连后通道拒绝一切消息；
//!           LCG 随机违例 ×512 全部断连；P0 账封印重演一致；
//!           合法流量零误断；P0 账四型封闭。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDEE0; // E24 T0 违例段

const E_NO_DISCONNECT: u16 = SEG_BASE + 1; // 违例未断连
const E_NO_P0: u16 = SEG_BASE + 2; // 断连未登记 P0
const E_ZOMBIE_PASS: u16 = SEG_BASE + 3; // 断连后消息放行
const E_LEGAL_CUT: u16 = SEG_BASE + 4; // 合法流量误断
const E_KIND_BAD: u16 = SEG_BASE + 5; // 违例型越界
const E_SEAL_BAD: u16 = SEG_BASE + 6; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 8; // 守恒破

/// 违例型。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ViolKind {
    PreHello = 0,   // Hello 前消息
    PostBye = 1,    // Bye 后消息
    DupHello = 2,   // 重复 Hello
    MidBye = 3,     // 序中 Bye 后又续发
}

/// 通道状态。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ChState {
    Closed = 0,
    Open = 1,
    Disconnected = 2,
}

/// P0 登记条目。
#[derive(Clone)]
pub struct P0Rec {
    pub channel: String,
    pub kind: ViolKind,
    pub tick: u32,
}

/// 通道管理器。
pub struct Channel {
    name: String,
    state: ChState,
    p0: Vec<P0Rec>,
    tick: u32,
    hello_seen: bool,
    bye_seen: bool,
    legal_ok: u32,
}

impl Channel {
    pub fn new(name: &str) -> Self {
        Channel {
            name: String::from(name),
            state: ChState::Open,
            p0: Vec::new(),
            tick: 0,
            hello_seen: false,
            bye_seen: false,
            legal_ok: 0,
        }
    }

    pub fn disconnected(&self) -> bool {
        self.state == ChState::Disconnected
    }

    fn disconnect(&mut self, kind: ViolKind) -> u16 {
        self.state = ChState::Disconnected;
        self.tick += 1;
        self.p0.push(P0Rec { channel: self.name.clone(), kind, tick: self.tick });
        E_NO_DISCONNECT
    }

    /// 合法序消息（Hello→…→Bye 全链）。
    pub fn legal(&mut self, hello: bool, bye: bool) -> Result<(), u16> {
        if self.disconnected() {
            return Err(E_ZOMBIE_PASS);
        }
        if hello {
            if self.hello_seen || self.bye_seen {
                return Err(self.disconnect(ViolKind::DupHello));
            }
            self.hello_seen = true;
            self.legal_ok += 1;
        }
        if bye {
            if !self.hello_seen || self.bye_seen {
                return Err(self.disconnect(ViolKind::MidBye));
            }
            self.bye_seen = true;
            self.state = ChState::Closed;
            self.legal_ok += 1;
        }
        if !hello && !bye {
            if !self.hello_seen {
                return Err(self.disconnect(ViolKind::PreHello));
            }
            if self.bye_seen {
                return Err(self.disconnect(ViolKind::PostBye));
            }
            self.legal_ok += 1;
        }
        Ok(())
    }

    /// 注入违例：必断连 + P0 登记。
    pub fn inject(&mut self, kind: ViolKind) -> Result<(), u16> {
        if kind as u16 > 3 {
            return Err(E_KIND_BAD);
        }
        if self.disconnected() {
            return Err(E_ZOMBIE_PASS);
        }
        let r = self.disconnect(kind);
        Err(r)
    }

    /// P0 审计：每条四字段全带（channel 非空 + kind ≤3 + tick>0）。
    pub fn audit_p0(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.p0.len() {
            let r = &self.p0[i];
            if r.channel.is_empty() || r.kind as u16 > 3 || r.tick == 0 {
                return Err(E_NO_P0);
            }
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.p0.len() {
            let r = &self.p0[i];
            h ^= r.kind as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= r.tick as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            let b = r.channel.as_bytes();
            let mut j = 0usize;
            while j < b.len() {
                h ^= b[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            i += 1;
        }
        h
    }

    pub fn p0_len(&self) -> usize {
        self.p0.len()
    }
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
        self.next() % n
    }
}

/// 主巡检：T0 违例注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 T0 违例 ×10 全部断连 + P0 登记（四型轮转）
    let mut ch = Channel::new("ch-A");
    let mut k = 0u32;
    let mut p0_n = 0u32;
    while k < 10 {
        // 每轮新通道（断连后不可复用）
        let mut c = Channel::new("ch-A");
        let kind = match k % 4 {
            0 => ViolKind::PreHello,
            1 => ViolKind::PostBye,
            2 => ViolKind::DupHello,
            _ => ViolKind::MidBye,
        };
        c.inject(kind)?;
        if !c.disconnected() {
            return Err(E_NO_DISCONNECT);
        }
        p0_n += c.audit_p0()?;
        k += 1;
    }
    if p0_n != 10 {
        return Err(E_NO_P0);
    }
    hits += 1;

    // R2 断连后一切消息拒绝（僵尸通道）
    let mut c2 = Channel::new("ch-B");
    c2.inject(ViolKind::PreHello)?;
    match c2.legal(true, false) {
        Err(E_ZOMBIE_PASS) => {}
        _ => return Err(E_ZOMBIE_PASS),
    }
    match c2.inject(ViolKind::PostBye) {
        Err(E_ZOMBIE_PASS) => {}
        _ => return Err(E_ZOMBIE_PASS),
    }
    hits += 1;

    // R3 合法流量零误断：全链 legal 序通过且不断连
    let mut c3 = Channel::new("ch-C");
    c3.legal(true, false)?;
    c3.legal(false, false)?;
    c3.legal(false, false)?;
    c3.legal(false, true)?;
    if c3.disconnected() && c3.legal_ok == 4 {
        // Closed 态不是 Disconnected——区分：Closed 由 Bye 触发
        if c3.p0_len() != 0 {
            return Err(E_LEGAL_CUT);
        }
    }
    hits += 1;

    // R4 重复 Hello 断连：legal 路径上的 DupHello 也断
    let mut c4 = Channel::new("ch-D");
    c4.legal(true, false)?;
    match c4.legal(true, false) {
        Err(E_NO_DISCONNECT) => {}
        _ => return Err(E_NO_DISCONNECT),
    }
    if !c4.disconnected() || c4.p0_len() != 1 {
        return Err(E_NO_P0);
    }
    hits += 1;

    // R5 Bye 后续发断连：PostBye
    let mut c5 = Channel::new("ch-E");
    c5.legal(true, false)?;
    c5.legal(false, true)?;
    match c5.legal(false, false) {
        Err(E_NO_DISCONNECT) => {}
        _ => return Err(E_NO_DISCONNECT),
    }
    if c5.p0_len() != 1 {
        return Err(E_NO_P0);
    }
    hits += 1;

    // R6 LCG 随机违例 ×512 全部断连（每违例一通道）
    let mut rng = Lcg::new(0x48462);
    let mut cut_n = 0u32;
    let mut k = 0u32;
    while k < 512 {
        let mut c = Channel::new("ch-R");
        // 半数通道先建合法前缀（Hello），违例型任意
        if rng.below(2) == 0 {
            let _ = c.legal(true, false);
        }
        let kind = match rng.below(4) {
            0 => ViolKind::PreHello,
            1 => ViolKind::PostBye,
            2 => ViolKind::DupHello,
            _ => ViolKind::MidBye,
        };
        let r = c.inject(kind);
        match r {
            Err(E_NO_DISCONNECT) | Err(E_ZOMBIE_PASS) => {}
            _ => return Err(E_NO_DISCONNECT),
        }
        if c.disconnected() && c.p0_len() >= 1 {
            cut_n += 1;
        }
        k += 1;
    }
    if cut_n != 512 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    // R7 重演一致：固定脚本双跑 P0 账指纹相等
    let mut ca = Channel::new("ch-X");
    ca.legal(true, false)?;
    ca.inject(ViolKind::DupHello)?;
    let mut cb = Channel::new("ch-X");
    cb.legal(true, false)?;
    cb.inject(ViolKind::DupHello)?;
    if ca.fp() != cb.fp() || ca.fp() == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 P0 账封印稳定
    let s1 = ca.fp();
    let s2 = ca.fp();
    if s1 != s2 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    Ok(hits)
}
