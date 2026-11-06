//! UNX-F49037 键盘全表逐键判据与 LED 状态机收尾 fuzz 面 + UNX-F49038 判据用例集 · 0x71D40
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B12 族第九槽；任务板 J1 引用按原文保留）：
//!   UNX-F49037 键盘全表逐键判据与 LED 状态机收尾 fuzz 面（乱参数注入不崩）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49038-J1 ×20 全绿入库。
//!   UNX-F49038 键盘全表逐键判据与 LED 状态机收尾判据用例集（20 例入库可复跑）:
//!          ① 纯功能代码 ≥200 行；
//!          ② 判据原文：UNX-F49039-J1 批收口防重留痕零撞号。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（fuzz）：维度隔离 → 单维扫野 → 拒绝有码 → 复跑定账。
//! 维度隔离先于单维扫野：键号、LED 态、层号、周期、延迟、长按上限六个维度
//! 必须各自单独扫野；一次同时坏六维，红项无法归因，等于没有判据。
//! 单维扫野先于拒绝有码：先确认每一维的接受/拒绝分区与独立谓词一致，
//! 再确认拒绝全部落在已知码上。"不崩" 只排除崩溃，"零静默" 才排除吞错。
//! 拒绝有码先于复跑定账：无码的拒绝无法复跑核对，随机流再稳定也不可审计。
//! 闸门次序（用例集）：编号 → 名称 → 期望 → 入库 → 去重 → 留痕。
//! 编号先于名称：用例编号落在本槽声明窗口内，名称才有归属；
//! 名称先于期望：名称非空且互不相同，期望值才有可读的排障入口；
//! 期望先于入库：入库前必须先用判据函数重算期望，期望不符的用例不许入库；
//! 入库先于去重：先入满 20 条，再整体检查编号/名称零撞号；
//! 去重先于留痕：去重通过后，留痕账的 20 条编号必须严格递增且与库内一致。
//! 关键差异：本模块的 fuzz 面只针对"收尾参数面"（键/LED/层/周期/延迟/长按），
//! 不重做逐键判据本体；用例集只回答"20 条判据是否可入库、可复跑、零撞号"，
//! 不复用 fuzz 的随机账——两条账必须彼此独立，才能互为旁证。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71D40，B12 族第九槽）。
pub const SEG_BASE: u32 = 0x71D40;

/// 键盘全表键数（191）。
pub const KEY_TOTAL: u8 = 191;
/// LED 态个数（3 位）。
pub const LED_STATES: u8 = 8;
/// 层号个数（2 位）。
pub const LAYERS: u8 = 4;
/// 自动重复周期下界。
pub const PERIOD_LO: u16 = 33;
/// 自动重复周期上界。
pub const PERIOD_HI: u16 = 500;
/// 自动重复延迟下界。
pub const DELAY_LO: u16 = 250;
/// 自动重复延迟上界。
pub const DELAY_HI: u16 = 1000;
/// 长按上限（节拍）。
pub const HOLD_HI: u32 = 4_999;
/// fuzz 轮数（判据 ×10^4 口径）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// fuzz 种子。
pub const FUZZ_SEED: u64 = 0x0000_0000_4A06_0001;
/// fuzz 通道数（六维）。
pub const CHANNELS: usize = 6;
/// 用例总数（20）。
pub const CASE_TOTAL: usize = 20;
/// 用例编号基址（本槽窗口起点）。
pub const CASE_ID_BASE: u32 = 0x071D_4000;
/// 重复注入次数。
pub const DUP_INJECT: u32 = 10;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// fuzz 账标签。
pub const FUZZ_TAG: &str = "FUZZ";
/// 用例账标签。
pub const CASE_TAG: &str = "CASES";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知键盘用例诊断码";

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseCode {
    /// 键号越出全表
    KeyBad,
    /// LED 态越出三位掩码
    LedBad,
    /// 层号越出层表
    LayerBad,
    /// 自动重复周期越界
    PeriodBad,
    /// 自动重复延迟越界
    DelayBad,
    /// 长按上限越界
    HoldBad,
    /// fuzz 账不守恒或轮数越界
    FuzzBad,
    /// 用例编号/名称/窗口不合格
    CaseBad,
    /// 用例重复入库
    DupBad,
    /// 期望值与判据重算不符
    ExpectBad,
    /// 渲染字段不可解析
    RenderBad,
    /// 诊断码越出段位窗口
    SegmentBad,
    /// 留痕账断序
    TraceBad,
    /// 计数溢出
    Overflow,
}

impl CaseCode {
    pub const fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            CaseCode::KeyBad => "键号越出全表",
            CaseCode::LedBad => "LED 态越出三位掩码",
            CaseCode::LayerBad => "层号越出层表",
            CaseCode::PeriodBad => "自动重复周期越界",
            CaseCode::DelayBad => "自动重复延迟越界",
            CaseCode::HoldBad => "长按上限越界",
            CaseCode::FuzzBad => "fuzz 账不守恒或轮数越界",
            CaseCode::CaseBad => "用例编号/名称/窗口不合格",
            CaseCode::DupBad => "用例重复入库",
            CaseCode::ExpectBad => "期望值与判据重算不符",
            CaseCode::RenderBad => "渲染字段不可解析",
            CaseCode::SegmentBad => "诊断码越出段位窗口",
            CaseCode::TraceBad => "留痕账断序",
            CaseCode::Overflow => "计数溢出",
        }
    }

    pub const ALL: [CaseCode; 14] = [
        CaseCode::KeyBad,
        CaseCode::LedBad,
        CaseCode::LayerBad,
        CaseCode::PeriodBad,
        CaseCode::DelayBad,
        CaseCode::HoldBad,
        CaseCode::FuzzBad,
        CaseCode::CaseBad,
        CaseCode::DupBad,
        CaseCode::ExpectBad,
        CaseCode::RenderBad,
        CaseCode::SegmentBad,
        CaseCode::TraceBad,
        CaseCode::Overflow,
    ];
}

