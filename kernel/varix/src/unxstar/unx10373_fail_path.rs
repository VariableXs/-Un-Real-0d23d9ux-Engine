//! UNX-F10373 不通过处置（闸失败回炉路径）（0x1b920 段）
//!
//! 锚点：UNX-F10373-J1 闸失败回炉路径——失败项必须带处置入回炉队列
//! （fix/exempt/defer 三选一），未处置失败残留 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·回炉谱系）。
//!
//! 结构：回炉队列（失败项+处置位）+ 处置三态语义（修复重判/豁免挂账/
//! 延期限量）+ 残留清点 + 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b920 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b920;
pub const FW_DUP_ID: u32 = SEG_BASE + 0x01; // 回炉项重号
pub const FW_EMPTY_NAME: u32 = SEG_BASE + 0x02; // 项名空
pub const FW_NO_DISPOSAL: u32 = SEG_BASE + 0x03; // 未处置即出队
pub const FW_SEALED: u32 = SEG_BASE + 0x04; // 封账后写账
pub const FW_CAP: u32 = SEG_BASE + 0x05; // 超容量
pub const FW_BAD_GREEN: u32 = SEG_BASE + 0x06; // 未修复却报翻绿
pub const FW_DEFER_OVER: u32 = SEG_BASE + 0x07; // 延期超限

/// 回炉队列定容。
pub const CAP: usize = 24;
/// 延期处置上限（defer 配额）。
pub const DEFER_LIMIT: usize = 3;

/// 处置三态。
#[derive(Clone, PartialEq, Eq)]
pub enum Disposal {
    /// 修复：修完重判翻绿。
    Fix,
    /// 豁免：带 ADR 挂账豁免。
    Exempt,
    /// 延期：占延期配额，下批再清。
    Defer,
}

impl Disposal {
    pub fn as_u32(&self) -> u32 {
        match self {
            Disposal::Fix => 1,
            Disposal::Exempt => 2,
            Disposal::Defer => 3,
        }
    }
}

/// 单条回炉项。
#[derive(Clone)]
pub struct ReworkItem {
    pub id: u32,
    pub name: String,
    pub disposal: Option<Disposal>,
    pub reg_green: bool,
    pub rounds: u32,
}

impl ReworkItem {
    /// 已处置。
    pub fn disposed(&self) -> bool {
        self.disposal.is_some()
    }

    /// 闭环：已修复重判绿 或 豁免挂账 或 延期入额。
    pub fn closed(&self) -> bool {
        match &self.disposal {
            Some(Disposal::Fix) => self.reg_green,
            Some(Disposal::Exempt) => true,
            Some(Disposal::Defer) => true,
            None => false,
        }
    }

    /// 幽灵翻绿：没走 Fix 却报绿。
    pub fn ghost(&self) -> bool {
        self.reg_green && self.disposal.as_ref() != Some(&Disposal::Fix)
    }

    pub fn render(&self) -> String {
        let d = self.disposal.as_ref().map(|d| d.as_u32()).unwrap_or(0);
        format!("W{}|{}|d{}|g{}|r{}", self.id, self.name, d, self.reg_green as u32, self.rounds)
    }
}

/// 回炉队列。
pub struct ReworkQueue {
    pub items: Vec<ReworkItem>,
    pub sealed: bool,
    pub note: String,
    defer_used: usize,
}

impl ReworkQueue {
    pub fn new(note: &str) -> ReworkQueue {
        ReworkQueue {
            items: Vec::new(),
            sealed: false,
            note: note.to_string(),
            defer_used: 0,
        }
    }

