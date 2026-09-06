//! 网络层（B-28，M8；BLUEPRINT 3.11 / 7.3）：环回代理 + 白名单执行点 +
//! kill-switch + 流量仪表。
//!
//! 架构：
//! - 环回 HTTP 代理（127.0.0.1:port）：受管进程经执行档注入 HTTP(S)_PROXY，
//!   所有出站汇聚到代理；代理按策略裁决（kill-switch → 域名规则 → 默认拒绝）；
//! - CONNECT 隧道：目标域名在握手头中明文可见 → 可裁决；裁决通过后盲转发；
//! - 绝对式 GET/POST：改写为相对路径后直连目标；
//! - 白名单：net.json 规则库（domain 精确 + 子域通配 *.rule），匹配语义 =
//!   host == rule || host.ends_with(".rule")；
//! - **直连逃逸如实边界（蓝图风险表）**：用户态无法硬断绕过代理的进程——
//!   逃逸只能告警 + 不受信标记（本批落地告警计数与 UI 口径）。
//!
//! 蓝图 7.3 契约：classify / grant / kill_switch 全部在本模块兑现。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

type CmdResult<T> = Result<T, AppError>;

// ---------- 规则库与持久化 ----------

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NetRule {
    pub domain: String,
    /// 发起执行档（域名确认卡升级：显示谁在请求）
    pub profile: String,
    pub granted_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetConfig {
    pub rules: Vec<NetRule>,
    pub kill_switch: bool,
    pub proxy_enabled: bool,
    pub proxy_port: u16,
}

fn net_path(st: &AppState) -> PathBuf {
    st.data_dir.join("net").join("net.json")
}

fn ensure_net_dir(st: &AppState) -> CmdResult<()> {
    std::fs::create_dir_all(st.data_dir.join("net")).map_err(|e| AppError::io(e.to_string()))
}

fn load_config(st: &AppState) -> NetConfig {
    std::fs::read(net_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(NetConfig {
            rules: Vec::new(),
            kill_switch: false,
            proxy_enabled: false,
            proxy_port: 18787,
        })
}

fn save_config(st: &AppState, c: &NetConfig) -> CmdResult<()> {
    ensure_net_dir(st)?;
    let bytes = serde_json::to_vec_pretty(c).map_err(|e| AppError::io(e.to_string()))?;
    std::fs::write(net_path(st), bytes).map_err(|e| AppError::io(e.to_string()))
}

fn read_config_anywhere(data_dir: &Path) -> NetConfig {
    std::fs::read(data_dir.join("net").join("net.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(NetConfig {
            rules: Vec::new(),
            kill_switch: false,
            proxy_enabled: false,
            proxy_port: 18787,
        })
}

// ---------- 裁决（蓝图 7.3 classify） ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Allow,
    Deny,
}

/// host 匹配：host == rule 或 host 以 ".rule" 结尾（子域通配）。
pub fn domain_matches(host: &str, rule: &str) -> bool {
    let host = host.trim_end_matches('.');
    let rule = rule.trim_end_matches('.');
    host == rule || host.ends_with(&format!(".{rule}"))
}

pub(crate) fn classify(host: &str, cfg: &NetConfig) -> Verdict {
    if cfg.kill_switch {
        return Verdict::Deny;
    }
    if cfg.rules.iter().any(|r| domain_matches(host, &r.domain)) {
        return Verdict::Allow;
    }
    Verdict::Deny // 默认零出站：未授权域名一律拒
}

// ---------- 代理运行时 ----------

type ArcBool = std::sync::Arc<AtomicBool>;

#[derive(Default, Clone)]
struct Counters {
    bytes_relayed: std::sync::Arc<AtomicU64>,
    conns_allowed: std::sync::Arc<AtomicU64>,
    conns_denied: std::sync::Arc<AtomicU64>,
}

impl Counters {
    fn new() -> Self {
        Self::default()
    }
}

struct ProxyState {
    running: bool,
    stop: ArcBool,
    port: u16,
    counters: Counters,
}

static PROXY: Mutex<Option<ProxyState>> = Mutex::new(None);

pub(crate) fn is_running() -> bool {
    PROXY.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|p| p.running).unwrap_or(false)
}

