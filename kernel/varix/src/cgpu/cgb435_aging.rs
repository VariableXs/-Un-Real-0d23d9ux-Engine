//! CGPU-F0435 · 优先级老化机制（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 老化：aging 机制（长等待任务优先级渐升——防饿死的仲裁侧深化）；
//!   老化曲线（升速可配——线性/阶梯）；老化上限（升至多少封顶——防
//!   老化反转）；老化显性（每次提升记录——可审计）。判据：渐升、曲线
//!   可配、封顶、显性、判据。
//!
//! 设计纪律：
//! - 老化 = 等待时间→优先级增益的映射：boost = f(waited)，
//!   有效优先级 = base + boost（clamp 到封顶）。只升不降（等待不减
//!   已得增益——老化是积累不是波动，防「快到顶又被拉回」的振荡饿死）。
//! - 曲线两档：Linear（每 per_ticks 步 +1）与 Step（每 every 步
//!   +bump）——切换曲线后 boost 重算一致（同 waited 同 boost，
//!   曲线只定义映射不携带状态）。
//! - 封顶：boost ≤ CAP_BOOST，且 cap 变更即时 clamp 已得增益
//!   （封顶收紧行为立即生效——防老化反转即防 boost 超过 cap）。
//! - 显性：每次 boost 实际提升（含曲线切换导致的重算变化）记
//!   (tick, from, to) 审计行——审计链与当前 boost 对得上。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0E60 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0E60，全仓独占）。
pub const SEG_BASE: u16 = 0x0E60;

pub const E_AG_TASK: u16 = SEG_BASE + 1; // 任务非法（未登记）
pub const E_AG_CURVE: u16 = SEG_BASE + 2; // 曲线参数非法（0 步长）
pub const E_AG_CAP: u16 = SEG_BASE + 3; // 超封顶（增益突破 cap）
pub const E_AG_PRIO: u16 = SEG_BASE + 4; // 基础优先级越界
pub const E_AG_LOG: u16 = SEG_BASE + 5; // 审计链断裂（记录与现状不符）

/// 老化封顶（增益上限——防老化反转）。
pub const CAP_BOOST: u8 = 6;

/// 优先级值域上界（含封顶后最大有效级）。
pub const PRIO_MAX: u8 = 31;

/// 老化曲线（映射 waited → boost；无状态可换）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgingCurve {
    /// 线性：每 per_ticks 步 +1。
    Linear { per_ticks: u64 },
    /// 阶梯：每 every 步 +bump。
    Step { every: u64, bump: u8 },
}

impl AgingCurve {
    /// 映射：waited → 未封顶增益。
    pub fn boost_of(&self, waited: u64) -> u8 {
        match *self {
            AgingCurve::Linear { per_ticks } => {
                if per_ticks == 0 {
                    0
                } else {
                    (waited / per_ticks).min(CAP_BOOST as u64) as u8
                }
            }
            AgingCurve::Step { every, bump } => {
                if every == 0 || bump == 0 {
                    0
                } else {
                    let steps = waited / every;
                    // saturating_mul 防溢出（bump 大步长巨时）。
                    let raw = (steps as u128 * bump as u128) as u64;
                    raw.min(CAP_BOOST as u64) as u8
                }
            }
        }
    }

    /// 参数合法性。
    pub fn valid(&self) -> bool {
        match *self {
            AgingCurve::Linear { per_ticks } => per_ticks > 0,
            AgingCurve::Step { every, bump } => every > 0 && bump > 0,
        }
    }
}

/// 老化任务。
#[derive(Clone, Debug)]
pub struct AgingTask {
    /// 任务 ID。
    pub id: u64,
    /// 基础优先级。
    pub base: u8,
    /// 已等待 tick 数。
    pub waited: u64,
    /// 当前增益（≤ cap）。
    pub boost: u8,
}

impl AgingTask {
    /// 有效优先级（封顶双保险：base+boost 再 clamp 到 PRIO_MAX）。
    pub fn effective(&self) -> u8 {
        self.base.saturating_add(self.boost).min(PRIO_MAX)
    }
}

/// 审计行：第 tick 时 boost 从 from 提到 to。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgingAudit {
    pub tick: u64,
    pub id: u64,
    pub from: u8,
    pub to: u8,
}

/// 老化器。
#[derive(Debug)]
pub struct Ager {
    curve: AgingCurve,
    cap: u8,
    tasks: Vec<AgingTask>,
    audit: Vec<AgingAudit>,
    now: u64,
}

impl Default for Ager {
    fn default() -> Self {
        // 公称曲线与硬顶必然合法（参数校验只拒 0 步长与 cap 越硬顶）。
        Self {
            curve: AgingCurve::Linear { per_ticks: 10 },
            cap: CAP_BOOST,
            tasks: Vec::new(),
            audit: Vec::new(),
            now: 0,
        }
    }
}

