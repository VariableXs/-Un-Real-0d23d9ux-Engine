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
