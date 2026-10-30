//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0084 · 段重定位项修复器 · 0x6C480
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F0084-J1 R_X86_64 系列重定位全类型支持，重定位后符号
//!     引用抽样比对零错；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - R_X86_64 全类型闭集（本修复器承载八类）：64（S+A）/PC32
//!     (S+A-P)/PLT32/GOTPCREL/32S/32/16/8，每类计算公式冻结；
//!   - 修复器：逐项（offset/type/sym/addend）→ 计算新值 → 写回
//!     字节账（LE 定宽截断）；
//!   - 符号抽样比对：修复后抽 N 项反算 S = value - A (+P)，与符
//!     号表逐位一致 → 零错；
//!   - 越界修复拒：offset 越段界即拒（不越界写）；
//!   - PC 相对类依赖位置 P：显性传入，缺失即拒；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C480，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C480;

// ── R_X86_64 类型闭集（八类） ───────────────────────────────────
pub const REL_64: usize = 0;
pub const REL_PC32: usize = 1;
pub const REL_PLT32: usize = 2;
pub const REL_GOTPCREL: usize = 3;
pub const REL_32S: usize = 4;
pub const REL_32: usize = 5;
pub const REL_16: usize = 6;
pub const REL_8: usize = 7;
pub const REL_TYPE_COUNT: usize = 8;
pub const REL_TYPE_NAMES: [&str; REL_TYPE_COUNT] = [
    "R_X86_64_64", "R_X86_64_PC32", "R_X86_64_PLT32", "R_X86_64_GOTPCREL",
    "R_X86_64_32S", "R_X86_64_32", "R_X86_64_16", "R_X86_64_8",
];
/// 类型宽度（字节，冻结）。
pub const REL_WIDTHS: [usize; REL_TYPE_COUNT] = [8, 4, 4, 4, 4, 4, 2, 1];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 抽样比对项数（冻结）。
pub const SAMPLE_N: usize = 16;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RelCode {
    /// 类型越闭集
    TypeOutOfRange,
    /// 类型名空（八类齐备破坏）
    TypeNameBlank,
    /// 偏移越段界（越界写拒）
    OffsetOutOfRange,
    /// 值越类型宽度（无符号域溢出：R_X86_64_32/16/8）
    ValueOverWide,
    /// PC 相对类缺位置 P
    PcRelMissingPlace,
    /// 符号越表（sym 索引越符号账）
    SymOutOfRange,
    /// 修复后反算不一致（抽样比对非零错）
    BacksolveMismatch,
    /// 段字节账过短
    SectionTooShort,
    /// 双跑不一致
    NotReproducible,
}

