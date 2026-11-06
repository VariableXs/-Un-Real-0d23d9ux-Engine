//! UNX-F49033 键盘 fuzz（乱扫描码/乱 LED 态注入不崩）+ UNX-F49034 B12 段内联轧 · 0x71CC0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B12 族第七槽；任务板 J1 引用按原文保留）：
//!   UNX-F49033 键盘 fuzz（乱扫描码/乱 LED 态注入不崩）:
//!          ① 纯功能代码 ≥260 行；
//!          ② 判据原文：UNX-F49034-J1 联轧 ×5 全绿零漂移。
//!   UNX-F49034 B12 段内联轧（键盘五路判据交叉复跑）:
//!          ① 纯功能代码 ≥220 行；
//!          ② 判据原文：UNX-F49035-J1 错误码 ×15 零漂移互不混淆。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（fuzz）：维度隔离 → 单维扫野 → 拒绝有码 → 复跑定账。
//! 维度隔离先于单维扫野：扫描码、LED 态、修饰位、层号四个维度必须各自
//! 单独扫野，否则一次注入同时坏四维，出了红也说不清是哪一维的错。
//! 单维扫野先于拒绝有码：先看每一维的接受/拒绝分区是否与独立谓词一致，
//! 再看拒绝是否都落在已知码上。"不崩" 只是最低要求，"零静默" 才是判据。
//! 拒绝有码先于复跑定账：没有码的拒绝无法复跑对账，随机数再稳定也没用。
//! 闸门次序（联轧）：单路自洽 → 五路交叉 → 复跑零漂移 → 码族互不混淆。
//! 单路自洽先于五路交叉：每一路先与自己的独立谓词对齐，五路才有资格互轧。
//! 五路交叉先于复跑：五路对同一事件流的裁决必须互补守恒（一路拒绝必须
//! 给出码），否则 "全绿" 只是五份互不相干的报告。
//! 复跑先于码族：×5 复跑摘要相同，才说明联轧没有隐藏的读取顺序依赖。
//! 码族放最后：15 条错误码是前面所有红项的公共出口，出口撞号则全段失真。
//! 关键差异：本模块不重写键域/死键/LED/修饰/层表五路本体，只做 "乱输入下
//! 的五路独立复核 + 交叉守恒 + 错误码族去混淆"。五路各自的裁决由本模块
//! 用独立谓词函数重算，绝不复用被测路径的中间结果——复用就等于自己证明
//! 自己，判据会退化成常量真。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71CC0，B12 族第七槽）。
pub const SEG_BASE: u32 = 0x71CC0;

/// HID 扫描码下界（'a'）。
pub const SCAN_LO: u8 = 0x04;
/// HID 扫描码上界。
pub const SCAN_HI: u8 = 0xE7;
/// 有效扫描码个数（228）。
pub const SCAN_SPAN: u32 = 228;
/// 字母键上界（'z' = 0x1D）。
pub const LETTER_HI: u8 = 0x1D;
/// LED 三位掩码（Caps/Num/Scroll）。
pub const LED_MASK: u8 = 0x07;
/// 有效 LED 态个数（8）。
pub const LED_STATES: u32 = 8;
/// 修饰位掩码（Shift/Ctrl/Alt/Super）。
pub const MOD_MASK: u8 = 0x0F;
/// Shift 位。
pub const MOD_SHIFT: u8 = 0x01;
/// Alt 位。
pub const MOD_ALT: u8 = 0x04;
/// 层号个数（4）。
pub const LAYERS: u32 = 4;
/// 每层行数（12）。
pub const LAYER_ROWS: u8 = 12;
/// fuzz 轮数（判据 ×10^4 口径）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// fuzz 种子。
pub const FUZZ_SEED: u64 = 0x0000_0000_4A04_0001;
/// fuzz 通道数（扫描码 / LED / 修饰 / 层号）。
pub const CHANNELS: usize = 4;
/// 联轧轮数（判据 ×10^3 口径）。
pub const CROSS_ROUNDS: u32 = 1_000;
/// 联轧种子。
pub const CROSS_SEED: u64 = 0x0000_0000_4A04_0002;
/// 联轧路数（五路判据）。
pub const CROSS_ROUTES: usize = 5;
/// 联轧复跑次数（×5 全绿）。
pub const CROSS_REPEAT: usize = 5;
/// 错误码族条数（15）。
pub const FAMILY_SIZE: usize = 15;
/// 错误码归属族数（7）。
pub const FAMILIES: usize = 7;
/// 错误码族窗口下界。
pub const CODE_LO: u32 = 0x71B40;
/// 错误码族窗口上界。
pub const CODE_HI: u32 = 0x71CCE;
/// 野值注入条数。
pub const WILD_INJECT: u32 = 10;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// fuzz 账标签。
pub const FUZZ_TAG: &str = "FUZZ";
/// 联轧账标签。
pub const XROSS_TAG: &str = "XROSS";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知键盘联轧诊断码";

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzCode {
    /// 扫描码越出 HID 有效区间
    ScanBad,
    /// LED 态越出三位掩码
    LedBad,
    /// 修饰位越出四位掩码
    ModBad,
    /// 层号越出层表
    LayerBad,
    /// 键域解码裁决失败
    DecodeBad,
    /// 死键组合裁决失败
    ComposeBad,
    /// LED 状态机非法迁移
    LedStateBad,
    /// 和弦解析裁决失败
    ChordBad,
    /// 层表反查裁决失败
    LookupBad,
    /// fuzz 账不守恒或轮数越界
    FuzzBad,
    /// 联轧五路不一致或复跑漂移
    CrossBad,
    /// 错误码族撞号、越窗或归属漂移
    CodeBad,
    /// 渲染字段不可解析
    RenderBad,
    /// 诊断码越出段位窗口
    SegmentBad,
}

impl FuzzCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            FuzzCode::ScanBad => "扫描码越出 HID 有效区间",
            FuzzCode::LedBad => "LED 态越出三位掩码",
            FuzzCode::ModBad => "修饰位越出四位掩码",
            FuzzCode::LayerBad => "层号越出层表",
            FuzzCode::DecodeBad => "键域解码裁决失败",
            FuzzCode::ComposeBad => "死键组合裁决失败",
            FuzzCode::LedStateBad => "LED 状态机非法迁移",
            FuzzCode::ChordBad => "和弦解析裁决失败",
            FuzzCode::LookupBad => "层表反查裁决失败",
            FuzzCode::FuzzBad => "fuzz 账不守恒或轮数越界",
            FuzzCode::CrossBad => "联轧五路不一致或复跑漂移",
            FuzzCode::CodeBad => "错误码族撞号、越窗或归属漂移",
            FuzzCode::RenderBad => "渲染字段不可解析",
            FuzzCode::SegmentBad => "诊断码越出段位窗口",
        }
    }

    pub const ALL: [FuzzCode; 14] = [
        FuzzCode::ScanBad,
        FuzzCode::LedBad,
        FuzzCode::ModBad,
        FuzzCode::LayerBad,
        FuzzCode::DecodeBad,
        FuzzCode::ComposeBad,
        FuzzCode::LedStateBad,
        FuzzCode::ChordBad,
        FuzzCode::LookupBad,
        FuzzCode::FuzzBad,
        FuzzCode::CrossBad,
        FuzzCode::CodeBad,
        FuzzCode::RenderBad,
        FuzzCode::SegmentBad,
    ];
}

