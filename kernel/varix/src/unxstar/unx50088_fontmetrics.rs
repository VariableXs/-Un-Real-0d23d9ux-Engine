//! UNX-F50088 字体度量一致性账（字距/行高与源一致）（0x43720 段）
//!
//! 锚点：UNX-F50088-J1 度量 ×30 与源逐项一致；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段 `SEG_BASE = 0x43720`：诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 9`，
//! 逐项见 [`MetricsError`]。
//!
//! 度量一致性的难点不是「比数值」，而是「拿什么口径比」：源字体以 design
//! units 记录（upem 常见 1000/2048），渲染栈按目标 upem 重新缩放。若直接比
//! 原始整数，2048 源与 1000 渲染必然“全红”，判据就失去意义。本模块先把
//! 两侧统一归一到 1000 upem（[`normalize_units`]），再逐项比对：
//! * 字距（advance/kern）归一后要求**精确相等**（容差 0）；
//! * 行高相关的 ascent/descent/line_gap 允许 ±1 单位，吸收缩放四舍五入。
//!
//! 结构清单：
//! * [`GlyphMetric`]/[`KerningPair`]：按 gid、按 (left,right) 严格升序的定长
//!   度量表，插入即校验重复与乱序，查询走二分；
//! * [`FontMetrics`] 解析产物 + 垂直度量，`line_height_units` 为闭式推导；
//! * [`parse_metrics`]/[`encode_metrics`]：文本往返，全部错误显式返回；
//! * [`MetricsLedger`] 一致性账本（Draft→Sealed 状态机 + 30 行差分 + 容差判定）；
//! * 注入核 [`injection_core`]：八类坏样本逐类断言具体诊断码。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43720 独占 0x20）。
pub const SEG_BASE: u32 = 0x43720;

pub const E_EMPTY: u16 = 1;
pub const E_BADHEADER: u16 = 2;
pub const E_BADNUMBER: u16 = 3;
pub const E_DUPLICATE: u16 = 4;
pub const E_ORDER: u16 = 5;
pub const E_TRUNCATED: u16 = 6;
pub const E_RANGE: u16 = 7;
pub const E_MISMATCH: u16 = 8;
pub const E_FULL: u16 = 9;

/// 度量一致性错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricsError {
    Empty,
    BadHeader,
    BadNumber,
    Duplicate,
    Order,
    Truncated,
    Range,
    Mismatch,
    Full,
}

impl MetricsError {
    pub fn code(self) -> u32 {
        let n = match self {
            MetricsError::Empty => E_EMPTY,
            MetricsError::BadHeader => E_BADHEADER,
            MetricsError::BadNumber => E_BADNUMBER,
            MetricsError::Duplicate => E_DUPLICATE,
            MetricsError::Order => E_ORDER,
            MetricsError::Truncated => E_TRUNCATED,
            MetricsError::Range => E_RANGE,
            MetricsError::Mismatch => E_MISMATCH,
            MetricsError::Full => E_FULL,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            MetricsError::Empty => "metrics blob is empty",
            MetricsError::BadHeader => "unknown or malformed section header",
            MetricsError::BadNumber => "numeric field is not a decimal integer",
            MetricsError::Duplicate => "duplicate glyph or kerning key",
            MetricsError::Order => "metrics row inserted out of order",
            MetricsError::Truncated => "required section or row missing",
            MetricsError::Range => "metric value outside design range",
            MetricsError::Mismatch => "rendered metric disagrees with source",
            MetricsError::Full => "metric table capacity exhausted",
        }
    }
}

/// upem 合法区间。低于 16 的 em 无法容纳可读字形，高于 16384 只是浪费位宽。
pub const UPEM_MIN: u16 = 16;
pub const UPEM_MAX: u16 = 16384;
/// 单表行数上限（advance 与 kerning 各自独立）。
pub const MAX_ROWS: usize = 4096;
/// 归一化分母：所有比较统一折算到 1000 upem。
pub const NORM_UPEM: i32 = 1000;

/// 度量类别。容差只在归一化单位上成立。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricKind {
    Ascent,
    Descent,
    LineGap,
    Advance,
    Kern,
}

impl MetricKind {
    pub fn code(self) -> u8 {
        match self {
            MetricKind::Ascent => 0,
            MetricKind::Descent => 1,
            MetricKind::LineGap => 2,
            MetricKind::Advance => 3,
            MetricKind::Kern => 4,
        }
    }

