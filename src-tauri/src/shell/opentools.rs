//! AI-15 开放工具组（开放生态域）：M-57 本地出站桥 / M-59 第三方嵌入声明协议 /
//! M-63 资源包安全扫描 / V-89 配置对比引擎 / V-90 沙盒试用支撑。
//!
//! 红线（承原计划）：
//! - M-57 默认关闭、URL 仅允许环回/内网段（公网需二次确认，由前端确认层承担）；
//!   未确认时 0 出站；
//! - M-63 离线静态扫描（无云查杀），白名单外文件类型一律拦截；
//! - V-89 只 diff 不合并（人来决定）；V-90 试用会话与主环境隔离。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

// ===========================================================================
// M-57 本地出站桥（Local Webhook Bridge，opt-in）
// ===========================================================================

/// 出站规则：事件类型 → 本地 URL。默认 enabled=false（零出站红线）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct WebhookRule {
    pub id: String,
    /// 事件类型（复用 O-52 事件流白名单子集，见 KNOWN_EVENTS）
    pub event: String,
    /// 目标 URL（仅 http://，且仅环回/内网段）
    pub url: String,
    pub enabled: bool,
}

impl Default for WebhookRule {
    fn default() -> Self {
        Self { id: String::new(), event: String::new(), url: String::new(), enabled: false }
    }
}

/// O-52 事件流白名单子集（出站桥只允许这些事件源）。
pub const KNOWN_EVENTS: [&str; 8] = [
    "boot.ready",
    "window.embedded",
    "theme.changed",
    "wallpaper.changed",
    "schedule.fired",
    "pack.installed",
    "clipboard.pin",
    "shutdown.clean",
];

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct WebhookConfig {
    /// 总开关（默认关 = 0 出站）
    pub enabled: bool,
    pub rules: Vec<WebhookRule>,
}

fn webhook_config_path(st: &AppState) -> PathBuf {
    st.data_dir.join("webhook-bridge.json")
}

fn webhook_load(st: &AppState) -> WebhookConfig {
    std::fs::read_to_string(webhook_config_path(st))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn webhook_save(st: &AppState, cfg: &WebhookConfig) -> CmdResult<()> {
    std::fs::create_dir_all(&st.data_dir).map_err(|e| AppError::io(e.to_string()))?;
    let json = serde_json::to_string_pretty(cfg).map_err(|e| AppError::io(e.to_string()))?;
    std::fs::write(webhook_config_path(st), json).map_err(|e| AppError::io(e.to_string()))?;
    Ok(())
}

/// 纯函数：URL 出站策略校验。
/// - 必须 http://（https 明确拒绝：本地桥面向 Home Assistant/n8n 等本地管道）；
/// - host 仅允许环回（127.x / localhost / [::1]）或内网段（10.x / 172.16-31.x / 192.168.x）；
/// - 其他（公网域名 / IP）= false（需用户在前端二次确认后走 netconsent 域名白名单，本层不放行）。
pub fn webhook_url_allowed(url: &str) -> bool {
    let rest = match url.strip_prefix("http://") {
        Some(r) => r,
        None => return false,
    };
    let host_port = rest.split('/').next().unwrap_or("");
    let host = host_port.rsplit_once(':').map(|(h, _)| h).unwrap_or(host_port);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") || host == "::1" {
        return true;
    }
    if let Some(ip) = parse_ipv4(host) {
        let [a, b, _, _] = ip;
        return a == 127 || a == 10 || (a == 192 && b == 168) || (a == 172 && (16..=31).contains(&b));
    }
    false
}

/// 纯 IPv4 解析（不引网络栈，四段数字即可）。
pub fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut out = [0u8; 4];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p.parse::<u8>().ok()?;
    }
    Some(out)
}

#[tauri::command]
pub fn webhook_rules_get(st: tauri::State<AppState>) -> CmdResult<WebhookConfig> {
    Ok(webhook_load(&st))
}

#[tauri::command]
pub fn webhook_rules_set(st: tauri::State<AppState>, config: WebhookConfig) -> CmdResult<WebhookConfig> {
    let mut cfg = config;
    // 红线：未知事件 / 非 http / 公网 URL 一律拒绝保存
    for r in cfg.rules.iter_mut() {
        if !KNOWN_EVENTS.contains(&r.event.as_str()) {
            return Err(AppError::validation(format!("未知事件类型: {}（白名单见文档）", r.event)));
        }
        if !r.url.starts_with("http://") {
            return Err(AppError::validation("URL 必须以 http:// 开头（本地出站桥不面向公网）"));
        }
        if !webhook_url_allowed(&r.url) {
            return Err(AppError::validation(format!("URL 主机不在环回/内网段: {}（公网地址请走网关域名白名单）", r.url)));
        }
        if r.id.trim().is_empty() {
            r.id = format!("wh-{}", crate::shell::opentools::now_suffix());
        }
    }
    webhook_save(&st, &cfg)?;
    Ok(cfg)
}

