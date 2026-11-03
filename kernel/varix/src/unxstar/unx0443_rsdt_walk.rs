//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0443 RSDT/XSDT 表指针遍历 · 0x75080段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   指针数 = 长度推算数 100% 一致，每指针 64 位范围账齐，坏指针
//!   显性计数，XSDT 优先裁定。
//! 设计要点：
//!   1. SDT 表头真实建模（36 字节：签名 4 + 长度 4 + revision 1 +
//!      checksum 1 + oemid 6 + oem_table_id 8 + oem_revision 4 +
//!      creator_id 4 + creator_revision 4），decode 双向。
//!   2. 双口径遍历器：RSDT 32 位指针（口径 (长度−36)/4）、XSDT
//!      64 位指针（口径 (长度−36)/8），镜像字节实数与长度推算数
//!      对拍，不一致即 CountMismatch（100% 一致才放行）。
//!   3. 每指针 64 位范围账：零扩展成 u64 后落在冻结窗
//!      [0x1000, 0x7FFF_FFFF] 且非零；对齐账 RSDT 4 / XSDT 8。
//!   4. 坏指针（越界/不对齐）显性计数入账（bad_range/bad_align），
//!      不静默丢弃；账面随镜像逐字节可复算。
//!   5. 裁定状态机：XSDT 存在即优先（Xsdt 裁定），两表皆缺显性
//!      ArbiAbsent；裁定相位单调，倒退显性拒。
//!   6. 注入四路（越界/不对齐/数账失配/双表缺）逐路必抓，LCG 种子固定。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75080，WBV1104 批 ACPI 栈第三卡区）。
pub const SEG_BASE: u32 = 0x75080;

// ── 冻结常量表（来源：ACPI 规范 §5.2.6/§5.2.7 SDT 通用表头） ─────
/// SDT 通用表头长度。
pub const HDR_LEN: u32 = 36;
/// RSDT 签名。
pub const RSDT_SIG: [u8; 4] = [0x52, 0x53, 0x44, 0x54];
/// XSDT 签名。
pub const XSDT_SIG: [u8; 4] = [0x58, 0x53, 0x44, 0x54];
/// RSDT 指针字节宽。
pub const RSDT_PTR_W: u32 = 4;
/// XSDT 指针字节宽。
pub const XSDT_PTR_W: u32 = 8;
/// 指针合法窗下沿（冻结：内核表区约定，0 页与实模式 IVT 除外）。
pub const PTR_MIN: u64 = 0x1000;
/// 指针合法窗上沿（冻结：32 位可用内存上限 2GB 内）。
pub const PTR_MAX: u64 = 0x7FFF_FFFF;
/// RSDT 指针对齐。
pub const RSDT_ALIGN: u64 = 4;
/// XSDT 指针对齐。
pub const XSDT_ALIGN: u64 = 8;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsdtWalkCode {
    /// 表头残缺（不足 36 字节或 length 字段 < 36）
    HdrShort,
    /// 表签名失配（喂错口径：RSDT 遍历器见 XSDT 像等）
    SigMismatch,
    /// 指针数与长度推算数失配
    CountMismatch,
    /// 指针越界（0 或落在冻结窗外）
    PtrRange,
    /// 指针不对齐（RSDT 4 / XSDT 8）
    PtrAlign,
    /// 双表皆缺，无法裁定
    ArbiAbsent,
    /// 遍历/裁定状态机倒退
    WalkRegress,
    /// 双跑不一致
    NotReproducible,
}

