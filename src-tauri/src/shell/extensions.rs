//! L3 shell — extensions.rs（X-1…X-3 扩展生态系统 V1）
//! - `.uxpack.json` 目录即扩展：`<data>/extensions/<id>/` 含 `.uxpack.json` 即为扩展包
//! - Web 扩展宿主：每扩展一个隐藏 WebView2（label = `ext-<id>`），经 asset 协议加载 entry
//! - IPC 桥：`window.variable.*`（initialization_script 注入）→ 唯一命令 `ext_invoke`
//!   · token 绑定扩展 id：Rust 侧校验调用方 webview label == `ext-<extId>`，JS 伪造无效
//!   · 权限模型（X-2）：逐 API 精确校验 `.uxpack.json.permissions`
//! - 崩溃隔离：webview Destroyed → 标记「已停止」，主进程无感，设置页可重启
//! - 能力边界（V1 如实声明）：扩展 webview 与引擎同 IPC 通道，命令面 ACL 隔离属后续批；
//!   manifest `csp` 字段登记但 V1 未按窗生效；`signature` Ed25519 可选，未签名 = 黄色警示。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Emitter, Manager};

use crate::error::{AppError, CmdResult};

// ---------------------------------------------------------------- manifest（X-2 Schema v1）

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UxpackManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    /// web | plugin | external
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub entry: String,
    #[serde(default)]
    pub csp: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// X-2 权限枚举（精确粒度）。storage 支持作用域后缀。
pub const PERMISSIONS: [&str; 12] = [
    "widget",
    "window",
    "events:subscribe",
    "storage",
    "vault",
    "net",
    "notify",
    "layout:control",
    "hardware:read",
    "theme:patch",
    "aihub:invoke",
    "__wildcard_scope",
];

fn valid_permission(p: &str) -> bool {
    let root = p.split(':').next().unwrap_or("");
    matches!(
        root,
        "widget"
            | "window"
            | "events"
            | "storage"
            | "vault"
            | "net"
            | "notify"
            | "layout"
            | "hardware"
            | "theme"
            | "aihub"
    )
}

impl UxpackManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err("id 只允许小写字母/数字/连字符".into());
        }
        if !matches!(self.kind.as_str(), "web" | "plugin" | "external") {
            return Err(format!("未知扩展类型: {}", self.kind));
        }
        if self.entry.trim().is_empty() {
            return Err("entry 不能为空".into());
        }
        if self.version.trim().is_empty() {
            return Err("version 不能为空".into());
        }
        for p in &self.permissions {
            if !valid_permission(p) {
                return Err(format!("未知权限: {p}"));
            }
        }
        Ok(())
    }

    /// 精确权限判定：
    /// - `root` 只授予 `root` 本身（不自动扩散到 `root:*`）；
    /// - `root:action` 授予 `root:action`；
    /// - `root:action:scope` 授予 `root:action`（任意 scope）；
    /// - `root:*` 授予 `root` 下任意 action；`*none` 恒拒绝。
    pub fn has_permission(&self, need: &str) -> bool {
        let (nr, ns) = match need.split_once(':') {
            Some(x) => x,
            None => return self.permissions.iter().any(|p| p == need),
        };
        self.permissions.iter().any(|p| match p.split_once(':') {
            None => false,
            Some((pr, ps)) => {
                if pr != nr || ps == "*none" {
                    return false;
                }
                match ps.split_once(':') {
                    Some((act, _scope)) => act == ns,
                    None => ps == "*" || ps == ns,
                }
            }
        })
    }
}

// ---------------------------------------------------------------- registry（X-1）

#[derive(Debug, Clone)]
pub struct ExtEntry {
    pub manifest: UxpackManifest,
    pub dir: PathBuf,
    pub enabled: bool,
    pub crashed: bool,
    /// 已订阅事件（events:subscribe）
    pub subs: Vec<String>,
}

static EXTS: Mutex<Vec<ExtEntry>> = Mutex::new(Vec::new());

/// 桌面小组件登记（widgets.register）
static WIDGETS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
/// 布局快照（layout API V1：内存态）
static LAYOUTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
/// 审计日志（越权/调用留痕）
static AUDIT: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn ext_root(st: &crate::state::AppState) -> PathBuf {
    st.data_dir.join("extensions")
}

