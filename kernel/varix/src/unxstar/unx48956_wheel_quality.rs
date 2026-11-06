//! UNX-F48956 滚轮观测埋点 · 0x71360
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B08）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F48956-J1 四点 100% 覆盖；异常零静默；
//!   ③ UNX-F48957-J1 fuzz ×10^4 零崩；
//!   ④ UNX-F48958-J1 ×20 全绿入库。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：观测点次序 → 时钟单调 → 异常码位置 → 四点覆盖 →
//!           守恒审计 → 用例复跑 → 渲染复读。
//! 先校次序再校异常：四点的顺序 Entry→Exit→Duration→Result 是账本
//! 可读的前提；若顺序错乱，异常码落在哪一点都无从判断。
//! 异常码只允许出现在 Result：入口/出口/耗时出现非零码，说明调用
//! 方把「诊断」塞进了「阶段」，这是静默异常的典型形态。
//! 关键差异：本模块只回答「四点是否全落账、20 例是否可复跑」，不做
//!           错误码族（B08-BRIDGE）、不做段位反重（B08-SEAL）。用例
//!           全部调用前序模块的真实函数，不允许硬编码期望值绕过实现。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;
use super::unx48943_wheel_meta as p_meta;
use super::unx48945_wheel_guard as p_guard;
use super::unx48948_wheel_dispatch as p_dispatch;
use super::unx48951_wheel_edge as p_edge;
use super::unx48953_wheel_bridge as p_bridge;

/// 诊断段基址（0x71360，B08 族第七槽）。
pub const SEG_BASE: u32 = 0x71360;

/// 观测操作数。
pub const INSTR_OPS: usize = 4;
/// 观测点数（入口/出口/耗时/结果）。
pub const POINT_COUNT: usize = 4;
/// 四点覆盖位图满值。
pub const COVER_BITS: u8 = 0x0F;
/// 账本记录总数（四点合计）。
pub const POINT_TOTAL: u32 = 16;
/// fuzz 轮数（卡面判据 ×10^4 的载体）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// fuzz 路数。
pub const FUZZ_ROUTES: usize = 4;
/// 用例条数（卡面判据 ×20）。
pub const CASES: usize = 20;
/// 黄金轨迹轮数。
pub const GOLDEN_ROUNDS: u32 = 250;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0B） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PwCode {
    /// 观测点非法（次序/编号）
    PointBad,
    /// 覆盖不足（四点未全落）
    CoverBad,
    /// 静默异常（非 Result 点出现异常码）
    SilentBad,
    /// fuzz 记账不平
    FuzzBad,
    /// 用例非法（条数/编号/文本）
    CaseBad,
    /// 复算不一致
    ReplayBad,
    /// 渲染非法
    RenderBad,
    /// 次序非法（时钟回退）
    OrderBad,
    /// 账不平（记录/周期/错误计数不守恒）
    LedgerBad,
    /// 段位越界
    SegmentBad,
    /// 语料非法（用例失败或重复）
    CorpusBad,
}

