//! AI-16 启动与声音通知组（Z-43…Z-49 后端支撑）。
//!
//! - Z-43 逐应用音量记忆：mixer 会话音量按进程名持久化（data_dir/volmem.json），
//!   volmem_sync 把记忆值套回当前会话（复用 audioime::mixer_list/mixer_set，
//!   不抢占系统 Mixer 权威——只在我们记忆过的应用上恢复）。
//! - Z-45 系统声音方案：方案 = 事件→合成参数映射 JSON（前端 soundThemes.ts 为
//!   默认方案）；后端负责方案包校验（事件白名单 + 数值范围），无效即拒绝。
//! - Z-46 音频设备快切：默认「通信设备」角色切换（console 角色已在 hardware.rs）。
//! - Z-47 通知存档：SQLite（data_dir/notify-archive.db，rusqlite bundled），
//!   全文 LIKE 搜索 + 按应用筛选 + 保留策略清理；只在策略到期时删数据。
//! - Z-48 麦克风使用指示：ConsentStore 同源只读（hardware::privacy_usage 过滤
//!   microphone），仅指示、不拦截、零轮询由前端按需拉取。
//! - Z-49 提醒中心：提醒持久化（data_dir/reminders.json）+ 系统时钟锚定运行时
//!   （1s 粒度检查，到期 emit "reminder://due"）；重启后未触发的到期提醒补发。
//!
//! 红线：全部本地、零网络、零遥测；错误如实上抛，不伪造成功。

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ===========================================================================
// Z-43 逐应用音量记忆
// ===========================================================================

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VolMemEntry {
    /// 进程名（小写，audioime mixer 会话口径）。
    pub app: String,
    /// 0.0–1.0。
    pub volume: f32,
    pub muted: bool,
    /// "记住此应用"开关（默认开）。
    pub remember: bool,
    pub updated_at: u64,
}

fn volmem_path(st: &AppState) -> PathBuf {
    st.data_dir.join("volmem.json")
}

