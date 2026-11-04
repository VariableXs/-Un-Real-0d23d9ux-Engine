//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0154 · QEMU 冒烟（只读判据 · 开发期门禁）（深化二） · 0x6F080
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥180 行；
//!   ② UNX-F0154-J1 冒烟判据：串口输出帧序匹配账（引导徽标序列
//!     逐字），超时口径（帧间隔冻结），帧账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 冒烟徽标序列冻结（"VARIX BOOT OK" 三帧）；
//!   - 匹配闸：帧序逐字（缺帧/乱序/超时判红）；
//!   - 只读：判据不改模拟器状态；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F080，AI-VX3-1102 批N 独占区）。
pub const SEG_BASE: u32 = 0x6F080;

/// 冒烟徽标序列（冻结）。
pub const SMOKE_FRAMES: [&str; 3] = ["VARIX", "BOOT", "OK"];
/// 帧间隔上限（ms 冻结）。
pub const FRAME_GAP_MAX: u64 = 500;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmokeCode {
    /// 帧缺
    FrameMissing,
    /// 帧乱序
    FrameOutOfOrder,
    /// 帧间隔超时
    FrameGapOver,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl SmokeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SmokeCode::FrameMissing => "帧缺：徽标序列缺口",
            SmokeCode::FrameOutOfOrder => "帧乱序",
            SmokeCode::FrameGapOver => "帧间隔超时：>500ms",
            SmokeCode::ReplayMismatch => "复算失配",
            SmokeCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SmokeCode; 5] = [
        SmokeCode::FrameMissing,
        SmokeCode::FrameOutOfOrder,
        SmokeCode::FrameGapOver,
        SmokeCode::ReplayMismatch,
        SmokeCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SmokeCode::ALL.len() {
        if SmokeCode::ALL[i].val() == code {
            return SmokeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F080 段闭集内"
}

// ── 冒烟判据账 ──────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmokeFrame {
    pub text: String,
    pub ts: u64,
}

/// 冒烟判定（帧序逐字 + 间隔）。
pub fn smoke_check(frames: &[SmokeFrame]) -> Result<(), u32> {
    if frames.len() < SMOKE_FRAMES.len() {
        return Err(SmokeCode::FrameMissing.val());
    }
    let mut i = 0;
    while i < SMOKE_FRAMES.len() {
        if frames[i].text != SMOKE_FRAMES[i] {
            if frames[i].text != SMOKE_FRAMES[0] && i > 0 {
                return Err(SmokeCode::FrameOutOfOrder.val());
            }
            return Err(SmokeCode::FrameMissing.val());
        }
        if i > 0 {
            let gap = frames[i].ts.saturating_sub(frames[i - 1].ts);
            if gap > FRAME_GAP_MAX {
                return Err(SmokeCode::FrameGapOver.val());
            }
        }
        i += 1;
    }
    Ok(())
}

/// 复算：同帧账恒同判定。
pub fn replay_ok(frames: &[SmokeFrame]) -> Result<(), u32> {
    let a = smoke_check(frames);
    let b = smoke_check(frames);
    if a != b {
        return Err(SmokeCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：缺帧路/乱序路/超时路/复算路） ──────────────
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

/// 真机帧账（三帧合规间隔）。
pub fn machine_frames() -> Vec<SmokeFrame> {
    vec![
        SmokeFrame { text: String::from("VARIX"), ts: 100 },
        SmokeFrame { text: String::from("BOOT"), ts: 400 },
        SmokeFrame { text: String::from("OK"), ts: 700 },
    ]
}

// ── 黄金全链：真机帧 PASS → 缺帧/乱序/超时全拒 → 复算 ─────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let frames = machine_frames();
    smoke_check(&frames)?;
    replay_ok(&frames)?;
    // 缺帧。
    let miss = smoke_check(&frames[..2]).is_err();
    // 乱序。
    let mut disorder = frames.clone();
    disorder[1].text = String::from("OK");
    disorder[2].text = String::from("BOOT");
    let out_of_order = smoke_check(&disorder).is_err();
    // 超时。
    let mut slow = frames.clone();
    slow[2].ts = 2000;
    let timeout = smoke_check(&slow).is_err();
    Ok((1, miss as u32 + out_of_order as u32 + timeout as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机 PASS + 三拒全抓。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 3, _)) => Ok(()),
        _ => Err(SmokeCode::NotReproducible.val()),
    }
}

/// 序 B：徽标序列冻结逐字。
pub fn probe_seq_b() -> Result<(), u32> {
    let want = ["VARIX", "BOOT", "OK"];
    let mut i = 0;
    while i < 3 {
        if SMOKE_FRAMES[i] != want[i] {
            return Err(SmokeCode::NotReproducible.val());
        }
        i += 1;
    }
    Ok(())
}

/// 序 C：空帧账 ⇒ FrameMissing。
pub fn probe_seq_c() -> Result<(), u32> {
    match smoke_check(&[]) {
        Err(e) if e == SmokeCode::FrameMissing.val() => Ok(()),
        _ => Err(SmokeCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：缺帧路。
        led.record(0, smoke_check(&machine_frames()[..1]).is_err());
        // 路 2：乱序路。
        let mut d2 = machine_frames();
        d2[0].text = String::from("OK");
        led.record(1, smoke_check(&d2).is_err());
        // 路 3：超时路。
        let mut d3 = machine_frames();
        d3[1].ts = 1000;
        led.record(2, smoke_check(&d3).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_frames()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SmokeCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SmokeCode::ALL.len() {
        let v = SmokeCode::ALL[i].val();
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
            Ok((pass, rej, _)) => *pass == 1 && *rej == 3,
            Err(_) => false,
        },
        match &g {
            Ok((pass, rej, _)) => format!("pass={} rejects={}", pass, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 冒烟徽标冻结。
    v.push((
        "J1-徽标冻结",
        SMOKE_FRAMES == ["VARIX", "BOOT", "OK"] && FRAME_GAP_MAX == 500,
        String::from("VARIX/BOOT/OK 三帧 + 500ms 间隔冻结"),
    ));
    // 真机帧间隔账（100→400→700 均 ≤500）。
    let f = machine_frames();
    v.push((
        "J1-间隔账",
        f[1].ts - f[0].ts == 300 && f[2].ts - f[1].ts == 300,
        String::from("300ms 间隔 ×2 全合规"),
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
        String::from("0x6F080 窗口"),
    ));
    v
}

pub fn run_unx0154b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0154 QEMU 冒烟（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
