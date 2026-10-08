//! 深化层五 · F132 差异表公开（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：差异表 → F036 星卡自动草稿的数据馈送、迁移
//! 横幅三要素文案生成、消费方通知行的确定性渲染。

use super::f132f::{DiffRow, DiffStatus, Impact3};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 星卡馈送接口：差异表 Open 行 → 星卡草稿候选（致命优先，追踪编号序）
// ---------------------------------------------------------------------------

pub struct StarCardCandidate {
    pub track_id: u32,
    pub api_name: &'static str,
    pub severity_label: &'static str,
}

/// 装配：Open 行按（致命>降级>无感, track_id 升序）取前 n 条。
/// api_names 与 rows 同序（登记纪律，f132f 口径复用）。
pub fn star_card_feed(
    rows: &[DiffRow],
    api_names: &[&'static str],
    n: usize,
) -> alloc::vec::Vec<StarCardCandidate> {
    let mut ranked: alloc::vec::Vec<(u8, u32, usize)> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.status == DiffStatus::Open)
        .map(|(i, r)| {
            let sev = match r.impact {
                Impact3::Lethal => 0,
                Impact3::Degraded => 1,
                Impact3::None => 2,
            };
            (sev, r.track_id, i)
        })
        .collect();
    for i in 1..ranked.len() {
        let k = ranked[i];
        let mut j = i;
        while j > 0 && ranked[j - 1] > k {
            ranked[j] = ranked[j - 1];
            j -= 1;
        }
        ranked[j] = k;
    }
    ranked
        .into_iter()
        .take(n)
        .map(|(sev, tid, i)| StarCardCandidate {
            track_id: tid,
            api_name: api_names.get(i).copied().unwrap_or(""),
            severity_label: match sev {
                0 => "致命",
                1 => "降级",
                _ => "无感",
            },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 迁移横幅：三要素文案（发生了什么/为什么/怎么办）——机器拼装
// ---------------------------------------------------------------------------

/// 输入：API 名 + 影响级 → 三行文案。无感级不横幅（返回空）。
pub fn migration_banner(api: &'static str, impact: Impact3) -> [alloc::string::String; 3] {
    let what = alloc::format!("API「{api}」的调用方式已变更");
    let why = match impact {
        Impact3::Lethal => alloc::string::String::from("旧签名将被移除，不迁移会无法编译"),
        Impact3::Degraded => alloc::string::String::from("旧签名暂时可用，但体验已降级"),
        Impact3::None => alloc::string::String::from("仅文档措辞修订，行为无变化"),
    };
    let how = match impact {
        Impact3::Lethal => alloc::format!("请在迁移窗内改用新签名（见差异表对应条目）"),
        Impact3::Degraded => alloc::format!("建议近期迁移；对照表已在开发者文档站更新"),
        Impact3::None => alloc::string::String::from("无需操作"),
    };
    [what, why, how]
}

/// 横幅展示门：无感级不出横幅（不骚扰纪律）。
pub fn banner_shown(impact: Impact3) -> bool {
    impact != Impact3::None
}

// ---------------------------------------------------------------------------
// 消费方通知行：受影响消费方清单 → 通知行（含迁移映射目标）
// ---------------------------------------------------------------------------

pub fn consumer_notice_rows(
    consumers: &[&'static str],
    old_api: &'static str,
    new_api: &'static str,
) -> alloc::vec::Vec<(&'static str, &'static str)> {
    consumers
        .iter()
        .map(|c| (*c, alloc::format!("{old_api} → {new_api}").leak() as &'static str))
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F132H_TAG: &str = "stareco-F132-deep5";

pub fn run_f132_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F132H_TAG);

    let rows = [
        DiffRow { track_id: 5, family: 1, impact: Impact3::Degraded, status: DiffStatus::Open },
        DiffRow { track_id: 2, family: 1, impact: Impact3::Lethal, status: DiffStatus::Open },
        DiffRow { track_id: 9, family: 2, impact: Impact3::None, status: DiffStatus::Resolved },
        DiffRow { track_id: 7, family: 2, impact: Impact3::None, status: DiffStatus::Open },
    ];
    let names = ["api_d", "api_l", "api_r", "api_n"];

    // 星卡馈送
    let feed = star_card_feed(&rows, &names, 3);
    set.add(
        "f132h feed order",
        feed.iter().map(|c| c.track_id).collect::<alloc::vec::Vec<_>>() == alloc::vec![2, 5, 7],
        "致命优先+编号序",
    );
    set.add(
        "f132h feed labels",
        feed[0].severity_label == "致命" && feed[2].severity_label == "无感",
        "严重度标注",
    );
    set.add("f132h resolved excluded", !feed.iter().any(|c| c.track_id == 9), "已解决不进草稿");

    // 横幅
    let b = migration_banner("fs_open", Impact3::Lethal);
    set.add(
        "f132h banner lethal",
        b[0].contains("fs_open") && b[1].contains("无法编译") && b[2].contains("迁移窗"),
        "三要素齐",
    );
    set.add(
        "f132h banner none",
        !banner_shown(Impact3::None) && banner_shown(Impact3::Degraded),
        "无感不出横幅",
    );

    // 通知行
    let rows2 = consumer_notice_rows(&["files", "editor"], "old", "new2");
    set.add(
        "f132h notices",
        rows2.len() == 2 && rows2[0].1.contains("old → new2"),
        "映射通知行",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn feed_cap_respected() {
        let rows = [
            DiffRow { track_id: 1, family: 1, impact: Impact3::Lethal, status: DiffStatus::Open },
            DiffRow { track_id: 3, family: 1, impact: Impact3::Lethal, status: DiffStatus::Open },
        ];
        let names = ["a", "b"];
        assert_eq!(star_card_feed(&rows, &names, 1).len(), 1);
        assert_eq!(star_card_feed(&rows, &names, 9).len(), 2);
    }
}
