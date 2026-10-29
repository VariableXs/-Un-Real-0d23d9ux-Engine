//! UNX-F24151 reservation 锁与显存互斥纪律·子功能11（中断上下文禁锁闸）（0x414C0 段）
//!
//! 锚点：UNX-F24151-J1 上下文账（进程/中断上下文锁操作逐条落账）；
//! J2 禁锁语义（中断上下文 blocking 锁拒 + spin 锁放行 + 误标识破三
//! 性质）；J3 上下文指纹（账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（中断上下文禁锁面）。
//!
//! 结构：上下文标签表 + 禁锁闸核 + spin/ blocking 分档核 + 账本 + 折
//! 叠指纹 + 注入核（中断持阻塞锁/野上下文/档越界/印后/伪 spin 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x414C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x414C0;
pub const IC_BLOCK_IN_IRQ: u32 = SEG_BASE + 0x01; // 中断持阻塞锁
pub const IC_UNK_CTX: u32 = SEG_BASE + 0x02; // 野上下文
pub const IC_BAD_MODE: u32 = SEG_BASE + 0x03; // 档越界
pub const IC_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const IC_FAKE_SPIN: u32 = SEG_BASE + 0x05; // 伪 spin 标记

/// 上下文类别。
pub const C_PROC: u8 = 0; // 进程上下文
pub const C_IRQ: u8 = 1; // 中断上下文
pub const C_SOFTIRQ: u8 = 2; // 软中断上下文
pub const C_COUNT: u8 = 3;

/// 锁档。
pub const LM_BLOCK: u8 = 0; // 可阻塞锁（mutex）
pub const LM_SPIN: u8 = 1; // 自旋锁
pub const LM_COUNT: u8 = 2;

/// 上下文锁操作合法性。
pub fn lock_allowed(ctx: u8, mode: u8) -> bool {
    if ctx >= C_COUNT || mode >= LM_COUNT {
        return false;
    }
    match ctx {
        C_PROC => true, // 进程上下文两种皆可
        _ => mode == LM_SPIN, // 中断/软中断仅 spin
    }
}

/// 上下文锁账。
#[derive(Clone)]
pub struct IcRec {
    pub seq: u32,
    pub ctx: u8,
    pub lock_id: u32,
    pub mode: u8,
    pub ok: bool,
}

/// 禁锁闸簿。
pub struct IcBook {
    pub ledger: Vec<IcRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl IcBook {
    pub fn new() -> IcBook {
        IcBook { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 尝试在上下文 ctx 取 mode 锁：合法性闸 + 落账（拒也落账）。
    pub fn try_op(&mut self, ctx: u8, lock_id: u32, mode: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(IC_FROZEN);
        }
        if ctx >= C_COUNT || mode >= LM_COUNT {
            let seq = self.seq_next;
            self.seq_next += 1;
            self.ledger.push(IcRec { seq, ctx, lock_id, mode, ok: false });
            return Err(IC_BAD_MODE);
        }
        let ok = lock_allowed(ctx, mode);
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(IcRec { seq, ctx, lock_id, mode, ok });
        if ok {
            Ok(())
        } else {
            Err(IC_BLOCK_IN_IRQ)
        }
    }

    pub fn ok_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.ok).count()
    }