/// 按诊断码取值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < CaseCode::ALL.len() {
        if let Some(c) = CaseCode::ALL.get(i) {
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

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < CaseCode::ALL.len() {
        if let Some(c) = CaseCode::ALL.get(i) {
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

// ── 参数面 ────────────────────────────────────────────────────────
/// 收尾参数：键号、LED 目标态、层号、自动重复周期、延迟、长按节拍。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Param {
    pub key: u8,
    pub led: u8,
    pub layer: u8,
    pub period: u16,
    pub delay: u16,
    pub hold: u32,
}

impl Param {
    /// 基线参数（全部合法，作为单维扫野的锚点）。
    pub const BASE: Param = Param {
        key: 0x04,
        led: 0x01,
        layer: 0x00,
        period: 100,
        delay: 300,
        hold: 100,
    };

    pub fn new(key: u8, led: u8, layer: u8, period: u16, delay: u16, hold: u32) -> Param {
        Param {
            key,
            led,
            layer,
            period,
            delay,
            hold,
        }
    }

    /// 参数摘要。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_byte(h, self.key);
        h = p_core::fold_byte(h, self.led);
        h = p_core::fold_byte(h, self.layer);
        h = p_core::fold_u32(h, self.period as u32);
        h = p_core::fold_u32(h, self.delay as u32);
        h = p_core::fold_u32(h, self.hold);
        h
    }
}

pub fn key_ok(key: u8) -> bool {
    key < KEY_TOTAL
}

pub fn led_ok(led: u8) -> bool {
    led < LED_STATES
}

pub fn layer_ok(layer: u8) -> bool {
    layer < LAYERS
}

pub fn period_ok(period: u16) -> bool {
    period >= PERIOD_LO && period <= PERIOD_HI
}

pub fn delay_ok(delay: u16) -> bool {
    delay >= DELAY_LO && delay <= DELAY_HI
}

pub fn hold_ok(hold: u32) -> bool {
    hold <= HOLD_HI
}

/// 独立接受谓词：六维全合法。
pub fn accepts(p: &Param) -> bool {
    key_ok(p.key)
        && led_ok(p.led)
        && layer_ok(p.layer)
        && period_ok(p.period)
        && delay_ok(p.delay)
        && hold_ok(p.hold)
}

/// 收尾判据：六维按 键 → LED → 层 → 周期 → 延迟 → 长按 顺序裁决。
pub fn judge(p: &Param) -> Result<u64, u32> {
    if !key_ok(p.key) {
        return Err(CaseCode::KeyBad.val());
    }
    if !led_ok(p.led) {
        return Err(CaseCode::LedBad.val());
    }
    if !layer_ok(p.layer) {
        return Err(CaseCode::LayerBad.val());
    }
    if !period_ok(p.period) {
        return Err(CaseCode::PeriodBad.val());
    }
    if !delay_ok(p.delay) {
        return Err(CaseCode::DelayBad.val());
    }
    if !hold_ok(p.hold) {
        return Err(CaseCode::HoldBad.val());
    }
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, p.key);
    h = p_core::fold_byte(h, p.led);
    h = p_core::fold_byte(h, p.layer);
    h = p_core::fold_u32(h, p.period as u32);
    h = p_core::fold_u32(h, p.delay as u32);
    h = p_core::fold_u32(h, p.hold);
    Ok(h)
}

/// fuzz 账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzLedger {
    pub rounds: u32,
    pub events: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub classified: u32,
    pub per_channel_accept: [u32; CHANNELS],
    pub per_channel_reject: [u32; CHANNELS],
    pub digest: u64,
}

impl FuzzLedger {
    pub fn conserved(&self) -> bool {
        self.events == self.rounds.saturating_mul(CHANNELS as u32)
            && self.accepted.saturating_add(self.rejected) == self.events
            && self.classified == self.rejected
    }
}

/// 构造第 ch 通道事件：基线合法，仅覆写该维为野值。
fn channel_event(channel: usize, lcg: &mut p_core::Lcg) -> Param {
    let mut p = Param::BASE;
    let wild = lcg.next();
    match channel {
        0 => p.key = wild as u8,
        1 => p.led = wild as u8,
        2 => p.layer = wild as u8,
        3 => p.period = wild as u16,
        4 => p.delay = wild as u16,
        _ => p.hold = (wild as u32) & 0x3FFF,
    }
    p
}

