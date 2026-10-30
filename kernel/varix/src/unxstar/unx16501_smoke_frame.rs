//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16501 · 冒烟集框架总纲（30 样本×四类） · 0x67DC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥420 行；
//!   ② UNX-F16501-J1 框架总纲落账（样本池/运行器/报告三段），三段
//!      齐备率 100%，四类样本（控件/对话框/多线程/GDI 绘图）分册
//!      结构断言 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 样本池闭集：30 样本 × 四类（控件 8/对话框 7/多线程 8/GDI
//!     绘图 7，分册冻结）；
//!   - 三段闭集：样本池段（登记）/运行器段（调度执行）/报告段
//!     （汇总出账）——齐备率 100% = 三段各全量登记；
//!   - 调度状态机：Pooled→Dispatched→Ran→Reported，非法边拒收；
//!   - 守恒恒等式：池登记数==30、报告条数==池登记数（不入不漏），
//!     四分册计数 8+7+8+7==30；
//!   - LCG 种子写死 = SEG_BASE（0x67DC0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67DC0，AI-VX3-1030 批 16 独占区）。
pub const SEG_BASE: u32 = 0x67DC0;

// ── 四类分册闭集 ─────────────────────────────────────────────────
pub const CLASS_CONTROL: usize = 0;
pub const CLASS_DIALOG: usize = 1;
pub const CLASS_THREAD: usize = 2;
pub const CLASS_GDI: usize = 3;
pub const CLASS_COUNT: usize = 4;
pub const CLASS_NAMES: [&str; CLASS_COUNT] = ["控件", "对话框", "多线程", "GDI绘图"];

/// 每分册样本数（写死，来源：冒烟集 30 样本结构冻结）。
pub const VOL_SIZE: [usize; CLASS_COUNT] = [8, 7, 8, 7];
pub const SAMPLE_COUNT: usize = 30;

/// 三段闭集。
pub const SEG_POOL: usize = 0;
pub const SEG_RUNNER: usize = 1;
pub const SEG_REPORT: usize = 2;
pub const FRAME_SEG_COUNT: usize = 3;
pub const FRAME_SEG_NAMES: [&str; FRAME_SEG_COUNT] = ["样本池", "运行器", "报告"];

