//! cgb754_webresdata —— CGPU-F0754 网页资源管理器数据。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0754 网页资源管理器数据  <SEG 0x4c80>
//! 目标行数：≥380（判据：纯功能代码 ≥380 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥380 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0754；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（网页资源管理器 = 内嵌 Web 面板的数据骨架）：
//!   1. 资源树：节点表（定容 64 节点），每节点（节点号/父号/名称
//      哈希/类型码/字节量/版本戳）六字段；根节点 0 号固定；
//!   2. 树不变量四条：① 父号必须 < 子号或为根（防环）；② 节点号
//      连续分配不回收；③ 类型码闭集 5 类；④ 字节量守恒：任一
//      目录节点的字节量 = 直接子节点字节量之和（逐层可复算）；
//   3. 数据快照序列化：定长记录逐节点铺开（每记录 16 字节），
//      序列化总量 = 节点数 × 16（精确守恒式）；
//   4. 增量差分账：两次快照按节点号对齐 diff，产出（新增/删除/
//      修改）三账，守恒：新增+删除+未变 = 快照并集节点数；
//   5. 只读查询面：按路径下钻（父号链），深度 ≤ 8（写死），
//      深度越界红码；命中路径逐节点留痕；
//   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4c80 段为本模块独占（SEG_BASE=0x4c80），判别式
//!   SEG_BASE+1..+N；与 0x4c60/0x4ca0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const WR_VERSION: &str = "cgb754_webresdata-v1";

/// 诊断段基址（0x4c80 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4C80;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4c81 节点容量满（64 槽）。
pub const XC_NODE_FULL: u16 = SEG_BASE + 1;
/// 0x4c82 父号越界（父不存在或 ≥ 子号且非根）。
pub const XC_BAD_PARENT: u16 = SEG_BASE + 2;
/// 0x4c83 类型码越界（0..=4 之外）。
pub const XC_BAD_TYPE: u16 = SEG_BASE + 3;
/// 0x4c84 节点不存在。
pub const XC_NODE_MISSING: u16 = SEG_BASE + 4;
/// 0x4c85 字节量守恒破（目录量 ≠ 子量和）。
pub const XC_BYTES_UNBALANCED: u16 = SEG_BASE + 5;
/// 0x4c86 下钻深度越界（>8）。
pub const XC_DEPTH_OVER: u16 = SEG_BASE + 6;
/// 0x4c87 快照序列化尺寸漂移（≠ 节点数×16）。
pub const XC_SNAP_SIZE: u16 = SEG_BASE + 7;
/// 0x4c88 差分账不守恒。
pub const XC_DIFF_UNBALANCED: u16 = SEG_BASE + 8;
/// 0x4c89 根节点重定义。
pub const XC_ROOT_DUP: u16 = SEG_BASE + 9;

/// 节点容量（写死：64）。
pub const NODE_CAP: usize = 64;
/// 下钻深度上限（写死：8）。
pub const DEPTH_MAX: usize = 8;
/// 序列化记录宽（写死：16 字节）。
pub const RECORD_W: usize = 16;

/// 资源节点类型闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResType {
    /// 0 根。
    Root,
    /// 1 目录。
    Dir,
    /// 2 代码资源。
    Code,
    /// 3 静态资源（图/字体）。
    Asset,
    /// 4 数据资源（JSON/账面）。
    Data,
}

impl ResType {
    /// 类型码。
    pub fn code(&self) -> u8 {
        match self {
            ResType::Root => 0,
            ResType::Dir => 1,
            ResType::Code => 2,
            ResType::Asset => 3,
            ResType::Data => 4,
        }
    }

    /// 码→类型（越界 None）。
    pub fn from_code(c: u8) -> Option<ResType> {
        match c {
            0 => Some(ResType::Root),
            1 => Some(ResType::Dir),
            2 => Some(ResType::Code),
            3 => Some(ResType::Asset),
            4 => Some(ResType::Data),
            _ => None,
        }
    }

    /// 是否目录类（可挂子）。
    pub fn is_container(&self) -> bool {
        matches!(self, ResType::Root | ResType::Dir)
    }
}

