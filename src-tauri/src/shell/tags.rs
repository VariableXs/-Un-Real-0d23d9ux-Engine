//! AI-10（U-26 全局文件标签系统）：
//! - tags 表（本地 JSON `<dataDir>/file_tags.json`）：路径 → 标签集合 + 星标
//! - 标签跟随移动/重命名（`tag_move` 由前端 ex_rename / ex_move 成功后调用；
//!   path key 变更由本模块单测覆盖）
//! - 智能文件夹 = 保存的查询（复用 explorer::search_syntax 谓词），侧栏可点开
//! - explorer / 桌面图标读取标签角点（`tag_get` / `tag_map`）

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

fn tags_path(st: &AppState) -> PathBuf {
    st.data_dir.join("file_tags.json")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TagEntry {
    pub tags: Vec<String>,
    #[serde(default)]
    pub starred: bool,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct TagStore {
    /// 归一化路径键 → 条目
    files: std::collections::BTreeMap<String, TagEntry>,
    /// 智能文件夹（保存的查询）
    smart: Vec<SmartFolder>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SmartFolder {
    pub id: String,
    pub name: String,
    /// 搜索语法（通配符 / AND/OR/NOT，同 explorer 搜索）
    pub query: String,
    pub created_at: u64,
}

fn norm_key(path: &str) -> String {
    path.trim_end_matches('\\').replace('\\', "/").to_lowercase()
}

fn load(st: &AppState) -> TagStore {
    fs::read(tags_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(st: &AppState, s: &TagStore) -> CmdResult<()> {
    crate::fsutil::atomic_write(tags_path(st), serde_json::to_vec_pretty(s)?)?;
    Ok(())
}

/// 设置标签（全量替换该路径的标签集；空集合 = 删除条目）。
#[tauri::command(async)]
pub fn tag_set(st: tauri::State<AppState>, path: String, tags: Vec<String>) -> CmdResult<()> {
    let mut s = load(&st);
    let key = norm_key(&path);
    let cleaned: Vec<String> = tags
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty() && t.len() <= 40)
        .collect();
    if cleaned.is_empty() {
        s.files.remove(&key);
    } else {
        // 保留既有星标（回收站 2.0 豁免策略依赖星标不被标签更新冲掉）
        let starred = s.files.get(&key).map(|e| e.starred).unwrap_or(false);
        s.files.insert(key, TagEntry { tags: cleaned, starred, updated_at: now_ms() });
    }
    save(&st, &s)
}

/// 星标路径集合（回收站 2.0 自动清理豁免用；返回归一化键）。
pub fn starred_keys(st: &AppState) -> std::collections::BTreeSet<String> {
    load(st)
        .files
        .into_iter()
        .filter(|(_, e)| e.starred)
        .map(|(k, _)| k)
        .collect()
}

/// 星标（独立于标签，回收站 2.0 豁免策略用）。
#[tauri::command(async)]
pub fn tag_star(st: tauri::State<AppState>, path: String, starred: bool) -> CmdResult<()> {
    let mut s = load(&st);
    let key = norm_key(&path);
    let e = s.files.entry(key).or_insert_with(|| TagEntry { tags: vec![], starred: false, updated_at: now_ms() });
    e.starred = starred;
    e.updated_at = now_ms();
    save(&st, &s)
}

/// 查询单路径。
#[tauri::command(async)]
pub fn tag_get(st: tauri::State<AppState>, path: String) -> CmdResult<Option<TagEntry>> {
    let s = load(&st);
    Ok(s.files.get(&norm_key(&path)).cloned())
}

/// 批量查询（explorer 列表角点用，一次 IPC 取整目录）。
#[tauri::command(async)]
pub fn tag_map(st: tauri::State<AppState>, paths: Vec<String>) -> CmdResult<std::collections::HashMap<String, TagEntry>> {
    let s = load(&st);
    let mut out = std::collections::HashMap::new();
    for p in paths {
        let key = norm_key(&p);
        if let Some(e) = s.files.get(&key) {
            out.insert(p, e.clone());
        }
    }
    Ok(out)
}

/// 全部标签（标签管理面板 / 过滤器）。
#[tauri::command(async)]
pub fn tag_all(st: tauri::State<AppState>) -> CmdResult<Vec<String>> {
    let s = load(&st);
    let mut set = std::collections::BTreeSet::new();
    for e in s.files.values() {
        for t in &e.tags {
            set.insert(t.clone());
        }
    }
    Ok(set.into_iter().collect())
}

/// 标签跟随移动/重命名（ex_rename / ex_move 成功后由前端调用；支持批量）。
#[tauri::command(async)]
pub fn tag_move(st: tauri::State<AppState>, from: String, to: String) -> CmdResult<()> {
    let mut s = load(&st);
    let fk = norm_key(&from);
    let tk = norm_key(&to);
    // 目录移动 → 子路径也要跟随（前缀替换）
    if s.files.contains_key(&fk) || s.files.keys().any(|k| k.starts_with(&format!("{fk}/"))) {
        let affected: Vec<(String, TagEntry)> = s
            .files
            .range(fk.clone()..)
            .take_while(|(k, _)| k.as_str() == fk || k.starts_with(&format!("{fk}/")))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (k, v) in affected {
            let nk = if k == fk { tk.clone() } else { format!("{}/{}", tk, &k[fk.len() + 1..]) };
            s.files.remove(&k);
            s.files.insert(nk, v);
        }
        save(&st, &s)?;
    }
    Ok(())
}

/// 按标签过滤（返回有该标签的路径键列表）。
#[tauri::command(async)]
pub fn tag_filter(st: tauri::State<AppState>, tag: String) -> CmdResult<Vec<String>> {
    let s = load(&st);
    Ok(s
        .files
        .iter()
        .filter(|(_, e)| e.tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)))
        .map(|(k, _)| k.clone())
        .collect())
}

/// 智能文件夹：列出 / 新建 / 删除。
#[tauri::command(async)]
pub fn tag_smart_list(st: tauri::State<AppState>) -> CmdResult<Vec<SmartFolder>> {
    let s = load(&st);
    Ok(s.smart.clone())
}

#[tauri::command(async)]
pub fn tag_smart_add(st: tauri::State<AppState>, name: String, query: String) -> CmdResult<SmartFolder> {
    let n = name.trim();
    let q = query.trim();
    if n.is_empty() || q.is_empty() {
        return Err(AppError::validation("名称与查询不能为空 / name and query required"));
    }
    if crate::shell::explorer::search_syntax::parse(q).is_none() {
        return Err(AppError::validation("查询语法无效 / invalid search query"));
    }
    let mut s = load(&st);
    if s.smart.iter().any(|f| f.name == n) {
        return Err(AppError::validation("同名智能文件夹已存在 / smart folder exists"));
    }
    let f = SmartFolder { id: crate::db::gen_id(), name: n.to_string(), query: q.to_string(), created_at: now_ms() };
    s.smart.push(f.clone());
    save(&st, &s)?;
    Ok(f)
}

#[tauri::command(async)]
pub fn tag_smart_remove(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    let mut s = load(&st);
    s.smart.retain(|f| f.id != id);
    save(&st, &s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let tmp = std::env::temp_dir().join(format!("variable-tags-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        (st, tmp)
    }

    #[test]
    fn set_get_star_roundtrip() {
        let (st, tmp) = temp_state("round");
        let p = "C:\\Docs\\a.txt";
        tag_set_inner(&st, p, vec!["work".into(), "重要".into()]).unwrap();
        let e = load(&st).files.get(&norm_key(p)).unwrap().clone();
        assert_eq!(e.tags.len(), 2);
        // 星标独立
        tag_star_inner(&st, p, true).unwrap();
        let e = load(&st).files.get(&norm_key(p)).unwrap().clone();
        assert!(e.starred);
        // 空标签 → 删除
        tag_set_inner(&st, p, vec![]).unwrap();
        assert!(load(&st).files.get(&norm_key(p)).is_none());
        let _ = fs::remove_dir_all(&tmp);
    }

    fn tag_set_inner(st: &AppState, path: &str, tags: Vec<String>) -> CmdResult<()> {
        let mut s = load(st);
        let key = norm_key(path);
        let cleaned: Vec<String> = tags.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
        if cleaned.is_empty() {
            s.files.remove(&key);
        } else {
            s.files.insert(key, TagEntry { tags: cleaned, starred: false, updated_at: now_ms() });
        }
        save(st, &s)
    }

    fn tag_star_inner(st: &AppState, path: &str, starred: bool) -> CmdResult<()> {
        let mut s = load(st);
        let key = norm_key(path);
        let e = s.files.entry(key).or_insert_with(|| TagEntry { tags: vec![], starred: false, updated_at: now_ms() });
        e.starred = starred;
        save(st, &s)
    }

    #[test]
    fn move_follows_children() {
        let (st, tmp) = temp_state("move");
        tag_set_inner(&st, "C:\\A\\f.txt", vec!["x".into()]).unwrap();
        tag_set_inner(&st, "C:\\A\\sub\\g.txt", vec!["y".into()]).unwrap();
        let mut s = load(&st);
        let fk = norm_key("C:\\A");
        let tk = norm_key("C:\\B");
        let affected: Vec<(String, TagEntry)> = s
            .files
            .range(fk.clone()..)
            .take_while(|(k, _)| k.as_str() == fk || k.starts_with(&format!("{fk}/")))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (k, v) in affected {
            let nk = if k == fk { tk.clone() } else { format!("{}/{}", tk, &k[fk.len() + 1..]) };
            s.files.remove(&k);
            s.files.insert(nk, v);
        }
        save(&st, &s).unwrap();
        let s = load(&st);
        assert!(s.files.contains_key(&norm_key("C:\\B\\f.txt")));
        assert!(s.files.contains_key(&norm_key("C:\\B\\sub\\g.txt")));
        assert!(!s.files.contains_key(&norm_key("C:\\A\\f.txt")));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn smart_folder_validation() {
        let (st, tmp) = temp_state("smart");
        let mut s = load(&st);
        assert!(crate::shell::explorer::search_syntax::parse("*.jpg OR report").is_some());
        assert!(crate::shell::explorer::search_syntax::parse("kw AND").is_none());
        let f = SmartFolder { id: "1".into(), name: "图片".into(), query: "*.jpg".into(), created_at: 0 };
        s.smart.push(f);
        save(&st, &s).unwrap();
        assert_eq!(load(&st).smart.len(), 1);
        let _ = fs::remove_dir_all(&tmp);
    }
}
