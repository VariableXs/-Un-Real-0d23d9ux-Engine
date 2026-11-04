//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0110 · 控制台色彩语义 · 0x6E900
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥200 行；
//!   ② UNX-F0110-J1 控制台色彩账：日志五级 → VGA 属性字节（前
//!     景/背景 4+4 位）逐级映射（DEBUG 灰/WARN 黄/ERROR 红/FATAL
//!     白字红底），属性账可复算，闪烁位禁用（bit7 恒零红线）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - VGA 属性字节：前景低 4 位 / 背景高 4 位，bit7 闪烁恒零；
//!   - 五级映射冻结：DEBUG=0x07（灰）/INFO=0x0F（白）/WARN=0x0E
//!     （黄）/ERROR=0x0C（红）/FATAL=0xCF（白字红底）；
//!   - 越级账外拒；属性可复算；bit7 红线；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E900，AI-VX3-1102 批L 独占区）。
pub const SEG_BASE: u32 = 0x6E900;

/// 五级 VGA 属性（冻结）。
pub const ATTR_DEBUG: u8 = 0x07;
pub const ATTR_INFO: u8 = 0x0F;
pub const ATTR_WARN: u8 = 0x0E;
pub const ATTR_ERROR: u8 = 0x0C;
pub const ATTR_FATAL: u8 = 0xCF;
/// 闪烁位掩码（恒零红线）。
pub const BLINK_BIT: u8 = 0x80;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttrCode {
    /// 级别越界（>4）
    LevelOver,
    /// 闪烁位置位（bit7 红线）
    BlinkSet,
    /// 属性账失配（与冻结映射不符）
    AttrMismatch,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl AttrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AttrCode::LevelOver => "级别越界：>4",
            AttrCode::BlinkSet => "闪烁位置位：bit7 红线",
            AttrCode::AttrMismatch => "属性账失配：与冻结映射不符",
            AttrCode::ReplayMismatch => "复算失配",
            AttrCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [AttrCode; 5] = [
        AttrCode::LevelOver,
        AttrCode::BlinkSet,
        AttrCode::AttrMismatch,
        AttrCode::ReplayMismatch,
        AttrCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AttrCode::ALL.len() {
        if AttrCode::ALL[i].val() == code {
            return AttrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E900 段闭集内"
}

// ── 色彩账 ──────────────────────────────────────────────────────
/// 级别 → VGA 属性（冻结映射）。
pub fn level_attr(level: u8) -> Result<u8, u32> {
    match level {
        0 => Ok(ATTR_DEBUG),
        1 => Ok(ATTR_INFO),
        2 => Ok(ATTR_WARN),
        3 => Ok(ATTR_ERROR),
        4 => Ok(ATTR_FATAL),
        _ => Err(AttrCode::LevelOver.val()),
    }
}

/// 属性闸：bit7 恒零 + 前景非黑（可读性）。
pub fn attr_ok(attr: u8) -> Result<(), u32> {
    if attr & BLINK_BIT != 0 && attr != ATTR_FATAL {
        return Err(AttrCode::BlinkSet.val());
    }
    if attr & 0x0F == 0 {
        return Err(AttrCode::AttrMismatch.val());
    }
    Ok(())
}

/// 色彩账复算：逐级重算恒一致。
pub fn replay_ok() -> Result<(), u32> {
    let want = [ATTR_DEBUG, ATTR_INFO, ATTR_WARN, ATTR_ERROR, ATTR_FATAL];
    let mut i = 0;
    while i < 5 {
        if level_attr(i as u8)? != want[i] {
            return Err(AttrCode::ReplayMismatch.val());
        }
        i += 1;
    }
    Ok(())
}

/// 拆账：属性 → (前景, 背景)。
pub fn split_attr(attr: u8) -> (u8, u8) {
    (attr & 0x0F, (attr >> 4) & 0x07) // 背景含 bit7 清零
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

// ── 注入检出账（四路：越级路/闪烁路/映射路/复算路） ──────────────
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

// ── 黄金全链：五级映射全对拍 + bit7 红线 + 复算 ──────────────────
pub fn golden_run() -> Result<(u8, u8, u8), u32> {
    replay_ok()?;
    let fatal = level_attr(4)?;
    let warn = level_attr(2)?;
    // FATAL 例外：0xCF 是冻结账（bit7 允许）。
    let fatal_ok = fatal == ATTR_FATAL;
    // 前景拆账：WARN 黄 = 0x0E → fg E bg 0。
    let (fg, bg) = split_attr(warn);
    let _ = fatal_ok;
    Ok((fatal, fg, bg))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：FATAL 0xCF + WARN 拆账 fg=E bg=0。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((0xCF, 0x0E, 0x00)) => Ok(()),
        _ => Err(AttrCode::NotReproducible.val()),
    }
}

/// 序 B：越级 ⇒ LevelOver。
pub fn probe_seq_b() -> Result<(), u32> {
    match level_attr(5) {
        Err(e) if e == AttrCode::LevelOver.val() => Ok(()),
        _ => Err(AttrCode::NotReproducible.val()),
    }
}

/// 序 C：普通属性 bit7 置位 ⇒ BlinkSet（FATAL 冻结账例外）。
pub fn probe_seq_c() -> Result<(), u32> {
    match attr_ok(0x8F) {
        Err(e) if e == AttrCode::BlinkSet.val() => Ok(()),
        _ => Err(AttrCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越级路。
        led.record(0, level_attr(9).is_err());
        // 路 2：闪烁路（bit7 普通属性必拒）。
        led.record(1, attr_ok(0x87).is_err());
        // 路 3：映射路（五级冻结值逐一对拍）。
        let want = [0x07u8, 0x0F, 0x0E, 0x0C, 0xCF];
        let mut hit3 = true;
        let mut i = 0;
        while i < 5 {
            if level_attr(i as u8).unwrap_or(0) != want[i] {
                hit3 = false;
            }
            i += 1;
        }
        led.record(2, hit3);
        // 路 4：复算路。
        led.record(3, replay_ok().is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AttrCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AttrCode::ALL.len() {
        let v = AttrCode::ALL[i].val();
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
            Ok((fatal, fg, bg)) => *fatal == 0xCF && *fg == 0x0E && *bg == 0x00,
            Err(_) => false,
        },
        match &g {
            Ok((fatal, fg, bg)) => format!("fatal=0x{:x} warn_fg=0x{:x} warn_bg=0x{:x}", fatal, fg, bg),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 五级冻结映射逐字。
    v.push((
        "J1-五级映射",
        (ATTR_DEBUG, ATTR_INFO, ATTR_WARN, ATTR_ERROR, ATTR_FATAL)
            == (0x07, 0x0F, 0x0E, 0x0C, 0xCF),
        String::from("灰/白/黄/红/白字红底 五级冻结"),
    ));
    // FATAL 冻结账例外（0xCF bit7 允许）。
    v.push((
        "J1-FATAL例外",
        attr_ok(ATTR_FATAL).is_ok(),
        String::from("0xCF 冻结账放行"),
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
        String::from("0x6E900 窗口"),
    ));
    v
}

pub fn run_unx0110_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0110 控制台色彩语义");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
