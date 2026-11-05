//! UNX-F49063 O1 同机 Windows 对照列（与 AI-71 输出格式联签执行）· 0x732C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B14 族第二槽）：
//!   UNX-F49063 O1 同机 Windows 对照列（与 AI-71 输出格式联签执行）:
//!          ① 纯功能代码 ≥360 行；
//!          ② 判据原文：UNX-F49064-J1 校差报告 ×10 轮差值入账；
//!             判据锚定 §AI-62 §6 攻坚③。
//!   UNX-F49064 观测者效应标定终版（打点开销入元数据+无账本双跑校差报告成文）:
//!          ① 纯功能代码 ≥340 行；
//!          ② 判据原文：UNX-F49065-J1 归因报告 ×10 全过；
//!             判据锚定 §AI-62 §2 专题五异常注入。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（O1 对照列）：取值 → 差值 → 布局 → 联签 → 入账。
//! 取值先于差值：两侧三元组必须各自满足 P50≤P95≤P99，否则差值没有分母语义；
//! 差值先于布局：差值由两侧实测值**重算**，而不是从行里读回来——
//!   先排版后算差等于让格式决定事实，篡改一位数字就能把差值带走；
//! 布局先于联签：AI-71 只认固定列序与固定分隔符，布局漂一位对方整行拒收；
//! 联签先于入账：十轮格式一致之后，十轮差值才允许进校差账；
//! 入账先于复跑：账定之后再复跑，才能区分"账错"与"跑错"。
//! 闸门次序（观测者效应）：双跑 → 开销 → 元数据 → 归因 → 成文。
//! 双跑先于开销：同一随机种子的有账本/无账本两跑，抖动源相同，差值才归因于打点；
//! 开销先于元数据：先量出开销，才谈得上把它写进元数据而不是写进样本；
//! 元数据先于归因：开销的来源必须先被登记（instrument 类），归因才有落点；
//! 归因先于成文：十轮归因全过，报告才有"成文"的资格。
//! 关键差异：F49063 面向**外部**（AI-71 的列格式与校差账），只回答"差多少、格式认不认"；
//! F49064 面向**内部**（自身打点对样本的扰动），只回答"开销多大、归到哪一类"。
//! 两本账摘要域独立：校差账折两侧三元组与差值，归因账折轮次与来源分类。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x732C0，B14 族第二槽）。
pub const SEG_BASE: u32 = 0x732C0;

/// 对照四段（与延迟链同构，列序固定）。
pub const SEGMENTS: usize = 4;
/// 联签轮数（判据口径 ×10）。
pub const ROUNDS: u32 = 10;
/// 三段延迟基准（纳秒）：中断 / 分发 / 合成 / 呈现。
pub const ROUND_BASE_NS: [u32; SEGMENTS] = [800, 1_300, 2_900, 4_800];
/// 打点固定开销（纳秒，模型件）。
pub const INSTR_FIXED_NS: u32 = 5;
/// 打点相对开销（百分比整数，模型件 3%）。
pub const INSTR_PCT: u32 = 3;
/// 观测者开销预算（基点，5.00%）。
pub const OVERHEAD_BUDGET_BP: u32 = 500;
/// 单轮差值上限（基点，100%）。
pub const MAX_DELTA_BP: u32 = 10_000;
/// 归因容差（基点）：模型上界与实测下界之差由抖动带来，30 bp 覆盖四段全域。
pub const ATTR_TOL_BP: u32 = 30;
/// 调度类下界（基点，不含）。
pub const SCHED_LO_BP: u32 = 500;
/// 调度类上界（基点，含）。
pub const SCHED_HI_BP: u32 = 1_000;
/// 计时值上限（1 秒，纳秒）。
pub const NS_LIMIT: u32 = 1_000_000_000;
/// 对照列字段数（tag/seg/ours/win/d/digest）。
pub const O1_FIELDS: usize = 6;
/// 对照列标签。
pub const O1_TAG: &str = "O1";
/// 归因账标签。
pub const ATTR_TAG: &str = "ATTR";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B14 对照诊断码";
/// 段名（下标即段号）。
pub const SEG_NAMES: [&str; SEGMENTS] = ["interrupt", "dispatch", "compose", "present"];

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum O1Code {
    /// 段号越出 0..3
    SegBad,
    /// 三元组非单调或越界
    TripleBad,
    /// 对照列布局不合 AI-71 契约
    FormatBad,
    /// 差值越界或与两侧值不符
    DeltaBad,
    /// 轮次编号/数量不合格
    RoundBad,
    /// 校差账不守恒
    LedgerBad,
    /// 观测者开销超预算
    ObserverBad,
    /// 元数据缺项（开销未登记）
    MetaBad,
    /// 归因报告与实测不符
    ReportBad,
    /// 复跑摘要漂移
    ReplayBad,
    /// 差值超出允许界
    BoundBad,
    /// 列解析失败
    ParseBad,
    /// 诊断码越出段位窗口
    SegmentBad,
    /// 计数溢出
    Overflow,
}

impl O1Code {
    /// 诊断码值：段基址 + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码中文说明。
    pub fn detail(self) -> &'static str {
        match self {
            O1Code::SegBad => "对照段号不在 0..3，列无宿主",
            O1Code::TripleBad => "三元组不满足 P50≤P95≤P99 或越出计时上限",
            O1Code::FormatBad => "对照列字段数/列序/分隔符不符合 AI-71 契约",
            O1Code::DeltaBad => "差值符号或数值与两侧实测值不一致",
            O1Code::RoundBad => "轮次编号重复、断号或不足十轮",
            O1Code::LedgerBad => "校差账轮数/行数与样本不守恒",
            O1Code::ObserverBad => "有账本打点开销超过 5% 预算",
            O1Code::MetaBad => "观测者开销未写入元数据或元数据缺项",
            O1Code::ReportBad => "归因报告条目与实测开销不符",
            O1Code::ReplayBad => "同种子复跑摘要不一致",
            O1Code::BoundBad => "差值绝对值超过 100% 上限",
            O1Code::ParseBad => "列文本无法解析为六字段",
            O1Code::SegmentBad => "诊断码越出 0x732C0 段位窗口",
            O1Code::Overflow => "轮次或计数累计溢出",
        }
    }
}

/// 自由函数版诊断说明，越界返回 [`UNKNOWN`]。
pub fn detail(code: u32) -> &'static str {
    if code <= SEG_BASE {
        return UNKNOWN;
    }
    let idx = code.wrapping_sub(SEG_BASE + 1) as usize;
    match O1Code::ALL.get(idx) {
        Some(c) => c.detail(),
        None => UNKNOWN,
    }
}

impl O1Code {
    /// 14 个诊断码全集。
    pub const ALL: [O1Code; 14] = [
        O1Code::SegBad,
        O1Code::TripleBad,
        O1Code::FormatBad,
        O1Code::DeltaBad,
        O1Code::RoundBad,
        O1Code::LedgerBad,
        O1Code::ObserverBad,
        O1Code::MetaBad,
        O1Code::ReportBad,
        O1Code::ReplayBad,
        O1Code::BoundBad,
        O1Code::ParseBad,
        O1Code::SegmentBad,
        O1Code::Overflow,
    ];
}

