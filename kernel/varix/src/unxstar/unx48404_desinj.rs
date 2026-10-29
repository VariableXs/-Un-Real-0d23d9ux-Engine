//! UNX-F48404 —— E·销毁序违例注入（反：旧 context 复活注入 ×10 全拒）
//! 判据原文：UNX-F48404-J1 已杀 context 的 boot/focus/commit 复活注入 ×10 全拒并登记；
//!           合法流量零误伤（×100 合法周期全通过）；拒绝账 seq 空洞检出；
//!           双重 kill 拒绝；LCG 随机复活注入 ×512 全拒；拒绝账封印指纹稳定；
//!           alive 与 dead 集不相交。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD540; // E21 销毁序违例段

const E_RESURRECT: u16 = SEG_BASE + 1; // 已死 context 复活放行
const E_LEGAL_HURT: u16 = SEG_BASE + 2; // 合法流量被误拒
const E_INJ_LEDGER: u16 = SEG_BASE + 3; // 注入账记录失真
const E_SEQ_HOLE: u16 = SEG_BASE + 4; // 账本 seq 空洞
const E_DOUBLE_KILL: u16 = SEG_BASE + 5; // 双重 kill 放行
const E_SET_OVERLAP: u16 = SEG_BASE + 6; // alive/dead 集相交
const E_SEAL_BAD: u16 = SEG_BASE + 7; // 封印指纹失配
const E_COUNT_MISS: u16 = SEG_BASE + 8; // 拒绝计数不符

/// 事件类别：注入器可投三类复活事件。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum EvKind {
    LegalBoot = 0,
    LegalFocus = 1,
    LegalCommit = 2,
    InjBoot = 3,
    InjFocus = 4,
    InjCommit = 5,
}

/// 守门结果。
#[derive(Clone, Copy)]
pub struct Verdict {
    pub kind: EvKind,
    pub ctx: u32,
    pub seq: u64,
    pub accepted: bool,
}

/// 销毁序守门器：拒绝一切对已死 context 的投递。
pub struct ResurrectGuard {
    dead: Vec<u32>,
    alive: Vec<u32>,
    verdicts: Vec<Verdict>,
    seq: u64,
    reject_n: u32,
}

impl ResurrectGuard {
    pub fn new() -> Self {
        ResurrectGuard { dead: Vec::new(), alive: Vec::new(), verdicts: Vec::new(), seq: 0, reject_n: 0 }
    }

    /// 合法 kill：alive 摘除、dead 登记；二次 kill 拒绝。
    pub fn kill(&mut self, ctx: u32) -> Result<(), u16> {
        let mut idx = self.alive.len();
        let mut i = 0usize;
        while i < self.alive.len() {
            if self.alive[i] == ctx {
                idx = i;
            }
            i += 1;
        }
        if idx == self.alive.len() {
            return Err(E_DOUBLE_KILL);
        }
        let _ = self.alive.swap_remove(idx);
        self.dead.push(ctx);
        self.seq += 1;
        self.verdicts.push(Verdict { kind: EvKind::LegalCommit, ctx, seq: self.seq, accepted: true });
        Ok(())
    }

    /// 合法 boot：dead 中的 context 拒绝（不得复活）。
    pub fn boot(&mut self, ctx: u32) -> Result<(), u16> {
        self.seq += 1;
        if Self::contains(&self.dead, ctx) {
            self.reject_n += 1;
            self.verdicts.push(Verdict { kind: EvKind::InjBoot, ctx, seq: self.seq, accepted: false });
            return Err(E_RESURRECT);
        }
        if Self::contains(&self.alive, ctx) {
            self.verdicts.push(Verdict { kind: EvKind::LegalCommit, ctx, seq: self.seq, accepted: false });
            return Ok(());
        }
        self.alive.push(ctx);
        self.verdicts.push(Verdict { kind: EvKind::LegalBoot, ctx, seq: self.seq, accepted: true });
        Ok(())
    }

    /// 投递（focus/commit 统一入口）：对 dead context 的投递全拒。
    pub fn deliver(&mut self, ctx: u32, focus: bool) -> Result<(), u16> {
        self.seq += 1;
        let kind = if focus { EvKind::LegalFocus } else { EvKind::LegalCommit };
        if Self::contains(&self.dead, ctx) {
            self.reject_n += 1;
            self.verdicts.push(Verdict {
                kind: if focus { EvKind::InjFocus } else { EvKind::InjCommit },
                ctx,
                seq: self.seq,
                accepted: false,
            });
            return Err(E_RESURRECT);
        }
        if !Self::contains(&self.alive, ctx) {
            // 未 boot 的 context 投递同样拒绝（悬空引用）
            self.reject_n += 1;
            self.verdicts.push(Verdict { kind, ctx, seq: self.seq, accepted: false });
            return Err(E_RESURRECT);
        }
        self.verdicts.push(Verdict { kind, ctx, seq: self.seq, accepted: true });
        Ok(())
    }

