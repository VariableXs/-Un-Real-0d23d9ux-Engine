//! UNX-F56159 F 型阶段体检件（波中体检前置）（0x613E0 段）
//!
//! 锚点：UNX-F56159-J1 体检项账（波中体检前置 8 项——批单齐/段位独
//! 占/判据注册/编译绿/注入核全抓/指纹确定/封账语义/渲染导出——逐项
//! 落账：项 × 态 × 明细）；J2 前置闸语义（8 项全 PASS 才放行波中体检，
//! 任一 FAIL 带定位明细）；J3 体检印（8 项全绿可印，印后只读）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（F 型体检面）。
//!
//! 结构：八项体检登记册（项名闭集 × 态机 PASS/FAIL/WARN）+ 前置闸
//! （全 PASS 放行判定）+ 差异明细（FAIL 项定位串）+ 体检印 + 注入核
//! （未全绿放行/未知项/印后改账/态漂移/空册印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x613E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x613E0;
pub const CHK_UNK_ITEM: u32 = SEG_BASE + 0x01; // 未知体检项
pub const CHK_GATE_BYPASS: u32 = SEG_BASE + 0x02; // 未全绿放行
pub const CHK_SEALED_WRITE: u32 = SEG_BASE + 0x03; // 印后改账
pub const CHK_STATE_DRIFT: u32 = SEG_BASE + 0x04; // 态漂移
pub const CHK_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空册印

/// 八项闭集。
pub const I_UNITS: usize = 0; // 批单齐
pub const I_SEGMENTS: usize = 1; // 段位独占
pub const I_REGISTERED: usize = 2; // 判据注册
pub const I_COMPILED: usize = 3; // 编译绿
pub const I_INJECT: usize = 4; // 注入核全抓
pub const I_FP: usize = 5; // 指纹确定
pub const I_SEAL: usize = 6; // 封账语义
pub const I_RENDER: usize = 7; // 渲染导出
pub const ITEM_COUNT: usize = 8;

/// 项名渲染。
pub const ITEM_NAMES: [&str; ITEM_COUNT] = [
    "units", "segments", "registered", "compiled", "inject", "fp", "seal", "render",
];

/// 态闭集。
pub const S_PASS: u8 = 0;
pub const S_FAIL: u8 = 1;
pub const S_WARN: u8 = 2;

/// 态名渲染。
pub fn state_name(st: u8) -> &'static str {
    match st {
        S_PASS => "PASS",
        S_FAIL => "FAIL",
        S_WARN => "WARN",
        _ => "?",
    }
}

/// 前置闸核：8 项全 PASS 才放行（WARN 不放行——前置闸严格口径）。
pub fn gate_open(states: &[u8; ITEM_COUNT]) -> bool {
    states.iter().all(|s| *s == S_PASS)
}

/// 单项体检账。
#[derive(Clone)]
pub struct CheckItem {
    pub item: usize,
    pub state: u8,
    pub detail: String,
}

impl CheckItem {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|{}",
            ITEM_NAMES[self.item],
            state_name(self.state),
            self.detail
        )
    }
}

/// 体检册。
pub struct HealthBook {
    items: [Option<CheckItem>; ITEM_COUNT],
    sealed: bool,
}

impl HealthBook {
    pub fn new() -> Self {
        HealthBook {
            items: [None, None, None, None, None, None, None, None],
            sealed: false,
        }
    }

    /// 记一项：项闭集 + 印后拒 + 重记拒。
    pub fn record(&mut self, item: usize, state: u8, detail: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CHK_SEALED_WRITE);
        }
        if item >= ITEM_COUNT {
            return Err(CHK_UNK_ITEM);
        }
        if self.items[item].is_some() {
            return Err(CHK_UNK_ITEM);
        }
        self.items[item] = Some(CheckItem {
            item,
            state,
            detail: detail.to_string(),
        });
        Ok(())
    }

    pub fn item(&self, item: usize) -> Option<&CheckItem> {
        if item >= ITEM_COUNT {
            return None;
        }
        self.items[item].as_ref()
    }

    /// 态全读。
    pub fn states(&self) -> [u8; ITEM_COUNT] {
        let mut st = [S_FAIL; ITEM_COUNT];
        for (i, s) in st.iter_mut().enumerate() {
            if let Some(it) = &self.items[i] {
                *s = it.state;
            }
        }
        st
    }

    /// 前置闸：8 项全登记 + 全 PASS。
    pub fn gate_ok(&self) -> bool {
        self.items.iter().all(|i| i.is_some()) && gate_open(&self.states())
    }

    /// FAIL 项定位串。
    pub fn fail_detail(&self) -> String {
        let mut out = String::new();
        for it in self.items.iter().flatten() {
            if it.state == S_FAIL {
                out.push_str(&format!("{}:{}\n", ITEM_NAMES[it.item], it.detail));
            }
        }
        if out.is_empty() {
            out.push_str("NO-FAIL\n");
        }
        out
    }

    /// 体检印：闸全绿才可印。
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
            out.push_str("HEALTH-SEAL\n");
        }
        out
    }
}

