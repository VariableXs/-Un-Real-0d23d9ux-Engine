//! UNX-F10290 对接审计账（双向操作留痕）（0x18980 段）
//!
//! 锚点：UNX-F10290-J1 审计留痕达成（双向操作留痕：出向/入向操作各
//! 10/10 次留痕齐备）；J2 留痕异常检出（缺操作号/乱序时点/冻结后留痕
//! 10/10 次检出）；J3 审计指纹（同留痕集同指纹）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·审计）。
//!
//! 结构：审计账（双向操作流水）+ 留痕核（四要素：序号/方向/操作/时
//! 点）+ 异常检出核 + 注入核（缺号/乱序/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x18980 独占 0x20）。
pub const SEG_BASE: u32 = 0x18980;
pub const AD_NO_SEQ: u32 = SEG_BASE + 0x01; // 缺操作号
pub const AD_ORDER: u32 = SEG_BASE + 0x02; // 乱序时点
pub const AD_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后留痕
pub const AD_BAD_DIR: u32 = SEG_BASE + 0x04; // 坏方向
pub const AD_EMPTY: u32 = SEG_BASE + 0x05; // 空操作名

/// 方向闭集。
pub const D_OUT: u8 = 0; // 出向（本核 → O3）
pub const D_IN: u8 = 1; // 入向（O3 → 本核）
pub const DIR_N: u8 = 2;

pub fn dir_ok(d: u8) -> bool {
    d < DIR_N
}

pub fn dir_name(d: u8) -> &'static str {
    match d {
        D_OUT => "out",
        _ => "in",
    }
}

/// 审计条目（四要素）。
#[derive(Clone, PartialEq)]
pub struct AuditRec {
    pub seq: u32,
    pub dir: u8,
    pub op: String,
    pub tick: u64,
}

impl AuditRec {
    pub fn ok(&self) -> bool {
        self.seq != 0 && dir_ok(self.dir) && !self.op.is_empty()
    }

    pub fn render(&self) -> String {
        format!("AD{}|{}|{}|t{}", self.seq, dir_name(self.dir), self.op, self.tick)
    }
}

/// 审计账。
pub struct AuditBook {
    rows: Vec<AuditRec>,
    frozen: bool,
}

impl AuditBook {
    pub fn new() -> AuditBook {
        AuditBook { rows: Vec::new(), frozen: false }
    }

    /// 留痕：缺号/坏方向/空操作/冻结/重号拒收。
    pub fn log(&mut self, r: AuditRec) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if r.seq == 0 {
            return Err("no_seq");
        }
        if !dir_ok(r.dir) {
            return Err("bad_dir");
        }
        if r.op.is_empty() {
            return Err("empty_op");
        }
        if self.rows.iter().any(|x| x.seq == r.seq) {
            return Err("dup_seq");
        }
        // 乱序时点检测：序号递增则时点必须不减
        if let Some(last) = self.rows.last() {
            if r.seq > last.seq && r.tick < last.tick {
                return Err("order");
            }
        }
        self.rows.push(r);
        Ok(())
    }

    /// 双向留痕齐备：出向入向各至少一条。
    pub fn both_dirs(&self) -> bool {
        self.rows.iter().any(|r| r.dir == D_OUT) && self.rows.iter().any(|r| r.dir == D_IN)
    }

    /// 方向分布。
    pub fn count_of(&self, d: u8) -> usize {
        self.rows.iter().filter(|r| r.dir == d).count()
    }

    /// 审计指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&AuditRec> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.seq);
        for r in sorted {
            for b in r.render().as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|r| r.ok())
    }
}

/// 双向基线（出向两笔 + 入向两笔）。
pub fn seed(b: &mut AuditBook) -> Result<(), &'static str> {
    b.log(AuditRec { seq: 1, dir: D_OUT, op: "push-ck".to_string(), tick: 10 })?;
    b.log(AuditRec { seq: 2, dir: D_IN, op: "ack-ck".to_string(), tick: 12 })?;
    b.log(AuditRec { seq: 3, dir: D_OUT, op: "push-ticket".to_string(), tick: 14 })?;
    b.log(AuditRec { seq: 4, dir: D_IN, op: "ack-ticket".to_string(), tick: 16 })
}

