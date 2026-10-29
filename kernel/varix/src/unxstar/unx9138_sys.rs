//! UNX-F9138 内存族升级接管注记件（0x63100 段）
//!
//! 锚点：UNX-F9138-J1 注记账（内存族从探针态升级到接管态的注记——
//! 注记号 × 前态 × 后态 × 接管边界逐条落账）；J2 注记语义（态迁移闭
//! 集 + 边界非空 + 前后态互异三性质）；J3 注记指纹（注记账折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（syscall 内存族升级接管面）。
//!
//! 结构：内存族态闭集 + 态迁移核 + 注记账 + 迁移闸（闭集闸 + 互异闸
//! + 边界闸）+ 注记指纹 + 注入核（未知态/同态空转/空边界/重注记号/
//! 印后注记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63100 独占 0x20）。
pub const SEG_BASE: u32 = 0x63100;
pub const UG_UNK_STATE: u32 = SEG_BASE + 0x01; // 未知态
pub const UG_SAME_STATE: u32 = SEG_BASE + 0x02; // 同态空转
pub const UG_EMPTY_BOUND: u32 = SEG_BASE + 0x03; // 空边界
pub const UG_DUP_NOTE: u32 = SEG_BASE + 0x04; // 重注记号
pub const UG_FROZEN: u32 = SEG_BASE + 0x05; // 印后注记

/// 内存族态闭集。
pub const S_PROBE: usize = 0; // 探针态（只观测）
pub const S_BOUNDED: usize = 1; // 有界态（观测 + 上限）
pub const S_TAKEOVER: usize = 2; // 接管态（自管分配面）
pub const S_COUNT: usize = 3;

pub const STATE_NAMES: [&str; S_COUNT] = ["probe", "bounded", "takeover"];

/// 单条升级注记。
#[derive(Clone)]
pub struct UpgradeNote {
    pub note_no: u32,
    pub from: usize,
    pub to: usize,
    pub boundary: String,
}

impl UpgradeNote {
    /// 注记合法：前后态闭集 + 互异 + 边界非空。
    pub fn note_ok(&self) -> bool {
        self.from < S_COUNT && self.to < S_COUNT && self.from != self.to && !self.boundary.is_empty()
    }

    /// 迁移方向合法：只允许升级（to > from）。
    pub fn upgrade_ok(&self) -> bool {
        self.note_ok() && self.to > self.from
    }

    fn render_line(&self) -> String {
        format!(
            "UP{}|{}->{}|{}",
            self.note_no, STATE_NAMES[self.from], STATE_NAMES[self.to], self.boundary
        )
    }
}

/// 注记账。
pub struct UpgradeLedger {
    notes: Vec<UpgradeNote>,
    sealed: bool,
}

impl UpgradeLedger {
    pub fn new() -> Self {
        UpgradeLedger {
            notes: Vec::new(),
            sealed: false,
        }
    }

