//! F181 交接预检器（secstar · G-G-11）——闸门从内核逻辑长成了看得见的门。
//!
//! 主册判据（验收标准第一句）：
//! **三查注入（正常/目标缺失/哈希坏）三态面板正确；全绿通行延迟 <1s；拦停路径审计留痕。**
//!
//! 功能定义（G-G-11）：切 Windows 域前自动预检（防自锁闸门三条件的用户
//! 可见化）：目标引导链存在/哈希可信/回路可回三查逐条显示；预检不过 →
//! 修复建议而非放行（R1 流程前置到 UX 层）。
//!
//! 【交互设计】预检面板 400×240px：三行检查项（图标+名称+结果）；全绿自动
//! 进入交接（500ms 停留确认感）；有红 → 停止+分诊文案（哪条坏/可能原因/
//! 建议动作）；「仍要继续」需 F037 式 3s 置灰+审计。
//! 【数据与存储】预检结果入审计（与交接快照关联）；无独立存储。
//! 【状态与异常】三查实现复用防自锁闸门既有逻辑（内核面同源——UX 不另造
//! 标准）；预检自身异常 → 保守拦停（预检失败=不放行——fail-closed）。
//! 【设计细节】检查项文案三条白话：「目标存在：Windows 引导链 WINESP」
//! 「哈希可信：引导文件与登记一致」「回路可回：VARIX 引导项完好」——每条
//! =一条闸门条件的白话；500ms 停留是「确认感」设计（太快像没检——微交互
//! 心理学登记 F124 注记）；红态色=危险令牌（F151）；与 F039 游戏分流共用
//! 预检（一次检两处用）。
//!
//! 时序预算（<1s 全绿通行硬线）：三查 ≤350ms（执行窗）+ 全绿确认停留
//! 500ms = 850ms 预算内——逐拍对账。
//!
//! 零堆纪律：定长审计环 + 定长面板模型，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 预检面板 400×240px。
pub const PANEL_W: u32 = 400;
pub const PANEL_H: u32 = 240;
/// 全绿确认停留 500ms（确认感设计——太快像没检）。
pub const ALLGREEN_HOLD_MS: u64 = 500;
/// 三查执行窗预算 350ms（+500ms 停留 = 850ms < 1s 通行硬线）。
pub const CHECKS_BUDGET_MS: u64 = 350;
/// 「仍要继续」危险钮置灰 3s（F037 式）。
pub const OVERRIDE_GRAY_MS: u64 = 3_000;
/// 审计环容量（定长——留痕永不越界）。
pub const AUDIT_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 三查模型（复用防自锁闸门三条件——UX 不另造标准）
// ---------------------------------------------------------------------------

/// 三查项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckId {
    /// 目标存在：Windows 引导链 WINESP。
    TargetExists,
    /// 哈希可信：引导文件与登记一致。
    HashTrusted,
    /// 回路可回：VARIX 引导项完好。
    ReturnPathOk,
}

impl CheckId {
    pub const ALL: [CheckId; 3] = [CheckId::TargetExists, CheckId::HashTrusted, CheckId::ReturnPathOk];
    /// 检查项白话文案（每条=一条闸门条件的白话）。
    pub fn copy(self) -> &'static str {
        match self {
            CheckId::TargetExists => "目标存在：Windows 引导链 WINESP",
            CheckId::HashTrusted => "哈希可信：引导文件与登记一致",
            CheckId::ReturnPathOk => "回路可回：VARIX 引导项完好",
        }
    }
    pub fn ord(self) -> u8 {
        match self {
            CheckId::TargetExists => 0,
            CheckId::HashTrusted => 1,
            CheckId::ReturnPathOk => 2,
        }
    }
}

/// 单查结果态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckState {
    Green,
    /// 红（含分诊：哪条坏/可能原因/建议动作）。
    Red,
    /// 预检自身异常 → 保守拦停（fail-closed——预检失败=不放行）。
    Exception,
}

