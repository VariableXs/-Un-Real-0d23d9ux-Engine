//! UNX-F11209 futex 操作集消费面登记（0x1f809 段）
//!
//! 锚点：UNX-F11209-J1 glibc 消费的 futex 操作集（WAIT/WAKE/WAIT_BITSET
//! 类）与 AI-14 四操作冻结接口逐项对账，操作覆盖清单零缺项。
//! 域：CoRun·UNX-C5-B01——操作表按 Linux futex_op 编号逐条登记消费态，
//! AI-14 冻结四操作单列对账；缺项、错号、重复分别拒收。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f809;
pub const FX_UNKNOWN: u32 = SEG_BASE + 0x01; // 未知操作号
pub const FX_DUP: u32 = SEG_BASE + 0x02; // 操作重录
pub const FX_MISS: u32 = SEG_BASE + 0x03; // 冻结操作缺项
pub const FX_NAME: u32 = SEG_BASE + 0x04; // 操作名空
pub const FX_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后登记
pub const FX_NOCONSUME: u32 = SEG_BASE + 0x06; // 消费态未落
pub const FX_OVER: u32 = SEG_BASE + 0x07; // 超容

/// Linux futex 操作编号。
pub const FT_WAIT: u32 = 0;
pub const FT_WAKE: u32 = 1;
pub const FT_REQUEUE: u32 = 3;
pub const FT_CMP_REQUEUE: u32 = 4;
pub const FT_WAKE_OP: u32 = 5;
pub const FT_LOCK_PI: u32 = 6;
pub const FT_UNLOCK_PI: u32 = 7;
pub const FT_TRYLOCK_PI: u32 = 8;
pub const FT_WAIT_BITSET: u32 = 9;
pub const FT_WAKE_BITSET: u32 = 10;
pub const FT_WAIT_REQUEUE_PI: u32 = 11;
pub const FT_CMP_REQUEUE_PI: u32 = 12;

/// AI-14 四操作冻结接口。
pub const AI14_OPS: [u32; 4] = [FT_WAIT, FT_WAKE, FT_WAIT_BITSET, FT_WAKE_BITSET];
/// 操作表容量。
pub const CAP: usize = 16;
pub const FX_TAG: u64 = 0x1f809c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 单条消费登记：操作号、名、消费态、是否 AI-14 冻结项。
#[derive(Clone, Debug)]
pub struct OpEntry {
    pub op: u32,
    pub name: &'static str,
    pub consumed: bool,
    pub ai14: bool,
}

impl OpEntry {
    pub fn render(&self) -> String {
        format!("{}#{}{}{}", self.name, self.op,
            if self.consumed { "|use" } else { "|idle" },
            if self.ai14 { "|ai14" } else { "" })
    }
}

/// 操作集登记册。
#[derive(Clone, Debug)]
pub struct FutexReg {
    pub ops: Vec<OpEntry>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl FutexReg {
    pub fn new() -> FutexReg {
        FutexReg { ops: Vec::new(), frozen: false, root: None }
    }

    /// 登记：冻结/未知号/空名/重录/超容五路拒。
    pub fn register(&mut self, op: u32, name: &'static str, consumed: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(FX_FROZEN);
        }
        if op > FT_CMP_REQUEUE_PI {
            return Err(FX_UNKNOWN);
        }
        if name.is_empty() {
            return Err(FX_NAME);
        }
        if self.ops.iter().any(|o| o.op == op) {
            return Err(FX_DUP);
        }
        if self.ops.len() >= CAP {
            return Err(FX_OVER);
        }
        let ai14 = AI14_OPS.contains(&op);
        self.ops.push(OpEntry { op, name, consumed, ai14 });
        Ok(())
    }

    pub fn has(&self, op: u32) -> bool {
        self.ops.iter().any(|o| o.op == op)
    }

    /// 冻结四操作缺项（只列未登记或未消费的）。
    pub fn missing_ai14(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < AI14_OPS.len() {
            let op = AI14_OPS[k];
            let mut hit = false;
            let mut used = false;
            let mut j = 0usize;
            while j < self.ops.len() {
                if self.ops[j].op == op {
                    hit = true;
                    used = self.ops[j].consumed;
                }
                j += 1;
            }
            if !hit || !used {
                out.push(op);
            }
            k += 1;
        }
        out
    }

    /// 消费覆盖数。
    pub fn consumed(&self) -> usize {
        self.ops.iter().filter(|o| o.consumed).count()
    }

    /// 未消费操作号（诊断面）。
    pub fn idle(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.ops.len() {
            if !self.ops[k].consumed {
                out.push(self.ops[k].op);
            }
            k += 1;
        }
        out
    }

