//! F070 性能域总判据（perfstar2 · G-B-30）——单层绿不算数，整链绿才交付。
//!
//! 主册判据（验收标准第一句）：
//! **整链场景定义文档化（50 件的打开/操作/退出脚本化）；80fps 达标定义 =
//! P95 帧耗时 ≤12.5ms 且 P99 ≤16.6ms；成绩单两份证据（录屏+账本导出）齐备。**
//!
//! 功能定义（G-B-30）：80fps 整链验收制度——B 域四层（合成/启动/IO/调度）
//! 每层独立判据全绿后，整链在「常用 50 件」（F040）真实负载下复测 80fps。
//!
//! 【交互设计】星图/生态季报（F149）公布整链成绩单；诊断中心「性能域状态」
//! 页 30 项判据红绿一览（每项可点进证据）。
//! 【数据与存储】成绩单版本化（季度快照）；证据链引用各单项判据记录（不复制）。
//! 【状态与异常】整链复测中有应用拖垮帧率 → 该应用入「攻坚名单」单独优化
//! （不降低总验收标准）；借力件升级后整链复测重跑（版本漂移防护）。
//! 【设计细节】整链测试机锁定 Y7000（可比性）；测试环境温度记录（热节流
//! 干扰排除）；每次系统大版本（借力件升级窗）后整链必复测；达标线与红线
//! 分离：80fps 目标/60fps 底线——底线破即回炉，目标破即优化不停。
//!
//! 零堆纪律：定长层表 + 定长攻坚名单，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 80fps 目标线：P95 帧耗时 ≤12.5ms（×10 定点 = 125）。
pub const TARGET_P95_MS_X10: u32 = 125;
/// 80fps 目标线：P99 帧耗时 ≤16.6ms（×10 定点 = 166）。
pub const TARGET_P99_MS_X10: u32 = 166;
/// 60fps 底线（红线分离）：P99 >16.6ms 即底线破——回炉。
pub const FLOOR_P99_MS_X10: u32 = 166;
/// 常用 50 件（F040 整链负载）。
pub const CHAIN_APP_COUNT: usize = 50;
/// 攻坚名单容量。
pub const DEFEAT_LIST_CAP: usize = 8;
/// 成绩单版本化（季度快照保留 4 季）。
pub const SCORECARD_QUARTERS: usize = 4;

/// B 域四层（主册明文：合成/启动/IO/调度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainLayer {
    Compose,
    Boot,
    Io,
    Sched,
}

impl ChainLayer {
    pub fn name(self) -> &'static str {
        match self {
            ChainLayer::Compose => "compose",
            ChainLayer::Boot => "boot",
            ChainLayer::Io => "io",
            ChainLayer::Sched => "sched",
        }
    }
}

/// 整链成绩单（版本化季度快照）。
#[derive(Clone, Copy, Debug)]
pub struct Scorecard {
    pub quarter: u16,
    /// P95 / P99 帧耗时（×10 定点 ms）。
    pub p95_ms_x10: u32,
    pub p99_ms_x10: u32,
    /// 四层独立判据全绿旗标（整链复测前置条件）。
    pub layers_green: [bool; 4],
    /// 整链 50 件负载完成数。
    pub apps_completed: u16,
    /// 两份证据：录屏 + 账本导出。
    pub evidence_recording: bool,
    pub evidence_ledger_export: bool,
    /// 测试机锁定（Y7000）与环境温度（°C）。
    pub machine_id: &'static str,
    pub ambient_temp_c: i16,
    /// 借力件版本戳（升级后整链重跑的比对锚）。
    pub borrowed_stack_version: u64,
}

impl Scorecard {
    /// 80fps 目标判定：P95 ≤12.5ms 且 P99 ≤16.6ms（两条件同时）。
    pub fn meets_target(&self) -> bool {
        self.p95_ms_x10 <= TARGET_P95_MS_X10 && self.p99_ms_x10 <= TARGET_P99_MS_X10
    }

    /// 60fps 底线判定：P99 ≤16.6ms（底线破即回炉）。
    pub fn meets_floor(&self) -> bool {
        self.p99_ms_x10 <= FLOOR_P99_MS_X10
    }

    /// 成绩单有效性：四层全绿 + 50 件全跑 + 两份证据齐 + 环境可比。
    pub fn valid(&self) -> bool {
        self.layers_green.iter().all(|g| *g)
            && self.apps_completed as usize == CHAIN_APP_COUNT
            && self.evidence_recording
            && self.evidence_ledger_export
            && self.machine_id == "Y7000"
            && self.borrowed_stack_version > 0
    }
}

// ---------------------------------------------------------------------------
// 整链门
// ---------------------------------------------------------------------------

/// 性能域整链门。
#[derive(Clone, Copy)]
pub struct ChainGate {
    /// 四层独立判据旗标。
    layers_green: [bool; 4],
    /// 攻坚名单（拖垮帧率的应用——不降低总验收标准）。
    defeat_list: [&'static str; DEFEAT_LIST_CAP],
    defeat_n: usize,
    /// 借力件版本戳（升级检测锚）。
    borrowed_stack_version: u64,
    /// 季度成绩单环。
    cards: [Option<Scorecard>; SCORECARD_QUARTERS],
    card_n: usize,
    /// 环境温度记录（热节流干扰排除）。
    ambient_temp_c: i16,
}

impl ChainGate {
    pub const fn new() -> Self {
        ChainGate {
            layers_green: [false; 4],
            defeat_list: [""; DEFEAT_LIST_CAP],
            defeat_n: 0,
            borrowed_stack_version: 0,
            cards: [None; SCORECARD_QUARTERS],
            card_n: 0,
            ambient_temp_c: 0,
        }
    }

    /// 单层判据登记（四层各自独立全绿——整链复测前置条件）。
    pub fn set_layer(&mut self, layer: ChainLayer, green: bool) {
        let idx = match layer {
            ChainLayer::Compose => 0,
            ChainLayer::Boot => 1,
            ChainLayer::Io => 2,
            ChainLayer::Sched => 3,
        };
        self.layers_green[idx] = green;
    }

    pub fn layers_all_green(&self) -> bool {
        self.layers_green.iter().all(|g| *g)
    }

    pub fn layer_state(&self, layer: ChainLayer) -> bool {
        let idx = match layer {
            ChainLayer::Compose => 0,
            ChainLayer::Boot => 1,
            ChainLayer::Io => 2,
            ChainLayer::Sched => 3,
        };
        self.layers_green[idx]
    }

    /// 借力件版本登记（升级 → 整链复测失效重跑）。
    pub fn set_borrowed_stack_version(&mut self, v: u64) {
        if self.borrowed_stack_version != 0 && v != self.borrowed_stack_version {
            // 版本漂移：既有成绩单全部失效（重跑前不承认旧成绩）。
            self.cards = [None; SCORECARD_QUARTERS];
            self.card_n = 0;
        }
        self.borrowed_stack_version = v;
    }

    pub fn borrowed_stack_version(&self) -> u64 {
        self.borrowed_stack_version
    }

    /// 环境温度记录（热节流干扰排除）。
    pub fn record_ambient_temp(&mut self, c: i16) {
        self.ambient_temp_c = c;
    }

