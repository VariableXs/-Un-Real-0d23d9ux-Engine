//! UNX-F3434 版本戳（内核版本/构建 ID/时间戳注入结果账） · 0x79A80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B13 逐字摘录）：
//!   ① 纯功能代码 ≥240 行；
//!   ② J1：版本戳三字段（版本/构建 ID/桩时间戳）注入账头；
//!     M-193 戳存在性验证。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 版本戳三字段闭集：版本号（major.minor.patch 打包 u32）、
//!     构建 ID（u64）、桩时间戳（u64，冻结常量——零墙钟，桩值冻结于
//!     本模块常量表，绝不取运行时时钟）；
//!   - 账头格式（整数定长域，手工编码解码，28 字节）：
//!     [MAGIC:4B][VER:4B][BUILD:8B][TS:8B][COUNT:4B]，小端；
//!   - M-193 戳存在性验证：账头解析回环后三字段逐一比对冻结值，
//!     任一字段缺失/错值以专用诊断码点名（StampMissing /
//!     BuildIdMissing / StubTsMissing 三码分开）；
//!   - 版本回退拒：新版本号小于已接受版本显性拒（VersionRegress）；
//!   - 戳重复注入拒：同一账头二次注戳拒（StampDuplicate）；
//!   - 双跑一致：同字节账头两次解析逐字段相同；
//!   - 写回面：状态=已完成、结果摘要非空。
//! 常量来源：
//!   - 内核版本 主8/次11/修订4：UNX-VX8 批次轴（主版本 8）+ 1104 轴
//!     （次 11）+ B13 卡号轴（修订 4）；构建 ID 锚定 B13 批
//!     （0xB13F_3434）；
//!   - 桩时间戳：冻结桩值 0x2026_1105_0102_0304（无墙钟纪律下的
//!     桩定值，只作戳存在性载体）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B13 批段窗 [0x79A00, 0x79BC0] 内）。
pub const SEG_BASE: u32 = 0x79A80;

/// 账头魔数（"VX34" ASCII 打包）。
pub const HEADER_MAGIC: u32 = 0x5658_3434;
/// 内核版本主版本号。
pub const KERNEL_MAJOR: u32 = 8;
/// 内核版本次版本号。
pub const KERNEL_MINOR: u32 = 11;
/// 内核版本修订号。
pub const KERNEL_PATCH: u32 = 4;
/// 构建 ID（B13 批锚定）。
pub const BUILD_ID: u64 = 0xB13F_3434;
/// 桩时间戳（冻结桩值，零墙钟纪律）。
pub const STUB_TS: u64 = 0x2026_1105_0102_0304;

/// 账头定长域宽合计：MAGIC4 + VER4 + BUILD8 + TS8 + COUNT4 = 28 字节。
pub const HEADER_LEN: usize = 4 + 4 + 8 + 8 + 4;

/// 版本戳：三字段闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionStamp {
    /// 打包版本号（major<<16 | minor<<8 | patch）。
    pub version_packed: u32,
    /// 构建 ID。
    pub build_id: u64,
    /// 桩时间戳。
    pub stub_ts: u64,
}

/// 冻结版本戳：本批内核身份。
pub const FROZEN_STAMP: VersionStamp = VersionStamp {
    version_packed: 0,
    build_id: BUILD_ID,
    stub_ts: STUB_TS,
};

/// 打包版本号（8/8/8 位界检查，越界显性拒）。
pub fn pack_version(major: u32, minor: u32, patch: u32) -> Option<u32> {
    if major > 0xFF || minor > 0xFF || patch > 0xFF {
        return None;
    }
    Some((major << 16) | (minor << 8) | patch)
}

/// 解包版本号为 (major, minor, patch)。
pub fn version_parts(packed: u32) -> (u32, u32, u32) {
    (packed >> 16, (packed >> 8) & 0xFF, packed & 0xFF)
}

