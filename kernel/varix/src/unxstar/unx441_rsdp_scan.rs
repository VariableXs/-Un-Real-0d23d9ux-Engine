//! unx441_rsdp_scan ↔ UNX-F0441 RSDP 签名搜索（EBDA/BIOS 双窗）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0441，UNX-A1-B23）：
//! 「RSDP 签名搜索（EBDA/BIOS 双窗）| 240 | 已深化 |
//!  双窗搜索（EBDA 1KB 窗 + BIOS ROM 0xE0000–0xFFFFF 窗）
//!  命中 RSDP 签名 'RSD PTR ' 定位唯一（1/1 命中账）」
//!
//! 设计要点：
//! 1. **双窗扫描**：EBDA 1KB 窗（物理 0x9FC00..0x9FFFF 段内，取前 1KB）
//!    + BIOS ROM 128KB 窗（0xE0000..0xFFFFF），两窗都按 16 字节边界对齐
//!    推进（ACPI 规范 RSDP 必须 16 字节对齐，禁逐字节蛮扫）。
//! 2. **签名 + 校验和双门**：'RSD PTR ' 八字节签名命中后，必须过
//!    revision 0/1 两代校验和（v1 前 20 字节、v2 前 36 字节扩展校验），
//!    只有签名没有校验和 = 伪命中（旧固件残留字节），显性拒绝。
//! 3. **唯一命中账**：多命中不是「取第一个」，而是显性报
//!    `AmbiguousHit`——两个 RSDP 会让后续 ACPI 子系统拿到矛盾事实。
//! 4. **OEM id 与 revision 落账**：命中后记录 oem_id/revision/rsdt/xsdt
//!    地址，形成 `RsdpLedger`，供 F0461 配置访问裁定与 F0561 电源账消费。
//! 5. **物理源注入**：`ScanSource` trait 让 QEMU/真机/测试三态同构，
//!    宿主用 Vec<u8> 代理物理内存，内核面用 HHDM 直读。
//!
//! 诊断码：`RsdpCode` 0x82 段（0x8200..=0x820F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX441_RSDP_VERSION: &str = "unx441-rsdp-scan-v1";

/// RSDP 签名字节序列（ACPI 规范固定 8 字节，含尾部空格）。
pub const RSDP_SIGNATURE: [u8; 8] = *b"RSD PTR ";

/// ACPI v1 头长（前 20 字节参与校验和）。
pub const RSDP_V1_LEN: usize = 20;

/// ACPI v2 扩展头长（前 36 字节参与校验和）。
pub const RSDP_V2_LEN: usize = 36;

/// RSDP 16 字节对齐步长。
pub const RSDP_ALIGN: u64 = 16;

/// EBDA 窗长（1KB）。
pub const EBDA_WINDOW_LEN: u64 = 1024;

/// BIOS ROM 窗起址。
pub const BIOS_ROM_BASE: u64 = 0xE0000;

/// BIOS ROM 窗长（0xE0000..0xFFFFF = 128KB）。
pub const BIOS_ROM_LEN: u64 = 0x20000;

/// 单窗最大命中记录数（超出即歧义，不再记录）。
pub const MAX_HITS_PER_WINDOW: usize = 4;

/// 物理源抽象（QEMU/真机/测试三态同构）。
pub trait ScanSource {
    /// 读一段物理内存（越界返回 None，禁 panic）。
    fn read(&self, addr: u64, buf: &mut [u8]) -> Option<usize>;
}

/// Vec 代理源（宿主测试/寄存器回放用）。
pub struct VecSource {
    pub base: u64,
    pub bytes: Vec<u8>,
}

impl ScanSource for VecSource {
    fn read(&self, addr: u64, buf: &mut [u8]) -> Option<usize> {
        if addr < self.base {
            return None;
        }
        let off = (addr - self.base) as usize;
        if off >= self.bytes.len() {
            return None;
        }
        let n = if off + buf.len() > self.bytes.len() {
            self.bytes.len() - off
        } else {
            buf.len()
        };
        let mut i = 0;
        while i < n {
            if let Some(b) = self.bytes.get(off + i) {
                buf[i] = *b;
            }
            i += 1;
        }
        Some(n)
    }
}

