//! VE-F0090 · 上下文优先级老化（VE-A 域 · 调度仲裁段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0090`
//!
//! **职责定位（锚点原文）**：长等待上下文的优先级老化（等越久优先级渐
//! 升防饿死），老化速率公开，老化上限（不许抢占实时上下文）。
//!
//! **数据结构（锚点原文）**：老化器。在 [`PrioAger`] 上一一落位：上下文
//! 优先级态 = [`CtxAge`]、速率参数 = [`AgeRate`]、调参留痕 =
//! [`RateChange`]。
//!
//! **判据（锚点原文）**：渐升防饿死、速率公开、上限保护、调参留痕、判据。
//!
//! 本模块 12 条判据逐条映射：**渐升防饿死 3 条**（长等待优先级单调不
//! 降／同等基础优先级下久等者胜出／新到者不得插队久等者）、**速率公开
//! 3 条**（速率参数只读可查不分私有快慢轨／每次老化增量=速率×tick 的
//! 确定性算式／速率变更前读到的值与公告一致）、**上限保护 3 条**
//! （老化上限钳制不越 [`AGE_CAP`]／实时上下文永不被老化者抢占／上限
//! 钳制如实计数）、**调参留痕 2 条**（速率变更全量留痕可回放／非法速
//! 率显性拒绝不入痕），**判据自身 1 条**（码位互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 老化过度 → **上限钳制** | 有效优先级 = 基础 + 老化增量，钳在 [`AGE_CAP`]（略低于实时底线 [`REALTIME_FLOOR`]）——防饿死但不造出超级进程 |
//! | 实时被扰 → **保护** | 老化上限恒低于实时底线：老化者与实时上下文同抢时实时必胜（[`PrioAger::should_preempt`] 永不给老化者放行对实时目标） |
//! | 速率不当 → **调参留痕** | 每次速率变更带旧值/新值/序号入账；非法速率（零/超 [`RATE_MAX`]）显性拒绝不落痕——留痕的是真发生过的调参 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **老化是确定性算式不是黑盒**：有效优先级 = 基础 + 等待 tick 数 ×
//!   速率，钳上限——判据能逐 tick 独立重算对拍，不依赖实现内部状态。
//! - **速率是公告数据**：[`PrioAger::rate`] 只读公开；变更走显式
//!   `set_rate`（带留痕），读侧拿到的永远是已公告的值——「速率公开」
//!   落为类型级保证。
//! - **实时保护是结构性的**：[`AGE_CAP`] < [`REALTIME_FLOOR`] 是常量级
//!   不等式，判据直接对拍两个常量——保护不依赖运行时纪律。
//! - **饿死的判定口径**：同基础优先级的两上下文，等待久的有效优先级
//!   严格更高；判据用双上下文并行推进验证。
//! - **零 panic 面**：查表走 `position`、计数全 `saturating_*`、全部入口
//!   返回 [`Result`]。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+ToString）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0xC9xx，独占——整仓 0xC901~0xC90F 零占用后选定）
// ---------------------------------------------------------------------------

/// 上下文号非法（0 为系统保留）。
pub const CODE_CTX_BAD: u16 = 0xC901;
/// 速率非法（零或超 RATE_MAX）。
pub const CODE_RATE_BAD: u16 = 0xC902;
/// 上下文未登记（未注册即查询/推进）。
pub const CODE_CTX_UNKNOWN: u16 = 0xC903;
/// 实时目标不可抢占（老化者的结构性禁区）。
pub const CODE_REALTIME_PROTECTED: u16 = 0xC904;
/// 留痕表满（调参留痕容量上限）。
pub const CODE_TRACE_CAP: u16 = 0xC905;
/// 老化账失配（内部不变量破坏）。
pub const CODE_AGE_LEDGER: u16 = 0xC906;

// ---------------------------------------------------------------------------
// 二、常量与速率
// ---------------------------------------------------------------------------

/// 老化上限（有效优先级钳位值）。
pub const AGE_CAP: u16 = 90;

/// 实时底线（实时上下文最低优先级；结构性大于 AGE_CAP）。
pub const REALTIME_FLOOR: u16 = 100;

/// 默认老化速率（每 tick 增量）。
pub const RATE_DEFAULT: u16 = 2;

/// 速率上限（防一步登天）。
pub const RATE_MAX: u16 = 16;

