//! CGPU-F0185 · 图与 GPU 硬件加速结构联动（判据段 0x0530）
//! GPU 硬件加速结构的图级利用：GPU 计数查询→图节点（遮挡查询结果
//! 作为条件依赖的输入——查询驱动的动态图分支 F0179 联动）、GPU 时间
//! 戳→图节点（实际耗时反馈——成本模型校准 F0004 联动）、GPU fence→
//! 图边（GPU 完成作为跨队列依赖的硬件信号）。判据：三类 GPU 结构
//! 联动、查询驱动分支、反馈校准、联动开销、联调用例。
//!
//! 诊断码段 0x0530（本模块独占：0x0530~0x053F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0530;

/// GPU 硬件结构种类（判据「三类 GPU 结构联动」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpuStruct {
    /// 计数查询（遮挡查询等）。
    Query,
    /// 时间戳（耗时反馈）。
    Timestamp,
    /// 围栏（跨队列硬件信号）。
    Fence,
}

impl GpuStruct {
    /// 序号。
    pub fn rank(self) -> u8 {
        match self {
            GpuStruct::Query => 0,
            GpuStruct::Timestamp => 1,
            GpuStruct::Fence => 2,
        }
    }
}

/// 查询节点（判据「查询驱动分支」载体）：查询挂到条件节点上。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QueryNode {
    /// 任务号。
    pub task: u32,
    /// 查询对象（遮挡/管线统计的查询槽号）。
    pub slot: u32,
    /// 分支阈值（查询值 ≥ 阈值才执行）。
    pub threshold: u32,
    /// 查询值（回填后有效）。
    pub value: Option<u32>,
}

impl QueryNode {
    /// 校验：槽号与阈值非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 || self.slot == 0 || self.threshold == 0 {
            return Err(SEG_BASE);
        }
        Ok(())
    }

    /// 回填并分支判（判据「查询驱动分支」核心）：value ≥ threshold。
    pub fn resolve(&mut self, v: u32) -> Result<bool, u16> {
        self.validate()?;
        if self.value.is_some() {
            return Err(SEG_BASE + 1);
        }
        self.value = Some(v);
        Ok(v >= self.threshold)
    }
}

/// 时间戳反馈节点（判据「反馈校准」载体）：预估 vs 实测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TsNode {
    /// 任务号。
    pub task: u32,
    /// 成本模型预估（微秒，F0004 联动）。
    pub est_us: u32,
    /// 实测（微秒，回填）。
    pub actual_us: Option<u32>,
}

impl TsNode {
    /// 校验。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 || self.est_us == 0 {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }

    /// 回填实测。
    pub fn record(&mut self, us: u32) -> Result<(), u16> {
        self.validate()?;
        if us == 0 || self.actual_us.is_some() {
            return Err(SEG_BASE + 3);
        }
        self.actual_us = Some(us);
        Ok(())
    }

    /// 校准偏差（千分比）：|实测-预估| / 预估。超 30% = 成本模型
    /// 需要重训（反馈校准的触发条件）。
    pub fn drift_permille(&self) -> Result<u32, u16> {
        let a = self.actual_us.ok_or(SEG_BASE + 4)?;
        let d = if a > self.est_us {
            a - self.est_us
        } else {
            self.est_us - a
        };
        Ok(((d as u64 * 1000) / self.est_us as u64) as u32)
    }
}

/// 围栏边（判据「GPU fence→图边」载体）：跨队列依赖的硬件信号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FenceEdge {
    /// 前驱（信号发出方）。
    pub from: u32,
    /// 后继（等待方）。
    pub to: u32,
    /// 围栏号。
    pub fence: u64,
    /// 是否已翻转（硬件完成信号）。
    pub signaled: bool,
}

impl FenceEdge {
    /// 校验：围栏号非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.from == self.to || self.fence == 0 {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }
}

/// 硬件联动账（判据「三类 GPU 结构联动」汇总载体）。
pub struct GpuLinkBook {
    /// 查询节点集。
    pub queries: Vec<QueryNode>,
    /// 时间戳节点集。
    pub stamps: Vec<TsNode>,
    /// 围栏边集。
    pub fences: Vec<FenceEdge>,
}

impl GpuLinkBook {
    /// 空账。
    pub fn new() -> Self {
        GpuLinkBook {
            queries: Vec::new(),
            stamps: Vec::new(),
            fences: Vec::new(),
        }
    }

    /// 挂查询（一任务一查询）。
    pub fn attach_query(&mut self, q: QueryNode) -> Result<(), u16> {
        q.validate()?;
        if self.queries.iter().any(|x| x.task == q.task) {
            return Err(SEG_BASE + 6);
        }
        self.queries.push(q);
        Ok(())
    }

    /// 挂时间戳（一任务一时间戳）。
    pub fn attach_stamp(&mut self, s: TsNode) -> Result<(), u16> {
        s.validate()?;
        if self.stamps.iter().any(|x| x.task == s.task) {
            return Err(SEG_BASE + 7);
        }
        self.stamps.push(s);
        Ok(())
    }

    /// 挂围栏（围栏号唯一）。
    pub fn attach_fence(&mut self, f: FenceEdge) -> Result<(), u16> {
        f.validate()?;
        if self.fences.iter().any(|x| x.fence == f.fence) {
            return Err(SEG_BASE + 8);
        }
        self.fences.push(f);
        Ok(())
    }

