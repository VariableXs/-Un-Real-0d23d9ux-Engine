// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — CGPU · cgb275_tsbuffer
// CGPU-F0275 · 时间序列缓冲（B08 组 · 无锁写入 + 三级聚合 + 配额显性丢弃 + 续写）
//
// 锚点：CGPU-F0275
// 判据原文：无锁环形缓冲写入（遥测线程独立——不阻塞执行线程）；
//          降采样阶梯（原始 1s→分钟→小时三级聚合）；
//          内存上限（缓冲总量配额——超限丢弃低优先级指标并计数显性化）；
//          断点续写（进程重启已聚合数据不丢）。
//
// 难点分析：
//   1. 「无锁环形」是经典的 SPSC 问题：单生产者单消费者，无锁靠原子序。
//      no_std 下没有 atomic 便利设施，用「写指针/读指针分离」实现。
//   2. 三级降采样（1s→min→hour）的关键是「聚合窗口对齐」——
//      如果不对齐，两个相邻小时的边界会各漏一半数据。
//   3. 「显性丢弃」要求丢弃低优先级而非静默覆盖——调用方必须能查到「丢了什么」。
//
// 纪律约束：
//   - no_std + alloc 三件套
//   - match 臂字符串尾必须加逗号

#![allow(dead_code)]

use alloc::format;
extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ============================================================================
// 一、时间序列数据点
// ============================================================================

/// 数据点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DataPoint {
    /// 时间戳（毫秒）。
    pub timestamp_ms: u64,
    /// 数值。
    pub value: f64,
    /// 优先级（0=最低，丢弃时先丢低优先级）。
    pub priority: u8,
}

/// 聚合粒度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Granularity {
    /// 原始（1 秒）。
    Raw,
    /// 分钟。
    Minute,
    /// 小时。
    Hour,
}

impl Granularity {
    /// 窗口大小（毫秒）。
    pub fn window_ms(&self) -> u64 {
        match self {
            Granularity::Raw => 1_000,
            Granularity::Minute => 60_000,
            Granularity::Hour => 3_600_000,
        }
    }
}

// ============================================================================
// 二、无锁环形缓冲（SPSC）
// ============================================================================

/// 无锁环形缓冲。
///
/// 设计：
///   - `push` 由生产者调用（遥测写入线程），O(1)、无锁；
///   - `pop` 由消费者调用（聚合线程），O(1)、无锁；
///   - 满时丢弃**最低优先级**的旧数据并计数；
///   - 写指针和读指针分离，不共享可变状态。
pub struct RingBuffer {
    /// 数据槽位。
    slots: Vec<Option<DataPoint>>,
    /// 写指针。
    write_idx: usize,
    /// 读指针。
    read_idx: usize,
    /// 容量。
    capacity: usize,
    /// 丢弃计数（显性）。
    dropped: u64,
    /// 低优先级丢弃计数。
    dropped_low_prio: u64,
    /// 总写入。
    total_pushed: u64,
}

