//! AI-11 系统集成与硬件组 — 电源/色彩/优先级（winpower.rs）。
//!
//! 覆盖：V-54 临时保持唤醒（SetThreadExecutionState，会话级）/ N-20 电源计划
//! 切换与电池健康（powercfg，只读查询 + 显式切换）/ U-46 色彩与时辰（gamma，
//! 退出字节级还原）/ V-57 进程优先级预设（用户级 SetPriorityClass）/
//! N-21 系统代理读写（HKCU）与显式测速 ping。
//!
//! 红线（承 ASCENT AI-3 红线）：gamma/电源类改动退出必须还原宿主状态——
//! 原 gamma ramp 在进程内保存一份，lib.rs RunEvent::Exit 统一调用
//! `restore_on_exit()`（gamma 还原 + 唤醒解除）；keep-awake 为会话级、
//! 进程退出自动失效，不改电源计划。

use serde::Serialize;

// ---------- V-54 临时保持唤醒 ----------

#[derive(Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeepAwakeState {
    pub on: bool,
    /// 是否同时阻止熄屏
    pub display: bool,
}

static KEEP_AWAKE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
// bit0 = on；bit1 = display

/// V-54：开启/解除「保持唤醒」。SetThreadExecutionState 会话级实现，
/// 不改电源计划；到期由前端定时器调用 set(false)，进程退出自动失效。
#[tauri::command]
pub fn keepawake_set(on: bool, display: bool) -> Result<KeepAwakeState, String> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Power::SetThreadExecutionState;
        const ES_CONTINUOUS: u32 = 0x8000_0000;
        const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
        const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;
        let mut flags = ES_CONTINUOUS;
        if on {
            flags |= ES_SYSTEM_REQUIRED;
            if display {
                flags |= ES_DISPLAY_REQUIRED;
            }
        }
        let r = unsafe { SetThreadExecutionState(windows::Win32::System::Power::EXECUTION_STATE(flags)) };
        if r.0 == 0 {
            return Err("SetThreadExecutionState 失败".into());
        }
        let v = if on { if display { 0b11 } else { 0b01 } } else { 0 };
        KEEP_AWAKE.store(v, std::sync::atomic::Ordering::Relaxed);
        Ok(KeepAwakeState { on, display })
    }
    #[cfg(not(windows))]
    {
        let _ = (on, display);
        Err("not-supported".into())
    }
}

#[tauri::command]
pub fn keepawake_get() -> KeepAwakeState {
    let v = KEEP_AWAKE.load(std::sync::atomic::Ordering::Relaxed);
    KeepAwakeState { on: v & 1 != 0, display: v & 2 != 0 }
}

/// 退出还原（lib.rs RunEvent::Exit 调用）：解除唤醒 + 还原 gamma。
pub fn restore_on_exit() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Power::SetThreadExecutionState;
        const ES_CONTINUOUS: u32 = 0x8000_0000;
        unsafe {
            SetThreadExecutionState(windows::Win32::System::Power::EXECUTION_STATE(ES_CONTINUOUS));
        }
        let _ = gamma_restore_internal();
    }
}

// ---------- N-20 电源计划（powercfg） ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PowerScheme {
    pub guid: String,
    pub name: String,
    pub active: bool,
}

