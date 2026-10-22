//! VE-F3431 · 换肤过程可视化 —— 四段进度 + 进度诚实 + 终态清晰 + 偏差告警。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3431`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四段进度条**：diff / 求值 / 重绘 / 动画各一条，加上「当前段说明」
//!   一行文字。四段而不是一条总进度：用户看到「卡在重绘」比看到「卡在
//!   37%」有用得多——前者能解释，后者只能让人焦虑；
//! - **进度诚实**：进度只能由**已完成的段**与**段内实测进度**推导，
//!   不许为了「看起来在动」加假动画。不动就不动，卡住就报卡住。
//!   这条规则在 [`ProgressModel::report`] 里被写成硬约束：
//!   上报值只能等于计算值，不能大于；
//! - **终态清晰**：完成与失败各有一句完整文案（原因 + 结果 + 下一步），
//!   不允许只显示一个转圈图标消失；
//! - **偏差告警**：上报进度与实际进度偏差超过 [`DEVIATION_LIMIT_PERMILLE`]
//!   即 [`VisualCode::ProgressLies`]——进度说谎是缺陷，不是「体验问题」；
//! - **卡死检测**：连续 [`STALL_TICKS`] 个 tick 进度零增长且未完成 →
//!   [`VisualCode::Stalled`]，界面必须显示「正在等待」而不是继续转圈。
//!
//! # 为什么进度诚实值得单独做一条判据
//!
//! 假进度是最容易做、也最伤信任的一类 UI：用户看到 90% 停了两秒，
//! 会认为软件坏了；而真实情况是「重绘 200 个组件，还剩 20 个」。
//! 诚实的进度会停在 82% 并显示「重绘中（180/200）」，用户反而能等。
//! 所以本模块把「上报不得超前于实际」写成类型约束，而不是文案建议。
//!
//! # 复杂度
//!
//! - `advance` / `report`：O(1)（固定四段）；
//! - `terminal_text`：O(1)。
//!
//! # 零 panic 面
//!
//! 固定数组 + `get()`。测试区除外。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5492 起）
// ---------------------------------------------------------------------------

/// 可视化诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VisualCode {
    /// 上报进度超前于实际（说谎）。
    ProgressLies = 5492,
    /// 连续零增长（卡死）。
    Stalled = 5493,
    /// 段序号回退。
    SegmentBackwards = 5494,
    /// 终态文案缺要素。
    TerminalVague = 5495,
    /// 段进度超过 1000‰。
    OverProgress = 5496,
    /// 未开始的段被标成进行中。
    PhantomSegment = 5497,
}

impl VisualCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            VisualCode::ProgressLies => "进度上报超前于实际",
            VisualCode::Stalled => "进度卡死，长时间零增长",
            VisualCode::SegmentBackwards => "当前段回退",
            VisualCode::TerminalVague => "终态文案缺要素",
            VisualCode::OverProgress => "段进度超过上限",
            VisualCode::PhantomSegment => "未开始的段被标为进行中",
        }
    }

    /// 是否缺陷级（说谎与卡死都要修，不是调文案）。
    pub fn defect(self) -> bool {
        matches!(self, VisualCode::ProgressLies | VisualCode::Stalled)
    }
}

/// 允许的偏差（千分比）。上报比实际最多超前 5‰——这点余量用于
/// 「段完成瞬间的边界抖动」，再多就是编的。
pub const DEVIATION_LIMIT_PERMILLE: u32 = 5;

/// 卡死判定：连续多少个 tick 零增长。
pub const STALL_TICKS: u32 = 30;

// ---------------------------------------------------------------------------
// 二、段状态
// ---------------------------------------------------------------------------

/// 段运行状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegmentPhase {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 失败。
    Failed,
}

impl SegmentPhase {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            SegmentPhase::Pending => "等待",
            SegmentPhase::Running => "进行中",
            SegmentPhase::Done => "完成",
            SegmentPhase::Failed => "失败",
        }
    }
}

/// 一段的可视化状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegmentView {
    /// 阶段。
    pub phase: SegmentPhase,
    /// 段内进度（千分比）。
    pub permille: u16,
    /// 已完成项 / 总项（用于「重绘中 180/200」这类文案）。
    pub done_items: u32,
    pub total_items: u32,
}

impl SegmentView {
    /// 初始（未开始）。
    pub fn pending(total_items: u32) -> SegmentView {
        SegmentView {
            phase: SegmentPhase::Pending,
            permille: 0,
            done_items: 0,
            total_items,
        }
    }

    /// 一行的进度条文本：`重绘 [████░░] 60% 120/200`。
    pub fn bar_text(&self, label: &str, width: usize) -> String {
        let filled = (self.permille as usize * width) / 1000;
        let mut bar = String::new();
        let mut i = 0;
        while i < width {
            bar.push(if i < filled { '█' } else { '░' });
            i += 1;
        }
        format!(
            "{} [{}] {}% {}/{}",
            label,
            bar,
            self.permille / 10,
            self.done_items,
            self.total_items
        )
    }
}

