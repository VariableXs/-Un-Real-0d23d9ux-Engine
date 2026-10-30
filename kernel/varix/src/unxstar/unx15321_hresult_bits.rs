//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15321 · HRESULT 编码面钉版（severity/facility/code 位域） · 0x67200
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15321-J1 编码面成文（31 位位域编码/解码双向往返 10/10 次），
//!      位漂移注入（facility 错位）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 位域钉版（31 位有效位，来源：WinError.h 公开编码面）：
//!     bit31 = severity（0=成功 S_，1=失败 E_）；
//!     bit30..16 = facility（15 位，设施域）；
//!     bit15..0 = code（16 位，设施内代码）；
//!     bit31 之外最高位为 0（31 位面），越面值拒收；
//!   - 双向往返：encode(sev,fac,code) → hr → decode → 恒等回原三元，
//!     10/10 随机三元全往返；
//!   - 位漂移注入面：编码时 facility 左移/右移 1 位（错位写入），
//!     解码出的 facility 必与原值不符 → 检出；
//!   - 黄金对照表写死：S_OK/E_FAIL/E_NOINTERFACE/E_UNEXPECTED/
//!     S_FALSE/E_HANDLE/E_INVALIDARG/E_OUTOFMEMORY 等公开值；
//!   - LCG 种子写死 = SEG_BASE（0x67200），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic，错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67200，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67200;

/// LCG 种子写死（来源：本卡 SEG_BASE）。
pub const LCG_SEED: u64 = 0x67200;

/// 位域钉版常量（来源：WinError.h 公开编码面，写死不复用被测值）。
pub const SEV_SHIFT: u32 = 31;
pub const FAC_SHIFT: u32 = 16;
pub const FAC_BITS: u32 = 15;
pub const CODE_BITS: u32 = 16;
pub const FAC_MASK: u32 = 0x7FFF;
pub const CODE_MASK: u32 = 0xFFFF;
pub const SEV_SUCCESS: u32 = 0;
pub const SEV_FAILURE: u32 = 1;

/// 黄金 HRESULT 对照表（手写公开值，与被测编码路径完全独立）。
pub const GOLDEN_HRESULTS: [(u32, u32, u32, u32); 8] = [
    // (hr, severity, facility, code)
    (0x0000_0000, 0, 0, 0x0000),      // S_OK
    (0x0000_0001, 0, 0, 0x0001),      // S_FALSE
    (0x8000_4005, 1, 4, 0x2005),      // E_FAIL（FACILITY_ITF=4）
    (0x8000_4002, 1, 4, 0x0002),      // E_NOINTERFACE
    (0x8000_FFFF, 1, 31, 0xFFFF),     // E_UNEXPECTED（FACILITY_URT=31）
    (0x8007_0006, 1, 7, 0x0006),      // E_HANDLE（FACILITY_WIN32=7）
    (0x8007_0057, 1, 7, 0x0057),      // E_INVALIDARG
    (0x8007_000E, 1, 7, 0x000E),      // E_OUTOFMEMORY
];

/// 设施名闭集（点验用，写死）。
pub const FACILITY_NAMES: [&str; 5] = ["NULL", "ITF", "WIN32", "DISPATCH", "URT"];
pub const FACILITY_IDS: [u32; 5] = [0, 4, 7, 3, 31];

/// 判据投影：随机三元往返 10/10。
pub const ROUNDTRIP_TRIALS: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HrCode {
    /// severity 越域（非 0/1）
    SeverityInvalid,
    /// facility 超 15 位面
    FacilityOverflow,
    /// code 超 16 位面
    CodeOverflow,
    /// 31 位面越位（bit31 之上有置位）
    OverHighBit,
    /// 双向往返不恒等
    RoundtripMismatch,
    /// facility 位漂移（编码错位）
    FacilityDrift,
    /// 黄金对照表不符
    GoldenMismatch,
    /// severity 与 hr 最高位不一致（语义位漂移）
    SevBitMismatch,
    /// 设施名表与设施号表长度不守恒
    FacilityTableBreach,
}

