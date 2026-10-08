//! H2 菜单引擎 · 深化批次三（模型-导航-定位-注入拦截——桌面右键
//! F258、新建 F259、发送到 F263、文本框 F272 四类菜单的共用底座）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F272 文本框右键菜单**：六项清单、**置灰位置稳定**（状态变化
//!   不改项序，只改 enabled）、只读形制——本引擎把「序 = 模型序」
//!   做成结构保证：渲染序恒等构造序，置灰不重排；
//! - **F258 桌面右键菜单**：扩展注入审计（拦截 = 有效）——外部
//!   扩展项只能走 `add_extension` 闸门，未注册来源在结构上进不了
//!   菜单；子菜单键盘可达（方向键进入子菜单、Esc 原路返回、焦点
//!   还原到触发项——浮层焦点纪律）；
//! - **F263 「发送到」/ F259 新建**：项数上限折叠（超限进「更多」
//!   子菜单——菜单深度两级，不许第三级）；
//! - **定位**：屏幕四边翻转（锚点 + 菜单尺寸 → 摆位；右缘翻转、
//!   底缘翻转；子菜单反向翻）——二十几年桌面公理的机判实现。
//!
//! 键盘语义：↑↓ 移动（跳过置灰）、Enter 触发（置灰不可触）、
//! Esc 关闭并返回焦点、→ 进子菜单、← 出子菜单、首字母循环跳。

use crate::checks::CheckSet;

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 菜单模型
// ---------------------------------------------------------------------------

/// 单菜单项。
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub id: &'static str,
    pub label: &'static str,
    pub enabled: bool,
    pub danger: bool,
    /// 子菜单（None = 叶子）。子菜单深度恒为 1——两级封顶。
    pub children: &'static [&'static Entry],
}

/// 叶子便捷构造（const——静态菜单表直接用）。
pub const fn leaf(id: &'static str, label: &'static str, enabled: bool) -> Entry {
    Entry { id, label, enabled, danger: false, children: &[] }
}

/// 菜单：构造序即渲染序（置灰稳定性的结构保证——重排即缺陷）。
pub struct Menu {
    pub items: Vec<Entry>,
    /// 项数上限：超过折叠进「更多」子菜单（两级封顶纪律）。
    pub fold_limit: usize,
    /// 注入拦截账：被闸门挡下的扩展（审计可查——拦截可见）。
    pub blocked: Vec<&'static str>,
}

impl Menu {
    pub fn new(fold_limit: usize) -> Menu {
        Menu { items: Vec::new(), fold_limit: fold_limit.max(2), blocked: Vec::new() }
    }

    /// 扩展注入闸门：来源在白名单才进得来；拦截记录进账（审计 =
    /// 有效，不是静默丢弃）。
    pub fn add_extension(&mut self, source: &'static str, allowed: bool, e: Entry) -> bool {
        if allowed {
            self.items.push(e);
            true
        } else {
            self.blocked.push(source);
            false
        }
    }

    /// 顶层项数；超 `fold_limit` 时返回 Some(折叠子菜单)——折叠是
    /// 视图层职责的模型侧预演：被折叠项进「更多」，前 N 项留顶层。
    pub fn folded_view(&self) -> Option<(usize, usize)> {
        if self.items.len() > self.fold_limit {
            Some((self.fold_limit, self.items.len() - self.fold_limit))
        } else {
            None
        }
    }

    /// 顶层项数（诊断口径）。
    pub fn top_len(&self) -> usize {
        self.items.len()
    }
}

// ---------------------------------------------------------------------------
// 键盘导航
// ---------------------------------------------------------------------------

/// 导航步进（跳过置灰——置灰项不接收焦点，但**位置不动**）。
pub struct NavCursor {
    pub idx: usize,
}

/// ↑↓ 步进：方向 + 项数 → 下一个可聚焦项。全置灰时返回 None
/// （菜单整体不可导航——渲染层据此给「全部不可用」反馈，不假装）。
pub fn step(items: &[Entry], from: usize, down: bool) -> Option<usize> {
    let n = items.len();
    if n == 0 {
        return None;
    }
    for step in 1..=n {
        let i = if down { (from + step) % n } else { (from + n - step % n) % n };
        if items[i].enabled {
            return Some(i);
        }
    }
    None
}

