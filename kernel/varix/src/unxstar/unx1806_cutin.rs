//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1806 · 切入路径（地址空间与栈衔接） · 0x72540
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B11）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1806-J1 切入序（地址空间切换→gs_base 装载→栈指针
//!     装载→上线）断言全过，地址空间一致断言（同 mm 免切换）全过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 切入序状态机：Idle→AddrSpace→GsBase→StackPtr→Online，四步严格定序，
//!     乱序/跳步/平移/倒退一律显性拒；
//!   · 地址空间一致断言：免切换标志必须与 (from_cr3==to_cr3) 严格同真同假
//!     ——同 mm 必免切换、异 mm 必真切换；cr3 影子寄存器在 Online 时逐值对账；
//!   · gs_base 必须非零且规范地址（bits 63:47 全同）；栈指针必须落在目标核
//!     栈窗内且 16 字节对齐；
//!   · 越界注入 10 路（乱序/零 gs/非规范 gs/栈越界/栈未对齐/cr3 影子篡改/
//!     跳步上线/重复切换/免切换标志失真/终态复用）10/10 必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72540，切入路径窗口独占）。
pub const SEG_BASE: u32 = 0x72540;
/// 目标核栈窗底（内核栈顶 - 栈深）。
pub const STACK_LO: u64 = 0xFFFF_FF80_0000_0000;
/// 目标核栈深（64 KiB）。
pub const STACK_SIZE: u64 = 0x1_0000;
/// 目标核栈窗顶（核队整片栈窗银行：CORE_N 核 × 单核栈深）。
pub const STACK_HI: u64 = STACK_LO + (CORE_N as u64) * STACK_SIZE;
/// 栈指针对齐（16 字节，SysV ABI）。
pub const SP_ALIGN: u64 = 16;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 诊断窗口宽（段内 0x40）。
pub const SEG_WINDOW: u32 = 0x40;
/// 核数（切入目标核覆盖面，冻结 4）。
pub const CORE_N: u32 = 4;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CutinCode {
    /// 切入序乱/跳步：四步严格定序，前一步没落位后一步一律拒
    StepOrder,
    /// gs_base 坏：零值或非规范地址（bits 63:47 不全同）
    GsBad,
    /// 栈指针坏：越出目标核栈窗或未 16 字节对齐
    SpBad,
    /// 地址空间不一致：免切换标志与 cr3 对不上 / cr3 影子被篡改
    AsMismatch,
    /// 相位倒退/平移：状态机单调，重复切换、终态复用一律拒
    PhaseBack,
    /// 地址空间页表坏：cr3 低两位特权位缺失或未按页对齐
    Cr3Bad,
    /// 切入目标核越权：核号 ≥ CORE_N
    CoreIdOff,
    /// 双跑不一致：golden_run 两次应同值（复现红线）
    NotReproducible,
}

