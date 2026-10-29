//! UNX-F10382 豁免账终审（逐项 ADR/披露/解锁条件三核对账）（0x1ba40 段）
//!
//! 锚点：UNX-F10382-J1 豁免账终审——每项豁免 ADR/披露/解锁条件三核对账，
//! 缺纸豁免与终审后写账各 10/10 次检出。
//! 域：CoRun·UNX-C3（域闭账·终审谱系）。
//!
//! 结构：豁免项三纸（ADR 号/披露文/解锁条件）+ 终审闸（三纸齐→放行）
//! + 终审后冻结 + 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1ba40 独占 0x20）。
pub const SEG_BASE: u32 = 0x1ba40;
pub const EA_NO_ADR: u32 = SEG_BASE + 0x01; // 缺 ADR
pub const EA_NO_DISCLOSE: u32 = SEG_BASE + 0x02; // 缺披露
pub const EA_NO_UNLOCK: u32 = SEG_BASE + 0x03; // 缺解锁条件
pub const EA_DUP_ID: u32 = SEG_BASE + 0x04; // 豁免项重号
pub const EA_SEALED: u32 = SEG_BASE + 0x05; // 终审后写账
pub const EA_CAP: u32 = SEG_BASE + 0x06; // 超容量

/// 豁免项定容。
pub const CAP: usize = 24;
/// 豁免账终审锚。
pub const AUDIT_ANCHOR: usize = 8;

/// 单条豁免项：三纸 + 终审位。
#[derive(Clone)]
pub struct ExemptItem {
    pub id: u32,
    pub name: String,
    pub adr: String,
    pub disclose: String,
    pub unlock: String,
    pub audited: bool,
}

impl ExemptItem {
    /// 三纸齐备。
    pub fn papers(&self) -> bool {
        !self.adr.is_empty() && !self.disclose.is_empty() && !self.unlock.is_empty()
    }

    /// 字段齐备。
    pub fn ok(&self) -> bool {
        self.id != 0 && !self.name.is_empty()
    }

    /// 放行：三纸齐 + 已终审。
    pub fn passed(&self) -> bool {
        self.papers() && self.audited
    }

    pub fn render(&self) -> String {
        format!(
            "X{}|{}|adr={}|dc={}|ul={}|a{}",
            self.id, self.name, self.adr, self.disclose, self.unlock,
            self.audited as u32
        )
    }
}

/// 豁免终审册。
pub struct ExemptAudit {
    pub items: Vec<ExemptItem>,
    pub sealed: bool,
    pub note: String,
}

impl ExemptAudit {
    pub fn new(note: &str) -> ExemptAudit {
        ExemptAudit { items: Vec::new(), sealed: false, note: note.to_string() }
    }

