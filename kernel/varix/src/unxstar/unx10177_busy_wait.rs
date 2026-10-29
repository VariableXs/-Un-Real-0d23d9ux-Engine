//! UNX-F10177 busy-wait 让渡正反双判据（让渡点达成与零让渡误用受控）（0x10940 段）
//!
//! 锚点：UNX-F10177-J1 让渡达成（忙等路径让渡点生效 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：忙等环（总迭代 × 让渡间隔）+ 让渡点核（每 GAP 次迭代一个
//! 让渡点，让渡数 = total/gap）+ 审计核（应让未让的零让渡环必标）+
//! 忙等账本 + 折叠指纹 + 注入核（零间隔/零迭代/零让渡伪造账/
//! 超额让渡/印后跑环）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10940 独占 0x20）。
pub const SEG_BASE: u32 = 0x10940;
pub const BW_GAP: u32 = SEG_BASE + 0x01; // 零间隔：让渡点无从发生
pub const BW_ZERO: u32 = SEG_BASE + 0x02; // 零迭代空环
pub const BW_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 留痕为空
pub const BW_OVER: u32 = SEG_BASE + 0x04; // 让渡数超迭代数（不可能）
pub const BW_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍跑环

/// 单次忙等环的账本留痕。
#[derive(Clone)]
pub struct LoopRec {
    pub seq: u32,
    pub total: u32,
    pub gap: u32,
    pub yields: u32,
    pub note: String,
}

impl LoopRec {
    /// 让渡点期望数：total / gap（整除截断）。
    pub fn expect_yields(&self) -> u32 {
        if self.gap == 0 {
            0
        } else {
            self.total / self.gap
        }
    }

    /// 账目合法性：留痕非空；总迭代/间隔为正；让渡数不超迭代数；
    /// 让渡数恒等于期望（伪造账/超额均判非法）。
    pub fn ok(&self) -> bool {
        if self.note.is_empty() || self.total == 0 || self.gap == 0 {
            return false;
        }
        if self.yields > self.total {
            return false;
        }
        self.yields == self.expect_yields()
    }

    pub fn render_line(&self) -> String {
        format!(
            "B{}|n{}|g{}|y{}|{}",
            self.seq, self.total, self.gap, self.yields, self.note
        )
    }
}

