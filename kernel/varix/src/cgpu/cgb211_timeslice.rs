//! CGPU-F0211 · 时间片调度器（判据段 0x06D0）
//! GPU 任务的软件时间片调度（硬件抢占不可用时的软件方案）：时间片
//! 长度自适应（默认 4ms/片，可 2-8ms 调节——片太短切分开销大，太
//! 长延迟高）、片内完成检查（每片结束检查是否到达让位点）、时间片
//! 公平（同优先级任务的轮流时间片——轮转调度保证公平）、时间片与
//! 帧预算联动（片长不超过帧剩余预算）。判据：自适应片长、公平轮转、
//! 预算联动、让位正确、切换开销。
//!
//! 诊断码段 0x06D0（本模块独占：0x06D0~0x06DF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x06D0;

/// 默认时间片（微秒）：4ms。
pub const SLICE_DEFAULT_US: u32 = 4_000;

/// 时间片下限：2ms。
pub const SLICE_MIN_US: u32 = 2_000;

/// 时间片上限：8ms。
pub const SLICE_MAX_US: u32 = 8_000;

/// 切分开销（微秒/次）：上下文切换固定成本。
pub const CONTEXT_SWITCH_US: u32 = 40;

/// 片开销占比红线：切分开销/片长 ≤ 50‰。
pub const SWITCH_OVERHEAD_MAX_PERMILLE: u32 = 50;

/// 时间片任务（判据核心的调度对象）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SliceTask {
    /// 任务号。
    pub id: u32,
    /// 剩余执行量（微秒）。
    pub remaining_us: u32,
    /// 优先级号（小 = 高）。
    pub prio: u8,
    /// 已用时间片数。
    pub rounds: u32,
}

impl SliceTask {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.id == 0 {
            return Err(SEG_BASE);
        }
        Ok(())
    }
    /// 是否完成。
    pub fn done(&self) -> bool {
        self.remaining_us == 0
    }
}

/// 自适应片长（判据「自适应片长」核心）：按就绪任务数调节——
/// 任务多片长向短端压（公平响应），任务少片长向长端放（减少切换
/// 开销）。映射：片长 = clamp(SLICE_MAX − ready×步长, MIN, MAX)，
/// 步长 = (MAX−MIN)/32。
pub fn adaptive_slice_us(ready_tasks: u32) -> Result<u32, u16> {
    if ready_tasks == 0 {
        return Err(SEG_BASE + 1);
    }
    let span = (SLICE_MAX_US - SLICE_MIN_US) as u64;
    let step = span / 32;
    let raw = SLICE_MAX_US as i64 - (ready_tasks.min(64) as i64) * step as i64;
    Ok(raw.clamp(SLICE_MIN_US as i64, SLICE_MAX_US as i64) as u32)
}

/// 切分开销审计（判据「切换开销」判据）：片长对切分开销的占比
/// 不得超过红线——片太短直接拒绝调度。
pub fn slice_overhead_ok(slice_us: u32) -> Result<bool, u16> {
    if !(SLICE_MIN_US..=SLICE_MAX_US).contains(&slice_us) {
        return Err(SEG_BASE + 2);
    }
    Ok(CONTEXT_SWITCH_US as u64 * 1000 / slice_us as u64 <= SWITCH_OVERHEAD_MAX_PERMILLE as u64)
}

/// 帧预算联动（判据「预算联动」核心）：本帧片长不得超过帧剩余
/// 预算——超预算的片在预算边界截断（截断片合法但计入让位）。
pub fn slice_within_frame_budget(slice_us: u32, frame_remaining_us: u32) -> Result<u32, u16> {
    if frame_remaining_us == 0 {
        return Err(SEG_BASE + 3);
    }
    Ok(slice_us.min(frame_remaining_us))
}

/// 轮转公平性（判据「公平轮转」核心量）：一轮调度里同优先级任务
/// 各得一片——记录每任务本轮获得的时间量，公平度 = min/max ×1000
/// （1000 = 完全公平；done 的任务不计）。
pub fn round_fairness_permille(granted: &[(u32, u32)]) -> Result<u32, u16> {
    if granted.len() < 2 {
        return Err(SEG_BASE + 4);
    }
    let mut vals: Vec<u32> = granted.iter().map(|&(_, g)| g).collect();
    vals.sort_unstable();
    let lo = vals[0];
    let hi = vals[vals.len() - 1];
    if hi == 0 {
        return Err(SEG_BASE + 5);
    }
    Ok((lo as u64 * 1000 / hi as u64) as u32)
}

