//! L3 shell — sysinfo.rs（批次D，规格 4.4.2 任务栏小组件区）
//! CPU / 内存 / 磁盘容量（sysinfo 本地读取，零网络）。
//! CPU 用法为两次调用间的真实增量（前端 2s 轮询）；首次调用返回 0（无历史窗口）。

use serde::Serialize;
use std::sync::Mutex;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")] // TS Shell.SysBrief 口径（memUsed/memTotal/runtimeMode）
pub struct SysBrief {
    /// 全局 CPU 占用百分比（0-100，两位小数）
    cpu: f64,
    /// 已用内存（字节）
    mem_used: u64,
    /// 总内存（字节）
    mem_total: u64,
    /// L-3 运行档（vm=Hyper-V/VBox 完整 VM；light=轻量直跑；direct=普通安装/便携模式）
    runtime_mode: &'static str,
}

/// L-3 运行档判定：引导器注入 VAR_RUNTIME_MODE 优先；否则 CPUID hypervisor 位。
pub fn runtime_mode() -> &'static str {
    match std::env::var("VAR_RUNTIME_MODE").as_deref() {
        Ok("vm") | Ok("vbox") => "vm",
        Ok("light") => "light",
        _ => {
            #[cfg(target_arch = "x86_64")]
            {
                let r = std::arch::x86_64::__cpuid(1);
                if (r.ecx >> 31) & 1 == 1 {
                    "vm"
                } else {
                    "direct"
                }
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                "direct"
            }
        }
    }
}

#[derive(Serialize)]
pub struct SysDisk {
    /// 盘符（如 "C"）
    letter: String,
    /// 挂载点（如 "C:\"）
    path: String,
    /// 总容量（字节）
    total: u64,
    /// 可用容量（字节）
    free: u64,
}

static SYS: Mutex<Option<sysinfo::System>> = Mutex::new(None);

/// X-3 扩展 API `hardware.status`（hardware:read 权限）：只读硬件摘要，零网络。
pub fn hardware_summary() -> crate::error::CmdResult<serde_json::Value> {
    let b = sys_brief();
    let disks = sys_disks();
    Ok(serde_json::json!({
        "cpuPercent": b.cpu,
        "memUsed": b.mem_used,
        "memTotal": b.mem_total,
        "disks": disks.iter().map(|d| serde_json::json!({
            "mount": d.path, "total": d.total, "free": d.free
        })).collect::<Vec<_>>()
    }))
}

/// 本机用户名（开始菜单底栏显示；仅读环境变量，零网络）。
#[tauri::command(async)]
pub fn sys_user() -> String {
    std::env::var("USERNAME").unwrap_or_else(|_| "User".into())
}

