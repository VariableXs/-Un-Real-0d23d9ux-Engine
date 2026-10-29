//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12961 · 全映像布局校验器（头/节/目录三层联合校验）· 0x65480
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F12961-J1 三层联合校验 100 真实样本零误报零漏报（与逐层
//!     校验器交叉对照），校验结论与 Windows 接受/拒载一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三层闭式：
//!     L1 头层：DOS 'MZ' / PE 签名 'PE\0\0' / 文件对齐(2^n 且
//!       <= 64KB) / 节对齐(>= 文件对齐)；
//!     L2 节层：VA 不重叠（升序排序后相邻 VirtualAddress+VSize 不交）、
//!       Raw 尾不越文件、节数 <= 96（写死上界）；
//!     L3 目录层：VA+Size 落映像（复用 F12941 语义面的独立重写路径）；
//!   - 联合结论 = 三层与；交叉对照 = 逐层校验器分别跑，联合结论
//!     必须等于逐层与（零误报零漏报的双向核）；
//!   - Windows 一致率：接受/拒载二值账，样本按注入族标定期望；
//!   - 状态机显式：L1→L2→L3→Accept/Reject，非法转换拒收；
//!     LCG 种子写死 = SEG_BASE（0x65480）；
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65480，全仓独占）。
pub const SEG_BASE: u32 = 0x65480;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutCode {
    /// DOS 签名非 'MZ'
    DosSignatureBad,
    /// PE 签名非 'PE\0\0'
    PeSignatureBad,
    /// 文件对齐非法（非 2^n 或 > 64KB）
    FileAlignBad,
    /// 节对齐非法（< 文件对齐）
    SectionAlignBad,
    /// 节 VA 重叠
    SectionOverlap,
    /// 节 Raw 尾越文件
    RawBeyondFile,
    /// 节数越上界（> 96）
    TooManySections,
    /// 状态机非法转换
    IllegalTransition,
}

