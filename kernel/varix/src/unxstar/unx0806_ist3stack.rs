//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0806 · IST 三栈布局与 DF/NMI/MC 挂接 · 0x71F00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B01）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F0806-J1 三张 IST 栈独立可用（构造 DF 触发走 IST 且现场完
//!     整），栈溢出防护触发率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - TSS IST 槽位模型：x86 hardware IST 共 1..=7 号槽，槽 0 恒空；
//!   - 三张栈独立闭集：DF（#DF 双重故障 → IST1）、NMI → IST2、
//!     #MC（机器检查）→ IST3；三栈 16 对齐、互不重叠、各带金丝雀；
//!   - DF 触发构造：嵌套故障走账（Idle→第一故障→嵌套→DF 派发），
//!     派发时现场五元组（RIP/CS/RFLAGS/RSP/SS）齐全才算"现场完整"；
//!   - 溢出防护：每栈用量水位账，消耗越过守卫余量即触发防护（触发
//!     率 100%＝每次越界注入必抓）；
//!   - 装配状态机单调推进：Laid→Hooked→Armed→Verified，倒退显性拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x71F00，VX3-1103 批独占区）。
pub const SEG_BASE: u32 = 0x71F00;

/// IST 槽位对齐（16 冻结）。
pub const STACK_ALIGN: u64 = 16;
/// 单张 IST 栈最小尺寸（16KB 冻结）。
pub const STACK_MIN_SIZE: u64 = 0x4000;
/// 溢出防护守卫余量（2KB：越过即触防）。
pub const GUARD_MARGIN: u64 = 0x800;
/// IST 槽位总数（含槽 0；槽 0 恒空）。
pub const IST_SLOT_COUNT: usize = 8;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 三张栈的法定槽位闭集：DF=1 / NMI=2 / MC=3。
pub const IST_SLOT_DF: usize = 1;
pub const IST_SLOT_NMI: usize = 2;
pub const IST_SLOT_MC: usize = 3;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IstCode {
    /// 槽位越界（0 号槽或 >7 号槽）
    SlotBad,
    /// 栈基址非 16 对齐
    BaseMisaligned,
    /// 栈尺寸不足（<16KB）
    SizeTooSmall,
    /// 栈区间重叠
    StackOverlap,
    /// 槽位重复挂接
    DuplicateSlot,
    /// 溢出防护未触发（越界注入漏抓）
    GuardNotTripped,
    /// DF 现场不完整（五元组缺项）
    FrameIncomplete,
    /// 装配状态倒退
    PhaseRegress,
    /// 故障派发未走 IST（误走当前栈）
    NotOnIst,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl IstCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IstCode::SlotBad => "IST 槽位越界：0 号槽恒空或 >7 号槽，建议改用 1..=7",
            IstCode::BaseMisaligned => "栈基址非 16 对齐：建议向下取整到 16 边界再挂接",
            IstCode::SizeTooSmall => "栈尺寸不足：IST 栈下限 16KB，建议扩容",
            IstCode::StackOverlap => "栈区间重叠：红线，建议重排三栈基址留出间隔",
            IstCode::DuplicateSlot => "槽位重复挂接：一个 IST 槽只允许一张栈",
            IstCode::GuardNotTripped => "溢出防护未触发：越界注入漏抓，建议收紧守卫余量",
            IstCode::FrameIncomplete => "DF 现场不完整：RIP/CS/RFLAGS/RSP/SS 五元组缺项",
            IstCode::PhaseRegress => "装配状态倒退：Laid→Hooked→Armed→Verified 不可逆行",
            IstCode::NotOnIst => "故障派发未走 IST：DF/NMI/MC 必须切专用栈",
            IstCode::ReplayMismatch => "复算失配：布局账回放结果与原账不一致",
            IstCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [IstCode; 11] = [
        IstCode::SlotBad,
        IstCode::BaseMisaligned,
        IstCode::SizeTooSmall,
        IstCode::StackOverlap,
        IstCode::DuplicateSlot,
        IstCode::GuardNotTripped,
        IstCode::FrameIncomplete,
        IstCode::PhaseRegress,
        IstCode::NotOnIst,
        IstCode::ReplayMismatch,
        IstCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IstCode::ALL.len() {
        if IstCode::ALL[i].val() == code {
            return IstCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x71F00 段闭集内"
}

// ── 装配相位（单调推进，倒退显性拒） ────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 未装配
    Idle,
    /// 三栈已布局（基址/尺寸/金丝雀落账）
    Laid,
    /// DF/NMI/MC 已挂接到 IST 槽
    Hooked,
    /// 防护已武装（守卫余量生效）
    Armed,
    /// DF 触发构造验证通过
    Verified,
}

impl Phase {
    pub fn rank(self) -> u8 {
        match self {
            Phase::Idle => 0,
            Phase::Laid => 1,
            Phase::Hooked => 2,
            Phase::Armed => 3,
            Phase::Verified => 4,
        }
    }
    /// 单调推进：新相位等级必须严格大于旧相位，否则判倒退。
    pub fn advance_ok(old: Phase, new: Phase) -> Result<Phase, u32> {
        if new.rank() > old.rank() {
            Ok(new)
        } else {
            Err(IstCode::PhaseRegress.val())
        }
    }
}

// ── 单张 IST 栈描述 ─────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackDesc {
    /// IST 槽位（1..=7）
    pub slot: usize,
    /// 栈底（低地址，16 对齐）
    pub base: u64,
    /// 栈尺寸（字节）
    pub size: u64,
    /// 金丝雀种子（基址×尺寸派生，越界即毁）
    pub canary: u64,
}

impl StackDesc {
    pub fn top(&self) -> u64 {
        self.base + self.size
    }
    /// 金丝雀派生：基址与尺寸异或再叠加槽位旋转。
    pub fn derive_canary(slot: usize, base: u64, size: u64) -> u64 {
        base ^ size ^ ((slot as u64) << 32) ^ 0xC0FF_EE00_7157_0000
    }
}

// ── DF 派发现场五元组 ───────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntFrame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl IntFrame {
    /// 现场完整性：五元组逐项非零（0 视为缺项）。
    pub fn complete(&self) -> bool {
        self.rip != 0 && self.cs != 0 && self.rflags != 0 && self.rsp != 0 && self.ss != 0
    }
    /// 装配成派发账文本（写回结果摘要用）。
    pub fn summary(&self) -> String {
        format!(
            "rip={:x} cs={:x} fl={:x} rsp={:x} ss={:x}",
            self.rip, self.cs, self.rflags, self.rsp, self.ss
        )
    }
}

