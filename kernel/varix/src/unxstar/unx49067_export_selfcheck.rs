//! UNX-F49067 账本导出面收口（四段三列导出对齐 O1 口径终验）· 0x73340
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B14 族第四槽）：
//!   UNX-F49067 账本导出面收口（四段三列导出对齐 O1 口径终验）:
//!          ① 纯功能代码 ≥320 行；
//!          ② 判据原文：UNX-F49068-J1 常驻 ×10 周期零漏报；
//!             判据锚定 体验十三·补。
//!   UNX-F49068 账本自检运营化（时钟源健康/TSC 漂移周期探测常驻）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49069-J1 越限 ×10 告警三要素齐全；
//!             判据锚定 体验九。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（导出面）：取值 → 单调 → 布局 → 摘要 → 十轮。
//! 取值先于单调：三列（P50/P95/P99）各自先过定义域，才谈三列之间的次序；
//! 单调先于布局：分位必须 P50≤P95≤P99，否则"四段三列"只是十二个数字；
//! 布局先于摘要：键序与列序固定成 O1 口径之后，摘要才折得进同一域；
//! 摘要先于十轮：单轮摘要自洽之后，十轮一致才有"终验"的意义；
//!   十轮一致的判据是**逐行**比对，不是只比一个总摘要——
//!   总摘要相同而两行互换（P50 与 P95 对调）同样会被单调闸门抓住，
//!   但只有逐行比对能把"换行"与"算错"分开。
//! 闸门次序（自检常驻）：周期 → 探针 → 判定 → 告警。
//! 周期先于探针：每一周期必须恰好落一枚探针，漏一拍即判红（MissedProbe）；
//!   常驻的意义在"零漏报"，不在"总探针数够多"——丢周期补探针是自欺；
//! 探针先于判定：先有观测值（ppm），再谈健康/预警/失准三档；
//! 判定先于告警：越限判定成立才出告警，且告警必须三要素齐全
//!   （阈值、观测量、处置动作）——缺一要素的告警等于没告。
//! 关键差异：F49067 是**输出面**（把账本变成 O1 可读的十二格），
//! F49068 是**输入面**（把时钟健康变成可运营的周期探针）。
//! 两本账摘要域独立：导出摘要折四段十二格，自检摘要折周期/探针/告警。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73340，B14 族第四槽）。
pub const SEG_BASE: u32 = 0x73340;

/// 延迟链四段。
pub const SEGMENTS: usize = 4;
/// 导出三列。
pub const COLUMNS: usize = 3;
/// 导出格数（四段 × 三列）。
pub const CELLS: usize = SEGMENTS * COLUMNS;
/// 自检周期数（判据口径 ×10）。
pub const CYCLES: u32 = 10;
/// 十轮导出（终验口径）。
pub const EXPORT_ROUNDS: u32 = 10;
/// 计时值上限（1 秒，纳秒）。
pub const NS_LIMIT: u32 = 1_000_000_000;
/// 健康刻度（千分比）。
pub const HEALTH_FULL: u32 = 1_000;
/// 健康跌落预算（千分比）：相对该时钟源基线允许的最大下落。
pub const HEALTH_DROP_BP: u32 = 260;
/// 漂移健康阈值（ppm 绝对值）。
pub const DRIFT_OK_PPM: u32 = 50;
/// 漂移预警阈值（ppm 绝对值）。
pub const DRIFT_WARN_PPM: u32 = 200;
/// 探针抖动上限（ppm）。
pub const DRIFT_LIMIT_PPM: u32 = 400;
/// 四段基准延迟（纳秒）。
pub const SEG_BASE_NS: [u32; SEGMENTS] = [760, 1_180, 2_720, 4_560];
/// 四段分位放大（千分比）：P95 = P50×1500‰，P99 = P50×1800‰。
pub const P95_RATIO_PERMILLE: u32 = 1_500;
/// P99 放大刻度。
pub const P99_RATIO_PERMILLE: u32 = 1_800;
/// 千分比刻度。
pub const PERMILLE: u32 = 1_000;
/// 账行字段数。
pub const ROW_FIELDS: usize = 6;
/// 导出账行标签（O1 口径）。
pub const EXPORT_TAG: &str = "O1EXP";
/// 自检账行标签。
pub const CHECK_TAG: &str = "SLFCHK";
/// 告警账行标签。
pub const ALARM_TAG: &str = "ALARM";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B14 导出/自检诊断码";
/// 段名。
pub const SEG_NAMES: [&str; SEGMENTS] = ["interrupt", "dispatch", "compose", "present"];
/// 三列名。
pub const COLUMN_NAMES: [&str; COLUMNS] = ["p50", "p95", "p99"];
/// 导出行的数字键（固定列序）。
pub const EXPORT_KEYS: [&str; 4] = ["seg", "p50", "p95", "p99"];

// ── 诊断码 ───────────────────────────────────────────────────────
/// 导出/自检诊断码（14 条，互不共用说明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpCode {
    /// 段号越出 0..3
    SegBad,
    /// 列号越出 0..2
    ColumnBad,
    /// 三列次序破坏（P50≤P95≤P99 不成立）
    MonotoneBad,
    /// 导出摘要与十二格不符
    DigestBad,
    /// 账行键序错位、数字非法或往返不等
    RenderBad,
    /// 周期数为 0 或超过 10
    CycleBad,
    /// 探针周期号错位
    ProbeBad,
    /// 周期内漏探针
    MissedProbe,
    /// 时钟源健康分越界
    ClockBad,
    /// 漂移观测越出探针量程
    DriftBad,
    /// 告警三要素缺失
    AlarmBad,
    /// 导出十轮不一致
    ExportBad,
    /// 健康分与探针不守恒
    HealthBad,
    /// 计数累计溢出
    Overflow,
}

impl ExpCode {
    /// 诊断码值：段基址 + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码中文说明（不得共用同一句）。
    pub fn detail(self) -> &'static str {
        match self {
            ExpCode::SegBad => "段号不在 0..3，导出格无主",
            ExpCode::ColumnBad => "列号不在 0..2，三列口径错位",
            ExpCode::MonotoneBad => "P50≤P95≤P99 单调被破坏",
            ExpCode::DigestBad => "导出摘要与四段十二格不符",
            ExpCode::RenderBad => "账行键序错位、数字非法或往返不等",
            ExpCode::CycleBad => "自检周期为 0 或超过 10 拍",
            ExpCode::ProbeBad => "探针周期号与拍序错位",
            ExpCode::MissedProbe => "某一周期未落探针，常驻断拍",
            ExpCode::ClockBad => "时钟源健康分越出 0..1000",
            ExpCode::DriftBad => "漂移观测越出 ±260 ppm 探针量程",
            ExpCode::AlarmBad => "告警缺阈值/观测量/处置动作之一",
            ExpCode::ExportBad => "导出十轮逐行不一致",
            ExpCode::HealthBad => "健康分与探针观测不守恒",
            ExpCode::Overflow => "周期或格数累计溢出",
        }
    }

    /// 14 个诊断码全集（顺序即变体顺序）。
    pub const ALL: [ExpCode; 14] = [
        ExpCode::SegBad,
        ExpCode::ColumnBad,
        ExpCode::MonotoneBad,
        ExpCode::DigestBad,
        ExpCode::RenderBad,
        ExpCode::CycleBad,
        ExpCode::ProbeBad,
        ExpCode::MissedProbe,
        ExpCode::ClockBad,
        ExpCode::DriftBad,
        ExpCode::AlarmBad,
        ExpCode::ExportBad,
        ExpCode::HealthBad,
        ExpCode::Overflow,
    ];
}

/// 自由函数版诊断说明，越界返回 [`UNKNOWN`]。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x0000 => UNKNOWN,
        _ => {
            let idx = code.wrapping_sub(SEG_BASE + 1);
            match ExpCode::ALL.get(idx as usize) {
                Some(c) => c.detail(),
                None => UNKNOWN,
            }
        }
    }
}

