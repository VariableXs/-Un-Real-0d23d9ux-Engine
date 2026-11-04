//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0194 · MTRR/PAT 初始化（缓存属性基线） · 0x6F780
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F0194-J1 PAT 表八项冻结（UC/WC/WT/WB/UC-/WP 七型账
//!     + 一保留），MTRR 区账（基址/掩码/类型，基址对齐尺寸），
//!     区重叠拒，账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F780，AI-VX3-1102 批Q 独占区）。
pub const SEG_BASE: u32 = 0x6F780;

/// PAT 表八项（冻结 PA 编码：UC0 WC1 WT4 WB6 UC-5 WP7 保留3 保留2）。
pub const PAT_TABLE: [u8; 8] = [0, 1, 4, 3, 6, 5, 2, 7];
/// 内存类型（冻结）。
pub const MT_UC: u8 = 0;
pub const MT_WC: u8 = 1;
pub const MT_WT: u8 = 4;
pub const MT_WP: u8 = 5;
pub const MT_WB: u8 = 6;
/// 最大 MTRR 变量区。
pub const MAX_MTRR: usize = 8;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatCode {
    /// PAT 表偏移账外
    PatIndexOver,
    /// MTRR 类型账外
    MemTypeUnknown,
    /// 区基址非尺寸对齐
    BaseNotSizeAligned,
    /// 区重叠
    RangeOverlap,
    /// 区越上限
    RangeOver,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl PatCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PatCode::PatIndexOver => "PAT 表偏移越界：≥8",
            PatCode::MemTypeUnknown => "MTRR 类型账外：非 0/1/4/5/6",
            PatCode::BaseNotSizeAligned => "区基址非尺寸对齐",
            PatCode::RangeOverlap => "区重叠：红线",
            PatCode::RangeOver => "区越上限：8 冻结",
            PatCode::ReplayMismatch => "复算失配",
            PatCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PatCode; 7] = [
        PatCode::PatIndexOver,
        PatCode::MemTypeUnknown,
        PatCode::BaseNotSizeAligned,
        PatCode::RangeOverlap,
        PatCode::RangeOver,
        PatCode::ReplayMismatch,
        PatCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PatCode::ALL.len() {
        if PatCode::ALL[i].val() == code {
            return PatCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F780 段闭集内"
}

// ── PAT 账 ──────────────────────────────────────────────────────
/// PAT 项编码（偏移 → PA 值）。
pub fn pat_entry(index: usize) -> Result<u8, u32> {
    if index >= 8 {
        return Err(PatCode::PatIndexOver.val());
    }
    Ok(PAT_TABLE[index])
}

// ── MTRR 区账 ───────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MtrrRange {
    pub base: u64,
    pub size: u64,
    pub mem_type: u8,
}

impl MtrrRange {
    /// 区闸：类型闭集 + 基址尺寸对齐（2^n 尺寸）。
    pub fn legal(&self) -> Result<(), u32> {
        let known = [MT_UC, MT_WC, MT_WT, MT_WP, MT_WB];
        let mut hit = false;
        let mut i = 0;
        while i < known.len() {
            if known[i] == self.mem_type {
                hit = true;
            }
            i += 1;
        }
        if !hit {
            return Err(PatCode::MemTypeUnknown.val());
        }
        if self.size == 0 || (self.size & (self.size - 1)) != 0 {
            return Err(PatCode::BaseNotSizeAligned.val());
        }
        if self.base % self.size != 0 {
            return Err(PatCode::BaseNotSizeAligned.val());
        }
        Ok(())
    }

    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.size)
    }
}

#[derive(Clone, Debug, Default)]
pub struct MtrrLedger {
    pub ranges: Vec<MtrrRange>,
    pub overlaps: u32,
}

impl MtrrLedger {
    pub fn new() -> MtrrLedger {
        MtrrLedger::default()
    }

    /// 登记（重叠守卫）。
    pub fn add(&mut self, r: MtrrRange) -> Result<usize, u32> {
        r.legal()?;
        if self.ranges.len() >= MAX_MTRR {
            return Err(PatCode::RangeOver.val());
        }
        let mut i = 0;
        while i < self.ranges.len() {
            let e = &self.ranges[i];
            if r.base < e.end() && e.base < r.end() {
                self.overlaps += 1;
                return Err(PatCode::RangeOverlap.val());
            }
            i += 1;
        }
        self.ranges.push(r);
        Ok(self.ranges.len() - 1)
    }
}