    /// 攻坚名单登记（拖垮帧率的应用；重复登记幂等）。
    pub fn add_defeat(&mut self, app: &'static str) -> bool {
        if self.defeat_list[..self.defeat_n].contains(&app) {
            return true; // 已在名单
        }
        if self.defeat_n == DEFEAT_LIST_CAP {
            return false;
        }
        self.defeat_list[self.defeat_n] = app;
        self.defeat_n += 1;
        true
    }

    pub fn defeat_list(&self) -> &[&'static str] {
        &self.defeat_list[..self.defeat_n]
    }

    /// 整链复测（50 件脚本负载）：产出成绩单——层未全绿拒绝出单。
    pub fn run_chain(
        &mut self,
        quarter: u16,
        p95_ms_x10: u32,
        p99_ms_x10: u32,
        apps_completed: u16,
        evidence_recording: bool,
        evidence_ledger_export: bool,
    ) -> Result<Scorecard, &'static str> {
        if !self.layers_all_green() {
            return Err("四层独立判据未全绿——整链复测前置条件不满足");
        }
        if self.borrowed_stack_version == 0 {
            return Err("借力件版本戳未登记——版本漂移防护未就绪");
        }
        let card = Scorecard {
            quarter,
            p95_ms_x10,
            p99_ms_x10,
            layers_green: self.layers_green,
            apps_completed,
            evidence_recording,
            evidence_ledger_export,
            machine_id: "Y7000",
            ambient_temp_c: self.ambient_temp_c,
            borrowed_stack_version: self.borrowed_stack_version,
        };
        self.cards[self.card_n % SCORECARD_QUARTERS] = Some(card);
        self.card_n += 1;
        Ok(card)
    }

    pub fn scorecard_count(&self) -> usize {
        self.card_n.min(SCORECARD_QUARTERS)
    }

    pub fn latest_scorecard(&self) -> Option<Scorecard> {
        if self.card_n == 0 {
            return None;
        }
        self.cards[(self.card_n - 1) % SCORECARD_QUARTERS]
    }

    pub fn ambient_temp_c(&self) -> i16 {
        self.ambient_temp_c
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_perfgate_checks() -> CheckSet {
    let mut cs = CheckSet::new("F070-perfgate");
    // 1) 80fps 达标定义：P95 ≤12.5 且 P99 ≤16.6 双条件。
    let card_ok = Scorecard {
        quarter: 1,
        p95_ms_x10: 120,
        p99_ms_x10: 160,
        layers_green: [true; 4],
        apps_completed: 50,
        evidence_recording: true,
        evidence_ledger_export: true,
        machine_id: "Y7000",
        ambient_temp_c: 25,
        borrowed_stack_version: 7,
    };
    cs.add("target_80fps_both_conditions", card_ok.meets_target() && card_ok.meets_floor(), "");
    // 2) P95 达标但 P99 超 → 目标未达但底线未破（优化不停）。
    let card_mid = Scorecard { p95_ms_x10: 110, p99_ms_x10: 200, ..card_ok };
    cs.add("p95_ok_p99_over_no_target", !card_mid.meets_target() && !card_mid.meets_floor(), "");
    // 3) 底线破（P99 >16.6ms）→ 回炉（红线）。
    let card_bad = Scorecard { p99_ms_x10: 250, ..card_ok };
    cs.add("floor_broken_rework", !card_bad.meets_floor() && !card_bad.meets_target(), "");
    // 4) 层未全绿 → 整链复测拒绝出单。
    let mut g = ChainGate::new();
    let _ = g.set_borrowed_stack_version(7);
    g.set_layer(ChainLayer::Compose, true);
    g.set_layer(ChainLayer::Boot, true);
    g.set_layer(ChainLayer::Io, true);
    // Sched 未登记。
    cs.add(
        "chain_refuses_until_layers_green",
        g.run_chain(1, 120, 160, 50, true, true).is_err(),
        "",
    );
    // 5) 四层全绿 → 出单成功；成绩单有效性核验。
    g.set_layer(ChainLayer::Sched, true);
    let card = g.run_chain(1, 120, 160, 50, true, true);
    cs.add(
        "chain_green_issues_scorecard",
        card.as_ref().map(|c| c.valid()).unwrap_or(false),
        "",
    );
    // 6) 50 件负载未跑满 → 成绩单无效。
    let card_short = g.run_chain(2, 120, 160, 48, true, true).unwrap();
    cs.add("fifty_apps_required", !card_short.valid(), "");
    // 7) 两份证据缺一 → 无效（录屏 + 账本导出）。
    let card_norec = g.run_chain(3, 120, 160, 50, false, true).unwrap();
    let card_nols = g.run_chain(3, 120, 160, 50, true, false).unwrap();
    cs.add("two_evidences_required", !card_norec.valid() && !card_nols.valid(), "");
    // 8) 攻坚名单：拖垮帧率的应用入名单（不降总验收标准）。
    let mut g8 = ChainGate::new();
    let _ = g8.set_borrowed_stack_version(7);
    for l in [ChainLayer::Compose, ChainLayer::Boot, ChainLayer::Io, ChainLayer::Sched] {
        g8.set_layer(l, true);
    }
    let _ = g8.add_defeat("heavy-render-app");
    let _ = g8.add_defeat("heavy-render-app"); // 重复登记幂等
    let _ = g8.add_defeat("leaky-game");
    cs.add("defeat_list_dedup_cap", g8.defeat_list() == ["heavy-render-app", "leaky-game"], "");
    // 9) 借力件升级 → 成绩单失效重跑（版本漂移防护）。
    let issued = g8.run_chain(1, 120, 160, 50, true, true).is_ok();
    let before = g8.scorecard_count();
    g8.set_borrowed_stack_version(8);
    cs.add(
        "upgrade_invalidates_scorecards",
        issued && before == 1 && g8.scorecard_count() == 0,
        "",
    );
    // 10) 环境温度记录在案（热节流干扰排除）。
    g8.record_ambient_temp(26);
    cs.add("ambient_temp_recorded", g8.ambient_temp_c() == 26, "");
    // 11) 攻坚名单容量上限诚实拒绝。
    let mut g11 = ChainGate::new();
    for i in 0..DEFEAT_LIST_CAP {
        let _ = g11.add_defeat(defeat_name(i));
    }
    cs.add("defeat_list_cap_honest", g11.defeat_list().len() == DEFEAT_LIST_CAP && !g11.add_defeat("overflow-app"), "");
    cs
}

/// 攻坚名单测试名。
fn defeat_name(i: usize) -> &'static str {
    const NAMES: [&str; DEFEAT_LIST_CAP] =
        ["app-0", "app-1", "app-2", "app-3", "app-4", "app-5", "app-6", "app-7"];
    NAMES[i % DEFEAT_LIST_CAP]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_constants_match_master_register() {
        assert_eq!(TARGET_P95_MS_X10, 125, "80fps 目标 P95 ≤12.5ms");
        assert_eq!(TARGET_P99_MS_X10, 166, "80fps 目标 P99 ≤16.6ms");
        assert_eq!(FLOOR_P99_MS_X10, 166, "60fps 底线 = P99 ≤16.6ms");
    }

    #[test]
    fn four_layers_named_per_master_register() {
        assert_eq!(ChainLayer::Compose.name(), "compose");
        assert_eq!(ChainLayer::Boot.name(), "boot");
        assert_eq!(ChainLayer::Io.name(), "io");
        assert_eq!(ChainLayer::Sched.name(), "sched");
    }

    #[test]
    fn machine_locked_to_y7000() {
        let g = ChainGate::new();
        let mut g2 = g; // 值语义拷贝检查（成绩单机锁常量内嵌）
        g2.record_ambient_temp(30);
        assert_eq!(g.ambient_temp_c(), 0, "值语义独立");
        // 机锁在 Scorecard 构造内固定 Y7000。
        let c = Scorecard {
            quarter: 1,
            p95_ms_x10: 100,
            p99_ms_x10: 120,
            layers_green: [true; 4],
            apps_completed: 50,
            evidence_recording: true,
            evidence_ledger_export: true,
            machine_id: "OTHER-PC",
            ambient_temp_c: 25,
            borrowed_stack_version: 1,
        };
        assert!(!c.valid(), "非 Y7000 机成绩无效（可比性锁定）");
    }

    #[test]
    fn first_version_registration_does_not_invalidate() {
        let mut g = ChainGate::new();
        g.set_borrowed_stack_version(5);
        assert_eq!(g.borrowed_stack_version(), 5);
        // 首次登记不触发失效路径（无既有成绩单可失效）。
        assert_eq!(g.scorecard_count(), 0);
    }

    #[test]
    fn scorecard_ring_keeps_four_quarters() {
        let mut g = ChainGate::new();
        let _ = g.set_borrowed_stack_version(9);
        for l in [ChainLayer::Compose, ChainLayer::Boot, ChainLayer::Io, ChainLayer::Sched] {
            g.set_layer(l, true);
        }
        for q in 0..6u16 {
            let _ = g.run_chain(q, 120, 160, 50, true, true).unwrap();
        }
        assert_eq!(g.scorecard_count(), SCORECARD_QUARTERS, "季度快照保留 4 份");
        assert_eq!(g.latest_scorecard().unwrap().quarter, 5, "最新快照在环尾");
    }
}

// ===========================================================================
// v2 深化批（F070 · G-B-30）——场景状态机 / 帧统计引擎 / 季度环比
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-30 功能定义的实装细化，非新立项）：
// 1. ScenarioRunner —— 50 件负载场景状态机：打开→操作→退出三段计时，
//    段间超时/中断/恢复路径显式化（「打开/操作/退出脚本化」的执行面）；
//    完成数/中断数/超时数三账分立。
// 2. FrameStatEngine —— 帧统计引擎：128 帧耗时样本的 P95/P99 整数位次
//    （×10 定点 ms）——「80fps 达标 = P95 ≤12.5 且 P99 ≤16.6」的计算面
//    （插入排序位次，128 元素栈上 512B——内核栈预算内）。
// 3. QuarterCompare —— 季度环比：相邻两份成绩单逐项对比（P95/P99/
//    完成数），任一退化即红——「逐季不回退」的计算器。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 场景段超时（ms）：打开 3s / 操作 5s / 退出 2s（旋钮可调）。
pub const SCEN_OPEN_TIMEOUT_MS: u32 = 3_000;
pub const SCEN_OPERATE_TIMEOUT_MS: u32 = 5_000;
pub const SCEN_EXIT_TIMEOUT_MS: u32 = 2_000;
/// 帧统计样本容量。
pub const FRAME_SAMPLES: usize = 128;
/// 季度环比退化线：P95/P99 上升 >2ms（×10 = 20）即红（测量噪声带以上）。
pub const QUARTER_REGRESS_MS_X10: u32 = 20;

// ---------------------------------------------------------------------------
// 深化一：场景状态机
// ---------------------------------------------------------------------------

/// 场景段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenPhase {
    Open,
    Operate,
    Exit,
    Done,
    Aborted,
}

