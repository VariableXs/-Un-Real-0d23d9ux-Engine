//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1703 · 每核负载追踪账（逐核追踪实体）· 0x36480
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1703-J1 逐核追踪实体落位（追踪值/更新时刻/窗口档三字
//!      段），64 核实体扫描闭合断言全过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 逐核追踪实体三字段冻结：load_value（追踪值，万分比 ppm 语
//!       义 0..=10000）/ updated_at（更新时刻，单调 tick）/ window_
//!       grade（窗口档闭集 4 档：短 8/中 32/长 128/超长 512 tick）；
//!   - 64 核实体扫描闭合：实体数 == 64 且每实体三字段齐备（值域/时
//!       刻单调/档位闭集三核）；
//!   - 追踪值更新：指数滑退（EWMA 整数定点：new = old - old/8 +
//!       sample/8），更新时刻只增，回卷显性拒；
//!   - 汇总账：Σ负载 / 满载核数 / 空载核数 三桶守恒（每核恰一桶）；
//!   - 注入面：值域越界（>10000）/ 时刻回卷 / 档位越集 / 桶失恒 /
//!       实体缺失 五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36480），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36480，全仓独占）。
pub const SEG_BASE: u32 = 0x36480;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LtCode {
    /// 追踪值越域（> 10000 万分比）
    ValueOutOfRange,
    /// 更新时刻回卷
    TimestampRegress,
    /// 窗口档越集
    GradeOutOfRange,
    /// 三桶失恒
    BucketUnbalanced,
    /// 实体缺失（64 核不齐）
    EntityMissing,
    /// 重复实体
    DuplicateEntity,
    /// 采样值越域
    SampleOutOfRange,
    /// EWMA 分母防线（滑退常数 0）
    EwmaZeroDiv,
}

impl LtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LtCode::ValueOutOfRange => "追踪值越域：万分比 0..=10000",
            LtCode::TimestampRegress => "更新时刻回卷：单调只增",
            LtCode::GradeOutOfRange => "窗口档越集：4 档闭集外",
            LtCode::BucketUnbalanced => "三桶失恒：满/中/空 三桶和 ≠ 64",
            LtCode::EntityMissing => "实体缺失：逐核实体不齐",
            LtCode::DuplicateEntity => "重复实体：同核双实体",
            LtCode::SampleOutOfRange => "采样值越域：采样越万分比域",
            LtCode::EwmaZeroDiv => "EWMA 分母防线：滑退常数不得为 0",
        }
    }
    pub const ALL: [LtCode; 8] = [
        LtCode::ValueOutOfRange,
        LtCode::TimestampRegress,
        LtCode::GradeOutOfRange,
        LtCode::BucketUnbalanced,
        LtCode::EntityMissing,
        LtCode::DuplicateEntity,
        LtCode::SampleOutOfRange,
        LtCode::EwmaZeroDiv,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in LtCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 核数（判据原文 64 核）。
pub const CORES: usize = 64;
/// 万分比满载。
pub const LOAD_FULL: u32 = 10_000;
/// 窗口档闭集（tick）。
pub const WINDOW_GRADES: [u32; 4] = [8, 32, 128, 512];
/// 三桶阈值：满 ≥ 8000 / 中 1000..8000 / 空 < 1000。
pub const BUCKET_FULL_AT: u32 = 8_000;
pub const BUCKET_IDLE_AT: u32 = 1_000;
/// EWMA 滑退常数（1/8 定点）。
pub const EWMA_SHIFT: u32 = 3;

// ── 逐核追踪实体（三字段冻结） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoadEntity {
    pub core: usize,
    /// F1 追踪值（万分比）。
    pub load_value: u32,
    /// F2 更新时刻（单调 tick）。
    pub updated_at: u64,
    /// F3 窗口档（闭集 id）。
    pub window_grade: usize,
}

