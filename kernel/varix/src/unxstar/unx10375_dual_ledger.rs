//! UNX-F10375 正演双档分账（模拟/真机正演结果分账）（0x1b960 段）
//!
//! 锚点：UNX-F10375-J1 正演双档分账——同一判据在模拟/真机两档各登记结果，
//! 单档项与未登记差异各 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·分账谱系）。
//!
//! 结构：双档账（每项两档结果位）+ 差异账（两档不一致必须显式登记差异位）
//! + 单档检出 + 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b960 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b960;
pub const DL_DUP_ID: u32 = SEG_BASE + 0x01; // 分账项重号
pub const DL_EMPTY_NAME: u32 = SEG_BASE + 0x02; // 项名空
pub const DL_ONE_LEG: u32 = SEG_BASE + 0x03; // 单档项（两档未齐）
pub const DL_UNDIFFED: u32 = SEG_BASE + 0x04; // 两档不一致却未登记差异
pub const DL_SEALED: u32 = SEG_BASE + 0x05; // 封账后写账
pub const DL_CAP: u32 = SEG_BASE + 0x06; // 超容量

/// 分账项定容。
pub const CAP: usize = 24;
/// 双档集锚。
pub const SET_ANCHOR: usize = 12;

/// 单条分账项：两档结果 + 差异登记位。
#[derive(Clone)]
pub struct DualItem {
    pub id: u32,
    pub name: String,
    pub sim_green: Option<bool>,
    pub real_green: Option<bool>,
    pub diff_noted: bool,
}

impl DualItem {
    /// 双档齐：两档结果都在场。
    pub fn both_legs(&self) -> bool {
        self.sim_green.is_some() && self.real_green.is_some()
    }

    /// 两档一致。
    pub fn consistent(&self) -> bool {
        self.both_legs() && self.sim_green == self.real_green
    }

    /// 分账合规：双档齐 且 （一致 或 差异已显式登记）。
    pub fn settled(&self) -> bool {
        self.both_legs() && (self.consistent() || self.diff_noted)
    }

    /// 未登记差异：两档不一致但差异位空。
    pub fn undiffed(&self) -> bool {
        self.both_legs() && !self.consistent() && !self.diff_noted
    }

    pub fn render(&self) -> String {
        let f = |v: &Option<bool>| match v {
            None => "-".to_string(),
            Some(true) => "G".to_string(),
            Some(false) => "R".to_string(),
        };
        format!(
            "D{}|{}|sim={}|real={}|diff={}",
            self.id, self.name, f(&self.sim_green), f(&self.real_green),
            self.diff_noted as u32
        )
    }
}

/// 双档分账册。
pub struct DualLedger {
    pub items: Vec<DualItem>,
    pub sealed: bool,
    pub note: String,
}

impl DualLedger {
    pub fn new(note: &str) -> DualLedger {
        DualLedger { items: Vec::new(), sealed: false, note: note.to_string() }
    }

