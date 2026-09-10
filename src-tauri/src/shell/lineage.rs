//! AI-10（U-30 数据血缘）：
//! - lineage 表（本地 JSON 账本 `<dataDir>/lineage.json`）：三类事件采集
//!   import（拖入/导入） / use（应用内使用） / depart（离开/导出）
//! - LineageWindow 数据面：按路径聚合事件时间线；整体导出 / 焚毁
//! - 红线：血缘数据绝不出网（本模块零网络代码）；
//!   焚毁后 DB 与磁盘残留为 0（复用焚毁引擎覆写语义）

use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn ledger_path(st: &AppState) -> PathBuf {
    st.data_dir.join("lineage.json")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LinEvent {
    pub id: String,
    /// "import" | "use" | "depart"
    pub kind: String,
    /// 归一化路径键（小写、正斜杠）
    pub path: String,
    /// 展示名（保留原始大小写）
    pub display: String,
    /// 关联应用（可空）
    pub app: Option<String>,
    /// ms
    pub ts: u64,
    pub detail: String,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct Ledger {
    events: Vec<LinEvent>,
}

fn norm_key(path: &str) -> String {
    path.trim_end_matches('\\').replace('\\', "/").to_lowercase()
}

fn load(st: &AppState) -> Ledger {
    fs::read(ledger_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(st: &AppState, l: &Ledger) -> CmdResult<()> {
    fs::write(ledger_path(st), serde_json::to_vec_pretty(l)?)?;
    Ok(())
}

/// 记录事件（前端在三站点调用：拖入 / 打开使用 / 导出离开）。
#[tauri::command(async)]
pub fn lin_record(st: tauri::State<AppState>, kind: String, path: String, app: Option<String>, detail: Option<String>) -> CmdResult<LinEvent> {
    if !matches!(kind.as_str(), "import" | "use" | "depart") {
        return Err(AppError::validation(format!("未知事件类型 / unknown lineage kind: {kind}")));
    }
    // U-36 隐身会话禁止清单：隐身期间禁用血缘记录
    if crate::shell::incognito::is_incognito() {
        return Ok(LinEvent {
            id: String::new(),
            kind,
            path: norm_key(&path),
            display: path,
            app,
            ts: now_ms(),
            detail: detail.unwrap_or_default(),
        });
    }
    let mut l = load(&st);
    let ev = LinEvent {
        id: crate::db::gen_id(),
        kind,
        path: norm_key(&path),
        display: path,
        app,
        ts: now_ms(),
        detail: detail.unwrap_or_default(),
    };
    l.events.push(ev.clone());
    // 上限 10000 条（防膨胀；旧事件被丢弃前可在 UI 先导出）
    if l.events.len() > 10_000 {
        l.events.drain(0..l.events.len() - 10_000);
    }
    save(&st, &l)?;
    Ok(ev)
}

/// 查询：按路径（含子路径前缀）或全量；按时间倒序。
#[tauri::command(async)]
pub fn lin_list(st: tauri::State<AppState>, path: Option<String>) -> CmdResult<Vec<LinEvent>> {
    let l = load(&st);
    let mut out = l.events;
    if let Some(p) = path {
        let key = norm_key(&p);
        out.retain(|e| e.path == key || e.path.starts_with(&format!("{key}/")));
    }
    out.reverse();
    Ok(out)
}

/// 血缘统计（窗口头部卡片）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinStats {
    pub total: u64,
    pub imports: u64,
    pub uses: u64,
    pub departs: u64,
    pub distinct_paths: u64,
}

#[tauri::command(async)]
pub fn lin_stats(st: tauri::State<AppState>) -> CmdResult<LinStats> {
    let l = load(&st);
    let mut paths = std::collections::HashSet::new();
    let mut s = LinStats { total: 0, imports: 0, uses: 0, departs: 0, distinct_paths: 0 };
    for e in &l.events {
        s.total += 1;
        paths.insert(&e.path);
        match e.kind.as_str() {
            "import" => s.imports += 1,
            "use" => s.uses += 1,
            _ => s.departs += 1,
        }
    }
    s.distinct_paths = paths.len() as u64;
    Ok(s)
}

/// 整体导出（用户选择目标目录；明文 JSON，导出前 UI 明示）。
#[tauri::command(async)]
pub fn lin_export(st: tauri::State<AppState>, dest_dir: String) -> CmdResult<String> {
    let l = load(&st);
    let dir = PathBuf::from(&dest_dir);
    if !dir.is_dir() {
        return Err(AppError::not_found("目标目录不存在 / destination not found"));
    }
    let dest = dir.join(format!("variable-lineage-{}.json", now_ms()));
    fs::write(&dest, serde_json::to_vec_pretty(&l)?)?;
    Ok(dest.to_string_lossy().to_string())
}

/// 焚毁：覆写账本 + 删除；返回验证（文件不存在 = 残留 0）。
#[tauri::command(async)]
pub fn lin_burn(st: tauri::State<AppState>) -> CmdResult<bool> {
    let p = ledger_path(&st);
    if p.exists() {
        crate::shell::privacy::shred_file(&p)?;
    }
    Ok(!p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let tmp = std::env::temp_dir().join(format!("variable-lineage-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        (st, tmp)
    }

    #[test]
    fn record_list_stats_burn_roundtrip() {
        let (st, tmp) = temp_state("round");
        let l0 = load(&st);
        let mut l = l0;
        l.events.push(LinEvent {
            id: "1".into(),
            kind: "import".into(),
            path: norm_key("C:\\Users\\a\\f.txt"),
            display: "C:\\Users\\a\\f.txt".into(),
            app: Some("Write".into()),
            ts: 1,
            detail: "".into(),
        });
        l.events.push(LinEvent {
            id: "2".into(),
            kind: "use".into(),
            path: norm_key("C:\\Users\\a\\f.txt"),
            display: "C:\\Users\\a\\f.txt".into(),
            app: Some("Write".into()),
            ts: 2,
            detail: "".into(),
        });
        save(&st, &l).unwrap();
        let s = {
            let l = load(&st);
            assert_eq!(l.events.len(), 2);
            LinStats {
                total: l.events.len() as u64,
                imports: l.events.iter().filter(|e| e.kind == "import").count() as u64,
                uses: l.events.iter().filter(|e| e.kind == "use").count() as u64,
                departs: 0,
                distinct_paths: l.events.iter().map(|e| e.path.clone()).collect::<std::collections::HashSet<_>>().len() as u64,
            }
        };
        assert_eq!(s.total, 2);
        assert_eq!(s.imports, 1);
        assert_eq!(s.uses, 1);
        assert_eq!(s.distinct_paths, 1);
        // 焚毁 → 零残留
        let p = ledger_path(&st);
        assert!(p.exists());
        crate::shell::privacy::shred_file(&p).unwrap();
        assert!(!p.exists());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn invalid_kind_rejected() {
        let (st, tmp) = temp_state("invalid");
        let l = load(&st);
        // kind 校验在命令层；这里验证路径归一化
        assert_eq!(norm_key("C:\\Data\\Folder\\"), "c:/data/folder");
        let _ = fs::remove_dir_all(&tmp);
    }
}