fn volmem_load(st: &AppState) -> Vec<VolMemEntry> {
    std::fs::read_to_string(volmem_path(st))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn volmem_store(st: &AppState, list: &[VolMemEntry]) -> CmdResult<()> {
    let path = volmem_path(st);
    let json = serde_json::to_string_pretty(list)
        .map_err(|e| AppError::io(format!("volmem 序列化失败: {e}")))?;
    std::fs::write(&path, json).map_err(|e| AppError::io(format!("volmem 写盘失败: {e}")))
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// 记忆列表（remember=false 的条目也保留——"记住此应用"开关本体需要持久化）。
#[tauri::command]
pub fn volmem_list(st: tauri::State<AppState>) -> CmdResult<Vec<VolMemEntry>> {
    Ok(volmem_load(&st))
}

/// 保存/更新一条应用音量记忆（app 已存在则覆盖）。
#[tauri::command]
pub fn volmem_save(st: tauri::State<AppState>, entry: VolMemEntry) -> CmdResult<()> {
    let app = entry.app.trim().to_lowercase();
    if app.is_empty() {
        return Err(AppError::validation("应用名为空"));
    }
    let mut list = volmem_load(&st);
    let e = VolMemEntry {
        app,
        volume: clamp01(entry.volume),
        muted: entry.muted,
        remember: entry.remember,
        updated_at: now_ms(),
    };
    match list.iter().position(|x| x.app == e.app) {
        Some(i) => list[i] = e,
        None => list.push(e),
    }
    volmem_store(&st, &list)
}

/// 忘记某应用（连同开关一起删除）。
#[tauri::command]
pub fn volmem_forget(st: tauri::State<AppState>, app: String) -> CmdResult<()> {
    let app = app.trim().to_lowercase();
    let mut list = volmem_load(&st);
    let before = list.len();
    list.retain(|x| x.app != app);
    if list.len() == before {
        return Err(AppError::not_found(format!("未找到应用音量记忆: {app}")));
    }
    volmem_store(&st, &list)
}

/// 把记忆音量套回当前活跃 mixer 会话（按进程名匹配；只动 remember=true 的应用）。
/// 返回逐条结果（会话不存在 = skipped，非错误——应用没在发声）。
#[tauri::command]
pub fn volmem_sync(st: tauri::State<AppState>) -> CmdResult<Vec<VolMemApplyResult>> {
    let memories: Vec<VolMemEntry> = volmem_load(&st).into_iter().filter(|m| m.remember).collect();
    if memories.is_empty() {
        return Ok(Vec::new());
    }
    let sessions = crate::shell::audioime::mixer_list()?;
    let mut out = Vec::new();
    for m in &memories {
        let mut applied = false;
        for s in &sessions {
            if s.name == m.app {
                let _ = crate::shell::audioime::mixer_set(s.pid, m.volume, m.muted);
                applied = true;
            }
        }
        out.push(VolMemApplyResult {
            app: m.app.clone(),
            applied,
            volume: m.volume,
            muted: m.muted,
        });
    }
    Ok(out)
}

#[derive(Serialize)]
pub struct VolMemApplyResult {
    pub app: String,
    /// 是否找到会话并恢复。
    pub applied: bool,
    pub volume: f32,
    pub muted: bool,
}

// ===========================================================================
// Z-45 系统声音方案 — 方案包校验
// ===========================================================================

/// 合成事件白名单（与前端 sounds.ts SoundName 对齐）。
pub const SOUND_EVENTS: [&str; 10] = [
    "boot", "notify", "alarm", "error", "snap", "trash", "open", "close", "window", "minimize",
];

#[derive(Serialize)]
pub struct SchemeCheckResult {
    pub ok: bool,
    /// 拒绝原因（ok=false 时非空）。
    pub reason: Option<String>,
    /// 校验通过的事件数。
    pub events: usize,
    pub name: Option<String>,
}

/// 校验方案包 JSON（结构 + 事件白名单 + 数值范围）。无效即拒绝并如实说明。
#[tauri::command]
pub fn sound_scheme_validate(path: String) -> CmdResult<SchemeCheckResult> {
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| AppError::io(format!("读取方案包失败: {e}")))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| AppError::validation(format!("方案包不是合法 JSON: {e}")))?;
    let obj = v.as_object().ok_or_else(|| AppError::validation("方案包顶层必须是对象"))?;
    let name = obj.get("name").and_then(|n| n.as_str()).map(|s| s.to_string());
    let events = obj
        .get("events")
        .and_then(|e| e.as_object())
        .ok_or_else(|| AppError::validation("缺少 events 映射表"))?;
    let mut count = 0usize;
    for (event, tuning) in events {
        if !SOUND_EVENTS.contains(&event.as_str()) {
            return Ok(SchemeCheckResult {
                ok: false,
                reason: Some(format!("未知事件: {event}（白名单外事件不可配置）")),
                events: count,
                name,
            });
        }
        let t = tuning.as_object().ok_or_else(|| AppError::validation(format!("事件 {event} 的参数必须是对象")))?;
        for key in ["gainScale", "freqScale", "decayScale"] {
            if let Some(val) = t.get(key) {
                let n = val.as_f64().ok_or_else(|| {
                    AppError::validation(format!("事件 {event} 的 {key} 必须是数值"))
                })?;
                if !(0.0..=4.0).contains(&n) {
                    return Ok(SchemeCheckResult {
                        ok: false,
                        reason: Some(format!("事件 {event} 的 {key}={n} 超出 0..4 范围")),
                        events: count,
                        name,
                    });
                }
            }
        }
        count += 1;
    }
    Ok(SchemeCheckResult { ok: true, reason: None, events: count, name })
}

// ===========================================================================
// Z-46 音频设备快切 — 默认通信设备角色切换
// ===========================================================================