/// 段位守卫：14 码恰好占满 [SEG_BASE+1, SEG_BASE+14]，且互不重复。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0E;
    let mut i = 0usize;
    while i < ExpCode::ALL.len() {
        let v = match ExpCode::ALL.get(i) {
            Some(c) => c.val(),
            None => return false,
        };
        if v < lo || v > hi {
            return false;
        }
        let mut j = i + 1;
        while j < ExpCode::ALL.len() {
            if let Some(other) = ExpCode::ALL.get(j) {
                if other.val() == v {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 把 64 位摘要折入 64 位累加器（高低两半各折一次，避免截断丢信息）。
fn fold_digest(h: u64, v: u64) -> u64 {
    p_core::fold_u32(p_core::fold_u32(h, (v >> 32) as u32), v as u32)
}

// ── 列与格 ───────────────────────────────────────────────────────
/// 导出列（三列固定序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    /// 中位
    P50,
    /// 尾部 95%
    P95,
    /// 尾部 99%
    P99,
}

impl Column {
    /// 列号。
    pub fn index(self) -> u8 {
        match self {
            Column::P50 => 0,
            Column::P95 => 1,
            Column::P99 => 2,
        }
    }

    /// 列名。
    pub fn name(self) -> &'static str {
        match COLUMN_NAMES.get(self.index() as usize) {
            Some(v) => v,
            None => "?",
        }
    }

    /// 由列号反查。
    pub fn from_index(i: u8) -> Result<Column, u32> {
        match i {
            0 => Ok(Column::P50),
            1 => Ok(Column::P95),
            2 => Ok(Column::P99),
            _ => Err(ExpCode::ColumnBad.val()),
        }
    }
}

/// 段名；越界返回 `"?"`。
pub fn seg_name(i: u8) -> &'static str {
    match SEG_NAMES.get(i as usize) {
        Some(v) => v,
        None => "?",
    }
}

/// 单段三格：P50/P95/P99。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Triple {
    /// 中位。
    pub p50: u32,
    /// P95。
    pub p95: u32,
    /// P99。
    pub p99: u32,
}

impl Triple {
    /// 校验并构造：三值均在 (0, 1s] 且单调。
    pub fn new(p50: u32, p95: u32, p99: u32) -> Result<Triple, u32> {
        if p50 == 0 || p99 > NS_LIMIT {
            return Err(ExpCode::MonotoneBad.val());
        }
        if p50 > p95 || p95 > p99 {
            return Err(ExpCode::MonotoneBad.val());
        }
        Ok(Triple { p50, p95, p99 })
    }

    /// 单列取值。
    pub fn get(&self, col: Column) -> u32 {
        match col {
            Column::P50 => self.p50,
            Column::P95 => self.p95,
            Column::P99 => self.p99,
        }
    }

    /// 单调。
    pub fn monotone(&self) -> bool {
        self.p50 <= self.p95 && self.p95 <= self.p99 && self.p50 > 0
    }

    /// 尾比（P99/P50，千分比）。
    pub fn tail_ratio_permille(&self) -> u32 {
        if self.p50 == 0 {
            return 0;
        }
        let bp = (self.p99 as u64)
            .saturating_mul(PERMILLE as u64)
            .checked_div(self.p50 as u64)
            .unwrap_or(0);
        if bp > u32::MAX as u64 {
            u32::MAX
        } else {
            bp as u32
        }
    }
}

// ── 导出面 ───────────────────────────────────────────────────────
/// 导出表：四段三列 + 十二格摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExportSheet {
    /// 四段三元组（下标即段号）。
    pub rows: [Triple; SEGMENTS],
    /// 十二格摘要。
    pub digest: u64,
}

impl ExportSheet {
    /// 校验并构造：四段全单调。
    pub fn new(rows: [Triple; SEGMENTS]) -> Result<ExportSheet, u32> {
        let mut i = 0usize;
        while i < SEGMENTS {
            match rows.get(i) {
                Some(t) => {
                    if !t.monotone() {
                        return Err(ExpCode::MonotoneBad.val());
                    }
                }
                None => return Err(ExpCode::SegBad.val()),
            }
            i += 1;
        }
        let digest = sheet_digest(&rows);
        Ok(ExportSheet { rows, digest })
    }

    /// 段行只读。
    pub fn row(&self, seg: u8) -> Result<Triple, u32> {
        match self.rows.get(seg as usize) {
            Some(v) => Ok(*v),
            None => Err(ExpCode::SegBad.val()),
        }
    }

    /// 单格取值。
    pub fn cell(&self, seg: u8, col: Column) -> Result<u32, u32> {
        match self.rows.get(seg as usize) {
            Some(t) => Ok(t.get(col)),
            None => Err(ExpCode::SegBad.val()),
        }
    }

    /// 格数。
    pub fn cells(&self) -> usize {
        SEGMENTS.saturating_mul(COLUMNS)
    }

    /// 摘要自洽。
    pub fn verify(&self) -> bool {
        sheet_digest(&self.rows) == self.digest
    }

    /// 全表单调。
    pub fn monotone(&self) -> bool {
        let mut i = 0usize;
        while i < SEGMENTS {
            match self.rows.get(i) {
                Some(t) => {
                    if !t.monotone() {
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

/// 十二格摘要：折段号与三列。
pub fn sheet_digest(rows: &[Triple; SEGMENTS]) -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < SEGMENTS {
        if let Some(t) = rows.get(i) {
            h = p_core::fold_byte(h, i as u8);
            h = p_core::fold_u32(h, t.p50);
            h = p_core::fold_u32(h, t.p95);
            h = p_core::fold_u32(h, t.p99);
        }
        i += 1;
    }
    h
}

/// 单行摘要：折段号与三列（与整表摘要同域，单行是其子折）。
pub fn export_row_digest(seg: u32, p50: u32, p95: u32, p99: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, seg);
    h = p_core::fold_u32(h, p50);
    h = p_core::fold_u32(h, p95);
    h = p_core::fold_u32(h, p99);
    h
}

/// 渲染 O1 口径导出账行：`O1EXP|seg=|p50=|p95=|p99=|digest=`。
pub fn render_row(tag: &str, seg: u32, p50: u32, p95: u32, p99: u32, digest: u64) -> String {
    format!(
        "{}|seg={}|p50={}|p95={}|p99={}|digest={:016X}",
        tag, seg, p50, p95, p99, digest
    )
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

/// 拆 `key=value`。
fn split_kv(p: &str) -> Option<(&str, &str)> {
    let mut it = p.split('=');
    let k = it.next()?;
    let v = it.next()?;
    if it.next().is_some() {
        return None;
    }
    if k.is_empty() || v.is_empty() {
        return None;
    }
    Some((k, v))
}

/// 按固定键序解析六字段行。
pub fn fields_with_keys<'a>(
    row: &'a str,
    keys: &[&str; 4],
) -> Option<(&'a str, [u32; 4], u64)> {
    let mut parts: [Option<&str>; ROW_FIELDS] = [None; ROW_FIELDS];
    let mut n = 0usize;
    for p in row.split('|') {
        if n >= ROW_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != ROW_FIELDS {
        return None;
    }
    let tag = match parts.first().and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return None,
    };
    let mut vals = [0u32; 4];
    let mut i = 0usize;
    while i < 4 {
        let p = match parts.get(i + 1).and_then(|x| *x) {
            Some(v) => v,
            None => return None,
        };
        let (k, v) = split_kv(p)?;
        if k != *keys.get(i)? {
            return None;
        }
        let num = parse_u32(v)?;
        if let Some(slot) = vals.get_mut(i) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return None,
    };
    let (k, v) = split_kv(dpart)?;
    if k != "digest" {
        return None;
    }
    let digest = parse_hex16(v)?;
    Some((tag, vals, digest))
}

/// 导出行解析。
pub fn row_fields<'a>(row: &'a str) -> Option<(&'a str, [u32; 4], u64)> {
    fields_with_keys(row, &EXPORT_KEYS)
}

/// 导出行审计：标签 + 键序 + 段界 + 三列单调 + 摘要重算 + 往返恒等。
pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, v, digest)) => {
            let seg = v[0];
            let t = match Triple::new(v[1], v[2], v[3]) {
                Ok(x) => x,
                Err(_) => return false,
            };
            tag == EXPORT_TAG
                && seg < SEGMENTS as u32
                && digest == export_row_digest(seg, t.p50, t.p95, t.p99)
                && render_row(tag, seg, t.p50, t.p95, t.p99, digest) == row
        }
        None => false,
    }
}

