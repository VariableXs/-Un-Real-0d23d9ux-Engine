//! VE-F0065 · 同步等待图可视化（VE-A 域 · A03b 同步与呈现组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0065`
//!
//! **职责定位（锚点原文）**：跨服务同步等待关系的图可视化（谁在等谁/等
//! 多久/超时没一图看懂），死锁候选高亮，与 X03 会话统一接入；含等待图的
//! 数据保留期声明。
//!
//! **数据结构（锚点原文）**：等待图。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 图更新失真 → 修正（边的时间账与事件账对不上时按事件账修正）；
//! - 死锁漏报 → 检测升级（候选环二次确认后升级为高亮并计数）；
//! - 开销超预算 → 降采样（快照频率按预算压档，不停采）。
//!
//! **性能逐项分解（锚点原文）**：O(节点)。边插入 O(1)；环检测 O(边)；
//! 快照 O(节点) 周期性。
//!
//! **跨批对接点（锚点原文）**：X03 会话接入——每条等待边带会话号，
//! 会话侧统一开关与保留期。
//!
//! **无障碍与隐私（锚点原文）**：图视图读屏替代——`read_screen_text()`
//! 把图转成「谁在等谁」逐行纯文本。
//!
//! **判据（锚点原文）**：等待图、死锁高亮、会话统一、开销预算、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0xE89B 基址（全域核查无占用）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const WAITGRAPH_VERSION: &str = "vea65_waitgraph-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0xE89B;

// ---- 诊断码 ----
pub const XC_NODE_BAD: u32 = SEG_BASE + 1; // 节点非法（零号/未知会话）
pub const XC_EDGE_BAD: u32 = SEG_BASE + 2; // 边非法（自等待/重复边）
pub const XC_DISTORT: u32 = SEG_BASE + 3; // 图更新失真（时间账不一致）
pub const XC_OVER_BUDGET: u32 = SEG_BASE + 4; // 采样开销超预算
pub const XC_NODE_FULL: u32 = SEG_BASE + 5; // 节点表满

/// 一条等待边：等待方 → 被等方。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitEdge {
    /// 等待方节点（谁在等）。
    pub from: u32,
    /// 被等方节点（等谁）。
    pub to: u32,
    /// 会话号（X03 统一接入；0 = 系统级）。
    pub session: u32,
    /// 等待开始时刻（µs 单调钟）。
    pub since_us: u64,
    /// 超时阈值（µs；0 = 无超时语义）。
    pub timeout_us: u64,
}

impl WaitEdge {
    /// 该边是否已超时（当前时刻超过开始 + 阈值；无超时语义恒 false）。
    pub fn timed_out(&self, now_us: u64) -> bool {
        self.timeout_us > 0 && now_us > self.since_us.saturating_add(self.timeout_us)
    }
    /// 等待时长。
    pub fn waited_us(&self, now_us: u64) -> u64 {
        now_us.saturating_sub(self.since_us)
    }
}

/// 等待图节点（服务/上下文）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitNode {
    pub id: u32,
    pub session: u32,
    /// 节点类型（1=渲染 2=合成 3=媒体 4=系统 0=其他）。
    pub kind: u8,
}

/// 死锁候选高亮记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeadlockCandidate {
    /// 环上节点序列（起点重复收尾不存，存环长）。
    pub cycle: [u32; 8],
    pub cycle_len: usize,
    /// 发现时刻。
    pub found_us: u64,
    /// 升级次数（二次确认计数）。
    pub confirmations: u32,
}

/// 等待图管理器。
pub struct WaitGraph {
    nodes: Vec<WaitNode>,
    edges: Vec<WaitEdge>,
    node_cap: usize,
    edge_cap: usize,
    last_code: u32,
    /// 死锁候选（环检测输出，环形只留最近 4 个）。
    candidates: [Option<DeadlockCandidate>; 4],
    cand_n: usize,
    /// 快照降采样档（100% / 50% / 25%）。
    sample_pct: u32,
    /// 采样开销累计（µs，预算判定数据源）。
    sample_cost_us: u64,
    /// 数据保留期声明（快照保留的边数上限；超出滚动淘汰）。
    pub retention_edges: usize,
    /// 事件账与时间账失真修正次数。
    distort_fixes: u32,
}

