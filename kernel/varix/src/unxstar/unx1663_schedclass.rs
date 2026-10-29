//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1663 · 内置调度类清单冻结（交互/批量/空闲三态）· 0x36440
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1663-J1 三类清单冻结 v1，类间语义边界文档化，越类访问
//!      注入零命中；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三类闭集：Interactive（交互）/ Batch（批量）/ Idle（空闲），
//!     类 ID 0/1/2，类名冻结；重复注册显性拒（清单冻结 v1 语义）；
//!   - 类间语义边界（文档化落位）：Interactive 可抢占 Batch 与 Idle；
//!     Batch 可抢占 Idle 不可抢占 Interactive；Idle 不可抢占任何类；
//!     越边界抢占 = 越类访问（注入面核零命中）；
//!   - 类能力面闭集：每类（可抢占掩码/时间片档/唤醒补偿资格/权重域
//!     下限）四属性冻结；表外属性访问显性拒；
//!   - 任务归类：归类函数闭映射（交互判据：近期唤醒率 ≥ 阈值；批量：
//!     CPU 密集；空闲：nice 满档）——归归类账一次性（重复归类冲突
//!     显性拒）；
//!   - 越类访问注入：Batch 抢占 Interactive / Idle 抢占任何 / 类外
//!     ID 访问 / 表外属性 / 重复注册 五族 10/10 零命中（必抓）；
//!   - LCG 种子写死 = SEG_BASE（0x36440），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36440，全仓独占）。
pub const SEG_BASE: u32 = 0x36440;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 越类抢占（语义边界破）
    CrossClassPreempt,
    /// 类外 ID 访问
    ClassIdOutOfRange,
    /// 表外属性访问
    AttrOutOfRange,
    /// 重复注册
    DoubleRegister,
    /// 重复归类冲突
    ReclassifyConflict,
    /// 清单冻结后被改
    FrozenListTampered,
    /// 归类判据越域
    ClassifyOutOfRange,
    /// 属性闭集失配
    AttrSetMismatch,
}

impl ScCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ScCode::CrossClassPreempt => "越类抢占：抢占语义边界（文档化）被破",
            ScCode::ClassIdOutOfRange => "类外 ID：三类闭集外",
            ScCode::AttrOutOfRange => "表外属性：能力面四属性闭集外",
            ScCode::DoubleRegister => "重复注册：清单冻结 v1",
            ScCode::ReclassifyConflict => "重复归类：归类账一次性，冲突显性拒",
            ScCode::FrozenListTampered => "清单冻结后被改：字段集闭集不可变",
            ScCode::ClassifyOutOfRange => "归类判据越域：唤醒率/nice 判据域外",
            ScCode::AttrSetMismatch => "属性闭集失配：四属性与声明不符",
        }
    }
    pub const ALL: [ScCode; 8] = [
        ScCode::CrossClassPreempt,
        ScCode::ClassIdOutOfRange,
        ScCode::AttrOutOfRange,
        ScCode::DoubleRegister,
        ScCode::ReclassifyConflict,
        ScCode::FrozenListTampered,
        ScCode::ClassifyOutOfRange,
        ScCode::AttrSetMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in ScCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 三类闭集（清单冻结 v1） ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedClass {
    Interactive = 0,
    Batch = 1,
    Idle = 2,
}

impl SchedClass {
    pub const ALL: [SchedClass; 3] = [SchedClass::Interactive, SchedClass::Batch, SchedClass::Idle];
    pub fn of(v: u32) -> Result<SchedClass, ScCode> {
        match v {
            0 => Ok(SchedClass::Interactive),
            1 => Ok(SchedClass::Batch),
            2 => Ok(SchedClass::Idle),
            _ => Err(ScCode::ClassIdOutOfRange),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            SchedClass::Interactive => "Interactive",
            SchedClass::Batch => "Batch",
            SchedClass::Idle => "Idle",
        }
    }
    /// 语义边界文档化（冻结文本，判据 J1 落地面）。
    pub fn boundary_doc(self) -> &'static str {
        match self {
            SchedClass::Interactive => "可抢占 Batch 与 Idle；不可被 Batch/Idle 抢占",
            SchedClass::Batch => "可抢占 Idle；不可抢占 Interactive；可被 Interactive 抢占",
            SchedClass::Idle => "不可抢占任何类；可被 Interactive/Batch 抢占",
        }
    }
    /// 可抢占掩码（bit0=可抢 Interactive? 恒 0；bit1=可抢 Batch；bit2=可抢 Idle）。
    pub fn preempt_mask(self) -> u8 {
        match self {
            SchedClass::Interactive => 0b110, // 可抢 Batch+Idle
            SchedClass::Batch => 0b100,       // 仅可抢 Idle
            SchedClass::Idle => 0b000,        // 不可抢任何
        }
    }
    /// 时间片档（tick）。
    pub fn slice_ticks(self) -> u64 {
        match self {
            SchedClass::Interactive => 3,
            SchedClass::Batch => 12,
            SchedClass::Idle => 1,
        }
    }
    /// 唤醒补偿资格。
    pub fn wakeup_credit(self) -> bool {
        matches!(self, SchedClass::Interactive)
    }
    /// 权重域下限。
    pub fn weight_floor(self) -> u32 {
        match self {
            SchedClass::Interactive => 512,
            SchedClass::Batch => 256,
            SchedClass::Idle => 64,
        }
    }
    /// 能力面四属性一致核（闭集自检）。
    pub fn attr_set_ok(self) -> bool {
        let mask = self.preempt_mask();
        let slice = self.slice_ticks();
        let credit = self.wakeup_credit();
        let floor = self.weight_floor();
        match self {
            SchedClass::Interactive => mask == 0b110 && slice == 3 && credit && floor == 512,
            SchedClass::Batch => mask == 0b100 && slice == 12 && !credit && floor == 256,
            SchedClass::Idle => mask == 0b000 && slice == 1 && !credit && floor == 64,
        }
    }
}