/// 出站日志行（JSONL，data_dir/webhook-log.jsonl）。
#[derive(Serialize)]
struct WebhookLogLine {
    ts: u64,
    event: String,
    url: String,
    ok: bool,
    status: Option<u16>,
    detail: String,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_suffix() -> String {
    let ms = now_ms();
    format!("{ms:x}")
}

/// 事件分发：匹配 enabled 规则 → POST（超时 3s，失败重试 1 次）。
/// 前端在事件发生处调用（如 workshop 触发、主题切换）。
#[tauri::command]
pub fn webhook_dispatch(st: tauri::State<AppState>, event: String, payload: String) -> CmdResult<u32> {
    let cfg = webhook_load(&st);
    if !cfg.enabled || !KNOWN_EVENTS.contains(&event.as_str()) {
        return Ok(0);
    }
    let rules: Vec<WebhookRule> = cfg
        .rules
        .iter()
        .filter(|r| r.enabled && r.event == event && webhook_url_allowed(&r.url))
        .cloned()
        .collect();
    let mut sent = 0u32;
    for r in rules {
        let body = serde_json::json!({ "event": r.event, "ts": now_ms(), "payload": payload });
        let (ok, status, detail) = post_with_retry(&r.url, &body.to_string());
        let line = WebhookLogLine {
            ts: now_ms(),
            event: r.event.clone(),
            url: r.url.clone(),
            ok,
            status,
            detail: detail.unwrap_or_default(),
        };
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(st.data_dir.join("webhook-log.jsonl"));
        if let Ok(f) = f.as_mut() {
            use std::io::Write as _;
            let _ = writeln!(f, "{}", serde_json::to_string(&line).unwrap_or_default());
        }
        if ok {
            sent += 1;
        }
    }
    Ok(sent)
}

/// 最小 HTTP POST（环回/内网专用；无 TLS —— https 在入口已拒绝）。
fn http_post(url: &str, body: &str) -> CmdResult<u16> {
    let rest = url.strip_prefix("http://").ok_or_else(|| AppError::validation("bad url"))?;
    let host_port = rest.split('/').next().unwrap_or("");
    let (host, port) = host_port.rsplit_once(':').unwrap_or((host_port, "80"));
    let addr = format!("{host}:{port}");
    let mut stream = std::net::TcpStream::connect(&addr)
        .map_err(|e| AppError::io(format!("connect {addr}: {e}")))?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(3))).ok();
    use std::io::Write as _;
    let req = format!(
        "POST / HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).map_err(|e| AppError::io(e.to_string()))?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp).map_err(|e| AppError::io(e.to_string()))?;
    let status = resp
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    if (200..300).contains(&status) {
        Ok(status)
    } else {
        Err(AppError::io(format!("HTTP {status}")))
    }
}

fn post_with_retry(url: &str, body: &str) -> (bool, Option<u16>, Option<String>) {
    match http_post(url, body) {
        Ok(s) => (true, Some(s), None),
        Err(e1) => {
            // 失败重试 1 次
            match http_post(url, body) {
                Ok(s) => (true, Some(s), None),
                Err(e2) => (false, None, Some(format!("重试后仍失败: {e1}; {e2}"))),
            }
        }
    }
}

/// 测试发送（前端「测试」按钮）：不要求规则已启用，但仍受 URL 策略约束。
#[tauri::command]
pub fn webhook_test(st: tauri::State<AppState>, url: String) -> CmdResult<bool> {
    if !webhook_url_allowed(&url) {
        return Err(AppError::validation("URL 主机不在环回/内网段"));
    }
    let body = r#"{"event":"webhook.test","ts":0,"payload":"variable local bridge self-test"}"#;
    let (ok, status, detail) = post_with_retry(&url, body);
    let _ = (st, status, detail);
    Ok(ok)
}

/// 最近 n 条出站日志（时间轴展示）。
#[tauri::command]
pub fn webhook_log_list(st: tauri::State<AppState>, limit: Option<usize>) -> CmdResult<Vec<Value>> {
    let raw = std::fs::read_to_string(st.data_dir.join("webhook-log.jsonl")).unwrap_or_default();
    let cap = limit.unwrap_or(50).min(200);
    let mut out: Vec<Value> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    out.reverse(); // 新 → 旧
    out.truncate(cap);
    Ok(out)
}

