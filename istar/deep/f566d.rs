//! 深化层 · F566 文件夹颜色标记（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F566 节）：
//! ①**三处呈现一致性对账引擎**——同一标记在图标底条/详情色点/树视图
//!   色条的派生参数同源：色值取 [`TintBook::render_token`] 唯一令牌，
//!   几何从基础层 `bar_width_px` 一处换算三分（色点 = 色条半宽），
//!   未标记路径三处一律归 None（不渲染即不几何）；
//! ②**过滤器状态机**——工具栏色点行的多色叠加过滤：toggle 叠色/再点
//!   退出、任意命中即入选、清除归零恢复全过；
//! ③**图标兼容仲裁**——F452 自定义图标在场时色条让位（半宽），仲裁
//!   一处裁决三处服从（令牌不变，只让几何）；
//! ④**12 色语义表持久化对账**——序号往返无损 + 越界拒绝 + 存档→还原
//!   →逐路径令牌一致（重读同色，不许读一次变一次）。

use crate::checks::CheckSet;
use crate::istar::foldertint::TintBook;
use crate::istar::ibase::{ISTAR_DOMAIN, Tint12};

// ---------------------------------------------------------------------------
// ① 三处呈现一致性对账引擎
// ---------------------------------------------------------------------------

/// 三视图几何派生（图标底条 / 详情色点 / 树视图色条）。
pub struct ViewRender {
    pub icon_bar_px: u32,
    pub dot_px: u32,
    pub tree_bar_px: u32,
}

/// 从簿册派生三视图几何（让位仲裁随 `bar_width_px` 一处生效）。
pub fn derive_views(book: &TintBook, path: &str) -> Option<ViewRender> {
    let bar = book.bar_width_px(path)?;
    Some(ViewRender { icon_bar_px: bar, dot_px: bar / 2, tree_bar_px: bar })
}

/// 一致性对账：令牌唯一源 + 几何同规（色点 = 色条半宽、树条 = 图标条）。
pub fn views_consistent(book: &TintBook, path: &str) -> bool {
    let token = match book.render_token(path) {
        Some(t) => t,
        None => return false,
    };
    let r = match derive_views(book, path) {
        Some(r) => r,
        None => return false,
    };
    token.starts_with("tint-") && r.dot_px * 2 == r.icon_bar_px && r.tree_bar_px == r.icon_bar_px
}

// ---------------------------------------------------------------------------
// ② 过滤器状态机
// ---------------------------------------------------------------------------

/// 颜色过滤器（12 色位掩码——多色叠加、再点退出、清除归零）。
pub struct TintFilter {
    mask: u16,
}

impl TintFilter {
    pub fn new() -> TintFilter {
        TintFilter { mask: 0 }
    }

    /// 叠色 / 再点退出（幂等翻转）。
    pub fn toggle(&mut self, t: Tint12) {
        self.mask ^= 1 << t.index();
    }

    pub fn active(&self, t: Tint12) -> bool {
        self.mask & (1 << t.index()) != 0
    }

    pub fn any_active(&self) -> bool {
        self.mask != 0
    }

    /// 清除：恢复全过态。
    pub fn clear(&mut self) {
        self.mask = 0;
    }

    /// 过滤裁决：无激活色 = 全过（不过滤）；有激活色 = 命中任一即入选。
    pub fn admits(&self, t: Tint12) -> bool {
        !self.any_active() || self.active(t)
    }

    /// 对簿册应用过滤：激活色的入选目录并集。
    pub fn apply<'a>(&self, book: &'a TintBook) -> alloc::vec::Vec<&'a str> {
        let mut out: alloc::vec::Vec<&'a str> = alloc::vec::Vec::new();
        for t in Tint12::ALL.iter() {
            if self.active(*t) {
                for p in book.filter_by(*t) {
                    out.push(p);
                }
            }
        }
        out
    }
}

impl Default for TintFilter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// ④ 12 色语义表持久化对账
// ---------------------------------------------------------------------------

/// 对账①：十二枚序号往返无损 + 越界序号诚实拒绝。
pub fn index_roundtrip_ok() -> bool {
    Tint12::ALL.iter().all(|&t| Tint12::from_index(t.index()) == Some(t))
        && Tint12::from_index(12).is_none()
        && Tint12::from_index(255).is_none()
}

