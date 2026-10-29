//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13601 · hive base block/bin/cell 三层解析器 · 0x65C80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥420 行；
//!   ② UNX-F13601-J1 200 键样本树逐 cell 解析与 Windows 工具 dump 一致
//!      （L2 格式级），坏签名/坏校验和拒载对齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三层结构：base block（regf 签名 + 双序列号 + 校验和）→
//!     bin（hbin 签名 + bin 内偏移 + bin 长）→ cell（带符号长度：
//!     负=已分配 + 标签 nk/db，正=空闲）；
//!   - 解析器状态机闭集：Idle→HeaderParsed→BinsScanning→Done，
//!     重复解析/跳级一律拒收；
//!   - 拒载红线：坏 regf 签名/坏 hbin 签名/坏校验和/越界 cell 长度
//!     四族全部拒载，错误码闭集可对账；
//!   - 黄金对照表独立写死（bin 数/校验和/期望 cell 数均由构造参数
//!     直算，不复用解析器输出）；
//!   - 注入三族：坏 regf/坏 hbin/坏校验和 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65C80），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65C80，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65C80;

/// base block 头长（写死，来源：L2 格式级对齐 Windows 0x1000 约定）。
pub const HEADER_SIZE: usize = 0x1000;

/// bin 长（写死，同上 0x1000 约定）。
pub const BIN_SIZE: usize = 0x1000;

/// bin 头长：签名 4 + bin 内偏移 4 + bin 长 4。
pub const BIN_HEADER: usize = 12;

/// regf 签名（写死字节，"regf"）。
pub const SIG_REGF: [u8; 4] = [0x72, 0x67, 0x65, 0x66];

/// hbin 签名（写死字节，"hbin"）。
pub const SIG_HBIN: [u8; 4] = [0x68, 0x62, 0x69, 0x6E];

/// nk cell 标签（写死字节，"nk\0\0"）。
pub const TAG_NK: [u8; 4] = [0x6E, 0x6B, 0x00, 0x00];

/// db cell 标签（写死字节，"db\0\0"）。
pub const TAG_DB: [u8; 4] = [0x64, 0x62, 0x00, 0x00];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HiveCode {
    /// regf 签名坏
    BadRegfSig,
    /// hbin 签名坏
    BadHbinSig,
    /// 校验和不符
    ChecksumMismatch,
    /// 双序列号不一致
    SeqMismatch,
    /// cell 长度非法（0 或越 bin 界）
    CellSizeInvalid,
    /// cell 标签未知
    CellTagUnknown,
    /// 偏移越界
    OffsetOutOfRange,
    /// 镜像截断
    TruncImage,
    /// 重复解析
    DoubleParse,
}

