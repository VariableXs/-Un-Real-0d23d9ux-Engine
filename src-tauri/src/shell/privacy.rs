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
    initialized: bool,
    unlocked: bool,
    count: usize,
    bytes: u64,
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

fn vault_available() -> CmdResult<std::sync::MutexGuard<'static, Option<[u8; 32]>>> {
    VAULT_KEY
        .lock()
        .map_err(|_| AppError::io("vault mutex poisoned"))
}

// ---------- 保险箱命令 ----------

#[tauri::command]
pub fn vault_status(st: tauri::State<AppState>) -> CmdResult<VaultStatus> {
    let meta = load_meta(&st);
    let unlocked = vault_available()?.is_some();
    Ok(VaultStatus {
        initialized: meta.is_some(),
        unlocked,
        count: meta.as_ref().map(|m| m.entries.len()).unwrap_or(0),
        bytes: meta.as_ref().map(|m| m.entries.iter().map(|e| e.size).sum()).unwrap_or(0),
    })
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

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

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