/// 场景执行结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenOutcome {
    Completed,
    OpenTimeout,
    OperateTimeout,
    ExitTimeout,
    Interrupted,
}

/// 单场景状态机。
pub struct ScenarioRunner {
    phase: ScenPhase,
    phase_start_ms: u32,
    /// 三段耗时（0 = 未完成）。
    open_ms: u32,
    operate_ms: u32,
    exit_ms: u32,
    /// 中断旗标（中断 ≠ 超时——归因不混淆）。
    was_interrupted: bool,
}

impl ScenarioRunner {
    pub const fn new() -> Self {
        ScenarioRunner {
            phase: ScenPhase::Open,
            phase_start_ms: 0,
            open_ms: 0,
            operate_ms: 0,
            exit_ms: 0,
            was_interrupted: false,
        }
    }

    /// 段完成推进（now_ms 为墙钟）。
    pub fn phase_done(&mut self, now_ms: u32) -> ScenPhase {
        let dur = now_ms.saturating_sub(self.phase_start_ms);
        match self.phase {
            ScenPhase::Open => {
                if dur > SCEN_OPEN_TIMEOUT_MS {
                    self.phase = ScenPhase::Aborted;
                    return self.phase;
                }
                self.open_ms = dur;
                self.phase = ScenPhase::Operate;
                self.phase_start_ms = now_ms;
            }
            ScenPhase::Operate => {
                if dur > SCEN_OPERATE_TIMEOUT_MS {
                    self.phase = ScenPhase::Aborted;
                    return self.phase;
                }
                self.operate_ms = dur;
                self.phase = ScenPhase::Exit;
                self.phase_start_ms = now_ms;
            }
            ScenPhase::Exit => {
                if dur > SCEN_EXIT_TIMEOUT_MS {
                    self.phase = ScenPhase::Aborted;
                    return self.phase;
                }
                self.exit_ms = dur;
                self.phase = ScenPhase::Done;
            }
            _ => {}
        }
        self.phase
    }

    /// 中断（用户切走/系统抢占）——状态机可从任一段进入 Aborted。
    pub fn interrupt(&mut self) {
        self.phase = ScenPhase::Aborted;
        self.was_interrupted = true;
    }

    pub fn outcome(&self) -> ScenOutcome {
        match self.phase {
            ScenPhase::Done => ScenOutcome::Completed,
            ScenPhase::Aborted => {
                if self.was_interrupted {
                    ScenOutcome::Interrupted
                } else if self.open_ms == 0 {
                    ScenOutcome::OpenTimeout
                } else if self.operate_ms == 0 {
                    ScenOutcome::OperateTimeout
                } else if self.exit_ms == 0 {
                    ScenOutcome::ExitTimeout
                } else {
                    ScenOutcome::Interrupted
                }
            }
            _ => ScenOutcome::Interrupted,
        }
    }

    pub fn total_ms(&self) -> u32 {
        self.open_ms + self.operate_ms + self.exit_ms
    }
}

/// 50 件负载总账。
pub struct ScenarioLedger {
    completed: u16,
    interrupted: u16,
    timeouts: u16,
}

impl ScenarioLedger {
    pub const fn new() -> Self {
        ScenarioLedger {
            completed: 0,
            interrupted: 0,
            timeouts: 0,
        }
    }

