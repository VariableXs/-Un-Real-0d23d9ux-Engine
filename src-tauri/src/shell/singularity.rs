//! SINGULARITY-100（奇点计划）· Rust 支撑命令。
//!
//! 六组命令（全部本地、零网络）：
//! - `singu_pulse`        硬件脉搏（CPU/内存/温度/电池/磁盘/网络速率；GPU 在
//!                        Windows 无可靠通用读数 → None，前端如实降级为 CPU 驱动并注明）；
//! - `singu_temp_scan`    环境临时文件账本（临时目录/缓存/日志的体积明细）；
//! - `singu_zone_check`   读取 NTFS Zone.Identifier（下载来源检疫）；
//! - `singu_journal_log/list/clear` 敏感目录访问日志（JSONL 环形 500 条，可焚毁）；
//! - `singu_batch_attrs`  批量文件属性（只读/隐藏/归档/时间平移）；
//! - `singu_archive_check` 压缩包逐条目 CRC 体检。
//!
//! 纪律：任何不可读的平台数据一律返回 None/空并如实标注（绝不编造数值）。

use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{Networks, System, Disks};

#[derive(Serialize, Clone)]
pub struct SinguDisk {
    pub name: String,
    pub mount: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Serialize, Clone)]
pub struct SinguPulse {
    /// 全局 CPU 占用 0..100。
    pub cpu_usage: f32,
    pub mem_used_mb: f64,
    pub mem_total_mb: f64,
    /// GPU 占用 0..100；Windows 无免驱动通用读数 → None（前端降级注明）。
    pub gpu_usage: Option<f32>,
    /// CPU 温度（℃）；sysinfo 组件不可读时给负载估算值。
    pub cpu_temp_c: Option<f32>,
    pub temp_estimated: bool,
    /// 风扇高速（由温度/CPU 阈值估算，hub 描述注明「估算」）。
    pub fan_high: bool,
    /// 电池电量 0..100；台式机 None。
    pub battery_percent: Option<u8>,
    /// 是否接通交流电源。
    pub battery_ac: bool,
    pub disks: Vec<SinguDisk>,
    /// 自上次 pulse 的上下行速率（字节/秒）。
    pub net_up_bps: u64,
    pub net_down_bps: u64,
    /// 采样时刻（unix ms）。
    pub ts_ms: u64,
}

struct PulseState {
    sys: System,
    nets: Networks,
    disks: Disks,
    last_up: u64,
    last_down: u64,
    last_ts: u64,
}

static PULSE: Mutex<Option<PulseState>> = Mutex::new(None);

#[cfg(windows)]
fn battery_status() -> (Option<u8>, bool) {
    use windows::Win32::System::Power::GetSystemPowerStatus;
    use windows::Win32::System::Power::SYSTEM_POWER_STATUS;
    unsafe {
        let mut st = SYSTEM_POWER_STATUS::default();
        if GetSystemPowerStatus(&mut st).is_ok() {
            let ac = st.ACLineStatus == 1;
            // BatteryLifePercent 0..100；255 = 未知
            let pct = if st.BatteryLifePercent != 255 && st.BatteryFlag != 128 {
                Some(st.BatteryLifePercent)
            } else {
                None
            };
            (pct, ac)
        } else {
            (None, false)
        }
    }
}

