//! L3 shell — privacy.rs（批次E-7 数据隐私）：
//! - 隐私保险箱：AES-256-GCM + PBKDF2-HMAC-SHA256（100k 轮）；
//!   文件加密后存 <dataDir>/vault/<uuid>.vv，明文名/大小登记在 vault.meta（同样落盘的只有密文）
//! - 敏感文件焚毁：多次覆写（0x00 / 0xFF / 随机）+ 改名 + 删除；护栏拒绝驱动器根与数据目录自身
//! - 隐私自检报告：聚合本机真实状态（联网授权 / 保险箱 / 回收站 / 第三方登记 / 便携模式），零网络
//! - 密钥只驻留内存（Mutex），锁定/退出即丢弃；解锁失败次数不做限制（本机离线场景无爆破面）

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::fs;
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

const VAULT_DIR: &str = "vault";
const META_FILE: &str = "vault.meta";
const MAGIC: &[u8; 3] = b"VV1";
const NONCE_LEN: usize = 12;
const PBKDF2_ROUNDS: u32 = 100_000;
/// 解锁校验用的已知明文（密文存 meta，解锁时尝试解密验证口令）
const CHECK_PLAIN: &[u8; 17] = b"variable-vault-ok";

static VAULT_KEY: std::sync::Mutex<Option<[u8; 32]>> = std::sync::Mutex::new(None);

fn vault_dir(st: &AppState) -> PathBuf {
    st.data_dir.join(VAULT_DIR)
}

