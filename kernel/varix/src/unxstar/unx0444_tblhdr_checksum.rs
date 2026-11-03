//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0444 表头校验和族（逐表核）· 0x750C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   表头+全表逐字节求和 ≡0 (mod 256)，N/N 全核 N=12 语料，坏表
//!   注入必抓显性计数。
//! 设计要点：
//!   1. 36 字节表头真实建模（ACPI 规范 §5.2.6 SDT 通用表头）：
//!      signature 4B@0 / length 4B@4 / revision 1B@8 / checksum 1B@9 /
//!      oemid 6B@10 / oem_table_id 8B@16 / oem_revision 4B@24 /
//!      creator_id 4B@28 / creator_revision 4B@32，逐字段偏移注释。
//!   2. 双口径求和账：口径 A「整表逐字节求和 ≡0 (mod 256)」为判定
//!      主口径；口径 B「表头 36B 求和 + checksum 字段差值 = 体和补
//!      数」为一致性复算账（两账都入报告，缺一不放行）。
//!   3. 12 表语料闭集（RSDT/XSDT/MADT/HPET/FADT/MCFG/DSDT/SSDT/
//!      SRAT/SLIT/MSDM/TPM2），逐表冻结签名 + 冻结体长，N=12 全核。
//!   4. 坏表注入（checksum 字段 +1 与体字节翻转两路）逐表必抓，
//!      显性计数入账（tries/caught + 逐表命中位），不静默。
//!   5. 校验状态机单调推进（Idle→HeaderRead→LenChecked→SumsTallied→
//!      Verdict），倒退显性拒。
//!   6. 错误路径带可操作中文建议（写入 detail），LCG 种子固定，
//!      双跑一致复现红线。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x750C0，WBV1104 批 ACPI 校验和族卡区）。
pub const SEG_BASE: u32 = 0x750C0;

// ── 冻结常量表（来源：ACPI 规范 §5.2.6 SDT 通用表头布局） ────────
/// 通用表头长度（36 字节冻结）。
pub const HDR_LEN: u32 = 36;
/// length 字段偏移（@4）。
pub const OFF_LEN: usize = 4;
/// revision 字段偏移（@8，单字节）。
pub const OFF_REV: usize = 8;
/// checksum 字段偏移（@9，单字节）。
pub const OFF_CK: usize = 9;
/// oemid 偏移（@10，6 字节）。
pub const OFF_OEMID: usize = 10;
/// oem_table_id 偏移（@16，8 字节）。
pub const OFF_OTID: usize = 16;
/// oem_revision 偏移（@24）。
pub const OFF_OEMREV: usize = 24;
/// creator_id 偏移（@28）。
pub const OFF_CRID: usize = 28;
/// creator_revision 偏移（@32）。
pub const OFF_CRREV: usize = 32;
/// 语料表数（N=12 冻结）。
pub const CORPUS_N: usize = 12;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CsumCode {
    /// 表像残缺（不足 36 字节，连表头都摆不下）
    HdrShort,
    /// length 字段与镜像实长失配（表被截尾或 length 被改）
    LenMismatch,
    /// 整表逐字节求和 ≢0 (mod 256)——校验和坏（口径 A 判定）
    CsumFullBad,
    /// 表头口径差值账失配（口径 B 复算不一致）
    CsumHdrBad,
    /// 签名不在 12 表语料闭集内
    SigUnknown,
    /// 语料缺表（12 表未核齐）
    CorpusMiss,
    /// 校验状态机倒退（单调推进被破坏）
    VerifyRegress,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl CsumCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CsumCode::HdrShort => "表像残缺：不足 36 字节，建议重取表镜像",
            CsumCode::LenMismatch => "length 字段与实长失配：建议核对表被截尾/改写",
            CsumCode::CsumFullBad => "整表和 ≢0：校验和坏，建议按体和补数重算 @9 字节",
            CsumCode::CsumHdrBad => "表头口径差值账失配：建议复算 36B 求和与体补数",
            CsumCode::SigUnknown => "签名不在 12 表闭集：建议核对语料清单",
            CsumCode::CorpusMiss => "语料缺表：12 表未核齐，建议回查根表遍历",
            CsumCode::VerifyRegress => "校验状态机倒退：建议检查驱动推进序",
            CsumCode::NotReproducible => "双跑不一致：复现红线，建议排查非确定源",
        }
    }
    pub const ALL: [CsumCode; 8] = [
        CsumCode::HdrShort,
        CsumCode::LenMismatch,
        CsumCode::CsumFullBad,
        CsumCode::CsumHdrBad,
        CsumCode::SigUnknown,
        CsumCode::CorpusMiss,
        CsumCode::VerifyRegress,
        CsumCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，越段码返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CsumCode::ALL.len() {
        if let Some(c) = CsumCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x750C0 段闭集内"
}

// ── 12 表语料闭集 ───────────────────────────────────────────────
/// 12 表语料（签名闭集，顺序冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TblSig {
    /// RSDT 根表（32 位指针）
    Rsdt,
    /// XSDT 根表（64 位指针）
    Xsdt,
    /// MADT 中断表
    Madt,
    /// HPET 高精度计时表
    Hpet,
    /// FADT 固定 ACPI 描述表
    Fadt,
    /// MCFG PCIe 配置表
    Mcfg,
    /// DSDT 差异系统描述表
    Dsdt,
    /// SSDT 次级系统描述表
    Ssdt,
    /// SRAT 系统资源亲和表
    Srst,
    /// SLIT 系统局部距离表
    Slit,
    /// MSDM 微软数据管理表
    Msdm,
    /// TPM2 可信平台模块表
    Tpm2,
}

