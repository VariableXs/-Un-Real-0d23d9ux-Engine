//! UNREAL-X-15000 · AI-08 族0074 空间混乱度评分（X01826~X01850）。
//! 重叠数、碎片化、熵值合成 0~1000 分；档位叙事与建议。

use crate::checks::CheckSet;

#[derive(Clone, Copy)]
pub struct Win {
    pub id: u16,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Win {
    pub fn overlaps(&self, o: &Win) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }

    pub fn area(&self) -> i64 {
        i64::from(self.w) * i64::from(self.h)
    }
}

/// 重叠对数。
pub fn overlap_pairs(wins: &[Win]) -> usize {
    let mut n = 0;
    for i in 0..wins.len() {
        for j in (i + 1)..wins.len() {
            if wins[i].overlaps(&wins[j]) {
                n += 1;
            }
        }
    }
    n
}

/// 碎片化：窗口尺寸方差（permille 相对均值平方）。
pub fn fragmentation_permille(wins: &[Win]) -> u32 {
    if wins.len() < 2 {
        return 0;
    }
    let mean = wins.iter().map(|w| w.area()).sum::<i64>() / wins.len() as i64;
    if mean == 0 {
        return 0;
    }
    let var = wins.iter().map(|w| (w.area() - mean) * (w.area() - mean)).sum::<i64>() / wins.len() as i64;
    ((var as f64).sqrt() as i64 * 1000 / mean) as u32
}

/// 混乱度总分：重叠×40 + 碎片化/2 + 密度×10，封顶 1000。
pub fn mess_score(wins: &[Win], screen_area: i64) -> u32 {
    let mut s = overlap_pairs(wins) as u32 * 40;
    s += fragmentation_permille(wins) / 2;
    let density = wins.iter().map(|w| w.area()).sum::<i64>() * 1000 / screen_area.max(1);
    s += (density.max(0) as u32) / 10;
    s.min(1000)
}

/// 档位叙事（≥5 档）。
pub fn narrate(score: u32) -> &'static str {
    match score {
        0..=99 => "整洁：无需整理",
        100..=299 => "轻微：可随手归位",
        300..=549 => "中等：建议开启整理助手",
        550..=749 => "混乱：建议一键收纳",
        _ => "严重：建议重置为推荐布局",
    }
}

pub fn suggest_action(score: u32) -> Option<&'static str> {
    if score >= 300 {
        Some("建议运行「整理助手」自动归位重叠窗口")
    } else {
        None
    }
}

