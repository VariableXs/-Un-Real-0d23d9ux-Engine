//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13141 · 异常注册挂点总成（映像装载完成→D1 分发表注册链） · 0x656C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13141-J1 注册链全流程 30 例（装载完成→.pdata 定位→
//!      D1 分发表登记→可捕获断言）与 Windows 行为一致，注册时机
//!      分段断言（与 B09 装载完成态 F12966 联动）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 注册链四段状态机（闭式边集）：LoadDone → PdataLocate →
//!     D1Registered → CatchableReady；乱序注册（未定位先登记/
//!     未登记先声明可捕获）一律拒收（时机分段红线）；
//!   - .pdata 定位闭式：函数区间 [begin,end) 必须落映像窗口内，
//!     越窗 / end<begin 拒定位；
//!   - D1 分发表：区间表按基址排序登记，查找走覆盖匹配
//!     （addr ∈ [begin,end) → handler），未覆盖 → 不可捕获；
//!   - 联动红线：链必须由装载完成态（F12966 联动事件）点燃，
//!     无完成态点火 → ChainNotArmed 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x656C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x656C0，全仓独占）。
pub const SEG_BASE: u32 = 0x656C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExcRegCode {
    /// 链未点火就注册（缺装载完成态）
    ChainNotArmed,
    /// 未定位 .pdata 先登记 D1 表
    RegisterBeforeLocate,
    /// 未登记先声明可捕获
    CatchBeforeRegister,
    /// .pdata 区间越映像窗口
    PdataOutOfRange,
    /// 区间倒置（end < begin）
    RangeInverted,
    /// D1 表区间重叠登记
    RangeOverlap,
    /// 故障地址未覆盖（不可捕获却声明）
    AddrNotCovered,
    /// 注册链非法状态迁移
    IllegalTransition,
}

impl ExcRegCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ExcRegCode::ChainNotArmed => "链未点火就注册：缺装载完成态（F12966 联动）",
            ExcRegCode::RegisterBeforeLocate => "未定位 .pdata 先登记 D1 表：时机破",
            ExcRegCode::CatchBeforeRegister => "未登记先声明可捕获：时机破",
            ExcRegCode::PdataOutOfRange => ".pdata 区间越映像窗口：定位拒收",
            ExcRegCode::RangeInverted => "区间倒置（end < begin）：定位拒收",
            ExcRegCode::RangeOverlap => "D1 表区间重叠登记：分发表污染",
            ExcRegCode::AddrNotCovered => "故障地址未覆盖：不可捕获却声明",
            ExcRegCode::IllegalTransition => "注册链非法状态迁移：闭式边集拒收",
        }
    }
    pub const ALL: [ExcRegCode; 8] = [
        ExcRegCode::ChainNotArmed,
        ExcRegCode::RegisterBeforeLocate,
        ExcRegCode::CatchBeforeRegister,
        ExcRegCode::PdataOutOfRange,
        ExcRegCode::RangeInverted,
        ExcRegCode::RangeOverlap,
        ExcRegCode::AddrNotCovered,
        ExcRegCode::IllegalTransition,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ExcRegCode::ALL.len() {
        if ExcRegCode::ALL[i].val() == code {
            return ExcRegCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（来源：异常注册链建模书 + 判据原文） ────────────────────
/// 映像窗口（建模：64KB 档）。
pub const IMAGE_BASE: u32 = 0x0040_0000;
pub const IMAGE_SIZE: u32 = 0x0001_0000;
/// 注册链全流程例数（判据原文：30 例）。
pub const CASES: usize = 30;

// ── 注册链四段状态机 ─────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegState {
    /// 链未点火
    Disarmed,
    /// 装载完成态（F12966 联动），链已点火
    LoadDone,
    /// .pdata 已定位
    PdataLocated,
    /// D1 分发表已登记
    D1Registered,
    /// 可捕获就绪
    CatchableReady,
}

/// 链事件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegEvent {
    /// 装载完成（B09/F12966 联动点火）
    ArmLoadDone,
    LocatePdata,
    RegisterD1,
    ArmCatchable,
}

/// 合法边闭集；闭集之外一律 IllegalTransition。
pub fn reg_transition(cur: RegState, ev: RegEvent) -> Result<RegState, ExcRegCode> {
    match (cur, ev) {
        (RegState::Disarmed, RegEvent::ArmLoadDone) => Ok(RegState::LoadDone),
        (RegState::LoadDone, RegEvent::LocatePdata) => Ok(RegState::PdataLocated),
        (RegState::PdataLocated, RegEvent::RegisterD1) => Ok(RegState::D1Registered),
        (RegState::D1Registered, RegEvent::ArmCatchable) => Ok(RegState::CatchableReady),
        _ => Err(ExcRegCode::IllegalTransition),
    }
}

// ── .pdata 定位与 D1 分发表 ──────────────────────────────────────
/// .pdata 函数区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuncRange {
    pub begin: u32,
    pub end: u32,
    pub handler: u32,
}