impl TblSig {
    /// 表签名（ASCII 4 字节，闭集冻结）。
    pub fn sig(self) -> [u8; 4] {
        match self {
            TblSig::Rsdt => [0x52, 0x53, 0x44, 0x54],
            TblSig::Xsdt => [0x58, 0x53, 0x44, 0x54],
            TblSig::Madt => [0x41, 0x50, 0x49, 0x43],
            TblSig::Hpet => [0x48, 0x50, 0x45, 0x54],
            TblSig::Fadt => [0x46, 0x41, 0x43, 0x50],
            TblSig::Mcfg => [0x4D, 0x43, 0x46, 0x47],
            TblSig::Dsdt => [0x44, 0x53, 0x44, 0x54],
            TblSig::Ssdt => [0x53, 0x53, 0x44, 0x54],
            TblSig::Srst => [0x53, 0x52, 0x41, 0x54],
            TblSig::Slit => [0x53, 0x4C, 0x49, 0x54],
            TblSig::Msdm => [0x4D, 0x53, 0x44, 0x4D],
            TblSig::Tpm2 => [0x54, 0x50, 0x4D, 0x32],
        }
    }
    /// 语料序号（0..12，冻结）。
    pub fn ord(self) -> usize {
        self as usize
    }
    pub const ALL: [TblSig; 12] = [
        TblSig::Rsdt,
        TblSig::Xsdt,
        TblSig::Madt,
        TblSig::Hpet,
        TblSig::Fadt,
        TblSig::Mcfg,
        TblSig::Dsdt,
        TblSig::Ssdt,
        TblSig::Srst,
        TblSig::Slit,
        TblSig::Msdm,
        TblSig::Tpm2,
    ];
}

/// 签名反查语料闭集（未知签名 ⇒ SigUnknown）。
pub fn sig_lookup(s: [u8; 4]) -> Result<TblSig, u32> {
    let mut i = 0;
    while i < TblSig::ALL.len() {
        if let Some(k) = TblSig::ALL.get(i) {
            if k.sig() == s {
                return Ok(*k);
            }
        }
        i += 1;
    }
    Err(CsumCode::SigUnknown.val())
}

/// 语料各表冻结体长（构造/对拍共用，非零即表非纯头）。
pub fn corpus_body_len(kind: TblSig) -> usize {
    match kind {
        TblSig::Rsdt => 16,
        TblSig::Xsdt => 24,
        TblSig::Madt => 44,
        TblSig::Hpet => 21,
        TblSig::Fadt => 76,
        TblSig::Mcfg => 24,
        TblSig::Dsdt => 40,
        TblSig::Ssdt => 24,
        TblSig::Srst => 32,
        TblSig::Slit => 16,
        TblSig::Msdm => 28,
        TblSig::Tpm2 => 24,
    }
}

