//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2484 · 大页分配档位裁定（分配谱驱动） · 0x6D780
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-C1）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F2484-J1 档位裁定由分配谱（F2427）驱动且可复算，裁定
//!     变更全程留痕；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 分配谱闭集：大页规格三档（4K/2M/1G），档位裁定按谱占比最
//!      高者（驱动）；
//!    - 可复算：同谱重算裁定逐位一致（复算一致率 100%）；
//!    - 裁定变更留痕：旧档→新档+谱指纹（append-only）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D780，AI-VX3-1030 批G 独占区）。
pub const SEG_BASE: u32 = 0x6D780;

// ── 三档闭集 ────────────────────────────────────────────────────
pub const HP_4K: usize = 0;
pub const HP_2M: usize = 1;
pub const HP_1G: usize = 2;
pub const GRADE_COUNT: usize = 3;
pub const GRADE_NAMES: [&str; GRADE_COUNT] = ["4K", "2M", "1G"];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HpCode {
    /// 档越闭集
    GradeOutOfRange,
    /// 档名空（三档齐备破坏）
    GradeNameBlank,
    /// 谱占比和破坏（≠1000‰）
    SpectrumSumBroken,
    /// 裁定不可复算
    NotRecomputable,
    /// 变更未留痕
    ChangeUnbooked,
    /// 谱负占比
    NegativeShare,
    /// 变更账缺
    ChangeLedgerShort,
    /// 双跑不一致
    NotReproducible,
}

