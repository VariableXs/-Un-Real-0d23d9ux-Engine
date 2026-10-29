//! UNX-F48529 · E·窗口销毁期输入（正：窗口关停瞬态 commit 丢弃入账）
//!
//! 判据原文：UNX-F48529-J1 —— 窗口三态（Alive→Dying→Gone）commit 语义
//! 冻结：Alive 期 commit 全上屏；Dying 期 commit 拒绝上屏但逐条入丢弃
//! 账本（丢弃也留痕，账本含原文指纹与时刻）；Gone 期 commit 直接拒且
//! 不入账（终态隔离）；三态行为互异零混淆；丢弃账本指纹稳定可重演；
//! 状态机不允许跳跃；同序列重放账本全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xEDE0;

pub const E_STATE_JUMP: u16 = SEG_BASE + 1;
pub const E_ALIVE_DROP: u16 = SEG_BASE + 2;
pub const E_DYING_COMMIT: u16 = SEG_BASE + 3;
pub const E_GONE_LEDGER: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 6;

/// 窗口状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinState {
    Alive,
    Dying,
    Gone,
}

/// commit 载荷。
#[derive(Clone, PartialEq, Eq)]
pub struct Commit {
    pub id: u32,
    pub text_fp: u64,
    pub tick: u64,
}

impl Commit {
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.id as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.text_fp;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.tick;
        h = h.wrapping_mul(0x100000001b3);
        h
    }
}

/// commit 结局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommitFate {
    /// Alive：上屏。
    Shown,
    /// Dying：拒上屏但入账。
    DroppedLogged,
    /// Gone：直接拒。
    Rejected,
}

/// 窗口 commit 处理器。
pub struct WinCommitGate {
    state: WinState,
    pub shown: Vec<Commit>,
    pub drop_ledger: Vec<Commit>,
    pub rejected_n: u64,
}

impl WinCommitGate {
    pub fn new() -> WinCommitGate {
        WinCommitGate {
            state: WinState::Alive,
            shown: Vec::new(),
            drop_ledger: Vec::new(),
            rejected_n: 0,
        }
    }

    pub fn state(&self) -> WinState {
        self.state
    }

    /// 单步推进：Alive→Dying→Gone，不允许回退或跳跃。
    pub fn advance(&mut self) -> Result<WinState, u16> {
        self.state = match self.state {
            WinState::Alive => WinState::Dying,
            WinState::Dying => WinState::Gone,
            WinState::Gone => return Err(E_STATE_JUMP),
        };
        Ok(self.state)
    }

    /// commit 分派：三态三命运。
    pub fn commit(&mut self, c: Commit) -> Result<CommitFate, u16> {
        let fate = match self.state {
            WinState::Alive => {
                self.shown.push(c.clone());
                CommitFate::Shown
            }
            WinState::Dying => {
                self.drop_ledger.push(c.clone());
                CommitFate::DroppedLogged
            }
            WinState::Gone => {
                self.rejected_n += 1;
                CommitFate::Rejected
            }
        };
        Ok(fate)
    }

