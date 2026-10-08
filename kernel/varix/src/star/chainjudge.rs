//! F070 性能域总判据 · 完整设计（STAR I 主册 G-B-30）。
//!
//! **判据（主册）**：整链场景定义文档化（50 件的打开/操作/退出脚本化）；
//! 80fps 达标定义 = P95 帧耗时 ≤12.5ms 且 P99 ≤16.6ms；成绩单两份证据
//! （录屏 + 账本导出）齐备。
//!
//! **设计要点（主册）**：
//! - 80fps 整链验收制度：B 域四层（合成/启动/IO/调度）每层独立判据全绿
//!   后，整链在「常用 50 件」（F040）真实负载下复测 80fps——**单层绿
//!   不算数，整链绿才交付**；
//! - 达标线与红线分离：80fps 目标 / 60fps 底线（P95 ≤16.6ms 帧间隔线）
//!   ——底线破即回炉（Red），目标破即优化不停（Amber）；
//! - 整链复测中有应用拖垮帧率 → 该应用入「攻坚名单」单独优化（**不降低
//!   总验收标准**——名单只记账，verdict 纯按整链分位）；
//! - 借力件升级后整链复测重跑（**版本漂移防护**：build_tag 变更 → 旧帧
//!   清零、整链从头）；
//! - 整链测试机锁定 Y7000（可比性）；测试环境温度记录（热节流干扰排除）；
//! - 成绩单版本化（季度快照）；证据链引用各单项判据记录（不复制）；
//! - 诊断中心「性能域状态」页 30 项判据红绿一览（F041-F070——AI-K1 的
//!   17 项以显式注入承接，本域 13 项直连真值）。
//!
//! 分位口径唯一：整链 P95/P99 = **全帧集合**（50 件全部帧合并）算分位，
//! 不是单件分位的平均（口径在自检中以反例钉死）。分位工具复用 sbase
//! `pct_near`。无外部依赖，帧耗时序列注入式（宿主测试确定复现）。

use crate::checks::CheckSet;
use crate::star::sbase::pct_near;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（判线唯一源）
// ---------------------------------------------------------------------------

/// 80fps 目标：P95 帧耗时 ≤12.5ms。
pub const TARGET_P95_US: u64 = 12_500;

/// 80fps 目标：P99 帧耗时 ≤16.6ms。
pub const TARGET_P99_US: u64 = 16_600;

/// 60fps 底线（红线）：P95 帧耗时 ≤16.6ms——破即回炉。
pub const FLOOR_P95_US: u64 = 16_600;

/// 整链测试机锁定（可比性）。
pub const LOCKED_MACHINE: &str = "Y7000";

/// 整链负载件数（F040 常用 50 件）。
pub const SCENARIO_APP_COUNT: usize = 50;

/// B 域判据一览容量（F041-F070 恰 30 项）。
pub const BOARD_CAP: usize = 30;

// ---------------------------------------------------------------------------
// 判据一览（30 项红绿一览）
// ---------------------------------------------------------------------------

/// B 域四层（整链验收的分层门）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// 合成层（帧率/合成器/资产）。
    Synth,
    /// 启动层（预读/冷热/二次启动）。
    Boot,
    /// IO 层（写合并/U 盘/日志）。
    Io,
    /// 调度层（频率/内存/调度预算）。
    Sched,
}

/// 四层全表（一览遍历序）。
pub const ALL_LAYERS: [Layer; 4] = [Layer::Synth, Layer::Boot, Layer::Io, Layer::Sched];

impl Layer {
    /// 层名（红绿一览列头）。
    pub fn name(self) -> &'static str {
        match self {
            Layer::Synth => "synth",
            Layer::Boot => "boot",
            Layer::Io => "io",
            Layer::Sched => "sched",
        }
    }
}

/// 单项判据登记（一览单元格）。
#[derive(Clone, Copy, Debug)]
pub struct Judgement {
    pub fid: &'static str,
    pub layer: Layer,
    pub passed: bool,
    /// 证据链引用（指向单项判据记录——不复制数据）。
    pub evidence: &'static str,
}

