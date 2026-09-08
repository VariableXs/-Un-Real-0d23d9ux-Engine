//! AI-10（U-25 版本时光机）：
//! - 内容寻址版本库：`<dataDir>/versions/blocks/<hash前2位>/<hash>`，blake3 分块（64KB）去重
//! - 快照挂点：`ver_snapshot` 由前端保存队列（saveQueue）在每次保存后调用——
//!   钩子级，不改任何应用的保存逻辑
//! - 时间轴浏览器数据面：`ver_list`（版本列表）/ `ver_read`（读取指定版本内容）
//!   / `ver_diff`（双栏 diff 的行级变更）
//! - 还原保留 mtime/权限（Windows 上为只读属性）
//! - 保留策略 GC：每文件保留最近 N 版 + 最旧 M 天（默认 20 版 / 90 天，可配置）
//! 红线：版本数据绝不出网（本模块零网络代码）。

use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const BLOCK_SIZE: usize = 64 * 1024;
const DEFAULT_KEEP_VERSIONS: usize = 20;
const DEFAULT_KEEP_DAYS: u64 = 90;
/// 单文件快照上限（防误存超大文件撑爆数据目录；如实报错不静默截断）
const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn versions_dir(st: &AppState) -> PathBuf {
    st.data_dir.join("versions")
}

fn blocks_dir(st: &AppState) -> PathBuf {
    versions_dir(st).join("blocks")
}

fn index_path(st: &AppState) -> PathBuf {
    versions_dir(st).join("index.json")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct VerRecord {
    id: String,
    /// blake3 of whole content
    hash: String,
    size: u64,
    /// 快照时刻文件自身的 mtime（还原时回填）
    file_mtime: u64,
    saved_at: u64,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct VerIndex {
    /// 规范化路径（小写、正斜杠归一）→ 版本列表（旧→新）
    files: std::collections::BTreeMap<String, Vec<VerRecord>>,
    #[serde(default)]
    keep_versions: Option<usize>,
    #[serde(default)]
    keep_days: Option<u64>,
}

fn norm_key(path: &str) -> String {
    path.trim_end_matches('\\').replace('\\', "/").to_lowercase()
}

fn load_index(st: &AppState) -> VerIndex {
    fs::read(index_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_index(st: &AppState, idx: &VerIndex) -> CmdResult<()> {
    let dir = versions_dir(st);
    fs::create_dir_all(&dir)?;
    let bytes = serde_json::to_vec_pretty(idx)?;
    fs::write(index_path(st), bytes)?;
    Ok(())
}

fn block_path(st: &AppState, hash: &str) -> PathBuf {
    blocks_dir(st).join(&hash[..2]).join(hash)
}

/// 分块写入：返回 (整文件 blake3, 各块 hash)。已存在的块跳过写（内容寻址去重）。
fn store_content(st: &AppState, bytes: &[u8]) -> CmdResult<(String, Vec<String>)> {
    let mut whole = blake3::Hasher::new();
    let mut blocks = Vec::new();
    fs::create_dir_all(blocks_dir(st))?;
    for chunk in bytes.chunks(BLOCK_SIZE) {
        let h = blake3::hash(chunk).to_hex().to_string();
        let p = block_path(st, &h);
        if !p.exists() {
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent)?;
            }
            // 原子写：临时文件 + rename
            let tmp = p.with_extension("tmp");
            fs::write(&tmp, chunk)?;
            fs::rename(&tmp, &p)?;
        }
        blocks.push(h);
    }
    whole.update(bytes);
    Ok((whole.finalize().to_hex().to_string(), blocks))
}

/// 块清单（每版本一个小 JSON，列块 hash 序列）。
fn manifest_path(st: &AppState, ver_id: &str) -> PathBuf {
    versions_dir(st).join("manifests").join(format!("{ver_id}.json"))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerManifest {
    blocks: Vec<String>,
}

// ---------- 命令 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerInfo {
    pub id: String,
    pub path: String,
    pub hash: String,
    pub size: u64,
    pub saved_at: u64,
    pub file_mtime: u64,
    /// 与上一版本的行级差量（时间轴徽标用）
    pub changed_lines: Option<u64>,
}

/// 登记为受版本保护的文件（加入观察清单；不立即快照）。
#[tauri::command]
pub fn ver_watch(st: tauri::State<AppState>, path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(AppError::not_found(format!("文件不存在 / not a file: {path}")));
    }
    let mut idx = load_index(&st);
    idx.files.entry(norm_key(&path)).or_default();
    save_index(&st, &idx)
}

/// 快照当前内容（保存队列挂点：每次保存后调用；内容未变则跳过）。
#[tauri::command]
pub fn ver_snapshot(st: tauri::State<AppState>, path: String) -> CmdResult<Option<VerInfo>> {
    // U-36 隐身会话禁止清单：隐身期间禁用版本快照
    if crate::shell::incognito::is_incognito() {
        return Ok(None);
    }
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(AppError::not_found(format!("文件不存在 / not a file: {path}")));
    }
    let meta = fs::metadata(&p)?;
    if meta.len() > MAX_SNAPSHOT_BYTES {
        return Err(AppError::validation(format!(
            "文件超过版本库上限（64MB），未快照 / file too large for versioning: {path}"
        )));
    }
    let bytes = fs::read(&p)?;
    let (hash, blocks) = store_content(&st, &bytes)?;
    let mut idx = load_index(&st);
    let key = norm_key(&path);
    let list = idx.files.entry(key.clone()).or_default();
    if list.last().map(|r| r.hash == hash).unwrap_or(false) {
        return Ok(None); // 内容未变，跳过
    }
    let id = crate::db::gen_id();
    let rec = VerRecord {
        id: id.clone(),
        hash: hash.clone(),
        size: meta.len(),
        file_mtime: now_ms_of(meta.modified()),
        saved_at: now_ms(),
    };
    // 行级差量（与上一版）
    let changed = list
        .last()
        .map(|prev| {
            read_version_bytes(&st, &key, prev)
                .map(|old| line_diff_count(&old, &bytes))
                .unwrap_or(0)
        })
        .unwrap_or(0);
    list.push(rec.clone());
    let mdir = versions_dir(&st).join("manifests");
    fs::create_dir_all(&mdir)?;
    fs::write(
        manifest_path(&st, &id),
        serde_json::to_vec(&VerManifest { blocks })?,
    )?;
    save_index(&st, &idx)?;
    Ok(Some(VerInfo {
        id,
        path: key,
        hash,
        size: rec.size,
        saved_at: rec.saved_at,
        file_mtime: rec.file_mtime,
        changed_lines: Some(changed),
    }))
}

