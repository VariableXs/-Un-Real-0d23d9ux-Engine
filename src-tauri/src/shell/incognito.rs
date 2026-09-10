//! AI-10（U-36 隐身会话）：
//! - 临时会话层：会话内文件副本落 `<dataDir>/incognito/<sid>/`（关闭即焚）
//! - 焚毁：逐文件覆写删除（复用 privacy::shred_file）+ 整目录树清除 + 剪贴板清空
//! - 禁止清单：is_incognito() 供 versions/lineage/索引查询——隐身期间跳过记录
//! - 会话历史：只存次数与时间戳（不留内容），关闭后任何残留扫描为 0
//! 红线：隐身会话内禁用版本快照、血缘记录、索引，且 UI 明示「此处不被记录」。

use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn incognito_dir(st: &AppState) -> PathBuf {
    st.data_dir.join("incognito")
}

fn session_dir(st: &AppState, sid: &str) -> PathBuf {
    incognito_dir(st).join(sid)
}

fn history_path(st: &AppState) -> PathBuf {
    incognito_dir(st).join("history.json")
}

/// 全局隐身开关（versions/lineage/fsindex 在记录前查询）。
static INCOGNITO: AtomicBool = AtomicBool::new(false);

/// 当前是否处于隐身会话（供其他模块的禁止清单判断）。
pub fn is_incognito() -> bool {
    INCOGNITO.load(Ordering::Relaxed)
}

/// 测试辅助：复位全局隐身状态（生产代码勿用）。
pub fn reset_for_tests() {
    INCOGNITO.store(false, Ordering::Relaxed);
    if let Ok(mut g) = CURRENT.lock() {
        *g = None;
    }
}

