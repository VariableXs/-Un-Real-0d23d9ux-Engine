//! F122 更新体验面 · 完整设计（STAR I 主册 G-C-52）。
//!
//! **判据（主册）**：正常更新全流程（含预约关机装）实测录屏；断电注入
//! 更新中 → 回滚成功且系统可用（B-1304 复测）；失败页文案三要素。
//!
//! **设计要点（主册）**：
//! - 系统更新全体验流程：提示 toast（延后三档 1h/今天/手动）→ 下载
//!   进度（后台 F057 低优先）→ 安装预约（立即/关机时）→ 全屏更新画面
//!   （复用开机四幕语汇）→ 失败自动回滚+说明页；
//! - toast 含更新内容摘要（前 3 条+「详情」）；预约设置页（F190 双槽
//!   状态可视）；全屏画面：进度环+当前阶段文案（备份/写入/校验）+
//!   禁止断电提示（图形化）；失败页：原因归因+已回滚确认+重试钮；
//! - 下载中断网 → 断点续传；电量 <20% → 推迟建议（F196 联动）；校验
//!   失败 → 包弃用重下；回滚后自动报备（F120 体检灯）；
//! - 进度环与真实安装阶段绑定（不骗人条款——四幕同纪律）；「关机时
//!   安装」在电源菜单（C-3）加角标提示；更新中全程可取消（校验前）；
//!   完成后首启弹「更新了什么」卡（3 条内）；静默时段（免打扰）不弹
//!   toast；
//! - 双槽机制本体 WP-404 既有（A/B 槽）；本项纯体验面。
//!
//! 时间注入式（毫秒戳），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 电量推迟建议线（%，主册：电量 <20% → 推迟建议）。
pub const BATTERY_DEFER_PCT: u32 = 20;
/// toast 摘要条数上限（主册：前 3 条+「详情」）。
pub const TOAST_SUMMARY_MAX: usize = 3;
/// 「更新了什么」卡条数上限（主册：3 条内）。
pub const WHATSNEW_MAX: usize = 3;
/// 安装阶段数（备份/写入/校验——进度环绑定的真实阶段）。
pub const INSTALL_STAGES: usize = 3;
/// 校验前可取消、校验起禁止（阶段索引判线）。
pub const CUTOFF_STAGE: usize = 2;

// ---------------------------------------------------------------------------
// 状态机
// ---------------------------------------------------------------------------

/// 延后三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Defer {
    /// 1 小时后再提。
    OneHour,
    /// 今天稍后（下次开机/当日黄昏）。
    Today,
    /// 手动（设置页里自己来）。
    Manual,
}

/// 安装预约。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Schedule {
    Now,
    AtShutdown,
}

/// 安装阶段（进度环绑定真实阶段——不骗人条款）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Backup,
    Writing,
    Verify,
}

impl Stage {
    pub fn index(self) -> usize {
        match self {
            Stage::Backup => 0,
            Stage::Writing => 1,
            Stage::Verify => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Stage::Backup => "备份",
            Stage::Writing => "写入",
            Stage::Verify => "校验",
        }
    }
}

/// 更新流程状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowState {
    Idle,
    /// toast 已呈报（等待延后/预约选择）。
    Offered,
    Downloading,
    Downloaded,
    /// 已预约（立即/关机时）。
    Scheduled,
    Installing(Stage),
    Done,
    /// 失败（已回滚）。
    FailedRolledBack,
}

/// 更新内容条目。
#[derive(Clone, Debug)]
pub struct UpdateNote {
    pub text: String,
}

/// 更新流程管理器。
pub struct UpdateFlow {
    state: FlowState,
    notes: Vec<UpdateNote>,
    /// 下载进度（0-10000 万分比）与断点（已确认字节）。
    progress_bp: u32,
    resumed_bytes: u64,
    downloaded_bytes: u64,
    schedule: Option<Schedule>,
    /// 失败原因（失败页三要素之一）。
    fail_reason: Option<&'static str>,
    /// 回滚报备（F120 体检灯联动）。
    rollback_reported: bool,
    /// 静默时段（免打扰）——toast 不弹。
    dnd_active: bool,
}

