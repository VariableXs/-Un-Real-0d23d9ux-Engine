//! AI-09 文件操作组（fileops 模块，承 APEX AI-4 / SUMMIT AI-1 领地）。
//! M-21 校验和 / Z-33 重复报告 / Z-34 空间分析 / Z-32 批量重命名 / Z-35 发送到 /
//! Z-31 网络驱动器 / M-25 文件锁定侦探 / M-23 压缩包只读浏览 / M-27 目录监控哨兵。
//!
//! 红线（承原计划）：
//! - 任何文件操作只读优先；写操作必须显式确认 + 可撤销；绝不静默删除。
//! - 重复文件报告「只报告不删除」。
//! - M-25 无强拆按钮；查不到占用者如实返回空列表（前端显示「系统未披露占用者」）。
//! - M-27 SMB/网络路径入口校验拒绝；哨兵上限 5。
//! - 压缩包只读清单不落盘解压（单文件提取除外，走 data/tmp/archive 临时区）。

use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

fn long_path(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if s.len() >= 240 && p.is_absolute() && !s.starts_with(r"\\?\") && !s.starts_with(r"\\") {
        PathBuf::from(format!(r"\\?\{s}"))
    } else {
        p.to_path_buf()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ===========================================================================
// M-21 校验和工具（分块流式 + 进度事件 + 可取消）
// ===========================================================================

/// 取消标志表：opId → 已请求取消。
fn cancel_flags() -> &'static Mutex<HashSet<String>> {
    static FLAGS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    FLAGS.get_or_init(|| Mutex::new(HashSet::new()))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChecksumProgress {
    pub op_id: String,
    pub done: u64,
    pub total: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecksumResult {
    pub op_id: String,
    pub algo: String,
    pub hex: String,
    pub bytes: u64,
    pub cancelled: bool,
}

/// 算法归一（前端传 md5|sha1|sha256|blake3）。
fn norm_algo(algo: &str) -> CmdResult<&'static str> {
    match algo.to_lowercase().as_str() {
        "md5" => Ok("md5"),
        "sha1" => Ok("sha1"),
        "sha256" => Ok("sha256"),
        "blake3" => Ok("blake3"),
        _ => Err(AppError::validation("不支持的算法 / Unsupported algorithm")),
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 流式哈希核心（1MB 分块；每 ~8MB 或完成时回报进度；检查取消标志）。
fn hash_file(
    app: &tauri::AppHandle,
    path: &Path,
    algo: &str,
    op_id: &str,
) -> CmdResult<(String, u64, bool)> {
    use blake3::Hasher as B3;
    use sha1::Digest;
    use sha2::Sha256;

    let lp = long_path(path);
    let meta = fs::metadata(&lp).map_err(|e| AppError::io(format!("读取失败 / Stat failed: {e}")))?;
    if !meta.is_file() {
        return Err(AppError::validation("目标不是文件 / Not a file"));
    }
    let total = meta.len();
    let mut f = fs::File::open(&lp)?;
    let mut buf = vec![0u8; 1024 * 1024];
    let mut done: u64 = 0;
    let mut last_report: u64 = 0;

    let mut b3 = B3::new();
    let mut md5 = md5::Md5::new();
    let mut sha1 = sha1::Sha1::new();
    let mut sha256 = Sha256::new();

    let send = |done: u64| {
        let _ = tauri::Emitter::emit(
            app,
            "checksum://progress",
            &ChecksumProgress {
                op_id: op_id.to_string(),
                done,
                total,
            },
        );
    };

    loop {
        if cancel_flags().lock().map(|s| s.contains(op_id)).unwrap_or(false) {
            return Ok((String::new(), done, true));
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        match algo {
            "blake3" => {
                b3.update(chunk);
            }
            "md5" => {
                md5.update(chunk);
            }
            "sha1" => {
                sha1.update(chunk);
            }
            _ => {
                sha256.update(chunk);
            }
        }
        done += n as u64;
        if done - last_report >= 8 * 1024 * 1024 {
            last_report = done;
            send(done);
        }
    }
    let out = match algo {
        "blake3" => b3.finalize().to_hex().to_string(),
        "md5" => hex(&md5.finalize()),
        "sha1" => hex(&sha1.finalize()),
        _ => hex(&sha256.finalize()),
    };
    send(total);
    Ok((out, done, false))
}

/// M-21：计算文件校验和（进度事件 `checksum://progress`；可经 checksum_cancel 取消）。
#[tauri::command]
pub fn checksum(
    _st: tauri::State<AppState>,
    app: tauri::AppHandle,
    path: String,
    algo: String,
    op_id: String,
) -> CmdResult<ChecksumResult> {
    let algo = norm_algo(&algo)?;
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::not_found(format!("文件不存在 / File not found: {path}")));
    }
    cancel_flags().lock().map(|mut s| s.remove(&op_id)).ok();
    let (out, bytes, cancelled) = hash_file(&app, &p, algo, &op_id)?;
    Ok(ChecksumResult {
        op_id: op_id.clone(),
        algo: algo.to_string(),
        hex: out,
        bytes,
        cancelled,
    })
}

/// M-21：请求取消（CancelToken 模式，复用 bench 先例语义）。
#[tauri::command]
pub fn checksum_cancel(op_id: String) -> CmdResult<()> {
    cancel_flags().lock().map(|mut s| s.insert(op_id)).ok();
    Ok(())
}

// ===========================================================================
// Z-33 重复文件报告器（大小分组 → 4KB 抽样 → 全量确认；只报告不删除）
// ===========================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeFile {
    pub path: String,
    pub size: u64,
    pub modified: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeGroup {
    pub hash: String,
    pub size: u64,
    pub files: Vec<DupeFile>,
    /// 该组冗余字节数 = size × (files-1)。
    pub wasted: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeReport {
    pub groups: Vec<DupeGroup>,
    pub scanned: u32,
    pub truncated: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupeProgress {
    pub phase: String, // collect | sample | confirm | done
    pub done: u32,
    pub total: u32,
}

fn blake3_file(p: &Path, limit: Option<u64>) -> CmdResult<String> {
    let lp = long_path(p);
    let mut f = fs::File::open(&lp)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 512 * 1024];
    let mut left = limit.unwrap_or(u64::MAX);
    loop {
        let want = buf.len().min(left as usize);
        if want == 0 {
            break;
        }
        let n = f.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        left -= n as u64;
    }
    Ok(h.finalize().to_hex().to_string())
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>, scanned: &mut u32, truncated: &mut bool) {
    const MAX_FILES: usize = 100_000;
    if *truncated {
        return;
    }
    let Ok(rd) = fs::read_dir(long_path(dir)) else { return };
    for item in rd.flatten() {
        if out.len() >= MAX_FILES {
            *truncated = true;
            return;
        }
        let p = item.path();
        // 不跟随符号链接/junction 目录：Windows 用户目录存在 junction 环
        //（Application Data 链等），跟随会重复扫描灌满上限甚至无限递归。
        // DirEntry::file_type() 不跟随链接，junction 在其结果中 is_symlink()==true
        //（与下方 scan_dir 的 symlink_metadata 语义一致）。
        let is_link = item.file_type().map(|ft| ft.is_symlink()).unwrap_or(false);
        if p.is_dir() {
            if !is_link {
                walk_files(&p, out, scanned, truncated);
            }
        } else {
            out.push(p);
            *scanned += 1;
        }
    }
}

/// Z-33：扫描目录树找出内容重复的文件（≥ minSize）。只报告不删除（红线）。
#[tauri::command]
pub fn dupe_scan(
    _st: tauri::State<AppState>,
    app: tauri::AppHandle,
    path: String,
    min_size: Option<u64>,
) -> CmdResult<DupeReport> {
    let root = PathBuf::from(&path);
    if !long_path(&root).is_dir() {
        return Err(AppError::not_found(format!("目录不存在 / Directory not found: {path}")));
    }
    let min_size = min_size.unwrap_or(1);
    let send = |phase: &str, done: u32, total: u32| {
        let _ = tauri::Emitter::emit(
            &app,
            "dupe://progress",
            &DupeProgress { phase: phase.into(), done, total },
        );
    };

    // ---- 阶段 1：收集 + 按大小分组 ----
    let mut files = Vec::new();
    let mut scanned = 0u32;
    let mut truncated = false;
    walk_files(&root, &mut files, &mut scanned, &mut truncated);
    send("collect", files.len() as u32, files.len() as u32);

    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for f in files {
        let Ok(meta) = fs::metadata(long_path(&f)) else { continue };
        if meta.len() >= min_size {
            by_size.entry(meta.len()).or_default().push(f);
        }
    }
    let candidates: Vec<(u64, Vec<PathBuf>)> = by_size
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .collect();
    let cand_total: u32 = candidates.iter().map(|(_, v)| v.len() as u32).sum();

    // ---- 阶段 2：4KB 抽样哈希（过滤掉抽样即不同的） ----
    let mut done = 0u32;
    let mut sample_groups: Vec<(u64, Vec<(String, PathBuf, u64)>)> = Vec::new();
    for (size, group) in &candidates {
        let mut by_sample: HashMap<String, Vec<(PathBuf, u64)>> = HashMap::new();
        for f in group {
            let Ok(s) = blake3_file(f, Some(4096)) else { continue };
            by_sample.entry(s).or_default().push((f.clone(), fs::metadata(long_path(f)).map(|m| m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0)).unwrap_or(0)));
            done += 1;
            if done % 25 == 0 {
                send("sample", done, cand_total);
            }
        }
        for (_, v) in by_sample {
            if v.len() > 1 {
                let key = v
                    .first()
                    .and_then(|(f, _)| blake3_file(f, Some(4096)).ok())
                    .unwrap_or_default();
                sample_groups.push((
                    *size,
                    v.into_iter().map(|(f, m)| (key.clone(), f, m)).collect(),
                ));
            }
        }
    }
    send("sample", done, cand_total);

    // ---- 阶段 3：全量哈希确认 ----
    let mut groups: Vec<DupeGroup> = Vec::new();
    let mut confirmed = 0u32;
    let confirm_total: u32 = sample_groups.iter().map(|(_, v)| v.len() as u32).sum();
    for (size, members) in &sample_groups {
        let mut by_hash: HashMap<String, Vec<DupeFile>> = HashMap::new();
        for (_, f, m) in members {
            let Ok(h) = blake3_file(f, None) else { continue };
            let entry = DupeFile {
                path: f.to_string_lossy().to_string(),
                size: *size,
                modified: *m,
            };
            by_hash.entry(h).or_default().push(entry);
            confirmed += 1;
            if confirmed % 5 == 0 {
                send("confirm", confirmed, confirm_total);
            }
        }
        for (h, v) in by_hash {
            if v.len() > 1 {
                let wasted = size * (v.len() as u64 - 1);
                groups.push(DupeGroup { hash: h, size: *size, files: v, wasted });
            }
        }
    }
    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted));
    send("done", 1, 1);
    Ok(DupeReport { groups, scanned, truncated })
}

// ===========================================================================
// Z-34 空间分析器（目录树统计 + 截断上限）
// ===========================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceNode {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub file_count: u32,
    pub dir_count: u32,
    pub children: Vec<SpaceNode>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceReport {
    pub root: SpaceNode,
    pub scanned: u32,
    pub truncated: bool,
}

const SPACE_MAX_ENTRIES: usize = 200_000;

fn scan_dir(node_path: &Path, counter: &mut u32, truncated: &mut bool) -> SpaceNode {
    let mut node = SpaceNode {
        name: node_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| node_path.to_string_lossy().to_string()),
        path: node_path.to_string_lossy().to_string(),
        size: 0,
        file_count: 0,
        dir_count: 0,
        children: Vec::new(),
    };
    let Ok(rd) = fs::read_dir(long_path(node_path)) else { return node };
    let mut dirs: Vec<PathBuf> = Vec::new();
    for item in rd.flatten() {
        if *truncated {
            break;
        }
        *counter += 1;
        if *counter > SPACE_MAX_ENTRIES as u32 {
            *truncated = true;
            break;
        }
        let p = item.path();
        let Ok(meta) = fs::symlink_metadata(long_path(&p)) else { continue };
        if meta.is_dir() {
            node.dir_count += 1;
            dirs.push(p);
        } else if meta.is_file() {
            node.file_count += 1;
            node.size += meta.len();
        }
    }
    for d in dirs {
        if *truncated {
            break;
        }
        let child = scan_dir(&d, counter, truncated);
        node.size += child.size;
        node.file_count += child.file_count;
        node.dir_count += child.dir_count;
        node.children.push(child);
    }
    node.children.sort_by(|a, b| b.size.cmp(&a.size));
    node
}

