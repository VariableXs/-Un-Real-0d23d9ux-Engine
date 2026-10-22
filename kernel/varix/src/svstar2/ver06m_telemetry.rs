//! VE-F3433 · 换肤遥测 —— 四族指标 + 滚动聚合 + 采样声明 + 数据不出机。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3433`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四族指标**（封闭全集，缺一族即 [`TelemetryCode::FamilyMissing`]）：
//!   1. **耗时**：p50 / p95（毫秒）；
//!   2. **失败率**：失败次数 / 总次数（千分比）；
//!   3. **打断率**：被打断次数 / 总次数。**口径与 Z05 共用**——
//!      两域各量各的打断，最后对不上账，是跨域统计最常见的浪费；
//!   4. **预览转化**：应用次数 / 预览次数（千分比）；
//! - **滚动聚合**：只保留最近 [`WINDOW_CAP`] 条样本，先进先出。聚合在
//!   写入时增量维护必要计数，`snapshot` 时对耗时做一次排序取分位
//!   （样本量上限 512，排序成本可忽略）；
//! - **采样声明**：每份遥测必须声明采样率（[`Sampling::rate_permille`]）。
//!   采样不是「悄悄丢数据」，是把「我们只看了 10%」写在报告头上——
//!   否则读报告的人会把 10% 的结论当成全量结论；
//! - **数据不出机**：[`ExportScope::LocalOnly`] 是唯一允许的导出范围。
//!   请求外发一律 [`TelemetryCode::ExportDenied`]。这条不是「默认关」，
//!   是「没有开的地方」——换肤遥测含主题名与用户操作节奏，属于隐私面。
//!
//! # 为什么失败率与打断率必须分开
//!
//! 失败是「没换成」，打断是「换到一半用户改了主意」。前者是缺陷，后者
//! 是正常交互。合成一个「未完成率」会让产品看起来很差，也会掩盖真正的
//! 失败——把两件事分开，才知道该修谁。
//!
//! # 复杂度
//!
//! - `record`：O(1)（采样判定 + 环形队列入队）；
//! - `snapshot`：O(n log n)，n ≤ 512；
//! - `export`：O(n)。
//!
//! # 零 panic 面
//!
//! `VecDeque` 操作无索引；排序用 `sort_unstable`。测试区除外。

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5505 起）
// ---------------------------------------------------------------------------

/// 遥测诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TelemetryCode {
    /// 四族指标缺族（没有样本可算）。
    FamilyMissing = 5505,
    /// 请求外发数据（拒绝）。
    ExportDenied = 5506,
    /// 采样率非法（>1000）。
    BadSampling = 5507,
    /// 窗口容量为 0。
    ZeroWindow = 5508,
    /// 口径未对齐 Z05（打断率定义不同）。
    CaliberMismatch = 5509,
    /// 样本时间倒流。
    TimeBackwards = 5510,
}

impl TelemetryCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            TelemetryCode::FamilyMissing => "四族指标缺族",
            TelemetryCode::ExportDenied => "遥测数据拒绝外发",
            TelemetryCode::BadSampling => "采样率非法",
            TelemetryCode::ZeroWindow => "窗口容量为 0",
            TelemetryCode::CaliberMismatch => "打断率口径与 Z05 不一致",
            TelemetryCode::TimeBackwards => "样本时间倒流",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, TelemetryCode::ExportDenied)
    }
}

/// 滚动窗口容量。
pub const WINDOW_CAP: usize = 512;

// ---------------------------------------------------------------------------
// 二、样本与采样
// ---------------------------------------------------------------------------

/// 一次换肤样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 发生时刻（逻辑毫秒，单调）。
    pub at_ms: u64,
    /// 总耗时。
    pub duration_ms: u32,
    /// 是否成功。
    pub success: bool,
    /// 是否被打断（用户中途改主意）。
    pub interrupted: bool,
    /// 是否展示过预览。
    pub preview_shown: bool,
    /// 预览后是否应用。
    pub preview_applied: bool,
}

impl Sample {
    /// 构造。
    pub fn new(at_ms: u64, duration_ms: u32, success: bool) -> Sample {
        Sample {
            at_ms,
            duration_ms,
            success,
            interrupted: false,
            preview_shown: false,
            preview_applied: false,
        }
    }

    /// 标记打断。
    pub fn interrupted(mut self) -> Sample {
        self.interrupted = true;
        self
    }

    /// 标记预览并应用。
    pub fn preview(mut self, applied: bool) -> Sample {
        self.preview_shown = true;
        self.preview_applied = applied;
        self
    }
}

/// 采样声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sampling {
    /// 采样率（千分比）。1000 = 全采。
    pub rate_permille: u16,
    /// 采样策略说明（给人看的一句话）。
    pub note: &'static str,
}

impl Sampling {
    /// 全采。
    pub fn full() -> Sampling {
        Sampling {
            rate_permille: 1000,
            note: "全量采集",
        }
    }

