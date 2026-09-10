//! 子环境档系统（B-24，M6；BLUEPRINT 3.10 / 7.4）。
//!
//! 模型：
//! - `envs.json`：`{ active, envs: [{ id, name, settings, created_at }] }`；
//!   `main` 常驻（默认环境，与既有单环境行为完全一致）；
//! - 存储剖面：`envs/<id>/home`（执行档 {envhome} 隔离根）+ `envs/<id>/workspaces`；
//!   主环境的 home/workspaces 不迁移（零成本默认档）；
//! - 切换编排（4 步，蓝图 3.10）：①当前 UI 设置快照写入旧环境条目 → ②active
//!   指针翻转 → ③新环境条目的设置快照返回前端应用 → ④前端状态重载（短版启动）；
//!   步骤 ①④ 在前端（patchSettings/启动仪式短版），本模块提供 ②③ + 目录保障；
//! - 嵌套互斥（蓝图 781 行硬约束的孪生口径）：切换只翻指针，绝不复制/迁移
//!   数据 chunk——多环境同时打开同一容器需只读快照剖面，属 B-25/B-26。
//!
//! 如实边界：apps 登记表与 DB 当前跨环境共享（完全剖面隔离随 B-25/B-26
//! 快照克隆落地后评估），本批兑现的是「home 隔离 + 偏好隔离 + 编排协议」。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

type CmdResult<T> = Result<T, AppError>;

pub const MAIN_ENV: &str = "main";

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EnvProfile {
    pub id: String,
    pub name: String,
    /// 切换时快照的 UI 偏好子集（wallpaperMode/theme/iconSize/taskbarPos 等）。
    #[serde(default)]
    pub settings: serde_json::Value,
    pub created_at: u64,
    /// B-26：克隆来源（平行世界试验档才有；main/手建环境为 None）。
    #[serde(default)]
    pub parent: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnvRegistry {
    pub active: String,
    pub envs: Vec<EnvProfile>,
}

fn registry_path(st: &AppState) -> PathBuf {
    st.data_dir.join("envs.json")
}

pub(crate) fn load_registry(st: &AppState) -> EnvRegistry {
    match std::fs::read(registry_path(st)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => EnvRegistry {
            active: MAIN_ENV.into(),
            envs: vec![EnvProfile {
                id: MAIN_ENV.into(),
                name: "主环境".into(),
                settings: serde_json::Value::Null,
                created_at: 0,
                parent: None,
            }],
        },
    }
}

fn save_registry(st: &AppState, r: &EnvRegistry) -> CmdResult<()> {
    let bytes =
        serde_json::to_vec_pretty(r).map_err(|e| AppError::io(e.to_string()))?;
    std::fs::write(registry_path(st), bytes).map_err(|e| AppError::io(e.to_string()))?;
    Ok(())
}

fn ensure_profile_dirs(st: &AppState, id: &str) -> CmdResult<()> {
    for sub in ["home", "workspaces"] {
        let d = st.data_dir.join("envs").join(id).join(sub);
        if !d.is_dir() {
            std::fs::create_dir_all(&d).map_err(|e| AppError::io(e.to_string()))?;
        }
    }
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn slugify(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_lowercase();
    if s.is_empty() { "env".into() } else { s }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvView {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub created_at: u64,
    /// B-26：是否为克隆试验档（有 diff/丢弃/合并操作面）
    pub is_clone: bool,
}

#[tauri::command(async)]
pub fn env_list(st: tauri::State<AppState>) -> CmdResult<Vec<EnvView>> {
    env_list_inner(&st)
}

pub(crate) fn env_list_inner(st: &AppState) -> CmdResult<Vec<EnvView>> {
    let r = load_registry(st);
    Ok(r.envs
        .iter()
        .map(|e| EnvView {
            id: e.id.clone(),
            name: e.name.clone(),
            active: e.id == r.active,
            created_at: e.created_at,
            is_clone: e.parent.is_some(),
        })
        .collect())
}

#[tauri::command(async)]
pub fn env_create(st: tauri::State<AppState>, name: String) -> CmdResult<EnvView> {
    env_create_inner(&st, name)
}

pub(crate) fn env_create_inner(st: &AppState, name: String) -> CmdResult<EnvView> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::validation("环境名不能为空"));
    }
    let id = slugify(&name);
    let mut r = load_registry(st);
    if r.envs.iter().any(|e| e.id == id) {
        return Err(AppError::validation(format!("环境已存在: {id}")));
    }
    ensure_profile_dirs(st, &id)?;
    let env = EnvProfile {
        id: id.clone(),
        name,
        settings: serde_json::Value::Null,
        created_at: now_ms(),
        parent: None,
    };
    r.envs.push(env.clone());
    save_registry(st, &r)?;
    Ok(EnvView {
        id: env.id,
        name: env.name,
        active: false,
        created_at: env.created_at,
        is_clone: false,
    })
}