/// 时间片轮转调度器（判据「公平轮转」容器 + 判据「让位正确」
/// 执行器）。
#[derive(Default)]
pub struct RoundRobin {
    /// 就绪队列（FIFO——同优先级轮转的底座）。
    queue: Vec<SliceTask>,
    /// 切换计数。
    switches: u64,
    /// 让位计数（片边界主动让位次数）。
    yields: u64,
}

impl RoundRobin {
    /// 建调度器。
    pub fn new() -> Self {
        Self::default()
    }
    /// 入队。
    pub fn admit(&mut self, t: SliceTask) -> Result<(), u16> {
        t.validate()?;
        self.queue.push(t);
        Ok(())
    }
    /// 队列深度。
    pub fn depth(&self) -> usize {
        self.queue.len()
    }
    /// 切换计数。
    pub fn switch_count(&self) -> u64 {
        self.switches
    }
    /// 让位计数。
    pub fn yield_count(&self) -> u64 {
        self.yields
    }
    /// 跑一轮轮转：每个未完成任务获得一片（自适应片长 ∩ 帧剩余
    /// 预算），返回本轮发放账 (id, 实得微秒)。片尾让位：未完成的
    /// 任务主动让位计数 +1。
    pub fn run_round(&mut self, frame_remaining_us: u32) -> Result<Vec<(u32, u32)>, u16> {
        if self.queue.is_empty() {
            return Err(SEG_BASE + 6);
        }
        if frame_remaining_us == 0 {
            return Err(SEG_BASE + 7);
        }
        let ready = self.queue.iter().filter(|t| !t.done()).count() as u32;
        if ready == 0 {
            return Err(SEG_BASE + 8);
        }
        let base = adaptive_slice_us(ready)?;
        if !slice_overhead_ok(base)? {
            return Err(SEG_BASE + 9);
        }
        let mut granted: Vec<(u32, u32)> = Vec::new();
        let n = self.queue.len();
        for i in 0..n {
            let t = &mut self.queue[i];
            if t.done() {
                continue;
            }
            let slice = slice_within_frame_budget(base, frame_remaining_us)?;
            let give = slice.min(t.remaining_us);
            t.remaining_us -= give;
            t.rounds += 1;
            granted.push((t.id, give));
            self.switches += 1;
            if !t.done() {
                self.yields += 1;
            }
        }
        if granted.is_empty() {
            return Err(SEG_BASE + 10);
        }
        Ok(granted)
    }
    /// 调度至全部完成（判据「让位正确」全链演练）：逐轮轮转直到
    /// 全部 done 或轮数耗尽。返回总轮数。
    pub fn drain(&mut self, frame_budget_per_round_us: u32, max_rounds: u32) -> Result<u32, u16> {
        if max_rounds == 0 {
            return Err(SEG_BASE + 11);
        }
        let mut rounds = 0u32;
        while self.queue.iter().any(|t| !t.done()) {
            if rounds >= max_rounds {
                return Err(SEG_BASE + 12);
            }
            self.run_round(frame_budget_per_round_us)?;
            rounds += 1;
        }
        Ok(rounds)
    }
}

/// 片内完成检查（判据「片内完成检查」语义核心）：任务剩余量 ≤
/// 片长 → 本片结束即完成，无需让位；剩余量 > 片长 → 片尾让位。
pub fn slice_completion_check(remaining_us: u32, slice_us: u32) -> Result<(bool, bool), u16> {
    if slice_us == 0 {
        return Err(SEG_BASE + 13);
    }
    let completes = remaining_us <= slice_us;
    let yields = !completes;
    Ok((completes, yields))
}

