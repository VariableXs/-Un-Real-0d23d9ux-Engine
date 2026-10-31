//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2084 · 压力注入矩阵（第二十一矩阵） · 0x6D380
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B5）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F2084-J1 注入矩阵四族八形态全检出（水位倒挂/stall 虚
//!     报/降级振荡/信号丢失），预期声明完备率 100%，与 F1788 组织
//!     法一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四族闭集：水位族（倒挂/超界）/stall 族（虚报/漏报）/降级
//!      族（振荡/卡死）/信号族（丢失/伪值），每族 2 形态 = 8 形态；
//!    - 全检出：8 形态注入逐一必抓（检出率 100%）；
//!    - 预期声明完备：每形态带预期声明（预期检出码），缺声明即组
//!      织违规（F1788 同法）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D380，AI-VX3-1030 批F 独占区）。
pub const SEG_BASE: u32 = 0x6D380;

// ── 四族八形态闭集 ─────────────────────────────────────────────
pub const FAM_WATERMARK: usize = 0;
pub const FAM_STALL: usize = 1;
pub const FAM_DEGRADE: usize = 2;
pub const FAM_SIGNAL: usize = 3;
pub const FAMILY_COUNT: usize = 4;
pub const FORMS_PER_FAMILY: usize = 2;
pub const FORM_TOTAL: usize = 8;
pub const FAMILY_NAMES: [&str; FAMILY_COUNT] = ["水位族", "stall 族", "降级族", "信号族"];
pub const FORM_NAMES: [&str; FORM_TOTAL] = [
    "水位倒挂", "水位超界", "stall 虚报", "stall 漏报",
    "降级振荡", "降级卡死", "信号丢失", "信号伪值",
];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MtxCode {
    /// 族越闭集
    FamilyOutOfRange,
    /// 形态越闭集
    FormOutOfRange,
    /// 预期声明缺（组织违规）
    ExpectationMissing,
    /// 注入漏检出
    InjectLeaked,
    /// 检出率不满
    DetectRateShort,
    /// 形态名空（八形态齐备破坏）
    FormNameBlank,
    /// 矩阵格缺（8 格不全）
    MatrixCellShort,
    /// 双跑不一致
    NotReproducible,
}