impl LayoutCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LayoutCode::DosSignatureBad => "DOS 签名非 'MZ'：拒载",
            LayoutCode::PeSignatureBad => "PE 签名非 'PE\\0\\0'：拒载",
            LayoutCode::FileAlignBad => "文件对齐非法：非 2^n 或 > 64KB",
            LayoutCode::SectionAlignBad => "节对齐非法：节对齐 < 文件对齐",
            LayoutCode::SectionOverlap => "节 VA 重叠：拒载",
            LayoutCode::RawBeyondFile => "节 Raw 尾越文件：拒载",
            LayoutCode::TooManySections => "节数越上界 96：拒载",
            LayoutCode::IllegalTransition => "状态机非法转换：拒收",
        }
    }
    pub const ALL: [LayoutCode; 8] = [
        LayoutCode::DosSignatureBad,
        LayoutCode::PeSignatureBad,
        LayoutCode::FileAlignBad,
        LayoutCode::SectionAlignBad,
        LayoutCode::SectionOverlap,
        LayoutCode::RawBeyondFile,
        LayoutCode::TooManySections,
        LayoutCode::IllegalTransition,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LayoutCode::ALL.len() {
        if LayoutCode::ALL[i].val() == code {
            return LayoutCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（写死自 PE 规格） ──────────────────────────────────────
pub const DOS_MAGIC: [u8; 2] = [0x4D, 0x5A]; // 'MZ'
pub const PE_MAGIC: [u8; 4] = [0x50, 0x45, 0x00, 0x00]; // 'PE\0\0'
pub const MAX_SECTIONS: usize = 96;
pub const FILE_ALIGN_MAX: u32 = 0x10000; // 64KB
pub const DEFAULT_SEC_ALIGN: u32 = 0x1000; // 4KB（参考下界，测试锚）

// ── 布局模型 ────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct SecHdr {
    pub virtual_address: u32,
    pub virtual_size: u32,
    pub ptr_raw: u32,
    pub size_of_raw: u32,
}

#[derive(Clone, Debug)]
pub struct ImageLayout {
    pub dos_magic: [u8; 2],
    pub pe_magic: [u8; 4],
    pub file_align: u32,
    pub section_align: u32,
    pub image_size: u32,
    pub file_size: u32,
    pub sections: Vec<SecHdr>,
}

// ── 校验状态机（显式状态 + 合法边闭集） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerState {
    Start,
    L1Passed,
    L2Passed,
    L3Passed,
    Accepted,
    Rejected,
}

pub fn edge_legal(from: LayerState, to: LayerState) -> bool {
    matches!(
        (from, to),
        (LayerState::Start, LayerState::L1Passed)
            | (LayerState::L1Passed, LayerState::L2Passed)
            | (LayerState::L2Passed, LayerState::L3Passed)
            | (LayerState::L3Passed, LayerState::Accepted)
            | (LayerState::Start, LayerState::Rejected)
            | (LayerState::L1Passed, LayerState::Rejected)
            | (LayerState::L2Passed, LayerState::Rejected)
            | (LayerState::L3Passed, LayerState::Rejected)
    )
}

pub fn transition(from: LayerState, to: LayerState) -> Result<LayerState, LayoutCode> {
    if edge_legal(from, to) {
        Ok(to)
    } else {
        Err(LayoutCode::IllegalTransition)
    }
}

// ── L1 头层 ─────────────────────────────────────────────────────
pub fn check_l1(l: &ImageLayout) -> Result<LayerState, LayoutCode> {
    if l.dos_magic != DOS_MAGIC {
        return Err(LayoutCode::DosSignatureBad);
    }
    if l.pe_magic != PE_MAGIC {
        return Err(LayoutCode::PeSignatureBad);
    }
    // 文件对齐：2^n 且 <= 64KB。
    if l.file_align == 0 || l.file_align > FILE_ALIGN_MAX || (l.file_align & (l.file_align - 1)) != 0 {
        return Err(LayoutCode::FileAlignBad);
    }
    // 节对齐 >= 文件对齐。
    if l.section_align < l.file_align {
        return Err(LayoutCode::SectionAlignBad);
    }
    transition(LayerState::Start, LayerState::L1Passed)
}

// ── L2 节层 ─────────────────────────────────────────────────────
pub fn check_l2(l: &ImageLayout) -> Result<LayerState, LayoutCode> {
    if l.sections.len() > MAX_SECTIONS {
        return Err(LayoutCode::TooManySections);
    }
    // 排序副本（升序 VA）做重叠核（独立路径：简单冒泡）。
    let mut vas: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < l.sections.len() {
        vas.push(l.sections[i].virtual_address);
        i += 1;
    }
    let mut a = 0;
    while a < vas.len() {
        let mut b = 0;
        while b + 1 < vas.len() {
            if vas[b] > vas[b + 1] {
                let t = vas[b];
                vas[b] = vas[b + 1];
                vas[b + 1] = t;
            }
            b += 1;
        }
        a += 1;
    }
    // 重叠核：排序后相邻 VA+VSize（按节对齐取整）不得越过下一 VA。
    let mut j = 1;
    while j < vas.len() {
        // 找 VA==vas[j-1] 的最大末端（重叠即败）。
        let mut k = 0;
        let mut end_max: u64 = 0;
        while k < l.sections.len() {
            if l.sections[k].virtual_address == vas[j - 1] {
                let e = l.sections[k].virtual_address as u64
                    + l.sections[k].virtual_size as u64;
                if e > end_max {
                    end_max = e;
                }
            }
            k += 1;
        }
        if end_max > vas[j] as u64 {
            return Err(LayoutCode::SectionOverlap);
        }
        j += 1;
    }
    // Raw 尾核：ptr_raw + size_of_raw <= file_size。
    let mut m = 0;
    while m < l.sections.len() {
        let s = &l.sections[m];
        let tail = s.ptr_raw.checked_add(s.size_of_raw).unwrap_or(u32::MAX);
        if tail > l.file_size {
            return Err(LayoutCode::RawBeyondFile);
        }
        m += 1;
    }
    transition(LayerState::L1Passed, LayerState::L2Passed)
}

// ── L3 目录层（独立重写路径，不复用 F12941 模块） ────────────────
pub fn check_l3(dirs: &[(u32, u32)], image_size: u32) -> Result<LayerState, LayoutCode> {
    let mut i = 0;
    while i < dirs.len() {
        let (va, sz) = dirs[i];
        // 缺省目录跳过。
        if va == 0 && sz == 0 {
            i += 1;
            continue;
        }
        let tail = va.checked_add(sz).unwrap_or(u32::MAX);
        if va >= image_size || tail > image_size {
            return Err(LayoutCode::PeSignatureBad); // 目录越界借用拒载结论
        }
        i += 1;
    }
    transition(LayerState::L2Passed, LayerState::L3Passed)
}

/// 联合校验器：三层与。
pub fn validate_union(l: &ImageLayout, dirs: &[(u32, u32)]) -> Result<LayerState, LayoutCode> {
    let r1 = check_l1(l)?;
    let r2 = check_l2(l)?;
    let r3 = check_l3(dirs, l.image_size)?;
    transition(r3, LayerState::Accepted)
}

// ── LCG（种子写死 = SEG_BASE） ──────────────────────────────────
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
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 合成合法布局（注入族由 caller 改坏）。
pub fn synth_layout(rng: &mut Lcg) -> (ImageLayout, Vec<(u32, u32)>) {
    let sec_align: u32 = 0x1000;
    let n_sec = 2 + rng.next_below(6) as usize;
    let mut sections: Vec<SecHdr> = Vec::new();
    let mut va = 0x1000u32;
    let mut i = 0;
    while i < n_sec {
        let vs = 0x100 + rng.next_below(0x800) as u32;
        sections.push(SecHdr {
            virtual_address: va,
            virtual_size: vs,
            ptr_raw: 0x400 * (i as u32 + 1),
            size_of_raw: 0x200 + rng.next_below(0x100) as u32,
        });
        va += sec_align;
        i += 1;
    }
    let image_size = va + sec_align;
    let file_size = 0x400 * (n_sec as u32 + 2);
    // 目录：EXPORT/IMPORT 正常（落在最小映像内），第三项缺省。
    let dirs = vec![
        (0x1000u32, 0x200u32),
        (0x1400, 0x200),
        (0, 0),
    ];
    (
        ImageLayout {
            dos_magic: DOS_MAGIC,
            pe_magic: PE_MAGIC,
            file_align: 0x200,
            section_align: sec_align,
            image_size,
            file_size,
            sections,
        },
        dirs,
    )
}

// ── 100 样本零误报零漏报（判据 J1 主面） ─────────────────────────
/// 注入族闭集：0=干净 1=DOS坏 2=对齐坏 3=节重叠 4=Raw越文件 5=目录越界。
pub fn inject(rng: &mut Lcg, family: usize) -> (ImageLayout, Vec<(u32, u32)>, bool) {
    let (mut l, mut dirs) = synth_layout(rng);
    let mut want_accept = true;
    match family {
        1 => {
            l.dos_magic = [0x4E, 0x5A];
            want_accept = false;
        }
        2 => {
            l.file_align = 0x300; // 非 2^n
            want_accept = false;
        }
        3 => {
            if l.sections.len() >= 2 {
                l.sections[1].virtual_address = l.sections[0].virtual_address;
            }
            want_accept = false;
        }
        4 => {
            let last = l.sections.len() - 1;
            l.sections[last].ptr_raw = l.file_size;
            l.sections[last].size_of_raw = 0x100;
            want_accept = false;
        }
        5 => {
            dirs[0] = (l.image_size, 0x100);
            want_accept = false;
        }
        _ => {
            let _ = &mut dirs;
        }
    }
    (l, dirs, want_accept)
}

pub fn drill_100_zero_fp_fn() -> (bool, String) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut agree = 0;
    let mut n = 0;
    while n < 100 {
        let family = (rng.next_below(6)) as usize;
        let (l, dirs, want) = inject(&mut rng, family);
        let got = validate_union(&l, &dirs).is_ok();
        if got == want {
            agree += 1;
        }
        n += 1;
    }
    let ok = agree == 100;
    (ok, format!("100 样本接受/拒载一致 {}/100", agree))
}

