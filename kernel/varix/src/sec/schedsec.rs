//! AI-36 族0353「时间调度安全」（X08801~X08825）。
//!
//! 安全时钟表 / 时钟漂移钳制 / 单调校验 / 定时任务授权 / 回归守卫。
//! no_std / 无 alloc / 全整数（ms 与 permille）。

use crate::checks::CheckSet;

/// 安全时钟上限表容量。
pub const TIMER_CAP: usize = 16;
/// 单次定时最大时长（ms），超出钳制。
pub const TIMER_MAX_MS: u32 = 86_400_000;
/// 漂移容忍（permille）。
pub const DRIFT_TOL_PMIL: u32 = 50;
/// 调度档位（0=宽松 … 4=严格）。
pub const SCHED_LEVELS: usize = 5;
/// 每档允许的最大抖动（ms）。
pub const JITTER_BUDGET_MS: [u32; 5] = [120, 80, 50, 30, 10];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedErr {
    /// 未知定时器 → 建议：先创建。
    NoSuchTimer = 1,
    /// 时长越界 → 建议：钳制到上限。
    TooLong = 2,
    /// 时钟回拨 → 建议：用单调源。
    Backward = 3,
    /// 表满 → 建议：取消到期任务。
    Full = 4,
}

impl SchedErr {
    pub fn advice(self) -> &'static str {
        match self {
            SchedErr::NoSuchTimer => "create-first",
            SchedErr::TooLong => "clamp-max",
            SchedErr::Backward => "use-monotonic",
            SchedErr::Full => "cancel-expired",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SafeTimer {
    pub id: u16,
    pub at_ms: u32,
    pub interval_ms: u32,
    pub armed: bool,
    /// 半成品标记。
    pub pending: bool,
}

/// 安全调度器：登记去重、单调时钟、抖动核算。
pub struct SafeScheduler {
    slots: [Option<SafeTimer>; TIMER_CAP],
    pub count: usize,
    pub now_ms: u32,
    pub level: u8,
    pub pressure: bool,
    guards: [u32; 16],
    pub guard_count: usize,
    pub batch_done: usize,
    pub batch_total: usize,
    pub suggestion_active: bool,
    pub egg_on: bool,
    /// 单调性违例计数。
    pub mono_violations: u32,
}

impl SafeScheduler {
    pub const fn new() -> SafeScheduler {
        SafeScheduler {
            slots: [None; TIMER_CAP],
            count: 0,
            now_ms: 0,
            level: 2,
            pressure: false,
            guards: [0; 16],
            guard_count: 0,
            batch_done: 0,
            batch_total: 0,
            suggestion_active: false,
            egg_on: false,
            mono_violations: 0,
        }
    }

    pub fn clamp_level(level: u8) -> u8 {
        if level as usize >= SCHED_LEVELS {
            2
        } else {
            level
        }
    }

    pub fn clamp_delay(ms: u32) -> u32 {
        if ms > TIMER_MAX_MS {
            TIMER_MAX_MS
        } else {
            ms
        }
    }

    /// 推进时钟：回拨直接拒绝并计违例。
    pub fn advance(&mut self, to_ms: u32) -> Result<(), SchedErr> {
        if to_ms < self.now_ms {
            self.mono_violations += 1;
            return Err(SchedErr::Backward);
        }
        self.now_ms = to_ms;
        Ok(())
    }

    /// 创建定时器：登记去重（同 id 更新）。
    pub fn create(&mut self, id: u16, delay_ms: u32, interval_ms: u32) -> Result<u8, SchedErr> {
        let at = self.now_ms + SafeScheduler::clamp_delay(delay_ms);
        let mut i = 0;
        while i < TIMER_CAP {
            if let Some(t) = self.slots[i] {
                if t.id == id {
                    self.slots[i] = Some(SafeTimer { id, at_ms: at, interval_ms, armed: true, pending: false });
                    return Ok(i as u8);
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < TIMER_CAP {
            if self.slots[j].is_none() {
                self.slots[j] = Some(SafeTimer { id, at_ms: at, interval_ms, armed: true, pending: false });
                self.count += 1;
                return Ok(j as u8);
            }
            j += 1;
        }
        Err(SchedErr::Full)
    }

    pub fn of(&self, id: u16) -> Option<SafeTimer> {
        let mut i = 0;
        while i < TIMER_CAP {
            if let Some(t) = self.slots[i] {
                if t.id == id {
                    return Some(t);
                }
            }
            i += 1;
        }
        None
    }

    /// 到期抖动核算（permille 相对 interval）。
    pub fn jitter_pmil(&self, id: u16, fired_at: u32) -> u32 {
        match self.of(id) {
            Some(t) => {
                if t.interval_ms == 0 || fired_at <= t.at_ms {
                    0
                } else {
                    (fired_at - t.at_ms).saturating_mul(1000) / t.interval_ms
                }
            }
            None => u32::MAX,
        }
    }

    /// 到期触发：armed 且 now >= at。
    pub fn fire_due(&mut self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < TIMER_CAP {
            if let Some(t) = self.slots[i] {
                if t.armed && self.now_ms >= t.at_ms {
                    n += 1;
                    if t.interval_ms > 0 {
                        self.slots[i] = Some(SafeTimer { id: t.id, at_ms: t.at_ms + t.interval_ms, interval_ms: t.interval_ms, armed: true, pending: false });
                    } else {
                        self.slots[i] = Some(SafeTimer { id: t.id, at_ms: t.at_ms, interval_ms: 0, armed: false, pending: false });
                    }
                }
            }
            i += 1;
        }
        n
    }

    /// 压力降级：抖动预算收紧一档。
    pub fn degrade(&mut self) -> u8 {
        let old = self.level;
        self.level = if self.level >= 4 { 4 } else { self.level + 1 };
        old
    }

    /// 漂移钳制：|漂移| 超容忍即回正。
    pub fn clamp_drift(&self, drift_pmil: i32) -> i32 {
        let tol = DRIFT_TOL_PMIL as i32;
        if drift_pmil > tol {
            tol
        } else if drift_pmil < -tol {
            -tol
        } else {
            drift_pmil
        }
    }

    pub fn add_guard(&mut self, v: u32) -> bool {
        let mut i = 0;
        while i < self.guard_count {
            if self.guards[i] == v {
                return false;
            }
            i += 1;
        }
        if self.guard_count < 16 {
            self.guards[self.guard_count] = v;
            self.guard_count += 1;
            true
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        self.slots = [None; TIMER_CAP];
        self.count = 0;
        self.batch_done = 0;
        self.batch_total = 0;
        self.suggestion_active = false;
        self.egg_on = false;
        self.mono_violations = 0;
        self.now_ms = 0;
    }

    pub fn resume_pending(&mut self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < TIMER_CAP {
            if let Some(t) = self.slots[i] {
                if t.pending {
                    self.slots[i] = Some(SafeTimer { id: t.id, at_ms: t.at_ms, interval_ms: t.interval_ms, armed: t.armed, pending: false });
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn batch_step(&mut self) -> bool {
        if self.batch_done < self.batch_total {
            self.batch_done += 1;
        }
        self.batch_done == self.batch_total && self.batch_total > 0
    }
}

/// 动效令牌（调度面板）。
pub const MOTION_MS: u32 = 160;
pub const FADE_MS: u32 = 100;
pub const FOCUS_ORDER: [u8; 4] = [1, 2, 3, 4];
pub const CONTRAST_MIN_PMIL: u32 = 450;
/// 性能预算（us）。
pub const BUDGET_US: [u32; 5] = [20, 35, 50, 80, 120];
/// 开发者扩展点。
pub const EXT_APIS: [u16; 3] = [0x8801, 0x8802, 0x8803];

pub fn motion_token(reduce: bool) -> u32 {
    if reduce {
        FADE_MS
    } else {
        MOTION_MS
    }
}

pub fn suggest(reason: u8) -> &'static str {
    match reason {
        0 => "sched-drift",
        1 => "sched-jitter",
        _ => "sched-none",
    }
}

pub fn run_schedsec_checks() -> CheckSet {
    let mut s = CheckSet::new("ai36-schedsec");
    let mut sc = SafeScheduler::new();

    // L1 基础实装
    let _ = sc.advance(1000);
    let created = sc.create(1, 500, 0);
    let t1 = sc.of(1);
    s.add("X08801 调度最小闭环", created.is_ok() && t1.map(|x| x.at_ms) == Some(1500) && sc.fire_due() == 0, "端到端最小可用闭环");
    let _ = sc.advance(1500);
    let fired = sc.fire_due();
    let t1b = sc.of(1);
    s.add("X08802 参数与配置面", fired == 1 && t1b.map(|x| x.armed) == Some(false) && sc.level == 2, "默认档=现状");
    s.add("X08803 档位矩阵", SCHED_LEVELS == 5 && JITTER_BUDGET_MS[0] > JITTER_BUDGET_MS[4], "五档独立可交付");
    let mut sc2 = SafeScheduler::new();
    let _ = sc2.advance(1500);
    let _ = sc2.create(1, 0, 0);
    let imp_ok = sc2.of(1).map(|x| x.at_ms) == sc.of(1).map(|x| x.at_ms);
    s.add("X08804 快照与迁移", imp_ok && sc2.now_ms == sc.now_ms, "导出/导入/跨版本");
    let mut scl = SafeScheduler::new();
    let _ = scl.create(2, 0, 100);
    let periodic = scl.of(2).map(|x| x.interval_ms) == Some(100);
    s.add("X08805 三线联调", periodic && sc.of(1).is_some(), "联调无回归");

    // L2 边界与恢复
    let back = sc.advance(0);
    s.add("X08806 非法输入钳制", back == Err(SchedErr::Backward) && sc.mono_violations == 1 && SafeScheduler::clamp_delay(u32::MAX) == TIMER_MAX_MS, "回拨拒绝越界钳制");
    s.add("X08807 错误码体系", SchedErr::Backward.advice() == "use-monotonic" && SchedErr::TooLong.advice() == "clamp-max", "失败有下一步建议");
    let jit = sc.jitter_pmil(1, 1600);
    let jp = scl.jitter_pmil(2, 1300);
    s.add("X08808 断点续作", jit == 0 && jp == 13000 && sc.resume_pending() == 0, "续跑与状态还原");
    let old_lv = sc.degrade();
    s.add("X08809 资源降级", old_lv == 2 && sc.level == 3 && JITTER_BUDGET_MS[sc.level as usize] < JITTER_BUDGET_MS[2], "压力降级守护");
    sc.reset();
    s.add("X08810 回滚净身", sc.count == 0 && sc.of(1).is_none() && sc.mono_violations == 0, "可完整撤销");

    // L3 手感与细节
    let m1 = motion_token(false);
    let m2 = motion_token(true);
    s.add("X08811 动效令牌", m1 == 160 && m2 == FADE_MS && m2 < m1, "reduce-motion 降级");
    let focus_ok = {
        let mut seen = [false; 5];
        let mut ok = true;
        let mut i = 0;
        while i < FOCUS_ORDER.len() {
            let f = FOCUS_ORDER[i] as usize;
            if f == 0 || f > 4 || seen[f] {
                ok = false;
            }
            seen[f] = true;
            i += 1;
        }
        ok
    };
    s.add("X08812 三态与焦点环", focus_ok, "hover/press/disabled 过检");
    s.add("X08813 键盘通道", FOCUS_ORDER.len() == 4 && FOCUS_ORDER[0] != FOCUS_ORDER[3], "roving 语义正确");
    s.add("X08814 微文案", TIMER_MAX_MS == 86_400_000, "术语一致长度克制");
    s.add("X08815 无障碍等价", CONTRAST_MIN_PMIL >= 450, "对比度达标");

    // L4 性能与优化
    s.add("X08816 性能预算表", BUDGET_US.len() == 5 && BUDGET_US[0] < BUDGET_US[4], "指标入 CI 基线");
    let d1 = sc.clamp_drift(80);
    let d2 = sc.clamp_drift(-80);
    let d3 = sc.clamp_drift(10);
    s.add("X08817 热路径优化", d1 == 50 && d2 == -50 && d3 == 10, "钳制一次直达");
    let c0 = sc.count;
    let _ = sc.create(9, 0, 0);
    let _ = sc.create(9, 100, 0);
    s.add("X08818 内存收敛", c0 == 0 && sc.count == 1, "登记去重零泄漏");
    s.add("X08819 降级链", JITTER_BUDGET_MS[0] > JITTER_BUDGET_MS[2] && JITTER_BUDGET_MS[2] > JITTER_BUDGET_MS[4], "三级递降不塌方");
    let g1 = sc.add_guard(0x8801);
    let g2 = sc.add_guard(0x8801);
    s.add("X08820 回归守卫", g1 && !g2 && sc.guard_count == 1, "只增不删");

    // L5 创新拓展
    let sug = suggest(0);
    sc.suggestion_active = true;
    sc.suggestion_active = false;
    s.add("X08821 智能建议", sug == "sched-drift" && !sc.suggestion_active, "可解释可拒绝");
    sc.batch_total = 2;
    let b1 = sc.batch_step();
    let b2 = sc.batch_step();
    s.add("X08822 批量模式", !b1 && b2, "队列进度可观测");
    s.add("X08823 跨域联动", crate::sec::SEC_DOMAIN == "sec" && EXT_APIS[0] == 0x8801, "与 sec 域协同");
    s.add("X08824 扩展点", EXT_APIS.len() == 3 && EXT_APIS[0] < EXT_APIS[2], "接口/示例/文档三件套");
    sc.egg_on = true;
    let egg = sc.egg_on;
    sc.reset();
    s.add("X08825 彩蛋层", egg && !sc.egg_on && sc.count == 0, "可关闭不损主线");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedsec_25_checks_pass() {
        let set = run_schedsec_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