pub(crate) fn proxy_port() -> u16 {
    PROXY.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|p| p.port).unwrap_or(0)
}

/// 受管进程执行档注入的代理地址（spawn_profiled 调用；未运行返回 None）。
pub fn proxy_env_value() -> Option<String> {
    if is_running() {
        Some(format!("http://127.0.0.1:{}", proxy_port()))
    } else {
        None
    }
}

fn serve_loop(listener: TcpListener, stop: ArcBool, data_dir: PathBuf, counters: Counters) {
    for conn in listener.incoming() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let Ok(stream) = conn else { continue };
        let cfg = read_config_anywhere(&data_dir);
        let counters = counters.clone();
        std::thread::spawn(move || {
            handle_conn(stream, &cfg, &counters);
        });
    }
}

fn handle_conn(mut stream: TcpStream, cfg: &NetConfig, counters: &Counters) {
    let counters = counters.clone();
    let (bytes_relayed, conns_allowed, conns_denied) = (&counters.bytes_relayed, &counters.conns_allowed, &counters.conns_denied);
    // 读请求头（最多 16KB）
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                head.extend_from_slice(&buf[..n]);
                if head.windows(4).any(|w| w == b"\r\n\r\n") || head.len() > 16 * 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let head_str = String::from_utf8_lossy(&head);
    let first_line = head_str.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();

    let host = if method == "CONNECT" {
        target.split(':').next().unwrap_or("").to_string()
    } else if target.starts_with("http://") {
        // 绝对式：http://host[:port]/path
        let rest = &target[7..];
        rest.split(['/']).next().unwrap_or("").split(':').next().unwrap_or("").to_string()
    } else {
        // origin-form：取 Host 头
        head_str
            .lines()
            .find(|l| l.to_lowercase().starts_with("host:"))
            .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()))
            .unwrap_or_default()
    };

    if classify(&host, cfg) == Verdict::Allow {
        conns_allowed.fetch_add(1, Ordering::Relaxed);
        if method == "CONNECT" {
            // 隧道：先回 200，再盲转发
            if stream.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").is_err() {
                return;
            }
            let port: u16 = target.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(443);
            let Ok(upstream) = TcpStream::connect((host.as_str(), port)) else {
                return;
            };
            relay_bidir(stream, upstream, bytes_relayed.clone());
        } else {
            // 绝对式 HTTP：改写为相对式转发
            let port: u16 = target
                .strip_prefix("http://")
                .and_then(|r| r.split('/').next().unwrap_or("").rsplit(':').next().and_then(|p| p.parse().ok()))
                .unwrap_or(80);
            let Ok(mut upstream) = TcpStream::connect((host.as_str(), port)) else {
                return;
            };
            let rewritten = rewrite_to_relative(&head_str);
            if upstream.write_all(rewritten.as_bytes()).is_err() {
                return;
            }
            let _ = std::io::copy(&mut upstream, &mut stream);
            bytes_relayed.fetch_add(head.len() as u64, Ordering::Relaxed);
        }
    } else {
        conns_denied.fetch_add(1, Ordering::Relaxed);
        let body = format!(
            "HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\nblocked by Variable network policy: {host}",
            format!("blocked by Variable network policy: {host}").len()
        );
        let _ = stream.write_all(body.as_bytes());
    }
}