/// 30 项判据红绿一览（F041-F070）。
pub struct JudgeBoard {
    items: Vec<Judgement>,
}

impl JudgeBoard {
    pub fn new() -> JudgeBoard {
        JudgeBoard { items: Vec::new() }
    }

    /// 登记（重复 fid 拒绝；超 30 项拒绝——F041-F070 恰满）。
    pub fn register(&mut self, j: Judgement) -> bool {
        if self.items.len() >= BOARD_CAP || self.items.iter().any(|x| x.fid == j.fid) {
            return false;
        }
        self.items.push(j);
        true
    }

    /// 单层绿：该层有登记且全绿（空层不算绿——未验收不等于通过）。
    pub fn layer_green(&self, layer: Layer) -> bool {
        let mut any = false;
        for it in &self.items {
            if it.layer == layer {
                if !it.passed {
                    return false;
                }
                any = true;
            }
        }
        any
    }

    /// 四层全绿——整链运行的门（单层绿不算数）。
    pub fn four_layers_green(&self) -> bool {
        ALL_LAYERS.iter().all(|l| self.layer_green(*l))
    }

    /// 红绿一览直读（诊断中心「性能域状态」页数据源）。
    pub fn view(&self) -> Vec<Judgement> {
        self.items.clone()
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// 层内登记数（一览分布核对）。
    pub fn layer_count(&self, layer: Layer) -> usize {
        self.items.iter().filter(|x| x.layer == layer).count()
    }
}

// ---------------------------------------------------------------------------
// 整链复测（50 件场景）
// ---------------------------------------------------------------------------

/// 「常用 50 件」（F040 五类 × 10 件）整链负载名表——唯一名表，名字唯一。
pub const SCENARIO_APPS: [&str; 50] = [
    "edit-00", "edit-01", "edit-02", "edit-03", "edit-04", "edit-05", "edit-06", "edit-07", "edit-08", "edit-09",
    "arch-00", "arch-01", "arch-02", "arch-03", "arch-04", "arch-05", "arch-06", "arch-07", "arch-08", "arch-09",
    "image-00", "image-01", "image-02", "image-03", "image-04", "image-05", "image-06", "image-07", "image-08", "image-09",
    "term-00", "term-01", "term-02", "term-03", "term-04", "term-05", "term-06", "term-07", "term-08", "term-09",
    "dev-00", "dev-01", "dev-02", "dev-03", "dev-04", "dev-05", "dev-06", "dev-07", "dev-08", "dev-09",
];

/// 攻坚名单条目（拖垮帧率的件——单独优化，不降总验收）。
#[derive(Clone, Copy, Debug)]
pub struct HeavyApp {
    pub app: &'static str,
    pub p95_us: u64,
    pub p99_us: u64,
}

/// 三档结论（达标线与红线分离）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 达标：P95 ≤12.5ms 且 P99 ≤16.6ms（80fps 目标全中）。
    Green,
    /// 目标破、底线守：优化不停（不回炉）。
    Amber,
    /// 底线破（P95 >16.6ms）：回炉。
    Red,
}

/// verdict 唯一裁决（口径一处一事实）。
pub fn verdict_of(p95_us: u64, p99_us: u64) -> Verdict {
    if p95_us > FLOOR_P95_US {
        Verdict::Red
    } else if p95_us <= TARGET_P95_US && p99_us <= TARGET_P99_US {
        Verdict::Green
    } else {
        Verdict::Amber
    }
}

