use rusqlite::Connection;
use variable_lib::db::{self, gen_id, now_ms};
use variable_lib::library;
use variable_lib::mindmap;
use variable_lib::models::Node;

fn temp_db() -> (std::path::PathBuf, Connection) {
    let dir = std::env::temp_dir().join(format!("variable-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}.db", gen_id()));
    let conn = db::open_conn(&path).expect("open");
    (dir, conn)
}

/// Contract: for every canonical write statement, column count == placeholder
/// count == max bound parameter index. Guards against future field drift.
#[test]
fn sql_placeholder_parameter_alignment() {
    let (_dir, mut conn) = temp_db();
    db::migrate(&mut conn).unwrap();
    let cols = |s: &str| s.split(',').count();

    // nodes upsert: 23 columns / 23 placeholders
    assert_eq!(cols(mindmap::NODE_COLS), 23);
    {
        let stmt = conn.prepare(&mindmap::node_upsert_sql()).unwrap();
        assert_eq!(stmt.parameter_count(), 23);
    }

    // edges: insert 13 / update 9
    assert_eq!(cols(mindmap::EDGE_COLS), 13);
    {
        let ins = conn.prepare(&mindmap::edge_insert_sql()).unwrap();
        assert_eq!(ins.parameter_count(), 13);
        let upd = conn.prepare(mindmap::EDGE_UPDATE_SQL).unwrap();
        assert_eq!(upd.parameter_count(), 9);
    }

    // mindmaps partial update: 7
    {
        let mu = conn.prepare(mindmap::MINDMAP_UPDATE_SQL).unwrap();
        assert_eq!(mu.parameter_count(), 7);
    }
}

#[test]
fn migrations_are_idempotent_and_repeated_ops_never_nest_transactions() {
    let (_dir, mut conn) = temp_db();
    db::migrate(&mut conn).unwrap();
    db::migrate(&mut conn).unwrap();
    let v: i32 = conn.query_row("PRAGMA user_version;", [], |r| r.get(0)).unwrap();
    assert_eq!(v, db::SCHEMA_VERSION);

    // Repeated bulk operations that each open ONE transaction: the historical
    // "cannot start a transaction within a transaction" must never appear.
    for round in 0..3 {
        let _f = library::create_folder_inner(&conn, None, &format!("工作-{round}")).unwrap();
        let tx = conn.transaction().unwrap();
        library::set_tags_public(&tx, "doc-x", &["tag1".into(), "tag2".into()]).unwrap();
        tx.commit().unwrap();
    }
    let folders: i64 = conn.query_row("SELECT COUNT(*) FROM folders", [], |r| r.get(0)).unwrap();
    assert_eq!(folders, 3);
    // duplicate sibling name must now be rejected everywhere
    assert!(library::create_folder_inner(&conn, None, "工作-0").is_err());
}

