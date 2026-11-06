//! UNX-F49035 键盘全表逐键判据与 LED 状态机收尾错误码族 + UNX-F49036 四点观测埋点 · 0x71D00
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B12 族第八槽；任务板 J1 引用按原文保留）：
//!   UNX-F49035 键盘全表逐键判据与 LED 状态机收尾错误码族（15 条）:
//!          ① 纯功能代码 ≥220 行；
//!          ② 判据原文：UNX-F49036-J1 四点 100% 覆盖；异常零静默。
//!   UNX-F49036 键盘全表逐键判据与 LED 状态机收尾观测埋点（入口/出口/耗时/结果四点全覆盖）:
//!          ① 纯功能代码 ≥260 行；
//!          ② 判据原文：UNX-F49037-J1 fuzz ×10^4 零崩。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（错误码族）：窗口 → 码值 → 名称 → 阶段 → 族属 → 独立推导。
//! 窗口先于码值：15 条码先落在声明窗口内，窗口外的码一律不收；
//! 否则"15 条"只是计数，不是可审计的闭集。
//! 码值先于名称：码值互不相同，名称才有资格谈唯一；码值撞号时，
//! 两条名称再工整也指向同一条诊断，排障会随机命中。
//! 名称先于阶段：每条码必须声明它属于入口/出口/耗时/结果中的哪一点；
//! 阶段不明的码无法回答"异常发生在四点中的哪一点"。
//! 阶段先于族属：族属用于横向归并（逐键/ LED / 埋点），必须先有阶段
//! 再谈归族，否则归族会掩盖阶段缺失。
//! 独立推导最后：用只读码值区间的第二套实现推导阶段与族属，与表内声明
//! 逐条比对。这一关不通过，表内所有声明都只是自说自话。
//! 闸门次序（四点埋点）：入口 → 出口 → 耗时 → 结果 → 异常分类。
//! 入口先于出口：没有入口的出口是无根账，无法判断时长；
//! 出口先于耗时：耗时只在两次打点齐全且顺序正确时才可计算；
//! 耗时先于结果：结果点必须落在出口之后，否则结果是"未完结的结论"；
//! 异常分类最后：四点中任何一点的缺失/越界都必须落到一条已知码上，
//! "记录到了"不等于"异常不静默"。
//! 关键差异：本模块不重做逐键判据本体与 LED 状态机本体，只做两件事——
//! ① 把逐键/LED 两个收尾面的 15 条诊断码整理成可独立推导的闭集；
//! ② 用入口/出口/耗时/结果四点给每一次判定记账，并证明异常全部有码。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71D00，B12 族第八槽）。
pub const SEG_BASE: u32 = 0x71D00;

/// 键盘全表键数（191）。
pub const KEY_TOTAL: u32 = 191;
/// 每键判据数（解析 / 记账）。
pub const CRITERIA: u32 = 2;
/// 逐键判定总条数（382）。
pub const SPAN_TOTAL: u32 = KEY_TOTAL * CRITERIA;
/// LED 态个数（3 位）。
pub const LED_STATES: u32 = 8;
/// LED 迁移对总数（8×8）。
pub const LED_PAIRS: u32 = LED_STATES * LED_STATES;
/// 单键切换允许的跳变数（每个源态 1 + 3）。
pub const LED_LEGAL: u32 = LED_STATES * 4;
/// 四点观测的阶段数。
pub const STAGES: usize = 4;
/// 错误码族条数（15）。
pub const ERR_SIZE: usize = 15;
/// 错误码族窗口下界。
pub const ERR_LO: u32 = 0x71D10;
/// 错误码族窗口上界。
pub const ERR_HI: u32 = 0x71D1E;
/// 错误码族归属族数（3）。
pub const FAMILIES: usize = 3;
/// 耗时预算（单位：节拍）。
pub const BUDGET: u32 = 200;
/// 异常注入条数。
pub const FAIL_INJECT: u32 = 10;
/// 参数 fuzz 轮数（判据 ×10^4 口径）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 参数 fuzz 种子。
pub const FUZZ_SEED: u64 = 0x0000_0000_4A05_0001;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 账目标签。
pub const PROBE_TAG: &str = "PROBE";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知逐键埋点诊断码";

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeCode {
    /// 键号越出全表
    KeyBad,
    /// 判据号越界
    CriterionBad,
    /// 入口点重复或缺失
    EntryBad,
    /// 出口点重复或缺失
    ExitBad,
    /// 耗时越界或倒序
    ElapsedBad,
    /// 结果点缺失或先于出口
    ResultBad,
    /// 四点顺序错乱
    OrderBad,
    /// LED 态越界
    LedBad,
    /// LED 跳变非法（跨多键）
    LedStateBad,
    /// 错误码族不合格
    FamilyBad,
    /// fuzz 账不守恒
    FuzzBad,
    /// 渲染不可解析
    RenderBad,
    /// 段位越窗
    SegmentBad,
    /// 计数溢出
    Overflow,
}

impl ProbeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            ProbeCode::KeyBad => "键号越出全表",
            ProbeCode::CriterionBad => "判据号越界",
            ProbeCode::EntryBad => "入口点重复或缺失",
            ProbeCode::ExitBad => "出口点重复或缺失",
            ProbeCode::ElapsedBad => "耗时越界或倒序",
            ProbeCode::ResultBad => "结果点缺失或先于出口",
            ProbeCode::OrderBad => "四点顺序错乱",
            ProbeCode::LedBad => "LED 态越界",
            ProbeCode::LedStateBad => "LED 跳变非法（跨多键）",
            ProbeCode::FamilyBad => "错误码族不合格",
            ProbeCode::FuzzBad => "fuzz 账不守恒",
            ProbeCode::RenderBad => "渲染不可解析",
            ProbeCode::SegmentBad => "段位越窗",
            ProbeCode::Overflow => "计数溢出",
        }
    }

    pub const ALL: [ProbeCode; 14] = [
        ProbeCode::KeyBad,
        ProbeCode::CriterionBad,
        ProbeCode::EntryBad,
        ProbeCode::ExitBad,
        ProbeCode::ElapsedBad,
        ProbeCode::ResultBad,
        ProbeCode::OrderBad,
        ProbeCode::LedBad,
        ProbeCode::LedStateBad,
        ProbeCode::FamilyBad,
        ProbeCode::FuzzBad,
        ProbeCode::RenderBad,
        ProbeCode::SegmentBad,
        ProbeCode::Overflow,
    ];
}