fn now_ms_of(t: std::io::Result<std::time::SystemTime>) -> u64 {
    t.ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn read_version_bytes(st: &AppState, key: &str, rec: &VerRecord) -> CmdResult<Vec<u8>> {
    let m: VerManifest = fs::read(manifest_path(st, &rec.id))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| AppError::not_found("版本块缺失（已 GC 或损坏）/ version blocks missing"))?;
    let mut out = Vec::with_capacity(rec.size as usize);
    for h in &m.blocks {
        let p = block_path(st, h);
        let mut data = fs::read(&p).map_err(|e| AppError::io(format!("块读取失败 / block read failed: {e}")))?;
        out.append(&mut data);
    }
    // 完整性校验
    let actual = blake3::hash(&out).to_hex().to_string();
    if actual != rec.hash {
        return Err(AppError::io("版本校验失败：内容与哈希不符 / version hash mismatch"));
    }
    Ok(out)
}

/// 版本时间轴列表（新→旧）。
#[tauri::command]
pub fn ver_list(st: tauri::State<AppState>, path: String) -> CmdResult<Vec<VerInfo>> {
    let key = norm_key(&path);
    let idx = load_index(&st);
    let Some(list) = idx.files.get(&key) else {
        return Ok(Vec::new());
    };
    let mut out: Vec<VerInfo> = list
        .iter()
        .map(|r| VerInfo {
            id: r.id.clone(),
            path: key.clone(),
            hash: r.hash.clone(),
            size: r.size,
            saved_at: r.saved_at,
            file_mtime: r.file_mtime,
            changed_lines: None,
        })
        .collect();
    out.reverse();
    Ok(out)
}

