//! F441 计划任务创建 · 完整设计（STAR I 主册 G-I-41）。
//!
//! **判据（主册）**：三步流程用例；频率选择器全型（一次性/每日/每周/
//! 每月）；执行留痕与失败归因；清单联动；空闲条件判定。＋通12。
//!
//! 设计：计划任务向导核——三步建任务（做什么：应用/脚本；什么时候：
//! 图形化频率四型（一次性时刻 / 每日 HH:MM / 每周几+时刻 / 每月几号+
//! 时刻——不写 cron 表达式，人话入参）；什么条件：空闲判定（idle_ms ≥
//! 阈值）与电源态白名单）；触发器纯函数（给时刻算应否发——全型矩阵
//! 机检）；执行留痕账（触发时间 + 结果 + 失败归因人话）；清单联动
//! （建即入册，可禁用/改期/删除——F348 清单创建端）。
//!
//! **v4 深化批次新增（AI-U1）**：
//! - [`validate_schedule`] 入参域验证（时刻/星期/日期越界一律拒绝——
//!   坏输入在创建入口就被拦，不进清单）；
//! - 下次触发算术 [`TaskLedger::next_fire_after`]：四型各自的下次触发
//!   时刻（分钟序号）——含月长（大小月/闰年二月）真实日历算术，每月
//!   型跨月推进不做 30 天近似；
//! - 错失策略 [`MissPolicy`]：休眠/关机错过的每日/每周/每月触发——
//!   Skip（错过即弃）与 RunLate（醒来补跑一次）两档，判定纯函数；
//! - 重试退避：失败执行自动按 `RETRY_BACKOFF_MIN` 排重试（上限
//!   `MAX_RETRIES` 次——失败风暴不无限重试，账面诚实）；
//! - 执行统计 [`PlannedTask::success_rate_permille`]：留痕账的成功率
//!   （千分比）——诊断面直接可读。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 失败重试退避（分钟）。
pub const RETRY_BACKOFF_MIN: u64 = 5;
/// 单任务重试上限（防失败风暴）。
pub const MAX_RETRIES: u32 = 3;

/// 频率全型（图形化频率选择器——不写表达式）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Schedule {
    /// 一次性：年月日时分（简化为绝对分钟序号）。
    Once { at_min: u64 },
    /// 每日 HH:MM。
    Daily { hour: u8, minute: u8 },
    /// 每周几（0=周日..6=周六）+ 时刻。
    Weekly { weekday: u8, hour: u8, minute: u8 },
    /// 每月几号（1-28 防月末歧义）+ 时刻。
    Monthly { day: u8, hour: u8, minute: u8 },
}

/// 错失策略：系统不在线期间错过的周期触发怎么办。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissPolicy {
    /// 错过即弃（下个周期再说）。
    Skip,
    /// 醒来补跑一次（补跑留痕标 "missed-catchup"）。
    RunLate,
}

/// 运行条件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunCondition {
    Always,
    /// 空闲判定：系统空闲 ≥ 指定分钟才发。
    IdleFor { minutes: u64 },
    /// 电源态白名单。
    OnPowerOnly,
}

/// 调度入参域验证：任何越界值在创建入口被拒（坏数据不进清单）。
pub fn validate_schedule(s: &Schedule) -> bool {
    let hm_ok = |h: u8, m: u8| h < 24 && m < 60;
    match s {
        Schedule::Once { .. } => true,
        Schedule::Daily { hour, minute } => hm_ok(*hour, *minute),
        Schedule::Weekly { weekday, hour, minute } => *weekday < 7 && hm_ok(*hour, *minute),
        Schedule::Monthly { day, hour, minute } => (1..=28).contains(day) && hm_ok(*hour, *minute),
    }
}

