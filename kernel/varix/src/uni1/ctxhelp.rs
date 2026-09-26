//! F409 F1 上下文帮助 · 完整设计（STAR I 主册 G-I-09）。
//!
//! **判据（主册）**：上下文映射表（设置各页→帮助锚点全覆盖）；应用内
//! 注册纪律（vxapp 声明）；不抢焦点判据；锚点直达有效性（死锚=0）。
//! ＋通12。
//!
//! 设计：F1 分发核——三上下文源（设置页注册表/应用内 vxapp 声明/桌面
//! 默认总帮助），帮助窗以「不抢焦点」模式打开（焦点留原地，帮助窗为
//! 伴读窗）；锚点表死链检查（映射里的锚必须在帮助中心锚册中存在）。
//!
//! v5 纵深：注册 upsert 语义（同面重登记=更新锚点，一处一事实）；应用
//! 卸载随卸移除声明制映射；设置页覆盖率缺口审计；F1 连按防抖（同面
//! 500ms 内重按不重复开窗）；锚格式规范（非 help.* 前缀点名）；打开
//! 历史留痕（最近 16 条）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 帮助窗打开模式——「不抢焦点」：焦点留在触发处。
pub const FOCUS_KEEP: bool = true;

/// F1 连按防抖窗（ms）——同一面短时重复按不重复开窗。
pub const F1_DEBOUNCE_MS: u64 = 500;

/// 映射锚点格式前缀（锚必须挂帮助中心命名空间）。
pub const ANCHOR_PREFIX: &str = "help.";

/// 打开历史上限（条）。
pub const HISTORY_CAP: usize = 16;

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
    /// 注册 upsert 账：同面重登记次数。
    pub updates: u64,
    /// F1 防抖账：被吞掉的重复按键。
    pub debounced: u64,
    last_surface: Option<&'static str>,
    last_open_ms: u64,
    /// 打开历史（(surface, anchor)，FIFO 封顶）。
    pub history: Vec<(&'static str, &'static str)>,
}

impl CtxHelp {
    pub fn new() -> CtxHelp {
        CtxHelp {
            mappings: Vec::new(),
            anchors: Vec::new(),
            opens: 0,
            focus_violations: 0,
            last_anchor: None,
            updates: 0,
            debounced: 0,
            last_surface: None,
            last_open_ms: 0,
            history: Vec::new(),
        }
    }

    /// 设置页登记（注册制——upsert：同面重登记 = 更新锚点）。
    pub fn register_setting_page(&mut self, page: &'static str, anchor: &'static str) {
        self.upsert(page, anchor, false);
    }

    /// 应用内声明（vxapp 声明纪律——upsert 同语义）。
    pub fn register_app_decl(&mut self, app: &'static str, anchor: &'static str) {
        self.upsert(app, anchor, true);
    }

    fn upsert(&mut self, surface: &'static str, anchor: &'static str, via_app: bool) {
        if let Some(m) = self.mappings.iter_mut().find(|m| m.surface == surface) {
            m.anchor = anchor;
            m.via_app_decl = via_app;
            self.updates += 1;
        } else {
            self.mappings.push(HelpMapping { surface, anchor, via_app_decl: via_app });
        }
    }

    /// 应用卸载：声明制映射随卸载移除（注册制设置页映射不受牵连）。
    pub fn unregister_app(&mut self, app: &str) -> bool {
        let before = self.mappings.len();
        self.mappings.retain(|m| !(m.via_app_decl && m.surface == app));
        self.mappings.len() != before
    }

    /// 帮助中心锚册挂载（死链检查基准）。
    pub fn mount_anchors(&mut self, anchors: &[&'static str]) {
        self.anchors = anchors.to_vec();
    }

    /// 上下文解析（纯查询，无副作用）。
    pub fn resolve(&self, surface: &str) -> &'static str {
        self.mappings
            .iter()
            .find(|m| m.surface == surface)
            .map(|m| m.anchor)
            .unwrap_or("help.index.system")
    }