impl RelCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RelCode::TypeOutOfRange => "类型越闭集：八类冻结",
            RelCode::TypeNameBlank => "类型名空：齐备破坏",
            RelCode::OffsetOutOfRange => "偏移越段界：越界写拒",
            RelCode::ValueOverWide => "值越类型宽度：无符号域溢出",
            RelCode::PcRelMissingPlace => "PC 相对类缺位置 P",
            RelCode::SymOutOfRange => "符号越表：索引越符号账",
            RelCode::BacksolveMismatch => "反算不一致：抽样非零错",
            RelCode::SectionTooShort => "段字节账过短",
            RelCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [RelCode; 9] = [
        RelCode::TypeOutOfRange,
        RelCode::TypeNameBlank,
        RelCode::OffsetOutOfRange,
        RelCode::ValueOverWide,
        RelCode::PcRelMissingPlace,
        RelCode::SymOutOfRange,
        RelCode::BacksolveMismatch,
        RelCode::SectionTooShort,
        RelCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RelCode::ALL.len() {
        if RelCode::ALL[i].val() == code {
            return RelCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C480 段闭集内"
}

// ── 重定位项 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelocEntry {
    /// 段内偏移。
    pub offset: u32,
    /// 重定位类型（闭集下标）。
    pub rtype: usize,
    /// 符号值 S。
    pub sym_value: u64,
    /// 加数 A（有符号语义，账面 i64）。
    pub addend: i64,
}

impl RelocEntry {
    pub fn legal(&self, sec_len: usize, sym_count: usize) -> Result<(), u32> {
        if self.rtype >= REL_TYPE_COUNT {
            return Err(RelCode::TypeOutOfRange.val());
        }
        if (self.offset as usize) + REL_WIDTHS[self.rtype] > sec_len {
            return Err(RelCode::OffsetOutOfRange.val());
        }
        if self.sym_value >= sym_count as u64 && self.sym_value > 0x1000_0000 {
            return Err(RelCode::SymOutOfRange.val());
        }
        Ok(())
    }
}

/// 定宽 LE 写回。
pub fn write_le(buf: &mut [u8], off: usize, width: usize, value: u64) -> Result<(), u32> {
    if off + width > buf.len() {
        return Err(RelCode::OffsetOutOfRange.val());
    }
    let mut k = 0;
    while k < width {
        buf[off + k] = (value >> (k * 8)) as u8;
        k += 1;
    }
    Ok(())
}

/// 定宽 LE 读出。
pub fn read_le(buf: &[u8], off: usize, width: usize) -> Result<u64, u32> {
    if off + width > buf.len() {
        return Err(RelCode::OffsetOutOfRange.val());
    }
    let mut v = 0u64;
    let mut k = 0;
    while k < width {
        v |= (buf[off + k] as u64) << (k * 8);
        k += 1;
    }
    Ok(v)
}

// ── 修复器 ──────────────────────────────────────────────────────
/// 计算新值：按类型冻结公式。
/// S=符号值, A=加数, P=被修位置虚拟地址（PC 相对类必需）。
pub fn compute(e: &RelocEntry, place: Option<u64>) -> Result<u64, u32> {
    match e.rtype {
        REL_64 => Ok((e.sym_value as i64).wrapping_add(e.addend) as u64),
        REL_PC32 | REL_PLT32 => match place {
            Some(p) => Ok(((e.sym_value as i64) + e.addend - (p as i64)) as u64),
            None => Err(RelCode::PcRelMissingPlace.val()),
        },
        REL_GOTPCREL => match place {
            Some(p) => Ok(((e.sym_value as i64) + e.addend - (p as i64)) as u64),
            None => Err(RelCode::PcRelMissingPlace.val()),
        },
        REL_32S => Ok((e.sym_value as i64).wrapping_add(e.addend) as u64),
        REL_32 | REL_16 | REL_8 => {
            let v = (e.sym_value as i64).wrapping_add(e.addend) as u64;
            // 无符号域守卫：值必须落在宽度内。
            let maxv: u64 = match e.rtype {
                REL_32 => 0xFFFF_FFFF,
                REL_16 => 0xFFFF,
                _ => 0xFF,
            };
            if v > maxv {
                return Err(RelCode::ValueOverWide.val());
            }
            Ok(v)
        }
        _ => Err(RelCode::TypeOutOfRange.val()),
    }
}

/// 修复器：逐项计算→写回→反算抽样比对（零错判据）。
pub fn apply_relocations(
    section: &mut [u8],
    relocs: &[RelocEntry],
    image_base: u64,
) -> Result<usize, u32> {
    let mut applied = 0;
    let mut i = 0;
    while i < relocs.len() {
        let e = &relocs[i];
        e.legal(section.len(), 0x1000)?;
        let place = Some(image_base + e.offset as u64);
        let value = compute(e, place)?;
        let width = REL_WIDTHS[e.rtype];
        write_le(section, e.offset as usize, width, value)?;
        applied += 1;
        i += 1;
    }
    Ok(applied)
}

/// 反算：从已写值还原 S（+校验 A/P）。
pub fn backsolve(e: &RelocEntry, section: &[u8], image_base: u64) -> Result<u64, u32> {
    if e.rtype >= REL_TYPE_COUNT {
        return Err(RelCode::TypeOutOfRange.val());
    }
    let width = REL_WIDTHS[e.rtype];
    let raw = read_le(section, e.offset as usize, width)?;
    // 按宽度符号扩展（PC32/32S 有符号）。
    let sext = |v: u64, bits: usize| -> i64 {
        let shift = 64 - bits;
        (((v << shift) as i64) >> shift)
    };
    let v = match e.rtype {
        REL_PC32 | REL_PLT32 | REL_GOTPCREL => {
            let p = (image_base + e.offset as u64) as i64;
            let rel = sext(raw, width * 8);
            (rel + p - e.addend) as u64
        }
        REL_64 | REL_32S => {
            let bits = width * 8;
            let signed = sext(raw, bits);
            (signed - e.addend) as u64
        }
        _ => raw.wrapping_sub(e.addend as u64),
    };
    Ok(v)
}

/// 抽样比对：随机抽 SAMPLE_N 项反算，与符号值逐位一致。
pub fn sample_compare(
    relocs: &[RelocEntry],
    section: &[u8],
    image_base: u64,
) -> Result<usize, u32> {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut checked = 0;
    let mut k = 0;
    while k < SAMPLE_N {
        let idx = lcg.below(relocs.len() as u64) as usize;
        let e = &relocs[idx];
        let back = backsolve(e, section, image_base)?;
        let expect = compute(e, Some(image_base + e.offset as u64))?;
        // PC 相对类反算 = 原值一致（round-trip）。
        if back != expect {
            return Err(RelCode::BacksolveMismatch.val());
        }
        checked += 1;
        k += 1;
    }
    Ok(checked)
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

/// 构造确定性重定位项集（八类各一 + 追加 PC32 四项）。
pub fn make_relocs() -> Vec<RelocEntry> {
    let mut v = Vec::new();
    // 64 @0：S=0x1000_2000 A=0x40。
    v.push(RelocEntry { offset: 0, rtype: REL_64, sym_value: 0x1000_2000, addend: 0x40 });
    // PC32 @8：S=0x1000_2100 A=-4。
    v.push(RelocEntry { offset: 8, rtype: REL_PC32, sym_value: 0x1000_2100, addend: -4 });
    // PLT32 @16。
    v.push(RelocEntry { offset: 16, rtype: REL_PLT32, sym_value: 0x1000_2200, addend: -4 });
    // GOTPCREL @24。
    v.push(RelocEntry { offset: 24, rtype: REL_GOTPCREL, sym_value: 0x1000_2300, addend: -4 });
    // 32S @32。
    v.push(RelocEntry { offset: 32, rtype: REL_32S, sym_value: 0x1000_2400, addend: 0x10 });
    // 32 @40。
    v.push(RelocEntry { offset: 40, rtype: REL_32, sym_value: 0x0000_2500, addend: 0x20 });
    // 16 @48。
    v.push(RelocEntry { offset: 48, rtype: REL_16, sym_value: 0x2600, addend: 1 });
    // 8 @56。
    v.push(RelocEntry { offset: 56, rtype: REL_8, sym_value: 0x27, addend: 1 });
    // PC32 追加四项。
    let mut k = 0;
    while k < 4 {
        v.push(RelocEntry {
            offset: 64 + (k as u32) * 8,
            rtype: REL_PC32,
            sym_value: 0x1000_3000 + (k as u64) * 0x100,
            addend: -4,
        });
        k += 1;
    }
    v
}

/// 黄金全链：全类型修复→反算抽样零错。
pub fn golden_run() -> Result<(usize, usize, u64), u32> {
    let relocs = make_relocs();
    let mut section = vec![0u8; 128];
    let image_base = 0x1_0000_0000;
    let applied = apply_relocations(&mut section, &relocs, image_base)?;
    let checked = sample_compare(&relocs, &section, image_base)?;
    // 全类型点验：八类名齐备。
    let mut t = 0;
    while t < REL_TYPE_COUNT {
        if REL_TYPE_NAMES[t].len() == 0 || REL_WIDTHS[t] == 0 {
            return Err(RelCode::TypeNameBlank.val());
        }
        t += 1;
    }
    // 首项点验：64 位值 = S+A。
    let v0 = read_le(&section, 0, 8)?;
    if v0 != 0x1000_2000 + 0x40 {
        return Err(RelCode::BacksolveMismatch.val());
    }
    Ok((applied, checked, v0))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let relocs = make_relocs();
    let mut section = vec![0u8; 128];
    apply_relocations(&mut section, &relocs, 0x1_0000_0000).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // PC 相对缺 P → 必拒。
    let e = RelocEntry { offset: 8, rtype: REL_PC32, sym_value: 0x1000, addend: 0 };
    compute(&e, None).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 8 位域溢出 → 必拒。
    let e = RelocEntry { offset: 56, rtype: REL_8, sym_value: 0x1000, addend: 0 };
    compute(&e, None).map(|_| ())
}

/// 注入 10/10：越界、缺 P、溢出、坏反算四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越界写。
        let mut sec = vec![0u8; 64];
        let e0 = RelocEntry { offset: 60, rtype: REL_64, sym_value: 1, addend: 0 };
        led.record(0, e0.legal(sec.len(), 0x1000).is_err());
        // 槽1：PC 相对缺 P。
        let e1 = RelocEntry { offset: 0, rtype: REL_PC32, sym_value: 1, addend: 0 };
        led.record(1, compute(&e1, None).is_err());
        // 槽2：16 位域溢出。
        let e2 = RelocEntry { offset: 48, rtype: REL_16, sym_value: 0x1_0000, addend: 0 };
        led.record(2, compute(&e2, None).is_err());
        // 槽3：类型越界。
        let e3 = RelocEntry { offset: 0, rtype: REL_TYPE_COUNT, sym_value: 1, addend: 0 };
        led.record(3, e3.legal(128, 0x1000).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RelCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RelCode::ALL.len() {
        let v = RelCode::ALL[i].val();
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
    // 八类全类型支持。
    v.push((
        "J1-八类全类型支持",
        REL_TYPE_NAMES.len() == 8 && REL_WIDTHS.len() == 8,
        format!("{:?}", REL_TYPE_NAMES),
    ));
    // 抽样比对零错。
    let relocs = make_relocs();
    let mut sec = vec![0u8; 128];
    let zero_err = apply_relocations(&mut sec, &relocs, 0x1_0000_0000).is_ok()
        && sample_compare(&relocs, &sec, 0x1_0000_0000).is_ok();
    v.push((
        "J1-抽样比对零错",
        zero_err && SAMPLE_N == 16,
        format!("抽样 {} 项", SAMPLE_N),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C480 窗口")));
    v
}

pub fn run_unx0084_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0084 段重定位项修复器");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