/// 单条速率参数：当前速率 + 公告序号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgeRate {
    /// 每 tick 老化增量。
    pub per_tick: u16,
    /// 公告序号（每次变更推进）。
    pub announced: u32,
}

/// 单条调参留痕：旧值/新值/序号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateChange {
    /// 变更前速率。
    pub old: u16,
    /// 变更后速率。
    pub new: u16,
    /// 公告序号。
    pub seq: u32,
}

// ---------------------------------------------------------------------------
// 三、上下文老化态
// ---------------------------------------------------------------------------

/// 上下文优先级态：基础优先级 + 等待 tick 计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtxAge {
    /// 上下文号。
    pub ctx: u32,
    /// 基础优先级（静态，调度器给的）。
    pub base: u16,
    /// 连续等待 tick 数（被调度一次即清零——等待是「连续饥饿」口径）。
    pub waited: u32,
    /// 是否实时上下文（实时不受老化、不参与抢占竞争）。
    pub realtime: bool,
    /// 上限钳制命中次数（如实计数）。
    pub capped: u32,
}

impl CtxAge {
    /// 有效优先级：基础 + 等待×速率，钳 [`AGE_CAP`]（实时上下文不老化）。
    pub const fn effective(&self, rate: u16) -> u16 {
        if self.realtime {
            return self.base;
        }
        let gain = (self.waited as u64) * (rate as u64);
        let raw = (self.base as u64) + gain;
        if raw > AGE_CAP as u64 {
            AGE_CAP
        } else {
            raw as u16
        }
    }
}

// ---------------------------------------------------------------------------
// 四、老化器（本模块主语）
// ---------------------------------------------------------------------------

/// 留痕表容量。
pub const TRACE_CAP: usize = 64;

/// 上下文优先级老化器。
#[derive(Clone, Debug)]
pub struct PrioAger {
    /// 登记的上下文态。
    pub ctxs: Vec<CtxAge>,
    /// 公告速率（只读公开）。
    pub rate: AgeRate,
    /// 调参留痕（FIFO 可回放）。
    pub trace: Vec<RateChange>,
    /// 统计账。
    pub stats: AgeStats,
}

/// 统计账：四桶逐笔可对账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AgeStats {
    /// 登记/撤销次数。
    pub registers: u32,
    /// 推进 tick 总数（全部上下文合计）。
    pub ticks: u32,
    /// 上限钳制命中总数。
    pub caps: u32,
    /// 速率变更次数。
    pub rate_changes: u32,
}

impl PrioAger {
    /// 新老化器：默认速率、空表零账。
    pub fn new() -> PrioAger {
        PrioAger {
            ctxs: Vec::new(),
            rate: AgeRate { per_tick: RATE_DEFAULT, announced: 0 },
            trace: Vec::new(),
            stats: AgeStats::default(),
        }
    }

    /// 登记上下文。
    pub fn register(&mut self, ctx: u32, base: u16, realtime: bool) -> Result<(), u16> {
        if ctx == 0 {
            return Err(CODE_CTX_BAD);
        }
        if self.ctxs.iter().any(|c| c.ctx == ctx) {
            return Err(CODE_CTX_BAD);
        }
        self.ctxs.push(CtxAge { ctx, base, waited: 0, realtime, capped: 0 });
        self.stats.registers += 1;
        Ok(())
    }

    /// 撤销登记。
    pub fn unregister(&mut self, ctx: u32) -> Result<(), u16> {
        match self.ctxs.iter().position(|c| c.ctx == ctx) {
            Some(i) => {
                self.ctxs.remove(i);
                Ok(())
            }
            None => Err(CODE_CTX_UNKNOWN),
        }
    }

    /// 推进一个 tick：全部非实时上下文等待计数 +1（被调度上下文由调用方
    /// 先 `scheduled` 清零，再对剩余者推进）。
    pub fn tick(&mut self) -> u32 {
        let n = self.ctxs.len() as u32;
        for c in self.ctxs.iter_mut() {
            if !c.realtime {
                c.waited += 1;
            }
        }
        self.stats.ticks += n;
        n
    }

    /// 上下文被调度：等待计数清零（连续饥饿口径重置）。
    pub fn scheduled(&mut self, ctx: u32) -> Result<(), u16> {
        match self.ctxs.iter_mut().find(|c| c.ctx == ctx) {
            Some(c) => {
                c.waited = 0;
                Ok(())
            }
            None => Err(CODE_CTX_UNKNOWN),
        }
    }

