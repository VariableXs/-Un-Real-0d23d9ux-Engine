use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::Serialize;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Built-in workspace folder management: a user-visible directory tree of
/// `.mindmap` / `.json` files with move / copy-in / rename / trash semantics.
/// Every path that targets something INSIDE the workspace is validated against
/// the canonical workspace root (no `..` escapes, no symlink jumps out).
/// `.project` / `.fatetree` archives live alongside `.mindmap` (spec 12.2).
const ALLOWED_EXTS: &[&str] = &["mindmap", "json", "project", "fatetree"];
const BLOCKED_EXTS: &[&str] = &["exe", "bat", "cmd", "com", "scr", "ps1", "vbs", "msi", "jar"];
const MAX_DEPTH: usize = 8;
const TRASH_DIR: &str = ".trash";
const MAX_READ_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsEntry {
    pub name: String,
    pub path: String,
    /// "dir" | "file"
    pub kind: String,
    pub ext: Option<String>,
    pub size: u64,
    pub updated_at: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn ext_of(p: &Path) -> Option<String> {
    p.extension().map(|e| e.to_string_lossy().to_lowercase())
}

/// Strip the Windows extended-length prefix (`\\?\C:\鈥) so paths crossing the
/// IPC boundary stay plain, comparable strings for both sides.
fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

fn is_allowed_ext(p: &Path) -> bool {
    matches!(ext_of(p).as_deref(), Some(e) if ALLOWED_EXTS.contains(&e))
}

fn is_blocked_ext(name: &str) -> bool {
    matches!(ext_of(Path::new(name)).as_deref(), Some(e) if BLOCKED_EXTS.contains(&e))
}

/// Sanitize a single path component (folder / file name).
fn sanitize_component(name: &str) -> CmdResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Err(AppError::validation("鍚嶇О鏃犳晥 / Invalid name"));
    }
    if trimmed.chars().count() > 120 || is_blocked_ext(trimmed) {
        return Err(AppError::validation("鍚嶇О杩囬暱鎴栫被鍨嬭绂佹 / Name too long or blocked type"));
    }
    let cleaned: String = trimmed
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect();
    if cleaned.trim().is_empty() {
        return Err(AppError::validation("鍚嶇О鏃犳晥 / Invalid name"));
    }
    Ok(cleaned)
}

fn canonical_root(root: &str) -> CmdResult<PathBuf> {
    let r = Path::new(root);
    if !r.is_dir() {
        return Err(AppError::not_found(format!("宸ヤ綔鍖虹洰褰曚笉瀛樺湪 / Workspace folder not found: {root}")));
    }
    r.canonicalize().map_err(|e| AppError::io(format!("鏃犳硶瑙ｆ瀽宸ヤ綔鍖虹洰褰?/ Cannot resolve workspace root: {e}")))
}

/// Ensure `target` (which may not exist yet) stays inside the canonical root,
/// even when intermediate components are symlinks. Walks up to the deepest
/// existing ancestor, canonicalizes it and re-appends the lexical remainder.
/// The result is returned in display form (no `\\?\` prefix).
fn ensure_inside(root_canon: &Path, target: &Path) -> CmdResult<PathBuf> {
    let mut ancestor = target.to_path_buf();
    let mut suffix: Vec<OsString> = Vec::new();
    loop {
        match ancestor.canonicalize() {
            Ok(canon) => {
                let mut full = canon;
                for part in suffix.iter().rev() {
                    full.push(part);
                }
                if !full.starts_with(root_canon) {
                    return Err(AppError::validation("璺緞瓒婄晫 / Path escapes the workspace"));
                }
                return Ok(PathBuf::from(display_path(&full)));
            }
            Err(_) => {
                match ancestor.file_name() {
                    Some(name) => {
                        suffix.push(name.to_os_string());
                        if !ancestor.pop() {
                            return Err(AppError::validation("鏃犳晥璺緞 / Invalid path"));
                        }
                    }
                    None => return Err(AppError::validation("鏃犳晥璺緞 / Invalid path")),
                }
            }
        }
    }
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut i = 0u32;
    loop {
        let cand = if i == 0 {
            dir.join(format!("{stem}{ext}"))
        } else {
            dir.join(format!("{stem}-{i}{ext}"))
        };
        if !cand.exists() {
            return cand;
        }
        i += 1;
    }
}

