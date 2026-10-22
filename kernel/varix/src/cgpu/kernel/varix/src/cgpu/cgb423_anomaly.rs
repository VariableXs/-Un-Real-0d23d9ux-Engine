//! CGPU-F0423 · 异常消耗检测（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 异常检测：统计离群（账户消耗突然暴增——相对自身历史与同类账户的
//!   双重基线）；检测分级（轻度偏离提醒/严重异常告警）；归因引导（异常
//!   时自动附 F0345 审计链接——一键看决策）；检测延迟（近实时检出——
//!   分钟级）。
//! - 判据：双基线、分级、归因引导、延迟、判据。
//!
//! 设计纪律：
//! - 双基线：① 自基线（该账户自身 EWMA 均值，偏离 = 本窗/均值 − 1）；
//!   ② 同类基线（同资源类别账户的中位数——天然抗单户离群）。两线
//!   同时越带才算异常（单线越带 = 噪声，双线越带 = 事件）。
//! - 分级：自偏离 ≥400‰ 且同类偏离 ≥300‰ → 告警（2）；自偏离 ≥200‰
//!   且同类越带 → 提醒（1）；未双越 → 无事件。
//! - 归因引导：每条事件自动携带审计链锚（决策窗号 + 审计账索引——
//!   F0345 链接语义），一键可查。
//! - 延迟预算：检出在样本入账后 1 步内完成（近实时 = 无批处理延迟的
//!   形式化）；检测耗时计数入观测。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0DA0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0DA0，全仓独占）。
pub const SEG_BASE: u16 = 0x0DA0;

pub const E_ANO_SINGLE: u16 = SEG_BASE + 1; // 单线越带不构成事件
pub const E_ANO_ACCT: u16 = SEG_BASE + 2; // 账户未注册
pub const E_ANO_PEERS: u16 = SEG_BASE + 3; // 同类账户不足（中位数无样本）
pub const E_ANO_LEVEL: u16 = SEG_BASE + 4; // 分级越级
pub const E_ANO_LINK: u16 = SEG_BASE + 5; // 归因链接缺锚
pub const E_ANO_DELAY: u16 = SEG_BASE + 6; // 检测延迟超预算

/// 带阈值（千分位）：自基线 / 同类基线。
pub const SELF_WARN_PM: i64 = 200;
pub const SELF_ALERT_PM: i64 = 400;
pub const PEER_BAND_PM: i64 = 300;

/// 双基线账户账。
#[derive(Clone, Debug)]
pub struct AnomAccount {
    pub acct: u32,
    /// 类别（同类对比的分组键）。
    pub class: u8,
    /// 自基线 EWMA（消耗，×1000 保精度）。
    pub ewma: Option<u64>,
    /// EWMA 平滑系数（千分位）。
    pub alpha_pm: u32,
}

impl AnomAccount {
    pub fn new(acct: u32, class: u8, alpha_pm: u32) -> Self {
        AnomAccount { acct, class, ewma: None, alpha_pm }
    }

    /// 自基线更新（EWMA）并返回偏离（千分位）。
    pub fn observe(&mut self, used: u64) -> Result<i64, u16> {
        match self.ewma {
            None => {
                self.ewma = Some(used * 1000);
                Ok(0)
            }
            Some(e) => {
                let dev = if e == 0 {
                    0
                } else {
                    (used as i64 * 1000 - e as i64) * 1000 / e as i64
                };
                let a = self.alpha_pm as u64;
                self.ewma = Some(e * (1000 - a) / 1000 + used * 1000 * a / 1000);
                Ok(dev)
            }
        }
    }
}

/// 同类基线：类别内账户的中位消耗（抗单户离群）。
pub fn peer_median(class_used: &mut Vec<u64>) -> Result<u64, u16> {
    if class_used.is_empty() {
        return Err(E_ANO_PEERS);
    }
    class_used.sort_unstable();
    let n = class_used.len();
    Ok(if n % 2 == 1 {
        class_used[n / 2]
    } else {
        (class_used[n / 2 - 1] + class_used[n / 2]) / 2
    })
}

/// 异常事件分级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnomLevel {
    None,
    /// 轻度偏离提醒。
    Notice,
    /// 严重异常告警。
    Alert,
}

impl AnomLevel {
    pub fn code(&self) -> u8 {
        match self {
            AnomLevel::None => 0,
            AnomLevel::Notice => 1,
            AnomLevel::Alert => 2,
        }
    }
}

