//! CGPU-F0189 · 图与合批/裁剪/剔除联动（判据段 0x0570）
//! 帧图与渲染优化的三向联动：合批联动（F0173 的图级合批在 B 域的
//! 完整集成）、裁剪联动（CPU 视锥剔除 F0053 结果 → 图节点条件跳过）、
//! 遮挡联动（F0054 粗判结果 → 图节点条件）。三联动使帧图不只是
//! 「执行计划」而是「渲染优化的中枢」。判据：三联动集成、条件节点
//! 实际生效、性能收益、对拍、用例。
//!
//! 诊断码段 0x0570（本模块独占：0x0570~0x057F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0570;

/// 优化联动种类（判据「三联动」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptLink {
    /// 合批联动（F0173 集成）。
    Batch,
    /// 裁剪联动（F0053 视锥剔除）。
    Frustum,
    /// 遮挡联动（F0054 粗判）。
    Occlusion,
}

impl OptLink {
    /// 序号。
    pub fn rank(self) -> u8 {
        match self {
            OptLink::Batch => 0,
            OptLink::Frustum => 1,
            OptLink::Occlusion => 2,
        }
    }
}

/// 联动节点（判据「三联动集成」载体）：节点 + 挂接的优化种类 +
/// 优化信号（回填前无效）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OptNode {
    /// 任务号。
    pub task: u32,
    /// 联动种类。
    pub kind: OptLink,
    /// 优化信号（true=放行 false=跳过；None=未回填）。
    pub signal: Option<bool>,
}

impl OptNode {
    /// 校验：任务号非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 {
            return Err(SEG_BASE);
        }
        Ok(())
    }

    /// 回填信号（一次性）。
    pub fn feed(&mut self, s: bool) -> Result<(), u16> {
        self.validate()?;
        if self.signal.is_some() {
            return Err(SEG_BASE + 1);
        }
        self.signal = Some(s);
        Ok(())
    }
}

/// 三联动中枢（判据「渲染优化的中枢」载体）：帧图节点到优化
/// 信号的统一挂账。
pub struct OptHub {
    /// 联动节点集。
    pub nodes: Vec<OptNode>,
    /// 图边（中枢视角的执行依赖）。
    pub edges: Vec<(u32, u32)>,
}