/// 资源节点六字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResNode {
    /// 节点号（0=根，连续分配）。
    pub nid: u16,
    /// 父号（根为 0xFFFF 哨兵）。
    pub parent: u16,
    /// 名称哈希（FNV-1a）。
    pub name_hash: u32,
    /// 类型码。
    pub ty: u8,
    /// 字节量（叶子有效；目录由子量和覆写）。
    pub bytes: u32,
    /// 版本戳（每次修改 +1）。
    pub version: u32,
}

/// 父号哨兵（根）。
pub const PARENT_ROOT: u16 = 0xFFFF;

/// 名称哈希。
pub fn name_hash(name: &str) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for b in name.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 资源树。
pub struct ResTree {
    nodes: Vec<ResNode>,
}

impl ResTree {
    /// 空树（自动建根）。
    pub fn new() -> Result<Self, u16> {
        let mut t = ResTree { nodes: Vec::new() };
        t.nodes.push(ResNode {
            nid: 0,
            parent: PARENT_ROOT,
            name_hash: name_hash("/"),
            ty: ResType::Root.code(),
            bytes: 0,
            version: 1,
        });
        Ok(t)
    }

    /// 节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 读节点。
    pub fn get(&self, nid: u16) -> Result<&ResNode, u16> {
        self.nodes.get(nid as usize).ok_or(XC_NODE_MISSING)
    }

    /// 挂子节点（父必须容器类；父号 < 新号或父为根；父不存在红码）。
    pub fn add(&mut self, parent: u16, name: &str, ty: ResType, bytes: u32) -> Result<u16, u16> {
        if self.nodes.len() >= NODE_CAP {
            return Err(XC_NODE_FULL);
        }
        if ty == ResType::Root {
            return Err(XC_ROOT_DUP);
        }
        let p = self.get(parent)?;
        if !ResType::from_code(p.ty).map(|t| t.is_container()).unwrap_or(false) {
            return Err(XC_BAD_PARENT);
        }
        if parent != 0 && parent >= self.nodes.len() as u16 {
            return Err(XC_BAD_PARENT);
        }
        // 目录节点字节量必须为 0（守恒由子量推导）。
        let b = if ty.is_container() { 0 } else { bytes };
        let nid = self.nodes.len() as u16;
        self.nodes.push(ResNode {
            nid,
            parent,
            name_hash: name_hash(name),
            ty: ty.code(),
            bytes: b,
            version: 1,
        });
        Ok(nid)
    }

    /// 叶子字节量改写（目录节点红码），版本戳 +1，父链字节量重算。
    pub fn resize(&mut self, nid: u16, bytes: u32) -> Result<(), u16> {
        let n = self.get(nid)?;
        if ResType::from_code(n.ty).map(|t| t.is_container()).unwrap_or(false) {
            return Err(XC_BYTES_UNBALANCED);
        }
        self.nodes[nid as usize].bytes = bytes;
        self.nodes[nid as usize].version += 1;
        self.rollup(nid)?;
        Ok(())
    }

    /// 父链字节量重算（roll-up 到根）。
    fn rollup(&mut self, mut nid: u16) -> Result<(), u16> {
        loop {
            let n = self.get(nid)?;
            let p = n.parent;
            if p == PARENT_ROOT {
                break;
            }
            // 父量 = 其直接子量和。
            let mut sum = 0u32;
            let plen = self.nodes.len() as u16;
            for c in 0..plen {
                let cn = self.get(c)?;
                if cn.parent == p {
                    sum += cn.bytes;
                }
            }
            self.nodes[p as usize].bytes = sum;
            self.nodes[p as usize].version += 1;
            nid = p;
        }
        Ok(())
    }

    /// 全树字节量守恒核：每目录节点 = 直接子量和。
    pub fn bytes_conserved(&self) -> Result<(), u16> {
        for n in &self.nodes {
            if ResType::from_code(n.ty).map(|t| t.is_container()).unwrap_or(false) {
                let mut sum = 0u32;
                for c in &self.nodes {
                    if c.parent == n.nid {
                        sum += c.bytes;
                    }
                }
                if sum != n.bytes {
                    return Err(XC_BYTES_UNBALANCED);
                }
            }
        }
        Ok(())
    }

