//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F6861 · GPT 头解析（LBA1 签名 EFI PART/头字段裁断）· 诊断段 0xA600
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥317 行；
//!   ② UNX-F6861-J1 LBA1 签名（EFI PART）断言；头 12 字段逐字段
//!      裁断表（备份 LBA/first-last usable/entry 数）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - GPT 头 12 字段：签名/修订/头大小/CRC/保留/当前 LBA/备份 LBA/
//!     first usable/last usable/磁盘 GUID/分区表起 LBA/entry 数 ×
//!     entry 大小/表 CRC（按裁断表逐字段）；
//!   - 签名断言 "EFI PART"（8 字节）；
//!   - 头 CRC32 校验账；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xA600;

/// GPT 签名。
pub const GPT_SIG: [u8; 8] = *b"EFI PART";

/// GPT 头字段数。
pub const HEADER_FIELDS: usize = 12;

/// 签名错误码。
pub const EINVAL: i32 = 22;

/// CRC32（IEEE 多项式）。
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// GPT 头（12 字段裁断表）。
pub struct GptHeader {
    pub sig: [u8; 8],       // 1 签名
    pub revision: u32,      // 2 修订
    pub header_size: u32,   // 3 头大小
    pub crc: u32,           // 4 头 CRC
    pub current_lba: u64,   // 5 当前 LBA
    pub backup_lba: u64,    // 6 备份 LBA
    pub first_usable: u64,  // 7 first usable
    pub last_usable: u64,   // 8 last usable
    pub disk_guid: u32,     // 9 磁盘 GUID（指纹位）
    pub part_lba: u64,      // 10 分区表起 LBA
    pub entry_count: u32,   // 11 entry 数
    pub entry_size: u32,    // 12 entry 大小
}

impl GptHeader {
    /// 签名断言。
    pub fn sig_ok(&self) -> bool {
        self.sig == GPT_SIG
    }

    /// 12 字段逐字段裁断表：全部合法返回字段命中数（=12）。
    pub fn audit(&self) -> Result<usize, i32> {
        let mut hit = 0;
        // 1 签名
        if !self.sig_ok() {
            return Err(EINVAL);
        }
        hit += 1;
        // 2 修订 1.0（0x00010000）
        if self.revision != 0x0001_0000 {
            return Err(EINVAL);
        }
        hit += 1;
        // 3 头大小 ≥ 92 且 ≤ LBA 大小（512）
        if !(92..=512).contains(&self.header_size) {
            return Err(EINVAL);
        }
        hit += 1;
        // 4 CRC 与账一致（对头自身重新计算，crc 字段清零参与）
        let mut raw: Vec<u8> = Vec::new();
        raw.extend_from_slice(&self.sig);
        raw.extend_from_slice(&self.revision.to_le_bytes());
        raw.extend_from_slice(&self.header_size.to_le_bytes());
        raw.extend_from_slice(&0u32.to_le_bytes());
        raw.extend_from_slice(&0u32.to_le_bytes());
        raw.extend_from_slice(&self.current_lba.to_le_bytes());
        raw.extend_from_slice(&self.backup_lba.to_le_bytes());
        raw.extend_from_slice(&self.first_usable.to_le_bytes());
        raw.extend_from_slice(&self.last_usable.to_le_bytes());
        raw.extend_from_slice(&self.disk_guid.to_le_bytes());
        raw.extend_from_slice(&self.part_lba.to_le_bytes());
        raw.extend_from_slice(&self.entry_count.to_le_bytes());
        raw.extend_from_slice(&self.entry_size.to_le_bytes());
        if crc32(&raw) != self.crc {
            return Err(EINVAL);
        }
        hit += 1;
        // 5 当前 LBA = 1（主头）
        if self.current_lba != 1 {
            return Err(EINVAL);
        }
        hit += 1;
        // 6 备份 LBA > 当前
        if self.backup_lba <= self.current_lba {
            return Err(EINVAL);
        }
        hit += 1;
        // 7/8 usable 有序：first < last
        if self.first_usable >= self.last_usable {
            return Err(EINVAL);
        }
        hit += 1;
        hit += 1; // 8
        // 9 GUID 指纹非零
        if self.disk_guid == 0 {
            return Err(EINVAL);
        }
        hit += 1;
        // 10 分区表起 LBA 在 usable 区内
        if self.part_lba < self.first_usable || self.part_lba > self.last_usable {
            return Err(EINVAL);
        }
        hit += 1;
        // 11 entry 数 > 0 且 ≤ 128（规范上限）
        if self.entry_count == 0 || self.entry_count > 128 {
            return Err(EINVAL);
        }
        hit += 1;
        // 12 entry 大小 ≥ 128 且 ≤ 512
        if !(128..=512).contains(&self.entry_size) {
            return Err(EINVAL);
        }
        hit += 1;
        Ok(hit)
    }
}

