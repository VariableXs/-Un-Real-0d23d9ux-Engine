//! UNX-F9120 ktest syscall 面 B16 批断言集（0x62EC0 段）
//!
//! 锚点：UNX-F9120-J1 批断言账（B16 四子批闭集全录 + 子批 × 用
//! 例数 × 绿数逐子批落账 + 渲染导出面）；J2 批断言语义（用例守恒 + 全绿
//! 判定 + 绿批计数三性质）；J3 封账印（批断言指纹 + 封印后只读 + 注入核）
//! 三判据全绿。
//! 域：CoRun·UNX-C2-B16（fcntl/ioctl 面·ktest 断言面）。
//!
//! 结构：四子批闭集 + 子批登记核（越绿闸 + 重复拒）+ 批断言账 + 批断言
//! 指纹 + 注入核（越绿登记/子批重复/未知子批/非全绿印/空账印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62EC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x62EC0;
pub const KC_OVERGREEN: u32 = SEG_BASE + 0x01; // 绿数 > 用例数
pub const KC_DUP: u32 = SEG_BASE + 0x02; // 子批重复登记
pub const KC_UNK: u32 = SEG_BASE + 0x03; // 未知子批
pub const KC_SEALED: u32 = SEG_BASE + 0x04; // 封账印后改账
pub const KC_NOT_GREEN: u32 = SEG_BASE + 0x05; // 非全绿封印

/// B16 四子批闭集。
pub const KC_SLOT_A: usize = 0; // b13 子批
pub const KC_SLOT_B: usize = 1; // b14 子批
pub const KC_SLOT_C: usize = 2; // b15 子批
pub const KC_SLOT_D: usize = 3; // b16 子批
pub const KC_SLOT_COUNT: usize = 4;

/// 子批名渲染。
pub const KC_SLOT_NAMES: [&str; KC_SLOT_COUNT] =
    ["b13", "b14", "b15", "b16"];

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
            KC_SLOT_NAMES[self.slot],
            self.cases,
            self.passed
        )
    }
}

/// 子批越绿核：绿数不得超过用例数。
pub fn kc_green_ok(cases: u64, passed: u64) -> bool {
    passed <= cases
}

/// 子批名渲染单点：越界回退 BADSLOT。
pub fn kc_slot_name(slot: usize) -> &'static str {
    if slot >= KC_SLOT_COUNT {
        return "BADSLOT";
    }
    KC_SLOT_NAMES[slot]
}

/// ktest B16 批断言账本。
pub struct BatchAssert16 {
    entries: [Option<BatchEntry>; KC_SLOT_COUNT],
    sealed: bool,
}

impl BatchAssert16 {
    pub fn new() -> Self {
        BatchAssert16 {
            entries: [None, None, None, None],
            sealed: false,
        }
    }

    /// 登记一个子批：子批闭集 + 越绿闸 + 重复拒 + 印后拒。
    pub fn register(&mut self, slot: usize, cases: u64, passed: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(KC_SEALED);
        }
        if slot >= KC_SLOT_COUNT {
            return Err(KC_UNK);
        }
        if !kc_green_ok(cases, passed) {
            return Err(KC_OVERGREEN);
        }
        if self.entries[slot].is_some() {
            return Err(KC_DUP);
        }
        self.entries[slot] = Some(BatchEntry { slot, cases, passed });
        Ok(())
    }

    pub fn entry(&self, slot: usize) -> Option<&BatchEntry> {
        if slot >= KC_SLOT_COUNT {
            return None;
        }
        self.entries[slot].as_ref()
    }

    /// 批完整性：四子批全录 + 每子批全绿。
    pub fn batch_ok(&self) -> bool {
        self.entries.iter().flatten().all(|e| e.entry_ok() && e.entry_green())
            && self.entries.iter().filter(|e| e.is_some()).count() == KC_SLOT_COUNT
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
    pub fn kc_fp(&self) -> u32 {
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
            "KB16-TOTAL|{}/{}\n",
            self.total_cases(),
            self.total_passed()
        ));
        if self.sealed {
            out.push_str("KB16-SEAL\n");
        }
        out
    }
}