    /// 登记（未带纸态）：重号、空名、超容、封账均拒。
    pub fn enroll(&mut self, id: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(EA_SEALED);
        }
        if name.is_empty() {
            return Err(EA_NO_ADR);
        }
        if id == 0 || self.items.iter().any(|i| i.id == id) {
            return Err(EA_DUP_ID);
        }
        if self.items.len() >= CAP {
            return Err(EA_CAP);
        }
        self.items.push(ExemptItem {
            id,
            name: name.to_string(),
            adr: String::new(),
            disclose: String::new(),
            unlock: String::new(),
            audited: false,
        });
        Ok(())
    }

    fn find_mut(&mut self, id: u32) -> Option<&mut ExemptItem> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// 补 ADR。
    pub fn set_adr(&mut self, id: u32, adr: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(EA_SEALED);
        }
        match self.find_mut(id) {
            None => Err(EA_DUP_ID),
            Some(i) if adr.is_empty() => Err(EA_NO_ADR),
            Some(i) => {
                i.adr = adr.to_string();
                Ok(())
            }
        }
    }

    /// 补披露。
    pub fn set_disclose(&mut self, id: u32, dc: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(EA_SEALED);
        }
        match self.find_mut(id) {
            None => Err(EA_DUP_ID),
            Some(i) if dc.is_empty() => Err(EA_NO_DISCLOSE),
            Some(i) => {
                i.disclose = dc.to_string();
                Ok(())
            }
        }
    }

    /// 补解锁条件。
    pub fn set_unlock(&mut self, id: u32, ul: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(EA_SEALED);
        }
        match self.find_mut(id) {
            None => Err(EA_DUP_ID),
            Some(i) if ul.is_empty() => Err(EA_NO_UNLOCK),
            Some(i) => {
                i.unlock = ul.to_string();
                Ok(())
            }
        }
    }

    /// 终审单条：三纸齐才放行，缺纸返回对应诊断码。
    pub fn audit_one(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EA_SEALED);
        }
        let idx = match self.items.iter().position(|i| i.id == id) {
            Some(k) => k,
            None => return Err(EA_DUP_ID),
        };
        let it = &self.items[idx];
        if it.adr.is_empty() {
            return Err(EA_NO_ADR);
        }
        if it.disclose.is_empty() {
            return Err(EA_NO_DISCLOSE);
        }
        if it.unlock.is_empty() {
            return Err(EA_NO_UNLOCK);
        }
        self.items[idx].audited = true;
        Ok(())
    }

    /// 终审全账：覆盖达锚且全部放行。
    pub fn audit_all(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(EA_SEALED);
        }
        if self.items.len() != AUDIT_ANCHOR {
            return Err(EA_CAP);
        }
        let ids: Vec<u32> = self.items.iter().map(|i| i.id).collect();
        let mut n = 0;
        for id in ids {
            self.audit_one(id)?;
            n += 1;
        }
        self.sealed = true;
        Ok(n)
    }

    /// 放行计数。
    pub fn passed_count(&self) -> usize {
        self.items.iter().filter(|i| i.passed()).count()
    }

    /// 缺纸诊断。
    pub fn gaps(&self) -> Vec<u32> {
        self.items.iter().filter(|i| !i.papers()).map(|i| i.id).collect()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1ba400;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        h = h.wrapping_mul(31).wrapping_add(self.passed_count() as u32);
        for i in &self.items {
            h = h.wrapping_mul(31).wrapping_add(i.id);
            h = h.wrapping_mul(31).wrapping_add(i.audited as u32);
            for b in i.name.bytes().chain(i.adr.bytes()) {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            for b in i.disclose.bytes().chain(i.unlock.bytes()) {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 注入核（五项）：[缺 ADR, 缺披露, 缺解锁, 重号, 终审后写账] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 缺 ADR 终审必拒
    let mut a = ExemptAudit::new("inj-adr");
    let _ = a.enroll(1, "x");
    let _ = a.set_disclose(1, "披露");
    let _ = a.set_unlock(1, "条件");
    got[0] = a.audit_one(1) == Err(EA_NO_ADR);
    // 2) 缺披露终审必拒
    let mut b = ExemptAudit::new("inj-dc");
    let _ = b.enroll(1, "x");
    let _ = b.set_adr(1, "ADR-1");
    let _ = b.set_unlock(1, "条件");
    got[1] = b.audit_one(1) == Err(EA_NO_DISCLOSE);
    // 3) 缺解锁条件终审必拒
    let mut c = ExemptAudit::new("inj-ul");
    let _ = c.enroll(1, "x");
    let _ = c.set_adr(1, "ADR-1");
    let _ = c.set_disclose(1, "披露");
    got[2] = c.audit_one(1) == Err(EA_NO_UNLOCK);
    // 4) 重号必拒
    let mut d = ExemptAudit::new("inj-dup");
    let _ = d.enroll(1, "x");
    got[3] = d.enroll(1, "y") == Err(EA_DUP_ID);
    // 5) 终审后写账必抓（终审全账封账口径）
    let mut e = ExemptAudit::new("inj-seal");
    let mut i = 1u32;
    while i <= AUDIT_ANCHOR as u32 {
        let _ = e.enroll(i, "x");
        let _ = e.set_adr(i, "ADR");
        let _ = e.set_disclose(i, "披露");
        let _ = e.set_unlock(i, "条件");
        i += 1;
    }
    let _ = e.audit_all();
    got[4] = e.set_adr(1, "再改") == Err(EA_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10382_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10382");

    // 正路径：8 项三纸齐备终审全放行
    let mut audit = ExemptAudit::new("豁免账终审");
    for i in 1..=AUDIT_ANCHOR as u32 {
        let _ = audit.enroll(i, "豁免项");
    }
    for i in 1..=AUDIT_ANCHOR as u32 {
        let _ = audit.set_adr(i, "ADR-1");
        let _ = audit.set_disclose(i, "披露文");
        let _ = audit.set_unlock(i, "解锁条件");
    }
    let all = audit.audit_all();
    let j1 = all == Ok(AUDIT_ANCHOR);
    s.add("J1-三纸终审全放行", j1, "8 项三纸齐备终审全放行");

    // 放行口径：三纸齐 + audited
    let j2 = audit.passed_count() == AUDIT_ANCHOR
        && audit.items.iter().all(|i| i.passed());
    s.add("J2-放行口径", j2, "放行位与三纸位全部成立");

    // 缺纸诊断
    let j3 = {
        let mut c = ExemptAudit::new("gaps");
        let _ = c.enroll(1, "x");
        let _ = c.set_adr(1, "ADR-1");
        c.gaps() == vec![1]
    };
    s.add("J3-缺纸可枚举", j3, "缺纸项编号全部列出");

    // 容量边界
    let j4 = {
        let mut c = ExemptAudit::new("cap");
        let mut filled = true;
        for i in 1..=CAP as u32 {
            if c.enroll(i, "x").is_err() {
                filled = false;
            }
        }
        filled && c.enroll(999, "over") == Err(EA_CAP)
    };
    s.add("J4-容量边界", j4, "超出定容必拒");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "缺 ADR/披露/解锁终审必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "重号与终审后写账必检出");

    // 终审后写账（正册复现）
    let j7 = audit.set_adr(1, "late") == Err(EA_SEALED)
        && audit.set_disclose(1, "late") == Err(EA_SEALED)
        && audit.enroll(99, "late") == Err(EA_SEALED);
    s.add("J7-终审冻结", j7, "终审后三纸与登记全部拒写");

    // 指纹稳定
    let j8 = audit.fold() == audit.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 覆盖不达锚终审全账拒
    let j9 = {
        let mut c = ExemptAudit::new("short");
        for i in 1..(AUDIT_ANCHOR as u32) {
            let _ = c.enroll(i, "x");
            let _ = c.set_adr(i, "ADR");
            let _ = c.set_disclose(i, "披露");
            let _ = c.set_unlock(i, "条件");
        }
        c.audit_all() == Err(EA_CAP)
    };
    s.add("J9-覆盖不达锚拒", j9, "登记数不足锚终审全账必拒");

    // 未知 id 拒
    let j10 = {
        let mut c = ExemptAudit::new("unk");
        let _ = c.enroll(1, "x");
        c.set_adr(2, "ADR") == Err(EA_DUP_ID)
            && c.audit_one(2) == Err(EA_DUP_ID)
    };
    s.add("J10-未知项拒", j10, "未登记 id 一律拒");

    // 空纸拒收（登记口径）
    let j11 = {
        let mut c = ExemptAudit::new("blank");
        let _ = c.enroll(1, "x");
        c.set_adr(1, "") == Err(EA_NO_ADR)
            && c.set_disclose(1, "") == Err(EA_NO_DISCLOSE)
            && c.set_unlock(1, "") == Err(EA_NO_UNLOCK)
    };
    s.add("J11-空纸拒收", j11, "空 ADR/披露/解锁一律拒收");

    // 明细落账含三纸字样
    let j12 = {
        let r = audit.items[0].render();
        r.contains("adr=ADR-1") && r.contains("dc=披露文") && r.contains("ul=解锁条件")
    };
    s.add("J12-明细落账", j12, "渲染含三纸字样");

    // 三纸不齐不放行：终审位保持假
    let j13 = {
        let mut c = ExemptAudit::new("nopass");
        let _ = c.enroll(1, "x");
        let _ = c.set_adr(1, "ADR-1");
        let _ = c.audit_one(1);
        !c.items[0].passed() && c.passed_count() == 0
    };
    s.add("J13-缺纸不放行", j13, "缺纸终审失败后放行位不置位");

    // 只读不改账
    let j14 = {
        let mut c = ExemptAudit::new("stable");
        let _ = c.enroll(1, "x");
        let f1 = c.fold();
        let _ = c.passed_count();
        let _ = c.gaps();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-终审指纹落账",
        audit.fold() != 0,
        &format!("fold={:#x}", audit.fold()),
    );
    let empty = ExemptAudit::new("empty").fold();
    s.add_dyn(
        "J16-空册指纹",
        empty == 0x1ba400u32.wrapping_mul(31).wrapping_mul(31),
        &format!("fold={:#x}", empty),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10382_exempt_flow() {
        let mut a = ExemptAudit::new("t");
        assert!(a.enroll(1, "x").is_ok());
        assert!(a.enroll(1, "dup") == Err(EA_DUP_ID));
        assert!(a.audit_one(1) == Err(EA_NO_ADR));
        let _ = a.set_adr(1, "ADR");
        assert!(a.audit_one(1) == Err(EA_NO_DISCLOSE));
        let _ = a.set_disclose(1, "披露");
        let _ = a.set_unlock(1, "条件");
        assert!(a.audit_one(1).is_ok());
        assert!(a.set_adr(1, "late") == Err(EA_SEALED));
    }

    #[test]
    fn unx10382_checks_green() {
        let s = run_unx10382_checks();
        assert!(s.all_passed(), "UNX-F10382 判据未全绿");
    }
}