/// 成绩单（版本化季度快照）。
#[derive(Clone, Debug)]
pub struct ScoreCard {
    /// 测试机（恒锁定 Y7000——可比性）。
    pub machine: &'static str,
    /// 环境温度（热节流干扰排除；None = 未采集，如实留空）。
    pub temp_c: Option<i32>,
    /// 构建指纹（版本漂移防护锚）。
    pub build_tag: &'static str,
    /// 成绩单版本（季度快照递增）。
    pub suite_version: u32,
    /// 整链 P95（全帧集合口径）。
    pub p95_us: u64,
    /// 整链 P99（全帧集合口径）。
    pub p99_us: u64,
    pub verdict: Verdict,
    /// 攻坚名单（超标件——只记账不降标准）。
    pub heavy: Vec<HeavyApp>,
    /// 证据一：录屏（引用名）。
    pub evidence_screen: &'static str,
    /// 证据二：账本导出（引用名）。
    pub evidence_ledger: &'static str,
}

/// 开跑结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeginOutcome {
    /// 开跑（版本续测或首跑）。
    Started,
    /// 四层未全绿——单层绿不算数，整链不开跑。
    LayersNotGreen,
}

/// 出单结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalizeOutcome {
    /// 成绩单出具。
    Issued,
    /// 证据两件不齐备——成绩单不成立。
    EvidenceMissing,
    /// 50 件未跑满（版本漂移后半途出单——拒绝）。
    Incomplete,
}

/// 整链复测执行器。
pub struct ChainRunner {
    /// 整链全帧集合（整链分位唯一源——全帧口径）。
    frames: Vec<u64>,
    /// 单件档案（app, p95_us, p99_us）。
    per_app: Vec<(&'static str, u64, u64)>,
    /// 攻坚名单。
    heavy: Vec<HeavyApp>,
    /// 版本漂移锚。
    last_build: Option<&'static str>,
    /// 漂移后须整链重跑标记。
    rerun_needed: bool,
    /// 成绩单版本（季度快照）。
    suite_version: u32,
}

impl ChainRunner {
    pub fn new() -> ChainRunner {
        ChainRunner {
            frames: Vec::new(),
            per_app: Vec::new(),
            heavy: Vec::new(),
            last_build: None,
            rerun_needed: false,
            suite_version: 0,
        }
    }

    /// 开跑：门（四层全绿）+ 版本漂移防护（build_tag 变更 → 旧帧清零）。
    pub fn begin_run(&mut self, board: &JudgeBoard, build_tag: &'static str) -> BeginOutcome {
        if !board.four_layers_green() {
            return BeginOutcome::LayersNotGreen;
        }
        match self.last_build {
            Some(prev) if prev != build_tag => {
                // 版本漂移：借力件升级窗过后整链必复测——旧数据全部作废。
                self.frames.clear();
                self.per_app.clear();
                self.heavy.clear();
                self.rerun_needed = true;
            }
            Some(_) => {}
            None => {}
        }
        self.last_build = Some(build_tag);
        BeginOutcome::Started
    }

    /// 跑一件（打开/操作/退出的帧耗时序列注入）：
    /// 帧并入整链集合 + 单件档案 + 超标入攻坚名单。
    /// 空帧序列拒绝（false——不产假档案）。
    pub fn run_app(&mut self, app: &'static str, frames: &[u64]) -> bool {
        if frames.is_empty() || !SCENARIO_APPS.iter().any(|a| *a == app) {
            return false;
        }
        if self.per_app.iter().any(|(a, _, _)| *a == app) {
            return false; // 同版本内重复跑一件 = 场景脚本缺陷，拒绝。
        }
        let p95 = pct_near(frames, 95);
        let p99 = pct_near(frames, 99);
        if p95 > TARGET_P95_US || p99 > TARGET_P99_US {
            self.heavy.push(HeavyApp { app, p95_us: p95, p99_us: p99 });
        }
        self.per_app.push((app, p95, p99));
        for f in frames {
            self.frames.push(*f);
        }
        true
    }

