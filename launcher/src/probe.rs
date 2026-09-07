//! L-1 探测管线：目标 < 3s，结果缓存 probe-cache.json（同一宿主机二次启动跳过重探测）。
//!
//! 探测项（终极形态施工总计划 5.1）：
//! - OS 版本/构建号 + 是否家庭版（注册表 EditionID）
//! - Hyper-V 可用性（CPUID hypervisor bit + vmms 服务探测）
//! - CPU 核数 / 可用内存
//! - GPU（DXGI EnumAdapters）
//! - 显示器数量与分辨率
//!
//! 注意：不做每次启动的介质写测试——伤 U 盘寿命（删减记录 25.4）。

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

/// 探测结果（同时是引擎/设置页可读的 JSON 契约，字段勿随意改名）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeReport {
    pub host_key: String,
    pub os_display: String,
    pub os_build: String,
    pub edition_id: String,
    pub is_home_edition: bool,
    pub hypervisor_present: bool,
    pub hyperv_tools: bool,
    pub cpu_cores: usize,
    pub total_mem_mb: u64,
    pub avail_mem_mb: u64,
    pub gpus: Vec<GpuInfo>,
    pub monitors: Vec<MonitorInfo>,
    pub probed_at_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub description: String,
    pub dedicated_video_mem_mb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub width: i32,
    pub height: i32,
}

impl ProbeReport {
    /// 运行档建议：给 L-2 autoTier 与 L-3 降级链消费
    pub fn suggest_tier(&self) -> &'static str {
        let dgpu = self
            .gpus
            .iter()
            .any(|g| g.dedicated_video_mem_mb >= 4096);
        if self.is_vm_like() {
            // VM 无 GPU 加速（虚拟显卡）
            "B"
        } else if dgpu {
            "S"
        } else if self.total_mem_mb > 8 * 1024 {
            "A"
        } else {
            "B"
        }
    }

    fn is_vm_like(&self) -> bool {
        let d = self.gpus.iter().map(|g| g.description.to_lowercase()).collect::<String>();
        d.contains("hyper-v") || d.contains("vmvga") || d.contains("vbox") || d.contains("virtual")
    }
}

const CACHE_FILE: &str = "probe-cache.json";
const CACHE_TTL: Duration = Duration::from_secs(30 * 24 * 3600); // 30 天，mtime 失效重探

/// 探测入口：命中缓存直接返回（二次启动 < 3s 的关键）
pub fn probe_with_cache() -> Result<ProbeReport, String> {
    let exe_dir = std::env::current_exe()
        .map_err(|e| format!("定位引导器目录失败: {e}"))?
        .parent()
        .ok_or("无父目录")?
        .to_path_buf();
    let cache = exe_dir.join(CACHE_FILE);
    if let Ok(text) = std::fs::read_to_string(&cache) {
        if let Ok(rep) = serde_json::from_str::<ProbeReport>(&text) {
            let age = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                .saturating_sub(rep.probed_at_secs);
            if Duration::from_secs(age) < CACHE_TTL && rep.host_key == host_key() {
                return Ok(rep);
            }
        }
    }
    let rep = probe_fresh()?;
    // 缓存写入失败不致命（只读介质场景）
    if let Ok(json) = serde_json::to_string_pretty(&rep) {
        let _ = std::fs::write(&cache, json);
    }
    Ok(rep)
}

fn host_key() -> String {
    format!(
        "{}|{}|{}",
        std::env::var("COMPUTERNAME").unwrap_or_default(),
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        total_mem_mb()
    )
}