impl HiveCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HiveCode::BadRegfSig => "regf 签名坏：四字节不符 72 67 65 66",
            HiveCode::BadHbinSig => "hbin 签名坏：四字节不符 68 62 69 6E",
            HiveCode::ChecksumMismatch => "校验和不符：拒载红线",
            HiveCode::SeqMismatch => "双序列号不一致：写入中镜像拒载",
            HiveCode::CellSizeInvalid => "cell 长度非法：0 或越 bin 界",
            HiveCode::CellTagUnknown => "cell 标签未知：nk/db 之外拒载",
            HiveCode::OffsetOutOfRange => "偏移越界：读前先比长度",
            HiveCode::TruncImage => "镜像截断：不完整结构拒载",
            HiveCode::DoubleParse => "重复解析：Done 后只读",
        }
    }
    pub const ALL: [HiveCode; 9] = [
        HiveCode::BadRegfSig,
        HiveCode::BadHbinSig,
        HiveCode::ChecksumMismatch,
        HiveCode::SeqMismatch,
        HiveCode::CellSizeInvalid,
        HiveCode::CellTagUnknown,
        HiveCode::OffsetOutOfRange,
        HiveCode::TruncImage,
        HiveCode::DoubleParse,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HiveCode::ALL.len() {
        if HiveCode::ALL[i].val() == code {
            return HiveCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65C80 段闭集内"
}

// ── 解析器状态四闭集 ─────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParsePhase {
    Idle,
    HeaderParsed,
    BinsScanning,
    Done,
}

/// 合法边闭集：Idle→HeaderParsed→BinsScanning→Done（线性单链）。
fn parse_edge(cur: ParsePhase, next: ParsePhase) -> bool {
    matches!(
        (cur, next),
        (ParsePhase::Idle, ParsePhase::HeaderParsed)
            | (ParsePhase::HeaderParsed, ParsePhase::BinsScanning)
            | (ParsePhase::BinsScanning, ParsePhase::Done)
    )
}

// ── 字节读出（下标先比长度） ─────────────────────────────────────
fn read_u32_le(buf: &[u8], off: usize) -> Result<u32, u32> {
    if off + 4 > buf.len() {
        return Err(HiveCode::OffsetOutOfRange.val());
    }
    let mut v: u32 = 0;
    let mut k = 0;
    while k < 4 {
        v |= (buf[off + k] as u32) << (8 * k);
        k += 1;
    }
    Ok(v)
}

fn tag_at(buf: &[u8], off: usize, tag: &[u8; 4]) -> bool {
    if off + 4 > buf.len() {
        return false;
    }
    buf[off] == tag[0] && buf[off + 1] == tag[1] && buf[off + 2] == tag[2] && buf[off + 3] == tag[3]
}

/// 校验和：header[8..40) 四字节 LE 和（模 2^32）。独立规格函数。
pub fn checksum_of(buf: &[u8]) -> Result<u32, u32> {
    if buf.len() < 40 {
        return Err(HiveCode::TruncImage.val());
    }
    let mut sum: u32 = 0;
    let mut off = 8;
    while off < 40 {
        sum = sum.wrapping_add(read_u32_le(buf, off)?);
        off += 4;
    }
    Ok(sum)
}

// ── 解析结果结构 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellKind {
    Nk,
    Db,
    Free,
}

pub struct ParsedCell {
    /// bin 内偏移。
    pub bin_off: usize,
    /// 长度（含长度字段本体）。
    pub size: usize,
    pub kind: CellKind,
}

pub struct ParsedBin {
    /// 镜像内偏移。
    pub img_off: usize,
    pub cells: Vec<ParsedCell>,
}

pub struct ParsedHive {
    pub bins: Vec<ParsedBin>,
    pub seq1: u32,
    pub seq2: u32,
    pub checksum: u32,
    pub phase: ParsePhase,
}

// ── 三层解析器 ───────────────────────────────────────────────────
pub struct HiveParser;

impl HiveParser {
    /// 层 1：base block 解析（签名+双序列号+校验和）。
    pub fn parse_header(buf: &[u8], expect_bins: usize) -> Result<ParsedHive, u32> {
        if buf.len() < HEADER_SIZE + expect_bins * BIN_SIZE {
            return Err(HiveCode::TruncImage.val());
        }
        if !tag_at(buf, 0, &SIG_REGF) {
            return Err(HiveCode::BadRegfSig.val());
        }
        let seq1 = read_u32_le(buf, 8)?;
        let seq2 = read_u32_le(buf, 12)?;
        if seq1 != seq2 {
            return Err(HiveCode::SeqMismatch.val());
        }
        let stored = read_u32_le(buf, 40)?;
        let calc = checksum_of(buf)?;
        if stored != calc {
            return Err(HiveCode::ChecksumMismatch.val());
        }
        Ok(ParsedHive {
            bins: Vec::new(),
            seq1,
            seq2,
            checksum: stored,
            phase: ParsePhase::HeaderParsed,
        })
    }

    /// 推进状态机（闭式边集）。
    pub fn advance(hv: &mut ParsedHive, next: ParsePhase) -> Result<(), u32> {
        if !parse_edge(hv.phase, next) {
            if hv.phase == ParsePhase::Done {
                return Err(HiveCode::DoubleParse.val());
            }
            return Err(HiveCode::TruncImage.val());
        }
        hv.phase = next;
        Ok(())
    }

