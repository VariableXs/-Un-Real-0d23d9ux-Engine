//! AI-15 V-83 计划任务工坊 + V-86 启动项延迟编排。
//!
//! V-83 红线（承化境计划）：
//! - 动作白名单制（只有环境注册的动作可被定时，绝不执行任意命令）；
//! - 不写系统任务计划程序库（用户级本地调度器，data_dir 存储）；
//! - 执行日志最近 20 次。
//!
//! V-86 红线：
//! - 延迟 = 环境启动完成后 N 秒拉起（不依赖系统任务计划）；
//! - 安全类启动项（杀毒/驱动辅助）默认不显示延迟选项（前端规则）；
//! - 不做智能自动编排、不做依赖感知并行。

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ===========================================================================
// V-83 计划任务工坊
// ===========================================================================

/// 动作白名单（环境注册的动作；枚举见 workshop_dispatch 的前端映射）。
pub const SCHED_ACTIONS: [&str; 7] = [
    "theme.set",
    "wallpaper.set",
    "perf.set",
    "notify.remind",
    "notes.review",
    "palette.run",
    "sound.chime",
];

/// 触发器：时刻（每日 HH:MM）/ 间隔（秒）/ 空闲（秒）/ 登录后（秒）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SchedTrigger {
    /// 每日 HH:MM（24h 制）
    At { hour: u32, minute: u32 },
    /// 每 n 秒（60..86400）
    Interval { secs: u32 },
    /// 空闲 n 秒后（600..86400）
    Idle { secs: u32 },
    /// 登录（环境启动完成）后 n 秒（0..3600）
    Login { secs: u32 },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SchedTask {
    pub id: String,
    pub name: String,
    /// 动作 id（白名单内）
    pub action: String,
    /// 动作参数（如 theme id / 提醒文本）
    pub arg: String,
    pub trigger: SchedTrigger,
    pub enabled: bool,
    /// 上次触发时刻（ms；0 = 从未）
    pub last_fired: u64,
}

fn validate_task(t: &SchedTask) -> CmdResult<()> {
    if !SCHED_ACTIONS.contains(&t.action.as_str()) {
        return Err(AppError::validation(format!("动作不在白名单: {}（白名单外动作不可配置）", t.action)));
    }
    if t.name.trim().is_empty() {
        return Err(AppError::validation("任务名为空"));
    }
    match &t.trigger {
        SchedTrigger::At { hour, minute } => {
            if *hour > 23 || *minute > 59 {
                return Err(AppError::validation(format!("At 触发器时刻越界: {hour}:{minute}")));
            }
        }
        SchedTrigger::Interval { secs } => {
            if !(60..=86_400).contains(secs) {
                return Err(AppError::validation("Interval secs 必须在 60..86400"));
            }
        }
        SchedTrigger::Idle { secs } => {
            if !(600..=86_400).contains(secs) {
                return Err(AppError::validation("Idle secs 必须在 600..86400"));
            }
        }
        SchedTrigger::Login { secs } => {
            if *secs > 3600 {
                return Err(AppError::validation("Login secs 必须在 0..3600"));
            }
        }
    }
    Ok(())
}

fn tasks_path(st: &AppState) -> PathBuf {
    st.data_dir.join("workshop-tasks.json")
}

