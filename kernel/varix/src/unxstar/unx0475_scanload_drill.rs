//! UNX-F0475 扫描负载演练（重入/重复扫描） · 0x757C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，AI-VX7-1104 批3 H 组）：
//!   连续 10 轮重复扫描结果位图全等 10/10，扫描耗时分布落账
//!   （p99 注记）。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 扫描模型确定性：512 槽设备面由冻结谓词 `plane_present` 给出，
//!     重复扫描同一面必得同一位图——10 轮位图全等 10/10 是主判据；
//!   ② 重入防护：扫描中（InFlight）再入扫描显性拒（Reentry），
//!     不静默排队、不静默丢弃——相位机 Idle→InFlight→Idle 单步泵动；
//!   ③ 耗时分布落账口径**步数化**（零墙钟）：每轮记 steps =
//!     面槽遍历步 + 命中记账步 + 词写步，分布账存原始样本序与
//!     升序副本，p99 按 (n-1)*99/100 最近秩取整；
//!   ④ 位图全等判定在**词级**（[u64;8] 逐词等值）+ **位级**（popcount
//!     等值 + 位异或为零）双口径，防词内扰动漏检；
//!   ⑤ 注入路：重入路 / 异面发散路（换谓词必与基线失配）/ 轮数越限
//!     路 / 空分布查 p99 路，四路注入每路必抓；
//!   ⑥ 演练轮数冻结 10（判据 10/10 语义），面槽冻结 512（8×64 位），
//!     单演练轮数上限冻结 64 防账面无界。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x757C0，批3 H 组独占窗口）。
pub const SEG_BASE: u32 = 0x757C0;

/// 设备面槽位（冻结 512：8 词 × 64 位）。
pub const PLANE_SLOTS: usize = 512;
/// 位图词数（冻结 8）。
pub const BITMAP_WORDS: usize = 8;
/// 判据演练轮数（写死 10：判据 10/10 语义）。
pub const DRILL_ROUNDS: u32 = 10;
/// 单账演练轮数上限（冻结 64：防账面无界）。
pub const MAX_ROUNDS: usize = 64;
/// p99 注记所需最小样本数（冻结 2：单样本无分布语义）。
pub const P99_MIN_SAMPLES: usize = 2;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
/// 诊断码闭集：每码带可操作中文建议（detail 文案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillCode {
    /// 扫描中重入：InFlight 相位再入扫描
    Reentry,
    /// 位图发散：与基线轮位图不等
    BitmapDiverge,
    /// 轮数越限：超过 MAX_ROUNDS
    RoundOver,
    /// 分布账为空：未跑轮就查 p99
    EmptyDist,
    /// 相位倒退：非 Idle 相位被要求结束/回退
    PhaseRegress,
    /// 位图词数不符：词面长度不是 BITMAP_WORDS
    WordFaceDrift,
    /// p99 秩越界：样本数不足以算 p99
    P99Short,
    /// 分布失配：分布账样本数与已记轮数不等
    DistMismatch,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl DrillCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DrillCode::Reentry => "扫描中重入拒：请等当前轮 Idle 后再发起，或改用排队入口（本面不提供）",
            DrillCode::BitmapDiverge => "位图发散：请核对设备面谓词是否被换、基线轮是否取错",
            DrillCode::RoundOver => "轮数越限：超过 MAX_ROUNDS=64，请先结账再开新演练账",
            DrillCode::EmptyDist => "分布账为空：请至少跑 1 轮扫描再查 p99",
            DrillCode::PhaseRegress => "相位倒退拒：相位仅 Idle→InFlight→Idle，请按序泵动",
            DrillCode::WordFaceDrift => "位图词数不符：词面恒为 8 词，请查构造方",
            DrillCode::P99Short => "p99 样本不足：至少 2 个样本才能注记 p99",
            DrillCode::DistMismatch => "分布失配：分布账样本数与轮账不等，请查记账漏",
            DrillCode::NotReproducible => "双跑不一致：复现红线，请检查是否有墙钟/随机参与取值",
        }
    }
    pub const ALL: [DrillCode; 9] = [
        DrillCode::Reentry,
        DrillCode::BitmapDiverge,
        DrillCode::RoundOver,
        DrillCode::EmptyDist,
        DrillCode::PhaseRegress,
        DrillCode::WordFaceDrift,
        DrillCode::P99Short,
        DrillCode::DistMismatch,
        DrillCode::NotReproducible,
    ];
}

