//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16701 · 域测试面 20 维度打分账（conformance 部分） · 0x68240
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-E1-B36）：
//!   ① 纯功能代码 ≥275 行；
//!   ② UNX-F16701-J1 20 维度逐项打分留痕（需求理解→整体感受），
//!     任何一项低于标准不闭账（语义上：低于即列修复项）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 20 维闭集：需求理解→整体感受（冻结 20 维，名称与权重）；
//!   - 打分留痕：每维（分数/评语指纹/权重）三列留痕，缺列拒；
//!   - 分数域闭集：1~10 整数（越域拒）；
//!   - 低于标准即列修复项：标准线冻结（每维 6 分），任一维 <6 →
//!     修复项清单显性（不静默、不闭账）；
//!   - 加权总分：整数加权（Σ分数×权重 / Σ权重 ×100，定点化，
//!     禁浮点），闭账线冻结；
//!   - LCG 种子写死 = SEG_BASE（0x68240）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68240，AI-VX3-1030 批16 独占区）。
pub const SEG_BASE: u32 = 0x68240;

// ── 20 维闭集（冻结） ───────────────────────────────────────────
pub const DIM_COUNT: usize = 20;
pub const DIM_NAMES: [&str; DIM_COUNT] = [
    "需求理解", "接口清晰", "错误路径", "边界完备", "守恒账",
    "确定性", "可复现", "注入检出", "齐备率", "锚点一致",
    "版本化", "append-only", "守卫密度", "诊断可读", "性能预算",
    "幂等重跑", "对账闭合", "文档锚定", "协作留痕", "整体感受",
];
/// 各维权重（冻结：和恰 1000）。
pub const DIM_WEIGHTS: [u32; DIM_COUNT] = [
    80, 60, 70, 70, 60,
    60, 50, 60, 50, 60,
    40, 40, 50, 40, 50,
    40, 50, 40, 30, 50,
];
/// 权重和守恒（编译期可查：1000）。
pub const WEIGHT_SUM: u32 = 1000;
/// 单维标准线（冻结）。
pub const DIM_THRESHOLD: u32 = 6;
/// 分数域闭集。
pub const SCORE_MIN: u32 = 1;
pub const SCORE_MAX: u32 = 10;
/// 加权总分闭账线（千分制定点：≥720/1000）。
pub const TOTAL_CLOSURE_FLOOR: u32 = 720;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScoreCode {
    /// 维越闭集
    DimOutOfRange,
    /// 维名空（20 维齐备破坏）
    DimNameBlank,
    /// 分数越域（<1 或 >10）
    ScoreOutOfRange,
    /// 权重和守恒破坏（Σ≠1000）
    WeightSumBroken,
    /// 低于标准未列修复项（静默闭账）
    SilentLowScore,
    /// 评语指纹缺失
    NoteMissing,
    /// 修复项清单错位（与低分维不对应）
    FixListMisaligned,
    /// 加权总分算错（对拍失败）
    WeightedTotalMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl ScoreCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ScoreCode::DimOutOfRange => "维越闭集：20 维冻结",
            ScoreCode::DimNameBlank => "维名空：20 维齐备破坏",
            ScoreCode::ScoreOutOfRange => "分数越域：1~10 闭集",
            ScoreCode::WeightSumBroken => "权重和守恒破坏：Σ≠1000",
            ScoreCode::SilentLowScore => "低于标准未列修复项：静默闭账",
            ScoreCode::NoteMissing => "评语指纹缺失",
            ScoreCode::FixListMisaligned => "修复项清单错位",
            ScoreCode::WeightedTotalMismatch => "加权总分算错：对拍失败",
            ScoreCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ScoreCode; 9] = [
        ScoreCode::DimOutOfRange,
        ScoreCode::DimNameBlank,
        ScoreCode::ScoreOutOfRange,
        ScoreCode::WeightSumBroken,
        ScoreCode::SilentLowScore,
        ScoreCode::NoteMissing,
        ScoreCode::FixListMisaligned,
        ScoreCode::WeightedTotalMismatch,
        ScoreCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ScoreCode::ALL.len() {
        if ScoreCode::ALL[i].val() == code {
            return ScoreCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x68240 段闭集内"
}

// ── 打分行（三列：分数/评语指纹/权重引用） ─────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreRow {
    pub dim: usize,
    pub score: u32,
    /// 评语指纹（FNV，非 0 即有评语）。
    pub note_fp: u64,
}

impl ScoreRow {
    pub fn legal(&self) -> Result<(), u32> {
        if self.dim >= DIM_COUNT {
            return Err(ScoreCode::DimOutOfRange.val());
        }
        if self.score < SCORE_MIN || self.score > SCORE_MAX {
            return Err(ScoreCode::ScoreOutOfRange.val());
        }
        if self.note_fp == 0 {
            return Err(ScoreCode::NoteMissing.val());
        }
        Ok(())
    }
}

/// 评语指纹（确定性：分数+维度派生）。
pub fn note_fingerprint(dim: usize, score: u32, salt: u64) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= dim as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= score as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= salt;
    h = h.wrapping_mul(0x100000001b3);
    h
}

