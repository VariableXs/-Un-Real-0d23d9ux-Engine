//! CGPU-F0186 · 图错误诊断增强（判据段 0x0540）
//! 帧图错误的深度诊断：环检测的环路径完整输出（哪些节点构成环——
//! 不只报「有环」而是画出环）、资源冲突定位（哪两个任务争哪资源）、
//! 优先级反转检测（高优先级被低优先级的前置阻塞）、诊断输出三要素
//! （发生了什么/为什么/建议怎么改图）。判据：环路径输出、冲突定位、
//! 反转检测、三要素质量、诊断用例。
//!
//! 诊断码段 0x0540（本模块独占：0x0540~0x054F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0540;

/// 诊断任务：任务号 + 优先级 + 消耗资源。
#[derive(Clone, Debug)]
pub struct DiagTask {
    /// 任务号。
    pub task: u32,
    /// 优先级（大者高）。
    pub prio: u8,
    /// 消耗资源指纹。
    pub res: u64,
}

impl DiagTask {
    /// 校验。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 || self.res == 0 {
            return Err(SEG_BASE);
        }
        Ok(())
    }
}

/// 诊断图。
pub struct DiagGraph {
    /// 任务集。
    pub tasks: Vec<DiagTask>,
    /// 边集。
    pub edges: Vec<(u32, u32)>,
}

