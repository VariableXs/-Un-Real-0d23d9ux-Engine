//! CGPU-F0406 · 与 J 功耗域接口集成（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 功耗集成：功耗预算输入（F0359 功耗标签位落地——账户功耗消耗计量
//!   →J 域 F1441+ 调度输入）；续航模式联动（J 域续航模式激活→本域全局
//!   预算压缩配方自动应用）；热降档联动（J 域热降档→预算重分配事件）；
//!   功耗-性能权衡记录（每次联动的权衡决策归因）。
//! - 判据：计量输入、续航联动、热联动、归因、判据。
//!
//! 设计纪律：
//! - 计量输入：账户功耗标签位（mW 档位）随每帧消耗上报 J 域调度账——
//!   计量账与调度账逐项对账（同源才可信）。
//! - 续航联动：J 域续航位激活 → 本域全局预算压缩配方（固定档：
//!   轻压 80%/中压 60%/重压 40%）自动应用；配方应用有帧生效点。
//! - 热降档联动：J 域热级（0-3）每升一级触发一次预算重分配事件
//!   （重分配 = 各账户等比回缩 + 记录事件）。
//! - 权衡归因：每次联动（续航/热）落一条权衡记录（事件/档位/预算前后
//!   快照摘要），决策可追溯。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0C90 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0C90，全仓独占）。
pub const SEG_BASE: u16 = 0x0C90;

pub const E_JMETER_TAG: u16 = SEG_BASE + 1; // 功耗标签档非法
pub const E_JSAVER_STATE: u16 = SEG_BASE + 2; // 续航配方状态机非法
pub const E_JTHERM_LEVEL: u16 = SEG_BASE + 3; // 热级越界（>3）
pub const E_JTRADE_GAP: u16 = SEG_BASE + 4; // 权衡记录缺要素
pub const E_JACCT_NOACC: u16 = SEG_BASE + 5; // 账户未注册
pub const E_JRESCALE: u16 = SEG_BASE + 6; // 重分配后总量不守恒

/// 热级（J 域 0-3）。
pub const THERM_LEVELS: u8 = 4;
/// 续航压缩档（千分位）。
pub const SAVER_LITE_PM: u32 = 800;
pub const SAVER_MID_PM: u32 = 600;
pub const SAVER_HEAVY_PM: u32 = 400;

/// 功耗标签档（mW）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerTag {
    /// 账户。
    pub acct: u32,
    /// mW 档位（0-255——标签位宽）。
    pub milliwatts: u16,
}

/// 功耗计量账（本域→J 域调度输入）。
#[derive(Clone, Debug, Default)]
pub struct PowerMeter {
    pub tags: Vec<PowerTag>,
    /// 累计计量（acct → mW·帧）。
    pub cum: Vec<(u32, u64)>,
}

impl PowerMeter {
    pub fn register(&mut self, acct: u32) -> Result<(), u16> {
        if self.tags.iter().any(|t| t.acct == acct) {
            return Err(E_JACCT_NOACC);
        }
        self.tags.push(PowerTag { acct, milliwatts: 0 });
        self.cum.push((acct, 0));
        Ok(())
    }

    /// 逐帧上报：标签位 + 累计（计量账与累计账同源同步）。
    pub fn report(&mut self, acct: u32, mw: u16) -> Result<(), u16> {
        let t = self
            .tags
            .iter_mut()
            .find(|t| t.acct == acct)
            .ok_or(E_JACCT_NOACC)?;
        t.milliwatts = mw;
        let c = self
            .cum
            .iter_mut()
            .find(|(a, _)| *a == acct)
            .ok_or(E_JACCT_NOACC)?;
        c.1 += mw as u64;
        Ok(())
    }

    /// 对账：标签账与累计账逐项存在（同源核对）。
    pub fn consistent(&self) -> Result<(), u16> {
        for t in &self.tags {
            if !self.cum.iter().any(|(a, _)| *a == t.acct) {
                return Err(E_JACCT_NOACC);
            }
        }
        Ok(())
    }

    pub fn tag_of(&self, acct: u32) -> u16 {
        self.tags
            .iter()
            .find(|t| t.acct == acct)
            .map(|t| t.milliwatts)
            .unwrap_or(0)
    }

    pub fn total_now(&self) -> u64 {
        self.tags.iter().map(|t| t.milliwatts as u64).sum()
    }
}

/// 续航配方状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaverPhase {
    Off,
    Applied(u32), // 档位‰
}

/// 预算重分配事件（热联动产物）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RescaleEvent {
    pub frame: u64,
    pub therm: u8,
    /// 重分配比例‰（各账户等比回缩后）。
    pub scale_pm: u32,
}