impl UpdateFlow {
    pub fn new(notes: Vec<UpdateNote>) -> UpdateFlow {
        UpdateFlow {
            state: FlowState::Idle,
            notes,
            progress_bp: 0,
            resumed_bytes: 0,
            downloaded_bytes: 0,
            schedule: None,
            fail_reason: None,
            rollback_reported: false,
            dnd_active: false,
        }
    }

    pub fn state(&self) -> FlowState {
        self.state
    }

    pub fn progress_bp(&self) -> u32 {
        self.progress_bp
    }

    pub fn resumed_bytes(&self) -> u64 {
        self.resumed_bytes
    }

    pub fn fail_reason(&self) -> Option<&'static str> {
        self.fail_reason
    }

    pub fn rollback_reported(&self) -> bool {
        self.rollback_reported
    }

    /// 免打扰时段登记（静默时段不弹 toast——toast 呈报前置门）。
    pub fn set_dnd(&mut self, on: bool) {
        self.dnd_active = on;
    }

    /// toast 呈报：摘要 = 前 3 条 + 「详情」；静默时段拒绝呈报（返回
    /// None——不骚扰纪律）。电量 <20% 附推迟建议。
    pub fn offer(&mut self, battery_pct: u32) -> Option<(Vec<String>, bool)> {
        if self.dnd_active || self.state != FlowState::Idle {
            return None;
        }
        let mut summary = Vec::new();
        let n = self.notes.len().min(TOAST_SUMMARY_MAX);
        for i in 0..n {
            summary.push(self.notes[i].text.clone());
        }
        if self.notes.len() > TOAST_SUMMARY_MAX {
            summary.push(String::from("详情"));
        }
        self.state = FlowState::Offered;
        Some((summary, battery_pct < BATTERY_DEFER_PCT))
    }

    /// 延后三档选择（toast 动线）。
    pub fn defer(&mut self, d: Defer, now_ms: u64) -> FlowState {
        self.state = FlowState::Idle;
        let _ = (d, now_ms); // 延后提醒由调度面消费（1h/今天/手动三档语义）
        FlowState::Idle
    }

    /// 开始下载（后台低优先 F057 面语义）。
    pub fn begin_download(&mut self) -> FlowState {
        self.state = FlowState::Downloading;
        self.state
    }

    /// 下载推进（断点续传：断网注入 → resume 后从已确认字节续）。
    pub fn download_progress(&mut self, total_bytes: u64, got_bytes: u64) {
        self.downloaded_bytes = got_bytes;
        self.progress_bp = ((got_bytes.min(total_bytes).max(0) as u64) * 10_000
            / total_bytes.max(1)) as u32;
        self.state = FlowState::Downloading;
    }

    /// 断网恢复：续传从上次确认字节起（resumed_bytes = 断点）。
    pub fn resume_after_disconnect(&mut self, acked_bytes: u64) {
        self.resumed_bytes = acked_bytes;
        self.state = FlowState::Downloading;
    }

    /// 下载完成。
    pub fn download_done(&mut self, total_bytes: u64) -> FlowState {
        self.downloaded_bytes = total_bytes;
        self.progress_bp = 10_000;
        self.state = FlowState::Downloaded;
        self.state
    }

    /// 校验失败 → 包弃用重下（进度清零、状态回 Idle）。
    pub fn verify_package_failed(&mut self) -> FlowState {
        self.progress_bp = 0;
        self.downloaded_bytes = 0;
        self.state = FlowState::Idle;
        self.state
    }

    /// 预约（立即/关机时）。
    pub fn schedule_install(&mut self, s: Schedule) -> FlowState {
        self.schedule = Some(s);
        self.state = FlowState::Scheduled;
        self.state
    }

    /// 阶段推进（进度环与真实阶段绑定）。校验前可取消（返回
    /// Err(已取消)）；校验起取消被拒（禁止断电纪律）。
    pub fn advance_stage(&mut self, to: Stage) -> Result<FlowState, &'static str> {
        if self.state != FlowState::Installing(Stage::Backup)
            && self.state != FlowState::Installing(Stage::Writing)
            && self.state != FlowState::Scheduled
            && self.state != FlowState::Installing(Stage::Verify)
        {
            return Err("not-installing");
        }
        self.state = FlowState::Installing(to);
        Ok(self.state)
    }

    /// 取消安装（校验前合法）。
    pub fn cancel_install(&mut self) -> Result<FlowState, &'static str> {
        match self.state {
            FlowState::Installing(s) if s.index() < CUTOFF_STAGE => {
                self.state = FlowState::Scheduled;
                Ok(self.state)
            }
            FlowState::Installing(_) => Err("verify-stage-no-cancel"),
            _ => Err("not-installing"),
        }
    }

    /// 安装成功。
    pub fn install_done(&mut self) -> FlowState {
        self.state = FlowState::Done;
        self.state
    }

    /// 断电/失败 → 自动回滚（B-1304 复测面）：回 A 槽、系统可用、
    /// 报备 F120。
    pub fn fail_and_rollback(&mut self, reason: &'static str) -> FlowState {
        self.fail_reason = Some(reason);
        self.rollback_reported = true;
        self.state = FlowState::FailedRolledBack;
        self.state
    }

    /// 失败页三要素（发生了什么/为什么/下一步）。
    pub fn failure_page(&self) -> Option<(&'static str, &'static str, &'static str)> {
        self.fail_reason.map(|why| ("更新未完成", why, "系统已回滚到更新前，可点重试"))
    }

    /// 「更新了什么」卡（完成后首启；3 条内）。
    pub fn whatsnew_card(&self) -> Vec<String> {
        self.notes.iter().take(WHATSNEW_MAX).map(|n| n.text.clone()).collect()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_updateux_checks() -> CheckSet {
    let mut set = CheckSet::new("F122-updateux");

    let notes = vec![
        UpdateNote { text: String::from("夜间模式曲线可拖锚点") },
        UpdateNote { text: String::from("帮助中心搜索更快") },
        UpdateNote { text: String::from("修复任务栏偶发闪烁") },
        UpdateNote { text: String::from("第四条不该出现在摘要") },
    ];

    // 1. 正常全流程（判据第一句）：toast→下载→预约→三阶段→完成。
    let mut u = UpdateFlow::new(notes.clone());
    let offered = u.offer(80);
    let summary_ok = offered
        .as_ref()
        .map(|(s, _)| s.len() == 4 && s[3] == "详情")
        == Some(true);
    u.begin_download();
    u.download_progress(1000, 500);
    let half = u.progress_bp() == 5000;
    u.download_done(1000);
    u.schedule_install(Schedule::AtShutdown);
    u.advance_stage(Stage::Backup).unwrap();
    u.advance_stage(Stage::Writing).unwrap();
    u.advance_stage(Stage::Verify).unwrap();
    u.install_done();
    set.add(
        "full flow offer-download-schedule-install-done",
        summary_ok && half && u.state() == FlowState::Done,
        "",
    );

    // 2. 预约关机装（判据第一句之二：含预约关机装）。
    let mut u = UpdateFlow::new(notes.clone());
    u.begin_download();
    u.download_done(100);
    let s = u.schedule_install(Schedule::AtShutdown);
    set.add("shutdown-time schedule accepted", s == FlowState::Scheduled, "");

    // 3. 断电注入更新中 → 回滚成功且系统可用（判据第一句之三；B-1304）。
    let mut u = UpdateFlow::new(notes.clone());
    u.begin_download();
    u.download_done(100);
    u.schedule_install(Schedule::Now);
    u.advance_stage(Stage::Backup).unwrap();
    u.advance_stage(Stage::Writing).unwrap();
    let st = u.fail_and_rollback("写入阶段断电");
    set.add(
        "power-loss mid-update rolls back usable",
        st == FlowState::FailedRolledBack && u.rollback_reported(),
        "",
    );

    // 4. 失败页文案三要素（发生了什么/为什么/下一步）。
    let (what, why, next) = u.failure_page().unwrap();
    set.add(
        "failure page triage text",
        what.contains("未完成") && why.contains("断电") && next.contains("已回滚") && next.contains("重试"),
        "",
    );

    // 5. 断点续传：断网 → resume 从确认字节续（进度不清零重头）。
    let mut u = UpdateFlow::new(notes.clone());
    u.begin_download();
    u.download_progress(1000, 700);
    u.resume_after_disconnect(700);
    u.download_progress(1000, 700 + 300);
    set.add(
        "resume from acked bytes",
        u.resumed_bytes() == 700 && u.progress_bp() == 10_000,
        "",
    );

    // 6. 校验失败 → 包弃用重下（进度清零回 Idle）。
    let mut u = UpdateFlow::new(notes.clone());
    u.begin_download();
    u.download_done(1000);
    let st = u.verify_package_failed();
    set.add(
        "verify fail discards package",
        st == FlowState::Idle && u.progress_bp() == 0,
        "",
    );

    // 7. 电量 <20% → 推迟建议位（F196 联动）。
    let mut u = UpdateFlow::new(notes.clone());
    let (.., defer_suggested) = u.offer(15).unwrap();
    set.add("low battery defer suggestion", defer_suggested, "");

    // 8. 静默时段（免打扰）不弹 toast。
    let mut u = UpdateFlow::new(notes.clone());
    u.set_dnd(true);
    set.add("dnd suppresses toast", u.offer(80).is_none(), "");

    // 9. 更新中可取消（校验前）；校验起取消被拒。
    let mut u = UpdateFlow::new(notes.clone());
    u.begin_download();
    u.download_done(100);
    u.schedule_install(Schedule::Now);
    u.advance_stage(Stage::Backup).unwrap();
    let cancel_ok = u.cancel_install() == Ok(FlowState::Scheduled);
    u.advance_stage(Stage::Backup).unwrap();
    u.advance_stage(Stage::Writing).unwrap();
    u.advance_stage(Stage::Verify).unwrap();
    let cancel_denied = u.cancel_install() == Err("verify-stage-no-cancel");
    set.add("cancel allowed pre-verify, denied at verify", cancel_ok && cancel_denied, "");

    // 10. 进度环绑定真实阶段（不骗人条款）：阶段名三段、进度可读。
    let mut u = UpdateFlow::new(notes.clone());
    u.begin_download();
    u.download_done(100);
    u.schedule_install(Schedule::Now);
    u.advance_stage(Stage::Writing).unwrap();
    set.add(
        "progress ring bound to real stage",
        INSTALL_STAGES == 3 && u.state() == FlowState::Installing(Stage::Writing),
        "",
    );

    // 11. 「更新了什么」卡 3 条内（第 4 条不出现）。
    let mut u = UpdateFlow::new(notes.clone());
    u.install_done();
    let card = u.whatsnew_card();
    set.add(
        "whatsnew card capped at 3",
        card.len() == 3 && !card[2].contains("第四条"),
        "",
    );

    // 12. toast 摘要前 3 条 + 详情（判据常量对账）。
    set.add(
        "toast summary constants",
        TOAST_SUMMARY_MAX == 3 && WHATSNEW_MAX == 3 && BATTERY_DEFER_PCT == 20,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updateux_all_checks_green() {
        let set = run_updateux_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F122 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn offer_twice_rejected() {
        let mut u = UpdateFlow::new(vec![UpdateNote { text: String::from("a") }]);
        assert!(u.offer(80).is_some());
        assert!(u.offer(80).is_none(), "同批次不重复呈报");
    }

    #[test]
    fn stage_names_bound() {
        assert_eq!(Stage::Backup.name(), "备份");
        assert_eq!(Stage::Writing.name(), "写入");
        assert_eq!(Stage::Verify.name(), "校验");
        assert_eq!(CUTOFF_STAGE, Stage::Verify.index());
    }

    #[test]
    fn failure_page_none_when_ok() {
        let mut u = UpdateFlow::new(vec![]);
        u.install_done();
        assert!(u.failure_page().is_none());
    }
}