#[cfg(not(windows))]
fn battery_status() -> (Option<u8>, bool) {
    (None, false)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[tauri::command(async)]
pub fn singu_pulse() -> Result<SinguPulse, String> {
    let mut guard = PULSE.lock().map_err(|e| e.to_string())?;
    let now = now_ms();
    let st = guard.get_or_insert_with(|| PulseState {
        sys: System::new(),
        nets: Networks::new(),
        disks: Disks::new(),
        last_up: 0,
        last_down: 0,
        last_ts: now_ms(),
    });
    st.sys.refresh_cpu_usage();
    st.sys.refresh_memory();
    st.nets.refresh();
    st.disks.refresh_list();

    // CPU：首次调用无 delta（refresh 间隔不足）→ 0，前端按诚实 0 处理
    let cpu = st.sys.global_cpu_info().cpu_usage();
    let mem_total = st.sys.total_memory() as f64 / 1024.0;
    let mem_used = st.sys.used_memory() as f64 / 1024.0;

    // 温度：优先 sysinfo 组件（Windows 常不可读 → 负载估算并标注）
    let mut temp: Option<f32> = None;
    let mut estimated = false;
    for comp in &sysinfo::Components::new_with_refreshed_list() {
        let t = comp.temperature();
        if t > 0.0 && t < 120.0 {
            temp = Some(t);
            break;
        }
    }
    if temp.is_none() {
        estimated = true;
        temp = Some(35.0 + cpu * 0.45);
    }
    let temp_v = temp.unwrap_or(0.0);
    let fan_high = temp_v >= 75.0 || cpu >= 90.0;

    // 磁盘
    let disks: Vec<SinguDisk> = st
        .disks
        .list()
        .iter()
        .map(|d| SinguDisk {
            name: d.name().to_string_lossy().to_string(),
            mount: d.mount_point().to_string_lossy().to_string(),
            total_bytes: d.total_space(),
            available_bytes: d.available_space(),
        })
        .collect();

    // 网络速率（自上次 pulse 的差分；首拍 0）
    let mut up: u64 = 0;
    let mut down: u64 = 0;
    for (_, data) in st.nets.iter() {
        up = up.saturating_add(data.transmitted());
        down = down.saturating_add(data.received());
    }
    let dt = now.saturating_sub(st.last_ts).max(1);
    let net_up = if st.last_ts == 0 { 0 } else { up.saturating_sub(st.last_up) as u64 * 1000 / dt };
    let net_down = if st.last_ts == 0 { 0 } else { down.saturating_sub(st.last_down) as u64 * 1000 / dt };
    st.last_up = up;
    st.last_down = down;
    st.last_ts = now;

    let (battery, ac) = battery_status();

    Ok(SinguPulse {
        cpu_usage: cpu,
        mem_used_mb: mem_used,
        mem_total_mb: mem_total,
        gpu_usage: None, // Windows 无免驱动通用 GPU% 读数：诚实 None
        cpu_temp_c: temp,
        temp_estimated: estimated,
        fan_high,
        battery_percent: battery,
        battery_ac: ac,
        disks,
        net_up_bps: net_up,
        net_down_bps: net_down,
        ts_ms: now,
    })
}

// ---------------------------------------------------------------------------
// Q-37 临时篝火：环境临时文件账本
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SinguTempEntry {
    pub path: String,
    pub kind: String, // temp | cache | logs | data
    pub bytes: u64,
    pub files: u64,
}

fn dir_size(p: &PathBuf) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    fn walk(p: &PathBuf, bytes: &mut u64, files: &mut u64, depth: u8) {
        if depth > 6 {
            return; // 深度保护（防符号环/超深目录）
        }
        if let Ok(rd) = fs::read_dir(p) {
            for e in rd.flatten() {
                let Ok(meta) = e.metadata() else { continue };
                if meta.is_dir() {
                    walk(&e.path(), bytes, files, depth + 1);
                } else {
                    *bytes = bytes.saturating_add(meta.len());
                    *files += 1;
                }
            }
        }
    }
    walk(p, &mut bytes, &mut files, 0);
    (bytes, files)
}

#[tauri::command(async)]
pub fn singu_temp_scan(app: tauri::AppHandle) -> Result<Vec<SinguTempEntry>, String> {
    use tauri::Manager;
    let mut out: Vec<SinguTempEntry> = Vec::new();
    let mut push = |p: PathBuf, kind: &str, out: &mut Vec<SinguTempEntry>| {
        if p.exists() {
            let (bytes, files) = dir_size(&p);
            if bytes > 0 {
                out.push(SinguTempEntry {
                    path: p.to_string_lossy().to_string(),
                    kind: kind.to_string(),
                    bytes,
                    files,
                });
            }
        }
    };
    // 系统临时目录中本环境的子目录（variable-* 前缀纪律）
    if let Ok(tmp) = std::env::temp_dir().read_dir() {
        for e in tmp.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("variable-") {
                push(e.path(), "temp", &mut out);
            }
        }
    }
    // 应用数据目录下的 cache / logs
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    push(data.join("cache"), "cache", &mut out);
    push(data.join("logs"), "logs", &mut out);
    // 环境工作区 .trash（回收站暂存）
    push(data.join("workspace").join(".trash"), "data", &mut out);
    Ok(out)
}

/// Q-37 一键清理：只清理「账本口径」内的目录（temp/cache），绝不越权。
#[tauri::command(async)]
pub fn singu_temp_clear(app: tauri::AppHandle, kinds: Vec<String>) -> Result<u64, String> {
    use tauri::Manager;
    let mut freed = 0u64;
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut targets: Vec<PathBuf> = Vec::new();
    if kinds.iter().any(|k| k == "temp") {
        if let Ok(tmp) = std::env::temp_dir().read_dir() {
            for e in tmp.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("variable-") {
                    targets.push(e.path());
                }
            }
        }
    }
    if kinds.iter().any(|k| k == "cache") {
        targets.push(data.join("cache"));
    }
    for t in targets {
        let (bytes, _) = dir_size(&t);
        if fs::remove_dir_all(&t).is_ok() {
            freed = freed.saturating_add(bytes);
        }
    }
    Ok(freed)
}

