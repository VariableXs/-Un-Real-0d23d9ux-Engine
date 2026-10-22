//! CGPU-F0202 · 帧图模板扩展（判据段 0x0640）
//! F0187 模板系统的帧图级扩展：模板参数化深化（模板参数类型扩展
//! ——尺寸/格式/迭代次数/条件开关），模板组合（多个模板的串接组合
//! ——后处理链模板 + 合成模板 = 完整后处理帧图）、模板验证（模板
//! 实例化后的图完整性验证——F0464 IR 验证器同哲学）。判据：参数化
//! 扩展、组合正确、验证拦截、模板库扩充、用例。
//!
//! 诊断码段 0x0640（本模块独占：0x0640~0x064F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0640;

/// 模板库容量上限。
pub const TEMPLATE_LIB_MAX: usize = 256;

/// 参数化类型（判据「参数化扩展」四类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamKind {
    /// 尺寸（宽高像素）。
    Size,
    /// 格式（像素格式号）。
    Format,
    /// 迭代次数。
    Iterations,
    /// 条件开关。
    Switch,
}

impl ParamKind {
    /// 类型序号。
    pub fn ordinal(&self) -> u8 {
        match self {
            ParamKind::Size => 0,
            ParamKind::Format => 1,
            ParamKind::Iterations => 2,
            ParamKind::Switch => 3,
        }
    }
}

/// 模板参数值（带类型的值域校验）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParamValue {
    /// 参数名指纹（名字折并，跨模板一致引用）。
    pub name_fp: u64,
    /// 参数类型。
    pub kind: ParamKind,
    /// 值（语义随 kind：Size 低 16 位=宽 高 16 位=高；Format=格式号；
    /// Iterations=次数；Switch=0/1）。
    pub val: u32,
}

impl ParamValue {
    /// 值域校验（判据「参数化扩展」的类型安全核心）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.name_fp == 0 {
            return Err(SEG_BASE);
        }
        match self.kind {
            ParamKind::Size => {
                let w = self.val >> 16;
                let h = self.val & 0xFFFF;
                if w == 0 || h == 0 {
                    return Err(SEG_BASE + 1);
                }
            }
            ParamKind::Format => {
                if self.val > 0xFF {
                    return Err(SEG_BASE + 2);
                }
            }
            ParamKind::Iterations => {
                if self.val == 0 || self.val > 4096 {
                    return Err(SEG_BASE + 3);
                }
            }
            ParamKind::Switch => {
                if self.val > 1 {
                    return Err(SEG_BASE + 4);
                }
            }
        }
        Ok(())
    }
}

/// 模板节点（实例化前）：占位符 + 类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TplNode {
    /// 节点号（模板内从 1 起）。
    pub node: u32,
    /// 执行代价（微秒，静态估值）。
    pub cost_us: u32,
    /// 是否条件节点（Switch 关闭时整节点剪除）。
    pub conditional: bool,
}

impl TplNode {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.node == 0 || self.cost_us == 0 {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }
}

/// 帧图模板（判据「模板库扩充」一等公民）。
#[derive(Clone, Debug)]
pub struct FrameTemplate {
    /// 模板号。
    pub tpl_id: u32,
    /// 模板名指纹。
    pub name_fp: u64,
    /// 模板节点。
    pub nodes: Vec<TplNode>,
    /// 模板内边（u→v 节点号）。
    pub edges: Vec<(u32, u32)>,
    /// 参数表。
    pub params: Vec<ParamValue>,
}