fn scan_one(dir: &std::path::Path) -> Option<ExtEntry> {
    let mf_path = dir.join(".uxpack.json");
    let raw = std::fs::read_to_string(&mf_path).ok()?;
    let manifest: UxpackManifest = serde_json::from_str(&raw).ok()?;
    manifest.validate().ok()?;
    Some(ExtEntry {
        manifest,
        dir: dir.to_path_buf(),
        enabled: true,
        crashed: false,
        subs: Vec::new(),
    })
}

fn with_ext<T>(id: &str, f: impl FnOnce(&mut ExtEntry) -> T) -> CmdResult<T> {
    let mut guard = EXTS.lock().map_err(|_| AppError::db("extensions mutex poisoned"))?;
    let e = guard
        .iter_mut()
        .find(|e| e.manifest.id == id)
        .ok_or_else(|| AppError::not_found(format!("扩展未加载: {id}")))?;
    Ok(f(e))
}

fn audit(msg: String) {
    if let Ok(mut g) = AUDIT.lock() {
        g.push(msg);
        let len = g.len();
        if len > 500 {
            g.drain(0..len - 500);
        }
    }
}

/// 跨模块审计入口（X-4 插件 / X-5 守护共用）。
pub fn ext_log(msg: &str) {
    audit(msg.to_string());
}

// ---------------------------------------------------------------- asset url

/// 等价前端 convertFileSrc：`http://asset.localhost/<encodeURIComponent(path)>`
fn asset_url(p: &std::path::Path) -> CmdResult<url::Url> {
    let text = p.to_string_lossy();
    let mut enc = String::new();
    for &b in text.as_bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric() || "-_.~".contains(c) {
            enc.push(c);
        } else {
            enc.push_str(&format!("%{b:02X}"));
        }
    }
    url::Url::parse(&format!("http://asset.localhost/{enc}"))
        .map_err(|e| AppError::validation(format!("asset url 解析失败: {e}")))
}

/// 扩展宿主初始化脚本：注入 `window.variable.*` 桥（token = 扩展 id，来自 Rust）。
fn host_bridge_script(ext_id: &str) -> String {
    let safe = ext_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>();
    format!(
        r#"(function () {{
  var EXT_ID = "{safe}";
  function call(api, payload) {{
    return window.__TAURI_INTERNALS__.invoke("ext_invoke", {{ extId: EXT_ID, api: api, payload: payload ?? null }});
  }}
  window.variable = {{
    apiVersion: 1,
    extId: EXT_ID,
    storage: {{
      read: function (scope, key) {{ return call("storage.read", {{ scope: scope, key: key }}); }},
      write: function (scope, key, value) {{ return call("storage.write", {{ scope: scope, key: key, value: value }}); }},
      delete: function (scope, key) {{ return call("storage.delete", {{ scope: scope, key: key }}); }},
      keys: function (scope) {{ return call("storage.keys", {{ scope: scope }}); }}
    }},
    notify: {{ create: function (o) {{ return call("notify.create", o || {{}}); }} }},
    hardware: {{ status: function () {{ return call("hardware.status", null); }} }},
    events: {{ on: function (ev) {{ return call("events.subscribe", {{ event: ev }}); }} }},
    widgets: {{ register: function (o) {{ return call("widgets.register", o || {{}}); }} }},
    windows: {{ open: function (o) {{ return call("windows.open", o || {{}}); }} }},
    layout: {{
      getSnapshots: function () {{ return call("layout.getSnapshots", null); }},
      apply: function (o) {{ return call("layout.apply", o || {{}}); }}
    }},
    theme: {{ patch: function (o) {{ return call("theme.patch", o || {{}}); }} }},
    aihub: {{ invoke: function (toolId, args) {{ return call("aihub.invoke", {{ toolId: toolId, args: args ?? null }}); }} }}
  }};
}})();"#
    )
}

// ---------------------------------------------------------------- storage（AES 加密落容器）

fn ext_store_dir(st: &crate::state::AppState, id: &str) -> PathBuf {
    st.data_dir.join("extensions-store").join(id)
}