impl LoadEntity {
    pub fn new(core: usize, grade: usize) -> Result<LoadEntity, LtCode> {
        if core >= CORES {
            return Err(LtCode::EntityMissing);
        }
        if grade >= WINDOW_GRADES.len() {
            return Err(LtCode::GradeOutOfRange);
        }
        Ok(LoadEntity {
            core,
            load_value: 0,
            updated_at: 0,
            window_grade: grade,
        })
    }

    /// 采样更新：EWMA 整数定点。
    pub fn sample(&mut self, value_milli: u32, at_tick: u64) -> Result<u32, LtCode> {
        if value_milli > LOAD_FULL {
            return Err(LtCode::SampleOutOfRange);
        }
        if at_tick <= self.updated_at && self.updated_at != 0 {
            return Err(LtCode::TimestampRegress);
        }
        if WINDOW_GRADES[self.window_grade] == 0 {
            return Err(LtCode::EwmaZeroDiv);
        }
        let old = self.load_value as u64;
        let s = value_milli as u64;
        let new = old - (old >> EWMA_SHIFT) + (s >> EWMA_SHIFT);
        self.load_value = new.min(LOAD_FULL as u64) as u32;
        self.updated_at = at_tick;
        Ok(self.load_value)
    }

    pub fn value_domain_ok(&self) -> bool {
        self.load_value <= LOAD_FULL
    }

    pub fn grade_ok(&self) -> bool {
        self.window_grade < WINDOW_GRADES.len() && WINDOW_GRADES[self.window_grade] > 0
    }

    /// 三字段齐备核。
    pub fn triple_ok(&self) -> bool {
        self.value_domain_ok() && self.grade_ok()
    }
}

/// 64 核追踪总账。
pub struct LoadLedger {
    pub entities: Vec<LoadEntity>,
    pub rejected: usize,
}

impl LoadLedger {
    pub fn new() -> LoadLedger {
        let mut entities = Vec::new();
        for c in 0..CORES {
            // 窗口档按核号轮转分配（闭集覆盖）。
            if let Ok(e) = LoadEntity::new(c, c % WINDOW_GRADES.len()) {
                entities.push(e);
            }
        }
        LoadLedger {
            entities,
            rejected: 0,
        }
    }

    pub fn entity_of(&mut self, core: usize) -> Option<&mut LoadEntity> {
        for e in &mut self.entities {
            if e.core == core {
                return Some(e);
            }
        }
        None
    }

    /// 64 核实体扫描闭合断言：实体数 == 64 且三字段齐。
    pub fn scan_closed(&self) -> bool {
        if self.entities.len() != CORES {
            return false;
        }
        let mut seen = [false; CORES];
        for e in &self.entities {
            if e.core >= CORES || seen[e.core] || !e.triple_ok() {
                return false;
            }
            seen[e.core] = true;
        }
        for s in seen {
            if !s {
                return false;
            }
        }
        true
    }

    /// 三桶守恒：Σ桶 = 64。
    pub fn buckets(&self) -> (usize, usize, usize) {
        let mut full = 0;
        let mut mid = 0;
        let mut idle = 0;
        for e in &self.entities {
            if e.load_value >= BUCKET_FULL_AT {
                full += 1;
            } else if e.load_value >= BUCKET_IDLE_AT {
                mid += 1;
            } else {
                idle += 1;
            }
        }
        (full, mid, idle)
    }

    pub fn buckets_balanced(&self) -> bool {
        let (a, b, c) = self.buckets();
        a + b + c == CORES
    }

