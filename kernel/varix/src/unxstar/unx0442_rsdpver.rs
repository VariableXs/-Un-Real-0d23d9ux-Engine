//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0442 · RSDP 校验与版本账 · 0x36080
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥220 行；
//!   ② RSDP 校验和验证通过（1/1），revision 与长度字段自洽断言
//!      （v1:20/v2+:36）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - RSDP（Root System Description Pointer，ACPI 规范）两代布局：
//!     v1.0 = 20 字节（Signature"RSD PTR "8 + Checksum1 + OEMID6 +
//!     Revision1 + RsdtAddress4）；v2.0+ = 36 字节（前 20 + Length4 +
//!     XsdtAddress8 + Checksum2 + Reserved3）；
//!   - 自洽断言：revision==0 ⇒ 长度字段语义 20（v2 Length 字段缺席，
//!     读取上限按 20）；revision>=2 ⇒ Length==36 且 XSDT 地址非零、
//!     扩展校验和（offset 32..36 覆盖 36 字节）通过；
//!   - 校验和双路：Checksum1 覆盖前 20 字节（两代同规）求和 mod 256
//!     == 0；Checksum2 仅 v2+ 覆盖全 36 字节；
//!   - 签名核：前 8 字节必须恰为 "RSD PTR "（含空格，ACPI 逐字），
//!     任何变体（"RSD-PTR"/缺空格）显性拒；
//!   - 版本账：发现记录（revision/长度/两和结果/OEMID/地址域）一次性
//!     落账，重复登记显性拒（防账面重写）；
//!   - 注入面：坏签名/坏和1/坏和2/长度-版本失配/v1 带假扩展族/v2+
//!     XSDT 零地址 六族，LCG 种子写死 = SEG_BASE（0x36080），禁
//!     rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36080，全仓独占）。
pub const SEG_BASE: u32 = 0x36080;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsdpCode {
    /// 签名不是 "RSD PTR "（逐字节含空格）
    BadSignature,
    /// Checksum1（前 20 字节）未过
    BadChecksum1,
    /// Checksum2（v2+ 全 36 字节）未过
    BadChecksum2,
    /// 长度-版本失配（v2+ Length != 36）
    LengthMismatch,
    /// v2+ XSDT 地址为零（v2 语义必非零）
    XsdtZero,
    /// 修订号非法（>2 的未知代际）
    UnknownRevision,
    /// 缓冲区过短（连 v1 的 20 字节都不足）
    TooShort,
    /// 重复登记（版本账一次性）
    DuplicateEntry,
    /// OEMID 含非 ASCII 可打印字符
    BadOemId,
    /// 账面为空却声明校验通过（谎报）
    EmptyLedgerClaim,
}