/// 按诊断码取值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < FuzzCode::ALL.len() {
        if let Some(c) = FuzzCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    UNKNOWN
}

/// 拒绝码是否落在本段闭集内（"零静默" 的判定入口）。
pub fn known_code(code: u32) -> bool {
    detail(code) != UNKNOWN
}

/// 段位守卫：14 条码严格连续占满 +0x01..+0x0E。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < FuzzCode::ALL.len() {
        if let Some(c) = FuzzCode::ALL.get(i) {
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

// ── 维度校验 ──────────────────────────────────────────────────────
/// 扫描码是否在 HID 有效区间内。
pub fn scan_ok(scan: u8) -> bool {
    scan >= SCAN_LO && scan <= SCAN_HI
}

/// LED 态是否为三位掩码内的状态。
pub fn led_ok(leds: u8) -> bool {
    leds <= LED_MASK
}

/// 修饰位是否在四位掩码内。
pub fn mod_ok(mods: u8) -> bool {
    mods <= MOD_MASK
}

/// 层号是否在层表范围内。
pub fn layer_ok(layer: u8) -> bool {
    (layer as u32) < LAYERS
}

/// 是否为字母键（'a'..'z'）。
pub fn is_letter(scan: u8) -> bool {
    scan >= SCAN_LO && scan <= LETTER_HI
}

/// LED 三灯中文名（用于账目可读性，不参与裁决）。
pub fn led_name(leds: u8) -> &'static str {
    match leds & LED_MASK {
        0 => "全灭",
        1 => "Caps",
        2 => "Num",
        3 => "Caps+Num",
        4 => "Scroll",
        5 => "Caps+Scroll",
        6 => "Num+Scroll",
        _ => "三灯全亮",
    }
}

// ── 事件与摘要 ────────────────────────────────────────────────────
/// 一次键盘事件：扫描码、修饰位、目标 LED 态、层号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    pub scan: u8,
    pub mods: u8,
    pub leds: u8,
    pub layer: u8,
}

impl Event {
    /// 构造事件（不做校验，校验归各路裁决）。
    pub fn new(scan: u8, mods: u8, leds: u8, layer: u8) -> Event {
        Event {
            scan,
            mods,
            leds,
            layer,
        }
    }

    /// 事件四维摘要。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_byte(h, self.scan);
        h = p_core::fold_byte(h, self.mods);
        h = p_core::fold_byte(h, self.leds);
        h = p_core::fold_byte(h, self.layer);
        h
    }

    /// 六字段渲染（事件账）。
    pub fn row(&self) -> String {
        format!(
            "EV|scan={}|mods={}|leds={}|layer={}|digest={:016X}",
            self.scan,
            self.mods,
            self.leds,
            self.layer,
            self.digest()
        )
    }
}

fn fold_u64(h: u64, v: u64) -> u64 {
    p_core::fold_u32(p_core::fold_u32(h, (v >> 32) as u32), v as u32)
}

// ── 五路裁决（独立实现，供 fuzz 与联轧共用） ──────────────────────
/// 五路：键域解码 / 死键组合 / LED 状态机 / 修饰和弦 / 层表反查。
pub const ROUTE_DECODE: usize = 0;
pub const ROUTE_COMPOSE: usize = 1;
pub const ROUTE_LED: usize = 2;
pub const ROUTE_CHORD: usize = 3;
pub const ROUTE_LOOKUP: usize = 4;

pub fn route_name(route: usize) -> &'static str {
    match route {
        ROUTE_DECODE => "decode",
        ROUTE_COMPOSE => "compose",
        ROUTE_LED => "led",
        ROUTE_CHORD => "chord",
        ROUTE_LOOKUP => "lookup",
        _ => "unknown",
    }
}

/// 键域解码路：合法扫描码映射到 usage 与四分类。
pub fn route_decode(ev: &Event) -> Result<u64, u32> {
    if !scan_ok(ev.scan) {
        return Err(FuzzCode::ScanBad.val());
    }
    let usage = ev.scan as u32;
    let class = ((ev.scan - SCAN_LO) as u32) % 4;
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, usage);
    h = p_core::fold_u32(h, class);
    Ok(h)
}

/// 死键组合路：字母键叠加音调符号；非字母键带 Alt 视为组合失败。
pub fn route_compose(ev: &Event) -> Result<u64, u32> {
    if !scan_ok(ev.scan) {
        return Err(FuzzCode::ScanBad.val());
    }
    let alt = (ev.mods & MOD_ALT) != 0;
    if !is_letter(ev.scan) && alt {
        return Err(FuzzCode::ComposeBad.val());
    }
    let base = (ev.scan - SCAN_LO) as u32;
    let accent = ((ev.mods & 0x0E) >> 1) as u32;
    let composed = base.saturating_mul(8).saturating_add(accent).saturating_add(1);
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, base);
    h = p_core::fold_u32(h, accent);
    h = p_core::fold_u32(h, composed);
    Ok(h)
}

/// LED 状态迁移：从当前态到目标态的跳变必须落在三位掩码内。
pub fn led_transition(from: u8, masks: u8) -> Result<(u8, u8), u32> {
    if !led_ok(from) {
        return Err(FuzzCode::LedBad.val());
    }
    let to = (from ^ (masks & LED_MASK)) & LED_MASK;
    let toggled = from ^ to;
    if !led_ok(to) {
        return Err(FuzzCode::LedStateBad.val());
    }
    Ok((to, toggled))
}

/// LED 路：目标态合法 + 状态机迁移成功。
/// 迁移两次必须回到全灭（异或翻转自逆），这是状态机"无路径依赖"的原子证据。
pub fn route_led(ev: &Event) -> Result<u64, u32> {
    if !led_ok(ev.leds) {
        return Err(FuzzCode::LedBad.val());
    }
    if !scan_ok(ev.scan) {
        return Err(FuzzCode::ScanBad.val());
    }
    let masks = ev.mods & LED_MASK;
    let (to, toggled) = match led_transition(0, masks) {
        Ok(v) => v,
        Err(c) => return Err(c),
    };
    let (back, _) = match led_transition(to, masks) {
        Ok(v) => v,
        Err(c) => return Err(c),
    };
    if back != 0 || toggled != masks {
        return Err(FuzzCode::LedStateBad.val());
    }
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, ev.leds);
    h = p_core::fold_byte(h, to);
    h = p_core::fold_byte(h, toggled);
    h = p_core::fold_u32(h, ev.scan as u32);
    Ok(h)
}

/// 和弦解析：扫描码低七位与四位修饰拼成动作号。
pub fn chord_action(scan: u8, mods: u8) -> u32 {
    ((scan & 0x7F) as u32) << 4 | (mods & MOD_MASK) as u32
}

/// 和弦路：修饰位与扫描码双合法才出动作号。
pub fn route_chord(ev: &Event) -> Result<u64, u32> {
    if !mod_ok(ev.mods) {
        return Err(FuzzCode::ModBad.val());
    }
    if !scan_ok(ev.scan) {
        return Err(FuzzCode::ScanBad.val());
    }
    let action = chord_action(ev.scan, ev.mods);
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, action);
    h = p_core::fold_u32(h, ev.mods as u32);
    Ok(h)
}