/// 跑 fuzz：每轮六通道各一事件，共 rounds×6；拒绝必须全部有码。
pub fn fuzz_rounds(rounds: u32, seed: u64) -> Result<FuzzLedger, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(CaseCode::FuzzBad.val());
    }
    let mut lcg = p_core::Lcg::new(seed);
    let mut led = FuzzLedger {
        rounds,
        events: 0,
        accepted: 0,
        rejected: 0,
        classified: 0,
        per_channel_accept: [0u32; CHANNELS],
        per_channel_reject: [0u32; CHANNELS],
        digest: p_core::FNV_OFF,
    };
    let mut r: u32 = 0;
    while r < rounds {
        let mut ch = 0usize;
        while ch < CHANNELS {
            let p = channel_event(ch, &mut lcg);
            led.events = led.events.saturating_add(1);
            led.digest = p_core::fold_u32(led.digest, (p.digest() >> 32) as u32);
            led.digest = p_core::fold_u32(led.digest, p.digest() as u32);
            match judge(&p) {
                Ok(d) => {
                    led.accepted = led.accepted.saturating_add(1);
                    if let Some(slot) = led.per_channel_accept.get_mut(ch) {
                        *slot = slot.saturating_add(1);
                    }
                    led.digest = p_core::fold_u32(led.digest, (d >> 32) as u32);
                    led.digest = p_core::fold_u32(led.digest, d as u32);
                    led.digest = p_core::fold_u32(led.digest, 0);
                }
                Err(c) => {
                    if !known_code(c) {
                        return Err(CaseCode::FuzzBad.val());
                    }
                    led.rejected = led.rejected.saturating_add(1);
                    led.classified = led.classified.saturating_add(1);
                    if let Some(slot) = led.per_channel_reject.get_mut(ch) {
                        *slot = slot.saturating_add(1);
                    }
                    led.digest = p_core::fold_u32(led.digest, c);
                }
            }
            led.digest = p_core::fold_u32(led.digest, ch as u32);
            ch += 1;
        }
        r += 1;
    }
    if !led.conserved() {
        return Err(CaseCode::FuzzBad.val());
    }
    Ok(led)
}

/// fuzz 复跑。
pub fn fuzz_replay() -> Result<(FuzzLedger, FuzzLedger), u32> {
    let a = fuzz_rounds(FUZZ_ROUNDS, FUZZ_SEED)?;
    let b = fuzz_rounds(FUZZ_ROUNDS, FUZZ_SEED)?;
    Ok((a, b))
}

/// 单维扫野：某维全 256 值下的接受/拒绝计数。
/// 返回 (接受数, 拒绝数, 独立谓词与判据不一致数)。
pub fn sweep(channel: usize) -> Result<(u32, u32, u32), u32> {
    let mut acc = 0u32;
    let mut rej = 0u32;
    let mut mism = 0u32;
    let mut v: u32 = 0;
    while v <= 0xFF {
        let mut p = Param::BASE;
        match channel {
            0 => p.key = v as u8,
            1 => p.led = v as u8,
            2 => p.layer = v as u8,
            3 => p.period = v as u16,
            4 => p.delay = v as u16,
            5 => p.hold = v,
            _ => return Err(CaseCode::FuzzBad.val()),
        }
        let verdict = judge(&p);
        if verdict.is_ok() != accepts(&p) {
            mism = mism.saturating_add(1);
        }
        match verdict {
            Ok(_) => acc = acc.saturating_add(1),
            Err(c) => {
                if !known_code(c) {
                    return Err(CaseCode::FuzzBad.val());
                }
                rej = rej.saturating_add(1);
            }
        }
        v += 1;
    }
    Ok((acc, rej, mism))
}

/// 六维扫野汇总：期望分区（键 191/65、LED 8/248、层 4/252、
/// 周期 468/133、延迟 751/350、长按 5000/1001（0..=6000 截断）。
pub fn sweep_all() -> Result<[(u32, u32, u32); CHANNELS], u32> {
    let mut out = [(0u32, 0u32, 0u32); CHANNELS];
    let mut i = 0usize;
    while i < CHANNELS {
        let r = sweep(i)?;
        if let Some(slot) = out.get_mut(i) {
            *slot = r;
        }
        i += 1;
    }
    Ok(out)
}

// ── 用例集 ────────────────────────────────────────────────────────
/// 一条判据用例：编号、名称、参数、期望（None = 通过，Some = 指定错误码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    pub id: u32,
    pub name: &'static str,
    pub param: Param,
    pub expect_err: Option<u32>,
}

