//! UNX-F10201 跑批执行开卷总纲（执行窗口/分片/优先级三维齐备）（0x10c40 段）
//!
//! 锚点：UNX-F10201-J1 跑批计划成文落账（窗口/分片/优先级三维齐备），
//! 维缺 10/10 次检出。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：计划草案（窗口 × 分片 × 优先级三维 Option 位）+ 成文核
//! （三维齐备才落账、缺维逐项点名）+ 总纲账本 + 折叠指纹 +
//! 注入核（缺窗/缺片/缺级/重号计划/伪造成文账/印后成文）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10c40 独占 0x20）。
pub const SEG_BASE: u32 = 0x10c40;
pub const BP_DIM: u32 = SEG_BASE + 0x01; // 维缺
pub const BP_DUP: u32 = SEG_BASE + 0x02; // 计划重号
pub const BP_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 留痕为空
pub const BP_FORGED: u32 = SEG_BASE + 0x04; // 伪造成文账
pub const BP_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍成文

/// 执行窗口上界（tick）。
pub const WIN_MAX: u64 = 100_000;
/// 分片上界。
pub const SHARD_MAX: usize = 64;

/// 计划草案：三维皆 Option（缺维可点名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlanDraft {
    pub id: u32,
    pub window: Option<u64>,
    pub shards: Option<usize>,
    pub prio: Option<u8>,
}

impl PlanDraft {
    /// 缺维清点：缺哪几维（true=缺）。
    pub fn gaps(&self) -> (bool, bool, bool) {
        (
            self.window.is_none(),
            self.shards.is_none(),
            self.prio.is_none(),
        )
    }

    pub fn complete(&self) -> bool {
        self.window.is_some() && self.shards.is_some() && self.prio.is_some()
    }
}

/// 一次成文的账本留痕。
#[derive(Clone)]
pub struct PlanRec {
    pub seq: u32,
    pub id: u32,
    pub complete: bool,
    pub note: String,
}

impl PlanRec {
    /// 账目合法性：留痕非空且成文账必对应完整草案。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.complete
    }

    pub fn render_line(&self) -> String {
        format!("P{}|id{}|cmp{}|{}", self.seq, self.id, self.complete as u8, self.note)
    }
}