#[cfg(windows)]
fn run_powercfg(args: &[&str]) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powercfg")
        .args(args)
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|e| format!("powercfg 启动失败: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// N-20：列出系统电源计划 + 当前激活项（只读）。
#[tauri::command]
pub fn power_schemes_list() -> Result<Vec<PowerScheme>, String> {
    #[cfg(windows)]
    {
        let txt = run_powercfg(&["/list"])?;
        let mut out = Vec::new();
        for line in txt.lines() {
            let line = line.trim();
            if !line.contains("GUID") {
                continue;
            }
            let guid = line
                .split_whitespace()
                .find(|t| t.len() == 36 && t.matches('-').count() == 4)
                .unwrap_or("")
                .to_string();
            if guid.is_empty() {
                continue;
            }
            // 名称：GUID 后剩余部分（中英文格式均含「(名称)」括号段）
            let name = line
                .split('*')
                .last()
                .unwrap_or(line)
                .split('(')
                .nth(1)
                .map(|s| s.trim_end_matches(')').trim().to_string())
                .unwrap_or_else(|| line.to_string());
            out.push(PowerScheme { guid, name, active: line.contains('*') });
        }
        Ok(out)
    }
    #[cfg(not(windows))]
    Err("not-supported".into())
}

/// N-20：切换电源计划（powercfg /setactive，用户显式触发）。
#[tauri::command]
pub fn power_scheme_set(guid: String) -> Result<(), String> {
    if !guid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') || guid.len() != 36 {
        return Err("GUID 格式非法".into());
    }
    #[cfg(windows)]
    return run_powercfg(&["/setactive", &guid]).map(|_| ());
    #[cfg(not(windows))]
    Err("not-supported".into())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BatteryHealth {
    pub design_mwh: Option<u64>,
    pub full_mwh: Option<u64>,
    pub cycle_count: Option<u64>,
    /// 损耗百分比（基于满充/设计值）
    pub wear_pct: Option<u64>,
}

/// N-20：电池健康（powercfg /batteryreport XML 解析；台式机/不支持机型如实报错）。
#[tauri::command]
pub fn battery_health(st: tauri::State<'_, crate::state::AppState>) -> Result<BatteryHealth, String> {
    #[cfg(windows)]
    {
        let cache = st.data_dir.join("cache");
        let _ = std::fs::create_dir_all(&cache);
        let xml_path = cache.join("battery-report.xml");
        let arg = format!("/output:{}", xml_path.display());
        run_powercfg(&["/batteryreport", &arg, "/XML"])?;
        let xml = std::fs::read_to_string(&xml_path).map_err(|e| format!("报告读取失败: {e}"))?;

        let grab = |tag: &str| -> Option<u64> {
            let open = format!("<{tag}>");
            let close = format!("</{tag}>");
            let s = xml.find(&open)? + open.len();
            let e = xml[s..].find(&close)? + s;
            xml[s..e].trim().parse::<u64>().ok()
        };
        let design = grab("DesignCapacity");
        let full = grab("FullChargeCapacity");
        let cycle = grab("CycleCount");
        let wear = match (design, full) {
            (Some(d), Some(f)) if d > 0 => Some((100u64.saturating_sub(f * 100 / d)).min(100)),
            _ => None,
        };
        Ok(BatteryHealth { design_mwh: design, full_mwh: full, cycle_count: cycle, wear_pct: wear })
    }
    #[cfg(not(windows))]
    {
        let _ = st;
        Err("not-supported".into())
    }
}

// ---------- U-46 色彩与时辰（gamma，退出还原） ----------

static GAMMA_ORIG: std::sync::Mutex<Option<[u16; 768]>> = std::sync::Mutex::new(None);

/// kelvin → RGB 近似（Tanner Helland 拟合，色温域 2800–6500K 收敛）。
/// 导出供单测与前端预览曲线使用。
pub fn kelvin_to_rgb(kelvin: u32) -> (f64, f64, f64) {
    let t = kelvin.clamp(2800, 6500) as f64 / 100.0;
    let (r, g, b) = if t <= 66.0 {
        let r = 255.0;
        let g = (99.4708025861 * t.ln() - 161.1195681661).clamp(0.0, 255.0);
        let b = if t >= 19.0 {
            (138.5177312231 * (t - 10.0).ln() - 305.0447927307).clamp(0.0, 255.0)
        } else {
            0.0
        };
        (r, g, b)
    } else {
        let r = (329.698727446 * (t - 60.0).powf(-0.1332047592)).clamp(0.0, 255.0);
        let g = (288.1221695283 * (t - 60.0).powf(-0.0755148492)).clamp(0.0, 255.0);
        let b = 255.0;
        (r, g, b)
    };
    (r / 255.0, g / 255.0, b / 255.0)
}

/// 按色温生成 768 项 gamma ramp（256×R,G,B，值域 0..=65535）。
pub fn build_ramp(kelvin: u32) -> [u16; 768] {
    let (r, g, b) = kelvin_to_rgb(kelvin);
    let mut ramp = [0u16; 768];
    for i in 0..256usize {
        let v = (i as f64 / 255.0 * 65535.0) as u32;
        ramp[i] = (v as f64 * r) as u16;
        ramp[256 + i] = (v as f64 * g) as u16;
        ramp[512 + i] = (v as f64 * b) as u16;
    }
    ramp
}

#[cfg(windows)]
fn gamma_apply(ramp: &[u16; 768]) -> Result<(), String> {
    use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
    use windows::Win32::UI::ColorSystem::SetDeviceGammaRamp;
    unsafe {
        let hdc = GetDC(None);
        let ok = SetDeviceGammaRamp(hdc, ramp.as_ptr() as *const core::ffi::c_void).as_bool();
        let _ = ReleaseDC(None, hdc);
        if ok {
            Ok(())
        } else {
            Err("SetDeviceGammaRamp 失败（显示器/驱动不支持）".into())
        }
    }
}

/// U-46：设置色温（2800–6500K）。首次调用保存宿主原 ramp，退出时还原。
#[tauri::command]
pub fn gamma_set(kelvin: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
        use windows::Win32::UI::ColorSystem::GetDeviceGammaRamp;
        // 保存原始 ramp（仅一次；还原即回到宿主进入前状态）
        {
            let mut orig = GAMMA_ORIG.lock().unwrap_or_else(|e| e.into_inner());
            if orig.is_none() {
                unsafe {
                    let hdc = GetDC(None);
                    let mut buf = [0u16; 768];
                    if GetDeviceGammaRamp(hdc, buf.as_mut_ptr().cast::<core::ffi::c_void>()).as_bool() {
                        *orig = Some(buf);
                    }
                    let _ = ReleaseDC(None, hdc);
                }
            }
        }
        let ramp = build_ramp(kelvin);
        gamma_apply(&ramp)
    }
    #[cfg(not(windows))]
    {
        let _ = kelvin;
        Err("not-supported".into())
    }
}