impl Ager {
    /// 构造：曲线参数非法或 cap 越界拒。
    pub fn new(curve: AgingCurve, cap: u8) -> Result<Self, u16> {
        if !curve.valid() {
            return Err(E_AG_CURVE);
        }
        if cap > CAP_BOOST {
            return Err(E_AG_CAP);
        }
        Ok(Self {
            curve,
            cap,
            tasks: Vec::new(),
            audit: Vec::new(),
            now: 0,
        })
    }

    /// 登记任务（基础优先级越界拒）。
    pub fn enroll(&mut self, id: u64, base: u8) -> Result<(), u16> {
        if base > PRIO_MAX {
            return Err(E_AG_PRIO);
        }
        if self.tasks.iter().any(|t| t.id == id) {
            return Err(E_AG_TASK);
        }
        self.tasks.push(AgingTask {
            id,
            base,
            waited: 0,
            boost: 0,
        });
        Ok(())
    }

    /// 推进 n tick：等待累积 + 增益重算（只升不降 + 封顶）+ 显性审计。
    pub fn tick(&mut self, n: u64) {
        self.now = self.now.saturating_add(n);
        for t in self.tasks.iter_mut() {
            t.waited = t.waited.saturating_add(n);
            let want = self.curve.boost_of(t.waited).min(self.cap);
            if want > t.boost {
                let from = t.boost;
                t.boost = want;
                self.audit.push(AgingAudit {
                    tick: self.now,
                    id: t.id,
                    from,
                    to: want,
                });
            }
        }
    }

    /// 换曲线：参数非法拒；换后全部任务按新映射重算（只升不降——
    /// 新映射更慢时已得增益保留，不回退）。
    pub fn set_curve(&mut self, curve: AgingCurve) -> Result<(), u16> {
        if !curve.valid() {
            return Err(E_AG_CURVE);
        }
        self.curve = curve;
        for t in self.tasks.iter_mut() {
            let want = curve.boost_of(t.waited).min(self.cap).max(t.boost);
            if want > t.boost {
                let from = t.boost;
                t.boost = want;
                self.audit.push(AgingAudit {
                    tick: self.now,
                    id: t.id,
                    from,
                    to: want,
                });
            }
        }
        Ok(())
    }

    /// 收紧封顶：即时 clamp 已得增益（防反转——现状立即合法）。
    /// 被 clamp 的任务记审计行（显性纪律：任何变化都有记录）。
    pub fn tighten_cap(&mut self, cap: u8) -> Result<(), u16> {
        if cap > CAP_BOOST {
            return Err(E_AG_CAP);
        }
        self.cap = cap;
        for t in self.tasks.iter_mut() {
            let clamped = t.boost.min(cap);
            if clamped != t.boost {
                let from = t.boost;
                t.boost = clamped;
                self.audit.push(AgingAudit {
                    tick: self.now,
                    id: t.id,
                    from,
                    to: clamped,
                });
            }
        }
        Ok(())
    }

    /// 有效优先级查询。
    pub fn prio_of(&self, id: u64) -> Result<u8, u16> {
        self.tasks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.effective())
            .ok_or(E_AG_TASK)
    }

    /// 审计链一致性：每任务当前 boost == 审计链最后一次 to（有提升史时）。
    pub fn audit_consistent(&self) -> Result<(), u16> {
        for t in &self.tasks {
            let last = self.audit.iter().filter(|a| a.id == t.id).last();
            if let Some(a) = last {
                if a.to != t.boost {
                    return Err(E_AG_LOG);
                }
            } else if t.boost != 0 {
                return Err(E_AG_LOG); // 有增益却无审计 = 悄悄改。
            }
        }
        Ok(())
    }

    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    pub fn waited_of(&self, id: u64) -> Result<u64, u16> {
        self.tasks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.waited)
            .ok_or(E_AG_TASK)
    }
}