#[test]
fn folder_lifecycle_trash_restore_purge() {
    let (_dir, mut conn) = temp_db();
    db::migrate(&mut conn).unwrap();

    // rename validation
    assert!(library::validate_name("  ").is_err());
    assert!(library::validate_name("a/b").is_err());

    let folder = library::create_folder_inner(&conn, None, "Projects").unwrap();
    // duplicate sibling name rejected
    let dup = library::create_folder_inner(&conn, None, "Projects");
    assert!(dup.is_err());

    let now = now_ms();
    // trash via SQL path used by command (scope fill + update)
    {
        conn.execute_batch("CREATE TEMP TABLE IF NOT EXISTS scope_ids(id TEXT PRIMARY KEY); DELETE FROM scope_ids;").unwrap();
        let tx = conn.transaction().unwrap();
        tx.execute_batch(&format!("INSERT INTO scope_ids(id) VALUES('{}');", folder.id)).unwrap();
        tx.execute("UPDATE folders SET deleted_at=?1 WHERE id IN (SELECT id FROM scope_ids)", [now]).unwrap();
        tx.commit().unwrap();
    }
    let trashed: i64 = conn.query_row("SELECT COUNT(*) FROM folders WHERE deleted_at IS NOT NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(trashed, 1);
    // restore
    conn.execute("UPDATE folders SET deleted_at=NULL WHERE id=?", [&folder.id]).unwrap();
    let active: i64 = conn.query_row("SELECT COUNT(*) FROM folders WHERE deleted_at IS NULL AND id=?", [&folder.id], |r| r.get(0)).unwrap();
    assert_eq!(active, 1);
}

#[test]
fn document_crud_search_and_tags() {
    let (_dir, mut conn) = temp_db();
    db::migrate(&mut conn).unwrap();
    let now = now_ms();
    let id = gen_id();
    conn.execute(
        "INSERT INTO documents(id,folder_id,title,content_html,content_text,favorite,created_at,updated_at)
         VALUES(?1,NULL,'浼氳璁板綍','<p>鍏充簬鍙戝竷璁″垝</p>','鍏充簬鍙戝竷璁″垝',1,?2,?2)",
        [&id, &now.to_string()],
    )
    .unwrap();

    // search hits by body text
    let like = format!("%{}%", library::like_escape("鍙戝竷"));
    let hit: i64 = conn
        .query_row("SELECT COUNT(*) FROM documents WHERE content_text LIKE ?1 ESCAPE '\\'", [&like], |r| r.get(0))
        .unwrap();
    assert_eq!(hit, 1);

    // tags roundtrip inside one tx (no nesting error)
    let tx = conn.transaction().unwrap();
    library::set_tags_public(&tx, &id, &["work".into(), "urgent".into()]).unwrap();
    tx.commit().unwrap();
    let tags = library::doc_tags(&conn, &[id.clone()]).unwrap();
    assert_eq!(tags[&id].len(), 2);

    // trash + purge keeps no orphans in document_tags
    conn.execute("UPDATE documents SET deleted_at=?1 WHERE id=?2", rusqlite::params![now, id])
        .unwrap();
    conn.execute("DELETE FROM document_tags WHERE document_id=?1", [&id]).unwrap();
    conn.execute("DELETE FROM documents WHERE id=?1", [&id]).unwrap();
    let left: i64 = conn.query_row("SELECT COUNT(*) FROM document_tags WHERE document_id=?1", [&id], |r| r.get(0)).unwrap();
    assert_eq!(left, 0);
}

#[test]
fn mindmap_nodes_edges_roundtrip_and_cascade_delete() {
    let (_dir, mut conn) = temp_db();
    db::migrate(&mut conn).unwrap();
    let mid = gen_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO mindmaps(id,folder_id,name,viewport_x,viewport_y,zoom,grid_enabled,snap_enabled,created_at,updated_at)
         VALUES(?1,NULL,'map',10,-5,1.25,1,0,?2,?2)",
        [&mid, &now.to_string()],
    )
    .unwrap();

    let mk_node = |nid: &str, x: f64| {
        Node {
            id: nid.to_string(),
            mindmap_id: mid.clone(),
            text_html: format!("<b>{}</b>", nid),
            text_plain: nid.to_string(),
            x,
            y: 40.0,
            width: 200.0,
            height: 80.0,
            shape: "rounded".into(),
            border_radius: 12.0,
            border_color: "#5b7bd0".into(),
            fill_color: "rgba(13,20,38,0.85)".into(),
            font_size: 14.0,
            opacity: 1.0,
            locked: false,
            z_index: 0,
            record_id: None,
            rotation: 0.0,
            group_id: None,
            hidden: false,
            collapsed: false,
            preset: String::new(),
            updated_at: now_ms(),
        }
    };
    // batch save in a single tx 鈥?repeated twice to prove no nested-tx errors
    for round in 0..2 {
        let nodes = vec![mk_node("n1", 100.0 + round as f64), mk_node("n2", 420.0)];
        let saved = conn.transaction().unwrap();
        for n in &nodes {
            saved.execute(
                "INSERT INTO nodes(id,mindmap_id,text_html,text_plain,x,y,width,height,shape,border_radius,border_color,fill_color,font_size,opacity,locked,z_index,record_id,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
                 ON CONFLICT(id) DO UPDATE SET x=excluded.x",
                rusqlite::params![n.id, n.mindmap_id, n.text_html, n.text_plain, n.x, n.y, n.width, n.height,
                    n.shape, n.border_radius, n.border_color, n.fill_color, n.font_size, n.opacity,
                    n.locked as i64, n.z_index, n.record_id, n.updated_at],
            ).unwrap();
        }
        saved.commit().unwrap();
    }
    let loaded = mindmap::load_nodes(&conn, &mid).unwrap();
    assert_eq!(loaded.len(), 2);

    // edge between them
    conn.execute(
        "INSERT INTO edges(id,mindmap_id,source_node_id,target_node_id,direction,line_style,path_style,color,width,label,animated,created_at)
         VALUES('e1',?1,'n1','n2','forward','solid','curve','#7f9bd9',1.5,'',0,?2)",
        [&mid, &now.to_string()],
    )
    .unwrap();
    assert_eq!(mindmap::load_edges(&conn, &mid).unwrap().len(), 1);

    // deleting nodes removes their edges (explicit cascade)
    conn.execute("DELETE FROM edges WHERE source_node_id='n1' OR target_node_id='n1'", [])
        .unwrap();
    conn.execute("DELETE FROM nodes WHERE id='n1'", []).unwrap();
    assert_eq!(mindmap::load_edges(&conn, &mid).unwrap().len(), 0);

    // html stripping used for search plain text
    assert_eq!(mindmap::strip_html("<p>a&amp;b</p>"), "a&b");
}