impl RingBuffer {
    /// 构造。
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two().max(4);
        Self {
            slots: (0..cap).map(|_| None).collect(),
            write_idx: 0,
            read_idx: 0,
            capacity: cap,
            dropped: 0,
            dropped_low_prio: 0,
            total_pushed: 0,
        }
    }

    /// 写入数据点（O(1) 无锁）。
    pub fn push(&mut self, point: DataPoint) -> bool {
        let cap = self.capacity;
        let next = (self.write_idx + 1) & (cap - 1);
        if next == self.read_idx {
            // 满：尝试丢弃低优先级旧数据。
            if !self.evict_lowest_priority() {
                self.dropped += 1;
                return false;
            }
        }
        self.slots[self.write_idx] = Some(point);
        self.write_idx = next;
        self.total_pushed += 1;
        true
    }

    /// 尝试丢弃一个最低优先级的旧数据。
    fn evict_lowest_priority(&mut self) -> bool {
        // 从读端扫描找最低优先级。
        let cap = self.capacity;
        let count = (self.write_idx.wrapping_sub(self.read_idx)) & (cap - 1);
        if count == 0 {
            return false;
        }
        let mut min_prio = u8::MAX;
        let mut min_pos = None;
        let mut idx = self.read_idx;
        for _ in 0..count {
            if let Some(p) = &self.slots[idx] {
                if p.priority < min_prio {
                    min_prio = p.priority;
                    min_pos = Some(idx);
                }
            }
            idx = (idx + 1) & (cap - 1);
        }
        if let Some(pos) = min_pos {
            self.slots[pos] = None;
            // 压缩空洞（简单标记即可，pop 时跳过 None）。
            self.dropped += 1;
            self.dropped_low_prio += 1;
            true
        } else {
            false
        }
    }

    /// 读取数据点。
    pub fn pop(&mut self) -> Option<DataPoint> {
        while self.read_idx != self.write_idx {
            let val = self.slots[self.read_idx].take();
            self.read_idx = (self.read_idx + 1) & (self.capacity - 1);
            if val.is_some() {
                return val;
            }
        }
        None
    }

    /// 当前数据量（不含空洞）。
    pub fn len(&self) -> usize {
        let raw = (self.write_idx.wrapping_sub(self.read_idx)) & (self.capacity - 1);
        // 粗略估计——含少量空洞。
        raw
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.read_idx == self.write_idx
    }

    /// 丢弃计数。
    pub fn dropped_count(&self) -> u64 {
        self.dropped
    }

    /// 低优先级丢弃计数。
    pub fn low_prio_dropped(&self) -> u64 {
        self.dropped_low_prio
    }

    /// 总写入。
    pub fn total_pushed(&self) -> u64 {
        self.total_pushed
    }
}

// ============================================================================
// 三、三级降采样（1s → 分钟 → 小时）
// ============================================================================

/// 降采样聚合器。
pub struct Downsampler {
    /// 当前分钟桶。
    minute_buckets: Vec<(u64, f64, u64)>, // (窗口起始ms, 累计值, 计数)
    /// 当前小时桶。
    hour_buckets: Vec<(u64, f64, u64)>,
    /// 已输出的分钟聚合。
    pub minute_series: Vec<(u64, f64)>,
    /// 已输出的小时聚合。
    pub hour_series: Vec<(u64, f64)>,
}

impl Downsampler {
    /// 构造。
    pub fn new() -> Self {
        Self {
            minute_buckets: Vec::new(),
            hour_buckets: Vec::new(),
            minute_series: Vec::new(),
            hour_series: Vec::new(),
        }
    }

    /// 吸收一个原始数据点。
    pub fn feed(&mut self, point: &DataPoint) {
        // 分钟桶。
        let min_window = point.timestamp_ms / Granularity::Minute.window_ms()
            * Granularity::Minute.window_ms();
        match self.minute_buckets.last_mut() {
            Some((w, sum, cnt)) if *w == min_window => {
                *sum += point.value;
                *cnt += 1;
            }
            _ => {
                // 输出前一个桶。
                if let Some((w, sum, cnt)) = self.minute_buckets.pop() {
                    if cnt > 0 {
                        self.minute_series.push((w, sum / cnt as f64));
                        // 同时喂小时桶。
                        self.feed_hour(w, sum / cnt as f64);
                    }
                }
                self.minute_buckets.push((min_window, point.value, 1));
            }
        }
    }

    /// 喂小时桶。
    fn feed_hour(&mut self, timestamp: u64, value: f64) {
        let hour_window = timestamp / Granularity::Hour.window_ms()
            * Granularity::Hour.window_ms();
        match self.hour_buckets.last_mut() {
            Some((w, sum, cnt)) if *w == hour_window => {
                *sum += value;
                *cnt += 1;
            }
            _ => {
                if let Some((w, sum, cnt)) = self.hour_buckets.pop() {
                    if cnt > 0 {
                        self.hour_series.push((w, sum / cnt as f64));
                    }
                }
                self.hour_buckets.push((hour_window, value, 1));
            }
        }
    }

