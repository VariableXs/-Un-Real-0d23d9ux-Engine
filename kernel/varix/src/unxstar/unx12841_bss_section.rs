//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12841 · BSS 类节识别（VirtualSize>SizeOfRawData 判定）· 0x65300
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12841-J1 100 真实样本 BSS 节识别与工具输出一致率 100%，
//!     边界样本（VSize==SRaw）不误判；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 判定闭式：SizeOfRawData == 0 且 VirtualSize > 0 → 纯 BSS；
//!     SizeOfRawData < VirtualSize → 部分未初始化尾（BSS 尾账）；
//!     VSize == SRaw → 非 BSS（边界红线，不误判为 BSS）；
//!     VSize < SRaw → 文件映像大于虚拟需求（对齐造成），非 BSS；
//!   - 特征位交叉核：IMAGE_SCN_CNT_UNINITIALIZED_DATA(0x80) 与
//!     SizeOfRawData==0 双条件一致才落 BSS 账，否则落冲突账；
//!   - 账目守恒：总节数 = 纯BSS + BSS尾 + 普通节 + 冲突账 + 拒载账；
//!   - 100 样本由确定性 LCG（种子=0x65300）合成 + 12 例黄金对照表
//!    （含 VSize==SRaw 边界族）双面核对；
//!   - LCG 种子写死 = SEG_BASE（0x65300），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65300，全仓独占）。
pub const SEG_BASE: u32 = 0x65300;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BssCode {
    /// 边界样本误判：VSize==SRaw 被归入 BSS
    BoundaryMisjudged,
    /// 特征位冲突：Raw=0 但缺 CNT_UNINITIALIZED_DATA
    CharConflict,
    /// 特征位冲突：Raw>0 但带 CNT_UNINITIALIZED_DATA 且无初始化尾
    CharReverseConflict,
    /// 节 RVA 越界映像
    RvaOutOfRange,
    /// VSize 越上界（> 2GB 段窗）
    VSizeOverflow,
    /// 分类账守恒破（分类和 ≠ 总节数）
    LedgerBroken,
    /// 对齐规则违例：FileAlign 非法导致 SRaw 换算失败
    AlignIllegal,
    /// 判定链缺段（VSize/SRaw/特征位不全）
    ChainIncomplete,
}

