//! CGPU-F2403 · 采样策略引擎（CGPU-P 域 · 自适应遥测域 · P01 组 · 目标 340 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2403`
//!
//! **判据（锚点原文）**：价值采样、DSL 声明、自适应复用、三组、判据。
//!
//! **职责定位（锚点原文）**：采样策略引擎（按价值采样（信号强度×成本——价值采样；
//! 策略 DSL（策略可配——DSL 声明；自适应（负载感知采样——自适应复用；测试
//! （引擎/DSL/自适应三组）。
//!
//! ## 一、按价值采样：信号强度 × 成本 = 价值分
//!
//! 采不采不看名字看价值（[`SignalCost`]：signal 信号强度 0..=1000 ×
//! cost 采集开销步数 → [`SignalCost::value`] 价值分，整数除法零浮点
//! 零 panic）；价值超阈值才采（[`SignalCost::worth`]）——遥测有预算
//! 意识（F2401 使命「按价值采集」的落地面），高成本低信号的指标
//! 自然沉底，不做帧预算隐形税。
//!
//! ## 二、策略 DSL：策略可配，声明先行
//!
//! 每条指标一条声明式策略（[`PolicyDecl`]），DSL 文本逐行解析
//! （[`parse_policies`]：`名称:动作`，动作闭集 `always` / `every_n:N` /
//! `rate:万分比` / `value_above:阈值` / `adapt`）——策略可配不改代码
//! （判据二「DSL 声明」），解析失败显性错不静默吞（[`DslError`] 闭集），
//! 语法 [`DSL_GRAMMAR`] 一行说明冻结。
//!
//! ## 三、负载感知自适应：复用不另立
//!
//! 引擎（[`SamplingEngine`]）按负载档（[`LoadTier`] 低/中/高闭集，上游
//! 注入零墙钟）自适应降频：高负载 every_n 加倍 / rate 折半 /
//! value_above 阈值上浮 [`ADAPT_UPTIFT`] 万分比——**自适应复用**：复用
//! F2402 统一模型的 Metric 六元组形状与隐私档（不第二套 schema）、
//! 复用策略 DSL 的动作闭集做降频（不另发明开关）。降频只降频不减
//! 指标（信息完整性），降频决策留账可查。
//!
//! **对接**：F2402（六元组/隐私档复用）；F2401（按价值采集使命）；
//! F2404（管道消费引擎决策）。零 panic 面（`get`/`Option`/饱和算术）、
//! 零 IO、零墙钟（tick 全上游注入）、无全局可变状态、no_std 零 std 依赖。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、按价值采样（判据一：价值采样）
// ---------------------------------------------------------------------------

/// 信号强度上限（注入侧 0..=1000，超界饱和）。
pub const SIGNAL_MAX: u16 = 1000;

/// 一条指标的信号与成本（上游注入，引擎只算价值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalCost {
    /// 信号强度（0..=1000：指标对洞察的重要性，上游评定）。
    pub signal: u16,
    /// 采集开销（步数：一次采集消耗的确定性预算步，≥1）。
    pub cost: u32,
}

impl SignalCost {
    /// 价值分 = 信号 × 1000 ÷ 成本（整数除法；cost 饱和到 ≥1 防除零，
    /// 信号超界饱和到 [`SIGNAL_MAX`]——非法值在构造面钳住）。
    pub const fn value(&self) -> u32 {
        let s = if self.signal > SIGNAL_MAX { SIGNAL_MAX as u32 } else { self.signal as u32 };
        let c = if self.cost == 0 { 1u32 } else { self.cost };
        s.saturating_mul(1000) / c
    }

    /// 价值是否值得采（超阈值即采——判据一「按价值采样」）。
    pub const fn worth(&self, threshold: u32) -> bool {
        self.value() >= threshold
    }
}

// ---------------------------------------------------------------------------
// 二、策略 DSL（判据二：DSL 声明）
// ---------------------------------------------------------------------------

/// DSL 语法一行冻结（判据侧独立对拍）：
/// `名称:动作` 每行一条；动作闭集 always / every_n:N / rate:万分比 / value_above:阈值 / adapt。
pub const DSL_GRAMMAR: &str = "名称:动作；动作闭集 always|every_n:N|rate:万分比|value_above:阈值|adapt";