    /// 归一化到 1000 upem 后的允许偏差。垂直度量 ±1 吸收缩放取整；
    /// 字距必须精确，否则排版会出现累积漂移。
    pub fn tolerance(self) -> i32 {
        match self {
            MetricKind::Ascent | MetricKind::Descent | MetricKind::LineGap => 1,
            MetricKind::Advance | MetricKind::Kern => 0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MetricKind::Ascent => "ascent",
            MetricKind::Descent => "descent",
            MetricKind::LineGap => "line-gap",
            MetricKind::Advance => "advance",
            MetricKind::Kern => "kern",
        }
    }
}

/// 单字形步进。gid 升序存放，查询二分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphMetric {
    pub gid: u16,
    pub advance: i16,
}

/// 字距对。按 (left,right) 升序存放。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KerningPair {
    pub left: u16,
    pub right: u16,
    pub value: i16,
}

/// 一份字体度量快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontMetrics {
    pub upem: u16,
    pub ascent: i16,
    pub descent: i16,
    pub line_gap: i16,
    pub advances: Vec<GlyphMetric>,
    pub kerning: Vec<KerningPair>,
}

impl FontMetrics {
    /// 构造并校验垂直度量。descent 以负值表示基线以下。
    pub fn new(upem: u16, ascent: i16, descent: i16, line_gap: i16) -> Result<FontMetrics, MetricsError> {
        if upem < UPEM_MIN || upem > UPEM_MAX {
            return Err(MetricsError::Range);
        }
        if ascent < 0 || ascent > 4000 {
            return Err(MetricsError::Range);
        }
        if descent > 0 || descent < -4000 {
            return Err(MetricsError::Range);
        }
        if line_gap < -500 || line_gap > 4000 {
            return Err(MetricsError::Range);
        }
        Ok(FontMetrics {
            upem,
            ascent,
            descent,
            line_gap,
            advances: Vec::new(),
            kerning: Vec::new(),
        })
    }

    /// 行高 = 上升部 −（负的）下降部 + 行间隙。以 design units 返回。
    pub fn line_height_units(&self) -> i32 {
        self.ascent as i32 - self.descent as i32 + self.line_gap as i32
    }

    /// 插入步进。重复 gid 报 Duplicate，低于表尾 gid 报 Order（必须升序）。
    pub fn add_advance(&mut self, gid: u16, advance: i16) -> Result<(), MetricsError> {
        if self.advances.len() >= MAX_ROWS {
            return Err(MetricsError::Full);
        }
        if advance < 0 || advance > 4000 {
            return Err(MetricsError::Range);
        }
        if let Some(last) = self.advances.last() {
            if gid == last.gid {
                return Err(MetricsError::Duplicate);
            }
            if gid < last.gid {
                return Err(MetricsError::Order);
            }
        }
        self.advances.push(GlyphMetric { gid, advance });
        Ok(())
    }

    /// 插入字距对。键必须严格升序，值限制在 ±2000。
    pub fn add_kern(&mut self, left: u16, right: u16, value: i16) -> Result<(), MetricsError> {
        if self.kerning.len() >= MAX_ROWS {
            return Err(MetricsError::Full);
        }
        if value < -2000 || value > 2000 {
            return Err(MetricsError::Range);
        }
        if let Some(last) = self.kerning.last() {
            if left == last.left && right == last.right {
                return Err(MetricsError::Duplicate);
            }
            if (left, right) < (last.left, last.right) {
                return Err(MetricsError::Order);
            }
        }
        self.kerning.push(KerningPair { left, right, value });
        Ok(())
    }