    /// 登记（未跑态）：重号、空名、超容、封账均拒。
    pub fn enroll(&mut self, id: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DL_SEALED);
        }
        if name.is_empty() {
            return Err(DL_EMPTY_NAME);
        }
        if id == 0 || self.items.iter().any(|i| i.id == id) {
            return Err(DL_DUP_ID);
        }
        if self.items.len() >= CAP {
            return Err(DL_CAP);
        }
        self.items.push(DualItem {
            id,
            name: name.to_string(),
            sim_green: None,
            real_green: None,
            diff_noted: false,
        });
        Ok(())
    }

    fn find_mut(&mut self, id: u32) -> Option<&mut DualItem> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// 模拟档结果登记。
    pub fn put_sim(&mut self, id: u32, green: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(DL_SEALED);
        }
        match self.find_mut(id) {
            None => Err(DL_DUP_ID),
            Some(i) => {
                i.sim_green = Some(green);
                Ok(())
            }
        }
    }

    /// 真机档结果登记。
    pub fn put_real(&mut self, id: u32, green: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(DL_SEALED);
        }
        match self.find_mut(id) {
            None => Err(DL_DUP_ID),
            Some(i) => {
                i.real_green = Some(green);
                Ok(())
            }
        }
    }

    /// 差异显式登记：只有两档不一致时才允许登记差异位。
    pub fn note_diff(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(DL_SEALED);
        }
        match self.find_mut(id) {
            None => Err(DL_DUP_ID),
            Some(i) if !i.both_legs() => Err(DL_ONE_LEG),
            Some(i) if i.consistent() => Err(DL_UNDIFFED),
            Some(i) => {
                i.diff_noted = true;
                Ok(())
            }
        }
    }

    /// 单档项清点：只跑了一档的项数。
    pub fn one_leg_count(&self) -> usize {
        self.items.iter().filter(|i| !i.both_legs()).count()
    }

    /// 未登记差异清点。
    pub fn undiffed_count(&self) -> usize {
        self.items.iter().filter(|i| i.undiffed()).count()
    }

    /// 差异项清单。
    pub fn diff_ids(&self) -> Vec<u32> {
        self.items
            .iter()
            .filter(|i| i.both_legs() && !i.consistent())
            .map(|i| i.id)
            .collect()
    }

    /// 集覆盖核。
    pub fn set_complete(&self) -> bool {
        self.items.len() == SET_ANCHOR
    }

    /// 终局裁定：覆盖达锚、零单档、零未登记差异、全部合规。
    pub fn all_settled(&self) -> bool {
        self.set_complete()
            && self.one_leg_count() == 0
            && self.undiffed_count() == 0
            && self.items.iter().all(|i| i.settled())
    }

    /// 残留诊断。
    pub fn gaps(&self) -> Vec<u32> {
        self.items.iter().filter(|i| !i.settled()).map(|i| i.id).collect()
    }

    /// 封账并返回折叠指纹。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b9600;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        for i in &self.items {
            h = h.wrapping_mul(31).wrapping_add(i.id);
            h = h.wrapping_mul(31)
                .wrapping_add(i.sim_green.map(|v| v as u32 + 1).unwrap_or(0));
            h = h.wrapping_mul(31)
                .wrapping_add(i.real_green.map(|v| v as u32 + 1).unwrap_or(0));
            h = h.wrapping_mul(31).wrapping_add(i.diff_noted as u32);
            for b in i.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 注入核（五项）：[单档裁定, 未登记差异, 一致项记差异, 重号, 空名] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 单档项进终局裁定必不通过
    let mut l = DualLedger::new("inj-oneleg");
    let _ = l.enroll(1, "a");
    let _ = l.put_sim(1, true);
    got[0] = !l.all_settled() && l.one_leg_count() == 1;
    // 2) 两档不一致未登记差异必检出
    let mut l2 = DualLedger::new("inj-undiff");
    let _ = l2.enroll(1, "a");
    let _ = l2.put_sim(1, true);
    let _ = l2.put_real(1, false);
    got[1] = l2.undiffed_count() == 1 && l2.note_diff(1).is_ok();
    // 3) 一致项不允许记差异
    let mut l3 = DualLedger::new("inj-cons");
    let _ = l3.enroll(1, "a");
    let _ = l3.put_sim(1, true);
    let _ = l3.put_real(1, true);
    got[2] = l3.note_diff(1) == Err(DL_UNDIFFED);
    // 4) 重号必拒
    let mut l4 = DualLedger::new("inj-dup");
    let _ = l4.enroll(1, "a");
    got[3] = l4.enroll(1, "b") == Err(DL_DUP_ID);
    // 5) 空名必拒
    let mut l5 = DualLedger::new("inj-empty");
    got[4] = l5.enroll(2, "") == Err(DL_EMPTY_NAME);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10375_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10375");

    // 正路径：12 项双档齐，前 10 项两档一致、后 2 项差异显式登记
    let mut ledger = DualLedger::new("模拟/真机双档分账");
    for i in 1..=SET_ANCHOR as u32 {
        let _ = ledger.enroll(i, "分账项");
    }
    for i in 1..=SET_ANCHOR as u32 {
        let _ = ledger.put_sim(i, true);
    }
    for i in 1..=(SET_ANCHOR - 2) as u32 {
        let _ = ledger.put_real(i, true);
    }
    for i in (SET_ANCHOR - 1) as u32..=SET_ANCHOR as u32 {
        let _ = ledger.put_real(i, false);
        let _ = ledger.note_diff(i);
    }
    let j1 = ledger.all_settled();
    s.add("J1-双档全合规", j1, "12 项双档齐且差异显式登记");

    // 一致项与差异项分账清晰
    let j2 = ledger.diff_ids() == vec![(SET_ANCHOR - 1) as u32, SET_ANCHOR as u32];
    s.add("J2-差异清单", j2, "差异项编号恰好两项");

    // 残留诊断：只跑模拟档的账残留可枚举
    let j3 = {
        let mut c = DualLedger::new("gaps");
        let _ = c.enroll(1, "a");
        let _ = c.put_sim(1, true);
        c.one_leg_count() == 1 && c.gaps() == vec![1]
    };
    s.add("J3-单档可枚举", j3, "单档项编号全部列出");

    // 容量边界
    let j4 = {
        let mut c = DualLedger::new("cap");
        let mut filled = true;
        for i in 1..=CAP as u32 {
            if c.enroll(i, "x").is_err() {
                filled = false;
            }
        }
        filled && c.enroll(999, "over") == Err(DL_CAP)
    };
    s.add("J4-容量边界", j4, "超出定容必拒");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "单档裁定/未登记差异/一致记差异必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "重号与空名必检出");

    // 封账拒写
    let j7 = {
        ledger.seal();
        ledger.enroll(77, "late") == Err(DL_SEALED)
    };
    s.add("J7-封账拒写", j7, "封账后分账登记必拒");

    // 指纹稳定
    let j8 = ledger.fold() == ledger.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 未登记差异进终局裁定负路径
    let j9 = {
        let mut c = DualLedger::new("neg");
        for i in 1..=SET_ANCHOR as u32 {
            let _ = c.enroll(i, "x");
            let _ = c.put_sim(i, true);
            let _ = c.put_real(i, true);
        }
        let _ = c.put_real(SET_ANCHOR as u32, false);
        !c.all_settled() && c.undiffed_count() == 1
    };
    s.add("J9-未差异进裁定", j9, "一项未登记差异即终局不通过");

    // 未知 id 拒
    let j10 = {
        let mut c = DualLedger::new("unk");
        let _ = c.enroll(1, "a");
        c.put_sim(2, true) == Err(DL_DUP_ID)
            && c.put_real(2, true) == Err(DL_DUP_ID)
            && c.note_diff(2) == Err(DL_DUP_ID)
    };
    s.add("J10-未知项拒", j10, "未登记 id 一律拒");

    // 结果三态编码：None=0、false=1、true=2（fold 口径单调）
    let j11 = {
        let mut a = DualLedger::new("enc-a");
        let _ = a.enroll(1, "x");
        let mut b = DualLedger::new("enc-b");
        let _ = b.enroll(1, "x");
        let _ = b.put_real(1, false);
        let mut c = DualLedger::new("enc-c");
        let _ = c.enroll(1, "x");
        let _ = c.put_real(1, true);
        a.fold() != b.fold() && b.fold() != c.fold()
    };
    s.add("J11-结果编码分辨", j11, "无结果/红/绿三种状态指纹可分辨");

    // 明细落账含两档字样
    let j12 = {
        let r = ledger.items[0].render();
        r.contains("sim=G") && r.contains("real=G")
    };
    s.add("J12-明细落账", j12, "渲染含两档结果字样");

    // 差异登记后 settled 成立
    let j13 = {
        let mut c = DualLedger::new("settled");
        let _ = c.enroll(1, "a");
        let _ = c.put_sim(1, true);
        let _ = c.put_real(1, false);
        let before = c.items[0].settled();
        let _ = c.note_diff(1);
        !before && c.items[0].settled()
    };
    s.add("J13-差异登记生效", j13, "登记差异前后合规位翻转");

    // 只读不改账
    let j14 = {
        let mut c = DualLedger::new("stable");
        let _ = c.enroll(1, "a");
        let f1 = c.fold();
        let _ = c.one_leg_count();
        let _ = c.diff_ids();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-账面指纹落账",
        ledger.fold() != 0,
        &format!("fold={:#x}", ledger.fold()),
    );
    let empty = DualLedger::new("empty").fold();
    s.add_dyn(
        "J16-空册指纹",
        empty == 0x1b9600u32.wrapping_mul(31),
        &format!("fold={:#x}", empty),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10375_dual_flow() {
        let mut l = DualLedger::new("t");
        assert!(l.enroll(1, "a").is_ok());
        assert!(l.put_sim(1, true).is_ok());
        assert!(l.note_diff(1) == Err(DL_ONE_LEG));
        assert!(l.put_real(1, false).is_ok());
        assert!(l.items[0].undiffed());
        assert!(l.note_diff(1).is_ok());
        assert!(l.items[0].settled());
        l.seal();
        assert!(l.enroll(2, "b") == Err(DL_SEALED));
    }

    #[test]
    fn unx10375_checks_green() {
        let s = run_unx10375_checks();
        assert!(s.all_passed(), "UNX-F10375 判据未全绿");
    }
}
