use crate::db::{gen_id, now_ms};
use crate::error::{AppError, CmdResult};
use crate::media::{ext_of, sanitize_file_name, MAX_IMAGE_BYTES, MAX_VIDEO_BYTES};
use crate::mindmap::{load_edges, load_nodes, strip_html};
use crate::models::*;
use crate::state::AppState;
use rusqlite::params;
use serde::Serialize;
use std::fs;
use std::path::Path;

// ---------- html -> text / markdown ----------

pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let bytes: Vec<char> = html.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '<' {
            let mut tag = String::new();
            i += 1;
            while i < bytes.len() && bytes[i] != '>' {
                tag.push(bytes[i]);
                i += 1;
            }
            i += 1;
            let t = tag.trim().to_lowercase();
            match t.split_whitespace().next().unwrap_or("") {
                "p" | "div" | "br" | "h1" | "h2" | "h3" | "li" | "blockquote" => out.push('\n'),
                _ => {}
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    decoded.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string()
}

fn inline_md(html: &str) -> String {
    html.replace("<strong>", "**").replace("</strong>", "**")
        .replace("<b>", "**").replace("</b>", "**")
        .replace("<em>", "*").replace("</em>", "*")
        .replace("<i>", "*").replace("</i>", "*")
        .replace("<del>", "~~").replace("</del>", "~~")
        .replace("<code>", "`").replace("</code>", "`")
}

/// Minimal Markdown export for the subset of rich text Variable produces.
pub fn html_to_markdown(html: &str) -> String {
    let src = inline_md(html);
    let mut out = String::new();
    let rest = src.as_bytes();
    let mut pos = 0usize;
    let s = String::from_utf8_lossy(rest).to_string();
    let chars: Vec<char> = s.chars().collect();
    while pos < chars.len() {
        if chars[pos] == '<' {
            let start = pos + 1;
            let mut end = start;
            while end < chars.len() && chars[end] != '>' { end += 1; }
            let tag: String = chars[start..end.min(chars.len())].iter().collect::<String>().trim().to_lowercase();
            pos = end + 1;
            let name = tag.split_whitespace().next().unwrap_or("");
            match name {
                "h1" => out.push_str("\n# "),
                "h2" => out.push_str("\n## "),
                "h3" => out.push_str("\n### "),
                "li" => out.push_str("\n- "),
                "blockquote" => out.push_str("\n> "),
                "hr" => out.push_str("\n---\n"),
                "img" => {
                    let alt = extract_attr(&tag, "alt");
                    out.push_str(&format!("![{alt}](local-image)\n"));
                }
                "br" => out.push('\n'),
                "/p" | "/div" | "/h1" | "/h2" | "/h3" | "/li" | "/blockquote" | "p" | "div" => out.push('\n'),
                _ => {}
            }
        } else {
            out.push(chars[pos]);
            pos += 1;
        }
    }
    let joined: Vec<String> = out.lines().map(|l| l.trim_end().to_string()).collect();
    joined.join("\n").trim().to_string() + "\n"
}

fn extract_attr(tag: &str, attr: &str) -> String {
    let pat = format!("{attr}=\"");
    if let Some(i) = tag.to_lowercase().find(&pat) {
        let rest = &tag[i + pat.len()..];
        if let Some(j) = rest.find('"') {
            return rest[..j].to_string();
        }
    }
    String::new()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub count: usize,
}

fn doc_export_body(doc: &Document, format: &str) -> String {
    match format {
        "md" => format!(
            "# {}\n\n{}\n",
            doc.title,
            html_to_markdown(&doc.content_html)
        ),
        "html" => format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title>\
             <body style=\"max-width:760px;margin:40px auto;font-family:'Segoe UI',sans-serif;line-height:1.7;color:#222\">\
             <h1>{}</h1>\n{}</body></html>",
            escape_html(&doc.title), escape_html(&doc.title), doc.content_html
        ),
        "txt" => format!("{}\n\n{}\n", doc.title, html_to_text(&doc.content_html)),
        "json" => serde_json::to_string_pretty(doc).unwrap_or_else(|_| "{}".into()),
        _ => doc.content_text.clone(),
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[tauri::command]
pub fn export_documents(st: tauri::State<AppState>, ids: Vec<String>, format: String, dest_path: String) -> CmdResult<ExportResult> {
    if ids.is_empty() {
        return Err(AppError::validation("未选择要导出的记录 / No documents selected"));
    }
    if !matches!(format.as_str(), "md" | "html" | "txt" | "json") {
        return Err(AppError::validation(format!("不支持的导出格式 / Unsupported format: {format}")));
    }
    st.with_conn(|conn| {
        let mut docs = Vec::new();
        for id in &ids {
            docs.push(crate::library::get_document_public(conn, id)?);
        }
        let dest = Path::new(&dest_path);
        if ids.len() == 1 {
            if dest.exists() {
                return Err(AppError::validation("目标文件已存在 / Target file already exists"));
            }
            let body = doc_export_body(&docs[0], &format);
            fs::write(dest, body).map_err(|e| AppError::io(format!("写入失败 / Write failed: {e}")))?;
            Ok(ExportResult { path: dest_path, count: 1 })
        } else {
            // Multi-export writes into a user-chosen directory with unique names.
            fs::create_dir_all(dest).map_err(AppError::from)?;
            for d in &docs {
                let base = if d.title.is_empty() { "untitled".to_string() } else { sanitize_file_name(&d.title)? };
                let name = format!("{}-{}.{}", base, &d.id[..8], format);
                let p = dest.join(name);
                if !p.exists() {
                    fs::write(&p, doc_export_body(d, &format))
                        .map_err(|e| AppError::io(format!("写入失败 / Write failed: {e}")))?;
                }
            }
            Ok(ExportResult { path: dest_path, count: docs.len() })
        }
    })
}

#[tauri::command]
pub fn export_mindmap_json(st: tauri::State<AppState>, id: String, dest_path: String) -> CmdResult<ExportResult> {
    st.with_conn(|conn| {
        let map_name: String = conn
            .query_row("SELECT name FROM mindmaps WHERE id=?1 AND deleted_at IS NULL", params![id], |r| r.get(0))
            .map_err(|_| AppError::not_found("思维导图不存在 / Mindmap not found"))?;
        let nodes = load_nodes(conn, &id)?;
        let edges = load_edges(conn, &id)?;
        let payload = serde_json::json!({
            "app": "variable-mindmap",
            "formatVersion": 1,
            "name": map_name,
            "exportedAt": now_ms(),
            "nodes": nodes,
            "edges": edges
        });
        let dest = Path::new(&dest_path);
        if dest.exists() {
            return Err(AppError::validation("目标文件已存在 / Target file already exists"));
        }
        let body = serde_json::to_vec_pretty(&payload).map_err(|e| AppError::io(e.to_string()))?;
        fs::write(dest, body).map_err(|e| AppError::io(format!("写入失败 / Write failed: {e}")))?;
        Ok(ExportResult { path: dest_path, count: nodes.len() })
    })
}

// ---------- workspace export / import ----------

#[tauri::command]
pub async fn export_workspace(st: tauri::State<'_, AppState>, dest_dir: String) -> CmdResult<ExportResult> {
    let root = Path::new(&dest_dir);
    if root.exists() && std::fs::read_dir(root).map(|d| d.count()).unwrap_or(0) > 0 {
        return Err(AppError::validation("目标文件夹必须为空 / Destination folder must be empty"));
    }
    fs::create_dir_all(root).map_err(AppError::from)?;
    let media_out = root.join("media");
    fs::create_dir_all(&media_out).map_err(AppError::from)?;
    let dest_dir_out = dest_dir.clone();

    st.with_conn(|conn| {
        use std::collections::HashMap;
        // folders
        let mut stmt = conn.prepare("SELECT id,parent_id,name,sort_order,created_at,updated_at,deleted_at FROM folders WHERE deleted_at IS NULL ORDER BY id").map_err(AppError::from)?;
        let folders: Vec<Folder> = stmt.query_map([], |r| Ok(Folder { id: r.get(0)?, parent_id: r.get(1)?, name: r.get(2)?, sort_order: r.get(3)?, created_at: r.get(4)?, updated_at: r.get(5)?, deleted_at: None })).map_err(AppError::from)?.filter_map(|r| r.ok()).collect();

        // documents full
        let mut stmt = conn.prepare(
            "SELECT d.id,d.folder_id,d.title,d.content_html,d.content_text,d.favorite,d.created_at,d.updated_at FROM documents d WHERE d.deleted_at IS NULL ORDER BY d.id",
        ).map_err(AppError::from)?;
        let mut documents: Vec<Document> = stmt.query_map([], |r| {
            let tags_map: HashMap<String, Vec<String>> = HashMap::new();
            let _ = tags_map;
            Ok(Document {
                id: r.get(0)?, folder_id: r.get(1)?, title: r.get(2)?,
                content_html: r.get(3)?, content_text: r.get(4)?,
                favorite: r.get::<_, i64>(5)? != 0, tags: vec![],
                created_at: r.get(6)?, updated_at: r.get(7)?, deleted_at: None,
            })
        }).map_err(AppError::from)?.filter_map(|r| r.ok()).collect();
        let ids: Vec<String> = documents.iter().map(|d| d.id.clone()).collect();
        let tags = crate::library::doc_tags(conn, &ids)?;
        for d in documents.iter_mut() {
            d.tags = tags.get(&d.id).cloned().unwrap_or_default();
        }

        // mindmaps + graph
        let mut stmt = conn.prepare(&format!("SELECT {} FROM mindmaps WHERE deleted_at IS NULL", crate::mindmap::MAP_COLS)).map_err(AppError::from)?;
        let maps: Vec<Mindmap> = stmt.query_map([], crate::mindmap::map_from_row).map_err(AppError::from)?.filter_map(|r| r.ok()).collect();
        let mut graphs = Vec::new();
        for m in &maps {
            graphs.push(serde_json::json!({
                "mindmap": m,
                "nodes": load_nodes(conn, &m.id)?,
                "edges": load_edges(conn, &m.id)?
            }));
        }

        // media manifest + copy files
        let mut stmt = conn.prepare("SELECT id,file_name,copied,checksum,media_type,size FROM media").map_err(AppError::from)?;
        let mut media_rows: Vec<(String, String, bool, String, String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0, r.get(3)?, r.get(4)?, r.get(5)?)))
            .map_err(AppError::from)?
            .filter_map(|r| r.ok())
            .collect();
        let mut exported_media = Vec::new();
        let mut copied_count = 0usize;
        for (mid, fname, copied, checksum, mtype, size) in media_rows.drain(..) {
            if copied {
                let src = st.media_dir.join(sanitize_file_name(&fname)?);
                if src.exists() {
                    let dest = unique_in(&media_out, &sanitize_file_name(&fname)?);
                    fs::copy(&src, &dest).map_err(AppError::from)?;
                    copied_count += 1;
                    exported_media.push(serde_json::json!({"fileName": dest.file_name().unwrap_or_default().to_string_lossy(), "mediaId": mid, "checksum": checksum, "mediaType": mtype, "size": size}));
                    continue;
                }
            }
            exported_media.push(serde_json::json!({"fileName": fname, "mediaId": mid, "checksum": checksum, "mediaType": mtype, "size": size, "external": true}));
        }

        let payload = serde_json::json!({
            "app": "variable-workspace",
            "formatVersion": 1,
            "exportedAt": now_ms(),
            "folders": folders,
            "documents": documents,
            "mindmaps": graphs,
            "media": exported_media
        });
        let body = serde_json::to_vec_pretty(&payload).map_err(|e| AppError::io(e.to_string()))?;
        fs::write(root.join("workspace.json"), body).map_err(AppError::from)?;
        Ok(ExportResult { path: dest_dir_out, count: copied_count })
    })
}

