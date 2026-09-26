//! F409 F1 上下文帮助 · 完整设计（STAR I 主册 G-I-09）。
//!
//! **判据（主册）**：上下文映射表（设置各页→帮助锚点全覆盖）；应用内
//! 注册纪律（vxapp 声明）；不抢焦点判据；锚点直达有效性（死锚=0）。
//! ＋通12。
//!
//! 设计：F1 分发核——三上下文源（设置页注册表/应用内 vxapp 声明/桌面
//! 默认总帮助），帮助窗以「不抢焦点」模式打开（焦点留原地，帮助窗为
//! 伴读窗）；锚点表死链检查（映射里的锚必须在帮助中心锚册中存在）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 帮助窗打开模式——「不抢焦点」：焦点留在触发处。
pub const FOCUS_KEEP: bool = true;

/// 一条上下文映射：触发面 → 帮助锚点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelpMapping {
    /// 设置页路径或应用 ID。
    pub surface: &'static str,
    /// 帮助中心锚点（F119 章节）。
    pub anchor: &'static str,
    /// 来源：设置注册制 / 应用声明制。
    pub via_app_decl: bool,
}

/// F1 分发核。
pub struct CtxHelp {
    mappings: Vec<HelpMapping>,
    /// 帮助中心锚册（F119 的锚全集——死链检查的基准）。
    anchors: Vec<&'static str>,
    /// 打开次数/不抢焦点违例数（判据账）。
    pub opens: u64,
    pub focus_violations: u64,
    /// 最近一次解析结果。
    pub last_anchor: Option<&'static str>,
}

impl CtxHelp {
    pub fn new() -> CtxHelp {
        CtxHelp {
            mappings: Vec::new(),
            anchors: Vec::new(),
            opens: 0,
            focus_violations: 0,
            last_anchor: None,
        }
    }

    /// 设置页登记（注册制——新增设置页必须登记，一处一事实）。
    pub fn register_setting_page(&mut self, page: &'static str, anchor: &'static str) {
        self.mappings.push(HelpMapping { surface: page, anchor, via_app_decl: false });
    }

    /// 应用内声明（vxapp 声明纪律——应用安装时随清单登记）。
    pub fn register_app_decl(&mut self, app: &'static str, anchor: &'static str) {
        self.mappings.push(HelpMapping { surface: app, anchor, via_app_decl: true });
    }

    /// 帮助中心锚册挂载（死链检查基准）。
    pub fn mount_anchors(&mut self, anchors: &[&'static str]) {
        self.anchors = anchors.to_vec();
    }

    /// F1：上下文感知解析——精确命中 → 该锚；未命中 → 桌面默认总帮助
    /// 锚（永不弹「要自己再找的目录页」——总帮助也是直达锚）。
    pub fn f1(&mut self, surface: &'static str) -> &'static str {
        self.opens += 1;
        let anchor = self
            .mappings
            .iter()
            .find(|m| m.surface == surface)
            .map(|m| m.anchor)
            .unwrap_or("help.index.system");
        self.last_anchor = Some(anchor);
        anchor
    }

    /// 焦点纪律：帮助窗打开时不抢焦点——违例上报入口（每违例计数）。
    pub fn report_focus_steal(&mut self, stole: bool) {
        if stole {
            self.focus_violations += 1;
        }
    }

    /// 死锚检查：所有映射的锚必须在锚册中（死锚=0 判据）。
    /// 锚册未挂载返回 None（无法判定——不谎报通过）。
    pub fn dead_anchor_scan(&self) -> Option<Vec<&'static str>> {
        if self.anchors.is_empty() {
            return None;
        }
        let dead: Vec<&'static str> = self
            .mappings
            .iter()
            .filter(|m| !self.anchors.contains(&m.anchor))
            .map(|m| m.anchor)
            .collect();
        Some(dead)
    }

    pub fn mapping_count(&self) -> usize {
        self.mappings.len()
    }
}

pub fn run_ctxhelp_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F409");
    let mut h = CtxHelp::new();
    h.register_setting_page("settings.display", "help.disp.brightness");
    h.register_setting_page("settings.sound", "help.snd.volume");
    h.register_app_decl("notepad.vx", "help.app.notepad");
    set.add(
        "f409-mappings-registered",
        h.mapping_count() == 3,
        "",
    );
    // 上下文感知：设置页/应用/桌面三路解析。
    set.add("f409-setting-hit", h.f1("settings.sound") == "help.snd.volume", "");
    set.add("f409-app-hit", h.f1("notepad.vx") == "help.app.notepad", "");
    set.add("f409-desktop-default", h.f1("desktop") == "help.index.system", "");
    // 不抢焦点：默认纪律 + 违例记账。
    set.add("f409-focus-kept", FOCUS_KEEP && h.focus_violations == 0, "");
    h.report_focus_steal(true);
    h.report_focus_steal(false);
    set.add("f409-focus-violation-logged", h.focus_violations == 1, "");
    // 死锚=0：锚册齐 → 零死锚；锚册缺 → 显式 None（不谎报）。
    set.add("f409-no-anchor-book-no-claim", h.dead_anchor_scan().is_none(), "");
    h.mount_anchors(&["help.disp.brightness", "help.snd.volume", "help.app.notepad"]);
    set.add(
        "f409-dead-anchor-zero",
        h.dead_anchor_scan().map(|d| d.is_empty()).unwrap_or(false),
        "",
    );
    // 死锚检出：注册一个锚册没有的锚 → 被点名。
    h.register_setting_page("settings.network", "help.net.missing");
    set.add(
        "f409-dead-anchor-named",
        h.dead_anchor_scan().as_ref().map(|d| d == &alloc::vec!["help.net.missing"]).unwrap_or(false),
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_resolution_priority() {
        let mut h = CtxHelp::new();
        h.register_setting_page("p1", "a1");
        h.register_app_decl("p1", "a2"); // 同名面：后注册覆盖不成立——首条命中
        assert_eq!(h.f1("p1"), "a1");
        assert_eq!(h.last_anchor, Some("a1"));
        assert_eq!(h.f1("unknown"), "help.index.system");
        assert_eq!(h.opens, 2);
    }

    #[test]
    fn anchor_scan_requires_book() {
        let mut h = CtxHelp::new();
        h.register_setting_page("p", "a");
        // 无锚册不判定。
        assert!(h.dead_anchor_scan().is_none());
        h.mount_anchors(&["a"]);
        assert_eq!(h.dead_anchor_scan(), Some(alloc::vec![]));
    }
}