    /// 落一条注记：闭集闸 + 互异闸 + 边界闸 + 重号拒 + 印后拒。
    /// 且必须与账内最新态衔接（链式升级）。
    pub fn note(&mut self, no: u32, from: usize, to: usize, boundary: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(UG_FROZEN);
        }
        if from >= S_COUNT || to >= S_COUNT {
            return Err(UG_UNK_STATE);
        }
        if from == to {
            return Err(UG_SAME_STATE);
        }
        if boundary.is_empty() {
            return Err(UG_EMPTY_BOUND);
        }
        if self.notes.iter().any(|x| x.note_no == no) {
            return Err(UG_DUP_NOTE);
        }
        // 链式衔接：首条 from 必须是 probe；后续 from = 前条 to。
        let expect_from = match self.notes.last() {
            None => S_PROBE,
            Some(last) => last.to,
        };
        if from != expect_from {
            return Err(UG_UNK_STATE);
        }
        self.notes.push(UpgradeNote {
            note_no: no,
            from,
            to,
            boundary: boundary.to_string(),
        });
        Ok(())
    }

    pub fn note_entry(&self, no: u32) -> Option<&UpgradeNote> {
        self.notes.iter().find(|x| x.note_no == no)
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// 当前态（末条 to；空账为 probe）。
    pub fn current_state(&self) -> usize {
        self.notes.last().map(|x| x.to).unwrap_or(S_PROBE)
    }

    /// 注记指纹。
    pub fn ledger_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for x in &self.notes {
            h = h.wrapping_mul(0x0100_0193) ^ x.note_no;
            h = h.wrapping_mul(0x0100_0193) ^ x.from as u32;
            h = h.wrapping_mul(0x0100_0193) ^ x.to as u32;
            for b in x.boundary.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印（要求已到接管态）。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.notes.is_empty() || self.current_state() != S_TAKEOVER {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for x in &self.notes {
            out.push_str(&x.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("UPGRADE-SEAL\n");
        }
        out
    }
}

impl Default for UpgradeLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：probe→bounded→takeover 两跳注记。
fn standard_ledger() -> UpgradeLedger {
    let mut l = UpgradeLedger::new();
    let _ = l.note(1, S_PROBE, S_BOUNDED, "cap=256MiB");
    let _ = l.note(2, S_BOUNDED, S_TAKEOVER, "handoff-page-tables");
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知态
    let mut l = UpgradeLedger::new();
    if l.note(1, 9, S_TAKEOVER, "x") == Err(UG_UNK_STATE) {
        caught += 1;
    }
    // 注 2：同态空转
    let mut l = UpgradeLedger::new();
    if l.note(1, S_PROBE, S_PROBE, "x") == Err(UG_SAME_STATE) {
        caught += 1;
    }
    // 注 3：空边界
    let mut l = UpgradeLedger::new();
    if l.note(1, S_PROBE, S_BOUNDED, "") == Err(UG_EMPTY_BOUND) {
        caught += 1;
    }
    // 注 4：断链（首条 from 非 probe）
    let mut l = UpgradeLedger::new();
    if l.note(1, S_BOUNDED, S_TAKEOVER, "x") == Err(UG_UNK_STATE) {
        caught += 1;
    }
    // 注 5：印后注记
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.note(9, S_TAKEOVER, S_TAKEOVER, "x") == Err(UG_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 注记账；J2 注记语义；J3 注记指纹。
pub fn run_unx9138_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9138");
    // J1-1 双跳全落账
    let l = standard_ledger();
    let ok = l.len() == 2 && l.note_entry(1).is_some() && l.note_entry(2).is_some();
    s.add("9138-J1-双跳全落账", ok, if ok { "" } else { "注记账不全" });
    // J1-2 渲染导出齐
    let mut l2 = standard_ledger();
    let _ = l2.seal();
    let text = l2.render_ledger();
    let ok = text.contains("UP1|probe->bounded|cap=256MiB")
        && text.contains("UP2|bounded->takeover|handoff-page-tables")
        && text.contains("UPGRADE-SEAL");
    s.add("9138-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 注记合法性（全部）
    let ok = (1..=2).all(|n| l.note_entry(n).map(|x| x.note_ok()).unwrap_or(false));
    s.add("9138-J2-注记合法性", ok, if ok { "" } else { "合法性漂移" });
    // J2-2 升级方向（to > from）
    let ok = (1..=2).all(|n| l.note_entry(n).map(|x| x.upgrade_ok()).unwrap_or(false));
    s.add("9138-J2-升级方向", ok, if ok { "" } else { "方向漂移" });
    // J2-3 链式衔接（当前态 = takeover）
    let ok = l.current_state() == S_TAKEOVER;
    s.add("9138-J2-链式衔接", ok, if ok { "" } else { "衔接漂移" });
    // J2-4 边界非空抽查
    let ok = l.note_entry(2).map(|x| x.boundary == "handoff-page-tables").unwrap_or(false);
    s.add("9138-J2-边界抽查", ok, if ok { "" } else { "边界漂移" });
    // J2-5 断链拒
    let mut l3 = UpgradeLedger::new();
    let _ = l3.note(1, S_PROBE, S_BOUNDED, "c1");
    let ok = l3.note(2, S_PROBE, S_TAKEOVER, "c2") == Err(UG_UNK_STATE);
    s.add("9138-J2-断链拒", ok, if ok { "" } else { "断链闸失效" });
    // J2-6 同态空转拒
    let mut l4 = UpgradeLedger::new();
    let ok = l4.note(1, S_TAKEOVER, S_TAKEOVER, "x") == Err(UG_SAME_STATE);
    s.add("9138-J2-同态拒", ok, if ok { "" } else { "同态闸失效" });
    // J3-1 注记指纹确定
    let ok = standard_ledger().ledger_fp() == standard_ledger().ledger_fp();
    s.add("9138-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 边界变签变
    let mut l5 = standard_ledger();
    let fp0 = l5.ledger_fp();
    if let Some(x) = l5.notes.first_mut() {
        x.boundary.push('x');
    }
    let ok = l5.ledger_fp() != fp0;
    s.add("9138-J3-边界变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（须到接管态）
    let mut l6 = standard_ledger();
    let ok = l6.seal() && l6.sealed();
    s.add("9138-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 未接管封印拒
    let mut l7 = UpgradeLedger::new();
    let _ = l7.note(1, S_PROBE, S_BOUNDED, "c");
    let ok = !l7.seal() && !l7.sealed();
    s.add("9138-J3-未接管封印拒", ok, if ok { "" } else { "封印闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9138-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9138_all_checks_green() {
        let set = run_unx9138_checks();
        assert!(set.all_passed(), "UNX-F9138 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9138_ledger_stable() {
        assert!(standard_ledger().current_state() == S_TAKEOVER);
    }
}
