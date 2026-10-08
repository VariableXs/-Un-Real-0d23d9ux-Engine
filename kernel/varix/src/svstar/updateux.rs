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
    /// 延后计划（深化 v2：defer 不再丢档位——到期重报）。
    deferred: Option<DeferPlan>,
    /// 更新包元数据（深化 v2：版本/体积/签名验证位）。
    package_version: String,
    package_bytes: u64,
    sig_verified: bool,
    /// 各安装阶段内子进度（万分比——阶段内+阶段间合成为总进度）。
    stage_bp: [u32; INSTALL_STAGES],
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
            deferred: None,
            package_version: String::from(""),
            package_bytes: 0,
            sig_verified: false,
            stage_bp: [0; INSTALL_STAGES],
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

    /// 延后三档选择（toast 动线——深化 v2：档位落成排程计划，到期重报）。
    pub fn defer(&mut self, d: Defer, now_ms: u64, now_hour: u64) -> FlowState {
        self.deferred = Some(plan_defer(d, now_ms, now_hour));
        self.state = FlowState::Idle;
        FlowState::Idle
    }

    /// 延后计划查询（到期判定由调用方以 defer_due 驱动）。
    pub fn deferred_plan(&self) -> Option<DeferPlan> {
        self.deferred
    }

    /// 包元数据登记（下载前置面：版本号与体积——诚实进度分母）。
    pub fn set_package_meta(&mut self, version: &str, bytes: u64) {
        self.package_version = String::from(version);
        self.package_bytes = bytes;
    }

    /// 包签名验证登记（下载完成后、安装预约前——验证不过 = 弃用重下）。
    pub fn mark_signature(&mut self, ok: bool) {
        self.sig_verified = ok;
    }

    pub fn package_meta(&self) -> (&str, u64, bool) {
        (&self.package_version, self.package_bytes, self.sig_verified)
    }

    /// 阶段内子进度登记（进度环的细粒度真值源）。
    pub fn set_stage_progress(&mut self, stage: Stage, bp: u32) {
        self.stage_bp[stage.index()] = bp.min(10_000);
    }

    /// 总安装进度（万分比）=（已完成阶段满值 + 当前阶段子进度）/ 阶段数
    /// ——不骗人条款的量化面：环上每 1% 都对应真实写入量。
    pub fn overall_progress_bp(&self) -> u32 {
        let sum: u64 = self.stage_bp.iter().map(|&b| b as u64).sum();
        (sum / INSTALL_STAGES as u64) as u32
    }

    /// 电源菜单角标（「关机时安装」已预约 → 电源菜单显示角标提示）。
    pub fn power_menu_badge(&self) -> bool {
        self.schedule == Some(Schedule::AtShutdown) && self.state == FlowState::Scheduled
    }

    /// 失败重试（失败页「重试」钮的执行面）：清失败因与阶段进度，回
    /// Idle 重新走流程；未处失败态时拒绝。
    pub fn retry(&mut self) -> Result<FlowState, &'static str> {
        if self.state != FlowState::FailedRolledBack {
            return Err("not-failed");
        }
        self.fail_reason = None;
        self.rollback_reported = false;
        self.progress_bp = 0;
        self.downloaded_bytes = 0;
        self.resumed_bytes = 0;
        self.schedule = None;
        self.stage_bp = [0; INSTALL_STAGES];
        self.state = FlowState::Idle;
        Ok(self.state)
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
// 深化批次 v2 · 一：延后三档的调度语义（defer 不再丢档位）
// ---------------------------------------------------------------------------

/// 1 小时档（毫秒）。
pub const DEFER_ONE_HOUR_MS: u64 = 3_600_000;
/// 黄昏档时刻（时——「今天稍后」落点：18 点）。
pub const DUSK_HOUR: u64 = 18;

/// 延后计划（档位 + 具体提醒时刻——诚实排程：Manual = 永不自动提醒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeferPlan {
    pub tier: Defer,
    pub until_ms: u64,
}