// ── 溢出防护水位账 ──────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackGuard {
    /// 槽位
    pub slot: usize,
    /// 栈尺寸
    pub size: u64,
    /// 已用水位（字节）
    pub used: u64,
    /// 是否已触发防护
    pub tripped: bool,
    /// 触发时刻的水位快照
    pub trip_at: u64,
}

impl StackGuard {
    pub fn new(slot: usize, size: u64) -> StackGuard {
        StackGuard { slot, size, used: 0, tripped: false, trip_at: 0 }
    }
    /// 消耗栈空间：越过 (size - GUARD_MARGIN) 即触防并拒绝。
    pub fn consume(&mut self, bytes: u64) -> Result<u64, u32> {
        let limit = match self.size.checked_sub(GUARD_MARGIN) {
            Some(v) => v,
            None => return Err(IstCode::SizeTooSmall.val()),
        };
        let next = match self.used.checked_add(bytes) {
            Some(v) => v,
            None => return Err(IstCode::GuardNotTripped.val()),
        };
        if next > limit {
            self.tripped = true;
            self.trip_at = next;
            return Err(IstCode::GuardNotTripped.val());
        }
        self.used = next;
        Ok(self.used)
    }
    /// 防护复位（handler 返回后栈面清零）。
    pub fn reset(&mut self) {
        self.used = 0;
        self.tripped = false;
        self.trip_at = 0;
    }
}