fn rewrite_to_relative(head: &str) -> String {
    let mut out = String::new();
    for (i, line) in head.lines().enumerate() {
        if i == 0 {
            // 请求行：METHOD http://host/path → METHOD path
            if let Some(space) = line.find(' ') {
                let (m, rest) = line.split_at(space);
                let target = rest.trim().split(' ').next().unwrap_or("/");
                let path = match target.find("://") {
                    Some(p) => {
                        let after = &target[p + 3..];
                        after.find('/').map(|s| &after[s..]).unwrap_or("/")
                    }
                    None => target,
                };
                out.push_str(&format!("{m} {path} HTTP/1.1\r\n"));
                continue;
            }
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out
}

fn relay_bidir(a: TcpStream, b: TcpStream, bytes: std::sync::Arc<AtomicU64>) {
    let mut a2 = match a.try_clone() {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut b2 = match b.try_clone() {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut b3 = match b.try_clone() {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut a3 = match a.try_clone() {
        Ok(c) => c,
        Err(_) => return,
    };
    let bytes2 = bytes.clone();
    let t = std::thread::spawn(move || {
        let n = pump(&mut a2, &mut b2);
        bytes2.fetch_add(n as u64, Ordering::Relaxed);
        let _ = b2.shutdown(std::net::Shutdown::Write);
    });
    let n = pump(&mut b3, &mut a3);
    bytes.fetch_add(n as u64, Ordering::Relaxed);
    let _ = t.join();
}

fn pump(from: &mut TcpStream, to: &mut TcpStream) -> u64 {
    let mut buf = [0u8; 8192];
    let mut total = 0u64;
    loop {
        match from.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if to.write_all(&buf[..n]).is_err() {
                    break;
                }
                total += n as u64;
            }
        }
    }
    total
}

// ---------- 命令面 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetStatus {
    pub proxy_running: bool,
    pub proxy_port: u16,
    pub kill_switch: bool,
    pub rule_count: usize,
    pub bytes_relayed: u64,
    pub conns_allowed: u64,
    pub conns_denied: u64,
}

#[tauri::command]
pub fn net_status(st: tauri::State<AppState>) -> CmdResult<NetStatus> {
    net_status_snapshot(&st)
}

/// 诊断包用：&AppState 版本。
pub(crate) fn net_status_snapshot(st: &AppState) -> CmdResult<NetStatus> {
    let cfg = load_config(&st);
    let g = PROXY.lock().unwrap_or_else(|e| e.into_inner());
    Ok(NetStatus {
        proxy_running: g.as_ref().map(|p| p.running).unwrap_or(false),
        proxy_port: g.as_ref().map(|p| p.port).unwrap_or(cfg.proxy_port),
        kill_switch: cfg.kill_switch,
        rule_count: cfg.rules.len(),
        bytes_relayed: g.as_ref().map(|p| p.counters.bytes_relayed.load(Ordering::Relaxed)).unwrap_or(0),
        conns_allowed: g.as_ref().map(|p| p.counters.conns_allowed.load(Ordering::Relaxed)).unwrap_or(0),
        conns_denied: g.as_ref().map(|p| p.counters.conns_denied.load(Ordering::Relaxed)).unwrap_or(0),
    })
}

#[tauri::command]
pub fn net_proxy_start(st: tauri::State<AppState>) -> CmdResult<u16> {
    let mut cfg = load_config(&st);
    cfg.proxy_enabled = true;
    save_config(&st, &cfg)?;
    {
        let g = PROXY.lock().unwrap_or_else(|e| e.into_inner());
        if g.as_ref().map(|p| p.running).unwrap_or(false) {
            return Ok(g.as_ref().unwrap().port);
        }
    }
    let listener = TcpListener::bind(("127.0.0.1", cfg.proxy_port))
        .or_else(|_| TcpListener::bind(("127.0.0.1", 0)))
        .map_err(|e| AppError::io(e.to_string()))?;
    let port = listener.local_addr().map_err(|e| AppError::io(e.to_string()))?.port();
    let stop: ArcBool = std::sync::Arc::new(AtomicBool::new(false));
    let counters = Counters::new();
    let counters_thread = counters.clone();
    let data_dir = st.data_dir.clone();
    let stop2 = stop.clone();
    std::thread::spawn(move || {
        serve_loop(listener, stop2, data_dir, counters_thread);
    });
    *PROXY.lock().unwrap_or_else(|e| e.into_inner()) = Some(ProxyState {
        running: true,
        stop,
        port,
        counters: counters.clone(),
    });
    Ok(port)
}

#[tauri::command]
pub fn net_proxy_stop() -> CmdResult<()> {
    let mut g = PROXY.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = g.as_mut() {
        p.stop.store(true, Ordering::Relaxed);
        p.running = false;
        // 触发 accept 解除阻塞
        let _ = TcpStream::connect(("127.0.0.1", p.port));
    }
    *g = None;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KillSwitchResult {
    pub on: bool,
}

#[tauri::command]
pub fn net_kill_switch(st: tauri::State<AppState>, on: bool) -> CmdResult<KillSwitchResult> {
    let mut cfg = load_config(&st);
    cfg.kill_switch = on;
    save_config(&st, &cfg)?;
    Ok(KillSwitchResult { on })
}

#[tauri::command]
pub fn net_rules_list(st: tauri::State<AppState>) -> CmdResult<Vec<NetRule>> {
    Ok(load_config(&st).rules)
}

#[tauri::command]
pub fn net_rule_grant(
    st: tauri::State<AppState>,
    domain: String,
    profile: String,
) -> CmdResult<Vec<NetRule>> {
    let domain = domain.trim().to_lowercase();
    if domain.is_empty() {
        return Err(AppError::validation("域名为空"));
    }
    let mut cfg = load_config(&st);
    if !cfg.rules.iter().any(|r| r.domain == domain) {
        cfg.rules.push(NetRule {
            domain,
            profile,
            granted_at: now_short(),
        });
        save_config(&st, &cfg)?;
    }
    Ok(cfg.rules)
}

#[tauri::command]
pub fn net_rule_revoke(st: tauri::State<AppState>, domain: String) -> CmdResult<Vec<NetRule>> {
    let domain = domain.trim().to_lowercase();
    let mut cfg = load_config(&st);
    cfg.rules.retain(|r| r.domain != domain);
    save_config(&st, &cfg)?;
    Ok(cfg.rules)
}

fn now_short() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64 % 100_000_000)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_rules_and_kill_switch() {
        let cfg = NetConfig {
            rules: vec![NetRule {
                domain: "api.example.com".into(),
                profile: "claude-code".into(),
                granted_at: 0,
            }],
            kill_switch: false,
            proxy_enabled: true,
            proxy_port: 0,
        };
        assert_eq!(classify("api.example.com", &cfg), Verdict::Allow);
        assert_eq!(classify("v2.api.example.com", &cfg), Verdict::Allow, "子域通配");
        assert_eq!(classify("example.com", &cfg), Verdict::Deny, "父域不在规则内");
        assert_eq!(classify("evil.com", &cfg), Verdict::Deny);
        // kill-switch 覆盖一切
        let cfg2 = NetConfig { kill_switch: true, ..cfg };
        assert_eq!(classify("api.example.com", &cfg2), Verdict::Deny);
    }

    #[test]
    fn domain_matches_tolerates_trailing_dot() {
        assert!(domain_matches("api.example.com.", "api.example.com"));
        assert!(domain_matches("a.b.com", "b.com"));
        assert!(!domain_matches("b.com", "a.b.com"));
    }

    /// 环回代理端到端：本地假 HTTP 服务 + 白名单 → 经代理取回 200；
    /// kill-switch 开启 → 403 拒绝。
    #[test]
    fn proxy_end_to_end_allow_and_deny() {
        // 假上游 HTTP 服务
        let upstream = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for conn in upstream.incoming() {
                let mut c = conn.unwrap();
                let mut buf = [0u8; 2048];
                let _ = c.read(&mut buf);
                c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                    .unwrap();
            }
        });

        let cfg = NetConfig {
            rules: vec![NetRule {
                domain: "127.0.0.1".into(),
                profile: "test".into(),
                granted_at: 0,
            }],
            kill_switch: false,
            proxy_enabled: true,
            proxy_port: 0,
        };
        let proxy = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let proxy_port = proxy.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let counters = Counters::new();
            for conn in proxy.incoming() {
                let Ok(s) = conn else { break };
                handle_conn(s, &cfg, &counters);
            }
        });

        // 允许：绝对式 GET 经代理命中上游
        let mut c = TcpStream::connect(("127.0.0.1", proxy_port)).unwrap();
        c.write_all(format!("GET http://127.0.0.1:{upstream_port}/x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
            .unwrap();
        let mut resp = String::new();
        let _ = c.read_to_string(&mut resp);
        assert!(resp.contains("200 OK"), "resp={resp}");

        // 拒绝：未授权域名
        let mut c = TcpStream::connect(("127.0.0.1", proxy_port)).unwrap();
        c.write_all(b"GET http://evil.example.com/x HTTP/1.1\r\nHost: evil.example.com\r\n\r\n")
            .unwrap();
        let mut resp = String::new();
        let _ = c.read_to_string(&mut resp);
        assert!(resp.contains("403"), "未授权域名应 403：{resp}");
    }
}