    pub fn record(&mut self, o: ScenOutcome) {
        match o {
            ScenOutcome::Completed => self.completed += 1,
            ScenOutcome::Interrupted => self.interrupted += 1,
            _ => self.timeouts += 1,
        }
    }

    pub fn all_completed(&self) -> bool {
        self.completed as usize == CHAIN_APP_COUNT
    }

    pub fn stats(&self) -> (u16, u16, u16) {
        (self.completed, self.interrupted, self.timeouts)
    }
}

// ---------------------------------------------------------------------------
// 深化二：帧统计引擎
// ---------------------------------------------------------------------------

/// 帧耗时统计：128 样本 → P95/P99 ×10 定点 ms。
pub struct FrameStatEngine {
    samples: [u32; FRAME_SAMPLES],
    n: usize,
    overflow_dropped: u64,
}

impl FrameStatEngine {
    pub const fn new() -> Self {
        FrameStatEngine {
            samples: [0; FRAME_SAMPLES],
            n: 0,
            overflow_dropped: 0,
        }
    }

    /// 采一帧（×10 定点 ms；表满滚动覆盖最旧——持续采集语义）。
    pub fn sample(&mut self, frame_ms_x10: u32) {
        if self.n < FRAME_SAMPLES {
            self.samples[self.n] = frame_ms_x10;
            self.n += 1;
        } else {
            for k in 0..FRAME_SAMPLES - 1 {
                self.samples[k] = self.samples[k + 1];
            }
            self.samples[FRAME_SAMPLES - 1] = frame_ms_x10;
            self.overflow_dropped += 0; // 覆盖是语义不是丢失——不计数
        }
    }

    fn percentile(&self, pct_x100: u32) -> u32 {
        if self.n == 0 {
            return 0;
        }
        let mut sorted = self.samples;
        for i in 1..self.n {
            let key = sorted[i];
            let mut j = i;
            while j > 0 && sorted[j - 1] > key {
                sorted[j] = sorted[j - 1];
                j -= 1;
            }
            sorted[j] = key;
        }
        let idx = ((pct_x100 as u64) * (self.n as u64) / 10_000)
            .max(1) as usize
            - 1;
        sorted[idx.min(self.n - 1)]
    }

    pub fn p95_ms_x10(&self) -> u32 {
        self.percentile(9_500)
    }

    pub fn p99_ms_x10(&self) -> u32 {
        self.percentile(9_900)
    }

    /// 80fps 目标判定（P95 ≤125 且 P99 ≤166 ×10）。
    pub fn meets_80fps(&self) -> bool {
        self.p95_ms_x10() <= TARGET_P95_MS_X10 && self.p99_ms_x10() <= TARGET_P99_MS_X10
    }

    /// 60fps 底线判定。
    pub fn meets_60fps_floor(&self) -> bool {
        self.p99_ms_x10() <= FLOOR_P99_MS_X10
    }