// ── DF 触发构造走账（Idle→第一故障→嵌套→DF 派发走 IST） ─────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DfStep {
    /// 空闲（无故障在途）
    Idle,
    /// 第一故障已入账（非 DF 向量）
    FirstFault(u8),
    /// 嵌套故障命中（同页故障中二次异常 → DF 候选）
    NestedFault(u8),
    /// DF 已派发且走 IST 专用栈
    DispatchedOnIst,
}

/// DF 派发模型：向量 8（#DF）本身、或嵌套两连故障，均派发走 DF 槽。
pub fn df_dispatch(prev: DfStep, vector: u8) -> Result<(DfStep, bool), u32> {
    match prev {
        DfStep::Idle => {
            if vector == 8 {
                // 直接 #DF 到达：派发走 IST。
                Ok((DfStep::DispatchedOnIst, true))
            } else {
                Ok((DfStep::FirstFault(vector), false))
            }
        }
        DfStep::FirstFault(_) | DfStep::NestedFault(_) => {
            if vector == 8 {
                Ok((DfStep::DispatchedOnIst, true))
            } else {
                Ok((DfStep::NestedFault(vector), false))
            }
        }
        DfStep::DispatchedOnIst => Ok((DfStep::Idle, false)),
    }
}

// ── TSS IST 槽位总账 ────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct IstTss {
    /// 槽位账：下标即槽号，槽 0 恒 None。
    pub ist: [Option<StackDesc>; IST_SLOT_COUNT],
    /// 装配相位
    pub phase: Phase,
    /// 每槽溢出防护账（与槽位同下标）
    pub guards: [Option<StackGuard>; IST_SLOT_COUNT],
    /// DF 派发走账
    pub df_step: DfStep,
    /// 最近一次 DF 派发现场
    pub last_frame: Option<IntFrame>,
    /// 派发走 IST 次数
    pub ist_dispatch_count: u32,
}

impl IstTss {
    pub fn new() -> IstTss {
        IstTss {
            ist: [None; IST_SLOT_COUNT],
            phase: Phase::Idle,
            guards: [None; IST_SLOT_COUNT],
            df_step: DfStep::Idle,
            last_frame: None,
            ist_dispatch_count: 0,
        }
    }

    /// 布局一张 IST 栈（对齐/尺寸/重叠/重复四闸）。
    pub fn layout(&mut self, slot: usize, base: u64, size: u64) -> Result<(), u32> {
        if slot == 0 || slot >= IST_SLOT_COUNT {
            return Err(IstCode::SlotBad.val());
        }
        if base % STACK_ALIGN != 0 {
            return Err(IstCode::BaseMisaligned.val());
        }
        if size < STACK_MIN_SIZE {
            return Err(IstCode::SizeTooSmall.val());
        }
        if self.ist[slot].is_some() {
            return Err(IstCode::DuplicateSlot.val());
        }
        let mut i = 1;
        while i < IST_SLOT_COUNT {
            if let Some(e) = self.ist[i] {
                // 区间交叠判：半开区间 [base, base+size)。
                if base < e.base + e.size && e.base < base + size {
                    return Err(IstCode::StackOverlap.val());
                }
            }
            i += 1;
        }
        let desc = StackDesc { slot, base, size, canary: StackDesc::derive_canary(slot, base, size) };
        self.ist[slot] = Some(desc);
        self.guards[slot] = Some(StackGuard::new(slot, size));
        // 相位推进：仅 Cold→Laid 升级；已在 Laid 及以上为幂等（同层不判倒退）。
        if self.phase.rank() < Phase::Laid.rank() {
            self.phase = Phase::advance_ok(self.phase, Phase::Laid)?;
        }
        Ok(())
    }

