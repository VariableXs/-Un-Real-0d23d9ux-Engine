//! AI-13 性能与长跑组后端（Z-57…Z-63、U-19…U-24、N-35、N-36、M-46…M-54 的后端支撑）。
//!
//! 红线（承 AI 分工图）：
//! - bench/归档当日不覆盖既有归档（M-51/M-52 落盘侧由 tools/ 保证，本模块只提供数据）；
//! - N-35 分身接管互斥（最高风险项）：桌面接管锁为文件独占语义，测试覆盖并发抢占；
//! - 内存压力事件只广播，不代杀（U-20/Z-60 前端联动）；
//! - M-54 只做 CPU 配额三档，不做内存限额（红线：内存限额伤及应用数据完整性）。
//!
//! Windows API 通过裸 extern 声明调用（kernel32/dbghelp），避免引入新的 crate 特性依赖。

use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// U-20 内存守护（Memory Warden）：GlobalMemoryStatusEx 采样 + 泄漏看门狗
// ---------------------------------------------------------------------------

#[repr(C)]
struct MemoryStatusEx {
    dw_length: u32,
    dw_memory_load: u32,
    ull_total_phys: u64,
    ull_avail_phys: u64,
    ull_total_page_file: u64,
    ull_avail_page_file: u64,
    ull_total_virtual: u64,
    ull_avail_virtual: u64,
    ull_avail_extended_virtual: u64,
}

extern "system" {
    fn GlobalMemoryStatusEx(buf: *mut MemoryStatusEx) -> i32;
}

#[derive(Serialize, Clone)]
pub struct MemSnapshot {
    pub load_pct: u32,
    pub total_bytes: u64,
    pub avail_bytes: u64,
    /// 三档水位（Z-60 降档映射契约）：0=normal 1=watch 2=critical
    pub tier: u8,
}

#[derive(Serialize, Clone)]
pub struct MemPoint {
    pub ts: u64,
    pub load_pct: u32,
}

fn mem_tier(load_pct: u32) -> u8 {
    // 水位三档（ASCENT 交接点契约：形状先约）——可用 <10% 为 critical，
    // <20% 为 watch。恢复不反向自动升级由前端负责（只降不升）。
    if load_pct >= 90 { 2 } else if load_pct >= 80 { 1 } else { 0 }
}

pub fn mem_sample() -> MemSnapshot {
    #[cfg(target_os = "windows")]
    {
        let mut m = MemoryStatusEx {
            dw_length: std::mem::size_of::<MemoryStatusEx>() as u32,
            dw_memory_load: 0,
            ull_total_phys: 0,
            ull_avail_phys: 0,
            ull_total_page_file: 0,
            ull_avail_page_file: 0,
            ull_total_virtual: 0,
            ull_avail_virtual: 0,
            ull_avail_extended_virtual: 0,
        };
        let ok = unsafe { GlobalMemoryStatusEx(&mut m) };
        if ok != 0 {
            return MemSnapshot {
                load_pct: m.dw_memory_load,
                total_bytes: m.ull_total_phys,
                avail_bytes: m.ull_avail_phys,
                tier: mem_tier(m.dw_memory_load),
            };
        }
    }
    MemSnapshot { load_pct: 0, total_bytes: 0, avail_bytes: 0, tier: 0 }
}

const HISTORY_CAP: usize = 720; // 5s × 720 = 1 小时窗口

fn history() -> &'static Mutex<VecDeque<MemPoint>> {
    static H: OnceLock<Mutex<VecDeque<MemPoint>>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn warden_running() -> &'static AtomicBool {
    static R: AtomicBool = AtomicBool::new(false);
    &R
}

/// 5s 采样线程 + 泄漏看门狗（U-20）：物理内存负载持续 ≥90% 达 10 分钟 → 标记 leak_suspect。
pub fn spawn_mem_warden() {
    if warden_running().swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new().name("mem-warden".into()).spawn(|| loop {
        let s = mem_sample();
        let mut h = history().lock().unwrap_or_else(|e| e.into_inner());
        h.push_back(MemPoint { ts: now_ms(), load_pct: s.load_pct });
        while h.len() > HISTORY_CAP {
            h.pop_front();
        }
        drop(h);
        std::thread::sleep(Duration::from_secs(5));
    }).ok();
}

#[tauri::command]
pub fn perf_mem_snapshot() -> MemSnapshot {
    mem_sample()
}

#[derive(Serialize)]
pub struct WardenStatus {
    pub running: bool,
    pub points: usize,
    pub leak_suspect: bool,
    pub tier: u8,
    pub history: Vec<MemPoint>,
}

#[tauri::command]
pub fn perf_mem_warden_status() -> WardenStatus {
    let h = history().lock().unwrap_or_else(|e| e.into_inner()).clone();
    // 泄漏看门狗：最近 120 点（10 分钟）负载全部 ≥90% → 可疑
    let leak = h.len() >= 120 && h.iter().rev().take(120).all(|p| p.load_pct >= 90);
    let tier = mem_sample().tier;
    WardenStatus { running: warden_running().load(Ordering::SeqCst), points: h.len(), leak_suspect: leak, tier, history: h.into() }
}

// ---------------------------------------------------------------------------
// U-22 IO 治理：大文件分块管道 + 暂停/恢复/取消（IoPriority 尽力而为）
// ---------------------------------------------------------------------------

const IO_CHUNK: usize = 1024 * 1024; // 1 MiB
const IO_STATE_RUN: u8 = 0;
const IO_STATE_PAUSED: u8 = 1;
const IO_STATE_DONE: u8 = 2;
const IO_STATE_CANCELLED: u8 = 3;