fn scope_file(st: &crate::state::AppState, id: &str, scope: &str) -> CmdResult<PathBuf> {
    let safe_scope: String = scope
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe_scope.is_empty() || safe_scope != scope {
        return Err(AppError::validation("非法存储作用域"));
    }
    let dir = ext_store_dir(st, id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::io(format!("extensions-store 创建失败: {e}")))?;
    Ok(dir.join(format!("{safe_scope}.bin")))
}

fn scope_key(dir: &PathBuf) -> CmdResult<[u8; 32]> {
    let kf = dir.join("key.bin");
    if kf.is_file() {
        let raw = std::fs::read(&kf).map_err(|e| AppError::io(e.to_string()))?;
        let mut k = [0u8; 32];
        k.copy_from_slice(&raw[..32]);
        Ok(k)
    } else {
        use rand::RngCore;
        let mut k = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut k);
        std::fs::write(&kf, k).map_err(|e| AppError::io(e.to_string()))?;
        Ok(k)
    }
}

fn load_map(st: &crate::state::AppState, id: &str, scope: &str) -> CmdResult<BTreeMap<String, Value>> {
    let f = scope_file(st, id, scope)?;
    if !f.is_file() {
        return Ok(BTreeMap::new());
    }
    let dir = ext_store_dir(st, id);
    let key = scope_key(&dir)?;
    let blob = std::fs::read(&f).map_err(|e| AppError::io(e.to_string()))?;
    let plain = crate::shell::privacy::open_seal_pub(&key, &blob)?;
    let map: BTreeMap<String, Value> = serde_json::from_slice(&plain)
        .map_err(|e| AppError::validation(format!("存储损坏: {e}")))?;
    Ok(map)
}

fn save_map(
    st: &crate::state::AppState,
    id: &str,
    scope: &str,
    map: &BTreeMap<String, Value>,
) -> CmdResult<()> {
    let f = scope_file(st, id, scope)?;
    let dir = ext_store_dir(st, id);
    let key = scope_key(&dir)?;
    let plain = serde_json::to_vec(map).map_err(|e| AppError::validation(e.to_string()))?;
    let blob = crate::shell::privacy::seal_pub(&key, &plain)?;
    std::fs::write(&f, blob).map_err(|e| AppError::io(e.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------- commands

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: String,
    pub permissions: Vec<String>,
    pub description: Option<String>,
    pub enabled: bool,
    pub signed: bool,
    pub crashed: bool,
    pub running: bool,
    /// V1 边界：csp 登记未按窗生效（见模块头声明）
    pub csp: Option<String>,
}

#[tauri::command]
pub fn ext_list(st: tauri::State<'_, crate::state::AppState>) -> CmdResult<Vec<ExtView>> {
    ext_rescan(st)
}

/// 目录即安装：每次列表都重扫（含热重载语义）；卸载 = 删目录。
#[tauri::command]
pub fn ext_rescan(st: tauri::State<'_, crate::state::AppState>) -> CmdResult<Vec<ExtView>> {
    let root = ext_root(&st);
    let _ = std::fs::create_dir_all(&root);
    let mut found: Vec<ExtEntry> = Vec::new();
    let rd = std::fs::read_dir(&root).map_err(|e| AppError::io(e.to_string()))?;
    for e in rd.flatten() {
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            if let Some(mut entry) = scan_one(&e.path()) {
                // 保留内存态（enabled/crashed/subs）
                if let Ok(g) = EXTS.lock() {
                    if let Some(prev) = g.iter().find(|p| p.manifest.id == entry.manifest.id) {
                        entry.enabled = prev.enabled;
                        entry.crashed = prev.crashed;
                        entry.subs = prev.subs.clone();
                    }
                }
                found.push(entry);
            }
        }
    }
    *EXTS.lock().map_err(|_| AppError::db("extensions mutex poisoned"))? = found;
    let guard = EXTS.lock().map_err(|_| AppError::db("extensions mutex poisoned"))?;
    Ok(guard
        .iter()
        .map(|e| ExtView {
            id: e.manifest.id.clone(),
            name: e.manifest.name.clone(),
            version: e.manifest.version.clone(),
            kind: e.manifest.kind.clone(),
            permissions: e.manifest.permissions.clone(),
            description: e.manifest.description.clone(),
            enabled: e.enabled,
            signed: e.manifest.signature.as_ref().map(|s| !s.is_empty()).unwrap_or(false),
            crashed: e.crashed,
            running: !e.crashed && e.enabled && e.manifest.kind == "web",
            csp: e.manifest.csp.clone(),
        })
        .collect())
}

#[tauri::command]
pub fn ext_set_enabled(id: String, on: bool) -> CmdResult<()> {
    with_ext(&id, |e| {
        e.enabled = on;
        if on {
            e.crashed = false;
        }
    })
}

/// 启动 Web 扩展宿主（隐藏窗口；entry 经 asset 协议加载，注入 variable 桥）。
#[tauri::command]
pub fn ext_open_web(app: tauri::AppHandle, id: String) -> CmdResult<String> {
    let (dir, entry, script) = {
        let guard = EXTS.lock().map_err(|_| AppError::db("extensions mutex poisoned"))?;
        let e = guard
            .iter()
            .find(|e| e.manifest.id == id && e.enabled && e.manifest.kind == "web")
            .ok_or_else(|| AppError::not_found(format!("Web 扩展不可启动: {id}")))?;
        (
            e.dir.clone(),
            e.manifest.entry.clone(),
            host_bridge_script(&id),
        )
    };
    let label = format!("ext-{id}");
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.destroy();
    }
    let url = asset_url(&dir.join(&entry))?;
    let _ = tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::External(url),
    )
    .title(format!("Variable Extension: {id}"))
    .visible(false)
    .inner_size(360.0, 240.0)
    .initialization_script(&script)
    .build()
    .map_err(|e| AppError::io(format!("扩展宿主创建失败: {e}")))?;
    with_ext(&id, |e| {
        e.crashed = false;
    })?;
    // boot 事件（events:subscribe 语义）
    let _ = app.emit_to(&label, "variable://event", serde_json::json!({ "event": "boot", "extId": id }));
    Ok(label)
}