impl CutinCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CutinCode::StepOrder => "切入序乱：地址空间→gs_base→栈指针→上线，严格定序不可跳",
            CutinCode::GsBad => "gs_base 坏：零值/非规范地址，按核心号重派规范基址",
            CutinCode::SpBad => "栈指针坏：必须落在目标核栈窗内且 16 字节对齐",
            CutinCode::AsMismatch => "地址空间不一致：同 mm 必免切换、异 mm 必真切换，cr3 影子逐值对账",
            CutinCode::PhaseBack => "相位倒退：切入状态机只进不退，重复切换/终态复用拒",
            CutinCode::Cr3Bad => "cr3 坏：页表基址须 4K 对齐且带低两位特权位",
            CutinCode::CoreIdOff => "目标核越权：核号超出舰队规模，查切入目标表",
            CutinCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [CutinCode; 8] = [
        CutinCode::StepOrder,
        CutinCode::GsBad,
        CutinCode::SpBad,
        CutinCode::AsMismatch,
        CutinCode::PhaseBack,
        CutinCode::Cr3Bad,
        CutinCode::CoreIdOff,
        CutinCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CutinCode::ALL.len() {
        if CutinCode::ALL[i].val() == code {
            return CutinCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72540 段闭集内"
}

// ── 地址空间模型 ────────────────────────────────────────────────
/// 地址空间：id + cr3。cr3 派生确定：页目录按 4K 对齐，低两位带特权位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddrSpace {
    pub id: u64,
    pub cr3: u64,
}

/// 确定性建空间：cr3 = 0x1000 + id*0x1000，低两位 |0x3。
pub fn build_as(id: u64) -> AddrSpace {
    AddrSpace { id, cr3: (0x1000u64.checked_add(id.wrapping_mul(0x1000)).unwrap_or(u64::MAX - 3)) | 0x3 }
}

/// cr3 形状审计：4K 对齐 + 低两位特权位。
pub fn cr3_shape_ok(cr3: u64) -> bool {
    (cr3 & 0xFFF) == 0x3
}

/// 规范地址审计：bits 63:47 全 0 或全 1。
pub fn is_canonical(v: u64) -> bool {
    let hi = v >> 47;
    hi == 0 || hi == 0x1_FFFF
}

/// gs_base 派生（确定性）：每核一个规范基址。
pub fn gs_base_for(core: u32) -> u64 {
    0x0000_0000_FEE0_0000u64.wrapping_add((core as u64).wrapping_mul(0x1000))
}

// ── 切入序状态机（Idle→AddrSpace→GsBase→StackPtr→Online） ───────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CutStep {
    /// 未切入。
    Idle,
    /// 地址空间已切换（或确认同 mm 免切换）。
    AddrSpace,
    /// gs_base 已装载。
    GsBase,
    /// 栈指针已装载。
    StackPtr,
    /// 上线完成。
    Online,
}

impl CutStep {
    /// 单调序值。
    pub fn rank(self) -> u8 {
        match self {
            CutStep::Idle => 0,
            CutStep::AddrSpace => 1,
            CutStep::GsBase => 2,
            CutStep::StackPtr => 3,
            CutStep::Online => 4,
        }
    }
    /// 只允许向前推进一步；平移与倒退一律 false。
    pub fn can_advance(self, to: CutStep) -> bool {
        to.rank() == self.rank() + 1
    }
}

// ── 切入上下文 ──────────────────────────────────────────────────
/// 一次切入的完整账：源/目标 cr3 + gs/栈 + 状态机。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cutin {
    pub from_cr3: u64,
    pub to_cr3: u64,
    /// 免切换标志：from==to 时必须为 true，异 mm 必须为 false。
    pub skip_switch: bool,
    /// cr3 影子寄存器（切完后实际生效值）。
    pub cr3_shadow: u64,
    pub gs: u64,
    pub rsp: u64,
    pub step: CutStep,
    pub core: u32,
    /// 簿记：各步落位计数（对账用）。
    pub steps_done: u32,
}

impl Cutin {
    /// 建切入账：记录源/目标 cr3，免切换标志按 cr3 相等判定。
    pub fn begin(from_cr3: u64, to: &AddrSpace, core: u32) -> Cutin {
        let skip = from_cr3 == to.cr3;
        Cutin {
            from_cr3,
            to_cr3: to.cr3,
            skip_switch: skip,
            cr3_shadow: from_cr3,
            gs: 0,
            rsp: 0,
            step: CutStep::Idle,
            core,
            steps_done: 0,
        }
    }

    /// 相位推进公开口：必须严格 +1，否则倒退/平移拒。
    pub fn advance(&mut self, to: CutStep) -> Result<(), u32> {
        if !self.step.can_advance(to) {
            return Err(CutinCode::PhaseBack.val());
        }
        self.step = to;
        self.steps_done += 1;
        Ok(())
    }
}

// ── 切入四步 ────────────────────────────────────────────────────
/// 第一步 地址空间切换：Idle→AddrSpace。异 mm 换影子；同 mm 免切换保影子。
pub fn switch_addr_space(c: &mut Cutin, to: &AddrSpace) -> Result<(), u32> {
    if c.step != CutStep::Idle {
        // 重复切换：平移按倒退拒。
        return Err(CutinCode::PhaseBack.val());
    }
    if !cr3_shape_ok(to.cr3) {
        return Err(CutinCode::Cr3Bad.val());
    }
    if to.cr3 != c.to_cr3 {
        return Err(CutinCode::AsMismatch.val());
    }
    if c.skip_switch {
        // 同 mm 免切换：影子保持源值（即目标值）。
        c.cr3_shadow = c.from_cr3;
    } else {
        c.cr3_shadow = to.cr3;
    }
    c.advance(CutStep::AddrSpace)
}

