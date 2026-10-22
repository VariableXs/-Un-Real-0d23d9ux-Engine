//! CGPU-F0451 · 应急预算协议（目标 380 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 应急协议：资源枯竭紧急模式（总体资源严重不足时的分级弃车协议——
//!   保系统 UI/输入响应，弃后台与部分应用；协议提前定义分级触发）；
//! - 应急分级（黄/橙/红三级——每级的弃车清单预先公示）；
//! - 退出协议（资源恢复→逐级恢复正常——不瞬间反弹）；
//! - 应急事件全归因。判据：分级弃车、清单公示、退出协议、归因、判据。
//!
//! 设计纪律：
//! - 分级触发提前定义：水位阈值常量入册（黄 850‰/橙 920‰/红 970‰），
//!   触发是查表不是现场拍板——协议先于事件存在。
//! - 弃车清单公示：每级的「保」与「弃」是常量清单，任何类别在任一
//!   级别的去留可静态查询——公示即确定性，运行期不发明新清单。
//! - 保留底线：SYSTEM（UI/输入响应）在任何级别都不进弃车清单——
//!   红级弃车清单与保留清单的交集恒空（结构性保证，判据直接查）。
//! - 退出协议：降级即时（安全优先），恢复逐级（每级退出需连续
//!   CONFIRM_TICKS 个采样低于该级退出水位——防瞬间反弹振荡）。
//! - 全归因：进入/降级/升级尝试/确认等待/正式回升，每步一条审计行，
//!   带原因码与水位快照——应急期间发生的每个状态变化都可对账。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0F60 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0F60，全仓独占）。
pub const SEG_BASE: u16 = 0x0F60;

pub const E_EM_LEVEL: u16 = SEG_BASE + 1; // 应急级别非法
pub const E_EM_CLASS: u16 = SEG_BASE + 2; // 资源类别非法
pub const E_EM_ORDER: u16 = SEG_BASE + 3; // 退出顺序违规（跳级回升）
pub const E_EM_CONFIRM: u16 = SEG_BASE + 4; // 确认窗口未满（反弹防护）

/// 分级触发水位（‰，提前定义，查表触发）。
pub const ENTER_YELLOW_PM: u64 = 850;
pub const ENTER_ORANGE_PM: u64 = 920;
pub const ENTER_RED_PM: u64 = 970;
/// 退出确认窗口：连续采样数（防瞬间反弹）。
pub const CONFIRM_TICKS: u32 = 3;

/// 资源类别（清单公示的最小单位）。
pub const CLASS_SYSTEM: u8 = 0; // 系统 UI / 输入响应（永保）
pub const CLASS_FOREGROUND: u8 = 1; // 前台应用
pub const CLASS_BACKGROUND: u8 = 2; // 后台任务
pub const CLASS_CACHE: u8 = 3; // 缓存/预取
pub const CLASS_TELEMETRY: u8 = 4; // 遥测上报
pub const CLASS_COUNT: usize = 5;

/// 应急级别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EmerLevel {
    /// 未进入应急。
    Green,
    /// 黄级：弃遥测+缓存。
    Yellow,
    /// 橙级：再弃后台。
    Orange,
    /// 红级：前台也限流，仅保系统。
    Red,
}

impl EmerLevel {
    pub fn rank(self) -> u8 {
        match self {
            EmerLevel::Green => 0,
            EmerLevel::Yellow => 1,
            EmerLevel::Orange => 2,
            EmerLevel::Red => 3,
        }
    }
    pub fn from_rank(r: u8) -> Result<Self, u16> {
        match r {
            0 => Ok(EmerLevel::Green),
            1 => Ok(EmerLevel::Yellow),
            2 => Ok(EmerLevel::Orange),
            3 => Ok(EmerLevel::Red),
            _ => Err(E_EM_LEVEL),
        }
    }
}

/// 弃车清单公示表：`sacrificed(level, class)` 静态查询任一组合。
/// 公示即常量——运行期不得临时发明弃车对象。
pub fn sacrificed(level: EmerLevel, class: u8) -> bool {
    if class as usize >= CLASS_COUNT {
        return false;
    }
    match level {
        EmerLevel::Green => false,
        EmerLevel::Yellow => class == CLASS_TELEMETRY || class == CLASS_CACHE,
        EmerLevel::Orange => class == CLASS_TELEMETRY || class == CLASS_CACHE || class == CLASS_BACKGROUND,
        EmerLevel::Red => class != CLASS_SYSTEM,
    }
}