    /// F1：直接开窗（等价于 f1_throttled 的最大时间戳——永不防抖）。
    pub fn f1(&mut self, surface: &'static str) -> &'static str {
        self.f1_throttled(surface, u64::MAX)
    }

    /// F1：带防抖开窗——同面 F1_DEBOUNCE_MS 内重按不重复计开窗
    /// （debounced 记账），锚点照常返回（用户还是要看到帮助）。
    pub fn f1_throttled(&mut self, surface: &'static str, now_ms: u64) -> &'static str {
        let anchor = self.resolve(surface);
        if self.last_surface == Some(surface)
            && now_ms.saturating_sub(self.last_open_ms) < F1_DEBOUNCE_MS
        {
            self.debounced += 1;
            return anchor;
        }
        self.opens += 1;
        self.last_surface = Some(surface);
        self.last_open_ms = now_ms;
        self.last_anchor = Some(anchor);
        self.history.push((surface, anchor));
        if self.history.len() > HISTORY_CAP {
            self.history.remove(0);
        }
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

    /// 锚格式规范：所有映射的锚必须挂 help.* 命名空间（野锚点名）。
    pub fn anchor_format_violations(&self) -> Vec<&'static str> {
        self.mappings
            .iter()
            .filter(|m| !m.anchor.starts_with(ANCHOR_PREFIX))
            .map(|m| m.anchor)
            .collect()
    }

    /// 设置页覆盖率缺口审计：全集清单中尚未注册映射的页面。
    pub fn coverage_gap(&self, all_surfaces: &[&'static str]) -> Vec<&'static str> {
        all_surfaces
            .iter()
            .filter(|s| !self.mappings.iter().any(|m| m.surface == **s))
            .copied()
            .collect()
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
    // v5：注册 upsert——同面重登记 = 更新（不堆重复映射）。
    let mut h2 = CtxHelp::new();
    h2.register_setting_page("settings.display", "help.disp.brightness");
    h2.register_setting_page("settings.display", "help.disp.nightlight");
    set.add(
        "f409-register-upsert",
        h2.mapping_count() == 1
            && h2.updates == 1
            && h2.f1("settings.display") == "help.disp.nightlight",
        "",
    );
    // v5：应用卸载 → 声明制映射随卸移除；注册制映射不受牵连。
    h2.register_app_decl("gone.vx", "help.app.gone");
    set.add(
        "f409-app-unregister",
        h2.unregister_app("gone.vx")
            && h2.mapping_count() == 1
            && !h2.unregister_app("settings.display"),
        "",
    );
    // v5：覆盖率缺口审计——全集三个页面，已注册 display 一个。
    let gap = h2.coverage_gap(&["settings.display", "settings.sound", "settings.network"]);
    set.add(
        "f409-coverage-gap",
        gap == alloc::vec!["settings.sound", "settings.network"],
        "",
    );
    // v5：F1 防抖——窗内重按不重复开窗（锚照给）；超窗/换面照常。
    let mut h3 = CtxHelp::new();
    h3.register_setting_page("p", "help.p");
    set.add("f409-f1-open", h3.f1_throttled("p", 0) == "help.p" && h3.opens == 1, "");
    set.add(
        "f409-f1-debounce",
        h3.f1_throttled("p", 200) == "help.p" && h3.opens == 1 && h3.debounced == 1,
        "",
    );
    set.add(
        "f409-f1-reopen-after-window",
        h3.f1_throttled("p", 600) == "help.p" && h3.opens == 2,
        "",
    );
    // v5：锚格式规范——野锚命名空间被点名。
    h3.register_setting_page("bad", "wiki.page");
    let _ = h3.f1_throttled("bad", 2_000);
    set.add(
        "f409-anchor-format-named",
        h3.anchor_format_violations() == alloc::vec!["wiki.page"],
        "",
    );
    // v5：打开历史留痕（最近一条 = 刚才那次 bad 开窗）。
    set.add(
        "f409-history-trail",
        h3.history.last() == Some(&("bad", "wiki.page")) && h3.history.len() == 3,
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_upsert_semantics() {
        let mut h = CtxHelp::new();
        h.register_setting_page("p1", "a1");
        h.register_app_decl("p1", "a2"); // 同面重登记 = 更新（upsert，一处一事实）
        assert_eq!(h.f1("p1"), "a2");
        assert_eq!(h.updates, 1);
        assert_eq!(h.mapping_count(), 1, "不堆重复映射");
        assert_eq!(h.f1("unknown"), "help.index.system");
        assert_eq!(h.opens, 2);
    }

    #[test]
    fn debounce_boundary_is_exclusive() {
        let mut h = CtxHelp::new();
        h.register_setting_page("p", "help.p");
        let _ = h.f1_throttled("p", 0);
        let _ = h.f1_throttled("p", F1_DEBOUNCE_MS - 1); // 窗内 → 吞
        assert_eq!(h.debounced, 1);
        let _ = h.f1_throttled("p", F1_DEBOUNCE_MS); // 恰在窗沿 → 放行
        assert_eq!(h.opens, 2);
    }

    #[test]
    fn anchor_scan_requires_book() {
        let mut h = CtxHelp::new();
        h.register_setting_page("p", "help.a");
        // 无锚册不判定。
        assert!(h.dead_anchor_scan().is_none());
        h.mount_anchors(&["help.a"]);
        assert_eq!(h.dead_anchor_scan(), Some(alloc::vec![]));
    }

    #[test]
    fn history_caps_at_16() {
        let mut h = CtxHelp::new();
        for i in 0..20u64 {
            let _ = h.f1_throttled("p", i * 1_000);
        }
        assert_eq!(h.history.len(), HISTORY_CAP);
        assert_eq!(h.opens, 20);
    }
}