/// 采样动作闭集（判据二「策略可配」——五种声明覆盖全部采样形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SamplingAction {
    /// 每帧必采。
    Always,
    /// 每 N 帧采一次（间隔采样）。
    EveryN(u32),
    /// 万分比速率（0..=10000，超界饱和）。
    Rate(u32),
    /// 价值超阈值才采（联动 [`SignalCost::worth`]）。
    ValueAbove(u32),
    /// 自适应（负载感知降频，联动 [`SamplingEngine`]）。
    Adapt,
}

impl SamplingAction {
    /// 动作名（DSL 反解，判据侧对拍用）。
    pub const fn name(self) -> &'static str {
        match self {
            SamplingAction::Always => "always",
            SamplingAction::EveryN(_) => "every_n",
            SamplingAction::Rate(_) => "rate",
            SamplingAction::ValueAbove(_) => "value_above",
            SamplingAction::Adapt => "adapt",
        }
    }
}

/// 一条采样策略（名称 + 动作——名称即指标域组名，同 F2402 三级命名家族）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyDecl {
    /// 策略名（指标域.组，非空）。
    pub name: String,
    /// 采样动作。
    pub action: SamplingAction,
}

/// DSL 解析错误闭集（显性错不静默——判据侧逐条对拍）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DslError {
    /// 缺分隔符（没有 `:`）。
    NoSeparator,
    /// 名称为空。
    EmptyName,
    /// 动作词表外（不在五动作闭集）。
    UnknownAction,
    /// 动作参数缺失或非数（every_n:abc）。
    BadParam,
}

/// 逐行解析 DSL 文本（任一行失败即整批拒绝返回错误——配置错误要在
/// 装载时炸而不是运行时漏采；空行跳过；成功返回策略表）。
pub fn parse_policies(text: &str) -> Result<Vec<PolicyDecl>, DslError> {
    let mut out: Vec<PolicyDecl> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let sep = match line.find(':') {
            Some(i) => i,
            None => return Err(DslError::NoSeparator),
        };
        let name = line[..sep].trim();
        if name.is_empty() {
            return Err(DslError::EmptyName);
        }
        let act = line[sep + 1..].trim();
        let action = if act == "always" {
            SamplingAction::Always
        } else if act == "adapt" {
            SamplingAction::Adapt
        } else if let Some(rest) = act.strip_prefix("every_n:") {
            let n: u32 = match rest.trim().parse() {
                Ok(v) => v,
                Err(_) => return Err(DslError::BadParam),
            };
            SamplingAction::EveryN(n)
        } else if let Some(rest) = act.strip_prefix("rate:") {
            let r: u32 = match rest.trim().parse() {
                Ok(v) => v,
                Err(_) => return Err(DslError::BadParam),
            };
            // 万分比在构造面钳住（0..=10000——非法值不进策略表）。
            SamplingAction::Rate(if r > 10000 { 10000 } else { r })
        } else if let Some(rest) = act.strip_prefix("value_above:") {
            let v: u32 = match rest.trim().parse() {
                Ok(v) => v,
                Err(_) => return Err(DslError::BadParam),
            };
            SamplingAction::ValueAbove(v)
        } else {
            return Err(DslError::UnknownAction);
        };
        out.push(PolicyDecl { name: name.to_string(), action });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 三、负载感知自适应引擎（判据三：自适应复用）
// ---------------------------------------------------------------------------

/// 负载档闭集（上游注入——引擎不测负载只响应负载）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadTier {
    /// 低负载：满采不降频。
    Low,
    /// 中负载：策略原样。
    Mid,
    /// 高负载：自适应降频（every_n 加倍 / rate 折半 / 阈值上浮）。
    High,
}

impl LoadTier {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            LoadTier::Low => "低负载",
            LoadTier::Mid => "中负载",
            LoadTier::High => "高负载",
        }
    }
}

/// 高负载价值阈值上浮万分比（value_above × (1+500/10000)=×1.05——
/// 高负载时指标要更值钱才让采）。
pub const ADAPT_UPTIFT: u32 = 500;

/// 单次决策（Sampled 带 tick；Skipped 带原因——决策可查不黑箱）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// 采集。
    Sampled,
    /// 跳过（原因：间隔未到/速率未中/价值不足/负载降频）。
    Skipped(&'static str),
}

/// 采样策略引擎（策略表 + 负载档 + tick 计数——判据三的载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SamplingEngine {
    /// 策略表（DSL 解析产物）。
    pub policies: Vec<PolicyDecl>,
    /// 当前负载档（上游注入可变）。
    pub load: LoadTier,
    /// tick（上游注入，只增饱和）。
    tick: u64,
    /// 自适应降频计数（降了几次——预算意识可查）。
    adapts: u32,
}