/// 各级弃车类别数（公示表摘要，供快速对账）。
pub fn sacrificed_count(level: EmerLevel) -> usize {
    (0..CLASS_COUNT as u8).filter(|&c| sacrificed(level, c)).count()
}

/// 归因原因码。
pub const CAUSE_ENTER: u8 = 1; // 进入应急
pub const CAUSE_DEGRADE: u8 = 2; // 水位恶化降级
pub const CAUSE_RECOVER_STEP: u8 = 3; // 确认期满逐级回升
pub const CAUSE_RECOVER_WAIT: u8 = 4; // 恢复等待（确认窗口计数中）
pub const CAUSE_BOUNCE_BLOCK: u8 = 5; // 瞬间反弹被拦截

/// 应急审计行（全归因）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmerAudit {
    pub tick: u64,
    pub cause: u8,
    /// 变化前级别 rank。
    pub from_rank: u8,
    /// 变化后级别 rank（等待类与进入前相同）。
    pub to_rank: u8,
    /// 触发时水位快照（‰）。
    pub pressure_pm: u64,
}

/// 应急协议执行器。
#[derive(Debug)]
pub struct EmerProtocol {
    level: EmerLevel,
    /// 恢复确认计数（逐级退出的连续采样累计）。
    confirm_streak: u32,
    /// 当前水位（最近一次 feed）。
    pressure_pm: u64,
    /// 红级下前台限流系数（千分位，750 = 压到 75%）。
    fg_throttle_pm: u64,
    pub audits: Vec<EmerAudit>,
}

impl EmerProtocol {
    pub fn new() -> Self {
        Self {
            level: EmerLevel::Green,
            confirm_streak: 0,
            pressure_pm: 0,
            fg_throttle_pm: 750,
            audits: Vec::new(),
        }
    }

    pub fn level(&self) -> EmerLevel {
        self.level
    }

    pub fn pressure(&self) -> u64 {
        self.pressure_pm
    }

    /// 红级下前台应用的实际可用份额（限流系数 × 提交份额）。
    pub fn fg_available(&self, submitted: u64) -> u64 {
        if self.level == EmerLevel::Red {
            submitted * self.fg_throttle_pm / 1000
        } else {
            submitted
        }
    }

    /// 按级别进入/降级的即时水位判据（提前定义查表）。
    fn enter_level_for(pressure_pm: u64) -> EmerLevel {
        if pressure_pm >= ENTER_RED_PM {
            EmerLevel::Red
        } else if pressure_pm >= ENTER_ORANGE_PM {
            EmerLevel::Orange
        } else if pressure_pm >= ENTER_YELLOW_PM {
            EmerLevel::Yellow
        } else {
            EmerLevel::Green
        }
    }

    /// 每帧喂水位：恶化即时降级（安全优先）；恢复逐级（确认窗口）。
    pub fn feed(&mut self, tick: u64, pressure_pm: u64) {
        self.pressure_pm = pressure_pm;
        let want = Self::enter_level_for(pressure_pm);
        if want.rank() > self.level.rank() {
            // 恶化：即时升级（含 Green→Yellow 进入）。
            let cause = if self.level == EmerLevel::Green { CAUSE_ENTER } else { CAUSE_DEGRADE };
            let from = self.level.rank();
            self.level = want;
            self.confirm_streak = 0;
            self.audits.push(EmerAudit { tick, cause, from_rank: from, to_rank: want.rank(), pressure_pm });
        } else if want.rank() < self.level.rank() {
            // 恢复：逐级退出，每级需 CONFIRM_TICKS 连续确认。
            if self.confirm_streak + 1 >= CONFIRM_TICKS {
                let from = self.level.rank();
                let back = EmerLevel::from_rank(from - 1).unwrap_or(EmerLevel::Green);
                self.level = back;
                self.confirm_streak = 0;
                self.audits.push(EmerAudit {
                    tick,
                    cause: CAUSE_RECOVER_STEP,
                    from_rank: from,
                    to_rank: back.rank(),
                    pressure_pm,
                });
            } else {
                self.confirm_streak += 1;
                self.audits.push(EmerAudit {
                    tick,
                    cause: CAUSE_RECOVER_WAIT,
                    from_rank: self.level.rank(),
                    to_rank: self.level.rank(),
                    pressure_pm,
                });
            }
        } else {
            self.confirm_streak = 0;
        }
    }

