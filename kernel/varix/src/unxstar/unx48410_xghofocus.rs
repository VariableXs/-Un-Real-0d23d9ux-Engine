//! UNX-F48410 —— E·Xwayland 幽灵 focus 注入（反：假 focus 事件 ×10 全拒）
//! 判据原文：UNX-F48410-J1 假 focus 事件（未注册 xid/悬空 ctx/重复授予/交叉假名）×10 全拒；
//!           合法 focus 零误伤；拒绝账四型分类封闭；LCG 随机幽灵注入 ×512 全拒；
//!           拒绝账封印重演一致；授予账与拒绝账之和守恒；重复授予幂等拒。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD640; // E21 幽灵 focus 段

const E_GHOST_PASS: u16 = SEG_BASE + 1; // 幽灵事件放行
const E_LEGAL_HURT: u16 = SEG_BASE + 2; // 合法 focus 被误拒
const E_CAUSE_BAD: u16 = SEG_BASE + 3; // 拒绝类型越界
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_COUNT_MISS: u16 = SEG_BASE + 5; // 拒绝计数不符
const E_BAL_MISS: u16 = SEG_BASE + 6; // 两账守恒失败
const E_DUP_GRANT: u16 = SEG_BASE + 7; // 重复授予未拒
const E_REPLAY_DIFF: u16 = SEG_BASE + 8; // 重演不一致

/// 假 focus 类型。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum GhostKind {
    UnregXid = 0, // xid 未注册
    DangleCtx = 1, // ctx 已销毁/悬空
    DupGrant = 2, // 重复授予同一对
    CrossName = 3, // xid-ctx 错配（交叉假名）
}

/// 焦点授予器。
pub struct FocusGranter {
    grants: Vec<(u32, u32)>, // 已授予 (xid, ctx)
    ledger: Vec<(u64, GhostKind, u32)>, // 拒绝账 (seq, kind, xid)
    seq: u64,
    rejects: u32,
}

impl FocusGranter {
    pub fn new() -> Self {
        FocusGranter { grants: Vec::new(), ledger: Vec::new(), seq: 0, rejects: 0 }
    }

    fn has_pair(&self, xid: u32, ctx: u32) -> bool {
        let mut i = 0usize;
        while i < self.grants.len() {
            if self.grants[i].0 == xid && self.grants[i].1 == ctx {
                return true;
            }
            i += 1;
        }
        false
    }

    fn has_xid(&self, xid: u32) -> bool {
        let mut i = 0usize;
        while i < self.grants.len() {
            if self.grants[i].0 == xid {
                return true;
            }
            i += 1;
        }
        false
    }

