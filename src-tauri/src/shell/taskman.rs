//! L3 shell — taskman.rs（F-3 任务管理器增强后端）
//! - proc_list：进程快照（sysinfo 0.30 差分采样）+ 三色分类
//!   （Variable 家族 = 引擎自身 exe 同目录进程树；嵌入应用并入家族；宿主进程）
//! - proc_kill：结束任务护栏——系统关键进程一律拒绝；宿主进程仅显式 force 可结束
//! - startup_list / startup_disable：启动项（注册表 Run 键）；直跑档只读（如实降级）
//! - service_list / service_set：VM 档 PowerShell Get-Service 列表与启停（管理员提示）
//! - GPU：PDH 未接线，性能页如实标注「暂不可用」（降级路径有文案）

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, System, UpdateKind};

use std::sync::Mutex;
use std::time::Instant;

use crate::error::{AppError, CmdResult};

/// 全局快照缓存（跨调用持有 System → CPU% 可差分；刷新节奏由前端控制）。
static SYS: Mutex<Option<(System, Instant)>> = Mutex::new(None);

fn with_sys<T>(f: impl FnOnce(&mut System) -> T) -> T {
    let mut guard = SYS.lock().unwrap_or_else(|e| e.into_inner());
    let sys = guard.get_or_insert_with(|| (System::new(), Instant::now()));
    f(&mut sys.0)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: Option<u32>,
    pub name: String,
    /// 内存（字节）。
    pub mem: u64,
    /// CPU 百分比（自上次采样差分）。
    pub cpu: f32,
    /// variable = 引擎家族（含嵌入应用宿主）；host = 宿主进程。
    pub family: &'static str,
    /// 系统关键进程（结束任务一律禁用）。
    pub critical: bool,
}

/// Windows 系统关键进程（结束任务一律拒绝）。
const CRITICAL_NAMES: &[&str] = &[
    "system", "idle", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe",
    "services.exe", "lsass.exe", "svchost.exe", "ntoskrnl.exe", "registry",
    "dwm.exe", "fontdrvhost.exe",
];

#[tauri::command]
pub fn proc_list() -> CmdResult<Vec<ProcInfo>> {
    let self_exe = std::env::current_exe().ok();
    let self_dir = self_exe.as_ref().and_then(|p| p.parent()).map(|d| d.to_path_buf());
    let self_pid = std::process::id();

    let procs = with_sys(|sys| {
        sys.refresh_processes_specifics(
            ProcessRefreshKind::new().with_cpu().with_memory().with_exe(UpdateKind::OnlyIfNotSet),
        );
        let mut out: Vec<ProcInfo> = Vec::new();
        for (pid, p) in sys.processes() {
            let name = p.name().to_lowercase();
            let exe = p.exe().map(|e| e.to_path_buf());
            let in_family = pid.as_u32() == self_pid
                || exe
                    .as_ref()
                    .zip(self_dir.as_ref())
                    .map(|(e, dir)| e.parent().map(|pd| pd == dir).unwrap_or(false))
                    .unwrap_or(false)
                || name.contains("variable");
            // 宿主关键进程：仅按名单标注（同为宿主但不在名单可被显式 force 结束）
            let critical = !in_family
                && CRITICAL_NAMES.iter().any(|c| name == *c || name.starts_with(&format!("{c} (")));
            out.push(ProcInfo {
                pid: pid.as_u32(),
                ppid: p.parent().map(|pp| pp.as_u32()),
                name: p.name().to_owned(),
                mem: p.memory(),
                cpu: p.cpu_usage().max(0.0),
                family: if in_family { "variable" } else { "host" },
                critical,
            });
        }
        out.sort_by(|a, b| {
            b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal).then(b.mem.cmp(&a.mem))
        });
        Ok(out)
    });
    procs
}

