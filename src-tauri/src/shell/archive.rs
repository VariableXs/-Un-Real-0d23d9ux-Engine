//! AI-10（U-29 存档柜）：
//! - `.vxa` 归档格式：`VXA1` 魔数 + u64 清单长度 + zstd(清单 JSON) + 逐条目 zstd 帧
//! - SHA-256 清单：每条目记录原始大小与哈希；审计 = 逐条解压校验（翻转 1 字节
//!   必须被捕获）；修复 = 提取可抢救条目到新归档并报告损失
//! - 挂载只读浏览：`arch_browse`（虚拟目录列表）+ `arch_read`（单条目预览）
//! - 柜卡片墙数据面：arch_list / arch_remove
//! 红线：归档柜零网络；全部本地文件操作。

use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"VXA1";
/// 单条目读出上限（预览保护）
const READ_LIMIT: u64 = 8 * 1024 * 1024;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn archive_dir(st: &AppState) -> PathBuf {
    st.data_dir.join("archive")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct ArcEntry {
    /// 相对路径（正斜杠）
    path: String,
    size: u64,
    /// 原始内容 SHA-256 hex
    sha256: String,
    /// 压缩帧长度
    frame_len: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct ArcManifest {
    name: String,
    created_at: u64,
    entries: Vec<ArcEntry>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ArcCard {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub files: u64,
    pub bytes: u64,
    /// 磁盘占用
    pub stored_bytes: u64,
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// zstd 压缩（level 3）。
fn zc(data: &[u8]) -> CmdResult<Vec<u8>> {
    zstd::bulk::compress(data, 3).map_err(|e| AppError::io(format!("压缩失败 / compress failed: {e}")))
}

/// zstd 解压（上限容量预估）。
fn zd(data: &[u8], cap: usize) -> CmdResult<Vec<u8>> {
    zstd::bulk::decompress(data, cap).map_err(|e| AppError::io(format!("解压失败 / decompress failed: {e}")))
}

/// 收集源（目录展开 / 文件直取）→ 相对路径列表。
fn collect(src: &Path, base: &Path, out: &mut Vec<(String, PathBuf)>) -> CmdResult<()> {
    let rel = src
        .strip_prefix(base)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let meta = fs::symlink_metadata(src).map_err(|e| AppError::io(format!("读取失败 / stat failed: {e}")))?;
    if meta.is_dir() {
        for e in fs::read_dir(src)?.flatten() {
            collect(&e.path(), base, out)?;
        }
    } else if meta.is_file() {
        out.push((rel, src.to_path_buf()));
    }
    Ok(())
}

/// 创建归档（srcs 支持混合文件/目录）。返回卡片。
#[tauri::command(async)]
pub fn arch_create(st: tauri::State<AppState>, name: String, srcs: Vec<String>) -> CmdResult<ArcCard> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::validation("归档名不能为空 / archive name required"));
    }
    let dir = archive_dir(&st);
    fs::create_dir_all(&dir)?;
    let mut files = Vec::new();
    for s in &srcs {
        let p = PathBuf::from(s);
        if !p.exists() {
            return Err(AppError::not_found(format!("源不存在 / source not found: {s}")));
        }
        collect(&p, p.parent().unwrap_or(Path::new("/")), &mut files)?;
    }
    if files.is_empty() {
        return Err(AppError::validation("无可归档内容 / nothing to archive"));
    }
    let mut entries = Vec::new();
    let mut frames: Vec<Vec<u8>> = Vec::new();
    let mut total = 0u64;
    for (rel, p) in &files {
        let raw = fs::read(p)?;
        total += raw.len() as u64;
        let sha = sha256_hex(&raw);
        let frame = zc(&raw)?;
        entries.push(ArcEntry { path: rel.clone(), size: raw.len() as u64, sha256: sha, frame_len: frame.len() as u64 });
        frames.push(frame);
    }
    let manifest = ArcManifest { name: n.to_string(), created_at: now_ms(), entries };
    let files_count = manifest.entries.len() as u64;
    let mbytes = zc(&serde_json::to_vec(&manifest)?)?;
    let id = format!("{}-{}", now_ms(), crate::db::gen_id());
    let path = dir.join(format!("{id}.vxa"));
    let mut f = fs::File::create(&path)?;
    f.write_all(MAGIC)?;
    f.write_all(&(mbytes.len() as u64).to_le_bytes())?;
    f.write_all(&mbytes)?;
    for fr in frames {
        f.write_all(&fr)?;
    }
    f.sync_all()?;
    let stored = fs::metadata(&path)?.len();
    Ok(ArcCard { id, name: n.to_string(), created_at: manifest.created_at, files: files_count, bytes: total, stored_bytes: stored })
}

/// 读 manifest。
fn read_manifest(path: &Path) -> CmdResult<ArcManifest> {
    let mut f = fs::File::open(path)?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(AppError::validation("归档魔数不符（文件损坏或非 .vxa）/ bad archive magic"));
    }
    let mut len_b = [0u8; 8];
    f.read_exact(&mut len_b)?;
    let mlen = u64::from_le_bytes(len_b) as usize;
    if mlen > 16 * 1024 * 1024 {
        return Err(AppError::validation("清单过大（损坏？）/ manifest too large"));
    }
    let mut mbytes = vec![0u8; mlen];
    f.read_exact(&mut mbytes)?;
    let raw = zd(&mbytes, 64 * 1024 * 1024)?;
    Ok(serde_json::from_slice(&raw)?)
}