    /// 三类齐判（判据「三类 GPU 结构联动」门槛）：三类各至少一条。
    pub fn kinds_complete(&self) -> Result<(), u16> {
        if self.queries.is_empty() {
            return Err(SEG_BASE + 9);
        }
        if self.stamps.is_empty() {
            return Err(SEG_BASE + 10);
        }
        if self.fences.is_empty() {
            return Err(SEG_BASE + 11);
        }
        Ok(())
    }

    /// 查询驱动分支执行集（判据「查询驱动分支」批量面）：全部查询
    /// 已回填，产出放行/跳过集合。
    pub fn branch_verdicts(&self) -> Result<(Vec<u32>, Vec<u32>), u16> {
        if self.queries.is_empty() {
            return Err(SEG_BASE + 12);
        }
        let mut go: Vec<u32> = Vec::new();
        let mut skip: Vec<u32> = Vec::new();
        for q in &self.queries {
            match q.value {
                Some(v) => {
                    if v >= q.threshold {
                        go.push(q.task);
                    } else {
                        skip.push(q.task);
                    }
                }
                None => return Err(SEG_BASE + 13),
            }
        }
        Ok((go, skip))
    }

    /// 成本模型校准审计（判据「反馈校准」批量面）：全部时间戳
    /// 已回填且偏差 ≤ 30%——超差任务清单返回。
    pub fn calibration(&self) -> Result<Vec<u32>, u16> {
        if self.stamps.is_empty() {
            return Err(SEG_BASE + 14);
        }
        let mut over: Vec<u32> = Vec::new();
        for s in &self.stamps {
            let d = s.drift_permille()?;
            if d > 300 {
                over.push(s.task);
            }
        }
        Ok(over)
    }

    /// 围栏等待闭包（判据「GPU fence→图边」执行面）：围栏未翻转
    /// 时后继不可发射——返回当前可发射集。
    pub fn launchable(&self) -> Result<Vec<u32>, u16> {
        if self.fences.is_empty() {
            return Err(SEG_BASE + 15);
        }
        let mut blocked: Vec<u32> = Vec::new();
        for f in &self.fences {
            if !f.signaled {
                blocked.push(f.to);
            }
        }
        let mut out: Vec<u32> = Vec::new();
        for f in &self.fences {
            if f.signaled && !blocked.contains(&f.to) && !out.contains(&f.to) {
                out.push(f.to);
            }
        }
        Ok(out)
    }
}

/// 联动开销审计（判据「联动开销」面）：三类结构每帧的总管理开销
/// （挂账+回填+等待）不得超帧预算 1‰。
pub fn link_overhead(links: usize, self_us: u32, frame_budget_us: u32) -> Result<(), u16> {
    if links == 0 || self_us == 0 || frame_budget_us == 0 {
        return Err(SEG_BASE + 16);
    }
    if (self_us as u64 * 1000) > frame_budget_us as u64 {
        return Err(SEG_BASE + 17);
    }
    Ok(())
}

/// 硬件联动一站式审计（判据「联调用例」总闸）：三类齐 + 分支闭 +
/// 校准闭 + 开销达标。
pub fn link_audit(book: &GpuLinkBook, self_us: u32, frame_budget_us: u32) -> Result<(), u16> {
    book.kinds_complete()?;
    book.branch_verdicts()?;
    book.calibration()?;
    let links = book.queries.len() + book.stamps.len() + book.fences.len();
    link_overhead(links, self_us, frame_budget_us)?;
    book.launchable()?;
    Ok(())
}

/// 硬件联动账深度扩展面。
impl GpuLinkBook {
    /// 联动规模。
    pub fn scale(&self) -> (usize, usize, usize) {
        (self.queries.len(), self.stamps.len(), self.fences.len())
    }

    /// 查询槽唯一性（判据「查询驱动分支」卫生面）：一个查询槽
    /// 只挂一个节点（槽复用 = 分支语义串扰）。
    pub fn slot_unique(&self) -> Result<(), u16> {
        for i in 0..self.queries.len() {
            for j in (i + 1)..self.queries.len() {
                if self.queries[i].slot == self.queries[j].slot {
                    return Err(SEG_BASE + 18);
                }
            }
        }
        Ok(())
    }

    /// 成本模型漂移总量（判据「反馈校准」汇总面）：全部时间戳
    /// 的绝对偏差均值（千分比）——模型整体健康度。
    pub fn drift_mean_permille(&self) -> Result<u32, u16> {
        if self.stamps.is_empty() {
            return Err(SEG_BASE + 19);
        }
        let mut sum: u64 = 0;
        for s in &self.stamps {
            sum += s.drift_permille()? as u64;
        }
        Ok((sum / self.stamps.len() as u64) as u32)
    }

    /// 围栏翻转率（判据「GPU fence 到图边」遥测面）。
    pub fn fence_signal_permille(&self) -> Result<u32, u16> {
        if self.fences.is_empty() {
            return Err(SEG_BASE + 20);
        }
        let s = self.fences.iter().filter(|f| f.signaled).count();
        Ok(((s * 1000) / self.fences.len()) as u32)
    }
}

/// 查询回填演练（独立函数面）：查询回填两次拒（一次性语义）+
/// 分支裁决与回填值一致。
pub fn query_resolve_audit(book: &mut GpuLinkBook, task: u32, v: u32) -> Result<bool, u16> {
    let q = book
        .queries
        .iter_mut()
        .find(|q| q.task == task)
        .ok_or(SEG_BASE + 21)?;
    q.resolve(v)?;
    // 二次回填必须被拒。
    let second = q.resolve(v);
    if second.is_ok() {
        return Err(SEG_BASE + 22);
    }
    Ok(v >= q.threshold)
}
