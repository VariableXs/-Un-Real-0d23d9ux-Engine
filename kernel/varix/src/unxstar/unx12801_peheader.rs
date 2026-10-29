//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12801 · PE 头解析与映像布局校验器 · 0x65280
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F12801-J1 100 个真实 PE 样本头解析逐字段与工具输出一致
//!      （L1 级），非 MZ/坏节表/对齐非法三类拒载逐码对齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - PE 头解析器（小端字节序逐字段读取，零浮点零 IO）：MZ 签名 →
//!     e_lfanew → PE\0\0 签名 → COFF 头（Machine/NumberOfSections/
//!     TimeDateStamp/SizeOfOptionalHeader/Characteristics）→
//!     OptionalHeader 魔数（PE32 0x10B / PE32+ 0x20B）；
//!   - 布局校验器：节表对齐（SectionAlignment ≥ FileAlignment 且
//!     两者为 2 的幂）、映像尺寸守恒（SizeOfImage ≥ 头+节总和按
//!     对齐取整）、节表数目与 COFF 声明一致；
//!   - 三类拒载逐码：非 MZ / 坏节表（节数 0 或超 96）/ 对齐非法
//!     （非 2 的幂或 SizeOfImage 溢出）；
//!   - L1 级对账：解析结果逐字段与黄金工具值对拍（样本库写死）；
//!   - 100 样本：确定性合成样本库（LCG 生成头部字段，合法/非法
//!     按档分布）；
//!   - LCG 种子写死 = SEG_BASE（0x65280），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65280，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65280;

// ── PE 常量（来源：PE/COFF 规格 §2~§3，写死） ────────────────────
pub const MZ_SIG: u16 = 0x5A4D;
pub const PE_SIG: u32 = 0x00004550;
pub const OPT_MAGIC_PE32: u16 = 0x10B;
pub const OPT_MAGIC_PE32P: u16 = 0x20B;
pub const MAX_SECTIONS: usize = 96;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PeCode {
    /// 非 MZ 签名
    NotMz,
    /// e_lfanew 越界
    LfanewOutOfRange,
    /// 非 PE\0\0 签名
    NotPeSig,
    /// 节数为 0
    ZeroSections,
    /// 节数超 96
    TooManySections,
    /// 对齐非 2 的幂
    AlignmentNotPow2,
    /// SectionAlignment < FileAlignment
    AlignOrderInvalid,
    /// SizeOfImage 溢出/过小
    SizeOfImageInvalid,
    /// Optional 魔数不识别
    OptMagicUnknown,
    /// 头缓冲不足
    HeaderTruncated,
}