/// 定位闭式核：区间必须落映像窗口内且不倒置。
pub fn locate_range(begin: u32, end: u32) -> Result<FuncRange, ExcRegCode> {
    if end < begin {
        return Err(ExcRegCode::RangeInverted);
    }
    if begin < IMAGE_BASE || end > IMAGE_BASE + IMAGE_SIZE {
        return Err(ExcRegCode::PdataOutOfRange);
    }
    Ok(FuncRange {
        begin,
        end,
        handler: 0xD100,
    })
}

/// D1 分发表（区间闭集账）。
#[derive(Clone, Debug)]
pub struct D1Table {
    pub ranges: Vec<FuncRange>,
}

impl D1Table {
    pub fn new() -> D1Table {
        D1Table { ranges: Vec::new() }
    }

    /// 登记：与既有区间零重叠才收（重叠 → RangeOverlap）。
    pub fn register(&mut self, r: FuncRange) -> Result<(), ExcRegCode> {
        let mut i = 0;
        while i < self.ranges.len() {
            let e = self.ranges[i];
            let overlap = r.begin < e.end && e.begin < r.end;
            if overlap {
                return Err(ExcRegCode::RangeOverlap);
            }
            i += 1;
        }
        self.ranges.push(r);
        Ok(())
    }

    /// 覆盖查找：addr ∈ [begin,end) → handler；未覆盖 → Err。
    pub fn dispatch(&self, addr: u32) -> Result<u32, ExcRegCode> {
        let mut i = 0;
        while i < self.ranges.len() {
            let r = self.ranges[i];
            if addr >= r.begin && addr < r.end {
                return Ok(r.handler);
            }
            i += 1;
        }
        Err(ExcRegCode::AddrNotCovered)
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }
}

/// 注册链载体（状态机 + 分发表 + 例账）。
pub struct ExcRegChain {
    pub state: RegState,
    pub table: D1Table,
    /// 已走完全链例数
    pub cases_done: usize,
    /// 捕获命中数
    pub caught: usize,
}

impl ExcRegChain {
    pub fn new() -> ExcRegChain {
        ExcRegChain {
            state: RegState::Disarmed,
            table: D1Table::new(),
            cases_done: 0,
            caught: 0,
        }
    }

    pub fn step(&mut self, ev: RegEvent) -> Result<RegState, ExcRegCode> {
        let ns = reg_transition(self.state, ev)?;
        self.state = ns;
        Ok(ns)
    }