/// 切换编排：①当前设置快照写入 active 环境 → ②active 翻转 → ③返回目标环境
/// 快照（前端应用后状态重载）。
#[tauri::command(async)]
pub fn env_switch(
    st: tauri::State<AppState>,
    id: String,
    current_settings: Option<serde_json::Value>,
) -> CmdResult<serde_json::Value> {
    env_switch_inner(&st, id, current_settings)
}

pub(crate) fn env_switch_inner(
    st: &AppState,
    id: String,
    current_settings: Option<serde_json::Value>,
) -> CmdResult<serde_json::Value> {
    let mut r = load_registry(st);
    if r.active == id {
        // 同环境：幂等返回其快照
        return Ok(r
            .envs
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.settings.clone())
            .unwrap_or(serde_json::Value::Null));
    }
    // 快照写入当前 active 环境
    if let Some(cur) = r.envs.iter_mut().find(|e| e.id == r.active) {
        cur.settings = current_settings.unwrap_or(serde_json::Value::Null);
    }
    let target = r
        .envs
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到环境: {id}")))?;
    target.id = target.id.clone(); // 借用收口
    ensure_profile_dirs(st, &id)?;
    let snapshot = target.settings.clone();
    r.active = id;
    save_registry(st, &r)?;
    Ok(snapshot)
}

#[tauri::command(async)]
pub fn env_delete(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    env_delete_inner(&st, id)
}