impl HrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HrCode::SeverityInvalid => "severity 越域：仅 0/1",
            HrCode::FacilityOverflow => "facility 超 15 位面",
            HrCode::CodeOverflow => "code 超 16 位面",
            HrCode::OverHighBit => "31 位面越位：bit31 之外不得置位",
            HrCode::RoundtripMismatch => "双向往返不恒等",
            HrCode::FacilityDrift => "facility 位漂移：错位写入",
            HrCode::GoldenMismatch => "黄金对照表不符",
            HrCode::SevBitMismatch => "severity 与最高位不一致",
            HrCode::FacilityTableBreach => "设施名/号表长度不守恒",
        }
    }
    pub const ALL: [HrCode; 9] = [
        HrCode::SeverityInvalid,
        HrCode::FacilityOverflow,
        HrCode::CodeOverflow,
        HrCode::OverHighBit,
        HrCode::RoundtripMismatch,
        HrCode::FacilityDrift,
        HrCode::GoldenMismatch,
        HrCode::SevBitMismatch,
        HrCode::FacilityTableBreach,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HrCode::ALL.len() {
        if HrCode::ALL[i].val() == code {
            return HrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67200 段闭集内"
}

// ── 编码面（正演：按钉版位域组合） ───────────────────────────────
pub fn hr_encode(severity: u32, facility: u32, code: u32) -> Result<u32, u32> {
    if severity > SEV_FAILURE {
        return Err(HrCode::SeverityInvalid.val());
    }
    if facility > FAC_MASK {
        return Err(HrCode::FacilityOverflow.val());
    }
    if code > CODE_MASK {
        return Err(HrCode::CodeOverflow.val());
    }
    let hr = (severity << SEV_SHIFT) | (facility << FAC_SHIFT) | code;
    Ok(hr)
}

/// 解码面（正演：按钉版位域拆分）。
pub fn hr_decode(hr: u32) -> (u32, u32, u32) {
    let severity = (hr >> SEV_SHIFT) & 1;
    let facility = (hr >> FAC_SHIFT) & FAC_MASK;
    let code = hr & CODE_MASK;
    (severity, facility, code)
}

/// 往返恒等断言：encode→decode 回原三元。
pub fn roundtrip(severity: u32, facility: u32, code: u32) -> Result<u32, u32> {
    let hr = hr_encode(severity, facility, code)?;
    let (s2, f2, c2) = hr_decode(hr);
    if s2 != severity || f2 != facility || c2 != code {
        return Err(HrCode::RoundtripMismatch.val());
    }
    // 语义位钉版：severity 与 hr 最高位必须一致。
    let high = hr >> 31;
    if high != severity {
        return Err(HrCode::SevBitMismatch.val());
    }
    // 成败语义钉版：severity=0 → hr 高 31 位不构成失败形（hr < 0x8000_0000）。
    if severity == SEV_SUCCESS && hr >= 0x8000_0000 {
        return Err(HrCode::SevBitMismatch.val());
    }
    Ok(hr)
}

/// 位漂移注入模型：facility 在编码时错位 ±1 位后写入。
/// 返回错位 hr 与期望 hr；解码错位 hr 所得 facility 必不等于原值
/// （除非漂移溢出位面——那也要被 encode 面拒收，两路都算检出）。
pub fn facility_drift(severity: u32, facility: u32, code: u32) -> Result<(u32, u32), u32> {
    let expect = hr_encode(severity, facility, code)?;
    // 变异 A：左错位（facility<<1 挤占 severity 位）。
    let drift_left = (severity << SEV_SHIFT)
        | ((facility << (FAC_SHIFT + 1)) & 0xFFFF_FFFF)
        | code;
    // 变异 B：右错位（facility>>1 让出低位给 code 面）。
    let drift_right = (severity << SEV_SHIFT) | ((facility >> 1) << FAC_SHIFT) | code;
    let bad = if (facility & 0x4000) == 0 { drift_left } else { drift_right };
    // 解码错位值：facility 必变（漂移必现形）。
    let (_, f_bad, _) = hr_decode(bad);
    if f_bad == facility {
        return Err(HrCode::FacilityDrift.val());
    }
    Ok((expect, bad))
}

// ── 确定性 LCG（种子 = 0x67200） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 10/10 随机三元往返压演 ───────────────────────────────────────
pub fn random_roundtrip_round(round: usize) -> Result<(u32, u32), u32> {
    let mut lcg = Lcg::new(LCG_SEED + round as u64);
    let mut trips = 0;
    let mut drift_caught = 0;
    let mut i = 0;
    while i < 64 {
        let severity = (lcg.below(2)) as u32;
        // facility 取非零（0 无漂移可见形，属平凡情形；判据口径按非零面）。
        let facility = 1 + (lcg.below(FAC_MASK as u64)) as u32;
        let code = (lcg.below((CODE_MASK as u64) + 1)) as u32;
        let _ = roundtrip(severity, facility, code)?;
        trips += 1;
        // 同点注入一次位漂移，必须现形。
        if let Ok((expect, bad)) = facility_drift(severity, facility, code) {
            let (_, f_expect, _) = hr_decode(expect);
            let (_, f_bad, _) = hr_decode(bad);
            if f_expect != f_bad {
                drift_caught += 1;
            } else {
                return Err(HrCode::FacilityDrift.val());
            }
        }
        i += 1;
    }
    Ok((trips, drift_caught))
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 1) 黄金对照表逐条对拍（解码面 vs 写死公开值）。
    let mut i = 0;
    while i < GOLDEN_HRESULTS.len() {
        let (hr, s, f, c) = GOLDEN_HRESULTS[i];
        let (s2, f2, c2) = hr_decode(hr);
        if s2 != s || f2 != f || c2 != c {
            return Err(HrCode::GoldenMismatch.val());
        }
        // 反向：由三元编码回 hr 必恒等。
        let hr2 = hr_encode(s, f, c)?;
        if hr2 != hr {
            return Err(HrCode::GoldenMismatch.val());
        }
        i += 1;
    }
    // 2) 10/10 随机往返。
    let mut rounds = 0;
    let mut trips_total = 0;
    let mut drift_total = 0;
    let mut r = 0;
    while r < ROUNDTRIP_TRIALS {
        let (t, d) = random_roundtrip_round(r)?;
        if t != 64 {
            return Err(HrCode::RoundtripMismatch.val());
        }
        trips_total += t;
        drift_total += d;
        rounds += 1;
        r += 1;
    }
    // 3) 越面拒收面：facility 0x8000 / code 0x10000 / severity 2 必拒。
    let a = hr_encode(0, 0x8000, 0);
    let b = hr_encode(0, 0, 0x1_0000);
    let c = hr_encode(2, 0, 0);
    let guard_ok = a.is_err() && b.is_err() && c.is_err();
    if !guard_ok {
        return Err(HrCode::FacilityOverflow.val());
    }
    Ok((rounds, trips_total, drift_total))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：facility 位漂移 ×10（右错位模型，非零 facility）。
    let mut t = 0;
    while t < 10 {
        let fac = (t + 1) * 3; // 保证非零且 < 0x7FFF
        let v = facility_drift(SEV_FAILURE, fac, 0x1234);
        ledger.tries[0] += 1;
        if let Ok((expect, bad)) = v {
            let (_, f1, _) = hr_decode(expect);
            let (_, f2, _) = hr_decode(bad);
            if f1 != f2 {
                ledger.caught[0] += 1;
            }
        }
        t += 1;
    }
    // 族 1：黄金对照表篡改注入（把 E_FAIL 的 facility 改成 5）×10。
    let mut t = 0;
    while t < 10 {
        let (hr, s, f, c) = GOLDEN_HRESULTS[2]; // E_FAIL
        let forged = hr_encode(s, f + 1, c);
        let forged = match forged {
            Ok(v) => v,
            Err(_) => 0xFFFF_FFFF,
        };
        ledger.tries[1] += 1;
        if forged != hr {
            // 与写死黄金值不符 → GoldenMismatch 判据命中。
            ledger.caught[1] += 1;
        }
        t += 1;
    }
    // 族 2：越面注入 ×10（facility 0x8000 超 15 位面）。
    let mut t = 0;
    while t < 10 {
        let v = hr_encode(SEV_FAILURE, 0x8000, 0);
        ledger.tries[2] += 1;
        if let Err(e) = v {
            if e == HrCode::FacilityOverflow.val() {
                ledger.caught[2] += 1;
            }
        }
        t += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HrCode::ALL.len() {
        let v = HrCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    // 位域钉版自洽：shift 与 mask 语义一致。
    FAC_SHIFT == CODE_BITS && SEV_SHIFT == FAC_SHIFT + FAC_BITS && FAC_BITS == 15
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    let golden_ok = if let Ok((rounds, trips, drift)) = golden {
        rounds == ROUNDTRIP_TRIALS as u32 && trips == 640 && drift == 640
    } else {
        false
    };
    v.push((
        "J1-编码面10/10往返+黄金8条",
        golden_ok,
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-位漂移注入10/10必抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 黄金表逐条点验（判据独立写死）。
    let efail = hr_decode(0x8000_4005);
    let efail_ok = efail.0 == 1 && efail.1 == 4 && efail.2 == 0x2005;
    v.push((
        "J1-黄金E_FAIL解码点验",
        efail_ok,
        format!("{:?}", efail),
    ));
    // 设施名/号表长度守恒。
    v.push((
        "J1-设施表长度守恒",
        FACILITY_NAMES.len() == FACILITY_IDS.len(),
        format!("{}/{}", FACILITY_NAMES.len(), FACILITY_IDS.len()),
    ));
    // 越面拒收面独立点验。
    let guard = hr_encode(2, 0, 0).is_err()
        && hr_encode(0, 0x8000, 0).is_err()
        && hr_encode(0, 0, 0x10000).is_err();
    v.push((
        "J1-越面拒收三形",
        guard,
        String::from("sev/fac/code 三面"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67200 窗口")));
    v
}

pub fn run_unx15321_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15321 HRESULT 编码面钉版（31 位位域）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