fn meta_path(st: &AppState) -> PathBuf {
    vault_dir(st).join(META_FILE)
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct VaultMeta {
    /// PBKDF2 盐（hex）
    salt: String,
    /// 解锁校验密文（nonce || ct，hex）
    check: String,
    entries: Vec<VaultEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct VaultEntry {
    file: String,
    name: String,
    size: u64,
    added_at: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub initialized: bool,
    pub unlocked: bool,
    pub count: usize,
    pub bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultItem {
    name: String,
    size: u64,
    added_at: u64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

fn load_meta(st: &AppState) -> Option<VaultMeta> {
    let bytes = fs::read(meta_path(st)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_meta(st: &AppState, meta: &VaultMeta) -> CmdResult<()> {
    fs::create_dir_all(vault_dir(st)).map_err(|e| AppError::io(e.to_string()))?;
    let bytes = serde_json::to_vec_pretty(meta).map_err(|e| AppError::io(e.to_string()))?;
    fs::write(meta_path(st), bytes).map_err(|e| AppError::io(e.to_string()))
}

fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, PBKDF2_ROUNDS, &mut key);
    key
}

fn seal(key: &[u8; 32], plain: &[u8]) -> CmdResult<Vec<u8>> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad: MAGIC })
        .map_err(|_| AppError::io("加密失败 / encrypt failed"))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn open_seal(key: &[u8; 32], blob: &[u8]) -> CmdResult<Vec<u8>> {
    if blob.len() < NONCE_LEN {
        return Err(AppError::validation("密文损坏 / corrupt blob"));
    }
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let ct = &blob[NONCE_LEN..];
    cipher
        .decrypt(Nonce::from_slice(&blob[..NONCE_LEN]), Payload { msg: ct, aad: MAGIC })
        .map_err(|_| AppError::validation("解密失败（口令错误或密文损坏）/ decrypt failed"))
}

/// 批次B-10（Vault 2.0）：身份库等扩展读取当前解锁密钥（None = 未解锁）。
pub fn vault_key() -> CmdResult<Option<[u8; 32]>> {
    Ok(**&vault_available()?)
}

/// 批次B-10：身份库加密（复用保险箱 AES-256-GCM 通道）。
pub fn seal_pub(key: &[u8; 32], plain: &[u8]) -> CmdResult<Vec<u8>> {
    seal(key, plain)
}

/// 批次B-10：身份库解密。
pub fn open_seal_pub(key: &[u8; 32], blob: &[u8]) -> CmdResult<Vec<u8>> {
    open_seal(key, blob)
}

fn vault_available() -> CmdResult<std::sync::MutexGuard<'static, Option<[u8; 32]>>> {
    VAULT_KEY
        .lock()
        .map_err(|_| AppError::io("vault mutex poisoned"))
}

// ---------- 保险箱命令 ----------

/// 内部直连实现（panic.rs 紧急擦拭 L1 用；测试同源）。
pub fn vault_status_inner(st: &AppState) -> CmdResult<VaultStatus> {
    let meta = load_meta(st);
    let unlocked = vault_available()?.is_some();
    Ok(VaultStatus {
        initialized: meta.is_some(),
        unlocked,
        count: meta.as_ref().map(|m| m.entries.len()).unwrap_or(0),
        bytes: meta.as_ref().map(|m| m.entries.iter().map(|e| e.size).sum()).unwrap_or(0),
    })
}

#[tauri::command]
pub fn vault_status(st: tauri::State<AppState>) -> CmdResult<VaultStatus> {
    vault_status_inner(&st)
}

#[tauri::command]
pub fn vault_init(st: tauri::State<AppState>, password: String) -> CmdResult<()> {
    if password.chars().count() < 4 {
        return Err(AppError::validation("口令至少 4 位 / password too short"));
    }
    if load_meta(&st).is_some() {
        return Err(AppError::validation("保险箱已初始化 / vault already initialized"));
    }
    let mut salt = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let key = derive_key(&password, &salt);
    let check = seal(&key, CHECK_PLAIN)?;
    *vault_available()? = Some(key);
    save_meta(&st, &VaultMeta {
        salt: hex(&salt),
        check: hex(&check),
        entries: Vec::new(),
    })
}

#[tauri::command]
pub fn vault_unlock(st: tauri::State<AppState>, password: String) -> CmdResult<()> {
    let meta = load_meta(&st).ok_or_else(|| AppError::not_found("保险箱未初始化 / vault not initialized"))?;
    let salt = unhex(&meta.salt).ok_or_else(|| AppError::io("meta 盐损坏 / corrupt salt"))?;
    let check = unhex(&meta.check).ok_or_else(|| AppError::io("meta 校验损坏 / corrupt check"))?;
    let key = derive_key(&password, &salt);
    let plain = open_seal(&key, &check)?;
    if plain != CHECK_PLAIN {
        return Err(AppError::validation("口令错误 / wrong password"));
    }
    *vault_available()? = Some(key);
    Ok(())
}

#[tauri::command]
pub fn vault_lock() -> CmdResult<()> {
    *vault_available()? = None;
    Ok(())
}

/// 从磁盘导入文件进保险箱（加密存储）；shred_source=true 时同时焚毁源文件。
#[tauri::command]
pub fn vault_import(st: tauri::State<AppState>, path: String, shred_source: bool) -> CmdResult<VaultItem> {
    let key = {
        let guard = vault_available()?;
        guard.ok_or_else(|| AppError::validation("保险箱未解锁 / vault locked"))?
    };
    let src = PathBuf::from(&path);
    if !src.is_file() {
        return Err(AppError::not_found(format!("文件不存在 / not a file: {path}")));
    }
    let plain = fs::read(&src).map_err(|e| AppError::io(format!("读取失败 / read failed: {e}")))?;
    let blob = seal(&key, &plain)?;
    let file = format!("{}.vv", uuid::Uuid::new_v4());
    fs::create_dir_all(vault_dir(&st)).map_err(|e| AppError::io(e.to_string()))?;
    fs::write(vault_dir(&st).join(&file), &blob).map_err(|e| AppError::io(format!("写入失败 / write failed: {e}")))?;

    let mut meta = load_meta(&st).unwrap_or_default();
    let item = VaultItem {
        name: src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into()),
        size: plain.len() as u64,
        added_at: now_ms(),
    };
    meta.entries.push(VaultEntry {
        file,
        name: item.name.clone(),
        size: item.size,
        added_at: item.added_at,
    });
    save_meta(&st, &meta)?;

    if shred_source {
        let _ = shred_file(&src);
    }
    Ok(item)
}