// ===========================================================================
// M-59 第三方嵌入声明协议（variable-embed.json）
// ===========================================================================

/// 嵌入声明（应用侧主动声明；优先级低于用户登记）。
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct EmbedManifest {
    /// 声明格式版本（当前 1）
    pub version: u32,
    /// 窗口标题匹配（正则；空 = 不匹配）
    pub title_match: String,
    /// 最小窗口尺寸（嵌入容器不小于此值；0 = 不限）
    pub min_width: u32,
    pub min_height: u32,
    /// 多实例策略："single" | "multi"
    pub multi_instance: String,
    /// 检测等待毫秒（0..60000）
    pub wait_ms: u32,
}

/// 纯校验：非法声明返回 Err（调用方忽略该文件并记日志）。
pub fn embed_manifest_validate(m: &EmbedManifest) -> Result<(), String> {
    if m.version != 1 {
        return Err(format!("version 必须为 1，实际 {}", m.version));
    }
    if m.title_match.len() > 256 {
        return Err("titleMatch 超长（>256）".into());
    }
    // 正则可编译性（防 ReDoS 只限长度）
    if !m.title_match.is_empty() {
        regex_lite_check(&m.title_match)?;
    }
    if m.min_width > 16384 || m.min_height > 16384 {
        return Err("minWidth/minHeight 超过 16384".into());
    }
    if !(m.multi_instance.is_empty() || m.multi_instance == "single" || m.multi_instance == "multi") {
        return Err(format!("multiInstance 必须是 single/multi，实际 {}", m.multi_instance));
    }
    if m.wait_ms > 60_000 {
        return Err(format!("waitMs 必须在 0..60000，实际 {}", m.wait_ms));
    }
    Ok(())
}

/// 轻量正则检查：用 regex crate 编译（依赖已在引擎内）。
fn regex_lite_check(re: &str) -> Result<(), String> {
    match regex::Regex::new(re) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("titleMatch 不是合法正则: {e}")),
    }
}

/// 扫描 exe 同目录的 variable-embed.json（应用作者放置）。
#[tauri::command]
pub fn embed_manifest_scan(exe_path: String) -> CmdResult<Option<EmbedManifest>> {
    let dir = Path::new(&exe_path)
        .parent()
        .ok_or_else(|| AppError::validation("无父目录"))?;
    let f = dir.join("variable-embed.json");
    if !f.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&f).map_err(|e| AppError::io(e.to_string()))?;
    let m: EmbedManifest = serde_json::from_str(&raw)
        .map_err(|e| AppError::validation(format!("variable-embed.json 解析失败: {e}")))?;
    embed_manifest_validate(&m).map_err(AppError::validation)?;
    Ok(Some(m))
}

// ===========================================================================
// M-63 资源包安全扫描（.vxs Pack Safety Scan）
// ===========================================================================

/// 扫描报告（导入确认模态展示）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VxsScanReport {
    pub path: String,
    pub total_files: usize,
    pub total_bytes: u64,
    /// 类型分布（扩展名 → 个数）
    pub type_dist: Vec<(String, usize)>,
    /// 拦截项（每条 = 拒绝导入的理由）
    pub blocked: Vec<String>,
    /// 可疑但放行（如接近阈值的大文件）
    pub warnings: Vec<String>,
    pub safe: bool,
}

/// 白名单扩展名（图片/CSS/JSON/字体/音频）。
pub const VXS_EXT_WHITELIST: [&str; 16] = [
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "svg", "css", "json", "woff", "woff2", "ttf",
    "otf", "mp3", "wav", "ogg",
];

/// 可执行/脚本扩展名（一律拦截）。
const DANGEROUS_EXT: [&str; 13] = [
    "exe", "dll", "bat", "cmd", "ps1", "sh", "msi", "scr", "vbs", "js", "jar", "com", "pyd",
];

/// 单文件大小上限（伪图攻击拦截：500MB）。
const MAX_FILE_BYTES: u64 = 500 * 1024 * 1024;
/// 包总大小上限（2GB）。
const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// 图片类单文件阈值（超大图告警：64MB）。
const BIG_IMAGE_WARN: u64 = 64 * 1024 * 1024;

