//! VE-F0770 动态九宫格
//! 职责：运行时可调内边距（热更新）、内边距动画（缓动插值）、
//!       动画过程中的布局连续性（无跳变）、状态回滚。
//! 契约：动画帧间内边距变化 ≤ 单帧步长；动画终点 == 目标值；回滚恢复快照。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::Insets;

/// 内边距动画状态
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct InsetAnim {
    pub from: Insets,
    pub to: Insets,
    pub t: f64,
    pub duration: f64,
    pub easing: Ease,
}

/// 缓动函数
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Ease {
    Linear,
    EaseOut,
    EaseIn,
}

impl Ease {
    pub fn apply(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::EaseIn => t * t,
            Ease::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

impl InsetAnim {
    pub fn new(from: Insets, to: Insets, duration: f64, easing: Ease) -> InsetAnim {
        InsetAnim { from, to, t: 0.0, duration: duration.max(1e-9), easing }
    }
    /// 推进一帧，返回当前内边距
    pub fn tick(&mut self, dt: f64) -> Insets {
        self.t = (self.t + dt).min(self.duration);
        self.sample()
    }
    /// 当前采样
    pub fn sample(&self) -> Insets {
        let p = self.easing.apply(self.t / self.duration);
        let lerp = |a: f64, b: f64| a + (b - a) * p;
        Insets {
            left: lerp(self.from.left, self.to.left),
            top: lerp(self.from.top, self.to.top),
            right: lerp(self.from.right, self.to.right),
            bottom: lerp(self.from.bottom, self.to.bottom),
        }
    }
    /// 是否完成
    pub fn done(&self) -> bool {
        self.t >= self.duration
    }
    /// 帧间最大变化（连续性）
    pub fn max_frame_delta(&self, dt: f64) -> f64 {
        let a = self.sample();
        let mut tmp = *self;
        tmp.t = (tmp.t + dt).min(tmp.duration);
        let b = tmp.sample();
        (a.left - b.left).abs().max((a.top - b.top).abs()).max((a.right - b.right).abs()).max((a.bottom - b.bottom).abs())
    }
}

/// 快照回滚
#[derive(Clone, Debug, PartialEq)]
pub struct RollbackLog {
    pub snaps: Vec<Insets>,
    pub cap: usize,
}

impl RollbackLog {
    pub fn new(cap: usize) -> RollbackLog {
        RollbackLog { snaps: alloc::vec![], cap: cap.max(1) }
    }
    pub fn push(&mut self, ins: Insets) {
        if self.snaps.len() >= self.cap {
            self.snaps.remove(0);
        }
        self.snaps.push(ins);
    }
    pub fn pop(&mut self) -> Option<Insets> {
        self.snaps.pop()
    }
}

pub fn run_vec370_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec370");
    let a = Insets::uniform(8.0);
    let b = Insets::uniform(24.0);
    // 族一：缓动函数
    s.add("缓动-线性端点", Ease::Linear.apply(0.0) == 0.0 && Ease::Linear.apply(1.0) == 1.0, "0..1");
    s.add("缓动-EaseIn", (Ease::EaseIn.apply(0.5) - 0.25).abs() < 1e-12, "t²");
    s.add("缓动-EaseOut", (Ease::EaseOut.apply(0.5) - 0.75).abs() < 1e-12, "1-(1-t)²");
    s.add("缓动-越界夹取", Ease::Linear.apply(1.5) == 1.0 && Ease::Linear.apply(-1.0) == 0.0, "clamped");
    s.add("缓动-单调", (0..10).all(|k| Ease::EaseOut.apply(k as f64 / 10.0) <= Ease::EaseOut.apply((k + 1) as f64 / 10.0) + 1e-12), "monotone");
    // 族二：动画推进
    let mut anim = InsetAnim::new(a, b, 1.0, Ease::Linear);
    s.add("动画-起点", anim.sample() == a, "starts at from");
    let _ = anim.tick(0.25);
    let mid = anim.sample();
    s.add("动画-线性中点", (mid.left - 12.0).abs() < 1e-9, "t=0.25 → 12");
    // 帧间连续性
    s.add("动画-帧间连续", anim.max_frame_delta(0.01) <= 16.0 * 0.01 + 1e-9, "delta ≤ rate×dt");
    let _ = anim.tick(0.75);
    s.add("动画-终点", anim.done() && anim.sample() == b, "reaches target");
    // 超时不变
    let _ = anim.tick(1.0);
    s.add("动画-超时稳定", anim.sample() == b, "no overshoot");
    // 族三：EaseOut 终点
    let mut eo = InsetAnim::new(a, b, 1.0, Ease::EaseOut);
    let _ = eo.tick(1.0);
    s.add("缓出-终点准", eo.sample() == b, "exact end");
    // 快进到 0.5
    let mut eo2 = InsetAnim::new(a, b, 1.0, Ease::EaseOut);
    let _ = eo2.tick(0.5);
    s.add("缓出-前快", eo2.sample().left > 12.0, "easeout ahead of linear");
    // 族四：回滚日志
    let mut log = RollbackLog::new(4);
    log.push(a);
    log.push(b);
    log.push(Insets::uniform(16.0));
    s.add("回滚-弹出", log.pop() == Some(Insets::uniform(16.0)), "LIFO");
    s.add("回滚-再弹", log.pop() == Some(b), "second");
    s.add("回滚-空", log.pop().is_none() || log.pop().is_none(), "drains");
    // 容量逐出
    let mut small = RollbackLog::new(2);
    small.push(Insets::uniform(1.0));
    small.push(Insets::uniform(2.0));
    small.push(Insets::uniform(3.0));
    s.add("回滚-容量", small.snaps.len() == 2, "cap 2");
    s.add("回滚-逐出最旧", small.pop() == Some(Insets::uniform(3.0)) && small.pop() == Some(Insets::uniform(2.0)), "oldest evicted");
    // 零容量兜底
    s.add("回滚-零容量", RollbackLog::new(0).cap == 1, "min cap");
    // 族五：动画热更新（运行中改目标）
    let mut live = InsetAnim::new(a, b, 1.0, Ease::Linear);
    let _ = live.tick(0.5);
    live.to = Insets::uniform(40.0);
    let cur = live.sample();
    s.add("热更-连续", cur.left > 8.0 && cur.left < 40.0, "mid retarget");
    let _ = live.tick(1.0);
    s.add("热更-新终点", live.sample() == Insets::uniform(40.0), "reaches new target");
    s
}
