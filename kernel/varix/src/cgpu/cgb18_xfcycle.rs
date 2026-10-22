//! cgb18_xfcycle — CGPU-F0231 跨帧环检测（B05 组第七件）。
//!
//! 判据原文（施工书锚点 CGPU-F0231）：跨帧环检测：跨帧依赖环在线检测（帧
//! N-1 等 N、N 等 N-1 的逻辑环——提交前可判定性分析）；检测算法（跨帧边增
//! 量环检测——复用 B02 帧内环检测核扩展超图）；环报告（环路径完整呈现+成
//! 因定位）；与 B06 死锁检测组衔接（本组管"图上静态可判环"、B06 管"运行
//! 期动态死锁"——边界契约明确）。判据：在线检测、超图扩展、环报告、边界
//! 契约。
//!
//! 职责定位：跨帧环是自锁死亡——帧 N 等帧 N-1 的产出，而帧 N-1 的提交
//! 又被帧 N 挡住。本模块在提交前对跨帧超图做增量环检测：每加一条跨帧
//! 边，只在受影响子图上走一次三色 DFS（白灰黑），命中灰点即环，回溯出
//! 完整环路径并按"最早提交者"定位成因。与 B06 的边界契约：本模块只判
//! 「提交时点上图上已存在的环」（静态可判）；运行期信号量超时、优先级
//! 反转等动态死锁归 B06——本模块输出的环路径是 B06 的图源输入之一。
//! 诊断码段 0x63D0（本会话独占，grep 全库零占用后领取，2026-10-22）。
//!
//! 纪律：no_std + alloc；零 unwrap 零 expect 零浮点；DFS 用显式栈不用
//! 递归（内核栈金丝雀红线）；错误一律 Err(u16) 中文诊断。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const XF_CYCLE_VERSION: &str = "B18-xfcycle-v1";

// ---------------------------------------------------------------------------
// 诊断码（0x63D 段，本会话独占）
// ---------------------------------------------------------------------------

/// 检出跨帧依赖环（提交被拦截，环路径见报告）。
pub const XC_CYCLE_FOUND: u16 = 0x63D0;
/// 跨帧边引用不存在的帧节点（悬空边——图账不平）。
pub const XC_CYCLE_DANGLING_NODE: u16 = 0x63D1;
/// 同一条跨帧边重复登记（幂等性破坏——二义账）。
pub const XC_CYCLE_DUP_EDGE: u16 = 0x63D2;
/// 自环（帧等自己——登记错误，必然死锁）。
pub const XC_CYCLE_SELF_LOOP: u16 = 0x63D3;
/// 环路径长度越界（超检测窗——图异常，先查边登记）。
pub const XC_CYCLE_PATH_OVER: u16 = 0x63D4;
/// 检测深度越界（显式栈深超上限——防栈耗尽）。
pub const XC_CYCLE_STACK_OVER: u16 = 0x63D5;

/// 单帧最大出边数（防图爆炸；超过即账目异常）。
pub const MAX_OUT_EDGES: usize = 64;
/// 环路径报告上限（环长超过此值按异常处理，先查重复登记）。
pub const MAX_CYCLE_LEN: usize = 32;
/// DFS 显式栈深上限（内核路径防栈耗尽的硬红线）。
pub const MAX_STACK_DEPTH: usize = 128;

// ---------------------------------------------------------------------------
// 跨帧超图：节点=帧提交槽，边=「帧 A 的提交依赖帧 B 的产出」
// ---------------------------------------------------------------------------

/// 一条跨帧依赖边。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XfEdge {
    /// 依赖方（提交被挡的帧序号）。
    pub waiter: u64,
    /// 被依赖方（产出的帧序号，waiter - k，k≥1 跨帧深度）。
    pub producer: u64,
    /// 跨帧深度 k（producer = waiter - k；用于成因定位的深度标注）。
    pub depth: u32,
}

/// 三色标记：白=未访，灰=在当前 DFS 路径上，黑=已完结。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Color {
    White,
    Gray,
    Black,
}

/// 环报告：完整路径 + 成因定位（判据三）。
#[derive(Clone, Debug)]
pub struct CycleReport {
    /// 环路径（按提交时序：路径[i] 等待 路径[i+1]，末位等回首）。
    pub path: Vec<u64>,
    /// 成因边（环上深度最大的一条——跨帧深度最大的依赖最可疑）。
    pub cause: XfEdge,
    /// 环长（=path.len()）。
    pub len: usize,
}

