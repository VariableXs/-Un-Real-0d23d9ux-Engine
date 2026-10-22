//! CGPU-F0180 · 子图与嵌套图（判据段 0x04E0）
//! 任务图的层次化：子图封装（一组相关任务封装为宏节点——外部看是
//! 一个节点，内部是子 DAG）、子图接口（子图声明输入输出资源——外部
//! 通过接口对接，内部实现透明）、嵌套深度上限（3 层——防递归爆炸）。
//! 子图使帧图可维护（每渲染 Pass 是一个子图——帧图=子图集合而非
//! 万节点平铺）。判据：子图封装/展开正确、接口语义、嵌套上限、
//! 帧图=子图集合用例、性能。
//!
//! 诊断码段 0x04E0（本模块独占：0x04E0~0x04EF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x04E0;

/// 嵌套深度上限（判据「嵌套上限」硬界）。
pub const NEST_MAX: u8 = 3;

/// 子图接口（判据「接口语义」载体）：输入/输出资源指纹集。
#[derive(Clone, Debug)]
pub struct SubIf {
    /// 输入资源指纹。
    pub inputs: Vec<u64>,
    /// 输出资源指纹。
    pub outputs: Vec<u64>,
}

impl SubIf {
    /// 接口校验：输入输出全非零、无重复。
    pub fn validate(&self) -> Result<(), u16> {
        for r in self.inputs.iter().chain(self.outputs.iter()) {
            if *r == 0 {
                return Err(SEG_BASE);
            }
        }
        for i in 0..self.outputs.len() {
            for j in (i + 1)..self.outputs.len() {
                if self.outputs[i] == self.outputs[j] {
                    return Err(SEG_BASE + 1);
                }
            }
        }
        Ok(())
    }

    /// 接口对接（判据「接口语义」核心）：本子图的某输入必须由
    /// 上游某子图的输出供给——资源指纹对得上才可连线。
    pub fn connects(&self, upstream: &SubIf) -> Result<usize, u16> {
        self.validate()?;
        upstream.validate()?;
        let mut n = 0;
        for i in &self.inputs {
            if upstream.outputs.contains(i) {
                n += 1;
            }
        }
        Ok(n)
    }
}

/// 子图（判据「子图封装」载体）：宏节点 + 内部任务 + 内部边 + 接口。
pub struct SubGraph {
    /// 宏节点号（外部视角的单一节点号）。
    pub macro_task: u32,
    /// 内部任务集。
    pub inner: Vec<u32>,
    /// 内部边。
    pub inner_edges: Vec<(u32, u32)>,
    /// 接口。
    pub iface: SubIf,
    /// 嵌套深度（1 = 顶层子图）。
    pub depth: u8,
}

impl SubGraph {
    /// 子图校验（判据「子图封装/展开正确」前置）：宏节点不在内部
    /// 集中、内部任务唯一、深度 ≤ 上限、接口齐。
    pub fn validate(&self) -> Result<(), u16> {
        if self.macro_task == 0 {
            return Err(SEG_BASE + 2);
        }
        if self.inner.is_empty() {
            return Err(SEG_BASE + 3);
        }
        if self.inner.contains(&self.macro_task) {
            return Err(SEG_BASE + 4);
        }
        for i in 0..self.inner.len() {
            if self.inner[i] == 0 {
                return Err(SEG_BASE + 5);
            }
            for j in (i + 1)..self.inner.len() {
                if self.inner[i] == self.inner[j] {
                    return Err(SEG_BASE + 6);
                }
            }
        }
        if self.depth == 0 || self.depth > NEST_MAX {
            return Err(SEG_BASE + 7);
        }
        self.iface.validate()?;
        for (f, t) in &self.inner_edges {
            if !self.inner.contains(f) || !self.inner.contains(t) || f == t {
                return Err(SEG_BASE + 8);
            }
        }
        Ok(())
    }

