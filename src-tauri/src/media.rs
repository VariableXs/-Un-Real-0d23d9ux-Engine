use crate::db::{gen_id, now_ms};
use crate::error::{AppError, CmdResult};
use crate::models::*;
use crate::state::AppState;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "avif"];
const VIDEO_EXTS: &[&str] = &["mp4", "webm", "ogv", "mov", "m4v", "mkv"];
const AUDIO_EXTS: &[&str] = &["mp3", "wav", "ogg", "oga", "m4a", "flac"];
const BLOCKED_EXTS: &[&str] = &["exe", "dll", "bat", "cmd", "com", "scr", "ps1", "vbs", "msi", "js", "jar"];
pub(crate) const MAX_IMAGE_BYTES: u64 = 50 * 1024 * 1024;
pub(crate) const MAX_VIDEO_BYTES: u64 = 500 * 1024 * 1024;
pub(crate) const MAX_FILE_BYTES: u64 = 200 * 1024 * 1024;

pub fn ext_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Strip any directory components; allow only a safe flat file name.
pub fn sanitize_file_name(raw: &str) -> CmdResult<String> {
    let p = Path::new(raw);
    let mut safe_name = String::new();
    for c in p.components() {
        match c {
            Component::Normal(part) => {
                safe_name = part.to_string_lossy().to_string();
            }
            _ => {}
        }
    }
    let safe_name = safe_name.trim();
    if safe_name.is_empty() || safe_name == "." || safe_name == ".." {
        return Err(AppError::validation("无效的文件名 / Invalid file name"));
    }
    if safe_name.len() > 180 {
        return Err(AppError::validation("文件名过长 / File name too long"));
    }
    Ok(safe_name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_"))
}

fn classify(ext: &str) -> Option<&'static str> {
    if IMAGE_EXTS.contains(&ext) {
        Some("image")
    } else if VIDEO_EXTS.contains(&ext) {
        Some("video")
    } else if AUDIO_EXTS.contains(&ext) {
        Some("audio")
    } else if !BLOCKED_EXTS.contains(&ext) {
        Some("file")
    } else {
        None
    }
}

pub(crate) fn size_limit_for(kind: &str) -> u64 {
    match kind {
        "image" => MAX_IMAGE_BYTES,
        "video" => MAX_VIDEO_BYTES,
        _ => MAX_FILE_BYTES,
    }
}

