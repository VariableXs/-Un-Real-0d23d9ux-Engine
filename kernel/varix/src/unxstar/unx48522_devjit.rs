//! UNX-F48522 —— E·设备抖动注入（反：快速拔插 ×10 重枚举后零幽灵设备）
//! 判据原文：UNX-F48522-J1 反向判据：对 M2 总线做快速拔插抖动注入，
//! 连续 10 轮重枚举后枚举集必须与真实在位集逐位一致（零幽灵设备）：
//! 枚举集不得残留已拔设备，也不得漏登在位设备；
//! 拔插账守恒（每轮 plug/unplug 逐笔记账）、重枚举幂等、
//! 指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 拔插闭集；R2 十轮重枚举零幽灵；
//! R3 拔后即从枚举集消失；R4 插后即入枚举集；
//! R5 拔插账守恒；R6 重枚举幂等；R7 幽灵检出器自洽；
//! R8 fuzz 抖动流零错态；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xECC0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_OP: u16 = SEG_BASE + 1; // 拔插操作越出闭集
const E_GHOST: u16 = SEG_BASE + 2; // 枚举集残留幽灵设备
const E_MISS: u16 = SEG_BASE + 3; // 枚举集漏登在位设备
const E_CONSERV: u16 = SEG_BASE + 4; // 拔插账不守恒
const E_IDEM: u16 = SEG_BASE + 5; // 重枚举不幂等
const E_DETECT: u16 = SEG_BASE + 6; // 幽灵检出器失灵
const E_SLOT: u16 = SEG_BASE + 7; // 槽位闭集异常
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 抖动流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 拔插操作闭集：1 插入 / 2 拔出。
pub const OP_PLUG: u8 = 1;
pub const OP_UNPLUG: u8 = 2;

/// 槽位闭集：1..=8。
pub const SLOT_N: u8 = 8;

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 拔插操作闭集判定。
pub fn known_op(op: u8) -> bool {
    op == OP_PLUG || op == OP_UNPLUG
}

/// 槽位闭集判定。
pub fn known_slot(s: u8) -> bool {
    s >= 1 && s <= SLOT_N
}

/// 一笔拔插事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JitEvent {
    pub op: u8,
    pub slot: u8,
    pub round: u32,
}

/// 拔插账：每槽 (plug 计数, unplug 计数)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotAccount {
    pub slot: u8,
    pub plugs: u32,
    pub unplugs: u32,
}

/// 设备总线：真实在位集 + 枚举集 + 拔插账。
pub struct DevBus {
    pub live: [bool; 8],
    pub enumerated: [bool; 8],
    pub accounts: Vec<SlotAccount>,
    pub round: u32,
}

impl DevBus {
    pub fn new() -> DevBus {
        DevBus {
            live: [false; 8],
            enumerated: [false; 8],
            accounts: vec![],
            round: 0,
        }
    }

