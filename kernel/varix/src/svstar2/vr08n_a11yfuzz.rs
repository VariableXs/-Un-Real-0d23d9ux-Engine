//! VE-F3754 · 创作无障碍 fuzz
//!
//! 锚点：创作无障碍 fuzz（F3155 模式创作版复刻：变异三级（工具变异（画布极端/
//! 图层风暴复用）/产出变异（产出无障碍破坏注入/通道移除注入）/时序变异
//! （reduce×低端×辅具三重叠加复用）；不变量（创作四不变量（主权/可逆/禁令/通道
//! 完备——违例分级复用（P0 级）；独立配额 20% 复述；最小化+闭环复述。
//!
//! 数据结构：三变异；四不变量；分级规则复用；配额复述。
//! 错误路径与降级矩阵：不变量违例→分级立案（复述红线实测）；配额不足→修正（复述）；
//! 挂起→看门狗（复述）；闭环缺→阻断（复述）。
//! 跨批对接点：F3155/F2878 基建单源；F2487 总闸（R08 段）；F3746/F3747 对端。
//!
//! 诊断码段：独占 0x91xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x91xx）
// ─────────────────────────────────────────────────────────────

/// F3754 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Z7Code(pub u16);

impl Z7Code {
    /// 主权不变量违例——P0。
    pub const INV_SOVEREIGN: Z7Code = Z7Code(0x9101);
    /// 可逆不变量违例——P0。
    pub const INV_REVERSIBLE: Z7Code = Z7Code(0x9102);
    /// 禁令不变量违例——P0。
    pub const INV_BAN: Z7Code = Z7Code(0x9103);
    /// 通道完备不变量违例——P0。
    pub const INV_CHANNEL: Z7Code = Z7Code(0x9104);
    /// 配额不足——修正。
    pub const QUOTA_LOW: Z7Code = Z7Code(0x9105);
    /// 看门狗触发——挂起。
    pub const WATCHDOG: Z7Code = Z7Code(0x9106);
    /// 闭环缺失——阻断。
    pub const LOOP_OPEN: Z7Code = Z7Code(0x9107);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9101 => "主权不变量违例，P0 分级立案。",
            0x9102 => "可逆不变量违例，P0 分级立案。",
            0x9103 => "禁令不变量违例，P0 分级立案。",
            0x9104 => "通道完备不变量违例，P0 分级立案。",
            0x9105 => "独立配额低于 20%，需修正。",
            0x9106 => "看门狗触发，用例挂起。",
            0x9107 => "闭环缺失，fuzz 流程阻断。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 四不变量（主权/可逆/禁令/通道完备）
// ─────────────────────────────────────────────────────────────

/// 不变量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Invariant {
    /// 主权。
    Sovereign,
    /// 可逆。
    Reversible,
    /// 禁令。
    Ban,
    /// 通道完备。
    ChannelComplete,
}

impl Invariant {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            Invariant::Sovereign => 0,
            Invariant::Reversible => 1,
            Invariant::Ban => 2,
            Invariant::ChannelComplete => 3,
        }
    }

    /// 全集。
    pub fn all() -> [Invariant; 4] {
        [
            Invariant::Sovereign,
            Invariant::Reversible,
            Invariant::Ban,
            Invariant::ChannelComplete,
        ]
    }

    /// 对应诊断码。
    pub fn code(self) -> Z7Code {
        match self {
            Invariant::Sovereign => Z7Code::INV_SOVEREIGN,
            Invariant::Reversible => Z7Code::INV_REVERSIBLE,
            Invariant::Ban => Z7Code::INV_BAN,
            Invariant::ChannelComplete => Z7Code::INV_CHANNEL,
        }
    }
}

/// 不变量状态。
#[derive(Clone, Debug)]
pub struct InvState {
    /// 每项是否成立。
    holds: [bool; 4],
}

impl InvState {
    /// 全成立。
    pub fn all_hold() -> InvState {
        InvState { holds: [true; 4] }
    }

    /// 全违例（演练）。
    pub fn all_break() -> InvState {
        InvState { holds: [false; 4] }
    }

    /// 打破一项。
    pub fn break_one(&mut self, inv: Invariant) {
        self.holds[inv.index() as usize] = false;
    }

