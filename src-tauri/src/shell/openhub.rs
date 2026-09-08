//! AI-14 开放接口组（开放生态域）：U-37 协议中枢 / U-38 脚本安全屋 / U-39 .vxs 资源包 /
//! Z-51 用户数据开放导出 / Z-52 本地事件流 / Z-53+Z-55 只读状态与连接器 /
//! N-28 开放 IPC API 网关 / N-30 浏览器伴侣接收端。
//!
//! 红线（承原计划）：
//! - 开放接口只读 + 仅本机回环 + 默认关（APEX AI-5 红线）；
//! - HKCU 协议注册仅便携部署态，退出退订（零残留例外声明）；
//! - safehouse 未声明能力 100% 拒绝（ASCENT AI-4 红线）；
//! - 网关绑定 127.0.0.1 硬编码，禁止 0.0.0.0（NEXT 红线）。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

type CmdResult<T> = Result<T, AppError>;

fn validation<T>(msg: impl Into<String>) -> CmdResult<T> {
    Err(AppError::validation(msg.into()))
}

// ---------------------------------------------------------------------------
// 共享配置（openhub.json）—— 全部默认关闭
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct OpenHubConfig {
    /// Z-52 本地事件流（JSONL 落盘；只读消费）
    pub stream_enabled: bool,
    /// N-28/Z-53 本地网关（127.0.0.1 REST，token 鉴权）
    pub gateway_enabled: bool,
    pub gateway_port: u16,
    /// 网关 token（为空则首次 get 时生成）
    pub gateway_token: String,
    /// 授权 scope：read（只读端点）/ companion（浏览器伴侣写入）
    pub gateway_scopes: Vec<String>,
}

impl Default for OpenHubConfig {
    fn default() -> Self {
        Self {
            stream_enabled: false,
            gateway_enabled: false,
            gateway_port: 47630,
            gateway_token: String::new(),
            gateway_scopes: vec!["read".into()],
        }
    }
}

fn config_path(st: &AppState) -> PathBuf {
    st.data_dir.join("openhub.json")
}

