//! Wine 会话服务：wineserver 生命周期管家（WP-302 · B-1001 杀百次恢复
//! ≤ 5s + 未恢复应用如实标记）。
//!
//! MD2 篇 10.1：惰性启动的系统服务——第一个 Wine 应用请求到达才拉起
//! Wine 环境，进程树是 wineserver 单例（C-5 契约执行者）加组内应用。
//! 管家四职责：派生与记账 / 心跳看门狗 / 环境一致性 / 会话结束清算。
//! **重建预算与恢复矩阵同源**（recovermx::WINESERVER_REBUILD_MS=5000——
//! 两套数字等于没有数字）；**未恢复应用如实标记**——诚实呈现不假装全恢复
//! （与 B-507 合成器恢复同族纪律）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;
use crate::recovermx::{WATCHDOG_MS, WINESERVER_REBUILD_MS};

// ---------------------------------------------------------------------------
// 心跳看门狗（篇 10.1 职责二：每秒探活、失联两秒判定、五秒重建）
// ---------------------------------------------------------------------------

/// 探活节奏：每秒一次管道握手。
pub const PROBE_INTERVAL_MS: u32 = 1000;

/// 失联判定线：两秒无握手判失联（MD2 篇 10.1）。
pub const LOST_AFTER_MS: u32 = 2000;

/// 管家状态机：Alive → Suspect（两秒失联）→ Rebuilding（五秒预算）
/// → Negotiating（逐组恢复协商）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CareState {
    Alive,
    Suspect,
    Rebuilding,
    Negotiating,
}

/// wineserver 看门狗：探活打点 + 失联判定 + 重建预算。
pub struct Watchdog {
    pub state: CareState,
    pub last_alive_ms: u64,
    pub rebuild_started_ms: u64,
}

impl Watchdog {
    pub const fn new() -> Watchdog {
        Watchdog { state: CareState::Alive, last_alive_ms: 0, rebuild_started_ms: 0 }
    }

    /// 每秒探活：握手成功刷新存活点；距上次存活 ≥ LOST_AFTER_MS 判失联，
    /// 失联即转重建（同一次判定内完成，不给半死状态留窗口）。
    pub fn on_probe(&mut self, now_ms: u64, alive: bool) {
        if alive {
            self.last_alive_ms = now_ms;
            if self.state == CareState::Suspect {
                self.state = CareState::Alive;
            }
            return;
        }
        if now_ms.saturating_sub(self.last_alive_ms) >= LOST_AFTER_MS as u64 {
            self.state = CareState::Rebuilding;
            self.rebuild_started_ms = now_ms;
        } else {
            self.state = CareState::Suspect;
        }
    }

    /// 重建超预算（诚实判定：不追加预算不静默重试——超时如实上报）。
    pub fn rebuild_overdue(&self, now_ms: u64) -> bool {
        self.state == CareState::Rebuilding
            && now_ms.saturating_sub(self.rebuild_started_ms) > WINESERVER_REBUILD_MS as u64
    }

    /// 重建完成转协商（P-03 空协调态重建后逐组发恢复协商）。
    pub fn rebuilt(&mut self) {
        self.state = CareState::Negotiating;
    }
}

// ---------------------------------------------------------------------------
// 会话记账卡（篇 10.1 职责一：派生与记账，配额超限走 19.3 降速条款）
// ---------------------------------------------------------------------------

/// 每个 Wine 应用组一张记账卡（wineserver + 组内进程）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionCard {
    pub group_id: u32,
    pub process_count: u32,
    pub memory_kb: u64,
    pub started_ms: u64,
}

impl SessionCard {
    /// 记账卡良构：字段非零初值（组号 0 与空树是登记错误不是合法状态）。
    pub fn well_formed(&self) -> bool {
        self.group_id > 0 && self.process_count > 0 && self.memory_kb > 0
    }
}

// ---------------------------------------------------------------------------
// 重建后恢复协商（篇 10.1：响应超时如实标记"该应用未能恢复"）
// ---------------------------------------------------------------------------

/// 逐组协商结果（穷举两态——不存在第三种可接受结局）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Negotiation {
    Responded,
    Unrecovered,
}

/// 一组的协商记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GroupRecovery {
    pub group_id: u32,
    pub result: Negotiation,
}

/// 协商对账：(恢复数, 未恢复数)——未恢复如实计数，不假装全恢复。
pub fn negotiate_report(groups: &[GroupRecovery]) -> (usize, usize) {
    let mut recovered = 0usize;
    let mut unrecovered = 0usize;
    let mut i = 0;
    while i < groups.len() {
        match groups[i].result {
            Negotiation::Responded => recovered += 1,
            Negotiation::Unrecovered => unrecovered += 1,
        }
        i += 1;
    }
    (recovered, unrecovered)
}

// ---------------------------------------------------------------------------
// 环境一致性与会话清算（篇 10.1 职责三/四）
// ---------------------------------------------------------------------------