    /// 修复一项。
    pub fn fix_one(&mut self, inv: Invariant) {
        self.holds[inv.index() as usize] = true;
    }

    /// 违例清单。
    pub fn violations(&self) -> Vec<Invariant> {
        let mut bad = Vec::new();
        for inv in Invariant::all() {
            if !self.holds[inv.index() as usize] {
                bad.push(inv);
            }
        }
        bad
    }

    /// 全成立。
    pub fn all_ok(&self) -> bool {
        self.holds.iter().all(|b| *b)
    }
}

impl Default for InvState {
    fn default() -> InvState {
        InvState::all_hold()
    }
}

// ─────────────────────────────────────────────────────────────
// 变异三级
// ─────────────────────────────────────────────────────────────

/// 变异类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MutKind {
    /// 工具变异（画布极端/图层风暴）。
    Tool,
    /// 产出变异（无障碍破坏/通道移除）。
    Output,
    /// 时序变异（reduce×低端×辅具三重叠加）。
    Timing,
}

impl MutKind {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            MutKind::Tool => 0,
            MutKind::Output => 1,
            MutKind::Timing => 2,
        }
    }

    /// 全集。
    pub fn all() -> [MutKind; 3] {
        [MutKind::Tool, MutKind::Output, MutKind::Timing]
    }
}

/// 变异发生器。
#[derive(Clone, Debug, Default)]
pub struct Mutator {
    /// 每类已生成用例数。
    counts: [u32; 3],
    /// 伪随机种子。
    seed: u64,
}

impl Mutator {
    /// 构造。
    pub fn new(seed: u64) -> Mutator {
        Mutator {
            counts: [0; 3],
            seed,
        }
    }

    /// 生成一例（返回变体参数）。
    pub fn generate(&mut self, kind: MutKind) -> u32 {
        // LCG 步进
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.counts[kind.index() as usize] += 1;
        (self.seed >> 33) as u32
    }

    /// 某类计数。
    pub fn count_of(&self, kind: MutKind) -> u32 {
        self.counts[kind.index() as usize]
    }

    /// 总计。
    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }

    /// 三类均衡：每类 ≥ 1。
    pub fn balanced(&self) -> bool {
        self.counts.iter().all(|c| *c > 0)
    }
}

// ─────────────────────────────────────────────────────────────
// 配额与看门狗
// ─────────────────────────────────────────────────────────────

/// 配额管理。
#[derive(Clone, Debug, Default)]
pub struct Quota {
    /// 无障碍 fuzz 配额（万分比，20% = 2000）。
    a11y_bp: u16,
    /// 总预算用例数。
    total: u32,
    /// 已跑无障碍用例。
    a11y_used: u32,
}

impl Quota {
    /// 构造（总 1000，配额 20%）。
    pub fn new(total: u32) -> Quota {
        Quota {
            a11y_bp: 2000,
            total,
            a11y_used: 0,
        }
    }

    /// 目标无障碍用例数。
    pub fn target(&self) -> u32 {
        ((self.total as u64 * self.a11y_bp as u64) / 10000) as u32
    }

    /// 消耗一例。
    pub fn consume(&mut self) -> bool {
        if self.a11y_used >= self.target() {
            return false;
        }
        self.a11y_used += 1;
        true
    }

    /// 配额满足。
    pub fn satisfied(&self) -> bool {
        self.a11y_used >= self.target()
    }

    /// 剩余。
    pub fn remaining(&self) -> u32 {
        self.target().saturating_sub(self.a11y_used)
    }
}

/// 看门狗。
#[derive(Clone, Debug, Default)]
pub struct Watchdog {
    /// 时钟步数。
    ticks: u32,
    /// 超时阈值。
    timeout: u32,
    /// 是否触发。
    tripped: bool,
}

impl Watchdog {
    /// 构造。
    pub fn new(timeout: u32) -> Watchdog {
        Watchdog {
            ticks: 0,
            timeout,
            tripped: false,
        }
    }

    /// 步进。
    pub fn tick(&mut self) -> bool {
        self.ticks += 1;
        if self.ticks > self.timeout {
            self.tripped = true;
            return false;
        }
        true
    }

    /// 复位。
    pub fn reset(&mut self) {
        self.ticks = 0;
        self.tripped = false;
    }