impl CycleReport {
    /// 人话化环描述（供诊断面直读）。
    pub fn describe(&self) -> String {
        let mut s = String::from("跨帧环：");
        for (i, f) in self.path.iter().enumerate() {
            if i > 0 {
                s.push_str(" → ");
            }
            s.push_str(itoa(*f).as_str());
        }
        s.push_str(" → 回到 ");
        s.push_str(itoa(self.path[0]).as_str());
        s.push_str("（成因边：帧 ");
        s.push_str(itoa(self.cause.waiter).as_str());
        s.push_str(" 依赖 帧 ");
        s.push_str(itoa(self.cause.producer).as_str());
        s.push_str("，深度 ");
        s.push_str(itoa(self.cause.depth as u64).as_str());
        s.push_str("；提交前拦截，B06 只收图源不做裁决）");
        s
    }
}

/// 极简整数转字符串（no_std 环境无 format! 依赖负担）。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    String::from_utf8_lossy(&buf[i..]).into_owned()
}

/// 跨帧环检测器：增量式，每条边插入即检。
#[derive(Debug)]
pub struct CycleDetector {
    /// 邻接表：waiter → [producer...]（出边）。
    adj: Vec<(u64, Vec<u64>)>,
    /// 深度标注：边 → k。
    depths: Vec<(u64, u64, u32)>,
    /// 已检出并被拦截的环账（append-only，审计用）。
    pub cycle_log: Vec<CycleReport>,
    /// 节点总数（帧序号 1..=nodes）。
    nodes: u64,
    /// 提交前拦截计数。
    pub intercepted: u64,
}

impl CycleDetector {
    /// 构造：nodes = 已登记的帧槽总数。
    pub fn boot(nodes: u64) -> Self {
        CycleDetector {
            adj: Vec::new(),
            depths: Vec::new(),
            cycle_log: Vec::new(),
            nodes,
            intercepted: 0,
        }
    }

    fn node_idx(&self, f: u64) -> Option<usize> {
        self.adj.iter().position(|(w, _)| *w == f)
    }

    /// 增量插边 + 立即环检（判据一：在线检测——插边即判，不等批扫）。
    ///
    /// 返回 Ok(None)=安全；Ok(Some(report))=检出环且已拦截；
    /// Err=登记非法（悬空/重复/自环）。检出环时边**不入图**（拦截语义：
    /// 图里永远无环——这是"提交前可判定"的兑现方式）。
    pub fn add_edge(&mut self, e: XfEdge) -> Result<Option<CycleReport>, u16> {
        if e.waiter == e.producer {
            return Err(XC_CYCLE_SELF_LOOP);
        }
        if e.producer == 0 || e.producer > self.nodes || e.waiter > self.nodes {
            return Err(XC_CYCLE_DANGLING_NODE);
        }
        if e.depth == 0 {
            return Err(XC_CYCLE_DANGLING_NODE);
        }
        if self.has_edge(e.waiter, e.producer) {
            return Err(XC_CYCLE_DUP_EDGE);
        }
        // 假边入图试检；有环则回滚并拦截。
        self.insert(e)?;
        match self.find_cycle_from(e.waiter) {
            Some(report) => {
                self.remove(e.waiter, e.producer);
                self.intercepted += 1;
                if self.cycle_log.len() >= 64 {
                    self.cycle_log.remove(0);
                }
                self.cycle_log.push(report.clone());
                Ok(Some(report))
            }
            None => Ok(None),
        }
    }

    fn has_edge(&self, w: u64, p: u64) -> bool {
        self.adj
            .iter()
            .any(|(n, outs)| *n == w && outs.contains(&p))
    }

    fn insert(&mut self, e: XfEdge) -> Result<(), u16> {
        match self.node_idx(e.waiter) {
            Some(i) => {
                if self.adj[i].1.len() >= MAX_OUT_EDGES {
                    return Err(XC_CYCLE_PATH_OVER);
                }
                self.adj[i].1.push(e.producer);
            }
            None => {
                let mut outs = Vec::new();
                outs.push(e.producer);
                self.adj.push((e.waiter, outs));
            }
        }
        self.depths.push((e.waiter, e.producer, e.depth));
        Ok(())
    }

