//! F304 单页还原默认 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：页级还原只碰已改项判据；改动标记点（自定义值可视
//! 化）；逐条还原用例；还原后即时反馈；误触保护（确认框）。
//!
//! **设计要点（主册）**：
//! - 每个设置页右上角「还原默认」：一键把本页全部条目恢复出厂值，执行
//!   前显示将改变哪些项（勾选确认——只还原动了的那几项，没动的不碰）；
//! - 单条目另有逐条还原（条目右侧自定义值标记点，点标记还原该条）；
//! - 无感标准：试设置像试衣服——乱调一气之后一键回原样，且「原样」是
//!   精确的（逐条知道哪些动了）；恢复默认永远是安全出口。
//!
//! 数据面在 [`super::hbase::SettingRegistry`]（改动追踪唯一源）；本模块
//! 实现确认流状态机（预览→确认/取消）与标记点视图。

use crate::checks::CheckSet;

use super::hbase::{SettingRegistry, INSTANT_FEEDBACK_MS};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 改动标记点（自定义值可视化）
// ---------------------------------------------------------------------------

/// 一个改动标记点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModMark {
    pub item: &'static str,
    /// 当前值。
    pub value: i64,
    /// 出厂默认值。
    pub default: i64,
}

/// 页内改动标记点清单（空 = 本页全出厂态——标记点不显示）。
pub fn page_modifications(reg: &SettingRegistry, page: &str) -> Vec<ModMark> {
    reg.items()
        .iter()
        .filter(|i| i.page == page && i.value != i.default)
        .map(|i| ModMark { item: i.name, value: i.value, default: i.default })
        .collect()
}

/// 单条目是否有自定义值标记点。
pub fn item_marked(reg: &SettingRegistry, name: &str) -> bool {
    reg.items()
        .iter()
        .any(|i| i.name == name && i.value != i.default)
}

// ---------------------------------------------------------------------------
// 页级还原确认流（预览 → 确认/取消）
// ---------------------------------------------------------------------------

/// 还原预览（确认框内容：将还原哪些项——只列已改项）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResetPreview {
    pub page: String,
    /// 将被还原的条目（只含已改项——顺序同登记序，确定）。
    pub will_reset: Vec<&'static str>,
    /// 明确不动 untouched 的条目数（预览可解释性）。
    pub untouched: usize,
}

/// 页级还原确认流状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResetStage {
    /// 无进行中的还原。
    Idle,
    /// 预览已出（确认框在场）。
    Preview,
    /// 已执行（终态）。
    Done,
}

pub struct PageResetFlow {
    stage: ResetStage,
    pending: Option<ResetPreview>,
}

impl PageResetFlow {
    pub fn new() -> PageResetFlow {
        PageResetFlow { stage: ResetStage::Idle, pending: None }
    }

    /// 请求页级还原：出预览（误触保护第一步——不动值）。
    pub fn request(&mut self, reg: &SettingRegistry, page: &str) -> Option<ResetPreview> {
        let mut will_reset: Vec<&'static str> = Vec::new();
        let mut untouched = 0usize;
        for i in reg.items().iter().filter(|i| i.page == page) {
            if i.value != i.default {
                will_reset.push(i.name);
            } else {
                untouched += 1;
            }
        }
        let preview =
            ResetPreview { page: String::from(page), will_reset, untouched };
        self.stage = ResetStage::Preview;
        self.pending = Some(preview.clone());
        Some(preview)
    }

    /// 全出厂态页：预览为空清单（按钮置灰语义——无可还原项）。
    pub fn pending_is_empty(&self) -> bool {
        self.pending.as_ref().map(|p| p.will_reset.is_empty()).unwrap_or(true)
    }

    /// 确认执行：只还原预览列出的项（已改项），返回即时反馈预算。
    pub fn confirm(&mut self, reg: &mut SettingRegistry) -> Option<(u64, Vec<&'static str>)> {
        if self.stage != ResetStage::Preview {
            return None;
        }
        let page = self.pending.as_ref()?.page.clone();
        let restored = reg.restore_page_defaults(&page);
        self.stage = ResetStage::Done;
        Some((INSTANT_FEEDBACK_MS, restored))
    }

    /// 取消（安全出路——什么都没发生）。
    pub fn cancel(&mut self) {
        self.stage = ResetStage::Idle;
        self.pending = None;
    }

    pub fn in_preview(&self) -> bool {
        self.stage == ResetStage::Preview
    }

    /// 终态后再确认拒绝（状态机闭环——不留半空状态）。
    pub fn confirm_twice_rejected(&self) -> bool {
        self.stage == ResetStage::Done
    }
}

impl Default for PageResetFlow {
    fn default() -> PageResetFlow {
        PageResetFlow::new()
    }
}

// ---------------------------------------------------------------------------
// 逐条还原
// ---------------------------------------------------------------------------

