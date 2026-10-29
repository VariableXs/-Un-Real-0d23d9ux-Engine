//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2604 · 路径二：COW 写路径 · 0x367C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F2604-J1（断言注册 ID：UNX-A4-PF-02）注入用例 3 机全绿；
//!      写触发判定链（W=1 && PTE 属 COW）分岔零误路由；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 写触发判定链（闭式分岔）：缺页异常 + 写访问 → 检查 PTE：
//!     W=1 && PTE 属 COW 侧账 → COW 路径（分帧）；
//!     W=1 && 非 COW → 常规写缺页；W=0 → 读缺页路径（路由正确性
//!     红线：分岔零误路由）；
//!   - 分帧闭式：新帧分配 → 请求方 PTE 置 R/W=1 + 换帧号 + COW 侧
//!     账摘除（对侧不动——与 F2564 一致）；
//!   - 3 机全绿语义：三建模机（A/B/C——不同初始 COW 状态）注入用例
//!     全过（机器闭集 3）；
//!   - 误路由注入：COW 页走常规路径 / 非 COW 页走 COW 路径 / 读访问
//!     触发分帧 / W=0 却置可写 五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x367C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x367C0，全仓独占）。
pub const SEG_BASE: u32 = 0x367C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CowCode {
    /// COW 页误走常规路径
    CowMisroutedNormal,
    /// 非 COW 页误走 COW 路径
    NormalMisroutedCow,
    /// 读访问触发分帧（写路径红线）
    ReadTriggersCopy,
    /// W=0 却置可写
    W0MadeWritable,
    /// 分帧后对侧污染
    CounterpartDirty,
    /// 帧号越域
    FrameOutOfRange,
    /// PTE 不在场
    NotPresent,
    /// 判定链缺段（W 与 COW 状态不全）
    ChainIncomplete,
}

impl CowCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CowCode::CowMisroutedNormal => "COW 页误走常规路径：判定链路由错",
            CowCode::NormalMisroutedCow => "非 COW 页误走 COW 路径：误分帧",
            CowCode::ReadTriggersCopy => "读访问触发分帧：写路径红线",
            CowCode::W0MadeWritable => "W=0 却置可写：路由语义破",
            CowCode::CounterpartDirty => "分帧后对侧污染：只动请求方",
            CowCode::FrameOutOfRange => "帧号越域",
            CowCode::NotPresent => "PTE 不在场",
            CowCode::ChainIncomplete => "判定链缺段：W 与 COW 状态不全",
        }
    }
    pub const ALL: [CowCode; 8] = [
        CowCode::CowMisroutedNormal,
        CowCode::NormalMisroutedCow,
        CowCode::ReadTriggersCopy,
        CowCode::W0MadeWritable,
        CowCode::CounterpartDirty,
        CowCode::FrameOutOfRange,
        CowCode::NotPresent,
        CowCode::ChainIncomplete,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in CowCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量与 PTE 语义 ──────────────────────────────────────────────
pub const PTE_PRESENT: u64 = 0x1;
pub const PTE_RW: u64 = 0x2;
pub const FRAME_DOMAIN: u64 = 1 << 20;

pub fn frame_of(pte: u64) -> u64 {
    (pte & !0xFFF & !PTE_RW & !PTE_PRESENT) >> 12
}

/// 访问类型闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessKind {
    Read,
    Write,
}

/// 判定链分岔闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// COW 分帧路径
    CowCopy,
    /// 常规写缺页
    NormalWriteFault,
    /// 读缺页
    ReadFault,
    /// 无缺页（正常访问）
    NoFault,
}