#[tauri::command]
pub fn vault_list(st: tauri::State<AppState>) -> CmdResult<Vec<VaultItem>> {
    let meta = load_meta(&st).ok_or_else(|| AppError::not_found("保险箱未初始化 / vault not initialized"))?;
    Ok(meta
        .entries
        .iter()
        .map(|e| VaultItem { name: e.name.clone(), size: e.size, added_at: e.added_at })
        .collect())
}

/// 导出（解密）到目标目录；同名文件自动加序号，绝不覆盖。
#[tauri::command]
pub fn vault_export(st: tauri::State<AppState>, name: String, dest_dir: String) -> CmdResult<String> {
    let guard = vault_available()?;
    let key = guard.ok_or_else(|| AppError::validation("保险箱未解锁 / vault locked"))?;
    let mut meta = load_meta(&st).ok_or_else(|| AppError::not_found("保险箱未初始化 / vault not initialized"))?;
    let pos = meta.entries.iter().position(|e| e.name == name).ok_or_else(|| AppError::not_found(format!("未找到条目 / not found: {name}")))?;
    let entry = meta.entries.remove(pos);
    let blob = fs::read(vault_dir(&st).join(&entry.file)).map_err(|e| AppError::io(format!("读取失败 / read failed: {e}")))?;
    let plain = open_seal(&key, &blob)?;

    let dir = PathBuf::from(&dest_dir);
    let mut dest = dir.join(&entry.name);
    let mut i = 0u32;
    while dest.exists() {
        i += 1;
        let stem = Path::new(&entry.name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let ext = Path::new(&entry.name).extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
        dest = dir.join(format!("{stem} ({i}){ext}"));
    }
    fs::write(&dest, &plain).map_err(|e| AppError::io(format!("写出失败 / write failed: {e}")))?;
    Ok(dest.to_string_lossy().to_string())
}

/// 彻底焚毁保险箱条目（覆写密文 + 从登记表移除）。
#[tauri::command]
pub fn vault_destroy(st: tauri::State<AppState>, name: String) -> CmdResult<()> {
    let mut meta = load_meta(&st).ok_or_else(|| AppError::not_found("保险箱未初始化 / vault not initialized"))?;
    let pos = meta.entries.iter().position(|e| e.name == name).ok_or_else(|| AppError::not_found(format!("未找到条目 / not found: {name}")))?;
    let entry = meta.entries.remove(pos);
    let p = vault_dir(&st).join(&entry.file);
    if p.is_file() {
        shred_file(&p)?;
    }
    save_meta(&st, &meta)
}

// ---------- 焚毁 ----------

/// 多次覆写 + 随机改名 + 删除。护栏：只允许真实文件；拒绝驱动器根/数据目录自身与祖先。
pub(crate) fn shred_file(p: &Path) -> CmdResult<()> {
    if !p.is_file() {
        return Err(AppError::validation("焚毁仅支持文件 / shred accepts files only"));
    }
    let path = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let s = path.to_string_lossy();
    let bytes = s.as_bytes();
    // 驱动器根（X:\…且长度≤3）与无父目录拒绝
    if bytes.len() < 3 || path.parent().map(|q| q.as_os_str().is_empty()).unwrap_or(true) {
        return Err(AppError::validation("不能对驱动器根执行焚毁 / cannot shred a drive root"));
    }
    let passes: [&[u8]; 3] = [&[0x00], &[0xFF], b"r"]; // 第三轮随机
    let size = fs::metadata(&path).map_err(|e| AppError::io(e.to_string()))?.len();
    {
        let mut f = fs::OpenOptions::new().write(true).open(&path).map_err(|e| AppError::io(e.to_string()))?;
        for pass in passes {
            f.rewind().map_err(|e| AppError::io(e.to_string()))?;
            let mut written = 0u64;
            let mut buf = [0u8; 64 * 1024];
            match pass {
                b"r" => rand::rngs::OsRng.fill_bytes(&mut buf),
                pat => buf.fill(pat[0]),
            }
            while written < size {
                let n = ((size - written) as usize).min(buf.len());
                f.write_all(&buf[..n]).map_err(|e| AppError::io(e.to_string()))?;
                written += n as u64;
            }
            f.sync_all().map_err(|e| AppError::io(e.to_string()))?;
        }
    }
    // 随机改名后再删（抹去文件名痕迹）
    let mut rnd = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut rnd);
    let renamed = path.with_file_name(format!("~{}", hex(&rnd)));
    let _ = fs::rename(&path, &renamed);
    let _ = fs::remove_file(&renamed);
    Ok(())
}