/// 分诊文案（红态三要素：哪条坏/可能原因/建议动作）。
pub fn triage_copy(id: CheckId) -> (&'static str, &'static str, &'static str) {
    match id {
        CheckId::TargetExists => (
            "引导目标校验失败",
            "WINESP 分区可能被外部改动或移除",
            "查看详情，或运行引导修复后再交接",
        ),
        CheckId::HashTrusted => (
            "引导文件哈希与登记不一致",
            "引导文件可能被篡改或损坏",
            "查看差异详情，确认后再决定修复",
        ),
        CheckId::ReturnPathOk => (
            "VARIX 引导项异常",
            "回路引导项可能丢失，交接后可能无法返回",
            "先修复 VARIX 引导项（恢复环境 F198）",
        ),
    }
}

/// 三查注入器（内核防自锁闸门结果的显式注入口——UX 不另造标准）。
pub trait GateProbe {
    /// 执行一查：返回该查状态与耗时 ms。
    fn probe(&mut self, id: CheckId) -> (CheckState, u64);
}

// ---------------------------------------------------------------------------
// 预检状态机
// ---------------------------------------------------------------------------

/// 预检面板状态机。
/// Idle → Checking（三查逐条）→ AllGreenHold(500ms) → Proceed
///                                       → Stopped{red 分诊} → Override 灰 3s → Armed → ProceedOverride
/// 任一 Exception → Blocked（fail-closed，无越权出路）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    Idle,
    Checking,
    /// 全绿确认停留（确认感设计）。
    AllGreenHold,
    /// 已放行（全绿自动通行）。
    Proceed,
    /// 拦停（红态——修复建议而非放行）。
    Stopped,
    /// 危险钮置灰中（3s）。
    OverrideGray,
    /// 危险钮已武装（可按下）。
    OverrideArmed,
    /// 越权通行（审计留痕）。
    ProceedOverride,
    /// 预检自身异常 → 保守拦停（无出路——fail-closed）。
    Blocked,
}

/// 面板运行时。
pub struct PrecheckPanel {
    pub flow: Flow,
    /// 三查结果（索引=CheckId::ord）。
    pub states: [CheckState; 3],
    /// 三查耗时合计（执行窗预算对账——注入台架回填）。
    checks_elapsed_ms: u64,
    /// AllGreenHold 起点。
    hold_start_ms: u64,
    /// OverrideGray 起点。
    gray_start_ms: u64,
    /// 审计环。
    audit: [Option<AuditEntry>; AUDIT_CAP],
    audit_n: usize,
}

/// 审计条目（拦停路径与越权通行全部留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditEntry {
    pub day: u32,
    /// 动作：0=全绿通行 1=拦停 2=越权通行 3=fail-closed 拦停。
    pub action: u8,
    /// 三查快照（0 绿/1 红/2 异常 × 3 查打包为 6bit）。
    pub snapshot6: u8,
}

impl PrecheckPanel {
    pub const fn new() -> PrecheckPanel {
        PrecheckPanel {
            flow: Flow::Idle,
            states: [CheckState::Green; 3],
            checks_elapsed_ms: 0,
            hold_start_ms: 0,
            gray_start_ms: 0,
            audit: [const { None }; AUDIT_CAP],
            audit_n: 0,
        }
    }

    fn audit_push(&mut self, day: u32, action: u8) {
        if self.audit_n < AUDIT_CAP {
            let snap: u8 = self
                .states
                .iter()
                .enumerate()
                .fold(0u8, |acc, (i, s)| acc | (match s { CheckState::Green => 0u8, CheckState::Red => 1, CheckState::Exception => 2 } << (i * 2)));
            self.audit[self.audit_n] = Some(AuditEntry { day, action, snapshot6: snap });
            self.audit_n += 1;
        }
    }

    /// 审计环读取（拦停路径审计留痕的对账口）。
    pub fn audit_entries(&self) -> impl Iterator<Item = &AuditEntry> {
        self.audit[..self.audit_n].iter().flatten()
    }

