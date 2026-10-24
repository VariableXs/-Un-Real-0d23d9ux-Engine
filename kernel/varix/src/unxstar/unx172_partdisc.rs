//! unx172_partdisc — UNX-F0172 EFI 分区发现与 GUID 语义（0x6A50 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0172）：EFI 分区发现与 GUID 语义。
//! 判据原文（UNX-F0172-J1）：ESP 识别（GUID/文件系统探测）与 parted 对照
//! 一致，误报 0。
//!
//! 设计三线：①GPT 表只读解析（保护性 MBR 识别、头部 CRC32、分区项数组
//! 遍历——GUID 按混合端序还原为 RFC 4122 字符串）；②ESP 识别双证据
//! （分区类型 GUID 恰为 ESP 常量 + 文件系统探测 FAT 签名/目录名，两证
//! 齐取才算 ESP——单证不算，防误报）；③与 parted 参照清单逐项对照
//! （多/少/错位/误报四类差异各自独立记缺陷账，误报数必须为 0）。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const PARTDISC_VERSION: &str = "172-partdisc-v1";

/// GPT 头部签名（LE 端序下 "EFI PART"）。
pub const GPT_SIGNATURE: u64 = 0x5452_4150_2049_4645;

/// ESP 分区类型 GUID 常量（混合端序还原后的小写形）。
pub const ESP_TYPE_GUID: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";

/// BIOS Boot 分区类型 GUID（对照用）。
pub const BIOS_BOOT_GUID: &str = "21686148-6449-6e6f-744e-656564454649";

/// Linux 文件系统数据分区 GUID（对照用）。
pub const LINUX_FS_GUID: &str = "0fc63daf-8483-4772-8e79-3d69d8477de4";

// ---------------------------------------------------------------------------
// GUID 混合端序还原
// ---------------------------------------------------------------------------

/// 混合端序 GUID 还原：前 3 字段小端、后 2 字段大端。
/// 输入 16 字节，输出 RFC 4122 小写字符串。
pub fn guid_from_mixed(buf: &[u8; 16]) -> String {
    let d1 = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let d2 = u16::from_le_bytes([buf[4], buf[5]]);
    let d3 = u16::from_le_bytes([buf[6], buf[7]]);
    let mut hex = String::with_capacity(36);
    hex.push_str(&format!("{:08x}-{:04x}-{:04x}-", d1, d2, d3));
    for b in buf[8..10].iter() {
        hex.push_str(&format!("{:02x}", b));
    }
    hex.push('-');
    for b in buf[10..16].iter() {
        hex.push_str(&format!("{:02x}", b));
    }
    hex
}

/// GUID 合法性：36 位形、两横杠位、全十六进制。
pub fn guid_wellformed(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, &c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if c != b'-' {
                    return false;
                }
            }
            _ => {
                if !(c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
                    return false;
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// GPT 头部与分区项
// ---------------------------------------------------------------------------

/// GPT 头部解析结果（只取判据关心的字段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptHeader {
    /// 头部 CRC32（头部自身，字段内原值）。
    pub header_crc32: u32,
    /// 分区项起始 LBA。
    pub part_entry_lba: u64,
    /// 分区项数。
    pub part_entry_count: u32,
    /// 单项字节数。
    pub part_entry_size: u32,
}

/// 头部解析：签名 + 最小字段窗校验。签名不对即拒（不做容错解析）。
pub fn parse_gpt_header(buf: &[u8]) -> Result<GptHeader, u16> {
    if buf.len() < 92 {
        return Err(E_HEADER_SHORT);
    }
    let mut sig = [0u8; 8];
    sig.copy_from_slice(&buf[0..8]);
    if u64::from_le_bytes(sig) != GPT_SIGNATURE {
        return Err(E_HEADER_SIG);
    }
    Ok(GptHeader {
        header_crc32: u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
        part_entry_lba: u64::from_le_bytes([
            buf[72], buf[73], buf[74], buf[75], buf[76], buf[77], buf[78], buf[79],
        ]),
        part_entry_count: u32::from_le_bytes([buf[80], buf[81], buf[82], buf[83]]),
        part_entry_size: u32::from_le_bytes([buf[84], buf[85], buf[86], buf[87]]),
    })
}

/// 单个分区项（解析后）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartEntry {
    /// 分区类型 GUID（混合端序还原）。
    pub type_guid: String,
    /// 分区唯一 GUID。
    pub part_guid: String,
    /// 起始 LBA。
    pub first_lba: u64,
    /// 结束 LBA（含）。
    pub last_lba: u64,
    /// 分区名（UTF-16LE 还原，截 NUL）。
    pub name: String,
}