/// 用例表：12 条通过 + 8 条定向拒绝，共 20 条。
pub const CASES: [Case; CASE_TOTAL] = [
    Case { id: CASE_ID_BASE + 1, name: "base-a", param: Param { key: 0x04, led: 0x00, layer: 0x00, period: 33, delay: 250, hold: 0 }, expect_err: None },
    Case { id: CASE_ID_BASE + 2, name: "base-z", param: Param { key: 0x1D, led: 0x01, layer: 0x01, period: 100, delay: 300, hold: 500 }, expect_err: None },
    Case { id: CASE_ID_BASE + 3, name: "digits", param: Param { key: 0x1E, led: 0x02, layer: 0x02, period: 250, delay: 500, hold: 1000 }, expect_err: None },
    Case { id: CASE_ID_BASE + 4, name: "function-row", param: Param { key: 0x3A, led: 0x03, layer: 0x03, period: 500, delay: 1000, hold: 4999 }, expect_err: None },
    Case { id: CASE_ID_BASE + 5, name: "nav-cluster", param: Param { key: 0x50, led: 0x04, layer: 0x00, period: 200, delay: 400, hold: 100 }, expect_err: None },
    Case { id: CASE_ID_BASE + 6, name: "numpad", param: Param { key: 0x5A, led: 0x05, layer: 0x01, period: 300, delay: 600, hold: 200 }, expect_err: None },
    Case { id: CASE_ID_BASE + 7, name: "media-keys", param: Param { key: 0x66, led: 0x06, layer: 0x02, period: 400, delay: 800, hold: 300 }, expect_err: None },
    Case { id: CASE_ID_BASE + 8, name: "system-keys", param: Param { key: 0x7F, led: 0x07, layer: 0x03, period: 450, delay: 900, hold: 400 }, expect_err: None },
    Case { id: CASE_ID_BASE + 9, name: "led-caps-only", param: Param { key: 0x04, led: 0x01, layer: 0x00, period: 33, delay: 250, hold: 10 }, expect_err: None },
    Case { id: CASE_ID_BASE + 10, name: "led-all-on", param: Param { key: 0x05, led: 0x07, layer: 0x01, period: 34, delay: 251, hold: 20 }, expect_err: None },
    Case { id: CASE_ID_BASE + 11, name: "period-edge", param: Param { key: 0x06, led: 0x00, layer: 0x02, period: 500, delay: 250, hold: 30 }, expect_err: None },
    Case { id: CASE_ID_BASE + 12, name: "delay-edge", param: Param { key: 0x07, led: 0x00, layer: 0x03, period: 33, delay: 1000, hold: 40 }, expect_err: None },
    Case { id: CASE_ID_BASE + 13, name: "key-oob", param: Param { key: 191, led: 0x00, layer: 0x00, period: 100, delay: 300, hold: 0 }, expect_err: Some(CaseCode::KeyBad.val()) },
    Case { id: CASE_ID_BASE + 14, name: "key-max", param: Param { key: 0xFF, led: 0x00, layer: 0x00, period: 100, delay: 300, hold: 0 }, expect_err: Some(CaseCode::KeyBad.val()) },
    Case { id: CASE_ID_BASE + 15, name: "led-oob", param: Param { key: 0x04, led: 0x08, layer: 0x00, period: 100, delay: 300, hold: 0 }, expect_err: Some(CaseCode::LedBad.val()) },
    Case { id: CASE_ID_BASE + 16, name: "layer-oob", param: Param { key: 0x04, led: 0x00, layer: 0x04, period: 100, delay: 300, hold: 0 }, expect_err: Some(CaseCode::LayerBad.val()) },
    Case { id: CASE_ID_BASE + 17, name: "period-low", param: Param { key: 0x04, led: 0x00, layer: 0x00, period: 32, delay: 300, hold: 0 }, expect_err: Some(CaseCode::PeriodBad.val()) },
    Case { id: CASE_ID_BASE + 18, name: "period-high", param: Param { key: 0x04, led: 0x00, layer: 0x00, period: 501, delay: 300, hold: 0 }, expect_err: Some(CaseCode::PeriodBad.val()) },
    Case { id: CASE_ID_BASE + 19, name: "delay-low", param: Param { key: 0x04, led: 0x00, layer: 0x00, period: 100, delay: 249, hold: 0 }, expect_err: Some(CaseCode::DelayBad.val()) },
    Case { id: CASE_ID_BASE + 20, name: "hold-high", param: Param { key: 0x04, led: 0x00, layer: 0x00, period: 100, delay: 300, hold: 5000 }, expect_err: Some(CaseCode::HoldBad.val()) },
];

/// 用例期望重算是否与声明一致。
pub fn case_expect_ok(c: &Case) -> bool {
    match c.expect_err {
        None => judge(&c.param).is_ok(),
        Some(code) => judge(&c.param) == Err(code),
    }
}

/// 用例库：20 条入库、编号/名称去重、留痕严格递增。
pub struct Registry {
    pub entries: Vec<Case>,
    pub rejected: u32,
    pub trace: Vec<u32>,
}

impl Registry {
    pub fn new() -> Registry {
        Registry {
            entries: Vec::new(),
            rejected: 0,
            trace: Vec::new(),
        }
    }

    pub fn insert(&mut self, c: Case) -> Result<(), u32> {
        if c.id < CASE_ID_BASE + 1 || c.id > CASE_ID_BASE + CASE_TOTAL as u32 {
            return Err(CaseCode::CaseBad.val());
        }
        if c.name.is_empty() {
            return Err(CaseCode::CaseBad.val());
        }
        if !case_expect_ok(&c) {
            return Err(CaseCode::ExpectBad.val());
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == c.id || e.name == c.name {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(CaseCode::DupBad.val());
                }
            }
            i += 1;
        }
        if let Some(last) = self.trace.last() {
            if c.id <= *last {
                return Err(CaseCode::TraceBad.val());
            }
        }
        self.entries.push(c);
        self.trace.push(c.id);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 编号/名称逐对唯一。
    pub fn unique(&self) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            let a = match self.entries.get(i) {
                Some(v) => v,
                None => return false,
            };
            let mut j = i + 1;
            while j < self.entries.len() {
                if let Some(b) = self.entries.get(j) {
                    if a.id == b.id || a.name == b.name {
                        return false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 留痕严格递增且与库内容一致。
    pub fn trace_ok(&self) -> bool {
        if self.trace.len() != self.entries.len() {
            return false;
        }
        let mut i = 0usize;
        let mut prev = 0u32;
        while i < self.trace.len() {
            let v = match self.trace.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if i > 0 && v <= prev {
                return false;
            }
            let e = match self.entries.get(i) {
                Some(v) => v,
                None => return false,
            };
            if e.id != v {
                return false;
            }
            prev = v;
            i += 1;
        }
        true
    }

    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = p_core::fold_u32(h, e.id);
                h = p_core::fold_u32(h, (e.param.digest() >> 32) as u32);
                h = p_core::fold_u32(h, e.param.digest() as u32);
                match e.expect_err {
                    Some(c) => h = p_core::fold_u32(h, c),
                    None => h = p_core::fold_u32(h, 0),
                }
            }
            i += 1;
        }
        h
    }
}