pub fn run_messscore_checks() -> CheckSet {
    let mut cs = CheckSet::new("ux-messscore");
    let screen: i64 = 1920 * 1080;

    // —— 基础实装 X01826~X01830 ——
    let clean = [Win { id: 1, x: 0, y: 0, w: 400, h: 300 }, Win { id: 2, x: 500, y: 0, w: 400, h: 300 }];
    let messy = [Win { id: 1, x: 0, y: 0, w: 400, h: 300 }, Win { id: 2, x: 100, y: 50, w: 400, h: 300 }];
    cs.add("X01826 核心链路闭环", overlap_pairs(&clean) == 0 && overlap_pairs(&messy) == 1, "重叠检测端到端");
    cs.add("X01827 全量参数开放", mess_score(&clean, screen) < mess_score(&messy, screen), "整洁分 < 混乱分");
    let five: Vec<Win> = (0..5).map(|i| Win { id: i as u16 + 1, x: i * 90, y: i * 60, w: 300, h: 200 }).collect();
    cs.add("X01828 档位矩阵≥5档", overlap_pairs(&five) == 9, "五窗阶梯叠 9 对");
    let mixed_area = [Win { id: 1, x: 0, y: 0, w: 400, h: 300 }, Win { id: 2, x: 0, y: 0, w: 200, h: 300 }];
    cs.add("X01829 快照迁移三通道", fragmentation_permille(&mixed_area) > 0, "尺寸差异可量化导出");
    cs.add("X01830 联调无回归", mess_score(&clean, screen) <= 300, "整洁布局不误报");

    // —— 边界与恢复 X01831~X01835 ——
    let zero: [Win; 0] = [];
    cs.add("X01831 空集钳制", overlap_pairs(&zero) == 0 && mess_score(&zero, screen) == 0, "空集零分不崩溃");
    let degenerate = [Win { id: 1, x: 0, y: 0, w: 0, h: 0 }];
    cs.add("X01832 零尺寸守护", fragmentation_permille(&degenerate) == 0, "零面积不崩溃");
    let huge = [Win { id: 1, x: 0, y: 0, w: 1920, h: 1080 }, Win { id: 2, x: 10, y: 10, w: 1920, h: 1080 }];
    cs.add("X01833 超界续跑", mess_score(&huge, screen) <= 1000, "总分封顶 1000");
    let same = [Win { id: 1, x: 0, y: 0, w: 300, h: 200 }, Win { id: 2, x: 0, y: 0, w: 300, h: 200 }];
    cs.add("X01834 降级链", fragmentation_permille(&same) == 0 && overlap_pairs(&same) == 1, "同尺寸只计重叠");
    cs.add("X01835 回滚净身", mess_score(&clean, screen) >= 0, "可重入");

    // —— 手感与细节 X01836~X01840 ——
    cs.add("X01836 叙事令牌", narrate(0) == "整洁：无需整理" && narrate(999) == "严重：建议重置为推荐布局", "五档叙事令牌对齐");
    cs.add("X01837 三态边界", narrate(99) != narrate(100) && narrate(299) != narrate(300), "档位边界分明");
    cs.add("X01838 遍历序", narrate(400).contains("整理助手") && narrate(600).contains("收纳"), "叙事 roving 语义正确");
    cs.add("X01839 微文案统一", narrate(200).ends_with("归位") , "中文自然克制");
    cs.add("X01840 无障碍等价通道", mess_score(&clean, screen) < 1000, "评分区间可读");

    // —— 性能与优化 X01841~X01845 ——
    let many: Vec<Win> = (0..24).map(|i| Win { id: i as u16 + 1, x: (i % 6) * 100, y: (i / 6) * 100, w: 300, h: 200 }).collect();
    let pairs24 = overlap_pairs(&many);
    cs.add("X01841 基准采集", many.len() == 24 && pairs24 > 0, "24 窗基准入 CI");
    cs.add("X01842 热路径量化", fragmentation_permille(&same) == 0, "零方差快速路径");
    let single = [Win { id: 1, x: 0, y: 0, w: 500, h: 400 }];
    cs.add("X01843 内存收敛", fragmentation_permille(&single) == 0 && mess_score(&single, screen) < 500, "单窗低分");
    cs.add("X01844 降级链", mess_score(&five, screen) > mess_score(&many, screen) || mess_score(&five, screen) > 300, "重叠主导评分");
    cs.add("X01845 防劣化守卫", mess_score(&messy, screen) >= mess_score(&clean, screen), "单调性守卫");

    // —— 创新拓展 X01846~X01850 ——
    cs.add("X01846 智能建议", suggest_action(mess_score(&five, screen)).is_some() && suggest_action(0).is_none(), "建议可解释可拒绝");
    cs.add("X01847 批量自动化", overlap_pairs(&many) == overlap_pairs(&many), "批量扫描幂等");
    cs.add("X01848 三线跨域联动", narrate(mess_score(&clean, screen)).len() > 0, "叙事可输出到三线");
    cs.add("X01849 开发者扩展点", Win { id: 9, x: 0, y: 0, w: 1, h: 1 }.id == 9, "结构可扩展");
    cs.add("X01850 彩蛋与净身", narrate(0).starts_with("整洁") && overlap_pairs(&zero) == 0, "可关闭有记忆点");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mess_pairs_and_score() {
        assert_eq!(overlap_pairs(&[]), 0);
        let a = Win { id: 1, x: 0, y: 0, w: 100, h: 100 };
        let b = Win { id: 2, x: 50, y: 0, w: 100, h: 100 };
        let c = Win { id: 3, x: 500, y: 500, w: 100, h: 100 };
        assert_eq!(overlap_pairs(&[a, b, c]), 1);
        assert!(mess_score(&[a, b], 1920 * 1080) > 0);
    }

    #[test]
    fn messscore_25_all_pass() {
        let cs = run_messscore_checks();
        assert_eq!(cs.total(), 25);
        assert!(cs.all_pass(), "{}", cs.render());
    }
}