/// 按诊断码取值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ProbeCode::ALL.len() {
        if let Some(c) = ProbeCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    UNKNOWN
}

/// 是否为本段已知码。
pub fn known_code(code: u32) -> bool {
    detail(code) != UNKNOWN
}

/// 段位守卫：14 条码连续占满 +0x01..+0x0E。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ProbeCode::ALL.len() {
        if let Some(c) = ProbeCode::ALL.get(i) {
            if c.val() != SEG_BASE + 0x01 + i as u32 {
                return false;
            }
            if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 错误码族（15 条） ─────────────────────────────────────────────
/// 一条收尾错误码：码值、名称、所属观测点（0 入口 / 1 出口 / 2 耗时 / 3 结果）、归属族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrEntry {
    pub code: u32,
    pub name: &'static str,
    pub stage: u8,
    pub family: u8,
}

/// 观测点名称。
pub fn stage_name(stage: u8) -> &'static str {
    match stage {
        0 => "入口",
        1 => "出口",
        2 => "耗时",
        3 => "结果",
        _ => "",
    }
}

/// 归属族名称（逐键 / LED / 埋点）。
pub fn family_name(family: u8) -> &'static str {
    match family {
        0 => "逐键",
        1 => "led",
        2 => "埋点",
        _ => "",
    }
}

/// 15 条收尾诊断码（窗口 0x71D10..=0x71D1E，连续占满）。
pub const ERR_FAMILY: [ErrEntry; ERR_SIZE] = [
    ErrEntry { code: 0x71D10, name: "code-window-drift", stage: 3, family: 2 },
    ErrEntry { code: 0x71D11, name: "key-table-missing", stage: 0, family: 0 },
    ErrEntry { code: 0x71D12, name: "key-scan-out", stage: 0, family: 0 },
    ErrEntry { code: 0x71D13, name: "key-criterion-dup", stage: 0, family: 0 },
    ErrEntry { code: 0x71D14, name: "key-expected-drift", stage: 3, family: 0 },
    ErrEntry { code: 0x71D15, name: "key-screen-fail", stage: 3, family: 0 },
    ErrEntry { code: 0x71D16, name: "led-state-illegal", stage: 3, family: 1 },
    ErrEntry { code: 0x71D17, name: "led-toggle-collision", stage: 2, family: 1 },
    ErrEntry { code: 0x71D18, name: "led-write-timeout", stage: 2, family: 1 },
    ErrEntry { code: 0x71D19, name: "led-replay-drift", stage: 3, family: 1 },
    ErrEntry { code: 0x71D1A, name: "trace-entry-missing", stage: 0, family: 2 },
    ErrEntry { code: 0x71D1B, name: "trace-exit-missing", stage: 1, family: 2 },
    ErrEntry { code: 0x71D1C, name: "trace-elapsed-bad", stage: 2, family: 2 },
    ErrEntry { code: 0x71D1D, name: "trace-result-silent", stage: 3, family: 2 },
    ErrEntry { code: 0x71D1E, name: "trace-order-bad", stage: 1, family: 2 },
];

/// 独立阶段推导：只看码值，不看表内声明。
pub fn expected_stage(code: u32) -> Option<u8> {
    match code {
        0x71D10 | 0x71D14 | 0x71D15 | 0x71D16 | 0x71D19 | 0x71D1D => Some(3),
        0x71D11 | 0x71D12 | 0x71D13 | 0x71D1A => Some(0),
        0x71D17 | 0x71D18 | 0x71D1C => Some(2),
        0x71D1B | 0x71D1E => Some(1),
        _ => None,
    }
}

/// 独立族属推导：只看码值。
pub fn expected_family(code: u32) -> Option<u8> {
    match code {
        0x71D11..=0x71D15 => Some(0),
        0x71D16..=0x71D19 => Some(1),
        0x71D10 | 0x71D1A..=0x71D1E => Some(2),
        _ => None,
    }
}