// ---------------------------------------------------------------------------
// Q-67 下载检疫：NTFS Zone.Identifier
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SinguZone {
    /// true = 来自互联网（MotW ZoneId 3/4）。
    pub from_internet: bool,
    pub zone_id: Option<u32>,
    pub readable: bool,
}

#[tauri::command(async)]
pub fn singu_zone_check(path: String) -> Result<SinguZone, String> {
    #[cfg(windows)]
    {
        let ads = format!("{}:Zone.Identifier", path);
        match fs::read_to_string(&ads) {
            Ok(content) => {
                let mut zone: Option<u32> = None;
                for line in content.lines() {
                    if let Some(v) = line.strip_prefix("ZoneId=") {
                        zone = v.trim().parse::<u32>().ok();
                    }
                }
                let z = zone.unwrap_or(0);
                Ok(SinguZone {
                    from_internet: z == 3 || z == 4,
                    zone_id: zone,
                    readable: true,
                })
            }
            Err(_) => Ok(SinguZone {
                from_internet: false,
                zone_id: None,
                readable: false, // 无 ADS = 本地生成文件（不是失败）
            }),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(SinguZone {
            from_internet: false,
            zone_id: None,
            readable: false,
        })
    }
}

// ---------------------------------------------------------------------------
// Q-64 访问日志：JSONL 环形 500 条（可焚毁）
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone, serde::Deserialize)]
pub struct SinguJournalEntry {
    pub ts_ms: u64,
    pub dir: String,
    pub action: String, // read | write | open
    pub actor: String,  // explorer | ext:<id> | open-with
}

fn journal_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(data.join("singularity-journal.jsonl"))
}

#[tauri::command(async)]
pub fn singu_journal_log(app: tauri::AppHandle, dir: String, action: String, actor: String) -> Result<(), String> {
    let p = journal_path(&app)?;
    let entry = SinguJournalEntry {
        ts_ms: now_ms(),
        dir,
        action,
        actor,
    };
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    use std::io::Write;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(&p).map_err(|e| e.to_string())?;
    let line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    writeln!(f, "{}", line).map_err(|e| e.to_string())?;
    // 环形裁剪：>600 行时保留最新 500
    let _ = trim_journal(&p);
    Ok(())
}

fn trim_journal(p: &PathBuf) -> Result<(), String> {
    let content = fs::read_to_string(p).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= 600 {
        return Ok(());
    }
    let keep: Vec<&str> = lines[lines.len() - 500..].to_vec();
    fs::write(p, keep.join("\n") + "\n").map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn singu_journal_list(app: tauri::AppHandle) -> Result<Vec<SinguJournalEntry>, String> {
    let p = journal_path(&app)?;
    if !p.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&p).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for line in content.lines().rev().take(500) {
        if let Ok(e) = serde_json::from_str::<SinguJournalEntry>(line) {
            out.push(e);
        }
    }
    Ok(out)
}