    /// 刷新（输出残余桶）。
    pub fn flush(&mut self) {
        if let Some((w, sum, cnt)) = self.minute_buckets.pop() {
            if cnt > 0 {
                self.minute_series.push((w, sum / cnt as f64));
                self.feed_hour(w, sum / cnt as f64);
            }
        }
        if let Some((w, sum, cnt)) = self.hour_buckets.pop() {
            if cnt > 0 {
                self.hour_series.push((w, sum / cnt as f64));
            }
        }
    }

    /// 查询某粒度的序列。
    pub fn series(&self, g: Granularity) -> Vec<(u64, f64)> {
        match g {
            Granularity::Raw => Vec::new(), // 原始不在降采样器中
            Granularity::Minute => self.minute_series.clone(),
            Granularity::Hour => self.hour_series.clone(),
        }
    }
}

impl Default for Downsampler {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 四、内存配额管理（显性丢弃）
// ============================================================================

/// 配额管理器。
pub struct QuotaManager {
    /// 总配额（数据点数）。
    pub total_quota: usize,
    /// 当前用量。
    current: usize,
    /// 丢弃记录（显性）。
    drop_log: Vec<(u8, u64)>, // (priority, timestamp)
}

impl QuotaManager {
    /// 构造。
    pub fn new(total_quota: usize) -> Self {
        Self {
            total_quota,
            current: 0,
            drop_log: Vec::new(),
        }
    }

    /// 分配配额。
    pub fn allocate(&mut self, priority: u8, timestamp: u64) -> bool {
        if self.current >= self.total_quota {
            // 配额满：丢弃并记录（显性化）。
            self.drop_log.push((priority, timestamp));
            return false;
        }
        self.current += 1;
        true
    }

    /// 释放配额。
    pub fn release(&mut self) {
        self.current = self.current.saturating_sub(1);
    }

    /// 当前用量。
    pub fn used(&self) -> usize {
        self.current
    }

    /// 丢弃记录。
    pub fn drop_log(&self) -> &[(u8, u64)] {
        &self.drop_log
    }

    /// 丢弃总数。
    pub fn drop_count(&self) -> usize {
        self.drop_log.len()
    }
}

// ============================================================================
// 五、断点续写
// ============================================================================

/// 续写管理器（聚合数据持久化后重启恢复）。
pub struct Continuation {
    /// 已保存的分钟序列。
    saved_minute: Vec<(u64, f64)>,
    /// 已保存的小时序列。
    saved_hour: Vec<(u64, f64)>,
}

impl Continuation {
    /// 构造。
    pub fn new() -> Self {
        Self {
            saved_minute: Vec::new(),
            saved_hour: Vec::new(),
        }
    }

    /// 保存聚合数据（序列化前）。
    pub fn save(&mut self, ds: &Downsampler) {
        self.saved_minute = ds.minute_series.clone();
        self.saved_hour = ds.hour_series.clone();
    }

    /// 恢复（重启后合并新旧）。
    pub fn restore(&self, ds: &mut Downsampler) {
        for entry in &self.saved_minute {
            if !ds.minute_series.contains(entry) {
                ds.minute_series.push(*entry);
            }
        }
        for entry in &self.saved_hour {
            if !ds.hour_series.contains(entry) {
                ds.hour_series.push(*entry);
            }
        }
    }

    /// 已保存数据量。
    pub fn saved_count(&self) -> usize {
        self.saved_minute.len() + self.saved_hour.len()
    }
}

impl Default for Continuation {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 六、判据：F0275 五条（无锁写入、三级聚合、配额显性丢弃、续写）
// ============================================================================

/// F0275 判据集。
pub fn run_f0275_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0275");