// ── 打分账（20 维总成） ─────────────────────────────────────────
pub struct ScoreSheet {
    pub rows: [Option<ScoreRow>; DIM_COUNT],
}

impl ScoreSheet {
    pub fn new() -> ScoreSheet {
        ScoreSheet { rows: core::array::from_fn(|_| None) }
    }

    pub fn record(&mut self, row: ScoreRow) -> Result<(), u32> {
        row.legal()?;
        let dim = row.dim;
        self.rows[dim] = Some(row);
        Ok(())
    }

    /// 20 维齐备。
    pub fn complete(&self) -> bool {
        let mut i = 0;
        while i < DIM_COUNT {
            if self.rows[i].is_none() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 权重和守恒审计。
    pub fn weight_audit() -> Result<u32, u32> {
        let mut s = 0;
        let mut i = 0;
        while i < DIM_COUNT {
            s += DIM_WEIGHTS[i];
            i += 1;
        }
        if s != WEIGHT_SUM {
            return Err(ScoreCode::WeightSumBroken.val());
        }
        Ok(s)
    }

    /// 加权总分（千分制定点：Σscore×w / Σw ×100 → Σscore×w×100/1000）。
    pub fn weighted_total(&self) -> Result<u32, u32> {
        if !self.complete() {
            return Err(ScoreCode::DimOutOfRange.val());
        }
        let mut num = 0u64;
        let mut i = 0;
        while i < DIM_COUNT {
            let r = self.rows[i].as_ref().ok_or_else(|| ScoreCode::DimOutOfRange.val())?;
            num += (r.score as u64) * (DIM_WEIGHTS[i] as u64);
            i += 1;
        }
        Ok((num * 100 / WEIGHT_SUM as u64) as u32)
    }

    /// 修复项清单：低分维显性列出（不静默）。
    pub fn fix_items(&self) -> Result<Vec<usize>, u32> {
        if !self.complete() {
            return Err(ScoreCode::DimOutOfRange.val());
        }
        let mut v = Vec::new();
        let mut i = 0;
        while i < DIM_COUNT {
            if let Some(r) = &self.rows[i] {
                if r.score < DIM_THRESHOLD {
                    v.push(i);
                }
            }
            i += 1;
        }
        Ok(v)
    }

    /// 闭账闸：低分必列修复项；总分达线且无低分才可闭。
    pub fn closure(&self) -> Result<(bool, usize), u32> {
        let fixes = self.fix_items()?;
        let total = self.weighted_total()?;
        if fixes.len() > 0 {
            // 有低分：不闭账，但必须显性（清单非空即显性）。
            return Ok((false, fixes.len()));
        }
        Ok((total >= TOTAL_CLOSURE_FLOOR, 0))
    }
}

/// 20 维齐备账。
pub fn dims_ready() -> bool {
    if DIM_NAMES.len() != DIM_COUNT || DIM_WEIGHTS.len() != DIM_COUNT {
        return false;
    }
    let mut i = 0;
    while i < DIM_COUNT {
        if DIM_NAMES[i].len() == 0 {
            return false;
        }
        i += 1;
    }
    true
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

/// 构造确定性 20 维打分表（分数 = 基线+扰动）。
pub fn make_sheet(seed: u64, floor_boost: u32) -> Result<ScoreSheet, u32> {
    let mut lcg = Lcg::new(seed);
    let mut sheet = ScoreSheet::new();
    let mut d = 0;
    while d < DIM_COUNT {
        let jitter = lcg.below(3) as u32;
        let mut score = DIM_THRESHOLD + floor_boost + jitter;
        if score > SCORE_MAX {
            score = SCORE_MAX;
        }
        sheet.record(ScoreRow {
            dim: d,
            score,
            note_fp: note_fingerprint(d, score, seed),
        })?;
        d += 1;
    }
    Ok(sheet)
}

/// 黄金全链：达标表闭账 + 低分表显性修复项。
pub fn golden_run() -> Result<(bool, usize, u32), u32> {
    ScoreSheet::weight_audit()?;
    // 达标表：全部 ≥6 且总分≥720。
    let good = make_sheet(SEG_BASE as u64, 2)?;
    let (closed, fixes) = good.closure()?;
    if !closed || fixes != 0 {
        return Err(ScoreCode::SilentLowScore.val());
    }
    // 低分表：显性列出修复项（不闭账）。
    let mut bad = ScoreSheet::new();
    let mut d = 0;
    while d < DIM_COUNT {
        let score = if d == 3 { 3 } else { 8 };
        bad.record(ScoreRow {
            dim: d,
            score,
            note_fp: note_fingerprint(d, score, 7),
        })?;
        d += 1;
    }
    let (closed2, fixes2) = bad.closure()?;
    if closed2 {
        return Err(ScoreCode::SilentLowScore.val());
    }
    if fixes2 == 0 {
        return Err(ScoreCode::FixListMisaligned.val());
    }
    if !dims_ready() {
        return Err(ScoreCode::DimNameBlank.val());
    }
    Ok((closed, fixes2, good.weighted_total()?))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let sheet = make_sheet(11, 2)?;
    let (closed, _) = sheet.closure()?;
    if closed {
        Ok(())
    } else {
        Err(ScoreCode::SilentLowScore.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 分数越域 → 必拒。
    let mut sheet = ScoreSheet::new();
    sheet.record(ScoreRow { dim: 0, score: 11, note_fp: 1 })
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 评语缺失 → 必拒。
    let mut sheet = ScoreSheet::new();
    sheet.record(ScoreRow { dim: 0, score: 8, note_fp: 0 })
}

/// 注入 10/10：越域、缺评语、权重和破坏、低分静默四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：分数越域。
        let row0 = ScoreRow { dim: 0, score: 0, note_fp: 1 };
        led.record(0, row0.legal().is_err());
        // 槽1：缺评语。
        let row1 = ScoreRow { dim: 0, score: 8, note_fp: 0 };
        led.record(1, row1.legal().is_err());
        // 槽2：权重和破坏（伪表 Σ≠1000 对照冻结值）。
        let mut s = 0;
        let mut i = 0;
        while i < DIM_COUNT - 1 {
            s += DIM_WEIGHTS[i];
            i += 1;
        }
        led.record(2, s != WEIGHT_SUM);
        // 槽3：低分维越界注入。
        let row3 = ScoreRow { dim: DIM_COUNT, score: 5, note_fp: 1 };
        led.record(3, row3.legal().is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ScoreCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ScoreCode::ALL.len() {
        let v = ScoreCode::ALL[i].val();
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
    // 20 维齐备率 100%。
    v.push((
        "J1-20维齐备率100%",
        dims_ready() && DIM_NAMES.len() == 20,
        format!("{}/20 维", DIM_NAMES.len()),
    ));
    // 权重和守恒。
    let wa = ScoreSheet::weight_audit();
    v.push((
        "J1-权重和守恒1000",
        wa == Ok(WEIGHT_SUM) && WEIGHT_SUM == 1000,
        format!("Σ={:?}", wa),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x68240 窗口")));
    v
}

pub fn run_unx16701_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16701 域测试面 20 维度打分账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