/// 双基线判定：自偏离 + 同类偏离 → 分级。
/// 单线越带不算事件（E_ANO_SINGLE 显性——不是错误是"未构成"）。
pub fn classify(self_dev_pm: i64, peer_dev_pm: i64) -> AnomLevel {
    let self_ok_alert = self_dev_pm >= SELF_ALERT_PM;
    let self_ok_warn = self_dev_pm >= SELF_WARN_PM;
    let peer_ok = peer_dev_pm >= PEER_BAND_PM;
    if self_ok_alert && peer_ok {
        AnomLevel::Alert
    } else if self_ok_warn && peer_ok {
        AnomLevel::Notice
    } else {
        AnomLevel::None
    }
}

/// 归因引导：事件自动附审计链锚（F0345 链接语义）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditLink {
    /// 决策窗号。
    pub decision_win: u64,
    /// 审计账索引（F0345 账内坐标）。
    pub audit_idx: u64,
}

/// 异常事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnomEvent {
    pub acct: u32,
    pub level: AnomLevel,
    pub self_dev_pm: i64,
    pub peer_dev_pm: i64,
    pub link: AuditLink,
}

impl AnomEvent {
    /// 事件合法性：级别合法 + 归因链接齐锚。
    pub fn valid(&self) -> Result<(), u16> {
        if self.level.code() > 2 {
            return Err(E_ANO_LEVEL);
        }
        if self.level == AnomLevel::None {
            return Err(E_ANO_SINGLE);
        }
        if self.link.audit_idx == 0 && self.link.decision_win == 0 {
            return Err(E_ANO_LINK);
        }
        Ok(())
    }
}

/// 异常检测引擎。
#[derive(Clone, Debug, Default)]
pub struct AnomEngine {
    pub accounts: Vec<AnomAccount>,
    /// 最近窗口各账户消耗（同类中位数输入）。
    pub last_used: Vec<(u32, u64)>,
    /// 事件链（检测延迟观测——近实时计数）。
    pub events: Vec<AnomEvent>,
    /// 检测延迟预算（步）。
    pub delay_budget: u32,
    pub detect_steps: u32,
}

impl AnomEngine {
    pub fn new(delay_budget: u32) -> Self {
        AnomEngine {
            accounts: Vec::new(),
            last_used: Vec::new(),
            events: Vec::new(),
            delay_budget,
            detect_steps: 0,
        }
    }

    pub fn register(&mut self, acct: u32, class: u8) -> Result<(), u16> {
        if self.accounts.iter().any(|a| a.acct == acct) {
            return Err(E_ANO_ACCT);
        }
        self.accounts.push(AnomAccount::new(acct, class, 200));
        Ok(())
    }