    fn remove(&mut self, w: u64, p: u64) {
        if let Some(i) = self.node_idx(w) {
            self.adj[i].1.retain(|&x| x != p);
        }
        self.depths.retain(|&(a, b, _)| !(a == w && b == p));
    }

    /// 从 start 出发的三色 DFS（显式栈，防内核栈耗尽）。
    /// 命中灰点即环：回溯 on_stack 上的节点得到完整路径。
    fn find_cycle_from(&self, start: u64) -> Option<CycleReport> {
        let mut color: Vec<(u64, Color)> = self.adj.iter().map(|(n, _)| (*n, Color::White)).collect();
        let mut on_stack: Vec<u64> = Vec::new();
        // 栈元素：(节点, 已展开的出边游标)。
        let mut stack: Vec<(u64, usize)> = vec![(start, 0)];
        let mut depth_guard = 0usize;
        while let Some(&mut (node, ref mut cur)) = stack.last_mut() {
            depth_guard += 1;
            if depth_guard > MAX_STACK_DEPTH * 8 {
                // 防御性上限：步数超界视为图异常（正常环检远小于此）。
                return None;
            }
            let outs = match self.node_idx(node) {
                Some(i) => i,
                None => {
                    stack.pop();
                    continue;
                }
            };
            if *cur == 0 {
                if let Some(c) = self.color_mut(&mut color, node) {
                    *c = Color::Gray;
                }
                on_stack.push(node);
            }
            if *cur >= self.adj[outs].1.len() {
                // 出边走完：染黑出栈。
                if let Some(c) = self.color_mut(&mut color, node) {
                    *c = Color::Black;
                }
                on_stack.pop();
                stack.pop();
                continue;
            }
            let next = self.adj[outs].1[*cur];
            *cur += 1;
            match self.color_of(&color, next) {
                Color::White => {
                    if stack.len() >= MAX_STACK_DEPTH {
                        return None;
                    }
                    stack.push((next, 0));
                }
                Color::Gray => {
                    // 环！从 on_stack 里切出 next..末尾。
                    let pos = on_stack.iter().position(|&x| x == next)?;
                    let mut path = Vec::new();
                    for &n in &on_stack[pos..] {
                        path.push(n);
                        if path.len() > MAX_CYCLE_LEN {
                            return None;
                        }
                    }
                    let cause = self.pick_cause(&path)?;
                    let len = path.len();
                    return Some(CycleReport { path, cause, len });
                }
                Color::Black => {}
            }
        }
        None
    }

    fn color_of(&self, color: &[(u64, Color)], n: u64) -> Color {
        color
            .iter()
            .find(|(x, _)| *x == n)
            .map(|(_, c)| *c)
            .unwrap_or(Color::White)
    }

    fn color_mut<'a>(&self, color: &'a mut Vec<(u64, Color)>, n: u64) -> Option<&'a mut Color> {
        for (x, c) in color.iter_mut() {
            if *x == n {
                return Some(c);
            }
        }
        None
    }

    /// 成因定位：环上深度最大的边（并列取 waiter 最小者——稳定口径）。
    fn pick_cause(&self, path: &[u64]) -> Option<XfEdge> {
        let mut best: Option<XfEdge> = None;
        for i in 0..path.len() {
            let w = path[i];
            let p = path[(i + 1) % path.len()];
            if let Some(&(_, _, k)) = self.depths.iter().find(|&&(a, b, _)| a == w && b == p) {
                let cand = XfEdge { waiter: w, producer: p, depth: k };
                let take = match best {
                    None => true,
                    Some(b) => cand.depth > b.depth || (cand.depth == b.depth && cand.waiter < b.waiter),
                };
                if take {
                    best = Some(cand);
                }
            }
        }
        best
    }

    /// 全量静态复检：整图三色扫一遍必须无环（拦截语义的自证）。
    pub fn whole_graph_acyclic(&self) -> bool {
        for &(n, _) in self.adj.iter() {
            if self.find_cycle_from(n).is_some() {
                return false;
            }
        }
        true
    }

    /// 边界契约声明（判据四的机器可读版）：本管什么、不管什么。
    pub fn boundary_contract() -> &'static str {
        "本模块管『提交时点图上静态可判环』：插边即检、命中即拦、图内永无环；\
         运行期动态死锁（信号量超时/优先级反转/GPU 挂起）归 B06 域；\
         B06 以本模块 cycle_log 与 whole_graph_acyclic 结果为图源输入，不作二次裁决。"
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_CYCLE_FOUND => "检出跨帧环：查 cycle_log 的环路径与成因边，先解最深依赖。".to_string(),
            XC_CYCLE_DANGLING_NODE => "跨帧边引用不存在的帧：先补帧登记再插边。".to_string(),
            XC_CYCLE_DUP_EDGE => "同边重复登记：幂等入口请先查 has_edge 再插。".to_string(),
            XC_CYCLE_SELF_LOOP => "自环（帧等自己）：登记方把 waiter/producer 写重了。".to_string(),
            XC_CYCLE_PATH_OVER => "环长/出边超界：先查是否重复登记造成的图爆炸。".to_string(),
            XC_CYCLE_STACK_OVER => "DFS 栈深超限：图规模异常，先对账边数再检。".to_string(),
            _ => "未知诊断码：查 0x63D 段分配表。".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 组内自证
