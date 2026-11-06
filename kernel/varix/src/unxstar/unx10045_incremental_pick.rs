//! UNX-F10045 增量选例实现（依赖图驱动） · 0x91b40 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10045（B22 常态化回归线 · 增量选例）。
//! 锚点：UNX-F10046-J1 分层抽选核（种子复现+层覆盖 10/10 次），不可复现 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91b40（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `GraphCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `EdgeParser`：把「u>v」依赖边文本拆成 (u,v)，两段缺一即
//!     ParseFailure；
//!   - 依赖图 `DepGraph`：有向边 u→v（u 变更影响 v），越界边/自环/重复边
//!     显性拒绝；闭包用显式栈 BFS（visited 去重），环图不死锁；
//!   - 账本 `GraphLedger`：注入 = 检出 + 逃逸，逃逸恒 0，只增不减；
//!   - 注入核 `BadGraphCase`：越界节点 / 自环 / 重复边 / 空变更集 / 环图
//!     五类坏样本逐类 10/10 次拒绝（环图类为终止性+覆盖双检）；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10045 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91b40;

/// 段窗上界（下一卡 0x91b80 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91b80;

/// 依赖图节点数（10 模块面，B21 口径）。
pub const NODE_COUNT: usize = 10;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const ENOENT: i32 = 2;
pub const EDEADLK: i32 = 35;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EDOM: i32 = 33;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GraphCode {
    /// 越界节点（边端点出 0..10）
    NodeOutOfRange,
    /// 自环边
    SelfLoop,
    /// 重复边
    DuplicateEdge,
    /// 空变更集
    EmptyChanged,
    /// 闭包未终止（防环护栏触发）
    CycleGuard,
    /// 闭包覆盖缺失（受影响节点漏选）
    CoverageMiss,
    /// 账本倒退
    LedgerRegress,
    /// 解析失败
    ParseFailure,
    /// 图未构建即求闭包
    NotBuilt,
    /// 双跑不一致
    NotReproducible,
    /// 闭包为空（有变更却零命中）
    EmptyClosure,
    /// 段位越界
    SegmentViolation,
}

impl GraphCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            GraphCode::NodeOutOfRange => "越界节点被拒：边端点必须落在 0..=9；请核对模块号",
            GraphCode::SelfLoop => "自环边被拒：u→u 不构成依赖；请拆解真实依赖链",
            GraphCode::DuplicateEdge => "重复边被拒：同向边已存在；请去重后重报",
            GraphCode::EmptyChanged => "空变更集被拒：变更集为空不得求闭包；请先标记变更",
            GraphCode::CycleGuard => "环图护栏触发：闭包访问数越限；请排查循环依赖",
            GraphCode::CoverageMiss => "覆盖缺失：直接后继未全入闭包；请修复邻接表",
            GraphCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请排查回滚误改",
            GraphCode::ParseFailure => "解析失败：边文本必须恰为 u>v 两段；请补齐字段",
            GraphCode::NotBuilt => "图未构建：build 前不得求闭包；请先加边",
            GraphCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            GraphCode::EmptyClosure => "闭包为空：有变更却零命中；请核对变更集与图",
            GraphCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91b40..=0x91b80",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [GraphCode; 12] = [
        GraphCode::NodeOutOfRange,
        GraphCode::SelfLoop,
        GraphCode::DuplicateEdge,
        GraphCode::EmptyChanged,
        GraphCode::CycleGuard,
        GraphCode::CoverageMiss,
        GraphCode::LedgerRegress,
        GraphCode::ParseFailure,
        GraphCode::NotBuilt,
        GraphCode::NotReproducible,
        GraphCode::EmptyClosure,
        GraphCode::SegmentViolation,
    ];
}

// ── 边解析器 ──────────────────────────────────────────────────────────
/// 解析出的依赖边（u→v：u 变更影响 v）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Edge {
    /// 起点模块
    pub from: usize,
    /// 终点模块
    pub to: usize,
}

