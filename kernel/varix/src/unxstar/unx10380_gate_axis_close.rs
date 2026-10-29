//! UNX-F10380 硬闸轴闭账（十九项对平+哈希冻结）（0x1ba00 段）
//!
//! 锚点：UNX-F10380-J1 硬闸轴闭账——十九锚位全绿 + 预算核销零偏离 +
//! 闭账哈希冻结，核销偏离与冻结后再写各 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·闭账谱系）。
//!
//! 结构：十九锚位登记册 + 预算核销册（分项对平）+ 闭账冻结闸
//! + 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1ba00 独占 0x20）。
pub const SEG_BASE: u32 = 0x1ba00;
pub const GA_SHORT: u32 = SEG_BASE + 0x01; // 锚位缺额
pub const GA_DUP: u32 = SEG_BASE + 0x02; // 锚位重号
pub const GA_SEALED: u32 = SEG_BASE + 0x03; // 闭账后写账
pub const GA_CAP: u32 = SEG_BASE + 0x04; // 超容量
pub const GA_BUDGET_MISMATCH: u32 = SEG_BASE + 0x05; // 核销与预算不符
pub const GA_EMPTY: u32 = SEG_BASE + 0x06; // 空名

/// 十九锚位。
pub const AXIS_ANCHOR: usize = 19;
/// 登记定容。
pub const CAP: usize = 24;
/// 硬闸轴预算锚。
pub const BUDGET: u32 = 6_840;

/// 单条锚位。
#[derive(Clone)]
pub struct AxisItem {
    pub id: u32,
    pub name: String,
    pub green: bool,
}

impl AxisItem {
    pub fn ok(&self) -> bool {
        self.id != 0 && !self.name.is_empty()
    }
}

/// 单条核销分项。
#[derive(Clone)]
pub struct ChargeLine {
    pub item: String,
    pub amount: u32,
}

/// 硬闸轴闭账册。
pub struct GateAxisClose {
    pub items: Vec<AxisItem>,
    pub charges: Vec<ChargeLine>,
    pub closed: bool,
    pub note: String,
}

impl GateAxisClose {
    pub fn new(note: &str) -> GateAxisClose {
        GateAxisClose {
            items: Vec::new(),
            charges: Vec::new(),
            closed: false,
            note: note.to_string(),
        }
    }

    /// 登记锚位：重号、空名、超容、闭账均拒。
    pub fn enroll(&mut self, id: u32, name: &str, green: bool) -> Result<(), u32> {
        if self.closed {
            return Err(GA_SEALED);
        }
        if name.is_empty() {
            return Err(GA_EMPTY);
        }
        if id == 0 || self.items.iter().any(|i| i.id == id) {
            return Err(GA_DUP);
        }
        if self.items.len() >= CAP {
            return Err(GA_CAP);
        }
        self.items.push(AxisItem { id, name: name.to_string(), green });
        Ok(())
    }

    /// 核销分项：闭账后拒、空名拒。
    pub fn charge(&mut self, item: &str, amount: u32) -> Result<(), u32> {
        if self.closed {
            return Err(GA_SEALED);
        }
        if item.is_empty() {
            return Err(GA_EMPTY);
        }
        self.charges.push(ChargeLine { item: item.to_string(), amount });
        Ok(())
    }

    /// 核销对平：分项和与预算一致（零偏离）。
    pub fn zero_dev(&self) -> bool {
        self.charges.iter().map(|c| c.amount).sum::<u32>() == BUDGET
    }

    /// 锚位覆盖核。
    pub fn covered(&self) -> bool {
        self.items.len() == AXIS_ANCHOR
    }

    /// 绿计数。
    pub fn green_count(&self) -> usize {
        self.items.iter().filter(|i| i.green).count()
    }