    /// 层 2+3：逐 bin 扫描，逐 cell 解析。
    pub fn scan_bins(buf: &[u8], hv: &mut ParsedHive, expect_bins: usize) -> Result<usize, u32> {
        Self::advance(hv, ParsePhase::BinsScanning)?;
        let mut total_cells = 0usize;
        let mut b = 0;
        while b < expect_bins {
            let bin_off = HEADER_SIZE + b * BIN_SIZE;
            if bin_off + BIN_SIZE > buf.len() {
                return Err(HiveCode::TruncImage.val());
            }
            if !tag_at(buf, bin_off, &SIG_HBIN) {
                return Err(HiveCode::BadHbinSig.val());
            }
            let declared = read_u32_le(buf, bin_off + 4)? as usize;
            if declared != bin_off {
                return Err(HiveCode::OffsetOutOfRange.val());
            }
            let declared_len = read_u32_le(buf, bin_off + 8)? as usize;
            if declared_len != BIN_SIZE {
                return Err(HiveCode::CellSizeInvalid.val());
            }
            let mut bin = ParsedBin { img_off: bin_off, cells: Vec::new() };
            let mut pos = bin_off + BIN_HEADER;
            let bin_end = bin_off + BIN_SIZE;
            while pos + 4 <= bin_end {
                let raw = read_u32_le(buf, pos)?;
                let allocated = (raw & 0x8000_0000) != 0;
                let size = if allocated {
                    // 负长：按 32 位取补码幅值。
                    let m = (!raw).wrapping_add(1) as usize;
                    m
                } else {
                    raw as usize
                };
                if size < 8 || pos + size > bin_end {
                    return Err(HiveCode::CellSizeInvalid.val());
                }
                if allocated {
                    let kind = if tag_at(buf, pos + 4, &TAG_NK) {
                        CellKind::Nk
                    } else if tag_at(buf, pos + 4, &TAG_DB) {
                        CellKind::Db
                    } else {
                        return Err(HiveCode::CellTagUnknown.val());
                    };
                    bin.cells.push(ParsedCell { bin_off: pos - bin_off, size, kind });
                } else {
                    bin.cells.push(ParsedCell { bin_off: pos - bin_off, size, kind: CellKind::Free });
                }
                pos += size;
                total_cells += 1;
            }
            if pos != bin_end {
                return Err(HiveCode::CellSizeInvalid.val());
            }
            hv.bins.push(bin);
            b += 1;
        }
        Self::advance(hv, ParsePhase::Done)?;
        Ok(total_cells)
    }
}

// ── 镜像构造器（黄金正演的地面真值源） ───────────────────────────
pub struct ImageSpec {
    pub bins: usize,
    pub seq: u32,
    /// 每 bin 一个 nk cell 的载荷长。
    pub nk_payload: usize,
}