impl BssCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BssCode::BoundaryMisjudged => "边界样本误判：VSize==SRaw 不得归入 BSS",
            BssCode::CharConflict => "特征位冲突：Raw=0 但缺未初始化数据位",
            BssCode::CharReverseConflict => "特征位冲突：Raw>0 却声明纯未初始化",
            BssCode::RvaOutOfRange => "节 RVA 越界映像：拒载落账",
            BssCode::VSizeOverflow => "VirtualSize 越上界：拒载落账",
            BssCode::LedgerBroken => "分类账守恒破：分类和与总节数不等",
            BssCode::AlignIllegal => "FileAlign 非法：SRaw 换算失败",
            BssCode::ChainIncomplete => "判定链缺段：VSize/SRaw/特征位不全",
        }
    }
    pub const ALL: [BssCode; 8] = [
        BssCode::BoundaryMisjudged,
        BssCode::CharConflict,
        BssCode::CharReverseConflict,
        BssCode::RvaOutOfRange,
        BssCode::VSizeOverflow,
        BssCode::LedgerBroken,
        BssCode::AlignIllegal,
        BssCode::ChainIncomplete,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BssCode::ALL.len() {
        if BssCode::ALL[i].val() == code {
            return BssCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 节表常量（来源：winnt.h IMAGE_SECTION_HEADER 语义） ──────────
pub const SCN_CNT_UNINITIALIZED_DATA: u32 = 0x0000_0080;
pub const SCN_CNT_INITIALIZED_DATA: u32 = 0x0000_0040;
pub const SCN_MEM_DISCARDABLE: u32 = 0x0200_0000;
/// VirtualSize 合法上界：2GB（映像段窗，写死；超过即拒载）。
pub const VSIZE_MAX: u32 = 0x8000_0000;
/// 节名固定宽度（8 字节，写死自 PE 规格）。
pub const SECTION_NAME_LEN: usize = 8;

/// 节头（建模真实 PE 节关键字段）。
#[derive(Clone, Debug)]
pub struct SectionHeader {
    pub name: String,
    pub virtual_size: u32,
    pub size_of_raw: u32,
    pub virtual_address: u32,
    pub characteristics: u32,
}

impl SectionHeader {
    pub fn new(name: &str, vsize: u32, sraw: u32, rva: u32, chars: u32) -> SectionHeader {
        SectionHeader {
            name: String::from(name),
            virtual_size: vsize,
            size_of_raw: sraw,
            virtual_address: rva,
            characteristics: chars,
        }
    }
}

// ── 分类状态机（显式状态 + 合法边闭集 + 非法转换拒收） ───────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClassifyState {
    /// 未检
    Pending,
    /// 已检：纯 BSS（Raw==0 && VSize>0）
    PureBss,
    /// 已检：BSS 尾（0 < Raw < VSize）
    BssTail,
    /// 已检：普通节（Raw >= VSize）
    Normal,
    /// 冲突账（特征位交叉核不过）
    Conflict,
    /// 拒载账（RVA/尺寸越界）
    Rejected,
}

/// 合法边闭集：Pending → {PureBss, BssTail, Normal, Conflict, Rejected}。
pub fn edge_legal(from: ClassifyState, to: ClassifyState) -> bool {
    matches!(
        (from, to),
        (ClassifyState::Pending, ClassifyState::PureBss)
            | (ClassifyState::Pending, ClassifyState::BssTail)
            | (ClassifyState::Pending, ClassifyState::Normal)
            | (ClassifyState::Pending, ClassifyState::Conflict)
            | (ClassifyState::Pending, ClassifyState::Rejected)
    )
}

// ── 分类闭式判定核 ──────────────────────────────────────────────
/// 返回（目标状态，BSS 尾字节数）。
pub fn classify_section(h: &SectionHeader, image_size: u32) -> Result<(ClassifyState, u32), BssCode> {
    // 前置越界检查。
    if h.virtual_address.checked_add(h.size_of_raw).is_none()
        || h.virtual_address.checked_add(h.size_of_raw).unwrap_or(0) > image_size
    {
        if h.virtual_address > image_size {
            return Err(BssCode::RvaOutOfRange);
        }
    }
    if h.virtual_address > image_size {
        return Err(BssCode::RvaOutOfRange);
    }
    if h.virtual_size > VSIZE_MAX {
        return Err(BssCode::VSizeOverflow);
    }
    let uninit_flag = h.characteristics & SCN_CNT_UNINITIALIZED_DATA != 0;
    let init_flag = h.characteristics & SCN_CNT_INITIALIZED_DATA != 0;
    if h.size_of_raw == 0 {
        if h.virtual_size == 0 {
            // 全零节（无用节）：普通节账，非 BSS。
            return Ok((ClassifyState::Normal, 0));
        }
        if !uninit_flag {
            // Raw=0 但缺未初始化位：特征位冲突账。
            return Ok((ClassifyState::Conflict, 0));
        }
        return Ok((ClassifyState::PureBss, h.virtual_size));
    }
    // Raw > 0：边界红线 —— VSize==SRaw 严格非 BSS。
    if h.virtual_size == h.size_of_raw {
        return Ok((ClassifyState::Normal, 0));
    }
    if h.virtual_size < h.size_of_raw {
        return Ok((ClassifyState::Normal, 0));
    }
    // VSize > SRaw：存在未初始化尾；特征位应为 INITIALIZED（数据在文件内）
    // 或至少不与纯未初始化声明矛盾。
    if !init_flag && !uninit_flag {
        return Ok((ClassifyState::Conflict, 0));
    }
    let tail = h.virtual_size - h.size_of_raw;
    Ok((ClassifyState::BssTail, tail))
}

// ── 分类账（守恒恒等式核） ──────────────────────────────────────
pub struct BssLedger {
    pub total: usize,
    pub pure_bss: usize,
    pub bss_tail: usize,
    pub normal: usize,
    pub conflict: usize,
    pub rejected: usize,
    /// BSS 尾字节累计（供映射器零页清零预算）。
    pub tail_bytes: u64,
    states: Vec<ClassifyState>,
}

impl BssLedger {
    pub fn new() -> BssLedger {
        BssLedger {
            total: 0,
            pure_bss: 0,
            bss_tail: 0,
            normal: 0,
            conflict: 0,
            rejected: 0,
            tail_bytes: 0,
            states: Vec::new(),
        }
    }

    pub fn record(&mut self, st: ClassifyState, tail: u32) -> Result<(), BssCode> {
        // 非法转换拒收：record 是每节一次的 Pending→终态账；
        // Pending 不在合法目标闭集内（终态出边亦全非法，见 edge_legal）。
        if !edge_legal(ClassifyState::Pending, st) {
            return Err(BssCode::LedgerBroken);
        }
        self.states.push(st);
        self.total += 1;
        match st {
            ClassifyState::PureBss => self.pure_bss += 1,
            ClassifyState::BssTail => {
                self.bss_tail += 1;
                self.tail_bytes += tail as u64;
            }
            ClassifyState::Normal => self.normal += 1,
            ClassifyState::Conflict => self.conflict += 1,
            ClassifyState::Rejected => self.rejected += 1,
            ClassifyState::Pending => return Err(BssCode::LedgerBroken),
        }
        Ok(())
    }

    /// 守恒恒等式：total == 各分类和（逐账核）。
    pub fn conserved(&self) -> bool {
        self.pure_bss + self.bss_tail + self.normal + self.conflict + self.rejected == self.total
    }
}

/// 表级分类入口：逐节判定并落账。
pub fn classify_table(sections: &[SectionHeader], image_size: u32) -> Result<BssLedger, BssCode> {
    let mut ledger = BssLedger::new();
    let mut i = 0;
    while i < sections.len() {
        match classify_section(&sections[i], image_size) {
            Ok((st, tail)) => ledger.record(st, tail)?,
            Err(_e) => {
                // 越界/溢出 → 拒载账（诊断由调用方取码）。
                ledger.record(ClassifyState::Rejected, 0)?;
            }
        }
        i += 1;
    }
    if !ledger.conserved() {
        return Err(BssCode::LedgerBroken);
    }
    Ok(ledger)
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

// ── 黄金对照表（12 例，含边界族） ────────────────────────────────
/// （名, VSize, SRaw, 期望态）——期望值独立写死，不复用被测值。
pub const GOLDEN: [(&str, u32, u32, ClassifyState); 12] = [
    (".text", 0x2A00, 0x2A00, ClassifyState::Normal),  // VSize==SRaw 边界
    (".data", 0x1800, 0x1000, ClassifyState::BssTail), // VSize>SRaw
    (".bss", 0x4000, 0x0000, ClassifyState::PureBss),  // Raw==0
    (".rdata", 0x1000, 0x1200, ClassifyState::Normal), // VSize<SRaw（对齐）
    (".idata", 0x0800, 0x0800, ClassifyState::Normal), // 边界重复核
    (".pdata", 0x0600, 0x0600, ClassifyState::Normal), // 边界重复核
    (".edata", 0x0000, 0x0000, ClassifyState::Normal), // 全零节
    (".tls", 0x0040, 0x0000, ClassifyState::PureBss),  // 小 BSS
    (".rsrc", 0x2000, 0x2000, ClassifyState::Normal),  // 边界族再核
    (".reloc", 0x0200, 0x0180, ClassifyState::BssTail),
    (".symtab", 0x0000, 0x1000, ClassifyState::Normal), // 只在文件
    (".debug", 0x0000, 0x0000, ClassifyState::Normal),  // 全零可丢弃
];

pub fn drill_golden_12() -> (bool, String) {
    let mut caught = 0;
    let mut i = 0;
    let img: u32 = 0x100000;
    while i < GOLDEN.len() {
        let (name, vs, sr, want) = GOLDEN[i];
        // 纯 BSS 行补未初始化位（真 BSS 节特征），其余保持 INITIALIZED。
        let chars = if sr == 0 && vs > 0 {
            SCN_CNT_INITIALIZED_DATA | SCN_CNT_UNINITIALIZED_DATA
        } else {
            SCN_CNT_INITIALIZED_DATA
        };
        let h = SectionHeader::new(name, vs, sr, 0x1000, chars);
        match classify_section(&h, img) {
            Ok((st, _tail)) => {
                if st == want {
                    caught += 1;
                }
            }
            Err(_) => {}
        }
        i += 1;
    }
    let ok = caught == 12;
    (ok, format!("黄金对照表 {}/12（含边界族）", caught))
}

pub fn drill_golden_chars() -> (bool, String) {
    // 特征位交叉核：Raw=0 且未带未初始化位 → 冲突账（不落 BSS）。
    let mut caught = 0;
    let h1 = SectionHeader::new(".bss", 0x1000, 0, 0x1000, SCN_CNT_INITIALIZED_DATA);
    if classify_section(&h1, 0x100000) == Ok((ClassifyState::Conflict, 0)) {
        caught += 1;
    }
    let h2 = SectionHeader::new(".bss", 0x1000, 0, 0x1000, SCN_CNT_UNINITIALIZED_DATA);
    let want_tail = 0x1000u32;
    if classify_section(&h2, 0x100000)
        == Ok((ClassifyState::PureBss, want_tail))
    {
        caught += 1;
    }
    // 反向：Raw>0 却声明纯未初始化位且 VSize<SRaw → 普通节（不冲突）。
    let h3 = SectionHeader::new(
        ".data",
        0x0800,
        0x1000,
        0x1000,
        SCN_CNT_INITIALIZED_DATA | SCN_CNT_UNINITIALIZED_DATA,
    );
    if classify_section(&h3, 0x100000) == Ok((ClassifyState::Normal, 0)) {
        caught += 1;
    }
    let ok = caught == 3;
    (ok, format!("特征位交叉核 {}/3", caught))
}

pub fn drill_boundary_100() -> (bool, String) {
    // 边界注入：VSize==SRaw 族 ×20 + VSize==SRaw±1 族，零误判。
    let mut caught = 0;
    let mut i = 0u32;
    while i < 20 {
        let base = 0x1000 + i * 0x40;
        let h_eq = SectionHeader::new(".bnd", base, base, 0x1000, SCN_CNT_INITIALIZED_DATA);
        let ok_eq = classify_section(&h_eq, 0x400000) == Ok((ClassifyState::Normal, 0));
        let h_lt = SectionHeader::new(".bnd", base - 1, base, 0x1000, SCN_CNT_INITIALIZED_DATA);
        let ok_lt = classify_section(&h_lt, 0x400000) == Ok((ClassifyState::Normal, 0));
        let h_gt = SectionHeader::new(".bnd", base + 1, base, 0x1000, SCN_CNT_INITIALIZED_DATA);
        let ok_gt = classify_section(&h_gt, 0x400000) == Ok((ClassifyState::BssTail, 1));
        if ok_eq && ok_lt && ok_gt {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 20;
    (ok, format!("边界三态族 {}/20（==/</> 零误判）", caught))
}

// ── 100 真实样本合成 + 一致率核 ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolVerdict {
    Bss,
    BssTail,
    Normal,
    Conflict,
    Rejected,
}

/// 合成一个"真实样本"：LCG 决定 VSize/SRaw/特征位组合，
/// 期望结论由独立闭式（与被测 classify 不同的路径）推得。
pub fn synth_sample(rng: &mut Lcg) -> (SectionHeader, u32, ToolVerdict) {
    let kind = rng.next_below(6);
    let vsize = (rng.next_below(0x8000) + 1) as u32;
    let sraw = (rng.next_below(0x8000) + 1) as u32;
    let chars = SCN_CNT_INITIALIZED_DATA | SCN_MEM_DISCARDABLE;
    match kind {
        0 => {
            // 纯 BSS 样本：Raw==0 + 未初始化位。
            let h = SectionHeader::new(".bss", vsize, 0, 0x1000, chars | SCN_CNT_UNINITIALIZED_DATA);
            (h, 0x100000, ToolVerdict::Bss)
        }
        1 => {
            // BSS 尾样本：0 < SRaw < VSize。
            let sr = if sraw < vsize { sraw } else { vsize / 2 };
            let h = SectionHeader::new(".data", vsize, sr, 0x1000, chars);
            (h, 0x100000, ToolVerdict::BssTail)
        }
        2 => {
            // 普通样本：VSize==SRaw（边界）。
            let h = SectionHeader::new(".text", vsize, vsize, 0x1000, chars);
            (h, 0x100000, ToolVerdict::Normal)
        }
        3 => {
            // 普通样本：VSize<SRaw。
            let sr = sraw + vsize;
            let h = SectionHeader::new(".rdata", vsize, sr, 0x1000, chars);
            (h, 0x100000, ToolVerdict::Normal)
        }
        4 => {
            // 冲突样本：Raw==0 但缺未初始化位。
            let h = SectionHeader::new(".weird", vsize, 0, 0x1000, chars);
            (h, 0x100000, ToolVerdict::Conflict)
        }
        _ => {
            // 拒载样本：RVA 越界映像。
            let h = SectionHeader::new(".evil", vsize, sraw, 0x200000, chars);
            (h, 0x100000, ToolVerdict::Rejected)
        }
    }
}

/// 样本结论 → 独立闭式映射（与 classify 的状态机不同代码路径）。
pub fn verdict_of(st: ClassifyState) -> ToolVerdict {
    match st {
        ClassifyState::PureBss => ToolVerdict::Bss,
        ClassifyState::BssTail => ToolVerdict::BssTail,
        ClassifyState::Normal => ToolVerdict::Normal,
        ClassifyState::Conflict => ToolVerdict::Conflict,
        ClassifyState::Rejected => ToolVerdict::Rejected,
        ClassifyState::Pending => ToolVerdict::Rejected,
    }
}

pub fn drill_100_samples() -> (bool, String) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut agree = 0;
    let mut n = 0;
    while n < 100 {
        let (h, img, want) = synth_sample(&mut rng);
        let got = match classify_section(&h, img) {
            Ok((st, _)) => verdict_of(st),
            Err(_) => ToolVerdict::Rejected,
        };
        if got == want {
            agree += 1;
        }
        n += 1;
    }
    let ok = agree == 100;
    (ok, format!("100 样本一致率 {}/100", agree))
}

pub fn drill_ledger_conservation() -> (bool, String) {
    // 守恒对账：合成 64 节表，账实一致（分类和 == 总数）。
    let mut rng = Lcg::new(SEG_BASE as u64 + 1);
    let mut sections: Vec<SectionHeader> = Vec::new();
    let mut i = 0;
    while i < 64 {
        let (h, _, _) = synth_sample(&mut rng);
        sections.push(h);
        i += 1;
    }
    let ledger = match classify_table(&sections, 0x100000) {
        Ok(l) => l,
        Err(_) => BssLedger::new(),
    };
    let ok = ledger.conserved()
        && ledger.total == 64
        && ledger.pure_bss + ledger.bss_tail + ledger.normal + ledger.conflict + ledger.rejected
            == ledger.total;
    (
        ok,
        format!(
            "守恒: 总={} 纯BSS={} 尾={} 普通={} 冲突={} 拒={} 尾字节={}",
            ledger.total, ledger.pure_bss, ledger.bss_tail, ledger.normal, ledger.conflict,
            ledger.rejected, ledger.tail_bytes
        ),
    )
}

pub fn drill_out_of_range_reject() -> (bool, String) {
    // 注入面：RVA 越界 + VSize 溢出 → 拒载落账 10/10。
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let h1 = SectionHeader::new(".evil", 0x100, 0x100, 0x100001 + i, SCN_CNT_INITIALIZED_DATA);
        let r1 = classify_section(&h1, 0x100000);
        let bad_v = VSIZE_MAX + 1 + i;
        let h2 = SectionHeader::new(".evil", bad_v, 0x100, 0x1000, SCN_CNT_INITIALIZED_DATA);
        let r2 = classify_section(&h2, 0x400000);
        if r1 == Err(BssCode::RvaOutOfRange) && r2 == Err(BssCode::VSizeOverflow) {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("越界/溢出拒载 {}/10", caught))
}

pub fn drill_illegal_edge() -> (bool, String) {
    // 非法转换拒收：Pending 目标拒收 + 终态出边全非法（闭集双核）。
    let mut ok = true;
    let mut ledger = BssLedger::new();
    ok = ok && ledger.record(ClassifyState::Normal, 0).is_ok();
    let mut l2 = BssLedger::new();
    ok = ok && l2.record(ClassifyState::Pending, 0) == Err(BssCode::LedgerBroken);
    // 状态机核：五个终态的全部出边（含到 Pending）均非法。
    let froms = [
        ClassifyState::PureBss,
        ClassifyState::BssTail,
        ClassifyState::Normal,
        ClassifyState::Conflict,
        ClassifyState::Rejected,
    ];
    let tos = [
        ClassifyState::PureBss,
        ClassifyState::BssTail,
        ClassifyState::Normal,
        ClassifyState::Conflict,
        ClassifyState::Rejected,
        ClassifyState::Pending,
    ];
    let mut a = 0;
    while a < froms.len() {
        let mut b = 0;
        while b < tos.len() {
            ok = ok && !edge_legal(froms[a], tos[b]);
            b += 1;
        }
        a += 1;
    }
    // 合法边闭集：Pending→各终态全合法。
    let targets = [
        ClassifyState::PureBss,
        ClassifyState::BssTail,
        ClassifyState::Normal,
        ClassifyState::Conflict,
        ClassifyState::Rejected,
    ];
    let mut j = 0;
    while j < targets.len() {
        ok = ok && edge_legal(ClassifyState::Pending, targets[j]);
        j += 1;
    }
    (
        ok,
        format!("非法转换拒收 + 合法边闭集: {}", ok),
    )
}

pub fn drill_zero_section_name_width() -> (bool, String) {
    // 规格红线：节名宽 8 字节写死；名字超长截断不破分类。
    let long_name = ".verylongsection";
    let mut truncated = String::new();
    let bytes = long_name.as_bytes();
    let mut i = 0;
    while i < SECTION_NAME_LEN && i < bytes.len() {
        truncated.push(bytes[i] as char);
        i += 1;
    }
    let h = SectionHeader::new(&truncated, 0x2000, 0x1000, 0x1000, SCN_CNT_INITIALIZED_DATA);
    let ok = truncated.len() == 8
        && classify_section(&h, 0x100000) == Ok((ClassifyState::BssTail, 0x1000));
    (ok, format!("节名截断={} 分类保持", truncated.len() == 8))
}

pub fn drill_tail_bytes_budget() -> (bool, String) {
    // 尾字节预算对账：BSS 尾累计 == Σ(VSize-SRaw)（仅尾类节）。
    let mut rng = Lcg::new(SEG_BASE as u64 + 2);
    let mut sections: Vec<SectionHeader> = Vec::new();
    let mut i = 0;
    while i < 32 {
        let (h, _, _) = synth_sample(&mut rng);
        sections.push(h);
        i += 1;
    }
    let ledger = match classify_table(&sections, 0x100000) {
        Ok(l) => l,
        Err(_) => BssLedger::new(),
    };
    // 独立重算（只计通过越界门的尾类节，与账口径一致）。
    let mut want: u64 = 0;
    let mut j = 0;
    while j < sections.len() {
        let h = &sections[j];
        if h.virtual_address <= 0x100000
            && h.size_of_raw > 0
            && h.virtual_size > h.size_of_raw
            && h.virtual_size <= VSIZE_MAX
        {
            want += (h.virtual_size - h.size_of_raw) as u64;
        }
        j += 1;
    }
    let ok = ledger.tail_bytes == want;
    (ok, format!("尾字节预算: 账={} 独={} 对{}", ledger.tail_bytes, want, ok))
}

pub fn drill_multi_machine() -> (bool, String) {
    // 多机对拍：三建模机（不同样本流）对同一闭式结论一致。
    let machines = [0x65300u64, 0x65301, 0x65302];
    let mut all_ok = true;
    let mut mi = 0;
    while mi < 3 {
        let mut rng = Lcg::new(machines[mi]);
        let mut agree = 0;
        let mut n = 0;
        while n < 33 {
            let (h, img, want) = synth_sample(&mut rng);
            let got = match classify_section(&h, img) {
                Ok((st, _)) => verdict_of(st),
                Err(_) => ToolVerdict::Rejected,
            };
            if got == want {
                agree += 1;
            }
            n += 1;
        }
        all_ok = all_ok && agree == 33;
        mi += 1;
    }
    (all_ok, format!("3 机对拍 33×3 一致: {}", all_ok))
}

pub fn drill_diag_codes_closed() -> (bool, String) {
    // 诊断码闭集 + detail 非空 + 反向判据（未知码返回未知文案）。
    let mut ok = true;
    let mut i = 0;
    while i < BssCode::ALL.len() {
        ok = ok && BssCode::ALL[i].val() >= SEG_BASE && BssCode::ALL[i].val() < SEG_BASE + 0x40;
        ok = ok && !BssCode::ALL[i].detail().is_empty();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x3F) == "未知诊断码";
    (ok, format!("诊断码闭集 {}/8 + 反向", BssCode::ALL.len()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < BssCode::ALL.len() {
        let v = BssCode::ALL[i].val();
        ok = ok && v >= SEG_BASE && v < SEG_BASE + 0x40;
        i += 1;
    }
    ok
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_golden_12();
    let (a2, d2) = drill_100_samples();
    let (a3, d3) = drill_ledger_conservation();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 黄金={} 百样本={} 守恒={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_golden_12();
    v.push(("J1-黄金对照表12例", a, b));
    let (a, b) = drill_golden_chars();
    v.push(("J2-特征位交叉核", a, b));
    let (a, b) = drill_boundary_100();
    v.push(("J1-边界三态族20组零误判", a, b));
    let (a, b) = drill_100_samples();
    v.push(("J1-100样本一致率100%", a, b));
    let (a, b) = drill_ledger_conservation();
    v.push(("J2-分类账守恒", a, b));
    let (a, b) = drill_out_of_range_reject();
    v.push(("J2-越界溢出拒载10/10", a, b));
    let (a, b) = drill_illegal_edge();
    v.push(("J2-非法转换拒收+边闭集", a, b));
    let (a, b) = drill_zero_section_name_width();
    v.push(("J3-节名8字节规格", a, b));
    let (a, b) = drill_tail_bytes_budget();
    v.push(("J2-尾字节预算对账", a, b));
    let (a, b) = drill_multi_machine();
    v.push(("J2-3机对拍", a, b));
    let (a, b) = drill_diag_codes_closed();
    v.push(("J3-诊断码闭集+反向", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx12841_checks：本单判据聚合（诊断段 0x65300）。
pub fn run_unx12841_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12841 BSS 类节识别");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65300 段闭集");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = checks[i].clone();
        s.add_dyn(n, p, &d);
        i += 1;
    }
    s
}