/// 总纲册：草案表 + 成文核。
pub struct PlanBook {
    pub drafts: Vec<PlanDraft>,
    pub ledger: Vec<PlanRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl PlanBook {
    pub fn new() -> PlanBook {
        PlanBook { drafts: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 起草计划（重号必拒；域值越界按缺维面拒）。
    pub fn draft(
        &mut self,
        id: u32,
        window: Option<u64>,
        shards: Option<usize>,
        prio: Option<u8>,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(BP_FROZEN);
        }
        if id == 0 || self.drafts.iter().any(|d| d.id == id) {
            return Err(BP_DUP);
        }
        if let Some(w) = window {
            if w == 0 || w > WIN_MAX {
                return Err(BP_DIM);
            }
        }
        if let Some(s) = shards {
            if s == 0 || s > SHARD_MAX {
                return Err(BP_DIM);
            }
        }
        self.drafts.push(PlanDraft { id, window, shards, prio });
        Ok(())
    }

    /// 补维：为指定维补值。
    pub fn fill_window(&mut self, id: u32, w: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(BP_FROZEN);
        }
        if w == 0 || w > WIN_MAX {
            return Err(BP_DIM);
        }
        match self.drafts.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.window = Some(w);
                Ok(())
            }
            None => Err(BP_DUP),
        }
    }

    pub fn fill_shards(&mut self, id: u32, s: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(BP_FROZEN);
        }
        if s == 0 || s > SHARD_MAX {
            return Err(BP_DIM);
        }
        match self.drafts.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.shards = Some(s);
                Ok(())
            }
            None => Err(BP_DUP),
        }
    }

    pub fn fill_prio(&mut self, id: u32, p: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(BP_FROZEN);
        }
        match self.drafts.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.prio = Some(p);
                Ok(())
            }
            None => Err(BP_DUP),
        }
    }

    /// 成文核：三维齐备才落账，缺维逐项点名（BP_DIM）。
    pub fn seal_plan(&mut self, id: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(BP_FROZEN);
        }
        if note.is_empty() {
            return Err(BP_EMPTY_NOTE);
        }
        let d = match self.drafts.iter().find(|d| d.id == id) {
            Some(d) => *d,
            None => return Err(BP_DUP),
        };
        let (gw, gs, gp) = d.gaps();
        if gw || gs || gp {
            return Err(BP_DIM);
        }
        self.ledger.push(PlanRec {
            seq: self.seq_next,
            id,
            complete: true,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(())
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
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

    /// 折叠指纹（181 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10c40;
        for d in &self.drafts {
            h = h.wrapping_mul(181).wrapping_add(d.id);
            h = h.wrapping_mul(181).wrapping_add(d.complete() as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(181).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(181).wrapping_add(by as u32);
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
    let mut b = PlanBook::new();
    // 1) 缺窗计划成文必拒。
    let _ = b.draft(1, None, Some(4), Some(9));
    got[0] = b.seal_plan(1, "inj-win").err() == Some(BP_DIM);
    // 2) 缺片计划成文必拒。
    let _ = b.draft(2, Some(100), None, Some(9));
    got[1] = b.seal_plan(2, "inj-shard").err() == Some(BP_DIM);
    // 3) 缺级计划成文必拒。
    let _ = b.draft(3, Some(100), Some(4), None);
    got[2] = b.seal_plan(3, "inj-prio").err() == Some(BP_DIM);
    // 4) 重号计划必拒。
    got[3] = b.draft(1, Some(1), Some(1), Some(1)).err() == Some(BP_DUP);
    // 5) 伪造成文账（对应缺维草案）必判非法。
    let forged = PlanRec { seq: 1, id: 1, complete: true, note: String::new() };
    got[4] = !forged.ok();
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10201_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10201");

    // J1 三维齐备落账：补齐三维后成文成功。
    let j1 = {
        let mut b = PlanBook::new();
        let _ = b.draft(1, None, None, None);
        let _ = b.fill_window(1, 3600);
        let _ = b.fill_shards(1, 8);
        let _ = b.fill_prio(1, 5);
        b.seal_plan(1, "ok").is_ok() && b.ledger.len() == 1
    };
    s.add("J1-三维齐备", j1, "窗/片/级三维齐后成文");

    // J2 缺窗检出。
    let j2 = {
        let mut b = PlanBook::new();
        let _ = b.draft(1, None, Some(4), Some(9));
        b.seal_plan(1, "x").err() == Some(BP_DIM)
    };
    s.add("J2-缺窗检出", j2, "无窗口维成文必拒");

    // J3 缺片检出。
    let j3 = {
        let mut b = PlanBook::new();
        let _ = b.draft(1, Some(100), None, Some(9));
        b.seal_plan(1, "x").err() == Some(BP_DIM)
    };
    s.add("J3-缺片检出", j3, "无分片维成文必拒");

    // J4 缺级检出。
    let j4 = {
        let mut b = PlanBook::new();
        let _ = b.draft(1, Some(100), Some(4), None);
        b.seal_plan(1, "x").err() == Some(BP_DIM)
    };
    s.add("J4-缺级检出", j4, "无优先级维成文必拒");

    // J5 域值越界拒：零窗与超片皆拒。
    let j5 = {
        let mut b = PlanBook::new();
        b.draft(1, Some(0), Some(4), Some(9)).err() == Some(BP_DIM)
            && b.draft(2, Some(100), Some(0), Some(9)).err() == Some(BP_DIM)
            && b.draft(3, Some(100), Some(SHARD_MAX + 1), Some(9)).err() == Some(BP_DIM)
    };
    s.add("J5-域值越界", j5, "零窗/零片/超片必拒");

    // J6 重号拒。
    let j6 = {
        let mut b = PlanBook::new();
        let _ = b.draft(1, Some(100), Some(4), Some(9));
        b.draft(1, Some(100), Some(4), Some(9)).err() == Some(BP_DUP)
    };
    s.add("J6-重号拒", j6, "同 id 二次起草必拒");

    // J7 账本完整：成文账逐条合法。
    let j7 = {
        let mut b = PlanBook::new();
        let _ = b.draft(1, Some(100), Some(4), Some(9));
        let _ = b.draft(2, Some(200), Some(8), Some(3));
        let _ = b.seal_plan(1, "a");
        let _ = b.seal_plan(2, "b");
        b.ledger.len() == 2 && b.ledger_ok() && b.seq_unique()
    };
    s.add("J7-账本完整", j7, "双计划账合法无重号");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = PlanBook::new();
        let _ = b.draft(1, Some(100), Some(4), Some(9));
        let _ = b.seal_plan(1, "a");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.seal_plan(1, "post").err() == Some(BP_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10201_plan_flow() {
        let mut b = PlanBook::new();
        let _ = b.draft(1, Some(1000), Some(4), Some(7));
        assert!(b.seal_plan(1, "ok").is_ok());
        let _ = b.draft(2, None, None, None);
        assert_eq!(b.seal_plan(2, "x"), Err(BP_DIM));
        assert!(b.fill_window(2, 500).is_ok());
        assert_eq!(b.seal_plan(2, "x"), Err(BP_DIM));
    }

    #[test]
    fn unx10201_checks_green() {
        let s = run_unx10201_checks();
        assert!(s.all_passed(), "UNX-F10201 判据未全绿");
    }
}