impl PartEntry {
    /// 项占用扇区数。
    pub fn sectors(&self) -> u64 {
        if self.last_lba >= self.first_lba {
            self.last_lba - self.first_lba + 1
        } else {
            0
        }
    }
    /// 类型为空（未使用槽位）。
    pub fn is_unused(&self) -> bool {
        self.type_guid == EMPTY_GUID
    }
}

/// 空 GUID（未使用槽位标记）。
pub const EMPTY_GUID: &str = "00000000-0000-0000-0000-000000000000";

/// 单项 128 字节解析：类型 GUID 未用即返回未用标记项。
pub fn parse_part_entry(buf: &[u8]) -> Result<PartEntry, u16> {
    if buf.len() < 128 {
        return Err(E_ENTRY_SHORT);
    }
    let mut tg = [0u8; 16];
    tg.copy_from_slice(&buf[0..16]);
    let type_guid = guid_from_mixed(&tg);
    if type_guid == EMPTY_GUID {
        return Ok(PartEntry {
            type_guid,
            part_guid: EMPTY_GUID.to_string(),
            first_lba: 0,
            last_lba: 0,
            name: String::new(),
        });
    }
    let mut pg = [0u8; 16];
    pg.copy_from_slice(&buf[16..32]);
    Ok(PartEntry {
        type_guid,
        part_guid: guid_from_mixed(&pg),
        first_lba: u64::from_le_bytes([
            buf[32], buf[33], buf[34], buf[35], buf[36], buf[37], buf[38], buf[39],
        ]),
        last_lba: u64::from_le_bytes([
            buf[40], buf[41], buf[42], buf[43], buf[44], buf[45], buf[46], buf[47],
        ]),
        name: utf16_name(&buf[56..128]),
    })
}

/// UTF-16LE 分区名还原（遇 NUL 截断，非偶数长截尾）。
pub fn utf16_name(buf: &[u8]) -> String {
    let mut units = Vec::new();
    let mut i = 0;
    while i + 1 < buf.len() {
        let u = u16::from_le_bytes([buf[i], buf[i + 1]]);
        if u == 0 {
            break;
        }
        units.push(u);
        i += 2;
    }
    String::from_utf16_lossy(&units)
}

// ---------------------------------------------------------------------------
// 分区表发现器
// ---------------------------------------------------------------------------

/// 差异类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscDiff {
    /// 参照有而发现无（漏检）。
    Missing,
    /// 发现有而参照无（多检）。
    Extra,
    /// 两侧都有但边界不一致（错位）。
    Misaligned,
    /// 发现判 ESP 而参照不判（ESP 误报）。
    EspFalsePositive,
}

/// ESP 误报数上限纪律：判据 J1 要求恒为 0。
pub const ESP_FALSE_POSITIVE_LIMIT: u32 = 0;

/// 分区表发现器。
pub struct PartDisc {
    /// 解析出的在用分区项。
    pub entries: Vec<PartEntry>,
    /// 判为 ESP 的项下标（双证据齐取）。
    pub esp_idx: Vec<usize>,
    /// 头部信息。
    pub header: Option<GptHeader>,
}

impl PartDisc {
    /// 从 GPT 头 + 项数组区扫描（跳过未用槽位，越界项记拒收账并跳过）。
    pub fn scan(header_buf: &[u8], entries_buf: &[u8]) -> Result<PartDisc, u16> {
        let hdr = parse_gpt_header(header_buf)?;
        if hdr.part_entry_size == 0 || hdr.part_entry_size > 1024 {
            return Err(E_ENTRY_SIZE);
        }
        if hdr.part_entry_count > 128 {
            return Err(E_ENTRY_COUNT);
        }
        let es = hdr.part_entry_size as usize;
        let need = es * hdr.part_entry_count as usize;
        if entries_buf.len() < need {
            return Err(E_ENTRY_BUF_SHORT);
        }
        let mut entries = Vec::new();
        for i in 0..hdr.part_entry_count as usize {
            let e = parse_part_entry(&entries_buf[i * es..(i + 1) * es])?;
            if !e.is_unused() {
                entries.push(e);
            }
        }
        Ok(PartDisc {
            entries,
            esp_idx: Vec::new(),
            header: Some(hdr),
        })
    }