    /// 覆盖清单零缺项。
    pub fn coverage_complete(&self) -> bool {
        self.missing_ai14().is_empty() && self.ops.len() >= AI14_OPS.len()
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = FX_TAG;
        let mut k = 0usize;
        while k < self.ops.len() {
            h = (h ^ self.ops[k].op as u64).wrapping_mul(FP_STEP);
            let nb = self.ops[k].name.as_bytes();
            let mut b = 0usize;
            while b < nb.len() {
                h = (h ^ nb[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            h = (h ^ self.ops[k].consumed as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.ops[k].ai14 as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 建册：12 个 Linux 操作号逐个登记消费态（TRYLOCK_PI 留空闲样本）。
pub fn build_reg() -> (FutexReg, bool) {
    let rows: [(u32, &'static str, bool); 12] = [
        (FT_WAIT, "WAIT", true),
        (FT_WAKE, "WAKE", true),
        (FT_REQUEUE, "REQUEUE", true),
        (FT_CMP_REQUEUE, "CMP_REQUEUE", true),
        (FT_WAKE_OP, "WAKE_OP", true),
        (FT_LOCK_PI, "LOCK_PI", true),
        (FT_UNLOCK_PI, "UNLOCK_PI", true),
        (FT_TRYLOCK_PI, "TRYLOCK_PI", false),
        (FT_WAIT_BITSET, "WAIT_BITSET", true),
        (FT_WAKE_BITSET, "WAKE_BITSET", true),
        (FT_WAIT_REQUEUE_PI, "WAIT_REQUEUE_PI", true),
        (FT_CMP_REQUEUE_PI, "CMP_REQUEUE_PI", true),
    ];
    let mut r = FutexReg::new();
    let mut k = 0usize;
    while k < rows.len() {
        if r.register(rows[k].0, rows[k].1, rows[k].2).is_err() {
            return (r, false);
        }
        k += 1;
    }
    let ok = r.coverage_complete() && r.ops.len() == 12;
    (r, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 未知操作号必拒。
    let mut a = FutexReg::new();
    got[0] = a.register(FT_CMP_REQUEUE_PI + 1, "BOGUS", true).err() == Some(FX_UNKNOWN);
    // 2) 操作重录必拒（同号异名也算重录）。
    let mut b = FutexReg::new();
    let _ = b.register(FT_WAIT, "WAIT", true);
    got[1] = b.register(FT_WAIT, "WAIT2", true).err() == Some(FX_DUP);
    // 3) 冻结四操作未消费即列缺项。
    let mut c = FutexReg::new();
    let _ = c.register(FT_WAIT, "WAIT", true);
    let _ = c.register(FT_WAKE, "WAKE", false);
    got[2] = c.missing_ai14() == vec![FT_WAKE, FT_WAIT_BITSET, FT_WAKE_BITSET]
        && !c.coverage_complete();
    // 4) 冻结后登记拒。
    let (mut d, _) = build_reg();
    d.freeze();
    got[3] = d.register(FT_WAIT, "DUP", true).err() == Some(FX_FROZEN);
    got
}

/// 批内判据。
pub fn run_unx11209_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11209");
    let (mut r, built) = build_reg();

    s.add("J1-十二操作登记", built && r.ops.len() == 12, "12 个 futex 操作号逐条落账");
    s.add("J2-冻结四操作在册", r.has(FT_WAIT) && r.has(FT_WAKE) && r.has(FT_WAIT_BITSET) && r.has(FT_WAKE_BITSET),
        "AI-14 四操作均在册");
    s.add("J3-消费零缺项", r.missing_ai14().is_empty() && r.coverage_complete(),
        "四操作消费覆盖清单零缺项");
    s.add("J4-覆盖数", r.consumed() == 11, "12 条中 11 条已消费（TRYLOCK_PI 留空）");
    s.add("J5-空闲可枚举", r.idle() == vec![FT_TRYLOCK_PI], "空闲操作可逐号枚举");
    s.add("J6-冻结标注", r.ops.iter().filter(|o| o.ai14).count() == 4,
        "冻结接口标注恰四条");

    s.add("J7-操作号单调", {
        let mut k = 1usize;
        let mut mono = true;
        while k < r.ops.len() {
            if r.ops[k - 1].op >= r.ops[k].op {
                mono = false;
            }
            k += 1;
        }
        mono
    }, "登记序即操作号升序");

    s.add("J8-坏号拒", {
        let mut a = FutexReg::new();
        a.register(200, "X", true).err() == Some(FX_UNKNOWN)
    }, "200 号非 futex 操作必拒");

    s.add("J9-空名拒", {
        let mut a = FutexReg::new();
        a.register(FT_WAKE, "", true).err() == Some(FX_NAME)
    }, "空操作名必拒");

    s.add("J10-渲染面", r.ops[0].render() == "WAIT#0|use|ai14" && r.ops[7].render() == "TRYLOCK_PI#8|idle",
        "冻结项与空闲项渲染可区分");

    let inj = inject_four();
    s.add("J11-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "坏号/重录/缺消费/冻结四路必抓");

    let root = r.freeze();
    s.add("J12-冻结根", r.fold() == root && root != 0, "冻结后操作册指纹可重算");

    let fp = r.fold();
    s.add_dyn("J13-操作明细", r.missing_ai14().is_empty() && fp == root,
        &format!("ops={} used={}", r.ops.len(), r.consumed()));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11209_futex_flow() {
        let (mut r, built) = build_reg();
        assert!(built);
        assert!(r.coverage_complete());
        assert_eq!(r.consumed(), 11);
        assert_eq!(r.register(FT_WAIT, "DUP", true), Err(FX_DUP));
        assert_eq!(r.register(13, "BAD", true), Err(FX_UNKNOWN));
        let root = r.freeze();
        assert_eq!(r.fold(), root);
        assert_eq!(r.register(FT_WAKE, "X", true), Err(FX_FROZEN));
        assert_eq!(AI14_OPS.len(), 4);
    }

    #[test]
    fn unx11209_checks_green() {
        let s = run_unx11209_checks();
        assert!(s.all_passed(), "UNX-F11209 判据未全绿");
    }
}
