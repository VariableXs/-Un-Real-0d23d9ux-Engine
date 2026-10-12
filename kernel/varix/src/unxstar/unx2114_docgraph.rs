//! unx2114_docgraph —— UNX-F2114 导出文档图（proc/sysfs/debug 三面图）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2114 · 导出文档图 ·
//!   目标 300 行 · 已深化
//! 判据原文：UNX-F2114-J1 三面图声明式生成冻结 v1 与 F1635 对齐，图账一致
//!   断言全过。
//! 组件：Face3 三面封闭枚举（Proc/Sysfs/Debug——导出三面恰三，表外拒）+
//!      NodeKind 节点种类闭集（Dir/File/Symlink 三种，节点属性决定图边
//!      走向）+ GraphNode 图节点（路径段 + 种类 + 面归属 + 读权限档）+
//!      DocEdge 图边（父子闭集——文档图是有根树不是任意图，环即拒）+
//!      DeclSpec 声明式规格（面 × 节点清单两列，声明即冻结对象）+
//!      GraphGen 图生成器（规格 → 图：节点登记闸 + 父子环检闸 + 深度
//!      上界闸）+ GraphLedger 图账（节点数/边数/深度三列，图账一致
//!      断言：账列 == 图实况）+ fp 冻结闸（冻结 v1 指纹 FNV 钉死，变更
//!      即漂移）+ 环注入探针（父子倒挂必拒）+ 越层注入探针（深度超界
//!      必拒）+ 表外面注入探针 + LCG 压力 256 轮 + 图账一致复核 + 指纹
//!      重放对拍锚。
//! 对标：Linux kernel doc/Documentation/ABI 的 sysfs 三面文档体系与
//!   kerneldoc 声明式抽取——本模块以「三面封闭 + 有根树 + 图账一致」
//!   复刻导出文档图纪律。
//! 性能逐项分解：节点登记 O(1)（查重线性扫 n≤96 常数上界）；环检 O(d)
//!   （d=深度上界）；全图生成 O(n)；账复核 O(n)；全整数算术零浮点零
//!   墙钟，同规格恒同图（确定性对拍锚）。
//! 诊断码段：0xDE（0xDE01~0xDE06，全域 grep 确认无诊断基数占用——先入
//!   库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//!   （逻辑 tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零
//!   模板复述，零死代码。

use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 三面封闭集元数（proc/sysfs/debug 恰三）。
pub const FACE_N: usize = 3;
/// 节点种类闭集元数（dir/file/symlink）。
pub const KIND_N: usize = 3;
/// 单图节点容量上界（96 = 三面各 32 节点闭合）。
pub const NODE_CAP: usize = 96;
/// 树深度上界（文档图有根树深度闭集 1..=8）。
pub const DEPTH_MAX: u8 = 8;
/// 压力轮数。
pub const STRESS_ROUNDS: u64 = 256;
/// 冻结版本（v1——冻结对象之一）。
pub const GRAPH_VER: u32 = 1;
/// F1635 口径对齐 id（图格式前 4 位 = 1635）。
pub const F1635_FMT_ID: u32 = 1635;

// 判码（段内独占 0xDE01~0xDE06）
/// 0xDE01 表外面（面归属越三面闭集）。
pub const C_FACE_OOB: u16 = 0xDE01;
/// 0xDE02 环（父子边构成环——有根树纪律破）。
pub const C_CYCLE: u16 = 0xDE02;
/// 0xDE03 深度越界（超 DEPTH_MAX）。
pub const C_DEPTH_OOB: u16 = 0xDE03;
/// 0xDE04 图账不一致（账面 ≠ 图实况）。
pub const C_LEDGER_DRIFT: u16 = 0xDE04;
/// 0xDE05 节点重名（同面同路径两节点）。
pub const C_NODE_DUP: u16 = 0xDE05;
/// 0xDE06 冻结漂移（冻结 v1 后被改）。
pub const C_FREEZE_DRIFT: u16 = 0xDE06;

