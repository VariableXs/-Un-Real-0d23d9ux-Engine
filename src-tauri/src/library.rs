use crate::db::{gen_id, now_ms};
use crate::error::{AppError, CmdResult};
use crate::models::*;
use crate::state::AppState;
use rusqlite::{params, Connection};

// ---------- row mappers ----------

fn folder_from_row(r: &rusqlite::Row) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        name: r.get(2)?,
        sort_order: r.get(3)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
        deleted_at: r.get(6)?,
    })
}

pub fn doc_tags(conn: &Connection, ids: &[String]) -> CmdResult<std::collections::HashMap<String, Vec<String>>> {
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    if ids.is_empty() {
        return Ok(map);
    }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT dt.document_id, t.name FROM document_tags dt JOIN tags t ON t.id = dt.tag_id
         WHERE dt.document_id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql).map_err(AppError::from)?;
    let mut p: Vec<&dyn rusqlite::ToSql> = Vec::new();
    for i in ids {
        p.push(i);
    }
    let rows = stmt.query_map(p.as_slice(), |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, name) = row.map_err(AppError::from)?;
        map.entry(id).or_default().push(name);
    }
    Ok(map)
}

const DOC_COLS_FULL: &str =
    "id, folder_id, title, content_html, content_text, favorite, created_at, updated_at, deleted_at";
const DOC_COLS_LIST: &str =
    "id, folder_id, title, NULL, NULL, favorite, created_at, updated_at, deleted_at";

// ---------- validation ----------

pub fn validate_name(raw: &str) -> CmdResult<String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(AppError::validation("名称不能为空 / Name cannot be empty"));
    }
    if name.len() > 120 {
        return Err(AppError::validation("名称过长（最多 120 字符）/ Name too long (max 120 chars)"));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(AppError::validation("名称包含非法字符 / Name contains invalid characters"));
    }
    Ok(name.to_string())
}

fn ensure_unique_folder_name(conn: &Connection, parent: Option<&str>, name: &str, exclude: Option<&str>) -> CmdResult<()> {
    let dup: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM folders WHERE deleted_at IS NULL AND name = ?1
             AND (parent_id IS ?2 OR (?2 IS NULL AND parent_id IS NULL))
             AND (id IS NOT ?3 OR ?3 IS NULL)",
            params![name, parent, exclude],
            |r| r.get(0),
        )
        .map_err(AppError::from)?;
    if dup > 0 {
        return Err(AppError::validation("同级已存在同名文件夹 / A sibling folder has the same name"));
    }
    Ok(())
}

// ---------- folders ----------

#[tauri::command]
pub fn list_folders(st: tauri::State<AppState>) -> CmdResult<Vec<Folder>> {
    st.with_conn(|conn| {
        let mut stmt = conn
            .prepare("SELECT id,parent_id,name,sort_order,created_at,updated_at,deleted_at FROM folders ORDER BY sort_order, name COLLATE NOCASE")
            .map_err(AppError::from)?;
        let rows = stmt
            .query_map([], folder_from_row)
            .map_err(AppError::from)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

#[tauri::command]
pub fn create_folder(st: tauri::State<AppState>, name: String, parent_id: Option<String>) -> CmdResult<Folder> {
    let name = validate_name(&name)?;
    st.with_conn(|conn| {
        if let Some(pid) = parent_id.as_deref() {
            let exists: i64 = conn
                .query_row("SELECT COUNT(*) FROM folders WHERE id=?1 AND deleted_at IS NULL", params![pid], |r| r.get(0))
                .map_err(AppError::from)?;
            if exists == 0 {
                return Err(AppError::not_found("父文件夹不存在 / Parent folder not found"));
            }
        }
        ensure_unique_folder_name(conn, parent_id.as_deref(), &name, None)?;
        create_folder_inner(conn, parent_id.as_deref(), &name)
    })
}

pub fn create_folder_inner(conn: &Connection, parent_id: Option<&str>, name: &str) -> CmdResult<Folder> {
    ensure_unique_folder_name(conn, parent_id, name, None)?;
    let id = gen_id();
    let now = now_ms();
    let max_order: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order),0) FROM folders WHERE parent_id IS ?",
            params![parent_id],
            |r| r.get(0),
        )
        .map_err(AppError::from)?;
    conn.execute(
        "INSERT INTO folders(id,parent_id,name,sort_order,created_at,updated_at,deleted_at) VALUES(?1,?2,?3,?4,?5,?5,NULL)",
        params![id, parent_id, name, max_order + 1, now],
    )
    .map_err(AppError::from)?;
    Ok(Folder { id, parent_id: parent_id.map(|s| s.to_string()), name: name.to_string(), sort_order: max_order + 1, created_at: now, updated_at: now, deleted_at: None })
}

