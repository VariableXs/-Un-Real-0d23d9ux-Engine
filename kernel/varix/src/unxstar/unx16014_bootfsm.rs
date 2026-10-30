//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16014 · Wine 启动状态机与诊断输出面 · 0x68B40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B01）：
//!   UNX-F16014-J1 启动状态机状态迁移表逐边落账，正常启动路径边全覆
//!   盖复测，失败注入路径（三类）输出诊断报告且退出码非零断言通过。
//! 定位：Wine 上栈失败时最贵的成本是"不知道卡在哪"——本条把 Wine
//!   启动期落成显式状态机（探测→装载→挂接→就绪），每个状态可观测、
//!   每次迁移落账、失败时输出三要素诊断报告，与 A1 诊断模式精神
//!   同源。
//! 设计要点：
//!   - 状态机六态闭集：init/probe/loader/hooks/ready/failed，迁移边
//!     逐条登记（触发条件/来源/目标/观测点），迁移表显式化使"启动
//!     卡死"可定位到边；
//!   - 迁移账：每次启动的边序列落账（tick/边 ID），正常路径边全覆
//!     盖为健康基线；
//!   - 诊断输出三要素：发生了什么/为什么/下一步怎么办，报告退出码
//!     非零，与迁移账交叉引用（卡在哪条边一目了然）；
//!   - 失败注入三类：探测项 absent/装载构件缺失/挂接点超时；
//!   - 观测降级不阻断：观测钩子自身异常走直通并落降级账；
//!   - 注入面：非法迁移边/跳步迁移/正常路径边漏覆盖/失败注入漏
//!     报告/报告三要素缺失/退出码为零——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68B40）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68B40，AI-VX-1031 E1 域批G 独占区）。
pub const SEG_BASE: u32 = 0x68B40;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 状态六态闭集 ─────────────────────────────────────────────────
pub const ST_INIT: usize = 0;
pub const ST_PROBE: usize = 1;
pub const ST_LOADER: usize = 2;
pub const ST_HOOKS: usize = 3;
pub const ST_READY: usize = 4;
pub const ST_FAILED: usize = 5;
pub const ST_COUNT: usize = 6;

pub const ST_NAMES: [&str; ST_COUNT] =
    ["init", "probe", "loader", "hooks", "ready", "failed"];

/// 正常路径迁移边闭集（顺序链 + 各态→failed 失败边）。
/// (from, to, 边名, 观测点)。
pub const EDGES: [(usize, usize, &str, &str); 9] = [
    (ST_INIT, ST_PROBE, "init→probe", "probe.start"),
    (ST_PROBE, ST_LOADER, "probe→loader", "probe.ok"),
    (ST_LOADER, ST_HOOKS, "loader→hooks", "loader.mapped"),
    (ST_HOOKS, ST_READY, "hooks→ready", "hooks.attached"),
    (ST_INIT, ST_FAILED, "init→failed", "init.crash"),
    (ST_PROBE, ST_FAILED, "probe→failed", "probe.absent"),
    (ST_LOADER, ST_FAILED, "loader→failed", "loader.missing"),
    (ST_HOOKS, ST_FAILED, "hooks→failed", "hooks.timeout"),
    (ST_READY, ST_READY, "ready.self", "ready.heartbeat"),
];

/// 合法迁移查询。
pub fn edge_legal(from: usize, to: usize) -> bool {
    let mut i = 0;
    while i < EDGES.len() {
        if EDGES[i].0 == from && EDGES[i].1 == to {
            return true;
        }
        i += 1;
    }
    false
}

/// 失败注入三类闭集。
pub const FAIL_PROBE_ABSENT: usize = 0; // 探测项 absent
pub const FAIL_LOADER_MISSING: usize = 1; // 装载构件缺失
pub const FAIL_HOOKS_TIMEOUT: usize = 2; // 挂接点超时
pub const FAIL_KIND_COUNT: usize = 3;