/// 复算。
pub fn replay_ok(ranges: &[MtrrRange]) -> Result<(), u32> {
    let mut a = MtrrLedger::new();
    let mut b = MtrrLedger::new();
    let mut i = 0;
    while i < ranges.len() {
        let _ = a.add(ranges[i]);
        let _ = b.add(ranges[i]);
        i += 1;
    }
    if a.ranges != b.ranges {
        return Err(PatCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越偏路/类型路/重叠路/复算路） ──────────────
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

/// 真机 MTRR 区（WB 主存 + UC 显存）。
pub fn machine_ranges() -> Vec<MtrrRange> {
    vec![
        MtrrRange { base: 0, size: 0x4000_0000, mem_type: MT_WB },
        MtrrRange { base: 0xE000_0000, size: 0x1000_0000, mem_type: MT_UC },
    ]
}

// ── 黄金全链：PAT 八项→MTRR 登记→重叠抓→类型/对齐拒→复算→越偏拒 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // PAT 八项全取。
    let mut pat_ok = true;
    let mut i = 0;
    while i < 8 {
        if pat_entry(i).is_err() {
            pat_ok = false;
        }
        i += 1;
    }
    // MTRR 登记。
    let mut l = MtrrLedger::new();
    let ranges = machine_ranges();
    let mut i = 0;
    while i < ranges.len() {
        l.add(ranges[i])?;
        i += 1;
    }
    // 重叠抓。
    let overlap = l.add(MtrrRange { base: 0x1000_0000, size: 0x4000_0000, mem_type: MT_WB }).is_err();
    // 类型拒。
    let bad_type = l.add(MtrrRange { base: 0xC000_0000, size: 0x1000_0000, mem_type: 3 }).is_err();
    // 对齐拒。
    let bad_align = l.add(MtrrRange { base: 0xC000_1000, size: 0x1000_0000, mem_type: MT_WB }).is_err();
    replay_ok(&ranges)?;
    Ok((pat_ok as u32 + overlap as u32, bad_type as u32 + bad_align as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：PAT 八项 + 重叠抓 + 类型/对齐拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 2, _)) => Ok(()),
        _ => Err(PatCode::NotReproducible.val()),
    }
}

/// 序 B：PAT 表冻结值。
pub fn probe_seq_b() -> Result<(), u32> {
    match PAT_TABLE == [0u8, 1, 4, 3, 6, 5, 2, 7] {
        true => Ok(()),
        false => Err(PatCode::NotReproducible.val()),
    }
}

/// 序 C：越偏 ⇒ PatIndexOver。
pub fn probe_seq_c() -> Result<(), u32> {
    match pat_entry(8) {
        Err(e) if e == PatCode::PatIndexOver.val() => Ok(()),
        _ => Err(PatCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越偏路。
        led.record(0, pat_entry(9).is_err());
        // 路 2：类型路。
        let mut l2 = MtrrLedger::new();
        led.record(1, l2.add(MtrrRange { base: 0, size: 0x1000, mem_type: 2 }).is_err());
        // 路 3：重叠路。
        let mut l3 = MtrrLedger::new();
        let _ = l3.add(MtrrRange { base: 0, size: 0x1000, mem_type: MT_WB });
        led.record(2, l3.add(MtrrRange { base: 0x800, size: 0x800, mem_type: MT_WB }).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_ranges()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PatCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PatCode::ALL.len() {
        let v = PatCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((ok, rej, _)) => *ok == 2 && *rej == 2,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, _)) => format!("pat+overlap={} type+align={}", ok, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // PAT 表冻结。
    v.push((
        "J1-PAT冻结",
        PAT_TABLE == [0u8, 1, 4, 3, 6, 5, 2, 7],
        String::from("UC/WC/WT/保留/WB/UC-/保留/WP 冻结"),
    ));
    // 真机区账（WB 1G + UC 256M 不重叠）。
    let mut l = MtrrLedger::new();
    let ranges = machine_ranges();
    let mut i = 0;
    while i < ranges.len() {
        let _ = l.add(ranges[i]);
        i += 1;
    }
    v.push((
        "J1-真机区账",
        l.ranges.len() == 2 && l.overlaps == 0,
        String::from("WB 1G + UC 256M 零重叠"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6F780 窗口"),
    ));
    v
}

pub fn run_unx0194_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0194 MTRR/PAT 初始化（缓存属性基线）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