/// Z-34：空间分析（目录树 + 大小/文件数统计；20 万项截断如实返回）。
#[tauri::command]
pub fn space_scan(_st: tauri::State<AppState>, path: String) -> CmdResult<SpaceReport> {
    let root = PathBuf::from(&path);
    if !long_path(&root).is_dir() {
        return Err(AppError::not_found(format!("目录不存在 / Directory not found: {path}")));
    }
    let mut counter = 0u32;
    let mut truncated = false;
    let node = scan_dir(&root, &mut counter, &mut truncated);
    Ok(SpaceReport { root: node, scanned: counter, truncated })
}

// ===========================================================================
// Z-32 批量重命名（规则管线 + 预览冲突 + 撤销日志）
// ===========================================================================

/// 规则管线（顺序应用）。前端按序组管线：替换 → 序号 → 大小写 → 扩展名。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RenameRule {
    /// 查找替换（find 为空 = 不启用）。
    Replace { find: String, replace: String },
    /// 序号：{start} 开始、步长 {step}、补零到 {pad} 位。
    Number { start: i64, step: i64, pad: u32 },
    /// 大小写：upper | lower（仅作用于主名，不动扩展名）。
    Case { mode: String },
    /// 扩展名替换：from 为空 = 全部；to 为空 = 去掉扩展名。
    Ext { from: String, to: String },
}