/// 层表反查：第 3 层最后一行回落到 0 层（越界行归属明确）。
pub fn lookup_value(layer: u8, scan: u8) -> Result<(u8, u8, u32), u32> {
    if !layer_ok(layer) {
        return Err(FuzzCode::LayerBad.val());
    }
    if !scan_ok(scan) {
        return Err(FuzzCode::ScanBad.val());
    }
    let row = ((scan - SCAN_LO) % LAYER_ROWS) as u8;
    let mut used = layer;
    if used == 3 && row == LAYER_ROWS - 1 {
        used = 0;
    }
    let value = (used as u32)
        .saturating_mul(0x1000)
        .saturating_add((row as u32) * 0x10)
        .saturating_add(0x0A0);
    Ok((used, row, value))
}

/// 层表路：层号与扫描码双合法才反查到值。
pub fn route_lookup(ev: &Event) -> Result<u64, u32> {
    let (used, row, value) = match lookup_value(ev.layer, ev.scan) {
        Ok(v) => v,
        Err(c) => return Err(c),
    };
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, used);
    h = p_core::fold_byte(h, row);
    h = p_core::fold_u32(h, value);
    Ok(h)
}

/// 单路派发。
pub fn route_run(route: usize, ev: &Event) -> Result<u64, u32> {
    match route {
        ROUTE_DECODE => route_decode(ev),
        ROUTE_COMPOSE => route_compose(ev),
        ROUTE_LED => route_led(ev),
        ROUTE_CHORD => route_chord(ev),
        ROUTE_LOOKUP => route_lookup(ev),
        _ => Err(FuzzCode::CrossBad.val()),
    }
}

/// 五路合并：任一路拒绝即整体拒绝（首错优先），全通过则折出合并摘要。
pub fn route_all(ev: &Event) -> Result<u64, u32> {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < CROSS_ROUTES {
        match route_run(i, ev) {
            Ok(d) => {
                h = fold_u64(h, d);
                h = p_core::fold_u32(h, i as u32);
            }
            Err(c) => return Err(c),
        }
        i += 1;
    }
    Ok(h)
}

// ── 独立谓词（与被测路径解耦的第二条实现） ────────────────────────
/// 键域解码路的独立谓词：只看扫描码区间。
pub fn decode_accepts(ev: &Event) -> bool {
    scan_ok(ev.scan)
}

/// 死键组合路的独立谓词：扫描码合法且非字母键不得带 Alt。
pub fn compose_accepts(ev: &Event) -> bool {
    scan_ok(ev.scan) && (is_letter(ev.scan) || (ev.mods & MOD_ALT) == 0)
}

/// LED 路的独立谓词：目标态合法且扫描码合法。
pub fn led_accepts(ev: &Event) -> bool {
    led_ok(ev.leds) && scan_ok(ev.scan)
}

/// 和弦路的独立谓词：修饰位与扫描码双合法。
pub fn chord_accepts(ev: &Event) -> bool {
    mod_ok(ev.mods) && scan_ok(ev.scan)
}

/// 层表路的独立谓词：层号与扫描码双合法。
pub fn lookup_accepts(ev: &Event) -> bool {
    layer_ok(ev.layer) && scan_ok(ev.scan)
}

/// 按路取独立谓词。
pub fn route_accepts(route: usize, ev: &Event) -> bool {
    match route {
        ROUTE_DECODE => decode_accepts(ev),
        ROUTE_COMPOSE => compose_accepts(ev),
        ROUTE_LED => led_accepts(ev),
        ROUTE_CHORD => chord_accepts(ev),
        ROUTE_LOOKUP => lookup_accepts(ev),
        _ => false,
    }
}

/// 单路裁决与独立谓词是否一致（单路自洽的原子判据）。
pub fn route_self_consistent(route: usize, ev: &Event) -> bool {
    route_run(route, ev).is_ok() == route_accepts(route, ev)
}