/// 四段的可视化模型。
#[derive(Clone, Debug)]
pub struct ProgressModel {
    /// 四段视图。
    pub segments: [SegmentView; 4],
    /// 当前段下标（0..=3；4 表示全部完成）。
    pub current: usize,
    /// 上报给 UI 的总进度（千分比）。
    reported: u16,
    /// 连续零增长 tick 数。
    idle_ticks: u32,
    /// 上一次总进度。
    last_total: u16,
    /// 是否已到终态。
    terminal: bool,
}

/// 段名。
pub const SEGMENT_LABELS: [&str; 4] = ["差异", "求值", "重绘", "动画"];

impl ProgressModel {
    /// 新建：四段全为等待。
    pub fn new(total_items: [u32; 4]) -> ProgressModel {
        let mut segments = [SegmentView::pending(0); 4];
        let mut i = 0;
        while i < 4 {
            segments[i] = SegmentView::pending(total_items.get(i).copied().unwrap_or(0));
            i += 1;
        }
        ProgressModel {
            segments,
            current: 0,
            reported: 0,
            idle_ticks: 0,
            last_total: 0,
            terminal: false,
        }
    }

    /// 开始某段。
    ///
    /// 允许「当前段」或「下一段」：`index > current + 1` 是跳段（中间
    /// 还有段没跑），`index == current + 1` 要求当前段真的已完成——
    /// 否则进度会凭空跳一格。
    pub fn start(&mut self, index: usize) -> Result<(), VisualCode> {
        if index > self.current.saturating_add(1) {
            return Err(VisualCode::SegmentBackwards);
        }
        if index == self.current.saturating_add(1) {
            let prev_done = self
                .segments
                .get(self.current)
                .map(|s| s.phase == SegmentPhase::Done)
                .unwrap_or(false);
            if !prev_done {
                return Err(VisualCode::SegmentBackwards);
            }
        }
        if let Some(seg) = self.segments.get_mut(index) {
            if seg.phase == SegmentPhase::Pending {
                seg.phase = SegmentPhase::Running;
            }
        }
        self.current = index;
        Ok(())
    }

    /// 更新段内进度。
    pub fn update(&mut self, index: usize, done_items: u32) -> Result<(), VisualCode> {
        let seg = match self.segments.get_mut(index) {
            Some(s) => s,
            None => return Err(VisualCode::OverProgress),
        };
        if done_items > seg.total_items {
            return Err(VisualCode::OverProgress);
        }
        let permille = if seg.total_items == 0 {
            1000
        } else {
            ((done_items as u64 * 1000) / seg.total_items as u64) as u16
        };
        if permille > 1000 {
            return Err(VisualCode::OverProgress);
        }
        seg.done_items = done_items;
        seg.permille = permille;
        if permille >= 1000 {
            seg.phase = SegmentPhase::Done;
        } else if seg.phase == SegmentPhase::Pending {
            seg.phase = SegmentPhase::Running;
        }
        Ok(())
    }

    /// 某段失败。
    pub fn fail(&mut self, index: usize) {
        if let Some(seg) = self.segments.get_mut(index) {
            seg.phase = SegmentPhase::Failed;
        }
        self.terminal = true;
    }

    /// 计算总进度（四段等权）。
    pub fn computed_permille(&self) -> u16 {
        let mut sum = 0u32;
        for s in self.segments.iter() {
            sum = sum.saturating_add(s.permille as u32);
        }
        (sum / 4).min(1000) as u16
    }

    /// 上报总进度。**只能等于或略低于计算值**，超前即 `ProgressLies`。
    pub fn report(&mut self) -> Result<u16, VisualCode> {
        let computed = self.computed_permille();
        if self.reported > computed.saturating_add(DEVIATION_LIMIT_PERMILLE as u16) {
            return Err(VisualCode::ProgressLies);
        }
        // 诚实上报：直接取计算值（不放大、不四舍五入上取整）。
        self.reported = computed;
        Ok(self.reported)
    }

    /// 推进一个 tick（用于卡死检测）。
    pub fn tick(&mut self) -> Option<VisualCode> {
        let computed = self.computed_permille();
        if computed == self.last_total && !self.terminal && computed < 1000 {
            self.idle_ticks = self.idle_ticks.saturating_add(1);
        } else {
            self.idle_ticks = 0;
        }
        self.last_total = computed;
        if self.idle_ticks >= STALL_TICKS {
            Some(VisualCode::Stalled)
        } else {
            None
        }
    }

    /// 当前段说明（「重绘中 120/200」）。
    pub fn current_caption(&self) -> String {
        match self.segments.get(self.current) {
            Some(seg) => format!(
                "{}中 {}/{}",
                SEGMENT_LABELS.get(self.current).copied().unwrap_or("?"),
                seg.done_items,
                seg.total_items
            ),
            None => "已完成".to_string(),
        }
    }