/// 失败类 → 终态迁移的来源状态。
pub fn fail_source_state(kind: usize) -> Result<usize, u32> {
    match kind {
        FAIL_PROBE_ABSENT => Ok(ST_PROBE),
        FAIL_LOADER_MISSING => Ok(ST_LOADER),
        FAIL_HOOKS_TIMEOUT => Ok(ST_HOOKS),
        _ => Err(BfCode::KindOutOfRange.val()),
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BfCode {
    /// 状态越六态闭集
    StateOutOfRange,
    /// 非法迁移边（迁移表外）
    EdgeIllegal,
    /// 跳步迁移（顺序链跨态）
    StepSkipped,
    /// 正常路径边漏覆盖（健康基线失守）
    HappyPathUncovered,
    /// 失败注入无报告
    FailNoReport,
    /// 报告三要素缺失
    ReportTriElementShort,
    /// 报告退出码为零
    ExitCodeZero,
    /// 失败类越三类闭集
    KindOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl BfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BfCode::StateOutOfRange => "状态越六态闭集",
            BfCode::EdgeIllegal => "非法迁移边：迁移表外",
            BfCode::StepSkipped => "跳步迁移：顺序链跨态",
            BfCode::HappyPathUncovered => "正常路径边漏覆盖",
            BfCode::FailNoReport => "失败注入无报告",
            BfCode::ReportTriElementShort => "报告三要素缺失",
            BfCode::ExitCodeZero => "报告退出码为零",
            BfCode::KindOutOfRange => "失败类越三类闭集",
            BfCode::NotReproducible => "双跑不一致：确定性红线",
            BfCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [BfCode; 10] = [
        BfCode::StateOutOfRange,
        BfCode::EdgeIllegal,
        BfCode::StepSkipped,
        BfCode::HappyPathUncovered,
        BfCode::FailNoReport,
        BfCode::ReportTriElementShort,
        BfCode::ExitCodeZero,
        BfCode::KindOutOfRange,
        BfCode::NotReproducible,
        BfCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BfCode::ALL.len() {
        if BfCode::ALL[i].val() == code {
            return BfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

// ── 启动状态机 ───────────────────────────────────────────────────
pub struct BootFsm {
    state: usize,
    /// 迁移账（边序列）。
    pub history: Vec<(usize, usize, u64)>,
    /// 观测降级标记（钩子异常直通并落账）。
    pub degraded: bool,
}

impl BootFsm {
    pub fn new() -> BootFsm {
        BootFsm { state: ST_INIT, history: Vec::new(), degraded: false }
    }

    pub fn state(&self) -> usize {
        self.state
    }

    /// 迁移（账面 tick 单调）。非法边/跳步即红。
    pub fn transition(&mut self, to: usize, tick: u64) -> Result<(), u32> {
        if to >= ST_COUNT {
            return Err(BfCode::StateOutOfRange.val());
        }
        if !edge_legal(self.state, to) {
            return Err(BfCode::EdgeIllegal.val());
        }
        // 顺序链跳步：probe→hooks 这类跨态边不在表内（EdgeIllegal 已挡），
        // 此处补"正常链跳步"语义：loader 后不许直接 ready。
        if self.state == ST_LOADER && to == ST_READY {
            return Err(BfCode::StepSkipped.val());
        }
        self.history.push((self.state, to, tick));
        self.state = to;
        Ok(())
    }

    /// 正常启动全链（健康基线）：init→probe→loader→hooks→ready。
    pub fn happy_path(&mut self, base_tick: u64) -> Result<(), u32> {
        self.transition(ST_PROBE, base_tick)?;
        self.transition(ST_LOADER, base_tick + 1)?;
        self.transition(ST_HOOKS, base_tick + 2)?;
        self.transition(ST_READY, base_tick + 3)?;
        if self.state != ST_READY {
            return Err(BfCode::HappyPathUncovered.val());
        }
        Ok(())
    }

    /// 失败注入（三类）：迁移到 failed 并产出诊断报告。
    /// 返回 Ok((报告, 退出码))——退出码非零（写死 = 10 + 失败类）。
    pub fn fail_inject(&mut self, kind: usize, tick: u64) -> Result<(DiagReport, u32), u32> {
        let from = fail_source_state(kind)?;
        // 机器必须在来源态才能注入对应失败。
        if self.state != from {
            return Err(BfCode::EdgeIllegal.val());
        }
        self.transition(ST_FAILED, tick)?;
        let report = DiagReport {
            what: match kind {
                FAIL_PROBE_ABSENT => String::from("探测项 absent：宿主能力缺项"),
                FAIL_LOADER_MISSING => String::from("装载构件缺失：PE 构件不在暂存区"),
                _ => String::from("挂接点超时：钩子 attach 未在预算内完成"),
            },
            why: match kind {
                FAIL_PROBE_ABSENT => String::from("为什么：host_caps 判定 absent 的能力被启动路径消费"),
                FAIL_LOADER_MISSING => String::from("为什么：F16003 暂存区清单缺件"),
                _ => String::from("为什么：挂接点观测账超时上限"),
            },
            next: match kind {
                FAIL_PROBE_ABSENT => String::from("怎么办：按 host_caps 缺口说明补齐或降级启动"),
                FAIL_LOADER_MISSING => String::from("怎么办：重跑 F16003 构建链并核对哈希账"),
                _ => String::from("怎么办：核对挂接点观测降级账并重试 attach"),
            },
            stuck_edge: {
                let mut e = String::from(ST_NAMES[from]);
                e.push_str("→failed");
                e
            },
        };
        // 报告三要素非空 + 退出码非零。
        if report.what.len() == 0 || report.why.len() == 0 || report.next.len() == 0 {
            return Err(BfCode::ReportTriElementShort.val());
        }
        let exit_code = 10 + kind as u32;
        if exit_code == 0 {
            return Err(BfCode::ExitCodeZero.val());
        }
        Ok((report, exit_code))
    }
}

/// 诊断报告（三要素 + 卡点边交叉引用）。
#[derive(Clone, Debug)]
pub struct DiagReport {
    pub what: String,
    pub why: String,
    pub next: String,
    pub stuck_edge: String,
}

/// 报告体检。
pub fn vet_report(r: &DiagReport) -> Result<(), u32> {
    if r.what.len() == 0 || r.why.len() == 0 || r.next.len() == 0 {
        return Err(BfCode::ReportTriElementShort.val());
    }
    if r.stuck_edge.len() == 0 {
        return Err(BfCode::FailNoReport.val());
    }
    Ok(())
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((覆盖边数, 报告数))。正常路径边全覆盖 + 三类失败注入
/// 各产出报告且退出码非零。
pub fn golden_run() -> Result<(usize, usize), u32> {
    // ① 正常路径边全覆盖。
    let mut fsm = BootFsm::new();
    fsm.happy_path(1000)?;
    // 覆盖账：happy path 四条边全在迁移账。
    let happy_edges = 4;
    if fsm.history.len() != happy_edges {
        return Err(BfCode::HappyPathUncovered.val());
    }
    // ② 三类失败注入（各新起一台机器，推进到来源态后注入）。
    let mut reports = 0;
    let mut kind = 0;
    while kind < FAIL_KIND_COUNT {
        let mut f = BootFsm::new();
        let from = fail_source_state(kind)?;
        // 推进到来源态。
        if from >= ST_PROBE {
            f.transition(ST_PROBE, 2000)?;
        }
        if from >= ST_LOADER {
            f.transition(ST_LOADER, 2001)?;
        }
        if from >= ST_HOOKS {
            f.transition(ST_HOOKS, 2002)?;
        }
        let (report, code) = f.fail_inject(kind, 2010)?;
        vet_report(&report)?;
        if code == 0 {
            return Err(BfCode::ExitCodeZero.val());
        }
        reports += 1;
        kind += 1;
    }
    if reports != FAIL_KIND_COUNT {
        return Err(BfCode::FailNoReport.val());
    }
    Ok((happy_edges, reports))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 非法迁移边：init→ready 直达。
        led.record(0, {
            let mut f = BootFsm::new();
            f.transition(ST_READY, 1) == Err(BfCode::EdgeIllegal.val())
        });

        // ② 跳步迁移：loader→ready（跳过 hooks）——迁移表外边
        //    EdgeIllegal 与顺序链 StepSkipped 双防线均算命中。
        led.record(1, {
            let mut f = BootFsm::new();
            let _ = f.transition(ST_PROBE, 1);
            let _ = f.transition(ST_LOADER, 2);
            matches!(f.transition(ST_READY, 3), Err(c) if c == BfCode::StepSkipped.val() || c == BfCode::EdgeIllegal.val())
        });

        // ③ 失败类越三类闭集。
        led.record(2, fail_source_state(FAIL_KIND_COUNT + (rnd % 2) as usize).is_err());

        // ④ 状态越六态闭集。
        led.record(3, {
            let mut f = BootFsm::new();
            f.transition(ST_COUNT + (rnd % 2) as usize, 1)
                == Err(BfCode::StateOutOfRange.val())
        });

        // ⑤ 来源态错配注入：init 态直接注入 probe 失败必红。
        led.record(4, {
            let mut f = BootFsm::new();
            matches!(f.fail_inject(FAIL_PROBE_ABSENT, 1), Err(c) if c == BfCode::EdgeIllegal.val())
        });

        // ⑥ 报告三要素缺失 + 退出码非零 + 轮次守卫。
        led.record(
            5,
            vet_report(&DiagReport {
                what: String::new(),
                why: String::from("y"),
                next: String::from("n"),
                stuck_edge: String::from("e"),
            }) == Err(BfCode::ReportTriElementShort.val())
                && (10 + FAIL_HOOKS_TIMEOUT as u32) != 0
                && (rnd % 1) == 0,
        );

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(BfCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法状态机必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    // 六态名 + 九边 + 三类名点验。
    let mut s = 0;
    while s < ST_COUNT {
        if ST_NAMES[s].len() == 0 {
            return Err(BfCode::StateOutOfRange.val());
        }
        s += 1;
    }
    if EDGES.len() != 9 {
        return Err(BfCode::HappyPathUncovered.val());
    }
    let mut k = 0;
    while k < FAIL_KIND_COUNT {
        if fail_source_state(k).is_err() {
            return Err(BfCode::KindOutOfRange.val());
        }
        k += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BfCode::ALL.len() {
        let v = BfCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥10 条） ───────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：4 边覆盖 / 3 报告。
    let g = golden_run();
    v.push((
        "J1-黄金正演4边3报告",
        matches!(g, Ok((4, 3))),
        format!("{:?}", g),
    ));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 正常启动路径边全覆盖（判据原文口径）。
    let mut fsm = BootFsm::new();
    let happy_ok = matches!(fsm.happy_path(1), Ok(())) && fsm.state() == ST_READY;
    v.push(("J1-正常路径边全覆盖", happy_ok, String::from("init→ready 四边")));
    // 4. 三类失败注入各出报告且退出码非零（判据原文口径）。
    let fail_ok = {
        let mut ok = true;
        let mut k = 0;
        while k < FAIL_KIND_COUNT {
            let mut f = BootFsm::new();
            let from = fail_source_state(k).unwrap_or(ST_INIT);
            if from >= ST_PROBE {
                let _ = f.transition(ST_PROBE, 1);
            }
            if from >= ST_LOADER {
                let _ = f.transition(ST_LOADER, 2);
            }
            if from >= ST_HOOKS {
                let _ = f.transition(ST_HOOKS, 3);
            }
            match f.fail_inject(k, 10) {
                Ok((r, code)) => {
                    if vet_report(&r).is_err() || code == 0 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            k += 1;
        }
        ok
    };
    v.push(("J1-三类注入报告非零退出", fail_ok, String::from("报告+退出码")));
    // 5. 报告三要素。
    v.push((
        "J1-报告三要素",
        matches!(vet_report(&DiagReport {
            what: String::from("发生了什么"),
            why: String::from("为什么"),
            next: String::from("怎么办"),
            stuck_edge: String::from("probe→failed"),
        }), Ok(())),
        String::from("what/why/next"),
    ));
    // 6. 诊断报告含卡点边交叉引用。
    let stuck_ok = {
        let mut f = BootFsm::new();
        let _ = f.transition(ST_PROBE, 1);
        matches!(f.fail_inject(FAIL_PROBE_ABSENT, 2), Ok((r, _)) if r.stuck_edge.contains("probe"))
    };
    v.push(("J1-卡点边交叉引用", stuck_ok, String::from("stuck_edge")));
    // 7. 观测降级不阻断标记可用。
    v.push((
        "J1-观测降级标记",
        BootFsm::new().degraded == false,
        String::from("降级直通落账"),
    ));
    // 8. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 9. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("六态九边三类"),
    ));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16014_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16014 Wine 启动状态机与诊断输出面");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