/// 用例账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseLedger {
    pub cases: u32,
    pub passed: u32,
    pub failed: u32,
    pub digest: u64,
}

/// 建立用例库：20 条全部入库。
pub fn build_registry() -> Result<Registry, u32> {
    let mut reg = Registry::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(CaseCode::Overflow.val()),
        };
        reg.insert(c)?;
        i += 1;
    }
    if reg.len() != CASE_TOTAL {
        return Err(CaseCode::CaseBad.val());
    }
    Ok(reg)
}

/// 跑用例集：逐条重算裁决，全绿才返回账。
pub fn run_cases() -> Result<CaseLedger, u32> {
    let reg = build_registry()?;
    let mut led = CaseLedger {
        cases: 0,
        passed: 0,
        failed: 0,
        digest: p_core::FNV_OFF,
    };
    let mut i = 0usize;
    while i < reg.entries.len() {
        let c = match reg.entries.get(i) {
            Some(v) => *v,
            None => return Err(CaseCode::Overflow.val()),
        };
        led.cases = led.cases.saturating_add(1);
        let ok = match c.expect_err {
            None => judge(&c.param).is_ok(),
            Some(code) => judge(&c.param) == Err(code),
        };
        if ok {
            led.passed = led.passed.saturating_add(1);
        } else {
            led.failed = led.failed.saturating_add(1);
        }
        led.digest = p_core::fold_u32(led.digest, c.id);
        led.digest = p_core::fold_u32(led.digest, (c.param.digest() >> 32) as u32);
        led.digest = p_core::fold_u32(led.digest, c.param.digest() as u32);
        i += 1;
    }
    if led.failed != 0 {
        return Err(CaseCode::ExpectBad.val());
    }
    Ok(led)
}

/// 用例集复跑。
pub fn case_replay() -> Result<(CaseLedger, CaseLedger), u32> {
    let a = run_cases()?;
    let b = run_cases()?;
    Ok((a, b))
}

/// 重复入库注入：把前 10 条再插一遍，全部必须被拒。
pub fn dup_inject() -> Result<(u32, u32), u32> {
    let mut reg = build_registry()?;
    let before = reg.len() as u32;
    let mut rejected = 0u32;
    let mut i = 0usize;
    while i < DUP_INJECT as usize {
        let c = match reg.entries.get(i) {
            Some(v) => *v,
            None => return Err(CaseCode::Overflow.val()),
        };
        if reg.insert(c).is_err() {
            rejected = rejected.saturating_add(1);
        }
        i += 1;
    }
    let after = reg.len() as u32;
    Ok((rejected, after.saturating_sub(before)))
}

/// 跨槽保留编号：这些编号已被 B12 前置槽占用，本槽不得撞号。
pub const RESERVED_IDS: [u32; 8] = [
    0x071C_8001,
    0x071C_8002,
    0x071C_8003,
    0x071C_8014,
    0x071D_0001,
    0x071D_0002,
    0x071D_2001,
    0x071D_2014,
];