#[tauri::command]
pub fn gamma_restore() -> Result<(), String> {
    #[cfg(windows)]
    return gamma_restore_internal();
    #[cfg(not(windows))]
    Err("not-supported".into())
}

#[cfg(windows)]
fn gamma_restore_internal() -> Result<(), String> {
    let orig = GAMMA_ORIG.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(ramp) = orig {
        gamma_apply(&ramp)?;
    }
    Ok(())
}

// ---------- V-57 进程优先级预设（用户级 SetPriorityClass） ----------

/// V-57：为指定 PID 设置优先级类（仅用户显式创建的预设生效；失败如实报错不重试）。
/// class ∈ idle | below | normal | above | high（realtime 拒绝——需要权限且危险）。
#[tauri::command]
pub fn proc_priority_set(pid: u32, class: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{
            OpenProcess, SetPriorityClass, PROCESS_SET_INFORMATION,
            ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
            IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
        };
        let cls = match class.as_str() {
            "idle" => IDLE_PRIORITY_CLASS,
            "below" => BELOW_NORMAL_PRIORITY_CLASS,
            "normal" => NORMAL_PRIORITY_CLASS,
            "above" => ABOVE_NORMAL_PRIORITY_CLASS,
            "high" => HIGH_PRIORITY_CLASS,
            "realtime" => return Err("realtime 优先级被拒绝（危险且需特权，不做）".into()),
            _ => return Err(format!("未知优先级 {class}")),
        };
        unsafe {
            let h = OpenProcess(PROCESS_SET_INFORMATION, false, pid)
                .map_err(|e| format!("打开进程 {pid} 失败（权限不足或已退出）: {e}"))?;
            SetPriorityClass(h, cls)
                .map_err(|e| format!("设置优先级失败（pid {pid}）: {e}"))?;
            let _ = windows::Win32::Foundation::CloseHandle(h);
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, class);
        Err("not-supported".into())
    }
}

// ---------- N-21 系统代理（HKCU）与显式测速 ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProxyState {
    pub enabled: bool,
    pub server: String,
    pub override_list: String,
    pub auto_config_url: String,
}

#[cfg(windows)]
fn open_inet_settings() -> Option<winreg::RegKey> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings", KEY_READ | KEY_WRITE)
        .ok()
}

#[tauri::command]
pub fn proxy_get() -> Result<ProxyState, String> {
    #[cfg(windows)]
    {
        let k = open_inet_settings().ok_or("注册表打开失败")?;
        let g = |n: &str| k.get_value::<String, _>(n).unwrap_or_default();
        Ok(ProxyState {
            enabled: k.get_value::<u32, _>("ProxyEnable").unwrap_or(0) != 0,
            server: g("ProxyServer"),
            override_list: g("ProxyOverride"),
            auto_config_url: g("AutoConfigURL"),
        })
    }
    #[cfg(not(windows))]
    Err("not-supported".into())
}