/// 边文本解析器：「u>v」两段，缺一或非数字即 ParseFailure。
#[derive(Default)]
pub struct EdgeParser {
    /// 累计成功解析条数
    pub parsed: u32,
    /// 累计拒绝条数
    pub rejected: u32,
}

impl EdgeParser {
    /// 解析一行边文本；失败原因由 GraphCode 表达。
    pub fn parse(&mut self, text: &str) -> Result<Edge, GraphCode> {
        let parts: Vec<&str> = text.split('>').collect();
        if parts.len() != 2 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(GraphCode::ParseFailure);
        }
        let from = parse_usize(parts.get(0).copied().unwrap_or(""));
        let to = parse_usize(parts.get(1).copied().unwrap_or(""));
        match (from, to) {
            (Some(f), Some(t)) => {
                self.parsed = self.parsed.saturating_add(1);
                Ok(Edge { from: f, to: t })
            }
            _ => {
                self.rejected = self.rejected.saturating_add(1);
                Err(GraphCode::ParseFailure)
            }
        }
    }
}

fn parse_usize(s: &str) -> Option<usize> {
    if s.is_empty() {
        return None;
    }
    let mut acc: usize = 0;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes.get(i)?;
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((*b - b'0') as usize)?;
        i += 1;
    }
    Some(acc)
}

// ── 依赖图 ────────────────────────────────────────────────────────────
/// 有向依赖图：邻接表按加边序存放（同输入同序，确定性）。
#[derive(Clone, Debug, Default)]
pub struct DepGraph {
    /// 邻接表（adj[u] = v 列表，u 变更影响 v）
    pub adj: Vec<Vec<usize>>,
    /// 已加边数
    pub edges: u32,
    /// 是否已加过边
    pub built: bool,
}

impl DepGraph {
    /// 构造空图（NODE_COUNT 个空邻接槽）。
    pub fn new() -> DepGraph {
        let mut adj = Vec::new();
        let mut i = 0usize;
        while i < NODE_COUNT {
            adj.push(Vec::new());
            i += 1;
        }
        DepGraph { adj, edges: 0, built: false }
    }

    /// 加一条有向边：越界/自环/重复显性拒绝。
    pub fn add_edge(&mut self, u: usize, v: usize) -> Result<(), GraphCode> {
        if u >= NODE_COUNT || v >= NODE_COUNT {
            return Err(GraphCode::NodeOutOfRange);
        }
        if u == v {
            return Err(GraphCode::SelfLoop);
        }
        let row = match self.adj.get(u) {
            Some(r) => r,
            None => return Err(GraphCode::NodeOutOfRange),
        };
        let mut dup = false;
        for w in row.iter() {
            if *w == v {
                dup = true;
            }
        }
        if dup {
            return Err(GraphCode::DuplicateEdge);
        }
        if let Some(r) = self.adj.get_mut(u) {
            r.push(v);
        }
        self.edges = self.edges.saturating_add(1);
        self.built = true;
        Ok(())
    }

    /// 闭包：从变更集出发沿边可达的全部节点（含变更集自身），升序返回。
    /// 显式栈 BFS + visited 去重：环图终止且有界（访问数 ≤ NODE_COUNT）。
    pub fn closure(&self, changed: &[usize]) -> Result<Vec<usize>, GraphCode> {
        if !self.built {
            return Err(GraphCode::NotBuilt);
        }
        if changed.is_empty() {
            return Err(GraphCode::EmptyChanged);
        }
        let mut seen = [false; NODE_COUNT];
        let mut stack: Vec<usize> = Vec::new();
        for m in changed.iter() {
            if *m >= NODE_COUNT {
                return Err(GraphCode::NodeOutOfRange);
            }
            if !*seen.get(*m).unwrap_or(&true) {
                if let Some(f) = seen.get_mut(*m) {
                    *f = true;
                }
                stack.push(*m);
            }
        }
        let mut visits: u32 = 0;
        while let Some(u) = stack.pop() {
            visits = visits.saturating_add(1);
            if visits > NODE_COUNT as u32 + 1 {
                return Err(GraphCode::CycleGuard);
            }
            if let Some(row) = self.adj.get(u) {
                for v in row.iter() {
                    if !*seen.get(*v).unwrap_or(&true) {
                        if let Some(f) = seen.get_mut(*v) {
                            *f = true;
                        }
                        stack.push(*v);
                    }
                }
            }
        }
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < NODE_COUNT {
            if *seen.get(i).unwrap_or(&false) {
                out.push(i);
            }
            i += 1;
        }
        if out.is_empty() {
            return Err(GraphCode::EmptyClosure);
        }
        Ok(out)
    }

