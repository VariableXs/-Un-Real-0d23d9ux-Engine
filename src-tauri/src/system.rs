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
#[tauri::command]
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

/// Open a file with the Windows default application or a folder in Explorer.
#[tauri::command]
pub fn open_path(_st: tauri::State<AppState>, path: String) -> CmdResult<()> {
    let p = validate_open_target(&path)?;
    if p.is_dir() {
        std::process::Command::new("explorer")
            .arg(&p)
            .spawn()
            .map_err(|e| AppError::io(format!("无法打开文件夹 / Cannot open folder: {e}")))?;
    } else {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("cmd")
                .raw_arg("/C")
                .raw_arg(format!("start \"\" \"{}\"", p.to_string_lossy()))
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .spawn()
                .map_err(|e| AppError::io(format!("无法打开文件 / Cannot open file: {e}")))?;
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::process::Command::new("xdg-open").arg(&p).spawn().ok();
        }
    }
    Ok(())
}

/// Reveal a file or folder in Windows Explorer.
#[tauri::command]
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
#[tauri::command]
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

#[tauri::command]
pub fn log_frontend(st: tauri::State<AppState>, level: String, message: String) -> CmdResult<()> {
    AppState::append_log_public(&st.logs_dir, &level, &message);
    Ok(())
}

/// Generic UTF-8 text export to a user-chosen path (never overwrites silently:
/// the frontend uses a save dialog which already confirms replacement intent,
/// but an existing-file guard is still enforced here unless allow_overwrite).
#[tauri::command]
pub fn save_text_file(path: String, contents: String, allow_overwrite: Option<bool>) -> CmdResult<String> {
    let p = Path::new(&path);
    if p.exists() && !allow_overwrite.unwrap_or(false) {
        return Err(AppError::validation("目标文件已存在 / Target exists"));
    }
    std::fs::write(p, contents.as_bytes())
        .map_err(|e| AppError::io(format!("写入失败 / Write failed: {e}")))?;
    Ok(path)
}
