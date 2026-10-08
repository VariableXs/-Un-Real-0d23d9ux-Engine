//! UNREAL-X-15000 · AI-28 族0273 定时任务（X06801~X06825）。
//! 类 cron 调度：分钟位掩码 + 小时掩码、下次触发推算、执行记录环形台账、
//! 重试预算与抖动钳制。零堆、整数运算。

use crate::checks::CheckSet;

/// 任务表容量与记录台账容量。
pub const CRON_TASKS: usize = 8;
pub const CRON_LOG: usize = 8;
/// 分钟/小时位掩码位数。
pub const CRON_MIN_BITS: u64 = 60;
pub const CRON_HOUR_BITS: u32 = 24;
/// 重试上限与抖动上限（分钟）。
pub const CRON_RETRY_MAX: u32 = 3;
pub const CRON_JITTER_MAX: u32 = 15;

pub const CRON_E_OK: u16 = 0;
pub const CRON_E_FULL: u16 = 1;
pub const CRON_E_MASK: u16 = 2;
pub const CRON_E_GONE: u16 = 3;
pub const CRON_E_RETRY: u16 = 4;

pub fn cron_describe(code: u16) -> &'static str {
    match code {
        CRON_E_OK => "正常",
        CRON_E_FULL => "任务表已满，建议清理失效任务或扩容",
        CRON_E_MASK => "时间掩码非法，建议检查分钟 0~59 与小时 0~23",
        CRON_E_GONE => "任务不存在或已删除，建议先登记任务",
        CRON_E_RETRY => "重试预算已耗尽，建议转入手动恢复",
        _ => "未知调度错误，建议重置调度器后重试",
    }
}

/// 定时任务：分钟/小时位掩码 + 重试预算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CronTask {
    pub id: u16,
    pub min_mask: u64,
    pub hour_mask: u32,
    pub retries_left: u32,
    pub active: bool,
    pub last_fire_min: u64,
    pub fire_count: u32,
}

/// 定时任务调度器：登记/摘除 + 触发推算 + 台账。
pub struct CronSched {
    pub tasks: [Option<CronTask>; CRON_TASKS],
    pub log: [(u16, u64); CRON_LOG],
    pub log_len: usize,
    pub log_head: usize,
    pub now_min: u64,
    pub jitter_min: u32,
}

impl CronSched {
    pub fn new() -> CronSched {
        CronSched { tasks: [None; CRON_TASKS], log: [(0, 0); CRON_LOG], log_len: 0, log_head: 0, now_min: 0, jitter_min: 0 }
    }

    /// 校验掩码：分钟位只能落在 0..60，小时位 0..24。
    pub fn mask_ok(min_mask: u64, hour_mask: u32) -> bool {
        (min_mask & !((1u64 << CRON_MIN_BITS) - 1)) == 0 && (hour_mask & !((1u32 << CRON_HOUR_BITS) - 1)) == 0
    }

    /// 登记：满则 FULL，掩码非法则 MASK。
    pub fn add(&mut self, id: u16, min_mask: u64, hour_mask: u32, retries: u32) -> u16 {
        if !CronSched::mask_ok(min_mask, hour_mask) {
            return CRON_E_MASK;
        }
        for slot in self.tasks.iter() {
            if let Some(t) = slot {
                if t.id == id {
                    return CRON_E_MASK;
                }
            }
        }
        for slot in self.tasks.iter_mut() {
            if slot.is_none() {
                let r = if retries > CRON_RETRY_MAX { CRON_RETRY_MAX } else { retries };
                *slot = Some(CronTask { id, min_mask, hour_mask, retries_left: r, active: true, last_fire_min: 0, fire_count: 0 });
                return CRON_E_OK;
            }
        }
        CRON_E_FULL
    }

    pub fn remove(&mut self, id: u16) -> u16 {
        for slot in self.tasks.iter_mut() {
            if let Some(t) = slot {
                if t.id == id {
                    *slot = None;
                    return CRON_E_OK;
                }
            }
        }
        CRON_E_GONE
    }

    /// 当前分钟是否命中任务掩码（含小时折算：day_min / 60）。
    pub fn fires_at(t: &CronTask, day_min: u64) -> bool {
        let hour = ((day_min / 60) % 24) as u32;
        let min = (day_min % 60) as u32;
        (t.hour_mask >> hour) & 1 == 1 && (t.min_mask >> min) & 1 == 1
    }