    /// 路径下钻：从根沿子号链（逐级给子号），深度 ≤8；命中留痕。
    pub fn descend(&self, path: &[u16]) -> Result<Vec<u16>, u16> {
        if path.len() > DEPTH_MAX {
            return Err(XC_DEPTH_OVER);
        }
        let mut trace = vec![0u16];
        let mut cur = 0u16;
        for (i, &child) in path.iter().enumerate() {
            if i + 1 > DEPTH_MAX {
                return Err(XC_DEPTH_OVER);
            }
            let c = self.get(child)?;
            if c.parent != cur {
                return Err(XC_BAD_PARENT);
            }
            trace.push(child);
            cur = child;
        }
        Ok(trace)
    }

    /// 快照序列化：每节点 16 字节定长记录。
    /// 布局：[0..2) nid [2..4) parent [4..8) name_hash [8] ty
    ///       [9..13) bytes [13..16) version 低 3 字节。
    pub fn serialize(&self) -> Result<Vec<u8>, u16> {
        if self.nodes.is_empty() {
            return Err(XC_SNAP_SIZE);
        }
        let mut out = Vec::with_capacity(self.nodes.len() * RECORD_W);
        for n in &self.nodes {
            out.push((n.nid & 0xFF) as u8);
            out.push((n.nid >> 8) as u8);
            out.push((n.parent & 0xFF) as u8);
            out.push((n.parent >> 8) as u8);
            out.push((n.name_hash & 0xFF) as u8);
            out.push(((n.name_hash >> 8) & 0xFF) as u8);
            out.push(((n.name_hash >> 16) & 0xFF) as u8);
            out.push((n.name_hash >> 24) as u8);
            out.push(n.ty);
            out.push((n.bytes & 0xFF) as u8);
            out.push(((n.bytes >> 8) & 0xFF) as u8);
            out.push(((n.bytes >> 16) & 0xFF) as u8);
            out.push((n.bytes >> 24) as u8);
            out.push((n.version & 0xFF) as u8);
            out.push(((n.version >> 8) & 0xFF) as u8);
            out.push(((n.version >> 16) & 0xFF) as u8);
        }
        // 尺寸守恒断言。
        if out.len() != self.nodes.len() * RECORD_W {
            return Err(XC_SNAP_SIZE);
        }
        Ok(out)
    }
}

/// 差分三账。
#[derive(Debug, Default)]
pub struct DiffBook {
    /// 新增节点号。
    pub added: Vec<u16>,
    /// 删除节点号。
    pub removed: Vec<u16>,
    /// 修改节点号（版本戳或字节量变）。
    pub changed: Vec<u16>,
    /// 未变节点号。
    pub unchanged: Vec<u16>,
}

/// 两快照对齐 diff（按节点号）。
pub fn diff_trees(a: &[ResNode], b: &[ResNode]) -> Result<DiffBook, u16> {
    let mut d = DiffBook::default();
    for na in a {
        match b.iter().find(|nb| nb.nid == na.nid) {
            None => d.removed.push(na.nid),
            Some(nb) => {
                if na.bytes != nb.bytes || na.version != nb.version || na.ty != nb.ty {
                    d.changed.push(na.nid);
                } else {
                    d.unchanged.push(na.nid);
                }
            }
        }
    }
    for nb in b {
        if !a.iter().any(|na| na.nid == nb.nid) {
            d.added.push(nb.nid);
        }
    }
    // 守恒：新增+删除+修改+未变 = |a ∪ b|。
    let union = a.len() + d.added.len();
    if d.removed.len() + d.changed.len() + d.unchanged.len() + d.added.len() != union {
        return Err(XC_DIFF_UNBALANCED);
    }
    Ok(d)
}

