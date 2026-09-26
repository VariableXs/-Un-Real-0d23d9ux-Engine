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

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

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

/// 运行条件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunCondition {
    Always,
    /// 空闲判定：系统空闲 ≥ 指定分钟才发。
    IdleFor { minutes: u64 },
    /// 电源态白名单。
    OnPowerOnly,
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
    pub enabled: bool,
    /// 执行留痕：((触发时刻, 成功, 归因))。
    pub runs: Vec<(u64, bool, &'static str)>,
}

/// 计划任务清单核（F348 清单创建端）。
pub struct TaskLedger {
    pub tasks: Vec<PlannedTask>,
}

impl TaskLedger {
    pub fn new() -> TaskLedger {
        TaskLedger { tasks: Vec::new() }
    }

    /// 三步创建（第三步完成即入册）。
    pub fn create(
        &mut self,
        name: &str,
        target: &str,
        schedule: Schedule,
        condition: RunCondition,
    ) -> usize {
        self.tasks.push(PlannedTask {
            name: String::from(name),
            target: String::from(target),
            schedule,
            condition,
            enabled: true,
            runs: Vec::new(),
        });
        self.tasks.len() - 1
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

    /// 触发判定（纯函数）：频率全型矩阵 + 条件判定。
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
        if !sched_hit {
            return false;
        }
        match &task.condition {
            RunCondition::Always => true,
            RunCondition::IdleFor { minutes } => ctx.idle_ms >= minutes * 60 * 1_000,
            RunCondition::OnPowerOnly => ctx.on_power,
        }
    }

    /// 执行留痕（结果 + 失败归因人话）。
    pub fn record_run(&mut self, idx: usize, at_min: u64, ok: bool, cause: &'static str) -> bool {
        match self.tasks.get_mut(idx) {
            Some(t) => {
                t.runs.push((at_min, ok, cause));
                true
            }
            None => false,
        }
    }
}

pub fn run_schedtask_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F441");
    let mut led = TaskLedger::new();
    // 三步流程：四型全建（频率选择器全型——不写表达式）。
    let i_once = led.create("备份文档", "vxbackup.run", Schedule::Once { at_min: 500 }, RunCondition::Always);
    let i_daily = led.create("清理缓存", "vxclean.run", Schedule::Daily { hour: 3, minute: 30 }, RunCondition::IdleFor { minutes: 15 });
    let i_weekly = led.create("周报归档", "vxarch.run", Schedule::Weekly { weekday: 1, hour: 9, minute: 0 }, RunCondition::Always);
    let i_monthly = led.create("月度体检", "vxcheck.run", Schedule::Monthly { day: 1, hour: 12, minute: 0 }, RunCondition::OnPowerOnly);
    set.add("f441-three-step-create", led.tasks.len() == 4 && i_once == 0 && i_daily == 1, "");
    // 频率矩阵：周一 09:00 时刻——一次性未到、每日 3:30 不中、每周一 9:00 中。
    let ctx = TriggerCtx { now_min: 9 * 60, weekday: 1, day_of_month: 1, idle_ms: 20 * 60 * 1_000, on_power: true };
    set.add(
        "f441-schedule-matrix",
        !led.due(&led.tasks[i_once].clone(), &ctx)
            && !led.due(&led.tasks[i_daily].clone(), &ctx)
            && led.due(&led.tasks[i_weekly].clone(), &ctx),
        "",
    );
    // 每月 1 号 12:00：时刻与日期同时命中才发。
    let ctx_noon = TriggerCtx { now_min: 12 * 60, weekday: 1, day_of_month: 1, idle_ms: 0, on_power: true };
    set.add(
        "f441-monthly-due",
        led.due(&led.tasks[i_monthly].clone(), &ctx_noon)
            && !led.due(
                &led.tasks[i_monthly].clone(),
                &TriggerCtx { day_of_month: 2, ..ctx_noon },
            ),
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_condition_gates_run() {
        let mut led = TaskLedger::new();
        let i = led.create("空闲整理", "t.run", Schedule::Daily { hour: 3, minute: 30 }, RunCondition::IdleFor { minutes: 15 });
        let awake = TriggerCtx { now_min: 3 * 60 + 30, weekday: 3, day_of_month: 12, idle_ms: 5 * 60 * 1_000, on_power: true };
        let idle = TriggerCtx { idle_ms: 16 * 60 * 1_000, ..awake };
        let t = led.tasks[i].clone();
        assert!(!led.due(&t, &awake), "空闲不足 15 分钟：不发");
        assert!(led.due(&t, &idle), "空闲达标：发");
    }

    #[test]
    fn disabled_task_never_fires() {
        let mut led = TaskLedger::new();
        let i = led.create("已禁用", "t.run", Schedule::Daily { hour: 3, minute: 30 }, RunCondition::Always);
        led.set_enabled(i, false);
        let ctx = TriggerCtx { now_min: 3 * 60 + 30, weekday: 3, day_of_month: 12, idle_ms: 0, on_power: true };
        let t = led.tasks[i].clone();
        assert!(!led.due(&t, &ctx), "禁用即停——不触发");
        // 执行留痕与失败归因。
        led.set_enabled(i, true);
        led.record_run(i, 210, false, "目标脚本不存在——检查路径");
        led.record_run(i, 211, true, "");
        assert_eq!(led.tasks[i].runs.len(), 2);
        assert!(!led.tasks[i].runs[0].1 && led.tasks[i].runs[0].2.contains("脚本不存在"));
    }
}
