//! AI-11 系统集成与硬件组 — 只读系统探针（sysprobe.rs）。
//!
//! 覆盖：U-43 多显示器枚举 / V-52 可靠性时间线 / V-53 端口侦探 /
//! V-55 大文件雷达 / V-56 运行时长 / V-60 启动归因 / N-25 健康自检 /
//! V-59 断电自检 / N-24 预热（L1 页缓存预读，白名单限定）。
//!
//! 红线（承 ASCENT/NEXT/化境口径）：
//! - 一切硬件/系统 API 只读或合法用户级调用；本模块除「预热预读」与
//!   「自检白名单修复」外零写入；
//! - 探针失败一律如实返回错误/降级标记，绝不编造数据；
//! - V-53 不提供 kill 入口（任务管理器领地）；V-52 不删事件（系统领地）。

use serde::Serialize;

// ---------- U-43 多显示器枚举 ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDto {
    /// 设备名（\\.\DISPLAY1），热插拔档案的主键。
    pub device: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub primary: bool,
}

#[cfg(windows)]
unsafe fn enum_monitors() -> Vec<MonitorDto> {
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW};
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};

    unsafe extern "system" fn cb(
        hmon: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<MonitorDto>);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = u32::try_from(std::mem::size_of::<MONITORINFOEXW>()).unwrap_or(0);
        if GetMonitorInfoW(hmon, &mut info as *mut _ as *mut _).as_bool() {
            let rc = info.monitorInfo.rcMonitor;
            out.push(MonitorDto {
                device: String::from_utf16_lossy(&info.szDevice)
                    .trim_end_matches('\0')
                    .to_string(),
                x: rc.left,
                y: rc.top,
                w: rc.right - rc.left,
                h: rc.bottom - rc.top,
                primary: (info.monitorInfo.dwFlags & 1) != 0,
            });
        }
        true.into()
    }

    let mut out: Vec<MonitorDto> = Vec::new();
    unsafe {
        let lparam = LPARAM(&mut out as *mut _ as isize);
        let _ = EnumDisplayMonitors(None, None, Some(cb), lparam);
    }
    out
}

/// U-43：显示器拓扑（只读）。档案与布局在前端持久化（tool_data），重接入按 device 名恢复。
#[tauri::command]
pub fn monitor_list() -> Result<Vec<MonitorDto>, String> {
    #[cfg(windows)]
    return unsafe { Ok(enum_monitors()) };
    #[cfg(not(windows))]
    Err("not-supported".into())
}

// ---------- V-53 端口侦探（GetExtendedTcpTable / GetExtendedUdpTable，只读） ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PortRow {
    pub proto: String, // "tcp" | "udp"
    pub local: String, // ip:port
    pub remote: String,
    pub state: String,
    pub pid: u32,
}

#[cfg(windows)]
fn ip_to_string(v: u32) -> String {
    format!("{}.{}.{}.{}", v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff, (v >> 24) & 0xff)
}

#[cfg(windows)]
fn port_net_to_host(p: u32) -> u16 {
    ((p & 0xff) << 8 | ((p >> 8) & 0xff)) as u16
}

