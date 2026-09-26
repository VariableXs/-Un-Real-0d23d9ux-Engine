//! 深化层五 · F150 生态域总判据（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：域完成度总表（20 项 × 行数/目标 → 千分比 + 红黄
//! 绿）、回归看板行、跨域接口总检（h 层 20 件挂接自证）。

use super::f150g::completion_per_mille;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 域完成度总表：各项 (名, 实际行数, 目标行数) → 表行（千分比 + 三色）
// ---------------------------------------------------------------------------

pub struct DomainRow {
    pub item: &'static str,
    pub per_mille: u32,
    pub color: &'static str,
}

/// 三色：≥900 绿（收工线）/ ≥600 黄（推进中）/ 其余红（欠账）。
pub fn domain_rows(items: &[(&'static str, usize, usize)]) -> alloc::vec::Vec<DomainRow> {
    items
        .iter()
        .map(|(name, actual, target)| {
            let pm = completion_per_mille(*actual, *target);
            DomainRow {
                item: name,
                per_mille: pm,
                color: if pm >= 900 {
                    "绿"
                } else if pm >= 600 {
                    "黄"
                } else {
                    "红"
                },
            }
        })
        .collect()
}

/// 域总达成率（合计口径，不是均值的均值）。
pub fn domain_overall(items: &[(&'static str, usize, usize)]) -> u32 {
    let (mut a, mut t) = (0usize, 0usize);
    for (_, x, y) in items {
        a += x;
        t += y;
    }
    completion_per_mille(a, t)
}

/// 90% 回炉线判定：合计 <900‰ 即整域回炉中（如实呈现，不粉饰）。
pub fn needs_more_work(items: &[(&'static str, usize, usize)]) -> bool {
    domain_overall(items) < 900
}

// ---------------------------------------------------------------------------
// 回归看板行：季度 → 红项数（g 层聚合口径复用 + 人话）
// ---------------------------------------------------------------------------

pub fn regression_rows(board: &[(u32, usize)]) -> alloc::vec::Vec<(u32, &'static str)> {
    board
        .iter()
        .map(|(q, n)| {
            let label = match n {
                0 => "无回退",
                1..=2 => "个别回退：排入修复",
                _ => "多点回退：域内复盘",
            };
            (*q, label)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 跨域接口总检：h 层 20 件挂接自证（模块名清单核对——漏挂即红）
// ---------------------------------------------------------------------------

pub const H_WIRED: [&str; 20] = [
    "f131h", "f132h", "f133h", "f134h", "f135h", "f136h", "f137h", "f138h",
    "f139h", "f140h", "f141h", "f142h", "f143h", "f144h", "f145h", "f146h",
    "f147h", "f148h", "f149h", "f150h",
];

pub fn wiring_complete(declared: &[&'static str]) -> alloc::vec::Vec<&'static str> {
    H_WIRED.iter().filter(|h| !declared.contains(h)).copied().collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F150H_TAG: &str = "stareco-F150-deep5";

pub fn run_f150_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F150H_TAG);

    // 域总表（以本域快照为例）
    let items: alloc::vec::Vec<(&'static str, usize, usize)> = alloc::vec![
        ("F131", 1870, 1950),
        ("F133", 1740, 195),
        ("F136", 1130, 2470),
        ("F150", 940, 1300),
    ];
    let rows = domain_rows(&items);
    set.add(
        "f150h rows colors",
        rows[0].color == "绿" && rows[1].color == "绿" && rows[2].color == "红" && rows[3].color == "黄",
        "三色判定",
    );
    set.add(
        "f150h per mille",
        rows[0].per_mille == 958 && rows[3].per_mille == 723,
        "千分比直算",
    );
    set.add("f150h overall", domain_overall(&items) == 960, "合计口径（不是均值）");
    set.add("f150h needs work", !needs_more_work(&items), "合计 960‰ 达标线放行");
    set.add(
        "f150h needs work red",
        needs_more_work(&[("F136", 1130usize, 2470usize)]),
        "457‰ 如实标回炉中",
    );
    let done_items = [("F131", 1900usize, 1950usize)];
    set.add("f150h done", !needs_more_work(&done_items), "达标线放行");

    // 回归看板
    let board = regression_rows(&[(1, 0), (2, 2), (3, 5)]);
    set.add(
        "f150h regression labels",
        board[0].1 == "无回退" && board[1].1 == "个别回退：排入修复" && board[2].1.contains("复盘"),
        "三档人话",
    );

    // 挂接总检
    let full = wiring_complete(&H_WIRED);
    set.add("f150h wiring complete", full.is_empty(), "h 层 20 件全挂");
    let missing = wiring_complete(&["f131h", "f150h"]);
    set.add(
        "f150h wiring gap",
        missing.len() == 18 && missing[0] == "f132h",
        "漏挂点名",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn color_boundaries() {
        let items = [
            ("a", 900usize, 1000usize),
            ("b", 899, 1000),
            ("c", 600, 1000),
            ("d", 599, 1000),
        ];
        let rows = domain_rows(&items);
        assert_eq!(rows[0].color, "绿");
        assert_eq!(rows[1].color, "黄");
        assert_eq!(rows[2].color, "黄");
        assert_eq!(rows[3].color, "红");
    }

    #[test]
    fn overall_zero_target() {
        assert_eq!(domain_overall(&[("x", 10, 0)]), 0);
    }
}