    /// 失败项入队：重号、空名、超容、封账均拒。
    pub fn push(&mut self, id: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(FW_SEALED);
        }
        if name.is_empty() {
            return Err(FW_EMPTY_NAME);
        }
        if id == 0 || self.items.iter().any(|i| i.id == id) {
            return Err(FW_DUP_ID);
        }
        if self.items.len() >= CAP {
            return Err(FW_CAP);
        }
        self.items.push(ReworkItem {
            id,
            name: name.to_string(),
            disposal: None,
            reg_green: false,
            rounds: 0,
        });
        Ok(())
    }

    fn find_mut(&mut self, id: u32) -> Option<&mut ReworkItem> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// 处置：fix/exempt/defer 三选一；defer 占配额，超限拒。
    pub fn dispose(&mut self, id: u32, d: Disposal) -> Result<(), u32> {
        if self.sealed {
            return Err(FW_SEALED);
        }
        if d == Disposal::Defer && self.defer_used >= DEFER_LIMIT {
            return Err(FW_DEFER_OVER);
        }
        let pos = self.items.iter().position(|i| i.id == id);
        match pos {
            None => Err(FW_DUP_ID),
            Some(p) => {
                if d == Disposal::Defer {
                    self.defer_used += 1;
                }
                self.items[p].disposal = Some(d);
                Ok(())
            }
        }
    }

    /// 修复重判：仅 Fix 处置可登记翻绿。
    pub fn regreen(&mut self, id: u32, green: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(FW_SEALED);
        }
        match self.find_mut(id) {
            None => Err(FW_DUP_ID),
            Some(i) if i.disposal.as_ref() != Some(&Disposal::Fix) => Err(FW_BAD_GREEN),
            Some(i) => {
                i.reg_green = green;
                i.rounds += 1;
                Ok(())
            }
        }
    }

    /// 残留清点：未处置项数。
    pub fn leftover_count(&self) -> usize {
        self.items.iter().filter(|i| !i.disposed()).count()
    }

    /// 延期占用数。
    pub fn defer_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.disposal.as_ref() == Some(&Disposal::Defer))
            .count()
    }

    /// 终局裁定：零残留、零幽灵、全部闭环。
    pub fn all_clear(&self) -> bool {
        self.leftover_count() == 0
            && self.items.iter().all(|i| i.closed())
            && self.items.iter().all(|i| !i.ghost())
    }

    /// 残留诊断。
    pub fn gaps(&self) -> Vec<u32> {
        self.items.iter().filter(|i| !i.closed()).map(|i| i.id).collect()
    }

    /// 封账并返回折叠指纹。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b9200;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        h = h.wrapping_mul(31).wrapping_add(self.defer_used as u32);
        for i in &self.items {
            h = h.wrapping_mul(31).wrapping_add(i.id);
            h = h.wrapping_mul(31).wrapping_add(
                i.disposal.as_ref().map(|d| d.as_u32()).unwrap_or(0),
            );
            h = h.wrapping_mul(31).wrapping_add(i.reg_green as u32);
            h = h.wrapping_mul(31).wrapping_add(i.rounds);
            for b in i.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 注入核（五项）：[未处置出队, 未修复报绿, 延期超限, 重号, 空名] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 未处置项进终局裁定必不通过（残留检出）
    let mut q = ReworkQueue::new("inj-left");
    let _ = q.push(1, "a");
    got[0] = !q.all_clear() && q.leftover_count() == 1;
    // 2) 未修复（Exempt）却报翻绿必拒
    let mut q2 = ReworkQueue::new("inj-ghost");
    let _ = q2.push(1, "a");
    let _ = q2.dispose(1, Disposal::Exempt);
    got[1] = q2.regreen(1, true) == Err(FW_BAD_GREEN);
    // 3) 延期超限必拒
    let mut q3 = ReworkQueue::new("inj-defer");
    let mut i = 1u32;
    while i <= DEFER_LIMIT as u32 + 1 {
        let _ = q3.push(i, "d");
        i += 1;
    }
    let mut k = 1u32;
    let mut first_ok = true;
    while k <= DEFER_LIMIT as u32 {
        if q3.dispose(k, Disposal::Defer).is_err() {
            first_ok = false;
        }
        k += 1;
    }
    got[2] = first_ok && q3.dispose(DEFER_LIMIT as u32 + 1, Disposal::Defer)
        == Err(FW_DEFER_OVER);
    // 4) 重号必拒
    let mut q4 = ReworkQueue::new("inj-dup");
    let _ = q4.push(1, "a");
    got[3] = q4.push(1, "b") == Err(FW_DUP_ID);
    // 5) 空名必拒
    let mut q5 = ReworkQueue::new("inj-empty");
    got[4] = q5.push(2, "") == Err(FW_EMPTY_NAME);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10373_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10373");

    // 正路径：fix 修复翻绿 + exempt 挂账 + defer 入额，全闭环
    let mut q = ReworkQueue::new("闸失败回炉");
    let _ = q.push(1, "修一批");
    let _ = q.push(2, "豁免批");
    let _ = q.push(3, "延期批");
    let _ = q.dispose(1, Disposal::Fix);
    let _ = q.dispose(2, Disposal::Exempt);
    let _ = q.dispose(3, Disposal::Defer);
    let _ = q.regreen(1, true);
    let j1 = q.all_clear();
    s.add("J1-三处置全闭环", j1, "fix/exempt/defer 三路全闭环");

    // 处置编码单调
    let j2 = Disposal::Fix.as_u32() < Disposal::Exempt.as_u32()
        && Disposal::Exempt.as_u32() < Disposal::Defer.as_u32();
    s.add("J2-处置编码", j2, "fix<exempt<defer 编码单调");

    // fix 未翻绿不闭环：残留可枚举
    let j3 = {
        let mut c = ReworkQueue::new("gaps");
        let _ = c.push(1, "a");
        let _ = c.dispose(1, Disposal::Fix);
        !c.items[0].closed() && c.gaps() == vec![1]
    };
    s.add("J3-fix 未绿不闭环", j3, "fix 处置未重判绿即残留");

    // 容量边界
    let j4 = {
        let mut c = ReworkQueue::new("cap");
        let mut filled = true;
        let mut i = 1u32;
        while i <= CAP as u32 {
            if c.push(i, "x").is_err() {
                filled = false;
            }
            i += 1;
        }
        filled && c.push(999, "over") == Err(FW_CAP)
    };
    s.add("J4-容量边界", j4, "超出定容必拒");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "残留/未修复报绿/延期超限必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "重号与空名必检出");

    // 封账拒写
    let j7 = {
        q.seal();
        q.push(77, "late") == Err(FW_SEALED)
    };
    s.add("J7-封账拒写", j7, "封账后回炉登记必拒");

    // 指纹稳定
    let j8 = q.fold() == q.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 延期配额记账：占用数与登记一致
    let j9 = {
        let mut c = ReworkQueue::new("defer");
        let mut i = 1u32;
        while i <= DEFER_LIMIT as u32 {
            let _ = c.push(i, "d");
            let _ = c.dispose(i, Disposal::Defer);
            i += 1;
        }
        c.defer_count() == DEFER_LIMIT
    };
    s.add("J9-延期记账", j9, "延期占用数与登记一致");

    // 幽灵翻绿进终局裁定
    let j10 = {
        let mut c = ReworkQueue::new("ghost");
        let _ = c.push(1, "a");
        let _ = c.dispose(1, Disposal::Exempt);
        c.items[0].reg_green = true;
        !c.all_clear() && c.items[0].ghost()
    };
    s.add("J10-幽灵进裁定", j10, "非 fix 报绿即终局不通过");

    // 未知 id 拒
    let j11 = {
        let mut c = ReworkQueue::new("unk");
        let _ = c.push(1, "a");
        c.dispose(2, Disposal::Fix) == Err(FW_DUP_ID)
            && c.regreen(2, true) == Err(FW_DUP_ID)
    };
    s.add("J11-未知项拒", j11, "未登记 id 一律拒");

    // 明细落账含处置位与轮次
    let j12 = {
        let r = q.items[0].render();
        r.contains("d1") && r.contains("r1")
    };
    s.add("J12-明细落账", j12, "渲染含处置位与重判轮次");

    // 修复重判计数：多轮重判 rounds 累加
    let j13 = {
        let mut c = ReworkQueue::new("rounds");
        let _ = c.push(1, "a");
        let _ = c.dispose(1, Disposal::Fix);
        let _ = c.regreen(1, false);
        let _ = c.regreen(1, true);
        c.items[0].rounds == 2 && c.items[0].closed()
    };
    s.add("J13-重判轮次", j13, "多轮重判轮次累加且末轮绿闭环");

    // 只读不改账
    let j14 = {
        let mut c = ReworkQueue::new("stable");
        let _ = c.push(1, "a");
        let f1 = c.fold();
        let _ = c.leftover_count();
        let _ = c.defer_count();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-账面指纹落账",
        q.fold() != 0,
        &format!("fold={:#x}", q.fold()),
    );
    let empty = ReworkQueue::new("empty").fold();
    s.add_dyn("J16-空册指纹", empty == 0x1b9200u32.wrapping_mul(31).wrapping_mul(31), &format!("fold={:#x}", empty));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10373_rework_flow() {
        let mut q = ReworkQueue::new("t");
        assert!(q.push(1, "a").is_ok());
        assert!(q.dispose(1, Disposal::Defer).is_ok());
        assert!(q.dispose(2, Disposal::Fix) == Err(FW_DUP_ID));
        assert!(q.push(1, "dup") == Err(FW_DUP_ID));
        q.seal();
        assert!(q.push(3, "late") == Err(FW_SEALED));
    }

    #[test]
    fn unx10373_checks_green() {
        let s = run_unx10373_checks();
        assert!(s.all_passed(), "UNX-F10373 判据未全绿");
    }
}