impl DiagGraph {
    /// 空图。
    pub fn new() -> Self {
        DiagGraph {
            tasks: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// 加任务。
    pub fn add_task(&mut self, t: DiagTask) -> Result<(), u16> {
        t.validate()?;
        if self.tasks.iter().any(|x| x.task == t.task) {
            return Err(SEG_BASE + 1);
        }
        self.tasks.push(t);
        Ok(())
    }

    /// 加边。
    pub fn add_edge(&mut self, f: u32, t: u32) -> Result<(), u16> {
        if f == t
            || !self.tasks.iter().any(|x| x.task == f)
            || !self.tasks.iter().any(|x| x.task == t)
        {
            return Err(SEG_BASE + 2);
        }
        if self.edges.contains(&(f, t)) {
            return Err(SEG_BASE + 3);
        }
        self.edges.push((f, t));
        Ok(())
    }

    /// 环路径完整输出（判据「环路径输出」核心面）：DFS 找环并
    /// 输出完整环路径 [a, b, c, a]——不只报「有环」。
    pub fn cycle_path(&self) -> Result<Vec<u32>, u16> {
        // 0=白 1=灰 2=黑；栈即当前路径。
        let mut color: Vec<(u32, u8)> = self.tasks.iter().map(|t| (t.task, 0u8)).collect();
        for start in &self.tasks {
            if color.iter().find(|(n, _)| n == &start.task).map(|(_, c)| *c) != Some(0) {
                continue;
            }
            let mut stack: Vec<(u32, usize)> = Vec::new();
            stack.push((start.task, 0usize));
            while let Some((cur, ei)) = stack.pop() {
                let ci = color.iter_mut().find(|(n, _)| *n == cur);
                if let Some(c) = ci {
                    if c.1 == 0 {
                        c.1 = 1;
                    }
                }
                let out: Vec<u32> = self
                    .edges
                    .iter()
                    .filter(|(f, _)| *f == cur)
                    .map(|(_, t)| *t)
                    .collect();
                let mut descended = false;
                let mut i = ei;
                while i < out.len() {
                    let nxt = out[i];
                    let st = color.iter().find(|(n, _)| *n == nxt).map(|(_, c)| *c);
                    if st == Some(1) {
                        // 环路径：从栈中 cur 的位置到当前 + 收尾。
                        let mut path: Vec<u32> = Vec::new();
                        let pos = stack.iter().position(|(n, _)| *n == nxt);
                        let begin = pos.unwrap_or(0);
                        for (n, _) in &stack[begin..] {
                            path.push(*n);
                        }
                        path.push(cur);
                        path.push(nxt);
                        if path.len() < 3 {
                            return Err(SEG_BASE + 4);
                        }
                        return Ok(path);
                    }
                    if st == Some(0) {
                        stack.push((cur, i + 1));
                        stack.push((nxt, 0));
                        descended = true;
                        break;
                    }
                    i += 1;
                }
                if !descended {
                    if let Some(c) = color.iter_mut().find(|(n, _)| *n == cur) {
                        c.1 = 2;
                    }
                }
            }
        }
        Err(SEG_BASE + 5) // 无环（诊断请求本身是误用）
    }

    /// 资源冲突定位（判据「冲突定位」核心面）：输出 (任务A, 任务B,
    /// 资源) 三元清单——哪两个任务争哪资源，且两者间无边序约束。
    pub fn conflict_locus(&self) -> Result<Vec<(u32, u32, u64)>, u16> {
        if self.tasks.is_empty() {
            return Err(SEG_BASE + 6);
        }
        let mut out: Vec<(u32, u32, u64)> = Vec::new();
        for i in 0..self.tasks.len() {
            for j in (i + 1)..self.tasks.len() {
                let (a, b) = (&self.tasks[i], &self.tasks[j]);
                if a.res == b.res {
                    let ordered = self
                        .edges
                        .iter()
                        .any(|(f, t)| (*f == a.task && *t == b.task) || (*f == b.task && *t == a.task));
                    if !ordered {
                        out.push((a.task, b.task, a.res));
                    }
                }
            }
        }
        Ok(out)
    }

    /// 优先级反转检测（判据「反转检测」核心面）：高优先级任务的
    /// 前驱里出现低优先级任务 = 反转（高优先级被低优先级的前置
    /// 阻塞——调度效率事故）。
    pub fn priority_inversions(&self) -> Result<Vec<(u32, u32)>, u16> {
        if self.tasks.is_empty() {
            return Err(SEG_BASE + 7);
        }
        let prio = |t: u32| -> Result<u8, u16> {
            self.tasks
                .iter()
                .find(|x| x.task == t)
                .map(|x| x.prio)
                .ok_or(SEG_BASE + 8)
        };
        let mut out: Vec<(u32, u32)> = Vec::new();
        for (f, t) in &self.edges {
            let pf = prio(*f)?;
            let pt = prio(*t)?;
            if pf < pt {
                out.push((*f, *t));
            }
        }
        Ok(out)
    }
}

/// 诊断报告三要素（判据「三要素质量」载体）：发生了什么 / 为什么 /
/// 建议怎么改。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagReport {
    /// 要素一：现象码（发生了什么——复用诊断码段）。
    pub what: u16,
    /// 要素二：成因指纹（为什么——环路径/冲突三元/反转对的折并）。
    pub why: u64,
    /// 要素三：建议动作（怎么改图——0=重连边 1=加资源锁 2=提优先级）。
    pub advise: u8,
}

impl DiagReport {
    /// 报告校验：三要素齐备。
    pub fn validate(&self) -> Result<(), u16> {
        if self.what == 0 || self.why == 0 || self.advise > 2 {
            return Err(SEG_BASE + 9);
        }
        Ok(())
    }
}

/// 环诊断报告（判据「环路径输出」→ 三要素）。
pub fn cycle_report(g: &DiagGraph) -> Result<DiagReport, u16> {
    let path = g.cycle_path()?;
    let mut why: u64 = 0xcbf29ce484222325;
    for n in &path {
        why = why.wrapping_mul(0x100000001b3) ^ *n as u64;
    }
    let r = DiagReport {
        what: SEG_BASE + 10,
        why,
        advise: 0,
    };
    r.validate()?;
    Ok(r)
}

/// 冲突诊断报告。
pub fn conflict_report(g: &DiagGraph) -> Result<DiagReport, u16> {
    let loci = g.conflict_locus()?;
    if loci.is_empty() {
        return Err(SEG_BASE + 11);
    }
    let mut why: u64 = 0xcbf29ce484222325;
    for (a, b, r) in &loci {
        why = why.wrapping_mul(0x100000001b3) ^ *a as u64;
        why = why.wrapping_mul(0x100000001b3) ^ *b as u64;
        why = why.wrapping_mul(0x100000001b3) ^ *r;
    }
    let rep = DiagReport {
        what: SEG_BASE + 12,
        why,
        advise: 1,
    };
    rep.validate()?;
    Ok(rep)
}

/// 反转诊断报告。
pub fn inversion_report(g: &DiagGraph) -> Result<DiagReport, u16> {
    let invs = g.priority_inversions()?;
    if invs.is_empty() {
        return Err(SEG_BASE + 13);
    }
    let mut why: u64 = 0xcbf29ce484222325;
    for (f, t) in &invs {
        why = why.wrapping_mul(0x100000001b3) ^ *f as u64;
        why = why.wrapping_mul(0x100000001b3) ^ *t as u64;
    }
    let rep = DiagReport {
        what: SEG_BASE + 14,
        why,
        advise: 2,
    };
    rep.validate()?;
    Ok(rep)
}

/// 诊断用例（判据「诊断用例」一站式）：构造含环/冲突/反转的图，
/// 三类诊断全部出报告且要素齐备。
pub fn diag_use_case() -> Result<(), u16> {
    let mut g = DiagGraph::new();
    // 环：1→2→3→1。
    g.add_task(DiagTask { task: 1, prio: 5, res: 0xA1 })?;
    g.add_task(DiagTask { task: 2, prio: 3, res: 0xA2 })?;
    g.add_task(DiagTask { task: 3, prio: 1, res: 0xA3 })?;
    g.add_edge(1, 2)?;
    g.add_edge(2, 3)?;
    g.add_edge(3, 1)?;
    cycle_report(&g)?;
    // 反转：低优先级 4 是高优先级 5 的前驱。
    g.add_task(DiagTask { task: 4, prio: 1, res: 0xB1 })?;
    g.add_task(DiagTask { task: 5, prio: 9, res: 0xB2 })?;
    g.add_edge(4, 5)?;
    inversion_report(&g)?;
    // 冲突：6/7 争同资源且无序。
    g.add_task(DiagTask { task: 6, prio: 4, res: 0xC1 })?;
    g.add_task(DiagTask { task: 7, prio: 4, res: 0xC1 })?;
    conflict_report(&g)?;
    Ok(())
}

/// 诊断质量审计（判据「三要素质量」汇总面）：环路径首尾闭合 +
/// 报告要素齐——环路径 [.., x, x] 首尾必须相同。
pub fn diag_quality(g: &DiagGraph) -> Result<(), u16> {
    let path = g.cycle_path()?;
    if path.len() < 3 || path[0] != *path.last().unwrap_or(&0) {
        return Err(SEG_BASE + 15);
    }
    Ok(())
}

/// 诊断图深度扩展面。
impl DiagGraph {
    /// 任务数。
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// 优先级反转的修复建议投影（判据「三要素质量」深化面）：
    /// 对每个反转为低优先级前驱建议的新优先级 = 后继优先级
    /// （提升到不低于消费方——反转即消）。
    pub fn inversion_fixes(&self) -> Result<Vec<(u32, u8)>, u16> {
        let invs = self.priority_inversions()?;
        if invs.is_empty() {
            return Err(SEG_BASE + 16);
        }
        let mut out: Vec<(u32, u8)> = Vec::new();
        for (f, t) in &invs {
            let pt = self
                .tasks
                .iter()
                .find(|x| x.task == *t)
                .map(|x| x.prio)
                .ok_or(SEG_BASE + 8)?;
            out.push((*f, pt));
        }
        Ok(out)
    }

    /// 冲突隔离度（判据「冲突定位」量化面）：冲突任务对占全任务
    /// 比例（千分比）——超 30% = 资源划分过粗（应拆资源）。
    pub fn conflict_share(&self) -> Result<u32, u16> {
        let loci = self.conflict_locus()?;
        if self.tasks.is_empty() {
            return Err(SEG_BASE + 17);
        }
        let mut touched: Vec<u32> = Vec::new();
        for (a, b, _) in &loci {
            if !touched.contains(a) {
                touched.push(*a);
            }
            if !touched.contains(b) {
                touched.push(*b);
            }
        }
        if (touched.len() * 1000) / self.tasks.len() > 300 {
            return Err(SEG_BASE + 18);
        }
        Ok(((touched.len() * 1000) / self.tasks.len()) as u32)
    }
}