    /// 二分查步进。未登记的字形返回 None，由调用方决定如何降级。
    pub fn lookup_advance(&self, gid: u16) -> Option<i16> {
        let mut lo = 0usize;
        let mut hi = self.advances.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.advances.get(mid) {
                Some(m) => {
                    if m.gid == gid {
                        return Some(m.advance);
                    } else if m.gid < gid {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                None => return None,
            }
        }
        None
    }

    /// 二分查字距对。未登记返回 None（零字距与“未登记”严格区分）。
    pub fn lookup_kern(&self, left: u16, right: u16) -> Option<i16> {
        let mut lo = 0usize;
        let mut hi = self.kerning.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.kerning.get(mid) {
                Some(p) => {
                    let key = (p.left, p.right);
                    if key == (left, right) {
                        return Some(p.value);
                    } else if key < (left, right) {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                None => return None,
            }
        }
        None
    }
}

/// 把 design units 归一到 1000 upem。四舍五入（远离零）保证正负对称。
pub fn normalize_units(value: i32, upem: u16) -> i32 {
    if upem == 0 {
        return 0;
    }
    let u = upem as i32;
    let scaled = value * NORM_UPEM;
    if scaled >= 0 {
        (scaled + u / 2) / u
    } else {
        (scaled - u / 2) / u
    }
}

// ---------------------------------------------------------------------------
// 文本解析 / 编码
// ---------------------------------------------------------------------------

fn parse_i32(s: &str) -> Result<i32, MetricsError> {
    let t = s.trim();
    if t.is_empty() {
        return Err(MetricsError::BadNumber);
    }
    let (neg, body) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    if body.is_empty() {
        return Err(MetricsError::BadNumber);
    }
    let mut v: i64 = 0;
    let mut i = 0usize;
    let bytes = body.as_bytes();
    while i < bytes.len() {
        let b = bytes[i];
        if b < b'0' || b > b'9' {
            return Err(MetricsError::BadNumber);
        }
        v = v.saturating_mul(10).saturating_add((b - b'0') as i64);
        if v > 1_000_000 {
            return Err(MetricsError::BadNumber);
        }
        i += 1;
    }
    let signed = if neg { -v } else { v };
    Ok(signed as i32)
}

fn parse_u16(s: &str) -> Result<u16, MetricsError> {
    let v = parse_i32(s)?;
    if v < 0 || v > 65535 {
        return Err(MetricsError::BadNumber);
    }
    Ok(v as u16)
}

/// 带行数上限的解析入口。注入核用小上限把 [`MetricsError::Full`] 真正走通。
pub fn parse_metrics_with_cap(text: &str, max_rows: usize) -> Result<FontMetrics, MetricsError> {
    if text.trim().is_empty() {
        return Err(MetricsError::Empty);
    }
    let mut upem: Option<u16> = None;
    let mut asc: Option<i16> = None;
    let mut desc: Option<i16> = None;
    let mut gap: Option<i16> = None;
    let mut adv_raw: Vec<(u16, i16)> = Vec::new();
    let mut kern_raw: Vec<(u16, u16, i16)> = Vec::new();
    let mut tokens = 0usize;
    let mut it = text.split('|');

    while let Some(raw) = it.next() {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        tokens += 1;
        // 8192 个 token 已经远超现实字体度量行数，多余者按容量问题拒绝。
        if tokens > 8192 {
            return Err(MetricsError::Full);
        }
        let (key, val) = match token.split_once('=') {
            Some(kv) => kv,
            None => return Err(MetricsError::BadHeader),
        };
        match key {
            "upem" => {
                if upem.is_some() {
                    return Err(MetricsError::Duplicate);
                }
                let v = parse_u16(val)?;
                if v < UPEM_MIN || v > UPEM_MAX {
                    return Err(MetricsError::Range);
                }
                upem = Some(v);
            }
            "asc" | "desc" | "gap" => {
                let v = parse_i32(val)?;
                if v < -4000 || v > 4000 {
                    return Err(MetricsError::Range);
                }
                let slot = match key {
                    "asc" => &mut asc,
                    "desc" => &mut desc,
                    _ => &mut gap,
                };
                if slot.is_some() {
                    return Err(MetricsError::Duplicate);
                }
                *slot = Some(v as i16);
            }
            "adv" => {
                if adv_raw.len() >= max_rows {
                    return Err(MetricsError::Full);
                }
                let (g, a) = match val.split_once(':') {
                    Some(ga) => ga,
                    None => return Err(MetricsError::BadNumber),
                };
                let gid = parse_u16(g)?;
                let adv = parse_i32(a)?;
                if adv < 0 || adv > 4000 {
                    return Err(MetricsError::Range);
                }
                adv_raw.push((gid, adv as i16));
            }
            "kern" => {
                if kern_raw.len() >= max_rows {
                    return Err(MetricsError::Full);
                }
                let (lr, v) = match val.split_once(':') {
                    Some(kv) => kv,
                    None => return Err(MetricsError::BadNumber),
                };
                let (l, r) = match lr.split_once(',') {
                    Some(pair) => pair,
                    None => return Err(MetricsError::BadNumber),
                };
                let left = parse_u16(l)?;
                let right = parse_u16(r)?;
                let value = parse_i32(v)?;
                if value < -2000 || value > 2000 {
                    return Err(MetricsError::Range);
                }
                kern_raw.push((left, right, value as i16));
            }
            _ => return Err(MetricsError::BadHeader),
        }
    }

    let (u, a, d, g) = match (upem, asc, desc, gap) {
        (Some(u), Some(a), Some(d), Some(g)) => (u, a, d, g),
        _ => return Err(MetricsError::Truncated),
    };
    let mut m = FontMetrics::new(u, a, d, g)?;
    let mut i = 0usize;
    while i < adv_raw.len() {
        if let Some((gid, adv)) = adv_raw.get(i) {
            m.add_advance(*gid, *adv)?;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < kern_raw.len() {
        if let Some((l, r, v)) = kern_raw.get(k) {
            m.add_kern(*l, *r, *v)?;
        }
        k += 1;
    }
    Ok(m)
}

/// 标准解析入口（行数上限取 [`MAX_ROWS`]）。
pub fn parse_metrics(text: &str) -> Result<FontMetrics, MetricsError> {
    parse_metrics_with_cap(text, MAX_ROWS)
}

/// 规范编码。输出顺序与表内顺序一致，因此编码是确定的。
pub fn encode_metrics(m: &FontMetrics) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "upem={}|asc={}|desc={}|gap={}",
        m.upem, m.ascent, m.descent, m.line_gap
    ));
    let mut i = 0usize;
    while i < m.advances.len() {
        if let Some(a) = m.advances.get(i) {
            out.push_str(&format!("|adv={}:{}", a.gid, a.advance));
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < m.kerning.len() {
        if let Some(p) = m.kerning.get(k) {
            out.push_str(&format!("|kern={},{}:{}", p.left, p.right, p.value));
        }
        k += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 一致性账本
// ---------------------------------------------------------------------------

/// 账本状态。封账后只读，避免“边比边改源”。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerState {
    Open,
    Sealed,
}

/// 一行度量差分。`within` 由 delta 与容差当场算出，事后不可篡改。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricDiff {
    pub index: u16,
    pub kind: MetricKind,
    pub gid: u16,
    pub source: i32,
    pub rendered: i32,
    pub delta: i32,
    pub tolerance: i32,
    pub within: bool,
}

/// 度量一致性账本：逐项差分 + 守恒计数 + 封账纪律。
#[derive(Clone, Debug)]
pub struct MetricsLedger {
    diffs: Vec<MetricDiff>,
    pub state: LedgerState,
    pub mismatches: u32,
    pub max_abs_delta: i32,
}

impl MetricsLedger {
    pub fn new() -> MetricsLedger {
        MetricsLedger {
            diffs: Vec::new(),
            state: LedgerState::Open,
            mismatches: 0,
            max_abs_delta: 0,
        }
    }

    fn push(&mut self, kind: MetricKind, gid: u16, source: i32, rendered: i32) {
        let delta = rendered - source;
        let tol = kind.tolerance();
        let within = if delta < 0 { -delta <= tol } else { delta <= tol };
        if !within {
            self.mismatches += 1;
        }
        let abs = if delta < 0 { -delta } else { delta };
        if abs > self.max_abs_delta {
            self.max_abs_delta = abs;
        }
        let index = self.diffs.len() as u16;
        self.diffs.push(MetricDiff {
            index,
            kind,
            gid,
            source,
            rendered,
            delta,
            tolerance: tol,
            within,
        });
    }

    /// 对比源/渲染两套度量。行集合必须一一对应（按 gid 与字距对匹配），
    /// 缺少任一行都按 Truncated 处理——少比一行比“比错”更危险。
    pub fn compare(&mut self, source: &FontMetrics, rendered: &FontMetrics) -> Result<(), MetricsError> {
        if self.state != LedgerState::Open {
            return Err(MetricsError::Order);
        }
        if source.advances.len() != rendered.advances.len()
            || source.kerning.len() != rendered.kerning.len()
        {
            return Err(MetricsError::Truncated);
        }
        self.diffs.clear();
        self.mismatches = 0;
        self.max_abs_delta = 0;

        self.push(
            MetricKind::Ascent,
            0,
            normalize_units(source.ascent as i32, source.upem),
            normalize_units(rendered.ascent as i32, rendered.upem),
        );
        self.push(
            MetricKind::Descent,
            0,
            normalize_units(source.descent as i32, source.upem),
            normalize_units(rendered.descent as i32, rendered.upem),
        );
        self.push(
            MetricKind::LineGap,
            0,
            normalize_units(source.line_gap as i32, source.upem),
            normalize_units(rendered.line_gap as i32, rendered.upem),
        );

        let mut i = 0usize;
        while i < source.advances.len() {
            let src = match source.advances.get(i) {
                Some(v) => *v,
                None => return Err(MetricsError::Truncated),
            };
            let out = match rendered.lookup_advance(src.gid) {
                Some(v) => v,
                None => return Err(MetricsError::Truncated),
            };
            self.push(
                MetricKind::Advance,
                src.gid,
                normalize_units(src.advance as i32, source.upem),
                normalize_units(out as i32, rendered.upem),
            );
            i += 1;
        }

        let mut k = 0usize;
        while k < source.kerning.len() {
            let src = match source.kerning.get(k) {
                Some(v) => *v,
                None => return Err(MetricsError::Truncated),
            };
            let out = match rendered.lookup_kern(src.left, src.right) {
                Some(v) => v,
                None => return Err(MetricsError::Truncated),
            };
            self.push(
                MetricKind::Kern,
                src.left,
                normalize_units(src.value as i32, source.upem),
                normalize_units(out as i32, rendered.upem),
            );
            k += 1;
        }
        Ok(())
    }

    /// 严格判据：任一项越容差即 Mismatch，并保留全部差分供定位。
    pub fn strict(&self) -> Result<(), MetricsError> {
        if self.mismatches == 0 && !self.diffs.is_empty() {
            Ok(())
        } else {
            Err(MetricsError::Mismatch)
        }
    }

    /// 封账。二次封账按顺序错误拒绝，防止重复盖章。
    pub fn seal(&mut self) -> Result<(), MetricsError> {
        if self.state == LedgerState::Sealed {
            return Err(MetricsError::Order);
        }
        self.state = LedgerState::Sealed;
        Ok(())
    }

    pub fn rows(&self) -> &[MetricDiff] {
        &self.diffs
    }

    pub fn count_of(&self, kind: MetricKind) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.diffs.len() {
            if let Some(d) = self.diffs.get(i) {
                if d.kind == kind {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 首个越差行的索引（用于现场定位），无越差返回 None。
    pub fn first_mismatch(&self) -> Option<&MetricDiff> {
        let mut i = 0usize;
        while i < self.diffs.len() {
            if let Some(d) = self.diffs.get(i) {
                if !d.within {
                    return Some(d);
                }
            }
            i += 1;
        }
        None
    }

    pub fn summary(&self) -> String {
        format!(
            "rows={} mismatches={} max_abs_delta={}",
            self.diffs.len(),
            self.mismatches,
            self.max_abs_delta
        )
    }
}

// ---------------------------------------------------------------------------
// 注入核：八类坏样本
// ---------------------------------------------------------------------------

const GOOD: &str = "upem=1000|asc=800|desc=-200|gap=90|adv=3:500|adv=5:620|kern=3,5:-20";

/// 注入核。返回（被抓数, 总数），每条对照具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    if parse_metrics("   ").err() == Some(MetricsError::Empty) {
        caught += 1;
    }
    if parse_metrics("upem=1000|bogus=1").err() == Some(MetricsError::BadHeader) {
        caught += 1;
    }
    if parse_metrics("upem=abc|asc=800|desc=-200|gap=90").err() == Some(MetricsError::BadNumber) {
        caught += 1;
    }
    if parse_metrics("upem=1000|asc=800|desc=-200").err() == Some(MetricsError::Truncated) {
        caught += 1;
    }
    if parse_metrics("upem=1000|asc=800|desc=-200|gap=90|adv=5:500|adv=5:600").err()
        == Some(MetricsError::Duplicate)
    {
        caught += 1;
    }
    if parse_metrics("upem=1000|asc=800|desc=-200|gap=90|adv=5:500|adv=4:500").err()
        == Some(MetricsError::Order)
    {
        caught += 1;
    }
    if parse_metrics("upem=8|asc=800|desc=-200|gap=90").err() == Some(MetricsError::Range) {
        caught += 1;
    }
    if parse_metrics_with_cap(GOOD, 1).err() == Some(MetricsError::Full) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

fn build_source() -> FontMetrics {
    let mut m = match FontMetrics::new(2048, 1638, -410, 184) {
        Ok(v) => v,
        Err(_) => FontMetrics {
            upem: 1000,
            ascent: 800,
            descent: -200,
            line_gap: 90,
            advances: Vec::new(),
            kerning: Vec::new(),
        },
    };
    let mut gid = 3u16;
    while gid <= 21 {
        let adv = 500 + (gid as i32) * 7;
        let _ = m.add_advance(gid, adv as i16);
        gid += 1;
    }
    let mut i = 0u16;
    while i < 8 {
        let left = 3 + i * 2;
        let right = left + 2;
        let value = -40 + (i as i16) * 5;
        let _ = m.add_kern(left, right, value);
        i += 1;
    }
    m
}

/// 用源快照构造一份“等价但 upem=1000”的渲染快照，模拟渲染栈重缩放。
fn render_from(src: &FontMetrics) -> FontMetrics {
    let mut m = match FontMetrics::new(
        NORM_UPEM as u16,
        normalize_units(src.ascent as i32, src.upem) as i16,
        normalize_units(src.descent as i32, src.upem) as i16,
        normalize_units(src.line_gap as i32, src.upem) as i16,
    ) {
        Ok(v) => v,
        Err(_) => FontMetrics {
            upem: 1000,
            ascent: 800,
            descent: -200,
            line_gap: 90,
            advances: Vec::new(),
            kerning: Vec::new(),
        },
    };
    let mut i = 0usize;
    while i < src.advances.len() {
        if let Some(a) = src.advances.get(i) {
            let norm = normalize_units(a.advance as i32, src.upem) as i16;
            let _ = m.add_advance(a.gid, norm);
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < src.kerning.len() {
        if let Some(p) = src.kerning.get(k) {
            let norm = normalize_units(p.value as i32, src.upem) as i16;
            let _ = m.add_kern(p.left, p.right, norm);
        }
        k += 1;
    }
    m
}

pub fn run_unx50088_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50088");
    let src = build_source();
    let out = render_from(&src);

    // J1-1 度量 ×30 逐项一致：3 垂直 + 19 步进 + 8 字距
    let mut ledger = MetricsLedger::new();
    let compared = ledger.compare(&src, &out);
    let ok = compared.is_ok()
        && ledger.rows().len() == 30
        && ledger.mismatches == 0
        && ledger.strict().is_ok()
        && ledger.count_of(MetricKind::Advance) == 19
        && ledger.count_of(MetricKind::Kern) == 8
        && ledger.count_of(MetricKind::Ascent)
            + ledger.count_of(MetricKind::Descent)
            + ledger.count_of(MetricKind::LineGap)
            == 3;
    let det = format!(
        "rows={} mism={} max={}",
        ledger.rows().len(),
        ledger.mismatches,
        ledger.max_abs_delta
    );
    s.add_dyn("50088-J1-度量×30一致", ok, &det);

    // J1-2 归一化口径正确：2048→1000 后源与渲染逐项相等
    let mut norm_ok = normalize_units(1638, 2048) == 800
        && normalize_units(-410, 2048) == -200
        && normalize_units(184, 2048) == 90;
    let mut i = 0usize;
    while i < src.advances.len() {
        if let Some(a) = src.advances.get(i) {
            if let Some(b) = out.lookup_advance(a.gid) {
                if normalize_units(a.advance as i32, src.upem) != b as i32 {
                    norm_ok = false;
                }
            } else {
                norm_ok = false;
            }
        }
        i += 1;
    }
    let det = format!("asc={} desc={}", normalize_units(1638, 2048), normalize_units(-410, 2048));
    s.add_dyn("50088-J1-归一化口径", norm_ok, &det);

    // J1-3 行高闭式：源与渲染归一后相等，且等于三项组合
    let src_lh = normalize_units(src.line_height_units(), src.upem);
    let out_lh = normalize_units(out.line_height_units(), out.upem);
    let sum = normalize_units(src.ascent as i32, src.upem)
        - normalize_units(src.descent as i32, src.upem)
        + normalize_units(src.line_gap as i32, src.upem);
    let ok = src_lh == out_lh && out_lh == sum && out_lh == 1090;
    let det = format!("src={} out={} sum={}", src_lh, out_lh, sum);
    s.add_dyn("50088-J1-行高一致", ok, &det);

    // J1-4 单步进扰动 +1 必须精确定位（且只有一行越差）
    let bad = render_from(&src);
    let gid = 7u16;
    let old = bad.lookup_advance(gid);
    let mut rebuilt = match FontMetrics::new(bad.upem, bad.ascent, bad.descent, bad.line_gap) {
        Ok(v) => v,
        Err(_) => bad.clone(),
    };
    let mut idx = 0usize;
    while idx < bad.advances.len() {
        if let Some(a) = bad.advances.get(idx) {
            let adv = if a.gid == gid {
                match old {
                    Some(v) => v + 1,
                    None => a.advance,
                }
            } else {
                a.advance
            };
            let _ = rebuilt.add_advance(a.gid, adv);
        }
        idx += 1;
    }
    let mut kk = 0usize;
    while kk < bad.kerning.len() {
        if let Some(p) = bad.kerning.get(kk) {
            let _ = rebuilt.add_kern(p.left, p.right, p.value);
        }
        kk += 1;
    }
    let mut lb = MetricsLedger::new();
    let cmp = lb.compare(&src, &rebuilt);
    let located = lb.first_mismatch();
    let ok = cmp.is_ok()
        && lb.mismatches == 1
        && match located {
            Some(d) => d.kind == MetricKind::Advance && d.gid == gid && d.delta == 1 && !d.within,
            None => false,
        }
        && lb.strict().err() == Some(MetricsError::Mismatch);
    let det = format!("mism={} first={:?}", lb.mismatches, located.map(|d| d.index));
    s.add_dyn("50088-J1-单点扰动定位", ok, &det);

    // J1-5 容差真实生效：垂直 +1 通过，+2 判红且 delta=2
    let mut tol1 = match FontMetrics::new(
        out.upem,
        out.ascent + 1,
        out.descent,
        out.line_gap,
    ) {
        Ok(v) => v,
        Err(_) => out.clone(),
    };
    let mut a1 = 0usize;
    while a1 < out.advances.len() {
        if let Some(a) = out.advances.get(a1) {
            let _ = tol1.add_advance(a.gid, a.advance);
        }
        a1 += 1;
    }
    let mut k1 = 0usize;
    while k1 < out.kerning.len() {
        if let Some(p) = out.kerning.get(k1) {
            let _ = tol1.add_kern(p.left, p.right, p.value);
        }
        k1 += 1;
    }
    let mut l1 = MetricsLedger::new();
    let c1 = l1.compare(&src, &tol1);
    let mut tol2 = match FontMetrics::new(out.upem, out.ascent + 2, out.descent, out.line_gap) {
        Ok(v) => v,
        Err(_) => out.clone(),
    };
    let mut a2 = 0usize;
    while a2 < out.advances.len() {
        if let Some(a) = out.advances.get(a2) {
            let _ = tol2.add_advance(a.gid, a.advance);
        }
        a2 += 1;
    }
    let mut k2 = 0usize;
    while k2 < out.kerning.len() {
        if let Some(p) = out.kerning.get(k2) {
            let _ = tol2.add_kern(p.left, p.right, p.value);
        }
        k2 += 1;
    }
    let mut l2 = MetricsLedger::new();
    let c2 = l2.compare(&src, &tol2);
    let ok = c1.is_ok()
        && l1.mismatches == 0
        && c2.is_ok()
        && l2.mismatches == 1
        && l1.max_abs_delta == 1;
    let det = format!("tol1={} tol2={}", l1.mismatches, l2.mismatches);
    s.add_dyn("50088-J1-容差生效", ok, &det);

    // J1-6 行集合缺失必须 Truncated，而不是少比几行
    let mut short = match FontMetrics::new(out.upem, out.ascent, out.descent, out.line_gap) {
        Ok(v) => v,
        Err(_) => out.clone(),
    };
    let mut q = 0usize;
    while q + 1 < out.advances.len() {
        if let Some(a) = out.advances.get(q) {
            let _ = short.add_advance(a.gid, a.advance);
        }
        q += 1;
    }
    let mut ks = 0usize;
    while ks < out.kerning.len() {
        if let Some(p) = out.kerning.get(ks) {
            let _ = short.add_kern(p.left, p.right, p.value);
        }
        ks += 1;
    }
    let mut ls = MetricsLedger::new();
    let ok = ls.compare(&src, &short).err() == Some(MetricsError::Truncated);
    s.add("50088-J1-缺行必拒", ok, if ok { "" } else { "缺行被放过" });

    // J1-7 解析往返：编码→解析后与源逐字段相等
    let blob = encode_metrics(&src);
    let rt = parse_metrics(&blob);
    let ok = match rt {
        Ok(m) => {
            m == src
                && m.advances.len() == 19
                && m.kerning.len() == 8
                && m.lookup_advance(21) == src.lookup_advance(21)
                && m.lookup_kern(3, 5) == src.lookup_kern(3, 5)
        }
        Err(_) => false,
    };
    s.add("50088-J1-编码往返", ok, if ok { "" } else { "往返不等" });

    // J1-8 插入纪律：重复 gid / 乱序 gid / 越界值 / 容量满
    let mut order_err = true;
    let mut m2 = match FontMetrics::new(1000, 800, -200, 0) {
        Ok(v) => v,
        Err(_) => src.clone(),
    };
    let a = m2.add_advance(5, 500).is_ok();
    let dup = m2.add_advance(5, 600).err();
    let ord = m2.add_advance(4, 400).err();
    let rng = m2.add_advance(6, 9000).err();
    let ok = a
        && dup == Some(MetricsError::Duplicate)
        && ord == Some(MetricsError::Order)
        && rng == Some(MetricsError::Range)
        && m2.advances.len() == 1;
    if !ok {
        order_err = false;
    }
    let det = format!("dup={:?} ord={:?} rng={:?}", dup, ord, rng);
    s.add_dyn("50088-J1-插入纪律", ok && order_err, &det);

    // J1-9 字距二分：8 对全部可查，未登记对返回 None（与 0 区分）
    let mut found = 0usize;
    let mut j = 0u16;
    while j < 8 {
        let left = 3 + j * 2;
        if src.lookup_kern(left, left + 2).is_some() {
            found += 1;
        }
        j += 1;
    }
    let ok = found == 8 && src.lookup_kern(100, 101).is_none() && src.lookup_advance(999).is_none();
    let det = format!("found={}", found);
    s.add_dyn("50088-J1-二分查表", ok, &det);

    // J1-10 封账状态机：封账后拒绝再比，二次封账拒绝
    let mut sealed = MetricsLedger::new();
    let c = sealed.compare(&src, &out);
    let s1 = sealed.seal();
    let again = sealed.compare(&src, &out).err();
    let s2 = sealed.seal().err();
    let ok = c.is_ok()
        && s1.is_ok()
        && again == Some(MetricsError::Order)
        && s2 == Some(MetricsError::Order)
        && sealed.state == LedgerState::Sealed;
    s.add("50088-J1-封账只读", ok, if ok { "" } else { "封账后仍可改写" });

    // J1-11 确定性：编码与差分两次逐字段一致
    let blob2 = encode_metrics(&src);
    let mut lx = MetricsLedger::new();
    let mut ly = MetricsLedger::new();
    let cx = lx.compare(&src, &out);
    let cy = ly.compare(&src, &out);
    let ok = blob == blob2 && cx.is_ok() && cy.is_ok() && lx.rows() == ly.rows() && lx.summary() == ly.summary();
    let det = format!("rows={}", lx.rows().len());
    s.add_dyn("50088-J1-确定性", ok, &det);

    // J1-12 摘要非空且字段可审计
    let text = ledger.summary();
    let ok = !text.is_empty() && text.contains("rows=30") && text.contains("mismatches=0");
    s.add("50088-J1-摘要非空", ok, if ok { "" } else { "摘要缺字段" });

    // J1-13 错误码闭集：九个码连续且消息非空
    let all = [
        MetricsError::Empty,
        MetricsError::BadHeader,
        MetricsError::BadNumber,
        MetricsError::Duplicate,
        MetricsError::Order,
        MetricsError::Truncated,
        MetricsError::Range,
        MetricsError::Mismatch,
        MetricsError::Full,
    ];
    let mut e = 0usize;
    let mut ok = true;
    while e < all.len() {
        if let Some(err) = all.get(e) {
            if err.code() != SEG_BASE + (e as u32) + 1 || err.message().is_empty() {
                ok = false;
            }
        }
        e += 1;
    }
    s.add("50088-J1-码值闭集", ok, if ok { "" } else { "码值漂移" });

    // J1-14 注入核八类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50088-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50088_checks_green() {
        let set = run_unx50088_checks();
        assert!(set.all_passed(), "UNX-F50088 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50088_thirty_rows_green() {
        let src = build_source();
        let out = render_from(&src);
        let mut l = MetricsLedger::new();
        assert!(l.compare(&src, &out).is_ok());
        assert_eq!(l.rows().len(), 30);
        assert_eq!(l.mismatches, 0);
        assert!(l.strict().is_ok());
    }

    #[test]
    fn unx50088_roundtrip() {
        let src = build_source();
        let back = parse_metrics(&encode_metrics(&src));
        assert!(back.is_ok());
        match back {
            Ok(m) => assert_eq!(m, src),
            Err(_) => panic!("roundtrip must parse"),
        }
    }
}