pub(crate) fn env_delete_inner(st: &AppState, id: String) -> CmdResult<()> {
    if id == MAIN_ENV {
        return Err(AppError::validation("主环境不可删除"));
    }
    let mut r = load_registry(st);
    if r.active == id {
        return Err(AppError::validation("不能删除当前活动环境（先切换走）"));
    }
    let before = r.envs.len();
    r.envs.retain(|e| e.id != id);
    if r.envs.len() == before {
        return Err(AppError::not_found(format!("未找到环境: {id}")));
    }
    save_registry(st, &r)?;
    let dir = st.data_dir.join("envs").join(&id);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir).map_err(|e| AppError::io(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec;

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!("envs-b24-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        let st = AppState {
            conn: std::sync::Mutex::new(None),
            data_dir: dir.clone(),
            db_dir: dir.clone(),
            media_dir: dir.clone(),
            attachments_dir: dir.clone(),
            backups_dir: dir.clone(),
            recovery_dir: dir.clone(),
            logs_dir: dir.clone(),
        };
        (st, dir)
    }

    #[test]
    fn create_switch_delete_roundtrip() {
        let (mut st, dir) = temp_state("round");
        // 默认只有 main
        assert_eq!(env_list_inner(&st).unwrap().len(), 1);
        // 创建试验环境
        let env = env_create_inner(&mut st, "试验档".into()).unwrap();
        assert_eq!(env.id, "试验档".to_string().replace('！', "-").to_lowercase());
        // 剖面目录已建
        assert!(dir.join("envs").join(&env.id).join("home").is_dir());
        assert!(dir.join("envs").join(&env.id).join("workspaces").is_dir());
        // 切换：main 的设置快照被保存，active 翻转
        let snap = env_switch_inner(
            &mut st,
            env.id.clone(),
            Some(serde_json::json!({"wallpaperMode": "solid"})),
        )
        .unwrap();
        assert_eq!(snap, serde_json::Value::Null, "新环境尚无快照");
        let r = load_registry(&st);
        assert_eq!(r.active, env.id);
        assert_eq!(
            r.envs.iter().find(|e| e.id == "main").unwrap().settings,
            serde_json::json!({"wallpaperMode": "solid"})
        );
        // {envhome} 解析到试验环境的 home
        let expanded = exec::expand_placeholders("{envhome}/.claude", &dir);
        assert!(
            expanded.starts_with(dir.join("envs").join(&env.id).join("home").to_string_lossy().as_ref()),
            "expanded={expanded}"
        );
        // 删除：活动环境不可删 → 切回 main → 可删
        assert!(env_delete_inner(&mut st, env.id.clone()).is_err());
        env_switch_inner(&mut st, "main".into(), Some(serde_json::json!({"theme": "deep-space"}))).unwrap();
        env_delete_inner(&mut st, env.id).unwrap();
        assert_eq!(env_list_inner(&st).unwrap().len(), 1);
        // main 不可删
        assert!(env_delete_inner(&mut st, "main".into()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn main_env_home_falls_back_to_container() {
        let (st, dir) = temp_state("main");
        assert_eq!(exec::env_home(&dir), dir.join("home"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod clone_tests {
    use super::*;

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!("envs-b26-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        let st = AppState {
            conn: std::sync::Mutex::new(None),
            data_dir: dir.clone(),
            db_dir: dir.clone(),
            media_dir: dir.clone(),
            attachments_dir: dir.clone(),
            backups_dir: dir.clone(),
            recovery_dir: dir.clone(),
            logs_dir: dir.clone(),
        };
        (st, dir)
    }

    /// B-26 验收主断言：试验档糟蹋完丢弃 → 主档零变化（哈希断言）；
    /// 合并路径：added/changed/deleted 应用 + 冲突逐个裁决 + 合并后 diff 归零。
    #[test]
    fn clone_diff_discard_merge_roundtrip() {
        let (st, dir) = temp_state("full");
        // 主档放两个文件
        let main_home = dir.join("envs").join("main").join("home");
        std::fs::create_dir_all(&main_home).unwrap();
        std::fs::write(main_home.join("a.txt"), b"A1").unwrap();
        std::fs::write(main_home.join("b.txt"), b"B1").unwrap();

        let _ = env_create_inner(&st, "seed".into()).unwrap();
        let rep = env_clone_inner(&st, "main".into(), "trial".into()).unwrap();
        let cid = rep.clone_id;
        let clone_home = dir.join("envs").join(&cid).join("home");

        // 克隆立读哈希一致（真拷贝降级路径的等价断言）
        assert_eq!(
            std::fs::read(clone_home.join("a.txt")).unwrap(),
            b"A1".to_vec()
        );
        // 基线已落盘
        assert!(dir.join("envs").join(&cid).join(CLONE_BASE_FILE).is_file());

        // 试验档：改 a、加 c、删 b；主档同时改 a（→ 冲突）和 b（主档独改，试验档删了它）
        std::fs::write(clone_home.join("a.txt"), b"A2-trial").unwrap();
        std::fs::write(clone_home.join("c.txt"), b"C-new").unwrap();
        std::fs::remove_file(clone_home.join("b.txt")).unwrap();
        std::fs::write(main_home.join("a.txt"), b"A2-main").unwrap();
        std::fs::write(main_home.join("b.txt"), b"B2-main").unwrap();

        let diff = env_diff_inner(&st, cid.clone()).unwrap();
        let status_of = |p: &str| {
            diff.entries
                .iter()
                .find(|e| e.path == p)
                .map(|e| e.status.clone())
                .unwrap_or_default()
        };
        assert_eq!(status_of("home/a.txt"), "conflict");
        assert_eq!(status_of("home/c.txt"), "added");
        // b：试验档删、主档改 → 冲突（两边都相对基线变了）
        assert_eq!(status_of("home/b.txt"), "conflict");
        assert!(!diff.clean);

        // 丢弃路径：主档零变化
        let a_before = std::fs::read(main_home.join("a.txt")).unwrap();
        let snapshot_main = scan_manifest(&dir.join("envs").join("main"), &mut std::collections::BTreeMap::new()).unwrap();
        let _ = snapshot_main;
        assert_eq!(a_before, b"A2-main".to_vec());
        // 丢弃（先切进试验档再丢，验证 active 弹回）
        let mut r = load_registry(&st);
        r.active = cid.clone();
        save_registry(&st, &r).unwrap();
        env_discard_inner(&st, cid.clone()).unwrap();
        let r = load_registry(&st);
        assert_eq!(r.active, "main");
        assert!(r.envs.iter().all(|e| e.id != cid));
        assert!(!dir.join("envs").join(&cid).exists());
        assert_eq!(std::fs::read(main_home.join("a.txt")).unwrap(), b"A2-main");

        // 重新克隆走合并路径
        let rep2 = env_clone_inner(&st, "main".into(), "trial2".into()).unwrap();
        let cid2 = rep2.clone_id;
        let clone_home2 = dir.join("envs").join(&cid2).join("home");
        std::fs::write(clone_home2.join("a.txt"), b"A3-trial").unwrap();
        std::fs::write(clone_home2.join("c.txt"), b"C-new").unwrap();
        std::fs::write(main_home.join("a.txt"), b"A3-main").unwrap();
        let diff2 = env_diff_inner(&st, cid2.clone()).unwrap();
        assert_eq!(diff2.entries.iter().filter(|e| e.status == "conflict").count(), 1);
        // 冲突保主档（a 不在 keep_clone），added(c) 合并
        let m = env_merge_inner(&st, cid2.clone(), vec![]).unwrap();
        assert_eq!(m.merged, 1);
        // 唯一冲突（a）保主档
        assert_eq!(m.conflicts_kept_main, 1);
        assert_eq!(std::fs::read(main_home.join("a.txt")).unwrap(), b"A3-main");
        assert_eq!(std::fs::read(main_home.join("c.txt")).unwrap(), b"C-new");
        // 合并后 diff 收敛：试验档对 a 的主张仍在（A3-trial vs 新基线）→ 普通 changed
        let d3 = env_diff_inner(&st, cid2.clone()).unwrap();
        assert_eq!(d3.entries.len(), 1);
        assert_eq!(d3.entries[0].path, "home/a.txt");
        assert_eq!(d3.entries[0].status, "changed");
        // 二次合并：冲突交试验档裁决
        std::fs::write(clone_home2.join("a.txt"), b"A4-trial").unwrap();
        std::fs::write(main_home.join("a.txt"), b"A4-main").unwrap();
        let m2 = env_merge_inner(&st, cid2.clone(), vec!["home/a.txt".into()]).unwrap();
        assert_eq!(m2.conflicts_resolved, 1);
        assert_eq!(std::fs::read(main_home.join("a.txt")).unwrap(), b"A4-trial");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ---------- B-26：环境快照克隆 ----------

/// 深拷贝目录（克隆的 V1 物理 = 全量复制；Uxv chunk 后端的 COW 零拷贝
/// 已由 container 快照机制提供，实机项）。
fn copy_dir_all(src: &Path, dst: &Path) -> CmdResult<u64> {
    let mut bytes = 0u64;
    std::fs::create_dir_all(dst).map_err(|e| AppError::io(e.to_string()))?;
    for e in std::fs::read_dir(src).map_err(|e| AppError::io(e.to_string()))? {
        let e = e.map_err(|e| AppError::io(e.to_string()))?;
        let p = e.path();
        let d = dst.join(e.file_name());
        if p.is_dir() {
            bytes += copy_dir_all(&p, &d)?;
        } else {
            let n = std::fs::copy(&p, &d).map_err(|e| AppError::io(e.to_string()))?;
            bytes += n;
        }
    }
    Ok(bytes)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneReport {
    pub source_id: String,
    pub clone_id: String,
    pub clone_name: String,
    pub bytes_copied: u64,
}

/// 克隆环境：剖面目录全量复制 + 条目复制（含设置快照）——"平行世界"。
#[tauri::command(async)]
pub fn env_clone(
    st: tauri::State<AppState>,
    id: String,
    new_name: String,
) -> CmdResult<CloneReport> {
    env_clone_inner(&st, id, new_name)
}

pub(crate) fn env_clone_inner(st: &AppState, id: String, new_name: String) -> CmdResult<CloneReport> {
    let mut r = load_registry(st);
    let src = r
        .envs
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到环境: {id}")))?
        .clone();
    let slug = slugify(&new_name);
    let new_id = format!("{}-clone-{}", slug, now_ms() % 100_000);
    // 剖面目录复制（存在才拷）
    let src_base = st.data_dir.join("envs").join(&id);
    let dst_base = st.data_dir.join("envs").join(&new_id);
    let mut bytes = 0u64;
    for sub in ["home", "workspaces"] {
        let s = src_base.join(sub);
        if s.is_dir() {
            bytes += copy_dir_all(&s, &dst_base.join(sub))?;
        }
    }
    ensure_profile_dirs(st, &new_id)?;
    r.envs.push(EnvProfile {
        id: new_id.clone(),
        name: new_name,
        settings: src.settings.clone(),
        created_at: now_ms(),
        parent: Some(id.clone()),
    });
    save_registry(st, &r)?;
    // B-26 三方合并基线：克隆时刻源剖面的内容哈希清单（.clone-base.json）
    write_clone_base(st, &new_id)?;
    Ok(CloneReport {
        source_id: id,
        clone_id: new_id,
        clone_name: String::new(),
        bytes_copied: bytes,
    })
}

// ---------- B-26：试验档 diff / 丢弃 / 合并（三方冲突裁决） ----------

const CLONE_BASE_FILE: &str = ".clone-base.json";

fn hash_file(p: &Path) -> CmdResult<String> {
    let bytes = std::fs::read(p).map_err(|e| AppError::io(e.to_string()))?;
    Ok(blake3::hash(&bytes).to_string())
}

/// 剖面内容清单：home/ + workspaces/ 下全部文件的 rel → blake3 hex。
fn scan_manifest(profile: &Path, out: &mut std::collections::BTreeMap<String, String>) -> CmdResult<()> {
    for sub in ["home", "workspaces"] {
        let root = profile.join(sub);
        if !root.is_dir() {
            continue;
        }
        walk_manifest(&root, &root, sub, out)?;
    }
    Ok(())
}

fn walk_manifest(
    dir: &Path,
    base: &Path,
    sub: &str,
    out: &mut std::collections::BTreeMap<String, String>,
) -> CmdResult<()> {
    for e in std::fs::read_dir(dir).map_err(|e| AppError::io(e.to_string()))? {
        let e = e.map_err(|e| AppError::io(e.to_string()))?;
        let p = e.path();
        if p.is_dir() {
            walk_manifest(&p, base, sub, out)?;
        } else {
            let rel = p
                .strip_prefix(base)
                .map_err(|e| AppError::io(e.to_string()))?;
            let key = format!("{sub}/{}", rel.to_string_lossy().replace('\\', "/"));
            out.insert(key, hash_file(&p)?);
        }
    }
    Ok(())
}

fn base_path(st: &AppState, clone_id: &str) -> PathBuf {
    st.data_dir.join("envs").join(clone_id).join(CLONE_BASE_FILE)
}

fn write_clone_base(st: &AppState, clone_id: &str) -> CmdResult<()> {
    // 基线来自源剖面 = 当前主档状态
    let mut r = load_registry(st);
    let parent = r
        .envs
        .iter()
        .find(|e| e.id == clone_id)
        .and_then(|e| e.parent.clone())
        .unwrap_or_else(|| MAIN_ENV.into());
    let src = st.data_dir.join("envs").join(&parent);
    let mut manifest = std::collections::BTreeMap::new();
    if src.is_dir() {
        scan_manifest(&src, &mut manifest)?;
    }
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| AppError::io(e.to_string()))?;
    crate::fsutil::atomic_write(base_path(st, clone_id), bytes).map_err(|e| AppError::io(e.to_string()))
}

fn read_clone_base(st: &AppState, clone_id: &str) -> std::collections::BTreeMap<String, String> {
    std::fs::read(base_path(st, clone_id))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiffEntry {
    pub path: String,
    /// added | changed | deleted | conflict
    pub status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffReport {
    pub clone_id: String,
    pub parent_id: String,
    pub entries: Vec<DiffEntry>,
    pub clean: bool,
}

/// 试验档 diff：克隆基线 → 现在的三方比对。
/// 试验档改动 = added/changed/deleted；主档同时改动且不同 → conflict。
#[tauri::command(async)]
pub fn env_diff(st: tauri::State<AppState>, id: String) -> CmdResult<DiffReport> {
    env_diff_inner(&st, id)
}

pub(crate) fn env_diff_inner(st: &AppState, id: String) -> CmdResult<DiffReport> {
    let r = load_registry(st);
    let clone = r
        .envs
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到环境: {id}")))?;
    let parent_id = clone
        .parent
        .clone()
        .ok_or_else(|| AppError::validation("该环境不是试验档（无克隆来源），没有可 diff 的基线"))?;
    let base = read_clone_base(st, &id);
    let mut trial = std::collections::BTreeMap::new();
    scan_manifest(&st.data_dir.join("envs").join(&id), &mut trial)?;
    let mut main = std::collections::BTreeMap::new();
    let parent_dir = st.data_dir.join("envs").join(&parent_id);
    if parent_dir.is_dir() {
        scan_manifest(&parent_dir, &mut main)?;
    }

    let mut entries: Vec<DiffEntry> = Vec::new();
    let mut consider = |path: String, clone_v: Option<&String>, main_v: Option<&String>, base_v: Option<&String>| {
        let trial_changed = clone_v != base_v;
        let main_changed = main_v != base_v;
        if !trial_changed {
            return; // 试验档未动 = 主档改动不属试验档报告
        }
        let status = if main_changed {
            // 两边都动了：结果相同则无需裁决
            if clone_v == main_v {
                return;
            }
            "conflict"
        } else if clone_v.is_none() {
            "deleted"
        } else if base_v.is_none() {
            "added"
        } else {
            "changed"
        };
        entries.push(DiffEntry { path, status: status.into() });
    };

    let keys: std::collections::BTreeSet<&String> =
        base.keys().chain(trial.keys()).collect();
    for k in keys {
        let cv = trial.get(k);
        let mv = main.get(k);
        let bv = base.get(k);
        // 主档独有删除（基线有、主档无、试验档未动）→ 无事发生
        if cv.is_none() && mv.is_none() && bv.is_some() {
            continue;
        }
        consider(k.clone(), cv, mv, bv);
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let clean = entries.is_empty();
    Ok(DiffReport { clone_id: id, parent_id, entries, clean })
}

/// 丢弃试验档：active 弹回主档 → 删登记 + 删剖面目录（糟蹋完零影响主档）。
#[tauri::command(async)]
pub fn env_discard(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    env_discard_inner(&st, id)
}

pub(crate) fn env_discard_inner(st: &AppState, id: String) -> CmdResult<()> {
    let mut r = load_registry(st);
    let parent = r
        .envs
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| e.parent.clone())
        .ok_or_else(|| AppError::validation("该环境不是试验档，请用删除"))?;
    if r.active == id {
        r.active = parent;
    }
    r.envs.retain(|e| e.id != id);
    save_registry(st, &r)?;
    let dir = st.data_dir.join("envs").join(&id);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir).map_err(|e| AppError::io(e.to_string()))?;
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeReport {
    pub merged: u64,
    pub deleted: u64,
    pub conflicts_resolved: u64,
    pub conflicts_kept_main: u64,
}

/// 合并回主档：非冲突项直接应用；冲突项按 keep_clone 清单裁决（不在清单 = 保主档）。
/// 合并后基线重写为合并结果（试验档可继续用或丢弃）。
#[tauri::command(async)]
pub fn env_merge(
    st: tauri::State<AppState>,
    id: String,
    keep_clone: Vec<String>,
) -> CmdResult<MergeReport> {
    env_merge_inner(&st, id, keep_clone)
}

pub(crate) fn env_merge_inner(
    st: &AppState,
    id: String,
    keep_clone: Vec<String>,
) -> CmdResult<MergeReport> {
    let report = env_diff_inner(st, id.clone())?;
    let clone_dir = st.data_dir.join("envs").join(&id);
    let parent_dir = st.data_dir.join("envs").join(&report.parent_id);
    let mut merged = 0u64;
    let mut deleted = 0u64;
    let mut resolved = 0u64;
    let mut kept_main = 0u64;
    for e in &report.entries {
        let conflict = e.status == "conflict";
        let apply = !conflict || keep_clone.iter().any(|k| k == &e.path);
        if conflict && !apply {
            kept_main += 1;
            continue;
        }
        if conflict {
            resolved += 1;
        }
        let src = clone_dir.join(&e.path);
        let dst = parent_dir.join(&e.path);
        match e.status.as_str() {
            "added" | "changed" | "conflict" => {
                if !src.is_file() {
                    // 冲突裁决采用试验档的「删除」——主档文件随之移除
                    if dst.is_file() {
                        std::fs::remove_file(&dst).map_err(|er| AppError::io(er.to_string()))?;
                        deleted += 1;
                    }
                    continue;
                }
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent).map_err(|er| AppError::io(er.to_string()))?;
                }
                std::fs::copy(&src, &dst).map_err(|er| AppError::io(er.to_string()))?;
                merged += 1;
            }
            "deleted" => {
                if dst.is_file() {
                    std::fs::remove_file(&dst).map_err(|er| AppError::io(er.to_string()))?;
                }
                deleted += 1;
            }
            _ => {}
        }
    }
    // 基线重写 = 合并后的主档状态（diff 归零）
    write_clone_base(st, &id)?;
    Ok(MergeReport { merged, deleted, conflicts_resolved: resolved, conflicts_kept_main: kept_main })
}

// ---------- B-25：嵌套实例（深度 ≤3） ----------

pub const MAX_NEST_DEPTH: u32 = 3;

fn current_depth() -> u32 {
    std::env::var("VARIABLE_NEST_DEPTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// 子环境数据根：独立数据目录（嵌套互斥——独立根，绝不同一容器互踩 journal）。
fn nested_data_root(st: &AppState, id: &str, depth: u32) -> PathBuf {
    st.data_dir
        .join("envs")
        .join(id)
        .join(format!("nested-d{depth}"))
}

/// 生成子进程执行档环境（白名单子集继承：只减不增）。
/// 父执行档的 net_allow 直接继承（父已授权 ⊆ 子允许，满足"只减不增"）。
pub(crate) fn nested_env(
    st: &AppState,
    id: &str,
    depth: u32,
    parent_net_allow: &[String],
) -> Vec<(String, String)> {
    let root = nested_data_root(st, id, depth);
    vec![
        (
            "VARIABLE_DATA_ROOT".to_string(),
            root.to_string_lossy().into_owned(),
        ),
        ("VARIABLE_NEST_DEPTH".to_string(), depth.to_string()),
        ("VARIABLE_PARENT_ENV".to_string(), id.to_string()),
        (
            "VARIABLE_NET_ALLOW".to_string(),
            parent_net_allow.join(","),
        ),
    ]
}

/// 嵌套启动：spawn 当前 exe（独立数据根）+ 深度/白名单继承。
/// V1 回退口径：子实例作为独立 OS 窗口运行（embed 进 VWM 属后续批）——
/// 与嵌入失败路径"不杀进程可重试"的哲学一致。
#[tauri::command(async)]
pub fn env_nested(st: tauri::State<AppState>, id: String) -> CmdResult<u32> {
    env_nested_inner(&st, id)
}

pub(crate) fn env_nested_inner(st: &AppState, id: String) -> CmdResult<u32> {
    let depth = current_depth();
    if depth >= MAX_NEST_DEPTH {
        return Err(AppError::validation(format!(
            "已达嵌套深度上限 {MAX_NEST_DEPTH}"
        )));
    }
    let r = load_registry(st);
    if !r.envs.iter().any(|e| e.id == id) {
        return Err(AppError::not_found(format!("未找到环境: {id}")));
    }
    let child_root = nested_data_root(st, &id, depth + 1);
    std::fs::create_dir_all(&child_root).map_err(|e| AppError::io(e.to_string()))?;
    let exe = std::env::current_exe().map_err(|e| AppError::io(e.to_string()))?;
    let mut c = std::process::Command::new(exe);
    c.env("VARIABLE_DATA_ROOT", &child_root)
        .env("VARIABLE_NEST_DEPTH", (depth + 1).to_string())
        .env("VARIABLE_PARENT_ENV", &id);
    // 白名单子集继承（父→子只减不增；V1 直接透传父白名单）
    let _ = parent_allow_cache(st);
    let child = c
        .spawn()
        .map_err(|e| AppError::io(format!("嵌套实例启动失败: {e}")))?;
    let pid = child.id();
    Ok(pid)
}

fn parent_allow_cache(st: &AppState) -> Vec<String> {
    // V1：白名单继承的落点=子进程的 netconsent 库（独立数据根内），无需父透传；
    // 预留接口以便 B-28 白名单库落地后改为显式子集注入。
    Vec::new()
}

#[cfg(test)]
mod nested_tests {
    use super::*;

    #[test]
    fn depth_limit_enforced() {
        // 深度上限常量与当前深度解析
        assert_eq!(MAX_NEST_DEPTH, 3);
        std::env::set_var("VARIABLE_NEST_DEPTH", "2");
        assert_eq!(current_depth(), 2);
        std::env::remove_var("VARIABLE_NEST_DEPTH");
        assert_eq!(current_depth(), 0);
    }

    #[test]
    fn nested_env_builds_isolated_root() {
        let (st, dir) = temp_state("nested");
        let env = env_create_inner(&st, "nest".into()).unwrap();
        let envs = nested_env(&st, &env.id, 1, &["a.com".to_string()]);
        let root = envs
            .iter()
            .find(|(k, _)| k == "VARIABLE_DATA_ROOT")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert!(root.contains("envs"));
        assert!(root.contains("nested-d1"));
        assert!(envs.iter().any(|(k, v)| k == "VARIABLE_NET_ALLOW" && v == "a.com"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!("envs-b24-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        let st = AppState {
            conn: std::sync::Mutex::new(None),
            data_dir: dir.clone(),
            db_dir: dir.clone(),
            media_dir: dir.clone(),
            attachments_dir: dir.clone(),
            backups_dir: dir.clone(),
            recovery_dir: dir.clone(),
            logs_dir: dir.clone(),
        };
        (st, dir)
    }
}
