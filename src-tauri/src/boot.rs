//! Boot loader: performs the REAL application loading sequence and reports
//! every real step to the frontend through the `boot://event` channel.
//!
//! Hard rules (see docs/ARCHITECTURE_V2.md §5):
//! - Progress reflects only real work; there is no timeline, no sleep, no
//!   synthetic events, ever.
//! - Log text is the actual task being performed, including real file paths.
//! - The total duration is whatever the real work takes.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

use crate::error::AppError;
use crate::state::AppState;

pub const BOOT_EVENT: &str = "boot://event";

/// Replay buffer: events emitted before the webview attaches its listener can
/// be fetched via the `boot_replay` command, so no real progress is ever lost
/// and the frontend never needs to fake-fill the bar.
static REPLAY: Mutex<Vec<LoadEvent>> = Mutex::new(Vec::new());
static SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BootStats {
    pub folders: i64,
    pub records: i64,
    pub mindmaps: i64,
    pub nodes: i64,
    pub edges: i64,
    pub media_files: i64,
    pub attachments: i64,
    pub workspace_files: u64,
    pub workspace_folders: u64,
    pub workspace_bytes: u64,
    pub media_dir_files: u64,
    pub media_dir_bytes: u64,
    pub backups: u64,
    pub schema_version: i32,
    pub version: String,
    pub portable: bool,
    pub data_dir: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoadEvent {
    /// Monotonic sequence number for replay deduplication.
    pub seq: u64,
    /// Real cumulative progress 0.0–1.0, advanced only by completed work.
    pub progress: f32,
    /// The real task currently being performed.
    pub current_task: String,
    /// Real file being processed, when applicable.
    pub file_path: Option<String>,
    /// Files processed so far (running count), when applicable.
    pub file_count: Option<u64>,
    /// Total files when known up-front.
    pub total_count: Option<u64>,
    /// Task-type icon: 📦🔍📚🧠🖼💽🔐✓⚠❌
    pub icon: &'static str,
    /// 0 = info, 1 = warn (skipped/recoverable), 2 = error.
    pub level: u8,
    /// Real elapsed time since boot start.
    pub elapsed_ms: u64,
    pub timestamp: u64,
    /// Real summary, present only on the final `ready` event.
    pub stats: Option<BootStats>,
}

struct BootEmit<'a> {
    app: &'a AppHandle,
    t0: Instant,
}

impl BootEmit<'_> {
    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn send(
        &self,
        progress: f32,
        current_task: &str,
        icon: &'static str,
        level: u8,
        file_path: Option<String>,
        file_count: Option<u64>,
        total_count: Option<u64>,
        stats: Option<BootStats>,
    ) {
        let ev = LoadEvent {
            seq: SEQ.fetch_add(1, Ordering::Relaxed),
            progress,
            current_task: current_task.to_string(),
            file_path,
            file_count,
            total_count,
            icon,
            level,
            elapsed_ms: self.t0.elapsed().as_millis() as u64,
            timestamp: Self::now_ms(),
            stats,
        };
        if let Ok(mut buf) = REPLAY.lock() {
            if buf.len() >= 512 {
                buf.clear(); // keep memory bounded; the tail is what matters
            }
            buf.push(ev.clone());
        }
        let _ = self.app.emit(BOOT_EVENT, ev);
    }
}

/// Fetch every boot event emitted so far (replay for late-attaching listeners).
#[tauri::command]
pub fn boot_replay() -> Vec<LoadEvent> {
    REPLAY.lock().map(|g| g.clone()).unwrap_or_default()
}

/// Spawn the boot loader thread. Called from `setup` right after `AppState`
/// (directories) has been managed; the database is opened inside the thread so
/// that the real open + migrate work becomes observable progress.
pub fn spawn_boot_loader(app: AppHandle) {
    std::thread::Builder::new()
        .name("boot-loader".into())
        .spawn(move || run_boot(app))
        .expect("spawn boot loader");
}