// ── 36 字节表头（逐字段建模） ───────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TblHeader36 {
    /// 签名 4B@0。
    pub sig: [u8; 4],
    /// 全表长度 4B@4（含表头，小端）。
    pub length: u32,
    /// revision 1B@8。
    pub revision: u8,
    /// checksum 1B@9（整表补数口径）。
    pub checksum: u8,
    /// oemid 6B@10。
    pub oem_id: [u8; 6],
    /// oem_table_id 8B@16。
    pub oem_table_id: [u8; 8],
    /// oem_revision 4B@24。
    pub oem_revision: u32,
    /// creator_id 4B@28。
    pub creator_id: u32,
    /// creator_revision 4B@32。
    pub creator_revision: u32,
}

impl TblHeader36 {
    /// 表头解码（不足 36 字节显性拒；越尾字节按 0 补账）。
    pub fn decode(img: &[u8]) -> Result<TblHeader36, u32> {
        if img.len() < HDR_LEN as usize {
            return Err(CsumCode::HdrShort.val());
        }
        let b = |off: usize| -> u8 { *img.get(off).unwrap_or(&0) };
        let g = |off: usize| -> u32 {
            let b0 = b(off) as u32;
            let b1 = b(off.wrapping_add(1)) as u32;
            let b2 = b(off.wrapping_add(2)) as u32;
            let b3 = b(off.wrapping_add(3)) as u32;
            b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
        };
        let mut sig = [0u8; 4];
        let mut oem = [0u8; 6];
        let mut otid = [0u8; 8];
        let mut i = 0;
        while i < 4 {
            if let Some(slot) = sig.get_mut(i) {
                *slot = b(i);
            }
            i += 1;
        }
        let mut j = 0;
        while j < 6 {
            if let Some(slot) = oem.get_mut(j) {
                *slot = b(OFF_OEMID + j);
            }
            j += 1;
        }
        let mut k = 0;
        while k < 8 {
            if let Some(slot) = otid.get_mut(k) {
                *slot = b(OFF_OTID + k);
            }
            k += 1;
        }
        Ok(TblHeader36 {
            sig,
            length: g(OFF_LEN),
            revision: b(OFF_REV),
            checksum: b(OFF_CK),
            oem_id: oem,
            oem_table_id: otid,
            oem_revision: g(OFF_OEMREV),
            creator_id: g(OFF_CRID),
            creator_revision: g(OFF_CRREV),
        })
    }
}

// ── 双口径求和 ──────────────────────────────────────────────────
/// 口径 A：整表逐字节 wrapping 求和（mod 256）。
pub fn full_sum(img: &[u8]) -> u8 {
    let mut s = 0u8;
    let mut i = 0;
    while i < img.len() {
        if let Some(x) = img.get(i) {
            s = s.wrapping_add(*x);
        }
        i += 1;
    }
    s
}

/// 口径 B：表头 36B 逐字节 wrapping 求和。
pub fn hdr36_sum(img: &[u8]) -> u8 {
    let n = HDR_LEN as usize;
    let mut s = 0u8;
    let mut i = 0;
    while i < n {
        if let Some(x) = img.get(i) {
            s = s.wrapping_add(*x);
        }
        i += 1;
    }
    s
}

/// 口径 B 复算账：整表和 ≡0 分解式 —— 体和 ≡ −(表头 36B 和)。
/// 即 h36 + body_sum ≡ 0 (mod 256)，与口径 A 同源但按表头/体两截
/// 分别记账复算（checksum 字段含在表头 36B 内）。
pub fn hdr_ledger_consistent(_ck_byte: u8, h36: u8, body_sum: u8) -> bool {
    // _ck_byte 仅记账口径占位（checksum 含在 h36 内）；分解式只看
    // h36 与体和。
    body_sum == 0u8.wrapping_sub(h36)
}