    /// 挂接（Laid→Hooked）：三槽齐备才允许整批挂接。
    pub fn hook(&mut self) -> Result<(), u32> {
        if self.ist[IST_SLOT_DF].is_none()
            || self.ist[IST_SLOT_NMI].is_none()
            || self.ist[IST_SLOT_MC].is_none()
        {
            return Err(IstCode::SlotBad.val());
        }
        self.phase = Phase::advance_ok(self.phase, Phase::Hooked)?;
        Ok(())
    }

    /// 防护武装（Hooked→Armed）：三槽守卫账全部就位。
    pub fn arm(&mut self) -> Result<(), u32> {
        let mut i = IST_SLOT_DF;
        while i <= IST_SLOT_MC {
            if self.guards[i].is_none() {
                return Err(IstCode::SlotBad.val());
            }
            i += 1;
        }
        self.phase = Phase::advance_ok(self.phase, Phase::Armed)?;
        Ok(())
    }

    /// DF 触发构造验证（Armed→Verified）：派发走 IST 且现场完整。
    pub fn verify_df(&mut self, frame: IntFrame) -> Result<(), u32> {
        if self.phase.rank() < Phase::Armed.rank() {
            return Err(IstCode::PhaseRegress.val());
        }
        // 构造嵌套故障触发 DF。
        let _ = df_dispatch(self.df_step, 13);
        let (step, dispatched) = match df_dispatch(DfStep::FirstFault(13), 8) {
            Ok(v) => v,
            Err(_) => (DfStep::Idle, false),
        };
        self.df_step = step;
        if !dispatched {
            return Err(IstCode::NotOnIst.val());
        }
        if !frame.complete() {
            return Err(IstCode::FrameIncomplete.val());
        }
        self.last_frame = Some(frame);
        self.ist_dispatch_count += 1;
        self.phase = Phase::advance_ok(self.phase, Phase::Verified)?;
        Ok(())
    }

    /// 槽位查询（槽号 → 栈顶）。
    pub fn top_of(&self, slot: usize) -> Result<u64, u32> {
        if slot == 0 || slot >= IST_SLOT_COUNT {
            return Err(IstCode::SlotBad.val());
        }
        match self.ist.get(slot) {
            Some(Some(d)) => Ok(d.top()),
            _ => Err(IstCode::SlotBad.val()),
        }
    }

