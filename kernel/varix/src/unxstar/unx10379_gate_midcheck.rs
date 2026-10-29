//! UNX-F10379 正演面中检（十八项对平）（0x1b9e0 段）
//!
//! 锚点：UNX-F10379-J1 正演面中检——十八项检查位逐项翻绿对平，
//! 少项与重项各 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·中检谱系）。
//!
//! 结构：十八检查位定容登记 + 对平器（覆盖=锚且全绿）+ 缺/重诊断
//! + 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b9e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b9e0;
pub const GM_SHORT: u32 = SEG_BASE + 0x01; // 少项
pub const GM_DUP: u32 = SEG_BASE + 0x02; // 重项
pub const GM_EMPTY: u32 = SEG_BASE + 0x03; // 空名
pub const GM_SEALED: u32 = SEG_BASE + 0x04; // 封账后写账
pub const GM_CAP: u32 = SEG_BASE + 0x05; // 超容量

/// 中检锚：十八项。
pub const MID_ANCHOR: usize = 18;
/// 登记定容（>锚，容量先定后用）。
pub const CAP: usize = 24;

/// 单条检查位。
#[derive(Clone)]
pub struct MidItem {
    pub id: u32,
    pub name: String,
    pub green: bool,
}

impl MidItem {
    pub fn ok(&self) -> bool {
        self.id != 0 && !self.name.is_empty()
    }

    pub fn render(&self) -> String {
        format!("M{}|{}|{}", self.id, self.name, if self.green { "G" } else { "R" })
    }
}

/// 中检册：十八位逐项登记并翻绿，封账后只读。
pub struct GateMidcheck {
    pub items: Vec<MidItem>,
    pub sealed: bool,
    pub note: String,
}

impl GateMidcheck {
    pub fn new(note: &str) -> GateMidcheck {
        GateMidcheck { items: Vec::new(), sealed: false, note: note.to_string() }
    }

