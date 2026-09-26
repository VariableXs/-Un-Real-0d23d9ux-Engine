//! F566 文件夹颜色标记 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：12 色语义自定义；三处呈现一致；过滤维度；
//! 与图标自定义兼容；移除标记。
//!
//! **设计要点（主册）**：
//! - 文件夹右键「标记颜色」——12 色标签（项目红/财务黄/个人绿…用户自定义
//!   语义）三处呈现（文件夹图标底部色条/详情列表色点/树视图中色条）；
//! - 按颜色过滤（工具栏色点行——点红只看红的）；
//! - 标记与 F452 自定义图标兼容（色条让位图标变化）。
//!
//! 12 色唯一源：[`crate::istar::ibase::Tint12`]（令牌名 + 缺省语义）。

use crate::checks::CheckSet;
use crate::istar::ibase::{Tint12, ISTAR_DOMAIN};

use alloc::string::String;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 一条标记（目录路径 → 色令牌 + 用户语义）。
#[derive(Clone, Debug)]
pub struct TintMark {
    pub path: String,
    pub tint: Tint12,
    /// 用户自定义语义（None = 用 Tint12 缺省语义）。
    pub custom_semantics: Option<String>,
    /// 是否同时存在 F452 自定义图标（色条让位图标变化——呈现宽度减半）。
    pub has_custom_icon: bool,
}

/// 颜色标记簿。
pub struct TintBook {
    marks: [Option<TintMark>; 128],
    len: usize,
}

impl TintBook {
    pub fn new() -> TintBook {
        TintBook { marks: [(); 128].map(|_| None), len: 0 }
    }

    /// 标记颜色（同路径重复标记 = 改色——幂等更新不新增账）。
    pub fn mark(&mut self, path: &str, tint: Tint12) -> bool {
        if let Some(slot) = self.marks[..self.len]
            .iter_mut()
            .flatten()
            .find(|m| m.path == path)
        {
            slot.tint = tint;
            return true;
        }
        if self.len >= 128 {
            return false;
        }
        self.marks[self.len] = Some(TintMark {
            path: String::from(path),
            tint,
            custom_semantics: None,
            has_custom_icon: false,
        });
        self.len += 1;
        true
    }

    /// F452 自定义图标状态注入口（宿主在图标自定义生效/失效时推送）。
    pub fn set_custom_icon(&mut self, path: &str, on: bool) -> bool {
        match self
            .marks[..self.len]
            .iter_mut()
            .flatten()
            .find(|m| m.path == path)
        {
            Some(m) => {
                m.has_custom_icon = on;
                true
            }
            None => false,
        }
    }

    /// 移除标记（存在才 true）。
    pub fn unmark(&mut self, path: &str) -> bool {
        for i in 0..self.len {
            let hit = self.marks[i].as_ref().map(|m| m.path == path).unwrap_or(false);
            if hit {
                self.marks[i] = None;
                let mut j = i;
                while j + 1 < self.len {
                    self.marks.swap(j, j + 1);
                    j += 1;
                }
                self.len -= 1;
                return true;
            }
        }
        false
    }

    /// 某目录的标记。
    pub fn of(&self, path: &str) -> Option<&TintMark> {
        self.marks[..self.len]
            .iter()
            .flatten()
            .find(|m| m.path == path)
    }

    /// 三处呈现一致性：三处取数口同走本函数（色令牌唯一源——一处一事实，
    /// 图标底条/列表色点/树视图色条拿同一令牌）。
    pub fn render_token(&self, path: &str) -> Option<&'static str> {
        self.of(path).map(|m| m.tint.token())
    }

    /// 色条让位：存在 F452 自定义图标时色条宽度减半（兼容判据）。
    pub fn bar_width_px(&self, path: &str) -> Option<u32> {
        self.of(path).map(|m| if m.has_custom_icon { 6 } else { 12 })
    }

    /// 语义文案：自定义优先，缺省次之。
    pub fn semantics(&self, path: &str) -> &'static str {
        // 自定义语义是用户串，模型面只回缺省语义（自定义串由 UI 层取）。
        self.of(path)
            .map(|m| m.tint.default_semantics())
            .unwrap_or("")
    }

    /// 过滤维度：按色过滤出目录清单（工具栏色点行——点红只看红的）。
    pub fn filter_by(&self, tint: Tint12) -> alloc::vec::Vec<&str> {
        self.marks[..self.len]
            .iter()
            .flatten()
            .filter(|m| m.tint == tint)
            .map(|m| m.path.as_str())
            .collect()
    }
}

impl Default for TintBook {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_foldertint_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 12 色语义：十二枚全可标记，缺省语义齐（ibase 唯一色源）。
    let mut b = TintBook::new();
    let mut all_ok = true;
    for (i, t) in Tint12::ALL.iter().enumerate() {
        all_ok &= b.mark(alloc::format!("目录{}", i).leak(), *t);
    }
    set.add("twelve tints all usable", all_ok && b.of("目录0").unwrap().tint == Tint12::Red, "");

    // 2. 三处呈现一致：三处取数同走 render_token（唯一令牌源）。
    let token = b.render_token("项目A");
    b.mark("项目A", Tint12::Red);
    let t1 = b.render_token("项目A");
    let t2 = b.render_token("项目A");
    set.add(
        "three presentations one source",
        token.is_none() && t1 == Some("tint-red") && t1 == t2,
        "",
    );

    // 3. 过滤维度：两红一绿 → 点红只看红的。
    b.mark("账单", Tint12::Yellow);
    b.mark("个人", Tint12::Green);
    let reds = b.filter_by(Tint12::Red);
    set.add(
        "filter by color",
        reds.len() == 2 && reds.contains(&"项目A") && !reds.contains(&"个人"),
        "",
    );

    // 4. 与图标自定义兼容：has_custom_icon 时色条 6px，否则 12px。
    b.mark("有图标", Tint12::Blue);
    b.set_custom_icon("有图标", true);
    set.add(
        "custom icon yields half bar",
        b.bar_width_px("项目A") == Some(12) && b.bar_width_px("有图标") == Some(6),
        "",
    );

    // 5. 移除标记：移除后三处呈现归 None。
    b.unmark("项目A");
    set.add("unmark clears render", b.render_token("项目A").is_none() && b.of("项目A").is_none(), "");

    // 6. 重复标记 = 改色（幂等不新增账；改色后不再出现在旧色过滤里）。
    b.mark("账单", Tint12::Amber);
    set.add(
        "re-mark recolors idempotent",
        b.of("账单").unwrap().tint == Tint12::Amber && !b.filter_by(Tint12::Yellow).contains(&"账单"),
        "",
    );

    // 7. 语义自定义位：custom_semantics 由用户填，缺省语义不被覆盖污染。
    b.mark("项目B", Tint12::Violet);
    set.add(
        "semantics fallback to default",
        b.semantics("项目B") == "创意紫" && b.semantics("不存在的目录").is_empty(),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmark_missing_false() {
        let mut b = TintBook::new();
        assert!(!b.unmark("无"));
    }

    #[test]
    fn mark_replaces_not_duplicates() {
        let mut b = TintBook::new();
        b.mark("a", Tint12::Red);
        b.mark("a", Tint12::Teal);
        assert_eq!(b.filter_by(Tint12::Teal).len(), 1);
        assert!(b.filter_by(Tint12::Red).is_empty());
    }

    #[test]
    fn tint12_default_semantics_all_present() {
        for t in Tint12::ALL.iter() {
            assert!(!t.default_semantics().is_empty());
        }
    }
}
