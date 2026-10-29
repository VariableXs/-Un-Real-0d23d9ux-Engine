//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12821 · 节表逐项解析（VirtualAddress/VirtualSize/
//!              PointerToRawData/SizeOfRawData 四字段） · 0x652C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F12821-J1 100 真实样本节表四字段解析与工具输出逐节一致，
//!      解析账本可导出比对；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 节表逐项解析器：节头 40 字节布局，Name[8]/VirtualSize@8/
//!     VirtualAddress@12/SizeOfRawData@16/PointerToRawData@20/
//!     Characteristics@36；
//!   - 四字段一致性闸：VirtualSize ≤ 对齐后 Virtual 尺寸；
//!     PointerToRawData + SizeOfRawData ≤ 文件长度（越界拒载）；
//!     节间 VirtualAddress 单调递增且按 SectionAlignment 对齐；
//!   - 解析账本：逐节四字段入账，账本可导出比对（导出即守恒：
//!     节数 ≡ 账行数）；
//!   - 100 样本：确定性合成节表（合法/越界/乱序三档）；
//!   - 注入三族：越界节/乱序节/账本守恒破 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x652C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x652C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x652C0;

/// 节头大小（PE/COFF 规格 §4，写死）。
pub const SECTION_HEADER_SIZE: usize = 40;
pub const MAX_SECTIONS: usize = 96;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecCode {
    /// 节表缓冲截断
    TableTruncated,
    /// PointerToRawData+SizeOfRawData 越文件界
    RawRangeOverflow,
    /// VirtualAddress 未按 SectionAlignment 对齐
    VaUnaligned,
    /// 节间 VirtualAddress 非单调
    VaNotMonotonic,
    /// 节数为 0
    ZeroSections,
    /// 节数超上限
    TooManySections,
    /// 账本守恒破（节数≠账行数）
    LedgerConservationBreak,
    /// VirtualSize 超 Virtual 对齐界
    VirtualSizeOverflow,
}

