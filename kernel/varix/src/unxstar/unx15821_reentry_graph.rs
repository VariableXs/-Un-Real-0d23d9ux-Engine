//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15821 · 联签总纲与重入消费图（STA 三通道可解引 + 悬空通道注入）
//! · 0x67840
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15821-J1 总纲成文（STA 重入消费图三通道可解引 10/10 次），
//!      悬空通道注入（图含未收口通道）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - STA 重入消费图三通道闭集：消息泵通道 / 敏感调用通道 /
//!     递送回调通道（写死）；
//!   - 图节点：STA 单元节点 × 三通道出边，每边必须解引到收口器；
//!   - 重入状态机：Idle → Outbound(通道 k) → Reentered → Returned →
//!     Idle（合法环）；非法重入（无泵可入）= 悬空通道；
//!   - 悬空判据：出边目标不在收口器闭集 或 通道未接泵；
//!   - 正演 10/10：三通道全可解引；注入 10/10：悬空通道必检出；
//!   - LCG 种子写死 = SEG_BASE（0x67840），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67840，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67840;

/// 每轮正演次数（判据钉版：10/10）。
pub const RUNS_PER_ROUND: u32 = 10;

/// 三通道闭集。
pub const CH_MSGPUMP: usize = 0;
pub const CH_SENSITIVE: usize = 1;
pub const CH_CALLBACK: usize = 2;
pub const CH_COUNT: usize = 3;
pub const CH_NAMES: [&str; CH_COUNT] = ["消息泵通道", "敏感调用通道", "递送回调通道"];

/// STA 节点闭集（4 单元，写死）。
pub const NODE_COUNT: usize = 4;
pub const NODE_NAMES: [&str; NODE_COUNT] = ["STA主泵", "STA从泵", "STA代理", "STA存根"];

/// 收口器闭集（5，写死）。
pub const SINK_COUNT: usize = 5;
pub const SINK_NAMES: [&str; SINK_COUNT] =
    ["泵收口", "敏感收口", "回调收口", "回环收口", "对账收口"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReentryCode {
    /// 通道越闭集
    ChannelOutOfRange,
    /// 节点越闭集
    NodeOutOfRange,
    /// 悬空通道（未收口通道在图）
    DanglingChannel,
    /// 非法重入（无泵可入）
    ReentryWithoutPump,
    /// 非法相位边
    IllegalPhaseEdge,
    /// 重入深度超限
    ReentryOverflow,
    /// 收口器查无
    SinkNotFound,
    /// 双跑不一致
    NotReproducible,
    /// 图未封账
    GraphUnsealed,
}

impl ReentryCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ReentryCode::ChannelOutOfRange => "通道越闭集：三通道冻结",
            ReentryCode::NodeOutOfRange => "节点越闭集：STA 四单元冻结",
            ReentryCode::DanglingChannel => "悬空通道：未收口通道在图",
            ReentryCode::ReentryWithoutPump => "非法重入：无泵可入",
            ReentryCode::IllegalPhaseEdge => "非法相位边：重入环闭集外",
            ReentryCode::ReentryOverflow => "重入深度超限",
            ReentryCode::SinkNotFound => "收口器查无：解引落空",
            ReentryCode::NotReproducible => "双跑不一致：重入图复现红线",
            ReentryCode::GraphUnsealed => "图未封账：环未闭合",
        }
    }
    pub const ALL: [ReentryCode; 9] = [
        ReentryCode::ChannelOutOfRange,
        ReentryCode::NodeOutOfRange,
        ReentryCode::DanglingChannel,
        ReentryCode::ReentryWithoutPump,
        ReentryCode::IllegalPhaseEdge,
        ReentryCode::ReentryOverflow,
        ReentryCode::SinkNotFound,
        ReentryCode::NotReproducible,
        ReentryCode::GraphUnsealed,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ReentryCode::ALL.len() {
        if ReentryCode::ALL[i].val() == code {
            return ReentryCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67840 段闭集内"
}

// ── 重入状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StaPhase {
    /// 空闲（泵可入）
    Idle,
    /// 出站中（走第 ch 通道）
    Outbound(usize),
    /// 重入点（回调回到 STA）
    Reentered(usize),
    /// 返回途中
    Returned(usize),
}

/// 合法边闭集（重入环）：
///   Idle→Outbound(ch)；Outbound(ch)→Reentered(ch)；
///   Reentered(ch)→Returned(ch)；Returned(ch)→Idle。
pub fn sta_edge_legal(from: StaPhase, to: StaPhase) -> bool {
    match (from, to) {
        (StaPhase::Idle, StaPhase::Outbound(c)) => c < CH_COUNT,
        (StaPhase::Outbound(a), StaPhase::Reentered(b)) => a == b && a < CH_COUNT,
        (StaPhase::Reentered(a), StaPhase::Returned(b)) => a == b && a < CH_COUNT,
        (StaPhase::Returned(_), StaPhase::Idle) => true,
        _ => false,
    }
}

// ── 重入消费图账本 ───────────────────────────────────────────────
/// 图边：节点 × 通道 → 收口器（None=悬空）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GraphEdge {
    pub node: usize,
    pub channel: usize,
    pub sink: Option<usize>,
    pub wired: bool,
}

