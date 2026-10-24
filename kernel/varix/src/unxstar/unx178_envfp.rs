//! unx178_envfp — UNX-F0178 EFI 环境指纹（机型/固件版本）（0x6B50 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0178）：EFI 环境指纹（机型/固件版本）。
//! 判据原文（UNX-F0178-J1）：固件厂商/版本/机型指纹入 BootManifest，与
//! SMBIOS 交叉一致。
//!
//! 设计三线：①环境指纹采集账（厂商/版本/机型/发布日期四字段从 EFI 系统表
//! 与 SMBIOS Type1/Type0 双路采集，逐字段双源比对）；②指纹合成（四字段
//! 依序拼接做 FNV-1a 64 指纹，Per-Boot 可复现——同环境同指纹）；③
//! BootManifest 写入账（指纹、双源原始值、比对结论三要素齐才许入册，
//! 缺一即拒——防半截数据进册）。

use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const ENVFP_VERSION: &str = "178-envfp-v1";

/// 指纹段基（0x6B50）。
pub const SEG_BASE: u16 = 0x6B50;

// ---------------------------------------------------------------------------
// 双源环境字段
// ---------------------------------------------------------------------------

/// 环境字段四键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvKey {
    /// 固件厂商（EFI 系统表 Vendor / SMBIOS Type0 Vendor）。
    Vendor,
    /// 固件版本（SMBIOS Type0 Version / EFI 系统表 FirmwareVersion）。
    Version,
    /// 机型（SMBIOS Type1 Product Name）。
    Product,
    /// 发布日期（SMBIOS Type0 Release Date，mm/dd/yyyy）。
    ReleaseDate,
}

impl EnvKey {
    /// 键名字面量（指纹拼接序 = 本枚举声明序）。
    pub fn key(self) -> &'static str {
        match self {
            EnvKey::Vendor => "vendor",
            EnvKey::Version => "version",
            EnvKey::Product => "product",
            EnvKey::ReleaseDate => "release",
        }
    }
}

/// 双源采集值：EFI 路与 SMBIOS 路各一份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DualSource {
    pub key: EnvKey,
    /// EFI 侧原始值。
    pub efi: String,
    /// SMBIOS 侧原始值。
    pub smbios: String,
}

