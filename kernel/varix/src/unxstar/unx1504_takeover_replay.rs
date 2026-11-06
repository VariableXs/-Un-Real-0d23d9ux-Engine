//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1504 接管等价回归复跑 · 0x76200段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定）：
//!   回归态复跑（五步协议回归态复跑），复跑结果与首轮留痕全等（位图/
//!   账本/指纹三面全等），复跑漂移注入必抓。
//! 设计要点：
//!   1. 五步协议冻结表（F1389 接管五步：测绘对照/逐点退役/等价验证/
//!      回退预备/接管封账），每步产出检查位集（u32 位图）与账本行，
//!      复跑按同一冻结口径重放，不许临场改口径；
//!   2. 首轮留痕三面：位图（每步一个 u32 位集，位号 = 检查项序号）+
//!      账本（逐行 String，行序 = 步序）+ 指纹（FNV-1a 32 位全链压缩），
//!      首轮封账后三面转为只读留痕；
//!   3. 复跑逐面全等断言：位图逐步相等 且 账本逐行相等 且 指纹相等，
//!      三面任一不等即显性 Err，不许"大体一致就算过"；
//!   4. 漂移注入三路（改位图位 / 改账本行 / 改指纹）× 10 轮，复跑
//!      逐面对拍必抓，抓不住即红线；
//!   5. 复跑账只追加：落账接口唯一（push 复跑行），无删改路径，行号
//!      单调递增，账长单调不减；
//!   6. 状态机单调：待首轮 → 首轮封账 → 复跑完成，倒退（封账后再跑
//!      首轮、完成后重回封账）显性拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x76200，AI-VX7-1104 批7 O 组独占区）。
pub const SEG_BASE: u32 = 0x76200;

/// 五步协议步数（冻结 5，F1389 口径）。
pub const STEP_COUNT: usize = 5;
/// 每步检查位集容量（u32 位图，位号 0..32）。
pub const BITS_PER_STEP: u32 = 32;
/// 漂移注入路数（位图/账本/指纹 三路）。
pub const DRIFT_ROUTES: usize = 3;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 五步协议冻结表（F1389 接管五步，步名逐字锚定） ───────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepSpec {
    pub id: usize,
    pub name: &'static str,
}

pub const STEP_TABLE: [StepSpec; STEP_COUNT] = [
    StepSpec { id: 0, name: "测绘对照" },
    StepSpec { id: 1, name: "逐点退役" },
    StepSpec { id: 2, name: "等价验证" },
    StepSpec { id: 3, name: "回退预备" },
    StepSpec { id: 4, name: "接管封账" },
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakeoverCode {
    /// 步序号越界：不在五步冻结表内
    StepOver,
    /// 状态机倒退：封账/复跑后禁回前态
    StateBackward,
    /// 复跑漂移：留痕被改而复跑未抓（红线）
    DriftEscaped,
    /// 三面不全等：位图/账本/指纹任一失配
    FaceMismatch,
    /// 复跑账回卷：账长缩短或行号回退
    LedgerRewind,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl TakeoverCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TakeoverCode::StepOver => "步序号越界：不在五步冻结表内，建议按 STEP_TABLE 0..5 取号",
            TakeoverCode::StateBackward => "状态机倒退：复跑态不可回首轮态，建议新开回归现场",
            TakeoverCode::DriftEscaped => "漂移逃逸：留痕被改复跑未抓，建议核对三面对拍口径",
            TakeoverCode::FaceMismatch => "三面不全等：位图/账本/指纹逐面定位失配面后重跑",
            TakeoverCode::LedgerRewind => "复跑账回卷：账只追加不许删改，建议封账重开",
            TakeoverCode::NotReproducible => "双跑不一致：复现红线，建议固定种子后对拍",
        }
    }
    pub const ALL: [TakeoverCode; 6] = [
        TakeoverCode::StepOver,
        TakeoverCode::StateBackward,
        TakeoverCode::DriftEscaped,
        TakeoverCode::FaceMismatch,
        TakeoverCode::LedgerRewind,
        TakeoverCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TakeoverCode::ALL.len() {
        if let Some(c) = TakeoverCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x76200 段闭集内"
}

// ── FNV-1a 32 位指纹（整数运算，无浮点无随机） ───────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h = h.wrapping_mul(0x0100_0193) ^ (b as u32);
    }
    h
}

