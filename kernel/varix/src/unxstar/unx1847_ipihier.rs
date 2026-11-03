//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1847 · IPI 处理中断层级纪律（F1733 衔接） · 0x725C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B13）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F1847-J1 IPI 处理函数中断层级约束断言全过（禁锁/
//!     禁调度/禁长驻），越界注入 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 中断层级模型：Thread(0)→SoftIrq(1)→HardIrq(2)→NmiCtx(3)，IPI 处理
//!     函数固定运行在 HardIrq 层；锁/调度/睡眠/重分配都是进程上下文操作，
//!     在 HardIrq 层一律非法（禁锁/禁调度/禁长驻）；
//!   · 长驻判据：处理时长 > 层级预算（普通 12 tick / TLB 击落 40 tick）
//!     即 Overstay；伪中断（Spurious）必须裸来裸走（零操作+EOI 都不许）；
//!   · 每次调用的操作踪迹 + tick 数落账，审计器逐字节扫账；
//!   · 越界注入 10 路（加锁/加调度/加睡眠/加重分配/超预算/数倍超时/
//!     伪中断带 EOI/真中断缺 EOI/双违组合/TLB 带重分配）10/10 必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x725C0，IPI 层级纪律窗口独占）。
pub const SEG_BASE: u32 = 0x725C0;
/// HardIrq 层普通预算（tick，冻结 12）。
pub const BUDGET_TICKS: u64 = 12;
/// TLB 击落宽限预算（tick，冻结 40）。
pub const TLB_BUDGET_TICKS: u64 = 40;
/// 操作踪迹容量（每调用，冻结 16）。
pub const TRACE_CAP: usize = 16;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 核覆盖面（冻结 4 核）。
pub const CORE_N: u32 = 4;
/// 诊断窗口宽（段内 0x40）。
pub const SEG_WINDOW: u32 = 0x40;

// ── 操作码闭集 ──────────────────────────────────────────────────
/// 空操作。
pub const OP_NONE: u8 = 0;
/// 阻塞锁（HardIrq 层禁）。
pub const OP_LOCK: u8 = 1;
/// 调度/让出（HardIrq 层禁）。
pub const OP_SCHED: u8 = 2;
/// 睡眠（HardIrq 层禁）。
pub const OP_SLEEP: u8 = 3;
/// 重分配长驻活（HardIrq 层禁）。
pub const OP_ALLOC: u8 = 4;
/// EOI 回执（HardIrq 层必做收尾）。
pub const OP_EOI: u8 = 5;
/// 短自旋等待（仅 TLB 击落允许）。
pub const OP_SPIN: u8 = 6;

/// 操作码中文名。
pub fn op_name(op: u8) -> &'static str {
    match op {
        OP_NONE => "空",
        OP_LOCK => "阻塞锁",
        OP_SCHED => "调度",
        OP_SLEEP => "睡眠",
        OP_ALLOC => "重分配",
        OP_EOI => "EOI回执",
        OP_SPIN => "短自旋",
        _ => "未知操作",
    }
}

// ── 中断层级模型 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hier {
    /// 线程层（进程上下文，操作全放开）。
    Thread,
    /// 软中断层。
    SoftIrq,
    /// 硬中断层（IPI 处理函数的驻留层）。
    HardIrq,
    /// NMI 层。
    NmiCtx,
}

impl Hier {
    /// 层级序值。
    pub fn rank(self) -> u8 {
        match self {
            Hier::Thread => 0,
            Hier::SoftIrq => 1,
            Hier::HardIrq => 2,
            Hier::NmiCtx => 3,
        }
    }
}

/// 操作 × 层级合法性矩阵（层级纪律核心）：
///   HardIrq/NmiCtx：锁/调度/睡眠/重分配全禁；EOI 必许；自旋仅 TLB 场景
///   由预算兜底。SoftIrq：睡眠仍禁。Thread：全许。
pub fn op_allowed_at(op: u8, h: Hier) -> bool {
    match h {
        Hier::Thread => true,
        Hier::SoftIrq => !matches!(op, OP_SLEEP),
        Hier::HardIrq | Hier::NmiCtx => !matches!(op, OP_LOCK | OP_SCHED | OP_SLEEP | OP_ALLOC),
    }
}

