//! AI-15 V-82 环境变量编辑器：用户级（HKCU\Environment）GUI 后端。
//!
//! 红线（承化境计划）：
//! - 仅用户级编辑；系统级（HKLM）如实只读展示并标注「需要管理员且风险高，本轮不做」；
//! - 改动前自动备份（保留最近 10 份），一键回滚到任意备份点；
//! - 改动生效范围如实提示：新进程生效（已运行进程不回读注册表）；
//! - 不做变量 diff 视图（V-89 通用 diff 引擎可覆盖）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    pub name: String,
    pub value: String,
    /// REG_EXPAND_SZ = true（未展开原样存储）
    pub expand: bool,
}

/// winreg 0.52：RegValue → EnvVar（仅字符串类）。
#[cfg(windows)]
fn decode_reg_var(name: String, rv: &winreg::RegValue) -> Option<EnvVar> {
    use winreg::enums::{REG_EXPAND_SZ, REG_SZ};
    use winreg::types::FromRegValue as _;
    match rv.vtype {
        REG_SZ | REG_EXPAND_SZ => {
            let value = String::from_reg_value(rv).ok()?;
            Some(EnvVar { name, value, expand: rv.vtype == REG_EXPAND_SZ })
        }
        _ => None,
    }
}

/// winreg 0.52：字符串 → RegValue（UTF-16LE + NUL 终止）。
#[cfg(windows)]
fn to_reg_value(s: &str, expand: bool) -> winreg::RegValue {
    use winreg::enums::{REG_EXPAND_SZ, REG_SZ};
    let mut bytes: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    bytes.extend_from_slice(&[0, 0]);
    winreg::RegValue { bytes, vtype: if expand { REG_EXPAND_SZ } else { REG_SZ } }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 纯函数：把 HKCU\Environment 的值表转排序变量表。
#[cfg(windows)]
fn read_hkcu_environment() -> CmdResult<Vec<EnvVar>> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey("Environment")
        .map_err(|e| AppError::io(format!("打开 HKCU\\Environment 失败: {e}")))?;
    let mut out = Vec::new();
    for (name, value) in key.enum_values().map_while(|r| r.ok()) {
        if let Some(ev) = decode_reg_var(name, &value) {
            out.push(ev);
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

#[cfg(not(windows))]
fn read_hkcu_environment() -> CmdResult<Vec<EnvVar>> {
    Ok(vec![])
}

/// 纯函数：把 HKLM 系统环境只读转排序变量表（失败 = 空表 + 前端提示）。
#[cfg(windows)]
fn read_hklm_environment() -> CmdResult<Vec<EnvVar>> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(key) = hklm.open_subkey_with_flags("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment", KEY_READ) else {
        return Ok(vec![]);
    };
    let mut out = Vec::new();
    for (name, value) in key.enum_values().map_while(|r| r.ok()) {
        if let Some(ev) = decode_reg_var(name, &value) {
            out.push(ev);
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

#[cfg(not(windows))]
fn read_hklm_environment() -> CmdResult<Vec<EnvVar>> {
    Ok(vec![])
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvOverview {
    pub user: Vec<EnvVar>,
    /// 系统级：只读展示
    pub system: Vec<EnvVar>,
}

#[tauri::command]
pub fn env_overview() -> CmdResult<EnvOverview> {
    Ok(EnvOverview { user: read_hkcu_environment()?, system: read_hklm_environment()? })
}

// ---------- 备份（data_dir/env-backups/*.json，保留 10 份） ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EnvBackup {
    pub id: String,
    pub created_at: u64,
    pub vars: Vec<EnvVar>,
}

fn backups_dir(st: &AppState) -> PathBuf {
    st.data_dir.join("env-backups")
}

pub fn backup_take(st: &AppState) -> CmdResult<EnvBackup> {
    let vars = read_hkcu_environment()?;
    let b = EnvBackup { id: format!("eb-{}", now_ms()), created_at: now_ms(), vars };
    let dir = backups_dir(st);
    std::fs::create_dir_all(&dir).map_err(|e| AppError::io(e.to_string()))?;
    let json = serde_json::to_string_pretty(&b).map_err(|e| AppError::io(e.to_string()))?;
    crate::fsutil::atomic_write(dir.join(format!("{}.json", b.id)), json).map_err(|e| AppError::io(e.to_string()))?;
    // 只保留最近 10 份
    let mut all = backup_list_inner(st)?;
    while all.len() > 10 {
        let old = all.remove(0);
        let _ = std::fs::remove_file(dir.join(format!("{}.json", old.id)));
    }
    Ok(b)
}

fn backup_list_inner(st: &AppState) -> CmdResult<Vec<EnvBackup>> {
    let mut out: Vec<EnvBackup> = Vec::new();
    let dir = backups_dir(st);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if let Ok(s) = std::fs::read_to_string(e.path()) {
                if let Ok(b) = serde_json::from_str(&s) {
                    out.push(b);
                }
            }
        }
    }
    out.sort_by_key(|b| b.created_at);
    Ok(out)
}

#[tauri::command]
pub fn env_backup_list(st: tauri::State<AppState>) -> CmdResult<Vec<EnvBackup>> {
    backup_list_inner(&st)
}

/// 写一个用户变量（改前自动备份）。PATH 值由前端分行编辑后用 ';' join 传入。
#[tauri::command]
pub fn env_var_set(st: tauri::State<AppState>, name: String, value: String, expand: bool) -> CmdResult<()> {
    if name.trim().is_empty() || name.contains('=') || name.contains('\0') {
        return Err(AppError::validation("变量名非法（空 / 含 = 或 NUL）"));
    }
    #[cfg(windows)]
    {
        let _ = backup_take(&st); // 改动前自动备份
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags("Environment", KEY_SET_VALUE)
            .map_err(|e| AppError::io(format!("打开 HKCU\\Environment 失败（权限不足？）: {e}")))?;
        let rv = to_reg_value(&value, expand);
        key.set_raw_value(&name, &rv)
            .map_err(|e| AppError::io(format!("写入变量失败: {e}")))?;
        broadcast_env_change();
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (st, name, value, expand);
        Err(AppError::validation("仅支持 Windows"))
    }
}

/// 删除一个用户变量（改前自动备份）。
#[tauri::command]
pub fn env_var_delete(st: tauri::State<AppState>, name: String) -> CmdResult<()> {
    if name.eq_ignore_ascii_case("PATH") {
        return Err(AppError::validation("PATH 不允许删除（清空请用变量编辑）"));
    }
    #[cfg(windows)]
    {
        let _ = backup_take(&st);
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags("Environment", KEY_SET_VALUE)
            .map_err(|e| AppError::io(format!("打开 HKCU\\Environment 失败: {e}")))?;
        key.delete_value(&name)
            .map_err(|e| AppError::io(format!("删除变量失败（不存在？）: {e}")))?;
        broadcast_env_change();
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (st, name);
        Err(AppError::validation("仅支持 Windows"))
    }
}

/// 回滚到指定备份（当前值先自动备份，可再回滚回来）。
#[tauri::command]
pub fn env_restore_backup(st: tauri::State<AppState>, backup_id: String) -> CmdResult<usize> {
    let all = backup_list_inner(&st)?;
    let target = all
        .into_iter()
        .find(|b| b.id == backup_id)
        .ok_or_else(|| AppError::validation("备份不存在"))?;
    let _ = backup_take(&st); // 当前态再备份一份（反悔可再回）
    #[cfg(windows)]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags("Environment", KEY_SET_VALUE)
            .map_err(|e| AppError::io(format!("打开 HKCU\\Environment 失败: {e}")))?;
        // 先读当前名集合，清掉备份中没有的变量
        let cur = read_hkcu_environment()?;
        for v in cur {
            if !target.vars.iter().any(|t| t.name == v.name) {
                let _ = key.delete_value(&v.name);
            }
        }
        let mut n = 0usize;
        for v in &target.vars {
            let rv = to_reg_value(&v.value, v.expand);
            key.set_raw_value(&v.name, &rv)
                .map_err(|e| AppError::io(format!("写入 {} 失败: {e}", v.name)))?;
            n += 1;
        }
        broadcast_env_change();
        Ok(n)
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err(AppError::validation("仅支持 Windows"))
    }
}

/// WM_SETTINGCHANGE 广播（Explorer / 新进程感知环境变化；失败不阻断——新登录后必然生效）。
#[cfg(windows)]
fn broadcast_env_change() {
    #[allow(clippy::single_match)]
    unsafe {
        use windows::Win32::Foundation::LPARAM;
        use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
        let env: Vec<u16> = "Environment\0".encode_utf16().collect();
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            windows::Win32::Foundation::WPARAM(0),
            LPARAM(env.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            1000,
            None,
        );
    }
}

#[cfg(not(windows))]
fn broadcast_env_change() {}

// ---------- 纯函数：PATH 行拆分/合并（前端共用语义的 Rust 镜像，供测试对齐） ----------

/// PATH 值 → 行（空段剔除，保留原顺序）。
pub fn split_path(value: &str) -> Vec<String> {
    value.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// 行 → PATH 值（去重保序 + ';' join）。
pub fn join_path_rows(rows: &[String]) -> String {
    let mut seen = std::collections::HashSet::new();
    rows.iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .filter(|r| seen.insert(r.to_lowercase()))
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_split_join_roundtrip() {
        let rows = split_path("C:\\A;C:\\B;;C:\\A\\");
        assert_eq!(rows, vec!["C:\\A", "C:\\B", "C:\\A\\"]);
        let joined = join_path_rows(&rows);
        assert_eq!(joined, "C:\\A;C:\\B;C:\\A\\");
    }

    #[test]
    fn path_join_dedup_case_insensitive() {
        let joined = join_path_rows(&["C:\\A".into(), "c:\\a".into(), "C:\\B".into()]);
        assert_eq!(joined, "C:\\A;C:\\B");
    }
}