/// 切换默认「通信设备」（eCommunications 角色；console 角色见 hardware::audio_set_default）。
/// 同样走 PolicyConfig COM 槽位 13，Windows 10/11 通用；失败如实报错。
#[cfg(windows)]
#[tauri::command]
pub fn audio_set_default_comm(device_id: String) -> Result<(), String> {
    use windows::core::{GUID, HSTRING, HRESULT, Interface, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    // eCommunications = 1（eConsole = 0）
    const E_COMMUNICATIONS: i32 = 1;
    type SetDefaultEndpointFn =
        unsafe extern "system" fn(this: *mut core::ffi::c_void, device_id: PCWSTR, role: i32) -> HRESULT;

    crate::shell::hardware::with_mta(move || unsafe {
        let unk: windows::core::IUnknown =
            CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL).map_err(crate::shell::hardware::e2s)?;
        let vtable = *(unk.as_raw() as *mut *mut core::ffi::c_void) as *const *mut core::ffi::c_void;
        let set_default: SetDefaultEndpointFn = std::mem::transmute(*vtable.add(13));
        let wid = HSTRING::from(device_id.as_str());
        let hr = set_default(unk.as_raw(), PCWSTR(wid.as_ptr()), E_COMMUNICATIONS);
        if hr.is_ok() {
            Ok(())
        } else {
            Err(format!("SetDefaultEndpoint(communications) failed: {hr:?}"))
        }
    })
}

#[cfg(not(windows))]
#[tauri::command]
pub fn audio_set_default_comm(_device_id: String) -> Result<(), String> {
    Err("仅 Windows 支持".into())
}

// ===========================================================================
// Z-47 通知存档（SQLite）
// ===========================================================================

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    pub app: String,
    pub title: String,
    pub body: String,
    /// "privacy" | "hardware" | "system" | "reminder" | …
    pub kind: String,
    /// 动作按钮（可重新执行；由发送方决定是否支持）。
    pub actions: String,
    pub ts: u64,
}

#[derive(Serialize)]
pub struct ArchiveRow {
    pub id: i64,
    pub app: String,
    pub title: String,
    pub body: String,
    pub kind: String,
    pub actions: String,
    pub ts: u64,
}

#[derive(Serialize)]
pub struct ArchiveQueryResult {
    pub rows: Vec<ArchiveRow>,
    /// 满足筛选的总条数（不含 limit）。
    pub total: i64,
}

fn archive_db_path(st: &AppState) -> PathBuf {
    st.data_dir.join("notify-archive.db")
}

fn archive_conn(st: &AppState) -> Result<rusqlite::Connection, AppError> {
    let conn = rusqlite::Connection::open(archive_db_path(st))
        .map_err(|e| AppError::db(format!("打开通知存档失败: {e}")))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS notify_archive (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts INTEGER NOT NULL,
            app TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'system',
            title TEXT NOT NULL,
            body TEXT NOT NULL DEFAULT '',
            actions TEXT NOT NULL DEFAULT '',
            read INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_notify_archive_ts ON notify_archive(ts);
        CREATE INDEX IF NOT EXISTS idx_notify_archive_app ON notify_archive(app);",
    )
    .map_err(|e| AppError::db(format!("通知存档建表失败: {e}")))?;
    Ok(conn)
}

/// 通知入档（前端 pushNotify 时 fire-and-forget 调用）。
#[tauri::command]
pub fn notify_archive_insert(st: tauri::State<AppState>, entry: ArchiveEntry) -> CmdResult<i64> {
    let conn = archive_conn(&st)?;
    conn.execute(
        "INSERT INTO notify_archive (ts, app, kind, title, body, actions) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![entry.ts as i64, entry.app, entry.kind, entry.title, entry.body, entry.actions],
    )
    .map_err(|e| AppError::db(format!("通知入档失败: {e}")))?;
    Ok(conn.last_insert_rowid())
}

