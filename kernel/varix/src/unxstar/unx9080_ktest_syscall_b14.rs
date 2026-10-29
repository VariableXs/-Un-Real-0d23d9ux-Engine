//! UNX-F9080 ktest syscall 面 B14 批断言集（0x629C0 段）
//!
//! 锚点：UNX-F9080-J1 批断言账（B14 四子批闭集全录 + 子批 × 用
//! 例数 × 绿数逐子批落账 + 渲染导出面）；J2 批断言语义（用例守恒 + 全绿
//! 判定 + 绿批计数三性质）；J3 封账印（批断言指纹 + 封印后只读 + 注入核）
//! 三判据全绿。
//! 域：CoRun·UNX-C2-B14（glibc 消费面·ktest 断言面）。
//!
//! 结构：四子批闭集 + 子批登记核（越绿闸 + 重复拒）+ 批断言账 + 批断言
//! 指纹 + 注入核（越绿登记/子批重复/未知子批/非全绿印/空账印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x629C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x629C0;
pub const KB_OVERGREEN: u32 = SEG_BASE + 0x01; // 绿数 > 用例数
pub const KB_DUP: u32 = SEG_BASE + 0x02; // 子批重复登记
pub const KB_UNK: u32 = SEG_BASE + 0x03; // 未知子批
pub const KB_SEALED: u32 = SEG_BASE + 0x04; // 封账印后改账
pub const KB_NOT_GREEN: u32 = SEG_BASE + 0x05; // 非全绿封印

/// B14 四子批闭集。
pub const KB_SLOT_A: usize = 0; // b11 子批
pub const KB_SLOT_B: usize = 1; // b12 子批
pub const KB_SLOT_C: usize = 2; // b13 子批
pub const KB_SLOT_D: usize = 3; // b14 子批
pub const KB_SLOT_COUNT: usize = 4;

/// 子批名渲染。
pub const KB_SLOT_NAMES: [&str; KB_SLOT_COUNT] =
    ["b11", "b12", "b13", "b14"];

/// 单子批断言账。
#[derive(Clone)]
pub struct BatchEntry {
    pub slot: usize,
    pub cases: u64,
    pub passed: u64,
}

impl BatchEntry {
    /// 子批用例守恒核：0 ≤ 绿数 ≤ 用例数。
    pub fn entry_ok(&self) -> bool {
        self.passed <= self.cases
    }

    /// 子批全绿核。
    pub fn entry_green(&self) -> bool {
        self.passed == self.cases
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|{}",
            KB_SLOT_NAMES[self.slot],
            self.cases,
            self.passed
        )
    }
}

/// 子批越绿核：绿数不得超过用例数。
pub fn kb_green_ok(cases: u64, passed: u64) -> bool {
    passed <= cases
}

/// 子批名渲染单点：越界回退 BADSLOT。
pub fn kb_slot_name(slot: usize) -> &'static str {
    if slot >= KB_SLOT_COUNT {
        return "BADSLOT";
    }
    KB_SLOT_NAMES[slot]
}

/// ktest B14 批断言账本。
pub struct BatchAssert {
    entries: [Option<BatchEntry>; KB_SLOT_COUNT],
    sealed: bool,
}

impl BatchAssert {
    pub fn new() -> Self {
        BatchAssert {
            entries: [None, None, None, None],
            sealed: false,
        }
    }

