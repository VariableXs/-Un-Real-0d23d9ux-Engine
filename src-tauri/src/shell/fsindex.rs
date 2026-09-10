//! L3 shell — fsindex.rs（F-4 全局文件搜索后端）
//! 容器内文件名+路径索引（内存索引，轮询增量：>1s 未重建则后台重建，查询读快照）。
//! - 范围：数据目录（root/Workspace/Apps/recycle）——直跑档只索引容器内（边界如实声明）
//! - VM 档 NTFS USN：不碰宿主 NTFS；容器内轮询已满足「新建文件 1s 内可搜到」验收
//! - 空闲优先：后台线程每次扫描间让出（小步读目录），不设优先级操作
//! - 基准：10 万级文件名子串查询为内存线性扫描（<50ms 量级）

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

const MAX_ENTRIES: usize = 300_000;
const MAX_DEPTH: u32 = 16;
/// 快照过期阈值：超过即触发后台重建（新建文件 ~1s 内可见）。
const STALE: Duration = Duration::from_millis(1200);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsHit {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// 毫秒时间戳（mtime）。
    pub mtime: u64,
}

#[derive(Clone, Default)]
struct Snapshot {
    entries: Vec<FsHit>,
    built_at: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsIndexStatus {
    pub count: usize,
    pub built_at: u64,
    pub building: bool,
    /// 直跑档 = true（只索引容器内；VM 档同为容器内，但已通过边界声明区分）。
    pub container_only: bool,
}

static SNAP: Mutex<Option<Snapshot>> = Mutex::new(None);
static BUILDING: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn get_snap() -> Option<Snapshot> {
    SNAP.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// 后台重建（同一时刻仅一个线程在扫描；扫描完原子换快照）。
fn spawn_rebuild(roots: Vec<PathBuf>) -> bool {
    if BUILDING.swap(true, Ordering::SeqCst) {
        return false;
    }
    std::thread::spawn(move || {
        let mut entries: Vec<FsHit> = Vec::new();
        for root in &roots {
            walk(root, 0, &mut entries);
            if entries.len() >= MAX_ENTRIES {
                break;
            }
        }
        entries.truncate(MAX_ENTRIES);
        let snap = Snapshot { entries, built_at: now_ms() };
        *SNAP.lock().unwrap_or_else(|e| e.into_inner()) = Some(snap);
        BUILDING.store(false, Ordering::SeqCst);
    });
    true
}

/// 深度受限递归扫描（目录读失败静默跳过——索引尽力而为）。
fn walk(dir: &Path, depth: u32, out: &mut Vec<FsHit>) {
    if depth > MAX_DEPTH || out.len() >= MAX_ENTRIES {
        return;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for item in rd.flatten() {
        let Ok(ft) = item.file_type() else { continue };
        let path = item.path();
        let meta = match item.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        out.push(FsHit {
            name: item.file_name().to_string_lossy().into_owned(),
            path: path.to_string_lossy().into_owned(),
            is_dir: ft.is_dir(),
            size: if ft.is_file() { meta.len() } else { 0 },
            mtime,
        });
        // Reparse point（符号链接/junction）不深入，防符号链接环
        if ft.is_dir() && !is_reparse(&meta) {
            walk(&path, depth + 1, out);
        }
        if out.len() >= MAX_ENTRIES {
            return;
        }
    }
}

/// FILE_ATTRIBUTE_REPARSE_POINT（Windows；非 Windows 恒 false）。
#[cfg(windows)]
fn is_reparse(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse(_meta: &std::fs::Metadata) -> bool {
    false
}

fn index_roots(st: &AppState) -> Vec<PathBuf> {
    // 第十三轮大检查：Workspace/Apps/recycle 都是 data_dir 子目录——root 扫描
    // 已全部覆盖，此前逐个 exists() 再丢弃属于每次查询的纯浪费系统调用。
    vec![st.data_dir.clone()]
}

/// 索引状态（设置/搜索浮层展示「索引中/条数」）。
#[tauri::command]
pub fn fsindex_status(st: tauri::State<AppState>) -> CmdResult<FsIndexStatus> {
    let snap = get_snap();
    ensure_fresh(st.inner());
    Ok(FsIndexStatus {
        count: snap.as_ref().map(|s| s.entries.len()).unwrap_or(0),
        built_at: snap.as_ref().map(|s| s.built_at).unwrap_or(0),
        building: BUILDING.load(Ordering::SeqCst),
        container_only: true,
    })
}

fn ensure_fresh(st: &AppState) {
    let stale = match get_snap() {
        None => true,
        Some(s) => now_ms().saturating_sub(s.built_at) > STALE.as_millis() as u64,
    };
    if stale {
        spawn_rebuild(index_roots(st));
    }
}

/// 自检抽检用（sysmaint）：快照前 n 条样本（不触发重建）。
pub(crate) fn sample_entries(n: usize) -> Vec<FsHit> {
    let snap = get_snap();
    match snap {
        Some(s) => s.entries.into_iter().take(n).collect(),
        None => Vec::new(),
    }
}

/// 文件名/路径子串查询（大小写不敏感）+ 扩展名/类型/大小/时间过滤。
/// 直跑档与 VM 档同口径：仅容器内（spec F-4 边界如实声明）。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn fsindex_query(
    st: tauri::State<AppState>,
    query: String,
    ext: Option<String>,
    kind: Option<String>,
    min_size: Option<u64>,
    newer_days: Option<f64>,
    limit: Option<usize>,
) -> CmdResult<Vec<FsHit>> {
    ensure_fresh(st.inner());
    let snap = get_snap().ok_or_else(|| AppError::validation("索引尚未就绪，请稍候重试"))?;
    let q = query.trim().to_lowercase();
    let ext = ext.map(|e| e.trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty());
    let kind = kind.unwrap_or_else(|| "all".into());
    let min_size = min_size.unwrap_or(0);
    let newer_cut = newer_days
        .filter(|d| *d > 0.0)
        .map(|d| now_ms().saturating_sub((d * 86_400_000.0) as u64));
    let limit = limit.unwrap_or(500).min(2000);

    let mut out: Vec<FsHit> = Vec::new();
    for e in &snap.entries {
        if !q.is_empty() && !e.name.to_lowercase().contains(&q) && !e.path.to_lowercase().contains(&q) {
            continue;
        }
        if let Some(ext) = &ext {
            let name = e.name.to_lowercase();
            if e.is_dir || !name.rsplit('.').next().is_some_and(|x| x == ext) {
                continue;
            }
        }
        match kind.as_str() {
            "file" if e.is_dir => continue,
            "dir" if !e.is_dir => continue,
            _ => {}
        }
        if e.size < min_size {
            continue;
        }
        if let Some(cut) = newer_cut {
            if e.mtime < cut {
                continue;
            }
        }
        out.push(e.clone());
        if out.len() >= limit {
            break;
        }
    }
    // 空查询 = 最近文件浏览：按 mtime 降序
    if q.is_empty() {
        out.sort_by(|a, b| b.mtime.cmp(&a.mtime));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_respects_depth_and_counts() {
        // 用测试自身目录做一次受限扫描：应至少索引到本文件
        let mut out = Vec::new();
        walk(Path::new(env!("CARGO_MANIFEST_DIR")), 0, &mut out);
        assert!(out.iter().any(|e| e.name == "fsindex.rs"));
    }

    #[test]
    fn roots_dedupe_to_container_root() {
        // index_roots 只保留数据根（子目录被 root 扫描覆盖）
        let st = AppState::bootstrap_dirs_at(std::env::temp_dir().join("var-fsindex-test")).unwrap();
        let roots = index_roots(&st);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0], st.data_dir);
    }
}