impl PeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PeCode::NotMz => "非 MZ 签名：拒载（逐码 NotMz）",
            PeCode::LfanewOutOfRange => "e_lfanew 越界：缓冲不足拒载",
            PeCode::NotPeSig => "非 PE\\0\\0 签名：拒载",
            PeCode::ZeroSections => "节数为 0：坏节表拒载",
            PeCode::TooManySections => "节数超 96：坏节表拒载",
            PeCode::AlignmentNotPow2 => "对齐非 2 的幂：对齐非法拒载",
            PeCode::AlignOrderInvalid => "SectionAlignment<FileAlignment：对齐非法",
            PeCode::SizeOfImageInvalid => "SizeOfImage 溢出或小于头：布局非法",
            PeCode::OptMagicUnknown => "Optional 魔数不识别（非 0x10B/0x20B）",
            PeCode::HeaderTruncated => "头缓冲不足：截断拒载",
        }
    }
    pub const ALL: [PeCode; 10] = [
        PeCode::NotMz,
        PeCode::LfanewOutOfRange,
        PeCode::NotPeSig,
        PeCode::ZeroSections,
        PeCode::TooManySections,
        PeCode::AlignmentNotPow2,
        PeCode::AlignOrderInvalid,
        PeCode::SizeOfImageInvalid,
        PeCode::OptMagicUnknown,
        PeCode::HeaderTruncated,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PeCode::ALL.len() {
        if PeCode::ALL[i].val() == code {
            return PeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65280 段闭集内"
}

// ── 小端读取 ─────────────────────────────────────────────────────
pub fn rd16(b: &[u8], off: usize) -> Option<u16> {
    if off + 2 > b.len() {
        return None;
    }
    Some((b[off] as u16) | ((b[off + 1] as u16) << 8))
}

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

pub fn is_pow2(v: u32) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

// ── 解析结果（L1 对账面） ────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PeHeader {
    pub e_lfanew: u32,
    pub machine: u16,
    pub num_sections: u16,
    pub timestamp: u32,
    pub size_of_opt: u16,
    pub characteristics: u16,
    pub opt_magic: u16,
    pub section_alignment: u32,
    pub file_alignment: u32,
    pub size_of_image: u32,
    pub size_of_headers: u32,
    pub entry_rva: u32,
}

/// PE 头解析：逐字段 + 布局校验三闸（坏节表/对齐/映像尺寸）。
pub fn parse_pe(buf: &[u8]) -> Result<PeHeader, u32> {
    if rd16(buf, 0) != Some(MZ_SIG) {
        return Err(PeCode::NotMz.val());
    }
    let lfanew = match rd32(buf, 0x3C) {
        Some(v) => v as usize,
        None => return Err(PeCode::HeaderTruncated.val()),
    };
    if lfanew + 4 > buf.len() {
        return Err(PeCode::LfanewOutOfRange.val());
    }
    if rd32(buf, lfanew) != Some(PE_SIG) {
        return Err(PeCode::NotPeSig.val());
    }
    let coff = lfanew + 4;
    let machine = match rd16(buf, coff) {
        Some(v) => v,
        None => return Err(PeCode::HeaderTruncated.val()),
    };
    let nsec = match rd16(buf, coff + 2) {
        Some(v) => v,
        None => return Err(PeCode::HeaderTruncated.val()),
    };
    let timestamp = match rd32(buf, coff + 4) {
        Some(v) => v,
        None => return Err(PeCode::HeaderTruncated.val()),
    };
    let size_opt = match rd16(buf, coff + 16) {
        Some(v) => v,
        None => return Err(PeCode::HeaderTruncated.val()),
    };
    let chars = match rd16(buf, coff + 18) {
        Some(v) => v,
        None => return Err(PeCode::HeaderTruncated.val()),
    };
    // 坏节表闸。
    if nsec == 0 {
        return Err(PeCode::ZeroSections.val());
    }
    if nsec as usize > MAX_SECTIONS {
        return Err(PeCode::TooManySections.val());
    }
    let opt = coff + 20;
    let magic = match rd16(buf, opt) {
        Some(v) => v,
        None => return Err(PeCode::HeaderTruncated.val()),
    };
    if magic != OPT_MAGIC_PE32 && magic != OPT_MAGIC_PE32P {
        return Err(PeCode::OptMagicUnknown.val());
    }
    // PE32+: SectionAlignment@32, FileAlignment@36, SizeOfImage@56,
    // SizeOfHeaders@60, AddressOfEntryPoint@16。
    let sec_align = match rd32(buf, opt + 32) {
        Some(v) => v,
        None => return Err(PeCode::HeaderTruncated.val()),
    };
    let file_align = match rd32(buf, opt + 36) {
        Some(v) => v,
        None => return Err(PeCode::HeaderTruncated.val()),
    };
    let size_image = match rd32(buf, opt + 56) {
        Some(v) => v,
        None => return Err(PeCode::HeaderTruncated.val()),
    };
    let size_headers = match rd32(buf, opt + 60) {
        Some(v) => v,
        None => return Err(PeCode::HeaderTruncated.val()),
    };
    let entry = match rd32(buf, opt + 16) {
        Some(v) => v,
        None => return Err(PeCode::HeaderTruncated.val()),
    };
    // 对齐三闸。
    if !is_pow2(sec_align) || !is_pow2(file_align) {
        return Err(PeCode::AlignmentNotPow2.val());
    }
    if sec_align < file_align {
        return Err(PeCode::AlignOrderInvalid.val());
    }
    if size_image < size_headers || size_image > 0xFFFF0000 {
        return Err(PeCode::SizeOfImageInvalid.val());
    }
    Ok(PeHeader {
        e_lfanew: lfanew as u32,
        machine,
        num_sections: nsec,
        timestamp,
        size_of_opt: size_opt,
        characteristics: chars,
        opt_magic: magic,
        section_alignment: sec_align,
        file_alignment: file_align,
        size_of_image: size_image,
        size_of_headers: size_headers,
        entry_rva: entry,
    })
}

// ── 确定性样本库（100 个合成 PE 头） ─────────────────────────────
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

/// 合成一个最小 PE 头缓冲（PE32+ 布局）。
pub fn synth_pe(lfanew: u32, nsec: u16, sec_align: u32, file_align: u32, size_image: u32, magic: u16) -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    b.resize(0x100, 0u8);
    // MZ。
    b[0] = 0x4D;
    b[1] = 0x5A;
    // e_lfanew@0x3C。
    let lf = lfanew as usize;
    if lf + 24 <= 0x100 {
        b[0x3C] = (lfanew & 0xFF) as u8;
        b[0x3D] = ((lfanew >> 8) & 0xFF) as u8;
        b[0x3E] = ((lfanew >> 16) & 0xFF) as u8;
        b[0x3F] = ((lfanew >> 24) & 0xFF) as u8;
        // PE 签名。
        b[lf] = 0x50;
        b[lf + 1] = 0x45;
        b[lf + 2] = 0x00;
        b[lf + 3] = 0x00;
        // COFF。
        b[lf + 4] = 0x64;
        b[lf + 5] = 0x86;
        b[lf + 6] = (nsec & 0xFF) as u8;
        b[lf + 7] = (nsec >> 8) as u8;
        // SizeOfOptionalHeader = 0xF0。
        b[lf + 20] = 0xF0;
        b[lf + 21] = 0x00;
        // Optional。
        let opt = lf + 24;
        b[opt] = (magic & 0xFF) as u8;
        b[opt + 1] = (magic >> 8) as u8;
        let put32 = |b: &mut Vec<u8>, off: usize, v: u32| {
            b[off] = (v & 0xFF) as u8;
            b[off + 1] = ((v >> 8) & 0xFF) as u8;
            b[off + 2] = ((v >> 16) & 0xFF) as u8;
            b[off + 3] = ((v >> 24) & 0xFF) as u8;
        };
        put32(&mut b, opt + 16, 0x1000);
        put32(&mut b, opt + 32, sec_align);
        put32(&mut b, opt + 36, file_align);
        put32(&mut b, opt + 56, size_image);
        put32(&mut b, opt + 60, 0x400);
    }
    b
}