#[tauri::command]
pub fn rename_folder(st: tauri::State<AppState>, id: String, name: String) -> CmdResult<()> {
    let name = validate_name(&name)?;
    st.with_conn(|conn| {
        let parent: Option<String> = conn
            .query_row("SELECT parent_id FROM folders WHERE id=?", params![id], |r| r.get(0))
            .map_err(|_| AppError::not_found("文件夹不存在 / Folder not found"))?;
        ensure_unique_folder_name(conn, parent.as_deref(), &name, Some(&id))?;
        // Prevent renaming a folder into its own subtree is irrelevant for rename;
        // but guard moving later. Rename now:
        conn.execute("UPDATE folders SET name=?1, updated_at=?2 WHERE id=?3", params![name, now_ms(), id])
            .map_err(AppError::from)?;
        Ok(())
    })
}

#[tauri::command]
pub fn move_folder(st: tauri::State<AppState>, id: String, new_parent_id: Option<String>) -> CmdResult<()> {
    st.with_conn(|conn| {
        if new_parent_id.as_deref() == Some(id.as_str()) {
            return Err(AppError::validation("不能移动到自身 / Cannot move into itself"));
        }
        // Guard cycles: new parent must not be a descendant of id.
        if let Some(np) = new_parent_id.as_deref() {
            let mut cur = Some(np.to_string());
            let mut hops = 0;
            while let Some(c) = cur {
                if c == id {
                    return Err(AppError::validation("不能移动到自己的子文件夹 / Cannot move into own subtree"));
                }
                cur = conn
                    .query_row("SELECT parent_id FROM folders WHERE id=?", params![c], |r| r.get::<_, Option<String>>(0))
                    .map_err(|_| AppError::not_found("目标文件夹不存在 / Target folder not found"))?;
                hops += 1;
                if hops > 1000 {
                    return Err(AppError::validation("文件夹层级异常 / Folder hierarchy cycle detected"));
                }
            }
        }
        let name: String = conn.query_row("SELECT name FROM folders WHERE id=?", params![id], |r| r.get(0))
            .map_err(|_| AppError::not_found("文件夹不存在 / Folder not found"))?;
        ensure_unique_folder_name(conn, new_parent_id.as_deref(), &name, Some(&id))?;
        conn.execute(
            "UPDATE folders SET parent_id=?1, updated_at=?2 WHERE id=?3",
            params![new_parent_id, now_ms(), id],
        )
        .map_err(AppError::from)?;
        Ok(())
    })
}

/// Collect a folder subtree into TEMP table trash_scope. Must run inside tx.
fn fill_subtree_scope(conn: &Connection, root: &str) -> CmdResult<()> {
    conn.execute_batch("CREATE TEMP TABLE IF NOT EXISTS scope_ids(id TEXT PRIMARY KEY); DELETE FROM scope_ids;")
        .map_err(AppError::from)?;
    conn.execute(
        "INSERT INTO scope_ids(id) WITH RECURSIVE sub(id) AS (
            SELECT id FROM folders WHERE id = ?1
            UNION ALL
            SELECT f.id FROM folders f JOIN sub s ON f.parent_id = s.id
         ) SELECT id FROM sub",
        params![root],
    )
    .map_err(AppError::from)?;
    Ok(())
}

