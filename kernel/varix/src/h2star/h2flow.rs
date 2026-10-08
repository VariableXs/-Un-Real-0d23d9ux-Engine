//! H2 域集成演练 · 深化批次三收束件（跨引擎场景编排——五十项不是
//! 五十座孤岛：装配→设置→拖放→文件操作→生命周期→拆除全链走通）。
//!
//! **承接判据**（主册 H 域正文 + 十四章状态机公理）：
//! - 会话从装配到拆除的**完整生命周期**必须可演练（F273/F253/F266/
//!   F286 恢复族 + F281 浮层出路拆除——零「开得来关不掉」路径）；
//! - **场景可回放**：同脚本同结果（演练步骤确定性——随机数进不来
//!   本层，时间由调用方注入）；
//! - 演练账：每步产出 (步骤, 结论, 耗时)——十三章「体验日志」的
//!   演练侧落点，失败步不中断后续步（一次演练出全量红项清单）。

use crate::checks::CheckSet;

use alloc::vec;
use alloc::vec::Vec;

use crate::h2star::h2appctl::{Instance, Lifecycle, Entry};
use crate::h2star::h2dnd::{DragSession, DragState, DropKind, DropZone, hit_test, drop_action, DropAction};
use crate::h2star::h2fsops::{plan_item, OpKind, Collision, Action};
use crate::h2star::h2host::{SessionAssembler, teardown_plan, TearLayer};
use crate::h2star::h2settings::{SettingsStore, export_pack, import_pack, SCHEMA_VERSION};

// ---------------------------------------------------------------------------
// 演练账
// ---------------------------------------------------------------------------

/// 一条演练步骤账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrillStep {
    pub name: &'static str,
    pub passed: bool,
    pub detail: &'static str,
}

/// 演练记录器：失败不中断（一次跑出全量结论——与 CheckSet 纪律同源）。
pub struct Drill {
    pub steps: Vec<DrillStep>,
}

impl Drill {
    pub fn new() -> Drill {
        Drill { steps: Vec::new() }
    }

    pub fn step(&mut self, name: &'static str, passed: bool, detail: &'static str) {
        self.steps.push(DrillStep { name, passed, detail });
    }

    pub fn all_green(&self) -> bool {
        self.steps.iter().all(|s| s.passed)
    }

    pub fn red_count(&self) -> usize {
        self.steps.iter().filter(|s| !s.passed).count()
    }
}

// ---------------------------------------------------------------------------
// 场景一：会话全生命周期（装配 → 使用 → 拆除）
// ---------------------------------------------------------------------------

/// 会话演练：装配四步（含一个损坏快照降级）→ 使用中写入设置 →
/// 拆除计划后开先关 → 导出封包可回读。
pub fn drill_session_lifecycle() -> Drill {
    let mut d = Drill::new();
    // 1. 装配：第三步解析出损坏项——降级自首但链不断。
    let mut asm = SessionAssembler::new();
    asm.advance(&[]);
    asm.advance(&[]);
    asm.advance(&["F298.tiles"]);
    asm.advance(&[]);
    d.step("assemble-ready-with-degrade", asm.is_ready() && asm.degraded().len() == 1, "1 degraded");
    // 2. 使用：设置写入 + 校验和账可导出。
    let mut st = SettingsStore::new();
    let ok_set = st.set("f286.mode", vec![2, 0, 0, 0]) && st.set("f271.tab_count", vec![3, 0, 0, 0]);
    let pack = export_pack(&st, SCHEMA_VERSION);
    d.step("settings-exportable", ok_set && pack.len() > 4, "2 keys packed");
    // 3. 拆除：五层齐发 → 浮层先关、遥测最后（后开先关机判）。
    let plan = teardown_plan(&[
        (TearLayer::Window, 1),
        (TearLayer::Floating, 3),
        (TearLayer::Telemetry, 1),
        (TearLayer::Tray, 2),
        (TearLayer::Panel, 1),
    ]);
    d.step(
        "teardown-lifo",
        plan[0].0 == TearLayer::Floating && plan[4].0 == TearLayer::Telemetry,
        "floating first",
    );
    // 4. 重启回读：封包 round-trip（装配读回同值——升级不破坏旧数据）。
    let mut corr = Vec::new();
    let back = import_pack(&pack, &mut corr);
    d.step(
        "restart-roundtrip",
        back.map(|items| {
            items.iter().find(|(k, _)| *k == "f271.tab_count").map(|(_, v)| v.as_slice()) == Some(&[3, 0, 0, 0][..])
        }) == Some(true),
        "value survives reboot",
    );
    d
}