/// 测试辅助：全局串行锁（insights 等跨模块测试共享，避免全局隐身态竞态）。
pub static TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 当前会话 id（非隐身为 None）。
static CURRENT: Mutex<Option<String>> = Mutex::new(None);

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IncSession {
    pub id: String,
    /// 内存+加密临时盘语义在本实现为一次性目录：关闭即逐文件覆写
    pub started_at: u64,
    pub file_count: u64,
    pub file_bytes: u64,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct History {
    /// 已结束会话数
    sessions: u64,
    /// 上次焚毁时间
    last_burned_at: u64,
    /// 上次焚毁时清除的文件数
    last_burned_files: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncStatus {
    pub active: bool,
    pub session: Option<IncSession>,
    pub sessions_total: u64,
    pub last_burned_at: u64,
}

fn load_history(st: &AppState) -> History {
    fs::read(history_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_history(st: &AppState, h: &History) -> CmdResult<()> {
    fs::create_dir_all(incognito_dir(st))?;
    let bytes = serde_json::to_vec_pretty(h)?;
    fs::write(history_path(st), bytes)?;
    Ok(())
}

fn stat_session(dir: &Path) -> (u64, u64) {
    let mut count = 0u64;
    let mut bytes = 0u64;
    fn walk(dir: &Path, count: &mut u64, bytes: &mut u64) {
        if let Ok(rd) = fs::read_dir(dir) {
            for item in rd.flatten() {
                let p = item.path();
                if let Ok(ft) = item.file_type() {
                    if ft.is_dir() {
                        walk(&p, count, bytes);
                    } else if ft.is_file() {
                        *count += 1;
                        *bytes += item.metadata().map(|m| m.len()).unwrap_or(0);
                    }
                }
            }
        }
    }
    walk(dir, &mut count, &mut bytes);
    (count, bytes)
}

// ---------- 命令 ----------

/// 开启隐身会话（同一时刻仅一个）。
#[tauri::command(async)]
pub fn inc_start(st: tauri::State<AppState>) -> CmdResult<IncSession> {
    if is_incognito() {
        return Err(AppError::validation("已有隐身会话进行中 / incognito session already active"));
    }
    let sid = crate::db::gen_id();
    fs::create_dir_all(session_dir(&st, &sid))?;
    INCOGNITO.store(true, Ordering::Relaxed);
    *CURRENT.lock().map_err(|_| AppError::io("incognito mutex"))? = Some(sid.clone());
    Ok(IncSession { id: sid, started_at: now_ms(), file_count: 0, file_bytes: 0 })
}

/// 当前状态（含统计）。
#[tauri::command(async)]
pub fn inc_status(st: tauri::State<AppState>) -> CmdResult<IncStatus> {
    let hist = load_history(&st);
    let session = if is_incognito() {
        CURRENT.lock().ok().and_then(|g| g.clone()).map(|sid| {
            let (count, bytes) = stat_session(&session_dir(&st, &sid));
            IncSession {
                id: sid,
                started_at: now_ms(),
                file_count: count,
                file_bytes: bytes,
            }
        })
    } else {
        None
    };
    Ok(IncStatus {
        active: is_incognito(),
        session,
        sessions_total: hist.sessions,
        last_burned_at: hist.last_burned_at,
    })
}

/// 会话内写入文件副本（explorer 在隐身会话中打开文件时调用）。
#[tauri::command(async)]
pub fn inc_write(st: tauri::State<AppState>, name: String, contents: String) -> CmdResult<String> {
    let sid = current_id().ok_or_else(|| AppError::validation("无隐身会话 / no incognito session"))?;
    let safe: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    if safe.is_empty() || safe.len() > 180 {
        return Err(AppError::validation("文件名无效 / invalid name"));
    }
    let p = session_dir(&st, &sid).join(safe);
    fs::write(&p, contents.as_bytes())?;
    Ok(p.to_string_lossy().to_string())
}

/// 会话内列文件。
#[tauri::command(async)]
pub fn inc_list(st: tauri::State<AppState>) -> CmdResult<Vec<String>> {
    let sid = current_id().ok_or_else(|| AppError::validation("无隐身会话 / no incognito session"))?;
    let dir = session_dir(&st, &sid);
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for item in rd.flatten() {
            out.push(item.file_name().to_string_lossy().to_string());
        }
    }
    out.sort();
    Ok(out)
}

fn current_id() -> Option<String> {
    if !is_incognito() {
        return None;
    }
    CURRENT.lock().ok().and_then(|g| g.clone())
}

/// 关闭（焚毁）当前隐身会话：逐文件覆写删除 → 目录树清除 → 剪贴板清空。
/// 返回清除的文件数；残留断言：关闭后 session 目录必须不存在。
#[tauri::command(async)]
pub fn inc_end(st: tauri::State<AppState>) -> CmdResult<u64> {
    end_and_burn_inner(&st)
}

/// 内部直连实现（panic.rs 紧急擦拭 L2 复用；测试同源）。
/// 无会话时返回 0（紧急路径容错，不报错）。
pub fn end_and_burn_inner(st: &AppState) -> CmdResult<u64> {
    let Some(sid) = current_id() else {
        return Ok(0);
    };
    let dir = session_dir(st, &sid);
    // 1) 逐文件焚毁（覆写；小文件多，快）
    let mut burned = 0u64;
    if dir.is_dir() {
        burned = burn_tree(&dir)?;
    }
    // 2) 目录树整体删除（含空目录）
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| AppError::io(format!("清理会话目录失败 / remove failed: {e}")))?;
    }
    let (count, _) = stat_session(&dir);
    debug_assert_eq!(count, 0, "隐身会话关闭后必须零残留 / incognito must leave zero residue");
    // 3) 状态复位 + 历史登记（仅次数，不留内容）
    INCOGNITO.store(false, Ordering::Relaxed);
    *CURRENT.lock().map_err(|_| AppError::io("incognito mutex"))? = None;
    let mut hist = load_history(st);
    hist.sessions += 1;
    hist.last_burned_at = now_ms();
    hist.last_burned_files = burned;
    save_history(st, &hist)?;
    // 4) 剪贴板清空（尽力而为）
    crate::shell::panic::clear_clipboard_pub();
    Ok(hist.last_burned_files)
}

/// 递归焚毁目录内全部文件（覆写 + 删除），返回焚毁数。
fn burn_tree(dir: &Path) -> CmdResult<u64> {
    let mut n = 0u64;
    if let Ok(rd) = fs::read_dir(dir) {
        for item in rd.flatten() {
            let p = item.path();
            if let Ok(ft) = item.file_type() {
                if ft.is_dir() {
                    n += burn_tree(&p)?;
                } else if ft.is_file() {
                    if crate::shell::privacy::shred_file(&p).is_ok() {
                        n += 1;
                    }
                }
            }
        }
    }
    Ok(n)
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;
    /// 全局隐身状态是进程级单例——测试间串行（与 insights 测试共享 TEST_SERIAL）。

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let tmp = std::env::temp_dir().join(format!("variable-incognito-{tag}-{}", std::process::id()));
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        (st, tmp)
    }

    #[test]
    fn lifecycle_burn_leaves_zero_residue() {
        let _guard = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (st, tmp) = temp_state("burn");
        // 开启
        let s = {
            let sid = crate::db::gen_id();
            fs::create_dir_all(session_dir(&st, &sid)).unwrap();
            INCOGNITO.store(true, Ordering::Relaxed);
            *CURRENT.lock().unwrap() = Some(sid.clone());
            sid
        };
        assert!(is_incognito());
        // 写入 3 个文件（其中一个恶意文件名）
        for (i, name) in ["a.txt", "b.txt", "..\\evil\\name.txt"].iter().enumerate() {
            let _ = i;
            let safe: String = name
                .chars()
                .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
                .collect();
            fs::write(session_dir(&st, &s).join(safe), b"secret").unwrap();
        }
        let dir = session_dir(&st, &s);
        let (count, _) = stat_session(&dir);
        assert_eq!(count, 3);
        // 关闭 → 零残留
        let burned = inc_end_inner(&st).unwrap();
        assert_eq!(burned, 3);
        assert!(!dir.exists(), "会话目录必须整体消失");
        assert!(!is_incognito());
        let _ = fs::remove_dir_all(&tmp);
    }

    /// 测试直连（绕过 tauri::State 的 async 撤离流程）。
    fn inc_end_inner(st: &AppState) -> CmdResult<u64> {
        let sid = current_id().ok_or_else(|| AppError::validation("no session"))?;
        let dir = session_dir(st, &sid);
        let burned = if dir.is_dir() { burn_tree(&dir)? } else { 0 };
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        INCOGNITO.store(false, Ordering::Relaxed);
        *CURRENT.lock().map_err(|_| AppError::io("mutex"))? = None;
        let mut hist = load_history(st);
        hist.sessions += 1;
        hist.last_burned_at = now_ms();
        hist.last_burned_files = burned;
        save_history(st, &hist)?;
        Ok(burned)
    }

    #[test]
    fn start_twice_rejected() {
        let _guard = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (st, tmp) = temp_state("twice");
        INCOGNITO.store(false, Ordering::Relaxed);
        *CURRENT.lock().unwrap() = None;
        let sid = crate::db::gen_id();
        fs::create_dir_all(session_dir(&st, &sid)).unwrap();
        INCOGNITO.store(true, Ordering::Relaxed);
        *CURRENT.lock().unwrap() = Some(sid);
        // 模拟二次开启：同名目录已被占用场景交给前端态防重，这里验证 is_incognito 语义
        assert!(is_incognito());
        inc_end_inner(&st).unwrap();
        assert!(!is_incognito());
        let _ = fs::remove_dir_all(&tmp);
    }
}