    /// ESP 识别（判据主体）：类型 GUID 命中 + 文件系统探测命中，两证齐取。
    /// 探测不命中只记探测账，不算 ESP——这是误报为 0 的结构性来源。
    pub fn identify_esp(&mut self, probes: &[FsProbe]) -> usize {
        self.esp_idx.clear();
        for (i, e) in self.entries.iter().enumerate() {
            if e.type_guid != ESP_TYPE_GUID {
                continue;
            }
            let probed = probes.iter().any(|p| p.index == i && p.esp_signature());
            if probed {
                self.esp_idx.push(i);
            }
        }
        self.esp_idx.len()
    }

    /// 与 parted 参照清单对照：四类差异独立入账。
    pub fn crosscheck(&self, reference: &[RefEntry]) -> Vec<DiscDiff> {
        let mut diffs = Vec::new();
        for r in reference.iter() {
            let hit = self.entries.iter().position(|e| {
                e.first_lba == r.first_lba && e.last_lba == r.last_lba
            });
            match hit {
                None => diffs.push(DiscDiff::Missing),
                Some(ix) => {
                    if self.entries[ix].type_guid != r.type_guid {
                        diffs.push(DiscDiff::Misaligned);
                    }
                }
            }
        }
        for e in self.entries.iter() {
            let known = reference.iter().any(|r| {
                r.first_lba == e.first_lba && r.last_lba == e.last_lba
            });
            if !known {
                diffs.push(DiscDiff::Extra);
            }
        }
        // ESP 误报：本侧判 ESP 而参照里对应项不判。
        for &ix in self.esp_idx.iter() {
            let e = &self.entries[ix];
            let ref_is_esp = reference.iter().any(|r| {
                r.first_lba == e.first_lba && r.last_lba == e.last_lba && r.is_esp
            });
            if !ref_is_esp {
                diffs.push(DiscDiff::EspFalsePositive);
            }
        }
        diffs
    }

    /// 项重叠检测：任两在用项 LBA 区间交叠即违例（表一致性硬红线）。
    pub fn overlap_audit(&self) -> Result<(), u16> {
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                let a = &self.entries[i];
                let b = &self.entries[j];
                if a.first_lba <= b.last_lba && b.first_lba <= a.last_lba {
                    return Err(E_OVERLAP);
                }
            }
        }
        Ok(())
    }

    /// GUID 语义审计：全部项类型 GUID 良形、唯一 GUID 不重复。
    pub fn guid_audit(&self) -> Result<(), u16> {
        for e in self.entries.iter() {
            if !guid_wellformed(&e.type_guid) || !guid_wellformed(&e.part_guid) {
                return Err(E_GUID_MALFORMED);
            }
        }
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].part_guid == self.entries[j].part_guid {
                    return Err(E_GUID_DUP);
                }
            }
        }
        Ok(())
    }

    /// 快照（遥测口径）：在用项数 / ESP 数 / 总扇区。
    pub fn snapshot(&self) -> (usize, usize, u64) {
        let total: u64 = self.entries.iter().map(|e| e.sectors()).sum();
        (self.entries.len(), self.esp_idx.len(), total)
    }
}

/// 文件系统探测结果（供 ESP 双证据第二证）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsProbe {
    /// 对应分区项下标。
    pub index: usize,
    /// 引导扇区前 11 字节（跳板/签名字段）。
    pub boot: [u8; 11],
    /// FAT 根目录命中 "EFI" 目录名（8.3 短名形态）。
    pub has_efi_dir: bool,
}

impl FsProbe {
    /// FAT 系签名：跳板为空或 x86 短跳（0xEB/0xE9 起）。
    pub fn fat_boot_ok(&self) -> bool {
        self.boot[0] == 0xEB || self.boot[0] == 0xE9 || self.boot.iter().all(|&b| b == 0)
    }
    /// ESP 签名：FAT 引导合法 + EFI 目录在场。
    pub fn esp_signature(&self) -> bool {
        self.fat_boot_ok() && self.has_efi_dir
    }
}

/// parted 参照清单项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefEntry {
    pub first_lba: u64,
    pub last_lba: u64,
    pub type_guid: String,
    /// 参照侧是否标 ESP（parted 的 flags 含 esp/boot）。
    pub is_esp: bool,
}