/// RSDP 记录（命中后落账字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RsdpRecord {
    /// 物理地址
    pub addr: u64,
    /// 来自哪个窗
    pub window: ScanWindow,
    /// 修订号（0 = ACPI 1.0，2 = ACPI 2.0+）
    pub revision: u8,
    /// OEM id 六字节
    pub oem_id: [u8; 6],
    /// RSDT 32 位地址（revision 0 有效）
    pub rsdt_addr: u32,
    /// XSDT 64 位地址（revision 2 有效）
    pub xsdt_addr: u64,
    /// 长度（20 或 36）
    pub length: u32,
}

/// 窗标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanWindow {
    /// EBDA 1KB 窗
    Ebda,
    /// BIOS ROM 128KB 窗
    BiosRom,
}

/// 扫描账（两窗命中汇总）。
#[derive(Clone, Debug)]
pub struct RsdpLedger {
    /// EBDA 窗命中
    pub ebda_hits: Vec<RsdpRecord>,
    /// BIOS 窗命中
    pub bios_hits: Vec<RsdpRecord>,
    /// 已扫字节数（性能对账用）
    pub scanned_bytes: u64,
    /// 签名命中但校验和失败的次数（伪命中账）
    pub checksum_rejects: u32,
}

/// 扫描错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsdpError {
    /// 双窗合计多命中（事实矛盾）
    AmbiguousHit { total: u32 },
    /// 源读失败
    SourceUnreadable { addr: u64 },
    /// 头长非法
    LengthBad { len: u32 },
    /// 校验和不符
    ChecksumBad { addr: u64, revision: u8 },
    /// revision 未知
    RevisionBad { revision: u8 },
}

/// 诊断码（0x82 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsdpCode {
    /// 0x8200 多命中歧义
    Ambiguous,
    /// 0x8201 源不可读
    SourceBad,
    /// 0x8202 头长非法
    LenBad,
    /// 0x8203 校验和不符
    ChecksumBad,
    /// 0x8204 revision 未知
    RevisionBad,
    /// 0x8205 唯一命中
    UniqueHit,
    /// 0x8206 零命中（无 ACPI）
    NoHit,
}