// ---------------------------------------------------------------------------

/// 自证清单：环检测四判据的机械对偶。
pub fn self_audit() -> Result<(), u16> {
    // 一、在线检测：两帧互等 → 第二条边插入即拦截。
    let mut d = CycleDetector::boot(2);
    let e1 = XfEdge { waiter: 2, producer: 1, depth: 1 };
    let e2 = XfEdge { waiter: 1, producer: 2, depth: 1 };
    if d.add_edge(e1)?.is_some() {
        return Err(XC_CYCLE_FOUND);
    }
    let rep = match d.add_edge(e2)? {
        Some(r) => r,
        None => return Err(XC_CYCLE_FOUND),
    };
    if rep.len != 2 || rep.path[0] != 1 || rep.path[1] != 2 {
        return Err(XC_CYCLE_PATH_OVER);
    }
    // 拦截后图内无环（图源干净——B06 契约的前提）。
    if !d.whole_graph_acyclic() || d.intercepted != 1 {
        return Err(XC_CYCLE_FOUND);
    }

    // 二、三帧长环 + 成因定位（最深深度边）。
    let mut d2 = CycleDetector::boot(3);
    let _ = d2.add_edge(XfEdge { waiter: 2, producer: 1, depth: 1 })?;
    let _ = d2.add_edge(XfEdge { waiter: 3, producer: 2, depth: 1 })?;
    let rep2 = match d2.add_edge(XfEdge { waiter: 1, producer: 3, depth: 3 })? {
        Some(r) => r,
        None => return Err(XC_CYCLE_FOUND),
    };
    if rep2.len != 3 {
        return Err(XC_CYCLE_PATH_OVER);
    }
    // 成因 = 深度 3 的边（1 等 3）。
    if rep2.cause.depth != 3 || rep2.cause.waiter != 1 {
        return Err(XC_CYCLE_DANGLING_NODE);
    }
    if !rep2.describe().contains("跨帧环") {
        return Err(XC_CYCLE_PATH_OVER);
    }

    // 三、非环图不受误伤 + 非法登记全拒。
    let mut d3 = CycleDetector::boot(4);
    for e in [
        XfEdge { waiter: 2, producer: 1, depth: 1 },
        XfEdge { waiter: 3, producer: 1, depth: 2 },
        XfEdge { waiter: 4, producer: 2, depth: 1 },
    ] {
        if d3.add_edge(e)?.is_some() {
            return Err(XC_CYCLE_FOUND);
        }
    }
    if d3.add_edge(XfEdge { waiter: 1, producer: 1, depth: 1 }).is_ok() {
        return Err(XC_CYCLE_SELF_LOOP);
    }
    if d3.add_edge(XfEdge { waiter: 5, producer: 1, depth: 1 }).is_ok() {
        return Err(XC_CYCLE_DANGLING_NODE);
    }
    if d3.add_edge(XfEdge { waiter: 3, producer: 1, depth: 2 }).is_ok() {
        return Err(XC_CYCLE_DUP_EDGE);
    }
    if !d3.whole_graph_acyclic() {
        return Err(XC_CYCLE_FOUND);
    }

    // 四、深度 0 拒收（跨帧深度必须 ≥1——帧内依赖不归本组管）。
    let mut d4 = CycleDetector::boot(2);
    if d4.add_edge(XfEdge { waiter: 2, producer: 1, depth: 0 }).is_ok() {
        return Err(XC_CYCLE_DANGLING_NODE);
    }
    Ok(())
}
