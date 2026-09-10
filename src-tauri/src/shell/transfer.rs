//! AI-10（U-28 传输指挥台）：
//! - 队列中枢：`<dataDir>/transfer.json` 持久化；并发 3；
//!   冲突四策略（replace / skip / keep 两者 / 取消——取消由前端在入队前用
//!   ex_conflicts 探测后放弃）；失败重试面板（tr_retry）
//! - 三来源接入（explorer 复制/移动、拖入、版本导出）统一走 tr_enqueue
//! - 进度事件 `transfer://progress`（速度迷你曲线由前端按 bytes 时间差计算）
//! - 暂停/恢复：分块间检查控制位（暂停挂起、取消退出；已复制部分保留，
//!   恢复 = 重新入队续传——本版为重传，如实声明）

use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const CONCURRENCY: usize = 3;
const CHUNK: usize = 1024 * 1024;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TrStatus {
    Queued,
    Running,
    Paused,
    Done,
    Failed,
    Canceled,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrItem {
    pub id: String,
    pub src: String,
    pub dest: String,
    /// "copy" | "move"
    pub kind: String,
    /// "replace" | "skip" | "keep"
    pub on_conflict: String,
    pub status: TrStatus,
    pub bytes: u64,
    pub total: u64,
    pub files_done: u64,
    pub files_total: u64,
    pub error: Option<String>,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TrStore {
    items: Vec<TrItem>,
}

static HUB: OnceLock<tauri::AppHandle> = OnceLock::new();
static ITEMS: OnceLock<Mutex<Vec<TrItem>>> = OnceLock::new();
static CONTROL: OnceLock<Mutex<std::collections::HashMap<String, Ctrl>>> = OnceLock::new();
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

#[derive(PartialEq, Clone, Copy)]
enum Ctrl {
    Pause,
    Cancel,
}

fn items() -> &'static Mutex<Vec<TrItem>> {
    ITEMS.get_or_init(|| Mutex::new(Vec::new()))
}

fn control() -> &'static Mutex<std::collections::HashMap<String, Ctrl>> {
    CONTROL.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn data_dir() -> CmdResult<PathBuf> {
    let app = HUB.get().ok_or_else(|| AppError::io("transfer hub 未启动 / hub not started"))?;
    Ok(app.state::<AppState>().data_dir.clone())
}

fn store_path() -> CmdResult<PathBuf> {
    Ok(data_dir()?.join("transfer.json"))
}

fn persist() {
    let Ok(sp) = store_path() else { return };
    // 第十轮大检查：锁中毒恢复（后台线程不能因 .unwrap() 静默死亡）+
    // 写盘移出锁窗口（磁盘慢时不阻塞其他命令）。
    let bytes = {
        let guard = items().lock().unwrap_or_else(|e| e.into_inner());
        match serde_json::to_vec_pretty(&TrStore { items: guard.clone() }) {
            Ok(b) => b,
            Err(_) => return,
        }
    };
    let _ = fs::write(sp, bytes);
}

fn emit_progress() {
    if let Some(app) = HUB.get() {
        use tauri::Emitter;
        let snapshot = items().lock().unwrap_or_else(|e| e.into_inner()).clone();
        let _ = app.emit("transfer://progress", snapshot);
    }
}

/// 启动传输中枢（lib.rs setup 调用）：载入持久化队列 + 扫描线程。
pub fn spawn_hub(app: tauri::AppHandle) {
    let _ = HUB.set(app);
    // 恢复持久化队列（运行中的项目按中断处理 → 重新排队）
    if let Ok(sp) = store_path() {
        if let Ok(bytes) = fs::read(&sp) {
            if let Ok(mut s) = serde_json::from_slice::<TrStore>(&bytes) {
                let mut guard = items().lock().unwrap_or_else(|e| e.into_inner());
                for it in &mut s.items {
                    match it.status {
                        TrStatus::Running | TrStatus::Queued | TrStatus::Paused => it.status = TrStatus::Queued,
                        _ => {}
                    }
                    guard.push(it.clone());
                }
            }
        }
    }
    std::thread::spawn(scan_loop);
}

fn scan_loop() {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if ACTIVE.load(Ordering::Relaxed) >= CONCURRENCY {
            continue;
        }
        // 取下一个 queued 项（锁中毒恢复：扫描线程绝不能 panic 死掉，
        // 否则整个传输队列永久停摆）
        let next = {
            let mut guard = items().lock().unwrap_or_else(|e| e.into_inner());
            guard
                .iter_mut()
                .find(|it| it.status == TrStatus::Queued)
                .map(|it| {
                    it.status = TrStatus::Running;
                    it.started_at = Some(now_ms());
                    it.error = None;
                    it.clone()
                })
        };
        if let Some(item) = next {
            persist();
            emit_progress();
            ACTIVE.fetch_add(1, Ordering::Relaxed);
            std::thread::spawn(move || {
                run_item(item);
                ACTIVE.fetch_sub(1, Ordering::Relaxed);
            });
        }
    }
}

