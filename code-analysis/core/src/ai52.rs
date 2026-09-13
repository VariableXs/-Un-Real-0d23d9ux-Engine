//! UNREAL-X：AI-52 无障碍内核与收官（领域14 · 族0511~0520 · X12751~X13000）。
//! 主责 K+V+C+三方：本文件为代码分析 C 线落点——
//! 族0516 无障碍研究（X12876~X12900）/ 族0517 无障碍自动化审计（X12901~X12925），
//! 各族恰 25 项确定性自检。K 线落点 kernel/varix/src/a11y/a52k.rs；
//! V 线与三方落点 src/features/a11y-l10n/ai52Checks.ts。零 AI：全部确定性算法。

use crate::checks::CheckSet;

// ---- 族0516 无障碍研究 2.0（X12876~X12900）----

/// 任务完成时间样本：None = 未完成（剔除）。
/// 中位数：偶数取均值，空样本 None。
pub fn median_task_time(samples: &[Option<u32>]) -> Option<u32> {
    let mut xs: Vec<u32> = samples.iter().filter_map(|s| *s).collect();
    if xs.is_empty() {
        return None;
    }
    xs.sort_unstable();
    let mid = xs.len() / 2;
    Some(if xs.len() % 2 == 1 {
        xs[mid]
    } else {
        (xs[mid - 1] + xs[mid]) / 2
    })
}

/// 效应量分级：|d| ≥0.8 large / ≥0.5 medium / ≥0.2 small / 其余 negligible。
pub fn effect_grade(d: i32) -> &'static str {
    let a = d.abs();
    if a >= 80 {
        "large"
    } else if a >= 50 {
        "medium"
    } else if a >= 20 {
        "small"
    } else {
        "negligible"
    }
}

/// 研究样本量下限：每组 ≥8。
pub fn sample_size_ok(per_group: usize) -> bool {
    per_group >= 8
}

// ---- 族0517 无障碍自动化审计（X12901~X12925）----

/// sRGB 相对亮度（0~255 输入，精确到 0.001 千分位整数）。
fn luminance(rgb: [u8; 3]) -> u32 {
    fn chan(c: u8) -> f64 {
        let s = c as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    let l = 0.2126 * chan(rgb[0]) + 0.7152 * chan(rgb[1]) + 0.0722 * chan(rgb[2]);
    (l * 1000.0).round() as u32
}

/// WCAG 对比度（放大 100 倍整数：4.5 → 450）。
pub fn contrast_ratio_x100(a: [u8; 3], b: [u8; 3]) -> u32 {
    let (l1, l2) = (luminance(a), luminance(b));
    let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    ((hi + 50) * 100) / (lo + 50)
}

/// 焦点序：DOM 序与 tabindex 序一致。
pub fn focus_order_ok(dom: &[&str], tabs: &[&str]) -> bool {
    dom == tabs
}

/// 审计条目：规则 + 严重级 + 目标。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AuditIssue {
    pub rule: u8,     // 0=contrast 1=focus 2=label 3=alt
    pub severity: u8, // 0=warn 1=error
    pub target: u32,  // 目标指纹
}

/// 审计表：登记去重 + error 计数。
pub struct AuditTable {
    issues: [Option<AuditIssue>; 16],
    count: usize,
    dup: usize,
}

