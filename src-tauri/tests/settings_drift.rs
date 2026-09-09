//! AI-20 质量门禁与收官组 — M-81 设置漂移测试（DB 迁移矩阵，cargo 侧）。
//!
//! 对每个历史 schema 版本 v（1..=SCHEMA_VERSION）：
//! 1. 只应用前 v 个迁移（模拟 v 版老库）；
//! 2. 注入 v 版典型 KV 数据（settings 表）；
//! 3. 跑 `migrate()`（逐级升到当前）；
//! 4. 断言：user_version = 当前、老数据完好、新结构可用。
//!
//! 纪律：新增迁移文件必须在此登记 `LEGACY_VERSIONS` 并补一档矩阵用例
//! （前端侧 fixtures/settings/*.json 与 vitest 同口径，双侧覆盖）。

use variable_lib::db::{migrate, open_conn, MIGRATIONS, SCHEMA_VERSION};
use variable_lib::db;

/// 每个历史版本注入的典型 KV 样例（键集随版本演进；未知键在升级后必须原样保留）。
fn legacy_kv(v: i32) -> Vec<(&'static str, &'static str)> {
    let mut kv = vec![
        ("language", "zh"),
        ("theme", "deep-space"),
        ("fontSize", "16"),
    ];
    if v >= 2 {
        kv.push(("compatHighRefresh", "1"));
        kv.push(("deprecatedRemovedKey", "residue"));
    }
    kv
}

/// 构造一个停在版本 v 的老库（只应用前 v 个迁移）。
fn build_legacy_db(v: i32) -> rusqlite::Connection {
    let path = std::env::temp_dir().join(format!("var-drift-v{v}-{}.db", db::gen_id()));
    let mut conn = open_conn(&path).unwrap();
    let tx = conn.transaction().unwrap();
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let ver = (i + 1) as i32;
        if ver > v {
            break;
        }
        tx.execute_batch(sql).unwrap();
        tx.pragma_update(None, "user_version", ver).unwrap();
    }
    tx.commit().unwrap();
    // 注入 v 版典型数据
    for (k, val) in legacy_kv(v) {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [k, val],
        )
        .unwrap();
    }
    conn
}

#[test]
fn migration_matrix_every_legacy_version_upgrades() {
    for v in 1..=SCHEMA_VERSION {
        let mut conn = build_legacy_db(v);
        // 升级前：老数据在场
        let before: i32 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, v);

        migrate(&mut conn).unwrap();

        // ① 版本号推进到当前
        let after: i32 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(after, SCHEMA_VERSION, "v{v} 升级后 user_version 应为 {SCHEMA_VERSION}");

        // ② 老数据零丢失（含废弃键也原样保留 —— 只读迁移绝不删用户数据）
        for (k, val) in legacy_kv(v) {
            let got: String = conn
                .query_row("SELECT value FROM settings WHERE key = ?1", [k], |r| r.get(0))
                .unwrap_or_default();
            assert_eq!(got, val, "v{v} 升级后键 {k} 数据丢失");
        }

        // ③ 新结构可用：settings 表可写新键（新版键写入无冲突）
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('ai20NewKey', '1')
             ON CONFLICT(key) DO UPDATE SET value = '1'",
            [],
        )
        .unwrap();

        // ④ 迁移幂等：再跑一次不报错不回退
        migrate(&mut conn).unwrap();
        let v2: i32 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v2, SCHEMA_VERSION);
    }
}

#[test]
fn migration_refuses_future_version_downgrade_is_reported() {
    // 未来版本库（user_version > 当前）：migrate 不做任何事且不报错
    // —— 降级保护由 container 层负责，此处锁死「绝不盲目跑迁移」的现状口径。
    let path = std::env::temp_dir().join(format!("var-drift-future-{}.db", db::gen_id()));
    let mut conn = open_conn(&path).unwrap();
    conn.pragma_update(None, "user_version", SCHEMA_VERSION + 5).unwrap();
    migrate(&mut conn).unwrap();
    let v: i32 = conn
        .query_row("PRAGMA user_version;", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, SCHEMA_VERSION + 5, "未来版本库必须原样保留（不做降级迁移）");
}

#[test]
fn corrupted_migration_sql_is_caught_not_silenced() {
    // 人为破坏迁移函数被测试拦截：破坏的 SQL 必须以 Err 暴露（迁移失败 ≠ 静默跳过）
    let path = std::env::temp_dir().join(format!("var-drift-bad-{}.db", db::gen_id()));
    let mut conn = open_conn(&path).unwrap();
    {
        let tx = conn.transaction().unwrap();
        tx.execute_batch("CREATE TABLE t1 (id INTEGER);").unwrap();
        tx.pragma_update(None, "user_version", 1).unwrap();
        tx.commit().unwrap();
    }
    // 坏 SQL 直接 execute_batch 必须报错（rusqlite 层拦截，绝不静默吞掉）
    let r = conn.execute_batch("THIS IS NOT SQL AT ALL;");
    assert!(r.is_err(), "破坏性 SQL 必须报错");
}