    /// 丢弃账本指纹（时间序 FNV 链）。
    pub fn ledger_fp(&self) -> u64 {
        let mut h: u64 = 0x9e3779b97f4a7c15;
        h ^= self.drop_ledger.len() as u64;
        h = h.wrapping_mul(0x100000001b3);
        for c in self.drop_ledger.iter() {
            h ^= c.fp();
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 审计：三账互斥守恒。
    pub fn audit(&self, total: u64) -> Result<(), u16> {
        let s = self.shown.len() as u64;
        let d = self.drop_ledger.len() as u64;
        if s + d + self.rejected_n != total {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// R1 Alive 全上屏。
pub fn r1_alive_shown() -> Result<(), u16> {
    let mut g = WinCommitGate::new();
    for i in 0..5u32 {
        let f = g.commit(Commit { id: i, text_fp: 100 + i as u64, tick: i as u64 })?;
        if f != CommitFate::Shown {
            return Err(E_ALIVE_DROP);
        }
    }
    if g.shown.len() != 5 || g.drop_ledger.len() != 0 || g.rejected_n != 0 {
        return Err(E_LEDGER_BAD);
    }
    g.audit(5)
}

/// R2 Dying 丢弃入账：commit 全拒上屏、全入账本。
pub fn r2_dying_dropped_logged() -> Result<(), u16> {
    let mut g = WinCommitGate::new();
    g.advance()?;
    for i in 0..4u32 {
        let f = g.commit(Commit { id: i, text_fp: 200 + i as u64, tick: 10 + i as u64 })?;
        if f != CommitFate::DroppedLogged {
            return Err(E_DYING_COMMIT);
        }
    }
    if g.shown.len() != 0 {
        return Err(E_DYING_COMMIT);
    }
    if g.drop_ledger.len() != 4 {
        return Err(E_LEDGER_BAD);
    }
    g.audit(4)
}

/// R3 Gone 直接拒：不入账本。
pub fn r3_gone_rejected() -> Result<(), u16> {
    let mut g = WinCommitGate::new();
    g.advance()?;
    g.advance()?;
    for i in 0..3u32 {
        let f = g.commit(Commit { id: i, text_fp: 1, tick: i as u64 })?;
        if f != CommitFate::Rejected {
            return Err(E_GONE_LEDGER);
        }
    }
    if g.drop_ledger.len() != 0 || g.shown.len() != 0 {
        return Err(E_GONE_LEDGER);
    }
    if g.rejected_n != 3 {
        return Err(E_LEDGER_BAD);
    }
    g.audit(3)
}

/// R4 三态互异：同 commit 三态三命运。
pub fn r4_three_fates() -> Result<(), u16> {
    let c0 = Commit { id: 7, text_fp: 42, tick: 0 };
    let mut g = WinCommitGate::new();
    let f1 = g.commit(c0.clone())?;
    g.advance()?;
    let f2 = g.commit(c0.clone())?;
    g.advance()?;
    let f3 = g.commit(c0.clone())?;
    if f1 == f2 || f2 == f3 || f1 == f3 {
        return Err(E_NONDET);
    }
    Ok(())
}

/// R5 状态跳跃拒收：Gone 上 advance 报错。
pub fn r5_jump_reject() -> Result<(), u16> {
    let mut g = WinCommitGate::new();
    g.advance()?;
    g.advance()?;
    match g.advance() {
        Err(E_STATE_JUMP) => {}
        _ => return Err(E_STATE_JUMP),
    }
    Ok(())
}

/// R6 账本指纹稳定：同序列两轮指纹全等且非零。
pub fn r6_ledger_fp_stable() -> Result<(), u16> {
    let run = || -> u64 {
        let mut g = WinCommitGate::new();
        match g.advance() {
            Ok(_) => {}
            Err(_) => return 0,
        }
        for i in 0..6u32 {
            let _ = g.commit(Commit { id: i, text_fp: 500 + i as u64, tick: i as u64 });
        }
        g.ledger_fp()
    };
    let a = run();
    let b = run();
    if a != b || a == 0 {
        return Err(E_NONDET);
    }
    Ok(())
}

/// R7 账本保序：丢弃账本顺序 == 提交序。
pub fn r7_ledger_order() -> Result<(), u16> {
    let mut g = WinCommitGate::new();
    g.advance()?;
    for i in 0..8u32 {
        let _ = g.commit(Commit { id: i, text_fp: i as u64, tick: i as u64 })?;
    }
    for i in 0..8usize {
        if g.drop_ledger[i].id != i as u32 {
            return Err(E_NONDET);
        }
    }
    Ok(())
}

/// R8 跨态混合流：2 上屏 + 3 丢弃 + 2 拒，三账精确分割。
pub fn r8_mixed_stream() -> Result<(), u16> {
    let mut g = WinCommitGate::new();
    let mut id: u32 = 0;
    for _ in 0..2 {
        let _ = g.commit(Commit { id, text_fp: id as u64, tick: id as u64 })?;
        id += 1;
    }
    g.advance()?;
    for _ in 0..3 {
        let _ = g.commit(Commit { id, text_fp: id as u64, tick: id as u64 })?;
        id += 1;
    }
    g.advance()?;
    for _ in 0..2 {
        let _ = g.commit(Commit { id, text_fp: id as u64, tick: id as u64 })?;
        id += 1;
    }
    g.audit(7)?;
    if g.shown.len() != 2 || g.drop_ledger.len() != 3 || g.rejected_n != 2 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R9 fuzz 全程流：随机 40 commit + 定点两次迁移，账守恒且命运与态对应。
pub fn r9_fuzz_stream() -> Result<(), u16> {
    let mut g_lcg = Lcg::new(0x48529_0001);
    let mut g = WinCommitGate::new();
    let mut total: u64 = 0;
    for i in 0..40u32 {
        if i == 15 {
            g.advance()?;
        }
        if i == 30 {
            g.advance()?;
        }
        let fate = g.commit(Commit { id: i, text_fp: g_lcg.next(), tick: i as u64 })?;
        let expect = match g.state() {
            WinState::Alive => CommitFate::Shown,
            WinState::Dying => CommitFate::DroppedLogged,
            WinState::Gone => CommitFate::Rejected,
        };
        if fate != expect {
            return Err(E_NONDET);
        }
        total += 1;
    }
    g.audit(total)
}

/// R10 确定性重放：同种子两轮三账全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(usize, usize, u64), u16> {
        let mut l = Lcg::new(seed);
        let mut g = WinCommitGate::new();
        for i in 0..25u32 {
            if i == 8 {
                g.advance()?;
            }
            if i == 18 {
                g.advance()?;
            }
            let _ = g.commit(Commit { id: i, text_fp: l.next(), tick: i as u64 })?;
        }
        Ok((g.shown.len(), g.drop_ledger.len(), g.rejected_n))
    };
    let a = run(0x48529_0002)?;
    let b = run(0x48529_0002)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_alive_shown()?;
    hits += 1;
    r2_dying_dropped_logged()?;
    hits += 1;
    r3_gone_rejected()?;
    hits += 1;
    r4_three_fates()?;
    hits += 1;
    r5_jump_reject()?;
    hits += 1;
    r6_ledger_fp_stable()?;
    hits += 1;
    r7_ledger_order()?;
    hits += 1;
    r8_mixed_stream()?;
    hits += 1;
    r9_fuzz_stream()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