fn run_boot(app: AppHandle) {
    let t0 = Instant::now();
    let em = BootEmit { app: &app, t0 };
    let st = app.state::<AppState>();
    let mut warns: Vec<String> = Vec::new();

    // ---- Phase 1 (5%): verify data directory structure ---------------------
    em.send(0.0, "校验数据目录结构 / Verifying data directories", "📦", 0, None, None, None, None);
    let dirs: [(&str, &PathBuf); 6] = [
        ("db", &st.db_dir),
        ("media", &st.media_dir),
        ("attachments", &st.attachments_dir),
        ("backups", &st.backups_dir),
        ("recovery", &st.recovery_dir),
        ("logs", &st.logs_dir),
    ];
    for (name, path) in dirs {
        let ok = path.is_dir();
        if !ok {
            warns.push(format!("directory missing: {}", path.display()));
            em.send(
                0.05,
                &format!("目录缺失，已重建 / Rebuilt missing directory: data/{name}"),
                "⚠",
                1,
                Some(path.to_string_lossy().to_string()),
                None,
                None,
                None,
            );
            let _ = fs::create_dir_all(path);
        }
    }
    em.send(0.05, "数据目录就绪 / Data directories ready", "✓", 0, None, None, None, None);

    // ---- Phase 2 (10%): open database + migrations --------------------------
    let db_path = st.db_dir.join("variable.db");
    em.send(
        0.05,
        &format!("打开数据库 / Opening database: {}", db_path.display()),
        "📦",
        0,
        Some(db_path.to_string_lossy().to_string()),
        None,
        None,
        None,
    );
    let db_open = st.open_database();
    let mut db_ready = false;
    match &db_open {
        Ok(()) => {
            db_ready = true;
            em.send(0.15, "数据库就绪 (WAL) / Database ready", "✓", 0, None, None, None, None);
        }
        Err(e) => {
            warns.push(format!("database open failed: {e}"));
            em.send(
                0.15,
                &format!("数据库打开失败，尝试恢复 / Database failed, attempting recovery: {e}"),
                "❌",
                2,
                Some(db_path.to_string_lossy().to_string()),
                None,
                None,
                None,
            );
            // M8 拔出保护兜底：意外拔出/文件损坏 → 从最新备份自动恢复
            if let Some(bak) = newest_backup(&st.backups_dir) {
                em.send(
                    0.15,
                    &format!("发现备份，自动恢复 / Restoring from backup: {}", bak.display()),
                    "🔐",
                    1,
                    Some(bak.to_string_lossy().to_string()),
                    None,
                    None,
                    None,
                );
                let _ = fs::remove_file(db_path.with_extension("db-wal"));
                let _ = fs::remove_file(db_path.with_extension("db-shm"));
                match fs::copy(&bak, &db_path) {
                    Ok(_) => match st.open_database() {
                        Ok(()) => {
                            db_ready = true;
                            warns.push(format!("restored from backup: {}", bak.display()));
                            em.send(
                                0.15,
                                "备份恢复成功 / Restored from backup",
                                "✓",
                                1,
                                None,
                                None,
                                None,
                                None,
                            );
                        }
                        Err(e2) => {
                            warns.push(format!("restore open failed: {e2}"));
                            em.send(
                                0.15,
                                &format!("恢复后仍无法打开 / Still failing after restore: {e2}"),
                                "❌",
                                2,
                                None,
                                None,
                                None,
                                None,
                            );
                        }
                    },
                    Err(e2) => {
                        warns.push(format!("restore copy failed: {e2}"));
                        em.send(
                            0.15,
                            &format!("备份复制失败 / Backup copy failed: {e2}"),
                            "❌",
                            2,
                            None,
                            None,
                            None,
                            None,
                        );
                    }
                }
            }
        }
    }

    // ---- Phase 3 (25%): real table row counts -------------------------------
    let mut stats = BootStats {
        folders: 0,
        records: 0,
        mindmaps: 0,
        nodes: 0,
        edges: 0,
        media_files: 0,
        attachments: 0,
        workspace_files: 0,
        workspace_folders: 0,
        workspace_bytes: 0,
        media_dir_files: 0,
        media_dir_bytes: 0,
        backups: 0,
        schema_version: crate::db::SCHEMA_VERSION,
        version: env!("CARGO_PKG_VERSION").to_string(),
        portable: is_portable(),
        data_dir: st.data_dir.to_string_lossy().to_string(),
    };
    const TABLES: &[(&str, &str)] = &[
        ("folders", "📚"),
        ("documents", "📚"),
        ("mindmaps", "🧠"),
        ("nodes", "🧠"),
        ("edges", "🧠"),
        ("media", "🖼"),
        ("attachments", "🖼"),
        ("settings", "📦"),
        ("backups", "🔐"),
    ];
    if db_ready {
        let per = 0.25 / TABLES.len() as f32;
        let mut p = 0.15;
        for (table, icon) in TABLES {
            let count = st.with_conn(|c| {
                c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_, i64>(0))
                    .map_err(AppError::from)
            });
            match count {
                Ok(n) => {
                    match *table {
                        "folders" => stats.folders = n,
                        "documents" => stats.records = n,
                        "mindmaps" => stats.mindmaps = n,
                        "nodes" => stats.nodes = n,
                        "edges" => stats.edges = n,
                        "media" => stats.media_files = n,
                        "attachments" => stats.attachments = n,
                        _ => {}
                    }
                    p += per;
                    em.send(
                        p,
                        &format!("索引 {table}: {n} 行 / indexing {table} ({n} rows)"),
                        icon,
                        0,
                        None,
                        None,
                        None,
                        None,
                    );
                }
                Err(e) => {
                    warns.push(format!("table {table}: {e}"));
                    em.send(
                        p + per,
                        &format!("跳过表 {table}: {e} / skipped table {table}"),
                        "⚠",
                        1,
                        None,
                        None,
                        None,
                        None,
                    );
                }
            }
        }
    } else {
        em.send(0.40, "数据库不可用，跳过表统计 / Skipping table stats", "⚠", 1, None, None, None, None);
    }

    // ---- Phase 4 (30%): real workspace tree scan ----------------------------
    let ws_root = st.data_dir.join("Workspace");
    em.send(
        0.40,
        &format!("🔍 扫描工作区 / Scanning workspace: {}", ws_root.display()),
        "🔍",
        0,
        Some(ws_root.to_string_lossy().to_string()),
        Some(0),
        None,
        None,
    );
    let mut scan = Scan { files: 0, folders: 0, bytes: 0 };
    scan_dir(&ws_root, &em, 0.40, 0.70, &mut scan, 0);
    stats.workspace_files = scan.files;
    stats.workspace_folders = scan.folders;
    stats.workspace_bytes = scan.bytes;
    em.send(
        0.70,
        &format!(
            "工作区扫描完成 / Workspace scanned: {} folders, {} files",
            scan.folders, scan.files
        ),
        "✓",
        0,
        None,
        Some(scan.files),
        Some(scan.files),
        None,
    );

    // ---- Phase 5 (20%): real media library index ----------------------------
    em.send(0.70, "🖼 索引媒体库 / Indexing media library", "🖼", 0, None, Some(0), None, None);
    let mut media_scan = Scan { files: 0, folders: 0, bytes: 0 };
    scan_dir_quiet(&st.media_dir, &mut media_scan, 0);
    scan_dir_quiet(&st.attachments_dir, &mut media_scan, 0);
    stats.media_dir_files = media_scan.files;
    stats.media_dir_bytes = media_scan.bytes;
    em.send(
        0.90,
        &format!(
            "媒体库就绪 / Media ready: {} files ({})",
            media_scan.files,
            human_bytes(media_scan.bytes)
        ),
        "✓",
        0,
        None,
        Some(media_scan.files),
        Some(media_scan.files),
        None,
    );

    // ---- Phase 6 (5%): real backup inventory --------------------------------
    em.send(0.90, "🔐 清点备份 / Checking backups", "🔐", 0, None, None, None, None);
    let backups = count_backups(&st.backups_dir);
    stats.backups = backups;
    em.send(
        0.95,
        &format!("备份清点完成 / {} backup(s) available", backups),
        "✓",
        0,
        None,
        None,
        None,
        None,
    );

    // ---- Ready (100%) with the real summary ---------------------------------
    if !warns.is_empty() {
        em.send(1.0, &format!("启动包含警告 / {} warning(s)", warns.len()), "⚠", 1, None, None, None, None);
    }
    em.send(1.0, "✓ ready", "✓", 0, None, None, None, Some(stats));
    crate::state::append_log(&st.logs_dir, &format!("boot complete in {}ms", t0.elapsed().as_millis()));
}