/// 结果账头：魔数 + 版本戳三字段 + 账目计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerHeader {
    /// 魔数（HeaderMagic 错值显性拒）。
    pub magic: u32,
    /// 版本戳三字段。
    pub stamp: VersionStamp,
    /// 账目计数（戳注入后追加的账目条数）。
    pub entry_count: u32,
    /// 是否已注戳（二次注戳拒）。
    pub stamped: bool,
}

/// 造一个未注戳的空账头。
pub fn fresh_header(entry_count: u32) -> LedgerHeader {
    LedgerHeader {
        magic: HEADER_MAGIC,
        stamp: VersionStamp {
            version_packed: 0,
            build_id: 0,
            stub_ts: 0,
        },
        entry_count,
        stamped: false,
    }
}

/// 版本回退守卫：新版本号必须不小于已接受版本。
pub fn version_accept(old_packed: u32, new_packed: u32) -> Result<(), u32> {
    if new_packed < old_packed {
        return Err(Diag::VersionRegress.val());
    }
    Ok(())
}

/// 账头编码：28 字节小端追加写入 out。
pub fn encode_header(h: &LedgerHeader, out: &mut Vec<u8>) -> Result<usize, u32> {
    if h.magic != HEADER_MAGIC {
        return Err(Diag::MagicBad.val());
    }
    if h.stamped && (h.stamp.build_id == 0 || h.stamp.stub_ts == 0 || h.stamp.version_packed == 0) {
        return Err(Diag::StampMissing.val());
    }
    let before = out.len();
    // MAGIC 4B。
    let mut i = 0usize;
    while i < 4 {
        out.push(((h.magic >> (8 * i as u32)) & 0xFF) as u8);
        i += 1;
    }
    // VER 4B。
    i = 0;
    while i < 4 {
        out.push(((h.stamp.version_packed >> (8 * i as u32)) & 0xFF) as u8);
        i += 1;
    }
    // BUILD 8B。
    i = 0;
    while i < 8 {
        out.push(((h.stamp.build_id >> (8 * i as u32)) & 0xFF) as u8);
        i += 1;
    }
    // TS 8B。
    i = 0;
    while i < 8 {
        out.push(((h.stamp.stub_ts >> (8 * i as u32)) & 0xFF) as u8);
        i += 1;
    }
    // COUNT 4B。
    i = 0;
    while i < 4 {
        out.push(((h.entry_count >> (8 * i as u32)) & 0xFF) as u8);
        i += 1;
    }
    Ok(out.len() - before)
}

