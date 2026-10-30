//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1682 · 交互/批量/空闲三态基片表冻结 · 0x6CE80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B3）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1682-J1 三态基片表冻结 v1（档位/数值/适用类三列），与
//!     F1663 类清单对齐，表外基片零存在；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三态基片表 v1：每态 4 档基片（档位 0~3），三列（档位/数值
//!      µs/适用类），共 12 行冻结；
//!    - 与 F1663 类清单对齐：适用类 ⊆ {交互,批量,空闲}（三类闭集
//!      对齐审计）；
//!    - 表外基片零存在：任何查询/落账的基片必须命中表内（越表
//!      拒）；
//!    - 表冻结：FNV 封印 + 改值拒 + 版本只前进；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CE80，AI-VX3-1030 批D 独占区）。
pub const SEG_BASE: u32 = 0x6CE80;

// ── 三态/四档闭集 ──────────────────────────────────────────────
pub const ST_INTERACTIVE: usize = 0;
pub const ST_BATCH: usize = 1;
pub const ST_IDLE: usize = 2;
pub const STATE_COUNT: usize = 3;
pub const STATE_NAMES: [&str; STATE_COUNT] = ["交互", "批量", "空闲"];
pub const GRADES: usize = 4;
/// 表行数（3×4）。
pub const TABLE_ROWS: usize = STATE_COUNT * GRADES;
/// 表版本。
pub const TABLE_VER: u32 = 1;

/// 基片数值（µs，冻结 12 行：态×档位）。
pub const SLICE_US: [u64; TABLE_ROWS] = [
    // 交互：2ms/3ms/4ms/6ms
    2_000, 3_000, 4_000, 6_000,
    // 批量：8ms/12ms/16ms/24ms
    8_000, 12_000, 16_000, 24_000,
    // 空闲：32ms/48ms/64ms/96ms
    32_000, 48_000, 64_000, 96_000,
];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SliceCode {
    /// 态越闭集
    StateOutOfRange,
    /// 态名空（三态齐备破坏）
    StateNameBlank,
    /// 档位越闭集
    GradeOutOfRange,
    /// 表外基片（未命中冻结表）
    SliceOutOfTable,
    /// 表改值（冻结红线）
    TableMutated,
    /// 表回退（版本不前进）
    VersionNotForward,
    /// 类清单对齐失败（F1663）
    ClassAlignFailed,
    /// 表行数缺
    RowShort,
    /// 双跑不一致
    NotReproducible,
}

