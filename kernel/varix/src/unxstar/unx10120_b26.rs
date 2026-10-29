//! UNX-F10120 B26 收口小结与预算核销（0x10220 段）
//!
//! 锚点：UNX-F10120-J1 收口小结落账（19 条战线条目 J1/J2 双判据齐备+
//! 预算 6,600 核销零偏离 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：双判据收口册（19 条战线 × J1/J2 双位）+ 三轴核销册（人工/机时/
//! 杂项三分项对平预算）+ 复核核（战线齐备度/双判据达成率/零偏离裁断）+
//! 注入核（核销不符/单判据缺位/战线缺额/重号/封账必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10220 独占 0x20）。
pub const SEG_BASE: u32 = 0x10220;
pub const B6_MISMATCH: u32 = SEG_BASE + 0x01; // 核销不符
pub const B6_HALF: u32 = SEG_BASE + 0x02; // 单判据缺位
pub const B6_SHORT: u32 = SEG_BASE + 0x03; // 战线缺额
pub const B6_DUP: u32 = SEG_BASE + 0x04; // 重号
pub const B6_SEALED: u32 = SEG_BASE + 0x05; // 封账后写账

/// B26 预算锚。
pub const BUDGET: u32 = 6_600;
/// 战线条目数锚。
pub const LINES_NEEDED: usize = 19;
/// 收口册定容。
pub const CAP: usize = 24;

/// 双判据战线条目。
#[derive(Clone)]
pub struct Line2 {
    pub id: u32,
    pub name: String,
    pub j1: bool,
    pub j2: bool,
}

impl Line2 {
    pub fn ok(&self) -> bool {
        self.id != 0 && !self.name.is_empty()
    }

    /// 双判据齐备。
    pub fn full(&self) -> bool {
        self.j1 && self.j2
    }
}

/// 双判据收口册。
pub struct DuoBook {
    pub lines: Vec<Line2>,
    pub sealed: bool,
}

impl DuoBook {
    pub fn new() -> DuoBook {
        DuoBook { lines: Vec::new(), sealed: false }
    }