    /// 开始预检（三查注入执行——逐条回填；耗时合计进预算对账）。
    pub fn begin(&mut self, _now_ms: u64, probe: &mut dyn GateProbe) {
        self.flow = Flow::Checking;
        let mut cost = 0u64;
        for id in CheckId::ALL {
            let (state, c) = probe.probe(id);
            cost += c;
            self.states[id.ord() as usize] = state;
        }
        self.checks_elapsed_ms = cost;
    }

    /// 时间一拍（驱动停留/置灰时序）。
    pub fn tick(&mut self, now_ms: u64, day: u32) {
        match self.flow {
            Flow::Checking => {
                if self.states.iter().any(|s| *s == CheckState::Exception) {
                    self.flow = Flow::Blocked;
                    self.audit_push(day, 3);
                } else if self.states.iter().any(|s| *s == CheckState::Red) {
                    self.flow = Flow::Stopped;
                    self.audit_push(day, 1);
                } else {
                    self.flow = Flow::AllGreenHold;
                    self.hold_start_ms = now_ms;
                }
            }
            Flow::AllGreenHold => {
                if now_ms.saturating_sub(self.hold_start_ms) >= ALLGREEN_HOLD_MS {
                    self.flow = Flow::Proceed;
                    self.audit_push(day, 0);
                }
            }
            Flow::OverrideGray => {
                if now_ms.saturating_sub(self.gray_start_ms) >= OVERRIDE_GRAY_MS {
                    self.flow = Flow::OverrideArmed;
                }
            }
            _ => {}
        }
    }

    /// 拦停后用户选择「仍要继续」：按下（置灰期间无效——3s 防手滑）。
    pub fn override_press(&mut self, now_ms: u64) -> bool {
        if self.flow == Flow::Stopped {
            self.flow = Flow::OverrideGray;
            self.gray_start_ms = now_ms;
            true
        } else {
            false
        }
    }

    /// 置灰期满后确认越权（审计留痕）。
    pub fn override_confirm(&mut self, day: u32) -> bool {
        if self.flow == Flow::OverrideArmed {
            self.flow = Flow::ProceedOverride;
            self.audit_push(day, 2);
            true
        } else {
            false
        }
    }

    /// 全绿通行延迟对账：三查 + 500ms 停留 < 1s。
    pub fn proceed_latency_ok(&self) -> bool {
        self.checks_elapsed_ms + ALLGREEN_HOLD_MS < 1_000 && self.checks_elapsed_ms <= CHECKS_BUDGET_MS
    }

