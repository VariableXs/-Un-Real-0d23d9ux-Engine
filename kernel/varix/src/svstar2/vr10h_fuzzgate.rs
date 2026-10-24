//! VE-F3788 · R 域 fuzz 总闸确认（R10 收口组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3788`
//!
//! **判据（锚点原文）**：七不变量、双配额、闭环、72h、复用八用、判据。
//!
//! **本条是什么**：六源轮转证据四要素确认 + R 版七不变量快照
//! （崩溃/挂起/预算/资源/数据正确性/安全/创作四不变量并入）+ 案例闭环 +
//! 72h 档案 + 安全配额 30% + 无障碍配额 20% 双配额执行核验。
//!
//! 诊断码段：独占 0xB3xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xB3xx）
// ─────────────────────────────────────────────────────────────

/// F3788 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B3Code(pub u16);

impl B3Code {
    /// 要素缺——补跑。
    pub const FACTOR_MISS: B3Code = B3Code(0xB301);
    /// 非零——分级。
    pub const INVARIANT_NONZERO: B3Code = B3Code(0xB302);
    /// 未闭环——阻断。
    pub const LOOP_OPEN: B3Code = B3Code(0xB303);
    /// 配额不足——修正（红线）。
    pub const QUOTA_LOW: B3Code = B3Code(0xB304);
    /// 72h 档案缺。
    pub const ARCHIVE_MISS: B3Code = B3Code(0xB305);
    /// 六源缺源。
    pub const SOURCE_MISS: B3Code = B3Code(0xB306);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xB301 => "轮转证据四要素缺失，需补跑。",
            0xB302 => "七不变量非零，需分级处置。",
            0xB303 => "案例未闭环，阻断收口。",
            0xB304 => "双配额不足，红线修正。",
            0xB305 => "72h 档案缺失。",
            0xB306 => "六源轮转缺源。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 六源轮转四要素
// ─────────────────────────────────────────────────────────────

/// fuzz 六源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzSource {
    /// 输入变异。
    Input,
    /// 状态变异。
    State,
    /// 时序变异。
    Timing,
    /// 资源变异。
    Resource,
    /// 并发变异。
    Concurrency,
    /// 创作语义变异（R 段）。
    Creative,
}

impl FuzzSource {
    /// 全集。
    pub const ALL: [FuzzSource; 6] = [
        FuzzSource::Input,
        FuzzSource::State,
        FuzzSource::Timing,
        FuzzSource::Resource,
        FuzzSource::Concurrency,
        FuzzSource::Creative,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            FuzzSource::Input => 0,
            FuzzSource::State => 1,
            FuzzSource::Timing => 2,
            FuzzSource::Resource => 3,
            FuzzSource::Concurrency => 4,
            FuzzSource::Creative => 5,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            FuzzSource::Input => "输入变异",
            FuzzSource::State => "状态变异",
            FuzzSource::Timing => "时序变异",
            FuzzSource::Resource => "资源变异",
            FuzzSource::Concurrency => "并发变异",
            FuzzSource::Creative => "创作语义变异",
        }
    }
}

/// 四要素（种子/耗时/案例数/覆盖增量）。
#[derive(Clone, Copy, Debug)]
pub struct RunFactors {
    /// 种子。
    pub seed: u32,
    /// 耗时分钟。
    pub minutes: u16,
    /// 案例数。
    pub cases: u32,
    /// 覆盖增量万分比。
    pub cov_bp: u16,
    /// 齐备。
    pub complete: bool,
}

impl RunFactors {
    /// 新建缺。
    pub const fn empty() -> Self {
        RunFactors {
            seed: 0,
            minutes: 0,
            cases: 0,
            cov_bp: 0,
            complete: false,
        }
    }

    /// 从完整参数构造。
    pub const fn new(seed: u32, minutes: u16, cases: u32, cov_bp: u16) -> Self {
        RunFactors {
            seed,
            minutes,
            cases,
            cov_bp,
            complete: true,
        }
    }

    /// 要素齐。
    pub fn ok(self) -> bool {
        self.complete && self.seed != 0 && self.minutes > 0 && self.cases > 0
    }
}