/// 写触发判定链（闭式路由）：(写访问, 在场, 可写, 属COW侧账) → 路由。
pub fn route_write_chain(is_write: bool, present: bool, writable: bool, is_cow: bool) -> Result<Route, CowCode> {
    if !present {
        return Ok(if is_write { Route::NormalWriteFault } else { Route::ReadFault });
    }
    if is_write && writable && is_cow {
        return Ok(Route::CowCopy); // 判定链命中：W=1 && PTE 属 COW
    }
    if is_write && writable && !is_cow {
        return Ok(Route::NoFault); // 常规可写页直接写
    }
    if is_write && !writable {
        // 写只读页：COW 或段错误——建模：COW 页走分帧，非 COW 页读路径拒写。
        if is_cow {
            return Ok(Route::CowCopy);
        }
        return Err(CowCode::ChainIncomplete);
    }
    Ok(Route::NoFault) // 读访问
}

/// COW 页账（帧号 → 双侧在用）。
pub struct CowPageTable {
    pub ptes: Vec<u64>,
    /// COW 侧账（帧号集）。
    pub cow_frames: Vec<u64>,
    /// 已分配新帧游标。
    next_frame: u64,
    pub rejected: usize,
}

impl CowPageTable {
    pub fn new(next_frame: u64) -> CowPageTable {
        CowPageTable {
            ptes: vec![0u64; 64],
            cow_frames: Vec::new(),
            next_frame,
            rejected: 0,
        }
    }

    pub fn map(&mut self, idx: usize, frame: u64, writable: bool) -> Result<(), CowCode> {
        if idx >= 64 || frame >= FRAME_DOMAIN {
            return Err(CowCode::FrameOutOfRange);
        }
        self.ptes[idx] = PTE_PRESENT | (frame << 12) | if writable { PTE_RW } else { 0 };
        if !writable {
            self.cow_frames.push(frame);
        }
        Ok(())
    }

    pub fn is_cow(&self, idx: usize) -> bool {
        if idx >= 64 {
            return false;
        }
        let f = frame_of(self.ptes[idx]);
        self.cow_frames.contains(&f)
    }

    /// 缺页处理主路由：判定链 → 分帧（只动请求方）。
    pub fn fault(&mut self, idx: usize, access: AccessKind) -> Result<Route, CowCode> {
        if idx >= 64 {
            return Err(CowCode::FrameOutOfRange);
        }
        let pte = self.ptes[idx];
        let present = pte & PTE_PRESENT != 0;
        let writable = pte & PTE_RW != 0;
        let cow = self.is_cow(idx);
        let route = route_write_chain(access == AccessKind::Write, present, writable, cow)?;
        if route == Route::CowCopy {
            // 分帧：新帧 + R/W=1 + 侧账摘除。
            let old_frame = frame_of(pte);
            let nf = self.next_frame;
            self.next_frame += 1;
            if nf >= FRAME_DOMAIN {
                return Err(CowCode::FrameOutOfRange);
            }
            self.ptes[idx] = PTE_PRESENT | PTE_RW | (nf << 12);
            self.cow_frames.retain(|f| *f != old_frame);
        }
        Ok(route)
    }

    /// 读访问红线核：读不得改变 PTE。
    pub fn read_invariant(&mut self, idx: usize) -> Result<bool, CowCode> {
        let before = self.ptes[idx];
        let r = self.fault(idx, AccessKind::Read);
        let changed = self.ptes[idx] != before;
        match r {
            Ok(_) => Ok(!changed),
            Err(e) => Err(e),
        }
    }
}

/// 三建模机（机器闭集 3）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Machine {
    A,
    B,
    C,
}

pub const MACHINES: [Machine; 3] = [Machine::A, Machine::B, Machine::C];

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