fn entry_from_path(p: &Path, display_dir: &Path) -> Option<WsEntry> {
    let meta = fs::metadata(p).ok()?;
    let name = p.file_name()?.to_string_lossy().to_string();
    let display = display_dir.join(&name);
    if meta.is_dir() {
        return Some(WsEntry {
            name,
            path: display_path(&display),
            kind: "dir".into(),
            ext: None,
            size: 0,
            updated_at: now_ms(),
        });
    }
    // Only surface files the app can open; everything else is invisible noise.
    if !is_allowed_ext(p) {
        return None;
    }
    let updated_at = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Some(WsEntry {
        name,
        path: display_path(&display),
        kind: "file".into(),
        ext: ext_of(p),
        size: meta.len(),
        updated_at,
    })
}

fn walk(dir: &Path, display_dir: &Path, out: &mut Vec<WsEntry>, depth: usize) -> CmdResult<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let rd = fs::read_dir(dir).map_err(|e| AppError::io(format!("璇诲彇鐩綍澶辫触 / Read dir failed: {e}")))?;
    let mut dirs: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut files: Vec<(PathBuf, PathBuf)> = Vec::new();
    for item in rd.flatten() {
        let p = item.path();
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if name == TRASH_DIR || name.starts_with('.') {
            continue;
        }
        let disp = display_dir.join(
            p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        );
        let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            dirs.push((p, disp));
        } else if is_allowed_ext(&p) {
            files.push((p, disp));
        }
    }
    dirs.sort_by_key(|a| a.1.to_string_lossy().to_lowercase());
    files.sort_by_key(|a| a.1.to_string_lossy().to_lowercase());
    for (d, dd) in dirs {
        if let Some(e) = entry_from_path(&d, display_dir) {
            out.push(e);
        }
        walk(&d, &dd, out, depth + 1)?;
    }
    for (f, _fd) in files {
        if let Some(e) = entry_from_path(&f, display_dir) {
            out.push(e);
        }
    }
    Ok(())
}

/// Default (and initial) workspace root: `<dataDir>/Workspace`.
#[tauri::command]
pub fn ws_default_dir(st: tauri::State<AppState>) -> CmdResult<String> {
    let d = st.data_dir.join("Workspace");
    fs::create_dir_all(&d)?;
    Ok(display_path(&d))
}

/// Recursive listing of the workspace tree (dirs first, dot/`.trash` skipped).
#[tauri::command]
pub fn ws_list(_st: tauri::State<AppState>, root: String) -> CmdResult<Vec<WsEntry>> {
    let rc = canonical_root(&root)?;
    let mut out = Vec::new();
    walk(&rc, Path::new(&display_path(&rc)), &mut out, 0)?;
    Ok(out)
}

/// Read a `.mindmap` / `.json` file from anywhere (used for Save/Open flows
/// whose target the user picked outside the workspace too).
#[tauri::command]
pub fn ws_read_text(_st: tauri::State<AppState>, path: String) -> CmdResult<String> {
    let p = Path::new(&path);
    if !p.is_file() {
        return Err(AppError::not_found(format!("鏂囦欢涓嶅瓨鍦?/ File not found: {path}")));
    }
    if !is_allowed_ext(p) {
        return Err(AppError::validation("浠呮敮鎸?.mindmap / .json 鏂囦欢 / Only .mindmap or .json supported"));
    }
    let meta = fs::metadata(p)?;
    if meta.len() > MAX_READ_BYTES {
        return Err(AppError::validation("鏂囦欢杩囧ぇ / File too large"));
    }
    fs::read_to_string(p).map_err(|e| AppError::io(format!("璇诲彇澶辫触 / Read failed: {e}")))
}

#[tauri::command]
pub fn ws_create_dir(st: tauri::State<AppState>, root: String, parent_dir: String, name: String) -> CmdResult<String> {
    let _ = st;
    let rc = canonical_root(&root)?;
    let parent = ensure_inside(&rc, Path::new(&parent_dir))?;
    if !parent.is_dir() {
        return Err(AppError::not_found("鐖舵枃浠跺す涓嶅瓨鍦?/ Parent folder not found"));
    }
    let clean = sanitize_component(&name)?;
    let dest = unique_path(&parent, &clean);
    fs::create_dir_all(&dest)?;
    Ok(display_path(&dest))
}

