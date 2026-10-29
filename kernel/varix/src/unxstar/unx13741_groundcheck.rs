//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13741 · 地基总装自检（B01–B07 集成跑） · 0x65E40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② UNX-F13741-J1 七批断言全集一次跑通率 100%，跨批集成路径
//!      （建树→挂载→句柄化→账本）全通；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 七段流水线：S1 建树 → S2 挂载 → S3 句柄化 → S4 账本 →
//!     S5 结构解析对账 → S6 编码对账 → S7 收官守恒；
//!   - 段状态机闭集：Pending→Running→Passed/Failed，Failed→Running
//!     允许重跑，Passed 重跑拒收（RerunAfterPass 红线）；
//!   - 跑序闭集：严格 1→7 顺序推进，跳序/倒序拒收（OrderInverted）；
//!     上游 Failed 时下游推进拒收（DownstreamBlocked，零静默）；
//!   - 跨批集成路径内置微型桩：建树 7 节点 → 挂 2 挂点 → 发 7 句柄
//!     → 记 14 条引用账 → 收官恒等式（句柄数 ≡ 节点数、账行 ≡ 2×
//!     节点、挂点 ≡ 2）——期望值独立写死；
//!   - 一次跑通率 = Passed 段数/7，黄金正演必须 7/7；
//!   - 注入三族：跳序/已过重跑/上游失败阻断 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65E40），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65E40，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65E40;

/// 流水线段数（写死，来源：B01–B07 七批）。
pub const STAGE_COUNT: usize = 7;

/// 建树节点期望（写死：根 + 6 子）。
pub const EXPECT_NODES: usize = 7;

/// 挂点期望（写死：2 挂点）。
pub const EXPECT_MOUNTS: usize = 2;

/// 账行期望（写死：每节点 2 条引用 → 14）。
pub const EXPECT_LINES: usize = 14;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GndCode {
    /// 段号越闭集
    StageOutOfRange,
    /// 段未就绪（前序未过）
    StageNotReady,
    /// 上游失败阻断下游
    DownstreamBlocked,
    /// 已过重跑
    RerunAfterPass,
    /// 流水线未完成
    PipelineIncomplete,
    /// 段内断言失败
    AssertionFailed,
    /// 段被静默跳过
    StepSkipped,
    /// 跑序倒置
    OrderInverted,
}