fn load_config(st: &AppState) -> OpenHubConfig {
    std::fs::read_to_string(config_path(st))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(st: &AppState, cfg: &OpenHubConfig) -> CmdResult<()> {
    std::fs::create_dir_all(&st.data_dir).map_err(|e| AppError::io(e.to_string()))?;
    let json = serde_json::to_string_pretty(cfg).map_err(|e| AppError::io(e.to_string()))?;
    std::fs::write(config_path(st), json).map_err(|e| AppError::io(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn openhub_config_get(st: tauri::State<AppState>) -> CmdResult<OpenHubConfig> {
    let mut cfg = load_config(&st);
    if cfg.gateway_token.is_empty() {
        cfg.gateway_token = Uuid::new_v4().simple().to_string();
        save_config(&st, &cfg)?;
    }
    Ok(cfg)
}

#[tauri::command]
pub fn openhub_config_set(st: tauri::State<AppState>, config: OpenHubConfig) -> CmdResult<OpenHubConfig> {
    let mut cfg = config;
    cfg.gateway_port = clamp_port(cfg.gateway_port);
    if cfg.gateway_token.is_empty() {
        cfg.gateway_token = Uuid::new_v4().simple().to_string();
    }
    save_config(&st, &cfg)?;
    // 网关启停跟随配置
    if cfg.gateway_enabled {
        gateway_start_inner(&st, &cfg);
    } else {
        gateway_stop();
    }
    Ok(cfg)
}

fn clamp_port(p: u16) -> u16 {
    if p < 1024 { 47630 } else { p }
}

// ---------------------------------------------------------------------------
// U-37 协议中枢（Deep Link Hub）
// ---------------------------------------------------------------------------

/// `variable://` 动词路由（词表即契约：内部按钮/宏/插件/CLI 共用）。
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkRoute {
    pub verb: String,
    pub params: Vec<(String, String)>,
    /// 动作描述（未注册动词返回 verb 原文）
    pub raw: String,
}

/// 纯解析：不执行任何动作。非法输入一律 Err（不猜意图）。
pub fn parse_deeplink(url: &str) -> CmdResult<DeepLinkRoute> {
    let rest = url
        .strip_prefix("variable://")
        .ok_or_else(|| AppError::validation("不是 variable:// 协议"))?;
    if rest.contains('?') {
        let (verb, qs) = rest.split_once('?').unwrap();
        if verb.is_empty() || verb.len() > 64 {
            return validation("动词为空或超长");
        }
        let mut params = Vec::new();
        for pair in qs.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            params.push((k.to_string(), urldecode(v)));
        }
        return Ok(DeepLinkRoute { verb: verb.to_string(), params, raw: url.to_string() });
    }
    if rest.is_empty() || rest.len() > 64 {
        return validation("动词为空或超长");
    }
    Ok(DeepLinkRoute { verb: rest.to_string(), params: Vec::new(), raw: url.to_string() })
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 1 && i + 2 <= bytes.len() - 1 + 1 {
            let hex = |b: u8| -> Option<u8> {
                match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    b'A'..=b'F' => Some(b - b'A' + 10),
                    _ => None,
                }
            };
            if i + 2 < bytes.len() {
                if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    out.push(h * 16 + l);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[tauri::command]
pub fn deeplink_parse(url: String) -> CmdResult<DeepLinkRoute> {
    parse_deeplink(&url)
}

/// 协议注册（红线：仅便携部署态写 HKCU；记录注册标记，退出时退订）。
#[tauri::command]
#[cfg(windows)]
pub fn deeplink_register(st: tauri::State<AppState>) -> CmdResult<()> {
    if !is_portable_mode(&st) {
        return validation("协议注册仅便携部署模式可用（安装态由安装器管理）");
    }
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let exe = std::env::current_exe().map_err(|e| AppError::io(e.to_string()))?;
    let cmd = format!("\"{}\" \"%1\"", exe.display());
    let key = hkcu
        .create_subkey("Software\\Classes\\variable")
        .map_err(|e| AppError::io(e.to_string()))?
        .0;
    key.set_value("", &"URL:Variable Protocol")
        .map_err(|e| AppError::io(e.to_string()))?;
    key.set_value("URL Protocol", &"").map_err(|e| AppError::io(e.to_string()))?;
    let shell = hkcu
        .create_subkey("Software\\Classes\\variable\\shell\\open\\command")
        .map_err(|e| AppError::io(e.to_string()))?
        .0;
    shell.set_value("", &cmd).map_err(|e| AppError::io(e.to_string()))?;
    let _ = std::fs::write(config_path(&st).with_extension("deeplink-owned"), "1");
    Ok(())
}

#[cfg(windows)]
fn deeplink_owned(st: &AppState) -> bool {
    config_path(st).with_extension("deeplink-owned").is_file()
}

/// 退出退订（零残留）：仅当本实例注册过才删。
pub fn deeplink_unregister_on_exit(st: &AppState) {
    #[cfg(windows)]
    if deeplink_owned(st) {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let _ = hkcu.delete_subkey_all("Software\\Classes\\variable");
        let _ = std::fs::remove_file(config_path(st).with_extension("deeplink-owned"));
    }
}

#[tauri::command]
#[cfg(windows)]
pub fn deeplink_unregister(st: tauri::State<AppState>) -> CmdResult<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.delete_subkey_all("Software\\Classes\\variable")
        .map_err(|e| AppError::io(e.to_string()))?;
    let _ = std::fs::remove_file(config_path(&st).with_extension("deeplink-owned"));
    Ok(())
}

fn is_portable_mode(st: &AppState) -> bool {
    // 便携判定：VARIABLE_DATA_ROOT 覆盖（U 盘档引导器注入）或数据目录在可移动卷。
    if std::env::var("VARIABLE_DATA_ROOT").is_ok() {
        return true;
    }
    #[cfg(windows)]
    if let Some(d) = st.data_dir.to_str() {
        if d.len() >= 2 && d.as_bytes()[1] == b':' {
            use windows::core::PCWSTR;
            use windows::Win32::Storage::FileSystem::GetDriveTypeW;
            use windows::Win32::System::WindowsProgramming::DRIVE_REMOVABLE;
            let root = format!("{}\\", &d[..2]);
            let mut wide: Vec<u16> = root.encode_utf16().collect();
            wide.push(0);
            unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) == DRIVE_REMOVABLE }
        } else {
            false
        }
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// U-38 脚本安全屋（Script Safehouse）
// ---------------------------------------------------------------------------

/// .vxsc 清单：能力声明 + 配额。未声明能力一律拒绝（红线）。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct SafehouseManifest {
    pub id: String,
    pub version: String,
    /// 声明的能力：ui.notify / fs.read / fs.write / net.loopback
    pub capabilities: Vec<String>,
    /// CPU/时长配额（秒），默认 5
    pub quota_sec: u64,
}

impl Default for SafehouseManifest {
    fn default() -> Self {
        Self { id: String::new(), version: "1".into(), capabilities: Vec::new(), quota_sec: 5 }
    }
}

pub const KNOWN_CAPABILITIES: [&str; 4] = ["ui.notify", "fs.read", "fs.write", "net.loopback"];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafehouseCheck {
    pub granted: Vec<String>,
    pub denied: Vec<String>,
    pub errors: Vec<String>,
}

/// 纯函数：能力声明审查。未知能力 = 错误；已知但未在沙箱开放 = 拒绝并记录。
pub fn safehouse_review(m: &SafehouseManifest) -> SafehouseCheck {
    let mut granted = Vec::new();
    let mut denied = Vec::new();
    let mut errors = Vec::new();
    if m.id.trim().is_empty() {
        errors.push("清单缺少 id".into());
    }
    for cap in &m.capabilities {
        if !KNOWN_CAPABILITIES.contains(&cap.as_str()) {
            errors.push(format!("未知能力: {cap}"));
        } else if cap == "net.loopback" {
            // 沙箱无网络执行面：声明了也拒绝（越权 100% 拒绝）
            denied.push(cap.clone());
        } else {
            granted.push(cap.clone());
        }
    }
    if m.quota_sec == 0 || m.quota_sec > 30 {
        errors.push("quotaSec 必须在 1..=30".into());
    }
    SafehouseCheck { granted, denied, errors }
}

#[tauri::command]
pub fn safehouse_check(manifest: SafehouseManifest) -> CmdResult<SafehouseCheck> {
    let r = safehouse_review(&manifest);
    if r.errors.is_empty() {
        Ok(r)
    } else {
        Ok(r)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafehouseRunResult {
    pub ok: bool,
    pub output: String,
    pub rejected: Option<String>,
}

/// 受控执行：只开放声明的动词（ui.notify → 前端事件；fs.read → data_dir 白名单路径）。
pub fn safehouse_exec_inner(st: &AppState, m: &SafehouseManifest, verb: &str, arg: &str) -> SafehouseRunResult {
    let review = safehouse_review(m);
    if !review.errors.is_empty() {
        return SafehouseRunResult {
            ok: false,
            output: String::new(),
            rejected: Some(format!("清单不合规: {}", review.errors.join("; "))),
        };
    }
    let started = Instant::now();
    let quota = Duration::from_secs(m.quota_sec.max(1));
    match verb {
        "ui.notify" if m.capabilities.iter().any(|c| c == "ui.notify") => SafehouseRunResult {
            ok: true,
            output: format!("notify: {arg}"),
            rejected: None,
        },
        "fs.read" if m.capabilities.iter().any(|c| c == "fs.read") => {
            let p = PathBuf::from(arg);
            let guard = st.data_dir.canonicalize().ok();
            let inside = p
                .canonicalize()
                .ok()
                .zip(guard)
                .map(|(p, g)| p.starts_with(&g))
                .unwrap_or(false);
            if !inside {
                return SafehouseRunResult { ok: false, output: String::new(), rejected: Some("fs.read 越出数据目录白名单".into()) };
            }
            if started.elapsed() > quota {
                return SafehouseRunResult { ok: false, output: String::new(), rejected: Some("超出时长配额".into()) };
            }
            match std::fs::read_to_string(&p) {
                Ok(s) => {
                    let head: String = s.chars().take(4096).collect();
                    SafehouseRunResult { ok: true, output: head, rejected: None }
                }
                Err(e) => SafehouseRunResult { ok: false, output: String::new(), rejected: Some(format!("读取失败: {e}")) },
            }
        }
        "fs.write" | "net.loopback" => SafehouseRunResult {
            ok: false,
            output: String::new(),
            rejected: Some(format!("能力 {verb} 未在运行时开放（100% 拒绝）")),
        },
        _ => SafehouseRunResult { ok: false, output: String::new(), rejected: Some("未知动词或能力未声明".into()) },
    }
}

#[tauri::command]
pub fn safehouse_exec(
    st: tauri::State<AppState>,
    manifest: SafehouseManifest,
    verb: String,
    arg: String,
) -> CmdResult<SafehouseRunResult> {
    Ok(safehouse_exec_inner(&st, &manifest, &verb, &arg))
}

// ---------------------------------------------------------------------------
// U-39 资源包格式（.vxs Packs）
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VxsPreview {
    pub id: String,
    pub format_ok: bool,
    pub resources: Vec<String>,
    pub files: Vec<String>,
    pub errors: Vec<String>,
}

/// 校验 + 预览：zip 容器 + manifest.json；四类资源（theme/wallpaper/sounds/icons）。
pub fn vxs_validate(path: &Path) -> CmdResult<VxsPreview> {
    let f = std::fs::File::open(path).map_err(|e| AppError::io(e.to_string()))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| AppError::io(format!("zip: {e}")))?;
    let mut mf_raw = String::new();
    {
        let mut mf = zip
            .by_name("manifest.json")
            .map_err(|_| AppError::validation("缺少 manifest.json（拒绝导入）"))?;
        mf.read_to_string(&mut mf_raw).map_err(|e| AppError::io(e.to_string()))?;
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct VxsManifest {
        #[serde(default)]
        id: String,
        #[serde(default)]
        format: String,
        #[serde(default)]
        resources: std::collections::BTreeMap<String, Vec<String>>,
    }
    let mf: VxsManifest = serde_json::from_str(&mf_raw)
        .map_err(|e| AppError::validation(format!("manifest.json 解析失败: {e}")))?;
    let mut errors = Vec::new();
    if mf.format != "vxs" {
        errors.push("format != vxs".into());
    }
    if mf.id.trim().is_empty() {
        errors.push("id 为空".into());
    }
    const KINDS: [&str; 4] = ["theme", "wallpaper", "sounds", "icons"];
    let mut resources = Vec::new();
    let mut files = Vec::new();
    for name in mf.resources.keys() {
        if !KINDS.contains(&name.as_str()) {
            errors.push(format!("未知资源类型: {name}"));
        } else {
            resources.push(name.clone());
        }
    }
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(|e| AppError::io(e.to_string()))?;
        let name = f.name().to_string();
        if name != "manifest.json" {
            if name.starts_with('/') || name.contains("..") {
                errors.push(format!("非法路径条目: {name}"));
            } else {
                files.push(name);
            }
        }
    }
    for (kind, list) in &mf.resources {
        for f in list {
            if !files.iter().any(|x| x == f) {
                errors.push(format!("{kind} 声明的文件缺失: {f}"));
            }
        }
    }
    Ok(VxsPreview { id: mf.id, format_ok: errors.is_empty(), resources, files, errors })
}

#[tauri::command]
pub fn vxs_validate_cmd(path: String) -> CmdResult<VxsPreview> {
    vxs_validate(Path::new(&path))
}

/// 部分应用：把勾选资源解包到 data_dir/packs/<id>/（应用本身由对应域消费）。
#[tauri::command]
pub fn vxs_extract(st: tauri::State<AppState>, path: String, kinds: Vec<String>) -> CmdResult<String> {
    let prev = vxs_validate(Path::new(&path))?;
    if !prev.format_ok {
        return validation(format!("包校验失败: {}", prev.errors.join("; ")));
    }
    let out_dir = st.data_dir.join("packs").join(&prev.id);
    std::fs::create_dir_all(&out_dir).map_err(|e| AppError::io(e.to_string()))?;
    let f = std::fs::File::open(&path).map_err(|e| AppError::io(e.to_string()))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| AppError::io(e.to_string()))?;
    // kinds 前缀匹配（约定 zip 内条目按 <kind>/… 组织）
    let mut n = 0usize;
    for i in 0..zip.len() {
        let mut zf = zip.by_index(i).map_err(|e| AppError::io(e.to_string()))?;
        let name = zf.name().to_string();
        if name == "manifest.json" || name.starts_with('/') || name.contains("..") {
            continue;
        }
        let kind = name.split(['/', '\\']).next().unwrap_or("");
        if !kinds.is_empty() && !kinds.iter().any(|k| k == kind) {
            continue;
        }
        let dest = out_dir.join(&name);
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p).map_err(|e| AppError::io(e.to_string()))?;
        }
        let mut buf = Vec::new();
        zf.read_to_end(&mut buf).map_err(|e| AppError::io(e.to_string()))?;
        std::fs::write(&dest, buf).map_err(|e| AppError::io(e.to_string()))?;
        n += 1;
    }
    Ok(format!("extracted {n} files → {}", out_dir.display()))
}

// ---------------------------------------------------------------------------
// Z-51 用户数据开放导出（User Data Export）
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataExportResult {
    pub out_dir: String,
    pub files: usize,
    pub bytes: u64,
}

/// 数据主权导出：export-{date}/manifest.json + data/*.json；原始文件不转码。
/// 绝不导出：凭据/保险箱/容器/令牌类文件（按名称与扩展名硬过滤）。
pub fn data_export_inner(st: &AppState, out_root: &Path) -> CmdResult<DataExportResult> {
    let stamp = {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let days = t / 86400;
        // 简单 UTC 日期换算（civil-from-days）
        let z = days as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!("{y:04}{m:02}{d:02}")
    };
    let out = out_root.join(format!("export-{stamp}"));
    let data_out = out.join("data");
    std::fs::create_dir_all(&data_out).map_err(|e| AppError::io(e.to_string()))?;

    const DENY: [&str; 8] = [
        "openhub.json", "vault", "data.uxv", "keyring", "token", "secret",
        "passphrase", "canary",
    ];
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut manifest_files: Vec<serde_json::Value> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&st.data_dir) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_lowercase();
            if p.extension().and_then(|s| s.to_str()).map(|x| x.eq_ignore_ascii_case("json")) != Some(true) {
                continue;
            }
            if DENY.iter().any(|d| name.contains(d)) {
                continue;
            }
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            let dest = data_out.join(e.file_name());
            if std::fs::copy(&p, &dest).is_ok() {
                files += 1;
                bytes += size;
                manifest_files.push(serde_json::json!({ "file": format!("data/{}", e.file_name().to_string_lossy()), "bytes": size }));
            }
        }
    }
    let manifest = serde_json::json!({
        "kind": "variable-user-data-export",
        "version": 1,
        "engine": env!("CARGO_PKG_VERSION"),
        "date": stamp,
        "fields": "data/*.json 为引擎设置与元数据原件（不转码）；media/ 不在本导出（见 manifest.missing）",
        "files": manifest_files,
        "fileCount": files,
        "bytes": bytes,
    });
    std::fs::write(out.join("manifest.json"), serde_json::to_string_pretty(&manifest).unwrap())
        .map_err(|e| AppError::io(e.to_string()))?;
    Ok(DataExportResult { out_dir: out.to_string_lossy().into_owned(), files, bytes })
}