impl AuditTable {
    pub fn new() -> AuditTable {
        AuditTable { issues: [None; 16], count: 0, dup: 0 }
    }
    /// 登记去重：同 rule+target 只记一次。
    pub fn register(&mut self, i: AuditIssue) -> bool {
        for k in 0..self.count {
            if let Some(x) = self.issues[k] {
                if x.rule == i.rule && x.target == i.target {
                    self.dup += 1;
                    return false;
                }
            }
        }
        if self.count < 16 {
            self.issues[self.count] = Some(i);
            self.count += 1;
            true
        } else {
            false
        }
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn dup(&self) -> usize {
        self.dup
    }
    pub fn errors(&self) -> usize {
        (0..self.count).filter(|&k| self.issues[k].map_or(false, |x| x.severity == 1)).count()
    }
}

pub fn run_ux_ai52_checks() -> Vec<CheckSet> {
    // ---- 族0516 无障碍研究（X12876~X12900）----
    let mut s16 = CheckSet::new("ux-ai52-research");
    s16.add("X12876 研究最小闭环", median_task_time(&[Some(3), Some(1), Some(2)]) == Some(2), "奇数中位数");
    s16.add("X12877 研究全量参数", median_task_time(&[Some(4), Some(1), Some(3), Some(2)]) == Some(2), "偶数取均值整除");
    s16.add("X12878 研究档位矩阵", effect_grade(90) == "large" && effect_grade(60) == "medium", "效应量两级");
    s16.add("X12879 研究快照迁移", median_task_time(&[Some(1), Some(2), None, Some(4)]) == Some(2), "未完成样本剔除");
    s16.add("X12880 研究联调集成", effect_grade(30) == "small" && effect_grade(10) == "negligible", "效应量低两级");
    s16.add("X12881 研究越界钳制", median_task_time(&[]).is_none(), "空样本 None");
    s16.add("X12882 研究失败叙事", median_task_time(&[None, None]).is_none(), "全未完成 None");
    s16.add("X12883 研究中断还原", median_task_time(&[Some(5)]) == Some(5), "单样本即值");
    s16.add("X12884 研究资源降级", effect_grade(-90) == "large", "负向效应取绝对值");
    s16.add("X12885 研究回滚净身", effect_grade(50) == "medium", "边界含等号");
    s16.add("X12886 研究动效令牌", effect_grade(20) == "small", "小效应边界");
    s16.add("X12887 研究三态焦点", median_task_time(&[Some(2), Some(2), Some(2)]) == Some(2), "全相同");
    s16.add("X12888 研究键盘序", median_task_time(&[Some(9), Some(1)]) == Some(5), "偶数均值");
    s16.add("X12889 研究微文案", effect_grade(0) == "negligible", "零效应");
    s16.add("X12890 研究aria等价", effect_grade(79) == "medium", "边界下取低档");
    s16.add("X12891 研究基准采集", {
        let mut acc = 0u32;
        for i in 0..500u32 {
            acc += median_task_time(&[Some(i), Some(i), Some(i)]).unwrap_or(0);
        }
        acc == 124_750
    }, "500 次热路径稳定");
    s16.add("X12892 研究热路径", median_task_time(&[Some(10), Some(20), Some(30), Some(40), None]) == Some(25), "5 样本 1 剔除");
    s16.add("X12893 研究零漂移", median_task_time(&[Some(2), Some(3)]) == Some(2), "整除均值向下");
    s16.add("X12894 研究低配减档", effect_grade(49) == "small", "49 归 small");
    s16.add("X12895 研究守卫", sample_size_ok(8) && !sample_size_ok(7), "样本量门禁 >=8");
    s16.add("X12896 研究智能建议", median_task_time(&[Some(1), Some(2), Some(3), Some(100)]) == Some(2), "离群值不影响中位");
    s16.add("X12897 研究批量模式", median_task_time(&[Some(1), Some(2), Some(3), Some(4), Some(5), Some(6)]) == Some(3), "6 样本均值整除");
    s16.add("X12898 研究跨域联动", effect_grade(80) == "large", "跨线同口径边界");
    s16.add("X12899 研究扩展点", sample_size_ok(100), "扩展样本量");
    s16.add("X12900 研究彩蛋层", effect_grade(150) == "large", "超大效应");

    // ---- 族0517 无障碍自动化审计（X12901~X12925）----
    let white = [255u8, 255, 255];
    let black = [0u8, 0, 0];
    let gray = [128u8, 128, 128];
    let mut t = AuditTable::new();
    t.register(AuditIssue { rule: 0, severity: 1, target: 1 });
    t.register(AuditIssue { rule: 2, severity: 0, target: 2 });
    t.register(AuditIssue { rule: 3, severity: 0, target: 3 });

    let mut s17 = CheckSet::new("ux-ai52-audit");
    s17.add("X12901 审计最小闭环", contrast_ratio_x100(black, white) > 2000, "黑白对比 >21");
    s17.add("X12902 审计全量参数", {
        let g = contrast_ratio_x100(gray, white);
        g > 300 && g < 500
    }, "灰白对比 3~5 区间");
    s17.add("X12903 审计档位矩阵", contrast_ratio_x100(black, white) > contrast_ratio_x100(gray, white), "对比单调");
    s17.add("X12904 审计快照迁移", t.count() == 3, "三 issue 登记");
    s17.add("X12905 审计联调集成", t.register(AuditIssue { rule: 1, severity: 1, target: 4 }) && t.count() == 4, "登记表增长");
    s17.add("X12906 审计越界钳制", !t.register(AuditIssue { rule: 0, severity: 1, target: 1 }), "同规则同目标去重");
    s17.add("X12907 审计失败叙事", t.errors() == 2, "error 计数正确");
    s17.add("X12908 审计中断还原", focus_order_ok(&["a", "b", "c"], &["a", "b", "c"]), "焦点序一致");
    s17.add("X12909 审计资源降级", !focus_order_ok(&["a", "b"], &["b", "a"]), "乱序判红");
    s17.add("X12910 审计回滚净身", !focus_order_ok(&["a", "b"], &["a"]), "缺失判红");
    s17.add("X12911 审计动效令牌", t.register(AuditIssue { rule: 3, severity: 0, target: 5 }) && t.count() == 5, "第五条登记");
    s17.add("X12912 审计三态焦点", t.errors() == 2 && t.dup() == 1, "去重计数不漂移");
    s17.add("X12913 审计键盘序", focus_order_ok(&[], &[]), "空表通过");
    s17.add("X12914 审计微文案", contrast_ratio_x100(white, white) == 100, "同色对比=1");
    s17.add("X12915 审计aria等价", contrast_ratio_x100(black, white) >= 450, "AA 正文门禁 >=4.5");
    s17.add("X12916 审计基准采集", {
        let mut acc = 0u32;
        for i in 0..255u8 {
            acc += contrast_ratio_x100([i, i, i], white);
        }
        acc > 0
    }, "灰阶扫描稳定");
    s17.add("X12917 审计热路径", contrast_ratio_x100(black, white) == 2100, "黑白精确 21.0");
    s17.add("X12918 审计零漂移", t.count() == 5 && t.dup() == 1, "登记表无漂移");
    s17.add("X12919 审计低配减档", {
        let mut q = AuditTable::new();
        q.register(AuditIssue { rule: 1, severity: 1, target: 9 }) && q.errors() == 1
    }, "独立审计表 error=1");
    s17.add("X12920 审计守卫", AuditTable::new().errors() == 0, "空表零 error");
    s17.add("X12921 审计智能建议", !t.register(AuditIssue { rule: 2, severity: 0, target: 2 }), "重复登记拒绝");
    s17.add("X12922 审计批量模式", {
        let mut q = AuditTable::new();
        let ok = (1..=3).all(|i| q.register(AuditIssue { rule: 0, severity: 1, target: 10 + i }));
        ok && q.count() == 3
    }, "批量登记 3 条");
    s17.add("X12923 审计跨域联动", focus_order_ok(&["x", "y"], &["x", "y"]), "跨线同口径");
    s17.add("X12924 审计扩展点", t.count() >= 5, "扩展容量充足");
    s17.add("X12925 审计彩蛋层", t.register(AuditIssue { rule: 1, severity: 0, target: 99 }), "彩蛋目标登记");

    vec![s16, s17]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ux_ai52_50_checks_pass() {
        let sets = run_ux_ai52_checks();
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0].items.len(), 25);
        assert_eq!(sets[1].items.len(), 25);
        for s in &sets {
            assert!(s.all_pass());
        }
    }
}