/// 纯扫描：不落盘、不联网。zip 容器逐条目检查。
pub fn vxs_scan(path: &Path) -> CmdResult<VxsScanReport> {
    let f = std::fs::File::open(path).map_err(|e| AppError::io(e.to_string()))?;
    let file_size = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut zip = zip::ZipArchive::new(f).map_err(|e| AppError::io(format!("zip: {e}")))?;
    let mut blocked = Vec::new();
    let mut warnings = Vec::new();
    let mut total_bytes = 0u64;
    let mut dist: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut total_files = 0usize;
    for i in 0..zip.len() {
        let zf = zip.by_index(i).map_err(|e| AppError::io(e.to_string()))?;
        let name = zf.name().to_string();
        // zip slip：绝对路径 / .. 穿越 / 盘符
        if name.starts_with('/') || name.starts_with('\\') || name.contains("..") {
            blocked.push(format!("路径穿越条目: {name}"));
            continue;
        }
        if name.len() >= 2 && name.as_bytes()[1] == b':' {
            blocked.push(format!("绝对盘符路径: {name}"));
            continue;
        }
        let size = zf.size();
        let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
        if DANGEROUS_EXT.contains(&ext.as_str()) {
            blocked.push(format!("可执行/脚本文件被拦截: {name}"));
            continue;
        }
        if !name.ends_with('/') && !VXS_EXT_WHITELIST.contains(&ext.as_str()) {
            if name == "manifest.json" {
                // manifest 本体放行
            } else {
                blocked.push(format!("非白名单类型 .{ext}: {name}"));
                continue;
            }
        }
        if size > MAX_FILE_BYTES {
            blocked.push(format!("单文件超 500MB（{} bytes）: {name}", size));
            continue;
        }
        if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp") && size > BIG_IMAGE_WARN {
            warnings.push(format!("超大图片（{} bytes）: {name}", size));
        }
        total_bytes += size;
        *dist.entry(if ext.is_empty() { "(无扩展名)".into() } else { ext }).or_insert(0) += 1;
        total_files += 1;
    }
    if total_bytes > MAX_TOTAL_BYTES {
        blocked.push(format!("包总大小超 2GB（{} bytes）", total_bytes));
    }
    if file_size > MAX_TOTAL_BYTES {
        blocked.push(format!("包文件本体超 2GB（{} bytes）", file_size));
    }
    let safe = blocked.is_empty();
    Ok(VxsScanReport {
        path: path.to_string_lossy().into_owned(),
        total_files,
        total_bytes,
        type_dist: dist.into_iter().collect(),
        blocked,
        warnings,
        safe,
    })
}

#[tauri::command]
pub fn vxs_scan_cmd(path: String) -> CmdResult<VxsScanReport> {
    vxs_scan(Path::new(&path))
}

// ===========================================================================
// V-89 配置对比引擎（语义化 diff；V-84 关联快照共用）
// ===========================================================================

/// 语义 diff 条目：path = 点分路径（数组按 idKey 对齐时带 [id]）。
#[derive(Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiffEntry {
    pub path: String,
    /// "add"（b 有 a 无）/ "del"（a 有 b 无）/ "mod"（值不同）
    pub kind: String,
    pub a: Value,
    pub b: Value,
}

