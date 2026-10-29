//! UNX-F48570 —— I·F4 变更回归通知链（接口变更→本域回归触发登记）
//! 判据原文：UNX-F48570-J1 F4 域接口变更事件（接口 id + 旧版→新版）进入
//!           通知链后本域必须登记回归触发项（iface/version 对）；
//!           同接口同版本对不重复登记；未知接口 id 拒收；
//!           链序保持（登记序 = 事件序）；LCG 随机事件流 ×256 全归类零崩；
//!           通知全投递；重演指纹一致；守恒 seq = 登记 + 拒收。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF6E0; // I29 F4 通知链段

const E_REG_MISS: u16 = SEG_BASE + 1; // 变更未登记触发项
const E_DUP_REG: u16 = SEG_BASE + 2; // 重复登记未拦
const E_IFACE_BAD: u16 = SEG_BASE + 3; // 未知接口 id 混入
const E_ORDER_BREAK: u16 = SEG_BASE + 4; // 链序破
const E_KIND_LEAK: u16 = SEG_BASE + 5; // 事件流归类泄漏
const E_DELIVER_MISS: u16 = SEG_BASE + 6; // 通知未全投递
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 8; // 守恒破

/// F4 已知接口白名单。
pub const KNOWN_IFACES: [u32; 4] = [0x10, 0x11, 0x12, 0x13];

/// 变更事件。
#[derive(Clone, Copy)]
pub struct ChangeEv {
    pub iface: u32,
    pub from_ver: u32,
    pub to_ver: u32,
}

/// 回归触发登记项。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RegEntry {
    pub iface: u32,
    pub from_ver: u32,
    pub to_ver: u32,
    pub order: u64,
}

/// 通知链。
pub struct NotifyChain {
    pub regs: Vec<RegEntry>,
    pub rejects: Vec<(u64, u32)>,
    pub delivered: Vec<u64>,
    pub seq: u64,
    pub label: String,
}

impl NotifyChain {
    pub fn new() -> Self {
        NotifyChain {
            regs: Vec::new(),
            rejects: Vec::new(),
            delivered: Vec::new(),
            seq: 0,
            label: "f4-notify".to_string(),
        }
    }

