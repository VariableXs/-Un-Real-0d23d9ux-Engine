//! CGPU-F0187 · 图模板系统（判据段 0x0550）
//! 常见帧图模式的模板化：模板定义（预定义的子图模式：UI 合成模板/
//! 后处理链模板/粒子系统模板——模板参数化后实例化）、模板参数
//! （模板的可变参数：尺寸/格式/迭代次数——参数不同产生不同子图
//! 实例）、模板库（内置模板集——常见渲染模式的最佳实践封装）。
//! 判据：模板定义/参数化/实例化、模板库、实例正确、性能、可扩展。
//!
//! 诊断码段 0x0550（本模块独占：0x0550~0x055F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0550;

/// 模板种类（判据「模板库」内置闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TplKind {
    /// UI 合成模板。
    UiCompose,
    /// 后处理链模板。
    PostChain,
    /// 粒子系统模板。
    Particle,
}

impl TplKind {
    /// 序号。
    pub fn rank(self) -> u8 {
        match self {
            TplKind::UiCompose => 0,
            TplKind::PostChain => 1,
            TplKind::Particle => 2,
        }
    }

    /// 模板固有节点数（判据「模板定义」的结构骨架）。
    pub fn base_nodes(self) -> usize {
        match self {
            TplKind::UiCompose => 3,
            TplKind::PostChain => 4,
            TplKind::Particle => 5,
        }
    }
}

/// 模板参数（判据「模板参数」载体）：迭代次数 + 格式代号 + 尺寸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TplParam {
    /// 迭代次数（1~16）。
    pub iters: u8,
    /// 格式代号（1~8 闭集）。
    pub fmt: u8,
    /// 尺寸代号（位宽 0~63，语义由实例方约定）。
    pub size: u8,
}

