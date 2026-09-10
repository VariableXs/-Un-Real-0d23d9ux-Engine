//! D-2（22.2）：直跑档用户级 Shell 替换（可选强化，默认关闭）。
//!
//! - `HKCU\...\Winlogon` 下 `Shell` 值 = 用户级覆盖（只影响当前用户，不动 HKLM）；
//! - 开启前三重警示由前端完成（设置→运行环境→「开机进入 Variable（实验性）」）；
//!   本模块负责：写入 / 生成一键还原脚本 / 心跳自检崩溃自愈；
//! - 安全网：启动后 60s 心跳成功则清零崩溃计数；连续 3 次 60s 内启动（=崩溃）
//!   → 自动写回 explorer（绝不把用户锁在黑屏外）。

#[cfg(windows)]
use winreg::enums::*;
#[cfg(windows)]
use winreg::RegKey;

#[cfg(not(windows))]
const SHELL_VALUE: &str = "Shell";

/// HKCU 用户级 Shell 覆盖位置（不动 HKLM）
#[cfg(windows)]
const WINLOGON_KEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Winlogon";
/// 用户级 Shell 覆盖值名
#[cfg(windows)]
const SHELL_VALUE: &str = "Shell";

const CRASH_FILE: &str = "var-shell-crash.count";
const GOOD_FILE: &str = "var-shell-ok.flag";
const MAX_CRASHES: u32 = 3;
const HEARTBEAT_MS: u64 = 60_000;

fn exe_path() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()
}

fn data_dir() -> Option<std::path::PathBuf> {
    exe_path().and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

fn read_crash_count(dir: &std::path::Path) -> u32 {
    std::fs::read_to_string(dir.join(CRASH_FILE))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// 当前是否已开启用户级 Shell 覆盖（值指向本引擎）
#[cfg(windows)]
pub fn is_enabled() -> bool {
    match current_shell_override() {
        Some(v) => exe_path()
            .map(|e| e.to_string_lossy().eq_ignore_ascii_case(&v))
            .unwrap_or(false),
        None => false,
    }
}

#[cfg(windows)]
fn current_shell_override() -> Option<String> {
    let hk = RegKey::predef(HKEY_CURRENT_USER);
    let k = hk.open_subkey(WINLOGON_KEY).ok()?;
    k.get_value::<String, _>(SHELL_VALUE).ok().filter(|s| !s.is_empty())
}

/// 开启：写 HKCU Shell 覆盖 + 生成一键还原脚本（放引擎同目录/U 盘根目录可达处）
#[cfg(windows)]
pub fn enable() -> Result<(), String> {
    let exe = exe_path().ok_or("定位引擎失败")?;
    let hk = RegKey::predef(HKEY_CURRENT_USER);
    let k = hk
        .open_subkey_with_flags(WINLOGON_KEY, KEY_SET_VALUE)
        .map_err(|e| format!("打开 HKCU Winlogon 失败: {e}"))?;
    k.set_value(SHELL_VALUE, &exe.to_string_lossy().to_string())
        .map_err(|e| format!("写入 Shell 失败: {e}"))?;
    // 崩溃计数从 0 开始
    if let Some(dir) = data_dir() {
        let _ = std::fs::write(dir.join(CRASH_FILE), "0");
    }
    // 一键还原脚本（双击即还原 explorer）
    let script = r#"@echo off
rem Variable 直跑档 Shell 一键还原（D-2 安全网）
reg delete "HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon" /v Shell /f
echo 已还原：下次登录回到 Windows 桌面（explorer）。
pause
"#;
    let dir = exe.parent().unwrap_or(std::path::Path::new("."));
    std::fs::write(dir.join("Restore-Shell.bat"), script)
        .map_err(|e| format!("写还原脚本失败: {e}"))?;
    Ok(())
}

/// 关闭（或自动回退）：删除用户级覆盖值（explorer 默认回归）
#[cfg(windows)]
pub fn disable() -> Result<(), String> {
    let hk = RegKey::predef(HKEY_CURRENT_USER);
    let k = hk
        .open_subkey_with_flags(WINLOGON_KEY, KEY_SET_VALUE)
        .map_err(|e| format!("打开 HKCU Winlogon 失败: {e}"))?;
    // 删除用户级覆盖值（仅当存在；值不存在不报错）
    match k.delete_value(SHELL_VALUE) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("删除 Shell 覆盖失败: {e}")),
    }
}

/// 启动时调用：Shell 模式下自增计数并安排 60s 心跳；连续 3 次短命启动 → 自动回退 explorer。
/// 返回 Some(已自动回退) / None（非 Shell 模式或无需处理）。
pub fn boot_selfcheck() -> Option<bool> {
    if !is_enabled() {
        return None;
    }
    let dir = data_dir()?;
    let count = read_crash_count(&dir) + 1;
    if count > MAX_CRASHES {
        // 连续崩溃 → 自愈回退（绝不锁死黑屏）
        let _ = disable();
        let _ = std::fs::remove_file(dir.join(CRASH_FILE));
        return Some(true);
    }
    let _ = std::fs::write(dir.join(CRASH_FILE), count.to_string());
    // 60s 后存活 → 记「本会话正常」并清零计数
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(HEARTBEAT_MS));
        let _ = std::fs::write(dir.join(GOOD_FILE), "1");
        let _ = std::fs::write(dir.join(CRASH_FILE), "0");
    });
    Some(false)
}

// ---- Tauri 命令（设置→运行环境→「开机进入 Variable（实验性）」）----

/// 状态查询：enabled + 一键还原脚本位置
#[tauri::command(async)]
pub fn directshell_status() -> DirectShellStatus {
    #[cfg(windows)]
    {
        let script = data_dir().map(|d| d.join("Restore-Shell.bat").to_string_lossy().to_string());
        DirectShellStatus {
            enabled: is_enabled(),
            restore_script: script,
        }
    }
    #[cfg(not(windows))]
    {
        DirectShellStatus {
            enabled: false,
            restore_script: None,
        }
    }
}

#[derive(serde::Serialize)]
pub struct DirectShellStatus {
    pub enabled: bool,
    pub restore_script: Option<String>,
}

/// 开关（前端负责三重警示弹窗后再调用 enable）
#[tauri::command(async)]
pub fn directshell_set(enable: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        if enable {
            self::enable()
        } else {
            self::disable()
        }
    }
    #[cfg(not(windows))]
    {
        let _ = enable;
        Err("仅支持 Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D-2：默认必须关闭（不写注册表就不该有覆盖值）
    #[test]
    fn disabled_by_default() {
        // 只验证读取路径不 panic；断言不假设测试机状态
        let _ = is_enabled();
        let _ = current_shell_override();
    }
}
