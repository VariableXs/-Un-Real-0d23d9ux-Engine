//! 阶段 6 · S3.2 引擎编排底座（AI-2 · Windows 部署与引擎线）。
//!
//! 职责（施工总案 6.1/6.2，三体分工图 S3.2）：
//!   差分盘挂载 → Hyper-V VM 创建/上电 → 就绪探针（47631 心跳）→ 保活看门狗
//!   → 休眠/恢复（内存快照写差分盘）→ 优雅关闭。五态状态机显式建模，拉起幂等。
//!
//! 架构：
//!   - 逻辑核心（EngineShared + 步进函数）与后端（`EngineBackend` trait）解耦：
//!     MockBackend 供单测/非 Windows 构建真实可跑；HyperVBackend 只在
//!     `#[cfg(windows)]` 下编入。开放性验收：VM 底座可替换，逻辑核心不动。
//!   - **步进式拉起**：`launch_step` 每调用推进一步（发阶段事件/挂盘/建机/上电/探针），
//!     每步独立短持锁——engine_status 在整个冷启动期间永远可响应，不会卡 150s。
//!   - 事件走 `engine://state`（前端 engineSessions.ts 已挂载消费）：
//!     `{ seq, kind, stage?, reason? }`，seq 单调递增 + 有界重放缓冲（对齐 boot.rs 模式）。
//!     stage 取值与前端 `ENGINE_BOOT_STAGES` 五键严格一致。
//!   - 心跳协议与 `vm_agent.rs` 同源：连接 127.0.0.1:47631 发 `PING` 期待 `READY`。
//!   - 拉起幂等：状态机即锁（Launching/Ready 重复 wake 直接吸收）+ 进程级 BUSY
//!     原子标志（跨线程不双开，拉起线程全程持有）。
//!   - 心跳超时重启策略：连续 MISSES 次失联 → 自动重启一次（closed→五阶段重放）；
//!     再失联 → Failed（crashed 事件带三要素原因）。
//!   - 拔盘联动（异常场景一/三）：REMOVED 原子标志 + 拉起步进内中止检查 →
//!     安静收束为 Closed + usb-removed 事件（只发一次），绝不把拔盘伪装成崩溃。
//!   - 零 unwrap：生产路径全部显式错误。
//!
//! 边界（诚实文化）：
//!   - 默认关：本模块只被前端 engine_wake 唤醒（settings.engineEnabled 门槛在前端）。
//!   - 真实 VM 链路验证依赖 S1.2（WIN_ENGINE 镜像部署）——本批交付=代码级+Mock 单测。
//!   - 差分链布局契约（与 portable/ 三体差分链脚本一致）：
//!     `<WIN_ENGINE 卷>\Engine\Base.vhdx → Apps.vhdx → User.vhdx`，VM 只挂 User 层。

use serde::Serialize;
use std::collections::VecDeque;
use std::net::TcpStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
#[cfg(windows)]
use tauri::Emitter;

use crate::error::{AppError, CmdResult};

pub const ENGINE_EVENT: &str = "engine://state";

/// 与 `vm_agent.rs` 的 HEARTBEAT_PORT 同源（该模块在 vm-agent feature 门控下，
/// 此处独立常量防 feature 牵连；下方有 feature 开启时的相等性测试看护）。
pub const HEARTBEAT_PORT: u16 = 47631;

/// 与前端 `ENGINE_BOOT_STAGES`（src/system/engine/engineModel.ts）逐键一致的五阶段。
pub const STAGE_VHDX_MOUNT: &str = "vhdx-mount";
pub const STAGE_VM_CREATE: &str = "vm-create";
pub const STAGE_VM_POWER: &str = "vm-power";
pub const STAGE_AGENT_HEARTBEAT: &str = "agent-heartbeat";
pub const STAGE_READY_HANDSHAKE: &str = "ready-handshake";

const BOOT_STAGE_KEYS: [&str; 5] = [
    STAGE_VHDX_MOUNT,
    STAGE_VM_CREATE,
    STAGE_VM_POWER,
    STAGE_AGENT_HEARTBEAT,
    STAGE_READY_HANDSHAKE,
];

/// 冷启动硬超时（总案口径 20-40s 如实公示；超时=如实报错不虚构进度）。
pub const BOOT_TIMEOUT_SECS: u64 = 150;
/// 心跳探针默认间隔（秒；Mock 后端覆写为零时长，单测瞬时跑完）。
pub const HEARTBEAT_POLL_SECS: u64 = 1;
/// 看门狗 tick 间隔（秒）与重启前连续失联次数（≈15s 失联即处置）。
pub const WATCHDOG_TICK_SECS: u64 = 5;
pub const MISSES_BEFORE_RESTART: u32 = 3;
/// 心跳失联自动重启预算：1 次；再失联转 Failed。
pub const MAX_HEARTBEAT_RESTARTS: u32 = 1;

// ---------------------------------------------------------------- 五态状态机

/// 引擎生命周期五态（PS 编排器同表 + 显式 Stop 出口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    Closed,
    Launching,
    Ready,
    Hibernating,
    Hibernated,
    Failed,
}

impl EngineState {
    pub fn as_str(&self) -> &'static str {
        match self {
            EngineState::Closed => "closed",
            EngineState::Launching => "starting",
            EngineState::Ready => "ready",
            EngineState::Hibernating => "hibernating",
            EngineState::Hibernated => "hibernated",
            EngineState::Failed => "failed",
        }
    }

    fn can_go(&self, to: EngineState) -> bool {
        use EngineState::*;
        matches!(
            (self, to),
            (Closed, Launching)
                | (Launching, Ready)
                | (Launching, Failed)
                | (Ready, Hibernating)
                | (Ready, Closed)
                | (Ready, Failed)
                | (Hibernating, Hibernated)
                | (Hibernating, Failed)
                | (Hibernated, Ready)
                | (Hibernated, Closed)
                | (Hibernated, Failed)
                | (Failed, Closed)
        )
    }
}

// ---------------------------------------------------------------- 事件

/// 后端 → 前端事件（serde 契约 = 前端 `EngineStateMsg`：seq/kind/stage?/reason?）。
#[derive(Debug, Clone, Serialize)]
pub struct EngineEvent {
    pub seq: u64,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 编排共享状态（Tauri 布线与单测共用；锁由调用方持有，步进短临界区）。
pub struct EngineShared {
    pub state: EngineState,
    /// 当前/最后到达的冷启动阶段（诊断用）。
    pub stage: Option<&'static str>,
    pub last_seq: u64,
    /// 步进游标（偶数=发阶段事件，奇数=执行动作；见 launch_step）。
    cursor: u32,
    /// 心跳等待起始时刻（ms；进入 agent-heartbeat 步时记录）。
    hb_started_ms: Option<u64>,
    /// 看门狗连续失联计数。
    pub misses: u32,
    /// 已用掉的心跳自动重启次数。
    pub restarts: u32,
    /// 本会话累计完成的 VM 拉起次数（幂等审计）。
    pub launch_count: u32,
    pub started_at_ms: Option<u64>,
    pub replay: VecDeque<EngineEvent>,
}

impl EngineShared {
    pub fn new() -> Self {
        EngineShared {
            state: EngineState::Closed,
            stage: None,
            last_seq: 0,
            cursor: 0,
            hb_started_ms: None,
            misses: 0,
            restarts: 0,
            launch_count: 0,
            started_at_ms: None,
            replay: VecDeque::new(),
        }
    }