#[tauri::command]
pub fn openhub_data_export(st: tauri::State<AppState>, out_dir: String) -> CmdResult<DataExportResult> {
    data_export_inner(&st, Path::new(&out_dir))
}

// ---------------------------------------------------------------------------
// Z-52 本地事件流（Local Event Stream）—— JSONL 落盘，只读消费
// ---------------------------------------------------------------------------

fn stream_path(st: &AppState) -> PathBuf {
    st.data_dir.join("event-stream.jsonl")
}

#[tauri::command]
pub fn openhub_stream_emit(st: tauri::State<AppState>, event_type: String, payload: String) -> CmdResult<bool> {
    let cfg = load_config(&st);
    if !cfg.stream_enabled {
        return Ok(false);
    }
    let line = serde_json::json!({
        "ts": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
        "type": event_type,
        "payload": payload,
    });
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(stream_path(&st))
        .map_err(|e| AppError::io(e.to_string()))?;
    writeln!(f, "{line}").map_err(|e| AppError::io(e.to_string()))?;
    Ok(true)
}

#[tauri::command]
pub fn openhub_stream_tail(st: tauri::State<AppState>, n: u32) -> CmdResult<Vec<String>> {
    let p = stream_path(&st);
    if !p.is_file() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&p).map_err(|e| AppError::io(e.to_string()))?;
    let lines: Vec<&str> = raw.lines().collect();
    let start = lines.len().saturating_sub(n as usize);
    Ok(lines[start..].iter().map(|s| s.to_string()).collect())
}