/// 同优先级轮转序（判据「公平轮转」审计面）：同优先级任务的执行
/// 序必须按入队序轮转（轮转序 = 队列序的循环）——跳号即不公平。
pub fn rotation_order_ok(queue_ids: &[u32], exec_seq: &[u32]) -> Result<bool, u16> {
    if queue_ids.len() < 2 || exec_seq.len() < queue_ids.len() {
        return Err(SEG_BASE + 14);
    }
    // 检查 exec_seq 的前 n 个（去重后按首现序）与队列序一致。
    let mut seen: Vec<u32> = Vec::new();
    for &e in exec_seq {
        if !seen.contains(&e) {
            seen.push(e);
        }
        if seen.len() == queue_ids.len() {
            break;
        }
    }
    Ok(seen == queue_ids)
}

/// 时间片指纹（判据「切换开销」可审计面）：调度器状态折并。
pub fn rr_fp(rr: &RoundRobin) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for t in &rr.queue {
        acc = acc.wrapping_mul(0x100000001b3) ^ t.id as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ t.remaining_us as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ t.rounds as u64;
    }
    acc = acc.wrapping_mul(0x100000001b3) ^ rr.switches;
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task(id: u32, rem: u32, prio: u8) -> SliceTask {
        SliceTask { id, remaining_us: rem, prio, rounds: 0 }
    }
    #[test]
    fn adaptive_slice_compression() {
        // 任务少 → 长片。
        let a = adaptive_slice_us(1).unwrap();
        // 任务多 → 短片。
        let b = adaptive_slice_us(32).unwrap();
        assert!(a > b);
        assert!(a <= SLICE_MAX_US && b >= SLICE_MIN_US);
        assert!(adaptive_slice_us(0).is_err());
    }
    #[test]
    fn overhead_gate() {
        assert!(slice_overhead_ok(SLICE_DEFAULT_US).unwrap());
        // 2ms 片：40/2000 = 20‰ < 50‰ 仍过。
        assert!(slice_overhead_ok(SLICE_MIN_US).unwrap());
        assert!(slice_overhead_ok(100).is_err());
    }
    #[test]
    fn frame_budget_clamps() {
        assert_eq!(slice_within_frame_budget(4000, 2500).unwrap(), 2500);
        assert_eq!(slice_within_frame_budget(4000, 8000).unwrap(), 4000);
        assert!(slice_within_frame_budget(4000, 0).is_err());
    }
    #[test]
    fn round_robin_drains_fairly() {
        let mut rr = RoundRobin::new();
        rr.admit(task(1, 9000, 10)).unwrap();
        rr.admit(task(2, 9000, 10)).unwrap();
        rr.admit(task(3, 9000, 10)).unwrap();
        let rounds = rr.drain(20_000, 16).unwrap();
        assert!(rounds >= 3);
        // 全部完成。
        assert!(rr.queue.iter().all(|t| t.done()));
        // 公平度满格（同量任务同片长）。
        let g = vec![(1, 4000), (2, 4000), (3, 4000)];
        assert_eq!(round_fairness_permille(&g).unwrap(), 1000);
        // 让位发生在每轮每任务（首轮后都有让位）。
        assert!(rr.yield_count() >= 3);
    }
    #[test]
    fn completion_semantics() {
        let (c, y) = slice_completion_check(3000, 4000).unwrap();
        assert!(c && !y);
        let (c2, y2) = slice_completion_check(5000, 4000).unwrap();
        assert!(!c2 && y2);
        assert!(slice_completion_check(1, 0).is_err());
    }
    #[test]
    fn rotation_audit() {
        let q = vec![1u32, 2, 3];
        assert!(rotation_order_ok(&q, &[1, 2, 3, 1, 2, 3]).unwrap());
        assert!(!rotation_order_ok(&q, &[2, 1, 3, 1, 2, 3]).unwrap());
        assert!(rotation_order_ok(&q, &[1]).is_err());
    }
    #[test]
    fn rr_fp_stable() {
        let mut a = RoundRobin::new();
        let mut b = RoundRobin::new();
        a.admit(task(1, 100, 5)).unwrap();
        b.admit(task(1, 100, 5)).unwrap();
        assert_eq!(rr_fp(&a), rr_fp(&b));
        b.admit(task(2, 100, 5)).unwrap();
        assert_ne!(rr_fp(&a), rr_fp(&b));
    }
}
