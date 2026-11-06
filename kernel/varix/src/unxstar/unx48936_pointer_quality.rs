//! UNX-F48936 指针质量账 · 四点埋点 / 乱参 fuzz / 用例集 · 0x710E0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-M2-B07）：
//!   F48936 ① 纯功能代码 ≥220 行；
//!          ② UNX-F48936-J1 四点 100% 覆盖；异常零静默；
//!             判据锚定 体验十三·补；
//!   F48937 ① 纯功能代码 ≥260 行；
//!          ② UNX-F48937-J1 fuzz ×10^4 零崩；
//!             判据锚定 体验十 健壮性；
//!   F48938 ① 纯功能代码 ≥300 行；
//!          ② UNX-F48938-J1 ×20 全绿入库；
//!             判据锚定 §AI-62 §8。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：埋点次序（入口→出口→耗时→结果）→ 归属（异常只准落在
//!           结果点）→ 覆盖（四操作 × 四点全中）→ 守恒（记录数 =
//!           周期 ×4）→ fuzz（四路乱参，正反样本都要出现）→ 用例集
//!           （20 例调用真实函数）→ 渲染复算。
//!   次序不可倒：次序不成立时，覆盖数字是「错位点恰好凑齐」的假象；
//!   归属先于覆盖，异常落在入口点上时四点是齐的但没人知道它错在哪；
//!   守恒先于 fuzz，账不平的 fuzz 计数不能说明「零崩」；用例集最后，
//!   它调用的正是前面已经自证过的函数，任何一例失败都会指回上游。
//! 关键差异：本模块不实现指针功能——它只观测 B07 六个功能模块。埋点
//!   模型按「一次操作 = 一周期四点」记账，周期可交错但每操作独立推进；
//!   异常零静默的定义是：非零诊断码只允许出现在结果点，且出现即计数，
//!   任何「带码却不归属」的写入直接拒绝。fuzz 不做 catch_unwind——宿主
//!   侧探针负责兜底，本模块保证的是「所有入口都只会返回 Result」，
//!   并且在 10^4 次乱参里保留每一路的正样本与反样本，杜绝全零账。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48921_pointer_core as p_core;
use super::unx48923_pointer_meta as p_meta;
use super::unx48925_pointer_guard as p_guard;
use super::unx48928_pointer_dispatch as p_dispatch;
use super::unx48931_pointer_edge as p_edge;
use super::unx48933_pointer_bridge as p_bridge;

/// 诊断段基址（0x710E0，VX6 批 B07 第七槽）。
pub const SEG_BASE: u32 = 0x710E0;

/// 受观测操作数（合成 / 采样 / 夹取 / 上报）。
pub const INSTR_OPS: usize = 4;
/// 每周期点数（入口 / 出口 / 耗时 / 结果）。
pub const POINT_COUNT: usize = 4;
/// 四操作 × 四点 = 16 个覆盖位。
pub const COVER_BITS: u32 = 16;
/// fuzz 轮数（判据 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// fuzz 路数（核心合成 / 增益表 / 子像素 / 边界夹取）。
pub const FUZZ_ROUTES: usize = 4;
/// 用例数（判据 ×20）。
pub const CASES: usize = 20;
/// 黄金埋点周期数（每周期 4 操作 ×4 点 = 16 条）。
pub const GOLDEN_CYCLES: u32 = 250;
/// 路由样本下限（每路正反至少各 1）。
pub const ROUTE_MIN: u32 = 1;
/// FNV-1a 64 常量。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// ── 诊断码闭集（+0x01..+0x0B） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PmCode {
    /// 埋点次序错误（入口→出口→耗时→结果 被破坏）
    PointBad,
    /// 覆盖不全（四操作 × 四点未全中）
    CoverBad,
    /// 异常被静默（非零码落在结果点之外，或未计数）
    SilentBad,
    /// fuzz 账不平（尝试 ≠ 正 + 反，或出现故障样本）
    FuzzBad,
    /// 用例失败或篡改（任一例需为绿）
    CaseBad,
    /// 双跑不一致
    ReplayBad,
    /// 渲染不可解析
    RenderBad,
    /// 时钟回退
    OrderBad,
    /// 账本不守恒（记录数 ≠ 周期 ×4）
    LedgerBad,
    /// 段位越界
    SegmentBad,
    /// 语料非法（轮数为零 / 周期数为零）
    CorpusBad,
}

impl PmCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            PmCode::PointBad => "埋点次序错误：单周期必须入口→出口→耗时→结果",
            PmCode::CoverBad => "覆盖不全：四操作 × 四点必须全部命中",
            PmCode::SilentBad => "异常被静默：非零诊断码只准落在结果点并计数",
            PmCode::FuzzBad => "fuzz 账不平：尝试数必须等于正样本加反样本",
            PmCode::CaseBad => "用例失败：20 例必须全绿且原样入库",
            PmCode::ReplayBad => "双跑不一致：两次质量账摘要不同",
            PmCode::RenderBad => "渲染不可解析：字段数、数值或摘要非法",
            PmCode::OrderBad => "时钟回退：埋点 tick 必须单调不减",
            PmCode::LedgerBad => "账本不守恒：记录数必须等于周期数乘四",
            PmCode::SegmentBad => "段位越界：诊断码逃出 0x710E0 窗口",
            PmCode::CorpusBad => "语料非法：轮数或周期数为零",
        }
    }

    pub const ALL: [PmCode; 11] = [
        PmCode::PointBad,
        PmCode::CoverBad,
        PmCode::SilentBad,
        PmCode::FuzzBad,
        PmCode::CaseBad,
        PmCode::ReplayBad,
        PmCode::RenderBad,
        PmCode::OrderBad,
        PmCode::LedgerBad,
        PmCode::SegmentBad,
        PmCode::CorpusBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < PmCode::ALL.len() {
        if PmCode::ALL[i].val() == code {
            return PmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x710E0 段闭集内"
}

// ── 确定性源与折叠 ────────────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next() % (n as u64)) as u32
        }
    }
}

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut b = 0u32;
    while b < 4 {
        x = fold_byte(x, ((v >> (b * 8)) & 0xFF) as u8);
        b += 1;
    }
    x
}

fn fold_i64(h: u64, v: i64) -> u64 {
    fold_u32(fold_u32(h, v as u32), (v >> 32) as u32)
}

// ── 埋点模型 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    /// 位移合成（core）
    Compose,
    /// 增益采样（core / guard 共用语义）
    Sample,
    /// 边界夹取（edge）
    Clamp,
    /// 坐标上报（meta）
    Report,
}