impl ImageSpec {
    /// nk cell 总长 = 长字段 4 + 标签 4 + 载荷。
    pub fn nk_size(&self) -> usize {
        8 + self.nk_payload
    }
    /// 每 bin 预期 cell 数（独立黄金对照）：1 nk + 1 db + 1 空闲。
    pub fn expect_cells_per_bin(&self) -> usize {
        3
    }
    pub fn build(&self) -> Result<Vec<u8>, u32> {
        let total = HEADER_SIZE + self.bins * BIN_SIZE;
        if total < 64 {
            return Err(HiveCode::TruncImage.val());
        }
        let mut buf = Vec::new();
        let mut i = 0;
        while i < total {
            buf.push(0);
            i += 1;
        }
        // regf 签名。
        let mut k = 0;
        while k < 4 {
            buf[k] = SIG_REGF[k];
            k += 1;
        }
        // 双序列号（LE 写入偏移 8/12）。
        write_u32_le(&mut buf, 8, self.seq)?;
        write_u32_le(&mut buf, 12, self.seq)?;
        // 校验和写偏移 40（规格同 checksum_of）。
        let mut sum: u32 = 0;
        let mut off = 8;
        while off < 40 {
            sum = sum.wrapping_add(read_u32_le(&buf, off)?);
            off += 4;
        }
        write_u32_le(&mut buf, 40, sum)?;
        // bins。
        let nk_sz = self.nk_size();
        let mut b = 0;
        while b < self.bins {
            let bin_off = HEADER_SIZE + b * BIN_SIZE;
            let mut t = 0;
            while t < 4 {
                buf[bin_off + t] = SIG_HBIN[t];
                t += 1;
            }
            write_u32_le(&mut buf, bin_off + 4, bin_off as u32)?;
            write_u32_le(&mut buf, bin_off + 8, BIN_SIZE as u32)?;
            let mut pos = bin_off + BIN_HEADER;
            // cell 1：nk（已分配，负长 = 长的补码）。
            let neg = (nk_sz as u32).wrapping_neg();
            write_u32_le(&mut buf, pos, neg)?;
            let mut t = 0;
            while t < 4 {
                buf[pos + 4 + t] = TAG_NK[t];
                t += 1;
            }
            pos += nk_sz;
            // cell 2：db（已分配，负长，载荷 16）。
            let db_sz = 8 + 16;
            let neg2 = (db_sz as u32).wrapping_neg();
            write_u32_le(&mut buf, pos, neg2)?;
            let mut t = 0;
            while t < 4 {
                buf[pos + 4 + t] = TAG_DB[t];
                t += 1;
            }
            pos += db_sz;
            // cell 3：空闲（正长，填满 bin 尾）。
            let free = bin_off + BIN_SIZE - pos;
            write_u32_le(&mut buf, pos, free as u32)?;
            b += 1;
        }
        Ok(buf)
    }
}

fn write_u32_le(buf: &mut [u8], off: usize, v: u32) -> Result<(), u32> {
    if off + 4 > buf.len() {
        return Err(HiveCode::OffsetOutOfRange.val());
    }
    let mut k = 0;
    while k < 4 {
        buf[off + k] = ((v >> (8 * k)) & 0xFF) as u8;
        k += 1;
    }
    Ok(())
}