/// 渲染整表四行（段序 0..3）。
pub fn export_rows(sheet: &ExportSheet) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(SEGMENTS);
    let mut i = 0usize;
    while i < SEGMENTS {
        if let Some(t) = sheet.rows.get(i) {
            let seg = i as u32;
            let d = export_row_digest(seg, t.p50, t.p95, t.p99);
            out.push(render_row(EXPORT_TAG, seg, t.p50, t.p95, t.p99, d));
        }
        i += 1;
    }
    out
}

/// 导出十轮逐行一致。
pub fn export_stable(sheet: &ExportSheet, rounds: u32) -> bool {
    if rounds == 0 {
        return false;
    }
    let base = export_rows(sheet);
    if base.len() != SEGMENTS {
        return false;
    }
    let mut r = 0u32;
    while r < rounds {
        let again = export_rows(sheet);
        if again.len() != base.len() {
            return false;
        }
        let mut i = 0usize;
        while i < base.len() {
            match (base.get(i), again.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        r += 1;
    }
    true
}

/// 由四段基准构表（P95/P99 按固定放大刻度，抖动取高位）。
pub fn synth_sheet(seed: u64) -> Result<ExportSheet, u32> {
    let mut rng = p_core::Lcg::new(seed ^ 0x4558_5053_4845_4554);
    let mut rows = [Triple {
        p50: 1,
        p95: 1,
        p99: 1,
    }; SEGMENTS];
    let mut i = 0usize;
    while i < SEGMENTS {
        let base = match SEG_BASE_NS.get(i) {
            Some(v) => *v,
            None => return Err(ExpCode::SegBad.val()),
        };
        let jitter = ((rng.next() >> 40) as u32) % (base / 8 + 1);
        let p50 = base.saturating_add(jitter);
        let p95 = (p50 as u64)
            .saturating_mul(P95_RATIO_PERMILLE as u64)
            .checked_div(PERMILLE as u64)
            .ok_or(ExpCode::Overflow.val())? as u32;
        let p99 = (p50 as u64)
            .saturating_mul(P99_RATIO_PERMILLE as u64)
            .checked_div(PERMILLE as u64)
            .ok_or(ExpCode::Overflow.val())? as u32;
        let t = Triple::new(p50, p95, p99)?;
        if let Some(slot) = rows.get_mut(i) {
            *slot = t;
        }
        i += 1;
    }
    ExportSheet::new(rows)
}

// ── 时钟源与漂移 ─────────────────────────────────────────────────
/// 时钟源（自检对象）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockSource {
    /// 高精度事件定时器
    Hpet,
    /// 时间戳计数器
    Tsc,
    /// 可编程间隔定时器
    Pit,
}

impl ClockSource {
    /// 源号。
    pub fn index(self) -> u8 {
        match self {
            ClockSource::Hpet => 0,
            ClockSource::Tsc => 1,
            ClockSource::Pit => 2,
        }
    }

    /// 源名。
    pub fn name(self) -> &'static str {
        match self {
            ClockSource::Hpet => "hpet",
            ClockSource::Tsc => "tsc",
            ClockSource::Pit => "pit",
        }
    }

    /// 基线健康分（千分比）：HPET 最稳，TSC 依赖变频，PIT 精度有限。
    pub fn baseline_health(self) -> u32 {
        match self {
            ClockSource::Hpet => 990,
            ClockSource::Tsc => 880,
            ClockSource::Pit => 720,
        }
    }

    /// 由源号反查。
    pub fn from_index(i: u8) -> Result<ClockSource, u32> {
        match i {
            0 => Ok(ClockSource::Hpet),
            1 => Ok(ClockSource::Tsc),
            2 => Ok(ClockSource::Pit),
            _ => Err(ExpCode::ClockBad.val()),
        }
    }
}

/// 漂移判定三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriftClass {
    /// 健康：|ppm| ≤ 50
    Healthy,
    /// 预警：50 < |ppm| ≤ 200
    Warning,
    /// 失准：|ppm| > 200
    Impaired,
}

impl DriftClass {
    /// 判定。
    pub fn classify(ppm: i32) -> DriftClass {
        let mag = ppm.unsigned_abs();
        if mag <= DRIFT_OK_PPM {
            DriftClass::Healthy
        } else if mag <= DRIFT_WARN_PPM {
            DriftClass::Warning
        } else {
            DriftClass::Impaired
        }
    }

    /// 序号。
    pub fn index(self) -> u8 {
        match self {
            DriftClass::Healthy => 0,
            DriftClass::Warning => 1,
            DriftClass::Impaired => 2,
        }
    }

    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            DriftClass::Healthy => "healthy",
            DriftClass::Warning => "warning",
            DriftClass::Impaired => "impaired",
        }
    }
}

/// 单拍探针：周期号 + 观测漂移（ppm）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    /// 周期号（0..9）。
    pub cycle: u8,
    /// 观测漂移（ppm，带符号）。
    pub ppm: i32,
    /// 判定。
    pub class: DriftClass,
}

impl Probe {
    /// 校验并构造。
    pub fn new(cycle: u8, ppm: i32) -> Result<Probe, u32> {
        if (cycle as u32) >= CYCLES {
            return Err(ExpCode::ProbeBad.val());
        }
        if ppm.unsigned_abs() > DRIFT_LIMIT_PPM {
            return Err(ExpCode::DriftBad.val());
        }
        Ok(Probe {
            cycle,
            ppm,
            class: DriftClass::classify(ppm),
        })
    }
}

/// 自检监视器：十拍探针 + 缺席计数。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Monitor {
    /// 计划周期数。
    pub cycles: u32,
    /// 探针序列（按周期序）。
    pub probes: Vec<Probe>,
    /// 漏拍数。
    pub missed: u32,
    /// 摘要。
    pub digest: u64,
}

impl Monitor {
    /// 逐拍落入一枚探针；周期号必须等于当前拍序。
    pub fn observe(&mut self, p: Probe) -> Result<(), u32> {
        if self.cycles == 0 || self.cycles > CYCLES {
            return Err(ExpCode::CycleBad.val());
        }
        if p.cycle as u32 != self.probes.len() as u32 {
            return Err(ExpCode::ProbeBad.val());
        }
        if self.probes.len() >= self.cycles as usize {
            return Err(ExpCode::CycleBad.val());
        }
        self.probes.push(p);
        self.digest = monitor_digest(self.cycles, &self.probes, self.missed);
        Ok(())
    }

    /// 拍数。
    pub fn len(&self) -> usize {
        self.probes.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.probes.is_empty()
    }

    /// 覆盖率（千分比）。
    pub fn coverage_permille(&self) -> u32 {
        if self.cycles == 0 {
            return 0;
        }
        let n = (self.probes.len() as u64)
            .saturating_mul(PERMILLE as u64)
            .checked_div(self.cycles as u64)
            .unwrap_or(0);
        if n > u32::MAX as u64 {
            u32::MAX
        } else {
            n as u32
        }
    }

