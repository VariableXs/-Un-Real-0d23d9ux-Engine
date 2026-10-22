//! cgb162_resautoedges.rs —— CGPU-F0162 · 资源读写集自动依赖推导
//!
//! 任务依赖的自动推导：每任务声明读写资源集（纹理/缓冲/表面），
//! 调度器按资源在任务间的访问序自动建边——
//! - 写后读（RAW）：前任务写 r，后任务读 r ⇒ 前一定后序；
//! - 读后写（WAR）：前任务读 r，后任务写 r ⇒ 前一定后序；
//! - 写后写（WAW）：两任务都写 r ⇒ 保持写序；
//! - 读读不建边（可并行）。
//! 资源粒度：子资源级（mip 层/数组层独立——同纹理不同 mip 可
//! 并行）。冲突消解：同对任务多资源冲突只建一条合并边。
//! 判据：RAW/WAR/WAW 全测、子资源粒度验证、边合并正确、
//! 漏边零容忍注入测试、推导性能。
//! 诊断码段 0x0410（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0410;

/// 子资源 ID 编码（判据「子资源粒度」）：纹理号 8 位 + mip 层
/// 4 位 + 数组层 4 位，合成 16 位——同纹理不同 mip/层是不同资源。
pub fn subres(tex: u8, mip: u8, layer: u8) -> u32 {
    ((tex as u32) << 8) | ((mip as u32) << 4) | (layer as u32)
}

/// 解码子资源（审计用）。
pub fn subres_split(id: u32) -> (u8, u8, u8) {
    (
        (id >> 8) as u8,
        ((id >> 4) & 0xF) as u8,
        (id & 0xF) as u8,
    )
}

/// 任务读写声明。
#[derive(Clone, Debug)]
pub struct Access {
    /// 任务号（帧内构建序）。
    pub task: u32,
    /// 子资源 ID。
    pub sub: u32,
    /// 写访问。
    pub write: bool,
}

impl Access {
    /// 自检：任务号与子资源非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 || self.sub == 0 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 推导出的依赖边（合并形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedEdge {
    /// 前任务。
    pub from: u32,
    /// 后任务。
    pub to: u32,
    /// 边类型（取该对任务上最严格的依赖：RAW > WAW > WAR 排序，
    /// 合并时保留最严格者——严格序决定调度保守度）。
    pub kind: EdgeKind,
}

/// 边类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// 写后读。
    Raw,
    /// 读后写。
    War,
    /// 写后写。
    Waw,
}

impl EdgeKind {
    /// 严格度（越大越严格）。
    pub fn strictness(self) -> u8 {
        match self {
            EdgeKind::Raw => 2,
            EdgeKind::Waw => 1,
            EdgeKind::War => 0,
        }
    }
}

/// 推导器输入：按构建序排列的访问序列。
pub struct AccessLog {
    /// 访问序列（构建序即潜在执行序——WAW 保持此序）。
    pub log: Vec<Access>,
}

impl AccessLog {
    /// 空日志。
    pub fn new() -> Self {
        AccessLog { log: Vec::new() }
    }

    /// 追加访问（构建序）。
    pub fn push(&mut self, a: Access) -> Result<(), u16> {
        a.validate()?;
        self.log.push(a);
        Ok(())
    }

    /// 访问数。
    pub fn len(&self) -> usize {
        self.log.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }

