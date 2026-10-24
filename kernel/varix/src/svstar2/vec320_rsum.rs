//! VE-F0720 · 栅格化组阶段总结与跨域影响
//!
//! 职责：栅格化组对全引擎的影响总结（域内知识沉淀）——
//! 栅格化接口被哪些域消费（VE-E 文字统一接口 F0707 / VE-D04 蒙版 /
//! VE-N UI 框架绘制 / VE-Y 工具链预览）、
//! 栅格化质量对"4K 高清界面"承诺的支撑关系、
//! 性能预算占用（栅格化在整帧预算中的份额目标 ≤30% 渲染路径）。
//! 总结入知识库（F0398 模式）。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

/// 消费域清单：栅格化接口的跨域消费方。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consumer {
    TextRender,  // VE-E 文字（F0707 统一接口）
    MaskLayer,   // VE-D04 蒙版
    UiFramework, // VE-N UI 框架绘制
    ToolPreview, // VE-Y 工具链预览
}

impl Consumer {
    pub fn all() -> Vec<Consumer> {
        vec![Consumer::TextRender, Consumer::MaskLayer, Consumer::UiFramework, Consumer::ToolPreview]
    }
    pub fn name(self) -> &'static str {
        match self {
            Consumer::TextRender => "VE-E 文字",
            Consumer::MaskLayer => "VE-D04 蒙版",
            Consumer::UiFramework => "VE-N UI 框架",
            Consumer::ToolPreview => "VE-Y 工具链预览",
        }
    }
}

/// 消费核验：每个消费方引用的接口必须在冻结清单内。
pub fn consumers_served(active: &[Consumer]) -> Result<(), String> {
    let all = Consumer::all();
    let missing: Vec<&str> = all
        .iter()
        .filter(|c| !active.contains(c))
        .map(|c| c.name())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("消费方未接入：{}", missing.join("、")))
    }
}

/// 4K 承诺支撑分析：栅格化质量档与 4K 高清界面的支撑关系。
/// 4K（3840×2160）下图标级路径必须用解析 AA（质量档最高）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResSupport {
    pub width: u32,
    pub height: u32,
    /// 支撑承诺要求的最低 AA 档（0=无 4=4x 8=8x 3=解析）。
    pub required_aa: u32,
}

/// 承诺核验：4K 分辨率 + 解析档 → 支撑成立。
pub fn supports_4k(res: &ResSupport, actual_aa: u32) -> bool {
    let is_4k = res.width >= 3840 && res.height >= 2160;
    if is_4k {
        actual_aa >= res.required_aa
    } else {
        true // 非 4K 分辨率不受承诺约束
    }
}

/// 性能预算份额：栅格化耗时 / 整帧渲染预算 ≤ 30%。
pub fn budget_share(raster_ms: u32, frame_budget_ms: u32) -> u32 {
    if frame_budget_ms == 0 {
        return 1000;
    }
    raster_ms * 1000 / frame_budget_ms
}

pub fn budget_ok(raster_ms: u32, frame_budget_ms: u32) -> bool {
    budget_share(raster_ms, frame_budget_ms) <= 300
}

/// 知识库入库（F0398 模式）：总结条目入库——标题 + 要点 + 关联条目号。
#[derive(Clone, Debug, PartialEq)]
pub struct KnowledgeItem {
    pub title: String,
    pub points: Vec<String>,
    /// 关联条目号（F 号后三位）。
    pub refs: Vec<u32>,
}

impl KnowledgeItem {
    /// 入库核验：标题非空、要点 >= 3、关联条目号在栅格化组范围（700~720）。
    pub fn valid(&self) -> bool {
        !self.title.is_empty()
            && self.points.len() >= 3
            && !self.refs.is_empty()
            && self.refs.iter().all(|&r| (700..=720).contains(&r))
    }
}

/// 阶段总结构造（栅格化组的标准总结）。
pub fn stage_summary() -> KnowledgeItem {
    KnowledgeItem {
        title: "栅格化组阶段总结".to_string(),
        points: vec![
            "三路由决策器统一（CPU/SC/解析）".to_string(),
            "AA gamma 纪律全域贯彻".to_string(),
            "瓦片粒度帧间缓存命中滚动场景".to_string(),
            "质量对拍 Skia 级量化闭环".to_string(),
        ],
        refs: vec![701, 703, 706, 708],
    }
}

pub fn run_vec320_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec320");
    // 族一：消费域清单
    let all = Consumer::all();
    s.add("四方消费齐备", all.len() == 4, "four");
    s.add("全部接入通过", consumers_served(&all).is_ok(), "served");
    let partial = vec![Consumer::TextRender, Consumer::UiFramework];
    match consumers_served(&partial) {
        Err(m) => s.add("未接入点名", m.contains("蒙版") && m.contains("预览"), "unserved"),
        Ok(()) => s.add("未接入点名", false, "expected err"),
    }
    s.add("消费方名称互异", {
        let n: Vec<&str> = all.iter().map(|c| c.name()).collect();
        n[0] != n[1] && n[2] != n[3]
    }, "names");
    // 族二：4K 承诺支撑
    let r4k = ResSupport { width: 3840, height: 2160, required_aa: 3 };
    s.add("4K+解析档支撑成立", supports_4k(&r4k, 3), "support");
    s.add("4K+低档支撑不成立", !supports_4k(&r4k, 1), "weaken");
    s.add("非 4K 不受约束", supports_4k(&ResSupport { width: 1920, height: 1080, required_aa: 3 }, 0), "non-4k");
    s.add("4K 判定阈值正确", {
        let r = ResSupport { width: 3839, height: 2160, required_aa: 3 };
        supports_4k(&r, 0) // 3839 非全 4K
    }, "threshold");
    // 族三：预算份额
    s.add("份额计算", budget_share(3, 10) == 300, "share");
    s.add("30% 线内通过", budget_ok(3, 10), "in-budget");
    s.add("超 30% 拒绝", !budget_ok(4, 10), "over-budget");
    s.add("零预算判超", !budget_ok(1, 0), "zero-budget");
    s.add("小份额余量大", budget_share(1, 10) == 100 && budget_ok(1, 10), "headroom");
    // 族四：知识库入库
    let item = stage_summary();
    s.add("总结条目合法", item.valid(), "valid");
    s.add("要点数达标", item.points.len() == 4, "points");
    s.add("关联条目号在组内", item.refs.iter().all(|&r| (700..=720).contains(&r)), "refs");
    let thin = KnowledgeItem { title: "x".to_string(), points: vec!["a".to_string()], refs: vec![701] };
    s.add("要点不足判无效", !thin.valid(), "thin");
    let out_of_range = KnowledgeItem {
        title: "x".to_string(),
        points: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        refs: vec![300],
    };
    s.add("组外引用判无效", !out_of_range.valid(), "out-refs");
    s.add("空标题判无效", !KnowledgeItem { title: String::new(), points: item.points.clone(), refs: item.refs.clone() }.valid(), "empty-title");
    // 族五：跨域影响闭环
    s.add("消费+承诺+预算+入库构成总结", {
        consumers_served(&all).is_ok() && supports_4k(&r4k, 3) && budget_ok(3, 10) && item.valid()
    }, "closed-loop");
    s
}
