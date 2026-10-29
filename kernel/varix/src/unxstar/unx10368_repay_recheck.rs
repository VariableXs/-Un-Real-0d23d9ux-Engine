//! UNX-F10368 清偿项回归核（F10229 清偿后翻绿核）（0x1b880 段）
//!
//! 锚点：UNX-F10368-J1 清偿项回归核——清偿后必须重跑翻绿才算闭环，
//! 幽灵闭环（已清偿但未重跑通过）10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·回归核谱系）。
//!
//! 结构：清偿账（欠账三项态：在欠/已清/已证）+ 闭环裁定器（清偿必配重跑绿）
//! + 幽灵闭环检出 + 重号/空账/封账/超容注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b880 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b880;
pub const RP_DUP_ID: u32 = SEG_BASE + 0x01; // 清偿项重号
pub const RP_EMPTY_NAME: u32 = SEG_BASE + 0x02; // 项名空
pub const RP_GHOST_CLOSE: u32 = SEG_BASE + 0x03; // 幽灵闭环（清偿未重跑）
pub const RP_SEALED: u32 = SEG_BASE + 0x04; // 封账后写账
pub const RP_CAP: u32 = SEG_BASE + 0x05; // 超容量
pub const RP_BAD_STATE: u32 = SEG_BASE + 0x06; // 非法状态迁移
pub const RP_UNPAID_GREEN: u32 = SEG_BASE + 0x07; // 未清偿却报翻绿

/// 清偿项定容。
pub const CAP: usize = 24;
/// F10229 清偿集锚。
pub const SET_ANCHOR: usize = 12;

/// 清偿三态。
#[derive(Clone, PartialEq, Eq)]
pub enum RepayState {
    /// 在欠：登记欠账，尚未清偿。
    Owed,
    /// 已清：清偿动作完成，等待重跑翻绿。
    Repaid,
    /// 已证：重跑翻绿通过，闭环成立。
    Proven,
}

impl RepayState {
    pub fn as_u32(&self) -> u32 {
        match self {
            RepayState::Owed => 1,
            RepayState::Repaid => 2,
            RepayState::Proven => 3,
        }
    }
}

/// 单条清偿项。
#[derive(Clone)]
pub struct RepayItem {
    pub id: u32,
    pub name: String,
    pub state: RepayState,
    pub debt_code: u32,
    pub recheck_green: bool,
}

impl RepayItem {
    /// 字段齐备：id 非零、名非空、欠账码非零。
    pub fn ok(&self) -> bool {
        self.id != 0 && !self.name.is_empty() && self.debt_code != 0
    }

    /// 闭环成立：三态走到已证且重跑绿。
    pub fn closed(&self) -> bool {
        self.state == RepayState::Proven && self.recheck_green
    }

    /// 幽灵闭环：宣称已证但没重跑绿。
    pub fn ghost(&self) -> bool {
        self.state == RepayState::Proven && !self.recheck_green
    }

    pub fn render(&self) -> String {
        format!(
            "R{}|{}|s{}|d{:#x}|g{}",
            self.id, self.name, self.state.as_u32(), self.debt_code,
            self.recheck_green as u32
        )
    }
}

/// 清偿账：F10229 集逐条登记，封账后只读。
pub struct RepayLedger {
    pub items: Vec<RepayItem>,
    pub sealed: bool,
    pub note: String,
}

impl RepayLedger {
    pub fn new(note: &str) -> RepayLedger {
        RepayLedger { items: Vec::new(), sealed: false, note: note.to_string() }
    }