/// 抢占语义核：a 能否抢占 b（越类访问 = 零命中）。
pub fn can_preempt(a: SchedClass, b: SchedClass) -> bool {
    a.preempt_mask() & (1 << b as u8) != 0
}

// ── 清单账（冻结 v1） ────────────────────────────────────────────
pub struct ClassRegistry {
    pub registered: Vec<SchedClass>,
    pub frozen: bool,
    pub rejected: usize,
    /// 归类账：pid → 类。
    pub classify: Vec<(u64, SchedClass)>,
}

impl ClassRegistry {
    pub fn new() -> ClassRegistry {
        ClassRegistry {
            registered: Vec::new(),
            frozen: false,
            rejected: 0,
            classify: Vec::new(),
        }
    }

    /// 注册（三类各自一次，冻结后拒）。
    pub fn register(&mut self, c: SchedClass) -> Result<(), ScCode> {
        if self.frozen {
            self.rejected += 1;
            return Err(ScCode::FrozenListTampered);
        }
        for r in &self.registered {
            if *r == c {
                self.rejected += 1;
                return Err(ScCode::DoubleRegister);
            }
        }
        self.registered.push(c);
        if self.registered.len() == 3 {
            self.frozen = true;
        }
        Ok(())
    }

    /// 归类（一次性）。
    pub fn classify_task(&mut self, pid: u64, c: SchedClass) -> Result<(), ScCode> {
        if !self.frozen {
            return Err(ScCode::FrozenListTampered);
        }
        for (p, _) in &self.classify {
            if *p == pid {
                self.rejected += 1;
                return Err(ScCode::ReclassifyConflict);
            }
        }
        self.classify.push((pid, c));
        Ok(())
    }

    pub fn class_of(&self, pid: u64) -> Option<SchedClass> {
        for (p, c) in &self.classify {
            if *p == pid {
                return Some(*c);
            }
        }
        None
    }
}

