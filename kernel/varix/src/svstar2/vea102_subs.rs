//! VE-F0102 · 性能异常订阅接口（VE-A 域 · A04b 诊断·降级·安全组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0102`
//!
//! **职责定位（锚点原文）**：提供性能异常订阅模型——订阅者按指标与阈值注册回调，
//! 异常触发时按序投递事件信封；投递失败不阻塞采集主路，重试有上限后停用该订阅
//! 并通知注册方；订阅注册表可枚举可注销，重复注册幂等合并。
//!
//! **铁律**：
//! 1. **有界投递**——串行有界队列防淹没。
//! 2. **幂等注册**——重复注册合并不重复。
//! 3. **停用机制**——回调失败达上限停用并通知。
//! 4. **零开销**——无订阅时直通零开销。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8AD（异常订阅）。
pub const SEG_BASE: u16 = 0xE8AD;

/// 重复订阅（幂等合并）。
pub const XC_DUP_SUB: u16 = SEG_BASE | 0x01;
/// 订阅不存在。
pub const XC_NO_SUB: u16 = SEG_BASE | 0x02;
/// 投递队列满。
pub const XC_QUEUE_FULL: u16 = SEG_BASE | 0x03;
/// 订阅已停用。
pub const XC_SUB_DISABLED: u16 = SEG_BASE | 0x04;
/// 接口已关闭。
pub const XC_CLOSED: u16 = SEG_BASE | 0x05;

/// 事件信封。
#[derive(Clone, Copy, Debug)]
pub struct EventEnvelope {
    pub tick: u64,
    /// 观测值。
    pub value: f64,
    /// 方向（1=超上限 -1=破下限）。
    pub direction: i8,
    pub metric_id: u32,
}

/// 订阅状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubState {
    Active,
    Disabled,
}

/// 订阅记录。
#[derive(Clone, Debug)]
pub struct Subscription {
    pub id: u32,
    pub metric_id: u32,
    pub threshold: f64,
    /// 阈值方向：true=超上限触发。
    pub is_upper: bool,
    pub state: SubState,
    /// 连续失败计数。
    pub fail_streak: u32,
    /// 回调目标 id（模拟回调句柄）。
    pub callback_id: u32,
}

/// 有界投递队列。
#[derive(Clone, Debug)]
struct BoundedQueue {
    items: Vec<EventEnvelope>,
    cap: usize,
}

impl BoundedQueue {
    fn new(cap: usize) -> Self {
        BoundedQueue {
            items: Vec::new(),
            cap: if cap == 0 { 32 } else { cap },
        }
    }

    fn push(&mut self, e: EventEnvelope) -> Result<(), u16> {
        if self.items.len() >= self.cap {
            return Err(XC_QUEUE_FULL);
        }
        self.items.push(e);
        Ok(())
    }

    fn pop(&mut self) -> Option<EventEnvelope> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.items.remove(0))
        }
    }
}

/// 订阅接口。
pub struct AnomalySubscriptions {
    subs: Vec<Subscription>,
    queue: BoundedQueue,
    next_id: u32,
    closed: bool,
    /// 重试上限。
    max_retries: u32,
    last_code: u16,
    /// 停用通知计数。
    disable_notices: u32,
    /// 投递成功计数。
    delivered: u64,
    /// 投递失败计数。
    failed: u64,
}