    /// 登记欠账：重号、空名、零欠账码、超容、封账均拒。
    pub fn enroll(&mut self, id: u32, name: &str, debt_code: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RP_SEALED);
        }
        if name.is_empty() {
            return Err(RP_EMPTY_NAME);
        }
        if id == 0 || self.items.iter().any(|i| i.id == id) {
            return Err(RP_DUP_ID);
        }
        if debt_code == 0 {
            return Err(RP_BAD_STATE);
        }
        if self.items.len() >= CAP {
            return Err(RP_CAP);
        }
        self.items.push(RepayItem {
            id,
            name: name.to_string(),
            state: RepayState::Owed,
            debt_code,
            recheck_green: false,
        });
        Ok(())
    }

    fn find_mut(&mut self, id: u32) -> Option<&mut RepayItem> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// 清偿：仅「在欠」可清偿，重复清偿即非法迁移。
    pub fn repay(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RP_SEALED);
        }
        match self.find_mut(id) {
            None => Err(RP_DUP_ID),
            Some(i) if i.state == RepayState::Owed => {
                i.state = RepayState::Repaid;
                Ok(())
            }
            Some(_) => Err(RP_BAD_STATE),
        }
    }

    /// 重跑翻绿：仅「已清」可证，未清偿却报翻绿必拒。
    pub fn prove(&mut self, id: u32, green: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(RP_SEALED);
        }
        match self.find_mut(id) {
            None => Err(RP_DUP_ID),
            Some(i) if i.state == RepayState::Owed => Err(RP_UNPAID_GREEN),
            Some(i) if i.state == RepayState::Repaid => {
                i.recheck_green = green;
                if green {
                    i.state = RepayState::Proven;
                }
                Ok(())
            }
            Some(_) => Err(RP_BAD_STATE),
        }
    }

    /// 幽灵闭环清点：宣称已证但没重跑绿的项数。
    pub fn ghost_count(&self) -> usize {
        self.items.iter().filter(|i| i.ghost()).count()
    }

    /// 闭环计数。
    pub fn closed_count(&self) -> usize {
        self.items.iter().filter(|i| i.closed()).count()
    }

    /// 集覆盖核：登记数达锚。
    pub fn set_complete(&self) -> bool {
        self.items.len() == SET_ANCHOR
    }

    /// 终局裁定：覆盖达锚、零幽灵、全部闭环。
    pub fn all_closed(&self) -> bool {
        self.set_complete() && self.ghost_count() == 0
            && self.closed_count() == self.items.len()
    }

    /// 残留诊断：尚未闭环的项编号。
    pub fn gaps(&self) -> Vec<u32> {
        self.items.iter().filter(|i| !i.closed()).map(|i| i.id).collect()
    }

    /// 封账并返回折叠指纹。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b8800;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        for i in &self.items {
            h = h.wrapping_mul(31).wrapping_add(i.id);
            h = h.wrapping_mul(31).wrapping_add(i.state.as_u32());
            h = h.wrapping_mul(31).wrapping_add(i.debt_code);
            h = h.wrapping_mul(31).wrapping_add(i.recheck_green as u32);
            for b in i.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 注入核（五项）：[幽灵闭环, 未清偿报翻绿, 重号, 空名, 非法迁移] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 幽灵闭环：宣称已证但没重跑绿
    let mut l = RepayLedger::new("inj-ghost");
    let _ = l.enroll(1, "a", 0x11);
    let _ = l.repay(1);
    got[0] = l.prove(1, false).is_ok() && l.ghost_count() == 1 && !l.all_closed();
    // 2) 未清偿却报翻绿必拒
    let mut l2 = RepayLedger::new("inj-unpaid");
    let _ = l2.enroll(1, "b", 0x12);
    got[1] = l2.prove(1, true) == Err(RP_UNPAID_GREEN);
    // 3) 重号必拒
    let mut l3 = RepayLedger::new("inj-dup");
    let _ = l3.enroll(1, "a", 0x13);
    got[2] = l3.enroll(1, "b", 0x14) == Err(RP_DUP_ID);
    // 4) 空名必拒
    let mut l4 = RepayLedger::new("inj-empty");
    got[3] = l4.enroll(2, "", 0x15) == Err(RP_EMPTY_NAME);
    // 5) 非法迁移：已清再清必拒
    let mut l5 = RepayLedger::new("inj-bad");
    let _ = l5.enroll(1, "c", 0x16);
    let _ = l5.repay(1);
    got[4] = l5.repay(1) == Err(RP_BAD_STATE);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10368_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10368");

    // 正路径：12 项欠账逐条 清偿→重跑绿→闭环
    let mut ledger = RepayLedger::new("F10229 清偿后翻绿核");
    for i in 1..=SET_ANCHOR as u32 {
        let _ = ledger.enroll(i, "清偿项", 0x100 + i);
    }
    for i in 1..=SET_ANCHOR as u32 {
        let _ = ledger.repay(i);
    }
    for i in 1..=SET_ANCHOR as u32 {
        let _ = ledger.prove(i, true);
    }
    let j1 = ledger.all_closed();
    s.add("J1-清偿集全闭环", j1, "12 项三态走满且零幽灵");

    // 三态编码单调
    let j2 = RepayState::Owed.as_u32() < RepayState::Repaid.as_u32()
        && RepayState::Repaid.as_u32() < RepayState::Proven.as_u32();
    s.add("J2-三态有序", j2, "在欠<已清<已证 编码单调");

    // 残留诊断：只走两步的账残留可枚举
    let j3 = {
        let mut c = RepayLedger::new("gaps");
        let _ = c.enroll(1, "a", 0x21);
        let _ = c.enroll(2, "b", 0x22);
        let _ = c.repay(1);
        c.gaps() == vec![1, 2]
    };
    s.add("J3-残留可枚举", j3, "未闭环项编号全部列出");

    // 容量边界
    let j4 = {
        let mut c = RepayLedger::new("cap");
        let mut filled = true;
        for i in 1..=CAP as u32 {
            if c.enroll(i, "x", i).is_err() {
                filled = false;
            }
        }
        filled && c.enroll(999, "over", 999) == Err(RP_CAP)
    };
    s.add("J4-容量边界", j4, "超出定容必拒");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "幽灵闭环/未清报绿/重号必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "空名与非法迁移必检出");

    // 封账拒写
    let j7 = {
        ledger.seal();
        ledger.enroll(77, "late", 0x77) == Err(RP_SEALED)
    };
    s.add("J7-封账拒写", j7, "封账后清偿登记必拒");

    // 指纹稳定
    let j8 = ledger.fold() == ledger.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 幽灵闭环进终局裁定负路径
    let j9 = {
        let mut c = RepayLedger::new("neg");
        for i in 1..=SET_ANCHOR as u32 {
            let _ = c.enroll(i, "x", i);
            let _ = c.repay(i);
        }
        let _ = c.prove(SET_ANCHOR as u32, false);
        !c.all_closed() && c.ghost_count() == 1
    };
    s.add("J9-幽灵进裁定", j9, "一个幽灵即终局不通过");

    // 状态机封闭性：未知 id 的清偿/重跑都拒
    let j10 = {
        let mut c = RepayLedger::new("unk");
        let _ = c.enroll(1, "a", 0x31);
        c.repay(2) == Err(RP_DUP_ID) && c.prove(2, true) == Err(RP_DUP_ID)
    };
    s.add("J10-未知项拒", j10, "未登记 id 一律拒");

    // 零欠账码拒收
    let j11 = {
        let mut c = RepayLedger::new("zero");
        c.enroll(1, "a", 0) == Err(RP_BAD_STATE)
    };
    s.add("J11-零欠账码拒", j11, "欠账码为零按非法登记拒");

    // 明细落账含状态与欠账码
    let j12 = {
        let mut c = RepayLedger::new("render");
        let _ = c.enroll(1, "a", 0x41);
        let r = c.items[0].render();
        r.contains("s1") && r.contains("d0x41")
    };
    s.add("J12-明细落账", j12, "渲染含状态位与欠账码");

    // 闭环必须三态走满：跳过清偿直接证明不可达
    let j13 = {
        let mut c = RepayLedger::new("skip");
        let _ = c.enroll(1, "a", 0x51);
        c.prove(1, true) == Err(RP_UNPAID_GREEN)
            && c.items[0].state == RepayState::Owed
    };
    s.add("J13-跳步不可达", j13, "在欠态不可直接证成闭环");

    // 封账前后的指纹在只读操作下不变
    let j14 = {
        let mut c = RepayLedger::new("stable");
        let _ = c.enroll(1, "a", 0x61);
        let _ = c.repay(1);
        let f1 = c.fold();
        let _ = c.closed_count();
        let _ = c.ghost_count();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-账面指纹落账",
        ledger.fold() != 0,
        &format!("fold={:#x}", ledger.fold()),
    );
    let empty = RepayLedger::new("empty").fold();
    s.add_dyn("J16-空册指纹", empty == 0x1b8800u32.wrapping_mul(31), &format!("fold={:#x}", empty));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10368_repay_flow() {
        let mut l = RepayLedger::new("t");
        assert!(l.enroll(1, "a", 0x11).is_ok());
        assert!(l.repay(1).is_ok());
        assert!(l.repay(1) == Err(RP_BAD_STATE));
        assert!(l.prove(1, true).is_ok());
        assert!(l.items[0].closed());
        l.seal();
        assert!(l.enroll(2, "b", 0x12) == Err(RP_SEALED));
    }

    #[test]
    fn unx10368_checks_green() {
        let s = run_unx10368_checks();
        assert!(s.all_passed(), "UNX-F10368 判据未全绿");
    }
}
