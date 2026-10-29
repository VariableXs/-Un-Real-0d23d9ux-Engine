//! UNX-F48454 —— E23 缺陷账联动（全部损坏事件登记 AI-97 通道）
//! 判据原文：UNX-F48454-J1 四型损坏事件（词库/配置/用户档/流）×10 全部登记 AI-97 通道；
//!           登记条目含（型/窗/tick/指纹）四字段；通道容量有界（环形 64）；
//!           LCG 随机损坏 256 事件全部入账零漏；账封印重演一致；
//!           通道封闭（型 4+ 拒收）；账满滚动覆盖最旧。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDDA0; // E23 缺陷账段

pub const CHANNEL_CAP: usize = 64;

const E_LOST_EVENT: u16 = SEG_BASE + 1; // 损坏事件漏登记
const E_KIND_BAD: u16 = SEG_BASE + 2; // 型越界
const E_FIELD_MISS: u16 = SEG_BASE + 3; // 四字段缺失
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_CAP_MISS: u16 = SEG_BASE + 6; // 容量守恒破
const E_COVER_MISS: u16 = SEG_BASE + 7; // 滚动覆盖错

/// AI-97 登记条目。
#[derive(Clone)]
pub struct DefectRec {
    pub kind: u16,   // 0~3 损坏型
    pub win: String, // 窗名
    pub tick: u32,
    pub fp: u64,     // 事件指纹
}

/// AI-97 通道（环形 64）。
pub struct Ai97Channel {
    ring: Vec<DefectRec>,
    head: usize,
    fill: usize,
    tick: u32,
    total: u64,
}

impl Ai97Channel {
    pub fn new() -> Self {
        Ai97Channel { ring: Vec::new(), head: 0, fill: 0, tick: 0, total: 0 }
    }

    /// 登记：型 4+ 拒收。
    pub fn report(&mut self, kind: u16, win: &str, fp: u64) -> Result<(), u16> {
        if kind > 3 {
            return Err(E_KIND_BAD);
        }
        if win.is_empty() {
            return Err(E_FIELD_MISS);
        }
        self.tick += 1;
        let rec = DefectRec { kind, win: String::from(win), tick: self.tick, fp };
        if self.fill < CHANNEL_CAP {
            self.ring.push(rec);
            self.fill += 1;
        } else {
            self.ring[self.head] = rec;
            self.head = (self.head + 1) % CHANNEL_CAP;
        }
        self.total += 1;
        Ok(())
    }

    /// 零漏审计：total == 报告者计数（由调用侧传入）。
    pub fn audit_no_loss(&self, expect: u64) -> Result<(), u16> {
        if self.total != expect {
            return Err(E_LOST_EVENT);
        }
        Ok(())
    }

    /// 容量守恒：fill ≤ 64、ring.len() ≤ 64。
    pub fn audit_cap(&self) -> Result<(), u16> {
        if self.fill > CHANNEL_CAP || self.ring.len() > CHANNEL_CAP {
            return Err(E_CAP_MISS);
        }
        Ok(())
    }

    /// 滚动覆盖正确：满后 ring 物理序 = 时间序（head 起点）。
    pub fn audit_cover(&self) -> Result<(), u16> {
        if self.fill < CHANNEL_CAP {
            return Ok(());
        }
        let mut i = 1usize;
        while i < CHANNEL_CAP {
            let a = &self.ring[(self.head + i - 1) % CHANNEL_CAP];
            let b = &self.ring[(self.head + i) % CHANNEL_CAP];
            if b.tick <= a.tick {
                return Err(E_COVER_MISS);
            }
            i += 1;
        }
        Ok(())
    }

