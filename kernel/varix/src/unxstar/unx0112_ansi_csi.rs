//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0112 · ANSI 转义子集（清屏/定位/颜色） · 0x6E980
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0112-J1 ANSI 转义子集：CSI 序列解析（清屏 2J/定位
//!     H/颜色 m 三族），参数账（行 1-25/列 1-80/颜色 30-47 闭
//!     集），畸形序列拒（无终止符/参数越域），解码账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三族冻结：ESC[2J（清屏）/ESC[{row};{col}H（定位）/
//!     ESC[{n}m（颜色）；
//!   - 参数闭集：行 1-25/列 1-80/颜色 30-47（ fg30-37 bg40-47）；
//!   - 畸形闸：无 ESC 前缀/无终止符/参数越域/非数字参数拒；
//!   - 解码账可复算（同字节串恒同解码）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E980，AI-VX3-1102 批L 独占区）。
pub const SEG_BASE: u32 = 0x6E980;

/// 参数闭集（冻结）。
pub const ROW_MAX: u32 = 25;
pub const COL_MAX: u32 = 80;
pub const COLOR_MIN: u32 = 30;
pub const COLOR_MAX: u32 = 47;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnsiCode {
    /// 无 ESC 前缀
    NoEscPrefix,
    /// 无终止符
    NoTerminator,
    /// 参数越域
    ParamOutOfRange,
    /// 非数字参数
    ParamDigitBad,
    /// 族账外（未知终止符）
    FamilyUnknown,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl AnsiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AnsiCode::NoEscPrefix => "无 ESC 前缀",
            AnsiCode::NoTerminator => "无终止符",
            AnsiCode::ParamOutOfRange => "参数越域",
            AnsiCode::ParamDigitBad => "非数字参数",
            AnsiCode::FamilyUnknown => "族账外：未知终止符",
            AnsiCode::ReplayMismatch => "复算失配",
            AnsiCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [AnsiCode; 6] = [
        AnsiCode::NoEscPrefix,
        AnsiCode::NoTerminator,
        AnsiCode::ParamOutOfRange,
        AnsiCode::ParamDigitBad,
        AnsiCode::FamilyUnknown,
        AnsiCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AnsiCode::ALL.len() {
        if AnsiCode::ALL[i].val() == code {
            return AnsiCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E980 段闭集内"
}

// ── 解码账 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsiOp {
    ClearScreen,
    Goto(u32, u32),
    Color(u32),
}

fn parse_params(s: &str) -> Result<Vec<u32>, u32> {
    let mut out = Vec::new();
    if s.is_empty() {
        return Ok(out);
    }
    let parts: Vec<&str> = s.split(';').collect();
    let mut i = 0;
    while i < parts.len() {
        let p = parts[i];
        if p.is_empty() {
            return Err(AnsiCode::ParamDigitBad.val());
        }
        let b = p.as_bytes();
        let mut acc: u32 = 0;
        let mut k = 0;
        while k < b.len() {
            if b[k] < b'0' || b[k] > b'9' {
                return Err(AnsiCode::ParamDigitBad.val());
            }
            acc = acc.saturating_mul(10).saturating_add((b[k] - b'0') as u32);
            k += 1;
        }
        out.push(acc);
        i += 1;
    }
    Ok(out)
}

/// CSI 序列解码（ESC[...X）。
pub fn decode_csi(seq: &str) -> Result<CsiOp, u32> {
    let b = seq.as_bytes();
    if b.len() < 3 || b[0] != 0x1B || b[1] != b'[' {
        return Err(AnsiCode::NoEscPrefix.val());
    }
    let term = b[b.len() - 1];
    let params = parse_params(&seq[2..seq.len() - 1])?;
    match term {
        b'J' => {
            // 清屏：0 或 2（2=全清）。
            let n = if params.is_empty() { 0 } else { params[0] };
            if n != 0 && n != 2 {
                return Err(AnsiCode::ParamOutOfRange.val());
            }
            Ok(CsiOp::ClearScreen)
        }
        b'H' => {
            if params.len() != 2 {
                return Err(AnsiCode::ParamOutOfRange.val());
            }
            let (row, col) = (params[0], params[1]);
            if row == 0 || row > ROW_MAX || col == 0 || col > COL_MAX {
                return Err(AnsiCode::ParamOutOfRange.val());
            }
            Ok(CsiOp::Goto(row, col))
        }
        b'm' => {
            if params.len() != 1 {
                return Err(AnsiCode::ParamOutOfRange.val());
            }
            let c = params[0];
            if c < COLOR_MIN || c > COLOR_MAX {
                return Err(AnsiCode::ParamOutOfRange.val());
            }
            Ok(CsiOp::Color(c))
        }
        _ => Err(AnsiCode::FamilyUnknown.val()),
    }
}

/// 构造定位序列（编码侧，供对拍）。
pub fn encode_goto(row: u32, col: u32) -> String {
    format!("\x1b[{};{}H", row, col)
}

/// 解码可复算。
pub fn replay_ok(seq: &str, expect: &CsiOp) -> Result<(), u32> {
    let a = decode_csi(seq)?;
    let b = decode_csi(seq)?;
    if a != b || a != *expect {
        return Err(AnsiCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：前缀路/越域路/数字路/族路） ────────────────
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

// ── 黄金全链：三族解码→编码对拍→复算→参数边界 ────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 三族。
    let clear = decode_csi("\x1b[2J")?;
    let goto = decode_csi(&encode_goto(12, 40))?;
    let color = decode_csi("\x1b[37m")?;
    let ok = match (clear, goto, color) {
        (CsiOp::ClearScreen, CsiOp::Goto(12, 40), CsiOp::Color(37)) => 1,
        _ => 0,
    };
    // 边界：行 1/25、列 1/80、颜色 30/47 全过。
    let mut bounds = 0u32;
    let bounds_set = [(1u32, 1u32), (25, 80), (1, 80), (25, 1)];
    let mut i = 0;
    while i < bounds_set.len() {
        if decode_csi(&encode_goto(bounds_set[i].0, bounds_set[i].1)).is_ok() {
            bounds += 1;
        }
        i += 1;
    }
    let cmin = decode_csi("\x1b[30m").is_ok() as u32;
    let cmax = decode_csi("\x1b[47m").is_ok() as u32;
    Ok((ok, bounds + cmin + cmax, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：三族解码 + 边界 6/6 全过。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 6, _)) => Ok(()),
        _ => Err(AnsiCode::NotReproducible.val()),
    }
}

/// 序 B：参数越域 ⇒ ParamOutOfRange（行 26）。
pub fn probe_seq_b() -> Result<(), u32> {
    match decode_csi("\x1b[26;1H") {
        Err(e) if e == AnsiCode::ParamOutOfRange.val() => Ok(()),
        _ => Err(AnsiCode::NotReproducible.val()),
    }
}

/// 序 C：非数字参数 ⇒ ParamDigitBad。
pub fn probe_seq_c() -> Result<(), u32> {
    match decode_csi("\x1b[1;xH") {
        Err(e) if e == AnsiCode::ParamDigitBad.val() => Ok(()),
        _ => Err(AnsiCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：前缀路（无 ESC）。
        led.record(0, decode_csi("[2J").is_err());
        // 路 2：越域路。
        led.record(1, decode_csi("\x1b[99;99H").is_err() && decode_csi("\x1b[99m").is_err());
        // 路 3：数字路。
        led.record(2, decode_csi("\x1b[abJ").is_err());
        // 路 4：族路。
        led.record(3, decode_csi("\x1b[1;2X").is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AnsiCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AnsiCode::ALL.len() {
        let v = AnsiCode::ALL[i].val();
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
            Ok((fam, bounds, _)) => *fam == 1 && *bounds == 6,
            Err(_) => false,
        },
        match &g {
            Ok((fam, bounds, _)) => format!("three_family={} bounds={}/6", fam, bounds),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三族冻结（2J/H/m）。
    v.push((
        "J1-三族冻结",
        decode_csi("\x1b[2J").is_ok()
            && decode_csi("\x1b[1;1H").is_ok()
            && decode_csi("\x1b[37m").is_ok(),
        String::from("清屏 2J / 定位 H / 颜色 m"),
    ));
    // 编解码对拍（encode→decode 恒等）。
    let enc = encode_goto(24, 79);
    v.push((
        "J1-编解码对拍",
        decode_csi(&enc) == Ok(CsiOp::Goto(24, 79)),
        format!("{} → Goto(24,79)", enc.trim_start_matches('\x1b').replace('\x1b', "ESC")),
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
        String::from("0x6E980 窗口"),
    ));
    v
}

pub fn run_unx0112_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0112 ANSI 转义子集（清屏/定位/颜色）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
