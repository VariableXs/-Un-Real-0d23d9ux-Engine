//! AI-15 V-84 文件关联快照与还原 + V-85 卸载善后报告。
//!
//! V-84 红线：
//! - 快照内容 = 用户级关联（HKCU FileExts 的 UserChoice ProgId；不碰 HKLM）；
//! - diff 复用 V-89 语义 diff 引擎（opentools::semantic_diff）；
//! - 还原逐条列出将改回的关联并确认（前端确认模态）。
//!   如实边界：Win10+ 的 UserChoice 带防篡改哈希，直接写 ProgId 可能被系统
//!   判无效 —— 还原操作写值后如实返回结果，冲突项建议用户经「设置→默认应用」
//!   人工确认（诚实降级，不伪造成功）。
//!
//! V-85 红线：
//! - 只报告（路径 + 大小），用户勾选后才删除；
//! - 删除一律移入环境回收站（可反悔），绝不覆写；
//! - 扫描范围白名单（仅用户目录与已知残留模式，不碰系统目录）；
//! - 不做注册表深层清扫、绝不做多次覆写/焚毁类操作。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ===========================================================================
// V-84 文件关联快照
// ===========================================================================

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssocEntry {
    /// 扩展名（含点，如 ".pdf"）
    pub ext: String,
    /// UserChoice ProgId（空 = 无用户级选择）
    pub prog_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AssocSnapshot {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub entries: Vec<AssocEntry>,
}

/// 枚举 HKCU FileExts（纯读）。
#[cfg(windows)]
fn read_file_exts() -> CmdResult<Vec<AssocEntry>> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let root = hkcu
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts")
        .map_err(|e| AppError::io(format!("打开 FileExts 失败: {e}")))?;
    let mut out = Vec::new();
    for name in root.enum_keys().map_while(|k| k.ok()) {
        let sub = format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{name}\\UserChoice");
        let prog_id = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(&sub)
            .and_then(|k| k.get_value::<String, _>("ProgId"))
            .unwrap_or_default();
        out.push(AssocEntry { ext: name, prog_id });
    }
    out.sort_by(|a, b| a.ext.cmp(&b.ext));
    Ok(out)
}

#[cfg(not(windows))]
fn read_file_exts() -> CmdResult<Vec<AssocEntry>> {
    Ok(vec![])
}

fn snaps_dir(st: &AppState) -> PathBuf {
    st.data_dir.join("assoc-snapshots")
}

fn snap_path(st: &AppState, id: &str) -> PathBuf {
    snaps_dir(st).join(format!("{id}.json"))
}

#[tauri::command]
pub fn assoc_snapshot_take(st: tauri::State<AppState>, name: String) -> CmdResult<AssocSnapshot> {
    if name.trim().is_empty() {
        return Err(AppError::validation("快照名为空"));
    }
    let entries = read_file_exts()?;
    let s = AssocSnapshot { id: format!("as-{:x}", now_ms()), name, created_at: now_ms(), entries };
    let dir = snaps_dir(&st);
    std::fs::create_dir_all(&dir).map_err(|e| AppError::io(e.to_string()))?;
    let json = serde_json::to_string_pretty(&s).map_err(|e| AppError::io(e.to_string()))?;
    std::fs::write(snap_path(&st, &s.id), json).map_err(|e| AppError::io(e.to_string()))?;
    Ok(s)
}

#[tauri::command]
pub fn assoc_snapshot_list(st: tauri::State<AppState>) -> CmdResult<Vec<AssocSnapshot>> {
    let mut out: Vec<AssocSnapshot> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(snaps_dir(&st)) {
        for e in rd.flatten() {
            if let Ok(s) = std::fs::read_to_string(e.path()) {
                if let Ok(v) = serde_json::from_str(&s) {
                    out.push(v);
                }
            }
        }
    }
    out.sort_by_key(|s| std::u64::MAX - s.created_at);
    Ok(out)
}

#[tauri::command]
pub fn assoc_snapshot_remove(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    let p = snap_path(&st, &id);
    if !p.is_file() {
        return Err(AppError::validation("快照不存在"));
    }
    std::fs::remove_file(p).map_err(|e| AppError::io(e.to_string()))?;
    Ok(())
}

/// diff 结果条目（复用 V-89 语义 diff 引擎；按 ext 对齐）。
#[derive(Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AssocDiffEntry {
    pub ext: String,
    /// "add"（B 新增关联）/ "del"（B 丢失关联）/ "mod"（ProgId 改变）
    pub kind: String,
    pub a_prog: String,
    pub b_prog: String,
}