/// 结束任务（护栏）：系统关键进程一律拒绝；宿主进程需 force=true（用户显式确认）；
/// Variable 家族可直接结束（嵌入应用结束走 VWM 占位卡路径）。
#[tauri::command]
pub fn proc_kill(pid: u32, force: bool) -> CmdResult<String> {
    let procs = with_sys(|sys| {
        sys.refresh_processes_specifics(ProcessRefreshKind::new().with_exe(UpdateKind::OnlyIfNotSet));
        sys.process(Pid::from_u32(pid)).map(|p| (p.name().to_lowercase(), p.exe().map(|e| e.to_path_buf())))
    });
    let Some((name, exe)) = procs else {
        return Err(AppError::validation(format!("进程 {pid} 不存在或已退出")));
    };
    if CRITICAL_NAMES.iter().any(|c| name.starts_with(c)) {
        return Err(AppError::validation(format!("「{name}」为系统关键进程，禁止结束")));
    }
    let self_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let in_family = exe
        .as_ref()
        .zip(self_dir.as_ref())
        .map(|(e, dir)| e.parent().map(|pd| pd == dir).unwrap_or(false))
        .unwrap_or(false)
        || name.contains("variable");
    if !in_family && !force {
        return Err(AppError::validation(format!("「{name}」为宿主进程，需显式确认后才能结束")));
    }
    with_sys(|sys| {
        if let Some(p) = sys.process(Pid::from_u32(pid)) {
            if p.kill() {
                Ok(format!("已结束 {name} ({pid})"))
            } else {
                Err(AppError::io(format!("结束 {name} ({pid}) 失败（权限不足或已退出）")))
            }
        } else {
            Err(AppError::validation(format!("进程 {pid} 不存在或已退出")))
        }
    })
}

/// 性能页：总 CPU + 每核（百分比；前端 1s 轮询，采样成本 sysinfo 内部差分）。
#[tauri::command]
pub fn perf_cpu() -> CmdResult<Vec<f32>> {
    with_sys(|sys| {
        sys.refresh_cpu_usage();
        Ok(sys.cpus().iter().map(|c| c.cpu_usage()).collect())
    })
}

// ---------- 启动项（注册表 Run 键） ----------

/// 序列化/反序列化复用（startup_disable 命令参数需要 Deserialize）。
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    pub name: String,
    pub cmd: String,
    /// HKCU / HKLM。
    pub hive: String,
}

#[cfg(windows)]
fn read_run_key(hive: &str) -> Vec<StartupItem> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ, REG_VALUE_TYPE,
    };
    let root = if hive == "HKCU" { HKEY_CURRENT_USER } else { HKEY_LOCAL_MACHINE };
    let sub: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0".encode_utf16().collect();
    let mut out = Vec::new();
    unsafe {
        let mut key = HKEY::default();
        // windows 0.58：RegOpenKeyExW 返回 WIN32_ERROR（0 = 成功）；ulOptions 为 u32。
        if RegOpenKeyExW(root, PCWSTR(sub.as_ptr()), 0, KEY_READ, &mut key).0 != 0 {
            return out;
        }
        let mut idx = 0u32;
        loop {
            let mut name_buf = [0u16; 16384];
            let mut name_len = name_buf.len() as u32;
            let mut dtype = REG_VALUE_TYPE::default();
            let mut data_buf = [0u8; 4096];
            let mut data_len = data_buf.len() as u32;
            let st = RegEnumValueW(
                key,
                idx,
                windows::core::PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                Some(&mut dtype.0),
                Some(data_buf.as_mut_ptr()),
                Some(&mut data_len),
            );
            if st.0 != 0 || name_len == 0 {
                break;
            }
            let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            let cmd = if dtype.0 == 1 || dtype.0 == 2 {
                // REG_SZ / REG_EXPAND_SZ（UTF-16）
                let s: Vec<u16> = data_buf
                    .chunks_exact(2)
                    .take_while(|c| c[0] != 0)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&s)
            } else {
                String::new()
            };
            out.push(StartupItem { name, cmd, hive: hive.to_string() });
            idx += 1;
        }
        let _ = RegCloseKey(key);
    }
    out
}

#[cfg(not(windows))]
fn read_run_key(_hive: &str) -> Vec<StartupItem> {
    Vec::new()
}

/// 启动项列表（HKCU + HKLM Run）。直跑档仅展示（禁用操作如实报错）。
#[tauri::command]
pub fn startup_list() -> CmdResult<Vec<StartupItem>> {
    let mut out = read_run_key("HKCU");
    out.extend(read_run_key("HKLM"));
    Ok(out)
}

/// 禁用启动项 = 删除注册表值。仅 VM 档可写；直跑档返回只读提示（跳转宿主任务管理器）。
#[tauri::command]
pub fn startup_disable(item: StartupItem) -> CmdResult<()> {
    if crate::shell::sysinfo::runtime_mode() != "vm" {
        return Err(AppError::validation("直跑档启动项为只读：请在宿主任务管理器 → 启动应用 中管理"));
    }
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::System::Registry::{
            RegDeleteValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_SET_VALUE,
        };
        let root = if item.hive == "HKCU" { HKEY_CURRENT_USER } else { HKEY_LOCAL_MACHINE };
        let sub: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0".encode_utf16().collect();
        let name: Vec<u16> = item.name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let mut key = HKEY::default();
            let st = RegOpenKeyExW(root, PCWSTR(sub.as_ptr()), 0, KEY_SET_VALUE, &mut key);
            if st.0 != 0 {
                return Err(AppError::io(format!("打开注册表失败（错误码 {}，权限不足？）", st.0)));
            }
            let st = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
            let _ = windows::Win32::System::Registry::RegCloseKey(key);
            if st.0 != 0 {
                return Err(AppError::io(format!("禁用「{}」失败（错误码 {}）", item.name, st.0)));
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    Err(AppError::validation("仅支持 Windows"))
}

// ---------- 服务页（VM 档 PowerShell） ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceItem {
    pub name: String,
    pub display: String,
    pub status: String,
    pub start_type: String,
}