// ── LCG（种子固定，禁止墙钟/真随机） ────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 首轮留痕三面 ────────────────────────────────────────────────
/// 回归态复跑首轮留痕：位图 + 账本 + 指纹 三面。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunTrace {
    /// 每步检查位集（位图面）。
    pub bitmap: [u32; STEP_COUNT],
    /// 逐行账本（账本面，行序 = 步序）。
    pub ledger: Vec<String>,
    /// 全链 FNV-1a 指纹（指纹面）。
    pub fingerprint: u32,
}

impl RunTrace {
    /// 指纹复算：对位图字节 + 账本行重算 FNV，须与存档指纹相等。
    pub fn recomputed_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut s = 0;
        while s < STEP_COUNT {
            if let Some(bm) = self.bitmap.get(s) {
                h = h.wrapping_mul(0x0100_0193) ^ (bm.wrapping_mul(0x9E37_79B9) & 0xFFFF_FFFF);
                h = h.wrapping_mul(0x0100_0193) ^ (s as u32);
            }
            s += 1;
        }
        for line in &self.ledger {
            h = fnv1a_mix(h, line.as_bytes());
        }
        h
    }
}

fn fnv1a_mix(h: u32, bytes: &[u8]) -> u32 {
    let mut acc = h;
    for &b in bytes {
        acc = acc.wrapping_mul(0x0100_0193) ^ (b as u32);
    }
    acc
}

/// 按冻结口径生成一轮五步产出（确定性：同种子必同迹）。
pub fn trace_of_run(seed: u64) -> RunTrace {
    let mut rng = Lcg::new(seed);
    let mut bitmap = [0u32; STEP_COUNT];
    let mut ledger: Vec<String> = Vec::new();
    let mut s = 0;
    while s < STEP_COUNT {
        let name = match STEP_TABLE.get(s) {
            Some(sp) => sp.name,
            None => "越界步",
        };
        // 每步落 3 个检查位（确定性位号），位图面。
        let mut k = 0;
        while k < 3 {
            let bit = rng.below(BITS_PER_STEP as u64) as u32;
            if let Some(bm) = bitmap.get_mut(s) {
                *bm |= 1u32 << (bit % BITS_PER_STEP);
            }
            k += 1;
        }
        // 账本行（账本面）：步序 + 步名 + 位图摘要。
        let bm = match bitmap.get(s) {
            Some(v) => *v,
            None => 0,
        };
        ledger.push(format!("步{}·{}·位图0x{:08X}", s, name, bm));
        s += 1;
    }
    let fp = {
        let t = RunTrace { bitmap, ledger: ledger.clone(), fingerprint: 0 };
        t.recomputed_fp()
    };
    RunTrace { bitmap, ledger, fingerprint: fp }
}

// ── 复跑状态机（单调：待首轮 → 首轮封账 → 复跑完成） ─────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayPhase {
    /// 待首轮
    AwaitingFirst,
    /// 首轮封账（留痕只读）
    FirstSealed,
    /// 复跑完成
    Replayed,
}

impl ReplayPhase {
    fn ordinal(self) -> u8 {
        match self {
            ReplayPhase::AwaitingFirst => 0,
            ReplayPhase::FirstSealed => 1,
            ReplayPhase::Replayed => 2,
        }
    }
}

/// 复跑逐面对拍报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceReport {
    pub bitmap_equal: bool,
    pub ledger_equal: bool,
    pub fingerprint_equal: bool,
    pub faces_equal_count: u32,
}

/// 复跑账行（只追加）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayRow {
    pub seq: u64,
    pub text: String,
}

/// 接管等价回归复跑机。
#[derive(Clone, Debug)]
pub struct TakeoverReplay {
    pub phase: ReplayPhase,
    pub first: Option<RunTrace>,
    pub replay: Option<RunTrace>,
    /// 复跑账（只追加）。
    pub rows: Vec<ReplayRow>,
    /// 复跑账行号游标（单调递增）。
    row_seq: u64,
    /// 漂移注入记账：tries/caught 按三路。
    pub drift_tries: [u32; DRIFT_ROUTES],
    pub drift_caught: [u32; DRIFT_ROUTES],
}