#[tauri::command]
pub fn trash_folder(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    st.with_conn(|conn| {
        let tx = conn.transaction().map_err(AppError::from)?;
        fill_subtree_scope(&tx, &id)?;
        let now = now_ms();
        tx.execute("UPDATE folders SET deleted_at=?1, updated_at=?1 WHERE id IN (SELECT id FROM scope_ids)", params![now])
            .map_err(AppError::from)?;
        tx.execute("UPDATE documents SET deleted_at=?1 WHERE deleted_at IS NULL AND folder_id IN (SELECT id FROM scope_ids)", params![now])
            .map_err(AppError::from)?;
        tx.execute("UPDATE mindmaps SET deleted_at=?1 WHERE deleted_at IS NULL AND folder_id IN (SELECT id FROM scope_ids)", params![now])
            .map_err(AppError::from)?;
        tx.commit().map_err(AppError::from)?;
        Ok(())
    })
}

#[tauri::command]
pub fn restore_folder(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    st.with_conn(|conn| {
        let tx = conn.transaction().map_err(AppError::from)?;
        fill_subtree_scope(&tx, &id)?;
        // If an ancestor is still trashed, refuse to partially restore.
        let ancestor_trashed: i64 = tx
            .query_row(
                "WITH RECURSIVE up(id, pid) AS (
                    SELECT id, parent_id FROM folders WHERE id=?1
                    UNION ALL
                    SELECT f.id, f.parent_id FROM folders f JOIN up u ON f.id=u.pid
                 ) SELECT COUNT(*) FROM up u JOIN folders f ON f.id=u.id
                   WHERE f.deleted_at IS NOT NULL AND u.id != ?1",
                params![id],
                |r| r.get(0),
            )
            .map_err(AppError::from)?;
        if ancestor_trashed > 0 {
            return Err(AppError::validation("请先恢复其上级文件夹 / Restore its parent folder first"));
        }
        tx.execute("UPDATE folders SET deleted_at=NULL, updated_at=?1 WHERE id IN (SELECT id FROM scope_ids)", params![now_ms()])
            .map_err(AppError::from)?;
        tx.execute("UPDATE documents SET deleted_at=NULL WHERE folder_id IN (SELECT id FROM scope_ids)", [])
            .map_err(AppError::from)?;
        tx.execute("UPDATE mindmaps SET deleted_at=NULL WHERE folder_id IN (SELECT id FROM scope_ids)", [])
            .map_err(AppError::from)?;
        tx.commit().map_err(AppError::from)?;
        Ok(())
    })
}

fn purge_docs_in_scope(tx: &Connection) -> CmdResult<Vec<(String, String)>> {
    // Returns (media rel path, kind) pairs safe to unlink from disk after commit.
    let mut out: Vec<(String, String)> = Vec::new();
    {
        let mut stmt = tx
            .prepare(
                "SELECT a.rel_path, m.file_name FROM attachments a
                 JOIN media m ON m.id=a.media_id
                 WHERE a.document_id IN (SELECT id FROM documents WHERE deleted_at IS NOT NULL AND folder_id IN (SELECT id FROM scope_ids))",
            )
            .map_err(AppError::from)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(AppError::from)?;
        for r in rows {
            out.push(r.map_err(AppError::from)?);
        }
    }
    tx.execute_batch(
        "DELETE FROM document_tags WHERE document_id IN (SELECT id FROM documents WHERE deleted_at IS NOT NULL AND folder_id IN (SELECT id FROM scope_ids));
         DELETE FROM attachments WHERE document_id IN (SELECT id FROM documents WHERE deleted_at IS NOT NULL AND folder_id IN (SELECT id FROM scope_ids));
         DELETE FROM documents WHERE deleted_at IS NOT NULL AND folder_id IN (SELECT id FROM scope_ids);",
    )
    .map_err(AppError::from)?;
    Ok(out)
}

fn purge_mindmaps_in_scope(tx: &Connection) -> CmdResult<()> {
    tx.execute_batch(
        "DELETE FROM edges WHERE mindmap_id IN (SELECT id FROM mindmaps WHERE deleted_at IS NOT NULL AND folder_id IN (SELECT id FROM scope_ids));
         DELETE FROM nodes WHERE mindmap_id IN (SELECT id FROM mindmaps WHERE deleted_at IS NOT NULL AND folder_id IN (SELECT id FROM scope_ids));
         DELETE FROM mindmaps WHERE deleted_at IS NOT NULL AND folder_id IN (SELECT id FROM scope_ids);",
    )
    .map_err(AppError::from)?;
    Ok(())
}

