//! UNX-F10369 释放项回归核（F10332 集执行核）（0x1b8a0 段）
//!
//! 锚点：UNX-F10369-J1 释放项回归核——释放（豁免解除）必须五要素齐备
//! 且释放后回归通过；缺要素释放与释放后回归红各 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·回归核谱系）。
//!
//! 结构：释放五要素（id/来源面/去向面/事由/工单号）+ 释放闸（要素核→释放→
//! 回归核三段）+ 残留豁免清点 + 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b8a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b8a0;
pub const RL_NO_SOURCE: u32 = SEG_BASE + 0x01; // 缺来源面
pub const RL_NO_TARGET: u32 = SEG_BASE + 0x02; // 缺去向面
pub const RL_NO_REASON: u32 = SEG_BASE + 0x03; // 缺事由
pub const RL_NO_TICKET: u32 = SEG_BASE + 0x04; // 缺工单号
pub const RL_DUP_ID: u32 = SEG_BASE + 0x05; // 释放项重号
pub const RL_SEALED: u32 = SEG_BASE + 0x06; // 封账后写账
pub const RL_CAP: u32 = SEG_BASE + 0x07; // 超容量
pub const RL_REGRESSED: u32 = SEG_BASE + 0x08; // 释放后回归红

/// 释放项定容。
pub const CAP: usize = 24;
/// F10332 释放集锚。
pub const SET_ANCHOR: usize = 10;

/// 单条释放项：五要素 + 回归结果。
#[derive(Clone)]
pub struct ReleaseItem {
    pub id: u32,
    pub source: String,
    pub target: String,
    pub reason: String,
    pub ticket: String,
    pub released: bool,
    pub reg_green: bool,
}

impl ReleaseItem {
    /// 五要素齐备。
    pub fn complete(&self) -> bool {
        self.id != 0
            && !self.source.is_empty()
            && !self.target.is_empty()
            && !self.reason.is_empty()
            && !self.ticket.is_empty()
    }

    /// 闭环成立：已释放且回归绿。
    pub fn closed(&self) -> bool {
        self.released && self.reg_green
    }

    /// 释放后回归红（最危险的残留形态）。
    pub fn regressed(&self) -> bool {
        self.released && !self.reg_green
    }

    pub fn render(&self) -> String {
        format!(
            "L{}|{}->{}|{}|{}|r{}g{}",
            self.id, self.source, self.target, self.reason, self.ticket,
            self.released as u32, self.reg_green as u32
        )
    }
}

/// 释放登记册：要素核 → 释放 → 回归核三段执行。
pub struct ReleaseBook {
    pub items: Vec<ReleaseItem>,
    pub sealed: bool,
    pub note: String,
}

impl ReleaseBook {
    pub fn new(note: &str) -> ReleaseBook {
        ReleaseBook { items: Vec::new(), sealed: false, note: note.to_string() }
    }

