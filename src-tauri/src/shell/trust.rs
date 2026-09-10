//! AI-10（U-35 信任链中心）：
//! - Authenticode 签名验证（WinVerifyTrust 合法 API，只读）：四态判定
//!   valid / unsigned / expired / tampered（非 Windows 返回 unsupported）
//! - 签名者与有效期：CryptQueryObject → PKCS7 证书store → 首个签名者证书
//! - 来源档案：文件进入方式（drag/download/copy/install）+ 首次时间
//! - 熟识度：同一签名者登记 ≥2 次即视为「熟识」（signer_counts 聚合派生）
//! - 信任视图：全部登记软件的签名状态墙，一键重验（8 路并发）
//! 红线：只读验证，绝不修改被验证文件；验证结果仅存本机，零网络。

use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn trust_dir(st: &AppState) -> PathBuf {
    st.data_dir.join("trust")
}

fn index_path(st: &AppState) -> PathBuf {
    trust_dir(st).join("index.json")
}

/// 单条签名验证结果。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrustVerdict {
    /// valid | unsigned | expired | tampered | unsupported（非 Windows）| error
    pub status: String,
    /// 签名者主体（CN），无签名/未知为空
    pub signer: String,
    /// 证书有效期起（ms，0 = 未知）
    pub valid_from: u64,
    /// 证书有效期止（ms，0 = 未知）
    pub valid_to: u64,
    /// 链完整性：true = WinVerifyTrust 整链通过
    pub chain_ok: bool,
    /// 失败/异常时的补充说明
    pub detail: String,
}

/// 登记条目：路径 + 来源档案 + 最近一次验证结果。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrustEntry {
    /// 规范化路径键
    pub path: String,
    /// 显示路径
    pub display: String,
    /// drag | download | copy | install
    pub origin: String,
    pub first_seen: u64,
    pub last_verified: u64,
    #[serde(default)]
    pub verdict: Option<TrustVerdict>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct TrustIndex {
    /// 规范化路径（小写、正斜杠）→ 条目
    entries: std::collections::BTreeMap<String, TrustEntry>,
}

fn norm_key(path: &str) -> String {
    path.trim_end_matches('\\').replace('\\', "/").to_lowercase()
}