fn archive_path(st: &AppState, id: &str) -> CmdResult<PathBuf> {
    let p = archive_dir(st).join(format!("{id}.vxa"));
    if !p.exists() {
        return Err(AppError::not_found("归档不存在 / archive not found"));
    }
    Ok(p)
}

/// 柜卡片墙。
#[tauri::command(async)]
pub fn arch_list(st: tauri::State<AppState>) -> CmdResult<Vec<ArcCard>> {
    let dir = archive_dir(&st);
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".vxa") {
                continue;
            }
            let Ok(m) = read_manifest(&e.path()) else {
                // 损坏卡片也如实列出（bytes=0，UI 标红）
                out.push(ArcCard {
                    id: name.trim_end_matches(".vxa").to_string(),
                    name: name.clone(),
                    created_at: 0,
                    files: 0,
                    bytes: 0,
                    stored_bytes: e.metadata().map(|x| x.len()).unwrap_or(0),
                });
                continue;
            };
            out.push(ArcCard {
                id: name.trim_end_matches(".vxa").to_string(),
                name: m.name,
                created_at: m.created_at,
                files: m.entries.len() as u64,
                bytes: m.entries.iter().map(|x| x.size).sum(),
                stored_bytes: e.metadata().map(|x| x.len()).unwrap_or(0),
            });
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

/// 挂载只读浏览：归档内虚拟目录列表（subpath = 归档内相对目录，"" = 根）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArcNode {
    pub name: String,
    /// 相对路径（正斜杠）
    pub path: String,
    pub kind: String,
    pub size: u64,
}

#[tauri::command(async)]
pub fn arch_browse(st: tauri::State<AppState>, id: String, subpath: String) -> CmdResult<Vec<ArcNode>> {
    let p = archive_path(&st, &id)?;
    let m = read_manifest(&p)?;
    let prefix = if subpath.is_empty() { String::new() } else { format!("{}/", subpath.trim_matches('/')) };
    let mut seen = std::collections::BTreeMap::new();
    for e in &m.entries {
        if !e.path.starts_with(&prefix) {
            continue;
        }
        let rest = &e.path[prefix.len()..];
        if rest.is_empty() {
            continue;
        }
        match rest.split_once('/') {
            Some((dir, _)) => {
                let key = format!("{prefix}{dir}");
                seen.entry(key.clone())
                    .or_insert(ArcNode { name: dir.to_string(), path: key, kind: "dir".into(), size: 0 });
            }
            None => {
                seen.insert(e.path.clone(), ArcNode { name: rest.to_string(), path: e.path.clone(), kind: "file".into(), size: e.size });
            }
        }
    }
    Ok(seen.into_values().collect())
}

/// 读单条目（预览；≤8MB）。
#[tauri::command(async)]
pub fn arch_read(st: tauri::State<AppState>, id: String, entry_path: String) -> CmdResult<Vec<u8>> {
    let p = archive_path(&st, &id)?;
    let m = read_manifest(&p)?;
    let e = m
        .entries
        .iter()
        .find(|x| x.path == entry_path)
        .ok_or_else(|| AppError::not_found("条目不存在 / entry not found"))?;
    if e.size > READ_LIMIT {
        return Err(AppError::validation("条目过大（>8MB）/ entry too large to preview"));
    }
    let raw = read_entry(&p, e)?;
    if sha256_hex(&raw) != e.sha256 {
        return Err(AppError::io("校验失败：条目内容与清单不符 / entry hash mismatch"));
    }
    Ok(raw)
}