impl Default for BatchAssert16 {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四子批全绿登记（14/14、12/12、11/11、7/7，共 44/44）。
fn standard_batch() -> BatchAssert16 {
    let mut b = BatchAssert16::new();
    let _ = b.register(KC_SLOT_A, 14, 14);
    let _ = b.register(KC_SLOT_B, 12, 12);
    let _ = b.register(KC_SLOT_C, 11, 11);
    let _ = b.register(KC_SLOT_D, 7, 7);
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：越绿登记（绿 > 用例）
    let mut b = BatchAssert16::new();
    if b.register(KC_SLOT_A, 5, 9) == Err(KC_OVERGREEN) {
        caught += 1;
    }
    // 注 2：子批重复登记
    let mut b = standard_batch();
    if b.register(KC_SLOT_B, 12, 12) == Err(KC_DUP) {
        caught += 1;
    }
    // 注 3：未知子批
    let mut b = BatchAssert16::new();
    if b.register(7, 3, 3) == Err(KC_UNK) {
        caught += 1;
    }
    // 注 4：非全绿封印（缺批 + 有红）
    let mut b = BatchAssert16::new();
    let _ = b.register(KC_SLOT_A, 14, 14);
    let _ = b.register(KC_SLOT_B, 12, 12);
    if !b.batch_ok() && !b.seal() {
        caught += 1;
    }
    // 注 5：空账印
    let mut b = BatchAssert16::new();
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 批断言账；J2 批断言语义；J3 封账印。
pub fn run_unx9120_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9120");
    // J1-1 四子批全录
    let b = standard_batch();
    let ok = (0..KC_SLOT_COUNT).all(|i| b.entry(i).is_some()) && b.batch_ok();
    s.add("9120-J1-四子批全录", ok, if ok { "" } else { "批断言账不全" });
    // J1-2 渲染导出齐
    let text = standard_batch().render();
    let ok = text.contains("b13|14|14")
        && text.contains("b14|12|12")
        && text.contains("b15|11|11")
        && text.contains("b16|7|7")
        && text.contains("KB16-TOTAL|44/44");
    s.add("9120-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J1-3 未封印渲染无印面（SEAL 行只在封账后出现）
    let ok = !BatchAssert16::new().render().contains("KB16-SEAL")
        && standard_batch().render().contains("b13|14|14");
    s.add("9120-J1-未印无印面", ok, if ok { "" } else { "印面泄漏" });
    // J2-1 用例总数核（44）
    let ok = b.total_cases() == 44;
    s.add("9120-J2-用例总数", ok, if ok { "" } else { "用例总数漂移" });
    // J2-2 绿数总数核（44）
    let ok = b.total_passed() == 44;
    s.add("9120-J2-绿数总数", ok, if ok { "" } else { "绿数总数漂移" });
    // J2-3 全绿子批计数（4）
    let ok = b.green_slots() == 4;
    s.add("9120-J2-全绿计数", ok, if ok { "" } else { "全绿计数漂移" });
    // J2-4 越绿核（绿 ≤ 用例）
    let ok = kc_green_ok(15, 15) && !kc_green_ok(9, 12);
    s.add("9120-J2-越绿核", ok, if ok { "" } else { "越绿闸失效" });
    // J2-5 子批全绿核
    let ok = b.entry(KC_SLOT_C).map(|e| e.entry_green()).unwrap_or(false);
    s.add("9120-J2-子批全绿", ok, if ok { "" } else { "子批全绿漂移" });
    // J2-6 批完整性核
    let ok = b.batch_ok();
    s.add("9120-J2-批完整性", ok, if ok { "" } else { "批完整性漂移" });
    // J2-7 绿数守恒核（绿数总数 ≤ 用例总数且两总数同源）
    let ok = b.total_passed() == b.total_cases();
    s.add("9120-J2-绿数守恒", ok, if ok { "" } else { "绿数守恒破坏" });
    // J2-8 子批名渲染核（闭集全名可渲染、无 BADSLOT）
    let ok = (0..KC_SLOT_COUNT).all(|i| kc_slot_name(i) != "BADSLOT");
    s.add("9120-J2-子批名渲染", ok, if ok { "" } else { "子批名漂移" });
    // J2-9 越界名回退核（越界槽回退 BADSLOT，首槽回 b13）
    let ok = kc_slot_name(9) == "BADSLOT" && kc_slot_name(0) == "b13";
    s.add("9120-J2-越界名回退", ok, if ok { "" } else { "回退漂移" });
    // J3-1 批断言指纹确定
    let ok = standard_batch().kc_fp() == standard_batch().kc_fp();
    s.add("9120-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut b2 = standard_batch();
    let fp0 = b2.kc_fp();
    if let Some(e) = b2.entries[0].as_mut() {
        e.cases += 1;
    }
    let ok = b2.kc_fp() != fp0;
    s.add("9120-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut b3 = standard_batch();
    let ok = b3.seal() && b3.sealed() && b3.render().contains("KB16-SEAL");
    s.add("9120-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒登记
    let ok = b3.register(KC_SLOT_A, 1, 1) == Err(KC_SEALED);
    s.add("9120-J3-印后拒登记", ok, if ok { "" } else { "只读闸失效" });
    // J3-5 重封拒
    let mut b4 = standard_batch();
    let first = b4.seal();
    let ok = first && !b4.seal();
    s.add("9120-J3-重封拒", ok, if ok { "" } else { "重封闸失效" });
    // J3-6 印后指纹漂移（封印位入签）
    let mut b5 = standard_batch();
    let fp_pre = b5.kc_fp();
    let _ = b5.seal();
    let ok = b5.kc_fp() != fp_pre;
    s.add("9120-J3-印后指纹漂移", ok, if ok { "" } else { "封印位未入签" });
    // J3-7 印面渲染一致（封印后渲染含 SEAL 且 TOTAL 行不变）
    let mut b7 = standard_batch();
    let tot_before = b7.total_cases();
    let _ = b7.seal();
    let ok = b7.render().contains("KB16-SEAL") && b7.total_cases() == tot_before;
    s.add("9120-J3-印面渲染一致", ok, if ok { "" } else { "印面渲染漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9120-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9120_all_checks_green() {
        let set = run_unx9120_checks();
        assert!(set.all_passed(), "UNX-F9120 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9120_batch_stable() {
        assert!(standard_batch().batch_ok());
        assert_eq!(standard_batch().total_cases(), 44);
        assert_eq!(standard_batch().total_passed(), 44);
    }
}