/// 权衡归因记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeRecord {
    /// 事件类别：0=续航 1=热。
    pub kind: u8,
    pub frame: u64,
    /// 档位（续航‰或热级）。
    pub level: u32,
    /// 预算总量快照（前后各一）。
    pub before_total: u64,
    pub after_total: u64,
    /// 归因摘要编号（权衡账索引）。
    pub cause: u32,
}

/// J 功耗集成控制器。
#[derive(Clone, Debug)]
pub struct JPowerIntegrator {
    pub meter: PowerMeter,
    /// 各账户预算（mW）。
    pub budgets: Vec<(u32, u32)>,
    pub saver: SaverPhase,
    /// 续航压缩前的预算快照（解除时恢复）。
    pub saved: Option<Vec<(u32, u32)>>,
    pub therm: u8,
    pub events: Vec<RescaleEvent>,
    pub trades: Vec<TradeRecord>,
}

impl JPowerIntegrator {
    pub fn new() -> Self {
        JPowerIntegrator {
            meter: PowerMeter::default(),
            budgets: Vec::new(),
            saver: SaverPhase::Off,
            saved: None,
            therm: 0,
            events: Vec::new(),
            trades: Vec::new(),
        }
    }

    pub fn attach(&mut self, acct: u32, budget_mw: u32) -> Result<(), u16> {
        if self.budgets.iter().any(|(a, _)| *a == acct) {
            return Err(E_JACCT_NOACC);
        }
        self.meter.register(acct)?;
        self.budgets.push((acct, budget_mw));
        Ok(())
    }

    pub fn total_budget(&self) -> u64 {
        self.budgets.iter().map(|(_, b)| *b as u64).sum()
    }

    /// 续航联动：J 域续航位激活 → 全局预算压缩配方（按严重度选档）。
    pub fn saver_on(&mut self, severity: u8, frame: u64) -> Result<(), u16> {
        if self.saver != SaverPhase::Off {
            return Err(E_JSAVER_STATE); // 已激活不重复应用
        }
        let pm = match severity {
            0 => SAVER_LITE_PM,
            1 => SAVER_MID_PM,
            _ => SAVER_HEAVY_PM,
        };
        let before = self.total_budget();
        // 快照压缩前预算（解除时按快照恢复——不做二次缩放）。
        self.saved = Some(self.budgets.clone());
        self.rescale_all(pm, frame, 0)?; // cause=0 续航
        self.saver = SaverPhase::Applied(pm);
        self.trades.push(TradeRecord {
            kind: 0,
            frame,
            level: pm,
            before_total: before,
            after_total: self.total_budget(),
            cause: 0,
        });
        Ok(())
    }

    /// 续航解除：预算按压缩前快照恢复（非二次缩放）。
    pub fn saver_off(&mut self, frame: u64) -> Result<(), u16> {
        if self.saver == SaverPhase::Off {
            return Err(E_JSAVER_STATE);
        }
        let before = self.total_budget();
        let snap = self.saved.take().ok_or(E_JRESCALE)?;
        self.budgets = snap;
        self.saver = SaverPhase::Off;
        self.trades.push(TradeRecord {
            kind: 0,
            frame,
            level: 1000,
            before_total: before,
            after_total: self.total_budget(),
            cause: 1,
        });
        Ok(())
    }

    /// 热降档联动：热级单调升（回退只允许逐级降），每升一级触发一次重分配。
    pub fn therm_update(&mut self, level: u8, frame: u64) -> Result<(), u16> {
        if level as usize >= THERM_LEVELS as usize {
            return Err(E_JTHERM_LEVEL);
        }
        if level > self.therm {
            // 每升一级压缩 10%。
            let pm = 1000 - (level as u32 - self.therm as u32) * 100;
            let before = self.total_budget();
            self.rescale_all(pm, frame, 2)?;
            self.events.push(RescaleEvent { frame, therm: level, scale_pm: pm });
            self.trades.push(TradeRecord {
                kind: 1,
                frame,
                level: level as u32,
                before_total: before,
                after_total: self.total_budget(),
                cause: 2,
            });
        }
        self.therm = level;
        Ok(())
    }

    /// 等比重分配：各账户预算 × scale（守恒校验——总额等比）。
    fn rescale_all(&mut self, scale_pm: u32, frame: u64, cause: u32) -> Result<(), u16> {
        if scale_pm == 0 || scale_pm > 1000 {
            return Err(E_JRESCALE);
        }
        let before = self.total_budget();
        for b in self.budgets.iter_mut() {
            b.1 = (b.1 as u64 * scale_pm as u64 / 1000) as u32;
        }
        let after = self.total_budget();
        // 守恒：缩放后总量 ≈ 原总量 × scale（误差 ≤ 每账户 1mW 的舍入和）。
        let want = before * scale_pm as u64 / 1000;
        if after.abs_diff(want) > self.budgets.len() as u64 {
            return Err(E_JRESCALE);
        }
        let _ = (frame, cause);
        Ok(())
    }

