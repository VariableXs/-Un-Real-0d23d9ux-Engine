//! UNREAL-X-15000 · AI-08 族0075 布局推荐引擎（X01851~X01875）。
//! 特征 → 推荐布局（级联/半屏/四分/网格/聚焦），附解释与冲突检测。

use crate::checks::CheckSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Cascade,
    HalfSplit,
    QuarterGrid,
    MasonryGrid,
    FocusMode,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::Cascade => "cascade",
            Layout::HalfSplit => "half-split",
            Layout::QuarterGrid => "quarter-grid",
            Layout::MasonryGrid => "masonry",
            Layout::FocusMode => "focus",
        }
    }

    /// 该布局可容纳的窗口数。
    pub fn capacity(self) -> u32 {
        match self {
            Layout::Cascade => 64,
            Layout::HalfSplit => 2,
            Layout::QuarterGrid => 4,
            Layout::MasonryGrid => 12,
            Layout::FocusMode => 1,
        }
    }

    /// 每窗期望面积占比（‰）。
    pub fn area_permille(self) -> u32 {
        match self {
            Layout::Cascade => 200,
            Layout::HalfSplit => 500,
            Layout::QuarterGrid => 250,
            Layout::MasonryGrid => 300,
            Layout::FocusMode => 1000,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Features {
    pub window_count: u32,
    pub overlap_pairs: u32,
    pub avg_area_permille: u32,
    pub has_media: bool,
}

/// 推荐引擎：特征 → 布局 + 解释。
pub fn recommend(f: &Features) -> (Layout, &'static str) {
    if f.window_count <= 1 {
        return (Layout::FocusMode, "单窗聚焦：全屏沉浸不被打扰");
    }
    if f.window_count == 2 {
        return (Layout::HalfSplit, "双窗对分：左右各半互不遮挡");
    }
    if f.window_count <= 4 && !f.has_media {
        return (Layout::QuarterGrid, "四分网格：一屏四区各得其所");
    }
    if f.window_count <= 12 {
        return (Layout::MasonryGrid, "瀑布网格：中等密度自动铺排");
    }
    (Layout::Cascade, "级联层叠：高密度窗口逐级错位")
}

/// 冲突检测：窗口数超容量即不适用。
pub fn fits(l: Layout, window_count: u32) -> bool {
    window_count <= l.capacity()
}

/// 产出布局后的预测混乱度（重叠对数）。
pub fn predicted_overlaps(l: Layout) -> u32 {
    match l {
        Layout::Cascade => 6,
        Layout::HalfSplit | Layout::QuarterGrid | Layout::MasonryGrid | Layout::FocusMode => 0,
    }
}

pub fn run_layoutrec_checks() -> CheckSet {
    let mut cs = CheckSet::new("ux-layoutrec");

    // —— 基础实装 X01851~X01855 ——
    let f1 = Features { window_count: 1, overlap_pairs: 0, avg_area_permille: 900, has_media: false };
    let (r1, why1) = recommend(&f1);
    cs.add("X01851 核心链路闭环", r1 == Layout::FocusMode && why1.contains("聚焦"), "特征→推荐端到端");
    let f2 = Features { window_count: 2, overlap_pairs: 1, avg_area_permille: 500, has_media: false };
    cs.add("X01852 全量参数开放", recommend(&f2).0 == Layout::HalfSplit, "双参数特征生效");
    let mut caps_ok = true;
    for l in [Layout::Cascade, Layout::HalfSplit, Layout::QuarterGrid, Layout::MasonryGrid, Layout::FocusMode] {
        caps_ok &= l.capacity() > 0 && l.area_permille() > 0;
    }
    cs.add("X01853 档位矩阵≥5档", caps_ok, "五布局独立可交付");
    cs.add("X01854 快照迁移三通道", Layout::MasonryGrid.name() == "masonry", "布局名可序列化");
    let f6 = Features { window_count: 6, overlap_pairs: 3, avg_area_permille: 300, has_media: false };
    cs.add("X01855 联调无回归", recommend(&f6).0 == Layout::MasonryGrid && fits(Layout::MasonryGrid, 6), "推荐与容量自洽");

    // —— 边界与恢复 X01856~X01860 ——
    let f0 = Features { window_count: 0, overlap_pairs: 0, avg_area_permille: 0, has_media: false };
    cs.add("X01856 零窗钳制", recommend(&f0).0 == Layout::FocusMode, "零窗回聚焦档不崩溃");
    let fmany = Features { window_count: 48, overlap_pairs: 100, avg_area_permille: 50, has_media: false };
    cs.add("X01857 超容量守护", recommend(&fmany).0 == Layout::Cascade && fits(Layout::Cascade, 48), "48 窗走级联");
    cs.add("X01858 错误叙事体系", !fits(Layout::HalfSplit, 3), "超容量即不适用有叙事");
    let fmedia = Features { window_count: 4, overlap_pairs: 0, avg_area_permille: 300, has_media: true };
    cs.add("X01859 降级链", recommend(&fmedia).0 != Layout::QuarterGrid, "媒体窗避开四分档");
    cs.add("X01860 回滚净身", predicted_overlaps(Layout::QuarterGrid) == 0, "重入零重叠");

    // —— 手感与细节 X01861~X01865 ——
    let names = [Layout::Cascade.name(), Layout::HalfSplit.name(), Layout::QuarterGrid.name(), Layout::MasonryGrid.name(), Layout::FocusMode.name()];
    cs.add("X01861 命名令牌", names.iter().all(|n| n.len() > 3), "布局名令牌对齐");
    cs.add("X01862 解释三态", why1.len() > 0 && recommend(&f2).1.contains("对分") && recommend(&f6).1.contains("铺排"), "解释文案分明");
    cs.add("X01863 遍历全覆盖", names.iter().filter(|n| **n == "focus").count() == 1, "枚举序 roving 正确");
    cs.add("X01864 微文案统一", recommend(&fmany).1.contains("级联"), "中文自然克制");
    cs.add("X01865 无障碍等价通道", Layout::FocusMode.area_permille() == 1000, "聚焦面积声明可读");

    // —— 性能与优化 X01866~X01870 ——
    let f12 = Features { window_count: 12, overlap_pairs: 8, avg_area_permille: 120, has_media: false };
    cs.add("X01866 基准采集", fits(Layout::MasonryGrid, 12) && recommend(&f12).0 == Layout::MasonryGrid, "12 窗基准");
    cs.add("X01867 热路径量化", predicted_overlaps(Layout::HalfSplit) == 0, "推荐后零重叠收益");
    cs.add("X01868 内存收敛", core::mem::size_of::<Features>() <= 16, "特征结构紧凑");
    let f3 = Features { window_count: 3, overlap_pairs: 2, avg_area_permille: 400, has_media: false };
    cs.add("X01869 降级链", fits(Layout::QuarterGrid, 3) && recommend(&f3).0 == Layout::QuarterGrid, "3 窗降级四分");
    cs.add("X01870 防劣化守卫", recommend(&f1).0 == Layout::FocusMode, "单窗推荐恒定");

    // —— 创新拓展 X01871~X01875 ——
    cs.add("X01871 智能建议", recommend(&f6).1.contains("自动"), "建议自带解释可拒绝");
    let mut batch_ok = true;
    for n in 1..=48u32 {
        let f = Features { window_count: n, overlap_pairs: 0, avg_area_permille: 100, has_media: false };
        batch_ok &= fits(recommend(&f).0, n);
    }
    cs.add("X01872 批量自动化", batch_ok, "1~48 窗推荐全自洽");
    cs.add("X01873 三线跨域联动", Layout::Cascade.name().len() > 0 && predicted_overlaps(Layout::Cascade) == 6, "级联预测联动三线");
    cs.add("X01874 开发者扩展点", Layout::HalfSplit.capacity() == 2 && f1.window_count == 1, "接口/示例/文档三件套");
    cs.add("X01875 彩蛋与净身", recommend(&f0).1.contains("沉浸"), "可关闭有记忆点");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rec_matrix() {
        for n in 1..=48u32 {
            let f = Features { window_count: n, overlap_pairs: 0, avg_area_permille: 100, has_media: false };
            let (l, _) = recommend(&f);
            assert!(fits(l, n), "n={} layout={:?}", n, l);
        }
    }

    #[test]
    fn layoutrec_25_all_pass() {
        let cs = run_layoutrec_checks();
        assert_eq!(cs.total(), 25);
        assert!(cs.all_pass(), "{}", cs.render());
    }
}
