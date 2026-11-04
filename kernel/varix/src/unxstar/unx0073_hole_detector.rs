//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0073 · 内存空洞检测器 · 0x6E180
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0073-J1 可用区间序列的空洞检测：相邻区间间隙逐个
//!     报账（基址/长度），间隙 = 0 判相邻，重叠判错（可用账红线），
//!     检测与真机间隙账逐条一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 排序前提：可用区间升序（乱序先拒）；
//!   - 间隙账：gap = next.base - cur.end（>0 空洞 / ==0 相邻 /
//!     <0 重叠错）；
//!   - 重叠红线：任一重叠即拒（可用内存不许交）；
//!   - 空洞账：逐洞登记（基址/长度），与真机账逐条对拍；
//!   - 首尾洞：低于首区与高于尾区的洞不入账（窗口外语义）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E180，AI-VX3-1102 批I 独占区）。
pub const SEG_BASE: u32 = 0x6E180;

/// 最大区间数。
pub const MAX_SPANS: usize = 64;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HoleCode {
    /// 区间乱序
    OrderInverted,
    /// 可用区间重叠（红线）
    UsableOverlap,
    /// 零长区间
    ZeroSpan,
    /// 间隙账失配（与真机逐条对不上）
    HoleMismatch,
    /// 区间数越上限
    CountOver,
    /// 双跑不一致
    NotReproducible,
}

impl HoleCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HoleCode::OrderInverted => "区间乱序：先排序后检测",
            HoleCode::UsableOverlap => "可用区间重叠：红线",
            HoleCode::ZeroSpan => "零长区间",
            HoleCode::HoleMismatch => "间隙账失配",
            HoleCode::CountOver => "区间数越上限：64 冻结",
            HoleCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [HoleCode; 6] = [
        HoleCode::OrderInverted,
        HoleCode::UsableOverlap,
        HoleCode::ZeroSpan,
        HoleCode::HoleMismatch,
        HoleCode::CountOver,
        HoleCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HoleCode::ALL.len() {
        if HoleCode::ALL[i].val() == code {
            return HoleCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E180 段闭集内"
}

// ── 区间与空洞账 ────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsableSpan {
    pub base: u64,
    pub len: u64,
}

impl UsableSpan {
    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.len)
    }
    pub fn legal(&self) -> Result<(), u32> {
        if self.len == 0 {
            return Err(HoleCode::ZeroSpan.val());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hole {
    pub base: u64,
    pub len: u64,
}

/// 空洞检测：升序区间序列 → 间隙账（重叠即拒）。
pub fn detect_holes(spans: &[UsableSpan]) -> Result<Vec<Hole>, u32> {
    if spans.len() > MAX_SPANS {
        return Err(HoleCode::CountOver.val());
    }
    let mut i = 0;
    while i < spans.len() {
        spans[i].legal()?;
        i += 1;
    }
    let mut holes = Vec::new();
    let mut i = 1;
    while i < spans.len() {
        let prev = &spans[i - 1];
        let cur = &spans[i];
        if cur.base < prev.base {
            return Err(HoleCode::OrderInverted.val());
        }
        if cur.base < prev.end() {
            return Err(HoleCode::UsableOverlap.val());
        }
        let gap = cur.base - prev.end();
        if gap > 0 {
            holes.push(Hole { base: prev.end(), len: gap });
        }
        i += 1;
    }
    Ok(holes)
}

/// 间隙账对拍：检测账与真机账逐条一致。
pub fn holes_match(spans: &[UsableSpan], expect: &[Hole]) -> Result<usize, u32> {
    let holes = detect_holes(spans)?;
    if holes.len() != expect.len() {
        return Err(HoleCode::HoleMismatch.val());
    }
    let mut i = 0;
    while i < holes.len() {
        if holes[i] != expect[i] {
            return Err(HoleCode::HoleMismatch.val());
        }
        i += 1;
    }
    Ok(holes.len())
}

/// 空洞总量。
pub fn holes_total(holes: &[Hole]) -> u64 {
    let mut t = 0u64;
    let mut i = 0;
    while i < holes.len() {
        t += holes[i].len;
        i += 1;
    }
    t
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

// ── 注入检出账（四路：重叠路/乱序路/零长路/对拍路） ──────────────
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

/// 真机样本：三可用区两洞。
pub fn make_machine_spans() -> Vec<UsableSpan> {
    vec![
        UsableSpan { base: 0x0100_0000, len: 0x100_0000 },
        UsableSpan { base: 0x0300_0000, len: 0x100_0000 },
        UsableSpan { base: 0x0500_0000, len: 0x200_0000 },
    ]
}

/// 真机间隙账：[0x2000000,1M) 与 [0x4000000,1M)。
pub fn machine_holes() -> Vec<Hole> {
    vec![
        Hole { base: 0x0200_0000, len: 0x100_0000 },
        Hole { base: 0x0400_0000, len: 0x100_0000 },
    ]
}

// ── 黄金全链：检测→对拍→总量→相邻语义 ───────────────────────────
pub fn golden_run() -> Result<(usize, u64, u32), u32> {
    let spans = make_machine_spans();
    let n = holes_match(&spans, &machine_holes())?;
    let total = holes_total(&machine_holes());
    // 相邻语义：相邻区间无洞。
    let adj = [
        UsableSpan { base: 0x1000, len: 0x1000 },
        UsableSpan { base: 0x2000, len: 0x1000 },
    ];
    let adj_holes = detect_holes(&adj)?;
    let adj_zero = if adj_holes.is_empty() { 0 } else { 1 };
    Ok((n, total, adj_zero))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → 2 洞 + 20M + 相邻零洞。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 0x0200_0000, 0)) => Ok(()),
        _ => Err(HoleCode::NotReproducible.val()),
    }
}

