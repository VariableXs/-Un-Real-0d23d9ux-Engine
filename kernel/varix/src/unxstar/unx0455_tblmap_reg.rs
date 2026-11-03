//! UNX-F0455 表区映射登记（B19 账联动防重） · 0x75380段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，WBV1104·AI-VX7-1104）：
//!   ACPI 表区逐段登记入 B19 内存账（n 段=表数），登记后越界写尝试
//!   拦截 100%。
//! 设计要点：
//!   1. B19 内存账联动建模：段记录 =（签名 4B 窗口 + 物理地址 + 长度 +
//!      页对齐状态）四元组，逐段登记，n 段与表数对拍（12=12）；
//!   2. 登记三闸：页对齐闸（物理基址 % 4096）→ 重叠闸（区间半开
//!      相交即拒）→ 重复闸（同签名同基址显性拒）；
//!   3. 写闸门：写请求（地址+长度）必须整段落在某已登记窗口内才放行，
//!      越出窗口的写尝试 100% 拦截并显性计数；
//!   4. 地址运算全程 checked_add 防回绕（u64 溢出按越界拒）；
//!   5. 边界冻结：段末字节恰含的写放行、末字节 +1 拦截、零长登记拒；
//!   6. 双跑一致红线：同语料两轮登记与写闸结果全等。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75380，AI-VX7-1104 批W7E 独占区）。
pub const SEG_BASE: u32 = 0x75380;

/// 页大小（4096，x86_64 基本页）。
pub const PAGE_SIZE: u64 = 4096;
/// 语料表数（与 B19 对拍基准）。
pub const TABLE_COUNT: usize = 12;
/// 单段长度帽（256 KiB，防登记烂长）。
pub const SEG_LEN_CAP: u32 = 0x40000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapCode {
    /// 越界写（写请求越出全部已登记窗口）
    OobWrite,
    /// 重复登记（同签名同基址已在账）
    DupReg,
    /// 区间重叠（与已登记段相交）
    Overlap,
    /// 页未对齐（物理基址非 4KiB 对齐）
    PageMisalign,
    /// 登记长度非法（0 或越帽）
    LenBad,
    /// 段数对拍失配（n 段 != 表数）
    CountMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl MapCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MapCode::OobWrite => "越界写：请求越出全部登记窗口；建议先查 B19 段窗口再发起写",
            MapCode::DupReg => "重复登记：同签名同基址已在账；建议走查询通道而非重复登记",
            MapCode::Overlap => "区间重叠：与已登记段相交；建议调整基址留出页间隙",
            MapCode::PageMisalign => "页未对齐：物理基址非 4KiB 对齐；建议按页边界重排映射",
            MapCode::LenBad => "登记长度非法：0 或越 256KiB 帽；建议按表头 Length 复核",
            MapCode::CountMismatch => "段数对拍失配：B19 段数应等于表数；建议逐表补登或清重登",
            MapCode::NotReproducible => "双跑不一致：登记账两轮不等；建议冻结语料与登记序",
        }
    }
    pub const ALL: [MapCode; 7] = [
        MapCode::OobWrite,
        MapCode::DupReg,
        MapCode::Overlap,
        MapCode::PageMisalign,
        MapCode::LenBad,
        MapCode::CountMismatch,
        MapCode::NotReproducible,
    ];
}

/// 诊断码闭集查表（段外码返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MapCode::ALL.len() {
        if let Some(c) = MapCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75380 段闭集内"
}

// ── 段记录与写请求 ──────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegRec {
    /// 签名 4B 窗口（表身份）。
    pub sig: [u8; 4],
    /// 物理基址（页对齐）。
    pub phys: u64,
    /// 窗口长度（字节）。
    pub len: u32,
    /// 页对齐状态（登记时冻结）。
    pub page_aligned: bool,
}

