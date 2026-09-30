//! 深化层五 · F131 上游回馈通道（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：贡献者页数据行渲染（g 层档案 → 展示行）、
//! 与 F129 判例线的状态查询接口、上游不可达的诚实降级行。

use super::f131g::{ContributorProfile, TrustTier};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 贡献者页装配：档案 → 展示行（信任分级徽标位 + 战绩），名字序
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ContributorRow {
    pub name: &'static str,
    pub tier_label: &'static str,
    pub merged: u32,
    pub pre_review_exempt: bool,
}

pub fn assemble_contributor_rows(profiles: &[ContributorProfile]) -> alloc::vec::Vec<ContributorRow> {
    let mut rows: alloc::vec::Vec<ContributorRow> = profiles
        .iter()
        .map(|p| ContributorRow {
            name: p.name,
            tier_label: match p.tier() {
                TrustTier::Newcomer => "新人",
                TrustTier::Regular => "常客",
                TrustTier::Senior => "资深",
            },
            merged: p.merged,
            pre_review_exempt: p.pre_review_exempt(),
        })
        .collect();
    // 名字字节序（页面确定性）。
    for i in 1..rows.len() {
        let k = rows[i].name;
        let tmp = rows[i].clone();
        let mut j = i;
        while j > 0 && rows[j - 1].name > k {
            rows[j] = rows[j - 1].clone();
            j -= 1;
        }
        rows[j] = tmp;
    }
    rows
}

// ---------------------------------------------------------------------------
// 跨域接口：F129 判例线状态查询 → 贡献页「我的判例」区块
// ---------------------------------------------------------------------------

/// F129 侧输入：(判例名, 五态序号 0-4)。
pub struct PrecedentState {
    pub name: &'static str,
    pub state: u8,
}

pub struct PrecedentView {
    pub rows: alloc::vec::Vec<(&'static str, &'static str)>,
    pub degraded: bool,
}

const PRECEDENT_LABELS: [&str; 5] = ["已收录", "复核中", "待补证", "已驳回", "已归档"];

/// 装配：越界态如实标「状态未知」——不猜不静默。
pub fn assemble_precedents(states: Option<&[PrecedentState]>) -> PrecedentView {
    match states {
        None => PrecedentView { rows: alloc::vec![("判例线", "上游不可达：稍后重试")], degraded: true },
        Some(list) => PrecedentView {
            rows: list
                .iter()
                .map(|s| {
                    let label = PRECEDENT_LABELS
                        .get(s.state as usize)
                        .copied()
                        .unwrap_or("状态未知");
                    (s.name, label)
                })
                .collect(),
            degraded: false,
        },
    }
}

// ---------------------------------------------------------------------------
// 上游不可达降级面：看板/雷达区块统一降级行（区块级，不是整页白）
// ---------------------------------------------------------------------------

pub fn degraded_block_rows(blocks: &[&'static str]) -> alloc::vec::Vec<(&'static str, &'static str)> {
    blocks
        .iter()
        .map(|b| (*b, "数据源不可达：显示缓存快照时间戳"))
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F131H_TAG: &str = "stareco-F131-deep5";

pub fn run_f131_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F131H_TAG);

    // 贡献者装配
    let profiles = [
        ContributorProfile { name: "zeta", merged: 6, rejected: 1, first_day: 1 },
        ContributorProfile { name: "alpha", merged: 2, rejected: 0, first_day: 1 },
        ContributorProfile { name: "novice", merged: 0, rejected: 1, first_day: 1 },
    ];
    let rows = assemble_contributor_rows(&profiles);
    set.add(
        "f131h rows order",
        rows.iter().map(|r| r.name).collect::<alloc::vec::Vec<_>>() == alloc::vec!["alpha", "novice", "zeta"],
        "名字序装配",
    );
    set.add(
        "f131h tier labels",
        rows[0].tier_label == "常客" && rows[2].tier_label == "资深" && rows[1].tier_label == "新人",
        "分级徽标位",
    );
    set.add("f131h exempt mark", rows[2].pre_review_exempt && !rows[0].pre_review_exempt, "豁免标注");

    // 判例线接口
    let pv = assemble_precedents(Some(&[
        PrecedentState { name: "p1", state: 0 },
        PrecedentState { name: "p2", state: 9 },
    ]));
    set.add(
        "f131h precedents",
        pv.rows[0] == ("p1", "已收录") && pv.rows[1] == ("p2", "状态未知") && !pv.degraded,
        "越界态诚实标注",
    );
    let degraded = assemble_precedents(None);
    set.add(
        "f131h degraded",
        degraded.degraded && degraded.rows[0].1.contains("不可达"),
        "不可达降级行",
    );

    // 区块级降级
    let blocks = degraded_block_rows(&["看板", "雷达"]);
    set.add(
        "f131h block rows",
        blocks.len() == 2 && blocks[0].1.contains("缓存快照"),
        "区块级降级不整页白",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn row_merge_count_passthrough() {
        let rows = assemble_contributor_rows(&[ContributorProfile {
            name: "m",
            merged: 9,
            rejected: 0,
            first_day: 0,
        }]);
        assert_eq!(rows[0].merged, 9);
        assert_eq!(rows[0].tier_label, "资深");
    }
}
