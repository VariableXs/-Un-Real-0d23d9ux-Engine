use crate::db::{gen_id, now_ms};
use crate::error::{AppError, CmdResult};
use crate::models::*;
use crate::state::AppState;
use rusqlite::{params, Connection};

pub(crate) fn map_from_row(r: &rusqlite::Row) -> rusqlite::Result<Mindmap> {
    Ok(Mindmap {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        name: r.get(2)?,
        viewport_x: r.get(3)?,
        viewport_y: r.get(4)?,
        zoom: r.get(5)?,
        grid_enabled: r.get::<_, i64>(6)? != 0,
        snap_enabled: r.get::<_, i64>(7)? != 0,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
        deleted_at: r.get(10)?,
    })
}

pub(crate) const MAP_COLS: &str = "id,folder_id,name,viewport_x,viewport_y,zoom,grid_enabled,snap_enabled,created_at,updated_at,deleted_at";

/// Build an explicit numbered placeholder list (?1..?n) sized EXACTLY to a
/// column list. Placeholders must never be hand-written next to a column list:
/// this keeps columns / placeholders / bound params structurally aligned.
fn placeholders(n: usize) -> String {
    (1..=n).map(|i| format!("?{i}")).collect::<Vec<_>>().join(",")
}

#[tauri::command]
pub fn list_mindmaps(st: tauri::State<AppState>) -> CmdResult<Vec<Mindmap>> {
    st.with_conn(|conn| {
        let mut stmt = conn
            .prepare(&format!("SELECT {MAP_COLS} FROM mindmaps WHERE deleted_at IS NULL ORDER BY updated_at DESC"))
            .map_err(AppError::from)?;
        let rows = stmt.query_map([], map_from_row).map_err(AppError::from)?;
        let out: Vec<Mindmap> = rows.filter_map(|r| r.ok()).collect();
        Ok(out)
    })
}

#[tauri::command]
pub fn create_mindmap(st: tauri::State<AppState>, name: Option<String>, folder_id: Option<String>) -> CmdResult<Mindmap> {
    let name = {
        let n = name.unwrap_or_default().trim().to_string();
        if n.is_empty() {
            "未命名导图 / Untitled Map".to_string()
        } else if n.len() > 120 {
            return Err(AppError::validation("名称过长 / Name too long"));
        } else {
            n
        }
    };
    let id = gen_id();
    let now = now_ms();
    st.with_conn(|conn| {
        conn.execute(
            "INSERT INTO mindmaps(id,folder_id,name,viewport_x,viewport_y,zoom,grid_enabled,snap_enabled,created_at,updated_at)
             VALUES(?1,?2,?3,0,0,1,1,1,?4,?4)",
            params![id, folder_id, name, now],
        )
        .map_err(AppError::from)?;
        Ok(Mindmap {
            id,
            folder_id,
            name,
            viewport_x: 0.0,
            viewport_y: 0.0,
            zoom: 1.0,
            grid_enabled: true,
            snap_enabled: true,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        })
    })
}

#[tauri::command]
pub fn get_mindmap(st: tauri::State<AppState>, id: String) -> CmdResult<MindmapData> {
    st.with_conn(|conn| get_mindmap_inner(conn, &id))
}

pub struct MindmapData {
    pub mindmap: Mindmap,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl serde::Serialize for MindmapData {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("MindmapData", 3)?;
        st.serialize_field("mindmap", &self.mindmap)?;
        st.serialize_field("nodes", &self.nodes)?;
        st.serialize_field("edges", &self.edges)?;
        st.end()
    }
}

pub(crate) fn get_mindmap_inner(conn: &Connection, id: &str) -> CmdResult<MindmapData> {
    let mindmap = conn
        .query_row(
            &format!("SELECT {MAP_COLS} FROM mindmaps WHERE id=?1 AND deleted_at IS NULL"),
            params![id],
            map_from_row,
        )
        .map_err(|_| AppError::not_found("思维导图不存在 / Mindmap not found"))?;
    let nodes = load_nodes(conn, id)?;
    let edges = load_edges(conn, id)?;
    Ok(MindmapData { mindmap, nodes, edges })
}

fn node_from_row(r: &rusqlite::Row) -> rusqlite::Result<Node> {
    Ok(Node {
        id: r.get(0)?,
        mindmap_id: r.get(1)?,
        text_html: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
        text_plain: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
        x: r.get(4)?,
        y: r.get(5)?,
        width: r.get(6)?,
        height: r.get(7)?,
        shape: r.get(8)?,
        border_radius: r.get(9)?,
        border_color: r.get(10)?,
        fill_color: r.get(11)?,
        font_size: r.get(12)?,
        opacity: r.get(13)?,
        locked: r.get::<_, i64>(14)? != 0,
        z_index: r.get(15)?,
        record_id: r.get(16)?,
        rotation: r.get::<_, Option<f64>>(17)?.unwrap_or(0.0),
        group_id: r.get(18)?,
        hidden: r.get::<_, Option<i64>>(19)?.unwrap_or(0) != 0,
        collapsed: r.get::<_, Option<i64>>(20)?.unwrap_or(0) != 0,
        preset: r.get::<_, Option<String>>(21)?.unwrap_or_default(),
        updated_at: r.get(22)?,
    })
}