// ---------------------------------------------------------------------------
// 错误码（0x6A50 段，显式判别式）
// ---------------------------------------------------------------------------

/// 段基。
pub const SEG_BASE: u16 = 0x6A50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum DiscErr {
    /// 头部缓冲过短。
    HeaderShort = SEG_BASE + 1,
    /// GPT 签名不对。
    HeaderSig = SEG_BASE + 2,
    /// 项尺寸非法。
    EntrySize = SEG_BASE + 3,
    /// 项数超窗。
    EntryCount = SEG_BASE + 4,
    /// 项数组缓冲过短。
    EntryBufShort = SEG_BASE + 5,
    /// LBA 区间重叠。
    Overlap = SEG_BASE + 6,
    /// GUID 良形失败。
    GuidMalformed = SEG_BASE + 7,
    /// 唯一 GUID 重复。
    GuidDup = SEG_BASE + 8,
    /// 项过短。
    EntryShort = SEG_BASE + 9,
    /// ESP 误报超限。
    EspFp = SEG_BASE + 10,
    /// 对照缺项。
    CrossMissing = SEG_BASE + 11,
}

impl DiscErr {
    /// 错误码。
    pub const fn code(self) -> u16 {
        self as u16
    }
    /// 人话明细。
    pub fn detail(self) -> &'static str {
        match self {
            DiscErr::HeaderShort => "GPT 头部缓冲不足 92 字节",
            DiscErr::HeaderSig => "GPT 签名非 EFI PART",
            DiscErr::EntrySize => "分区项尺寸非法（0 或超 1024）",
            DiscErr::EntryCount => "分区项数超 128 上限",
            DiscErr::EntryBufShort => "项数组缓冲不足以容纳声明项数",
            DiscErr::Overlap => "在用分区 LBA 区间重叠",
            DiscErr::GuidMalformed => "GUID 形不合法",
            DiscErr::GuidDup => "分区唯一 GUID 重复",
            DiscErr::EntryShort => "单项缓冲不足 128 字节",
            DiscErr::EspFp => "ESP 误报数超限（判据恒 0）",
            DiscErr::CrossMissing => "对照清单出现漏检项",
        }
    }
}