impl AnomalySubscriptions {
    pub fn new(queue_cap: usize, max_retries: u32) -> Self {
        AnomalySubscriptions {
            subs: Vec::new(),
            queue: BoundedQueue::new(queue_cap),
            next_id: 1,
            closed: false,
            max_retries: if max_retries == 0 { 3 } else { max_retries },
            last_code: 0,
            disable_notices: 0,
            delivered: 0,
            failed: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 注册订阅：重复注册幂等合并（同 metric+阈值+方向）。
    pub fn subscribe(
        &mut self,
        metric_id: u32,
        threshold: f64,
        is_upper: bool,
        callback_id: u32,
    ) -> Result<u32, u16> {
        if self.closed {
            self.last_code = XC_CLOSED;
            return Err(XC_CLOSED);
        }
        // 幂等：已存在同参订阅直接返回既有 id。
        if let Some(s) = self
            .subs
            .iter()
            .find(|s| s.metric_id == metric_id && s.threshold == threshold && s.is_upper == is_upper)
        {
            self.last_code = XC_DUP_SUB;
            return Ok(s.id);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.subs.push(Subscription {
            id,
            metric_id,
            threshold,
            is_upper,
            state: SubState::Active,
            fail_streak: 0,
            callback_id,
        });
        self.last_code = 0;
        Ok(id)
    }

    /// 注销订阅。
    pub fn unsubscribe(&mut self, id: u32) -> Result<(), u16> {
        let before = self.subs.len();
        self.subs.retain(|s| s.id != id);
        if self.subs.len() == before {
            self.last_code = XC_NO_SUB;
            return Err(XC_NO_SUB);
        }
        self.last_code = 0;
        Ok(())
    }

    /// 采样点判定触发（O(1)）：无订阅时零开销直通。
    pub fn observe(&mut self, metric_id: u32, value: f64, tick: u64) -> Result<usize, u16> {
        if self.closed {
            self.last_code = XC_CLOSED;
            return Err(XC_CLOSED);
        }
        // 无任何匹配订阅 → 直通零开销。
        let matching: Vec<u32> = self
            .subs
            .iter()
            .filter(|s| {
                s.metric_id == metric_id
                    && s.state == SubState::Active
                    && if s.is_upper {
                        value > s.threshold
                    } else {
                        value < s.threshold
                    }
            })
            .map(|s| s.id)
            .collect();
        if matching.is_empty() {
            self.last_code = 0;
            return Ok(0);
        }
        let mut triggered = 0;
        for id in matching {
            let dir = match self.subs.iter().find(|s| s.id == id) {
                Some(s) => {
                    if s.is_upper {
                        1
                    } else {
                        -1
                    }
                }
                None => continue,
            };
            let env = EventEnvelope {
                tick,
                value,
                direction: dir,
                metric_id,
            };
            match self.queue.push(env) {
                Ok(()) => triggered += 1,
                Err(_) => {
                    self.last_code = XC_QUEUE_FULL;
                }
            }
        }
        Ok(triggered)
    }

    /// 投递一批（串行有界）。cb_ok 模拟回调结果（true 成功）。
    pub fn deliver_one(&mut self, cb_ok: bool) -> Result<EventEnvelope, u16> {
        if self.closed {
            self.last_code = XC_CLOSED;
            return Err(XC_CLOSED);
        }
        let env = match self.queue.pop() {
            Some(e) => e,
            None => {
                self.last_code = XC_NO_SUB;
                return Err(XC_NO_SUB);
            }
        };
        if cb_ok {
            self.delivered += 1;
            // 成功清零失败链。
            for s in self.subs.iter_mut() {
                if s.metric_id == env.metric_id {
                    s.fail_streak = 0;
                }
            }
            self.last_code = 0;
            Ok(env)
        } else {
            self.failed += 1;
            // 失败累计，达上限停用并通知。
            for s in self.subs.iter_mut() {
                if s.metric_id == env.metric_id && s.state == SubState::Active {
                    s.fail_streak += 1;
                    if s.fail_streak >= self.max_retries {
                        s.state = SubState::Disabled;
                        self.disable_notices += 1;
                    }
                }
            }
            self.last_code = XC_SUB_DISABLED;
            Err(XC_SUB_DISABLED)
        }
    }

    /// 接口关闭：全部注销并逐一通知。
    pub fn shutdown(&mut self) -> usize {
        let n = self.subs.len();
        self.disable_notices += n as u32;
        self.subs.clear();
        self.closed = true;
        n
    }

    pub fn list(&self) -> &[Subscription] {
        &self.subs
    }

    pub fn active_count(&self) -> usize {
        self.subs
            .iter()
            .filter(|s| s.state == SubState::Active)
            .count()
    }

    pub fn queue_len(&self) -> usize {
        self.queue.items.len()
    }

    pub fn delivered(&self) -> u64 {
        self.delivered
    }

    pub fn failed(&self) -> u64 {
        self.failed
    }

    pub fn disable_notices(&self) -> u32 {
        self.disable_notices
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

/// 自检：有界投递、幂等注册、停用机制、零开销。
pub fn run_vea102_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea102_subs");

    // C102-01 注册订阅
    {
        let mut api = AnomalySubscriptions::new(16, 3);
        let r = api.subscribe(1, 100.0, true, 42);
        chk(&mut set, "C102-01 注册", r.is_ok() && api.active_count() == 1, "注册失败");
    }

    // C102-02 幂等合并
    {
        let mut api = AnomalySubscriptions::new(16, 3);
        let a = api.subscribe(1, 100.0, true, 42).unwrap();
        let b = api.subscribe(1, 100.0, true, 99).unwrap();
        let ok = a == b && api.active_count() == 1;
        chk(&mut set, "C102-02 幂等合并", ok, "重复注册分裂");
    }

    // C102-03 触发入队
    {
        let mut api = AnomalySubscriptions::new(16, 3);
        let _ = api.subscribe(1, 100.0, true, 1);
        let r = api.observe(1, 150.0, 1);
        chk(&mut set, "C102-03 触发入队", r == Ok(1) && api.queue_len() == 1, "未入队");
    }

    // C102-04 零开销直通（无订阅）
    {
        let mut api = AnomalySubscriptions::new(16, 3);
        let r = api.observe(9, 999.0, 1);
        chk(&mut set, "C102-04 零开销", r == Ok(0) && api.queue_len() == 0, "直通失败");
    }

    // C102-05 有界队列防淹没
    {
        let mut api = AnomalySubscriptions::new(2, 3);
        let _ = api.subscribe(1, 0.0, true, 1);
        let _ = api.observe(1, 10.0, 1);
        let _ = api.observe(1, 10.0, 2);
        let r = api.observe(1, 10.0, 3);
        let ok = r == Ok(0) && api.last_code() == XC_QUEUE_FULL;
        chk(&mut set, "C102-05 有界队列", ok, "队列溢出未报");
    }

    // C102-06 投递成功清失败链
    {
        let mut api = AnomalySubscriptions::new(16, 3);
        let _ = api.subscribe(1, 0.0, true, 1);
        let _ = api.observe(1, 5.0, 1);
        let r = api.deliver_one(true);
        let ok = r.is_ok() && api.delivered() == 1;
        chk(&mut set, "C102-06 投递成功", ok, "投递失败");
    }

    // C102-07 失败达上限停用
    {
        let mut api = AnomalySubscriptions::new(16, 2);
        let _ = api.subscribe(1, 0.0, true, 1);
        for i in 0..3 {
            let _ = api.observe(1, 5.0, i);
            let _ = api.deliver_one(false);
        }
        let ok = api.active_count() == 0 && api.disable_notices() >= 1;
        chk(&mut set, "C102-07 停用机制", ok, "未停用");
    }

    // C102-08 注销
    {
        let mut api = AnomalySubscriptions::new(16, 3);
        let id = api.subscribe(1, 1.0, true, 1).unwrap();
        let r = api.unsubscribe(id);
        chk(&mut set, "C102-08 注销", r.is_ok() && api.active_count() == 0, "注销失败");
    }

    // C102-09 关闭接口全注销并通知
    {
        let mut api = AnomalySubscriptions::new(16, 3);
        let _ = api.subscribe(1, 1.0, true, 1);
        let _ = api.subscribe(2, 1.0, true, 2);
        let n = api.shutdown();
        let ok = n == 2 && api.is_closed() && api.active_count() == 0;
        chk(&mut set, "C102-09 关闭全注销", ok, "关闭未清");
    }

    // C102-10 关闭后注册拒绝
    {
        let mut api = AnomalySubscriptions::new(16, 3);
        api.shutdown();
        let r = api.subscribe(1, 1.0, true, 1);
        chk(
            &mut set,
            "C102-10 关闭拒注",
            r.is_err() && api.last_code() == XC_CLOSED,
            "关闭后仍收",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea102_all_green() {
        let s = run_vea102_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