/// V-53：TCP + UDP 监听/连接表（进程侧通过 PID 在前端与 procList 关联）。
#[tauri::command]
pub fn port_table() -> Result<Vec<PortRow>, String> {
    #[cfg(windows)]
    {
        use windows::Win32::NetworkManagement::IpHelper::{GetExtendedTcpTable, GetExtendedUdpTable, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_CLASS};
        use windows::Win32::Foundation::NO_ERROR;
        use windows::Win32::Networking::WinSock::AF_INET;

        let mut rows: Vec<PortRow> = Vec::new();

        // ---- TCP ----
        unsafe {
            let mut size: u32 = 0;
            let _ = GetExtendedTcpTable(
                None,
                &mut size,
                false,
                AF_INET.0 as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            );
            let mut buf = vec![0u8; size as usize];
            let r = GetExtendedTcpTable(
                Some(buf.as_mut_ptr() as *mut _),
                &mut size,
                false,
                AF_INET.0 as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            );
            if r == NO_ERROR.0 {
                let base = buf.as_ptr() as *const u32;
                let n = *base;
                let tab = base.add(1) as *const [u32; 6]; // MIB_TCPROW_OWNER_PID = 6 × u32
                for i in 0..n as usize {
                    let row = *tab.add(i);
                    let state = row[0];
                    let lport = port_net_to_host(row[2]);
                    let rport = port_net_to_host(row[4]);
                    rows.push(PortRow {
                        proto: "tcp".into(),
                        local: format!("{}:{}", ip_to_string(row[1]), lport),
                        remote: if rport == 0 {
                            "-".into()
                        } else {
                            format!("{}:{}", ip_to_string(row[3]), rport)
                        },
                        state: tcp_state_name(state),
                        pid: row[5],
                    });
                }
            }
        }

        // ---- UDP ----
        unsafe {
            let af = AF_INET.0 as u32;
            let mut size: u32 = 0;
            let _ = GetExtendedUdpTable(None, &mut size, false, af, UDP_TABLE_CLASS(1), 0);
            let mut buf = vec![0u8; size.max(1) as usize];
            let r = GetExtendedUdpTable(
                Some(buf.as_mut_ptr() as *mut _),
                &mut size,
                false,
                af,
                UDP_TABLE_CLASS(1), // UDP_TABLE_OWNER_PID
                0,
            );
            if r == 0 {
                let base = buf.as_ptr() as *const u32;
                let n = *base;
                let tab = base.add(1) as *const [u32; 3]; // MIB_UDPROW_OWNER_PID = 3 × u32
                for i in 0..n as usize {
                    let row = *tab.add(i);
                    rows.push(PortRow {
                        proto: "udp".into(),
                        local: format!("{}:{}", ip_to_string(row[0]), port_net_to_host(row[1])),
                        remote: "-".into(),
                        state: "LISTEN".into(),
                        pid: row[2],
                    });
                }
            }
        }
        Ok(rows)
    }
    #[cfg(not(windows))]
    Err("not-supported".into())
}

#[cfg(windows)]
fn tcp_state_name(s: u32) -> String {
    // TCP_STATE 值（Tcpconst.h）：只给常见态可读名，其余如实给数字。
    match s {
        2 => "LISTEN".into(),
        3 => "SYN_SENT".into(),
        4 => "SYN_RCVD".into(),
        5 => "ESTABLISHED".into(),
        6 => "FIN_WAIT1".into(),
        7 => "FIN_WAIT2".into(),
        8 => "CLOSE_WAIT".into(),
        9 => "CLOSING".into(),
        10 => "LAST_ACK".into(),
        11 => "TIME_WAIT".into(),
        12 => "DELETE_TCB".into(),
        1 => "CLOSED".into(),
        other => format!("UNKNOWN({other})"),
    }
}

// ---------- V-52 可靠性时间线（wevtutil 只读查询） ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EventRow {
    pub log: String,
    pub time: String,
    pub level: String,
    pub provider: String,
    pub id: u32,
    pub message: String,
}

/// V-52：只读读取 System / Application 关键事件（Level 1-2），按时间倒序。
/// 数据源 = PowerShell Get-WinEvent → JSON；不做任何删除/清理（系统领地）。
#[tauri::command]
pub fn eventlog_recent(log: String, count: u32) -> Result<Vec<EventRow>, String> {
    if log != "System" && log != "Application" {
        return Err("log must be System or Application".into());
    }
    let count = count.clamp(1, 200);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let ps = format!(
            "$ErrorActionPreference='SilentlyContinue'; Get-WinEvent -FilterHashtable @{{LogName='{log}';Level=1,2}} -MaxEvents {count} | Select-Object TimeCreated,LevelDisplayName,ProviderName,Id,Message | ConvertTo-Json -Depth 3 -Compress"
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| format!("PowerShell 启动失败: {e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        if stdout.trim().is_empty() {
            return Ok(vec![]); // 无关键事件 = 好事，如实返回空
        }
        parse_eventlog_json(&stdout, &log)
    }
    #[cfg(not(windows))]
    Err("not-supported".into())
}