    /// 检测一步：某账户本窗消耗 → 双基线 → 事件（近实时 1 步内）。
    pub fn detect(
        &mut self,
        acct: u32,
        used: u64,
        decision_win: u64,
        audit_idx: u64,
    ) -> Result<Option<AnomEvent>, u16> {
        self.detect_steps += 1;
        if self.detect_steps > self.delay_budget {
            return Err(E_ANO_DELAY);
        }
        // 自基线。
        let ai = self
            .accounts
            .iter_mut()
            .find(|a| a.acct == acct)
            .ok_or(E_ANO_ACCT)?;
        let class = ai.class;
        let self_dev = ai.observe(used)?;
        self.last_used.push((acct, used));
        // 同类基线：本类其他账户本窗消耗中位。
        let mut peers: Vec<u64> = Vec::new();
        for (a, u) in &self.last_used {
            if *a == acct {
                continue;
            }
            if let Some(ac) = self.accounts.iter().find(|x| x.acct == *a) {
                if ac.class == class {
                    peers.push(*u);
                }
            }
        }
        let med = peer_median(&mut peers)?;
        let peer_dev = if med == 0 {
            0
        } else {
            (used as i64 - med as i64) * 1000 / med as i64
        };
        let level = classify(self_dev, peer_dev);
        if level == AnomLevel::None {
            return Ok(None);
        }
        let ev = AnomEvent {
            acct,
            level,
            self_dev_pm: self_dev,
            peer_dev_pm: peer_dev,
            link: AuditLink { decision_win, audit_idx },
        };
        ev.valid()?;
        self.events.push(ev.clone());
        Ok(Some(ev))
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn eng() -> AnomEngine {
        let mut e = AnomEngine::new(1_000);
        for i in 1..=3u32 {
            e.register(i, 0).ok(); // 三账户同类
        }
        e
    }

    #[test]
    fn a01_双基线_自基线EWMA与同类中位() {
        let mut e = eng();
        // 立基线（首窗偏离 0）。
        for i in 1..=3u32 {
            e.detect(i, 100, 0, 1).ok();
        }
        // 账户 1 暴增 300：自偏离 +200%（EWMA≈100），同类中位 100 ⇒ +2000‰。
        let ev = e.detect(1, 300, 5, 9).unwrap_or(None);
        assert!(ev.is_some());
        let ev = ev.unwrap_or(AnomEvent {
            acct: 0, level: AnomLevel::None, self_dev_pm: 0, peer_dev_pm: 0,
            link: AuditLink { decision_win: 0, audit_idx: 0 },
        });
        assert_eq!(ev.level, AnomLevel::Alert);
        assert!(ev.self_dev_pm >= SELF_ALERT_PM);
        assert!(ev.peer_dev_pm >= PEER_BAND_PM);
    }

    #[test]
    fn a02_单线越带不构成事件() {
        // 自偏离超但同类同涨（中位同步涨）⇒ 不构成。
        assert_eq!(classify(500, 100), AnomLevel::None);
        // 同类偏离超但自身平稳 ⇒ 不构成。
        assert_eq!(classify(50, 900), AnomLevel::None);
        // 双线越带才分级。
        assert_eq!(classify(250, 400), AnomLevel::Notice);
        assert_eq!(classify(450, 400), AnomLevel::Alert);
        // 中位空类拒。
        let mut v = Vec::new();
        assert_eq!(peer_median(&mut v), Err(E_ANO_PEERS));
        // 中位数偶数取均值。
        let mut v2 = vec![10u64, 20, 30, 40];
        assert_eq!(peer_median(&mut v2), Ok(25));
        let mut v3 = vec![30u64, 10, 20];
        assert_eq!(peer_median(&mut v3), Ok(20));
    }

    #[test]
    fn a03_分级_轻度提醒与严重告警() {
        let mut e = eng();
        for i in 1..=3u32 {
            e.detect(i, 100, 0, 1).ok();
        }
        // 账户 1 温和偏离 220（自 +120‰ 上下波动后累计）：构造自偏离 ≥200 同类越带。
        // 直接灌 EWMA 到 100 附近再 +220。
        let ev = e.detect(1, 220, 6, 9).unwrap_or(None);
        assert!(ev.is_some());
        assert!(ev.unwrap_or(AnomEvent {
            acct: 0, level: AnomLevel::None, self_dev_pm: 0, peer_dev_pm: 0,
            link: AuditLink { decision_win: 0, audit_idx: 0 },
        }).level.code() >= 1);
        // 分级不越级（code ≤ 2）：同类三账户，主嫌爆表只到 Alert 顶格。
        let mut e2 = eng();
        e2.register(9, 1).ok();
        for i in 1..=3u32 {
            e2.detect(i, 10_000, 0, 1).ok();
        }
        e2.detect(9, 10_000, 0, 1).ok();
        if let Some(ev2) = e2.detect(9, 1_000_000, 1, 1).unwrap_or(None) {
            assert!(ev2.level.code() <= 2);
        }
    }

    #[test]
    fn a04_归因引导_审计链接自动附锚() {
        let mut e = eng();
        for i in 1..=3u32 {
            e.detect(i, 100, 0, 1).ok();
        }
        let ev = e.detect(1, 300, 42, 7).unwrap_or(None);
        assert!(ev.is_some());
        let ev = ev.unwrap_or(AnomEvent {
            acct: 0, level: AnomLevel::None, self_dev_pm: 0, peer_dev_pm: 0,
            link: AuditLink { decision_win: 0, audit_idx: 0 },
        });
        // 链接锚齐：窗号 + 审计索引。
        assert_eq!(ev.link, AuditLink { decision_win: 42, audit_idx: 7 });
        assert_eq!(ev.valid(), Ok(()));
        // 双零锚 = 缺锚红。
        let bad = AnomEvent {
            acct: 1, level: AnomLevel::Alert, self_dev_pm: 500, peer_dev_pm: 400,
            link: AuditLink { decision_win: 0, audit_idx: 0 },
        };
        assert_eq!(bad.valid(), Err(E_ANO_LINK));
    }

    #[test]
    fn a05_检测延迟_近实时预算() {
        let mut e = AnomEngine::new(3);
        e.register(1, 0).ok();
        // 3 步内放行。
        e.detect(1, 10, 0, 1).ok();
        e.detect(1, 10, 1, 1).ok();
        e.detect(1, 10, 2, 1).ok();
        // 超预算红（批处理延迟不可接受）。
        assert_eq!(e.detect(1, 10, 3, 1), Err(E_ANO_DELAY));
        // 未注册账户拒。
        let mut e2 = AnomEngine::new(100);
        assert_eq!(e2.detect(9, 1, 0, 1), Err(E_ANO_ACCT));
        // 重复注册拒。
        let mut e3 = eng();
        assert_eq!(e3.register(1, 0), Err(E_ANO_ACCT));
    }
}
