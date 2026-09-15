use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapInfo {
    pub data_dir: String,
    pub db_path: String,
    pub media_dir: String,
    pub backups_dir: String,
    pub version: String,
    pub schema_version: i32,
    pub portable: bool,
}

fn is_portable(_st: &AppState) -> bool {
    std::env::var("VARIABLE_PORTABLE").is_ok()
        || std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(".portable").exists()))
            .unwrap_or(false)
}

/// Called by the frontend right after load. Runs pending migrations (idempotent)
/// and reports environment paths so the UI can show the real data location.
#[tauri::command(async)]
pub fn app_bootstrap(_app: tauri::AppHandle, st: tauri::State<AppState>) -> CmdResult<BootstrapInfo> {
    let mut conn_guard = st.conn.lock().map_err(|_| AppError::db("db mutex"))?;
    let conn = conn_guard
        .as_mut()
        .ok_or_else(|| AppError::db("Database closed"))?;
    crate::db::migrate(conn)?;
    drop(conn_guard);
    Ok(BootstrapInfo {
        data_dir: st.data_dir.to_string_lossy().to_string(),
        db_path: st.db_dir.join("variable.db").to_string_lossy().to_string(),
        media_dir: st.media_dir.to_string_lossy().to_string(),
        backups_dir: st.backups_dir.to_string_lossy().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version: crate::db::SCHEMA_VERSION,
        portable: is_portable(&st),
    })
}

fn validate_open_target(p: &str) -> CmdResult<std::path::PathBuf> {
    let path = Path::new(p);
    if !path.exists() {
        return Err(AppError::not_found(format!("路径不存在 / Path not found: {p}")));
    }
    if path.is_file() {
        // Block executing binaries from within the app.
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        const BLOCKED: &[&str] = &["exe", "bat", "cmd", "com", "scr", "ps1", "vbs", "msi", "jar"];
        if BLOCKED.contains(&ext.as_str()) {
            return Err(AppError::validation(format!("不允许打开可执行文件 / Blocked file type: .{ext}")));
        }
    }
    Ok(path.to_path_buf())
}

/// Steam 产物识别：`steam://` 链接本体，或内容指向 `steam://` 的 `.url`
/// 快捷方式（Steam 桌面快捷键即此格式）。命中 → 返回协议 URL。
/// 供 open_path 把 Steam 产物路由进 Variable 收编通道（不在宿主桌面打开）。
fn steam_probe(path: &str) -> Option<String> {
    let lower = path.trim().to_lowercase();
    if lower.starts_with("steam://") {
        return Some(path.trim().to_string());
    }
    if lower.ends_with(".url") {
        let content = std::fs::read_to_string(path).ok()?;
        let line = content
            .lines()
            .find(|l| l.trim().to_lowercase().starts_with("url="))?;
        let url = line.trim()[4..].trim().trim_matches('"').trim();
        if url.to_lowercase().starts_with("steam://") {
            return Some(url.to_string());
        }
    }
    None
}

/// Open a file with the Windows default application or a folder in Explorer.
/// Steam 产物（steam:// 链接 / Steam 快捷方式 .url）例外：不走宿主默认程序，
/// 改走 Steam 通道（CEF 兼容态 + 收编看护），Steam 主窗与游戏窗由
/// spawn_steam_adopt_watcher / WinEventHook 收进 Variable 桌面运行。
#[tauri::command(async)]
pub fn open_path(app: tauri::AppHandle, _st: tauri::State<AppState>, path: String) -> CmdResult<()> {
    if let Some(url) = steam_probe(&path) {
        crate::shell::ecosystem::steam_open_url(&app, &url)?;
        return Ok(());
    }
    let p = validate_open_target(&path)?;
    // One Shell boundary for both folders and files.  Do not use `cmd /C
    // start`: it loses the caller's environment and turns a path into shell
    // syntax.  ShellExecuteExW lets Windows resolve associations, .lnk,
    // AppX, and folder navigation exactly as Explorer does.
    crate::shell::compat::shell_execute_path(&p, Some("open"), None, p.parent(), None)?;
    Ok(())
}

/// Reveal a file or folder in Windows Explorer.
#[tauri::command(async)]
pub fn reveal_path(_st: tauri::State<AppState>, path: String) -> CmdResult<()> {
    let p = validate_open_target(&path)?;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let target = if p.is_file() {
            format!("/select,\"{}\"", p.to_string_lossy())
        } else {
            format!("\"{}\"", p.to_string_lossy())
        };
        std::process::Command::new("explorer")
            .raw_arg(&target)
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|e| AppError::io(format!("无法打开资源管理器 / Cannot open Explorer: {e}")))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open").arg(&p).spawn().ok();
    }
    Ok(())
}