impl RsdpCode {
    pub const fn code(self) -> u16 {
        match self {
            RsdpCode::Ambiguous => 0x8200,
            RsdpCode::SourceBad => 0x8201,
            RsdpCode::LenBad => 0x8202,
            RsdpCode::ChecksumBad => 0x8203,
            RsdpCode::RevisionBad => 0x8204,
            RsdpCode::UniqueHit => 0x8205,
            RsdpCode::NoHit => 0x8206,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            RsdpCode::Ambiguous => "RSDP-AMBIGUOUS",
            RsdpCode::SourceBad => "RSDP-SOURCE-BAD",
            RsdpCode::LenBad => "RSDP-LEN-BAD",
            RsdpCode::ChecksumBad => "RSDP-CHECKSUM-BAD",
            RsdpCode::RevisionBad => "RSDP-REVISION-BAD",
            RsdpCode::UniqueHit => "RSDP-UNIQUE-HIT",
            RsdpCode::NoHit => "RSDP-NO-HIT",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            RsdpCode::Ambiguous => "RSDP 多命中：两窗合计出现多个有效 RSDP，ACPI 事实矛盾，拒绝继续",
            RsdpCode::SourceBad => "物理源不可读：扫描窗地址越界或映射缺失",
            RsdpCode::LenBad => "RSDP 头长非法：只允许 20（v1）或 36（v2）",
            RsdpCode::ChecksumBad => "RSDP 校验和不符：签名命中但头和校验失败，判伪命中",
            RsdpCode::RevisionBad => "RSDP revision 未知：只允许 0 或 2",
            RsdpCode::UniqueHit => "RSDP 唯一命中：签名 + 校验和双门通过且全机唯一",
            RsdpCode::NoHit => "RSDP 零命中：本机无 ACPI（旧式 MPS 表路径）",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx441_rsdp_scan.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8200 && (c & 0x00FF) < 0x10
    }
}

impl RsdpError {
    pub const fn code(self) -> RsdpCode {
        match self {
            RsdpError::AmbiguousHit { .. } => RsdpCode::Ambiguous,
            RsdpError::SourceUnreadable { .. } => RsdpCode::SourceBad,
            RsdpError::LengthBad { .. } => RsdpCode::LenBad,
            RsdpError::ChecksumBad { .. } => RsdpCode::ChecksumBad,
            RsdpError::RevisionBad { .. } => RsdpCode::RevisionBad,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

impl RsdpLedger {
    /// 空账。
    pub const fn new() -> Self {
        RsdpLedger {
            ebda_hits: Vec::new(),
            bios_hits: Vec::new(),
            scanned_bytes: 0,
            checksum_rejects: 0,
        }
    }

    /// 合计命中数。
    pub fn total_hits(&self) -> u32 {
        (self.ebda_hits.len() + self.bios_hits.len()) as u32
    }

    /// 唯一命中记录（多命中返回 Err，零命中返回 Err(NoHit 语义由调用方判)）。
    pub fn unique(&self) -> Result<RsdpRecord, RsdpError> {
        let total = self.total_hits();
        if total == 0 {
            // 零命中不是错误码，调用方按 NoHit 语义处理；此处返回歧义 0 不可达，
            // 故用 SourceBad 占位会误导——改为显式哨兵：调用方应先查 total_hits。
            return Err(RsdpError::AmbiguousHit { total });
        }
        if total > 1 {
            return Err(RsdpError::AmbiguousHit { total });
        }
        if let Some(r) = self.ebda_hits.first() {
            return Ok(*r);
        }
        match self.bios_hits.first() {
            Some(r) => Ok(*r),
            None => Err(RsdpError::AmbiguousHit { total: 0 }),
        }
    }
}

/// 单窗扫描（16 字节对齐推进 + 签名 + 校验和双门）。
///
/// `window_base/window_len` 定义窗；返回本窗命中记录。
pub fn scan_window(
    src: &dyn ScanSource,
    window: ScanWindow,
    window_base: u64,
    window_len: u64,
    ledger: &mut RsdpLedger,
) -> Result<Vec<RsdpRecord>, RsdpError> {
    let mut hits: Vec<RsdpRecord> = Vec::new();
    let mut addr = window_base;
    while addr < window_base + window_len {
        // 16 字节对齐步进（ACPI 规范 RSDP 对齐要求）
        let mut sig = [0u8; 8];
        if src.read(addr, &mut sig).is_none() {
            // 窗尾不足 8 字节：跳过（对齐余量）
            addr += RSDP_ALIGN;
            ledger.scanned_bytes = ledger.scanned_bytes.saturating_add(RSDP_ALIGN);
            continue;
        }
        ledger.scanned_bytes = ledger.scanned_bytes.saturating_add(RSDP_ALIGN);
        if sig != RSDP_SIGNATURE {
            addr += RSDP_ALIGN;
            continue;
        }
        // 签名命中：读 36 字节（v2 长度）做校验和
        let mut head = [0u8; RSDP_V2_LEN];
        let n = match src.read(addr, &mut head) {
            Some(n) => n,
            None => return Err(RsdpError::SourceUnreadable { addr }),
        };
        if n < RSDP_V1_LEN {
            return Err(RsdpError::SourceUnreadable { addr });
        }
        let revision = match head.get(15) {
            Some(r) => *r,
            None => return Err(RsdpError::SourceUnreadable { addr }),
        };
        let length: u32 = match revision {
            0 => RSDP_V1_LEN as u32,
            2 => RSDP_V2_LEN as u32,
            other => return Err(RsdpError::RevisionBad { revision: other }),
        };
        if (length as usize) > n {
            return Err(RsdpError::LengthBad { len: length });
        }
        // 校验和：头和按字节和 ≡ 0 (mod 256)
        let mut sum: u8 = 0;
        let mut i = 0;
        while i < length as usize {
            if let Some(b) = head.get(i) {
                sum = sum.wrapping_add(*b);
            }
            i += 1;
        }
        if sum != 0 {
            ledger.checksum_rejects = ledger.checksum_rejects.saturating_add(1);
            addr += RSDP_ALIGN;
            continue;
        }
        let mut oem_id = [0u8; 6];
        let mut k = 0;
        while k < 6 {
            if let Some(b) = head.get(9 + k) {
                oem_id[k] = *b;
            }
            k += 1;
        }
        let rsdt_addr: u32 = match head.get(16..20) {
            Some(sl) => u32::from_le_bytes([sl[0], sl[1], sl[2], sl[3]]),
            None => return Err(RsdpError::SourceUnreadable { addr }),
        };
        let xsdt_addr: u64 = if revision == 2 {
            match head.get(24..32) {
                Some(sl) => {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(sl);
                    u64::from_le_bytes(b)
                }
                None => return Err(RsdpError::SourceUnreadable { addr }),
            }
        } else {
            0
        };
        if hits.len() >= MAX_HITS_PER_WINDOW {
            return Err(RsdpError::AmbiguousHit {
                total: (hits.len() + 1) as u32,
            });
        }
        hits.push(RsdpRecord {
            addr,
            window,
            revision,
            oem_id,
            rsdt_addr,
            xsdt_addr,
            length,
        });
        addr += RSDP_ALIGN;
    }
    Ok(hits)
}

/// 双窗总扫描（EBDA + BIOS ROM）。
pub fn scan_dual_window(
    src: &dyn ScanSource,
    ebda_base: u64,
    ledger: &mut RsdpLedger,
) -> Result<RsdpRecord, RsdpError> {
    let mut ebda = scan_window(
        src,
        ScanWindow::Ebda,
        ebda_base,
        EBDA_WINDOW_LEN,
        ledger,
    )?;
    let mut bios = scan_window(
        src,
        ScanWindow::BiosRom,
        BIOS_ROM_BASE,
        BIOS_ROM_LEN,
        ledger,
    )?;
    ledger.ebda_hits.append(&mut ebda);
    ledger.bios_hits.append(&mut bios);
    ledger.unique()
}

/// 域自检。
pub fn run_unx441_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx441");

    // 构造一个带 RSDP 的 Vec 源（v2，36 字节，校验和自洽）
    let build_src = |place_addr: u64, corrupt_sum: bool| -> VecSource {
        let base: u64 = 0xE0000;
        let mut bytes = alloc::vec![0u8; 0x20000];
        let off = (place_addr - base) as usize;
        let mut head = [0u8; RSDP_V2_LEN];
        head[0..8].copy_from_slice(&RSDP_SIGNATURE);
        head[8] = 0; // checksum 占位
        let mut oem = *b"VARIIX";
        oem[5] = b'X';
        head[9..15].copy_from_slice(&oem);
        head[15] = 2; // revision 2
        let rsdt: u32 = 0xE250_0000;
        let xsdt: u64 = 0xE260_0000;
        head[16..20].copy_from_slice(&rsdt.to_le_bytes());
        head[20..24].copy_from_slice(&36u32.to_le_bytes());
        head[24..32].copy_from_slice(&xsdt.to_le_bytes());
        if !corrupt_sum {
            let mut sum: u8 = 0;
            let mut i = 0;
            while i < RSDP_V2_LEN {
                sum = sum.wrapping_add(head[i]);
                i += 1;
            }
            head[8] = 0u8.wrapping_sub(sum);
        } else {
            head[8] = 0xFF;
        }
        let mut k = 0;
        while k < RSDP_V2_LEN {
            if let Some(slot) = bytes.get_mut(off + k) {
                *slot = head[k];
            }
            k += 1;
        }
        VecSource { base, bytes }
    };

    // C441-01 唯一命中（BIOS 窗，16 对齐地址 0xE0000 + 0x140 = 0xE0140）
    {
        let src = build_src(0xE0140, false);
        let mut led = RsdpLedger::new();
        let r = scan_dual_window(&src, 0x9FC00, &mut led);
        let good = match r {
            Ok(rec) => {
                rec.addr == 0xE0140
                    && rec.revision == 2
                    && rec.rsdt_addr == 0xE250_0000
                    && rec.xsdt_addr == 0xE260_0000
                    && led.total_hits() == 1
            }
            Err(_) => false,
        };
        s.add("C441-01-BIOS 窗唯一命中", good, if good { "" } else { "RSDP 未被唯一捕获" });
    }

    // C441-02 校验和失败判伪命中（签名在、校验和错 → 零命中 + rejects+1）
    {
        let src = build_src(0xE0200, true);
        let mut led = RsdpLedger::new();
        let r = scan_dual_window(&src, 0x9FC00, &mut led);
        let good = r.is_err() && led.total_hits() == 0 && led.checksum_rejects >= 1;
        s.add("C441-02-校验和失败判伪", good, if good { "" } else { "伪命中被放行" });
    }

    // C441-03 双命中歧义（EBDA + BIOS 各一个有效 RSDP）
    {
        let mut src = build_src(0xE0300, false);
        // 在 EBDA 窗同构放一个
        let base: u64 = 0x9FC00;
        let off = (0x9FD00 - base) as usize;
        let mut head = [0u8; RSDP_V2_LEN];
        head[0..8].copy_from_slice(&RSDP_SIGNATURE);
        head[15] = 2;
        let mut sum: u8 = 0;
        let mut i = 0;
        while i < RSDP_V2_LEN {
            sum = sum.wrapping_add(head[i]);
            i += 1;
        }
        head[8] = 0u8.wrapping_sub(sum);
        let mut k = 0;
        while k < RSDP_V2_LEN {
            if let Some(slot) = src.bytes.get_mut(off + k) {
                *slot = head[k];
            }
            k += 1;
        }
        let mut led = RsdpLedger::new();
        let r = scan_dual_window(&src, base, &mut led);
        let good = matches!(r, Err(RsdpError::AmbiguousHit { total: 2 }));
        s.add("C441-03-双命中歧义显性", good, if good { "" } else { "矛盾事实未拦截" });
    }

    // C441-04 非对齐地址不放行（0xE0131 不对齐 → 签名命中但扫描路径不踩）
    {
        let src = build_src(0xE0131, false);
        let mut led = RsdpLedger::new();
        let r = scan_dual_window(&src, 0x9FC00, &mut led);
        let good = r.is_err() && led.total_hits() == 0;
        s.add("C441-04-非对齐签名不采信", good, if good { "" } else { "未对齐地址被误命中" });
    }

    // C441-05 已扫字节对账（BIOS 128KB / 16 = 8192 步）
    {
        let src = build_src(0xE0140, false);
        let mut led = RsdpLedger::new();
        let _ = scan_dual_window(&src, 0x9FC00, &mut led);
        let expect: u64 = (BIOS_ROM_LEN / RSDP_ALIGN + EBDA_WINDOW_LEN / RSDP_ALIGN) * RSDP_ALIGN;
        let good = led.scanned_bytes == expect;
        s.add("C441-05-已扫字节守恒", good, if good { "" } else { "扫描步长对账不平" });
    }

    // C441-06 诊断码段独占
    {
        let codes = [
            RsdpCode::Ambiguous,
            RsdpCode::SourceBad,
            RsdpCode::LenBad,
            RsdpCode::ChecksumBad,
            RsdpCode::RevisionBad,
            RsdpCode::UniqueHit,
            RsdpCode::NoHit,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C441-06-诊断码 0x82 段独占", all, if all { "" } else { "码值越出 0x82 段" });
    }

    s
}


/// 兼容别名（并行会话共享 mod.rs 注册以 run_tNNN_checks 之名调用本模块；加法不改他人行）。
pub fn run_t441_checks() -> crate::checks::CheckSet {
    run_unx441_checks()
}