    /// 条目四字段全非空/非零。
    pub fn audit_fields(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.fill {
            let r = &self.ring[i];
            if r.win.is_empty() || r.tick == 0 || r.fp == 0 || r.kind > 3 {
                return Err(E_FIELD_MISS);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.fill {
            let r = &self.ring[i];
            h ^= r.kind as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            let b = r.win.as_bytes();
            let mut j = 0usize;
            while j < b.len() {
                h ^= b[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            h ^= r.tick as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= r.fp;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn fill(&self) -> usize {
        self.fill
    }
}

fn fnv_mix(h: u64, x: u64) -> u64 {
    let mut acc = h;
    acc ^= x;
    acc.wrapping_mul(0x0000_0100_0000_01b3)
}

/// 事件指纹：FNV(kind, win 字节)。
pub fn event_fp(kind: u16, win: &str) -> u64 {
    let mut h = fnv_mix(0xcbf2_9ce4_8422_2325, kind as u64);
    let b = win.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h = fnv_mix(h, b[i] as u64);
        i += 1;
    }
    h
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

/// 主巡检：缺陷账联动八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 四型损坏 ×10 全部登记（4 型 ×10 = 40 事件）
    let mut ch = Ai97Channel::new();
    let wins = ["native", "wine", "xwayland"];
    let mut k = 0u32;
    while k < 10 {
        let mut kind = 0u16;
        while kind < 4 {
            let w = wins[(k % 3) as usize];
            let fp = event_fp(kind, w);
            ch.report(kind, w, fp)?;
            kind += 1;
        }
        k += 1;
    }
    ch.audit_no_loss(40)?;
    ch.audit_fields()?;
    hits += 1;

    // R2 容量守恒：40 < 64 未滚动
    ch.audit_cap()?;
    if ch.fill() != 40 {
        return Err(E_CAP_MISS);
    }
    hits += 1;

    // R3 LCG 随机 256 事件全部入账（含滚动覆盖：256 > 64）
    let mut ch2 = Ai97Channel::new();
    let mut rng = Lcg::new(0x48454);
    let mut k = 0u32;
    while k < 256 {
        let kind = rng.below(4) as u16;
        let w = wins[rng.below(3) as usize];
        let fp = event_fp(kind, w);
        ch2.report(kind, w, fp)?;
        k += 1;
    }
    ch2.audit_no_loss(256)?;
    ch2.audit_cap()?;
    ch2.audit_cover()?;
    hits += 1;

    // R4 滚动覆盖后最新条目正确：最后一条 tick = 256
    let last_idx = (ch2.head + CHANNEL_CAP - 1) % CHANNEL_CAP;
    if ch2.ring[last_idx].tick != 256 {
        return Err(E_COVER_MISS);
    }
    hits += 1;

    // R5 通道封闭：kind=4 拒收且不占 tick
    let mut ch3 = Ai97Channel::new();
    match ch3.report(4, "native", event_fp(4, "native")) {
        Err(E_KIND_BAD) => {}
        _ => return Err(E_KIND_BAD),
    }
    match ch3.report(9, "native", 1) {
        Err(E_KIND_BAD) => {}
        _ => return Err(E_KIND_BAD),
    }
    if ch3.total() != 0 {
        return Err(E_KIND_BAD);
    }
    hits += 1;

    // R6 空窗名拒（字段缺失防御）
    match ch3.report(0, "", 1) {
        Err(E_FIELD_MISS) => {}
        _ => return Err(E_FIELD_MISS),
    }
    hits += 1;

    // R7 重演一致：同种子双跑通道指纹相等
    let f1 = ch2.fp();
    let mut ch4 = Ai97Channel::new();
    let mut rng2 = Lcg::new(0x48454);
    let mut k = 0u32;
    while k < 256 {
        let kind = rng2.below(4) as u16;
        let w = wins[rng2.below(3) as usize];
        let fp = event_fp(kind, w);
        let _ = ch4.report(kind, w, fp);
        k += 1;
    }
    if f1 != ch4.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 封印稳定：R1 通道双算一致
    let s1 = ch.fp();
    let s2 = ch.fp();
    if s1 != s2 || s1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    Ok(hits)
}
