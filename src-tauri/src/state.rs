use crate::error::{AppError, CmdResult};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Holds one SQLite connection behind a Mutex<Option<_>> so that backup/restore
/// can temporarily close the connection to safely replace the database file.
pub struct AppState {
    pub conn: Mutex<Option<Connection>>,
    pub data_dir: PathBuf,
    pub db_dir: PathBuf,
    pub media_dir: PathBuf,
    pub attachments_dir: PathBuf,
    pub backups_dir: PathBuf,
    pub recovery_dir: PathBuf,
    pub logs_dir: PathBuf,
}

const DB_FILE: &str = "variable.db";

/// Ensure the default workspace root exists (`<dataDir>/Workspace`), matching
/// `workspace::ws_default_dir` so the boot scanner can rely on it.
fn ensure_workspace(base: &Path) -> std::io::Result<()> {
    fs::create_dir_all(base.join("Workspace"))
}

fn portable_base() -> Option<PathBuf> {
    if std::env::var("VARIABLE_PORTABLE").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false) {
        return std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    }
    let exe = std::env::current_exe().ok()?;
    let marker = exe.parent()?.join(".portable");
    if marker.exists() {
        return exe.parent().map(|d| d.to_path_buf());
    }
    None
}

/// 便携模式判定（U 盘完全便携，M8）：`.portable` 标记或 `VARIABLE_PORTABLE=1`。
pub fn is_portable() -> bool {
    portable_base().is_some()
}

impl AppState {
    /// Create all data directories only. The database itself is opened later by
    /// the boot loader so every real loading step can be reported to the UI as
    /// a progress event. Fails loudly with actionable messages when directories
    /// cannot be created.
    pub fn bootstrap() -> CmdResult<AppState> {
        let base = match portable_base() {
            Some(b) => b.join("data"),
            None => dirs_app_data().ok_or_else(|| {
                AppError::io("Cannot resolve application data directory. Check USERPROFILE permissions.")
            })?,
        };
        Self::bootstrap_dirs_at(base)
    }

    /// Directory-only bootstrap: no database is opened (`conn` stays empty).
    pub fn bootstrap_dirs_at(base: PathBuf) -> CmdResult<AppState> {
        let db_dir = base.join("db");
        let media_dir = base.join("media");
        let attachments_dir = base.join("attachments");
        let backups_dir = base.join("backups");
        let recovery_dir = base.join("recovery");
        let logs_dir = base.join("logs");
        for d in [&base, &db_dir, &media_dir, &attachments_dir, &backups_dir, &recovery_dir, &logs_dir] {
            fs::create_dir_all(d).map_err(|e| {
                AppError::io(format!("Failed to create directory {}: {e}", d.display()))
            })?;
        }
        let _ = crate::state::ensure_workspace(&base);
        Ok(AppState {
            conn: Mutex::new(None),
            data_dir: base,
            db_dir,
            media_dir,
            attachments_dir,
            backups_dir,
            recovery_dir,
            logs_dir,
        })
    }

    /// Open the database (if not already open) and run pending migrations.
    /// Idempotent; called by the boot loader and by tests.
    pub fn open_database(&self) -> CmdResult<()> {
        let mut guard = self.conn.lock().map_err(|_| AppError::db("Database mutex poisoned"))?;
        if guard.is_some() {
            return Ok(());
        }
        let conn = crate::db::open_conn(&self.db_dir.join(DB_FILE))?;
        let mut conn = conn;
        crate::db::migrate(&mut conn)?;
        *guard = Some(conn);
        Ok(())
    }

    /// Bootstrap against an explicit base directory (used by tests): directories
    /// plus a fully opened database.
    pub fn bootstrap_at(base: PathBuf) -> CmdResult<AppState> {
        let st = Self::bootstrap_dirs_at(base)?;
        st.open_database()?;
        Ok(st)
    }

    /// Run `f` with the connection while it stays open. All commands funnel
    /// through here; transactions must be created inside `f` only.
    pub fn with_conn<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T, AppError>) -> Result<T, AppError> {
        let mut guard = self.conn.lock().map_err(|_| AppError::db("Database mutex poisoned"))?;
        let conn = guard
            .as_mut()
            .ok_or_else(|| AppError::db("Database connection is closed (restore in progress?)"))?;
        f(conn)
    }

    /// Close the connection, run `f`, then reopen the database.
    pub fn with_conn_closed<T>(&self, f: impl FnOnce() -> Result<T, AppError>) -> Result<T, AppError> {
        let mut guard = self.conn.lock().map_err(|_| AppError::db("Database mutex poisoned"))?;
        let taken = guard.take();
        drop(guard);
        // Release the SQLite handle BEFORE running `f`, otherwise file swaps
        // fail with ERROR_SHARING_VIOLATION (the binding would otherwise keep
        // the connection alive until the end of this function).
        drop(taken);
        let result = f();
        let needs_reopen = self.conn.lock().map_err(|_| AppError::db("Database mutex poisoned"))?.is_none();
        if needs_reopen {
            let conn = crate::db::open_conn(&self.db_dir.join(DB_FILE))?;
            *self.conn.lock().map_err(|_| AppError::db("Database mutex poisoned"))? = Some(conn);
        }
        result
    }
}

#[cfg(target_os = "windows")]
fn dirs_app_data() -> Option<PathBuf> {
    let base = std::env::var("APPDATA").ok()?;
    Some(Path::new(&base).join("com.variable.app"))
}

#[cfg(not(target_os = "windows"))]
fn dirs_app_data() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(Path::new(&home).join(".local/share/com.variable.app"))
}

pub fn append_log(logs_dir: &Path, msg: &str) {
    use std::io::Write;
    let stamp = now_stamp();
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("variable.log"))
    {
        let _ = writeln!(f, "{stamp} {msg}");
    }
}

impl AppState {
    pub fn append_log_public(logs_dir: &Path, level: &str, msg: &str) {
        let clean: String = msg.chars().filter(|c| !c.is_control()).take(2000).collect();
        append_log(logs_dir, &format!("[{level}] {clean}"));
    }
}

pub fn now_stamp() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    format!("[{secs}]")
}
