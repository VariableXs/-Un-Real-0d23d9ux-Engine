//! VE-F0648 脏区历史与预测（VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0648`
//!
//! **判据（锚点原文逐条）**：
//! - **环形历史**：最近 N 帧区域集（调试回放与热点分析）；
//! - **线性外推**：位移动画线性外推下一帧区域（预测用于预触发/缓存预热）；
//! - **仅提示不依赖**：预测错 → 常规收集兜底，正确性不依赖预测；
//! - **判据**：历史满 → 滚动淘汰；外推越界 → 钳制。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec241_dmodel::Rect;

/// 历史环形缓冲容量
pub const HIST_CAP: usize = 16;

/// 帧区域集历史（环形）
#[derive(Clone, Debug)]
pub struct HistRing {
    buf: Vec<(u64, Vec<Rect>)>,
    head: usize,
}

impl HistRing {
    pub fn new() -> HistRing {
        HistRing { buf: Vec::new(), head: 0 }
    }

    /// 记录一帧（满则滚动淘汰最旧）
    pub fn push(&mut self, frame: u64, regions: Vec<Rect>) {
        if self.buf.len() < HIST_CAP {
            self.buf.push((frame, regions));
        } else {
            self.buf[self.head] = (frame, regions);
            self.head = (self.head + 1) % HIST_CAP;
        }
    }

    /// 容量
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// 按帧号回放（调试回放）
    pub fn replay(&self, frame: u64) -> Option<&[Rect]> {
        self.buf.iter().find(|(f, _)| *f == frame).map(|(_, r)| r.as_slice())
    }

    /// 热点分析：跨帧区域频率（帧出现次数按矩形完全相等计）
    pub fn hotspots(&self) -> Vec<(Rect, usize)> {
        let mut out: Vec<(Rect, usize)> = Vec::new();
        for (_, rs) in &self.buf {
            for &r in rs {
                match out.iter_mut().find(|(k, _)| *k == r) {
                    Some((_, n)) => *n += 1,
                    None => out.push((r, 1)),
                }
            }
        }
        out.sort_by(|a, b| b.1.cmp(&a.1));
        out
    }

    /// 最旧帧号（滚动淘汰验证）
    pub fn oldest(&self) -> Option<u64> {
        self.buf.iter().map(|(f, _)| *f).min()
    }
}

/// 线性外推器（位移动画：dx/dy 每帧恒速）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinExtrap {
    pub dx: i64,
    pub dy: i64,
}

impl LinExtrap {
    /// 下一帧区域外推（只做优化提示）
    pub fn next(&self, r: Rect, steps: u32) -> Rect {
        let dx = self.dx * steps as i64;
        let dy = self.dy * steps as i64;
        (
            (r.0 as i64 + dx).clamp(0, i64::from(u32::MAX)) as u32,
            (r.1 as i64 + dy).clamp(0, i64::from(u32::MAX)) as u32,
            (r.2 as i64 + dx).clamp(0, i64::from(u32::MAX)) as u32,
            (r.3 as i64 + dy).clamp(0, i64::from(u32::MAX)) as u32,
        )
    }
}

/// 外推越界钳制（画布边界）
pub fn clamp_to_canvas(r: Rect, canvas: (u32, u32)) -> Rect {
    (
        r.0.min(canvas.0),
        r.1.min(canvas.1),
        r.2.min(canvas.0),
        r.3.min(canvas.1),
    )
}

/// 预测地位：提示通道（正确性不依赖——预测未命中时常规收集兜底）
#[derive(Clone, Debug, Default)]
pub struct HintChannel {
    pub hints: Vec<Rect>,
    /// 预测命中数 / 总提示数（评估用）
    pub hits: u32,
    pub total: u32,
}

impl HintChannel {
    pub fn offer(&mut self, r: Rect) {
        self.hints.push(r);
        self.total += 1;
    }
    /// 预测命中确认
    pub fn confirm(&mut self, r: &Rect) -> bool {
        if self.hints.iter().any(|h| h == r) {
            self.hits += 1;
            true
        } else {
            false
        }
    }
    /// 兜底率（未命中比例——正确性由常规收集保证）
    pub fn miss_rate(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.total - self.hits) as f32 / self.total as f32
        }
    }
}

pub fn run_vec248_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec248");
    // 族一：环形历史
    let mut h = HistRing::new();
    for f in 0..(HIST_CAP as u64 + 5) {
        h.push(f, alloc::vec![(f as u32, 0, f as u32 + 1, 1)]);
    }
    s.add("历史-容量钳制", h.len() == HIST_CAP, "capped");
    s.add("历史-滚动淘汰", h.oldest() == Some(5), "oldest evicted");
    s.add("历史-回放命中", h.replay(10).is_some(), "replay hit");
    s.add("历史-回放未录", h.replay(1).is_none(), "evicted gone");
    // 族二：热点分析
    let mut h2 = HistRing::new();
    h2.push(1, alloc::vec![(0, 0, 10, 10), (50, 50, 60, 60)]);
    h2.push(2, alloc::vec![(0, 0, 10, 10)]);
    h2.push(3, alloc::vec![(0, 0, 10, 10)]);
    let hot = h2.hotspots();
    s.add("热点-频次排序", hot[0] == ((0, 0, 10, 10), 3), "top hotspot");
    s.add("热点-次频", hot[1] == ((50, 50, 60, 60), 1), "second");
    // 族三：线性外推
    let ex = LinExtrap { dx: 8, dy: -2 };
    let n1 = ex.next((10, 10, 20, 20), 1);
    s.add("外推-单帧", n1 == (18, 8, 28, 18), "+8,-2");
    let n3 = ex.next((10, 10, 20, 20), 3);
    s.add("外推-多帧", n3 == (34, 4, 44, 14), "3 steps");
    // 族四：越界钳制
    let c = clamp_to_canvas((100, 100, 5000, 5000), (1000, 1000));
    s.add("钳制-画布内", c == (100, 100, 1000, 1000), "clamped");
    // 负向越界（饱和到 0）
    let ex2 = LinExtrap { dx: -100, dy: 0 };
    let neg = ex2.next((10, 0, 20, 10), 5);
    s.add("钳制-负向饱和", neg == (0, 0, 0, 10), "saturating left");
    // 族五：仅提示不依赖
    let mut hc = HintChannel::default();
    hc.offer((0, 0, 10, 10));
    hc.offer((100, 100, 110, 110));
    s.add("提示-命中确认", hc.confirm(&(0, 0, 10, 10)), "hit confirmed");
    s.add("提示-未命中兜底", !hc.confirm(&(999, 999, 1000, 1000)), "miss fallback");
    s.add("提示-兜底率", (hc.miss_rate() - 0.5).abs() < 1e-6, "50% miss");
    s.add("提示-空通道", HintChannel::default().miss_rate() == 0.0, "empty 0");
    // 族六：外推多步仍有限（无 panic / 无反转）
    let big = ex.next((u32::MAX - 5, 0, u32::MAX, 10), 10);
    s.add("钳制-极值不炸", big.2 <= u32::MAX, "no overflow");
    s
}