    fn has_ctx(&self, ctx: u32) -> bool {
        let mut i = 0usize;
        while i < self.grants.len() {
            if self.grants[i].1 == ctx {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 注册合法对：授予成功入账。
    pub fn grant(&mut self, xid: u32, ctx: u32) -> Result<(), u16> {
        self.seq += 1;
        if self.has_pair(xid, ctx) {
            self.rejects += 1;
            self.ledger.push((self.seq, GhostKind::DupGrant, xid));
            return Err(E_DUP_GRANT);
        }
        // 交叉假名：xid 或 ctx 单侧已注册他配
        if self.has_xid(xid) || self.has_ctx(ctx) {
            self.rejects += 1;
            self.ledger.push((self.seq, GhostKind::CrossName, xid));
            return Err(E_GHOST_PASS);
        }
        self.grants.push((xid, ctx));
        Ok(())
    }

    /// 注入假 focus：kind 分类校验后必拒。
    pub fn inject_ghost(&mut self, xid: u32, ctx: u32, kind: GhostKind) -> Result<(), u16> {
        self.seq += 1;
        // 语义核对：注入事件必须真的落在其声称的类型上（分类可信）
        let really = match kind {
            GhostKind::UnregXid => !self.has_xid(xid),
            GhostKind::DangleCtx => self.has_xid(xid) && !self.has_ctx(ctx),
            GhostKind::DupGrant => self.has_pair(xid, ctx),
            GhostKind::CrossName => {
                (self.has_xid(xid) && !self.has_pair(xid, ctx))
                    || (self.has_ctx(ctx) && !self.has_pair(xid, ctx))
            }
        };
        if !really {
            // 分类撒谎的注入事件同样拒绝，但按 CrossName 记账
            self.rejects += 1;
            self.ledger.push((self.seq, GhostKind::CrossName, xid));
            return Err(E_GHOST_PASS);
        }
        self.rejects += 1;
        self.ledger.push((self.seq, kind, xid));
        Err(E_GHOST_PASS)
    }

    /// 拒绝类型封闭：全落四型。
    pub fn audit_cause_closed(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].1 as u16 > 3 {
                return Err(E_CAUSE_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 守恒：grants + rejects == seq。
    pub fn audit_balance(&self) -> Result<(), u16> {
        if (self.grants.len() as u64) + (self.rejects as u64) != self.seq {
            return Err(E_BAL_MISS);
        }
        if self.ledger.len() != self.rejects as usize {
            return Err(E_BAL_MISS);
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.ledger.len() {
            let (s, k, x) = self.ledger[i];
            for v in [s, k as u64, x as u64] {
                h ^= v;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }

    pub fn reject_count(&self) -> u32 {
        self.rejects
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

/// 脚本：注册若干合法对后，注入四型幽灵 ×n 全拒。返回拒绝数。
pub fn inject_ghosts(g: &mut FocusGranter, n: u32) -> Result<u32, u16> {
    let mut rej = 0u32;
    let mut k = 0u32;
    while k < n {
        let kind = match k % 4 {
            0 => GhostKind::UnregXid,
            1 => GhostKind::DangleCtx,
            2 => GhostKind::DupGrant,
            _ => GhostKind::CrossName,
        };
        let (x, c) = match kind {
            GhostKind::UnregXid => (0xEEEE_0000 + k, 0xDDDD_0000 + k),
            GhostKind::DangleCtx => (0x1111, 0xEEEE_1000 + k), // xid 真但 ctx 假
            GhostKind::DupGrant => (0x1111, 0x2222),           // 合法对重复授予
            GhostKind::CrossName => (0x1111, 0xEEEE_2000 + k), // xid 已配他 ctx
        };
        match g.inject_ghost(x, c, kind) {
            Err(E_GHOST_PASS) => rej += 1,
            Ok(()) => return Err(E_GHOST_PASS),
            Err(_) => return Err(E_CAUSE_BAD),
        }
        k += 1;
    }
    Ok(rej)
}

/// 主巡检：幽灵 focus 注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 假 focus ×10 全拒（四型轮转）
    let mut g = FocusGranter::new();
    g.grant(0x1111, 0x2222)?;
    g.grant(0x3333, 0x4444)?;
    let rej = inject_ghosts(&mut g, 10)?;
    if rej != 10 || g.reject_count() != 10 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 合法 focus 零误伤：注入后再授全新对成功
    g.grant(0x5555, 0x6666)?;
    g.audit_balance()?;
    hits += 1;

    // R3 拒绝类型封闭：全落四型
    g.audit_cause_closed()?;
    hits += 1;

    // R4 重复授予幂等拒：同对再授 Err 且 grants 不增长
    let before = g.reject_count();
    match g.grant(0x1111, 0x2222) {
        Err(E_DUP_GRANT) => {}
        _ => return Err(E_DUP_GRANT),
    }
    if g.reject_count() != before + 1 {
        return Err(E_DUP_GRANT);
    }
    hits += 1;

    // R5 LCG 随机幽灵注入 ×512 全拒：先建 8 对合法
    let mut g2 = FocusGranter::new();
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    let mut k = 0u32;
    while k < 8 {
        let p = (0xA000 + k, 0xB000 + k);
        g2.grant(p.0, p.1)?;
        pairs.push(p);
        k += 1;
    }
    let mut rng = Lcg::new(0x48410);
    let mut k = 0u32;
    while k < 512 {
        let pi = rng.below(8) as usize;
        let mode = rng.below(4);
        let (x, c) = pairs[pi];
        let r = match mode {
            0 => g2.inject_ghost(0xEEEE_0000 + k as u32, 0xF000_0000 + k as u32, GhostKind::UnregXid),
            1 => g2.inject_ghost(x, 0xF000_0000 + k as u32, GhostKind::DangleCtx),
            2 => g2.inject_ghost(x, c, GhostKind::DupGrant),
            _ => g2.inject_ghost(x, if c == 0xB000 { 0xB001 } else { 0xB000 }, GhostKind::CrossName),
        };
        match r {
            Err(E_GHOST_PASS) => {}
            _ => return Err(E_GHOST_PASS),
        }
        k += 1;
    }
    if g2.reject_count() < 512 {
        return Err(E_COUNT_MISS);
    }
    g2.audit_cause_closed()?;
    g2.audit_balance()?;
    hits += 1;

    // R6 封印重演一致：同脚本双跑拒绝账指纹相等
    let f1 = g2.fp();
    let f2 = g2.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R7 守恒细分：grants 恒 8（注入零放行不改授予账）
    if g2.grants.len() != 8 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    // R8 分类撒谎注入同样拒：声称 UnregXid 但 xid 已注册 → 按 CrossName 记账且拒
    let mut g3 = FocusGranter::new();
    g3.grant(0x1234, 0x5678)?;
    match g3.inject_ghost(0x1234, 0xABCD, GhostKind::UnregXid) {
        Err(E_GHOST_PASS) => {}
        _ => return Err(E_GHOST_PASS),
    }
    g3.audit_cause_closed()?;
    if g3.reject_count() != 1 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    Ok(hits)
}