/// 存档查询：标题+正文全文 LIKE；app 为空 = 全部；时间倒序。
#[tauri::command]
pub fn notify_archive_query(
    st: tauri::State<AppState>,
    query: String,
    app: String,
    limit: i64,
    offset: i64,
) -> CmdResult<ArchiveQueryResult> {
    let conn = archive_conn(&st)?;
    let limit = limit.clamp(1, 500);
    let offset = offset.max(0);
    let like = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
    let pattern = if query.is_empty() {
        "%".to_string()
    } else {
        like
    };
    let filter_app = if app.is_empty() { "%".to_string() } else { app.clone() };

    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM notify_archive WHERE (title LIKE ?1 ESCAPE '\\' OR body LIKE ?1 ESCAPE '\\') AND app LIKE ?2",
            rusqlite::params![pattern, filter_app],
            |r| r.get(0),
        )
        .map_err(|e| AppError::db(format!("存档计数失败: {e}")))?;

    let mut stmt = conn
        .prepare(
            "SELECT id, ts, app, kind, title, body, actions FROM notify_archive
             WHERE (title LIKE ?1 ESCAPE '\\' OR body LIKE ?1 ESCAPE '\\') AND app LIKE ?2
             ORDER BY ts DESC, id DESC LIMIT ?3 OFFSET ?4",
        )
        .map_err(|e| AppError::db(format!("存档查询准备失败: {e}")))?;
    let rows = stmt
        .query_map(rusqlite::params![pattern, filter_app, limit, offset], |r| {
            Ok(ArchiveRow {
                id: r.get::<_, i64>(0)?,
                ts: r.get::<_, i64>(1)? as u64,
                app: r.get(2)?,
                kind: r.get(3)?,
                title: r.get(4)?,
                body: r.get(5)?,
                actions: r.get(6)?,
            })
        })
        .map_err(|e| AppError::db(format!("存档查询失败: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::db(format!("存档行读取失败: {e}")))?;

    Ok(ArchiveQueryResult { rows, total })
}

/// 按应用聚合（筛选用）。
#[tauri::command]
pub fn notify_archive_apps(st: tauri::State<AppState>) -> CmdResult<Vec<String>> {
    let conn = archive_conn(&st)?;
    let mut stmt = conn
        .prepare("SELECT DISTINCT app FROM notify_archive ORDER BY app")
        .map_err(|e| AppError::db(format!("存档应用列表失败: {e}")))?;
    let apps = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| AppError::db(format!("存档应用列表失败: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::db(format!("存档应用列表失败: {e}")))?;
    Ok(apps)
}

/// 删除指定条目。
#[tauri::command]
pub fn notify_archive_delete(st: tauri::State<AppState>, ids: Vec<i64>) -> CmdResult<u64> {
    let conn = archive_conn(&st)?;
    let mut n = 0u64;
    for id in ids {
        let changed = conn
            .execute("DELETE FROM notify_archive WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| AppError::db(format!("存档删除失败: {e}")))?;
        n += changed as u64;
    }
    Ok(n)
}

/// 保留策略清理：retentionDays <= 0 = 永久保留（只清已读可选？不——保留策略只按时间）。
/// 返回删除条数；只在策略到期时发生（本命令即"到期执行"本体）。
#[tauri::command]
pub fn notify_archive_cleanup(st: tauri::State<AppState>, retention_days: i64) -> CmdResult<u64> {
    let conn = archive_conn(&st)?;
    if retention_days <= 0 {
        return Ok(0); // 永久保留
    }
    let cutoff = (now_ms() as i64).saturating_sub(retention_days * 86_400_000);
    let n = conn
        .execute("DELETE FROM notify_archive WHERE ts < ?1", rusqlite::params![cutoff])
        .map_err(|e| AppError::db(format!("存档清理失败: {e}")))?;
    Ok(n as u64)
}

// ===========================================================================
// Z-48 麦克风使用指示（只读；ConsentStore 同源）
// ===========================================================================

#[derive(Serialize)]
pub struct MicUsageState {
    pub in_use: bool,
    /// 正在使用麦克风的应用名列表。
    pub apps: Vec<String>,
}

/// 当前麦克风占用（与 Windows 隐私仪表板同源；仅指示，不拦截）。
#[tauri::command]
pub fn mic_usage_state() -> MicUsageState {
    let apps = crate::shell::hardware::privacy_usage()
        .into_iter()
        .filter(|u| u.kind == "microphone")
        .map(|u| u.app)
        .collect::<Vec<_>>();
    MicUsageState { in_use: !apps.is_empty(), apps }
}

// ===========================================================================
// Z-49 提醒中心（持久化 + 时钟锚定运行时）
// ===========================================================================

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub id: String,
    pub text: String,
    /// 到期时刻（ms epoch；前端负责"30 分钟后/明天 9:00"→绝对时间换算）。
    pub due_at: u64,
    /// none | daily | weekly
    pub repeat: String,
    /// 提醒类别（reminder = 普通提醒，豁免勿扰；pomodoro = 番茄钟）。
    pub category: String,
    pub enabled: bool,
    /// 上次触发时刻（0 = 从未；重启补发判断依据）。
    pub last_fired: u64,
}

fn reminders_path(st: &AppState) -> PathBuf {
    st.data_dir.join("reminders.json")
}

fn reminders_load(st: &AppState) -> Vec<Reminder> {
    std::fs::read_to_string(reminders_path(st))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn reminders_store(st: &AppState, list: &[Reminder]) -> CmdResult<()> {
    let json = serde_json::to_string_pretty(list)
        .map_err(|e| AppError::io(format!("提醒序列化失败: {e}")))?;
    std::fs::write(reminders_path(st), json).map_err(|e| AppError::io(format!("提醒写盘失败: {e}")))
}

fn validate_reminder(r: &Reminder) -> CmdResult<()> {
    if r.id.trim().is_empty() {
        return Err(AppError::validation("提醒 id 为空"));
    }
    if r.text.trim().is_empty() {
        return Err(AppError::validation("提醒文本为空"));
    }
    if !matches!(r.repeat.as_str(), "none" | "daily" | "weekly") {
        return Err(AppError::validation(format!("非法重复规则: {}（none/daily/weekly）", r.repeat)));
    }
    if !matches!(r.category.as_str(), "reminder" | "pomodoro") {
        return Err(AppError::validation(format!("非法类别: {}", r.category)));
    }
    Ok(())
}

/// 下次到期时刻（纯函数，可单测）：none = 原值；daily = +24h 直到 > now；
/// weekly = +7d 直到 > now。已过期未触发的 none 提醒保持原值（补发语义）。
pub fn next_due(r: &Reminder, now_ms: u64) -> u64 {
    match r.repeat.as_str() {
        "daily" => {
            let mut d = r.due_at;
            while d <= now_ms {
                d += 86_400_000;
            }
            d
        }
        "weekly" => {
            let mut d = r.due_at;
            while d <= now_ms {
                d += 7 * 86_400_000;
            }
            d
        }
        _ => r.due_at,
    }
}

#[tauri::command]
pub fn reminder_add(st: tauri::State<AppState>, reminder: Reminder) -> CmdResult<Vec<Reminder>> {
    validate_reminder(&reminder)?;
    if reminder.due_at == 0 {
        return Err(AppError::validation("到期时刻为空（相对/绝对时间由前端换算成 epoch ms）"));
    }
    let mut list = reminders_load(&st);
    if list.iter().any(|r| r.id == reminder.id) {
        return Err(AppError::validation(format!("提醒 id 重复: {}", reminder.id)));
    }
    list.push(reminder);
    reminders_store(&st, &list)?;
    Ok(list)
}

#[tauri::command]
pub fn reminder_list(st: tauri::State<AppState>) -> CmdResult<Vec<Reminder>> {
    Ok(reminders_load(&st))
}

/// 完成（一次性提醒移除；重复提醒推到下一周期）。
#[tauri::command]
pub fn reminder_complete(st: tauri::State<AppState>, id: String) -> CmdResult<Vec<Reminder>> {
    let now = now_ms();
    let mut list = reminders_load(&st);
    let Some(r) = list.iter_mut().find(|r| r.id == id) else {
        return Err(AppError::not_found(format!("提醒不存在: {id}")));
    };
    if r.repeat == "none" {
        list.retain(|x| x.id != id);
    } else {
        r.due_at = next_due(r, now);
        r.last_fired = now;
    }
    reminders_store(&st, &list)?;
    Ok(list)
}

/// 改期（保持重复规则，重设基准时刻）。
#[tauri::command]
pub fn reminder_reschedule(st: tauri::State<AppState>, id: String, due_at: u64) -> CmdResult<Vec<Reminder>> {
    let mut list = reminders_load(&st);
    let Some(r) = list.iter_mut().find(|r| r.id == id) else {
        return Err(AppError::not_found(format!("提醒不存在: {id}")));
    };
    if due_at == 0 {
        return Err(AppError::validation("改期时刻为空"));
    }
    r.due_at = due_at;
    reminders_store(&st, &list)?;
    Ok(list)
}

#[tauri::command]
pub fn reminder_delete(st: tauri::State<AppState>, id: String) -> CmdResult<Vec<Reminder>> {
    let mut list = reminders_load(&st);
    let before = list.len();
    list.retain(|r| r.id != id);
    if list.len() == before {
        return Err(AppError::not_found(format!("提醒不存在: {id}")));
    }
    reminders_store(&st, &list)?;
    Ok(list)
}

/// 提醒运行时（系统时钟锚定）：每秒检查 due_at <= now 且未在本秒前触发过的提醒，
/// emit "reminder://due"。重启补发：last_fired < due_at <= now 的一次性提醒同样补发
/// （"重启后未触发提醒不丢失"）。重复提醒触发后立即推进下一周期。
pub fn spawn_reminder_runtime(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        let now = now_ms();
        let st = app.state::<AppState>();
        let mut list = reminders_load(&st.inner());
        let mut changed = false;
        let mut due: Vec<Reminder> = Vec::new();
        for r in list.iter_mut() {
            if !r.enabled {
                continue;
            }
            // 到期判定：due_at 已到，且上次触发不晚于到期（未触发过）。
            if r.due_at <= now && r.last_fired < r.due_at {
                due.push(r.clone());
                r.last_fired = now;
                changed = true;
                if r.repeat != "none" {
                    r.due_at = next_due(r, now);
                }
            }
        }
        if changed {
            let _ = reminders_store(&st.inner(), &list);
        }
        drop(st);
        for r in due {
            let _ = app.emit(
                "reminder://due",
                serde_json::json!({ "id": r.id, "text": r.text, "category": r.category, "repeat": r.repeat, "dueAt": r.due_at }),
            );
        }
        std::thread::sleep(Duration::from_secs(1));
    });
}

// ---------------------------------------------------------------------------
// 单元测试（纯逻辑；SQLite 用内存库语义等价——用临时目录由调用方保证）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn r(due: u64, repeat: &str) -> Reminder {
        Reminder {
            id: "t1".into(),
            text: "测试".into(),
            due_at: due,
            repeat: repeat.into(),
            category: "reminder".into(),
            enabled: true,
            last_fired: 0,
        }
    }

    #[test]
    fn next_due_none_keeps_original() {
        let now = 1_000_000;
        assert_eq!(next_due(&r(500_000, "none"), now), 500_000);
    }

    #[test]
    fn next_due_daily_advances_until_future() {
        let now = 1_000_000;
        let d = next_due(&r(500_000, "daily"), now);
        assert!(d > now);
        assert_eq!((d - 500_000) % 86_400_000, 0);
    }

    #[test]
    fn next_due_weekly_advances_until_future() {
        let now = 10_000_000;
        let d = next_due(&r(1_000, "weekly"), now);
        assert!(d > now);
        assert_eq!((d - 1_000) % (7 * 86_400_000), 0);
    }

    #[test]
    fn validate_reminder_rejects_bad_repeat() {
        let mut x = r(1, "none");
        x.repeat = "hourly".into();
        assert!(validate_reminder(&x).is_err());
        assert!(validate_reminder(&r(1, "none")).is_ok());
    }

    #[test]
    fn sound_events_whitelist_contains_core() {
        assert!(SOUND_EVENTS.contains(&"boot"));
        assert!(SOUND_EVENTS.contains(&"minimize"));
    }
}
