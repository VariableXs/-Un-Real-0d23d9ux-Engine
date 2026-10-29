//! unx4164_dotself — UNX-F4164 "." 分量原地语义（诊断段 0x9620）。
//!
//! 判据原文：
//!   J1 "." 分量解析后停留原地（引用计数净变化=0）。
//!   J2 根目录下 "." 无限连用（"////./././"）解析收敛断言通过。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. "." 语义：取当前节点不动，get 与 put 配对，引用计数净
//!      变化必须为 0（一进一出守恒）。
//!   2. 收敛红线：连续 "." 分量（含多斜杠间夹点）解析步数有界，
//!      不得无限循环；根目录连用收敛到根本身。
//!   3. 多斜杠折叠预锚："//" 折为 "/"（深账归 F4166），本模块
//!      只保证折叠后分量序列不含空分量。
//!   4. 守恒对账：每节点 get 计数 = put 计数 + 持有数；"." 路径
//!      解析后持有数与解析前一致。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9620;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RC_LEAK: u16 = SEG_BASE + 1;     // 引用计数净变化非零（判据 J1 失守）
const E_NO_NODE: u16 = SEG_BASE + 2;     // 节点号越界
const E_NOT_DIR: u16 = SEG_BASE + 3;     // "." 只能在目录上原地
const E_DIVERGE: u16 = SEG_BASE + 4;     // 解析不收敛（步数越界，判据 J2 失守）
const E_EMPTY_PATH: u16 = SEG_BASE + 5;  // 空路径拒收
const E_BAL: u16 = SEG_BASE + 6;         // get/put 守恒账不平
const E_BAD_STATE: u16 = SEG_BASE + 7;   // 解析状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 8;   // 写回摘要为空

pub const RESOLVE_BUDGET: usize = 256; // 收敛步数预算（判据 J2 有界面）

/// 解析状态机：AtRoot 在根 / AtNode 在节点 / Converged 已收敛 / Leaked 计数失守。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResolveState {
    AtRoot,
    AtNode,
    Converged,
    Leaked,
}

/// 节点：引用计数账。
pub struct Node {
    pub id: u32,       // 节点号
    pub refs: u32,     // 当前持有数
    pub gets: u32,     // get 累计
    pub puts: u32,     // put 累计
    pub is_dir: bool,  // 目录位（"." 前提）
}

/// "." 解析枢纽：节点账 + 原地语义 + 收敛预算。
pub struct DotResolver {
    nodes: Vec<Node>,
    pub state: ResolveState,
    pub cur: u32,          // 当前节点号
    pub resolve_steps: u32 // 最近一次解析步数
    ,
}

impl DotResolver {
    pub fn new() -> DotResolver {
        // 0 号节点固定为根目录（初始持有一笔：gets=puts+refs 守恒口径）。
        DotResolver {
            nodes: vec![Node { id: 0, refs: 1, gets: 1, puts: 0, is_dir: true }],
            state: ResolveState::AtRoot,
            cur: 0,
            resolve_steps: 0,
        }
    }