fn take_ctrl(id: &str) -> Option<Ctrl> {
    control().lock().unwrap_or_else(|e| e.into_inner()).get(id).copied()
}

fn update_item(id: &str, f: impl FnOnce(&mut TrItem)) {
    update_item_mem(id, f);
    persist();
    emit_progress();
}

/// 只更新内存中的条目（锁中毒恢复），不写盘不广播——给高频路径用。
fn update_item_mem(id: &str, f: impl FnOnce(&mut TrItem)) {
    let mut guard = items().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(it) = guard.iter_mut().find(|i| i.id == id) {
        f(it);
    }
}

/// 统计目录/文件总字节与文件数。
fn walk_total(p: &Path, bytes: &mut u64, files: &mut u64) {
    if let Ok(meta) = fs::symlink_metadata(p) {
        if meta.is_dir() {
            if let Ok(rd) = fs::read_dir(p) {
                for e in rd.flatten() {
                    walk_total(&e.path(), bytes, files);
                }
            }
        } else if meta.is_file() {
            *bytes += meta.len();
            *files += 1;
        }
    }
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let ext = Path::new(name).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut i = 0u32;
    loop {
        let cand = if i == 0 { dir.join(format!("{stem}{ext}")) } else { dir.join(format!("{stem} ({i}){ext}")) };
        if !cand.exists() {
            return cand;
        }
        i += 1;
    }
}

/// 单文件分块复制（进度 + 暂停/取消检查点）。返回 (written, canceled, paused)。
/// 第十轮大检查：进度持久化/广播按 250ms 节流——此前每个 1MiB 分块都做
/// 全队列 JSON 序列化 + 写盘 + 事件广播（10GB 文件 = 万次写盘风暴，
/// 严重拖垮传输期间的整机流畅度）。进度条视觉不变（250ms 足够平滑），
/// 结束时强制冲刷一次保证最终状态落盘。
fn copy_file_progress(src: &Path, dest: &Path, id: &str) -> CmdResult<(u64, bool, bool)> {
    let mut in_f = fs::File::open(src)?;
    let mut out_f = fs::File::create(dest)?;
    let mut buf = vec![0u8; CHUNK];
    let mut written = 0u64;
    let mut last_push = now_ms();
    loop {
        match take_ctrl(id) {
            Some(Ctrl::Cancel) => return Ok((written, true, false)),
            Some(Ctrl::Pause) => return Ok((written, false, true)),
            None => {}
        }
        let n = in_f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out_f.write_all(&buf[..n])?;
        written += n as u64;
        update_item_mem(id, |it| it.bytes += n as u64);
        let t = now_ms();
        if t.saturating_sub(last_push) >= 250 {
            last_push = t;
            persist();
            emit_progress();
        }
    }
    out_f.sync_all()?;
    persist();
    emit_progress();
    Ok((written, false, false))
}

fn run_item(item: TrItem) {
    let res = transfer_one(&item);
    match res {
        Ok(()) => update_item(&item.id, |it| {
            it.status = TrStatus::Done;
            it.finished_at = Some(now_ms());
            it.error = None;
        }),
        Err(TrErr::Cancelled) => update_item(&item.id, |it| {
            it.status = TrStatus::Canceled;
            it.finished_at = Some(now_ms());
        }),
        Err(TrErr::Paused) => update_item(&item.id, |it| {
            it.status = TrStatus::Paused;
        }),
        Err(TrErr::Failed(e)) => update_item(&item.id, |it| {
            it.status = TrStatus::Failed;
            it.error = Some(e);
            it.finished_at = Some(now_ms());
        }),
    }
    control().lock().unwrap_or_else(|e| e.into_inner()).remove(&item.id);
}

#[derive(Debug)]
enum TrErr {
    Cancelled,
    Paused,
    Failed(String),
}

impl From<std::io::Error> for TrErr {
    fn from(e: std::io::Error) -> Self {
        TrErr::Failed(e.to_string())
    }
}