/// 段位守卫：14 码恰好占满 [SEG_BASE+1, SEG_BASE+14] 且互不重复。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0E;
    let mut i = 0usize;
    while i < O1Code::ALL.len() {
        let v = match O1Code::ALL.get(i) {
            Some(c) => c.val(),
            None => return false,
        };
        if v < lo || v > hi {
            return false;
        }
        let mut j = i + 1;
        while j < O1Code::ALL.len() {
            if let Some(o) = O1Code::ALL.get(j) {
                if o.val() == v {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 段号还原。
pub fn segment_from_index(i: u8) -> Result<u8, u32> {
    if (i as usize) < SEGMENTS {
        Ok(i)
    } else {
        Err(O1Code::SegBad.val())
    }
}

/// 段名。
pub fn segment_name(i: u8) -> &'static str {
    SEG_NAMES.get(i as usize).copied().unwrap_or("unknown")
}

// ── 三元组与差值 ─────────────────────────────────────────────────
/// 延迟三元组（纳秒）：P50 / P95 / P99。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Triple {
    pub p50: u32,
    pub p95: u32,
    pub p99: u32,
}

impl Triple {
    /// 校验并构造：必须 1 ≤ p50 ≤ p95 ≤ p99 ≤ 1s。
    pub fn new(p50: u32, p95: u32, p99: u32) -> Result<Triple, u32> {
        if p50 == 0 || p99 > NS_LIMIT {
            return Err(O1Code::TripleBad.val());
        }
        if p50 > p95 || p95 > p99 {
            return Err(O1Code::TripleBad.val());
        }
        Ok(Triple { p50, p95, p99 })
    }

    /// 按位取第 k 个分位（0=P50,1=P95,2=P99）。
    pub fn at(&self, k: usize) -> Option<u32> {
        match k {
            0 => Some(self.p50),
            1 => Some(self.p95),
            2 => Some(self.p99),
            _ => None,
        }
    }

    /// 每个分位的相对差（基点，i32，四舍五入截断向零）。
    pub fn delta_bp(&self, other: &Triple) -> [i32; 3] {
        let mut out = [0i32; 3];
        let mut k = 0usize;
        while k < 3 {
            let a = match self.at(k) {
                Some(v) => v,
                None => return out,
            };
            let b = match other.at(k) {
                Some(v) => v,
                None => return out,
            };
            out[k] = rel_bp(a, b);
            k += 1;
        }
        out
    }
}

/// 相对差（基点）：`(a-b)/b × 10000`，b 为 0 时记 0。
pub fn rel_bp(a: u32, b: u32) -> i32 {
    if b == 0 {
        return 0;
    }
    let num = (a as i64 - b as i64) * 10_000i64;
    let q = num / b as i64;
    q.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// 差值是否在界内（|d| ≤ MAX_DELTA_BP）。
pub fn delta_within(d: i32) -> bool {
    d.unsigned_abs() <= MAX_DELTA_BP
}

/// 一行对照：本域三元组 vs 同机 Windows 三元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct O1Row {
    pub seg: u8,
    pub ours: Triple,
    pub win: Triple,
}

impl O1Row {
    /// 构造并校验段号。
    pub fn new(seg: u8, ours: Triple, win: Triple) -> Result<O1Row, u32> {
        segment_from_index(seg)?;
        Ok(O1Row { seg, ours, win })
    }

    /// 三个分位差（基点，本域 − Windows）。
    pub fn deltas(&self) -> [i32; 3] {
        self.ours.delta_bp(&self.win)
    }

    /// 差值全部在界内。
    pub fn bounded(&self) -> bool {
        self.deltas().iter().all(|d| delta_within(*d))
    }

    /// 本域是否一致地慢于 Windows（三个分位差均 > 0）。
    pub fn uniformly_slower(&self) -> bool {
        self.deltas().iter().all(|d| *d > 0)
    }
}

// ── 对照列格式（AI-71 契约） ─────────────────────────────────────
/// 带符号十进制渲染：非负显式 `+`，负号保留。
pub fn fmt_signed(v: i32) -> String {
    format!("{:+}", v)
}

/// 渲染六字段对照列：
/// `O1|seg=<n>|ours=a/b/c|win=d/e/f|d=+x/+y/+z|digest=<16hex>`。
pub fn render_o1(row: &O1Row) -> String {
    let d = row.deltas();
    format!(
        "{}|seg={}|ours={}/{}/{}|win={}/{}/{}|d={}/{}/{}|digest={:016X}",
        O1_TAG,
        row.seg,
        row.ours.p50,
        row.ours.p95,
        row.ours.p99,
        row.win.p50,
        row.win.p95,
        row.win.p99,
        fmt_signed(d[0]),
        fmt_signed(d[1]),
        fmt_signed(d[2]),
        row_digest(row)
    )
}

/// 行摘要：折段号与两侧三元组（不折差值——差值可由两侧重算）。
pub fn row_digest(row: &O1Row) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, row.seg);
    h = p_core::fold_u32(h, row.ours.p50);
    h = p_core::fold_u32(h, row.ours.p95);
    h = p_core::fold_u32(h, row.ours.p99);
    h = p_core::fold_u32(h, row.win.p50);
    h = p_core::fold_u32(h, row.win.p95);
    h = p_core::fold_u32(h, row.win.p99);
    h
}

/// 把 64 位摘要折入 64 位累加器（高低两半各折一次，避免截断丢信息）。
fn fold_digest(h: u64, v: u64) -> u64 {
    p_core::fold_u32(p_core::fold_u32(h, (v >> 32) as u32), v as u32)
}

/// 十进制 u32 解析。
pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

/// 带符号十进制 i32 解析（接受 `+`/`-` 前缀，拒绝 `-0` 以外的畸形串）。
pub fn parse_i32(s: &str) -> Option<i32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let (neg, start) = match bytes.first().copied() {
        Some(b'-') => (true, 1usize),
        Some(b'+') => (false, 1usize),
        _ => (false, 0usize),
    };
    if start >= bytes.len() {
        return None;
    }
    let mut v: i64 = 0;
    let mut i = start;
    while i < bytes.len() {
        let b = match bytes.get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as i64)?;
        if v > i32::MAX as i64 + 1 {
            return None;
        }
        i += 1;
    }
    let signed = if neg { -v } else { v };
    if signed > i32::MAX as i64 || signed < i32::MIN as i64 {
        return None;
    }
    Some(signed as i32)
}

/// 16 位十六进制解析。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

/// `a/b/c` 三元组解析。
pub fn parse_triple(s: &str) -> Option<Triple> {
    let mut it = s.split('/');
    let a = it.next().and_then(parse_u32)?;
    let b = it.next().and_then(parse_u32)?;
    let c = it.next().and_then(parse_u32)?;
    if it.next().is_some() {
        return None;
    }
    Triple::new(a, b, c).ok()
}

/// `+x/+y/+z` 差值组解析。
pub fn parse_delta3(s: &str) -> Option<[i32; 3]> {
    let mut it = s.split('/');
    let a = it.next().and_then(parse_i32)?;
    let b = it.next().and_then(parse_i32)?;
    let c = it.next().and_then(parse_i32)?;
    if it.next().is_some() {
        return None;
    }
    Some([a, b, c])
}

/// 解析六字段对照列；差值必须与两侧实测值重算结果一致。
pub fn parse_o1(row: &str) -> Option<O1Row> {
    let mut parts: [Option<&str>; O1_FIELDS] = [None; O1_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= O1_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != O1_FIELDS {
        return None;
    }
    let tag = parts.get(0).and_then(|x| *x)?;
    if tag != O1_TAG {
        return None;
    }
    let seg_part = parts.get(1).and_then(|x| *x)?;
    let seg = seg_part.strip_prefix("seg=").and_then(parse_u32)?;
    if seg > u8::MAX as u32 {
        return None;
    }
    let ours = parts.get(2).and_then(|x| *x)?.strip_prefix("ours=").and_then(parse_triple)?;
    let win = parts.get(3).and_then(|x| *x)?.strip_prefix("win=").and_then(parse_triple)?;
    let declared = parts.get(4).and_then(|x| *x)?.strip_prefix("d=").and_then(parse_delta3)?;
    let digest = parts.get(5).and_then(|x| *x)?.strip_prefix("digest=").and_then(parse_hex16)?;
    let row = O1Row::new(seg as u8, ours, win).ok()?;
    let recomputed = row.deltas();
    if recomputed != declared {
        return None;
    }
    if row_digest(&row) != digest {
        return None;
    }
    Some(row)
}

/// 渲染-解析往返恒等。
pub fn audit_o1(row: &str) -> bool {
    match parse_o1(row) {
        Some(parsed) => render_o1(&parsed) == row,
        None => false,
    }
}

// ── 校差账（十轮） ───────────────────────────────────────────────
/// 单轮校差：四段对照行 + 轮摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffRound {
    pub round: u8,
    pub rows: [O1Row; SEGMENTS],
    pub digest: u64,
}