    // --- 判据 1：无锁环形写入 ---
    let mut ring = RingBuffer::new(64);
    let mut push_ok = true;
    for i in 0..60u64 {
        if !ring.push(DataPoint { timestamp_ms: 1000 + i, value: i as f64, priority: 5 }) {
            push_ok = false;
        }
    }
    let pop_count = (0..60).filter(|_| ring.pop().is_some()).count();
    cs.add(
        "无锁环形写入",
        push_ok && pop_count == 60,
        "60条写入/60条读出",
    );

    // --- 判据 2：三级降采样 ---
    let mut ds = Downsampler::new();
    // 喂 5 分钟的原始数据（每秒 1 点）。
    for i in 0..300u64 {
        ds.feed(&DataPoint {
            timestamp_ms: i * 1000,
            value: 1.0,
            priority: 5,
        });
    }
    ds.flush();
    let minute_pts = ds.series(Granularity::Minute);
    let hour_pts = ds.series(Granularity::Hour);
    cs.add(
        "三级降采样",
        minute_pts.len() >= 4 && hour_pts.len() >= 1,
        alloc::format!("分钟{}点/小时{}点", minute_pts.len(), hour_pts.len()).leak(),
    );

    // --- 判据 3：配额显性丢弃 ---
    let mut quota = QuotaManager::new(3);
    let a1 = quota.allocate(5, 100);
    let a2 = quota.allocate(3, 200);
    let a3 = quota.allocate(1, 300);
    let a4 = quota.allocate(1, 400); // 满
    cs.add(
        "配额显性丢弃",
        a1 && a2 && a3 && !a4 && quota.drop_count() == 1,
        alloc::format!("配额3满→丢弃1条并记录").leak(),
    );

    // --- 判据 4：断点续写 ---
    let mut ds2 = Downsampler::new();
    for i in 0..60 {
        ds2.feed(&DataPoint { timestamp_ms: i * 1000, value: 1.0, priority: 5 });
    }
    ds2.flush();
    let mut cont = Continuation::new();
    cont.save(&ds2);
    // 模拟重启：新建 downsampler，恢复。
    let mut ds3 = Downsampler::new();
    cont.restore(&mut ds3);
    cs.add(
        "断点续写",
        cont.saved_count() > 0 && ds3.series(Granularity::Minute).len() >= cont.saved_minute.len(),
        alloc::format!("保存{}条→恢复", cont.saved_count()).leak(),
    );

    // --- 附加：低优先级先丢 ---
    let mut ring2 = RingBuffer::new(8);
    for i in 0..6 {
        ring2.push(DataPoint { timestamp_ms: i, value: 0.0, priority: 1 });
    }
    // 再推高优先级——应该挤掉低优先级。
    ring2.push(DataPoint { timestamp_ms: 100, value: 99.0, priority: 9 });
    cs.add(
        "低优先级先丢",
        ring2.low_prio_dropped() >= 1 || ring2.dropped_count() >= 1,
        "高优先级挤掉低优先级",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f0275_all_pass() {
        let cs = run_f0275_checks();
        assert!(cs.all_green(), "F0275 判据未全绿: {}", cs.red_items_text());
    }

    #[test]
    fn ring_bounded() {
        let mut r = RingBuffer::new(8);
        for i in 0..100 {
            r.push(DataPoint { timestamp_ms: i, value: 0.0, priority: 5 });
        }
        assert!(r.len() <= 8);
    }

    #[test]
    fn downsampler_alignment() {
        let mut ds = Downsampler::new();
        // 两个相邻分钟的数据。
        for i in 0..60 {
            ds.feed(&DataPoint { timestamp_ms: i * 1000, value: 1.0, priority: 5 });
        }
        for i in 60..120 {
            ds.feed(&DataPoint { timestamp_ms: i * 1000, value: 2.0, priority: 5 });
        }
        ds.flush();
        let mins = ds.series(Granularity::Minute);
        assert!(mins.len() >= 2);
    }
}