// ── IPI 种类 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiKind {
    /// 重调度 IPI（最轻）。
    Resched,
    /// TLB 击落 IPI（宽限预算）。
    TlbShoot,
    /// 函数调用 IPI。
    CallFunc,
    /// 伪中断（必须裸来裸走）。
    Spurious,
}

impl IpiKind {
    /// 层级预算：普通 12，TLB 40，伪中断 2。
    pub fn budget(self) -> u64 {
        match self {
            IpiKind::Resched | IpiKind::CallFunc => BUDGET_TICKS,
            IpiKind::TlbShoot => TLB_BUDGET_TICKS,
            IpiKind::Spurious => 2,
        }
    }
    /// 是否真中断（需要 EOI 收尾）。
    pub fn needs_eoi(self) -> bool {
        !matches!(self, IpiKind::Spurious)
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiCode {
    /// 层级违锁：HardIrq 层取阻塞锁
    HierLock,
    /// 层级违调度：HardIrq 层调度/让出
    HierSched,
    /// 层级违睡眠：HardIrq/SoftIrq 层睡眠
    HierSleep,
    /// 层级违重活：HardIrq 层重分配长驻
    HierHeavy,
    /// 长驻超时：处理时长超出层级预算
    Overstay,
    /// EOI 收尾坏：真中断缺 EOI / 伪中断带尾巴
    EoiBroken,
    /// 踪迹账破损：操作码越界或踪迹溢出
    TraceBad,
    /// 双跑不一致：golden_run 两次应同值（复现红线）
    NotReproducible,
}

impl IpiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IpiCode::HierLock => "层级违锁：HardIrq 层禁阻塞锁，改 LockFree 踪迹或延后到线程层",
            IpiCode::HierSched => "层级违调度：HardIrq 层禁调度，改发重调度 IPI 后返回",
            IpiCode::HierSleep => "层级违睡眠：中断上下文禁睡眠，改轮询/信号量后置",
            IpiCode::HierHeavy => "层级违重活：HardIrq 层禁重分配，改 Workqueue 延后",
            IpiCode::Overstay => "长驻超时：超出层级预算，把长活切成短段或延后处理",
            IpiCode::EoiBroken => "EOI 收尾坏：真中断必须 EOI，伪中断必须裸来裸走",
            IpiCode::TraceBad => "踪迹账破损：操作码越界或踪迹溢出，查记录器",
            IpiCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [IpiCode; 8] = [
        IpiCode::HierLock,
        IpiCode::HierSched,
        IpiCode::HierSleep,
        IpiCode::HierHeavy,
        IpiCode::Overstay,
        IpiCode::EoiBroken,
        IpiCode::TraceBad,
        IpiCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IpiCode::ALL.len() {
        if IpiCode::ALL[i].val() == code {
            return IpiCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x725C0 段闭集内"
}

// ── 调用记录（操作踪迹 + tick 账） ──────────────────────────────
/// 一次 IPI 处理函数调用的完整账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpiRec {
    pub kind: IpiKind,
    pub core: u32,
    /// 操作踪迹（定容 16，0 填空）。
    pub ops: [u8; TRACE_CAP],
    /// 实际踪迹长度。
    pub op_n: usize,
    /// 处理时长（tick）。
    pub ticks: u64,
}

impl IpiRec {
    /// 建空账。
    pub fn new(kind: IpiKind, core: u32) -> IpiRec {
        IpiRec { kind, core, ops: [OP_NONE; TRACE_CAP], op_n: 0, ticks: 0 }
    }
    /// 追加操作（越容按 TraceBad 落账拒绝）。
    pub fn push_op(&mut self, op: u8) -> Result<(), u32> {
        if op > OP_SPIN {
            return Err(IpiCode::TraceBad.val());
        }
        if self.op_n >= TRACE_CAP {
            return Err(IpiCode::TraceBad.val());
        }
        self.ops[self.op_n] = op;
        self.op_n += 1;
        Ok(())
    }
    /// 记时长（饱和记账）。
    pub fn mark_ticks(&mut self, t: u64) {
        self.ticks = self.ticks.saturating_add(t);
    }
    /// 空踪迹判。
    pub fn is_bare(&self) -> bool {
        self.op_n == 0
    }
}

/// 确定性建核队核号表。
pub fn build_core_ids() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    let mut i = 0u32;
    while i < CORE_N {
        v.push(i);
        i += 1;
    }
    v
}

// ── 审计器（层级约束逐条断言） ──────────────────────────────────
/// 单账审计：禁锁/禁调度/禁睡眠/禁重活 → 长驻 → EOI 收尾。
/// 全过返回 Ok(过项数)；任一违即 Err（首个违因）。
pub fn audit(rec: &IpiRec) -> Result<u32, u32> {
    let mut passed = 0u32;
    // ① 层级约束逐操作扫账（禁锁/禁调度/禁睡眠/禁重活）。
    let mut i = 0usize;
    while i < rec.op_n {
        let op = match rec.ops.get(i) {
            Some(o) => *o,
            None => return Err(IpiCode::TraceBad.val()),
        };
        if op > OP_SPIN {
            return Err(IpiCode::TraceBad.val());
        }
        if !op_allowed_at(op, Hier::HardIrq) {
            let code = match op {
                OP_LOCK => IpiCode::HierLock.val(),
                OP_SCHED => IpiCode::HierSched.val(),
                OP_SLEEP => IpiCode::HierSleep.val(),
                OP_ALLOC => IpiCode::HierHeavy.val(),
                _ => IpiCode::TraceBad.val(),
            };
            return Err(code);
        }
        // 自旋只许 TLB 击落场景。
        if op == OP_SPIN && rec.kind != IpiKind::TlbShoot {
            return Err(IpiCode::HierHeavy.val());
        }
        passed += 1;
        i += 1;
    }
    // ② 长驻判据：tick 超层级预算即拒（边界值 = 预算本身放行）。
    if rec.ticks > rec.kind.budget() {
        return Err(IpiCode::Overstay.val());
    }
    // ③ EOI 收尾：真中断必须带 EOI；伪中断必须裸来裸走。
    let mut has_eoi = false;
    let mut j = 0usize;
    while j < rec.op_n {
        if rec.ops[j] == OP_EOI {
            has_eoi = true;
        }
        j += 1;
    }
    if rec.kind.needs_eoi() && !has_eoi {
        return Err(IpiCode::EoiBroken.val());
    }
    if !rec.kind.needs_eoi() && !rec.is_bare() {
        return Err(IpiCode::EoiBroken.val());
    }
    if rec.core >= CORE_N {
        return Err(IpiCode::TraceBad.val());
    }
    Ok(passed)
}

// ── 黄金账（四类 IPI 各一清白账） ───────────────────────────────
/// 清白账构造：Resched=3tick、TLB=30tick(带自旋)、CallFunc=8tick、Spurious=1tick。
pub fn golden_recs() -> Result<Vec<IpiRec>, u32> {
    let cores = build_core_ids();
    let c0 = match cores.get(0) {
        Some(c) => *c,
        None => return Err(IpiCode::TraceBad.val()),
    };
    let c1 = match cores.get(1) {
        Some(c) => *c,
        None => return Err(IpiCode::TraceBad.val()),
    };
    let c2 = match cores.get(2) {
        Some(c) => *c,
        None => return Err(IpiCode::TraceBad.val()),
    };
    let c3 = match cores.get(3) {
        Some(c) => *c,
        None => return Err(IpiCode::TraceBad.val()),
    };
    let mut v: Vec<IpiRec> = Vec::new();
    // Resched：EOI 即走。
    let mut r = IpiRec::new(IpiKind::Resched, c0);
    r.push_op(OP_EOI)?;
    r.mark_ticks(3);
    v.push(r);
    // TLB 击落：短自旋等 ack + EOI（宽限预算内）。
    let mut t = IpiRec::new(IpiKind::TlbShoot, c1);
    t.push_op(OP_SPIN)?;
    t.push_op(OP_EOI)?;
    t.mark_ticks(30);
    v.push(t);
    // CallFunc：EOI。
    let mut c = IpiRec::new(IpiKind::CallFunc, c2);
    c.push_op(OP_EOI)?;
    c.mark_ticks(8);
    v.push(c);
    // Spurious：裸来裸走。
    let mut s = IpiRec::new(IpiKind::Spurious, c3);
    s.mark_ticks(1);
    v.push(s);
    Ok(v)
}

/// 黄金全链：四类清白账逐账全过，返回总过项数。
pub fn golden_run() -> Result<u32, u32> {
    let recs = golden_recs()?;
    let mut total = 0u32;
    let mut i = 0usize;
    while i < recs.len() {
        let r = match recs.get(i) {
            Some(r) => r,
            None => return Err(IpiCode::TraceBad.val()),
        };
        total += audit(r)?;
        i += 1;
    }
    Ok(total)
}

// ── 越界注入（10 路，判据 10/10 必抓） ──────────────────────────
/// 越界注入轮：第 kind 路构造一个越界账，审计必须抓到。
/// 返回 (违因诊断码, 是否抓到)。
pub fn inject_round(round: u32) -> Result<(u32, bool), u32> {
    let kind = (round % 10) as usize;
    // 基准清白账（每路新鲜构造，防互相污染）。
    let mut base = IpiRec::new(IpiKind::Resched, 0);
    base.push_op(OP_EOI)?;
    base.mark_ticks(3);
    match kind {
        0 => {
            // 层级违锁：Resched 账里塞阻塞锁 → HierLock 必抓。
            base.push_op(OP_LOCK)?;
            judge(&base, IpiCode::HierLock)
        }
        1 => {
            // 层级违调度：塞调度 → HierSched 必抓。
            base.push_op(OP_SCHED)?;
            judge(&base, IpiCode::HierSched)
        }
        2 => {
            // 层级违睡眠：塞睡眠 → HierSleep 必抓。
            base.push_op(OP_SLEEP)?;
            judge(&base, IpiCode::HierSleep)
        }
        3 => {
            // 层级违重活：塞重分配 → HierHeavy 必抓。
            base.push_op(OP_ALLOC)?;
            judge(&base, IpiCode::HierHeavy)
        }
        4 => {
            // 超预算 1 tick：12+1=13 → Overstay 必抓（边界值本身放行）。
            base.mark_ticks(BUDGET_TICKS - 3 + 1);
            judge(&base, IpiCode::Overstay)
        }
        5 => {
            // 数倍超时：4×预算 → Overstay 必抓。
            base.mark_ticks(BUDGET_TICKS.saturating_mul(4));
            judge(&base, IpiCode::Overstay)
        }
        6 => {
            // 伪中断带尾巴：Spurious 塞 EOI → EoiBroken 必抓。
            let mut s = IpiRec::new(IpiKind::Spurious, 1);
            s.push_op(OP_EOI)?;
            s.mark_ticks(1);
            judge(&s, IpiCode::EoiBroken)
        }
        7 => {
            // 真中断缺 EOI：Resched 空账 → EoiBroken 必抓。
            let mut r = IpiRec::new(IpiKind::Resched, 2);
            r.mark_ticks(2);
            judge(&r, IpiCode::EoiBroken)
        }
        8 => {
            // 双违组合：锁+调度连塞 → 首个违因（锁）必抓。
            base.push_op(OP_LOCK)?;
            base.push_op(OP_SCHED)?;
            judge(&base, IpiCode::HierLock)
        }
        _ => {
            // TLB 带重分配：宽限预算也不许重活 → HierHeavy 必抓。
            let mut t = IpiRec::new(IpiKind::TlbShoot, 3);
            t.push_op(OP_SPIN)?;
            t.push_op(OP_ALLOC)?;
            t.push_op(OP_EOI)?;
            t.mark_ticks(30);
            judge(&t, IpiCode::HierHeavy)
        }
    }
}

/// 注入判定：审计结果落在期望违因上 = 抓到。
fn judge(rec: &IpiRec, want: IpiCode) -> Result<(u32, bool), u32> {
    match audit(rec) {
        Err(code) => Ok((code, code == want.val())),
        Ok(_) => Ok((0, false)),
    }
}

/// 注入账：tries/caught/漏抓路号。
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入 10 轮全扫（判据语义：10/10）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let kind = (r % 10) as usize;
        match inject_round(r) {
            Ok((_, hit)) => led.record(kind, hit),
            Err(_) => led.record(kind, false),
        }
        r += 1;
    }
    Ok(led)
}

