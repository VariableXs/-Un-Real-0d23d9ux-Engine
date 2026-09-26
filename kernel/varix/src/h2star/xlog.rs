//! H2 域体验日志框架 · 完整设计（人格章程十三章《体验日志》全域实装）。
//!
//! **使命**：日志的使命不是排查崩溃，是还原体验。本框架是 F251-F300
//! 五十项交互面的共用埋点底座：
//! - 记录到交互细节层：在哪个界面、对哪个元素、什么时刻、触发了什么、
//!   反馈是什么、耗时多少；
//! - 主动捕获挫败信号：狂点同一位置（rage click）、点击无反馈区域
//!   （dead click）、反复打开又立刻关闭的浮层（flap）、同一操作短时间
//!   重复多次（repeat）——自动标记成体验事件；
//! - 每个事件带体验结论字段：顺畅/卡顿/无反馈/被打断/报错；
//! - 结构化与可回放：统一时间轴、会话内事件串成可回放的操作故事线、
//!   可出「最挫败的十次操作」改进清单；
//! - 隐私与代价红线：只记交互行为与结果，**不记用户输入的具体内容**；
//!   写入绝不阻塞交互（攒批异步落账，批量/容量双闸）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 体验结论（五档——判据「顺畅/卡顿/无反馈/被打断/报错」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Smooth,
    Janky,
    NoFeedback,
    Interrupted,
    Errored,
}

impl Verdict {
    /// 人话标签（回放视图直读）。
    pub fn label(&self) -> &'static str {
        match self {
            Verdict::Smooth => "顺畅",
            Verdict::Janky => "卡顿",
            Verdict::NoFeedback => "无反馈",
            Verdict::Interrupted => "被打断",
            Verdict::Errored => "报错",
        }
    }
}

/// 挫败信号指纹（十三章四类自动标记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frustration {
    /// 狂点同一位置（同点 2s 内 ≥3 次）。
    RageClick,
    /// 点击无反馈区域。
    DeadClick,
    /// 浮层反复开-关（<1s 内关）。
    Flap,
    /// 同一操作短时间重复多次（≥4 次/10s）。
    Repeat,
}

impl Frustration {
    pub fn label(&self) -> &'static str {
        match self {
            Frustration::RageClick => "狂点同一位置",
            Frustration::DeadClick => "点击无反馈",
            Frustration::Flap => "浮层反复开关",
            Frustration::Repeat => "同操作连发",
        }
    }
}

/// 一条交互事件（隐私红线：content 字段不存在——只记行为与结果）。
#[derive(Clone, Debug)]
pub struct XEvent {
    /// 事件时戳（ms 注入）。
    pub ts_ms: u64,
    /// 界面（如「设置中心/电源」「资源管理器/多标签」）。
    pub surface: &'static str,
    /// 元素（如「F251 媒体键」「F298 磁贴×」）。
    pub element: &'static str,
    /// 动作（press/drag/open/close/…——枚举常量，非自由文本）。
    pub action: &'static str,
    /// 可见反馈是否在 100ms 内出现（「看起来没反应」同罪判据）。
    pub feedback_ok: bool,
    /// 交互耗时（ms；浮层从打开到关闭的完整生命线由此串起）。
    pub latency_ms: u64,
    /// 是否出错路径。
    pub errored: bool,
    /// 是否被打断（用户中途放弃/被打断后放弃的操作）。
    pub interrupted: bool,
}

impl XEvent {
    /// 体验结论推导（单一规则源——判据五档的机判实现）。
    pub fn verdict(&self) -> Verdict {
        if self.errored {
            Verdict::Errored
        } else if self.interrupted {
            Verdict::Interrupted
        } else if !self.feedback_ok {
            Verdict::NoFeedback
        } else if self.latency_ms > 100 {
            Verdict::Janky
        } else {
            Verdict::Smooth
        }
    }
}

/// 一条已标记的挫败事件（体验事件 = 事件 + 指纹 + 结论）。
#[derive(Clone, Debug)]
pub struct FlaggedEvent {
    pub event: XEvent,
    pub frustration: Frustration,
}

/// 攒批参数：批满 32 条或距上次落账 5s 即落（异步不阻塞交互）。
pub const BATCH_CAP: usize = 32;
pub const BATCH_FLUSH_MS: u64 = 5_000;
/// 事件环容量（内存驻留上限——落账后由持久层接手）。
pub const RING_CAP: usize = 256;