/// 规则管线核心（纯函数，供单测）。index = 文件在批内序号（序号规则用）。
pub fn apply_rules(name: &str, rules: &[RenameRule], index: usize) -> String {
    let mut cur = name.to_string();
    for r in rules {
        match r {
            RenameRule::Replace { find, replace } => {
                if !find.is_empty() {
                    cur = cur.replace(find.as_str(), replace.as_str());
                }
            }
            RenameRule::Number { start, step, pad } => {
                let n = start + *step * index as i64;
                let s = if *pad > 0 {
                    format!("{:0width$}", n, width = *pad as usize)
                } else {
                    n.to_string()
                };
                // 序号插入主名末尾（扩展名之前）
                let dot = cur.rfind('.');
                match dot {
                    Some(d) if d > 0 => {
                        cur = format!("{}{}{}{}", &cur[..d], s, "", &cur[d..]);
                    }
                    _ => cur = format!("{cur}{s}"),
                }
            }
            RenameRule::Case { mode } => {
                let dot = cur.rfind('.');
                match dot {
                    Some(d) if d > 0 => match mode.as_str() {
                        "upper" => cur = format!("{}{}", cur[..d].to_uppercase(), &cur[d..]),
                        "lower" => cur = format!("{}{}", cur[..d].to_lowercase(), &cur[d..]),
                        _ => {}
                    },
                    _ => match mode.as_str() {
                        "upper" => cur = cur.to_uppercase(),
                        "lower" => cur = cur.to_lowercase(),
                        _ => {}
                    },
                }
            }
            RenameRule::Ext { from, to } => {
                let dot = cur.rfind('.');
                let has_ext = matches!(dot, Some(d) if d > 0);
                let ext_cur = if has_ext {
                    cur[dot.unwrap_or(0)..].to_lowercase()
                } else {
                    String::new()
                };
                let want = if from.is_empty() || ext_cur == format!(".{}", from.to_lowercase()) {
                    if to.is_empty() {
                        String::new()
                    } else {
                        format!(".{}", to.trim_start_matches('.'))
                    }
                } else {
                    ext_cur
                };
                if has_ext {
                    cur = format!("{}{}", &cur[..dot.unwrap_or(0)], want);
                } else if !want.is_empty() {
                    cur = format!("{cur}{want}");
                }
            }
        }
    }
    cur
}

fn sanitize(name: &str) -> CmdResult<String> {
    let t = name.trim();
    if t.is_empty() || t == "." || t == ".." {
        return Err(AppError::validation("名称无效 / Invalid name"));
    }
    Ok(t.to_string())
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RenameItem {
    pub path: String,
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewRow {
    pub path: String,
    pub old_name: String,
    pub new_name: String,
    /// 目标与现存文件或同批其他目标撞名。
    pub conflict: bool,
    pub reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreview {
    pub rows: Vec<RenamePreviewRow>,
}

/// Z-32：预览（不落盘）。冲突 = 目标已存在（非自身）或同批重复。
#[tauri::command]
pub fn batch_rename_preview(
    _st: tauri::State<AppState>,
    items: Vec<RenameItem>,
    rules: Vec<RenameRule>,
) -> CmdResult<RenamePreview> {
    let mut rows = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut dup_in_batch: HashSet<String> = HashSet::new();
    // 先算出全部新名，找出批内重复
    let mut news: Vec<(usize, String)> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let n = apply_rules(&it.name, &rules, i);
        dup_in_batch.insert(n.clone());
        news.push((i, n));
    }
    for (i, it) in items.iter().enumerate() {
        let new_name = sanitize(news[i].1.as_str())?;
        let dir = Path::new(&it.path)
            .parent()
            .ok_or_else(|| AppError::validation("无效路径 / Invalid path"))?;
        let dest = dir.join(&new_name);
        let same = new_name == it.name;
        let mut conflict = false;
        let mut reason = String::new();
        if !same && dest.exists() {
            conflict = true;
            reason = "target-exists".into();
        }
        if !dup_in_batch.is_empty() && news.iter().filter(|(j, n)| n == &new_name && *j != i).count() > 0 {
            conflict = true;
            reason = "batch-duplicate".into();
        }
        let _ = seen.insert(new_name.clone());
        rows.push(RenamePreviewRow {
            path: it.path.clone(),
            old_name: it.name.clone(),
            new_name,
            conflict,
            reason,
        });
    }
    Ok(RenamePreview { rows })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameApplyResult {
    pub renamed: u32,
    pub undo_id: String,
}

fn undo_dir(st: &AppState) -> PathBuf {
    st.data_dir.join("rename_undo")
}

/// Z-32：执行批量重命名（预览已确认；写撤销点日志，可整体回滚）。
#[tauri::command]
pub fn batch_rename_apply(
    st: tauri::State<AppState>,
    items: Vec<RenameItem>,
    rules: Vec<RenameRule>,
) -> CmdResult<RenameApplyResult> {
    // 先全量预演（全部合法才执行，避免半批）
    let preview = batch_rename_preview(st.clone(), items.clone(), rules.clone())?;
    if preview.rows.iter().any(|r| r.conflict) {
        return Err(AppError::validation("存在冲突项，未执行 / Conflicts present, aborted"));
    }
    let ts = now_ms();
    let undo_id = ts.to_string();
    let mut log: Vec<(String, String)> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let new_name = apply_rules(&it.name, &rules, i);
        if new_name == it.name {
            continue;
        }
        let src = Path::new(&it.path);
        let parent = src.parent().ok_or_else(|| AppError::validation("无效路径 / Invalid path"))?;
        let dest = parent.join(&new_name);
        fs::rename(long_path(src), long_path(&dest))?;
        log.push((it.path.clone(), dest.to_string_lossy().to_string()));
    }
    let dir = undo_dir(&st);
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join(format!("{undo_id}.json")),
        serde_json::to_string(&log)?,
    )?;
    Ok(RenameApplyResult { renamed: log.len() as u32, undo_id })
}

#[derive(Deserialize)]
struct UndoLog(Vec<(String, String)>);

/// Z-32：撤销最近一次批量重命名（按撤销点日志反向恢复）。
#[tauri::command]
pub fn batch_rename_undo(st: tauri::State<AppState>, undo_id: String) -> CmdResult<u32> {
    if !undo_id.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::validation("无效撤销点 / Invalid undo id"));
    }
    let f = undo_dir(&st).join(format!("{undo_id}.json"));
    if !f.exists() {
        return Err(AppError::not_found("撤销点不存在 / Undo point not found"));
    }
    let raw = fs::read_to_string(&f)?;
    let log: UndoLog = serde_json::from_str(&raw)?;
    let mut n = 0u32;
    // 反向恢复：仅当新名存在且旧名不存在（中途被用户改过的项如实跳过）
    for (old, new) in log.0.iter().rev() {
        let np = Path::new(new);
        let op = Path::new(old);
        if np.exists() && !op.exists() {
            fs::rename(long_path(np), long_path(op))?;
            n += 1;
        }
    }
    let _ = fs::remove_file(&f);
    Ok(n)
}