#[tauri::command]
pub fn ext_close(app: tauri::AppHandle, id: String) -> CmdResult<()> {
    if let Some(w) = app.get_webview_window(&format!("ext-{id}")) {
        let _ = w.destroy();
    }
    Ok(())
}

/// 扩展崩溃/退出标记（webview Destroyed 时由窗口事件回调调用）。
pub fn mark_crashed(label: &str) {
    if let Some(id) = label.strip_prefix("ext-") {
        let _ = with_ext(id, |e| e.crashed = true);
        audit(format!("[ext] {id} webview destroyed (crashed/stopped)"));
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtAuditView {
    pub lines: Vec<String>,
}

#[tauri::command]
pub fn ext_audit() -> CmdResult<ExtAuditView> {
    Ok(ExtAuditView {
        lines: AUDIT.lock().map_err(|_| AppError::db("mutex"))?.clone(),
    })
}

/// X-1 验收示例：时钟小组件（目录即安装）。
#[tauri::command]
pub fn ext_install_example(st: tauri::State<'_, crate::state::AppState>) -> CmdResult<String> {
    let dir = ext_root(&st).join("clock-widget");
    std::fs::create_dir_all(&dir).map_err(|e| AppError::io(e.to_string()))?;
    let manifest = r#"{
  "id": "clock-widget",
  "name": "Clock Widget",
  "version": "1.0.0",
  "type": "web",
  "entry": "main.html",
  "permissions": ["widget", "notify", "events:subscribe", "storage:read:kv", "storage:write:kv"],
  "description": "示例扩展：桌面时钟小组件（X-1 验收样例）"
}"#;
    let html = r#"<!doctype html><html><head><meta charset="utf-8"><style>
body { margin: 0; font-family: "Segoe UI", sans-serif; color: #e8eefc; }
#clock { font-size: 34px; letter-spacing: 2px; }
#sub { font-size: 12px; opacity: .7; }</style></head><body>
<div id="clock">--:--:--</div><div id="sub">clock-widget</div>
<script>
variable.widgets.register({ slot: "desktop-top-right", title: "Clock" }).then(function () {
  variable.storage.write("kv", "boot", String(Date.now()));
});
setInterval(function () {
  document.getElementById("clock").textContent = new Date().toLocaleTimeString();
}, 1000);
</script></body></html>"#;
    std::fs::write(dir.join(".uxpack.json"), manifest).map_err(|e| AppError::io(e.to_string()))?;
    std::fs::write(dir.join("main.html"), html).map_err(|e| AppError::io(e.to_string()))?;
    Ok(dir.to_string_lossy().to_string())
}

// ---------------------------------------------------------------- IPC 桥（X-3 API 面 v1）

#[tauri::command]
pub fn ext_invoke(
    app: tauri::AppHandle,
    st: tauri::State<'_, crate::state::AppState>,
    webview: tauri::WebviewWindow,
    ext_id: String,
    api: String,
    payload: Option<Value>,
) -> CmdResult<Value> {
    use serde_json::json;
    let p = payload.unwrap_or(Value::Null);

    // token 绑定：调用方 webview label 必须等于 ext-<extId>（JS 伪造 extId 无效）
    if webview.label() != format!("ext-{ext_id}") {
        audit(format!("[ext] DENY {ext_id}: label mismatch ({})", webview.label()));
        return Err(AppError::validation("扩展身份校验失败"));
    }

    let manifest = {
        let guard = EXTS.lock().map_err(|_| AppError::db("extensions mutex poisoned"))?;
        guard
            .iter()
            .find(|e| e.manifest.id == ext_id)
            .map(|e| e.manifest.clone())
            .ok_or_else(|| AppError::not_found(format!("扩展未加载: {ext_id}")))?
    };
    if !manifest.enabled_flag() {
        audit(format!("[ext] DENY {ext_id}: disabled"));
        return Err(AppError::validation("扩展已停用"));
    }

    macro_rules! need {
        ($perm:expr) => {
            if !manifest.has_permission($perm) {
                audit(format!("[ext] DENY {ext_id}: missing permission {}", $perm));
                return Err(AppError::validation(format!("缺少权限: {}", $perm)));
            }
        };
    }

    let out = match api.as_str() {
        // ---- storage（作用域 KV，AES-GCM 落容器）----
        "storage.read" => {
            let scope = pj(&p, "scope")?;
            let key = pj(&p, "key")?;
            need!("storage:read");
            let map = load_map(&st, &ext_id, scope)?;
            json!(map.get(key).cloned())
        }
        "storage.write" => {
            let scope = pj(&p, "scope")?;
            let key = pj(&p, "key")?;
            need!("storage:write");
            let mut map = load_map(&st, &ext_id, scope)?;
            let value = p.get("value").cloned().unwrap_or(Value::Null);
            map.insert(key.to_string(), value);
            save_map(&st, &ext_id, scope, &map)?;
            json!(true)
        }
        "storage.delete" => {
            let scope = pj(&p, "scope")?;
            let key = pj(&p, "key")?;
            need!("storage:write");
            let mut map = load_map(&st, &ext_id, scope)?;
            map.remove(key);
            save_map(&st, &ext_id, scope, &map)?;
            json!(true)
        }
        "storage.keys" => {
            let scope = pj(&p, "scope")?;
            need!("storage:read");
            let map = load_map(&st, &ext_id, scope)?;
            json!(map.keys().collect::<Vec<_>>())
        }
        // ---- notify ----
        "notify.create" => {
            need!("notify");
            let title = p.get("title").and_then(|v| v.as_str()).unwrap_or("扩展通知");
            let body = p.get("body").and_then(|v| v.as_str()).unwrap_or("");
            let _ = app.emit_to("desktop", "ext://notify", json!({ "extId": ext_id, "title": title, "body": body }));
            // 转发给已订阅 notify 的扩展（events:subscribe）
            forward_event(&app, "notify", &json!({ "event": "notify", "from": ext_id }));
            json!(true)
        }
        // ---- hardware（只读摘要）----
        "hardware.status" => {
            need!("hardware:read");
            let s = crate::shell::sysinfo::hardware_summary()?;
            json!(s)
        }
        // ---- events ----
        "events.subscribe" => {
            need!("events:subscribe");
            let ev = p
                .get("event")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AppError::validation("缺少 event"))?
                .to_string();
            with_ext(&ext_id, |e| {
                if !e.subs.contains(&ev) {
                    e.subs.push(ev.clone());
                }
            })?;
            json!(true)
        }
        // ---- widgets ----
        "widgets.register" => {
            need!("widget");
            let mut g = WIDGETS.lock().map_err(|_| AppError::db("mutex"))?;
            g.retain(|w| w.get("extId").and_then(|v| v.as_str()) != Some(ext_id.as_str()));
            let w = json!({
                "extId": ext_id,
                "slot": p.get("slot").and_then(|v| v.as_str()).unwrap_or("desktop-top-right"),
                "title": p.get("title").and_then(|v| v.as_str()).unwrap_or(&ext_id),
            });
            let _ = app.emit_to("desktop", "ext://widget-registered", w.clone());
            g.push(w);
            json!(true)
        }
        // ---- windows（VWM 自定义窗口 V1：独立轻量窗口承载）----
        "windows.open" => {
            need!("window");
            let title = p
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Extension Window")
                .to_string();
            // content 限扩展目录内相对路径（防目录穿越）
            let rel = p.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let dir = {
                let guard = EXTS.lock().map_err(|_| AppError::db("mutex"))?;
                guard
                    .iter()
                    .find(|e| e.manifest.id == ext_id)
                    .map(|e| e.dir.clone())
                    .ok_or_else(|| AppError::not_found("ext gone"))?
            };
            let target = dir.join(rel.trim_start_matches(['/']));
            if !target.starts_with(&dir) || !target.is_file() {
                return Err(AppError::validation("content 必须是扩展目录内存在的文件"));
            }
            let url = asset_url(&target)?;
            let label = format!("extwin-{ext_id}");
            if let Some(w) = app.get_webview_window(&label) {
                let _ = w.set_focus();
                return Ok(json!({ "label": label }));
            }
            tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::External(url))
                .title(title)
                .inner_size(480.0, 360.0)
                .min_inner_size(280.0, 200.0)
                .initialization_script(&host_bridge_script(&ext_id))
                .build()
                .map_err(|e| AppError::io(format!("扩展窗口创建失败: {e}")))?;
            json!({ "label": label })
        }
        // ---- layout（V1 内存态快照）----
        "layout.getSnapshots" => {
            need!("layout:control");
            let g = LAYOUTS.lock().map_err(|_| AppError::db("mutex"))?;
            json!(g.clone())
        }
        "layout.apply" => {
            need!("layout:control");
            let _ = app.emit_to("desktop", "ext://layout-apply", p.clone());
            json!(true)
        }
        // ---- theme（局部 token）----
        "theme.patch" => {
            need!("theme:patch");
            let _ = app.emit_to("desktop", "ext://theme-patch", json!({ "extId": ext_id, "tokens": p }));
            json!(true)
        }
        // ---- aihub（受控调用 AI Hub 启动通道）----
        "aihub.invoke" => {
            need!("aihub:invoke");
            let tool_id = p
                .get("toolId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AppError::validation("缺少 toolId"))?
                .to_string();
            let _ = p.get("args"); // V1：aihub 启动通道不透传任意参数（最小面）
            let msg = crate::shell::ai::ai_launch(st, tool_id, None)?;
            json!({ "launched": msg })
        }
        other => {
            audit(format!("[ext] DENY {ext_id}: unknown api {other}"));
            return Err(AppError::validation(format!("未知 API: {other}")));
        }
    };
    audit(format!("[ext] {ext_id} {} OK", api));
    Ok(out)
}

fn pj<'a>(p: &'a Value, k: &str) -> CmdResult<&'a str> {
    p.get(k)
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::validation(format!("缺少参数: {k}")))
}

