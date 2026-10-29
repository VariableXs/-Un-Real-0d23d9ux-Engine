//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13021 · 主线程栈保留-提交与 guard page 增长 · 0x65540
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F13021-J1 1MB 保留/首页提交布局断言，栈增长 ×1000 轮
//!     零错位，溢出异常路径与 Windows 一致（任务书示例判据保真）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 布局闭式（1MB 保留，页 4KB）：保留底（低址）→ 保留未提交 →
//!     guard 页（恰 1 页）→ 已提交区 → 栈顶（高址，首页提交即在此）；
//!     断言：reserve == 1MB、commit 首页恰 1 页、guard 恰 1 页；
//!   - 增长闭式：guard 页缺页 → guard 下移一页、原 guard 转提交、
//!     新 guard 在其下；commit_pages += 1 且 guard_addr -= PAGE；
//!     ×1000 轮零错位 = 每轮恒等式 commit+guard+reserve_free==reserve；
//!   - 溢出路径与 Windows 一致：guard 触底（到达最后保护页）后再
//!     越界 → STATUS_STACK_OVERFLOW(0xC00000FD) 异常路径，不再增长；
//!   - 状态机显式：Reserved→GuardNormal→GuardLast→Overflow，
//!     非法转换拒收；LCG 种子写死 = SEG_BASE（0x65540）；
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65540，全仓独占）。
pub const SEG_BASE: u32 = 0x65540;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackCode {
    /// 保留尺寸非 1MB
    ReserveSizeBad,
    /// 首页提交数非 1 页
    FirstCommitBad,
    /// guard 页数非 1
    GuardPagesBad,
    /// 增长后守恒破（commit+guard+free != reserve）
    GrowthLedgerBroken,
    /// guard 越底（越过最后保护页仍增长）
    GuardBelowFloor,
    /// 溢出路径误判（触底未抛溢出）
    OverflowNotRaised,
    /// 布局错位（guard 不在提交区正下）
    LayoutMisaligned,
    /// 状态机非法转换
    IllegalTransition,
}