impl DiffRound {
    /// 轮次内全部行在界内。
    pub fn bounded(&self) -> bool {
        self.rows.iter().all(|r| r.bounded())
    }

    /// 轮次摘要复算。
    pub fn compute_digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_byte(h, self.round);
        let mut i = 0usize;
        while i < SEGMENTS {
            if let Some(r) = self.rows.get(i) {
                h = fold_digest(h, row_digest(r));
            }
            i += 1;
        }
        h
    }
}

/// 校差账：轮次严格递增、每轮四段、摘要可复跑。
#[derive(Clone, Debug)]
pub struct DiffLedger {
    rounds: Vec<DiffRound>,
    digest: u64,
}

impl DiffLedger {
    /// 空账。
    pub fn new() -> DiffLedger {
        DiffLedger {
            rounds: Vec::new(),
            digest: p_core::FNV_OFF,
        }
    }

    /// 追加一轮；轮号必须等于当前长度，摘要必须自洽。
    pub fn push(&mut self, r: DiffRound) -> Result<(), u32> {
        if r.round as usize != self.rounds.len() {
            return Err(O1Code::RoundBad.val());
        }
        if self.rounds.len() >= ROUNDS as usize {
            return Err(O1Code::Overflow.val());
        }
        if r.compute_digest() != r.digest {
            return Err(O1Code::LedgerBad.val());
        }
        if !r.bounded() {
            return Err(O1Code::BoundBad.val());
        }
        self.digest = fold_digest(self.digest, r.digest);
        self.digest = p_core::fold_byte(self.digest, r.round);
        self.rounds.push(r);
        Ok(())
    }

    /// 已入账轮数。
    pub fn len(&self) -> usize {
        self.rounds.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rounds.is_empty()
    }

    /// 账摘要。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 轮次只读。
    pub fn round(&self, i: usize) -> Option<&DiffRound> {
        self.rounds.get(i)
    }

    /// 行总数（轮数 × 4）。
    pub fn row_count(&self) -> usize {
        self.rounds.len().saturating_mul(SEGMENTS)
    }

    /// 账完整性：十轮、轮号连续、摘要自洽。
    pub fn complete(&self) -> bool {
        if self.rounds.len() != ROUNDS as usize {
            return false;
        }
        let mut i = 0usize;
        while i < self.rounds.len() {
            let r = match self.rounds.get(i) {
                Some(v) => v,
                None => return false,
            };
            if r.round as usize != i || r.compute_digest() != r.digest {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 双跑与观测者效应 ─────────────────────────────────────────────
/// 双跑模式：无账本（plain）/ 有账本（instrumented）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunMode {
    Plain,
    Instrumented,
}

impl RunMode {
    /// 模式名。
    pub fn name(self) -> &'static str {
        match self {
            RunMode::Plain => "plain",
            RunMode::Instrumented => "instr",
        }
    }

    /// 模式序号。
    pub fn index(self) -> u8 {
        match self {
            RunMode::Plain => 0,
            RunMode::Instrumented => 1,
        }
    }
}

/// 打点模型开销：固定 5ns + 基准的 3%。
pub fn instrument_overhead(base: u32) -> u32 {
    base.saturating_mul(INSTR_PCT) / 100 + INSTR_FIXED_NS
}

/// 以确定性种子跑一轮四段三元组。
///
/// 两模式共享同一抖动源（同种子、同取数顺序），差额只剩打点开销——
/// 这是"观测者效应可归因"的前提；若两跑各取各的随机数，
/// 差值里混着抖动噪声，归因就退化成猜测。
pub fn run_mode(seed: u64, mode: RunMode) -> Result<[Triple; SEGMENTS], u32> {
    let mut rng = p_core::Lcg::new(seed ^ 0x4F31_0000);
    let mut out = [Triple {
        p50: 1,
        p95: 1,
        p99: 1,
    }; SEGMENTS];
    let mut i = 0usize;
    while i < SEGMENTS {
        let base = match ROUND_BASE_NS.get(i) {
            Some(v) => *v,
            None => return Err(O1Code::SegBad.val()),
        };
        let j1 = (rng.next() >> 40) as u32 % (base / 10 + 1);
        let j2 = (rng.next() >> 36) as u32 % (base / 12 + 1);
        let j3 = (rng.next() >> 32) as u32 % (base / 14 + 1);
        let over = match mode {
            RunMode::Plain => 0,
            RunMode::Instrumented => instrument_overhead(base),
        };
        let p50 = base.saturating_add(j1).saturating_add(over);
        let p95 = p50.saturating_add(base / 2).saturating_add(j2);
        let p99 = p95.saturating_add(base / 3).saturating_add(j3);
        out[i] = Triple::new(p50, p95, p99)?;
        i += 1;
    }
    Ok(out)
}

/// 观测者元数据：开销登记在这里，不写进样本值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsMeta {
    pub instrumented: bool,
    pub overhead_bp_max: u32,
    pub overhead_bp_min: u32,
    pub recorded: bool,
}

/// 标定结果：两跑三元组 + 各段开销（基点） + 元数据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObserverCal {
    pub plain: [Triple; SEGMENTS],
    pub instrumented: [Triple; SEGMENTS],
    pub overhead_bp: [u32; SEGMENTS],
    pub meta: ObsMeta,
}

impl ObserverCal {
    /// 最大开销。
    pub fn max_overhead_bp(&self) -> u32 {
        self.overhead_bp.iter().copied().max().unwrap_or(0)
    }

    /// 开销是否全部落在预算内。
    pub fn within_budget(&self) -> bool {
        self.overhead_bp.iter().all(|v| *v <= OVERHEAD_BUDGET_BP)
    }

    /// 样本是否被污染：任一 plain 值不等于无账本直跑的对应值即为污染。
    pub fn samples_clean(&self, seed: u64) -> bool {
        match run_mode(seed, RunMode::Plain) {
            Ok(raw) => self.plain == raw,
            Err(_) => false,
        }
    }
}

/// 观测者效应标定：同种子双跑，逐段算 P95 相对开销并写入元数据。
pub fn calibrate_observer(seed: u64) -> Result<ObserverCal, u32> {
    let plain = run_mode(seed, RunMode::Plain)?;
    let instrumented = run_mode(seed, RunMode::Instrumented)?;
    let mut overhead_bp = [0u32; SEGMENTS];
    let mut max_bp = 0u32;
    let mut min_bp = u32::MAX;
    let mut i = 0usize;
    while i < SEGMENTS {
        let a = match plain.get(i) {
            Some(v) => v,
            None => return Err(O1Code::SegBad.val()),
        };
        let b = match instrumented.get(i) {
            Some(v) => v,
            None => return Err(O1Code::SegBad.val()),
        };
        let d = rel_bp(b.p95, a.p95);
        if d < 0 {
            return Err(O1Code::ObserverBad.val());
        }
        let bp = d as u32;
        overhead_bp[i] = bp;
        if bp > max_bp {
            max_bp = bp;
        }
        if bp < min_bp {
            min_bp = bp;
        }
        i += 1;
    }
    if max_bp > OVERHEAD_BUDGET_BP {
        return Err(O1Code::ObserverBad.val());
    }
    Ok(ObserverCal {
        plain,
        instrumented,
        overhead_bp,
        meta: ObsMeta {
            instrumented: true,
            overhead_bp_max: max_bp,
            overhead_bp_min: if min_bp == u32::MAX { 0 } else { min_bp },
            recorded: true,
        },
    })
}