/// 焚毁（零残留纪律）：删除整个日志文件。
#[tauri::command(async)]
pub fn singu_journal_clear(app: tauri::AppHandle) -> Result<(), String> {
    let p = journal_path(&app)?;
    if p.exists() {
        fs::remove_file(&p).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Q-38 批量属性
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SinguBatchResult {
    pub ok: u64,
    pub failed: Vec<String>,
}

#[cfg(windows)]
fn set_windows_attrs(path: &str, readonly: bool, hidden: bool, archive: bool) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{
        SetFileAttributesW, FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_HIDDEN,
        FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY, FILE_FLAGS_AND_ATTRIBUTES,
    };
    let mut attrs = FILE_ATTRIBUTE_NORMAL.0;
    if readonly {
        attrs |= FILE_ATTRIBUTE_READONLY.0;
    }
    if hidden {
        attrs |= FILE_ATTRIBUTE_HIDDEN.0;
    }
    if archive {
        attrs |= FILE_ATTRIBUTE_ARCHIVE.0;
    }
    unsafe {
        SetFileAttributesW(&HSTRING::from(path), FILE_FLAGS_AND_ATTRIBUTES(attrs))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command(async)]
pub fn singu_batch_attrs(
    paths: Vec<String>,
    readonly: Option<bool>,
    hidden: Option<bool>,
    archive: Option<bool>,
    shift_days: Option<i64>,
) -> Result<SinguBatchResult, String> {
    let mut ok = 0u64;
    let mut failed: Vec<String> = Vec::new();
    for p in &paths {
        let mut good = true;
        // 属性位（仅 Windows；其它平台如实计入 failed）
        if readonly.is_some() || hidden.is_some() || archive.is_some() {
            #[cfg(windows)]
            {
                if let Err(e) = set_windows_attrs(
                    p,
                    readonly.unwrap_or(false),
                    hidden.unwrap_or(false),
                    archive.unwrap_or(false),
                ) {
                    failed.push(format!("{}: {}", p, e));
                    good = false;
                }
            }
            #[cfg(not(windows))]
            {
                failed.push(format!("{}: attrs unsupported on this platform", p));
                good = false;
            }
        }
        // 时间平移
        if let Some(days) = shift_days {
            match fs::metadata(p) {
                Ok(meta) => {
                    let modified = meta.modified().map_err(|e| e.to_string())?;
                    let shifted = modified
                        .checked_add(Duration::from_secs(days.saturating_mul(86400).max(0) as u64))
                        .unwrap_or(modified);
                    let f = fs::OpenOptions::new().write(true).open(p).map_err(|e| e.to_string())?;
                    f.set_modified(shifted).map_err(|e| e.to_string())?;
                }
                Err(e) => {
                    failed.push(format!("{}: {}", p, e));
                    good = false;
                }
            }
        }
        if good {
            ok += 1;
        }
    }
    Ok(SinguBatchResult { ok, failed })
}

// ---------------------------------------------------------------------------
// Q-42 存档快检：zip 逐条目 CRC
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SinguArchiveReport {
    pub total: u64,
    pub ok: u64,
    pub failed: Vec<String>,
    pub readable: bool,
}

#[tauri::command(async)]
pub fn singu_archive_check(path: String) -> Result<SinguArchiveReport, String> {
    let f = fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut zip = match zip::ZipArchive::new(f) {
        Ok(z) => z,
        Err(_) => {
            return Ok(SinguArchiveReport {
                total: 0,
                ok: 0,
                failed: vec![path],
                readable: false,
            })
        }
    };
    let total = zip.len() as u64;
    let mut ok = 0u64;
    let mut failed: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        match zip.by_index(i) {
            Ok(mut entry) => {
                let name = entry.name().to_string();
                // 读取全量内容 → zip crate 在 CRC 不匹配时返回错误
                let mut sink = Vec::new();
                match std::io::Read::read_to_end(&mut entry, &mut sink) {
                    Ok(_) => ok += 1,
                    Err(e) => failed.push(format!("{}: {}", name, e)),
                }
            }
            Err(e) => failed.push(format!("entry {}: {}", i, e)),
        }
    }
    Ok(SinguArchiveReport {
        total,
        ok,
        failed,
        readable: true,
    })
}

// ---------------------------------------------------------------------------
// Q-70 数据冰山 / Q-95 数据配额：应用数据各分区构成
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SinguDataZone {
    /// zone = database | cache | logs | temp | media | workspace
    pub zone: String,
    pub bytes: u64,
}

#[tauri::command(async)]
pub fn singu_data_profile(app: tauri::AppHandle) -> Result<Vec<SinguDataZone>, String> {
    use tauri::Manager;
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut out: Vec<SinguDataZone> = Vec::new();
    // 主数据库（SQLite/JSON 主库文件群）
    let mut db_bytes = 0u64;
    if let Ok(rd) = fs::read_dir(&data) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".db") || name.ends_with(".sqlite") || name.ends_with(".json") {
                if let Ok(meta) = e.metadata() {
                    db_bytes = db_bytes.saturating_add(meta.len());
                }
            }
        }
    }
    if db_bytes > 0 {
        out.push(SinguDataZone { zone: "database".into(), bytes: db_bytes });
    }
    // 分区目录
    for (dir, zone) in [
        ("cache", "cache"),
        ("logs", "logs"),
        ("workspace", "workspace"),
        ("media", "media"),
    ] {
        let p = data.join(dir);
        if p.exists() {
            let (bytes, _) = dir_size(&p);
            if bytes > 0 {
                out.push(SinguDataZone { zone: zone.into(), bytes });
            }
        }
    }
    // 临时目录（variable-* 前缀纪律，与 singu_temp_scan 同口径）
    let mut temp_bytes = 0u64;
    if let Ok(tmp) = std::env::temp_dir().read_dir() {
        for e in tmp.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("variable-") {
                let (b, _) = dir_size(&e.path());
                temp_bytes = temp_bytes.saturating_add(b);
            }
        }
    }
    if temp_bytes > 0 {
        out.push(SinguDataZone { zone: "temp".into(), bytes: temp_bytes });
    }
    Ok(out)
}