fn transfer_one(item: &TrItem) -> Result<(), TrErr> {
    let src = PathBuf::from(&item.src);
    let dest_dir = PathBuf::from(item.dest.as_str());
    // 目标目录必须在传输期间有效（前端保证是目录；被删则失败如实上报）
    fs::create_dir_all(&dest_dir).map_err(|e| TrErr::Failed(e.to_string()))?;
    // 统计总量
    let mut total = 0u64;
    let mut files_total = 0u64;
    walk_total(&src, &mut total, &mut files_total);
    update_item(&item.id, |it| {
        it.total = total;
        it.files_total = files_total;
        it.bytes = 0;
        it.files_done = 0;
    });
    // 恢复场景：若已完成过一部分，简单重传（bytes 已归零）
    transfer_dir(&src, &dest_dir, item)?;
    // move：删除源
    if item.kind == "move" {
        if src.is_dir() {
            fs::remove_dir_all(&src).map_err(|e| TrErr::Failed(e.to_string()))?;
        } else {
            fs::remove_file(&src).map_err(|e| TrErr::Failed(e.to_string()))?;
        }
    }
    Ok(())
}

fn dest_for(src: &Path, dest_dir: &Path, on_conflict: &str) -> Result<PathBuf, TrErr> {
    let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let dest = dest_dir.join(&name);
    if !dest.exists() {
        return Ok(dest);
    }
    match on_conflict {
        "replace" => {
            if dest.is_dir() {
                fs::remove_dir_all(&dest).map_err(|e| TrErr::Failed(e.to_string()))?;
            } else {
                fs::remove_file(&dest).map_err(|e| TrErr::Failed(e.to_string()))?;
            }
            Ok(dest)
        }
        "skip" => Err(TrErr::Failed(format!("SKIPPED:{name}"))),
        _ => Ok(unique_path(dest_dir, &name)),
    }
}