#[tauri::command]
pub fn purge_folder(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    let files = st.with_conn(|conn| {
        let tx = conn.transaction().map_err(AppError::from)?;
        fill_subtree_scope(&tx, &id)?;
        let files = purge_docs_in_scope(&tx)?;
        purge_mindmaps_in_scope(&tx)?;
        tx.execute_batch("DELETE FROM folders WHERE id IN (SELECT id FROM scope_ids); DELETE FROM scope_ids;")
            .map_err(AppError::from)?;
        tx.commit().map_err(AppError::from)?;
        Ok(files)
    })?;
    crate::media::unlink_media_files(&st, &files);
    Ok(())
}

// ---------- documents ----------

fn load_doc(conn: &Connection, id: &str) -> CmdResult<Document> {
    let sql = format!("SELECT {DOC_COLS_FULL} FROM documents WHERE id = ?1");
    let mut doc = conn
        .query_row(&sql, params![id], |r| doc_from_row_named(r))
        .map_err(|_| AppError::not_found("记录不存在 / Document not found"))?;
    doc.tags = doc_tags(conn, &[id.to_string()])?
        .remove(id)
        .unwrap_or_default();
    Ok(doc)
}

pub(crate) fn get_document_public(conn: &Connection, id: &str) -> CmdResult<Document> {
    load_doc(conn, id)
}

pub fn set_tags_public(conn: &Connection, doc_id: &str, tags: &[String]) -> CmdResult<()> {
    set_tags_inner(conn, doc_id, tags)
}

fn doc_from_row_named(r: &rusqlite::Row) -> rusqlite::Result<Document> {
    Ok(Document {
        id: r.get("id")?,
        folder_id: r.get("folder_id")?,
        title: r.get("title")?,
        content_html: r.get::<_, Option<String>>("content_html")?.unwrap_or_default(),
        content_text: r.get::<_, Option<String>>("content_text")?.unwrap_or_default(),
        favorite: r.get::<_, i64>("favorite")? != 0,
        tags: Vec::new(),
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        deleted_at: r.get("deleted_at")?,
    })
}

#[tauri::command]
pub fn create_document(st: tauri::State<AppState>, folder_id: Option<String>, title: Option<String>) -> CmdResult<Document> {
    st.with_conn(|conn| {
        if let Some(fid) = folder_id.as_deref() {
            let ok: i64 = conn.query_row("SELECT COUNT(*) FROM folders WHERE id=?1 AND deleted_at IS NULL", params![fid], |r| r.get(0)).map_err(AppError::from)?;
            if ok == 0 {
                return Err(AppError::not_found("目标文件夹不存在 / Target folder not found"));
            }
        }
        let id = gen_id();
        let now = now_ms();
        conn.execute(
            "INSERT INTO documents(id,folder_id,title,content_html,content_text,favorite,created_at,updated_at,deleted_at)
             VALUES(?1,?2,?3,'','',0,?4,?4,NULL)",
            params![id, folder_id, title.unwrap_or_default(), now],
        )
        .map_err(AppError::from)?;
        load_doc(conn, &id)
    })
}

#[tauri::command]
pub fn get_document(st: tauri::State<AppState>, id: String) -> CmdResult<Document> {
    st.with_conn(|conn| load_doc(conn, &id))
}

#[tauri::command]
pub async fn save_document(st: tauri::State<'_, AppState>, input: DocumentInput) -> CmdResult<Document> {
    st.with_conn(|conn| {
        let n = conn
            .execute(
                "UPDATE documents SET title=?2, content_html=?3, content_text=?4,
                 favorite = COALESCE(?5, favorite), folder_id = COALESCE(?6, folder_id),
                 updated_at=?7
                 WHERE id=?1 AND deleted_at IS NULL",
                params![
                    input.id,
                    input.title,
                    input.content_html,
                    input.content_text,
                    input.favorite.map(|b| b as i64),
                    input.folder_id,
                    now_ms()
                ],
            )
            .map_err(AppError::from)?;
        if n == 0 {
            return Err(AppError::not_found("记录不存在或已在回收站 / Document missing or trashed"));
        }
        load_doc(conn, &input.id)
    })
}