impl WaitGraph {
    pub fn new(node_cap: usize, edge_cap: usize) -> WaitGraph {
        WaitGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
            node_cap,
            edge_cap,
            last_code: 0,
            candidates: [None; 4],
            cand_n: 0,
            sample_pct: 100,
            sample_cost_us: 0,
            retention_edges: 256,
            distort_fixes: 0,
        }
    }

    /// 登记节点。
    pub fn add_node(&mut self, n: WaitNode) -> Result<(), u32> {
        if n.id == 0 || n.kind > 4 {
            self.last_code = XC_NODE_BAD;
            return Err(self.last_code);
        }
        if self.nodes.iter().any(|m| m.id == n.id) || self.nodes.len() >= self.node_cap {
            self.last_code = XC_NODE_FULL;
            return Err(self.last_code);
        }
        self.nodes.push(n);
        Ok(())
    }

    /// 记录一条等待边（谁在等谁）。
    pub fn wait_on(&mut self, e: WaitEdge) -> Result<(), u32> {
        if e.from == 0 || e.to == 0 {
            self.last_code = XC_EDGE_BAD;
            return Err(self.last_code);
        }
        if e.from == e.to {
            self.last_code = XC_EDGE_BAD;
            return Err(self.last_code);
        }
        if !self.nodes.iter().any(|n| n.id == e.from) || !self.nodes.iter().any(|n| n.id == e.to)
        {
            self.last_code = XC_NODE_BAD;
            return Err(self.last_code);
        }
        // 重复边：同 from→to 幂等更新时间账（不重复计数）。
        if let Some(old) = self.edges.iter_mut().find(|x| x.from == e.from && x.to == e.to) {
            if old.since_us > e.since_us {
                // 新事件时间倒挂 = 图更新失真：按事件账修正并计数。
                self.distort_fixes += 1;
                self.last_code = XC_DISTORT;
            }
            *old = e;
            return Ok(());
        }
        if self.edges.len() >= self.edge_cap {
            // 滚动淘汰最旧（保留期声明的机器可读形式）。
            self.edges.remove(0);
        }
        self.edges.push(e);
        Ok(())
    }

    /// 解除等待（等待结束）。
    pub fn wait_done(&mut self, from: u32, to: u32) -> bool {
        let before = self.edges.len();
        self.edges.retain(|x| !(x.from == from && x.to == to));
        before != self.edges.len()
    }

    /// 环检测（死锁候选高亮）：from → to 有向边找环。
    /// node_cap ≤ 64 下 DFS 定容栈可行；返回首环（确定性：按节点 id 升序
    /// 展开）。
    pub fn detect_cycle(&mut self, now_us: u64) -> Option<DeadlockCandidate> {
        // 邻接表（定容展开）。
        let n = self.nodes.len();
        if n == 0 || self.edges.is_empty() {
            return None;
        }
        const MAXN: usize = 64;
        let mut adj = [[0u32; MAXN]; MAXN];
        let mut adj_n = [0usize; MAXN];
        for e in self.edges.iter() {
            let f = match self.nodes.iter().position(|m| m.id == e.from) {
                Some(i) => i,
                None => continue,
            };
            let t = match self.nodes.iter().position(|m| m.id == e.to) {
                Some(i) => i,
                None => continue,
            };
            if adj_n[f] < MAXN {
                adj[f][adj_n[f]] = t as u32;
                adj_n[f] += 1;
            }
        }
        // 三色 DFS 找环。
        const WHITE: u8 = 0;
        const GRAY: u8 = 1;
        const BLACK: u8 = 2;
        let mut color = [WHITE; MAXN];
        let mut path: Vec<usize> = Vec::new();
        let mut found: Option<Vec<usize>> = None;
        for start in 0..n {
            if color[start] != WHITE {
                continue;
            }
            path.clear();
            path.push(start);
            color[start] = GRAY;
            let mut i = 0usize;
            while i < path.len() {
                let cur = path[i];
                if i >= adj_n[cur] {
                    color[cur] = BLACK;
                    path.pop();
                    if i == 0 {
                        break;
                    }
                    i = path.len().saturating_sub(1);
                    if path.is_empty() {
                        break;
                    }
                    continue;
                }
                let nxt = adj[cur][i] as usize;
                // 推进游标：用局部变量模拟（重入式实现为保持栈形状）。
                // 由于 path 游标语义复杂，这里改为显式游标表。
                let _ = nxt;
                break;
            }
            // 上面重入式实现复杂度失控，改用标准递归展开（node_cap ≤ 64
            // 栈深可控）——见下方 find_cycle_from。
        }
        let _ = (GRAY, BLACK, &mut found);
        // 标准实现：逐起点 DFS。
        let mut result: Option<DeadlockCandidate> = None;
        'outer: for start in 0..n {
            color = [WHITE; MAXN];
            path.clear();
            if self.find_cycle_from(start, &adj, &adj_n, &mut color, &mut path, n) {
                // path 形如 [a, b, c, a 起点]：环 = path 内容。
                let mut cand = DeadlockCandidate {
                    cycle: [0u32; 8],
                    cycle_len: path.len().min(8),
                    found_us: now_us,
                    confirmations: 1,
                };
                for (k, idx) in path.iter().take(8).enumerate() {
                    cand.cycle[k] = self.nodes[*idx].id;
                }
                // 二次确认：与最近候选相同环则确认数 +1（检测升级）。
                for c in self.candidates.iter_mut().take(self.cand_n) {
                    if let Some(c) = c {
                        if c.cycle_len == cand.cycle_len && c.cycle[..c.cycle_len] == cand.cycle[..cand.cycle_len] {
                            c.confirmations += 1;
                            c.found_us = now_us;
                            result = Some(*c);
                            break 'outer;
                        }
                    }
                }
                self.candidates[self.cand_n % 4] = Some(cand);
                self.cand_n += 1;
                result = Some(cand);
                break 'outer;
            }
        }
        result
    }

    fn find_cycle_from(
        &self,
        start: usize,
        adj: &[[u32; 64]; 64],
        adj_n: &[usize; 64],
        color: &mut [u8; 64],
        path: &mut Vec<usize>,
        n: usize,
    ) -> bool {
        if path.len() > 8 {
            return false;
        }
        color[start] = 1; // GRAY
        path.push(start);
        for k in 0..adj_n[start] {
            let nxt = adj[start][k] as usize;
            if nxt >= n {
                continue;
            }
            if color[nxt] == 1 {
                // 找到环：path 中从 nxt 起到末尾即环。
                let pos = path.iter().position(|x| *x == nxt).unwrap_or(0);
                let cyc: Vec<usize> = path[pos..].to_vec();
                path.clear();
                path.extend(cyc);
                return true;
            }
            if color[nxt] == 0 && self.find_cycle_from(nxt, adj, adj_n, color, path, n) {
                return true;
            }
        }
        color[start] = 2; // BLACK
        path.pop();
        false
    }

    /// 开销预算判定：采样开销超预算 → 降采样档（100 → 50 → 25）。
    pub fn budget_gate(&mut self, budget_us: u64) -> u32 {
        if self.sample_cost_us > budget_us && self.sample_pct > 25 {
            self.sample_pct /= 2;
            self.last_code = XC_OVER_BUDGET;
        }
        self.sample_pct
    }

    pub fn sample_cost_add(&mut self, us: u64) {
        self.sample_cost_us += us;
    }
    pub fn sample_pct(&self) -> u32 {
        self.sample_pct
    }
    pub fn distort_fixes(&self) -> u32 {
        self.distort_fixes
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 读屏替代：图 → 「谁在等谁」逐行纯文本（无障碍判据核心）。
    pub fn read_screen_text(&self, now_us: u64) -> String {
        let mut out = String::new();
        for e in self.edges.iter() {
            let state = if e.timed_out(now_us) { "已超时" } else { "等待中" };
            out.push_str(&alloc::format!(
                "节点{} 等待 节点{} 会话{} 已等{}µs {}\n",
                e.from,
                e.to,
                e.session,
                e.waited_us(now_us),
                state
            ));
        }
        if out.is_empty() {
            out.push_str("等待图为空");
        }
        out
    }
}