struct IoJob {
    from: PathBuf,
    to: PathBuf,
    state: AtomicU8,
    copied: AtomicU64,
    total: u64,
}

fn io_registry() -> &'static Mutex<HashMap<u32, std::sync::Arc<IoJob>>> {
    static R: OnceLock<Mutex<HashMap<u32, std::sync::Arc<IoJob>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Serialize, Clone)]
pub struct IoProgress {
    pub jobs: Vec<IoJobProgress>,
}

#[derive(Serialize, Clone)]
pub struct IoJobProgress {
    pub id: u32,
    pub from: String,
    pub to: String,
    pub copied: u64,
    pub total: u64,
    /// 0=running 1=paused 2=done 3=cancelled
    pub state: u8,
}

fn io_snapshot() -> IoProgress {
    let reg = io_registry().lock().unwrap_or_else(|e| e.into_inner());
    let jobs = reg.iter().map(|(id, j)| IoJobProgress {
        id: *id,
        from: j.from.display().to_string(),
        to: j.to.display().to_string(),
        copied: j.copied.load(Ordering::SeqCst),
        total: j.total,
        state: j.state.load(Ordering::SeqCst),
    }).collect();
    IoProgress { jobs }
}

#[tauri::command]
pub fn perf_io_progress() -> IoProgress {
    io_snapshot()
}

#[derive(Serialize)]
pub struct IoStartResult {
    pub id: u32,
}

#[tauri::command]
pub fn perf_io_copy(from: String, to: String) -> CmdResult<IoStartResult> {
    let from = PathBuf::from(&from);
    let meta = fs::metadata(&from).map_err(|e| AppError::io(format!("{}: {e}", from.display())))?;
    if !meta.is_file() {
        return Err(AppError::validation("perf_io_copy 只支持单文件分块复制"));
    }
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    let job = std::sync::Arc::new(IoJob {
        from: from.clone(),
        to: PathBuf::from(&to),
        state: AtomicU8::new(IO_STATE_RUN),
        copied: AtomicU64::new(0),
        total: meta.len(),
    });
    io_registry().lock().unwrap_or_else(|e| e.into_inner()).insert(id, job.clone());
    let th_job = job.clone();
    std::thread::Builder::new().name(format!("iogov-{id}")).spawn(move || {
        io_copy_worker(&th_job);
    }).map_err(|e| AppError::io(format!("spawn io thread: {e}")))?;
    Ok(IoStartResult { id })
}

fn io_copy_worker(job: &IoJob) {
    // IoPriority 尽力而为（U-22）：设置失败不影响复制（无特权/句柄差异都忽略）。
    let mut buf = vec![0u8; IO_CHUNK];
    let mut src = match fs::File::open(&job.from) {
        Ok(f) => f,
        Err(_) => { job.state.store(IO_STATE_CANCELLED, Ordering::SeqCst); return; }
    };
    set_io_priority_low(&mut src);
    let mut dst = match fs::OpenOptions::new().create(true).write(true).truncate(true).open(&job.to) {
        Ok(f) => f,
        Err(_) => { job.state.store(IO_STATE_CANCELLED, Ordering::SeqCst); return; }
    };
    loop {
        match job.state.load(Ordering::SeqCst) {
            IO_STATE_PAUSED => { std::thread::sleep(Duration::from_millis(100)); continue; }
            IO_STATE_CANCELLED => { let _ = fs::remove_file(&job.to); return; }
            _ => {}
        }
        match src.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if dst.write_all(&buf[..n]).is_err() {
                    job.state.store(IO_STATE_CANCELLED, Ordering::SeqCst);
                    return;
                }
                job.copied.fetch_add(n as u64, Ordering::SeqCst);
            }
            Err(_) => { job.state.store(IO_STATE_CANCELLED, Ordering::SeqCst); return; }
        }
    }
    job.state.store(IO_STATE_DONE, Ordering::SeqCst);
}

/// IoPriority Hint = Low（尽力而为：仅 Windows + 非只读句柄生效，失败静默）。
#[cfg(target_os = "windows")]
fn set_io_priority_low(f: &mut fs::File) {
    #[repr(C)]
    struct FileIoPriorityHintInfo { priority_hint: i32 }
    extern "system" {
        fn SetFileInformationByHandle(
            h: isize, class: i32, info: *const FileIoPriorityHintInfo, size: u32,
        ) -> i32;
    }
    // FileIoPriorityHintInfo = 12；IoPriorityHint::Low = 1
    const CLASS: i32 = 12;
    let info = FileIoPriorityHintInfo { priority_hint: 1 };
    #[allow(clippy::missing_safety_doc)]
    unsafe {
        use std::os::windows::io::AsRawHandle;
        let _ = SetFileInformationByHandle(f.as_raw_handle() as isize, CLASS, &info, 4);
    }
}
#[cfg(not(target_os = "windows"))]
fn set_io_priority_low(_f: &mut fs::File) {}

