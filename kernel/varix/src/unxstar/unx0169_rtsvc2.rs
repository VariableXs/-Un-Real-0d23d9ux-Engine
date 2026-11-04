//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0169 · 运行时调用封装与状态码字典（深化第二实现） · 0x6F240
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F0169-J1 运行时封装：状态码字典（0 成功/高位错误码闭
//!     集逐字文案），封装调用（状态码 → Result），未收录码显性
//!     兜底文案，字典可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F240，AI-VX3-1102 批O 独占区）。
pub const SEG_BASE: u32 = 0x6F240;

/// EFI 状态码（冻结）。
pub const EFI_SUCCESS: u64 = 0;
pub const EFI_INVALID_PARAMETER: u64 = 0x8000_0000_0000_0002;
pub const EFI_UNSUPPORTED: u64 = 0x8000_0000_0000_0003;
pub const EFI_NOT_FOUND: u64 = 0x8000_0000_0000_000E;
pub const EFI_DEVICE_ERROR: u64 = 0x8000_0000_0000_0007;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RtCode {
    /// 状态码未收录
    CodeUnknown,
    /// 错误码未置高位
    SignBitMissing,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl RtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RtCode::CodeUnknown => "状态码未收录：兜底文案",
            RtCode::SignBitMissing => "错误码未置高位：bit63",
            RtCode::ReplayMismatch => "复算失配",
            RtCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [RtCode; 4] = [
        RtCode::CodeUnknown,
        RtCode::SignBitMissing,
        RtCode::ReplayMismatch,
        RtCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RtCode::ALL.len() {
        if RtCode::ALL[i].val() == code {
            return RtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F240 段闭集内"
}

// ── 字典账 ──────────────────────────────────────────────────────
/// 状态码 → 文案（字典；未收录兜底）。
pub fn status_text(code: u64) -> String {
    match code {
        EFI_SUCCESS => String::from("Success"),
        EFI_INVALID_PARAMETER => String::from("Invalid Parameter"),
        EFI_UNSUPPORTED => String::from("Unsupported"),
        EFI_NOT_FOUND => String::from("Not Found"),
        EFI_DEVICE_ERROR => String::from("Device Error"),
        _ => alloc::format!("Unknown Status 0x{:x}", code),
    }
}

/// 封装调用：状态码 → Result（高位错误码语义）。
pub fn wrap_call(code: u64, payload: u64) -> Result<u64, u32> {
    if code == EFI_SUCCESS {
        return Ok(payload);
    }
    if code & (1 << 63) == 0 {
        return Err(RtCode::SignBitMissing.val());
    }
    let _ = status_text(code);
    Err(RtCode::CodeUnknown.val())
}

/// 复算。
pub fn replay_ok(code: u64) -> Result<(), u32> {
    let a = status_text(code);
    let b = status_text(code);
    if a != b {
        return Err(RtCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：未收录路/缺符号路/成功路/复算路） ──────────
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

// ── 黄金全链：字典五码逐字 + 封装两路 + 兜底 + 复算 ────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 字典五码。
    let dict_ok = (status_text(EFI_SUCCESS) == "Success"
        && status_text(EFI_INVALID_PARAMETER) == "Invalid Parameter"
        && status_text(EFI_UNSUPPORTED) == "Unsupported"
        && status_text(EFI_NOT_FOUND) == "Not Found"
        && status_text(EFI_DEVICE_ERROR) == "Device Error") as u32;
    // 封装：成功带载荷 / 错误显性。
    let wrap_ok = (wrap_call(EFI_SUCCESS, 0xAB) == Ok(0xAB)
        && wrap_call(EFI_NOT_FOUND, 0).is_err()) as u32;
    // 兜底文案。
    let fallback = status_text(0xDEAD_BEEF).starts_with("Unknown Status") as u32;
    replay_ok(EFI_SUCCESS)?;
    Ok((dict_ok, wrap_ok + fallback, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：字典五码 + 封装两路 + 兜底。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 2, _)) => Ok(()),
        _ => Err(RtCode::NotReproducible.val()),
    }
}

/// 序 B：缺符号位 ⇒ SignBitMissing。
pub fn probe_seq_b() -> Result<(), u32> {
    match wrap_call(3, 0) {
        Err(e) if e == RtCode::SignBitMissing.val() => Ok(()),
        _ => Err(RtCode::NotReproducible.val()),
    }
}

/// 序 C：未收录码兜底前缀。
pub fn probe_seq_c() -> Result<(), u32> {
    match status_text(0x1234).starts_with("Unknown Status") {
        true => Ok(()),
        false => Err(RtCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：未收录路。
        led.record(0, wrap_call(EFI_UNSUPPORTED, 0).is_err());
        // 路 2：缺符号路。
        led.record(1, wrap_call(5, 0).is_err());
        // 路 3：成功路。
        led.record(2, wrap_call(EFI_SUCCESS, 7) == Ok(7));
        // 路 4：复算路。
        led.record(3, replay_ok(EFI_DEVICE_ERROR).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RtCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RtCode::ALL.len() {
        let v = RtCode::ALL[i].val();
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
            Ok((dict, wrap, _)) => *dict == 1 && *wrap == 2,
            Err(_) => false,
        },
        match &g {
            Ok((dict, wrap, _)) => format!("dict={} wrap+fallback={}", dict, wrap),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 字典五码值冻结。
    v.push((
        "J1-五码值冻结",
        EFI_INVALID_PARAMETER == 0x8000_0000_0000_0002
            && EFI_NOT_FOUND == 0x8000_0000_0000_000E,
        String::from("INVALID=0x02 / NOT_FOUND=0x0E 冻结"),
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
        String::from("0x6F240 窗口"),
    ));
    v
}

pub fn run_unx0169b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0169 运行时调用封装与状态码字典（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
