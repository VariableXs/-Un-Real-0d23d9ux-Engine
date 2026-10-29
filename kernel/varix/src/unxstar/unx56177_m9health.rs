//! UNX-F56177 M 型批推进体检件（B09 批位）（0x61620 段）
//!
//! 锚点：UNX-F56177-J1 批推进体检账（M 型 B09 批位推进体检 6 项——批
//! 单齐/段位独占/编译绿/注入全抓/指纹确定/封账语义——逐项落账）；J2
//! 前置闸（6 项全 PASS 放行）；J3 体检印（全绿可印）三判据全绿。
//! 域：CoRun·UNX-F2-B40（M 型 B09 体检面）。
//!
//! 结构：六项闭集 + 态机（PASS/FAIL）+ 前置闸 + 差异定位 + 体检印 +
//! 注入核（未全绿放行/未知项/印后改/态漂移/缺项印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61620 独占 0x20）。
pub const SEG_BASE: u32 = 0x61620;
pub const M9_UNK_ITEM: u32 = SEG_BASE + 0x01; // 未知体检项
pub const M9_GATE_BYPASS: u32 = SEG_BASE + 0x02; // 未全绿放行
pub const M9_SEALED_WRITE: u32 = SEG_BASE + 0x03; // 印后改
pub const M9_STATE_DRIFT: u32 = SEG_BASE + 0x04; // 态漂移
pub const M9_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 缺项印

/// 六项闭集。
pub const B9_UNITS: usize = 0;
pub const B9_SEGS: usize = 1;
pub const B9_COMPILED: usize = 2;
pub const B9_INJECT: usize = 3;
pub const B9_FP: usize = 4;
pub const B9_SEAL: usize = 5;
pub const ITEM_N: usize = 6;

/// 项名渲染。
pub const ITEM_NAMES: [&str; ITEM_N] =
    ["units", "segs", "compiled", "inject", "fp", "seal"];

/// 单项账。
#[derive(Clone)]
pub struct M9Item {
    pub item: usize,
    pub passed: bool,
    pub detail: String,
}

impl M9Item {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|{}",
            ITEM_NAMES[self.item],
            if self.passed { "PASS" } else { "FAIL" },
            self.detail
        )
    }
}

/// 体检册。
pub struct M9Book {
    items: [Option<M9Item>; ITEM_N],
    sealed: bool,
}

impl M9Book {
    pub fn new() -> Self {
        M9Book {
            items: [None, None, None, None, None, None],
            sealed: false,
        }
    }

    /// 记一项。
    pub fn record(&mut self, item: usize, passed: bool, detail: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(M9_SEALED_WRITE);
        }
        if item >= ITEM_N {
            return Err(M9_UNK_ITEM);
        }
        if self.items[item].is_some() {
            return Err(M9_UNK_ITEM);
        }
        self.items[item] = Some(M9Item {
            item,
            passed,
            detail: detail.to_string(),
        });
        Ok(())
    }

    pub fn item(&self, item: usize) -> Option<&M9Item> {
        if item >= ITEM_N {
            return None;
        }
        self.items[item].as_ref()
    }

    /// 前置闸：6 项全登记全 PASS。
    pub fn gate_ok(&self) -> bool {
        self.items.iter().all(|i| i.as_ref().map(|x| x.passed).unwrap_or(false))
    }

    /// FAIL 定位串。
    pub fn fail_detail(&self) -> String {
        let mut out = String::new();
        for it in self.items.iter().flatten() {
            if !it.passed {
                out.push_str(&format!("{}:{}\n", ITEM_NAMES[it.item], it.detail));
            }
        }
        if out.is_empty() {
            out.push_str("ALL-PASS\n");
        }
        out
    }

    /// 体检印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.gate_ok() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for it in self.items.iter().flatten() {
            out.push_str(&it.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("M9-SEAL\n");
        }
        out
    }
}

impl Default for M9Book {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：六项全 PASS。
fn standard_book() -> M9Book {
    let mut b = M9Book::new();
    for i in 0..ITEM_N {
        let _ = b.record(i, true, "");
    }
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知项
    let mut b = M9Book::new();
    if b.record(9, true, "") == Err(M9_UNK_ITEM) {
        caught += 1;
    }
    // 注 2：未全绿放行（一项 FAIL gate_ok 必假）
    let mut b = M9Book::new();
    for i in 0..ITEM_N {
        let _ = b.record(i, i != B9_COMPILED, "err");
    }
    if !b.gate_ok() {
        caught += 1;
    }
    // 注 3：缺项放行假
    let mut b = M9Book::new();
    for i in 0..ITEM_N - 1 {
        let _ = b.record(i, true, "");
    }
    if !b.gate_ok() {
        caught += 1;
    }
    // 注 4：印后改
    let mut b = standard_book();
    let _ = b.seal();
    if b.record(0, false, "x") == Err(M9_SEALED_WRITE) {
        caught += 1;
    }
    // 注 5：空册印
    let mut b = M9Book::new();
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 体检账；J2 前置闸；J3 体检印。
pub fn run_unx56177_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56177");
    // J1-1 六项全落账
    let b = standard_book();
    let ok = (0..ITEM_N).all(|i| b.item(i).is_some()) && b.gate_ok();
    s.add("56177-J1-六项全落账", ok, if ok { "" } else { "体检账不全" });
    // J1-2 渲染导出齐
    let text = standard_book().render_book();
    let ok = text.contains("units|PASS|") && text.contains("seal|PASS|");
    s.add("56177-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 全 PASS 放行
    let ok = standard_book().gate_ok();
    s.add("56177-J2-全PASS放行", ok, if ok { "" } else { "闸漂移" });
    // J2-2 单 FAIL 拦
    let mut b = M9Book::new();
    for i in 0..ITEM_N {
        let _ = b.record(i, i != B9_INJECT, "x");
    }
    let ok = !b.gate_ok();
    s.add("56177-J2-单FAIL拦", ok, if ok { "" } else { "闸穿透" });
    // J2-3 FAIL 定位
    let ok = b.fail_detail().contains("inject:x");
    s.add("56177-J2-FAIL定位", ok, if ok { "" } else { "定位漂移" });
    // J2-4 全绿显示 ALL-PASS
    let ok = standard_book().fail_detail() == "ALL-PASS\n";
    s.add("56177-J2-ALL-PASS", ok, if ok { "" } else { "定位面漂移" });
    // J3-1 体检印
    let mut b = standard_book();
    let ok = b.seal() && b.sealed();
    s.add("56177-J3-体检印", ok, if ok { "" } else { "体检印失败" });
    // J3-2 印后只读
    let ok = b.record(0, false, "x") == Err(M9_SEALED_WRITE);
    s.add("56177-J3-印后只读", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = b.render_book().contains("M9-SEAL");
    s.add("56177-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 项名闭集
    let ok = ITEM_NAMES[B9_UNITS] == "units" && ITEM_NAMES[B9_SEAL] == "seal";
    s.add("56177-J3-项名闭集", ok, if ok { "" } else { "名漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56177-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56177_all_checks_green() {
        let set = run_unx56177_checks();
        assert!(set.all_passed(), "UNX-F56177 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56177_gate_stable() {
        assert!(standard_book().gate_ok());
    }
}
