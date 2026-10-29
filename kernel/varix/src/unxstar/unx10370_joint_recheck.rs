//! UNX-F10370 联签项回归核（F10311 集执行核）（0x1b8c0 段）
//!
//! 锚点：UNX-F10370-J1 联签项回归核——联签项必须双签齐备且回归通过；
//! 单签跑批与双签回归红各 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·回归核谱系）。
//!
//! 结构：双签位（本域签/对方域签）+ 回归结果位 + 联签裁定器
//! + 单签/双签红/重号/空名/封账五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b8c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b8c0;
pub const JT_DUP_ID: u32 = SEG_BASE + 0x01; // 联签项重号
pub const JT_EMPTY_NAME: u32 = SEG_BASE + 0x02; // 项名空
pub const JT_SINGLE_SIGN: u32 = SEG_BASE + 0x03; // 单签跑批
pub const JT_SEALED: u32 = SEG_BASE + 0x04; // 封账后写账
pub const JT_CAP: u32 = SEG_BASE + 0x05; // 超容量
pub const JT_SELF_SIGN: u32 = SEG_BASE + 0x06; // 同域双签（自己签自己）

/// 联签项定容。
pub const CAP: usize = 24;
/// F10311 联签集锚。
pub const SET_ANCHOR: usize = 14;

/// 单条联签项：双签位 + 回归位。
#[derive(Clone)]
pub struct JointItem {
    pub id: u32,
    pub name: String,
    pub sign_a: Option<String>,
    pub sign_b: Option<String>,
    pub reg_green: bool,
}

impl JointItem {
    /// 双签齐备：两个签名域都在且互异。
    pub fn double_signed(&self) -> bool {
        match (&self.sign_a, &self.sign_b) {
            (Some(a), Some(b)) => !a.is_empty() && !b.is_empty() && a != b,
            _ => false,
        }
    }

    /// 单签：恰好一个签名在场。
    pub fn single_signed(&self) -> bool {
        let n = self.sign_a.is_some() as usize + self.sign_b.is_some() as usize;
        n == 1
    }

    /// 闭环：双签齐 + 回归绿。
    pub fn closed(&self) -> bool {
        self.double_signed() && self.reg_green
    }

    pub fn render(&self) -> String {
        let a = self.sign_a.clone().unwrap_or_default();
        let b = self.sign_b.clone().unwrap_or_default();
        format!(
            "J{}|{}|{}x{}|g{}",
            self.id, self.name, a, b, self.reg_green as u32
        )
    }
}

/// 联签登记册。
pub struct JointBook {
    pub items: Vec<JointItem>,
    pub sealed: bool,
    pub note: String,
}

impl JointBook {
    pub fn new(note: &str) -> JointBook {
        JointBook { items: Vec::new(), sealed: false, note: note.to_string() }
    }