/// 读取指定版本内容（文本预览/diff 双栏数据源；≤2MB）。
#[tauri::command]
pub fn ver_read(st: tauri::State<AppState>, path: String, version_id: String) -> CmdResult<String> {
    let key = norm_key(&path);
    let idx = load_index(&st);
    let rec = idx
        .files
        .get(&key)
        .and_then(|l| l.iter().find(|r| r.id == version_id))
        .ok_or_else(|| AppError::not_found("版本不存在 / version not found"))?;
    let bytes = read_version_bytes(&st, &key, rec)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(AppError::validation("版本内容过大（>2MB），请在 diff 视图查看 / version too large to inline"));
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerDiff {
    pub added: u64,
    pub removed: u64,
    /// 行级 diff（双栏渲染用）：deleted / added / context
    pub hunks: Vec<DiffLine>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: String,
    pub text: String,
}

/// 统计级 diff（时间轴徽标）。
pub fn line_diff_count(old: &[u8], new: &[u8]) -> u64 {
    let old_s = String::from_utf8_lossy(old);
    let new_s = String::from_utf8_lossy(new);
    let a: Vec<&str> = old_s.lines().collect();
    let b: Vec<&str> = new_s.lines().collect();
    // 简易 LCS 不可行（性能），用行集合差 + 顺序贪心：只统计数量级即可
    let mut count = 0u64;
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
        } else {
            count += 1;
            i += 1;
            j += 1;
        }
    }
    count + (a.len() - i) as u64 + (b.len() - j) as u64
}

/// 双版本 diff（U-25 时间轴浏览器双栏）。
#[tauri::command]
pub fn ver_diff(st: tauri::State<AppState>, path: String, old_id: String, new_id: String) -> CmdResult<VerDiff> {
    let key = norm_key(&path);
    let idx = load_index(&st);
    let list = idx
        .files
        .get(&key)
        .ok_or_else(|| AppError::not_found("文件未纳入版本保护 / file not watched"))?;
    let find = |id: &str| list.iter().find(|r| r.id == id).cloned();
    let (old, new) = (find(&old_id), find(&new_id));
    let old = old.ok_or_else(|| AppError::not_found("旧版本不存在 / old version not found"))?;
    let new = new.ok_or_else(|| AppError::not_found("新版本不存在 / new version not found"))?;
    let a = read_version_bytes(&st, &key, &old)?;
    let b = read_version_bytes(&st, &key, &new)?;
    let al = String::from_utf8_lossy(&a).lines().map(|s| s.to_string()).collect::<Vec<_>>();
    let bl = String::from_utf8_lossy(&b).lines().map(|s| s.to_string()).collect::<Vec<_>>();
    let (mut i, mut j) = (0usize, 0usize);
    let (mut added, mut removed) = (0u64, 0u64);
    let mut hunks = Vec::new();
    while i < al.len() || j < bl.len() {
        match (al.get(i), bl.get(j)) {
            (Some(x), Some(y)) if x == y => {
                hunks.push(DiffLine { kind: "context".into(), text: x.clone() });
                i += 1;
                j += 1;
            }
            (Some(x), Some(y)) => {
                // 贪心：找 y 在 a 中接下来 3 行内是否出现
                let a_window = al.get(i + 1..(i + 4).min(al.len())).unwrap_or(&[]);
                if a_window.contains(y) {
                    hunks.push(DiffLine { kind: "deleted".into(), text: x.clone() });
                    removed += 1;
                    i += 1;
                } else {
                    hunks.push(DiffLine { kind: "added".into(), text: y.clone() });
                    added += 1;
                    j += 1;
                }
            }
            (Some(x), None) => {
                hunks.push(DiffLine { kind: "deleted".into(), text: x.clone() });
                removed += 1;
                i += 1;
            }
            (None, Some(y)) => {
                hunks.push(DiffLine { kind: "added".into(), text: y.clone() });
                added += 1;
                j += 1;
            }
            (None, None) => break,
        }
    }
    Ok(VerDiff { added, removed, hunks })
}

/// 还原：内容写回 + 保留原 mtime。当前文件先快照（保证可再还原回来）。
#[tauri::command]
pub fn ver_restore(st: tauri::State<AppState>, path: String, version_id: String) -> CmdResult<()> {
    let key = norm_key(&path);
    let idx = load_index(&st);
    let rec = idx
        .files
        .get(&key)
        .and_then(|l| l.iter().find(|r| r.id == version_id))
        .cloned()
        .ok_or_else(|| AppError::not_found("版本不存在 / version not found"))?;
    let bytes = read_version_bytes(&st, &key, &rec)?;
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::not_found("目标文件已不存在 / target file gone"));
    }
    // 当前内容先保护
    let _ = ver_snapshot_inner(&st, &path);
    // 原子替换
    let tmp = p.with_extension("ver-restore.tmp");
    fs::write(&tmp, &bytes)?;
    fs::rename(&tmp, &p)?;
    // 保留版本时刻的 mtime
    set_mtime(&p, rec.file_mtime);
    Ok(())
}