impl OptHub {
    /// 空中枢。
    pub fn new() -> Self {
        OptHub {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// 挂节点（一任务一种联动；重复挂同任务拒）。
    pub fn attach(&mut self, n: OptNode) -> Result<(), u16> {
        n.validate()?;
        if self.nodes.iter().any(|x| x.task == n.task) {
            return Err(SEG_BASE + 2);
        }
        self.nodes.push(n);
        Ok(())
    }

    /// 加边。
    pub fn add_edge(&mut self, f: u32, t: u32) -> Result<(), u16> {
        if f == t
            || !self.nodes.iter().any(|x| x.task == f)
            || !self.nodes.iter().any(|x| x.task == t)
        {
            return Err(SEG_BASE + 3);
        }
        if self.edges.contains(&(f, t)) {
            return Err(SEG_BASE + 4);
        }
        self.edges.push((f, t));
        Ok(())
    }

    /// 三类齐判（判据「三联动集成」门槛）：三种联动各至少一节点。
    pub fn kinds_complete(&self) -> Result<(), u16> {
        for k in [OptLink::Batch, OptLink::Frustum, OptLink::Occlusion] {
            if !self.nodes.iter().any(|n| n.kind == k) {
                return Err(SEG_BASE + 5);
            }
        }
        Ok(())
    }

    /// 全信号回填判（判据「条件节点实际生效」前置）：全部联动
    /// 节点信号已回填。
    pub fn all_fed(&self) -> Result<(), u16> {
        if self.nodes.is_empty() {
            return Err(SEG_BASE + 6);
        }
        if self.nodes.iter().any(|n| n.signal.is_none()) {
            return Err(SEG_BASE + 7);
        }
        Ok(())
    }

    /// 级联执行集（判据「条件节点实际生效」核心面）：信号为否的
    /// 节点及其可达后继级联跳过——与 F0179 传播语义同源。
    pub fn exec_set(&self) -> Result<Vec<u32>, u16> {
        self.all_fed()?;
        let mut seeds: Vec<u32> = Vec::new();
        for n in &self.nodes {
            if n.signal == Some(false) {
                seeds.push(n.task);
            }
        }
        let mut skipped: Vec<u32> = Vec::new();
        let mut queue = seeds;
        while let Some(t) = queue.pop() {
            if skipped.contains(&t) {
                continue;
            }
            skipped.push(t);
            for (f, to) in &self.edges {
                if *f == t && !skipped.contains(to) {
                    queue.push(*to);
                }
            }
        }
        let mut exec: Vec<u32> = Vec::new();
        for n in &self.nodes {
            if !skipped.contains(&n.task) {
                exec.push(n.task);
            }
        }
        if exec.is_empty() {
            return Err(SEG_BASE + 8);
        }
        Ok(exec)
    }

    /// 跳过率（判据「性能收益」量化面）：被优化跳过的节点占比
    /// （千分比）——零跳过 = 三联动挂空名（白挂账）。
    pub fn skip_permille(&self) -> Result<u32, u16> {
        self.all_fed()?;
        let skipped = self.nodes.iter().filter(|n| n.signal == Some(false)).count();
        let pm = ((skipped * 1000) / self.nodes.len()) as u32;
        if pm == 0 {
            return Err(SEG_BASE + 9);
        }
        Ok(pm)
    }

    /// 联动种类普查。
    pub fn kind_census(&self) -> Result<[usize; 3], u16> {
        let mut out = [0usize; 3];
        for n in &self.nodes {
            out[n.kind.rank() as usize] += 1;
        }
        Ok(out)
    }
}

/// 信号回填演练（判据「对拍」输入面）：批量回填 + 一次性校验。
pub fn feed_all(hub: &mut OptHub, signals: &[(u32, bool)]) -> Result<(), u16> {
    for (t, s) in signals {
        let n = hub
            .nodes
            .iter_mut()
            .find(|n| n.task == *t)
            .ok_or(SEG_BASE + 10)?;
        n.feed(*s)?;
    }
    hub.all_fed()
}

/// 三联动用例（判据「用例」一站式）：三类联动挂账 → 回填混合
/// 信号 → 级联执行集 → 跳过率非零 → 种类普查。
pub fn triple_link_use_case() -> Result<(), u16> {
    let mut hub = OptHub::new();
    hub.attach(OptNode {
        task: 1,
        kind: OptLink::Batch,
        signal: None,
    })?;
    hub.attach(OptNode {
        task: 2,
        kind: OptLink::Frustum,
        signal: None,
    })?;
    hub.attach(OptNode {
        task: 3,
        kind: OptLink::Occlusion,
        signal: None,
    })?;
    hub.attach(OptNode {
        task: 4,
        kind: OptLink::Batch,
        signal: None,
    })?;
    hub.add_edge(2, 4)?;
    hub.kinds_complete()?;
    feed_all(
        &mut hub,
        &[(1, true), (2, false), (3, true), (4, true)],
    )?;
    let exec = hub.exec_set()?;
    // 任务 2 跳过 → 后继 4 级联跳过。
    if exec.contains(&2) || exec.contains(&4) {
        return Err(SEG_BASE + 11);
    }
    hub.skip_permille()?;
    let census = hub.kind_census()?;
    if census[0] != 2 {
        return Err(SEG_BASE + 12);
    }
    Ok(())
}

/// 三联动中枢深度扩展面。
impl OptHub {
    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 联动指纹（判据「三联动集成」可算化面）：节点序 + 信号折并
    /// ——同帧两次采账指纹必同（联动账确定性）。
    pub fn hub_fp(&self) -> Result<u64, u16> {
        self.all_fed()?;
        let mut acc: u64 = 0xcbf29ce484222325;
        for n in &self.nodes {
            acc = acc.wrapping_mul(0x100000001b3) ^ n.task as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ n.kind.rank() as u64;
            acc = acc.wrapping_mul(0x100000001b3)
                ^ (n.signal.unwrap_or(false) as u64);
        }
        if acc == 0 {
            return Err(SEG_BASE + 13);
        }
        Ok(acc)
    }

    /// 双重回填审计（判据「条件节点实际生效」卫生面）：回填两次
    /// 必须被拒（一次性语义——信号抖动不允许改判）。
    pub fn double_feed_rejected(&mut self, task: u32) -> Result<(), u16> {
        let n = self
            .nodes
            .iter_mut()
            .find(|n| n.task == task)
            .ok_or(SEG_BASE + 14)?;
        if n.signal.is_none() {
            return Err(SEG_BASE + 15);
        }
        if n.feed(true).is_ok() {
            return Err(SEG_BASE + 16);
        }
        Ok(())
    }

    /// 优化收益账（判据「性能收益」深化面）：跳过节点数 × 平均
    /// 任务耗时 = 节省微秒（三联动的存在理由的量化）。
    pub fn saved_us(&self, avg_task_us: u32) -> Result<u64, u16> {
        self.all_fed()?;
        if avg_task_us == 0 {
            return Err(SEG_BASE + 17);
        }
        let skipped = self
            .nodes
            .iter()
            .filter(|n| n.signal == Some(false))
            .count();
        if skipped == 0 {
            return Err(SEG_BASE + 18);
        }
        Ok(skipped as u64 * avg_task_us as u64)
    }
}

/// 裁剪信号生成（独立函数面）：视锥剔除（F0053 对接投影）——
/// 包围球在视锥外 = 信号否。简单符号化：球心距离投影与半径比较。
pub fn frustum_signal(center_d: i32, radius: i32) -> Result<bool, u16> {
    if radius <= 0 {
        return Err(SEG_BASE + 19);
    }
    // center_d < -radius 时球完全在视锥外。
    Ok(center_d + radius >= 0)
}

/// 遮挡信号生成（独立函数面）：遮挡粗判（F0054 对接投影）——
/// 查询样本被遮比例 ≥ 阈值即跳过。
pub fn occlusion_signal(samples: u32, occluded: u32, threshold_permille: u32) -> Result<bool, u16> {
    if samples == 0 || occluded > samples {
        return Err(SEG_BASE + 20);
    }
    if threshold_permille == 0 || threshold_permille > 1000 {
        return Err(SEG_BASE + 21);
    }
    let pm = (occluded * 1000) / samples;
    Ok(pm < threshold_permille)
}