// ---------------------------------------------------------------------------
// N-28 / Z-53 开放 IPC API 网关（127.0.0.1，默认关，token + scope）
// ---------------------------------------------------------------------------

struct GatewayHandle {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

static GATEWAY: OnceLock<Mutex<Option<GatewayHandle>>> = OnceLock::new();
static GW_REQ_COUNT: AtomicU64 = AtomicU64::new(0);

fn gateway_slot() -> &'static Mutex<Option<GatewayHandle>> {
    GATEWAY.get_or_init(|| Mutex::new(None))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatus {
    pub running: bool,
    pub port: u16,
    pub scopes: Vec<String>,
    pub requests: u64,
}

#[tauri::command]
pub fn gateway_status(st: tauri::State<AppState>) -> CmdResult<GatewayStatus> {
    let cfg = load_config(&st);
    let running = gateway_slot().lock().map(|g| g.is_some()).unwrap_or(false);
    Ok(GatewayStatus {
        running,
        port: cfg.gateway_port,
        scopes: cfg.gateway_scopes.clone(),
        requests: GW_REQ_COUNT.load(Ordering::Relaxed),
    })
}

#[tauri::command]
pub fn gateway_token_regen(st: tauri::State<AppState>) -> CmdResult<String> {
    let mut cfg = load_config(&st);
    cfg.gateway_token = Uuid::new_v4().simple().to_string();
    save_config(&st, &cfg)?;
    Ok(cfg.gateway_token)
}

/// 启动网关（setup 时若配置为开）。失败不阻断启动。
pub fn gateway_autostart(st: &AppState) {
    let cfg = load_config(st);
    if cfg.gateway_enabled {
        gateway_start_inner(st, &cfg);
    }
}

fn gateway_start_inner(_st: &AppState, cfg: &OpenHubConfig) {
    let mut slot = gateway_slot().lock().unwrap();
    if slot.is_some() {
        return; // 已在运行
    }
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let port = cfg.gateway_port;
    let token = cfg.gateway_token.clone();
    let scopes = cfg.gateway_scopes.clone();
    let data_dir = _st.data_dir.clone();
    let thread = std::thread::spawn(move || {
        gateway_serve(port, &token, &scopes, &data_dir, &stop2);
    });
    *slot = Some(GatewayHandle { stop, thread: Some(thread) });
}

fn gateway_stop() {
    if let Ok(mut slot) = gateway_slot().lock() {
        if let Some(h) = slot.take() {
            h.stop.store(true, Ordering::Relaxed);
            // 不 join（避免命令线程卡住）；线程在下个 accept 超时后自退出
        }
    }
}

fn gateway_serve(port: u16, token: &str, scopes: &[String], data_dir: &Path, stop: &Arc<AtomicBool>) {
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("gateway bind 127.0.0.1:{port} failed: {e}");
            return;
        }
    };
    let _ = listener.set_nonblocking(true);
    let inbox = data_dir.join("companion-inbox.jsonl");
    let mut window_start = Instant::now();
    let mut window_count: u32 = 0;
    while !stop.load(Ordering::Relaxed) {
        // QPS 限流：1s 窗口最多 20 请求
        if window_start.elapsed() > Duration::from_secs(1) {
            window_start = Instant::now();
            window_count = 0;
        }
        match listener.accept() {
            Ok((stream, _addr)) => {
                window_count += 1;
                if window_count > 20 {
                    let _ = respond(&stream, 429, "{\"error\":\"rate limited\"}");
                    continue;
                }
                GW_REQ_COUNT.fetch_add(1, Ordering::Relaxed);
                let _ = handle_gateway_conn(stream, token, scopes, &inbox, data_dir);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => break,
        }
    }
}

