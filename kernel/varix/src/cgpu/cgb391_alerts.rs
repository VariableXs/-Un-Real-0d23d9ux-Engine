//! CGPU-F0391 · 预算告警规则（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 告警：规则引擎（预算耗尽率/降档频率/公平指数跌破阈值——可配置）；
//!   三级告警（提示/警告/严重——阈值与通知渠道对应）；风暴抑制（同类告警
//!   合并限频）；告警历史（全部入总日志）。判据：规则引擎、三档、风暴
//!   抑制、历史、判据。
//!
//! 设计纪律：
//! - 规则是纯谓词+阈值，可配置可枚举；三档映射三个通知渠道（编号化）。
//! - 风暴抑制：同类（规则+档位）在冷却窗内合并（限频一次）。
//! - 历史全量入总日志（append-only，含被抑制事件——审计不失真）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0B50 独占。

use alloc::vec::Vec;

/// 诊断段基址（0x0B50，全仓独占）。
pub const SEG_BASE: u16 = 0x0B50;

pub const E_RULE_UNKNOWN: u16 = SEG_BASE + 1;
pub const E_THRESHOLD_INVALID: u16 = SEG_BASE + 2;
pub const E_CHANNEL_INVALID: u16 = SEG_BASE + 3;

/// 告警三档（阈值升档、渠道对应）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    /// 提示（渠道 0——日志面）。
    Info,
    /// 警告（渠道 1——带外通知）。
    Warn,
    /// 严重（渠道 2——运维直呼）。
    Critical,
}

impl Level {
    pub fn channel(self) -> u8 {
        match self {
            Level::Info => 0,
            Level::Warn => 1,
            Level::Critical => 2,
        }
    }
}

/// 规则谓词种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleKind {
    /// 预算耗尽率 ≥ 阈值（千分位）。
    BurnRate,
    /// 窗口降档频率 ≥ 阈值（次）。
    DowngradeFreq,
    /// 公平指数 < 阈值（千分位）。
    FairnessLow,
}

impl RuleKind {
    pub fn id(self) -> u8 {
        match self {
            RuleKind::BurnRate => 1,
            RuleKind::DowngradeFreq => 2,
            RuleKind::FairnessLow => 3,
        }
    }
}

/// 告警规则（可配置）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    pub kind: RuleKind,
    pub warn: u32,
    pub critical: u32,
    /// 风暴抑制冷却窗（帧）。
    pub cooldown_frames: u64,
}

impl Rule {
    pub fn validate(&self) -> Result<(), u16> {
        // 阈值方向随谓词方向：越上型 warn < critical；越下型 warn > critical。
        let ok = match self.kind {
            RuleKind::FairnessLow => self.warn > self.critical,
            RuleKind::BurnRate | RuleKind::DowngradeFreq => self.warn < self.critical,
        };
        if !ok {
            return Err(E_THRESHOLD_INVALID);
        }
        if self.cooldown_frames == 0 {
            return Err(E_THRESHOLD_INVALID);
        }
        Ok(())
    }
}

/// 触发样（规则引擎输入）。
#[derive(Clone, Copy, Debug)]
pub struct Reading {
    pub kind: RuleKind,
    /// 读数（千分位或次数）。
    pub value: u32,
    pub frame: u64,
}

/// 告警事件（历史行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlertEvent {
    pub kind: RuleKind,
    pub level: Level,
    pub frame: u64,
    pub value: u32,
    /// 是否被风暴抑制（true=只入历史不外发）。
    pub suppressed: bool,
}

/// 告警引擎。
pub struct AlertEngine {
    rules: Vec<Rule>,
    /// 每规则上次外发帧（风暴抑制账）。
    last_sent: Vec<Option<u64>>,
    /// 总日志（含抑制——append-only）。
    pub history: Vec<AlertEvent>,
    /// 外发计数（抑制不算）。
    pub sent_count: u64,
}

impl AlertEngine {
    pub fn new() -> Self {
        AlertEngine { rules: Vec::new(), last_sent: Vec::new(), history: Vec::new(), sent_count: 0 }
    }

    /// 注册规则（判据「规则引擎」——可配置入口）。
    pub fn register(&mut self, r: Rule) -> Result<(), u16> {
        r.validate()?;
        if self.rules.iter().any(|x| x.kind == r.kind) {
            return Ok(()); // 幂等：同类规则覆盖语义由 update 承担
        }
        self.rules.push(r);
        self.last_sent.push(None);
        Ok(())
    }