impl PwCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            PwCode::PointBad => "观测点非法：次序或编号不合法",
            PwCode::CoverBad => "覆盖不足：四点未全部落账",
            PwCode::SilentBad => "静默异常：非 Result 点出现异常码",
            PwCode::FuzzBad => "fuzz 记账不平：接受+拒绝≠尝试",
            PwCode::CaseBad => "用例非法：条数、编号或文本不合法",
            PwCode::ReplayBad => "复算不一致：同一输入重放不同",
            PwCode::RenderBad => "渲染非法：字段缺失或回读失败",
            PwCode::OrderBad => "次序非法：时钟回退",
            PwCode::LedgerBad => "账不平：记录/周期/错误计数不守恒",
            PwCode::SegmentBad => "段位越界：诊断码逃出 0x71360 窗口",
            PwCode::CorpusBad => "语料非法：用例失败或编号重复",
        }
    }

    pub const ALL: [PwCode; 11] = [
        PwCode::PointBad,
        PwCode::CoverBad,
        PwCode::SilentBad,
        PwCode::FuzzBad,
        PwCode::CaseBad,
        PwCode::ReplayBad,
        PwCode::RenderBad,
        PwCode::OrderBad,
        PwCode::LedgerBad,
        PwCode::SegmentBad,
        PwCode::CorpusBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < PwCode::ALL.len() {
        if PwCode::ALL[i].val() == code {
            return PwCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 观测点与操作 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    Accumulate,
    Upsample,
    Downsample,
    Route,
}

impl OpKind {
    pub fn code(self) -> u32 {
        match self {
            OpKind::Accumulate => SEG_BASE + 0x20,
            OpKind::Upsample => SEG_BASE + 0x21,
            OpKind::Downsample => SEG_BASE + 0x22,
            OpKind::Route => SEG_BASE + 0x23,
        }
    }

    pub fn index(self) -> u8 {
        match self {
            OpKind::Accumulate => 0,
            OpKind::Upsample => 1,
            OpKind::Downsample => 2,
            OpKind::Route => 3,
        }
    }

    pub fn from_index(i: u8) -> Result<OpKind, u32> {
        match i {
            0 => Ok(OpKind::Accumulate),
            1 => Ok(OpKind::Upsample),
            2 => Ok(OpKind::Downsample),
            3 => Ok(OpKind::Route),
            _ => Err(PwCode::PointBad.val()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointKind {
    Entry,
    Exit,
    Duration,
    Result,
}

impl PointKind {
    pub fn bit(self) -> u8 {
        1u8 << self.index()
    }

    pub fn index(self) -> u8 {
        match self {
            PointKind::Entry => 0,
            PointKind::Exit => 1,
            PointKind::Duration => 2,
            PointKind::Result => 3,
        }
    }

    pub fn from_index(i: u8) -> Result<PointKind, u32> {
        match i {
            0 => Ok(PointKind::Entry),
            1 => Ok(PointKind::Exit),
            2 => Ok(PointKind::Duration),
            3 => Ok(PointKind::Result),
            _ => Err(PwCode::PointBad.val()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceRecord {
    pub op: OpKind,
    pub point: PointKind,
    pub tick: u64,
    pub code: u32,
    pub value: i32,
}

impl TraceRecord {
    pub fn new(op: OpKind, point: PointKind, tick: u64, code: u32, value: i32) -> TraceRecord {
        TraceRecord {
            op,
            point,
            tick,
            code,
            value,
        }
    }
}

/// 四点账本：每次操作必须完整走完四点，异常码只允许落在 Result。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceBook {
    pub records: u32,
    pub cycles: u32,
    pub errors: u32,
    pub rejects: u32,
    pub digest: u64,
    pub masks: [u8; INSTR_OPS],
    pub counts: [u32; INSTR_OPS],
    pub phase: u8,
    pub last_tick: u64,
}

impl TraceBook {
    pub fn new() -> TraceBook {
        TraceBook {
            records: 0,
            cycles: 0,
            errors: 0,
            rejects: 0,
            digest: p_core::FNV_OFF,
            masks: [0; INSTR_OPS],
            counts: [0; INSTR_OPS],
            phase: 0,
            last_tick: 0,
        }
    }

    pub fn push(&mut self, rec: TraceRecord) -> Result<(), u32> {
        if rec.tick < self.last_tick {
            return self.fail(PwCode::OrderBad.val());
        }
        if rec.point.index() != self.phase {
            return self.fail(PwCode::PointBad.val());
        }
        if rec.point != PointKind::Result && rec.code != 0 {
            return self.fail(PwCode::SilentBad.val());
        }
        let op = rec.op.index() as usize;
        match self.masks.get_mut(op) {
            Some(m) => *m |= rec.point.bit(),
            None => return self.fail(PwCode::PointBad.val()),
        }
        if let Some(c) = self.counts.get_mut(op) {
            *c = c.saturating_add(1);
        }
        if rec.point == PointKind::Result {
            self.cycles = self.cycles.saturating_add(1);
            if rec.code != 0 {
                self.errors = self.errors.saturating_add(1);
            }
        }
        self.phase = ((self.phase as usize + 1) % POINT_COUNT) as u8;
        self.last_tick = rec.tick;
        self.records = self.records.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, rec.op.index() as u32);
        self.digest = p_core::fold_u32(self.digest, rec.point.index() as u32);
        self.digest = p_core::fold_u32(self.digest, rec.tick as u32);
        self.digest = p_core::fold_u32(self.digest, rec.code);
        self.digest = p_core::fold_i32(self.digest, rec.value);
        Ok(())
    }

    fn fail(&mut self, code: u32) -> Result<(), u32> {
        self.rejects = self.rejects.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, code);
        Err(code)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.phase != 0 {
            return Err(PwCode::PointBad.val());
        }
        if self.records == 0 || self.records % POINT_COUNT as u32 != 0 {
            return Err(PwCode::PointBad.val());
        }
        if self.cycles.saturating_mul(POINT_COUNT as u32) != self.records {
            return Err(PwCode::LedgerBad.val());
        }
        if self.errors > self.cycles {
            return Err(PwCode::LedgerBad.val());
        }
        let mut i = 0usize;
        let mut sum = 0u32;
        while i < INSTR_OPS {
            let c = *self.counts.get(i).unwrap_or(&0);
            if c % POINT_COUNT as u32 != 0 {
                return Err(PwCode::LedgerBad.val());
            }
            sum = sum.saturating_add(c);
            i += 1;
        }
        if sum != self.records {
            return Err(PwCode::LedgerBad.val());
        }
        Ok(())
    }

    /// 四点覆盖：每个操作的四个点都必须出现过，合计 16 位。
    pub fn audit_cover(&self) -> Result<(), u32> {
        let mut i = 0usize;
        let mut points = 0u32;
        while i < INSTR_OPS {
            if *self.masks.get(i).unwrap_or(&0) != COVER_BITS {
                return Err(PwCode::CoverBad.val());
            }
            points = points.saturating_add(POINT_COUNT as u32);
            i += 1;
        }
        if points != POINT_TOTAL {
            return Err(PwCode::CoverBad.val());
        }
        Ok(())
    }

    pub fn points_covered(&self) -> u32 {
        let mut i = 0usize;
        let mut sum = 0u32;
        while i < INSTR_OPS {
            let m = *self.masks.get(i).unwrap_or(&0);
            sum = sum.saturating_add(m.count_ones());
            i += 1;
        }
        sum
    }
}

/// 黄金轨迹：每个操作完整四点，异常落在每第 8 轮。
pub fn golden_traces(rounds: u32) -> Result<TraceBook, u32> {
    if rounds == 0 {
        return Err(PwCode::CoverBad.val());
    }
    let mut book = TraceBook::new();
    let mut r = 0u32;
    while r < rounds {
        let mut oi = 0u8;
        while oi < INSTR_OPS as u8 {
            let op = OpKind::from_index(oi)?;
            let base = (r as u64).saturating_mul(16).saturating_add((oi as u64) * 4);
            let input = sample_input(r, oi);
            let emit = match op {
                OpKind::Accumulate => i32::from(input > 0) - i32::from(input < 0),
                OpKind::Upsample => input,
                OpKind::Downsample => input / 120,
                OpKind::Route => input % 7,
            };
            let code = if r % 8 == 3 && oi as u32 <= r % 8 {
                op.code()
            } else {
                0
            };
            book.push(TraceRecord::new(op, PointKind::Entry, base, 0, input))?;
            book.push(TraceRecord::new(op, PointKind::Exit, base + 1, 0, emit))?;
            book.push(TraceRecord::new(op, PointKind::Duration, base + 2, 0, 1 + (oi as i32)))?;
            book.push(TraceRecord::new(op, PointKind::Result, base + 3, code, emit))?;
            oi += 1;
        }
        r = r.saturating_add(1);
    }
    book.audit()?;
    book.audit_cover()?;
    if book.errors == 0 {
        return Err(PwCode::SilentBad.val());
    }
    if book.points_covered() != POINT_TOTAL {
        return Err(PwCode::CoverBad.val());
    }
    Ok(book)
}

fn sample_input(r: u32, oi: u8) -> i32 {
    let base = match (r + oi as u32) % 6 {
        0 => 120,
        1 => -120,
        2 => 1,
        3 => -1,
        4 => 119,
        _ => -119,
    };
    if oi % 2 == 0 {
        base
    } else {
        -base
    }
}

// ── fuzz 面 ───────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzStats {
    pub attempts: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub faults: u32,
    pub digest: u64,
    pub per_route: [[u32; 3]; FUZZ_ROUTES],
}

impl FuzzStats {
    pub fn new() -> FuzzStats {
        FuzzStats {
            attempts: 0,
            accepted: 0,
            rejected: 0,
            faults: 0,
            digest: p_core::FNV_OFF,
            per_route: [[0; 3]; FUZZ_ROUTES],
        }
    }

    /// 记账：`expected` 判处置是否符合预期（不符记 fault），`inject_bad`
    /// 区分样本类别。两者分开记，`route_cover` 的「正反样本均非零」才有意义。
    fn tally(&mut self, route: usize, expected: bool, inject_bad: bool) {
        if route >= FUZZ_ROUTES {
            self.faults = self.faults.saturating_add(1);
            return;
        }
        if let Some(row) = self.per_route.get_mut(route) {
            if let Some(a) = row.get_mut(0) {
                *a = a.saturating_add(1);
            }
            let idx = if inject_bad { 2 } else { 1 };
            if let Some(c) = row.get_mut(idx) {
                *c = c.saturating_add(1);
            }
        }
        self.attempts = self.attempts.saturating_add(1);
        if inject_bad {
            self.rejected = self.rejected.saturating_add(1);
        } else {
            self.accepted = self.accepted.saturating_add(1);
        }
        if !expected {
            self.faults = self.faults.saturating_add(1);
        }
        self.digest = p_core::fold_u32(self.digest, route as u32);
        self.digest = p_core::fold_u32(self.digest, expected as u32);
        self.digest = p_core::fold_u32(self.digest, inject_bad as u32);
    }

    pub fn balanced(&self) -> bool {
        self.attempts == self.accepted.saturating_add(self.rejected) && self.faults == 0
    }

    pub fn route_cover(&self) -> bool {
        let mut i = 0usize;
        while i < FUZZ_ROUTES {
            match self.per_route.get(i) {
                Some(row) => {
                    if row[0] == 0 || row[1] == 0 || row[2] == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 四路 fuzz：累积 / 细格 / 降采样 / 路由，每 9 次注入一次非法。
pub fn fuzz_face(rounds: u32) -> Result<FuzzStats, u32> {
    if rounds == 0 {
        return Err(PwCode::CoverBad.val());
    }
    let mut stats = FuzzStats::new();
    let mut rng = p_core::Lcg::new(0x8A11_0056);
    let mut i = 0u32;
    while i < rounds {
        // 路线取高位（LCG 低位周期短，`below(4)` 会锁死在单路，见 bridge 同注）。
        let route = ((rng.next() >> 33) % FUZZ_ROUTES as u64) as usize;
        let bad = i % 9 == 0;
        let ok = match route {
            0 => {
                let d = if bad {
                    0
                } else {
                    let span = rng.below(20_000);
                    rng.signed(1 + span)
                };
                let mut a = p_core::WheelAccumulator::vertical();
                let res = a.push(d);
                if bad { res.is_err() } else { res.is_ok() }
            }
            1 => {
                let d = if bad {
                    p_meta::FINE_MAX + 1
                } else {
                    let span = rng.below(5_000);
                    rng.signed(1 + span)
                };
                let mut s = p_meta::HiResStream::vertical();
                let res = s.push_fine(d);
                if bad { res.is_err() } else { res.is_ok() }
            }
            2 => {
                let mut s = p_guard::LegacySynth::new();
                let d = if bad { 0 } else { rng.signed(240) };
                let res = s.push_fine(d);
                if bad { res.is_err() } else { res.is_ok() }
            }
            _ => {
                let mut r = p_guard::WheelRouter::new();
                if bad {
                    r.set_hover(Some(0)).is_err()
                } else {
                    r.set_focus(Some(1 + rng.below(999_999))).is_ok()
                }
            }
        };
        stats.tally(route, ok, bad);
        i = i.saturating_add(1);
    }
    if !stats.balanced() || !stats.route_cover() {
        return Err(PwCode::FuzzBad.val());
    }
    Ok(stats)
}

// ── 20 例语料 ─────────────────────────────────────────────────────
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaseRow {
    pub id: String,
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

fn case_01() -> (bool, String) {
    (p_core::WHEEL_DELTA == 120, format!("delta={}", p_core::WHEEL_DELTA))
}

fn case_02() -> (bool, String) {
    match p_core::wheel_units(121) {
        Ok((n, r)) => (n == 1 && r == 1, format!("121->({},{})", n, r)),
        Err(e) => (false, format!("err=0x{:X}", e)),
    }
}

fn case_03() -> (bool, String) {
    match p_core::wheel_units(-121) {
        Ok((n, r)) => (n == -1 && r == -1, format!("-121->({},{})", n, r)),
        Err(e) => (false, format!("err=0x{:X}", e)),
    }
}

fn case_04() -> (bool, String) {
    let mut a = p_core::WheelAccumulator::vertical();
    let x = a.push(119);
    let y = a.push(1);
    (
        x == Ok(0) && y == Ok(1) && a.audit().is_ok() && a.remainder == 0,
        format!("pushes=({:?},{:?}) rem={}", x, y, a.remainder),
    )
}

fn case_05() -> (bool, String) {
    let mut a = p_core::WheelAccumulator::vertical();
    let x = a.push(-119);
    let y = a.push(-1);
    (
        x == Ok(0) && y == Ok(-1) && a.audit().is_ok() && a.remainder == 0,
        format!("pushes=({:?},{:?}) rem={}", x, y, a.remainder),
    )
}

fn case_06() -> (bool, String) {
    match p_core::golden_vertical(10) {
        Ok(a) => (
            a.total == -1_210 && a.notches == -10 && a.remainder == -10,
            format!("total={} notches={}", a.total, a.notches),
        ),
        Err(e) => (false, format!("err=0x{:X}", e)),
    }
}

fn case_07() -> (bool, String) {
    match p_meta::fine_units(120) {
        Ok((n, r)) => (n == 1 && r == 0, format!("120->({},{})", n, r)),
        Err(e) => (false, format!("err=0x{:X}", e)),
    }
}

fn case_08() -> (bool, String) {
    let mut s = p_meta::HiResStream::vertical();
    let a = s.push_fine(60);
    let b = s.push_fine(60);
    (
        a == Ok(0) && b == Ok(1) && s.audit().is_ok(),
        format!("pushes=({:?},{:?}) notches={}", a, b, s.notches),
    )
}

fn case_09() -> (bool, String) {
    match p_meta::fine_to_whole(240) {
        Ok(n) => (n == 2, format!("240->{}", n)),
        Err(e) => (false, format!("err=0x{:X}", e)),
    }
}

fn case_10() -> (bool, String) {
    match p_meta::horizontal_rounds(10) {
        // 8 步一循环净零，10 步再走两步（+120/−120）仍为零：两轴必须
        // 逐字段一致且净账归零——首版断言「非零」与样本构造自相矛盾。
        Ok((v, h)) => (v == h && v == 0, format!("v={} h={}", v, h)),
        Err(e) => (false, format!("err=0x{:X}", e)),
    }
}

fn case_11() -> (bool, String) {
    let mut s = p_guard::LegacySynth::new();
    let a = s.push_fine(119);
    let b = s.push_fine(1);
    (
        a == Ok(0) && b == Ok(1) && s.audit().is_ok(),
        format!("pushes=({:?},{:?}) emitted={}", a, b, s.emitted),
    )
}

fn case_12() -> (bool, String) {
    match p_guard::reconcile_stream(200) {
        Ok((f, e, r)) => (f == 0 && e == 0 && r == 0, format!("({},{},{})", f, e, r)),
        Err(e) => (false, format!("err=0x{:X}", e)),
    }
}

fn case_13() -> (bool, String) {
    let mut on = p_guard::DirectionPolicy::new(true);
    let mut off = p_guard::DirectionPolicy::new(false);
    let a = on.apply(120);
    let b = off.apply(120);
    (a == Ok(-120) && b == Ok(120), format!("on={:?} off={:?}", a, b))
}

fn case_14() -> (bool, String) {
    let mut r = p_guard::WheelRouter::new();
    let _ = r.set_hover(Some(5));
    let _ = r.set_focus(Some(9));
    match r.deliver() {
        Ok(t) => (t == p_guard::RouteTarget::Hover(5), format!("target={:?}", t)),
        Err(e) => (false, format!("err=0x{:X}", e)),
    }
}

fn case_15() -> (bool, String) {
    let mut r = p_guard::WheelRouter::new();
    let _ = r.set_focus(Some(9));
    match r.deliver() {
        Ok(t) => (t == p_guard::RouteTarget::Focus(9), format!("target={:?}", t)),
        Err(e) => (false, format!("err=0x{:X}", e)),
    }
}

fn case_16() -> (bool, String) {
    let mut a = p_dispatch::AxisLock::new();
    let _ = a.begin();
    let fed = a.feed(40, 3);
    (
        fed == Ok((40, 0)) && a.state == p_dispatch::LockState::LockedX,
        format!("feed={:?} state={:?}", fed, a.state),
    )
}

fn case_17() -> (bool, String) {
    match p_dispatch::stamp_rounds(10) {
        Ok(b) => (
            b.rows == 40 && b.counts == [10, 10, 10, 10],
            format!("rows={} entry={}", b.rows, b.counts[0]),
        ),
        Err(e) => (false, format!("err=0x{:X}", e)),
    }
}

fn case_18() -> (bool, String) {
    let r = p_edge::SmoothRegistry::new();
    (r.audit().is_ok() && r.steps == 0, r.text())
}

fn case_19() -> (bool, String) {
    let mut l = p_edge::AnomalyLedger::new();
    let a = l.admit(-120);
    (
        a == Ok(p_edge::DeltaKind::Backward) && l.rejects == 0,
        format!("admit={:?} rejects={}", a, l.rejects),
    )
}

fn case_20() -> (bool, String) {
    match p_bridge::roll_rounds() {
        Ok(n) => (n == p_bridge::ROLLS as u32, format!("rolls={}", n)),
        Err(e) => (false, format!("err=0x{:X}", e)),
    }
}

pub fn case_rows() -> Vec<CaseRow> {
    let table: [(&'static str, fn() -> (bool, String)); CASES] = [
        ("WHEEL_DELTA基准", case_01),
        ("正整格拆分", case_02),
        ("负整格拆分", case_03),
        ("正余量补齐", case_04),
        ("负余量补齐", case_05),
        ("黄金流总账", case_06),
        ("细格换算", case_07),
        ("细格两段补整", case_08),
        ("细格折整", case_09),
        ("横向轴一致", case_10),
        ("降采样补整", case_11),
        ("合成流对账中性", case_12),
        ("自然滚动镜像", case_13),
        ("hover优先", case_14),
        ("焦点回落", case_15),
        ("水平轴锁定", case_16),
        ("四段打点", case_17),
        ("平滑登记零实装", case_18),
        ("负值非异常", case_19),
        ("五路联轧", case_20),
    ];
    let mut rows = Vec::new();
    let mut i = 0usize;
    while i < table.len() {
        if let Some((name, f)) = table.get(i) {
            let (passed, detail) = f();
            rows.push(CaseRow {
                id: format!("F48958-{:02}", i + 1),
                name,
                passed,
                detail,
            });
        }
        i += 1;
    }
    rows
}

pub fn audit_cases(rows: &[CaseRow]) -> Result<u32, u32> {
    if rows.len() != CASES {
        return Err(PwCode::CaseBad.val());
    }
    let mut i = 0usize;
    let mut prev = String::new();
    while i < CASES {
        let row = match rows.get(i) {
            Some(v) => v,
            None => return Err(PwCode::CaseBad.val()),
        };
        if !row.passed {
            return Err(PwCode::CorpusBad.val());
        }
        if row.id.is_empty() || row.name.is_empty() || row.detail.is_empty() {
            return Err(PwCode::CaseBad.val());
        }
        if row.detail.contains("未知") {
            return Err(PwCode::CaseBad.val());
        }
        if !row.id.starts_with("F48958-") {
            return Err(PwCode::CaseBad.val());
        }
        if i > 0 && row.id <= prev {
            return Err(PwCode::CaseBad.val());
        }
        prev = row.id.clone();
        i += 1;
    }
    Ok(CASES as u32)
}

pub fn case_digest(rows: &[CaseRow]) -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            let mut k = 0usize;
            while k < r.id.len() {
                if let Some(b) = r.id.as_bytes().get(k) {
                    h = p_core::fold_byte(h, *b);
                }
                k += 1;
            }
            h = p_core::fold_u32(h, r.passed as u32);
            h = p_core::fold_u32(h, r.detail.len() as u32);
        }
        i += 1;
    }
    h
}

// ── 账本渲染 ──────────────────────────────────────────────────────
pub fn render_traces(b: &TraceBook, rows: &[CaseRow]) -> String {
    let mut cases = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            cases = p_core::fold_u32(cases, r.passed as u32);
        }
        i += 1;
    }
    format!(
        "{}|{}|{}|{}|0x{:016X}",
        b.records,
        b.cycles,
        b.points_covered(),
        b.errors,
        b.digest ^ cases
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(PwCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(PwCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(PwCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(PwCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(PwCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(PwCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(PwCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(PwCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 5 {
        return Err(PwCode::RenderBad.val());
    }
    let records = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(PwCode::RenderBad.val()),
    })?;
    let cycles = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(PwCode::RenderBad.val()),
    })?;
    let points = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(PwCode::RenderBad.val()),
    })?;
    let errors = parse_u32(match parts.get(3) {
        Some(v) => v,
        None => return Err(PwCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(4) {
        Some(v) => v,
        None => return Err(PwCode::RenderBad.val()),
    })?;
    if records == 0 || records > 10_000_000 {
        return Err(PwCode::RenderBad.val());
    }
    if records != cycles.saturating_mul(POINT_COUNT as u32) {
        return Err(PwCode::LedgerBad.val());
    }
    if points != POINT_TOTAL {
        return Err(PwCode::CoverBad.val());
    }
    if errors > cycles {
        return Err(PwCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(PwCode::RenderBad.val());
    }
    Ok(records)
}

pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let book = golden_traces(GOLDEN_ROUNDS)?;
    let rows = case_rows();
    audit_cases(&rows)?;
    let text = render_traces(&book, &rows);
    if audit_render(&text)? != book.records {
        return Err(PwCode::ReplayBad.val());
    }
    let fuzz = fuzz_face(2_000)?;
    Ok((book.records, rows.len() as u32, fuzz.rejected, book.digest))
}

// ── 注入对账（四路缺陷必须被捕获） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 {
        return Err(PwCode::CoverBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 三点残局必须炸 PointBad（缺 Result）。
        let mut b = TraceBook::new();
        let _ = b.push(TraceRecord::new(OpKind::Accumulate, PointKind::Entry, 0, 0, 1));
        let _ = b.push(TraceRecord::new(OpKind::Accumulate, PointKind::Exit, 1, 0, 1));
        let _ = b.push(TraceRecord::new(OpKind::Accumulate, PointKind::Duration, 2, 0, 1));
        if b.audit().err() == Some(PwCode::PointBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 入口带异常码必须炸 SilentBad。
        let mut s = TraceBook::new();
        if s.push(TraceRecord::new(OpKind::Route, PointKind::Entry, 0, 7, 0))
            .err()
            == Some(PwCode::SilentBad.val())
        {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 翻转用例必须炸 CorpusBad。
        let mut rows = case_rows();
        if let Some(r) = rows.get_mut(0) {
            r.passed = false;
        }
        if audit_cases(&rows).err() == Some(PwCode::CorpusBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ fuzz 账被破坏必须炸 FuzzBad。
        let mut f = FuzzStats::new();
        f.tally(0, true, false);
        f.accepted = f.accepted.saturating_add(1);
        if !f.balanced() {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(PwCode::CoverBad.val());
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let b = golden_traces(50)?;
    if b.records != 50 * 16 || b.errors == 0 {
        return Err(PwCode::LedgerBad.val());
    }
    if b.points_covered() != POINT_TOTAL {
        return Err(PwCode::CoverBad.val());
    }
    // 首条记录必须是 Entry：从 Exit 起手即跳过入口打点，判 PointBad。
    let mut bad = TraceBook::new();
    if bad.push(TraceRecord::new(OpKind::Accumulate, PointKind::Exit, 0, 0, 1))
        != Err(PwCode::PointBad.val())
    {
        return Err(PwCode::PointBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let f = fuzz_face(2_000)?;
    if !f.balanced() || !f.route_cover() {
        return Err(PwCode::FuzzBad.val());
    }
    let mut i = 0usize;
    while i < FUZZ_ROUTES {
        match f.per_route.get(i) {
            Some(row) => {
                if row[1] == 0 || row[2] == 0 {
                    return Err(PwCode::FuzzBad.val());
                }
            }
            None => return Err(PwCode::FuzzBad.val()),
        }
        i += 1;
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let a = case_rows();
    let b = case_rows();
    if case_digest(&a) != case_digest(&b) {
        return Err(PwCode::ReplayBad.val());
    }
    audit_cases(&a)?;
    if !segment_guard() {
        return Err(PwCode::SegmentBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_traces(40)?;
    let b = golden_traces(40)?;
    if a.digest != b.digest || a.records != b.records {
        return Err(PwCode::ReplayBad.val());
    }
    if case_digest(&case_rows()) != case_digest(&case_rows()) {
        return Err(PwCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < PwCode::ALL.len() {
        let v = PwCode::ALL[i].val();
        if v <= SEG_BASE || v > SEG_BASE + 0x3F {
            return false;
        }
        i += 1;
    }
    true
}

// ── 域检查项 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    let book = golden_traces(8);
    // 覆盖与异常记账合并：后者依赖前者同一本账，分开报只会让同一根因占两槽。
    out.push((
        "F48956 四点覆盖与异常记账",
        matches!(&book, Ok(b) if b.points_covered() == POINT_TOTAL
            && b.errors > 0 && b.audit().is_ok() && b.rejects == 0),
        match &book {
            Ok(b) => format!(
                "points={} errors={} cycles={} rejects={}",
                b.points_covered(), b.errors, b.cycles, b.rejects
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut order = TraceBook::new();
    let _ = order.push(TraceRecord::new(OpKind::Route, PointKind::Entry, 10, 0, 0));
    let back = order.push(TraceRecord::new(OpKind::Route, PointKind::Exit, 9, 0, 0));
    out.push((
        "F48956 时钟单调",
        back == Err(PwCode::OrderBad.val()),
        format!("push={:?}", back),
    ));

    out.push((
        "F48956 段位闭集",
        segment_guard(),
        format!("codes={} seg=0x{:04X}", PwCode::ALL.len(), SEG_BASE),
    ));

    let fuzz = fuzz_face(FUZZ_ROUNDS);
    // 零崩与四路正反合并：四路覆盖本就在同一 fuzz 账上复算。
    out.push((
        "F48957 fuzz 零崩与四路正反",
        matches!(&fuzz, Ok(s) if s.balanced() && s.route_cover()),
        match &fuzz {
            Ok(s) => format!(
                "attempts={} ok={} bad={}",
                s.attempts, s.accepted, s.rejected
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let rows = case_rows();
    let cases = audit_cases(&rows);
    out.push((
        "F48958 20例全绿",
        cases == Ok(CASES as u32),
        match &cases {
            Ok(n) => format!("cases={}", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut ids_ok = true;
    let mut k = 0usize;
    while k < rows.len() {
        if let Some(r) = rows.get(k) {
            if !r.id.starts_with("F48958-") {
                ids_ok = false;
            }
        }
        k += 1;
    }
    // 复跑一致与编号规整合并：两者都是用例表自身的完整性。
    out.push((
        "F48958 用例复跑与编号",
        case_digest(&rows) == case_digest(&case_rows())
            && ids_ok
            && rows.len() == CASES,
        format!(
            "digest=0x{:016X} rows={} ids_ok={}",
            case_digest(&rows),
            rows.len(),
            ids_ok
        ),
    ));

    let golden = golden_run();
    out.push((
        "F48958 账本守恒",
        matches!(&golden, Ok((r, c, x, _)) if *r == GOLDEN_ROUNDS * 16 && *c == CASES as u32 && *x > 0),
        match &golden {
            Ok((r, c, x, d)) => format!("records={} cases={} rejects={} d=0x{:X}", r, c, x, d),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out
}

pub fn run_unx48956_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48956 滚轮观测埋点与用例集");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_book_requires_full_cycle() {
        let mut b = TraceBook::new();
        let _ = b.push(TraceRecord::new(OpKind::Accumulate, PointKind::Entry, 0, 0, 1));
        assert_eq!(b.audit(), Err(PwCode::PointBad.val()));
    }

    #[test]
    fn trace_book_rejects_silent_code() {
        let mut b = TraceBook::new();
        let e = b.push(TraceRecord::new(OpKind::Route, PointKind::Entry, 0, 5, 0));
        assert_eq!(e, Err(PwCode::SilentBad.val()));
        assert_eq!(b.rejects, 1);
    }

    #[test]
    fn trace_book_rejects_clock_backwards() {
        let mut b = TraceBook::new();
        let _ = b.push(TraceRecord::new(OpKind::Route, PointKind::Entry, 10, 0, 0));
        let e = b.push(TraceRecord::new(OpKind::Route, PointKind::Exit, 9, 0, 0));
        assert_eq!(e, Err(PwCode::OrderBad.val()));
    }

    #[test]
    fn trace_book_rejects_wrong_point_order() {
        let mut b = TraceBook::new();
        let e = b.push(TraceRecord::new(OpKind::Route, PointKind::Duration, 0, 0, 0));
        assert_eq!(e, Err(PwCode::PointBad.val()));
    }

    #[test]
    fn golden_traces_cover_sixteen_points() {
        match golden_traces(8) {
            Ok(b) => {
                assert_eq!(b.points_covered(), POINT_TOTAL);
                assert_eq!(b.records, 8 * 16);
                assert_eq!(b.cycles, 8 * 4);
                assert!(b.errors > 0);
                assert!(b.audit().is_ok());
                assert!(b.audit_cover().is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn golden_traces_reject_zero_rounds() {
        assert_eq!(golden_traces(0), Err(PwCode::CoverBad.val()));
    }

    #[test]
    fn trace_render_roundtrip_and_tamper() {
        let b = match golden_traces(8) {
            Ok(v) => v,
            Err(_) => {
                assert!(false);
                return;
            }
        };
        let rows = case_rows();
        let text = render_traces(&b, &rows);
        assert_eq!(audit_render(&text), Ok(b.records));
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
    }

    #[test]
    fn trace_render_rejects_missing_points() {
        let b = TraceBook::new();
        let text = render_traces(&b, &[]);
        assert!(audit_render(&text).is_err());
    }

    #[test]
    fn fuzz_face_balances() {
        match fuzz_face(1_000) {
            Ok(s) => {
                assert!(s.balanced());
                assert!(s.route_cover());
                assert_eq!(s.attempts, 1_000);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_face_is_deterministic() {
        match (fuzz_face(400), fuzz_face(400)) {
            (Ok(a), Ok(b)) => {
                assert_eq!(a.digest, b.digest);
                assert_eq!(a.per_route, b.per_route);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn cases_are_twenty_and_green() {
        let rows = case_rows();
        assert_eq!(rows.len(), CASES);
        assert_eq!(audit_cases(&rows), Ok(CASES as u32));
        let mut i = 0usize;
        while i < rows.len() {
            match rows.get(i) {
                Some(r) => {
                    assert!(r.passed, "{} {}", r.id, r.detail);
                    assert!(!r.detail.is_empty());
                }
                None => assert!(false),
            }
            i += 1;
        }
    }

    #[test]
    fn cases_are_stable() {
        assert_eq!(case_digest(&case_rows()), case_digest(&case_rows()));
    }

    #[test]
    fn case_audit_catches_flip() {
        let mut rows = case_rows();
        if let Some(r) = rows.get_mut(3) {
            r.passed = false;
        }
        assert_eq!(audit_cases(&rows), Err(PwCode::CorpusBad.val()));
    }

    #[test]
    fn case_audit_catches_short_list() {
        let mut rows = case_rows();
        rows.pop();
        assert_eq!(audit_cases(&rows), Err(PwCode::CaseBad.val()));
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => assert!(l.all_caught()),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_is_deterministic() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48956_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