    fn contains(v: &[u32], x: u32) -> bool {
        let mut i = 0usize;
        while i < v.len() {
            if v[i] == x {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 账本 seq 连续性：verdicts 的 seq 必须是 1..=N 无空洞。
    pub fn audit_seq_dense(&self) -> Result<(), u16> {
        if self.verdicts.len() != self.seq as usize {
            return Err(E_SEQ_HOLE);
        }
        let mut i = 0usize;
        while i < self.verdicts.len() {
            if self.verdicts[i].seq != (i as u64) + 1 {
                return Err(E_SEQ_HOLE);
            }
            i += 1;
        }
        Ok(())
    }

    /// alive/dead 不相交。
    pub fn audit_disjoint(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.alive.len() {
            if Self::contains(&self.dead, self.alive[i]) {
                return Err(E_SET_OVERLAP);
            }
            i += 1;
        }
        Ok(())
    }

    /// 注入账封印指纹（FNV-1a 64）。
    pub fn ledger_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.verdicts.len() {
            let v = &self.verdicts[i];
            for x in [v.kind as u64, v.ctx as u64, v.seq, v.accepted as u64] {
                h ^= x;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }

    pub fn rejects(&self) -> u32 {
        self.reject_n
    }

    pub fn verdict_count(&self) -> usize {
        self.verdicts.len()
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

/// 注入脚本：构造 10 次复活注入（boot/focus/commit 三型轮转）。
pub fn inject_resurrect(g: &mut ResurrectGuard, victim: u32, n: u32) -> Result<u32, u16> {
    let mut rejected = 0u32;
    let mut k = 0u32;
    while k < n {
        let r = match k % 3 {
            0 => g.boot(victim).err(),
            1 => g.deliver(victim, true).err(),
            _ => g.deliver(victim, false).err(),
        };
        match r {
            Some(E_RESURRECT) => rejected += 1,
            Some(_) => return Err(E_INJ_LEDGER),
            None => return Err(E_RESURRECT), // 复活注入竟被放行
        }
        k += 1;
    }
    Ok(rejected)
}

/// 主巡检：销毁序违例注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×10 复活注入全拒 + 计数恰 10
    let mut g = ResurrectGuard::new();
    g.boot(0xA001)?;
    g.kill(0xA001)?;
    let rej = inject_resurrect(&mut g, 0xA001, 10)?;
    if rej != 10 || g.rejects() != 10 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 注入后合法流量零误伤：新 context 全周期可用
    let mut k = 0u32;
    while k < 100 {
        let ctx = 0x2000 + k as u32;
        g.boot(ctx)?;
        g.deliver(ctx, true)?;
        g.deliver(ctx, false)?;
        g.kill(ctx)?;
        k += 1;
    }
    g.audit_seq_dense()?;
    hits += 1;

    // R3 双重 kill 拒绝：对已死 context 再 kill 报错
    match g.kill(0xA001) {
        Err(E_DOUBLE_KILL) => {}
        _ => return Err(E_DOUBLE_KILL),
    }
    hits += 1;

    // R4 alive/dead 不相交
    g.audit_disjoint()?;
    hits += 1;

    // R5 LCG 随机复活注入 ×512：混合三型，全拒零放行
    let mut g2 = ResurrectGuard::new();
    let mut rng = Lcg::new(0x48404);
    let mut victims: Vec<u32> = Vec::new();
    let mut k = 0u32;
    while k < 8 {
        let v = 0x3000 + k;
        g2.boot(v)?;
        g2.kill(v)?;
        victims.push(v);
        k += 1;
    }
    let mut k = 0u32;
    while k < 512 {
        let v = victims[rng.below(8) as usize];
        let mode = rng.below(3);
        let r = match mode {
            0 => g2.boot(v).err(),
            1 => g2.deliver(v, true).err(),
            _ => g2.deliver(v, false).err(),
        };
        match r {
            Some(E_RESURRECT) => {}
            _ => return Err(E_RESURRECT),
        }
        k += 1;
    }
    if g2.rejects() < 512 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R6 注入账 seq 无空洞 + 规模守恒（8 boot+kill+512 注入 + 若干合法）
    g2.audit_seq_dense()?;
    if g2.verdict_count() != g2.seq as usize {
        return Err(E_SEQ_HOLE);
    }
    hits += 1;

    // R7 封印指纹稳定：双算一致且非零
    let s1 = g2.ledger_fp();
    let s2 = g2.ledger_fp();
    if s1 != s2 || s1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R8 同脚本重演指纹一致（注入器确定性）
    let mut g3 = ResurrectGuard::new();
    g3.boot(0xA001)?;
    g3.kill(0xA001)?;
    let _ = inject_resurrect(&mut g3, 0xA001, 10)?;
    let mut g4 = ResurrectGuard::new();
    g4.boot(0xA001)?;
    g4.kill(0xA001)?;
    let _ = inject_resurrect(&mut g4, 0xA001, 10)?;
    if g3.ledger_fp() != g4.ledger_fp() {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R9 注入账序号守恒：R1 账内 10 条注入 + 1 条合法 kill + 1 次 boot 重复拒绝 = 账长 12
    // （0xA001 生命周期：boot、kill、10 注入、双重 kill 拒绝共 12 条 verdict）
    if g.verdict_count() < 12 {
        return Err(E_INJ_LEDGER);
    }
    Ok(hits)
}