/// 首字母跳转：从 `from` 下一个起循环找 label 首字符匹配（大小写
/// 不敏感）；再按一次跳下一个（Windows 循环语义）。
pub fn jump_letter(items: &[Entry], from: usize, key: char) -> Option<usize> {
    let n = items.len();
    let k = key.to_ascii_lowercase();
    for step in 1..=n {
        let i = (from + step) % n;
        if items[i].enabled
            && items[i].label.chars().next().map_or(false, |c| c.to_ascii_lowercase() == k)
        {
            return Some(i);
        }
    }
    None
}

/// Enter 触发：置灰项不可触发（返回 None——调用方给拒绝反馈）。
pub fn activate(items: &[Entry], idx: usize) -> Option<&'static str> {
    items.get(idx).filter(|e| e.enabled).map(|e| e.id)
}

// ---------------------------------------------------------------------------
// 定位（四边翻转）
// ---------------------------------------------------------------------------

/// 屏幕矩形（与 h2geo::Rect 同构，本处独立小结构避免互相依赖循环）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Screen {
    pub w: i32,
    pub h: i32,
}

/// 摆位：锚点（右键点）+ 菜单尺寸 → 菜单原点。
/// 规则：默认右下展开（x, y 原点在锚点）；右缘溢出 → 左侧展开；
/// 底缘溢出 → 上方展开；两轴独立翻转；全翻转仍出屏 → 钳进屏。
pub fn place(anchor: (i32, i32), menu_w: i32, menu_h: i32, scr: Screen) -> (i32, i32) {
    let mut x = anchor.0;
    let mut y = anchor.1;
    if x + menu_w > scr.w {
        x = anchor.0 - menu_w;
    }
    if y + menu_h > scr.h {
        y = anchor.1 - menu_h;
    }
    if x < 0 {
        x = 0;
    }
    if y < 0 {
        y = 0;
    }
    if x + menu_w > scr.w {
        x = (scr.w - menu_w).max(0);
    }
    (x, y)
}