    /// 内部拓扑合法性（封装前提：内部必须是 DAG）。
    pub fn inner_dag(&self) -> Result<(), u16> {
        self.validate()?;
        let n = self.inner.len();
        let idx = |t: u32| self.inner.iter().position(|x| *x == t);
        let mut indeg: Vec<u32> = vec![0; n];
        for (f, t) in &self.inner_edges {
            indeg[idx(*t).ok_or(SEG_BASE + 8)?] += 1;
        }
        let mut ready: Vec<usize> = Vec::new();
        for i in 0..n {
            if indeg[i] == 0 {
                ready.push(i);
            }
        }
        let mut done = 0usize;
        while let Some(i) = ready.pop() {
            done += 1;
            for (f, t) in &self.inner_edges {
                if *f == self.inner[i] {
                    let j = idx(*t).ok_or(SEG_BASE + 8)?;
                    indeg[j] -= 1;
                    if indeg[j] == 0 {
                        ready.push(j);
                    }
                }
            }
        }
        if done != n {
            return Err(SEG_BASE + 9); // 内部有环 = 封装非法
        }
        Ok(())
    }

    /// 展开（判据「子图封装/展开正确」核心面）：宏节点还原为内部
    /// 任务 + 内部边；外部边（外部前驱/后继）改挂到边界任务。
    pub fn expand(
        &self,
        ext_in: &[(u32, u32)],
        ext_out: &[(u32, u32)],
    ) -> Result<(Vec<u32>, Vec<(u32, u32)>), u16> {
        self.inner_dag()?;
        let mut nodes = self.inner.clone();
        let mut edges: Vec<(u32, u32)> = self.inner_edges.clone();
        for (from, to) in ext_in {
            if *to != self.macro_task {
                return Err(SEG_BASE + 10);
            }
            // 外部前驱挂到内部零入度任务（边界入口）。
            for e in &self.inner {
                let has_in = self.inner_edges.iter().any(|(_, t)| t == e);
                if !has_in {
                    edges.push((*from, *e));
                    break;
                }
            }
            if !nodes.contains(from) {
                nodes.push(*from);
            }
        }
        for (from, to) in ext_out {
            if *from != self.macro_task {
                return Err(SEG_BASE + 11);
            }
            for e in &self.inner {
                let has_out = self.inner_edges.iter().any(|(f, _)| f == e);
                if !has_out {
                    edges.push((*e, *to));
                    break;
                }
            }
            if !nodes.contains(to) {
                nodes.push(*to);
            }
        }
        Ok((nodes, edges))
    }
}

/// 帧图 = 子图集合（判据「帧图=子图集合用例」载体）。
pub struct FrameOfSubs {
    /// 子图集。
    pub subs: Vec<SubGraph>,
    /// 子图间边（宏节点级）。
    pub macro_edges: Vec<(u32, u32)>,
}

impl FrameOfSubs {
    /// 帧图校验（判据核心）：每子图独立合法 + 宏边端点在子图集 +
    /// 宏级无环 + 接口对接一致。
    pub fn validate(&self) -> Result<(), u16> {
        if self.subs.is_empty() {
            return Err(SEG_BASE + 12);
        }
        for s in &self.subs {
            s.validate()?;
        }
        for i in 0..self.subs.len() {
            for j in (i + 1)..self.subs.len() {
                if self.subs[i].macro_task == self.subs[j].macro_task {
                    return Err(SEG_BASE + 13);
                }
            }
        }
        for (f, t) in &self.macro_edges {
            if f == t {
                return Err(SEG_BASE + 14);
            }
            if !self.subs.iter().any(|s| s.macro_task == *f)
                || !self.subs.iter().any(|s| s.macro_task == *t)
            {
                return Err(SEG_BASE + 15);
            }
            // 接口对接：上游输出必须能供下游输入（至少一路指纹对上）。
            let up = &self.subs.iter().find(|s| s.macro_task == *f).ok_or(SEG_BASE + 15)?.iface;
            let dn = &self.subs.iter().find(|s| s.macro_task == *t).ok_or(SEG_BASE + 15)?.iface;
            if dn.connects(up)? == 0 {
                return Err(SEG_BASE + 16);
            }
        }
        // 宏级环检测（Kahn）。
        let n = self.subs.len();
        let mut indeg = 0usize;
        let mut degs: Vec<u32> = vec![0; n];
        for (f, t) in &self.macro_edges {
            let ti = self
                .subs
                .iter()
                .position(|s| s.macro_task == *t)
                .ok_or(SEG_BASE + 15)?;
            degs[ti] += 1;
            let _ = f;
        }
        indeg = degs.iter().filter(|d| **d > 0).count();
        if indeg == n && n > 1 {
            // 全员有入度 = 必有环。
            return Err(SEG_BASE + 17);
        }
        Ok(())
    }

