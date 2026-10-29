//! UNX-F11271 musl 与 glibc 双线行为差异账（0x1f470 段）
//! 纯功能模型：双线差异四列账（条目/甲线值/乙线值/判定）——逐条登记，
//! 判定由两侧值是否相同**自动推出**（同则 same、异则 differ），不接受手填；
//! 差异不判失败：differ 条目必须带备注，零备注即静默，静默才是红。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f470 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f470;
/// 条目号越界。
pub const DF_NO: u32 = SEG_BASE + 0x01;
/// 条目号重登。
pub const DF_DUP: u32 = SEG_BASE + 0x02;
/// 条目名空。
pub const DF_ITEM: u32 = SEG_BASE + 0x03;
/// 判定与两侧值不符（同值却填 differ / 异值却填 same）。
pub const DF_VERDICT: u32 = SEG_BASE + 0x04;
/// 差异条目缺备注（静默差异）。
pub const DF_NOTE: u32 = SEG_BASE + 0x05;
/// 封册后写账。
pub const DF_FROZEN: u32 = SEG_BASE + 0x06;
/// 差异表已满。
pub const DF_CAP: u32 = SEG_BASE + 0x07;
/// 封册时仍有未判条目或静默差异。
pub const DF_MISS: u32 = SEG_BASE + 0x08;

/// 判定闭集。
pub const V_SAME: u8 = 0;
pub const V_DIFFER: u8 = 1;
/// 条目号上界。
pub const NO_MAX: u32 = 999;
/// 差异表定容。
pub const CAP: usize = 48;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_71;

/// 单条四列账：条目 / 甲线 / 乙线 / 判定，另附备注。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub no: u32,
    pub item: String,
    pub a: String,
    pub b: String,
    pub note: String,
}

impl DiffRow {
    /// 两侧值是否相同（判定的唯一来源）。
    pub fn same(&self) -> bool {
        self.a == self.b
    }

    /// 自动判定。
    pub fn verdict(&self) -> u8 {
        if self.same() {
            V_SAME
        } else {
            V_DIFFER
        }
    }

    /// 静默：差异但没有备注。
    pub fn silent(&self) -> bool {
        self.verdict() == V_DIFFER && self.note.is_empty()
    }

    /// 四列渲染：`#7|条目|甲|乙|D`。
    pub fn render(&self) -> String {
        let v = if self.same() { "S" } else { "D" };
        format!("#{}|{}|{}|{}|{}", self.no, self.item, self.a, self.b, v)
    }
}