/// 语义 diff：对象按 key、数组优先按 idKey 对齐（无 idKey 的数组按下标）。
/// 纯函数（V-84 关联快照 diff 与 V-89 配置 diff 共用）。
pub fn semantic_diff(a: &Value, b: &Value, path: &str, out: &mut Vec<DiffEntry>) {
    match (a, b) {
        (Value::Object(ma), Value::Object(mb)) => {
            for (k, va) in ma {
                match mb.get(k) {
                    Some(vb) => semantic_diff(va, vb, &join_path(path, k), out),
                    None => out.push(DiffEntry {
                        path: join_path(path, k),
                        kind: "del".into(),
                        a: va.clone(),
                        b: Value::Null,
                    }),
                }
            }
            for k in mb.keys() {
                if !ma.contains_key(k) {
                    out.push(DiffEntry {
                        path: join_path(path, k),
                        kind: "add".into(),
                        a: Value::Null,
                        b: mb[k].clone(),
                    });
                }
            }
        }
        (Value::Array(va), Value::Array(vb)) => {
            // 尝试 idKey 对齐（对象数组的常见主键：id / name / ext）
            if let Some(idk) = array_id_key(va).or_else(|| array_id_key(vb)) {
                let key_of = |v: &Value| -> Option<String> {
                    v.as_object().and_then(|o| o.get(&idk)).map(|x| {
                        x.as_str().map(|s| s.to_string()).unwrap_or_else(|| x.to_string())
                    })
                };
                let mut vb_used = vec![false; vb.len()];
                for va_item in va {
                    let ka = key_of(va_item);
                    let ka_display = ka.clone().unwrap_or_else(|| "?".into());
                    let mut matched = false;
                    if let Some(ka) = ka {
                        for (j, vb_item) in vb.iter().enumerate() {
                            if !vb_used[j] && key_of(vb_item).as_deref() == Some(ka.as_str()) {
                                vb_used[j] = true;
                                matched = true;
                                semantic_diff(
                                    va_item,
                                    vb_item,
                                    &format!("{path}[{ka}]"),
                                    out,
                                );
                                break;
                            }
                        }
                    }
                    if !matched {
                        out.push(DiffEntry {
                            path: format!("{path}[{ka_display}]"),
                            kind: "del".into(),
                            a: va_item.clone(),
                            b: Value::Null,
                        });
                    }
                }
                for (j, vb_item) in vb.iter().enumerate() {
                    if !vb_used[j] {
                        let kb = key_of(vb_item).unwrap_or_else(|| format!("#{j}"));
                        out.push(DiffEntry {
                            path: format!("{path}[{kb}]"),
                            kind: "add".into(),
                            a: Value::Null,
                            b: vb_item.clone(),
                        });
                    }
                }
            } else {
                let n = va.len().max(vb.len());
                for i in 0..n {
                    let p = format!("{path}[{i}]");
                    match (va.get(i), vb.get(i)) {
                        (Some(x), Some(y)) => semantic_diff(x, y, &p, out),
                        (Some(x), None) => out.push(DiffEntry { path: p, kind: "del".into(), a: x.clone(), b: Value::Null }),
                        (None, Some(y)) => out.push(DiffEntry { path: p, kind: "add".into(), a: Value::Null, b: y.clone() }),
                        (None, None) => {}
                    }
                }
            }
        }
        (x, y) => {
            if x != y {
                out.push(DiffEntry { path: path.to_string(), kind: "mod".into(), a: x.clone(), b: y.clone() });
            }
        }
    }
}

fn join_path(base: &str, key: &str) -> String {
    if base.is_empty() { key.to_string() } else { format!("{base}.{key}") }
}

fn array_id_key(arr: &[Value]) -> Option<String> {
    for v in arr {
        if let Some(o) = v.as_object() {
            if o.contains_key("id") { return Some("id".into()); }
            if o.contains_key("name") { return Some("name".into()); }
            if o.contains_key("ext") { return Some("ext".into()); }
        }
    }
    None
}

#[tauri::command]
pub fn cfg_diff(a: String, b: String) -> CmdResult<Vec<DiffEntry>> {
    let va: Value = serde_json::from_str(&a).map_err(|e| AppError::validation(format!("A 不是合法 JSON: {e}")))?;
    let vb: Value = serde_json::from_str(&b).map_err(|e| AppError::validation(format!("B 不是合法 JSON: {e}")))?;
    let mut out = Vec::new();
    semantic_diff(&va, &vb, "", &mut out);
    Ok(out)
}

// ===========================================================================
// V-90 沙盒试用支撑（N-35 perf 实例之上的薄层）
// ===========================================================================

/// 试用会话登记（data_dir/sandbox-trials.json）：试用前快照 + 当前试用项。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SandboxTrial {
    pub kind: String,   // "theme" | "wallpaper"
    pub id: String,
    pub started_at: u64,
    /// 试用前的原值（还原用；空 = 原本未设置）
    pub prev_value: String,
}

impl Default for SandboxTrial {
    fn default() -> Self {
        Self { kind: String::new(), id: String::new(), started_at: 0, prev_value: String::new() }
    }
}

fn trials_path(st: &AppState) -> PathBuf {
    st.data_dir.join("sandbox-trials.json")
}