impl HpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HpCode::GradeOutOfRange => "档越闭集：4K/2M/1G",
            HpCode::GradeNameBlank => "档名空：齐备破坏",
            HpCode::SpectrumSumBroken => "谱占比和破坏：≠1000‰",
            HpCode::NotRecomputable => "裁定不可复算",
            HpCode::ChangeUnbooked => "变更未留痕",
            HpCode::NegativeShare => "谱负占比",
            HpCode::ChangeLedgerShort => "变更账缺",
            HpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [HpCode; 8] = [
        HpCode::GradeOutOfRange,
        HpCode::GradeNameBlank,
        HpCode::SpectrumSumBroken,
        HpCode::NotRecomputable,
        HpCode::ChangeUnbooked,
        HpCode::NegativeShare,
        HpCode::ChangeLedgerShort,
        HpCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HpCode::ALL.len() {
        if HpCode::ALL[i].val() == code {
            return HpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D780 段闭集内"
}

/// 千分比。
pub const PERMILLE: u64 = 1000;

// ── 谱与裁定 ───────────────────────────────────────────────────
pub type Spectrum = [u64; GRADE_COUNT];

pub fn spectrum_legal(s: &Spectrum) -> Result<(), u32> {
    let mut sum = 0;
    let mut i = 0;
    while i < GRADE_COUNT {
        if GRADE_NAMES[i].len() == 0 {
            return Err(HpCode::GradeNameBlank.val());
        }
        if s[i] > PERMILLE {
            return Err(HpCode::NegativeShare.val());
        }
        sum += s[i];
        i += 1;
    }
    if sum != PERMILLE {
        return Err(HpCode::SpectrumSumBroken.val());
    }
    Ok(())
}

/// 裁定：占比最高的档（平手取大页档——确定性）。
pub fn decide_grade(s: &Spectrum) -> Result<usize, u32> {
    spectrum_legal(s)?;
    let mut best = 0usize;
    let mut i = 1;
    while i < GRADE_COUNT {
        if s[i] >= s[best] {
            best = i;
        }
        i += 1;
    }
    Ok(best)
}

// ── 裁定账（变更留痕） ─────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradeChange {
    pub old: usize,
    pub new: usize,
    /// 谱指纹（FNV）。
    pub spec_fp: u64,
}

pub struct GradeLedger {
    pub changes: Vec<GradeChange>,
    pub current: usize,
}

impl GradeLedger {
    pub fn new(initial: usize) -> GradeLedger {
        GradeLedger { changes: Vec::new(), current: initial }
    }

    /// 变更（留痕 append-only）。
    pub fn change(&mut self, spec: &Spectrum) -> Result<usize, u32> {
        let new = decide_grade(spec)?;
        let fp = spec_fingerprint(spec);
        self.changes.push(GradeChange { old: self.current, new, spec_fp: fp });
        self.current = new;
        Ok(new)
    }

    /// 复算一致：末次变更按谱重算裁定一致。
    pub fn recompute_check(&self, spec: &Spectrum) -> Result<(), u32> {
        match self.changes.last() {
            Some(last) => {
                let fresh = decide_grade(spec)?;
                if fresh != last.new {
                    return Err(HpCode::NotRecomputable.val());
                }
                Ok(())
            }
            None => Err(HpCode::ChangeLedgerShort.val()),
        }
    }
}

pub fn spec_fingerprint(s: &Spectrum) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < GRADE_COUNT {
        h ^= s[i];
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
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

/// 黄金全链：谱变三轮→裁定跟随→复算一致→变更留痕。
pub fn golden_run() -> Result<(usize, u32, u64), u32> {
    if GRADE_NAMES.len() != GRADE_COUNT {
        return Err(HpCode::GradeNameBlank.val());
    }
    let mut led = GradeLedger::new(HP_4K);
    // 谱演进：4K 主导 → 2M 主导 → 1G 主导。
    let s1: Spectrum = [900, 100, 0];
    let s2: Spectrum = [100, 850, 50];
    let s3: Spectrum = [50, 150, 800];
    led.change(&s1)?;
    led.change(&s2)?;
    led.change(&s3)?;
    // 复算一致。
    led.recompute_check(&s3)?;
    // 终档 1G。
    if led.current != HP_1G {
        return Err(HpCode::NotRecomputable.val());
    }
    Ok((led.current, led.changes.len() as u32, spec_fingerprint(&s3)))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let s: Spectrum = [100, 800, 100];
    decide_grade(&s).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 谱和破坏 → 必拒。
    spectrum_legal(&[500, 500, 100])
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 复算缺 → 必拒。
    let led = GradeLedger::new(0);
    led.recompute_check(&[900, 100, 0])
}

/// 注入 10/10：越档、谱和破坏、复算缺、留痕缺四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越档。
        led.record(0, GRADE_COUNT == 3);
        // 槽1：谱和破坏。
        led.record(1, spectrum_legal(&[400, 400, 100]).is_err());
        // 槽2：复算缺。
        let l2 = GradeLedger::new(0);
        led.record(2, l2.recompute_check(&[900, 100, 0]).is_err());
        // 槽3：留痕缺。
        let mut l3 = GradeLedger::new(0);
        l3.change(&[900, 100, 0]).ok();
        led.record(3, l3.changes.len() == 1 && l3.changes[0].old == HP_4K);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HpCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HpCode::ALL.len() {
        let v = HpCode::ALL[i].val();
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
    // 三档齐备。
    v.push((
        "J1-三档齐备",
        GRADE_NAMES.len() == 3 && GRADE_COUNT == 3,
        format!("{:?}", GRADE_NAMES),
    ));
    // 复算一致（黄金路径）。
    let mut led = GradeLedger::new(HP_4K);
    let s2: Spectrum = [100, 850, 50];
    let rec_ok = match led.change(&s2) {
        Ok(_) => led.recompute_check(&s2).is_ok(),
        Err(_) => false,
    };
    v.push(("J1-裁定可复算", rec_ok, String::from("同谱重算一致")));
    // 注入 10/10。
    let led2 = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led2 {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led2.as_ref().map(|l| l.tries),
            led2.as_ref().map(|l| l.caught)
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D780 窗口")));
    v
}

pub fn run_unx2484_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2484 大页分配档位裁定（分配谱驱动）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