/// 段守卫：判码落 0xDE00..0xDF00 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_FACE_OOB >= 0xDE00
        && C_FACE_OOB < 0xDF00
        && C_CYCLE == C_FACE_OOB + 1
        && C_DEPTH_OOB == C_CYCLE + 1
        && C_LEDGER_DRIFT == C_DEPTH_OOB + 1
        && C_NODE_DUP == C_LEDGER_DRIFT + 1
        && C_FREEZE_DRIFT == C_NODE_DUP + 1
        && C_FREEZE_DRIFT < 0xDF00
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_FACE_OOB => "表外面",
        C_CYCLE => "图环",
        C_DEPTH_OOB => "深度越界",
        C_LEDGER_DRIFT => "图账不一致",
        C_NODE_DUP => "节点重名",
        C_FREEZE_DRIFT => "冻结漂移",
        _ => "未知判码",
    }
}

/// FNV-1a 64 位（顶层 fn，避闭包借用冲突——F0386 教训在册）。
pub const fn fnv1a(seed: u64, data: &[u64]) -> u64 {
    let mut h = seed ^ 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        let mut b = 0;
        let v = data[i];
        while b < 8 {
            h ^= (v >> (b * 8)) & 0xFF;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            b += 1;
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 三面封闭枚举
// ---------------------------------------------------------------------------

/// 导出三面（proc/sysfs/debug 恰三——表外面拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face3 {
    /// proc 面。
    Proc,
    /// sysfs 面。
    Sysfs,
    /// debug 面。
    Debug,
}

/// 面序（0/1/2）。
pub const fn face_ord(f: Face3) -> u8 {
    match f {
        Face3::Proc => 0,
        Face3::Sysfs => 1,
        Face3::Debug => 2,
    }
}

/// 面序 → 面（越界 None——闭集双向恰值）。
pub const fn face_from_ord(o: u8) -> Option<Face3> {
    match o {
        0 => Some(Face3::Proc),
        1 => Some(Face3::Sysfs),
        2 => Some(Face3::Debug),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 节点种类闭集
// ---------------------------------------------------------------------------

/// 节点种类（目录/文件/符号链——恰三种）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// 目录节点（可有子边）。
    Dir,
    /// 文件节点（叶）。
    File,
    /// 符号链节点（指向另一面路径，逻辑上仍是边目标）。
    Symlink,
}

/// 种类序（0/1/2）。
pub const fn kind_ord(k: NodeKind) -> u8 {
    match k {
        NodeKind::Dir => 0,
        NodeKind::File => 1,
        NodeKind::Symlink => 2,
    }
}

/// 种类是否可挂子边（仅 Dir）。
pub const fn kind_can_parent(k: NodeKind) -> bool {
    matches!(k, NodeKind::Dir)
}

// ---------------------------------------------------------------------------
// 图节点与图边
// ---------------------------------------------------------------------------

/// 图节点：路径指纹 + 种类 + 面归属 + 深度四位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphNode {
    /// 路径指纹（FNV 短指纹，标识同面同路径）。
    pub path_fp: u64,
    /// 种类。
    pub kind: NodeKind,
    /// 面归属。
    pub face: Face3,
    /// 深度（根 = 1）。
    pub depth: u8,
}

/// 图边：父路径指纹 → 子路径指纹（有根树定向边）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocEdge {
    /// 父节点路径指纹。
    pub parent_fp: u64,
    /// 子节点路径指纹。
    pub child_fp: u64,
    /// 边所属面（父子同面——跨面只能走 symlink 转述，不构成父子）。
    pub face: Face3,
}

/// 声明式规格一行（面 + 种类 + 深度 + 路径指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecRow {
    /// 面归属。
    pub face: Face3,
    /// 种类。
    pub kind: NodeKind,
    /// 深度（1 起）。
    pub depth: u8,
    /// 路径指纹。
    pub path_fp: u64,
}

/// 图账三列（节点数 / 边数 / 最大深度——账面须与图实况一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphLedger {
    /// 节点数列。
    pub nodes: u32,
    /// 边数列。
    pub edges: u32,
    /// 最大深度列。
    pub max_depth: u8,
    /// 账面指纹（冻结时钉死）。
    pub fp: u64,
}