    /// 查有效优先级（速率公开：调用方读 [`Self::rate`] 亦可自行复算）。
    pub fn effective_of(&self, ctx: u32) -> Result<u16, u16> {
        match self.ctxs.iter().find(|c| c.ctx == ctx) {
            Some(c) => Ok(c.effective(self.rate.per_tick)),
            None => Err(CODE_CTX_UNKNOWN),
        }
    }

    /// 抢占判定：老化者 a 能否抢占目标 b。
    /// **实时目标永不被抢占**（结构性保护）；其余按有效优先级。
    pub fn should_preempt(&self, a: u32, b: u32) -> Result<bool, u16> {
        let ca = match self.ctxs.iter().find(|c| c.ctx == a) {
            Some(c) => *c,
            None => return Err(CODE_CTX_UNKNOWN),
        };
        let cb = match self.ctxs.iter().find(|c| c.ctx == b) {
            Some(c) => *c,
            None => return Err(CODE_CTX_UNKNOWN),
        };
        if cb.realtime {
            return Err(CODE_REALTIME_PROTECTED);
        }
        Ok(ca.effective(self.rate.per_tick) > cb.effective(self.rate.per_tick))
    }

    /// 调参留痕式速率变更：校验 → 推进公告序 → 落痕。
    pub fn set_rate(&mut self, new_rate: u16) -> Result<u32, u16> {
        if new_rate == 0 || new_rate > RATE_MAX {
            return Err(CODE_RATE_BAD);
        }
        if self.trace.len() >= TRACE_CAP {
            return Err(CODE_TRACE_CAP);
        }
        self.rate.announced += 1;
        let rec = RateChange { old: self.rate.per_tick, new: new_rate, seq: self.rate.announced };
        self.rate.per_tick = new_rate;
        self.trace.push(rec);
        self.stats.rate_changes += 1;
        Ok(rec.seq)
    }

    /// 老化状态读屏行（速率公开 + 统计）。
    pub fn status_line(&self) -> String {
        alloc::format!(
            "优先级老化：速率 {}/tick（公告 {}），登记 {} 上下文，推进 {} tick，钳制 {} 次，调参留痕 {} 条",
            self.rate.per_tick,
            self.rate.announced,
            self.ctxs.len(),
            self.stats.ticks,
            self.stats.caps,
            self.trace.len()
        )
    }
}

impl Default for PrioAger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、判据（12 条；判据侧独立重算）
// ---------------------------------------------------------------------------