pub fn checksum_file(path: &Path) -> CmdResult<(String, u64)> {
    let mut f = fs::File::open(path).map_err(|e| AppError::io(format!("无法读取文件 / Cannot read file {}: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut total: u64 = 0;
    loop {
        let n = f.read(&mut buf).map_err(AppError::from)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    let hex = format!("{:x}", hasher.finalize());
    Ok((hex[..16.min(hex.len())].to_string(), total))
}

pub fn checksum_file_public(path: &Path) -> (String, u64) {
    checksum_file(path).unwrap_or((String::new(), 0))
}

/// Unique destination under dir, never overwriting existing files.
fn unique_dest(dir: &Path, file_name: &str) -> PathBuf {
    let stem = Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = Path::new(file_name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut i = 0u32;
    loop {
        let candidate = if i == 0 {
            dir.join(format!("{stem}{ext}"))
        } else {
            dir.join(format!("{stem}-{i}{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
        i += 1;
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    pub paths: Vec<String>,
    /// copy | reference
    pub mode: String,
    pub document_id: Option<String>,
    pub node_id: Option<String>,
}

#[tauri::command]
pub async fn import_media(st: tauri::State<'_, AppState>, req: ImportRequest) -> CmdResult<Vec<AttachmentView>> {
    let copy_mode = matches!(req.mode.as_str(), "copy" | "");
    // Phase 1: validate everything and prepare destinations (no writes).
    struct Prepared {
        canonical_src: PathBuf,
        original_name: String,
        kind: &'static str,
        rel_path: String,
        dest_opt: Option<PathBuf>,
        checksum: String,
        size: i64,
    }
    let mut prepared: Vec<Prepared> = Vec::new();
    for raw in &req.paths {
        let src = PathBuf::from(raw);
        let meta = fs::metadata(&src)
            .map_err(|_| AppError::not_found(format!("文件不存在 / File not found: {}", src.display())))?;
        if !meta.is_file() {
            return Err(AppError::validation(format!("不是文件 / Not a file: {}", src.display())));
        }
        let original_name = sanitize_file_name(
            src.file_name().map(|s| s.to_string_lossy()).unwrap_or_default().as_ref(),
        )?;
        let ext = ext_of(&original_name);
        let kind = classify(&ext)
            .ok_or_else(|| AppError::validation(format!("不支持的文件类型 / Unsupported file type: .{ext}")))?;
        if meta.len() > size_limit_for(kind) {
            return Err(AppError::validation(format!(
                "文件过大（上限 {} MB）/ File too large (max {} MB)",
                size_limit_for(kind) / 1024 / 1024,
                size_limit_for(kind) / 1024 / 1024
            )));
        }
        let canonical_src =
            src.canonicalize().map_err(|e| AppError::io(format!("路径解析失败 / Path resolve failed: {e}")))?;
        let (rel_path, dest_opt, checksum, size) = if copy_mode {
            let dest = unique_dest(&st.media_dir, &original_name);
            (
                format!("media/{}", dest.file_name().unwrap_or_default().to_string_lossy()),
                Some(dest),
                String::new(),
                meta.len() as i64,
            )
        } else {
            let (sum, _) = checksum_file(&canonical_src)?;
            (canonical_src.to_string_lossy().to_string(), None, sum, meta.len() as i64)
        };
        prepared.push(Prepared { canonical_src, original_name, kind, rel_path, dest_opt, checksum, size });
    }
    // Phase 2: copy files; roll back partial copies on failure.
    let mut copied_files: Vec<PathBuf> = Vec::new();
    for p in prepared.iter_mut() {
        if let Some(dest) = p.dest_opt.clone() {
            let outcome: Result<(String, u64), AppError> = match fs::copy(&p.canonical_src, &dest) {
                Ok(sz) => match checksum_file(&dest) {
                    Ok((sum, _)) => Ok((sum, sz)),
                    Err(e) => Err(e),
                },
                Err(e) => Err(match e.kind() {
                    std::io::ErrorKind::PermissionDenied => {
                        AppError::io("权限不足，无法复制文件 / Permission denied copying file")
                    }
                    std::io::ErrorKind::StorageFull => AppError::io("磁盘空间不足 / Disk full"),
                    _ => AppError::io(format!("复制失败 / Copy failed: {e}")),
                }),
            };
            match outcome {
                Ok((sum, sz)) => {
                    p.checksum = sum;
                    p.size = sz as i64;
                    copied_files.push(dest.clone());
                }
                Err(e) => {
                    for f in &copied_files {
                        let _ = fs::remove_file(f);
                    }
                    return Err(e);
                }
            }
        }
    }
    // Phase 3: single transaction for all rows.
    st.with_conn(|conn| {
        let tx = conn.transaction().map_err(AppError::from)?;
        let mut views = Vec::new();
        for p in &prepared {
            let mid = gen_id();
            tx.execute(
                "INSERT INTO media(id,file_name,original_path,copied,checksum,media_type,size,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    mid,
                    p.original_name,
                    p.canonical_src.to_string_lossy(),
                    p.dest_opt.is_some() as i64,
                    p.checksum,
                    p.kind,
                    p.size,
                    now_ms()
                ],
            )
            .map_err(AppError::from)?;
            let aid = gen_id();
            tx.execute(
                "INSERT INTO attachments(id,document_id,node_id,media_id,display_name,rel_path,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![aid, req.document_id, req.node_id, mid, p.original_name, p.rel_path, now_ms()],
            )
            .map_err(AppError::from)?;
            views.push(AttachmentView {
                id: aid,
                media_id: mid,
                media_type: p.kind.to_string(),
                display_name: p.original_name.clone(),
                rel_path: p.rel_path.clone(),
                abs_path: absolute_media_path(&st, &p.rel_path),
                original_path: p.canonical_src.to_string_lossy().to_string(),
                copied: p.dest_opt.is_some(),
            });
        }
        tx.commit().map_err(AppError::from)?;
        Ok(views)
    })
}

pub fn unlink_media_files(st: &AppState, files: &[(String, String)]) {
    for (rel, _name) in files {
        if let Some(name) = rel.strip_prefix("media/") {
            if !name.contains("..") && !name.contains('/') && !name.contains('\\') {
                let p = st.media_dir.join(name);
                let _ = fs::remove_file(p);
            }
        }
    }
}

pub fn absolute_media_path(st: &AppState, rel_path: &str) -> String {
    if rel_path.starts_with("media/") && !rel_path[6..].contains(['/', '\\']) {
        st.media_dir.join(&rel_path[6..]).to_string_lossy().to_string()
    } else {
        rel_path.to_string()
    }
}

#[tauri::command]
pub fn import_data_url(st: tauri::State<AppState>, data_url: String, suggested_name: Option<String>) -> CmdResult<AttachmentView> {
    let (meta, b64) = data_url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(","))
        .ok_or_else(|| AppError::validation("无效的数据 URL / Invalid data URL"))?;
    let mime = meta.split(';').next().unwrap_or("");
    let kind = match mime {
        "image/png" => ("png", "image"),
        "image/jpeg" => ("jpg", "image"),
        "image/webp" => ("webp", "image"),
        "image/gif" => ("gif", "image"),
        _ => return Err(AppError::validation("仅支持粘贴图片 / Only image paste supported")),
    };
    let bytes = base64_decode(b64).ok_or_else(|| AppError::validation("无法解码粘贴数据 / Paste decode failed"))?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(AppError::validation("图片过大 / Image too large"));
    }
    let base_name = suggested_name.unwrap_or_else(|| format!("paste-{}.{}", now_ms(), kind.0));
    let safe = sanitize_file_name(&base_name)?;
    let dest = unique_dest(&st.media_dir, &safe);
    fs::write(&dest, &bytes).map_err(AppError::from)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sum = format!("{:x}", hasher.finalize())[..16].to_string();
    let rel = format!("media/{}", dest.file_name().unwrap_or_default().to_string_lossy());

    st.with_conn(|conn| {
        let mid = gen_id();
        conn.execute(
            "INSERT INTO media(id,file_name,original_path,copied,checksum,media_type,size,created_at)
             VALUES(?1,?2,'',1,?3,'image',?4,?5)",
            params![mid, safe, sum, bytes.len() as i64, now_ms()],
        )
        .map_err(AppError::from)?;
        let aid = gen_id();
        conn.execute(
            "INSERT INTO attachments(id,document_id,node_id,media_id,display_name,rel_path,created_at)
             VALUES(?1,NULL,NULL,?2,?3,?4,?5)",
            params![aid, mid, safe, rel, now_ms()],
        )
        .map_err(AppError::from)?;
        Ok(AttachmentView {
            id: aid,
            media_id: mid,
            media_type: "image".into(),
            display_name: safe,
            rel_path: rel.clone(),
            abs_path: absolute_media_path(&st, &rel),
            original_path: String::new(),
            copied: true,
        })
    })
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const TBL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let s: Vec<u8> = s.bytes().filter(|b| !b" \t\r\n=".contains(b)).collect();
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for b in s {
        let v = TBL.iter().position(|t| *t == b)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

#[tauri::command]
pub fn attach_media(
    st: tauri::State<AppState>,
    media_id: String,
    document_id: Option<String>,
    node_id: Option<String>,
) -> CmdResult<Attachment> {
    st.with_conn(|conn| {
        let (name, rel): (String, String) = conn
            .query_row("SELECT file_name, CASE WHEN copied=1 THEN 'media/'||file_name ELSE original_path END FROM media WHERE id=?1", params![media_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|_| AppError::not_found("媒体不存在 / Media not found"))?;
        let aid = gen_id();
        conn.execute(
            "INSERT INTO attachments(id,document_id,node_id,media_id,display_name,rel_path,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![aid, document_id, node_id, media_id, name, rel, now_ms()],
        )
        .map_err(AppError::from)?;
        Ok(Attachment { id: aid, document_id, node_id, media_id, display_name: name, rel_path: rel, created_at: now_ms() })
    })
}

#[tauri::command]
pub fn list_attachments(st: tauri::State<AppState>, document_id: Option<String>, node_id: Option<String>) -> CmdResult<Vec<AttachmentView>> {
    st.with_conn(|conn| list_attachments_inner(conn, &st, document_id, node_id))
}

pub(crate) fn list_attachments_inner(conn: &Connection, st: &AppState, document_id: Option<String>, node_id: Option<String>) -> CmdResult<Vec<AttachmentView>> {
    // Filter semantics: return rows matching the PROVIDED scope only.
    // document scope → that doc's attachments; node scope → that node's;
    // both provided → either link qualifies; none → empty.
    let sql = "SELECT a.id,a.media_id,m.media_type,a.display_name,a.rel_path,m.original_path,m.copied
               FROM attachments a JOIN media m ON m.id=a.media_id
               WHERE (?1 IS NOT NULL AND a.document_id = ?1)
                  OR (?2 IS NOT NULL AND a.node_id = ?2)
               ORDER BY a.created_at";
    let mut stmt = conn.prepare(sql).map_err(AppError::from)?;
    let rows = stmt
        .query_map(params![document_id, node_id], |r| {
            let rel: String = r.get(4)?;
            let orig: String = r.get(5)?;
            Ok(AttachmentView {
                id: r.get(0)?,
                media_id: r.get(1)?,
                media_type: r.get(2)?,
                display_name: r.get(3)?,
                abs_path: String::new(),
                rel_path: rel.clone(),
                original_path: orig,
                copied: r.get::<_, i64>(6)? != 0,
            })
        })
        .map_err(AppError::from)?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();
    Ok(rows
        .into_iter()
        .map(|mut a| {
            a.abs_path = absolute_media_path(st, &a.rel_path);
            a
        })
        .collect())
}

#[tauri::command]
pub fn resolve_media_path(st: tauri::State<AppState>, attachment_id: String, new_path: String) -> CmdResult<AttachmentView> {
    let src = PathBuf::from(&new_path);
    let meta = fs::metadata(&src).map_err(|_| AppError::not_found("新路径不存在 / New path does not exist"))?;
    let name = sanitize_file_name(src.file_name().map(|s| s.to_string_lossy()).unwrap_or_default().as_ref())?;
    st.with_conn(|conn| {
        let (media_id, old_rel): (String, String) = conn
            .query_row("SELECT media_id, rel_path FROM attachments WHERE id=?1", params![attachment_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|_| AppError::not_found("附件不存在 / Attachment not found"))?;
        let mtype: String = conn.query_row("SELECT media_type FROM media WHERE id=?1", params![media_id], |r| r.get(0)).map_err(AppError::from)?;
        let ext = ext_of(&name);
        let ok_ext = match mtype.as_str() {
            "image" => IMAGE_EXTS.contains(&ext.as_str()),
            "video" => VIDEO_EXTS.contains(&ext.as_str()),
            "audio" => AUDIO_EXTS.contains(&ext.as_str()),
            _ => !BLOCKED_EXTS.contains(&ext.as_str()),
        };
        if !ok_ext {
            return Err(AppError::validation("新文件的类型与原媒体不符 / Replacement file type mismatch"));
        }
        let limit = size_limit_for(&mtype);
        if meta.len() > limit {
            return Err(AppError::validation("新文件过大 / Replacement file too large"));
        }
        let new_rel;
        if old_rel.starts_with("media/") {
            let dest = unique_dest(&st.media_dir, &name);
            fs::copy(&src, &dest).map_err(AppError::from)?;
            new_rel = format!("media/{}", dest.file_name().unwrap_or_default().to_string_lossy());
            conn.execute("UPDATE media SET file_name=?2, copied=1, size=?3 WHERE id=?1", params![media_id, name, meta.len() as i64])
                .map_err(AppError::from)?;
        } else {
            new_rel = src.canonicalize().map_err(|e| AppError::io(e.to_string()))?.to_string_lossy().to_string();
            conn.execute("UPDATE media SET original_path=?2, size=?3 WHERE id=?1", params![media_id, new_rel, meta.len() as i64])
                .map_err(AppError::from)?;
        }
        conn.execute("UPDATE attachments SET rel_path=?2, display_name=?3 WHERE id=?1", params![attachment_id, new_rel, name])
            .map_err(AppError::from)?;
        Ok(AttachmentView {
            id: attachment_id,
            media_id,
            media_type: mtype,
            display_name: name,
            rel_path: new_rel.clone(),
            abs_path: absolute_media_path(&st, &new_rel),
            original_path: String::new(),
            copied: new_rel.starts_with("media/"),
        })
    })
}

#[tauri::command]
pub fn delete_media(st: tauri::State<AppState>, media_id: String) -> CmdResult<()> {
    let files = st.with_conn(|conn| {
        let rel: Option<String> = conn
            .query_row("SELECT CASE WHEN copied=1 THEN 'media/'||file_name ELSE '' END FROM media WHERE id=?1", params![media_id], |r| r.get(0))
            .map_err(|_| AppError::not_found("媒体不存在 / Media not found"))?;
        let tx = conn.transaction().map_err(AppError::from)?;
        tx.execute("DELETE FROM attachments WHERE media_id=?1", params![media_id]).map_err(AppError::from)?;
        tx.execute("DELETE FROM media WHERE id=?1", params![media_id]).map_err(AppError::from)?;
        tx.commit().map_err(AppError::from)?;
        Ok(rel.map(|r| vec![(r, String::new())]).unwrap_or_default())
    })?;
    unlink_media_files(&st, &files);
    Ok(())
}