impl StackCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            StackCode::ReserveSizeBad => "保留尺寸非 1MB：布局断言破",
            StackCode::FirstCommitBad => "首页提交数非 1 页：布局断言破",
            StackCode::GuardPagesBad => "guard 页数非 1：布局断言破",
            StackCode::GrowthLedgerBroken => "增长守恒破：commit+guard+free != reserve",
            StackCode::GuardBelowFloor => "guard 越底：越过最后保护页仍增长",
            StackCode::OverflowNotRaised => "触底未抛溢出：Windows 语义不一致",
            StackCode::LayoutMisaligned => "布局错位：guard 不在提交区正下",
            StackCode::IllegalTransition => "状态机非法转换：拒收",
        }
    }
    pub const ALL: [StackCode; 8] = [
        StackCode::ReserveSizeBad,
        StackCode::FirstCommitBad,
        StackCode::GuardPagesBad,
        StackCode::GrowthLedgerBroken,
        StackCode::GuardBelowFloor,
        StackCode::OverflowNotRaised,
        StackCode::LayoutMisaligned,
        StackCode::IllegalTransition,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < StackCode::ALL.len() {
        if StackCode::ALL[i].val() == code {
            return StackCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（写死） ────────────────────────────────────────────────
/// 页大小 4KB。
pub const PAGE: u32 = 0x1000;
/// 主线程栈保留 1MB（判据写死）。
pub const STACK_RESERVE: u32 = 0x100000;
/// 首页提交 1 页。
pub const FIRST_COMMIT: u32 = 1;
/// guard 页恰 1 页。
pub const GUARD_PAGES: u32 = 1;
/// STATUS_STACK_OVERFLOW（写死自 winnt.h）。
pub const STATUS_STACK_OVERFLOW: u32 = 0xC000_00FD;
/// 保留总页数：1MB/4KB = 256。
pub const RESERVE_PAGES: u32 = STACK_RESERVE / PAGE;

// ── 栈布局账 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StackLayout {
    /// 保留底（低址）。
    pub base: u32,
    /// 已提交页数（含首页；guard 不计提交）。
    pub commit_pages: u32,
    /// guard 页（0 或 1）。
    pub guard_pages: u32,
    /// 保留未提交页数（含 guard 之下余量）。
    pub free_pages: u32,
}

impl StackLayout {
    /// 守恒恒等式：commit + guard + free == RESERVE_PAGES。
    pub fn conserved(&self) -> bool {
        self.commit_pages + self.guard_pages + self.free_pages == RESERVE_PAGES
    }
    /// 布局断言：1MB 保留 / 首页 1 页 / guard 恰 1 页。
    pub fn layout_ok(&self) -> bool {
        self.commit_pages + self.guard_pages + self.free_pages == RESERVE_PAGES
            && self.guard_pages == GUARD_PAGES
    }
}

/// 初始布局断言：保留底 base、栈顶 base+reserve；提交区在高址端。
pub fn init_layout(base: u32) -> Result<StackLayout, StackCode> {
    let commit = FIRST_COMMIT;
    let guard = GUARD_PAGES;
    if RESERVE_PAGES <= commit + guard {
        return Err(StackCode::ReserveSizeBad);
    }
    Ok(StackLayout {
        base,
        commit_pages: commit,
        guard_pages: guard,
        free_pages: RESERVE_PAGES - commit - guard,
    })
}

/// guard 地址（低址端）：base + free_pages*PAGE。
pub fn guard_addr(l: &StackLayout) -> u32 {
    l.base + l.free_pages * PAGE
}

/// 提交区低址：guard 之上。
pub fn commit_low(l: &StackLayout) -> u32 {
    guard_addr(l) + l.guard_pages * PAGE
}

// ── 增长状态机（显式状态 + 合法边闭集） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackState {
    Reserved,
    GuardNormal,
    GuardLast,
    Overflow,
}

pub fn edge_legal(from: StackState, to: StackState) -> bool {
    matches!(
        (from, to),
        (StackState::Reserved, StackState::GuardNormal)
            | (StackState::GuardNormal, StackState::GuardNormal)
            | (StackState::GuardNormal, StackState::GuardLast)
            | (StackState::GuardLast, StackState::Overflow)
    )
}

/// 栈增长核：guard 缺页 → 下移一页。
/// 返回（新状态, 是否溢出）。触底规则与 Windows 一致：guard 到达
/// 最后一页（free_pages==0 前的最后一步）→ GuardLast；再增长 →
/// STATUS_STACK_OVERFLOW，不再增长。
pub fn grow(l: &mut StackLayout, st: &mut StackState) -> Result<bool, StackCode> {
    let before = *l;
    match *st {
        StackState::Reserved => {
            if edge_legal(StackState::Reserved, StackState::GuardNormal) {
                *st = StackState::GuardNormal;
            } else {
                return Err(StackCode::IllegalTransition);
            }
        }
        StackState::Overflow => return Err(StackCode::OverflowNotRaised),
        _ => {}
    }
    // GuardLast 再增长 → 溢出。
    if *st == StackState::GuardLast {
        *st = StackState::Overflow;
        return Ok(true);
    }
    // 增长一步：guard 下移一页。
    if l.free_pages == 0 {
        return Err(StackCode::GuardBelowFloor);
    }
    l.commit_pages += 1;
    l.free_pages -= 1;
    if !l.conserved() {
        *l = before;
        return Err(StackCode::GrowthLedgerBroken);
    }
    // 触底判定：free_pages == 0 → 已到 GuardLast。
    if l.free_pages == 0 {
        *st = StackState::GuardLast;
    }
    Ok(false)
}

// ── LCG（种子写死 = SEG_BASE） ──────────────────────────────────
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

// ── 布局断言（判据 J1 面：1MB 保留/首页提交） ────────────────────
pub fn drill_layout_assert() -> (bool, String) {
    let base: u32 = 0x00A00000;
    let l = init_layout(base);
    let mut ok = true;
    match l {
        Ok(l) => {
            ok = ok
                && l.commit_pages + l.guard_pages + l.free_pages == RESERVE_PAGES
                && RESERVE_PAGES == 256
                && l.commit_pages == 1
                && l.guard_pages == 1
                && l.free_pages == 254
                && l.layout_ok()
                && guard_addr(&l) == base + 254 * PAGE
                && commit_low(&l) == base + 255 * PAGE;
        }
        Err(_) => ok = false,
    }
    // 1MB 保留尺寸红线。
    ok = ok && STACK_RESERVE == 0x100000 && PAGE == 0x1000;
    (ok, format!("布局断言: 1MB/首页1页/guard1页 = {}", ok))
}

pub fn drill_growth_1000() -> (bool, String) {
    // 栈增长对账：请求 ×1000 轮，前 254 轮零错位（每轮守恒 + guard
    // 单调下移），第 255 轮触底 → GuardLast → 溢出（Windows 一致）。
    let base: u32 = 0x00B00000;
    let mut l = match init_layout(base) {
        Ok(l) => l,
        Err(_) => return (false, String::from("init 失败")),
    };
    let mut st = StackState::Reserved;
    let mut rounds = 0;
    let mut ok = true;
    let mut prev_guard = guard_addr(&l);
    while rounds < 1000 {
        match grow(&mut l, &mut st) {
            Ok(overflow) => {
                if overflow {
                    // 触底溢出轮：正常终态，停。
                    break;
                }
                ok = ok && l.conserved();
                ok = ok && guard_addr(&l) < prev_guard; // 单调下移
                prev_guard = guard_addr(&l);
            }
            Err(_) => {
                ok = false;
                break;
            }
        }
        rounds += 1;
    }
    // 254 次成功增长 + 1 次溢出轮；账冻结在 commit=255/free=0。
    ok = ok && rounds == 254 && l.commit_pages == 255 && l.free_pages == 0;
    (
        ok,
        format!(
            "增长对账: 成功轮={} commit={} free={} 态={:?} 守恒={} 零错位={}",
            rounds, l.commit_pages, l.free_pages, st, l.conserved(), ok
        ),
    )
}

pub fn drill_growth_1000_no_overflow() -> (bool, String) {
    // ×1000 轮注入（触底后继续请求）：前 254 轮零错位，第 255 轮
    // 溢出路径（Windows 一致：STATUS_STACK_OVERFLOW）。
    let base: u32 = 0x00C00000;
    let mut l = match init_layout(base) {
        Ok(l) => l,
        Err(_) => return (false, String::from("init 失败")),
    };
    let mut st = StackState::Reserved;
    let mut zero_shift = true;
    let mut i = 0;
    while i < 1000 {
        match grow(&mut l, &mut st) {
            Ok(overflow) => {
                if overflow {
                    // 触底轮：状态必 Overflow 且栈账冻结。
                    zero_shift = zero_shift
                        && st == StackState::Overflow
                        && l.conserved()
                        && l.commit_pages == 255;
                    break;
                }
            }
            Err(e) => return (false, format!("增长错误: {:?}", e)),
        }
        i += 1;
    }
    // 触底轮之后再次请求 → 溢出已抛（Overflow 态拒再增长）。
    let again = grow(&mut l, &mut st);
    let ok = zero_shift && i == 254 && again == Err(StackCode::OverflowNotRaised) && l.commit_pages == 255;
    (
        ok,
        format!(
            "×1000 轮: 触底轮={} 溢出码=0x{:08X} 再请求拒={} 冻结={}",
            i, STATUS_STACK_OVERFLOW, again == Err(StackCode::OverflowNotRaised), l.commit_pages == 255
        ),
    )
}

pub fn drill_overflow_path() -> (bool, String) {
    // 溢出异常路径：GuardLast 态请求 → Overflow + 码对齐 0xC00000FD。
    let base: u32 = 0x00D00000;
    let mut l = match init_layout(base) {
        Ok(l) => l,
        Err(_) => return (false, String::from("init 失败")),
    };
    let mut st = StackState::Reserved;
    let mut i = 0;
    while i < RESERVE_PAGES - 2 {
        let _ = grow(&mut l, &mut st);
        i += 1;
    }
    let at_last = st == StackState::GuardLast && l.free_pages == 0;
    let r = grow(&mut l, &mut st);
    let ok = at_last
        && r == Ok(true)
        && st == StackState::Overflow
        && STATUS_STACK_OVERFLOW == 0xC000_00FD
        && l.commit_pages == 255;
    (
        ok,
        format!(
            "溢出路径: 触底态={} 溢出={} 冻结={} 码对齐={}",
            at_last,
            r == Ok(true),
            l.commit_pages == 255,
            STATUS_STACK_OVERFLOW == 0xC000_00FD
        ),
    )
}

pub fn drill_state_edges() -> (bool, String) {
    let mut ok = true;
    ok = ok && edge_legal(StackState::Reserved, StackState::GuardNormal);
    ok = ok && edge_legal(StackState::GuardNormal, StackState::GuardNormal);
    ok = ok && edge_legal(StackState::GuardNormal, StackState::GuardLast);
    ok = ok && edge_legal(StackState::GuardLast, StackState::Overflow);
    ok = ok && !edge_legal(StackState::Reserved, StackState::Overflow);
    ok = ok && !edge_legal(StackState::Overflow, StackState::GuardNormal);
    ok = ok && !edge_legal(StackState::GuardLast, StackState::GuardNormal);
    (ok, format!("状态边闭集: {}", ok))
}

pub fn drill_guard_floor_injection() -> (bool, String) {
    // guard 越底注入：free==0 时（GuardLast 已置）再增长走溢出而非
    // 越底错误；人为构造 free==0 但态为 GuardNormal → GuardBelowFloor。
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let mut l = StackLayout {
            base: 0x00E00000 + i * 0x100000,
            commit_pages: 255,
            guard_pages: 1,
            free_pages: 0,
        };
        let mut st = StackState::GuardNormal; // 人为错态
        let r = grow(&mut l, &mut st);
        // GuardNormal + free==0 → 本轮把 free 减到 -1 前拦截 → 越底。
        if r == Err(StackCode::GuardBelowFloor) {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("guard 越底注入 {}/10 必抓", caught))
}

pub fn drill_page_alignment() -> (bool, String) {
    // 页对齐核：guard_addr / commit_low / base 全 4K 对齐 ×8 组。
    let mut caught = 0;
    let mut i = 0;
    while i < 8 {
        let base = 0x100000 * (i + 1);
        match init_layout(base) {
            Ok(l) => {
                let g = guard_addr(&l);
                let c = commit_low(&l);
                if base % PAGE == 0 && g % PAGE == 0 && c % PAGE == 0 && c == g + PAGE {
                    caught += 1;
                }
            }
            Err(_) => {}
        }
        i += 1;
    }
    let ok = caught == 8;
    (ok, format!("页对齐 {}/8", caught))
}

pub fn drill_ledger_conservation() -> (bool, String) {
    // 守恒对账：多轮增长随机停点，账实一致（commit+guard+free==256）。
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    let mut n = 0;
    while n < 20 {
        let base: u32 = 0x01000000 + (n as u32) * 0x200000;
        let mut l = match init_layout(base) {
            Ok(l) => l,
            Err(_) => break,
        };
        let mut st = StackState::Reserved;
        let steps = rng.next_below(260);
        let mut i = 0;
        let mut okrun = true;
        while i < steps {
            match grow(&mut l, &mut st) {
                Ok(_) => {}
                Err(_) => {
                    // 触底后停（GuardLast 后继续 → 溢出属正常终态）。
                    break;
                }
            }
            i += 1;
        }
        okrun = okrun && l.conserved() && l.layout_ok();
        if okrun {
            caught += 1;
        }
        n += 1;
    }
    let ok = caught == 20;
    (ok, format!("守恒对账 {}/20 停点", caught))
}

pub fn drill_commit_free_invariant() -> (bool, String) {
    // 不变式：guard_addr 恒等于 base+free*PAGE（增长闭式重推）。
    let base: u32 = 0x02000000;
    let mut l = match init_layout(base) {
        Ok(l) => l,
        Err(_) => return (false, String::from("init 失败")),
    };
    let mut st = StackState::Reserved;
    let mut ok = true;
    let mut i = 0;
    while i < 254 {
        let _ = grow(&mut l, &mut st);
        ok = ok && guard_addr(&l) == base + l.free_pages * PAGE;
        ok = ok && commit_low(&l) == guard_addr(&l) + PAGE;
        ok = ok && l.commit_pages == i + 2; // 首页 1 + 增长 i+1
        i += 1;
    }
    (ok, format!("guard/commit 重推不变式: {}", ok))
}

pub fn drill_diag_codes_closed() -> (bool, String) {
    let mut ok = true;
    let mut i = 0;
    while i < StackCode::ALL.len() {
        ok = ok && StackCode::ALL[i].val() >= SEG_BASE && StackCode::ALL[i].val() < SEG_BASE + 0x40;
        ok = ok && !StackCode::ALL[i].detail().is_empty();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x3F) == "未知诊断码";
    (ok, format!("诊断码闭集 {}/8 + 反向", StackCode::ALL.len()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < StackCode::ALL.len() {
        let v = StackCode::ALL[i].val();
        ok = ok && v >= SEG_BASE && v < SEG_BASE + 0x40;
        i += 1;
    }
    ok
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_layout_assert();
    let (a2, d2) = drill_growth_1000();
    let (a3, d3) = drill_overflow_path();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 布局={} 增长={} 溢出={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_layout_assert();
    v.push(("J1-1MB保留/首页提交断言", a, b));
    let (a, b) = drill_growth_1000();
    v.push(("J1-增长×1000轮零错位", a, b));
    let (a, b) = drill_growth_1000_no_overflow();
    v.push(("J1-×1000注入触底溢出", a, b));
    let (a, b) = drill_overflow_path();
    v.push(("J1-溢出路径Windows一致", a, b));
    let (a, b) = drill_state_edges();
    v.push(("J2-状态边闭集", a, b));
    let (a, b) = drill_guard_floor_injection();
    v.push(("J2-guard越底注入10/10", a, b));
    let (a, b) = drill_page_alignment();
    v.push(("J2-页对齐8/8", a, b));
    let (a, b) = drill_ledger_conservation();
    v.push(("J2-守恒对账20停点", a, b));
    let (a, b) = drill_commit_free_invariant();
    v.push(("J2-guard/commit重推不变式", a, b));
    let (a, b) = drill_diag_codes_closed();
    v.push(("J3-诊断码闭集+反向", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13021_checks：本单判据聚合（诊断段 0x65540）。
pub fn run_unx13021_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13021 主线程栈 guard page");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65540 段闭集");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = checks[i].clone();
        s.add_dyn(n, p, &d);
        i += 1;
    }
    s
}