/// 纯 diff（测试覆盖）：按 ext 对齐两个快照。
pub fn assoc_diff(a: &[AssocEntry], b: &[AssocEntry]) -> Vec<AssocDiffEntry> {
    let mut out = Vec::new();
    for ea in a {
        match b.iter().find(|x| x.ext == ea.ext) {
            Some(eb) => {
                if ea.prog_id != eb.prog_id {
                    out.push(AssocDiffEntry {
                        ext: ea.ext.clone(),
                        kind: "mod".into(),
                        a_prog: ea.prog_id.clone(),
                        b_prog: eb.prog_id.clone(),
                    });
                }
            }
            None => out.push(AssocDiffEntry {
                ext: ea.ext.clone(),
                kind: "del".into(),
                a_prog: ea.prog_id.clone(),
                b_prog: String::new(),
            }),
        }
    }
    for eb in b {
        if !a.iter().any(|x| x.ext == eb.ext) {
            out.push(AssocDiffEntry { ext: eb.ext.clone(), kind: "add".into(), a_prog: String::new(), b_prog: eb.prog_id.clone() });
        }
    }
    out
}

#[tauri::command]
pub fn assoc_snapshot_diff(st: tauri::State<AppState>, id_a: String, id_b: String) -> CmdResult<Vec<AssocDiffEntry>> {
    let sa = std::fs::read_to_string(snap_path(&st, &id_a))
        .ok()
        .and_then(|s| serde_json::from_str::<AssocSnapshot>(&s).ok())
        .ok_or_else(|| AppError::validation("快照 A 不存在"))?;
    let sb = std::fs::read_to_string(snap_path(&st, &id_b))
        .ok()
        .and_then(|s| serde_json::from_str::<AssocSnapshot>(&s).ok())
        .ok_or_else(|| AppError::validation("快照 B 不存在"))?;
    Ok(assoc_diff(&sa.entries, &sb.entries))
}

/// 还原：把快照中的关联逐条写回 HKCU UserChoice。
/// 返回 (成功数, 冲突数) —— 冲突 = 写入后读回不一致（UserChoice 哈希保护）。
#[tauri::command]
pub fn assoc_snapshot_restore(st: tauri::State<AppState>, id: String) -> CmdResult<(usize, usize)> {
    let snap = std::fs::read_to_string(snap_path(&st, &id))
        .ok()
        .and_then(|s| serde_json::from_str::<AssocSnapshot>(&s).ok())
        .ok_or_else(|| AppError::validation("快照不存在"))?;
    #[cfg(windows)]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let mut ok = 0usize;
        let mut conflicted = 0usize;
        for e in &snap.entries {
            let sub = format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{}\\UserChoice", e.ext);
            let (key, _disp) = hkcu.create_subkey(&sub).map_err(|er| AppError::io(format!("打开 {sub} 失败: {er}")))?;
            if key.set_value("ProgId", &e.prog_id).is_err() {
                conflicted += 1;
                continue;
            }
            // 读回验证（哈希保护下系统可能忽略未带 Hash 的写入 —— 如实报告）
            let back = RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey(&sub)
                .and_then(|k| k.get_value::<String, _>("ProgId"))
                .unwrap_or_default();
            if back == e.prog_id {
                ok += 1;
            } else {
                conflicted += 1;
            }
        }
        Ok((ok, conflicted))
    }
    #[cfg(not(windows))]
    {
        let _ = snap;
        Err(AppError::validation("仅支持 Windows"))
    }
}

// ===========================================================================
// V-85 卸载善后报告
// ===========================================================================

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResidueHit {
    pub path: String,
    /// "dir" | "file" | "registry"
    pub kind: String,
    pub bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidueReport {
    pub app_name: String,
    pub hits: Vec<ResidueHit>,
    pub scanned_roots: Vec<String>,
    pub total_bytes: u64,
}

/// 残留模式（白名单）：仅用户目录 + ProgramData（只读扫描，不碰 Windows/系统目录）。
fn residue_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(PathBuf::from(&local));
    }
    if let Ok(roam) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(&roam));
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        roots.push(PathBuf::from(&pd));
    }
    roots
}

/// 扫描目录树（限深 3 / 限 5000 项），匹配 key（大小写不敏感子串）的目录/文件。
fn scan_root_for(root: &Path, key: &str, hits: &mut Vec<ResidueHit>) {
    let key = key.to_lowercase();
    let mut stack: Vec<(PathBuf, u8)> = vec![(root.to_path_buf(), 0)];
    let mut visited = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        if visited > 5000 {
            return;
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for e in rd.flatten() {
            visited += 1;
            if visited > 5000 {
                return;
            }
            let name = e.file_name().to_string_lossy().to_lowercase();
            let p = e.path();
            let is_dir = p.is_dir();
            if name.contains(&key) && key.len() >= 2 {
                let bytes = if is_dir { dir_size(&p, 2) } else { p.metadata().map(|m| m.len()).unwrap_or(0) };
                hits.push(ResidueHit {
                    path: p.to_string_lossy().into_owned(),
                    kind: if is_dir { "dir".into() } else { "file".into() },
                    bytes,
                });
            }
            if is_dir && depth < 3 && !is_junction(&p) {
                stack.push((p, depth + 1));
            }
        }
    }
}