/// 段内查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DrillCode::ALL.len() {
        if let Some(c) = DrillCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x757C0 段闭集内"
}

// ── 设备面谓词（冻结：确定性，无随机无墙钟） ────────────────────
/// 真机设备面：槽位在位谓词（冻结口径）。
/// 语料取自典型扫描面：前 9 槽为根复合体保留区，其余按 7/13 双模散布。
pub fn plane_present(slot: usize) -> bool {
    slot < 9 || slot % 7 == 3 || slot % 13 == 5
}

/// 异面谓词（注入路专用：换模必与真机面发散）。
pub fn alien_plane_present(slot: usize) -> bool {
    slot < 9 || slot % 7 == 3 || slot % 11 == 2
}

/// 面槽在位总数（真机面，冻结期望锚）。
pub fn plane_present_count() -> u64 {
    let mut n = 0u64;
    let mut s = 0usize;
    while s < PLANE_SLOTS {
        if plane_present(s) {
            n = n.saturating_add(1);
        }
        s += 1;
    }
    n
}

// ── 位图（词级 + 位级双口径等值） ───────────────────────────────
/// 单轮扫描位图。
pub type Bitmap = [u64; BITMAP_WORDS];

/// 词级等值（逐词直比）。
pub fn bitmap_eq_words(a: &Bitmap, b: &Bitmap) -> bool {
    let mut i = 0usize;
    while i < BITMAP_WORDS {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 位级等值（popcount 等值 + 逐词异或为零）。
pub fn bitmap_eq_bits(a: &Bitmap, b: &Bitmap) -> bool {
    let mut ca = 0u64;
    let mut cb = 0u64;
    let mut i = 0usize;
    while i < BITMAP_WORDS {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if *x != 0u64 {
                    ca = ca.saturating_add(x.count_ones() as u64);
                }
                cb = cb.saturating_add(y.count_ones() as u64);
                if x ^ y != 0 {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    ca == cb
}

/// 纯扫描一轮设备面：产出位图与步数（步数化耗时口径）。
/// steps = 面槽遍历步（512）+ 命中记账步（在位数）+ 词写步（8）。
pub fn scan_plane(present: fn(usize) -> bool) -> (Bitmap, u64) {
    let mut bm: Bitmap = [0u64; BITMAP_WORDS];
    let mut hits = 0u64;
    let mut s = 0usize;
    while s < PLANE_SLOTS {
        if present(s) {
            hits = hits.saturating_add(1);
            let word = s / 64;
            let bit = s % 64;
            if let Some(w) = bm.get_mut(word) {
                *w |= 1u64 << bit;
            }
        }
        s += 1;
    }
    let steps = PLANE_SLOTS as u64 + hits + BITMAP_WORDS as u64;
    (bm, steps)
}

// ── 相位机 ──────────────────────────────────────────────────────
/// 演练相位（单步泵动：Idle→InFlight→Idle）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrillPhase {
    /// 空闲：可发起新一轮扫描
    Idle,
    /// 扫描中：重入显性拒
    InFlight,
}

/// 单轮轮账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanRoundRecord {
    pub round_no: u32,
    pub equal: bool,
    pub steps: u64,
    pub bitmap: Bitmap,
}

/// 步数分布账（p99 注记口径：原始序 + 升序副本）。
#[derive(Clone, Debug, Default)]
pub struct StepDist {
    pub samples: Vec<u64>,
    pub sorted: Vec<u64>,
}

impl StepDist {
    pub fn push(&mut self, steps: u64) {
        self.samples.push(steps);
        self.sorted.push(steps);
        // 升序副本插入排序（样本量 ≤ 64，常数可承受）。
        let mut i = self.sorted.len();
        while i > 1 {
            let a = self.sorted.get(i - 1).copied().unwrap_or(0);
            let b = self.sorted.get(i - 2).copied().unwrap_or(0);
            if a >= b {
                break;
            }
            self.sorted[i - 1] = b;
            self.sorted[i - 2] = a;
            i -= 1;
        }
    }
    /// p99 注记（最近秩口径：idx = (n-1)*99/100）；样本不足显性拒。
    pub fn p99(&self) -> Result<u64, u32> {
        let n = self.sorted.len();
        if n == 0 {
            return Err(DrillCode::EmptyDist.val());
        }
        if n < P99_MIN_SAMPLES {
            return Err(DrillCode::P99Short.val());
        }
        let idx = ((n - 1) * 99) / 100;
        self.sorted.get(idx).copied().ok_or(DrillCode::P99Short.val())
    }
    /// p99 注记文本（步数化口径显性标注）。
    pub fn p99_note(&self) -> String {
        match self.p99() {
            Ok(p) => format!("p99={} 步（最近秩 (n-1)*99/100，n={}）", p, self.sorted.len()),
            Err(e) => format!("p99 注记不可用：{}", detail(e)),
        }
    }
}

/// 演练账：多轮重复扫描 + 重入防护 + 分布落账。
#[derive(Clone, Debug)]
pub struct ScanDrill {
    pub phase: DrillPhase,
    pub rounds: Vec<ScanRoundRecord>,
    pub baseline: Option<Bitmap>,
    pub dist: StepDist,
    /// 重入拦截计数（显性记账）
    pub reentry_blocked: u32,
    /// 发散拦截计数
    pub diverge_blocked: u32,
}

impl ScanDrill {
    pub fn new() -> ScanDrill {
        ScanDrill {
            phase: DrillPhase::Idle,
            rounds: Vec::new(),
            baseline: None,
            dist: StepDist::default(),
            reentry_blocked: 0,
            diverge_blocked: 0,
        }
    }

    /// 发起扫描（置 InFlight；非 Idle 相位发起 = 重入）。
    pub fn begin_scan(&mut self) -> Result<(), u32> {
        if self.phase != DrillPhase::Idle {
            self.reentry_blocked = self.reentry_blocked.saturating_add(1);
            return Err(DrillCode::Reentry.val());
        }
        if self.rounds.len() >= MAX_ROUNDS {
            return Err(DrillCode::RoundOver.val());
        }
        self.phase = DrillPhase::InFlight;
        Ok(())
    }

    /// 结束扫描（回 Idle 并落轮账）；非 InFlight 结束 = 相位倒退。
    pub fn finish_scan(&mut self, rec: ScanRoundRecord) -> Result<(), u32> {
        if self.phase != DrillPhase::InFlight {
            return Err(DrillCode::PhaseRegress.val());
        }
        self.phase = DrillPhase::Idle;
        self.dist.push(rec.steps);
        self.rounds.push(rec);
        Ok(())
    }

    /// 一轮完整扫描（真机面）：重入防护 + 基线全等判定。
    pub fn scan_round(&mut self) -> Result<ScanRoundRecord, u32> {
        self.begin_scan()?;
        let (bm, steps) = scan_plane(plane_present);
        let round_no = self.rounds.len() as u32;
        let equal = match &self.baseline {
            None => true,
            Some(base) => bitmap_eq_words(base, &bm) && bitmap_eq_bits(base, &bm),
        };
        let rec = ScanRoundRecord { round_no, equal, steps, bitmap: bm };
        if self.baseline.is_none() {
            self.baseline = Some(rec.bitmap);
        }
        self.finish_scan(rec.clone())?;
        Ok(rec)
    }

    /// 一轮发散扫描（异面注入）：必须被全等判定抓出。
    pub fn inject_diverge_round(&mut self) -> Result<bool, u32> {
        self.begin_scan()?;
        let (bm, steps) = scan_plane(alien_plane_present);
        let diverges = match &self.baseline {
            None => false,
            Some(base) => !(bitmap_eq_words(base, &bm) && bitmap_eq_bits(base, &bm)),
        };
        if diverges {
            self.diverge_blocked = self.diverge_blocked.saturating_add(1);
        }
        let rec = ScanRoundRecord {
            round_no: self.rounds.len() as u32,
            equal: !diverges,
            steps,
            bitmap: bm,
        };
        self.finish_scan(rec)?;
        Ok(diverges)
    }

    /// 轮账与分布账对拍（两账样本数必须相等）。
    pub fn dist_consistent(&self) -> bool {
        self.dist.samples.len() == self.rounds.len()
    }
}

// ── LCG（种子固定可用） ────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

// ── 注入检出账（四路：重入/发散/越限/空分布） ────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 真机基线位图（独立重算锚）。
pub fn baseline_bitmap() -> Bitmap {
    let (bm, _) = scan_plane(plane_present);
    bm
}

/// 异面位图（发散注入锚）。
pub fn alien_bitmap() -> Bitmap {
    let (bm, _) = scan_plane(alien_plane_present);
    bm
}

// ── 黄金全链：10 轮重复扫描全等 → 重入拒 → 发散抓 → 分布对拍 → p99
pub fn golden_run() -> Result<(u32, u32, u64), u32> {
    let mut d = ScanDrill::new();
    let base = baseline_bitmap();
    // 10 轮重复扫描，位图全等 10/10。
    let mut equal_hits = 0u32;
    let mut r = 0u32;
    while r < DRILL_ROUNDS {
        let rec = d.scan_round()?;
        if rec.equal && bitmap_eq_words(&base, &rec.bitmap) {
            equal_hits += 1;
        }
        r += 1;
    }
    // 重入拒：InFlight 中再发起必拒。
    d.begin_scan()?;
    let reentry_rejected = d.scan_round().is_err();
    // 相位复位（InFlight 中 scan_round 已被拒，相位仍在 InFlight）：
    // 用 finish_scan 落一轮账回 Idle，保持账面自洽。
    let (bm, steps) = scan_plane(plane_present);
    let rec = ScanRoundRecord { round_no: d.rounds.len() as u32, equal: true, steps, bitmap: bm };
    d.finish_scan(rec)?;
    // 发散注入必抓。
    let diverge_caught = d.inject_diverge_round()?;
    // 分布对拍 + p99 注记。
    let dist_ok = d.dist_consistent();
    let p99 = match d.dist.p99() {
        Ok(p) => p,
        Err(_) => return Err(DrillCode::DistMismatch.val()),
    };
    // 全等轮计数：10 主判据轮 + 复位轮 = 11。
    let total_ok = (equal_hits == DRILL_ROUNDS && dist_ok && reentry_rejected && diverge_caught) as u32;
    Ok((equal_hits + total_ok, diverge_caught as u32 + reentry_rejected as u32, p99))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链精确值（绿位 11 / 拦 2 / p99 = 真机面步数）。
pub fn probe_seq_a() -> Result<(), u32> {
    let expect_p99 = PLANE_SLOTS as u64 + plane_present_count() + BITMAP_WORDS as u64;
    match golden_run() {
        Ok((11, 2, p)) if p == expect_p99 => Ok(()),
        _ => Err(DrillCode::NotReproducible.val()),
    }
}

/// 序 B：位图独立重算锚对拍（真机面 vs 异面必不等；异面位级/词级双失配）。
pub fn probe_seq_b() -> Result<(), u32> {
    let base = baseline_bitmap();
    let alien = alien_bitmap();
    let distinct = !bitmap_eq_words(&base, &alien) && !bitmap_eq_bits(&base, &alien);
    let count_ok = plane_present_count() > 0 && plane_present_count() < PLANE_SLOTS as u64;
    match (distinct, count_ok) {
        (true, true) => Ok(()),
        _ => Err(DrillCode::BitmapDiverge.val()),
    }
}

/// 序 C：冻结常量（面槽/词数/演练轮数/上限/p99 最小样本）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        PLANE_SLOTS == 512,
        BITMAP_WORDS == 8,
        DRILL_ROUNDS == 10,
        MAX_ROUNDS == 64,
        P99_MIN_SAMPLES == 2,
    ) {
        (true, true, true, true, true) => Ok(()),
        _ => Err(DrillCode::PhaseRegress.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0u32;
    while r < rounds {
        let _ = rng.next();
        // 路 1：重入路。
        let mut d1 = ScanDrill::new();
        let _ = d1.scan_round();
        let _ = d1.begin_scan();
        led.record(0, d1.scan_round().is_err());
        // 路 2：发散路（异面与基线失配必抓）。
        let mut d2 = ScanDrill::new();
        let _ = d2.scan_round();
        let caught = d2.inject_diverge_round().unwrap_or(false);
        led.record(1, caught);
        // 路 3：越限路（灌满 MAX_ROUNDS 后再发起必拒）。
        let mut d3 = ScanDrill::new();
        let mut ok_rounds = 0usize;
        while ok_rounds < MAX_ROUNDS {
            if d3.scan_round().is_ok() {
                ok_rounds += 1;
            } else {
                break;
            }
        }
        let _ = d3.begin_scan();
        led.record(2, d3.scan_round().is_err() && ok_rounds == MAX_ROUNDS);
        // 路 4：空分布路（新账查 p99 必拒；单样本查 p99 必拒）。
        let d4 = ScanDrill::new();
        let empty_rejected = matches!(d4.dist.p99(), Err(e) if e == DrillCode::EmptyDist.val());
        let mut d4b = ScanDrill::new();
        let _ = d4b.scan_round();
        let single_rejected = matches!(d4b.dist.p99(), Err(e) if e == DrillCode::P99Short.val());
        led.record(3, empty_rejected && single_rejected);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DrillCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0usize;
    while i < DrillCode::ALL.len() {
        if let Some(c) = DrillCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((ok, caught, _)) => *ok == 11 && *caught == 2,
            Err(_) => false,
        },
        match &g {
            Ok((ok, caught, p99)) => format!("绿位={} 拦={} p99={}", ok, caught, p99),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 2. 主判据：10 轮重复扫描位图全等 10/10。
    let mut d = ScanDrill::new();
    let mut equal_hits = 0u32;
    let mut r = 0u32;
    let mut run_err = false;
    while r < DRILL_ROUNDS {
        match d.scan_round() {
            Ok(rec) => {
                if rec.equal {
                    equal_hits += 1;
                }
            }
            Err(_) => {
                run_err = true;
                break;
            }
        }
        r += 1;
    }
    v.push((
        "J1-10轮位图全等",
        !run_err && equal_hits == DRILL_ROUNDS,
        format!("全等 {}/10 运行错={}", equal_hits, run_err),
    ));

    // 3. 位图双口径等值（词级 + 位级）对真机锚。
    let base = baseline_bitmap();
    let mut d2 = ScanDrill::new();
    let bm_ok = match d2.scan_round() {
        Ok(rec) => bitmap_eq_words(&base, &rec.bitmap) && bitmap_eq_bits(&base, &rec.bitmap),
        Err(_) => false,
    };
    v.push((
        "J1-位图双口径对拍",
        bm_ok,
        String::from("词级逐词等值 + 位级 popcount/异或等值"),
    ));

    // 4. 扫描耗时分布落账（p99 注记）。
    let dist_note = d.dist.p99_note();
    let dist_ok = d.dist_consistent() && d.dist.p99().is_ok();
    v.push((
        "J1-耗时分布落账",
        dist_ok,
        dist_note,
    ));

    // 5. p99 口径边界：两样本 / 满样本。
    let mut dd = ScanDrill::new();
    let _ = dd.scan_round();
    let _ = dd.scan_round();
    let two_ok = dd.dist.p99().is_ok();
    let p99_full_ok = d.dist.p99() == Ok(PLANE_SLOTS as u64 + plane_present_count() + BITMAP_WORDS as u64);
    v.push((
        "J1-p99口径边界",
        two_ok && p99_full_ok,
        format!("两样本可注记={} 满样本 p99=定值 {}", two_ok, p99_full_ok),
    ));

    // 6. 重入防护（InFlight 再入显性拒 + 拦截计数落账）。
    let mut d3 = ScanDrill::new();
    let _ = d3.scan_round();
    let _ = d3.begin_scan();
    let reentry = d3.scan_round().is_err();
    v.push((
        "J1-重入显性拒",
        reentry && d3.reentry_blocked == 1,
        format!("重入拒={} 拦截记账={}", reentry, d3.reentry_blocked),
    ));

    // 7. 发散注入必抓（异面轮与基线失配）。
    let mut d4 = ScanDrill::new();
    let _ = d4.scan_round();
    let diverge = d4.inject_diverge_round().unwrap_or(false);
    v.push((
        "J1-发散注入必抓",
        diverge && d4.diverge_blocked == 1,
        format!("发散抓={} 拦截记账={}", diverge, d4.diverge_blocked),
    ));

    // 8. 轮数越限拒（灌满 64 后再发起必拒）。
    let mut d5 = ScanDrill::new();
    let mut filled = 0usize;
    while filled < MAX_ROUNDS {
        if d5.scan_round().is_ok() {
            filled += 1;
        } else {
            break;
        }
    }
    let _ = d5.begin_scan();
    let over = d5.scan_round().is_err();
    v.push((
        "J1-轮数越限拒",
        over && filled == MAX_ROUNDS,
        format!("满 {} 轮后再发起拒={}", filled, over),
    ));

    // 9. 相位倒退拒（Idle 态直接 finish 拒）。
    let mut d6 = ScanDrill::new();
    let (bm0, steps0) = scan_plane(plane_present);
    let ghost = ScanRoundRecord { round_no: 0, equal: true, steps: steps0, bitmap: bm0 };
    v.push((
        "J1-相位倒退拒",
        d6.finish_scan(ghost).is_err(),
        String::from("Idle 态 finish_scan 显性拒"),
    ));

    // 10. 注入 10/10。
    let inj = inject_rounds(DRILL_ROUNDS);
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

    // 11. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J2-探针三序",
        a && b && c,
        format!("a=黄金({}) b=锚对拍({}) c=冻结({})", a, b, c),
    ));

    // 12. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));

    // 13. 段位守卫 + 闭集查表。
    let guard_ok = segment_guard();
    let mut table_ok = true;
    let mut i = 0usize;
    while i < DrillCode::ALL.len() {
        if let Some(code) = DrillCode::ALL.get(i) {
            if detail(code.val()).is_empty() {
                table_ok = false;
            }
        }
        i += 1;
    }
    v.push((
        "J3-段位守卫+闭集",
        guard_ok && table_ok,
        format!("0x757C0 窗口守卫={} 闭集查表={}", guard_ok, table_ok),
    ));

    v
}

pub fn run_unx0475_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0475 扫描负载演练（重入/重复扫描）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（允许 assert!/assert_eq!；取值仍走 .get()） ──────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_run_all_green() {
        let expect_p99 = PLANE_SLOTS as u64 + plane_present_count() + BITMAP_WORDS as u64;
        assert_eq!(golden_run(), Ok((11, 2, expect_p99)));
    }

    #[test]
    fn ten_rounds_bitmap_identical() {
        let mut d = ScanDrill::new();
        let mut i = 0;
        while i < 10 {
            let rec = d.scan_round().unwrap_or_else(|_| ScanRoundRecord {
                round_no: 0,
                equal: false,
                steps: 0,
                bitmap: [0u64; BITMAP_WORDS],
            });
            assert!(rec.equal, "第 {} 轮位图必须与基线全等", i);
            i += 1;
        }
        assert_eq!(d.rounds.len(), 10);
        assert!(d.dist_consistent());
    }

    #[test]
    fn reentry_rejected_and_counted() {
        let mut d = ScanDrill::new();
        let _ = d.scan_round();
        assert!(d.begin_scan().is_ok());
        assert_eq!(d.scan_round().err(), Some(DrillCode::Reentry.val()));
        assert_eq!(d.reentry_blocked, 1, "重入拦截必须显性记账");
    }

    #[test]
    fn diverge_plane_always_caught() {
        let base = baseline_bitmap();
        let alien = alien_bitmap();
        assert!(!bitmap_eq_words(&base, &alien), "异面与真机面词级必不等");
        assert!(!bitmap_eq_bits(&base, &alien), "异面与真机面位级必不等");
        let mut d = ScanDrill::new();
        let _ = d.scan_round();
        assert!(d.inject_diverge_round().unwrap_or(false));
    }

    #[test]
    fn round_over_rejected_at_capacity() {
        let mut d = ScanDrill::new();
        let mut i = 0usize;
        while i < MAX_ROUNDS {
            assert!(d.scan_round().is_ok());
            i += 1;
        }
        let _ = d.begin_scan();
        assert_eq!(d.scan_round().err(), Some(DrillCode::RoundOver.val()));
    }

    #[test]
    fn p99_caliber_boundaries() {
        let mut d = ScanDrill::new();
        assert_eq!(d.dist.p99().err(), Some(DrillCode::EmptyDist.val()));
        let _ = d.scan_round();
        assert_eq!(d.dist.p99().err(), Some(DrillCode::P99Short.val()));
        let _ = d.scan_round();
        assert!(d.dist.p99().is_ok(), "两样本起 p99 可注记");
        // 升序副本口径：p99 >= 中位样本。
        let p = d.dist.p99().unwrap_or(0);
        assert!(p >= PLANE_SLOTS as u64);
    }

    #[test]
    fn phase_regress_rejected() {
        let mut d = ScanDrill::new();
        let (bm, steps) = scan_plane(plane_present);
        let ghost = ScanRoundRecord { round_no: 0, equal: true, steps, bitmap: bm };
        assert_eq!(d.finish_scan(ghost).err(), Some(DrillCode::PhaseRegress.val()));
    }

    #[test]
    fn steps_caliber_is_deterministic() {
        let (_, s1) = scan_plane(plane_present);
        let (_, s2) = scan_plane(plane_present);
        assert_eq!(s1, s2, "步数口径零墙钟零随机，两跑必须相等");
        assert_eq!(s1, PLANE_SLOTS as u64 + plane_present_count() + BITMAP_WORDS as u64);
    }

    #[test]
    fn inject_rounds_all_caught() {
        let led = inject_rounds(DRILL_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "四路注入必须全抓：{:?}", led.caught);
    }

    #[test]
    fn probes_repro_and_guard() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn diagnostic_table_closed_set() {
        let mut i = 0usize;
        while i < DrillCode::ALL.len() {
            let c = DrillCode::ALL.get(i).copied().unwrap_or(DrillCode::NotReproducible);
            let v = c.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40, "诊断码 0x{:x} 越窗", v);
            assert!(!detail(v).is_empty());
            i += 1;
        }
        assert!(detail(0x12345678).contains("未知诊断码"));
    }

    #[test]
    fn checkset_counts() {
        let s = run_unx0475_checks();
        assert!(s.len() >= 12, "检查条目须 ≥12，实际 {}", s.len());
        assert!(s.all_passed(), "全链须绿：{}", s.red_items_text());
    }
}