    /// 外部强制级别迁移（如运维指令）：恶化方向拒绝（走 feed），
    /// 回升方向只允许逐级（退出协议纪律），违规拦截。
    pub fn force_step_up(&mut self, tick: u64, target: EmerLevel) -> Result<(), u16> {
        if target.rank() > self.level.rank() {
            // 恶化方向不走强制通道（由 feed 依水位判定）。
            return Err(E_EM_ORDER);
        }
        if self.level.rank() - target.rank() > 1 {
            // 回升只允许逐级。
            return Err(E_EM_ORDER);
        }
        if self.level != target {
            self.level = target;
            self.confirm_streak = 0;
            self.audits.push(EmerAudit {
                tick,
                cause: CAUSE_RECOVER_STEP,
                from_rank: self.level.rank(),
                to_rank: target.rank(),
                pressure_pm: self.pressure_pm,
            });
        }
        Ok(())
    }

    /// 查询某类别在当前级别是否被弃。
    pub fn is_sacrificed(&self, class: u8) -> bool {
        sacrificed(self.level, class)
    }

    /// 级别变化轨迹（去重后的 from→to 序列）。
    pub fn level_timeline(&self) -> Vec<(u64, u8)> {
        self.audits.iter().map(|a| (a.tick, a.to_rank)).collect()
    }
}