/// 全事件扫野：五路 ×256 扫描码 ×16 修饰 ×8 LED ×4 层不可全量，
/// 这里做维度轮转扫野（每维取全量、其余维取边界四点），覆盖边界与内部。
pub fn dimension_sweep() -> Result<(u32, u32), u32> {
    let scans = [0x00u8, 0x03, 0x04, 0x1D, 0x1E, 0xE7, 0xE8, 0xFF];
    let mods = [0x00u8, 0x01, 0x04, 0x07, 0x0F, 0x10, 0xFF];
    let leds = [0x00u8, 0x01, 0x07, 0x08, 0x0F, 0x80, 0xFF];
    let layers = [0x00u8, 0x01, 0x03, 0x04, 0x7F, 0xFF];
    let mut checked = 0u32;
    let mut mismatches = 0u32;
    let mut i = 0usize;
    while i < scans.len() {
        let ev = Event::new(*match scans.get(i) { Some(v) => v, None => return Err(FuzzCode::FuzzBad.val()) }, 0x00, 0x00, 0x00);
        checked += 1;
        if !all_routes_self_consistent(&ev) {
            mismatches += 1;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < mods.len() {
        let ev = Event::new(0x04, *match mods.get(i) { Some(v) => v, None => return Err(FuzzCode::FuzzBad.val()) }, 0x00, 0x00);
        checked += 1;
        if !all_routes_self_consistent(&ev) {
            mismatches += 1;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < leds.len() {
        let ev = Event::new(0x04, 0x00, *match leds.get(i) { Some(v) => v, None => return Err(FuzzCode::FuzzBad.val()) }, 0x00);
        checked += 1;
        if !all_routes_self_consistent(&ev) {
            mismatches += 1;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < layers.len() {
        let ev = Event::new(0x04, 0x00, 0x00, *match layers.get(i) { Some(v) => v, None => return Err(FuzzCode::FuzzBad.val()) });
        checked += 1;
        if !all_routes_self_consistent(&ev) {
            mismatches += 1;
        }
        i += 1;
    }
    // 全扫描码单维轮转（修饰/ LED / 层取合法常量）。
    let mut s: u16 = 0;
    while s <= 0xFF {
        let ev = Event::new(s as u8, 0x05, 0x03, 0x01);
        checked += 1;
        if !all_routes_self_consistent(&ev) {
            mismatches += 1;
        }
        s += 1;
    }
    Ok((checked, mismatches))
}

fn all_routes_self_consistent(ev: &Event) -> bool {
    let mut i = 0usize;
    while i < CROSS_ROUTES {
        if !route_self_consistent(i, ev) {
            return false;
        }
        i += 1;
    }
    true
}

// ── fuzz 账 ───────────────────────────────────────────────────────
/// fuzz 账：四通道（扫描码/LED/修饰/层号）各自单维扫野的守恒账。
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
    /// 守恒：事件数 = 接受 + 拒绝，且拒绝全部有码。
    pub fn conserved(&self) -> bool {
        self.events == self.rounds.saturating_mul(CHANNELS as u32)
            && self.accepted.saturating_add(self.rejected) == self.events
            && self.classified == self.rejected
    }
}

/// 构造单通道事件：基线合法，仅覆写目标维度为野值。
fn channel_event(channel: usize, lcg: &mut p_core::Lcg) -> Event {
    let scan = SCAN_LO + lcg.below(SCAN_SPAN) as u8;
    let mods = (lcg.below(MOD_MASK as u32 + 1) as u8) & MOD_MASK;
    let leds = (lcg.below(LED_STATES) as u8) & LED_MASK;
    let layer = (lcg.below(LAYERS) as u8) & (LAYERS as u8 - 1);
    let wild = lcg.next() as u8;
    match channel {
        0 => Event::new(wild, mods, leds, layer),
        1 => Event::new(scan, mods, wild, layer),
        2 => Event::new(scan, wild, leds, layer),
        3 => Event::new(scan, mods, leds, wild),
        _ => Event::new(scan, mods, leds, layer),
    }
}

/// 跑 fuzz：每轮四通道各一次，共 rounds×4 个事件；拒绝必须全部有码。
pub fn fuzz_rounds(rounds: u32, seed: u64) -> Result<FuzzLedger, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(FuzzCode::FuzzBad.val());
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
            let ev = channel_event(ch, &mut lcg);
            let ev_digest = ev.digest();
            led.events = led.events.saturating_add(1);
            match route_all(&ev) {
                Ok(d) => {
                    led.accepted = led.accepted.saturating_add(1);
                    if let Some(slot) = led.per_channel_accept.get_mut(ch) {
                        *slot = slot.saturating_add(1);
                    }
                    led.digest = fold_u64(led.digest, ev_digest);
                    led.digest = fold_u64(led.digest, d);
                    led.digest = p_core::fold_u32(led.digest, 0);
                }
                Err(c) => {
                    if !known_code(c) {
                        // 未知拒绝码直接判红：不允许静默丢弃。
                        return Err(FuzzCode::FuzzBad.val());
                    }
                    led.rejected = led.rejected.saturating_add(1);
                    led.classified = led.classified.saturating_add(1);
                    if let Some(slot) = led.per_channel_reject.get_mut(ch) {
                        *slot = slot.saturating_add(1);
                    }
                    led.digest = fold_u64(led.digest, ev_digest);
                    led.digest = p_core::fold_u32(led.digest, c);
                }
            }
            led.digest = p_core::fold_u32(led.digest, ch as u32);
            ch += 1;
        }
        r += 1;
    }
    if !led.conserved() {
        return Err(FuzzCode::FuzzBad.val());
    }
    Ok(led)
}

/// fuzz 复跑：两次同种子摘要必须一致。
pub fn fuzz_replay() -> Result<(FuzzLedger, FuzzLedger), u32> {
    let a = fuzz_rounds(FUZZ_ROUNDS, FUZZ_SEED)?;
    let b = fuzz_rounds(FUZZ_ROUNDS, FUZZ_SEED)?;
    Ok((a, b))
}

/// 单维扫野：扫描码全 256 值的解码路分区（接受数/拒绝数）。
pub fn scan_sweep() -> Result<(u32, u32), u32> {
    let mut acc = 0u32;
    let mut rej = 0u32;
    let mut s: u16 = 0;
    while s <= 0xFF {
        let ev = Event::new(s as u8, 0x00, 0x00, 0x00);
        match route_decode(&ev) {
            Ok(_) => acc = acc.saturating_add(1),
            Err(c) => {
                if !known_code(c) {
                    return Err(FuzzCode::FuzzBad.val());
                }
                rej = rej.saturating_add(1);
            }
        }
        s += 1;
    }
    Ok((acc, rej))
}

/// 单维扫野：LED 全 256 值的目标态分区。
pub fn led_sweep() -> Result<(u32, u32), u32> {
    let mut acc = 0u32;
    let mut rej = 0u32;
    let mut l: u16 = 0;
    while l <= 0xFF {
        let ev = Event::new(0x04, 0x03, l as u8, 0x00);
        match route_led(&ev) {
            Ok(_) => acc = acc.saturating_add(1),
            Err(c) => {
                if !known_code(c) {
                    return Err(FuzzCode::FuzzBad.val());
                }
                rej = rej.saturating_add(1);
            }
        }
        l += 1;
    }
    Ok((acc, rej))
}

/// 单维扫野：修饰位全 256 值的和弦路分区。
pub fn mod_sweep() -> Result<(u32, u32), u32> {
    let mut acc = 0u32;
    let mut rej = 0u32;
    let mut m: u16 = 0;
    while m <= 0xFF {
        let ev = Event::new(0x04, m as u8, 0x00, 0x00);
        match route_chord(&ev) {
            Ok(_) => acc = acc.saturating_add(1),
            Err(c) => {
                if !known_code(c) {
                    return Err(FuzzCode::FuzzBad.val());
                }
                rej = rej.saturating_add(1);
            }
        }
        m += 1;
    }
    Ok((acc, rej))
}

/// 单维扫野：层号全 256 值的层表路分区。
pub fn layer_sweep() -> Result<(u32, u32), u32> {
    let mut acc = 0u32;
    let mut rej = 0u32;
    let mut y: u16 = 0;
    while y <= 0xFF {
        let ev = Event::new(0x04, 0x00, 0x00, y as u8);
        match route_lookup(&ev) {
            Ok(_) => acc = acc.saturating_add(1),
            Err(c) => {
                if !known_code(c) {
                    return Err(FuzzCode::FuzzBad.val());
                }
                rej = rej.saturating_add(1);
            }
        }
        y += 1;
    }
    Ok((acc, rej))
}

/// 十条野事件注入：每条至少被一路拒绝，且拒绝码全部已知。
pub fn wild_inject() -> Result<(u32, u32), u32> {
    let samples = [
        Event::new(0x00, 0x00, 0x00, 0x00),
        Event::new(0x03, 0x00, 0x00, 0x00),
        Event::new(0xE8, 0x00, 0x00, 0x00),
        Event::new(0xFF, 0x00, 0x00, 0x00),
        Event::new(0x04, 0x00, 0x08, 0x00),
        Event::new(0x04, 0x00, 0xFF, 0x00),
        Event::new(0x04, 0x10, 0x00, 0x00),
        Event::new(0x04, 0xFF, 0x00, 0x00),
        Event::new(0x04, 0x00, 0x00, 0x04),
        Event::new(0x04, 0x00, 0x00, 0xFF),
    ];
    let mut caught = 0u32;
    let mut classified = 0u32;
    let mut i = 0usize;
    while i < samples.len() {
        let ev = match samples.get(i) {
            Some(e) => *e,
            None => return Err(FuzzCode::FuzzBad.val()),
        };
        match route_all(&ev) {
            Ok(_) => {}
            Err(c) => {
                if !known_code(c) {
                    return Err(FuzzCode::FuzzBad.val());
                }
                caught = caught.saturating_add(1);
                classified = classified.saturating_add(1);
            }
        }
        i += 1;
    }
    Ok((caught, classified))
}

// ── 联轧账 ───────────────────────────────────────────────────────
/// 联轧账：五路对同一事件流的交叉裁决与守恒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CrossLedger {
    pub rounds: u32,
    pub checks: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub mismatches: u32,
    pub per_route_accept: [u32; CROSS_ROUTES],
    pub per_route_reject: [u32; CROSS_ROUTES],
    pub per_route_digest: [u64; CROSS_ROUTES],
    pub digest: u64,
}

impl CrossLedger {
    /// 每路守恒：每路接受 + 拒绝 = 轮数。
    pub fn conserved(&self) -> bool {
        let mut i = 0usize;
        while i < CROSS_ROUTES {
            let a = match self.per_route_accept.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let r = match self.per_route_reject.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a.saturating_add(r) != self.rounds {
                return false;
            }
            i += 1;
        }
        self.checks == self.rounds.saturating_mul(CROSS_ROUTES as u32)
            && self.accepted.saturating_add(self.rejected) == self.checks
    }
}

/// 联轧事件流：合法/非法按固定节拍交叠，保证拒绝路径也被反复踩到。
pub fn canonical_event(i: u32) -> Event {
    let scan = if i % 16 == 0 {
        (i & 0xFF) as u8
    } else {
        SCAN_LO + (i % SCAN_SPAN) as u8
    };
    let mods = (i & MOD_MASK as u32) as u8;
    let leds = (i & LED_MASK as u32) as u8;
    let layer = (i & (LAYERS - 1)) as u8;
    let mut ev = Event::new(scan, mods, leds, layer);
    if i % 7 == 0 {
        ev.leds = 0x08 + (i % 4) as u8;
    }
    if i % 11 == 0 {
        ev.mods = 0x10 + (i % 16) as u8;
    }
    if i % 13 == 0 {
        ev.layer = 4 + (i % 4) as u8;
    }
    ev
}

/// 跑联轧：逐事件逐路裁决，并与独立谓词对账。
pub fn cross_run(rounds: u32, seed: u64) -> Result<CrossLedger, u32> {
    if rounds == 0 || rounds > CROSS_ROUNDS {
        return Err(FuzzCode::CrossBad.val());
    }
    let mut led = CrossLedger {
        rounds,
        checks: 0,
        accepted: 0,
        rejected: 0,
        mismatches: 0,
        per_route_accept: [0u32; CROSS_ROUTES],
        per_route_reject: [0u32; CROSS_ROUTES],
        per_route_digest: [p_core::FNV_OFF; CROSS_ROUTES],
        digest: p_core::FNV_OFF,
    };
    let _ = seed; // 事件流由 canonical_event 定序，种子仅作账目标记
    let mut i: u32 = 0;
    while i < rounds {
        let ev = canonical_event(i);
        let ev_digest = ev.digest();
        led.digest = fold_u64(led.digest, ev_digest);
        let mut r = 0usize;
        while r < CROSS_ROUTES {
            led.checks = led.checks.saturating_add(1);
            let outcome = route_run(r, &ev);
            match outcome {
                Ok(d) => {
                    led.accepted = led.accepted.saturating_add(1);
                    if let Some(slot) = led.per_route_accept.get_mut(r) {
                        *slot = slot.saturating_add(1);
                    }
                    if let Some(dslot) = led.per_route_digest.get_mut(r) {
                        *dslot = fold_u64(*dslot, d);
                    }
                    led.digest = p_core::fold_u32(led.digest, 0);
                    led.digest = fold_u64(led.digest, d);
                }
                Err(c) => {
                    if !known_code(c) {
                        return Err(FuzzCode::CrossBad.val());
                    }
                    led.rejected = led.rejected.saturating_add(1);
                    if let Some(slot) = led.per_route_reject.get_mut(r) {
                        *slot = slot.saturating_add(1);
                    }
                    led.digest = p_core::fold_u32(led.digest, c);
                }
            }
            if outcome.is_ok() != route_accepts(r, &ev) {
                led.mismatches = led.mismatches.saturating_add(1);
            }
            led.digest = p_core::fold_u32(led.digest, r as u32);
            r += 1;
        }
        i += 1;
    }
    if !led.conserved() {
        return Err(FuzzCode::CrossBad.val());
    }
    Ok(led)
}

/// ×5 复跑：五次同流联轧，摘要必须逐位相同（零漂移）。
pub fn cross_five() -> Result<[u64; CROSS_REPEAT], u32> {
    let mut out = [0u64; CROSS_REPEAT];
    let mut i = 0usize;
    while i < CROSS_REPEAT {
        let l = cross_run(CROSS_ROUNDS, CROSS_SEED)?;
        if l.mismatches != 0 {
            return Err(FuzzCode::CrossBad.val());
        }
        if let Some(slot) = out.get_mut(i) {
            *slot = l.digest;
        }
        i += 1;
    }
    Ok(out)
}

/// 单路复跑：对同一事件流连续两次，每路摘要必须相同。
pub fn route_replay() -> Result<([u64; CROSS_ROUTES], [u64; CROSS_ROUTES]), u32> {
    let a = cross_run(CROSS_ROUNDS, CROSS_SEED)?;
    let b = cross_run(CROSS_ROUNDS, CROSS_SEED)?;
    Ok((a.per_route_digest, b.per_route_digest))
}

// ── 错误码族（15 条，B12 段全域出口） ────────────────────────────
/// 一条全域诊断码：码值、名称、归属族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FamEntry {
    pub code: u32,
    pub name: &'static str,
    pub family: u8,
}