/// 第二步 gs_base 装载：AddrSpace→GsBase。跳步/零值/非规范一律拒。
pub fn load_gs_base(c: &mut Cutin, gs: u64) -> Result<(), u32> {
    if c.step != CutStep::AddrSpace {
        return Err(CutinCode::StepOrder.val());
    }
    if gs == 0 || !is_canonical(gs) {
        return Err(CutinCode::GsBad.val());
    }
    c.gs = gs;
    c.advance(CutStep::GsBase)
}

/// 第三步 栈指针装载：GsBase→StackPtr。跳步/越窗/未对齐一律拒。
pub fn load_stack_ptr(c: &mut Cutin, rsp: u64) -> Result<(), u32> {
    if c.step != CutStep::GsBase {
        return Err(CutinCode::StepOrder.val());
    }
    if rsp < STACK_LO || rsp >= STACK_HI {
        return Err(CutinCode::SpBad.val());
    }
    if rsp % SP_ALIGN != 0 {
        return Err(CutinCode::SpBad.val());
    }
    c.rsp = rsp;
    c.advance(CutStep::StackPtr)
}

/// 第四步 上线：StackPtr→Online。跳步拒；上线前做地址空间一致断言。
pub fn go_online(c: &mut Cutin) -> Result<(), u32> {
    if c.step != CutStep::StackPtr {
        return Err(CutinCode::StepOrder.val());
    }
    assert_as_consistency(c)?;
    c.advance(CutStep::Online)
}

/// 地址空间一致断言（判据②核心）：
///   ① 免切换标志必须与 (from_cr3==to_cr3) 同真同假；
///   ② cr3 影子必须等于实际生效值（同 mm=源，异 mm=目标）。
pub fn assert_as_consistency(c: &Cutin) -> Result<(), u32> {
    let expect_skip = c.from_cr3 == c.to_cr3;
    if c.skip_switch != expect_skip {
        return Err(CutinCode::AsMismatch.val());
    }
    let expect_shadow = if c.skip_switch { c.from_cr3 } else { c.to_cr3 };
    if c.cr3_shadow != expect_shadow {
        return Err(CutinCode::AsMismatch.val());
    }
    Ok(())
}

// ── 完整切入路径（黄金全链） ────────────────────────────────────
/// 核栈顶派生（确定性）：每核一窗，窗顶 = STACK_LO + (core+1)*STACK_SIZE。
pub fn stack_top_for(core: u32) -> u64 {
    STACK_LO.wrapping_add(((core as u64) + 1).wrapping_mul(STACK_SIZE))
}

/// 跨 mm 切入：完整四步。返回落位后的切入账。
pub fn cutin_cross(from: &AddrSpace, to: &AddrSpace, core: u32) -> Result<Cutin, u32> {
    if core >= CORE_N {
        return Err(CutinCode::CoreIdOff.val());
    }
    let mut c = Cutin::begin(from.cr3, to, core);
    switch_addr_space(&mut c, to)?;
    load_gs_base(&mut c, gs_base_for(core))?;
    let sp = stack_top_for(core).wrapping_sub(16);
    load_stack_ptr(&mut c, sp)?;
    go_online(&mut c)?;
    Ok(c)
}

/// 同 mm 免切换切入：from==to，第一步免切换，其余照走。
pub fn cutin_same_mm(mm: &AddrSpace, core: u32) -> Result<Cutin, u32> {
    if core >= CORE_N {
        return Err(CutinCode::CoreIdOff.val());
    }
    let mut c = Cutin::begin(mm.cr3, mm, core);
    switch_addr_space(&mut c, mm)?;
    load_gs_base(&mut c, gs_base_for(core))?;
    let sp = stack_top_for(core).wrapping_sub(16);
    load_stack_ptr(&mut c, sp)?;
    go_online(&mut c)?;
    Ok(c)
}