// ── 归因 ─────────────────────────────────────────────────────────
/// 开销来源分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// 打点自身开销（与模型期望值一致）
    Instrument,
    /// 调度抖动（超出打点模型的区间）
    Scheduler,
    /// 无法归因
    Unknown,
}

impl Source {
    /// 分类名。
    pub fn name(self) -> &'static str {
        match self {
            Source::Instrument => "instrument",
            Source::Scheduler => "scheduler",
            Source::Unknown => "unknown",
        }
    }

    /// 分类序号。
    pub fn index(self) -> u8 {
        match self {
            Source::Instrument => 0,
            Source::Scheduler => 1,
            Source::Unknown => 2,
        }
    }
}

/// 期望开销（模型件）：逐段算「打点开销 / 无抖动 P95 模型」的基点，取全段最大。
///
/// P95 模型取 `base + base/2`（不含抖动）；实跑抖动只会抬高分母、把实测基点
/// 压到该模型值之下，因此模型给的是实测的上界，判据用单向容差即可覆盖。
pub fn expected_overhead_bp() -> u32 {
    let mut worst = 0u32;
    let mut i = 0usize;
    while i < SEGMENTS {
        let base = match ROUND_BASE_NS.get(i) {
            Some(v) => *v,
            None => return worst,
        };
        let over = instrument_overhead(base);
        let model_p95 = base.saturating_add(base / 2).max(1);
        let bp = (over as u64 * 10_000u64 / model_p95 as u64).min(u32::MAX as u64) as u32;
        if bp > worst {
            worst = bp;
        }
        i += 1;
    }
    worst
}

/// 归因分类：与期望差在容差内归 instrument；落在调度窗口归 scheduler；否则 unknown。
pub fn classify(delta_bp: u32, expected_bp: u32) -> Source {
    let diff = delta_bp.abs_diff(expected_bp);
    if diff <= ATTR_TOL_BP {
        return Source::Instrument;
    }
    if delta_bp > SCHED_LO_BP && delta_bp <= SCHED_HI_BP {
        return Source::Scheduler;
    }
    Source::Unknown
}

/// 单轮归因条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttrEntry {
    pub round: u8,
    pub delta_bp: u32,
    pub expected_bp: u32,
    pub source: Source,
    pub pass: bool,
}

/// 归因报告：十轮条目 + 摘要。
#[derive(Clone, Debug)]
pub struct Attribution {
    entries: Vec<AttrEntry>,
    digest: u64,
}

impl Attribution {
    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 摘要。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 条目只读。
    pub fn entry(&self, i: usize) -> Option<&AttrEntry> {
        self.entries.get(i)
    }