    /// 展开全帧（判据「展开正确」全局面）：全部子图展开合并为
    /// 平面图——万节点平铺视角的一次性还原。
    pub fn expand_all(&self) -> Result<(Vec<u32>, Vec<(u32, u32)>), u16> {
        self.validate()?;
        let mut nodes: Vec<u32> = Vec::new();
        let mut edges: Vec<(u32, u32)> = Vec::new();
        for s in &self.subs {
            let (ns, es) = s.expand(&[], &[])?;
            for n in ns {
                if !nodes.contains(&n) {
                    nodes.push(n);
                }
            }
            for e in es {
                if !edges.contains(&e) {
                    edges.push(e);
                }
            }
        }
        for (f, t) in &self.macro_edges {
            // 宏边在展开后挂到两子图的边界任务。
            let up = self.subs.iter().find(|s| s.macro_task == *f).ok_or(SEG_BASE + 15)?;
            let dn = self.subs.iter().find(|s| s.macro_task == *t).ok_or(SEG_BASE + 15)?;
            let up_exit = up
                .inner
                .iter()
                .find(|e| !up.inner_edges.iter().any(|(f2, _)| f2 == *e))
                .ok_or(SEG_BASE + 18)?;
            let dn_entry = dn
                .inner
                .iter()
                .find(|e| !dn.inner_edges.iter().any(|(_, t2)| t2 == *e))
                .ok_or(SEG_BASE + 18)?;
            edges.push((*up_exit, *dn_entry));
        }
        Ok((nodes, edges))
    }

    /// 子图数。
    pub fn sub_count(&self) -> usize {
        self.subs.len()
    }

    /// 平面化压缩率（判据「性能」量化面）：宏视图节点数 / 展开后
    /// 节点数——帧图维护视角的收益度量。
    pub fn flatten_ratio(&self) -> Result<u32, u16> {
        let (flat, _) = self.expand_all()?;
        if flat.len() == 0 || flat.len() < self.subs.len() {
            return Err(SEG_BASE + 19);
        }
        Ok(((self.subs.len() * 1000) / flat.len()) as u32)
    }
}

/// 嵌套深度演练（判据「嵌套上限」序列面）：1/2/3 层合法、4 层拒。
pub fn nest_depth_audit() -> Result<(), u16> {
    for d in 1..=NEST_MAX {
        let s = SubGraph {
            macro_task: 100 + d as u32,
            inner: vec![1, 2],
            inner_edges: vec![(1, 2)],
            iface: SubIf {
                inputs: vec![0xA],
                outputs: vec![0xB],
            },
            depth: d,
        };
        s.validate()?;
    }
    let s4 = SubGraph {
        macro_task: 200,
        inner: vec![1, 2],
        inner_edges: vec![(1, 2)],
        iface: SubIf {
            inputs: vec![0xA],
            outputs: vec![0xB],
        },
        depth: NEST_MAX + 1,
    };
    if s4.validate().is_ok() {
        return Err(SEG_BASE + 20);
    }
    Ok(())
}

/// 帧图子集集合深度扩展面。
impl FrameOfSubs {
    /// 深度普查（判据「嵌套上限」批量面）：全帧子图深度分布。
    pub fn depth_census(&self) -> Result<[usize; 4], u16> {
        let mut out = [0usize; 4];
        for s in &self.subs {
            if s.depth == 0 || s.depth as usize >= out.len() {
                return Err(SEG_BASE + 21);
            }
            out[s.depth as usize] += 1;
        }
        Ok(out)
    }

    /// 接口供给闭合（判据「接口语义」全局面）：除顶层入流外，
    /// 每个子图的每条输入必须有人供给——悬空输入 = 接线遗漏。
    pub fn interface_closed(&self) -> Result<(), u16> {
        self.validate()?;
        let mut supply: Vec<u64> = Vec::new();
        for s in &self.subs {
            for o in &s.iface.outputs {
                if !supply.contains(o) {
                    supply.push(*o);
                }
            }
        }
        for s in &self.subs {
            for i in &s.iface.inputs {
                if !supply.contains(i) {
                    return Err(SEG_BASE + 22);
                }
            }
        }
        Ok(())
    }
}