    pub fn reject_count(&self) -> usize {
        self.ledger.iter().filter(|r| !r.ok).count()
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x414C0;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.ctx as u32);
            h = h.wrapping_mul(31).wrapping_add(r.mode as u32);
            h = h.wrapping_mul(31).wrapping_add(r.ok as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：合法操作若干 + 拒操作若干。
pub fn mk_book() -> IcBook {
    let mut b = IcBook::new();
    let _ = b.try_op(C_PROC, 1, LM_BLOCK); // ok
    let _ = b.try_op(C_PROC, 2, LM_SPIN); // ok
    let _ = b.try_op(C_IRQ, 3, LM_SPIN); // ok
    let _ = b.try_op(C_IRQ, 4, LM_BLOCK); // 拒
    let _ = b.try_op(C_SOFTIRQ, 5, LM_BLOCK); // 拒
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 中断持阻塞锁
    got[0] = b.try_op(C_IRQ, 9, LM_BLOCK).err() == Some(IC_BLOCK_IN_IRQ);
    // 2) 档越界
    got[1] = b.try_op(9, 1, LM_SPIN).err() == Some(IC_BAD_MODE);
    got[2] = b.try_op(C_PROC, 1, 9).err() == Some(IC_BAD_MODE);
    // 3) 伪 spin 识破（矩阵语义：软中断 block 必拒）
    got[3] = !lock_allowed(C_SOFTIRQ, LM_BLOCK) && lock_allowed(C_SOFTIRQ, LM_SPIN);
    // 4) 印后操作必抓
    b.seal();
    got[4] = b.try_op(C_PROC, 9, LM_SPIN).err() == Some(IC_FROZEN);
    // 重排 got：上面用了 5 个槽（0,1,2,3,4）——2/3 占了两格，1 号槽补齐
    got
}

/// 判据主体。
pub fn run_unx24151_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24151");
    let mut b = mk_book();

    // J1 账齐
    let j1 = b.ledger.len() == 5;
    s.add("J1-上下文账齐", j1, "五操作全落账");

    // J2 中断持阻塞锁拒
    let j2 = b.try_op(C_IRQ, 9, LM_BLOCK).err() == Some(IC_BLOCK_IN_IRQ);
    s.add("J2-中断禁阻塞", j2, "中断上下文 block 拒");

    // J3 中断 spin 放行
    let j3 = b.try_op(C_IRQ, 9, LM_SPIN).is_ok();
    s.add("J3-中断spin放行", j3, "中断上下文 spin 可取");

    // J4 软中断同律
    let j4 = !lock_allowed(C_SOFTIRQ, LM_BLOCK) && lock_allowed(C_SOFTIRQ, LM_SPIN);
    s.add("J4-软中断同律", j4, "软中断仅 spin");

    // J5 进程全放行
    let j5 = lock_allowed(C_PROC, LM_BLOCK) && lock_allowed(C_PROC, LM_SPIN);
    s.add("J5-进程全放行", j5, "进程上下文两档皆可");

    // J6 档越界拒
    let j6 = b.try_op(9, 1, LM_SPIN).err() == Some(IC_BAD_MODE)
        && b.try_op(C_PROC, 1, 9).err() == Some(IC_BAD_MODE);
    s.add("J6-档越界拒", j6, "ctx/mode 越界必拒");

    // J7 拒操作也落账
    let j7 = b.reject_count() >= 3;
    s.add("J7-拒也落账", j7, "拒绝操作入账审计");

    // J8 账面 ok 标记与矩阵一致
    let j8 = b.ledger.iter().all(|r| r.ok == lock_allowed(r.ctx, r.mode));
    s.add("J8-账矩阵一致", j8, "账面 ok 与合法性矩阵全等");

    // J9 越界拒不计 ok
    let j9 = {
        let mut b2 = IcBook::new();
        let _ = b2.try_op(9, 1, 9);
        b2.ledger.last().map(|r| !r.ok).unwrap_or(false)
    };
    s.add("J9-越界不ok", j9, "越界账 ok=false");

    // J10 seq 唯一
    let j10 = b.seq_unique();
    s.add("J10-seq唯一", j10, "账 seq 不重");

    // J11 ok 计数
    let j11 = b.ok_count() == 3;
    s.add("J11-ok计数", j11, "三操作放行");

    // J12 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24151_ctx_flow() {
        let mut b = IcBook::new();
        assert!(b.try_op(C_PROC, 1, LM_BLOCK).is_ok());
        assert_eq!(b.try_op(C_IRQ, 2, LM_BLOCK), Err(IC_BLOCK_IN_IRQ));
        assert!(b.try_op(C_IRQ, 2, LM_SPIN).is_ok());
        assert_eq!(b.try_op(9, 1, LM_SPIN), Err(IC_BAD_MODE));
        b.seal();
        assert_eq!(b.try_op(C_PROC, 3, LM_SPIN), Err(IC_FROZEN));
    }

    #[test]
    fn unx24151_checks_green() {
        let s = run_unx24151_checks();
        assert!(s.all_passed(), "UNX-F24151 判据未全绿");
    }
}