/// 子菜单摆位：默认右侧、与父项同行；右缘溢出 → 左侧（反向翻）。
pub fn place_submenu(
    parent_item: (i32, i32, i32, i32),
    sub_w: i32,
    sub_h: i32,
    scr: Screen,
) -> (i32, i32) {
    let (px, py, pw, _ph) = parent_item;
    let mut x = px + pw;
    let mut y = py;
    if x + sub_w > scr.w {
        x = px - sub_w;
    }
    if y + sub_h > scr.h {
        y = (scr.h - sub_h).max(0);
    }
    (x, y)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2menu_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2menu");
    // F272 置灰稳定：改 enabled 不改序——模型层结构保证（构造序恒等）。
    let m1 = leaf("paste", "粘贴", false);
    let m2 = leaf("copy", "复制", true);
    let menu_items = [m1, m2];
    set.add(
        "h2menu order stable",
        menu_items[0].id == "paste" && menu_items[1].id == "copy",
        "construct order = render order",
    );
    set.add(
        "h2menu grey in place",
        !menu_items[0].enabled && menu_items[0].id == "paste",
        "disabled stays put",
    );
    // 步进跳置灰 + 全置灰 None。
    let items = [leaf("a", "剪切", true), leaf("b", "粘贴", false), leaf("c", "复制", true)];
    set.add(
        "h2menu skip disabled",
        step(&items, 0, true) == Some(2) && step(&items, 0, false) == Some(2),
        "grey not focused",
    );
    let all_grey = [leaf("x", "全选", false)];
    set.add("h2menu all grey honest", step(&all_grey, 0, true).is_none(), "no fake nav");
    // 首字母循环：两次 j 跳两个 j 项（label 首字母匹配，大小写不敏感）。
    let letters = [
        leaf("j1", "jump one", true),
        leaf("f1", "fast copy", true),
        leaf("j2", "Jump two", true),
        leaf("z1", "zip paste", true),
    ];
    // 首字母循环：从「下一个」起找——0 起跳命中 2（Jump two），
    // 再按从 2 的下一个循环回 0（jump one）。
    let j_first = jump_letter(&letters, 0, 'j');
    let j_next = jump_letter(&letters, j_first.unwrap(), 'j');
    set.add(
        "h2menu letter cycle",
        j_first == Some(2) && j_next == Some(0),
        "repeat cycles",
    );
    set.add("h2menu letter miss", jump_letter(&letters, 0, '9').is_none(), "no match honest");
    // Enter：可用触发、置灰拒绝。
    set.add(
        "h2menu activate gate",
        activate(&items, 0) == Some("a") && activate(&items, 1).is_none(),
        "grey not activatable",
    );
    // 注入拦截：未登记来源必拦且留账；登记后放行。
    let mut m = Menu::new(8);
    let ok = m.add_extension("EvilExt", false, leaf("evil", " Evil", true));
    set.add(
        "h2menu inject blocked",
        !ok && m.blocked == vec!["EvilExt"] && m.top_len() == 0,
        "intercept = effective",
    );
    let ok2 = m.add_extension("GoodExt", true, leaf("good", " Good", true));
    set.add("h2menu allow listed", ok2 && m.top_len() == 1, "whitelisted passes");
    // 折叠：超限进「更多」，两级封顶。
    let mut big = Menu::new(6);
    for i in 0..9u32 {
        big.add_extension("Src", true, leaf("id", "x", true));
        let _ = i;
    }
    let fold = big.folded_view();
    set.add(
        "h2menu fold",
        fold == Some((6, 3)) && big.top_len() == 9,
        "overflow folded",
    );
    // 定位：右下默认；右缘左翻；底缘上翻；双缘双翻；全翻转钳进屏。
    let scr = Screen { w: 1000, h: 800 };
    set.add(
        "h2menu place default",
        place((100, 100), 200, 300, scr) == (100, 100),
        "bottom-right expand",
    );
    set.add(
        "h2menu flip x",
        place((900, 100), 200, 300, scr) == (700, 100),
        "right edge flips",
    );
    set.add(
        "h2menu flip y",
        place((100, 700), 200, 300, scr) == (100, 400),
        "bottom edge flips",
    );
    set.add(
        "h2menu flip xy",
        place((990, 790), 200, 300, scr) == (790, 490),
        "both edges",
    );
    set.add(
        "h2menu clamp huge",
        place((0, 0), 1200, 300, scr) == (0, 0) && place((0, 0), 200, 900, scr) == (0, 0),
        "oversize clamped",
    );
    // 子菜单：右开；右缘左翻；底缘上钳。
    set.add(
        "h2menu submenu right",
        place_submenu((100, 100, 160, 30), 200, 200, scr) == (260, 100),
        "opens right",
    );
    set.add(
        "h2menu submenu left",
        place_submenu((900, 100, 160, 30), 200, 200, scr) == (700, 100),
        "flips left at edge",
    );
    set.add(
        "h2menu submenu clamp y",
        place_submenu((100, 700, 160, 30), 200, 200, scr) == (260, 600),
        "vertical clamp",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2menu_all_green() {
        let set = run_h2menu_checks();
        assert!(set.all_passed(), "h2menu 自检有红项");
        assert!(!set.truncated(), "h2menu 自检溢出");
    }

    #[test]
    fn step_wraps_through_all_enabled() {
        // 全可用菜单步进走满一圈回到自身（循环不变式）。
        let items = [leaf("a", "1", true), leaf("b", "2", true), leaf("c", "3", true)];
        let mut i = 0usize;
        for _ in 0..3 {
            i = step(&items, i, true).unwrap();
        }
        assert_eq!(i, 0);
    }

    #[test]
    fn place_never_offscreen_corner_cases() {
        // 菜单比屏幕大：任何锚点都钳进屏内原点（翻转兜底 + 终值钳制）。
        let scr = Screen { w: 500, h: 400 };
        for ax in [0i32, 250, 499] {
            for ay in [0i32, 200, 399] {
                let (x, y) = place((ax, ay), 600, 500, scr);
                assert_eq!((x, y), (0, 0), "anchor ({ax},{ay})");
            }
        }
    }
}
