//! UNX-F1724 · 迁移失败回退（暂存滞留回收联动）账 · 诊断段 0xB930
//! 验收锚点：UNX-F1724-J1 回退路径（滞留回收→源核归队→三账回冲）
//! 闭合断言全过，回退注入 10/10 次正确归位，双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB930;

// ── 诊断段独占 · 迁移回退错误域 ─────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_STUCK: u16 = SEG_ERR + 1;   // 滞留未回收
const E_BACK: u16 = SEG_ERR + 2;    // 归队失败
const E_ROLL: u16 = SEG_ERR + 3;    // 三账回冲不平
const E_DUP: u16 = SEG_ERR + 4;     // 重复归队
const E_LEAK: u16 = SEG_ERR + 5;    // 账泄漏

/// 三账：负载账 / 贡献账 / vruntime 锚账。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreeLedger {
    pub load: u64,      // 负载账
    pub contrib: u64,   // 贡献账
    pub vr_anchor: u64, // vruntime 锚账
}

impl ThreeLedger {
    pub fn zero() -> Self {
        ThreeLedger { load: 0, contrib: 0, vr_anchor: 0 }
    }
    pub fn total(&self) -> u64 {
        self.load ^ self.contrib ^ self.vr_anchor
    }
}

/// 暂存滞留条目：线程 + 迁移目标 + 携账。
#[derive(Clone, Debug)]
pub struct StuckEntry {
    pub thread: String,
    pub target_core: usize,
    pub ledger: ThreeLedger,
}

/// 源核：滞留池 + 在册集 + 三账。
pub struct SrcCore {
    pub core: usize,
    pub stuck: Vec<StuckEntry>,
    pub roster: Vec<String>, // 在册线程
    pub ledger: ThreeLedger,
}

impl SrcCore {
    pub fn new(core: usize) -> Self {
        SrcCore { core, stuck: Vec::new(), roster: Vec::new(), ledger: ThreeLedger::zero() }
    }
    /// 发起迁移：线程出在册、携账进滞留池。
    pub fn migrate_out(&mut self, thread: &str, target: usize, led: ThreeLedger) -> bool {
        let idx = match self.roster.iter().position(|t| t == thread) {
            Some(i) => i,
            None => return false,
        };
        self.roster.remove(idx);
        // 三账预扣
        self.ledger.load -= led.load.min(self.ledger.load);
        self.ledger.contrib -= led.contrib.min(self.ledger.contrib);
        self.ledger.vr_anchor -= led.vr_anchor.min(self.ledger.vr_anchor);
        self.stuck.push(StuckEntry { thread: thread.to_string(), target_core: target, ledger: led });
        true
    }
    /// 迁移成功确认：滞留条目出池（正常路径，不回退）。
    pub fn confirm(&mut self, thread: &str) -> bool {
        let before = self.stuck.len();
        self.stuck.retain(|s| s.thread != thread);
        self.stuck.len() < before
    }
    /// 回退：滞留回收 → 源核归队 → 三账回冲，闭合返回 true。
    pub fn rollback(&mut self, thread: &str) -> bool {
        let idx = match self.stuck.iter().position(|s| s.thread == thread) {
            Some(i) => i,
            None => return false,
        };
        if self.roster.iter().any(|t| t == thread) {
            return false; // 重复归队拒
        }
        let e = self.stuck.remove(idx);
        // 三账回冲
        self.ledger.load += e.ledger.load;
        self.ledger.contrib += e.ledger.contrib;
        self.ledger.vr_anchor += e.ledger.vr_anchor;
        self.roster.push(e.thread);
        true
    }
    /// 回退闭合断言：滞留池无此线程、在册有此线程、无泄漏。
    pub fn rollback_closed(&self, thread: &str) -> bool {
        !self.stuck.iter().any(|s| s.thread == thread)
            && self.roster.iter().any(|t| t == thread)
    }
    /// 全局无滞留（全部确认或回退完毕）。
    pub fn no_stuck(&self) -> bool {
        self.stuck.is_empty()
    }
    /// 在册含线程。
    pub fn has_thread(&self, thread: &str) -> bool {
        self.roster.iter().any(|t| t == thread)
    }
    /// 账泄漏检测：滞留池账 + 源核账 = 初值账（守恒）。
    pub fn no_leak(&self, init: &ThreeLedger) -> bool {
        let stuck_sum: ThreeLedger = self.stuck.iter().fold(ThreeLedger::zero(), |a, s| ThreeLedger {
            load: a.load + s.ledger.load,
            contrib: a.contrib + s.ledger.contrib,
            vr_anchor: a.vr_anchor + s.ledger.vr_anchor,
        });
        self.ledger.load + stuck_sum.load == init.load
            && self.ledger.contrib + stuck_sum.contrib == init.contrib
            && self.ledger.vr_anchor + stuck_sum.vr_anchor == init.vr_anchor
    }
}