fn load(st: &AppState) -> TrustIndex {
    fs::read(index_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(st: &AppState, idx: &TrustIndex) -> CmdResult<()> {
    fs::create_dir_all(trust_dir(st))?;
    let bytes = serde_json::to_vec_pretty(idx)?;
    fs::write(index_path(st), bytes)?;
    Ok(())
}

// ---------- WinVerifyTrust 封装（仅 Windows） ----------

#[cfg(windows)]
fn verify_file(p: &Path) -> TrustVerdict {
    use windows::Win32::Security::WinTrust::{
        WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_FILE_INFO,
        WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY,
        WTD_UI_NONE,
    };

    if !p.is_file() {
        return TrustVerdict {
            status: "error".into(),
            signer: String::new(),
            valid_from: 0,
            valid_to: 0,
            chain_ok: false,
            detail: "文件不存在 / file not found".into(),
        };
    }

    let wpath: Vec<u16> = p
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut finfo = WINTRUST_FILE_INFO::default();
        finfo.cbStruct = std::mem::size_of::<WINTRUST_FILE_INFO>() as u32;
        finfo.pcwszFilePath = windows::core::PCWSTR(wpath.as_ptr());

        let mut wd = WINTRUST_DATA::default();
        wd.cbStruct = std::mem::size_of::<WINTRUST_DATA>() as u32;
        wd.dwUIChoice = WTD_UI_NONE;
        wd.fdwRevocationChecks = WTD_REVOKE_NONE;
        wd.dwUnionChoice = WTD_CHOICE_FILE;
        wd.Anonymous.pFile = &mut finfo;
        wd.dwStateAction = WTD_STATEACTION_VERIFY;

        let mut guid = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        // 注意：pwvtdata 为 *mut c_void
        let hr = WinVerifyTrust(
            windows::Win32::Foundation::HWND::default(),
            &mut guid,
            &mut wd as *mut _ as *mut core::ffi::c_void,
        );
        // 关闭状态句柄（释放 provider 状态）
        wd.dwStateAction = WTD_STATEACTION_CLOSE;
        let _ = WinVerifyTrust(
            windows::Win32::Foundation::HWND::default(),
            &mut guid,
            &mut wd as *mut _ as *mut core::ffi::c_void,
        );

        let mut verdict = TrustVerdict {
            status: "error".into(),
            signer: String::new(),
            valid_from: 0,
            valid_to: 0,
            chain_ok: false,
            detail: String::new(),
        };
        if hr == 0 {
            verdict.status = "valid".into();
            verdict.chain_ok = true;
        } else {
            let code = hr as u32;
            match code {
                // TRUST_E_NOSIGNATURE
                0x800B_0100 => verdict.status = "unsigned".into(),
                // CERT_E_EXPIRED
                0x800B_0101 => verdict.status = "expired".into(),
                // TRUST_E_BAD_DIGEST（被篡改）
                0x8009_6010 => verdict.status = "tampered".into(),
                // TRUST_E_PROVIDER_UNKNOWN / CERT_E_UNTRUSTEDROOT / CERT_E_CHAINING 等 → 链问题
                0x800B_0104 | 0x8009_6002 => {
                    verdict.status = "tampered".into();
                    verdict.detail = format!("证书链不完整 / chain error 0x{code:08X}");
                }
                _ => {
                    verdict.status = "tampered".into();
                    verdict.detail = format!("WinVerifyTrust 0x{code:08X}");
                }
            }
        }

        // 签名者与有效期（有签名才查）
        if verdict.status == "valid" || verdict.status == "expired" {
            let (signer, vf, vt) = query_signer(&wpath);
            verdict.signer = signer;
            verdict.valid_from = vf;
            verdict.valid_to = vt;
        }
        verdict
    }
}

/// 从 PKCS7 签名块提取首个签名者证书的 CN 与有效期（尽力而为，失败不致命）。
#[cfg(windows)]
unsafe fn query_signer(wpath: &[u16]) -> (String, u64, u64) {
    use windows::Win32::Security::Cryptography::{
        CertCloseStore, CertEnumCertificatesInStore, CertFreeCertificateContext, CertGetNameStringW,
        CryptQueryObject, CERT_CONTEXT, CERT_NAME_SIMPLE_DISPLAY_TYPE,
        CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED, CERT_QUERY_FORMAT_FLAG_BINARY,
        CERT_QUERY_OBJECT_FILE,
    };

    let mut hstore = windows::Win32::Security::Cryptography::HCERTSTORE::default();
    let mut hmsg = std::ptr::null_mut::<core::ffi::c_void>();
    let content_flags = CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED;
    let r = CryptQueryObject(
        CERT_QUERY_OBJECT_FILE,
        wpath.as_ptr() as *const core::ffi::c_void,
        content_flags,
        CERT_QUERY_FORMAT_FLAG_BINARY,
        0,
        None,
        None,
        None,
        Some(&mut hstore),
        Some(&mut hmsg),
        None,
    );
    if r.is_err() {
        return (String::new(), 0, 0);
    }

    let mut signer = String::new();
    let mut vf = 0u64;
    let mut vt = 0u64;
    // 遍历 store 内证书，取首个有 CN 的（PKCS7 store 首张通常是签名者）
    let mut prev: Option<*const CERT_CONTEXT> = None;
    for _ in 0..8 {
        let ctx = CertEnumCertificatesInStore(hstore, prev);
        if ctx.is_null() {
            break;
        }
        let mut buf = [0u16; 256];
        let n = CertGetNameStringW(
            ctx,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            None,
            Some(&mut buf),
        );
        if n > 0 {
            let s = String::from_utf16_lossy(&buf[..(n as usize).saturating_sub(1)]);
            if !s.is_empty() {
                signer = s;
                let info = (*ctx).pCertInfo;
                if !info.is_null() {
                    vf = ft_to_ms((*info).NotBefore);
                    vt = ft_to_ms((*info).NotAfter);
                }
                CertFreeCertificateContext(Some(ctx));
                break;
            }
        }
        prev = Some(ctx);
    }
    // 释放最后一张未释放的上下文（CertEnumCertificatesInStore 转移了所有权）
    if let Some(p) = prev {
        let _ = CertFreeCertificateContext(Some(p));
    }
    let _ = CertCloseStore(hstore, 0);
    (signer, vf, vt)
}

/// FILETIME（100ns 自 1601）→ UNIX ms。
#[cfg(windows)]
fn ft_to_ms(ft: windows::Win32::Foundation::FILETIME) -> u64 {
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
    let v = (ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64;
    v.saturating_sub(EPOCH_DIFF_100NS) / 10_000
}

#[cfg(not(windows))]
fn verify_file(p: &Path) -> TrustVerdict {
    TrustVerdict {
        status: if p.is_file() { "unsupported".into() } else { "error".into() },
        signer: String::new(),
        valid_from: 0,
        valid_to: 0,
        chain_ok: false,
        detail: "仅 Windows 支持 Authenticode 验证 / Windows-only".into(),
    }
}

// ---------- 命令 ----------

/// 内部实现（登记 + 验证；测试直连）。
pub fn trust_register_inner(
    st: &AppState,
    path: &str,
    origin: &str,
) -> CmdResult<TrustEntry> {
    let p = PathBuf::from(path);
    if !p.is_file() {
        return Err(AppError::not_found(format!("文件不存在 / not found: {path}")));
    }
    if !matches!(origin, "drag" | "download" | "copy" | "install") {
        return Err(AppError::validation(format!("未知来源 / unknown origin: {origin}")));
    }
    let key = norm_key(path);
    let mut idx = load(st);
    let verdict = verify_file(&p);
    let entry = TrustEntry {
        path: key.clone(),
        display: path.to_string(),
        origin: origin.to_string(),
        first_seen: idx.entries.get(&key).map(|e| e.first_seen).unwrap_or_else(now_ms),
        last_verified: now_ms(),
        verdict: Some(verdict),
    };
    idx.entries.insert(key, entry.clone());
    save(st, &idx)?;
    Ok(entry)
}

/// 登记 + 立即验证一个可执行文件（登记任何 exe 时自动调用）。
#[tauri::command(async)]
pub fn trust_register(
    st: tauri::State<AppState>,
    path: String,
    origin: Option<String>,
) -> CmdResult<TrustEntry> {
    trust_register_inner(&st, &path, &origin.unwrap_or_else(|| "copy".into()))
}

/// 验证任意文件（不登记）。
#[tauri::command(async)]
pub fn trust_verify(path: String) -> CmdResult<TrustVerdict> {
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(AppError::not_found(format!("文件不存在 / not found: {path}")));
    }
    Ok(verify_file(&p))
}

/// 信任视图：全部登记条目 + 签名者熟识度聚合。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustWall {
    pub entries: Vec<TrustEntry>,
    /// 签名者 → 登记次数（≥2 视为「熟识」）
    pub signer_counts: std::collections::BTreeMap<String, usize>,
}