fn tasks_load(st: &AppState) -> Vec<SchedTask> {
    std::fs::read_to_string(tasks_path(st))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn tasks_save(st: &AppState, list: &[SchedTask]) -> CmdResult<()> {
    std::fs::create_dir_all(&st.data_dir).map_err(|e| AppError::io(e.to_string()))?;
    let json = serde_json::to_string_pretty(list).map_err(|e| AppError::io(e.to_string()))?;
    std::fs::write(tasks_path(st), json).map_err(|e| AppError::io(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn sched_list(st: tauri::State<AppState>) -> CmdResult<Vec<SchedTask>> {
    Ok(tasks_load(&st))
}

#[tauri::command]
pub fn sched_upsert(st: tauri::State<AppState>, task: SchedTask) -> CmdResult<Vec<SchedTask>> {
    let mut t = task;
    validate_task(&t)?;
    let mut list = tasks_load(&st);
    if t.id.trim().is_empty() {
        t.id = format!("wt-{:x}", now_ms());
        list.push(t);
    } else {
        match list.iter_mut().find(|x| x.id == t.id) {
            Some(slot) => *slot = t,
            None => {
                t.id = format!("wt-{:x}", now_ms());
                list.push(t);
            }
        }
    }
    tasks_save(&st, &list)?;
    Ok(list)
}

#[tauri::command]
pub fn sched_remove(st: tauri::State<AppState>, id: String) -> CmdResult<Vec<SchedTask>> {
    let mut list = tasks_load(&st);
    list.retain(|t| t.id != id);
    tasks_save(&st, &list)?;
    Ok(list)
}

#[tauri::command]
pub fn sched_toggle(st: tauri::State<AppState>, id: String, enabled: bool) -> CmdResult<Vec<SchedTask>> {
    let mut list = tasks_load(&st);
    for t in list.iter_mut() {
        if t.id == id {
            t.enabled = enabled;
        }
    }
    tasks_save(&st, &list)?;
    Ok(list)
}

/// 下次触发时间预览（epoch ms；None = 无法计算）。纯函数（有 vitest 镜像）。
pub fn next_fire_ms(t: &SchedTask, now_epoch_ms: u64, last_fired: u64) -> Option<u64> {
    match &t.trigger {
        SchedTrigger::At { hour, minute } => {
            // 当天该时刻已过 → 明天；用本地时区近似（UTC+8 桌面环境的常见口径，
            // 引入 chrono 属过度依赖；如实边界见完成报告）
            let day_ms = 86_400_000u64;
            let tz_off = local_tz_offset_secs(now_epoch_ms);
            let local = now_epoch_ms as i64 + tz_off as i64 * 1000;
            let today_start = local - local.rem_euclid(day_ms as i64);
            let target = today_start + (*hour as i64 * 3_600_000 + *minute as i64 * 60_000);
            let mut next = target;
            if next <= local as i64 {
                next += day_ms as i64;
            }
            Some((next - tz_off as i64 * 1000) as u64)
        }
        SchedTrigger::Interval { secs } => {
            if !t.enabled {
                return None;
            }
            Some(now_epoch_ms + (*secs as u64) * 1000)
        }
        SchedTrigger::Idle { secs: _ } => None, // 空闲触发依赖运行时观察，无法静态预览
        SchedTrigger::Login { secs } => {
            if !t.enabled {
                return None;
            }
            // 登录后 N 秒：以调度器启动时刻近似（前端展示「下次环境启动 + Ns」）
            None
        }
    }
}

/// 本地时区偏移（秒）。Windows 注册表读取太重，用 UTC+8 近似？不行——用
/// 标准库无法直接取。这里保守取 8h（项目主力用户口径），边界如实注明。
fn local_tz_offset_secs(_now: u64) -> i64 {
    8 * 3600
}

/// 本地日期的 HH:MM（返回 (hour, minute)）。
fn local_hhmm(now_ms: u64) -> (u32, u32) {
    let secs = now_ms / 1000 + (local_tz_offset_secs(now_ms) as u64);
    (((secs % 86_400) / 3_600) as u32, ((secs % 3_600) / 60) as u32)
}

/// 触发判定（纯函数，供测试）：给定任务与当前时刻（含本地 HH:MM 与空闲秒），是否应触发。
/// last_* 用于去抖（同一天同一 At 只触发一次）。
pub fn should_fire(t: &SchedTask, now_ms_now: u64, idle_secs: u32, boot_done_ms: u64, last_at_day: u64) -> bool {
    if !t.enabled {
        return false;
    }
    match &t.trigger {
        SchedTrigger::At { hour, minute } => {
            let (h, m) = local_hhmm(now_ms_now);
            let day = now_ms_now / 86_400_000;
            h == *hour && m == *minute && day != last_at_day
        }
        SchedTrigger::Interval { secs } => {
            now_ms_now.saturating_sub(t.last_fired) >= (*secs as u64) * 1000
        }
        SchedTrigger::Idle { secs } => idle_secs >= *secs,
        SchedTrigger::Login { secs } => {
            // boot_done_ms 之后 N 秒一次性触发（last_fired==0 表示本次会话未触发过）
            t.last_fired == 0 && now_ms_now >= boot_done_ms + (*secs as u64) * 1000
        }
    }
}

// ---------- 执行日志（最近 20 次） ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SchedLogEntry {
    pub ts: u64,
    pub task_id: String,
    pub name: String,
    pub action: String,
    pub ok: bool,
    pub detail: String,
}

fn log_path(st: &AppState) -> PathBuf {
    st.data_dir.join("workshop-log.json")
}

fn log_load(st: &AppState) -> Vec<SchedLogEntry> {
    std::fs::read_to_string(log_path(st))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn log_push(st: &AppState, e: SchedLogEntry) {
    let mut list = log_load(st);
    list.push(e);
    let cut = list.len().saturating_sub(20);
    let list: Vec<SchedLogEntry> = list.into_iter().skip(cut).collect();
    let _ = std::fs::write(
        log_path(st),
        serde_json::to_string_pretty(&list).unwrap_or_default(),
    );
}

#[tauri::command]
pub fn sched_log_list(st: tauri::State<AppState>) -> CmdResult<Vec<SchedLogEntry>> {
    let mut list = log_load(&st);
    list.reverse();
    Ok(list)
}

/// 前端手动「立即执行」（白名单同校验；不写触发时刻）。
#[tauri::command]
pub fn sched_run_now(app: tauri::AppHandle, st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    let list = tasks_load(&st);
    let t = list.iter().find(|t| t.id == id).ok_or_else(|| AppError::validation("任务不存在"))?;
    let task = t.clone();
    log_push(&st, SchedLogEntry {
        ts: now_ms(),
        task_id: task.id.clone(),
        name: task.name.clone(),
        action: task.action.clone(),
        ok: true,
        detail: "手动执行".into(),
    });
    // 触发前端执行（workshop://run 事件）
    let _ = app.emit("workshop://run", serde_json::json!({ "action": task.action, "arg": task.arg, "taskId": task.id }));
    Ok(())
}

// ---------- 运行时线程（启动时拉起；每 5s 扫描） ----------

struct RuntimeCtx {
    boot_done_ms: u64,
    last_at_day: u64,
}

static RUNTIME_CTX: Mutex<Option<RuntimeCtx>> = Mutex::new(None);

/// 前端空闲秒（前端调用更新；GetLastInputInfo 在前端可见性受限，用前端采样）。
static IDLE_SECS: Mutex<u32> = Mutex::new(0);

#[tauri::command]
pub fn workshop_idle_report(idle_secs: u32) -> CmdResult<()> {
    if let Ok(mut g) = IDLE_SECS.lock() {
        *g = idle_secs;
    }
    Ok(())
}

/// 启动调度器（lib.rs setup 调用）。boot_done 由首次 tick 基准。
pub fn spawn_workshop_runtime(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let st = app.state::<AppState>();
        let boot_done = now_ms() + 3_000; // 近似：调度器启动后 3s 视为环境就绪
        if let Ok(mut g) = RUNTIME_CTX.lock() {
            *g = Some(RuntimeCtx { boot_done_ms: boot_done, last_at_day: 0 });
        }
        // Login 触发器的 last_fired 跨会话持久 —— 每次启动重置（本次会话未触发）
        {
            let mut list = tasks_load(&st);
            let mut dirty = false;
            for t in list.iter_mut() {
                if matches!(t.trigger, SchedTrigger::Login { .. }) && t.last_fired != 0 {
                    t.last_fired = 0;
                    dirty = true;
                }
            }
            if dirty {
                let _ = tasks_save(&st, &list);
            }
        }
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let now = now_ms();
            let idle = IDLE_SECS.lock().map(|g| *g).unwrap_or(0);
            let last_at_day = RUNTIME_CTX.lock().ok().and_then(|g| g.as_ref().map(|c| c.last_at_day)).unwrap_or(0);
            let mut fired: Vec<SchedTask> = Vec::new();
            let mut at_day = last_at_day;
            {
                let list = tasks_load(&st);
                for t in list {
                    let is_at = matches!(t.trigger, SchedTrigger::At { .. });
                    if should_fire(&t, now, idle, boot_done, last_at_day) {
                        if is_at {
                            at_day = now / 86_400_000;
                        }
                        fired.push(t);
                    }
                }
            }
            if !fired.is_empty() {
                if let Ok(mut g) = RUNTIME_CTX.lock() {
                    if let Some(c) = g.as_mut() {
                        c.last_at_day = at_day;
                    }
                }
                let mut updated = tasks_load(&st);
                for t in &fired {
                    // 派发前端执行（动作全部由前端域执行：主题/壁纸/性能/提醒/速记/面板）
                    let _ = app.emit(
                        "workshop://run",
                        serde_json::json!({ "action": t.action, "arg": t.arg, "taskId": t.id }),
                    );
                    // 事件 → M-57 出站桥（schedule.fired 白名单事件）
                    let _ = crate::shell::opentools::webhook_dispatch(
                        app.state::<AppState>(),
                        "schedule.fired".into(),
                        serde_json::to_string(&serde_json::json!({ "taskId": t.id, "action": t.action, "name": t.name })).unwrap_or_default(),
                    );
                    log_push(&st, SchedLogEntry {
                        ts: now,
                        task_id: t.id.clone(),
                        name: t.name.clone(),
                        action: t.action.clone(),
                        ok: true,
                        detail: "定时触发".into(),
                    });
                    for u in updated.iter_mut() {
                        if u.id == t.id {
                            u.last_fired = now;
                        }
                    }
                }
                let _ = tasks_save(&st, &updated);
            }
        }
    });
}

// ===========================================================================
// V-86 启动项延迟编排
// ===========================================================================

/// 延迟配置：启动项名 → 延迟秒（0..120；0 = 立即）。
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct StartDelayConfig {
    /// 仅 VM 档拉起生效（直跑档延迟仅记录建议值）
    pub vm_only: bool,
    pub delays: Vec<StartDelayEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StartDelayEntry {
    /// 启动项名（HKCU Run 值名）
    pub name: String,
    /// 0..120（0 = 立即）
    pub delay_secs: u32,
}

/// 安全类启动项关键词（杀毒/驱动辅助：不显示延迟选项，如实避嫌）。
pub const SAFE_STARTUP_KEYWORDS: [&str; 10] = [
    "antivirus", "defender", "杀毒", "安全卫士", "360", "huorong", "hips", "firewall",
    "driver", "驱动",
];

/// 纯函数：是否安全类启动项（不提供延迟建议）。
pub fn is_safe_startup(name: &str, cmd: &str) -> bool {
    let hay = format!("{} {}", name.to_lowercase(), cmd.to_lowercase());
    SAFE_STARTUP_KEYWORDS.iter().any(|k| hay.contains(k))
}

fn delay_config_path(st: &AppState) -> PathBuf {
    st.data_dir.join("startdelay.json")
}

fn delay_load(st: &AppState) -> StartDelayConfig {
    std::fs::read_to_string(delay_config_path(st))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn delay_save(st: &AppState, cfg: &StartDelayConfig) -> CmdResult<()> {
    for e in &cfg.delays {
        if e.delay_secs > 120 {
            return Err(AppError::validation(format!("{} 的延迟必须在 0..120s", e.name)));
        }
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| AppError::io(e.to_string()))?;
    std::fs::write(delay_config_path(st), json).map_err(|e| AppError::io(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn startdelay_get(st: tauri::State<AppState>) -> CmdResult<StartDelayConfig> {
    Ok(delay_load(&st))
}

#[tauri::command]
pub fn startdelay_set(st: tauri::State<AppState>, config: StartDelayConfig) -> CmdResult<StartDelayConfig> {
    delay_save(&st, &config)?;
    Ok(config)
}

/// 启动时间轴（最近 200 行；JSONL）。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StartDelayTimelineEntry {
    /// 会话启动（调度器拉起）时刻
    pub boot_ms: u64,
    pub name: String,
    pub delay_secs: u32,
    /// 实际拉起 / 观测到进程的时刻
    pub launched_ms: u64,
    /// 理论对比：无延迟时本应在 boot_ms 拉起
    pub theoretical_ms: u64,
    /// 观测方式："launched"（我们拉起）| "seen"（轮询观测到进程）
    pub via: String,
}

fn timeline_path(st: &AppState) -> PathBuf {
    st.data_dir.join("startdelay-timeline.jsonl")
}

#[tauri::command]
pub fn startdelay_timeline(st: tauri::State<AppState>) -> CmdResult<Vec<StartDelayTimelineEntry>> {
    let raw = std::fs::read_to_string(timeline_path(&st)).unwrap_or_default();
    let mut out: Vec<StartDelayTimelineEntry> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    out.reverse();
    out.truncate(200);
    Ok(out)
}

/// VM 档拉起运行时：boot 后按延迟逐项拉起 HKCU Run 启动项（我们自己写的注册表值）。
/// 直跑档：只观测（进程出现时刻记时间轴），不干预系统启动。
pub fn spawn_startdelay_runtime(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let st = app.state::<AppState>();
        let boot_ms = now_ms();
        let cfg = delay_load(&st);
        let items = crate::shell::taskman::startup_list().unwrap_or_default();
        // 计划：name → delay
        let mut plan: Vec<(String, u32)> = Vec::new();
        for it in &items {
            if it.hive != "HKCU" {
                continue;
            }
            let d = cfg
                .delays
                .iter()
                .find(|e| e.name == it.name)
                .map(|e| e.delay_secs)
                .unwrap_or(0);
            if is_safe_startup(&it.name, &it.cmd) {
                continue; // 安全类不参与延迟
            }
            plan.push((it.name.clone(), d));
        }
        if crate::shell::sysinfo::runtime_mode() == "vm" {
            // VM 档：延迟拉起（delay 0 的交给系统正常启动，不重复拉起）
            for (name, d) in plan {
                if d == 0 {
                    continue;
                }
                let launch_at = boot_ms + (d as u64) * 1000;
                let now = now_ms();
                if launch_at > now {
                    std::thread::sleep(Duration::from_millis(launch_at - now));
                }
                if let Some(item) = items.iter().find(|i| i.name == name) {
                    let launched = now_ms();
                    let _ = run_startdelay_item(&item.cmd);
                    push_timeline(&st, StartDelayTimelineEntry {
                        boot_ms,
                        name: name.clone(),
                        delay_secs: d,
                        launched_ms: launched,
                        theoretical_ms: boot_ms,
                        via: "launched".into(),
                    });
                }
            }
        } else {
            // 直跑档：观测模式 —— 轮询进程名出现时刻（10 分钟窗口），记时间轴
            let watch: Vec<(String, u32)> = plan;
            let mut seen: Vec<String> = Vec::new();
            let deadline = boot_ms + 600_000;
            while now_ms() < deadline {
                std::thread::sleep(Duration::from_millis(1500));
                let names = proc_name_lower_set();
                for (name, d) in &watch {
                    if seen.contains(name) {
                        continue;
                    }
                    let exe = start_item_exe_hint(name, &items);
                    if let Some(exe) = exe {
                        if names.iter().any(|p| p.contains(&exe)) {
                            seen.push(name.clone());
                            push_timeline(&st, StartDelayTimelineEntry {
                                boot_ms,
                                name: name.clone(),
                                delay_secs: *d,
                                launched_ms: now_ms(),
                                theoretical_ms: boot_ms,
                                via: "seen".into(),
                            });
                        }
                    }
                }
                if seen.len() >= watch.len() {
                    break;
                }
            }
        }
    });
}

fn push_timeline(st: &tauri::State<'_, AppState>, e: StartDelayTimelineEntry) {
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(timeline_path(st))
    {
        let _ = writeln!(f, "{}", serde_json::to_string(&e).unwrap_or_default());
    }
}

fn start_item_exe_hint(name: &str, items: &[crate::shell::taskman::StartupItem]) -> Option<String> {
    let it = items.iter().find(|i| i.name == name)?;
    // cmd 形如 "C:\path\app.exe" -arg —— 提取 exe 文件名（去引号取最后一段 \）
    let s = it.cmd.trim().trim_start_matches('"');
    let first = s.split_whitespace().next().unwrap_or("");
    let base = first.rsplit(['\\', '/']).next().unwrap_or(first);
    let base = base.trim_end_matches('"').to_lowercase();
    if base.is_empty() { None } else { Some(base) }
}

fn proc_name_lower_set() -> Vec<String> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
        };
        use windows::Win32::System::Diagnostics::ToolHelp::Process32FirstW;
        use windows::Win32::System::Diagnostics::ToolHelp::Process32NextW;
        let mut out = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).unwrap_or_default();
            if snap.is_invalid() {
                return out;
            }
            let mut pe = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut ok = Process32FirstW(snap, &mut pe).is_ok();
            while ok {
                let name = String::from_utf16_lossy(&pe.szExeFile);
                out.push(name.to_lowercase());
                ok = Process32NextW(snap, &mut pe).is_ok();
            }
            let _ = windows::Win32::Foundation::CloseHandle(snap);
        }
        out
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