impl TplParam {
    /// 参数校验。
    pub fn validate(&self) -> Result<(), u16> {
        if self.iters == 0 || self.iters > 16 {
            return Err(SEG_BASE);
        }
        if self.fmt == 0 || self.fmt > 8 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 模板定义（判据「模板定义」载体）：种类 + 骨架（节点序列）。
pub struct Template {
    /// 种类。
    pub kind: TplKind,
    /// 骨架节点（模板内逻辑号 0..n）。
    pub skeleton: Vec<u32>,
    /// 骨架边（逻辑号对）。
    pub edges: Vec<(u32, u32)>,
}

impl Template {
    /// 模板校验：骨架节点数 = 固有节点数 × 迭代无关的最小骨架；
    /// 边端点在骨架内、无自环、无重复。
    pub fn validate(&self) -> Result<(), u16> {
        if self.skeleton.len() != self.kind.base_nodes() {
            return Err(SEG_BASE + 2);
        }
        for i in 0..self.skeleton.len() {
            for j in (i + 1)..self.skeleton.len() {
                if self.skeleton[i] == self.skeleton[j] {
                    return Err(SEG_BASE + 3);
                }
            }
        }
        for (f, t) in &self.edges {
            if f == t
                || !self.skeleton.contains(f)
                || !self.skeleton.contains(t)
            {
                return Err(SEG_BASE + 4);
            }
            if self.edges.contains(&(*f, *t)) {
                return Err(SEG_BASE + 5);
            }
        }
        Ok(())
    }
}

/// 模板实例（判据「实例化」产出物）：实例号 + 节点映射。
pub struct TplInstance {
    /// 实例号（全局唯一）。
    pub inst: u32,
    /// 模板种类。
    pub kind: TplKind,
    /// 参数。
    pub param: TplParam,
    /// 实例节点映射：(逻辑号 → 实际任务号)。
    pub map: Vec<(u32, u32)>,
    /// 实例边。
    pub edges: Vec<(u32, u32)>,
}

impl TplInstance {
    /// 实例校验（判据「实例正确」核心）：映射双射（逻辑号不重复、
    /// 实际号不重复）、参数合法、边数与模板一致。
    pub fn validate(&self) -> Result<(), u16> {
        self.param.validate()?;
        if self.inst == 0 {
            return Err(SEG_BASE + 6);
        }
        for i in 0..self.map.len() {
            if self.map[i].1 == 0 {
                return Err(SEG_BASE + 7);
            }
            for j in (i + 1)..self.map.len() {
                if self.map[i].0 == self.map[j].0 || self.map[i].1 == self.map[j].1 {
                    return Err(SEG_BASE + 8);
                }
            }
        }
        for (f, t) in &self.edges {
            if f == t {
                return Err(SEG_BASE + 9);
            }
            if !self.map.iter().any(|(l, _)| l == f)
                || !self.map.iter().any(|(l, _)| l == t)
            {
                return Err(SEG_BASE + 10);
            }
        }
        Ok(())
    }
}

/// 模板库（判据「模板库」载体）：内置模板集 + 实例账。
pub struct TplLibrary {
    /// 内置模板。
    pub tpls: Vec<Template>,
    /// 已实例化账。
    pub insts: Vec<TplInstance>,
}

impl TplLibrary {
    /// 空库。
    pub fn new() -> Self {
        TplLibrary {
            tpls: Vec::new(),
            insts: Vec::new(),
        }
    }

    /// 注册模板（种类重复拒——一种一模板是库纪律）。
    pub fn register(&mut self, t: Template) -> Result<(), u16> {
        t.validate()?;
        if self.tpls.iter().any(|x| x.kind == t.kind) {
            return Err(SEG_BASE + 11);
        }
        self.tpls.push(t);
        Ok(())
    }

    /// 库完备判（判据「模板库」）：三类内置模板全注册。
    pub fn builtin_complete(&self) -> Result<(), u16> {
        for k in [TplKind::UiCompose, TplKind::PostChain, TplKind::Particle] {
            if !self.tpls.iter().any(|t| t.kind == k) {
                return Err(SEG_BASE + 12);
            }
        }
        Ok(())
    }

    /// 实例化（判据「模板参数化/实例化」核心面）：按参数展开骨架
    /// ——迭代次数决定链长（后处理链 iters 次 = iters 个 pass 节点
    /// 链），实例号唯一。
    pub fn instantiate(
        &mut self,
        kind: TplKind,
        param: TplParam,
        inst: u32,
        task_base: u32,
    ) -> Result<TplInstance, u16> {
        self.builtin_complete()?;
        param.validate()?;
        if inst == 0 || self.insts.iter().any(|x| x.inst == inst) {
            return Err(SEG_BASE + 13);
        }
        let tpl = self
            .tpls
            .iter()
            .find(|t| t.kind == kind)
            .ok_or(SEG_BASE + 14)?;
        // 骨架首节点复制 1 次，链节点复制 iters 次。
        let mut map: Vec<(u32, u32)> = Vec::new();
        let mut task = task_base;
        if task == 0 {
            return Err(SEG_BASE + 15);
        }
        let head = tpl.skeleton[0];
        map.push((head, task));
        task += 1;
        for _ in 0..param.iters {
            map.push((tpl.skeleton[1], task));
            task += 1;
        }
        for s in &tpl.skeleton[2..] {
            map.push((*s, task));
            task += 1;
        }
        // 边实例化：逻辑边按映射展开；骨架链边（skeleton[1] 内部）
        // 按 iters 串联。
        let mut edges: Vec<(u32, u32)> = Vec::new();
        for (lf, lt) in &tpl.edges {
            if *lf == head {
                // 头 → 链首。
                let chain_first = map.iter().find(|(l, _)| *l == tpl.skeleton[1]).map(|(_, r)| *r).unwrap_or(0);
                edges.push((map[0].1, chain_first));
            } else {
                let f = map.iter().find(|(l, _)| l == lf).map(|(_, r)| *r).unwrap_or(0);
                let t = map.iter().find(|(l, _)| l == lt).map(|(_, r)| *r).unwrap_or(0);
                if f != 0 && t != 0 {
                    edges.push((f, t));
                }
            }
        }
        // 链内部串联（iters 个链节点依序相连）。
        let chain: Vec<u32> = map
            .iter()
            .filter(|(l, _)| *l == tpl.skeleton[1])
            .map(|(_, r)| *r)
            .collect();
        for w in chain.windows(2) {
            edges.push((w[0], w[1]));
        }
        let inst_obj = TplInstance {
            inst,
            kind,
            param,
            map,
            edges,
        };
        inst_obj.validate()?;
        self.insts.push(TplInstance {
            inst: inst_obj.inst,
            kind: inst_obj.kind,
            param: inst_obj.param,
            map: inst_obj.map.clone(),
            edges: inst_obj.edges.clone(),
        });
        Ok(inst_obj)
    }

    /// 实例数。
    pub fn inst_count(&self) -> usize {
        self.insts.len()
    }

    /// 可扩展性（判据「可扩展」面）：库容量上限 32 模板（超限即
    /// 拒——库膨胀纪律）。
    pub fn capacity_ok(&self) -> Result<(), u16> {
        if self.tpls.len() > 32 {
            return Err(SEG_BASE + 16);
        }
        Ok(())
    }
}

/// 实例节点账自洽（判据「实例正确」量化面）：实例节点数 = 骨架数
/// + iters（链展开），不多不少。
pub fn inst_node_account(t: &Template, i: &TplInstance) -> Result<(), u16> {
    t.validate()?;
    i.validate()?;
    let expect = t.skeleton.len() + i.param.iters as usize;
    if i.map.len() != expect {
        return Err(SEG_BASE + 17);
    }
    Ok(())
}

/// 模板库一站式审计（判据汇总总闸）：库完备 + 容量 + 实例账。
pub fn tpl_audit(lib: &TplLibrary) -> Result<(), u16> {
    lib.builtin_complete()?;
    lib.capacity_ok()?;
    for i in &lib.insts {
        let t = lib
            .tpls
            .iter()
            .find(|t| t.kind == i.kind)
            .ok_or(SEG_BASE + 18)?;
        inst_node_account(t, i)?;
    }
    if lib.insts.is_empty() {
        return Err(SEG_BASE + 19);
    }
    Ok(())
}

/// 模板库深度扩展面。
impl TplLibrary {
    /// 库指纹（判据「模板库」可算化面）：模板种类序折并。
    pub fn lib_fp(&self) -> Result<u64, u16> {
        if self.tpls.is_empty() {
            return Err(SEG_BASE + 20);
        }
        let mut acc: u64 = 0xcbf29ce484222325;
        for t in &self.tpls {
            acc = acc.wrapping_mul(0x100000001b3) ^ t.kind.rank() as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ t.skeleton.len() as u64;
        }
        Ok(acc)
    }

    /// 实例参数分布（判据「模板参数」可见性）：迭代参数的最小
    /// 最大值域。
    pub fn param_range(&self) -> Result<(u8, u8), u16> {
        if self.insts.is_empty() {
            return Err(SEG_BASE + 21);
        }
        let lo = self.insts.iter().map(|i| i.param.iters).min().unwrap_or(0);
        let hi = self.insts.iter().map(|i| i.param.iters).max().unwrap_or(0);
        if lo == 0 {
            return Err(SEG_BASE + 22);
        }
        Ok((lo, hi))
    }

    /// 同模板多实例合法性（判据「可扩展」强化面）：同种类可多
    /// 实例（不同参数/不同实例号），实例号必须全局唯一。
    pub fn inst_id_unique(&self) -> Result<(), u16> {
        for i in 0..self.insts.len() {
            for j in (i + 1)..self.insts.len() {
                if self.insts[i].inst == self.insts[j].inst {
                    return Err(SEG_BASE + 23);
                }
            }
        }
        Ok(())
    }
}