    /// 零漏报：拍数齐、周期号连续、未记漏拍。
    pub fn zero_missed(&self) -> bool {
        if self.missed != 0 || self.probes.len() != self.cycles as usize {
            return false;
        }
        let mut i = 0usize;
        while i < self.probes.len() {
            match self.probes.get(i) {
                Some(p) => {
                    if p.cycle as usize != i {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 摘要自洽。
    pub fn verify(&self) -> bool {
        monitor_digest(self.cycles, &self.probes, self.missed) == self.digest
    }

    /// 各档计数：(healthy, warning, impaired)。
    pub fn class_counts(&self) -> (u32, u32, u32) {
        let mut h = 0u32;
        let mut w = 0u32;
        let mut i = 0u32;
        let mut k = 0usize;
        while k < self.probes.len() {
            if let Some(p) = self.probes.get(k) {
                match p.class {
                    DriftClass::Healthy => h = h.saturating_add(1),
                    DriftClass::Warning => w = w.saturating_add(1),
                    DriftClass::Impaired => i = i.saturating_add(1),
                }
            }
            k += 1;
        }
        (h, w, i)
    }

    /// 最大绝对漂移。
    pub fn max_abs_ppm(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0usize;
        while i < self.probes.len() {
            if let Some(p) = self.probes.get(i) {
                let v = p.ppm.unsigned_abs();
                if v > m {
                    m = v;
                }
            }
            i += 1;
        }
        m
    }
}

/// 监视器摘要：折周期数、逐拍周期/漂移/判定、漏拍数。
pub fn monitor_digest(cycles: u32, probes: &[Probe], missed: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, cycles);
    h = p_core::fold_u32(h, missed);
    let mut i = 0usize;
    while i < probes.len() {
        if let Some(p) = probes.get(i) {
            h = p_core::fold_byte(h, p.cycle);
            h = p_core::fold_u32(h, p.ppm as u32);
            h = p_core::fold_byte(h, p.class.index());
        }
        i += 1;
    }
    h
}

/// 跑一轮十拍常驻自检（每拍一枚探针，抖动用高位取符号）。
pub fn run_monitor(seed: u64) -> Result<Monitor, u32> {
    let mut m = Monitor {
        cycles: CYCLES,
        probes: Vec::with_capacity(CYCLES as usize),
        missed: 0,
        digest: p_core::FNV_OFF,
    };
    let mut rng = p_core::Lcg::new(seed ^ 0x4452_4946_5450_5242);
    let mut c = 0u32;
    while c < CYCLES {
        let raw = ((rng.next() >> 33) as u32) % (DRIFT_LIMIT_PPM * 2 + 1);
        let ppm = raw as i64 - DRIFT_LIMIT_PPM as i64;
        let ppm = if ppm == 0 { 1 } else { ppm };
        let clamped = if ppm > i32::MAX as i64 {
            i32::MAX
        } else if ppm < i32::MIN as i64 {
            i32::MIN
        } else {
            ppm as i32
        };
        m.observe(Probe::new(c as u8, clamped)?)?;
        c = c.saturating_add(1);
    }
    Ok(m)
}

// ── 告警三要素 ───────────────────────────────────────────────────
/// 告警处置动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlarmAction {
    /// 仅记录观察
    Observe,
    /// 限速时钟读数
    Throttle,
    /// 触发重标定
    Recalibrate,
}

impl AlarmAction {
    /// 动作序号。
    pub fn index(self) -> u8 {
        match self {
            AlarmAction::Observe => 0,
            AlarmAction::Throttle => 1,
            AlarmAction::Recalibrate => 2,
        }
    }

    /// 动作名。
    pub fn name(self) -> &'static str {
        match self {
            AlarmAction::Observe => "observe",
            AlarmAction::Throttle => "throttle",
            AlarmAction::Recalibrate => "recalibrate",
        }
    }

    /// 由判定档推动作：预警观察，失准重标定。
    pub fn for_class(c: DriftClass) -> AlarmAction {
        match c {
            DriftClass::Healthy => AlarmAction::Observe,
            DriftClass::Warning => AlarmAction::Observe,
            DriftClass::Impaired => AlarmAction::Recalibrate,
        }
    }
}

/// 越限告警：阈值 + 观测 + 动作，三要素缺一即为无效告警。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Alarm {
    /// 触发周期。
    pub cycle: u8,
    /// 阈值（ppm 绝对值）。
    pub threshold_ppm: u32,
    /// 观测量（ppm，带符号）。
    pub observed_ppm: i32,
    /// 处置动作。
    pub action: AlarmAction,
}

impl Alarm {
    /// 三要素齐全：阈值 > 0、观测非零、动作非空。
    pub fn three_elements(&self) -> bool {
        self.threshold_ppm > 0 && self.observed_ppm != 0 && self.action.index() <= 2
    }

    /// 越限成立：|观测| > 阈值。
    pub fn exceeded(&self) -> bool {
        self.observed_ppm.unsigned_abs() > self.threshold_ppm
    }

    /// 摘要。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_byte(h, self.cycle);
        h = p_core::fold_u32(h, self.threshold_ppm);
        h = p_core::fold_u32(h, self.observed_ppm as u32);
        h = p_core::fold_byte(h, self.action.index());
        h
    }
}

/// 由监视器收集越限告警（阈值取预警线 200 ppm）。
pub fn collect_alarms(m: &Monitor) -> Vec<Alarm> {
    let mut out: Vec<Alarm> = Vec::new();
    let mut i = 0usize;
    while i < m.probes.len() {
        if let Some(p) = m.probes.get(i) {
            if p.ppm.unsigned_abs() > DRIFT_WARN_PPM {
                out.push(Alarm {
                    cycle: p.cycle,
                    threshold_ppm: DRIFT_WARN_PPM,
                    observed_ppm: p.ppm,
                    action: AlarmAction::for_class(p.class),
                });
            }
        }
        i += 1;
    }
    out
}

/// 告警账行：`ALARM|cycle=|thr=|obs=|act=|digest=`（六字段，数字四格）。
pub fn render_alarm(a: &Alarm) -> String {
    format!(
        "{}|cycle={}|thr={}|obs={}|act={}|digest={:016X}",
        ALARM_TAG,
        a.cycle,
        a.threshold_ppm,
        a.observed_ppm.unsigned_abs(),
        a.action.index(),
        a.digest()
    )
}

/// 自检结果：监视器 + 时钟源 + 健康分 + 告警集。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SelfCheck {
    /// 监视器。
    pub monitor: Monitor,
    /// 时钟源。
    pub source: ClockSource,
    /// 健康分（千分比）。
    pub health_bp: u32,
    /// 越限告警。
    pub alarms: Vec<Alarm>,
    /// 摘要。
    pub digest: u64,
}

impl SelfCheck {
    /// 健康分推导：基线 − 最大漂移的一半（向下取整），下限 0。
    pub fn health_for(source: ClockSource, max_abs_ppm: u32) -> u32 {
        let base = source.baseline_health();
        let penalty = max_abs_ppm / 2;
        base.saturating_sub(penalty).min(HEALTH_FULL)
    }

    /// 健康分是否达标：相对该时钟源基线跌落不超过 `HEALTH_DROP_BP`。
    pub fn healthy(&self) -> bool {
        let floor = self.source.baseline_health().saturating_sub(HEALTH_DROP_BP);
        self.health_bp >= floor && self.health_bp <= HEALTH_FULL
    }