impl TakeoverReplay {
    pub fn new() -> TakeoverReplay {
        TakeoverReplay {
            phase: ReplayPhase::AwaitingFirst,
            first: None,
            replay: None,
            rows: Vec::new(),
            row_seq: 0,
            drift_tries: [0; DRIFT_ROUTES],
            drift_caught: [0; DRIFT_ROUTES],
        }
    }

    fn push_row(&mut self, text: &str) {
        self.row_seq += 1;
        self.rows.push(ReplayRow { seq: self.row_seq, text: String::from(text) });
    }

    /// 首轮：按冻结口径产出留痕并封账（单调态：仅待首轮可入）。
    pub fn first_run(&mut self, seed: u64) -> Result<usize, u32> {
        if self.phase.ordinal() > ReplayPhase::AwaitingFirst.ordinal() {
            return Err(TakeoverCode::StateBackward.val());
        }
        let t = trace_of_run(seed);
        let lines = t.ledger.len();
        self.first = Some(t);
        self.phase = ReplayPhase::FirstSealed;
        self.push_row("首轮封账·留痕三面入账");
        Ok(lines)
    }

    /// 复跑：重放同口径产出并与首轮留痕逐面全等对拍。
    pub fn replay(&mut self, seed: u64) -> Result<FaceReport, u32> {
        match self.phase {
            ReplayPhase::AwaitingFirst => Err(TakeoverCode::FaceMismatch.val()),
            ReplayPhase::FirstSealed | ReplayPhase::Replayed => {
                let first = match &self.first {
                    Some(t) => t.clone(),
                    None => return Err(TakeoverCode::FaceMismatch.val()),
                };
                let second = trace_of_run(seed);
                let rep = faces_cmp(&first, &second);
                self.replay = Some(second);
                if rep.faces_equal_count == 3 {
                    self.phase = ReplayPhase::Replayed;
                    self.push_row("复跑·三面全等·绿");
                    Ok(rep)
                } else {
                    self.push_row("复跑·三面失配·红（漂移必抓）");
                    Err(TakeoverCode::FaceMismatch.val())
                }
            }
        }
    }

    /// 漂移注入：改首轮留痕后复跑，必须被抓（三面失配即抓）。
    /// 路号：0=改位图 1=改账本 2=改指纹。
    pub fn inject_drift(&mut self, seed: u64, route: usize, round: u32) -> Result<bool, u32> {
        if route >= DRIFT_ROUTES {
            return Err(TakeoverCode::StepOver.val());
        }
        // 取首轮留痕副本并注入漂移。
        let mut tainted = match &self.first {
            Some(t) => t.clone(),
            None => return Err(TakeoverCode::FaceMismatch.val()),
        };
        let changed = match route {
            0 => {
                // 改位图：翻转第 round%5 步的低位。
                let step = (round as usize) % STEP_COUNT;
                match tainted.bitmap.get_mut(step) {
                    Some(bm) => {
                        *bm ^= 1u32;
                        true
                    }
                    None => false,
                }
            }
            1 => {
                // 改账本：替换一行文本。
                let idx = (round as usize) % tainted.ledger.len().max(1);
                match tainted.ledger.get_mut(idx) {
                    Some(line) => {
                        *line = format!("步{}·漂移注入行·0x{:08X}", idx, round);
                        true
                    }
                    None => false,
                }
            }
            _ => {
                // 改指纹：直接翻动存档指纹。
                tainted.fingerprint = tainted.fingerprint.wrapping_add(1);
                true
            }
        };
        // 路号已守卫（route < DRIFT_ROUTES），取槽记 try。
        match self.drift_tries.get_mut(route) {
            Some(t) => *t += 1,
            None => return Err(TakeoverCode::StepOver.val()),
        }
        // 对拍：污染迹 vs 同口径重放迹（漂移必须真实改动过留痕）。
        let clean = trace_of_run(seed);
        let rep = faces_cmp(&tainted, &clean);
        let caught = changed && rep.faces_equal_count < 3;
        if caught {
            if let Some(c) = self.drift_caught.get_mut(route) {
                *c += 1;
            }
            self.push_row("漂移注入·复跑对拍必抓");
        } else {
            self.push_row("漂移注入·逃逸（红线）");
        }
        Ok(caught)
    }

    /// 复跑账只追加守卫：账长单调不减且行号严格递增连续。
    pub fn ledger_append_only_ok(&self) -> bool {
        let mut prev_seq: u64 = 0;
        for row in &self.rows {
            if row.seq <= prev_seq {
                return false;
            }
            if row.seq != prev_seq + 1 {
                return false;
            }
            prev_seq = row.seq;
        }
        true
    }
}

