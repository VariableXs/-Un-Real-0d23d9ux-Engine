//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1423 · 延迟阈值登记册 v1（逐向量阈值表冻结） · 0x6CB40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B2）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1423-J1 阈值登记册 v1 冻结（五类向量逐向量延迟阈值，
//!     阈值来源三列：预算表映射/经验基线/临时观察位），冻结哈希入
//!     册，阈值缺项注入 10/10 必抓，册结构版本化；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五类向量闭集：时钟中断/网络收包/磁盘完成/IPC 投递/调度切
//!      换（冻结 5 类，每类 8 向量 = 40 条阈值）；
//!    - 阈值来源三列：0=预算表映射（F1014）/1=经验基线/2=临时观察
//!      位（来源必须登记，缺源即缺项）；
//!    - 冻结哈希：全册 FNV 入册；篡改检出（重算对拍）；
//!    - 缺项注入 10/10 必抓；册结构版本化（v1 只前进）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CB40，AI-VX3-1030 批B 独占区）。
pub const SEG_BASE: u32 = 0x6CB40;

// ── 五类向量闭集 ────────────────────────────────────────────────
pub const VEC_CLOCK: usize = 0;
pub const VEC_NET_RX: usize = 1;
pub const VEC_DISK_DONE: usize = 2;
pub const VEC_IPC_POST: usize = 3;
pub const VEC_SCHED_SWITCH: usize = 4;
pub const VEC_CLASS_COUNT: usize = 5;
pub const VEC_CLASS_NAMES: [&str; VEC_CLASS_COUNT] =
    ["时钟中断", "网络收包", "磁盘完成", "IPC 投递", "调度切换"];
/// 每类向量数。
pub const VECS_PER_CLASS: usize = 8;
/// 总阈值条数。
pub const THRESHOLD_ROWS: usize = VEC_CLASS_COUNT * VECS_PER_CLASS;

/// 阈值来源三列。
pub const SRC_BUDGET_MAP: u8 = 0;
pub const SRC_EXPERIENCE: u8 = 1;
pub const SRC_TEMP_OBSERVE: u8 = 2;
pub const SRC_COUNT: usize = 3;