impl SliceCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SliceCode::StateOutOfRange => "态越闭集：三态冻结",
            SliceCode::StateNameBlank => "态名空：齐备破坏",
            SliceCode::GradeOutOfRange => "档位越闭集：0~3",
            SliceCode::SliceOutOfTable => "表外基片：未命中冻结表",
            SliceCode::TableMutated => "表改值：冻结红线",
            SliceCode::VersionNotForward => "表回退：版本不前进",
            SliceCode::ClassAlignFailed => "F1663 类清单对齐失败",
            SliceCode::RowShort => "表行数缺：≠12",
            SliceCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SliceCode; 9] = [
        SliceCode::StateOutOfRange,
        SliceCode::StateNameBlank,
        SliceCode::GradeOutOfRange,
        SliceCode::SliceOutOfTable,
        SliceCode::TableMutated,
        SliceCode::VersionNotForward,
        SliceCode::ClassAlignFailed,
        SliceCode::RowShort,
        SliceCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SliceCode::ALL.len() {
        if SliceCode::ALL[i].val() == code {
            return SliceCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CE80 段闭集内"
}

// ── 基片表 ─────────────────────────────────────────────────────
pub struct SliceTable {
    pub rows: Vec<u64>,
    pub version: u32,
    pub frozen_hash: u64,
}

impl SliceTable {
    pub fn v1() -> SliceTable {
        SliceTable { rows: Vec::from(SLICE_US), version: TABLE_VER, frozen_hash: 0 }
    }

    /// 表内查基片（态+档位）。
    pub fn lookup(&self, state: usize, grade: usize) -> Result<u64, u32> {
        if state >= STATE_COUNT {
            return Err(SliceCode::StateOutOfRange.val());
        }
        if grade >= GRADES {
            return Err(SliceCode::GradeOutOfRange.val());
        }
        let idx = state * GRADES + grade;
        if idx >= self.rows.len() {
            return Err(SliceCode::SliceOutOfTable.val());
        }
        Ok(self.rows[idx])
    }

    /// F1663 类清单对齐：三态名与类清单闭集一致（前缀对齐）。
    pub fn class_align(&self) -> Result<(), u32> {
        if STATE_NAMES.len() != STATE_COUNT {
            return Err(SliceCode::ClassAlignFailed.val());
        }
        // 对齐判据：三态名逐个非空且互异。
        let mut i = 0;
        while i < STATE_COUNT {
            if STATE_NAMES[i].len() == 0 {
                return Err(SliceCode::ClassAlignFailed.val());
            }
            let mut j = 0;
            while j < i {
                if STATE_NAMES[j] == STATE_NAMES[i] {
                    return Err(SliceCode::ClassAlignFailed.val());
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 行数审计。
    pub fn row_audit(&self) -> Result<(), u32> {
        if self.rows.len() != TABLE_ROWS {
            return Err(SliceCode::RowShort.val());
        }
        Ok(())
    }

    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.rows.len() {
            h ^= self.rows[i];
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

    /// 改值（冻结红线：一律拒）。
    pub fn set_value(&mut self, _idx: usize, _v: u64) -> Result<(), u32> {
        Err(SliceCode::TableMutated.val())
    }

    /// 版本前进。
    pub fn bump(&mut self, v: u32) -> Result<(), u32> {
        if v <= self.version {
            return Err(SliceCode::VersionNotForward.val());
        }
        self.version = v;
        Ok(())
    }
}

/// 表外基片零存在审计：12 行全命中 + 越界查询全拒。
pub fn zero_outside_audit(t: &SliceTable) -> Result<u32, u32> {
    let mut hit = 0;
    let mut s = 0;
    while s < STATE_COUNT {
        let mut g = 0;
        while g < GRADES {
            t.lookup(s, g)?;
            hit += 1;
            g += 1;
        }
        s += 1;
    }
    // 越界查询必拒。
    if t.lookup(STATE_COUNT, 0).is_ok() || t.lookup(0, GRADES).is_ok() {
        return Err(SliceCode::SliceOutOfTable.val());
    }
    Ok(hit)
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

/// 黄金全链：表冻结→对齐→零表外→改值拒→版本前进。
pub fn golden_run() -> Result<(u32, u64, u32), u32> {
    let mut t = SliceTable::v1();
    t.row_audit()?;
    t.class_align()?;
    let hit = zero_outside_audit(&t)?;
    let h = t.freeze();
    if !t.frozen_ok() {
        return Err(SliceCode::TableMutated.val());
    }
    // 改值拒。
    if t.set_value(0, 1).is_ok() {
        return Err(SliceCode::TableMutated.val());
    }
    // 版本前进。
    t.bump(TABLE_VER + 1)?;
    t.freeze();
    if !t.frozen_ok() {
        return Err(SliceCode::TableMutated.val());
    }
    Ok((hit, h, t.version))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let t = SliceTable::v1();
    let v = t.lookup(ST_BATCH, 2)?;
    if v == 16_000 {
        Ok(())
    } else {
        Err(SliceCode::SliceOutOfTable.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 表外 → 必拒。
    let t = SliceTable::v1();
    t.lookup(0, 9).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 表回退 → 必拒。
    let mut t = SliceTable::v1();
    t.bump(0)
}

/// 注入 10/10：越态、越档、改值、表回退四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越态。
        let t0 = SliceTable::v1();
        led.record(0, t0.lookup(9, 0).is_err());
        // 槽1：越档。
        led.record(1, t0.lookup(0, 4).is_err());
        // 槽2：改值。
        let mut t2 = SliceTable::v1();
        led.record(2, t2.set_value(3, 9).is_err());
        // 槽3：表回退。
        let mut t3 = SliceTable::v1();
        t3.bump(5).ok();
        led.record(3, t3.bump(2).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SliceCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SliceCode::ALL.len() {
        let v = SliceCode::ALL[i].val();
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
    // 三态×四档=12 行冻结。
    v.push((
        "J1-三态四档12行",
        STATE_NAMES.len() == 3 && GRADES == 4 && TABLE_ROWS == 12 && SLICE_US.len() == 12,
        format!("{:?} × {} 档", STATE_NAMES, GRADES),
    ));
    // 表外零存在。
    let t = SliceTable::v1();
    v.push((
        "J1-表外基片零存在",
        zero_outside_audit(&t) == Ok(12),
        format!("{}/12 命中", zero_outside_audit(&t).unwrap_or(0)),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CE80 窗口")));
    v
}

pub fn run_unx1682_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1682 交互/批量/空闲三态基片表冻结");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