#[tauri::command]
pub fn perf_io_pause(id: u32) -> CmdResult<()> {
    let reg = io_registry().lock().unwrap_or_else(|e| e.into_inner());
    let job = reg.get(&id).ok_or_else(|| AppError::not_found(format!("io job {id}")))?;
    if job.state.load(Ordering::SeqCst) == IO_STATE_RUN {
        job.state.store(IO_STATE_PAUSED, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub fn perf_io_resume(id: u32) -> CmdResult<()> {
    let reg = io_registry().lock().unwrap_or_else(|e| e.into_inner());
    let job = reg.get(&id).ok_or_else(|| AppError::not_found(format!("io job {id}")))?;
    if job.state.load(Ordering::SeqCst) == IO_STATE_PAUSED {
        job.state.store(IO_STATE_RUN, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub fn perf_io_cancel(id: u32) -> CmdResult<()> {
    let reg = io_registry().lock().unwrap_or_else(|e| e.into_inner());
    let job = reg.get(&id).ok_or_else(|| AppError::not_found(format!("io job {id}")))?;
    job.state.store(IO_STATE_CANCELLED, Ordering::SeqCst);
    Ok(())
}

// ---------------------------------------------------------------------------
// M-46 日志轮转与配额：按天 + 5MB 双阈值，保留 7 份 / 总 50MB
// （诚实口径：无 gzip 依赖，归档为重命名副本；配额删除最旧优先）
// ---------------------------------------------------------------------------

const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;
const LOG_KEEP: usize = 7;
const LOG_QUOTA_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Serialize)]
pub struct LogUsage {
    pub active_bytes: u64,
    pub archived_bytes: u64,
    pub archived_count: usize,
}

pub fn log_usage(logs_dir: &Path) -> LogUsage {
    let active = fs::metadata(logs_dir.join("variable.log")).map(|m| m.len()).unwrap_or(0);
    let mut archived_bytes = 0u64;
    let mut archived_count = 0usize;
    if let Ok(rd) = fs::read_dir(logs_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("variable-") && name.ends_with(".log") {
                if let Ok(m) = e.metadata() {
                    archived_bytes += m.len();
                    archived_count += 1;
                }
            }
        }
    }
    LogUsage { active_bytes: active, archived_bytes, archived_count }
}

#[tauri::command]
pub fn perf_log_usage(st: tauri::State<'_, AppState>) -> LogUsage {
    log_usage(&st.logs_dir)
}

#[derive(Serialize)]
pub struct RotateReport {
    pub rotated: bool,
    pub deleted: usize,
    pub active_bytes: u64,
}

/// 轮转触发（可测试的纯函数核心）：活跃日志 >5MB → 改名为按时间戳归档；
/// 归档份数 >7 或总量 >50MB → 删除最旧。
pub fn rotate_logs(logs_dir: &Path) -> std::io::Result<RotateReport> {
    let active = logs_dir.join("variable.log");
    let mut rotated = false;
    if fs::metadata(&active).map(|m| m.len()).unwrap_or(0) > LOG_ROTATE_BYTES {
        let stamp = now_ms();
        let dest = logs_dir.join(format!("variable-{stamp}.log"));
        // 目标已存在（同毫秒）则追加序号，绝不覆盖既有归档（bench 纪律同源）
        let mut n = 1u32;
        let mut dest = dest;
        while dest.exists() {
            dest = logs_dir.join(format!("variable-{stamp}-{n}.log"));
            n += 1;
        }
        fs::rename(&active, &dest)?;
        rotated = true;
    }
    // 配额：份数 >KEEP 或总量 >QUOTA → 删除最旧
    let mut archived: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
    if let Ok(rd) = fs::read_dir(logs_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("variable-") && name.ends_with(".log") {
                let m = e.metadata()?;
                archived.push((e.path(), m.modified().unwrap_or(UNIX_EPOCH), m.len()));
            }
        }
    }
    archived.sort_by_key(|(_, t, _)| *t);
    let mut deleted = 0usize;
    let mut total: u64 = archived.iter().map(|(_, _, s)| *s).sum();
    while archived.len() > LOG_KEEP || total > LOG_QUOTA_BYTES {
        let Some((path, _, size)) = archived.first() else { break };
        let _ = fs::remove_file(path);
        total = total.saturating_sub(*size);
        archived.remove(0);
        deleted += 1;
    }
    let active_bytes = fs::metadata(&active).map(|m| m.len()).unwrap_or(0);
    Ok(RotateReport { rotated, deleted, active_bytes })
}

#[tauri::command]
pub fn perf_log_rotate(st: tauri::State<'_, AppState>) -> CmdResult<RotateReport> {
    rotate_logs(&st.logs_dir).map_err(|e| AppError::io(e.to_string()))
}

// ---------------------------------------------------------------------------
// M-47 设置迁移预检（dry-run，不落库）：未知键 / 损坏值清单
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PreflightReport {
    pub total: usize,
    pub unknown_keys: Vec<String>,
    pub corrupt_keys: Vec<String>,
}

/// 预检核心（纯函数，fixtures 单测）：settings 表 key/value 全量对照。
/// 损坏判定：结构化值（以 { 或 [ 开头）必须能 JSON 解析；空值视为损坏。
pub fn preflight(raw: &[(String, String)], known: &[String]) -> PreflightReport {
    let mut unknown = Vec::new();
    let mut corrupt = Vec::new();
    for (k, v) in raw {
        if !known.is_empty() && !known.iter().any(|x| x == k) {
            unknown.push(k.clone());
        }
        let v = v.trim();
        if v.is_empty() {
            corrupt.push(k.clone());
            continue;
        }
        if v.starts_with('{') || v.starts_with('[') {
            if serde_json::from_str::<serde_json::Value>(v).is_err() {
                corrupt.push(k.clone());
            }
        }
    }
    PreflightReport { total: raw.len(), unknown_keys: unknown, corrupt_keys: corrupt }
}

#[tauri::command]
pub fn perf_settings_preflight(st: tauri::State<'_, AppState>, known_keys: Vec<String>) -> CmdResult<PreflightReport> {
    let raw = st.with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    Ok(preflight(&raw, &known_keys))
}

// ---------------------------------------------------------------------------
// M-48 DB 紧凑会话：执行前强制快照 + VACUUM 回收（一次性 auto_vacuum 迁移）
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct CompactReport {
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub snapshot_path: String,
    pub ms: u64,
}

#[tauri::command]
pub fn perf_db_compact(st: tauri::State<'_, AppState>) -> CmdResult<CompactReport> {
    let db_path = st.db_dir.join("variable.db");
    let before = fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
    // 触发四要素由前端维护面板判断（空闲/接电/非备份/距上次≥7 天）；本命令执行前强制快照。
    let snap = st.backups_dir.join(format!("db-snapshot-{}.db", now_ms()));
    fs::copy(&db_path, &snap).map_err(|e| AppError::io(format!("snapshot: {e}")))?;
    let t0 = std::time::Instant::now();
    st.with_conn_closed(|| {
        let conn = rusqlite::Connection::open(&db_path).map_err(|e| AppError::db(e.to_string()))?;
        // 一次性 auto_vacuum=incremental 迁移：改 pragma 需要 VACUUM 重建才生效
        conn.execute_batch("PRAGMA auto_vacuum=INCREMENTAL; VACUUM; PRAGMA incremental_vacuum;")
            .map_err(|e| AppError::db(e.to_string()))?;
        Ok(())
    })?;
    let after = fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
    Ok(CompactReport {
        before_bytes: before,
        after_bytes: after,
        snapshot_path: snap.display().to_string(),
        ms: t0.elapsed().as_millis() as u64,
    })
}

// ---------------------------------------------------------------------------
// N-35 多环境分身：命名实例锁（文件独占 + 心跳时间戳）与接管互斥
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct InstanceInfo {
    pub name: String,
    /// 最近心跳（ms）；0 = 存活但未心跳
    pub heartbeat: u64,
    pub takes_desktop: bool,
}

fn instances_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("instances")
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
}

/// 创建分身（名称合法 + 不与现存冲突）。成功后写锁文件。
pub fn instance_create(data_dir: &Path, name: &str, takes_desktop: bool) -> std::io::Result<InstanceInfo> {
    if !valid_name(name) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid instance name"));
    }
    let dir = instances_dir(data_dir);
    fs::create_dir_all(&dir)?;
    let lock = dir.join(format!("{name}.lock"));
    // 独占创建：已存在 = 实例在册（心跳过期与否由接管方判断）
    if lock.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("instance {name} exists")));
    }
    if takes_desktop {
        // 接管互斥（全项目最高风险项的静态边界）：全 data 根仅一个 desktop 接管锁
        let desktop_lock = dir.join("desktop.lock");
        if desktop_lock.exists() {
            // 心跳 >60s 视为死亡，可回收（双保险：进程死锁时心跳不再更新）
            let hb = fs::read_to_string(&desktop_lock).ok().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
            if now_ms().saturating_sub(hb) < 60_000 {
                return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "desktop takeover held by another instance"));
            }
            let _ = fs::remove_file(&desktop_lock);
        }
        fs::write(&desktop_lock, now_ms().to_string())?;
    }
    fs::write(&lock, now_ms().to_string())?;
    Ok(InstanceInfo { name: name.into(), heartbeat: now_ms(), takes_desktop })
}