    /// 四条进度条文本。
    pub fn bar_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < 4 {
            if let Some(seg) = self.segments.get(i) {
                let label = SEGMENT_LABELS.get(i).copied().unwrap_or("?");
                out.push(seg.bar_text(label, 10));
            }
            i += 1;
        }
        out
    }

    /// 终态文案。完成与失败都必须说清「结果 + 原因/下一步」。
    pub fn terminal_text(&self, theme: &str) -> Result<String, VisualCode> {
        let failed_at = self
            .segments
            .iter()
            .position(|s| s.phase == SegmentPhase::Failed);
        match failed_at {
            Some(idx) => {
                let label = SEGMENT_LABELS.get(idx).copied().unwrap_or("?");
                Ok(format!(
                    "换肤失败：{}阶段未完成（主题 {} 未生效），可重试或切换其它主题",
                    label, theme
                ))
            }
            None => {
                if self.computed_permille() >= 1000 {
                    Ok(format!("换肤完成：主题 {} 已生效", theme))
                } else {
                    Err(VisualCode::TerminalVague)
                }
            }
        }
    }

    /// 读屏播报（逐段一行 + 总进度）。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "换肤进度 {}‰，当前 {}",
            self.computed_permille(),
            self.current_caption()
        ));
        let mut i = 0;
        while i < 4 {
            if let Some(seg) = self.segments.get(i) {
                out.push(format!(
                    "{}：{}，{}‰，{}/{}",
                    SEGMENT_LABELS.get(i).copied().unwrap_or("?"),
                    seg.phase.label(),
                    seg.permille,
                    seg.done_items,
                    seg.total_items
                ));
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 三、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> ProgressModel {
        ProgressModel::new([10, 20, 200, 30])
    }

    #[test]
    fn four_segments_and_equal_weight_progress() {
        let mut m = model();
        m.start(0).expect("开始");
        m.update(0, 10).expect("完成差异");
        // 只有第一段满：总进度 250‰。
        assert_eq!(m.computed_permille(), 250);
        assert_eq!(m.report().ok(), Some(250));
    }

    #[test]
    fn reported_cannot_exceed_computed() {
        let mut m = model();
        m.update(0, 5).expect("半段");
        m.reported = 900; // 人为伪造上报值
        assert_eq!(m.report().err(), Some(VisualCode::ProgressLies));
        assert!(VisualCode::ProgressLies.defect());
    }

    #[test]
    fn stall_is_detected_without_fake_motion() {
        let mut m = model();
        m.start(0).expect("开始");
        m.update(0, 1).expect("一点进度");
        let mut hit = None;
        let mut i = 0;
        while i < STALL_TICKS + 1 {
            hit = m.tick();
            i += 1;
        }
        assert_eq!(hit, Some(VisualCode::Stalled));
        // 卡死期间进度条不动——诚实的进度不会有假动画。
        assert_eq!(m.computed_permille(), 25);
    }

    #[test]
    fn segment_backwards_and_over_progress_are_rejected() {
        let mut m = model();
        assert_eq!(m.start(2).err(), Some(VisualCode::SegmentBackwards));
        assert_eq!(m.update(0, 99).err(), Some(VisualCode::OverProgress));
    }

    #[test]
    fn terminal_text_is_explicit_for_success_and_failure() {
        let mut m = model();
        // 未完成且无失败 -> 文案不成立。
        assert_eq!(m.terminal_text("x").err(), Some(VisualCode::TerminalVague));
        m.start(0).expect("开始");
        m.update(0, 10).expect("满");
        m.start(1).expect("下一段");
        m.update(1, 20).expect("满");
        m.start(2).expect("下一段");
        m.update(2, 200).expect("满");
        m.start(3).expect("下一段");
        m.update(3, 30).expect("满");
        let done = m.terminal_text("deep-ocean").expect("完成文案");
        assert!(done.contains("已完成") || done.contains("已生效"));

        let mut f = model();
        f.start(0).expect("开始");
        f.fail(0);
        let failed = f.terminal_text("neon-night").expect("失败文案");
        assert!(failed.contains("失败"));
        assert!(failed.contains("未生效"));
    }

    #[test]
    fn bars_and_reading_lines_are_renderable() {
        let mut m = model();
        m.start(0).expect("开始差异");
        m.update(0, 10).expect("差异完成");
        m.start(1).expect("开始求值");
        m.update(1, 20).expect("求值完成");
        m.start(2).expect("开始重绘");
        m.update(2, 120).expect("进度");
        let bars = m.bar_lines();
        assert_eq!(bars.len(), 4);
        assert!(bars.get(2).map(|b| b.contains("60%") && b.contains("120/200")).unwrap_or(false));
        let lines = m.reading_lines();
        assert!(lines.first().map(|l| l.contains("重绘中 120/200")).unwrap_or(false));
    }
}