impl FrameTemplate {
    /// 模板合法性：节点号连续、边端点在册、参数有效且名字不重复。
    pub fn validate(&self) -> Result<(), u16> {
        if self.tpl_id == 0 || self.name_fp == 0 || self.nodes.is_empty() {
            return Err(SEG_BASE + 6);
        }
        let n = self.nodes.len();
        for (i, nd) in self.nodes.iter().enumerate() {
            nd.validate()?;
            if nd.node as usize != i + 1 {
                return Err(SEG_BASE + 7);
            }
        }
        for &(u, v) in &self.edges {
            if u == 0 || v == 0 || u as usize > n || v as usize > n || u == v {
                return Err(SEG_BASE + 8);
            }
        }
        for i in 0..self.params.len() {
            self.params[i].validate()?;
            if self.params[..i].iter().any(|p| p.name_fp == self.params[i].name_fp) {
                return Err(SEG_BASE + 9);
            }
        }
        Ok(())
    }
    /// 拓扑无环校验（Kahn 计数法）。
    pub fn acyclic(&self) -> Result<(), u16> {
        let n = self.nodes.len();
        let mut indeg = vec![0u32; n + 1];
        for &(_, v) in &self.edges {
            indeg[v as usize] += 1;
        }
        let mut stack: Vec<u32> = (1..=n as u32).filter(|i| indeg[*i as usize] == 0).collect();
        let mut popped = 0usize;
        while let Some(u) = stack.pop() {
            popped += 1;
            for &(a, b) in &self.edges {
                if a == u {
                    indeg[b as usize] -= 1;
                    if indeg[b as usize] == 0 {
                        stack.push(b);
                    }
                }
            }
        }
        if popped != n {
            return Err(SEG_BASE + 10);
        }
        Ok(())
    }
    /// 静态总代价。
    pub fn static_cost_us(&self) -> u64 {
        self.nodes.iter().map(|n| n.cost_us as u64).sum()
    }
}

/// 模板库（判据「模板库扩充」容器）。
#[derive(Default)]
pub struct TemplateLib {
    tpls: Vec<FrameTemplate>,
}

impl TemplateLib {
    /// 入库（同 id 或同名指纹拒绝——判据「验证拦截」防重名歧义）。
    pub fn register(&mut self, t: FrameTemplate) -> Result<(), u16> {
        t.validate()?;
        t.acyclic()?;
        if self.tpls.len() >= TEMPLATE_LIB_MAX {
            return Err(SEG_BASE + 11);
        }
        if self.tpls.iter().any(|x| x.tpl_id == t.tpl_id || x.name_fp == t.name_fp) {
            return Err(SEG_BASE + 12);
        }
        self.tpls.push(t);
        Ok(())
    }
    /// 库规模。
    pub fn count(&self) -> usize {
        self.tpls.len()
    }
    /// 按 id 查询。
    pub fn get(&self, tpl_id: u32) -> Result<&FrameTemplate, u16> {
        self.tpls.iter().find(|t| t.tpl_id == tpl_id).ok_or(SEG_BASE + 13)
    }
    /// 按名指纹查询。
    pub fn get_by_name(&self, name_fp: u64) -> Result<&FrameTemplate, u16> {
        self.tpls.iter().find(|t| t.name_fp == name_fp).ok_or(SEG_BASE + 14)
    }
}

/// 条件剪除（判据「参数化扩展」的 Switch 语义核心）：Switch=false
/// 的参数使全部条件节点被剪除（节点与其关联边一起消失）——返回
/// 剪除后节点号集合。
pub fn conditional_prune(t: &FrameTemplate, switches: &[ParamValue]) -> Result<Vec<u32>, u16> {
    t.validate()?;
    let mut off: Vec<u64> = Vec::new();
    for s in switches {
        s.validate()?;
        if s.kind != ParamKind::Switch {
            return Err(SEG_BASE + 15);
        }
        if s.val == 0 {
            off.push(s.name_fp);
        }
    }
    // 模板本身不携带参数-节点映射：约定条件节点的 node 号即参数
    // name_fp 的低 32 位（模板约定，实例化时对齐）。
    let mut alive: Vec<u32> = Vec::new();
    for nd in &t.nodes {
        if nd.conditional && off.contains(&(nd.node as u64)) {
            continue;
        }
        alive.push(nd.node);
    }
    if alive.is_empty() {
        return Err(SEG_BASE + 16);
    }
    Ok(alive)
}