// ==========================================================================
// 判据（施工书：渐升、曲线可配、封顶、显性、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_线性渐升_防饿死() {
        let mut a = Ager::new(AgingCurve::Linear { per_ticks: 10 }, CAP_BOOST).unwrap();
        a.enroll(1, 5).unwrap();
        // 前 9 tick：无增益。
        a.tick(9);
        assert_eq!(a.prio_of(1), Ok(5));
        // 第 10 tick：+1。
        a.tick(1);
        assert_eq!(a.prio_of(1), Ok(6));
        // 到 50 tick：+5。
        a.tick(40);
        assert_eq!(a.prio_of(1), Ok(10));
        // 未登记任务查询拒。
        assert_eq!(a.prio_of(99), Err(E_AG_TASK));
        // 重复登记拒。
        assert_eq!(a.enroll(1, 1), Err(E_AG_TASK));
    }

    #[test]
    fn a02_曲线可配_线性与阶梯() {
        // 阶梯：每 20 步 +2。
        let c = AgingCurve::Step { every: 20, bump: 2 };
        assert_eq!(c.boost_of(19), 0);
        assert_eq!(c.boost_of(20), 2);
        assert_eq!(c.boost_of(60), 6);
        // 非法曲线拒。
        assert!(matches!(Ager::new(AgingCurve::Linear { per_ticks: 0 }, CAP_BOOST), Err(E_AG_CURVE)));
        assert!(matches!(Ager::new(AgingCurve::Step { every: 0, bump: 1 }, CAP_BOOST), Err(E_AG_CURVE)));
        assert!(matches!(Ager::new(AgingCurve::Step { every: 5, bump: 0 }, CAP_BOOST), Err(E_AG_CURVE)));
        // 运行中换曲线：同 waited 同映射（只升不降——新曲线更慢不回退）。
        let mut a = Ager::new(AgingCurve::Linear { per_ticks: 10 }, CAP_BOOST).unwrap();
        a.enroll(2, 0).unwrap();
        a.tick(30);
        assert_eq!(a.prio_of(2), Ok(3));
        // 换成更慢的曲线：增益保留 3（不降到 1）。
        a.set_curve(AgingCurve::Linear { per_ticks: 100 }).unwrap();
        assert_eq!(a.prio_of(2), Ok(3));
        // 换成更快的曲线：立即补齐。
        a.set_curve(AgingCurve::Step { every: 10, bump: 6 }).unwrap();
        assert_eq!(a.prio_of(2), Ok(6));
    }

    #[test]
    fn a03_封顶_防老化反转() {
        let mut a = Ager::new(AgingCurve::Linear { per_ticks: 10 }, 4).unwrap();
        a.enroll(3, PRIO_MAX - 3).unwrap();
        a.tick(100);
        // cap=4：增益封 4，有效级 clamp 到 PRIO_MAX。
        assert_eq!(a.prio_of(3), Ok(PRIO_MAX));
        // 收紧封顶：即时 clamp。
        a.tighten_cap(2).unwrap();
        assert_eq!(a.waited_of(3), Ok(100));
        let boost_now = a.prio_of(3).unwrap_or(0) - (PRIO_MAX - 3).min(PRIO_MAX);
        assert_eq!(boost_now, 2);
        // 超硬顶拒。
        assert_eq!(a.tighten_cap(CAP_BOOST + 1), Err(E_AG_CAP));
        assert!(matches!(Ager::new(AgingCurve::Linear { per_ticks: 1 }, CAP_BOOST + 1), Err(E_AG_CAP)));
        // clamp 后继续 tick：不超新 cap。
        a.tick(50);
        assert_eq!(a.audit_consistent(), Ok(()));
    }

    #[test]
    fn a04_显性审计_每次提升有记录() {
        let mut a = Ager::new(AgingCurve::Linear { per_ticks: 10 }, CAP_BOOST).unwrap();
        a.enroll(4, 0).unwrap();
        a.tick(30);
        // 批量推进一次到位：boost 0→3 是本 tick 的实际变化，1 条审计行。
        assert_eq!(a.audit_len(), 1);
        // 审计链与现状一致。
        assert_eq!(a.audit_consistent(), Ok(()));
        // 无增益任务无审计行也一致。
        a.enroll(5, 1).unwrap();
        assert_eq!(a.audit_consistent(), Ok(()));
        // 篡改现状（模拟悄悄改）：链断红。
        a.tasks[0].boost = 0;
        assert_eq!(a.audit_consistent(), Err(E_AG_LOG));
        // 恢复后一致。
        a.tasks[0].boost = 3;
        assert_eq!(a.audit_consistent(), Ok(()));
        // 审计行内容：from/to 单调衔接。
        let rows: Vec<(u8, u8)> = a.audit.iter().filter(|r| r.id == 4).map(|r| (r.from, r.to)).collect();
        assert_eq!(rows, alloc::vec![(0, 3)]);
    }

    #[test]
    fn a05_优先级越界与登记卫生() {
        let mut a = Ager::default();
        // 基础优先级越界拒。
        assert_eq!(a.enroll(9, PRIO_MAX + 1), Err(E_AG_PRIO));
        assert_eq!(a.task_count(), 0);
        // 边界值放行。
        assert_eq!(a.enroll(9, PRIO_MAX), Ok(()));
        assert_eq!(a.task_count(), 1);
        // base 已到顶：增益照涨（审计如实记录），有效级 clamp 到 PRIO_MAX。
        a.tick(1000);
        assert_eq!(a.prio_of(9), Ok(PRIO_MAX));
        assert_eq!(a.audit_len(), 1); // boost 0→6 有据可查。
        assert_eq!(a.audit_consistent(), Ok(()));
    }
}
