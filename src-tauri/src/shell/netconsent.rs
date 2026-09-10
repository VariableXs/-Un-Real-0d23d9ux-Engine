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

#[tauri::command(async)]
pub fn net_consent_check(st: tauri::State<AppState>, host: String) -> CmdResult<Option<String>> {
    check_inner(&st, &host)
}

#[tauri::command(async)]
pub fn net_consent_set(st: tauri::State<AppState>, host: String, policy: String) -> CmdResult<()> {
    set_inner(&st, &host, &policy)
}

// ---------- AI-10（U-32 应用防火墙 2.0） ----------
// 每个嵌入应用的网络策略档案：blocked（默认，禁网）/ whitelist（域名白名单）/ full（放行）；
// 出站告警中心（被拦截尝试集中呈列，一键放行）；单应用日流量配额（超出自动降为禁网）。
// 与 M8 域名库互补：这里只做应用级档案与告警中心这一层。

const FW_LEVEL_BLOCKED: &str = "blocked";
const FW_LEVEL_WHITELIST: &str = "whitelist";
const FW_LEVEL_FULL: &str = "full";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FwProfile {
    pub app: String,
    /// blocked | whitelist | full（默认 blocked——零出站哲学）
    pub level: String,
    /// 域名白名单（level=whitelist 时生效）
    #[serde(default)]
    pub whitelist: Vec<String>,
    /// 单应用日流量上限（字节，0 = 不限）
    #[serde(default)]
    pub daily_quota_bytes: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FwAlert {
    pub id: String,
    pub app: String,
    pub host: String,
    pub ts: u64,
    /// 拦截时该应用当日累计尝试次数（高频标红 UI 用）
    pub attempts: u32,
    /// 是否已被用户放行（一键放行 → 告警消解）
    #[serde(default)]
    pub resolved: bool,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct FwStore {
    #[serde(default)]
    profiles: Vec<FwProfile>,
    #[serde(default)]
    alerts: Vec<FwAlert>,
    /// 当日流量：<app> → { day, bytes }
    #[serde(default)]
    traffic: std::collections::BTreeMap<String, FwDayUse>,
    /// 配额触发降级记录（app, day）——当日不再重复触发
    #[serde(default)]
    quota_hits: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct FwDayUse {
    day: String,
    bytes: u64,
}

fn fw_path(st: &AppState) -> PathBuf {
    st.data_dir.join("app_firewall.json")
}

fn fw_day() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let days = now / 86_400_000;
    format!("d{days}")
}

fn load_fw(st: &AppState) -> FwStore {
    fs::read(fw_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_fw(st: &AppState, s: &FwStore) -> CmdResult<()> {
    fs::write(fw_path(st), serde_json::to_vec_pretty(s)?)?;
    Ok(())
}

fn fw_profile_of<'a>(s: &'a FwStore, app: &str) -> FwProfile {
    s.profiles
        .iter()
        .find(|p| p.app == app)
        .cloned()
        .unwrap_or(FwProfile {
            app: app.to_string(),
            level: FW_LEVEL_BLOCKED.into(),
            whitelist: vec![],
            daily_quota_bytes: 0,
        })
}

/// 决策：某应用访问某域名是否放行（前端 netGuard 在请求前调用）。
/// 返回 (allowed, reason)。拦截会写入告警中心。
pub fn fw_check_inner(st: &AppState, app: &str, host: &str) -> CmdResult<(bool, String)> {
    let host = normalize_host(host);
    if host.is_empty() {
        return Err(AppError::validation("主机名不能为空 / host cannot be empty"));
    }
    let mut s = load_fw(st);
    let p = fw_profile_of(&s, app);
    // 配额熔断：当日配额已触发 → 无条件拦截（不可绕过）
    let today = fw_day();
    if s.quota_hits.iter().any(|(a, d)| a == app && *d == today) {
        return Ok((false, "quota_hit".into()));
    }
    let allowed = match p.level.as_str() {
        FW_LEVEL_FULL => true,
        FW_LEVEL_WHITELIST => p.whitelist.iter().any(|w| normalize_host(w) == host),
        _ => false, // blocked（默认）
    };
    if !allowed {
        // 写入告警中心
        let day_attempts = s.alerts.iter().filter(|a| a.app == app).count() as u32 + 1;
        s.alerts.push(FwAlert {
            id: crate::db::gen_id(),
            app: app.to_string(),
            host: host.clone(),
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            attempts: day_attempts,
            resolved: false,
        });
        if s.alerts.len() > 5_000 {
            s.alerts.drain(0..s.alerts.len() - 5_000);
        }
        save_fw(st, &s)?;
        return Ok((false, "blocked".into()));
    }
    Ok((true, String::new()))
}

/// 流量记账（前端在请求完成后上报实际字节数）。配额触发 → 自动降级禁网。
/// 返回 Some(quota_hit=true) 当本次记账触发了降级（前端据此发通知）。
pub fn fw_traffic_inner(st: &AppState, app: &str, bytes: u64) -> CmdResult<bool> {
    let mut s = load_fw(st);
    let p = fw_profile_of(&s, app);
    if p.daily_quota_bytes == 0 {
        return Ok(false);
    }
    let today = fw_day();
    let use_slot = s.traffic.entry(app.to_string()).or_default();
    if use_slot.day != today {
        *use_slot = FwDayUse { day: today.clone(), bytes: 0 };
    }
    use_slot.bytes += bytes;
    let hit = use_slot.bytes >= p.daily_quota_bytes;
    if hit && !s.quota_hits.iter().any(|(a, d)| a == app && *d == today) {
        s.quota_hits.push((app.to_string(), today));
        // 自动降级为禁网
        if let Some(p2) = s.profiles.iter_mut().find(|q| q.app == app) {
            p2.level = FW_LEVEL_BLOCKED.into();
        }
        save_fw(st, &s)?;
        return Ok(true);
    }
    save_fw(st, &s)?;
    Ok(false)
}

// ---------- 防火墙命令 ----------

#[tauri::command(async)]
pub fn fw_profile_get(st: tauri::State<AppState>, app: String) -> CmdResult<FwProfile> {
    Ok(fw_profile_of(&load_fw(&st), &app))
}

#[tauri::command(async)]
pub fn fw_profile_set(st: tauri::State<AppState>, profile: FwProfile) -> CmdResult<()> {
    if !matches!(profile.level.as_str(), FW_LEVEL_BLOCKED | FW_LEVEL_WHITELIST | FW_LEVEL_FULL) {
        return Err(AppError::validation(format!("无效级别 / invalid level: {}", profile.level)));
    }
    let mut s = load_fw(&st);
    let mut p = profile;
    p.whitelist = p
        .whitelist
        .into_iter()
        .map(|w| normalize_host(&w))
        .filter(|w| !w.is_empty())
        .collect();
    p.whitelist.dedup();
    p.whitelist.sort();
    if let Some(slot) = s.profiles.iter_mut().find(|q| q.app == p.app) {
        *slot = p;
    } else {
        s.profiles.push(p);
    }
    save_fw(&st, &s)
}

/// 决策检查（前端 netGuard 请求前调用）。
#[tauri::command(async)]
pub fn fw_check(st: tauri::State<AppState>, app: String, host: String) -> CmdResult<(bool, String)> {
    fw_check_inner(&st, &app, &host)
}

/// 流量记账（请求完成后调用）。返回是否触发配额降级。
#[tauri::command(async)]
pub fn fw_traffic(st: tauri::State<AppState>, app: String, bytes: u64) -> CmdResult<bool> {
    fw_traffic_inner(&st, &app, bytes)
}

/// 告警中心：未消解的拦截列表 + 高频标红（attempts ≥10）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FwAlertCenter {
    pub alerts: Vec<FwAlert>,
    /// 高频尝试（≥10 次）的应用集合
    pub hot_apps: Vec<String>,
}

#[tauri::command(async)]
pub fn fw_alerts(st: tauri::State<AppState>) -> CmdResult<FwAlertCenter> {
    let s = load_fw(&st);
    let alerts: Vec<FwAlert> = s.alerts.iter().filter(|a| !a.resolved).cloned().collect();
    let mut counts: std::collections::BTreeMap<String, u32> = Default::default();
    for a in &alerts {
        *counts.entry(a.app.clone()).or_insert(0) += 1;
    }
    Ok(FwAlertCenter {
        alerts,
        hot_apps: counts.into_iter().filter(|(_, n)| *n >= 10).map(|(a, _)| a).collect(),
    })
}

/// 一键放行：域名加入该应用白名单 + 告警消解（或保持拦截 = 仅清除告警）。
#[tauri::command(async)]
pub fn fw_alert_resolve(st: tauri::State<AppState>, alert_id: String, allow: bool) -> CmdResult<()> {
    let mut s = load_fw(&st);
    let alert = s
        .alerts
        .iter_mut()
        .find(|a| a.id == alert_id)
        .ok_or_else(|| AppError::not_found("告警不存在 / alert not found"))?;
    alert.resolved = true;
    let (app, host) = (alert.app.clone(), alert.host.clone());
    if allow {
        if let Some(p) = s.profiles.iter_mut().find(|q| q.app == app) {
            if !p.whitelist.contains(&host) {
                p.whitelist.push(host);
                p.whitelist.sort();
            }
            if p.level == FW_LEVEL_BLOCKED {
                p.level = FW_LEVEL_WHITELIST.into();
            }
        } else {
            s.profiles.push(FwProfile {
                app,
                level: FW_LEVEL_WHITELIST.into(),
                whitelist: vec![host],
                daily_quota_bytes: 0,
            });
        }
    }
    save_fw(&st, &s)
}

/// 应用档案总览（防火墙设置页）。
#[tauri::command(async)]
pub fn fw_profiles(st: tauri::State<AppState>) -> CmdResult<Vec<FwProfile>> {
    Ok(load_fw(&st).profiles)
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

    // ---------- U-32 应用防火墙 2.0 ----------

    #[test]
    fn fw_default_blocked_and_alert_center() {
        let (st, tmp) = temp_state("fw-default");
        // 默认禁网：未登记档案 → 拦截 + 告警
        let (ok, why) = fw_check_inner(&st, "app-embed", "cdn.example.com").unwrap();
        assert!(!ok);
        assert_eq!(why, "blocked");
        let s = load_fw(&st);
        assert_eq!(s.alerts.len(), 1);
        assert_eq!(s.alerts[0].app, "app-embed");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fw_whitelist_full_and_resolve() {
        let (st, tmp) = temp_state("fw-whitelist");
        // 白名单级：命中放行，未命中拦截
        {
            let mut s = load_fw(&st);
            s.profiles.push(FwProfile {
                app: "app-a".into(),
                level: FW_LEVEL_WHITELIST.into(),
                whitelist: vec!["api.good.com".into()],
                daily_quota_bytes: 0,
            });
            save_fw(&st, &s).unwrap();
        }
        let (ok, _) = fw_check_inner(&st, "app-a", "https://api.good.com/v1").unwrap();
        assert!(ok, "白名单域名应放行");
        let (ok, _) = fw_check_inner(&st, "app-a", "evil.com").unwrap();
        assert!(!ok, "非白名单域名应拦截");
        // full 级：全放行
        {
            let mut s = load_fw(&st);
            s.profiles[0].level = FW_LEVEL_FULL.into();
            save_fw(&st, &s).unwrap();
        }
        let (ok, _) = fw_check_inner(&st, "app-a", "anything.com").unwrap();
        assert!(ok);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fw_quota_triggers_downgrade_and_cannot_bypass() {
        let (st, tmp) = temp_state("fw-quota");
        {
            let mut s = load_fw(&st);
            s.profiles.push(FwProfile {
                app: "app-q".into(),
                level: FW_LEVEL_FULL.into(),
                whitelist: vec![],
                daily_quota_bytes: 1000,
            });
            save_fw(&st, &s).unwrap();
        }
        // 配额内：放行 + 记账不触发
        assert!(!fw_traffic_inner(&st, "app-q", 600).unwrap());
        let (ok, _) = fw_check_inner(&st, "app-q", "x.com").unwrap();
        assert!(ok);
        // 超配额：降级禁网
        assert!(fw_traffic_inner(&st, "app-q", 500).unwrap(), "累计 1100 ≥ 1000 应触发降级");
        // 降级后 full 也不放行（不可绕过）
        let (ok, why) = fw_check_inner(&st, "app-q", "x.com").unwrap();
        assert!(!ok, "配额熔断后不可绕过");
        assert_eq!(why, "quota_hit");
        let s = load_fw(&st);
        assert_eq!(s.profiles[0].level, FW_LEVEL_BLOCKED, "档案应自动降为禁网");
        let _ = fs::remove_dir_all(&tmp);
    }
}