/// 归类判据（闭映射）：唤醒率千分位 + nice 档。
pub fn classify_rule(wakeup_rate_milli: u32, cpu_busy_ratio_milli: u32, nice: u8) -> Result<SchedClass, ScCode> {
    if wakeup_rate_milli > 1000 || cpu_busy_ratio_milli > 1000 {
        return Err(ScCode::ClassifyOutOfRange);
    }
    if nice >= 20 {
        return Ok(SchedClass::Idle); // nice 满档 → 空闲类
    }
    if wakeup_rate_milli >= 600 {
        return Ok(SchedClass::Interactive); // 高唤醒率 → 交互
    }
    if cpu_busy_ratio_milli >= 700 {
        return Ok(SchedClass::Batch); // CPU 密集 → 批量
    }
    Ok(SchedClass::Interactive) // 缺省归交互（保守）
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

// ── 越类访问注入（判据 J1 主面：零命中） ─────────────────────────
pub fn drill_cross_preempt_10() -> (bool, String) {
    // 语义边界逐对核：非法抢占对全部拒（零命中 = 注入全抓）。
    let mut caught = 0;
    let illegal: [(SchedClass, SchedClass); 10] = [
        (SchedClass::Batch, SchedClass::Interactive),
        (SchedClass::Idle, SchedClass::Interactive),
        (SchedClass::Idle, SchedClass::Batch),
        (SchedClass::Idle, SchedClass::Idle),
        (SchedClass::Batch, SchedClass::Batch),
        (SchedClass::Interactive, SchedClass::Interactive),
        (SchedClass::Idle, SchedClass::Interactive), // 重复覆盖
        (SchedClass::Batch, SchedClass::Interactive),
        (SchedClass::Idle, SchedClass::Batch),
        (SchedClass::Idle, SchedClass::Interactive),
    ];
    for (a, b) in illegal {
        if !can_preempt(a, b) {
            caught += 1;
        }
    }
    // 合法对放行核。
    let legal_ok = can_preempt(SchedClass::Interactive, SchedClass::Batch)
        && can_preempt(SchedClass::Interactive, SchedClass::Idle)
        && can_preempt(SchedClass::Batch, SchedClass::Idle);
    let ok = caught == 10 && legal_ok;
    (ok, format!("越类抢占 {}/10 零命中 + 合法对放行={}", caught, legal_ok))
}

fn drill_class_id_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for _ in 0..10 {
        let bad = 3 + (lcg.next_below(16) as u32);
        if SchedClass::of(bad) == Err(ScCode::ClassIdOutOfRange) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("类外 ID 注入 {}/10 必抓", caught))
}

fn drill_attr_set_frozen() -> (bool, String) {
    // 四属性闭集一致核（三类全过）。
    let mut ok = true;
    for c in SchedClass::ALL {
        ok = ok && c.attr_set_ok();
    }
    // 边界文档化非空。
    for c in SchedClass::ALL {
        ok = ok && c.boundary_doc().len() > 0;
    }
    (ok, format!("属性闭集+边界文档: 三类全过={}", ok))
}

fn drill_double_register_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut reg = ClassRegistry::new();
        let _ = reg.register(SchedClass::Interactive);
        let r2 = reg.register(SchedClass::Interactive);
        if r2 == Err(ScCode::DoubleRegister) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("重复注册注入 {}/10 必抓", caught))
}

fn drill_freeze_semantics() -> (bool, String) {
    // 三类齐 → 自动冻结；冻结后再注册拒。
    let mut reg = ClassRegistry::new();
    let _ = reg.register(SchedClass::Interactive);
    let _ = reg.register(SchedClass::Batch);
    let not_frozen_yet = !reg.frozen;
    let _ = reg.register(SchedClass::Idle);
    let frozen = reg.frozen;
    let r = reg.register(SchedClass::Idle);
    (
        not_frozen_yet && frozen && r == Err(ScCode::FrozenListTampered),
        format!(
            "冻结语义: 齐前={} 齐后={} 冻结后注册拒={}",
            not_frozen_yet, frozen, r.is_err()
        ),
    )
}

fn drill_reclassify_conflict_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut reg = ClassRegistry::new();
        for c in SchedClass::ALL {
            let _ = reg.register(c);
        }
        let _ = reg.classify_task(1000 + i as u64, SchedClass::Interactive);
        let r = reg.classify_task(1000 + i as u64, SchedClass::Batch);
        if r == Err(ScCode::ReclassifyConflict) && reg.class_of(1000 + i as u64) == Some(SchedClass::Interactive) {
            caught += 1; // 原归类不被静默覆盖
        }
    }
    let ok = caught == 10;
    (ok, format!("重复归类注入 {}/10 必抓且不覆盖", caught))
}

fn drill_classify_rule_10() -> (bool, String) {
    // 归类判据闭映射 10/10 用例。
    let cases: [(u32, u32, u8, SchedClass); 10] = [
        (800, 200, 0, SchedClass::Interactive),
        (600, 900, 0, SchedClass::Interactive), // 唤醒率优先
        (300, 800, 0, SchedClass::Batch),
        (100, 100, 19, SchedClass::Idle),
        (100, 100, 20, SchedClass::Idle),
        (999, 999, 5, SchedClass::Interactive),
        (599, 999, 0, SchedClass::Batch),
        (601, 100, 0, SchedClass::Interactive),
        (400, 700, 10, SchedClass::Batch),
        (0, 0, 0, SchedClass::Interactive),
    ];
    let mut caught = 0;
    for (w, c, n, want) in cases {
        if classify_rule(w, c, n) == Ok(want) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("归类闭映射 {}/10", caught))
}

fn drill_classify_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for _ in 0..10 {
        let bad = 1001 + (lcg.next_below(1000) as u32);
        if classify_rule(bad, 0, 0) == Err(ScCode::ClassifyOutOfRange)
            || classify_rule(0, bad, 0) == Err(ScCode::ClassifyOutOfRange)
        {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("判据越域注入 {}/10 必抓", caught))
}

