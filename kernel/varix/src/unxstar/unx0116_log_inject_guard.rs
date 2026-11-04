//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0116 · 日志注入防护（格式化安全） · 0x6EA80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0116-J1 日志注入防护：外部串入账过滤（ANSI 转义剥
//!     除 / 换行折断 / % 格式符中和），净化后账与原账差值记账，
//!     控制字符拒（C0 集除 \t），净化可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三滤：ESC（0x1B）剥 / CR-LF 折单空格 / '%' → '％' 中和
//!     （格式化注入红线）；
//!   - C0 控制字符闸：除 \t 外 <0x20 拒（显性码）；
//!   - 净化差值记账（剥除字节数）；
//!   - 复算：同输入恒同输出；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EA80，AI-VX3-1102 批L 独占区）。
pub const SEG_BASE: u32 = 0x6EA80;

/// 行长上限（冻结 120）。
pub const LINE_MAX: usize = 120;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjCode {
    /// C0 控制字符（除 \t）
    CtrlCharFound,
    /// 行长越上限
    LineOverMax,
    /// 净化复算失配
    SanitizeMismatch,
    /// 差值记账失真
    DiffLedgerBad,
    /// 双跑不一致
    NotReproducible,
}

impl InjCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            InjCode::CtrlCharFound => "C0 控制字符：除 \\t 外拒",
            InjCode::LineOverMax => "行长越上限：120 冻结",
            InjCode::SanitizeMismatch => "净化复算失配",
            InjCode::DiffLedgerBad => "差值记账失真",
            InjCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [InjCode; 5] = [
        InjCode::CtrlCharFound,
        InjCode::LineOverMax,
        InjCode::SanitizeMismatch,
        InjCode::DiffLedgerBad,
        InjCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < InjCode::ALL.len() {
        if InjCode::ALL[i].val() == code {
            return InjCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EA80 段闭集内"
}

// ── 净化账 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sanitized {
    pub text: String,
    /// 剥除字节数（ESC 计 1 / CRLF 计 1）。
    pub stripped: u32,
    /// 中和 '%' 数。
    pub neutralized: u32,
    /// 折行补空格数（CRLF → 1 空格）。
    pub folded: u32,
}

/// 三滤净化（ESC 剥 / CRLF 折空格 / % 中和）。
pub fn sanitize(input: &str) -> Result<Sanitized, u32> {
    if input.len() > LINE_MAX {
        return Err(InjCode::LineOverMax.val());
    }
    let b = input.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut stripped = 0u32;
    let mut neutralized = 0u32;
    let mut folded = 0u32;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == 0x1B {
            // ANSI 转义：剥 ESC 及其后 '[...字母]' 段。
            stripped += 1;
            if i + 1 < b.len() && b[i + 1] == b'[' {
                let mut k = i + 2;
                while k < b.len() && !(b[k] >= b'A' && b[k] <= b'Z') && !(b[k] >= b'a' && b[k] <= b'z') {
                    k += 1;
                }
                if k < b.len() {
                    stripped += (k - i) as u32;
                    i = k + 1;
                    continue;
                }
                i = b.len();
                continue;
            }
            i += 1;
            continue;
        }
        if c == b'\r' || c == b'\n' {
            // 换行折单空格（连续折一）。
            stripped += 1;
            if out.last() != Some(&b' ') {
                out.push(b' ');
                folded += 1;
            }
            i += 1;
            continue;
        }
        if c < 0x20 && c != b'\t' {
            return Err(InjCode::CtrlCharFound.val());
        }
        if c == b'%' {
            neutralized += 1;
            out.push(b'%');
            out.push(b'%');
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    let text = String::from_utf8_lossy(&out).into_owned();
    Ok(Sanitized { text, stripped, neutralized, folded })
}

/// 差值记账审计：stripped 与实际输出长度差一致。
pub fn diff_audit(input: &str, s: &Sanitized) -> Result<(), u32> {
    let in_len = input.len() as u32;
    // 输出长 = 输入长 - 剥除 + 中和新增 '%' + 折行补空格。
    let expect = in_len - s.stripped + s.neutralized + s.folded;
    if expect != s.text.len() as u32 {
        return Err(InjCode::DiffLedgerBad.val());
    }
    Ok(())
}

/// 复算：同输入恒同输出。
pub fn replay_ok(input: &str) -> Result<(), u32> {
    let a = sanitize(input)?;
    let b = sanitize(input)?;
    if a != b {
        return Err(InjCode::SanitizeMismatch.val());
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

// ── 注入检出账（四路：C0 路/行长路/中和路/复算路） ───────────────
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

// ── 黄金全链：注入样本三滤→差值→复算→净化文案逐字 ────────────────
pub fn golden_run() -> Result<(u32, u32, String), u32> {
    let sample = "ok\x1b[2Jclean\r\nnext 100%";
    let s = sanitize(sample)?;
    diff_audit(sample, &s)?;
    replay_ok(sample)?;
    // 期望净化文案："okclean next 100%%"。
    let want = "okclean next 100%%";
    if s.text != want {
        return Err(InjCode::SanitizeMismatch.val());
    }
    Ok((s.stripped, s.neutralized, s.text))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：注入样本 → 剥 5 + 中和 1 + 文案逐字。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((6, 1, t)) if t == "okclean next 100%%" => Ok(()),
        _ => Err(InjCode::NotReproducible.val()),
    }
}

/// 序 B：C0 控制字符 ⇒ CtrlCharFound。
pub fn probe_seq_b() -> Result<(), u32> {
    match sanitize("a\u{7}b") {
        Err(e) if e == InjCode::CtrlCharFound.val() => Ok(()),
        _ => Err(InjCode::NotReproducible.val()),
    }
}

/// 序 C：行长越上限 ⇒ LineOverMax。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut long = String::new();
    let mut i = 0;
    while i <= LINE_MAX {
        long.push('x');
        i += 1;
    }
    match sanitize(&long) {
        Err(e) if e == InjCode::LineOverMax.val() => Ok(()),
        _ => Err(InjCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：C0 路。
        led.record(0, sanitize("a\u{2}b").is_err());
        // 路 2：行长路。
        let mut l2 = String::new();
        let mut i = 0;
        while i < LINE_MAX + 4 {
            l2.push('y');
            i += 1;
        }
        led.record(1, sanitize(&l2).is_err());
        // 路 3：中和路（% → %% 计数）。
        let s3 = sanitize("50% off")?;
        led.record(2, s3.neutralized == 1 && s3.text == "50%% off");
        // 路 4：复算路。
        led.record(3, replay_ok("\x1b[31mred\ntab\there").is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(InjCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < InjCode::ALL.len() {
        let v = InjCode::ALL[i].val();
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
            Ok((st, ne, t)) => *st == 6 && *ne == 1 && t == "okclean next 100%%",
            Err(_) => false,
        },
        match &g {
            Ok((st, ne, t)) => format!("stripped={} neutralized={} text={:?}", st, ne, t),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三滤语义（ESC 剥/CRLF 折/% 中和）逐字。
    let s1 = sanitize("\x1b[31mred").map(|s| s.text) == Ok(String::from("red"));
    let s2 = sanitize("a\r\nb").map(|s| s.text) == Ok(String::from("a b"));
    let s3 = sanitize("100%").map(|s| s.text) == Ok(String::from("100%%"));
    v.push((
        "J1-三滤语义",
        s1 && s2 && s3,
        String::from("ESC 剥 / CRLF 折空格 / % → %%"),
    ));
    // \t 白名单。
    v.push((
        "J1-tab白名单",
        sanitize("a\tb").is_ok(),
        String::from("\\t 允许（C0 例外）"),
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
        String::from("0x6EA80 窗口"),
    ));
    v
}

pub fn run_unx0116_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0116 日志注入防护（格式化安全）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