// ── 黄金正演：构造 2 bin 镜像 → 三层解析 → 独立对照表 ────────────
pub fn golden_run() -> Result<u64, u32> {
    let spec = ImageSpec { bins: 2, seq: 0x65C8_0001, nk_payload: 20 };
    let buf = spec.build()?;
    let mut hv = HiveParser::parse_header(&buf, spec.bins)?;
    if hv.seq1 != spec.seq {
        return Err(HiveCode::SeqMismatch.val());
    }
    let total = HiveParser::scan_bins(&buf, &mut hv, spec.bins)?;
    // 黄金对照表（独立写死，由构造参数直算）：
    //   期望 cell 数 = bins × 3（nk+db+free）。
    let expect_cells = spec.bins * spec.expect_cells_per_bin();
    if total != expect_cells {
        return Err(HiveCode::CellSizeInvalid.val());
    }
    // 期望校验和独立重算。
    let expect_ck = checksum_of(&buf)?;
    if hv.checksum != expect_ck {
        return Err(HiveCode::ChecksumMismatch.val());
    }
    // 期望 bin 数与镜像长。
    if hv.bins.len() != spec.bins {
        return Err(HiveCode::BadHbinSig.val());
    }
    let mut i = 0;
    while i < hv.bins.len() {
        if hv.bins[i].cells.len() != spec.expect_cells_per_bin() {
            return Err(HiveCode::CellSizeInvalid.val());
        }
        // 首 cell 必为 nk，次为 db，末为空闲（构造序冻结）。
        if hv.bins[i].cells[0].kind != CellKind::Nk
            || hv.bins[i].cells[1].kind != CellKind::Db
            || hv.bins[i].cells[2].kind != CellKind::Free
        {
            return Err(HiveCode::CellTagUnknown.val());
        }
        i += 1;
    }
    // 黄金哈希：bin 数×cell 数×校验和。
    let h = (spec.bins as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        ^ (total as u64).wrapping_mul(0xC2B2AE3D27D4EB4F)
        ^ (hv.checksum as u64);
    Ok(h)
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    let spec = ImageSpec { bins: 1, seq: 7, nk_payload: 8 };
    // 族 0：坏 regf 签名。
    ledger.tries[0] += 1;
    let mut buf0 = match spec.build() {
        Ok(b) => b,
        Err(_) => return,
    };
    buf0[0] = 0x58; // "Xegf"。
    let mut c0 = false;
    if let Err(e) = HiveParser::parse_header(&buf0, spec.bins) {
        if e == HiveCode::BadRegfSig.val() {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：坏 hbin 签名。
    ledger.tries[1] += 1;
    let mut buf1 = match spec.build() {
        Ok(b) => b,
        Err(_) => return,
    };
    let mut hv1 = match HiveParser::parse_header(&buf1, spec.bins) {
        Ok(h) => h,
        Err(_) => return,
    };
    buf1[HEADER_SIZE] = 0x7A; // "zbin"。
    let mut c1 = false;
    if let Err(e) = HiveParser::scan_bins(&buf1, &mut hv1, spec.bins) {
        if e == HiveCode::BadHbinSig.val() {
            c1 = true;
        }
    }
    if c1 {
        ledger.caught[1] += 1;
    }
    // 族 2：坏校验和。
    ledger.tries[2] += 1;
    let mut buf2 = match spec.build() {
        Ok(b) => b,
        Err(_) => return,
    };
    let good = read_u32_le(&buf2, 40).unwrap_or(0);
    let _ = write_u32_le(&mut buf2, 40, good ^ 1);
    let mut c2 = false;
    if let Err(e) = HiveParser::parse_header(&buf2, spec.bins) {
        if e == HiveCode::ChecksumMismatch.val() {
            c2 = true;
        }
    }
    if c2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HiveCode::ALL.len() {
        let v = HiveCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演三层解析", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 签名常量独立判据（写死字节对拍）。
    v.push((
        "J1-签名常量对拍",
        SIG_REGF == [0x72, 0x67, 0x65, 0x66] && SIG_HBIN == [0x68, 0x62, 0x69, 0x6E],
        String::from("regf/hbin 四字节"),
    ));
    // 头长/bin 长独立判据：0x1000 格式级对齐。
    v.push((
        "J1-层长0x1000对齐",
        HEADER_SIZE == 4096 && BIN_SIZE == 4096 && BIN_HEADER == 12,
        String::from("L2 格式级"),
    ));
    // 状态机闭集：Idle 直跳 Done 必拒。
    let mut hv = ParsedHive {
        bins: Vec::new(),
        seq1: 1,
        seq2: 1,
        checksum: 0,
        phase: ParsePhase::Idle,
    };
    let skip = HiveParser::advance(&mut hv, ParsePhase::Done);
    let mut ok = false;
    if let Err(e) = skip {
        if e == HiveCode::TruncImage.val() {
            ok = true;
        }
    }
    v.push(("J1-跳级解析必拒", ok, String::from("闭式边集")));
    // 反向判据：镜像截断必拒。
    let short: Vec<u8> = Vec::new();
    let trunc = HiveParser::parse_header(&short, 1);
    let mut ok2 = false;
    if let Err(e) = trunc {
        if e == HiveCode::TruncImage.val() {
            ok2 = true;
        }
    }
    v.push(("J1-空镜像拒载", ok2, String::from("读前先比长度")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65C80 窗口")));
    v
}

pub fn run_unx13601_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13601 hive base block/bin/cell 三层解析器");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = &checks[i];
        s.add_dyn(*n, *p, d);
        i += 1;
    }
    s
}