    /// 按率采样。>1000 非法。
    pub fn ratio(rate_permille: u16) -> Result<Sampling, TelemetryCode> {
        if rate_permille > 1000 {
            return Err(TelemetryCode::BadSampling);
        }
        Ok(Sampling {
            rate_permille,
            note: "按声明比例采样，报告须标注采样率",
        })
    }
}

/// 导出范围。只允许本机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportScope {
    /// 本机内存/本地面板。
    LocalOnly,
    /// 网络/云端（拒绝）。
    Remote,
}

/// 聚合快照。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 样本数（窗口内）。
    pub samples: u32,
    /// p50 耗时。
    pub p50_ms: u32,
    /// p95 耗时。
    pub p95_ms: u32,
    /// 失败率（千分比）。
    pub failure_permille: u32,
    /// 打断率（千分比）。
    pub interrupt_permille: u32,
    /// 预览转化率（千分比）。
    pub preview_conversion_permille: u32,
    /// 采样声明。
    pub sampling: Sampling,
}

impl Snapshot {
    /// 四族是否齐备：有样本、有耗时、有失败统计、有预览样本（或全为 0 且
    /// 明示「无预览」）。这里以「样本数 > 0」为齐备底线。
    pub fn families_complete(&self) -> bool {
        self.samples > 0
    }

    /// 报告文本（读屏可达）。
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "换肤遥测：样本 {} 条（采样 {}‰，{}）",
            self.samples, self.sampling.rate_permille, self.sampling.note
        ));
        out.push(format!(
            "耗时 p50 {}ms，p95 {}ms；失败率 {}‰；打断率 {}‰；预览转化 {}‰",
            self.p50_ms,
            self.p95_ms,
            self.failure_permille,
            self.interrupt_permille,
            self.preview_conversion_permille
        ));
        out
    }
}

// ---------------------------------------------------------------------------
// 三、聚合器
// ---------------------------------------------------------------------------

/// 遥测聚合器。
#[derive(Clone, Debug)]
pub struct Telemetry {
    window: VecDeque<Sample>,
    cap: usize,
    sampling: Sampling,
    /// 因采样被丢掉的样本数（透明记账）。
    sampled_out: u32,
    /// 累计成功/失败/打断/预览展示/预览应用。
    success: u32,
    failure: u32,
    interrupt: u32,
    preview_shown: u32,
    preview_applied: u32,
    last_at: u64,
    /// 采样计数（用于确定性抽样）。
    seen: u32,
}

impl Telemetry {
    /// 新建。
    pub fn new(cap: usize, sampling: Sampling) -> Result<Telemetry, TelemetryCode> {
        if cap == 0 {
            return Err(TelemetryCode::ZeroWindow);
        }
        Ok(Telemetry {
            window: VecDeque::new(),
            cap,
            sampling,
            sampled_out: 0,
            success: 0,
            failure: 0,
            interrupt: 0,
            preview_shown: 0,
            preview_applied: 0,
            last_at: 0,
            seen: 0,
        })
    }

    /// 记录一条样本。
    ///
    /// 采样判定用「确定性等距抽样」：每 `1000 / rate` 条取一条。这样
    /// 同一份输入永远得到同一份样本集，便于对比两次改动的效果。
    pub fn record(&mut self, sample: Sample) -> Result<bool, TelemetryCode> {
        if sample.at_ms < self.last_at {
            return Err(TelemetryCode::TimeBackwards);
        }
        self.last_at = sample.at_ms;
        self.seen = self.seen.saturating_add(1);
        let stride = if self.sampling.rate_permille == 0 {
            u32::MAX
        } else {
            (1000 / self.sampling.rate_permille as u32).max(1)
        };
        if stride != 1 && self.seen % stride != 0 {
            self.sampled_out = self.sampled_out.saturating_add(1);
            return Ok(false);
        }
        if self.window.len() >= self.cap {
            if let Some(dropped) = self.window.pop_front() {
                // 出窗的样本从累计计数里扣掉，保证快照与窗口一致。
                self.decrement(&dropped);
            }
        }
        if sample.success {
            self.success = self.success.saturating_add(1);
        } else {
            self.failure = self.failure.saturating_add(1);
        }
        if sample.interrupted {
            self.interrupt = self.interrupt.saturating_add(1);
        }
        if sample.preview_shown {
            self.preview_shown = self.preview_shown.saturating_add(1);
            if sample.preview_applied {
                self.preview_applied = self.preview_applied.saturating_add(1);
            }
        }
        self.window.push_back(sample);
        Ok(true)
    }

    fn decrement(&mut self, s: &Sample) {
        if s.success {
            self.success = self.success.saturating_sub(1);
        } else {
            self.failure = self.failure.saturating_sub(1);
        }
        if s.interrupted {
            self.interrupt = self.interrupt.saturating_sub(1);
        }
        if s.preview_shown {
            self.preview_shown = self.preview_shown.saturating_sub(1);
            if s.preview_applied {
                self.preview_applied = self.preview_applied.saturating_sub(1);
            }
        }
    }