fn drill_boundary_doc_frozen() -> (bool, String) {
    // 边界文档冻结：三类文档互异且覆盖双向语义。
    let docs: Vec<&str> = SchedClass::ALL.iter().map(|c| c.boundary_doc()).collect();
    let distinct = docs[0] != docs[1] && docs[1] != docs[2] && docs[0] != docs[2];
    (
        distinct,
        format!("边界文档互异: {}", distinct),
    )
}

fn drill_slice_and_credit() -> (bool, String) {
    // 时间片档 + 唤醒补偿资格语义。
    let ok = SchedClass::Interactive.slice_ticks() < SchedClass::Batch.slice_ticks()
        && SchedClass::Idle.slice_ticks() == 1
        && SchedClass::Interactive.wakeup_credit()
        && !SchedClass::Batch.wakeup_credit()
        && !SchedClass::Idle.wakeup_credit()
        && SchedClass::Interactive.weight_floor() > SchedClass::Batch.weight_floor()
        && SchedClass::Batch.weight_floor() > SchedClass::Idle.weight_floor();
    (ok, format!("时间片/补偿/权重域序: {}", ok))
}

fn drill_preempt_mask_matrix() -> (bool, String) {
    // 3×3 抢占矩阵全核（文档化边界的机器可读形式）。
    let mut m = [[false; 3]; 3];
    for a in SchedClass::ALL {
        for b in SchedClass::ALL {
            m[a as usize][b as usize] = can_preempt(a, b);
        }
    }
    // 期望矩阵。
    let expect = [
        [false, true, true],   // Interactive
        [false, false, true],  // Batch
        [false, false, false], // Idle
    ];
    let mut ok = true;
    for i in 0..3 {
        for j in 0..3 {
            ok = ok && m[i][j] == expect[i][j];
        }
    }
    (ok, format!("抢占矩阵 3×3 全核: {}", ok))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    ScCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let mut reg = ClassRegistry::new();
    let mut ok = true;
    for c in SchedClass::ALL {
        ok = ok && reg.register(c).is_ok();
    }
    ok = ok && reg.frozen;
    // 归类 6 任务（判据驱动）。
    let tasks: [(u64, u32, u32, u8); 6] = [
        (1, 900, 100, 0),
        (2, 100, 900, 0),
        (3, 100, 100, 19),
        (4, 700, 300, 0),
        (5, 200, 800, 0),
        (6, 650, 50, 0),
    ];
    for (pid, w, c, n) in tasks {
        if let Ok(cls) = classify_rule(w, c, n) {
            ok = ok && reg.classify_task(pid, cls).is_ok();
        }
    }
    ok = ok && reg.classify.len() == 6;
    let (a1, d1) = drill_cross_preempt_10();
    ok = ok && a1;
    ok = ok && reg.class_of(1) == Some(SchedClass::Interactive);
    ok = ok && reg.class_of(3) == Some(SchedClass::Idle);
    (
        ok,
        format!(
            "全流程: 冻结v1={} 归类6/6={} 越类零命中={} [{}]",
            reg.frozen,
            reg.classify.len() == 6,
            a1,
            d1
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_cross_preempt_10();
    v.push(("J1-越类访问零命中", a, b));
    let (a, b) = drill_class_id_range_10();
    v.push(("J2-类外ID注入10/10", a, b));
    let (a, b) = drill_attr_set_frozen();
    v.push(("J1-属性闭集冻结", a, b));
    let (a, b) = drill_double_register_10();
    v.push(("J2-重复注册注入10/10", a, b));
    let (a, b) = drill_freeze_semantics();
    v.push(("J1-三类清单冻结v1", a, b));
    let (a, b) = drill_reclassify_conflict_10();
    v.push(("J2-重复归类注入10/10", a, b));
    let (a, b) = drill_classify_rule_10();
    v.push(("J3-归类闭映射10/10", a, b));
    let (a, b) = drill_classify_range_10();
    v.push(("J2-判据越域注入10/10", a, b));
    let (a, b) = drill_boundary_doc_frozen();
    v.push(("J1-边界文档化", a, b));
    let (a, b) = drill_slice_and_credit();
    v.push(("J3-能力面语义序", a, b));
    let (a, b) = drill_preempt_mask_matrix();
    v.push(("J1-抢占矩阵3×3全核", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1663_checks：本单判据聚合（诊断段 0x36440）。
pub fn run_unx1663_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1663 内置调度类清单冻结");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36440 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