impl RsdtWalkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RsdtWalkCode::HdrShort => "表头残缺：不足 36 字节或 length 字段 < 36，建议核对表镜像完整性",
            RsdtWalkCode::SigMismatch => "表签名失配：口径与镜像不配，建议按签名选对遍历器",
            RsdtWalkCode::CountMismatch => "指针数与长度推算数失配：镜像字节实数 ≠ (length−36)/口径，建议重取表镜像",
            RsdtWalkCode::PtrRange => "指针越界：0 或不在 [0x1000, 0x7FFF_FFFF] 冻结窗，建议核对固件表指针",
            RsdtWalkCode::PtrAlign => "指针不对齐：RSDT 须 4 对齐 / XSDT 须 8 对齐，建议核对指针值",
            RsdtWalkCode::ArbiAbsent => "双表皆缺：RSDP 未给出任何根表，建议回查 RSDP 校验链",
            RsdtWalkCode::WalkRegress => "遍历状态机倒退：单调推进被破坏，建议检查遍历驱动序",
            RsdtWalkCode::NotReproducible => "双跑不一致：复现红线，建议排查非确定源",
        }
    }
    pub const ALL: [RsdtWalkCode; 8] = [
        RsdtWalkCode::HdrShort,
        RsdtWalkCode::SigMismatch,
        RsdtWalkCode::CountMismatch,
        RsdtWalkCode::PtrRange,
        RsdtWalkCode::PtrAlign,
        RsdtWalkCode::ArbiAbsent,
        RsdtWalkCode::WalkRegress,
        RsdtWalkCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RsdtWalkCode::ALL.len() {
        if let Some(c) = RsdtWalkCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75080 段闭集内"
}

// ── 表类 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableKind {
    /// 32 位指针根表
    Rsdt,
    /// 64 位指针根表
    Xsdt,
}

impl TableKind {
    pub fn sig(self) -> [u8; 4] {
        match self {
            TableKind::Rsdt => RSDT_SIG,
            TableKind::Xsdt => XSDT_SIG,
        }
    }
    pub fn ptr_w(self) -> u32 {
        match self {
            TableKind::Rsdt => RSDT_PTR_W,
            TableKind::Xsdt => XSDT_PTR_W,
        }
    }
    pub fn align(self) -> u64 {
        match self {
            TableKind::Rsdt => RSDT_ALIGN,
            TableKind::Xsdt => XSDT_ALIGN,
        }
    }
}

// ── SDT 通用表头（36 字节） ─────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdtHeader {
    pub sig: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_revision: u32,
}

impl SdtHeader {
    /// 表头解码（不足 36 字节显性拒）。
    pub fn decode(img: &[u8]) -> Result<SdtHeader, u32> {
        if img.len() < HDR_LEN as usize {
            return Err(RsdtWalkCode::HdrShort.val());
        }
        let mut sig = [0u8; 4];
        let mut i = 0;
        while i < 4 {
            sig[i] = *img.get(i).unwrap_or(&0);
            i += 1;
        }
        let g = |off: usize| -> u32 {
            let b0 = *img.get(off).unwrap_or(&0) as u32;
            let b1 = *img.get(off + 1).unwrap_or(&0) as u32;
            let b2 = *img.get(off + 2).unwrap_or(&0) as u32;
            let b3 = *img.get(off + 3).unwrap_or(&0) as u32;
            b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
        };
        let mut oem = [0u8; 6];
        let mut j = 0;
        while j < 6 {
            oem[j] = *img.get(10 + j).unwrap_or(&0);
            j += 1;
        }
        let mut otid = [0u8; 8];
        let mut k = 0;
        while k < 8 {
            otid[k] = *img.get(16 + k).unwrap_or(&0);
            k += 1;
        }
        Ok(SdtHeader {
            sig,
            length: g(4),
            revision: *img.get(8).unwrap_or(&0),
            checksum: *img.get(9).unwrap_or(&0),
            oem_id: oem,
            oem_table_id: otid,
            oem_revision: g(24),
            creator_id: g(28),
            creator_revision: g(32),
        })
    }
}

// ── 双口径对拍 ──────────────────────────────────────────────────
/// 长度推算指针数（RSDT 口径 (length−36)/4）。
pub fn rsdt_count_from_len(length: u32) -> Result<usize, u32> {
    if length < HDR_LEN {
        return Err(RsdtWalkCode::HdrShort.val());
    }
    match (length - HDR_LEN).checked_div(RSDT_PTR_W) {
        Some(n) => Ok(n as usize),
        None => Err(RsdtWalkCode::CountMismatch.val()),
    }
}