pub const NODE_COLS: &str =
    "id,mindmap_id,text_html,text_plain,x,y,width,height,shape,border_radius,border_color,fill_color,font_size,opacity,locked,z_index,record_id,rotation,group_id,hidden,collapsed,preset,updated_at";

/// Canonical nodes upsert: placeholder count is derived from NODE_COLS, so a
/// future column addition cannot desynchronize columns ↔ placeholders ↔ params.
pub fn node_upsert_sql() -> String {
    format!(
        "INSERT INTO nodes({NODE_COLS})
         VALUES({})
         ON CONFLICT(id) DO UPDATE SET
           text_html=excluded.text_html, text_plain=excluded.text_plain,
           x=excluded.x, y=excluded.y, width=excluded.width, height=excluded.height,
           shape=excluded.shape, border_radius=excluded.border_radius,
           border_color=excluded.border_color, fill_color=excluded.fill_color,
           font_size=excluded.font_size, opacity=excluded.opacity,
           locked=excluded.locked, z_index=excluded.z_index,
           record_id=excluded.record_id, rotation=excluded.rotation,
           group_id=excluded.group_id, hidden=excluded.hidden,
           collapsed=excluded.collapsed, preset=excluded.preset,
           updated_at=excluded.updated_at",
        placeholders(NODE_COLS.split(',').count())
    )
}

pub fn load_nodes(conn: &Connection, mid: &str) -> CmdResult<Vec<Node>> {
    let mut stmt = conn
        .prepare(&format!("SELECT {NODE_COLS} FROM nodes WHERE mindmap_id=?1 ORDER BY z_index, rowid"))
        .map_err(AppError::from)?;
    let rows = stmt.query_map(params![mid], node_from_row).map_err(AppError::from)?;
    let out = rows.filter_map(|r| r.ok()).collect();
    Ok(out)
}

pub const EDGE_COLS: &str =
    "id,mindmap_id,source_node_id,target_node_id,direction,line_style,path_style,color,width,label,animated,glow,created_at";

/// Canonical edge statements (insert derives placeholders from EDGE_COLS).
pub fn edge_insert_sql() -> String {
    format!(
        "INSERT INTO edges({EDGE_COLS}) VALUES({})",
        placeholders(EDGE_COLS.split(',').count())
    )
}
pub const EDGE_UPDATE_SQL: &str =
    "UPDATE edges SET direction=?2, line_style=?3, path_style=?4, color=?5, width=?6,
     label=?7, animated=?8, glow=?9 WHERE id=?1";
pub const MINDMAP_UPDATE_SQL: &str =
    "UPDATE mindmaps SET
       viewport_x = COALESCE(?2, viewport_x),
       viewport_y = COALESCE(?3, viewport_y),
       zoom = COALESCE(?4, zoom),
       grid_enabled = COALESCE(?5, grid_enabled),
       snap_enabled = COALESCE(?6, snap_enabled),
       updated_at = ?7
     WHERE id=?1";

fn edge_from_row(r: &rusqlite::Row) -> rusqlite::Result<Edge> {
    Ok(Edge {
        id: r.get(0)?,
        mindmap_id: r.get(1)?,
        source_node_id: r.get(2)?,
        target_node_id: r.get(3)?,
        direction: r.get(4)?,
        line_style: r.get(5)?,
        path_style: r.get(6)?,
        color: r.get(7)?,
        width: r.get(8)?,
        label: r.get::<_, Option<String>>(9)?.unwrap_or_default(),
        animated: r.get::<_, i64>(10)? != 0,
        glow: r.get::<_, Option<i64>>(11)?.unwrap_or(0) != 0,
        created_at: r.get(12)?,
    })
}