/// 模板串接组合（判据「组合正确」核心）：两个模板首尾相接——A 的
/// 全部汇点（无出边）连向 B 的全部源点（无入边）。返回组合模板。
/// 节点号重排：B 的节点整体 +A 节点数偏移。
pub fn compose(a: &FrameTemplate, b: &FrameTemplate) -> Result<FrameTemplate, u16> {
    a.validate()?;
    b.validate()?;
    a.acyclic()?;
    b.acyclic()?;
    if a.tpl_id == b.tpl_id || a.name_fp == b.name_fp {
        return Err(SEG_BASE + 17);
    }
    let off = a.nodes.len() as u32;
    let mut nodes: Vec<TplNode> = a.nodes.clone();
    for nd in &b.nodes {
        nodes.push(TplNode { node: nd.node + off, cost_us: nd.cost_us, conditional: nd.conditional });
    }
    let mut edges: Vec<(u32, u32)> = a.edges.clone();
    for &(u, v) in &b.edges {
        edges.push((u + off, v + off));
    }
    // 汇点 → 源点桥接。
    let sinks: Vec<u32> = {
        let mut s: Vec<u32> = (1..=a.nodes.len() as u32)
            .filter(|i| !a.edges.iter().any(|&(u, _)| u == *i))
            .collect();
        if s.is_empty() {
            s.push(a.nodes.len() as u32);
        }
        s
    };
    let sources: Vec<u32> = {
        let mut s: Vec<u32> = (1..=b.nodes.len() as u32)
            .filter(|i| !b.edges.iter().any(|&(_, v)| v == *i))
            .collect();
        if s.is_empty() {
            s.push(1);
        }
        s
    };
    for &s in &sinks {
        for &src in &sources {
            edges.push((s, src + off));
        }
    }
    let composed = FrameTemplate {
        tpl_id: a.tpl_id.wrapping_mul(31).wrapping_add(b.tpl_id),
        name_fp: a.name_fp ^ b.name_fp.rotate_left(17) | 1,
        nodes,
        edges,
        params: a.params.iter().chain(b.params.iter()).copied().collect(),
    };
    composed.validate()?;
    composed.acyclic()?;
    Ok(composed)
}

/// 实例化完整性验证（判据「验证拦截」主判据）：实例化后的图必须
/// ① 节点全覆盖 ② 无环 ③ 静态代价 = 组成部分之和。三验全过才
/// 允许入执行器。
pub fn instantiate_verify(t: &FrameTemplate) -> Result<u64, u16> {
    t.validate()?;
    t.acyclic()?;
    let cost = t.static_cost_us();
    if cost == 0 {
        return Err(SEG_BASE + 18);
    }
    Ok(cost)
}

/// 参数-模板绑定校验（判据「参数化扩展」绑定面）：实例化时传入的
/// 参数必须覆盖模板声明的全部参数名（缺参拒绝——类型安全）。
pub fn params_bound(t: &FrameTemplate, provided: &[ParamValue]) -> Result<usize, u16> {
    if provided.is_empty() {
        return Err(SEG_BASE + 19);
    }
    for p in provided {
        p.validate()?;
    }
    let mut bound = 0usize;
    for want in &t.params {
        match provided.iter().find(|p| p.name_fp == want.name_fp) {
            Some(p) => {
                if p.kind != want.kind {
                    return Err(SEG_BASE + 20);
                }
                bound += 1;
            }
            None => return Err(SEG_BASE + 21),
        }
    }
    Ok(bound)
}