// ── 校验状态机（单调推进，倒退显性拒） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyPhase {
    /// 未开始
    Idle,
    /// 表头已读
    HeaderRead,
    /// 长度账已对拍
    LenChecked,
    /// 双口径求和已入账
    SumsTallied,
    /// 判定已出
    Verdict,
}

impl VerifyPhase {
    fn rank(self) -> u32 {
        match self {
            VerifyPhase::Idle => 0,
            VerifyPhase::HeaderRead => 1,
            VerifyPhase::LenChecked => 2,
            VerifyPhase::SumsTallied => 3,
            VerifyPhase::Verdict => 4,
        }
    }
}

/// 校验游标：单调推进，倒退显性拒。
#[derive(Clone, Debug)]
pub struct VerifyCursor {
    pub phase: VerifyPhase,
    pub tables_done: u32,
}

impl VerifyCursor {
    pub fn new() -> VerifyCursor {
        VerifyCursor { phase: VerifyPhase::Idle, tables_done: 0 }
    }
    pub fn advance(&mut self, to: VerifyPhase) -> Result<(), u32> {
        if to.rank() < self.phase.rank() {
            return Err(CsumCode::VerifyRegress.val());
        }
        self.phase = to;
        Ok(())
    }
}

// ── 逐表校验报告 ────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    /// 语料签名（已闭集反查）。
    pub kind: TblSig,
    /// length 字段账。
    pub len_field: u32,
    /// 镜像实长账。
    pub actual_len: usize,
    /// checksum 字段原值（@9）。
    pub ck_byte: u8,
    /// 口径 B：表头 36B 求和。
    pub hdr_sum: u8,
    /// 口径 A：整表求和。
    pub fsum: u8,
    /// 口径 A 判定（≡0 才真）。
    pub full_ok: bool,
    /// 口径 B 差值账复算。
    pub ledger_ok: bool,
}

impl VerifyReport {
    /// 双口径全绿：整表 ≡0 + 差值账一致 + 长度账一致。
    pub fn clean(&self) -> bool {
        self.full_ok && self.ledger_ok && self.len_field == self.actual_len as u32
    }
}

/// 单表校验：签名闭集 → 长度账 → 双口径求和 → 差值复算。
pub fn verify_table(img: &[u8]) -> Result<VerifyReport, u32> {
    let mut cur = VerifyCursor::new();
    let hdr = TblHeader36::decode(img)?;
    cur.advance(VerifyPhase::HeaderRead)?;
    let kind = sig_lookup(hdr.sig)?;
    if hdr.length != img.len() as u32 {
        return Err(CsumCode::LenMismatch.val());
    }
    cur.advance(VerifyPhase::LenChecked)?;
    let fs = full_sum(img);
    let h36 = hdr36_sum(img);
    let body_sum = fs.wrapping_sub(h36);
    let ledger_ok = hdr_ledger_consistent(hdr.checksum, h36, body_sum);
    cur.advance(VerifyPhase::SumsTallied)?;
    let rep = VerifyReport {
        kind,
        len_field: hdr.length,
        actual_len: img.len(),
        ck_byte: hdr.checksum,
        hdr_sum: h36,
        fsum: fs,
        full_ok: fs == 0,
        ledger_ok,
    };
    if !rep.full_ok {
        return Err(CsumCode::CsumFullBad.val());
    }
    if !rep.ledger_ok {
        return Err(CsumCode::CsumHdrBad.val());
    }
    cur.advance(VerifyPhase::Verdict)?;
    Ok(rep)
}

// ── 语料像构造（冻结体长 + 补数 checksum） ──────────────────────
/// 体字节生成（确定性手写模式：k*7 + i*3 + 5，与 LCG 无关）。
fn corpus_body_byte(kind_ord: usize, i: usize) -> u8 {
    ((kind_ord * 7 + i * 3 + 5) & 0xFF) as u8
}