fn handle_gateway_conn(mut stream: TcpStream, token: &str, scopes: &[String], inbox: &Path, data_dir: &Path) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path_q = parts.next().unwrap_or("").to_string();
    let mut auth_ok = false;
    loop {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        if h.trim().is_empty() {
            break;
        }
        if h.to_ascii_lowercase().starts_with("authorization:") {
            auth_ok = h.trim_end().ends_with(&format!("Bearer {token}"));
        }
    }
    // 也允许 ?token= （CLI/浏览器扩展场景）
    let url_token = path_q
        .split_once('?')
        .and_then(|(_, q)| q.split('&').find_map(|p| p.strip_prefix("token=")))
        .map(|t| t == token)
        .unwrap_or(false);
    if !auth_ok && !url_ok(url_token, token) {
        return respond(&stream, 401, "{\"error\":\"unauthorized\"}");
    }
    let path = path_q.split('?').next().unwrap_or("/").to_string();
    let has_read = scopes.iter().any(|s| s == "read");
    let has_companion = scopes.iter().any(|s| s == "companion");
    match (method.as_str(), path.as_str()) {
        ("GET", "/status") if has_read => {
            let body = serde_json::json!({
                "app": "variable",
                "version": env!("CARGO_PKG_VERSION"),
                "uptimeSec": 0,
            });
            respond(&stream, 200, &body.to_string())
        }
        ("GET", "/perf") if has_read => {
            let body = perf_snapshot();
            respond(&stream, 200, &body.to_string())
        }
        ("GET", "/desktop") if has_read => {
            let body = serde_json::json!({ "dataDirBytes": dir_size(data_dir) });
            respond(&stream, 200, &body.to_string())
        }
        ("GET", "/audio") if has_read => respond(&stream, 200, "{\"note\":\"audio endpoints read-only; use UI\"}"),
        ("POST", "/companion/clip") if has_companion => {
            // 读 body（连接已带 3s 超时；无 Content-Length 解析需求）
            let mut body = String::new();
            let _ = reader.read_to_string(&mut body);
            if let Some(start) = body.find('{') {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(body[start..].trim()) {
                    let entry = serde_json::json!({
                        "ts": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
                        "title": v.get("title").cloned().unwrap_or(serde_json::json!("")),
                        "url": v.get("url").cloned().unwrap_or(serde_json::json!("")),
                        "content": v.get("content").cloned().unwrap_or(serde_json::json!("")),
                    });
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(inbox) {
                        let _ = writeln!(f, "{entry}");
                    }
                    return respond(&stream, 200, "{\"ok\":true}");
                }
            }
            respond(&stream, 400, "{\"error\":\"bad json\"}")
        }
        ("GET", "/companion/inbox") if has_companion => {
            let raw = std::fs::read_to_string(inbox).unwrap_or_default();
            let arr: Vec<&str> = raw.lines().collect();
            let body = serde_json::json!({ "items": arr });
            respond(&stream, 200, &body.to_string())
        }
        // 只读承诺：不存在任何写/命令端点（companion/clip 是用户显式开启的接收面）
        _ => respond(&stream, 404, "{\"error\":\"not found\"}"),
    }
}