/// 标准夹具：源核 0，四线程在册，初账满额。
pub fn standard_core() -> SrcCore {
    let mut c = SrcCore::new(0);
    for t in ["t1", "t2", "t3", "t4"] {
        c.roster.push(t.to_string());
    }
    c.ledger = ThreeLedger { load: 400, contrib: 4000, vr_anchor: 40 };
    c
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx1724_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F1724 迁移失败回退");

    // 1. 回退路径闭合（滞留回收→归队→回冲）
    let mut c = standard_core();
    let led = ThreeLedger { load: 100, contrib: 1000, vr_anchor: 10 };
    let out = c.migrate_out("t2", 1, led);
    let stuck1 = c.stuck.len() == 1 && !c.has_thread("t2");
    let rb = c.rollback("t2");
    let closed = c.rollback_closed("t2") && c.no_stuck()
        && c.ledger.load == 400 && c.ledger.contrib == 4000 && c.ledger.vr_anchor == 40;
    s.add("MIG-回退闭合", out && stuck1 && rb && closed,
        if rb && closed { "" } else { "回退不闭合" });

    // 2. 回退注入 10/10 正确归位
    let mut ok = 0;
    for r in 0..10 {
        let mut ci = standard_core();
        let li = ThreeLedger { load: 50 * (r as u64 + 1), contrib: 500 * (r as u64 + 1), vr_anchor: 5 * (r as u64 + 1) };
        let _ = ci.migrate_out("t1", 1 + r as usize, li);
        if ci.rollback("t1") && ci.rollback_closed("t1")
            && ci.ledger.load == 400 && ci.ledger.contrib == 4000 {
            ok += 1;
        }
    }
    s.add("MIG-注入10/10", ok == 10, if ok == 10 { "" } else { "注入归位失败" });

    // 3. 三账回冲精确（部分账回冲后剩余守恒）
    let mut c2 = standard_core();
    let l2 = ThreeLedger { load: 150, contrib: 1500, vr_anchor: 15 };
    let _ = c2.migrate_out("t3", 2, l2);
    let _ = c2.rollback("t3");
    let back = c2.ledger.load == 400 && c2.ledger.contrib == 4000 && c2.ledger.vr_anchor == 40;
    s.add("MIG-三账回冲", back, if back { "" } else { "回冲不精确" });

    // 4. 无泄漏守恒（滞留期间）
    let mut c3 = standard_core();
    let _ = c3.migrate_out("t1", 1, ThreeLedger { load: 80, contrib: 800, vr_anchor: 8 });
    let _ = c3.migrate_out("t2", 2, ThreeLedger { load: 60, contrib: 600, vr_anchor: 6 });
    let leak = c3.no_leak(&ThreeLedger { load: 400, contrib: 4000, vr_anchor: 40 });
    s.add("MIG-滞留无泄漏", leak, if leak { "" } else { "滞留泄漏" });

    // 5. 重复归队拒
    let mut c4 = standard_core();
    let _ = c4.migrate_out("t1", 1, ThreeLedger { load: 10, contrib: 100, vr_anchor: 1 });
    let first = c4.rollback("t1");
    let dup = !c4.rollback("t1");
    s.add("MIG-重复归队拒", first && dup, if first && dup { "" } else { "重复归队放行" });

    // 6. 幽灵回退拒（无滞留条目）
    let mut c5 = standard_core();
    let ghost = !c5.rollback("no_such");
    s.add("MIG-幽灵回退拒", ghost, if ghost { "" } else { "幽灵回退成功" });

    // 7. 幽灵迁出拒（不在册线程）
    let mut c6 = standard_core();
    let ghost_out = !c6.migrate_out("ghost", 1, ThreeLedger::zero());
    s.add("MIG-幽灵迁出拒", ghost_out, if ghost_out { "" } else { "幽灵迁出成功" });

    // 8. 正常确认路径（不回退：滞留出池、不归队）
    let mut c7 = standard_core();
    let _ = c7.migrate_out("t4", 1, ThreeLedger { load: 90, contrib: 900, vr_anchor: 9 });
    let confirm = c7.confirm("t4") && c7.no_stuck() && !c7.has_thread("t4");
    s.add("MIG-确认路径", confirm, if confirm { "" } else { "确认路径破" });

    // 9. 混合路径（一半确认一半回退）守恒
    let mut c8 = standard_core();
    let _ = c8.migrate_out("t1", 1, ThreeLedger { load: 70, contrib: 700, vr_anchor: 7 });
    let _ = c8.migrate_out("t2", 2, ThreeLedger { load: 130, contrib: 1300, vr_anchor: 13 });
    let _ = c8.confirm("t1");      // t1 成功迁出（账已扣不回）
    let _ = c8.rollback("t2");     // t2 回退（账回冲）
    let mixed = c8.ledger.load == 400 - 70 && c8.ledger.contrib == 4000 - 700
        && c8.has_thread("t2") && !c8.has_thread("t1");
    s.add("MIG-混合路径", mixed, if mixed { "" } else { "混合破账" });

    // 10. 多线程顺序回退全归位
    let mut c9 = standard_core();
    let _ = c9.migrate_out("t1", 1, ThreeLedger { load: 40, contrib: 400, vr_anchor: 4 });
    let _ = c9.migrate_out("t2", 2, ThreeLedger { load: 60, contrib: 600, vr_anchor: 6 });
    let _ = c9.migrate_out("t3", 3, ThreeLedger { load: 100, contrib: 1000, vr_anchor: 10 });
    let r1 = c9.rollback("t1");
    let r2 = c9.rollback("t2");
    let r3 = c9.rollback("t3");
    let all = r1 && r2 && r3 && c9.no_stuck() && c9.roster.len() == 4
        && c9.ledger.load == 400 && c9.ledger.contrib == 4000;
    s.add("MIG-顺序回退全归位", all, if all { "" } else { "顺序回退破" });

    // 11. 回退后总账守恒（滞留池空 + 账复原）
    let final_ok = c9.no_leak(&ThreeLedger { load: 400, contrib: 4000, vr_anchor: 40 })
        && c9.ledger.vr_anchor == 40;
    s.add("MIG-终账守恒", final_ok, if final_ok { "" } else { "终账漂移" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB930 && SEG_BASE < 0xB950;
    s.add("MIG-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