/// 事件转发：发给订阅了该事件的扩展宿主窗口。
fn forward_event(app: &tauri::AppHandle, ev: &str, payload: &Value) {
    if let Ok(g) = EXTS.lock() {
        for e in g.iter() {
            if e.enabled && e.subs.iter().any(|s| s == ev) {
                let _ = app.emit_to(&format!("ext-{}", e.manifest.id), "variable://event", payload.clone());
            }
        }
    }
}

impl UxpackManifest {
    /// enabled 存于 ExtEntry；manifest 层恒 true（占位供桥内快速判定）。
    fn enabled_flag(&self) -> bool {
        if let Ok(g) = EXTS.lock() {
            if let Some(e) = g.iter().find(|e| e.manifest.id == self.id) {
                return e.enabled;
            }
        }
        false
    }
}

// ---------------------------------------------------------------- X-6 分发与商店

/// `.uxpack` 分发容器 V1：JSON 信封 `{ uxpack: 1, manifest, files: { 相对路径: base64 } }`。
/// V1 边界（如实声明）：容器未压缩；`signature` 字段随包分发，但验签执行属后续批；
/// 安装前逐权限回显确认由 UI 层完成（见 Marketplace.tsx）。
#[derive(Serialize, Deserialize)]
struct UxpackEnvelope {
    uxpack: u32,
    manifest: UxpackManifest,
    files: BTreeMap<String, String>,
}