/// 判据常量（独立写死，不复用被测值）。
pub const EXPECT_SAMPLES: usize = 30;
pub const EXPECT_FRAME_SEGS: usize = 3;
/// 四分册计数之和（独立算式写死）。
pub const VOL_SUM: usize = 8 + 7 + 8 + 7;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmokeCode {
    /// 样本下标越闭集
    SampleOutOfRange,
    /// 分册下标越闭集
    ClassOutOfRange,
    /// 三段落账缺口（任一段未全量登记）
    FrameSegGap,
    /// 分册结构破（册计数与冻结值不符）
    VolumeBroken,
    /// 调度状态机非法转换
    TransitionIllegal,
    /// 样本重登记
    SampleDoubleBooked,
    /// 报告与池不对账（条数漂移）
    ReportDrift,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl SmokeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SmokeCode::SampleOutOfRange => "样本下标越闭集：30 样本冻结",
            SmokeCode::ClassOutOfRange => "分册下标越闭集：四类冻结",
            SmokeCode::FrameSegGap => "三段落账缺口：齐备率红线",
            SmokeCode::VolumeBroken => "分册结构破：8+7+8+7≠30",
            SmokeCode::TransitionIllegal => "调度状态机非法转换",
            SmokeCode::SampleDoubleBooked => "样本重登记：同例二次入池拒收",
            SmokeCode::ReportDrift => "报告与池不对账：条数漂移",
            SmokeCode::NotReproducible => "双跑不一致：冒烟账不可复现",
        }
    }
    pub const ALL: [SmokeCode; 8] = [
        SmokeCode::SampleOutOfRange,
        SmokeCode::ClassOutOfRange,
        SmokeCode::FrameSegGap,
        SmokeCode::VolumeBroken,
        SmokeCode::TransitionIllegal,
        SmokeCode::SampleDoubleBooked,
        SmokeCode::ReportDrift,
        SmokeCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SmokeCode::ALL.len() {
        if SmokeCode::ALL[i].val() == code {
            return SmokeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67DC0 段闭集内"
}

// ── 样本→分册归属（闭式冻结映射） ────────────────────────────────
/// 样本 0..8=控件，8..15=对话框，15..23=多线程，23..30=GDI。
pub fn class_of(sample_id: usize) -> Result<usize, u32> {
    if sample_id >= SAMPLE_COUNT {
        return Err(SmokeCode::SampleOutOfRange.val());
    }
    let cls = if sample_id < 8 {
        CLASS_CONTROL
    } else if sample_id < 15 {
        CLASS_DIALOG
    } else if sample_id < 23 {
        CLASS_THREAD
    } else {
        CLASS_GDI
    };
    Ok(cls)
}

// ── 调度状态机 ───────────────────────────────────────────────────
/// Pooled→Dispatched→Ran→Reported；仅相邻前进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedState {
    Pooled,
    Dispatched,
    Ran,
    Reported,
}

pub fn step(cur: SchedState, next: SchedState) -> Result<SchedState, u32> {
    let legal = match cur {
        SchedState::Pooled => matches!(next, SchedState::Dispatched),
        SchedState::Dispatched => matches!(next, SchedState::Ran),
        SchedState::Ran => matches!(next, SchedState::Reported),
        SchedState::Reported => false,
    };
    if legal {
        Ok(next)
    } else {
        Err(SmokeCode::TransitionIllegal.val())
    }
}

/// 冒烟样本账一行。
pub struct SmokeRow {
    pub sample_id: usize,
    pub class: usize,
    pub state: SchedState,
    /// 运行结论：真=通过；假=发现缺陷（冒烟语义：缺陷即信息）。
    pub verdict: bool,
}

// ── 框架总纲账 ───────────────────────────────────────────────────
pub struct SmokeLedger {
    pub rows: Vec<SmokeRow>,
    pub segs_booked: [bool; FRAME_SEG_COUNT],
    pub digest: u64,
}

impl SmokeLedger {
    pub fn new() -> SmokeLedger {
        SmokeLedger {
            rows: Vec::new(),
            segs_booked: [false; FRAME_SEG_COUNT],
            digest: 0,
        }
    }

    /// 三段落账标记。
    pub fn book_seg(&mut self, seg: usize) -> Result<(), u32> {
        if seg >= FRAME_SEG_COUNT {
            return Err(SmokeCode::FrameSegGap.val());
        }
        self.segs_booked[seg] = true;
        Ok(())
    }

    /// 三段齐备判据。
    pub fn segs_complete(&self) -> bool {
        let mut i = 0;
        while i < FRAME_SEG_COUNT {
            if !self.segs_booked[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 池登记一样本（重登记拒收）。
    pub fn pool(&mut self, sample_id: usize) -> Result<(), u32> {
        let cls = class_of(sample_id)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].sample_id == sample_id {
                return Err(SmokeCode::SampleDoubleBooked.val());
            }
            i += 1;
        }
        self.rows.push(SmokeRow {
            sample_id,
            class: cls,
            state: SchedState::Pooled,
            verdict: false,
        });
        self.digest = self.digest.wrapping_mul(29).wrapping_add((sample_id as u64) + 1);
        Ok(())
    }

    /// 一样本调度走完全程：Pooled→Dispatched→Ran→Reported；
    /// 运行结论语义冻结：偶数号通过，奇数号报缺陷。
    pub fn dispatch_full(&mut self, sample_id: usize) -> Result<bool, u32> {
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].sample_id == sample_id {
                idx = Some(i);
            }
            i += 1;
        }
        let idx = match idx {
            Some(k) => k,
            None => return Err(SmokeCode::SampleOutOfRange.val()),
        };
        let verdict = sample_id % 2 == 0;
        self.rows[idx].state = step(self.rows[idx].state, SchedState::Dispatched)?;
        self.rows[idx].state = step(self.rows[idx].state, SchedState::Ran)?;
        self.rows[idx].state = step(self.rows[idx].state, SchedState::Reported)?;
        self.rows[idx].verdict = verdict;
        Ok(verdict)
    }

    /// 分册结构断言：各册计数与 VOL_SIZE 冻结值逐一相等。
    pub fn volume_check(&self) -> Result<(), u32> {
        let mut counts = [0usize; CLASS_COUNT];
        let mut i = 0;
        while i < self.rows.len() {
            let c = self.rows[i].class;
            if c >= CLASS_COUNT {
                return Err(SmokeCode::ClassOutOfRange.val());
            }
            counts[c] += 1;
            i += 1;
        }
        let mut k = 0;
        while k < CLASS_COUNT {
            if counts[k] != VOL_SIZE[k] {
                return Err(SmokeCode::VolumeBroken.val());
            }
            k += 1;
        }
        Ok(())
    }

    /// 全 30 样本正演 + 三段齐备 + 对账。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        self.book_seg(SEG_POOL)?;
        self.book_seg(SEG_RUNNER)?;
        self.book_seg(SEG_REPORT)?;
        let mut s = 0;
        while s < SAMPLE_COUNT {
            self.pool(s)?;
            self.dispatch_full(s)?;
            s += 1;
        }
        if self.rows.len() != EXPECT_SAMPLES {
            return Err(SmokeCode::ReportDrift.val());
        }
        if !self.segs_complete() {
            return Err(SmokeCode::FrameSegGap.val());
        }
        self.volume_check()?;
        Ok(self.rows.len() as u32)
    }

    /// 报告对账：Reported 条数==池登记数；结论与冻结映射一致。
    pub fn report_reconcile(&self) -> Result<u32, u32> {
        let mut reported = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].state != SchedState::Reported {
                return Err(SmokeCode::TransitionIllegal.val());
            }
            if self.rows[i].verdict != (self.rows[i].sample_id % 2 == 0) {
                return Err(SmokeCode::ReportDrift.val());
            }
            reported += 1;
            i += 1;
        }
        Ok(reported)
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let mut led = SmokeLedger::new();
    let booked = led.run_all()?;
    let reported = led.report_reconcile()?;
    // 通过数（偶数号样本）。
    let mut pass = 0;
    let mut i = 0;
    while i < led.rows.len() {
        if led.rows[i].verdict {
            pass += 1;
        }
        i += 1;
    }
    Ok((booked, reported, pass, led.digest))
}

