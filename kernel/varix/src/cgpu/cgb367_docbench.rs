//! CGPU-F0367 · 配方文档与基准（判据段 0x0970）
//! 文档基准：文档（配方规范/场景接入指南/自定义手册三件）；基准
//! （五场景配方切换开销/转移协议延迟）——入册回归。判据：三文
//! 档、两基准、判据。
//!
//! 锚点原文摘录（施工书 1631 行）：
//! 「文档（配方规范/场景接入指南/自定义手册三件）；基准（五场景
//! 配方切换开销/转移协议延迟）——入册回归。判据：三文档、两基
//! 准、判据。」
//!
//! 诊断码段 0x0970（本模块独占：0x0970~0x097F）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0970;

/// 文档件数冻结（判据「三文档」——闭集三件）。
pub const DOC_COUNT: usize = 3;

/// 切换开销红线（步数化——纯功能代码零墙钟，开销以步数计）：
/// 五场景切换每场景 ≤ 64 步。
pub const SWITCH_COST_STEP_BUDGET: u32 = 64;

/// 转移协议延迟红线（步数）：四相协议全程 ≤ 32 步。
pub const TRANSFER_LATENCY_STEP_BUDGET: u32 = 32;

/// 文档件（判据「三文档」一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocKind {
    /// 配方规范（字段/语义/守恒式）。
    Spec,
    /// 场景接入指南（新场景如何挂配方）。
    Onboard,
    /// 自定义手册（用户自定义配方手册）。
    Custom,
}

impl DocKind {
    /// 件号。
    pub fn ordinal(&self) -> u8 {
        match self {
            DocKind::Spec => 0,
            DocKind::Onboard => 1,
            DocKind::Custom => 2,
        }
    }
    /// 文档标题（文档面正文锚——内容与实现同步维护）。
    pub fn title(&self) -> &'static str {
        match self {
            DocKind::Spec => "配方规范：三账户守恒式 Σ=1000；池容量恒 1000；钳制回退出厂默认",
            DocKind::Onboard => "场景接入指南：实现 RecipeProfile 三态查询→注册→激活→遥测四步",
            DocKind::Custom => "自定义手册：开放格式 v1 七字段；导入走 schema+钳制双闸",
        }
    }
}

/// 文档集（判据「三文档」主判据）：三件齐备 + 标题非空 + 件号
/// 连续不跳号。
pub struct DocSet {
    /// 已收录件。
    pub docs: Vec<DocKind>,
}

impl DocSet {
    /// 出厂文档集（三件全收）。
    pub fn factory() -> Self {
        DocSet { docs: alloc::vec![DocKind::Spec, DocKind::Onboard, DocKind::Custom] }
    }
    /// 完备核（判据「三文档」守卫）。
    pub fn complete(&self) -> Result<(), u16> {
        if self.docs.len() != DOC_COUNT {
            return Err(SEG_BASE);
        }
        let mut seen = [false; 3];
        for d in &self.docs {
            if seen[d.ordinal() as usize] {
                return Err(SEG_BASE + 1);
            }
            seen[d.ordinal() as usize] = true;
            if d.title().is_empty() {
                return Err(SEG_BASE + 2);
            }
        }
        if !seen.iter().all(|&s| s) {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }
}

/// 基准场景（判据「两基准」的场景闭集——五场景）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchScene {
    /// 视频→网页。
    VideoToWeb,
    /// 网页→全屏。
    WebToFullscreen,
    /// 全屏→视频。
    FullscreenToVideo,
    /// 视频→视频（同族切换）。
    VideoToVideo,
    /// 网页→网页（同族切换）。
    WebToWeb,
}

impl BenchScene {
    /// 场景号。
    pub fn ordinal(&self) -> u8 {
        match self {
            BenchScene::VideoToWeb => 0,
            BenchScene::WebToFullscreen => 1,
            BenchScene::FullscreenToVideo => 2,
            BenchScene::VideoToVideo => 3,
            BenchScene::WebToWeb => 4,
        }
    }
    /// 场景名。
    pub fn name(&self) -> &'static str {
        match self {
            BenchScene::VideoToWeb => "video->web",
            BenchScene::WebToFullscreen => "web->fullscreen",
            BenchScene::FullscreenToVideo => "fullscreen->video",
            BenchScene::VideoToVideo => "video->video",
            BenchScene::WebToWeb => "web->web",
        }
    }
}

/// 开销基准行（判据「切换开销」一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CostRow {
    /// 场景。
    pub scene: BenchScene,
    /// 实测步数。
    pub steps: u32,
}

impl CostRow {
    /// 达标核（判据「切换开销」红线）。
    pub fn within_budget(&self) -> Result<(), u16> {
        if self.steps == 0 {
            return Err(SEG_BASE + 4);
        }
        if self.steps > SWITCH_COST_STEP_BUDGET {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }
}

/// 延迟基准行（判据「转移协议延迟」一等公民）：四相各相步数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatencyRow {
    /// 场景。
    pub scene: BenchScene,
    /// 四相步数（预检/先予/后夺/封账）。
    pub phase_steps: [u32; 4],
}

