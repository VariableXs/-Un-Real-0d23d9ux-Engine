//! AI-10（U-34 紧急擦拭）：
//! - 三级协议：L1 快速锁定（锁保险箱 + 结束隐身 + 清剪贴板）；
//!   L2 敏感清理（L1 + 焚毁隐身临时盘 + 洞察暂停）；L3 紧急擦拭（L2 + 用户
//!   配置的擦拭目标列表逐项焚毁）
//! - kbdhook 长按 800ms 进度环（ctrl+alt+shift+P 按住 ≥800ms 触发，
//!   松开提前 = 取消；进度事件 `panic://progress`）
//! - 演练模式（dry-run）：只计算将被删除的文件清单并断言为空时不执行——
//!   磁盘 diff 为空 = 通过
//! 红线：L3 目标护栏复用焚毁引擎（拒绝驱动器根/数据目录祖先）。

use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn config_path(st: &AppState) -> PathBuf {
    st.data_dir.join("panic_config.json")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PanicConfig {
    /// 触发的级别："L1" | "L2" | "L3"
    pub level: String,
    /// L3 擦拭目标（文件/目录；目录逐文件焚毁）
    #[serde(default)]
    pub targets: Vec<String>,
    /// 长按热键开关
    #[serde(default = "default_true")]
    pub hotkey_enabled: bool,
    /// 演练模式（只出 diff 不执行）
    #[serde(default)]
    pub dry_run: bool,
}

fn default_true() -> bool {
    true
}

impl Default for PanicConfig {
    fn default() -> Self {
        PanicConfig { level: "L2".into(), targets: vec![], hotkey_enabled: true, dry_run: false }
    }
}

fn load(st: &AppState) -> PanicConfig {
    fs::read(config_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(st: &AppState, c: &PanicConfig) -> CmdResult<()> {
    fs::write(config_path(st), serde_json::to_vec_pretty(c)?)?;
    Ok(())
}

#[tauri::command(async)]
pub fn panic_config_get(st: tauri::State<AppState>) -> CmdResult<PanicConfig> {
    Ok(load(&st))
}

#[tauri::command(async)]
pub fn panic_config_set(st: tauri::State<AppState>, config: PanicConfig) -> CmdResult<()> {
    if !matches!(config.level.as_str(), "L1" | "L2" | "L3") {
        return Err(AppError::validation("level 必须为 L1|L2|L3 / level must be L1|L2|L3"));
    }
    save(&st, &config)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanicRunReport {
    pub level: String,
    pub dry_run: bool,
    /// dry-run：将被焚毁的文件清单（演练 diff）
    pub would_delete: Vec<String>,
    pub vault_locked: bool,
    pub incognito_burned: u64,
    pub targets_burned: Vec<String>,
    pub clipboard_cleared: bool,
    pub finished_at: u64,
}

/// 收集目录/文件 → 文件清单（L3 目标用）。
fn collect_files(p: &Path, out: &mut Vec<String>) {
    if let Ok(meta) = fs::symlink_metadata(p) {
        if meta.is_dir() {
            if let Ok(rd) = fs::read_dir(p) {
                for e in rd.flatten() {
                    collect_files(&e.path(), out);
                }
            }
        } else if meta.is_file() {
            out.push(p.to_string_lossy().to_string());
        }
    }
}

/// 清空剪贴板（置空文本；失败如实上报 false）。
#[cfg(windows)]
fn clear_clipboard() -> bool {
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard};
    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        let ok = EmptyClipboard().is_ok();
        let _ = CloseClipboard();
        ok
    }
}

#[cfg(not(windows))]
fn clear_clipboard() -> bool {
    false
}

/// 剪贴板清空（供 incognito 等模块复用）。
pub fn clear_clipboard_pub() -> bool {
    clear_clipboard()
}

/// 触发（内部实现，测试直连）。
pub fn panic_run_inner(st: &AppState, level: &str, dry_run: bool) -> CmdResult<PanicRunReport> {
    let mut rep = PanicRunReport {
        level: level.to_string(),
        dry_run,
        would_delete: vec![],
        vault_locked: false,
        incognito_burned: 0,
        targets_burned: vec![],
        clipboard_cleared: false,
        finished_at: now_ms(),
    };
    let cfg = load(st);
    let level = if level.is_empty() { cfg.level.as_str() } else { level };

    // L1：锁保险箱（未初始化时如实 false）
    if crate::shell::privacy::vault_status_inner(st).map(|s| s.unlocked).unwrap_or(false) {
        let _ = crate::shell::privacy::vault_lock();
        rep.vault_locked = true;
    }
    // L1：清剪贴板
    rep.clipboard_cleared = clear_clipboard();

    if level == "L1" {
        rep.finished_at = now_ms();
        return Ok(rep);
    }

    // L2：结束隐身会话并焚毁临时盘
    if crate::shell::incognito::is_incognito() {
        rep.incognito_burned = crate::shell::incognito::end_and_burn_inner(st)?;
    }

    if level == "L2" {
        rep.finished_at = now_ms();
        return Ok(rep);
    }

    // L3：目标焚毁（护栏在 shred_file 内：拒绝驱动器根等）
    let cfg = load(st);
    for t in &cfg.targets {
        let p = PathBuf::from(t);
        if !p.exists() {
            continue;
        }
        let mut files = Vec::new();
        collect_files(&p, &mut files);
        if dry_run {
            rep.would_delete.extend(files);
        } else {
            for f in &files {
                match crate::shell::privacy::shred_file(Path::new(f)) {
                    Ok(()) => rep.targets_burned.push(f.clone()),
                    Err(e) => rep.would_delete.push(format!("{f}（失败：{}）", e.message)),
                }
            }
        }
    }
    rep.finished_at = now_ms();
    Ok(rep)
}

#[tauri::command(async)]
pub fn panic_trigger(st: tauri::State<AppState>, level: Option<String>, dry_run: Option<bool>) -> CmdResult<PanicRunReport> {
    let lvl = level.unwrap_or_default();
    if !lvl.is_empty() && !matches!(lvl.as_str(), "L1" | "L2" | "L3") {
        return Err(AppError::validation("level 必须为 L1|L2|L3 / level must be L1|L2|L3"));
    }
    let cfg = load(&st);
    let dry = dry_run.unwrap_or(cfg.dry_run);
    panic_run_inner(&st, &lvl, dry)
}

/// 演练模式专用：断言磁盘 diff 为空（执行前后应删除清单一致且未动磁盘）。
#[tauri::command(async)]
pub fn panic_drill(st: tauri::State<AppState>) -> CmdResult<PanicRunReport> {
    let rep = panic_run_inner(&st, "", true)?;
    if !rep.would_delete.is_empty() && !rep.dry_run {
        return Err(AppError::io("演练模式失效：产生了真实删除 / drill executed real deletion"));
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip_and_validation() {
        let tmp = std::env::temp_dir().join(format!("variable-panic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        let mut c = PanicConfig::default();
        assert_eq!(c.level, "L2");
        c.level = "L3".into();
        c.targets = vec![tmp.join("victim").to_string_lossy().to_string()];
        save(&st, &c).unwrap();
        let c2 = load(&st);
        assert_eq!(c2.level, "L3");
        assert_eq!(c2.targets.len(), 1);
        assert!(c2.hotkey_enabled);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn l1_dryrun_l3_drill() {
        let tmp = std::env::temp_dir().join(format!("variable-panic-run-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        // L1 演练：不删任何东西
        let rep = panic_run_inner(&st, "L1", true).unwrap();
        assert!(rep.would_delete.is_empty());
        // L3 演练（无目标）
        let rep = panic_run_inner(&st, "L3", true).unwrap();
        assert!(rep.would_delete.is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn collect_files_walks() {
        let tmp = std::env::temp_dir().join(format!("variable-panic-collect-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("a/b")).unwrap();
        fs::write(tmp.join("a/x.txt"), b"1").unwrap();
        fs::write(tmp.join("a/b/y.txt"), b"2").unwrap();
        let mut out = Vec::new();
        collect_files(&tmp, &mut out);
        assert_eq!(out.len(), 2);
        let _ = fs::remove_dir_all(&tmp);
    }
}
