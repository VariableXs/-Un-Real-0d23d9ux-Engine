//! 深化层四 · F145 教育/作品集友好（2026-09-27 深化批次四 · g 层）。
//!
//! 发布流水线状态机（草稿→审→脱敏→发布，守卫齐全）、课程先修链、
//! 读者反馈聚合（推荐线）、翻译适配清单、引用完整性（文中引用⊆清单）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 发布流水线：Draft→Review→Sanitized→Published；跳步拒绝 + 审阅留痕
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PubStage {
    Draft,
    Review,
    Sanitized,
    Published,
}

pub struct PublishFlow {
    pub slug: &'static str,
    pub stage: PubStage,
    pub reviewer: Option<&'static str>,
}

impl PublishFlow {
    pub fn advance(&mut self, to: PubStage) -> Result<(), &'static str> {
        let legal = match (self.stage, to) {
            (PubStage::Draft, PubStage::Review) => true,
            (PubStage::Review, PubStage::Sanitized) => {
                // 审阅人必须留痕（谁审的——可溯性）。
                if self.reviewer.is_none() {
                    return Err("无审阅人留痕：不得进入脱敏段");
                }
                true
            }
            (PubStage::Sanitized, PubStage::Published) => true,
            _ => false,
        };
        if !legal {
            return Err("非法发布迁移");
        }
        self.stage = to;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 课程先修链：课程序登记，先修只指向更早课程（f136g 同构审计）
// ---------------------------------------------------------------------------

pub struct Course {
    pub title: &'static str,
    pub prereq: usize,
}

pub fn prereq_violations(courses: &[Course]) -> alloc::vec::Vec<&'static str> {
    courses
        .iter()
        .enumerate()
        .filter(|(i, c)| c.prereq >= i + 1)
        .map(|(_, c)| c.title)
        .collect()
}

// ---------------------------------------------------------------------------
// 读者反馈聚合：评分 1-5 → 均值千分比；≥3500 进推荐列
// ---------------------------------------------------------------------------

pub fn rating_per_mille(scores: &[u32]) -> u32 {
    let valid: alloc::vec::Vec<u32> = scores.iter().copied().filter(|s| (1..=5).contains(s)).collect();
    if valid.is_empty() {
        return 0;
    }
    valid.iter().sum::<u32>() * 1000 / (valid.len() as u32 * 5)
}

pub fn recommended(scores: &[u32]) -> bool {
    rating_per_mille(scores) >= 800 && scores.len() >= 5
}

// ---------------------------------------------------------------------------
// 翻译适配清单：文章 × 语言 → 已译/待译分列
// ---------------------------------------------------------------------------

pub fn translation_todo(
    done: &[(&'static str, &'static str)],
    articles: &[&'static str],
    langs: &[&'static str],
) -> alloc::vec::Vec<(&'static str, &'static str)> {
    let mut todo: alloc::vec::Vec<(&'static str, &'static str)> = alloc::vec::Vec::new();
    for a in articles {
        for l in langs {
            if !done.iter().any(|(da, dl)| *da == *a && *dl == *l) {
                todo.push((a, l));
            }
        }
    }
    todo
}

// ---------------------------------------------------------------------------
// 引用完整性：文中引用（r1..rn）⊆ 文末清单；未使用引用点名
// ---------------------------------------------------------------------------

pub fn ref_integrity(
    cited: &[u32],
    listed: &[u32],
) -> (alloc::vec::Vec<u32>, alloc::vec::Vec<u32>) {
    let dangling: alloc::vec::Vec<u32> = cited.iter().filter(|c| !listed.contains(c)).copied().collect();
    let unused: alloc::vec::Vec<u32> = listed.iter().filter(|l| !cited.contains(l)).copied().collect();
    (dangling, unused)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F145G_TAG: &str = "stareco-F145-deep4";

pub fn run_f145_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F145G_TAG);

    // 发布流水线
    let mut f = PublishFlow { slug: "usb-boot-101", stage: PubStage::Draft, reviewer: None };
    set.add("f145g skip", f.advance(PubStage::Sanitized).is_err(), "跳步拒绝");
    let _ = f.advance(PubStage::Review);
    set.add("f145g no reviewer", f.advance(PubStage::Sanitized).is_err(), "无审阅人拦截");
    f.reviewer = Some("ai-k2");
    let _ = f.advance(PubStage::Sanitized);
    let _ = f.advance(PubStage::Published);
    set.add("f145g published", f.stage == PubStage::Published, "全链走通");

    // 先修链
    let courses = [
        Course { title: "装机", prereq: 0 },
        Course { title: "分区", prereq: 0 },
        Course { title: "引导", prereq: 1 },
    ];
    set.add("f145g prereq ok", prereq_violations(&courses).is_empty(), "链方向合法");
    let bad = [Course { title: "a", prereq: 1 }];
    set.add("f145g prereq bad", prereq_violations(&bad) == alloc::vec!["a"], "前向引用点名");

    // 读者反馈
    set.add(
        "f145g rating",
        rating_per_mille(&[5, 4, 4, 4, 5]) == 880,
        "22/25 → 880‰",
    );
    set.add("f145g recommend", recommended(&[5, 4, 4, 4, 5]) && !recommended(&[5, 4]), "推荐线+样本量");
    set.add("f145g dirty data", rating_per_mille(&[0, 9, 5, 5, 5, 5, 5]) == 1000, "越界剔除全满分");

    // 翻译适配
    let done = [("usb-boot-101", "zh")];
    let todo = translation_todo(&done, &["usb-boot-101", "tokens-201"], &["zh", "en"]);
    set.add(
        "f145g todo",
        todo == alloc::vec![("usb-boot-101", "en"), ("tokens-201", "zh"), ("tokens-201", "en")],
        "待译笛卡尔积点名",
    );

    // 引用完整性
    let (dang, unused) = ref_integrity(&[1, 3], &[1, 2]);
    set.add(
        "f145g refs",
        dang == alloc::vec![3] && unused == alloc::vec![2],
        "悬空/未用双向点名",
    );
    set.add(
        "f145g refs clean",
        ref_integrity(&[1, 2], &[1, 2]) == (alloc::vec![], alloc::vec![]),
        "干净引用",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn publish_cannot_go_back() {
        let mut f = PublishFlow { slug: "x", stage: PubStage::Published, reviewer: Some("r") };
        assert!(f.advance(PubStage::Draft).is_err());
    }

    #[test]
    fn rating_empty_and_bounds() {
        assert_eq!(rating_per_mille(&[]), 0);
        assert_eq!(rating_per_mille(&[5, 5, 5, 5, 5]), 1000);
        assert_eq!(rating_per_mille(&[1, 1, 1, 1, 1]), 200);
    }
}