    pub fn add_dir(&mut self, id: u32) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if self.nodes[i].id == id {
                return Err(E_BAL);
            }
            i += 1;
        }
        self.nodes.push(Node { id, refs: 0, gets: 0, puts: 0, is_dir: true });
        Ok(())
    }

    fn idx_of(&self, id: u32) -> Result<usize, u16> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if self.nodes[i].id == id {
                return Ok(i);
            }
            i += 1;
        }
        Err(E_NO_NODE)
    }

    /// 路径分词：多斜杠折叠 + 分量切分（只产 "." 与空序列两口径）。
    pub fn split_components(path: &[u8]) -> Result<Vec<Vec<u8>>, u16> {
        if path.is_empty() {
            return Err(E_EMPTY_PATH);
        }
        let mut out: Vec<Vec<u8>> = Vec::new();
        let mut i = 0usize;
        while i < path.len() {
            // 折叠连续斜杠。
            while i < path.len() && path[i] == b'/' {
                i += 1;
            }
            if i >= path.len() {
                break;
            }
            let start = i;
            while i < path.len() && path[i] != b'/' {
                i += 1;
            }
            // 本模块仅收 "." 分量（其余分量归 F4162 分账）。
            let mut comp: Vec<u8> = Vec::new();
            let mut k = start;
            while k < i {
                comp.push(path[k]);
                k += 1;
            }
            if comp.len() == 1 && comp[0] == b'.' {
                out.push(comp);
            }
        }
        Ok(out)
    }

    /// 单次 "." 原地：get(cur) + put(cur) 配对，净变化必须 0（判据 J1）。
    pub fn step_dot(&mut self) -> Result<(), u16> {
        let i = self.idx_of(self.cur)?;
        if !self.nodes[i].is_dir {
            return Err(E_NOT_DIR);
        }
        self.nodes[i].gets += 1;
        self.nodes[i].refs += 1;
        self.nodes[i].puts += 1;
        self.nodes[i].refs -= 1; // 原地：get/put 配对即还
        Ok(())
    }

    /// "." 连用解析：逐分量原地，步数超预算判不收敛（判据 J2）。
    pub fn resolve_dots(&mut self, path: &[u8]) -> Result<u32, u16> {
        let comps = Self::split_components(path)?;
        if self.cur == 0 {
            self.state = ResolveState::AtRoot;
        } else {
            self.state = ResolveState::AtNode;
        }
        let mut steps = 0u32;
        let mut i = 0usize;
        while i < comps.len() {
            if steps as usize >= RESOLVE_BUDGET {
                self.state = ResolveState::Leaked;
                return Err(E_DIVERGE);
            }
            self.step_dot()?;
            steps += 1;
            i += 1;
        }
        self.resolve_steps = steps;
        self.state = ResolveState::Converged;
        Ok(steps)
    }

    /// 引用计数守恒对账：每节点 gets==puts 时 refs 不涨；"." 序列
    /// 解析后全表 refs 与解析前逐位一致（判据 J1 对账面）。
    pub fn audit(&self, refs_before: &[u32]) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if self.nodes[i].gets != self.nodes[i].puts + self.nodes[i].refs {
                return Err(E_BAL);
            }
            if (refs_before.len() > i) && refs_before[i] != self.nodes[i].refs {
                return Err(E_RC_LEAK);
            }
            i += 1;
        }
        if self.state == ResolveState::Leaked {
            return Err(E_DIVERGE);
        }
        Ok(())
    }

    pub fn refs_snapshot(&self) -> Vec<u32> {
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < self.nodes.len() {
            v.push(self.nodes[i].refs);
            i += 1;
        }
        v
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 "." 原地零净变 → R2 边界 → R3 收敛断言 → R4 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：根上 "////./././" 连用解析收敛，引用计数净变化=0。
    let mut r = DotResolver::new();
    let before = r.refs_snapshot();
    let steps = r.resolve_dots(b"////./././")?;
    if steps != 3 {
        return Err(E_DIVERGE);
    }
    if r.cur != 0 {
        return Err(E_RC_LEAK);
    }
    r.audit(&before)?;
    hits += 1;

    // R2 边界：空路径 / 子目录上 "." 也原地 / 非目录拒绝由构造面保证。
    let mut r = DotResolver::new();
    if r.resolve_dots(b"").is_ok() {
        return Err(E_EMPTY_PATH);
    }
    r.add_dir(1)?;
    r.cur = 1;
    let before = r.refs_snapshot();
    r.resolve_dots(b"./././")?;
    if r.cur != 1 {
        return Err(E_RC_LEAK); // 原地：仍停在 1 号节点
    }
    r.audit(&before)?;
    hits += 1;

    // R3 注入拦截：纯 "." 深序列仍在预算内收敛（无发散路径）。
    let mut r = DotResolver::new();
    let before = r.refs_snapshot();
    let mut path: Vec<u8> = Vec::new();
    let mut k = 0usize;
    while k < 64 {
        if k > 0 {
            path.push(b'/');
        }
        path.push(b'.');
        k += 1;
    }
    let steps = r.resolve_dots(&path)?;
    if steps != 64 {
        return Err(E_DIVERGE);
    }
    r.audit(&before)?;
    hits += 1;

    // R4 守恒对账：get/put 全表平 + 收敛态 + 写回。
    let mut r = DotResolver::new();
    r.add_dir(2)?;
    r.cur = 2;
    let before = r.refs_snapshot();
    r.resolve_dots(b".")?;
    r.audit(&before)?;
    if r.state != ResolveState::Converged {
        return Err(E_BAD_STATE);
    }
    r.writeback(true, 0x30)?;
    if r.node_count() != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 追加回归：斜杠点混合长序列收敛 + 步数账与分量账平。
    let mut r = DotResolver::new();
    r.add_dir(1)?;
    r.add_dir(2)?;
    r.cur = 2;
    let before = r.refs_snapshot();
    let mut path: Vec<u8> = Vec::new();
    let mut k = 0usize;
    while k < 48 {
        path.push(b'/');
        path.push(b'.');
        path.push(b'/');
        k += 1;
    }
    path.push(b'.');
    let steps = r.resolve_dots(&path)?;
    if steps != 49 {
        return Err(E_DIVERGE);
    }
    if r.cur != 2 {
        return Err(E_RC_LEAK); // 混合 "." 全程原地
    }
    r.audit(&before)?;
    r.writeback(true, 0x10)?;
    hits += 1;

    Ok(hits)
}