    /// 登记（未签态）。
    pub fn enroll(&mut self, id: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(JT_SEALED);
        }
        if name.is_empty() {
            return Err(JT_EMPTY_NAME);
        }
        if id == 0 || self.items.iter().any(|i| i.id == id) {
            return Err(JT_DUP_ID);
        }
        if self.items.len() >= CAP {
            return Err(JT_CAP);
        }
        self.items.push(JointItem {
            id,
            name: name.to_string(),
            sign_a: None,
            sign_b: None,
            reg_green: false,
        });
        Ok(())
    }

    fn find_mut(&mut self, id: u32) -> Option<&mut JointItem> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// 本域签（A 位）：同域双签必拒。
    pub fn sign_a(&mut self, id: u32, domain: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(JT_SEALED);
        }
        match self.find_mut(id) {
            None => Err(JT_DUP_ID),
            Some(i) if i.sign_b.as_deref() == Some(domain) => Err(JT_SELF_SIGN),
            Some(i) => {
                i.sign_a = Some(domain.to_string());
                Ok(())
            }
        }
    }

    /// 对方域签（B 位）：同域双签必拒。
    pub fn sign_b(&mut self, id: u32, domain: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(JT_SEALED);
        }
        match self.find_mut(id) {
            None => Err(JT_DUP_ID),
            Some(i) if i.sign_a.as_deref() == Some(domain) => Err(JT_SELF_SIGN),
            Some(i) => {
                i.sign_b = Some(domain.to_string());
                Ok(())
            }
        }
    }

    /// 回归结果登记：仅双签齐后接受（单签跑批必拒）。
    pub fn reg_check(&mut self, id: u32, green: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(JT_SEALED);
        }
        match self.find_mut(id) {
            None => Err(JT_DUP_ID),
            Some(i) if !i.double_signed() => Err(JT_SINGLE_SIGN),
            Some(i) => {
                i.reg_green = green;
                Ok(())
            }
        }
    }

    /// 单签项清点。
    pub fn single_count(&self) -> usize {
        self.items.iter().filter(|i| i.single_signed()).count()
    }

    /// 双签但回归红清点。
    pub fn double_red_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.double_signed() && !i.reg_green)
            .count()
    }

    /// 集覆盖核。
    pub fn set_complete(&self) -> bool {
        self.items.len() == SET_ANCHOR
    }

    /// 终局裁定：覆盖达锚、零单签、零双签红、全部闭环。
    pub fn all_closed(&self) -> bool {
        self.set_complete()
            && self.single_count() == 0
            && self.double_red_count() == 0
            && self.items.iter().all(|i| i.closed())
    }

    /// 残留诊断：未闭环项编号。
    pub fn gaps(&self) -> Vec<u32> {
        self.items.iter().filter(|i| !i.closed()).map(|i| i.id).collect()
    }

    /// 封账并返回折叠指纹。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b8c00;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        for i in &self.items {
            h = h.wrapping_mul(31).wrapping_add(i.id);
            h = h.wrapping_mul(31).wrapping_add(i.sign_a.is_some() as u32);
            h = h.wrapping_mul(31).wrapping_add(i.sign_b.is_some() as u32);
            h = h.wrapping_mul(31).wrapping_add(i.reg_green as u32);
            for b in i.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            if let Some(a) = &i.sign_a {
                for b in a.bytes() {
                    h = h.wrapping_mul(31).wrapping_add(b as u32);
                }
            }
            if let Some(b) = &i.sign_b {
                for bb in b.bytes() {
                    h = h.wrapping_mul(31).wrapping_add(bb as u32);
                }
            }
        }
        h
    }
}