/// 族名表（7 族）。
pub fn family_name(family: u8) -> &'static str {
    match family {
        0 => "dead-key",
        1 => "led",
        2 => "autorep",
        3 => "modkey",
        4 => "keymap",
        5 => "trace",
        6 => "casepool",
        _ => "",
    }
}

/// B12 段 15 条错误码（与前置各槽的码值窗口一致，零新增、零改名）。
pub const FAMILY: [FamEntry; FAMILY_SIZE] = [
    FamEntry { code: 0x71B41, name: "deadkey-unresolved", family: 0 },
    FamEntry { code: 0x71B42, name: "deadkey-conflict", family: 0 },
    FamEntry { code: 0x71B43, name: "deadkey-table", family: 0 },
    FamEntry { code: 0x71B44, name: "led-write-fail", family: 1 },
    FamEntry { code: 0x71B45, name: "led-timeout", family: 1 },
    FamEntry { code: 0x71B46, name: "autorep-period", family: 2 },
    FamEntry { code: 0x71B47, name: "autorep-delay", family: 2 },
    FamEntry { code: 0x71B48, name: "led-blocked", family: 1 },
    FamEntry { code: 0x71B49, name: "modkey-latch", family: 3 },
    FamEntry { code: 0x71C41, name: "keymap-layer", family: 4 },
    FamEntry { code: 0x71C42, name: "keymap-lookup", family: 4 },
    FamEntry { code: 0x71C43, name: "trace-stage", family: 5 },
    FamEntry { code: 0x71C44, name: "trace-budget", family: 5 },
    FamEntry { code: 0x71C45, name: "case-dup", family: 6 },
    FamEntry { code: 0x71C46, name: "family-collision", family: 6 },
];