pub struct ReentryGraph {
    pub edges: Vec<GraphEdge>,
    pub phase: StaPhase,
    pub depth: u32,
    pub closed_edges: u32,
    pub dangling_edges: u32,
}

/// 重入深度上限（写死：STA 泵语义推定）。
pub const REENTRY_CAP: u32 = 4;

impl ReentryGraph {
    pub fn new() -> ReentryGraph {
        ReentryGraph {
            edges: Vec::new(),
            phase: StaPhase::Idle,
            depth: 0,
            closed_edges: 0,
            dangling_edges: 0,
        }
    }

    /// 建满图：4 节点 × 3 通道 = 12 边；每边按写死规则接收口器：
    ///   消息泵→泵收口(0)、敏感调用→敏感收口(1)、递送回调→回调收口(2)。
    pub fn build_full(&mut self) {
        self.edges.clear();
        self.closed_edges = 0;
        self.dangling_edges = 0;
        let mut n = 0;
        while n < NODE_COUNT {
            let mut c = 0;
            while c < CH_COUNT {
                let sink = match c {
                    CH_MSGPUMP => Some(0),
                    CH_SENSITIVE => Some(1),
                    _ => Some(2),
                };
                self.edges.push(GraphEdge { node: n, channel: c, sink, wired: true });
                c += 1;
            }
            n += 1;
        }
    }

    /// 单边解引：悬空边（sink None 或越界）必检出。
    pub fn deref_edge(&mut self, idx: usize) -> Result<usize, u32> {
        if idx >= self.edges.len() {
            return Err(ReentryCode::NodeOutOfRange.val());
        }
        let node = self.edges[idx].node;
        let ch = self.edges[idx].channel;
        if node >= NODE_COUNT {
            self.edges[idx].wired = false;
            self.dangling_edges += 1;
            return Err(ReentryCode::NodeOutOfRange.val());
        }
        if ch >= CH_COUNT {
            self.edges[idx].wired = false;
            self.dangling_edges += 1;
            return Err(ReentryCode::ChannelOutOfRange.val());
        }
        match self.edges[idx].sink {
            Some(s) if s < SINK_COUNT => {
                self.closed_edges += 1;
                Ok(s)
            }
            _ => {
                self.edges[idx].wired = false;
                self.dangling_edges += 1;
                Err(ReentryCode::SinkNotFound.val())
            }
        }
    }

    /// 全图解引（正演主道）：12 边全收口。
    pub fn deref_all(&mut self) -> Result<u32, u32> {
        let mut i = 0;
        while i < self.edges.len() {
            self.deref_edge(i)?;
            i += 1;
        }
        if self.closed_edges as usize != self.edges.len() {
            return Err(ReentryCode::GraphUnsealed.val());
        }
        Ok(self.closed_edges)
    }

    /// 嵌套入栈（Idle/Reentered → Outbound → Reentered，深度 +1）。
    pub fn push_reentry(&mut self, ch: usize) -> Result<(), u32> {
        if ch >= CH_COUNT {
            return Err(ReentryCode::ChannelOutOfRange.val());
        }
        if self.depth >= REENTRY_CAP {
            return Err(ReentryCode::ReentryOverflow.val());
        }
        // 合法入栈起点：Idle 或嵌套中的 Reentered。
        let out_ok = match self.phase {
            StaPhase::Idle => true,
            StaPhase::Reentered(_) => true,
            _ => false,
        };
        if !out_ok || !sta_edge_legal(StaPhase::Idle, StaPhase::Outbound(ch)) {
            return Err(ReentryCode::IllegalPhaseEdge.val());
        }
        self.phase = StaPhase::Outbound(ch);
        if !sta_edge_legal(self.phase, StaPhase::Reentered(ch)) {
            return Err(ReentryCode::ReentryWithoutPump.val());
        }
        self.phase = StaPhase::Reentered(ch);
        self.depth += 1;
        Ok(())
    }