    /// 出单：证据齐备门 + 50 件跑满门 + 全帧分位 + 三档结论。
    pub fn finalize(
        &mut self,
        temp_c: Option<i32>,
        evidence_screen: &'static str,
        evidence_ledger: &'static str,
    ) -> (FinalizeOutcome, Option<ScoreCard>) {
        if evidence_screen.is_empty() || evidence_ledger.is_empty() {
            return (FinalizeOutcome::EvidenceMissing, None);
        }
        if self.per_app.len() < SCENARIO_APP_COUNT {
            return (FinalizeOutcome::Incomplete, None);
        }
        let p95 = pct_near(&self.frames, 95);
        let p99 = pct_near(&self.frames, 99);
        self.suite_version += 1;
        self.rerun_needed = false;
        let card = ScoreCard {
            machine: LOCKED_MACHINE,
            temp_c,
            build_tag: self.last_build.unwrap_or(""),
            suite_version: self.suite_version,
            p95_us: p95,
            p99_us: p99,
            verdict: verdict_of(p95, p99),
            heavy: self.heavy.clone(),
            evidence_screen,
            evidence_ledger,
        };
        (FinalizeOutcome::Issued, Some(card))
    }

    /// 已跑件数（Incomplete 诊断直读）。
    pub fn apps_done(&self) -> usize {
        self.per_app.len()
    }

    /// 版本漂移须重跑标记。
    pub fn rerun_needed(&self) -> bool {
        self.rerun_needed
    }