/// 忙等审计册：环账 + 审计核。
pub struct BusyLedger {
    pub loops: Vec<LoopRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl BusyLedger {
    pub fn new() -> BusyLedger {
        BusyLedger { loops: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 忙等环执行核：按 gap 布让渡点，回实际让渡数。
    pub fn run_busy(&mut self, total: u32, gap: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(BW_FROZEN);
        }
        if note.is_empty() {
            return Err(BW_EMPTY_NOTE);
        }
        if gap == 0 {
            return Err(BW_GAP);
        }
        if total == 0 {
            return Err(BW_ZERO);
        }
        // 让渡点布设：迭代轴上每 gap 个位置一个点。
        let yields = total / gap;
        self.loops.push(LoopRec {
            seq: self.seq_next,
            total,
            gap,
            yields,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(yields)
    }

    /// 审计核：全部环账合法，且零让渡债务已被补足（重放账本：零让渡环
    /// 各记 1 笔应让未让债务，其后每环的实际让渡逐笔冲抵，终态债务为零才绿）。
    pub fn audit(&self) -> bool {
        if !self.loops.iter().all(|r| r.ok()) {
            return false;
        }
        self.audit_deficit() == 0
    }

    /// 总让渡数（跨环汇总）。
    pub fn total_yields(&self) -> u32 {
        self.loops.iter().map(|r| r.yields).sum()
    }

    /// 让渡密度：总让渡 / 总迭代（万分位整型，避浮点）。
    pub fn yield_density_bps(&self) -> u32 {
        let total: u64 = self.loops.iter().map(|r| r.total as u64).sum();
        if total == 0 {
            return 0;
        }
        (self.total_yields() as u64 * 10000 / total) as u32
    }

    /// 最长零让渡嫌疑环：让渡数为 0 的最大环（诊断面）。
    pub fn suspect_loop(&self) -> Option<&LoopRec> {
        let mut best: Option<&LoopRec> = None;
        for r in &self.loops {
            if r.yields == 0 {
                match best {
                    Some(w) if w.total >= r.total => {}
                    _ => best = Some(r),
                }
            }
        }
        best
    }

    /// 审计缺口：应让未让的让渡点总数（重放账本：零让渡环各记 1 笔债务，
    /// 被其后各环的实际让渡逐笔冲抵；补足后审计才能绿）。
    pub fn audit_deficit(&self) -> u32 {
        let mut debt: u32 = 0;
        for r in &self.loops {
            if r.yields == 0 {
                debt += 1;
            } else {
                debt = debt.saturating_sub(r.yields);
            }
        }
        debt
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.loops.len() {
            for b in (a + 1)..self.loops.len() {
                if self.loops[a].seq == self.loops[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（61 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10940;
        for r in &self.loops {
            h = h.wrapping_mul(61).wrapping_add(r.seq);
            h = h.wrapping_mul(61).wrapping_add(r.total);
            h = h.wrapping_mul(61).wrapping_add(r.gap);
            h = h.wrapping_mul(61).wrapping_add(r.yields);
            for by in r.note.bytes() {
                h = h.wrapping_mul(61).wrapping_add(by as u32);
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
    let mut b = BusyLedger::new();
    // 1) 零间隔环必拒（让渡点无从布设）。
    got[0] = b.run_busy(100, 0, "inj-gap").err() == Some(BW_GAP);
    // 2) 零迭代空环必拒。
    got[1] = b.run_busy(0, 10, "inj-zero").err() == Some(BW_ZERO);
    // 3) 零让渡伪造账：total=1000/gap=10 却报 0 让渡必判非法。
    let forged = LoopRec {
        seq: 1,
        total: 1000,
        gap: 10,
        yields: 0,
        note: "inj-no-yield".to_string(),
    };
    got[2] = !forged.ok();
    // 4) 超额让渡账（让渡数 > 迭代数）必判非法。
    let forged2 = LoopRec {
        seq: 2,
        total: 10,
        gap: 10,
        yields: 11,
        note: "inj-over".to_string(),
    };
    got[3] = !forged2.ok();
    // 5) 印账后跑环必抓。
    b.seal();
    got[4] = b.run_busy(10, 5, "inj-frozen").err() == Some(BW_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10177_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10177");

    // J1 让渡点达成：total=100/gap=10 → 10 个让渡点。
    let j1 = {
        let mut b = BusyLedger::new();
        b.run_busy(100, 10, "a") == Ok(10)
    };
    s.add("J1-让渡点达成", j1, "百迭代十间隔得十让渡");

    // J2 截断语义：total=105/gap=10 → 10 点（尾段不足一间隔不布点）。
    let j2 = {
        let mut b = BusyLedger::new();
        b.run_busy(105, 10, "b") == Ok(10)
    };
    s.add("J2-截断语义", j2, "尾段不足一间隔不布点");

    // J3 零让渡受控：环短于间隔时让渡为 0，但审计核必标非法。
    let j3 = {
        let mut b = BusyLedger::new();
        let r = b.run_busy(5, 10, "short");
        // 运行面接受（Ok(0)），审计面必红——分离受控。
        r == Ok(0) && !b.audit()
    };
    s.add("J3-零让渡检出", j3, "应让未让环审计必标");

    // J4 零间隔/零迭代双拒。
    let j4 = {
        let mut b = BusyLedger::new();
        b.run_busy(10, 0, "g").err() == Some(BW_GAP)
            && b.run_busy(0, 10, "z").err() == Some(BW_ZERO)
    };
    s.add("J4-零参双拒", j4, "零间隔与空环必拒");

    // J5 多环混排审计：正常环全绿。
    let j5 = {
        let mut b = BusyLedger::new();
        let _ = b.run_busy(100, 10, "a");
        let _ = b.run_busy(30, 3, "b");
        let _ = b.run_busy(7, 1, "c");
        b.audit() && b.total_yields() == 10 + 10 + 7
    };
    s.add("J5-多环审计", j5, "三环让渡汇总 27 全绿");

    // J6 账本完整：seq 唯一且逐条合法。
    let j6 = {
        let mut b = BusyLedger::new();
        let _ = b.run_busy(50, 5, "a");
        let _ = b.run_busy(50, 50, "b");
        b.loops.len() == 2 && b.seq_unique() && b.loops.iter().all(|r| r.ok())
    };
    s.add("J6-账本完整", j6, "双环账合法无重号");

    // J7 期望守恒：逐环 expect 与实记一致。
    let j7 = {
        let mut b = BusyLedger::new();
        let _ = b.run_busy(99, 9, "a");
        let _ = b.run_busy(100, 25, "b");
        b.loops.iter().all(|r| r.yields == r.expect_yields())
    };
    s.add("J7-期望守恒", j7, "实记让渡与期望逐环对平");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = BusyLedger::new();
        let _ = b.run_busy(20, 10, "a");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.run_busy(20, 10, "post").err() == Some(BW_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // J10 诊断面：让渡密度与嫌疑环导出自洽。
    let j10 = {
        let mut b = BusyLedger::new();
        let _ = b.run_busy(1000, 10, "a");
        let _ = b.run_busy(5, 100, "short");
        // 密度 = 100 / 1005 → 995 bps；嫌疑环是 total=5 的零让渡环。
        b.yield_density_bps() == 995
            && b.suspect_loop().map(|r| r.total) == Some(5)
            && b.audit_deficit() > 0
    };
    s.add("J10-诊断面自洽", j10, "密度 994bps 与嫌疑环 5 自洽");

    // J11 补足收敛：零让渡环按缺口补跑后审计翻绿。
    let j11 = {
        let mut b = BusyLedger::new();
        let _ = b.run_busy(5, 100, "short"); // 审计红
        let deficit = b.audit_deficit();
        // 补救路径：以合法间隔补一个等量环（模拟重排让渡点）。
        let _ = b.run_busy(deficit * 100, 100, "fix");
        b.audit()
    };
    s.add("J11-补足收敛", j11, "按缺口补跑后审计翻绿");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10177_busy_flow() {
        let mut b = BusyLedger::new();
        assert_eq!(b.run_busy(30, 5, "a"), Ok(6));
        assert_eq!(b.run_busy(4, 10, "b"), Ok(0));
        assert!(!b.audit(), "零让渡环必须被审计标红");
        assert_eq!(b.total_yields(), 6);
        assert_eq!(b.run_busy(1, 0, "c"), Err(BW_GAP));
    }

    #[test]
    fn unx10177_checks_green() {
        let s = run_unx10177_checks();
        assert!(s.all_passed(), "UNX-F10177 判据未全绿");
    }

    #[test]
    fn unx10177_density_flow() {
        let mut b = BusyLedger::new();
        let _ = b.run_busy(200, 20, "a");
        let _ = b.run_busy(100, 50, "b");
        assert_eq!(b.total_yields(), 12);
        assert_eq!(b.yield_density_bps(), 400);
        assert!(b.suspect_loop().is_none());
        assert!(b.audit_deficit() == 0 && b.audit());
    }
}