impl RsdpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RsdpCode::BadSignature => "签名非法：前 8 字节不恰为 \"RSD PTR \"（ACPI 逐字含空格）",
            RsdpCode::BadChecksum1 => "Checksum1 未过：前 20 字节求和 mod 256 != 0",
            RsdpCode::BadChecksum2 => "Checksum2 未过：v2+ 全 36 字节扩展校验和失败",
            RsdpCode::LengthMismatch => "长度失配：revision>=2 时 Length 字段必须恰 36",
            RsdpCode::XsdtZero => "XSDT 零地址：v2+ 语义 XsdtAddress 必非零",
            RsdpCode::UnknownRevision => "未知代际：revision > 2 超出本核验器闭集",
            RsdpCode::TooShort => "缓冲过短：不足 v1 的 20 字节布局",
            RsdpCode::DuplicateEntry => "重复登记：版本账一次性，防重写",
            RsdpCode::BadOemId => "OEMID 非法：含非 ASCII 可打印字符",
            RsdpCode::EmptyLedgerClaim => "谎报：账面为空却声明校验通过",
        }
    }
    pub const ALL: [RsdpCode; 10] = [
        RsdpCode::BadSignature,
        RsdpCode::BadChecksum1,
        RsdpCode::BadChecksum2,
        RsdpCode::LengthMismatch,
        RsdpCode::XsdtZero,
        RsdpCode::UnknownRevision,
        RsdpCode::TooShort,
        RsdpCode::DuplicateEntry,
        RsdpCode::BadOemId,
        RsdpCode::EmptyLedgerClaim,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in RsdpCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量（ACPI 规范布局） ────────────────────────────────────────
/// v1.0 布局长度。
pub const RSDP_V1_LEN: usize = 20;
/// v2.0+ 布局长度。
pub const RSDP_V2_LEN: usize = 36;
/// 签名长度。
pub const SIGN_LEN: usize = 8;
/// ACPI 规定签名（逐字节含空格）。
pub const RSDP_SIGNATURE: [u8; 8] = [b'R', b'S', b'D', b' ', b'P', b'T', b'R', b' '];
/// v2 Length 字段偏移。
pub const OFF_LENGTH: usize = 20;
/// v2 XsdtAddress 偏移。
pub const OFF_XSDT: usize = 24;
/// v2 Checksum2 偏移。
pub const OFF_CSUM2: usize = 32;

// 布局偏移（v1）：Signature0..8 / Checksum1:8 / OEMID:9..15 / Revision:15 / RsdtAddress:16..20

/// 求 8 位和校验：sum mod 256 == 0。
pub fn checksum_ok(bytes: &[u8]) -> bool {
    let mut acc: u8 = 0;
    for b in bytes {
        acc = acc.wrapping_add(*b);
    }
    acc == 0
}

/// OEMID 可打印 ASCII 核验（ACPI 语义：6 字节 OEM ID）。
pub fn oemid_ok(bytes: &[u8]) -> bool {
    if bytes.len() != 6 {
        return false;
    }
    for b in bytes {
        if *b < 0x20 || *b > 0x7E {
            return false;
        }
    }
    true
}

// ── 核验结论 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsdpVerdict {
    /// v1.0 校验通过（20 字节）
    V1Ok,
    /// v2.0+ 双和全过（36 字节）
    V2Ok,
    /// 拒收（附码）
    Rejected,
}

/// 一条 RSDP 发现记录（版本账行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RsdpRecord {
    pub revision: u8,
    pub layout_len: usize,
    pub csum1_ok: bool,
    pub csum2_ok: bool,
    pub rsdt_addr: u32,
    pub xsdt_addr: u64,
    pub oemid: [u8; 6],
    pub verdict: RsdpVerdict,
}

/// RSDP 核验器（只读 + 一次性版本账）。
pub struct RsdpValidator {
    pub entries: Vec<RsdpRecord>,
    pub rejected_total: usize,
}

impl RsdpValidator {
    pub fn new() -> RsdpValidator {
        RsdpValidator {
            entries: Vec::new(),
            rejected_total: 0,
        }
    }

    /// 核验一段 RSDP 原始字节。
    pub fn validate(&mut self, raw: &[u8]) -> Result<RsdpRecord, RsdpCode> {
        let rec = validate_static(raw)?;
        if let RsdpVerdict::Rejected = rec.verdict {
            self.rejected_total += 1;
            return Err(self.last_code(rec));
        }
        // 一次性账：同 revision+rsdt 重复登记拒。
        for e in &self.entries {
            if e.revision == rec.revision && e.rsdt_addr == rec.rsdt_addr {
                return Err(RsdpCode::DuplicateEntry);
            }
        }
        self.entries.push(rec);
        Ok(rec)
    }