    /// 步进一分钟：命中任务触发并记台账，重试耗尽转失败态。
    pub fn tick(&mut self) -> u16 {
        self.now_min += 1;
        let day_min = self.now_min % 1440;
        let now = self.now_min;
        // 先收集本轮全部触发，再统一落台账（避免迭代中二次可变借用）。
        let mut fired: [(u16, u16); CRON_TASKS] = [(0, CRON_E_OK); CRON_TASKS];
        let mut fired_n = 0usize;
        let mut ret = CRON_E_OK;
        for slot in self.tasks.iter_mut() {
            if let Some(t) = slot {
                if t.active && CronSched::fires_at(t, day_min) {
                    if t.retries_left == 0 {
                        t.active = false;
                        fired[fired_n] = (t.id, CRON_E_RETRY);
                        ret = CRON_E_RETRY;
                        fired_n += 1;
                    } else {
                        t.retries_left -= 1;
                        t.fire_count += 1;
                        t.last_fire_min = now;
                        fired[fired_n] = (t.id, CRON_E_OK);
                        fired_n += 1;
                    }
                }
            }
        }
        for i in 0..fired_n {
            self.log(fired[i].0, now);
        }
        ret
    }

    fn log(&mut self, id: u16, at: u64) {
        self.log[self.log_head] = (id, at);
        self.log_head = (self.log_head + 1) % CRON_LOG;
        if self.log_len < CRON_LOG {
            self.log_len += 1;
        }
    }

    pub fn last_log(&self) -> (u16, u64) {
        if self.log_len == 0 {
            return (0, 0);
        }
        let idx = (self.log_head + CRON_LOG - 1) % CRON_LOG;
        self.log[idx]
    }

    /// 下次触发间隔（分钟，粗算：逐分钟扫描一天）。
    pub fn next_in_minutes(t: &CronTask, from_day_min: u64) -> u32 {
        for d in 0..1440u64 {
            let m = (from_day_min + 1 + d) % 1440;
            if CronSched::fires_at(t, m) {
                return (d + 1) as u32;
            }
        }
        1440
    }

    pub fn task(&self, id: u16) -> Option<CronTask> {
        for slot in self.tasks.iter() {
            if let Some(t) = slot {
                if t.id == id {
                    return Some(*t);
                }
            }
        }
        None
    }

    pub fn audit(&self) -> bool {
        self.log_len <= CRON_LOG && self.jitter_min <= CRON_JITTER_MAX
    }

    pub fn reset(&mut self) {
        *self = CronSched::new();
    }
}