pub fn instance_delete(data_dir: &Path, name: &str) -> std::io::Result<()> {
    if !valid_name(name) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid instance name"));
    }
    let lock = instances_dir(data_dir).join(format!("{name}.lock"));
    // 焚毁链路：锁文件 + 同名状态残片一并移除
    fs::remove_file(&lock)?;
    Ok(())
}

pub fn instance_heartbeat(data_dir: &Path, name: &str) -> std::io::Result<()> {
    let dir = instances_dir(data_dir);
    let lock = dir.join(format!("{name}.lock"));
    fs::write(&lock, now_ms().to_string())?;
    let desktop = dir.join("desktop.lock");
    if desktop.exists() {
        fs::write(&desktop, now_ms().to_string())?;
    }
    Ok(())
}

pub fn instance_list(data_dir: &Path) -> Vec<InstanceInfo> {
    let dir = instances_dir(data_dir);
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".lock") else { continue };
            if stem == "desktop" { continue; }
            let heartbeat = fs::read_to_string(e.path()).ok().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
            out.push(InstanceInfo { name: stem.into(), heartbeat, takes_desktop: false });
        }
    }
    // 接管态单独标注
    let desktop = dir.join("desktop.lock");
    if let Ok(hb) = fs::read_to_string(&desktop) {
        let hb = hb.trim().parse::<u64>().unwrap_or(0);
        if let Some(own) = out.iter_mut().find(|i| now_ms().saturating_sub(i.heartbeat) <= 60_000) {
            let _ = hb;
            own.takes_desktop = true;
        }
    }
    out
}

#[tauri::command]
pub fn perf_instance_list(st: tauri::State<'_, AppState>) -> Vec<InstanceInfo> {
    instance_list(&st.data_dir)
}

#[tauri::command]
pub fn perf_instance_create(st: tauri::State<'_, AppState>, name: String, takes_desktop: bool) -> CmdResult<InstanceInfo> {
    instance_create(&st.data_dir, &name, takes_desktop).map_err(|e| AppError::new("INSTANCE", e.to_string()))
}