    /// 告警三要素全部齐全。
    pub fn alarms_complete(&self) -> bool {
        let mut i = 0usize;
        while i < self.alarms.len() {
            match self.alarms.get(i) {
                Some(a) => {
                    if !a.three_elements() || !a.exceeded() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 摘要自洽。
    pub fn verify(&self) -> bool {
        let mut h = p_core::FNV_OFF;
        h = fold_digest(h, self.monitor.digest);
        h = p_core::fold_byte(h, self.source.index());
        h = p_core::fold_u32(h, self.health_bp);
        let mut i = 0usize;
        while i < self.alarms.len() {
            if let Some(a) = self.alarms.get(i) {
                h = fold_digest(h, a.digest());
            }
            i += 1;
        }
        h == self.digest
    }
}

/// 跑一轮自检（十拍 + 健康分 + 告警）。
pub fn run_selfcheck(seed: u64, source: ClockSource) -> Result<SelfCheck, u32> {
    let monitor = run_monitor(seed)?;
    if !monitor.zero_missed() || !monitor.verify() {
        return Err(ExpCode::MissedProbe.val());
    }
    let health_bp = SelfCheck::health_for(source, monitor.max_abs_ppm());
    if health_bp > HEALTH_FULL {
        return Err(ExpCode::ClockBad.val());
    }
    let alarms = collect_alarms(&monitor);
    let mut h = p_core::FNV_OFF;
    h = fold_digest(h, monitor.digest);
    h = p_core::fold_byte(h, source.index());
    h = p_core::fold_u32(h, health_bp);
    let mut i = 0usize;
    while i < alarms.len() {
        if let Some(a) = alarms.get(i) {
            h = fold_digest(h, a.digest());
        }
        i += 1;
    }
    Ok(SelfCheck {
        monitor,
        source,
        health_bp,
        alarms,
        digest: h,
    })
}

/// 自检复跑一致。
pub fn selfcheck_replay(seed: u64, source: ClockSource, times: u32) -> bool {
    if times == 0 {
        return false;
    }
    let base = match run_selfcheck(seed, source) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut t = 0u32;
    while t < times {
        match run_selfcheck(seed, source) {
            Ok(v) => {
                if v.digest != base.digest
                    || v.health_bp != base.health_bp
                    || v.alarms.len() != base.alarms.len()
                    || v.monitor.len() != base.monitor.len()
                {
                    return false;
                }
            }
            Err(_) => return false,
        }
        t += 1;
    }
    true
}

/// 自检账行：`SLFCHK|cycles=|probes=|missed=|health=|digest=`。
pub fn render_check(s: &SelfCheck) -> String {
    format!(
        "{}|cycles={}|probes={}|missed={}|health={}|digest={:016X}",
        CHECK_TAG,
        s.monitor.cycles,
        s.monitor.len(),
        s.monitor.missed,
        s.health_bp,
        s.digest
    )
}

// ── 判据八条 ─────────────────────────────────────────────────────
/// 八条判据（F49067 四条 + F49068 四条）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::with_capacity(8);

    // 1) 导出四段三列：四行、十二格、全单调、摘要自洽、逐行可回读。
    let mut ok = true;
    let mut info = String::from("sheet failed");
    match synth_sheet(0x6701) {
        Ok(sheet) => {
            let rows = export_rows(&sheet);
            let mut cells = 0u32;
            let mut i = 0usize;
            while i < rows.len() {
                match rows.get(i) {
                    Some(row) => {
                        if !audit_render(row) {
                            ok = false;
                        }
                        cells = cells.saturating_add(COLUMNS as u32);
                    }
                    None => ok = false,
                }
                i += 1;
            }
            let mut seg = 0u8;
            while (seg as usize) < SEGMENTS {
                let t = match sheet.row(seg) {
                    Ok(v) => v,
                    Err(_) => {
                        ok = false;
                        Triple {
                            p50: 1,
                            p95: 1,
                            p99: 1,
                        }
                    }
                };
                if !t.monotone() {
                    ok = false;
                }
                let mut col = 0u8;
                while col < COLUMNS as u8 {
                    match Column::from_index(col) {
                        Ok(c) => {
                            if sheet.cell(seg, c).is_err() {
                                ok = false;
                            }
                        }
                        Err(_) => ok = false,
                    }
                    col = col.saturating_add(1);
                }
                seg = seg.saturating_add(1);
            }
            ok = ok && cells == CELLS as u32 && sheet.verify() && sheet.monotone();
            info = format!("rows={} cells={} mono={}", rows.len(), cells, sheet.monotone());
        }
        Err(_) => ok = false,
    }
    out.push(("F49067/导出四段三列", ok, info));

    // 2) 对齐 O1 口径：标签/键序/列序固定；改一位数字立即判红；坏行全拒。
    let mut ok = true;
    let mut tamper = 0u32;
    match synth_sheet(0x6702) {
        Ok(sheet) => {
            let rows = export_rows(&sheet);
            let mut i = 0usize;
            while i < rows.len() {
                match rows.get(i) {
                    Some(row) => {
                        match row_fields(row) {
                            Some((tag, v, d)) => {
                                if tag != EXPORT_TAG {
                                    ok = false;
                                }
                                let broken = render_row(
                                    EXPORT_TAG,
                                    v[0],
                                    v[1],
                                    v[2].saturating_add(1),
                                    v[3],
                                    d,
                                );
                                if audit_render(&broken) {
                                    ok = false;
                                } else {
                                    tamper = tamper.saturating_add(1);
                                }
                            }
                            None => ok = false,
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
        }
        Err(_) => ok = false,
    }
    let bad = [
        "",
        "O1EXP|seg=0|p50=1|p95=2|p99=3",
        "O1EXP|seg=0|p50=1|p95=2|p99=3|digest=0000000000000000",
        "O1EXP|p50=1|seg=0|p95=2|p99=3|digest=0000000000000000",
        "O1EXP|seg=4|p50=1|p95=2|p99=3|digest=0000000000000000",
        "O1EXP|seg=0|p50=3|p95=2|p99=3|digest=0000000000000000",
        "O1EXP|seg=0|p50=0|p95=2|p99=3|digest=0000000000000000",
        "O1EXP|seg=0|p50=1|p95=2|p99=3|digest=00000000000000zz",
        "|seg=0|p50=1|p95=2|p99=3|digest=0000000000000000",
    ];
    let mut refused = 0u32;
    let mut k = 0usize;
    while k < bad.len() {
        if let Some(row) = bad.get(k) {
            if !audit_render(row) {
                refused = refused.saturating_add(1);
            }
        }
        k += 1;
    }
    ok = ok && tamper == SEGMENTS as u32 && refused == bad.len() as u32;
    out.push((
        "F49067/对齐 O1 口径",
        ok,
        format!("tamper={} refuse={}", tamper, refused),
    ));

    // 3) 导出十轮：逐行十轮一致、段序 0..3、摘要非零且稳定。
    let mut ok = false;
    let mut info = String::from("sheet failed");
    if let Ok(sheet) = synth_sheet(0x6703) {
        let rows = export_rows(&sheet);
        let mut ordered = true;
        let mut i = 0usize;
        while i < rows.len() {
            match row_fields(row_at(&rows, i).as_str()) {
                Some((_t, v, _d)) => {
                    if v[0] != i as u32 {
                        ordered = false;
                    }
                }
                None => ordered = false,
            }
            i += 1;
        }
        ok = export_stable(&sheet, EXPORT_ROUNDS)
            && ordered
            && sheet.digest != 0
            && rows.len() == SEGMENTS;
        info = format!("rounds={} ordered={} digest={}", EXPORT_ROUNDS, ordered, sheet.digest != 0);
    }
    out.push(("F49067/导出十轮", ok, info));

    // 4) 导出异常面：段越界/单调破坏/量程越界逐码拒绝。
    let mut ok = true;
    let mut bits = String::new();
    if Triple::new(3, 2, 4).err() != Some(ExpCode::MonotoneBad.val()) {
        ok = false;
        bits.push_str("mono miss; ");
    }
    if Triple::new(1, 2, NS_LIMIT + 1).err() != Some(ExpCode::MonotoneBad.val()) {
        ok = false;
        bits.push_str("limit miss; ");
    }
    if Column::from_index(3).err() != Some(ExpCode::ColumnBad.val()) {
        ok = false;
        bits.push_str("column miss; ");
    }
    let mut sheet = match synth_sheet(0x6704) {
        Ok(v) => v,
        Err(_) => {
            ok = false;
            ExportSheet {
                rows: [Triple {
                    p50: 1,
                    p95: 1,
                    p99: 1,
                }; SEGMENTS],
                digest: 0,
            }
        }
    };
    if sheet.row(4).is_err() {
        // 期望的越界行为
    } else {
        ok = false;
        bits.push_str("seg leak; ");
    }
    let good = sheet.digest;
    sheet.digest = good.wrapping_add(1);
    if sheet.verify() {
        ok = false;
        bits.push_str("digest drift; ");
    }
    sheet.digest = good;
    if !sheet.verify() {
        ok = false;
        bits.push_str("digest restore; ");
    }
    if ok && bits.is_empty() {
        bits = String::from("seg/mono/column/digest ok");
    }
    out.push(("F49067/导出异常面", ok, bits));

    // 5) 常驻十周期：十拍、逐拍周期号连续、零漏报、覆盖率满、摘要自洽。
    let ok;
    let mut info = String::from("monitor failed");
    match run_monitor(0x6801) {
        Ok(m) => {
            let (h, w, imp) = m.class_counts();
            ok = m.zero_missed()
                && m.len() == CYCLES as usize
                && m.coverage_permille() == PERMILLE
                && m.verify()
                && m.missed == 0
                && (h + w + imp) == CYCLES;
            info = format!(
                "probes={} cover={}‰ class={}/{}/{}",
                m.len(),
                m.coverage_permille(),
                h,
                w,
                imp
            );
        }
        Err(_) => ok = false,
    }
    out.push(("F49068/常驻十周期", ok, info));

    // 6) 时钟健康：三源基线有序、健康分守恒、TSC 抖动可检出、失准可判。
    let mut ok = ClockSource::Hpet.baseline_health() > ClockSource::Tsc.baseline_health()
        && ClockSource::Tsc.baseline_health() > ClockSource::Pit.baseline_health();
    let mut bits = String::new();
    let mut src = 0u8;
    while src <= 2 {
        match ClockSource::from_index(src) {
            Ok(s) => {
                if s.index() != src || s.name().is_empty() {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        src = src.saturating_add(1);
    }
    if ClockSource::from_index(3).is_err() {
        // 期望
    } else {
        ok = false;
        bits.push_str("source leak; ");
    }
    match run_selfcheck(0x6802, ClockSource::Tsc) {
        Ok(sc) => {
            let want = SelfCheck::health_for(sc.source, sc.monitor.max_abs_ppm());
            if sc.health_bp != want || !sc.verify() || sc.health_bp > HEALTH_FULL {
                ok = false;
                bits.push_str("health drift; ");
            }
            if DriftClass::classify(DRIFT_OK_PPM as i32) != DriftClass::Healthy
                || DriftClass::classify(DRIFT_OK_PPM as i32 + 1) != DriftClass::Warning
                || DriftClass::classify(DRIFT_WARN_PPM as i32 + 1) != DriftClass::Impaired
            {
                ok = false;
                bits.push_str("class miss; ");
            }
        }
        Err(_) => {
            ok = false;
            bits.push_str("selfcheck failed; ");
        }
    }
    if ok && bits.is_empty() {
        bits = String::from("3 sources/health/class ok");
    }
    out.push(("F49068/时钟健康", ok, bits));

    // 7) 越限十告警：每次越限必出告警且三要素齐全；阈值/观测/动作不含混。
    let ok;
    let mut info = String::from("monitor failed");
    match run_selfcheck(0x6803, ClockSource::Tsc) {
        Ok(sc) => {
            let mut i = 0usize;
            let mut all_three = true;
            while i < sc.alarms.len() {
                match sc.alarms.get(i) {
                    Some(a) => {
                        if !a.three_elements() || !a.exceeded() {
                            all_three = false;
                        }
                        if a.threshold_ppm != DRIFT_WARN_PPM {
                            all_three = false;
                        }
                        if a.action.index() > 2 {
                            all_three = false;
                        }
                    }
                    None => all_three = false,
                }
                i += 1;
            }
            let expect_over = sc
                .monitor
                .probes
                .iter()
                .filter(|p| p.ppm.unsigned_abs() > DRIFT_WARN_PPM)
                .count();
            ok = all_three
                && sc.alarms_complete()
                && sc.alarms.len() == expect_over
                && sc.alarms.len() > 0
                && sc.alarms.len() <= CYCLES as usize
                && sc.verify();
            info = format!(
                "alarms={}/{} three={}",
                sc.alarms.len(),
                expect_over,
                all_three
            );
        }
        Err(_) => ok = false,
    }
    out.push(("F49068/越限十告警", ok, info));

    // 8) 异常码守卫：14 码满窗、说明不越界、坏输入逐码命中、坏行全拒。
    let mut ok = segment_guard();
    let mut bits = String::new();
    if !ok {
        bits.push_str("guard red; ");
    }
    if detail(SEG_BASE) != UNKNOWN || detail(SEG_BASE + 15) != UNKNOWN {
        ok = false;
        bits.push_str("detail leak; ");
    }
    let codes_ok = Triple::new(0, 0, 0).err() == Some(ExpCode::MonotoneBad.val())
        && Triple::new(5, 4, 6).err() == Some(ExpCode::MonotoneBad.val())
        && Column::from_index(3).err() == Some(ExpCode::ColumnBad.val())
        && ClockSource::from_index(9).err() == Some(ExpCode::ClockBad.val())
        && Probe::new(CYCLES as u8, 0).err() == Some(ExpCode::ProbeBad.val())
        && Probe::new(0, DRIFT_LIMIT_PPM as i32 + 1).err() == Some(ExpCode::DriftBad.val())
        && Monitor {
            cycles: 0,
            probes: Vec::new(),
            missed: 0,
            digest: 0,
        }
        .observe(Probe {
            cycle: 0,
            ppm: 1,
            class: DriftClass::Healthy,
        })
        .err()
            == Some(ExpCode::CycleBad.val());
    if !codes_ok {
        ok = false;
        bits.push_str("code miss; ");
    }
    let missing = Alarm {
        cycle: 0,
        threshold_ppm: 0,
        observed_ppm: 0,
        action: AlarmAction::Observe,
    };
    if missing.three_elements() {
        ok = false;
        bits.push_str("alarm blind; ");
    }
    if ok && bits.is_empty() {
        bits = String::from("14 codes/guard/alarm ok");
    }
    out.push(("F49068/异常码守卫", ok, bits));

    out
}

/// 取第 i 行（缺省空串，避免越界）。
fn row_at(rows: &[String], i: usize) -> String {
    match rows.get(i) {
        Some(v) => v.clone(),
        None => String::new(),
    }
}

/// 模块入口：八条检查折叠为一个 CheckSet（域标签 `unxstar`）。
pub fn run_unx49067_checks() -> CheckSet {
    let mut set = CheckSet::new("unxstar");
    for (name, passed, detail_text) in extra_checks() {
        set.add_dyn(name, passed, &detail_text);
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn triple(p50: u32, p95: u32, p99: u32) -> Triple {
        match Triple::new(p50, p95, p99) {
            Ok(v) => v,
            Err(_) => Triple {
                p50: 1,
                p95: 1,
                p99: 1,
            },
        }
    }

    fn sheet(seed: u64) -> ExportSheet {
        match synth_sheet(seed) {
            Ok(v) => v,
            Err(_) => ExportSheet {
                rows: [Triple {
                    p50: 1,
                    p95: 1,
                    p99: 1,
                }; SEGMENTS],
                digest: 0,
            },
        }
    }

    fn monitor(seed: u64) -> Monitor {
        match run_monitor(seed) {
            Ok(v) => v,
            Err(_) => Monitor {
                cycles: 0,
                probes: Vec::new(),
                missed: 0,
                digest: 0,
            },
        }
    }

    #[test]
    fn code_values_are_dense_window() {
        let mut seen = [false; 14];
        for c in ExpCode::ALL {
            let off = c.val() - SEG_BASE;
            assert!(off >= 1 && off <= 14);
            let idx = (off - 1) as usize;
            assert!(!seen[idx]);
            seen[idx] = true;
        }
        assert!(seen.iter().all(|x| *x));
    }

    #[test]
    fn code_detail_nonempty_and_distinct() {
        let mut texts: Vec<&'static str> = Vec::new();
        for c in ExpCode::ALL {
            let t = c.detail();
            assert!(!t.is_empty());
            assert!(!texts.contains(&t));
            texts.push(t);
        }
        assert_eq!(texts.len(), 14);
    }

    #[test]
    fn free_detail_bounds() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 15), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 1), ExpCode::SegBad.detail());
        assert_eq!(detail(SEG_BASE + 14), ExpCode::Overflow.detail());
    }

    #[test]
    fn segment_guard_green() {
        assert!(segment_guard());
    }

    #[test]
    fn column_roundtrip() {
        for i in 0..COLUMNS {
            let c = Column::from_index(i as u8).expect("col");
            assert_eq!(c.index(), i as u8);
            assert_eq!(c.name(), COLUMN_NAMES[i]);
        }
        assert_eq!(Column::from_index(3).err(), Some(ExpCode::ColumnBad.val()));
    }

    #[test]
    fn seg_names_table() {
        assert_eq!(seg_name(0), "interrupt");
        assert_eq!(seg_name(3), "present");
        assert_eq!(seg_name(4), "?");
    }

    #[test]
    fn triple_validation() {
        assert!(Triple::new(1, 1, 1).is_ok());
        assert!(Triple::new(1, NS_LIMIT, NS_LIMIT).is_ok());
        assert_eq!(Triple::new(0, 0, 0), Err(ExpCode::MonotoneBad.val()));
        assert_eq!(Triple::new(2, 1, 3), Err(ExpCode::MonotoneBad.val()));
        assert_eq!(Triple::new(1, 3, 2), Err(ExpCode::MonotoneBad.val()));
        assert_eq!(
            Triple::new(1, 2, NS_LIMIT + 1),
            Err(ExpCode::MonotoneBad.val())
        );
    }

    #[test]
    fn triple_get_and_monotone() {
        let t = triple(10, 20, 30);
        assert_eq!(t.get(Column::P50), 10);
        assert_eq!(t.get(Column::P95), 20);
        assert_eq!(t.get(Column::P99), 30);
        assert!(t.monotone());
        assert_eq!(t.tail_ratio_permille(), 3_000);
    }

    #[test]
    fn triple_tail_ratio_zero_guard() {
        let t = Triple {
            p50: 0,
            p95: 1,
            p99: 2,
        };
        assert_eq!(t.tail_ratio_permille(), 0);
        assert!(!t.monotone());
    }

    #[test]
    fn sheet_shape() {
        let s = sheet(0x1001);
        assert_eq!(s.cells(), CELLS);
        assert!(s.verify());
        assert!(s.monotone());
        assert_ne!(s.digest, 0);
    }

    #[test]
    fn sheet_row_bounds() {
        let s = sheet(0x1002);
        for seg in 0..SEGMENTS {
            assert!(s.row(seg as u8).is_ok());
        }
        assert_eq!(s.row(4), Err(ExpCode::SegBad.val()));
        assert_eq!(s.cell(4, Column::P50), Err(ExpCode::SegBad.val()));
    }

    #[test]
    fn sheet_rejects_non_monotone() {
        let rows = [
            Triple {
                p50: 10,
                p95: 20,
                p99: 30,
            },
            Triple {
                p50: 10,
                p95: 5,
                p99: 30,
            },
            Triple {
                p50: 10,
                p95: 20,
                p99: 30,
            },
            Triple {
                p50: 10,
                p95: 20,
                p99: 30,
            },
        ];
        assert_eq!(
            ExportSheet::new(rows).err(),
            Some(ExpCode::MonotoneBad.val())
        );
    }

    #[test]
    fn sheet_digest_field_sensitive() {
        let a = sheet(0x1003);
        let mut b = a;
        b.rows[0].p50 = b.rows[0].p50.saturating_add(1);
        assert_ne!(sheet_digest(&b.rows), a.digest);
        assert!(!b.verify());
    }

    #[test]
    fn sheet_verify_detects_tamper() {
        let mut s = sheet(0x1004);
        s.digest = s.digest.wrapping_add(1);
        assert!(!s.verify());
    }

    #[test]
    fn render_row_roundtrip() {
        let s = sheet(0x1005);
        let rows = export_rows(&s);
        assert_eq!(rows.len(), SEGMENTS);
        for row in &rows {
            assert!(audit_render(row));
        }
    }

    #[test]
    fn export_rows_ordered() {
        let s = sheet(0x1006);
        let rows = export_rows(&s);
        let mut i = 0usize;
        while i < rows.len() {
            let (_, v, _) = row_fields(&rows[i]).expect("fields");
            assert_eq!(v[0], i as u32);
            i += 1;
        }
    }

    #[test]
    fn audit_render_rejects_bad_rows() {
        let bad = [
            "",
            "O1EXP",
            "O1EXP|seg=0|p50=1|p95=2|p99=3",
            "O1EXP|seg=0|p50=1|p95=2|p99=3|digest=0",
            "O1EXP|p50=1|seg=0|p95=2|p99=3|digest=0000000000000000",
            "O1EXP|seg=0|p50=1|p95=2|p99=3|hash=0000000000000000",
            "O1EXP|seg=9|p50=1|p95=2|p99=3|digest=0000000000000000",
            "O1EXP|seg=0|p50=3|p95=2|p99=3|digest=0000000000000000",
            "O1EXP|seg=0|p50=1|p95=2|p99=3|digest=00000000000000zz",
            "O1EXP|seg=0|p50=1|p95=2|p99=3|digest=0000000000000000|x=1",
        ];
        for row in bad {
            assert!(!audit_render(row), "accepted {}", row);
        }
    }

    #[test]
    fn audit_render_rejects_value_tamper() {
        let s = sheet(0x1007);
        let rows = export_rows(&s);
        let row = match rows.first() {
            Some(v) => v.clone(),
            None => String::new(),
        };
        let (tag, v, d) = match row_fields(&row) {
            Some(x) => x,
            None => ("", [0u32; 4], 0u64),
        };
        let broken = render_row(tag, v[0], v[1], v[2] + 1, v[3], d);
        assert!(!audit_render(&broken));
    }

    #[test]
    fn export_stable_rounds() {
        let s = sheet(0x1008);
        assert!(export_stable(&s, EXPORT_ROUNDS));
        assert!(!export_stable(&s, 0));
    }

    #[test]
    fn export_digest_matches_rows() {
        let s = sheet(0x1009);
        let rows = export_rows(&s);
        let mut h = p_core::FNV_OFF;
        for row in &rows {
            let (_, v, d) = row_fields(row).expect("f");
            h = fold_digest(h, export_row_digest(v[0], v[1], v[2], v[3]));
            assert_ne!(d, 0);
        }
        let mut g = p_core::FNV_OFF;
        let mut seg = 0u8;
        while (seg as usize) < SEGMENTS {
            if let Ok(t) = s.row(seg) {
                g = p_core::fold_byte(g, seg);
                g = p_core::fold_u32(g, t.p50);
                g = p_core::fold_u32(g, t.p95);
                g = p_core::fold_u32(g, t.p99);
            }
            seg += 1;
        }
        assert_eq!(g, s.digest);
        assert_ne!(h, 0);
    }

    #[test]
    fn synth_sheet_seed_sensitive() {
        let a = sheet(0x100A);
        let b = sheet(0x100C);
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn clock_source_roundtrip() {
        for i in 0..3u8 {
            let s = ClockSource::from_index(i).expect("src");
            assert_eq!(s.index(), i);
            assert!(!s.name().is_empty());
        }
        assert_eq!(
            ClockSource::from_index(3).err(),
            Some(ExpCode::ClockBad.val())
        );
    }

    #[test]
    fn clock_health_ordering() {
        assert!(ClockSource::Hpet.baseline_health() > ClockSource::Tsc.baseline_health());
        assert!(ClockSource::Tsc.baseline_health() > ClockSource::Pit.baseline_health());
        assert!(ClockSource::Hpet.baseline_health() <= HEALTH_FULL);
    }

    #[test]
    fn drift_class_boundaries() {
        assert_eq!(DriftClass::classify(0), DriftClass::Healthy);
        assert_eq!(DriftClass::classify(-50), DriftClass::Healthy);
        assert_eq!(DriftClass::classify(51), DriftClass::Warning);
        assert_eq!(DriftClass::classify(-200), DriftClass::Warning);
        assert_eq!(DriftClass::classify(201), DriftClass::Impaired);
        assert_eq!(DriftClass::classify(i32::MIN), DriftClass::Impaired);
    }

    #[test]
    fn drift_class_names() {
        assert_eq!(DriftClass::Healthy.name(), "healthy");
        assert_eq!(DriftClass::Warning.index(), 1);
        assert_eq!(DriftClass::Impaired.index(), 2);
    }

    #[test]
    fn probe_validation() {
        assert!(Probe::new(0, 0).is_ok());
        assert!(Probe::new(9, DRIFT_LIMIT_PPM as i32).is_ok());
        assert!(Probe::new(9, -(DRIFT_LIMIT_PPM as i32)).is_ok());
        assert_eq!(
            Probe::new(10, 0).err(),
            Some(ExpCode::ProbeBad.val())
        );
        assert_eq!(
            Probe::new(0, DRIFT_LIMIT_PPM as i32 + 1).err(),
            Some(ExpCode::DriftBad.val())
        );
    }

    #[test]
    fn probe_records_class() {
        assert_eq!(Probe::new(0, 300 - 10).expect("p").class, DriftClass::Impaired);
        assert_eq!(Probe::new(0, 100).expect("p").class, DriftClass::Warning);
    }

    #[test]
    fn monitor_ten_probes() {
        let m = monitor(0x2001);
        assert_eq!(m.cycles, CYCLES);
        assert_eq!(m.len(), CYCLES as usize);
        assert!(m.zero_missed());
        assert!(m.verify());
        assert_eq!(m.coverage_permille(), PERMILLE);
    }

    #[test]
    fn monitor_rejects_out_of_order_probe() {
        let mut m = Monitor {
            cycles: CYCLES,
            probes: Vec::new(),
            missed: 0,
            digest: p_core::FNV_OFF,
        };
        m.observe(Probe::new(0, 1).expect("p")).expect("obs");
        assert_eq!(
            m.observe(Probe::new(2, 1).expect("p")).err(),
            Some(ExpCode::ProbeBad.val())
        );
        assert_eq!(
            m.observe(Probe::new(0, 1).expect("p")).err(),
            Some(ExpCode::ProbeBad.val())
        );
    }

    #[test]
    fn monitor_rejects_overflow_cycle() {
        let mut m = Monitor {
            cycles: CYCLES,
            probes: Vec::new(),
            missed: 0,
            digest: p_core::FNV_OFF,
        };
        let mut c = 0u32;
        while c < CYCLES {
            m.observe(Probe::new(c as u8, 1).expect("p")).expect("obs");
            c += 1;
        }
        let extra = Probe {
            cycle: CYCLES as u8,
            ppm: 1,
            class: DriftClass::classify(1),
        };
        assert_eq!(m.observe(extra).unwrap_err(), ExpCode::CycleBad.val());
        // 满拍后仍以 ProbeBad（拍序越界）先拒。
        assert!(m.observe(Probe::new(0, 1).expect("p")).is_err());
    }

    #[test]
    fn monitor_rejects_zero_cycles() {
        let mut m = Monitor {
            cycles: 0,
            probes: Vec::new(),
            missed: 0,
            digest: 0,
        };
        assert_eq!(
            m.observe(Probe::new(0, 1).expect("p")).err(),
            Some(ExpCode::CycleBad.val())
        );
    }

    #[test]
    fn monitor_class_counts_sum() {
        let m = monitor(0x2002);
        let (h, w, i) = m.class_counts();
        assert_eq!(h + w + i, CYCLES);
    }

    #[test]
    fn monitor_max_abs_ppm_in_range() {
        let m = monitor(0x2003);
        assert!(m.max_abs_ppm() <= DRIFT_LIMIT_PPM);
        assert!(m.max_abs_ppm() > 0);
    }

    #[test]
    fn monitor_verify_detects_tamper() {
        let mut m = monitor(0x2004);
        assert!(m.verify());
        m.missed = 1;
        assert!(!m.verify());
        m.missed = 0;
        assert!(m.verify());
    }

    #[test]
    fn monitor_zero_missed_requires_cycles() {
        let mut m = monitor(0x2005);
        m.missed = 1;
        assert!(!m.zero_missed());
        m.missed = 0;
        assert!(m.zero_missed());
        let _ = m.probes.pop();
        assert!(!m.zero_missed());
    }

    #[test]
    fn monitor_deterministic() {
        assert_eq!(monitor(0x2006), monitor(0x2006));
        assert_ne!(monitor(0x2006).digest, monitor(0x2008).digest);
    }

    #[test]
    fn monitor_digest_field_sensitive() {
        let a = monitor(0x2008);
        let mut b = a.clone();
        if let Some(p) = b.probes.get_mut(0) {
            p.ppm = p.ppm.wrapping_add(1);
        }
        assert_ne!(monitor_digest(b.cycles, &b.probes, b.missed), a.digest);
    }

    #[test]
    fn alarm_action_mapping() {
        assert_eq!(AlarmAction::for_class(DriftClass::Healthy).index(), 0);
        assert_eq!(AlarmAction::for_class(DriftClass::Warning).index(), 0);
        assert_eq!(AlarmAction::for_class(DriftClass::Impaired).index(), 2);
        assert_eq!(AlarmAction::Recalibrate.name(), "recalibrate");
    }

    #[test]
    fn alarm_three_elements() {
        let good = Alarm {
            cycle: 3,
            threshold_ppm: DRIFT_WARN_PPM,
            observed_ppm: -250,
            action: AlarmAction::Recalibrate,
        };
        assert!(good.three_elements());
        assert!(good.exceeded());
        assert_ne!(good.digest(), 0);
        let bad = Alarm {
            cycle: 3,
            threshold_ppm: 0,
            observed_ppm: -250,
            action: AlarmAction::Recalibrate,
        };
        assert!(!bad.three_elements());
        let zero = Alarm {
            cycle: 3,
            threshold_ppm: 200,
            observed_ppm: 0,
            action: AlarmAction::Observe,
        };
        assert!(!zero.three_elements());
        assert!(!zero.exceeded());
    }

    #[test]
    fn collect_alarms_matches_threshold() {
        let m = monitor(0x2009);
        let alarms = collect_alarms(&m);
        let expect = m
            .probes
            .iter()
            .filter(|p| p.ppm.unsigned_abs() > DRIFT_WARN_PPM)
            .count();
        assert_eq!(alarms.len(), expect);
        for a in &alarms {
            assert_eq!(a.threshold_ppm, DRIFT_WARN_PPM);
            assert!(a.three_elements() && a.exceeded());
        }
    }

    #[test]
    fn selfcheck_health_formula() {
        assert_eq!(SelfCheck::health_for(ClockSource::Hpet, 0), 990);
        assert_eq!(SelfCheck::health_for(ClockSource::Hpet, 200), 890);
        assert_eq!(SelfCheck::health_for(ClockSource::Tsc, 500), 630);
        assert_eq!(SelfCheck::health_for(ClockSource::Pit, 5_000), 0);
    }

    #[test]
    fn selfcheck_runs_and_verifies() {
        let sc = run_selfcheck(0x3001, ClockSource::Tsc).expect("sc");
        assert!(sc.verify());
        assert!(sc.monitor.zero_missed());
        assert!(sc.health_bp <= HEALTH_FULL);
        assert_eq!(sc.health_bp, SelfCheck::health_for(sc.source, sc.monitor.max_abs_ppm()));
        assert!(sc.alarms_complete());
        assert!(sc.alarms.len() > 0);
        assert!(sc.alarms.len() <= CYCLES as usize);
    }

    #[test]
    fn selfcheck_healthy_across_sources() {
        for src in [ClockSource::Hpet, ClockSource::Tsc, ClockSource::Pit] {
            let sc = run_selfcheck(0x3002, src).expect("sc");
            assert!(sc.healthy(), "{} health={}", src.name(), sc.health_bp);
        }
    }

    #[test]
    fn selfcheck_replay_stable() {
        assert!(selfcheck_replay(0x3003, ClockSource::Tsc, 3));
        assert!(!selfcheck_replay(0x3003, ClockSource::Tsc, 0));
    }

    #[test]
    fn selfcheck_digest_field_sensitive() {
        let a = run_selfcheck(0x3004, ClockSource::Tsc).expect("a");
        let mut b = a.clone();
        b.health_bp = b.health_bp.saturating_sub(1);
        assert!(!b.verify());
    }

    #[test]
    fn check_row_renders() {
        let sc = run_selfcheck(0x3005, ClockSource::Hpet).expect("sc");
        let row = render_check(&sc);
        assert!(row.starts_with(CHECK_TAG));
        assert!(row.contains("cycles=10"));
        assert!(row.contains("missed=0"));
    }

    #[test]
    fn alarm_row_renders() {
        let sc = run_selfcheck(0x3006, ClockSource::Tsc).expect("sc");
        let a = match sc.alarms.first() {
            Some(v) => *v,
            None => Alarm {
                cycle: 0,
                threshold_ppm: DRIFT_WARN_PPM,
                observed_ppm: 1,
                action: AlarmAction::Observe,
            },
        };
        let row = render_alarm(&a);
        assert!(row.starts_with(ALARM_TAG));
        assert!(row.contains("thr=200"));
    }

    #[test]
    fn checks_all_green() {
        let set = run_unx49067_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn checks_len_and_truncation() {
        let set = run_unx49067_checks();
        assert_eq!(set.len(), 8);
        assert!(!set.truncated());
        assert_eq!(set.passed_count(), 8);
        assert_eq!(set.failed_count(), 0);
    }

    #[test]
    fn checks_names_distinct() {
        let set = run_unx49067_checks();
        let mut names: Vec<&'static str> = Vec::new();
        for c in set.iter() {
            assert!(!names.contains(&c.name));
            names.push(c.name);
        }
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn checks_details_nonempty() {
        for (name, passed, det) in extra_checks() {
            assert!(passed, "{} red: {}", name, det);
            assert!(!det.is_empty());
        }
    }

    #[test]
    fn row_at_defaults_empty() {
        let rows: Vec<String> = vec![String::from("x")];
        assert_eq!(row_at(&rows, 0), "x");
        assert_eq!(row_at(&rows, 9), "");
    }
}