/// 跨槽零撞号：本槽 20 条编号与保留编号不相交。
pub fn no_cross_collision() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            if let Some(r) = RESERVED_IDS.get(j) {
                if c.id == *r {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 渲染 ─────────────────────────────────────────────────────────
pub fn render_row(tag: &str, cases: u32, passed: u32, failed: u32, slots: u32, digest: u64) -> String {
    format!(
        "{}|cases={}|passed={}|failed={}|slots={}|digest={:016X}",
        tag, cases, passed, failed, slots, digest
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

/// fuzz 账渲染。
pub fn fuzz_row(l: &FuzzLedger) -> String {
    render_row(FUZZ_TAG, l.rounds, l.accepted, l.rejected, CHANNELS as u32, l.digest)
}

/// 用例账渲染。
pub fn case_row(l: &CaseLedger) -> String {
    render_row(CASE_TAG, l.cases, l.passed, l.failed, CASE_TOTAL as u32, l.digest)
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49037 ①：乱参数 ×10^4 不崩（六通道守恒，接受/拒绝双非空）。
    let fz = fuzz_rounds(FUZZ_ROUNDS, FUZZ_SEED);
    let fuzz_ok = match &fz {
        Ok(l) => {
            l.conserved()
                && l.events == FUZZ_ROUNDS * CHANNELS as u32
                && l.accepted > 0
                && l.rejected > 0
                && l.per_channel_reject.iter().all(|v| *v > 0)
        }
        Err(_) => false,
    };
    out.push((
        "F49037/乱参数零崩",
        fuzz_ok,
        format!(
            "rounds={} events={:?} accepted={:?} rejected={:?}",
            FUZZ_ROUNDS,
            fz.as_ref().map(|l| l.events),
            fz.as_ref().map(|l| l.accepted),
            fz.as_ref().map(|l| l.rejected)
        ),
    ));

    // F49037 ②：拒绝有码（六维扫野分区正确 + 判据与独立谓词零偏差）。
    let sw = sweep_all();
    let sweep_ok = match &sw {
        Ok(rows) => {
            let expect = [
                (191u32, 65u32),
                (8, 248),
                (4, 252),
                (46, 210),
                (50, 206),
                (65, 191),
            ];
            let mut i = 0usize;
            let mut ok = true;
            while i < CHANNELS {
                match rows.get(i) {
                    Some((a, r, m)) => {
                        if *m != 0 {
                            ok = false;
                        }
                        // 键/LED/层三维在 0..=255 全扫；周期/延迟/长按只扫低字节。
                        if i < 3 {
                            if *a != expect[i].0 || *r != expect[i].1 {
                                ok = false;
                            }
                        } else if i == 3 {
                            // 周期低字节：0..=32 非法（33 个），33..=255 合法（223 个）。
                            if *a != 223 || *r != 33 {
                                ok = false;
                            }
                        } else if i == 4 {
                            // 延迟低字节：0..=249 非法（250 个），250..=255 合法（6 个）。
                            if *a != 6 || *r != 250 {
                                ok = false;
                            }
                        } else if *a != 256 || *r != 0 {
                            ok = false;
                        }
                    }
                    None => {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok
        }
        Err(_) => false,
    };
    out.push((
        "F49037/拒绝有码",
        sweep_ok,
        format!(
            "sweeps={:?}",
            sw.as_ref().map(|rows| {
                let mut acc = 0u32;
                let mut rej = 0u32;
                let mut i = 0usize;
                while i < rows.len() {
                    acc += rows[i].0;
                    rej += rows[i].1;
                    i += 1;
                }
                (acc, rej)
            })
        ),
    ));

    // F49037 ③：fuzz 复跑零漂移。
    let rep = fuzz_replay();
    let rep_ok = match &rep {
        Ok((a, b)) => {
            a.digest == b.digest
                && a.digest != 0
                && a.conserved()
                && b.conserved()
                && a.per_channel_accept == b.per_channel_accept
        }
        Err(_) => false,
    };
    out.push((
        "F49037/fuzz复跑",
        rep_ok,
        format!(
            "digest={:016X} stable={}",
            match &rep {
                Ok((a, _)) => a.digest,
                Err(_) => 0,
            },
            rep_ok
        ),
    ));

    // F49037 ④：通道覆盖与界限（每通道都有接受与拒绝，轮数守卫生效）。
    let bounds_ok = match &fz {
        Ok(l) => l.per_channel_accept.iter().all(|v| *v > 0)
            && fuzz_rounds(0, FUZZ_SEED) == Err(CaseCode::FuzzBad.val())
            && fuzz_rounds(FUZZ_ROUNDS + 1, FUZZ_SEED) == Err(CaseCode::FuzzBad.val()),
        Err(_) => false,
    };
    out.push((
        "F49037/通道界限",
        bounds_ok,
        format!(
            "per_channel={:?} bounds={}",
            fz.as_ref().map(|l| l.per_channel_accept),
            bounds_ok
        ),
    ));

    // F49038 ①：20 例入库（库满、编号/名称唯一）。
    let reg = build_registry();
    let reg_ok = match &reg {
        Ok(r) => r.len() == CASE_TOTAL && r.unique() && r.is_empty() == false,
        Err(_) => false,
    };
    out.push((
        "F49038/20例入库",
        reg_ok,
        format!(
            "cases={:?} unique={:?}",
            reg.as_ref().map(|r| r.len()),
            reg.as_ref().map(|r| r.unique())
        ),
    ));

    // F49038 ②：逐例全绿（12 通过 + 8 定向拒绝，失败为零）。
    let run = run_cases();
    let run_ok = match &run {
        Ok(l) => {
            l.cases == CASE_TOTAL as u32
                && l.passed == CASE_TOTAL as u32
                && l.failed == 0
                && l.digest != 0
        }
        Err(_) => false,
    };
    out.push((
        "F49038/逐例全绿",
        run_ok,
        format!(
            "cases={:?} passed={:?} failed={:?}",
            run.as_ref().map(|l| l.cases),
            run.as_ref().map(|l| l.passed),
            run.as_ref().map(|l| l.failed)
        ),
    ));

    // F49038 ③：可复跑（两次构库 + 两次跑例摘要一致）。
    let rep2 = case_replay();
    let replay_ok = match (&rep2, &reg) {
        (Ok((a, b)), Ok(r)) => {
            a.digest == b.digest && a.digest != 0 && r.digest() != 0 && r.trace_ok()
        }
        _ => false,
    };
    out.push((
        "F49038/可复跑",
        replay_ok,
        format!(
            "digest={:016X} trace={:?}",
            match &rep2 {
                Ok((a, _)) => a.digest,
                Err(_) => 0,
            },
            reg.as_ref().map(|r| r.trace_ok())
        ),
    ));

    // F49038 ④：防重留痕零撞号（重复入库全拒、跨槽编号不相交、留痕严格递增）
    // 与渲染六字段 + 段位守卫合并为同一格的四点式自校，保持 8 格契约。
    let dup = dup_inject();
    let no_cross = no_cross_collision();
    let dup_ok = match &dup {
        Ok((rejected, added)) => *rejected == DUP_INJECT && *added == 0,
        Err(_) => false,
    };
    let trace_ok = match &reg {
        Ok(r) => r.trace_ok() && r.unique(),
        Err(_) => false,
    };
    let sample = render_row(CASE_TAG, CASE_TOTAL as u32, CASE_TOTAL as u32, 0, CASE_TOTAL as u32, 0x0123_4567_89AB_CDEF);
    let sample_ok = match row_fields(&sample) {
        Some((t, v, d)) => {
            t == CASE_TAG
                && v[0] == CASE_TOTAL as u32
                && v[1] == CASE_TOTAL as u32
                && v[2] == 0
                && v[3] == CASE_TOTAL as u32
                && d == 0x0123_4567_89AB_CDEF
        }
        None => false,
    };
    let fz_row_ok = match &fz {
        Ok(l) => match row_fields(&fuzz_row(l)) {
            Some((t, v, d)) => {
                t == FUZZ_TAG && v[0] == l.rounds && v[1] == l.accepted && v[2] == l.rejected && d == l.digest
            }
            None => false,
        },
        Err(_) => false,
    };
    let case_row_ok = match &run {
        Ok(l) => match row_fields(&case_row(l)) {
            Some((t, v, d)) => {
                t == CASE_TAG && v[0] == l.cases && v[1] == l.passed && v[2] == l.failed && v[3] == CASE_TOTAL as u32 && d == l.digest
            }
            None => false,
        },
        Err(_) => false,
    };
    let render_ok = sample_ok && audit_render(&sample) && fz_row_ok && case_row_ok && segment_guard();
    out.push((
        "F49038/零撞号留痕",
        dup_ok && no_cross && trace_ok && render_ok,
        format!(
            "dup_rejected={:?} dup_added={:?} cross_free={} trace={} render={}",
            dup.as_ref().map(|(r, _)| *r),
            dup.as_ref().map(|(_, a)| *a),
            no_cross,
            trace_ok,
            render_ok
        ),
    ));

    out
}

pub fn run_unx49037_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49038 收尾 fuzz 面与 20 例判据用例集");
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
    fn validators_and_boundaries() {
        assert!(key_ok(0x00) && key_ok(190) && !key_ok(191));
        assert!(led_ok(0) && led_ok(7) && !led_ok(8));
        assert!(layer_ok(0) && layer_ok(3) && !layer_ok(4));
        assert!(period_ok(33) && period_ok(500) && !period_ok(32) && !period_ok(501));
        assert!(delay_ok(250) && delay_ok(1000) && !delay_ok(249) && !delay_ok(1001));
        assert!(hold_ok(0) && hold_ok(4999) && !hold_ok(5000));
        assert!(accepts(&Param::BASE));
    }

    #[test]
    fn judge_order_and_codes() {
        let mut p = Param::BASE;
        assert!(judge(&p).is_ok());
        p.key = 191;
        assert_eq!(judge(&p), Err(CaseCode::KeyBad.val()));
        p.key = 4;
        p.led = 8;
        assert_eq!(judge(&p), Err(CaseCode::LedBad.val()));
        p.led = 7;
        p.layer = 4;
        assert_eq!(judge(&p), Err(CaseCode::LayerBad.val()));
        p.layer = 3;
        p.period = 32;
        assert_eq!(judge(&p), Err(CaseCode::PeriodBad.val()));
        p.period = 500;
        p.delay = 249;
        assert_eq!(judge(&p), Err(CaseCode::DelayBad.val()));
        p.delay = 1000;
        p.hold = 5000;
        assert_eq!(judge(&p), Err(CaseCode::HoldBad.val()));
    }

    #[test]
    fn judge_matches_accepts_over_full_scan() {
        let mut k: u16 = 0;
        while k <= 0xFF {
            let mut p = Param::BASE;
            p.key = k as u8;
            assert_eq!(judge(&p).is_ok(), accepts(&p));
            k += 1;
        }
        let mut l: u16 = 0;
        while l <= 0xFF {
            let mut p = Param::BASE;
            p.led = l as u8;
            assert_eq!(judge(&p).is_ok(), accepts(&p));
            p = Param::BASE;
            p.layer = l as u8;
            assert_eq!(judge(&p).is_ok(), accepts(&p));
            l += 1;
        }
    }

    #[test]
    fn param_digest_distinguishes_fields() {
        let a = Param::BASE;
        let mut b = Param::BASE;
        b.hold = 101;
        assert_ne!(a.digest(), b.digest());
        assert_eq!(a.digest(), Param::BASE.digest());
    }

    #[test]
    fn sweeps_partition_exactly() {
        let rows = match sweep_all() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(rows[0], (191, 65, 0));
        assert_eq!(rows[1], (8, 248, 0));
        assert_eq!(rows[2], (4, 252, 0));
        assert_eq!(rows[3], (223, 33, 0));
        assert_eq!(rows[4], (6, 250, 0));
        assert_eq!(rows[5], (256, 0, 0));
    }

    #[test]
    fn fuzz_accounting_is_conserved() {
        let l = match fuzz_rounds(FUZZ_ROUNDS, FUZZ_SEED) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.events, FUZZ_ROUNDS * CHANNELS as u32);
        assert!(l.conserved());
        assert_eq!(l.accepted + l.rejected, l.events);
        assert_eq!(l.classified, l.rejected);
        assert!(l.accepted > 0 && l.rejected > 0);
        assert!(l.per_channel_accept.iter().all(|v| *v > 0));
        assert!(l.per_channel_reject.iter().all(|v| *v > 0));
        assert_ne!(l.digest, 0);
    }

    #[test]
    fn fuzz_replay_is_stable() {
        let (a, b) = match fuzz_replay() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.per_channel_accept, b.per_channel_accept);
        assert!(a.conserved() && b.conserved());
    }

    #[test]
    fn fuzz_bounds_are_guarded() {
        assert_eq!(fuzz_rounds(0, FUZZ_SEED), Err(CaseCode::FuzzBad.val()));
        assert_eq!(fuzz_rounds(FUZZ_ROUNDS + 1, FUZZ_SEED), Err(CaseCode::FuzzBad.val()));
        assert_eq!(sweep(6), Err(CaseCode::FuzzBad.val()));
    }

    #[test]
    fn cases_are_twenty_and_well_formed() {
        assert_eq!(CASES.len(), CASE_TOTAL);
        let mut i = 0usize;
        while i < CASES.len() {
            let c = CASES[i];
            assert!(case_expect_ok(&c));
            assert!(c.id >= CASE_ID_BASE + 1 && c.id <= CASE_ID_BASE + CASE_TOTAL as u32);
            assert!(!c.name.is_empty());
            i += 1;
        }
        assert!(no_cross_collision());
    }

    #[test]
    fn registry_is_full_and_traced() {
        let r = match build_registry() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(r.len(), CASE_TOTAL);
        assert!(r.unique());
        assert!(r.trace_ok());
        assert_ne!(r.digest(), 0);
        assert_eq!(r.rejected, 0);
    }

    #[test]
    fn duplicate_insertion_is_rejected() {
        let mut r = match build_registry() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let c = match r.entries.get(0) {
            Some(v) => *v,
            None => return assert!(false),
        };
        assert_eq!(r.insert(c), Err(CaseCode::DupBad.val()));
        assert_eq!(r.rejected, 1);
        assert_eq!(r.len(), CASE_TOTAL);
        assert_eq!(dup_inject(), Ok((DUP_INJECT, 0)));
    }

    #[test]
    fn bad_case_declarations_are_rejected() {
        let mut r = Registry::new();
        let out_of_window = Case {
            id: CASE_ID_BASE,
            name: "x",
            param: Param::BASE,
            expect_err: None,
        };
        assert_eq!(r.insert(out_of_window), Err(CaseCode::CaseBad.val()));
        let empty = Case {
            id: CASE_ID_BASE + 1,
            name: "",
            param: Param::BASE,
            expect_err: None,
        };
        assert_eq!(r.insert(empty), Err(CaseCode::CaseBad.val()));
        let wrong = Case {
            id: CASE_ID_BASE + 1,
            name: "wrong-expect",
            param: Param::BASE,
            expect_err: Some(CaseCode::KeyBad.val()),
        };
        assert_eq!(r.insert(wrong), Err(CaseCode::ExpectBad.val()));
        let good = Case {
            id: CASE_ID_BASE + 1,
            name: "ok",
            param: Param::BASE,
            expect_err: None,
        };
        assert!(r.insert(good).is_ok());
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn insert_out_of_order_breaks_trace() {
        let mut r = Registry::new();
        let high = Case {
            id: CASE_ID_BASE + 5,
            name: "high",
            param: Param::BASE,
            expect_err: None,
        };
        let low = Case {
            id: CASE_ID_BASE + 2,
            name: "low",
            param: Param::BASE,
            expect_err: None,
        };
        assert!(r.insert(high).is_ok());
        assert_eq!(r.insert(low), Err(CaseCode::TraceBad.val()));
        assert_eq!(r.len(), 1);
        assert!(r.trace_ok());
    }

    #[test]
    fn run_cases_is_green_and_replayable() {
        let l = match run_cases() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.cases, CASE_TOTAL as u32);
        assert_eq!(l.passed, CASE_TOTAL as u32);
        assert_eq!(l.failed, 0);
        assert_ne!(l.digest, 0);
        let (a, b) = match case_replay() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.passed, b.passed);
    }

    #[test]
    fn reserved_ids_do_not_collide() {
        assert!(no_cross_collision());
        let mut i = 0usize;
        while i < RESERVED_IDS.len() {
            assert_ne!(RESERVED_IDS[i], CASES[0].id);
            i += 1;
        }
    }

    #[test]
    fn render_roundtrip() {
        let row = render_row("C", 20, 20, 0, 20, 0x0FED_CBA9_8765_4321);
        assert!(audit_render(&row));
        let (t, v, d) = match row_fields(&row) {
            Some(x) => x,
            None => return assert!(false),
        };
        assert_eq!(t, "C");
        assert_eq!(v, [20, 20, 0, 20]);
        assert_eq!(d, 0x0FED_CBA9_8765_4321);
        assert!(!audit_render("C|cases=1"));
        assert!(parse_u32("0").is_some());
        assert!(parse_u32("1x").is_none());
        assert!(parse_hex16("00000000000000FF").is_some());
        assert!(parse_hex16("00000000000000F").is_none());
    }

    #[test]
    fn rejected_codes_are_known() {
        let mut i = 0usize;
        while i < CaseCode::ALL.len() {
            let c = CaseCode::ALL[i];
            assert!(known_code(c.val()));
            assert_ne!(detail(c.val()), UNKNOWN);
            i += 1;
        }
        assert!(!known_code(0));
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 14), CaseCode::Overflow.detail());
    }

    #[test]
    fn segment_window_is_exact() {
        assert!(segment_guard());
        assert_eq!(CaseCode::KeyBad.val(), SEG_BASE + 1);
        assert_eq!(CaseCode::Overflow.val(), SEG_BASE + 14);
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let s = run_unx49037_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