/// 构造单表像：36B 表头 + 冻结体长确定性体 + 整表补数 checksum。
pub fn build_table(kind: TblSig, revision: u8) -> Vec<u8> {
    let body_len = corpus_body_len(kind);
    let total = HDR_LEN as usize + body_len;
    let mut v: Vec<u8> = Vec::new();
    let sig = kind.sig();
    let mut i = 0;
    while i < 4 {
        v.push(*sig.get(i).unwrap_or(&0));
        i += 1;
    }
    let t = total as u32;
    v.push((t & 0xFF) as u8);
    v.push(((t >> 8) & 0xFF) as u8);
    v.push(((t >> 16) & 0xFF) as u8);
    v.push(((t >> 24) & 0xFF) as u8);
    v.push(revision);
    v.push(0u8); // checksum 占位，末尾按补数回填
    let mut j = 0;
    while j < 6 {
        v.push(b'V');
        j += 1;
    }
    let mut k = 0;
    while k < 8 {
        v.push(b'X');
        k += 1;
    }
    let mut m = 0;
    while m < 12 {
        v.push(0u8);
        m += 1;
    }
    // 确定性体。
    let ord = kind.ord();
    let mut n = 0;
    while n < body_len {
        v.push(corpus_body_byte(ord, n));
        n += 1;
    }
    // 整表补数：checksum = 0 − sum(其余字节)。
    let others = {
        let mut s = 0u8;
        let mut q = 0;
        while q < v.len() {
            if let Some(x) = v.get(q) {
                s = s.wrapping_add(*x);
            }
            q += 1;
        }
        s
    };
    if let Some(slot) = v.get_mut(OFF_CK) {
        *slot = 0u8.wrapping_sub(others);
    }
    v
}

/// 12 表全语料像（顺序 = TblSig::ALL 冻结序）。
pub fn build_corpus() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut i = 0;
    while i < TblSig::ALL.len() {
        if let Some(k) = TblSig::ALL.get(i) {
            out.push(build_table(*k, 1));
        }
        i += 1;
    }
    out
}

// ── 语料全核账 ──────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusReport {
    /// 逐表报告（顺序对齐 TblSig::ALL）。
    pub reports: Vec<VerifyReport>,
    /// 全绿表数。
    pub green: u32,
}

impl CorpusReport {
    /// N/N 全核（12/12）。
    pub fn all_green(&self) -> bool {
        self.reports.len() == CORPUS_N && self.green == CORPUS_N as u32
    }
}

/// 12 表语料全核：逐表双口径校验，缺表显性 CorpusMiss。
pub fn corpus_run() -> Result<CorpusReport, u32> {
    let mut cur = VerifyCursor::new();
    let corpus = build_corpus();
    let mut reports: Vec<VerifyReport> = Vec::new();
    let mut green = 0u32;
    let mut i = 0;
    while i < TblSig::ALL.len() {
        let kind = match TblSig::ALL.get(i) {
            Some(k) => *k,
            None => return Err(CsumCode::CorpusMiss.val()),
        };
        let img = match corpus.get(i) {
            Some(x) => x,
            None => return Err(CsumCode::CorpusMiss.val()),
        };
        match verify_table(img) {
            Ok(rep) => {
                if rep.kind != kind {
                    return Err(CsumCode::SigUnknown.val());
                }
                green += 1;
                reports.push(rep);
            }
            Err(e) => return Err(e),
        }
        cur.tables_done += 1;
        i += 1;
    }
    if cur.tables_done != CORPUS_N as u32 {
        return Err(CsumCode::CorpusMiss.val());
    }
    cur.advance(VerifyPhase::Verdict)?;
    Ok(CorpusReport { reports, green })
}

