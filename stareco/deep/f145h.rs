//! 深化层五 · F145 教育/作品集友好（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：课程链 → F119 帮助中心学习路径行（下一步推荐）、
//! 文章卡装配（分级/主题标注）、待译文章角标数据。

use super::f145g::Course;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 学习路径行：课程链 + 已完成集 → 下一步推荐（唯一路径，不选困难症）
// ---------------------------------------------------------------------------

pub struct PathRow {
    pub title: &'static str,
    pub state: &'static str, // 已完成 / 可开始 / 锁定
}

pub fn learning_path(courses: &[Course], completed: &[&'static str]) -> alloc::vec::Vec<PathRow> {
    courses
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let done = completed.contains(&c.title);
            let unlocked = courses[..i.min(courses.len())]
                .iter()
                .all(|p| completed.contains(&p.title));
            let state = if done {
                "已完成"
            } else if unlocked {
                "可开始"
            } else {
                "锁定"
            };
            PathRow { title: c.title, state }
        })
        .collect()
}

/// 下一步推荐：第一条「可开始」的课程（零推荐 = 全部完成或全锁定）。
pub fn next_step(rows: &[PathRow]) -> Option<&'static str> {
    rows.iter().find(|r| r.state == "可开始").map(|r| r.title)
}

// ---------------------------------------------------------------------------
// 文章卡装配：文章元数据 → 卡片（主题/分级/推荐位标注）
// ---------------------------------------------------------------------------

pub struct ArticleCard {
    pub slug: &'static str,
    pub theme_label: &'static str,
    pub level_label: &'static str,
    pub recommended: bool,
}

/// 主题 0-3 / 分级 0-2（f145g meta 口径同源）。
pub fn article_cards(
    articles: &[(&'static str, u8, u8)],
    recommended_set: &[&'static str],
) -> alloc::vec::Vec<ArticleCard> {
    const THEMES: [&str; 4] = ["装机与引导", "日常使用", "进阶玩法", "内核与安全"];
    const LEVELS: [&str; 3] = ["入门", "进阶", "深水区"];
    articles
        .iter()
        .map(|(slug, theme, level)| ArticleCard {
            slug,
            theme_label: THEMES.get(*theme as usize).copied().unwrap_or("未知主题"),
            level_label: LEVELS.get(*level as usize).copied().unwrap_or("未知分级"),
            recommended: recommended_set.contains(slug),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 待译角标：文章 × 语言矩阵 → 待译数角标（0 不显示——不骚扰）
// ---------------------------------------------------------------------------

pub fn translation_badge(
    article: &'static str,
    done: &[(&'static str, &'static str)],
    langs: &[&'static str],
) -> Option<u32> {
    let missing = langs
        .iter()
        .filter(|l| !done.iter().any(|(a, dl)| a == &article && dl == *l))
        .count() as u32;
    if missing == 0 {
        None
    } else {
        Some(missing)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F145H_TAG: &str = "stareco-F145-deep5";

pub fn run_f145_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F145H_TAG);

    let courses = [
        Course { title: "装机", prereq: 0 },
        Course { title: "分区", prereq: 1 },
        Course { title: "引导", prereq: 2 },
    ];

    // 学习路径
    let zero = learning_path(&courses, &[]);
    set.add(
        "f145h path zero",
        zero[0].state == "可开始" && zero[1].state == "锁定",
        "链首可开始后续锁定",
    );
    let one = learning_path(&courses, &["装机"]);
    set.add(
        "f145h path next",
        next_step(&one) == Some("分区"),
        "下一步唯一推荐",
    );
    let all = learning_path(&courses, &["装机", "分区", "引导"]);
    set.add("f145h path done", next_step(&all).is_none(), "全完成零推荐");
    set.add(
        "f145h states",
        all.iter().all(|r| r.state == "已完成"),
        "完成态",
    );

    // 文章卡
    let cards = article_cards(
        &[("usb-101", 0, 0), ("kernel-401", 3, 2)],
        &["usb-101"],
    );
    set.add(
        "f145h card labels",
        cards[0].theme_label == "装机与引导"
            && cards[1].theme_label == "内核与安全"
            && cards[1].level_label == "深水区",
        "主题/分级标注",
    );
    set.add(
        "f145h recommended",
        cards[0].recommended && !cards[1].recommended,
        "推荐位",
    );
    let oob = article_cards(&[("x", 9, 9)], &[]);
    set.add(
        "f145h card oob",
        oob[0].theme_label == "未知主题" && oob[0].level_label == "未知分级",
        "越界标注不猜",
    );

    // 待译角标
    let done = [("usb-101", "zh")];
    let langs = ["zh", "en", "ru"];
    set.add(
        "f145h badge",
        translation_badge("usb-101", &done, &langs) == Some(2)
            && translation_badge("ghost", &done, &langs) == Some(3),
        "待译计数",
    );
    set.add(
        "f145h badge zero hidden",
        translation_badge("usb-101", &done, &["zh"]).is_none(),
        "零待译不显示",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn single_course_chain() {
        let c = [Course { title: "唯一课", prereq: 0 }];
        let rows = learning_path(&c, &[]);
        assert_eq!(next_step(&rows), Some("唯一课"));
    }
}