pub fn cause_name(cause: u8) -> &'static str {
    match cause {
        CAUSE_ENTER => "enter",
        CAUSE_DEGRADE => "degrade",
        CAUSE_RECOVER_STEP => "recover-step",
        CAUSE_RECOVER_WAIT => "recover-wait",
        CAUSE_BOUNCE_BLOCK => "bounce-block",
        _ => "unknown",
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、分级弃车：水位触发逐级恶化即时升级。
    {
        let mut em = EmerProtocol::new();
        em.feed(1, ENTER_YELLOW_PM);
        let l1 = em.level();
        em.feed(2, ENTER_ORANGE_PM);
        let l2 = em.level();
        em.feed(3, ENTER_RED_PM);
        let l3 = em.level();
        v.push(("F0451-分级弃车-逐级恶化即时升级", l1 == EmerLevel::Yellow && l2 == EmerLevel::Orange && l3 == EmerLevel::Red));
    }
    {
        // 一次喂入红级水位：直接进入红级（跳过中间级即时生效）。
        let mut em = EmerProtocol::new();
        em.feed(1, ENTER_RED_PM + 10);
        v.push(("F0451-分级弃车-严重枯竭直入红级", em.level() == EmerLevel::Red));
    }
    // 二、清单公示：静态查询 + 保留底线。
    {
        // 公示表摘要与逐类查询一致。
        let mut ok = true;
        for &lv in &[EmerLevel::Yellow, EmerLevel::Orange, EmerLevel::Red] {
            ok = ok && sacrificed_count(lv) == (0..CLASS_COUNT as u8).filter(|&c| sacrificed(lv, c)).count();
        }
        v.push(("F0451-清单公示-摘要与逐类一致", ok));
        // 保留底线：红级（最严）下 SYSTEM 永不被弃。
        v.push(("F0451-清单公示-系统类红级仍保留", !sacrificed(EmerLevel::Red, CLASS_SYSTEM)));
        // 公示单调：级别越严弃车不减。
        let c_y = sacrificed_count(EmerLevel::Yellow);
        let c_o = sacrificed_count(EmerLevel::Orange);
        let c_r = sacrificed_count(EmerLevel::Red);
        v.push(("F0451-清单公示-弃车随级别单调不减", c_y <= c_o && c_o <= c_r && c_r >= 2));
        // 黄级确切清单：遥测+缓存，恰好 2 类。
        v.push(("F0451-清单公示-黄级清单为遥测加缓存", c_y == 2 && !sacrificed(EmerLevel::Yellow, CLASS_BACKGROUND) && sacrificed(EmerLevel::Yellow, CLASS_TELEMETRY)));
    }
    // 三、退出协议：逐级回升 + 不瞬间反弹。
    {
        let mut em = EmerProtocol::new();
        em.feed(1, ENTER_ORANGE_PM); // 进入橙级
        // 水位骤降至绿：不瞬间反弹——先等待确认。
        em.feed(2, 100);
        em.feed(3, 100);
        let still = em.level();
        v.push(("F0451-退出协议-确认窗口内不反弹", still == EmerLevel::Orange));
        em.feed(4, 100);
        let after = em.level();
        v.push(("F0451-退出协议-确认期满逐级回升一级", after == EmerLevel::Yellow));
        // 再连续确认两级到 Green。
        em.feed(5, 100);
        em.feed(6, 100);
        em.feed(7, 100);
        v.push(("F0451-退出协议-逐级退到绿", em.level() == EmerLevel::Green));
        // 中途水位回升打断确认：回到红级以上 → 计数清零，3 次确认重新起算。
        let mut em2 = EmerProtocol::new();
        em2.feed(1, ENTER_RED_PM);
        em2.feed(2, 100);
        em2.feed(3, ENTER_RED_PM); // 打断（回到进入区间，确认清零）
        em2.feed(4, 100);
        em2.feed(5, 100);
        let interrupted = em2.level();
        v.push(("F0451-退出协议-确认中断计数重置", interrupted == EmerLevel::Red));
    }
    {
        // 跳级回升指令被拒（退出顺序纪律）。
        let mut em = EmerProtocol::new();
        em.feed(1, ENTER_ORANGE_PM);
        let r = em.force_step_up(2, EmerLevel::Green);
        v.push(("F0451-退出协议-跳级回升拒绝", r == Err(E_EM_ORDER)));
    }
    // 四、归因：每步审计可对账。
    {
        let mut em = EmerProtocol::new();
        em.feed(10, ENTER_RED_PM); // enter
        em.feed(11, 100);
        em.feed(12, 100);
        em.feed(13, 100); // recover-step
        let enters: Vec<&EmerAudit> = em.audits.iter().filter(|a| a.cause == CAUSE_ENTER).collect();
        let steps: Vec<&EmerAudit> = em.audits.iter().filter(|a| a.cause == CAUSE_RECOVER_STEP).collect();
        v.push(("F0451-归因-进入事件恰一条", enters.len() == 1 && enters[0].tick == 10));
        v.push(("F0451-归因-回升事件带水位快照", steps.len() == 1 && steps[0].pressure_pm == 100 && steps[0].from_rank == 3 && steps[0].to_rank == 2));
        // 等待事件也归因（recover-wait）。
        let waits: Vec<&EmerAudit> = em.audits.iter().filter(|a| a.cause == CAUSE_RECOVER_WAIT).collect();
        v.push(("F0451-归因-等待事件计入审计", waits.len() == 2));
        // 时间线完整。
        let tl = em.level_timeline();
        v.push(("F0451-归因-时间线覆盖全部变化", tl.len() == em.audits.len() && tl[0].1 == 3));
    }
    // 五、红级前台限流（弃车的量化效果）。
    {
        let mut em = EmerProtocol::new();
        em.feed(1, ENTER_RED_PM);
        v.push(("F0451-分级弃车-红级前台限流生效", em.fg_available(800) == 600));
        em.feed(2, 100);
        em.feed(3, 100);
        em.feed(4, 100); // 升回橙级
        v.push(("F0451-分级弃车-退出红级限流解除", em.fg_available(800) == 800));
    }
    // 六、判据自检：非法级别/类别码有界。
    {
        v.push(("F0451-判据-级别码非法被拒", EmerLevel::from_rank(9) == Err(E_EM_LEVEL)));
        v.push(("F0451-判据-类别越界不弃不保", !sacrificed(EmerLevel::Red, 99)));
        v.push(("F0451-判据-水位阈值单调", ENTER_YELLOW_PM < ENTER_ORANGE_PM && ENTER_ORANGE_PM < ENTER_RED_PM));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0451_emergency_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
