//! UNX-F48530 · E·窗口死亡竞态注入（反：销毁后投递 ×10 全拒）
//!
//! 判据原文：UNX-F48530-J1 —— 死亡竞态语义：投递命运只由「受理时刻
//! 的窗口状态」决定，销毁（Gone）后到达的投递 ×10 轮全部拒绝（零投机
//! 放行）；销毁前已受理投递集不可变（死亡后不被污染、不被撤销重放）；
//! 代际戳防线：销毁使 epoch+1，携带旧 epoch 的投递一律拒；受理账与拒绝
//! 账互斥守恒；同序列重放全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xEEA0;

pub const E_STALE_ACCEPTED: u16 = SEG_BASE + 1;
pub const E_SET_POLLUTED: u16 = SEG_BASE + 2;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 3;
pub const E_EPOCH_MISS: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_ALIVE_REJECT: u16 = SEG_BASE + 6;

/// 投递事件。
#[derive(Clone, PartialEq, Eq)]
pub struct Delivery {
    pub id: u32,
    pub text_fp: u64,
    pub epoch: u32,
}

/// 竞态窗口。
pub struct RaceWindow {
    pub dead: bool,
    pub epoch: u32,
    pub accepted: Vec<Delivery>,
    pub rejected: u64,
}

impl RaceWindow {
    pub fn new() -> RaceWindow {
        RaceWindow { dead: false, epoch: 1, accepted: Vec::new(), rejected: 0 }
    }

    /// 销毁：epoch+1，已受理集冻结。
    pub fn destroy(&mut self) {
        if !self.dead {
            self.dead = true;
            self.epoch += 1;
        }
    }

    /// 投递：alive 且 epoch 匹配 → 受理；否则拒。
    pub fn deliver(&mut self, d: Delivery) -> Result<bool, u16> {
        if self.dead || d.epoch != self.epoch {
            self.rejected += 1;
            return Ok(false);
        }
        self.accepted.push(d);
        Ok(true)
    }

    /// 受理集冻结审计：destroy 后受理集不变。
    pub fn audit_frozen(&self, expect_len: usize) -> Result<(), u16> {
        if !self.dead {
            return Err(E_EPOCH_MISS);
        }
        if self.accepted.len() != expect_len {
            return Err(E_SET_POLLUTED);
        }
        Ok(())
    }