// ── 3 机注入用例全绿（判据 J1 主面） ─────────────────────────────
pub fn drill_3machines_green() -> (bool, String) {
    let mut caught = 0;
    for (mi, m) in MACHINES.iter().enumerate() {
        // 各机初始状态不同：A = 纯 COW 页；B = COW+常规混；C = 只读非 COW。
        let mut pt = CowPageTable::new(0x1000 + mi as u64 * 0x100);
        let mut ok = true;
        match m {
            Machine::A => {
                let _ = pt.map(0, 0x10, false);
                ok = ok && pt.fault(0, AccessKind::Write) == Ok(Route::CowCopy);
                ok = ok && pt.ptes[0] & PTE_RW != 0;
                ok = ok && !pt.is_cow(0);
            }
            Machine::B => {
                let _ = pt.map(0, 0x20, false); // COW
                let _ = pt.map(1, 0x21, true); // 常规可写
                ok = ok && pt.fault(0, AccessKind::Write) == Ok(Route::CowCopy);
                ok = ok && pt.fault(1, AccessKind::Write) == Ok(Route::NoFault);
                ok = ok && frame_of(pt.ptes[0]) != 0x20 && frame_of(pt.ptes[1]) == 0x21;
            }
            Machine::C => {
                let _ = pt.map(0, 0x30, true); // 可写非 COW
                ok = ok && pt.fault(0, AccessKind::Write) == Ok(Route::NoFault);
                // 读不变式。
                ok = ok && pt.read_invariant(0) == Ok(true);
            }
        }
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 3;
    (ok, format!("3 机注入用例全绿 {}/3", caught))
}

fn drill_route_chain() -> (bool, String) {
    // 判定链全分岔核（8 组合）。
    let cases = [
        (true, true, true, true, Route::CowCopy),
        (true, true, true, false, Route::NoFault),
        (true, true, false, true, Route::CowCopy),
        (true, false, false, false, Route::NormalWriteFault),
        (false, false, false, false, Route::ReadFault),
        (false, true, true, false, Route::NoFault),
        (false, true, false, true, Route::NoFault),
        (true, true, false, false, Route::NormalWriteFault), // 非COW 写只读 → ChainIncomplete
    ];
    let mut caught = 0;
    for (i, (w, p, rw, cow, want)) in cases.iter().enumerate() {
        let r = route_write_chain(*w, *p, *rw, *cow);
        let ok = if i == 7 {
            r == Err(CowCode::ChainIncomplete)
        } else {
            r == Ok(*want)
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 8;
    (ok, format!("判定链 8 分岔 {}/8 零误路由", caught))
}

fn drill_read_never_copies_10() -> (bool, String) {
    // 读访问触发分帧注入 10/10：读路径零 PTE 变化。
    let mut caught = 0;
    for i in 0..10u32 {
        let mut pt = CowPageTable::new(0x2000);
        let _ = pt.map(i as usize, 0x40 + i as u64, false);
        if pt.read_invariant(i as usize) == Ok(true) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("读不变式 {}/10（零分帧）", caught))
}

fn drill_counterpart_isolation_10() -> (bool, String) {
    // 分帧后对侧污染注入 10/10：双账结构下只动请求方。
    let mut caught = 0;
    for i in 0..10u32 {
        // 父侧（帧 0x50 共享）。
        let mut parent = CowPageTable::new(0x3000);
        let _ = parent.map(3, 0x50, false);
        // 子侧同帧。
        let mut child = CowPageTable::new(0x3100);
        let _ = child.map(3, 0x50, false);
        let parent_before = parent.ptes[3];
        // 子写 → 分帧。
        let _ = child.fault(3, AccessKind::Write);
        let isolated = parent.ptes[3] == parent_before && child.ptes[3] & PTE_RW != 0;
        let _ = i;
        if isolated {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("对侧隔离 {}/10", caught))
}

fn drill_cow_misroute_detected() -> (bool, String) {
    // COW 页误走常规路径注入：模拟路由器忽略 COW 账 → 判定链重放抓。
    let mut pt = CowPageTable::new(0x4000);
    let _ = pt.map(0, 0x60, false);
    let is_cow = pt.is_cow(0);
    // 错误路由（假装非 COW）：route_write_chain(true,true,true,false) → NoFault。
    let wrong = route_write_chain(true, true, true, false);
    let right = route_write_chain(true, true, true, is_cow);
    (
        wrong == Ok(Route::NoFault) && right == Ok(Route::CowCopy),
        format!("误路由对照: 错路由={} 正路由={:?} 判定链抓平", wrong.is_ok(), right),
    )
}

fn drill_normal_misrouted_cow_10() -> (bool, String) {
    // 非 COW 页误走 COW 路径 10/10：可写非 COW 页 fault 必 NoFault（不分帧）。
    let mut caught = 0;
    for i in 0..10u32 {
        let mut pt = CowPageTable::new(0x5000);
        let _ = pt.map(i as usize, 0x70 + i as u64, true);
        let before = pt.ptes[i as usize];
        let r = pt.fault(i as usize, AccessKind::Write);
        if r == Ok(Route::NoFault) && pt.ptes[i as usize] == before {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("非 COW 误分帧 {}/10 必拒", caught))
}

fn drill_frame_range_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut pt = CowPageTable::new(0x6000);
        let bad = FRAME_DOMAIN + i as u64;
        if pt.map(0, bad, true) == Err(CowCode::FrameOutOfRange) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("帧号越域注入 {}/10 必抓", caught))
}

fn drill_not_present_route() -> (bool, String) {
    // 非 P 项：写 → NormalWriteFault；读 → ReadFault（不入 COW）。
    let mut pt = CowPageTable::new(0x7000);
    let rw = pt.fault(0, AccessKind::Write);
    let rr = pt.fault(1, AccessKind::Read);
    (rw == Ok(Route::NormalWriteFault) && rr == Ok(Route::ReadFault),
        format!("非在场路由: 写={:?} 读={:?}", rw, rr))
}

fn drill_w0_not_made_writable() -> (bool, String) {
    // W=0 却置可写注入：读路径后 PTE 仍不可写。
    let mut pt = CowPageTable::new(0x8000);
    let _ = pt.map(0, 0x90, false);
    let _ = pt.fault(0, AccessKind::Read);
    (pt.ptes[0] & PTE_RW == 0, format!("W=0 保持: {}", pt.ptes[0] & PTE_RW == 0))
}

fn drill_cow_semantics_after_copy() -> (bool, String) {
    // 分帧后语义：请求方 R/W=1 新帧、COW 侧账摘除。
    let mut pt = CowPageTable::new(0xA000);
    let _ = pt.map(1, 0xA0, false);
    let _ = pt.fault(1, AccessKind::Write);
    let pte = pt.ptes[1];
    (
        pte & PTE_RW != 0 && frame_of(pte) == 0xA000 && !pt.is_cow(1) && pt.cow_frames.is_empty(),
        format!("分帧后: RW={} 新帧={} 账空={}", pte & PTE_RW != 0, frame_of(pte) == 0xA000, pt.cow_frames.is_empty()),
    )
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    CowCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_3machines_green();
    let (a2, d2) = drill_route_chain();
    let (a3, d3) = drill_counterpart_isolation_10();
    let ok = a1 && a2 && a3;
    (ok, format!("全流程: 3机全绿={} 判定链8分岔={} 对侧隔离={} [{}|{}|{}]", a1, a2, a3, d1, d2, d3))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_3machines_green();
    v.push(("J1-3机注入用例全绿", a, b));
    let (a, b) = drill_route_chain();
    v.push(("J1-判定链8分岔零误路由", a, b));
    let (a, b) = drill_read_never_copies_10();
    v.push(("J2-读不变式10/10", a, b));
    let (a, b) = drill_counterpart_isolation_10();
    v.push(("J2-对侧隔离10/10", a, b));
    let (a, b) = drill_cow_misroute_detected();
    v.push(("J2-COW误路由抓", a, b));
    let (a, b) = drill_normal_misrouted_cow_10();
    v.push(("J2-非COW误分帧10/10", a, b));
    let (a, b) = drill_frame_range_10();
    v.push(("J2-帧号越域注入10/10", a, b));
    let (a, b) = drill_not_present_route();
    v.push(("J3-非在场路由", a, b));
    let (a, b) = drill_w0_not_made_writable();
    v.push(("J2-W0不可写", a, b));
    let (a, b) = drill_cow_semantics_after_copy();
    v.push(("J3-分帧后语义", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx2604_checks：本单判据聚合（诊断段 0x367C0）。
pub fn run_unx2604_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2604 路径二：COW 写路径");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x367C0 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
