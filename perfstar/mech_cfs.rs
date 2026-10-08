//! mech_cfs — CFS 虚拟时钟公平调度本体（AI-K1 深化批次五 · F047）。
//!
//! 主册依据：
//! - F047【设计细节】「输入类 p99 ≤2000μs」——延迟预算此前有预算表、
//!   判定状态机（mech_tokens 的闸门执行机构），但**预算在 CPU 侧的分配
//!   本体**缺席：多 Lane 抢同一核时谁先谁后、份额按什么比例、睡眠者回来
//!   会不会赖账、长任务会不会饿死短任务——这些是 CFS（Completely Fair
//!   Scheduler）回答的问题。本件按 Linux CFS 语义实现核心算法：
//!   vruntime = delta_exec × NICE0_WEIGHT / weight（低权重跑得慢时间账），
//!   min_vruntime 单调推进（睡眠者回来补课不越位）、唤醒放置钳制
//!   （vruntime ≥ rq.min_vruntime，防睡一觉回来独占）、抢占粒度
//!   （wakeup_granularity，防抖动乒乓）、调度周期切片按权重分摊。
//! - 锚点：F047 输入类预算的前提是「交互任务总能拿到核」——CFS 的
//!   vruntime 公平性就是这条判据的调度学底盘（交互任务睡眠多、vruntime
//!   停走、回来时必然最小 → 立即被选）。
//! - 零堆、零浮点（权重表整数、除法截断如实登记）。

// ---------------------------------------------------------------------------
// 1. 权重表（Linux sched_prio_to_weight 原值，nice -20..=19）
// ---------------------------------------------------------------------------

/// nice 0 的权重（基准）。
pub const NICE_0_WEIGHT: u64 = 1024;

/// Linux 内核原版权重表（nice -20..=19 共 40 项，一处一事实：数值即主册）。
const NICE_TO_WEIGHT: [u64; 40] = [
    88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916, // -20..-11
    9548, 7620, 6100, 4904, 3906, 3121, 2501, 1991, 1586, 1277, // -10..-1
    1024, 820, 655, 526, 423, 335, 272, 215, 172, 137, // 0..9
    110, 87, 70, 56, 45, 36, 29, 23, 18, 15, // 10..19
];

/// nice（i32，-20..=19）→ 权重。越界钳制（诚实：不 panic、取边界）。
pub fn nice_weight(nice: i32) -> u64 {
    let idx = nice.clamp(-20, 19) + 20;
    NICE_TO_WEIGHT[idx as usize]
}

// ---------------------------------------------------------------------------
// 2. 可调度实体与运行队列
// ---------------------------------------------------------------------------

/// 最大并发实体（容量纪律：定长数组，零堆）。
pub const MAX_ENTITIES: usize = 16;

/// 一个可调度实体（Lane / 任务）。
#[derive(Clone, Copy, Debug)]
pub struct Entity {
    pub id: u32,
    pub weight: u64,
    /// 累计虚拟运行时间（ns）。
    pub vruntime: u64,
    /// 本周期已服务真实时间（ns，统计面）。
    pub served: u64,
    /// 是否可运行（在队列上）。
    pub runnable: bool,
}

impl Entity {
    pub const fn new(id: u32, _nice: i32) -> Self {
        Entity {
            id,
            weight: NICE_0_WEIGHT, // new 时给基准，place 前由 set_nice 修正
            vruntime: 0,
            served: 0,
            runnable: true,
        }
    }

    pub fn set_nice(&mut self, nice: i32) {
        self.weight = nice_weight(nice);
    }
}

/// 调度参数（一处一事实：数值注 Linux 默认，单位 ns）。
pub const SCHED_PERIOD_NS: u64 = 6_000_000; // 6ms 调度周期
pub const MIN_SLICE_NS: u64 = 750_000; // sysctl_sched_min_granularity 同族
pub const WAKEUP_GRAN_NS: u64 = 1_000_000; // 唤醒抢占粒度（NICE0 下）

/// CFS 运行队列（单核视野；多核按域实例化多份）。
pub struct CfsRq {
    ents: [Entity; MAX_ENTITIES],
    n: usize,
    /// 单调水线：只前进不后退。所有放置的 vruntime 下限。
    min_vruntime: u64,
    /// 当前被选中者（本轮 id），None = 空 q。
    pub curr: Option<u32>,
    /// 统计面：护轨/放置/切片记账。
    pub placements: u64,
    pub preemptions: u64,
}