/// 码值查表。
pub fn err_lookup(code: u32) -> Option<usize> {
    let mut i = 0usize;
    while i < ERR_FAMILY.len() {
        if let Some(e) = ERR_FAMILY.get(i) {
            if e.code == code {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 名称查表。
pub fn err_lookup_name(name: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < ERR_FAMILY.len() {
        if let Some(e) = ERR_FAMILY.get(i) {
            if e.name == name {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 族表自洽：窗口、唯一、阶段/族合法。
pub fn err_ok() -> bool {
    if ERR_FAMILY.len() != ERR_SIZE {
        return false;
    }
    let mut i = 0usize;
    while i < ERR_FAMILY.len() {
        let a = match ERR_FAMILY.get(i) {
            Some(v) => v,
            None => return false,
        };
        if a.code < ERR_LO || a.code > ERR_HI {
            return false;
        }
        if a.name.is_empty() || (a.stage as usize) >= STAGES || (a.family as usize) >= FAMILIES {
            return false;
        }
        if stage_name(a.stage).is_empty() || family_name(a.family).is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < ERR_FAMILY.len() {
            let b = match ERR_FAMILY.get(j) {
                Some(v) => v,
                None => return false,
            };
            if a.code == b.code || a.name == b.name {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 零漂移：表内阶段/族属与独立推导逐条相等。
pub fn err_zero_drift() -> bool {
    let mut i = 0usize;
    while i < ERR_FAMILY.len() {
        let e = match ERR_FAMILY.get(i) {
            Some(v) => v,
            None => return false,
        };
        if expected_stage(e.code) != Some(e.stage) {
            return false;
        }
        if expected_family(e.code) != Some(e.family) {
            return false;
        }
        if err_lookup(e.code) != Some(i) || err_lookup_name(e.name) != Some(i) {
            return false;
        }
        i += 1;
    }
    true
}

/// 族表摘要。
pub fn err_digest() -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < ERR_FAMILY.len() {
        if let Some(e) = ERR_FAMILY.get(i) {
            h = p_core::fold_u32(h, e.code);
            h = p_core::fold_byte(h, e.stage);
            h = p_core::fold_byte(h, e.family);
            let bytes = e.name.as_bytes();
            let mut k = 0usize;
            while k < bytes.len() {
                if let Some(b) = bytes.get(k) {
                    h = p_core::fold_byte(h, *b);
                }
                k += 1;
            }
        }
        i += 1;
    }
    h
}

// ── 四点埋点 ──────────────────────────────────────────────────────
/// 一次判定的四点账：入口时刻、出口时刻、耗时、结果码（0 = 通过）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub key: u8,
    pub criterion: u8,
    pub open_at: Option<u32>,
    pub close_at: Option<u32>,
    pub result: Option<u32>,
    opened: u8,
    closed: u8,
    settled: u8,
}

impl Span {
    pub fn new(key: u8, criterion: u8) -> Span {
        Span {
            key,
            criterion,
            open_at: None,
            close_at: None,
            result: None,
            opened: 0,
            closed: 0,
            settled: 0,
        }
    }

    /// 入口打点：只允许一次。
    pub fn open(&mut self, at: u32) -> Result<(), u32> {
        if self.opened > 0 {
            return Err(ProbeCode::EntryBad.val());
        }
        self.opened = 1;
        self.open_at = Some(at);
        Ok(())
    }

    /// 出口打点：必须已有入口，且只允许一次。
    pub fn close(&mut self, at: u32) -> Result<(), u32> {
        if self.opened == 0 {
            return Err(ProbeCode::OrderBad.val());
        }
        if self.closed > 0 {
            return Err(ProbeCode::ExitBad.val());
        }
        self.closed = 1;
        self.close_at = Some(at);
        Ok(())
    }

    /// 结果打点：必须已有出口，且只允许一次。
    pub fn settle(&mut self, code: u32) -> Result<(), u32> {
        if self.closed == 0 {
            return Err(ProbeCode::ResultBad.val());
        }
        if self.settled > 0 {
            return Err(ProbeCode::ResultBad.val());
        }
        self.settled = 1;
        self.result = Some(code);
        Ok(())
    }

    /// 耗时：只有入口/出口齐全且顺序正确才可计算。
    pub fn elapsed(&self) -> Option<u32> {
        let a = self.open_at?;
        let b = self.close_at?;
        if b < a {
            return None;
        }
        Some(b - a)
    }

    /// 覆盖率：四点各计 1。
    pub fn coverage(&self) -> u8 {
        let mut n = 0u8;
        if self.open_at.is_some() {
            n += 1;
        }
        if self.close_at.is_some() {
            n += 1;
        }
        if self.elapsed().is_some() {
            n += 1;
        }
        if self.result.is_some() {
            n += 1;
        }
        n
    }

    /// 四点审计：键/判据 → 顺序 → 出口 → 耗时 → 结果，首错优先。
    pub fn audit(&self, budget: u32) -> Result<(), u32> {
        if (self.key as u32) >= KEY_TOTAL {
            return Err(ProbeCode::KeyBad.val());
        }
        if self.criterion == 0 || self.criterion as u32 > CRITERIA {
            return Err(ProbeCode::CriterionBad.val());
        }
        if self.opened == 0 {
            return Err(ProbeCode::EntryBad.val());
        }
        if self.closed == 0 {
            return Err(ProbeCode::ExitBad.val());
        }
        let elapsed = match self.elapsed() {
            Some(v) => v,
            None => return Err(ProbeCode::OrderBad.val()),
        };
        if elapsed > budget {
            return Err(ProbeCode::ElapsedBad.val());
        }
        if self.settled == 0 {
            return Err(ProbeCode::ResultBad.val());
        }
        Ok(())
    }
}

/// 逐键期望值：由 (键号, 判据号) 独立导出。
pub fn expected_key(key: u8, criterion: u8) -> Result<u32, u32> {
    if (key as u32) >= KEY_TOTAL {
        return Err(ProbeCode::KeyBad.val());
    }
    if criterion == 0 || criterion as u32 > CRITERIA {
        return Err(ProbeCode::CriterionBad.val());
    }
    Ok(((criterion as u32) << 28) | (key as u32))
}

/// 逐键账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyLedger {
    pub spans: u32,
    pub four_point: u32,
    pub failures: u32,
    pub total_elapsed: u64,
    pub digest: u64,
}

/// 全表逐键判定：191 键 ×2 判据，每条都走四点打点并审计。
pub fn screen_keys() -> Result<KeyLedger, u32> {
    let mut led = KeyLedger {
        spans: 0,
        four_point: 0,
        failures: 0,
        total_elapsed: 0,
        digest: p_core::FNV_OFF,
    };
    let mut k: u32 = 0;
    while k < KEY_TOTAL {
        let mut c: u8 = 1;
        while (c as u32) <= CRITERIA {
            let idx = k.saturating_mul(CRITERIA).saturating_add((c - 1) as u32);
            let start = idx.saturating_mul(4);
            let dt = 8 + (idx % 5);
            let mut span = Span::new(k as u8, c);
            span.open(start)?;
            span.close(start.saturating_add(dt))?;
            let want = expected_key(k as u8, c)?;
            span.settle(want)?;
            if span.audit(BUDGET).is_err() {
                led.failures = led.failures.saturating_add(1);
            }
            if span.coverage() == STAGES as u8 {
                led.four_point = led.four_point.saturating_add(1);
            }
            let elapsed = match span.elapsed() {
                Some(v) => v,
                None => 0,
            };
            led.total_elapsed = led.total_elapsed.saturating_add(elapsed as u64);
            led.spans = led.spans.saturating_add(1);
            led.digest = p_core::fold_u32(led.digest, span.digest());
            led.digest = p_core::fold_u32(led.digest, elapsed);
            c += 1;
        }
        k += 1;
    }
    if led.spans != SPAN_TOTAL || led.four_point != SPAN_TOTAL {
        return Err(ProbeCode::Overflow.val());
    }
    Ok(led)
}

impl Span {
    /// 账目摘要：四点齐备时折叠全部字段。
    pub fn digest(&self) -> u32 {
        let open = match self.open_at {
            Some(v) => v,
            None => 0xFFFF_FFFF,
        };
        let close = match self.close_at {
            Some(v) => v,
            None => 0xFFFF_FFFF,
        };
        let result = match self.result {
            Some(v) => v,
            None => 0xFFFF_FFFF,
        };
        let mut h = p_core::FNV_OFF as u32;
        h = h.wrapping_mul(31).wrapping_add(self.key as u32);
        h = h.wrapping_mul(31).wrapping_add(self.criterion as u32);
        h = h.wrapping_mul(31).wrapping_add(open);
        h = h.wrapping_mul(31).wrapping_add(close);
        h = h.wrapping_mul(31).wrapping_add(result);
        h
    }
}

/// 构造第 i 条病态 span，返回 (账面状态, 审计裁决码)。
/// 十条互不相同的病态：缺入口/缺出口/倒序/缺结果/超预算/键上界/键野值/
/// 判据零/判据越界/零预算。
fn injected_span(i: usize) -> (Span, Option<u32>) {
    let mut span = Span::new(0x10, 1);
    let mut budget = BUDGET;
    match i {
        0 => {
            let _ = span.close(10);
            let _ = span.settle(0);
        }
        1 => {
            let _ = span.open(10);
        }
        2 => {
            let _ = span.open(100);
            let _ = span.close(90);
            let _ = span.settle(0);
        }
        3 => {
            let _ = span.open(10);
            let _ = span.close(20);
        }
        4 => {
            let _ = span.open(0);
            let _ = span.close(BUDGET + 1);
            let _ = span.settle(0);
        }
        5 => {
            let mut bad = Span::new(KEY_TOTAL as u8, 1);
            let _ = bad.open(0);
            let _ = bad.close(10);
            let _ = bad.settle(0);
            span = bad;
        }
        6 => {
            let mut bad = Span::new(0xFF, 1);
            let _ = bad.open(0);
            let _ = bad.close(10);
            let _ = bad.settle(0);
            span = bad;
        }
        7 => {
            let mut bad = Span::new(0x10, 0);
            let _ = bad.open(0);
            let _ = bad.close(10);
            let _ = bad.settle(0);
            span = bad;
        }
        8 => {
            let mut bad = Span::new(0x10, 3);
            let _ = bad.open(0);
            let _ = bad.close(10);
            let _ = bad.settle(0);
            span = bad;
        }
        _ => {
            let _ = span.open(0);
            let _ = span.close(1);
            let _ = span.settle(0);
            budget = 0;
        }
    }
    let code = match span.audit(budget) {
        Ok(_) => None,
        Err(c) => Some(c),
    };
    (span, code)
}

/// 异常注入：十条病态 span，全部必须被审计捕获且有码。
pub fn fail_inject() -> Result<(u32, u32), u32> {
    let mut caught = 0u32;
    let mut classified = 0u32;
    let mut i = 0usize;
    while i < FAIL_INJECT as usize {
        let (_, code) = injected_span(i);
        match code {
            Some(c) => {
                if !known_code(c) {
                    return Err(ProbeCode::FamilyBad.val());
                }
                caught = caught.saturating_add(1);
                classified = classified.saturating_add(1);
            }
            None => {}
        }
        i += 1;
    }
    Ok((caught, classified))
}

/// 部分覆盖检出：覆盖不足四点的 span 必须全部被审计判红。
/// 返回 (覆盖不足四点的条数, 其中被判红的条数)，两者必须相等。
pub fn partial_detect() -> Result<(u32, u32), u32> {
    let mut partial = 0u32;
    let mut red = 0u32;
    let mut i = 0usize;
    while i < FAIL_INJECT as usize {
        let (span, code) = injected_span(i);
        if span.coverage() < STAGES as u8 {
            partial = partial.saturating_add(1);
            if code.is_some() {
                red = red.saturating_add(1);
            }
        }
        i += 1;
    }
    Ok((partial, red))
}

/// 耗时总账重算：逐 span 累加与账内累计必须相等。
pub fn elapsed_audit(l: &KeyLedger) -> bool {
    let mut total = 0u64;
    let mut k: u32 = 0;
    while k < KEY_TOTAL {
        let mut c: u32 = 1;
        while c <= CRITERIA {
            let idx = k.saturating_mul(CRITERIA).saturating_add(c - 1);
            total = total.saturating_add((8 + (idx % 5)) as u64);
            c += 1;
        }
        k += 1;
    }
    total == l.total_elapsed
}

// ── LED 收尾 ─────────────────────────────────────────────────────
/// 跳变位数。
pub fn toggle_bits(from: u8, to: u8) -> u32 {
    (from ^ to).count_ones()
}

/// 单步合法：同态或只差一位。
pub fn led_pair_ok(from: u8, to: u8) -> Result<(), u32> {
    if from as u32 >= LED_STATES || to as u32 >= LED_STATES {
        return Err(ProbeCode::LedBad.val());
    }
    if toggle_bits(from, to) > 1 {
        return Err(ProbeCode::LedStateBad.val());
    }
    Ok(())
}

/// LED 账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedLedger {
    pub pairs: u32,
    pub legal: u32,
    pub illegal: u32,
    pub four_point: u32,
    pub digest: u64,
}

/// LED 状态机收尾：8×8 迁移逐对打点，跨多键的直接判红但有码。
pub fn led_closing() -> Result<LedLedger, u32> {
    let mut led = LedLedger {
        pairs: 0,
        legal: 0,
        illegal: 0,
        four_point: 0,
        digest: p_core::FNV_OFF,
    };
    let mut from: u8 = 0;
    while (from as u32) < LED_STATES {
        let mut to: u8 = 0;
        while (to as u32) < LED_STATES {
            let mut span = Span::new(0x20, 1);
            let base = (from as u32) * 8 + (to as u32);
            span.open(base)?;
            span.close(base.saturating_add(2))?;
            let verdict = match led_pair_ok(from, to) {
                Ok(_) => 0,
                Err(c) => {
                    if !known_code(c) {
                        return Err(ProbeCode::FamilyBad.val());
                    }
                    c
                }
            };
            span.settle(verdict)?;
            if span.coverage() == STAGES as u8 {
                led.four_point = led.four_point.saturating_add(1);
            }
            match verdict {
                0 => led.legal = led.legal.saturating_add(1),
                _ => led.illegal = led.illegal.saturating_add(1),
            }
            led.pairs = led.pairs.saturating_add(1);
            led.digest = p_core::fold_u32(led.digest, from as u32);
            led.digest = p_core::fold_u32(led.digest, to as u32);
            led.digest = p_core::fold_u32(led.digest, verdict);
            to += 1;
        }
        from += 1;
    }
    if led.pairs != LED_PAIRS || led.legal != LED_LEGAL {
        return Err(ProbeCode::Overflow.val());
    }
    Ok(led)
}

// ── 参数 fuzz ────────────────────────────────────────────────────
/// fuzz 账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzLedger {
    pub rounds: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub classified: u32,
    pub digest: u64,
}

impl FuzzLedger {
    pub fn conserved(&self) -> bool {
        self.accepted.saturating_add(self.rejected) == self.rounds && self.classified == self.rejected
    }
}

/// 构造并审计一条随机 span：fault 位决定注入哪一类异常。
pub fn build_probe(key: u8, criterion: u8, budget: u32, dt: u32, fault: u8) -> Result<(), u32> {
    let mut span = Span::new(key, criterion);
    match fault {
        0 => {
            span.open(0)?;
            span.close(dt)?;
            let want = expected_key(key, criterion)?;
            span.settle(want)?;
        }
        1 => {
            // 缺入口。
            let _ = span.close(dt);
        }
        2 => {
            // 缺出口。
            span.open(0)?;
        }
        3 => {
            // 出口早于入口。
            span.open(dt.saturating_add(1))?;
            span.close(dt)?;
            let want = expected_key(key, criterion)?;
            span.settle(want)?;
        }
        4 => {
            // 缺结果。
            span.open(0)?;
            span.close(dt)?;
        }
        5 => {
            // 出口重复：第二次出口必须被拒，并作为该轮拒绝码上报。
            span.open(0)?;
            span.close(dt)?;
            if span.close(dt.saturating_add(1)).is_err() {
                return Err(ProbeCode::ExitBad.val());
            }
            let want = expected_key(key, criterion)?;
            span.settle(want)?;
        }
        6 => {
            // 结果先于出口。
            span.open(0)?;
            let _ = span.settle(0);
        }
        _ => {
            // 耗时越界。
            span.open(0)?;
            span.close(budget.saturating_add(1))?;
            let want = expected_key(key, criterion)?;
            span.settle(want)?;
        }
    }
    span.audit(budget)
}

/// 参数 fuzz：乱键号 / 乱判据 / 乱预算 / 乱耗时 / 乱故障位。
pub fn probe_fuzz(rounds: u32, seed: u64) -> Result<FuzzLedger, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(ProbeCode::FuzzBad.val());
    }
    let mut lcg = p_core::Lcg::new(seed);
    let mut led = FuzzLedger {
        rounds,
        accepted: 0,
        rejected: 0,
        classified: 0,
        digest: p_core::FNV_OFF,
    };
    let mut i: u32 = 0;
    while i < rounds {
        let key = lcg.next() as u8;
        let criterion = (lcg.below(4)) as u8;
        let budget = lcg.below(500);
        let dt = lcg.below(600);
        // 故障位取高位，避免与 dt（低位模数）形成固定同余导致的"永不接受"。
        let fault = (lcg.next() >> 61) as u8;
        let outcome = build_probe(key, criterion, budget, dt, fault);
        let ev = ((key as u64) << 24) ^ ((criterion as u64) << 16) ^ ((fault as u64) << 8) ^ dt as u64;
        led.digest = p_core::fold_u32(led.digest, (ev >> 32) as u32);
        led.digest = p_core::fold_u32(led.digest, ev as u32);
        match outcome {
            Ok(_) => led.accepted = led.accepted.saturating_add(1),
            Err(c) => {
                if !known_code(c) {
                    return Err(ProbeCode::FuzzBad.val());
                }
                led.rejected = led.rejected.saturating_add(1);
                led.classified = led.classified.saturating_add(1);
                led.digest = p_core::fold_u32(led.digest, c);
            }
        }
        i += 1;
    }
    if !led.conserved() {
        return Err(ProbeCode::FuzzBad.val());
    }
    Ok(led)
}

/// fuzz 复跑。
pub fn probe_replay() -> Result<(FuzzLedger, FuzzLedger), u32> {
    let a = probe_fuzz(FUZZ_ROUNDS, FUZZ_SEED)?;
    let b = probe_fuzz(FUZZ_ROUNDS, FUZZ_SEED)?;
    Ok((a, b))
}

// ── 渲染 ─────────────────────────────────────────────────────────
pub fn render_row(tag: &str, rounds: u32, accepted: u32, rejected: u32, points: u32, digest: u64) -> String {
    format!(
        "{}|rounds={}|accepted={}|rejected={}|points={}|digest={:016X}",
        tag, rounds, accepted, rejected, points, digest
    )
}

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

pub fn row_fields<'a>(row: &'a str) -> Option<(&'a str, [u32; 4], u64)> {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    let tag = match parts.get(0).and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return None,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return None,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return None,
        };
        if sp.next().is_some() {
            return None;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return None,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return None,
    };
    if sp.next().is_some() {
        return None;
    }
    Some((tag, vals, digest))
}

pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, vals, digest)) => {
            render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
        }
        None => false,
    }
}