// ---------------------------------------------------------------------------
// 场景二：拖放 → 文件操作 → 生命周期（一次完整用户旅程）
// ---------------------------------------------------------------------------

/// 用户旅程演练：桌面拖文本到终端（安全确认路径）→ 拖文件进目录
/// 碰撞改名 → 应用无响应 → 恢复 → 正常结束清算。
pub fn drill_user_journey() -> Drill {
    let mut d = Drill::new();
    // 1. 触屏长按拖起文本；落点命中桌面（嵌套在桌面里的终端区取内层）。
    let mut drag = DragSession::new();
    drag.press(60, 60);
    drag.tick(500);
    drag.attach("一段要贴进终端的文本", false);
    let zones = [
        DropZone { kind: DropKind::Desktop, x: 0, y: 0, w: 800, h: 600 },
        DropZone { kind: DropKind::Terminal, x: 40, y: 40, w: 100, h: 80 },
    ];
    let kind = hit_test(&zones, 70, 70);
    let act = kind.map(|k| drop_action(k, false));
    d.step(
        "drag-to-terminal-confirms",
        drag.state == DragState::LongPress
            && kind == Some(DropKind::Terminal)
            && act == Some(DropAction::InsertWithConfirm),
        "terminal asks first",
    );
    // 2. 半路 Esc 放弃：拖影无残留（复原路径先于文件操作——十四章）。
    d.step("esc-cancels-clean", drag.cancel() && drag.payload.is_none(), "no ghost");
    // 3. 文件操作：跨盘移动三段（校验不过不删源）+ 碰撞改名。
    let plan = plan_item(
        OpKind::Move,
        "/c/报告.docx",
        "/d",
        &["/d/报告.docx".to_string()],
        Collision::AutoRename,
        false,
    );
    d.step(
        "move-plan-cross-volume",
        plan.final_name == "/d/报告 - 副本.docx"
            && plan.actions.len() == 3
            && matches!(plan.actions[2], Action::DeleteSrc { .. }),
        "verify before delete",
    );
    // 4. 应用生命周期：无响应 → 三连心跳恢复 → 正常结束清算。
    let mut app = Instance::new("记事本", 42);
    app.state = Lifecycle::Launching;
    app.beat_launch();
    app.beat_launch();
    app.beat_launch();
    let mut hung = false;
    for _ in 0..6 {
        hung |= app.tick(1000);
    }
    let recovered = {
        for _ in 0..3 {
            app.heartbeat();
        }
        app.state == Lifecycle::Running
    };
    let td = app.terminate(false);
    d.step(
        "app-full-lifecycle",
        hung && recovered && td.remove_taskbar_button && td.reclaim_resources,
        "hang→recover→clean end",
    );
    // 5. 生命周期账：两入口各记一笔（同账结构）。
    let mut ledger = crate::h2star::h2appctl::LifecycleLedger::new();
    ledger.record("记事本", Entry::Taskbar, false);
    ledger.record("记事本", Entry::StartMenu, false);
    d.step("activation-ledger", ledger.len() == 2 && ledger.consistent_for("记事本"), "one book");
    d
}

// ---------------------------------------------------------------------------
// 自检（演练确定性——同脚本同结果）
// ---------------------------------------------------------------------------

pub fn run_h2flow_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2flow");
    // 场景一：全绿 + 步数固定（确定性口径）。
    let s1 = drill_session_lifecycle();
    set.add(
        "h2flow session lifecycle",
        s1.all_green() && s1.steps.len() == 4,
        "assemble→use→tear→reboot",
    );
    // 场景二：全绿 + 步数固定。
    let s2 = drill_user_journey();
    set.add(
        "h2flow user journey",
        s2.all_green() && s2.steps.len() == 5,
        "drag→cancel→move→lifecycle",
    );
    // 可回放：同脚本两次跑逐条等值（确定性——无时钟无随机）。
    let s1b = drill_session_lifecycle();
    let s2b = drill_user_journey();
    set.add(
        "h2flow replayable",
        s1.steps == s1b.steps && s2.steps == s2b.steps,
        "same script same result",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2flow_all_green() {
        let set = run_h2flow_checks();
        assert!(set.all_passed(), "h2flow 自检有红项");
        assert!(!set.truncated(), "h2flow 自检溢出");
    }

    #[test]
    fn drills_report_human_summary() {
        // 演练结论可人话汇总（十三章：结论字段——不是一堆流水）。
        let d = drill_user_journey();
        let reds = d.red_count();
        assert!(reds == 0);
        assert_eq!(d.steps.len(), 5);
    }
}