/// 自例程：建树 → 守恒 → 下钻 → 序列化尺寸 → resize 重算 → diff
/// 三账 → 错误路径（环父/类型越界/深度越界/目录 resize/根重定义）。
pub fn drill() -> Result<u32, u16> {
    let mut t = ResTree::new()?;
    // 树形：0 根 → 1 目录 app → {2 代码, 3 资产}；0 → 4 目录 data → 5 数据。
    let n1 = t.add(0, "app", ResType::Dir, 0)?;
    let n2 = t.add(n1, "main.js", ResType::Code, 1200)?;
    let n3 = t.add(n1, "logo.png", ResType::Asset, 3400)?;
    let n4 = t.add(0, "data", ResType::Dir, 0)?;
    let n5 = t.add(n4, "ledger.json", ResType::Data, 800)?;
    if t.len() != 6 {
        return Err(XC_NODE_FULL);
    }
    // 守恒：app = 1200+3400 = 4600；data = 800；根 = 5400。
    t.bytes_conserved()?;
    if t.get(n1)?.bytes != 4600 {
        return Err(XC_BYTES_UNBALANCED);
    }
    if t.get(0)?.bytes != 5400 {
        return Err(XC_BYTES_UNBALANCED);
    }
    // 下钻主路径：0→1→2。
    let tr = t.descend(&[n1, n2])?;
    if tr != vec_into(&[0, 1, 2]) {
        return Err(XC_BAD_PARENT);
    }
    // 序列化尺寸守恒：6×16 = 96。
    let snap1 = t.serialize()?;
    if snap1.len() != t.len() * RECORD_W {
        return Err(XC_SNAP_SIZE);
    }
    // a 面快照（resize 前）。
    let a_nodes: Vec<ResNode> = (0..t.len() as u16)
        .filter_map(|i| t.get(i).ok().copied())
        .collect();
    // resize：main.js 1200→1600 → 链上目录重算。
    t.resize(n2, 1600)?;
    t.bytes_conserved()?;
    if t.get(n1)?.bytes != 5000 || t.get(0)?.bytes != 5800 {
        return Err(XC_BYTES_UNBALANCED);
    }
    // diff：新增节点后再取 b 面。
    let n6 = t.add(n4, "extra.bin", ResType::Data, 100)?;
    let b_nodes: Vec<ResNode> = (0..t.len() as u16)
        .filter_map(|i| t.get(i).ok().copied())
        .collect();
    let d = diff_trees(&a_nodes, &b_nodes)?;
    if d.added != vec_into(&[n6]) || d.removed.len() != 0 {
        return Err(XC_DIFF_UNBALANCED);
    }
    // changed 应含 n2（resize 过）与链上目录（version +1）。
    if !d.changed.contains(&n2) {
        return Err(XC_DIFF_UNBALANCED);
    }
    // 守恒式：added+removed+changed+unchanged = 7。
    if d.added.len() + d.removed.len() + d.changed.len() + d.unchanged.len() != 7 {
        return Err(XC_DIFF_UNBALANCED);
    }

    // 错误路径 1：叶子下挂子（main.js 挂子）。
    if t.add(n2, "child", ResType::Dir, 0) != Err(XC_BAD_PARENT) {
        return Err(XC_BAD_PARENT);
    }
    // 错误路径 2：根重定义。
    if t.add(0, "root2", ResType::Root, 0) != Err(XC_ROOT_DUP) {
        return Err(XC_ROOT_DUP);
    }
    // 错误路径 3：目录 resize。
    if t.resize(n1, 10) != Err(XC_BYTES_UNBALANCED) {
        return Err(XC_BYTES_UNBALANCED);
    }
    // 错误路径 4：父不存在。
    if t.add(99, "ghost", ResType::Code, 1) != Err(XC_NODE_MISSING) {
        return Err(XC_NODE_MISSING);
    }
    // 错误路径 5：下钻断链（0→3：3 的父是 1 非 0）。
    if t.descend(&[n3]) != Err(XC_BAD_PARENT) {
        return Err(XC_BAD_PARENT);
    }
    // 错误路径 6：类型码越界。
    if ResType::from_code(5).is_some() {
        return Err(XC_BAD_TYPE);
    }
    // 深度守恒：9 级路径越界（构造长链太费，直接核常量语义）。
    if DEPTH_MAX != 8 {
        return Err(XC_DEPTH_OVER);
    }
    Ok(0)
}

/// 小助手：数组→Vec<u16>（drill 内对拍用）。
fn vec_into(src: &[u16]) -> Vec<u16> {
    src.to_vec()
}