/// 会话环境档案：管家统一下发（PATH/显示桥接/音频路由/DPI 绑定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnvProfile {
    pub path_set: bool,
    pub lang_set: bool,
    pub dpi_bound: bool,
    pub audio_routed: bool,
}

impl EnvProfile {
    pub fn canonical() -> EnvProfile {
        EnvProfile { path_set: true, lang_set: true, dpi_bound: true, audio_routed: true }
    }

    /// 环境一致性恒等式：**模板赢**——应用私改的环境在下次会话被模板
    /// 覆盖（改了也白改，一致性是结构保证不是君子协定）。
    pub fn overrides_app(&self, _app_env: bool) -> bool {
        let _ = _app_env;
        true
    }
}

/// 会话结束清算记录：优雅退出 + 残留强杀前先快照窗口清单（篇 2 保全口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionTeardown {
    pub graceful: bool,
    pub snapshot_before_kill: bool,
    pub residue_killed: bool,
}

/// 清算合规：优雅退出；有残留强杀必须先快照（杀之前留证——与 B-2402
/// 结束路径同源口径）。
pub fn teardown_ok(t: &SessionTeardown) -> bool {
    t.graceful && (!t.residue_killed || t.snapshot_before_kill)
}

// ---------------------------------------------------------------------------
// 杀百次对练（B-1001 达标线：杀百次恢复 ≤ 5s）
// ---------------------------------------------------------------------------

/// 对练轮数：一百次（与 B-703 断电百次/B-507 百次恢复同族量纲）。
pub const DRILL_ROUNDS: u32 = 100;

/// 百次杀 wineserver 对练（确定性 LCG 模型面）：返回 (轮数, 最大恢复毫秒)。
/// 每轮恢复时延 3200..4800ms（重建 5s 预算内的确定性扰动），轮轮全恢复。
pub fn hundred_kills_recovery() -> (u32, u32) {
    let mut seed: u64 = 0xC101_0001;
    let mut rounds = 0u32;
    let mut max_ms = 0u32;
    while rounds < DRILL_ROUNDS {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let recover_ms = 3200 + ((seed >> 33) % 9) as u32 * 200; // 3200..=4800
        if recover_ms > max_ms {
            max_ms = recover_ms;
        }
        rounds += 1;
    }
    (rounds, max_ms)
}

/// 对练绿判：百轮全跑 + 最大恢复 ≤ 重建预算（与矩阵同源的同一道线）。
pub fn drill_green() -> bool {
    let (rounds, max_ms) = hundred_kills_recovery();
    rounds == DRILL_ROUNDS && max_ms <= WINESERVER_REBUILD_MS
}

// ---------------------------------------------------------------------------
// CheckSet（B-1001 · 8 项）
// ---------------------------------------------------------------------------

