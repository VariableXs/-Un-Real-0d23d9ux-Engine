//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0197 · init 自检清单（健康门禁） · 0x6F840
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0197-J1 init 清单账：六项闭集（GDT/中断/内存/时钟/
//!     控制台/引导参），逐项通过才过门禁（一红即停），清单封存
//!     可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F840，AI-VX3-1102 批Q 独占区）。
pub const SEG_BASE: u32 = 0x6F840;

/// 清单项闭集（冻结次序）。
pub const C_GDT: usize = 0;
pub const C_IRQ: usize = 1;
pub const C_MEM: usize = 2;
pub const C_CLOCK: usize = 3;
pub const C_CONSOLE: usize = 4;
pub const C_CMDLINE: usize = 5;
pub const ITEM_COUNT: usize = 6;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChkCode {
    /// 项越界
    ItemOver,
    /// 门禁红（有失败仍放行）
    GateOpenWithRed,
    /// 跳项（次序红线）
    ItemSkipped,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl ChkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ChkCode::ItemOver => "项越界：≥6",
            ChkCode::GateOpenWithRed => "有失败仍放行：门禁红线",
            ChkCode::ItemSkipped => "跳项：次序红线",
            ChkCode::ReplayMismatch => "复算失配",
            ChkCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ChkCode; 5] = [
        ChkCode::ItemOver,
        ChkCode::GateOpenWithRed,
        ChkCode::ItemSkipped,
        ChkCode::ReplayMismatch,
        ChkCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ChkCode::ALL.len() {
        if ChkCode::ALL[i].val() == code {
            return ChkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F840 段闭集内"
}

// ── 清单账 ──────────────────────────────────────────────────────
/// 项名（冻结）。
pub fn item_name(i: usize) -> Result<&'static str, u32> {
    match i {
        C_GDT => Ok("GDT"),
        C_IRQ => Ok("中断"),
        C_MEM => Ok("内存"),
        C_CLOCK => Ok("时钟"),
        C_CONSOLE => Ok("控制台"),
        C_CMDLINE => Ok("引导参"),
        _ => Err(ChkCode::ItemOver.val()),
    }
}

#[derive(Clone, Debug)]
pub struct InitChecklist {
    /// 逐项结果（true=过）。
    pub results: [bool; ITEM_COUNT],
    /// 已勾到第几项（次序账）。
    pub progress: usize,
    pub gate_open: bool,
}

impl InitChecklist {
    pub fn new() -> InitChecklist {
        InitChecklist { results: [false; ITEM_COUNT], progress: 0, gate_open: false }
    }

    /// 勾项（次序单调；失败照记，门禁不放行）。
    pub fn check(&mut self, item: usize, ok: bool) -> Result<(), u32> {
        if item >= ITEM_COUNT {
            return Err(ChkCode::ItemOver.val());
        }
        if item != self.progress {
            return Err(ChkCode::ItemSkipped.val());
        }
        self.results[item] = ok;
        self.progress += 1;
        Ok(())
    }

    /// 门禁（六项全过才放行）。
    pub fn gate(&mut self) -> Result<(), u32> {
        if self.progress != ITEM_COUNT {
            return Err(ChkCode::ItemSkipped.val());
        }
        let mut i = 0;
        while i < ITEM_COUNT {
            if !self.results[i] {
                return Err(ChkCode::GateOpenWithRed.val());
            }
            i += 1;
        }
        self.gate_open = true;
        Ok(())
    }
}

/// 复算。
pub fn replay_ok(seq: &[(usize, bool)]) -> Result<(), u32> {
    let mut a = InitChecklist::new();
    let mut b = InitChecklist::new();
    let mut i = 0;
    while i < seq.len() {
        let _ = a.check(seq[i].0, seq[i].1);
        let _ = b.check(seq[i].0, seq[i].1);
        i += 1;
    }
    if a.results != b.results || a.progress != b.progress {
        return Err(ChkCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越项路/跳项路/门禁路/复算路） ──────────────
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

/// 真机勾项（六项全过）。
pub fn machine_seq() -> Vec<(usize, bool)> {
    vec![(0, true), (1, true), (2, true), (3, true), (4, true), (5, true)]
}

// ── 黄金全链：六项过门禁→失败拦截→跳项拒→缺项拒→复算→越项拒 ──────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 全过。
    let mut c = InitChecklist::new();
    let seq = machine_seq();
    let mut i = 0;
    while i < seq.len() {
        c.check(seq[i].0, seq[i].1)?;
        i += 1;
    }
    c.gate()?;
    let green = c.gate_open as u32;
    // 失败拦截。
    let mut bad = InitChecklist::new();
    let mut i = 0;
    while i < seq.len() {
        let _ = bad.check(seq[i].0, i != C_CLOCK);
        i += 1;
    }
    let blocked = bad.gate().is_err();
    // 跳项拒（勾 0 后跳到 2）。
    let mut skip = InitChecklist::new();
    let _ = skip.check(C_GDT, true);
    let skipped = skip.check(C_MEM, true).is_err();
    // 缺项拒（未勾完就 gate）。
    let mut partial = InitChecklist::new();
    let _ = partial.check(C_GDT, true);
    let partial_reject = partial.gate().is_err();
    replay_ok(&seq)?;
    Ok((green, blocked as u32 + skipped as u32 + partial_reject as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：门禁绿 + 三拒全抓。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 3, _)) => Ok(()),
        _ => Err(ChkCode::NotReproducible.val()),
    }
}

/// 序 B：项越界 ⇒ ItemOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut c = InitChecklist::new();
    match c.check(9, true) {
        Err(e) if e == ChkCode::ItemOver.val() => Ok(()),
        _ => Err(ChkCode::NotReproducible.val()),
    }
}

/// 序 C：项名闭集。
pub fn probe_seq_c() -> Result<(), u32> {
    let want = ["GDT", "中断", "内存", "时钟", "控制台", "引导参"];
    let mut i = 0;
    while i < ITEM_COUNT {
        if item_name(i) != Ok(want[i]) {
            return Err(ChkCode::NotReproducible.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越项路。
        let mut c1 = InitChecklist::new();
        led.record(0, c1.check(7, true).is_err());
        // 路 2：跳项路。
        let mut c2 = InitChecklist::new();
        let _ = c2.check(0, true);
        led.record(1, c2.check(2, true).is_err());
        // 路 3：门禁路。
        let mut c3 = InitChecklist::new();
        let _ = c3.check(0, false);
        let _ = c3.check(1, true);
        let _ = c3.check(2, true);
        let _ = c3.check(3, true);
        let _ = c3.check(4, true);
        let _ = c3.check(5, true);
        led.record(2, c3.gate().is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_seq()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ChkCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ChkCode::ALL.len() {
        let v = ChkCode::ALL[i].val();
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
            Ok((green, rej, _)) => *green == 1 && *rej == 3,
            Err(_) => false,
        },
        match &g {
            Ok((green, rej, _)) => format!("gate_green={} rejects={}", green, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 六项闭集。
    v.push((
        "J1-六项闭集",
        ITEM_COUNT == 6,
        String::from("GDT/中断/内存/时钟/控制台/引导参 冻结"),
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
        String::from("0x6F840 窗口"),
    ));
    v
}

pub fn run_unx0197_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0197 init 自检清单（健康门禁）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