/// 六源登记表。
#[derive(Clone, Debug)]
pub struct SourceBoard {
    factors: [RunFactors; 6],
}

impl SourceBoard {
    /// 新建。
    pub fn new() -> Self {
        SourceBoard {
            factors: [RunFactors::empty(); 6],
        }
    }

    /// 登记一源。
    pub fn enroll(&mut self, s: FuzzSource, f: RunFactors) -> bool {
        self.factors[s.ordinal()] = f;
        f.ok()
    }

    /// 缺源数。
    pub fn missing(&self) -> usize {
        self.factors.iter().filter(|f| !f.ok()).count()
    }

    /// 六源齐。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }
}

impl Default for SourceBoard {
    fn default() -> Self {
        SourceBoard::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 七不变量快照
// ─────────────────────────────────────────────────────────────

/// 不变量（七项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Invariant {
    /// 崩溃。
    Crash,
    /// 挂起。
    Hang,
    /// 预算。
    Budget,
    /// 资源。
    Resource,
    /// 数据正确性。
    DataCorrect,
    /// 安全。
    Security,
    /// 创作四不变量并入（标记为一项汇总）。
    Creative,
}

impl Invariant {
    /// 全集。
    pub const ALL: [Invariant; 7] = [
        Invariant::Crash,
        Invariant::Hang,
        Invariant::Budget,
        Invariant::Resource,
        Invariant::DataCorrect,
        Invariant::Security,
        Invariant::Creative,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Invariant::Crash => 0,
            Invariant::Hang => 1,
            Invariant::Budget => 2,
            Invariant::Resource => 3,
            Invariant::DataCorrect => 4,
            Invariant::Security => 5,
            Invariant::Creative => 6,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Invariant::Crash => "崩溃",
            Invariant::Hang => "挂起",
            Invariant::Budget => "预算",
            Invariant::Resource => "资源",
            Invariant::DataCorrect => "数据正确性",
            Invariant::Security => "安全",
            Invariant::Creative => "创作不变量",
        }
    }
}

/// 快照（违例计数须全零）。
#[derive(Clone, Debug)]
pub struct InvSnapshot {
    violations: [u32; 7],
}

impl InvSnapshot {
    /// 新建（全零）。
    pub const fn new() -> Self {
        InvSnapshot {
            violations: [0; 7],
        }
    }

    /// 记违例。
    pub fn bump(&mut self, inv: Invariant) {
        let i = inv.ordinal();
        self.violations[i] = self.violations[i].saturating_add(1);
    }

    /// 清零一项。
    pub fn clear(&mut self, inv: Invariant) {
        self.violations[inv.ordinal()] = 0;
    }

    /// 总违例。
    pub fn total(&self) -> u32 {
        self.violations.iter().sum()
    }

    /// 全零。
    pub fn all_zero(&self) -> bool {
        self.total() == 0
    }

    /// 非零项数。
    pub fn nonzero_items(&self) -> usize {
        self.violations.iter().filter(|v| **v > 0).count()
    }
}

// ─────────────────────────────────────────────────────────────
// 闭环 + 72h + 双配额
// ─────────────────────────────────────────────────────────────

/// 案例闭环。
#[derive(Clone, Debug)]
pub struct CaseLoop {
    /// 发现数。
    pub found: u32,
    /// 已闭环数。
    pub closed: u32,
    /// 72h 档案在册。
    pub archive_72h: bool,
}

impl CaseLoop {
    /// 新建。
    pub const fn new() -> Self {
        CaseLoop {
            found: 0,
            closed: 0,
            archive_72h: false,
        }
    }

    /// 发现一案。
    pub fn find(&mut self) {
        self.found += 1;
    }

    /// 闭环一案。
    pub fn close(&mut self) -> bool {
        if self.closed < self.found {
            self.closed += 1;
            true
        } else {
            false
        }
    }

    /// 未闭环数。
    pub fn open(&self) -> u32 {
        self.found.saturating_sub(self.closed)
    }

