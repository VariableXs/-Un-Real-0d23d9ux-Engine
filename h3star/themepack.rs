//! F305 设置导入导出 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：导出内容白名单审计（凭据/文件路径=0）；跨版本导入
//! 用例（v1 包在新版读）；校验失败拒载；E 域 F134 分享页同格式互通。
//!
//! **设计要点（主册）**：
//! - 整套个性化配置（主题令牌+壁纸+磁贴布局+快捷键表+常用设置）一键
//!   导出为 .vxtheme 文件（E 域定制包同格式），换机/重装后一键导入
//!   还原；
//! - 导出文件自描述（含版本号与校验），跨版本导入向下兼容（新系统读
//!   旧包不炸）；
//! - 隐私红线写死：设置包不含文件、历史、凭据——只含偏好。
//!
//! 格式（F134 分享页互通的单一格式源）：
//! `VXTH|<version>|<n 条目>|k=v,k=v,...|<FNV-1a 校验>`
//! 键白名单（五类偏好面）+ 值面审计（无路径分隔符/无凭据样键）。

use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 当前格式版本。
pub const FORMAT_VERSION: u32 = 1;

/// 魔数（自描述头）。
pub const MAGIC: &str = "VXTH";

/// 键白名单（五类偏好面——白名单外键导出即拒）。
pub const KEY_WHITELIST: [&str; 5] =
    ["theme_tokens", "wallpaper_ref", "tile_layout", "hotkey_table", "common_prefs"];

// ---------------------------------------------------------------------------
// 包
// ---------------------------------------------------------------------------

/// 一个偏好条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefEntry {
    pub key: &'static str,
    pub value: String,
}

/// 设置包（内存形态）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrefPack {
    pub version: u32,
    pub entries: Vec<PrefEntry>,
}

/// 导出拒绝原因（审计诚实性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportReject {
    /// 键不在白名单。
    KeyOffWhitelist,
    /// 值疑似文件路径（含路径分隔符）。
    ValueLooksLikePath,
    /// 值疑似凭据（键名含凭据样词）。
    CredentialLikeKey,
    /// 重复键。
    DuplicateKey,
}

impl ExportReject {
    pub fn label(self) -> &'static str {
        match self {
            ExportReject::KeyOffWhitelist => "键不在偏好白名单",
            ExportReject::ValueLooksLikePath => "值疑似文件路径（设置包只含偏好）",
            ExportReject::CredentialLikeKey => "键疑似凭据（设置包不含凭据）",
            ExportReject::DuplicateKey => "重复键",
        }
    }
}

/// 值面审计：路径样值（含 `/` 或 `\` 或盘符样 `X:`）拒绝。
fn value_looks_like_path(v: &str) -> bool {
    v.contains('/') || v.contains('\\') || {
        // 盘符样：前两字符为大写字母+冒号。
        let b: Vec<char> = v.chars().take(2).collect();
        b.len() == 2 && b[0].is_ascii_uppercase() && b[1] == ':'
    }
}

/// 键面审计：凭据样词（auth/secret/password/credential/api_key 语义——
/// 「theme_tokens」这类偏好键的 token 字样不误伤）。
fn credential_like_key(k: &str) -> bool {
    let l = k.to_ascii_lowercase();
    l.contains("secret")
        || l.contains("password")
        || l.contains("credential")
        || l.contains("auth")
        || l.contains("api_key")
        || l.contains("session_key")
}