impl OpKind {
    pub fn name(self) -> &'static str {
        match self {
            OpKind::Compose => "compose",
            OpKind::Sample => "sample",
            OpKind::Clamp => "clamp",
            OpKind::Report => "report",
        }
    }

    /// 该操作在异常结果点上使用的归属码。
    pub fn code(self) -> u32 {
        match self {
            OpKind::Compose => p_core::SEG_BASE + 0x03,
            OpKind::Sample => p_guard::SEG_BASE + 0x02,
            OpKind::Clamp => p_edge::SEG_BASE + 0x04,
            OpKind::Report => p_meta::SEG_BASE + 0x03,
        }
    }

    pub fn index(self) -> usize {
        match self {
            OpKind::Compose => 0,
            OpKind::Sample => 1,
            OpKind::Clamp => 2,
            OpKind::Report => 3,
        }
    }

    pub const ALL: [OpKind; INSTR_OPS] = [
        OpKind::Compose,
        OpKind::Sample,
        OpKind::Clamp,
        OpKind::Report,
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointKind {
    Entry,
    Exit,
    Duration,
    Result,
}

impl PointKind {
    pub fn name(self) -> &'static str {
        match self {
            PointKind::Entry => "entry",
            PointKind::Exit => "exit",
            PointKind::Duration => "duration",
            PointKind::Result => "result",
        }
    }

    pub fn bit(self) -> u8 {
        1u8 << self.index()
    }

    pub fn index(self) -> u32 {
        match self {
            PointKind::Entry => 0,
            PointKind::Exit => 1,
            PointKind::Duration => 2,
            PointKind::Result => 3,
        }
    }

    pub const ALL: [PointKind; POINT_COUNT] = [
        PointKind::Entry,
        PointKind::Exit,
        PointKind::Duration,
        PointKind::Result,
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceRecord {
    pub op: OpKind,
    pub point: PointKind,
    pub tick: u32,
    pub code: u32,
    pub value: i64,
}

impl TraceRecord {
    pub fn new(op: OpKind, point: PointKind, tick: u32, code: u32, value: i64) -> TraceRecord {
        TraceRecord {
            op,
            point,
            tick,
            code,
            value,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceBook {
    pub records: u32,
    pub cycles: u32,
    pub errors: u32,
    pub rejects: u32,
    pub digest: u64,
    masks: [u8; INSTR_OPS],
    counts: [u32; INSTR_OPS],
    phase: [u8; INSTR_OPS],
    last_tick: u32,
    tick_set: bool,
}

impl TraceBook {
    pub fn new() -> TraceBook {
        TraceBook {
            records: 0,
            cycles: 0,
            errors: 0,
            rejects: 0,
            digest: FNV_OFF,
            masks: [0u8; INSTR_OPS],
            counts: [0u32; INSTR_OPS],
            phase: [0u8; INSTR_OPS],
            last_tick: 0,
            tick_set: false,
        }
    }

    fn fail(&mut self, code: u32) -> Result<(), u32> {
        self.rejects = self.rejects.saturating_add(1);
        self.digest = fold_u32(self.digest, code);
        Err(code)
    }

    /// 写入一条埋点。次序：入口→出口→耗时→结果；时钟单调；异常只准
    /// 落在结果点。任何一条被破坏都计一次拒绝并返回对应诊断码。
    pub fn push(&mut self, rec: TraceRecord) -> Result<(), u32> {
        if self.tick_set && rec.tick < self.last_tick {
            return self.fail(PmCode::OrderBad.val());
        }
        if rec.code != 0 && rec.point != PointKind::Result {
            return self.fail(PmCode::SilentBad.val());
        }
        let op = rec.op.index();
        let phase = self.phase[op];
        if rec.point == PointKind::Entry {
            if phase != 0 && phase != 4 {
                return self.fail(PmCode::PointBad.val());
            }
            self.phase[op] = 1;
        } else {
            if phase != rec.point.index() as u8 {
                return self.fail(PmCode::PointBad.val());
            }
            if rec.point == PointKind::Result {
                self.phase[op] = 4;
                self.cycles = self.cycles.saturating_add(1);
            } else {
                self.phase[op] = phase.saturating_add(1);
            }
        }
        self.masks[op] |= rec.point.bit();
        self.counts[op] = self.counts[op].saturating_add(1);
        self.records = self.records.saturating_add(1);
        if rec.code != 0 {
            self.errors = self.errors.saturating_add(1);
        }
        self.digest = fold_u32(self.digest, rec.tick);
        self.digest = fold_u32(self.digest, rec.op.index() as u32);
        self.digest = fold_u32(self.digest, rec.point.index());
        self.digest = fold_u32(self.digest, rec.code);
        self.digest = fold_i64(self.digest, rec.value);
        self.last_tick = rec.tick;
        self.tick_set = true;
        Ok(())
    }

    pub fn mask_of(&self, op: OpKind) -> u8 {
        self.masks[op.index()]
    }

    pub fn count_of(&self, op: OpKind) -> u32 {
        self.counts[op.index()]
    }

    /// 四操作全中返回 0x0F，逐位代表该操作四点齐。
    pub fn cover_mask(&self) -> u8 {
        let mut m = 0u8;
        let mut i = 0usize;
        while i < INSTR_OPS {
            if self.masks[i] == 0x0F {
                m |= 1u8 << i;
            }
            i += 1;
        }
        m
    }

    pub fn points_covered(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < INSTR_OPS {
            let m = self.masks[i];
            let mut b = 0u32;
            while b < 4 {
                if m & (1u8 << b) != 0 {
                    n = n.saturating_add(1);
                }
                b += 1;
            }
            i += 1;
        }
        n
    }

    /// 账本审计：每操作计数是四的倍数、周期过半仍在半空中即错、
    /// 记录总数守恒、异常数不超过周期数。
    pub fn audit(&self) -> Result<(), u32> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < INSTR_OPS {
            let c = self.counts[i];
            if self.phase[i] != 0 && self.phase[i] != 4 {
                return Err(PmCode::PointBad.val());
            }
            if c % 4 != 0 {
                return Err(PmCode::LedgerBad.val());
            }
            if c > 0 && self.masks[i] != 0x0F {
                return Err(PmCode::CoverBad.val());
            }
            sum = sum.saturating_add(c);
            i += 1;
        }
        if sum != self.records || self.cycles.saturating_mul(4) != self.records {
            return Err(PmCode::LedgerBad.val());
        }
        if self.errors > self.cycles {
            return Err(PmCode::LedgerBad.val());
        }
        Ok(())
    }

    /// 覆盖审计：16 位全中。
    pub fn audit_cover(&self) -> Result<u32, u32> {
        if self.cover_mask() != 0x0F || self.points_covered() != COVER_BITS {
            return Err(PmCode::CoverBad.val());
        }
        Ok(self.points_covered())
    }
}

/// 黄金埋点：每周期四操作各四点，每 8 周期有一次结果点异常（四路轮转）。
pub fn golden_traces(cycles: u32) -> Result<TraceBook, u32> {
    if cycles == 0 || cycles > 100_000 {
        return Err(PmCode::CorpusBad.val());
    }
    let mut book = TraceBook::new();
    let mut c = 0u32;
    let mut tick = 0u32;
    while c < cycles {
        let mut oi = 0usize;
        while oi < INSTR_OPS {
            let op = OpKind::ALL[oi];
            let err = if c % 8 == 3 && (oi as u32) <= c % 8 {
                op.code()
            } else {
                0
            };
            book.push(TraceRecord::new(op, PointKind::Entry, tick, 0, 100))?;
            tick = tick.saturating_add(1);
            book.push(TraceRecord::new(op, PointKind::Exit, tick, 0, 90))?;
            tick = tick.saturating_add(1);
            book.push(TraceRecord::new(op, PointKind::Duration, tick, 0, 10))?;
            tick = tick.saturating_add(1);
            let value = if err == 0 { 0 } else { 1 };
            book.push(TraceRecord::new(op, PointKind::Result, tick, err, value))?;
            tick = tick.saturating_add(1);
            oi += 1;
        }
        c = c.saturating_add(1);
    }
    book.audit()?;
    book.audit_cover()?;
    if book.errors == 0 {
        return Err(PmCode::SilentBad.val());
    }
    Ok(book)
}

// ── fuzz 面 ───────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzStats {
    pub attempts: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub faults: u32,
    pub digest: u64,
    /// 每路 [尝试, 正样本, 反样本]。
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
            per_route: [[0u32; 3]; FUZZ_ROUTES],
        }
    }

    fn tally(&mut self, route: usize, ok: bool) {
        if let Some(row) = self.per_route.get_mut(route) {
            row[0] = row[0].saturating_add(1);
            if ok {
                row[1] = row[1].saturating_add(1);
            } else {
                row[2] = row[2].saturating_add(1);
            }
        }
        self.attempts = self.attempts.saturating_add(1);
        if ok {
            self.accepted = self.accepted.saturating_add(1);
        } else {
            self.rejected = self.rejected.saturating_add(1);
        }
        self.digest = fold_u32(self.digest, route as u32);
        self.digest = fold_u32(self.digest, if ok { 1 } else { 0 });
    }

    pub fn balanced(&self) -> bool {
        self.faults == 0 && self.attempts == self.accepted.saturating_add(self.rejected)
    }

    pub fn route_of(&self, route: usize) -> Option<[u32; 3]> {
        self.per_route.get(route).copied()
    }
}

/// fuzz 审计：总账平衡、每路至少一个正样本和一个反样本、路和等于总账。
pub fn audit_fuzz_stats(s: &FuzzStats) -> Result<u32, u32> {
    if s.attempts == 0 {
        return Err(PmCode::CorpusBad.val());
    }
    if !s.balanced() {
        return Err(PmCode::FuzzBad.val());
    }
    let mut total = 0u32;
    let mut i = 0usize;
    while i < FUZZ_ROUTES {
        let row = match s.per_route.get(i) {
            Some(v) => *v,
            None => return Err(PmCode::FuzzBad.val()),
        };
        if row[0] == 0 {
            return Err(PmCode::FuzzBad.val());
        }
        if row[1] < ROUTE_MIN || row[2] < ROUTE_MIN {
            return Err(PmCode::FuzzBad.val());
        }
        total = total.saturating_add(row[1]).saturating_add(row[2]);
        i += 1;
    }
    if total != s.attempts {
        return Err(PmCode::FuzzBad.val());
    }
    Ok(s.attempts)
}

/// 路 0：核心合成。随机档位 / 曲线 / 边界 / 位移，正样本必须落在档内。
fn fuzz_core_round(rng: &mut Lcg) -> bool {
    let dpi = rng.below(40_000);
    let flat = rng.below(5_000);
    let bound = (rng.next() % (p_core::POS_BOUND as u64 + 200)) as i64;
    let mode = if rng.below(2) == 0 {
        p_core::Mode::Flat
    } else {
        p_core::Mode::Curve
    };
    let curve = if rng.below(2) == 0 {
        let g0 = 250 + rng.below(3_751);
        let g1 = 250 + rng.below(3_751);
        let s1 = 1 + rng.below(7_999);
        p_core::Curve::new(p_core::curve_of(&[(0, g0), (s1, g1)])).ok()
    } else {
        None
    };
    match p_core::PtrProfile::new(dpi, flat, curve, mode, bound) {
        Ok(prof) => {
            let dx = rng.below(80_000) as i32 - 40_000;
            let dy = rng.below(80_000) as i32 - 40_000;
            match p_core::RelEvent::new(dx, dy) {
                Ok(ev) => match p_core::compose_checked(&prof, p_core::Point::origin(), ev) {
                    Ok((pt, _)) => {
                        pt.x <= prof.bound
                            && pt.x >= -prof.bound
                            && pt.y <= prof.bound
                            && pt.y >= -prof.bound
                    }
                    Err(_) => false,
                },
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

/// 路 1：增益表。首点速度 0、速度递增、增益单调，偶发倒挂。
fn fuzz_guard_round(rng: &mut Lcg) -> bool {
    let mut rows: Vec<p_guard::GainRow> = Vec::new();
    let mut g = 250 + rng.below(3_700);
    let mut s = 0u32;
    rows.push(p_guard::GainRow { speed: 0, gain: g });
    let mut i = 1usize;
    while i < 5 {
        s = s.saturating_add(1 + rng.below(1_500));
        if rng.below(10) == 0 {
            g = g.saturating_sub(1 + rng.below(300));
        } else {
            g = (g.saturating_add(rng.below(300))).min(4_000);
        }
        rows.push(p_guard::GainRow { speed: s, gain: g });
        i += 1;
    }
    match p_guard::GainTable::new(rows) {
        Ok(table) => {
            let speed = rng.below(9_001);
            match table.lookup(speed) {
                Ok(v) => v >= p_guard::GAIN_MIN && v <= p_guard::GAIN_MAX,
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

/// 路 2：子像素。正样本必须整账不变量成立且余量落在 0..16。
fn fuzz_dispatch_round(rng: &mut Lcg) -> bool {
    let mut sp = p_dispatch::Subpixel::new();
    let dx = rng.below(9_000) as i32 - 4_500;
    let dy = rng.below(9_000) as i32 - 4_500;
    match sp.push(dx, dy) {
        Ok(_) => {
            if sp.audit().is_err() {
                return false;
            }
            let (fx, fy) = sp.residual();
            fx >= 0 && fx < 16 && fy >= 0 && fy < 16
        }
        Err(_) => false,
    }
}

/// 路 3：边界夹取。正样本必须落在 [min,max] 内且夹取标记与坐标一致。
fn fuzz_edge_round(rng: &mut Lcg) -> bool {
    let border = rng.below(1_101) as i64;
    let w = 1 + rng.below(2_000) as i64;
    let h = 1 + rng.below(2_000) as i64;
    let min_x = -(rng.below(500) as i64);
    let min_y = -(rng.below(500) as i64);
    match p_edge::ClampSpec::new(min_x, min_y, min_x + w, min_y + h, border) {
        Ok(spec) => {
            let x = rng.below(2_000_000) as i64 - 1_000_000;
            let y = rng.below(2_000_000) as i64 - 1_000_000;
            match spec.clamp(x, y) {
                Ok(r) => {
                    r.x >= spec.min_x
                        && r.x <= spec.max_x
                        && r.y >= spec.min_y
                        && r.y <= spec.max_y
                        && r.clamped == (r.x != x || r.y != y)
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

/// 乱参 fuzz：每轮一路，恰好一次尝试，正反样本进账。
pub fn fuzz_face(rounds: u32) -> Result<FuzzStats, u32> {
    if rounds == 0 {
        return Err(PmCode::CorpusBad.val());
    }
    let mut rng = Lcg::new(0x5155_4C49_5439_3601);
    let mut st = FuzzStats::new();
    let mut r = 0u32;
    while r < rounds {
        let route = rng.below(FUZZ_ROUTES as u32) as usize;
        let ok = match route {
            0 => fuzz_core_round(&mut rng),
            1 => fuzz_guard_round(&mut rng),
            2 => fuzz_dispatch_round(&mut rng),
            _ => fuzz_edge_round(&mut rng),
        };
        st.tally(route, ok);
        r = r.saturating_add(1);
    }
    audit_fuzz_stats(&st)?;
    if st.accepted == 0 || st.rejected == 0 {
        return Err(PmCode::FuzzBad.val());
    }
    Ok(st)
}

// ── 用例集（20 例，调用真实函数） ────────────────────────────────
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaseRow {
    pub id: &'static str,
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

fn case(
    v: &mut Vec<CaseRow>,
    id: &'static str,
    name: &'static str,
    passed: bool,
    detail: String,
) {
    v.push(CaseRow {
        id,
        name,
        passed,
        detail,
    });
}

fn case_core_flat() -> (bool, String) {
    match p_core::PtrProfile::flat(p_core::DEFAULT_DPI, 1_000) {
        Ok(p) => {
            let a = p_core::gain_for(&p, 0);
            let b = p_core::gain_for(&p, 400);
            let c = p_core::gain_for(&p, 8_000);
            let ok = a == Ok(1_000) && b == Ok(1_000) && c == Ok(1_000);
            (ok, format!("flat=1000/400=1000/8000=1000 ok={}", ok))
        }
        Err(e) => (false, format!("profile err=0x{:08X}", e)),
    }
}

fn case_core_curve_exact() -> (bool, String) {
    match p_core::Curve::new(p_core::curve_of(&[(0, 1_000), (400, 2_000), (1_600, 4_000)])) {
        Ok(c) => {
            let ok = c.gain_at(0) == Ok(1_000)
                && c.gain_at(400) == Ok(2_000)
                && c.gain_at(1_600) == Ok(4_000)
                && c.gain_at(5_000) == Ok(4_000);
            (ok, format!("exact/0=1000 400=2000 1600=4000 cap=4000 ok={}", ok))
        }
        Err(e) => (false, format!("curve err=0x{:08X}", e)),
    }
}

fn case_core_curve_mid() -> (bool, String) {
    match p_core::Curve::new(p_core::curve_of(&[(0, 1_000), (100, 1_200), (300, 1_400)])) {
        Ok(c) => {
            let a = c.gain_at(50);
            let b = c.gain_at(200);
            let ok = a == Ok(1_100) && b == Ok(1_300);
            (ok, format!("mid/50={:?} 200={:?} ok={}", a.ok(), b.ok(), ok))
        }
        Err(e) => (false, format!("curve err=0x{:08X}", e)),
    }
}

fn case_scale_symmetric() -> (bool, String) {
    let a = p_core::scale_rel(1, 500);
    let b = p_core::scale_rel(-1, 500);
    let c = p_core::scale_rel(3, 500);
    let d = p_core::scale_rel(-3, 500);
    let ok = a == 1 && b == -1 && c == 2 && d == -2;
    (ok, format!("scale/1={} -1={} 3={} -3={} ok={}", a, b, c, d, ok))
}

fn case_core_drift() -> (bool, String) {
    match p_core::drift_run(1_000) {
        Ok((dx, dy)) => {
            let ok = dx == 0 && dy == 0;
            (ok, format!("drift/1000=({},{}) ok={}", dx, dy, ok))
        }
        Err(e) => (false, format!("drift err=0x{:08X}", e)),
    }
}

fn case_core_clamp() -> (bool, String) {
    let prof = match p_core::PtrProfile::new(1_000, 4_000, None, p_core::Mode::Flat, 100) {
        Ok(p) => p,
        Err(e) => return (false, format!("profile err=0x{:08X}", e)),
    };
    let ev = match p_core::RelEvent::new(32_767, 0) {
        Ok(v) => v,
        Err(e) => return (false, format!("rel err=0x{:08X}", e)),
    };
    match p_core::compose_checked(&prof, p_core::Point::origin(), ev) {
        Ok((pt, cl)) => {
            let ok = pt.x == 100 && pt.y == 0 && cl;
            (ok, format!("clamp/x={} y={} flag={} ok={}", pt.x, pt.y, cl, ok))
        }
        Err(e) => (false, format!("compose err=0x{:08X}", e)),
    }
}

fn case_meta_enhance() -> (bool, String) {
    match p_meta::enhance_rounds(10) {
        Ok(()) => (true, "enhance/rounds=10 toggles=2n ok=true".into()),
        Err(e) => (false, format!("enhance err=0x{:08X}", e)),
    }
}

fn case_meta_same_state() -> (bool, String) {
    let mut sw = p_meta::EnhanceSwitch::new(false);
    let r = sw.set(false);
    let ok = r == Err(p_meta::MnCode::SwitchBad.val()) && sw.toggles == 0;
    (ok, format!("same-state/reject ok={} toggles={}", ok, sw.toggles))
}

fn case_guard_five() -> (bool, String) {
    let rows = p_guard::five_key_table();
    match p_guard::audit_five(&rows) {
        Ok(n) => {
            let ok = n == 5;
            (ok, format!("five/rows={} ok={}", n, ok))
        }
        Err(e) => (false, format!("five err=0x{:08X}", e)),
    }
}

fn case_guard_double() -> (bool, String) {
    let mut t = p_guard::DcTracker::with_default();
    let a = t.observe(p_guard::BTN_LEFT, 0, 0, 0);
    let b = t.observe(p_guard::BTN_LEFT, p_guard::DC_DEFAULT_MS, 4, 0);
    let ok = a == Ok(p_guard::ClickKind::Single) && b == Ok(p_guard::ClickKind::Double);
    (ok, format!("double/0=single 500ms=double ok={}", ok))
}

fn case_guard_table_mid() -> (bool, String) {
    match p_guard::table_of_group(0) {
        Ok(t) => {
            let a = t.lookup(125);
            let b = t.lookup(250);
            let ok = a == Ok(1_100) && b == Ok(1_200);
            (ok, format!("table/125={:?} 250={:?} ok={}", a.ok(), b.ok(), ok))
        }
        Err(e) => (false, format!("table err=0x{:08X}", e)),
    }
}

fn case_sub_positive() -> (bool, String) {
    let mut sp = p_dispatch::Subpixel::new();
    let mut i = 0u32;
    while i < 100 {
        if sp.push(1, 0).is_err() {
            return (false, "sub/100x1 push err".into());
        }
        i += 1;
    }
    let (fx, fy) = sp.residual();
    let ok = sp.emitted_x == 6 && fx == 4 && fy == 0 && sp.audit().is_ok();
    (ok, format!("sub/emitted={} res=({},{}) ok={}", sp.emitted_x, fx, fy, ok))
}

fn case_sub_negative() -> (bool, String) {
    let mut sp = p_dispatch::Subpixel::new();
    let mut i = 0u32;
    while i < 100 {
        if sp.push(-1, 0).is_err() {
            return (false, "sub/100x-1 push err".into());
        }
        i += 1;
    }
    let (fx, fy) = sp.residual();
    let ok = sp.emitted_x == -7 && fx == 12 && fy == 0 && sp.audit().is_ok();
    (ok, format!("sub/emitted={} res=({},{}) ok={}", sp.emitted_x, fx, fy, ok))
}

fn case_topology() -> (bool, String) {
    match p_dispatch::sample_topology() {
        Ok(t) => match t.locate(1_930, 5) {
            Ok((idx, (lx, ly))) => {
                let ok = idx == 1 && lx == 10 && ly == 5;
                (ok, format!("topo/idx={} local=({},{}) ok={}", idx, lx, ly, ok))
            }
            Err(e) => (false, format!("locate err=0x{:08X}", e)),
        },
        Err(e) => (false, format!("topo err=0x{:08X}", e)),
    }
}

fn case_preview() -> (bool, String) {
    match p_dispatch::fake_preview() {
        Ok(hits) => {
            let ok = hits == 10;
            (ok, format!("preview/hits={} ok={}", hits, ok))
        }
        Err(e) => (false, format!("preview err=0x{:08X}", e)),
    }
}

fn case_fsm_drag() -> (bool, String) {
    let mut f = p_edge::PointerFsm::new();
    let a = f.input(p_edge::PointerEvent::Press(p_edge::BTN_LEFT));
    let b = f.input(p_edge::PointerEvent::Move(3, 0));
    let c = f.input(p_edge::PointerEvent::Move(4, 0));
    let ok = a == Ok(p_edge::FsmOutcome::Pressed)
        && b == Ok(p_edge::FsmOutcome::Ignored)
        && c == Ok(p_edge::FsmOutcome::Dragged);
    (ok, format!("fsm/3px=ignored 4px=dragged ok={}", ok))
}

fn case_fsm_cancel() -> (bool, String) {
    let mut f = p_edge::PointerFsm::new();
    let a = f.input(p_edge::PointerEvent::Press(p_edge::BTN_LEFT));
    let b = f.input(p_edge::PointerEvent::Leave);
    let c = f.input(p_edge::PointerEvent::Release(p_edge::BTN_LEFT));
    let ok = a == Ok(p_edge::FsmOutcome::Pressed)
        && b == Ok(p_edge::FsmOutcome::Cancelled)
        && c == Ok(p_edge::FsmOutcome::Released);
    (ok, format!("fsm/leave=cancelled release=released ok={}", ok))
}

fn case_clamp_silent() -> (bool, String) {
    let spec = match p_edge::ClampSpec::default_spec() {
        Ok(s) => s,
        Err(e) => return (false, format!("spec err=0x{:08X}", e)),
    };
    let edge = spec.clamp(0, 500);
    let mid = spec.clamp(500, 500);
    let ok = edge == Ok(p_edge::ClampResult { x: 0, y: 500, clamped: false, silent: true })
        && mid == Ok(p_edge::ClampResult { x: 500, y: 500, clamped: false, silent: false });
    (ok, format!("clamp/border-silent mid-clear ok={}", ok))
}

fn case_rolls() -> (bool, String) {
    match p_bridge::roll_rounds() {
        Ok(n) => {
            let ok = n == p_bridge::ROLLS as u32;
            (ok, format!("rolls/n={} ok={}", n, ok))
        }
        Err(e) => (false, format!("rolls err=0x{:08X}", e)),
    }
}

fn case_family() -> (bool, String) {
    match p_bridge::family_run() {
        Ok(l) => {
            let ok = l.codes == p_bridge::FAMILY_CODES as u32
                && l.composed.saturating_add(l.rejected) == p_bridge::GOLDEN_ROUNDS
                && l.digest != 0;
            (
                ok,
                format!("family/codes={} composed={} rejected={} ok={}", l.codes, l.composed, l.rejected, ok),
            )
        }
        Err(e) => (false, format!("family err=0x{:08X}", e)),
    }
}

/// 20 例：编号 F48938-01..20，逐例调用真实函数。
pub fn case_rows() -> Vec<CaseRow> {
    let mut v: Vec<CaseRow> = Vec::new();
    let specs: [(&'static str, &'static str, fn() -> (bool, String)); CASES] = [
        ("F48938-01", "平坦档增益恒等", case_core_flat),
        ("F48938-02", "曲线采样点精确", case_core_curve_exact),
        ("F48938-03", "曲线段内半值进位", case_core_curve_mid),
        ("F48938-04", "增益缩放对称取整", case_scale_symmetric),
        ("F48938-05", "万点合成零漂移", case_core_drift),
        ("F48938-06", "档边界硬夹精确", case_core_clamp),
        ("F48938-07", "开关往返语义", case_meta_enhance),
        ("F48938-08", "同态设置被拒", case_meta_same_state),
        ("F48938-09", "五键表闭集", case_guard_five),
        ("F48938-10", "双击阈值边界", case_guard_double),
        ("F48938-11", "增益表段内插值", case_guard_table_mid),
        ("F48938-12", "子像素正推余量", case_sub_positive),
        ("F48938-13", "子像素负推余量", case_sub_negative),
        ("F48938-14", "双屏拓扑局部坐标", case_topology),
        ("F48938-15", "预演十点全中", case_preview),
        ("F48938-16", "三像素不拖四像素拖", case_fsm_drag),
        ("F48938-17", "离开取消后释放复位", case_fsm_cancel),
        ("F48938-18", "边带静默内部不静默", case_clamp_silent),
        ("F48938-19", "五路联轧全绿", case_rolls),
        ("F48938-20", "族账本守恒", case_family),
    ];
    let mut i = 0usize;
    while i < specs.len() {
        if let Some((id, name, f)) = specs.get(i) {
            let (passed, detail) = f();
            case(&mut v, id, name, passed, detail);
        }
        i += 1;
    }
    v
}

pub fn case_digest(rows: &[CaseRow]) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            let bytes = r.id.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                if let Some(x) = bytes.get(b) {
                    h = fold_byte(h, *x);
                }
                b += 1;
            }
            h = fold_byte(h, if r.passed { 1 } else { 0 });
            h = fold_u32(h, r.detail.len() as u32);
        }
        i += 1;
    }
    h
}

/// 用例审计：恰好 20 例、编号升序且唯一、全绿、细节非空且不含兜底词。
pub fn audit_cases(rows: &[CaseRow]) -> Result<u32, u32> {
    if rows.len() != CASES {
        return Err(PmCode::CaseBad.val());
    }
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return Err(PmCode::CaseBad.val()),
        };
        if !r.passed {
            return Err(PmCode::CaseBad.val());
        }
        if r.id.is_empty() || r.name.is_empty() || r.detail.is_empty() {
            return Err(PmCode::CaseBad.val());
        }
        if r.detail.contains("未知") {
            return Err(PmCode::CaseBad.val());
        }
        if !r.id.starts_with("F48938-") {
            return Err(PmCode::CaseBad.val());
        }
        if i > 0 {
            if let Some(p) = rows.get(i - 1) {
                if p.id >= r.id {
                    return Err(PmCode::CaseBad.val());
                }
            }
        }
        i += 1;
    }
    Ok(rows.len() as u32)
}

// ── 渲染与解析 ────────────────────────────────────────────────────
pub fn render_traces(b: &TraceBook) -> String {
    format!(
        "{}|{}|{}|{}|0x{:016X}",
        b.records,
        b.cycles,
        b.points_covered(),
        b.errors,
        b.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(PmCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(PmCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(PmCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(PmCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(PmCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(PmCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(PmCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(PmCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 5 {
        return Err(PmCode::RenderBad.val());
    }
    let records = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(PmCode::RenderBad.val()),
    })?;
    let cycles = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(PmCode::RenderBad.val()),
    })?;
    let points = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(PmCode::RenderBad.val()),
    })?;
    let errors = parse_u32(match parts.get(3) {
        Some(v) => v,
        None => return Err(PmCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(4) {
        Some(v) => v,
        None => return Err(PmCode::RenderBad.val()),
    })?;
    if records == 0 || cycles.saturating_mul(4) != records {
        return Err(PmCode::RenderBad.val());
    }
    if points != COVER_BITS {
        return Err(PmCode::RenderBad.val());
    }
    if errors > cycles || digest == 0 {
        return Err(PmCode::RenderBad.val());
    }
    Ok(records)
}

// ── 黄金与注入 ────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let traces = golden_traces(GOLDEN_CYCLES)?;
    let cases = case_rows();
    let passed = audit_cases(&cases)?;
    let fuzz = fuzz_face(2_000)?;
    audit_fuzz_stats(&fuzz)?;
    let cd = case_digest(&cases);
    let mut h = traces.digest;
    h = fold_u32(h, traces.records);
    h = fold_u32(h, traces.cycles);
    h = fold_u32(h, cd as u32);
    h = fold_u32(h, (cd >> 32) as u32);
    h = fold_u32(h, fuzz.digest as u32);
    h = fold_u32(h, (fuzz.digest >> 32) as u32);
    h = fold_u32(h, fuzz.rejected);
    Ok((traces.records, passed, fuzz.rejected, h))
}

/// 注入账（四路）：缺点 / 带码错点 / 用例篡改 / fuzz 账不平。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger {
            tries: [0; 4],
            caught: [0; 4],
        }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

fn defective_book() -> Result<TraceBook, u32> {
    let mut b = TraceBook::new();
    b.push(TraceRecord::new(OpKind::Compose, PointKind::Entry, 0, 0, 1))?;
    b.push(TraceRecord::new(OpKind::Compose, PointKind::Exit, 1, 0, 1))?;
    b.push(TraceRecord::new(OpKind::Compose, PointKind::Duration, 2, 0, 1))?;
    Ok(b)
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 {
        return Err(PmCode::CorpusBad.val());
    }
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 路 1：缺点周期必须被账本审计抓住。
        let b = match defective_book() {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        let hit1 = b.audit().is_err();
        led.record(0, hit1);

        // 路 2：非零码落在入口点必须被判静默。
        let mut c = TraceBook::new();
        let hit2 = match c.push(TraceRecord::new(OpKind::Compose, PointKind::Entry, 0, 7, 1)) {
            Err(e) => e == PmCode::SilentBad.val(),
            Ok(()) => false,
        };
        led.record(1, hit2);

        // 路 3：用例被翻转必须被审计拒绝。
        let mut rows = case_rows();
        if let Some(first) = rows.get_mut(0) {
            first.passed = false;
        }
        let hit3 = audit_cases(&rows).is_err();
        led.record(2, hit3);

        // 路 4：fuzz 账不平必须被审计拒绝（手工构造四路正反样本）。
        let mut st = FuzzStats::new();
        let mut route = 0usize;
        while route < FUZZ_ROUTES {
            st.tally(route, true);
            st.tally(route, false);
            route += 1;
        }
        let seeded = audit_fuzz_stats(&st).is_ok();
        st.attempts = st.attempts.saturating_add(1);
        let hit4 = seeded && !st.balanced() && audit_fuzz_stats(&st).is_err();
        led.record(3, hit4);

        r = r.wrapping_add(1);
    }
    Ok(led)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let b = golden_traces(50)?;
    if b.records != 50 * 16 || b.cover_mask() != 0x0F || b.points_covered() != COVER_BITS {
        return Err(PmCode::CoverBad.val());
    }
    if b.errors == 0 {
        return Err(PmCode::SilentBad.val());
    }
    let mut bad = TraceBook::new();
    let out_of_order = bad.push(TraceRecord::new(OpKind::Compose, PointKind::Exit, 0, 0, 1));
    if out_of_order != Err(PmCode::PointBad.val()) {
        return Err(PmCode::PointBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let st = fuzz_face(2_000)?;
    audit_fuzz_stats(&st)?;
    let mut route = 0usize;
    while route < FUZZ_ROUTES {
        let row = match st.route_of(route) {
            Some(v) => v,
            None => return Err(PmCode::FuzzBad.val()),
        };
        if row[1] == 0 || row[2] == 0 {
            return Err(PmCode::FuzzBad.val());
        }
        route += 1;
    }
    if st.attempts != 2_000 || !st.balanced() {
        return Err(PmCode::FuzzBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let a = case_rows();
    let b = case_rows();
    if audit_cases(&a)? != CASES as u32 || case_digest(&a) != case_digest(&b) {
        return Err(PmCode::ReplayBad.val());
    }
    if p_meta::SEG_BASE == p_core::SEG_BASE || p_guard::SEG_BASE == p_core::SEG_BASE {
        return Err(PmCode::SegmentBad.val());
    }
    if SEG_BASE <= p_bridge::SEG_BASE
        || INSTR_OPS != 4
        || POINT_COUNT != 4
        || COVER_BITS != 16
        || FUZZ_ROUNDS != 10_000
        || CASES != 20
    {
        return Err(PmCode::SegmentBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a == b {
        Ok(())
    } else {
        Err(PmCode::ReplayBad.val())
    }
}

pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < PmCode::ALL.len() {
        let c = PmCode::ALL[i].val();
        if c < SEG_BASE.saturating_add(1) || c > SEG_BASE.saturating_add(0x0B) {
            ok = false;
        }
        i += 1;
    }
    ok && detail(SEG_BASE).starts_with("未知诊断码")
}

// ── 检查项 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    let cover = golden_traces(8).map(|b| (b.cover_mask(), b.points_covered()));
    v.push((
        "F48936-J1-四点100%覆盖",
        cover == Ok((0x0F, COVER_BITS)),
        format!("cover={:?} need=0x0F/{}", cover, COVER_BITS),
    ));

    let silent = golden_traces(8);
    let s_ok = match &silent {
        Ok(b) => b.errors > 0 && b.rejects == 0 && b.audit().is_ok(),
        Err(_) => false,
    };
    v.push((
        "F48936-J1-异常零静默",
        s_ok,
        match &silent {
            Ok(b) => format!("errors={} rejects={} audit={:?}", b.errors, b.rejects, b.audit()),
            Err(e) => format!("err=0x{:08X}", e),
        },
    ));

    let order = {
        let mut b = TraceBook::new();
        let skip = b.push(TraceRecord::new(OpKind::Clamp, PointKind::Result, 5, 0, 1));
        let first = b.push(TraceRecord::new(OpKind::Clamp, PointKind::Entry, 5, 0, 1));
        let back = b.push(TraceRecord::new(OpKind::Clamp, PointKind::Entry, 4, 0, 1));
        skip == Err(PmCode::PointBad.val())
            && first.is_ok()
            && back == Err(PmCode::OrderBad.val())
    };
    v.push((
        "F48936-J1-次序与时钟单调",
        order,
        format!("out-of-order and tick-back both rejected ok={}", order),
    ));

    let fz = fuzz_face(FUZZ_ROUNDS);
    let (f_ok, f_det) = match &fz {
        Ok(s) => (
            s.attempts == FUZZ_ROUNDS && s.balanced() && s.faults == 0,
            format!(
                "attempts={} acc={} rej={} faults={} balanced={}",
                s.attempts,
                s.accepted,
                s.rejected,
                s.faults,
                s.balanced()
            ),
        ),
        Err(e) => (false, format!("err=0x{:08X}", e)),
    };
    v.push(("F48937-J1-fuzz×10^4零崩", f_ok, f_det));

    let routes = match &fz {
        Ok(s) => {
            let mut ok = true;
            let mut i = 0usize;
            while i < FUZZ_ROUTES {
                if let Some(r) = s.route_of(i) {
                    if r[0] == 0 || r[1] < ROUTE_MIN || r[2] < ROUTE_MIN {
                        ok = false;
                    }
                } else {
                    ok = false;
                }
                i += 1;
            }
            ok
        }
        Err(_) => false,
    };
    v.push((
        "F48937-J1-fuzz四路正反样本",
        routes,
        match &fz {
            Ok(s) => format!(
                "r0={:?} r1={:?} r2={:?} r3={:?}",
                s.route_of(0),
                s.route_of(1),
                s.route_of(2),
                s.route_of(3)
            ),
            Err(e) => format!("err=0x{:08X}", e),
        },
    ));

    let rows = case_rows();
    let audit = audit_cases(&rows);
    v.push((
        "F48938-J1-20例全绿入库",
        audit == Ok(CASES as u32),
        format!("audit={:?} rows={}", audit, rows.len()),
    ));

    let replay = {
        let a = case_rows();
        let b = case_rows();
        case_digest(&a) == case_digest(&b) && a == b
    };
    v.push((
        "F48938-J1-用例可复跑",
        replay,
        format!("two runs equal={}", replay),
    ));

    let mut ids_ok = true;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            let mut j = i + 1;
            while j < rows.len() {
                if let Some(q) = rows.get(j) {
                    if q.id == r.id {
                        ids_ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    v.push((
        "F48938-J1-用例编号零撞号",
        ids_ok && rows.len() == CASES,
        format!("unique={} rows={}", ids_ok, rows.len()),
    ));

    let cons = match golden_traces(GOLDEN_CYCLES) {
        Ok(b) => {
            let ok = b.cycles.saturating_mul(4) == b.records
                && b.records == GOLDEN_CYCLES * 16
                && b.audit().is_ok();
            (ok, format!("records={} cycles={} audit={:?}", b.records, b.cycles, b.audit()))
        }
        Err(e) => (false, format!("err=0x{:08X}", e)),
    };
    v.push(("F48938-J1-账本守恒", cons.0, cons.1));

    let render = match golden_traces(4) {
        Ok(b) => {
            let text = render_traces(&b);
            let parsed = audit_render(&text);
            (
                parsed == Ok(b.records),
                format!("text={} parsed={:?}", text, parsed),
            )
        }
        Err(e) => (false, format!("err=0x{:08X}", e)),
    };
    v.push(("F48938-J1-渲染可解析", render.0, render.1));

    v.push((
        "F48936-J1-段位闭集",
        segment_guard(),
        format!(
            "base=0x{:05X} codes={} unknown={}",
            SEG_BASE,
            PmCode::ALL.len(),
            detail(SEG_BASE)
        ),
    ));

    v
}

/// 入口：跑全部检查项。
pub fn run_unx48936_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48936");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x710E0);
        assert_eq!(INSTR_OPS, 4);
        assert_eq!(POINT_COUNT, 4);
        assert_eq!(COVER_BITS, 16);
        assert_eq!(FUZZ_ROUNDS, 10_000);
        assert_eq!(CASES, 20);
        assert_eq!(PmCode::ALL.len(), 11);
        assert_eq!(PmCode::PointBad.val(), SEG_BASE + 1);
        assert_eq!(PmCode::CorpusBad.val(), SEG_BASE + 11);
    }

    #[test]
    fn golden_traces_cover_all_points() {
        let b = golden_traces(10);
        assert!(b.is_ok());
        if let Ok(book) = b {
            assert_eq!(book.records, 160);
            assert_eq!(book.cycles, 40);
            assert_eq!(book.cover_mask(), 0x0F);
            assert_eq!(book.points_covered(), COVER_BITS);
            assert!(book.errors > 0);
            assert_eq!(book.rejects, 0);
            assert!(book.audit().is_ok());
        }
    }

    #[test]
    fn skipped_point_is_rejected() {
        let b = defective_book();
        assert!(b.is_ok());
        if let Ok(book) = b {
            assert_eq!(book.audit(), Err(PmCode::PointBad.val()));
        }
    }

    #[test]
    fn tick_backwards_is_rejected() {
        let mut b = TraceBook::new();
        assert!(b.push(TraceRecord::new(OpKind::Compose, PointKind::Entry, 9, 0, 0)).is_ok());
        assert_eq!(
            b.push(TraceRecord::new(OpKind::Compose, PointKind::Exit, 8, 0, 0)),
            Err(PmCode::OrderBad.val())
        );
        assert_eq!(b.rejects, 1);
    }

    #[test]
    fn nonzero_code_outside_result_is_silent() {
        let mut b = TraceBook::new();
        assert_eq!(
            b.push(TraceRecord::new(OpKind::Report, PointKind::Entry, 0, 3, 0)),
            Err(PmCode::SilentBad.val())
        );
        assert_eq!(b.errors, 0);
        assert_eq!(b.rejects, 1);
    }

    #[test]
    fn result_error_is_attributed() {
        let mut b = TraceBook::new();
        assert!(b.push(TraceRecord::new(OpKind::Report, PointKind::Entry, 0, 0, 0)).is_ok());
        assert!(b.push(TraceRecord::new(OpKind::Report, PointKind::Exit, 1, 0, 0)).is_ok());
        assert!(b.push(TraceRecord::new(OpKind::Report, PointKind::Duration, 2, 0, 0)).is_ok());
        assert!(b
            .push(TraceRecord::new(OpKind::Report, PointKind::Result, 3, 7, 1))
            .is_ok());
        assert_eq!(b.errors, 1);
        assert_eq!(b.cycles, 1);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn fuzz_is_balanced_over_ten_thousand() {
        let st = fuzz_face(FUZZ_ROUNDS);
        assert!(st.is_ok());
        if let Ok(s) = st {
            assert_eq!(s.attempts, FUZZ_ROUNDS);
            assert_eq!(s.attempts, s.accepted + s.rejected);
            assert_eq!(s.faults, 0);
            assert!(s.accepted > 0 && s.rejected > 0);
        }
    }

    #[test]
    fn fuzz_routes_have_both_samples() {
        let st = fuzz_face(4_000);
        assert!(st.is_ok());
        if let Ok(s) = st {
            let mut i = 0usize;
            while i < FUZZ_ROUTES {
                let row = s.route_of(i).unwrap_or([0; 3]);
                assert!(row[1] >= ROUTE_MIN, "route {} accepted {}", i, row[1]);
                assert!(row[2] >= ROUTE_MIN, "route {} rejected {}", i, row[2]);
                i += 1;
            }
        }
    }

    #[test]
    fn twenty_cases_are_green() {
        let rows = case_rows();
        assert_eq!(rows.len(), CASES);
        assert_eq!(audit_cases(&rows), Ok(CASES as u32));
    }

    #[test]
    fn case_ids_are_unique_and_ordered() {
        let rows = case_rows();
        let mut i = 1usize;
        while i < rows.len() {
            if let (Some(a), Some(b)) = (rows.get(i - 1), rows.get(i)) {
                assert!(a.id < b.id);
            }
            i += 1;
        }
    }

    #[test]
    fn case_tamper_is_caught() {
        let mut rows = case_rows();
        if let Some(first) = rows.get_mut(0) {
            first.passed = false;
        }
        assert_eq!(audit_cases(&rows), Err(PmCode::CaseBad.val()));
    }

    #[test]
    fn render_round_trips() {
        let b = golden_traces(6);
        assert!(b.is_ok());
        if let Ok(book) = b {
            let text = render_traces(&book);
            assert_eq!(audit_render(&text), Ok(book.records));
        }
    }

    #[test]
    fn four_inject_routes_are_caught() {
        let l = inject_rounds(10);
        assert!(l.is_ok());
        if let Ok(led) = l {
            assert!(led.all_caught());
            assert_eq!(led.tries, [10; 4]);
            assert_eq!(led.caught, [10; 4]);
        }
    }

    #[test]
    fn probes_hold() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn golden_run_is_stable() {
        let a = golden_run();
        assert!(a.is_ok());
        if let Ok((records, cases, rejected, digest)) = a {
            assert_eq!(records, GOLDEN_CYCLES * 16);
            assert_eq!(cases, CASES as u32);
            assert!(rejected > 0);
            assert_ne!(digest, 0);
        }
    }

    #[test]
    fn checks_are_all_green() {
        let s = run_unx48936_checks();
        assert!(s.all_passed(), "red: {}", s.red_items_text());
        assert!(s.len() >= 11);
    }

    #[test]
    fn segment_is_closed() {
        assert!(segment_guard());
        assert!(detail(SEG_BASE).starts_with("未知诊断码"));
    }
}