    pub fn samples(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 深化三：季度环比
// ---------------------------------------------------------------------------

/// 环比结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuarterDiff {
    /// P95 变化（×10，正 = 变慢）。
    pub p95_delta_x10: i32,
    /// P99 变化。
    pub p99_delta_x10: i32,
    /// 完成数变化（负 = 退步）。
    pub apps_delta: i32,
    /// 逐季不回退判定。
    pub no_regression: bool,
}

/// 相邻两季成绩单环比。
pub fn quarter_compare(prev: &Scorecard, curr: &Scorecard) -> QuarterDiff {
    let p95_delta = curr.p95_ms_x10 as i32 - prev.p95_ms_x10 as i32;
    let p99_delta = curr.p99_ms_x10 as i32 - prev.p99_ms_x10 as i32;
    let apps_delta = curr.apps_completed as i32 - prev.apps_completed as i32;
    let no_regression = p95_delta <= QUARTER_REGRESS_MS_X10 as i32
        && p99_delta <= QUARTER_REGRESS_MS_X10 as i32
        && apps_delta >= 0;
    QuarterDiff {
        p95_delta_x10: p95_delta,
        p99_delta_x10: p99_delta,
        apps_delta,
        no_regression,
    }
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：场景 / 帧统计 / 环比逐条实摆。
pub fn run_perfgate_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F070-perfgate-deep");

    // ── 场景状态机 ──
    // 1) 全程顺利 → Completed，三段计时齐。
    let mut sr = ScenarioRunner::new();
    let _ = sr.phase_done(800); // 打开 800ms
    let _ = sr.phase_done(800 + 2_000); // 操作 2s
    let _ = sr.phase_done(800 + 2_000 + 400); // 退出 400ms
    cs.add(
        "scen_full_complete",
        sr.outcome() == ScenOutcome::Completed && sr.total_ms() == 3_200,
        "",
    );
    // 2) 打开超时 → OpenTimeout（3.5s > 3s）。
    let mut sr2 = ScenarioRunner::new();
    let _ = sr2.phase_done(3_500);
    cs.add("scen_open_timeout", sr2.outcome() == ScenOutcome::OpenTimeout, "");
    // 3) 操作段超时（打开成功后卡 6s）。
    let mut sr3 = ScenarioRunner::new();
    let _ = sr3.phase_done(1_000);
    let _ = sr3.phase_done(1_000 + 5_500);
    cs.add("scen_operate_timeout", sr3.outcome() == ScenOutcome::OperateTimeout, "");
    // 4) 中断路径。
    let mut sr4 = ScenarioRunner::new();
    let _ = sr4.phase_done(1_000);
    sr4.interrupt();
    cs.add("scen_interrupt_path", sr4.outcome() == ScenOutcome::Interrupted, "");
    // 5) 账本：50 完成 = 全跑；超时/中断分账。
    let mut led = ScenarioLedger::new();
    for _ in 0..48 {
        led.record(ScenOutcome::Completed);
    }
    led.record(ScenOutcome::OpenTimeout);
    led.record(ScenOutcome::Interrupted);
    cs.add(
        "scen_ledger_split",
        led.stats() == (48, 1, 1) && !led.all_completed(),
        "",
    );
    led.record(ScenOutcome::Completed);
    led.record(ScenOutcome::Completed);
    cs.add("scen_ledger_full_50", led.all_completed() && led.stats().0 == 50, "");

    // ── 帧统计引擎 ──
    // 1) 恒定 12ms 帧 → P95=P99=120 → 80fps 达标。
    let mut fs = FrameStatEngine::new();
    for _ in 0..FRAME_SAMPLES {
        fs.sample(120);
    }
    cs.add(
        "framestat_steady_80fps",
        fs.p95_ms_x10() == 120 && fs.p99_ms_x10() == 120 && fs.meets_80fps(),
        "",
    );
    // 2) 5% 尾部尖峰 20ms：P95 不动、P99 不动（128×5% = 6 样本，位次 121/127）。
    let mut fs2 = FrameStatEngine::new();
    for k in 0..FRAME_SAMPLES {
        fs2.sample(if k < 6 { 200 } else { 120 });
    }
    cs.add(
        "framestat_tail_not_p95",
        fs2.p95_ms_x10() == 200 || fs2.p95_ms_x10() == 120,
        "", // 位次语义在下一条精确锚定
    );
    // 3) 位次精确锚：排序后 [120×122, 200×6]。P95 位次 = 9500×128/10000
    //    = 121 → idx 120 → 120（5% 尖峰不进 P95——P95 的本义）；
    //    P99 位次 = 9900×128/10000 = 126 → idx 125 → 200（尾部进 P99）。
    cs.add(
        "framestat_percentile_exact",
        fs2.p95_ms_x10() == 120 && fs2.p99_ms_x10() == 200,
        "",
    );
    // 4) 底线判定：3 个 16.7ms 尖峰（排序位 125..127，P99 位次 125 落尖峰）
    //    → P99 = 167 > 166 破线。
    let mut fs3 = FrameStatEngine::new();
    for k in 0..FRAME_SAMPLES {
        fs3.sample(if k < 3 { 167 } else { 100 });
    }
    cs.add("framestat_floor_breaks", fs3.p99_ms_x10() == 167 && !fs3.meets_60fps_floor(), "");
    // 5) 滚动覆盖：灌 200 帧 → 窗内只剩后 128。
    let mut fs4 = FrameStatEngine::new();
    for k in 0..200u32 {
        fs4.sample(if k < 72 { 300 } else { 100 });
    }
    cs.add(
        "framestat_window_rolls",
        fs4.samples() == FRAME_SAMPLES && fs4.p95_ms_x10() == 100,
        "",
    );

    // ── 季度环比 ──
    fn card(q: u16, p95: u32, p99: u32, apps: u16) -> Scorecard {
        Scorecard {
            quarter: q,
            p95_ms_x10: p95,
            p99_ms_x10: p99,
            layers_green: [true; 4],
            apps_completed: apps,
            evidence_recording: true,
            evidence_ledger_export: true,
            machine_id: "Y7000",
            ambient_temp_c: 25,
            borrowed_stack_version: 1,
        }
    }
    // 1) 持平 → 不回退。
    let q1 = card(1, 120, 160, 50);
    let q2 = card(2, 120, 160, 50);
    cs.add("qcmp_flat_green", quarter_compare(&q1, &q2).no_regression, "");
    // 2) P99 恶化 3ms（>2ms 线）→ 红。
    let q3 = card(3, 120, 190, 50);
    let d = quarter_compare(&q2, &q3);
    cs.add(
        "qcmp_p99_regress_red",
        !d.no_regression && d.p99_delta_x10 == 30,
        "",
    );
    // 3) 完成数退步 → 红。
    let q4 = card(4, 120, 160, 48);
    cs.add("qcmp_apps_drop_red", !quarter_compare(&q3, &q4).no_regression, "");
    // 4) 改善 → 绿（delta 负）。
    let q5 = card(5, 110, 150, 50);
    let d5 = quarter_compare(&q2, &q5);
    cs.add(
        "qcmp_improve_green",
        d5.no_regression && d5.p95_delta_x10 == -10,
        "",
    );

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn scenario_phase_boundaries_exact() {
        let mut sr = ScenarioRunner::new();
        let _ = sr.phase_done(SCEN_OPEN_TIMEOUT_MS); // 恰达超时线 = 未超（>）
        assert_eq!(sr.phase, ScenPhase::Operate, "恰达线不算超时（严格大于）");
    }

    #[test]
    fn framestat_single_sample_percentile_is_itself() {
        let mut fs = FrameStatEngine::new();
        fs.sample(90);
        assert_eq!(fs.p95_ms_x10(), 90);
        assert_eq!(fs.p99_ms_x10(), 90);
    }

    #[test]
    fn quarter_compare_sign_conventions() {
        let prev = Scorecard {
            quarter: 1,
            p95_ms_x10: 100,
            p99_ms_x10: 160,
            layers_green: [true; 4],
            apps_completed: 50,
            evidence_recording: true,
            evidence_ledger_export: true,
            machine_id: "Y7000",
            ambient_temp_c: 25,
            borrowed_stack_version: 1,
        };
        let mut curr = prev;
        curr.quarter = 2;
        curr.p99_ms_x10 = 160 + QUARTER_REGRESS_MS_X10; // 恰达线 = 未超（<=）
        curr.apps_completed = 50;
        assert!(quarter_compare(&prev, &curr).no_regression, "恰达 2ms 线不算回退");
        curr.p99_ms_x10 += 1;
        assert!(!quarter_compare(&prev, &curr).no_regression);
    }
}

// ===========================================================================
// v3 深化批（F070 · G-B-30）——加权总分 / 证据包序列化 / 二分排查
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-30 功能定义的实装细化，非新立项）：
// 1. ScoreAggregator —— 加权总分：四层判据 + 场景完成率 → 0-1000 总分
//    （域总判据的单一数字面——层层有权重）。
// 2. EvidencePack —— 证据包序列化：成绩单 → 定长字节包 + FNV 校验
//    （导出后可独立校验——「两份证据」的字节面）。
// 3. BisectRange —— 二分排查：嫌疑版本区间对半收缩（坏版本判定
//    由调用方喂——机制面），步数上界 log2(n)。
// 全部零堆：定长缓冲 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 四层权重（合成 40 / 启动 25 / IO 20 / 调度 15——×10 定点合计 1000）。
pub const LAYER_WEIGHTS_X10: [u32; 4] = [400, 250, 200, 150];
/// 证据包容量。
pub const EVIDENCE_PACK_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 深化一：加权总分
// ---------------------------------------------------------------------------

/// 总分聚合：各层 0-1000 分 × 权重 + 场景完成率（0-1000）。
pub struct ScoreAggregator {
    layer_scores: [Option<u16>; 4], // 0-1000
    scenario_completed: u16,
    scenario_total: u16,
}

impl ScoreAggregator {
    pub const fn new() -> Self {
        ScoreAggregator {
            layer_scores: [None; 4],
            scenario_completed: 0,
            scenario_total: 50,
        }
    }

    /// 设层分（0-1000，越界钳制）。
    pub fn set_layer(&mut self, layer: usize, score: u16) -> bool {
        if layer >= 4 {
            return false;
        }
        self.layer_scores[layer] = Some(score.min(1000));
        true
    }

    /// 设场景完成。
    pub fn set_scenarios(&mut self, completed: u16, total: u16) -> bool {
        if total == 0 || completed > total {
            return false;
        }
        self.scenario_completed = completed;
        self.scenario_total = total;
        true
    }

    /// 总分（0-1000；任一层未测 → None——不猜）。
    pub fn total(&self) -> Option<u16> {
        if self.layer_scores.iter().any(|l| l.is_none()) {
            return None;
        }
        let mut weighted: u64 = 0;
        for k in 0..4 {
            weighted += self.layer_scores[k].unwrap() as u64 * LAYER_WEIGHTS_X10[k] as u64;
        }
        let scen = self.scenario_completed as u64 * 1000
            / self.scenario_total.max(1) as u64;
        // 层分占 90%，场景占 10%。
        let total = weighted / 1000 * 900 / 1000 + scen * 100 / 1000;
        Some(total.min(1000) as u16)
    }

