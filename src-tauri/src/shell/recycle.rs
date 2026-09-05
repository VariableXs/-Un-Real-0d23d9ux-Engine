//! L3 shell — recycle.rs（M6）
//! 全局回收站：聚合 Variable 内全部"可还原的删除"于一个列表：
//! - "doc" / "folder" / "mindmap" — Write 数据库软删除（deleted_at）
//! - "ws-file" — Write 工作区 `<dataDir>/Workspace/.trash/` 的文件
//! - "fs-item" — 文件管理器删除的任意文件/文件夹（移入 `<dataDir>/recycle/`，
//!   元数据 JSON 记录原始路径，可还原回原位）
//! 全部本机操作，零网络。清理是真实的磁盘/数据库删除，走确认流程。

use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const RECYCLE_DIR: &str = "recycle";
pub const WS_TRASH_DIR: &str = ".trash";

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

fn recycle_dir(st: &AppState) -> PathBuf {
    st.data_dir.join(RECYCLE_DIR)
}

fn ws_trash_dir(st: &AppState) -> PathBuf {
    st.data_dir.join("Workspace").join(WS_TRASH_DIR)
}

/// 统一回收站条目（聚合视图）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecItem {
    pub id: String,
    /// "doc" | "folder" | "mindmap" | "ws-file" | "fs-item"
    pub source: String,
    pub title: String,
    /// 还原目标（fs-item / ws-file 有意义；数据库条目为空）
    pub origin: Option<String>,
    /// ms；数据库条目 = deleted_at，文件条目 = 移入时间
    pub deleted_at: u64,
    /// "file" | "dir" | "doc" | "folder" | "mindmap"
    pub kind: String,
    pub size: u64,
}

/// fs-item 元数据（持久化为 `<id>.meta.json`）。
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecMeta {
    pub id: String,
    pub original_path: String,
    pub name: String,
    /// "file" | "dir"
    pub kind: String,
    pub deleted_at: u64,
    pub size: u64,
}

/// 把任意文件/文件夹移入全局回收站，返回条目 id。由 explorer::ex_trash 调用。
pub fn intern_path(st: &AppState, target: &Path) -> CmdResult<String> {
    let rdir = recycle_dir(st);
    fs::create_dir_all(&rdir)?;
    let name = target
        .file_name()
        .ok_or_else(|| AppError::validation("无效路径 / Invalid path"))?
        .to_string_lossy()
        .to_string();
    let meta = fs::metadata(target).map_err(|e| AppError::io(format!("读取失败 / Stat failed: {e}")))?;
    let kind = if meta.is_dir() { "dir" } else { "file" };
    let size = if meta.is_file() { meta.len() } else { 0 };
    let id = format!("{}-{}", now_ms(), name);
    let dest = unique_dir(&rdir, &id);
    move_into(target, &dest)?;
    let meta = RecMeta {
        id: dest
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| id.clone()),
        original_path: display_path(target),
        name,
        kind: kind.into(),
        deleted_at: now_ms(),
        size,
    };
    let meta_path = rdir.join(format!("{}.meta.json", meta.id));
    fs::write(&meta_path, serde_json::to_vec(&meta).map_err(|e| AppError::io(e.to_string()))?)
        .map_err(|e| AppError::io(format!("写入元数据失败 / Write meta failed: {e}")))?;
    Ok(meta.id)
}

fn unique_dir(dir: &Path, name: &str) -> PathBuf {
    let mut cand = dir.join(name);
    let mut i = 0u32;
    while cand.exists() {
        i += 1;
        cand = dir.join(format!("{name}-{i}"));
    }
    cand
}

/// 跨盘安全的移动（rename 失败 → copy + 删除源）。
fn move_into(src: &Path, dest: &Path) -> CmdResult<()> {
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(17) => {
            copy_recursive(src, dest, 0)?;
            if src.is_dir() {
                fs::remove_dir_all(src)?;
            } else {
                fs::remove_file(src)?;
            }
            Ok(())
        }
        Err(e) => Err(AppError::io(format!("移动失败 / Move failed: {e}"))),
    }
}

/// 公开包装（批次E 便携化复制安装目录用）。
pub fn copy_recursive_pub(src: &Path, dest: &Path) -> CmdResult<u64> {
    copy_recursive(src, dest, 0)
}

fn copy_recursive(src: &Path, dest: &Path, depth: u8) -> CmdResult<u64> {
    if depth > 12 {
        return Err(AppError::validation("目录层级过深 / Directory nesting too deep"));
    }
    let meta = fs::symlink_metadata(src).map_err(|e| AppError::io(format!("读取失败 / Stat failed: {e}")))?;
    if meta.is_dir() {
        fs::create_dir_all(dest)?;
        let mut total = 0u64;
        for item in fs::read_dir(src)?.flatten() {
            total += copy_recursive(&item.path(), &dest.join(item.file_name()), depth + 1)?;
        }
        Ok(total)
    } else if meta.is_file() {
        Ok(fs::copy(src, dest)?)
    } else {
        Ok(0)
    }
}