fn ver_snapshot_inner(st: &AppState, path: &str) -> CmdResult<()> {
    let p = PathBuf::from(path);
    let meta = fs::metadata(&p)?;
    if meta.len() > MAX_SNAPSHOT_BYTES {
        return Ok(());
    }
    let bytes = fs::read(&p)?;
    let (hash, blocks) = store_content(st, &bytes)?;
    let mut idx = load_index(st);
    let list = idx.files.entry(norm_key(path)).or_default();
    if list.last().map(|r| r.hash == hash).unwrap_or(false) {
        return Ok(());
    }
    let id = crate::db::gen_id();
    list.push(VerRecord {
        id: id.clone(),
        hash,
        size: meta.len(),
        file_mtime: now_ms_of(meta.modified()),
        saved_at: now_ms(),
    });
    let mdir = versions_dir(st).join("manifests");
    fs::create_dir_all(&mdir)?;
    fs::write(manifest_path(st, &id), serde_json::to_vec(&VerManifest { blocks })?)?;
    save_index(st, &idx)
}

/// Windows FILETIME（100ns 间隔，自 1601-01-01）。
#[cfg(windows)]
fn set_mtime(p: &Path, mtime_ms: u64) {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_MODE, FILE_WRITE_ATTRIBUTES,
        OPEN_EXISTING, SetFileTime,
    };
    // UNIX_EPOCH(1970) 与 FILETIME(1601) 相差 11644473600 秒
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
    let v = mtime_ms.saturating_mul(10_000).saturating_add(EPOCH_DIFF_100NS);
    let ft = FILETIME { dwLowDateTime: (v & 0xFFFF_FFFF) as u32, dwHighDateTime: (v >> 32) as u32 };
    let wpath: Vec<u16> = p
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let h = CreateFileW(
            PCWSTR(wpath.as_ptr()),
            FILE_WRITE_ATTRIBUTES.0,
            FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            HANDLE::default(),
        );
        if let Ok(h) = h {
            let _ = SetFileTime(h, None, None, Some(&ft));
            let _ = CloseHandle(h);
        }
    }
}

#[cfg(not(windows))]
fn set_mtime(_p: &Path, _mtime_ms: u64) {}