#[tauri::command]
pub fn move_document(st: tauri::State<AppState>, id: String, folder_id: Option<String>) -> CmdResult<()> {
    st.with_conn(|conn| {
        conn.execute("UPDATE documents SET folder_id=?1, updated_at=?2 WHERE id=?3", params![folder_id, now_ms(), id])
            .map_err(AppError::from)?;
        Ok(())
    })
}

#[tauri::command]
pub fn set_document_favorite(st: tauri::State<AppState>, id: String, favorite: bool) -> CmdResult<()> {
    st.with_conn(|conn| {
        conn.execute("UPDATE documents SET favorite=?1, updated_at=?2 WHERE id=?3", params![favorite as i64, now_ms(), id])
            .map_err(AppError::from)?;
        Ok(())
    })
}

#[tauri::command]
pub fn set_document_tags(st: tauri::State<AppState>, id: String, tags: Vec<String>) -> CmdResult<Vec<String>> {
    let cleaned: Vec<String> = tags
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty() && t.len() <= 40)
        .map(|t| t.to_string())
        .collect();
    st.with_conn(|conn| {
        let tx = conn.transaction().map_err(AppError::from)?;
        set_tags_inner(&tx, &id, &cleaned)?;
        tx.commit().map_err(AppError::from)?;
        Ok(cleaned)
    })
}

fn set_tags_inner(conn: &Connection, doc_id: &str, tags: &[String]) -> CmdResult<()> {
    conn.execute("DELETE FROM document_tags WHERE document_id=?1", params![doc_id]).map_err(AppError::from)?;
    for t in tags {
        conn.execute(
            "INSERT INTO tags(id,name) VALUES(?1,?2) ON CONFLICT(name) DO NOTHING",
            params![gen_id(), t],
        )
        .map_err(AppError::from)?;
        let tid: String = conn
            .query_row("SELECT id FROM tags WHERE name=?1", params![t], |r| r.get(0))
            .map_err(AppError::from)?;
        conn.execute(
            "INSERT OR IGNORE INTO document_tags(document_id,tag_id) VALUES(?1,?2)",
            params![doc_id, tid],
        )
        .map_err(AppError::from)?;
    }
    conn.execute("UPDATE documents SET updated_at=?1 WHERE id=?2", params![now_ms(), doc_id])
        .map_err(AppError::from)?;
    Ok(())
}

#[tauri::command]
pub fn list_document_tags(st: tauri::State<AppState>) -> CmdResult<Vec<String>> {
    st.with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT name FROM tags ORDER BY name COLLATE NOCASE").map_err(AppError::from)?;
        let v = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(AppError::from)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })
}

#[tauri::command]
pub fn list_documents(st: tauri::State<AppState>, filter: ListFilter) -> CmdResult<Vec<Document>> {
    st.with_conn(|conn| {
        let mut clauses: Vec<String> = Vec::new();
        let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        match filter.view.as_deref().unwrap_or("all") {
            "trash" => clauses.push("deleted_at IS NOT NULL".into()),
            "favorites" => {
                clauses.push("deleted_at IS NULL".into());
                clauses.push("favorite = 1".into());
            }
            _ => clauses.push("deleted_at IS NULL".into()),
        }
        if let Some(fid) = &filter.folder_id {
            clauses.push(format!("folder_id = ?{}", p.len() + 1));
            p.push(Box::new(fid.clone()));
        }
        if let Some(q) = filter.query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
            clauses.push(format!(
                "(title LIKE ?{} ESCAPE '\\' OR content_text LIKE ?{} ESCAPE '\\')",
                p.len() + 1,
                p.len() + 2
            ));
            let like = format!("%{}%", like_escape(q));
            p.push(Box::new(like.clone()));
            p.push(Box::new(like));
        }
        if let Some(tag) = &filter.tag {
            clauses.push(format!(
                "id IN (SELECT dt.document_id FROM document_tags dt JOIN tags t ON t.id=dt.tag_id WHERE t.name = ?{})",
                p.len() + 1
            ));
            p.push(Box::new(tag.clone()));
        }
        let order = match filter.sort.as_deref() {
            Some("created") => "created_at DESC",
            _ => "updated_at DESC",
        };
        let sql = format!(
            "SELECT {DOC_COLS_LIST} FROM documents WHERE {} ORDER BY {order} LIMIT 500",
            clauses.join(" AND ")
        );
        let mut stmt = conn.prepare(&sql).map_err(AppError::from)?;
        let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
        let docs: Vec<Document> = stmt
            .query_map(refs.as_slice(), |r| {
                Ok(Document {
                    id: r.get(0)?,
                    folder_id: r.get(1)?,
                    title: r.get(2)?,
                    content_html: String::new(),
                    content_text: String::new(),
                    favorite: r.get::<_, i64>(5)? != 0,
                    tags: Vec::new(),
                    created_at: r.get(6)?,
                    updated_at: r.get(7)?,
                    deleted_at: r.get(8)?,
                })
            })
            .map_err(AppError::from)?
            .filter_map(|r| r.ok())
            .collect();
        let ids: Vec<String> = docs.iter().map(|d| d.id.clone()).collect();
        let tags = doc_tags(conn, &ids)?;
        let mut docs = docs;
        for d in docs.iter_mut() {
            d.tags = tags.get(&d.id).cloned().unwrap_or_default();
        }
        Ok(docs)
    })
}