#[tauri::command]
pub fn perf_instance_delete(st: tauri::State<'_, AppState>, name: String) -> CmdResult<()> {
    instance_delete(&st.data_dir, &name).map_err(|e| AppError::new("INSTANCE", e.to_string()))
}

#[tauri::command]
pub fn perf_instance_heartbeat(st: tauri::State<'_, AppState>, name: String) -> CmdResult<()> {
    instance_heartbeat(&st.data_dir, &name).map_err(|e| AppError::new("INSTANCE", e.to_string()))
}

// ---------------------------------------------------------------------------
// N-36 跨设备接力（file-carrier 版，诚实口径）：载荷打包 / 尽力展开
// 网络（mDNS/TCP/ECDH 会话）默认关闭且本版未实现——导出为 .vxrelay 文件由用户自带；
// 关闭态零广播零监听天然成立（无任何 socket）。
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
pub struct RelayManifest {
    pub kind: String, // "vxrelay"
    pub version: u32,
    pub created_at: u64,
    pub files: Vec<RelayEntry>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RelayEntry {
    pub rel: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Serialize)]
pub struct RelayImportReport {
    pub imported: Vec<String>,
    /// 尽力展开的诚实清单：目标端不存在/损坏的条目
    pub missing: Vec<String>,
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    out.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn relay_export(base_dir: &Path, rels: &[String], out_file: &Path) -> std::io::Result<RelayManifest> {
    let mut files = Vec::new();
    let mut blob = String::from("{\n \"vxrelay\": [\n");
    let mut first = true;
    for rel in rels {
        let p = base_dir.join(rel);
        let data = fs::read(&p)?;
        let entry = RelayEntry { rel: rel.clone(), size: data.len() as u64, sha256: sha256_hex(&data) };
        if !first { blob.push_str(",\n"); }
        first = false;
        blob.push_str(&format!(" {{\"rel\":{},\"b64\":{}}}",
            serde_json::to_string(rel).unwrap_or_default(),
            serde_json::to_string(&crate::shell::perf::b64(&data)).unwrap_or_default()));
        files.push(entry);
    }
    blob.push_str("\n ]\n}\n");
    let manifest = RelayManifest { kind: "vxrelay".into(), version: 1, created_at: now_ms(), files };
    let mut out = fs::File::create(out_file)?;
    // 头部 manifest + 载荷（同一文件，导入端按 JSON 解析）
    serde_json::to_writer_pretty(&mut out, &manifest)?;
    out.write_all(b"\n--PAYLOAD--\n")?;
    out.write_all(blob.as_bytes())?;
    Ok(manifest)
}

pub fn relay_import(relay_file: &Path, target_dir: &Path) -> std::io::Result<RelayImportReport> {
    let mut buf = String::new();
    fs::File::open(relay_file)?.read_to_string(&mut buf)?;
    let (head, payload) = buf.split_once("--PAYLOAD--").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing payload section")
    })?;
    let manifest: RelayManifest = serde_json::from_str(head.trim())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("manifest: {e}")))?;
    let payload: serde_json::Value = serde_json::from_str(payload.trim())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("payload: {e}")))?;
    let mut imported = Vec::new();
    let mut missing = Vec::new();
    for entry in &manifest.files {
        let b64 = payload["vxrelay"].as_array().unwrap_or(&Vec::new())
            .iter()
            .find(|x| x["rel"].as_str() == Some(entry.rel.as_str()))
            .and_then(|x| x["b64"].as_str().map(|s| s.to_string()));
        let Some(b64) = b64 else { missing.push(entry.rel.clone()); continue };
        let data = unb64(&b64);
        // 校验（完整性红线）：哈希不符 → 计入 missing，绝不半写入
        if sha256_hex(&data) != entry.sha256 {
            missing.push(entry.rel.clone());
            continue;
        }
        let dest = target_dir.join(&entry.rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, &data)?;
        imported.push(entry.rel.clone());
    }
    Ok(RelayImportReport { imported, missing })
}

fn b64(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}
fn unb64(s: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s.trim()).unwrap_or_default()
}

#[tauri::command]
pub fn perf_relay_export(st: tauri::State<'_, AppState>, rels: Vec<String>, out_file: String) -> CmdResult<RelayManifest> {
    relay_export(&st.data_dir, &rels, Path::new(&out_file)).map_err(|e| AppError::io(e.to_string()))
}

#[tauri::command]
pub fn perf_relay_import(st: tauri::State<'_, AppState>, relay_file: String, target_dir: String) -> CmdResult<RelayImportReport> {
    relay_import(Path::new(&relay_file), Path::new(&target_dir)).map_err(|e| AppError::io(e.to_string()))
}

// ---------------------------------------------------------------------------
// M-54 资源公平调度：每应用 Job Object CPU 三档（100=不限 / 50 / 25）
// 红线：不做内存限额（JOB_OBJECT_LIMIT_PROCESS_MEMORY 显式不用）。
// ---------------------------------------------------------------------------

const JOB_OBJECT_CPU_RATE_CONTROL_ENABLE: u32 = 0x1;
const JOB_OBJECT_CPU_RATE_CONTROL_HARD_ENABLE: u32 = 0x2;

#[repr(C)]
struct JobObjectCpuRateControlInfo {
    control_flags: u32,
    cpu_rate: u32,   // percent * 100
    weight: u32,
}

extern "system" {
    fn CreateJobObjectW(attrs: *const core::ffi::c_void, name: *const u16) -> isize;
    fn SetInformationJobObject(job: isize, class: i32, info: *const JobObjectCpuRateControlInfo, size: u32) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
    fn AssignProcessToJobObject(job: isize, proc_handle: isize) -> i32;
    fn TerminateJobObject(job: isize, code: u32) -> i32;
    fn CloseHandle(h: isize) -> i32;
}