    fn replay_push(&mut self, ev: EngineEvent) {
        if self.replay.len() >= 128 {
            self.replay.pop_front();
        }
        self.replay.push_back(ev);
    }
}

impl Default for EngineShared {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------- 后端 trait 与配置

/// 引擎差分链与 VM 参数（S3.1 布局契约的运行时形态）。
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub base_vhdx: String,
    pub apps_vhdx: String,
    pub user_vhdx: String,
    pub vm_name: String,
    pub heartbeat_port: u16,
    /// 心跳等待硬超时（毫秒）。生产=150s；单测置 0（首个失败探针即超时，瞬时收束）。
    pub boot_timeout_ms: u64,
}

impl EngineConfig {
    /// 测试/显式构造（生产超时；单测用 `cfg()` 辅助把 boot_timeout_ms 置 0）。
    pub fn explicit(base: String, apps: String, user: String) -> Self {
        EngineConfig {
            base_vhdx: base,
            apps_vhdx: apps,
            user_vhdx: user,
            vm_name: "VARIX-Engine".to_string(),
            heartbeat_port: HEARTBEAT_PORT,
            boot_timeout_ms: BOOT_TIMEOUT_SECS * 1000,
        }
    }

    /// 从 WIN_ENGINE 卷解析差分链布局；链未就绪时返回诚实错误（三要素文案）。
    pub fn discover() -> Result<Self, String> {
        let vol = find_engine_volume().ok_or_else(|| {
            "引擎差分链未就绪：未找到 WIN_ENGINE 卷。发生了什么=引擎通道需要 U 盘的 WIN_ENGINE 分区；\
             为什么=当前环境没有该卷（U 盘未插入或分区缺失）；\
             下一步=插入系统 U 盘后重试，或先完成 W2 差分链部署（S3.1）"
                .to_string()
        })?;
        let root = format!("{vol}\\Engine");
        let cfg = EngineConfig {
            base_vhdx: format!("{root}\\Base.vhdx"),
            apps_vhdx: format!("{root}\\Apps.vhdx"),
            user_vhdx: format!("{root}\\User.vhdx"),
            vm_name: "VARIX-Engine".to_string(),
            heartbeat_port: HEARTBEAT_PORT,
            boot_timeout_ms: BOOT_TIMEOUT_SECS * 1000,
        };
        if !std::path::Path::new(&cfg.base_vhdx).is_file() {
            return Err(
                "引擎差分链未就绪：WIN_ENGINE 卷已找到，但 Base.vhdx 缺失。发生了什么=差分链母本层不存在；\
                 为什么=W2 三级差分链尚未部署；\
                 下一步=先执行差分链部署（S3.1 脚本），再回来启用引擎通道"
                    .to_string(),
            );
        }
        Ok(cfg)
    }
}

/// 扫描卷标签找 WIN_ENGINE（按标签定位，绝不按盘符猜——部署纪律同源）。
pub fn find_engine_volume() -> Option<String> {
    volume_label_of("WIN_ENGINE")
}

/// 卷标签查询（Windows 委派 wiring 的 Get-Volume 实现；非 Windows 返回 None）。
pub fn volume_label_of(label: &str) -> Option<String> {
    #[cfg(windows)]
    {
        return wiring::volume_label_of_win(label);
    }
    #[cfg(not(windows))]
    {
        let _ = label;
        None
    }
}

/// 事件汇聚口（Tauri 布线=app.emit；单测=Vec 收集）。
pub type EventSink<'a> = &'a mut dyn FnMut(&EngineEvent);

/// 拉起中止探测（拔盘联动；布线层给 `|| REMOVED.load()`，单测给 `|| false`）。
pub type AbortCheck<'a> = &'a dyn Fn() -> bool;

/// VM 底座抽象（开放性：Hyper-V → 内核 VMX 可替换，不换逻辑核心）。
pub trait EngineBackend: Send {
    fn name(&self) -> &'static str;
    /// 心跳轮询间隔（Mock 覆写为零时长，单测瞬时跑完）。
    fn poll_delay(&self) -> Duration {
        Duration::from_secs(HEARTBEAT_POLL_SECS)
    }
    /// 挂载差分链（幂等）。
    fn mount(&mut self, cfg: &EngineConfig) -> Result<(), String>;
    /// 准备 VM（不存在则创建；幂等）。
    fn vm_prepare(&mut self, cfg: &EngineConfig) -> Result<(), String>;
    /// 上电。
    fn vm_power(&mut self, cfg: &EngineConfig) -> Result<(), String>;
    /// 单次心跳探针（PING→READY；不阻塞超过探针自身超时）。
    fn heartbeat_once(&mut self, cfg: &EngineConfig) -> bool;
    /// 休眠：内存快照写差分盘。
    fn hibernate(&mut self, cfg: &EngineConfig) -> Result<(), String>;
    /// 恢复。
    fn resume(&mut self, cfg: &EngineConfig) -> Result<(), String>;
    /// 优雅停止（VM 关机 + 卸差分盘；盘可能已不在，须尽力而为）。
    fn stop(&mut self, cfg: &EngineConfig) -> Result<(), String>;
}

// ---------------------------------------------------------------- 逻辑核心（后端无关，短步进）

fn fail_transition(from: EngineState, to: EngineState) -> AppError {
    AppError::new(
        "ENGINE_STATE",
        format!("非法状态转移: {} -> {}", from.as_str(), to.as_str()),
    )
}

fn set_state(sh: &mut EngineShared, to: EngineState) -> Result<(), AppError> {
    if !sh.state.can_go(to) {
        return Err(fail_transition(sh.state, to));
    }
    sh.state = to;
    if to == EngineState::Closed || to == EngineState::Failed {
        sh.stage = None;
        sh.misses = 0;
        sh.cursor = 0;
        sh.hb_started_ms = None;
    }
    Ok(())
}

fn emit_event(
    sh: &mut EngineShared,
    kind: &str,
    stage: Option<&str>,
    reason: Option<&str>,
    sink: EventSink,
) {
    sh.last_seq += 1;
    let ev = EngineEvent {
        seq: sh.last_seq,
        kind: kind.to_string(),
        stage: stage.map(|s| s.to_string()),
        reason: reason.map(|s| s.to_string()),
    };
    sh.replay_push(ev.clone());
    sink(&ev);
}

fn abort_err() -> AppError {
    AppError::new("ENGINE_ABORTED", "拉起已中止（U 盘被移除）")
}

/// 拉起步进：每调用推进一步，锁内耗时 = 单个后端动作（≤单条 PS 命令/单次探针）。
/// 返回 Done 表示拉起完成（Ready）。错误即 Failed 已收束（crashed 事件已发）。
pub fn launch_step(
    sh: &mut EngineShared,
    be: &mut dyn EngineBackend,
    cfg: &EngineConfig,
    sink: EventSink,
    abort: AbortCheck,
) -> Result<Step, AppError> {
    if abort() {
        return Err(abort_err());
    }
    if sh.cursor >= 10 {
        return Ok(Step::Done);
    }
    match sh.cursor {
        // 偶数游标：进入阶段 → 发 boot-stage 事件。
        0 | 2 | 4 | 6 | 8 => {
            let idx = (sh.cursor / 2) as usize;
            sh.stage = Some(BOOT_STAGE_KEYS[idx]);
            emit_event(sh, "boot-stage", Some(BOOT_STAGE_KEYS[idx]), None, sink);
            sh.cursor += 1;
            Ok(Step::Advance)
        }
        // 奇数游标：执行对应动作。
        1 => {
            be.mount(cfg).map_err(|e| crash(sh, &e, sink))?;
            sh.cursor += 1;
            Ok(Step::Advance)
        }
        3 => {
            be.vm_prepare(cfg).map_err(|e| crash(sh, &e, sink))?;
            sh.cursor += 1;
            Ok(Step::Advance)
        }
        5 => {
            be.vm_power(cfg).map_err(|e| crash(sh, &e, sink))?;
            sh.cursor += 1;
            Ok(Step::Advance)
        }
        7 => {
            // 心跳等待：从进入本步起计时；READY 即过，超时即诚实失败。
            let t0 = *sh.hb_started_ms.get_or_insert_with(now_ms);
            if be.heartbeat_once(cfg) {
                sh.cursor += 1;
                Ok(Step::Advance)
            } else if now_ms().saturating_sub(t0) >= cfg.boot_timeout_ms {
                Err(crash(
                    sh,
                    &format!(
                        "引擎心跳超时：{} 秒内未收到就绪心跳（47631）。发生了什么=引擎代理未在期限内应答；\
                         为什么=引擎系统启动异常或代理未运行；\
                         下一步=重试一次；连续失败请运行引擎预检（engine_preflight）",
                        BOOT_TIMEOUT_SECS
                    ),
                    sink,
                ))
            } else {
                Ok(Step::SleepMs(be.poll_delay()))
            }
        }
        9 => {
            sh.hb_started_ms = None;
            if !be.heartbeat_once(cfg) {
                return Err(crash(sh, "就绪握手失败：最终心跳探针未通过（READY 未回执）", sink));
            }
            sh.stage = None;
            sh.misses = 0;
            sh.restarts = 0;
            sh.cursor = 0;
            set_state(sh, EngineState::Ready)?;
            emit_event(sh, "ready", None, None, sink);
            Ok(Step::Done)
        }
        _ => Ok(Step::Done),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// 继续下一步。
    Advance,
    /// 步进间等待（锁外 sleep，不占编排锁）。
    SleepMs(Duration),
    /// 拉起完成。
    Done,
}

/// 拉起（幂等）：已在 Launching/Ready 直接 Ok(false)；Hibernated 走恢复；
/// Failed 先复位。驱动完整拉起序列（Mock 零延迟下测试直跑；布线层用步进循环）。
pub fn wake(
    sh: &mut EngineShared,
    be: &mut dyn EngineBackend,
    cfg: &EngineConfig,
    sink: EventSink,
    abort: AbortCheck,
) -> Result<bool, AppError> {
    match sh.state {
        EngineState::Launching | EngineState::Ready => return Ok(false),
        EngineState::Hibernated => {
            resume(sh, be, cfg, sink)?;
            return Ok(false);
        }
        EngineState::Failed => {
            // Failed → Closed → Launching（复位不留死锁态）。
            set_state(sh, EngineState::Closed)?;
            sh.restarts = 0;
        }
        EngineState::Hibernating => {
            return Err(AppError::new(
                "ENGINE_BUSY",
                "引擎正在休眠（内存快照写盘中），稍后再试。发生了什么=编排序列进行中；\
                 为什么=休眠中拉起会打断差分盘写入；\
                 下一步=等待休眠完成后重新打开应用",
            ));
        }
        EngineState::Closed => {}
    }
    set_state(sh, EngineState::Launching)?;
    sh.launch_count += 1;
    sh.started_at_ms = Some(now_ms());
    drive_full(sh, be, cfg, sink, abort)?;
    Ok(true)
}

/// 失败收束：转 Failed 并发 crashed 事件（原因=三要素文案）。
fn crash(sh: &mut EngineShared, reason: &str, sink: EventSink) -> AppError {
    let _ = set_state(sh, EngineState::Failed);
    emit_event(sh, "crashed", None, Some(reason), sink);
    AppError::new("ENGINE_LAUNCH_FAILED", reason.to_string())
}

/// 休眠（Ready→Hibernating→Hibernated；快照写差分盘由后端实现）。
pub fn sleep(
    sh: &mut EngineShared,
    be: &mut dyn EngineBackend,
    cfg: &EngineConfig,
    sink: EventSink,
) -> Result<(), AppError> {
    set_state(sh, EngineState::Hibernating)?;
    match be.hibernate(cfg) {
        Ok(()) => {
            set_state(sh, EngineState::Hibernated)?;
            emit_event(sh, "hibernated", None, None, sink);
            Ok(())
        }
        Err(e) => Err(crash(sh, &format!("休眠失败：{e}"), sink)),
    }
}

/// 恢复（Hibernated→Ready；幂等：已 Ready 直接 Ok）。
pub fn resume(
    sh: &mut EngineShared,
    be: &mut dyn EngineBackend,
    cfg: &EngineConfig,
    sink: EventSink,
) -> Result<(), AppError> {
    if sh.state == EngineState::Ready {
        return Ok(());
    }
    if sh.state != EngineState::Hibernated {
        return Err(fail_transition(sh.state, EngineState::Ready));
    }
    match be.resume(cfg) {
        Ok(()) => {
            set_state(sh, EngineState::Ready)?;
            emit_event(sh, "awake", None, None, sink);
            Ok(())
        }
        Err(e) => Err(crash(sh, &format!("恢复失败：{e}"), sink)),
    }
}

/// 优雅关闭（Ready/Hibernated/Failed→Closed；停 VM + 卸盘 + closed 事件）。
pub fn stop(
    sh: &mut EngineShared,
    be: &mut dyn EngineBackend,
    cfg: &EngineConfig,
    sink: EventSink,
) -> Result<(), AppError> {
    match sh.state {
        EngineState::Closed => return Ok(()),
        EngineState::Ready | EngineState::Hibernated | EngineState::Failed => {}
        EngineState::Launching | EngineState::Hibernating => {
            return Err(AppError::new(
                "ENGINE_BUSY",
                "引擎正在拉起/休眠中，暂不能关闭。发生了什么=编排序列进行中；\
                 为什么=关闭会打断差分盘写入；\
                 下一步=等待当前阶段完成后再关",
            ));
        }
    }
    be.stop(cfg)
        .map_err(|e| AppError::new("ENGINE_STOP_FAILED", format!("引擎停止失败：{e}")))?;
    set_state(sh, EngineState::Closed)?;
    emit_event(sh, "closed", None, None, sink);
    Ok(())
}

/// 看门狗 tick（布线层按 WATCHDOG_TICK_SECS 周期调用；单测手动驱动）。
/// 语义：Ready 态连续 MISSES_BEFORE_RESTART 次失联 → 自动重启一次；
/// 重启仍失联 → Failed（crashed 事件）。重启=closed 事件→完整重驱动拉起。
pub fn watchdog_tick(
    sh: &mut EngineShared,
    be: &mut dyn EngineBackend,
    cfg: &EngineConfig,
    sink: EventSink,
    abort: AbortCheck,
) {
    if sh.state != EngineState::Ready {
        return;
    }
    if be.heartbeat_once(cfg) {
        sh.misses = 0;
        return;
    }
    sh.misses += 1;
    if sh.misses < MISSES_BEFORE_RESTART {
        return;
    }
    if sh.restarts < MAX_HEARTBEAT_RESTARTS {
        sh.restarts += 1;
        sh.misses = 0;
        // closed 事件把前端 lifecycle 拉回 closed（reducer 此后才接受 boot-stage），
        // 再走完整拉起序列（mount/prepare 幂等）。
        emit_event(
            sh,
            "closed",
            None,
            Some("心跳失联，引擎自动重启中（第 1 次重启）"),
            sink,
        );
        sh.state = EngineState::Closed;
        if set_state(sh, EngineState::Launching).is_ok() {
            sh.launch_count += 1;
            let _ = drive_full(sh, be, cfg, sink, abort);
        }
    } else {
        let _ = crash(sh, "心跳持续失联：自动重启后仍未恢复，引擎已停止", sink);
    }
}

/// 完整驱动（wake 与看门狗重启复用；布线层主拉起用步进循环保证状态可查）。
fn drive_full(
    sh: &mut EngineShared,
    be: &mut dyn EngineBackend,
    cfg: &EngineConfig,
    sink: EventSink,
    abort: AbortCheck,
) -> Result<(), AppError> {
    loop {
        match launch_step(sh, be, cfg, sink, abort)? {
            Step::Advance => continue,
            Step::SleepMs(d) => std::thread::sleep(d),
            Step::Done => return Ok(()),
        }
    }
}

/// 拔盘联动（异常场景一/三）：任何非 Closed 态 → 尽力停止 → Closed + usb-removed。
/// 幂等：Closed 后再通知 = no-op。
pub fn notify_usb_removed(
    sh: &mut EngineShared,
    be: &mut dyn EngineBackend,
    cfg: &EngineConfig,
    sink: EventSink,
) {
    if sh.state == EngineState::Closed {
        return;
    }
    let _ = be.stop(cfg); // 尽力而为：盘可能已不在，失败不补救
    sh.state = EngineState::Closed;
    sh.stage = None;
    sh.misses = 0;
    sh.cursor = 0;
    sh.hb_started_ms = None;
    emit_event(
        sh,
        "usb-removed",
        None,
        Some("U 盘已移除，引擎会话已结束；下次插入自动恢复"),
        sink,
    );
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ---------------------------------------------------------------- 心跳探针（与 vm_agent.rs 协议同源）

/// 单次 PING→READY 探针（阻塞 ≤2s；失败=无心跳）。
pub fn ping_heartbeat(port: u16) -> bool {
    use std::io::{Read, Write};
    let addr = format!("127.0.0.1:{port}");
    let Ok(mut s) = TcpStream::connect(&addr) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
    if s.write_all(b"PING").is_err() {
        return false;
    }
    let mut buf = [0u8; 64];
    match s.read(&mut buf) {
        Ok(n) => n > 0 && buf[..n].starts_with(b"READY"),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------- 命令层 DTO

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: String,
    pub stage: Option<String>,
    pub last_seq: u64,
    pub launch_count: u32,
    pub backend: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preflight {
    pub hyperv_available: bool,
    pub elevated: bool,
    pub engine_volume: Option<String>,
    pub chain_base: Option<String>,
    pub chain_apps: Option<String>,
    pub chain_user: Option<String>,
    pub chain_complete: bool,
}

// ---------------------------------------------------------------- Tauri 全局布线（仅 Windows）

#[cfg(windows)]
mod wiring {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};

    static ENGINE: OnceLock<Mutex<EngineShared>> = OnceLock::new();
    /// 编排驱动标志：一次只允许一个线程驱动序列（跨线程幂等的第二道保险）。
    static BUSY: AtomicBool = AtomicBool::new(false);
    /// 拔盘标志（usb_removed 置位；拉起步进见位即安静中止；新唤醒时复位）。
    static REMOVED: AtomicBool = AtomicBool::new(false);
    /// usb-removed 事件只发一次（监视器与拉起线程谁先谁发）。
    static REMOVED_EMITTED: AtomicBool = AtomicBool::new(false);

    fn engine() -> &'static Mutex<EngineShared> {
        ENGINE.get_or_init(|| Mutex::new(EngineShared::new()))
    }

    fn busy_try() -> bool {
        BUSY.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }
    fn busy_rel() {
        BUSY.store(false, Ordering::SeqCst);
    }

    /// BUSY 的 RAII 守卫（同步命令用；拉起线程跨线程持有走手动 release）。
    struct BusyGuard;
    impl BusyGuard {
        fn take() -> Option<Self> {
            if busy_try() {
                Some(BusyGuard)
            } else {
                None
            }
        }
    }
    impl Drop for BusyGuard {
        fn drop(&mut self) {
            busy_rel();
        }
    }

    fn tauri_sink(app: &tauri::AppHandle) -> impl FnMut(&EngineEvent) + '_ {
        move |ev: &EngineEvent| {
            let _ = app.emit(ENGINE_EVENT, ev);
        }
    }

    fn no_sink() -> impl FnMut(&EngineEvent) {
        |_| {}
    }

    fn removed_check() -> bool {
        REMOVED.load(Ordering::SeqCst)
    }

    /// 发一次（且仅一次）usb-removed 并把状态收束为 Closed（拔盘场景不做 PS 尽力停止，
    /// 防卡在无效系统调用；空闲路径的 notify_usb_removed 仍做尽力停止）。
    fn emit_removed_once(app: &tauri::AppHandle) {
        if REMOVED_EMITTED.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Ok(mut sh) = engine().lock() {
            let mut sink = tauri_sink(app);
            sh.state = EngineState::Closed;
            sh.stage = None;
            sh.misses = 0;
            sh.cursor = 0;
            sh.hb_started_ms = None;
            emit_event(
                &mut sh,
                "usb-removed",
                None,
                Some("U 盘已移除，引擎会话已结束；下次插入自动恢复"),
                &mut sink,
            );
        }
    }

    // ---- Hyper-V 底座（真实实现；全部经 PowerShell cmdlet） ----

    pub struct HyperVBackend;

    impl HyperVBackend {
        pub fn new() -> Self {
            HyperVBackend
        }

        pub fn module_available() -> bool {
            ps_ok("if (Get-Module -ListAvailable -Name Hyper-V) { exit 0 } else { exit 7 }")
        }

        pub fn is_elevated() -> bool {
            ps_ok(
                "$p=[Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent(); \
                 if ($p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { exit 0 } else { exit 7 }",
            )
        }
    }

    impl Default for HyperVBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    impl EngineBackend for HyperVBackend {
        fn name(&self) -> &'static str {
            "hyperv"
        }

        fn mount(&mut self, cfg: &EngineConfig) -> Result<(), String> {
            if !HyperVBackend::module_available() {
                return Err(
                    "Hyper-V 底座不可用。发生了什么=本机未启用 Hyper-V 功能；\
                     为什么=引擎通道过渡期依赖 Hyper-V 挂载差分盘与运行 VM；\
                     下一步=在「启用或关闭 Windows 功能」中启用 Hyper-V 后重试"
                        .to_string(),
                );
            }
            if !HyperVBackend::is_elevated() {
                return Err(
                    "引擎编排需要管理员权限。发生了什么=Hyper-V cmdlet 拒绝非提权调用；\
                     为什么=挂载 VHDX 与控制 VM 是系统级操作；\
                     下一步=以管理员身份运行 Variable 后重试"
                        .to_string(),
                );
            }
            for p in [&cfg.apps_vhdx, &cfg.user_vhdx] {
                if !std::path::Path::new(p).is_file() {
                    return Err(format!(
                        "差分链不完整：缺失 {p}。发生了什么=差分链上层文件不存在；\
                         为什么=W2 链未部署或被清理；\
                         下一步=重跑差分链部署脚本（S3.1）"
                    ));
                }
            }
            ps_check(&format!(
                "$v=Get-VHD -Path '{u}' -ErrorAction SilentlyContinue; \
                 if ($v -and $v.Attached) {{ exit 0 }}; \
                 if (-not (Test-Path -LiteralPath '{u}')) {{ exit 8 }}; \
                 Mount-VHD -Path '{u}'",
                u = cfg.user_vhdx
            ))
        }

        fn vm_prepare(&mut self, cfg: &EngineConfig) -> Result<(), String> {
            ps_check(&format!(
                "$v=Get-VM -Name '{n}' -ErrorAction SilentlyContinue; \
                 if ($v) {{ exit 0 }}; \
                 $nv = New-VM -Name '{n}' -MemoryStartupBytes 4GB -Generation 2 -ErrorAction Stop; \
                 Set-VMFirmware -VM $nv -EnableSecureBoot Off; \
                 Add-VMHardDiskDrive -VM $nv -Path '{u}'",
                n = cfg.vm_name,
                u = cfg.user_vhdx
            ))
        }

        fn vm_power(&mut self, cfg: &EngineConfig) -> Result<(), String> {
            ps_check(&format!(
                "$v=Get-VM -Name '{n}' -ErrorAction Stop; \
                 if ($v.State -eq 'Running') {{ exit 0 }}; \
                 Start-VM -VM $v",
                n = cfg.vm_name
            ))
        }

        fn heartbeat_once(&mut self, cfg: &EngineConfig) -> bool {
            ping_heartbeat(cfg.heartbeat_port)
        }

        fn hibernate(&mut self, cfg: &EngineConfig) -> Result<(), String> {
            // Save-VM = 内存快照写差分盘（走既有差分链，不新造快照格式）。
            ps_check(&format!("Save-VM -Name '{n}'", n = cfg.vm_name))
        }

        fn resume(&mut self, cfg: &EngineConfig) -> Result<(), String> {
            ps_check(&format!(
                "$v=Get-VM -Name '{n}' -ErrorAction Stop; \
                 if ($v.State -eq 'Running') {{ exit 0 }}; \
                 Start-VM -VM $v",
                n = cfg.vm_name
            ))
        }

        fn stop(&mut self, cfg: &EngineConfig) -> Result<(), String> {
            // 先停 VM（尽力），再卸 User 层（尽力）——拔盘场景盘可能已不在。
            let _ = ps_check(&format!(
                "Stop-VM -Name '{n}' -Force -ErrorAction SilentlyContinue",
                n = cfg.vm_name
            ));
            let _ = ps_check(&format!(
                "$v=Get-VHD -Path '{u}' -ErrorAction SilentlyContinue; \
                 if ($v -and $v.Attached) {{ Dismount-VHD -Path '{u}' }}",
                u = cfg.user_vhdx
            ));
            Ok(())
        }
    }

    fn ps_check(script: &str) -> Result<(), String> {
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
            .output()
            .map_err(|e| format!("PowerShell 启动失败：{e}"))?;
        if out.status.success() {
            return Ok(());
        }
        if out.status.code() == Some(8) {
            return Err("差分盘文件不存在".to_string());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(3).collect();
        let tail = tail.into_iter().rev().collect::<Vec<_>>().join("; ");
        Err(format!(
            "Hyper-V 操作失败（exit={:?}）：{tail}",
            out.status.code()
        ))
    }

    fn ps_ok(script: &str) -> bool {
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// 卷标签查询（按标签定位卷，绝不按盘符猜）。
    pub fn volume_label_of_win(label: &str) -> Option<String> {
        let script = format!(
            "$v=Get-Volume -FileSystemLabel '{label}' -ErrorAction SilentlyContinue | Select-Object -First 1; \
             if ($v -and $v.DriveLetter) {{ Write-Output ($v.DriveLetter + ':') }}"
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    // ---- 对外只读面 ----

    pub fn preflight() -> Preflight {
        let cfg = EngineConfig::discover();
        let (chain_ok, base, apps, user) = match &cfg {
            Ok(c) => (
                std::path::Path::new(&c.apps_vhdx).is_file()
                    && std::path::Path::new(&c.user_vhdx).is_file(),
                Some(c.base_vhdx.clone()),
                Some(c.apps_vhdx.clone()),
                Some(c.user_vhdx.clone()),
            ),
            Err(_) => (false, None, None, None),
        };
        Preflight {
            hyperv_available: HyperVBackend::module_available(),
            elevated: HyperVBackend::is_elevated(),
            engine_volume: find_engine_volume(),
            chain_base: base,
            chain_apps: apps,
            chain_user: user,
            chain_complete: chain_ok,
        }
    }

    pub fn status() -> Status {
        match engine().lock() {
            Ok(sh) => Status {
                state: sh.state.as_str().to_string(),
                stage: sh.stage.map(|s| s.to_string()),
                last_seq: sh.last_seq,
                launch_count: sh.launch_count,
                backend: "hyperv",
            },
            Err(_) => Status {
                state: "closed".to_string(),
                stage: None,
                last_seq: 0,
                launch_count: 0,
                backend: "hyperv",
            },
        }
    }

    pub fn replay() -> Vec<EngineEvent> {
        match engine().lock() {
            Ok(sh) => sh.replay.iter().cloned().collect(),
            Err(_) => Vec::new(),
        }
    }

    // ---- 唤醒（步进循环在独立线程；每步短持锁，status 永远可响应） ----

    pub fn wake_async(app: tauri::AppHandle) -> Result<Status, String> {
        // 新会话：清上一轮拔盘标记（重新插入后的唤醒必须能走完拉起）。
        REMOVED.store(false, Ordering::SeqCst);
        REMOVED_EMITTED.store(false, Ordering::SeqCst);
        let cfg = EngineConfig::discover()?;
        // 幂等快路径：Launching/Ready 不再起线程。
        {
            let sh = engine().lock().map_err(|_| "引擎状态锁中毒".to_string())?;
            if matches!(sh.state, EngineState::Launching | EngineState::Ready) {
                return Ok(status());
            }
        }
        if !busy_try() {
            // 已有线程在驱动（看门狗重启/另一 wake）→ 幂等返回当前态。
            return Ok(status());
        }
        let handle = app.clone();
        let spawned = std::thread::Builder::new()
            .name("engine-launch".into())
            .spawn(move || {
                let run = || -> Result<(), AppError> {
                    let mut be = HyperVBackend::new();
                    // 状态复位（Closed/Failed→Launching / Hibernated→恢复）：短临界区。
                    {
                        let mut sh = engine()
                            .lock()
                            .map_err(|_| AppError::new("ENGINE_LOCK", "引擎状态锁中毒"))?;
                        match sh.state {
                            EngineState::Launching | EngineState::Ready => return Ok(()), // 并发先行完成
                            EngineState::Failed => {
                                set_state(&mut sh, EngineState::Closed)?;
                                sh.restarts = 0;
                            }
                            EngineState::Hibernating => {
                                return Err(AppError::new(
                                    "ENGINE_BUSY",
                                    "引擎正在休眠，暂不能拉起。发生了什么=内存快照写盘中；\
                                     为什么=休眠中拉起会打断差分盘写入；\
                                     下一步=等待休眠完成后重试",
                                ))
                            }
                            EngineState::Hibernated => {
                                let mut sink = tauri_sink(&handle);
                                resume(&mut sh, &mut be, &cfg, &mut sink)?;
                                return Ok(());
                            }
                            EngineState::Closed => {}
                        }
                        set_state(&mut sh, EngineState::Launching)?;
                        sh.launch_count += 1;
                        sh.started_at_ms = Some(now_ms());
                    }
                    // 步进循环：每步短持锁，状态查询全程可响应。
                    loop {
                        let step = {
                            let mut sh = engine()
                                .lock()
                                .map_err(|_| AppError::new("ENGINE_LOCK", "引擎状态锁中毒"))?;
                            let mut sink = tauri_sink(&handle);
                            launch_step(&mut sh, &mut be, &cfg, &mut sink, &removed_check)
                        };
                        match step {
                            Ok(Step::Advance) => continue,
                            Ok(Step::SleepMs(d)) => std::thread::sleep(d),
                            Ok(Step::Done) => return Ok(()),
                            Err(e) if e.code == "ENGINE_ABORTED" => {
                                emit_removed_once(&handle);
                                return Ok(());
                            }
                            Err(_) => return Ok(()), // Failed 已由 crash 收束并广播
                        }
                    }
                };
                let _ = run();
                busy_rel();
                spawn_watchdog(handle.clone());
            });
        if spawned.is_err() {
            busy_rel();
            return Err("引擎编排线程启动失败".to_string());
        }
        Ok(status())
    }

    /// 看门狗线程（单例；每 tick 短持锁，仅 Ready 态生效）。
    fn spawn_watchdog(handle: tauri::AppHandle) {
        static WATCHDOG_SPAWNED: OnceLock<()> = OnceLock::new();
        if WATCHDOG_SPAWNED.set(()).is_err() {
            return;
        }
        std::thread::Builder::new()
            .name("engine-watchdog".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_secs(WATCHDOG_TICK_SECS));
                if removed_check() {
                    emit_removed_once(&handle);
                    continue;
                }
                let quick_state = engine()
                    .lock()
                    .map(|sh| sh.state)
                    .unwrap_or(EngineState::Closed);
                if quick_state != EngineState::Ready {
                    continue;
                }
                let Ok(cfg) = EngineConfig::discover() else {
                    continue; // 盘不在场：留给拔盘链路收束
                };
                let mut be = HyperVBackend::new();
                if let Ok(mut sh) = engine().lock() {
                    let mut sink = tauri_sink(&handle);
                    watchdog_tick(&mut sh, &mut be, &cfg, &mut sink, &removed_check);
                }
            })
            .map_err(|e| eprintln!("engine-watchdog spawn failed: {e}"))
            .ok();
    }

    pub fn sleep_now() -> Result<Status, String> {
        let cfg = EngineConfig::discover()?;
        let Some(_busy) = BusyGuard::take() else {
            return Err(
                "引擎编排进行中，暂不能休眠。发生了什么=另一编排序列持有驱动权；\
                 为什么=并发编排会造成双开；\
                 下一步=稍候重试"
                    .to_string(),
            );
        };
        let mut be = HyperVBackend::new();
        let mut sh = engine().lock().map_err(|_| "引擎状态锁中毒".to_string())?;
        let mut sink = no_sink();
        sleep(&mut sh, &mut be, &cfg, &mut sink).map_err(|e| e.message)?;
        Ok(status())
    }

    pub fn resume_now() -> Result<Status, String> {
        let cfg = EngineConfig::discover()?;
        let Some(_busy) = BusyGuard::take() else {
            return Err("引擎编排进行中，暂不能恢复。发生了什么=另一编排序列持有驱动权；\
                 为什么=并发编排会造成双开；\
                 下一步=稍候重试"
                .to_string());
        };
        let mut be = HyperVBackend::new();
        let mut sh = engine().lock().map_err(|_| "引擎状态锁中毒".to_string())?;
        let mut sink = no_sink();
        resume(&mut sh, &mut be, &cfg, &mut sink).map_err(|e| e.message)?;
        Ok(status())
    }

    pub fn stop_now() -> Result<Status, String> {
        let cfg = EngineConfig::discover()?;
        let Some(_busy) = BusyGuard::take() else {
            return Err("引擎编排进行中，暂不能关闭。发生了什么=另一编排序列持有驱动权；\
                 为什么=并发编排会造成双开；\
                 下一步=等待当前阶段完成".to_string());
        };
        let mut be = HyperVBackend::new();
        let mut sh = engine().lock().map_err(|_| "引擎状态锁中毒".to_string())?;
        let mut sink = no_sink();
        stop(&mut sh, &mut be, &cfg, &mut sink).map_err(|e| e.message)?;
        Ok(status())
    }

    /// 拔盘联动入口（usb.rs 监视器调用）：置 REMOVED 标志 + 尽力收束。
    /// 编排线程持锁时由其 abort 路径收束（emit_removed_once 幂等去重）。
    pub fn usb_removed(app: tauri::AppHandle) {
        REMOVED.store(true, Ordering::SeqCst);
        if let Ok(sh) = engine().try_lock() {
            if sh.state != EngineState::Closed {
                drop(sh);
                emit_removed_once(&app);
            }
        }
    }
}

#[cfg(windows)]
pub use wiring::{preflight, replay, resume_now, sleep_now, status, stop_now, usb_removed, wake_async};

// ---------------------------------------------------------------- 命令层

/// 引擎状态查询（随时可调，不动编排）。
#[tauri::command(async)]
pub fn engine_status() -> CmdResult<Status> {
    #[cfg(windows)]
    {
        return Ok(status());
    }
    #[cfg(not(windows))]
    {
        Ok(Status {
            state: "closed".to_string(),
            stage: None,
            last_seq: 0,
            launch_count: 0,
            backend: "unavailable",
        })
    }
}

/// 重放缓冲（晚挂监听的前端补事件，对齐 boot_replay 模式）。
#[tauri::command(async)]
pub fn engine_replay() -> CmdResult<Vec<EngineEvent>> {
    #[cfg(windows)]
    {
        return Ok(replay());
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

/// 引擎预检（设置页/诊断；只读，不动编排状态）。
#[tauri::command(async)]
pub fn engine_preflight() -> CmdResult<Preflight> {
    #[cfg(windows)]
    {
        return Ok(preflight());
    }
    #[cfg(not(windows))]
    {
        Ok(Preflight {
            hyperv_available: false,
            elevated: false,
            engine_volume: None,
            chain_base: None,
            chain_apps: None,
            chain_user: None,
            chain_complete: false,
        })
    }
}

/// 唤醒/拉起（幂等；冷启动重活在线程里按步推进，立即返回当前态）。
#[tauri::command(async)]
pub fn engine_wake(app: tauri::AppHandle) -> CmdResult<Status> {
    #[cfg(windows)]
    {
        return wake_async(app).map_err(|e| AppError::new("ENGINE_WAKE", e));
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err(AppError::new(
            "ENGINE_UNSUPPORTED",
            "引擎通道仅支持 Windows 宿主",
        ))
    }
}

/// 休眠（内存快照写差分盘）。
#[tauri::command(async)]
pub fn engine_sleep() -> CmdResult<Status> {
    #[cfg(windows)]
    {
        return sleep_now().map_err(|e| AppError::new("ENGINE_SLEEP", e));
    }
    #[cfg(not(windows))]
    {
        Err(AppError::new(
            "ENGINE_UNSUPPORTED",
            "引擎通道仅支持 Windows 宿主",
        ))
    }
}

/// 恢复。
#[tauri::command(async)]
pub fn engine_resume() -> CmdResult<Status> {
    #[cfg(windows)]
    {
        return resume_now().map_err(|e| AppError::new("ENGINE_RESUME", e));
    }
    #[cfg(not(windows))]
    {
        Err(AppError::new(
            "ENGINE_UNSUPPORTED",
            "引擎通道仅支持 Windows 宿主",
        ))
    }
}

/// 优雅关闭。
#[tauri::command(async)]
pub fn engine_stop() -> CmdResult<Status> {
    #[cfg(windows)]
    {
        return stop_now().map_err(|e| AppError::new("ENGINE_STOP", e));
    }
    #[cfg(not(windows))]
    {
        Err(AppError::new(
            "ENGINE_UNSUPPORTED",
            "引擎通道仅支持 Windows 宿主",
        ))
    }
}


// ---------------------------------------------------------------- 测试（Mock 后端，逻辑核心全覆盖）
//
// 测试脚手架纪律：sh / be / config / events 必须是相互独立的局部变量
// （经 &mut 结构体的字段借用会被闭包整体捕获）；事件闭包必须内联传参，
// 让期望签名（dyn for<'x> FnMut(&'x EngineEvent)）推断出高阶生命周期。

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock 后端：脚本化心跳（按次消费，耗尽后重复末值）+ 失败点 + 计数器。
    struct MockBackend {
        /// 心跳脚本：每次探针按序消费一个值；耗尽后重复末值；空脚本=恒 true。
        heartbeat_script: Vec<bool>,
        /// 在该阶段失败（stage key）。
        fail_at: Option<&'static str>,
        probes: u32,
        mounts: u32,
        preps: u32,
        powers: u32,
        hibernates: u32,
        resumes: u32,
        stops: u32,
    }

    impl MockBackend {
        fn fast() -> Self {
            MockBackend {
                heartbeat_script: Vec::new(),
                fail_at: None,
                probes: 0,
                mounts: 0,
                preps: 0,
                powers: 0,
                hibernates: 0,
                resumes: 0,
                stops: 0,
            }
        }
        fn script(parts: &[bool]) -> Self {
            let mut m = MockBackend::fast();
            m.heartbeat_script = parts.to_vec();
            m
        }
        fn probe(&mut self) -> bool {
            if self.heartbeat_script.is_empty() {
                return true;
            }
            let idx = (self.probes as usize).min(self.heartbeat_script.len() - 1);
            self.probes += 1;
            self.heartbeat_script[idx]
        }
    }

    impl EngineBackend for MockBackend {
        fn name(&self) -> &'static str {
            "mock"
        }
        fn poll_delay(&self) -> Duration {
            Duration::ZERO
        }
        fn mount(&mut self, _c: &EngineConfig) -> Result<(), String> {
            self.mounts += 1;
            if self.fail_at == Some(STAGE_VHDX_MOUNT) {
                return Err("模拟挂载失败".into());
            }
            Ok(())
        }
        fn vm_prepare(&mut self, _c: &EngineConfig) -> Result<(), String> {
            self.preps += 1;
            if self.fail_at == Some(STAGE_VM_CREATE) {
                return Err("模拟建机失败".into());
            }
            Ok(())
        }
        fn vm_power(&mut self, _c: &EngineConfig) -> Result<(), String> {
            self.powers += 1;
            if self.fail_at == Some(STAGE_VM_POWER) {
                return Err("模拟上电失败".into());
            }
            Ok(())
        }
        fn heartbeat_once(&mut self, _c: &EngineConfig) -> bool {
            self.probe()
        }
        fn hibernate(&mut self, _c: &EngineConfig) -> Result<(), String> {
            self.hibernates += 1;
            Ok(())
        }
        fn resume(&mut self, _c: &EngineConfig) -> Result<(), String> {
            self.resumes += 1;
            Ok(())
        }
        fn stop(&mut self, _c: &EngineConfig) -> Result<(), String> {
            self.stops += 1;
            Ok(())
        }
    }

    fn cfg() -> EngineConfig {
        let mut c = EngineConfig::explicit(
            "X:\\Engine\\Base.vhdx".into(),
            "X:\\Engine\\Apps.vhdx".into(),
            "X:\\Engine\\User.vhdx".into(),
        );
        c.boot_timeout_ms = 0; // 单测瞬时收束（首个失败探针即超时）
        c
    }

    fn no_abort() -> bool {
        false
    }

    fn kinds(events: &[EngineEvent]) -> Vec<String> {
        events.iter().map(|e| e.kind.clone()).collect()
    }

    #[test]
    fn launch_emits_five_stages_then_ready_with_monotonic_seq() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        assert_eq!(sh.state, EngineState::Ready);
        assert_eq!(sh.launch_count, 1);
        // 五阶段键与前端 ENGINE_BOOT_STAGES 严格一致，顺序正确。
        let stages: Vec<&str> = events
            .iter()
            .filter(|e| e.kind == "boot-stage")
            .filter_map(|e| e.stage.as_deref())
            .collect();
        assert_eq!(
            stages,
            vec![
                STAGE_VHDX_MOUNT,
                STAGE_VM_CREATE,
                STAGE_VM_POWER,
                STAGE_AGENT_HEARTBEAT,
                STAGE_READY_HANDSHAKE
            ]
        );
        assert_eq!(kinds(&events).last().unwrap(), "ready");
        for (a, b) in events.iter().zip(events.iter().skip(1)) {
            assert!(b.seq > a.seq, "seq 必须单调: {} -> {}", a.seq, b.seq);
        }
    }

    #[test]
    fn wake_is_idempotent_no_double_launch() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        let again = wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        assert!(!again, "重复 wake 必须被幂等吸收");
        assert_eq!(sh.launch_count, 1, "launch_count 必须为 1（不双开）");
        assert_eq!(be.preps, 1, "VM 准备只发生一次");
        assert_eq!(kinds(&events).iter().filter(|k| *k == "ready").count(), 1);
    }

    #[test]
    fn launch_failure_transitions_to_failed_with_crashed_event() {
        for stage in [STAGE_VHDX_MOUNT, STAGE_VM_CREATE, STAGE_VM_POWER] {
            let mut sh = EngineShared::new();
            let mut be = MockBackend::fast();
            be.fail_at = Some(stage);
            let config = cfg();
            let mut events: Vec<EngineEvent> = Vec::new();
            let err = wake(
                &mut sh, &mut be, &config,
                &mut |ev: &EngineEvent| events.push(ev.clone()),
                &no_abort,
            )
            .unwrap_err();
            assert_eq!(err.code, "ENGINE_LAUNCH_FAILED");
            assert_eq!(sh.state, EngineState::Failed);
            assert_eq!(kinds(&events).last().unwrap(), "crashed");
            assert!(events.last().unwrap().reason.is_some(), "失败必须带三要素原因");
        }
    }

    #[test]
    fn heartbeat_timeout_crashes_honestly() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::script(&[false]); // 永不就绪（零延迟 → 150 次探针瞬时）
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        let err = wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap_err();
        assert_eq!(err.code, "ENGINE_LAUNCH_FAILED");
        assert_eq!(sh.state, EngineState::Failed);
        let last = events.last().unwrap();
        assert_eq!(last.kind, "crashed");
        let reason = last.reason.as_deref().unwrap_or("");
        assert!(reason.contains("超时"), "超时原因必须如实：{reason}");
    }