pub fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

#[tauri::command]
pub fn trash_document(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    st.with_conn(|conn| {
        conn.execute("UPDATE documents SET deleted_at=?1 WHERE id=?2", params![now_ms(), id])
            .map_err(AppError::from)?;
        Ok(())
    })
}

#[tauri::command]
pub fn restore_document(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    st.with_conn(|conn| {
        conn.execute("UPDATE documents SET deleted_at=NULL WHERE id=?1", params![id])
            .map_err(AppError::from)?;
        Ok(())
    })
}

#[tauri::command]
pub async fn purge_documents(st: tauri::State<'_, AppState>, ids: Vec<String>) -> CmdResult<()> {
    let files = st.with_conn(|conn| {
        let mut files: Vec<(String, String)> = Vec::new();
        for id in &ids {
            let mut stmt = conn
                .prepare("SELECT a.rel_path, m.file_name FROM attachments a JOIN media m ON m.id=a.media_id WHERE a.document_id=?1")
                .map_err(AppError::from)?;
            let rows = stmt
                .query_map(params![id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(AppError::from)?;
            for r in rows {
                files.push(r.map_err(AppError::from)?);
            }
        }
        let tx = conn.transaction().map_err(AppError::from)?;
        let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let p: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        tx.execute(
            &format!("DELETE FROM document_tags WHERE document_id IN ({ph})"),
            p.as_slice(),
        )
        .map_err(AppError::from)?;
        tx.execute(&format!("DELETE FROM attachments WHERE document_id IN ({ph})"), p.as_slice())
            .map_err(AppError::from)?;
        tx.execute(&format!("DELETE FROM documents WHERE id IN ({ph})"), p.as_slice())
            .map_err(AppError::from)?;
        // Mindmaps that were trashed standalone can be purged here too if requested via ids.
        tx.execute(
            &format!(
                "DELETE FROM edges WHERE mindmap_id IN ({ph});
                 DELETE FROM nodes WHERE mindmap_id IN ({ph});
                 DELETE FROM mindmaps WHERE id IN ({ph});"
            ),
            p.as_slice(),
        )
        .map_err(AppError::from)?;
        tx.execute("DELETE FROM media WHERE id NOT IN (SELECT media_id FROM attachments)", [])
            .map_err(AppError::from)?;
        tx.commit().map_err(AppError::from)?;
        drop(p);
        Ok(files)
    })?;
    crate::media::unlink_media_files(&st, &files);
    Ok(())
}

#[tauri::command]
pub async fn empty_trash(st: tauri::State<'_, AppState>) -> CmdResult<u32> {
    let ids: Vec<String> = st.with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT id FROM documents WHERE deleted_at IS NOT NULL").map_err(AppError::from)?;
        let v = stmt.query_map([], |r| r.get(0)).map_err(AppError::from)?.filter_map(|r| r.ok()).collect();
        Ok(v)
    })?;
    let count = ids.len() as u32;
    if count > 0 {
        purge_documents(st, ids).await?;
    }
    Ok(count)
}

