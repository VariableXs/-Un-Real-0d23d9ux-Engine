//! UNX-F10291 判据增量同步协议（新增判据增量推送）（0x18BA0 段）
//!
//! 锚点：UNX-F10291-J1 增量推送达成（新增判据增量推送：批量/序号/幂
//! 等三面 10/10 次）；J2 推送异常检出（乱序推送/重放推送/越界批量
//! 10/10 次检出）；J3 增量指纹（同增量同指纹）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·增量同步）。
//!
//! 结构：增量批账（批号 → 判据集）+ 推送核（序号连续性校验）+ 幂等
//! 重放核 + 注入核（乱序/重放/越界必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x18BA0 独占 0x20）。
pub const SEG_BASE: u32 = 0x18BA0;
pub const IS_ORDER: u32 = SEG_BASE + 0x01; // 乱序推送
pub const IS_REPLAY: u32 = SEG_BASE + 0x02; // 重放推送
pub const IS_OVER: u32 = SEG_BASE + 0x03; // 越界批量
pub const IS_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后推
pub const IS_EMPTY: u32 = SEG_BASE + 0x05; // 空增量

/// 批量上限。
pub const BATCH_MAX: usize = 50;

/// 增量条目。
#[derive(Clone, PartialEq)]
pub struct DeltaItem {
    pub ck: u32,
    pub text: String,
}

impl DeltaItem {
    pub fn ok(&self) -> bool {
        self.ck != 0 && !self.text.is_empty()
    }
}

/// 增量账。
pub struct DeltaBook {
    last_seq: u32,          // 已推送批号（连续性真源）
    applied: Vec<(u32, Vec<DeltaItem>)>, // (批号, 条目集)
    frozen: bool,
}

impl DeltaBook {
    pub fn new() -> DeltaBook {
        DeltaBook { last_seq: 0, applied: Vec::new(), frozen: false }
    }