/// fuzz 账渲染并自校。
pub fn fuzz_row(l: &FuzzLedger) -> String {
    render_row(PROBE_TAG, l.rounds, l.accepted, l.rejected, STAGES as u32, l.digest)
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49035 ①：15 条收尾错误码零漂移（窗口/唯一/独立阶段与族属推导一致）。
    let fam = err_ok() && err_zero_drift() && err_digest() != 0;
    let fam_alias = err_lookup(0x71D10) == Some(0)
        && err_lookup(0x71D1E) == Some(ERR_SIZE - 1)
        && err_lookup_name("trace-order-bad") == Some(ERR_SIZE - 1)
        && err_lookup(0x71D0F).is_none()
        && err_lookup(0xE000).is_none();
    out.push((
        "F49035/码族零漂移",
        fam && fam_alias,
        format!(
            "codes={} drift={} window=[{:X},{:X}]",
            ERR_SIZE,
            !err_zero_drift(),
            ERR_LO,
            ERR_HI
        ),
    ));

    // F49035 ②：全表逐键判定 382 条全绿（四点齐备、失败为零）。
    let keys = screen_keys();
    let keys_ok = match &keys {
        Ok(l) => {
            l.spans == SPAN_TOTAL
                && l.four_point == SPAN_TOTAL
                && l.failures == 0
                && l.total_elapsed > 0
                && l.digest != 0
        }
        Err(_) => false,
    };
    out.push((
        "F49035/逐键全覆盖",
        keys_ok,
        format!(
            "spans={:?} four_point={:?} failures={:?}",
            keys.as_ref().map(|l| l.spans),
            keys.as_ref().map(|l| l.four_point),
            keys.as_ref().map(|l| l.failures)
        ),
    ));

    // F49035 ③：异常零静默（十条病态全捕获、全有码）。
    let inj = fail_inject();
    let inj_ok = match &inj {
        Ok((c, k)) => *c == FAIL_INJECT && *k == FAIL_INJECT,
        Err(_) => false,
    };
    out.push((
        "F49035/异常零静默",
        inj_ok,
        format!(
            "caught={:?} classified={:?}",
            inj.as_ref().map(|(c, _)| *c),
            inj.as_ref().map(|(_, k)| *k)
        ),
    ));

    // F49035 ④：LED 状态机收尾（64 对迁移：32 合法 / 32 跨多键判红，全部四点）。
    let leds = led_closing();
    let led_ok = match &leds {
        Ok(l) => {
            l.pairs == LED_PAIRS
                && l.legal == LED_LEGAL
                && l.illegal == LED_PAIRS - LED_LEGAL
                && l.four_point == LED_PAIRS
                && l.digest != 0
        }
        Err(_) => false,
    };
    out.push((
        "F49035/LED收尾",
        led_ok,
        format!(
            "pairs={:?} legal={:?} illegal={:?} four_point={:?}",
            leds.as_ref().map(|l| l.pairs),
            leds.as_ref().map(|l| l.legal),
            leds.as_ref().map(|l| l.illegal),
            leds.as_ref().map(|l| l.four_point)
        ),
    ));

    // F49036 ①：四点 100% 覆盖（逐键 + LED 全覆盖；注入账的部分覆盖全部判红）。
    let part = partial_detect();
    let four_ok = match (&keys, &leds, &part) {
        (Ok(k), Ok(l), Ok((p, r))) => {
            k.four_point == k.spans && l.four_point == l.pairs && *p > 0 && *p == *r
        }
        _ => false,
    };
    out.push((
        "F49036/四点全覆盖",
        four_ok,
        format!(
            "key_cov={:?} led_cov={:?} partial={:?} red={:?}",
            keys.as_ref().map(|l| (l.four_point, l.spans)),
            leds.as_ref().map(|l| (l.four_point, l.pairs)),
            part.as_ref().map(|(p, _)| *p),
            part.as_ref().map(|(_, r)| *r)
        ),
    ));

    // F49036 ②：耗时记账一致（逐条重算总和等于账内累计）。
    let elapsed_ok = match &keys {
        Ok(l) => elapsed_audit(l),
        Err(_) => false,
    };
    out.push((
        "F49036/耗时记账",
        elapsed_ok,
        format!(
            "total={:?} recomputed={}",
            keys.as_ref().map(|l| l.total_elapsed),
            elapsed_ok
        ),
    ));

    // F49036 ③：结果点一致 + fuzz ×10^4 零崩（接受/拒绝守恒且全有码）。
    let fz = probe_fuzz(FUZZ_ROUNDS, FUZZ_SEED);
    let result_ok = match (&keys, &fz) {
        (Ok(l), Ok(f)) => {
            l.failures == 0
                && f.rounds == FUZZ_ROUNDS
                && f.conserved()
                && f.accepted > 0
                && f.rejected > 0
                && f.digest != 0
        }
        _ => false,
    };
    out.push((
        "F49036/结果一致",
        result_ok,
        format!(
            "key_failures={:?} fuzz_accepted={:?} fuzz_rejected={:?}",
            keys.as_ref().map(|l| l.failures),
            fz.as_ref().map(|l| l.accepted),
            fz.as_ref().map(|l| l.rejected)
        ),
    ));

    // F49036 ④：复跑零漂移 + 渲染六字段 + 段位守卫。
    let rep = probe_replay();
    let rep_ok = match &rep {
        Ok((a, b)) => a.digest == b.digest && a.digest != 0 && a.conserved() && b.conserved(),
        Err(_) => false,
    };
    let sample = render_row(PROBE_TAG, FUZZ_ROUNDS, 1, 2, STAGES as u32, 0x0123_4567_89AB_CDEF);
    let sample_ok = match row_fields(&sample) {
        Some((t, v, d)) => {
            t == PROBE_TAG
                && v[0] == FUZZ_ROUNDS
                && v[1] == 1
                && v[2] == 2
                && v[3] == STAGES as u32
                && d == 0x0123_4567_89AB_CDEF
        }
        None => false,
    };
    let fz_row_ok = match &fz {
        Ok(l) => match row_fields(&fuzz_row(l)) {
            Some((t, v, d)) => {
                t == PROBE_TAG && v[0] == l.rounds && v[1] == l.accepted && v[2] == l.rejected && d == l.digest
            }
            None => false,
        },
        Err(_) => false,
    };
    out.push((
        "F49036/复跑与段位",
        rep_ok && sample_ok && fz_row_ok && audit_render(&sample) && segment_guard(),
        format!(
            "digest={:016X} replay={} render={} seg={}",
            match &rep {
                Ok((a, _)) => a.digest,
                Err(_) => 0,
            },
            rep_ok,
            sample_ok && fz_row_ok,
            segment_guard()
        ),
    ));

    out
}

