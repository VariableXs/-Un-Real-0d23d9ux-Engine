//! L3 shell — 联网确认机制（批次0，规格 12.2.2）：
//! - Variable 默认零联网。任何将要发起网络请求的功能，必须先经前端 netGuard
//!   弹窗获得用户明确同意（拒绝 / 仅此一次 / 始终允许）。
//! - 本模块只做"授权策略存储"：<dataDir>/net_consent.json 记录每个主机的
//!   always-allow / always-deny 决定；本身零网络代码，不发起任何请求。
//! - "仅此一次"不落盘（会话内语义，由前端自行处理）。
//! - 策略文件仅存本机、随 U 盘便携；删库即彻底销毁授权记录。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

pub const POLICY_ALLOW: &str = "allow";
pub const POLICY_DENY: &str = "deny";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HostPolicy {
    pub host: String,
    /// allow | deny
    pub policy: String,
    pub updated_at: u64,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct ConsentStore {
    #[serde(default)]
    hosts: Vec<HostPolicy>,
}

fn store_path(st: &AppState) -> PathBuf {
    st.data_dir.join("net_consent.json")
}

fn load_store(st: &AppState) -> ConsentStore {
    let Ok(bytes) = fs::read(store_path(st)) else {
        return ConsentStore::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_store(st: &AppState, store: &ConsentStore) -> CmdResult<()> {
    let bytes = serde_json::to_vec_pretty(store)
        .map_err(|e| AppError::io(format!("序列化联网授权失败 / Serialize consent failed: {e}")))?;
    fs::write(store_path(st), bytes)
        .map_err(|e| AppError::io(format!("写入联网授权失败 / Write consent failed: {e}")))?;
    Ok(())
}

/// 归一化主机名：去协议/路径/端口/用户信息，统一小写。
/// IPv6 按方括号内的地址处理。
pub fn normalize_host(host: &str) -> String {
    let h = host.trim();
    let h = h.split("://").last().unwrap_or(h);
    let h = h.split(['/', '?', '#']).next().unwrap_or(h);
    let h = h.rsplit('@').next().unwrap_or(h);
    let h = if let Some(rest) = h.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest)
    } else {
        h.split(':').next().unwrap_or(h)
    };
    h.trim().to_lowercase()
}

fn valid_policy(p: &str) -> bool {
    matches!(p, POLICY_ALLOW | POLICY_DENY)
}

/// 查询某主机的持久化策略：None = 从未决定（前端需弹窗询问）。
pub fn check_inner(st: &AppState, host: &str) -> CmdResult<Option<String>> {
    let host = normalize_host(host);
    if host.is_empty() {
        return Err(AppError::validation(
            "主机名不能为空 / Host cannot be empty",
        ));
    }
    Ok(load_store(st)
        .hosts
        .into_iter()
        .find(|h| h.host == host)
        .map(|h| h.policy))
}

/// 记录"始终允许 / 始终拒绝"。同主机重复设置 → 覆盖旧策略。
pub fn set_inner(st: &AppState, host: &str, policy: &str) -> CmdResult<()> {
    if !valid_policy(policy) {
        return Err(AppError::validation(format!(
            "无效策略 / Invalid policy: {policy}"
        )));
    }
    let host = normalize_host(host);
    if host.is_empty() {
        return Err(AppError::validation(
            "主机名不能为空 / Host cannot be empty",
        ));
    }
    let mut store = load_store(st);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if let Some(slot) = store.hosts.iter_mut().find(|h| h.host == host) {
        slot.policy = policy.to_string();
        slot.updated_at = now;
    } else {
        store.hosts.push(HostPolicy {
            host,
            policy: policy.to_string(),
            updated_at: now,
        });
    }
    save_store(st, &store)
}

#[tauri::command]
pub fn net_consent_check(st: tauri::State<AppState>, host: String) -> CmdResult<Option<String>> {
    check_inner(&st, &host)
}

#[tauri::command]
pub fn net_consent_set(st: tauri::State<AppState>, host: String, policy: String) -> CmdResult<()> {
    set_inner(&st, &host, &policy)
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(tag: &str) -> (AppState, std::path::PathBuf) {
        let tmp = std::env::temp_dir().join(format!("variable-netconsent-{tag}-{}", std::process::id()));
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        (st, tmp)
    }

    #[test]
    fn host_normalization() {
        assert_eq!(normalize_host("https://Example.com:8443/a/b?q=1"), "example.com");
        assert_eq!(normalize_host("http://user@HOST.example.com/path#x"), "host.example.com");
        assert_eq!(normalize_host("example.com"), "example.com");
        assert_eq!(normalize_host("[::1]:8080/x"), "::1");
        assert_eq!(normalize_host("  EXAMPLE.com  "), "example.com");
    }

    #[test]
    fn consent_roundtrip_and_overwrite() {
        let (st, tmp) = temp_state("roundtrip");
        // 从未决定 → None
        assert_eq!(check_inner(&st, "example.com").unwrap(), None);
        // 带协议/端口/路径的目标 → 归一化后命中
        set_inner(&st, "https://Example.com:8443/wallpapers", POLICY_ALLOW).unwrap();
        assert_eq!(
            check_inner(&st, "example.com").unwrap().as_deref(),
            Some(POLICY_ALLOW)
        );
        // 覆盖为始终拒绝
        set_inner(&st, "example.com", POLICY_DENY).unwrap();
        assert_eq!(
            check_inner(&st, "EXAMPLE.com").unwrap().as_deref(),
            Some(POLICY_DENY)
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn consent_rejects_invalid_policy_and_host() {
        let (st, tmp) = temp_state("invalid");
        assert!(set_inner(&st, "example.com", "maybe").is_err());
        assert!(set_inner(&st, "   ", POLICY_ALLOW).is_err());
        assert!(check_inner(&st, "").is_err());
        let _ = fs::remove_dir_all(&tmp);
    }
}