fn trials_load(st: &AppState) -> Vec<SandboxTrial> {
    std::fs::read_to_string(trials_path(st))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn trials_save(st: &AppState, list: &[SandboxTrial]) -> CmdResult<()> {
    let json = serde_json::to_string_pretty(list).map_err(|e| AppError::io(e.to_string()))?;
    std::fs::write(trials_path(st), json).map_err(|e| AppError::io(e.to_string()))?;
    Ok(())
}

/// 登记一次试用（kind 白名单：theme/wallpaper；只读类）。
#[tauri::command]
pub fn sandbox_trial_begin(st: tauri::State<AppState>, kind: String, id: String, prev_value: String) -> CmdResult<()> {
    if !matches!(kind.as_str(), "theme" | "wallpaper") {
        return Err(AppError::validation("试用白名单：theme / wallpaper（写操作类插件不支持）"));
    }
    let mut list = trials_load(&st);
    list.retain(|t| !(t.kind == kind && t.id == id));
    list.push(SandboxTrial { kind, id, started_at: now_ms(), prev_value });
    trials_save(&st, &list)?;
    Ok(())
}

/// 结束试用（应用回主环境时移除登记）。
#[tauri::command]
pub fn sandbox_trial_end(st: tauri::State<AppState>, kind: String, id: String) -> CmdResult<Vec<SandboxTrial>> {
    let mut list = trials_load(&st);
    list.retain(|t| !(t.kind == kind && t.id == id));
    trials_save(&st, &list)?;
    Ok(list)
}

/// 当前试用清单（前端展示「试用中」徽标）。
#[tauri::command]
pub fn sandbox_trial_list(st: tauri::State<AppState>) -> CmdResult<Vec<SandboxTrial>> {
    Ok(trials_load(&st))
}

// ===========================================================================
// CLI 支撑（V-88 教程 / M-58 插件开发 / M-60 测试钩子）
// ===========================================================================

/// V-88 CLI 交互式教程。返回退出码。章节词表 = 教程契约（与 docs/TESTHOOKS.md 对齐）。
pub fn cli_tour(topic: &str) -> i32 {
    const CHAPTERS: [(&str, &str, &str); 7] = [
        ("start", "5 分钟上手", "桌面 = 虚拟窗口管理器（VWM）拖拽/贴靠/多开；任务栏/开始菜单/回收站与 Win11 一致。试试：双击桌面图标、把窗口拖到屏幕边缘贴靠。"),
        ("windows", "窗口四件套", "贴靠（拖边）、置顶（标题栏菜单）、保活（防休眠标签）、多开（同一应用第二个窗口）。快捷键 Ctrl+Alt+O 打开编排中心。"),
        ("files", "文件与数据", "文件管理器 2.0：多标签、双栏、批量重命名、重复文件报告；删除一律进全局回收站可还原。数据安全中心：容器快照/校验/导出。"),
        ("tools", "实用工具集", "计算器/便签/日历时钟/截图/剪贴板历史/换算中心……全部是 VWM 工具窗口，与主窗口同语义。"),
        ("open", "开放生态", "U-37 协议中枢 variable:// 深链、.vxs 资源包（安全扫描后导入）、开放接口网关（默认关闭）、变量 CLI（--doctor）。"),
        ("power", "高级能力", "计划任务工坊、环境变量编辑器、文件关联快照、服务依赖图、配置对比、沙盒试用——全部本地、可撤销。"),
        ("safety", "安全红线", "默认零联网；任何出站需显式确认。CLI 应急通道：--export-rescue / --repair。备份与回滚无处不在。"),
    ];
    if topic == "list" || topic.is_empty() {
        println!("== Variable 交互式教程（--tour <章节>）==");
        for (i, (key, title, _)) in CHAPTERS.iter().enumerate() {
            println!("  {}. {key:<8} {title}", i + 1);
        }
        println!("提示: --tour start 从头开始；每章末尾有下一步建议。");
        return 0;
    }
    match CHAPTERS.iter().find(|(k, _, _)| *k == topic) {
        Some((key, title, body)) => {
            println!("== 教程 · {title}（{key}）==\n{body}");
            let idx = CHAPTERS.iter().position(|(k, _, _)| *k == *key).unwrap();
            if idx + 1 < CHAPTERS.len() {
                let (nk, nt, _) = CHAPTERS[idx + 1];
                println!("\n下一章: --tour {nk}（{nt}）");
            } else {
                println!("\n已到最后一章。回顾: --tour list");
            }
            0
        }
        None => {
            eprintln!("未知章节: {topic}（--tour list 查看全部）");
            2
        }
    }
}

/// M-58 插件开发（CLI 侧）：校验插件目录结构 + 打印热重载工作流指引。
/// 目录约定：manifest.json（嵌入声明协议字段）+ 入口（index.html 或 main.js）。
pub fn cli_plugin_dev(dir: &Path) -> CmdResult<String> {
    if !dir.is_dir() {
        return Err(AppError::validation(format!("目录不存在: {}", dir.display())));
    }
    let mut problems: Vec<String> = Vec::new();
    let manifest_path = dir.join("manifest.json");
    let manifest: Option<Value> = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let Some(m) = manifest else {
        problems.push("manifest.json 缺失或不是合法 JSON".into());
        return Err(AppError::validation(problems.join("; ")));
    };
    // 复用嵌入声明协议的核心字段校验
    let emb = serde_json::from_value::<EmbedManifest>(m.clone())
        .map_err(|e| AppError::validation(format!("manifest 字段不符合嵌入声明协议: {e}")))?;
    embed_manifest_validate(&emb)
        .map_err(|e| AppError::validation(format!("manifest 校验失败: {e}")))?;
    let has_entry = dir.join("index.html").is_file() || dir.join("main.js").is_file();
    if !has_entry {
        problems.push("入口缺失（index.html 或 main.js）".into());
    }
    if !problems.is_empty() {
        return Err(AppError::validation(problems.join("; ")));
    }
    Ok(format!(
        "✓ 插件校验通过: {}（titleMatch={}）\n热重载工作流:\n  1. 在应用内打开「插件开发」面板加载该目录\n  2. 修改文件后 Ctrl+R（插件面板内）触发定向重载，仅该插件 iframe 重建\n  3. 再次运行 --plugin-dev {} 做提交前校验",
        dir.display(),
        emb.title_match,
        dir.display()
    ))
}

/// M-60 测试钩子规范（CLI 侧）：扫描前端 data-testid 覆盖与规范文件存在性。
/// 返回 (是否达标, 报告文本)。
pub fn cli_test_ready(root: &Path) -> CmdResult<(bool, String)> {
    let src_dir = root.join("src");
    if !src_dir.is_dir() {
        return Err(AppError::validation(format!("未找到 src/（在 {} 下运行）", root.display())));
    }
    // 1) 规范文件存在性
    let spec_ok = root.join("docs").join("TESTHOOKS.md").is_file();
    // 2) data-testid 覆盖率（按文件数）
    let mut files_total = 0usize;
    let mut files_with_hooks = 0usize;
    let mut stack = vec![src_dir.clone()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Some(ext) = p.extension().and_then(|x| x.to_str()) {
                if matches!(ext, "tsx" | "ts") && !p.to_string_lossy().contains("__tests__") {
                    files_total += 1;
                    if let Ok(content) = std::fs::read_to_string(&p) {
                        if content.contains("data-testid") {
                            files_with_hooks += 1;
                        }
                    }
                }
            }
        }
    }
    let ratio = if files_total == 0 {
        0.0
    } else {
        files_with_hooks as f64 / files_total as f64 * 100.0
    };
    // 3) i18n 键位对齐（zh/en 行数差）
    let zh = std::fs::read_to_string(root.join("src").join("i18n").join("zh.ts"));
    let en = std::fs::read_to_string(root.join("src").join("i18n").join("en.ts"));
    let i18n_note = match (zh, en) {
        (Ok(z), Ok(e)) => {
            let zl = z.lines().filter(|l| l.contains(':')).count();
            let el = e.lines().filter(|l| l.contains(':')).count();
            if zl == el {
                format!("i18n zh/en 键位对齐 ✓（{zl}）")
            } else {
                format!("i18n zh/en 行数不一致: zh={zl} en={el}（可能有跨行值，请用 tools/audit 核对）")
            }
        }
        _ => "i18n 文件缺失".to_string(),
    };
    let ok = spec_ok && ratio >= 25.0;
    let report = format!(
        "== Test-Ready 报告（M-60）==\n规范文档: {}\ndata-testid 覆盖: {}/{}（{:.1}%）\n{}\n判定: {}",
        if spec_ok { "存在 ✓（docs/TESTHOOKS.md）" } else { "缺失 ✗" },
        files_with_hooks,
        files_total,
        ratio,
        i18n_note,
        if ok { "通过" } else { "未达标（需规范文档存在且覆盖率 ≥25%）" }
    );
    Ok((ok, report))
}