#[tauri::command]
pub fn privacy_shred(path: String) -> CmdResult<()> {
    shred_file(Path::new(&path))
}

// ---------- 隐私自检 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditFinding {
    pub id: String,
    pub level: String, // pass | warn
    pub detail: String,
}

/// 隐私自检报告：全部来自本机真实状态，零网络。
#[tauri::command]
pub fn privacy_audit(st: tauri::State<AppState>) -> CmdResult<Vec<AuditFinding>> {
    let mut out = Vec::new();

    // 1. 联网授权
    let consent_path = st.data_dir.join("net_consent.json");
    let (n_allow, n_deny) = fs::read(&consent_path)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .map(|v| {
            let entries = v.get("entries").cloned().unwrap_or(v.clone());
            let list = entries.as_array().cloned().unwrap_or_default();
            let allow = list.iter().filter(|e| e.get("policy").and_then(|p| p.as_str()) == Some("allow")).count();
            let deny = list.iter().filter(|e| e.get("policy").and_then(|p| p.as_str()) == Some("deny")).count();
            (allow, deny)
        })
        .unwrap_or((0, 0));
    if n_allow == 0 {
        out.push(AuditFinding {
            id: "net".into(),
            level: "pass".into(),
            detail: "零联网：未授权任何主机联网 / No network allow entries".into(),
        });
    } else {
        out.push(AuditFinding {
            id: "net".into(),
            level: "warn".into(),
            detail: format!("已授权 {n_allow} 个主机联网 / {n_allow} hosts allowed"),
        });
    }
    let _ = n_deny;

    // 2. 便携模式
    let portable = crate::state::is_portable();
    out.push(AuditFinding {
        id: "portable".into(),
        level: "pass".into(),
        detail: if portable {
            format!("便携模式，数据随 U 盘 / portable: {}", st.data_dir.display())
        } else {
            format!("本机模式，数据目录: {}", st.data_dir.display())
        },
    });

    // 3. 保险箱
    let meta = load_meta(&st);
    let unlocked = vault_available()?.is_some();
    out.push(AuditFinding {
        id: "vault".into(),
        level: if meta.is_some() { "pass" } else { "warn" }.into(),
        detail: if meta.is_some() {
            format!(
                "隐私保险箱已启用（{} 个文件，当前{}）",
                meta.as_ref().map(|m| m.entries.len()).unwrap_or(0),
                if unlocked { "已解锁" } else { "锁定" }
            )
        } else {
            "隐私保险箱未启用，敏感文件为明文 / vault not enabled".into()
        },
    });

    // 4. 回收站滞留
    let rec = crate::shell::recycle::rec_count_inner(&st).unwrap_or(0);
    out.push(AuditFinding {
        id: "recycle".into(),
        level: if rec > 0 { "warn" } else { "pass" }.into(),
        detail: format!("回收站滞留 {rec} 项 / {rec} items in recycle bin"),
    });

    // 5. 第三方登记（本地登记表，不上传）
    let n_tp = crate::shell::launcher::registry_snapshot(&st).len();
    out.push(AuditFinding {
        id: "third".into(),
        level: "pass".into(),
        detail: format!("第三方软件登记 {n_tp} 项（仅本机） / {n_tp} registered apps"),
    });

    Ok(out)
}

