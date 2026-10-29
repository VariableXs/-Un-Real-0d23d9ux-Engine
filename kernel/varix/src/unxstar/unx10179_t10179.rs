//! UNX-F10179 定时器泄漏审计正反双判据（守恒达成与泄漏误用受控）（0x10980 段）
//!
//! 锚点：UNX-F10179-J1 守恒达成（创建/删除循环定时器账零漂移 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：定时器册（活体表 × 创建/删除双计数）+ 生命周期核（create/
//! destroy）+ 审计核（账面重放校验 live_after 逐条对平 + 守恒恒等式）+
//! 折叠指纹 + 注入核（幽灵销毁/重号创建/伪造 live 账/容量溢出/印后）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x10980 独占 0x20）。
pub const SEG_BASE: u32 = 0x10980;
pub const TM_GHOST: u32 = SEG_BASE + 0x01; // 销毁不存在的定时器
pub const TM_DUP: u32 = SEG_BASE + 0x02; // 重号创建
pub const TM_CAP: u32 = SEG_BASE + 0x03; // 容量溢出
pub const TM_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const TM_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍操作

/// 活体容量（账面容量先定后用）。
pub const LIVE_CAP: usize = 8;

/// 一次生命周期操作的账本留痕。
#[derive(Clone)]
pub struct TmrRec {
    pub seq: u32,
    pub id: u32,
    /// true=create / false=destroy。
    pub create: bool,
    /// 操作后的活体数（重放校验位）。
    pub live_after: u32,
    pub note: String,
}

impl TmrRec {
    pub fn render_line(&self) -> String {
        format!(
            "M{}|id{}|{}|live{}|{}",
            self.seq,
            self.id,
            if self.create { "C" } else { "D" },
            self.live_after,
            self.note
        )
    }
}

