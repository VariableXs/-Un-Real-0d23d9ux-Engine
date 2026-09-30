use crate::error::{AppError, CmdResult};
use rusqlite::Connection;
use std::path::Path;

/// SQL migrations, applied in order. `PRAGMA user_version` gates execution so
/// repeated startups are idempotent.
pub const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_frame_extras.sql"),
];
pub const SCHEMA_VERSION: i32 = MIGRATIONS.len() as i32;

pub fn open_conn(path: &Path) -> CmdResult<Connection> {
    let conn = Connection::open(path).map_err(|e| {
        AppError::io(format!("Cannot open database at {}: {e}", path.display()))
    })?;
    conn.execute_batch(
        "PRAGMA busy_timeout = 5000;
         PRAGMA foreign_keys = ON;",
    )
    .map_err(AppError::from)?;
    let _ = conn.query_row("PRAGMA journal_mode=WAL;", [], |r| r.get::<_, String>(0));
    Ok(conn)
}

pub fn migrate(conn: &mut Connection) -> CmdResult<()> {
    let current: i32 = conn
        .query_row("PRAGMA user_version;", [], |r| r.get(0))
        .map_err(AppError::from)?;
    if current >= SCHEMA_VERSION {
        return Ok(());
    }
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let v = (i + 1) as i32;
        if v <= current {
            continue;
        }
        // Single explicit transaction per migration; never nested because this
        // is the only place that opens one and helpers take &Connection.
        let tx = conn.transaction().map_err(AppError::from)?;
        tx.execute_batch(sql)
            .map_err(|e| AppError::db(format!("Migration {v:04} failed: {e}")))?;
        tx.pragma_update(None, "user_version", v)
            .map_err(AppError::from)?;
        tx.commit().map_err(AppError::from)?;
    }
    Ok(())
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn gen_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_is_idempotent() {
        let mut conn = open_conn(&std::env::temp_dir().join(format!("var-test-{}.db", gen_id()))).unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap();
        let v: i32 = conn.query_row("PRAGMA user_version;", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for t in ["folders", "documents", "mindmaps", "nodes", "edges", "media", "attachments", "settings", "backups"] {
            assert!(tables.iter().any(|x| x == t), "missing table {t}");
        }
        let _ = std::fs::remove_file(std::env::temp_dir().join("unused"));
    }
}