pub fn load_edges(conn: &Connection, mid: &str) -> CmdResult<Vec<Edge>> {
    let mut stmt = conn
        .prepare(&format!("SELECT {EDGE_COLS} FROM edges WHERE mindmap_id=?1 ORDER BY created_at"))
        .map_err(AppError::from)?;
    let rows = stmt.query_map(params![mid], edge_from_row).map_err(AppError::from)?;
    let out = rows.filter_map(|r| r.ok()).collect();
    Ok(out)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindmapUpdate {
    pub id: String,
    pub viewport_x: Option<f64>,
    pub viewport_y: Option<f64>,
    pub zoom: Option<f64>,
    pub grid_enabled: Option<bool>,
    pub snap_enabled: Option<bool>,
}

#[tauri::command]
pub async fn update_mindmap(st: tauri::State<'_, AppState>, update: MindmapUpdate) -> CmdResult<()> {
    st.with_conn(|conn| {
        conn.execute(
            MINDMAP_UPDATE_SQL,
            params![
                update.id,
                update.viewport_x,
                update.viewport_y,
                update.zoom,
                update.grid_enabled.map(|b| b as i64),
                update.snap_enabled.map(|b| b as i64),
                now_ms()
            ],
        )
        .map_err(AppError::from)?;
        Ok(())
    })
}

#[tauri::command]
pub fn rename_mindmap(st: tauri::State<AppState>, id: String, name: String) -> CmdResult<()> {
    let name = crate::library::validate_name(&name)?;
    st.with_conn(|conn| {
        conn.execute("UPDATE mindmaps SET name=?2, updated_at=?3 WHERE id=?1", params![id, name, now_ms()])
            .map_err(AppError::from)?;
        Ok(())
    })
}

#[tauri::command]
pub fn trash_mindmap(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    st.with_conn(|conn| {
        conn.execute("UPDATE mindmaps SET deleted_at=?1 WHERE id=?2", params![now_ms(), id])
            .map_err(AppError::from)?;
        Ok(())
    })
}

/// Upsert nodes in ONE transaction. Text plain is derived server-side so search
/// stays consistent even if the client forgets to maintain it.
#[tauri::command]
pub async fn save_nodes(st: tauri::State<'_, AppState>, nodes: Vec<Node>) -> CmdResult<Vec<Node>> {
    if nodes.is_empty() {
        return Ok(Vec::new());
    }
    st.with_conn(|conn| {
        let tx = conn.transaction().map_err(AppError::from)?;
        let mut out = Vec::with_capacity(nodes.len());
        let sql = node_upsert_sql();
        for n in &nodes {
            let plain = strip_html(&n.text_html);
            tx.execute(
                &sql,
                params![
                    n.id, n.mindmap_id, n.text_html, plain, n.x, n.y, n.width, n.height,
                    n.shape, n.border_radius, n.border_color, n.fill_color,
                    n.font_size, n.opacity, n.locked as i64, n.z_index, n.record_id,
                    n.rotation, n.group_id, n.hidden as i64, n.collapsed as i64,
                    n.preset, now_ms()
                ],
            )
            .map_err(AppError::from)?;
            let mut saved = n.clone();
            saved.text_plain = plain;
            saved.updated_at = now_ms();
            out.push(saved);
        }
        tx.execute(
            "UPDATE mindmaps SET updated_at=?1 WHERE id=?2",
            params![now_ms(), nodes[0].mindmap_id],
        )
        .map_err(AppError::from)?;
        tx.commit().map_err(AppError::from)?;
        Ok(out)
    })
}

pub fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[tauri::command]
pub async fn delete_nodes(st: tauri::State<'_, AppState>, ids: Vec<String>) -> CmdResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    st.with_conn(|conn| {
        let tx = conn.transaction().map_err(AppError::from)?;
        // Edges referencing deleted nodes are removed explicitly (no FK cascade on TEXT refs).
        let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let p: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let mut all: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() * 2);
        all.extend_from_slice(p.as_slice());
        all.extend_from_slice(p.as_slice());
        tx.execute(
            &format!("DELETE FROM edges WHERE source_node_id IN ({ph}) OR target_node_id IN ({ph})"),
            all.as_slice(),
        )
        .map_err(AppError::from)?;
        tx.execute(&format!("DELETE FROM nodes WHERE id IN ({ph})"), p.as_slice())
            .map_err(AppError::from)?;
        tx.commit().map_err(AppError::from)?;
        Ok(())
    })
}

#[tauri::command]
pub async fn save_edge(st: tauri::State<'_, AppState>, edge: Edge) -> CmdResult<Edge> {
    st.with_conn(|conn| {
        let exists: i64 = conn
            .query_row("SELECT COUNT(*) FROM edges WHERE id=?1", params![edge.id], |r| r.get(0))
            .map_err(AppError::from)?;
        if exists == 0 {
            conn.execute(
                &edge_insert_sql(),
                params![
                    edge.id, edge.mindmap_id, edge.source_node_id, edge.target_node_id,
                    edge.direction, edge.line_style, edge.path_style, edge.color,
                    edge.width, edge.label, edge.animated as i64, edge.glow as i64, now_ms()
                ],
            )
            .map_err(AppError::from)?;
        } else {
            conn.execute(
                EDGE_UPDATE_SQL,
                params![
                    edge.id, edge.direction, edge.line_style, edge.path_style,
                    edge.color, edge.width, edge.label, edge.animated as i64,
                    edge.glow as i64
                ],
            )
            .map_err(AppError::from)?;
        }
        Ok(edge)
    })
}

#[tauri::command]
pub async fn delete_edges(st: tauri::State<'_, AppState>, ids: Vec<String>) -> CmdResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    st.with_conn(|conn| {
        let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let p: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        conn.execute(&format!("DELETE FROM edges WHERE id IN ({ph})"), p.as_slice())
            .map_err(AppError::from)?;
        Ok(())
    })
}