    /// 受理集内容不可变（指纹比对）。
    pub fn accepted_fp(&self) -> u64 {
        let mut h: u64 = 0x9e3779b97f4a7c15;
        h ^= self.accepted.len() as u64;
        h = h.wrapping_mul(0x100000001b3);
        for d in self.accepted.iter() {
            h ^= d.id as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= d.text_fp;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
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

/// R1 销毁后投递全拒：×10 全拒。
pub fn r1_post_destroy_reject() -> Result<(), u16> {
    let mut w = RaceWindow::new();
    w.destroy();
    for i in 0..10u32 {
        let ok = w.deliver(Delivery { id: i, text_fp: 100 + i as u64, epoch: 1 })?;
        if ok {
            return Err(E_STALE_ACCEPTED);
        }
    }
    if w.rejected != 10 || w.accepted.len() != 0 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R2 epoch 陈旧拒：旧 epoch 投递被拒、新 epoch 放行。
pub fn r2_epoch_gate() -> Result<(), u16> {
    let mut w = RaceWindow::new();
    // alive 时 epoch=1 放行。
    let ok1 = w.deliver(Delivery { id: 1, text_fp: 1, epoch: 1 })?;
    if !ok1 {
        return Err(E_ALIVE_REJECT);
    }
    w.destroy(); // epoch → 2
    // 旧 epoch 1 投递拒。
    let ok2 = w.deliver(Delivery { id: 2, text_fp: 2, epoch: 1 })?;
    if ok2 {
        return Err(E_STALE_ACCEPTED);
    }
    // 新 epoch 2 也拒（窗口已死）。
    let ok3 = w.deliver(Delivery { id: 3, text_fp: 3, epoch: 2 })?;
    if ok3 {
        return Err(E_STALE_ACCEPTED);
    }
    if w.accepted.len() != 1 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R3 受理集冻结：销毁前后受理指纹一致。
pub fn r4_accepted_frozen() -> Result<(), u16> {
    let mut w = RaceWindow::new();
    for i in 0..3u32 {
        w.deliver(Delivery { id: i, text_fp: 10 + i as u64, epoch: 1 })?;
    }
    let fp_before = w.accepted_fp();
    w.destroy();
    for i in 0..5u32 {
        let _ = w.deliver(Delivery { id: 100 + i, text_fp: 999, epoch: 2 })?;
    }
    if w.accepted_fp() != fp_before {
        return Err(E_SET_POLLUTED);
    }
    w.audit_frozen(3)
}

/// R4 次序决定命运：同一投递在 alive 时受理、dead 后同 id 再投拒。
pub fn r4_order_decides() -> Result<(), u16> {
    let mut w = RaceWindow::new();
    let ok = w.deliver(Delivery { id: 7, text_fp: 7, epoch: 1 })?;
    if !ok {
        return Err(E_ALIVE_REJECT);
    }
    w.destroy();
    let ok2 = w.deliver(Delivery { id: 7, text_fp: 7, epoch: 2 })?;
    if ok2 {
        return Err(E_STALE_ACCEPTED);
    }
    Ok(())
}

/// R5 销毁幂等：多次 destroy epoch 只加一次。
pub fn r5_destroy_idempotent() -> Result<(), u16> {
    let mut w = RaceWindow::new();
    w.destroy();
    let e1 = w.epoch;
    w.destroy();
    w.destroy();
    if w.epoch != e1 {
        return Err(E_EPOCH_MISS);
    }
    Ok(())
}

/// R6 拒绝账守恒：alive 拒（epoch 错）+ dead 拒 合计正确。
pub fn r6_reject_ledger() -> Result<(), u16> {
    let mut w = RaceWindow::new();
    // alive 但 epoch 错（伪造未来 epoch）。
    let _ = w.deliver(Delivery { id: 1, text_fp: 0, epoch: 99 })?;
    // alive 且匹配。
    let _ = w.deliver(Delivery { id: 2, text_fp: 0, epoch: 1 })?;
    w.destroy();
    for i in 0..4u32 {
        let _ = w.deliver(Delivery { id: 10 + i, text_fp: 0, epoch: 2 })?;
    }
    if w.rejected != 5 {
        return Err(E_LEDGER_BAD);
    }
    if w.accepted.len() != 1 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R7 ×10 轮竞态重演：同序列十轮受理数/拒绝数全等。
pub fn r7_ten_rounds() -> Result<(), u16> {
    let run = || -> Result<(usize, u64), u16> {
        let mut w = RaceWindow::new();
        for i in 0..3u32 {
            let _ = w.deliver(Delivery { id: i, text_fp: i as u64, epoch: 1 })?;
        }
        w.destroy();
        for i in 0..7u32 {
            let _ = w.deliver(Delivery { id: 50 + i, text_fp: i as u64, epoch: 2 })?;
        }
        Ok((w.accepted.len(), w.rejected))
    };
    let a = run()?;
    if a.0 != 3 || a.1 != 7 {
        return Err(E_LEDGER_BAD);
    }
    for _ in 0..10 {
        if run()? != a {
            return Err(E_NONDET);
        }
    }
    Ok(())
}

/// R8 fuzz 竞态流：随机「投递 or 销毁」40 步，命运恒与状态对应。
pub fn r8_fuzz_race() -> Result<(), u16> {
    let mut g = Lcg::new(0x48530_0001);
    let mut w = RaceWindow::new();
    let mut id: u32 = 0;
    for _ in 0..40u64 {
        let op = g.below(3);
        if op == 0 {
            w.destroy();
        } else {
            let d = Delivery { id, text_fp: g.next(), epoch: w.epoch };
            id += 1;
            let ok = w.deliver(d)?;
            if ok && w.dead {
                return Err(E_STALE_ACCEPTED);
            }
            if !ok && !w.dead {
                return Err(E_ALIVE_REJECT);
            }
        }
    }
    Ok(())
}

/// R9 受理序保序：alive 段受理 id 严格递增。
pub fn r9_accept_order() -> Result<(), u16> {
    let mut w = RaceWindow::new();
    for i in 0..8u32 {
        let ok = w.deliver(Delivery { id: i, text_fp: i as u64, epoch: 1 })?;
        if !ok {
            return Err(E_ALIVE_REJECT);
        }
    }
    for i in 0..8usize {
        if w.accepted[i].id != i as u32 {
            return Err(E_NONDET);
        }
    }
    Ok(())
}

/// R10 确定性重放：同种子两轮全账全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(usize, u64, u32), u16> {
        let mut g = Lcg::new(seed);
        let mut w = RaceWindow::new();
        let mut id: u32 = 0;
        for _ in 0..30u64 {
            let op = g.below(3);
            if op == 0 {
                w.destroy();
            } else {
                let _ = w.deliver(Delivery { id, text_fp: g.next(), epoch: w.epoch })?;
                id += 1;
            }
        }
        Ok((w.accepted.len(), w.rejected, w.epoch))
    };
    let a = run(0x48530_0002)?;
    let b = run(0x48530_0002)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_post_destroy_reject()?;
    hits += 1;
    r2_epoch_gate()?;
    hits += 1;
    r4_accepted_frozen()?;
    hits += 1;
    r4_order_decides()?;
    hits += 1;
    r5_destroy_idempotent()?;
    hits += 1;
    r6_reject_ledger()?;
    hits += 1;
    r7_ten_rounds()?;
    hits += 1;
    r8_fuzz_race()?;
    hits += 1;
    r9_accept_order()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