    /// F039 游戏分流共用预检（一次检两处用——同引擎不同标签）。
    pub fn game_shunt_label() -> &'static str {
        "游戏分流预检（F039 · 同引擎）"
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 三查注入台架：脚本化三查结果（主册三态注入——正常/目标缺失/哈希坏）。
struct ScriptProbe {
    script: [CheckState; 3],
    cost_ms: u64,
}

impl GateProbe for ScriptProbe {
    fn probe(&mut self, id: CheckId) -> (CheckState, u64) {
        (self.script[id.ord() as usize], self.cost_ms)
    }
}

/// 域自检。
#[inline(never)]
pub fn run_handoffchk_checks() -> CheckSet {
    let mut cs = CheckSet::new("F181-handoffchk");

    // 1) 正常注入 → 三绿 → 500ms 停留 → 自动放行（全绿通行路径）。
    let mut p = PrecheckPanel::new();
    let mut probe = ScriptProbe { script: [CheckState::Green; 3], cost_ms: 100 };
    p.begin(0, &mut probe);
    p.tick(0, 1);
    let mid = p.flow == Flow::AllGreenHold;
    p.tick(499, 1);
    let not_yet = p.flow == Flow::AllGreenHold;
    p.tick(500, 1);
    cs.add("allgreen_auto_proceed", mid && not_yet && p.flow == Flow::Proceed, "");

    // 2) 全绿通行延迟 <1s（三查 350ms 预算 + 500ms 停留对账）。
    cs.add("proceed_latency_under_1s", p.proceed_latency_ok(), "");

    // 3) 目标缺失注入 → 拦停 + 分诊文案三要素（哪条坏/原因/建议）。
    let mut p2 = PrecheckPanel::new();
    let mut probe2 = ScriptProbe { script: [CheckState::Red, CheckState::Green, CheckState::Green], cost_ms: 100 };
    p2.begin(0, &mut probe2);
    p2.tick(0, 2);
    let (what, why, next) = triage_copy(CheckId::TargetExists);
    cs.add(
        "target_missing_triage",
        p2.flow == Flow::Stopped && p2.states[0] == CheckState::Red && !what.is_empty() && !why.is_empty() && !next.is_empty(),
        "",
    );

    // 4) 哈希坏注入 → 拦停（哈希可信检查红）。
    let mut p3 = PrecheckPanel::new();
    let mut probe3 = ScriptProbe { script: [CheckState::Green, CheckState::Red, CheckState::Green], cost_ms: 100 };
    p3.begin(0, &mut probe3);
    p3.tick(0, 3);
    cs.add("hash_bad_stopped", p3.flow == Flow::Stopped && p3.states[1] == CheckState::Red, "");

    // 5) 预检自身异常 → fail-closed 拦停（无越权出路——Blocked 不可 override）。
    let mut p4 = PrecheckPanel::new();
    let mut probe4 = ScriptProbe { script: [CheckState::Green, CheckState::Exception, CheckState::Green], cost_ms: 100 };
    p4.begin(0, &mut probe4);
    p4.tick(0, 4);
    let blocked = p4.flow == Flow::Blocked;
    let no_override = !p4.override_press(1_000);
    cs.add("exception_fail_closed", blocked && no_override, "");

    // 6) 拦停路径审计留痕（Stopped 动作=1 入环）。
    cs.add("stop_audit_trail", p2.audit_entries().any(|e| e.action == 1 && e.day == 2), "");

    // 7) 越权通行链：3s 置灰 → 武装 → 确认 → 审计留痕（动作=2）。
    let mut p5 = PrecheckPanel::new();
    let mut probe5 = ScriptProbe { script: [CheckState::Red, CheckState::Green, CheckState::Green], cost_ms: 100 };
    p5.begin(0, &mut probe5);
    p5.tick(0, 5);
    let pressed = p5.override_press(1_000);
    let gray = p5.flow == Flow::OverrideGray;
    p5.tick(1_000 + 2_999, 5);
    let still_gray = p5.flow == Flow::OverrideGray;
    p5.tick(1_000 + 3_000, 5);
    let armed = p5.flow == Flow::OverrideArmed;
    let confirmed = p5.override_confirm(5);
    cs.add(
        "override_3s_gray_audit",
        pressed && gray && still_gray && armed && confirmed && p5.flow == Flow::ProceedOverride
            && p5.audit_entries().any(|e| e.action == 2 && e.day == 5),
        "",
    );

    // 8) 全绿通行也有审计（动作=0——交接快照关联锚）。
    cs.add("proceed_audit_trail", p.audit_entries().any(|e| e.action == 0 && e.day == 1), "");

    // 9) 三查白话文案齐（每条=一条闸门条件的白话）。
    cs.add(
        "check_copy_plain_language",
        CheckId::ALL.iter().all(|id| !id.copy().is_empty())
            && CheckId::TargetExists.copy().contains("目标存在")
            && CheckId::HashTrusted.copy().contains("哈希可信")
            && CheckId::ReturnPathOk.copy().contains("回路可回"),
        "",
    );

    // 10) 面板几何与预算常量（400×240、500ms 停留、3s 置灰、350ms 查询预算）。
    cs.add(
        "panel_constants",
        PANEL_W == 400 && PANEL_H == 240 && ALLGREEN_HOLD_MS == 500 && OVERRIDE_GRAY_MS == 3_000 && CHECKS_BUDGET_MS == 350,
        "",
    );

    // 11) F039 共用预检标签在位（一次检两处用）。
    cs.add("game_shunt_shared", PrecheckPanel::game_shunt_label().contains("F039"), "");

    // 12) 审计环满不越界（32 条后静默封口——留痕纪律不崩预检）。
    let mut p6 = PrecheckPanel::new();
    let mut probe6 = ScriptProbe { script: [CheckState::Green; 3], cost_ms: 10 };
    for d in 0..40u32 {
        p6.begin((d as u64) * 1_000, &mut probe6);
        p6.tick((d as u64) * 1_000, d);
        p6.tick((d as u64) * 1_000 + 600, d);
    }
    cs.add("audit_ring_capped", p6.audit_n == AUDIT_CAP, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_full_lifecycle() {
        // Idle→Checking→AllGreenHold→Proceed 全生命周期推进。
        let mut p = PrecheckPanel::new();
        assert_eq!(p.flow, Flow::Idle);
        let mut probe = ScriptProbe { script: [CheckState::Green; 3], cost_ms: 50 };
        p.begin(10, &mut probe);
        assert_eq!(p.flow, Flow::Checking);
        p.tick(10, 1);
        assert_eq!(p.flow, Flow::AllGreenHold);
        p.tick(10 + ALLGREEN_HOLD_MS, 1);
        assert_eq!(p.flow, Flow::Proceed);
    }

    #[test]
    fn return_path_red_triage() {
        // 回路断 → 分诊指向恢复环境（F198 链——建议动作含修复指引）。
        let mut p = PrecheckPanel::new();
        let mut probe = ScriptProbe { script: [CheckState::Green, CheckState::Green, CheckState::Red], cost_ms: 10 };
        p.begin(0, &mut probe);
        p.tick(0, 1);
        assert_eq!(p.flow, Flow::Stopped);
        let (_, _, next) = triage_copy(CheckId::ReturnPathOk);
        assert!(next.contains("修复"));
    }

    #[test]
    fn override_never_bypasses_exception() {
        // Exception 态下 override_press 无效（fail-closed 无越权出路）。
        let mut p = PrecheckPanel::new();
        let mut probe = ScriptProbe { script: [CheckState::Exception; 3], cost_ms: 10 };
        p.begin(0, &mut probe);
        p.tick(0, 1);
        assert_eq!(p.flow, Flow::Blocked);
        assert!(!p.override_press(9_999));
        assert!(!p.override_confirm(1));
    }

    #[test]
    fn audit_snapshot6_encoding() {
        // 快照 6bit 打包：三查状态逐 2bit 编码可逆。
        let mut p = PrecheckPanel::new();
        let mut probe = ScriptProbe { script: [CheckState::Red, CheckState::Green, CheckState::Exception], cost_ms: 10 };
        p.begin(0, &mut probe);
        p.tick(0, 7);
        let e = p.audit_entries().next().unwrap();
        assert_eq!(e.snapshot6 & 0b11, 0b01); // TargetExists=Red
        assert_eq!((e.snapshot6 >> 2) & 0b11, 0b00); // HashTrusted=Green
        assert_eq!((e.snapshot6 >> 4) & 0b11, 0b10); // ReturnPathOk=Exception
    }

    #[test]
    fn proceed_requires_full_hold() {
        // 停留期内不放行（确认感设计——499ms 不走、500ms 走）。
        let mut p = PrecheckPanel::new();
        let mut probe = ScriptProbe { script: [CheckState::Green; 3], cost_ms: 10 };
        p.begin(0, &mut probe);
        p.tick(0, 1);
        for ms in [100, 300, 499] {
            p.tick(ms, 1);
            assert_eq!(p.flow, Flow::AllGreenHold, "{}ms 不应放行", ms);
        }
        p.tick(500, 1);
        assert_eq!(p.flow, Flow::Proceed);
    }
}