    pub fn update(&mut self, kind: RuleKind, r: Rule) -> Result<(), u16> {
        r.validate()?;
        match self.rules.iter_mut().find(|x| x.kind == kind) {
            Some(slot) => {
                *slot = r;
                Ok(())
            }
            None => Err(E_RULE_UNKNOWN),
        }
    }

    /// 评估一条读数（判据「三档」：阈值升档 + 渠道对应）。
    fn evaluate(rule: &Rule, rd: &Reading) -> Option<Level> {
        let crossed = match rule.kind {
            RuleKind::BurnRate | RuleKind::DowngradeFreq => rd.value >= rule.critical,
            RuleKind::FairnessLow => rd.value < rule.critical,
        };
        if crossed {
            return Some(Level::Critical);
        }
        let warn = match rule.kind {
            RuleKind::BurnRate | RuleKind::DowngradeFreq => rd.value >= rule.warn,
            RuleKind::FairnessLow => rd.value < rule.warn,
        };
        if warn {
            return Some(Level::Warn);
        }
        None
    }

    /// 喂一条读数：判档 + 风暴抑制 + 全量入历史。
    pub fn feed(&mut self, rd: Reading) -> Result<Option<AlertEvent>, u16> {
        let idx = self.rules.iter().position(|r| r.kind == rd.kind).ok_or(E_RULE_UNKNOWN)?;
        let level = match Self::evaluate(&self.rules[idx], &rd) {
            Some(l) => l,
            None => return Ok(None),
        };
        // 风暴抑制：同规则冷却窗内只外发一次（抑制事件仍入历史）。
        let cooldown = self.rules[idx].cooldown_frames;
        let suppressed = match self.last_sent[idx] {
            Some(f) => rd.frame.saturating_sub(f) < cooldown,
            None => false,
        };
        let ev = AlertEvent { kind: rd.kind, level, frame: rd.frame, value: rd.value, suppressed };
        if !suppressed {
            self.last_sent[idx] = Some(rd.frame);
            self.sent_count += 1;
        }
        self.history.push(ev.clone());
        Ok(Some(ev))
    }

    /// 通知渠道解析（判据「阈值与通知渠道对应」）。
    pub fn channel_of(kind: RuleKind) -> Result<u8, u16> {
        match kind.id() {
            1 | 2 | 3 => Ok(kind_id_default_channel(kind)),
            _ => Err(E_CHANNEL_INVALID),
        }
    }

    /// 历史回看：按档筛选。
    pub fn by_level(&self, l: Level) -> Vec<&AlertEvent> {
        self.history.iter().filter(|e| e.level == l).collect()
    }
}

fn kind_id_default_channel(kind: RuleKind) -> u8 {
    // 默认渠道：按最高可达档位定——耗尽率/降档频率可到严重(2)，
    // 公平低走警告(1)。同 RuleKind::channel 语义一致。
    match kind {
        RuleKind::BurnRate | RuleKind::DowngradeFreq => Level::Critical.channel(),
        RuleKind::FairnessLow => Level::Warn.channel(),
    }
}