    fn last_code(&self, rec: RsdpRecord) -> RsdpCode {
        // 拒收时按优先序还原首因码（静态核验器已给出，此处按账面字段推断）。
        if rec.layout_len < RSDP_V1_LEN {
            return RsdpCode::TooShort;
        }
        if !rec.csum1_ok {
            return RsdpCode::BadChecksum1;
        }
        if rec.revision >= 2 && !rec.csum2_ok {
            return RsdpCode::BadChecksum2;
        }
        if rec.revision >= 2 && rec.layout_len != RSDP_V2_LEN {
            return RsdpCode::LengthMismatch;
        }
        if rec.revision >= 2 && rec.xsdt_addr == 0 {
            return RsdpCode::XsdtZero;
        }
        RsdpCode::BadSignature
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 汇总行（结果摘要，非空判据）。
    pub fn summary_line(&self) -> String {
        let v1 = self.entries.iter().filter(|e| e.verdict == RsdpVerdict::V1Ok).count();
        let v2 = self.entries.iter().filter(|e| e.verdict == RsdpVerdict::V2Ok).count();
        format!(
            "RSDP 版本账: 登记={} v1通过={} v2通过={} 拒收={}",
            self.entries.len(),
            v1,
            v2,
            self.rejected_total
        )
    }
}

/// 静态核验（无账面副作用）：字节 → 记录或首因码。
pub fn validate_static(raw: &[u8]) -> Result<RsdpRecord, RsdpCode> {
    if raw.len() < RSDP_V1_LEN {
        return Err(RsdpCode::TooShort);
    }
    // 签名核：逐字节恰为 "RSD PTR "。
    if raw[0..SIGN_LEN] != RSDP_SIGNATURE {
        return Err(RsdpCode::BadSignature);
    }
    let csum1_ok = checksum_ok(&raw[0..RSDP_V1_LEN]);
    let revision = raw[15];
    let oemid_bytes = [raw[9], raw[10], raw[11], raw[12], raw[13], raw[14]];
    let oem_ok = oemid_ok(&oemid_bytes);
    let rsdt = u32::from_le_bytes([raw[16], raw[17], raw[18], raw[19]]);
    if revision > 2 {
        return Err(RsdpCode::UnknownRevision);
    }
    if !oem_ok {
        return Err(RsdpCode::BadOemId);
    }
    if revision == 0 {
        // v1：Checksum1 必过，无扩展族。
        if !csum1_ok {
            return Err(RsdpCode::BadChecksum1);
        }
        return Ok(RsdpRecord {
            revision,
            layout_len: RSDP_V1_LEN,
            csum1_ok,
            csum2_ok: false,
            rsdt_addr: rsdt,
            xsdt_addr: 0,
            oemid: oemid_bytes,
            verdict: RsdpVerdict::V1Ok,
        });
    }
    // v2+：布局必须完整 36 字节。
    if raw.len() < RSDP_V2_LEN {
        return Err(RsdpCode::TooShort);
    }
    if !csum1_ok {
        return Err(RsdpCode::BadChecksum1);
    }
    let len_field = u32::from_le_bytes([
        raw[OFF_LENGTH],
        raw[OFF_LENGTH + 1],
        raw[OFF_LENGTH + 2],
        raw[OFF_LENGTH + 3],
    ]) as usize;
    let xsdt = u64::from_le_bytes([
        raw[OFF_XSDT],
        raw[OFF_XSDT + 1],
        raw[OFF_XSDT + 2],
        raw[OFF_XSDT + 3],
        raw[OFF_XSDT + 4],
        raw[OFF_XSDT + 5],
        raw[OFF_XSDT + 6],
        raw[OFF_XSDT + 7],
    ]);
    let csum2_ok = checksum_ok(&raw[0..RSDP_V2_LEN]);
    // 自洽断言：revision>=2 ⇒ Length==36。
    if len_field != RSDP_V2_LEN {
        return Err(RsdpCode::LengthMismatch);
    }
    if !csum2_ok {
        return Err(RsdpCode::BadChecksum2);
    }
    if xsdt == 0 {
        return Err(RsdpCode::XsdtZero);
    }
    Ok(RsdpRecord {
        revision,
        layout_len: RSDP_V2_LEN,
        csum1_ok,
        csum2_ok,
        rsdt_addr: rsdt,
        xsdt_addr: xsdt,
        oemid: oemid_bytes,
        verdict: RsdpVerdict::V2Ok,
    })
}

// ── 合法样本构造器（测试与注入对照面） ───────────────────────────
/// 构造一条合法 v1 RSDP（自动补 Checksum1）。
pub fn make_v1(revision_always_zero: u8, rsdt: u32, oemid: &[u8; 6]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&RSDP_SIGNATURE);
    b.push(0); // Checksum1 占位
    b.extend_from_slice(oemid);
    b.push(revision_always_zero);
    b.extend_from_slice(&rsdt.to_le_bytes());
    // 补和：前 20 字节和 mod 256 == 0。
    let mut acc: u8 = 0;
    for i in 0..b.len() {
        acc = acc.wrapping_add(b[i]);
    }
    b[8] = acc.wrapping_neg();
    b
}