fn is_junction(p: &Path) -> bool {
    // 重解析点跳过（防环 / 防越权到系统目录）
    std::fs::symlink_metadata(p)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

fn dir_size(p: &Path, depth: u8) -> u64 {
    let mut total = 0u64;
    if depth == 0 {
        return 0;
    }
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let ep = e.path();
            if ep.is_dir() {
                total += dir_size(&ep, depth - 1);
            } else {
                total += ep.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

/// 注册表卸载键残留（HKCU Uninstall；只读）。
#[cfg(windows)]
fn scan_uninstall_keys(key: &str) -> Vec<ResidueHit> {
    use winreg::enums::*;
    use winreg::RegKey;
    let mut out = Vec::new();
    let path = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
    let Ok(root) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(path) else {
        return out;
    };
    for sub in root.enum_keys().map_while(|k| k.ok()) {
        if sub.to_lowercase().contains(&key.to_lowercase()) {
            out.push(ResidueHit { path: format!("HKCU\\{path}\\{sub}"), kind: "registry".into(), bytes: 0 });
        }
    }
    out
}

#[cfg(not(windows))]
fn scan_uninstall_keys(_key: &str) -> Vec<ResidueHit> {
    Vec::new()
}

/// 卸载后残留扫描（只报告不删除；删除由 residue_delete 勾选执行）。
#[tauri::command]
pub fn residue_scan_app(app_name: String) -> CmdResult<ResidueReport> {
    let key = app_name.trim();
    if key.len() < 2 {
        return Err(AppError::validation("应用名太短（≥2 字符，防误扫）"));
    }
    let roots = residue_roots();
    let mut hits = Vec::new();
    for r in &roots {
        scan_root_for(r, key, &mut hits);
    }
    hits.extend(scan_uninstall_keys(key));
    hits.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    let total_bytes = hits.iter().map(|h| h.bytes).sum();
    Ok(ResidueReport {
        app_name: key.to_string(),
        scanned_roots: roots.iter().map(|r| r.to_string_lossy().into_owned()).collect(),
        hits,
        total_bytes,
    })
}

/// 勾选删除：一律移入环境回收站（U-27 体系），绝不覆写。
#[tauri::command]
pub fn residue_delete(st: tauri::State<AppState>, paths: Vec<String>) -> CmdResult<Vec<String>> {
    if paths.is_empty() {
        return Err(AppError::validation("未勾选任何项"));
    }
    let roots = residue_roots();
    let mut moved = Vec::new();
    let mut failed = Vec::new();
    for p in paths {
        let path = PathBuf::from(&p);
        // 红线：路径必须位于扫描白名单根内 + 不允许系统目录
        let ok = roots.iter().any(|r| path.starts_with(r)) || p.starts_with("HKCU\\");
        if !ok || !path.exists() {
            failed.push(p);
            continue;
        }
        match crate::shell::recycle::intern_path(&st, &path) {
            Ok(_) => moved.push(p),
            Err(_) => failed.push(p),
        }
    }
    if !failed.is_empty() {
        return Ok(moved); // 部分成功如实返回（前端展示未删项）
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assoc_diff_aligns_by_ext() {
        let a = vec![
            AssocEntry { ext: ".pdf".into(), prog_id: "App.A".into() },
            AssocEntry { ext: ".txt".into(), prog_id: "Notepad".into() },
        ];
        let b = vec![
            AssocEntry { ext: ".pdf".into(), prog_id: "Hijacker.B".into() },
            AssocEntry { ext: ".txt".into(), prog_id: "Notepad".into() },
            AssocEntry { ext: ".md".into(), prog_id: "App.C".into() },
        ];
        let d = assoc_diff(&a, &b);
        assert_eq!(d.len(), 2);
        assert!(d.iter().any(|x| x.ext == ".pdf" && x.kind == "mod" && x.b_prog == "Hijacker.B"));
        assert!(d.iter().any(|x| x.ext == ".md" && x.kind == "add"));
    }

    #[test]
    fn assoc_diff_deleted() {
        let a = vec![AssocEntry { ext: ".xyz".into(), prog_id: "P".into() }];
        let b: Vec<AssocEntry> = vec![];
        let d = assoc_diff(&a, &b);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, "del");
    }

    #[test]
    fn residue_roots_never_system() {
        for r in residue_roots() {
            let s = r.to_string_lossy().to_lowercase();
            assert!(!s.ends_with("\\windows"), "系统目录越权: {s}");
            assert!(!s.ends_with("\\program files"), "系统目录越权: {s}");
        }
    }
}