/// 对账②：存档（路径 → 序号）→ 新簿还原 → 逐路径令牌一致（重读同色）。
pub fn reload_tokens_match(original: &TintBook, paths: &[&str]) -> bool {
    let mut saved: [Option<(usize, u8)>; 32] = [None; 32];
    let mut n = 0usize;
    for (i, p) in paths.iter().enumerate() {
        if let Some(m) = original.of(p) {
            if n < 32 {
                saved[n] = Some((i, m.tint.index()));
                n += 1;
            }
        }
    }
    let mut re = TintBook::new();
    for i in 0..n {
        let (pi, ix) = match saved[i] {
            Some(x) => x,
            None => return false,
        };
        let t = match Tint12::from_index(ix) {
            Some(t) => t,
            None => return false,
        };
        if !re.mark(paths[pi], t) {
            return false;
        }
    }
    paths.iter().all(|p| {
        let a = original.of(p).map(|m| m.tint.token());
        let b = re.of(p).map(|m| m.tint.token());
        a == b
    })
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f566_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 三处一致性：已标记路径令牌在册 + 几何同规；未标记路径对不上。
    let mut b = TintBook::new();
    b.mark("项目A", Tint12::Red);
    b.mark("账单", Tint12::Yellow);
    b.mark("个人", Tint12::Green);
    cs.add(
        "views consistent marked only",
        views_consistent(&b, "项目A") && !views_consistent(&b, "未标记"),
        "",
    );

    // 2) 图标兼容仲裁：无图标 12px/色点 6；有图标让位 6px/色点 3。
    let mut b2 = TintBook::new();
    b2.mark("甲", Tint12::Blue);
    let r0 = derive_views(&b2, "甲");
    b2.set_custom_icon("甲", true);
    let r1 = derive_views(&b2, "甲");
    cs.add(
        "arbitration yields to icon",
        r0.map(|r| r.icon_bar_px == 12 && r.dot_px == 6 && r.tree_bar_px == 12) == Some(true)
            && r1.map(|r| r.icon_bar_px == 6 && r.dot_px == 3 && r.tree_bar_px == 6) == Some(true),
        "",
    );

    // 3) 过滤叠加：簿册 3 笔（红/黄/绿各 1）——点红只看红的（1 笔）→
    //    叠绿成并集（2 笔）→ 再点红退出只剩绿（1 笔）。
    let mut f = TintFilter::new();
    f.toggle(Tint12::Red);
    let reds_only = f.apply(&b).len() == 1;
    f.toggle(Tint12::Green);
    let union = f.apply(&b).len() == 2;
    f.toggle(Tint12::Red);
    let green_only = f.apply(&b).len() == 1;
    cs.add(
        "filter overlay union and toggle off",
        reds_only && union && green_only,
        "",
    );

    // 4) 过滤裁决语义：有激活色时命中放行、未激活色拦下。
    let mut f2 = TintFilter::new();
    f2.toggle(Tint12::Red);
    cs.add(
        "filter admits active blocks idle",
        f2.admits(Tint12::Red) && !f2.admits(Tint12::Teal) && f2.any_active(),
        "",
    );

    // 5) 清除归零：恢复全过态（无激活色 = 什么色都放行）。
    let admits_while = f.admits(Tint12::Green);
    f.clear();
    cs.add(
        "filter clear admits all",
        !f.any_active() && f.admits(Tint12::Teal) && admits_while,
        "",
    );

    // 6) 持久化对账①：十二枚序号往返无损；越界 12/255 拒绝。
    cs.add("tint index roundtrip ok", index_roundtrip_ok(), "");

    // 7) 持久化对账②：三标记 + 一未标记 → 存档还原 → 逐路径令牌一致。
    cs.add(
        "reload tokens match",
        reload_tokens_match(&b, &["项目A", "账单", "个人", "未标记"]),
        "",
    );

    // 8) 改色幂等：重复标记 = 改色（旧色过滤即空、令牌跟着换）。
    b.mark("账单", Tint12::Amber);
    cs.add(
        "re-mark recolors token",
        b.render_token("账单") == Some("tint-amber") && b.filter_by(Tint12::Yellow).is_empty(),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_same_tint_twice_deactivates() {
        let mut f = TintFilter::new();
        f.toggle(Tint12::Red);
        assert!(f.active(Tint12::Red));
        f.toggle(Tint12::Red);
        assert!(!f.active(Tint12::Red));
        assert!(!f.any_active());
    }

    #[test]
    fn derive_views_none_for_unmarked() {
        let b = TintBook::new();
        assert!(derive_views(&b, "无").is_none());
    }

    #[test]
    fn from_index_out_of_range_none() {
        assert!(Tint12::from_index(12).is_none());
        assert_eq!(Tint12::from_index(11), Some(Tint12::Slate));
    }
}