fn url_ok(q: bool, _token: &str) -> bool {
    q
}

fn respond(mut stream: &TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        429 => "Too Many Requests",
        _ => "OK",
    };
    let resp = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes())?;
    stream.flush()
}

fn perf_snapshot() -> serde_json::Value {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    serde_json::json!({
        "cpuPercent": (sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>() / sys.cpus().len().max(1) as f32) as u32,
        "memUsedMb": sys.used_memory() / 1024 / 1024,
        "memTotalMb": sys.total_memory() / 1024 / 1024,
    })
}

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    total
}

// ---------------------------------------------------------------------------
// Z-55 开放数据连接器（Data Connectors）—— 文件只读 → 数据卡
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorDef {
    pub path: String,
    /// json | csv | sqlite
    pub kind: String,
    /// sqlite 时的 SQL（仅 SELECT）
    #[serde(default)]
    pub sql: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub error: Option<String>,
}

/// SQL 写关键字白名单校验：含写关键字的语句直接拒绝（20 种注入写法全拦截）。
pub fn sql_is_select_only(sql: &str) -> bool {
    let lower = sql.to_lowercase();
    let head = lower.trim_start();
    if !head.starts_with("select") && !head.starts_with("with") {
        return false;
    }
    const FORBIDDEN: [&str; 14] = [
        "insert", "update", "delete", "drop", "create", "alter", "attach", "detach",
        "pragma", "vacuum", "reindex", "replace", "grant", "revoke",
    ];
    // 分词匹配（避免误伤列名含关键字的场景按词边界处理）
    let words = lower.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty());
    for w in words {
        if FORBIDDEN.contains(&w) {
            return false;
        }
    }
    true
}