// ---------- global search ----------

fn snippet(text: &str, query: &str) -> String {
    let lower_t = text.to_lowercase();
    let lower_q = query.to_lowercase();
    let pos = lower_t.find(&lower_q);
    let start = pos.map(|p| p.saturating_sub(24)).unwrap_or(0);
    let end = (start + 88).min(text.len());
    let mut s: String = text.chars().skip(text[..start].chars().count()).take(200).collect();
    if start > 0 {
        s.insert_str(0, "…");
    }
    if end < text.len() {
        s.push('…');
    }
    s.replace('\n', " ")
}

#[tauri::command]
pub fn search_all(st: tauri::State<AppState>, query: String) -> CmdResult<Vec<SearchHit>> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    st.with_conn(|conn| {
        let like = format!("%{}%", like_escape(&q));
        let mut hits: Vec<SearchHit> = Vec::new();

        let mut stmt = conn
            .prepare("SELECT id,folder_id,title,content_text,updated_at FROM documents WHERE deleted_at IS NULL AND (title LIKE ?1 ESCAPE '\\' OR content_text LIKE ?1 ESCAPE '\\') ORDER BY updated_at DESC LIMIT 30")
            .map_err(AppError::from)?;
        for r in stmt
            .query_map(params![like], |r| {
                let title: String = r.get(2)?;
                let body: String = r.get::<_, Option<String>>(3)?.unwrap_or_default();
                Ok(SearchHit {
                    kind: "document".into(),
                    id: r.get(0)?,
                    parent_id: r.get(1)?,
                    title: if title.is_empty() { "(无标题 / Untitled)".into() } else { title },
                    snippet: if body.to_lowercase().contains(&q.to_lowercase()) { snippet(&body, &q) } else { body.chars().take(80).collect() },
                    updated_at: r.get(4)?,
                })
            })
            .map_err(AppError::from)?
        {
            hits.push(r.map_err(AppError::from)?);
        }

        let mut stmt = conn
            .prepare("SELECT id,name,updated_at FROM folders WHERE deleted_at IS NULL AND name LIKE ?1 ESCAPE '\\' LIMIT 10")
            .map_err(AppError::from)?;
        for r in stmt
            .query_map(params![like], |r| {
                let name: String = r.get(1)?;
                Ok(SearchHit { kind: "folder".into(), id: r.get(0)?, parent_id: None, title: name, snippet: String::new(), updated_at: r.get(2)? })
            })
            .map_err(AppError::from)?
        {
            hits.push(r.map_err(AppError::from)?);
        }

        let mut stmt = conn
            .prepare("SELECT id,folder_id,name,updated_at FROM mindmaps WHERE deleted_at IS NULL AND name LIKE ?1 ESCAPE '\\' LIMIT 10")
            .map_err(AppError::from)?;
        for r in stmt
            .query_map(params![like], |r| {
                let name: String = r.get(2)?;
                Ok(SearchHit { kind: "mindmap".into(), id: r.get(0)?, parent_id: r.get(1)?, title: name, snippet: String::new(), updated_at: r.get(3)? })
            })
            .map_err(AppError::from)?
        {
            hits.push(r.map_err(AppError::from)?);
        }

        let mut stmt = conn
            .prepare(
                "SELECT n.id,n.mindmap_id,n.text_plain,m.name,n.updated_at FROM nodes n
                 JOIN mindmaps m ON m.id=n.mindmap_id
                 WHERE m.deleted_at IS NULL AND n.text_plain LIKE ?1 ESCAPE '\\' LIMIT 20",
            )
            .map_err(AppError::from)?;
        for r in stmt
            .query_map(params![like], |r| {
                let text: String = r.get(2)?;
                let mname: String = r.get(3)?;
                Ok(SearchHit { kind: "node".into(), id: r.get(0)?, parent_id: r.get(1)?, title: mname, snippet: snippet(&text, &q), updated_at: r.get(4)? })
            })
            .map_err(AppError::from)?
        {
            hits.push(r.map_err(AppError::from)?);
        }
        Ok(hits)
    })
}