#[derive(Serialize)]
pub struct PathCheck {
    pub path: String,
    pub exists: bool,
    pub kind: Option<String>,
}

/// Batch existence check used for custom background images/videos and
/// referenced (not copied) media.
#[tauri::command(async)]
pub fn check_paths_exist(_st: tauri::State<AppState>, paths: Vec<String>) -> CmdResult<Vec<PathCheck>> {
    Ok(paths
        .into_iter()
        .map(|p| {
            let meta = std::fs::metadata(Path::new(&p));
            match meta {
                Ok(m) => PathCheck {
                    exists: true,
                    kind: Some(if m.is_dir() { "dir".into() } else { "file".into() }),
                    path: p,
                },
                Err(_) => PathCheck { exists: false, kind: None, path: p },
            }
        })
        .collect())
}

/// 剥控制字符 + 截断（log_frontend 与 applog 总线共用；消息永不反向影响调用方）。
fn sanitize_log_message(msg: &str) -> String {
    msg.chars().filter(|c| !c.is_control()).take(1000).collect()
}

#[tauri::command(async)]
pub fn log_frontend(st: tauri::State<AppState>, level: String, message: String) -> CmdResult<()> {
    // 实时总线：前端异常（logError / ErrorBoundary）推送 sys://applog，
    // 任务管理器「日志」页与 applog-*.log 同步可见（tag = fe:<level>）。
    let clean = sanitize_log_message(&message);
    // level 只保留字母数字，防止拼进 tag 的字符失控（level 来自前端）
    let level_tag: String = level.chars().filter(|c| c.is_ascii_alphanumeric()).take(16).collect();
    crate::shell::applog::log(&format!("fe:{level_tag}"), &clean);
    // 跨会话文件落盘（variable.log；既有链路保持不变）
    AppState::append_log_public(&st.logs_dir, &level, &message);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{sanitize_log_message, steam_probe};

    #[test]
    fn test_sanitize_log_message_strips_control_and_truncates() {
        assert_eq!(sanitize_log_message("plain message"), "plain message");
        assert_eq!(sanitize_log_message("line1\nline2\ttab"), "line1line2tab");
        let long = "x".repeat(1500);
        assert_eq!(sanitize_log_message(&long).chars().count(), 1000);
        assert_eq!(sanitize_log_message(""), "");
    }

    /// Steam 产物识别：steam:// 链接本体（大小写/首尾空白不敏感）直接命中；
    /// Steam 桌面快捷键 .url（URL=steam://…，带/不带引号）解析出协议 URL；
    /// 普通 http .url、其它路径、不存在的 .url 一律不命中。
    #[test]
    fn test_steam_probe_detects_protocol_and_url_shortcut() {
        assert_eq!(steam_probe("steam://rungameid/730"), Some("steam://rungameid/730".into()));
        assert_eq!(steam_probe("  STEAM://Store/ "), Some("STEAM://Store/".into()));

        let dir = std::env::temp_dir().join(format!("varix_steam_probe_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shortcut = dir.join("steam_game.url");
        std::fs::write(
            &shortcut,
            "[InternetShortcut]\r\nURL=steam://rungameid/570\r\n",
        )
        .unwrap();
        assert_eq!(
            steam_probe(shortcut.to_str().unwrap()),
            Some("steam://rungameid/570".into())
        );

        // 带引号的 URL 值（部分工具写出格式）
        let quoted = dir.join("steam_quoted.url");
        std::fs::write(&quoted, "[InternetShortcut]\r\nURL=\"steam://open/main\"\r\n").unwrap();
        assert_eq!(
            steam_probe(quoted.to_str().unwrap()),
            Some("steam://open/main".into())
        );

        // 普通 http .url → 不命中
        let web = dir.join("web.url");
        std::fs::write(&web, "[InternetShortcut]\r\nURL=https://example.com\r\n").unwrap();
        assert_eq!(steam_probe(web.to_str().unwrap()), None);

        // 非 .url 路径 / 不存在的 .url → 不命中
        assert_eq!(steam_probe("C:\\notepad.txt"), None);
        assert_eq!(steam_probe(dir.join("missing.url").to_str().unwrap()), None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Generic UTF-8 text export to a user-chosen path (never overwrites silently:
/// the frontend uses a save dialog which already confirms replacement intent,
/// but an existing-file guard is still enforced here unless allow_overwrite).
#[tauri::command(async)]
pub fn save_text_file(path: String, contents: String, allow_overwrite: Option<bool>) -> CmdResult<String> {
    let p = Path::new(&path);
    if p.exists() && !allow_overwrite.unwrap_or(false) {
        return Err(AppError::validation("目标文件已存在 / Target exists"));
    }
    std::fs::write(p, contents.as_bytes())
        .map_err(|e| AppError::io(format!("写入失败 / Write failed: {e}")))?;
    Ok(path)
}