#[tauri::command]
pub fn ws_rename(st: tauri::State<AppState>, root: String, path: String, new_name: String) -> CmdResult<String> {
    let _ = st;
    let rc = canonical_root(&root)?;
    let src = ensure_inside(&rc, Path::new(&path))?;
    if !src.exists() {
        return Err(AppError::not_found("鐩爣涓嶅瓨鍦?/ Target not found"));
    }
    if src.file_name().map(|n| n.to_string_lossy() == new_name).unwrap_or(false) {
        return Ok(display_path(&src));
    }
    let clean = sanitize_component(&new_name)?;
    let parent = src.parent().ok_or_else(|| AppError::validation("鏃犳晥璺緞 / Invalid path"))?;
    let dest = unique_path(parent, &clean);
    fs::rename(&src, &dest)?;
    Ok(display_path(&dest))
}

/// Move an entry between folders inside the same workspace. Collisions get a
/// numeric suffix instead of overwriting. Moving a directory into its own
/// descendant is rejected.
#[tauri::command]
pub fn ws_move(st: tauri::State<AppState>, root: String, src: String, dest_dir: String) -> CmdResult<String> {
    let _ = st;
    let rc = canonical_root(&root)?;
    let from = ensure_inside(&rc, Path::new(&src))?;
    let to_dir = ensure_inside(&rc, Path::new(&dest_dir))?;
    if !from.exists() {
        return Err(AppError::not_found("婧愪笉瀛樺湪 / Source not found"));
    }
    if !to_dir.is_dir() {
        return Err(AppError::not_found("鐩爣鏂囦欢澶逛笉瀛樺湪 / Destination folder not found"));
    }
    if from.is_dir() && to_dir.starts_with(&from) {
        return Err(AppError::validation("涓嶈兘绉诲姩鍒拌嚜韬唴閮?/ Cannot move into itself"));
    }
    let name = from.file_name().ok_or_else(|| AppError::validation("鏃犳晥璺緞 / Invalid path"))?;
    let dest = unique_path(&to_dir, &name.to_string_lossy());
    fs::rename(&from, &dest)?;
    Ok(display_path(&dest))
}

/// Copy external files (OS drag-in) into a workspace folder. Only allowed
/// extensions are copied; the returned list holds the final absolute paths.
#[tauri::command]
pub fn ws_copy_in(st: tauri::State<AppState>, root: String, paths: Vec<String>, dest_dir: String) -> CmdResult<Vec<String>> {
    let _ = st;
    let rc = canonical_root(&root)?;
    let to_dir = ensure_inside(&rc, Path::new(&dest_dir))?;
    if !to_dir.is_dir() {
        return Err(AppError::not_found("鐩爣鏂囦欢澶逛笉瀛樺湪 / Destination folder not found"));
    }
    let mut copied = Vec::new();
    for raw in paths {
        let src = Path::new(&raw);
        if !src.is_file() || !is_allowed_ext(src) {
            continue;
        }
        let name = src.file_name().ok_or_else(|| AppError::validation("鏃犳晥璺緞 / Invalid path"))?;
        let dest = unique_path(&to_dir, &name.to_string_lossy());
        fs::copy(src, &dest)?;
        copied.push(display_path(&dest));
    }
    Ok(copied)
}

/// Soft-delete: move the entry into `<workspaceRoot>/.trash/<ts>-<name>`.
#[tauri::command]
pub fn ws_delete_trash(st: tauri::State<AppState>, root: String, path: String) -> CmdResult<String> {
    let _ = st;
    let rc = canonical_root(&root)?;
    let target = ensure_inside(&rc, Path::new(&path))?;
    if !target.exists() {
        return Err(AppError::not_found("鐩爣涓嶅瓨鍦?/ Target not found"));
    }
    if rc.file_name().map(|n| n == target.file_name().unwrap_or_default()).unwrap_or(false) {
        return Err(AppError::validation("涓嶈兘鍒犻櫎宸ヤ綔鍖烘牴鐩綍 / Cannot delete the workspace root"));
    }
    let trash = rc.join(TRASH_DIR);
    fs::create_dir_all(&trash)?;
    let name = target.file_name().ok_or_else(|| AppError::validation("鏃犳晥璺緞 / Invalid path"))?;
    let dest = trash.join(format!("{}-{}", now_ms(), name.to_string_lossy()));
    fs::rename(&target, &dest)?;
    Ok(display_path(&dest))
}