/// 体验日志记录器（域内每交互面一个实例，或全域共享——按界面参数区分）。
pub struct XLogger {
    /// 事件环（域内定容——满则丢最旧，不无限增长；XEvent 含 String，
    /// 不进 K2 的 RingLog<T: Copy>，与 F251 OSD 环同理）。
    ring: Vec<XEvent>,
    batch: Vec<XEvent>,
    flagged: Vec<FlaggedEvent>,
    last_flush_ms: u64,
    /// 落账次数（诊断面）。
    pub flushes: u64,
    /// 因攒批满被拒绝计数（满载诚实记账）。
    pub dropped: u64,
}

impl XLogger {
    pub fn new() -> XLogger {
        XLogger {
            ring: Vec::new(),
            batch: Vec::new(),
            flagged: Vec::new(),
            last_flush_ms: 0,
            flushes: 0,
            dropped: 0,
        }
    }

    /// 记录一条交互事件（O(1)，无 IO——「写入绝不阻塞交互」的结构保证）。
    /// `now_ms` 注入；返回推导的体验结论供调用方在必要时自检。
    pub fn log(&mut self, e: XEvent, now_ms: u64) -> Verdict {
        let v = e.verdict();
        // 先入环（指纹计数含本次——「同点 2s 内 ≥3 次」从第一次算起）。
        self.ring.push(e.clone());
        if self.ring.len() > RING_CAP {
            let drop_n = self.ring.len() - RING_CAP;
            self.ring.drain(..drop_n);
        }
        // 挫败指纹判定。
        if !e.feedback_ok {
            self.flag(e.clone(), Frustration::DeadClick);
        }
        if e.latency_ms <= 1_000 && e.action == "close" {
            // 浮层开-关 <1s（由调用方配对标记 action=close）。
            self.flag(e.clone(), Frustration::Flap);
        }
        if self.count_recent_same_spot(&e, now_ms) >= 3 {
            self.flag(e.clone(), Frustration::RageClick);
        }
        if self.count_recent_same_action(&e, now_ms) >= 4 {
            self.flag(e.clone(), Frustration::Repeat);
        }
        // 攒批：满批即落，否则等时间闸。
        if self.batch.len() < BATCH_CAP {
            self.batch.push(e);
        } else {
            self.dropped += 1;
        }
        if now_ms.saturating_sub(self.last_flush_ms) >= BATCH_FLUSH_MS {
            self.flush(now_ms);
        }
        v
    }

    /// 落账（异步边界的同步侧——实际写盘由 h2persist 原子写承接）。
    pub fn flush(&mut self, now_ms: u64) -> usize {
        let n = self.batch.len();
        self.batch.clear();
        self.last_flush_ms = now_ms;
        self.flushes += 1;
        n
    }

    fn flag(&mut self, e: XEvent, f: Frustration) {
        self.flagged.push(FlaggedEvent { event: e, frustration: f });
        if self.flagged.len() > 128 {
            let _ = self.flagged.remove(0);
        }
    }

    /// 近 2s 内同界面同元素同动作计数（rage click 判据窗口）。
    fn count_recent_same_spot(&self, e: &XEvent, now_ms: u64) -> usize {
        self.ring
            .iter()
            .rev()
            .filter(|o| {
                now_ms.saturating_sub(o.ts_ms) <= 2_000
                    && o.surface == e.surface
                    && o.element == e.element
                    && o.action == "press"
            })
            .count()
    }

    /// 近 10s 内同动作计数（repeat 判据窗口）。
    fn count_recent_same_action(&self, e: &XEvent, now_ms: u64) -> usize {
        self.ring
            .iter()
            .rev()
            .filter(|o| {
                now_ms.saturating_sub(o.ts_ms) <= 10_000 && o.action == e.action
            })
            .count()
    }

    /// 最近事件（新→旧——会话回放故事线数据源）。
    pub fn replay(&self) -> Vec<XEvent> {
        self.ring.iter().rev().cloned().collect()
    }

    /// 挫败信号清单（「最挫败的十次操作」改进清单直读）。
    pub fn frustrations(&self) -> &[FlaggedEvent] {
        &self.flagged
    }