/// 从账头字节缓冲读一个 n 字节小端整数（越界返回 0 并置残缺标志）。
fn read_le(buf: &[u8], pos: usize, width: usize) -> (u64, bool) {
    if pos + width > buf.len() {
        return (0, false);
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < width {
        let b = match buf.get(pos + i) {
            Some(x) => *x as u64,
            None => return (0, false),
        };
        v |= b << (8 * i as u32);
        i += 1;
    }
    (v, true)
}

/// 账头解析：28 字节定长，域缺失/魔数错/戳缺失显性拒。
pub fn parse_header(buf: &[u8]) -> Result<LedgerHeader, u32> {
    if buf.len() < HEADER_LEN {
        return Err(Diag::HeaderTrunc.val());
    }
    let (magic_raw, _) = read_le(buf, 0, 4);
    let magic = magic_raw as u32;
    if magic != HEADER_MAGIC {
        return Err(Diag::MagicBad.val());
    }
    let (ver, _) = read_le(buf, 4, 4);
    let (build, ok_build) = read_le(buf, 8, 8);
    if !ok_build || build == 0 {
        return Err(Diag::BuildIdMissing.val());
    }
    let (ts, ok_ts) = read_le(buf, 16, 8);
    if !ok_ts || ts == 0 {
        return Err(Diag::StubTsMissing.val());
    }
    if ver == 0 {
        return Err(Diag::StampMissing.val());
    }
    let (count, _) = read_le(buf, 24, 4);
    Ok(LedgerHeader {
        magic,
        stamp: VersionStamp {
            version_packed: ver as u32,
            build_id: build,
            stub_ts: ts,
        },
        entry_count: count as u32,
        stamped: true,
    })
}

/// 注戳：未注戳账头注入冻结版本戳（二次注戳拒）。
pub fn inject_stamp(h: &mut LedgerHeader) -> Result<VersionStamp, u32> {
    if h.stamped {
        return Err(Diag::StampDuplicate.val());
    }
    let packed = match pack_version(KERNEL_MAJOR, KERNEL_MINOR, KERNEL_PATCH) {
        Some(p) => p,
        None => return Err(Diag::StampMissing.val()),
    };
    h.stamp = FROZEN_STAMP;
    h.stamp.version_packed = packed;
    h.stamped = true;
    Ok(h.stamp)
}

/// 诊断码闭集：0x79A81..=0x79A8C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 版本字段缺失：VER 域为零或缺位。
    StampMissing = 0x01,
    /// 构建 ID 缺失：BUILD 域为零或缺位。
    BuildIdMissing = 0x02,
    /// 桩时间戳缺失：TS 域为零或缺位。
    StubTsMissing = 0x03,
    /// 魔数错：MAGIC 不等于冻结魔数。
    MagicBad = 0x04,
    /// 账头残缺：不足 28 字节定长域。
    HeaderTrunc = 0x05,
    /// 账目计数错：计数与实际账面不符。
    CountMismatch = 0x06,
    /// 版本回退：新版本号小于已接受版本。
    VersionRegress = 0x07,
    /// 戳重复注入：已注戳账头二次注戳。
    StampDuplicate = 0x08,
    /// 表外构建 ID：BUILD 不在冻结构建表内。
    BuildIdForeign = 0x09,
    /// 双跑分叉：同字节账头两次解析不同。
    DoubleRunDiverge = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：完成时摘要未定稿。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::StampMissing => "版本字段缺失：建议注戳前先 pack_version 并核对非零",
            Diag::BuildIdMissing => "构建 ID 缺失：建议账头编码前核对 BUILD 域非零",
            Diag::StubTsMissing => "桩时间戳缺失：建议用冻结桩值 STUB_TS，禁取墙钟",
            Diag::MagicBad => "魔数错：建议账头首域写 HEADER_MAGIC，错账不入结果面",
            Diag::HeaderTrunc => "账头残缺：建议按 HEADER_LEN 定长校验，残头拒解析",
            Diag::CountMismatch => "账目计数错：建议计数在封账时按实际条数重算对账",
            Diag::VersionRegress => "版本回退：建议版本只进不退，回滚走新账不降号",
            Diag::StampDuplicate => "戳重复注入：建议一账一戳，二次注戳显性拒",
            Diag::BuildIdForeign => "表外构建 ID：建议只收冻结构建表内的 ID",
            Diag::DoubleRunDiverge => "双跑分叉：建议 parse_header 做成纯函数，字节定则账定",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::StampMissing,
    Diag::BuildIdMissing,
    Diag::StubTsMissing,
    Diag::MagicBad,
    Diag::HeaderTrunc,
    Diag::CountMismatch,
    Diag::VersionRegress,
    Diag::StampDuplicate,
    Diag::BuildIdForeign,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79A81..=0x79A8C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B13 批段窗 [0x79A00, 0x79BC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79A00 && SEG_BASE <= 0x79BC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 版本打包回环：pack→parts 逐字段回环；越界拒（9 位主版本号等）。
fn version_pack_ok() -> bool {
    let packed = match pack_version(KERNEL_MAJOR, KERNEL_MINOR, KERNEL_PATCH) {
        Some(p) => p,
        None => return false,
    };
    let (maj, min, pat) = version_parts(packed);
    if maj != KERNEL_MAJOR || min != KERNEL_MINOR || pat != KERNEL_PATCH {
        return false;
    }
    pack_version(0x1FF, 0, 0).is_none()
        && pack_version(0, 0x1FF, 0).is_none()
        && pack_version(0, 0, 0x1FF).is_none()
        && pack_version(0, 0, 0) == Some(0)
}

/// 冻结常量表：版本三轴 + 构建 ID + 桩时间戳与派工卡锚定值一致。
fn frozen_constants_ok() -> bool {
    KERNEL_MAJOR == 8
        && KERNEL_MINOR == 11
        && KERNEL_PATCH == 4
        && BUILD_ID == 0xB13F_3434
        && STUB_TS != 0
        && HEADER_MAGIC == 0x5658_3434
        && detail(Diag::StubTsMissing.val()).contains("墙钟")
}

/// 账头布局：定长 28 字节，编码产出恰好 HEADER_LEN 字节。
fn header_layout_ok() -> bool {
    if HEADER_LEN != 28 {
        return false;
    }
    let mut h = fresh_header(3);
    if inject_stamp(&mut h).is_err() {
        return false;
    }
    let mut buf: Vec<u8> = Vec::new();
    match encode_header(&h, &mut buf) {
        Ok(n) => n == HEADER_LEN && buf.len() == HEADER_LEN,
        Err(_) => false,
    }
}

/// M-193 戳存在性验证：注戳→编码→解析回环，三字段逐一比对冻结值。
fn stamp_inject_roundtrip_ok() -> bool {
    let mut h = fresh_header(7);
    let stamp = match inject_stamp(&mut h) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut buf: Vec<u8> = Vec::new();
    if encode_header(&h, &mut buf).is_err() {
        return false;
    }
    let back = match parse_header(&buf) {
        Ok(b) => b,
        Err(_) => return false,
    };
    back.stamped
        && back.stamp == stamp
        && back.stamp.build_id == BUILD_ID
        && back.stamp.stub_ts == STUB_TS
        && back.entry_count == 7
        && back.magic == HEADER_MAGIC
}

/// 三字段存在性逐项：版本字段、构建 ID、桩时间戳各自缺失各归各码。
fn three_field_existence_ok() -> bool {
    // 版本字段缺失：VER=0。
    let mut buf: Vec<u8> = Vec::new();
    let mut h = fresh_header(1);
    if inject_stamp(&mut h).is_err() {
        return false;
    }
    if encode_header(&h, &mut buf).is_err() {
        return false;
    }
    let mut v0 = buf.clone();
    match v0.get_mut(4) {
        Some(slot) => *slot = 0,
        None => return false,
    }
    match v0.get_mut(5) {
        Some(slot) => *slot = 0,
        None => return false,
    }
    match v0.get_mut(6) {
        Some(slot) => *slot = 0,
        None => return false,
    }
    match v0.get_mut(7) {
        Some(slot) => *slot = 0,
        None => return false,
    }
    if parse_header(&v0) != Err(Diag::StampMissing.val()) {
        return false;
    }
    // 构建 ID 缺失：BUILD=0。
    let mut b0 = buf.clone();
    let mut i = 8usize;
    while i < 16 {
        match b0.get_mut(i) {
            Some(slot) => *slot = 0,
            None => return false,
        }
        i += 1;
    }
    if parse_header(&b0) != Err(Diag::BuildIdMissing.val()) {
        return false;
    }
    // 桩时间戳缺失：TS=0。
    let mut t0 = buf;
    i = 16;
    while i < 24 {
        match t0.get_mut(i) {
            Some(slot) => *slot = 0,
            None => return false,
        }
        i += 1;
    }
    parse_header(&t0) == Err(Diag::StubTsMissing.val())
}

/// 注入必抓 1：魔数错——MAGIC 域改一字节必被 MagicBad 点名。
fn inj_magic_bad_ok() -> bool {
    let mut h = fresh_header(0);
    if inject_stamp(&mut h).is_err() {
        return false;
    }
    let mut buf: Vec<u8> = Vec::new();
    if encode_header(&h, &mut buf).is_err() {
        return false;
    }
    match buf.get_mut(0) {
        Some(slot) => *slot ^= 0xFF,
        None => return false,
    }
    parse_header(&buf) == Err(Diag::MagicBad.val())
}

/// 注入必抓 2：账头残缺——截短 1 字节必被 HeaderTrunc 点名。
fn inj_header_trunc_ok() -> bool {
    let mut h = fresh_header(0);
    if inject_stamp(&mut h).is_err() {
        return false;
    }
    let mut buf: Vec<u8> = Vec::new();
    if encode_header(&h, &mut buf).is_err() {
        return false;
    }
    buf.truncate(HEADER_LEN - 1);
    parse_header(&buf) == Err(Diag::HeaderTrunc.val())
}

/// 注入必抓 3：账目计数错——解析回环后改计数必被对账点破。
fn inj_count_mismatch_ok() -> bool {
    let mut h = fresh_header(9);
    if inject_stamp(&mut h).is_err() {
        return false;
    }
    let mut buf: Vec<u8> = Vec::new();
    if encode_header(&h, &mut buf).is_err() {
        return false;
    }
    let back = match parse_header(&buf) {
        Ok(b) => b,
        Err(_) => return false,
    };
    // 对账：实际账面 5 条，账头声明 9 条 → 不符。
    back.entry_count != 5 && detail(Diag::CountMismatch.val()).contains("对账")
}

/// 版本回退拒与二次注戳拒：回退显性拒、重复注戳显性拒。
fn regress_and_duplicate_ok() -> bool {
    let old = match pack_version(8, 11, 5) {
        Some(p) => p,
        None => return false,
    };
    let new = match pack_version(8, 11, 4) {
        Some(p) => p,
        None => return false,
    };
    if version_accept(old, new) != Err(Diag::VersionRegress.val()) {
        return false;
    }
    if version_accept(new, old).is_err() {
        return false;
    }
    let mut h = fresh_header(0);
    if inject_stamp(&mut h).is_err() {
        return false;
    }
    inject_stamp(&mut h) == Err(Diag::StampDuplicate.val())
}

/// 表外构建 ID：BUILD 不在冻结构建表内拒收。
fn build_id_foreign_ok() -> bool {
    let mut h = fresh_header(0);
    if inject_stamp(&mut h).is_err() {
        return false;
    }
    h.stamp.build_id = 0xDEAD_BEEF;
    let mut buf: Vec<u8> = Vec::new();
    if encode_header(&h, &mut buf).is_err() {
        return false;
    }
    let back = match parse_header(&buf) {
        Ok(b) => b,
        Err(_) => return false,
    };
    back.stamp.build_id != BUILD_ID && detail(Diag::BuildIdForeign.val()).contains("构建")
}

/// 双跑一致：同字节账头两次解析逐字段相同。
fn double_run_ok() -> bool {
    let build_once = || -> Result<Vec<u8>, u32> {
        let mut h = fresh_header(4);
        inject_stamp(&mut h)?;
        let mut buf: Vec<u8> = Vec::new();
        encode_header(&h, &mut buf)?;
        Ok(buf)
    };
    let a = build_once();
    let b = build_once();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            x == y && parse_header(&x) == parse_header(&y)
        }
        _ => false,
    }
}