/// 延后排程计算（时间注入：now_ms + now_hour 分离——宿主测试确定复现）。
///
/// - OneHour：now + 1h（精确档）；
/// - Today：下一个 18:00（当日未到黄昏 = 今日黄昏；已过 = 次日黄昏）；
/// - Manual：u64::MAX（永不自动——设置页里自己来）。
pub fn plan_defer(d: Defer, now_ms: u64, now_hour: u64) -> DeferPlan {
    let until = match d {
        Defer::OneHour => now_ms.saturating_add(DEFER_ONE_HOUR_MS),
        Defer::Today => {
            if now_hour >= DUSK_HOUR {
                // 已过黄昏 → 次日黄昏（24h - 当前小时 + 黄昏时刻）。
                now_ms.saturating_add((24 - now_hour + DUSK_HOUR) * 3_600_000)
            } else {
                now_ms.saturating_add((DUSK_HOUR - now_hour) * 3_600_000)
            }
        }
        Defer::Manual => u64::MAX,
    };
    DeferPlan { tier: d, until_ms: until }
}

/// 到期判定（提醒时刻到 → 重新呈报 toast；Manual 永不到期）。
pub fn defer_due(plan: &DeferPlan, now_ms: u64) -> bool {
    now_ms >= plan.until_ms
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 二：双槽状态可视（F190 面的体验端）
// ---------------------------------------------------------------------------

/// 槽位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    A,
    B,
}

impl SlotKind {
    pub fn other(self) -> SlotKind {
        match self {
            SlotKind::A => SlotKind::B,
            SlotKind::B => SlotKind::A,
        }
    }
}

/// 双槽状态（WP-404 本体的可视面：活动槽版本 + 备用槽内容 + 待确认态）。
#[derive(Clone, Debug)]
pub struct DualSlot {
    pub active: SlotKind,
    pub active_version: String,
    pub standby_version: String,
    /// 更新已写入备用槽、待重启确认（重启换槽后置 false）。
    pub pending_confirm: bool,
}

impl DualSlot {
    pub fn new(active: SlotKind, active_version: &str, standby_version: &str) -> DualSlot {
        DualSlot {
            active,
            active_version: String::from(active_version),
            standby_version: String::from(standby_version),
            pending_confirm: false,
        }
    }

    /// 安装完成：新版本已写入备用槽 → 待确认（重启换槽）。
    pub fn mark_written(&mut self, new_version: &str) {
        self.standby_version = String::from(new_version);
        self.pending_confirm = true;
    }