/// 三面对拍（纯函数，供复跑与注入双路复用）。
pub fn faces_cmp(a: &RunTrace, b: &RunTrace) -> FaceReport {
    let bitmap_equal = {
        let mut eq = true;
        let mut s = 0;
        while s < STEP_COUNT {
            let (x, y) = match (a.bitmap.get(s), b.bitmap.get(s)) {
                (Some(x), Some(y)) => (*x, *y),
                _ => {
                    eq = false;
                    break;
                }
            };
            if x != y {
                eq = false;
                break;
            }
            s += 1;
        }
        eq
    };
    let ledger_equal = a.ledger == b.ledger;
    let fingerprint_equal = a.fingerprint == b.fingerprint;
    let mut n = 0;
    if bitmap_equal {
        n += 1;
    }
    if ledger_equal {
        n += 1;
    }
    if fingerprint_equal {
        n += 1;
    }
    FaceReport {
        bitmap_equal,
        ledger_equal,
        fingerprint_equal,
        faces_equal_count: n,
    }
}

/// 真机五步留痕（冻结种子：批 7 O 组口径 seed = SEG_BASE）。
pub fn machine_seed() -> u64 {
    SEG_BASE as u64
}

// ── 黄金全链：首轮封账 → 复跑全等 → 三路注入全抓 → 账只追加 ──────
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut rig = TakeoverReplay::new();
    let seed = machine_seed();
    let lines = rig.first_run(seed)?;
    if lines != STEP_COUNT {
        return Err(TakeoverCode::FaceMismatch.val());
    }
    let rep = rig.replay(seed)?;
    let faces_ok = (rep.faces_equal_count == 3) as u32;
    let mut caught_all = 0;
    let mut route = 0;
    while route < DRIFT_ROUTES {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            if rig.inject_drift(seed, route, r)? {
                caught_all += 1;
            }
            r += 1;
        }
        route += 1;
    }
    Ok((faces_ok, caught_all))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金链 3 面绿 + 30 路注入全抓。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 30)) => Ok(()),
        _ => Err(TakeoverCode::NotReproducible.val()),
    }
}

/// 序 B：倒退拒——封账后再跑首轮必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut rig = TakeoverReplay::new();
    let seed = machine_seed();
    let _ = rig.first_run(seed);
    match rig.first_run(seed) {
        Err(e) if e == TakeoverCode::StateBackward.val() => Ok(()),
        _ => Err(TakeoverCode::NotReproducible.val()),
    }
}