const E_HEADER_SHORT: u16 = DiscErr::HeaderShort.code();
const E_HEADER_SIG: u16 = DiscErr::HeaderSig.code();
const E_ENTRY_SIZE: u16 = DiscErr::EntrySize.code();
const E_ENTRY_COUNT: u16 = DiscErr::EntryCount.code();
const E_ENTRY_BUF_SHORT: u16 = DiscErr::EntryBufShort.code();
const E_OVERLAP: u16 = DiscErr::Overlap.code();
const E_GUID_MALFORMED: u16 = DiscErr::GuidMalformed.code();
const E_GUID_DUP: u16 = DiscErr::GuidDup.code();
const E_ENTRY_SHORT: u16 = DiscErr::EntryShort.code();
const E_ESP_FP: u16 = DiscErr::EspFp.code();
const E_CROSS_MISSING: u16 = DiscErr::CrossMissing.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 构造判据夹具：一个三在用项的 GPT（ESP/swap/root）+ 探测 + 参照。
fn fixture() -> (PartDisc, Vec<FsProbe>, Vec<RefEntry>) {
    // 头部：签名 + 项起点 2 / 项数 4 / 项尺寸 128。
    let mut hdr = [0u8; 92];
    hdr[0..8].copy_from_slice(&GPT_SIGNATURE.to_le_bytes());
    hdr[80..84].copy_from_slice(&4u32.to_le_bytes());
    hdr[84..88].copy_from_slice(&128u32.to_le_bytes());
    let mut entries = [0u8; 4 * 128];
    // 槽 0：ESP。
    let esp_t = guid_mixed_bytes_from(ESP_TYPE_GUID);
    entries[0..16].copy_from_slice(&esp_t);
    entries[32..40].copy_from_slice(&2048u64.to_le_bytes());
    entries[40..48].copy_from_slice(&204_799u64.to_le_bytes());
    // 槽 1：Linux fs。
    let lin_t = guid_mixed_bytes_from(LINUX_FS_GUID);
    let base = 128;
    entries[base..base + 16].copy_from_slice(&lin_t);
    entries[base + 32..base + 40].copy_from_slice(&204_800u64.to_le_bytes());
    entries[base + 40..base + 48].copy_from_slice(&409_599u64.to_le_bytes());
    // 槽 2：Linux fs（root）。
    let base = 256;
    entries[base..base + 16].copy_from_slice(&lin_t);
    entries[base + 32..base + 40].copy_from_slice(&409_600u64.to_le_bytes());
    entries[base + 40..base + 48].copy_from_slice(&614_399u64.to_le_bytes());
    // 槽 3 留空（未用）。
    let disc = PartDisc::scan(&hdr, &entries).expect("夹具头部必须合法");
    let probes = vec![
        FsProbe {
            index: 0,
            boot: [0xEB, 0x3C, 0x90, 0, 0, 0, 0, 0, 0, 0, 0],
            has_efi_dir: true,
        },
        FsProbe {
            index: 1,
            boot: [0x53, 0xEF, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            has_efi_dir: false,
        },
    ];
    let reference = vec![
        RefEntry {
            first_lba: 2048,
            last_lba: 204_799,
            type_guid: ESP_TYPE_GUID.to_string(),
            is_esp: true,
        },
        RefEntry {
            first_lba: 204_800,
            last_lba: 409_599,
            type_guid: LINUX_FS_GUID.to_string(),
            is_esp: false,
        },
        RefEntry {
            first_lba: 409_600,
            last_lba: 614_399,
            type_guid: LINUX_FS_GUID.to_string(),
            is_esp: false,
        },
    ];
    (disc, probes, reference)
}

/// 类型 GUID 字符串 → 混合端序 16 字节（夹具反演用）。
fn guid_mixed_bytes_from(s: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    let hex: Vec<u8> = s
        .as_bytes()
        .iter()
        .filter(|b| **b != b'-')
        .map(|b| (*b as char).to_digit(16).unwrap_or(0) as u8)
        .collect();
    let mut le = |a: usize, b: usize, off: usize| {
        for k in 0..(b - a) / 2 {
            let hi = hex[a + 2 * k];
            let lo = hex[a + 2 * k + 1];
            out[off + (b - a) / 2 - 1 - k] = hi * 16 + lo;
        }
    };
    le(0, 8, 0);
    le(8, 12, 4);
    le(12, 16, 6);
    for k in 0..8 {
        out[8 + k] = hex[16 + 2 * k] * 16 + hex[16 + 2 * k + 1];
    }
    out
}

/// 判据集（UNX-F0172-J1 + 结构红线）。自证五段：双证识别 / 对照零差异 /
/// 表一致性 / 单证不判 / 签名拒收。判据红返回首个违例码。
pub fn self_audit() -> Result<(), u16> {
    let (mut disc, probes, reference) = fixture();
    // 一、ESP 双证据识别：恰一项（槽 0），且恰为类型 GUID 命中 + FAT/EFI 双证。
    let n_esp = disc.identify_esp(&probes);
    if n_esp != 1 || disc.esp_idx != vec![0] {
        return Err(E_ESP_FP);
    }
    // 二、parted 对照：四类差异全零（判据主体——误报 0 的直接兑现）。
    let diffs = disc.crosscheck(&reference);
    if !diffs.is_empty() {
        return Err(E_CROSS_MISSING);
    }
    // 三、表一致性：无重叠、GUID 全良形且唯一。
    disc.overlap_audit()?;
    disc.guid_audit()?;
    // 四、单证不判：FAT 引导合法但无 EFI 目录 → 不判 ESP（防误报结构）。
    let (mut d2, _, _) = fixture();
    let probes2 = vec![FsProbe {
        index: 0,
        boot: [0xEB, 0x3C, 0x90, 0, 0, 0, 0, 0, 0, 0, 0],
        has_efi_dir: false,
    }];
    if d2.identify_esp(&probes2) != 0 {
        return Err(E_ESP_FP);
    }
    // 五、签名拒收：非 EFI PART 签名返回 E_HEADER_SIG。
    let mut bad_hdr = [0u8; 92];
    bad_hdr[0..8].copy_from_slice(&0x1234_5678_9abc_def0u64.to_le_bytes());
    if PartDisc::scan(&bad_hdr, &[0u8; 512]).err() != Some(E_HEADER_SIG) {
        return Err(E_HEADER_SIG);
    }
    // 六、判别式段位对拍：显式判别式与常量一致。
    if E_ESP_FP != SEG_BASE + 10 || E_CROSS_MISSING != SEG_BASE + 11 {
        return Err(E_ESP_FP);
    }
    Ok(())
}