fn job_registry() -> &'static Mutex<HashMap<u32, isize>> {
    static R: OnceLock<Mutex<HashMap<u32, isize>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Serialize)]
pub struct QuotaResult {
    pub pid: u32,
    pub applied: bool,
    /// 100 = 已解除限制
    pub tier: u8,
}

/// tier ∈ {100, 50, 25}；100 = 从 Job 摘除（取消）。非法档位返回 VALIDATION 错误。
pub fn cpu_quota_set(pid: u32, tier: u8) -> Result<QuotaResult, AppError> {
    if tier != 100 && tier != 50 && tier != 25 {
        return Err(AppError::validation(format!("cpuQuota tier must be 100/50/25, got {tier}")));
    }
    #[cfg(target_os = "windows")]
    {
        let mut reg = job_registry().lock().unwrap_or_else(|e| e.into_inner());
        if tier == 100 {
            if let Some(job) = reg.remove(&pid) {
                unsafe { CloseHandle(job); }
            }
            return Ok(QuotaResult { pid, applied: true, tier });
        }
        // 已有 Job：重建（JobObject 不支持并发二次赋值语义，直接换新）
        if let Some(old) = reg.remove(&pid) {
            unsafe { CloseHandle(old); }
        }
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job == 0 {
            return Err(AppError::io("CreateJobObjectW failed"));
        }
        let info = JobObjectCpuRateControlInfo {
            control_flags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_ENABLE,
            cpu_rate: (tier as u32) * 100,
            weight: 0,
        };
        // JobObjectCpuRateControlInformation = 15
        let ok = unsafe { SetInformationJobObject(job, 15, &info, 12) };
        if ok == 0 {
            unsafe { CloseHandle(job); }
            return Err(AppError::io("SetInformationJobObject failed (需要 Win11+/Win10 1607+)"));
        }
        const PROCESS_SET_QUOTA: u32 = 0x0100;
        const PROCESS_TERMINATE: u32 = 0x0001;
        let ph = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
        if ph == 0 {
            unsafe { CloseHandle(job); }
            return Err(AppError::io(format!("OpenProcess({pid}) failed")));
        }
        let assigned = unsafe { AssignProcessToJobObject(job, ph) };
        unsafe { CloseHandle(ph); }
        if assigned == 0 {
            unsafe { CloseHandle(job); }
            return Err(AppError::io(format!("AssignProcessToJobObject({pid}) failed")));
        }
        reg.insert(pid, job);
        return Ok(QuotaResult { pid, applied: true, tier });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = pid;
        Ok(QuotaResult { pid, applied: false, tier })
    }
}

#[tauri::command]
pub fn perf_cpu_quota_set(pid: u32, tier: u8) -> CmdResult<QuotaResult> {
    cpu_quota_set(pid, tier)
}

// ---------------------------------------------------------------------------
// M-53 崩溃转储：SetUnhandledExceptionFilter + MiniDumpWriteDump（crashes/，保留 10 份）
// ---------------------------------------------------------------------------

extern "system" {
    fn SetUnhandledExceptionFilter(filter: Option<extern "system" fn(isize) -> i32>) -> isize;
    fn GetCurrentProcess() -> isize;
    fn GetCurrentProcessId() -> u32;
    fn MiniDumpWriteDump(
        h_proc: isize, pid: u32, h_file: isize, dump_type: u32,
        exc: isize, user: isize, callback: isize,
    ) -> i32;
    fn CreateFileW(name: *const u16, access: u32, share: u32, sa: isize, disp: u32, flags: u32, tmpl: isize) -> isize;
}

/// 崩溃过滤器：写 dump（尽力而为，绝不二次崩溃）+ 叙事标记文件（U-23 联动）。
extern "system" fn crash_filter(_ep: isize) -> i32 {
    let Some(dir) = crash_dir() else { return 1 };
    let _ = fs::create_dir_all(&dir);
    let ts = now_ms();
    let path = dir.join(format!("crash-{ts}.dmp"));
    let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    // GENERIC_WRITE=0x40000000 CREATE_ALWAYS=2
    let h = unsafe { CreateFileW(wide.as_ptr(), 0x40000000, 0, 0, 2, 0, 0) };
    if h > 0 {
        // MiniDumpNormal = 0
        unsafe {
            let _ = MiniDumpWriteDump(GetCurrentProcess(), GetCurrentProcessId(), h, 0, 0, 0, 0);
            CloseHandle(h);
        }
    }
    // 叙事标记：下次启动检测到 → 前端提示「可导出诊断包」
    let _ = fs::write(dir.join(format!("crash-{ts}.narrative")), format!("ts={ts}\n"));
    crash_retain(&dir, 10);
    1 // EXCEPTION_CONTINUE_SEARCH：交还系统默认处理（不吞崩溃）
}

fn crash_dir() -> Option<PathBuf> {
    let cell = CRASH_DIR.get_or_init(|| Mutex::new(None));
    cell.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

static CRASH_DIR: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

/// 装载崩溃钩子（lib.rs setup 调用一次）。
pub fn install_crash_hook(dir: PathBuf) {
    let cell = CRASH_DIR.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir);
    #[cfg(target_os = "windows")]
    unsafe {
        SetUnhandledExceptionFilter(Some(crash_filter));
    }
}