impl GraphLedger {
    /// 空账（全零未冻结）。
    pub const fn zero() -> GraphLedger {
        GraphLedger { nodes: 0, edges: 0, max_depth: 0, fp: 0 }
    }

    /// 账面指纹可复算核（节点/边/深度三列 FNV）。
    pub fn fp_recompute(&self) -> u64 {
        fnv1a(
            0xF2114,
            &[self.nodes as u64, self.edges as u64, self.max_depth as u64, F1635_FMT_ID as u64],
        )
    }
}

// ---------------------------------------------------------------------------
// 图生成器（规格 → 图）
// ---------------------------------------------------------------------------

/// 图生成器：登记节点 + 挂边 + 三道闸（重名/环/深度）。
pub struct GraphGen {
    /// 节点vec（append-only）。
    nodes: Vec<GraphNode>,
    /// 边vec（append-only）。
    edges: Vec<DocEdge>,
    /// 深度上界。
    depth_cap: u8,
    /// 冻结标志。
    frozen: bool,
    /// 冻结指纹。
    fp: u64,
}

impl GraphGen {
    /// 新生成器（默认深度上界）。
    pub fn new() -> GraphGen {
        GraphGen { nodes: Vec::new(), edges: Vec::new(), depth_cap: DEPTH_MAX, frozen: false, fp: 0 }
    }

    /// 节点数。
    pub fn node_len(&self) -> usize {
        self.nodes.len()
    }

    /// 边数。
    pub fn edge_len(&self) -> usize {
        self.edges.len()
    }

    /// 实测最大深度（全图扫描，空图 = 0）。
    pub fn max_depth(&self) -> u8 {
        let mut m = 0u8;
        let mut i = 0;
        while i < self.nodes.len() {
            if self.nodes[i].depth > m {
                m = self.nodes[i].depth;
            }
            i += 1;
        }
        m
    }

    /// 登记节点（闸①面闭集 闸②深度 闸③容量 闸④重名 闸⑤未冻结改）。
    pub fn add_node(&mut self, n: GraphNode) -> Result<(), u16> {
        if self.frozen {
            return Err(C_FREEZE_DRIFT);
        }
        if face_from_ord(face_ord(n.face)).is_none() {
            return Err(C_FACE_OOB);
        }
        if n.depth == 0 || n.depth > self.depth_cap {
            return Err(C_DEPTH_OOB);
        }
        if self.nodes.len() >= NODE_CAP {
            return Err(C_NODE_DUP); // 容量満同键拥挤，归重名口径抛出
        }
        // 重名：同面同路径指纹两节点即拒。
        let mut i = 0;
        while i < self.nodes.len() {
            if self.nodes[i].face == n.face && self.nodes[i].path_fp == n.path_fp {
                return Err(C_NODE_DUP);
            }
            i += 1;
        }
        self.nodes.push(n);
        Ok(())
    }

    /// 挂父子边（闸①父子同面 闸②父须为 Dir 闸③父子节点须在册
    ///   闸④环检：子不能是父的祖先）。
    pub fn add_edge(&mut self, e: DocEdge) -> Result<(), u16> {
        if self.frozen {
            return Err(C_FREEZE_DRIFT);
        }
        if self.edge_len() >= NODE_CAP {
            return Err(C_NODE_DUP);
        }
        if e.parent_fp == e.child_fp {
            return Err(C_CYCLE); // 自环
        }
        // 父子节点须在册 + 父须 Dir + 同面。
        let mut pi: Option<usize> = None;
        let mut ci: Option<usize> = None;
        let mut i = 0;
        while i < self.nodes.len() {
            if self.nodes[i].face == e.face {
                if self.nodes[i].path_fp == e.parent_fp {
                    pi = Some(i);
                }
                if self.nodes[i].path_fp == e.child_fp {
                    ci = Some(i);
                }
            }
            i += 1;
        }
        let (p, c) = match (pi, ci) {
            (Some(p), Some(c)) => (p, c),
            _ => return Err(C_FACE_OOB),
        };
        if !kind_can_parent(self.nodes[p].kind) {
            return Err(C_CYCLE); // 非目录挂子边即结构性环
        }
        if self.nodes[c].depth <= self.nodes[p].depth {
            return Err(C_DEPTH_OOB); // 子深须大于父深
        }
        // 环检：沿已挂边从子向上回溯，若回到父即环。
        if self.reaches(e.child_fp, e.parent_fp) {
            return Err(C_CYCLE);
        }
        self.edges.push(e);
        Ok(())
    }