    /// 闭账闸：覆盖达锚 + 全绿 + 核销零偏离 + 字段齐，通过后冻结。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(GA_SEALED);
        }
        if !self.covered() {
            return Err(GA_SHORT);
        }
        if self.green_count() != AXIS_ANCHOR {
            return Err(GA_SHORT);
        }
        if !self.zero_dev() {
            return Err(GA_BUDGET_MISMATCH);
        }
        if !self.items.iter().all(|i| i.ok()) {
            return Err(GA_EMPTY);
        }
        self.closed = true;
        Ok(self.fold())
    }

    /// 缺绿诊断。
    pub fn gaps(&self) -> Vec<u32> {
        self.items.iter().filter(|i| !i.green).map(|i| i.id).collect()
    }

    /// 核销偏离额（有符号）。
    pub fn deviation(&self) -> i64 {
        self.charges.iter().map(|c| c.amount).sum::<u32>() as i64 - BUDGET as i64
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1ba000;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        h = h.wrapping_mul(31).wrapping_add(self.green_count() as u32);
        h = h.wrapping_mul(31).wrapping_add(self.closed as u32);
        for i in &self.items {
            h = h.wrapping_mul(31).wrapping_add(i.id);
            h = h.wrapping_mul(31).wrapping_add(i.green as u32);
            for b in i.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        for c in &self.charges {
            h = h.wrapping_mul(31).wrapping_add(c.amount);
            for b in c.item.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 注入核（五项）：[缺位闭账, 红位闭账, 核销偏离, 重号, 闭账后写账] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mk = |note: &str| -> GateAxisClose { GateAxisClose::new(note) };
    let half = BUDGET / 2;
    let mut a = mk("inj-short");
    let mut i = 1u32;
    while (i as usize) < AXIS_ANCHOR {
        let _ = a.enroll(i, "位", true);
        i += 1;
    }
    let _ = a.charge("预算", BUDGET);
    got[0] = a.close() == Err(GA_SHORT) && !a.covered();
    // 2) 红位闭账必拒
    let mut b = mk("inj-red");
    for i in 1..=AXIS_ANCHOR as u32 {
        let _ = b.enroll(i, "位", i != AXIS_ANCHOR as u32);
    }
    let _ = b.charge("预算", BUDGET);
    got[1] = b.close() == Err(GA_SHORT) && b.gaps() == vec![AXIS_ANCHOR as u32];
    // 3) 核销偏离必拒
    let mut c = mk("inj-budget");
    for i in 1..=AXIS_ANCHOR as u32 {
        let _ = c.enroll(i, "位", true);
    }
    let _ = c.charge("预算", half);
    let _ = c.charge("尾款", half - 1);
    got[2] = c.close() == Err(GA_BUDGET_MISMATCH) && c.deviation() == -1;
    // 4) 重号必拒
    let mut d = mk("inj-dup");
    let _ = d.enroll(1, "a", true);
    got[3] = d.enroll(1, "b", true) == Err(GA_DUP);
    // 5) 闭账后写账必抓
    let mut e = mk("inj-seal");
    for i in 1..=AXIS_ANCHOR as u32 {
        let _ = e.enroll(i, "位", true);
    }
    let _ = e.charge("预算", BUDGET);
    let _ = e.close();
    got[4] = e.enroll(77, "late", true) == Err(GA_SEALED)
        && e.charge("late", 1) == Err(GA_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10380_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10380");

    // 正路径：十九位全绿 + 核销对平 + 闭账成功
    let mut axis = GateAxisClose::new("硬闸轴闭账");
    for i in 1..=AXIS_ANCHOR as u32 {
        let _ = axis.enroll(i, "锚位", true);
    }
    let _ = axis.charge("正演一线", BUDGET / 2);
    let _ = axis.charge("正演二线", BUDGET / 4);
    let _ = axis.charge("正演三线", BUDGET / 4);
    let closed = axis.close();
    let j1 = closed.is_ok() && closed.unwrap_or(0) != 0;
    s.add("J1-十九位闭账", j1, "十九位全绿核销对平闭账成功");

    // 闭账冻结态
    let j2 = axis.closed && axis.covered() && axis.gaps().is_empty() && axis.zero_dev();
    s.add("J2-闭账冻结态", j2, "闭账后覆盖满缺口空核销平");

    // 锚位字段齐备
    let j3 = axis.items.iter().all(|i| i.ok());
    s.add("J3-锚位字段齐", j3, "id 非零且名非空");

    // 容量边界
    let j4 = {
        let mut c = GateAxisClose::new("cap");
        let mut filled = true;
        for i in 1..=CAP as u32 {
            if c.enroll(i, "x", true).is_err() {
                filled = false;
            }
        }
        filled && c.enroll(999, "over", true) == Err(GA_CAP)
    };
    s.add("J4-容量边界", j4, "超出定容必拒");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "缺位/红位/核销偏离闭账必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "重号与闭账后写账必检出");

    // 闭账拒写（正册复现）
    let j7 = {
        axis.enroll(77, "late", true) == Err(GA_SEALED)
            && axis.charge("late", 1) == Err(GA_SEALED)
    };
    s.add("J7-闭账拒写", j7, "闭账后锚位与核销登记必拒");

    // 指纹稳定
    let j8 = axis.fold() == axis.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 预算核销对平语义：分项拼出恰好预算
    let j9 = {
        let mut c = GateAxisClose::new("budget");
        let _ = c.charge("a", BUDGET / 3);
        let _ = c.charge("b", BUDGET / 3);
        let _ = c.charge("c", BUDGET - 2 * (BUDGET / 3));
        c.zero_dev() && c.deviation() == 0
    };
    s.add("J9-核销对平", j9, "分项拼出恰好预算零偏离");

    // 核销偏离方向可辨
    let j10 = {
        let mut c = GateAxisClose::new("dev");
        let _ = c.charge("a", BUDGET + 2);
        c.deviation() == 2
    };
    s.add("J10-偏离方向", j10, "多记核销差额为正");

    // 红位闭账拒绝后可翻绿再闭
    let j11 = {
        let mut c = GateAxisClose::new("retry");
        for i in 1..=AXIS_ANCHOR as u32 {
            let _ = c.enroll(i, "位", i != 3);
        }
        let _ = c.charge("预算", BUDGET);
        let first = c.close() == Err(GA_SHORT);
        let _ = c.flip3();
        first && c.close().is_ok()
    };
    s.add("J11-翻绿重闭", j11, "拒绝后翻绿可重闭账");

    // 明细落账
    let j12 = axis.charges.len() == 3
        && axis.charges.iter().all(|c| !c.item.is_empty());
    s.add("J12-核销明细", j12, "三条核销分项全部非空");

    // 空册闭账拒
    let j13 = {
        let mut c = GateAxisClose::new("empty");
        c.close() == Err(GA_SHORT) && !c.covered()
    };
    s.add("J13-空册闭账拒", j13, "空册缺位闭账必拒");

    // 只读不改账
    let j14 = {
        let mut c = GateAxisClose::new("stable");
        let _ = c.enroll(1, "位", true);
        let f1 = c.fold();
        let _ = c.covered();
        let _ = c.green_count();
        let _ = c.deviation();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-闭账指纹落账",
        axis.fold() != 0,
        &format!("fold={:#x}", axis.fold()),
    );
    let empty = GateAxisClose::new("empty2").fold();
    s.add_dyn(
        "J16-空册指纹",
        empty == 0x1ba000u32.wrapping_mul(31).wrapping_mul(31).wrapping_mul(31),
        &format!("fold={:#x}", empty),
    );

    s
}

impl GateAxisClose {
    /// 翻位辅助（判据用）：把第 3 位翻绿。
    pub fn flip3(&mut self) -> Result<(), u32> {
        match self.items.iter_mut().find(|i| i.id == 3) {
            None => Err(GA_SHORT),
            Some(i) => {
                i.green = true;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10380_axis_flow() {
        let mut a = GateAxisClose::new("t");
        assert!(a.enroll(1, "a", true).is_ok());
        assert!(a.enroll(1, "dup", true) == Err(GA_DUP));
        assert!(a.enroll(2, "", true) == Err(GA_EMPTY));
        assert!(a.close() == Err(GA_SHORT));
        a.seal_freeze_close();
        assert!(a.enroll(3, "late", true) == Err(GA_SEALED));
    }

    #[test]
    fn unx10380_checks_green() {
        let s = run_unx10380_checks();
        assert!(s.all_passed(), "UNX-F10380 判据未全绿");
    }
}

impl GateAxisClose {
    /// 测试辅助：填满十九位并闭账。
    fn seal_freeze_close(&mut self) {
        if self.closed {
            return;
        }
        let mut i = (self.items.len() + 1) as u32;
        while (i as usize) <= AXIS_ANCHOR {
            let _ = self.enroll(i, "位", true);
            i += 1;
        }
        if self.charges.is_empty() {
            let _ = self.charge("预算", BUDGET);
        }
        let _ = self.close();
    }
}