pub fn run_unx49035_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49036 逐键收尾错误码族与四点观测埋点");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_family_is_windowed_and_unique() {
        assert!(err_ok());
        assert!(err_zero_drift());
        assert_ne!(err_digest(), 0);
        assert_eq!(ERR_FAMILY.len(), ERR_SIZE);
        assert_eq!(ERR_FAMILY[0].code, ERR_LO);
        assert_eq!(ERR_FAMILY[ERR_SIZE - 1].code, ERR_HI);
        assert_eq!(err_lookup(0x71D10), Some(0));
        assert_eq!(err_lookup(0x71D1E), Some(14));
        assert_eq!(err_lookup(0x71D0F), None);
        assert_eq!(err_lookup_name("trace-entry-missing"), Some(10));
        assert_eq!(err_lookup_name(""), None);
        assert_eq!(expected_stage(0x71D1B), Some(1));
        assert_eq!(expected_stage(0x71D0F), None);
        assert_eq!(expected_family(0x71D17), Some(1));
        assert_eq!(expected_family(0x9999), None);
    }

    #[test]
    fn span_four_points_roundtrip() {
        let mut s = Span::new(0x10, 1);
        assert_eq!(s.coverage(), 0);
        assert_eq!(s.audit(BUDGET), Err(ProbeCode::EntryBad.val()));
        assert!(s.open(100).is_ok());
        assert_eq!(s.coverage(), 1);
        assert_eq!(s.audit(BUDGET), Err(ProbeCode::ExitBad.val()));
        assert!(s.close(120).is_ok());
        assert_eq!(s.elapsed(), Some(20));
        assert_eq!(s.coverage(), 3);
        assert_eq!(s.audit(BUDGET), Err(ProbeCode::ResultBad.val()));
        assert!(s.settle(0).is_ok());
        assert_eq!(s.coverage(), 4);
        assert_eq!(s.audit(BUDGET), Ok(()));
        assert_ne!(s.digest(), 0);
    }

    #[test]
    fn span_order_violations_are_coded() {
        let mut a = Span::new(0x10, 1);
        assert_eq!(a.close(10), Err(ProbeCode::OrderBad.val()));
        let mut b = Span::new(0x10, 1);
        assert!(b.open(10).is_ok());
        assert_eq!(b.open(11), Err(ProbeCode::EntryBad.val()));
        assert!(b.close(20).is_ok());
        assert_eq!(b.close(21), Err(ProbeCode::ExitBad.val()));
        let mut c = Span::new(0x10, 1);
        assert!(c.open(10).is_ok());
        assert_eq!(c.settle(0), Err(ProbeCode::ResultBad.val()));
        let mut d = Span::new(0x10, 1);
        assert!(d.open(100).is_ok());
        assert!(d.close(90).is_ok());
        assert_eq!(d.elapsed(), None);
        assert_eq!(d.coverage(), 2);
        let _ = d.settle(0);
        assert_eq!(d.audit(BUDGET), Err(ProbeCode::OrderBad.val()));
    }

    #[test]
    fn span_key_and_criterion_checked_first() {
        let mut s = Span::new(0xFF, 1);
        assert!(s.open(0).is_ok());
        assert!(s.close(10).is_ok());
        assert!(s.settle(0).is_ok());
        assert_eq!(s.audit(BUDGET), Err(ProbeCode::KeyBad.val()));
        let mut t = Span::new(0x10, 0);
        assert!(t.open(0).is_ok());
        assert!(t.close(10).is_ok());
        assert!(t.settle(0).is_ok());
        assert_eq!(t.audit(BUDGET), Err(ProbeCode::CriterionBad.val()));
        assert_eq!(expected_key(0xFF, 1), Err(ProbeCode::KeyBad.val()));
        assert_eq!(expected_key(0x04, 0), Err(ProbeCode::CriterionBad.val()));
        assert_eq!(expected_key(0x04, 3), Err(ProbeCode::CriterionBad.val()));
        assert_eq!(expected_key(0x04, 1), Ok(0x1000_0004));
    }

    #[test]
    fn elapsed_budget_is_enforced() {
        let mut s = Span::new(0x10, 1);
        assert!(s.open(0).is_ok());
        assert!(s.close(BUDGET + 1).is_ok());
        assert!(s.settle(0).is_ok());
        assert_eq!(s.audit(BUDGET), Err(ProbeCode::ElapsedBad.val()));
        let mut t = Span::new(0x10, 1);
        assert!(t.open(0).is_ok());
        assert!(t.close(BUDGET).is_ok());
        assert!(t.settle(0).is_ok());
        assert_eq!(t.audit(BUDGET), Ok(()));
    }

    #[test]
    fn screen_is_fully_covered() {
        let l = match screen_keys() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.spans, SPAN_TOTAL);
        assert_eq!(l.four_point, SPAN_TOTAL);
        assert_eq!(l.failures, 0);
        assert!(l.total_elapsed > 0);
        assert_ne!(l.digest, 0);
        assert!(elapsed_audit(&l));
    }

    #[test]
    fn injection_is_all_caught_and_classified() {
        assert_eq!(fail_inject(), Ok((FAIL_INJECT, FAIL_INJECT)));
        let (p, r) = match partial_detect() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(p, 4);
        assert_eq!(r, 4);
    }

    #[test]
    fn led_pairs_are_partitioned() {
        assert_eq!(toggle_bits(0, 0), 0);
        assert_eq!(toggle_bits(0, 1), 1);
        assert_eq!(toggle_bits(0, 7), 3);
        assert_eq!(led_pair_ok(0, 0), Ok(()));
        assert_eq!(led_pair_ok(0, 1), Ok(()));
        assert_eq!(led_pair_ok(0, 3), Err(ProbeCode::LedStateBad.val()));
        assert_eq!(led_pair_ok(0, 8), Err(ProbeCode::LedBad.val()));
        let l = match led_closing() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.pairs, LED_PAIRS);
        assert_eq!(l.legal, LED_LEGAL);
        assert_eq!(l.illegal, LED_PAIRS - LED_LEGAL);
        assert_eq!(l.four_point, LED_PAIRS);
        assert_ne!(l.digest, 0);
    }

    #[test]
    fn probe_fuzz_conserved() {
        let l = match probe_fuzz(FUZZ_ROUNDS, FUZZ_SEED) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.rounds, FUZZ_ROUNDS);
        assert!(l.conserved());
        assert_eq!(l.accepted + l.rejected, FUZZ_ROUNDS);
        assert_eq!(l.classified, l.rejected);
        assert!(l.accepted > 0 && l.rejected > 0);
        assert_ne!(l.digest, 0);
    }

    #[test]
    fn probe_fuzz_replay_and_bounds() {
        let (a, b) = match probe_replay() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(a.digest, b.digest);
        assert_ne!(a.digest, 0);
        assert_eq!(probe_fuzz(0, FUZZ_SEED), Err(ProbeCode::FuzzBad.val()));
        assert_eq!(probe_fuzz(FUZZ_ROUNDS + 1, FUZZ_SEED), Err(ProbeCode::FuzzBad.val()));
    }

    #[test]
    fn build_probe_classifies_each_fault() {
        assert_eq!(build_probe(0x04, 1, BUDGET, 10, 0), Ok(()));
        assert_eq!(build_probe(0x04, 1, BUDGET, 10, 1), Err(ProbeCode::EntryBad.val()));
        assert_eq!(build_probe(0x04, 1, BUDGET, 10, 2), Err(ProbeCode::ExitBad.val()));
        assert_eq!(build_probe(0x04, 1, BUDGET, 10, 3), Err(ProbeCode::OrderBad.val()));
        assert_eq!(build_probe(0x04, 1, BUDGET, 10, 4), Err(ProbeCode::ResultBad.val()));
        assert_eq!(build_probe(0x04, 1, BUDGET, 10, 5), Err(ProbeCode::ExitBad.val()));
        assert_eq!(build_probe(0x04, 1, BUDGET, 10, 6), Err(ProbeCode::ExitBad.val()));
        assert_eq!(build_probe(0x04, 1, BUDGET, 10, 7), Err(ProbeCode::ElapsedBad.val()));
        assert_eq!(build_probe(0xFF, 1, BUDGET, 10, 0), Err(ProbeCode::KeyBad.val()));
        assert_eq!(build_probe(0x04, 0, BUDGET, 10, 0), Err(ProbeCode::CriterionBad.val()));
    }

    #[test]
    fn render_roundtrip() {
        let row = render_row("P", 10, 20, 30, 4, 0x0FED_CBA9_8765_4321);
        assert!(audit_render(&row));
        let (t, v, d) = match row_fields(&row) {
            Some(x) => x,
            None => return assert!(false),
        };
        assert_eq!(t, "P");
        assert_eq!(v, [10, 20, 30, 4]);
        assert_eq!(d, 0x0FED_CBA9_8765_4321);
        assert!(!audit_render("P|rounds=1"));
        assert!(parse_u32("0").is_some());
        assert!(parse_u32("9x").is_none());
        assert!(parse_hex16("00000000000000FF").is_some());
        assert!(parse_hex16("00000000000000F").is_none());
    }

    #[test]
    fn rejected_codes_are_known() {
        let mut i = 0usize;
        while i < ProbeCode::ALL.len() {
            let c = ProbeCode::ALL[i];
            assert!(known_code(c.val()));
            assert_ne!(detail(c.val()), UNKNOWN);
            i += 1;
        }
        assert!(!known_code(0));
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 14), ProbeCode::Overflow.detail());
    }

    #[test]
    fn segment_window_is_exact() {
        assert!(segment_guard());
        assert_eq!(ProbeCode::KeyBad.val(), SEG_BASE + 1);
        assert_eq!(ProbeCode::Overflow.val(), SEG_BASE + 14);
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let s = run_unx49035_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