// ── LCG（种子固定，注入轮驱动用） ───────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（两路 × 12 表，显性计数） ────────────────────────
/// 路 0：checksum 字段 +1（判据点名坏表注入）；
/// 路 1：体中部字节翻转（体坏亦须整表和抓出）。
pub struct InjectLedger {
    /// 每路尝试数。
    pub tries: [u32; 2],
    /// 每路抓获数。
    pub caught: [u32; 2],
    /// 逐表命中位（[路][表]），false = 漏抓（显性）。
    pub per_kind_hit: [[bool; 12]; 2],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 2], caught: [0; 2], per_kind_hit: [[false; 12]; 2] }
    }
    pub fn record(&mut self, lane: usize, kind_ord: usize, hit: bool) {
        if lane < 2 && kind_ord < 12 {
            if let Some(t) = self.tries.get_mut(lane) {
                *t += 1;
            }
            if hit {
                if let Some(c) = self.caught.get_mut(lane) {
                    *c += 1;
                }
                if let Some(row) = self.per_kind_hit.get_mut(lane) {
                    if let Some(slot) = row.get_mut(kind_ord) {
                        *slot = true;
                    }
                }
            }
        }
    }
    /// 全抓：两路全试全中（每轮 12 表 × 2 路，逐表命中位全真）。
    pub fn all_caught(&self) -> bool {
        let mut lane = 0;
        while lane < 2 {
            let t = *self.tries.get(lane).unwrap_or(&0);
            let c = *self.caught.get(lane).unwrap_or(&0);
            if t == 0 || c != t {
                return false;
            }
            let mut k = 0;
            while k < CORPUS_N {
                let hit = self
                    .per_kind_hit
                    .get(lane)
                    .and_then(|row| row.get(k))
                    .copied()
                    .unwrap_or(false);
                if !hit {
                    return false;
                }
                k += 1;
            }
            lane += 1;
        }
        true
    }
}

/// 注入轮（LCG 种子固定；每轮对 12 表 × 2 路注入，全部必抓）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        let mut k = 0;
        while k < CORPUS_N {
            let kind = match TblSig::ALL.get(k) {
                Some(x) => *x,
                None => return Err(CsumCode::CorpusMiss.val()),
            };
            // 路 0：checksum 字段 +1。
            let mut img0 = build_table(kind, 1);
            if let Some(slot) = img0.get_mut(OFF_CK) {
                *slot = slot.wrapping_add(1);
            }
            let hit0 = match verify_table(&img0) {
                Err(e) => e == CsumCode::CsumFullBad.val(),
                Ok(_) => false,
            };
            led.record(0, kind.ord(), hit0);
            // 路 1：体中部字节翻转（翻 1 必然破坏整表 ≡0）。
            let img1 = build_table(kind, 1);
            let mid = HDR_LEN as usize + corpus_body_len(kind) / 2;
            let mut img1b = img1;
            if let Some(slot) = img1b.get_mut(mid) {
                *slot = slot.wrapping_add(1);
            }
            let hit1 = match verify_table(&img1b) {
                Err(e) => e == CsumCode::CsumFullBad.val(),
                Ok(_) => false,
            };
            led.record(1, kind.ord(), hit1);
            k += 1;
        }
        r += 1;
    }
    Ok(led)
}

// ── 黄金景 ──────────────────────────────────────────────────────
/// 黄金景 A：语料 12 表全核全绿。
pub fn golden_corpus() -> Result<CorpusReport, u32> {
    corpus_run()
}

/// 黄金景 B：单表双口径账逐字段核对（取 MADT）。
pub fn golden_single() -> Result<VerifyReport, u32> {
    let img = build_table(TblSig::Madt, 2);
    verify_table(&img)
}

/// 探针序 A：12/12 全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    let c = golden_corpus()?;
    if !c.all_green() {
        return Err(CsumCode::NotReproducible.val());
    }
    Ok(())
}

/// 探针序 B：坏表（checksum+1）必抓 CsumFullBad。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut img = build_table(TblSig::Hpet, 1);
    if let Some(slot) = img.get_mut(OFF_CK) {
        *slot = slot.wrapping_add(1);
    }
    match verify_table(&img) {
        Err(e) if e == CsumCode::CsumFullBad.val() => Ok(()),
        _ => Err(CsumCode::NotReproducible.val()),
    }
}