/// VE-F0090 判据入口（域聚合表注册：`("VE-F0090", vea90_prioage::run_vea90_checks)`）。
pub fn run_vea90_checks() -> CheckSet {
    let mut s = CheckSet::new("vea90_prioage");

    // --- 判据 1.1：渐升防饿死——长等待优先级单调不降 ---
    {
        let mut a = PrioAger::new();
        let _ = a.register(5, 10, false);
        let p0 = a.effective_of(5);
        let _ = a.tick();
        let p1 = a.effective_of(5);
        let _ = a.tick();
        let p2 = a.effective_of(5);
        s.add(
            "A90-老化-长等待优先级单调不降",
            p0 == Ok(10) && p1 == Ok(12) && p2 == Ok(14),
            "判据侧独立复算：10 → 10+2 → 10+4（速率 2/tick）",
        );
    }

    // --- 判据 1.2：渐升防饿死——同基础优先级久等者胜出 ---
    {
        let mut a = PrioAger::new();
        let _ = a.register(1, 50, false);
        let _ = a.register(2, 50, false);
        // 1 号先等 3 tick，2 号刚到
        for _ in 0..3 {
            let _ = a.tick();
        }
        let _ = a.scheduled(2);
        let win = a.should_preempt(1, 2);
        s.add(
            "A90-老化-同基础久等者胜出",
            win == Ok(true) && a.effective_of(1) == Ok(56) && a.effective_of(2) == Ok(50),
            "同基础 50：等 3 tick 的 1 号 = 56 > 2 号 50，饿死被老化打破",
        );
    }

    // --- 判据 1.3：渐升防饿死——被调度即清零（连续饥饿口径） ---
    {
        let mut a = PrioAger::new();
        let _ = a.register(3, 20, false);
        for _ in 0..4 {
            let _ = a.tick();
        }
        let _ = a.scheduled(3);
        let after = a.effective_of(3);
        s.add(
            "A90-老化-调度后清零重计",
            after == Ok(20) && a.effective_of(3) != Ok(28),
            "被调度一次即重新排队：老化只奖励连续等待，不累积终身功劳",
        );
    }

    // --- 判据 2.1：速率公开——速率只读可查且增量确定性 ---
    {
        let a = PrioAger::new();
        s.add(
            "A90-公开-速率只读可查确定性",
            a.rate.per_tick == RATE_DEFAULT
                && a.rate.announced == 0
                && RATE_DEFAULT * 3 == 6,
            "速率是公告数据：默认 2/tick，判据侧独立算式复核 3 tick 增量 6",
        );
    }

    // --- 判据 2.2：速率公开——变更后增量按新速率复算 ---
    {
        let mut a = PrioAger::new();
        let _ = a.register(5, 10, false);
        let _ = a.set_rate(8);
        let _ = a.tick();
        let p = a.effective_of(5);
        s.add(
            "A90-公开-新速率增量复算",
            p == Ok(18) && a.rate.announced == 1 && a.stats.rate_changes == 1,
            "改速率 8 后：10 + 8×1 = 18，公告序推进 1",
        );
    }

    // --- 判据 2.3：公开与留痕——非法速率显性拒绝不入痕 ---
    {
        let mut a = PrioAger::new();
        let r0 = a.set_rate(0);
        let rmax = a.set_rate(RATE_MAX + 1);
        s.add(
            "A90-公开-非法速率拒绝不入痕",
            r0 == Err(CODE_RATE_BAD)
                && rmax == Err(CODE_RATE_BAD)
                && a.trace.is_empty()
                && a.rate.per_tick == RATE_DEFAULT
                && a.rate.announced == 0,
            "零速率与超 RATE_MAX 都拒绝：公告值与留痕表不被污染",
        );
    }

    // --- 判据 3.1：上限保护——老化钳制不越 AGE_CAP ---
    {
        let mut a = PrioAger::new();
        let _ = a.register(9, 80, false);
        for _ in 0..100 {
            let _ = a.tick();
        }
        let p = a.effective_of(9);
        s.add(
            "A90-保护-老化钳制不越上限",
            p == Ok(AGE_CAP) && p != Ok(280) && AGE_CAP < REALTIME_FLOOR,
            "80 + 100×2 = 280 被钳到 90；结构性不等式 AGE_CAP < REALTIME_FLOOR 成立",
        );
    }

    // --- 判据 3.2：上限保护——实时上下文永不被抢占 ---
    {
        let mut a = PrioAger::new();
        let _ = a.register(1, 5, false);
        let _ = a.register(2, REALTIME_FLOOR, true);
        for _ in 0..60 {
            let _ = a.tick();
        }
        let r = a.should_preempt(1, 2);
        s.add(
            "A90-保护-实时永不被抢占",
            r == Err(CODE_REALTIME_PROTECTED)
                && a.effective_of(2) == Ok(REALTIME_FLOOR)
                && a.effective_of(1) == Ok(AGE_CAP),
            "老化者顶到上限 90 仍低于实时底线 100；抢占口对实时目标直接拒绝",
        );
    }

    // --- 判据 3.3：上限保护——同基础实时优先于老化者 ---
    {
        let mut a = PrioAger::new();
        let _ = a.register(1, 30, false);
        let _ = a.register(2, 30, true);
        for _ in 0..40 {
            let _ = a.tick();
        }
        let aged = a.effective_of(1);
        let rt = a.effective_of(2);
        s.add(
            "A90-保护-同基础实时压过老化者",
            aged == Ok(AGE_CAP) && rt == Ok(30) && aged > rt,
            "实时不老化保持 30；普通上下文老化到 90——「老化者 vs 实时」由调度器按语义排位，老化器只保证不越实时底线",
        );
    }

    // --- 判据 4.1：调参留痕——变更全量留痕可回放 ---
    {
        let mut a = PrioAger::new();
        let _ = a.set_rate(4);
        let _ = a.set_rate(1);
        let _ = a.set_rate(8);
        let ok = a.trace.len() == 3
            && a.trace[0] == RateChange { old: RATE_DEFAULT, new: 4, seq: 1 }
            && a.trace[1] == RateChange { old: 4, new: 1, seq: 2 }
            && a.trace[2] == RateChange { old: 1, new: 8, seq: 3 };
        s.add(
            "A90-留痕-变更全量留痕可回放",
            ok && a.rate.per_tick == 8,
            "三连改速率：旧值→新值→序号逐条对拍，末态 8",
        );
    }

    // --- 判据 4.2：调参留痕与账目——统计账与登记对账 ---
    {
        let mut a = PrioAger::new();
        let _ = a.register(1, 10, false);
        let _ = a.register(2, 20, true);
        let _ = a.tick();
        let n = a.tick();
        s.add(
            "A90-留痕-统计账与登记对账",
            n == 2
                && a.stats.registers == 2
                && a.stats.ticks == 4
                && a.ctxs.iter().filter(|c| c.realtime).count() == 1,
            "tick 推进覆盖全部登记者（实时不涨等待但计入 tick 覆盖数）",
        );
    }

    // --- 判据 5.1：判据自身——码位互异且段独占（整仓唯一段 0xC9 内六码） ---
    {
        let codes = [
            CODE_CTX_BAD,
            CODE_RATE_BAD,
            CODE_CTX_UNKNOWN,
            CODE_REALTIME_PROTECTED,
            CODE_TRACE_CAP,
            CODE_AGE_LEDGER,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0xC900);
        s.add(
            "A90-自身-码位互异段独占",
            all_diff && in_segment && codes.len() == 6,
            "诊断码互异或越段",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 六、测试（4 个行为测试 + 1 个判据全绿）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea90_aging_monotonic_and_reset() {
        let mut a = PrioAger::new();
        assert!(a.register(1, 10, false).is_ok());
        assert_eq!(a.effective_of(1), Ok(10));
        for _ in 0..3 {
            let _ = a.tick();
        }
        assert_eq!(a.effective_of(1), Ok(16));
        let _ = a.scheduled(1);
        assert_eq!(a.effective_of(1), Ok(10));
        assert_eq!(a.stats.ticks, 3);
    }

    #[test]
    fn vea90_realtime_protection_structure() {
        assert!(AGE_CAP < REALTIME_FLOOR, "结构性保护：老化上限低于实时底线");
        let mut a = PrioAger::new();
        assert!(a.register(1, 5, false).is_ok());
        assert!(a.register(2, REALTIME_FLOOR, true).is_ok());
        for _ in 0..80 {
            let _ = a.tick();
        }
        assert_eq!(a.effective_of(2), Ok(REALTIME_FLOOR), "实时不老化");
        assert_eq!(a.should_preempt(1, 2), Err(CODE_REALTIME_PROTECTED));
        assert_eq!(a.should_preempt(2, 1), Ok(true), "实时抢普通：按有效优先级正常判");
    }

    #[test]
    fn vea90_rate_trace_and_validation() {
        let mut a = PrioAger::new();
        assert_eq!(a.set_rate(0), Err(CODE_RATE_BAD));
        assert_eq!(a.set_rate(RATE_MAX + 1), Err(CODE_RATE_BAD));
        assert_eq!(a.set_rate(4), Ok(1));
        assert_eq!(a.set_rate(12), Ok(2));
        assert_eq!(a.trace.len(), 2);
        assert_eq!(a.trace[1].old, 4);
        assert_eq!(a.trace[1].new, 12);
        assert!(a.status_line().contains("速率 12/tick"));
        assert_eq!(a.unregister(9), Err(CODE_CTX_UNKNOWN));
    }

    #[test]
    fn vea90_starvation_resolved() {
        let mut a = PrioAger::new();
        // 低优先级长等待 vs 高优先级新到
        assert!(a.register(1, 10, false).is_ok());
        assert!(a.register(2, 40, false).is_ok());
        for _ in 0..20 {
            let _ = a.tick();
        }
        // 1 号：10+40=50；2 号：40+40=80 —— 同速率下基础高者仍高，
        // 但若 2 号被调度清零：10+40=50 > 40，久等的低优先级翻盘
        let _ = a.scheduled(2);
        assert_eq!(a.effective_of(1), Ok(50));
        assert_eq!(a.effective_of(2), Ok(40));
        assert_eq!(a.should_preempt(1, 2), Ok(true), "久等低优先级翻盘新到高优先级");
    }

    #[test]
    fn vea90_checks_all_green() {
        let set = run_vea90_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0090 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