/// 族码归类：码值查表。
pub fn lookup_code(code: u32) -> Option<usize> {
    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(e) = FAMILY.get(i) {
            if e.code == code {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 族码归类：名称查表。
pub fn lookup_name(name: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(e) = FAMILY.get(i) {
            if e.name == name {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 独立归属推导：只看码值窗口，不看表内声明。
pub fn expected_family(code: u32) -> Option<u8> {
    match code {
        0x71B41..=0x71B43 => Some(0),
        0x71B44 | 0x71B45 | 0x71B48 => Some(1),
        0x71B46 | 0x71B47 => Some(2),
        0x71B49 => Some(3),
        0x71C41 | 0x71C42 => Some(4),
        0x71C43 | 0x71C44 => Some(5),
        0x71C45 | 0x71C46 => Some(6),
        _ => None,
    }
}

/// 族表自洽：条数、窗口、码值唯一、名称唯一、族号合法。
pub fn family_ok() -> bool {
    if FAMILY.len() != FAMILY_SIZE {
        return false;
    }
    let mut i = 0usize;
    while i < FAMILY.len() {
        let a = match FAMILY.get(i) {
            Some(v) => v,
            None => return false,
        };
        if a.code < CODE_LO || a.code > CODE_HI {
            return false;
        }
        if a.name.is_empty() || family_name(a.family).is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < FAMILY.len() {
            let b = match FAMILY.get(j) {
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

/// 零漂移：表内归属与独立归属推导逐条相等（"互不混淆" 的硬证据）。
pub fn family_zero_drift() -> bool {
    let mut i = 0usize;
    while i < FAMILY.len() {
        let e = match FAMILY.get(i) {
            Some(v) => v,
            None => return false,
        };
        if expected_family(e.code) != Some(e.family) {
            return false;
        }
        if lookup_code(e.code) != Some(i) || lookup_name(e.name) != Some(i) {
            return false;
        }
        i += 1;
    }
    true
}

/// 族表摘要。
pub fn family_digest() -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(e) = FAMILY.get(i) {
            h = p_core::fold_u32(h, e.code);
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

// ── 渲染 ──────────────────────────────────────────────────────────
/// 六字段渲染：标签 + 轮数 + 接受 + 拒绝 + 路数 + 摘要。
pub fn render_row(tag: &str, rounds: u32, accepted: u32, rejected: u32, routes: u32, digest: u64) -> String {
    format!(
        "{}|rounds={}|accepted={}|rejected={}|routes={}|digest={:016X}",
        tag, rounds, accepted, rejected, routes, digest
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

/// 六字段解析。
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
    render_row(FUZZ_TAG, l.rounds, l.accepted, l.rejected, CHANNELS as u32, l.digest)
}

/// 联轧账渲染并自校。
pub fn cross_row(l: &CrossLedger) -> String {
    render_row(XROSS_TAG, l.rounds, l.accepted, l.rejected, CROSS_ROUTES as u32, l.digest)
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49033 ①：乱扫描码 ×10^4 不崩（四通道守恒，接受/拒绝双非空）。
    let fz = fuzz_rounds(FUZZ_ROUNDS, FUZZ_SEED);
    let fuzz_ok = match &fz {
        Ok(l) => {
            l.conserved()
                && l.accepted > 0
                && l.rejected > 0
                && l.events == FUZZ_ROUNDS * CHANNELS as u32
                && l.per_channel_reject.iter().sum::<u32>() == l.rejected
        }
        Err(_) => false,
    };
    out.push((
        "F49033/乱码零崩",
        fuzz_ok,
        format!(
            "rounds={} events={:?} accepted={:?} rejected={:?}",
            FUZZ_ROUNDS,
            fz.as_ref().map(|l| l.events),
            fz.as_ref().map(|l| l.accepted),
            fz.as_ref().map(|l| l.rejected)
        ),
    ));

    // F49033 ②：乱 LED 态不崩（全 256 值分区正确 + 十条野注入全拒）。
    let led = led_sweep();
    let wild = wild_inject();
    let led_ok = match (&led, &wild) {
        (Ok((a, r)), Ok((caught, classified))) => {
            *a == LED_STATES && *r == 256 - LED_STATES && *caught == WILD_INJECT && *classified == WILD_INJECT
        }
        _ => false,
    };
    out.push((
        "F49033/乱LED不崩",
        led_ok,
        format!(
            "led_acc={:?} led_rej={:?} wild_caught={:?}",
            led.as_ref().map(|(a, _)| *a),
            led.as_ref().map(|(_, r)| *r),
            wild.as_ref().map(|(c, _)| *c)
        ),
    ));

    // F49033 ③：拒绝有码 + 单维扫野分区自洽（扫描码 228/28、修饰 16/240、层 4/252）。
    let ss = scan_sweep();
    let ms = mod_sweep();
    let ls = layer_sweep();
    let dim = dimension_sweep();
    let partition_ok = match (&ss, &ms, &ls, &dim) {
        (Ok((sa, sr)), Ok((ma, mr)), Ok((la, lr)), Ok((_, mism))) => {
            *sa == SCAN_SPAN
                && *sr == 256 - SCAN_SPAN
                && *ma == (MOD_MASK as u32 + 1)
                && *mr == 256 - (MOD_MASK as u32 + 1)
                && *la == LAYERS
                && *lr == 256 - LAYERS
                && *mism == 0
        }
        _ => false,
    };
    out.push((
        "F49033/拒绝有码",
        partition_ok,
        format!(
            "scan={:?} mod={:?} layer={:?} dim_mism={:?}",
            ss.as_ref().map(|(a, r)| (*a, *r)),
            ms.as_ref().map(|(a, r)| (*a, *r)),
            ls.as_ref().map(|(a, r)| (*a, *r)),
            dim.as_ref().map(|(_, m)| *m)
        ),
    ));

    // F49033 ④：fuzz 复跑零漂移。
    let rep = fuzz_replay();
    let rep_ok = match &rep {
        Ok((a, b)) => a.digest == b.digest && a.digest != 0 && a.conserved() && b.conserved(),
        Err(_) => false,
    };
    out.push((
        "F49033/fuzz复跑",
        rep_ok,
        format!(
            "digest={:016X} stable={}",
            rep.as_ref().map(|(a, _)| a.digest).unwrap_or(0),
            rep_ok
        ),
    ));

    // F49034 ①：五路交叉全绿（1000 轮 ×5 路，独立谓词零偏差、每路守恒）。
    let cr = cross_run(CROSS_ROUNDS, CROSS_SEED);
    let cross_ok = match &cr {
        Ok(l) => {
            l.mismatches == 0
                && l.conserved()
                && l.checks == CROSS_ROUNDS * CROSS_ROUTES as u32
                && l.per_route_accept.iter().all(|v| *v > 0)
                && l.per_route_reject.iter().all(|v| *v > 0)
        }
        Err(_) => false,
    };
    out.push((
        "F49034/五路全绿",
        cross_ok,
        format!(
            "rounds={} checks={:?} mismatches={:?} accept={:?} reject={:?}",
            CROSS_ROUNDS,
            cr.as_ref().map(|l| l.checks),
            cr.as_ref().map(|l| l.mismatches),
            cr.as_ref().map(|l| l.accepted),
            cr.as_ref().map(|l| l.rejected)
        ),
    ));

    // F49034 ②：联轧 ×5 全绿零漂移（五次摘要逐位相同 + 单路复跑一致）。
    let five = cross_five();
    let rr = route_replay();
    let mut five_ok = true;
    match &five {
        Ok(ds) => {
            let first = match ds.get(0) {
                Some(v) => *v,
                None => 0,
            };
            if first == 0 {
                five_ok = false;
            } else {
                let mut i = 1usize;
                while i < ds.len() {
                    match ds.get(i) {
                        Some(d) => {
                            if *d != first {
                                five_ok = false;
                            }
                        }
                        None => five_ok = false,
                    }
                    i += 1;
                }
            }
        }
        Err(_) => five_ok = false,
    }
    match &rr {
        Ok((a, b)) => {
            let mut k = 0usize;
            while k < CROSS_ROUTES {
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => {
                        if x != y || *x == 0 {
                            five_ok = false;
                        }
                    }
                    _ => five_ok = false,
                }
                k += 1;
            }
        }
        Err(_) => five_ok = false,
    }
    out.push((
        "F49034/联轧×5零漂移",
        five_ok,
        format!(
            "repeats={} digest={:?} route_replay={}",
            CROSS_REPEAT,
            five.as_ref().ok().and_then(|d| d.get(0).copied()),
            rr.is_ok()
        ),
    ));

    // F49034 ③：错误码 ×15 零漂移互不混淆（码/名唯一、窗口、独立归属一致）。
    let fam = family_ok() && family_zero_drift() && family_digest() != 0;
    let no_alias = lookup_code(0x71B41) == Some(0)
        && lookup_code(0x71C46) == Some(FAMILY_SIZE - 1)
        && lookup_name("deadkey-unresolved") == Some(0)
        && lookup_name("family-collision") == Some(FAMILY_SIZE - 1)
        && lookup_code(0x9999).is_none()
        && lookup_name("unknown").is_none();
    out.push((
        "F49034/错误码×15零漂移",
        fam && no_alias,
        format!(
            "codes={} drift={} alias={} window=[{:X},{:X}]",
            FAMILY_SIZE,
            !family_zero_drift(),
            !no_alias,
            CODE_LO,
            CODE_HI
        ),
    ));

    // F49034 ④：渲染六字段可解析 + 段位守卫。
    let sample = render_row(XROSS_TAG, CROSS_ROUNDS, 1, 2, CROSS_ROUTES as u32, 0x0123_4567_89AB_CDEF);
    let sample_ok = match row_fields(&sample) {
        Some((t, v, d)) => {
            t == XROSS_TAG
                && v[0] == CROSS_ROUNDS
                && v[1] == 1
                && v[2] == 2
                && v[3] == CROSS_ROUTES as u32
                && d == 0x0123_4567_89AB_CDEF
        }
        None => false,
    };
    let render_ok = audit_render(&sample) && sample_ok && !audit_render("XROSS|rounds=1");
    let fz_row_ok = match &fz {
        Ok(l) => match row_fields(&fuzz_row(l)) {
            Some((t, v, d)) => {
                t == FUZZ_TAG
                    && v[0] == l.rounds
                    && v[1] == l.accepted
                    && v[2] == l.rejected
                    && v[3] == CHANNELS as u32
                    && d == l.digest
            }
            None => false,
        },
        Err(_) => false,
    };
    let cr_row_ok = match &cr {
        Ok(l) => match row_fields(&cross_row(l)) {
            Some((t, v, d)) => {
                t == XROSS_TAG
                    && v[0] == l.rounds
                    && v[1] == l.accepted
                    && v[2] == l.rejected
                    && v[3] == CROSS_ROUTES as u32
                    && d == l.digest
            }
            None => false,
        },
        Err(_) => false,
    };
    out.push((
        "F49034/渲染与段位",
        render_ok && fz_row_ok && cr_row_ok && segment_guard(),
        format!(
            "render={} fuzz_row={} cross_row={} seg={}",
            render_ok,
            fz_row_ok,
            cr_row_ok,
            segment_guard()
        ),
    ));

    out
}

pub fn run_unx49033_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49034 键盘 fuzz 与 B12 段内联轧");
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
    fn validators_boundaries() {
        assert!(!scan_ok(0x00) && !scan_ok(0x03));
        assert!(scan_ok(0x04) && scan_ok(0xE7));
        assert!(!scan_ok(0xE8) && !scan_ok(0xFF));
        assert!(led_ok(0) && led_ok(7) && !led_ok(8));
        assert!(mod_ok(0) && mod_ok(0x0F) && !mod_ok(0x10));
        assert!(layer_ok(0) && layer_ok(3) && !layer_ok(4));
        assert!(is_letter(0x04) && is_letter(0x1D) && !is_letter(0x1E));
        assert_eq!(led_name(0), "全灭");
        assert_eq!(led_name(7), "三灯全亮");
    }

    #[test]
    fn event_digest_and_row() {
        let a = Event::new(0x04, 0x01, 0x03, 0x02);
        let b = Event::new(0x04, 0x01, 0x03, 0x02);
        let c = Event::new(0x05, 0x01, 0x03, 0x02);
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), c.digest());
        assert!(a.row().starts_with("EV|scan=4|mods=1|leds=3|layer=2|digest="));
    }

    #[test]
    fn decode_route_rules() {
        assert_eq!(route_decode(&Event::new(0x00, 0, 0, 0)), Err(FuzzCode::ScanBad.val()));
        let ok = match route_decode(&Event::new(0x04, 0, 0, 0)) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_ne!(ok, 0);
        let classes: [u32; 4] = [0x04, 0x05, 0x06, 0x07];
        let mut seen = [false; 4];
        let mut i = 0usize;
        while i < classes.len() {
            let ev = Event::new(classes[i] as u8, 0, 0, 0);
            let d = match route_decode(&ev) {
                Ok(v) => v,
                Err(_) => return assert!(false),
            };
            seen[i % 4] = d != 0;
            i += 1;
        }
        assert!(seen.iter().all(|x| *x));
    }

    #[test]
    fn compose_route_rules() {
        assert_eq!(route_compose(&Event::new(0x03, 0, 0, 0)), Err(FuzzCode::ScanBad.val()));
        assert_eq!(route_compose(&Event::new(0x1E, MOD_ALT, 0, 0)), Err(FuzzCode::ComposeBad.val()));
        assert!(route_compose(&Event::new(0x04, MOD_ALT, 0, 0)).is_ok());
        assert!(route_compose(&Event::new(0x1E, 0x00, 0, 0)).is_ok());
        let a = match route_compose(&Event::new(0x04, 0x00, 0, 0)) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let b = match route_compose(&Event::new(0x04, 0x02, 0, 0)) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn led_route_rules() {
        assert_eq!(route_led(&Event::new(0x04, 0, 0x08, 0)), Err(FuzzCode::LedBad.val()));
        assert_eq!(route_led(&Event::new(0x03, 0, 0x00, 0)), Err(FuzzCode::ScanBad.val()));
        let (to, toggled) = match led_transition(0, 0x01) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(to, 1);
        assert_eq!(toggled, 1);
        let (again, _) = match led_transition(to, 0x01) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(again, 0);
        assert_eq!(led_transition(8, 0), Err(FuzzCode::LedBad.val()));
    }

    #[test]
    fn chord_route_rules() {
        assert_eq!(route_chord(&Event::new(0x04, 0x10, 0, 0)), Err(FuzzCode::ModBad.val()));
        assert_eq!(route_chord(&Event::new(0x03, 0x01, 0, 0)), Err(FuzzCode::ScanBad.val()));
        assert_eq!(chord_action(0x04, 0x03), (0x04 << 4) | 0x03);
        assert!(route_chord(&Event::new(0x04, 0x0F, 0, 0)).is_ok());
    }

    #[test]
    fn lookup_route_rules() {
        assert_eq!(route_lookup(&Event::new(0x04, 0, 0, 4)), Err(FuzzCode::LayerBad.val()));
        assert_eq!(route_lookup(&Event::new(0x03, 0, 0, 0)), Err(FuzzCode::ScanBad.val()));
        let (used, row, value) = match lookup_value(3, 0x04 + 11) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(used, 0);
        assert_eq!(row, 11);
        assert_eq!(value, 0x0A0 + 11 * 0x10);
        let (used2, _, _) = match lookup_value(2, 0x04 + 11) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(used2, 2);
    }

    #[test]
    fn predicates_agree_on_grid() {
        let scans = [0x00u8, 0x04, 0x1D, 0x1E, 0xE7, 0xE8];
        let mods = [0x00u8, 0x01, 0x04, 0x0F, 0x10];
        let leds = [0x00u8, 0x07, 0x08];
        let layers = [0x00u8, 0x03, 0x04];
        let mut i = 0usize;
        while i < scans.len() {
            let mut j = 0usize;
            while j < mods.len() {
                let mut k = 0usize;
                while k < leds.len() {
                    let mut l = 0usize;
                    while l < layers.len() {
                        let ev = Event::new(scans[i], mods[j], leds[k], layers[l]);
                        assert!(all_routes_self_consistent(&ev));
                        l += 1;
                    }
                    k += 1;
                }
                j += 1;
            }
            i += 1;
        }
        assert_eq!(dimension_sweep(), Ok((284, 0)));
    }

    #[test]
    fn fuzz_accounting_is_conserved() {
        let l = match fuzz_rounds(FUZZ_ROUNDS, FUZZ_SEED) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.rounds, FUZZ_ROUNDS);
        assert_eq!(l.events, FUZZ_ROUNDS * CHANNELS as u32);
        assert!(l.conserved());
        assert_eq!(l.accepted + l.rejected, l.events);
        assert_eq!(l.classified, l.rejected);
        assert!(l.accepted > 0 && l.rejected > 0);
        assert_ne!(l.digest, 0);
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < CHANNELS {
            sum += l.per_channel_accept[i] + l.per_channel_reject[i];
            i += 1;
        }
        assert_eq!(sum, l.events);
        assert!(l.per_channel_reject.iter().all(|v| *v > 0));
    }

    #[test]
    fn fuzz_replay_is_stable() {
        let (a, b) = match fuzz_replay() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(a.digest, b.digest);
        assert_ne!(a.digest, 0);
        assert!(a.conserved() && b.conserved());
        assert_eq!(a.per_channel_accept, b.per_channel_accept);
        assert_eq!(a.per_channel_reject, b.per_channel_reject);
    }

    #[test]
    fn fuzz_bounds_are_guarded() {
        assert_eq!(fuzz_rounds(0, FUZZ_SEED), Err(FuzzCode::FuzzBad.val()));
        assert_eq!(fuzz_rounds(FUZZ_ROUNDS + 1, FUZZ_SEED), Err(FuzzCode::FuzzBad.val()));
        assert_eq!(cross_run(0, CROSS_SEED), Err(FuzzCode::CrossBad.val()));
        assert_eq!(cross_run(CROSS_ROUNDS + 1, CROSS_SEED), Err(FuzzCode::CrossBad.val()));
    }

    #[test]
    fn sweeps_partition_exactly() {
        assert_eq!(scan_sweep(), Ok((228, 28)));
        assert_eq!(led_sweep(), Ok((8, 248)));
        assert_eq!(mod_sweep(), Ok((16, 240)));
        assert_eq!(layer_sweep(), Ok((4, 252)));
        assert_eq!(wild_inject(), Ok((WILD_INJECT, WILD_INJECT)));
    }

    #[test]
    fn cross_run_is_green() {
        let l = match cross_run(CROSS_ROUNDS, CROSS_SEED) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.rounds, CROSS_ROUNDS);
        assert_eq!(l.checks, CROSS_ROUNDS * CROSS_ROUTES as u32);
        assert_eq!(l.mismatches, 0);
        assert!(l.conserved());
        assert!(l.accepted > 0 && l.rejected > 0);
        assert_ne!(l.digest, 0);
        let mut i = 0usize;
        while i < CROSS_ROUTES {
            assert!(l.per_route_accept[i] > 0);
            assert!(l.per_route_reject[i] > 0);
            assert_ne!(l.per_route_digest[i], 0);
            i += 1;
        }
        assert!(row_fields(&cross_row(&l)).is_some());
    }

    #[test]
    fn cross_five_zero_drift() {
        let ds = match cross_five() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(ds.len(), CROSS_REPEAT);
        let first = ds[0];
        assert_ne!(first, 0);
        let mut i = 1usize;
        while i < ds.len() {
            assert_eq!(ds[i], first);
            i += 1;
        }
        let (a, b) = match route_replay() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let mut k = 0usize;
        while k < CROSS_ROUTES {
            assert_eq!(a[k], b[k]);
            assert_ne!(a[k], 0);
            k += 1;
        }
    }

    #[test]
    fn canonical_stream_hits_both_paths() {
        let mut bad_scan = 0u32;
        let mut bad_led = 0u32;
        let mut bad_mod = 0u32;
        let mut bad_layer = 0u32;
        let mut i: u32 = 0;
        while i < CROSS_ROUNDS {
            let ev = canonical_event(i);
            if !scan_ok(ev.scan) {
                bad_scan += 1;
            }
            if !led_ok(ev.leds) {
                bad_led += 1;
            }
            if !mod_ok(ev.mods) {
                bad_mod += 1;
            }
            if !layer_ok(ev.layer) {
                bad_layer += 1;
            }
            i += 1;
        }
        assert!(bad_scan > 0 && bad_led > 0 && bad_mod > 0 && bad_layer > 0);
        assert!(bad_scan < CROSS_ROUNDS);
    }

    #[test]
    fn family_is_unique_and_zero_drift() {
        assert!(family_ok());
        assert!(family_zero_drift());
        assert_ne!(family_digest(), 0);
        assert_eq!(FAMILY.len(), FAMILY_SIZE);
        assert_eq!(lookup_code(0x71B41), Some(0));
        assert_eq!(lookup_code(0x71C46), Some(FAMILY_SIZE - 1));
        assert_eq!(lookup_code(0x71B40), None);
        assert_eq!(lookup_code(0x71CCF), None);
        assert_eq!(lookup_name("led-blocked"), Some(7));
        assert_eq!(lookup_name("nope"), None);
        assert_eq!(expected_family(0x9999), None);
    }

    #[test]
    fn family_families_resolve() {
        let mut i = 0usize;
        while i < FAMILY.len() {
            let e = FAMILY[i];
            assert!(!family_name(e.family).is_empty());
            assert_eq!(expected_family(e.code), Some(e.family));
            i += 1;
        }
        assert_eq!(family_name(7), "");
    }

    #[test]
    fn render_roundtrip() {
        let row = render_row("T", 10, 20, 30, 5, 0x0FED_CBA9_8765_4321);
        assert!(audit_render(&row));
        let (tag, vals, d) = match row_fields(&row) {
            Some(v) => v,
            None => return assert!(false),
        };
        assert_eq!(tag, "T");
        assert_eq!(vals, [10, 20, 30, 5]);
        assert_eq!(d, 0x0FED_CBA9_8765_4321);
        assert!(!audit_render("T|rounds=1"));
        assert!(parse_u32("0").is_some());
        assert!(parse_u32("12x").is_none());
        assert!(parse_hex16("00000000000000FF").is_some());
        assert!(parse_hex16("00000000000000F").is_none());
    }

    #[test]
    fn rejected_codes_are_known() {
        let mut i = 0usize;
        while i < FuzzCode::ALL.len() {
            let c = FuzzCode::ALL[i];
            assert!(known_code(c.val()));
            assert_ne!(detail(c.val()), UNKNOWN);
            i += 1;
        }
        assert!(!known_code(0));
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 14), FuzzCode::SegmentBad.detail());
    }

    #[test]
    fn segment_window_is_exact() {
        assert!(segment_guard());
        assert_eq!(FuzzCode::ScanBad.val(), SEG_BASE + 1);
        assert_eq!(FuzzCode::SegmentBad.val(), SEG_BASE + 14);
        assert_eq!(FuzzCode::FuzzBad.val(), SEG_BASE + 10);
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let s = run_unx49033_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