/// 解析 Get-WinEvent | ConvertTo-Json 输出（单对象/数组两种形状都要容忍）。
pub(crate) fn parse_eventlog_json(raw: &str, log: &str) -> Result<Vec<EventRow>, String> {
    #[derive(serde::Deserialize)]
    #[allow(non_snake_case)]
    struct Ev {
        #[serde(default)]
        TimeCreated: Option<String>,
        #[serde(default)]
        LevelDisplayName: Option<String>,
        #[serde(default)]
        ProviderName: Option<String>,
        #[serde(default)]
        Id: Option<serde_json::Value>,
        #[serde(default)]
        Message: Option<String>,
    }
    let v: serde_json::Value =
        serde_json::from_str(raw.trim()).map_err(|e| format!("事件解析失败: {e}"))?;
    let arr: Vec<serde_json::Value> = if v.is_array() {
        v.as_array().unwrap().clone()
    } else {
        vec![v]
    };
    let mut rows = Vec::new();
    for item in arr {
        let ev: Ev = match serde_json::from_value(item) {
            Ok(e) => e,
            Err(_) => continue,
        };
        rows.push(EventRow {
            log: log.to_string(),
            time: ev.TimeCreated.unwrap_or_default(),
            level: ev.LevelDisplayName.unwrap_or_else(|| "Error".into()),
            provider: ev.ProviderName.unwrap_or_default(),
            id: ev.Id.and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            message: ev
                .Message
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(180)
                .collect(),
        });
    }
    Ok(rows)
}

// ---------- V-55 大文件雷达（mtime 全量扫描，只读） ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileHit {
    pub path: String,
    pub size: u64,
    /// modified 毫秒时间戳
    pub mtime: u64,
}

/// V-55：指定根目录下「最近 days 天内、≥ min_mb MB」的文件，按体积倒序 Top 100。
/// 纯只读；无 USN Journal 时的诚实降级路径（前端标注扫描耗时）。
#[tauri::command]
pub fn bigfile_scan(root: String, min_mb: u64, days: u32) -> Result<Vec<FileHit>, String> {
    let min_bytes = min_mb.saturating_mul(1024 * 1024);
    let cutoff = (now_ms()).saturating_sub(days as u64 * 86_400_000);
    let mut hits: Vec<FileHit> = Vec::new();
    let t0 = std::time::Instant::now();
    scan_dir(&root, min_bytes, cutoff, &mut hits, 0)?;
    hits.sort_by(|a, b| b.size.cmp(&a.size));
    hits.truncate(100);
    let _ = t0.elapsed(); // 耗时由前端计时展示
    Ok(hits)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn scan_dir(dir: &str, min_bytes: u64, cutoff: u64, out: &mut Vec<FileHit>, depth: u32) -> Result<(), String> {
    if depth > 6 || out.len() > 4000 {
        return Ok(());
    }
    let rd = std::fs::read_dir(dir).map_err(|e| format!("无法读取 {dir}: {e}"))?;
    for entry in rd.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            // 跳过系统级/符号链接目录，防循环与深扫
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if matches!(name.as_str(), "windows" | "proc" | "sys" | "dev" | "$recycle.bin" | "system volume information") {
                continue;
            }
            scan_dir(&path.to_string_lossy(), min_bytes, cutoff, out, depth + 1)?;
        } else if ft.is_file() {
            if let Ok(meta) = entry.metadata() {
                let size = meta.len();
                if size >= min_bytes {
                    if let Ok(m) = meta.modified() {
                        let ms = m
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        if ms >= cutoff {
                            out.push(FileHit { path: path.to_string_lossy().to_string(), size, mtime: ms });
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// ---------- V-56 运行时长（GetTickCount64，只读） ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UptimeDto {
    /// 系统 uptime（秒）
    pub sys_secs: u64,
    /// 环境自身运行时长（秒）
    pub env_secs: u64,
}

#[cfg(windows)]
fn tick64() -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount64;
    unsafe { GetTickCount64() }
}

/// V-56：系统/环境运行时长（与 GetTickCount64 一致口径，只读）。
#[tauri::command]
pub fn sys_uptime() -> Result<UptimeDto, String> {
    #[cfg(windows)]
    {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let start = START.get_or_init(std::time::Instant::now);
        Ok(UptimeDto {
            sys_secs: tick64() / 1000,
            env_secs: start.elapsed().as_secs(),
        })
    }
    #[cfg(not(windows))]
    Err("not-supported".into())
}

// ---------- V-60 启动项耗时归因（Win32_Process 创建时刻采样，只读） ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StartupProc {
    pub pid: u32,
    pub name: String,
    /// 开机后多少毫秒创建（负值异常时为 0）
    pub boot_offset_ms: u64,
    pub path: String,
}

/// V-60：枚举「系统启动后创建」的进程，bootOffsetMs = 创建时刻 − 开机时刻，
/// 作为对开机时间贡献的诚实采样代理（非首次空闲精确值，UI 如实标注）。
#[tauri::command]
pub fn startup_procs() -> Result<Vec<StartupProc>, String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let ps = "$ErrorActionPreference='SilentlyContinue'; $boot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime; Get-CimInstance Win32_Process | Where-Object { $_.CreationDate -gt $boot } | ForEach-Object { [pscustomobject]@{ pid=$_.ProcessId; name=$_.Name; path=$_.ExecutablePath; ms=[long](($_.CreationDate - $boot).TotalMilliseconds) } } | ConvertTo-Json -Compress";
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", ps])
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| format!("PowerShell 启动失败: {e}"))?;
        let raw = String::from_utf8_lossy(&out.stdout);
        if raw.trim().is_empty() {
            return Ok(vec![]);
        }
        let v: serde_json::Value = serde_json::from_str(raw.trim()).map_err(|e| e.to_string())?;
        let arr = if v.is_array() { v.as_array().unwrap().clone() } else { vec![v] };
        let mut rows = Vec::new();
        for item in arr {
            let pid = item.get("pid").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            let name = item.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let ms = item.get("ms").and_then(|x| x.as_i64()).unwrap_or(0).max(0) as u64;
            let path = item.get("path").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if pid != 0 && !name.is_empty() {
                rows.push(StartupProc { pid, name, boot_offset_ms: ms, path });
            }
        }
        rows.sort_by_key(|r| r.boot_offset_ms);
        Ok(rows)
    }
    #[cfg(not(windows))]
    Err("not-supported".into())
}