    /// 是否触发。
    pub fn tripped(&self) -> bool {
        self.tripped
    }
}

// ─────────────────────────────────────────────────────────────
// 闭环
// ─────────────────────────────────────────────────────────────

/// 闭环状态。
#[derive(Clone, Debug, Default)]
pub struct LoopClosure {
    /// 变异已生成。
    pub generated: bool,
    /// 断言已执行。
    pub asserted: bool,
    /// 发现已入库。
    pub filed: bool,
    /// 已最小化。
    pub minimized: bool,
}

impl LoopClosure {
    /// 构造。
    pub fn new() -> LoopClosure {
        LoopClosure {
            generated: false,
            asserted: false,
            filed: false,
            minimized: false,
        }
    }

    /// 闭环完整。
    pub fn closed(&self) -> bool {
        self.generated && self.asserted && self.filed && self.minimized
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 创作无障碍 fuzz 引擎。
#[derive(Clone, Debug, Default)]
pub struct A11yFuzzEngine {
    mutator: Mutator,
    quota: Quota,
    watchdog: Watchdog,
    closure: LoopClosure,
}

impl A11yFuzzEngine {
    /// 构造。
    pub fn new(seed: u64, total: u32) -> A11yFuzzEngine {
        A11yFuzzEngine {
            mutator: Mutator::new(seed),
            quota: Quota::new(total),
            watchdog: Watchdog::new(1000),
            closure: LoopClosure::new(),
        }
    }

    /// 取变异器（可变）。
    pub fn mutator_mut(&mut self) -> &mut Mutator {
        &mut self.mutator
    }

    /// 取配额（可变）。
    pub fn quota_mut(&mut self) -> &mut Quota {
        &mut self.quota
    }

    /// 取看门狗（可变）。
    pub fn watchdog_mut(&mut self) -> &mut Watchdog {
        &mut self.watchdog
    }

    /// 取闭环（可变）。
    pub fn closure_mut(&mut self) -> &mut LoopClosure {
        &mut self.closure
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<Z7Code> {
        let mut bad = Vec::new();
        if !self.quota.satisfied() {
            bad.push(Z7Code::QUOTA_LOW);
        }
        if self.watchdog.tripped() {
            bad.push(Z7Code::WATCHDOG);
        }
        if !self.closure.closed() {
            bad.push(Z7Code::LOOP_OPEN);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3754 判据集。
pub fn run_vr08n_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3754");

    // 0x9101-0x9104 不变量
    {
        let st = InvState::all_hold();
        out.add("0x9101-全成立过", st.all_ok(), "");
        let mut broken = InvState::all_break();
        out.add("0x9101-全违例检出", broken.violations().len() == 4, "");
        broken.fix_one(Invariant::Sovereign);
        out.add("0x9102-修复递减", broken.violations().len() == 3, "");
    }

    // 变异三类
    {
        let mut m = Mutator::new(42);
        m.generate(MutKind::Tool);
        m.generate(MutKind::Output);
        m.generate(MutKind::Timing);
        out.add("0x9105-三类均衡", m.balanced() && m.total() == 3, "");
    }

    // 0x9105 配额 20%
    {
        let mut q = Quota::new(100);
        out.add("0x9105-目标20例", q.target() == 20, "");
        for _ in 0..20 {
            q.consume();
        }
        out.add("0x9105-配额满足", q.satisfied(), "");
        out.add("0x9105-超配额拒", !q.consume(), "");
    }

    // 0x9106 看门狗
    {
        let mut w = Watchdog::new(3);
        out.add("0x9106-步进3过", w.tick() && w.tick() && w.tick(), "");
        out.add("0x9106-第4步触发", !w.tick() && w.tripped(), "");
    }

    // 0x9107 闭环
    {
        let mut c = LoopClosure::new();
        out.add("0x9107-空闭环不闭", !c.closed(), "");
        c.generated = true;
        c.asserted = true;
        c.filed = true;
        c.minimized = true;
        out.add("0x9107-四步闭合", c.closed(), "");
    }

    // 判据
    {
        let mut e = A11yFuzzEngine::new(1, 100);
        let bad = e.audit();
        out.add("0x9105-引擎配额检出", bad.iter().any(|c| c.raw() == 0x9105), "");
    }

    out
}