// =====================================================================
// 自检（判据锚点：等待图 / 死锁高亮 / 会话统一 / 开销预算）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

fn node(id: u32, kind: u8) -> WaitNode {
    WaitNode { id, session: 1, kind }
}

fn edge(from: u32, to: u32, since: u64, timeout: u64) -> WaitEdge {
    WaitEdge { from, to, session: 1, since_us: since, timeout_us: timeout }
}

/// VE-F0065 域自检。
pub fn run_vea65_checks() -> CheckSet {
    let mut set = CheckSet::new("vea65");

    // C65-01 等待图：边登记与解除
    {
        let mut g = WaitGraph::new(8, 16);
        let _ = g.add_node(node(1, 1));
        let _ = g.add_node(node(2, 2));
        let _ = g.wait_on(edge(1, 2, 100, 0));
        let ok1 = g.edge_count() == 1;
        let ok2 = g.wait_done(1, 2) && g.edge_count() == 0;
        chk(&mut set, "C65-01 边登记解除", ok1 && ok2, "边账失准");
    }

    // C65-02 自等待拒绝
    {
        let mut g = WaitGraph::new(8, 16);
        let _ = g.add_node(node(1, 1));
        let ok = g.wait_on(edge(1, 1, 0, 0)) == Err(XC_EDGE_BAD);
        chk(&mut set, "C65-02 自等待拒", ok, "自等待被放行");
    }

    // C65-03 会话统一：边携带会话号（读屏文本可见）
    {
        let mut g = WaitGraph::new(8, 16);
        let _ = g.add_node(node(1, 1));
        let _ = g.add_node(node(2, 2));
        let _ = g.wait_on(edge(1, 2, 0, 0));
        let txt = g.read_screen_text(50);
        chk(&mut set, "C65-03 会话可见", txt.contains("会话1"), "会话号缺失");
    }

    // C65-04 死锁高亮：三点环检出且候选入册
    {
        let mut g = WaitGraph::new(8, 16);
        for id in [1u32, 2, 3] {
            let _ = g.add_node(node(id, 1));
        }
        let _ = g.wait_on(edge(1, 2, 0, 0));
        let _ = g.wait_on(edge(2, 3, 0, 0));
        let _ = g.wait_on(edge(3, 1, 0, 0));
        let c = g.detect_cycle(1000);
        let ok = matches!(c, Some(c) if c.cycle_len == 3);
        chk(&mut set, "C65-04 环检出", ok, "环漏报");
    }

    // C65-05 检测升级：同环再次检出确认数 +1
    {
        let mut g = WaitGraph::new(8, 16);
        for id in [1u32, 2, 3] {
            let _ = g.add_node(node(id, 1));
        }
        let _ = g.wait_on(edge(1, 2, 0, 0));
        let _ = g.wait_on(edge(2, 3, 0, 0));
        let _ = g.wait_on(edge(3, 1, 0, 0));
        let _ = g.detect_cycle(1000);
        let c2 = g.detect_cycle(2000);
        let ok = matches!(c2, Some(c) if c.confirmations == 2);
        chk(&mut set, "C65-05 二次确认", ok, "确认未升级");
    }

    // C65-06 无环图不误报
    {
        let mut g = WaitGraph::new(8, 16);
        for id in [1u32, 2, 3] {
            let _ = g.add_node(node(id, 1));
        }
        let _ = g.wait_on(edge(1, 2, 0, 0));
        let _ = g.wait_on(edge(2, 3, 0, 0));
        let ok = g.detect_cycle(1000).is_none();
        chk(&mut set, "C65-06 无环不误报", ok, "无环误报");
    }

    // C65-07 超时判定：无超时语义恒不超时；有阈值超时成立
    {
        let a = edge(1, 2, 1000, 0);
        let b = edge(1, 2, 1000, 500);
        let ok = !a.timed_out(2000) && b.timed_out(2000) && !b.timed_out(1500);
        chk(&mut set, "C65-07 超时判定", ok, "超时判定失准");
    }

    // C65-08 图更新失真：时间倒挂幂等更新触发修正计数
    {
        let mut g = WaitGraph::new(8, 16);
        let _ = g.add_node(node(1, 1));
        let _ = g.add_node(node(2, 1));
        let _ = g.wait_on(edge(1, 2, 2000, 0));
        let _ = g.wait_on(edge(1, 2, 1000, 0));
        let ok = g.distort_fixes() == 1 && g.last_code() == XC_DISTORT;
        chk(&mut set, "C65-08 失真修正", ok, "倒挂未修正");
    }

    // C65-09 开销预算：超预算降采样档
    {
        let mut g = WaitGraph::new(8, 16);
        g.sample_cost_add(1000);
        let p1 = g.budget_gate(500);
        let p2 = g.budget_gate(500);
        let ok = p1 == 50 && p2 == 25;
        chk(&mut set, "C65-09 降采样", ok, "降采样失准");
    }

    // C65-10 预算内不降档
    {
        let mut g = WaitGraph::new(8, 16);
        g.sample_cost_add(100);
        let ok = g.budget_gate(500) == 100;
        chk(&mut set, "C65-10 预算内不降", ok, "预算内误降档");
    }

    // C65-11 保留期滚动：超容量淘汰最旧边
    {
        let mut g = WaitGraph::new(8, 3);
        for id in [1u32, 2, 3, 4] {
            let _ = g.add_node(node(id, 1));
        }
        let _ = g.wait_on(edge(1, 2, 0, 0));
        let _ = g.wait_on(edge(2, 3, 0, 0));
        let _ = g.wait_on(edge(3, 4, 0, 0));
        let _ = g.wait_on(edge(1, 3, 0, 0));
        let ok = g.edge_count() == 3;
        chk(&mut set, "C65-11 滚动淘汰", ok, "淘汰账错");
    }

    // C65-12 非法节点拒绝（零 id / 未知类型）
    {
        let mut g = WaitGraph::new(8, 16);
        let ok = g.add_node(node(0, 1)) == Err(XC_NODE_BAD)
            && g.add_node(WaitNode { id: 5, session: 1, kind: 9 }) == Err(XC_NODE_BAD);
        chk(&mut set, "C65-12 节点校验", ok, "非法节点被放行");
    }

    // C65-13 未知端点边拒绝
    {
        let mut g = WaitGraph::new(8, 16);
        let _ = g.add_node(node(1, 1));
        let ok = g.wait_on(edge(1, 99, 0, 0)) == Err(XC_NODE_BAD);
        chk(&mut set, "C65-13 端点校验", ok, "未知端点被放行");
    }

    // C65-14 确定性
    {
        let seq = || -> usize {
            let mut g = WaitGraph::new(8, 16);
            for id in [1u32, 2, 3] {
                let _ = g.add_node(node(id, 1));
            }
            let _ = g.wait_on(edge(1, 2, 0, 0));
            let _ = g.wait_on(edge(2, 3, 0, 0));
            let _ = g.wait_on(edge(3, 1, 0, 0));
            let _ = g.detect_cycle(1000);
            g.cand_n
        };
        chk(&mut set, "C65-14 确定性", seq() == seq(), "两次不一致");
    }

    // C65-15 读屏替代逐行格式
    {
        let mut g = WaitGraph::new(8, 16);
        let _ = g.add_node(node(1, 1));
        let _ = g.add_node(node(2, 2));
        let _ = g.wait_on(edge(1, 2, 0, 0));
        let txt = g.read_screen_text(10);
        let ok = txt.contains("节点1 等待 节点2") && txt.contains("已等");
        chk(&mut set, "C65-15 读屏格式", ok, "读屏文本缺要素");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea65_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