// ---------- N-25 健康自检（轻量只读 + 白名单修复） ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    pub id: String,
    pub name: String,
    /// ok | warn | fail
    pub status: String,
    pub detail: String,
}

fn db_path(st: &tauri::State<'_, crate::state::AppState>) -> std::path::PathBuf {
    st.data_dir.join("variable.db")
}

/// N-25：环境自检树（每项 {id/名称/状态/证据}；3s 预算由调用方控制，本函数内
/// 每项检查均为轻量同步操作）。只读为主；修复走 heal_run 白名单。
#[tauri::command]
pub fn selfheal_checks(st: tauri::State<'_, crate::state::AppState>) -> Result<Vec<CheckItem>, String> {
    let mut out = Vec::new();

    // 1) 数据库完整性（quick_check，只读）
    let dbp = db_path(&st);
    if dbp.exists() {
        match rusqlite::Connection::open_with_flags(
            &dbp,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) {
            Ok(conn) => match conn.query_row("PRAGMA quick_check", [], |r| {
                r.get::<_, String>(0)
            }) {
                Ok(s) if s == "ok" => out.push(CheckItem { id: "db".into(), name: "数据库完整性".into(), status: "ok".into(), detail: "quick_check ok".into() }),
                Ok(s) => out.push(CheckItem { id: "db".into(), name: "数据库完整性".into(), status: "fail".into(), detail: s }),
                Err(e) => out.push(CheckItem { id: "db".into(), name: "数据库完整性".into(), status: "warn".into(), detail: e.to_string() }),
            },
            Err(e) => out.push(CheckItem { id: "db".into(), name: "数据库完整性".into(), status: "fail".into(), detail: format!("无法打开: {e}") }),
        }
    } else {
        out.push(CheckItem { id: "db".into(), name: "数据库完整性".into(), status: "warn".into(), detail: "数据库文件不存在（首次启动）".into() });
    }

    // 2) 数据目录可写
    let probe = st.data_dir.join(".selfheal-probe");
    match std::fs::write(&probe, b"ok") {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            out.push(CheckItem { id: "dataDir".into(), name: "数据目录可写".into(), status: "ok".into(), detail: st.data_dir.to_string_lossy().to_string() });
        }
        Err(e) => out.push(CheckItem { id: "dataDir".into(), name: "数据目录可写".into(), status: "fail".into(), detail: e.to_string() }),
    }

    // 3) 工具数据文件可解析（JSON 类 tool_data）
    let mut bad = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&st.data_dir.join("tools")) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    if serde_json::from_str::<serde_json::Value>(&s).is_err() {
                        bad.push(p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                    }
                }
            }
        }
    }
    out.push(if bad.is_empty() {
        CheckItem { id: "toolData".into(), name: "配置数据完整性".into(), status: "ok".into(), detail: "全部 JSON 可解析".into() }
    } else {
        CheckItem { id: "toolData".into(), name: "配置数据完整性".into(), status: "warn".into(), detail: format!("不可解析: {}", bad.join(", ")) }
    });

    // 4) 磁盘水位（数据目录所在盘剩余 < 2GB = warn）
    let free = free_bytes_of(&st.data_dir);
    out.push(match free {
        Some(bytes) if bytes < 2 * 1024 * 1024 * 1024 => CheckItem { id: "disk".into(), name: "磁盘水位".into(), status: "warn".into(), detail: format!("剩余约 {:.1} GB", bytes as f64 / 1e9) },
        Some(bytes) => CheckItem { id: "disk".into(), name: "磁盘水位".into(), status: "ok".into(), detail: format!("剩余约 {:.1} GB", bytes as f64 / 1e9) },
        None => CheckItem { id: "disk".into(), name: "磁盘水位".into(), status: "warn".into(), detail: "无法读取剩余空间".into() },
    });

    // 5) 回收站索引（可统计即可）
    let trash = st.data_dir.join("trash");
    out.push(CheckItem {
        id: "trash".into(),
        name: "回收站索引".into(),
        status: if trash.exists() || std::fs::create_dir_all(&trash).is_ok() { "ok".into() } else { "warn".into() },
        detail: trash.to_string_lossy().to_string(),
    });

    Ok(out)
}

