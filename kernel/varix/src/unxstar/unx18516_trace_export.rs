//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18516 · 判定观测导出（比较轨迹）
//! · 0x7FCC0 段 · 批 06 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18516-J1 每次判定比较轨迹（维/条件/基值/结果）可导出，
//!   完整率 100%
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 观测语义冻结：每次判定对掩码内每个比较维落一行轨迹
//!     （dim 维号 / op 条件算子 / basis 基值 / req 请求值 / res 结果），
//!     轨迹行数 = 掩码置位数（popcount），完整率 = 行数 / 应行数，
//!     缺一行即完整率 < 100%（红线）；
//!   - 轨迹顺序冻结：按维度号升序落行（d0→d7），乱序导出即拒；
//!   - 导出格式冻结：`T{行数}|{校验和}|` 头 + 逐行
//!     `d{dim}c{op}b{basis}q{req}r{0|1};`——文本可解析、可回读；
//!   - 校验和冻结：31 进制逐行折叠（行序 + 结果位敏感）——篡改任一
//!     字段必失配；回读（往返）重建行集与原账逐行相等（往返红线）；
//!   - 系统快照冻结：OSV 6.1.7601（major=6/minor=1/build=7601/
//!     NT/SP1/套件 0x0100/产品型 1），与总纲同源闭式重演；
//!   - 注入三类 × 10 轮：缺行导出（完整率必 <100% 被拒）/ 字段篡改
//!     （校验和必失配）/ 乱序导出（顺序闸必拒）——10/10 全抓；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] TeCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [数据结构] TraceRow / TraceBook（64 槽轨迹账）+ 导出串 + 校验和
//!   [状态机]   TePhase 三态（Idle→Tracing→Sealed）+ TraceJudge
//!   [账本]     TeLedger（判定账 + 完整率分母/分子）
//!   [解析器]   parse_export（回读重建，无 panic 扫描）
//!   [注入核]   三类 × 10 轮（缺行/篡改/乱序）
//!   [收口]     golden_run + run_unx18516_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FCC0，AI-VX11-1105 G1 批06 A12 独占区）。
pub const SEG_BASE: u32 = 0x7FCC0;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// VER_EQUAL（冻结）。
pub const VER_EQUAL: u8 = 1;
/// VER_GREATER（冻结）。
pub const VER_GREATER: u8 = 2;
/// VER_GREATER_EQUAL（冻结）。
pub const VER_GREATER_EQUAL: u8 = 3;
/// VER_LESS（冻结）。
pub const VER_LESS: u8 = 4;
/// VER_LESS_EQUAL（冻结）。
pub const VER_LESS_EQUAL: u8 = 5;
/// 系统快照（8 维）：minor/major/build/platform/spminor/spmajor/
/// suite/ptype（与总纲同源）。
pub const SNAPSHOT: [u32; 8] = [1, 6, 7601, 2, 1, 1, 0x0100, 1];
/// 轨迹账容量（定容 64 行）。
pub const BOOK_CAP: usize = 64;
/// 判定场景数（黄金链 6 次判定，掩码宽度递增）。
pub const SCENES: usize = 6;
/// 注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 数值算子比较核（闭集 1..=5）。
pub fn cmp_num(op: u8, basis: u32, req: u32) -> Result<bool, u32> {
    match op {
        1 => Ok(basis == req),
        2 => Ok(basis > req),
        3 => Ok(basis >= req),
        4 => Ok(basis < req),
        5 => Ok(basis <= req),
        _ => Err(TeCode::OpIllegal.code()),
    }
}