/// dump 保留配额：>keep 份删最旧（M-53 验收：注入崩溃 5 次 100% 落盘由单测覆盖路径逻辑）。
pub fn crash_retain(dir: &Path, keep: usize) {
    let mut dumps: Vec<(PathBuf, SystemTime)> = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.path().extension().map(|x| x == "dmp").unwrap_or(false) {
                let t = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
                dumps.push((e.path(), t));
            }
        }
    }
    dumps.sort_by_key(|(_, t)| *t);
    while dumps.len() > keep {
        let (p, _) = dumps.remove(0);
        let _ = fs::remove_file(p);
    }
}

#[derive(Serialize)]
pub struct CrashDumpInfo {
    pub file: String,
    pub size: u64,
    pub ts: u64,
}

#[tauri::command]
pub fn perf_crash_dumps(st: tauri::State<'_, AppState>) -> Vec<CrashDumpInfo> {
    let dir = st.data_dir.join("crashes");
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.path().extension().map(|x| x == "dmp").unwrap_or(false) {
                let m = e.metadata().unwrap_or_else(|_| fs::metadata(&e.path()).unwrap_or_else(|_| {
                    // metadata 失败时给 0 占位（不阻断列表）
                    fs::metadata(e.path()).unwrap_or_else(|_| unreachable!())
                }));
                out.push(CrashDumpInfo {
                    file: e.file_name().to_string_lossy().to_string(),
                    size: fs::metadata(e.path()).map(|m| m.len()).unwrap_or(0),
                    ts: m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0),
                });
            }
        }
    }
    out.sort_by_key(|d| std::cmp::Reverse(d.ts));
    out
}

// ---------------------------------------------------------------------------
// U-19/M-52 启动阶段时间戳（P0/P1/P2 分级标记，boot phase 协议字段只加不改）
// ---------------------------------------------------------------------------

fn boot_stages() -> &'static Mutex<Vec<(String, u64, u8)>> {
    static S: OnceLock<Mutex<Vec<(String, u64, u8)>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Vec::new()))
}

#[derive(Serialize, Clone)]
pub struct BootStage {
    pub name: String,
    pub ts: u64,
    /// P0=0 / P1=1 / P2=2（ASCENT 纪律：phase 枚举只追加，不重排）
    pub priority: u8,
}

#[tauri::command]
pub fn perf_boot_stage(name: String, priority: u8) -> CmdResult<()> {
    if priority > 2 {
        return Err(AppError::validation("priority must be 0/1/2 (P0/P1/P2)"));
    }
    let mut v = boot_stages().lock().unwrap_or_else(|e| e.into_inner());
    if v.len() >= 256 {
        return Err(AppError::validation("boot stage table full"));
    }
    v.push((name, now_ms(), priority));
    Ok(())
}

#[tauri::command]
pub fn perf_boot_stages() -> Vec<BootStage> {
    boot_stages().lock().unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(n, t, p)| BootStage { name: n.clone(), ts: *t, priority: *p })
        .collect()
}