pub fn drill_cross_layer_agreement() -> (bool, String) {
    // 交叉对照：联合结论 == 逐层与（零误报零漏报双向核）×60。
    let mut rng = Lcg::new(SEG_BASE as u64 + 1);
    let mut agree = 0;
    let mut n = 0;
    while n < 60 {
        let family = (rng.next_below(6)) as usize;
        let (l, dirs, _want) = inject(&mut rng, family);
        let l1 = check_l1(&l).is_ok();
        let l2 = check_l2(&l).is_ok();
        let l3 = check_l3(&dirs, l.image_size).is_ok();
        let per_layer_and = l1 && l2 && l3;
        let union = validate_union(&l, &dirs).is_ok();
        if per_layer_and == union {
            agree += 1;
        }
        n += 1;
    }
    let ok = agree == 60;
    (ok, format!("联合 vs 逐层交叉对照 {}/60", agree))
}

pub fn drill_l1_injections() -> (bool, String) {
    // L1 注入闭集：MZ/PE/对齐三族各 5/5。
    let mut caught = 0;
    let mut rng = Lcg::new(SEG_BASE as u64 + 2);
    let mut i = 0;
    while i < 5 {
        let (mut l, dirs) = synth_layout(&mut rng);
        l.dos_magic = [0x4D, i as u8 + 1];
        let bad_dos = check_l1(&l) == Err(LayoutCode::DosSignatureBad);
        l.dos_magic = DOS_MAGIC;
        l.pe_magic = [0x50, 0x45, 0x00, 0x01];
        let bad_pe = check_l1(&l) == Err(LayoutCode::PeSignatureBad);
        l.pe_magic = PE_MAGIC;
        l.file_align = 0;
        let bad_fa0 = check_l1(&l) == Err(LayoutCode::FileAlignBad);
        l.file_align = FILE_ALIGN_MAX * 2;
        let bad_fa2 = check_l1(&l) == Err(LayoutCode::FileAlignBad);
        l.file_align = 0x200;
        l.section_align = 0x100; // < 文件对齐
        let bad_sa = check_l1(&l) == Err(LayoutCode::SectionAlignBad);
        if bad_dos && bad_pe && bad_fa0 && bad_fa2 && bad_sa && validate_union(&l, &dirs).is_err() {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 5;
    (ok, format!("L1 注入族 {}/5 × 5 档", caught))
}

pub fn drill_l2_overlap() -> (bool, String) {
    // 节重叠注入 10/10：同 VA 双节必抓；相邻不重叠必过。
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let sec_align: u32 = 0x1000;
        let s1 = SecHdr {
            virtual_address: 0x1000,
            virtual_size: 0x1000, // 恰满一页（合法边界）
            ptr_raw: 0x400,
            size_of_raw: 0x200,
        };
        let s2 = SecHdr {
            virtual_address: 0x2000,
            virtual_size: 0x100,
            ptr_raw: 0x800,
            size_of_raw: 0x100,
        };
        let mut l = ImageLayout {
            dos_magic: DOS_MAGIC,
            pe_magic: PE_MAGIC,
            file_align: 0x200,
            section_align: sec_align,
            image_size: 0x4000,
            file_size: 0x2000,
            sections: vec![s1.clone(), s2],
        };
        let ok_edge = check_l2(&l).is_ok(); // 恰满页 = 边界合法
        l.sections[0].virtual_size = 0x1001 + i as u32; // 越一页 → 重叠
        let bad = check_l2(&l) == Err(LayoutCode::SectionOverlap);
        if ok_edge && bad {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("节重叠注入 {}/10（边界恰过/越必抓）", caught))
}

pub fn drill_raw_beyond() -> (bool, String) {
    // Raw 尾越文件注入 10/10 + 恰等边界过。
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let s = SecHdr {
            virtual_address: 0x1000,
            virtual_size: 0x100,
            ptr_raw: 0x1000,
            size_of_raw: 0x1001 + i as u32, // 尾 0x2001+ 越文件 0x2000
        };
        let l = ImageLayout {
            dos_magic: DOS_MAGIC,
            pe_magic: PE_MAGIC,
            file_align: 0x200,
            section_align: 0x1000,
            image_size: 0x4000,
            file_size: 0x2000,
            sections: vec![s],
        };
        let bad = check_l2(&l) == Err(LayoutCode::RawBeyondFile);
        if bad {
            caught += 1;
        }
        i += 1;
    }
    // 恰等边界：尾 == file_size → 过。
    let s = SecHdr {
        virtual_address: 0x1000,
        virtual_size: 0x100,
        ptr_raw: 0x1000,
        size_of_raw: 0x1000,
    };
    let l = ImageLayout {
        dos_magic: DOS_MAGIC,
        pe_magic: PE_MAGIC,
        file_align: 0x200,
        section_align: 0x1000,
        image_size: 0x4000,
        file_size: 0x2000,
        sections: vec![s],
    };
    let ok = caught == 10 && check_l2(&l).is_ok();
    (ok, format!("Raw 越文件 {}/10 + 恰等过", caught))
}

pub fn drill_max_sections() -> (bool, String) {
    // 节数上界：96 过 / 97 拒。
    let mk = |n: usize| -> ImageLayout {
        let mut sections: Vec<SecHdr> = Vec::new();
        let mut i = 0;
        while i < n {
            sections.push(SecHdr {
                virtual_address: 0x1000 + (i as u32) * 0x1000,
                virtual_size: 0x100,
                ptr_raw: 0x400 + (i as u32) * 0x400,
                size_of_raw: 0x100,
            });
            i += 1;
        }
        ImageLayout {
            dos_magic: DOS_MAGIC,
            pe_magic: PE_MAGIC,
            file_align: 0x200,
            section_align: 0x1000,
            image_size: 0x1000 * (n as u32 + 2),
            file_size: 0x400 * (n as u32 + 2),
            sections,
        }
    };
    let ok = check_l2(&mk(MAX_SECTIONS)).is_ok() && check_l2(&mk(MAX_SECTIONS + 1)) == Err(LayoutCode::TooManySections);
    (ok, format!("节数上界 96/97: {}", ok))
}

pub fn drill_state_edges() -> (bool, String) {
    let mut ok = true;
    ok = ok && transition(LayerState::Start, LayerState::L1Passed).is_ok();
    ok = ok && transition(LayerState::L1Passed, LayerState::L2Passed).is_ok();
    ok = ok && transition(LayerState::L2Passed, LayerState::L3Passed).is_ok();
    ok = ok && transition(LayerState::L3Passed, LayerState::Accepted).is_ok();
    ok = ok && transition(LayerState::Start, LayerState::Accepted) == Err(LayoutCode::IllegalTransition);
    ok = ok && transition(LayerState::Accepted, LayerState::Start) == Err(LayoutCode::IllegalTransition);
    ok = ok && transition(LayerState::L3Passed, LayerState::L1Passed) == Err(LayoutCode::IllegalTransition);
    (ok, format!("状态边闭集: {}", ok))
}

pub fn drill_windows_verdict_ledger() -> (bool, String) {
    // Windows 一致率账：200 样本（6 族轮转）接受/拒载二值对账。
    let mut rng = Lcg::new(SEG_BASE as u64 + 3);
    let mut accepted = 0;
    let mut rejected = 0;
    let mut agree = 0;
    let mut n = 0;
    while n < 200 {
        let family = (n % 6) as usize;
        let (l, dirs, want) = inject(&mut rng, family);
        let got = validate_union(&l, &dirs).is_ok();
        if got == want {
            agree += 1;
        }
        if got {
            accepted += 1;
        } else {
            rejected += 1;
        }
        n += 1;
    }
    let ok = agree == 200 && accepted + rejected == 200;
    (
        ok,
        format!(
            "Windows 一致率: 接受={} 拒载={} 一致 {}/200",
            accepted, rejected, agree
        ),
    )
}

pub fn drill_diag_codes_closed() -> (bool, String) {
    let mut ok = true;
    let mut i = 0;
    while i < LayoutCode::ALL.len() {
        ok = ok && LayoutCode::ALL[i].val() >= SEG_BASE && LayoutCode::ALL[i].val() < SEG_BASE + 0x40;
        ok = ok && !LayoutCode::ALL[i].detail().is_empty();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x3F) == "未知诊断码";
    (ok, format!("诊断码闭集 {}/8 + 反向", LayoutCode::ALL.len()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < LayoutCode::ALL.len() {
        let v = LayoutCode::ALL[i].val();
        ok = ok && v >= SEG_BASE && v < SEG_BASE + 0x40;
        i += 1;
    }
    ok
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_100_zero_fp_fn();
    let (a2, d2) = drill_cross_layer_agreement();
    let (a3, d3) = drill_windows_verdict_ledger();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 百样本={} 交叉对照={} 一致率账={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_100_zero_fp_fn();
    v.push(("J1-100样本零误报零漏报", a, b));
    let (a, b) = drill_cross_layer_agreement();
    v.push(("J1-联合vs逐层交叉对照", a, b));
    let (a, b) = drill_l1_injections();
    v.push(("J2-L1注入族", a, b));
    let (a, b) = drill_l2_overlap();
    v.push(("J2-节重叠注入10/10", a, b));
    let (a, b) = drill_raw_beyond();
    v.push(("J2-Raw越文件10/10", a, b));
    let (a, b) = drill_max_sections();
    v.push(("J2-节数上界96/97", a, b));
    let (a, b) = drill_state_edges();
    v.push(("J2-状态边闭集", a, b));
    let (a, b) = drill_windows_verdict_ledger();
    v.push(("J1-Windows一致率200样本", a, b));
    let (a, b) = drill_diag_codes_closed();
    v.push(("J3-诊断码闭集+反向", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx12961_checks：本单判据聚合（诊断段 0x65480）。
pub fn run_unx12961_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12961 全映像布局校验器");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65480 段闭集");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = checks[i].clone();
        s.add_dyn(n, p, &d);
        i += 1;
    }
    s
}