/// 聚合列表：数据库软删除 + 工作区 .trash + fs recycle，按删除时间倒序。
#[tauri::command]
pub fn rec_list(st: tauri::State<AppState>) -> CmdResult<Vec<RecItem>> {
    rec_list_core(&st)
}

/// 内部实现（后端模块直接用 &AppState 调用）。
pub fn rec_list_core(st: &AppState) -> CmdResult<Vec<RecItem>> {
    let mut out: Vec<RecItem> = Vec::new();
    {
        let conn_guard = st.conn.lock().map_err(|_| AppError::db("db mutex"))?;
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| AppError::db("Database closed"))?;
        let mut q = |sql: &str, source: &str, kind: &str| -> CmdResult<()> {
            let mut stmt = conn.prepare(sql).map_err(|e| AppError::db(e.to_string()))?;
            let rows = stmt
                .query_map([], |r| {
                    let id: String = r.get(0)?;
                    let title: String = r.get(1)?;
                    let deleted_at: Option<i64> = r.get(2)?;
                    Ok(RecItem {
                        id,
                        source: source.into(),
                        title,
                        origin: None,
                        deleted_at: deleted_at.unwrap_or(0) as u64,
                        kind: kind.into(),
                        size: 0,
                    })
                })
                .map_err(|e| AppError::db(e.to_string()))?;
            for row in rows.flatten() {
                out.push(row);
            }
            Ok(())
        };
        q("SELECT id, title, deleted_at FROM documents WHERE deleted_at IS NOT NULL", "doc", "doc")?;
        q("SELECT id, name, deleted_at FROM folders WHERE deleted_at IS NOT NULL", "folder", "folder")?;
        q("SELECT id, name, deleted_at FROM mindmaps WHERE deleted_at IS NOT NULL", "mindmap", "mindmap")?;
    }

    // Write 工作区 .trash（文件名 = `<ts>-<name>`）
    let wt = ws_trash_dir(&st);
    if let Ok(rd) = fs::read_dir(&wt) {
        for item in rd.flatten() {
            let name = item.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let (ts, title) = match name.split_once('-') {
                Some((t, rest)) => (t.parse::<u64>().unwrap_or(0), rest.to_string()),
                None => (0, name.clone()),
            };
            let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
            out.push(RecItem {
                id: name,
                source: "ws-file".into(),
                title,
                origin: Some(display_path(&st.data_dir.join("Workspace"))),
                deleted_at: ts,
                kind: if is_dir { "dir" } else { "file" }.into(),
                size: 0,
            });
        }
    }

    // fs recycle（*.meta.json）
    let rdir = recycle_dir(&st);
    if let Ok(rd) = fs::read_dir(&rdir) {
        for item in rd.flatten() {
            let fname = item.file_name().to_string_lossy().to_string();
            if !fname.ends_with(".meta.json") {
                continue;
            }
            let Ok(bytes) = fs::read(item.path()) else { continue };
            let Ok(meta) = serde_json::from_slice::<RecMeta>(&bytes) else { continue };
            out.push(RecItem {
                id: meta.id,
                source: "fs-item".into(),
                title: meta.name,
                origin: Some(meta.original_path),
                deleted_at: meta.deleted_at,
                kind: meta.kind,
                size: meta.size,
            });
        }
    }

    out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
    Ok(out)
}