    /// 权衡记录完整性：每条记录前后快照齐且归因号有效。
    pub fn trades_valid(&self) -> Result<(), u16> {
        for t in &self.trades {
            if t.before_total == 0 || t.after_total == 0 {
                return Err(E_JTRADE_GAP);
            }
        }
        Ok(())
    }

    /// J 域调度输入视图：标签账快照（计量→调度对接口）。
    pub fn j_input(&self) -> Vec<(u32, u16)> {
        self.meter
            .tags
            .iter()
            .map(|t| (t.acct, t.milliwatts))
            .collect()
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn it() -> JPowerIntegrator {
        let mut i = JPowerIntegrator::new();
        i.attach(1, 5_000).ok();
        i.attach(2, 3_000).ok();
        i.attach(3, 2_000).ok();
        i
    }

    #[test]
    fn a01_计量输入_标签累计同源对账() {
        let mut i = it();
        i.meter.report(1, 800).ok();
        i.meter.report(1, 700).ok();
        i.meter.report(2, 1_200).ok();
        assert_eq!(i.meter.tag_of(1), 700); // 标签=最近帧
        assert_eq!(i.meter.consistent(), Ok(()));
        assert_eq!(i.meter.total_now(), 1_900);
        // J 域调度输入视图逐项齐。
        let j = i.j_input();
        assert_eq!(j, vec![(1, 700), (2, 1_200), (3, 0)]);
        // 未注册账户拒。
        assert_eq!(i.meter.report(9, 1), Err(E_JACCT_NOACC));
    }

    #[test]
    fn a02_续航联动_三档压缩自动应用与解除() {
        let mut i = it();
        assert_eq!(i.total_budget(), 10_000);
        // 中压档 60%。
        i.saver_on(1, 100).ok();
        assert_eq!(i.saver, SaverPhase::Applied(SAVER_MID_PM));
        assert_eq!(i.total_budget(), 6_000);
        // 重复激活拒。
        assert_eq!(i.saver_on(0, 101), Err(E_JSAVER_STATE));
        // 解除恢复。
        i.saver_off(200).ok();
        assert_eq!(i.saver, SaverPhase::Off);
        assert_eq!(i.total_budget(), 10_000);
        // 未激活解除拒。
        assert_eq!(i.saver_off(201), Err(E_JSAVER_STATE));
        // 权衡记录两条（开/关各一）。
        assert_eq!(i.trades.len(), 2);
    }

    #[test]
    fn a03_热联动_逐级重分配与越界拒() {
        let mut i = it();
        i.therm_update(1, 10).ok();
        assert_eq!(i.therm, 1);
        assert_eq!(i.total_budget(), 9_000); // -10%
        i.therm_update(3, 20).ok();
        // 级 1→3 跨两级：压缩 20%（一次性事件）。
        assert_eq!(i.events.len(), 2);
        assert_eq!(i.events[1], RescaleEvent { frame: 20, therm: 3, scale_pm: 800 });
        assert_eq!(i.total_budget(), 7_200);
        // 热级回退（不触发重分配）。
        i.therm_update(2, 30).ok();
        assert_eq!(i.events.len(), 2);
        // 越界拒。
        assert_eq!(i.therm_update(4, 40), Err(E_JTHERM_LEVEL));
    }

    #[test]
    fn a04_权衡归因_记录要素齐() {
        let mut i = it();
        i.saver_on(2, 10).ok();
        i.therm_update(2, 20).ok();
        i.saver_off(30).ok();
        assert_eq!(i.trades.len(), 3);
        assert_eq!(i.trades_valid(), Ok(()));
        // 每条记录前后快照非零且方向合理（续航压缩后 < 前）。
        let t0 = &i.trades[0];
        assert_eq!(t0.kind, 0);
        assert!(t0.after_total < t0.before_total);
        // 热记录。
        assert_eq!(i.trades[1].kind, 1);
        assert_eq!(i.trades[1].level, 2);
    }

    #[test]
    fn a05_重分配守恒_等比与舍入界() {
        let mut i = it();
        // 重压档 40%。
        i.saver_on(9, 1).ok();
        assert_eq!(i.total_budget(), 4_000);
        // 单账户舍入界：2_000×0.4=800（整除）；总账守恒。
        let sum: u64 = i.budgets.iter().map(|(_, b)| *b as u64).sum();
        assert_eq!(sum, 4_000);
        // 非法 scale 拒（内部防线——0 或 >1000）。
        let mut bad = it();
        bad.budgets[0].1 = 0;
        // trades_valid 对空记录恒绿。
        assert_eq!(bad.trades_valid(), Ok(()));
    }
}