/// 长度推算指针数（XSDT 口径 (length−36)/8）。
pub fn xsdt_count_from_len(length: u32) -> Result<usize, u32> {
    if length < HDR_LEN {
        return Err(RsdtWalkCode::HdrShort.val());
    }
    match (length - HDR_LEN).checked_div(XSDT_PTR_W) {
        Some(n) => Ok(n as usize),
        None => Err(RsdtWalkCode::CountMismatch.val()),
    }
}

// ── 遍历报告 ────────────────────────────────────────────────────
/// 遍历账面：期望数/实数/合法指针/坏指针显性计数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkReport {
    pub kind: TableKind,
    /// 长度推算数。
    pub expected: usize,
    /// 镜像字节实数。
    pub actual: usize,
    /// 范围+对齐双绿的指针（64 位账面）。
    pub ok_ptrs: Vec<u64>,
    /// 越界指针显性计数。
    pub bad_range: u32,
    /// 不对齐指针显性计数。
    pub bad_align: u32,
}

impl WalkReport {
    /// 账面全绿：数账 100% 一致且零坏指针。
    pub fn clean(&self) -> bool {
        self.expected == self.actual && self.bad_range == 0 && self.bad_align == 0
    }
}

// ── 单调遍历状态机 ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkPhase {
    /// 未开始
    Idle,
    /// 表头已读
    HeaderRead,
    /// 数账已对拍
    CountVerified,
    /// 指针扫描中/已扫
    PointerScan,
    /// 遍历完结
    Walked,
}

impl WalkPhase {
    fn rank(self) -> u32 {
        match self {
            WalkPhase::Idle => 0,
            WalkPhase::HeaderRead => 1,
            WalkPhase::CountVerified => 2,
            WalkPhase::PointerScan => 3,
            WalkPhase::Walked => 4,
        }
    }
}

/// 遍历游标：单调推进，倒退显性拒。
#[derive(Clone, Debug)]
pub struct WalkCursor {
    pub phase: WalkPhase,
    pub scanned: u32,
}

impl WalkCursor {
    pub fn new() -> WalkCursor {
        WalkCursor { phase: WalkPhase::Idle, scanned: 0 }
    }
    pub fn advance(&mut self, to: WalkPhase) -> Result<(), u32> {
        if to.rank() < self.phase.rank() {
            return Err(RsdtWalkCode::WalkRegress.val());
        }
        self.phase = to;
        Ok(())
    }
}

// ── 指针 64 位范围账 ────────────────────────────────────────────
/// 单指针 64 位范围+对齐双账（合法 Some(p)，越界/不对齐归账）。
fn ptr_ledger(kind: TableKind, p: u64, rep: &mut WalkReport) -> Option<u64> {
    if p < PTR_MIN || p > PTR_MAX {
        rep.bad_range += 1;
        return None;
    }
    if p % kind.align() != 0 {
        rep.bad_align += 1;
        return None;
    }
    Some(p)
}

// ── 双口径遍历器 ────────────────────────────────────────────────
/// RSDT（32 位指针）遍历。
pub fn walk_rsdt(img: &[u8]) -> Result<WalkReport, u32> {
    walk_generic(TableKind::Rsdt, img)
}

/// XSDT（64 位指针）遍历。
pub fn walk_xsdt(img: &[u8]) -> Result<WalkReport, u32> {
    walk_generic(TableKind::Xsdt, img)
}