/// 导出审计：逐条目白名单/路径/凭据/重复四查（全过才准出包）。
pub fn audit_export(entries: &[PrefEntry]) -> Result<(), (usize, ExportReject)> {
    let mut seen: Vec<&str> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        if !KEY_WHITELIST.contains(&e.key) {
            return Err((i, ExportReject::KeyOffWhitelist));
        }
        if credential_like_key(e.key) {
            return Err((i, ExportReject::CredentialLikeKey));
        }
        if value_looks_like_path(&e.value) {
            return Err((i, ExportReject::ValueLooksLikePath));
        }
        if seen.contains(&e.key) {
            return Err((i, ExportReject::DuplicateKey));
        }
        seen.push(e.key);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 序列化（FNV-1a 校验——与 star/recenteng 同族口径）
// ---------------------------------------------------------------------------

/// FNV-1a 32 位。
fn fnv1a(data: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in data.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 序列化（自描述：魔数+版本+条目数+条目+校验）。
pub fn serialize(pack: &PrefPack) -> String {
    let mut body = String::new();
    for (i, e) in pack.entries.iter().enumerate() {
        if i > 0 {
            body.push(',');
        }
        body.push_str(e.key);
        body.push('=');
        body.push_str(&e.value);
    }
    let check = fnv1a(&body) ^ pack.version;
    let _ = core::format_args!("");
    let mut out = String::new();
    out.push_str(MAGIC);
    out.push('|');
    out.push_str(&itoa(pack.version));
    out.push('|');
    out.push_str(&itoa(pack.entries.len() as u32));
    out.push('|');
    out.push_str(&body);
    out.push('|');
    out.push_str(&itoa(check));
    out
}

/// 反序列化错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// 魔数不对（不是 vxtheme 包）。
    BadMagic,
    /// 结构段数不对。
    Malformed,
    /// 校验和不匹配（损坏/篡改——拒载）。
    ChecksumMismatch,
    /// 版本比本系统新（诚实降级——需要升级到 vX）。
    VersionTooNew,
    /// 包体条目键越白名单（防外带凭据混入）。
    KeyOffWhitelist,
}

impl ImportError {
    pub fn label(self) -> &'static str {
        match self {
            ImportError::BadMagic => "不是 .vxtheme 设置包",
            ImportError::Malformed => "包结构损坏",
            ImportError::ChecksumMismatch => "校验失败——包已损坏或被改动",
            ImportError::VersionTooNew => "包来自更新版本的系统，请先升级",
            ImportError::KeyOffWhitelist => "包内含白名单外的键，拒绝载入",
        }
    }
}

/// 反序列化（校验失败拒载；跨版本向下兼容：<= 当前版本可读）。
pub fn deserialize(text: &str) -> Result<PrefPack, ImportError> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.is_empty() || parts[0] != MAGIC {
        return Err(ImportError::BadMagic);
    }
    if parts.len() != 5 {
        return Err(ImportError::Malformed);
    }
    let version = parse_u32(parts[1]).ok_or(ImportError::Malformed)?;
    let count = parse_u32(parts[2]).ok_or(ImportError::Malformed)?;
    let body = parts[3];
    let check = parse_u32(parts[4]).ok_or(ImportError::Malformed)?;
    if fnv1a(body) ^ version != check {
        return Err(ImportError::ChecksumMismatch);
    }
    if version > FORMAT_VERSION {
        return Err(ImportError::VersionTooNew);
    }
    let mut entries: Vec<PrefEntry> = Vec::new();
    if count > 0 {
        for kv in body.split(',') {
            let mut it = kv.splitn(2, '=');
            let k = it.next().ok_or(ImportError::Malformed)?;
            let v = it.next().ok_or(ImportError::Malformed)?;
            if !KEY_WHITELIST.contains(&k) {
                return Err(ImportError::KeyOffWhitelist);
            }
            entries.push(PrefEntry {
                key: KEY_WHITELIST.iter().find(|w| **w == k).unwrap(),
                value: String::from(v),
            });
        }
    }
    if entries.len() as u32 != count {
        return Err(ImportError::Malformed);
    }
    Ok(PrefPack { version, entries })
}

/// 极简 u32 转十进制（no_std 无 itoa 依赖）。
fn itoa(mut v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    String::from_utf8_lossy(&buf[i..]).to_string()
}

/// 极简十进制解析。
fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    for c in s.chars() {
        let d = c.to_digit(10)?;
        acc = acc.checked_mul(10)?.checked_add(d)?;
    }
    Some(acc)
}

// ---------------------------------------------------------------------------
// 分享页互通（F134 同格式——人话摘要出账）
// ---------------------------------------------------------------------------