/// 单条目还原（点标记点）：只动该条；还原后即时反馈预算 <100ms。
/// 未改动的条目返回 None（标记点本就不显示——点了也不该有动作）。
pub fn restore_one(reg: &mut SettingRegistry, name: &str) -> Option<u64> {
    if reg.restore_item_default(name) {
        Some(INSTANT_FEEDBACK_MS)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F304 自检（判据：只碰已改项；标记点；逐条还原；即时反馈；确认框）。
pub fn run_pagedflt_checks() -> CheckSet {
    let mut set = CheckSet::new("F304-pagedflt");

    let mut reg = SettingRegistry::new();
    reg.add_page(super::hbase::SettingPage { path: "系统/显示", subtitle: "这里调屏幕怎么显示", cross_links: &[] });
    let mk = |name: &'static str, d: i64| super::hbase::SettingItem {
        name,
        page: "系统/显示",
        synonyms: &["a", "b", "c"],
        effect: super::hbase::EffectKind::Instant,
        default: d,
        value: d,
        control: super::hbase::ControlKind::Slider,
    };
    for (n, d) in [("亮度", 70i64), ("对比度", 50), ("色温", 0)] {
        reg.add_item(mk(n, d));
    }

    // 1. 出厂态：标记点清单为空（可视化不显示）。
    set.add("clean page shows no marks", page_modifications(&reg, "系统/显示").is_empty(), "");

    // 2. 乱调一气：标记点逐条报出（自定义值可视化）。
    reg.set_value("亮度", 90);
    reg.set_value("色温", 2);
    let marks = page_modifications(&reg, "系统/显示");
    set.add(
        "marks list modified only",
        marks.len() == 2
            && marks[0] == ModMark { item: "亮度", value: 90, default: 70 }
            && marks[1] == ModMark { item: "色温", value: 2, default: 0 },
        "",
    );

    // 3. 误触保护：请求出预览（未执行——值不动）。
    let mut flow = PageResetFlow::new();
    let preview = flow.request(&reg, "系统/显示").unwrap();
    set.add(
        "preview lists will reset only",
        flow.in_preview()
            && preview.will_reset == vec!["亮度", "色温"]
            && preview.untouched == 1
            && reg.items().iter().find(|i| i.name == "亮度").unwrap().value == 90,
        "",
    );

    // 4. 取消：安全出路（什么都没发生）。
    flow.cancel();
    set.add(
        "cancel leaves values untouched",
        !flow.in_preview()
            && reg.items().iter().find(|i| i.name == "亮度").unwrap().value == 90,
        "",
    );

    // 5. 确认执行：只碰已改项（对比度 50 未动），即时反馈 <100ms。
    let mut flow = PageResetFlow::new();
    let _ = flow.request(&reg, "系统/显示");
    let (budget, restored) = flow.confirm(&mut reg).unwrap();
    set.add(
        "confirm touches only changed",
        budget == INSTANT_FEEDBACK_MS
            && restored == vec!["亮度", "色温"]
            && reg.items().iter().find(|i| i.name == "对比度").unwrap().value == 50
            && reg.items().iter().find(|i| i.name == "亮度").unwrap().value == 70,
        "",
    );

    // 6. 终态状态机：Done 后再确认拒绝（无半空状态）。
    set.add("confirm twice rejected", flow.confirm_twice_rejected() && flow.confirm(&mut reg).is_none(), "");

    // 7. 逐条还原：改动条返回预算；未改条返回 None（标记点不显示语义）。
    reg.set_value("对比度", 80);
    let b1 = restore_one(&mut reg, "对比度");
    let b2 = restore_one(&mut reg, "对比度");
    set.add(
        "single restore one-shot",
        b1 == Some(INSTANT_FEEDBACK_MS)
            && b2.is_none()
            && reg.items().iter().find(|i| i.name == "对比度").unwrap().value == 50,
        "",
    );

    // 8. 全出厂页：预览空清单（按钮置灰语义）。
    let mut flow = PageResetFlow::new();
    let preview = flow.request(&reg, "系统/显示").unwrap();
    set.add(
        "empty preview grays button",
        preview.will_reset.is_empty() && flow.pending_is_empty(),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> SettingRegistry {
        let mut reg = SettingRegistry::new();
        reg.add_page(crate::h3star::hbase::SettingPage { path: "a/b", subtitle: "s", cross_links: &[] });
        reg.add_item(crate::h3star::hbase::SettingItem {
            name: "x",
            page: "a/b",
            synonyms: &["1", "2", "3"],
            effect: crate::h3star::hbase::EffectKind::Instant,
            default: 1,
            value: 1,
            control: crate::h3star::hbase::ControlKind::Toggle,
        });
        reg
    }

    #[test]
    fn request_without_confirm_never_writes() {
        let mut reg = demo();
        reg.set_value("x", 9);
        let mut flow = PageResetFlow::new();
        let _ = flow.request(&reg, "a/b");
        flow.cancel();
        assert_eq!(reg.items()[0].value, 9);
    }

    #[test]
    fn confirm_before_request_rejected() {
        let mut reg = demo();
        let mut flow = PageResetFlow::new();
        assert!(flow.confirm(&mut reg).is_none());
    }

    #[test]
    fn other_pages_untouched_by_page_reset() {
        let mut reg = demo();
        reg.add_page(crate::h3star::hbase::SettingPage { path: "c/d", subtitle: "s2", cross_links: &[] });
        reg.add_item(crate::h3star::hbase::SettingItem {
            name: "y",
            page: "c/d",
            synonyms: &["1", "2", "3"],
            effect: crate::h3star::hbase::EffectKind::Instant,
            default: 5,
            value: 5,
            control: crate::h3star::hbase::ControlKind::Toggle,
        });
        reg.set_value("x", 7);
        reg.set_value("y", 9);
        let mut flow = PageResetFlow::new();
        let _ = flow.request(&reg, "a/b");
        let (_, restored) = flow.confirm(&mut reg).unwrap();
        assert_eq!(restored, vec!["x"]);
        assert_eq!(reg.items().iter().find(|i| i.name == "y").unwrap().value, 9);
    }
}
