//! 深化层五 · F133 第三方图标包规范（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：图标包 → 桌面图标源（F084 网格取图接口，缺图
//! 降级到回退图标）、预览网格数据装配、包信息页数据行。

use super::f133g::{bucket_path, classify, IconCategory};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 桌面图标源接口：桌面请求图标名 → (路径, 是否回退)
// ---------------------------------------------------------------------------

pub struct IconSource {
    /// 包内实际登记的图标名。
    available: alloc::vec::Vec<&'static str>,
}

pub const FALLBACK_ICON: &str = "sys-fallback";

impl IconSource {
    pub fn new(available: &[&'static str]) -> IconSource {
        IconSource { available: available.iter().copied().collect() }
    }

    /// 取图：命中 → (真实路径, false)；缺图 → (回退路径, true)。
    /// 回退是显性降级——桌面永不静默画豆腐块。
    pub fn resolve(&self, name: &str) -> ([u8; 64], bool) {
        if self.available.iter().any(|a| *a == name) {
            (bucket_path(name), false)
        } else {
            (bucket_path(FALLBACK_ICON), true)
        }
    }

    pub fn len(&self) -> usize {
        self.available.len()
    }
}

// ---------------------------------------------------------------------------
// 预览网格装配：包清单 → 网格行（分类分组 + 名字序），未分类沉底
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct PreviewCell {
    pub name: &'static str,
    pub category_label: &'static str,
}

pub fn assemble_preview(names: &[&'static str]) -> alloc::vec::Vec<PreviewCell> {
    let cat_rank = |n: &str| match classify(n) {
        IconCategory::Action => 0,
        IconCategory::Status => 1,
        IconCategory::Mime => 2,
        IconCategory::Emblem => 3,
        IconCategory::Unclassified => 4,
    };
    let mut cells: alloc::vec::Vec<PreviewCell> = names
        .iter()
        .map(|n| PreviewCell {
            name: n,
            category_label: match classify(n) {
                IconCategory::Action => "动作",
                IconCategory::Status => "状态",
                IconCategory::Mime => "文件类型",
                IconCategory::Emblem => "徽记",
                IconCategory::Unclassified => "未分类",
            },
        })
        .collect();
    for i in 1..cells.len() {
        let k = (cat_rank(cells[i].name), cells[i].name);
        let tmp = cells[i].clone();
        let mut j = i;
        while j > 0 && (cat_rank(cells[j - 1].name), cells[j - 1].name) > k {
            cells[j] = cells[j - 1].clone();
            j -= 1;
        }
        cells[j] = tmp;
    }
    cells
}

// ---------------------------------------------------------------------------
// 包信息页数据行：登记项 → 信息行（值缺失显性「未登记」）
// ---------------------------------------------------------------------------

pub fn info_rows(entries: &[(&'static str, Option<&'static str>)]) -> alloc::vec::Vec<(&'static str, &'static str)> {
    entries
        .iter()
        .map(|(k, v)| (*k, v.unwrap_or("未登记")))
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F133H_TAG: &str = "stareco-F133-deep5";

pub fn run_f133_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F133H_TAG);

    // 桌面图标源
    let src = IconSource::new(&["folder", "file"]);
    let (path, fallback) = src.resolve("folder");
    set.add("f133h hit", !fallback && &path[..6] == b"icons/", "命中真实路径");
    let (p2, fb2) = src.resolve("ghost");
    set.add(
        "f133h fallback",
        fb2 && &p2[9..21] == b"sys-fallback",
        "缺图显性回退",
    );
    set.add("f133h empty pack", IconSource::new(&[]).len() == 0, "空包零登记");

    // 预览网格
    let names = ["mime-pdf", "emblem-lock", "action-open", "mystery", "status-busy"];
    let grid = assemble_preview(&names);
    set.add(
        "f133h grid order",
        grid.iter().map(|c| c.name).collect::<alloc::vec::Vec<_>>()
            == alloc::vec!["action-open", "status-busy", "mime-pdf", "emblem-lock", "mystery"],
        "分类序+未分类沉底",
    );
    set.add(
        "f133h grid labels",
        grid[0].category_label == "动作" && grid[4].category_label == "未分类",
        "分类标注",
    );

    // 信息行
    let rows = info_rows(&[("作者", Some("someone")), ("版本", None)]);
    set.add(
        "f133h info",
        rows[0].1 == "someone" && rows[1].1 == "未登记",
        "缺失显性化",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn resolve_stable_for_same_name() {
        let src = IconSource::new(&["a"]);
        let (p1, f1) = src.resolve("a");
        let (p2, f2) = src.resolve("a");
        assert_eq!((p1, f1), (p2, f2));
        assert!(!f1);
    }
}