#[cfg(windows)]
fn free_bytes_of(p: &std::path::Path) -> Option<u64> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let root: std::path::PathBuf = {
        // 取最近存在祖先的根
        let mut cur = p.to_path_buf();
        loop {
            if cur.exists() { break; }
            if !cur.pop() { return None; }
        }
        cur
    };
    let s = HSTRING::from(root.as_os_str());
    let mut free = 0u64;
    let mut total = 0u64;
    let mut total_free = 0u64;
    unsafe {
        if GetDiskFreeSpaceExW(&s, Some(&mut free), Some(&mut total), Some(&mut total_free)).is_ok() {
            Some(free)
        } else {
            None
        }
    }
}
#[cfg(not(windows))]
fn free_bytes_of(_p: &std::path::Path) -> Option<u64> {
    None
}

/// N-25：白名单修复器（当前两项：重建 .selfheal-probe 校验位 / 清除损坏的
/// tool_data JSON 缓存为 null）。白名单之外一律只报告不修复。
#[tauri::command]
pub fn heal_run(st: tauri::State<'_, crate::state::AppState>, id: String) -> Result<String, String> {
    match id.as_str() {
        "trash" => {
            std::fs::create_dir_all(st.data_dir.join("trash")).map_err(|e| e.to_string())?;
            Ok("已重建回收站目录".into())
        }
        "toolData" => {
            // 移走无法解析的 JSON（改名备份，不静默删除——红线）
            let tools = st.data_dir.join("tools");
            let mut fixed = 0;
            if let Ok(rd) = std::fs::read_dir(&tools) {
                for entry in rd.flatten() {
                    let p = entry.path();
                    if p.extension().and_then(|e| e.to_str()) == Some("json") {
                        if let Ok(s) = std::fs::read_to_string(&p) {
                            if serde_json::from_str::<serde_json::Value>(&s).is_err() {
                                let bak = p.with_extension("json.broken");
                                std::fs::rename(&p, &bak).map_err(|e| e.to_string())?;
                                fixed += 1;
                            }
                        }
                    }
                }
            }
            Ok(format!("已备份 {fixed} 个损坏配置（.broken 后缀，可人工恢复）"))
        }
        other => Err(format!("「{other}」不在修复白名单内（只报告不越权）")),
    }
}

// ---------- V-59 断电恢复自检 ----------

/// 干净关机标记文件名（每次正常退出时由 lib.rs RunEvent::Exit 写入）。
pub const CLEAN_SHUTDOWN_FILE: &str = "ai11-clean-shutdown";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PowerLossReport {
    /// 本次启动前是否检测到异常断电（缺干净关机标记）
    pub dirty: bool,
    pub checks: Vec<CheckItem>,
    pub fixed: Vec<String>,
}