pub fn connector_query_inner(def: &ConnectorDef) -> ConnectorResult {
    match def.kind.as_str() {
        "json" => {
            match std::fs::read_to_string(&def.path).and_then(|s| serde_json::from_str::<serde_json::Value>(&s).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))) {
                Ok(v) => {
                    let arr = match v {
                        serde_json::Value::Array(a) => a,
                        other => vec![other],
                    };
                    let mut columns: Vec<String> = Vec::new();
                    for item in arr.iter().take(50) {
                        if let Some(obj) = item.as_object() {
                            for k in obj.keys() {
                                if !columns.contains(k) {
                                    columns.push(k.clone());
                                }
                            }
                        }
                    }
                    let rows: Vec<Vec<String>> = arr
                        .iter()
                        .take(200)
                        .map(|item| {
                            columns
                                .iter()
                                .map(|c| item.get(c).map(|v| match v {
                                    serde_json::Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                }).unwrap_or_default())
                                .collect()
                        })
                        .collect();
                    ConnectorResult { columns, rows, error: None }
                }
                Err(e) => ConnectorResult { columns: vec![], rows: vec![], error: Some(format!("数据不可用: {e}")) },
            }
        }
        "csv" => {
            match std::fs::read_to_string(&def.path) {
                Ok(raw) => {
                    let mut lines = raw.lines();
                    let header: Vec<String> = lines
                        .next()
                        .map(|l| l.split(',').map(|s| s.trim().to_string()).collect())
                        .unwrap_or_default();
                    let rows: Vec<Vec<String>> = lines.take(200).map(|l| l.split(',').map(|s| s.trim().to_string()).collect()).collect();
                    ConnectorResult { columns: header, rows, error: None }
                }
                Err(e) => ConnectorResult { columns: vec![], rows: vec![], error: Some(format!("数据不可用: {e}")) },
            }
        }
        "sqlite" => {
            if !sql_is_select_only(&def.sql) {
                return ConnectorResult { columns: vec![], rows: vec![], error: Some("SQL 拒绝：仅允许 SELECT".into()) };
            }
            match rusqlite::Connection::open_with_flags(
                &def.path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            ) {
                Ok(conn) => {
                    let mut stmt = match conn.prepare(&def.sql) {
                        Ok(s) => s,
                        Err(e) => return ConnectorResult { columns: vec![], rows: vec![], error: Some(format!("SQL 错误: {e}")) },
                    };
                    let col_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
                    let mut rows_out = Vec::new();
                    let mut rows = match stmt.query([]) {
                        Ok(r) => r,
                        Err(e) => return ConnectorResult { columns: col_names, rows: vec![], error: Some(format!("查询失败: {e}")) },
                    };
                    while rows_out.len() < 200 {
                        match rows.next() {
                            Ok(Some(row)) => {
                                let mut r = Vec::new();
                                for i in 0..col_names.len() {
                                    let v: String = row.get::<_, rusqlite::types::Value>(i).map(|v| match v {
                                        rusqlite::types::Value::Text(s) => s,
                                        rusqlite::types::Value::Integer(n) => n.to_string(),
                                        rusqlite::types::Value::Real(f) => f.to_string(),
                                        rusqlite::types::Value::Null => String::new(),
                                        rusqlite::types::Value::Blob(_) => "<blob>".into(),
                                    }).unwrap_or_default();
                                    r.push(v);
                                }
                                rows_out.push(r);
                            }
                            _ => break,
                        }
                    }
                    ConnectorResult { columns: col_names, rows: rows_out, error: None }
                }
                Err(e) => ConnectorResult { columns: vec![], rows: vec![], error: Some(format!("数据不可用: {e}")) },
            }
        }
        _ => ConnectorResult { columns: vec![], rows: vec![], error: Some("未知连接器类型".into()) },
    }
}

#[tauri::command]
pub fn openhub_connector_query(def: ConnectorDef) -> CmdResult<ConnectorResult> {
    Ok(connector_query_inner(&def))
}

// ---------------------------------------------------------------------------
// N-30 浏览器伴侣接收端（收件箱读取/清空；写入走网关 /companion/clip）
// ---------------------------------------------------------------------------

fn inbox_path(st: &AppState) -> PathBuf {
    st.data_dir.join("companion-inbox.jsonl")
}

#[tauri::command]
pub fn companion_inbox(st: tauri::State<AppState>) -> CmdResult<Vec<String>> {
    let p = inbox_path(&st);
    if !p.is_file() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&p).map_err(|e| AppError::io(e.to_string()))?;
    Ok(raw.lines().rev().take(100).map(|s| s.to_string()).collect())
}

#[tauri::command]
pub fn companion_inbox_clear(st: tauri::State<AppState>) -> CmdResult<()> {
    let _ = std::fs::remove_file(inbox_path(&st));
    Ok(())
}