/// 闰年判定（公历规则——下月推进算术的底座）。
pub fn is_leap_year(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// 某年某月天数（1-12 月）。
pub fn days_in_month(y: u64, m: u64) -> u64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// 触发上下文（判定入参）。
#[derive(Clone, Copy, Debug)]
pub struct TriggerCtx {
    /// 当前时刻（分钟序号，自某纪元）。
    pub now_min: u64,
    /// 当前星期几（0-6）。
    pub weekday: u8,
    /// 当月第几日（1-31）。
    pub day_of_month: u8,
    /// 当前空闲毫秒。
    pub idle_ms: u64,
    /// 是否接通电源。
    pub on_power: bool,
    /// 错失判定：自上次在线时刻起经过的分钟数（0 = 一直在线）。
    pub offline_min: u64,
}

impl TriggerCtx {
    fn day_minute(&self) -> u64 {
        self.now_min % (24 * 60)
    }
}

/// 一个计划任务。
#[derive(Clone, Debug)]
pub struct PlannedTask {
    pub name: String,
    /// 执行目标（应用路径或脚本）。
    pub target: String,
    pub schedule: Schedule,
    pub condition: RunCondition,
    pub miss_policy: MissPolicy,
    pub enabled: bool,
    /// 已连续重试次数（成功清零；达上限不再重试——诚实失败）。
    pub retry_count: u32,
    /// 待重试时刻（Some = 到点补一次；None = 无在途重试）。
    pub retry_at_min: Option<u64>,
    /// 执行留痕：((触发时刻, 成功, 归因))。
    pub runs: Vec<(u64, bool, &'static str)>,
}

impl PlannedTask {
    /// 执行留痕统计：成功率（千分比；无留痕返回 None——不假装 100%）。
    pub fn success_rate_permille(&self) -> Option<u64> {
        if self.runs.is_empty() {
            return None;
        }
        let ok = self.runs.iter().filter(|(_, success, _)| *success).count();
        Some((ok as u64 * 1_000) / self.runs.len() as u64)
    }
}

/// 计划任务清单核（F348 清单创建端）。
pub struct TaskLedger {
    pub tasks: Vec<PlannedTask>,
    /// 建任务被入参验证拒绝的次数（诚实账）。
    pub rejected_creates: u64,
}

impl TaskLedger {
    pub fn new() -> TaskLedger {
        TaskLedger { tasks: Vec::new(), rejected_creates: 0 }
    }

    /// 三步创建（第三步完成即入册）。入参先过域验证——非法调度拒绝。
    pub fn create(
        &mut self,
        name: &str,
        target: &str,
        schedule: Schedule,
        condition: RunCondition,
    ) -> Result<usize, &'static str> {
        if name.is_empty() || target.is_empty() {
            self.rejected_creates += 1;
            return Err("名称与执行目标不能为空");
        }
        if !validate_schedule(&schedule) {
            self.rejected_creates += 1;
            return Err("时刻/星期/日期越界——请检查频率选择器的值");
        }
        self.tasks.push(PlannedTask {
            name: String::from(name),
            target: String::from(target),
            schedule,
            condition,
            miss_policy: MissPolicy::Skip,
            enabled: true,
            retry_count: 0,
            retry_at_min: None,
            runs: Vec::new(),
        });
        Ok(self.tasks.len() - 1)
    }

    pub fn set_enabled(&mut self, idx: usize, on: bool) -> bool {
        match self.tasks.get_mut(idx) {
            Some(t) => {
                t.enabled = on;
                true
            }
            None => false,
        }
    }

    pub fn reschedule(&mut self, idx: usize, schedule: Schedule) -> bool {
        if !validate_schedule(&schedule) {
            return false;
        }
        match self.tasks.get_mut(idx) {
            Some(t) => {
                t.schedule = schedule;
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, idx: usize) -> bool {
        if idx < self.tasks.len() {
            self.tasks.remove(idx);
            true
        } else {
            false
        }
    }

    /// 触发判定（纯函数）：频率全型矩阵 + 条件判定 + 错失补跑判定。
    pub fn due(&self, task: &PlannedTask, ctx: &TriggerCtx) -> bool {
        if !task.enabled {
            return false;
        }
        let hm_ok = |hour: u8, minute: u8| -> bool {
            let day_min = ctx.day_minute();
            day_min == hour as u64 * 60 + minute as u64
        };
        let sched_hit = match &task.schedule {
            Schedule::Once { at_min } => ctx.now_min == *at_min,
            Schedule::Daily { hour, minute } => hm_ok(*hour, *minute),
            Schedule::Weekly { weekday, hour, minute } => {
                ctx.weekday == *weekday && hm_ok(*hour, *minute)
            }
            Schedule::Monthly { day, hour, minute } => {
                ctx.day_of_month == *day && hm_ok(*hour, *minute)
            }
        };
        // 错失补跑：本周期没踩点，但刚恢复在线、错失窗口内有过本该触发的
        // 时刻，且策略是 RunLate → 补跑一次（以「在线首分钟」为补跑点）。
        let catchup_hit = if sched_hit {
            false
        } else if task.miss_policy == MissPolicy::RunLate && ctx.offline_min > 0 {
            self.missed_while_offline(task, ctx)
        } else {
            false
        };
        if !sched_hit && !catchup_hit {
            return false;
        }
        match &task.condition {
            RunCondition::Always => true,
            RunCondition::IdleFor { minutes } => ctx.idle_ms >= minutes * 60 * 1_000,
            RunCondition::OnPowerOnly => ctx.on_power,
        }
    }

    /// 错失判定：离线窗口（now-offline_min, now] 内是否含本该触发的时刻。
    /// 每日/每周/每月按日历对齐判定；一次性错失即错过（一次性无周期）。
    fn missed_while_offline(&self, task: &PlannedTask, ctx: &TriggerCtx) -> bool {
        let hm = match &task.schedule {
            Schedule::Daily { hour, minute } => *hour as u64 * 60 + *minute as u64,
            Schedule::Weekly { weekday, hour, minute } => {
                if ctx.weekday != *weekday {
                    return false; // 今日不是目标日——错失不在今日
                }
                *hour as u64 * 60 + *minute as u64
            }
            Schedule::Monthly { day, hour, minute } => {
                if ctx.day_of_month != *day {
                    return false;
                }
                *hour as u64 * 60 + *minute as u64
            }
            Schedule::Once { .. } => return false,
        };
        // 目标时刻 today_hm；离线起点 = now - offline_min。
        let now_day_min = ctx.day_minute();
        let today_target = if hm <= now_day_min { hm } else { return false };
        let offline_start_day_min = now_day_min.saturating_sub(ctx.offline_min);
        // 目标时刻落在离线窗口内（严格大于离线起点——在线时段的正常触发
        // 走 sched_hit，不在这里重复计）。
        today_target > offline_start_day_min && today_target < now_day_min
    }

    /// 下次触发时刻（分钟序号，开区间 `(after_min, …]` 里最早的一次）。
    /// 每日/每周/每月按真实日历推进（大小月/闰年二月不做近似）。
    pub fn next_fire_after(&self, task: &PlannedTask, after_min: u64, weekday_at_after: u8) -> Option<u64> {
        let day0 = after_min / (24 * 60); // 纪元起第几天
        match &task.schedule {
            Schedule::Once { at_min } => {
                if *at_min > after_min {
                    Some(*at_min)
                } else {
                    None
                }
            }
            Schedule::Daily { hour, minute } => {
                let tgt = *hour as u64 * 60 + *minute as u64;
                let today = day0 * 24 * 60 + tgt;
                Some(if today > after_min { today } else { today + 24 * 60 })
            }
            Schedule::Weekly { weekday, hour, minute } => {
                let tgt = *hour as u64 * 60 + *minute as u64;
                let wd = *weekday as u64;
                let cur = weekday_at_after as u64;
                let delta = (wd + 7 - cur) % 7;
                let mut cand = (day0 + delta) * 24 * 60 + tgt;
                if cand <= after_min {
                    cand += 7 * 24 * 60;
                }
                Some(cand)
            }
            Schedule::Monthly { day, hour, minute } => {
                // 从纪元精确定位 after 所在「年-月」（逐月走——不做 360 天
                // 近似；判据要求跨大小月/闰年推进全部按真实日历）。
                let (mut yy, mut mm) = (0u64, 1u64);
                let mut rem = day0;
                loop {
                    let yd = if is_leap_year(yy) { 366 } else { 365 };
                    if rem < yd {
                        break;
                    }
                    rem -= yd;
                    yy += 1;
                }
                loop {
                    let dim = days_in_month(yy, mm);
                    if rem < dim {
                        break;
                    }
                    rem -= dim;
                    mm += 1;
                    if mm > 12 {
                        mm = 1;
                        yy += 1;
                    }
                }
                // 从 (yy, mm) 起逐月找 day 号（day 恒 ≤28，任何月都存在）。
                let tgt = *hour as u64 * 60 + *minute as u64;
                loop {
                    let cand = (acc_of(yy, mm) + *day as u64 - 1) * 24 * 60 + tgt;
                    if cand > after_min {
                        return Some(cand);
                    }
                    mm += 1;
                    if mm > 12 {
                        mm = 1;
                        yy += 1;
                    }
                }
            }
        }
    }

    /// 执行留痕（结果 + 失败归因人话）；失败自动排重试（上限内）。
    pub fn record_run(&mut self, idx: usize, at_min: u64, ok: bool, cause: &'static str) -> bool {
        let Some(t) = self.tasks.get_mut(idx) else { return false };
        t.runs.push((at_min, ok, cause));
        if ok {
            t.retry_count = 0;
            t.retry_at_min = None;
        } else if t.retry_count < MAX_RETRIES {
            t.retry_count += 1;
            t.retry_at_min = Some(at_min + RETRY_BACKOFF_MIN * t.retry_count as u64);
        } else {
            // 达上限：诚实失败，不再排——留痕即终局。
            t.retry_at_min = None;
        }
        true
    }

    /// 重试到期判定（调度环每分钟问一次：到点补发）。
    pub fn retry_due(&self, idx: usize, now_min: u64) -> bool {
        match self.tasks.get(idx) {
            Some(t) => t.enabled && matches!(t.retry_at_min, Some(at) if now_min >= at),
            None => false,
        }
    }
}

/// 纪元起第 `y` 年第 `m` 月之前的累计天数（1 月 1 日 = 第 0 天）。
fn acc_of(y: u64, m: u64) -> u64 {
    let mut acc = 0;
    for yy in 0..y {
        acc += if is_leap_year(yy) { 366 } else { 365 };
    }
    for mm in 1..m {
        acc += days_in_month(y, mm);
    }
    acc
}

pub fn run_schedtask_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F441");
    let mut led = TaskLedger::new();
    // 三步流程：四型全建（频率选择器全型——不写表达式）。
    let i_once = led.create("备份文档", "vxbackup.run", Schedule::Once { at_min: 500 }, RunCondition::Always);
    let i_daily = led.create("清理缓存", "vxclean.run", Schedule::Daily { hour: 3, minute: 30 }, RunCondition::IdleFor { minutes: 15 });
    let i_weekly = led.create("周报归档", "vxarch.run", Schedule::Weekly { weekday: 1, hour: 9, minute: 0 }, RunCondition::Always);
    let i_monthly = led.create("月度体检", "vxcheck.run", Schedule::Monthly { day: 1, hour: 12, minute: 0 }, RunCondition::OnPowerOnly);
    set.add(
        "f441-three-step-create",
        led.tasks.len() == 4
            && i_once == Ok(0)
            && i_daily == Ok(1)
            && i_weekly == Ok(2)
            && i_monthly == Ok(3),
        "",
    );
    // 频率矩阵：周一 09:00 时刻——一次性未到、每日 3:30 不中、每周一 9:00 中。
    let ctx = TriggerCtx { now_min: 9 * 60, weekday: 1, day_of_month: 1, idle_ms: 20 * 60 * 1_000, on_power: true, offline_min: 0 };
    set.add(
        "f441-schedule-matrix",
        !led.due(&led.tasks[0].clone(), &ctx)
            && !led.due(&led.tasks[1].clone(), &ctx)
            && led.due(&led.tasks[2].clone(), &ctx),
        "",
    );
    // 每月 1 号 12:00：时刻与日期同时命中才发。
    let ctx_noon = TriggerCtx { now_min: 12 * 60, weekday: 1, day_of_month: 1, idle_ms: 0, on_power: true, offline_min: 0 };
    set.add(
        "f441-monthly-due",
        led.due(&led.tasks[3].clone(), &ctx_noon)
            && !led.due(
                &led.tasks[3].clone(),
                &TriggerCtx { day_of_month: 2, ..ctx_noon },
            ),
        "",
    );
    // 入参域验证：越界调度在创建入口被拒（坏数据不进清单）。
    set.add(
        "f441-input-domain-guard",
        led.create("坏时刻", "x.run", Schedule::Daily { hour: 24, minute: 0 }, RunCondition::Always) == Err("时刻/星期/日期越界——请检查频率选择器的值")
            && led.create("坏星期", "x.run", Schedule::Weekly { weekday: 7, hour: 1, minute: 0 }, RunCondition::Always) == Err("时刻/星期/日期越界——请检查频率选择器的值")
            && led.create("坏日期", "x.run", Schedule::Monthly { day: 0, hour: 1, minute: 0 }, RunCondition::Always) == Err("时刻/星期/日期越界——请检查频率选择器的值")
            && led.create("", "x.run", Schedule::Daily { hour: 1, minute: 0 }, RunCondition::Always) == Err("名称与执行目标不能为空")
            && led.rejected_creates == 4
            && led.tasks.len() == 4,
        "",
    );
    // 下次触发算术：每日型跨日推进。
    let next_daily = led.next_fire_after(&led.tasks[1].clone(), 3 * 60 + 30, 1);
    set.add("f441-next-daily", next_daily == Some(24 * 60 + 3 * 60 + 30), "");
    // 下次触发算术：每周型（周一 9:00，after=周三 12:00 → 下周一，隔 5 天）。
    let next_weekly = led.next_fire_after(&led.tasks[2].clone(), 3 * 24 * 60 + 12 * 60, 3);
    set.add("f441-next-weekly", next_weekly == Some(8 * 24 * 60 + 9 * 60), "");
    // 下次触发算术：每月型跨月推进（15 号 → 下月 1 号；与二月长度无关）。
    let next_monthly = led.next_fire_after(&led.tasks[3].clone(), 15 * 24 * 60 + 12 * 60, 0);
    set.add("f441-next-monthly", next_monthly == Some(31 * 24 * 60 + 12 * 60), "");
    // 一次性过期 → 永不再发（None）。
    set.add("f441-once-expired-none", led.next_fire_after(&led.tasks[0].clone(), 500, 1).is_none(), "");
    // 错失策略 RunLate：离线跨过每日 3:30，醒来补跑；Skip 则不补。
    let mut led2 = TaskLedger::new();
    let _ = led2.create("补跑任务", "t.run", Schedule::Daily { hour: 3, minute: 30 }, RunCondition::Always);
    led2.tasks[0].miss_policy = MissPolicy::RunLate;
    // 醒来在 4:00（离线 60 分钟 → 3:30 落在离线窗口内）。
    let wake = TriggerCtx { now_min: 4 * 60, weekday: 2, day_of_month: 2, idle_ms: 0, on_power: true, offline_min: 60 };
    set.add("f441-miss-runlate-catchup", led2.due(&led2.tasks[0].clone(), &wake), "");
    led2.tasks[0].miss_policy = MissPolicy::Skip;
    set.add("f441-miss-skip-drops", !led2.due(&led2.tasks[0].clone(), &wake), "");
    // 离线窗口外（目标时刻在离线起点之前）不补——正常在线触发不重复。
    let long_ago = TriggerCtx { now_min: 20 * 60, weekday: 2, day_of_month: 2, idle_ms: 0, on_power: true, offline_min: 60 };
    set.add("f441-miss-window-bounded", !led2.due(&led2.tasks[0].clone(), &long_ago), "");
    // 重试退避：失败排重试（5 分钟 × 递增轮次）；达上限诚实终局。
    let idx = led2.create("会失败的任务", "f.run", Schedule::Daily { hour: 5, minute: 0 }, RunCondition::Always);
    let idx = idx.unwrap_or_default();
    let _ = led2.record_run(idx, 300, false, "目标脚本不存在——检查路径");
    set.add(
        "f441-retry-backoff",
        led2.tasks[idx].retry_at_min == Some(300 + RETRY_BACKOFF_MIN) && led2.retry_due(idx, 305),
        "",
    );
    for round in 1..=MAX_RETRIES {
        let at = 305 + (round as u64) * 5;
        let _ = led2.record_run(idx, at, false, "目标脚本不存在——检查路径");
    }
    set.add(
        "f441-retry-cap-honest",
        led2.tasks[idx].retry_count == MAX_RETRIES
            && led2.tasks[idx].retry_at_min.is_none()
            && led2.tasks[idx].runs.len() == 1 + MAX_RETRIES as usize,
        "",
    );
    // 成功清零重试账。
    let _ = led2.record_run(idx, 400, true, "");
    set.add("f441-retry-cleared-on-success", led2.tasks[idx].retry_count == 0 && led2.tasks[idx].retry_at_min.is_none(), "");
    // 执行统计：留痕账成功率（无留痕 = None，不假装）。
    let mut led3 = TaskLedger::new();
    let j = led3.create("统计样本", "s.run", Schedule::Daily { hour: 1, minute: 0 }, RunCondition::Always);
    let j = j.unwrap_or_default();
    set.add("f441-stats-empty-honest", led3.tasks[j].success_rate_permille().is_none(), "");
    for i in 0..8u64 {
        let _ = led3.record_run(j, i, i % 4 != 3, "");
    }
    set.add("f441-stats-rate", led3.tasks[j].success_rate_permille() == Some(750), "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_condition_gates_run() {
        let mut led = TaskLedger::new();
        let i = led.create("空闲整理", "t.run", Schedule::Daily { hour: 3, minute: 30 }, RunCondition::IdleFor { minutes: 15 }).unwrap_or_default();
        let awake = TriggerCtx { now_min: 3 * 60 + 30, weekday: 3, day_of_month: 12, idle_ms: 5 * 60 * 1_000, on_power: true, offline_min: 0 };
        let idle = TriggerCtx { idle_ms: 16 * 60 * 1_000, ..awake };
        let t = led.tasks[i].clone();
        assert!(!led.due(&t, &awake), "空闲不足 15 分钟：不发");
        assert!(led.due(&t, &idle), "空闲达标：发");
    }

    #[test]
    fn disabled_task_never_fires() {
        let mut led = TaskLedger::new();
        let i = led.create("已禁用", "t.run", Schedule::Daily { hour: 3, minute: 30 }, RunCondition::Always).unwrap_or_default();
        led.set_enabled(i, false);
        let ctx = TriggerCtx { now_min: 3 * 60 + 30, weekday: 3, day_of_month: 12, idle_ms: 0, on_power: true, offline_min: 0 };
        let t = led.tasks[i].clone();
        assert!(!led.due(&t, &ctx), "禁用即停——不触发");
        // 执行留痕与失败归因。
        led.set_enabled(i, true);
        led.record_run(i, 210, false, "目标脚本不存在——检查路径");
        led.record_run(i, 211, true, "");
        assert_eq!(led.tasks[i].runs.len(), 2);
        assert!(!led.tasks[i].runs[0].1 && led.tasks[i].runs[0].2.contains("脚本不存在"));
    }

    #[test]
    fn leap_year_february_has_29_days() {
        assert!(is_leap_year(2028) && days_in_month(2028, 2) == 29);
        assert!(!is_leap_year(2029) && days_in_month(2029, 2) == 28);
        assert_eq!(days_in_month(2028, 4), 30, "小月 30 天");
    }

    #[test]
    fn reschedule_validates_too() {
        let mut led = TaskLedger::new();
        let i = led.create("改期样本", "t.run", Schedule::Daily { hour: 1, minute: 0 }, RunCondition::Always).unwrap_or_default();
        assert!(!led.reschedule(i, Schedule::Daily { hour: 25, minute: 0 }), "越界改期拒绝");
        assert!(led.reschedule(i, Schedule::Daily { hour: 23, minute: 59 }), "合法改期通过");
    }

    #[test]
    fn monthly_next_crosses_short_months() {
        // 1 月 1 号 0:00 之后 → 下次 2 月 1 号（31 天后），不是 30 天近似。
        let mut led = TaskLedger::new();
        let i = led.create("跨月", "t.run", Schedule::Monthly { day: 1, hour: 0, minute: 0 }, RunCondition::Always).unwrap_or_default();
        let next = led.next_fire_after(&led.tasks[i].clone(), 0, 0);
        assert_eq!(next, Some(31 * 24 * 60), "1 月 31 天——跨月推进按真实日历");
    }
}
