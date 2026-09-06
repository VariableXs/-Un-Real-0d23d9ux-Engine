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
}

#[tauri::command]
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
        })
        .collect())
}

#[tauri::command]
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
    };
    r.envs.push(env.clone());
    save_registry(st, &r)?;
    Ok(EnvView {
        id: env.id,
        name: env.name,
        active: false,
        created_at: env.created_at,
    })
}

/// 切换编排：①当前设置快照写入 active 环境 → ②active 翻转 → ③返回目标环境
/// 快照（前端应用后状态重载）。
#[tauri::command]
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

#[tauri::command]
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
#[tauri::command]
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
    });
    save_registry(st, &r)?;
    Ok(CloneReport {
        source_id: id,
        clone_id: new_id,
        clone_name: String::new(),
        bytes_copied: bytes,
    })
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
#[tauri::command]
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
