//! UNX-F48976 高频观测埋点（入口/出口/耗时/结果四点全覆盖）· 0x715C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B09）：
//!   F48976 ① 纯功能代码 ≥220 行；② UNX-F48976-J1 四点 100% 覆盖；
//!          异常零静默。
//!   F48977 ① 纯功能代码 ≥260 行；② UNX-F48977-J1 fuzz ×10^4 零崩。
//!   F48978 ① 纯功能代码 ≥300 行；② UNX-F48978-J1 ×20 全绿入库。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：事件身份 → 段序 → 时钟 → 容量 → 入账。
//! 先认事件身份再认段序：同一 tick 可能先后出现两个事件，若只按段序判，
//! 第二个事件的 Entry 会被当成第一个事件的重复段而拒绝，四点覆盖随之漏账。
//! 时钟放在段序之后：段序错本身就是更具体的诊断，先报时钟会把「少一段」
//! 误诊成「时钟回退」，排障方向被带偏。
//! 关键差异：本模块只回答「四点是否 100% 覆盖、异常是否显式记账、
//!           20 例判据是否可复跑」，不做乱事件洪泛（B09-FUZZ）、不做
//!           批递交矩阵（B09-BACKPRESSURE）。异常被静默吞掉时 errors
//!           不会增长，本模块的全部判据同时失效。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x715C0，VX6 批 B09 族埋点/用例槽）。
pub const SEG_BASE: u32 = 0x715C0;

/// 四点：入口 / 编码 / 递交 / 提交。
pub const POINT_TOTAL: usize = 4;
/// 四点全掩码。
pub const POINT_MASK: u8 = 0x0F;
/// 用例集规模（卡面 20 例）。
pub const CASES: usize = 20;
/// fuzz 路数（四路引擎）。
pub const FUZZ_ROUTES: usize = 4;
/// fuzz 轮数（卡面 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 埋点账行数上限（内核无堆增长失控防线）。
pub const TRACE_CAP: usize = 8_192;
/// 黄金轮数。
pub const GOLDEN_ROUNDS: u32 = 32;

/// FNV-1a 64 初始值（与 p_core 同参）。
pub const FNV_OFF: u64 = p_core::FNV_OFF;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PqCode {
    /// 段号越界（非四点之一）
    StageBad,
    /// 缺入口（新事件首段不是 Entry）
    PointBad,
    /// 段序断裂（跳段/重段）
    OrderBad,
    /// 时钟回退
    TickBad,
    /// 事件身份非法（重复入口或倒退）
    EventBad,
    /// 账本不平（行数/计数/异常数对不上）
    LedgerBad,
    /// 渲染非法（字段缺失或回读失败）
    RenderBad,
    /// 复算不一致（同流两次结果不同）
    ReplayBad,
    /// 覆盖不足（点数/轮数/用例数不足）
    CoverBad,
    /// 段位越界（诊断码逃出 0x715C0 窗口）
    SegmentBad,
    /// fuzz 处置错配（合法被拒/非法被接）
    FuzzBad,
    /// 用例集红（任一条判据失败）
    CorpusBad,
}