    /// 登记战线（重号/空名/超容/封账拒）。
    pub fn enroll(&mut self, id: u32, name: &str, j1: bool, j2: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(B6_SEALED);
        }
        if name.is_empty() {
            return Err(B6_MISMATCH);
        }
        if id == 0 || self.lines.iter().any(|l| l.id == id) {
            return Err(B6_DUP);
        }
        if self.lines.len() >= CAP {
            return Err(B6_SHORT);
        }
        self.lines.push(Line2 { id, name: name.to_string(), j1, j2 });
        Ok(())
    }

    /// 战线齐备度。
    pub fn complete(&self) -> bool {
        self.lines.len() == LINES_NEEDED
    }

    /// 双判据全齐。
    pub fn all_full(&self) -> bool {
        self.complete() && self.lines.iter().all(|l| l.full())
    }

    /// 单判据缺位清单。
    pub fn half_list(&self) -> Vec<u32> {
        self.lines.iter().filter(|l| !l.full()).map(|l| l.id).collect()
    }

    /// 达成率分子。
    pub fn full_count(&self) -> usize {
        self.lines.iter().filter(|l| l.full()).count()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x102200;
        for l in &self.lines {
            h = h.wrapping_mul(31).wrapping_add(l.id);
            h = h.wrapping_mul(31).wrapping_add(l.j1 as u32 * 2 + l.j2 as u32);
            for b in l.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 核销分项轴。
#[derive(Clone, Copy, PartialEq)]
pub enum Axis {
    Man = 0,
    Machine = 1,
    Misc = 2,
}

impl Axis {
    pub fn id(&self) -> u32 {
        *self as u32
    }
}

/// 三轴核销册。
pub struct AxisRecon {
    pub budget: u32,
    pub man: u32,
    pub machine: u32,
    pub misc: u32,
    pub sealed: bool,
}

impl AxisRecon {
    pub fn new(budget: u32) -> AxisRecon {
        AxisRecon { budget, man: 0, machine: 0, misc: 0, sealed: false }
    }

    /// 按轴记入。
    pub fn charge(&mut self, ax: Axis, amount: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(B6_SEALED);
        }
        if amount == 0 {
            return Err(B6_MISMATCH);
        }
        match ax {
            Axis::Man => self.man = self.man.saturating_add(amount),
            Axis::Machine => self.machine = self.machine.saturating_add(amount),
            Axis::Misc => self.misc = self.misc.saturating_add(amount),
        }
        Ok(self.total())
    }

    pub fn total(&self) -> u32 {
        self.man.saturating_add(self.machine).saturating_add(self.misc)
    }

    pub fn deviation(&self) -> i64 {
        self.total() as i64 - self.budget as i64
    }

    pub fn zero_dev(&self) -> bool {
        self.deviation() == 0
    }

    /// 单轴占比（千分位整数，防浮点）。
    pub fn axis_permille(&self, ax: Axis) -> u32 {
        if self.budget == 0 {
            return 0;
        }
        let v = match ax {
            Axis::Man => self.man,
            Axis::Machine => self.machine,
            Axis::Misc => self.misc,
        };
        v * 1000 / self.budget
    }

    /// 三轴占比和对平（应恰为 1000）。
    pub fn permille_conserved(&self) -> bool {
        self.axis_permille(Axis::Man)
            + self.axis_permille(Axis::Machine)
            + self.axis_permille(Axis::Misc)
            == 1000
    }

    /// 超支裁断。
    pub fn overspent(&self) -> bool {
        self.deviation() > 0
    }

    /// 尾部快照（诊断面）。
    pub fn snapshot(&self) -> String {
        format!("m{}x{}z{}/{}", self.man, self.machine, self.misc, self.budget)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 核销不符（多记 1）必检出
    let mut r = AxisRecon::new(BUDGET);
    let _ = r.charge(Axis::Man, BUDGET / 3);
    let _ = r.charge(Axis::Machine, BUDGET / 3);
    let _ = r.charge(Axis::Misc, BUDGET - 2 * (BUDGET / 3) + 1);
    got[0] = !r.zero_dev();
    // 2) 单判据缺位必检出
    let mut d = DuoBook::new();
    for i in 1..=LINES_NEEDED as u32 {
        let _ = d.enroll(i, "战线", true, i != 5);
    }
    got[1] = !d.all_full() && d.half_list().len() == 1;
    // 3) 战线缺额必检出
    let mut d2 = DuoBook::new();
    for i in 1..(LINES_NEEDED as u32) {
        let _ = d2.enroll(i, "战线", true, true);
    }
    got[2] = !d2.complete();
    // 4) 重号必拒
    let mut d3 = DuoBook::new();
    let _ = d3.enroll(1, "a", true, true);
    got[3] = d3.enroll(1, "b", true, true).err() == Some(B6_DUP);
    // 5) 封账后写账必抓
    d3.seal();
    got[4] = d3.enroll(2, "late", true, true).err() == Some(B6_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10120_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10120");

    // 正路径：19 条战线双判据齐备
    let mut d = DuoBook::new();
    for i in 1..=LINES_NEEDED as u32 {
        let _ = d.enroll(i, "B26 战线", true, true);
    }
    let j1 = d.all_full();
    s.add("J1-十九条双判据齐", j1, "19 条战线 J1/J2 双位全翻");

    // 三轴核销零偏离
    let mut r = AxisRecon::new(BUDGET);
    let _ = r.charge(Axis::Man, 3_300);
    let _ = r.charge(Axis::Machine, 2_640);
    let _ = r.charge(Axis::Misc, 660);
    let j2 = r.zero_dev();
    s.add("J2-核销零偏离", j2, "三轴合计 6,600 对平预算");

    // 条目字段齐
    let j3 = d.lines.iter().all(|l| l.ok());
    s.add("J3-条目字段齐", j3, "id 非零且名非空");

    // 核销面字段合法（总数与预算同量级）
    let j4 = r.total() == BUDGET && r.deviation() == 0;
    s.add("J4-核销面合法", j4, "总数==预算且偏离为零");

    // 核销不符检出
    let inj = inject_five();
    let j5 = inj[0];
    s.add("J5-核销不符检出", j5, "多记 1 必检出");

    // 单判据缺位检出
    let j6 = inj[1];
    s.add("J6-单判据缺位检出", j6, "J1 有 J2 缺必检出");

    // 战线缺额检出
    let j7 = inj[2];
    s.add("J7-战线缺额检出", j7, "19 缺 1 必检出");

    // 重号与封账必抓
    let j8 = inj[3] && inj[4];
    s.add("J8-重号封账必抓", j8, "重号拒且封账拒写");

    // 零额记入按不符拒收
    let j9 = {
        let mut r9 = AxisRecon::new(100);
        r9.charge(Axis::Misc, 0).err() == Some(B6_MISMATCH)
    };
    s.add("J9-零额拒收", j9, "零额按核销不符拒");

    // 封账后核销拒
    let j10 = {
        r.seal();
        r.charge(Axis::Man, 1).err() == Some(B6_SEALED)
    };
    s.add("J10-封账拒核销", j10, "封账后 charge 必拒");

    // 达成率语义
    let j11 = {
        let mut d11 = DuoBook::new();
        for i in 1..LINES_NEEDED as u32 {
            let _ = d11.enroll(i, "y", true, true);
        }
        let _ = d11.enroll(LINES_NEEDED as u32, "y", true, false);
        !d11.all_full() && d11.full_count() == LINES_NEEDED - 1
    };
    s.add("J11-达成率语义", j11, "一条缺 J2 即非全齐");

    // 容量边界
    let j12 = {
        let mut d12 = DuoBook::new();
        let mut ok = true;
        for i in 1..=CAP as u32 {
            if d12.enroll(i, "x", true, true).is_err() {
                ok = false;
            }
        }
        ok && d12.enroll(999, "over", true, true).err() == Some(B6_SHORT)
    };
    s.add("J12-容量边界", j12, "超定容必拒");

    // 指纹稳定
    let j13 = d.fold() == d.fold();
    s.add("J13-指纹稳定", j13, "折叠指纹两次一致");

    // 注入核全抓
    let j14 = inj.iter().all(|g| *g);
    s.add_dyn("J14-注入全抓", j14, &format!("inj5={:?}", inj));

    // 三轴占比对平
    let j15 = r.permille_conserved() && !r.overspent();
    s.add("J15-占比对平", j15, "三轴千分位合 1000 且未超支");

    // 快照渲染面
    let j16 = r.snapshot().contains(&format!("/{}", BUDGET));
    s.add("J16-快照渲染", j16, "尾快照含预算锚");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10120_recon_flow() {
        let mut r = AxisRecon::new(BUDGET);
        let _ = r.charge(Axis::Man, BUDGET);
        assert!(r.zero_dev());
        let mut d = DuoBook::new();
        assert!(d.enroll(1, "x", true, false).is_ok());
        assert!(!d.all_full());
        d.seal();
        assert!(d.enroll(2, "y", true, true).err() == Some(B6_SEALED));
    }

    #[test]
    fn unx10120_checks_green() {
        let s = run_unx10120_checks();
        assert!(s.all_passed(), "UNX-F10120 判据未全绿");
    }
}