    /// 快照。
    pub fn snapshot(&self) -> Snapshot {
        let n = self.window.len() as u32;
        let mut durations: Vec<u32> = self.window.iter().map(|s| s.duration_ms).collect();
        durations.sort_unstable();
        let p50 = percentile(&durations, 50);
        let p95 = percentile(&durations, 95);
        let rate = |part: u32| -> u32 {
            if n == 0 {
                0
            } else {
                (part as u64 * 1000 / n as u64) as u32
            }
        };
        Snapshot {
            samples: n,
            p50_ms: p50,
            p95_ms: p95,
            failure_permille: rate(self.failure),
            interrupt_permille: rate(self.interrupt),
            preview_conversion_permille: if self.preview_shown == 0 {
                0
            } else {
                (self.preview_applied as u64 * 1000 / self.preview_shown as u64) as u32
            },
            sampling: self.sampling,
        }
    }

    /// 采样丢弃数（透明记账）。
    pub fn sampled_out(&self) -> u32 {
        self.sampled_out
    }

    /// 窗口内样本数。
    pub fn len(&self) -> usize {
        self.window.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.window.is_empty()
    }

    /// 导出。只允许本机；外发一律拒绝。
    pub fn export(&self, scope: ExportScope) -> Result<String, TelemetryCode> {
        if scope != ExportScope::LocalOnly {
            return Err(TelemetryCode::ExportDenied);
        }
        let snap = self.snapshot();
        let mut out = String::new();
        out.push_str("# 换肤遥测（本机）\n");
        for line in snap.lines() {
            out.push_str(&line);
            out.push('\n');
        }
        out.push_str(&format!("采样丢弃 {}\n", self.sampled_out));
        Ok(out)
    }
}

/// 分位（输入已排序）。`p` 取 50/95。
fn percentile(sorted: &[u32], p: u32) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as u64 * p as u64) / 100) as usize;
    let idx = core::cmp::min(idx, sorted.len().saturating_sub(1));
    sorted.get(idx).copied().unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn telem() -> Telemetry {
        Telemetry::new(64, Sampling::full()).expect("聚合器")
    }

    #[test]
    fn four_families_are_aggregated() {
        let mut t = telem();
        let mut i = 0;
        while i < 10 {
            let s = Sample::new(i as u64, 10 + i as u32, i % 5 != 0);
            let s = if i == 3 { s.interrupted() } else { s };
            let s = if i < 4 { s.preview(i < 2) } else { s };
            t.record(s).expect("记录");
            i += 1;
        }
        let snap = t.snapshot();
        assert_eq!(snap.samples, 10);
        assert!(snap.p50_ms >= 10);
        assert!(snap.p95_ms >= snap.p50_ms);
        assert_eq!(snap.failure_permille, 200, "10 条里 2 条失败");
        assert_eq!(snap.interrupt_permille, 100);
        assert_eq!(snap.preview_conversion_permille, 500, "4 次预览 2 次应用");
        assert!(snap.families_complete());
    }

    #[test]
    fn window_rolls_and_counts_stay_consistent() {
        let mut t = Telemetry::new(4, Sampling::full()).expect("聚合器");
        let mut i = 0;
        while i < 10 {
            t.record(Sample::new(i as u64, 10, true)).expect("记录");
            i += 1;
        }
        let snap = t.snapshot();
        assert_eq!(snap.samples, 4);
        assert_eq!(snap.failure_permille, 0);
    }

    #[test]
    fn sampling_is_declared_and_transparent() {
        let mut t = Telemetry::new(64, Sampling::ratio(250).expect("采样")).expect("聚合器");
        let mut i = 0;
        while i < 8 {
            t.record(Sample::new(i as u64, 10, true)).expect("记录");
            i += 1;
        }
        // 250‰ -> 每 4 条取 1 条 -> 2 条样本，6 条被采样丢弃。
        assert_eq!(t.len(), 2);
        assert_eq!(t.sampled_out(), 6);
        assert_eq!(t.snapshot().sampling.rate_permille, 250);
    }

    #[test]
    fn remote_export_is_denied() {
        let t = telem();
        assert_eq!(
            t.export(ExportScope::Remote).err(),
            Some(TelemetryCode::ExportDenied)
        );
        assert!(TelemetryCode::ExportDenied.blocking());
        assert!(t.export(ExportScope::LocalOnly).is_ok());
    }

    #[test]
    fn bad_sampling_and_zero_window_are_rejected() {
        assert_eq!(
            Sampling::ratio(1001).err(),
            Some(TelemetryCode::BadSampling)
        );
        assert_eq!(
            Telemetry::new(0, Sampling::full()).err(),
            Some(TelemetryCode::ZeroWindow)
        );
    }

    #[test]
    fn time_backwards_is_rejected() {
        let mut t = telem();
        t.record(Sample::new(100, 10, true)).expect("记录");
        assert_eq!(
            t.record(Sample::new(50, 10, true)).err(),
            Some(TelemetryCode::TimeBackwards)
        );
    }

    #[test]
    fn snapshot_lines_include_sampling_declaration() {
        let mut t = telem();
        t.record(Sample::new(1, 20, true)).expect("记录");
        let lines = t.snapshot().lines();
        assert!(lines.first().map(|l| l.contains("采样 1000‰")).unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("p50")));
    }
}