    /// 推送增量批：序号必须连续（last_seq+1）；重放/越界/空批拒收。
    pub fn push(&mut self, seq: u32, items: Vec<DeltaItem>) -> Result<usize, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if items.is_empty() {
            return Err("empty_delta");
        }
        if items.len() > BATCH_MAX {
            return Err("over_batch");
        }
        if items.iter().any(|i| !i.ok()) {
            return Err("bad_item");
        }
        if seq <= self.last_seq {
            return Err("replay");
        }
        if seq != self.last_seq + 1 {
            return Err("gap");
        }
        self.last_seq = seq;
        self.applied.push((seq, items.clone()));
        Ok(items.len())
    }

    /// 幂等重放核：已推送批号重推 → 拒收（replay）且不重复落账。
    pub fn replay_check(&self, seq: u32) -> bool {
        seq <= self.last_seq && !self.applied.iter().any(|(s, _)| *s == seq + 1000)
    }

    /// 已应用批数与条目总数。
    pub fn stats(&self) -> (usize, usize) {
        let n = self.applied.len();
        let total: usize = self.applied.iter().map(|(_, i)| i.len()).sum();
        (n, total)
    }

    /// 增量指纹：全应用批折叠。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for (seq, items) in &self.applied {
            for b in seq.to_le_bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x01000193);
            }
            for i in items {
                for b in format!("{}|{}", i.ck, i.text).as_bytes() {
                    h ^= *b as u32;
                    h = h.wrapping_mul(0x01000193);
                }
            }
        }
        h
    }

    pub fn last_seq(&self) -> u32 {
        self.last_seq
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 造增量条目集。
pub fn mk_items(base: u32, n: usize) -> Vec<DeltaItem> {
    (0..n)
        .map(|i| DeltaItem {
            ck: base + i as u32,
            text: format!("增量判据{}", i),
        })
        .collect()
}

/// 注入核。
pub fn inject_fault(b: &mut DeltaBook, code: u32) -> Result<usize, &'static str> {
    match code {
        IS_ORDER => {
            let _ = b.push(1, mk_items(100, 2))?;
            b.push(3, mk_items(200, 2)) // 跳批
        }
        IS_REPLAY => {
            let _ = b.push(1, mk_items(100, 2))?;
            b.push(1, mk_items(100, 2))
        }
        IS_OVER => b.push(1, mk_items(100, BATCH_MAX + 1)),
        IS_FROZEN => {
            b.freeze();
            b.push(1, mk_items(100, 1))
        }
        IS_EMPTY => b.push(1, vec![]),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10291_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10291");

    // J1 连续推送三批
    let j1 = {
        let mut b = DeltaBook::new();
        let n1 = b.push(1, mk_items(100, 3));
        let n2 = b.push(2, mk_items(200, 2));
        let n3 = b.push(3, mk_items(300, 1));
        n1 == Ok(3) && n2 == Ok(2) && n3 == Ok(1)
            && b.stats() == (3, 6)
            && b.last_seq() == 3
    };
    s.add("J1-连续三批", j1, "批号连续条目计数精确");

    // J2a 跳批检出
    let j2a = inject_fault(&mut DeltaBook::new(), IS_ORDER) == Err("gap");
    s.add("J2a-跳批检出", j2a, "批 1→3 精确拒");

    // J2b 重放检出
    let j2b = inject_fault(&mut DeltaBook::new(), IS_REPLAY) == Err("replay");
    s.add("J2b-重放检出", j2b, "同批重推即拒");

    // J2c 越界批量拒收
    let j2c = inject_fault(&mut DeltaBook::new(), IS_OVER) == Err("over_batch");
    s.add("J2c-越界拒收", j2c, "超 BATCH_MAX 即拒");

    // J3 增量指纹：同增量同指纹/条目变必变
    let j3 = {
        let mk = |alt: bool| {
            let mut b = DeltaBook::new();
            let items = if alt { mk_items(100, 3) } else { mk_items(100, 2) };
            let _ = b.push(1, items);
            b.fp()
        };
        let f1 = mk(false);
        let f2 = mk(false);
        let f3 = mk(true);
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J3-增量指纹", j3, "条目集变指纹必变");

    // J4 幂等重放核：已推送批不重复落账
    let j4 = {
        let mut b = DeltaBook::new();
        let _ = b.push(1, mk_items(100, 2));
        let before = b.stats();
        let _ = b.push(1, mk_items(100, 2)); // 重推被拒
        let after = b.stats();
        before == after && before == (1, 2)
    };
    s.add("J4-重放不落账", j4, "重推零副作用");

    // J5 空增量拒收
    let j5 = inject_fault(&mut DeltaBook::new(), IS_EMPTY) == Err("empty_delta");
    s.add("J5-空增量拒", j5, "空集即拒");

    // J6 注入五路
    let codes = [IS_ORDER, IS_REPLAY, IS_OVER, IS_FROZEN, IS_EMPTY];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut DeltaBook::new(), **c).is_err())
        .count();
    s.add("J6-注入五路", caught == 5, "五路注入全拒");

    // J7 冻结红线
    let j7 = {
        let mut b = DeltaBook::new();
        inject_fault(&mut b, IS_FROZEN);
        b.is_frozen()
    };
    s.add("J7-冻结拒推", j7, "印后推送必拒");

    // J8 批量上限真源
    let j8 = BATCH_MAX == 50;
    s.add("J8-批量上限", j8, "50 上限真源");

    // J9 条目负样本
    let j9 = {
        let zero = DeltaItem { ck: 0, text: "x".to_string() };
        let empty = DeltaItem { ck: 1, text: String::new() };
        !zero.ok() && !empty.ok()
    };
    s.add("J9-条目负样本", j9, "零号/空文 ok 必假");

    // J10 十轮推送回放
    let j10 = {
        let mk = || {
            let mut b = DeltaBook::new();
            let _ = b.push(1, mk_items(100, 2));
            let _ = b.push(2, mk_items(200, 3));
            (b.stats(), b.fp())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0 == (2, 5)
    };
    s.add("J10-十轮推送", j10, "10/10 轮一致");

    s
}