pub fn probe_fresh() -> Result<ProbeReport, String> {
    let (os_display, os_build, edition_id) = os_info();
    let is_home = edition_id.contains("Core")
        && !edition_id.contains("Professional")
        && !edition_id.contains("Server");
    let (total, avail) = mem_info();
    Ok(ProbeReport {
        host_key: host_key(),
        os_display,
        os_build,
        edition_id: edition_id.clone(),
        is_home_edition: is_home,
        hypervisor_present: cpuid_hypervisor_bit(),
        hyperv_tools: hyperv_tools_present(),
        cpu_cores: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        total_mem_mb: total,
        avail_mem_mb: avail,
        gpus: enum_gpus(),
        monitors: enum_monitors(),
        probed_at_secs: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
}

fn os_info() -> (String, String, String) {
    use winreg::enums::*;
    use winreg::RegKey;
    let hk = RegKey::predef(HKEY_LOCAL_MACHINE);
    let cur = hk.open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    match cur {
        Ok(k) => {
            let name: String = k.get_value("ProductName").unwrap_or_else(|_| "Windows".into());
            let build: String = k.get_value("CurrentBuildNumber").unwrap_or_default();
            let edition: String = k.get_value("EditionID").unwrap_or_default();
            (name, build, edition)
        }
        Err(_) => ("Windows".into(), String::new(), String::new()),
    }
}

fn total_mem_mb() -> u64 {
    mem_info().0
}

fn mem_info() -> (u64, u64) {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut ms = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        unsafe {
            let _ = GlobalMemoryStatusEx(&mut ms);
        }
        (ms.ullTotalPhys / 1024 / 1024, ms.ullAvailPhys / 1024 / 1024)
    }
    #[cfg(not(windows))]
    (0, 0)
}

/// CPUID leaf 1 ECX bit 31：hypervisor present（免 WMI，毫秒级）
fn cpuid_hypervisor_bit() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let res = unsafe { std::arch::x86_64::__cpuid(1) };
        (res.ecx >> 31) & 1 == 1
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Hyper-V 管理工具（vmms 服务）探测；失败时保守返回 false（走 L-3 降级链）
fn hyperv_tools_present() -> bool {
    #[cfg(windows)]
    {
        use std::process::Command;
        // sc query 快速探测，≤100ms；避免加载 WMI 模块（重，伤 3s 预算）
        let out = Command::new("sc")
            .args(["query", "vmms"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output();
        matches!(out, Ok(o) if o.status.success())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn enum_gpus() -> Vec<GpuInfo> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_DESC1};
    let mut out = Vec::new();
    unsafe {
        let factory: IDXGIFactory1 = match CreateDXGIFactory1() {
            Ok(f) => f,
            Err(_) => return out,
        };
        for i in 0.. {
            let Ok(adapter) = factory.EnumAdapters1(i) else { break };
            if let Ok(desc) = adapter.GetDesc1() {
                let name = String::from_utf16_lossy(
                    &desc.Description[..desc.Description.iter().position(|&c| c == 0).unwrap_or(0)],
                );
                out.push(GpuInfo {
                    description: name,
                    dedicated_video_mem_mb: (desc.DedicatedVideoMemory / 1024 / 1024) as u32,
                });
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn enum_gpus() -> Vec<GpuInfo> {
    Vec::new()
}

#[cfg(windows)]
fn enum_monitors() -> Vec<MonitorInfo> {
    use windows::Win32::Foundation::{BOOL, LPARAM};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use std::sync::Mutex;

    static LIST: Mutex<Vec<MonitorInfo>> = Mutex::new(Vec::new());
    unsafe extern "system" fn cb(
        _: HMONITOR,
        _: HDC,
        rect: *mut windows::Win32::Foundation::RECT,
        _: LPARAM,
    ) -> BOOL {
        let r = unsafe { &*rect };
        if let Ok(mut v) = LIST.lock() {
            v.push(MonitorInfo {
                width: r.right - r.left,
                height: r.bottom - r.top,
            });
        }
        true.into()
    }
    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(cb),
            LPARAM(0),
        );
    }
    LIST.lock().map(|v| v.clone()).unwrap_or_default()
}

#[cfg(not(windows))]
fn enum_monitors() -> Vec<MonitorInfo> {
    Vec::new()
}

#[cfg(windows)]
use std::os::windows::process::CommandExt as _;