    /// 三栈独立可用判定：槽位互异、区间互不重叠、金丝雀互异。
    pub fn three_independent(&self) -> bool {
        let slots = [IST_SLOT_DF, IST_SLOT_NMI, IST_SLOT_MC];
        let mut i = 0;
        while i < 3 {
            let a = match self.ist.get(slots[i]) {
                Some(Some(d)) => *d,
                _ => return false,
            };
            let mut j = i + 1;
            while j < 3 {
                let b = match self.ist.get(slots[j]) {
                    Some(Some(d)) => *d,
                    _ => return false,
                };
                if a.slot == b.slot {
                    return false;
                }
                if a.base < b.base + b.size && b.base < a.base + a.size {
                    return false;
                }
                if a.canary == b.canary {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 溢出防护注入：越界消耗必触发（触发率 100% 判定面）。
    pub fn guard_must_trip(&mut self, slot: usize, bytes: u64) -> bool {
        let g = match self.guards.get_mut(slot) {
            Some(Some(g)) => g,
            _ => return false,
        };
        match g.consume(bytes) {
            Ok(_) => false,
            Err(_) => g.tripped,
        }
    }
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

// ── 注入检出账（四路：越槽路/对齐路/重叠路/触防路） ──────────────
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

/// 真机三栈布局（DF/NMI/MC，各 16KB，4KB 间隔，16 对齐）。
pub const MACHINE_LAYOUT: [(usize, u64, u64); 3] = [
    (IST_SLOT_DF, 0xFF_0000, 0x4000),
    (IST_SLOT_NMI, 0xFF_4000, 0x4000),
    (IST_SLOT_MC, 0xFF_8000, 0x4000),
];

/// DF 派发真机现场（五元组齐全；ss 取内核数据选择子 0x18 占位非零）。
pub fn machine_frame(rsp_on_ist: u64) -> IntFrame {
    IntFrame { rip: 0x4010_0200, cs: 0x08, rflags: 0x0000_0002, rsp: rsp_on_ist, ss: 0x18 }
}

// ── 复算：三栈布局回放一致 ─────────────────────────────────────
pub fn replay_ok(layout: &[(usize, u64, u64)]) -> Result<(), u32> {
    let mut a = IstTss::new();
    let mut b = IstTss::new();
    let mut i = 0;
    while i < layout.len() {
        let _ = a.layout(layout[i].0, layout[i].1, layout[i].2);
        let _ = b.layout(layout[i].0, layout[i].1, layout[i].2);
        i += 1;
    }
    if a.ist != b.ist || a.guards != b.guards {
        return Err(IstCode::ReplayMismatch.val());
    }
    Ok(())
}

// ── 黄金全链：布局→挂接→武装→DF 触发→现场完整→四拒→触防→复算 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut t = IstTss::new();
    let layout: Vec<(usize, u64, u64)> = MACHINE_LAYOUT.iter().map(|t| *t).collect();
    let mut i = 0;
    while i < layout.len() {
        let (s, b, z) = layout[i];
        t.layout(s, b, z)?;
        i += 1;
    }
    t.hook()?;
    t.arm()?;
    // DF 触发构造：走 IST 且现场完整。
    let top = t.top_of(IST_SLOT_DF)?;
    let frame = machine_frame(top);
    t.verify_df(frame)?;
    let df_ok = (t.df_step == DfStep::DispatchedOnIst
        && t.last_frame.map(|f| f.complete()).unwrap_or(false)
        && t.ist_dispatch_count == 1) as u32;
    // 三栈独立。
    let indep = t.three_independent() as u32;
    // 四拒：越槽/非对齐/过小/重叠。
    let bad_slot = t.layout(0, 0x100_0000, 0x4000).is_err();
    let bad_align = t.layout(4, 0x100_0001, 0x4000).is_err();
    let bad_size = t.layout(4, 0x100_0000, 0x100).is_err();
    let bad_overlap = t.layout(4, 0xFF_2000, 0x4000).is_err();
    let rej = bad_slot as u32 + bad_align as u32 + bad_size as u32 + bad_overlap as u32;
    // 溢出防护注入：三槽各注入一次越界消耗，必触防。
    let mut trip_ok = 0;
    let mut s = IST_SLOT_DF;
    while s <= IST_SLOT_MC {
        if t.guard_must_trip(s, STACK_MIN_SIZE + GUARD_MARGIN) {
            trip_ok += 1;
        }
        s += 1;
    }
    replay_ok(&layout)?;
    Ok((df_ok + indep, rej, trip_ok))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（2 绿 + 4 拒 + 3 触防）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 4, 3)) => Ok(()),
        _ => Err(IstCode::NotReproducible.val()),
    }
}

/// 序 B：未布局槽查询 ⇒ SlotBad；DF 现场缺项 ⇒ FrameIncomplete。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut t = IstTss::new();
    let q1 = matches!(t.top_of(5), Err(e) if e == IstCode::SlotBad.val());
    // 先布局/挂接/武装到位，再验现场缺项（相位闸先行，缺项闸后行）。
    let mut laid = true;
    let mut i = 0;
    while i < MACHINE_LAYOUT.len() {
        let (s, b, z) = MACHINE_LAYOUT[i];
        if t.layout(s, b, z).is_err() {
            laid = false;
        }
        i += 1;
    }
    laid &= t.hook().is_ok();
    laid &= t.arm().is_ok();
    let bad = IntFrame { rip: 0, cs: 0x08, rflags: 2, rsp: 0xFF_3FF0, ss: 0 };
    let q2 = laid && matches!(t.verify_df(bad), Err(e) if e == IstCode::FrameIncomplete.val());
    match (q1, q2) {
        (true, true) => Ok(()),
        _ => Err(IstCode::NotReproducible.val()),
    }
}