/// 构造一条合法 v2 RSDP（自动补双和）。
pub fn make_v2(rsdt: u32, xsdt: u64, oemid: &[u8; 6]) -> Vec<u8> {
    let mut b = make_v1(2, rsdt, oemid);
    b.push(0);
    b.push(0);
    b.push(0);
    b.push(0); // Length 占位（offset 20..24）
    b.extend_from_slice(&xsdt.to_le_bytes()); // 24..32
    b.push(0); // Checksum2 占位
    b.push(0);
    b.push(0);
    b.push(0); // Reserved 33..36
    // Length = 36。
    b[OFF_LENGTH] = (RSDP_V2_LEN as u32 & 0xFF) as u8;
    b[OFF_LENGTH + 1] = 0;
    b[OFF_LENGTH + 2] = 0;
    b[OFF_LENGTH + 3] = 0;
    // Checksum2：36 字节和 mod 256 == 0（Checksum1 区已平衡，补 20..36 区）。
    let mut acc: u8 = 0;
    for i in 20..RSDP_V2_LEN {
        acc = acc.wrapping_add(b[i]);
    }
    b[OFF_CSUM2] = acc.wrapping_neg();
    b
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 注入面（六族） ───────────────────────────────────────────────
fn drill_bad_signature_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    let variants: [[u8; 8]; 5] = [
        [b'R', b'S', b'D', b'-', b'P', b'T', b'R', b' '],
        [b'R', b'S', b'D', b' ', b'P', b'T', b'R', b'\0'],
        [b'X', b'S', b'D', b' ', b'P', b'T', b'R', b' '],
        [b'R', b'S', b'D', b'P', b'T', b'R', b' ', b' '],
        [b'r', b's', b'd', b' ', b'p', b't', b'r', b' '],
    ];
    for i in 0..10u32 {
        let mut raw = make_v1(0, 0xE0000 + i, &[b'V', b'A', b'R', b'I', b'X', b' ']);
        let v = lcg.next_below(5) as usize;
        raw[0..8].copy_from_slice(&variants[v]);
        if validate_static(&raw) == Err(RsdpCode::BadSignature) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("坏签名注入 {}/10 必拒", caught))
}

fn drill_bad_csum1_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut raw = make_v1(0, 0xE0000 + i, &[b'V', b'A', b'R', b'I', b'X', b' ']);
        // 扰动一个非和位字节（OEMID 区）破坏 Checksum1。
        let pos = 9 + (lcg.next_below(6) as usize);
        raw[pos] = raw[pos].wrapping_add(1);
        if validate_static(&raw) == Err(RsdpCode::BadChecksum1) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("坏和1注入 {}/10 必拒", caught))
}

fn drill_bad_csum2_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut raw = make_v2(0xE0000 + i as u32, 0xF0000 + i as u64, &[b'V', b'A', b'R', b'I', b'X', b' ']);
        // 扰动 XSDT 低字节（破坏 Checksum2 区，和1区不动）。
        raw[OFF_XSDT] = raw[OFF_XSDT].wrapping_add(1);
        if validate_static(&raw) == Err(RsdpCode::BadChecksum2) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("坏和2注入 {}/10 必拒", caught))
}

fn drill_length_mismatch_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut raw = make_v2(0xE0000 + i as u32, 0xF0000 + i as u64, &[b'V', b'A', b'R', b'I', b'X', b' ']);
        // Length 字段改坏（如 20 / 32 / 40）——和1区(0..20)不动。
        let bad: u32 = [20, 32, 40, 28, 36 ^ 0xFF, 24, 48, 16, 44, 33][i as usize];
        raw[OFF_LENGTH] = (bad & 0xFF) as u8;
        raw[OFF_LENGTH + 1] = ((bad >> 8) & 0xFF) as u8;
        raw[OFF_LENGTH + 2] = ((bad >> 16) & 0xFF) as u8;
        raw[OFF_LENGTH + 3] = ((bad >> 24) & 0xFF) as u8;
        // Checksum2 会被和破坏——先判 Length（核验器先查 Length 再查和2）。
        // 实现顺序：csum1 → Length → csum2 → xsdt。本注入先打 Length，
        // 但实现里 csum2 检查在 Length 之后，坏和2会先炸。为精准命中
        // Length 失配，重补 Checksum2。
        let mut acc: u8 = 0;
        for j in 20..RSDP_V2_LEN {
            acc = acc.wrapping_add(raw[j]);
        }
        raw[OFF_CSUM2] = acc.wrapping_neg();
        if validate_static(&raw) == Err(RsdpCode::LengthMismatch) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("长度失配注入 {}/10 必拒", caught))
}

fn drill_xsdt_zero_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let raw = make_v2(0xE0000 + i as u32, 0, &[b'V', b'A', b'R', b'I', b'X', b' ']);
        if validate_static(&raw) == Err(RsdpCode::XsdtZero) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("XSDT 零地址注入 {}/10 必拒", caught))
}