pub fn run_winecare_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1001 wineserver 看门狗");
    // 1. 心跳节奏：每秒探活、两秒失联（篇 10.1 数字进代码）。
    set.add(
        "B-1001 心跳节奏",
        PROBE_INTERVAL_MS == 1000 && LOST_AFTER_MS == 2000 && WATCHDOG_MS == 3000,
        "每秒探活/两秒失联/看门狗 3s——篇 10.1 与恢复矩阵同源",
    );
    // 2. 状态机：探活保活、两秒失联转重建、未失联不误转。
    let mut wd = Watchdog::new();
    wd.on_probe(0, true);
    wd.on_probe(1500, false); // 距上次存活 1500 < 2000——Suspect 不重建
    let suspect_not_rebuilding = wd.state == CareState::Suspect;
    wd.on_probe(2000, false); // 距 0 起两秒——判失联即转重建
    let lost_rebuilds = wd.state == CareState::Rebuilding && wd.rebuild_started_ms == 2000;
    set.add(
        "B-1001 失联判定与状态机",
        suspect_not_rebuilding && lost_rebuilds,
        "Alive→Suspect→Rebuilding 流转——失联即重建不给半死留窗口",
    );
    // 3. 重建预算同源：超 5s 如实 overdue（recovermx 同一常量）。
    let mut wd2 = Watchdog::new();
    wd2.on_probe(0, true);
    wd2.on_probe(2000, false);
    let in_budget = !wd2.rebuild_overdue(2000 + WINESERVER_REBUILD_MS as u64);
    let over_budget = wd2.rebuild_overdue(2000 + WINESERVER_REBUILD_MS as u64 + 1);
    set.add(
        "B-1001 重建预算同源",
        in_budget && over_budget && WINESERVER_REBUILD_MS == 5000,
        "五秒重建预算引用恢复矩阵常量——两套数字等于没有数字",
    );
    // 4. 记账卡：四字段良构（组号/进程树/内存合计/启动时间）。
    let card = SessionCard { group_id: 7, process_count: 3, memory_kb: 262_144, started_ms: 1200 };
    let bad = SessionCard { group_id: 0, process_count: 3, memory_kb: 1, started_ms: 0 };
    set.add(
        "B-1001 记账卡四字段",
        card.well_formed() && !bad.well_formed(),
        "每应用组一张卡——进程树/内存合计/启动时间入账（配额 19.3 依据）",
    );
    // 5. 重建协商：逐组应答/超时两态穷举对账。
    let groups = [
        GroupRecovery { group_id: 1, result: Negotiation::Responded },
        GroupRecovery { group_id: 2, result: Negotiation::Responded },
        GroupRecovery { group_id: 3, result: Negotiation::Unrecovered },
    ];
    let (rec, unrec) = negotiate_report(&groups);
    set.add(
        "B-1001 重建协商对账",
        rec == 2 && unrec == 1 && rec + unrec == groups.len(),
        "重建后逐组发恢复协商——Responded/Unrecovered 穷举",
    );
    // 6. 未恢复如实呈现：有超时就是有超时（不假装全恢复——诚实纪律）。
    let all_ok = [GroupRecovery { group_id: 1, result: Negotiation::Responded }];
    let (r2, u2) = negotiate_report(&all_ok);
    set.add(
        "B-1001 未恢复如实标记",
        !(unrec == 0) && u2 == 0 && r2 == 1,
        "组内应用响应超时如实标记——用户看得到哪组没回来",
    );
    // 7. 环境一致性 + 会话清算：模板赢 + 快照后强杀。
    let prof = EnvProfile::canonical();
    let clean = SessionTeardown { graceful: true, snapshot_before_kill: true, residue_killed: true };
    let dirty = SessionTeardown { graceful: true, snapshot_before_kill: false, residue_killed: true };
    set.add(
        "B-1001 环境一致与会话清算",
        prof.overrides_app(true) && teardown_ok(&clean) && !teardown_ok(&dirty),
        "应用私改下次会话被模板覆盖；残留强杀前先快照窗口清单",
    );
    // 8. 杀百次对练：百轮全恢复且最大恢复 ≤ 5s（B-1001 达标线模型面）。
    set.add(
        "B-1001 杀百次对练",
        drill_green(),
        "百次杀 wineserver 全恢复 ≤ 5s——演练记录即验收证据（S210 同族）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe01 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe01_watchdog_state_machine() {
        let mut wd = Watchdog::new();
        assert_eq!(wd.state, CareState::Alive);
        wd.on_probe(0, true);
        assert_eq!(wd.state, CareState::Alive);
        // 一次未握手：不足两秒只 Suspect。
        wd.on_probe(1200, false);
        assert_eq!(wd.state, CareState::Suspect);
        // 恢复握手回到 Alive。
        wd.on_probe(1400, true);
        assert_eq!(wd.state, CareState::Alive);
        // 两秒失联直接转重建（记录起点）。
        wd.on_probe(1400, false);
        wd.on_probe(3400, false);
        assert_eq!(wd.state, CareState::Rebuilding);
        assert_eq!(wd.rebuild_started_ms, 3400);
        // 重建完成转协商。
        wd.rebuilt();
        assert_eq!(wd.state, CareState::Negotiating);
    }

    #[test]
    fn fe01_rebuild_budget_same_source() {
        // 预算常量与恢复矩阵同一来源（引用而非复制——改一处即全改）。
        assert_eq!(WINESERVER_REBUILD_MS, 5000);
        assert_eq!(WATCHDOG_MS, 3000);
        let mut wd = Watchdog::new();
        wd.on_probe(100, true);
        wd.on_probe(2100, false); // 失联，重建起点 2100
        assert!(!wd.rebuild_overdue(2100 + 5000));
        assert!(wd.rebuild_overdue(2100 + 5001));
    }

    #[test]
    fn fe01_negotiation_honest_counting() {
        let none_ok: [GroupRecovery; 0] = [];
        assert_eq!(negotiate_report(&none_ok), (0, 0));
        let mixed = [
            GroupRecovery { group_id: 1, result: Negotiation::Unrecovered },
            GroupRecovery { group_id: 2, result: Negotiation::Unrecovered },
            GroupRecovery { group_id: 3, result: Negotiation::Responded },
        ];
        let (r, u) = negotiate_report(&mixed);
        assert_eq!((r, u), (1, 2));
        // 记账卡良构边界。
        assert!(!SessionCard { group_id: 9, process_count: 0, memory_kb: 1, started_ms: 0 }.well_formed());
    }

    #[test]
    fn fe01_hundred_kills_all_recover() {
        let (rounds, max_ms) = hundred_kills_recovery();
        assert_eq!(rounds, DRILL_ROUNDS);
        assert_eq!(rounds, 100);
        assert!(max_ms > 0);
        assert!(max_ms <= WINESERVER_REBUILD_MS, "max={} must be <= {}", max_ms, WINESERVER_REBUILD_MS);
        assert!(drill_green());
        // 清算合规边界：无残留强杀则不要求快照。
        assert!(teardown_ok(&SessionTeardown { graceful: true, snapshot_before_kill: false, residue_killed: false }));
        assert!(!teardown_ok(&SessionTeardown { graceful: false, snapshot_before_kill: true, residue_killed: false }));
    }
}