/// 双线差异册：四列账 + 自动判定 + 备注门禁。
#[derive(Clone, Debug)]
pub struct DiffBook {
    pub rows: Vec<DiffRow>,
    pub accepts: u32,
    pub rejects: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl DiffBook {
    pub fn new() -> DiffBook {
        DiffBook { rows: Vec::new(), accepts: 0, rejects: 0, frozen: false, root: None }
    }

    /// 登记一条四列账。判定值必须与两侧值一致（判定不可手改）。
    /// 判定序：封册 → 号域 → 重号 → 名空 → 判定一致 → 差异备注 → 定容。
    pub fn record(&mut self, no: u32, item: &str, a: &str, b: &str, claimed: u8, note: &str)
        -> Result<usize, u32>
    {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DF_FROZEN);
        }
        if no == 0 || no > NO_MAX {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DF_NO);
        }
        if self.rows.iter().any(|r| r.no == no) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DF_DUP);
        }
        if item.is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DF_ITEM);
        }
        let auto = if a == b { V_SAME } else { V_DIFFER };
        if claimed != auto || claimed > V_DIFFER {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DF_VERDICT);
        }
        if auto == V_DIFFER && note.is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DF_NOTE);
        }
        if self.rows.len() >= CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DF_CAP);
        }
        self.rows.push(DiffRow {
            no,
            item: clone_str(item),
            a: clone_str(a),
            b: clone_str(b),
            note: clone_str(note),
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.rows.len().wrapping_sub(1))
    }

    /// 同值条目数。
    pub fn same_count(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.rows.len() {
            if let Some(r) = self.rows.get(k) {
                if r.verdict() == V_SAME {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 差异条目数。
    pub fn differ_count(&self) -> u32 {
        self.rows.len() as u32 - self.same_count()
    }

    /// 静默差异清单（差异且无备注）。
    pub fn silent(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.rows.len() {
            if let Some(r) = self.rows.get(k) {
                if r.silent() {
                    v.push(r.no);
                }
            }
            k = k.wrapping_add(1);
        }
        v
    }

    /// 封册前裁断：条目齐、零静默。差异本身不判失败。
    pub fn seal(&mut self) -> Result<(u32, u32), u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DF_FROZEN);
        }
        if self.rows.len() < NEED || !self.silent().is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DF_MISS);
        }
        self.accepts = self.accepts.wrapping_add(1);
        Ok((self.same_count(), self.differ_count()))
    }

    /// 绿判：条目齐且零静默（差异不判失败）。
    pub fn green(&self) -> bool {
        self.rows.len() >= NEED && self.silent().is_empty()
    }

    /// FNV-1a 链指纹：四列全字段序敏感。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.rows.len() {
            if let Some(r) = self.rows.get(k) {
                h = (h ^ r.no as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ r.verdict() as u64).wrapping_mul(FNV_PRIME);
                for s in [&r.item, &r.a, &r.b, &r.note] {
                    let bs = s.as_bytes();
                    let mut j = 0usize;
                    while j < bs.len() {
                        if let Some(x) = bs.get(j) {
                            h = (h ^ *x as u64).wrapping_mul(FNV_PRIME);
                        }
                        j = j.wrapping_add(1);
                    }
                }
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 封册：根幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 借串入堆。
fn clone_str(s: &str) -> String {
    let mut o = String::new();
    o.push_str(s);
    o
}

/// 双线对账条目下限（12 条差异面）。
pub const NEED: usize = 12;

/// 十二列样本：偶号差异（带备注）、奇号同值。
pub fn sample_book() -> DiffBook {
    let mut b = DiffBook::new();
    let mut i = 1u32;
    while i <= NEED as u32 {
        if i % 2 == 0 {
            let _ = b.record(i, "双线差异面", "musl", "glibc", V_DIFFER, "行为差异立账");
        } else {
            let _ = b.record(i, "双线同值面", "same", "same", V_SAME, "");
        }
        i = i.wrapping_add(1);
    }
    b
}

/// 注入核：四路坏样本。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // (a) 条目号越界
    let mut a = DiffBook::new();
    got[0] = a.record(0, "面", "a", "a", V_SAME, "").err() == Some(DF_NO);
    // (b) 号重登
    let mut b = DiffBook::new();
    let _ = b.record(1, "面", "a", "a", V_SAME, "");
    got[1] = b.record(1, "面", "a", "a", V_SAME, "").err() == Some(DF_DUP);
    // (c) 判定与两侧值不符
    let mut c = DiffBook::new();
    got[2] = c.record(1, "面", "a", "a", V_DIFFER, "注").err() == Some(DF_VERDICT);
    // (d) 封册后写账
    let mut d = sample_book();
    let _ = d.freeze();
    got[3] = d.record(99, "面", "a", "b", V_DIFFER, "注").err() == Some(DF_FROZEN);
    got
}