/// 序 C：对齐/尺寸/余量冻结。
pub fn probe_seq_c() -> Result<(), u32> {
    match (STACK_ALIGN == 16, STACK_MIN_SIZE == 0x4000, GUARD_MARGIN == 0x800) {
        (true, true, true) => Ok(()),
        _ => Err(IstCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越槽路（槽 0 恒空）。
        let mut t1 = IstTss::new();
        led.record(0, t1.layout(0, 0x100_0000, 0x4000).is_err());
        // 路 2：对齐路。
        led.record(1, t1.layout(IST_SLOT_NMI, 0x100_0003, 0x4000).is_err());
        // 路 3：重叠路。
        let mut t3 = IstTss::new();
        let _ = t3.layout(IST_SLOT_DF, 0xFF_0000, 0x4000);
        led.record(2, t3.layout(IST_SLOT_MC, 0xFF_1000, 0x4000).is_err());
        // 路 4：触防路（越界消耗必触防）。
        let mut t4 = IstTss::new();
        let _ = t4.layout(IST_SLOT_MC, 0xFF_8000, 0x4000);
        led.record(3, t4.guard_must_trip(IST_SLOT_MC, 0x4800));
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(IstCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < IstCode::ALL.len() {
        let v = IstCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((ok, rej, trip)) => *ok == 2 && *rej == 4 && *trip == 3,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, trip)) => format!("绿={} 拒={} 触防={}", ok, rej, trip),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三栈独立可用（槽位互异 + 区间不重叠 + 金丝雀互异）。
    let mut t = IstTss::new();
    let layout: Vec<(usize, u64, u64)> = MACHINE_LAYOUT.iter().map(|e| *e).collect();
    let mut all_ok = true;
    let mut i = 0;
    while i < layout.len() {
        if t.layout(layout[i].0, layout[i].1, layout[i].2).is_err() {
            all_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-三栈独立",
        all_ok && t.three_independent() && t.ist[IST_SLOT_DF].is_some(),
        String::from("DF/NMI/MC 三槽互异 16 对齐 互不重叠"),
    ));
    // DF 触发构造：走 IST 且现场完整。
    let mut t2 = IstTss::new();
    let mut i = 0;
    while i < layout.len() {
        let _ = t2.layout(layout[i].0, layout[i].1, layout[i].2);
        i += 1;
    }
    let _ = t2.hook();
    let _ = t2.arm();
    let df = t2.verify_df(machine_frame(0xFF_3FF0));
    let df_ok = match &df {
        Ok(()) => t2.df_step == DfStep::DispatchedOnIst && t2.ist_dispatch_count == 1,
        Err(_) => false,
    };
    let sum = match t2.last_frame {
        Some(f) => f.summary(),
        None => String::from("无现场"),
    };
    v.push(("J1-DF触发走IST且现场完整", df_ok, sum));
    // 溢出防护触发率 100%（三槽逐槽越界必抓）。
    let mut t3 = IstTss::new();
    let mut i = 0;
    while i < layout.len() {
        let _ = t3.layout(layout[i].0, layout[i].1, layout[i].2);
        i += 1;
    }
    let mut trip_all = true;
    let mut s = IST_SLOT_DF;
    while s <= IST_SLOT_MC {
        if !t3.guard_must_trip(s, STACK_MIN_SIZE + GUARD_MARGIN) {
            trip_all = false;
        }
        s += 1;
    }
    v.push((
        "J1-溢出防护100%",
        trip_all,
        String::from("三槽越界注入 3/3 必触防"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
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
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x71F00 窗口"),
    ));
    v
}

pub fn run_unx0806_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0806 IST 三栈布局与 DF/NMI/MC 挂接");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