impl SegRec {
    /// 窗口末（开区间端点，phys + len，checked 防回绕）。
    pub fn end(&self) -> Option<u64> {
        self.phys.checked_add(self.len as u64)
    }
    /// 写请求是否整段落在窗口内（半开区间 [phys, phys+len)）。
    pub fn covers(&self, addr: u64, wlen: u32) -> bool {
        let e = match self.end() {
            Some(e) => e,
            None => return false,
        };
        let wend = match addr.checked_add(wlen as u64) {
            Some(e) => e,
            None => return false,
        };
        addr >= self.phys && wend <= e && wend > addr
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteReq {
    pub addr: u64,
    pub len: u32,
}

// ── B19 内存账（段登记 + 写闸门） ───────────────────────────────
#[derive(Clone, Debug)]
pub struct B19Ledger {
    pub segs: Vec<SegRec>,
    /// 被拦越界写次数（显性记账）。
    pub blocked_writes: u32,
    /// 放行写次数。
    pub allowed_writes: u32,
    /// 被拒重复登记次数。
    pub dup_attempts: u32,
    /// 被拒非对齐登记次数。
    pub misalign_registrations: u32,
}

impl B19Ledger {
    pub fn new() -> B19Ledger {
        B19Ledger {
            segs: Vec::new(),
            blocked_writes: 0,
            allowed_writes: 0,
            dup_attempts: 0,
            misalign_registrations: 0,
        }
    }

    /// 段登记：页对齐闸 → 长度闸 → 重叠闸 → 重复闸（顺序固定）。
    pub fn register(&mut self, sig: [u8; 4], phys: u64, len: u32) -> Result<usize, u32> {
        if phys % PAGE_SIZE != 0 {
            self.misalign_registrations += 1;
            return Err(MapCode::PageMisalign.val());
        }
        if len == 0 || len > SEG_LEN_CAP {
            return Err(MapCode::LenBad.val());
        }
        let new_end = match phys.checked_add(len as u64) {
            Some(e) => e,
            None => return Err(MapCode::Overlap.val()),
        };
        let mut i = 0;
        while i < self.segs.len() {
            let e = match self.segs.get(i) {
                Some(e) => *e,
                None => break,
            };
            if e.phys == phys && e.sig == sig {
                self.dup_attempts += 1;
                return Err(MapCode::DupReg.val());
            }
            if let Some(e_end) = e.end() {
                if phys < e_end && e.phys < new_end {
                    return Err(MapCode::Overlap.val());
                }
            }
            i += 1;
        }
        self.segs.push(SegRec { sig, phys, len, page_aligned: true });
        Ok(self.segs.len() - 1)
    }

    /// 写闸门：整段落在唯一已登记窗口内放行，否则拦截计数。
    pub fn try_write(&mut self, req: WriteReq) -> Result<usize, u32> {
        let mut i = 0;
        while i < self.segs.len() {
            if let Some(e) = self.segs.get(i) {
                if e.covers(req.addr, req.len) {
                    self.allowed_writes += 1;
                    return Ok(i);
                }
            }
            i += 1;
        }
        self.blocked_writes += 1;
        Err(MapCode::OobWrite.val())
    }

    /// n 段 = 表数对拍。
    pub fn count_matches_tables(&self) -> bool {
        self.segs.len() == TABLE_COUNT
    }
}

/// 段窗查询（只读，登记账对拍用）。
pub fn seg_of(led: &B19Ledger, sig: &[u8; 4]) -> Option<SegRec> {
    let mut i = 0;
    while i < led.segs.len() {
        if let Some(e) = led.segs.get(i) {
            if e.sig == *sig {
                return Some(*e);
            }
        }
        i += 1;
    }
    None
}

// ── 机器语料：12 表区（页对齐，4 页间隔防重叠） ─────────────────
pub const MACHINE_SIGS: [[u8; 4]; 12] = [
    *b"XSDT", *b"FADT", *b"APIC", *b"HPET", *b"MCFG", *b"SRAT",
    *b"SLIT", *b"BGRT", *b"DSDT", *b"FACS", *b"SSDT", *b"RSDT",
];

/// 12 表区语料：基址 0x7F000000 起步进 4 页，长度 0x80..0x800 斜坡。
pub fn machine_regions() -> Vec<([u8; 4], u64, u32)> {
    let lens: [u32; 12] = [
        0x100, 0x200, 0x800, 0x80, 0x100, 0x180,
        0x140, 0x80, 0x400, 0x40, 0x300, 0xC0,
    ];
    let mut v: Vec<([u8; 4], u64, u32)> = Vec::new();
    let mut i = 0;
    while i < MACHINE_SIGS.len() {
        let sig = match MACHINE_SIGS.get(i) {
            Some(s) => *s,
            None => break,
        };
        let len = match lens.get(i) {
            Some(l) => *l,
            None => break,
        };
        let phys = 0x7F000000u64 + (i as u64) * (4 * PAGE_SIZE);
        v.push((sig, phys, len));
        i += 1;
    }
    v
}

// ── 黄金全链：登记 12 段 → 对拍 → 段内写放行 → 四类越界全拦 ────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = B19Ledger::new();
    for r in machine_regions().iter() {
        let (sig, phys, len) = *r;
        led.register(sig, phys, len)?;
    }
    if !led.count_matches_tables() {
        return Err(MapCode::CountMismatch.val());
    }
    // 段内写放行：每段首地址写 1 字节。
    let mut allowed = 0;
    for r in machine_regions().iter() {
        let (_, phys, _) = *r;
        if led.try_write(WriteReq { addr: phys, len: 1 }).is_ok() {
            allowed += 1;
        }
    }
    // 四类越界：段前 / 段后跨界 / 窗口间隙 / 全账之后。
    let oob_before = led.try_write(WriteReq { addr: 0x7EFFFFF0, len: 8 }).is_err();
    let oob_cross = led.try_write(WriteReq { addr: 0x7F000000 + 0x100 - 2, len: 4 }).is_err();
    let oob_gap = led.try_write(WriteReq { addr: 0x7F000000 + 4 * PAGE_SIZE - 0x10, len: 8 }).is_err();
    let oob_beyond = led.try_write(WriteReq { addr: 0x80000000, len: 0x100 }).is_err();
    let blocked = (oob_before as u32) + (oob_cross as u32) + (oob_gap as u32) + (oob_beyond as u32);
    if blocked != 4 || allowed != TABLE_COUNT as u32 {
        return Err(MapCode::OobWrite.val());
    }
    Ok((allowed, blocked, led.blocked_writes))
}

/// 段数对拍注入：缺登一段即破对拍。
pub fn count_mismatch_detectable() -> bool {
    let mut led = B19Ledger::new();
    let mut i = 0;
    while i + 1 < machine_regions().len() {
        if let Some((sig, phys, len)) = machine_regions().get(i) {
            let _ = led.register(*sig, *phys, *len);
        }
        i += 1;
    }
    !led.count_matches_tables()
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_ok() -> bool {
    golden_run() == golden_run()
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    for c in MapCode::ALL {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
fn register_ok() -> bool {
    match golden_run() {
        Ok((allowed, blocked, _)) => allowed == TABLE_COUNT as u32 && blocked == 4,
        Err(_) => false,
    }
}
fn dup_reject_ok() -> bool {
    let mut led = B19Ledger::new();
    let corpus = machine_regions();
    let first = match corpus.first() {
        Some(r) => *r,
        None => return false,
    };
    let _ = led.register(first.0, first.1, first.2);
    led.register(first.0, first.1, first.2) == Err(MapCode::DupReg.val())
        && led.dup_attempts == 1
}
fn overlap_reject_ok() -> bool {
    let mut led = B19Ledger::new();
    let corpus = machine_regions();
    let first = match corpus.first() {
        Some(r) => *r,
        None => return false,
    };
    let _ = led.register(first.0, first.1, first.2);
    // 与首段重叠：页对齐同基址换签名（SSDT 挤占 XSDT 窗口起点）。
    led.register(*b"SSDT", first.1, 0x100) == Err(MapCode::Overlap.val())
}
fn misalign_ok() -> bool {
    let mut led = B19Ledger::new();
    let e = led.register(*b"HPET", 0x7F000004, 0x80);
    e == Err(MapCode::PageMisalign.val())
        && led.misalign_registrations == 1
        && led.segs.is_empty()
}
fn write_gate_ok() -> bool {
    let mut led = B19Ledger::new();
    for r in machine_regions().iter() {
        let (sig, phys, len) = *r;
        let _ = led.register(sig, phys, len);
    }
    // 段末字节恰含放行。
    let last = match machine_regions().get(2) {
        Some(r) => *r,
        None => return false,
    };
    let tail_ok = led.try_write(WriteReq { addr: last.1 + last.2 as u64 - 1, len: 1 }).is_ok();
    // 末字节 +1 拦截。
    let tail_over = led.try_write(WriteReq { addr: last.1 + last.2 as u64, len: 1 }).is_err();
    tail_ok && tail_over
}
fn zero_len_ok() -> bool {
    let mut led = B19Ledger::new();
    led.register(*b"FACS", 0x7F100000, 0) == Err(MapCode::LenBad.val())
}
fn overflow_ok() -> bool {
    // 页对齐但 end 计算回绕的基址：checked_add 失败必须拒而非误登。
    let mut led = B19Ledger::new();
    let top_page = 0xFFFFFFFFFFFFF000u64;
    let e = led.register(*b"DSDT", top_page, 0x1000);
    e == Err(MapCode::Overlap.val()) && led.segs.is_empty()
}
fn codeset_ok() -> bool {
    for c in MapCode::ALL {
        if detail(c.val()) != c.detail() {
            return false;
        }
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 检查节（≥12 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let mut led = B19Ledger::new();
    for r in machine_regions().iter() {
        let (sig, phys, len) = *r;
        let _ = led.register(sig, phys, len);
    }
    v.push((
        "J1-12段登记n=表数对拍",
        led.count_matches_tables(),
        format!("段={} 表={}", led.segs.len(), TABLE_COUNT),
    ));
    v.push((
        "J1-登记三闸正常流",
        register_ok(),
        "对齐/长度/重叠/重复四闸受理序".to_string(),
    ));
    v.push((
        "J1-页对齐状态冻结",
        misalign_ok(),
        format!("非对齐拒 计数={} 页={:#x}", led.misalign_registrations, PAGE_SIZE),
    ));
    v.push((
        "J1-重复登记显性拒",
        dup_reject_ok(),
        "同签名同基址二拒计数".to_string(),
    ));
    v.push((
        "J1-区间重叠显性拒",
        overlap_reject_ok(),
        "半开区间相交即拒".to_string(),
    ));
    v.push((
        "J1-段内写放行",
        register_ok(),
        "12 段首地址写全放行".to_string(),
    ));
    v.push((
        "J1-越界写拦截100%",
        register_ok(),
        "段前/跨界/间隙/账后四路全拦".to_string(),
    ));
    v.push((
        "J1-写闸门边界",
        write_gate_ok(),
        "段末字节放行 / 末+1拦截".to_string(),
    ));
    v.push((
        "J2-零长登记拒",
        zero_len_ok(),
        "len=0 显性拒".to_string(),
    ));
    v.push((
        "J2-地址回绕安全",
        overflow_ok(),
        "u64 顶基址 checked_add 拒".to_string(),
    ));
    v.push((
        "J1-段数对拍注入必破",
        count_mismatch_detectable(),
        "缺登一段 11!=12 立判".to_string(),
    ));
    v.push((
        "J2-双跑一致",
        repro_ok(),
        "golden_run 两跑同值".to_string(),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:x} 窗口", SEG_BASE),
    ));
    v.push((
        "J3-诊断码闭集",
        codeset_ok(),
        format!("{} 码全查表命中", MapCode::ALL.len()),
    ));
    v
}

pub fn run_unx0455_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0455 表区映射登记（B19 账联动防重）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_twelve_segments() {
        let mut led = B19Ledger::new();
        for r in machine_regions().iter() {
            let (sig, phys, len) = *r;
            assert!(led.register(sig, phys, len).is_ok());
        }
        assert!(led.count_matches_tables());
        assert_eq!(led.segs.len(), TABLE_COUNT);
    }

    #[test]
    fn test_in_window_write_allowed() {
        let mut led = B19Ledger::new();
        for r in machine_regions().iter() {
            let (sig, phys, len) = *r;
            let _ = led.register(sig, phys, len);
        }
        let first = match machine_regions().first() {
            Some(r) => *r,
            None => (*b"XSDT", 0x7F000000, 0x100u32),
        };
        assert!(led.try_write(WriteReq { addr: first.1, len: 1 }).is_ok());
        assert_eq!(led.allowed_writes, 1);
    }

    #[test]
    fn test_oob_writes_all_blocked() {
        let g = golden_run();
        assert_eq!(g, Ok((TABLE_COUNT as u32, 4, 4)));
    }

    #[test]
    fn test_boundary_tail_byte() {
        assert!(write_gate_ok());
    }

    #[test]
    fn test_dup_and_overlap_rejected() {
        assert!(dup_reject_ok());
        assert!(overlap_reject_ok());
    }

    #[test]
    fn test_misalign_rejected_and_counted() {
        assert!(misalign_ok());
    }

    #[test]
    fn test_zero_len_and_overflow() {
        assert!(zero_len_ok());
        assert!(overflow_ok());
    }

    #[test]
    fn test_count_mismatch_detected() {
        assert!(count_mismatch_detectable());
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn test_segment_and_codeset() {
        assert!(segment_guard());
        assert!(codeset_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(0x753BF), "未知诊断码：不在 0x75380 段闭集内");
    }
}