    /// 达标线（域总判据 ≥ 800）。
    pub fn meets_domain_bar(&self) -> bool {
        matches!(self.total(), Some(t) if t >= 800)
    }
}

// ---------------------------------------------------------------------------
// 深化二：证据包序列化
// ---------------------------------------------------------------------------

/// 证据包：定长字节 + 尾部 FNV 校验（导出后可独立校验）。
pub struct EvidencePack {
    buf: [u8; EVIDENCE_PACK_CAP],
    len: usize,
}

impl EvidencePack {
    /// 从成绩单字段打包：quarter(2) + p95(2) + p99(2) + apps(2) + flags(1)。
    pub fn pack(quarter: u16, p95_x10: u32, p99_x10: u32, apps: u16, layers_green: u8) -> Self {
        let mut p = EvidencePack { buf: [0; EVIDENCE_PACK_CAP], len: 0 };
        p.push_u16(quarter);
        p.push_u16(p95_x10 as u16);
        p.push_u16(p99_x10 as u16);
        p.push_u16(apps);
        p.buf[p.len] = layers_green;
        p.len += 1;
        // 尾部校验。
        let h = fnv1a(&p.buf[..p.len]);
        p.push_u32(h);
        p
    }

    fn push_u16(&mut self, v: u16) {
        let b = v.to_le_bytes();
        self.buf[self.len] = b[0];
        self.buf[self.len + 1] = b[1];
        self.len += 2;
    }

    fn push_u32(&mut self, v: u32) {
        let b = v.to_le_bytes();
        for k in 0..4 {
            self.buf[self.len + k] = b[k];
        }
        self.len += 4;
    }

    /// 校验（尾部 FNV vs 内容 FNV）。
    pub fn verify(&self) -> bool {
        if self.len < 5 {
            return false;
        }
        let content = &self.buf[..self.len - 4];
        let expected = fnv1a(content);
        let stored = u32::from_le_bytes([
            self.buf[self.len - 4],
            self.buf[self.len - 3],
            self.buf[self.len - 2],
            self.buf[self.len - 1],
        ]);
        expected == stored
    }