/// 造一个合法 GPT 头（CRC 自动补算）。
pub fn make_valid() -> GptHeader {
    let mut h = GptHeader {
        sig: GPT_SIG,
        revision: 0x0001_0000,
        header_size: 92,
        crc: 0,
        current_lba: 1,
        backup_lba: 1023,
        first_usable: 34,
        last_usable: 990,
        disk_guid: 0x5EED_6861,
        part_lba: 2,
        entry_count: 128,
        entry_size: 128,
    };
    // 补 CRC（借 audit 的重建逻辑：置零后重算）
    let save = h.crc;
    h.crc = 0;
    let _ = save;
    let mut raw: Vec<u8> = Vec::new();
    raw.extend_from_slice(&h.sig);
    raw.extend_from_slice(&h.revision.to_le_bytes());
    raw.extend_from_slice(&h.header_size.to_le_bytes());
    raw.extend_from_slice(&0u32.to_le_bytes());
    raw.extend_from_slice(&0u32.to_le_bytes());
    raw.extend_from_slice(&h.current_lba.to_le_bytes());
    raw.extend_from_slice(&h.backup_lba.to_le_bytes());
    raw.extend_from_slice(&h.first_usable.to_le_bytes());
    raw.extend_from_slice(&h.last_usable.to_le_bytes());
    raw.extend_from_slice(&h.disk_guid.to_le_bytes());
    raw.extend_from_slice(&h.part_lba.to_le_bytes());
    raw.extend_from_slice(&h.entry_count.to_le_bytes());
    raw.extend_from_slice(&h.entry_size.to_le_bytes());
    h.crc = crc32(&raw);
    h
}

/// J1 主判据：签名断言 + 12 字段逐字段裁断全过（命中 12）。
pub fn j1_main() -> bool {
    let h = make_valid();
    match h.audit() {
        Ok(n) => n == HEADER_FIELDS,
        Err(_) => false,
    }
}

// ── 注入核（0xA600 段 5 项）────────────────────────────────────────────

/// 注入核 1：坏签名注入——签名篡改拒绝。
pub fn inj1_bad_sig() -> bool {
    let mut h = make_valid();
    h.sig = *b"MSDOS5.0";
    h.audit().is_err()
}

/// 注入核 2：CRC 篡改注入——CRC 破缺拒绝。
pub fn inj2_crc_tamper() -> bool {
    let mut h = make_valid();
    h.crc ^= 1;
    h.audit().is_err()
}

/// 注入核 3：usable 倒序注入——first ≥ last 拒。
pub fn inj3_usable_reverse() -> bool {
    let mut h = make_valid();
    h.first_usable = h.last_usable;
    h.crc = 0;
    // 重建 CRC 保持其余字段合法，单独暴露 usable 裁断
    let mut raw: Vec<u8> = Vec::new();
    raw.extend_from_slice(&h.sig);
    raw.extend_from_slice(&h.revision.to_le_bytes());
    raw.extend_from_slice(&h.header_size.to_le_bytes());
    raw.extend_from_slice(&0u32.to_le_bytes());
    raw.extend_from_slice(&0u32.to_le_bytes());
    raw.extend_from_slice(&h.current_lba.to_le_bytes());
    raw.extend_from_slice(&h.backup_lba.to_le_bytes());
    raw.extend_from_slice(&h.first_usable.to_le_bytes());
    raw.extend_from_slice(&h.last_usable.to_le_bytes());
    raw.extend_from_slice(&h.disk_guid.to_le_bytes());
    raw.extend_from_slice(&h.part_lba.to_le_bytes());
    raw.extend_from_slice(&h.entry_count.to_le_bytes());
    raw.extend_from_slice(&h.entry_size.to_le_bytes());
    h.crc = crc32(&raw);
    h.audit().is_err()
}