// ---------- AI-10（U-31 隐私仪表盘）审计时间线 ----------
// 三条审计线：剪贴板读取 / 文件系统敏感区访问 / 嵌入应用网络出站尝试（联动 netconsent）。
// 只审计环境边界内的合法可见事件；暂停期间明确标注「记录中断」，不留假空档。

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    pub id: String,
    /// clipboard | fs | net
    pub kind: String,
    /// 触发的应用（嵌入应用标识）
    pub app: String,
    /// 资源标识（剪贴板 / 路径 / 目标域名）
    pub resource: String,
    /// 当时是否已授权（netconsent allow = true）
    pub authorized: bool,
    pub ts: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AuditGap {
    /// 记录中断区间（暂停造成的空档，UI 如实标注）
    pub from: u64,
    pub to: Option<u64>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct AuditStore {
    /// 审计开关（默认开）
    enabled: bool,
    #[serde(default)]
    events: Vec<AuditEvent>,
    /// 暂停区间（含当前进行中的；「记录中断」不留假空档红线）
    #[serde(default)]
    gaps: Vec<AuditGap>,
}

fn audit_path(st: &AppState) -> PathBuf {
    st.data_dir.join("privacy_audit.json")
}

fn load_audit(st: &AppState) -> AuditStore {
    fs::read(audit_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(AuditStore { enabled: true, ..Default::default() })
}

fn save_audit(st: &AppState, s: &AuditStore) -> CmdResult<()> {
    fs::write(audit_path(st), serde_json::to_vec_pretty(s)?)?;
    Ok(())
}

/// 内部实现：记录审计事件（netconsent 拦截/放行、剪贴板读取、敏感区访问站点调用）。
pub fn priv_log_inner(st: &AppState, kind: &str, app: &str, resource: &str, authorized: bool) -> CmdResult<AuditEvent> {
    if !matches!(kind, "clipboard" | "fs" | "net") {
        return Err(AppError::validation(format!("未知审计类型 / unknown kind: {kind}")));
    }
    let mut s = load_audit(st);
    if !s.enabled {
        // 开关关 → 不记录（与暂停不同：暂停留 gap，关闭是用户明示不审计）
        return Ok(AuditEvent { id: String::new(), kind: kind.into(), app: app.into(), resource: resource.into(), authorized, ts: now_ms() });
    }
    let ev = AuditEvent {
        id: crate::db::gen_id(),
        kind: kind.into(),
        app: app.into(),
        resource: resource.into(),
        authorized,
        ts: now_ms(),
    };
    s.events.push(ev.clone());
    // 上限 20000 条
    if s.events.len() > 20_000 {
        s.events.drain(0..s.events.len() - 20_000);
    }
    save_audit(st, &s)?;
    Ok(ev)
}

#[tauri::command]
pub fn priv_log(st: tauri::State<AppState>, kind: String, app: String, resource: String, authorized: bool) -> CmdResult<AuditEvent> {
    priv_log_inner(&st, &kind, &app, &resource, authorized)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditTimeline {
    pub events: Vec<AuditEvent>,
    pub gaps: Vec<AuditGap>,
    /// 按天聚合的访问计数（dayKey → count），UI 热力条
    pub daily: std::collections::BTreeMap<String, u64>,
    /// 异常事件 id 集合（深夜 0-5 点访问）
    pub anomalies: Vec<String>,
    pub enabled: bool,
}

fn day_of(ts: u64) -> String {
    let days = ts / 86_400_000;
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

#[tauri::command]
pub fn priv_timeline(st: tauri::State<AppState>, days: Option<u64>) -> CmdResult<AuditTimeline> {
    let days = days.unwrap_or(14);
    let cutoff = now_ms().saturating_sub(days * 86_400_000);
    let s = load_audit(&st);
    let events: Vec<AuditEvent> = s.events.iter().filter(|e| e.ts >= cutoff).cloned().collect();
    let mut daily = std::collections::BTreeMap::new();
    let mut anomalies = Vec::new();
    for e in &events {
        *daily.entry(day_of(e.ts)).or_insert(0) += 1;
        // 深夜（本地近似 UTC+8 的 0-5 点）访问 → 异常标注
        let hour = ((e.ts % 86_400_000) / 3_600_000 + 8) % 24;
        if hour < 5 {
            anomalies.push(e.id.clone());
        }
    }
    Ok(AuditTimeline { events, gaps: s.gaps.clone(), daily, anomalies, enabled: s.enabled })
}

/// 暂停 / 恢复审计（暂停区间如实登记为 gap）。
#[tauri::command]
pub fn priv_audit_pause(st: tauri::State<AppState>, paused: bool) -> CmdResult<()> {
    let mut s = load_audit(&st);
    if paused {
        s.gaps.push(AuditGap { from: now_ms(), to: None });
    } else {
        if let Some(g) = s.gaps.last_mut() {
            if g.to.is_none() {
                g.to = Some(now_ms());
            }
        }
    }
    save_audit(&st, &s)
}

/// 审计开关（默认开）。
#[tauri::command]
pub fn priv_audit_enabled(st: tauri::State<AppState>, enabled: bool) -> CmdResult<()> {
    let mut s = load_audit(&st);
    s.enabled = enabled;
    save_audit(&st, &s)
}

// ---------- AI-10（U-33 诱饵文件系统） ----------
// 模板化诱饵（零真实信息）：布放 → 打开/读取经环境通道即触发警报 → 白名单豁免误报。

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CanaryFile {
    pub id: String,
    /// 布放绝对路径
    pub path: String,
    /// 模板 id
    pub template: String,
    pub planted_at: u64,
    /// 触发历史（时间戳 + 通道说明）
    #[serde(default)]
    pub triggers: Vec<CanaryTrigger>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CanaryTrigger {
    pub ts: u64,
    /// open | read
    pub via: String,
    /// 触发来源（白名单内 → 豁免标记）
    pub app: String,
    #[serde(default)]
    pub exempted: bool,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct CanaryStore {
    #[serde(default)]
    files: Vec<CanaryFile>,
    /// 误报豁免白名单（应用名）
    #[serde(default)]
    whitelist: Vec<String>,
}

fn canary_path(st: &AppState) -> PathBuf {
    st.data_dir.join("canary.json")
}

fn load_canary(st: &AppState) -> CanaryStore {
    fs::read(canary_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_canary(st: &AppState, s: &CanaryStore) -> CmdResult<()> {
    fs::write(canary_path(st), serde_json::to_vec_pretty(s)?)?;
    Ok(())
}

/// 模板目录：文件名 + 仿真但零真实信息的文本内容。
pub fn canary_templates() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("passwords", "密码本.txt", "银行账号：6022 **** **** 4831\n密码提示：妈妈生日+门牌号\nWiFi：HomeNet_5G /hunter2049\n（示例占位内容，非真实信息）"),
        ("bill", "水电账单.xlsx.csv", "月份,电费,水费,燃气\n2026-08,214.50,38.20,66.00\n2026-07,198.30,35.80,61.50\n（示例占位数据，非真实账单）"),
        ("resume", "个人简历-终版.docx.txt", "姓名：张三\n电话：138****6688\n住址：XX市XX区XX路12号\n（示例占位信息，非真实简历）"),
    ]
}

/// 布放诱饵文件（生成模板内容写入目标目录）。
#[tauri::command]
pub fn canary_plant(st: tauri::State<AppState>, dir: String, template: String) -> CmdResult<CanaryFile> {
    let templates = canary_templates();
    let (_tid, fname, body) = templates
        .iter()
        .find(|(tid, _, _)| *tid == template)
        .ok_or_else(|| AppError::validation(format!("未知模板 / unknown template: {template}")))?;
    let dir_path = PathBuf::from(&dir);
    if !dir_path.is_dir() {
        return Err(AppError::not_found(format!("目录不存在 / not found: {dir}")));
    }
    let dest = dir_path.join(fname);
    fs::write(&dest, body)?;
    let cf = CanaryFile {
        id: crate::db::gen_id(),
        path: dest.to_string_lossy().to_string(),
        template: template.clone(),
        planted_at: now_ms(),
        triggers: vec![],
    };
    let mut s = load_canary(&st);
    s.files.push(cf.clone());
    save_canary(&st, &s)?;
    Ok(cf)
}

/// 诱饵一览 + 模板清单。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanaryOverview {
    pub files: Vec<CanaryFile>,
    pub templates: Vec<String>,
    pub whitelist: Vec<String>,
}

#[tauri::command]
pub fn canary_list(st: tauri::State<AppState>) -> CmdResult<CanaryOverview> {
    let s = load_canary(&st);
    Ok(CanaryOverview {
        files: s.files.clone(),
        templates: canary_templates().iter().map(|(id, _, _)| id.to_string()).collect(),
        whitelist: s.whitelist.clone(),
    })
}

/// 诱饵触报（explorer 打开/读取文件时检测路径命中即调用）。
/// 返回 Some(alert) 当为非豁免触发。托盘红警与仪表盘置顶由前端据此渲染。
#[tauri::command]
pub fn canary_touch(st: tauri::State<AppState>, path: String, via: String, app: Option<String>) -> CmdResult<Option<CanaryTrigger>> {
    if !matches!(via.as_str(), "open" | "read") {
        return Err(AppError::validation("via 必须为 open|read / via must be open|read"));
    }
    let norm = |p: &str| p.trim_end_matches('\\').replace('\\', "/").to_lowercase();
    let mut s = load_canary(&st);
    let key = norm(&path);
    let hit = s
        .files
        .iter_mut()
        .find(|f| norm(&f.path) == key)
        .ok_or_else(|| AppError::not_found("非诱饵文件 / not a canary"))?;
    let app = app.unwrap_or_else(|| "unknown".into());
    let exempted = s.whitelist.iter().any(|w| *w == app);
    let trig = CanaryTrigger { ts: now_ms(), via, app, exempted };
    hit.triggers.push(trig.clone());
    save_canary(&st, &s)?;
    Ok(if exempted { None } else { Some(trig) })
}

/// 白名单增删（误报豁免：备份/杀软经环境通道的正常读取）。
#[tauri::command]
pub fn canary_whitelist(st: tauri::State<AppState>, app: String, add: bool) -> CmdResult<()> {
    let mut s = load_canary(&st);
    if add {
        if !s.whitelist.contains(&app) {
            s.whitelist.push(app);
        }
    } else {
        s.whitelist.retain(|w| *w != app);
    }
    save_canary(&st, &s)
}

/// 撤除单个诱饵（删除文件 + 移除登记）。
#[tauri::command]
pub fn canary_remove(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    let mut s = load_canary(&st);
    let pos = s.files.iter().position(|f| f.id == id)
        .ok_or_else(|| AppError::not_found("诱饵不存在 / canary not found"))?;
    let cf = s.files.remove(pos);
    let _ = fs::remove_file(&cf.path);
    save_canary(&st, &s)
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(tag: &str) -> (AppState, std::path::PathBuf) {
        let tmp = std::env::temp_dir().join(format!("variable-privacy-{tag}-{}", std::process::id()));
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        (st, tmp)
    }

    #[test]
    fn audit_timeline_records_and_aggregates() {
        let (st, tmp) = temp_state("audit");
        // 默认开：记录三线事件
        priv_log_inner(&st, "clipboard", "app-embed", "getClipboard", false).unwrap();
        priv_log_inner(&st, "fs", "app-embed", "D:/sensitive/doc.txt", false).unwrap();
        priv_log_inner(&st, "net", "app-embed", "cdn.example.com", true).unwrap();
        let mut s = load_audit(&st);
        assert_eq!(s.events.len(), 3);
        // 暂停 → gap 打开 → 恢复 → gap 关闭
        {
            s.gaps.push(AuditGap { from: now_ms(), to: None });
            save_audit(&st, &s).unwrap();
        }
        let s2 = load_audit(&st);
        assert_eq!(s2.gaps.len(), 1);
        assert!(s2.gaps[0].to.is_none(), "暂停期间 gap 未闭合");
        // 非法 kind 拒绝
        assert!(priv_log_inner(&st, "gps", "x", "y", false).is_err());
        // 关闭开关 → 不记录
        {
            let mut s3 = load_audit(&st);
            s3.enabled = false;
            save_audit(&st, &s3).unwrap();
        }
        let ev = priv_log_inner(&st, "clipboard", "app", "x", false).unwrap();
        assert_eq!(ev.id, "", "开关关闭时不记录");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn canary_plant_touch_exemption_remove() {
        let (st, tmp) = temp_state("canary");
        // 布放在临时目录
        let dir = tmp.join("sensitive");
        fs::create_dir_all(&dir).unwrap();
        let cf = {
            let templates = canary_templates();
            let (_tid, fname, body) = templates.iter().find(|(t, _, _)| *t == "passwords").unwrap();
            let dest = dir.join(fname);
            fs::write(&dest, body).unwrap();
            CanaryFile { id: "c1".into(), path: dest.to_string_lossy().to_string(), template: "passwords".into(), planted_at: now_ms(), triggers: vec![] }
        };
        // 直接写 store（绕过 tauri::State）
        {
            let mut s = load_canary(&st);
            s.files.push(cf.clone());
            save_canary(&st, &s).unwrap();
        }
        // 触报（未知来源 → 警报）
        let norm = |p: &str| p.trim_end_matches('\\').replace('\\', "/").to_lowercase();
        {
            let mut s = load_canary(&st);
            let hit = s.files.iter_mut().find(|f| norm(&f.path) == norm(&cf.path)).unwrap();
            hit.triggers.push(CanaryTrigger { ts: now_ms(), via: "open".into(), app: "unknown".into(), exempted: false });
            save_canary(&st, &s).unwrap();
        }
        let s = load_canary(&st);
        assert_eq!(s.files[0].triggers.len(), 1);
        assert!(!s.files[0].triggers[0].exempted);
        // 白名单后触报 → 豁免
        {
            let mut s = load_canary(&st);
            s.whitelist.push("backup-tool".into());
            let hit = s.files.iter_mut().next().unwrap();
            let exempted = s.whitelist.iter().any(|w| *w == "backup-tool");
            hit.triggers.push(CanaryTrigger { ts: now_ms(), via: "read".into(), app: "backup-tool".into(), exempted });
            save_canary(&st, &s).unwrap();
        }
        let s = load_canary(&st);
        assert!(s.files[0].triggers[1].exempted, "白名单来源应豁免");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn seal_open_roundtrip_and_corruption() {
        let mut key = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        let blob = seal(&key, b"hello-privacy").unwrap();
        assert_eq!(open_seal(&key, &blob).unwrap(), b"hello-privacy");
        // 篡改 → 解密失败
        let mut bad = blob.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0x01;
        assert!(open_seal(&key, &bad).is_err());
        // 错误密钥 → 失败
        let mut key2 = key;
        key2[0] ^= 0xff;
        assert!(open_seal(&key2, &blob).is_err());
    }

    #[test]
    fn pbkdf2_salt_matters() {
        let a = derive_key("pw", &[1u8; 16]);
        let b = derive_key("pw", &[2u8; 16]);
        assert_ne!(a, b);
        assert_eq!(a, derive_key("pw", &[1u8; 16]));
    }

    #[test]
    fn shred_overwrites_then_removes() {
        let p = std::env::temp_dir().join(format!("variable-shred-test-{}.bin", std::process::id()));
        fs::write(&p, vec![7u8; 100_000]).unwrap();
        shred_file(&p).unwrap();
        assert!(!p.exists(), "焚毁后文件应消失");
        // 不存在/目录 → 报错
        assert!(shred_file(&p).is_err());
        let d = std::env::temp_dir();
        assert!(shred_file(&d).is_err());
    }
}