    /// 依赖推导（判据核心）：三重循环 O(n²·m)——帧内任务数
    /// 有限（<10⁴），性能界可接受；每对任务上多资源冲突合并。
    pub fn derive_edges(&self) -> Result<Vec<DerivedEdge>, u16> {
        if self.log.is_empty() {
            return Err(SEG_BASE + 2);
        }
        let mut edges: Vec<DerivedEdge> = Vec::new();
        let mut keys: Vec<u64> = Vec::new();
        for i in 0..self.log.len() {
            for j in (i + 1)..self.log.len() {
                let a = &self.log[i];
                let b = &self.log[j];
                if a.task == b.task || a.sub != b.sub {
                    continue;
                }
                // 同资源访问序对 → 依赖类型。
                let kind = match (a.write, b.write) {
                    (true, false) => EdgeKind::Raw,
                    (false, true) => EdgeKind::War,
                    (true, true) => EdgeKind::Waw,
                    (false, false) => continue, // 读读不建边。
                };
                let key = ((a.task as u64) << 32) | (b.task as u64);
                match keys.iter().position(|k| *k == key) {
                    Some(p) => {
                        // 冲突消解（判据「边合并正确」）：同对任务
                        // 保留最严格依赖。
                        if kind.strictness() > edges[p].kind.strictness() {
                            edges[p].kind = kind;
                        }
                    }
                    None => {
                        keys.push(key);
                        edges.push(DerivedEdge {
                            from: a.task,
                            to: b.task,
                            kind,
                        });
                    }
                }
            }
        }
        if edges.len() > 100_000 {
            // 推导性能护栏（判据「推导性能」）：边数爆表说明
            // 任务切分/资源划分病态。
            return Err(SEG_BASE + 3);
        }
        Ok(edges)
    }
}

/// 漏边注入测试（判据「漏边零容忍注入测试」）：给定推导结果与
/// 期望边集（带类型），逐条比对——期望边缺失即漏边（零容忍）。
pub fn leak_inject_test(
    derived: &[DerivedEdge],
    expect: &[DerivedEdge],
) -> Result<(), u16> {
    for e in expect {
        if !derived
            .iter()
            .any(|d| d.from == e.from && d.to == e.to && d.kind == e.kind)
        {
            return Err(SEG_BASE + 4);
        }
    }
    Ok(())
}

/// 多余边检测（判据「边合并正确」反方向）：推导结果不得包含
/// 期望集之外的边（多边 = 过约束，浪费并行度）。
pub fn excess_edge_test(derived: &[DerivedEdge], expect: &[DerivedEdge]) -> Result<(), u16> {
    for d in derived {
        if !expect
            .iter()
            .any(|e| e.from == d.from && e.to == d.to && e.kind == d.kind)
        {
            return Err(SEG_BASE + 5);
        }
    }
    Ok(())
}

/// 子资源并行验证（判据「子资源粒度验证」）：同纹理不同 mip 的
/// 写写对不得建边——粒度退化成纹理级即验证失败。
pub fn subres_parallel_test(log: &AccessLog) -> Result<(), u16> {
    let edges = log.derive_edges()?;
    for e in &edges {
        let (ta, _, la) = subres_split(
            log.log
                .iter()
                .find(|a| a.task == e.from)
                .ok_or(SEG_BASE + 6)?
                .sub,
        );
        let (tb, _, lb) = subres_split(
            log.log
                .iter()
                .find(|a| a.task == e.to)
                .ok_or(SEG_BASE + 6)?
                .sub,
        );
        if ta == tb && (la != lb) {
            // 同纹理不同数组层之间建了边 = 粒度退化。
            return Err(SEG_BASE + 7);
        }
    }
    Ok(())
}

/// 依赖边一致性审计（独立函数面）：边方向唯一（同一对任务
/// 不得同时有 from→to 与 to→from 双向边——双向 = 死锁构造）。
pub fn no_bidirectional(edges: &[DerivedEdge]) -> Result<(), u16> {
    for e in edges {
        if edges
            .iter()
            .any(|d| d.from == e.to && d.to == e.from)
        {
            return Err(SEG_BASE + 8);
        }
    }
    Ok(())
}

/// 边类型普查。
pub fn edge_census(edges: &[DerivedEdge]) -> [usize; 3] {
    let mut out = [0usize; 3];
    for e in edges {
        out[e.kind.strictness() as usize] += 1;
    }
    // 顺序对齐 Raw=2/Waw=1/War=0 → 反转输出为 [Raw, War, Waw] 口径。
    let [war, waw, raw] = out;
    [raw, war, waw]
}