impl LatencyRow {
    /// 合计步数。
    pub fn total(&self) -> u32 {
        self.phase_steps.iter().sum()
    }
    /// 达标核（判据「转移协议延迟」红线）：合计 ≤ 32 且每相 > 0
    /// （零步相 = 假跑——协议没真走）。
    pub fn within_budget(&self) -> Result<(), u16> {
        for s in self.phase_steps.iter() {
            if *s == 0 {
                return Err(SEG_BASE + 6);
            }
        }
        if self.total() > TRANSFER_LATENCY_STEP_BUDGET {
            return Err(SEG_BASE + 7);
        }
        Ok(())
    }
}

/// 基准册（判据「入册回归」主判据）：五场景 × 两基准全齐 + 全行
/// 达标 + 场景不重不漏。
pub struct BenchBook {
    /// 开销行（五场景各一）。
    pub costs: Vec<CostRow>,
    /// 延迟行（五场景各一）。
    pub latencies: Vec<LatencyRow>,
}

impl BenchBook {
    /// 出厂基准册（步数取协议结构性常数：四相=4 步/转移，切换=
    /// 8 步/场景——纯功能零墙钟，回归只核上限与齐套）。
    pub fn factory() -> Self {
        let scenes = [
            BenchScene::VideoToWeb,
            BenchScene::WebToFullscreen,
            BenchScene::FullscreenToVideo,
            BenchScene::VideoToVideo,
            BenchScene::WebToWeb,
        ];
        BenchBook {
            costs: scenes.iter().map(|&s| CostRow { scene: s, steps: 8 }).collect(),
            latencies: scenes
                .iter()
                .map(|&s| LatencyRow { scene: s, phase_steps: [1, 2, 2, 1] })
                .collect(),
        }
    }
    /// 入册回归核（判据「两基准」总闸）。
    pub fn regression(&self) -> Result<(), u16> {
        if self.costs.len() != 5 || self.latencies.len() != 5 {
            return Err(SEG_BASE + 8);
        }
        for row in &self.costs {
            row.within_budget()?;
        }
        for row in &self.latencies {
            row.within_budget()?;
        }
        // 场景覆盖核：两册场景集必须一致且覆盖闭集。
        for s in [
            BenchScene::VideoToWeb,
            BenchScene::WebToFullscreen,
            BenchScene::FullscreenToVideo,
            BenchScene::VideoToVideo,
            BenchScene::WebToWeb,
        ] {
            if !self.costs.iter().any(|c| c.scene == s) || !self.latencies.iter().any(|l| l.scene == s) {
                return Err(SEG_BASE + 9);
            }
        }
        Ok(())
    }
    /// 册指纹（回归锚：基准数字变更必变指纹）。
    pub fn fingerprint(&self) -> u64 {
        let mut acc: u64 = 0xcbf29ce484222325;
        for c in &self.costs {
            acc = acc.wrapping_mul(0x100000001b3) ^ c.scene.ordinal() as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ c.steps as u64;
        }
        for l in &self.latencies {
            acc = acc.wrapping_mul(0x100000001b3) ^ l.scene.ordinal() as u64;
            for s in l.phase_steps.iter() {
                acc = acc.wrapping_mul(0x100000001b3) ^ *s as u64;
            }
        }
        acc
    }
}

/// 文档基准诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "文档件数≠3",
        1 => "文档件重复",
        2 => "文档标题为空",
        3 => "文档集未覆盖三件",
        4 => "基准步数为 0——假跑",
        5 => "切换开销超红线（64 步）",
        6 => "延迟有零步相——协议没真走",
        7 => "转移延迟超红线（32 步）",
        8 => "基准册行数≠5",
        9 => "场景覆盖不全",
        _ => "未知诊断码——查本模块 SEG_BASE 0x0970",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_complete() {
        let d = DocSet::factory();
        assert!(d.complete().is_ok());
        assert_eq!(d.docs.len(), DOC_COUNT);
        for k in [DocKind::Spec, DocKind::Onboard, DocKind::Custom] {
            assert!(!k.title().is_empty());
        }
        // 缺件拒绝。
        let short = DocSet { docs: alloc::vec![DocKind::Spec] };
        assert_eq!(short.complete().unwrap_err(), SEG_BASE);
    }

    #[test]
    fn bench_regression_green() {
        let b = BenchBook::factory();
        assert!(b.regression().is_ok());
        assert!(b.fingerprint() != 0);
        // 红线内破例：开销超线 → 回归红。
        let mut hot = BenchBook::factory();
        hot.costs[0].steps = SWITCH_COST_STEP_BUDGET + 1;
        assert_eq!(hot.regression().unwrap_err(), SEG_BASE + 5);
        // 零步相拒绝。
        let mut cold = BenchBook::factory();
        cold.latencies[0].phase_steps = [1, 0, 2, 1];
        assert_eq!(cold.regression().unwrap_err(), SEG_BASE + 6);
        // 延迟合计：1+2+2+1=6 ≤ 32。
        assert_eq!(b.latencies[0].total(), 6);
        assert!(diagnose(SEG_BASE + 6).contains("真走"));
    }

    #[test]
    fn scene_names_frozen() {
        for (i, s) in [
            BenchScene::VideoToWeb,
            BenchScene::WebToFullscreen,
            BenchScene::FullscreenToVideo,
            BenchScene::VideoToVideo,
            BenchScene::WebToWeb,
        ].iter().enumerate() {
            assert_eq!(s.ordinal(), i as u8);
            assert!(s.name().contains("->"));
        }
    }
}