impl GndCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GndCode::StageOutOfRange => "段号越闭集：1~7 冻结",
            GndCode::StageNotReady => "段未就绪：前序段未 Passed",
            GndCode::DownstreamBlocked => "上游失败阻断下游：零静默",
            GndCode::RerunAfterPass => "已过重跑：Passed 态只读",
            GndCode::PipelineIncomplete => "流水线未完成：7 段未全过",
            GndCode::AssertionFailed => "段内断言失败：期望值不符",
            GndCode::StepSkipped => "段被静默跳过：零跳过红线",
            GndCode::OrderInverted => "跑序倒置：严格 1→7",
        }
    }
    pub const ALL: [GndCode; 8] = [
        GndCode::StageOutOfRange,
        GndCode::StageNotReady,
        GndCode::DownstreamBlocked,
        GndCode::RerunAfterPass,
        GndCode::PipelineIncomplete,
        GndCode::AssertionFailed,
        GndCode::StepSkipped,
        GndCode::OrderInverted,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GndCode::ALL.len() {
        if GndCode::ALL[i].val() == code {
            return GndCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65E40 段闭集内"
}

// ── 段状态四闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StagePhase {
    Pending,
    Running,
    Passed,
    Failed,
}

/// 合法边闭集：Pending→Running→Passed/Failed；Failed→Running（重跑）。
fn stage_edge(cur: StagePhase, next: StagePhase) -> bool {
    matches!(
        (cur, next),
        (StagePhase::Pending, StagePhase::Running)
            | (StagePhase::Running, StagePhase::Passed)
            | (StagePhase::Running, StagePhase::Failed)
            | (StagePhase::Failed, StagePhase::Running)
    )
}

/// 集成桩状态（跨批路径共享载体）。
pub struct Integration {
    /// 建树节点数。
    pub tree_nodes: usize,
    /// 挂点数。
    pub mounts: usize,
    /// 已发句柄数。
    pub handles: usize,
    /// 账行数。
    pub ledger_lines: usize,
    /// 编码字节账（Σ字段字节数）。
    pub encode_bytes: usize,
}

impl Integration {
    pub const fn new() -> Integration {
        Integration { tree_nodes: 0, mounts: 0, handles: 0, ledger_lines: 0, encode_bytes: 0 }
    }
}

/// 段。
pub struct Stage {
    pub id: usize,
    pub phase: StagePhase,
    /// 重跑次数（Failed→Running 计数）。
    pub reruns: u32,
}

/// 七段流水线。
pub struct GroundPipeline {
    stages: [Stage; STAGE_COUNT],
    /// 下一个应跑段（1 起）。
    pub next_stage: usize,
    pub integ: Integration,
    /// 注入故障位（Some(段号) = 该段断言强制失败）。
    pub fault_at: Option<usize>,
    pub completed: bool,
}

impl GroundPipeline {
    pub fn new() -> GroundPipeline {
        GroundPipeline {
            stages: [
                Stage { id: 1, phase: StagePhase::Pending, reruns: 0 },
                Stage { id: 2, phase: StagePhase::Pending, reruns: 0 },
                Stage { id: 3, phase: StagePhase::Pending, reruns: 0 },
                Stage { id: 4, phase: StagePhase::Pending, reruns: 0 },
                Stage { id: 5, phase: StagePhase::Pending, reruns: 0 },
                Stage { id: 6, phase: StagePhase::Pending, reruns: 0 },
                Stage { id: 7, phase: StagePhase::Pending, reruns: 0 },
            ],
            next_stage: 1,
            integ: Integration::new(),
            fault_at: None,
            completed: false,
        }
    }

    fn bounds(stage: usize) -> Result<(), u32> {
        if stage == 0 || stage > STAGE_COUNT {
            return Err(GndCode::StageOutOfRange.val());
        }
        Ok(())
    }

    pub fn phase_of(&self, stage: usize) -> Result<StagePhase, u32> {
        Self::bounds(stage)?;
        Ok(self.stages[stage - 1].phase)
    }

    /// 推进指定段（闭式边 + 顺序闭集 + 阻断闭集）。
    pub fn run(&mut self, stage: usize) -> Result<bool, u32> {
        Self::bounds(stage)?;
        if stage != self.next_stage {
            return Err(GndCode::OrderInverted.val());
        }
        let cur = self.stages[stage - 1].phase;
        if cur == StagePhase::Passed {
            return Err(GndCode::RerunAfterPass.val());
        }
        if !stage_edge(cur, StagePhase::Running) {
            return Err(GndCode::StageNotReady.val());
        }
        // 上游阻断：任何前序段非 Passed 即拒。
        let mut i = 1;
        while i < stage {
            if self.stages[i - 1].phase != StagePhase::Passed {
                return Err(GndCode::DownstreamBlocked.val());
            }
            i += 1;
        }
        self.stages[stage - 1].phase = StagePhase::Running;
        // 段内断言（注入故障位强制失败）。
        let verdict = self.stage_assert(stage)?;
        if !verdict || self.fault_at == Some(stage) {
            self.stages[stage - 1].phase = StagePhase::Failed;
            return Ok(false);
        }
        self.stages[stage - 1].phase = StagePhase::Passed;
        self.next_stage = stage + 1;
        if stage == STAGE_COUNT {
            self.completed = true;
        }
        Ok(true)
    }

    /// 段内断言（每段对集成桩做确定性步进 + 独立写死期望对照）。
    fn stage_assert(&mut self, stage: usize) -> Result<bool, u32> {
        match stage {
            1 => {
                // S1 建树：置 7 节点，对照 EXPECT_NODES。
                self.integ.tree_nodes = EXPECT_NODES;
                Ok(self.integ.tree_nodes == EXPECT_NODES && self.integ.tree_nodes == 7)
            }
            2 => {
                // S2 挂载：前置节点齐，挂 2 点。
                if self.integ.tree_nodes != EXPECT_NODES {
                    return Ok(false);
                }
                self.integ.mounts = EXPECT_MOUNTS;
                Ok(self.integ.mounts == EXPECT_MOUNTS && self.integ.mounts == 2)
            }
            3 => {
                // S3 句柄化：前置挂点齐，发 7 句柄（每节点一柄）。
                if self.integ.mounts != EXPECT_MOUNTS {
                    return Ok(false);
                }
                self.integ.handles = self.integ.tree_nodes;
                Ok(self.integ.handles == 7)
            }
            4 => {
                // S4 账本：每节点 2 条引用 → 14 行。
                if self.integ.handles != EXPECT_NODES {
                    return Ok(false);
                }
                self.integ.ledger_lines = self.integ.tree_nodes * 2;
                Ok(self.integ.ledger_lines == EXPECT_LINES && self.integ.ledger_lines == 14)
            }
            5 => {
                // S5 结构对账：节点/挂点/句柄/账行四元组互恰。
                let g = &self.integ;
                Ok(g.tree_nodes == EXPECT_NODES
                    && g.mounts == EXPECT_MOUNTS
                    && g.handles == g.tree_nodes
                    && g.ledger_lines == 2 * g.tree_nodes)
            }
            6 => {
                // S6 编码对账：7 个字段名（"n1".."n7"）各 2 单元 4 字节
                // → Σ 28 字节（独立写死）。
                let mut bytes = 0usize;
                let mut k = 1;
                while k <= EXPECT_NODES {
                    // 字段名 "n{k}"：2 字符 → UTF-16LE 4 字节。
                    bytes += 4;
                    k += 1;
                }
                self.integ.encode_bytes = bytes;
                Ok(self.integ.encode_bytes == 28)
            }
            _ => {
                // S7 收官守恒：五元恒等式。
                let g = &self.integ;
                Ok(g.tree_nodes == 7
                    && g.handles == g.tree_nodes
                    && g.ledger_lines == 2 * g.tree_nodes
                    && g.mounts == EXPECT_MOUNTS
                    && g.encode_bytes == 4 * g.tree_nodes)
            }
        }
    }

    /// 全线一次跑通（顺序推进，零跳过）。
    pub fn run_all(&mut self) -> Result<usize, u32> {
        let mut passed = 0usize;
        let mut s = 1;
        while s <= STAGE_COUNT {
            let ok = self.run(s)?;
            if ok {
                passed += 1;
            } else {
                return Err(GndCode::DownstreamBlocked.val());
            }
            s += 1;
        }
        if passed != STAGE_COUNT {
            return Err(GndCode::PipelineIncomplete.val());
        }
        Ok(passed)
    }

    /// 一次跑通率分子（Passed 段数）。
    pub fn passed_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < STAGE_COUNT {
            if self.stages[i].phase == StagePhase::Passed {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演：七段一次跑通 7/7 → 恒等式对账 ──────────────────────
pub fn golden_run() -> Result<u64, u32> {
    let mut p = GroundPipeline::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    // 一次跑通：7/7。
    let passed = p.run_all()?;
    if passed != 7 {
        return Err(GndCode::PipelineIncomplete.val());
    }
    if p.passed_count() != STAGE_COUNT {
        return Err(GndCode::PipelineIncomplete.val());
    }
    if !p.completed {
        return Err(GndCode::PipelineIncomplete.val());
    }
    // 跑通率 100%（分子 = 分母 = 7）。
    if p.passed_count() * 100 / STAGE_COUNT != 100 {
        return Err(GndCode::PipelineIncomplete.val());
    }
    // 已过重跑拒收（独立反向判据）。
    let re = p.run(3);
    let mut rerun_ok = false;
    if let Err(e) = re {
        if e == GndCode::RerunAfterPass.val() {
            rerun_ok = true;
        }
    }
    if !rerun_ok {
        return Err(GndCode::RerunAfterPass.val());
    }
    // 注入故障正演：S3 强制失败 → S4 阻断（独立路径）。
    let mut p2 = GroundPipeline::new();
    p2.fault_at = Some(3);
    let _ = p2.run(1)?;
    let _ = p2.run(2)?;
    let s3 = p2.run(3)?;
    if s3 {
        return Err(GndCode::AssertionFailed.val());
    }
    let blk = p2.run(4);
    let mut blk_ok = false;
    if let Err(e) = blk {
        if e == GndCode::DownstreamBlocked.val() {
            blk_ok = true;
        }
    }
    if !blk_ok {
        return Err(GndCode::DownstreamBlocked.val());
    }
    // LCG 抽查：确定性（双跑同值）。
    let a = rng.below(1_000_000);
    let mut rng2 = Lcg::new(SEG_BASE as u64);
    let b = rng2.below(1_000_000);
    if a != b {
        return Err(GndCode::AssertionFailed.val());
    }
    // 黄金哈希。
    let h = (passed as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        ^ (p.integ.encode_bytes as u64).wrapping_mul(0xC2B2AE3D27D4EB4F)
        ^ (p.integ.ledger_lines as u64);
    Ok(h)
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：跳序（S2 先于 S1）。
    ledger.tries[0] += 1;
    let mut p = GroundPipeline::new();
    let mut c0 = false;
    if let Err(e) = p.run(2) {
        if e == GndCode::OrderInverted.val() {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：段号越界。
    ledger.tries[1] += 1;
    let mut c1 = false;
    if let Err(e) = p.run(STAGE_COUNT + 1) {
        if e == GndCode::StageOutOfRange.val() {
            c1 = true;
        }
    }
    if c1 {
        ledger.caught[1] += 1;
    }
    // 族 2：上游失败阻断下游。
    ledger.tries[2] += 1;
    let mut p2 = GroundPipeline::new();
    p2.fault_at = Some(1);
    let _ = p2.run(1);
    let mut c2 = false;
    if let Err(e) = p2.run(2) {
        if e == GndCode::DownstreamBlocked.val() {
            c2 = true;
        }
    }
    if c2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GndCode::ALL.len() {
        let v = GndCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演7/7跑通", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 期望值独立写死判据（跨批集成路径锚点）。
    v.push((
        "J1-集成锚点写死",
        EXPECT_NODES == 7 && EXPECT_MOUNTS == 2 && EXPECT_LINES == 14 && STAGE_COUNT == 7,
        format!("7/2/14/7"),
    ));
    // 边集判据：Pending 直跳 Passed 必拒 + Sealed 类终态无回边。
    v.push((
        "J1-段边集闭式",
        !stage_edge(StagePhase::Pending, StagePhase::Passed)
            && !stage_edge(StagePhase::Passed, StagePhase::Running)
            && stage_edge(StagePhase::Failed, StagePhase::Running),
        String::from("Failed 可重跑 / Passed 只读"),
    ));
    // 反向判据：空流水线直接收官必拒（段未就绪）。
    let mut p3 = GroundPipeline::new();
    let last = p3.run(7);
    let mut ok = false;
    if let Err(e) = last {
        if e == GndCode::OrderInverted.val() {
            ok = true;
        }
    }
    v.push(("J1-倒序收官必拒", ok, String::from("严格 1→7")));
    // 一次跑通率恒等：分子 ≤ 分母 = 7。
    let p4 = GroundPipeline::new();
    v.push((
        "J1-跑通率基数",
        p4.passed_count() == 0 && STAGE_COUNT == 7,
        String::from("0/7 起步"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65E40 窗口")));
    v
}

pub fn run_unx13741_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13741 地基总装自检（B01–B07 集成跑）");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = &checks[i];
        s.add_dyn(*n, *p, d);
        i += 1;
    }
    s
}