    #[test]
    fn failed_can_recover_via_wake_reset() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        be.fail_at = Some(STAGE_VM_POWER);
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        let err = wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        );
        assert!(err.is_err());
        be.fail_at = None;
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        assert_eq!(sh.state, EngineState::Ready);
        assert_eq!(sh.launch_count, 2);
    }

    #[test]
    fn sleep_resume_roundtrip_and_states() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        sleep(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap();
        assert_eq!(sh.state, EngineState::Hibernated);
        assert_eq!(kinds(&events).last().unwrap(), "hibernated");
        resume(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap();
        assert_eq!(sh.state, EngineState::Ready);
        assert_eq!(kinds(&events).last().unwrap(), "awake");
        resume(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap(); // 幂等
        assert_eq!(sh.state, EngineState::Ready);
        assert_eq!(be.hibernates, 1);
        assert_eq!(be.resumes, 1);
    }

    #[test]
    fn stop_from_ready_and_closed_idempotent() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        stop(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap();
        assert_eq!(sh.state, EngineState::Closed);
        assert_eq!(kinds(&events).last().unwrap(), "closed");
        stop(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap(); // 幂等
        assert_eq!(be.stops, 1);
    }

    #[test]
    fn illegal_transition_rejected() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        let err = sleep(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()))
            .unwrap_err();
        assert_eq!(err.code, "ENGINE_STATE");
        assert_eq!(sh.state, EngineState::Closed, "非法转移不改变状态");
    }

    #[test]
    fn wake_from_hibernated_auto_resumes() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        sleep(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap();
        let launched = wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        assert!(!launched, "Hibernated 的 wake 走恢复语义");
        assert_eq!(sh.state, EngineState::Ready);
        assert_eq!(be.resumes, 1);
        assert_eq!(kinds(&events).last().unwrap(), "awake");
    }

    #[test]
    fn watchdog_recovers_via_single_restart_then_ready() {
        let mut sh = EngineShared::new();
        // 双后端：初始拉起用全绿 fast；看门狗失联/重启用脚本
        // [false,false,false,true]（tick1/2/3 各失败一次，重启后的拉起探针成功，
        // 耗尽后重复末值 true）。
        let mut ready_be = MockBackend::fast();
        let mut dead_be = MockBackend::script(&[false, false, false, true]);
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut ready_be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        assert_eq!(sh.state, EngineState::Ready);
        watchdog_tick(&mut sh, &mut dead_be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()), &no_abort);
        watchdog_tick(&mut sh, &mut dead_be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()), &no_abort);
        assert_eq!(sh.state, EngineState::Ready, "未达阈值前不处置");
        watchdog_tick(&mut sh, &mut dead_be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()), &no_abort);
        assert_eq!(sh.state, EngineState::Ready, "重启一次后应回到 Ready");
        let ks = kinds(&events);
        assert!(ks.contains(&"closed".to_string()), "重启前必须发 closed 事件");
        assert_eq!(ks.last().unwrap(), "ready");
        assert_eq!(dead_be.preps, 1, "重启路径 = 由 dead_be 再做一次 VM 准备");
        assert_eq!(sh.launch_count, 2);
    }

    #[test]
    fn watchdog_gives_up_after_restart_budget() {
        let mut sh = EngineShared::new();
        let mut ready_be = MockBackend::fast(); // 初始就绪
        let mut dead_be = MockBackend::script(&[false]); // 之后永久失联（含重启后的拉起）
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut ready_be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        assert_eq!(sh.state, EngineState::Ready);
        for _ in 0..MISSES_BEFORE_RESTART {
            watchdog_tick(&mut sh, &mut dead_be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()), &no_abort);
        }
        assert_eq!(sh.state, EngineState::Failed, "重启仍失联 → Failed");
        assert_eq!(kinds(&events).last().unwrap(), "crashed");
        // Failed 态 tick = no-op。
        let n = events.len();
        watchdog_tick(&mut sh, &mut dead_be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()), &no_abort);
        assert_eq!(events.len(), n);
    }

    #[test]
    fn watchdog_ignores_non_ready_states() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        watchdog_tick(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()), &no_abort);
        assert_eq!(sh.state, EngineState::Closed);
        assert!(events.is_empty());
    }

    #[test]
    fn usb_removed_closes_session_and_emits_once() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        notify_usb_removed(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()));
        assert_eq!(sh.state, EngineState::Closed);
        assert_eq!(kinds(&events).last().unwrap(), "usb-removed");
        assert!(be.stops >= 1, "拔盘必须尽力停止 VM/卸盘");
        let n = events.len();
        notify_usb_removed(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())); // 幂等
        assert_eq!(events.len(), n);
    }

    #[test]
    fn abort_during_launch_leaves_no_crash() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        // 从第一步起即中止（模拟拉起中拔盘）：布线层收到 ABORTED 后收束
        // Closed + usb-removed（emit_removed_once），逻辑核心不伪造崩溃事件。
        let always_abort = || true;
        let err = wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &always_abort,
        )
        .unwrap_err();
        assert_eq!(err.code, "ENGINE_ABORTED");
        assert!(!kinds(&events).contains(&"crashed".to_string()));
    }

    #[test]
    fn replay_buffer_bounded_and_ordered() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        for _ in 0..20 {
            wake(
                &mut sh, &mut be, &config,
                &mut |ev: &EngineEvent| events.push(ev.clone()),
                &no_abort,
            )
            .unwrap();
            stop(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap();
        }
        assert!(sh.replay.len() <= 128, "重放缓冲必须有界");
        let seqs: Vec<u64> = sh.replay.iter().map(|e| e.seq).collect();
        let mut sorted = seqs.clone();
        sorted.sort_unstable();
        assert_eq!(seqs, sorted, "replay 缓冲内 seq 保持追加序");
    }

    #[test]
    fn transition_table_matches_spec() {
        use EngineState::*;
        assert!(Closed.can_go(Launching));
        assert!(Launching.can_go(Ready));
        assert!(Launching.can_go(Failed));
        assert!(Ready.can_go(Hibernating));
        assert!(Ready.can_go(Closed));
        assert!(Ready.can_go(Failed));
        assert!(Hibernating.can_go(Hibernated));
        assert!(Hibernated.can_go(Ready));
        assert!(Hibernated.can_go(Closed));
        assert!(Failed.can_go(Closed));
        assert!(!Closed.can_go(Ready));
        assert!(!Closed.can_go(Hibernating));
        assert!(!Ready.can_go(Launching));
        assert!(!Hibernated.can_go(Hibernating));
        assert!(!Failed.can_go(Ready));
    }

    #[cfg(feature = "vm-agent")]
    #[test]
    fn heartbeat_port_matches_vm_agent() {
        assert_eq!(HEARTBEAT_PORT, crate::vm_agent::HEARTBEAT_PORT);
    }

    #[test]
    fn event_serde_contract_matches_frontend() {
        // 前端 EngineStateMsg: { seq, kind, stage?, reason? } —— 可选字段缺席不破坏解析。
        let mut sh = EngineShared::new();
        emit_event(&mut sh, "boot-stage", Some(STAGE_VHDX_MOUNT), None, &mut |_| {});
        let json = serde_json::to_value(&sh.replay[0]).unwrap();
        assert_eq!(json["kind"], "boot-stage");
        assert_eq!(json["stage"], STAGE_VHDX_MOUNT);
        assert!(json.get("reason").is_none(), "无 reason 时键必须缺席");
        assert!(json["seq"].is_u64());
    }
}