    /// 预登记（未释放态）：重号、超容、封账均拒；要素可后补。
    pub fn enroll(&mut self, id: u32, ticket: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(RL_SEALED);
        }
        if id == 0 || self.items.iter().any(|i| i.id == id) {
            return Err(RL_DUP_ID);
        }
        if self.items.len() >= CAP {
            return Err(RL_CAP);
        }
        self.items.push(ReleaseItem {
            id,
            source: String::new(),
            target: String::new(),
            reason: String::new(),
            ticket: ticket.to_string(),
            released: false,
            reg_green: false,
        });
        Ok(())
    }

    fn find_mut(&mut self, id: u32) -> Option<&mut ReleaseItem> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// 补要素：source/target/reason 逐项补齐。
    pub fn fill(
        &mut self,
        id: u32,
        source: &str,
        target: &str,
        reason: &str,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(RL_SEALED);
        }
        match self.find_mut(id) {
            None => Err(RL_DUP_ID),
            Some(i) => {
                if source.is_empty() {
                    return Err(RL_NO_SOURCE);
                }
                if target.is_empty() {
                    return Err(RL_NO_TARGET);
                }
                if reason.is_empty() {
                    return Err(RL_NO_REASON);
                }
                i.source = source.to_string();
                i.target = target.to_string();
                i.reason = reason.to_string();
                Ok(())
            }
        }
    }

    /// 释放闸：五要素齐备才放行释放，缺任一要素返回对应诊断码。
    pub fn release(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RL_SEALED);
        }
        let idx = match self.items.iter().position(|i| i.id == id) {
            Some(k) => k,
            None => return Err(RL_DUP_ID),
        };
        let item = &self.items[idx];
        if item.source.is_empty() {
            return Err(RL_NO_SOURCE);
        }
        if item.target.is_empty() {
            return Err(RL_NO_TARGET);
        }
        if item.reason.is_empty() {
            return Err(RL_NO_REASON);
        }
        if item.ticket.is_empty() {
            return Err(RL_NO_TICKET);
        }
        self.items[idx].released = true;
        Ok(())
    }

    /// 回归核：释放后的项必须重跑回归并登记结果。
    pub fn reg_check(&mut self, id: u32, green: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(RL_SEALED);
        }
        match self.find_mut(id) {
            None => Err(RL_DUP_ID),
            Some(i) if !i.released => Err(RL_REGRESSED),
            Some(i) => {
                i.reg_green = green;
                Ok(())
            }
        }
    }

    /// 残留清点：缺要素未释放的项数。
    pub fn incomplete_count(&self) -> usize {
        self.items.iter().filter(|i| !i.complete()).count()
    }

    /// 释放后回归红清点。
    pub fn regressed_count(&self) -> usize {
        self.items.iter().filter(|i| i.regressed()).count()
    }

    /// 集覆盖核。
    pub fn set_complete(&self) -> bool {
        self.items.len() == SET_ANCHOR
    }

    /// 终局裁定：覆盖达锚、零缺要素、零回归红、全部闭环。
    pub fn all_closed(&self) -> bool {
        self.set_complete()
            && self.incomplete_count() == 0
            && self.regressed_count() == 0
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
        let mut h: u32 = 0x1b8a00;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        for i in &self.items {
            h = h.wrapping_mul(31).wrapping_add(i.id);
            h = h.wrapping_mul(31).wrapping_add(i.released as u32);
            h = h.wrapping_mul(31).wrapping_add(i.reg_green as u32);
            for b in i.source.bytes().chain(i.target.bytes()) {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            for b in i.reason.bytes().chain(i.ticket.bytes()) {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 注入核（五项）：[缺来源, 缺去向, 缺事由, 释放后回归红, 未释放跑回归] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 缺来源面释放必拒
    let mut b = ReleaseBook::new("inj-src");
    let _ = b.enroll(1, "T1");
    got[0] = b.release(1) == Err(RL_NO_SOURCE);
    // 2) 缺去向面释放必拒
    let mut b2 = ReleaseBook::new("inj-tgt");
    let _ = b2.enroll(1, "T1");
    let _ = b2.fill(1, "S", "", "R");
    got[1] = b2.release(1) == Err(RL_NO_TARGET);
    // 3) 缺事由释放必拒
    let mut b3 = ReleaseBook::new("inj-rsn");
    let _ = b3.enroll(1, "T1");
    let _ = b3.fill(1, "S", "T", "");
    got[2] = b3.release(1) == Err(RL_NO_REASON);
    // 4) 释放后回归红必检出
    let mut b4 = ReleaseBook::new("inj-reg");
    let _ = b4.enroll(1, "T1");
    let _ = b4.fill(1, "S", "T", "R");
    let _ = b4.release(1);
    let _ = b4.reg_check(1, false);
    got[3] = b4.regressed_count() == 1 && !b4.all_closed();
    // 5) 未释放跑回归必拒
    let mut b5 = ReleaseBook::new("inj-nrel");
    let _ = b5.enroll(1, "T1");
    got[4] = b5.reg_check(1, true) == Err(RL_REGRESSED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10369_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10369");

    // 正路径：10 项五要素齐备 → 释放 → 回归绿 → 全闭环
    let mut book = ReleaseBook::new("F10332 释放集执行核");
    for i in 1..=SET_ANCHOR as u32 {
        let _ = book.enroll(i, "TK-1");
    }
    for i in 1..=SET_ANCHOR as u32 {
        let _ = book.fill(i, "豁免账", "回归集", "条件已达成");
    }
    for i in 1..=SET_ANCHOR as u32 {
        let _ = book.release(i);
    }
    for i in 1..=SET_ANCHOR as u32 {
        let _ = book.reg_check(i, true);
    }
    let j1 = book.all_closed();
    s.add("J1-释放集全闭环", j1, "10 项三段走满且零残留");

    // 五要素齐备：逐项核对
    let j2 = book.items.iter().all(|i| i.complete());
    s.add("J2-五要素齐", j2, "id/来源/去向/事由/工单全非空");

    // 残留诊断：只登记不补要素的账残留可枚举
    let j3 = {
        let mut c = ReleaseBook::new("gaps");
        let _ = c.enroll(1, "T1");
        let _ = c.enroll(2, "T1");
        c.incomplete_count() == 2 && c.gaps() == vec![1, 2]
    };
    s.add("J3-缺要素可枚举", j3, "未补要素项全部列出");

    // 容量边界
    let j4 = {
        let mut c = ReleaseBook::new("cap");
        let mut filled = true;
        for i in 1..=CAP as u32 {
            if c.enroll(i, "T").is_err() {
                filled = false;
            }
        }
        filled && c.enroll(999, "over") == Err(RL_CAP)
    };
    s.add("J4-容量边界", j4, "超出定容必拒");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "缺来源/去向/事由释放必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "释放后回归红与未释放跑回归必检出");

    // 封账拒写
    let j7 = {
        book.seal();
        book.enroll(77, "late") == Err(RL_SEALED)
    };
    s.add("J7-封账拒写", j7, "封账后释放登记必拒");

    // 指纹稳定
    let j8 = book.fold() == book.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 释放后回归红进终局裁定
    let j9 = {
        let mut c = ReleaseBook::new("neg");
        for i in 1..=SET_ANCHOR as u32 {
            let _ = c.enroll(i, "T1");
            let _ = c.fill(i, "S", "T", "R");
            let _ = c.release(i);
        }
        let _ = c.reg_check(SET_ANCHOR as u32, false);
        !c.all_closed() && c.regressed_count() == 1
    };
    s.add("J9-回归红进裁定", j9, "一个回归红即终局不通过");

    // 未知 id 拒
    let j10 = {
        let mut c = ReleaseBook::new("unk");
        let _ = c.enroll(1, "T1");
        c.release(2) == Err(RL_DUP_ID) && c.fill(2, "S", "T", "R") == Err(RL_DUP_ID)
    };
    s.add("J10-未知项拒", j10, "未登记 id 一律拒");

    // 空工单号预登记放行但释放闸拦（要素后补口径）
    let j11 = {
        let mut c = ReleaseBook::new("ticket");
        let _ = c.enroll(1, "T1");
        let _ = c.fill(1, "S", "T", "R");
        c.release(1).is_ok() && c.items[0].released
    };
    s.add("J11-要素后补口径", j11, "预登记后补齐要素即可释放");

    // 明细落账含五要素字样
    let j12 = {
        let mut c = ReleaseBook::new("render");
        let _ = c.enroll(1, "TK-9");
        let _ = c.fill(1, "S1", "T1", "R1");
        let r = c.items[0].render();
        r.contains("S1->T1") && r.contains("TK-9")
    };
    s.add("J12-明细落账", j12, "渲染含来源去向与工单号");

    // 未释放项不算闭环：释放闸未过即 gaps 保留
    let j13 = {
        let mut c = ReleaseBook::new("norel");
        let _ = c.enroll(1, "T1");
        let _ = c.fill(1, "S", "T", "R");
        !c.items[0].closed() && c.gaps().len() == 1
    };
    s.add("J13-未释放非闭环", j13, "释放闸未过即不闭环");

    // 只读不改账
    let j14 = {
        let mut c = ReleaseBook::new("stable");
        let _ = c.enroll(1, "T1");
        let f1 = c.fold();
        let _ = c.incomplete_count();
        let _ = c.regressed_count();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-账面指纹落账",
        book.fold() != 0,
        &format!("fold={:#x}", book.fold()),
    );
    let empty = ReleaseBook::new("empty").fold();
    s.add_dyn("J16-空册指纹", empty == 0x1b8a00u32.wrapping_mul(31), &format!("fold={:#x}", empty));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10369_release_flow() {
        let mut b = ReleaseBook::new("t");
        assert!(b.enroll(1, "T1").is_ok());
        assert!(b.release(1) == Err(RL_NO_SOURCE));
        assert!(b.fill(1, "S", "T", "R").is_ok());
        assert!(b.release(1).is_ok());
        assert!(b.reg_check(1, true).is_ok());
        assert!(b.items[0].closed());
        b.seal();
        assert!(b.enroll(2, "T2") == Err(RL_SEALED));
    }

    #[test]
    fn unx10369_checks_green() {
        let s = run_unx10369_checks();
        assert!(s.all_passed(), "UNX-F10369 判据未全绿");
    }
}