// ===========================================================================
// Z-35 发送到（系统 SendTo 合并 + 自定义目标 + 最近目标）
// ===========================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendToItem {
    /// system = 系统 SendTo 目录项（.lnk，执行走 shellExecute）；custom = 自定义目录。
    pub kind: String,
    pub name: String,
    /// system = .lnk 绝对路径；custom = 目录绝对路径。
    pub target: String,
}

fn sendto_custom_file(st: &AppState) -> PathBuf {
    st.data_dir.join("sendto_custom.json")
}

fn system_sendto_dir() -> Option<PathBuf> {
    std::env::var("APPDATA")
        .ok()
        .map(|a| Path::new(&a).join(r"Microsoft\Windows\SendTo"))
        .filter(|p| p.is_dir())
}

/// Z-35：合并系统 SendTo 目录与自定义目标。
#[tauri::command]
pub fn sendto_list(st: tauri::State<AppState>) -> CmdResult<Vec<SendToItem>> {
    let mut out = Vec::new();
    if let Some(sys) = system_sendto_dir() {
        if let Ok(rd) = fs::read_dir(&sys) {
            for item in rd.flatten() {
                let name = item.file_name().to_string_lossy().to_string();
                let ext = item.path().extension().map(|e| e.to_string_lossy().to_lowercase());
                if matches!(ext.as_deref(), Some("lnk") | Some("zipdest") | Some("mydocs")) {
                    out.push(SendToItem {
                        kind: "system".into(),
                        name: name.trim_end_matches(".lnk").trim_end_matches(".LNK").to_string(),
                        target: item.path().to_string_lossy().to_string(),
                    });
                }
            }
        }
    }
    let f = sendto_custom_file(&st);
    let customs: Vec<String> = fs::read_to_string(&f)
        .ok()
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_default();
    for c in customs {
        if Path::new(&c).is_dir() {
            out.push(SendToItem {
                kind: "custom".into(),
                name: Path::new(&c)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| c.clone()),
                target: c,
            });
        }
    }
    Ok(out)
}

fn read_custom(st: &AppState) -> Vec<String> {
    fs::read_to_string(sendto_custom_file(st))
        .ok()
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_default()
}

fn write_custom(st: &AppState, list: &[String]) -> CmdResult<()> {
    fs::write(sendto_custom_file(st), serde_json::to_string(list)?)?;
    Ok(())
}

/// Z-35：添加自定义发送目标（目录）。上限 10。
#[tauri::command]
pub fn sendto_custom_add(st: tauri::State<AppState>, path: String) -> CmdResult<Vec<String>> {
    let p = Path::new(&path);
    if !p.is_dir() {
        return Err(AppError::not_found("目标必须是文件夹 / Target must be a folder"));
    }
    let mut list = read_custom(&st);
    if list.len() >= 10 {
        return Err(AppError::validation("自定义目标已达上限 10 / Custom target limit (10) reached"));
    }
    if !list.contains(&path) {
        list.push(path);
        write_custom(&st, &list)?;
    }
    Ok(list)
}

/// Z-35：移除自定义发送目标。
#[tauri::command]
pub fn sendto_custom_remove(st: tauri::State<AppState>, path: String) -> CmdResult<Vec<String>> {
    let mut list = read_custom(&st);
    list.retain(|p| p != &path);
    write_custom(&st, &list)?;
    Ok(list)
}

/// Z-35：把文件/文件夹复制到自定义目标（防覆盖：重名自动保留两者后缀，绝不静默覆盖）。
#[tauri::command]
pub fn sendto_copy(_st: tauri::State<AppState>, src: String, target_dir: String) -> CmdResult<String> {
    let from = PathBuf::from(&src);
    let to = long_path(Path::new(&target_dir));
    if !from.exists() {
        return Err(AppError::not_found("源不存在 / Source not found"));
    }
    if !to.is_dir() {
        return Err(AppError::not_found("目标文件夹不存在 / Target folder not found"));
    }
    let name = from
        .file_name()
        .ok_or_else(|| AppError::validation("无效路径 / Invalid path"))?
        .to_string_lossy()
        .to_string();
    let mut dest = to.join(&name);
    if dest.exists() {
        // 保留两者：自动加 -N 后缀（不覆盖）
        let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let ext = Path::new(&name).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let mut i = 1u32;
        loop {
            let cand = to.join(format!("{stem}-{i}{ext}"));
            if !cand.exists() {
                dest = cand;
                break;
            }
            i += 1;
        }
    }
    // 递归复制（fileops 本地实现，不跨模块借用私有函数）
    copy_tree_local(&from, &dest)?;
    Ok(dest.to_string_lossy().to_string())
}

/// 本地递归复制（sendto_copy 专用；错误带上下文）。
fn copy_tree_local(src: &Path, dest: &Path) -> CmdResult<()> {
    if src.is_dir() {
        fs::create_dir_all(dest)
            .map_err(|e| AppError::io(format!("创建目录失败 / mkdir failed: {e}")))?;
        for item in fs::read_dir(src).map_err(|e| AppError::io(e.to_string()))?.flatten() {
            copy_tree_local(&item.path(), &dest.join(item.file_name()))?;
        }
    } else {
        fs::copy(src, dest).map_err(|e| {
            AppError::io(format!("复制失败 / Copy failed {}: {e}", src.display()))
        })?;
    }
    Ok(())
}

// ===========================================================================
// Z-31 网络驱动器（GetLogicalDriveStrings + GetDriveType；离线灰化依据）
// ===========================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetDrive {
    pub letter: String,
    pub path: String,
    /// fixed | removable | network | cdrom | ramdisk
    pub kind: String,
    /// 网络盘当前是否可达（离线灰化）。
    pub available: bool,
    /// 映射到的 UNC 路径（仅网络盘；读取失败 = null）。
    pub unc: Option<String>,
}