impl PqCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            PqCode::StageBad => "段号越界：非入口/编码/递交/提交之一",
            PqCode::PointBad => "缺入口：新事件首段必须是 Entry",
            PqCode::OrderBad => "段序断裂：跳段或重复段",
            PqCode::TickBad => "时钟回退：事件时间不得逆序",
            PqCode::EventBad => "事件身份非法：重复入口或事件号倒退",
            PqCode::LedgerBad => "账本不平：行数/计数/异常数对不上",
            PqCode::RenderBad => "渲染非法：字段缺失或回读失败",
            PqCode::ReplayBad => "复算不一致：同流两次结果不同",
            PqCode::CoverBad => "覆盖不足：点数/轮数/用例数不足",
            PqCode::SegmentBad => "段位越界：诊断码逃出 0x715C0 窗口",
            PqCode::FuzzBad => "fuzz 处置错配：合法被拒或非法被接",
            PqCode::CorpusBad => "用例集红：存在未通过判据",
        }
    }

    pub const ALL: [PqCode; 12] = [
        PqCode::StageBad,
        PqCode::PointBad,
        PqCode::OrderBad,
        PqCode::TickBad,
        PqCode::EventBad,
        PqCode::LedgerBad,
        PqCode::RenderBad,
        PqCode::ReplayBad,
        PqCode::CoverBad,
        PqCode::SegmentBad,
        PqCode::FuzzBad,
        PqCode::CorpusBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < PqCode::ALL.len() {
        if PqCode::ALL[i].val() == code {
            return PqCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 四点埋点 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceRecord {
    pub event: u32,
    pub stage: usize,
    pub tick: u32,
    pub cpu: u16,
    pub value: i32,
}

impl TraceRecord {
    pub fn new(event: u32, stage: usize, tick: u32, cpu: u16, value: i32) -> TraceRecord {
        TraceRecord {
            event,
            stage,
            tick,
            cpu,
            value,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TraceBook {
    pub rows: Vec<TraceRecord>,
    pub counts: [u32; POINT_TOTAL],
    pub accepts: u32,
    pub rejects: u32,
    pub errors: u32,
    pub full_events: u32,
    pub open_event: u32,
    pub open_mask: u8,
    pub seen: u8,
    pub last_event: Option<u32>,
    pub last_tick: u32,
    pub digest: u64,
}

impl TraceBook {
    pub fn new() -> TraceBook {
        TraceBook {
            rows: Vec::new(),
            counts: [0; POINT_TOTAL],
            accepts: 0,
            rejects: 0,
            errors: 0,
            full_events: 0,
            open_event: 0,
            open_mask: 0,
            seen: 0,
            last_event: None,
            last_tick: 0,
            digest: FNV_OFF,
        }
    }

    /// 异常显式记账：errors 与 rejects 同步增长，绝不留白。
    fn reject(&mut self, code: u32) -> Result<(), u32> {
        self.rejects = self.rejects.saturating_add(1);
        self.errors = self.errors.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, code);
        Err(code)
    }

    pub fn push(&mut self, rec: TraceRecord) -> Result<(), u32> {
        if rec.stage >= POINT_TOTAL {
            return self.reject(PqCode::StageBad.val());
        }
        if rec.event != self.open_event {
            // 新事件必须由 Entry 起手：缺入口等于整段观测丢失。
            if rec.stage != 0 {
                return self.reject(PqCode::PointBad.val());
            }
            if let Some(prev) = self.last_event {
                if rec.event <= prev {
                    return self.reject(PqCode::EventBad.val());
                }
            }
            self.open_event = rec.event;
            self.open_mask = 0;
        } else if rec.stage != trailing_stage(self.open_mask) {
            // 同一事件内只允许按 0→1→2→3 推进；重复段同样视为断裂。
            return self.reject(PqCode::OrderBad.val());
        }
        if rec.tick < self.last_tick {
            return self.reject(PqCode::TickBad.val());
        }
        if self.rows.len() >= TRACE_CAP {
            return self.reject(PqCode::LedgerBad.val());
        }
        self.last_event = Some(rec.event);
        self.last_tick = rec.tick;
        if let Some(c) = self.counts.get_mut(rec.stage) {
            *c = c.saturating_add(1);
        }
        self.accepts = self.accepts.saturating_add(1);
        self.open_mask |= 1u8 << rec.stage;
        self.seen |= 1u8 << rec.stage;
        if self.open_mask == POINT_MASK {
            self.full_events = self.full_events.saturating_add(1);
            self.open_mask = 0;
        }
        self.digest = p_core::fold_u32(self.digest, rec.event);
        self.digest = p_core::fold_u32(self.digest, rec.stage as u32);
        self.digest = p_core::fold_u32(self.digest, rec.tick);
        self.digest = p_core::fold_i32(self.digest, rec.value);
        self.rows.push(rec);
        Ok(())
    }

    pub fn points_covered(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < POINT_TOTAL {
            if (self.seen >> i) & 1 == 1 {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.rows.is_empty() {
            return Err(PqCode::CoverBad.val());
        }
        if self.rows.len() as u32 != self.accepts {
            return Err(PqCode::LedgerBad.val());
        }
        if self.errors != self.rejects {
            return Err(PqCode::LedgerBad.val());
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < POINT_TOTAL {
            match self.counts.get(i) {
                Some(c) => sum = sum.saturating_add(*c),
                None => return Err(PqCode::LedgerBad.val()),
            }
            i += 1;
        }
        if sum != self.accepts {
            return Err(PqCode::LedgerBad.val());
        }
        let bits = popcount8(self.open_mask);
        let expected = (self.full_events as usize)
            .saturating_mul(POINT_TOTAL)
            .saturating_add(bits);
        if expected != self.rows.len() {
            return Err(PqCode::LedgerBad.val());
        }
        Ok(())
    }
}

fn trailing_stage(mask: u8) -> usize {
    let mut n = 0usize;
    while n < POINT_TOTAL && (mask >> n) & 1 == 1 {
        n += 1;
    }
    n
}

fn popcount8(v: u8) -> usize {
    let mut x = v;
    let mut n = 0usize;
    while x != 0 {
        n += (x & 1) as usize;
        x >>= 1;
    }
    n
}

/// 黄金埋点流：每个事件四段齐全，再注入一次重复 Commit 作为显式异常。
pub fn golden_traces(rounds: u32) -> Result<TraceBook, u32> {
    if rounds == 0 {
        return Err(PqCode::CoverBad.val());
    }
    let mut book = TraceBook::new();
    let mut r = 0u32;
    while r < rounds {
        let base = r.saturating_mul(8);
        let mut stage = 0usize;
        while stage < POINT_TOTAL {
            let tick = base.saturating_add(stage as u32);
            book.push(TraceRecord::new(r, stage, tick, (r % 8) as u16, 120))?;
            stage += 1;
        }
        // 重复提交：该事件的四点已闭合，再来一段必须被拒并显式记账。
        let bad = TraceRecord::new(r, 3, base.saturating_add(4), 0, 120);
        if book.push(bad) != Err(PqCode::OrderBad.val()) {
            return Err(PqCode::OrderBad.val());
        }
        r = r.saturating_add(1);
    }
    book.audit()?;
    Ok(book)
}

// ── 四路 fuzz ─────────────────────────────────────────────────────
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
            digest: FNV_OFF,
            per_route: [[0; 3]; FUZZ_ROUTES],
        }
    }

    /// `ok` 是「处置符合预期」，`bad` 是样本类别；分类计数与对错判定分离。
    pub fn tally(&mut self, route: usize, ok: bool, bad: bool) {
        if route >= FUZZ_ROUTES {
            self.faults = self.faults.saturating_add(1);
            return;
        }
        if let Some(row) = self.per_route.get_mut(route) {
            if let Some(a) = row.get_mut(0) {
                *a = a.saturating_add(1);
            }
            let idx = if bad { 2 } else { 1 };
            if let Some(c) = row.get_mut(idx) {
                *c = c.saturating_add(1);
            }
        }
        self.attempts = self.attempts.saturating_add(1);
        if bad {
            self.rejected = self.rejected.saturating_add(1);
        } else {
            self.accepted = self.accepted.saturating_add(1);
        }
        if !ok {
            self.faults = self.faults.saturating_add(1);
        }
        self.digest = p_core::fold_u32(self.digest, route as u32);
        self.digest = p_core::fold_u32(self.digest, ok as u32);
        self.digest = p_core::fold_u32(self.digest, bad as u32);
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

    pub fn audit(&self) -> Result<(), u32> {
        if self.attempts == 0 {
            return Err(PqCode::CoverBad.val());
        }
        if !self.balanced() {
            return Err(PqCode::FuzzBad.val());
        }
        if !self.route_cover() {
            return Err(PqCode::CoverBad.val());
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < FUZZ_ROUTES {
            if let Some(row) = self.per_route.get(i) {
                sum = sum.saturating_add(row[0]);
            }
            i += 1;
        }
        if sum != self.attempts {
            return Err(PqCode::LedgerBad.val());
        }
        Ok(())
    }
}

/// 单例 fuzz：每路一个引擎，合法/非法两态各取一次，互不共用状态。
fn fuzz_case(route: usize, bad: bool) -> bool {
    let mut book = TraceBook::new();
    if book.push(TraceRecord::new(1, 0, 1, 2, 120)).is_err() {
        return false;
    }
    let res = match route {
        0 => {
            if bad {
                book.push(TraceRecord::new(1, 2, 2, 2, 120))
            } else {
                book.push(TraceRecord::new(1, 1, 2, 2, 120))
            }
        }
        1 => {
            if bad {
                book.push(TraceRecord::new(1, 1, 0, 2, 120))
            } else {
                book.push(TraceRecord::new(1, 1, 2, 2, 120))
            }
        }
        2 => {
            if bad {
                book.push(TraceRecord::new(1, POINT_TOTAL, 2, 2, 120))
            } else {
                book.push(TraceRecord::new(1, 1, 2, 2, 120))
            }
        }
        _ => {
            if bad {
                book.push(TraceRecord::new(2, 1, 2, 2, 120))
            } else {
                book.push(TraceRecord::new(1, 1, 2, 2, 120))
            }
        }
    };
    res.is_ok() != bad
}

pub fn fuzz_face(rounds: u32) -> Result<FuzzStats, u32> {
    if rounds == 0 {
        return Err(PqCode::CoverBad.val());
    }
    let mut stats = FuzzStats::new();
    let mut rng = p_core::Lcg::new(0x8A11_0076);
    let mut i = 0u32;
    while i < rounds {
        // 路号与样本类别各取高位：低位周期会与轮号模数同步。
        let route = ((rng.next() >> 33) % FUZZ_ROUTES as u64) as usize;
        let bad = (rng.next() >> 35) & 1 == 0;
        let ok = fuzz_case(route, bad);
        stats.tally(route, ok, bad);
        i = i.saturating_add(1);
    }
    stats.audit()?;
    Ok(stats)
}

// ── 账本渲染 ──────────────────────────────────────────────────────
pub fn render_traces(book: &TraceBook) -> String {
    format!(
        "{}|{}|{}|{}|0x{:016X}",
        book.rows.len(),
        book.full_events,
        book.errors,
        book.points_covered(),
        book.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(PqCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(PqCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(PqCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(PqCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(PqCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(PqCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(PqCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(PqCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 5 {
        return Err(PqCode::RenderBad.val());
    }
    let rows = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(PqCode::RenderBad.val()),
    })?;
    let full = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(PqCode::RenderBad.val()),
    })?;
    let errors = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(PqCode::RenderBad.val()),
    })?;
    let points = parse_u32(match parts.get(3) {
        Some(v) => v,
        None => return Err(PqCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(4) {
        Some(v) => v,
        None => return Err(PqCode::RenderBad.val()),
    })?;
    if rows == 0 || rows as usize > TRACE_CAP {
        return Err(PqCode::RenderBad.val());
    }
    if full.saturating_mul(POINT_TOTAL as u32) > rows {
        return Err(PqCode::LedgerBad.val());
    }
    if errors > rows {
        return Err(PqCode::LedgerBad.val());
    }
    if points != POINT_TOTAL as u32 {
        return Err(PqCode::CoverBad.val());
    }
    if digest == 0 {
        return Err(PqCode::RenderBad.val());
    }
    Ok(rows)
}

pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let book = golden_traces(GOLDEN_ROUNDS)?;
    let text = render_traces(&book);
    let rows = audit_render(&text)?;
    if rows != book.rows.len() as u32 {
        return Err(PqCode::ReplayBad.val());
    }
    let fuzz = fuzz_face(500)?;
    Ok((
        rows,
        book.full_events,
        book.errors,
        book.digest ^ fuzz.digest,
    ))
}

// ── 用例集（20 例入库可复跑） ─────────────────────────────────────
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaseRow {
    pub id: &'static str,
    pub title: &'static str,
    pub passed: bool,
    pub detail: String,
}

fn case(
    id: &'static str,
    title: &'static str,
    passed: bool,
    detail: String,
) -> CaseRow {
    CaseRow {
        id,
        title,
        passed,
        detail,
    }
}

pub fn case_rows() -> Vec<CaseRow> {
    let mut out = Vec::new();

    let golden = golden_traces(8);
    out.push(case(
        "F48976-01",
        "四点全覆盖",
        matches!(&golden, Ok(b) if b.points_covered() == POINT_TOTAL),
        match &golden {
            Ok(b) => format!("points={}", b.points_covered()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut b = TraceBook::new();
    let _ = b.push(TraceRecord::new(1, 0, 1, 0, 120));
    let skip = b.push(TraceRecord::new(1, 2, 2, 0, 120));
    out.push(case(
        "F48976-02",
        "跳段被拒",
        skip == Err(PqCode::OrderBad.val()),
        format!("res={:?}", skip),
    ));

    let mut b2 = TraceBook::new();
    let _ = b2.push(TraceRecord::new(1, 0, 5, 0, 120));
    let back = b2.push(TraceRecord::new(1, 1, 4, 0, 120));
    out.push(case(
        "F48976-03",
        "时钟回退被拒",
        back == Err(PqCode::TickBad.val()),
        format!("res={:?}", back),
    ));

    let mut b3 = TraceBook::new();
    let stage = b3.push(TraceRecord::new(1, POINT_TOTAL, 1, 0, 120));
    out.push(case(
        "F48976-04",
        "段号越界被拒",
        stage == Err(PqCode::StageBad.val()),
        format!("res={:?}", stage),
    ));

    let mut b4 = TraceBook::new();
    let miss = b4.push(TraceRecord::new(1, 1, 1, 0, 120));
    out.push(case(
        "F48976-05",
        "缺入口被拒",
        miss == Err(PqCode::PointBad.val()),
        format!("res={:?}", miss),
    ));

    let mut b5 = TraceBook::new();
    let _ = b5.push(TraceRecord::new(2, 0, 1, 0, 120));
    let older = b5.push(TraceRecord::new(1, 0, 2, 0, 120));
    out.push(case(
        "F48976-06",
        "事件倒退被拒",
        older == Err(PqCode::EventBad.val()),
        format!("res={:?}", older),
    ));

    out.push(case(
        "F48976-07",
        "空账被拒",
        TraceBook::new().audit() == Err(PqCode::CoverBad.val()),
        String::from("audit(empty)"),
    ));

    out.push(case(
        "F48976-08",
        "每段计数一致",
        matches!(&golden, Ok(b) if b.counts[0] == 8 && b.counts[1] == 8
            && b.counts[2] == 8 && b.counts[3] == 8),
        match &golden {
            Ok(b) => format!("counts={:?}", b.counts),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push(case(
        "F48976-09",
        "异常显式记账",
        matches!(&golden, Ok(b) if b.errors > 0 && b.errors == b.rejects),
        match &golden {
            Ok(b) => format!("errors={} rejects={}", b.errors, b.rejects),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push(case(
        "F48976-10",
        "满事件守恒",
        matches!(&golden, Ok(b) if b.full_events == 8 && b.rows.len() == 32),
        match &golden {
            Ok(b) => format!("full={} rows={}", b.full_events, b.rows.len()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let render = golden_traces(4).map(|b| render_traces(&b));
    out.push(case(
        "F48976-11",
        "账本可解析",
        matches!(&render, Ok(t) if audit_render(t) == Ok(16)),
        match &render {
            Ok(t) => format!("text={}", t),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let tampered = golden_traces(4).map(|b| render_traces(&b).replacen("0x", "0y", 1));
    out.push(case(
        "F48976-12",
        "篡改被拒",
        matches!(&tampered, Ok(t) if audit_render(t).is_err()),
        String::from("tamper 0x→0y"),
    ));

    let fuzz = fuzz_face(2_000);
    out.push(case(
        "F48976-13",
        "fuzz 账平衡",
        matches!(&fuzz, Ok(s) if s.balanced()),
        match &fuzz {
            Ok(s) => format!("attempts={} faults={}", s.attempts, s.faults),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push(case(
        "F48976-14",
        "fuzz 四路覆盖",
        matches!(&fuzz, Ok(s) if s.route_cover()),
        String::from("每路正反样本均非零"),
    ));

    let f2 = fuzz_face(2_000);
    out.push(case(
        "F48976-15",
        "fuzz 复跑同指纹",
        matches!((&fuzz, &f2), (Ok(a), Ok(b)) if a.digest == b.digest),
        String::from("double run"),
    ));

    out.push(case(
        "F48976-16",
        "码族 12 条互异",
        codes_are_distinct(),
        format!("codes={}", PqCode::ALL.len()),
    ));

    out.push(case(
        "F48976-17",
        "段位闭集",
        segment_guard(),
        format!("seg=0x{:04X}", SEG_BASE),
    ));

    out.push(case(
        "F48976-18",
        "黄金复跑一致",
        golden_run() == golden_run(),
        String::from("golden double run"),
    ));

    let mut b6 = TraceBook::new();
    let _ = b6.push(TraceRecord::new(1, 0, 1, 0, 120));
    let _ = b6.push(TraceRecord::new(1, 1, 2, 0, 120));
    out.push(case(
        "F48976-19",
        "未闭合事件不误判满",
        b6.full_events == 0 && b6.open_mask == 0x03,
        format!("full={} mask={}", b6.full_events, b6.open_mask),
    ));

    out.push(case(
        "F48976-20",
        "黄金账审计通过",
        matches!(&golden, Ok(b) if b.audit().is_ok()),
        String::from("audit(golden)"),
    ));

    out
}

pub fn audit_cases(rows: &[CaseRow]) -> Result<u32, u32> {
    if rows.len() != CASES {
        return Err(PqCode::CoverBad.val());
    }
    let mut i = 0usize;
    while i < rows.len() {
        match rows.get(i) {
            Some(r) => {
                if !r.id.starts_with("F48976-") {
                    return Err(PqCode::CorpusBad.val());
                }
                if !r.passed {
                    return Err(PqCode::CorpusBad.val());
                }
            }
            None => return Err(PqCode::CorpusBad.val()),
        }
        i += 1;
    }
    Ok(CASES as u32)
}

pub fn case_digest(rows: &[CaseRow]) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            h = p_core::fold_u32(h, r.passed as u32);
            let bytes = r.id.as_bytes();
            let mut k = 0usize;
            while k < bytes.len() {
                if let Some(b) = bytes.get(k) {
                    h = p_core::fold_u32(h, *b as u32);
                }
                k += 1;
            }
        }
        i += 1;
    }
    h
}

// ── 码族与段位 ────────────────────────────────────────────────────
pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < PqCode::ALL.len() {
        let mut j = i + 1;
        while j < PqCode::ALL.len() {
            if PqCode::ALL[i].val() == PqCode::ALL[j].val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < PqCode::ALL.len() {
        let v = PqCode::ALL[i].val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0F {
            return false;
        }
        i += 1;
    }
    true
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
        return Err(PqCode::CoverBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 段序断裂必须炸 OrderBad。
        let mut b = TraceBook::new();
        let _ = b.push(TraceRecord::new(1, 0, 1, 0, 120));
        if b.push(TraceRecord::new(1, 2, 2, 0, 120)).err()
            == Some(PqCode::OrderBad.val())
        {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 时钟回退必须炸 TickBad。
        let mut b2 = TraceBook::new();
        let _ = b2.push(TraceRecord::new(1, 0, 5, 0, 120));
        if b2.push(TraceRecord::new(1, 1, 4, 0, 120)).err()
            == Some(PqCode::TickBad.val())
        {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 段号越界必须炸 StageBad。
        let mut b3 = TraceBook::new();
        if b3.push(TraceRecord::new(1, POINT_TOTAL, 1, 0, 120)).err()
            == Some(PqCode::StageBad.val())
        {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ fuzz 账被破坏必须炸 FuzzBad。
        let mut f = FuzzStats::new();
        f.tally(0, true, false);
        f.accepted = f.accepted.saturating_add(1);
        if f.audit().err() == Some(PqCode::FuzzBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(PqCode::CoverBad.val());
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let book = golden_traces(GOLDEN_ROUNDS)?;
    if book.points_covered() != POINT_TOTAL {
        return Err(PqCode::CoverBad.val());
    }
    if book.errors == 0 || book.errors != book.rejects {
        return Err(PqCode::LedgerBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let f = fuzz_face(2_000)?;
    if !f.balanced() || !f.route_cover() {
        return Err(PqCode::FuzzBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let rows = case_rows();
    if audit_cases(&rows) != Ok(CASES as u32) {
        return Err(PqCode::CorpusBad.val());
    }
    if case_digest(&rows) != case_digest(&case_rows()) {
        return Err(PqCode::ReplayBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_traces(16)?;
    let b = golden_traces(16)?;
    if a.digest != b.digest || a.counts != b.counts {
        return Err(PqCode::ReplayBad.val());
    }
    let c = fuzz_face(500)?;
    let d = fuzz_face(500)?;
    if c.digest != d.digest || c.per_route != d.per_route {
        return Err(PqCode::ReplayBad.val());
    }
    Ok(())
}

// ── 域检查项（动态明细槽上限 8） ──────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    let golden = golden_traces(8);
    out.push((
        "F48976 四点100%覆盖",
        matches!(&golden, Ok(b) if b.points_covered() == POINT_TOTAL
            && b.full_events == 8),
        match &golden {
            Ok(b) => format!(
                "points={} full={} rows={}",
                b.points_covered(),
                b.full_events,
                b.rows.len()
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48976 异常零静默",
        matches!(&golden, Ok(b) if b.errors == 8
            && b.errors == b.rejects && b.audit().is_ok()),
        match &golden {
            Ok(b) => format!("errors={} rejects={}", b.errors, b.rejects),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut clock = TraceBook::new();
    let _ = clock.push(TraceRecord::new(1, 0, 9, 0, 120));
    let back = clock.push(TraceRecord::new(1, 1, 8, 0, 120));
    out.push((
        "F48976 时钟单调",
        back == Err(PqCode::TickBad.val()),
        format!("back={:?}", back),
    ));

    let fuzz = fuzz_face(FUZZ_ROUNDS);
    out.push((
        "F48977 fuzz×10^4零崩",
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
        "F48978 20例全绿",
        cases == Ok(CASES as u32),
        match &cases {
            Ok(n) => format!("cases={}", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let digest = case_digest(&rows);
    out.push((
        "F48978 用例复跑与编号",
        digest == case_digest(&case_rows()) && rows.len() == CASES,
        format!("digest=0x{:016X} rows={}", digest, rows.len()),
    ));

    out.push((
        "F48976 段位闭集",
        segment_guard(),
        format!("codes={} seg=0x{:04X}", PqCode::ALL.len(), SEG_BASE),
    ));

    let render = golden_traces(4).map(|b| render_traces(&b));
    out.push((
        "F48976 账本可解析",
        matches!(&render, Ok(t) if audit_render(t) == Ok(16)),
        match &render {
            Ok(t) => format!("text={}", t),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out
}

pub fn run_unx48976_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48976 高频观测埋点与用例集");
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
    fn constants_are_consistent() {
        assert_eq!(POINT_TOTAL, 4);
        assert_eq!(POINT_MASK, 0x0F);
        assert_eq!(CASES, 20);
        assert_eq!(PqCode::ALL.len(), 12);
        assert_eq!(GOLDEN_ROUNDS, 32);
    }

    #[test]
    fn record_new_keeps_fields() {
        let r = TraceRecord::new(7, 2, 33, 5, -120);
        assert_eq!(r.event, 7);
        assert_eq!(r.stage, 2);
        assert_eq!(r.tick, 33);
        assert_eq!(r.cpu, 5);
        assert_eq!(r.value, -120);
    }

    #[test]
    fn golden_covers_four_points() {
        match golden_traces(8) {
            Ok(b) => {
                assert_eq!(b.points_covered(), POINT_TOTAL);
                assert_eq!(b.full_events, 8);
                assert_eq!(b.rows.len(), 32);
                assert_eq!(b.accepts, 32);
                assert_eq!(b.errors, 8);
                assert_eq!(b.rejects, 8);
                assert!(b.audit().is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn stage_skip_is_rejected() {
        let mut b = TraceBook::new();
        assert_eq!(b.push(TraceRecord::new(1, 0, 1, 0, 120)), Ok(()));
        assert_eq!(
            b.push(TraceRecord::new(1, 2, 2, 0, 120)),
            Err(PqCode::OrderBad.val())
        );
        assert_eq!(b.errors, 1);
        assert_eq!(b.rejects, 1);
    }

    #[test]
    fn stage_overrun_is_rejected() {
        let mut b = TraceBook::new();
        assert_eq!(
            b.push(TraceRecord::new(1, POINT_TOTAL, 1, 0, 120)),
            Err(PqCode::StageBad.val())
        );
    }

    #[test]
    fn missing_entry_is_rejected() {
        let mut b = TraceBook::new();
        assert_eq!(
            b.push(TraceRecord::new(1, 1, 1, 0, 120)),
            Err(PqCode::PointBad.val())
        );
    }

    #[test]
    fn tick_regression_is_rejected() {
        let mut b = TraceBook::new();
        assert_eq!(b.push(TraceRecord::new(1, 0, 9, 0, 120)), Ok(()));
        assert_eq!(
            b.push(TraceRecord::new(1, 1, 8, 0, 120)),
            Err(PqCode::TickBad.val())
        );
    }

    #[test]
    fn event_regression_is_rejected() {
        let mut b = TraceBook::new();
        assert_eq!(b.push(TraceRecord::new(2, 0, 1, 0, 120)), Ok(()));
        assert_eq!(
            b.push(TraceRecord::new(1, 0, 2, 0, 120)),
            Err(PqCode::EventBad.val())
        );
    }

    #[test]
    fn empty_book_is_red() {
        assert_eq!(TraceBook::new().audit(), Err(PqCode::CoverBad.val()));
    }

    #[test]
    fn partial_event_keeps_mask() {
        let mut b = TraceBook::new();
        let _ = b.push(TraceRecord::new(1, 0, 1, 0, 120));
        let _ = b.push(TraceRecord::new(1, 1, 2, 0, 120));
        assert_eq!(b.open_mask, 0x03);
        assert_eq!(b.full_events, 0);
        assert_eq!(b.points_covered(), 2);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn full_event_resets_mask() {
        let mut b = TraceBook::new();
        let mut s = 0usize;
        while s < POINT_TOTAL {
            assert_eq!(b.push(TraceRecord::new(1, s, s as u32 + 1, 0, 120)), Ok(()));
            s += 1;
        }
        assert_eq!(b.open_mask, 0);
        assert_eq!(b.full_events, 1);
        assert_eq!(b.counts, [1, 1, 1, 1]);
    }

    #[test]
    fn duplicate_commit_is_rejected() {
        let mut b = TraceBook::new();
        let mut s = 0usize;
        while s < POINT_TOTAL {
            let _ = b.push(TraceRecord::new(1, s, s as u32 + 1, 0, 120));
            s += 1;
        }
        assert_eq!(
            b.push(TraceRecord::new(1, 3, 9, 0, 120)),
            Err(PqCode::OrderBad.val())
        );
    }

    #[test]
    fn fuzz_is_balanced_and_covers() {
        match fuzz_face(2_000) {
            Ok(s) => {
                assert!(s.balanced());
                assert!(s.route_cover());
                assert_eq!(s.attempts, 2_000);
                assert!(s.accepted > 0);
                assert!(s.rejected > 0);
                assert!(s.audit().is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_rejects_zero_rounds() {
        assert_eq!(fuzz_face(0), Err(PqCode::CoverBad.val()));
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        let b = golden_traces(4).unwrap_or_else(|_| TraceBook::new());
        let text = render_traces(&b);
        assert_eq!(audit_render(&text), Ok(16));
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
        assert!(audit_render("1|2").is_err());
    }

    #[test]
    fn render_rejects_wrong_points() {
        let b = golden_traces(2).unwrap_or_else(|_| TraceBook::new());
        let text = render_traces(&b).replacen("|4|", "|3|", 1);
        assert!(audit_render(&text).is_err());
    }

    #[test]
    fn corpus_is_twenty_green() {
        let rows = case_rows();
        assert_eq!(rows.len(), CASES);
        assert_eq!(audit_cases(&rows), Ok(CASES as u32));
        assert_eq!(case_digest(&rows), case_digest(&case_rows()));
    }

    #[test]
    fn corpus_detects_red() {
        let mut rows = case_rows();
        if let Some(r) = rows.get_mut(0) {
            r.passed = false;
        }
        assert_eq!(audit_cases(&rows), Err(PqCode::CorpusBad.val()));
    }

    #[test]
    fn corpus_detects_short() {
        let rows = case_rows();
        let short: Vec<CaseRow> = rows.iter().take(3).cloned().collect();
        assert_eq!(audit_cases(&short), Err(PqCode::CoverBad.val()));
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
    fn codes_are_distinct_and_in_window() {
        assert!(codes_are_distinct());
        assert!(segment_guard());
        assert_eq!(PqCode::ALL.len(), 12);
        assert!(!detail(PqCode::FuzzBad.val()).is_empty());
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48976_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
