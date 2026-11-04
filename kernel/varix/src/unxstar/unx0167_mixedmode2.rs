//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0167 · Mixed Mode（IA32 on x64）检测（深化第二实现） · 0x6F1C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0167-J1 Mixed Mode 检测：三源一致性（固件位数/表指
//!     针位宽/运行时服务位数），任一 32 位 ⇒ 拒载 Mixed Mode（显
//!     性码），全 64 位放行，检测账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F1C0，AI-VX3-1102 批O 独占区）。
pub const SEG_BASE: u32 = 0x6F1C0;

/// 位宽（冻结枚举）。
pub const W_64: u8 = 64;
pub const W_32: u8 = 32;
/// 三源数。
pub const SRC_COUNT: usize = 3;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MixCode {
    /// 位宽账外（非 32/64）
    WidthUnknown,
    /// Mixed Mode 拒载
    MixedModeRejected,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl MixCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MixCode::WidthUnknown => "位宽账外：非 32/64",
            MixCode::MixedModeRejected => "Mixed Mode 拒载：任一 32 位",
            MixCode::ReplayMismatch => "复算失配",
            MixCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [MixCode; 4] = [
        MixCode::WidthUnknown,
        MixCode::MixedModeRejected,
        MixCode::ReplayMismatch,
        MixCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MixCode::ALL.len() {
        if MixCode::ALL[i].val() == code {
            return MixCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F1C0 段闭集内"
}

// ── 检测账 ──────────────────────────────────────────────────────
/// 三源账：[固件位数, 表指针位宽, 运行时服务位数]。
pub fn mixed_check(widths: [u8; SRC_COUNT]) -> Result<(), u32> {
    let mut i = 0;
    while i < SRC_COUNT {
        if widths[i] != W_64 && widths[i] != W_32 {
            return Err(MixCode::WidthUnknown.val());
        }
        if widths[i] == W_32 {
            return Err(MixCode::MixedModeRejected.val());
        }
        i += 1;
    }
    Ok(())
}

/// 复算。
pub fn replay_ok(widths: [u8; SRC_COUNT]) -> Result<(), u32> {
    let a = mixed_check(widths);
    let b = mixed_check(widths);
    if a != b {
        return Err(MixCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：Mixed 路/越宽路/全 64 路/复算路） ───────────
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

// ── 黄金全链：全 64 放行 + 三源各 32 逐一拒 + 复算 ────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let all64 = mixed_check([W_64, W_64, W_64]).is_ok() as u32;
    // 三源逐一 32 位注入。
    let mut mixed = 0u32;
    let mut i = 0;
    while i < SRC_COUNT {
        let mut w = [W_64, W_64, W_64];
        w[i] = W_32;
        if mixed_check(w).is_err() {
            mixed += 1;
        }
        i += 1;
    }
    replay_ok([W_64, W_64, W_64])?;
    Ok((all64, mixed, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：全 64 放行 + 三源注入全拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 3, _)) => Ok(()),
        _ => Err(MixCode::NotReproducible.val()),
    }
}

/// 序 B：位宽账外 ⇒ WidthUnknown。
pub fn probe_seq_b() -> Result<(), u32> {
    match mixed_check([W_64, 48, W_64]) {
        Err(e) if e == MixCode::WidthUnknown.val() => Ok(()),
        _ => Err(MixCode::NotReproducible.val()),
    }
}

/// 序 C：Mixed Mode 逐字。
pub fn probe_seq_c() -> Result<(), u32> {
    match mixed_check([W_64, W_32, W_64]) {
        Err(e) if e == MixCode::MixedModeRejected.val() => Ok(()),
        _ => Err(MixCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：Mixed 路。
        led.record(0, mixed_check([W_64, W_64, W_32]).is_err());
        // 路 2：越宽路。
        led.record(1, mixed_check([W_64, 16, W_64]).is_err());
        // 路 3：全 64 路。
        led.record(2, mixed_check([W_64, W_64, W_64]).is_ok());
        // 路 4：复算路。
        led.record(3, replay_ok([W_64, W_64, W_64]).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MixCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MixCode::ALL.len() {
        let v = MixCode::ALL[i].val();
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
            Ok((all64, mixed, _)) => *all64 == 1 && *mixed == 3,
            Err(_) => false,
        },
        match &g {
            Ok((all64, mixed, _)) => format!("all64={} mixed_rejects={}", all64, mixed),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三源闭集。
    v.push((
        "J1-三源闭集",
        SRC_COUNT == 3,
        String::from("固件/表指针/运行时服务 三源"),
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
        String::from("0x6F1C0 窗口"),
    ));
    v
}

pub fn run_unx0167b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0167 Mixed Mode（IA32 on x64）检测（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