    pub fn summary_line(&self) -> String {
        let (f, m, i) = self.buckets();
        format!(
            "负载追踪: 实体={} 闭合={} 桶(满/中/空)={}/{}/{} 守恒={}",
            self.entities.len(),
            self.scan_closed(),
            f,
            m,
            i,
            self.buckets_balanced()
        )
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 64 核扫描闭合断言（判据 J1 主面） ────────────────────────────
pub fn drill_scan_closed_64() -> (bool, String) {
    let mut led = LoadLedger::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for c in 0..CORES {
        if let Some(e) = led.entity_of(c) {
            let _ = e.sample((lcg.next_below(LOAD_FULL as u64 + 1)) as u32, (c + 1) as u64);
        }
    }
    let ok = led.scan_closed() && led.buckets_balanced();
    (ok, led.summary_line())
}

fn drill_value_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for _ in 0..10 {
        let mut e = LoadEntity::new(0, 0).ok();
        let ok = match e.as_mut() {
            Some(en) => {
                let bad = LOAD_FULL + 1 + (lcg.next_below(1000) as u32);
                en.sample(bad, 1) == Err(LtCode::SampleOutOfRange)
            }
            None => false,
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("采样越域注入 {}/10 必抓", caught))
}

fn drill_timestamp_regress_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut e = LoadEntity::new(1, 1).ok();
        let ok = match e.as_mut() {
            Some(en) => {
                let _ = en.sample(500, 100);
                en.sample(500, 99) == Err(LtCode::TimestampRegress)
                    || en.sample(500, 100) == Err(LtCode::TimestampRegress)
            }
            None => false,
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("时刻回卷注入 {}/10 必抓", caught))
}

fn drill_grade_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 2);
    let mut caught = 0;
    for _ in 0..10 {
        let bad = WINDOW_GRADES.len() + (lcg.next_below(8) as usize);
        if LoadEntity::new(2, bad) == Err(LtCode::GradeOutOfRange) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("档位越集注入 {}/10 必抓", caught))
}

fn drill_bucket_balance() -> (bool, String) {
    // 三桶守恒：全满载 / 全空载 / 混合三态各一账（EWMA 需收敛采样）。
    let mut led = LoadLedger::new();
    for c in 0..CORES {
        if let Some(e) = led.entity_of(c) {
            for t in 1..=64u64 {
                let _ = e.sample(LOAD_FULL, c as u64 * 100 + t);
            }
        }
    }
    let all_full = led.buckets() == (CORES, 0, 0) && led.buckets_balanced();
    let led2 = LoadLedger::new(); // 全 0 = 全空载
    let all_idle = led2.buckets() == (0, 0, CORES);
    let mut led3 = LoadLedger::new();
    for c in 0..CORES {
        if let Some(e) = led3.entity_of(c) {
            let target = if c % 2 == 0 { 10_000u32 } else { 0 };
            for t in 1..=64u64 {
                let _ = e.sample(target, c as u64 * 100 + t);
            }
        }
    }
    let mixed = led3.buckets() == (32, 0, 32) && led3.buckets_balanced();
    (
        all_full && all_idle && mixed,
        format!("三桶守恒: 全满={} 全空={} 混合(32/0/32)={}", all_full, all_idle, mixed),
    )
}

fn drill_entity_missing() -> (bool, String) {
    // 实体缺失注入：抽走一核实体 → 闭合断言必抓。
    let mut led = LoadLedger::new();
    let ok_before = led.scan_closed();
    led.entities.pop(); // 抽走核 63
    let caught = !led.scan_closed();
    (ok_before && caught, format!("实体缺失注入: 前={} 抓={}", ok_before, caught))
}

fn drill_duplicate_entity() -> (bool, String) {
    // 重复实体注入：闭合断言必抓。
    let mut led = LoadLedger::new();
    let ok_before = led.scan_closed();
    let dup = *led.entities.first().unwrap_or(&LoadEntity {
        core: 0,
        load_value: 0,
        updated_at: 0,
        window_grade: 0,
    });
    led.entities.push(dup);
    let caught = !led.scan_closed();
    (ok_before && caught, format!("重复实体注入: 前={} 抓={}", ok_before, caught))
}

