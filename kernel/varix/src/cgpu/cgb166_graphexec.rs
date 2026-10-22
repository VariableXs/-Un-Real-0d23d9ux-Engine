//! cgb166_graphexec.rs —— CGPU-F0166 · 图执行引擎
//!
//! 帧图（F0161 结构 + F0162 依赖 + F0163 拓扑）的执行引擎：
//! - 就绪追踪：入度归零任务进入就绪池；
//! - 发射循环：就绪任务按优先级发射（对接 F0165）；
//! - 完成回灌：任务完成 → 后继入度减一 → 新就绪；
//! - 执行快照（每步账面可回放）；超时看门狗。
//! 与 cgb19_xfexecint（F0232 执行中断）分册：本册是正常路径
//! 的执行主循环，那册是中断/异常路径。
//! 诊断码段 0x0450（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0450;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 执行任务条目。
#[derive(Clone, Copy, Debug)]
pub struct ExecTask {
    /// 任务号（1..n 连续）。
    pub task: u32,
    /// 预估耗时（微秒）。
    pub cost_us: u32,
    /// 优先级（0 最高）。
    pub prio: u16,
}

impl ExecTask {
    /// 自检。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 || self.cost_us == 0 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 依赖边。
#[derive(Clone, Copy, Debug)]
pub struct ExecDep {
    /// 前任务。
    pub from: u32,
    /// 后任务。
    pub to: u32,
}

impl ExecDep {
    /// 自检。
    pub fn validate(&self) -> Result<(), u16> {
        if self.from == 0 || self.to == 0 || self.from == self.to {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 任务执行态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecState {
    /// 等待依赖。
    Blocked,
    /// 就绪待发射。
    Ready,
    /// 执行中。
    Running,
    /// 完成。
    Done,
    /// 超时（看门狗）。
    TimedOut,
}

/// 执行快照行（判据「执行快照可回放」）。
#[derive(Clone, Copy, Debug)]
pub struct ExecSnap {
    /// 步号。
    pub step: u32,
    /// 动作：0=发射 1=完成 2=超时。
    pub action: u8,
    /// 任务号。
    pub task: u32,
    /// 时刻（微秒）。
    pub at_us: u64,
}

/// 图执行引擎。
pub struct GraphExecEngine {
    /// 任务表。
    tasks: Vec<ExecTask>,
    /// 状态表（下标 = 任务号-1）。
    states: Vec<ExecState>,
    /// 入度表。
    indeg: Vec<u32>,
    /// 邻接表（from → to 列表）。
    adj: Vec<Vec<u32>>,
    /// 就绪池（按 prio 排序发射）。
    ready: Vec<u32>,
    /// 快照账。
    snaps: Vec<ExecSnap>,
    /// 虚拟时钟（微秒）。
    clock_us: u64,
    /// 看门狗阈值（单任务执行超时，微秒）。
    watchdog_us: u64,
    /// 执行中任务（任务号 → 开工时刻）。
    running: Vec<(u32, u64)>,
    /// 已完成数。
    done_count: u32,
    /// 启动标记（拓扑前置校验过才可启动）。
    started: bool,
}

impl GraphExecEngine {
    /// 建引擎（判据「就绪追踪」初始化）：任务号必须 1..n 连续，
    /// 依赖合法（无自环、端点在册）。
    pub fn new(tasks: Vec<ExecTask>, deps: &[ExecDep], watchdog_us: u64) -> Result<Self, u16> {
        if tasks.is_empty() {
            return Err(SEG_BASE + 3);
        }
        if watchdog_us == 0 {
            return Err(SEG_BASE + 4);
        }
        let n = tasks.len() as u32;
        for (i, t) in tasks.iter().enumerate() {
            t.validate()?;
            if t.task as usize != i + 1 {
                return Err(SEG_BASE + 5);
            }
        }
        for d in deps {
            d.validate()?;
            if d.from > n || d.to > n {
                return Err(SEG_BASE + 6);
            }
        }
        let mut indeg = vec![0u32; (n + 1) as usize];
        let mut adj: Vec<Vec<u32>> = vec![Vec::new(); (n + 1) as usize];
        for d in deps {
            indeg[d.to as usize] += 1;
            adj[d.from as usize].push(d.to);
        }
        let mut states = vec![ExecState::Blocked; (n + 1) as usize];
        let mut ready = Vec::new();
        for t in 1..=n {
            if indeg[t as usize] == 0 {
                states[t as usize] = ExecState::Ready;
                ready.push(t);
            }
        }
        if ready.is_empty() {
            // 零就绪入口 = 全图有环，引擎拒绝建。
            return Err(SEG_BASE + 7);
        }
        Ok(GraphExecEngine {
            tasks,
            states,
            indeg,
            adj,
            ready,
            snaps: Vec::new(),
            clock_us: 0,
            watchdog_us,
            running: Vec::new(),
            done_count: 0,
            started: false,
        })
    }

    /// 任务总数。
    pub fn task_total(&self) -> u32 {
        self.tasks.len() as u32
    }

    /// 已完成数。
    pub fn done(&self) -> u32 {
        self.done_count
    }

    /// 就绪池大小。
    pub fn ready_count(&self) -> usize {
        self.ready.len()
    }

    /// 快照数。
    pub fn snap_count(&self) -> usize {
        self.snaps.len()
    }

    /// 时钟。
    pub fn clock(&self) -> u64 {
        self.clock_us
    }

    /// 启动标记。
    pub fn is_started(&self) -> bool {
        self.started
    }

    /// 状态查询。
    pub fn state_of(&self, task: u32) -> Option<ExecState> {
        self.states.get(task as usize).copied()
    }

    /// 发射一枚就绪任务（判据「发射循环」）：就绪池按 prio 升序
    /// 取头（0 最高）。返回任务号。
    pub fn launch(&mut self) -> Result<u32, u16> {
        if self.ready.is_empty() {
            return Err(SEG_BASE + 8);
        }
        self.ready.sort_unstable_by_key(|t| {
            self.tasks[(*t - 1) as usize].prio
        });
        let t = self.ready.remove(0);
        self.states[t as usize] = ExecState::Running;
        self.running.push((t, self.clock_us));
        self.started = true;
        self.snaps.push(ExecSnap {
            step: self.snaps.len() as u32 + 1,
            action: 0,
            task: t,
            at_us: self.clock_us,
        });
        Ok(t)
    }

    /// 完成回灌（判据「完成回灌」）：任务完成 → 时钟推进其耗时
    /// → 后继入度减一 → 归零者进就绪池。
    pub fn complete(&mut self, task: u32) -> Result<Vec<u32>, u16> {
        if self.state_of(task) != Some(ExecState::Running) {
            return Err(SEG_BASE + 9);
        }
        let pos = self
            .running
            .iter()
            .position(|(t, _)| *t == task)
            .ok_or(SEG_BASE + 9)?;
        let started_at = self.running.remove(pos).1;
        let cost = self.tasks[(task - 1) as usize].cost_us as u64;
        self.clock_us = self.clock_us.max(started_at + cost);
        self.states[task as usize] = ExecState::Done;
        self.done_count += 1;
        self.snaps.push(ExecSnap {
            step: self.snaps.len() as u32 + 1,
            action: 1,
            task,
            at_us: self.clock_us,
        });
        let mut newly = Vec::new();
        for &m in self.adj[task as usize].clone().iter() {
            self.indeg[m as usize] -= 1;
            if self.indeg[m as usize] == 0 {
                self.states[m as usize] = ExecState::Ready;
                self.ready.push(m);
                newly.push(m);
            }
        }
        Ok(newly)
    }

    /// 看门狗扫描（判据「超时看门狗」）：执行中超阈值任务标记
    /// TimedOut 并移出执行池。返回超时任务号集。
    pub fn watchdog_sweep(&mut self) -> Vec<u32> {
        let mut timed = Vec::new();
        let mut i = 0;
        while i < self.running.len() {
            let (t, started_at) = self.running[i];
            if self.clock_us.saturating_sub(started_at) >= self.watchdog_us {
                self.states[t as usize] = ExecState::TimedOut;
                self.running.remove(i);
                self.snaps.push(ExecSnap {
                    step: self.snaps.len() as u32 + 1,
                    action: 2,
                    task: t,
                    at_us: self.clock_us,
                });
                timed.push(t);
            } else {
                i += 1;
            }
        }
        timed
    }

    /// 全图完成判（判据收口）：done == n 且无执行中且无超时。
    pub fn all_green(&self) -> Result<(), u16> {
        if self.done_count != self.tasks.len() as u32 {
            return Err(SEG_BASE + 10);
        }
        if !self.running.is_empty() {
            return Err(SEG_BASE + 11);
        }
        if self.states.iter().any(|s| *s == ExecState::TimedOut) {
            return Err(SEG_BASE + 12);
        }
        Ok(())
    }

    /// 发射序合规（判据「发射循环」验证面）：快照中发射动作的
    /// 任务优先级必须单调不降（同调度策略下高优先行）。
    pub fn launch_order_audit(&self) -> Result<(), u16> {
        let mut last_prio: Option<u16> = None;
        for s in &self.snaps {
            if s.action != 0 {
                continue;
            }
            let p = self.tasks[(s.task - 1) as usize].prio;
            if let Some(lp) = last_prio {
                if p < lp {
                    return Err(SEG_BASE + 13);
                }
            }
            last_prio = Some(p);
        }
        Ok(())
    }

    /// 快照回放一致性（判据「执行快照可回放」）：步号连续且
    /// 时刻单调不降。
    pub fn replay_consistent(&self) -> Result<(), u16> {
        let mut prev_step = 0u32;
        let mut prev_at = 0u64;
        for s in &self.snaps {
            if s.step != prev_step + 1 {
                return Err(SEG_BASE + 14);
            }
            if s.at_us < prev_at {
                return Err(SEG_BASE + 15);
            }
            prev_step = s.step;
            prev_at = s.at_us;
        }
        Ok(())
    }

    /// 执行账指纹（判据收口）：全部快照折入 FNV。
    pub fn exec_fp(&self) -> Result<u64, u16> {
        if self.snaps.is_empty() {
            return Err(SEG_BASE + 16);
        }
        let mut acc: u64 = 0xcbf29ce484222325;
        for s in &self.snaps {
            acc = acc.wrapping_mul(0x100000001b3) ^ (s.step as u64);
            acc = acc.wrapping_mul(0x100000001b3) ^ (s.action as u64);
            acc = acc.wrapping_mul(0x100000001b3) ^ (s.task as u64);
            acc = acc.wrapping_mul(0x100000001b3) ^ s.at_us;
        }
        if acc == 0 {
            return Err(SEG_BASE + 17);
        }
        Ok(acc)
    }

    /// 状态普查。
    pub fn state_census(&self) -> (usize, usize, usize, usize, usize) {
        (
            self.states.iter().filter(|s| **s == ExecState::Blocked).count(),
            self.states.iter().filter(|s| **s == ExecState::Ready).count(),
            self.states.iter().filter(|s| **s == ExecState::Running).count(),
            self.states.iter().filter(|s| **s == ExecState::Done).count(),
            self.states.iter().filter(|s| **s == ExecState::TimedOut).count(),
        )
    }
}

/// 引擎确定性断言（判据补充）：同图同策略全跑两遍，执行账
/// 指纹一致（引擎是确定性的——同输入同发射序同完成序）。
pub fn engine_deterministic(
    tasks: &[ExecTask],
    deps: &[ExecDep],
    watchdog_us: u64,
) -> Result<(), u16> {
    let run_once = || -> Result<u64, u16> {
        let mut e = GraphExecEngine::new(tasks.to_vec(), deps, watchdog_us)?;
        loop {
            if e.done() == e.task_total() {
                break;
            }
            match e.launch() {
                Ok(t) => {
                    e.complete(t)?;
                }
                Err(_) => break,
            }
        }
        e.all_green()?;
        e.replay_consistent()?;
        e.exec_fp()
    };
    let a = run_once()?;
    let b = run_once()?;
    if a == b {
        Ok(())
    } else {
        Err(SEG_BASE + 18)
    }
}

/// 执行耗时下界断言（独立函数面）：总执行时刻 ≥ 关键路径下界
/// （由任务成本与依赖推得的单链下界——简化口径：任一依赖链的
/// 成本和都不得超过总时钟）。
pub fn exec_floor_ok(
    e: &GraphExecEngine,
    tasks: &[ExecTask],
    deps: &[ExecDep],
) -> Result<(), u16> {
    if tasks.is_empty() {
        return Err(SEG_BASE + 19);
    }
    // 最长单链（简化 DP：按任务号序松弛）。
    let n = tasks.len() as u32;
    let mut dist = vec![0u64; (n + 1) as usize];
    let mut changed = true;
    while changed {
        changed = false;
        for d in deps {
            let w = tasks[(d.from - 1) as usize].cost_us as u64;
            if dist[d.from as usize] + w > dist[d.to as usize] {
                dist[d.to as usize] = dist[d.from as usize] + w;
                changed = true;
            }
        }
    }
    let lower = dist.iter().copied().max().unwrap_or(0);
    if e.clock() < lower {
        return Err(SEG_BASE + 20);
    }
    Ok(())
}

impl GraphExecEngine {
    /// 实际执行耗时投影（判据「执行快照」量化面）：完成动作
    /// 时刻 - 对应发射时刻。
    pub fn actual_durations(&self) -> Vec<(u32, u64)> {
        let mut out = Vec::new();
        for s in &self.snaps {
            if s.action == 1 {
                let start = self
                    .snaps
                    .iter()
                    .find(|p| p.action == 0 && p.task == s.task)
                    .map(|p| p.at_us)
                    .unwrap_or(0);
                out.push((s.task, s.at_us.saturating_sub(start)));
            }
        }
        out
    }

    /// 发射批次数（判据「发射循环」并行可见性）：同刻多枚发射
    /// 记为一批。
    pub fn launch_batches(&self) -> usize {
        let mut batches: Vec<u64> = Vec::new();
        for s in &self.snaps {
            if s.action == 0 && !batches.contains(&s.at_us) {
                batches.push(s.at_us);
            }
        }
        batches.len()
    }

    /// 超时率（千分比）。
    pub fn timeout_permille(&self) -> u32 {
        let total = self.snaps.iter().filter(|s| s.action != 2).count();
        if total == 0 {
            return 0;
        }
        let timed = self.snaps.iter().filter(|s| s.action == 2).count();
        ((timed * 1000) / total) as u32
    }
}