// ── 越界注入（10 路，判据 10/10 必抓） ──────────────────────────
/// 越界注入轮：第 kind 路构造一个违约现场，断言必须抓到。
/// 返回 (拒因诊断码, 是否抓到)。
pub fn inject_round(round: u32) -> Result<(u32, bool), u32> {
    let kind = (round % 10) as usize;
    let from = build_as(1);
    let to = build_as(2);
    let core = 0u32;
    match kind {
        0 => {
            // 乱序：栈指针先于地址空间步 → StepOrder 必拒。
            let mut c = Cutin::begin(from.cr3, &to, core);
            let sp = stack_top_for(core).wrapping_sub(8);
            match load_stack_ptr(&mut c, sp) {
                Err(code) => Ok((code, code == CutinCode::StepOrder.val())),
                Ok(()) => Ok((0, false)),
            }
        }
        1 => {
            // 零 gs_base → GsBad 必拒。
            let mut c = Cutin::begin(from.cr3, &to, core);
            let _ = switch_addr_space(&mut c, &to);
            match load_gs_base(&mut c, 0) {
                Err(code) => Ok((code, code == CutinCode::GsBad.val())),
                Ok(()) => Ok((0, false)),
            }
        }
        2 => {
            // 非规范 gs（bits 63:47 混合：0x8000_0000_0000 起）→ GsBad 必拒。
            let mut c = Cutin::begin(from.cr3, &to, core);
            let _ = switch_addr_space(&mut c, &to);
            let bad_gs: u64 = 0x0000_8000_0000_0000; // 高位非全同
            match load_gs_base(&mut c, bad_gs) {
                Err(code) => Ok((code, code == CutinCode::GsBad.val())),
                Ok(()) => Ok((0, false)),
            }
        }
        3 => {
            // 栈越界：低于栈窗底 → SpBad 必拒。
            let mut c = Cutin::begin(from.cr3, &to, core);
            let _ = switch_addr_space(&mut c, &to);
            let _ = load_gs_base(&mut c, gs_base_for(core));
            match load_stack_ptr(&mut c, STACK_LO - 0x100) {
                Err(code) => Ok((code, code == CutinCode::SpBad.val())),
                Ok(()) => Ok((0, false)),
            }
        }
        4 => {
            // 栈未对齐：rsp % 16 == 8 → SpBad 必拒。
            let mut c = Cutin::begin(from.cr3, &to, core);
            let _ = switch_addr_space(&mut c, &to);
            let _ = load_gs_base(&mut c, gs_base_for(core));
            let bad_sp = stack_top_for(core).wrapping_sub(8); // %16==8 未对齐
            match load_stack_ptr(&mut c, bad_sp) {
                Err(code) => Ok((code, code == CutinCode::SpBad.val())),
                Ok(()) => Ok((0, false)),
            }
        }
        5 => {
            // cr3 影子篡改：全链走通后翻影子 → 一致断言必抓。
            let mut c = match cutin_cross(&from, &to, core) {
                Ok(c) => c,
                Err(e) => return Err(e),
            };
            c.cr3_shadow = c.cr3_shadow.wrapping_add(0x1000);
            match assert_as_consistency(&c) {
                Err(code) => Ok((code, code == CutinCode::AsMismatch.val())),
                Ok(()) => Ok((0, false)),
            }
        }
        6 => {
            // 跳步上线：只切地址空间就直接上线 → StepOrder 必拒。
            let mut c = Cutin::begin(from.cr3, &to, core);
            let _ = switch_addr_space(&mut c, &to);
            match go_online(&mut c) {
                Err(code) => Ok((code, code == CutinCode::StepOrder.val())),
                Ok(()) => Ok((0, false)),
            }
        }
        7 => {
            // 重复切换：AddrSpace 后再切 → 平移必拒。
            let mut c = Cutin::begin(from.cr3, &to, core);
            let _ = switch_addr_space(&mut c, &to);
            match switch_addr_space(&mut c, &to) {
                Err(code) => Ok((code, code == CutinCode::PhaseBack.val())),
                Ok(()) => Ok((0, false)),
            }
        }
        8 => {
            // 免切换标志失真：同 mm 现场硬翻 skip=false → 一致断言必抓。
            let mm = build_as(3);
            let mut c = Cutin::begin(mm.cr3, &mm, core);
            c.skip_switch = false;
            match switch_addr_space(&mut c, &mm) {
                Err(code) => Ok((code, code == CutinCode::AsMismatch.val())),
                Ok(()) => match assert_as_consistency(&c) {
                    Err(code) => Ok((code, code == CutinCode::AsMismatch.val())),
                    Ok(()) => Ok((0, false)),
                },
            }
        }
        _ => {
            // 终态复用：Online 后再 load_gs → 跳步守卫必拒。
            let mut c = match cutin_cross(&from, &to, core) {
                Ok(c) => c,
                Err(e) => return Err(e),
            };
            match load_gs_base(&mut c, gs_base_for(core)) {
                Err(code) => Ok((code, code == CutinCode::StepOrder.val())),
                Ok(()) => Ok((0, false)),
            }
        }
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

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链（跨 mm 四步 + 同 mm 免切换）双绿。
pub fn probe_seq_a() -> Result<(), u32> {
    let from = build_as(1);
    let to = build_as(2);
    let cross = cutin_cross(&from, &to, 0)?;
    let mm = build_as(3);
    let same = cutin_same_mm(&mm, 1)?;
    let ok = cross.step == CutStep::Online
        && same.step == CutStep::Online
        && cross.steps_done == 4
        && same.steps_done == 4
        && cross.cr3_shadow == to.cr3
        && same.cr3_shadow == mm.cr3
        && same.skip_switch
        && !cross.skip_switch;
    if ok {
        Ok(())
    } else {
        Err(CutinCode::AsMismatch.val())
    }
}

/// 序 B：越界注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(CutinCode::StepOrder.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72540
        && CutinCode::NotReproducible.val() < SEG_BASE + SEG_WINDOW;
    let ok = STACK_SIZE == 0x1_0000 && SP_ALIGN == 16 && JUDGE_ROUNDS == 10
        && CORE_N == 4 && in_window
        && STACK_HI == STACK_LO.wrapping_add((CORE_N as u64).wrapping_mul(STACK_SIZE));
    if ok {
        Ok(())
    } else {
        Err(CutinCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let from = build_as(1);
    let to = build_as(2);
    let a = cutin_cross(&from, &to, 2)?;
    let b = cutin_cross(&from, &to, 2)?;
    if a != b {
        return Err(CutinCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CutinCode::ALL.len() {
        let v = CutinCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + SEG_WINDOW {
            return false;
        }
        i += 1;
    }
    true
}

// ── 全核切入扫（每核逐项过切入序） ─────────────────────────────
/// 全核扫描：0..CORE_N 逐核跨 mm + 同 mm 双路径，全绿返回过核数。
pub fn sweep_all_cores() -> Result<u32, u32> {
    let from = build_as(1);
    let to = build_as(2);
    let mm = build_as(3);
    let mut ok_n = 0u32;
    let mut core = 0u32;
    while core < CORE_N {
        let c1 = cutin_cross(&from, &to, core)?;
        let c2 = cutin_same_mm(&mm, core)?;
        if c1.step == CutStep::Online && c2.step == CutStep::Online {
            ok_n += 1;
        } else {
            return Err(CutinCode::StepOrder.val());
        }
        core += 1;
    }
    Ok(ok_n)
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let n = sweep_all_cores()?;
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    Ok(format!(
        "状态=已完成 cores={} order=AS→GS→SP→ONLINE inject={}/{} 免切换=通过",
        n, inj.caught, inj.tries
    ))
}

// ── 切入账渲染（逐步中文明细） ─────────────────────────────────
pub fn cutin_lines(c: &Cutin) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("源cr3=0x{:x} 目标cr3=0x{:x} 免切换={}", c.from_cr3, c.to_cr3, c.skip_switch));
    v.push(format!("cr3影子=0x{:x} gs=0x{:x} rsp=0x{:x}", c.cr3_shadow, c.gs, c.rsp));
    v.push(format!(
        "步骤序值={}/4 落位数={} 目标核={}",
        c.step.rank(), c.steps_done, c.core
    ));
    v
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：切入序四步断言（跨 mm 全链 + 逐步序值）。
    let from = build_as(1);
    let to = build_as(2);
    let g = cutin_cross(&from, &to, 0);
    let (gp, gd) = match &g {
        Ok(c) => (c.step == CutStep::Online && c.steps_done == 4 && c.cr3_shadow == to.cr3,
            format!("steps_done={} shadow=0x{:x}", c.steps_done, c.cr3_shadow)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-切入序四步断言", gp, gd));
    // J1 地址空间一致断言（同 mm 免切换）。
    let mm = build_as(3);
    let s = cutin_same_mm(&mm, 1);
    let (sp_, sd) = match &s {
        Ok(c) => (c.skip_switch && c.cr3_shadow == mm.cr3 && c.step == CutStep::Online,
            format!("skip={} shadow=0x{:x}", c.skip_switch, c.cr3_shadow)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-同mm免切换断言", sp_, sd));
    // J1 异 mm 必真切换（影子必须换到目标）。
    let cross_ok = match cutin_cross(&from, &to, 2) {
        Ok(c) => !c.skip_switch && c.cr3_shadow == to.cr3 && c.cr3_shadow != from.cr3,
        Err(_) => false,
    };
    v.push(("J1-异mm必真切换", cross_ok, String::from("skip=false 影子=目标cr3")));
    // J1 越界注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-越界注入10/10", ip, id));
    // J1 全核切入扫。
    let sw = sweep_all_cores();
    let sp2 = matches!(&sw, Ok(n) if *n == CORE_N);
    v.push(("J1-全核切入扫", sp2, format!("cores={} 冻结值={}", CORE_N, CORE_N)));
    // J2 单调守卫：状态机严格 +1（平移/跳步/倒退三拒）。
    let mut c3 = Cutin::begin(from.cr3, &to, 0);
    let m1 = c3.advance(CutStep::GsBase).is_err(); // 跳步拒
    let m2 = switch_addr_space(&mut c3, &to).is_ok();
    let m3 = c3.advance(CutStep::AddrSpace).is_err(); // 平移拒
    let m4 = c3.advance(CutStep::Idle).is_err(); // 倒退拒
    v.push((
        "J2-切入序单调守卫",
        m1 && m2 && m3 && m4,
        format!("跳步拒={} 平移拒={} 倒退拒={} 首步进={}", m1, m3, m4, m2),
    ));
    // J2 gs/栈形状审计：规范地址、栈窗、对齐边界值。
    let f1 = is_canonical(gs_base_for(0)) && is_canonical(gs_base_for(CORE_N - 1));
    let f2 = is_canonical(0x0000_7FFF_FFFF_FFFF) && !is_canonical(0x0000_8000_0000_0000);
    let sp_in = stack_top_for(0) <= STACK_HI && stack_top_for(CORE_N - 1) == STACK_HI;
    // 边界：栈窗顶（STACK_HI）越界必拒（GsBase 步就位后试装）。
    let mut ce = Cutin::begin(from.cr3, &to, 0);
    let _ = switch_addr_space(&mut ce, &to);
    let _ = load_gs_base(&mut ce, gs_base_for(0));
    let edge1 = load_stack_ptr(&mut ce, STACK_HI).is_err();
    let win_ok = STACK_LO < stack_top_for(0) && stack_top_for(0) <= STACK_HI;
    v.push((
        "J2-gs/栈形状审计",
        f1 && f2 && sp_in && edge1 && win_ok,
        format!("规范gs={} 非规范判={:?} 栈窗顶越界拒={}", f1, f2, edge1),
    ));
    // J2 cr3 形状审计。
    let c1 = cr3_shape_ok(build_as(0).cr3) && cr3_shape_ok(build_as(9).cr3);
    let c2 = !cr3_shape_ok(0x1000) && !cr3_shape_ok(0x1001);
    v.push(("J2-cr3形状审计", c1 && c2, String::from("4K对齐+低两位特权位；裸页目录/错位必拒")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("cutin_cross 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J3 切入账渲染非空。
    let lines = match cutin_cross(&from, &to, 3) {
        Ok(c) => cutin_lines(&c).len(),
        Err(_) => 0,
    };
    v.push(("J3-切入账渲染", lines >= 3, format!("行数={}", lines)));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72540 窗口")));
    v
}

pub fn run_unx1806_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1806 切入路径（地址空间与栈衔接）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