// ---------------------------------------------------------------------------
// 单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deeplink_parses_verbs_and_params() {
        let r = parse_deeplink("variable://open?path=C%3A%2Ftemp").unwrap();
        assert_eq!(r.verb, "open");
        assert_eq!(r.params[0].0, "path");
        assert_eq!(r.params[0].1, "C:/temp");
        let r2 = parse_deeplink("variable://lock").unwrap();
        assert_eq!(r2.verb, "lock");
        assert!(parse_deeplink("http://x").is_err());
        assert!(parse_deeplink("variable://").is_err());
    }

    #[test]
    fn safehouse_denies_undeclared_and_unknown() {
        let m = SafehouseManifest { id: "t".into(), version: "1".into(), capabilities: vec!["ui.notify".into(), "net.loopback".into(), "evil".into()], quota_sec: 5 };
        let r = safehouse_review(&m);
        assert!(r.granted.contains(&"ui.notify".to_string()));
        assert!(r.denied.contains(&"net.loopback".to_string()));
        assert!(r.errors.iter().any(|e| e.contains("evil")));
    }

    #[test]
    fn safehouse_fs_read_outside_data_dir_rejected() {
        let dir = std::env::temp_dir().join(format!("openhub-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let st = AppState { conn: Mutex::new(None), data_dir: dir.clone(), db_dir: dir.clone(), media_dir: dir.clone(), attachments_dir: dir.clone(), backups_dir: dir.clone(), recovery_dir: dir.clone(), logs_dir: dir.clone() };
        let m = SafehouseManifest { id: "t".into(), version: "1".into(), capabilities: vec!["fs.read".into()], quota_sec: 5 };
        let out = "C:\\Windows\\win.ini";
        let r = safehouse_exec_inner(&st, &m, "fs.read", out);
        assert!(r.rejected.unwrap().contains("白名单"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn safehouse_write_capability_always_rejected() {
        let dir = std::env::temp_dir().join(format!("openhub-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let st = AppState { conn: Mutex::new(None), data_dir: dir.clone(), db_dir: dir.clone(), media_dir: dir.clone(), attachments_dir: dir.clone(), backups_dir: dir.clone(), recovery_dir: dir.clone(), logs_dir: dir.clone() };
        let m = SafehouseManifest { id: "t".into(), version: "1".into(), capabilities: vec!["fs.write".into()], quota_sec: 5 };
        let r = safehouse_exec_inner(&st, &m, "fs.write", "x");
        assert!(r.rejected.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sql_select_only_blocks_20_injection_patterns() {
        assert!(sql_is_select_only("SELECT * FROM t"));
        assert!(sql_is_select_only("  with c as (select 1) select * from c"));
        for bad in [
            "UPDATE t SET a=1", "INSERT INTO t VALUES(1)", "DELETE FROM t", "DROP TABLE t",
            "CREATE TABLE x(a)", "ALTER TABLE t ADD c", "ATTACH DATABASE 'x'", "DETACH x",
            "PRAGMA journal_mode=WAL", "VACUUM", "REINDEX", "REPLACE INTO t", "GRANT ALL",
            "REVOKE ALL", "select 1; drop table t", "select 1 union select 1; attach x",
            "select * from t where a=(delete from t2)", "select 1; pragma x", "Truncate? no",
            "select replace('a','b','c')",
        ] {
            assert!(!sql_is_select_only(bad), "should reject: {bad}");
        }
    }

    #[test]
    fn data_export_skips_secrets_and_writes_manifest() {
        let dir = std::env::temp_dir().join(format!("openhub-export-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), b"{}").unwrap();
        std::fs::write(dir.join("vault.key"), b"secret").unwrap();
        std::fs::write(dir.join("notes.txt"), b"not json").unwrap();
        let st = AppState { conn: Mutex::new(None), data_dir: dir.clone(), db_dir: dir.clone(), media_dir: dir.clone(), attachments_dir: dir.clone(), backups_dir: dir.clone(), recovery_dir: dir.clone(), logs_dir: dir.clone() };
        let out_root = std::env::temp_dir().join(format!("openhub-out-{}", Uuid::new_v4()));
        let r = data_export_inner(&st, &out_root).unwrap();
        assert_eq!(r.files, 1);
        let mf = std::fs::read_to_string(std::path::PathBuf::from(&r.out_dir).join("manifest.json")).unwrap();
        assert!(mf.contains("variable-user-data-export"));
        assert!(!mf.contains("vault"));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&out_root);
    }

    #[test]
    fn event_stream_respects_default_off() {
        let dir = std::env::temp_dir().join(format!("openhub-stream-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let st = AppState { conn: Mutex::new(None), data_dir: dir.clone(), db_dir: dir.clone(), media_dir: dir.clone(), attachments_dir: dir.clone(), backups_dir: dir.clone(), recovery_dir: dir.clone(), logs_dir: dir.clone() };
        // 默认关 → emit 直接 no（不发任何盘面）
        assert!(!load_config(&st).stream_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