/// 双线差异账判据。
pub fn run_unx11271_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11271");

    // X1 四列账落账
    let x1 = {
        let b = sample_book();
        b.rows.len() == NEED && b.same_count() == 6 && b.differ_count() == 6
    };
    s.add("X1-四列账", x1, "12 条落账，同值 6 / 差异 6");

    // X2 差异不判失败
    let x2 = {
        let mut b = sample_book();
        let sealed = b.seal();
        sealed == Ok((6, 6)) && b.green()
    };
    s.add("X2-差异不判失败", x2, "6 条差异带备注仍判绿");

    // X3 静默差异必抓
    let x3 = {
        let mut b = DiffBook::new();
        b.record(1, "面", "m", "g", V_DIFFER, "").err() == Some(DF_NOTE)
    };
    s.add("X3-静默差异", x3, "差异无备注报静默");

    // X4 判定自动推出
    let x4 = {
        let mut a = DiffBook::new();
        let same_claim = a.record(1, "面", "x", "x", V_DIFFER, "注").err() == Some(DF_VERDICT);
        let diff_claim = a.record(2, "面", "x", "y", V_SAME, "").err() == Some(DF_VERDICT);
        let over = a.record(3, "面", "x", "x", 2, "").err() == Some(DF_VERDICT);
        same_claim && diff_claim && over
    };
    s.add("X4-判定自动", x4, "三路手填判定与两侧值不符均拒");

    // X5 重号
    let x5 = {
        let mut b = DiffBook::new();
        let _ = b.record(3, "面", "a", "a", V_SAME, "");
        b.record(3, "面", "a", "a", V_SAME, "").err() == Some(DF_DUP)
    };
    s.add("X5-重号", x5, "同号二次登记报重号");

    // X6 号域
    let x6 = {
        let mut a = DiffBook::new();
        let zero = a.record(0, "面", "a", "a", V_SAME, "").err() == Some(DF_NO);
        let over = a.record(NO_MAX.wrapping_add(1), "面", "a", "a", V_SAME, "").err() == Some(DF_NO);
        zero && over
    };
    s.add("X6-号域", x6, "0 号与 1000 号均越界");

    // X7 条目名空
    let x7 = DiffBook::new().record(1, "", "a", "a", V_SAME, "").err() == Some(DF_ITEM);
    s.add("X7-条目名空", x7, "空名条目不入账");

    // X8 定容
    let x8 = {
        let mut b = DiffBook::new();
        let mut i = 1u32;
        let mut ok = true;
        while i <= CAP as u32 {
            if b.record(i, "面", "a", "a", V_SAME, "").is_err() {
                ok = false;
            }
            i = i.wrapping_add(1);
        }
        ok && b.record(100, "面", "a", "a", V_SAME, "").err() == Some(DF_CAP)
    };
    s.add("X8-定容", x8, "48 条后第 49 条报满");

    // X9 封册缺项
    let x9 = {
        let mut b = DiffBook::new();
        let mut i = 1u32;
        while i < NEED as u32 {
            let _ = b.record(i, "面", "a", "a", V_SAME, "");
            i = i.wrapping_add(1);
        }
        b.seal().err() == Some(DF_MISS) && !b.green()
    };
    s.add("X9-封册缺项", x9, "11 条时封册报缺项");

    // X10 指纹确定
    let x10 = {
        let b = sample_book();
        let a = b.fold();
        a == b.fold() && a != 0
    };
    s.add("X10-指纹确定", x10, "同账两次指纹一致且非零");

    // X11 封册幂等
    let x11 = {
        let mut b = sample_book();
        let r1 = b.freeze();
        let r2 = b.freeze();
        let live = b.fold();
        b.rows.clear();
        let r3 = b.freeze();
        r1 == r2 && r2 == r3 && r1.is_some() && live != b.fold()
    };
    s.add("X11-封册幂等", x11, "二次封册回原根，账变不改根");

    // X12 封册拒写
    let x12 = {
        let mut b = sample_book();
        let _ = b.freeze();
        b.record(88, "面", "a", "b", V_DIFFER, "注").err() == Some(DF_FROZEN)
            && b.seal().err() == Some(DF_FROZEN)
    };
    s.add("X12-封册拒写", x12, "登记与封册裁断均拒");

    // X13 账行明细
    let (x13, line13) = {
        let b = sample_book();
        let line = format!("same={} diff={} first={}", b.same_count(), b.differ_count(), match b.rows.first() {
            Some(r) => r.render(),
            None => String::from("-"),
        });
        (b.green() && line.contains("|same|same|S"), line)
    };
    s.add_dyn("X13-账行明细", x13, &line13);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11271_flow() {
        let mut b = sample_book();
        assert_eq!(b.rows.len(), 12);
        assert_eq!(b.same_count(), 6);
        assert_eq!(b.differ_count(), 6);
        assert!(b.green());
        assert!(b.silent().is_empty());
        assert_eq!(b.seal(), Ok((6, 6)));
        assert_eq!(b.accepts, 13);
        assert_eq!(b.rejects, 0);
        let r1 = b.freeze();
        let r2 = b.freeze();
        assert_eq!(r1, r2);
        assert!(r1.is_some());
        assert!(b.root == r1);
        assert!(b.record(88, "面", "a", "b", V_DIFFER, "注").err() == Some(DF_FROZEN));
    }

    #[test]
    fn unx11271_gates() {
        let mut b = DiffBook::new();
        assert!(b.record(1, "面", "a", "a", V_SAME, "").is_ok());
        assert!(b.record(1, "面", "a", "a", V_SAME, "").err() == Some(DF_DUP));
        assert!(b.record(0, "面", "a", "a", V_SAME, "").err() == Some(DF_NO));
        assert!(b.record(2, "", "a", "a", V_SAME, "").err() == Some(DF_ITEM));
        assert!(b.record(2, "面", "a", "a", V_DIFFER, "").err() == Some(DF_VERDICT));
        assert!(b.record(2, "面", "a", "b", V_DIFFER, "").err() == Some(DF_NOTE));
        assert!(b.record(2, "面", "a", "b", V_DIFFER, "注").is_ok());
        assert!(b.seal().err() == Some(DF_MISS));
        assert!(b.freeze().is_some());
        assert!(b.record(3, "面", "a", "b", V_DIFFER, "注").err() == Some(DF_FROZEN));
        assert_eq!(b.accepts, 2);
        assert_eq!(b.rejects, 7);
    }

    #[test]
    fn unx11271_checks_green() {
        assert!(run_unx11271_checks().all_passed(), "UNX-F11271 判据未全绿");
    }
}