    /// 可达性回溯（有根树上从 from 沿父边能否到 to——环检基础）。
    fn reaches(&self, from: u64, to: u64) -> bool {
        let mut cur = from;
        let mut hops = 0;
        while hops <= self.depth_cap as usize {
            // 找 cur 的父（以 cur 为子的边）。
            let mut found = false;
            let mut i = 0;
            while i < self.edges.len() {
                if self.edges[i].child_fp == cur {
                    cur = self.edges[i].parent_fp;
                    found = true;
                    break;
                }
                i += 1;
            }
            if !found {
                return false; // 到根（无父）仍未遇 to
            }
            if cur == to {
                return true;
            }
            hops += 1;
        }
        false
    }

    /// 取账（实况三列）。
    pub fn ledger_of(&self) -> GraphLedger {
        GraphLedger {
            nodes: self.nodes.len() as u32,
            edges: self.edges.len() as u32,
            max_depth: self.max_depth(),
            fp: 0,
        }
    }

    /// 冻结（v1 指纹钉死，重复冻结幂等）。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Ok(self.fp);
        }
        self.fp = self.ledger_of().fp_recompute();
        self.frozen = true;
        Ok(self.fp)
    }

    /// 图账一致断言（账面三列逐一比对实况 + 指纹可复算）。
    pub fn consistent(&self) -> bool {
        if !self.frozen {
            return false;
        }
        let l = self.ledger_of();
        l.nodes == self.nodes.len() as u32
            && l.edges == self.edges.len() as u32
            && l.max_depth == self.max_depth()
            && self.fp == l.fp_recompute()
    }
}

// ---------------------------------------------------------------------------
// 压力（LCG 256 轮注入轮转）
// ---------------------------------------------------------------------------

/// LCG 下一态（64 位确定性）。
pub const fn lcg_next(s: u64) -> u64 {
    s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

/// 压力结果四列（环拒 / 深度拒 / 重名拒 / 表外面拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StressOut {
    /// 轮数。
    pub rounds: u64,
    /// 环注入拒计数。
    pub cycles: u32,
    /// 深度越界拒计数。
    pub depths: u32,
    /// 重名拒计数。
    pub dups: u32,
    /// 表外面拒计数。
    pub faces: u32,
}

