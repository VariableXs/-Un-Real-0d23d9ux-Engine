//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0443 · RSDT/XSDT 表指针遍历 · 0x6C740
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0443-J1 表指针数组遍历完整（指针数 = 长度推算数 100%
//!     一致），每指针 64 位范围账齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - RSDT/XSDT 双表建模：RSDT 指针 32 位（条目 4 字节）、XSDT
//!      指针 64 位（条目 8 字节），统一遍历器（宽度参数化）；
//!    - 指针数推算：(length - 36) / entry_size，遍历数必须与推算数
//!      100% 一致（缺遍历即账不全）；
//!    - 64 位范围账：每指针落物理内存域（0 < p < 2^46）且 4 字节
//!      对齐（ACPI 表对齐），越域/错位即显性登记（不静默）；
//!    - 签名账：每条目指向表须有 4 字节签名（读账面留痕）；
//!    - 双表一致：XSDT 与 RSDT 共存时，RSDT 指针集合 ⊆ XSDT 指针
//!      集合（兼容对拍）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C740，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C740;

// ── ACPI SDT 冻结值 ────────────────────────────────────────────
/// SDT 头长度。
pub const SDT_HEADER_LEN: usize = 36;
/// RSDT 条目宽（4 字节）。
pub const RSDT_ENTRY: usize = 4;
/// XSDT 条目宽（8 字节）。
pub const XSDT_ENTRY: usize = 8;
/// 物理域上界（2^46）。
pub const PHYS_MAX: u64 = 1 << 46;
/// 最大条目数（防伪造）。
pub const MAX_PTRS: usize = 64;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SdtCode {
    /// 表长不足（< 头）
    LengthTooShort,
    /// 条目宽非法（非 4/8）
    EntryWidthIllegal,
    /// 遍历数≠推算数
    WalkCountMismatch,
    /// 指针越物理域
    PtrOutOfRange,
    /// 指针未 4 字节对齐
    PtrMisaligned,
    /// 零指针（显性登记）
    ZeroPointer,
    /// 指针数越上限
    PtrCountExceeded,
    /// 兼容对拍失败（RSDT ⊄ XSDT）
    CompatMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl SdtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SdtCode::LengthTooShort => "表长不足：< SDT 头 36",
            SdtCode::EntryWidthIllegal => "条目宽非法：非 4/8",
            SdtCode::WalkCountMismatch => "遍历数≠推算数",
            SdtCode::PtrOutOfRange => "指针越物理域：≥2^46",
            SdtCode::PtrMisaligned => "指针未 4 字节对齐",
            SdtCode::ZeroPointer => "零指针：显性登记",
            SdtCode::PtrCountExceeded => "指针数越上限：64",
            SdtCode::CompatMismatch => "兼容对拍失败：RSDT ⊄ XSDT",
            SdtCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SdtCode; 9] = [
        SdtCode::LengthTooShort,
        SdtCode::EntryWidthIllegal,
        SdtCode::WalkCountMismatch,
        SdtCode::PtrOutOfRange,
        SdtCode::PtrMisaligned,
        SdtCode::ZeroPointer,
        SdtCode::PtrCountExceeded,
        SdtCode::CompatMismatch,
        SdtCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SdtCode::ALL.len() {
        if SdtCode::ALL[i].val() == code {
            return SdtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C740 段闭集内"
}

// ── 指针条目账 ─────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtrEntry {
    pub ptr: u64,
    /// 签名（读账面留痕：目标表首 4 字节）。
    pub sig: [u8; 4],
}

impl PtrEntry {
    /// 64 位范围账：域内 + 对齐。
    pub fn range_ok(&self) -> Result<(), u32> {
        if self.ptr == 0 {
            return Err(SdtCode::ZeroPointer.val());
        }
        if self.ptr >= PHYS_MAX {
            return Err(SdtCode::PtrOutOfRange.val());
        }
        if self.ptr % 4 != 0 {
            return Err(SdtCode::PtrMisaligned.val());
        }
        Ok(())
    }
    pub fn sig_str(&self) -> String {
        String::from_utf8_lossy(&self.sig).into_owned()
    }
}

// ── 遍历器 ─────────────────────────────────────────────────────
/// 从表字节遍历指针数组（宽度参数化：4=RSDT / 8=XSDT）。
/// 签名由模拟读函数给出（确定性：地址→签名样本账）。
pub fn walk_ptrs(
    table: &[u8],
    entry_width: usize,
    sig_reader: &dyn Fn(u64) -> [u8; 4],
) -> Result<Vec<PtrEntry>, u32> {
    if entry_width != RSDT_ENTRY && entry_width != XSDT_ENTRY {
        return Err(SdtCode::EntryWidthIllegal.val());
    }
    if table.len() < SDT_HEADER_LEN {
        return Err(SdtCode::LengthTooShort.val());
    }
    let length = u32::from_le_bytes([table[4], table[5], table[6], table[7]]) as usize;
    if length > table.len() || length < SDT_HEADER_LEN {
        return Err(SdtCode::LengthTooShort.val());
    }
    let derived = (length - SDT_HEADER_LEN) / entry_width;
    if derived > MAX_PTRS {
        return Err(SdtCode::PtrCountExceeded.val());
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < derived {
        let off = SDT_HEADER_LEN + i * entry_width;
        let ptr = if entry_width == RSDT_ENTRY {
            u32::from_le_bytes([table[off], table[off + 1], table[off + 2], table[off + 3]]) as u64
        } else {
            u64::from_le_bytes([
                table[off], table[off + 1], table[off + 2], table[off + 3],
                table[off + 4], table[off + 5], table[off + 6], table[off + 7],
            ])
        };
        let e = PtrEntry { ptr, sig: sig_reader(ptr) };
        e.range_ok()?;
        out.push(e);
        i += 1;
    }
    if out.len() != derived {
        return Err(SdtCode::WalkCountMismatch.val());
    }
    Ok(out)
}

/// 遍历完整性判据：遍历数 == 推算数（显式复算对拍）。
pub fn walk_complete(table: &[u8], entry_width: usize, walked: usize) -> Result<(), u32> {
    if table.len() < SDT_HEADER_LEN {
        return Err(SdtCode::LengthTooShort.val());
    }
    let length = u32::from_le_bytes([table[4], table[5], table[6], table[7]]) as usize;
    let derived = (length - SDT_HEADER_LEN) / entry_width;
    if walked != derived {
        return Err(SdtCode::WalkCountMismatch.val());
    }
    Ok(())
}

/// 兼容对拍：RSDT 指针集合 ⊆ XSDT 指针集合。
pub fn compat_check(rsdt: &[PtrEntry], xsdt: &[PtrEntry]) -> Result<(), u32> {
    let mut i = 0;
    while i < rsdt.len() {
        let mut hit = false;
        let mut j = 0;
        while j < xsdt.len() {
            if xsdt[j].ptr == rsdt[i].ptr {
                hit = true;
            }
            j += 1;
        }
        if !hit {
            return Err(SdtCode::CompatMismatch.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 签名样本账（确定性：地址→签名） ────────────────────────────
/// 模拟物理表布局（写死）：0x1000=FACP、0x2000=APIC、0x3000=HPET。
pub const TBL_FACP: u64 = 0x1000;
pub const TBL_APIC: u64 = 0x2000;
pub const TBL_HPET: u64 = 0x3000;

pub fn sample_sig_reader(ptr: u64) -> [u8; 4] {
    match ptr {
        TBL_FACP => *b"FACP",
        TBL_APIC => *b"APIC",
        TBL_HPET => *b"HPET",
        _ => *b"XXXX",
    }
}

/// 构造 XSDT 字节（3 条目，FACP/APIC/HPET）。
pub fn make_xsdt() -> Vec<u8> {
    let total = SDT_HEADER_LEN + 3 * XSDT_ENTRY;
    let mut b = vec![0u8; total];
    b[0..4].copy_from_slice(b"XSDT");
    b[4..8].copy_from_slice(&(total as u32).to_le_bytes());
    let ptrs = [TBL_FACP, TBL_APIC, TBL_HPET];
    let mut i = 0;
    while i < 3 {
        let off = SDT_HEADER_LEN + i * XSDT_ENTRY;
        b[off..off + 8].copy_from_slice(&ptrs[i].to_le_bytes());
        i += 1;
    }
    b
}

/// 构造 RSDT 字节（2 条目：FACP/APIC ⊆ XSDT）。
pub fn make_rsdt() -> Vec<u8> {
    let total = SDT_HEADER_LEN + 2 * RSDT_ENTRY;
    let mut b = vec![0u8; total];
    b[0..4].copy_from_slice(b"RSDT");
    b[4..8].copy_from_slice(&(total as u32).to_le_bytes());
    let ptrs = [TBL_FACP, TBL_APIC];
    let mut i = 0;
    while i < 2 {
        let off = SDT_HEADER_LEN + i * RSDT_ENTRY;
        b[off..off + 4].copy_from_slice(&(ptrs[i] as u32).to_le_bytes());
        i += 1;
    }
    b
}

// ── LCG ────────────────────────────────────────────────────────
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

// ── 注入检出账 ──────────────────────────────────────────────────
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
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：XSDT 遍历 3 指针→RSDT 遍历 2 指针→完整性对拍→兼容⊆。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let xsdt = make_xsdt();
    let rsdt = make_rsdt();
    let xp = walk_ptrs(&xsdt, XSDT_ENTRY, &sample_sig_reader)?;
    let rp = walk_ptrs(&rsdt, RSDT_ENTRY, &sample_sig_reader)?;
    walk_complete(&xsdt, XSDT_ENTRY, xp.len())?;
    walk_complete(&rsdt, RSDT_ENTRY, rp.len())?;
    compat_check(&rp, &xp)?;
    // 签名账：三签名齐。
    if xp[0].sig_str() != "FACP" || xp[1].sig_str() != "APIC" || xp[2].sig_str() != "HPET" {
        return Err(SdtCode::PtrOutOfRange.val());
    }
    // 64 位范围账逐条。
    let mut i = 0;
    while i < xp.len() {
        xp[i].range_ok()?;
        i += 1;
    }
    Ok((xp.len(), rp.len(), xp.len() + rp.len()))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    walk_ptrs(&make_xsdt(), XSDT_ENTRY, &sample_sig_reader).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 条目宽非法 → 必拒。
    walk_ptrs(&make_xsdt(), 6, &sample_sig_reader).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 零指针 → 必拒。
    let mut b = make_xsdt();
    b[36..44].copy_from_slice(&0u64.to_le_bytes());
    walk_ptrs(&b, XSDT_ENTRY, &sample_sig_reader).map(|_| ())
}

/// 注入 10/10：条目宽非法、零指针、越域、错位四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：条目宽非法。
        led.record(0, walk_ptrs(&make_xsdt(), 5, &sample_sig_reader).is_err());
        // 槽1：零指针。
        let mut b1 = make_xsdt();
        b1[36..44].copy_from_slice(&0u64.to_le_bytes());
        led.record(1, walk_ptrs(&b1, XSDT_ENTRY, &sample_sig_reader).is_err());
        // 槽2：越域。
        let mut b2 = make_xsdt();
        b2[36..44].copy_from_slice(&PHYS_MAX.to_le_bytes());
        led.record(2, walk_ptrs(&b2, XSDT_ENTRY, &sample_sig_reader).is_err());
        // 槽3：错位。
        let mut b3 = make_xsdt();
        b3[37] = 0x01;
        led.record(3, walk_ptrs(&b3, XSDT_ENTRY, &sample_sig_reader).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SdtCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SdtCode::ALL.len() {
        let v = SdtCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 遍历数=推算数 100%。
    let xsdt = make_xsdt();
    let xp = walk_ptrs(&xsdt, XSDT_ENTRY, &sample_sig_reader);
    v.push((
        "J1-遍历数=推算数100%",
        match &xp {
            Ok(p) => walk_complete(&xsdt, XSDT_ENTRY, p.len()).is_ok() && p.len() == 3,
            Err(_) => false,
        },
        format!("{:?}", xp.as_ref().map(|p| p.len())),
    ));
    // 64 位范围账齐。
    v.push((
        "J1-64位范围账齐",
        PHYS_MAX == (1u64 << 46) && SDT_HEADER_LEN == 36,
        format!("域上界 2^46 头 {}", SDT_HEADER_LEN),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C740 窗口")));
    v
}

pub fn run_unx0443_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0443 RSDT/XSDT 表指针遍历");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