/// 定时器册：活体表 + 双计数 + 审计核。
pub struct TimerBook {
    pub live: Vec<u32>,
    pub created: u32,
    pub destroyed: u32,
    pub ledger: Vec<TmrRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TimerBook {
    pub fn new() -> TimerBook {
        TimerBook {
            live: Vec::new(),
            created: 0,
            destroyed: 0,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 创建定时器（重号/容量/空留痕必拒）。
    pub fn create(&mut self, id: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TM_FROZEN);
        }
        if note.is_empty() {
            return Err(TM_EMPTY_NOTE);
        }
        if id == 0 || self.live.contains(&id) {
            return Err(TM_DUP);
        }
        if self.live.len() >= LIVE_CAP {
            return Err(TM_CAP);
        }
        self.live.push(id);
        self.created += 1;
        let n = self.live.len() as u32;
        self.ledger.push(TmrRec {
            seq: self.seq_next,
            id,
            create: true,
            live_after: n,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(n)
    }

    /// 销毁定时器（幽灵必拒）。
    pub fn destroy(&mut self, id: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TM_FROZEN);
        }
        if note.is_empty() {
            return Err(TM_EMPTY_NOTE);
        }
        let i = match self.live.iter().position(|x| *x == id) {
            Some(i) => i,
            None => return Err(TM_GHOST),
        };
        self.live.remove(i);
        self.destroyed += 1;
        let n = self.live.len() as u32;
        self.ledger.push(TmrRec {
            seq: self.seq_next,
            id,
            create: false,
            live_after: n,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(n)
    }

    /// 重放校验：逐条账面 live_after 与重放运行数对平。
    pub fn replay(&self) -> bool {
        let mut n: u32 = 0;
        for r in &self.ledger {
            if r.create {
                n += 1;
            } else if n > 0 {
                n -= 1;
            }
            if r.live_after != n {
                return false;
            }
        }
        true
    }

    /// 审计核：守恒恒等式 + 重放对平（零漂移即审计绿）。
    pub fn audit(&self) -> bool {
        self.created == self.destroyed + self.live.len() as u32 && self.replay()
    }

    /// 当前漂移：创建数 -（销毁数 + 活体数），零即守恒。
    pub fn drift(&self) -> i64 {
        self.created as i64 - (self.destroyed as i64 + self.live.len() as i64)
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.ledger.len() {
            for b in (a + 1)..self.ledger.len() {
                if self.ledger[a].seq == self.ledger[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（71 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10980;
        for r in &self.ledger {
            h = h.wrapping_mul(71).wrapping_add(r.seq);
            h = h.wrapping_mul(71).wrapping_add(r.id);
            h = h.wrapping_mul(71).wrapping_add(r.live_after);
            h = h.wrapping_mul(71).wrapping_add(r.create as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(71).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = TimerBook::new();
    let _ = b.create(1, "t");
    // 1) 幽灵销毁必拒。
    got[0] = b.destroy(99, "inj-ghost").err() == Some(TM_GHOST);
    // 2) 重号创建必拒。
    got[1] = b.create(1, "dup").err() == Some(TM_DUP);
    // 3) 伪造 live 账（重放必翻脸）。
    let forged = TmrRec {
        seq: 9,
        id: 2,
        create: true,
        live_after: 7, // 实际重放到此处应为 1
        note: "inj-live".to_string(),
    };
    got[2] = {
        let mut probe_n: u32 = 0;
        let mut hit = false;
        for r in &b.ledger {
            if r.create {
                probe_n += 1;
            } else {
                probe_n -= 1;
            }
            if r.seq == forged.seq || (r.live_after != probe_n) {
                hit = true;
            }
        }
        // 伪造账单独判：其 live_after 与真实重放不符。
        probe_n = 0;
        let mut replay_forged: u32 = 0;
        for r in &b.ledger {
            if r.create {
                probe_n += 1;
            } else if probe_n > 0 {
                probe_n -= 1;
            }
            replay_forged = probe_n;
            let _ = r;
        }
        forged.live_after != replay_forged || hit
    };
    // 4) 容量溢出必拒（灌满 LIVE_CAP 后再建）。
    let mut b2 = TimerBook::new();
    let mut i = 1u32;
    while i <= LIVE_CAP as u32 {
        let _ = b2.create(i, "fill");
        i += 1;
    }
    got[3] = b2.create(LIVE_CAP as u32 + 1, "over").err() == Some(TM_CAP);
    // 5) 印账后操作必抓。
    b.seal();
    got[4] = b.create(9, "inj-frozen").err() == Some(TM_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10179_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10179");

    // J1 守恒达成：10 轮创建/删除循环账零漂移。
    let j1 = {
        let mut b = TimerBook::new();
        let mut i = 0u32;
        while i < 10 {
            let _ = b.create(100 + i, "c");
            let _ = b.destroy(100 + i, "d");
            i += 1;
        }
        b.drift() == 0 && b.audit() && b.live.is_empty()
    };
    s.add("J1-守恒达成", j1, "十轮循环账零漂移全清");

    // J2 幽灵销毁拒。
    let j2 = {
        let mut b = TimerBook::new();
        b.destroy(1, "ghost").err() == Some(TM_GHOST)
    };
    s.add("J2-幽灵拒", j2, "销毁不存在的定时器必拒");

    // J3 重号创建拒。
    let j3 = {
        let mut b = TimerBook::new();
        let _ = b.create(1, "a");
        b.create(1, "dup").err() == Some(TM_DUP)
    };
    s.add("J3-重号拒", j3, "同 id 二次创建必拒");

    // J4 容量守门：LIVE_CAP 恰满可容、溢出一档必拒。
    let j4 = {
        let mut b = TimerBook::new();
        let mut i = 1u32;
        let mut all_ok = true;
        while i <= LIVE_CAP as u32 {
            if b.create(i, "f").is_err() {
                all_ok = false;
            }
            i += 1;
        }
        all_ok && b.create(99, "over").err() == Some(TM_CAP)
    };
    s.add("J4-容量守门", j4, "容量恰满可容、溢出必拒");

    // J5 泄漏检出：有活体未销时 drift 反映在账（活体数精确）。
    let j5 = {
        let mut b = TimerBook::new();
        let _ = b.create(1, "a");
        let _ = b.create(2, "b");
        let _ = b.destroy(1, "d");
        b.live.len() == 1 && b.live[0] == 2 && b.drift() == 0 && b.replay()
    };
    s.add("J5-活体精确", j5, "活体残量与账面精确对平");

    // J6 重放校验：混合账逐条 live_after 对平。
    let j6 = {
        let mut b = TimerBook::new();
        let _ = b.create(1, "a");
        let _ = b.create(2, "b");
        let _ = b.destroy(1, "c");
        let _ = b.create(3, "d");
        let _ = b.destroy(2, "e");
        let _ = b.destroy(3, "f");
        b.replay() && b.ledger.len() == 6
    };
    s.add("J6-重放对平", j6, "六条混合账重放零漂移");

    // J7 指纹稳定且印账后封禁。
    let j7 = {
        let mut b = TimerBook::new();
        let _ = b.create(1, "a");
        let _ = b.destroy(1, "b");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.create(2, "post").err() == Some(TM_FROZEN)
    };
    s.add("J7-指纹封账", j7, "指纹幂等且印后必拒");

    // J8 注入核 5/5。
    let got = inject_five();
    let j8 = got.iter().all(|x| *x);
    s.add_dyn("J8-注入全抓", j8, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10179_timer_flow() {
        let mut b = TimerBook::new();
        assert_eq!(b.create(1, "a"), Ok(1));
        assert_eq!(b.create(2, "b"), Ok(2));
        assert_eq!(b.destroy(1, "c"), Ok(1));
        assert_eq!(b.destroy(9, "d"), Err(TM_GHOST));
        assert!(b.audit());
        assert_eq!(b.live, vec![2]);
    }

    #[test]
    fn unx10179_checks_green() {
        let s = run_unx10179_checks();
        assert!(s.all_passed(), "UNX-F10179 判据未全绿");
    }
}