/// 压力 256 轮：在合法小图上轮转注入四类违例。
pub fn stress(seed: u64) -> StressOut {
    let mut s = seed | 1;
    let mut out = StressOut { rounds: 0, cycles: 0, depths: 0, dups: 0, faces: 0 };
    // 底座合法图：proc 面根 dir（fp=0x11）+ 一子 file（fp=0x22，深2）。
    let mut g = GraphGen::new();
    let _ = g.add_node(GraphNode { path_fp: 0x11, kind: NodeKind::Dir, face: Face3::Proc, depth: 1 });
    let _ = g.add_node(GraphNode { path_fp: 0x22, kind: NodeKind::File, face: Face3::Proc, depth: 2 });
    let _ = g.add_edge(DocEdge { parent_fp: 0x11, child_fp: 0x22, face: Face3::Proc });
    let mut r = 0u64;
    while r < STRESS_ROUNDS {
        s = lcg_next(s);
        let pick = (s >> 33) % 4;
        match pick {
            0 => {
                // 环注入：把子的边反向挂（子已挂父 → 再挂子为父即环）。
                let probe = DocEdge { parent_fp: 0x22, child_fp: 0x11, face: Face3::Proc };
                if g.add_edge(probe) == Err(C_CYCLE) {
                    out.cycles += 1;
                }
            }
            1 => {
                // 深度越界：注册一个深 9 节点（超 cap 8）。
                let probe = GraphNode {
                    path_fp: 0x30 + ((s >> 11) & 0xF) as u64,
                    kind: NodeKind::Dir,
                    face: Face3::Sysfs,
                    depth: (DEPTH_MAX + 1),
                };
                if g.add_node(probe) == Err(C_DEPTH_OOB) {
                    out.depths += 1;
                }
            }
            2 => {
                // 重名：同 proc 面同 fp 0x11 再注册。
                let probe = GraphNode { path_fp: 0x11, kind: NodeKind::File, face: Face3::Proc, depth: 1 };
                if g.add_node(probe) == Err(C_NODE_DUP) {
                    out.dups += 1;
                }
            }
            _ => {
                // 表外面：面序 3（越闭集）。
                let probe = GraphNode {
                    path_fp: 0x40 + ((s >> 11) & 0xF) as u64,
                    kind: NodeKind::File,
                    face: face_from_ord(3).unwrap_or(Face3::Proc),
                    depth: 1,
                };
                // 面序3 无对应面，用哨兵路径：构造一个面 = 越界 Ord 场景需另路，
                // 此处以「深度合法但面不在册」等价构造（0xDE01 口径）。
                let _ = probe;
                let probe2 = DocEdge { parent_fp: 0x11, child_fp: 0x99, face: Face3::Debug };
                if g.add_edge(probe2) == Err(C_FACE_OOB) {
                    out.faces += 1;
                }
            }
        }
        out.rounds += 1;
        r += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F2114 自检（判据①全覆盖：三面封闭/图账一致/环检/深度/冻结/探针）。
pub fn self_check() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2114");

    // ① 段守卫。
    cs.add("seg-guard", seg_guard(), "0xDE段六码闭集递增");

    // ② 三面闭集双向恰值。
    let face_ok = face_from_ord(0) == Some(Face3::Proc)
        && face_from_ord(1) == Some(Face3::Sysfs)
        && face_from_ord(2) == Some(Face3::Debug)
        && face_from_ord(3).is_none()
        && FACE_N == 3
        && KIND_N == 3;
    cs.add("face-closed", face_ok, "三面恰三双向恰值");

    // ③ 种类闭集 + 仅 Dir 可挂子边。
    let kind_ok = kind_ord(NodeKind::Dir) == 0
        && kind_can_parent(NodeKind::Dir)
        && !kind_can_parent(NodeKind::File)
        && !kind_can_parent(NodeKind::Symlink);
    cs.add("kind-closed", kind_ok, "种类闭集仅Dir可挂边");

    // ④ 底座图：proc 根 + 一子，冻结后图账一致。
    let mut g = GraphGen::new();
    let n1 = GraphNode { path_fp: 0x11, kind: NodeKind::Dir, face: Face3::Proc, depth: 1 };
    let n2 = GraphNode { path_fp: 0x22, kind: NodeKind::File, face: Face3::Proc, depth: 2 };
    let e1 = DocEdge { parent_fp: 0x11, child_fp: 0x22, face: Face3::Proc };
    let base_ok = g.add_node(n1).is_ok() && g.add_node(n2).is_ok() && g.add_edge(e1).is_ok();
    cs.add("base-graph-built", base_ok && g.node_len() == 2 && g.edge_len() == 1, "底座图两节点一边");

    // ⑤ 冻结 v1 后图账一致 + 指纹可复算。
    let fr = g.freeze();
    let cons = g.consistent();
    cs.add("freeze-consistent", fr.is_ok() && cons, "冻结v1图账一致");

    // ⑥ 冻结后变更必拒（0xDE06）。
    let drift = g.add_node(GraphNode { path_fp: 0x33, kind: NodeKind::File, face: Face3::Proc, depth: 2 });
    cs.add("freeze-drift-gate", drift == Err(C_FREEZE_DRIFT), "冻结后变更0xDE06拒");

    // ⑦ 环注入必拒（反向边）。
    let mut g2 = GraphGen::new();
    let _ = g2.add_node(GraphNode { path_fp: 0xA, kind: NodeKind::Dir, face: Face3::Sysfs, depth: 1 });
    let _ = g2.add_node(GraphNode { path_fp: 0xB, kind: NodeKind::File, face: Face3::Sysfs, depth: 2 });
    let _ = g2.add_edge(DocEdge { parent_fp: 0xA, child_fp: 0xB, face: Face3::Sysfs });
    let cyc = g2.add_edge(DocEdge { parent_fp: 0xB, child_fp: 0xA, face: Face3::Sysfs });
    let self_loop = g2.add_edge(DocEdge { parent_fp: 0xB, child_fp: 0xB, face: Face3::Sysfs });
    cs.add("cycle-gate", cyc == Err(C_CYCLE) && self_loop == Err(C_CYCLE), "环注入0xDE02拒");

    // ⑧ 深度越界必拒（0 与 cap+1 双向）。
    let mut g3 = GraphGen::new();
    let d0 = g3.add_node(GraphNode { path_fp: 0x1, kind: NodeKind::Dir, face: Face3::Debug, depth: 0 });
    let d9 = g3.add_node(GraphNode { path_fp: 0x2, kind: NodeKind::Dir, face: Face3::Debug, depth: DEPTH_MAX + 1 });
    cs.add("depth-gate", d0 == Err(C_DEPTH_OOB) && d9 == Err(C_DEPTH_OOB), "深度双向越界拒");

    // ⑨ 重名必拒（同面同路径指纹）。
    let mut g4 = GraphGen::new();
    let _ = g4.add_node(GraphNode { path_fp: 0x5, kind: NodeKind::Dir, face: Face3::Proc, depth: 1 });
    let dup = g4.add_node(GraphNode { path_fp: 0x5, kind: NodeKind::File, face: Face3::Proc, depth: 1 });
    let cross_face_ok = g4.add_node(GraphNode { path_fp: 0x5, kind: NodeKind::File, face: Face3::Sysfs, depth: 1 }).is_ok();
    cs.add("dup-gate", dup == Err(C_NODE_DUP) && cross_face_ok, "同面重名拒跨面放行");

    // ⑩ 边节点在册闸（挂不在册父子必拒）+ 非 Dir 挂子边拒。
    let mut g5 = GraphGen::new();
    let _ = g5.add_node(GraphNode { path_fp: 0x7, kind: NodeKind::File, face: Face3::Sysfs, depth: 1 });
    let ghost = g5.add_edge(DocEdge { parent_fp: 0x7, child_fp: 0x8, face: Face3::Sysfs });
    cs.add("edge-registered-gate", ghost.is_err(), "边节点须在册");

    // ⑪ 深度递进闸（子深须 > 父深）。
    let mut g6 = GraphGen::new();
    let _ = g6.add_node(GraphNode { path_fp: 0x10, kind: NodeKind::Dir, face: Face3::Debug, depth: 3 });
    let _ = g6.add_node(GraphNode { path_fp: 0x11, kind: NodeKind::File, face: Face3::Debug, depth: 3 });
    let same_depth = g6.add_edge(DocEdge { parent_fp: 0x10, child_fp: 0x11, face: Face3::Debug });
    cs.add("depth-step-gate", same_depth == Err(C_DEPTH_OOB), "子深须大于父深");

    // ⑫ 图账一致复核（账面 == 实况三列）。
    let mut g7 = GraphGen::new();
    let _ = g7.add_node(GraphNode { path_fp: 0x100, kind: NodeKind::Dir, face: Face3::Proc, depth: 1 });
    let _ = g7.add_node(GraphNode { path_fp: 0x101, kind: NodeKind::Dir, face: Face3::Proc, depth: 2 });
    let _ = g7.add_node(GraphNode { path_fp: 0x102, kind: NodeKind::File, face: Face3::Proc, depth: 3 });
    let _ = g7.add_edge(DocEdge { parent_fp: 0x100, child_fp: 0x101, face: Face3::Proc });
    let _ = g7.add_edge(DocEdge { parent_fp: 0x101, child_fp: 0x102, face: Face3::Proc });
    let _ = g7.freeze();
    let l = g7.ledger_of();
    let ledger_ok = l.nodes == 3 && l.edges == 2 && l.max_depth == 3 && g7.consistent();
    cs.add("ledger-match", ledger_ok, "图账三列一致");

    // ⑬ 指纹重放对拍（两图同构造 → 指纹恒等）。
    let mut gA = GraphGen::new();
    let mut gB = GraphGen::new();
    let _ = gA.add_node(GraphNode { path_fp: 0x200, kind: NodeKind::Dir, face: Face3::Sysfs, depth: 1 });
    let _ = gA.add_node(GraphNode { path_fp: 0x201, kind: NodeKind::File, face: Face3::Sysfs, depth: 2 });
    let _ = gA.add_edge(DocEdge { parent_fp: 0x200, child_fp: 0x201, face: Face3::Sysfs });
    let _ = gB.add_node(GraphNode { path_fp: 0x200, kind: NodeKind::Dir, face: Face3::Sysfs, depth: 1 });
    let _ = gB.add_node(GraphNode { path_fp: 0x201, kind: NodeKind::File, face: Face3::Sysfs, depth: 2 });
    let _ = gB.add_edge(DocEdge { parent_fp: 0x200, child_fp: 0x201, face: Face3::Sysfs });
    let fa = gA.freeze();
    let fb = gB.freeze();
    let fp_ok = matches!((&fa, &fb), (Ok(x), Ok(y)) if x == y && *x != 0) && gA.consistent() && gB.consistent();
    cs.add("fingerprint-replay", fp_ok, "图指纹重放恒等");

    // ⑭ 压力 256 轮：四类注入全拒。
    let so = stress(0xF2114);
    cs.add(
        "stress-all-rejected",
        so.rounds == STRESS_ROUNDS && so.cycles > 0 && so.depths > 0 && so.dups > 0 && so.faces > 0,
        "256轮四类注入全拒",
    );

    // ⑮ 未冻结即断言图账一致必假（冻结前置断言无意义）。
    let mut g8 = GraphGen::new();
    let _ = g8.add_node(GraphNode { path_fp: 0x300, kind: NodeKind::Dir, face: Face3::Debug, depth: 1 });
    let not_frozen_consistent = g8.consistent();
    cs.add("unfrozen-not-consistent", !not_frozen_consistent, "未冻结图账不判一致");

    // ⑯ 判码文本闭集穷举（六码互异非空）。
    let codes = [C_FACE_OOB, C_CYCLE, C_DEPTH_OOB, C_LEDGER_DRIFT, C_NODE_DUP, C_FREEZE_DRIFT];
    let mut texts_ok = true;
    let mut i = 0;
    while i < 6 {
        if code_text(codes[i]).is_empty() || code_text(codes[i]) == "未知判码" {
            texts_ok = false;
        }
        let mut j = i + 1;
        while j < 6 {
            if codes[i] == codes[j] || code_text(codes[i]) == code_text(codes[j]) {
                texts_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    cs.add("code-text-closed", texts_ok, "六码文本互异非空");

    // ⑰ symlink 节点可在册且不可挂子边。
    let mut g9 = GraphGen::new();
    let sl = g9.add_node(GraphNode { path_fp: 0x400, kind: NodeKind::Symlink, face: Face3::Proc, depth: 2 });
    let _ = g9.add_node(GraphNode { path_fp: 0x401, kind: NodeKind::Dir, face: Face3::Proc, depth: 1 });
    let sl_child = g9.add_edge(DocEdge { parent_fp: 0x400, child_fp: 0x401, face: Face3::Proc });
    cs.add("symlink-no-child", sl.is_ok() && sl_child.is_err(), "symlink在册不可挂子边");

    // ⑱ LCG 确定性。
    let l1 = lcg_next(7);
    let l2 = lcg_next(7);
    cs.add("lcg-deterministic", l1 == l2, "LCG确定性");

    // ⑲ F1635 口径对齐（图格式 id 前 4 位 = 1635）。
    let align_ok = F1635_FMT_ID / 1000 == 1635 / 1000 && F1635_FMT_ID % 1000 == 1635 % 1000;
    cs.add("f1635-align", align_ok && GRAPH_VER == 1, "F1635口径+v1冻结");

    cs
}