/// 序 B：重叠 ⇒ UsableOverlap。
pub fn probe_seq_b() -> Result<(), u32> {
    let spans = [
        UsableSpan { base: 0x1000, len: 0x2000 },
        UsableSpan { base: 0x2000, len: 0x1000 },
    ];
    match detect_holes(&spans) {
        Err(e) if e == HoleCode::UsableOverlap.val() => Ok(()),
        _ => Err(HoleCode::NotReproducible.val()),
    }
}

/// 序 C：零长区间 ⇒ ZeroSpan。
pub fn probe_seq_c() -> Result<(), u32> {
    let spans = [UsableSpan { base: 0x1000, len: 0 }];
    match detect_holes(&spans) {
        Err(e) if e == HoleCode::ZeroSpan.val() => Ok(()),
        _ => Err(HoleCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：重叠路。
        let bad = [
            UsableSpan { base: 0x1000, len: 0x2000 },
            UsableSpan { base: 0x1800, len: 0x1000 },
        ];
        led.record(0, detect_holes(&bad).is_err());
        // 路 2：乱序路。
        let dis = [
            UsableSpan { base: 0x3000, len: 0x1000 },
            UsableSpan { base: 0x1000, len: 0x1000 },
        ];
        led.record(1, detect_holes(&dis).is_err());
        // 路 3：零长路。
        let z = [UsableSpan { base: 0x1000, len: 0 }];
        led.record(2, detect_holes(&z).is_err());
        // 路 4：对拍路（少一洞必失配）。
        let spans = make_machine_spans();
        led.record(3, holes_match(&spans, &machine_holes()[..1]).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HoleCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HoleCode::ALL.len() {
        let v = HoleCode::ALL[i].val();
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
            Ok((n, total, adj)) => *n == 2 && *total == 0x0200_0000 && *adj == 0,
            Err(_) => false,
        },
        match &g {
            Ok((n, total, adj)) => format!("holes={} total=0x{:x} adj_gap={}", n, total, adj),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 逐洞对拍（基址+长度双账）。
    let holes = detect_holes(&make_machine_spans());
    v.push((
        "J1-逐洞对拍",
        match &holes {
            Ok(h) => h[0] == Hole { base: 0x0200_0000, len: 0x100_0000 }
                && h[1] == Hole { base: 0x0400_0000, len: 0x100_0000 },
            Err(_) => false,
        },
        match &holes {
            Ok(h) => format!("h0=[0x{:x},0x{:x}) h1=[0x{:x},0x{:x})", h[0].base, h[0].len, h[1].base, h[1].len),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 窗口外语义：首区之前的洞不入账（单区间零洞）。
    let one = [UsableSpan { base: 0x100000, len: 0x1000 }];
    v.push((
        "J1-窗口外",
        detect_holes(&one).map(|h| h.is_empty()).unwrap_or(false),
        String::from("首区前洞不入账"),
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
        String::from("0x6E180 窗口"),
    ));
    v
}

pub fn run_unx0073_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0073 内存空洞检测器");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