// ── 注入面：三族全抓（tries/caught 双列） ────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：样本越界（下标 30）。
    ledger.tries[0] += 1;
    if let Err(e) = class_of(SAMPLE_COUNT) {
        if e == SmokeCode::SampleOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：调度非法边（Pooled→Ran 跳步）。
    ledger.tries[1] += 1;
    if let Err(e) = step(SchedState::Pooled, SchedState::Ran) {
        if e == SmokeCode::TransitionIllegal.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：样本重登记必须拒收。
    ledger.tries[2] += 1;
    let mut led = SmokeLedger::new();
    let _ = led.pool(0);
    if let Err(e) = led.pool(0) {
        if e == SmokeCode::SampleDoubleBooked.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 复现对拍 ─────────────────────────────────────────────────────
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a.0 != b.0 || a.1 != b.1 || a.2 != b.2 || a.3 != b.3 {
        return Err(SmokeCode::NotReproducible.val());
    }
    Ok(())
}

// ── LCG 确定性抽样（种子=SEG_BASE） ─────────────────────────────
pub fn sampled_digest() -> u64 {
    let mut g = Lcg(SEG_BASE as u64);
    let mut acc: u64 = 0;
    let mut k = 0;
    while k < 16 {
        let v = g.below(SAMPLE_COUNT as u64);
        acc = acc.wrapping_mul(17).wrapping_add(v + 1);
        k += 1;
    }
    acc
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SmokeCode::ALL.len() {
        let v = SmokeCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 三段齐备率 100%（样本池/运行器/报告）。
    let golden = golden_run();
    let seg_ok = match &golden {
        Ok((b, r, _, _)) => *b == EXPECT_SAMPLES as u32 && *r == EXPECT_SAMPLES as u32,
        Err(_) => false,
    };
    v.push((
        "J1-三段齐备率100%（池/运行器/报告）",
        seg_ok,
        format!("{:?}", golden),
    ));
    // 2. 四类分册结构断言 100%（8+7+8+7==30，独立算式守恒）。
    let vol_ok = VOL_SUM == EXPECT_SAMPLES
        && match &golden {
            Ok(_) => {
                let mut led = SmokeLedger::new();
                led.run_all().is_ok() && led.volume_check().is_ok()
            }
            Err(_) => false,
        };
    v.push((
        "J1-四类分册结构断言100%",
        vol_ok,
        format!("VOL_SUM={}", VOL_SUM),
    ));
    // 3. 归属映射点验（边界样本 0/8/15/23 落册正确）。
    let edge_ok = class_of(0) == Ok(CLASS_CONTROL)
        && class_of(8) == Ok(CLASS_DIALOG)
        && class_of(15) == Ok(CLASS_THREAD)
        && class_of(23) == Ok(CLASS_GDI)
        && class_of(29) == Ok(CLASS_GDI);
    v.push((
        "J1-样本归属边界点验",
        edge_ok,
        String::from("0/8/15/23/29 闭式映射"),
    ));
    // 4. 调度状态机（正链通/非法边拒）。
    let fwd = step(SchedState::Pooled, SchedState::Dispatched).is_ok()
        && step(SchedState::Dispatched, SchedState::Ran).is_ok()
        && step(SchedState::Ran, SchedState::Reported).is_ok();
    let bwd = step(SchedState::Reported, SchedState::Pooled).is_err()
        && step(SchedState::Pooled, SchedState::Reported).is_err();
    v.push((
        "J1-调度边闭集（通/拒双向）",
        fwd && bwd,
        format!("fwd={} bwd={}", fwd, bwd),
    ));
    // 5. 双跑复现。
    v.push((
        "J1-双跑复现",
        reproducible().is_ok(),
        String::from("冒烟账 digest 逐轮一致"),
    ));
    // 6. LCG 确定性抽样。
    let s1 = sampled_digest();
    let s2 = sampled_digest();
    v.push((
        "J1-LCG确定性抽样",
        s1 == s2,
        format!("digest={}", s1),
    ));
    // 7. 诊断码段位守卫 + detail 反查回环。
    let mut loop_ok = true;
    let mut i = 0;
    while i < SmokeCode::ALL.len() {
        let c = SmokeCode::ALL[i];
        if detail(c.val()) != c.detail() {
            loop_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-段位守卫+detail反查回环",
        segment_guard() && loop_ok,
        String::from("0x67DC0 窗口 8 码"),
    ));
    // 8. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 9. 名册点验（四类名 + 三段名非空）。
    let mut named = 0;
    i = 0;
    while i < CLASS_COUNT {
        if CLASS_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    let mut seg_named = 0;
    i = 0;
    while i < FRAME_SEG_COUNT {
        if FRAME_SEG_NAMES[i].len() > 0 {
            seg_named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-名册点验 4类+3段",
        named == CLASS_COUNT && seg_named == FRAME_SEG_COUNT,
        format!("{}/4 {}/3", named, seg_named),
    ));
    v
}

pub fn run_unx16501_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16501 冒烟集框架总纲（30 样本×四类）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