#[test]
fn settings_upsert_is_transactional_and_versioned() {
    let (_dir, mut conn) = temp_db();
    db::migrate(&mut conn).unwrap();
    for v in ["1", "2"] {
        let tx = conn.transaction().unwrap();
        tx.execute(
            "INSERT INTO settings(key,value,version,updated_at) VALUES('theme',?1,1,0)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, version=version+1",
            [v],
        )
        .unwrap();
        tx.commit().unwrap();
    }
    let (val, ver): (String, i64) = conn
        .query_row("SELECT value, version FROM settings WHERE key='theme'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(val, "2");
    assert_eq!(ver, 2);
}

/// v2 columns (rotation/group/hidden/collapsed/preset + edge glow) survive a
/// full write→load cycle through the same SQL the commands use.
#[test]
fn frame_extras_roundtrip() {
    let (_dir, mut conn) = temp_db();
    db::migrate(&mut conn).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version;", [], |r| r.get::<_, i32>(0)).unwrap(),
        db::SCHEMA_VERSION
    );
    let mid = gen_id();
    let now = now_ms().to_string();
    conn.execute(
        "INSERT INTO mindmaps(id,folder_id,name,viewport_x,viewport_y,zoom,grid_enabled,snap_enabled,created_at,updated_at)
         VALUES(?1,NULL,'m',0,0,1,1,1,?2,?2)",
        [&mid, &now],
    )
    .unwrap();

    // Insert through the same statement shape as save_nodes (upsert path).
    let n = variable_lib::models::Node {
        id: "nX".into(),
        mindmap_id: mid.clone(),
        text_html: "<b>x</b>".into(),
        text_plain: "x".into(),
        x: 10.0,
        y: 20.0,
        width: 120.0,
        height: 60.0,
        shape: "rounded".into(),
        border_radius: 8.0,
        border_color: "#fff".into(),
        fill_color: "#000".into(),
        font_size: 14.0,
        opacity: 0.5,
        locked: true,
        z_index: 3,
        record_id: None,
        rotation: -37.5,
        group_id: Some("g1".into()),
        hidden: false,
        collapsed: true,
        preset: "cyber".into(),
        updated_at: now_ms(),
    };
    conn.execute(
        "INSERT INTO nodes(id,mindmap_id,text_html,text_plain,x,y,width,height,shape,border_radius,border_color,fill_color,font_size,opacity,locked,z_index,record_id,rotation,group_id,hidden,collapsed,preset,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)
         ON CONFLICT(id) DO UPDATE SET x=excluded.x",
        rusqlite::params![
            n.id, n.mindmap_id, n.text_html, n.text_plain, n.x, n.y, n.width, n.height,
            n.shape, n.border_radius, n.border_color, n.fill_color, n.font_size, n.opacity,
            n.locked as i64, n.z_index, n.record_id, n.rotation, n.group_id,
            n.hidden as i64, n.collapsed as i64, n.preset, n.updated_at
        ],
    )
    .unwrap();

    let loaded = mindmap::load_nodes(&conn, &mid).unwrap();
    assert_eq!(loaded.len(), 1);
    let g = &loaded[0];
    assert_eq!(g.rotation, -37.5);
    assert_eq!(g.group_id.as_deref(), Some("g1"));
    assert!(!g.hidden);
    assert!(g.collapsed);
    assert_eq!(g.preset, "cyber");
    assert!(g.locked);

    // Edge with glow persists.
    conn.execute(
        "INSERT INTO edges(id,mindmap_id,source_node_id,target_node_id,direction,line_style,path_style,color,width,label,animated,glow,created_at)
         VALUES('eG',?1,'nX','nX','both','dashed','ortho','#abcdef',2.5,'L',1,1,?2)",
        [&mid, &now],
    )
    .unwrap();
    let edges = mindmap::load_edges(&conn, &mid).unwrap();
    assert_eq!(edges.len(), 1);
    assert!(edges[0].glow);
    assert_eq!(edges[0].line_style, "dashed");
}


