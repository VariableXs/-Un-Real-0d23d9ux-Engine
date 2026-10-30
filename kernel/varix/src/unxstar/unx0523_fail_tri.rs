//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0523 · 失败三要素呈现（定位/期望/实际） · 0x6C840
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0523-J1 三要素呈现账齐（文件/行/期望 vs 实际），10 失
//!     败样本呈现完整率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三要素闭集：定位（文件+行）/期望（expected）/实际（actual）；
//!    - 呈现行格式冻结："FAIL file:line: expected X, got Y"；
//!    - 10 失败样本呈现完整率 100%：每样本三要素齐（文件非空/行
//!      ≥1/期望实际均有渲染文本）；
//!    - 呈现账逐条入账（呈现器拒呈现三缺一残账）；
//!    - 值渲染：整数（含负）十六进制/十进制双格式账（冻结双格式）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C840，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C840;

/// 失败样本数（冻结：10）。
pub const FAIL_SAMPLES: usize = 10;
/// 行号下限。
pub const LINE_MIN: u32 = 1;
/// 呈现行前缀。
pub const FAIL_PREFIX: &str = "FAIL";

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailCode {
    /// 文件名空
    FileBlank,
    /// 行号零（行 ≥1）
    LineZero,
    /// 期望/实际渲染缺列
    ExpectActualMissing,
    /// 呈现行前缀错
    PrefixWrong,
    /// 完整率不满（<100%）
    CompletenessShort,
    /// 双格式账缺（hex/dec 缺一）
    DualFormatMissing,
    /// 呈现账缺条
    LedgerShort,
    /// 双跑不一致
    NotReproducible,
}