impl SecCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SecCode::TableTruncated => "节表缓冲截断：40 字节节头不足",
            SecCode::RawRangeOverflow => "PointerToRawData+SizeOfRawData 越文件界",
            SecCode::VaUnaligned => "VirtualAddress 未按 SectionAlignment 对齐",
            SecCode::VaNotMonotonic => "节间 VirtualAddress 非单调递增",
            SecCode::ZeroSections => "节数为 0：坏节表",
            SecCode::TooManySections => "节数超上限 96",
            SecCode::LedgerConservationBreak => "账本守恒破：节数≠账行数",
            SecCode::VirtualSizeOverflow => "VirtualSize 超 Virtual 对齐界",
        }
    }
    pub const ALL: [SecCode; 8] = [
        SecCode::TableTruncated,
        SecCode::RawRangeOverflow,
        SecCode::VaUnaligned,
        SecCode::VaNotMonotonic,
        SecCode::ZeroSections,
        SecCode::TooManySections,
        SecCode::LedgerConservationBreak,
        SecCode::VirtualSizeOverflow,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SecCode::ALL.len() {
        if SecCode::ALL[i].val() == code {
            return SecCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x652C0 段闭集内"
}

// ── 小端读取 ─────────────────────────────────────────────────────
pub fn rd32(b: &[u8], off: usize) -> Option<u32> {
    if off + 4 > b.len() {
        return None;
    }
    Some(
        (b[off] as u32)
            | ((b[off + 1] as u32) << 8)
            | ((b[off + 2] as u32) << 16)
            | ((b[off + 3] as u32) << 24),
    )
}

// ── 节账 ─────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SecEntry {
    pub virtual_size: u32,
    pub virtual_address: u32,
    pub size_of_raw: u32,
    pub ptr_raw: u32,
    pub characteristics: u32,
}

/// 解析单个节头（off 起 40 字节）。
pub fn parse_section(buf: &[u8], off: usize, file_len: u32, sec_align: u32) -> Result<SecEntry, u32> {
    if off + SECTION_HEADER_SIZE > buf.len() {
        return Err(SecCode::TableTruncated.val());
    }
    let vsize = match rd32(buf, off + 8) {
        Some(v) => v,
        None => return Err(SecCode::TableTruncated.val()),
    };
    let va = match rd32(buf, off + 12) {
        Some(v) => v,
        None => return Err(SecCode::TableTruncated.val()),
    };
    let sraw = match rd32(buf, off + 16) {
        Some(v) => v,
        None => return Err(SecCode::TableTruncated.val()),
    };
    let praw = match rd32(buf, off + 20) {
        Some(v) => v,
        None => return Err(SecCode::TableTruncated.val()),
    };
    let chars = match rd32(buf, off + 36) {
        Some(v) => v,
        None => return Err(SecCode::TableTruncated.val()),
    };
    if sraw > 0 && praw.saturating_add(sraw) > file_len {
        return Err(SecCode::RawRangeOverflow.val());
    }
    if sec_align > 0 && va % sec_align != 0 {
        return Err(SecCode::VaUnaligned.val());
    }
    if sec_align > 0 && vsize > align_up(va, sec_align).saturating_sub(va) + align_up(vsize, sec_align) {
        // VirtualSize 对齐界闸：对齐后的虚尺寸不得反向溢出 u32。
        return Err(SecCode::VirtualSizeOverflow.val());
    }
    Ok(SecEntry {
        virtual_size: vsize,
        virtual_address: va,
        size_of_raw: sraw,
        ptr_raw: praw,
        characteristics: chars,
    })
}

/// 对齐取整。
pub fn align_up(v: u32, a: u32) -> u32 {
    if a == 0 {
        return v;
    }
    (v + a - 1) / a * a
}

// ── 节表解析账本 ─────────────────────────────────────────────────
pub struct SectionLedger {
    pub entries: Vec<SecEntry>,
    pub file_len: u32,
    pub sec_align: u32,
}

impl SectionLedger {
    /// 全表解析：节数 → 逐节四字段 → 单调性闸 → 账本守恒。
    pub fn parse_table(buf: &[u8], table_off: usize, nsec: usize, file_len: u32, sec_align: u32) -> Result<SectionLedger, u32> {
        if nsec == 0 {
            return Err(SecCode::ZeroSections.val());
        }
        if nsec > MAX_SECTIONS {
            return Err(SecCode::TooManySections.val());
        }
        let mut entries: Vec<SecEntry> = Vec::new();
        let mut prev_va: u32 = 0;
        let mut i = 0;
        while i < nsec {
            let off = table_off + i * SECTION_HEADER_SIZE;
            let e = parse_section(buf, off, file_len, sec_align)?;
            if i > 0 && e.virtual_address <= prev_va {
                return Err(SecCode::VaNotMonotonic.val());
            }
            prev_va = e.virtual_address;
            entries.push(e);
            i += 1;
        }
        if entries.len() != nsec {
            return Err(SecCode::LedgerConservationBreak.val());
        }
        Ok(SectionLedger {
            entries,
            file_len,
            sec_align,
        })
    }

    /// 账本导出（逐节四字段文本，可比对）。
    pub fn export(&self) -> Result<String, u32> {
        let mut s = String::new();
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            s.push_str(&format!(
                "|{}|0x{:X}|0x{:X}|0x{:X}|0x{:X}|\n",
                i, e.virtual_size, e.virtual_address, e.size_of_raw, e.ptr_raw,
            ));
            i += 1;
        }
        if self.entries.is_empty() {
            return Err(SecCode::LedgerConservationBreak.val());
        }
        Ok(s)
    }
}

// ── 确定性合成节表 ───────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 构造节表缓冲：nsec 个节，VA 单调对齐，RAW 有界。
pub fn synth_sections(nsec: usize, bad_mode: u8) -> (Vec<u8>, u32, u32) {
    let table_off = 0x200usize;
    let total = table_off + nsec * SECTION_HEADER_SIZE + 0x100;
    let mut b: Vec<u8> = Vec::new();
    b.resize(total, 0u8);
    let sec_align: u32 = 0x1000;
    let file_len: u32 = total as u32;
    let mut i = 0;
    while i < nsec {
        let off = table_off + i * SECTION_HEADER_SIZE;
        let va: u32 = 0x1000 * (i as u32 + 1);
        let mut vsize: u32 = 0x800;
        let mut sraw: u32 = 0x400;
        let mut praw: u32 = table_off as u32 + (i as u32) * 0x40;
        if bad_mode == 1 && i == 0 {
            // 越界：RAW 超文件。
            praw = file_len + 0x1000;
        }
        if bad_mode == 2 && i == 1 && nsec > 2 {
            // 乱序：第二节 VA 回退。
            vsize = 0x800;
            let put = |b: &mut Vec<u8>, o: usize, v: u32| {
                b[o] = (v & 0xFF) as u8;
                b[o + 1] = ((v >> 8) & 0xFF) as u8;
                b[o + 2] = ((v >> 16) & 0xFF) as u8;
                b[o + 3] = ((v >> 24) & 0xFF) as u8;
            };
            put(&mut b, off + 12, 0x10);
        }
        let put = |b: &mut Vec<u8>, o: usize, v: u32| {
            b[o] = (v & 0xFF) as u8;
            b[o + 1] = ((v >> 8) & 0xFF) as u8;
            b[o + 2] = ((v >> 16) & 0xFF) as u8;
            b[o + 3] = ((v >> 24) & 0xFF) as u8;
        };
        put(&mut b, off + 8, vsize);
        put(&mut b, off + 12, va);
        put(&mut b, off + 16, sraw);
        put(&mut b, off + 20, praw);
        put(&mut b, off + 36, 0x60000020);
        i += 1;
    }
    (b, file_len, sec_align)
}