fn read_entry(path: &Path, e: &ArcEntry) -> CmdResult<Vec<u8>> {
    let mut f = fs::File::open(path)?;
    // 跳过 magic+len+manifest
    let mut skip = 4 + 8;
    // 重新读 manifest 长度
    let mut f2 = fs::File::open(path)?;
    let mut hdr = [0u8; 12];
    f2.read_exact(&mut hdr)?;
    skip += u64::from_le_bytes(hdr[4..12].try_into().unwrap()) as u64;
    // 跳过之前所有条目的帧
    let m = read_manifest(path)?;
    let mut offset = skip;
    for x in &m.entries {
        if x.path == e.path {
            break;
        }
        offset += x.frame_len;
    }
    use std::io::Seek;
    f.seek(std::io::SeekFrom::Start(offset))?;
    let mut frame = vec![0u8; e.frame_len as usize];
    f.read_exact(&mut frame)?;
    zd(&frame, e.size as usize + 1024)
}

/// 审计：逐条解压校验哈希。返回损坏清单（为空 = 全过）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArcAudit {
    pub checked: u64,
    pub corrupt: Vec<String>,
    pub ok: bool,
}

#[tauri::command(async)]
pub fn arch_audit(st: tauri::State<AppState>, id: String) -> CmdResult<ArcAudit> {
    let p = archive_path(&st, &id)?;
    let m = read_manifest(&p)?;
    let mut corrupt = Vec::new();
    for e in &m.entries {
        let ok = read_entry(&p, e).map(|raw| sha256_hex(&raw) == e.sha256).unwrap_or(false);
        if !ok {
            corrupt.push(e.path.clone());
        }
    }
    let ok = corrupt.is_empty();
    Ok(ArcAudit { checked: m.entries.len() as u64, corrupt, ok })
}

/// 还原（整柜解包到目标目录）。
#[tauri::command(async)]
pub fn arch_extract(st: tauri::State<AppState>, id: String, dest_dir: String) -> CmdResult<u64> {
    let p = archive_path(&st, &id)?;
    let m = read_manifest(&p)?;
    let dest = PathBuf::from(&dest_dir);
    if !dest.is_dir() {
        return Err(AppError::not_found("目标目录不存在 / destination folder not found"));
    }
    let mut n = 0u64;
    for e in &m.entries {
        let raw = read_entry(&p, e)?;
        if sha256_hex(&raw) != e.sha256 {
            return Err(AppError::io(format!("还原中止：{} 校验失败 / hash mismatch, aborting", e.path)));
        }
        let target = dest.join(e.path.replace('/', "\\"));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, &raw)?;
        n += 1;
    }
    Ok(n)
}

/// 修复：提取可抢救条目到新归档（损坏条目被丢弃并报告）。
#[tauri::command(async)]
pub fn arch_repair(st: tauri::State<AppState>, id: String) -> CmdResult<ArcAudit> {
    let p = archive_path(&st, &id)?;
    let m = read_manifest(&p)?;
    let mut good: Vec<(String, Vec<u8>)> = Vec::new();
    let mut corrupt = Vec::new();
    for e in &m.entries {
        match read_entry(&p, e) {
            Ok(raw) if sha256_hex(&raw) == e.sha256 => good.push((e.path.clone(), raw)),
            _ => corrupt.push(e.path.clone()),
        }
    }
    if good.is_empty() {
        return Err(AppError::io("无可抢救条目 / nothing salvageable"));
    }
    // 覆盖写新归档（同 id，旧文件替换）
    let mut entries = Vec::new();
    let mut frames = Vec::new();
    for (rel, raw) in &good {
        let sha = sha256_hex(raw);
        let frame = zc(raw)?;
        entries.push(ArcEntry { path: rel.clone(), size: raw.len() as u64, sha256: sha, frame_len: frame.len() as u64 });
        frames.push(frame);
    }
    let manifest = ArcManifest { name: m.name.clone(), created_at: now_ms(), entries };
    let mbytes = zc(&serde_json::to_vec(&manifest)?)?;
    let tmp = p.with_extension("vxa.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(MAGIC)?;
        f.write_all(&(mbytes.len() as u64).to_le_bytes())?;
        f.write_all(&mbytes)?;
        for fr in frames {
            f.write_all(&fr)?;
        }
        f.sync_all()?;
    }
    fs::rename(&tmp, &p)?;
    let ok = corrupt.is_empty();
    Ok(ArcAudit { checked: good.len() as u64, corrupt, ok })
}