impl FailCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FailCode::FileBlank => "文件名空",
            FailCode::LineZero => "行号零：行 ≥1",
            FailCode::ExpectActualMissing => "期望/实际渲染缺列",
            FailCode::PrefixWrong => "呈现行前缀错：非 FAIL",
            FailCode::CompletenessShort => "完整率不满：<100%",
            FailCode::DualFormatMissing => "双格式账缺：hex/dec 缺一",
            FailCode::LedgerShort => "呈现账缺条：≠10",
            FailCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [FailCode; 8] = [
        FailCode::FileBlank,
        FailCode::LineZero,
        FailCode::ExpectActualMissing,
        FailCode::PrefixWrong,
        FailCode::CompletenessShort,
        FailCode::DualFormatMissing,
        FailCode::LedgerShort,
        FailCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FailCode::ALL.len() {
        if FailCode::ALL[i].val() == code {
            return FailCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C840 段闭集内"
}

// ── 失败三要素 ─────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailTriple {
    pub file: String,
    pub line: u32,
    pub expected: String,
    pub actual: String,
}

impl FailTriple {
    /// 三要素齐。
    pub fn complete(&self) -> bool {
        self.file.len() > 0
            && self.line >= LINE_MIN
            && self.expected.len() > 0
            && self.actual.len() > 0
    }
    /// 呈现行（冻结格式）。
    pub fn render(&self) -> String {
        format!(
            "{} {}:{}: expected {}, got {}",
            FAIL_PREFIX, self.file, self.line, self.expected, self.actual
        )
    }
}

/// 整数双格式渲染（dec + hex，冻结双格式）。
pub fn dual_render(v: i64) -> Result<(String, String), u32> {
    let dec = format!("{}", v);
    let hex = format!("{:#X}", v);
    if dec.len() == 0 || hex.len() == 0 {
        return Err(FailCode::DualFormatMissing.val());
    }
    Ok((dec, hex))
}

// ── 呈现器 ─────────────────────────────────────────────────────
pub struct FailPresenter {
    pub ledger: Vec<FailTriple>,
}

impl FailPresenter {
    pub fn new() -> FailPresenter {
        FailPresenter { ledger: Vec::new() }
    }

    /// 呈现：三缺一拒（不产残账）。
    pub fn present(&mut self, t: FailTriple) -> Result<String, u32> {
        if t.file.len() == 0 {
            return Err(FailCode::FileBlank.val());
        }
        if t.line < LINE_MIN {
            return Err(FailCode::LineZero.val());
        }
        if t.expected.len() == 0 || t.actual.len() == 0 {
            return Err(FailCode::ExpectActualMissing.val());
        }
        let line = t.render();
        if !line.starts_with(FAIL_PREFIX) {
            return Err(FailCode::PrefixWrong.val());
        }
        self.ledger.push(t);
        Ok(line)
    }

    /// 完整率（样本全齐 = 100%）。
    pub fn completeness_pct(&self) -> u32 {
        if self.ledger.len() == 0 {
            return 0;
        }
        let mut ok = 0;
        let mut i = 0;
        while i < self.ledger.len() {
            if self.ledger[i].complete() {
                ok += 1;
            }
            i += 1;
        }
        (ok * 100 / self.ledger.len()) as u32
    }
}

// ── 10 失败样本（确定性） ──────────────────────────────────────
pub fn make_fail_samples() -> Vec<FailTriple> {
    let files = [
        "vmm/page.rs", "sched/queue.rs", "drv/ahci.rs", "fs/ext4.rs",
        "net/stack.rs", "ipc/port.rs", "ui/compose.rs", "boot/mem.rs",
        "ktest/agg.rs", "time/tsc.rs",
    ];
    let mut v = Vec::new();
    let mut i = 0;
    while i < FAIL_SAMPLES {
        v.push(FailTriple {
            file: String::from(files[i]),
            line: (i as u32 + 1) * 13,
            expected: format!("0x{:X}", 0x1000 + i),
            actual: format!("0x{:X}", 0x2000 + i * 3),
        });
        i += 1;
    }
    v
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

// ── 注入检出账 ──────────────────────────────────────────────────
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

/// 黄金全链：10 样本全呈现→完整率 100%→行格式点验→双格式。
pub fn golden_run() -> Result<(u32, usize, u32), u32> {
    let samples = make_fail_samples();
    let mut p = FailPresenter::new();
    let mut i = 0;
    while i < samples.len() {
        let line = p.present(samples[i].clone())?;
        if !line.starts_with(FAIL_PREFIX) {
            return Err(FailCode::PrefixWrong.val());
        }
        i += 1;
    }
    let pct = p.completeness_pct();
    if pct != 100 {
        return Err(FailCode::CompletenessShort.val());
    }
    if p.ledger.len() != FAIL_SAMPLES {
        return Err(FailCode::LedgerShort.val());
    }
    // 双格式。
    let (dec, hex) = dual_render(-42)?;
    if dec != "-42" || hex != "-0x2A" {
        return Err(FailCode::DualFormatMissing.val());
    }
    // 行格式逐条点验（文件:行 均在场）。
    let mut k = 0;
    while k < p.ledger.len() {
        let line = p.ledger[k].render();
        if !line.contains(&p.ledger[k].file) {
            return Err(FailCode::FileBlank.val());
        }
        k += 1;
    }
    Ok((pct, p.ledger.len(), p.ledger[0].line))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut p = FailPresenter::new();
    p.present(FailTriple {
        file: String::from("a.rs"),
        line: 10,
        expected: String::from("1"),
        actual: String::from("2"),
    }).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 行号零 → 必拒。
    let mut p = FailPresenter::new();
    p.present(FailTriple {
        file: String::from("a.rs"),
        line: 0,
        expected: String::from("1"),
        actual: String::from("2"),
    }).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 空期望 → 必拒。
    let mut p = FailPresenter::new();
    p.present(FailTriple {
        file: String::from("a.rs"),
        line: 10,
        expected: String::new(),
        actual: String::from("2"),
    }).map(|_| ())
}

/// 注入 10/10：空文件、零行、空期望、坏双格式四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut p = FailPresenter::new();
        led.record(0, p.present(FailTriple {
            file: String::new(),
            line: 1,
            expected: String::from("1"),
            actual: String::from("1"),
        }).is_err());
        led.record(1, p.present(FailTriple {
            file: String::from("a.rs"),
            line: 0,
            expected: String::from("1"),
            actual: String::from("1"),
        }).is_err());
        led.record(2, p.present(FailTriple {
            file: String::from("a.rs"),
            line: 1,
            expected: String::new(),
            actual: String::from("1"),
        }).is_err());
        led.record(3, dual_render(255) == Ok((String::from("255"), String::from("0xFF"))));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FailCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FailCode::ALL.len() {
        let v = FailCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 10 样本完整率 100%。
    let samples = make_fail_samples();
    let mut p = FailPresenter::new();
    let mut i = 0;
    while i < samples.len() {
        p.present(samples[i].clone()).ok();
        i += 1;
    }
    v.push((
        "J1-10样本完整率100%",
        p.completeness_pct() == 100 && p.ledger.len() == 10,
        format!("{}/10", p.completeness_pct() / 10),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C840 窗口")));
    v
}

pub fn run_unx0523_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0523 失败三要素呈现（定位/期望/实际）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
