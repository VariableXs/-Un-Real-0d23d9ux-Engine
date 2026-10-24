//! VE-F1408 · 音频事件总线（VE-H 域 · 音频引擎 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1408`
//!
//! **规格原文**：发布-订阅音频事件（"发生了什么"→"该响什么"解耦——事件源
//! 不知道音效存在，令牌表决定响什么，音效替换零代码改动）；事件四要素（事件
//! 名/来源/强度/上下文——四要素驱动令牌选择与 intensity 分级 F1422）；节流
//! 合并（高频事件窗口内合并为单次触发+强度聚合——滚动音效轰炸的根治）；与
//! F1332 可视化总线的边界（那条是"正在播的"数据流、这条是"该不该播"决策
//! 流——往可视化总线发触发事件=架构违例）；订阅优先级（系统通知音>应用音效
//! ——类别仲裁）。判据：四要素、节流合并、边界声明、优先级、判据。
//!
//! **设计要点**：
//! - 解耦语义：发布方只发事件四要素，订阅方登记"听什么→响哪个令牌"——
//!   换音效 = 改令牌登记，发布方一行不动（十四开放的架构基础）；
//! - 四要素是路由键：强度进令牌分级（F1422），上下文进令牌选择（如通知
//!   类别）——四要素齐一个事件才合法；
//! - 节流合并按事件名配置：窗口内 N 发 1 收，强度按策略聚合（取大/取末），
//!   滚动音效从"轰炸"变"一次有强度梯度的触发"；
//! - 两总线边界显性：决策流（本总线）与数据流（F1332 可视化）方向不同，
//!   走错总线 = 架构违例，显性拒绝留痕；
//! - 优先级仲裁：同 sink 同 tick 竞争时系统类必胜，应用类让行（可记账的
//!   让行，不是静默丢）。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、事件四要素
// ---------------------------------------------------------------------------

/// 音频事件（四要素齐备才合法）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioEvent {
    /// 事件名（语义标识，如 "notification.arrived"）。
    pub name: String,
    /// 来源（哪个模块发的）。
    pub source: String,
    /// 强度（0..1000，千分比；驱动 F1422 令牌分级）。
    pub intensity: u32,
    /// 上下文（附加数据，如通知类别）。
    pub context: Vec<(String, String)>,
}

impl AudioEvent {
    /// 四要素校验：名/来源非空、强度在值域。
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("事件名缺失——四要素之一".to_string());
        }
        if self.source.is_empty() {
            return Err("来源缺失——四要素之一".to_string());
        }
        if self.intensity > 1000 {
            return Err(format!("强度 {} 超出 0..1000", self.intensity));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、两总线边界（决策流 vs 数据流）
// ---------------------------------------------------------------------------

/// 总线种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusKind {
    /// 决策流：本总线——"该不该播、响什么"。
    Decision,
    /// 数据流：F1332 可视化总线——"正在播的"只读镜像。
    Visualization,
}

/// 边界声明（防混用的机器可读文本）。
pub const BOUNDARY_DECLARATION: &str =
    "决策流（本总线）只收触发事件；可视化总线（F1332）只收播放数据只读镜像——\
往可视化总线发触发事件、往决策流发播放状态都是架构违例";

/// 边界错误（三要素）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusError {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl BusError {
    fn new(code: &'static str, what: String, why: String, next: String) -> BusError {
        BusError { code, what, why, next }
    }
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 三、订阅与优先级
// ---------------------------------------------------------------------------

/// 订阅者类别（优先级仲裁的轴）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubClass {
    /// 系统通知音（必胜级）。
    System,
    /// 应用音效（让行级）。
    App,
}

impl SubClass {
    pub fn priority(self) -> u8 {
        match self {
            SubClass::System => 2,
            SubClass::App => 1,
        }
    }
}

/// 一条订阅：听什么事件 → 响哪个令牌。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subscription {
    pub event_name: String,
    pub token_name: String,
    pub class: SubClass,
    pub sink: String,
}

/// 分发结果（该不该播的决策产物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dispatch {
    pub token_name: String,
    pub sink: String,
    pub class: SubClass,
    /// 聚合后的强度（节流合并后）。
    pub intensity: u32,
    pub event_name: String,
}

// ---------------------------------------------------------------------------
// 四、事件总线（发布订阅 + 节流合并 + 仲裁）
// ---------------------------------------------------------------------------

/// 强度聚合策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntensityAgg {
    /// 取窗口内最大强度（滚动的"最猛一下"）。
    Max,
    /// 取窗口内最后强度（"最新状态"）。
    Latest,
}

/// 节流规则（按事件名配置）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThrottleRule {
    pub window_ticks: u64,
    /// 窗口内最多触发次数（1 = 完全合并为单次）。
    pub max_per_window: u32,
    pub agg: IntensityAgg,
}

/// 总线错误统一用 BusError；发布账目：
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishRecord {
    pub event_name: String,
    pub source: String,
    pub at_tick: u64,
    /// 本条是否被节流窗口吞并（合并进前一条）。
    pub merged: bool,
}

/// 音频事件总线。
#[derive(Debug)]
pub struct EventBus {
    pub subs: Vec<Subscription>,
    pub throttles: Vec<(String, ThrottleRule)>,
    /// 节流窗口状态：事件名 → (窗口起点, 已触发数, 强度累积)。
    windows: Vec<(String, u64, u32, u32)>,
    /// 决策产物账（分发记录）。
    pub dispatches: Vec<Dispatch>,
    /// 让行账（优先级仲裁：低优先级同 tick 让行）。
    pub yield_log: Vec<(String, String, u64)>, // (event, sink, tick)
    /// 走错总线的违例账。
    pub boundary_violations: Vec<BusError>,
    pub published_total: u64,
    pub merged_total: u64,
}