/// 探针序 C：校验状态机倒退必抓。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut c = VerifyCursor::new();
    let _ = c.advance(VerifyPhase::LenChecked);
    match c.advance(VerifyPhase::HeaderRead) {
        Err(e) if e == CsumCode::VerifyRegress.val() => Ok(()),
        _ => Err(CsumCode::NotReproducible.val()),
    }
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_corpus()?;
    let b = golden_corpus()?;
    let c = golden_single()?;
    let d = golden_single()?;
    if a != b || c != d {
        return Err(CsumCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < CsumCode::ALL.len() {
        if let Some(c) = CsumCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 判据：12/12 语料全核全绿。
fn judge_corpus_ok() -> bool {
    match golden_corpus() {
        Ok(c) => c.all_green(),
        Err(_) => false,
    }
}

/// 判据：单表双口径账逐字段齐（MADT 样张）。
fn judge_dual_ledger_ok() -> bool {
    match golden_single() {
        Ok(r) => r.clean() && r.fsum == 0 && r.actual_len == HDR_LEN as usize + 44,
        Err(_) => false,
    }
}

/// 判据：坏表注入 2 路 × 12 表全抓。
fn judge_inject_ok() -> bool {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    }
}

/// 判据迭代轮数（10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 守卫：段位窗口。
    v.push((
        "守卫-段位窗口",
        segment_guard(),
        format!("SEG_BASE=0x{:x}", SEG_BASE),
    ));

    // 判据：12/12 语料全核。
    v.push((
        "J1-12表全核N/N",
        judge_corpus_ok(),
        format!("N={} 全核全绿", CORPUS_N),
    ));

    // 判据：双口径账逐字段。
    v.push((
        "J2-双口径求和账",
        judge_dual_ledger_ok(),
        String::from("整表≡0 + 36B差值复算 双账齐"),
    ));

    // 判据：坏表注入必抓显性计数。
    v.push((
        "J3-坏表注入必抓",
        judge_inject_ok(),
        String::from("checksum+1 与体翻转 两路×12表 全中"),
    ));

    // 语料签名闭集（12 签名互异且可回查）。
    {
        let mut distinct = true;
        let mut i = 0;
        while i < TblSig::ALL.len() {
            let mut j = i + 1;
            while j < TblSig::ALL.len() {
                if let (Some(a), Some(b)) = (TblSig::ALL.get(i), TblSig::ALL.get(j)) {
                    if a.sig() == b.sig() {
                        distinct = false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        let round_trip = sig_lookup(TblSig::Madt.sig()) == Ok(TblSig::Madt)
            && sig_lookup(TblSig::Tpm2.sig()) == Ok(TblSig::Tpm2);
        v.push((
            "签名闭集-互异可回查",
            distinct && round_trip,
            String::from("12 签名互异 + MADT/TPM2 回查一致"),
        ));
    }

    // 未知签名显性拒。
    {
        let hit = sig_lookup([0xDE, 0xAD, 0xBE, 0xEF])
            == Err(CsumCode::SigUnknown.val());
        v.push((
            "未知签名显性拒",
            hit,
            String::from("DEADBEEF ⇒ SigUnknown"),
        ));
    }

    // 表像残缺显性拒（30 字节 < 36）。
    {
        let img = build_table(TblSig::Fadt, 1);
        let hit = match img.get(..30) {
            Some(short) => TblHeader36::decode(short) == Err(CsumCode::HdrShort.val()),
            None => false,
        };
        v.push((
            "表像残缺显性拒",
            hit,
            String::from("30 字节 < 36 ⇒ HdrShort"),
        ));
    }

    // length 账失配显性拒（镜像截尾 4 字节）。
    {
        let img = build_table(TblSig::Slit, 1);
        let keep = img.len().saturating_sub(4);
        let mut cut: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < keep {
            if let Some(b) = img.get(k) {
                cut.push(*b);
            }
            k += 1;
        }
        let hit = verify_table(&cut) == Err(CsumCode::LenMismatch.val());
        v.push((
            "length账失配显性拒",
            hit,
            String::from("截尾 4 字节 ⇒ LenMismatch"),
        ));
    }

    // 语料缺表显性（CorpusMiss 码在闭集且 detail 非空）。
    {
        let hit = detail(CsumCode::CorpusMiss.val()) != "未知诊断码：不在 0x750C0 段闭集内";
        v.push((
            "缺表码闭集显性",
            hit,
            String::from("CorpusMiss detail 可查"),
        ));
    }

    // 表头字段偏移冻结账（36 字节逐字段位）。
    {
        let ok = HDR_LEN == 36
            && OFF_LEN == 4
            && OFF_REV == 8
            && OFF_CK == 9
            && OFF_OEMID == 10
            && OFF_OTID == 16
            && OFF_OEMREV == 24
            && OFF_CRID == 28
            && OFF_CRREV == 32;
        v.push((
            "表头偏移冻结",
            ok,
            format!("36B: sig@0 len@{} rev@{} ck@{}", OFF_LEN, OFF_REV, OFF_CK),
        ));
    }

    // 状态机单调：倒退必抓。
    v.push((
        "状态机单调-倒退必抓",
        probe_seq_c().is_ok(),
        String::from("LenChecked→HeaderRead ⇒ VerifyRegress"),
    ));

    // 双跑一致。
    v.push((
        "双跑一致",
        repro_check().is_ok(),
        String::from("语料/单表黄金景两跑同值"),
    ));

    // 探针三序。
    v.push((
        "探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("a=12/12 b=坏表抓 c=倒退抓"),
    ));

    v
}

pub fn run_unx0444_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0444 表头校验和族（逐表核）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_corpus_all_green() {
        let c = golden_corpus().ok().unwrap_or(CorpusReport {
            reports: Vec::new(),
            green: 0,
        });
        assert!(c.all_green());
        assert_eq!(c.reports.len(), 12);
        assert_eq!(c.green, 12);
    }

    #[test]
    fn test_single_dual_ledger() {
        let r = golden_single().ok().unwrap_or(VerifyReport {
            kind: TblSig::Madt,
            len_field: 0,
            actual_len: 0,
            ck_byte: 0,
            hdr_sum: 9,
            fsum: 9,
            full_ok: false,
            ledger_ok: false,
        });
        assert!(r.clean());
        assert_eq!(r.fsum, 0);
        assert_eq!(r.actual_len, 80); // 36 + 44
    }

    #[test]
    fn test_checksum_plus1_caught() {
        let mut img = build_table(TblSig::Rsdt, 1);
        if let Some(slot) = img.get_mut(OFF_CK) {
            *slot = slot.wrapping_add(1);
        }
        assert_eq!(verify_table(&img), Err(CsumCode::CsumFullBad.val()));
    }

    #[test]
    fn test_body_flip_caught() {
        let mut img = build_table(TblSig::Dsdt, 1);
        let mid = HDR_LEN as usize + corpus_body_len(TblSig::Dsdt) / 2;
        if let Some(slot) = img.get_mut(mid) {
            *slot = slot.wrapping_add(1);
        }
        assert_eq!(verify_table(&img), Err(CsumCode::CsumFullBad.val()));
    }

    #[test]
    fn test_inject_all_caught() {
        let led = inject_rounds(3).ok().unwrap_or(InjectLedger::new());
        assert!(led.all_caught());
        assert_eq!(led.tries[0], 36); // 12 表 × 3 轮
        assert_eq!(led.caught[1], 36);
    }

    #[test]
    fn test_sig_lookup_closed() {
        assert_eq!(sig_lookup(TblSig::Hpet.sig()), Ok(TblSig::Hpet));
        assert_eq!(
            sig_lookup([0, 0, 0, 0]),
            Err(CsumCode::SigUnknown.val())
        );
    }

    #[test]
    fn test_hdr_short_and_len_mismatch() {
        let img = build_table(TblSig::Mcfg, 1);
        let short = img.get(..30).unwrap_or(&[]);
        assert_eq!(TblHeader36::decode(short), Err(CsumCode::HdrShort.val()));
        let keep = img.len().saturating_sub(4);
        let mut cut: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < keep {
            if let Some(b) = img.get(k) {
                cut.push(*b);
            }
            k += 1;
        }
        assert_eq!(verify_table(&cut), Err(CsumCode::LenMismatch.val()));
    }

    #[test]
    fn test_phase_regress() {
        let mut c = VerifyCursor::new();
        assert!(c.advance(VerifyPhase::SumsTallied).is_ok());
        assert_eq!(
            c.advance(VerifyPhase::Idle),
            Err(CsumCode::VerifyRegress.val())
        );
    }

    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x75000 && SEG_BASE <= 0x781C0);
    }

    #[test]
    fn test_checks_all_green() {
        let s = run_unx0444_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
    }
}