fn market_dir(st: &crate::state::AppState) -> PathBuf {
    st.data_dir.join("market")
}

/// 解析并校验 .uxpack 信封：版本、manifest、路径安全（防目录穿越/绝对路径/反斜杠）、entry 必须在包内。
fn parse_envelope(raw: &str) -> CmdResult<UxpackEnvelope> {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw); // 容忍 UTF-8 BOM
    let env: UxpackEnvelope = serde_json::from_str(raw)
        .map_err(|e| AppError::validation(format!(".uxpack 解析失败: {e}")))?;
    if env.uxpack != 1 {
        return Err(AppError::validation(format!("不支持的容器版本: {}", env.uxpack)));
    }
    env.manifest
        .validate()
        .map_err(AppError::validation)?;
    if env.files.is_empty() {
        return Err(AppError::validation("包内没有文件"));
    }
    for name in env.files.keys() {
        if name.is_empty()
            || name.contains("..")
            || name.contains('\\')
            || name.starts_with('/')
        {
            return Err(AppError::validation(format!("非法包内路径: {name}")));
        }
    }
    if !env.files.contains_key(&env.manifest.entry) {
        return Err(AppError::validation(format!(
            "entry 不在包内: {}",
            env.manifest.entry
        )));
    }
    Ok(env)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketPackView {
    /// 市场目录内文件名（安装/删除以此为准）
    pub file: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: String,
    pub description: Option<String>,
    pub permissions: Vec<String>,
    pub installed: bool,
    pub signed: bool,
    pub size_bytes: u64,
}