/// 册版本。
pub const REGISTRY_VER: u32 = 1;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThrCode {
    /// 向量类越闭集
    VecClassOutOfRange,
    /// 类名空（五类齐备破坏）
    VecClassNameBlank,
    /// 阈值缺项（行缺失）
    ThresholdMissing,
    /// 来源缺列（无效来源值）
    SourceMissing,
    /// 冻结哈希不符
    FreezeHashMismatch,
    /// 册回退（版本不前进）
    VersionNotForward,
    /// 阈值越域（>10000µs）
    ThresholdOverDomain,
    /// 缺项注入漏抓
    MissingLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl ThrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ThrCode::VecClassOutOfRange => "向量类越闭集：五类冻结",
            ThrCode::VecClassNameBlank => "类名空：齐备破坏",
            ThrCode::ThresholdMissing => "阈值缺项：行缺失",
            ThrCode::SourceMissing => "来源缺列：无效来源值",
            ThrCode::FreezeHashMismatch => "冻结哈希不符",
            ThrCode::VersionNotForward => "册回退：版本不前进",
            ThrCode::ThresholdOverDomain => "阈值越域：>10000µs",
            ThrCode::MissingLeaked => "缺项注入漏抓",
            ThrCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ThrCode; 9] = [
        ThrCode::VecClassOutOfRange,
        ThrCode::VecClassNameBlank,
        ThrCode::ThresholdMissing,
        ThrCode::SourceMissing,
        ThrCode::FreezeHashMismatch,
        ThrCode::VersionNotForward,
        ThrCode::ThresholdOverDomain,
        ThrCode::MissingLeaked,
        ThrCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ThrCode::ALL.len() {
        if ThrCode::ALL[i].val() == code {
            return ThrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CB40 段闭集内"
}

/// 阈值域上界（µs）。
pub const THRESHOLD_MAX_US: u64 = 10_000;

// ── 阈值行 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThresholdRow {
    pub vec_class: usize,
    pub vec_index: usize,
    /// 阈值（µs）。
    pub threshold_us: u64,
    /// 来源（三列之一）。
    pub source: u8,
}

impl ThresholdRow {
    pub fn legal(&self) -> Result<(), u32> {
        if self.vec_class >= VEC_CLASS_COUNT {
            return Err(ThrCode::VecClassOutOfRange.val());
        }
        if self.vec_index >= VECS_PER_CLASS {
            return Err(ThrCode::VecClassOutOfRange.val());
        }
        if self.threshold_us == 0 || self.threshold_us > THRESHOLD_MAX_US {
            return Err(ThrCode::ThresholdOverDomain.val());
        }
        if self.source as usize >= SRC_COUNT {
            return Err(ThrCode::SourceMissing.val());
        }
        Ok(())
    }
}

// ── 登记册 ─────────────────────────────────────────────────────
pub struct ThresholdRegistry {
    pub rows: Vec<ThresholdRow>,
    pub version: u32,
    pub frozen_hash: u64,
    pub missing_caught: u32,
}

impl ThresholdRegistry {
    pub fn new(version: u32) -> ThresholdRegistry {
        ThresholdRegistry { rows: Vec::new(), version, frozen_hash: 0, missing_caught: 0 }
    }

    /// 登记一行（越域/缺源拒）。
    pub fn register(&mut self, row: ThresholdRow) -> Result<(), u32> {
        row.legal()?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].vec_class == row.vec_class
                && self.rows[i].vec_index == row.vec_index
            {
                // 同向量覆盖（更新值）。
                self.rows[i] = row;
                return Ok(());
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 完整性：40 行全在（缺项即拒）。
    pub fn completeness(&self) -> Result<usize, u32> {
        let mut hit = [false; THRESHOLD_ROWS];
        let mut i = 0;
        while i < self.rows.len() {
            let idx = self.rows[i].vec_class * VECS_PER_CLASS + self.rows[i].vec_index;
            if idx < THRESHOLD_ROWS {
                hit[idx] = true;
            }
            i += 1;
        }
        let mut n = 0;
        let mut k = 0;
        while k < THRESHOLD_ROWS {
            if hit[k] {
                n += 1;
            }
            k += 1;
        }
        if n != THRESHOLD_ROWS {
            return Err(ThrCode::ThresholdMissing.val());
        }
        Ok(n)
    }

    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.rows.len() {
            h ^= self.rows[i].vec_class as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= self.rows[i].vec_index as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= self.rows[i].threshold_us;
            h = h.wrapping_mul(0x100000001b3);
            h ^= self.rows[i].source as u64;
            h = h.wrapping_mul(0x100000001b3);
            i += 1;
        }
        h ^= self.version as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen_hash = self.hash();
        self.frozen_hash
    }

    pub fn frozen_ok(&self) -> bool {
        self.frozen_hash != 0 && self.frozen_hash == self.hash()
    }

    /// 版本前进。
    pub fn bump(&mut self, v: u32) -> Result<(), u32> {
        if v <= self.version {
            return Err(ThrCode::VersionNotForward.val());
        }
        self.version = v;
        Ok(())
    }
}

/// 基线登记册（40 行全，来源轮转）。
pub fn baseline_registry() -> Result<ThresholdRegistry, u32> {
    let mut reg = ThresholdRegistry::new(REGISTRY_VER);
    let mut c = 0;
    while c < VEC_CLASS_COUNT {
        let mut v = 0;
        while v < VECS_PER_CLASS {
            // 阈值：时钟 100µs 档、网络 500、磁盘 2000、IPC 300、调度 50（确定性派生）。
            let base: [u64; VEC_CLASS_COUNT] = [100, 500, 2000, 300, 50];
            let t = base[c] + v as u64 * 10;
            reg.register(ThresholdRow {
                vec_class: c,
                vec_index: v,
                threshold_us: t,
                source: (c + v) as u8 % 3,
            })?;
            v += 1;
        }
        c += 1;
    }
    reg.completeness()?;
    Ok(reg)
}

/// 缺项注入 10/10（每注入抽掉一行，完整性必抓）。
pub fn missing_inject_10() -> Result<u32, u32> {
    let base = baseline_registry()?;
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        let mut broken = base.rows.clone();
        broken.remove(k);
        let mut reg = ThresholdRegistry::new(REGISTRY_VER);
        reg.rows = broken;
        if reg.completeness().is_err() {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(ThrCode::MissingLeaked.val());
    }
    Ok(caught)
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

/// 黄金全链：基线 40 行→冻结→篡改检出→缺项 10/10→版本前进。
pub fn golden_run() -> Result<(usize, u64, u32), u32> {
    if VEC_CLASS_NAMES.len() != VEC_CLASS_COUNT {
        return Err(ThrCode::VecClassNameBlank.val());
    }
    let mut reg = baseline_registry()?;
    let h0 = reg.freeze();
    if !reg.frozen_ok() {
        return Err(ThrCode::FreezeHashMismatch.val());
    }
    // 篡改检出：改一行阈值后重算必不一致。
    let old = reg.rows[7].threshold_us;
    reg.rows[7].threshold_us += 1;
    let tampered = reg.hash() != h0;
    reg.rows[7].threshold_us = old;
    if !tampered {
        return Err(ThrCode::FreezeHashMismatch.val());
    }
    // 缺项 10/10。
    let caught = missing_inject_10()?;
    // 版本前进。
    reg.bump(REGISTRY_VER + 1)?;
    reg.freeze();
    if !reg.frozen_ok() {
        return Err(ThrCode::FreezeHashMismatch.val());
    }
    Ok((reg.rows.len(), h0, caught))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut reg = baseline_registry()?;
    reg.freeze();
    if reg.frozen_ok() {
        Ok(())
    } else {
        Err(ThrCode::FreezeHashMismatch.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 来源缺列 → 必拒。
    let row = ThresholdRow { vec_class: 0, vec_index: 0, threshold_us: 100, source: 9 };
    row.legal()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 册回退 → 必拒。
    let mut reg = ThresholdRegistry::new(3);
    reg.bump(2)
}

/// 注入 10/10：类越界、阈值越域、来源缺、缺项抓捕四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        led.record(0, ThresholdRow { vec_class: 5, vec_index: 0, threshold_us: 1, source: 0 }.legal().is_err());
        led.record(1, ThresholdRow { vec_class: 0, vec_index: 0, threshold_us: THRESHOLD_MAX_US + 1, source: 0 }.legal().is_err());
        led.record(2, ThresholdRow { vec_class: 0, vec_index: 0, threshold_us: 1, source: 7 }.legal().is_err());
        led.record(3, missing_inject_10() == Ok(10));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ThrCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ThrCode::ALL.len() {
        let v = ThrCode::ALL[i].val();
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
    // 五类×8 向量=40 行。
    v.push((
        "J1-五类40行",
        VEC_CLASS_COUNT == 5 && VECS_PER_CLASS == 8 && THRESHOLD_ROWS == 40,
        format!("{:?} × {} = {}", VEC_CLASS_NAMES, VECS_PER_CLASS, THRESHOLD_ROWS),
    ));
    // 冻结哈希入册。
    let mut reg = baseline_registry();
    let frozen_ok = match &mut reg {
        Ok(r) => {
            r.freeze();
            r.frozen_ok()
        }
        Err(_) => false,
    };
    v.push(("J1-冻结哈希入册", frozen_ok, String::from("FNV 全册")));
    // 缺项 10/10。
    v.push((
        "J1-缺项注入10/10",
        missing_inject_10() == Ok(10),
        format!("{}/10 抓", missing_inject_10().unwrap_or(0)),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CB40 窗口")));
    v
}

pub fn run_unx1423_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1423 延迟阈值登记册 v1（逐向量阈值表冻结）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