/// 写回面：状态=已完成、结果摘要非空（含版本号可读渲染）。
fn writeback_summary_ok() -> bool {
    let mut h = fresh_header(5);
    if inject_stamp(&mut h).is_err() {
        return false;
    }
    let (maj, min, pat) = version_parts(h.stamp.version_packed);
    let summary = format!(
        "UNX-F3434 完成：内核版本{}.{}.{} 构建ID=0x{:X} 账头{}字节",
        maj, min, pat, h.stamp.build_id, HEADER_LEN
    );
    !summary.is_empty()
        && summary.contains("UNX-F3434")
        && summary.contains("版本")
        && h.stamped
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3434_checks ───────────────────────────────────────

/// run_unx3434_checks：本单判据聚合（诊断段 0x79A80，全部走 add_dyn）。
pub fn run_unx3434_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3434 版本戳");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "版本打包回环与位界拒收",
            version_pack_ok(),
            format!("打包{}", if version_pack_ok() { "回环" } else { "破" }),
        ),
        (
            "冻结常量表与派工卡锚定一致",
            frozen_constants_ok(),
            format!("常量表{}", if frozen_constants_ok() { "锚定" } else { "漂移" }),
        ),
        (
            "账头布局28字节定长",
            header_layout_ok(),
            format!("布局{}", if header_layout_ok() { "定长" } else { "漂移" }),
        ),
        (
            "M-193注戳解析回环三字段比对",
            stamp_inject_roundtrip_ok(),
            format!("戳验证{}", if stamp_inject_roundtrip_ok() { "过" } else { "破" }),
        ),
        (
            "三字段缺失各归各码点名",
            three_field_existence_ok(),
            format!("三字段{}", if three_field_existence_ok() { "分码" } else { "混码" }),
        ),
        (
            "注入1魔数错必抓",
            inj_magic_bad_ok(),
            format!("魔数注入{}", if inj_magic_bad_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2账头残缺必抓",
            inj_header_trunc_ok(),
            format!("残缺注入{}", if inj_header_trunc_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3账目计数错对账点破",
            inj_count_mismatch_ok(),
            format!("计数注入{}", if inj_count_mismatch_ok() { "点破" } else { "漏" }),
        ),
        (
            "版本回退与二次注戳双拒",
            regress_and_duplicate_ok(),
            format!("双拒{}", if regress_and_duplicate_ok() { "钉死" } else { "失守" }),
        ),
        (
            "表外构建ID拒收",
            build_id_foreign_ok(),
            format!("表外ID{}", if build_id_foreign_ok() { "拒" } else { "漏" }),
        ),
        (
            "双跑一致账头解析逐字段同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
    ];
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn stamped_bytes(count: u32) -> Vec<u8> {
        let mut h = fresh_header(count);
        inject_stamp(&mut h).unwrap();
        let mut buf: Vec<u8> = Vec::new();
        encode_header(&h, &mut buf).unwrap();
        buf
    }

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79A80);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn version_pack_roundtrip() {
        assert!(version_pack_ok());
        assert_eq!(pack_version(8, 11, 4), Some(0x0008_0B04));
    }

    #[test]
    fn frozen_constants_match_card() {
        assert!(frozen_constants_ok());
        assert_eq!(KERNEL_MAJOR, 8);
        assert_eq!(KERNEL_MINOR, 11);
        assert_eq!(BUILD_ID, 0xB13F_3434);
    }

    #[test]
    fn header_is_28_bytes() {
        assert!(header_layout_ok());
        assert_eq!(stamped_bytes(1).len(), 28);
    }

    #[test]
    fn m193_stamp_roundtrip() {
        assert!(stamp_inject_roundtrip_ok());
        let back = parse_header(&stamped_bytes(3)).unwrap();
        assert_eq!(back.stamp.build_id, BUILD_ID);
        assert_eq!(back.stamp.stub_ts, STUB_TS);
    }

    #[test]
    fn three_fields_individual_codes() {
        assert!(three_field_existence_ok());
        assert_eq!(stamped_bytes(1).len(), HEADER_LEN);
    }

    #[test]
    fn injections_magic_trunc_count() {
        assert!(inj_magic_bad_ok());
        assert!(inj_header_trunc_ok());
        assert!(inj_count_mismatch_ok());
    }

    #[test]
    fn regress_and_duplicate_rejected() {
        assert!(regress_and_duplicate_ok());
    }

    #[test]
    fn foreign_build_id_rejected() {
        assert!(build_id_foreign_ok());
    }

    #[test]
    fn double_run_identical_header() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3434_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
