use crate::db::{gen_id, now_ms};
use crate::error::{AppError, CmdResult};
use crate::models::BackupInfo;
use crate::state::AppState;
use rusqlite::{params, Connection};
use std::fs;
use std::path::Path;

fn db_file(st: &AppState) -> std::path::PathBuf {
    st.db_dir.join("variable.db")
}

fn stamp_name() -> String {
    // Local time derived from epoch millis; naming only needs uniqueness.
    let ms = now_ms();
    let secs = ms / 1000;
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let (y, mo, d) = civil_from_days(days as i64);
    format!("variable-backup-{y:04}{mo:02}{d:02}-{h:02}{mi:02}{ss:02}", h = tod / 3600, mi = (tod % 3600) / 60, ss = tod % 60)
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[tauri::command]
pub async fn create_backup(st: tauri::State<'_, AppState>, source: Option<String>) -> CmdResult<BackupInfo> {
    st.with_conn(|conn| {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").map_err(AppError::from)?;
        Ok(())
    })?;
    let src = db_file(&st);
    if !src.exists() {
        return Err(AppError::not_found("数据库文件不存在 / Database file missing"));
    }
    let name = format!("{}.db", stamp_name());
    let dest = st.backups_dir.join(&name);
    fs::copy(&src, &dest).map_err(|e| AppError::io(format!("备份失败 / Backup failed: {e}")))?;
    let meta = fs::metadata(&dest).map_err(AppError::from)?;
    let (checksum, _) = crate::media::checksum_file_public(&dest);
    let info = BackupInfo {
        id: gen_id(),
        file_name: name,
        source: source.unwrap_or_else(|| "manual".into()),
        size: meta.len() as i64,
        checksum,
        status: "ok".into(),
        created_at: now_ms(),
    };
    st.with_conn(|conn| {
        conn.execute(
            "INSERT INTO backups(id,file_name,source,size,checksum,status,created_at) VALUES(?1,?2,?3,?4,?5,'ok',?6)",
            params![info.id, info.file_name, info.source, info.size, info.checksum, info.created_at],
        )
        .map_err(AppError::from)?;
        Ok(())
    })?;
    Ok(info)
}

#[tauri::command]
pub fn list_backups(st: tauri::State<AppState>) -> CmdResult<Vec<BackupInfo>> {
    st.with_conn(|conn| {
        let mut stmt = conn
            .prepare("SELECT id,file_name,source,size,checksum,status,created_at FROM backups ORDER BY created_at DESC")
            .map_err(AppError::from)?;
        let mut out: Vec<BackupInfo> = stmt
            .query_map([], |r| {
                Ok(BackupInfo {
                    id: r.get(0)?,
                    file_name: r.get(1)?,
                    source: r.get(2)?,
                    size: r.get(3)?,
                    checksum: r.get(4)?,
                    status: r.get(5)?,
                    created_at: r.get(6)?,
                })
            })
            .map_err(AppError::from)?
            .filter_map(|r| r.ok())
            .collect();
        for b in out.iter_mut() {
            let p = st.backups_dir.join(&b.file_name);
            b.status = if p.exists() { "ok".into() } else { "missing".into() };
        }
        Ok(out)
    })
}

fn safe_backup_path(st: &AppState, file_name: &str) -> CmdResult<std::path::PathBuf> {
    // Defense against traversal: only plain names inside backups dir.
    let p = Path::new(file_name);
    let plain_ok = p.file_name().map(|f| f.to_string_lossy()) == Some(file_name.into());
    if !plain_ok || file_name.contains("..") || file_name.contains('/') || file_name.contains('\\') {
        return Err(AppError::validation("非法备份文件名 / Invalid backup file name"));
    }
    let full = st.backups_dir.join(p);
    if !full.exists() {
        return Err(AppError::not_found("备份文件不存在 / Backup file missing"));
    }
    Ok(full)
}

#[tauri::command]
pub fn delete_backup(st: tauri::State<AppState>, file_name: String) -> CmdResult<()> {
    let p = safe_backup_path(&st, &file_name)?;
    fs::remove_file(&p).map_err(|e| AppError::io(format!("删除备份失败 / Delete failed: {e}")))?;
    st.with_conn(|conn| {
        conn.execute("DELETE FROM backups WHERE file_name=?1", params![file_name])
            .map_err(AppError::from)?;
        Ok(())
    })
}

#[tauri::command]
pub fn export_backup(st: tauri::State<AppState>, file_name: String, dest_path: String) -> CmdResult<String> {
    let src = safe_backup_path(&st, &file_name)?;
    let dest = Path::new(&dest_path);
    if dest.exists() {
        return Err(AppError::validation("目标文件已存在，请选择其他位置 / Target exists"));
    }
    fs::copy(&src, dest).map_err(|e| AppError::io(format!("导出备份失败 / Export failed: {e}")))?;
    Ok(dest_path)
}

/// Validate then atomically swap the database file with the backup copy.
#[tauri::command]
pub async fn restore_backup(st: tauri::State<'_, AppState>, file_name: String) -> CmdResult<()> {
    let backup = safe_backup_path(&st, &file_name)?;
    // Integrity gate BEFORE touching live data.
    {
        let check = Connection::open(&backup).map_err(AppError::from)?
            .query_row("PRAGMA quick_check;", [], |r| r.get::<_, String>(0))
            .map_err(AppError::from)?;
        if check != "ok" {
            return Err(AppError::validation(format!("备份校验失败（{}）/ Backup integrity check failed", check)));
        }
    }
    // Flush WAL so the main file is complete before swapping.
    st.with_conn(|conn| conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").map_err(AppError::from))?;
    let db_path = db_file(&st);
    let tmp = st.db_dir.join(".restore-old.db");
    let _ = fs::remove_file(&tmp);
    let _ = fs::remove_file(db_path.with_extension("db-wal"));
    let _ = fs::remove_file(db_path.with_extension("db-shm"));
    let result = st.with_conn_closed(|| -> CmdResult<()> {
        fs::rename(&db_path, &tmp)
            .map_err(|e| AppError::io(format!("无法暂存当前数据库 / Cannot stage current db: {e}")))?;
        match fs::copy(&backup, &db_path) {
            Ok(_) => {
                let _ = fs::remove_file(&tmp);
                Ok(())
            }
            Err(e) => {
                // Roll the previous database back.
                let _ = fs::rename(&tmp, &db_path);
                Err(AppError::io(format!("恢复失败，已保留当前数据 / Restore failed, current data kept: {e}")))
            }
        }
    });
    result
}