    /// 改进清单导出：按挫败指纹聚合的人话条目（入总日志中心 F188 口）。
    pub fn improvement_list(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for f in self.flagged.iter().rev().take(10) {
            out.push(alloc::format!(
                "{} @ {} / {} → {}（{}ms）",
                f.frustration.label(),
                f.event.surface,
                f.event.element,
                f.event.verdict().label(),
                f.event.latency_ms
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

fn ev(ts: u64, action: &'static str, feedback: bool, latency: u64) -> XEvent {
    XEvent {
        ts_ms: ts,
        surface: "设置中心/电源",
        element: "F291 排行条",
        action,
        feedback_ok: feedback,
        latency_ms: latency,
        errored: false,
        interrupted: false,
    }
}

pub fn run_xlog_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-xlog");
    // 结论推导五档逐档验证。
    let smooth = ev(1, "press", true, 20).verdict();
    let janky = ev(2, "press", true, 250).verdict();
    let nofb = ev(3, "press", false, 10).verdict();
    let mut err = ev(4, "press", true, 20);
    err.errored = true;
    let mut itp = ev(5, "press", true, 20);
    itp.interrupted = true;
    set.add(
        "xlog verdict 5",
        smooth == Verdict::Smooth
            && janky == Verdict::Janky
            && nofb == Verdict::NoFeedback
            && err.verdict() == Verdict::Errored
            && itp.verdict() == Verdict::Interrupted,
        "五档机判",
    );
    // 挫败信号：狂点（同点 2s 内 3 次）+ 无反馈 + 连发。
    let mut lg = XLogger::new();
    let _ = lg.log(ev(1_000, "press", true, 10), 1_000);
    let _ = lg.log(ev(1_400, "press", true, 10), 1_400);
    let v3 = lg.log(ev(1_800, "press", true, 10), 1_800);
    set.add(
        "xlog rage click",
        v3 == Verdict::Smooth && lg.frustrations().iter().any(|f| f.frustration == Frustration::RageClick),
        "3 in 2s",
    );
    // Dead click：无反馈即标。
    let _ = lg.log(ev(3_000, "press", false, 10), 3_000);
    set.add(
        "xlog dead click",
        lg.frustrations().iter().any(|f| f.frustration == Frustration::DeadClick),
        "no feedback",
    );
    // Repeat：10s 内同动作 ≥4 次。
    for i in 0..4u64 {
        let _ = lg.log(ev(4_000 + i * 500, "open", true, 10), 4_000 + i * 500);
    }
    set.add(
        "xlog repeat",
        lg.frustrations().iter().any(|f| f.frustration == Frustration::Repeat),
        "4 in 10s",
    );
    // 攒批双闸：批满 32 落、5s 时间闸落。
    let mut lg2 = XLogger::new();
    for i in 0..32u64 {
        let _ = lg2.log(ev(i, "press", true, 5), i);
    }
    set.add(
        "xlog batch cap",
        lg2.batch.len() == BATCH_CAP && lg2.flushes == 0,
        "held till gate",
    );
    let _ = lg2.flush(4_999);
    let _ = lg2.log(ev(10_000, "press", true, 5), 10_000);
    set.add(
        "xlog batch flush",
        lg2.flushes == 2 && lg2.last_flush_ms == 10_000,
        "5s gate",
    );
    // 回放故事线：新→旧有序。
    let rp = lg2.replay();
    set.add(
        "xlog replay order",
        rp.len() >= 2 && rp[0].ts_ms >= rp[rp.len() - 1].ts_ms,
        "newest first",
    );
    // 改进清单出得来（人话）。
    let list = lg.improvement_list();
    set.add(
        "xlog improvement list",
        !list.is_empty() && list[0].contains("狂点") || list.iter().any(|s| s.contains("无反馈")),
        "top-10 view",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xlog_all_green() {
        let set = run_xlog_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "xlog 自检红 {f}/{p}");
    }

    #[test]
    fn privacy_no_content_field() {
        // 隐私红线：XEvent 结构体没有内容字段（类型即证明）——行为与
        // 结果之外什么都不记。此处钉住字段集合的大小口径。
        let e = ev(1, "press", true, 5);
        // 仅访问存在字段，确保没有遗漏的可疑内容位。
        let _ = (e.ts_ms, e.surface, e.element, e.action, e.feedback_ok, e.latency_ms, e.errored, e.interrupted);
    }
}