#[cfg(windows)]
#[tauri::command]
pub fn net_drives(_st: tauri::State<AppState>) -> CmdResult<Vec<NetDrive>> {
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDriveStringsW};
    use windows::Win32::System::WindowsProgramming::{
        DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
    };

    let mut buf = [0u16; 512];
    let n = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) } as usize;
    if n == 0 || n > buf.len() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < n {
        let end = buf[i..n].iter().position(|&c| c == 0).map(|p| i + p).unwrap_or(n);
        if end == i {
            break;
        }
        let root: String = String::from_utf16_lossy(&buf[i..end]);
        i = end + 1;
        let root_w: Vec<u16> = root.encode_utf16().chain([0]).collect();
        let dt = unsafe { GetDriveTypeW(windows::core::PCWSTR(root_w.as_ptr())) };
        let kind = match dt {
            DRIVE_FIXED => "fixed",
            DRIVE_REMOVABLE => "removable",
            DRIVE_REMOTE => "network",
            DRIVE_CDROM => "cdrom",
            DRIVE_RAMDISK => "ramdisk",
            _ => "unknown",
        };
        if kind == "unknown" {
            continue;
        }
        // 离线探测只对网络盘做（固定盘 is_dir 零成本；网络盘离线时可能阻塞数秒，放线程池命令里可接受）
        let available = if kind == "network" {
            Path::new(&root).is_dir()
        } else {
            true
        };
        let unc = if kind == "network" {
            wnet_connection(&root)
        } else {
            None
        };
        let letter = root.trim_end_matches(':').to_string();
        out.push(NetDrive { letter, path: root, kind: kind.into(), available, unc });
    }
    Ok(out)
}

/// WNetGetConnectionW：映射盘符 → UNC。
#[cfg(windows)]
fn wnet_connection(root: &str) -> Option<String> {
    // WNetGetConnectionW 位于 Win32_NetworkManagement_Mpr（未启用该 feature 时不可用）。
    // 为避免新增 feature 面积，这里用 QueryDosDeviceW 推断（映射盘 = \Device\LanmanRedirector\...）。
    use windows::core::PWSTR;
    let mut root_w: Vec<u16> = root.encode_utf16().chain([0]).collect();
    let mut buf = [0u16; 512];
    let n = unsafe {
        windows::Win32::Storage::FileSystem::QueryDosDeviceW(
            PWSTR(root_w.as_mut_ptr()),
            Some(&mut buf),
        )
    } as usize;
    if n == 0 {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
    // "\Device\LanmanRedirector\;Z:0000000000\server\share" → "\server\share"
    if let Some(pos) = s.find(';') {
        if let Some(next) = s[pos..].find('\\') {
            return Some(s[pos + next..].trim_end_matches('\0').to_string());
        }
    }
    None
}

#[cfg(not(windows))]
#[tauri::command]
pub fn net_drives(_st: tauri::State<AppState>) -> CmdResult<Vec<NetDrive>> {
    Ok(Vec::new())
}

// ===========================================================================
// M-25 文件锁定侦探（Restart Manager；无强拆按钮红线）
// ===========================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockHolder {
    pub pid: u32,
    pub name: String,
    pub title: String,
}

#[cfg(windows)]
#[tauri::command]
pub fn who_locks(path: String) -> CmdResult<Vec<LockHolder>> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::System::RestartManager::{
        RmEndSession, RmGetList, RmRegisterResources, RmStartSession, RM_PROCESS_INFO,
        CCH_RM_SESSION_KEY,
    };

    if !Path::new(&path).exists() {
        return Err(AppError::not_found("路径不存在 / Path not found"));
    }
    unsafe {
        let mut session: u32 = 0;
        let mut key = [0u16; (CCH_RM_SESSION_KEY + 1) as usize];
        let hr = RmStartSession(&mut session, 0, PWSTR(key.as_mut_ptr()));
        if hr.is_err() {
            return Err(AppError::io("无法启动锁定会话 / Cannot start lock session"));
        }
        let path_w: Vec<u16> = path.encode_utf16().chain([0]).collect();
        let arr = [PCWSTR(path_w.as_ptr())];
        let hr = RmRegisterResources(session, Some(&arr), None, None);
        if hr.is_err() {
            RmEndSession(session);
            return Err(AppError::io("无法注册资源 / Cannot register resource"));
        }
        let mut needed: u32 = 0;
        let mut count: u32 = 0;
        let mut reboot: u32 = 0;
        // 第一次探测容量
        let _ = RmGetList(session, &mut needed, &mut count, None, &mut reboot);
        if needed == 0 {
            RmEndSession(session);
            return Ok(Vec::new()); // 系统未披露占用者（如实空列表）
        }
        let mut infos = vec![RM_PROCESS_INFO::default(); needed as usize];
        let count_mut = &mut needed.clone();
        let hr = RmGetList(
            session,
            count_mut,
            &mut count,
            Some(infos.as_mut_ptr()),
            &mut reboot,
        );
        RmEndSession(session);
        if hr.is_err() {
            return Ok(Vec::new()); // 权限不足等 → 如实空列表（前端显示未披露文案）
        }
        let mut out = Vec::new();
        for info in infos.iter().take(count as usize) {
            let pid = info.Process.dwProcessId;
            let name = wstr_to_string(&info.strAppName);
            let title = window_title_of_pid(pid).unwrap_or_default();
            out.push(LockHolder { pid, name, title });
        }
        out.sort_by(|a, b| a.pid.cmp(&b.pid));
        Ok(out)
    }
}

#[cfg(windows)]
fn wstr_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

#[cfg(windows)]
fn window_title_of_pid(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::{HWND, LPARAM, BOOL};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};

    struct Ctx {
        pid: u32,
        titles: Vec<String>,
    }
    unsafe extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam.0 as *mut Ctx);
        let mut wpid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut wpid)) };
        if wpid == ctx.pid && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            let mut buf = [0u16; 256];
            let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
            if n > 0 {
                let t = String::from_utf16_lossy(&buf[..n as usize]);
                if !t.is_empty() && !ctx.titles.contains(&t) {
                    ctx.titles.push(t);
                }
            }
        }
        windows::Win32::Foundation::BOOL::from(true)
    }
    let mut ctx = Ctx { pid, titles: Vec::new() };
    unsafe {
        EnumWindows(Some(proc), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.titles.into_iter().next()
}

#[cfg(not(windows))]
#[tauri::command]
pub fn who_locks(path: String) -> CmdResult<Vec<LockHolder>> {
    let _ = path;
    Err(AppError::validation("仅 Windows 支持 / Windows only"))
}

// ===========================================================================
// M-23 压缩包只读浏览（zip 优先；7z/tar 如实「暂不支持」）
// ===========================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    pub name: String,
    /// 去掉尾部 / 的完整内部路径。
    pub inner_path: String,
    pub size: u64,
    pub compressed_size: u64,
    pub is_dir: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveListing {
    pub path: String,
    pub entries: Vec<ArchiveEntry>,
}

fn open_zip(path: &Path) -> CmdResult<zip::ZipArchive<std::fs::File>> {
    let f = fs::File::open(long_path(path))
        .map_err(|e| AppError::io(format!("打开失败 / Open failed: {e}")))?;
    zip::ZipArchive::new(f).map_err(|_| {
        AppError::validation("损坏或不支持的压缩包（当前仅支持 zip）/ Corrupt or unsupported archive (zip only)")
    })
}

fn unsupported_format(path: &Path) -> Option<CmdResult<ArchiveListing>> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
    match ext.as_deref() {
        Some("7z") | Some("tar") | Some("gz") | Some("rar") => Some(Err(AppError::validation(
            "该格式暂不支持（当前仅支持 zip）/ Format not yet supported (zip only)",
        ))),
        _ => None,
    }
}