/// 商店列表：扫 `<data>/market/*.uxpack`，逐包解析（坏包跳过并审计）。
#[tauri::command]
pub fn ext_market_list(st: tauri::State<'_, crate::state::AppState>) -> CmdResult<Vec<MarketPackView>> {
    let dir = market_dir(&st);
    let _ = std::fs::create_dir_all(&dir);
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).map_err(|e| AppError::io(e.to_string()))?.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("uxpack") {
            continue;
        }
        let file = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let raw = match std::fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let env = match parse_envelope(&raw) {
            Ok(v) => v,
            Err(_) => {
                audit(format!("[market] skip broken pack {file}"));
                continue;
            }
        };
        let installed = ext_root(&st).join(&env.manifest.id).join(".uxpack.json").is_file();
        out.push(MarketPackView {
            file,
            id: env.manifest.id,
            name: env.manifest.name,
            version: env.manifest.version,
            kind: env.manifest.kind,
            description: env.manifest.description,
            permissions: env.manifest.permissions,
            installed,
            signed: env.manifest.signature.as_ref().map(|s| !s.is_empty()).unwrap_or(false),
            size_bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
        });
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}

/// 导入 .uxpack 包到市场目录（复制；解析校验通过才收货）。
#[tauri::command]
pub fn ext_market_import(st: tauri::State<'_, crate::state::AppState>, path: String) -> CmdResult<String> {
    let src = PathBuf::from(&path);
    if src.extension().and_then(|s| s.to_str()) != Some("uxpack") || !src.is_file() {
        return Err(AppError::validation("请选择 .uxpack 文件"));
    }
    let raw = std::fs::read_to_string(&src).map_err(|e| AppError::io(e.to_string()))?;
    let env = parse_envelope(&raw)?;
    let dir = market_dir(&st);
    std::fs::create_dir_all(&dir).map_err(|e| AppError::io(e.to_string()))?;
    let dest = dir.join(src.file_name().unwrap_or_default());
    std::fs::copy(&src, &dest).map_err(|e| AppError::io(e.to_string()))?;
    audit(format!("[market] import {} ({})", env.manifest.id, dest.display()));
    Ok(dest.to_string_lossy().to_string())
}

