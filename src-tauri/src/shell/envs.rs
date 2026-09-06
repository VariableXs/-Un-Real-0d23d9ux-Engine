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

use std::path::PathBuf;

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