/// 还原：按来源分派。fs-item 还原回原始路径（父目录不存在则重建，重名自动加后缀）。
#[tauri::command]
pub fn rec_restore(st: tauri::State<AppState>, id: String, source: String) -> CmdResult<()> {
    match source.as_str() {
        "doc" => crate::library::restore_document(st, id)?,
        "folder" => crate::library::restore_folder(st, id)?,
        "mindmap" => {
            use rusqlite::params;
            let conn_guard = st.conn.lock().map_err(|_| AppError::db("db mutex"))?;
            let conn = conn_guard
                .as_ref()
                .ok_or_else(|| AppError::db("Database closed"))?;
            let n = conn
                .execute(
                    "UPDATE mindmaps SET deleted_at = NULL, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NOT NULL",
                    params![now_ms() as i64, id],
                )
                .map_err(|e| AppError::db(e.to_string()))?;
            if n == 0 {
                return Err(AppError::not_found("条目不存在或已还原 / Item not found or already restored"));
            }
        }
        "ws-file" => {
            let src = ws_trash_dir(&st).join(&id);
            if !src.exists() {
                return Err(AppError::not_found("条目不存在 / Item not found"));
            }
            let title = id.split_once('-').map(|(_, r)| r.to_string()).unwrap_or_else(|| id.clone());
            let ws_root = st.data_dir.join("Workspace");
            fs::create_dir_all(&ws_root)?;
            let dest = unique_in(&ws_root, &title);
            move_into(&src, &dest)?;
        }
        "fs-item" => {
            let rdir = recycle_dir(&st);
            let meta_path = rdir.join(format!("{id}.meta.json"));
            let bytes = fs::read(&meta_path)
                .map_err(|_| AppError::not_found("元数据缺失 / Metadata missing"))?;
            let meta: RecMeta = serde_json::from_slice(&bytes)
                .map_err(|e| AppError::validation(format!("元数据损坏 / Metadata corrupted: {e}")))?;
            let src = rdir.join(&id);
            if !src.exists() {
                return Err(AppError::not_found("条目不存在 / Item not found"));
            }
            let original = PathBuf::from(&meta.original_path);
            if let Some(parent) = original.parent() {
                fs::create_dir_all(parent)?;
            }
            let dest = if original.exists() { unique_in(original.parent().unwrap_or(Path::new("/")), &meta.name) } else { original };
            move_into(&src, &dest)?;
            let _ = fs::remove_file(&meta_path);
        }
        other => return Err(AppError::validation(format!("未知来源 / Unknown source: {other}"))),
    }
    Ok(())
}

/// 同目录内唯一名（重名自动加后缀）。
fn unique_in(dir: &Path, name: &str) -> PathBuf {
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

/// 彻底删除单条（真实删除，前端走确认流程）。
#[tauri::command]
pub async fn rec_purge(st: tauri::State<'_, AppState>, id: String, source: String) -> CmdResult<()> {
    match source.as_str() {
        "doc" => crate::library::purge_documents(st, vec![id]).await?,
        "folder" => crate::library::purge_folder(st, id)?,
        "mindmap" => {
            use rusqlite::params;
            let mut conn_guard = st.conn.lock().map_err(|_| AppError::db("db mutex"))?;
            let conn = conn_guard
                .as_mut()
                .ok_or_else(|| AppError::db("Database closed"))?;
            conn.execute("DELETE FROM edges WHERE mindmap_id = ?1", params![id])
                .map_err(|e| AppError::db(e.to_string()))?;
            conn.execute("DELETE FROM nodes WHERE mindmap_id = ?1", params![id])
                .map_err(|e| AppError::db(e.to_string()))?;
            conn.execute("DELETE FROM mindmaps WHERE id = ?1", params![id])
                .map_err(|e| AppError::db(e.to_string()))?;
        }
        "ws-file" => {
            let src = ws_trash_dir(&st).join(&id);
            if src.is_dir() {
                fs::remove_dir_all(&src)?;
            } else if src.is_file() {
                fs::remove_file(&src)?;
            }
        }
        "fs-item" => {
            let rdir = recycle_dir(&st);
            let src = rdir.join(&id);
            if src.is_dir() {
                fs::remove_dir_all(&src)?;
            } else if src.is_file() {
                fs::remove_file(&src)?;
            }
            let _ = fs::remove_file(rdir.join(format!("{id}.meta.json")));
        }
        other => return Err(AppError::validation(format!("未知来源 / Unknown source: {other}"))),
    }
    Ok(())
}

/// 清空回收站（全部来源）。返回清理的条目数。
#[tauri::command]
pub async fn rec_empty(st: tauri::State<'_, AppState>) -> CmdResult<u32> {
    let mut count = crate::library::empty_trash(st.clone()).await?;

    // ws .trash 全部清除
    let wt = ws_trash_dir(&st);
    if let Ok(rd) = fs::read_dir(&wt) {
        for item in rd.flatten() {
            let name = item.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let p = item.path();
            let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let ok = if is_dir { fs::remove_dir_all(&p).is_ok() } else { fs::remove_file(&p).is_ok() };
            if ok {
                count += 1;
            }
        }
    }

    // fs recycle 全部清除（含元数据）
    let rdir = recycle_dir(&st);
    if let Ok(rd) = fs::read_dir(&rdir) {
        for item in rd.flatten() {
            let p = item.path();
            let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let ok = if is_dir { fs::remove_dir_all(&p).is_ok() } else { fs::remove_file(&p).is_ok() };
            if ok {
                count += 1;
            }
        }
    }
    Ok(count)
}

/// 回收站条目数（桌面图标徽标/占用提示用，轻量查询）。
#[tauri::command]
pub fn rec_count(st: tauri::State<AppState>) -> CmdResult<u32> {
    rec_count_inner(&st)
}

/// 内部计数入口（privacy_audit 等后端模块直接用 &AppState 调用）。
pub fn rec_count_inner(st: &AppState) -> CmdResult<u32> {
    Ok(rec_list_core(st)?.len() as u32)
}
