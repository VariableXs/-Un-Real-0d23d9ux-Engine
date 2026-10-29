//! UNX-F11038 五族挂号压力账（万次收发对平）（0x1e4c0 段）
//!
//! 锚点：UNX-F11038-J1 五族万次账守恒（10/10），压力下零丢项（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·压力账）。
//!
//! 结构：五族压力账（族 × 入项 + 出项 + 驻留）+ 收发挂点（入项出项逐族
//! 记账）+ 守恒器（入 = 出 + 驻留逐族闭合）+ 零丢项断言（万次入项全数
//! 出项或驻留可查）+ fold 与冻结 + 注入核（越族出项/未立账收发/驻留造假
//! /冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e4c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e4c0;
pub const ST_NOFAM: u32 = SEG_BASE + 0x01; // 族号越界
pub const ST_UNSET: u32 = SEG_BASE + 0x02; // 族未立账
pub const ST_OVEROUT: u32 = SEG_BASE + 0x03; // 出项超入项
pub const ST_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const ST_LOST: u32 = SEG_BASE + 0x05; // 丢项命中
pub const ST_DRAIN: u32 = SEG_BASE + 0x06; // 压力失平

/// 五族编号。
pub const ST_FAMS: usize = 5;
/// 万次收发步数。
pub const ST_STEPS: u32 = 10_000;
/// 压力账域标（fold 种子）。
pub const STRESS_TAG: u64 = 0x1e4c_38;

/// 单族压力账：入项 + 出项。
#[derive(Clone, Debug)]
pub struct StressFam {
    pub name: String,
    pub inn: u32,
    pub out: u32,
}

impl StressFam {
    /// 驻留。
    pub fn reside(&self) -> u32 {
        self.inn.saturating_sub(self.out)
    }

    /// 守恒：出不得超入。
    pub fn sound(&self) -> bool {
        self.out <= self.inn
    }

    pub fn render(&self) -> String {
        format!("st|{}|i{}|o{}", self.name, self.inn, self.out)
    }
}

/// 五族压力账：逐族入项出项记账。
pub struct StressBook {
    pub fams: Vec<StressFam>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl StressBook {
    pub fn new() -> StressBook {
        StressBook { fams: Vec::new(), frozen: false, root: None }
    }

    /// 族立账。
    pub fn establish(&mut self, name: &str) -> Result<usize, u32> {
        if self.frozen {
            return Err(ST_FROZEN);
        }
        if name.is_empty() {
            return Err(ST_NOFAM);
        }
        if self.fams.len() >= ST_FAMS {
            return Err(ST_NOFAM);
        }
        self.fams.push(StressFam { name: name.to_string(), inn: 0, out: 0 });
        Ok(self.fams.len() - 1)
    }

    /// 入项挂点。
    pub fn put(&mut self, fam: usize, n: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(ST_FROZEN);
        }
        if self.fams.is_empty() {
            return Err(ST_UNSET);
        }
        if fam >= self.fams.len() {
            return Err(ST_NOFAM);
        }
        self.fams[fam].inn = self.fams[fam].inn.wrapping_add(n);
        Ok(self.fams[fam].inn)
    }