#[tauri::command(async)]
pub fn arch_remove(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    let p = archive_path(&st, &id)?;
    fs::remove_file(&p)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let tmp = std::env::temp_dir().join(format!("variable-archive-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        (st, tmp)
    }

    fn make_tree(tmp: &Path) -> PathBuf {
        let d = tmp.join("data");
        fs::create_dir_all(d.join("sub")).unwrap();
        fs::write(d.join("a.txt"), b"hello archive").unwrap();
        fs::write(d.join("sub").join("b.bin"), vec![3u8; 100_000]).unwrap();
        d
    }

    #[test]
    fn create_browse_extract_roundtrip() {
        let (st, tmp) = temp_state("round");
        let d = make_tree(&tmp);
        // create
        let card = arch_create_inner(&st, "测试柜", vec![d.to_string_lossy().to_string()]).unwrap();
        assert_eq!(card.files, 2);
        assert_eq!(card.bytes, 13 + 100_000); // "hello archive" = 13 字节
        // browse root（顶层保留源目录名 data）
        let nodes = arch_browse_inner(&st, &card.id, "").unwrap();
        assert!(nodes.iter().any(|n| n.name == "data" && n.kind == "dir"));
        // browse data
        let data = arch_browse_inner(&st, &card.id, "data").unwrap();
        assert!(data.iter().any(|n| n.name == "a.txt" && n.kind == "file"));
        assert!(data.iter().any(|n| n.name == "sub" && n.kind == "dir"));
        // browse sub
        let sub = arch_browse_inner(&st, &card.id, "data/sub").unwrap();
        assert!(sub.iter().any(|n| n.name == "b.bin"));
        // read
        let raw = arch_read_inner(&st, &card.id, "data/sub/b.bin").unwrap();
        assert_eq!(raw.len(), 100_000);
        // extract & compare
        let out = tmp.join("out");
        fs::create_dir_all(&out).unwrap();
        let n = arch_extract_inner(&st, &card.id, &out.to_string_lossy()).unwrap();
        assert_eq!(n, 2);
        assert_eq!(fs::read(out.join("data").join("a.txt")).unwrap(), b"hello archive");
        assert_eq!(fs::read(out.join("data").join("sub").join("b.bin")).unwrap(), vec![3u8; 100_000]);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn audit_detects_bitflip_and_repair_salvages() {
        let (st, tmp) = temp_state("audit");
        let d = make_tree(&tmp);
        let card = arch_create_inner(&st, "审计柜", vec![d.to_string_lossy().to_string()]).unwrap();
        let p = archive_dir(&st).join(format!("{}.vxa", card.id));
        // 翻转 1 字节（manifest 之后）
        let mut bytes = fs::read(&p).unwrap();
        let mlen = u64::from_le_bytes(bytes[4..12].try_into().unwrap()) as usize;
        let idx = 12 + mlen + 10;
        bytes[idx] ^= 0xFF;
        fs::write(&p, &bytes).unwrap();
        let a = arch_audit_inner(&st, &card.id).unwrap();
        assert!(!a.ok, "翻转 1 字节必须被审计捕获");
        // 修复：抢救未损坏条目
        let r = arch_repair_inner(&st, &card.id).unwrap();
        assert!(r.checked >= 1);
        // 修复后审计（可能仍有损坏条目被丢弃后的柜）
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---- 内部直连实现（测试不走 tauri::State） ----

    fn arch_create_inner(st: &AppState, name: &str, srcs: Vec<String>) -> CmdResult<ArcCard> {
        let mut files = Vec::new();
        for s in &srcs {
            let p = PathBuf::from(s);
            collect(&p, p.parent().unwrap_or(Path::new("/")), &mut files)?;
        }
        let mut entries = Vec::new();
        let mut frames = Vec::new();
        let mut total = 0u64;
        for (rel, p) in &files {
            let raw = fs::read(p)?;
            total += raw.len() as u64;
            entries.push(ArcEntry { path: rel.clone(), size: raw.len() as u64, sha256: sha256_hex(&raw), frame_len: 0 });
            frames.push(zc(&raw)?);
        }
        for (e, f) in entries.iter_mut().zip(&frames) {
            e.frame_len = f.len() as u64;
        }
        let manifest = ArcManifest { name: name.to_string(), created_at: now_ms(), entries };
        let mbytes = zc(&serde_json::to_vec(&manifest)?)?;
        let id = format!("{}-{}", now_ms(), crate::db::gen_id());
        fs::create_dir_all(archive_dir(st))?;
        let path = archive_dir(st).join(format!("{id}.vxa"));
        let mut f = fs::File::create(&path)?;
        f.write_all(MAGIC)?;
        f.write_all(&(mbytes.len() as u64).to_le_bytes())?;
        f.write_all(&mbytes)?;
        for fr in frames {
            f.write_all(&fr)?;
        }
        let stored = fs::metadata(&path)?.len();
        Ok(ArcCard { id, name: name.to_string(), created_at: manifest.created_at, files: manifest.entries.len() as u64, bytes: total, stored_bytes: stored })
    }

    fn arch_browse_inner(st: &AppState, id: &str, subpath: &str) -> CmdResult<Vec<ArcNode>> {
        let p = archive_path(st, id)?;
        let m = read_manifest(&p)?;
        let prefix = if subpath.is_empty() { String::new() } else { format!("{}/", subpath.trim_matches('/')) };
        let mut seen = std::collections::BTreeMap::new();
        for e in &m.entries {
            if !e.path.starts_with(&prefix) {
                continue;
            }
            let rest = &e.path[prefix.len()..];
            if rest.is_empty() {
                continue;
            }
            match rest.split_once('/') {
                Some((dir, _)) => {
                    let key = format!("{prefix}{dir}");
                    seen.entry(key.clone()).or_insert(ArcNode { name: dir.to_string(), path: key, kind: "dir".into(), size: 0 });
                }
                None => {
                    seen.insert(e.path.clone(), ArcNode { name: rest.to_string(), path: e.path.clone(), kind: "file".into(), size: e.size });
                }
            }
        }
        Ok(seen.into_values().collect())
    }

    fn arch_read_inner(st: &AppState, id: &str, entry_path: &str) -> CmdResult<Vec<u8>> {
        let p = archive_path(st, id)?;
        let m = read_manifest(&p)?;
        let e = m.entries.iter().find(|x| x.path == entry_path).ok_or_else(|| AppError::not_found("条目不存在"))?;
        let raw = read_entry(&p, e)?;
        if sha256_hex(&raw) != e.sha256 {
            return Err(AppError::io("校验失败"));
        }
        Ok(raw)
    }

    fn arch_extract_inner(st: &AppState, id: &str, dest_dir: &str) -> CmdResult<u64> {
        let p = archive_path(st, id)?;
        let m = read_manifest(&p)?;
        let dest = PathBuf::from(dest_dir);
        let mut n = 0u64;
        for e in &m.entries {
            let raw = read_entry(&p, e)?;
            if sha256_hex(&raw) != e.sha256 {
                return Err(AppError::io("hash mismatch"));
            }
            let target = dest.join(e.path.replace('/', "\\"));
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&target, &raw)?;
            n += 1;
        }
        Ok(n)
    }

    fn arch_audit_inner(st: &AppState, id: &str) -> CmdResult<ArcAudit> {
        let p = archive_path(st, id)?;
        let m = read_manifest(&p)?;
        let mut corrupt = Vec::new();
        for e in &m.entries {
            let ok = read_entry(&p, e).map(|raw| sha256_hex(&raw) == e.sha256).unwrap_or(false);
            if !ok {
                corrupt.push(e.path.clone());
            }
        }
        let ok = corrupt.is_empty();
        Ok(ArcAudit { checked: m.entries.len() as u64, corrupt, ok })
    }

    fn arch_repair_inner(st: &AppState, id: &str) -> CmdResult<ArcAudit> {
        let p = archive_path(st, id)?;
        let m = read_manifest(&p)?;
        let mut good: Vec<(String, Vec<u8>)> = Vec::new();
        let mut corrupt = Vec::new();
        for e in &m.entries {
            match read_entry(&p, e) {
                Ok(raw) if sha256_hex(&raw) == e.sha256 => good.push((e.path.clone(), raw)),
                _ => corrupt.push(e.path.clone()),
            }
        }
        if good.is_empty() {
            return Err(AppError::io("无可抢救条目"));
        }
        let mut entries = Vec::new();
        let mut frames = Vec::new();
        for (rel, raw) in &good {
            let frame = zc(raw)?;
            entries.push(ArcEntry { path: rel.clone(), size: raw.len() as u64, sha256: sha256_hex(raw), frame_len: frame.len() as u64 });
            frames.push(frame);
        }
        let manifest = ArcManifest { name: m.name.clone(), created_at: now_ms(), entries };
        let mbytes = zc(&serde_json::to_vec(&manifest)?)?;
        let tmp = p.with_extension("vxa.tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(MAGIC)?;
            f.write_all(&(mbytes.len() as u64).to_le_bytes())?;
            f.write_all(&mbytes)?;
            for fr in frames {
                f.write_all(&fr)?;
            }
            f.sync_all()?;
        }
        fs::rename(&tmp, &p)?;
        let ok = corrupt.is_empty();
        Ok(ArcAudit { checked: good.len() as u64, corrupt, ok })
    }
}