/// 安装：解包到 `<data>/extensions/<id>/`（覆盖式），随后重扫生效。返回扩展 id。
#[tauri::command]
pub fn ext_market_install(st: tauri::State<'_, crate::state::AppState>, file: String) -> CmdResult<String> {
    // file 限市场目录内文件名（防路径穿越）
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err(AppError::validation("非法包文件名"));
    }
    let pack = market_dir(&st).join(&file);
    let raw = std::fs::read_to_string(&pack).map_err(|e| AppError::io(e.to_string()))?;
    let env = parse_envelope(&raw)?;
    let dest = ext_root(&st).join(&env.manifest.id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| AppError::io(e.to_string()))?;
    }
    std::fs::create_dir_all(&dest).map_err(|e| AppError::io(e.to_string()))?;
    use base64::Engine as _;
    for (name, b64) in &env.files {
        let target = dest.join(name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(e.to_string()))?;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|_| AppError::validation(format!("文件 base64 损坏: {name}")))?;
        std::fs::write(&target, bytes).map_err(|e| AppError::io(e.to_string()))?;
    }
    audit(format!("[market] install {} -> {}", env.manifest.id, dest.display()));
    Ok(env.manifest.id)
}

/// 从市场移除包（不影响已安装目录）。
#[tauri::command]
pub fn ext_market_remove(st: tauri::State<'_, crate::state::AppState>, file: String) -> CmdResult<()> {
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err(AppError::validation("非法包文件名"));
    }
    let pack = market_dir(&st).join(&file);
    if pack.is_file() {
        std::fs::remove_file(&pack).map_err(|e| AppError::io(e.to_string()))?;
    }
    audit(format!("[market] remove {file}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mf(perms: &[&str]) -> UxpackManifest {
        UxpackManifest {
            id: "test-ext".into(),
            name: "T".into(),
            version: "1.0.0".into(),
            kind: "web".into(),
            permissions: perms.iter().map(|s| s.to_string()).collect(),
            entry: "main.html".into(),
            csp: None,
            signature: None,
            description: None,
        }
    }

    #[test]
    fn permission_exact_and_scope() {
        let m = mf(&["notify", "storage:read:kv"]);
        assert!(m.has_permission("notify"));
        assert!(m.has_permission("storage:read"));
        assert!(!m.has_permission("storage:write"));
        assert!(!m.has_permission("aihub:invoke"));
    }

    #[test]
    fn manifest_validation() {
        let mut m = mf(&["notify"]);
        assert!(m.validate().is_ok());
        m.id = "Bad_ID".into();
        assert!(m.validate().is_err());
        m.id = "ok-id".into();
        m.kind = "alien".into();
        assert!(m.validate().is_err());
        m.kind = "external".into();
        m.permissions = vec!["nope".into()];
        assert!(m.validate().is_err());
    }

    #[test]
    fn bridge_script_binds_ext_id() {
        let s = host_bridge_script("clock-widget");
        assert!(s.contains("clock-widget"));
        assert!(s.contains("ext_invoke"));
    }

    #[test]
    fn envelope_parse_and_safety() {
        use base64::Engine as _;
        let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
        let good = serde_json::json!({
            "uxpack": 1,
            "manifest": { "id": "demo", "name": "D", "version": "1.0.0", "type": "web", "entry": "main.html", "permissions": ["notify"] },
            "files": { "main.html": b64("<h1>hi</h1>") }
        })
        .to_string();
        let env = parse_envelope(&good).unwrap();
        assert_eq!(env.manifest.id, "demo");
        assert_eq!(env.files.len(), 1);

        // entry 不在包内
        let no_entry = good.replace("main.html\":", "other.html\":");
        assert!(parse_envelope(&no_entry).is_err());
        // 目录穿越
        let evil = serde_json::json!({
            "uxpack": 1,
            "manifest": { "id": "demo", "name": "D", "version": "1.0.0", "type": "web", "entry": "../evil.js" },
            "files": { "../evil.js": b64("x") }
        })
        .to_string();
        assert!(parse_envelope(&evil).is_err());
        // 错误容器版本
        let bad_ver = good.replace("\"uxpack\":1", "\"uxpack\":2");
        assert!(parse_envelope(&bad_ver).is_err());
    }
}