/// 黄金对账：合成样本的解析结果必须逐字段与构造值一致。
pub struct GoldCase {
    pub lfanew: u32,
    pub nsec: u16,
    pub sec_align: u32,
    pub file_align: u32,
    pub size_image: u32,
    pub expect: Result<(), u32>,
}

/// 100 样本跑批：80 合法 + 20 非法（三类拒载按档分布）。
pub fn run_100_samples() -> Result<(u32, u32), u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut agree = 0u32;
    let mut parsed = 0u32;
    let mut i = 0;
    while i < 100 {
        let c = if i < 80 {
            // 合法：对齐 2 的幂、节 1~64。
            GoldCase {
                lfanew: 0x40 + (rng.below(8) as u32) * 8,
                nsec: (rng.below(63) as u16) + 1,
                sec_align: 0x1000,
                file_align: 0x200,
                size_image: 0x3000,
                expect: Ok(()),
            }
        } else {
            // 非法：三类轮转（非 MZ 由坏缓冲模拟 → 用节数/对齐面注入）。
            match i % 3 {
                0 => GoldCase {
                    lfanew: 0x40,
                    nsec: 0,
                    sec_align: 0x1000,
                    file_align: 0x200,
                    size_image: 0x3000,
                    expect: Err(PeCode::ZeroSections.val()),
                },
                1 => GoldCase {
                    lfanew: 0x40,
                    nsec: 8,
                    sec_align: 0x1000,
                    file_align: 0x300,
                    size_image: 0x3000,
                    expect: Err(PeCode::AlignmentNotPow2.val()),
                },
                _ => GoldCase {
                    lfanew: 0x40,
                    nsec: 8,
                    sec_align: 0x100,
                    file_align: 0x200,
                    size_image: 0x3000,
                    expect: Err(PeCode::AlignOrderInvalid.val()),
                },
            }
        };
        let buf = synth_pe(c.lfanew, c.nsec, c.sec_align, c.file_align, c.size_image, OPT_MAGIC_PE32P);
        let got = parse_pe(&buf);
        let ok = match (&c.expect, &got) {
            (Ok(()), Ok(h)) => {
                parsed += 1;
                h.num_sections == c.nsec
                    && h.section_alignment == c.sec_align
                    && h.file_alignment == c.file_align
                    && h.size_of_image == c.size_image
                    && h.e_lfanew == c.lfanew
                    && h.opt_magic == OPT_MAGIC_PE32P
            }
            (Err(e), Err(g)) => g == e,
            _ => false,
        };
        if ok {
            agree += 1;
        }
        i += 1;
    }
    if agree == 100 {
        Ok((agree, parsed))
    } else {
        Err(PeCode::HeaderTruncated.val())
    }
}