    /// 攻坚名单直读。
    pub fn heavy_list(&self) -> Vec<HeavyApp> {
        self.heavy.clone()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F070 自检（判据：P95 ≤12.5ms 且 P99 ≤16.6ms；两份证据齐备）。
pub fn run_chainjudge_checks() -> CheckSet {
    let mut set = CheckSet::new("F070-chainjudge");

    // 1. 一览容量恰 30：第 31 项拒绝；重复 fid 拒绝。
    let mut board = sample_board(false);
    assert!(board.count() == 30);
    let dup = Judgement { fid: "F041", layer: Layer::Synth, passed: true, evidence: "dup" };
    let over = Judgement { fid: "F071", layer: Layer::Sched, passed: true, evidence: "over" };
    set.add(
        "board holds exactly thirty",
        !board.register(dup) && !board.register(over),
        "",
    );

    // 2. 一览分布可核（注入分布 5/7/6/12——四层各列）。
    set.add(
        "layer distribution inspectable",
        board.layer_count(Layer::Synth) == 5
            && board.layer_count(Layer::Boot) == 7
            && board.layer_count(Layer::Io) == 6
            && board.layer_count(Layer::Sched) == 12,
        "",
    );

    // 3. 单层红 → 该层不绿（红绿一览一眼可辨）。
    let mut bad = JudgeBoard::new();
    let _ = bad.register(Judgement { fid: "F041", layer: Layer::Synth, passed: true, evidence: "e" });
    let _ = bad.register(Judgement { fid: "F042", layer: Layer::Synth, passed: false, evidence: "e" });
    set.add("single red blocks layer", !bad.layer_green(Layer::Synth), "");

    // 4. 空层不算绿（未验收 ≠ 通过）。
    set.add("empty layer is not green", !bad.layer_green(Layer::Boot), "");

    // 5. 单层绿不算数：F070 红（Sched 层不绿）→ 整链不开跑；四层全绿 → 放行。
    let board_green = sample_board(true);
    let board_red = sample_board(false);
    let mut runner = ChainRunner::new();
    set.add(
        "chain gate requires all four layers",
        runner.begin_run(&board_red, "b1") == BeginOutcome::LayersNotGreen
            && runner.begin_run(&board_green, "b1") == BeginOutcome::Started,
        "",
    );

    // 6. 50 件场景名表：唯一名 × 恰 50（脚本化唯一源）。
    let mut uniq = true;
    for i in 0..SCENARIO_APPS.len() {
        if SCENARIO_APPS[i + 1..].iter().any(|a| *a == SCENARIO_APPS[i]) {
            uniq = false;
        }
    }
    set.add("scenario names fifty unique", uniq && SCENARIO_APPS.len() == SCENARIO_APP_COUNT, "");

    // 7. 整链分位口径唯一：全帧集合算分位（反例——单件分位平均 ≠ 全帧分位）。
    //    件 A：10 帧 10_000us（P95=10_000）；件 B：10 帧 13_000us（P95=13_000）。
    //    单件分位平均 = 11_500；全帧 P95 = 13_000（第 19/20 大值邻域）。
    let mut r2 = ChainRunner::new();
    let mut frames_a = Vec::new();
    for _ in 0..10 {
        frames_a.push(10_000);
    }
    let mut frames_b = Vec::new();
    for _ in 0..10 {
        frames_b.push(13_000);
    }
    assert!(r2.run_app("edit-00", &frames_a));
    assert!(r2.run_app("edit-01", &frames_b));
    // 全帧 20 帧：19_000? 不——10×10_000 + 10×13_000，P95 = 第 19 序位 = 13_000。
    let whole_p95 = pct_near(&{
        let mut all = frames_a.clone();
        for f in &frames_b {
            all.push(*f);
        }
        all
    }, 95);
    set.add(
        "full-chain percentile over all frames",
        whole_p95 == 13_000 && (10_000 + 13_000) / 2 != whole_p95,
        "",
    );

    // 8. Green：P95 ≤12.5 且 P99 ≤16.6 双线全中。
    set.add(
        "green requires both targets",
        verdict_of(12_500, 16_600) == Verdict::Green && verdict_of(12_000, 16_000) == Verdict::Green,
        "",
    );

    // 9. Amber：目标破、底线守——优化不停（不回炉）。
    set.add(
        "amber keeps optimizing not reforge",
        verdict_of(14_000, 15_000) == Verdict::Amber && verdict_of(16_600, 16_600) == Verdict::Amber,
        "",
    );

    // 10. Red：底线破（P95 >16.6ms）——回炉。
    set.add("red means floor broken", verdict_of(16_601, 16_601) == Verdict::Red, "");

    // 11. P99 只在目标层判：P95 优但 P99 超目标 → Amber 非 Green。
    set.add(
        "p99 gates target level only",
        verdict_of(12_000, 16_601) == Verdict::Amber,
        "",
    );

    // 12. 50 件跑满 + 攻坚名单：拖垮件入名单、verdict 纯按整链分位（不降标准）。
    let mut r3 = ChainRunner::new();
    assert!(r3.begin_run(&board_green, "b1") == BeginOutcome::Started);
    let mut ok_frames = Vec::new();
    for _ in 0..20 {
        ok_frames.push(11_000);
    }
    for i in 0..SCENARIO_APP_COUNT {
        let name = SCENARIO_APPS[i];
        if i == 49 {
            let mut bad_frames = Vec::new();
            for _ in 0..40 {
                bad_frames.push(20_000);
            }
            assert!(r3.run_app(name, &bad_frames));
        } else {
            assert!(r3.run_app(name, &ok_frames));
        }
    }
    let heavy = r3.heavy_list();
    let (fin, card) = r3.finalize(Some(26), "rec-2026q1.mp4", "ledger-2026q1.json");
    if let FinalizeOutcome::Issued = fin {
        if let Some(c) = card {
            // 全帧 = 49×20×11_000 + 40×20_000 = 1020 帧。
            // P95 = 秩 969 → 优帧区 11_000；P99 = 秩 1010 → 劣帧区 20_000
            // → Amber（目标破底线守——优化不停）。名单恰 1 件（dev-09），
            // 不改变 verdict 判据（纯按整链分位）。
            set.add(
                "heavy app listed verdict not lowered",
                c.verdict == Verdict::Amber
                    && c.verdict == verdict_of(c.p95_us, c.p99_us)
                    && c.p95_us == 11_000
                    && c.p99_us == 20_000
                    && heavy.len() == 1
                    && heavy[0].app == "dev-09"
                    && c.heavy[0].app == "dev-09"
                    && c.machine == LOCKED_MACHINE
                    && c.temp_c == Some(26),
                "",
            );
        } else {
            set.add("heavy app listed verdict not lowered", false, "no card");
        }
    } else {
        set.add("heavy app listed verdict not lowered", false, "not issued");
    }

    // 13. 证据两件不齐 → 成绩单不成立。
    let mut r4 = full_run();
    let (fin4, card4) = r4.finalize(Some(25), "", "ledger.json");
    set.add(
        "evidence missing blocks scorecard",
        fin4 == FinalizeOutcome::EvidenceMissing && card4.is_none(),
        "",
    );

    // 14. 50 件未跑满 → 拒绝出单（版本漂移后半途防线）。
    let mut r5 = ChainRunner::new();
    assert!(r5.begin_run(&board_green, "b1") == BeginOutcome::Started);
    assert!(r5.run_app("edit-00", &ok_frames));
    let (fin5, _) = r5.finalize(Some(25), "rec", "ledger");
    set.add("incomplete run blocks scorecard", fin5 == FinalizeOutcome::Incomplete, "");

    // 15. 机器锁定：成绩单 machine 恒 Y7000（可比性）。
    let mut r6 = full_run();
    let (_, card6) = r6.finalize(None, "rec.mp4", "ledger.json");
    set.add(
        "machine locked to Y7000 temp optional",
        card6.map(|c| c.machine == LOCKED_MACHINE && c.temp_c.is_none()).unwrap_or(false),
        "",
    );

    // 16. 版本漂移防护：build_tag 变更 → 旧帧清零 + 须整链重跑（50 件从头）。
    let mut r7 = full_run();
    assert!(r7.begin_run(&board_green, "b2") == BeginOutcome::Started);
    set.add(
        "version drift forces rerun",
        r7.rerun_needed() && r7.apps_done() == 0,
        "",
    );

    // 17. 成绩单版本化：季度快照递增。
    let mut r8 = full_run();
    let (_, c81) = r8.finalize(Some(25), "rec1", "led1");
    // 同版本续跑被拒（同件重复跑拒绝）——出单不锁复测：新季度快照 =
    // 版本号在 finalize 递增，账本证据名区分季度。
    let v1 = c81.map(|c| c.suite_version).unwrap_or(0);
    // 重新构造第二季度成绩（新 build_tag 触发漂移清零 → 重跑 50 件）。
    assert!(r8.begin_run(&board_green, "b3") == BeginOutcome::Started);
    for i in 0..SCENARIO_APP_COUNT {
        assert!(r8.run_app(SCENARIO_APPS[i], &ok_frames));
    }
    let (_, c82) = r8.finalize(Some(25), "rec2", "led2");
    let v2 = c82.as_ref().map(|c| c.suite_version).unwrap_or(0);
    set.add("scorecard version increments", v1 == 1 && v2 == 2, "");

    // 18. 证据链引用（不复制）：成绩单只带引用名（录屏/账本导出名），
    //     不携带帧序列——结构尺寸固定，数据在账本原地。
    let c8 = c82.unwrap_or_else(|| ScoreCard {
        machine: "", temp_c: None, build_tag: "", suite_version: 0,
        p95_us: 0, p99_us: 0, verdict: Verdict::Red,
        heavy: Vec::new(), evidence_screen: "", evidence_ledger: "",
    });
    set.add(
        "evidence referenced not copied",
        c8.evidence_screen == "rec2" && c8.evidence_ledger == "led2" && c8.build_tag == "b3",
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 自检辅助（样例判据一览 / 全链跑件——仅本文件使用）
// ---------------------------------------------------------------------------

/// 样例 30 项一览：F041-F070，分布 Synth=5 / Boot=7 / Io=6 / Sched=12。
/// `sched_green=false` 时把 F070 置红（门控反演用）。
fn sample_board(sched_green: bool) -> JudgeBoard {
    let mut b = JudgeBoard::new();
    let fids: [&str; 30] = [
        "F041", "F042", "F043", "F044", "F045",
        "F046", "F047", "F048", "F049", "F050", "F051", "F052",
        "F053", "F054", "F055", "F056", "F057", "F058",
        "F059", "F060", "F061", "F062", "F063", "F064", "F065", "F066", "F067", "F068", "F069", "F070",
    ];
    for (i, f) in fids.iter().enumerate() {
        let layer = if i < 5 {
            Layer::Synth
        } else if i < 12 {
            Layer::Boot
        } else if i < 18 {
            Layer::Io
        } else {
            Layer::Sched
        };
        let passed = !(*f == "F070" && !sched_green);
        let _ = b.register(Judgement { fid: f, layer, passed, evidence: "ref" });
    }
    b
}

/// 预跑满 50 件的执行器（全部 11_000us 帧——Green 形态）。
fn full_run() -> ChainRunner {
    let mut r = ChainRunner::new();
    let board = sample_board(true);
    assert!(r.begin_run(&board, "b1") == BeginOutcome::Started);
    let mut frames = Vec::new();
    for _ in 0..20 {
        frames.push(11_000);
    }
    for i in 0..SCENARIO_APP_COUNT {
        assert!(r.run_app(SCENARIO_APPS[i], &frames));
    }
    r
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_boundaries_exact() {
        // 判线边界逐值钉死（一处一事实：TARGET/FLOOR 常量改值必炸这里）。
        assert_eq!(verdict_of(TARGET_P95_US, TARGET_P99_US), Verdict::Green);
        assert_eq!(verdict_of(TARGET_P95_US + 1, TARGET_P99_US), Verdict::Amber);
        assert_eq!(verdict_of(FLOOR_P95_US, TARGET_P99_US), Verdict::Amber);
        assert_eq!(verdict_of(FLOOR_P95_US + 1, FLOOR_P95_US + 1), Verdict::Red);
    }

    #[test]
    fn duplicate_app_run_rejected() {
        let mut r = ChainRunner::new();
        let mut frames = Vec::new();
        for _ in 0..10 {
            frames.push(11_000);
        }
        assert!(r.run_app("term-00", &frames));
        assert!(!r.run_app("term-00", &frames), "同版本内重复跑一件拒绝");
        assert!(!r.run_app("ghost-app", &frames), "名表外拒绝");
        assert!(!r.run_app("term-01", &[]), "空帧序列拒绝");
    }

    #[test]
    fn heavy_list_empty_when_all_fast() {
        let mut r = full_run();
        assert!(r.heavy_list().is_empty());
        let (fin, card) = r.finalize(Some(24), "rec", "led");
        assert_eq!(fin, FinalizeOutcome::Issued);
        let c = card.expect("card");
        assert_eq!(c.verdict, Verdict::Green);
        assert_eq!(c.p95_us, 11_000);
        assert_eq!(c.p99_us, 11_000);
    }

    #[test]
    fn layers_not_green_blocks_even_with_evidence() {
        // 门在前：四层未绿，跑件本身放不进去（begin_run 已拒）——
        // finalize 的 Incomplete 是半途防线，此处验证两道防线各自独立。
        let mut r = ChainRunner::new();
        let board = sample_board(false);
        assert_eq!(r.begin_run(&board, "b1"), BeginOutcome::LayersNotGreen);
    }

    #[test]
    fn scenario_apps_match_f040_classes() {
        // 五类 × 10 件（F040 类目结构）：类前缀可辨。
        for cls in ["edit", "arch", "image", "term", "dev"] {
            let n = SCENARIO_APPS.iter().filter(|a| a.starts_with(cls)).count();
            assert_eq!(n, 10, "{} 类应恰 10 件", cls);
        }
    }

    #[test]
    fn board_evidence_is_reference_only() {
        // 一览的 evidence 是引用名（不复制数据）——登记时即可为空引用占位。
        let mut b = JudgeBoard::new();
        assert!(b.register(Judgement {
            fid: "F041",
            layer: Layer::Synth,
            passed: true,
            evidence: "ref:F041-record",
        }));
        assert_eq!(b.view()[0].evidence, "ref:F041-record");
    }
}