    fn known(&self, iface: u32) -> bool {
        let mut i = 0usize;
        while i < KNOWN_IFACES.len() {
            if KNOWN_IFACES[i] == iface {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 提交变更事件：未知接口拒；同接口同版本对幂等拒；否则登记。
    pub fn submit(&mut self, ev: &ChangeEv) -> Result<bool, u16> {
        self.seq += 1;
        if !self.known(ev.iface) {
            self.rejects.push((self.seq, ev.iface));
            return Err(E_IFACE_BAD);
        }
        if ev.to_ver <= ev.from_ver {
            self.rejects.push((self.seq, ev.iface));
            return Err(E_KIND_LEAK);
        }
        let mut i = 0usize;
        while i < self.regs.len() {
            if self.regs[i].iface == ev.iface
                && self.regs[i].from_ver == ev.from_ver
                && self.regs[i].to_ver == ev.to_ver
            {
                self.rejects.push((self.seq, ev.iface));
                return Ok(false); // 幂等：重复事件不重复登记
            }
            i += 1;
        }
        self.regs.push(RegEntry {
            iface: ev.iface,
            from_ver: ev.from_ver,
            to_ver: ev.to_ver,
            order: self.seq,
        });
        self.delivered.push(self.seq);
        Ok(true)
    }

    /// 链序保持：order 严格递增。
    pub fn audit_order(&self) -> Result<(), u16> {
        let mut i = 1usize;
        while i < self.regs.len() {
            if self.regs[i].order <= self.regs[i - 1].order {
                return Err(E_ORDER_BREAK);
            }
            i += 1;
        }
        Ok(())
    }

    /// 通知全投递：delivered 数 = 登记数。
    pub fn audit_delivered(&self) -> Result<(), u16> {
        if self.delivered.len() != self.regs.len() {
            return Err(E_DELIVER_MISS);
        }
        Ok(())
    }

    pub fn audit_balance(&self) -> Result<(), u16> {
        if self.seq != (self.regs.len() + self.rejects.len()) as u64 {
            return Err(E_BAL_MISS);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.regs.len() {
            h ^= self.regs[i].iface as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.regs[i].from_ver as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.regs[i].to_ver as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h ^= self.rejects.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
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
        (self.next() >> 16) % n
    }
}

/// 主巡检：F4 通知链九判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 变更事件登记触发项
    let mut ch = NotifyChain::new();
    let ev = ChangeEv { iface: 0x10, from_ver: 3, to_ver: 4 };
    match ch.submit(&ev) {
        Ok(true) => {}
        _ => return Err(E_REG_MISS),
    }
    if ch.regs.len() != 1 || ch.regs[0].iface != 0x10 || ch.regs[0].to_ver != 4 {
        return Err(E_REG_MISS);
    }
    ch.audit_delivered()?;
    hits += 1;

    // R2 同接口同版本对幂等不重复登记
    match ch.submit(&ev) {
        Ok(false) => {}
        _ => return Err(E_DUP_REG),
    }
    if ch.regs.len() != 1 {
        return Err(E_DUP_REG);
    }
    hits += 1;

    // R3 未知接口拒收
    let bad = ChangeEv { iface: 0x99, from_ver: 1, to_ver: 2 };
    match ch.submit(&bad) {
        Err(E_IFACE_BAD) => {}
        _ => return Err(E_IFACE_BAD),
    }
    hits += 1;

    // R4 版本回退/平版本拒收
    let back = ChangeEv { iface: 0x11, from_ver: 5, to_ver: 5 };
    match ch.submit(&back) {
        Err(E_KIND_LEAK) => {}
        _ => return Err(E_KIND_LEAK),
    }
    let back2 = ChangeEv { iface: 0x11, from_ver: 6, to_ver: 2 };
    match ch.submit(&back2) {
        Err(E_KIND_LEAK) => {}
        _ => return Err(E_KIND_LEAK),
    }
    hits += 1;

    // R5 链序保持：多接口多版本登记后 order 严格递增
    let evs: [ChangeEv; 5] = [
        ChangeEv { iface: 0x10, from_ver: 4, to_ver: 5 },
        ChangeEv { iface: 0x11, from_ver: 1, to_ver: 2 },
        ChangeEv { iface: 0x12, from_ver: 2, to_ver: 3 },
        ChangeEv { iface: 0x13, from_ver: 1, to_ver: 9 },
        ChangeEv { iface: 0x10, from_ver: 5, to_ver: 6 },
    ];
    let mut i = 0usize;
    while i < evs.len() {
        match ch.submit(&evs[i]) {
            Ok(true) => {}
            _ => return Err(E_REG_MISS),
        }
        i += 1;
    }
    ch.audit_order()?;
    if ch.regs.len() != 6 {
        return Err(E_REG_MISS);
    }
    hits += 1;

    // R6 LCG 随机事件流 ×256 全归类零崩
    let mut rng = Lcg::new(0x48570);
    let mut c2 = NotifyChain::new();
    let mut k = 0u32;
    while k < 256 {
        let pick = rng.below(6);
        let ev2 = if pick < 4 {
            ChangeEv {
                iface: KNOWN_IFACES[pick as usize],
                from_ver: rng.below(8) as u32,
                to_ver: 8 + rng.below(8) as u32,
            }
        } else if pick == 4 {
            ChangeEv { iface: 0x77, from_ver: 1, to_ver: 2 } // 未知接口
        } else {
            ChangeEv { iface: KNOWN_IFACES[0], from_ver: 9, to_ver: 3 } // 版本回退
        };
        match c2.submit(&ev2) {
            Ok(true) => {
                if ev2.iface == 0x77 || ev2.to_ver <= ev2.from_ver {
                    return Err(E_KIND_LEAK);
                }
            }
            Ok(false) => {}
            Err(E_IFACE_BAD) => {
                if ev2.iface != 0x77 {
                    return Err(E_KIND_LEAK);
                }
            }
            Err(E_KIND_LEAK) => {
                if ev2.to_ver > ev2.from_ver {
                    return Err(E_KIND_LEAK);
                }
            }
            Err(_) => return Err(E_KIND_LEAK),
        }
        k += 1;
    }
    c2.audit_order()?;
    c2.audit_delivered()?;
    c2.audit_balance()?;
    hits += 1;

    // R7 通知全投递：登记序与投递序一致
    let mut i = 0usize;
    while i < c2.delivered.len() {
        if c2.delivered[i] != c2.regs[i].order {
            return Err(E_DELIVER_MISS);
        }
        i += 1;
    }
    hits += 1;

    // R8 重演一致：同种子指纹相等
    let fp1 = c2.fingerprint();
    let mut rng2 = Lcg::new(0x48570);
    let mut c3 = NotifyChain::new();
    let mut k = 0u32;
    while k < 256 {
        let pick = rng2.below(6);
        let ev2 = if pick < 4 {
            ChangeEv {
                iface: KNOWN_IFACES[pick as usize],
                from_ver: rng2.below(8) as u32,
                to_ver: 8 + rng2.below(8) as u32,
            }
        } else if pick == 4 {
            ChangeEv { iface: 0x77, from_ver: 1, to_ver: 2 }
        } else {
            ChangeEv { iface: KNOWN_IFACES[0], from_ver: 9, to_ver: 3 }
        };
        let _ = c3.submit(&ev2);
        k += 1;
    }
    if c3.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R9 守恒：seq = 登记 + 拒收
    if ch.seq != (ch.regs.len() + ch.rejects.len()) as u64 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    Ok(hits)
}