struct Scan {
    files: u64,
    folders: u64,
    bytes: u64,
}

/// Recursive workspace scan that reports intermediate progress every 50 files
/// with the real path of the last file found. `.trash` and dot-directories are
/// skipped, matching `workspace::ws_list` behavior.
fn scan_dir(dir: &Path, em: &BootEmit, p_from: f32, p_to: f32, scan: &mut Scan, depth: u32) {
    if depth > 12 {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            scan.folders += 1;
            scan_dir(&path, em, p_from, p_to, scan, depth + 1);
        } else if meta.is_file() {
            scan.files += 1;
            scan.bytes += meta.len();
            if scan.files % 50 == 0 {
                let p = p_from + (p_to - p_from) * 0.5; // intermediate: halfway inside the phase
                em.send(
                    p,
                    &format!("🔍 已发现 {} 个文件 / {} files found", scan.files, scan.files),
                    "🔍",
                    0,
                    Some(path.to_string_lossy().to_string()),
                    Some(scan.files),
                    None,
                    None,
                );
            }
        }
    }
}

/// Quiet variant used for media/attachment directories (summary event only).
fn scan_dir_quiet(dir: &Path, scan: &mut Scan, depth: u32) {
    if depth > 8 {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            scan_dir_quiet(&path, scan, depth + 1);
        } else if meta.is_file() {
            scan.files += 1;
            scan.bytes += meta.len();
        }
    }
}

fn count_backups(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|x| x.eq_ignore_ascii_case("db"))
                        .unwrap_or(false)
                })
                .count() as u64
        })
        .unwrap_or(0)
}

fn is_portable() -> bool {
    crate::state::is_portable()
}

/// 最新数据库备份（拔出保护兜底恢复用）。
fn newest_backup(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|i| i.path().extension().map(|e| e == "db").unwrap_or(false))
        .max_by_key(|i| i.metadata().and_then(|m| m.modified()).ok())
        .map(|i| i.path())
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}