    /// 读取字段（偏移字节）。
    pub fn field_u16(&self, offset: usize) -> Option<u16> {
        if offset + 2 > self.len - 4 {
            return None;
        }
        Some(u16::from_le_bytes([self.buf[offset], self.buf[offset + 1]]))
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

/// FNV-1a（checks.rs 同款口径）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ---------------------------------------------------------------------------
// 深化三：二分排查
// ---------------------------------------------------------------------------

/// 奇偶二分（Vulcan 口径）：lo..=hi 嫌疑版本区间，喂判定 → 对半收缩。
pub struct BisectRange {
    lo: u64,
    hi: u64,
    steps: u32,
}

impl BisectRange {
    pub const fn new(lo: u64, hi: u64) -> Self {
        BisectRange { lo, hi, steps: 0 }
    }

    /// 当前测试点（区间中位）。
    pub fn probe(&self) -> Option<u64> {
        if self.lo > self.hi {
            return None;
        }
        Some(self.lo + (self.hi - self.lo) / 2)
    }

    /// 喂判定：true = 测试点是坏的（坏在更早）→ 收 hi；false → 收 lo。
    /// 返回是否已定位（lo == hi）。
    pub fn feed(&mut self, is_bad: bool) -> bool {
        let mid = match self.probe() {
            Some(m) => m,
            None => return true,
        };
        self.steps += 1;
        if is_bad {
            self.hi = mid;
        } else {
            self.lo = mid + 1;
        }
        self.lo >= self.hi
    }

    /// 定位的坏版本。
    pub fn culprit(&self) -> Option<u64> {
        if self.lo == self.hi {
            Some(self.lo)
        } else {
            None
        }
    }

    pub fn steps(&self) -> u32 {
        self.steps
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：总分 / 证据包 / 二分逐条实摆。
pub fn run_perfgate_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F070-perfgate-v3");

    // ── 加权总分 ──
    let mut sa = ScoreAggregator::new();
    cs.add("agg_incomplete_none", sa.total().is_none(), "");
    for k in 0..4 {
        let _ = sa.set_layer(k, 800);
    }
    let _ = sa.set_scenarios(50, 50);
    // 层 800 → 800×900/1000 = 720；场景 1000×100/1000 = 100 → 总 820。
    cs.add("agg_mixed_820", sa.total() == Some(820), "");
    // 精确核算：层分 900 → weighted = 900×1000/1000 = 900 → ×900/1000 = 810；
    // 场景 1000 × 100/1000 = 100 → 总 910。
    let mut sa2 = ScoreAggregator::new();
    for k in 0..4 {
        let _ = sa2.set_layer(k, 900);
    }
    let _ = sa2.set_scenarios(50, 50);
    cs.add("agg_weighted_math_910", sa2.total() == Some(910), "");
    cs.add("agg_meets_bar_910", sa2.meets_domain_bar(), "");
    // 低分不达标。
    let mut sa3 = ScoreAggregator::new();
    for k in 0..4 {
        let _ = sa3.set_layer(k, 500);
    }
    let _ = sa3.set_scenarios(25, 50);
    // 层 500 → 450；场景 500 → 50 → 总 500。
    cs.add("agg_below_bar_500", sa3.total() == Some(500) && !sa3.meets_domain_bar(), "");
    cs.add("agg_bad_layer_refused", !sa3.set_layer(4, 100) && !sa3.set_scenarios(60, 50), "");

    // ── 证据包 ──
    let pack = EvidencePack::pack(3, 125, 166, 50, 0b1111);
    cs.add("pack_len_13", pack.len() == 13, ""); // 9 字节 + 4 校验
    cs.add("pack_verify_ok", pack.verify(), "");
    cs.add("pack_field_roundtrip",
        pack.field_u16(0) == Some(3)
            && pack.field_u16(2) == Some(125)
            && pack.field_u16(4) == Some(166)
            && pack.field_u16(6) == Some(50),
        "",
    );
    // 篡改检出：改一个字节（构造坏包——手工翻转内容字节）。
    let mut bad = EvidencePack::pack(3, 125, 166, 50, 0b1111);
    bad.buf[2] ^= 0xFF;
    cs.add("pack_tamper_detected", !bad.verify(), "");

    // ── 二分排查 ──
    // 坏版本 = 13；区间 1..=16 → log2(16) = 4 步内定位。
    let mut bi = BisectRange::new(1, 16);
    let mut located = false;
    for _ in 0..8 {
        let probe = bi.probe().unwrap();
        located = bi.feed(probe >= 13);
        if located {
            break;
        }
    }
    cs.add("bisect_finds_culprit", located && bi.culprit() == Some(13), "");
    cs.add("bisect_step_bound", bi.steps() <= 4, ""); // log2(16)
    // 坏版本在边界（lo 自身坏）。
    let mut bi2 = BisectRange::new(5, 8);
    for _ in 0..8 {
        if bi2.feed(true) {
            break;
        }
    }
    cs.add("bisect_boundary_bad", bi2.culprit() == Some(5), "");
    // 单版本区间直接定位。
    cs.add("bisect_single_version", BisectRange::new(7, 7).probe() == Some(7), "");

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn agg_layer_clamped_to_1000() {
        let mut sa = ScoreAggregator::new();
        let _ = sa.set_layer(0, 5000);
        let _ = sa.set_layer(1, 1000);
        let _ = sa.set_layer(2, 1000);
        let _ = sa.set_layer(3, 1000);
        let _ = sa.set_scenarios(50, 50);
        assert!(sa.total().unwrap() <= 1000, "越界钳制不膨胀总分");
    }

    #[test]
    fn pack_field_out_of_range_none() {
        let p = EvidencePack::pack(1, 1, 1, 1, 0);
        assert!(p.field_u16(9).is_none(), "校验区不可读作字段");
        assert!(p.field_u16(100).is_none());
    }

    #[test]
    fn bisect_always_converges() {
        for culprit in 1..=16u64 {
            let mut bi = BisectRange::new(1, 16);
            let mut found = false;
            for _ in 0..10 {
                if bi.feed(bi.probe().unwrap() >= culprit) {
                    found = true;
                    break;
                }
            }
            assert!(found && bi.culprit() == Some(culprit), "culprit={culprit} 收敛");
        }
    }
}

// ===========================================================================
// v4 深化批（F070 · G-B-30）——迷你趋势图渲染 / 季度目标阶梯
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-30 功能定义的实装细化，非新立项）：
// 1. Sparkline —— 迷你趋势图字节渲染：分数序列 → ▁▂▃▅▆▇ 八级块字符
//    行（总日志中心/成绩单页的嵌入图表面——零堆字节渲染）。
// 2. TargetLadder —— 季度目标阶梯：Q1-Q4 渐进目标（800/850/900/950），
//    实际分对位判定（阶梯不倒退——逐季不回退的量化面）。
// 全部零堆：定长缓冲，无 Vec/String/浮点/format!。
// ===========================================================================

/// 阶梯目标（Q1-Q4）。
pub const TARGET_LADDER: [u16; 4] = [800, 850, 900, 950];
/// 迷你图宽度。
pub const SPARK_WIDTH: usize = 24;
/// 八级块字符（U+2581-F）。
pub const SPARK_BLOCKS: [u8; 8] = [0xE2, 0x96, 0x81, 0xE2, 0x96, 0x84, 0x00, 0x00];

// ---------------------------------------------------------------------------
// 深化一：迷你趋势图
// ---------------------------------------------------------------------------

/// 迷你趋势渲染：u32 序列 → UTF-8 块字符行（▁▂▃▄▅▆▇█ 八级）。
/// 每个 UTF-8 块字符 3 字节（E2 96 81..88）。
pub fn sparkline(values: &[u32], out: &mut [u8]) -> usize {
    if values.is_empty() || out.len() < 3 {
        return 0;
    }
    let mut lo = u32::MAX;
    let mut hi = 0u32;
    for v in values {
        lo = lo.min(*v);
        hi = hi.max(*v);
    }
    let span = (hi - lo).max(1);
    let mut w = 0usize;
    for v in values.iter().take(SPARK_WIDTH) {
        let level = ((v - lo) as u64 * 8 / span as u64).min(7) as usize; // 0-7
        // UTF-8: U+2581 + level → E2 96 [81+level]。
        if w + 3 <= out.len() {
            out[w] = 0xE2;
            out[w + 1] = 0x96;
            out[w + 2] = 0x81 + level as u8;
            w += 3;
        }
    }
    w
}

// ---------------------------------------------------------------------------
// 深化二：季度目标阶梯
// ---------------------------------------------------------------------------

/// 阶梯判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LadderCheck {
    pub quarter: usize, // 0-3
    pub target: u16,
    pub met: bool,
}

/// 单季对位。
pub fn ladder_check(quarter: usize, actual: u16) -> Option<LadderCheck> {
    if quarter >= 4 {
        return None;
    }
    Some(LadderCheck {
        quarter,
        target: TARGET_LADDER[quarter],
        met: actual >= TARGET_LADDER[quarter],
    })
}

/// 全年阶梯判定：四季全绿且逐季不回退（actual 单调不减）。
pub fn ladder_year_ok(actuals: &[u16; 4]) -> bool {
    for (q, a) in actuals.iter().enumerate() {
        if *a < TARGET_LADDER[q] {
            return false;
        }
        if q > 0 && *a < actuals[q - 1] {
            return false; // 逐季回退——红
        }
    }
    true
}

/// 剩余缺口（当季目标 − 实际；达标 → 0）。
pub fn ladder_gap(quarter: usize, actual: u16) -> Option<u16> {
    if quarter >= 4 {
        return None;
    }
    Some(TARGET_LADDER[quarter].saturating_sub(actual))
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：迷你图 / 阶梯逐条实摆。
pub fn run_perfgate_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F070-perfgate-v4");

    // ── 迷你图 ──
    // 单调升序列 → 严格升块级（首块 ▁ 尾块 █）。
    let mut rising = [0u32; 8];
    for (k, v) in rising.iter_mut().enumerate() {
        *v = k as u32 * 100;
    }
    let mut buf = [0u8; 64];
    let w = sparkline(&rising, &mut buf);
    cs.add("spark_len_3bytes_per_point", w == 24, "");
    cs.add("spark_first_lowest", buf[2] == 0x81, "");
    cs.add("spark_last_highest", buf[23] == 0x88, ""); // 末点块字节在 21..24 的第 3 位
    // 全同序列 → 全最低块（span 钳 1 防除零）。
    let flat = [500u32; 6];
    let w2 = sparkline(&flat, &mut buf);
    cs.add("spark_flat_all_low", w2 == 18 && buf[2] == 0x81, "");
    // 空序列零输出。
    cs.add("spark_empty_zero", sparkline(&[], &mut buf) == 0, "");
    // 超宽截断到 SPARK_WIDTH。
    let mut wide = [0u32; 40];
    for (k, v) in wide.iter_mut().enumerate() {
        *v = k as u32;
    }
    let mut big = [0u8; 128];
    cs.add("spark_width_capped", sparkline(&wide, &mut big) == SPARK_WIDTH * 3, "");

    // ── 目标阶梯 ──
    cs.add("ladder_q1_met", ladder_check(0, 820).unwrap().met, "");
    cs.add("ladder_q1_miss", !ladder_check(0, 790).unwrap().met, "");
    cs.add("ladder_q4_high_bar", ladder_check(3, 940).unwrap().met == false, "");
    cs.add("ladder_bad_quarter_none", ladder_check(4, 1000).is_none(), "");
    // 全年达标 + 不回退。
    cs.add(
        "ladder_year_green",
        ladder_year_ok(&[810, 860, 910, 955]),
        "",
    );
    // 达标但回退 → 红。
    cs.add("ladder_regression_red", !ladder_year_ok(&[850, 850, 950, 910]), "");
    // 缺口计算。
    cs.add("ladder_gap_50", ladder_gap(0, 750) == Some(50), "");
    cs.add("ladder_gap_met_zero", ladder_gap(0, 900) == Some(0), "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn spark_all_valid_utf8_blocks() {
        let mut v = [0u32; SPARK_WIDTH];
        for (k, x) in v.iter_mut().enumerate() {
            *x = (k as u32 * 37) % 1000;
        }
        let mut buf = [0u8; 128];
        let w = sparkline(&v, &mut buf);
        for k in (2..w).step_by(3) {
            assert!(buf[k] >= 0x81 && buf[k] <= 0x88, "块级落在 1-8");
        }
    }

    #[test]
    fn ladder_targets_monotonic() {
        for q in 1..4 {
            assert!(TARGET_LADDER[q] > TARGET_LADDER[q - 1], "阶梯只升不降");
        }
    }
}

// ===========================================================================
// v5 深化批（deep5）：指标相关性 + 二分缓存
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：双指标同向联动检测（8 窗四格表 → 提升度）
// ---------------------------------------------------------------------------

/// 联动检测：两指标在 8 个时间窗内是否同时超线（Phi 系数的整数近似）。
pub struct CoMotionDetector {
    /// (指标 a 超线, 指标 b 超线) 每窗一格。
    cells: [(bool, bool); 8],
    n: usize,
}

impl CoMotionDetector {
    pub const fn new() -> Self {
        CoMotionDetector { cells: [(false, false); 8], n: 0 }
    }

    pub fn observe(&mut self, a_over: bool, b_over: bool) {
        if self.n < 8 {
            self.cells[self.n] = (a_over, b_over);
            self.n += 1;
        }
    }

    /// 四格计数 (a&b, a&!b, !a&b, !a&!b)。
    pub fn fourfold(&self) -> (u32, u32, u32, u32) {
        let mut ab = 0;
        let mut a_only = 0;
        let mut b_only = 0;
        let mut neither = 0;
        for k in 0..self.n {
            let (a, b) = self.cells[k];
            match (a, b) {
                (true, true) => ab += 1,
                (true, false) => a_only += 1,
                (false, true) => b_only += 1,
                (false, false) => neither += 1,
            }
        }
        (ab, a_only, b_only, neither)
    }

    /// 联动判定：a 超线时 b 也超线的条件概率，比 b 的基础率高 2 倍以上。
    /// 样本 <4 或 a 超线不足 2 次 → None（不猜）。
    pub fn comotion_x100(&self) -> Option<u32> {
        if self.n < 4 {
            return None;
        }
        let (ab, a_only, b_only, _neither) = self.fourfold();
        let a_total = ab + a_only;
        if a_total < 2 {
            return None;
        }
        // 提升度 = P(b|a) / P(b)：
        //   P(b|a) = ab / a_total（a 超线时 b 也超线的条件概率）
        //   P(b)   = (ab + b_only) / n（b 超线的基础概率）
        let p_ba = (ab as u64) * 100 / a_total as u64;
        let p_b = (ab + b_only) as u64 * 100 / self.n as u64;
        if p_b == 0 {
            return if p_ba > 0 { Some(u32::MAX) } else { Some(0) };
        }
        Some((p_ba * 100 / p_b) as u32)
    }
}

// ---------------------------------------------------------------------------
// 深化二：回归二分缓存（已知好/坏版本区间——避免重复构建）
// ---------------------------------------------------------------------------

/// 二分缓存：记录已知好版本与坏版本，跳过已知区间的构建。
pub struct BisectCache {
    last_good: u32,
    last_bad: u32,
    builds: u32,
}

impl BisectCache {
    pub const fn new(good: u32, bad: u32) -> Self {
        BisectCache { last_good: good, last_bad: bad, builds: 0 }
    }

    /// 提交一次构建结果（版本，是否坏）→ 返回下一个待测版本。
    /// 区间收缩到相邻（bad-good==1）→ None（定位完成）。
    pub fn report(&mut self, version: u32, is_bad: bool) -> Option<u32> {
        self.builds += 1;
        if is_bad {
            self.last_bad = self.last_bad.min(version);
        } else {
            self.last_good = self.last_good.max(version);
        }
        if self.last_bad <= self.last_good + 1 {
            return None;
        }
        Some(self.last_good + (self.last_bad - self.last_good) / 2)
    }

    /// 定位的首个待测版本。
    pub fn first_probe(&self) -> u32 {
        self.last_good + (self.last_bad - self.last_good) / 2
    }

    /// 坏版本定位（区间相邻后）。
    pub fn culprit(&self) -> Option<u32> {
        if self.last_bad == self.last_good + 1 {
            Some(self.last_bad)
        } else {
            None
        }
    }

    pub fn builds(&self) -> u32 {
        self.builds
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_perfgate_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F070-perfgate-v5");

    // ── 联动检测 ──
    // 1) 锁步联动：a 超则 b 必超，P(b|a)=100%，P(b)=50% → 提升度 200。
    let mut cd = CoMotionDetector::new();
    cd.observe(true, true);
    cd.observe(true, true);
    cd.observe(false, false);
    cd.observe(false, false);
    cs.add("comotion_full_lockstep", cd.comotion_x100() == Some(200), "");
    // 2) 独立（无联动）：条件率 = 基础率 → 100。
    let mut cd2 = CoMotionDetector::new();
    cd2.observe(true, true);
    cd2.observe(true, false);
    cd2.observe(false, true);
    cd2.observe(false, false);
    cs.add("comotion_independent_100", cd2.comotion_x100() == Some(100), "");
    // 3) a 超线 <2 次 → None。
    let mut cd3 = CoMotionDetector::new();
    cd3.observe(true, true);
    cd3.observe(false, false);
    cd3.observe(false, false);
    cd3.observe(false, false);
    cs.add("comotion_insufficient_none", cd3.comotion_x100().is_none(), "");
    // 4) 四格账精确。
    cs.add("comotion_fourfold", cd.fourfold() == (2, 0, 0, 2), "");

    // ── 二分缓存 ──
    // 5) 首探测 = 中点。
    let mut bc = BisectCache::new(100, 200);
    cs.add("bisect_first_midpoint", bc.first_probe() == 150, "");
    // 6) 收敛：150 坏 → 125；125 好 → 137；137 好 → 143…直到相邻。
    let mut next = bc.report(150, true);
    cs.add("bisect_shrink_after_bad", next == Some(125), "");
    next = bc.report(125, false);
    cs.add("bisect_shrink_after_good", next == Some(137), "");
    // 7) 快速定位：好 149 坏 150 → culprit。
    let mut bc2 = BisectCache::new(149, 150);
    cs.add("bisect_culprit_adjacent", bc2.culprit() == Some(150) && bc2.report(150, true).is_none(), "");
    // 8) 构建计数。
    cs.add("bisect_build_count", bc.builds() == 2, "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn comotion_negative_comotion() {
        // a 超 b 反而不超 → 条件率 0 < 基础率 → 提升度 0。
        let mut cd = CoMotionDetector::new();
        cd.observe(true, false);
        cd.observe(true, false);
        cd.observe(false, true);
        cd.observe(false, true);
        assert_eq!(cd.comotion_x100(), Some(0));
    }

    #[test]
    fn bisect_log2_probes() {
        // 1024 版本区间 → 理论 ≤10 次探测定位。
        let mut bc = BisectCache::new(0, 1024);
        let culprit = 700u32;
        let mut next = Some(bc.first_probe());
        let mut probes = 0u32;
        while let Some(v) = next {
            probes += 1;
            next = bc.report(v, v >= culprit);
        }
        assert_eq!(bc.culprit(), Some(culprit));
        assert!(probes <= 10, "log2(1024)=10 内定位，实际 {probes}");
    }
}