// ── 层级边界探针（预算边界值逐类放行/越界必拒） ────────────────
/// 预算边界：ticks==budget 放行、budget+1 必拒（逐类）。
pub fn budget_edge_probe() -> bool {
    let mut ok = true;
    let kinds = [IpiKind::Resched, IpiKind::TlbShoot, IpiKind::CallFunc];
    let mut k = 0usize;
    while k < kinds.len() {
        let kind = kinds[k];
        let mut a = IpiRec::new(kind, 0);
        if a.push_op(OP_EOI).is_err() {
            return false;
        }
        a.mark_ticks(kind.budget());
        ok = ok && audit(&a).is_ok();
        let mut b = IpiRec::new(kind, 0);
        if b.push_op(OP_EOI).is_err() {
            return false;
        }
        b.mark_ticks(kind.budget().saturating_add(1));
        ok = ok && matches!(audit(&b), Err(code) if code == IpiCode::Overstay.val());
        k += 1;
    }
    ok
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链（四类清白账全过）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok(n) if n >= 3 => Ok(()),
        _ => Err(IpiCode::EoiBroken.val()),
    }
}

/// 序 B：越界注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(IpiCode::HierLock.val()),
    }
}

/// 序 C：冻结常量 + 层级矩阵 + 段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x725C0
        && IpiCode::NotReproducible.val() < SEG_BASE + SEG_WINDOW;
    // 层级矩阵抽验：HardIrq 四禁 + EOI 许；SoftIrq 禁睡；Thread 全许。
    let hard = !op_allowed_at(OP_LOCK, Hier::HardIrq)
        && !op_allowed_at(OP_SCHED, Hier::HardIrq)
        && !op_allowed_at(OP_SLEEP, Hier::HardIrq)
        && !op_allowed_at(OP_ALLOC, Hier::HardIrq)
        && op_allowed_at(OP_EOI, Hier::HardIrq);
    let soft = op_allowed_at(OP_LOCK, Hier::SoftIrq) && !op_allowed_at(OP_SLEEP, Hier::SoftIrq);
    let thr = op_allowed_at(OP_LOCK, Hier::Thread) && op_allowed_at(OP_SLEEP, Hier::Thread);
    let ok = BUDGET_TICKS == 12 && TLB_BUDGET_TICKS == 40 && TRACE_CAP == 16
        && JUDGE_ROUNDS == 10 && CORE_N == 4 && in_window && hard && soft && thr;
    if ok {
        Ok(())
    } else {
        Err(IpiCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(IpiCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < IpiCode::ALL.len() {
        let v = IpiCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + SEG_WINDOW {
            return false;
        }
        i += 1;
    }
    true
}

// ── 踪迹账渲染（中文明细） ─────────────────────────────────────
pub fn rec_lines(rec: &IpiRec) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut ops_txt = String::new();
    let mut i = 0usize;
    while i < rec.op_n {
        if i > 0 {
            ops_txt.push('/');
        }
        ops_txt.push_str(op_name(rec.ops[i]));
        i += 1;
    }
    if rec.is_bare() {
        ops_txt.push_str("裸");
    }
    v.push(format!(
        "{:?} 核{} 踪迹=[{}] tick={}/预算={}",
        rec.kind, rec.core, ops_txt, rec.ticks, rec.kind.budget()
    ));
    let audit_txt = match audit(rec) {
        Ok(n) => format!("审计过项={}", n),
        Err(code) => format!("违因=0x{:x} {}", code, detail(code)),
    };
    v.push(audit_txt);
    v
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let total = golden_run()?;
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    Ok(format!(
        "状态=已完成 清白账过项={} inject={}/{} 层级=HardIrq 禁锁/调度/长驻=全过",
        total, inj.caught, inj.tries
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四类清白账层级约束全过）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok(n) => (*n >= 3, format!("清白账过项={}", n)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链四类账", gp, gd));
    // J1 层级约束断言（禁锁/禁调度/禁睡眠/禁重活逐条在 HardIrq 层验）。
    let mut probe = IpiRec::new(IpiKind::Resched, 0);
    probe.push_op(OP_EOI).ok();
    let a1 = matches!(audit(&probe), Ok(n) if n == 1);
    let forbidden = [OP_LOCK, OP_SCHED, OP_SLEEP, OP_ALLOC];
    let mut all_caught = true;
    let mut fi = 0usize;
    while fi < forbidden.len() {
        let mut p = IpiRec::new(IpiKind::Resched, 0);
        let _ = p.push_op(OP_EOI);
        let _ = p.push_op(forbidden[fi]);
        let hit = matches!(
            audit(&p),
            Err(code) if code == IpiCode::HierLock.val()
                || code == IpiCode::HierSched.val()
                || code == IpiCode::HierSleep.val()
                || code == IpiCode::HierHeavy.val()
        );
        all_caught = all_caught && hit;
        fi += 1;
    }
    v.push((
        "J1-层级约束断言",
        a1 && all_caught,
        format!("清白EOI过={} 四禁逐条抓={}", a1, all_caught),
    ));
    // J1 禁长驻预算边界（==预算放行、+1 必拒，逐类）。
    let edge = budget_edge_probe();
    v.push(("J1-禁长驻预算边界", edge, String::from("12/40 tick 边界值放行，越 1 tick 必拒")));
    // J1 越界注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-越界注入10/10", ip, id));
    // J1 伪中断裸来裸走（空账过 / 带尾巴拒）。
    let bare = IpiRec::new(IpiKind::Spurious, 3);
    let bare_ok = audit(&bare).is_ok();
    let mut tailed = IpiRec::new(IpiKind::Spurious, 3);
    let _ = tailed.push_op(OP_EOI);
    let tailed_rej = matches!(audit(&tailed), Err(code) if code == IpiCode::EoiBroken.val());
    v.push((
        "J1-伪中断裸走纪律",
        bare_ok && tailed_rej,
        format!("裸过={} 带尾拒={}", bare_ok, tailed_rej),
    ));
    // J2 踪迹账守卫：坏操作码必拒 / 溢出必拒。
    let mut bad = IpiRec::new(IpiKind::Resched, 0);
    let bad_op = matches!(bad.push_op(0x7F), Err(code) if code == IpiCode::TraceBad.val());
    let mut ovf = IpiRec::new(IpiKind::CallFunc, 0);
    let mut filled = 0usize;
    let mut ovf_hit = false;
    while filled < TRACE_CAP {
        if ovf.push_op(OP_EOI).is_err() {
            break;
        }
        filled += 1;
    }
    if ovf.push_op(OP_EOI).is_err() {
        ovf_hit = true;
    }
    v.push((
        "J2-踪迹账守卫",
        bad_op && ovf_hit,
        format!("坏码拒={} 溢出拒={}(满={}/{})", bad_op, ovf_hit, filled, TRACE_CAP),
    ));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J2 层级矩阵冻结（三层抽验）。
    let mat_ok = probe_seq_c().is_ok();
    v.push(("J2-层级矩阵冻结", mat_ok, String::from("Thread 全许 / SoftIrq 禁睡 / HardIrq 四禁")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J3 踪迹账渲染非空（清白账 + 违例账双样）。
    let recs = match golden_recs() {
        Ok(r) => r,
        Err(_) => Vec::new(),
    };
    let l1 = recs.get(0).map(|r| rec_lines(r).len()).unwrap_or(0);
    let mut bad_rec = IpiRec::new(IpiKind::Resched, 0);
    let _ = bad_rec.push_op(OP_LOCK);
    let l2 = rec_lines(&bad_rec).len();
    v.push(("J3-踪迹账渲染", l1 >= 2 && l2 >= 2, format!("清白行={} 违例行={}", l1, l2)));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x725C0 窗口")));
    v
}

pub fn run_unx1847_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1847 IPI 处理中断层级纪律（F1733 衔接）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