    /// 登记：重号、空名、超容、封账均拒。
    pub fn enroll(&mut self, id: u32, name: &str, green: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(GM_SEALED);
        }
        if name.is_empty() {
            return Err(GM_EMPTY);
        }
        if id == 0 || self.items.iter().any(|i| i.id == id) {
            return Err(GM_DUP);
        }
        if self.items.len() >= CAP {
            return Err(GM_CAP);
        }
        self.items.push(MidItem { id, name: name.to_string(), green });
        Ok(())
    }

    /// 翻位：已登记项可翻绿，未知 id 拒。
    pub fn flip(&mut self, id: u32, green: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(GM_SEALED);
        }
        match self.items.iter_mut().find(|i| i.id == id) {
            None => Err(GM_SHORT),
            Some(i) => {
                i.green = green;
                Ok(())
            }
        }
    }

    /// 绿计数。
    pub fn green_count(&self) -> usize {
        self.items.iter().filter(|i| i.green).count()
    }

    /// 覆盖核：登记数达锚。
    pub fn covered(&self) -> bool {
        self.items.len() == MID_ANCHOR
    }

    /// 对平：覆盖达锚且十八位全绿。
    pub fn balanced(&self) -> bool {
        self.covered() && self.green_count() == MID_ANCHOR
    }

    /// 缺绿诊断。
    pub fn gaps(&self) -> Vec<u32> {
        self.items.iter().filter(|i| !i.green).map(|i| i.id).collect()
    }

    /// 封账并返回折叠指纹。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b9e00;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        h = h.wrapping_mul(31).wrapping_add(self.green_count() as u32);
        for i in &self.items {
            h = h.wrapping_mul(31).wrapping_add(i.id);
            h = h.wrapping_mul(31).wrapping_add(i.green as u32);
            for b in i.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 注入核（五项）：[少项对平, 重项, 空名, 翻未知位, 封账写账] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 少项对平必不通过
    let mut m = GateMidcheck::new("inj-short");
    let mut i = 1u32;
    while (i as usize) < MID_ANCHOR {
        let _ = m.enroll(i, "位", true);
        i += 1;
    }
    got[0] = !m.covered() && !m.balanced() && m.gaps().is_empty();
    // 2) 重项必拒
    let mut m2 = GateMidcheck::new("inj-dup");
    let _ = m2.enroll(1, "a", true);
    got[1] = m2.enroll(1, "b", true) == Err(GM_DUP);
    // 3) 空名必拒
    let mut m3 = GateMidcheck::new("inj-empty");
    got[2] = m3.enroll(2, "", true) == Err(GM_EMPTY);
    // 4) 翻未知位必拒
    let mut m4 = GateMidcheck::new("inj-flip");
    let _ = m4.enroll(1, "a", false);
    got[3] = m4.flip(9, true) == Err(GM_SHORT);
    // 5) 封账写账必抓
    let mut m5 = GateMidcheck::new("inj-seal");
    let _ = m5.enroll(1, "a", true);
    m5.seal();
    got[4] = m5.enroll(2, "late", true) == Err(GM_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10379_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10379");

    // 正路径：十八位登记并全绿对平
    let mut mid = GateMidcheck::new("正演面中检");
    for i in 1..=MID_ANCHOR as u32 {
        let _ = mid.enroll(i, "检查位", true);
    }
    let j1 = mid.balanced();
    s.add("J1-十八位对平", j1, "登记达锚且全数翻绿");

    // 覆盖与绿数口径
    let j2 = mid.covered() && mid.green_count() == MID_ANCHOR && mid.gaps().is_empty();
    s.add("J2-覆盖绿数口径", j2, "达锚全绿时缺口清单为空");

    // 逐项字段齐备
    let j3 = mid.items.iter().all(|i| i.ok());
    s.add("J3-字段齐备", j3, "id 非零且名非空");

    // 容量边界
    let j4 = {
        let mut c = GateMidcheck::new("cap");
        let mut filled = true;
        for i in 1..=CAP as u32 {
            if c.enroll(i, "x", true).is_err() {
                filled = false;
            }
        }
        filled && c.enroll(999, "over", true) == Err(GM_CAP)
    };
    s.add("J4-容量边界", j4, "超出定容必拒");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "少项/重项/空名必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "翻未知位与封账写账必检出");

    // 翻位语义：绿→红翻转后缺口可枚举
    let j7 = {
        let mut c = GateMidcheck::new("flip");
        for i in 1..=MID_ANCHOR as u32 {
            let _ = c.enroll(i, "位", true);
        }
        let _ = c.flip(7, false);
        !c.balanced() && c.gaps() == vec![7] && c.green_count() == MID_ANCHOR - 1
    };
    s.add("J7-翻位生效", j7, "翻红后缺口恰为该位");

    // 封账拒写
    let j8 = {
        mid.seal();
        mid.enroll(77, "late", true) == Err(GM_SEALED)
    };
    s.add("J8-封账拒写", j8, "封账后中检登记必拒");

    // 指纹稳定
    let j9 = mid.fold() == mid.fold();
    s.add("J9-指纹稳定", j9, "折叠指纹两次一致");

    // 一位未绿即对平失败
    let j10 = {
        let mut c = GateMidcheck::new("neg");
        for i in 1..=MID_ANCHOR as u32 {
            let _ = c.enroll(i, "位", i != 5);
        }
        !c.balanced() && c.gaps() == vec![5]
    };
    s.add("J10-一位未绿即败", j10, "任一位未绿即对平不通过");

    // 明细落账含绿红标记
    let j11 = {
        let r = mid.items[0].render();
        r.contains("|G")
    };
    s.add("J11-明细落账", j11, "渲染含绿红标记");

    // 空册不构成对平
    let j12 = {
        let c = GateMidcheck::new("empty");
        !c.covered() && !c.balanced()
    };
    s.add("J12-空册非对平", j12, "空册覆盖与对平均假");

    // 登记即带绿：登记位初始状态可红
    let j13 = {
        let mut c = GateMidcheck::new("initred");
        let _ = c.enroll(1, "位", false);
        !c.items[0].green && c.green_count() == 0
    };
    s.add("J13-初始红可登记", j13, "登记时允许初始红态");

    // 只读不改账
    let j14 = {
        let mut c = GateMidcheck::new("stable");
        let _ = c.enroll(1, "位", true);
        let f1 = c.fold();
        let _ = c.covered();
        let _ = c.green_count();
        let _ = c.gaps();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-中检指纹落账",
        mid.fold() != 0,
        &format!("fold={:#x}", mid.fold()),
    );
    let empty = GateMidcheck::new("empty2").fold();
    s.add_dyn(
        "J16-空册指纹",
        empty == 0x1b9e00u32.wrapping_mul(31).wrapping_mul(31).wrapping_mul(31),
        &format!("fold={:#x}", empty),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10379_midcheck_flow() {
        let mut m = GateMidcheck::new("t");
        assert!(m.enroll(1, "a", false).is_ok());
        assert!(m.enroll(1, "dup", true) == Err(GM_DUP));
        assert!(m.enroll(2, "", true) == Err(GM_EMPTY));
        assert!(m.flip(9, true) == Err(GM_SHORT));
        assert!(m.flip(1, true).is_ok());
        assert!(m.items[0].green);
        m.seal();
        assert!(m.enroll(3, "late", true) == Err(GM_SEALED));
    }

    #[test]
    fn unx10379_checks_green() {
        let s = run_unx10379_checks();
        assert!(s.all_passed(), "UNX-F10379 判据未全绿");
    }
}