    /// 全闭环 + 档案在。
    pub fn gate_pass(&self) -> bool {
        self.open() == 0 && self.archive_72h
    }
}

/// 双配额（安全 30% + 无障碍 20%）。
#[derive(Clone, Copy, Debug)]
pub struct DualQuota {
    /// 总配额（案例数基数）。
    pub total: u32,
    /// 安全案例数。
    pub security: u32,
    /// 无障碍案例数。
    pub a11y: u32,
}

impl DualQuota {
    /// 新建。
    pub const fn new(total: u32) -> Self {
        DualQuota {
            total,
            security: 0,
            a11y: 0,
        }
    }

    /// 安全配额万分比（目标 ≥3000）。
    pub fn security_bp(&self) -> u32 {
        if self.total == 0 {
            0
        } else {
            (self.security * 10_000) / self.total
        }
    }

    /// 无障碍配额万分比（目标 ≥2000）。
    pub fn a11y_bp(&self) -> u32 {
        if self.total == 0 {
            0
        } else {
            (self.a11y * 10_000) / self.total
        }
    }

    /// 双配额达标。
    pub fn pass(&self) -> bool {
        self.security_bp() >= 3_000 && self.a11y_bp() >= 2_000
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3788 判据集。
pub fn run_vr10h_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3788");

    // 0xB301/0xB306 六源四要素
    {
        let mut b = SourceBoard::new();
        set.add("0xB306-初始缺六", b.missing() == 6, "");
        for s in FuzzSource::ALL.iter() {
            b.enroll(*s, RunFactors::new(1, 30, 100, 50));
        }
        set.add("0xB306-六源齐", b.complete(), "");
    }

    // 0xB301 要素缺失
    {
        let mut b = SourceBoard::new();
        b.enroll(FuzzSource::Input, RunFactors::empty());
        set.add("0xB301-空要素不过", b.missing() == 6, "");
        b.enroll(FuzzSource::Input, RunFactors::new(1, 0, 10, 1));
        set.add("0xB301-零分钟不过", b.missing() == 6, "");
    }

    // 0xB302 七不变量
    {
        let mut s = InvSnapshot::new();
        set.add("0xB302-初始全零", s.all_zero() && s.total() == 0, "");
        s.bump(Invariant::Crash);
        set.add("0xB302-违例检出", !s.all_zero() && s.nonzero_items() == 1, "");
        s.clear(Invariant::Crash);
        set.add("0xB302-清零后过", s.all_zero(), "");
        for inv in Invariant::ALL.iter() {
            s.bump(*inv);
        }
        set.add("0xB302-七项计数", s.nonzero_items() == 7, "");
    }

    // 0xB303 闭环
    {
        let mut c = CaseLoop::new();
        set.add("0xB303-空档案不过", !c.gate_pass(), "");
        c.archive_72h = true;
        set.add("0xB305-档案在空案过", c.gate_pass(), "");
        c.find();
        c.find();
        set.add("0xB303-未闭不过", !c.gate_pass() && c.open() == 2, "");
        c.close();
        c.close();
        set.add("0xB303-全闭过", c.gate_pass() && c.open() == 0, "");
        set.add("0xB303-空闭拒", !c.close(), "");
    }

    // 0xB304 双配额
    {
        let mut q = DualQuota::new(100);
        set.add("0xB304-零配不过", !q.pass(), "");
        q.security = 30;
        q.a11y = 20;
        set.add("0xB304-30/20正好过", q.security_bp() == 3000 && q.a11y_bp() == 2000, "");
        set.add("0xB304-双配额过", q.pass(), "");
        q.a11y = 19;
        set.add("0xB304-19%不过", !q.pass(), "");
    }

    // 源序自洽
    {
        let mut ok = true;
        for (i, s) in FuzzSource::ALL.iter().enumerate() {
            if s.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xB306-源序自洽", ok, "");
        let mut ok_i = true;
        for (i, inv) in Invariant::ALL.iter().enumerate() {
            if inv.ordinal() != i {
                ok_i = false;
            }
        }
        set.add("0xB302-不变量序自洽", ok_i, "");
    }

    set
}