/// 序 C：空现场直接复跑必拒（FaceMismatch）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut rig = TakeoverReplay::new();
    match rig.replay(machine_seed()) {
        Err(e) if e == TakeoverCode::FaceMismatch.val() => Ok(()),
        _ => Err(TakeoverCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 3 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<([u32; 3], [u32; 3]), u32> {
    let mut rig = TakeoverReplay::new();
    let seed = machine_seed();
    let _ = rig.first_run(seed);
    let mut r = 0;
    while r < rounds {
        let mut route = 0;
        while route < DRIFT_ROUTES {
            let caught = rig.inject_drift(seed, route, r)?;
            if !caught {
                return Err(TakeoverCode::DriftEscaped.val());
            }
            route += 1;
        }
        r += 1;
    }
    Ok((rig.drift_tries, rig.drift_caught))
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TakeoverCode::NotReproducible.val());
    }
    // 留痕本身双跑一致：同种子两迹全等。
    let t1 = trace_of_run(machine_seed());
    let t2 = trace_of_run(machine_seed());
    if t1 != t2 {
        return Err(TakeoverCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口 + 段基对齐与区间冻结） ──────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < TakeoverCode::ALL.len() {
        if let Some(c) = TakeoverCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有，断言方向：末行为成立条件） ──────────────────
/// J1：复跑结果与首轮留痕三面全等。
fn three_faces_ok() -> bool {
    let mut rig = TakeoverReplay::new();
    let seed = machine_seed();
    if rig.first_run(seed).is_err() {
        return false;
    }
    match rig.replay(seed) {
        Ok(rep) => rep.bitmap_equal && rep.ledger_equal && rep.fingerprint_equal,
        Err(_) => false,
    }
}

/// J1：复跑漂移注入必抓（10 轮 × 3 路全抓）。
fn drift_caught_ok() -> bool {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok((tries, caught)) => {
            let mut i = 0;
            while i < DRIFT_ROUTES {
                let (t, c) = match (tries.get(i), caught.get(i)) {
                    (Some(&t), Some(&c)) => (t, c),
                    _ => return false,
                };
                if t != JUDGE_ROUNDS || c != t {
                    return false;
                }
                i += 1;
            }
            true
        }
        Err(_) => false,
    }
}

/// J1：复跑账只追加（行号连续 + 只增不减）。
fn replay_ledger_ok() -> bool {
    let mut rig = TakeoverReplay::new();
    let seed = machine_seed();
    let _ = rig.first_run(seed);
    let len0 = rig.rows.len();
    let _ = rig.replay(seed);
    let len1 = rig.rows.len();
    let _ = rig.inject_drift(seed, 0, 0);
    let len2 = rig.rows.len();
    len0 == 1 && len1 == 2 && len2 == 3 && rig.ledger_append_only_ok()
}

/// J1：五步冻结表逐字（F1389 口径不许漂移）。
fn step_table_ok() -> bool {
    let expect = ["测绘对照", "逐点退役", "等价验证", "回退预备", "接管封账"];
    if STEP_TABLE.len() != STEP_COUNT {
        return false;
    }
    let mut i = 0;
    while i < STEP_COUNT {
        match (STEP_TABLE.get(i), expect.get(i)) {
            (Some(sp), Some(want)) => {
                if sp.id != i || sp.name != *want {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫，全 add_dyn） ─────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1 段位守卫。
    v.push((
        "J0-段位守卫",
        segment_guard(),
        format!("SEG_BASE=0x{:X} 窗口 0x75000..=0x781C0 且 0x40 对齐", SEG_BASE),
    ));
    // 2 五步冻结表。
    v.push(("J1-五步冻结表", step_table_ok(), String::from("F1389 五步名逐字对拍")));
    // 3 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((faces, caught)) => *faces == 1 && *caught == 30,
            Err(_) => false,
        },
        match &g {
            Ok((faces, caught)) => format!("faces={} caught={}/30", faces, caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 4 三面全等主判据。
    v.push((
        "J1-复跑三面全等",
        three_faces_ok(),
        String::from("位图/账本/指纹逐面全等"),
    ));
    // 5 漂移注入 10 轮全抓。
    v.push((
        "J1-漂移注入必抓",
        drift_caught_ok(),
        String::from("10轮×3路（位图/账本/指纹）全抓"),
    ));
    // 6 复跑账只追加。
    v.push((
        "J1-复跑账只追加",
        replay_ledger_ok(),
        String::from("行号连续递增，账长单调不减"),
    ));
    // 7 状态机倒退拒（封账后再跑首轮）。
    v.push((
        "J1-状态机倒退拒",
        probe_seq_b().is_ok(),
        String::from("封账后再跑首轮 ⇒ StateBackward"),
    ));
    // 8 空现场复跑拒。
    v.push((
        "J1-空现场复跑拒",
        probe_seq_c().is_ok(),
        String::from("未封账直接复跑 ⇒ FaceMismatch"),
    ));
    // 9 指纹复算。
    let t = trace_of_run(machine_seed());
    v.push((
        "J1-指纹复算",
        t.recomputed_fp() == t.fingerprint,
        format!("FNV 复算=0x{:08X} 存档=0x{:08X}", t.recomputed_fp(), t.fingerprint),
    ));
    // 10 探针序 A。
    v.push(("J2-探针序A", probe_seq_a().is_ok(), String::from("黄金链 3面+30抓")));
    // 11 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 与留痕两跑同值"),
    ));
    // 12 诊断码闭集查表。
    let unknown = detail(SEG_BASE + 0x3F);
    v.push((
        "J3-诊断码闭集",
        unknown.starts_with("未知") && detail(TakeoverCode::FaceMismatch.val()) == TakeoverCode::FaceMismatch.detail(),
        String::from("闭集查表命中 + 窗口外未知"),
    ));
    v
}

pub fn run_unx1504_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1504 接管等价回归复跑");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_chain_all_green() {
        let g = golden_run();
        assert_eq!(g, Ok((1, 30)), "黄金链应 3 面绿 + 30 路注入全抓");
    }

    #[test]
    fn replay_faces_equal() {
        let mut rig = TakeoverReplay::new();
        let seed = machine_seed();
        assert!(rig.first_run(seed).is_ok());
        let rep = rig.replay(seed);
        match rep {
            Ok(r) => {
                assert!(r.bitmap_equal);
                assert!(r.ledger_equal);
                assert!(r.fingerprint_equal);
                assert_eq!(r.faces_equal_count, 3);
            }
            Err(e) => panic!("复跑应绿 err=0x{:x}", e),
        }
    }

    #[test]
    fn state_backward_rejected() {
        let mut rig = TakeoverReplay::new();
        let seed = machine_seed();
        assert!(rig.first_run(seed).is_ok());
        assert_eq!(rig.first_run(seed), Err(TakeoverCode::StateBackward.val()));
        assert!(rig.replay(seed).is_ok());
        // 复跑完成后重回封账态：再跑首轮仍拒（单调不倒退）。
        assert_eq!(rig.first_run(seed), Err(TakeoverCode::StateBackward.val()));
    }

    #[test]
    fn drift_injection_all_routes_caught() {
        let mut rig = TakeoverReplay::new();
        let seed = machine_seed();
        assert!(rig.first_run(seed).is_ok());
        let mut route = 0;
        while route < DRIFT_ROUTES {
            let mut r = 0;
            while r < JUDGE_ROUNDS {
                match rig.inject_drift(seed, route, r) {
                    Ok(true) => {}
                    other => panic!("路{}轮{} 必抓，实得 {:?}", route, r, other),
                }
                r += 1;
            }
            route += 1;
        }
        let mut i = 0;
        while i < DRIFT_ROUTES {
            let (t, c) = match (rig.drift_tries.get(i), rig.drift_caught.get(i)) {
                (Some(t), Some(c)) => (*t, *c),
                _ => (0, 0),
            };
            assert_eq!(c, t, "路{}全抓", i);
            assert_eq!(t, JUDGE_ROUNDS);
            i += 1;
        }
    }

    #[test]
    fn replay_ledger_append_only() {
        let mut rig = TakeoverReplay::new();
        let seed = machine_seed();
        let _ = rig.first_run(seed);
        let l0 = rig.rows.len();
        let _ = rig.replay(seed);
        let l1 = rig.rows.len();
        assert!(l1 > l0, "复跑必须落账");
        assert!(rig.ledger_append_only_ok(), "行号连续且只增");
        // 行文本非空（留痕不许空行）。
        for row in &rig.rows {
            assert!(!row.text.is_empty());
        }
    }

    #[test]
    fn double_run_identical() {
        let t1 = trace_of_run(machine_seed());
        let t2 = trace_of_run(machine_seed());
        assert_eq!(t1, t2, "同种子双跑留痕必须全等");
        assert!(repro_check().is_ok());
    }

    #[test]
    fn boundary_step_and_route_over() {
        let mut rig = TakeoverReplay::new();
        let seed = machine_seed();
        assert!(rig.first_run(seed).is_ok());
        // 路号越界（=3）显性拒。
        assert_eq!(rig.inject_drift(seed, DRIFT_ROUTES, 0), Err(TakeoverCode::StepOver.val()));
    }

    #[test]
    fn empty_site_replay_rejected() {
        let mut rig = TakeoverReplay::new();
        assert!(rig.replay(machine_seed()).is_err(), "未封账复跑必拒");
    }

    #[test]
    fn segment_window_guard() {
        assert!(segment_guard(), "0x76200 须在 0x75000..=0x781C0 且 0x40 对齐");
        assert_eq!(SEG_BASE % 0x40, 0);
        let mut i = 0;
        while i < TakeoverCode::ALL.len() {
            if let Some(c) = TakeoverCode::ALL.get(i) {
                let v = c.val();
                assert!(v >= SEG_BASE && v < SEG_BASE + 0x40, "诊断码 0x{:X} 越窗", v);
            }
            i += 1;
        }
    }

    #[test]
    fn detail_closed_set_lookup() {
        assert_eq!(
            detail(TakeoverCode::DriftEscaped.val()),
            TakeoverCode::DriftEscaped.detail()
        );
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知"), "窗口外未知码须显性");
        assert_eq!(STEP_TABLE.len(), STEP_COUNT);
    }
}