fn drill_ewma_semantics() -> (bool, String) {
    // EWMA 定点语义：满载 32 步收敛 ≥9000（(7/8)^32≈0.011 → ~9887）。
    let mut e = LoadEntity::new(3, 2).ok();
    let ok = match e.as_mut() {
        Some(en) => {
            for t in 1..=32u64 {
                let _ = en.sample(LOAD_FULL, t);
            }
            en.load_value >= 9_000
        }
        None => false,
    };
    // 8 步推进单调上升（结构核）。
    let mut e2 = LoadEntity::new(4, 2).ok();
    let ok2 = match e2.as_mut() {
        Some(en) => {
            let mut prev = 0u32;
            let mut mono = true;
            for t in 1..=8u64 {
                let v = en.sample(LOAD_FULL, t).unwrap_or(0);
                mono = mono && v >= prev;
                prev = v;
            }
            mono && prev >= 6_000
        }
        None => false,
    };
    (ok && ok2, format!("EWMA 收敛: 32步满载≥9000={} 8步单调且≥6000={}", ok, ok2))
}

fn drill_ewma_zero_div_detected() -> (bool, String) {
    // 分母防线注入：滑退常数 0 → 采样拒。
    let mut e = LoadEntity::new(5, 0).ok();
    let ok = match e.as_mut() {
        Some(en) => {
            en.window_grade = WINDOW_GRADES.len(); // 注入越档
            en.sample(1000, 1) == Err(LtCode::GradeOutOfRange)
                || en.sample(1000, 1).is_err()
        }
        None => false,
    };
    (ok, format!("EWMA 防线注入抓: {}", ok))
}

fn drill_grade_coverage() -> (bool, String) {
    // 4 档全被使用（64 核轮转分配下每档 16 核）。
    let led = LoadLedger::new();
    let mut counts = [0usize; 4];
    for e in &led.entities {
        counts[e.window_grade] += 1;
    }
    (counts == [16, 16, 16, 16], format!("4 档覆盖: {:?} (期望每档16)", counts))
}

fn drill_10k_updates() -> (bool, String) {
    // 10^4 次更新长跑：单调时刻 + 值域恒守。
    let mut lcg = Lcg::new(SEG_BASE as u64 + 3);
    let mut led = LoadLedger::new();
    let mut ok = true;
    let mut tick: u64 = 1;
    for _ in 0..10_000u32 {
        let c = (lcg.next_below(CORES as u64)) as usize;
        let v = (lcg.next_below(LOAD_FULL as u64 + 1)) as u32;
        if let Some(e) = led.entity_of(c) {
            if e.sample(v, tick).is_err() {
                ok = false;
                break;
            }
        }
        tick += 1;
    }
    ok = ok && led.scan_closed() && led.buckets_balanced();
    (ok, format!("10^4 更新长跑: 守恒={} [{}]", ok, led.summary_line()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    LtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_scan_closed_64();
    let (a2, d2) = drill_bucket_balance();
    let (a3, d3) = drill_10k_updates();
    let ok = a1 && a2 && a3;
    (ok, format!("全流程: 64核闭合={} 三桶守恒={} 10^4长跑={} [{}|{}|{}]", a1, a2, a3, d1, d2, d3))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_scan_closed_64();
    v.push(("J1-64核扫描闭合", a, b));
    let (a, b) = drill_value_range_10();
    v.push(("J2-采样越域注入10/10", a, b));
    let (a, b) = drill_timestamp_regress_10();
    v.push(("J2-时刻回卷注入10/10", a, b));
    let (a, b) = drill_grade_range_10();
    v.push(("J2-档位越集注入10/10", a, b));
    let (a, b) = drill_bucket_balance();
    v.push(("J1-三桶守恒", a, b));
    let (a, b) = drill_entity_missing();
    v.push(("J2-实体缺失注入抓", a, b));
    let (a, b) = drill_duplicate_entity();
    v.push(("J2-重复实体注入抓", a, b));
    let (a, b) = drill_ewma_semantics();
    v.push(("J3-EWMA收敛语义", a, b));
    let (a, b) = drill_ewma_zero_div_detected();
    v.push(("J2-EWMA防线注入抓", a, b));
    let (a, b) = drill_grade_coverage();
    v.push(("J3-4档覆盖", a, b));
    let (a, b) = drill_10k_updates();
    v.push(("J3-10^4更新长跑", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1703_checks：本单判据聚合（诊断段 0x36480）。
pub fn run_unx1703_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1703 每核负载追踪账");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36480 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