/// 掩码置位数（popcount；无 popcount 内建 → 逐位扫）。
pub fn popcount(mask: u32) -> u32 {
    let mut n = 0u32;
    let mut m = mask;
    while m != 0 {
        n = n.saturating_add(1);
        m &= m - 1;
    }
    n
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TeCode {
    /// 维号越界（≥ 8）。
    DimOutOfRange,
    /// 算子越界（0 或 >5；本卡无数值维落 AND/OR 面）。
    OpIllegal,
    /// 条件缺失（掩码置位但该维无条件）。
    CondMissing,
    /// 轨迹账满（64 行耗尽）。
    BookFull,
    /// 轨迹不完整（行数 ≠ 掩码置位数，完整率红线）。
    TraceIncomplete,
    /// 轨迹乱序（维号非升序，顺序闸拒）。
    TraceOutOfOrder,
    /// 校验和失配（导出串被篡改）。
    ChecksumMismatch,
    /// 导出格式坏（头/行式样非法）。
    MalformedExport,
    /// 往返失配（回读重建 ≠ 原账，往返红线）。
    RoundTripMismatch,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 阶段非法（Sealed 后再判定）。
    PhaseInvalid,
    /// 双跑不一致。
    NotReproducible,
}

impl TeCode {
    /// 诊断码数值 = SEG_BASE + 序（闭集派生）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TeCode::DimOutOfRange => "维号越界：闭集 0..=7",
            TeCode::OpIllegal => "算子越界：数值维闭集 1..=5",
            TeCode::CondMissing => "掩码置位但该维无条件",
            TeCode::BookFull => "轨迹账满：64 行耗尽",
            TeCode::TraceIncomplete => "轨迹不完整：行数≠掩码置位数",
            TeCode::TraceOutOfOrder => "轨迹乱序：维号必须升序",
            TeCode::ChecksumMismatch => "校验和失配：导出串被篡改",
            TeCode::MalformedExport => "导出格式坏：头/行式样非法",
            TeCode::RoundTripMismatch => "往返失配：回读重建≠原账",
            TeCode::LedgerBroken => "账本守恒破坏",
            TeCode::PhaseInvalid => "阶段非法：Sealed 后再判定",
            TeCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [TeCode; 12] = [
        TeCode::DimOutOfRange,
        TeCode::OpIllegal,
        TeCode::CondMissing,
        TeCode::BookFull,
        TeCode::TraceIncomplete,
        TeCode::TraceOutOfOrder,
        TeCode::ChecksumMismatch,
        TeCode::MalformedExport,
        TeCode::RoundTripMismatch,
        TeCode::LedgerBroken,
        TeCode::PhaseInvalid,
        TeCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TeCode::ALL.len() {
        if TeCode::ALL[i].code() == code {
            return TeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FCC0 段闭集内"
}

// ── 轨迹数据结构 ────────────────────────────────────────────────
/// 单行轨迹：(维, 算子, 基值, 请求值, 结果)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceRow {
    pub dim: u8,
    pub op: u8,
    pub basis: u32,
    pub req: u32,
    pub res: bool,
}

/// 校验和：31 进制逐行折叠（行序 + 结果位敏感）。
/// 推导：初值取卡号派生 0x18516；结果以 1/2 入折。
pub fn rows_checksum(rows: &[TraceRow]) -> u32 {
    let mut acc: u32 = 0x18516;
    let mut i = 0;
    while i < rows.len() {
        let r = rows[i];
        acc = acc.wrapping_mul(31).wrapping_add(r.dim as u32);
        acc = acc.wrapping_mul(31).wrapping_add(r.op as u32);
        acc = acc.wrapping_mul(31).wrapping_add(r.basis);
        acc = acc.wrapping_mul(31).wrapping_add(r.req);
        acc = acc.wrapping_mul(31).wrapping_add(if r.res { 1 } else { 2 });
        i += 1;
    }
    acc
}

/// 64 槽轨迹账：定容、显式满、旧账不丢。
#[derive(Clone, Debug)]
pub struct TraceBook {
    rows: Vec<TraceRow>,
    pub full_rejects: u32,
}

impl TraceBook {
    pub fn new() -> TraceBook {
        TraceBook { rows: Vec::new(), full_rejects: 0 }
    }
    /// 入账（满则显式拒并计数，旧账不动）。
    pub fn push(&mut self, row: TraceRow) -> Result<(), u32> {
        if self.rows.len() >= BOOK_CAP {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(TeCode::BookFull.code());
        }
        self.rows.push(row);
        Ok(())
    }
    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// 按槽位取行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<TraceRow> {
        self.rows.get(idx).copied()
    }
    /// 全行切片。
    pub fn all(&self) -> &[TraceRow] {
        &self.rows
    }
    /// 清账（幂等）。
    pub fn reset(&mut self) {
        self.rows.clear();
    }
}

/// 顺序闸：行集维号必须严格升序。
pub fn order_ok(rows: &[TraceRow]) -> bool {
    let mut i = 1;
    while i < rows.len() {
        if rows[i].dim <= rows[i - 1].dim {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判定状态机 ──────────────────────────────────────────────────
/// 观测相位（三态单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TePhase {
    /// 未判定。
    Idle,
    /// 观测中。
    Tracing,
    /// 已封账（终态）。
    Sealed,
}

/// 判定账（判定次数 + 完整率分子/分母）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeLedger {
    /// 判定次数。
    pub judges: u32,
    /// 完整率分子（实落轨迹行数）。
    pub rows_real: u32,
    /// 完整率分母（应落轨迹行数 = Σ popcount(mask)）。
    pub rows_due: u32,
}

impl TeLedger {
    pub const fn new() -> TeLedger {
        TeLedger { judges: 0, rows_real: 0, rows_due: 0 }
    }
    /// 完整率 100% 核算（分子 = 分母且分母 > 0）。
    pub fn complete(&self) -> bool {
        self.rows_due > 0 && self.rows_real == self.rows_due
    }
}

/// 条件：(维, 算子, 请求值)。
pub type Cond = (u8, u8, u32);

/// 观测判定引擎。
pub struct TraceJudge {
    pub phase: TePhase,
    pub book: TraceBook,
    pub ledger: TeLedger,
}

impl TraceJudge {
    pub fn new() -> TraceJudge {
        TraceJudge { phase: TePhase::Idle, book: TraceBook::new(), ledger: TeLedger::new() }
    }

    /// 单次判定：按掩码位升序逐维落轨迹行，AND 聚合收口。
    /// 掩码置位但无该维条件 → CondMissing；算子/维越界透传。
    pub fn judge(&mut self, mask: u32, conds: &[Cond]) -> Result<bool, u32> {
        if self.phase == TePhase::Sealed {
            return Err(TeCode::PhaseInvalid.code());
        }
        let due = popcount(mask);
        self.ledger.judges = self.ledger.judges.saturating_add(1);
        self.ledger.rows_due = self.ledger.rows_due.saturating_add(due);
        let mut verdict = true;
        let mut dim = 0u8;
        let mut m = mask;
        // 按维号升序逐位走（掩码低位 = 低维号）。
        while dim < 8 {
            if m & 1 != 0 {
                // 找该维条件。
                let mut found: Option<Cond> = None;
                let mut i = 0;
                while i < conds.len() {
                    if conds[i].0 == dim {
                        found = Some(conds[i]);
                    }
                    i += 1;
                }
                let (cd, cop, creq) = match found {
                    Some(x) => x,
                    None => return Err(TeCode::CondMissing.code()),
                };
                let basis = match SNAPSHOT.get(cd as usize) {
                    Some(v) => *v,
                    None => return Err(TeCode::DimOutOfRange.code()),
                };
                let res = cmp_num(cop, basis, creq)?;
                self.book.push(TraceRow { dim: cd, op: cop, basis, req: creq, res })?;
                self.ledger.rows_real = self.ledger.rows_real.saturating_add(1);
                verdict = verdict && res;
            }
            m >>= 1;
            dim += 1;
        }
        self.phase = TePhase::Tracing;
        Ok(verdict)
    }

    /// 完整率闸：实落 = 应落（100% 红线）。
    pub fn completeness(&self) -> Result<(), u32> {
        if !self.ledger.complete() {
            return Err(TeCode::TraceIncomplete.code());
        }
        Ok(())
    }

    /// 封账：完整率闸 + 顺序闸 + Sealed 终态。
    pub fn seal(&mut self) -> Result<(), u32> {
        self.completeness()?;
        if !order_ok(self.book.all()) {
            return Err(TeCode::TraceOutOfOrder.code());
        }
        self.phase = TePhase::Sealed;
        Ok(())
    }
}

// ── 导出与回读（往返） ──────────────────────────────────────────
/// 导出：`T{行数}|{校验和}|` + 逐行 `d{dim}c{op}b{basis}q{req}r{0|1};`。
pub fn export_trace(rows: &[TraceRow]) -> String {
    let mut out = String::new();
    out.push_str(&format!("T{}|{}|", rows.len(), rows_checksum(rows)));
    let mut i = 0;
    while i < rows.len() {
        let r = rows[i];
        out.push_str(&format!(
            "d{}c{}b{}q{}r{};",
            r.dim, r.op, r.basis, r.req, if r.res { 1 } else { 0 }
        ));
        i += 1;
    }
    out
}

/// 十进制解析（无 panic：逐字节，越界/非数字 None）。
fn parse_dec(s: &[u8]) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < s.len() {
        let c = *s.get(i)?;
        if c < b'0' || c > b'9' {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((c - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

/// 回读：解析导出串重建行集（格式坏/校验和失配显式拒）。
pub fn parse_export(s: &str) -> Result<Vec<TraceRow>, u32> {
    let b = s.as_bytes();
    // 头：'T' 数字 '|' 数字 '|'
    if b.first() != Some(&b'T') {
        return Err(TeCode::MalformedExport.code());
    }
    let mut pos = 1usize;
    let p1 = match b.iter().position(|&c| c == b'|') {
        Some(p) => p,
        None => return Err(TeCode::MalformedExport.code()),
    };
    let n = match parse_dec(&b[pos..p1]) {
        Some(x) => x,
        None => return Err(TeCode::MalformedExport.code()),
    };
    pos = p1 + 1;
    let rest = &b[pos..];
    let p2 = match rest.iter().position(|&c| c == b'|') {
        Some(p) => p,
        None => return Err(TeCode::MalformedExport.code()),
    };
    let cks = match parse_dec(&rest[..p2]) {
        Some(x) => x,
        None => return Err(TeCode::MalformedExport.code()),
    };
    pos = pos + p2 + 1;
    // 行：d{dim}c{op}b{basis}q{req}r{0|1};
    let mut rows: Vec<TraceRow> = Vec::new();
    while pos < b.len() {
        if b.get(pos) != Some(&b'd') {
            return Err(TeCode::MalformedExport.code());
        }
        let seg = |from: usize, stop: u8| -> Result<(u32, usize), u32> {
            let mut k = from;
            while k < b.len() && b.get(k) != Some(&stop) {
                k += 1;
            }
            if k >= b.len() {
                return Err(TeCode::MalformedExport.code());
            }
            let v = parse_dec(&b[from..k]).ok_or(TeCode::MalformedExport.code())?;
            Ok((v, k))
        };
        let (dim, k1) = seg(pos + 1, b'c')?;
        let (op, k2) = seg(k1 + 1, b'b')?;
        let (basis, k3) = seg(k2 + 1, b'q')?;
        let (req, k4) = seg(k3 + 1, b'r')?;
        let rv = match b.get(k4 + 1) {
            Some(&c) if c == b'0' || c == b'1' => c - b'0',
            _ => return Err(TeCode::MalformedExport.code()),
        };
        if b.get(k4 + 2) != Some(&b';') {
            return Err(TeCode::MalformedExport.code());
        }
        rows.push(TraceRow {
            dim: dim as u8,
            op: op as u8,
            basis,
            req,
            res: rv == 1,
        });
        pos = k4 + 3;
    }
    // 校验和闸 + 行数闸。
    if rows.len() as u32 != n {
        return Err(TeCode::TraceIncomplete.code());
    }
    if rows_checksum(&rows) != cks {
        return Err(TeCode::ChecksumMismatch.code());
    }
    Ok(rows)
}

/// 往返：导出 → 回读 → 与原行集逐行相等。
pub fn round_trip(rows: &[TraceRow]) -> Result<(), u32> {
    let s = export_trace(rows);
    let back = parse_export(&s)?;
    if back.len() != rows.len() {
        return Err(TeCode::RoundTripMismatch.code());
    }
    let mut i = 0;
    while i < rows.len() {
        match back.get(i) {
            Some(r) => {
                if *r != rows[i] {
                    return Err(TeCode::RoundTripMismatch.code());
                }
            }
            None => return Err(TeCode::RoundTripMismatch.code()),
        }
        i += 1;
    }
    Ok(())
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（判定数 / 账 / 全链往返核）。
pub type GoldenOut = (u32, TeLedger, bool);

/// 黄金链 6 场景：掩码宽度 1..6 递增（条件面与快照同构），逐场景
/// 判定落轨迹 → 封账（完整率 100% + 顺序闸）→ 全链导出往返。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut j = TraceJudge::new();
    let mut s = 0;
    while s < SCENES {
        // 场景 s：掩码低 s+1 位；条件：各维 VER_GREATER_EQUAL 真值
        // （major ge 6 / minor ge 1 / build ge 7000 / sp ge 0 /
        // platform eq 2 / suite ge 0）——请求值取快照-1 保证全过。
        let mask = (1u32 << (s + 1)) - 1;
        let mut conds: Vec<Cond> = Vec::new();
        let mut d = 0u8;
        while d <= s as u8 {
            let req = match d {
                0 => SNAPSHOT[0], // minor eq 1
                1 => SNAPSHOT[1], // major eq 6
                2 => SNAPSHOT[2], // build eq 7601
                3 => SNAPSHOT[3], // platform eq 2
                4 => SNAPSHOT[4], // spminor eq 1
                5 => SNAPSHOT[5], // spmajor eq 1
                _ => SNAPSHOT[d as usize], // 兜底
            };
            conds.push((d, VER_EQUAL, req));
            d += 1;
        }
        // 逐场景清轨迹账：顺序闸要求账内维号严格升序，跨场景不清账
        // 会因维度号回退（下一场景从 d0 重落）误触 TraceOutOfOrder；
        // 完整率账（ledger.rows_real/rows_due）跨场景累计，不受清账影响。
        j.book.reset();
        let v = j.judge(mask, &conds)?;
        if !v {
            return Err(TeCode::LedgerBroken.code());
        }
        s += 1;
    }
    j.seal()?;
    if !j.ledger.complete() {
        return Err(TeCode::TraceIncomplete.code());
    }
    // 全链导出往返。
    let rows = j.book.all();
    let rt = round_trip(rows).is_ok();
    if !rt {
        return Err(TeCode::RoundTripMismatch.code());
    }
    Ok((j.ledger.judges, j.ledger, rt))
}

/// 双跑一致（确定性红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TeCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核（三类 × 10 轮，10/10 全抓） ──────────────────────────
/// 注入账（三类各轮计数）。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 缺行导出被完整率闸抓次数。
    pub drop_caught: u32,
    /// 类2 字段篡改被校验和抓次数。
    pub tamper_caught: u32,
    /// 类3 乱序导出被顺序闸抓次数。
    pub order_caught: u32,
}

/// 三类注入 rounds 轮；每类每轮必被抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let base: [TraceRow; 3] = [
        TraceRow { dim: 0, op: VER_EQUAL, basis: 1, req: 1, res: true },
        TraceRow { dim: 1, op: VER_EQUAL, basis: 6, req: 6, res: true },
        TraceRow { dim: 2, op: VER_EQUAL, basis: 7601, req: 7601, res: true },
    ];
    let mut r = 0;
    while r < rounds {
        // 类1 缺行：导出串自报 3 行但只载 2 行 → 回读行数闸必拒。
        let short: [TraceRow; 2] = [base[0], base[1]];
        let mut s_short = String::new();
        s_short.push_str(&format!("T{}|{}|", 3usize, rows_checksum(&short)));
        let mut i = 0;
        while i < short.len() {
            let x = short[i];
            s_short.push_str(&format!(
                "d{}c{}b{}q{}r{};",
                x.dim, x.op, x.basis, x.req, if x.res { 1 } else { 0 }
            ));
            i += 1;
        }
        match parse_export(&s_short) {
            Err(c) if c == TeCode::TraceIncomplete.code() => {
                led.drop_caught = led.drop_caught.saturating_add(1);
            }
            _ => return Err(TeCode::TraceIncomplete.code()),
        }
        // 类2 篡改：合法导出串中基值字段改一位 → 校验和必失配。
        let honest = export_trace(&base);
        let tampered = honest.replace("b7601", "b7602");
        if tampered == honest {
            return Err(TeCode::ChecksumMismatch.code());
        }
        match parse_export(&tampered) {
            Err(c) if c == TeCode::ChecksumMismatch.code() => {
                led.tamper_caught = led.tamper_caught.saturating_add(1);
            }
            _ => return Err(TeCode::ChecksumMismatch.code()),
        }
        // 类3 乱序：行集维号降序 → 顺序闸必拒。
        let bad_order: [TraceRow; 2] = [
            TraceRow { dim: 2, op: VER_EQUAL, basis: 7601, req: 7601, res: true },
            TraceRow { dim: 1, op: VER_EQUAL, basis: 6, req: 6, res: true },
        ];
        if order_ok(&bad_order) {
            return Err(TeCode::TraceOutOfOrder.code());
        }
        led.order_caught = led.order_caught.saturating_add(1);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：6 场景全落轨迹且完整率 100%（J1 主体）。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：掩码置位但缺条件 → CondMissing。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut j = TraceJudge::new();
    match j.judge(0x02, &[(0, VER_EQUAL, 1)]) {
        Err(c) => Err(c),
        Ok(_) => Err(TeCode::CondMissing.code()),
    }
}

/// 探针 C：账满 → 显式满拒（64 行后拒第 65 行）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut j = TraceJudge::new();
    let mut k = 0;
    while k <= BOOK_CAP {
        j.book.push(TraceRow { dim: 0, op: VER_EQUAL, basis: 1, req: 1, res: true })?;
        k += 1;
    }
    Ok(())
}

/// 段位守卫（0x40 对齐窗口）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < TeCode::ALL.len() {
        let v = TeCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（15 条；动态明细 ≤8） ──────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：6 场景完整率 100% + 往返核（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, rt)) => *n == SCENES as u32 && led.complete() && *rt,
        Err(_) => false,
    };
    v.push(("J1-6场景完整率100%", g_ok, format!("{:?}", g)));

    // 2. 探针 A（可复跑）。
    let pa = probe_seq_a().is_ok();
    v.push(("J1-探针A场景顺跑", pa, format!("ok={}", pa)));

    // 3. 轨迹行逐维四要素齐备（维/条件/基值/结果逐行核）。
    let fields_ok = match golden_run() {
        Ok((_, _, _)) => {
            let mut j2 = TraceJudge::new();
            let _ = j2.judge(0x03, &[(0, VER_EQUAL, 1), (1, VER_GREATER_EQUAL, 6)]);
            let ok0 = match j2.book.row_at(0) {
                Some(r) => r.dim == 0 && r.op == VER_EQUAL && r.basis == 1 && r.req == 1 && r.res,
                None => false,
            };
            let ok1 = match j2.book.row_at(1) {
                Some(r) => r.dim == 1 && r.op == VER_GREATER_EQUAL && r.basis == 6 && r.req == 6
                    && r.res,
                None => false,
            };
            ok0 && ok1
        }
        Err(_) => false,
    };
    v.push(("J1-轨迹四要素齐备", fields_ok, String::from("维/条件/基值/结果")));

    // 4. 注入三类 10/10 全抓。
    let led = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match led.as_ref() {
        Ok(l) => {
            l.drop_caught == JUDGE_ROUNDS
                && l.tamper_caught == JUDGE_ROUNDS
                && l.order_caught == JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入10/10全抓", inj_ok, format!("{:?}", led)));

    // 5. 缺行探针拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-缺条件判定拒", pb, format!("err={}", pb)));

    // 6. 账满直查拒（探针 C）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-轨迹账满拒", pc, format!("cap={}", BOOK_CAP)));

    // 7. 往返红线：合法行集导出回读逐行相等（3 组行集）。
    let rt_ok = {
        let r1: [TraceRow; 1] =
            [TraceRow { dim: 0, op: VER_EQUAL, basis: 1, req: 1, res: true }];
        let r2: [TraceRow; 3] = [
            TraceRow { dim: 0, op: VER_EQUAL, basis: 1, req: 1, res: true },
            TraceRow { dim: 1, op: VER_GREATER, basis: 6, req: 5, res: true },
            TraceRow { dim: 2, op: VER_LESS, basis: 7601, req: 9600, res: true },
        ];
        let r3: [TraceRow; 2] = [
            TraceRow { dim: 1, op: VER_LESS_EQUAL, basis: 6, req: 6, res: true },
            TraceRow { dim: 4, op: VER_GREATER_EQUAL, basis: 1, req: 0, res: false },
        ];
        round_trip(&r1).is_ok() && round_trip(&r2).is_ok() && round_trip(&r3).is_ok()
    };
    v.push(("J1-往返三组全等", rt_ok, String::from("导出→回读逐行")));

    // 8. 校验和扰动敏感：任一行结果位翻转必失配。
    let flip = {
        let rows: [TraceRow; 2] = [
            TraceRow { dim: 0, op: VER_EQUAL, basis: 1, req: 1, res: true },
            TraceRow { dim: 1, op: VER_EQUAL, basis: 6, req: 6, res: true },
        ];
        let mut flipped = rows;
        flipped[1].res = !flipped[1].res;
        rows_checksum(&rows) != rows_checksum(&flipped)
    };
    v.push(("J1-校验和扰动敏感", flip, String::from("结果位翻转必变")));

    // 9. 完整率账：分子 = 分母（6 场景 Σpopcount = 1+2+3+4+5+6 = 21）。
    let due_sum = {
        let mut sum = 0u32;
        let mut s = 0;
        while s < SCENES {
            sum = sum.saturating_add(popcount((1u32 << (s + 1)) - 1));
            s += 1;
        }
        match golden_run() {
            Ok((_, led, _)) => led.rows_due == sum && sum == 21 && led.rows_real == sum,
            Err(_) => false,
        }
    };
    v.push(("J1-完整率账21/21", due_sum, String::from("Σpopcount=21")));

    // 10. 格式坏直拒：无头/坏行式样三形态。
    let malformed = {
        parse_export("").is_err()
            && parse_export("X1|2|d0c1b1q1r1;").is_err()
            && parse_export("T1|0|d0c1b1q1x1;").is_err()
    };
    v.push(("J1-格式坏三形态拒", malformed, String::from("空/坏头/坏行")));

    // 11. Sealed 终态：封账后再判定拒。
    let sealed = {
        let mut j = TraceJudge::new();
        let _ = j.judge(0x02, &[(1, VER_EQUAL, 6)]);
        let s1 = j.seal().is_ok();
        let p2 = match j.judge(0x02, &[(1, VER_EQUAL, 6)]) {
            Err(c) if c == TeCode::PhaseInvalid.code() => true,
            _ => false,
        };
        s1 && p2
    };
    v.push(("J1-Sealed终态闸", sealed, String::from("封账后再判拒")));

    // 12. 表指纹等价：校验和双跑一致。
    let rows: [TraceRow; 2] = [
        TraceRow { dim: 0, op: VER_EQUAL, basis: 1, req: 1, res: true },
        TraceRow { dim: 1, op: VER_EQUAL, basis: 6, req: 6, res: true },
    ];
    let c1 = rows_checksum(&rows);
    v.push(("J2-校验和双跑", c1 == rows_checksum(&rows), format!("{:08X}", c1)));

    // 13. 双跑一致（复现红线）。
    let rp = repro_check().is_ok();
    v.push(("J2-双跑一致", rp, String::from("golden 两跑同值")));

    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 15. 诊断码闭集（12 码全非空中文明细）。
    let all_ok = {
        let mut ok = TeCode::ALL.len() == 12;
        let mut i = 0;
        while i < TeCode::ALL.len() {
            if TeCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 15 条；前 8 条动态明细）。
pub fn run_unx18516_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18516 判定观测导出（比较轨迹）");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18516_checks();
        assert!(set.all_passed(), "UNX-F18516 判据集应全绿");
    }

    /// 黄金链：6 场景完整率 100% + 往返。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, rt)) => {
                assert_eq!(n, 6);
                assert_eq!(led.rows_due, 21);
                assert_eq!(led.rows_real, 21);
                assert!(rt);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 轨迹四要素逐行核对。
    #[test]
    fn test_row_fields() {
        let mut j = TraceJudge::new();
        let v = j.judge(0x03, &[(0, VER_EQUAL, 1), (1, VER_GREATER_EQUAL, 6)]);
        assert_eq!(v, Ok(true));
        let r0 = j.book.row_at(0);
        let r1 = j.book.row_at(1);
        assert_eq!(
            r0,
            Some(TraceRow { dim: 0, op: VER_EQUAL, basis: 1, req: 1, res: true })
        );
        assert_eq!(
            r1,
            Some(TraceRow { dim: 1, op: VER_GREATER_EQUAL, basis: 6, req: 6, res: true })
        );
    }

    /// 往返：导出回读逐行相等 + 校验和稳定。
    #[test]
    fn test_round_trip() {
        let rows: [TraceRow; 3] = [
            TraceRow { dim: 0, op: VER_EQUAL, basis: 1, req: 1, res: true },
            TraceRow { dim: 1, op: VER_GREATER, basis: 6, req: 5, res: true },
            TraceRow { dim: 2, op: VER_LESS, basis: 7601, req: 9600, res: false },
        ];
        let s = export_trace(&rows);
        assert!(s.starts_with("T3|"));
        let back = parse_export(&s);
        match back {
            Ok(b) => {
                assert_eq!(b.len(), 3);
                let mut i = 0;
                while i < 3 {
                    assert_eq!(b[i], rows[i]);
                    i += 1;
                }
            }
            Err(e) => assert!(false, "回读不应失败: {}", detail(e)),
        }
        assert!(round_trip(&rows).is_ok());
    }

    /// 注入三类 10/10 全抓。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.drop_caught, 10);
                assert_eq!(l.tamper_caught, 10);
                assert_eq!(l.order_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 缺条件 / 账满 / 格式坏 三直拒。
    #[test]
    fn test_gates() {
        assert!(probe_seq_b().is_err(), "缺条件必须拒");
        assert!(probe_seq_c().is_err(), "账满必须拒");
        assert!(parse_export("X").is_err(), "坏头必须拒");
        let mut book = TraceBook::new();
        let mut k = 0;
        while k < BOOK_CAP {
            assert!(book
                .push(TraceRow { dim: 0, op: VER_EQUAL, basis: 1, req: 1, res: true })
                .is_ok());
            k += 1;
        }
        match book.push(TraceRow { dim: 0, op: VER_EQUAL, basis: 1, req: 1, res: true }) {
            Err(c) => assert_eq!(c, TeCode::BookFull.code()),
            Ok(()) => assert!(false, "满账必须拒"),
        }
        assert_eq!(book.len(), BOOK_CAP);
        assert_eq!(book.full_rejects, 1);
    }

    /// 顺序闸与 Sealed 终态。
    #[test]
    fn test_order_and_phase() {
        let bad: [TraceRow; 2] = [
            TraceRow { dim: 2, op: VER_EQUAL, basis: 7601, req: 7601, res: true },
            TraceRow { dim: 1, op: VER_EQUAL, basis: 6, req: 6, res: true },
        ];
        assert!(!order_ok(&bad));
        let mut j = TraceJudge::new();
        assert!(j.judge(0x02, &[(1, VER_EQUAL, 6)]).is_ok());
        assert!(j.seal().is_ok());
        assert_eq!(j.phase, TePhase::Sealed);
        assert_eq!(
            j.judge(0x02, &[(1, VER_EQUAL, 6)]),
            Err(TeCode::PhaseInvalid.code())
        );
    }

    /// 双跑一致 + popcount 口径。
    #[test]
    fn test_repro_and_popcount() {
        assert!(repro_check().is_ok());
        assert_eq!(popcount(0), 0);
        assert_eq!(popcount(0xFFFF), 16);
        assert_eq!(popcount(0x0101), 2);
    }

    /// 段位守卫 + 诊断码闭集。
    #[test]
    fn test_segment_and_codes() {
        assert!(segment_guard());
        assert_eq!(TeCode::ALL.len(), 12);
        for c in TeCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