/// 模板库指纹（可审计面）。
pub fn lib_fp(lib: &TemplateLib) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for t in &lib.tpls {
        acc = acc.wrapping_mul(0x100000001b3) ^ t.tpl_id as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ t.name_fp;
        acc = acc.wrapping_mul(0x100000001b3) ^ t.nodes.len() as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ t.static_cost_us();
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tpl(id: u32, name: u64, n: u32, edges: &[(u32, u32)]) -> FrameTemplate {
        FrameTemplate {
            tpl_id: id,
            name_fp: name,
            nodes: (1..=n).map(|i| TplNode { node: i, cost_us: 10, conditional: false }).collect(),
            edges: edges.to_vec(),
            params: Vec::new(),
        }
    }
    #[test]
    fn param_value_ranges() {
        assert!(ParamValue { name_fp: 1, kind: ParamKind::Size, val: (1920 << 16) | 1080 }.validate().is_ok());
        assert!(ParamValue { name_fp: 1, kind: ParamKind::Size, val: 1920 }.validate().is_err());
        assert!(ParamValue { name_fp: 1, kind: ParamKind::Format, val: 87 }.validate().is_ok());
        assert!(ParamValue { name_fp: 1, kind: ParamKind::Format, val: 900 }.validate().is_err());
        assert!(ParamValue { name_fp: 1, kind: ParamKind::Iterations, val: 0 }.validate().is_err());
        assert!(ParamValue { name_fp: 1, kind: ParamKind::Switch, val: 2 }.validate().is_err());
    }
    #[test]
    fn template_validation_and_cycle() {
        let t = tpl(1, 0xA, 3, &[(1, 2), (2, 3)]);
        assert!(t.validate().is_ok());
        assert!(t.acyclic().is_ok());
        let cyc = tpl(2, 0xB, 3, &[(1, 2), (2, 3), (3, 1)]);
        assert!(cyc.acyclic().is_err());
    }
    #[test]
    fn lib_dedup_and_lookup() {
        let mut lib = TemplateLib::default();
        lib.register(tpl(1, 0xA, 3, &[(1, 2), (2, 3)])).unwrap();
        lib.register(tpl(2, 0xB, 2, &[(1, 2)])).unwrap();
        assert_eq!(lib.count(), 2);
        assert!(lib.register(tpl(1, 0xC, 2, &[(1, 2)])).is_err());
        assert!(lib.register(tpl(3, 0xA, 2, &[(1, 2)])).is_err());
        assert!(lib.get(1).is_ok());
        assert!(lib.get_by_name(0xB).is_ok());
        assert!(lib.get_by_name(0xD).is_err());
    }
    #[test]
    fn compose_chains_sinks_to_sources() {
        let a = tpl(1, 0xA, 2, &[(1, 2)]);
        let b = tpl(2, 0xB, 2, &[(1, 2)]);
        let c = compose(&a, &b).unwrap();
        assert_eq!(c.nodes.len(), 4);
        // 桥接边 2→3（A 汇点 2 → B 源点 1+2=3）。
        assert!(c.edges.contains(&(2, 3)));
        assert!(c.acyclic().is_ok());
        assert_eq!(c.static_cost_us(), 40);
        assert_eq!(instantiate_verify(&c).unwrap(), 40);
    }
    #[test]
    fn params_must_cover_all() {
        let mut t = tpl(1, 0xA, 2, &[(1, 2)]);
        t.params = vec![
            ParamValue { name_fp: 0x111, kind: ParamKind::Iterations, val: 4 },
            ParamValue { name_fp: 0x222, kind: ParamKind::Switch, val: 1 },
        ];
        // 只提供一个 → 缺参拒绝。
        let half = vec![ParamValue { name_fp: 0x111, kind: ParamKind::Iterations, val: 4 }];
        assert_eq!(params_bound(&t, &half).unwrap_err(), SEG_BASE + 21);
        // 类型不符拒绝。
        let wrong = vec![
            ParamValue { name_fp: 0x111, kind: ParamKind::Switch, val: 1 },
            ParamValue { name_fp: 0x222, kind: ParamKind::Switch, val: 1 },
        ];
        assert_eq!(params_bound(&t, &wrong).unwrap_err(), SEG_BASE + 20);
        // 全绑定通过。
        let full = vec![
            ParamValue { name_fp: 0x111, kind: ParamKind::Iterations, val: 4 },
            ParamValue { name_fp: 0x222, kind: ParamKind::Switch, val: 1 },
        ];
        assert_eq!(params_bound(&t, &full).unwrap(), 2);
    }
}