fn run_startdelay_item(cmd: &str) -> CmdResult<()> {
    // 解析 "C:\path\app.exe" -args 形态；仅拉起，不等待
    let s = cmd.trim();
    let (exe, args) = if let Some(rest) = s.strip_prefix('"') {
        match rest.split_once('"') {
            Some((e, tail)) => (e.to_string(), tail.trim().to_string()),
            None => (rest.to_string(), String::new()),
        }
    } else {
        match s.split_once(' ') {
            Some((e, tail)) => (e.to_string(), tail.trim().to_string()),
            None => (s.to_string(), String::new()),
        }
    };
    let mut c = std::process::Command::new(&exe);
    if !args.is_empty() {
        c.arg(&args);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    c.spawn().map_err(|e| AppError::io(format!("拉起 {exe} 失败: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(trigger: SchedTrigger) -> SchedTask {
        SchedTask {
            id: "t1".into(),
            name: "测试".into(),
            action: "theme.set".into(),
            arg: "paper".into(),
            trigger,
            enabled: true,
            last_fired: 0,
        }
    }

    #[test]
    fn validate_whitelist() {
        assert!(validate_task(&task(SchedTrigger::At { hour: 22, minute: 0 })).is_ok());
        let mut bad = task(SchedTrigger::At { hour: 22, minute: 0 });
        bad.action = "cmd.exec".into();
        assert!(validate_task(&bad).is_err());
        assert!(validate_task(&task(SchedTrigger::At { hour: 24, minute: 0 })).is_err());
        assert!(validate_task(&task(SchedTrigger::Interval { secs: 30 })).is_err());
        assert!(validate_task(&task(SchedTrigger::Idle { secs: 60 })).is_err());
    }

    #[test]
    fn fire_at_once_per_day() {
        // 10:30 整（本地近似 UTC+8：epoch 0 = 1970-01-01 08:00 → 本地 08:00）
        // 选一个明确时刻：epoch ms 使本地时间 = 10:30
        let base_day = 86_400_000u64 * 19_000;
        let at_1030 = base_day + (2 * 3600 + 30 * 60) * 1000; // 本地 08:00 + 2h30 = 10:30
        let mut t = task(SchedTrigger::At { hour: 10, minute: 30 });
        t.last_fired = 0;
        assert!(should_fire(&t, at_1030, 0, 0, 0));
        // 同一天第二次 tick：不再触发
        assert!(!should_fire(&t, at_1030 + 5_000, 0, 0, at_1030 / 86_400_000));
    }

    #[test]
    fn fire_interval_and_login() {
        let mut t = task(SchedTrigger::Interval { secs: 60 });
        t.last_fired = 1_000_000;
        assert!(should_fire(&t, 1_000_000 + 60_000, 0, 0, 0));
        assert!(!should_fire(&t, 1_000_000 + 30_000, 0, 0, 0));
        let l = task(SchedTrigger::Login { secs: 10 });
        assert!(should_fire(&l, 110_000, 0, 100_000, 0));
        assert!(!should_fire(&l, 105_000, 0, 100_000, 0));
        let mut done = l.clone();
        done.last_fired = 111_000;
        assert!(!should_fire(&done, 120_000, 0, 100_000, 0));
    }

    #[test]
    fn idle_trigger() {
        let t = task(SchedTrigger::Idle { secs: 600 });
        assert!(should_fire(&t, 1, 600, 0, 0));
        assert!(!should_fire(&t, 1, 599, 0, 0));
    }

    #[test]
    fn safe_startup_guard() {
        assert!(is_safe_startup("Huorong 安全", "C:\\hrs\\hips.exe"));
        assert!(is_safe_startup("360Safe", "C:\\360\\safeguard.exe"));
        assert!(!is_safe_startup("Steam", "C:\\steam.exe"));
    }

    #[test]
    fn next_fire_interval_preview() {
        let t = task(SchedTrigger::Interval { secs: 120 });
        assert_eq!(next_fire_ms(&t, 1_000, 0), Some(121_000));
        let mut off = t.clone();
        off.enabled = false;
        assert_eq!(next_fire_ms(&off, 1_000, 0), None);
    }
}