/// N-21：写系统代理（仅 ProxyEnable/ProxyServer/ProxyOverride 三项；
/// 不做流量代理）。enable=false 即关闭系统代理。
#[tauri::command]
pub fn proxy_set(enabled: bool, server: String, override_list: String) -> Result<(), String> {
    if enabled && !server.contains(':') {
        return Err("server 需为 host:port 形式".into());
    }
    #[cfg(windows)]
    {
        let k = open_inet_settings().ok_or("注册表打开失败")?;
        k.set_value("ProxyEnable", &(if enabled { 1u32 } else { 0u32 }))
            .map_err(|e| e.to_string())?;
        if enabled {
            k.set_value("ProxyServer", &server).map_err(|e| e.to_string())?;
            k.set_value("ProxyOverride", &override_list).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (enabled, server, override_list);
        Err("not-supported".into())
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PingResult {
    pub host: String,
    pub avg_ms: Option<f64>,
    pub lost_pct: Option<f64>,
    pub raw_excerpt: String,
}

/// N-21：显式触发的延迟测速（ping ×4 取平均；仅用户点击时调用，绝不后台偷跑）。
#[tauri::command]
pub fn net_ping(host: String) -> Result<PingResult, String> {
    let host = host.trim().to_string();
    if host.is_empty() || host.contains(|c: char| !(c.is_ascii_alphanumeric() || ".-_:".contains(c))) {
        return Err("主机名非法".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("ping")
            .args(["-n", "4", "-w", "2000", &host])
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| format!("ping 启动失败: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        Ok(parse_ping(&text, &host))
    }
    #[cfg(not(windows))]
    Err("not-supported".into())
}

/// 解析 Windows ping 输出（平均延迟 / 丢包率；中英文输出通吃）。
pub fn parse_ping(text: &str, host: &str) -> PingResult {
    let mut avg: Option<f64> = None;
    let mut lost: Option<f64> = None;
    for line in text.lines() {
        let l = line.trim();
        // “平均 = 12ms” / "Minimum = 8ms, Maximum = 20ms, Average = 12ms"
        if l.contains("平均") || l.to_lowercase().contains("average") {
            let nums: Vec<&str> = l
                .split(|c: char| !c.is_ascii_digit() && c != '.')
                .filter(|s| !s.is_empty())
                .collect();
            // 最后一组数字即平均值（最短/最长在前）
            if let Some(last) = nums.last() {
                avg = last.parse::<f64>().ok().or(avg);
            }
        }
        // “(0% 丢失)” / "(50% loss)"
        if l.contains('%') && (l.contains("丢失") || l.to_lowercase().contains("loss")) {
            let before = l.split('%').next().unwrap_or("");
            if let Some(num) = before.split(|c: char| !c.is_ascii_digit()).next_back() {
                if let Ok(v) = num.parse::<f64>() {
                    if (0.0..=100.0).contains(&v) {
                        lost = Some(v);
                    }
                }
            }
        }
    }
    PingResult {
        host: host.to_string(),
        avg_ms: avg,
        lost_pct: lost,
        raw_excerpt: text
            .lines()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kelvin_rgb_monotonic_and_bounded() {
        let (r1, _, b1) = kelvin_to_rgb(2800);
        let (r2, _, b2) = kelvin_to_rgb(6500);
        assert!(r1 >= r2 - 1e-9, "低温红分量应更高");
        assert!(b1 <= b2 + 1e-9, "低温蓝分量应更低");
        for k in [2800u32, 4000, 5000, 6500] {
            let (r, g, b) = kelvin_to_rgb(k);
            assert!(r >= 0.0 && r <= 1.0 && g >= 0.0 && g <= 1.0 && b >= 0.0 && b <= 1.0);
        }
        // 越界输入收敛到边界
        assert_eq!(kelvin_to_rgb(100), kelvin_to_rgb(2800));
    }

    #[test]
    fn ramp_shape() {
        let ramp = build_ramp(3400);
        assert_eq!(ramp.len(), 768);
        // 各通道单调不减
        for ch in 0..3 {
            for i in 1..256 {
                assert!(ramp[ch * 256 + i] >= ramp[ch * 256 + i - 1]);
            }
        }
        // 3400K 时红 > 蓝
        assert!(ramp[255] > ramp[512 + 255]);
    }

    #[test]
    fn ping_parse_zh_and_en() {
        let zh = "\nPing 统计信息:\n    数据包: 已发送 = 4，已接收 = 4，丢失 = 0 (0% 丢失)，\n往返行程的估计时间(以毫秒为单位):\n    最短 = 8ms，最长 = 20ms，平均 = 12ms\n";
        let r = parse_ping(zh, "example.com");
        assert_eq!(r.avg_ms, Some(12.0));
        assert_eq!(r.lost_pct, Some(0.0));

        let en = "\nPing statistics for 1.2.3.4:\n    Packets: Sent = 4, Received = 2, Lost = 2 (50% loss),\nApproximate round trip times in milli-seconds:\n    Minimum = 10ms, Maximum = 30ms, Average = 20ms\n";
        let r = parse_ping(en, "1.2.3.4");
        assert_eq!(r.avg_ms, Some(20.0));
        assert_eq!(r.lost_pct, Some(50.0));
    }

    #[test]
    fn keepawake_state_default_off() {
        let s = keepawake_get();
        assert!(!s.on);
    }
}