    fn acc_idx(&self, slot: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.accounts.len() {
            if self.accounts[i].slot == slot {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 执行一笔拔插：真实集联动 + 拔插账逐笔记账。
    pub fn apply(&mut self, ev: JitEvent) -> Result<(), u16> {
        if !known_op(ev.op) || !known_slot(ev.slot) {
            return Err(E_OP);
        }
        let si = (ev.slot - 1) as usize;
        match ev.op {
            OP_PLUG => {
                if self.live[si] {
                    return Err(E_SLOT);
                }
                self.live[si] = true;
            }
            _ => {
                if !self.live[si] {
                    return Err(E_SLOT);
                }
                self.live[si] = false;
            }
        }
        if self.acc_idx(ev.slot).is_none() {
            self.accounts.push(SlotAccount {
                slot: ev.slot,
                plugs: 0,
                unplugs: 0,
            });
        }
        let idx = self.acc_idx(ev.slot);
        let i = match idx {
            Some(i) => i,
            None => return Err(E_CONSERV),
        };
        if ev.op == OP_PLUG {
            self.accounts[i].plugs += 1;
        } else {
            self.accounts[i].unplugs += 1;
        }
        self.round = ev.round;
        Ok(())
    }

    /// 重枚举：枚举集 := 真实在位集（幂等）。
    pub fn reenumerate(&mut self) {
        let mut si = 0usize;
        while si < 8 {
            self.enumerated[si] = self.live[si];
            si += 1;
        }
    }

    /// 零幽灵：枚举集 ⊆ 真实集（无残留）。
    pub fn no_ghost(&self) -> bool {
        let mut si = 0usize;
        while si < 8 {
            if self.enumerated[si] && !self.live[si] {
                return false;
            }
            si += 1;
        }
        true
    }

    /// 零漏登：真实集 ⊆ 枚举集（无遗漏）。
    pub fn no_missing(&self) -> bool {
        let mut si = 0usize;
        while si < 8 {
            if self.live[si] && !self.enumerated[si] {
                return false;
            }
            si += 1;
        }
        true
    }

    /// 枚举集与真实集逐位一致。
    pub fn exact(&self) -> bool {
        self.no_ghost() && self.no_missing()
    }

    /// 拔插账守恒：每槽在位态 == plugs - unplugs 的布尔特化。
    pub fn conserved(&self) -> bool {
        let mut si = 0usize;
        while si < self.accounts.len() {
            let a = &self.accounts[si];
            let idx = (a.slot - 1) as usize;
            let now = self.live[idx];
            let was = a.plugs > a.unplugs
                || (a.plugs == a.unplugs && a.plugs > 0 && !now);
            if a.plugs < a.unplugs {
                return false;
            }
            // 在位时 plugs 必须 > unplugs；不在位时 plugs == unplugs。
            if now && a.plugs <= a.unplugs {
                return false;
            }
            if !now && a.plugs != a.unplugs {
                return false;
            }
            let _ = was;
            si += 1;
        }
        true
    }
}

/// 幽灵检出器：独立对枚举集与真实集逐位比对，返回幽灵槽位数。
pub fn ghost_count(enumerated: &[bool; 8], live: &[bool; 8]) -> u32 {
    let mut n = 0u32;
    let mut si = 0usize;
    while si < 8 {
        if enumerated[si] && !live[si] {
            n += 1;
        }
        si += 1;
    }
    n
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 拔插闭集：op 1/2 合法、0/3/255 非法；槽位 1..=8 合法、0/9 非法。
    if !known_op(OP_PLUG) || !known_op(OP_UNPLUG) || known_op(0) || known_op(3) {
        return Err(E_OP);
    }
    if !known_slot(1) || !known_slot(8) || known_slot(0) || known_slot(9) {
        return Err(E_SLOT);
    }
    hits += 1;

    // R2 十轮重枚举零幽灵：每轮拔插若干后重枚举，exact() 恒真。
    let mut bus = DevBus::new();
    let mut lcg = Lcg::new(0x48522);
    let mut round = 1u32;
    while round <= 10 {
        // 每轮固定抖动脚本：插 1/3/5 → 拔 3 → 插 2 → 拔 1。
        bus.apply(JitEvent { op: OP_PLUG, slot: 1, round })?;
        bus.apply(JitEvent { op: OP_PLUG, slot: 3, round })?;
        bus.apply(JitEvent { op: OP_PLUG, slot: 5, round })?;
        bus.apply(JitEvent { op: OP_UNPLUG, slot: 3, round })?;
        bus.apply(JitEvent { op: OP_PLUG, slot: 2, round })?;
        bus.apply(JitEvent { op: OP_UNPLUG, slot: 1, round })?;
        bus.reenumerate();
        if !bus.exact() {
            return Err(E_GHOST);
        }
        let _ = lcg.below(4);
        round += 1;
    }
    hits += 1;

    // R3 拔后即从枚举集消失：插 4 → 枚举 → 拔 4 → 枚举，槽 4 必不在枚举集。
    let mut bus2 = DevBus::new();
    bus2.apply(JitEvent { op: OP_PLUG, slot: 4, round: 1 })?;
    bus2.reenumerate();
    if !bus2.enumerated[3] {
        return Err(E_MISS);
    }
    bus2.apply(JitEvent { op: OP_UNPLUG, slot: 4, round: 1 })?;
    bus2.reenumerate();
    if bus2.enumerated[3] {
        return Err(E_GHOST);
    }
    hits += 1;

    // R4 插后即入枚举集：插 6 → 枚举，槽 6 必在枚举集。
    bus2.apply(JitEvent { op: OP_PLUG, slot: 6, round: 2 })?;
    bus2.reenumerate();
    if !bus2.enumerated[5] || !bus2.live[5] {
        return Err(E_MISS);
    }
    hits += 1;

    // R5 拔插账守恒：R2 脚本 10 轮后每槽 plugs/unplugs 与在位态一致。
    if !bus.conserved() {
        return Err(E_CONSERV);
    }
    // 槽 1：10 轮各 1 插 1 拔 → 不在位。
    let mut found = false;
    let mut i = 0usize;
    while i < bus.accounts.len() {
        if bus.accounts[i].slot == 1 {
            if bus.accounts[i].plugs != 10 || bus.accounts[i].unplugs != 10 {
                return Err(E_CONSERV);
            }
            found = true;
        }
        i += 1;
    }
    if !found {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 重枚举幂等：连续两次枚举结果逐位一致。
    let mut bus3 = DevBus::new();
    bus3.apply(JitEvent { op: OP_PLUG, slot: 7, round: 1 })?;
    bus3.apply(JitEvent { op: OP_PLUG, slot: 8, round: 1 })?;
    bus3.reenumerate();
    let snap = bus3.enumerated;
    bus3.reenumerate();
    let mut si = 0usize;
    while si < 8 {
        if bus3.enumerated[si] != snap[si] {
            return Err(E_IDEM);
        }
        si += 1;
    }
    hits += 1;

    // R7 幽灵检出器自洽：人为构造幽灵集与干净集，计数恰为 1 与 0。
    let mut en = [false; 8];
    let mut lv = [false; 8];
    en[2] = true;
    if ghost_count(&en, &lv) != 1 {
        return Err(E_DETECT);
    }
    lv[2] = true;
    if ghost_count(&en, &lv) != 0 {
        return Err(E_DETECT);
    }
    hits += 1;

    // R8 fuzz 抖动流零错态：LCG 驱动 8 槽 × 10 轮随机合法拔插
    // （只在合法态上操作），每轮枚举后零幽灵、零漏登、账守恒。
    let mut bus4 = DevBus::new();
    let mut lcg2 = Lcg::new(0x26E1_20);
    let mut round = 1u32;
    while round <= 10 {
        let mut slot = 1u8;
        while slot <= 8 {
            let op = if bus4.live[(slot - 1) as usize] { OP_UNPLUG } else { OP_PLUG };
            if lcg2.below(2) == 0 {
                bus4.apply(JitEvent { op, slot, round })?;
            }
            slot += 1;
        }
        bus4.reenumerate();
        if !bus4.exact() || !bus4.conserved() {
            return Err(E_FUZZ);
        }
        round += 1;
    }
    if ghost_count(&bus4.enumerated, &bus4.live) != 0 {
        return Err(E_GHOST);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = bus_fp(&bus4);
    let f2 = bus_fp(&bus4);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：round=/live=/enum= 三段前缀在位。
    let line = bus_line(&bus4);
    if !has_kw(line.as_bytes(), b"round=")
        || !has_kw(line.as_bytes(), b"live=")
        || !has_kw(line.as_bytes(), b"enum=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：round + 真实集 + 枚举集 + 拔插账串联。
pub fn bus_fp(b: &DevBus) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&b.round.to_le_bytes());
    let mut si = 0usize;
    while si < 8 {
        buf.push(b.live[si] as u8);
        buf.push(b.enumerated[si] as u8);
        si += 1;
    }
    let mut i = 0usize;
    while i < b.accounts.len() {
        buf.push(b.accounts[i].slot);
        buf.extend_from_slice(&b.accounts[i].plugs.to_le_bytes());
        buf.extend_from_slice(&b.accounts[i].unplugs.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：round=<轮次> live=<在位数> enum=<枚举数>。
pub fn bus_line(b: &DevBus) -> String {
    let mut live_n = 0u32;
    let mut enum_n = 0u32;
    let mut si = 0usize;
    while si < 8 {
        if b.live[si] {
            live_n += 1;
        }
        if b.enumerated[si] {
            enum_n += 1;
        }
        si += 1;
    }
    let mut s = String::new();
    s.push_str("round=");
    s.push_str(&b.round.to_string());
    s.push_str(" live=");
    s.push_str(&live_n.to_string());
    s.push_str(" enum=");
    s.push_str(&enum_n.to_string());
    s
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