/// M-23：压缩包只读清单（不落盘解压）。
#[tauri::command]
pub fn archive_ls(_st: tauri::State<AppState>, path: String) -> CmdResult<ArchiveListing> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::not_found("文件不存在 / File not found"));
    }
    if let Some(err) = unsupported_format(&p) {
        return err;
    }
    let mut zip = open_zip(&p)?;
    let mut entries = Vec::new();
    for i in 0..zip.len() {
        let e = zip
            .by_index(i)
            .map_err(|_| AppError::io("读取条目失败 / Read entry failed"))?;
        let inner = e.name().to_string();
        let is_dir = e.is_dir();
        let inner_path = inner.trim_end_matches('/').to_string();
        let name = inner_path
            .rsplit('/')
            .next()
            .unwrap_or(&inner_path)
            .to_string();
        entries.push(ArchiveEntry {
            name,
            inner_path,
            size: e.size(),
            compressed_size: e.compressed_size(),
            is_dir,
        });
    }
    Ok(ArchiveListing { path, entries })
}

/// zip-slip 防护：拒绝绝对路径与 .. 穿越。
fn safe_inner(inner: &str) -> CmdResult<PathBuf> {
    let p = Path::new(inner);
    if p.is_absolute() || inner.contains("..") || inner.contains('\\') {
        return Err(AppError::validation("包内路径非法 / Illegal entry path"));
    }
    Ok(p.to_path_buf())
}

/// M-23：解压单个条目到临时区（data/tmp/archive/，启动时清理）→ 返回解出文件路径。
/// 前端随后 openPath；关闭后由下次启动清理（sysmaint 临时区语义）。
#[tauri::command]
pub fn archive_extract_one(
    st: tauri::State<AppState>,
    archive: String,
    inner_path: String,
) -> CmdResult<String> {
    let ap = PathBuf::from(&archive);
    if !ap.exists() {
        return Err(AppError::not_found("压缩包不存在 / Archive not found"));
    }
    let safe = safe_inner(&inner_path)?;
    let mut zip = open_zip(&ap)?;
    let mut found = None;
    for i in 0..zip.len() {
        let e = zip
            .by_index(i)
            .map_err(|_| AppError::io("读取条目失败 / Read entry failed"))?;
        if e.name().trim_end_matches('/') == inner_path.trim_end_matches('/') && !e.is_dir() {
            found = Some(i);
            break;
        }
    }
    let idx = found.ok_or_else(|| AppError::not_found("包内文件未找到 / Entry not found"))?;
    // 临时区：data/tmp/archive/<archive文件名>/<内部相对路径>
    let stem = ap.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "archive".into());
    let out_dir = st.data_dir.join("tmp").join("archive").join(stem).join(safe.parent().unwrap_or(Path::new("")));
    fs::create_dir_all(&out_dir)?;
    let file_name = safe
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| AppError::validation("包内路径非法 / Illegal entry path"))?;
    let out_path = out_dir.join(&file_name);
    let mut e = zip
        .by_index(idx)
        .map_err(|_| AppError::io("读取条目失败 / Read entry failed"))?;
    let mut out = fs::File::create(&out_path)?;
    std::io::copy(&mut e, &mut out)?;
    Ok(out_path.to_string_lossy().to_string())
}

/// 启动时清理压缩包临时区（lib.rs setup 调用；一行最小 diff）。
pub fn startup_cleanup(st: &AppState) {
    let tmp = st.data_dir.join("tmp").join("archive");
    if tmp.exists() {
        let _ = fs::remove_dir_all(&tmp);
    }
}