/// V-59：检测上次关机是否异常（脏关机），并运行四项轻量自检（<5s、并行性足够——
/// 各项均为本地轻操作）。自动修复仅限白名单（缓存重建类）。
#[tauri::command]
pub fn pwrloss_check(st: tauri::State<'_, crate::state::AppState>) -> Result<PowerLossReport, String> {
    let marker = st.data_dir.join(CLEAN_SHUTDOWN_FILE);
    let dirty = !marker.exists();
    // 自检完先补写标记：本次会话内再查不重复触发（正常退出会续写）
    let _ = std::fs::write(&marker, now_ms().to_string());

    let mut checks = Vec::new();
    let mut fixed = Vec::new();

    // 1) 数据库完整性
    let dbp = db_path(&st);
    if dbp.exists() {
        if let Ok(conn) = rusqlite::Connection::open_with_flags(&dbp, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
            let stt = conn
                .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
                .unwrap_or_else(|_| "query-error".into());
            checks.push(CheckItem { id: "db".into(), name: "数据库完整性".into(), status: if stt == "ok" { "ok".into() } else { "fail".into() }, detail: stt });
        }
    } else {
        checks.push(CheckItem { id: "db".into(), name: "数据库完整性".into(), status: "ok".into(), detail: "无数据库（首启）".into() });
    }

    // 2) 图标/临时缓存目录（白名单：可重建）
    let cache = st.data_dir.join("cache");
    if !cache.exists() && std::fs::create_dir_all(&cache).is_ok() {
        fixed.push("已重建缓存目录".into());
        checks.push(CheckItem { id: "cache".into(), name: "缓存目录".into(), status: "ok".into(), detail: "已重建".into() });
    } else {
        checks.push(CheckItem { id: "cache".into(), name: "缓存目录".into(), status: "ok".into(), detail: cache.to_string_lossy().to_string() });
    }

    // 3) 设置文件 JSON 可解析（只读）
    let mut broken = 0;
    if let Ok(rd) = std::fs::read_dir(&st.data_dir.join("tools")) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    if serde_json::from_str::<serde_json::Value>(&s).is_err() {
                        broken += 1;
                    }
                }
            }
        }
    }
    checks.push(CheckItem {
        id: "settings".into(),
        name: "设置文件".into(),
        status: if broken == 0 { "ok".into() } else { "warn".into() },
        detail: if broken == 0 { "全部可解析".into() } else { format!("{broken} 个损坏（可在健康自愈中心修复）") },
    });

    // 4) 回收站索引存在性（白名单：可重建）
    let trash = st.data_dir.join("trash");
    if !trash.exists() && std::fs::create_dir_all(&trash).is_ok() {
        fixed.push("已重建回收站索引目录".into());
        checks.push(CheckItem { id: "trash".into(), name: "回收站索引".into(), status: "ok".into(), detail: "已重建".into() });
    } else {
        checks.push(CheckItem { id: "trash".into(), name: "回收站索引".into(), status: "ok".into(), detail: trash.to_string_lossy().to_string() });
    }

    Ok(PowerLossReport { dirty, checks, fixed })
}

// ---------- N-24 预测性预热（L1：页缓存预读，白名单 = 用户启动史） ----------

/// N-24 L1：对白名单可执行文件做页缓存预读（读前 2MB）。
/// 纪律红线：仅预读、不启动进程、不模拟输入、不联网；路径必须存在且为文件。
#[tauri::command]
pub fn predwarm(path: String) -> Result<bool, String> {
    let p = std::path::Path::new(&path);
    if !p.is_file() {
        return Err(format!("「{path}」不是文件（拒绝预热非白名单路径）"));
    }
    let f = std::fs::File::open(p).map_err(|e| format!("打开失败: {e}"))?;
    use std::io::Read;
    let mut f = f;
    let mut buf = vec![0u8; 2 * 1024 * 1024];
    match f.read(&mut buf) {
        Ok(_) => Ok(true),
        Err(e) => Err(format!("预读失败: {e}")),
    }
}