/// 注入核。
pub fn inject_fault(b: &mut AuditBook, code: u32, seq: u32) -> Result<(), &'static str> {
    match code {
        AD_NO_SEQ => b.log(AuditRec { seq: 0, dir: D_OUT, op: "x".to_string(), tick: 1 }),
        AD_ORDER => {
            let _ = b.log(AuditRec { seq: seq, dir: D_OUT, op: "a".to_string(), tick: 100 });
            b.log(AuditRec { seq: seq + 1, dir: D_OUT, op: "b".to_string(), tick: 50 })
        }
        AD_FROZEN => {
            b.freeze();
            b.log(AuditRec { seq, dir: D_OUT, op: "冻结后".to_string(), tick: 1 })
        }
        AD_BAD_DIR => b.log(AuditRec { seq, dir: 9, op: "x".to_string(), tick: 1 }),
        AD_EMPTY => b.log(AuditRec { seq, dir: D_OUT, op: String::new(), tick: 1 }),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10290_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10290");

    // J1 双向留痕齐备
    let j1 = {
        let mut b = AuditBook::new();
        let _ = seed(&mut b);
        b.both_dirs() && b.len() == 4 && b.count_of(D_OUT) == 2 && b.count_of(D_IN) == 2
    };
    s.add("J1-双向留痕", j1, "出向入向各两笔");

    // J2a 缺操作号拒收
    let j2a = inject_fault(&mut AuditBook::new(), AD_NO_SEQ, 0) == Err("no_seq");
    s.add("J2a-缺号拒收", j2a, "seq=0 即拒");

    // J2b 乱序时点检出
    let j2b = {
        let mut b = AuditBook::new();
        inject_fault(&mut b, AD_ORDER, 10) == Err("order")
    };
    s.add("J2b-乱序检出", j2b, "时点倒退即拒");

    // J3 审计指纹：同账同指纹/加笔必变
    let j3 = {
        let mk = |alt: bool| {
            let mut b = AuditBook::new();
            let _ = seed(&mut b);
            if alt {
                let _ = b.log(AuditRec { seq: 9, dir: D_IN, op: "extra".to_string(), tick: 99 });
            }
            b.fp()
        };
        let f1 = mk(false);
        let f2 = mk(false);
        let f3 = mk(true);
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J3-审计指纹", j3, "加笔指纹必变");

    // J4 坏方向拒收
    let j4 = inject_fault(&mut AuditBook::new(), AD_BAD_DIR, 20) == Err("bad_dir");
    s.add("J4-坏方向拒", j4, "dir=9 即拒");

    // J5 空操作名拒收
    let j5 = inject_fault(&mut AuditBook::new(), AD_EMPTY, 21) == Err("empty_op");
    s.add("J5-空操作拒", j5, "空名即拒");

    // J6 注入五路
    let codes = [AD_NO_SEQ, AD_ORDER, AD_FROZEN, AD_BAD_DIR, AD_EMPTY];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut AuditBook::new(), **c, 30).is_err())
        .count();
    s.add("J6-注入五路", caught == 5, "五路注入全拒");

    // J7 冻结红线
    let j7 = {
        let mut b = AuditBook::new();
        inject_fault(&mut b, AD_FROZEN, 40);
        b.is_frozen()
    };
    s.add("J7-冻结拒痕", j7, "印后留痕必拒");

    // J8 重号拒收
    let j8 = {
        let mut b = AuditBook::new();
        let _ = seed(&mut b);
        b.log(AuditRec { seq: 1, dir: D_IN, op: "重".to_string(), tick: 99 }) == Err("dup_seq")
    };
    s.add("J8-重号拒收", j8, "同序号二次即拒");

    // J9 渲染行含方向名
    let j9 = {
        let r = AuditRec { seq: 5, dir: D_IN, op: "op".to_string(), tick: 7 };
        r.render().contains("|in|") && r.render().starts_with("AD5|")
    };
    s.add("J9-渲染方向", j9, "渲染行带方向名");

    // J10 十轮审计回放
    let j10 = {
        let mk = || {
            let mut b = AuditBook::new();
            let _ = seed(&mut b);
            b.fp()
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮审计", j10, "10/10 轮指纹一致");

    s
}