impl Default for AlertEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> AlertEngine {
        let mut e = AlertEngine::new();
        e.register(Rule { kind: RuleKind::BurnRate, warn: 800, critical: 950, cooldown_frames: 10 }).unwrap();
        e.register(Rule { kind: RuleKind::FairnessLow, warn: 400, critical: 200, cooldown_frames: 5 }).unwrap();
        e
    }

    #[test]
    fn a91_规则引擎_注册与阈值校验() {
        let mut e = AlertEngine::new();
        // warn ≥ critical 拒。
        assert_eq!(e.register(Rule { kind: RuleKind::BurnRate, warn: 900, critical: 900, cooldown_frames: 1 }).unwrap_err(), E_THRESHOLD_INVALID);
        assert_eq!(e.register(Rule { kind: RuleKind::BurnRate, warn: 950, critical: 900, cooldown_frames: 1 }).unwrap_err(), E_THRESHOLD_INVALID);
        // 冷却 0 拒。
        assert_eq!(e.register(Rule { kind: RuleKind::BurnRate, warn: 1, critical: 2, cooldown_frames: 0 }).unwrap_err(), E_THRESHOLD_INVALID);
        // 合法注册；重复幂等。
        e.register(Rule { kind: RuleKind::BurnRate, warn: 800, critical: 950, cooldown_frames: 10 }).unwrap();
        e.register(Rule { kind: RuleKind::BurnRate, warn: 800, critical: 950, cooldown_frames: 10 }).unwrap();
        assert_eq!(e.rules.len(), 1);
        // 未知规则更新拒（用合法的低向阈值——先注册校验后查册）。
        assert_eq!(e.update(RuleKind::FairnessLow, Rule { kind: RuleKind::FairnessLow, warn: 5, critical: 2, cooldown_frames: 1 }).unwrap_err(), E_RULE_UNKNOWN);
        // 评估：未触发。
        assert_eq!(e.feed(Reading { kind: RuleKind::BurnRate, value: 100, frame: 1 }).unwrap(), None);
        // 未注册规则喂入拒。
        assert_eq!(e.feed(Reading { kind: RuleKind::DowngradeFreq, value: 9, frame: 1 }).unwrap_err(), E_RULE_UNKNOWN);
    }

    #[test]
    fn a91_三档_阈值升档与渠道对应() {
        let mut e = engine();
        // 烧速率 900 → Warn（渠道 1）。
        let ev = e.feed(Reading { kind: RuleKind::BurnRate, value: 900, frame: 1 }).unwrap().unwrap();
        assert_eq!(ev.level, Level::Warn);
        assert_eq!(ev.level.channel(), 1);
        assert_eq!(ev.suppressed, false);
        // 烧速率 960 → Critical（渠道 2）。
        let ev = e.feed(Reading { kind: RuleKind::BurnRate, value: 960, frame: 2 }).unwrap().unwrap();
        assert_eq!(ev.level, Level::Critical);
        assert_eq!(ev.level.channel(), 2);
        // 公平指数方向相反：399 → Warn（<400），150 → Critical（<200）。
        let ev = e.feed(Reading { kind: RuleKind::FairnessLow, value: 399, frame: 3 }).unwrap().unwrap();
        assert_eq!(ev.level, Level::Warn);
        let ev = e.feed(Reading { kind: RuleKind::FairnessLow, value: 150, frame: 4 }).unwrap().unwrap();
        assert_eq!(ev.level, Level::Critical);
        // 渠道映射。
        assert_eq!(AlertEngine::channel_of(RuleKind::BurnRate).unwrap(), 2);
        assert_eq!(AlertEngine::channel_of(RuleKind::DowngradeFreq).unwrap(), 2);
        assert_eq!(AlertEngine::channel_of(RuleKind::FairnessLow).unwrap(), 1);
        // 三档有序。
        assert!(Level::Info < Level::Warn && Level::Warn < Level::Critical);
    }

    #[test]
    fn a91_风暴抑制_冷却窗内合并() {
        let mut e = engine(); // BurnRate 冷却 10
        // 帧 1 触发 Warn → 外发。
        e.feed(Reading { kind: RuleKind::BurnRate, value: 900, frame: 1 }).unwrap().unwrap();
        // 帧 5（窗内）→ 抑制。
        let ev = e.feed(Reading { kind: RuleKind::BurnRate, value: 900, frame: 5 }).unwrap().unwrap();
        assert!(ev.suppressed);
        // 帧 11（窗外）→ 外发。
        let ev = e.feed(Reading { kind: RuleKind::BurnRate, value: 900, frame: 11 }).unwrap().unwrap();
        assert!(!ev.suppressed);
        // 外发 2 次，历史 3 条（抑制也入历史）。
        assert_eq!(e.sent_count, 2);
        assert_eq!(e.history.len(), 3);
        // 历史按档筛选（帧1/5/11 三条 Warn——帧5 虽抑制仍入历史）。
        assert_eq!(e.by_level(Level::Warn).len(), 3);
        assert_eq!(e.by_level(Level::Critical).len(), 0);
        // 公平规则独立冷却（互不抑制）。
        e.feed(Reading { kind: RuleKind::FairnessLow, value: 300, frame: 12 }).unwrap().unwrap();
        e.feed(Reading { kind: RuleKind::FairnessLow, value: 300, frame: 14 }).unwrap().unwrap(); // 冷却5内→抑制
        assert_eq!(e.history.iter().filter(|x| x.suppressed).count(), 2);
    }
}