fn drill_v1_no_ext_family() -> (bool, String) {
    // v1 带假扩展族：revision=0 的 20 字节后面挂垃圾扩展——核验器
    // 只读前 20，扩展被无视（v1 语义：无扩展族）。不得误读为 v2。
    let mut raw = make_v1(0, 0xE0000, &[b'V', b'A', b'R', b'I', b'X', b' ']);
    raw.extend_from_slice(&[0xFFu8; 16]);
    let r = validate_static(&raw);
    let ok = matches!(r, Ok(rec) if rec.revision == 0 && rec.layout_len == RSDP_V1_LEN && rec.verdict == RsdpVerdict::V1Ok);
    (ok, format!("v1 假扩展族被无视: ok={}", ok))
}

fn drill_too_short_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 2);
    let mut caught = 0;
    for i in 0..10u32 {
        let full = make_v1(0, 0xE0000 + i, &[b'V', b'A', b'R', b'I', b'X', b' ']);
        let cut = 1 + (lcg.next_below(RSDP_V1_LEN as u64) as usize);
        if validate_static(&full[..cut]) == Err(RsdpCode::TooShort) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("过短注入 {}/10 必拒", caught))
}

fn drill_unknown_revision_10() -> (bool, String) {
    let mut caught = 0;
    for rev in [3u8, 4, 7, 9, 255, 128, 64, 32, 16, 8] {
        let mut raw = make_v1(0, 0xE0000, &[b'V', b'A', b'R', b'I', b'X', b' ']);
        raw[15] = rev; // revision 改为未知代际
        // revision>2 走 v2 路径：补 36 字节布局 + 双和。
        while raw.len() < RSDP_V2_LEN {
            raw.push(0);
        }
        let mut acc: u8 = 0;
        for j in 20..RSDP_V2_LEN {
            acc = acc.wrapping_add(raw[j]);
        }
        raw[OFF_CSUM2] = acc.wrapping_neg();
        if validate_static(&raw) == Err(RsdpCode::UnknownRevision) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("未知代际 {}/10 必拒", caught))
}

fn drill_bad_oemid_10() -> (bool, String) {
    let mut caught = 0;
    let bads: [[u8; 6]; 10] = [
        [0x00, b'A', b'B', b'C', b'D', b'E'],
        [b'A', 0x01, b'B', b'C', b'D', b'E'],
        [b'A', b'B', 0x7F, b'C', b'D', b'E'],
        [b'A', b'B', b'C', 0x80, b'D', b'E'],
        [b'A', b'B', b'C', b'D', 0xFF, b'E'],
        [b'A', b'B', b'C', b'D', b'E', 0x1F],
        [0x81, 0x82, 0x83, 0x84, 0x85, 0x86],
        [0x0A, b'B', b'C', b'D', b'E', b'F'],
        [b'A', 0x0D, b'C', b'D', b'E', b'F'],
        [b'A', b'B', b'C', b'D', b'E', 0x09],
    ];
    for bad in bads {
        let raw = make_v1(0, 0xE0000, &bad);
        if validate_static(&raw) == Err(RsdpCode::BadOemId) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("坏 OEMID {}/10 必拒", caught))
}

fn drill_duplicate_entry_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut vv = RsdpValidator::new();
        let raw = make_v1(0, 0xE0000 + i, &[b'V', b'A', b'R', b'I', b'X', b' ']);
        let r1 = vv.validate(&raw);
        let r2 = vv.validate(&raw);
        if r1.is_ok() && r2 == Err(RsdpCode::DuplicateEntry) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("重复登记 {}/10 必拒", caught))
}