fn walk_generic(kind: TableKind, img: &[u8]) -> Result<WalkReport, u32> {
    let mut cur = WalkCursor::new();
    // 表头。
    let hdr = SdtHeader::decode(img)?;
    cur.advance(WalkPhase::HeaderRead)?;
    if hdr.sig != kind.sig() {
        return Err(RsdtWalkCode::SigMismatch.val());
    }
    if hdr.length < HDR_LEN {
        return Err(RsdtWalkCode::HdrShort.val());
    }
    // 数账对拍：长度推算数 vs 镜像字节实数。
    let expected = match kind {
        TableKind::Rsdt => rsdt_count_from_len(hdr.length)?,
        TableKind::Xsdt => xsdt_count_from_len(hdr.length)?,
    };
    let body = match img.len().checked_sub(HDR_LEN as usize) {
        Some(b) => b,
        None => return Err(RsdtWalkCode::HdrShort.val()),
    };
    let actual = body / kind.ptr_w() as usize;
    if expected != actual {
        return Err(RsdtWalkCode::CountMismatch.val());
    }
    cur.advance(WalkPhase::CountVerified)?;
    // 指针扫描。
    cur.advance(WalkPhase::PointerScan)?;
    let mut rep = WalkReport {
        kind,
        expected,
        actual,
        ok_ptrs: Vec::new(),
        bad_range: 0,
        bad_align: 0,
    };
    let mut i = 0;
    while i < expected {
        let off = HDR_LEN as usize + i * kind.ptr_w() as usize;
        let p = read_le(img, off, kind.ptr_w() as usize);
        let p64 = match kind {
            TableKind::Rsdt => p as u64, // 32 位零扩展
            TableKind::Xsdt => p,
        };
        if let Some(good) = ptr_ledger(kind, p64, &mut rep) {
            rep.ok_ptrs.push(good);
        }
        cur.scanned += 1;
        i += 1;
    }
    cur.advance(WalkPhase::Walked)?;
    Ok(rep)
}

/// 从镜像 off 起读 w 字节小端（越尾显性拒以 0 记账）。
fn read_le(img: &[u8], off: usize, w: usize) -> u64 {
    let mut v = 0u64;
    let mut k = 0;
    while k < w {
        let b = match img.get(off + k) {
            Some(x) => *x as u64,
            None => return 0,
        };
        v |= b << (8 * k);
        k += 1;
    }
    v
}

// ── 裁定状态机（XSDT 优先） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArbPhase {
    /// 未开始
    ArbIdle,
    /// RSDT 已过账
    RsdtDone,
    /// XSDT 已过账
    XsdtDone,
    /// 已裁定
    Arbitrated,
}

impl ArbPhase {
    fn rank(self) -> u32 {
        match self {
            ArbPhase::ArbIdle => 0,
            ArbPhase::RsdtDone => 1,
            ArbPhase::XsdtDone => 2,
            ArbPhase::Arbitrated => 3,
        }
    }
}

/// 裁定游标：单调推进，倒退显性拒。
#[derive(Clone, Debug)]
pub struct Arbitrator {
    pub phase: ArbPhase,
}

impl Arbitrator {
    pub fn new() -> Arbitrator {
        Arbitrator { phase: ArbPhase::ArbIdle }
    }
    pub fn advance(&mut self, to: ArbPhase) -> Result<(), u32> {
        if to.rank() < self.phase.rank() {
            return Err(RsdtWalkCode::WalkRegress.val());
        }
        self.phase = to;
        Ok(())
    }
}

/// 裁定结果：XSDT 优先，选中表 + 其遍历账面。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArbReport {
    pub chosen: TableKind,
    pub report: WalkReport,
    /// 落选表存在与否（显性记账）。
    pub loser_present: bool,
}

/// 双表裁定：XSDT 存在即优先；皆缺 ArbiAbsent。
pub fn arbitrate(rsdt: Option<&[u8]>, xsdt: Option<&[u8]>) -> Result<ArbReport, u32> {
    let mut ar = Arbitrator::new();
    // XSDT 优先分支。
    if let Some(x) = xsdt {
        let rep = walk_xsdt(x)?;
        ar.advance(ArbPhase::XsdtDone)?;
        let loser = rsdt.is_some();
        ar.advance(ArbPhase::Arbitrated)?;
        return Ok(ArbReport { chosen: TableKind::Xsdt, report: rep, loser_present: loser });
    }
    // RSDT 兜底分支。
    if let Some(r) = rsdt {
        let rep = walk_rsdt(r)?;
        ar.advance(ArbPhase::RsdtDone)?;
        ar.advance(ArbPhase::Arbitrated)?;
        return Ok(ArbReport { chosen: TableKind::Rsdt, report: rep, loser_present: false });
    }
    Err(RsdtWalkCode::ArbiAbsent.val())
}