impl EventBus {
    pub fn new() -> EventBus {
        EventBus {
            subs: Vec::new(),
            throttles: Vec::new(),
            windows: Vec::new(),
            dispatches: Vec::new(),
            yield_log: Vec::new(),
            boundary_violations: Vec::new(),
            published_total: 0,
            merged_total: 0,
        }
    }

    /// 订阅：听什么事件 → 响哪个令牌（解耦点）。
    pub fn subscribe(&mut self, sub: Subscription) {
        self.subs.push(sub);
    }

    /// 配置节流规则。
    pub fn set_throttle(&mut self, event_name: &str, rule: ThrottleRule) {
        self.throttles.retain(|(n, _)| n != event_name);
        self.throttles.push((event_name.to_string(), rule));
    }

    /// 发布（决策流）：四要素校验 → 节流合并 → 匹配订阅 → 优先级仲裁分发。
    pub fn publish(
        &mut self,
        event: &AudioEvent,
        tick: u64,
    ) -> Result<usize, BusError> {
        event.validate().map_err(|m| {
            BusError::new(
                "E_EVENT_INVALID",
                m,
                "四要素是事件的总纲：缺要素的事件无从路由".to_string(),
                "补齐事件名/来源/强度/上下文后再发布".to_string(),
            )
        })?;
        self.published_total += 1;

        // 节流合并：命中规则且窗口未开新窗 → 合并强度、不触发分发。
        if let Some((_, rule)) = self
            .throttles
            .iter()
            .find(|(n, _)| *n == event.name)
            .map(|(n, r)| (n.clone(), *r))
        {
            let pos = self
                .windows
                .iter()
                .position(|(n, _, _, _)| *n == event.name);
            let (win_start, fired, agg) = match pos {
                Some(p) => {
                    let (ws, fired0, agg0) = {
                        let w = &self.windows[p];
                        (w.1, w.2, w.3)
                    };
                    if tick >= ws + rule.window_ticks {
                        // 开新窗
                        let w = self.windows.get_mut(p).unwrap();
                        w.1 = tick;
                        w.2 = 0;
                        w.3 = 0;
                        (tick, 0u32, 0u32)
                    } else {
                        (ws, fired0, agg0)
                    }
                }
                None => {
                    self.windows.push((event.name.clone(), tick, 0, 0));
                    (tick, 0u32, 0u32)
                }
            };
            // 窗口内聚合强度
            let new_agg = match rule.agg {
                IntensityAgg::Max => agg.max(event.intensity),
                IntensityAgg::Latest => event.intensity,
            };
            let pos = self
                .windows
                .iter()
                .position(|(n, _, _, _)| *n == event.name)
                .unwrap();
            if fired < rule.max_per_window {
                {
                    let w = self.windows.get_mut(pos).unwrap();
                    w.2 = fired + 1;
                    w.3 = new_agg;
                }
                // 首触发用当前聚合强度分发；后续窗口内到达 = 合并
                if fired > 0 {
                    self.merged_total += 1;
                    self.update_dispatch_intensity(&event.name, new_agg);
                    return Ok(0);
                }
                self.dispatch_matched(&event.clone(), new_agg, tick);
                return Ok(1);
            } else {
                // 窗口满：合并但不再触发
                let w = self.windows.get_mut(pos).unwrap();
                w.3 = new_agg;
                self.merged_total += 1;
                self.update_dispatch_intensity(&event.name, new_agg);
                return Ok(0);
            }
        }

        self.dispatch_matched(event, event.intensity, tick);
        Ok(1)
    }

    /// 更新最近一次同令牌分发的强度（合并后的强度聚合落账）。
    fn update_dispatch_intensity(&mut self, event_name: &str, intensity: u32) {
        for d in self.dispatches.iter_mut().rev() {
            if d.event_name == event_name {
                d.intensity = intensity;
                break;
            }
        }
    }

    fn dispatch_matched(&mut self, event: &AudioEvent, intensity: u32, tick: u64) {
        let matched: Vec<Subscription> = self
            .subs
            .iter()
            .filter(|s| s.event_name == event.name)
            .cloned()
            .collect();
        if matched.is_empty() {
            return;
        }
        // 同 sink 竞争：系统类必胜，应用类让行（记账不静默）。
        let mut by_sink: Vec<(String, Vec<Subscription>)> = Vec::new();
        for s in matched.iter() {
            match by_sink.iter_mut().find(|(k, _)| *k == s.sink) {
                Some((_, v)) => v.push(s.clone()),
                None => by_sink.push((s.sink.clone(), vec![s.clone()])),
            }
        }
        for (sink, cands) in by_sink.iter() {
            let mut best = cands
                .iter()
                .max_by_key(|s| s.class.priority())
                .unwrap()
                .clone();
            for s in cands.iter() {
                if s.class.priority() < best.class.priority() {
                    self.yield_log.push((event.name.clone(), s.sink.clone(), tick));
                } else {
                    best = s.clone();
                }
            }
            self.dispatches.push(Dispatch {
                token_name: best.token_name.clone(),
                sink: sink.clone(),
                class: best.class,
                intensity,
                event_name: event.name.clone(),
            });
        }
    }

    /// 发布到可视化总线（F1332）：触发事件 = 架构违例，显性拒绝。
    pub fn publish_to_visualization(&mut self, event: &AudioEvent) -> Result<(), BusError> {
        let e = BusError::new(
            "E_WRONG_BUS",
            format!("事件 {} 是触发事件，不能走可视化总线", event.name),
            BOUNDARY_DECLARATION.to_string(),
            "触发事件走决策流（EventBus::publish）；可视化总线只收播放数据镜像".to_string(),
        );
        self.boundary_violations.push(e.clone());
        Err(e)
    }
}