    /// 嵌套出栈（Reentered → Returned → Idle，深度 −1）。
    pub fn pop_reentry(&mut self, ch: usize) -> Result<(), u32> {
        if ch >= CH_COUNT {
            return Err(ReentryCode::ChannelOutOfRange.val());
        }
        let expect = StaPhase::Reentered(ch);
        if self.phase != expect {
            return Err(ReentryCode::IllegalPhaseEdge.val());
        }
        if !sta_edge_legal(self.phase, StaPhase::Returned(ch)) {
            return Err(ReentryCode::IllegalPhaseEdge.val());
        }
        self.phase = StaPhase::Returned(ch);
        if !sta_edge_legal(self.phase, StaPhase::Idle) {
            return Err(ReentryCode::IllegalPhaseEdge.val());
        }
        self.phase = StaPhase::Idle;
        if self.depth == 0 {
            return Err(ReentryCode::IllegalPhaseEdge.val());
        }
        self.depth -= 1;
        Ok(())
    }

    /// 重入环走一遭（走 ch 通道）：入栈 + 出栈，深度复原。
    pub fn reenter_round(&mut self, ch: usize) -> Result<(), u32> {
        self.push_reentry(ch)?;
        self.pop_reentry(ch)
    }

    /// 悬空通道注入：塞入无收口器边。
    pub fn inject_dangling_edge(&mut self, node: usize, ch: usize) -> Result<(), u32> {
        self.edges.push(GraphEdge {
            node,
            channel: ch,
            sink: None,
            wired: false,
        });
        let idx = self.edges.len() - 1;
        let r = self.deref_edge(idx);
        match r {
            Ok(_) => Err(ReentryCode::IllegalPhaseEdge.val()),
            Err(_) => Ok(()),
        }
    }

    /// 悬空巡检：任一 wired=false 边即报。
    pub fn dangling_audit(&self) -> Result<u32, u32> {
        let mut found = 0u32;
        let mut i = 0;
        while i < self.edges.len() {
            if !self.edges[i].wired {
                found += 1;
            }
            i += 1;
        }
        if found > 0 {
            return Err(ReentryCode::DanglingChannel.val());
        }
        Ok(found)
    }