// ── 表像构造（供黄金流/注入用） ─────────────────────────────────
fn build_header(kind: TableKind, total_len: u32) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let sig = kind.sig();
    let mut i = 0;
    while i < 4 {
        v.push(sig[i]);
        i += 1;
    }
    v.push((total_len & 0xFF) as u8);
    v.push(((total_len >> 8) & 0xFF) as u8);
    v.push(((total_len >> 16) & 0xFF) as u8);
    v.push(((total_len >> 24) & 0xFF) as u8);
    v.push(1u8); // revision
    v.push(0u8); // checksum 占位
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
    // oem_revision / creator_id / creator_revision 共 12 字节。
    let mut m = 0;
    while m < 12 {
        v.push(0u8);
        m += 1;
    }
    v
}

/// 构造 RSDT 像（指针数组 → 36 + n*4 字节）。
pub fn build_rsdt(ptrs: &[u32]) -> Vec<u8> {
    let total = HDR_LEN + ptrs.len() as u32 * RSDT_PTR_W;
    let mut v = build_header(TableKind::Rsdt, total);
    let mut i = 0;
    while i < ptrs.len() {
        let p = match ptrs.get(i) {
            Some(x) => *x,
            None => 0,
        };
        v.push((p & 0xFF) as u8);
        v.push(((p >> 8) & 0xFF) as u8);
        v.push(((p >> 16) & 0xFF) as u8);
        v.push(((p >> 24) & 0xFF) as u8);
        i += 1;
    }
    v
}

/// 构造 XSDT 像（指针数组 → 36 + n*8 字节）。
pub fn build_xsdt(ptrs: &[u64]) -> Vec<u8> {
    let total = HDR_LEN + ptrs.len() as u32 * XSDT_PTR_W;
    let mut v = build_header(TableKind::Xsdt, total);
    let mut i = 0;
    while i < ptrs.len() {
        let p = match ptrs.get(i) {
            Some(x) => *x,
            None => 0,
        };
        let mut k = 0;
        while k < 8 {
            v.push(((p >> (8 * k)) & 0xFF) as u8);
            k += 1;
        }
        i += 1;
    }
    v
}

// ── LCG（种子固定） ─────────────────────────────────────────────
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

// ── 黄金流 ──────────────────────────────────────────────────────
/// 真机指针集（4 表：MADT/HPET/FADT/MCFG，冻结地址）。
pub fn machine_ptrs() -> [u64; 4] {
    [0x00E1000, 0x00E2000, 0x00E3000, 0x00E4000]
}

/// 黄金景 A：RSDT 双口径对拍全绿。
pub fn golden_rsdt() -> Result<WalkReport, u32> {
    let mut p: Vec<u32> = Vec::new();
    let src = machine_ptrs();
    let mut i = 0;
    while i < src.len() {
        if let Some(x) = src.get(i) {
            p.push(*x as u32);
        }
        i += 1;
    }
    let img = build_rsdt(&p);
    walk_rsdt(&img)
}

/// 黄金景 B：XSDT 双口径对拍全绿。
pub fn golden_xsdt() -> Result<WalkReport, u32> {
    let img = build_xsdt(&machine_ptrs());
    walk_xsdt(&img)
}

/// 黄金景 C：XSDT 优先裁定（双表并存选 Xsdt，落选显性记账）。
pub fn golden_arbitrate() -> Result<ArbReport, u32> {
    let r = build_rsdt(&[0x00E1000u32, 0x00E2000]);
    let x = build_xsdt(&machine_ptrs());
    arbitrate(Some(&r), Some(&x))
}

/// 探针序 A：双窗黄金全绿且账面可复算。
pub fn probe_seq_a() -> Result<(), u32> {
    let a = golden_rsdt()?;
    let b = golden_xsdt()?;
    if !a.clean() || !b.clean() {
        return Err(RsdtWalkCode::NotReproducible.val());
    }
    if a.ok_ptrs.len() != 4 || b.ok_ptrs.len() != 4 {
        return Err(RsdtWalkCode::NotReproducible.val());
    }
    Ok(())
}

/// 探针序 B：双表皆缺 ⇒ ArbiAbsent 显性。
pub fn probe_seq_b() -> Result<(), u32> {
    match arbitrate(None, None) {
        Err(e) if e == RsdtWalkCode::ArbiAbsent.val() => Ok(()),
        _ => Err(RsdtWalkCode::NotReproducible.val()),
    }
}