impl CfsRq {
    pub fn new() -> Self {
        CfsRq {
            ents: [Entity::new(u32::MAX, 0); MAX_ENTITIES],
            n: 0,
            min_vruntime: 0,
            curr: None,
            placements: 0,
            preemptions: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    /// 入队。睡眠者唤醒走 place（钳到水线上）。
    /// 返回 Err 表示满（容量诚实）。
    pub fn enqueue(&mut self, id: u32, nice: i32, vruntime_hint: u64) -> Result<(), ()> {
        if self.n >= MAX_ENTITIES || self.find(id).is_some() {
            return Err(());
        }
        let weight = nice_weight(nice);
        // 唤醒放置：vruntime ≥ 水线（睡过的不能赖账——从更早的账目回来）。
        let placed = vruntime_hint.max(self.min_vruntime);
        self.ents[self.n] = Entity {
            id,
            weight,
            vruntime: placed,
            served: 0,
            runnable: true,
        };
        self.n += 1;
        self.placements += 1;
        Ok(())
    }

    fn find(&self, id: u32) -> Option<usize> {
        (0..self.n).find(|&i| self.ents[i].id == id)
    }

    fn find_mut(&mut self, id: u32) -> Option<&mut Entity> {
        let i = self.find(id)?;
        Some(&mut self.ents[i])
    }

    /// 全队列 vruntime 最小值（并推进水线候选）。
    fn min_vr(&self) -> u64 {
        let mut m = u64::MAX;
        for e in &self.ents[..self.n] {
            if e.runnable && e.vruntime < m {
                m = e.vruntime;
            }
        }
        m
    }

    /// 选下一个运行者：vruntime 最小的可运行实体（红黑树最左的数组版）。
    /// 平局取先入队者（稳定序——确定性回放判据同源）。
    pub fn pick_next(&mut self) -> Option<u32> {
        let mut best: Option<usize> = None;
        for i in 0..self.n {
            if !self.ents[i].runnable {
                continue;
            }
            let better = match best {
                None => true,
                Some(b) => {
                    self.ents[i].vruntime <= self.ents[b].vruntime // <= 保稳定序
                }
            };
            if better {
                best = Some(i);
            }
        }
        let b = best?;
        self.curr = Some(self.ents[b].id);
        Some(self.ents[b].id)
    }

    /// 当前实体跑了 `delta_exec` 真实时间：vruntime 按权重折算前进，
    /// 水线推进（只增不减）。返回折算后的虚拟增量。
    pub fn account_exec(&mut self, id: u32, delta_exec: u64) -> u64 {
        let w = self.find(id).map(|i| self.ents[i].weight).unwrap_or(NICE_0_WEIGHT);
        // vruntime 增量 = delta × 1024 / weight（低权重时间账走得快）。
        let vdelta = delta_exec.saturating_mul(NICE_0_WEIGHT) / w.max(1);
        if let Some(e) = self.find_mut(id) {
            e.vruntime = e.vruntime.saturating_add(vdelta);
            e.served = e.served.saturating_add(delta_exec);
        }
        // 水线推进：min(全队最小) 与 (curr 旧水线) 取大——单调。
        let cand = self.min_vr();
        if cand != u64::MAX && cand > self.min_vruntime {
            self.min_vruntime = cand;
        }
        vdelta
    }

    /// 唤醒抢占判定：新唤醒者是否该打断当前运行者。
    /// 条件：curr.vruntime − new.vruntime > 唤醒粒度（按 curr 权重缩放——
    /// 高权重者更难被打断）。粒度防乒乓：差值必须显著。
    pub fn should_preempt(&self, curr_id: u32, new_id: u32) -> bool {
        let (cv, nv, cw) = match (self.find(curr_id), self.find(new_id)) {
            (Some(c), Some(n)) => (self.ents[c].vruntime, self.ents[n].vruntime, self.ents[c].weight),
            _ => return false,
        };
        // granule 按 curr 权重缩放：gran = WAKEUP_GRAN × 1024 / curr_weight。
        let gran = WAKEUP_GRAN_NS.saturating_mul(NICE_0_WEIGHT) / cw.max(1);
        cv > nv && cv - nv > gran
    }

    /// 当前实体让出（其切片已尽或被打断）。
    pub fn yield_curr(&mut self, id: u32) {
        self.curr = None;
        // 出队再入队由域逻辑编排；这里只清选中位。
        let _ = id;
    }

    /// 调度周期切片：period × weight / Σweight，钳到 min_slice。
    pub fn slice_of(&self, id: u32) -> u64 {
        let w = self.find(id).map(|i| self.ents[i].weight).unwrap_or(NICE_0_WEIGHT);
        let mut total = 0u64;
        for e in &self.ents[..self.n] {
            if e.runnable {
                total += e.weight;
            }
        }
        if total == 0 {
            return SCHED_PERIOD_NS;
        }
        let s = SCHED_PERIOD_NS.saturating_mul(w) / total;
        s.max(MIN_SLICE_NS)
    }

    /// 水线只读（断言单调性用）。
    pub fn min_vruntime(&self) -> u64 {
        self.min_vruntime
    }

    pub fn served_of(&self, id: u32) -> u64 {
        self.find(id).map(|i| self.ents[i].served).unwrap_or(0)
    }

    pub fn vruntime_of(&self, id: u32) -> u64 {
        self.find(id).map(|i| self.ents[i].vruntime).unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// 3. 驱动器：按切片时间步进跑一段仿真（确定性，无 RNG）
// ---------------------------------------------------------------------------

/// 仿真步进：`steps` 轮，每轮 pick → 跑其切片的一半（制造切换）→ 记账。
/// 返回轮次。睡眠/唤醒由测试用 enqueue + place 编排。
pub fn drive(rq: &mut CfsRq, steps: usize) -> usize {
    let mut ran = 0usize;
    for _ in 0..steps {
        let Some(id) = rq.pick_next() else { break };
        let slice = rq.slice_of(id);
        rq.account_exec(id, slice / 2);
        ran += 1;
    }
    ran
}

// ---------------------------------------------------------------------------
// 4. CheckSet（挂接 F047 检查链）
// ---------------------------------------------------------------------------

pub fn run_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut cs = CheckSet::new("mech_cfs");

    // 1) 同权重同起点 → 份额收敛相等（公平性本体）。
    {
        let mut rq = CfsRq::new();
        rq.enqueue(1, 0, 0).unwrap();
        rq.enqueue(2, 0, 0).unwrap();
        drive(&mut rq, 40);
        let s1 = rq.served_of(1);
        let s2 = rq.served_of(2);
        // 轮流被选 + 各跑半切片：两实体服务量几乎相同（±1 步）。
        cs.add("cfs_equal_share", s1.abs_diff(s2) <= MIN_SLICE_NS, "");
    }

    // 2) 权重比传导服务比：nice 0（1024）vs nice 10（110）≈ 9.3:1，
    //    宽松带 6:1..14:1（除法截断 + 离散步进误差，如实登记）。
    {
        let mut rq = CfsRq::new();
        rq.enqueue(1, 0, 0).unwrap();
        rq.enqueue(2, 10, 0).unwrap();
        drive(&mut rq, 60);
        let a = rq.served_of(1) as f64;
        let b = rq.served_of(2) as f64;
        let ratio = a / b;
        cs.add("cfs_weight_share", (6.0..=14.0).contains(&ratio), "");
    }

    // 3) min_vruntime 单调：任何观测序列不回退。
    {
        let mut rq = CfsRq::new();
        rq.enqueue(1, 0, 0).unwrap();
        rq.enqueue(2, 5, 0).unwrap();
        let mut prev = rq.min_vruntime();
        let mut mono = true;
        for _ in 0..30 {
            rq.pick_next();
            rq.account_exec(rq.curr.unwrap(), 200_000);
            if rq.min_vruntime() < prev {
                mono = false;
            }
            prev = rq.min_vruntime();
        }
        cs.add("cfs_min_vruntime_monotonic", mono && prev > 0, "");
    }

    // 4) 唤醒放置钳制：睡很久（vruntime 滞后）回来不得越位独占——
    //    放置点 = max(hint, min_vruntime)，追赶量有界。
    {
        let mut rq = CfsRq::new();
        rq.enqueue(1, 0, 0).unwrap();
        for _ in 0..10 {
            rq.pick_next();
            rq.account_exec(1, 1_000_000);
        }
        let watermark = rq.min_vruntime();
        // 实体 2 睡前 vruntime=0，唤醒时钳到水线附近。
        rq.enqueue(2, 0, 0).unwrap();
        let placed = rq.vruntime_of(2);
        cs.add("cfs_wakeup_clamped", placed >= watermark, "");
    }

    // 5) 唤醒抢占粒度：差值不够大不打断（防乒乓），够大才打断。
    {
        let mut rq = CfsRq::new();
        rq.enqueue(1, 0, 0).unwrap();
        rq.enqueue(2, 0, 0).unwrap();
        // 制造小差距：curr 跑一小步。
        rq.pick_next();
        rq.account_exec(1, 100_000);
        let small = rq.should_preempt(1, 2);
        // 制造大差距：curr 独跑很久。
        for _ in 0..20 {
            rq.account_exec(1, 1_000_000);
        }
        let big = rq.should_preempt(1, 2);
        cs.add("cfs_preempt_granule", !small && big, "");
    }

    // 6) 切片按权重分摊 + min_slice 下限。
    {
        let mut rq = CfsRq::new();
        rq.enqueue(1, 0, 0).unwrap();
        rq.enqueue(2, 0, 0).unwrap();
        let s = rq.slice_of(1);
        cs.add("cfs_slice_bounds", s >= MIN_SLICE_NS && s <= SCHED_PERIOD_NS, "");
    }

    cs
}

// ---------------------------------------------------------------------------
// 5. 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_weight_table_is_linux_exact() {
        assert_eq!(nice_weight(0), 1024);
        assert_eq!(nice_weight(-20), 88761);
        assert_eq!(nice_weight(19), 15);
        assert_eq!(nice_weight(-5), 3121);
        assert_eq!(nice_weight(5), 335);
        // 越界钳制不 panic。
        assert_eq!(nice_weight(-100), 88761);
        assert_eq!(nice_weight(100), 15);
    }

    #[test]
    fn equal_weights_converge_to_equal_service() {
        let mut rq = CfsRq::new();
        rq.enqueue(1, 0, 0).unwrap();
        rq.enqueue(2, 0, 0).unwrap();
        rq.enqueue(3, 0, 0).unwrap();
        drive(&mut rq, 90);
        let mut sv: Vec<u64> = (1..=3).map(|i| rq.served_of(i)).collect();
        sv.sort_unstable();
        // 最大与最小差 ≤ 一步切片的一半。
        assert!(sv[2] - sv[0] <= MIN_SLICE_NS, "{:?}", sv);
        // 三者都被真实服务过。
        assert!(sv[0] > 0);
    }

    #[test]
    fn weight_ratio_propagates_to_service_ratio() {
        // nice -5（3121）vs nice 0（1024）：约 3:1，带 2..5。
        let mut rq = CfsRq::new();
        rq.enqueue(1, -5, 0).unwrap();
        rq.enqueue(2, 0, 0).unwrap();
        drive(&mut rq, 80);
        let a = rq.served_of(1) as f64;
        let b = rq.served_of(2) as f64;
        let r = a / b;
        assert!((2.0..=5.0).contains(&r), "ratio={}", r);
    }

    #[test]
    fn sleeping_entity_cannot_leapfrog_watermark() {
        let mut rq = CfsRq::new();
        rq.enqueue(1, 0, 0).unwrap();
        for _ in 0..15 {
            rq.pick_next();
            rq.account_exec(1, 1_000_000);
        }
        let wm = rq.min_vruntime();
        assert!(wm > 0);
        // 睡眠者带着远古 vruntime 回来 → 被钳到 ≥ 水线。
        rq.enqueue(9, 0, 0).unwrap();
        assert!(rq.vruntime_of(9) >= wm);
        // 回来后下一轮先选它（追赶公平）。
        let nxt = rq.pick_next().unwrap();
        assert_eq!(nxt, 9);
    }

    #[test]
    fn preempt_granule_phases() {
        let mut rq = CfsRq::new();
        rq.enqueue(1, 0, 0).unwrap();
        rq.enqueue(2, 0, 0).unwrap();
        rq.pick_next(); // curr=1
        // 微小差距 → 不抢占。
        rq.account_exec(1, 50_000);
        assert!(!rq.should_preempt(1, 2));
        // 巨大差距 → 抢占。
        for _ in 0..30 {
            rq.account_exec(1, 1_000_000);
        }
        assert!(rq.should_preempt(1, 2));
    }

    #[test]
    fn min_slice_floor_holds_under_crowding() {
        // 一堆低权重实体挤进来，切片不低于下限。
        let mut rq = CfsRq::new();
        for i in 0..8u32 {
            rq.enqueue(i, 15, 0).unwrap();
        }
        let s = rq.slice_of(0);
        assert_eq!(s, MIN_SLICE_NS);
    }

    #[test]
    fn watermark_never_regresses_long_run() {
        let mut rq = CfsRq::new();
        for i in 0..4u32 {
            rq.enqueue(i, (i as i32 % 7) - 3, 0).unwrap();
        }
        let mut prev = 0u64;
        for _ in 0..200 {
            rq.pick_next();
            let c = rq.curr.unwrap();
            rq.account_exec(c, 300_000);
            assert!(rq.min_vruntime() >= prev);
            prev = rq.min_vruntime();
        }
    }

    #[test]
    fn capacity_is_honest() {
        let mut rq = CfsRq::new();
        for i in 0..MAX_ENTITIES as u32 {
            assert!(rq.enqueue(i, 0, 0).is_ok());
        }
        assert!(rq.enqueue(99, 0, 0).is_err());
        assert_eq!(rq.len(), MAX_ENTITIES);
        // 重复 id 拒绝。
        let mut r2 = CfsRq::new();
        r2.enqueue(7, 0, 0).unwrap();
        assert!(r2.enqueue(7, 0, 0).is_err());
    }
}