/// 分享页摘要（同人话：包里有什么、多少条、多新）。
pub fn share_summary(pack: &PrefPack) -> String {
    let mut s = String::from("个性化设置包 · ");
    s.push_str(&itoa(pack.entries.len() as u32));
    s.push_str(" 项偏好 · 格式 v");
    s.push_str(&itoa(pack.version));
    s
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 合法演示包（自检与单测共用）。
pub fn demo_pack() -> PrefPack {
    PrefPack {
        version: FORMAT_VERSION,
        entries: alloc::vec![
            PrefEntry { key: "theme_tokens", value: String::from("accent=teal;dark=1") },
            PrefEntry { key: "wallpaper_ref", value: String::from("aurora-default") },
            PrefEntry { key: "tile_layout", value: String::from("grid=4x3;pinned=8") },
            PrefEntry { key: "hotkey_table", value: String::from("snap=win+arrows") },
            PrefEntry { key: "common_prefs", value: String::from("volume=50;brightness=70") },
        ],
    }
}

/// F305 自检（判据：白名单审计；跨版本；校验拒载；F134 同格式互通）。
pub fn run_themepack_checks() -> CheckSet {
    let mut set = CheckSet::new("F305-themepack");

    // 1. 白名单外键拒绝（导出面）。
    let rogue = PrefEntry { key: "browser_history", value: String::from("many") };
    let mut bad = demo_pack();
    bad.entries.push(rogue);
    set.add(
        "export rejects off-whitelist key",
        audit_export(&bad.entries) == Err((5, ExportReject::KeyOffWhitelist)),
        "",
    );

    // 2. 路径样值拒绝（隐私红线：不含文件）。
    let pathful = PrefEntry { key: "wallpaper_ref", value: String::from("C:/Users/me/x.jpg") };
    set.add(
        "export rejects file path value",
        audit_export(&[pathful]).is_err(),
        "",
    );

    // 3. 凭据样键拒绝（token 样）。
    let cred = PrefEntry { key: "auth_token", value: String::from("abc") };
    set.add(
        "export rejects credential-like key",
        audit_export(&[cred]).is_err(),
        "",
    );

    // 4. 合法包：审计通过 + round-trip 保真。
    let pack = demo_pack();
    set.add("export clean audit", audit_export(&pack.entries).is_ok(), "");
    let text = serialize(&pack);
    let back = deserialize(&text).unwrap();
    set.add("roundtrip faithful", back == pack, "");

    // 5. 校验失败拒载（改一位校验）。
    let mut tampered = text.clone();
    let last = tampered.pop().unwrap();
    tampered.push(if last == '9' { '0' } else { char::from_u32(last as u32 + 1).unwrap_or('0') });
    set.add(
        "checksum mismatch rejects",
        matches!(deserialize(&tampered), Err(ImportError::ChecksumMismatch)),
        "",
    );

    // 6. 跨版本导入：v1 包在（假想）v2 系统读——FORMAT_VERSION 兼容面。
    let v1_text = serialize(&PrefPack { version: 1, entries: pack.entries.clone() });
    set.add(
        "v1 pack readable while current is v1",
        matches!(deserialize(&v1_text), Ok(_)),
        "",
    );
    // 更新版本包诚实降级（需要升级提示）。
    let v99_text = serialize(&PrefPack { version: 99, entries: pack.entries.clone() });
    set.add(
        "newer pack asks for upgrade",
        matches!(deserialize(&v99_text), Err(ImportError::VersionTooNew)),
        "",
    );

    // 7. 非本格式文件拒载（BadMagic——不炸不误读）。
    set.add(
        "foreign file rejected",
        matches!(deserialize("某软件配置文件|1|0||0"), Err(ImportError::BadMagic)),
        "",
    );

    // 8. F134 同格式互通：分享页摘要与包一致（同人话出账）。
    let s = share_summary(&pack);
    set.add(
        "share page same format summary",
        s.contains("5 项偏好") && s.contains("格式 v1"),
        "",
    );

    // 9. 导入端白名单复核（外带包塞凭据键 → KeyOffWhitelist 拒载）。
    let sneaky = PrefPack {
        version: 1,
        entries: alloc::vec![PrefEntry { key: "secret_key", value: String::from("x") }],
    };
    set.add(
        "import re-audits whitelist",
        matches!(deserialize(&serialize(&sneaky)), Err(ImportError::KeyOffWhitelist)),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pack_roundtrip() {
        let pack = PrefPack { version: FORMAT_VERSION, entries: Vec::new() };
        assert_eq!(deserialize(&serialize(&pack)).unwrap(), pack);
    }

    #[test]
    fn fnv1a_known_vector() {
        // FNV-1a("a") = 0xe40c292c（公开测试向量）。
        assert_eq!(fnv1a("a"), 0xe40c_292c);
    }

    #[test]
    fn drive_letter_value_rejected() {
        assert!(value_looks_like_path("D:\\x"));
        assert!(!value_looks_like_path("grid=4x3"));
    }

    #[test]
    fn malformed_segment_count_rejected() {
        assert!(matches!(deserialize("VXTH|1|0"), Err(ImportError::Malformed)));
    }

    #[test]
    fn reject_reasons_human_readable() {
        assert!(!ExportReject::ValueLooksLikePath.label().is_empty());
        assert!(!ImportError::ChecksumMismatch.label().is_empty());
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · 主题包导出迁移（十四章开放性的主题域落法）
// ---------------------------------------------------------------------------

/// 主题包导出迁移（配置可备份可迁移的主题面）：主题键值 → 人话行
/// 导出；导入校验（键白名单 + 值非空）；round-trip 可迁移证明；
/// 键白名单外拒绝留痕（与调速器配置快照同纪律）。
pub struct ThemePackExport {
    pub entries: Vec<(&'static str, String)>,
}

/// 主题键白名单。
pub const THEME_KEYS: [&str; 5] = ["accent", "wallpaper", "font_scale", "dark_mode", "transparency"];

impl ThemePackExport {
    pub fn capture(entries: Vec<(&'static str, String)>) -> ThemePackExport {
        ThemePackExport { entries }
    }

    pub fn export(&self) -> String {
        self.entries
            .iter()
            .map(|(k, v)| alloc::format!("{}={}\n", k, v))
            .collect()
    }

    /// 导入解析：键白名单 + 值非空双闸；拒绝留痕。
    pub fn import(text: &str) -> (Vec<(&'static str, String)>, Vec<String>) {
        let mut ok = Vec::new();
        let mut rejected = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match line.split_once('=') {
                Some((k, v)) if THEME_KEYS.contains(&k) && !v.is_empty() => {
                    let key = THEME_KEYS.iter().find(|c| **c == k).unwrap();
                    ok.push((*key, String::from(v)));
                }
                _ => rejected.push(String::from(line)),
            }
        }
        (ok, rejected)
    }

    /// round-trip 自证。
    pub fn round_trip(&self) -> bool {
        let (parsed, rej) = Self::import(&self.export());
        rej.is_empty() && parsed == self.entries
    }
}

/// 深化层二自检（主题包迁移）。
pub fn run_themepack_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F305-deep2");

    // 1. 导出人话行。
    let pack = ThemePackExport::capture(alloc::vec![
        ("accent", alloc::string::String::from("vx-blue")),
        ("dark_mode", alloc::string::String::from("on")),
    ]);
    set.add(
        "theme export human",
        pack.export().contains("accent=vx-blue") && pack.export().contains("dark_mode=on"),
        "",
    );

    // 2. round-trip 可迁移。
    set.add("theme round trip", pack.round_trip(), "");

    // 3. 导入防呆：未知键、空值拒绝留痕；空行跳过。
    let (_, rej) = ThemePackExport::import("evil_key=1\naccent=\n\naccent=vx-red\n");
    set.add(
        "theme import guards",
        rej.len() == 2 && rej[0] == "evil_key=1" && rej[1] == "accent=",
        "",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn theme_keys_pinned() {
        assert_eq!(THEME_KEYS.len(), 5, "主题键白名单五键钉死");
    }

    #[test]
    fn empty_pack_round_trip() {
        let pack = ThemePackExport::capture(alloc::vec::Vec::new());
        assert!(pack.round_trip(), "空主题包往返平凡绿");
    }
}