/// 探针序 C：遍历状态机倒退必抓。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut c = WalkCursor::new();
    let _ = c.advance(WalkPhase::PointerScan);
    match c.advance(WalkPhase::HeaderRead) {
        Err(e) if e == RsdtWalkCode::WalkRegress.val() => Ok(()),
        _ => Err(RsdtWalkCode::NotReproducible.val()),
    }
}

// ── 注入检出账（四路） ──────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 注入四路：路 1 越界指针显性计数、路 2 不对齐显性计数、
/// 路 3 数账失配必拒、路 4 双表缺必报。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越界指针（0 或 PTR_MAX 之外，LCG 二选一）。
        let bad_val = if rng.below(2) == 0 { 0u64 } else { 0x8000_0000u64 };
        let img1 = build_rsdt(&[0x00E1000, 0x00E2000]);
        let img1b = build_xsdt(&[0x00E1000, bad_val]);
        let hit1 = match arbitrate(Some(&img1), Some(&img1b)) {
            Ok(arb) => arb.chosen == TableKind::Xsdt && arb.report.bad_range == 1,
            Err(_) => false,
        };
        led.record(0, hit1);
        // 路 2：不对齐指针（XSDT 8 对齐账下 4 对齐地址必抓）。
        let img2 = build_xsdt(&[0x00E1004]);
        let hit2 = match walk_xsdt(&img2) {
            Ok(rep) => rep.bad_align == 1 && rep.ok_ptrs.is_empty(),
            Err(_) => false,
        };
        led.record(1, hit2);
        // 路 3：数账失配（镜像截尾 4 字节 ⇒ 实数 < 推算数）。
        let mut img3 = build_rsdt(&[0x00E1000, 0x00E2000]);
        let keep = img3.len().saturating_sub(4);
        let mut cut: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < keep {
            if let Some(b) = img3.get(k) {
                cut.push(*b);
            }
            k += 1;
        }
        img3 = cut;
        let hit3 = match walk_rsdt(&img3) {
            Err(e) => e == RsdtWalkCode::CountMismatch.val(),
            Ok(_) => false,
        };
        led.record(2, hit3);
        // 路 4：双表缺。
        led.record(3, probe_seq_b().is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_rsdt()?;
    let b = golden_rsdt()?;
    let c = golden_arbitrate()?;
    let d = golden_arbitrate()?;
    if a != b || c != d {
        return Err(RsdtWalkCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < RsdtWalkCode::ALL.len() {
        if let Some(c) = RsdtWalkCode::ALL.get(i) {
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
/// 判据：指针数 = 长度推算数 100% 一致（双口径各 4 指针）。
fn judge_count_ok() -> bool {
    match (golden_rsdt(), golden_xsdt()) {
        (Ok(a), Ok(b)) => a.expected == 4 && a.actual == 4 && b.expected == 4 && b.actual == 4,
        _ => false,
    }
}

/// 判据：每指针 64 位范围账齐（零扩展 + 冻结窗）。
fn judge_range_ok() -> bool {
    match golden_rsdt() {
        Ok(a) => {
            let mut all = a.ok_ptrs.len() == 4;
            let mut i = 0;
            while i < a.ok_ptrs.len() {
                if let Some(p) = a.ok_ptrs.get(i) {
                    if *p < PTR_MIN || *p > PTR_MAX {
                        all = false;
                    }
                }
                i += 1;
            }
            all
        }
        Err(_) => false,
    }
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 守卫：段位窗口。
    v.push((
        "守卫-段位窗口",
        segment_guard(),
        format!("SEG_BASE=0x{:x}", SEG_BASE),
    ));

    // 判据：双口径数账 100% 一致。
    v.push((
        "J1-双口径数账100%",
        judge_count_ok(),
        String::from("RSDT (len−36)/4 与 XSDT (len−36)/8 各 4/4"),
    ));

    // 判据：64 位范围账齐。
    v.push((
        "J2-64位范围账齐",
        judge_range_ok(),
        format!("窗 [0x{:x}, 0x{:x}] 零扩展逐指针对账", PTR_MIN, PTR_MAX),
    ));

    // XSDT 优先裁定。
    let c = golden_arbitrate();
    v.push((
        "J3-XSDT优先裁定",
        match &c {
            Ok(arb) => arb.chosen == TableKind::Xsdt && arb.loser_present,
            Err(_) => false,
        },
        match &c {
            Ok(arb) => format!("chosen={:?} loser_present={}", arb.chosen, arb.loser_present),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // RSDT 兜底裁定。
    {
        let r = build_rsdt(&machine_ptrs().iter().map(|p| *p as u32).collect::<Vec<u32>>().as_slice());
        let ok = match arbitrate(Some(&r), None) {
            Ok(arb) => arb.chosen == TableKind::Rsdt && arb.report.clean(),
            Err(_) => false,
        };
        v.push((
            "RSDT兜底裁定",
            ok,
            String::from("无 XSDT 时 RSDT 顶上且账面全绿"),
        ));
    }

    // 双表皆缺显性。
    v.push((
        "双表缺-ArbiAbsent显性",
        probe_seq_b().is_ok(),
        String::from("RSDP 未给根表 ⇒ 0x75084 段内 ArbiAbsent"),
    ));

    // 坏指针显性计数（越界 1 + 不对齐 1，不静默）。
    {
        let img = build_xsdt(&[0x00E1000, 0x8000_0000, 0x00E1004, 0x00E2000]);
        let ok = match walk_xsdt(&img) {
            Ok(rep) => {
                rep.expected == 4
                    && rep.actual == 4
                    && rep.bad_range == 1
                    && rep.bad_align == 1
                    && rep.ok_ptrs.len() == 2
            }
            Err(_) => false,
        };
        v.push((
            "坏指针显性计数",
            ok,
            String::from("bad_range=1 bad_align=1 合法 2 不静默丢"),
        ));
    }

    // 数账失配必拒。
    v.push((
        "数账失配必拒",
        inject_rounds(1).map(|l| l.caught[2] >= 1).unwrap_or(false),
        String::from("镜像截尾 4 字节 ⇒ CountMismatch"),
    ));

    // 表头残缺显性拒。
    {
        let img = build_xsdt(&machine_ptrs());
        let hit = match img.get(..30) {
            Some(short) => match SdtHeader::decode(short) {
                Err(e) => e == RsdtWalkCode::HdrShort.val(),
                Ok(_) => false,
            },
            None => false,
        };
        v.push((
            "表头残缺显性拒",
            hit,
            String::from("30 字节 < 36 ⇒ HdrShort"),
        ));
    }

    // 常量冻结账。
    v.push((
        "常量冻结",
        HDR_LEN == 36 && RSDT_PTR_W == 4 && XSDT_PTR_W == 8 && PTR_MIN == 0x1000
            && PTR_MAX == 0x7FFF_FFFF && RSDT_ALIGN == 4 && XSDT_ALIGN == 8,
        format!(
            "hdr={} w={}/{} 窗[0x{:x},0x{:x}] 对齐{}/{}",
            HDR_LEN, RSDT_PTR_W, XSDT_PTR_W, PTR_MIN, PTR_MAX, RSDT_ALIGN, XSDT_ALIGN
        ),
    ));

    // 签名闭集（RSDT/XSDT 各 4 字节）。
    v.push((
        "签名闭集",
        RSDT_SIG == [0x52u8, 0x53, 0x44, 0x54] && XSDT_SIG == [0x58u8, 0x53, 0x44, 0x54],
        String::from("RSDT=52534454 XSDT=58534454"),
    ));

    // 状态机单调：倒退必抓。
    v.push((
        "状态机单调-倒退必抓",
        probe_seq_c().is_ok(),
        String::from("PointerScan→HeaderRead ⇒ WalkRegress"),
    ));

    // 注入 10/10。
    let inj = inject_rounds(10);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));

    // 双跑一致。
    v.push((
        "双跑一致",
        repro_check().is_ok(),
        String::from("黄金景两跑同值"),
    ));

    // 探针三序。
    v.push((
        "探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("a=双口径全绿 b=双缺显性 c=倒退必抓"),
    ));

    v
}

pub fn run_unx0443_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0443 RSDT/XSDT 表指针遍历");
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
    fn test_rsdt_golden_clean() {
        let rep = golden_rsdt().ok().unwrap_or(WalkReport {
            kind: TableKind::Rsdt,
            expected: 0,
            actual: 0,
            ok_ptrs: Vec::new(),
            bad_range: 99,
            bad_align: 99,
        });
        assert!(rep.clean());
        assert_eq!(rep.expected, 4);
        assert_eq!(rep.ok_ptrs.len(), 4);
    }

    #[test]
    fn test_xsdt_golden_clean() {
        let rep = golden_xsdt().ok().unwrap_or(WalkReport {
            kind: TableKind::Xsdt,
            expected: 0,
            actual: 0,
            ok_ptrs: Vec::new(),
            bad_range: 99,
            bad_align: 99,
        });
        assert!(rep.clean());
        assert_eq!(rep.actual, 4);
    }

    #[test]
    fn test_xsdt_preferred() {
        let arb = golden_arbitrate().ok().unwrap_or(ArbReport {
            chosen: TableKind::Rsdt,
            report: WalkReport {
                kind: TableKind::Rsdt,
                expected: 0,
                actual: 0,
                ok_ptrs: Vec::new(),
                bad_range: 0,
                bad_align: 0,
            },
            loser_present: false,
        });
        assert_eq!(arb.chosen, TableKind::Xsdt);
        assert!(arb.loser_present);
    }

    #[test]
    fn test_bad_pointers_counted_not_silent() {
        let img = build_xsdt(&[0x00E1000, 0x8000_0000, 0x00E1004, 0x00E2000]);
        let rep = walk_xsdt(&img).ok().unwrap_or(WalkReport {
            kind: TableKind::Xsdt,
            expected: 0,
            actual: 0,
            ok_ptrs: Vec::new(),
            bad_range: 0,
            bad_align: 0,
        });
        assert_eq!(rep.bad_range, 1);
        assert_eq!(rep.bad_align, 1);
        assert_eq!(rep.ok_ptrs.len(), 2);
        assert_eq!(rep.expected, rep.actual);
    }

    #[test]
    fn test_count_mismatch_rejected() {
        let mut img = build_rsdt(&[0x00E1000, 0x00E2000]);
        let keep = img.len().saturating_sub(4);
        let mut cut: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < keep {
            if let Some(b) = img.get(k) {
                cut.push(*b);
            }
            k += 1;
        }
        img = cut;
        assert_eq!(walk_rsdt(&img), Err(RsdtWalkCode::CountMismatch.val()));
    }

    #[test]
    fn test_header_short_rejected() {
        let img = build_xsdt(&machine_ptrs());
        let short = img.get(..30).unwrap_or(&[]);
        assert_eq!(SdtHeader::decode(short), Err(RsdtWalkCode::HdrShort.val()));
    }

    #[test]
    fn test_sig_mismatch_rejected() {
        // RSDT 遍历器喂 XSDT 像 ⇒ 签名守卫拒。
        let img = build_xsdt(&machine_ptrs());
        assert_eq!(walk_rsdt(&img), Err(RsdtWalkCode::SigMismatch.val()));
    }

    #[test]
    fn test_arbi_absent() {
        assert_eq!(arbitrate(None, None), Err(RsdtWalkCode::ArbiAbsent.val()));
    }

    #[test]
    fn test_walk_phase_regress() {
        let mut c = WalkCursor::new();
        assert!(c.advance(WalkPhase::PointerScan).is_ok());
        assert_eq!(
            c.advance(WalkPhase::Idle),
            Err(RsdtWalkCode::WalkRegress.val())
        );
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let led = inject_rounds(10).ok().unwrap_or(InjectLedger::new());
        assert!(led.all_caught());
        assert_eq!(led.tries[0], 10);
        assert_eq!(led.caught[3], 10);
    }

    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_checks_all_green() {
        let s = run_unx0443_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
    }
}