pub fn trust_wall_inner(st: &AppState) -> CmdResult<TrustWall> {
    let idx = load(st);
    let mut signer_counts = std::collections::BTreeMap::new();
    for e in idx.entries.values() {
        if let Some(v) = &e.verdict {
            if !v.signer.is_empty() {
                *signer_counts.entry(v.signer.clone()).or_insert(0) += 1;
            }
        }
    }
    Ok(TrustWall { entries: idx.entries.values().cloned().collect(), signer_counts })
}

#[tauri::command(async)]
pub fn trust_wall(st: tauri::State<AppState>) -> CmdResult<TrustWall> {
    trust_wall_inner(&st)
}

/// 一键重新验证全部（8 路并发）。返回更新后的信任墙。
#[tauri::command(async)]
pub fn trust_reverify_all(st: tauri::State<AppState>) -> CmdResult<TrustWall> {
    let mut idx = load(&st);
    let items: Vec<(String, String)> =
        idx.entries.values().map(|e| (e.path.clone(), e.display.clone())).collect();
    let mut results = Vec::with_capacity(items.len());
    if !items.is_empty() {
        let chunk = (items.len() / 8).max(1);
        let mut handles = Vec::new();
        for ch in items.chunks(chunk) {
            let ch: Vec<(String, String)> = ch.to_vec();
            handles.push(std::thread::spawn(move || {
                ch.into_iter()
                    .map(|(k, d)| (k, verify_file(Path::new(&d))))
                    .collect::<Vec<_>>()
            }));
        }
        for h in handles {
            if let Ok(list) = h.join() {
                results.extend(list);
            }
        }
    }
    let now = now_ms();
    for (k, v) in results {
        if let Some(e) = idx.entries.get_mut(&k) {
            e.last_verified = now;
            e.verdict = Some(v);
        }
    }
    save(&st, &idx)?;
    trust_wall_inner(&st)
}

/// 移除登记条目（不动文件本身）。
#[tauri::command(async)]
pub fn trust_remove(st: tauri::State<AppState>, path: String) -> CmdResult<()> {
    let mut idx = load(&st);
    idx.entries.remove(&norm_key(&path));
    save(&st, &idx)
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let tmp = std::env::temp_dir().join(format!("variable-trust-{tag}-{}", std::process::id()));
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        (st, tmp)
    }

    #[test]
    fn register_and_wall_roundtrip() {
        let (st, tmp) = temp_state("wall");
        let exe = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
        let notepad = format!("{exe}\\notepad.exe");
        if !Path::new(&notepad).is_file() {
            let _ = fs::remove_dir_all(&tmp);
            return;
        }
        let e = trust_register_inner(&st, &notepad, "install").unwrap();
        assert!(
            matches!(
                e.verdict.unwrap().status.as_str(),
                "valid" | "unsigned" | "expired" | "tampered" | "unsupported"
            )
        );
        let wall = trust_wall_inner(&st).unwrap();
        assert_eq!(wall.entries.len(), 1);
        // 重复登记同一路径 → 保留首次时间
        let e2 = trust_register_inner(&st, &notepad, "copy").unwrap();
        assert_eq!(e2.first_seen, e.first_seen);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn verify_missing_file_errors() {
        assert_eq!(verify_file(Path::new("Z:/definitely-not-exist-xyz.exe")).status, "error");
    }

    #[test]
    fn register_rejects_missing_and_bad_origin() {
        let (st, tmp) = temp_state("reject");
        assert!(trust_register_inner(&st, "Z:/nope.exe", "drag").is_err());
        let p = tmp.join("x.exe");
        fs::write(&p, b"MZ fake").unwrap();
        assert!(trust_register_inner(&st, p.to_str().unwrap(), "teleport").is_err());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn norm_key_lowercases_and_slashes() {
        assert_eq!(norm_key(r"D:\Foo\Bar.EXE"), "d:/foo/bar.exe");
    }
}