fn transfer_dir(src: &Path, dest_dir: &Path, item: &TrItem) -> Result<(), TrErr> {
    let dest = match dest_for(src, dest_dir, &item.on_conflict) {
        Ok(d) => d,
        Err(TrErr::Failed(msg)) if msg.starts_with("SKIPPED:") => {
            // skip：整个跳过（目录整体跳过）
            update_item(&item.id, |it| it.files_done += 1);
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let meta = fs::symlink_metadata(src).map_err(|e| TrErr::Failed(e.to_string()))?;
    if meta.is_dir() {
        fs::create_dir_all(&dest).map_err(|e| TrErr::Failed(e.to_string()))?;
        let rd = fs::read_dir(src).map_err(|e| TrErr::Failed(e.to_string()))?;
        for e in rd.flatten() {
            transfer_dir(&e.path(), &dest, item)?;
        }
        Ok(())
    } else if meta.is_file() {
        let (written, canceled, paused) =
            copy_file_progress(src, &dest, &item.id).map_err(|e| TrErr::Failed(e.to_string()))?;
        if canceled {
            let _ = fs::remove_file(&dest);
            return Err(TrErr::Cancelled);
        }
        if paused {
            let _ = fs::remove_file(&dest);
            return Err(TrErr::Paused);
        }
        let _ = written;
        update_item(&item.id, |it| it.files_done += 1);
        Ok(())
    } else {
        Ok(())
    }
}

// ---------- 命令 ----------

/// 入队（来源不限：explorer 操作 / 拖入 / 版本导出）。
#[tauri::command(async)]
pub fn tr_enqueue(st: tauri::State<AppState>, srcs: Vec<String>, dest_dir: String, kind: String, on_conflict: Option<String>) -> CmdResult<Vec<TrItem>> {
    if !matches!(kind.as_str(), "copy" | "move") {
        return Err(AppError::validation("kind 必须为 copy|move / kind must be copy|move"));
    }
    let oc = on_conflict.unwrap_or_else(|| "keep".into());
    if !matches!(oc.as_str(), "replace" | "skip" | "keep") {
        return Err(AppError::validation("冲突策略无效 / invalid conflict strategy"));
    }
    let dest = PathBuf::from(&dest_dir);
    if !dest.is_dir() {
        return Err(AppError::not_found("目标目录不存在 / destination folder not found"));
    }
    let dir = st.data_dir.clone();
    // 第十轮大检查：源存在性校验和写盘都移出锁窗口——此前整个
    // fs::exists 检查 + JSON 序列化 + 写盘都持着队列锁，磁盘慢时
    // 阻塞所有其他传输命令。
    for s in &srcs {
        if !Path::new(s).exists() {
            return Err(AppError::not_found(format!("源不存在 / source not found: {s}")));
        }
    }
    {
        let mut guard = items().lock().map_err(|_| AppError::io("queue mutex"))?;
        for s in &srcs {
            guard.push(TrItem {
                id: crate::db::gen_id(),
                src: s.clone(),
                dest: dest_dir.clone(),
                kind: kind.clone(),
                on_conflict: oc.clone(),
                status: TrStatus::Queued,
                bytes: 0,
                total: 0,
                files_done: 0,
                files_total: 0,
                error: None,
                created_at: now_ms(),
                started_at: None,
                finished_at: None,
            });
        }
    }
    // 持久化（用调用方给的 data_dir，避免 HUB 未就绪）
    let sp = dir.join("transfer.json");
    if let Ok(bytes) = serde_json::to_vec_pretty(&TrStore { items: tr_list()? }) {
        let _ = fs::write(sp, bytes);
    }
    emit_progress();
    tr_list()
}

#[tauri::command(async)]
pub fn tr_list() -> CmdResult<Vec<TrItem>> {
    let guard = items().lock().map_err(|_| AppError::io("queue mutex"))?;
    Ok(guard.clone())
}

#[tauri::command(async)]
pub fn tr_pause(id: String) -> CmdResult<Vec<TrItem>> {
    control().lock().unwrap_or_else(|e| e.into_inner()).insert(id, Ctrl::Pause);
    Ok(tr_list()?)
}

#[tauri::command(async)]
pub fn tr_resume(id: String) -> CmdResult<Vec<TrItem>> {
    control().lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    update_item(&id, |it| {
        if it.status == TrStatus::Paused {
            it.status = TrStatus::Queued; // 重传（如实：续传为重传）
            it.bytes = 0;
        }
    });
    Ok(tr_list()?)
}

#[tauri::command(async)]
pub fn tr_cancel(id: String) -> CmdResult<Vec<TrItem>> {
    control().lock().unwrap_or_else(|e| e.into_inner()).insert(id, Ctrl::Cancel);
    Ok(tr_list()?)
}

#[tauri::command(async)]
pub fn tr_retry(id: String) -> CmdResult<Vec<TrItem>> {
    update_item(&id, |it| {
        if it.status == TrStatus::Failed || it.status == TrStatus::Canceled {
            it.status = TrStatus::Queued;
            it.bytes = 0;
            it.files_done = 0;
            it.error = None;
            it.finished_at = None;
        }
    });
    Ok(tr_list()?)
}

/// 清理已完成/失败/取消的历史条目。
#[tauri::command(async)]
pub fn tr_clear_done() -> CmdResult<Vec<TrItem>> {
    {
        let mut guard = items().lock().map_err(|_| AppError::io("queue mutex"))?;
        guard.retain(|it| !matches!(it.status, TrStatus::Done | TrStatus::Failed | TrStatus::Canceled));
    }
    persist();
    emit_progress();
    tr_list()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let tmp = std::env::temp_dir().join(format!("variable-transfer-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        (st, tmp)
    }

    #[test]
    fn walk_total_counts() {
        let (st, tmp) = temp_state("walk");
        let d = tmp.join("src");
        fs::create_dir_all(d.join("sub")).unwrap();
        fs::write(d.join("a.txt"), vec![1u8; 100]).unwrap();
        fs::write(d.join("sub").join("b.txt"), vec![1u8; 50]).unwrap();
        let mut total = 0;
        let mut files = 0;
        walk_total(&d, &mut total, &mut files);
        assert_eq!(total, 150);
        assert_eq!(files, 2);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn copy_file_progress_writes_all() {
        let (st, tmp) = temp_state("copy");
        let src = tmp.join("in.bin");
        fs::write(&src, vec![7u8; CHUNK as usize * 2 + 10]).unwrap();
        let dest = tmp.join("out.bin");
        let (written, canceled, paused) = copy_file_progress(&src, &dest, "test-id").unwrap();
        assert_eq!(written, (CHUNK as u64) * 2 + 10);
        assert!(!canceled);
        assert!(!paused);
        assert_eq!(fs::metadata(&dest).unwrap().len(), (CHUNK as u64) * 2 + 10);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn dest_for_conflict_strategies() {
        let (st, tmp) = temp_state("conflict");
        let dir = tmp.join("d");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.txt"), b"old").unwrap();
        // keep → 加后缀
        let d = dest_for(&dir.join("a.txt"), &dir, "keep").unwrap();
        assert_ne!(d, dir.join("a.txt"));
        assert!(d.to_string_lossy().contains("a (1)"));
        // replace → 原地
        let d = dest_for(&dir.join("a.txt"), &dir, "replace").unwrap();
        assert_eq!(d, dir.join("a.txt"));
        assert!(fs::metadata(&d).is_err()); // replace 已删除旧文件
        // skip → 标记
        fs::write(dir.join("a.txt"), b"old").unwrap();
        match dest_for(&dir.join("a.txt"), &dir, "skip") {
            Err(TrErr::Failed(m)) => assert!(m.starts_with("SKIPPED:")),
            _ => panic!("expect skip marker"),
        }
        let _ = fs::remove_dir_all(&tmp);
    }
}