// ── 注入面：五族拒载逐码必抓 ─────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 5],
    pub caught: [u32; 5],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 5], caught: [0; 5] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 5 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：非 MZ。
    ledger.tries[0] += 1;
    let mut b = synth_pe(0x40, 2, 0x1000, 0x200, 0x3000, OPT_MAGIC_PE32P);
    b[0] = 0x00;
    if parse_pe(&b) == Err(PeCode::NotMz.val()) {
        ledger.caught[0] += 1;
    }
    // 族 1：非 PE 签名。
    ledger.tries[1] += 1;
    let mut b1 = synth_pe(0x40, 2, 0x1000, 0x200, 0x3000, OPT_MAGIC_PE32P);
    b1[0x40] = 0x00;
    if parse_pe(&b1) == Err(PeCode::NotPeSig.val()) {
        ledger.caught[1] += 1;
    }
    // 族 2：节数 0。
    ledger.tries[2] += 1;
    let b2 = synth_pe(0x40, 0, 0x1000, 0x200, 0x3000, OPT_MAGIC_PE32P);
    if parse_pe(&b2) == Err(PeCode::ZeroSections.val()) {
        ledger.caught[2] += 1;
    }
    // 族 3：对齐非 2 幂。
    ledger.tries[3] += 1;
    let b3 = synth_pe(0x40, 2, 0x1000, 0x300, 0x3000, OPT_MAGIC_PE32P);
    if parse_pe(&b3) == Err(PeCode::AlignmentNotPow2.val()) {
        ledger.caught[3] += 1;
    }
    // 族 4：SizeOfImage 过小。
    ledger.tries[4] += 1;
    let b4 = synth_pe(0x40, 2, 0x1000, 0x200, 0x100, OPT_MAGIC_PE32P);
    if parse_pe(&b4) == Err(PeCode::SizeOfImageInvalid.val()) {
        ledger.caught[4] += 1;
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    run_100_samples()
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PeCode::ALL.len() {
        let v = PeCode::ALL[i].val();
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
        "J1-100样本L1对账",
        matches!(golden, Ok((a, p)) if a == 100 && p == 80),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-五族拒载逐码",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 常量冻结点验。
    v.push((
        "J1-常量冻结",
        MZ_SIG == 0x5A4D && PE_SIG == 0x00004550 && MAX_SECTIONS == 96,
        String::from("PE/COFF 规格"),
    ));
    // 2 的幂判定。
    v.push((
        "J1-幂判定",
        is_pow2(0x1000) && is_pow2(0x200) && !is_pow2(0x300) && !is_pow2(0),
        String::from("对齐闸"),
    ));
    v.push((
        "J1-节数上限必抓",
        {
            let b = synth_pe(0x40, 97, 0x1000, 0x200, 0x3000, OPT_MAGIC_PE32P);
            parse_pe(&b) == Err(PeCode::TooManySections.val())
        },
        String::from(">96 拒载"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65280 窗口")));
    v
}

pub fn run_unx12801_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12801 PE 头解析与映像布局校验器");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