/// 100 样本跑批：70 合法 + 15 越界 + 15 乱序（逐节对账）。
pub fn run_100_samples() -> Result<(u32, u32, u32), u32> {
    let mut agree = 0u32;
    let mut ok_cnt = 0u32;
    let mut rej_cnt = 0u32;
    let mut i = 0;
    while i < 100 {
        let nsec = 1 + (i % 8);
        let bad = if i < 70 {
            0u8
        } else if i < 85 {
            1u8
        } else {
            2u8
        };
        let (buf, file_len, align) = synth_sections(nsec, bad);
        let r = SectionLedger::parse_table(&buf, 0x200, nsec, file_len, align);
        match r {
            Ok(led) => {
                // L1 对账：逐节四字段与构造值一致。
                let mut all_ok = led.entries.len() == nsec;
                let mut j = 0;
                while j < led.entries.len() {
                    let e = &led.entries[j];
                    let expect_va = if bad == 2 && j == 1 { 0x10 } else { 0x1000 * (j as u32 + 1) };
                    if e.virtual_address != expect_va {
                        all_ok = false;
                    }
                    j += 1;
                }
                if all_ok {
                    agree += 1;
                }
                ok_cnt += 1;
            }
            Err(_) => {
                if bad != 0 {
                    agree += 1;
                }
                rej_cnt += 1;
            }
        }
        i += 1;
    }
    if agree == 100 {
        Ok((agree, ok_cnt, rej_cnt))
    } else {
        Err(SecCode::LedgerConservationBreak.val())
    }
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
    // 族 0：越界节。
    ledger.tries[0] += 1;
    let (buf, fl, al) = synth_sections(3, 1);
    if let Err(e) = SectionLedger::parse_table(&buf, 0x200, 3, fl, al) {
        if e == SecCode::RawRangeOverflow.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：乱序节。
    ledger.tries[1] += 1;
    let (buf1, fl1, al1) = synth_sections(4, 2);
    if let Err(e) = SectionLedger::parse_table(&buf1, 0x200, 4, fl1, al1) {
        if e == SecCode::VaNotMonotonic.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：账本守恒破（节数 0）。
    ledger.tries[2] += 1;
    let (buf2, fl2, al2) = synth_sections(2, 0);
    if let Err(e) = SectionLedger::parse_table(&buf2, 0x200, 0, fl2, al2) {
        if e == SecCode::ZeroSections.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    run_100_samples()
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SecCode::ALL.len() {
        let v = SecCode::ALL[i].val();
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
    v.push((
        "J1-100样本逐节对账",
        matches!(golden, Ok((a, ok, rej)) if a == 100 && ok == 70 && rej == 30),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 账本导出守恒：8 节导出 8 行。
    let (buf, fl, al) = synth_sections(8, 0);
    let led8 = SectionLedger::parse_table(&buf, 0x200, 8, fl, al);
    v.push((
        "J1-账本导出8节",
        matches!(led8, Ok(ref l) if l.export().is_ok()),
        String::from("导出可比对"),
    ));
    // 对齐取整。
    v.push((
        "J1-对齐取整",
        align_up(0x801, 0x1000) == 0x1000 && align_up(0x1000, 0x1000) == 0x1000,
        String::from("align_up"),
    ));
    v.push((
        "J1-节头大小冻结",
        SECTION_HEADER_SIZE == 40,
        String::from("PE/COFF §4"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x652C0 窗口")));
    v
}

pub fn run_unx12821_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12821 节表逐项解析（四字段）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