/// 注入核（五项）：[单签跑批, 同域双签, 重号, 空名, 双签回归红] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 单签跑批必拒
    let mut b = JointBook::new("inj-single");
    let _ = b.enroll(1, "a");
    let _ = b.sign_a(1, "fs");
    got[0] = b.reg_check(1, true) == Err(JT_SINGLE_SIGN) && b.single_count() == 1;
    // 2) 同域双签必拒
    let mut b2 = JointBook::new("inj-self");
    let _ = b2.enroll(1, "a");
    let _ = b2.sign_a(1, "fs");
    got[1] = b2.sign_b(1, "fs") == Err(JT_SELF_SIGN);
    // 3) 重号必拒
    let mut b3 = JointBook::new("inj-dup");
    let _ = b3.enroll(1, "a");
    got[2] = b3.enroll(1, "b") == Err(JT_DUP_ID);
    // 4) 空名必拒
    let mut b4 = JointBook::new("inj-empty");
    got[3] = b4.enroll(2, "") == Err(JT_EMPTY_NAME);
    // 5) 双签但回归红必检出
    let mut b5 = JointBook::new("inj-red");
    let _ = b5.enroll(1, "a");
    let _ = b5.sign_a(1, "fs");
    let _ = b5.sign_b(1, "net");
    let _ = b5.reg_check(1, false);
    got[4] = b5.double_red_count() == 1 && !b5.all_closed();
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10370_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10370");

    // 正路径：14 项双签（fs x net）+ 回归绿 → 全闭环
    let mut book = JointBook::new("F10311 联签集执行核");
    for i in 1..=SET_ANCHOR as u32 {
        let _ = book.enroll(i, "联签项");
    }
    for i in 1..=SET_ANCHOR as u32 {
        let _ = book.sign_a(i, "fs");
        let _ = book.sign_b(i, "net");
    }
    for i in 1..=SET_ANCHOR as u32 {
        let _ = book.reg_check(i, true);
    }
    let j1 = book.all_closed();
    s.add("J1-联签集全闭环", j1, "14 项双签齐且回归全绿");

    // 双签互异：签名域成对出现且不同
    let j2 = book.items.iter().all(|i| i.double_signed());
    s.add("J2-双签互异", j2, "每项两域签名齐且互异");

    // 残留诊断：只单签的账残留可枚举
    let j3 = {
        let mut c = JointBook::new("gaps");
        let _ = c.enroll(1, "a");
        let _ = c.enroll(2, "b");
        let _ = c.sign_a(1, "fs");
        c.single_count() == 1 && c.gaps() == vec![1, 2]
    };
    s.add("J3-残留可枚举", j3, "未闭环项编号全部列出");

    // 容量边界
    let j4 = {
        let mut c = JointBook::new("cap");
        let mut filled = true;
        for i in 1..=CAP as u32 {
            if c.enroll(i, "x").is_err() {
                filled = false;
            }
        }
        filled && c.enroll(999, "over") == Err(JT_CAP)
    };
    s.add("J4-容量边界", j4, "超出定容必拒");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "单签跑批/同域双签/重号必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "空名与双签红必检出");

    // 封账拒写
    let j7 = {
        book.seal();
        book.enroll(77, "late") == Err(JT_SEALED)
    };
    s.add("J7-封账拒写", j7, "封账后联签登记必拒");

    // 指纹稳定
    let j8 = book.fold() == book.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 单签项进终局裁定负路径
    let j9 = {
        let mut c = JointBook::new("neg");
        for i in 1..=SET_ANCHOR as u32 {
            let _ = c.enroll(i, "x");
            let _ = c.sign_a(i, "fs");
            let _ = c.sign_b(i, "net");
            let _ = c.reg_check(i, true);
        }
        let _ = c.reg_check(SET_ANCHOR as u32, false);
        !c.all_closed() && c.double_red_count() == 1
    };
    s.add("J9-双签红进裁定", j9, "一个双签红即终局不通过");

    // 未知 id 拒
    let j10 = {
        let mut c = JointBook::new("unk");
        let _ = c.enroll(1, "a");
        c.sign_a(2, "fs") == Err(JT_DUP_ID) && c.reg_check(2, true) == Err(JT_DUP_ID)
    };
    s.add("J10-未知项拒", j10, "未登记 id 一律拒");

    // 空域签名视同未签（单签口径不变）
    let j11 = {
        let mut c = JointBook::new("blank");
        let _ = c.enroll(1, "a");
        let _ = c.sign_a(1, "");
        !c.single_signed_of(1)
    };
    s.add("J11-空签非签", j11, "空域名不构成有效签名（零签非单签）");

    // 明细落账含双域字样
    let j12 = {
        let r = book.items[0].render();
        r.contains("fsxnet")
    };
    s.add("J12-明细落账", j12, "渲染含双方域名字样");

    // 覆盖不达锚即裁定不通过
    let j13 = {
        let mut c = JointBook::new("short");
        for i in 1..(SET_ANCHOR as u32) {
            let _ = c.enroll(i, "x");
            let _ = c.sign_a(i, "fs");
            let _ = c.sign_b(i, "net");
            let _ = c.reg_check(i, true);
        }
        !c.set_complete() && !c.all_closed()
    };
    s.add("J13-覆盖达锚", j13, "少一项即集不完整");

    // 只读不改账
    let j14 = {
        let mut c = JointBook::new("stable");
        let _ = c.enroll(1, "a");
        let f1 = c.fold();
        let _ = c.single_count();
        let _ = c.double_red_count();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-账面指纹落账",
        book.fold() != 0,
        &format!("fold={:#x}", book.fold()),
    );
    let empty = JointBook::new("empty").fold();
    s.add_dyn("J16-空册指纹", empty == 0x1b8c00u32.wrapping_mul(31), &format!("fold={:#x}", empty));

    s
}

impl JointBook {
    /// 单签判定（含空域签名口径）：恰好一个有效签名在场。
    pub fn single_signed_of(&self, id: u32) -> bool {
        match self.items.iter().find(|i| i.id == id) {
            None => false,
            Some(i) => {
                let valid = |s: &Option<String>| {
                    s.as_deref().map(|d| !d.is_empty()).unwrap_or(false)
                };
                valid(&i.sign_a) != valid(&i.sign_b)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10370_joint_flow() {
        let mut b = JointBook::new("t");
        assert!(b.enroll(1, "a").is_ok());
        assert!(b.sign_a(1, "fs").is_ok());
        assert!(b.sign_b(1, "fs") == Err(JT_SELF_SIGN));
        assert!(b.sign_b(1, "net").is_ok());
        assert!(b.reg_check(1, true).is_ok());
        assert!(b.items[0].closed());
        b.seal();
        assert!(b.enroll(2, "b") == Err(JT_SEALED));
    }

    #[test]
    fn unx10370_checks_green() {
        let s = run_unx10370_checks();
        assert!(s.all_passed(), "UNX-F10370 判据未全绿");
    }
}