impl Default for HealthBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：八项全 PASS。
fn standard_book() -> HealthBook {
    let mut b = HealthBook::new();
    for i in 0..ITEM_COUNT {
        let _ = b.record(i, S_PASS, "");
    }
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知项
    let mut b = HealthBook::new();
    if b.record(9, S_PASS, "") == Err(CHK_UNK_ITEM) {
        caught += 1;
    }
    // 注 2：未全绿放行（一项 FAIL 时 gate_ok 必假）
    let mut b = standard_book();
    // 重写一项为 FAIL：先清空重造
    let mut b2 = HealthBook::new();
    for i in 0..ITEM_COUNT {
        let _ = b2.record(i, if i == I_COMPILED { S_FAIL } else { S_PASS }, "err");
    }
    if !b2.gate_ok() {
        caught += 1;
    }
    let _ = b;
    // 注 3：缺项不放行
    let mut b = HealthBook::new();
    for i in 0..ITEM_COUNT - 1 {
        let _ = b.record(i, S_PASS, "");
    }
    if !b.gate_ok() {
        caught += 1;
    }
    // 注 4：印后改账
    let mut b = standard_book();
    let _ = b.seal();
    if b.record(0, S_FAIL, "x") == Err(CHK_SEALED_WRITE) {
        caught += 1;
    }
    // 注 5：空册印
    let mut b = HealthBook::new();
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 体检项账；J2 前置闸；J3 体检印。
pub fn run_unx56159_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56159");
    // J1-1 八项全落账
    let b = standard_book();
    let ok = (0..ITEM_COUNT).all(|i| b.item(i).is_some()) && b.gate_ok();
    s.add("56159-J1-八项全落账", ok, if ok { "" } else { "体检账不全" });
    // J1-2 渲染导出齐
    let text = standard_book().render_book();
    let ok = text.contains("units|PASS|") && text.contains("render|PASS|");
    s.add("56159-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 全 PASS 放行
    let ok = gate_open(&[S_PASS; ITEM_COUNT]);
    s.add("56159-J2-全PASS放行", ok, if ok { "" } else { "闸漂移" });
    // J2-2 单 FAIL 拦
    let mut st = [S_PASS; ITEM_COUNT];
    st[I_FP] = S_FAIL;
    let ok = !gate_open(&st);
    s.add("56159-J2-单FAIL拦", ok, if ok { "" } else { "闸穿透" });
    // J2-3 WARN 不放行（严格口径）
    let mut st = [S_PASS; ITEM_COUNT];
    st[I_RENDER] = S_WARN;
    let ok = !gate_open(&st);
    s.add("56159-J2-WARN不放行", ok, if ok { "" } else { "口径漂移" });
    // J2-4 FAIL 定位串
    let mut b = HealthBook::new();
    for i in 0..ITEM_COUNT {
        let _ = b.record(i, if i == I_SEGMENTS { S_FAIL } else { S_PASS }, if i == I_SEGMENTS { "seg dup" } else { "" });
    }
    let ok = b.fail_detail().contains("segments:seg dup");
    s.add("56159-J2-FAIL定位", ok, if ok { "" } else { "定位漂移" });
    // J2-5 无 FAIL 显示 NO-FAIL
    let ok = standard_book().fail_detail() == "NO-FAIL\n";
    s.add("56159-J2-NO-FAIL", ok, if ok { "" } else { "定位面漂移" });
    // J3-1 体检印
    let mut b = standard_book();
    let ok = b.seal() && b.sealed();
    s.add("56159-J3-体检印", ok, if ok { "" } else { "体检印失败" });
    // J3-2 印后只读
    let ok = b.record(0, S_FAIL, "x") == Err(CHK_SEALED_WRITE);
    s.add("56159-J3-印后只读", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = b.render_book().contains("HEALTH-SEAL");
    s.add("56159-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 态名闭集
    let ok = state_name(S_PASS) == "PASS" && state_name(S_WARN) == "WARN" && state_name(9) == "?";
    s.add("56159-J3-态名闭集", ok, if ok { "" } else { "态名漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56159-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56159_all_checks_green() {
        let set = run_unx56159_checks();
        assert!(set.all_passed(), "UNX-F56159 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56159_gate_stable() {
        assert!(gate_open(&[S_PASS; ITEM_COUNT]));
        let mut st = [S_PASS; ITEM_COUNT];
        st[0] = S_FAIL;
        assert!(!gate_open(&st));
    }
}