/// 当前联网网卡的 IPv4（批次E，规格 6.1 Wi-Fi 详情；只读，零网络请求——仅枚举本机适配器）。
#[tauri::command(async)]
pub fn net_ip() -> Option<String> {
    #[cfg(windows)]
    {
        use windows::Win32::NetworkManagement::IpHelper::{GetAdaptersAddresses, GET_ADAPTERS_ADDRESSES_FLAGS, IP_ADAPTER_ADDRESSES_LH};
        use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
        use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};
        const NO_ERROR: u32 = 0;
        const ERROR_BUFFER_OVERFLOW: u32 = 122;
        unsafe {
            let mut size: u32 = 0;
            let family = AF_INET.0 as u32;
            // 第一次调用取所需缓冲区大小
            let flags = GET_ADAPTERS_ADDRESSES_FLAGS(0);
            let r = GetAdaptersAddresses(family, flags, None, None, &mut size);
            if !(r == NO_ERROR || r == ERROR_BUFFER_OVERFLOW) || size == 0 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            let adapters = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
            if GetAdaptersAddresses(family, flags, None, Some(adapters), &mut size) != NO_ERROR {
                return None;
            }
            let mut p = adapters;
            while !p.is_null() {
                let a = &*p;
                if a.OperStatus == IfOperStatusUp {
                    let mut ua = a.FirstUnicastAddress;
                    while !ua.is_null() {
                        let sa = (*ua).Address.lpSockaddr;
                        if !sa.is_null() && (*sa).sa_family == AF_INET {
                            let sin = &*(sa as *const SOCKADDR_IN);
                            let b = sin.sin_addr.S_un.S_un_b;
                            return Some(format!("{}.{}.{}.{}", b.s_b1, b.s_b2, b.s_b3, b.s_b4));
                        }
                        ua = (*ua).Next;
                    }
                }
                p = a.Next;
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[tauri::command(async)]
pub fn sys_brief() -> SysBrief {
    let mut guard = SYS.lock().unwrap_or_else(|e| e.into_inner());
    let sys = guard.get_or_insert_with(sysinfo::System::new);
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    let cpu = sys.global_cpu_info().cpu_usage();
    SysBrief {
        cpu: ((cpu.max(0.0) as f64) * 100.0).round() / 100.0,
        mem_used: sys.used_memory(),
        mem_total: sys.total_memory(),
        runtime_mode: runtime_mode(),
    }
}

/// 固定盘 + 可移动盘（本地卷）；排除网络/虚拟挂载与无盘符卷。
#[tauri::command(async)]
pub fn sys_disks() -> Vec<SysDisk> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut out = Vec::new();
    for d in disks.list() {
        let mp = d.mount_point().to_string_lossy().to_string();
        // 仅本地盘符卷：C:\ 形态
        let mut chars = mp.chars();
        let (Some(letter), Some(':')) = (chars.next(), chars.next()) else {
            continue;
        };
        if !letter.is_ascii_alphabetic() {
            continue;
        }
        if !mp[2..].trim_matches('\\').is_empty() {
            continue;
        }
        if d.total_space() == 0 {
            continue;
        }
        out.push(SysDisk {
            letter: letter.to_ascii_uppercase().to_string(),
            path: mp,
            total: d.total_space(),
            free: d.available_space(),
        });
    }
    out.sort_by(|a, b| a.letter.cmp(&b.letter));
    out
}

/// V-3：磁盘磨损/健康读数（硬件面板「运行环境」页按需拉取；PowerShell 一次性调用，
/// 结果由调用方缓存，不做轮询）。无计数器的设备 wear/temp 为 null（诚实上报）。
#[tauri::command(async)]
pub fn sys_disk_health() -> Result<Vec<DiskHealth>, String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "Get-PhysicalDisk | ForEach-Object {{ $r = $_ | Get-StorageReliabilityCounter -ErrorAction SilentlyContinue; [pscustomobject]@{{ name=$($_.FriendlyName); media=$($_.MediaType); bus=$($_.BusType); health=$($_.HealthStatus); wear=$($r.Wear); temp=$($r.Temperature); hours=$($r.PowerOnHours) }} }} | ConvertTo-Json -Compress"
                ),
            ])
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| format!("PowerShell 启动失败: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        #[derive(serde::Deserialize)]
        struct Raw {
            name: Option<String>,
            media: Option<String>,
            bus: Option<String>,
            health: Option<String>,
            wear: Option<f64>,
            temp: Option<f64>,
            hours: Option<f64>,
        }
        let raws: Vec<Raw> = if stdout.trim_start().starts_with('[') {
            serde_json::from_str(stdout.trim()).map_err(|e| e.to_string())?
        } else if stdout.trim_start().starts_with('{') {
            vec![serde_json::from_str(stdout.trim()).map_err(|e| e.to_string())?]
        } else {
            Vec::new()
        };
        Ok(raws
            .into_iter()
            .map(|r| DiskHealth {
                name: r.name.unwrap_or_default(),
                media: r.media.unwrap_or_default(),
                bus: r.bus.unwrap_or_default(),
                health: r.health.unwrap_or_default(),
                wear_pct: r.wear.map(|v| v as u32),
                temp_c: r.temp.map(|v| v as i32),
                power_on_hours: r.hours.map(|v| v as u64),
            })
            .collect())
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskHealth {
    pub name: String,
    pub media: String,
    pub bus: String,
    pub health: String,
    /// 磨损百分比（无计数器 = null，诚实上报）
    pub wear_pct: Option<u32>,
    pub temp_c: Option<i32>,
    pub power_on_hours: Option<u64>,
}