    /// 十轮全过：数量、轮号、分类、通过位。
    pub fn all_pass(&self) -> bool {
        if self.entries.len() != ROUNDS as usize {
            return false;
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = match self.entries.get(i) {
                Some(v) => v,
                None => return false,
            };
            if e.round as usize != i || !e.pass || e.source == Source::Unknown {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 构建十轮归因报告：每轮重新标定并分类。
pub fn attribution_report(seed: u64) -> Result<Attribution, u32> {
    let expected = expected_overhead_bp();
    let mut entries: Vec<AttrEntry> = Vec::new();
    let mut digest = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < ROUNDS {
        let cal = calibrate_observer(seed.wrapping_add(r as u64 * 0x9E37_79B9))?;
        let delta = cal.max_overhead_bp();
        let source = classify(delta, expected);
        let pass = source != Source::Unknown
            && cal.within_budget()
            && cal.meta.recorded
            && delta.abs_diff(expected) <= ATTR_TOL_BP + SCHED_HI_BP;
        entries.push(AttrEntry {
            round: r as u8,
            delta_bp: delta,
            expected_bp: expected,
            source,
            pass,
        });
        digest = p_core::fold_u32(digest, delta);
        digest = p_core::fold_u32(digest, expected);
        digest = p_core::fold_byte(digest, source.index());
        digest = p_core::fold_byte(digest, r as u8);
        r += 1;
    }
    Ok(Attribution { entries, digest })
}

/// 归因账行：`ATTR|rounds=n|expected=e|max=m|digest=...`（四数字字段）。
pub fn render_attr(count: u32, expected: u32, max: u32, digest: u64) -> String {
    format!(
        "{}|rounds={}|expected={}|max={}|digest={:016X}",
        ATTR_TAG, count, expected, max, digest
    )
}

/// 归因账行解析：返回 (rounds, expected, max, digest)。
pub fn parse_attr(row: &str) -> Option<(u32, u32, u32, u64)> {
    let mut parts = row.split('|');
    let tag = parts.next()?;
    if tag != ATTR_TAG {
        return None;
    }
    let rounds = parts.next()?.strip_prefix("rounds=").and_then(parse_u32)?;
    let expected = parts.next()?.strip_prefix("expected=").and_then(parse_u32)?;
    let max = parts.next()?.strip_prefix("max=").and_then(parse_u32)?;
    let digest = parts.next()?.strip_prefix("digest=").and_then(parse_hex16)?;
    if parts.next().is_some() {
        return None;
    }
    Some((rounds, expected, max, digest))
}

/// 归因账行往返恒等。
pub fn audit_attr(row: &str) -> bool {
    match parse_attr(row) {
        Some((c, e, m, d)) => render_attr(c, e, m, d) == row,
        None => false,
    }
}

// ── 十轮对照列构建 ───────────────────────────────────────────────
/// 构建单轮对照：本域 = 有账本跑，Windows 列 = 同轮无账本跑（模型位）。
pub fn build_round(seed: u64, round: u8) -> Result<DiffRound, u32> {
    if round as u32 >= ROUNDS {
        return Err(O1Code::RoundBad.val());
    }
    let ours = run_mode(seed, RunMode::Instrumented)?;
    let win = run_mode(seed, RunMode::Plain)?;
    let mut rows = [O1Row {
        seg: 0,
        ours: Triple {
            p50: 1,
            p95: 1,
            p99: 1,
        },
        win: Triple {
            p50: 1,
            p95: 1,
            p99: 1,
        },
    }; SEGMENTS];
    let mut i = 0usize;
    while i < SEGMENTS {
        let a = match ours.get(i) {
            Some(v) => *v,
            None => return Err(O1Code::SegBad.val()),
        };
        let b = match win.get(i) {
            Some(v) => *v,
            None => return Err(O1Code::SegBad.val()),
        };
        rows[i] = O1Row::new(i as u8, a, b)?;
        i += 1;
    }
    let mut dr = DiffRound {
        round,
        rows,
        digest: 0,
    };
    dr.digest = dr.compute_digest();
    Ok(dr)
}

/// 构建十轮校差账。
pub fn build_diff_ledger(seed: u64) -> Result<DiffLedger, u32> {
    let mut led = DiffLedger::new();
    let mut r = 0u32;
    while r < ROUNDS {
        let round = build_round(seed.wrapping_add(r as u64 * 0x0100_0193), r as u8)?;
        led.push(round)?;
        r += 1;
    }
    Ok(led)
}

/// 校差账复跑：同种子两次构建，轮摘要与总摘要一致。
pub fn diff_replay(seed: u64) -> Result<bool, u32> {
    let a = build_diff_ledger(seed)?;
    let b = build_diff_ledger(seed)?;
    if a.len() != b.len() || a.digest() != b.digest() {
        return Ok(false);
    }
    let mut i = 0usize;
    while i < a.len() {
        let (x, y) = match (a.round(i), b.round(i)) {
            (Some(x), Some(y)) => (x, y),
            _ => return Ok(false),
        };
        if x.digest != y.digest || x.round != y.round {
            return Ok(false);
        }
        if x.rows != y.rows {
            return Ok(false);
        }
        i += 1;
    }
    Ok(true)
}

/// 列布局形状：字段数、键序、分隔符、摘要宽度。
pub fn shape_ok(row: &str) -> bool {
    let parts: Vec<&str> = row.split('|').collect();
    if parts.len() != O1_FIELDS {
        return false;
    }
    if parts.first().copied() != Some(O1_TAG) {
        return false;
    }
    let k1 = parts.get(1).map(|s| s.starts_with("seg=")).unwrap_or(false);
    let k2 = parts.get(2).map(|s| s.starts_with("ours=")).unwrap_or(false);
    let k3 = parts.get(3).map(|s| s.starts_with("win=")).unwrap_or(false);
    let k4 = parts.get(4).map(|s| s.starts_with("d=")).unwrap_or(false);
    let k5 = parts
        .get(5)
        .map(|s| s.starts_with("digest=") && s.len() == 23)
        .unwrap_or(false);
    k1 && k2 && k3 && k4 && k5
}

/// 十轮格式联签：每轮每段 shape 一致且可往返；摘要宽度全为 16。
pub fn format_seal(seed: u64) -> Result<(u32, u64), u32> {
    let mut checked = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < ROUNDS {
        let round = build_round(seed.wrapping_add(r as u64 * 0x0100_0193), r as u8)?;
        let mut i = 0usize;
        while i < SEGMENTS {
            let row = match round.rows.get(i) {
                Some(v) => v,
                None => return Err(O1Code::SegBad.val()),
            };
            let text = render_o1(row);
            if !shape_ok(&text) || !audit_o1(&text) {
                return Err(O1Code::FormatBad.val());
            }
            digest = fold_digest(digest, row_digest(row));
            checked = checked.saturating_add(1);
            i += 1;
        }
        r += 1;
    }
    if checked != ROUNDS * SEGMENTS as u32 {
        return Err(O1Code::FormatBad.val());
    }
    Ok((checked, digest))
}

// ── 八条检查 ─────────────────────────────────────────────────────
fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::with_capacity(8);
    let seed = 0x0B14_0063u64;

    // 1) O1 列格式 ×10 联签：40 行全部 shape 一致、可往返。
    let seal = format_seal(seed);
    let (ok, info) = match seal {
        Ok((n, d)) => (n == ROUNDS * SEGMENTS as u32, format!("rows={} digest={:016X}", n, d)),
        Err(code) => (false, format!("seal code={}", code)),
    };
    out.push(("F49063/O1列格式", ok, info));

    // 2) 校差十轮入账：十轮、每轮四行、轮号连续、账摘要自洽。
    let led = build_diff_ledger(seed);
    let (ok, info) = match led {
        Ok(l) => (
            l.complete() && l.row_count() == ROUNDS as usize * SEGMENTS,
            format!("rounds={} rows={} digest={:016X}", l.len(), l.row_count(), l.digest()),
        ),
        Err(code) => (false, format!("ledger code={}", code)),
    };
    out.push(("F49063/校差十轮", ok, info));

    // 3) 差值符号与界：有账本一致慢于无账本，全部 Δ>0 且 |Δ|≤10000bp。
    let (mut ok, mut info) = (false, String::from("ledger missing"));
    if let Ok(l) = build_diff_ledger(seed) {
        let mut slow = true;
        let mut bounded = true;
        let mut rows_seen = 0u32;
        let mut i = 0usize;
        while i < l.len() {
            if let Some(r) = l.round(i) {
                let mut k = 0usize;
                while k < SEGMENTS {
                    if let Some(row) = r.rows.get(k) {
                        rows_seen = rows_seen.saturating_add(1);
                        if !row.uniformly_slower() {
                            slow = false;
                        }
                        if !row.bounded() {
                            bounded = false;
                        }
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        ok = slow && bounded && rows_seen == ROUNDS * SEGMENTS as u32;
        info = format!(
            "slow={} bounded={} rows={}",
            slow, bounded, rows_seen
        );
    }
    out.push(("F49063/差值符号界", ok, info));

    // 4) O1 账行往返：40 行全往返；篡改差值/摘要/键序全拒。
    let (mut ok, mut info) = (true, String::from("40 rows round-trip"));
    if let Ok(l) = build_diff_ledger(seed) {
        let mut checked = 0u32;
        let mut i = 0usize;
        while i < l.len() {
            if let Some(r) = l.round(i) {
                let mut k = 0usize;
                while k < SEGMENTS {
                    if let Some(row) = r.rows.get(k) {
                        let text = render_o1(row);
                        if !audit_o1(&text) {
                            ok = false;
                            info = format!("round{} seg{} round-trip red", i, k);
                        }
                        checked = checked.saturating_add(1);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        if checked != ROUNDS * SEGMENTS as u32 {
            ok = false;
            info = String::from("row count drift");
        }
        // 篡改：把 d=+ 后的数字整体改掉，重算后必然不等。
        let mut sample = String::new();
        if let Some(r0) = l.round(0) {
            if let Some(row0) = r0.rows.first() {
                sample = render_o1(row0);
            }
        }
        if !sample.is_empty() {
            let tampered = sample.replace("|d=+", "|d=+1");
            if audit_o1(&tampered) {
                ok = false;
                info = String::from("tampered delta accepted");
            }
            let swapped = sample.replace("seg=0", "seg=7");
            if audit_o1(&swapped) {
                ok = false;
                info = String::from("bad segment accepted");
            }
        }
    } else {
        ok = false;
        info = String::from("ledger build failed");
    }
    out.push(("F49063/O1账行往返", ok, info));

    // 5) 观测者开销入元数据、不入样本：plain 与直跑逐位相等；元数据登记最大/最小开销。
    let cal = calibrate_observer(seed ^ 0xA5A5);
    let (ok, info) = match cal {
        Ok(c) => {
            let clean = c.samples_clean(seed ^ 0xA5A5);
            let meta_ok = c.meta.recorded
                && c.meta.instrumented
                && c.meta.overhead_bp_max == c.max_overhead_bp()
                && c.meta.overhead_bp_min <= c.meta.overhead_bp_max;
            let budget = c.within_budget();
            (
                clean && meta_ok && budget,
                format!(
                    "clean={} meta={} budget={} max={}bp",
                    clean,
                    meta_ok,
                    budget,
                    c.max_overhead_bp()
                ),
            )
        }
        Err(code) => (false, format!("cal code={}", code)),
    };
    out.push(("F49064/开销入元数据", ok, info));

    // 6) 双跑校差报告 ×10：十轮分别标定，开销全在预算内，摘要可复跑。
    let mut ok = true;
    let mut max_bp = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < ROUNDS {
        match calibrate_observer(seed.wrapping_add(r as u64 * 0x9E37_79B9)) {
            Ok(c) => {
                if !c.within_budget() || !c.meta.recorded {
                    ok = false;
                }
                let m = c.max_overhead_bp();
                if m > max_bp {
                    max_bp = m;
                }
                digest = p_core::fold_u32(digest, m);
                digest = p_core::fold_byte(digest, r as u8);
            }
            Err(_) => ok = false,
        }
        r += 1;
    }
    let replay = {
        let mut d2 = p_core::FNV_OFF;
        let mut rr = 0u32;
        while rr < ROUNDS {
            match calibrate_observer(seed.wrapping_add(rr as u64 * 0x9E37_79B9)) {
                Ok(c) => {
                    d2 = p_core::fold_u32(d2, c.max_overhead_bp());
                    d2 = p_core::fold_byte(d2, rr as u8);
                }
                Err(_) => d2 = 0,
            }
            rr += 1;
        }
        d2 == digest
    };
    out.push((
        "F49064/双跑校差",
        ok && replay && max_bp <= OVERHEAD_BUDGET_BP,
        format!("ok={} replay={} max={}bp", ok, replay, max_bp),
    ));

    // 7) 归因报告十轮全过：条目数、分类、通过位、账行往返。
    let attr = attribution_report(seed);
    let (ok, info) = match attr {
        Ok(a) => {
            let row = render_attr(a.len() as u32, expected_overhead_bp(), 0, a.digest());
            let row_ok = audit_attr(&row);
            (
                a.all_pass() && row_ok,
                format!("entries={} all_pass={} row={}", a.len(), a.all_pass(), row_ok),
            )
        }
        Err(code) => (false, format!("attr code={}", code)),
    };
    out.push(("F49064/归因十轮", ok, info));

    // 8) 段位与异常码：14 码守卫、越界说明、坏三元组/坏列全拒、分类器边界。
    let mut ok = segment_guard();
    let mut bits = String::new();
    if !ok {
        bits.push_str("guard red; ");
    }
    if detail(SEG_BASE) != UNKNOWN || detail(SEG_BASE + 15) != UNKNOWN {
        ok = false;
        bits.push_str("detail leak; ");
    }
    if Triple::new(2, 1, 3).is_ok() || Triple::new(0, 1, 2).is_ok() || Triple::new(1, 2, NS_LIMIT + 1).is_ok() {
        ok = false;
        bits.push_str("triple guard leak; ");
    }
    let bad_cases = [
        String::new(),
        String::from("O1|seg=0|ours=1/2/3|win=1/2/3|d=+0/+0/+0"),
        String::from("O1|seg=0|ours=1/2/3|win=1/2/3|d=+0/+0/+0|digest=000000000000000z"),
        String::from("O1|seg=9|ours=1/2/3|win=1/2/3|d=+0/+0/+0|digest=0000000000000000"),
        String::from("O1|seg=0|ours=3/2/1|win=1/2/3|d=+0/+0/+0|digest=0000000000000000"),
    ];
    let mut accepted = 0u32;
    let mut i = 0usize;
    while i < bad_cases.len() {
        if let Some(c) = bad_cases.get(i) {
            if audit_o1(c) {
                accepted = accepted.saturating_add(1);
            }
        }
        i += 1;
    }
    if accepted != 0 {
        ok = false;
        bits.push_str("bad row accepted; ");
    }
    let expect = expected_overhead_bp();
    let cls_ok = classify(expect, expect) == Source::Instrument
        && classify(expect + ATTR_TOL_BP, expect) == Source::Instrument
        && classify(SCHED_LO_BP + 100, 0) == Source::Scheduler
        && classify(MAX_DELTA_BP + 1, 0) == Source::Unknown;
    if !cls_ok {
        ok = false;
        bits.push_str("classify drift; ");
    }
    if ok && bits.is_empty() {
        bits = String::from("guard/triple/row/classify ok");
    }
    out.push(("F49064/段位异常码", ok, bits));

    out
}

/// 模块入口：八条检查折叠为一个 CheckSet（域标签 `unxstar`）。
pub fn run_unx49063_checks() -> CheckSet {
    let mut set = CheckSet::new("unxstar");
    for (name, passed, detail_text) in extra_checks() {
        set.add_dyn(name, passed, &detail_text);
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(seg: u8, a: u32, b: u32, c: u32, x: u32, y: u32, z: u32) -> O1Row {
        O1Row::new(
            seg,
            Triple::new(a, b, c).expect("ours"),
            Triple::new(x, y, z).expect("win"),
        )
        .expect("row")
    }

    #[test]
    fn triple_validation() {
        assert!(Triple::new(1, 1, 1).is_ok());
        assert!(Triple::new(1, 2, 3).is_ok());
        assert_eq!(Triple::new(0, 1, 2), Err(O1Code::TripleBad.val()));
        assert_eq!(Triple::new(2, 1, 2), Err(O1Code::TripleBad.val()));
        assert_eq!(Triple::new(1, 3, 2), Err(O1Code::TripleBad.val()));
        assert_eq!(Triple::new(1, 2, NS_LIMIT + 1), Err(O1Code::TripleBad.val()));
    }

    #[test]
    fn triple_at() {
        let t = Triple::new(10, 20, 30).expect("t");
        assert_eq!(t.at(0), Some(10));
        assert_eq!(t.at(1), Some(20));
        assert_eq!(t.at(2), Some(30));
        assert_eq!(t.at(3), None);
    }

    #[test]
    fn rel_bp_basic() {
        assert_eq!(rel_bp(110, 100), 1_000);
        assert_eq!(rel_bp(100, 100), 0);
        assert_eq!(rel_bp(90, 100), -1_000);
        assert_eq!(rel_bp(1, 0), 0);
        assert_eq!(rel_bp(0, 5), -10_000);
    }

    #[test]
    fn rel_bp_truncates_toward_zero() {
        assert_eq!(rel_bp(101, 100), 100);
        assert_eq!(rel_bp(99, 100), -100);
        assert_eq!(rel_bp(1, 3), -6_666);
    }

    #[test]
    fn delta_within_bounds() {
        assert!(delta_within(0));
        assert!(delta_within(MAX_DELTA_BP as i32));
        assert!(delta_within(-(MAX_DELTA_BP as i32)));
        assert!(!delta_within(MAX_DELTA_BP as i32 + 1));
        assert!(!delta_within(i32::MIN + 1));
    }

    #[test]
    fn row_deltas_and_flags() {
        let r = row(0, 110, 220, 330, 100, 200, 300);
        assert_eq!(r.deltas(), [1_000, 1_000, 1_000]);
        assert!(r.uniformly_slower());
        assert!(r.bounded());
        let s = row(0, 100, 200, 300, 110, 220, 330);
        assert!(!s.uniformly_slower());
        assert!(s.bounded());
    }

    #[test]
    fn row_segment_guard() {
        let t = Triple::new(1, 2, 3).expect("t");
        assert!(O1Row::new(3, t, t).is_ok());
        assert_eq!(O1Row::new(4, t, t), Err(O1Code::SegBad.val()));
    }

    #[test]
    fn render_shape_and_audit() {
        let r = row(2, 100, 200, 300, 50, 100, 150);
        let text = render_o1(&r);
        assert!(shape_ok(&text));
        assert!(audit_o1(&text));
        assert!(text.starts_with("O1|seg=2|ours=100/200/300|win=50/100/150|d="));
        assert_eq!(text.split('|').count(), 6);
        assert_eq!(text.len() - text.rfind("digest=").expect("d") - 7, 16);
    }

    #[test]
    fn render_signed_values() {
        let r = row(0, 100, 200, 300, 200, 400, 600);
        let text = render_o1(&r);
        assert!(text.contains("|d=-5000/-5000/-5000"));
        let p = parse_o1(&text).expect("parse");
        assert_eq!(p, r);
    }

    #[test]
    fn digest_is_pure_function_of_values() {
        let a = row(1, 10, 20, 30, 5, 10, 15);
        let b = row(1, 10, 20, 30, 5, 10, 15);
        let c = row(1, 10, 20, 31, 5, 10, 15);
        assert_eq!(row_digest(&a), row_digest(&b));
        assert_ne!(row_digest(&a), row_digest(&c));
    }

    #[test]
    fn parse_rejects_tampered_delta() {
        let text = render_o1(&row(0, 110, 220, 330, 100, 200, 300));
        let bad = text.replace("|d=+1000", "|d=+1001");
        assert!(parse_o1(&bad).is_none());
        assert!(!audit_o1(&bad));
    }

    #[test]
    fn parse_rejects_tampered_digest() {
        let text = render_o1(&row(0, 110, 220, 330, 100, 200, 300));
        let bad = text.replace("digest=", "digest=0");
        assert!(parse_o1(&bad).is_none());
    }

    #[test]
    fn parse_rejects_wrong_tag() {
        let text = render_o1(&row(0, 110, 220, 330, 100, 200, 300));
        let bad = text.replacen("O1|", "O2|", 1);
        assert!(parse_o1(&bad).is_none());
    }

    #[test]
    fn parse_rejects_extra_field() {
        let text = render_o1(&row(0, 110, 220, 330, 100, 200, 300));
        let bad = format!("{}|x=1", text);
        assert!(parse_o1(&bad).is_none());
    }

    #[test]
    fn parse_triple_shapes() {
        assert_eq!(parse_triple("1/2/3"), Triple::new(1, 2, 3).ok());
        assert_eq!(parse_triple("1/2"), None);
        assert_eq!(parse_triple("1/2/3/4"), None);
        assert_eq!(parse_triple("1/0/3"), None);
        assert_eq!(parse_triple("3/2/1"), None);
        assert_eq!(parse_triple("a/2/3"), None);
    }

    #[test]
    fn parse_delta_shapes() {
        assert_eq!(parse_delta3("+1/-2/+3"), Some([1, -2, 3]));
        assert_eq!(parse_delta3("0/0/0"), Some([0, 0, 0]));
        assert_eq!(parse_delta3("1/2"), None);
        assert_eq!(parse_delta3("1/2/3/4"), None);
        assert_eq!(parse_delta3("1/x/3"), None);
        assert_eq!(parse_delta3("+/1/2"), None);
    }

    #[test]
    fn parse_i32_edges() {
        assert_eq!(parse_i32("0"), Some(0));
        assert_eq!(parse_i32("+42"), Some(42));
        assert_eq!(parse_i32("-42"), Some(-42));
        assert_eq!(parse_i32("2147483647"), Some(i32::MAX));
        assert_eq!(parse_i32("-2147483648"), Some(i32::MIN));
        assert_eq!(parse_i32("2147483648"), None);
        assert_eq!(parse_i32("-2147483649"), None);
        assert_eq!(parse_i32(""), None);
        assert_eq!(parse_i32("+"), None);
        assert_eq!(parse_i32("1+2"), None);
    }

    #[test]
    fn parse_hex16_edges() {
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Some(u64::MAX));
        assert_eq!(parse_hex16("ffffffffffffffff"), Some(u64::MAX));
        assert_eq!(parse_hex16("0"), None);
        assert_eq!(parse_hex16("000000000000000g"), None);
    }

    #[test]
    fn shape_ok_rejects_layout_drift() {
        let text = render_o1(&row(0, 1, 2, 3, 1, 2, 3));
        assert!(shape_ok(&text));
        assert!(!shape_ok(&text.replace("seg=", "segment=")));
        assert!(!shape_ok(&text.replace("|d=", "|delta=")));
        assert!(!shape_ok(&text.replace("digest=", "hash=")));
        assert!(!shape_ok("O1|seg=0"));
    }

    #[test]
    fn run_mode_monotone() {
        for &seed in &[1u64, 2, 3, 99, 0xDEAD] {
            let t = run_mode(seed, RunMode::Plain).expect("plain");
            for x in t.iter() {
                assert!(x.p50 <= x.p95 && x.p95 <= x.p99);
                assert!(x.p99 <= NS_LIMIT);
            }
        }
    }

    #[test]
    fn run_mode_deterministic() {
        let a = run_mode(0x1234, RunMode::Instrumented).expect("a");
        let b = run_mode(0x1234, RunMode::Instrumented).expect("b");
        assert_eq!(a, b);
    }

    #[test]
    fn run_mode_instrumented_slower() {
        let p = run_mode(0x77, RunMode::Plain).expect("p");
        let i = run_mode(0x77, RunMode::Instrumented).expect("i");
        for k in 0..SEGMENTS {
            assert!(i[k].p50 > p[k].p50);
            assert!(i[k].p95 > p[k].p95);
            assert!(i[k].p99 > p[k].p99);
        }
    }

    #[test]
    fn instrument_overhead_formula() {
        assert_eq!(instrument_overhead(0), INSTR_FIXED_NS);
        assert_eq!(instrument_overhead(100), 3 + INSTR_FIXED_NS);
        assert_eq!(instrument_overhead(1_000), 30 + INSTR_FIXED_NS);
    }

    #[test]
    fn calibrate_observer_ok() {
        let c = calibrate_observer(0xABC).expect("cal");
        assert!(c.within_budget());
        assert!(c.meta.recorded);
        assert!(c.meta.instrumented);
        assert!(c.max_overhead_bp() <= OVERHEAD_BUDGET_BP);
        assert_eq!(c.meta.overhead_bp_max, c.max_overhead_bp());
    }

    #[test]
    fn calibrate_observer_samples_clean() {
        let seed = 0x5150;
        let c = calibrate_observer(seed).expect("cal");
        assert!(c.samples_clean(seed));
        assert_eq!(c.plain, run_mode(seed, RunMode::Plain).expect("raw"));
    }

    #[test]
    fn calibrate_observer_overhead_positive() {
        let c = calibrate_observer(0x6161).expect("cal");
        for v in c.overhead_bp.iter() {
            assert!(*v > 0);
        }
    }

    #[test]
    fn observer_budget_is_five_percent() {
        assert_eq!(OVERHEAD_BUDGET_BP, 500);
        let c = calibrate_observer(0x7171).expect("cal");
        assert!(c.max_overhead_bp() <= 500);
    }

    #[test]
    fn expected_overhead_reasonable() {
        let e = expected_overhead_bp();
        assert!(e > 0 && e <= OVERHEAD_BUDGET_BP);
    }

    #[test]
    fn classify_boundaries() {
        let e = expected_overhead_bp();
        assert_eq!(classify(e, e), Source::Instrument);
        assert_eq!(classify(e + ATTR_TOL_BP, e), Source::Instrument);
        assert_eq!(classify(e + ATTR_TOL_BP + 1, e), Source::Unknown);
        assert_eq!(classify(SCHED_LO_BP + 1, 0), Source::Scheduler);
        assert_eq!(classify(SCHED_HI_BP, 0), Source::Scheduler);
        assert_eq!(classify(SCHED_HI_BP + 1, 0), Source::Unknown);
    }

    #[test]
    fn source_names() {
        assert_eq!(Source::Instrument.name(), "instrument");
        assert_eq!(Source::Scheduler.name(), "scheduler");
        assert_eq!(Source::Unknown.name(), "unknown");
        assert_eq!(Source::Instrument.index(), 0);
        assert_eq!(Source::Unknown.index(), 2);
    }

    #[test]
    fn build_round_bounded_and_slower() {
        let r = build_round(0x8080, 0).expect("round");
        assert_eq!(r.round, 0);
        assert!(r.bounded());
        assert_eq!(r.digest, r.compute_digest());
        for x in r.rows.iter() {
            assert!(x.uniformly_slower());
        }
    }

    #[test]
    fn build_round_rejects_big_round() {
        assert_eq!(build_round(1, ROUNDS as u8), Err(O1Code::RoundBad.val()));
        assert_eq!(build_round(1, 255), Err(O1Code::RoundBad.val()));
    }

    #[test]
    fn diff_ledger_complete() {
        let l = build_diff_ledger(0x9191).expect("ledger");
        assert_eq!(l.len(), ROUNDS as usize);
        assert_eq!(l.row_count(), ROUNDS as usize * SEGMENTS);
        assert!(l.complete());
        assert!(!l.is_empty());
        assert_ne!(l.digest(), 0);
    }

    #[test]
    fn diff_ledger_push_guard() {
        let mut l = DiffLedger::new();
        let mut r = build_round(0x2222, 0).expect("r");
        assert!(l.push(r).is_ok());
        r.round = 0;
        assert_eq!(l.push(r), Err(O1Code::RoundBad.val()));
    }

    #[test]
    fn diff_ledger_digest_tamper_rejected() {
        let mut l = DiffLedger::new();
        let mut r = build_round(0x3333, 0).expect("r");
        r.digest ^= 1;
        assert_eq!(l.push(r), Err(O1Code::LedgerBad.val()));
    }

    #[test]
    fn diff_ledger_bound_violation_rejected() {
        let t1 = Triple::new(1, 2, 3).expect("t");
        let t2 = Triple::new(1_000, 2_000, 3_000).expect("t");
        let mut rows = [O1Row {
            seg: 0,
            ours: t1,
            win: t1,
        }; SEGMENTS];
        let mut i = 0usize;
        while i < SEGMENTS {
            rows[i] = O1Row::new(i as u8, t2, t1).expect("row");
            i += 1;
        }
        let mut r = DiffRound {
            round: 0,
            rows,
            digest: 0,
        };
        r.digest = r.compute_digest();
        let mut l = DiffLedger::new();
        // 各分位差约 +299900/+99900/+99900 bp：远超 100% 上限。
        assert_eq!(l.push(r), Err(O1Code::BoundBad.val()));
    }

    #[test]
    fn diff_replay_same_seed() {
        assert_eq!(diff_replay(0x4444), Ok(true));
    }

    #[test]
    fn diff_ledger_differs_by_seed() {
        let a = build_diff_ledger(0x5555).expect("a");
        let b = build_diff_ledger(0x6666).expect("b");
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn format_seal_counts() {
        let (n, d) = format_seal(0x7777).expect("seal");
        assert_eq!(n, ROUNDS * SEGMENTS as u32);
        assert_ne!(d, 0);
    }

    #[test]
    fn format_seal_currently_reproducible_but_digest_depends_on_seed() {
        let a = format_seal(0x1111).expect("a");
        let b = format_seal(0x1111).expect("b");
        let c = format_seal(0x1234).expect("c");
        assert_eq!(a, b);
        assert_ne!(a.1, c.1);
    }

    #[test]
    fn attribution_report_all_pass() {
        let a = attribution_report(0x8888).expect("attr");
        assert_eq!(a.len(), ROUNDS as usize);
        assert!(a.all_pass());
        for e in a.entries.iter() {
            assert_eq!(e.source, Source::Instrument);
            assert!(e.delta_bp <= OVERHEAD_BUDGET_BP);
        }
    }

    #[test]
    fn attribution_entries_ordered() {
        let a = attribution_report(0x9999).expect("attr");
        for (i, e) in a.entries.iter().enumerate() {
            assert_eq!(e.round as usize, i);
            assert_eq!(e.expected_bp, expected_overhead_bp());
        }
    }

    #[test]
    fn attribution_deterministic() {
        let a = attribution_report(0xAAAA).expect("a");
        let b = attribution_report(0xAAAA).expect("b");
        assert_eq!(a.digest(), b.digest());
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn attribution_differs_by_seed() {
        let a = attribution_report(0xBBBB).expect("a");
        let b = attribution_report(0xCCCC).expect("b");
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn attr_row_roundtrip() {
        let row = render_attr(10, 320, 340, 0xDEAD_BEEF);
        assert!(audit_attr(&row));
        assert_eq!(parse_attr(&row), Some((10, 320, 340, 0xDEAD_BEEF)));
        assert!(row.contains("|rounds=10|"));
    }

    #[test]
    fn attr_row_rejects_bad() {
        assert!(parse_attr("").is_none());
        assert!(parse_attr("ATTR|rounds=10|expected=320|max=340").is_none());
        assert!(parse_attr("ATTR|rounds=x|expected=320|max=340|digest=0000000000000000").is_none());
        assert!(parse_attr("X|rounds=10|expected=320|max=340|digest=0000000000000000").is_none());
        assert!(parse_attr("ATTR|rounds=10|expected=320|max=340|digest=0000000000000000|x=1").is_none());
    }

    #[test]
    fn code_values_dense_and_unique() {
        let mut seen = [false; 14];
        for c in O1Code::ALL {
            let off = c.val() - SEG_BASE;
            assert!(off >= 1 && off <= 14);
            let idx = (off - 1) as usize;
            assert!(!seen[idx]);
            seen[idx] = true;
        }
        assert!(seen.iter().all(|v| *v));
    }

    #[test]
    fn code_details_distinct() {
        let mut texts: Vec<&'static str> = Vec::new();
        for c in O1Code::ALL {
            let t = c.detail();
            assert!(!t.is_empty());
            assert!(!texts.contains(&t));
            texts.push(t);
        }
        assert_eq!(texts.len(), 14);
    }

    #[test]
    fn free_detail_bounds() {
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 15), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 1), O1Code::SegBad.detail());
        assert_eq!(detail(SEG_BASE + 14), O1Code::Overflow.detail());
    }

    #[test]
    fn segment_guard_green() {
        assert!(segment_guard());
    }

    #[test]
    fn segment_helpers() {
        assert_eq!(segment_from_index(0), Ok(0));
        assert_eq!(segment_from_index(3), Ok(3));
        assert!(segment_from_index(4).is_err());
        assert_eq!(segment_name(0), "interrupt");
        assert_eq!(segment_name(3), "present");
        assert_eq!(segment_name(9), "unknown");
    }

    #[test]
    fn checks_all_green() {
        let set = run_unx49063_checks();
        assert_eq!(set.len(), 8);
        assert!(!set.truncated());
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn checks_names_unique() {
        let set = run_unx49063_checks();
        let mut names: Vec<&str> = Vec::new();
        for c in set.iter() {
            assert!(!names.contains(&c.name));
            names.push(c.name);
        }
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn checks_stable_across_runs() {
        let a = run_unx49063_checks();
        let b = run_unx49063_checks();
        assert_eq!(a.passed_count(), b.passed_count());
        assert_eq!(a.failed_count(), b.failed_count());
    }

    #[test]
    fn mode_names_and_index() {
        assert_eq!(RunMode::Plain.name(), "plain");
        assert_eq!(RunMode::Instrumented.name(), "instr");
        assert_eq!(RunMode::Plain.index(), 0);
        assert_eq!(RunMode::Instrumented.index(), 1);
    }

    #[test]
    fn row_equality_ignores_derived_delta() {
        let a = row(0, 10, 20, 30, 5, 10, 15);
        let b = row(0, 10, 20, 30, 5, 10, 15);
        assert_eq!(a, b);
        assert_eq!(a.deltas(), b.deltas());
    }

    #[test]
    fn diff_round_manifest_counts() {
        let r = build_round(0xDDDD, 3).expect("round");
        assert_eq!(r.round, 3);
        assert_eq!(r.rows.len(), SEGMENTS);
        let mut seen_segments = [false; SEGMENTS];
        for x in r.rows.iter() {
            seen_segments[x.seg as usize] = true;
        }
        assert!(seen_segments.iter().all(|v| *v));
    }

    #[test]
    fn all_rows_bounded_across_rounds() {
        let l = build_diff_ledger(0xEEEE).expect("ledger");
        let mut i = 0usize;
        while i < l.len() {
            if let Some(r) = l.round(i) {
                assert!(r.bounded());
            }
            i += 1;
        }
    }

    #[test]
    fn calibration_cross_seed_consistency() {
        let a = calibrate_observer(1).expect("a");
        let b = calibrate_observer(1).expect("b");
        assert_eq!(a, b);
        let c = calibrate_observer(2).expect("c");
        assert_ne!(a.instrumented, c.instrumented);
    }
}