// ===========================================================================
// 测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- M-57 ----
    #[test]
    fn webhook_url_policy() {
        assert!(webhook_url_allowed("http://127.0.0.1:9000/hook"));
        assert!(webhook_url_allowed("http://localhost:9000/hook"));
        assert!(webhook_url_allowed("http://192.168.1.10:8123/"));
        assert!(webhook_url_allowed("http://10.0.0.5:1880/"));
        assert!(webhook_url_allowed("http://172.16.0.1/"));
        // 公网 / https / 域名 = 拒绝
        assert!(!webhook_url_allowed("https://127.0.0.1:9000/hook"));
        assert!(!webhook_url_allowed("http://8.8.8.8:9000/hook"));
        assert!(!webhook_url_allowed("http://example.com/hook"));
        assert!(!webhook_url_allowed("ftp://127.0.0.1/x"));
    }

    #[test]
    fn webhook_rule_validation() {
        let mut r = WebhookRule { id: "1".into(), event: "theme.changed".into(), url: "http://127.0.0.1:9000/h".into(), enabled: true };
        assert!(serde_json::to_string(&r).is_ok());
        r.event = "unknown.event".into();
        // 事件白名单由 webhook_rules_set 强制；这里校验 URL 层
        assert!(webhook_url_allowed(&r.url));
        r.url = "http://203.0.113.5/x".into();
        assert!(!webhook_url_allowed(&r.url));
    }

    // ---- M-59 ----
    #[test]
    fn embed_manifest_ok_and_bad() {
        let ok = EmbedManifest {
            version: 1,
            title_match: "^My App.*".into(),
            min_width: 640,
            min_height: 480,
            multi_instance: "single".into(),
            wait_ms: 3000,
        };
        assert!(embed_manifest_validate(&ok).is_ok());
        let bad_ver = EmbedManifest { version: 2, ..ok.clone() };
        assert!(embed_manifest_validate(&bad_ver).is_err());
        let bad_re = EmbedManifest { title_match: "(".into(), ..ok.clone() };
        assert!(embed_manifest_validate(&bad_re).is_err());
        let bad_wait = EmbedManifest { wait_ms: 61_000, ..ok.clone() };
        assert!(embed_manifest_validate(&bad_wait).is_err());
        let bad_mi = EmbedManifest { multi_instance: "always".into(), ..ok };
        assert!(embed_manifest_validate(&bad_mi).is_err());
    }

    // ---- M-63 ----
    fn make_zip(items: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            for (name, content) in items {
                w.start_file::<_, ()>(name.to_string(), zip::write::FileOptions::default()).unwrap();
                use std::io::Write as _;
                w.write_all(content.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("variable-vxs-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{name}-{}.vxs", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn vxs_scan_blocks_exe_and_traversal() {
        let zip = make_zip(&[
            ("theme/ok.json", "{}"),
            ("evil.exe", "MZ"),
            ("../escape.txt", "x"),
            ("img/pic.png", "png"),
        ]);
        let p = write_temp("bad", &zip);
        let r = vxs_scan(&p).unwrap();
        assert!(!r.safe);
        assert!(r.blocked.iter().any(|b| b.contains("evil.exe")));
        assert!(r.blocked.iter().any(|b| b.contains("escape")));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn vxs_scan_ok_pack() {
        let zip = make_zip(&[
            ("manifest.json", r#"{"id":"t1","format":"vxs"}"#),
            ("theme/vars.css", "body{}"),
            ("icons/a.png", "png"),
        ]);
        let p = write_temp("ok", &zip);
        let r = vxs_scan(&p).unwrap();
        assert!(r.safe, "blocked: {:?}", r.blocked);
        assert_eq!(r.total_files, 3);
        let _ = std::fs::remove_file(&p);
    }

    // ---- V-89 ----
    #[test]
    fn semantic_diff_by_id_not_index() {
        let a: Value = serde_json::from_str(
            r#"[{"id":"x","v":1},{"id":"y","v":2}]"#,
        )
        .unwrap();
        let b: Value = serde_json::from_str(
            r#"[{"id":"y","v":3},{"id":"x","v":1},{"id":"z","v":9}]"#,
        )
        .unwrap();
        let mut out = Vec::new();
        semantic_diff(&a, &b, "", &mut out);
        // y 的 v 修改 + z 新增；x 不得误报（按 id 对齐，路径落到具体字段）
        assert_eq!(out.len(), 2, "entries: {out:?}");
        assert!(out.iter().any(|d| d.path == "[y].v" && d.kind == "mod"));
        assert!(out.iter().any(|d| d.path == "[z]" && d.kind == "add"));
    }

    #[test]
    fn semantic_diff_nested_objects() {
        let a: Value = serde_json::from_str(r#"{"theme":"dark","ui":{"zoom":1}}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"theme":"light","ui":{"zoom":1.2},"new":true}"#).unwrap();
        let mut out = Vec::new();
        semantic_diff(&a, &b, "", &mut out);
        assert_eq!(out.len(), 3);
        assert!(out.iter().any(|d| d.path == "theme" && d.kind == "mod"));
        assert!(out.iter().any(|d| d.path == "ui.zoom" && d.kind == "mod"));
        assert!(out.iter().any(|d| d.path == "new" && d.kind == "add"));
    }

    // ---- V-90 ----
    #[test]
    fn sandbox_trial_kind_whitelist_shape() {
        // 结构完整性（命令层白名单在 sandbox_trial_begin 强制）
        let t = SandboxTrial { kind: "theme".into(), id: "deep-space".into(), started_at: 1, prev_value: "paper".into() };
        let s = serde_json::to_string(&t).unwrap();
        let back: SandboxTrial = serde_json::from_str(&s).unwrap();
        assert_eq!(back, t);
    }
}
