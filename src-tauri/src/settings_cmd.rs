use crate::db::now_ms;
use crate::error::{AppError, CmdResult};
use crate::models::RecoveryEntry;
use crate::state::AppState;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

#[tauri::command]
pub fn get_all_settings(st: tauri::State<AppState>) -> CmdResult<HashMap<String, String>> {
    st.with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT key, value FROM settings").map_err(AppError::from)?;
        let mut map = HashMap::new();
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(AppError::from)?;
        for r in rows {
            let (k, v) = r.map_err(AppError::from)?;
            map.insert(k, v);
        }
        Ok(map)
    })
}

/// Upsert multiple settings in a single transaction.
#[tauri::command]
pub async fn set_settings(st: tauri::State<'_, AppState>, entries: HashMap<String, String>) -> CmdResult<()> {
    if entries.is_empty() {
        return Ok(());
    }
    st.with_conn(|conn| {
        let tx = conn.transaction().map_err(AppError::from)?;
        for (k, v) in &entries {
            if k.len() > 100 || v.len() > 1_000_000 {
                return Err(AppError::validation("设置项过大 / Setting too large"));
            }
            tx.execute(
                "INSERT INTO settings(key,value,version,updated_at) VALUES(?1,?2,1,?3)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value, version=version+1, updated_at=excluded.updated_at",
                params![k, v, now_ms()],
            )
            .map_err(AppError::from)?;
        }
        tx.commit().map_err(AppError::from)?;
        Ok(())
    })
}

const UI_KEYS: &[&str] = &[
    "theme", "language", "perfMode", "launchAnim", "showStatusBar", "editorWidthPct",
    "editorAlign", "fontSize", "lineHeight", "autosaveDelayMs", "uiZoom",
    "reduceMotion", "safeMode", "bgCustom", "mindDefaults", "bgTier",
];

#[tauri::command]
pub fn reset_ui_settings(st: tauri::State<AppState>) -> CmdResult<()> {
    st.with_conn(|conn| {
        conn.execute(
            &format!(
                "DELETE FROM settings WHERE key IN ({}) OR key LIKE 'lastDocId' OR key LIKE 'lastMindmapId'",
                UI_KEYS.iter().map(|_| "?").collect::<Vec<_>>().join(",")
            ),
            rusqlite::params_from_iter(UI_KEYS.iter()),
        )
        .map_err(AppError::from)?;
        Ok(())
    })
}

// ---------- recovery files ----------

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryPayload {
    pub saved_at: i64,
    pub title: String,
    pub content_html: String,
    pub content_text: String,
}

fn recovery_path(st: &AppState, id: &str) -> CmdResult<std::path::PathBuf> {
    if !id.chars().all(|c| c.is_ascii_hexdigit()) || id.len() != 32 {
        return Err(AppError::validation("无效的恢复文件 ID / Invalid recovery id"));
    }
    Ok(st.recovery_dir.join(format!("recovery-{id}.json")))
}

#[tauri::command]
pub fn write_recovery_file(st: tauri::State<AppState>, payload: RecoveryPayload) -> CmdResult<String> {
    let mut hasher: u64 = 0xcbf29ce484222325;
    for b in format!("{}{}", payload.saved_at, payload.title).bytes() {
        hasher ^= b as u64;
        hasher = hasher.wrapping_mul(0x100000001b3);
    }
    let id = format!("{hasher:032x}");
    let p = st.recovery_dir.join(format!("recovery-{id}.json"));
    fs::write(&p, serde_json::to_vec(&payload).map_err(|e| AppError::io(e.to_string()))?)
        .map_err(|e| AppError::io(format!("写入恢复文件失败 / Recovery write failed: {e}")))?;
    Ok(id)
}

fn read_recovery_entry(p: &std::path::Path) -> Option<RecoveryEntry> {
    let raw = fs::read_to_string(p).ok()?;
    let v: RecoveryPayload = serde_json::from_str(&raw).ok()?;
    let name = p.file_stem()?.to_string_lossy().to_string();
    let id = name.strip_prefix("recovery-")?.to_string();
    Some(RecoveryEntry {
        id,
        saved_at: v.saved_at,
        title: if v.title.is_empty() { "(无标题 / Untitled)".into() } else { v.title },
        preview: v.content_text.chars().take(120).collect(),
    })
}

#[tauri::command]
pub fn list_recovery_files(st: tauri::State<AppState>) -> CmdResult<Vec<RecoveryEntry>> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&st.recovery_dir) {
        for e in rd.filter_map(|e| e.ok()) {
            if let Some(entry) = read_recovery_entry(&e.path()) {
                out.push(entry);
            }
        }
    }
    out.sort_by_key(|e| std::cmp::Reverse(e.saved_at));
    Ok(out)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryFileContent {
    pub saved_at: i64,
    pub title: String,
    pub content_html: String,
    pub content_text: String,
}

#[tauri::command]
pub fn read_recovery_file(st: tauri::State<AppState>, id: String) -> CmdResult<RecoveryFileContent> {
    let p = recovery_path(&st, &id)?;
    let raw = fs::read_to_string(&p).map_err(|_| AppError::not_found("恢复文件不存在或已删除 / Recovery file missing"))?;
    serde_json::from_str(&raw).map_err(|e| AppError::validation(format!("恢复文件损坏 / Recovery file corrupted: {e}")))
}

#[tauri::command]
pub fn delete_recovery_file(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    let p = recovery_path(&st, &id)?;
    if p.exists() {
        fs::remove_file(&p).map_err(|e| AppError::io(format!("删除恢复文件失败 / Delete failed: {e}")))?;
    }
    Ok(())
}

/// Turn a recovery file into a real document (never overwrites existing data).
#[tauri::command]
pub fn recover_to_document(st: tauri::State<AppState>, id: String) -> CmdResult<String> {
    let p = recovery_path(&st, &id)?;
    let raw = std::fs::read_to_string(&p)
        .map_err(|_| AppError::not_found("恢复文件不存在或已删除 / Recovery file missing"))?;
    let payload: RecoveryPayload = serde_json::from_str(&raw)
        .map_err(|e| AppError::validation(format!("恢复文件损坏 / Recovery file corrupted: {e}")))?;
    let _ = std::fs::remove_file(&p);
    st.with_conn(move |conn| {
        let doc_id = crate::db::gen_id();
        let now = now_ms();
        conn.execute(
            "INSERT INTO documents(id,folder_id,title,content_html,content_text,favorite,created_at,updated_at,deleted_at)
             VALUES(?1,NULL,?2,?3,?4,0,?5,?5,NULL)",
            params![doc_id, payload.title, payload.content_html, payload.content_text, now],
        )
        .map_err(AppError::from)?;
        Ok(doc_id)
    })
}