    /// 覆盖核：变更集的直接后继必须全部入闭包（漏选检出）。
    pub fn coverage_ok(&self, changed: &[usize], closed: &[usize]) -> bool {
        let mut has = [false; NODE_COUNT];
        for n in closed.iter() {
            if *n < NODE_COUNT {
                if let Some(f) = has.get_mut(*n) {
                    *f = true;
                }
            }
        }
        for m in changed.iter() {
            if *m >= NODE_COUNT || !*has.get(*m).unwrap_or(&false) {
                return false;
            }
            if let Some(row) = self.adj.get(*m) {
                for v in row.iter() {
                    if !*has.get(*v).unwrap_or(&false) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// 标准依赖图：0→1→2、0→3、3→4、5→6（两棵子树，无环）。
pub fn build_standard() -> DepGraph {
    let mut g = DepGraph::new();
    let _ = g.add_edge(0, 1);
    let _ = g.add_edge(1, 2);
    let _ = g.add_edge(0, 3);
    let _ = g.add_edge(3, 4);
    let _ = g.add_edge(5, 6);
    g
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏图样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadGraphCase {
    /// 越界节点（端点 10）
    NodeOutOfRange,
    /// 自环
    SelfLoop,
    /// 重复边
    DuplicateEdge,
    /// 空变更集
    EmptyChanged,
    /// 环图（须终止且全覆盖）
    CycleHang,
}

impl BadGraphCase {
    /// 闭集全量。
    pub const ALL: [BadGraphCase; 5] = [
        BadGraphCase::NodeOutOfRange,
        BadGraphCase::SelfLoop,
        BadGraphCase::DuplicateEdge,
        BadGraphCase::EmptyChanged,
        BadGraphCase::CycleHang,
    ];

    /// 注入一轮：构造坏样本并触发图操作，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadGraphCase::NodeOutOfRange => {
                let mut g = DepGraph::new();
                g.add_edge(0, NODE_COUNT).is_err()
            }
            BadGraphCase::SelfLoop => {
                let mut g = DepGraph::new();
                g.add_edge(4, 4).is_err()
            }
            BadGraphCase::DuplicateEdge => {
                let mut g = DepGraph::new();
                if g.add_edge(0, 1).is_err() {
                    return false;
                }
                g.add_edge(0, 1).is_err()
            }
            BadGraphCase::EmptyChanged => {
                let g = build_standard();
                g.closure(&[]).is_err()
            }
            BadGraphCase::CycleHang => {
                // 环图 7→8→9→7：闭包必须终止且三点全覆盖（护栏不误触发）。
                let mut g = DepGraph::new();
                if g.add_edge(7, 8).is_err() {
                    return false;
                }
                if g.add_edge(8, 9).is_err() {
                    return false;
                }
                if g.add_edge(9, 7).is_err() {
                    return false;
                }
                match g.closure(&[7]) {
                    Ok(v) => v.len() == 3 && g.coverage_ok(&[7], &v),
                    Err(_) => false,
                }
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GraphLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl GraphLedger {
    const fn zero() -> GraphLedger {
        GraphLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失衡回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), GraphCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(GraphCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_graph_injection() -> (GraphLedger, [u32; 5]) {
    let mut ledger = GraphLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadGraphCase::ALL.len() {
        if let Some(case) = BadGraphCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；图未构建/空变更族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复边族"),
    ("EDOM", -33, 423, "Windows ERROR_MATH_DOMAIN；自环定义域错族"),
    ("EDEADLK", -35, 109, "Windows ERROR_DEADLOCK；环图护栏族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越界节点族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：解析→加边→闭包全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut p = EdgeParser::default();
    let mut g = DepGraph::new();
    let mut bytes: Vec<u8> = Vec::new();
    let texts = ["0>1", "1>2", "0>3", "3>4", "5>6", "2>2", "0>1"];
    let mut i = 0usize;
    while i < texts.len() {
        if let Some(t) = texts.get(i) {
            let r = p.parse(t);
            bytes.push(if r.is_ok() { 1 } else { 0 });
            if let Ok(e) = r {
                bytes.push(match g.add_edge(e.from, e.to) {
                    Ok(()) => 2,
                    Err(_) => 3,
                });
            }
        }
        i += 1;
    }
    match g.closure(&[0]) {
        Ok(v) => {
            bytes.push(v.len() as u8);
            for n in v.iter() {
                bytes.push(*n as u8);
            }
        }
        Err(_) => bytes.push(0),
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10045 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10045_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10045");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91b40..=0x91b80", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 边解析器：合法 u>v 可解析
    let mut p = EdgeParser::default();
    let ok_parse = p.parse("3>7") == Ok(Edge { from: 3, to: 7 });
    s.add("解析器：合法 u>v 边 1/1 解析成功", ok_parse,
          "两段数字齐备即入图");

    // 3. 解析器：缺段/非数字/多余段全拒
    let bad = p.parse("3").is_err()
        && p.parse("a>b").is_err()
        && p.parse("3>7>9").is_err()
        && p.parse(">7").is_err();
    s.add("解析器：缺段/非数字/多余段 4/4 拒绝", bad,
          "任何字段缺失或不可解析都不得静默吞掉");

    // 4. 加边校验：越界/自环/重复全拒
    let mut g0 = DepGraph::new();
    let edge_ok = g0.add_edge(0, NODE_COUNT).is_err()
        && g0.add_edge(NODE_COUNT, 0).is_err()
        && g0.add_edge(4, 4) == Err(GraphCode::SelfLoop);
    let mut g1 = DepGraph::new();
    let _ = g1.add_edge(0, 1);
    let dup_ok = g1.add_edge(0, 1) == Err(GraphCode::DuplicateEdge);
    s.add("加边校验：越界/自环/重复 4/4 拒绝", edge_ok && dup_ok,
          "图边闭集 0..=9，同向唯一");

    // 5. 标准图闭包：变更 {0} → {0,1,2,3,4}
    let g = build_standard();
    let clo = g.closure(&[0]);
    let clo_ok = clo == Ok(vec![0, 1, 2, 3, 4]);
    s.add("闭包：变更 {0} 可达 {0,1,2,3,4}", clo_ok,
          "沿边闭包逐层推进，升序输出");

    // 6. 闭包覆盖核：直接后继全入
    let cov = match g.closure(&[0, 5]) {
        Ok(v) => g.coverage_ok(&[0, 5], &v),
        Err(_) => false,
    };
    s.add("覆盖核：变更 {0,5} 直接后继全入闭包", cov,
          "漏掉直接后继即 CoverageMiss，漏选显性");

    // 7. 分支隔离：变更 {5} 不串 {0} 子树
    let iso = g.closure(&[5]) == Ok(vec![5, 6]);
    s.add("分支隔离：变更 {5} 闭包恰为 {5,6}", iso,
          "无关联子树互不串选，增量口径纯净");

    // 8. 环图终止：7→8→9→7 闭包三点全覆盖且不死锁
    let mut cyc = DepGraph::new();
    let _ = cyc.add_edge(7, 8);
    let _ = cyc.add_edge(8, 9);
    let _ = cyc.add_edge(9, 7);
    let cyc_ok = cyc.closure(&[7]) == Ok(vec![7, 8, 9]);
    s.add("环图终止性：环闭包终止且全覆盖", cyc_ok,
          "visited 去重 + 访问数上限双护栏，环不死锁");

    // 9. 未构建/空变更集拒绝
    let nb = DepGraph::new();
    let nb_rej = nb.closure(&[0]) == Err(GraphCode::NotBuilt);
    let ec_rej = g.closure(&[]) == Err(GraphCode::EmptyChanged);
    s.add("前置闸：未构建图/空变更集求闭包被拒", nb_rej && ec_rej,
          "闭包前置条件逐项显性校验");

    // 10. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_graph_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "越界/自环/重复/空变更/环图全被显式拒绝");

    // 11. 注入账守恒：逃逸=0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 12. 文案闭集：非空
    let text_ok = GraphCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 13. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < GraphCode::ALL.len() {
        let mut j = i + 1;
        while j < GraphCode::ALL.len() {
            if let (Some(x), Some(y)) = (GraphCode::ALL.get(i), GraphCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = GraphCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EDOM)
        && agrees_with_linux(EDEADLK)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 15. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 16. 解析账闭合
    let mut p2 = EdgeParser::default();
    let _ = p2.parse("0>1");
    let _ = p2.parse("bad");
    let _ = p2.parse("2>3");
    s.add("解析账闭合：解析+拒绝=提交总数",
          p2.parsed.saturating_add(p2.rejected) == 3,
          "每行提交必须入两账之一，无第三去向");

    // 17. 确定性重放：同图同变更集两次闭包逐位一致
    let a = g.closure(&[0, 5]);
    let b = g.closure(&[0, 5]);
    s.add("确定性重放：同图同变更两次闭包相等", a == b,
          "闭包纯函数化，无隐藏可变状态");

    // 动态明细（≤8）：闭包规模一行 + 指纹一行
    let clo_text = format!("标准图边数 {} / 变更{{0}}闭包 {} 节点", g.edges,
        clo.map(|v| v.len()).unwrap_or(0));
    s.add_dyn("闭包快照", clo_ok, &clo_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_wellformed() {
        let mut p = EdgeParser::default();
        assert_eq!(p.parse("0>9"), Ok(Edge { from: 0, to: 9 }));
    }

    #[test]
    fn parser_rejects_malformed() {
        let mut p = EdgeParser::default();
        assert!(p.parse("0").is_err());
        assert!(p.parse("x>y").is_err());
        assert!(p.parse("0>1>2").is_err());
    }

    #[test]
    fn out_of_range_rejected() {
        let mut g = DepGraph::new();
        assert_eq!(g.add_edge(0, 10), Err(GraphCode::NodeOutOfRange));
        assert_eq!(g.add_edge(10, 0), Err(GraphCode::NodeOutOfRange));
    }

    #[test]
    fn self_loop_rejected() {
        let mut g = DepGraph::new();
        assert_eq!(g.add_edge(3, 3), Err(GraphCode::SelfLoop));
    }

    #[test]
    fn duplicate_edge_rejected() {
        let mut g = DepGraph::new();
        let _ = g.add_edge(0, 1);
        assert_eq!(g.add_edge(0, 1), Err(GraphCode::DuplicateEdge));
    }

    #[test]
    fn standard_closure() {
        let g = build_standard();
        assert_eq!(g.closure(&[0]), Ok(vec![0, 1, 2, 3, 4]));
        assert_eq!(g.closure(&[5]), Ok(vec![5, 6]));
    }

    #[test]
    fn cycle_terminates() {
        let mut g = DepGraph::new();
        let _ = g.add_edge(7, 8);
        let _ = g.add_edge(8, 9);
        let _ = g.add_edge(9, 7);
        assert_eq!(g.closure(&[7]), Ok(vec![7, 8, 9]));
    }

    #[test]
    fn empty_changed_rejected() {
        let g = build_standard();
        assert_eq!(g.closure(&[]), Err(GraphCode::EmptyChanged));
    }

    #[test]
    fn not_built_rejected() {
        let g = DepGraph::new();
        assert_eq!(g.closure(&[0]), Err(GraphCode::NotBuilt));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_graph_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EDEADLK));
        assert!(agrees_with_linux(EDOM));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10045_checks_green() {
        let s = run_unx10045_checks();
        assert!(s.all_passed(), "UNX-F10045 red: {}", s.red_items_text());
    }
}