    /// 出项挂点：出不得超入。
    pub fn take(&mut self, fam: usize, n: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(ST_FROZEN);
        }
        if self.fams.is_empty() {
            return Err(ST_UNSET);
        }
        if fam >= self.fams.len() {
            return Err(ST_NOFAM);
        }
        if n > self.fams[fam].reside() {
            return Err(ST_OVEROUT);
        }
        self.fams[fam].out = self.fams[fam].out.wrapping_add(n);
        Ok(self.fams[fam].out)
    }

    /// 压力失平校验：守恒不过即报失平。
    pub fn check_drain(&self, steps: u32) -> Result<(), u32> {
        if self.conserved(steps) {
            Ok(())
        } else {
            Err(ST_DRAIN)
        }
    }

    /// 按名查位次。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.fams.len() {
            if self.fams[k].name == name {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 入项总量。
    pub fn inn_total(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.fams.len() {
            n = n.wrapping_add(self.fams[k].inn);
            k += 1;
        }
        n
    }

    /// 出项总量。
    pub fn out_total(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.fams.len() {
            n = n.wrapping_add(self.fams[k].out);
            k += 1;
        }
        n
    }

    /// 驻留总量。
    pub fn resid_total(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.fams.len() {
            n = n.wrapping_add(self.fams[k].reside());
            k += 1;
        }
        n
    }

    /// 万次守恒：入 = 出 + 驻留且逐族健全。
    pub fn conserved(&self, steps: u32) -> bool {
        if self.fams.len() != ST_FAMS {
            return false;
        }
        let mut ok = self.inn_total() == steps;
        let mut k = 0usize;
        while k < self.fams.len() {
            if !self.fams[k].sound() {
                ok = false;
            }
            k += 1;
        }
        ok && self.inn_total() == self.out_total().wrapping_add(self.resid_total())
    }

    /// 零丢项断言：万次入项全数出项（零驻留）。
    pub fn zero_lost(&self, steps: u32) -> bool {
        self.out_total() == steps && self.resid_total() == 0
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = STRESS_TAG;
        let mut k = 0usize;
        while k < self.fams.len() {
            h = (h ^ self.fams[k].inn as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.fams[k].out as u64).wrapping_mul(0x100000001b3);
            let bytes = self.fams[k].name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(0x100000001b3);
                b += 1;
            }
            k += 1;
        }
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 族名确定性文本。
pub fn stress_fam(no: usize) -> String {
    match no % 5 {
        0 => "sysv".to_string(),
        1 => "posix".to_string(),
        2 => "pty".to_string(),
        3 => "sock".to_string(),
        _ => "evfd".to_string(),
    }
}

/// 万次收发全链：五族立账；万步轮转入项并即时出项（零驻留）。
pub fn build_stressbook() -> (StressBook, bool) {
    let mut b = StressBook::new();
    let mut ok = true;
    let mut f = 0usize;
    while f < ST_FAMS {
        if b.establish(&stress_fam(f)).is_err() {
            ok = false;
        }
        f += 1;
    }
    let mut s = 0u32;
    while s < ST_STEPS {
        let f = (s % ST_FAMS as u32) as usize;
        if b.put(f, 1).is_err() {
            ok = false;
        }
        if b.take(f, 1).is_err() {
            ok = false;
        }
        s += 1;
    }
    let b_ok = ok && b.conserved(ST_STEPS) && b.zero_lost(ST_STEPS);
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 出项超入项必拒
    let mut a = StressBook::new();
    let _ = a.establish("sysv");
    let _ = a.put(0, 3);
    got[0] = a.take(0, 4).err() == Some(ST_OVEROUT);
    // 2) 未立账族收发必拒
    let mut b = StressBook::new();
    let _ = b.establish("posix");
    got[1] = b.put(1, 1).err() == Some(ST_NOFAM) && b.take(1, 1).err() == Some(ST_NOFAM);
    // 3) 驻留造假探针：发收不符驻留可查
    let mut c = StressBook::new();
    let _ = c.establish("pty");
    let _ = c.put(0, 5);
    let _ = c.take(0, 2);
    got[2] = c.fams[0].reside() == 3 && c.resid_total() == 3 && !c.zero_lost(ST_STEPS);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_stressbook();
    d.freeze();
    got[3] = d.put(0, 1).err() == Some(ST_FROZEN) && d.take(0, 1).err() == Some(ST_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11038_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11038");

    // J1 五族万次账守恒：入 = 出 + 驻留逐族闭合
    let (mut book, green) = build_stressbook();
    s.add("J1-万次守恒", green && book.conserved(ST_STEPS) && book.check_drain(ST_STEPS).is_ok(), "五族万次入出驻留三面守恒");

    // J2 压力下零丢项：出项全数且零驻留
    s.add("J2-零丢项", book.zero_lost(ST_STEPS), "万次入项全数出项零驻留");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.put(0, 1).err() == Some(ST_FROZEN)
        && book.take(0, 1).err() == Some(ST_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后入出均必拒");

    // J5 注入检出：出项超入 + 未立账收发
    let inj = inject_four();
    s.add("J5-超出未立抓", inj[0] && inj[1], "出项超入与未立账收发两路必抓");

    // J6 注入检出：驻留造假 + 冻结后写
    s.add("J6-驻留冻结检出", inj[2] && inj[3], "驻留可查与冻结后写必抓");

    // J7 轮转均匀：逐族入出各两千
    let j7 = {
        let per = ST_STEPS / ST_FAMS as u32;
        let mut even = true;
        let mut k = 0usize;
        while k < book.fams.len() {
            if book.fams[k].inn != per || book.fams[k].out != per {
                even = false;
            }
            k += 1;
        }
        even && per == 2000
    };
    s.add("J7-轮转均匀", j7, "万步轮转逐族入出各两千");

    // J8 分次出项：多次出项累计不超入
    let j8 = {
        let mut c = StressBook::new();
        let _ = c.establish("sock");
        let _ = c.put(0, 10);
        let _ = c.take(0, 4);
        let _ = c.take(0, 6);
        c.fams[0].out == 10 && c.zero_lost(10)
    };
    s.add("J8-分次出项", j8, "分次出项累计齐平");

    // J9 超出后账不损：拒收后入出面不变
    let j9 = {
        let mut c = StressBook::new();
        let _ = c.establish("evfd");
        let _ = c.put(0, 5);
        let f0 = c.fold();
        let _ = c.take(0, 6);
        c.fold() == f0
    };
    s.add("J9-拒后不损", j9, "越限出项被拒不损账");

    // J10 族名确定性
    // 族名按 no%5 归类：pty 落在余数 2 位（no=7）
    let j10 = stress_fam(0) == "sysv" && stress_fam(3) == "sock" && stress_fam(7) == "pty";
    s.add("J10-族名确定", j10, "族名样本同入同出");

    // J11 渲染面：族渲染含名入出
    let j11 = {
        let x = StressFam { name: "sysv".to_string(), inn: 12, out: 9 };
        let r = x.render();
        r.contains("sysv") && r.contains("i12") && r.contains("o9")
    };
    s.add("J11-渲染明细", j11, "族渲染含名入出");

    // J12 空账零面：未步账指纹为种子
    let j12 = StressBook::new().fold() == STRESS_TAG && StressBook::new().inn_total() == 0;
    s.add("J12-空账零面", j12, "空账零入零出指纹成立");

    // J13 建族上限：第五族后立账必拒
    let j13 = {
        let mut c = StressBook::new();
        let mut f = 0usize;
        let mut filled = true;
        while f < ST_FAMS {
            if c.establish(&stress_fam(f)).is_err() {
                filled = false;
            }
            f += 1;
        }
        filled && c.establish("extra").is_err()
    };
    s.add("J13-建族上限", j13, "超出五族立账必拒");

    // J14 丢项注入：漏出项千次守恒仍在但零丢项为假
    let j14 = {
        // 守恒按五族批面裁断：五族齐立账后单族漏出项
        let mut c = StressBook::new();
        let mut f = 0usize;
        while f < ST_FAMS {
            let _ = c.establish(&stress_fam(f));
            f += 1;
        }
        let _ = c.put(0, 1000);
        let _ = c.take(0, 900);
        c.conserved(1000) && !c.zero_lost(1000)
    };
    s.add("J14-丢项注入", j14, "漏出项守恒仍在零丢项为假");

    // J15 动态明细：万次对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-万次明细",
        fp == root && book.inn_total() == ST_STEPS,
        &format!("root={:#x} in={} out={}", fp, book.inn_total(), book.out_total()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11038_stress_flow() {
        let (mut b, built) = build_stressbook();
        assert!(built);
        b.freeze();
        assert!(b.take(0, 1).err() == Some(ST_FROZEN));
        assert!(b.put(1, 1).err() == Some(ST_FROZEN));
        assert!(b.find("sock").is_some());
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11038_checks_green() {
        let s = run_unx11038_checks();
        assert!(s.all_passed(), "UNX-F11038 判据未全绿");
    }
}