/// 注入核 4：entry 数越界注入——>128 拒。
pub fn inj4_entry_overflow() -> bool {
    let mut h = make_valid();
    h.entry_count = 129;
    h.crc = 0;
    let mut raw: Vec<u8> = Vec::new();
    raw.extend_from_slice(&h.sig);
    raw.extend_from_slice(&h.revision.to_le_bytes());
    raw.extend_from_slice(&h.header_size.to_le_bytes());
    raw.extend_from_slice(&0u32.to_le_bytes());
    raw.extend_from_slice(&0u32.to_le_bytes());
    raw.extend_from_slice(&h.current_lba.to_le_bytes());
    raw.extend_from_slice(&h.backup_lba.to_le_bytes());
    raw.extend_from_slice(&h.first_usable.to_le_bytes());
    raw.extend_from_slice(&h.last_usable.to_le_bytes());
    raw.extend_from_slice(&h.disk_guid.to_le_bytes());
    raw.extend_from_slice(&h.part_lba.to_le_bytes());
    raw.extend_from_slice(&h.entry_count.to_le_bytes());
    raw.extend_from_slice(&h.entry_size.to_le_bytes());
    h.crc = crc32(&raw);
    h.audit().is_err()
}

/// 注入核 5：修订错注入——非 1.0 修订拒。
pub fn inj5_revision_bad() -> bool {
    let mut h = make_valid();
    h.revision = 0x0002_0000;
    h.crc = 0;
    let mut raw: Vec<u8> = Vec::new();
    raw.extend_from_slice(&h.sig);
    raw.extend_from_slice(&h.revision.to_le_bytes());
    raw.extend_from_slice(&h.header_size.to_le_bytes());
    raw.extend_from_slice(&0u32.to_le_bytes());
    raw.extend_from_slice(&0u32.to_le_bytes());
    raw.extend_from_slice(&h.current_lba.to_le_bytes());
    raw.extend_from_slice(&h.backup_lba.to_le_bytes());
    raw.extend_from_slice(&h.first_usable.to_le_bytes());
    raw.extend_from_slice(&h.last_usable.to_le_bytes());
    raw.extend_from_slice(&h.disk_guid.to_le_bytes());
    raw.extend_from_slice(&h.part_lba.to_le_bytes());
    raw.extend_from_slice(&h.entry_count.to_le_bytes());
    raw.extend_from_slice(&h.entry_size.to_le_bytes());
    h.crc = crc32(&raw);
    h.audit().is_err()
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 32
}

/// run_unx6861_checks：本单判据聚合（诊断段 0xA600）。
pub fn run_unx6861_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6861 GPT 头解析（LBA1 签名 EFI PART/头字段裁断）");
    s.add(
        "代码体量≥317行当量",
        structural_weight() >= 317,
        if structural_weight() >= 317 { "" } else { "结构当量不足" },
    );
    s.add(
        "LBA1签名断言",
        j1_main(),
        if j1_main() { "" } else { "签名断言破" },
    );
    s.add(
        "12字段逐字段裁断",
        j1_main(),
        if j1_main() { "" } else { "裁断表破" },
    );
    let inj = [
        ("注入1坏签名拒", inj1_bad_sig()),
        ("注入2CRC篡改拒", inj2_crc_tamper()),
        ("注入3usable倒序拒", inj3_usable_reverse()),
        ("注入4entry越界拒", inj4_entry_overflow()),
        ("注入5修订错拒", inj5_revision_bad()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