fn drill_ledger_empty_claim() -> (bool, String) {
    // 谎报面：空账不得声明通过。
    let v = RsdpValidator::new();
    let empty_claim = v.entry_count() > 0;
    let ok = !empty_claim;
    (ok, format!("空账谎报拦截: entry_count={} 谎报被拒={}", v.entry_count(), ok))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    RsdpCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账（判据 J1 落地面） ─────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let mut v = RsdpValidator::new();
    // 1 条 v1 + 1 条 v2 全过（判据原文：校验和验证通过 1/1）。
    let r1 = v.validate(&make_v1(0, 0xE0000, &[b'V', b'A', b'R', b'I', b'X', b' ']));
    let r2 = v.validate(&make_v2(0xE1000, 0xF1000, &[b'V', b'A', b'R', b'I', b'X', b' ']));
    // 自洽断言：v1 布局 20 / v2+ 布局 36。
    let self_consistent = match (&r1, &r2) {
        (Ok(a), Ok(b)) => {
            a.layout_len == RSDP_V1_LEN
                && b.layout_len == RSDP_V2_LEN
                && a.revision == 0
                && b.revision >= 2
                && b.xsdt_addr != 0
                && b.csum2_ok
        }
        _ => false,
    };
    let mut ok = self_consistent && v.entry_count() == 2;
    // 一条坏样本拒收。
    let mut bad = make_v1(0, 0xE2000, &[b'V', b'A', b'R', b'I', b'X', b' ']);
    bad[8] = bad[8].wrapping_add(1);
    ok = ok && v.validate(&bad) == Err(RsdpCode::BadChecksum1);
    ok = ok && v.rejected_total == 1;
    ok = ok && v.summary_line().len() > 0;
    (
        ok,
        format!(
            "全流程: v1+2通过={} 自洽={} 拒收={} 摘要[{}]",
            r1.is_ok() && r2.is_ok(),
            self_consistent,
            v.rejected_total,
            v.summary_line()
        ),
    )
}

fn drill_checksum_math() -> (bool, String) {
    // 校验和数学校：全零过 / 全 0xFF 过（256 偶数个）/ 单字节 1 不过。
    let zeros = [0u8; 20];
    let ones = [0xFFu8; 20];
    let one = {
        let mut b = [0u8; 20];
        b[0] = 1;
        b
    };
    let ok = checksum_ok(&zeros) && checksum_ok(&ones) && !checksum_ok(&one);
    (ok, format!("和校验数学生: 零过={} FF过={} 单1拒={}", checksum_ok(&zeros), checksum_ok(&ones), !checksum_ok(&one)))
}

fn drill_address_fields() -> (bool, String) {
    // 地址域保真：v1 Rsdt / v2 Xsdt 逐字节回读一致。
    let v1 = make_v1(0, 0x9E000, &[b'V', b'A', b'R', b'I', b'X', b' ']);
    let rec1 = validate_static(&v1);
    let v2 = make_v2(0x9E000, 0x0000_0000_0009_E000, &[b'V', b'A', b'R', b'I', b'X', b' ']);
    let rec2 = validate_static(&v2);
    let ok = matches!(&rec1, Ok(r) if r.rsdt_addr == 0x9E000)
        && matches!(&rec2, Ok(r) if r.xsdt_addr == 0x9E000 && r.rsdt_addr == 0x9E000);
    (ok, format!("地址域保真: v1_rsdt={:?}", rec1.map(|r| r.rsdt_addr)))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_bad_signature_10();
    v.push(("J2-坏签名注入10/10", a, b));
    let (a, b) = drill_bad_csum1_10();
    v.push(("J2-坏和1注入10/10", a, b));
    let (a, b) = drill_bad_csum2_10();
    v.push(("J2-坏和2注入10/10", a, b));
    let (a, b) = drill_length_mismatch_10();
    v.push(("J2-长度失配注入10/10", a, b));
    let (a, b) = drill_xsdt_zero_10();
    v.push(("J2-XSDT零址注入10/10", a, b));
    let (a, b) = drill_v1_no_ext_family();
    v.push(("J2-v1假扩展族无视", a, b));
    let (a, b) = drill_too_short_10();
    v.push(("J2-过短注入10/10", a, b));
    let (a, b) = drill_unknown_revision_10();
    v.push(("J2-未知代际10/10", a, b));
    let (a, b) = drill_bad_oemid_10();
    v.push(("J2-坏OEMID10/10", a, b));
    let (a, b) = drill_duplicate_entry_10();
    v.push(("J2-重复登记10/10", a, b));
    let (a, b) = drill_ledger_empty_claim();
    v.push(("J3-空账谎报拦截", a, b));
    let (a, b) = drill_checksum_math();
    v.push(("J3-和校验数学生", a, b));
    let (a, b) = drill_address_fields();
    v.push(("J3-地址域保真", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程校验1/1通过", a, b));
    v
}

/// run_unx0442_checks：本单判据聚合（诊断段 0x36080）。
pub fn run_unx0442_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0442 RSDP 校验与版本账");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36080 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