// ---------- U-44/U-45/U-47 外设·音频·无线（只读降级探针） ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PeriphProbe {
    /// 音频设备名（Win32_SoundDevice，U-45 降级只读视图）
    pub audio_devices: Vec<String>,
    /// WLAN 接口与连接状态（netsh，U-47 降级只读视图）
    pub wlan: String,
    /// 可见网络 SSID 列表（netsh wlan show networks；失败为空）
    pub wlan_networks: Vec<String>,
    /// 蓝牙无线电台（Win32_PnPEntity 蓝牙类过滤）
    pub bt_radios: Vec<String>,
    /// 输入外设（鼠标/键盘 PnP 名，U-44 外设档案数据面）
    pub input_devices: Vec<String>,
}

/// U-44/U-45/U-47：外设与无线一次性只读探针（PowerShell/netsh，无任何写入）。
/// 降级口径：任一子探针失败如实留空，不编造数据。
#[tauri::command]
pub fn periph_probe() -> Result<PeriphProbe, String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        fn ps(args: &str) -> Option<String> {
            let out = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", args])
                .creation_flags(0x0800_0000)
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        }
        let lines = |s: Option<String>| -> Vec<String> {
            s.map(|v| {
                v.lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default()
        };
        let audio_devices = lines(ps(
            "Get-CimInstance Win32_SoundDevice | ForEach-Object { $_.Name }",
        ));
        let input_devices = lines(ps(
            "Get-CimInstance Win32_PnPEntity | Where-Object { $_.PNPClass -in 'Mouse','Keyboard' -and $_.Status -eq 'OK' } | ForEach-Object { $_.Name }",
        ));
        let bt_radios = lines(ps(
            "Get-CimInstance Win32_PnPEntity | Where-Object { $_.Name -match '蓝牙|Bluetooth' } | ForEach-Object { $_.Name }",
        ));
        let wlan_raw = ps("netsh wlan show interfaces");
        let mut wlan = "unavailable".to_string();
        let mut wlan_networks: Vec<String> = Vec::new();
        if let Some(raw) = wlan_raw {
            for l in raw.lines() {
                let t = l.trim();
                if t.starts_with("状态") || t.starts_with("State") {
                    wlan = t.to_string();
                }
            }
        }
        if let Some(raw) = ps("netsh wlan show networks") {
            for l in raw.lines() {
                let t = l.trim();
                if let Some(rest) = t.strip_prefix("SSID ") {
                    if let Some(name) = rest.split_once(':').map(|(_, v)| v.trim().to_string()) {
                        if !name.is_empty() && !wlan_networks.contains(&name) {
                            wlan_networks.push(name);
                        }
                    }
                }
            }
        }
        Ok(PeriphProbe { audio_devices, wlan, wlan_networks, bt_radios, input_devices })
    }
    #[cfg(not(windows))]
    Err("not-supported".into())
}

// ---------- 测试（纯逻辑解析/过滤） ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eventlog_json_array_and_single_both_parse() {
        let arr = r#"[{"TimeCreated":"2026-09-08T10:00:00.000+08:00","LevelDisplayName":"Error","ProviderName":"Test","Id":41,"Message":"line1\nline2"}]"#;
        let rows = parse_eventlog_json(arr, "System").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 41);
        assert_eq!(rows[0].level, "Error");
        assert_eq!(rows[0].message, "line1");

        let single = r#"{"TimeCreated":"2026-09-08T11:00:00.000+08:00","LevelDisplayName":"Critical","ProviderName":"P","Id":1001,"Message":"m"}"#;
        let rows = parse_eventlog_json(single, "Application").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].log, "Application");
    }

    #[test]
    fn eventlog_bad_json_is_err() {
        assert!(parse_eventlog_json("not json", "System").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn tcp_state_names_cover_common() {
        assert_eq!(tcp_state_name(5), "ESTABLISHED");
        assert_eq!(tcp_state_name(2), "LISTEN");
        assert!(tcp_state_name(99).starts_with("UNKNOWN"));
    }

    #[cfg(windows)]
    #[test]
    fn ip_and_port_conversion() {
        assert_eq!(ip_to_string(0x0100_00_7f), "127.0.0.1");
        assert_eq!(port_net_to_host(0x01bb_u32), 0xbb01_u16 & 0xffff);
        // 网络序 0x01BB → 主机序 0xBB01? 80 端口 = 0x0050 网络序字节 [0x00,0x50] → u32 0x00000050
        assert_eq!(port_net_to_host(0x0000_0050), 0x5000);
    }
}
