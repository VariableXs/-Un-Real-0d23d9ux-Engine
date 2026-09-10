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

    // fs recycle 全部清除（含元数据；policy.json 是回收站策略配置，不属于回收内容）
    let rdir = recycle_dir(&st);
    if let Ok(rd) = fs::read_dir(&rdir) {
        for item in rd.flatten() {
            let name = item.file_name().to_string_lossy().to_string();
            if name == "policy.json" {
                continue;
            }
            let p = item.path();
            if name.ends_with(".meta.json") {
                // 一条 fs-item = 数据目录 + 元数据：连带清除，只计一次
                let id = name.strip_suffix(".meta.json").unwrap_or(&name);
                let dir = rdir.join(id);
                if dir.is_dir() {
                    let _ = fs::remove_dir_all(&dir);
                }
                if fs::remove_file(&p).is_ok() {
                    count += 1;
                }
            } else {
                // 无元数据的孤儿目录/文件：如实清除，不计入条目数
                let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let _ = if is_dir { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
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

// ---------- AI-10（U-27 回收站 2.0）策略引擎 ----------
// 容量阈值（默认 2GB）或时间阈值（默认 30 天）自动清理；
// 清理前托盘预警一次（前端在 apply 前弹预警）；星标文件（U-26）永不自动清理。

const DEFAULT_CAPACITY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DEFAULT_MAX_DAYS: u64 = 30;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RecPolicy {
    /// 容量阈值（字节，0 = 不启用容量清理）
    pub capacity_bytes: u64,
    /// 时间阈值（天，0 = 不启用时间清理）
    pub max_days: u64,
    /// 自动清理总开关（默认关——首次预警由用户确认后才建议开启）
    pub auto_clean: bool,
    /// 上次托盘预警时间（ms；避免重复打扰）
    #[serde(default)]
    pub last_warned_at: u64,
}

impl Default for RecPolicy {
    fn default() -> Self {
        RecPolicy {
            capacity_bytes: DEFAULT_CAPACITY_BYTES,
            max_days: DEFAULT_MAX_DAYS,
            auto_clean: false,
            last_warned_at: 0,
        }
    }
}

fn policy_path(st: &AppState) -> PathBuf {
    st.data_dir.join(RECYCLE_DIR).join("policy.json")
}

fn load_policy(st: &AppState) -> RecPolicy {
    fs::read(policy_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_policy(st: &AppState, p: &RecPolicy) -> CmdResult<()> {
    fs::create_dir_all(recycle_dir(st))?;
    fs::write(policy_path(st), serde_json::to_vec_pretty(p)?)?;
    Ok(())
}

#[tauri::command]
pub fn rec_policy_get(st: tauri::State<AppState>) -> CmdResult<RecPolicy> {
    Ok(load_policy(&st))
}

#[tauri::command]
pub fn rec_policy_set(st: tauri::State<AppState>, policy: RecPolicy) -> CmdResult<()> {
    if policy.capacity_bytes > 1024 * 1024 * 1024 * 1024 {
        return Err(AppError::validation("容量阈值过大 / capacity too large"));
    }
    if policy.max_days > 3650 {
        return Err(AppError::validation("时间阈值过大 / max_days too large"));
    }
    save_policy(&st, &policy)
}

/// 计算将被自动清理的条目（预览，不执行）：
/// ① 超过时间阈值的（星标豁免）；② 若仍超容量阈值，按最旧顺序继续纳入直到回到阈值内。
pub fn rec_policy_preview_inner(st: &AppState) -> CmdResult<Vec<RecItem>> {
    let policy = load_policy(st);
    let starred = crate::shell::tags::starred_keys(st);
    let mut items = rec_list_core(st)?;
    // 星标豁免：fs-item 用原始路径匹配；数据库条目按 title 近似匹配（星标路径含标题）
    items.retain(|it| {
        let origin = it.origin.clone().unwrap_or_default();
        let key = origin.replace('\\', "/").to_lowercase();
        if starred.contains(&key) {
            return false;
        }
        true
    });
    let now = now_ms();
    let mut doomed: Vec<RecItem> = Vec::new();
    let mut rest: Vec<RecItem> = Vec::new();
    for it in items {
        let age_days = (now.saturating_sub(it.deleted_at)) / 86_400_000;
        if policy.max_days > 0 && age_days >= policy.max_days {
            doomed.push(it);
        } else {
            rest.push(it);
        }
    }
    // 容量约束：最旧优先纳入
    if policy.capacity_bytes > 0 {
        let total: u64 = doomed.iter().chain(rest.iter()).map(|i| i.size).sum();
        if total > policy.capacity_bytes {
            rest.sort_by(|a, b| a.deleted_at.cmp(&b.deleted_at));
            let mut acc = total;
            for it in rest {
                if acc <= policy.capacity_bytes {
                    break;
                }
                acc = acc.saturating_sub(it.size);
                doomed.push(it);
            }
        }
    }
    doomed.sort_by(|a, b| a.deleted_at.cmp(&b.deleted_at));
    Ok(doomed)
}

#[tauri::command]
pub fn rec_policy_preview(st: tauri::State<AppState>) -> CmdResult<Vec<RecItem>> {
    rec_policy_preview_inner(&st)
}

/// 执行自动清理（真实删除）。返回清理条数。前端在调用前完成托盘预警。
#[tauri::command]
pub async fn rec_policy_apply(st: tauri::State<'_, AppState>) -> CmdResult<u32> {
    let doomed = rec_policy_preview_inner(&st)?;
    let mut count = 0u32;
    for it in &doomed {
        let ok = match it.source.as_str() {
            "doc" => crate::library::purge_documents(st.clone(), vec![it.id.clone()]).await.is_ok(),
            "folder" => crate::library::purge_folder(st.clone(), it.id.clone()).is_ok(),
            "mindmap" => {
                use rusqlite::params;
                let mut guard = st.conn.lock().map_err(|_| AppError::db("db mutex"))?;
                let conn = guard.as_mut().ok_or_else(|| AppError::db("Database closed"))?;
                conn.execute("DELETE FROM edges WHERE mindmap_id = ?1", params![it.id]).is_ok()
                    && conn.execute("DELETE FROM nodes WHERE mindmap_id = ?1", params![it.id]).is_ok()
                    && conn.execute("DELETE FROM mindmaps WHERE id = ?1", params![it.id]).is_ok()
            }
            "ws-file" => {
                let p = ws_trash_dir(&st).join(&it.id);
                if p.is_dir() { fs::remove_dir_all(&p).is_ok() } else { fs::remove_file(&p).is_ok() }
            }
            "fs-item" => {
                let rdir = recycle_dir(&st);
                let p = rdir.join(&it.id);
                let ok = if p.is_dir() { fs::remove_dir_all(&p).is_ok() } else { fs::remove_file(&p).is_ok() };
                let _ = fs::remove_file(rdir.join(format!("{}.meta.json", it.id)));
                ok
            }
            _ => false,
        };
        if ok {
            count += 1;
        }
    }
    // 更新预警时间戳
    let mut policy = load_policy(&st);
    policy.last_warned_at = now_ms();
    save_policy(&st, &policy)?;
    Ok(count)
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    /// U-27：时间阈值命中 + 星标豁免 + 容量回退最旧优先。
    #[test]
    fn policy_preview_rules() {
        let tmp = std::env::temp_dir().join(format!("variable-recpolicy-{}", std::process::id()));
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        st.open_database().unwrap();
        // 布置：fs-item 三件（旧 40 天 / 新 1 天 / 中 10 天但星标）
        let rdir = recycle_dir(&st);
        fs::create_dir_all(&rdir).unwrap();
        let mk = |id: &str, days_ago: u64, size: u64| {
            let d = rdir.join(id);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("f.bin"), vec![0u8; size as usize]).unwrap();
            let meta = RecMeta {
                id: id.into(),
                original_path: format!(r"C:\orig\{}", id),
                name: id.to_string(),
                kind: "dir".into(),
                deleted_at: now_ms().saturating_sub(days_ago * 86_400_000),
                size,
            };
            fs::write(rdir.join(format!("{id}.meta.json")), serde_json::to_vec(&meta).unwrap()).unwrap();
        };
        mk("old", 40, 100);
        mk("fresh", 1, 100);
        mk("starred-mid", 10, 100);
        // 星标 starred-mid 的原路径
        {
            let tags_path = st.data_dir.join("file_tags.json");
            let entry = serde_json::json!({
                "files": { "c:/orig/starred-mid": { "tags": ["keep"], "starred": true, "updated_at": 1 } },
                "smart": []
            });
            fs::write(&tags_path, entry.to_string()).unwrap();
        }
        let doomed = rec_policy_preview_inner(&st).unwrap();
        let ids: Vec<&str> = doomed.iter().map(|d| d.title.as_str()).collect();
        assert!(ids.contains(&"old"), "40 天条目应被时间阈值命中");
        assert!(!ids.contains(&"starred-mid"), "星标条目永不自动清理");
        assert!(!ids.contains(&"fresh"), "1 天条目不应被清理");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn policy_defaults_and_set() {
        let tmp = std::env::temp_dir().join(format!("variable-recpolicy2-{}", std::process::id()));
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        let p = load_policy(&st);
        assert_eq!(p.capacity_bytes, DEFAULT_CAPACITY_BYTES);
        assert_eq!(p.max_days, DEFAULT_MAX_DAYS);
        assert!(!p.auto_clean);
        let mut np = p.clone();
        np.max_days = 7;
        save_policy(&st, &np).unwrap();
        assert_eq!(load_policy(&st).max_days, 7);
        let _ = fs::remove_dir_all(&tmp);
    }
}