fn ps_json(script: &str) -> Result<Vec<u8>, AppError> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(0x08000000)
            .output()
            .map_err(|e| AppError::io(format!("PowerShell 调用失败: {e}")))?;
        if !out.status.success() {
            return Err(AppError::io(format!(
                "PowerShell 失败: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(out.stdout)
    }
    #[cfg(not(windows))]
    {
        let _ = script;
        Err(AppError::validation("仅支持 Windows"))
    }
}

/// VM 档服务列表（Get-Service，零网络）。直跑档/PowerShell 不可用时返回空 + 前端文案。
#[tauri::command]
pub fn service_list() -> CmdResult<Vec<ServiceItem>> {
    let raw = ps_json(
        "Get-Service | Select-Object -First 120 Name,DisplayName,Status,StartType | ConvertTo-Json -Compress",
    )
    .map_err(|_| AppError::validation("服务列表不可用（需 VM 档 + PowerShell）"))?;
    parse_services(&raw)
}

fn parse_services(raw: &[u8]) -> CmdResult<Vec<ServiceItem>> {
    // ConvertTo-Json 单对象/数组两形态——用 serde_json（引擎已有依赖）
    #[derive(serde::Deserialize)]
    struct SvcRaw {
        #[serde(default, rename = "Name")]
        name: serde_json::Value,
        #[serde(default, rename = "DisplayName")]
        display: serde_json::Value,
        #[serde(default, rename = "Status")]
        status: serde_json::Value,
        #[serde(default, rename = "StartType")]
        start_type: serde_json::Value,
    }
    fn as_str(v: &serde_json::Value) -> String {
        v.as_str().unwrap_or("").to_string()
    }
    let doc: serde_json::Value =
        serde_json::from_slice(raw).map_err(|e| AppError::io(format!("服务 JSON 解析失败: {e}")))?;
    let list: Vec<SvcRaw> = match doc {
        serde_json::Value::Array(a) => a.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect(),
        v @ serde_json::Value::Object(_) => vec![serde_json::from_value(v).map_err(|e| AppError::io(e.to_string()))?],
        _ => vec![],
    };
    Ok(list
        .into_iter()
        .map(|s| ServiceItem {
            name: as_str(&s.name),
            display: as_str(&s.display),
            status: as_str(&s.status),
            start_type: as_str(&s.start_type),
        })
        .collect())
}

/// 启停服务（VM 档；需要管理员权限，失败如实返回）。
#[tauri::command]
pub fn service_set(name: String, start: bool) -> CmdResult<String> {
    if crate::shell::sysinfo::runtime_mode() != "vm" {
        return Err(AppError::validation("直跑档服务为只读：请在宿主 services.msc 中管理"));
    }
    let verb = if start { "Start-Service" } else { "Stop-Service" };
    let script = format!("{verb} -Name '{}' -ErrorAction Stop", name.replace('\'', "''"));
    ps_json(&script)?;
    Ok(format!("服务 {name} 已{}", if start { "启动" } else { "停止" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_names_guard() {
        // 系统关键进程名单护栏：lsass/svchost 一律命中
        assert!(CRITICAL_NAMES.contains(&"lsass.exe"));
        assert!(CRITICAL_NAMES.iter().any(|c| "svchost.exe".starts_with(c)));
    }

    #[test]
    fn parse_services_single_and_array() {
        let arr = br#"[{"Name":"AudioSrv","DisplayName":"Windows Audio","Status":"Running","StartType":"Automatic"}]"#;
        let l = parse_services(arr).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].name, "AudioSrv");
        assert_eq!(l[0].status, "Running");
        // 单对象形态（ConvertTo-Json 对单条记录不包数组）
        let one = br#"{"Name":"W32Time","DisplayName":"","Status":"Stopped","StartType":"Manual"}"#;
        let l = parse_services(one).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].name, "W32Time");
    }
}