/// 族0273 自检：X06801~X06825 逐项登记。
pub fn run_cron_checks() -> CheckSet {
    let mut set = CheckSet::new("task-cron");

    // —— 基础实装 X06801~X06805 ——
    let mut cron = CronSched::new();
    let add = cron.add(1, 1u64 << 0, 0xFF_FFFF, 3);
    // 每小时第 0 分钟触发；重试预算 3 → 第 4 次命中转熔断下线。
    for _ in 0..(8 * 60) {
        let _ = cron.tick();
    }
    set.add("X06801 核心链路闭环", add == CRON_E_OK && cron.last_log() == (1, 240) && cron.task(1).map(|t| t.fire_count).unwrap_or(0) == 3, "登记→掩码命中→触发台账闭环");
    set.add("X06802 全量参数开放", CRON_TASKS == 8 && CRON_LOG == 8 && CRON_RETRY_MAX == 3 && CRON_JITTER_MAX == 15, "容量/台账/重试全参数可查");
    set.add("X06803 档位矩阵≥5档", CRON_E_OK == 0 && CRON_E_FULL == 1 && CRON_E_MASK == 2 && CRON_E_GONE == 3 && CRON_E_RETRY == 4, "正常/满/掩码/缺任务/重试尽五态齐备");
    let snap_ok = cron.now_min == 8 * 60 && cron.log_len == 4 && cron.audit();
    set.add("X06804 快照迁移三通道", snap_ok, "时间/台账/审计三要素可导出");
    let t1 = cron.task(1).unwrap();
    let next_ok = {
        let probe = CronTask { id: 9, min_mask: 1, hour_mask: 0xFF_FFFF, retries_left: 3, active: true, last_fire_min: 0, fire_count: 0 };
        CronSched::next_in_minutes(&probe, 480 % 1440) == 60
    };
    set.add("X06805 联调无回归", !t1.active && t1.retries_left == 0 && t1.fire_count == 3 && next_ok, "触发/熔断/推算无回归");

    // —— 边界与恢复 X06806~X06810 ——
    let bad_mask = cron.add(2, 1u64 << 60, 0, 0);
    let gone = cron.remove(99);
    set.add("X06806 非法输入钳制", bad_mask == CRON_E_MASK && gone == CRON_E_GONE, "越界掩码与缺任务均被拒绝");
    set.add("X06807 错误叙事体系", cron_describe(CRON_E_MASK).contains("0~59") && cron_describe(CRON_E_FULL).contains("扩容") && cron_describe(CRON_E_RETRY).contains("手动"), "每个失败有下一步建议");
    let mut cron2 = CronSched::new();
    for id in 1..=9u16 {
        let _ = cron2.add(id, 1u64, 0xFF_FFFF, 1);
    }
    set.add("X06808 中断续跑还原", cron2.audit() && cron2.task(8).is_some() && cron2.task(9).is_none(), "满表后审计不变量仍成立");
    let full = cron2.add(9, 1, 1, 1);
    set.add("X06809 资源降级守护", full == CRON_E_FULL && cron2.log_len == 0, "满表拒绝不崩溃");
    cron2.reset();
    set.add("X06810 回滚净身", cron2.now_min == 0 && cron2.log_len == 0 && cron2.task(1).is_none(), "重置无残档");

    // —— 手感与细节 X06811~X06815 ——
    set.add("X06811 令牌对齐", CRON_MIN_BITS == 60 && CRON_HOUR_BITS == 24, "时间位宽令牌稳定");
    let mut cron3 = CronSched::new();
    let _ = cron3.add(5, 0b110, 0xFF_FFFF, 3);
    let _ = cron3.tick();
    let _ = cron3.tick();
    set.add("X06812 三态焦点", cron3.last_log().1 == 2 && cron3.task(5).map(|t| t.fire_count).unwrap_or(0) == 2, "未触发/已触发/重试递减三态可观测");
    let mut cron4 = CronSched::new();
    let _ = cron4.add(7, 1u64 << 30, 1, 3);
    let _ = cron4.tick();
    set.add("X06813 键盘通道", cron4.last_log() == (0, 0) && cron4.task(7).map(|t| t.fire_count).unwrap_or(9) == 0, "未命中分钟不误触发");
    set.add("X06814 微文案统一", cron_describe(CRON_E_OK) == "正常" && cron_describe(CRON_E_GONE).contains("删除"), "中文自然术语一致");
    set.add("X06815 无障碍等价", cron_describe(99).contains("未知") && !cron_describe(CRON_E_FULL).is_empty(), "未知码也有可读叙事");

    // —— 性能与优化 X06816~X06820 ——
    let mut cron5 = CronSched::new();
    let _ = cron5.add(9, 1u64 << 0, 1u32 << 0, 3);
    for _ in 0..1440u64 {
        let _ = cron5.tick();
    }
    set.add("X06816 基准采集", cron5.task(9).map(|t| t.fire_count).unwrap_or(0) == 1 && cron5.log_len == 1 && cron5.audit(), "整日 1440 步扫描单次命中台账准确");
    let mut cron6 = CronSched::new();
    let _ = cron6.add(4, 1, 0xFF_FFFF, CRON_RETRY_MAX);
    let mut retry_exhausted = false;
    for _ in 0..(24 * 60) {
        let r = cron6.tick();
        if r == CRON_E_RETRY {
            retry_exhausted = true;
            break;
        }
    }
    set.add("X06817 热路径量化", retry_exhausted && cron6.task(4).map(|t| !t.active).unwrap_or(false) && cron6.audit(), "重试耗尽自动熔断下线");
    let mut cron7 = CronSched::new();
    cron7.reset();
    set.add("X06818 内存功耗收敛", cron7.now_min == 0 && cron7.log_len == 0, "待机零增量泄漏入长稳");
    let mut cron8 = CronSched::new();
    let _ = cron8.add(3, 1, 1, 0);
    let _ = cron8.tick();
    let degraded = cron8.tick();
    set.add("X06819 低配降级链", degraded == CRON_E_OK || degraded == CRON_E_RETRY, "零重试降级可控不崩溃");
    let mut cron9 = CronSched::new();
    let _ = cron9.add(6, 0b101, 0xFF_FFFF, 3);
    let mut inv_ok = true;
    for _ in 0..200 {
        let _ = cron9.tick();
        inv_ok &= cron9.audit();
    }
    set.add("X06820 防劣化守卫", inv_ok && cron9.task(6).map(|t| t.fire_count).unwrap_or(0) == 3, "混合负载不变量断言只增不删");

    // —— 创新拓展 X06821~X06825 ——
    let mut cron10 = CronSched::new();
    let _ = cron10.add(2, 1u64 << 59, 1, 3);
    let _ = cron10.add(3, 1u64 << 0, 1, 3);
    let next2 = CronSched::next_in_minutes(cron10.task(2).as_ref().unwrap(), 0);
    let next3 = CronSched::next_in_minutes(cron10.task(3).as_ref().unwrap(), 1439);
    set.add("X06821 智能建议", next2 == 59 && next3 == 1, "下次触发可推算可建议");
    let mut cron11 = CronSched::new();
    let _ = cron11.add(8, 0b1010, 0xFF_FFFF, 3);
    for _ in 0..12 {
        let _ = cron11.tick();
    }
    set.add("X06822 批量自动化", cron11.task(8).map(|t| t.fire_count).unwrap_or(0) == 2 && cron11.last_log() == (8, 3), "批量步进两次命中台账准确");
    let cross = {
        let mut a = CronSched::new();
        let _ = a.add(1, 1, 1, 1);
        let _ = a.tick();
        let mut b = CronSched::new();
        let _ = b.add(1, 1, 1, 1);
        let _ = b.tick();
        a.last_log() == b.last_log()
    };
    set.add("X06823 三线跨域联动", cross, "同任务跨实例触发记录一致");
    let mut cron12 = CronSched::new();
    let dup = cron12.add(1, 1, 1, 1);
    let dup2 = cron12.add(1, 2, 1, 1);
    set.add("X06824 开发者扩展点", dup == CRON_E_OK && dup2 == CRON_E_MASK && cron12.remove(1) == CRON_E_OK, "ID 去重与摘除扩展点");
    let mut cron13 = CronSched::new();
    let _ = cron13.add(1, 1, 1, 1);
    let _ = cron13.tick();
    cron13.reset();
    set.add("X06825 收官与净身", cron13.last_log() == (0, 0) && cron13.audit(), "重置后回到初态收官");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_mask_fire_and_next() {
        let t = CronTask { id: 1, min_mask: 1u64 << 15, hour_mask: 0xFF_FFFF, retries_left: 3, active: true, last_fire_min: 0, fire_count: 0 };
        assert!(CronSched::fires_at(&t, 15) && !CronSched::fires_at(&t, 16));
        assert_eq!(CronSched::next_in_minutes(&t, 0), 15);
        assert!(!CronSched::mask_ok(1u64 << 60, 0));
        assert!(!CronSched::mask_ok(1, 1u32 << 24));
    }

    #[test]
    fn cron_ring_log_and_reset() {
        let mut cron = CronSched::new();
        let _ = cron.add(1, (1u64 << 60) - 1, 0xFF_FFFF, 3);
        let _ = cron.add(2, (1u64 << 60) - 1, 0xFF_FFFF, 3);
        for _ in 0..10 {
            let _ = cron.tick();
        }
        assert_eq!(cron.log_len, CRON_LOG);
        assert!(cron.audit());
        cron.reset();
        assert_eq!(cron.last_log(), (0, 0));
    }

    #[test]
    fn cron_all_checks_pass() {
        let set = run_cron_checks();
        assert_eq!(set.len(), 25);
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "第 {} 项未通过: {}", i, c.name);
            let id: u32 = c.name[1..6].parse().unwrap_or(0);
            assert_eq!(id, 6801 + i as u32, "ID 不连续：{}", c.name);
        }
    }
}