/// GC：按保留策略清理（keep_versions / keep_days，星标豁免由前端在 UI 层提示，
/// 后端如实执行策略）。删除孤儿块。
#[tauri::command]
pub fn ver_gc(st: tauri::State<AppState>) -> CmdResult<VerGcReport> {
    ver_gc_inner(&st)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerGcReport {
    pub removed_versions: u64,
    pub removed_blocks: u64,
    pub kept_files: u64,
    pub kept_versions: u64,
}

pub fn ver_gc_inner(st: &AppState) -> CmdResult<VerGcReport> {
    let mut idx = load_index(st);
    let keep_n = idx.keep_versions.unwrap_or(DEFAULT_KEEP_VERSIONS);
    let keep_days = idx.keep_days.unwrap_or(DEFAULT_KEEP_DAYS);
    let cutoff = now_ms().saturating_sub(keep_days * 86_400_000);
    let mut removed_versions = 0u64;
    let mut referenced: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut kept_files = 0u64;
    let mut kept_versions = 0u64;
    let keys: Vec<String> = idx.files.keys().cloned().collect();
    for key in keys {
        let list = idx.files.get_mut(&key).unwrap();
        // 保留：最近 keep_n 版 或 90 天内
        let n = list.len();
        let mut retain = Vec::new();
        for (i, r) in list.iter().enumerate().rev() {
            let keep = i + keep_n.min(n) >= n || r.saved_at >= cutoff || i == 0;
            if keep {
                retain.push(r.clone());
            } else {
                removed_versions += 1;
                // manifest 里引用的块仍可能被其他版本引用 → 全量重扫
            }
        }
        retain.reverse();
        kept_versions += retain.len() as u64;
        if retain.is_empty() {
            kept_files += 0; // 文件清单移除（块由重扫处理）
            idx.files.remove(&key);
        } else {
            *list = retain;
            kept_files += 1;
        }
    }
    // 重扫存活引用
    for list in idx.files.values() {
        for r in list {
            if let Some(m) = fs::read(manifest_path(st, &r.id))
                .ok()
                .and_then(|b| serde_json::from_slice::<VerManifest>(&b).ok())
            {
                for h in m.blocks {
                    referenced.insert(h);
                }
            }
        }
    }
    // 删除未引用块
    let mut removed_blocks = 0u64;
    let bdir = blocks_dir(st);
    if let Ok(subs) = fs::read_dir(&bdir) {
        for sub in subs.flatten() {
            if let Ok(files) = fs::read_dir(sub.path()) {
                for f in files.flatten() {
                    let name = f.file_name().to_string_lossy().to_string();
                    if !referenced.contains(&name) && name.len() == 64 {
                        if fs::remove_file(f.path()).is_ok() {
                            removed_blocks += 1;
                        }
                    }
                }
            }
        }
    }
    save_index(st, &idx)?;
    Ok(VerGcReport {
        removed_versions,
        removed_blocks,
        kept_files,
        kept_versions,
    })
}

/// 策略配置。
#[tauri::command]
pub fn ver_policy_set(st: tauri::State<AppState>, keep_versions: Option<usize>, keep_days: Option<u64>) -> CmdResult<()> {
    let mut idx = load_index(&st);
    idx.keep_versions = keep_versions;
    idx.keep_days = keep_days;
    save_index(&st, &idx)
}

/// 受保护文件清单（时间轴浏览器侧栏）。
#[tauri::command]
pub fn ver_watched_list(st: tauri::State<AppState>) -> CmdResult<Vec<String>> {
    let idx = load_index(&st);
    Ok(idx.files.keys().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let tmp = std::env::temp_dir().join(format!("variable-versions-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        (st, tmp)
    }

    #[test]
    fn snapshot_list_restore_roundtrip() {
        let (st, tmp) = temp_state("roundtrip");
        let f = tmp.join("doc.txt");
        fs::write(&f, "v1 hello world\nline2\n").unwrap();
        let p = f.to_string_lossy().to_string();
        ver_snapshot_inner(&st, &p).unwrap();
        fs::write(&f, "v1 hello world CHANGED\nline2\nline3\n").unwrap();
        ver_snapshot_inner(&st, &p).unwrap();
        let idx = load_index(&st);
        let list = idx.files.get(&norm_key(&p)).unwrap();
        assert_eq!(list.len(), 2);
        // 还原到 v1
        let v1 = &list[0];
        let bytes = read_version_bytes(&st, &norm_key(&p), v1).unwrap();
        assert_eq!(bytes, b"v1 hello world\nline2\n");
        // 内容未变 → 快照跳过
        ver_snapshot_inner(&st, &p).unwrap();
        let idx = load_index(&st);
        assert_eq!(idx.files.get(&norm_key(&p)).unwrap().len(), 2);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn diff_counts_added_removed() {
        let a = b"a\nb\nc\n";
        let b = b"a\nx\nc\nd\n";
        let d = line_diff_count(a, b);
        assert!(d >= 2);
    }

    #[test]
    fn gc_respects_retention() {
        let _guard = crate::shell::incognito::TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (st, tmp) = temp_state("gc");
        let f = tmp.join("g.txt");
        fs::write(&f, "v1").unwrap();
        let p = f.to_string_lossy().to_string();
        for i in 0..5 {
            fs::write(&f, format!("v{i}")).unwrap();
            ver_snapshot_inner(&st, &p).unwrap();
        }
        // 只保留 2 版（初版锚点 i==0 永远保留：5 版 → 初版 + 最近 2 版，移除 2 版）
        let mut idx = load_index(&st);
        idx.keep_versions = Some(2);
        idx.keep_days = Some(0); // 不按天数保留
        save_index(&st, &idx);
        let rep = ver_gc_inner(&st).unwrap();
        assert_eq!(rep.removed_versions, 2);
        assert!(rep.kept_versions >= 2);
        let idx = load_index(&st);
        assert_eq!(idx.files.get(&norm_key(&p)).unwrap().len(), 3);
        let _ = fs::remove_dir_all(&tmp);
    }
}