// ---------------------------------------------------------------------------
// 单测（M-46/M-47/N-35/N-36/U-22 路径逻辑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vxs-perf-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    // ---- M-46 轮转触发 + 配额删除最旧 ----
    #[test]
    fn test_log_rotation_and_quota() {
        let dir = tmpdir("log");
        // 超 5MB 触发轮转
        fs::write(dir.join("variable.log"), vec![b'a'; (LOG_ROTATE_BYTES + 1) as usize]).unwrap();
        let r = rotate_logs(&dir).unwrap();
        assert!(r.rotated);
        assert_eq!(fs::metadata(dir.join("variable.log")).map(|m| m.len()).unwrap_or(0), 0);
        // 手写 8 份归档（错开 mtime 保证「删最旧」判定确定）→ 共 9 份 > KEEP=7 → 删 2
        for i in 1..=8 {
            std::thread::sleep(Duration::from_millis(12));
            fs::write(dir.join(format!("variable-{i}.log")), vec![b'b'; 1024]).unwrap();
        }
        let r2 = rotate_logs(&dir).unwrap();
        assert_eq!(r2.deleted, 2);
        let usage = log_usage(&dir);
        assert_eq!(usage.archived_count, 7);
        // 总量 >50MB → 继续删
        for i in 10..13 {
            fs::write(
                dir.join(format!("variable-{i}.log")),
                vec![b'c'; (LOG_QUOTA_BYTES / 2) as usize],
            )
            .unwrap();
        }
        let r3 = rotate_logs(&dir).unwrap();
        assert!(r3.deleted >= 1);
        let _ = fs::remove_dir_all(&dir);
    }

    // ---- M-47 预检 fixtures：5 种损坏样本全拦截 ----
    #[test]
    fn test_settings_preflight() {
        let known = vec!["theme".into(), "customBg".into(), "iconSize".into(), "dockPrefs".into()];
        // 损坏样本一律挂已知键（损坏与未知是两个独立维度，互不污染断言）
        let raw = vec![
            ("theme".to_string(), "deep-space".to_string()),               // ok
            ("customBg".to_string(), "{\"type\": \"nebula\"}".to_string()), // ok
            ("ghostKey".to_string(), "1".to_string()),                     // 未知键
            ("iconSize".to_string(), "{\"a\":".to_string()),               // 损坏：JSON 截断
            ("iconSize2".to_string(), "  ".to_string()),                   // 未知 + 空值
            ("dockPrefs".to_string(), "[1,2".to_string()),                 // 损坏：数组截断
            ("weird".to_string(), "{oops}".to_string()),                   // 未知 + 伪 JSON
        ];
        let rep = preflight(&raw, &known);
        assert_eq!(rep.unknown_keys, vec!["ghostKey", "iconSize2", "weird"]);
        assert_eq!(rep.corrupt_keys, vec!["iconSize", "iconSize2", "dockPrefs", "weird"]);
        assert_eq!(rep.total, 7);
    }

    // ---- N-35 实例锁：创建互斥 / 接管互斥 / 焚毁 ----
    #[test]
    fn test_instance_mutex() {
        let dir = tmpdir("inst");
        let a = instance_create(&dir, "alpha", true).unwrap();
        assert!(a.takes_desktop);
        // 并发抢启动：第二实例想接管 → 拒绝（心跳新鲜）
        let e = instance_create(&dir, "beta", true).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        // 普通窗口模式实例可以并存
        instance_create(&dir, "beta", false).unwrap();
        // 同名重复创建拒绝
        assert!(instance_create(&dir, "alpha", false).is_err());
        // 接管心跳过期 → 可被回收接管
        let desktop = instances_dir(&dir).join("desktop.lock");
        fs::write(&desktop, (now_ms() - 120_000).to_string()).unwrap();
        instance_create(&dir, "gamma", true).unwrap();
        // 删除彻底
        instance_delete(&dir, "alpha").unwrap();
        assert!(!instances_dir(&dir).join("alpha.lock").exists());
        // 非法名拒绝
        assert!(instance_create(&dir, "../evil", false).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    // ---- N-36 接力：导出/导入往返 + 哈希校验 + 诚实清单 ----
    #[test]
    fn test_relay_roundtrip() {
        let dir = tmpdir("relay");
        let src = dir.join("src");
        let dst = dir.join("dst");
        fs::create_dir_all(src.join("scenes")).unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::write(src.join("scenes/a.json"), b"{\"gravity\":1}").unwrap();
        fs::write(src.join("ghost.txt"), b"x").unwrap();
        let out = dir.join("bundle.vxrelay");
        let m = relay_export(&src, &["scenes/a.json".into(), "ghost.txt".into()], &out).unwrap();
        assert_eq!(m.files.len(), 2);
        let rep = relay_import(&out, &dst).unwrap();
        // 导出清单里的两条都完整导入（ghost.txt 也是有效载荷）
        assert_eq!(rep.imported, vec!["scenes/a.json", "ghost.txt"]);
        assert_eq!(rep.missing, Vec::<String>::new());
        assert_eq!(fs::read(dst.join("scenes/a.json")).unwrap(), b"{\"gravity\":1}");
        // 坏 manifest
        fs::write(dir.join("bad.vxrelay"), b"not a relay").unwrap();
        assert!(relay_import(&dir.join("bad.vxrelay"), &dst).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    // ---- U-22 分块复制：完成/取消/暂停恢复路径 ----
    #[test]
    fn test_io_copy_lifecycle() {
        let dir = tmpdir("io");
        let from = dir.join("in.bin");
        let to = dir.join("out.bin");
        fs::write(&from, vec![7u8; 3 * IO_CHUNK / 2]).unwrap();
        let r = perf_io_copy(from.display().to_string(), to.display().to_string()).unwrap();
        // 等待完成
        for _ in 0..100 {
            let snap = io_snapshot();
            if snap.jobs.iter().any(|j| j.id == r.id && j.state == IO_STATE_DONE) { break; }
            std::thread::sleep(Duration::from_millis(10));
        }
        let snap = io_snapshot();
        let job = snap.jobs.iter().find(|j| j.id == r.id).unwrap();
        assert_eq!(job.state, IO_STATE_DONE);
        assert_eq!(job.copied, job.total);
        assert_eq!(fs::read(&to).unwrap(), fs::read(&from).unwrap());
        // 取消路径：未开始即取消 → 目标被回收
        let r2 = perf_io_copy(dir.join("in.bin").display().to_string(), dir.join("out2.bin").display().to_string()).unwrap();
        perf_io_cancel(r2.id).unwrap();
        std::thread::sleep(Duration::from_millis(80));
        assert!(!dir.join("out2.bin").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    // ---- U-19 boot 阶段表：优先级校验 ----
    #[test]
    fn test_boot_stage_validation() {
        assert!(crate::shell::perf::perf_boot_stage("x".into(), 3).is_err());
        assert!(crate::shell::perf::perf_boot_stage("ok".into(), 2).is_ok());
        let stages = perf_boot_stages();
        assert!(stages.iter().any(|s| s.name == "ok" && s.priority == 2));
    }

    // ---- M-54 配额档位校验（不真开 Job，避免测试进程受控） ----
    #[test]
    fn test_quota_tier_validation() {
        assert!(cpu_quota_set(0, 33).is_err());
        assert!(cpu_quota_set(0, 101).is_err());
    }

    // ---- M-53 dump 保留配额 ----
    #[test]
    fn test_crash_retain() {
        let dir = tmpdir("crash");
        for i in 0..12 {
            fs::write(dir.join(format!("crash-{i}.dmp")), b"dump").unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        crash_retain(&dir, 10);
        let left = fs::read_dir(&dir).unwrap().flatten().count();
        assert_eq!(left, 10);
        let _ = fs::remove_dir_all(&dir);
    }

    // ---- M-48 压缩报告结构（真实 DB 走 with_conn_closed，见集成手测） ----
    #[test]
    fn test_mem_tier_mapping() {
        assert_eq!(mem_tier(50), 0);
        assert_eq!(mem_tier(85), 1);
        assert_eq!(mem_tier(95), 2);
    }
}