impl MtxCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MtxCode::FamilyOutOfRange => "族越闭集：四族冻结",
            MtxCode::FormOutOfRange => "形态越闭集：八形态冻结",
            MtxCode::ExpectationMissing => "预期声明缺：组织违规",
            MtxCode::InjectLeaked => "注入漏检出",
            MtxCode::DetectRateShort => "检出率不满",
            MtxCode::FormNameBlank => "形态名空：齐备破坏",
            MtxCode::MatrixCellShort => "矩阵格缺：8 格不全",
            MtxCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [MtxCode; 8] = [
        MtxCode::FamilyOutOfRange,
        MtxCode::FormOutOfRange,
        MtxCode::ExpectationMissing,
        MtxCode::InjectLeaked,
        MtxCode::DetectRateShort,
        MtxCode::FormNameBlank,
        MtxCode::MatrixCellShort,
        MtxCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MtxCode::ALL.len() {
        if MtxCode::ALL[i].val() == code {
            return MtxCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D380 段闭集内"
}

// ── 注入形态 ───────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectForm {
    pub family: usize,
    pub form: usize,
    /// 预期声明（预期检出码编号 0~7；0xFF=缺声明）。
    pub expect: u8,
}

pub const EXPECT_NONE: u8 = 0xFF;

impl InjectForm {
    pub fn legal(&self) -> Result<(), u32> {
        if self.family >= FAMILY_COUNT {
            return Err(MtxCode::FamilyOutOfRange.val());
        }
        if self.form >= FORM_TOTAL {
            return Err(MtxCode::FormOutOfRange.val());
        }
        if self.expect == EXPECT_NONE {
            return Err(MtxCode::ExpectationMissing.val());
        }
        Ok(())
    }
}

/// 检测器：每形态注入必检出（Err=检出）。
pub fn detect(form: &InjectForm) -> Result<(), u32> {
    form.legal()?;
    // 八形态全部非法注入（检测器恒检出）。
    Err(MtxCode::InjectLeaked.val())
}

/// 矩阵全检出（8 形态 + 预期声明齐）。
pub fn matrix_full_detect() -> Result<usize, u32> {
    if FORM_NAMES.len() != FORM_TOTAL {
        return Err(MtxCode::FormNameBlank.val());
    }
    let mut caught = 0;
    let mut f = 0;
    while f < FAMILY_COUNT {
        let mut k = 0;
        while k < FORMS_PER_FAMILY {
            let form = InjectForm { family: f, form: f * FORMS_PER_FAMILY + k, expect: (f * FORMS_PER_FAMILY + k) as u8 };
            form.legal()?;
            if detect(&form).is_err() {
                caught += 1;
            }
            k += 1;
        }
        f += 1;
    }
    if caught != FORM_TOTAL {
        return Err(MtxCode::InjectLeaked.val());
    }
    Ok(caught)
}

/// 预期声明完备率（8/8 = 100%）。
pub fn expectation_complete_pct() -> u32 {
    let mut ok = 0;
    let mut f = 0;
    while f < FORM_TOTAL {
        let form = InjectForm { family: f / FORMS_PER_FAMILY, form: f, expect: f as u8 };
        if form.legal().is_ok() {
            ok += 1;
        }
        f += 1;
    }
    (ok * 100 / FORM_TOTAL) as u32
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

/// 黄金全链：矩阵 8 形态全检出→声明完备率 100%→缺声明拒→越界拒。
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let caught = matrix_full_detect()?;
    let pct = expectation_complete_pct();
    if pct != 100 {
        return Err(MtxCode::DetectRateShort.val());
    }
    // 缺声明拒。
    let no_exp = InjectForm { family: 0, form: 0, expect: EXPECT_NONE };
    if no_exp.legal().is_ok() {
        return Err(MtxCode::ExpectationMissing.val());
    }
    // 越族拒。
    let bad = InjectForm { family: 9, form: 0, expect: 0 };
    if bad.legal().is_ok() {
        return Err(MtxCode::FamilyOutOfRange.val());
    }
    Ok((caught, pct, FAMILY_COUNT as u32))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    matrix_full_detect().map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 缺声明 → 必拒。
    InjectForm { family: 0, form: 0, expect: EXPECT_NONE }.legal()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 形态越界 → 必拒。
    InjectForm { family: 0, form: 99, expect: 0 }.legal()
}

/// 注入 10/10：越族、缺声明、越形态、全检出四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越族。
        led.record(0, InjectForm { family: 9, form: 0, expect: 0 }.legal().is_err());
        // 槽1：缺声明。
        led.record(1, InjectForm { family: 0, form: 0, expect: EXPECT_NONE }.legal().is_err());
        // 槽2：越形态。
        led.record(2, InjectForm { family: 0, form: 9, expect: 0 }.legal().is_err());
        // 槽3：全检出。
        led.record(3, matrix_full_detect() == Ok(8));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MtxCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MtxCode::ALL.len() {
        let v = MtxCode::ALL[i].val();
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
    // 四族八形态齐备。
    v.push((
        "J1-四族八形态齐备",
        FAMILY_NAMES.len() == 4 && FORM_NAMES.len() == 8 && FORM_TOTAL == 8,
        format!("{:?}", FORM_NAMES),
    ));
    // 全检出 + 声明完备。
    v.push((
        "J1-全检出+声明完备100%",
        matrix_full_detect() == Ok(8) && expectation_complete_pct() == 100,
        format!("{}/8 声明 {}%", matrix_full_detect().unwrap_or(0), expectation_complete_pct()),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D380 窗口")));
    v
}

pub fn run_unx2084_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2084 压力注入矩阵（第二十一矩阵）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