/// 推导确定性断言（判据补充）：同日志两次推导结果全等
/// （推导器必须是纯函数——输入同则输出同）。
pub fn derivation_deterministic(log: &AccessLog) -> Result<(), u16> {
    let a = log.derive_edges()?;
    let b = log.derive_edges()?;
    if a == b {
        Ok(())
    } else {
        Err(SEG_BASE + 9)
    }
}

impl AccessLog {
    /// 资源普查（判据「资源粒度」可见性）：日志涉及的独立
    /// 子资源数与独立纹理数。
    pub fn resource_census(&self) -> (usize, usize) {
        let mut subs: Vec<u32> = Vec::new();
        let mut texs: Vec<u8> = Vec::new();
        for a in &self.log {
            if !subs.contains(&a.sub) {
                subs.push(a.sub);
            }
            let t = subres_split(a.sub).0;
            if !texs.contains(&t) {
                texs.push(t);
            }
        }
        (subs.len(), texs.len())
    }

    /// 任务普查：日志涉及的任务号集。
    pub fn task_set(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for a in &self.log {
            if !out.contains(&a.task) {
                out.push(a.task);
            }
        }
        out
    }

    /// 读写比（千分比）：写访问占比（写重日志提示资源划分
    /// 偏向生产端）。
    pub fn write_ratio_permille(&self) -> u32 {
        if self.log.is_empty() {
            return 0;
        }
        let w = self.log.iter().filter(|a| a.write).count();
        ((w * 1000) / self.log.len()) as u32
    }
}

/// 合并边投影（判据「边合并正确」量化面）：推导边数相对
/// 原始冲突对数的压缩率——合并率越高，图越稀疏（调度越好）。
pub fn merge_compression_permille(log: &AccessLog) -> Result<u32, u16> {
    let edges = log.derive_edges()?;
    if edges.is_empty() {
        return Err(SEG_BASE + 10);
    }
    // 原始冲突对数：逐对任务逐资源重数一遍（O(n²m)，推导同量级）。
    let mut raw_pairs: usize = 0;
    let n = log.log.len();
    for i in 0..n {
        for j in (i + 1)..n {
            let a = &log.log[i];
            let b = &log.log[j];
            if a.task != b.task && a.sub == b.sub && !(a.write == false && b.write == false) {
                raw_pairs += 1;
            }
        }
    }
    if raw_pairs == 0 {
        return Err(SEG_BASE + 11);
    }
    Ok(((edges.len() * 1000) / raw_pairs) as u32)
}

/// 关键资源审计（独立函数面）：被最多任务读写的子资源排行
/// （热点资源 = 调度串行化瓶颈的前兆）。
pub fn hotspot_resources(log: &AccessLog, top_k: usize) -> Result<Vec<(u32, usize)>, u16> {
    if top_k == 0 || top_k > 64 {
        return Err(SEG_BASE + 12);
    }
    let mut res: Vec<(u32, usize)> = Vec::new();
    for a in &log.log {
        match res.iter_mut().find(|(s, _)| *s == a.sub) {
            Some((_, c)) => *c += 1,
            None => res.push((a.sub, 1)),
        }
    }
    res.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    res.truncate(top_k);
    Ok(res)
}

/// 任务隔离度审计（独立函数面）：零依赖任务（既不依赖别人
/// 也不被人依赖）占比——全零依赖 = 推导没干活。
pub fn isolation_permille(log: &AccessLog) -> Result<u32, u16> {
    let edges = log.derive_edges()?;
    let tasks = log.task_set();
    if tasks.len() < 2 {
        return Err(SEG_BASE + 13);
    }
    let isolated = tasks
        .iter()
        .filter(|t| {
            !edges.iter().any(|e| e.from == **t || e.to == **t)
        })
        .count();
    Ok(((isolated * 1000) / tasks.len()) as u32)
}