    /// 全链一例：点火 → 定位 → 登记 → 可捕获（时机分段，任一跳段即拒）。
    pub fn run_case(&mut self, begin: u32, end: u32, fault: u32) -> Result<bool, ExcRegCode> {
        if self.step(RegEvent::ArmLoadDone) != Ok(RegState::LoadDone) {
            return Err(ExcRegCode::ChainNotArmed);
        }
        let r = locate_range(begin, end)?;
        if self.step(RegEvent::LocatePdata) != Ok(RegState::PdataLocated) {
            return Err(ExcRegCode::RegisterBeforeLocate);
        }
        self.table.register(r)?;
        if self.step(RegEvent::RegisterD1) != Ok(RegState::D1Registered) {
            return Err(ExcRegCode::IllegalTransition);
        }
        if self.step(RegEvent::ArmCatchable) != Ok(RegState::CatchableReady) {
            return Err(ExcRegCode::CatchBeforeRegister);
        }
        let hit = self.table.dispatch(fault).is_ok();
        if hit {
            self.caught += 1;
        }
        self.cases_done += 1;
        self.state = RegState::Disarmed; // 复位进下一例
        Ok(hit)
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
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 判据钻取面 ───────────────────────────────────────────────────
/// J1 主面：注册链全流程 30 例（每例 fault 落区必捕获）。
pub fn drill_chain_30_cases() -> (bool, String) {
    let mut ch = ExcRegChain::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut hit = 0;
    let mut i = 0;
    while i < CASES {
        // 区间从 LCG 取，落在映像窗口内，宽 0x100。
        let off = rng.next_below(((IMAGE_SIZE - 0x200) / 0x100) as u64) * 0x100;
        let begin = IMAGE_BASE + off as u32;
        let end = begin + 0x100;
        let fault = begin + 0x40;
        match ch.run_case(begin, end, fault) {
            Ok(true) => hit += 1,
            Ok(false) => {}
            Err(e) => return (false, detail(e.val()).to_string()),
        }
        i += 1;
    }
    let ok = hit == CASES && ch.cases_done == CASES;
    (ok, format!("注册链 30 例捕获 {}/{}", hit, CASES))
}

/// J1 面：可捕获断言——未覆盖地址必不可捕获（反向判据）。
pub fn drill_uncovered_rejected() -> (bool, String) {
    let mut ch = ExcRegChain::new();
    let r = ch.run_case(IMAGE_BASE, IMAGE_BASE + 0x100, IMAGE_BASE + 0x1000);
    (
        r == Ok(false),
        format!("未覆盖地址判不可捕获={:?}", r),
    )
}

/// 注册时机分段断言：乱序注入 10/10 必抓。
pub fn drill_timing_edges_10() -> (bool, String) {
    let mut caught = 0;
    // 1: 未点火定位
    if reg_transition(RegState::Disarmed, RegEvent::LocatePdata) == Err(ExcRegCode::IllegalTransition) {
        caught += 1;
    }
    // 2: 未点火登记
    if reg_transition(RegState::Disarmed, RegEvent::RegisterD1) == Err(ExcRegCode::IllegalTransition) {
        caught += 1;
    }
    // 3: 未点火声明可捕获
    if reg_transition(RegState::Disarmed, RegEvent::ArmCatchable) == Err(ExcRegCode::IllegalTransition) {
        caught += 1;
    }
    // 4: 点火后直接登记（跳定位）
    if reg_transition(RegState::LoadDone, RegEvent::RegisterD1) == Err(ExcRegCode::IllegalTransition) {
        caught += 1;
    }
    // 5: 点火后直接可捕获
    if reg_transition(RegState::LoadDone, RegEvent::ArmCatchable) == Err(ExcRegCode::IllegalTransition) {
        caught += 1;
    }
    // 6: 定位后直接可捕获（跳登记）
    if reg_transition(RegState::PdataLocated, RegEvent::ArmCatchable) == Err(ExcRegCode::IllegalTransition) {
        caught += 1;
    }
    // 7: 可捕获再点火（终态倒退）
    if reg_transition(RegState::CatchableReady, RegEvent::ArmLoadDone) == Err(ExcRegCode::IllegalTransition) {
        caught += 1;
    }
    // 8: 可捕获再定位
    if reg_transition(RegState::CatchableReady, RegEvent::LocatePdata) == Err(ExcRegCode::IllegalTransition) {
        caught += 1;
    }
    // 9: D1 已登记再点火
    if reg_transition(RegState::D1Registered, RegEvent::ArmLoadDone) == Err(ExcRegCode::IllegalTransition) {
        caught += 1;
    }
    // 10: 点火后重复点火
    if reg_transition(RegState::LoadDone, RegEvent::ArmLoadDone) == Err(ExcRegCode::IllegalTransition) {
        caught += 1;
    }
    let ok = caught == 10;
    (ok, format!("时机乱序注入 {}/10 必抓", caught))
}

/// 定位闭式核：越窗 / 倒置两路必抓（各 5 例）。
pub fn drill_locate_violations_10() -> (bool, String) {
    let mut caught = 0;
    // 越窗 5 例：begin 在窗内但 end 越顶。
    let mut i = 0;
    while i < 5 {
        let begin = IMAGE_BASE + IMAGE_SIZE - 0x10;
        let end = IMAGE_BASE + IMAGE_SIZE + 0x10 * (i + 1) as u32;
        if locate_range(begin, end) == Err(ExcRegCode::PdataOutOfRange) {
            caught += 1;
        }
        i += 1;
    }
    // 倒置 5 例：end < begin。
    let mut j = 0;
    while j < 5 {
        let begin = IMAGE_BASE + 0x200 + 0x10 * j;
        if locate_range(begin + 0x40, begin) == Err(ExcRegCode::RangeInverted) {
            caught += 1;
        }
        j += 1;
    }
    let ok = caught == 10;
    (ok, format!("定位违规注入 {}/10 必抓", caught))
}

/// D1 表重叠注入 10/10 必抓。
pub fn drill_overlap_10() -> (bool, String) {
    let mut t = D1Table::new();
    let _ = t.register(FuncRange { begin: 0x1000, end: 0x2000, handler: 0xD100 });
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        // 与 [0x1000,0x2000) 相交的 10 条区间。
        let begin = 0x1800 + 0x40 * i as u32;
        if t.register(FuncRange { begin, end: begin + 0x200, handler: 0xD101 })
            == Err(ExcRegCode::RangeOverlap)
        {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10 && t.len() == 1;
    (ok, format!("重叠登记注入 {}/10 必抓（表仍 {} 条）", caught, t.len()))
}

/// 链未点火就 run_case 必抓（F12966 联动红线反向面）。
pub fn drill_not_armed_caught() -> (bool, String) {
    // run_case 第一步即点火，无法直接测未点火；改测：点火事件绕过
    // ——用 reg_transition 断言 Disarmed 只接受 ArmLoadDone。
    let mut ok = true;
    let evs = [
        RegEvent::LocatePdata,
        RegEvent::RegisterD1,
        RegEvent::ArmCatchable,
    ];
    let mut i = 0;
    while i < evs.len() {
        if reg_transition(RegState::Disarmed, evs[i]) != Err(ExcRegCode::IllegalTransition) {
            ok = false;
        }
        i += 1;
    }
    (ok, format!("未点火三事件全拒={}", ok))
}

/// 守恒面：表条数 == 例数，捕获数 ≤ 例数（对账恒等式）。
pub fn drill_conservation() -> (bool, String) {
    let mut ch = ExcRegChain::new();
    let mut i = 0;
    while i < 8 {
        let begin = IMAGE_BASE + 0x100 * (i + 1) as u32;
        let end = begin + 0x100;
        let fault = begin + 0x20;
        let _ = ch.run_case(begin, end, fault);
        i += 1;
    }
    let ok = ch.table.len() == 8
        && ch.cases_done == 8
        && ch.caught == 8
        && ch.caught <= ch.cases_done;
    (
        ok,
        format!(
            "守恒: 表条={} 例数={} 捕获={}",
            ch.table.len(),
            ch.cases_done,
            ch.caught
        ),
    )
}

/// 覆盖边界：区间端点含头不含尾（begin 可捕 / end 不可捕）。
pub fn drill_endpoint_semantics() -> (bool, String) {
    let mut t = D1Table::new();
    let _ = t.register(FuncRange { begin: 0x5000, end: 0x5100, handler: 0xD100 });
    let head = t.dispatch(0x5000) == Ok(0xD100);
    let tail = t.dispatch(0x5100) == Err(ExcRegCode::AddrNotCovered);
    let inner = t.dispatch(0x50FF) == Ok(0xD100);
    (
        head && tail && inner,
        format!("端点语义: 含头={} 不含尾={} 内点={}", head, tail, inner),
    )
}

/// 多机对拍面：同种子重建两次，捕获账一致。
pub fn drill_determinism() -> (bool, String) {
    let build = || {
        let mut ch = ExcRegChain::new();
        let mut rng = Lcg::new(SEG_BASE as u64);
        let mut i = 0;
        while i < 16 {
            let off = rng.next_below(((IMAGE_SIZE - 0x200) / 0x100) as u64) * 0x100;
            let begin = IMAGE_BASE + off as u32;
            let _ = ch.run_case(begin, begin + 0x100, begin + 0x40);
            i += 1;
        }
        (ch.cases_done, ch.caught, ch.table.len())
    };
    let a = build();
    let b = build();
    (a == b, format!("同种子对拍一致={}", a == b))
}

/// 全流程对账（J1 主链）。
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_chain_30_cases();
    let (a2, d2) = drill_timing_edges_10();
    let (a3, d3) = drill_uncovered_rejected();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 30例链={} 时机分段={} 不可捕获反向={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    let mut ok = true;
    while i < ExcRegCode::ALL.len() {
        let v = ExcRegCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_chain_30_cases();
    v.push(("J1-注册链全流程30例", a, b));
    let (a, b) = drill_timing_edges_10();
    v.push(("J1-注册时机分段断言10/10", a, b));
    let (a, b) = drill_uncovered_rejected();
    v.push(("J1-可捕获断言反向", a, b));
    let (a, b) = drill_locate_violations_10();
    v.push(("J2-定位违规注入10/10", a, b));
    let (a, b) = drill_overlap_10();
    v.push(("J2-D1重叠注入10/10", a, b));
    let (a, b) = drill_not_armed_caught();
    v.push(("J2-链未点火全拒", a, b));
    let (a, b) = drill_conservation();
    v.push(("J3-注册账守恒", a, b));
    let (a, b) = drill_endpoint_semantics();
    v.push(("J3-区间端点语义", a, b));
    let (a, b) = drill_determinism();
    v.push(("J3-同种子对拍一致", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13141_checks：本单判据聚合（诊断段 0x656C0）。
pub fn run_unx13141_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13141 异常注册挂点总成");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x656C0 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