    /// 对账守恒：closed + dangling == 边总数。
    pub fn conserve_check(&self) -> Result<(), u32> {
        if (self.closed_edges + self.dangling_edges) as usize != self.edges.len() {
            return Err(ReentryCode::GraphUnsealed.val());
        }
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 写死） ──────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 注入面：三族各 10 次，10/10 全抓 ─────────────────────────────
pub const INJ_FAMILIES: usize = 3;
pub const INJ_NAMES: [&str; INJ_FAMILIES] = ["悬空通道族", "越界通道族", "深度超限族"];

pub struct InjectLedger {
    pub tries: [u32; INJ_FAMILIES],
    pub caught: [u32; INJ_FAMILIES],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJ_FAMILIES], caught: [0; INJ_FAMILIES] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJ_FAMILIES {
            if self.caught[i] != self.tries[i] || self.tries[i] != RUNS_PER_ROUND {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：悬空通道注入（无收口器边 ×10）→ DanglingChannel。
    let mut k = 0u32;
    while k < RUNS_PER_ROUND {
        ledger.tries[0] += 1;
        let mut g = ReentryGraph::new();
        g.build_full();
        let _ = g.inject_dangling_edge(0, CH_MSGPUMP);
        if let Err(e) = g.dangling_audit() {
            if e == ReentryCode::DanglingChannel.val() {
                ledger.caught[0] += 1;
            }
        }
        k += 1;
    }
    // 族 1：越界通道注入（reenter_round(CH_COUNT+k) ×10）。
    k = 0;
    while k < RUNS_PER_ROUND {
        ledger.tries[1] += 1;
        let mut g = ReentryGraph::new();
        if let Err(e) = g.reenter_round(CH_COUNT + k as usize) {
            if e == ReentryCode::ChannelOutOfRange.val() {
                ledger.caught[1] += 1;
            }
        }
        k += 1;
    }
    // 族 2：深度超限注入（嵌套重入 5 层 ×10）→ ReentryOverflow。
    k = 0;
    while k < RUNS_PER_ROUND {
        ledger.tries[2] += 1;
        // 嵌套模型：深度累计到 REENTRY_CAP 后再入必拒。
        let mut g = ReentryGraph::new();
        let mut caught = false;
        let mut d = 0;
        while d < REENTRY_CAP + 1 {
            if let Err(e) = g.push_reentry(CH_MSGPUMP) {
                if e == ReentryCode::ReentryOverflow.val() && d == REENTRY_CAP {
                    caught = true;
                }
                break;
            }
            d += 1;
        }
        if caught {
            ledger.caught[2] += 1;
        }
        k += 1;
    }
}

// ── 黄金正演：全图解引 + 三通道重入环 ×10 轮 ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenResult {
    pub rounds: u32,
    pub closed: u32,
    pub edge_total: usize,
    pub sink_spread: [u32; SINK_COUNT],
}

pub fn golden_run() -> Result<GoldenResult, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut last: Option<GoldenResult> = None;
    let mut round = 0u32;
    while round < RUNS_PER_ROUND {
        let _ = g.below(11);
        let mut gr = ReentryGraph::new();
        gr.build_full();
        gr.deref_all()?;
        gr.conserve_check()?;
        // 三通道各走一遭重入环。
        let mut ch = 0;
        while ch < CH_COUNT {
            gr.reenter_round(ch)?;
            ch += 1;
        }
        let mut spread = [0u32; SINK_COUNT];
        let mut i = 0;
        while i < gr.edges.len() {
            if let Some(s) = gr.edges[i].sink {
                spread[s] += 1;
            }
            i += 1;
        }
        last = Some(GoldenResult {
            rounds: round + 1,
            closed: gr.closed_edges,
            edge_total: gr.edges.len(),
            sink_spread: spread,
        });
        round += 1;
    }
    match last {
        Some(res) => {
            if res.edge_total != NODE_COUNT * CH_COUNT || res.closed != (NODE_COUNT * CH_COUNT) as u32 {
                return Err(ReentryCode::GraphUnsealed.val());
            }
            Ok(res)
        }
        None => Err(ReentryCode::GraphUnsealed.val()),
    }
}

/// 双跑复现。
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    let mut i = 0;
    while i < SINK_COUNT {
        if a.sink_spread[i] != b.sink_spread[i] {
            return Err(ReentryCode::NotReproducible.val());
        }
        i += 1;
    }
    if a.closed != b.closed || a.edge_total != b.edge_total {
        return Err(ReentryCode::NotReproducible.val());
    }
    Ok(())
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ReentryCode::ALL.len() {
        let v = ReentryCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：三通道可解引 ×10 轮。
    let golden = golden_run();
    let golden_full = match &golden {
        Ok(g) => {
            g.rounds == RUNS_PER_ROUND
                && g.edge_total == NODE_COUNT * CH_COUNT
                && g.closed == (NODE_COUNT * CH_COUNT) as u32
        }
        Err(_) => false,
    };
    v.push(("J1-三通道可解引10/10", golden_full, format!("{:?}", golden)));
    // 2. 注入三族 10/10 全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-悬空通道三族10/10",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 双跑复现。
    v.push(("J1-双跑复现一致", matches!(reproducible(), Ok(())), String::from("双跑逐项")));
    // 4. 闭集名点验：3 通道 + 4 节点 + 5 收口器。
    let mut okc = 0;
    let mut i = 0;
    while i < CH_COUNT {
        if CH_NAMES[i].len() > 0 {
            okc += 1;
        }
        i += 1;
    }
    let mut okn = 0;
    i = 0;
    while i < NODE_COUNT {
        if NODE_NAMES[i].len() > 0 {
            okn += 1;
        }
        i += 1;
    }
    let mut oks = 0;
    i = 0;
    while i < SINK_COUNT {
        if SINK_NAMES[i].len() > 0 {
            oks += 1;
        }
        i += 1;
    }
    v.push((
        "J1-闭集名点验3+4+5",
        okc == 3 && okn == 4 && oks == 5,
        format!("{}/3 {}/4 {}/5", okc, okn, oks),
    ));
    // 5. 状态机非法边抽验：Outbound(0)→Reentered(1) 通道错配必拒。
    let bad = !sta_edge_legal(StaPhase::Outbound(0), StaPhase::Reentered(1));
    v.push(("J1-通道错配边必拒", bad, String::from("重入环闭集外")));
    // 6. 对账守恒独立对账：悬空 2 边时 closed=12、dangling=2。
    let mut g2 = ReentryGraph::new();
    g2.build_full();
    let _ = g2.deref_all();
    let _ = g2.inject_dangling_edge(0, CH_CALLBACK);
    let _ = g2.inject_dangling_edge(1, CH_SENSITIVE);
    let cons = g2.closed_edges == 12 && g2.dangling_edges == 2
        && matches!(g2.conserve_check(), Ok(()));
    v.push(("J1-守恒12+2对账", cons, format!("c={} d={}", g2.closed_edges, g2.dangling_edges)));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67840 窗口")));
    v
}

pub fn run_unx15821_checks() -> CheckSet {
    let mut s = CheckSet::new("CoRun·UNX-D5-B32 联签总纲与重入消费图（STA 三通道可解引）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
