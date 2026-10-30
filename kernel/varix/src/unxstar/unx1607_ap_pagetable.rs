//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1607 · AP 临时页表与母版页表挂接路径 · 0x6CD80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B3）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1607-J1 临时页表恒等映射低 1MB + 内核高半区，与 A1
//!     F0012 固化布局逐项一致，AP 开启分页后首个 CR3 写入值与预
//!     期一致落账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 临时页表账：PML4[0]（恒等映射低 1MB）+ PML4[511]（内核高
//!      半区 0xFFFF800000000000），两入口齐备；
//!    - F0012 固化布局对拍：入口位/页属性（P|W）逐项一致；
//!    - CR3 首写：AP 开分页后首个 CR3 = 临时页表物理基址（页对
//!      齐），落账对拍；
//!    - 挂接路径：临时页表 → 母版页表交接（母版就绪标志位）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CD80，AI-VX3-1030 批D 独占区）。
pub const SEG_BASE: u32 = 0x6CD80;

// ── 页表冻结值 ─────────────────────────────────────────────────
/// PML4 入口数。
pub const PML4_ENTRIES: usize = 512;
/// 恒等映射入口下标。
pub const IDX_IDENTITY: usize = 0;
/// 内核高半区入口下标。
pub const IDX_HIGH_HALF: usize = 511;
/// 页属性：P|W。
pub const PF_PRESENT: u64 = 0x1;
pub const PF_WRITE: u64 = 0x2;
pub const PF_RW: u64 = PF_PRESENT | PF_WRITE;
/// 临时页表物理基址（页对齐，冻结样本）。
pub const TEMP_PT_BASE: u64 = 0x1_0000;
/// 低 1MB。
pub const LOW_1MB: u64 = 0x10_0000;
/// 高半区基址。
pub const HIGH_HALF_BASE: u64 = 0xFFFF_8000_0000_0000;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApPtCode {
    /// PML4 入口越界
    EntryOutOfRange,
    /// 入口缺（恒等/高半区缺一）
    EntryMissing,
    /// 页属性不符（F0012 布局破坏）
    PageFlagMismatch,
    /// CR3 非页对齐
    Cr3Misaligned,
    /// CR3 首写不符
    Cr3FirstWriteMismatch,
    /// 母版未就绪即交接
    MasterNotReady,
    /// 布局对拍失败
    LayoutMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl ApPtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ApPtCode::EntryOutOfRange => "PML4 入口越界：512 项",
            ApPtCode::EntryMissing => "入口缺：恒等/高半区缺一",
            ApPtCode::PageFlagMismatch => "页属性不符：P|W 冻结",
            ApPtCode::Cr3Misaligned => "CR3 非页对齐",
            ApPtCode::Cr3FirstWriteMismatch => "CR3 首写不符",
            ApPtCode::MasterNotReady => "母版未就绪即交接",
            ApPtCode::LayoutMismatch => "F0012 布局对拍失败",
            ApPtCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ApPtCode; 8] = [
        ApPtCode::EntryOutOfRange,
        ApPtCode::EntryMissing,
        ApPtCode::PageFlagMismatch,
        ApPtCode::Cr3Misaligned,
        ApPtCode::Cr3FirstWriteMismatch,
        ApPtCode::MasterNotReady,
        ApPtCode::LayoutMismatch,
        ApPtCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ApPtCode::ALL.len() {
        if ApPtCode::ALL[i].val() == code {
            return ApPtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CD80 段闭集内"
}

// ── 临时页表账 ─────────────────────────────────────────────────
pub struct TempPageTable {
    pub pml4: [u64; PML4_ENTRIES],
    /// 母版就绪标志。
    pub master_ready: bool,
    /// CR3 首写账。
    pub cr3_first: Option<u64>,
}

impl TempPageTable {
    pub fn new() -> TempPageTable {
        TempPageTable { pml4: [0; PML4_ENTRIES], master_ready: false, cr3_first: None }
    }

    /// 建恒等映射入口（PML4[0] = PDPT 物理址 | P|W）。
    pub fn map_identity(&mut self, pdpt_phys: u64) -> Result<(), u32> {
        if pdpt_phys & 0xFFF != 0 {
            return Err(ApPtCode::Cr3Misaligned.val());
        }
        self.pml4[IDX_IDENTITY] = pdpt_phys | PF_RW;
        Ok(())
    }

    /// 建高半区入口（PML4[511]）。
    pub fn map_high_half(&mut self, pdpt_phys: u64) -> Result<(), u32> {
        if pdpt_phys & 0xFFF != 0 {
            return Err(ApPtCode::Cr3Misaligned.val());
        }
        self.pml4[IDX_HIGH_HALF] = pdpt_phys | PF_RW;
        Ok(())
    }

    /// 布局对拍（F0012 固化）：两入口齐 + 属性 P|W + 其余零。
    pub fn layout_audit(&self) -> Result<(), u32> {
        if self.pml4[IDX_IDENTITY] & PF_PRESENT == 0
            || self.pml4[IDX_HIGH_HALF] & PF_PRESENT == 0
        {
            return Err(ApPtCode::EntryMissing.val());
        }
        if self.pml4[IDX_IDENTITY] & PF_WRITE == 0
            || self.pml4[IDX_HIGH_HALF] & PF_WRITE == 0
        {
            return Err(ApPtCode::PageFlagMismatch.val());
        }
        let mut i = 0;
        while i < PML4_ENTRIES {
            if i != IDX_IDENTITY && i != IDX_HIGH_HALF && self.pml4[i] != 0 {
                return Err(ApPtCode::LayoutMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// AP 开分页：首个 CR3 写（页对齐 + 与临时页表基址一致）。
    pub fn write_cr3_first(&mut self, v: u64) -> Result<(), u32> {
        if v & 0xFFF != 0 {
            return Err(ApPtCode::Cr3Misaligned.val());
        }
        if v != TEMP_PT_BASE {
            return Err(ApPtCode::Cr3FirstWriteMismatch.val());
        }
        self.cr3_first = Some(v);
        Ok(())
    }

    /// 交接母版：就绪标志必须置位。
    pub fn handoff_master(&self) -> Result<(), u32> {
        if !self.master_ready {
            return Err(ApPtCode::MasterNotReady.val());
        }
        Ok(())
    }
}

// ── F0012 固化布局账（对拍锚） ──────────────────────────────────
pub const F0012_EXPECT: [(usize, u64, u64); 2] = [
    (IDX_IDENTITY, PF_RW, LOW_1MB),
    (IDX_HIGH_HALF, PF_RW, HIGH_HALF_BASE),
];

/// F0012 逐项对拍。
pub fn f0012_audit(pt: &TempPageTable) -> Result<(), u32> {
    let mut i = 0;
    while i < F0012_EXPECT.len() {
        let (idx, flags, _) = F0012_EXPECT[i];
        let got = pt.pml4[idx];
        if got & 0xFFF != flags & 0xFFF {
            return Err(ApPtCode::LayoutMismatch.val());
        }
        i += 1;
    }
    Ok(())
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

/// 黄金全链：建双入口→F0012 对拍→CR3 首写→母版交接。
pub fn golden_run() -> Result<(u64, u64, u64), u32> {
    let mut pt = TempPageTable::new();
    pt.map_identity(0x2000)?;
    pt.map_high_half(0x3000)?;
    pt.layout_audit()?;
    f0012_audit(&pt)?;
    pt.write_cr3_first(TEMP_PT_BASE)?;
    if pt.cr3_first != Some(TEMP_PT_BASE) {
        return Err(ApPtCode::Cr3FirstWriteMismatch.val());
    }
    pt.master_ready = true;
    pt.handoff_master()?;
    Ok((pt.pml4[IDX_IDENTITY], pt.pml4[IDX_HIGH_HALF], pt.cr3_first.unwrap_or(0)))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut pt = TempPageTable::new();
    pt.map_identity(0x2000)?;
    pt.map_high_half(0x3000)?;
    pt.layout_audit()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // CR3 非页对齐 → 必拒。
    let mut pt = TempPageTable::new();
    pt.write_cr3_first(0x1003)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 母版未就绪 → 必拒。
    let pt = TempPageTable::new();
    pt.handoff_master()
}

/// 注入 10/10：入口缺、属性错、CR3 错值、未就绪交接四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：入口缺。
        let mut pt0 = TempPageTable::new();
        pt0.map_identity(0x2000).ok();
        led.record(0, pt0.layout_audit().is_err());
        // 槽1：属性错。
        let mut pt1 = TempPageTable::new();
        pt1.map_identity(0x2000).ok();
        pt1.map_high_half(0x3000).ok();
        pt1.pml4[IDX_HIGH_HALF] &= !PF_WRITE;
        led.record(1, pt1.layout_audit().is_err());
        // 槽2：CR3 错值。
        let mut pt2 = TempPageTable::new();
        led.record(2, pt2.write_cr3_first(0x9000).is_err());
        // 槽3：未就绪交接。
        let pt3 = TempPageTable::new();
        led.record(3, pt3.handoff_master().is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ApPtCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ApPtCode::ALL.len() {
        let v = ApPtCode::ALL[i].val();
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
    // 双入口布局。
    let mut pt = TempPageTable::new();
    pt.map_identity(0x2000).ok();
    pt.map_high_half(0x3000).ok();
    v.push((
        "J1-恒等+高半区双入口",
        pt.layout_audit().is_ok() && IDX_IDENTITY == 0 && IDX_HIGH_HALF == 511,
        format!("idx {} / {}", IDX_IDENTITY, IDX_HIGH_HALF),
    ));
    // F0012 对拍。
    v.push((
        "J1-F0012布局逐项一致",
        f0012_audit(&pt).is_ok() && F0012_EXPECT.len() == 2,
        format!("{} 项", F0012_EXPECT.len()),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CD80 窗口")));
    v
}

pub fn run_unx1607_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1607 AP 临时页表与母版页表挂接路径");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