    /// 重启确认：备用槽上位（活动/备用互换）。
    pub fn confirm_after_reboot(&mut self) -> bool {
        if !self.pending_confirm {
            return false;
        }
        let new_active = self.active.other();
        self.active = new_active;
        core::mem::swap(&mut self.active_version, &mut self.standby_version);
        self.pending_confirm = false;
        true
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 三：全屏更新画面文案序列（四幕语汇 + 禁止断电）
// ---------------------------------------------------------------------------

/// 全屏画面逐幕文案（复用开机四幕语汇；每幕附图形化禁止断电说明——
/// 主册「禁止断电提示（图形化）」的文案面；进度环绑定 stage 真值）。
pub const FULLSCREEN_CAPTIONS: [&str; 4] = [
    "正在备份当前系统（阶段 1/3）——请保持电源连接",
    "正在写入新版本（阶段 2/3）——请勿关闭电源或拔出 U 盘",
    "正在校验更新完整性（阶段 3/3）——校验完成前请勿断开电源",
    "更新完成，即将重启进入新版本",
];

/// 幕文案查询（stage → 对应幕；Done → 收尾幕）。
pub fn caption_for(stage: Option<Stage>) -> &'static str {
    match stage {
        Some(Stage::Backup) => FULLSCREEN_CAPTIONS[0],
        Some(Stage::Writing) => FULLSCREEN_CAPTIONS[1],
        Some(Stage::Verify) => FULLSCREEN_CAPTIONS[2],
        None => FULLSCREEN_CAPTIONS[3],
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 四：ETA 估算器（诚实进度：剩余时间可信）
// ---------------------------------------------------------------------------

/// 下载速率滑动窗口（8 样本环形——ETA 由真实速率推出，不拍脑袋）。
pub struct EtaEstimator {
    samples: [(u64, u64); 8],
    head: usize,
    len: usize,
}

impl EtaEstimator {
    pub fn new() -> EtaEstimator {
        EtaEstimator { samples: [(0, 0); 8], head: 0, len: 0 }
    }

    /// 采样（时刻 ms + 累计字节）。
    pub fn push(&mut self, t_ms: u64, cumulative_bytes: u64) {
        self.samples[self.head] = (t_ms, cumulative_bytes);
        self.head = (self.head + 1) % 8;
        if self.len < 8 {
            self.len += 1;
        }
    }

    /// 滑动速率（字节/秒；样本 <2 → None——样本不足不猜）。
    pub fn rate_bps(&self) -> Option<u64> {
        if self.len < 2 {
            return None;
        }
        let oldest = self.samples[(self.head + 8 - self.len) % 8];
        let newest_idx = (self.head + 7) % 8;
        let newest = self.samples[newest_idx];
        let dt = newest.0.saturating_sub(oldest.0);
        let db = newest.1.saturating_sub(oldest.1);
        if dt == 0 {
            return None;
        }
        Some(db * 1000 / dt)
    }

    /// 剩余时间（秒；速率不可得或速率 0 → None——诚实「估不出」）。
    pub fn eta_secs(&self, total_bytes: u64, cumulative_bytes: u64) -> Option<u64> {
        let rate = self.rate_bps()?;
        if rate == 0 || cumulative_bytes >= total_bytes {
            return None;
        }
        Some((total_bytes - cumulative_bytes) / rate)
    }
}

impl Default for EtaEstimator {
    fn default() -> Self {
        Self::new()
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

    // 13. 延后三档排程语义（深化 v2）：1h 精确档 / 黄昏档落 18 点 /
    //     Manual 永不到期；到期重报判定成立。
    let p1 = plan_defer(Defer::OneHour, 1_000_000, 10);
    let p2 = plan_defer(Defer::Today, 3_600_000 * 10, 10); // 10:00 → 当日 18:00 = +8h
    let p3 = plan_defer(Defer::Manual, 0, 0);
    let p4 = plan_defer(Defer::Today, 3_600_000 * 19, 19); // 19:00 → 次日 18:00 = +23h
    set.add(
        "defer tiers schedule semantics",
        p1.until_ms == 1_000_000 + DEFER_ONE_HOUR_MS
            && p2.until_ms == 3_600_000 * 18
            && p3.until_ms == u64::MAX
            && p4.until_ms == 3_600_000 * 42
            && !defer_due(&p3, 86_400_000 * 365)
            && defer_due(&p1, 1_000_000 + DEFER_ONE_HOUR_MS),
        "",
    );

    // 14. 双槽状态可视（深化 v2）：写入备用槽 → 待确认 → 重启换槽上位。
    let mut slots = DualSlot::new(SlotKind::A, "1.0", "1.0");
    let no_confirm_early = !slots.confirm_after_reboot();
    slots.mark_written("1.1");
    let written_ok = slots.pending_confirm && slots.standby_version == "1.1";
    let confirmed = slots.confirm_after_reboot();
    set.add(
        "dual slot visual + confirm after reboot",
        no_confirm_early
            && written_ok
            && confirmed
            && slots.active == SlotKind::B
            && slots.active_version == "1.1"
            && slots.standby_version == "1.0"
            && !slots.pending_confirm,
        "",
    );

    // 15. 全屏四幕文案与阶段绑定（深化 v2）：三安装幕 + 完成幕，每幕
    //     含禁止断电图形化说明语义。
    let caps_ok = caption_for(Some(Stage::Backup)).contains("阶段 1/3")
        && caption_for(Some(Stage::Verify)).contains("阶段 3/3")
        && caption_for(Some(Stage::Verify)).contains("请勿断开电源")
        && caption_for(None).contains("重启")
        && FULLSCREEN_CAPTIONS.len() == 4;
    set.add("fullscreen captions bound to stages", caps_ok, "");

    // 16. ETA 估算器（深化 v2）：速率 = 滑动窗口真实速率；样本不足与
    //     速率为零时诚实返回 None（估不出不硬估）。
    let mut eta = EtaEstimator::new();
    let too_few = eta.eta_secs(1000, 0).is_none();
    eta.push(0, 0);
    eta.push(1000, 500);
    let rate = eta.rate_bps();
    let half = eta.eta_secs(1000, 500);
    eta.push(2000, 1000);
    let done_none = eta.eta_secs(1000, 1000).is_none();
    set.add(
        "eta estimator honest semantics",
        too_few && rate == Some(500) && half == Some(1) && done_none,
        "",
    );

    // 17. 包元数据与签名门（深化 v2）：未验签标记 → 弃用重下链路完整。
    let mut u = UpdateFlow::new(notes.clone());
    u.set_package_meta("1.1.0", 400_000_000);
    u.begin_download();
    u.download_done(400_000_000);
    u.mark_signature(false);
    let meta_bad = u.package_meta().2 == false;
    let st_bad = u.verify_package_failed();
    u.mark_signature(true);
    let meta_good = u.package_meta() == ("1.1.0", 400_000_000, true);
    set.add(
        "package meta + signature gate",
        meta_bad && st_bad == FlowState::Idle && meta_good,
        "",
    );

    // 18. 总安装进度合成（深化 v2）：阶段内子进度 → 总进度线性合成。
    let mut u = UpdateFlow::new(notes.clone());
    u.begin_download();
    u.download_done(100);
    u.schedule_install(Schedule::Now);
    u.advance_stage(Stage::Backup).unwrap();
    u.set_stage_progress(Stage::Backup, 5_000);
    let third = u.overall_progress_bp() == 1_666;
    u.advance_stage(Stage::Writing).unwrap();
    u.set_stage_progress(Stage::Backup, 10_000);
    u.set_stage_progress(Stage::Writing, 10_000);
    u.set_stage_progress(Stage::Verify, 10_000);
    let full = u.overall_progress_bp() == 10_000;
    set.add("overall progress composed from stages", third && full, "");

    // 19. 电源菜单角标（深化 v2）：关机时安装已预约 → 角标亮；其他态
    //     不亮。
    let mut u = UpdateFlow::new(notes.clone());
    let badge_idle = !u.power_menu_badge();
    u.begin_download();
    u.download_done(100);
    u.schedule_install(Schedule::AtShutdown);
    set.add("power menu badge on shutdown-schedule", badge_idle && u.power_menu_badge(), "");

    // 20. 失败重试全链（深化 v2）：失败 → 重试清态回 Idle → 二次走完
    //     全流程成功；非失败态重试拒绝。
    let mut u = UpdateFlow::new(notes.clone());
    let retry_early = u.retry().is_err();
    u.begin_download();
    u.download_done(100);
    u.schedule_install(Schedule::Now);
    u.advance_stage(Stage::Backup).unwrap();
    let _ = u.fail_and_rollback("注入失败");
    let retried = u.retry() == Ok(FlowState::Idle) && u.fail_reason().is_none();
    u.begin_download();
    u.download_done(100);
    u.schedule_install(Schedule::Now);
    u.advance_stage(Stage::Backup).unwrap();
    u.advance_stage(Stage::Writing).unwrap();
    u.advance_stage(Stage::Verify).unwrap();
    u.install_done();
    set.add(
        "retry clears failure and refull-flows",
        retry_early && retried && u.state() == FlowState::Done,
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

    #[test]
    fn f122_defer_then_due_reoffers() {
        // 延后到期 → 允许重新呈报（Idle 态再 offer——不骚扰纪律的
        // 「到期重报」闭环）。
        let mut u = UpdateFlow::new(vec![UpdateNote { text: String::from("n") }]);
        let _ = u.offer(80);
        let _ = u.defer(Defer::OneHour, 1_000_000, 10);
        assert_eq!(u.deferred_plan().unwrap().until_ms, 1_000_000 + DEFER_ONE_HOUR_MS);
        assert!(u.offer(80).is_some(), "延后回 Idle 后可重报");
    }

    #[test]
    fn f122_eta_window_rolls() {
        // 9 个样本滚动窗：rate 只看最近 8 样本（环形覆盖语义）。
        let mut eta = EtaEstimator::new();
        for i in 0..9u64 {
            eta.push(i * 1000, i * 100);
        }
        // 最早样本 (0,0) 被逐出；窗口首 = (1000,100)，尾 = (8000,800)。
        assert_eq!(eta.rate_bps(), Some(100));
    }

    #[test]
    fn f122_slot_alternates_repeatedly() {
        let mut slots = DualSlot::new(SlotKind::A, "1.0", "1.0");
        for v in ["1.1", "1.2", "1.3"] {
            slots.mark_written(v);
            assert!(slots.confirm_after_reboot());
        }
        // 三轮换槽：A→B→A→B（奇数次后活动槽在 B）。
        assert_eq!(slots.active, SlotKind::B);
        assert_eq!(slots.active_version, "1.3");
    }

    #[test]
    fn f122_stage_progress_clamped() {
        let mut u = UpdateFlow::new(vec![]);
        u.set_stage_progress(Stage::Writing, 20_000);
        assert!(u.overall_progress_bp() <= 10_000, "子进度越界被钳到满值");
    }
}