fn unique_in(dir: &Path, name: &str) -> std::path::PathBuf {
    let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let ext = Path::new(name).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut i = 0u32;
    loop {
        let cand = if i == 0 { dir.join(format!("{stem}{ext}")) } else { dir.join(format!("{stem}-{i}{ext}")) };
        if !cand.exists() { return cand; }
        i += 1;
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub folders: usize,
    pub documents: usize,
    pub mindmaps: usize,
    pub nodes: usize,
    pub edges: usize,
    pub media: usize,
}

const MAX_IMPORT_FILE_BYTES: u64 = if MAX_VIDEO_BYTES > MAX_IMAGE_BYTES { MAX_VIDEO_BYTES } else { MAX_IMAGE_BYTES };

/// Import a workspace bundle. Every row receives a NEW id; nothing is overwritten.
/// File names inside the bundle are sanitized; traversal is rejected.
#[tauri::command]
pub async fn import_workspace(st: tauri::State<'_, AppState>, src_file: String) -> CmdResult<ImportSummary> {
    let src = Path::new(&src_file);
    if !src.exists() {
        return Err(AppError::not_found(format!("文件不存在 / File not found: {src_file}")));
    }
    if ext_of(&src.to_string_lossy()) != "json" {
        return Err(AppError::validation("仅支持导入 Variable 工作区 JSON / Only workspace JSON supported"));
    }
    let raw = fs::read_to_string(src).map_err(|e| AppError::io(e.to_string()))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| AppError::validation(format!("JSON 解析失败 / Parse failed: {e}")))?;
    if v.get("app").and_then(|x| x.as_str()) != Some("variable-workspace") {
        return Err(AppError::validation("不是 Variable 工作区文件 / Not a Variable workspace file"));
    }
    let version = v.get("formatVersion").and_then(|x| x.as_i64()).unwrap_or(0);
    if version != 1 {
        return Err(AppError::validation(format!("不支持的格式版本 / Unsupported formatVersion: {version}")));
    }
    // Size guard for the whole payload.
    if raw.len() as u64 > 512 * 1024 * 1024 {
        return Err(AppError::validation("工作区文件过大 / Workspace too large"));
    }
    let root_dir = src.parent().ok_or_else(|| AppError::validation("无效路径 / Invalid path"))?;

    use std::collections::HashMap;
    let mut folder_map: HashMap<String, String> = HashMap::new();
    let mut doc_map: HashMap<String, String> = HashMap::new();
    let mut counts = ImportSummary { folders: 0, documents: 0, mindmaps: 0, nodes: 0, edges: 0, media: 0 };

    st.with_conn(|conn| {
        let tx = conn.transaction().map_err(AppError::from)?;
        // Folders (two passes to resolve parents).
        if let Some(arr) = v.get("folders").and_then(|x| x.as_array()) {
            for f in arr {
                let old_id = f.get("id").and_then(|x| x.as_str()).unwrap_or_default().to_string();
                let new_id = gen_id();
                folder_map.insert(old_id.clone(), new_id.clone());
                let name = sanitize_file_name_public(f.get("name").and_then(|x| x.as_str()).unwrap_or_default())?;
                let now = now_ms();
                tx.execute(
                    "INSERT INTO folders(id,parent_id,name,sort_order,created_at,updated_at) VALUES(?1,NULL,?2,COALESCE(?3,0),?4,?4)",
                    params![new_id, name, f.get("sortOrder").and_then(|x| x.as_i64()), now],
                ).map_err(AppError::from)?;
                counts.folders += 1;
            }
        }
        // Fix parents with a second pass using the old->new id map.
        if let Some(arr) = v.get("folders").and_then(|x| x.as_array()) {
            for f in arr {
                let old_id = f.get("id").and_then(|x| x.as_str()).unwrap_or("");
                let parent_old = f.get("parentId").and_then(|x| x.as_str());
                if let Some(po) = parent_old {
                    if let (Some(nid), Some(np)) = (folder_map.get(old_id), folder_map.get(po)) {
                        tx.execute("UPDATE folders SET parent_id=?1 WHERE id=?2", params![np, nid])
                            .map_err(AppError::from)?;
                    }
                }
            }
        }
        // Documents
        if let Some(arr) = v.get("documents").and_then(|x| x.as_array()) {
            for d in arr {
                let old_id = d.get("id").and_then(|x| x.as_str()).unwrap_or_default().to_string();
                let new_id = gen_id();
                doc_map.insert(old_id, new_id.clone());
                let title: String = d.get("title").and_then(|x| x.as_str()).unwrap_or_default().chars().take(300).collect();
                let html = d.get("contentHtml").and_then(|x| x.as_str()).unwrap_or_default();
                if html.len() > 8 * 1024 * 1024 {
                    return Err(AppError::validation("记录正文过大 / Document body too large"));
                }
                let text = strip_html(html);
                let folder_new = d.get("folderId").and_then(|x| x.as_str()).and_then(|f| folder_map.get(f));
                let fav = d.get("favorite").and_then(|x| x.as_bool()).unwrap_or(false);
                let now = now_ms();
                tx.execute(
                    "INSERT INTO documents(id,folder_id,title,content_html,content_text,favorite,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?7)",
                    params![new_id, folder_new, title, html, text, fav as i64, now],
                ).map_err(AppError::from)?;
                if let Some(tags) = d.get("tags").and_then(|x| x.as_array()) {
                    let names: Vec<String> = tags.iter()
                        .filter_map(|t| t.as_str())
                        .filter(|t| !t.is_empty() && t.len() <= 40)
                        .map(str::to_string)
                        .collect();
                    crate::library::set_tags_public(&tx, &new_id, &names)?;
                }
                counts.documents += 1;
            }
        }
        // Mindmaps + nodes + edges
        if let Some(maps) = v.get("mindmaps").and_then(|x| x.as_array()) {
            for m in maps {
                let meta = m.get("mindmap").cloned().unwrap_or(serde_json::json!({}));
                let _old_mid = meta.get("id").and_then(|x| x.as_str()).unwrap_or_default().to_string();
                let new_mid = gen_id();

                let name = sanitize_file_name_public(meta.get("name").and_then(|x| x.as_str()).unwrap_or("Imported Map"))?;
                let folder_new = meta.get("folderId").and_then(|x| x.as_str()).and_then(|f| folder_map.get(f));
                let now = now_ms();
                tx.execute(
                    "INSERT INTO mindmaps(id,folder_id,name,viewport_x,viewport_y,zoom,grid_enabled,snap_enabled,created_at,updated_at)
                     VALUES(?1,?2,?3,COALESCE(?4,0),COALESCE(?5,0),COALESCE(?6,1),1,1,?7,?7)",
                    params![new_mid, folder_new, name,
                        meta.get("viewportX").and_then(|x| x.as_f64()),
                        meta.get("viewportY").and_then(|x| x.as_f64()),
                        meta.get("zoom").and_then(|x| x.as_f64()), now],
                ).map_err(AppError::from)?;
                counts.mindmaps += 1;

                let mut local_node_map: HashMap<String, String> = HashMap::new();
                if let Some(nodes) = m.get("nodes").and_then(|x| x.as_array()) {
                    if nodes.len() > 20_000 {
                        return Err(AppError::validation("节点过多 / Too many nodes"));
                    }
                    for n in nodes {
                        let old_nid = n.get("id").and_then(|x| x.as_str()).unwrap_or_default().to_string();
                        let new_nid = gen_id();
                        local_node_map.insert(old_nid.clone(), new_nid.clone());
                        let html = n.get("textHtml").and_then(|x| x.as_str()).unwrap_or_default();
                        let plain = strip_html(html);
                        let rec_old = n.get("recordId").and_then(|x| x.as_str()).and_then(|r| doc_map.get(r));
                        tx.execute(
                            "INSERT INTO nodes(id,mindmap_id,text_html,text_plain,x,y,width,height,shape,border_radius,border_color,fill_color,font_size,opacity,locked,z_index,record_id,updated_at)
                             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,COALESCE(?9,'rounded'),COALESCE(?10,12),COALESCE(?11,'#5b7bd0'),COALESCE(?12,'rgba(13,20,38,0.85)'),COALESCE(?13,14),COALESCE(?14,1),?15,COALESCE(?16,0),?17,?18)",
                            params![
                                new_nid, new_mid, html, plain,
                                n.get("x").and_then(|x| x.as_f64()).unwrap_or(0.0),
                                n.get("y").and_then(|x| x.as_f64()).unwrap_or(0.0),
                                n.get("width").and_then(|x| x.as_f64()).unwrap_or(220.0),
                                n.get("height").and_then(|x| x.as_f64()).unwrap_or(80.0),
                                n.get("shape").and_then(|x| x.as_str()),
                                n.get("borderRadius").and_then(|x| x.as_f64()),
                                n.get("borderColor").and_then(|x| x.as_str()),
                                n.get("fillColor").and_then(|x| x.as_str()),
                                n.get("fontSize").and_then(|x| x.as_f64()),
                                n.get("opacity").and_then(|x| x.as_f64()),
                                n.get("locked").and_then(|x| x.as_bool()).map(|b| b as i64),
                                n.get("zIndex").and_then(|x| x.as_i64()),
                                rec_old, now
                            ],
                        ).map_err(AppError::from)?;
                        counts.nodes += 1;
                    }
                }
                if let Some(edges) = m.get("edges").and_then(|x| x.as_array()) {
                    for e in edges {
                        let s_old = e.get("sourceNodeId").and_then(|x| x.as_str()).unwrap_or_default();
                        let t_old = e.get("targetNodeId").and_then(|x| x.as_str()).unwrap_or_default();
                        let (Some(sn), Some(tn)) = (local_node_map.get(s_old), local_node_map.get(t_old)) else { continue };
                        tx.execute(
                            "INSERT INTO edges(id,mindmap_id,source_node_id,target_node_id,direction,line_style,path_style,color,width,label,animated,created_at)
                             VALUES(?1,?2,?3,?4,COALESCE(?5,'forward'),COALESCE(?6,'solid'),COALESCE(?7,'curve'),COALESCE(?8,'#7f9bd9'),COALESCE(?9,1.5),?10,?11,?12)",
                            params![
                                gen_id(), new_mid, sn, tn,
                                e.get("direction").and_then(|x| x.as_str()),
                                e.get("lineStyle").and_then(|x| x.as_str()),
                                e.get("pathStyle").and_then(|x| x.as_str()),
                                e.get("color").and_then(|x| x.as_str()),
                                e.get("width").and_then(|x| x.as_f64()),
                                e.get("label").and_then(|x| x.as_str()).unwrap_or(""),
                                e.get("animated").and_then(|x| x.as_bool()).map(|b| b as i64).unwrap_or(0),
                                now_ms()
                            ],
                        ).map_err(AppError::from)?;
                        counts.edges += 1;
                    }
                }
            }
        }
        // Media files from the bundle's media/ directory.
        let bundle_media_dir = root_dir.join("media");
        if let Some(items) = v.get("media").and_then(|x| x.as_array()) {
            if items.len() > 10_000 {
                return Err(AppError::validation("媒体过多 / Too many media entries"));
            }
            for mi in items {
                let fname_raw = mi.get("fileName").and_then(|x| x.as_str()).unwrap_or_default();
                if mi.get("external").and_then(|x| x.as_bool()).unwrap_or(false) || fname_raw.is_empty() {
                    continue;
                }
                if fname_raw.contains('/') || fname_raw.contains('\\') || fname_raw.contains("..") {
                    continue; // reject anything that is not a plain file name
                }
                let safe = sanitize_file_name(fname_raw)?;
                let candidate = bundle_media_dir.join(&safe);
                if !candidate.exists() {
                    continue;
                }
                let meta = fs::metadata(&candidate).map_err(AppError::from)?;
                if meta.len() > MAX_IMPORT_FILE_BYTES {
                    return Err(AppError::validation(format!("导入媒体过大 / Imported media too large: {safe}")));
                }
                let dest = unique_in(&st.media_dir, &safe);
                fs::copy(&candidate, &dest).map_err(AppError::from)?;
                let final_name = dest.file_name().unwrap_or_default().to_string_lossy().to_string();
                let mid = gen_id();
                tx.execute(
                    "INSERT INTO media(id,file_name,original_path,copied,checksum,media_type,size,created_at)
                     VALUES(?1,?2,'',1,COALESCE(?3,''),'file',?4,?5)",
                    params![mid, final_name,
                        mi.get("checksum").and_then(|x| x.as_str()),
                        meta.len() as i64, now_ms()],
                ).map_err(AppError::from)?;
                counts.media += 1;
            }
        }
        tx.commit().map_err(AppError::from)?;
        Ok(counts)
    })
}

fn sanitize_file_name_public(name: &str) -> CmdResult<String> {
    let cleaned: String = name.chars().take(120).collect();
    if cleaned.trim().is_empty() {
        return Err(AppError::validation("名称为空 / Empty name"));
    }
    Ok(cleaned.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_"))
}