impl DualSource {
    /// 双源一致（判据 J1 的交叉一致主体）。
    pub fn consistent(&self) -> bool {
        self.efi == self.smbios
    }
    /// 一致值（不一致返回 None——不一致时禁止取值入册）。
    pub fn agreed(&self) -> Option<&str> {
        if self.consistent() {
            Some(&self.smbios)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// FNV-1a 64 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 64 单字节。
fn fnv1a_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 环境指纹合成：四键依声明序 "key=value" 拼接后 FNV-1a。
/// 同环境必同指纹（可复现红线）；缺任一键即拒（半截指纹禁出）。
pub fn env_fingerprint(sources: &[DualSource]) -> Result<u64, u16> {
    for k in [
        EnvKey::Vendor,
        EnvKey::Version,
        EnvKey::Product,
        EnvKey::ReleaseDate,
    ] {
        if !sources.iter().any(|s| s.key == k) {
            return Err(E_MISSING_KEY);
        }
    }
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for k in [
        EnvKey::Vendor,
        EnvKey::Version,
        EnvKey::Product,
        EnvKey::ReleaseDate,
    ] {
        let s = sources.iter().find(|s| s.key == k);
        let s = match s {
            Some(s) => s,
            None => return Err(E_MISSING_KEY),
        };
        let v = match s.agreed() {
            Some(v) => v,
            None => return Err(E_SOURCE_MISMATCH),
        };
        for b in k.key().as_bytes() {
            h = fnv1a_byte(h, *b);
        }
        h = fnv1a_byte(h, b'=');
        for b in v.as_bytes() {
            h = fnv1a_byte(h, *b);
        }
        h = fnv1a_byte(h, b'\n');
    }
    Ok(h)
}

// ---------------------------------------------------------------------------
// BootManifest 入册账
// ---------------------------------------------------------------------------

/// BootManifest 环境指纹条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// 合成指纹。
    pub fingerprint: u64,
    /// 双源原始值快照（一致值）。
    pub agreed_values: Vec<(EnvKey, String)>,
    /// 双源全一致标记。
    pub all_consistent: bool,
}

/// BootManifest 指纹区。
#[derive(Debug, Default)]
pub struct BootManifest {
    entries: Vec<ManifestEntry>,
    /// 入册账（写入序号 → 指纹）。
    ledger: Vec<(u32, u64)>,
    seq: u32,
}

impl BootManifest {
    /// 空册。
    pub fn new() -> Self {
        BootManifest {
            entries: Vec::new(),
            ledger: Vec::new(),
            seq: 0,
        }
    }

    /// 入册：双源全一致才许写（缺键/不一致都拒），写后账平。
    pub fn record(&mut self, sources: &[DualSource]) -> Result<u64, u16> {
        let fp = env_fingerprint(sources)?;
        let all_ok = sources.iter().all(|s| s.consistent());
        if !all_ok {
            return Err(E_SOURCE_MISMATCH);
        }
        let agreed = sources
            .iter()
            .map(|s| (s.key, s.smbios.clone()))
            .collect();
        self.entries.push(ManifestEntry {
            fingerprint: fp,
            agreed_values: agreed,
            all_consistent: true,
        });
        self.seq += 1;
        self.ledger.push((self.seq, fp));
        Ok(fp)
    }

    /// 复现校验：同环境二次采集指纹必须与在册一致（Per-Boot 红线）。
    pub fn reproducible(&self, sources: &[DualSource]) -> Result<bool, u16> {
        let fp = env_fingerprint(sources)?;
        Ok(self.entries.last().map(|e| e.fingerprint) == Some(fp))
    }

    /// 册账平：条目数 ≡ 台账数。
    pub fn balanced(&self) -> bool {
        self.entries.len() == self.ledger.len()
    }

    /// 在册条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// SMBIOS 字符串区还原（只读）
// ---------------------------------------------------------------------------

/// SMBIOS 字符串区还原：NUL 分隔、双 NUL 收尾，返回第 i 个（1 起）。
pub fn smbios_string(buf: &[u8], index: u8) -> Option<String> {
    if index == 0 {
        return None;
    }
    let mut cur = 1u8;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < buf.len() {
        if buf[i] == 0 {
            if cur == index {
                return Some(String::from_utf8_lossy(&buf[start..i]).into_owned());
            }
            // 双 NUL：字符串区结束。
            if i + 1 < buf.len() && buf[i + 1] == 0 {
                return None;
            }
            cur += 1;
            start = i + 1;
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 错误码（0x6B50 段，显式判别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum EnvFpErr {
    /// 缺环境键。
    MissingKey = SEG_BASE + 1,
    /// 双源不一致。
    SourceMismatch = SEG_BASE + 2,
    /// 册账不平。
    LedgerUnbalanced = SEG_BASE + 3,
    /// 复现失败。
    NotReproducible = SEG_BASE + 4,
    /// SMBIOS 字符串越界。
    SmBiosIndex = SEG_BASE + 5,
}

impl EnvFpErr {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            EnvFpErr::MissingKey => "环境键缺失（四键必须齐）",
            EnvFpErr::SourceMismatch => "EFI 与 SMBIOS 双源不一致",
            EnvFpErr::LedgerUnbalanced => "条目数与台账数不平",
            EnvFpErr::NotReproducible => "同环境二次指纹与在册不一致",
            EnvFpErr::SmBiosIndex => "SMBIOS 字符串下标越界",
        }
    }
}

const E_MISSING_KEY: u16 = EnvFpErr::MissingKey.code();
const E_SOURCE_MISMATCH: u16 = EnvFpErr::SourceMismatch.code();
const E_LEDGER_UNBALANCED: u16 = EnvFpErr::LedgerUnbalanced.code();
const E_NOT_REPRODUCIBLE: u16 = EnvFpErr::NotReproducible.code();
const E_SMBIOS_INDEX: u16 = EnvFpErr::SmBiosIndex.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 判据夹具：双源一致的四键环境（QEMU 标准环境）。
fn fixture_sources() -> Vec<DualSource> {
    vec![
        DualSource {
            key: EnvKey::Vendor,
            efi: String::from("EDK II"),
            smbios: String::from("EDK II"),
        },
        DualSource {
            key: EnvKey::Version,
            efi: String::from("0.0.0"),
            smbios: String::from("0.0.0"),
        },
        DualSource {
            key: EnvKey::Product,
            efi: String::from("Varix Virtual Machine"),
            smbios: String::from("Varix Virtual Machine"),
        },
        DualSource {
            key: EnvKey::ReleaseDate,
            efi: String::from("01/01/2026"),
            smbios: String::from("01/01/2026"),
        },
    ]
}

/// 判据集（UNX-F0178-J1 + 结构红线）。自证五段。
pub fn self_audit() -> Result<(), u16> {
    // 一、J1 主体：指纹入册成功 + 双源交叉一致。
    let mut m = BootManifest::new();
    let src = fixture_sources();
    let fp = m.record(&src)?;
    if !m.balanced() {
        return Err(E_LEDGER_UNBALANCED);
    }
    // 二、Per-Boot 可复现：同环境二次采集指纹一致。
    if !m.reproducible(&src)? {
        return Err(E_NOT_REPRODUCIBLE);
    }
    // 三、环境变即指纹变：改机型后指纹必然不同（防恒指纹）。
    let mut src2 = fixture_sources();
    src2[2].efi = String::from("Varix Virtual Machine II");
    src2[2].smbios = String::from("Varix Virtual Machine II");
    let fp2 = env_fingerprint(&src2)?;
    if fp2 == fp {
        return Err(E_NOT_REPRODUCIBLE);
    }
    // 四、缺键拒 + 双源不一致拒（半截数据禁入册）。
    let partial = vec![src[0].clone(), src[1].clone()];
    if env_fingerprint(&partial).is_ok() {
        return Err(E_MISSING_KEY);
    }
    let mut src3 = fixture_sources();
    src3[1].smbios = String::from("9.9.9");
    if env_fingerprint(&src3).is_ok() {
        return Err(E_SOURCE_MISMATCH);
    }
    // 五、SMBIOS 字符串区还原：第 2 串命中、越界返回 None、下标 0 恒 None。
    let area = b"EDK II\0Varix Virtual Machine\0";
    match smbios_string(area, 2) {
        Some(s) => {
            if s != "Varix Virtual Machine" {
                return Err(E_SMBIOS_INDEX);
            }
        }
        None => return Err(E_SMBIOS_INDEX),
    }
    if smbios_string(area, 3).is_some() || smbios_string(area, 0).is_some() {
        return Err(E_SMBIOS_INDEX);
    }
    // 六、判别式段位对拍。
    if E_NOT_REPRODUCIBLE != SEG_BASE + 4 || E_SMBIOS_INDEX != SEG_BASE + 5 {
        return Err(E_NOT_REPRODUCIBLE);
    }
    Ok(())
}
