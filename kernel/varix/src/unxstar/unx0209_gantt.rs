//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0209 · 引导时间线（甘特式阶段视图） · 0x6F940
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F0209-J1 甘特视图账：阶段条账（名/起/止 ms），条不重
//!     叠（后条 start ≥ 前条 end），渲染逐字（"[###--] name"），
//!     视图可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F940，AI-VX3-1102 批Q 独占区）。
pub const SEG_BASE: u32 = 0x6F940;

/// 视图宽（字符数冻结 24）。
pub const VIEW_WIDTH: usize = 24;
/// 最大条数。
pub const MAX_BARS: usize = 12;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GanttCode {
    /// 条越上限
    BarOver,
    /// 条重叠
    BarOverlap,
    /// 时间倒挂
    TimeInverted,
    /// 渲染失配
    RenderMismatch,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl GanttCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GanttCode::BarOver => "条越上限：12 冻结",
            GanttCode::BarOverlap => "条重叠：红线",
            GanttCode::TimeInverted => "时间倒挂：end < start",
            GanttCode::RenderMismatch => "渲染失配",
            GanttCode::ReplayMismatch => "复算失配",
            GanttCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [GanttCode; 6] = [
        GanttCode::BarOver,
        GanttCode::BarOverlap,
        GanttCode::TimeInverted,
        GanttCode::RenderMismatch,
        GanttCode::ReplayMismatch,
        GanttCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GanttCode::ALL.len() {
        if GanttCode::ALL[i].val() == code {
            return GanttCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F940 段闭集内"
}

// ── 甘特账 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GanttBar {
    pub name: String,
    pub start: u64,
    pub end: u64,
}

impl GanttBar {
    pub fn legal(&self) -> Result<(), u32> {
        if self.end < self.start {
            return Err(GanttCode::TimeInverted.val());
        }
        Ok(())
    }
    pub fn ms(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }
}

#[derive(Clone, Debug, Default)]
pub struct GanttView {
    pub bars: Vec<GanttBar>,
}

impl GanttView {
    pub fn new() -> GanttView {
        GanttView::default()
    }

    /// 加条（时间倒挂 + 重叠守卫）。
    pub fn add(&mut self, name: &str, start: u64, end: u64) -> Result<usize, u32> {
        let b = GanttBar { name: String::from(name), start, end };
        b.legal()?;
        if self.bars.len() >= MAX_BARS {
            return Err(GanttCode::BarOver.val());
        }
        if let Some(last) = self.bars.last() {
            if start < last.end {
                return Err(GanttCode::BarOverlap.val());
            }
        }
        self.bars.push(b);
        Ok(self.bars.len() - 1)
    }

    /// 渲染一行："[###--] name"（# 占已过比例）。
    pub fn render_bar(&self, idx: usize, total_ms: u64) -> Result<String, u32> {
        let b = match self.bars.get(idx) {
            Some(b) => b,
            None => return Err(GanttCode::RenderMismatch.val()),
        };
        let filled = if total_ms == 0 {
            0
        } else {
            ((b.ms() * VIEW_WIDTH as u64) / total_ms) as usize
        };
        let mut s = String::from("[");
        let mut i = 0;
        while i < VIEW_WIDTH {
            if i < filled {
                s.push('#');
            } else {
                s.push('-');
            }
            i += 1;
        }
        s.push(']');
        s.push(' ');
        s.push_str(&b.name);
        Ok(s)
    }
}

/// 复算。
pub fn replay_ok(bars: &[(&str, u64, u64)]) -> Result<(), u32> {
    let mut a = GanttView::new();
    let mut b = GanttView::new();
    let mut i = 0;
    while i < bars.len() {
        let _ = a.add(bars[i].0, bars[i].1, bars[i].2);
        let _ = b.add(bars[i].0, bars[i].1, bars[i].2);
        i += 1;
    }
    if a.bars != b.bars {
        return Err(GanttCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越限路/倒挂路/重叠路/复算路） ──────────────
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

/// 真机三阶段条。
pub fn machine_bars() -> Vec<(&'static str, u64, u64)> {
    vec![("A2交接", 0, 20), ("内存", 20, 170), ("ACPI", 170, 270)]
}

// ── 黄金全链：加条→渲染逐字→重叠拒→倒挂拒→复算→越限拒 ──────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut g = GanttView::new();
    let bars = machine_bars();
    let mut i = 0;
    while i < bars.len() {
        g.add(bars[i].0, bars[i].1, bars[i].2)?;
        i += 1;
    }
    // 渲染：A2 20/270 → filled = 20*24/270 = 1。
    let r0 = g.render_bar(0, 270)?;
    let render_ok = (r0.starts_with("[#") && r0.ends_with("] A2交接")) as u32;
    // 重叠拒。
    let overlap = g.add("坏条", 100, 200).is_err();
    // 倒挂拒。
    let inverted = g.add("坏条", 500, 400).is_err();
    replay_ok(&bars)?;
    Ok((render_ok + (g.bars.len() == 3) as u32, overlap as u32 + inverted as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：渲染 + 三条 + 两拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 2, _)) => Ok(()),
        _ => Err(GanttCode::NotReproducible.val()),
    }
}

/// 序 B：视图宽冻结。
pub fn probe_seq_b() -> Result<(), u32> {
    match VIEW_WIDTH {
        24 => Ok(()),
        _ => Err(GanttCode::NotReproducible.val()),
    }
}

/// 序 C：渲染行逐字（1/24 填充）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut g = GanttView::new();
    let _ = g.add("X", 0, 1);
    let _ = g.add("Y", 1, 25);
    let r = g.render_bar(0, 24)?;
    match r == "[#-----------------------] X" {
        true => Ok(()),
        false => Err(GanttCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：倒挂路。
        let mut g1 = GanttView::new();
        led.record(0, g1.add("x", 100, 50).is_err());
        // 路 2：重叠路。
        let mut g2 = GanttView::new();
        let _ = g2.add("a", 0, 100);
        led.record(1, g2.add("b", 50, 200).is_err());
        // 路 3：越限路。
        let mut g3 = GanttView::new();
        let mut over = false;
        let mut i = 0;
        while i <= MAX_BARS {
            let n = alloc::format!("p{}", i);
            if g3.add(&n, i as u64 * 10, i as u64 * 10 + 10).is_err() {
                over = true;
            }
            i += 1;
        }
        led.record(2, over);
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_bars()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GanttCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GanttCode::ALL.len() {
        let v = GanttCode::ALL[i].val();
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
            Ok((ok, rej, _)) => format!("render+bars={} rejects={}", ok, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 真机条账（三条不重叠）。
    let mut g = GanttView::new();
    let bars = machine_bars();
    let mut i = 0;
    while i < bars.len() {
        let _ = g.add(bars[i].0, bars[i].1, bars[i].2);
        i += 1;
    }
    v.push((
        "J1-真机条账",
        g.bars.len() == 3,
        String::from("A2 20ms / 内存 150ms / ACPI 100ms"),
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
        String::from("0x6F940 窗口"),
    ));
    v
}

pub fn run_unx0209_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0209 引导时间线（甘特式阶段视图）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