    /// 登记一个子批：子批闭集 + 越绿闸 + 重复拒 + 印后拒。
    pub fn register(&mut self, slot: usize, cases: u64, passed: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(KB_SEALED);
        }
        if slot >= KB_SLOT_COUNT {
            return Err(KB_UNK);
        }
        if !kb_green_ok(cases, passed) {
            return Err(KB_OVERGREEN);
        }
        if self.entries[slot].is_some() {
            return Err(KB_DUP);
        }
        self.entries[slot] = Some(BatchEntry { slot, cases, passed });
        Ok(())
    }

    pub fn entry(&self, slot: usize) -> Option<&BatchEntry> {
        if slot >= KB_SLOT_COUNT {
            return None;
        }
        self.entries[slot].as_ref()
    }

    /// 批完整性：四子批全录 + 每子批全绿。
    pub fn batch_ok(&self) -> bool {
        self.entries.iter().flatten().all(|e| e.entry_ok() && e.entry_green())
            && self.entries.iter().filter(|e| e.is_some()).count() == KB_SLOT_COUNT
    }

    /// 用例总数。
    pub fn total_cases(&self) -> u64 {
        self.entries.iter().flatten().map(|e| e.cases).sum()
    }

    /// 绿数总数。
    pub fn total_passed(&self) -> u64 {
        self.entries.iter().flatten().map(|e| e.passed).sum()
    }

    /// 全绿子批计数。
    pub fn green_slots(&self) -> usize {
        self.entries
            .iter()
            .flatten()
            .filter(|e| e.entry_green())
            .count()
    }

    /// 批断言指纹。
    pub fn kb_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in self.entries.iter().flatten() {
            h = h.wrapping_mul(0x0100_0193) ^ (e.cases as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (e.passed as u32);
            h ^= ((e.cases >> 32) as u32) ^ ((e.passed >> 32) as u32);
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印：非全批绿拒 + 重封拒。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.batch_ok() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render(&self) -> String {
        let mut out = String::new();
        for e in self.entries.iter().flatten() {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        out.push_str(&format!(
            "KB14-TOTAL|{}/{}\n",
            self.total_cases(),
            self.total_passed()
        ));
        if self.sealed {
            out.push_str("KB14-SEAL\n");
        }
        out
    }
}

impl Default for BatchAssert {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四子批全绿登记（12/12、9/9、15/15、8/8，共 44/44）。
fn standard_batch() -> BatchAssert {
    let mut b = BatchAssert::new();
    let _ = b.register(KB_SLOT_A, 12, 12);
    let _ = b.register(KB_SLOT_B, 9, 9);
    let _ = b.register(KB_SLOT_C, 15, 15);
    let _ = b.register(KB_SLOT_D, 8, 8);
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：越绿登记（绿 > 用例）
    let mut b = BatchAssert::new();
    if b.register(KB_SLOT_A, 5, 9) == Err(KB_OVERGREEN) {
        caught += 1;
    }
    // 注 2：子批重复登记
    let mut b = standard_batch();
    if b.register(KB_SLOT_B, 9, 9) == Err(KB_DUP) {
        caught += 1;
    }
    // 注 3：未知子批
    let mut b = BatchAssert::new();
    if b.register(7, 3, 3) == Err(KB_UNK) {
        caught += 1;
    }
    // 注 4：非全绿封印（缺批 + 有红）
    let mut b = BatchAssert::new();
    let _ = b.register(KB_SLOT_A, 12, 12);
    let _ = b.register(KB_SLOT_B, 9, 9);
    if !b.batch_ok() && !b.seal() {
        caught += 1;
    }
    // 注 5：空账印
    let mut b = BatchAssert::new();
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 批断言账；J2 批断言语义；J3 封账印。
pub fn run_unx9080_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9080");
    // J1-1 四子批全录
    let b = standard_batch();
    let ok = (0..KB_SLOT_COUNT).all(|i| b.entry(i).is_some()) && b.batch_ok();
    s.add("9080-J1-四子批全录", ok, if ok { "" } else { "批断言账不全" });
    // J1-2 渲染导出齐
    let text = standard_batch().render();
    let ok = text.contains("b11|12|12")
        && text.contains("b12|9|9")
        && text.contains("b13|15|15")
        && text.contains("b14|8|8")
        && text.contains("KB14-TOTAL|44/44");
    s.add("9080-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J1-3 未封印渲染无印面（SEAL 行只在封账后出现）
    let ok = !BatchAssert::new().render().contains("KB14-SEAL")
        && standard_batch().render().contains("b11|12|12");
    s.add("9080-J1-未印无印面", ok, if ok { "" } else { "印面泄漏" });
    // J2-1 用例总数核（44）
    let ok = b.total_cases() == 44;
    s.add("9080-J2-用例总数", ok, if ok { "" } else { "用例总数漂移" });
    // J2-2 绿数总数核（44）
    let ok = b.total_passed() == 44;
    s.add("9080-J2-绿数总数", ok, if ok { "" } else { "绿数总数漂移" });
    // J2-3 全绿子批计数（4）
    let ok = b.green_slots() == 4;
    s.add("9080-J2-全绿计数", ok, if ok { "" } else { "全绿计数漂移" });
    // J2-4 越绿核（绿 ≤ 用例）
    let ok = kb_green_ok(15, 15) && !kb_green_ok(9, 12);
    s.add("9080-J2-越绿核", ok, if ok { "" } else { "越绿闸失效" });
    // J2-5 子批全绿核
    let ok = b.entry(KB_SLOT_C).map(|e| e.entry_green()).unwrap_or(false);
    s.add("9080-J2-子批全绿", ok, if ok { "" } else { "子批全绿漂移" });
    // J2-6 批完整性核
    let ok = b.batch_ok();
    s.add("9080-J2-批完整性", ok, if ok { "" } else { "批完整性漂移" });
    // J2-7 绿数守恒核（绿数总数 ≤ 用例总数且两总数同源）
    let ok = b.total_passed() == b.total_cases();
    s.add("9080-J2-绿数守恒", ok, if ok { "" } else { "绿数守恒破坏" });
    // J2-8 子批名渲染核（闭集全名可渲染、无 BADSLOT）
    let ok = (0..KB_SLOT_COUNT).all(|i| kb_slot_name(i) != "BADSLOT");
    s.add("9080-J2-子批名渲染", ok, if ok { "" } else { "子批名漂移" });
    // J2-9 越界名回退核（越界槽回退 BADSLOT，首槽回 b11）
    let ok = kb_slot_name(9) == "BADSLOT" && kb_slot_name(0) == "b11";
    s.add("9080-J2-越界名回退", ok, if ok { "" } else { "回退漂移" });
    // J3-1 批断言指纹确定
    let ok = standard_batch().kb_fp() == standard_batch().kb_fp();
    s.add("9080-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut b2 = standard_batch();
    let fp0 = b2.kb_fp();
    if let Some(e) = b2.entries[0].as_mut() {
        e.cases += 1;
    }
    let ok = b2.kb_fp() != fp0;
    s.add("9080-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut b3 = standard_batch();
    let ok = b3.seal() && b3.sealed() && b3.render().contains("KB14-SEAL");
    s.add("9080-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒登记
    let ok = b3.register(KB_SLOT_A, 1, 1) == Err(KB_SEALED);
    s.add("9080-J3-印后拒登记", ok, if ok { "" } else { "只读闸失效" });
    // J3-5 重封拒
    let mut b4 = standard_batch();
    let first = b4.seal();
    let ok = first && !b4.seal();
    s.add("9080-J3-重封拒", ok, if ok { "" } else { "重封闸失效" });
    // J3-6 印后指纹漂移（封印位入签）
    let mut b5 = standard_batch();
    let fp_pre = b5.kb_fp();
    let _ = b5.seal();
    let ok = b5.kb_fp() != fp_pre;
    s.add("9080-J3-印后指纹漂移", ok, if ok { "" } else { "封印位未入签" });
    // J3-7 印面渲染一致（封印后渲染含 SEAL 且 TOTAL 行不变）
    let mut b7 = standard_batch();
    let tot_before = b7.total_cases();
    let _ = b7.seal();
    let ok = b7.render().contains("KB14-SEAL") && b7.total_cases() == tot_before;
    s.add("9080-J3-印面渲染一致", ok, if ok { "" } else { "印面渲染漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9080-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9080_all_checks_green() {
        let set = run_unx9080_checks();
        assert!(set.all_passed(), "UNX-F9080 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9080_batch_stable() {
        assert!(standard_batch().batch_ok());
        assert_eq!(standard_batch().total_cases(), 44);
        assert_eq!(standard_batch().total_passed(), 44);
    }
}