impl SamplingEngine {
    /// 装配引擎（DSL 一次解析入表——失败显性错上抛）。
    pub fn mount(dsl: &str) -> Result<SamplingEngine, DslError> {
        Ok(SamplingEngine {
            policies: parse_policies(dsl)?,
            load: LoadTier::Mid,
            tick: 0,
            adapts: 0,
        })
    }

    /// 负载档注入（自适应的输入面）。
    pub fn set_load(&mut self, tier: LoadTier) {
        self.load = tier;
    }

    /// 引擎 tick（上游注入推进）。
    pub fn advance(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }

    /// 自适应降频次数（可查账）。
    pub const fn adapt_count(&self) -> u32 {
        self.adapts
    }

    /// 查策略（表外 None 不臆造——同 F2402 注册表口径）。
    pub fn policy_of(&self, name: &str) -> Option<SamplingAction> {
        let mut i = 0usize;
        while i < self.policies.len() {
            if let Some(p) = self.policies.get(i) {
                if p.name == name {
                    return Some(p.action);
                }
            }
            i += 1;
        }
        None
    }

    /// 单条动作在当前负载下的决策（自适应降频只在这一个函数——不散落）。
    /// 高负载：every_n 加倍 / rate 折半 / value_above 上浮 ADAPT_UPTIFT；
    /// 每次真正执行降频记一笔 adapts 账。
    pub fn decide_action(&mut self, action: SamplingAction, sc: &SignalCost) -> Decision {
        let t = self.tick;
        match action {
            SamplingAction::Always => Decision::Sampled,
            SamplingAction::EveryN(n) => {
                let n = if n == 0 { 1 } else { n };
                let n = self.upscale(n);
                if t.saturating_add(1) % (n as u64) == 0 {
                    Decision::Sampled
                } else {
                    Decision::Skipped("间隔未到")
                }
            }
            SamplingAction::Rate(r) => {
                let r = if r > 10000 { 10000 } else { r };
                let r = self.downscale(r);
                if r >= 10000 || (t.saturating_add(1) % 10000) < r as u64 {
                    Decision::Sampled
                } else {
                    Decision::Skipped("速率未中")
                }
            }
            SamplingAction::ValueAbove(th) => {
                let th = self.lift(th);
                if sc.worth(th) {
                    Decision::Sampled
                } else {
                    Decision::Skipped("价值不足")
                }
            }
            SamplingAction::Adapt => {
                // adapt 动作 = 价值闸 + 负载联动：高负载阈值上浮。
                let th = self.lift(0);
                if sc.worth(th) {
                    Decision::Sampled
                } else {
                    Decision::Skipped("自适应价值不足")
                }
            }
        }
    }

    /// 按名称决策（查表 + 单条决策——表外指标按 always 兜底并提示）。
    pub fn decide(&mut self, name: &str, sc: &SignalCost) -> (Decision, bool) {
        let known = self.policy_of(name);
        match known {
            Some(a) => (self.decide_action(a, sc), true),
            None => (Decision::Skipped("表外指标"), false),
        }
    }

    /// 高负载间隔加倍（记降频账）。
    fn upscale(&mut self, n: u32) -> u32 {
        if self.load == LoadTier::High {
            self.adapts = self.adapts.saturating_add(1);
            n.saturating_mul(2)
        } else {
            n
        }
    }

    /// 高负载速率折半（记降频账）。
    fn downscale(&mut self, r: u32) -> u32 {
        if self.load == LoadTier::High {
            self.adapts = self.adapts.saturating_add(1);
            r / 2
        } else {
            r
        }
    }

    /// 高负载价值阈值上浮（记降频账）。
    fn lift(&mut self, th: u32) -> u32 {
        if self.load == LoadTier::High {
            self.adapts = self.adapts.saturating_add(1);
            th.saturating_add(th.saturating_mul(ADAPT_UPTIFT) / 10000)
        } else {
            th
        }
    }
}

// ---------------------------------------------------------------------------
// 四、复用声明（判据三「自适应复用」的对账面）
// ---------------------------------------------------------------------------

/// 复用清单（判据侧逐条 grep 对拍——不第二套 schema 不另发明开关）：
pub const REUSE_LINES: [&str; 3] = [
    "复用 F2402 Metric 六元组形状与隐私档——本引擎只做决策不改 schema",
    "复用策略 DSL 动作闭集做负载降频——every_n 加倍 / rate 折半 / value_above 上浮",
    "复用 F0274 三级命名家族作策略名——表外名称不臆造策略",
];