// ===========================================================================
// M-27 目录监控哨兵（ReadDirectoryChangesW 每哨兵一线程；1s 节流合并）
// ===========================================================================

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SentinelCfg {
    pub id: String,
    pub path: String,
    pub enabled: bool,
    /// 静默时段 [start, end)（小时，0-23；255 = 无）。
    pub quiet_start: u8,
    pub quiet_end: u8,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SentinelEvent {
    pub id: String,
    pub path: String,
    pub changes: Vec<SentinelChange>,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SentinelChange {
    /// added | removed | renamed | modified
    pub kind: String,
    pub name: String,
}

const SENTINEL_MAX: usize = 5;

fn sentinel_file(st: &AppState) -> PathBuf {
    st.data_dir.join("sentinels.json")
}

fn read_sentinels(st: &AppState) -> Vec<SentinelCfg> {
    fs::read_to_string(sentinel_file(st))
        .ok()
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_default()
}

fn write_sentinels(st: &AppState, list: &[SentinelCfg]) -> CmdResult<()> {
    fs::write(sentinel_file(st), serde_json::to_string(list)?)?;
    Ok(())
}

/// 事件合并（1s 窗口语义：同 kind+name 去重；纯函数供单测）。
pub fn merge_changes(raw: &[(u32, String)]) -> Vec<SentinelChange> {
    let kind_of = |a: u32| -> &'static str {
        match a {
            1 => "added",     // FILE_ACTION_ADDED
            2 => "removed",   // FILE_ACTION_REMOVED
            3 => "modified",  // FILE_ACTION_MODIFIED
            4 => "renamed",   // FILE_ACTION_RENAMED_OLD_NAME
            5 => "renamed",   // FILE_ACTION_RENAMED_NEW_NAME
            _ => "modified",
        }
    };
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut out = Vec::new();
    for (a, n) in raw {
        let k = kind_of(*a).to_string();
        if seen.insert((k.clone(), n.clone())) {
            out.push(SentinelChange { kind: k, name: n.clone() });
        }
    }
    out
}

/// 静默时段判定（支持跨午夜，如 22→6）。纯函数供单测。
pub fn in_quiet_hours(hour: u8, start: u8, end: u8) -> bool {
    if start == 255 || end == 255 || start == end {
        return false;
    }
    if start < end {
        hour >= start && hour < end
    } else {
        // 跨午夜：22..6
        hour >= start || hour < end
    }
}

struct SentinelRuntime {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

fn sentinel_runtimes() -> &'static Mutex<HashMap<String, SentinelRuntime>> {
    static RT: OnceLock<Mutex<HashMap<String, SentinelRuntime>>> = OnceLock::new();
    RT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn sentinel_apply(st: &AppState, app: &tauri::AppHandle) -> CmdResult<()> {
    let list = read_sentinels(st);
    let want: HashMap<String, SentinelCfg> = list
        .iter()
        .filter(|c| c.enabled)
        .map(|c| (c.id.clone(), c.clone()))
        .collect();
    let mut rt = sentinel_runtimes().lock().map_err(|_| AppError::io("sentinel lock poisoned"))?;
    // 停掉不再需要的
    let to_remove: Vec<String> = rt
        .keys()
        .filter(|id| !want.contains_key(*id))
        .cloned()
        .collect();
    for id in to_remove {
        if let Some(mut r) = rt.remove(&id) {
            r.stop.store(true, Ordering::Relaxed);
            if let Some(h) = r.handle.take() {
                let _ = h.join();
            }
        }
    }
    // 启动新增的
    for (id, cfg) in want {
        if rt.contains_key(&id) {
            continue;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let app2 = app.clone();
        let cfg2 = cfg.clone();
        let h = std::thread::Builder::new()
            .name(format!("sentinel-{id}"))
            .spawn(move || sentinel_loop(app2, cfg2, stop2))
            .map_err(|e| AppError::io(format!("哨兵线程启动失败 / Sentinel thread failed: {e}")))?;
        rt.insert(id, SentinelRuntime { stop, handle: Some(h) });
    }
    Ok(())
}

#[cfg(windows)]
fn sentinel_loop(app: tauri::AppHandle, cfg: SentinelCfg, stop: Arc<AtomicBool>) {
    use windows::Win32::Foundation::{FALSE, HANDLE, TRUE, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
        FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
        FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows::Win32::System::Threading::CreateEventW;

    let path_w: Vec<u16> = cfg.path.encode_utf16().chain([0]).collect();
    let Ok(handle) = (unsafe {
        CreateFileW(
            windows::core::PCWSTR(path_w.as_ptr()),
            0x0001, // FILE_LIST_DIRECTORY（读目录变更所需访问位）
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
            HANDLE::default(),
        )
    }) else {
        return; // 目录不可打开（权限/离线）→ 线程如实退出
    };
    let Ok(ev) = (unsafe { CreateEventW(None, FALSE, FALSE, None) }) else {
        unsafe { windows::Win32::Foundation::CloseHandle(handle) };
        return;
    };
    let mut buf = vec![0u8; 8 * 1024];
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    overlapped.hEvent = ev;
    let filter = FILE_NOTIFY_CHANGE_FILE_NAME
        | FILE_NOTIFY_CHANGE_DIR_NAME
        | FILE_NOTIFY_CHANGE_SIZE
        | FILE_NOTIFY_CHANGE_LAST_WRITE;
    let mut pending: Vec<(u32, String)> = Vec::new();
    let mut watch = true;

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let mut bytes: u32 = 0;
        if watch {
            let ok = unsafe {
                ReadDirectoryChangesW(
                    handle,
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as u32,
                    TRUE, // 监视子树
                    filter,
                    Some(&mut bytes),
                    Some(&mut overlapped),
                    None,
                )
            };
            if ok.is_err() {
                break;
            }
            watch = false; // 挂起中，等待完成
        }
        let wait = unsafe { windows::Win32::System::Threading::WaitForSingleObject(ev, 1000) };
        if stop.load(Ordering::Relaxed) {
            unsafe { CancelIoEx(handle, Some(&overlapped)) };
            break;
        }
        if wait == WAIT_OBJECT_0 {
            let mut got: u32 = 0;
            let ok = unsafe { GetOverlappedResult(handle, &overlapped, &mut got, FALSE) };
            watch = true;
            if ok.is_err() || got == 0 {
                continue;
            }
            // 解析 FILE_NOTIFY_INFORMATION 变长记录
            let mut off = 0usize;
            while off + 12 <= got as usize {
                let next = u32::from_ne_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
                let action = u32::from_ne_bytes(buf[off + 4..off + 8].try_into().unwrap());
                let len = u32::from_ne_bytes(buf[off + 8..off + 12].try_into().unwrap()) as usize;
                if off + 12 + len > buf.len() {
                    break;
                }
                let name = String::from_utf16_lossy(
                    &buf[off + 12..off + 12 + len]
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect::<Vec<u16>>(),
                );
                if !name.is_empty() {
                    pending.push((action, name));
                }
                if next == 0 {
                    break;
                }
                off += next;
            }
            // 合并窗口：1s 内到达的后续事件并入同一批
            continue;
        }
        if wait == WAIT_TIMEOUT {
            if !pending.is_empty() {
                // 1s 静默期到 → 合并发射
                let changes = merge_changes(&pending);
                pending.clear();
                let hour = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| (d.as_secs() / 3600 + 8) % 24) // 本地时区近似由前端再过滤；此处只做粗过滤
                    .unwrap_or(255) as u8;
                if !in_quiet_hours(hour, cfg.quiet_start, cfg.quiet_end) && !changes.is_empty() {
                    let _ = tauri::Emitter::emit(
                        &app,
                        "sentinel://event",
                        &SentinelEvent { id: cfg.id.clone(), path: cfg.path.clone(), changes },
                    );
                }
            }
        }
    }
    unsafe {
        windows::Win32::Foundation::CloseHandle(ev);
        windows::Win32::Foundation::CloseHandle(handle);
    }
}

#[cfg(not(windows))]
fn sentinel_loop(_app: tauri::AppHandle, _cfg: SentinelCfg, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// M-27：哨兵列表。
#[tauri::command]
pub fn sentinel_list(st: tauri::State<AppState>) -> CmdResult<Vec<SentinelCfg>> {
    Ok(read_sentinels(&st))
}

/// M-27：添加哨兵（上限 5；SMB/网络路径入口拒绝）。
#[tauri::command]
pub fn sentinel_add(
    st: tauri::State<AppState>,
    app: tauri::AppHandle,
    path: String,
    quiet_start: Option<u8>,
    quiet_end: Option<u8>,
) -> CmdResult<Vec<SentinelCfg>> {
    if path.starts_with(r"\\") || path.starts_with("//") {
        return Err(AppError::validation("暂不支持网络目录 / Network directories not supported"));
    }
    let p = PathBuf::from(&path);
    if !long_path(&p).is_dir() {
        return Err(AppError::not_found("目录不存在 / Directory not found"));
    }
    let mut list = read_sentinels(&st);
    if list.iter().any(|c| c.path.eq_ignore_ascii_case(&path)) {
        return Err(AppError::validation("该目录已在监控 / Already watched"));
    }
    if list.len() >= SENTINEL_MAX {
        return Err(AppError::validation("哨兵已达上限 5 / Sentinel limit (5) reached"));
    }
    let cfg = SentinelCfg {
        id: uuid::Uuid::new_v4().to_string(),
        path,
        enabled: true,
        quiet_start: quiet_start.unwrap_or(255),
        quiet_end: quiet_end.unwrap_or(255),
    };
    list.push(cfg);
    write_sentinels(&st, &list)?;
    sentinel_apply(&st, &app)?;
    Ok(list)
}

/// M-27：移除哨兵。
#[tauri::command]
pub fn sentinel_remove(
    st: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: String,
) -> CmdResult<Vec<SentinelCfg>> {
    let mut list = read_sentinels(&st);
    list.retain(|c| c.id != id);
    write_sentinels(&st, &list)?;
    sentinel_apply(&st, &app)?;
    Ok(list)
}

/// M-27：启停哨兵。
#[tauri::command]
pub fn sentinel_toggle(
    st: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: String,
    enabled: bool,
) -> CmdResult<Vec<SentinelCfg>> {
    let mut list = read_sentinels(&st);
    for c in list.iter_mut() {
        if c.id == id {
            c.enabled = enabled;
        }
    }
    write_sentinels(&st, &list)?;
    sentinel_apply(&st, &app)?;
    Ok(list)
}

/// 启动时恢复已启用哨兵（lib.rs setup 调用）。
pub fn startup_init(st: &AppState, app: &tauri::AppHandle) {
    let _ = sentinel_apply(st, app);
}

// ===========================================================================
// 测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- M-21 已知向量 ----

    #[test]
    fn md5_known_vector() {
        // md5("abc") = 900150983cd24fb0d6963f7d28e17f72
        let mut h = md5::Md5::new();
        use md5::Digest;
        h.update(b"abc");
        assert_eq!(hex(&h.finalize()), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn sha1_known_vector() {
        use sha1::Digest;
        let mut h = sha1::Sha1::new();
        h.update(b"abc");
        assert_eq!(hex(&h.finalize()), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn sha256_known_vector() {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(b"abc");
        assert_eq!(
            hex(&h.finalize()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn blake3_known_vector() {
        let mut h = blake3::Hasher::new();
        h.update(b"abc");
        assert_eq!(
            h.finalize().to_hex().to_string(),
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
    }

    // ---- Z-32 重命名规则管线 ----

    #[test]
    fn rename_replace_rule() {
        let rules = vec![RenameRule::Replace { find: "草稿".into(), replace: "终稿".into() }];
        assert_eq!(apply_rules("报告-草稿.md", &rules, 0), "报告-终稿.md");
        assert_eq!(apply_rules("a.txt", &rules, 0), "a.txt");
    }

    #[test]
    fn rename_number_rule() {
        let rules = vec![RenameRule::Number { start: 1, step: 1, pad: 3 }];
        assert_eq!(apply_rules("photo.jpg", &rules, 0), "photo001.jpg");
        assert_eq!(apply_rules("photo.jpg", &rules, 9), "photo010.jpg");
        assert_eq!(apply_rules("noext", &rules, 0), "noext001");
    }

    #[test]
    fn rename_case_rule() {
        let rules = vec![RenameRule::Case { mode: "upper".into() }];
        assert_eq!(apply_rules("readme.md", &rules, 0), "README.md");
        let rules = vec![RenameRule::Case { mode: "lower".into() }];
        assert_eq!(apply_rules("README.MD", &rules, 0), "readme.MD");
    }

    #[test]
    fn rename_ext_rule() {
        let rules = vec![RenameRule::Ext { from: "jpeg".into(), to: "jpg".into() }];
        assert_eq!(apply_rules("a.jpeg", &rules, 0), "a.jpg");
        let rules = vec![RenameRule::Ext { from: "txt".into(), to: String::new() }];
        assert_eq!(apply_rules("a.txt", &rules, 0), "a");
        let rules = vec![RenameRule::Ext { from: "txt".into(), to: "md".into() }];
        assert_eq!(apply_rules("a.log", &rules, 0), "a.log"); // 不匹配 → 原样
    }

    #[test]
    fn rename_pipeline_order() {
        // 替换 → 序号 → 大小写 → 扩展名（顺序应用）
        let rules = vec![
            RenameRule::Replace { find: "IMG_".into(), replace: "photo".into() },
            RenameRule::Number { start: 10, step: 10, pad: 2 },
            RenameRule::Ext { from: "jpeg".into(), to: "jpg".into() },
        ];
        assert_eq!(apply_rules("IMG_0001.jpeg", &rules, 0), "photo000110.jpg");
    }

    // ---- M-27 哨兵纯函数 ----

    #[test]
    fn sentinel_merge_dedupes() {
        let raw = vec![
            (1, "a.txt".to_string()),
            (1, "a.txt".to_string()), // 同类去重
            (3, "a.txt".to_string()), // 不同类保留
            (2, "b.txt".to_string()),
        ];
        let out = merge_changes(&raw);
        assert_eq!(out.len(), 3);
        assert!(out.contains(&SentinelChange { kind: "added".into(), name: "a.txt".into() }));
        assert!(out.contains(&SentinelChange { kind: "modified".into(), name: "a.txt".into() }));
        assert!(out.contains(&SentinelChange { kind: "removed".into(), name: "b.txt".into() }));
    }

    #[test]
    fn quiet_hours_wrap_midnight() {
        assert!(in_quiet_hours(23, 22, 6));
        assert!(in_quiet_hours(2, 22, 6));
        assert!(!in_quiet_hours(12, 22, 6));
        assert!(in_quiet_hours(10, 9, 18));
        assert!(!in_quiet_hours(18, 9, 18));
        assert!(!in_quiet_hours(5, 255, 6)); // 无静默
    }

    // ---- M-23 zip-slip 防护 ----

    #[test]
    fn zip_slip_rejected() {
        assert!(safe_inner("ok/path/file.txt").is_ok());
        assert!(safe_inner("../escape.txt").is_err());
        assert!(safe_inner("C:\\abs.txt").is_err());
        assert!(safe_inner("back\\slash.txt").is_err());
    }

    // ---- M-26 三档删除 / M-25 在 explorer.rs 侧测 ----

    // ---- Z-33 抽样分组逻辑 ----

    #[test]
    fn dupe_sample_limit_respects_boundary() {
        // 4KB 抽样边界：小于 4KB 的文件全量读取也不出错
        let dir = std::env::temp_dir().join(format!("dupe_test_{}", now_ms()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.txt"), b"same content").unwrap();
        fs::write(dir.join("b.txt"), b"same content").unwrap();
        let ha = blake3_file(&dir.join("a.txt"), Some(4096)).unwrap();
        let hb = blake3_file(&dir.join("b.txt"), Some(4096)).unwrap();
        assert_eq!(ha, hb);
        let _ = fs::remove_dir_all(&dir);
    }
}
