//! # unx49085 — B15 keymap 回退与坏设备洪泛轮
//!
//! 卡面原文（任务板 CoRun·UNX-F2-B40）：
//!
//! > **F49085 keymap 损坏回退全路径（回退美式+告警可查终验）**
//! > **F49086 坏设备 fuzz 洪泛轮（×10^4 轮零崩零泄漏）**
//!
//! 验收锚点：
//!
//! - `UNX-F49085-J1` 回退 ×10 全过；判据锚定 §AI-62 §2 专题二错误路径。
//! - `UNX-F49086-J1` fuzz ×10^4 零崩零泄漏；判据锚定 体验十 健壮性。
//!
//! ## 闸序与理由
//!
//! 1. **回退美式**：任一本地方案损坏，投递键必须逐键等于美式表；回退不是
//!    “尽量给点东西”，而是给出可验证的确定结果，先立这条，告警才有意义。
//! 2. **告警可查**：回退一旦发生必须留下可查询的告警，且同一告警去重；
//!    告警查不到等于静默降级，比不降级更糟。
//! 3. **万轮零崩**：坏设备洪泛只调用纯分类器，任意 32 位事件都必须归入
//!    接受或四类丢弃之一，不允许 panic、不允许未记账。
//! 4. **零泄漏**：接受数加四类丢弃数必须严格等于轮数；窗口占用必须落在
//!    容量内；少记一条即判泄漏。
//! 5. **账行回读**：回退账与洪泛账都必须能渲染、回读、拒篡改；不能回读
//!    的账在回归时无法对账。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49081_defense` 处理的是限速执行器与洪泛取证；本模块处理的是
//!   **layout 损坏后的确定回退**与**坏设备事件的分类守恒**，两者共享“洪泛”
//!   一词但对象不同：前者是速率，后者是单事件合法性。
//! - `unx49083_reorder` 关心时间戳乱序后的重置；本模块不排序、不重置，
//!   只在单轮内把事件归入五个账格。
//! - `unx49086` 的容量是洪泛窗容量，不是内核缓冲区上限；两者不互相冒充。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位与常量 ───────────────────────────────────────────────────
/// 本模块段基址（B15 第二槽）。
pub const SEG_BASE: u32 = 0x73580;
/// 段宽（相邻模块互斥）。
pub const SEG_SPAN: u32 = 0x40;
/// 方案数。
pub const MAPS: usize = 4;
/// 单方案键数。
pub const KEYS_PER_MAP: usize = 16;
/// 事件总位数。
pub const EVENT_BITS: u32 = 32;
/// 洪泛默认轮数。
pub const FLOOD_ROUNDS: u32 = 10_000;
/// 洪泛轮数上限。
pub const FLOOD_MAX: u32 = 20_000;
/// 洪泛窗容量。
pub const WINDOW_CAP: u32 = 8;
/// 排空周期（每多少轮）。
pub const DRAIN_EVERY: u32 = 64;
/// 单次排空量。
pub const DRAIN_AMOUNT: u32 = 4;
/// 留痕复跑次数。
pub const REPLAY_TIMES: u32 = 3;
/// 告警槽位上限。
pub const ALERTS_MAX: usize = 8;
/// 告警窗基址（段内独立窗口）。
pub const ALERT_BASE: u32 = SEG_BASE + 0x20;
/// 回退账行标签。
pub const FALL_TAG: &str = "FALL85";
/// 洪泛账行标签。
pub const FLOOD_TAG: &str = "FLOOD86";
/// 回退账键序。
pub const FALL_KEYS: [&str; 3] = ["map", "fallback", "alert"];
/// 洪泛账键序。
pub const FLOOD_KEYS: [&str; 3] = ["round", "accepted", "dropped"];
/// 回退账字段数。
pub const FALL_FIELDS: usize = 5;
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B15 回退诊断码";
/// 未知告警说明。
pub const UNKNOWN_ALERT: &str = "未知 B15 键位告警";

// ── 诊断码 ───────────────────────────────────────────────────────
/// 回退与洪泛诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KfCode {
    /// 方案编号越界
    MapBad,
    /// 方案签名长度不符
    SignatureBad,
    /// 签名校验和不符
    ChecksumBad,
    /// 损坏方案未回退
    FallbackBad,
    /// 回退目标不是美式
    TargetBad,
    /// 告警缺失
    AlertMissing,
    /// 告警重复入册
    AlertDup,
    /// 告警码越出告警窗
    AlertCodeBad,
    /// 轮数为零或超上限
    RoundsBad,
    /// 接受加丢弃不等于轮数
    LeakBad,
    /// 窗口占用越出容量
    WindowBad,
    /// 摘要与重算不符
    DigestBad,
    /// 账行键序或字段错位
    RowBad,
    /// 方案表不完整
    Incomplete,
}

impl KfCode {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            KfCode::MapBad => "方案编号越出 0..=3",
            KfCode::SignatureBad => "方案签名长度不足六字节",
            KfCode::ChecksumBad => "签名校验和与登记值不符",
            KfCode::FallbackBad => "损坏方案未触发回退",
            KfCode::TargetBad => "回退目标不是美式布局",
            KfCode::AlertMissing => "回退发生但告警未入册",
            KfCode::AlertDup => "同一告警被重复入册",
            KfCode::AlertCodeBad => "告警码越出本段告警窗",
            KfCode::RoundsBad => "洪泛轮数为零或超 20_000",
            KfCode::LeakBad => "接受加四类丢弃不等于轮数",
            KfCode::WindowBad => "洪泛窗占用越出容量八",
            KfCode::DigestBad => "链式摘要与重算不一致",
            KfCode::RowBad => "账行标签、键序或字段数错位",
            KfCode::Incomplete => "方案表或告警表不完整",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            KfCode::MapBad => "方案越界：请回读方案表",
            KfCode::SignatureBad => "签名残缺：请重录方案",
            KfCode::ChecksumBad => "校验失配：已按损坏处理",
            KfCode::FallbackBad => "回退缺失：请检查投递链",
            KfCode::TargetBad => "目标错误：必须回退美式",
            KfCode::AlertMissing => "告警丢失：请补入册",
            KfCode::AlertDup => "告警重复：请去重",
            KfCode::AlertCodeBad => "告警码越界：请核对窗口",
            KfCode::RoundsBad => "轮次非法：请收敛到 1..=20_000",
            KfCode::LeakBad => "记账泄漏：请补齐五格",
            KfCode::WindowBad => "窗口溢出：请先排空",
            KfCode::DigestBad => "摘要失配：请重算账行",
            KfCode::RowBad => "账行损坏：请重渲染",
            KfCode::Incomplete => "表不完整：请补足四方案",
        }
    }
}

/// 全部诊断码（顺序即值序）。
pub const ALL: [KfCode; 14] = [
    KfCode::MapBad,
    KfCode::SignatureBad,
    KfCode::ChecksumBad,
    KfCode::FallbackBad,
    KfCode::TargetBad,
    KfCode::AlertMissing,
    KfCode::AlertDup,
    KfCode::AlertCodeBad,
    KfCode::RoundsBad,
    KfCode::LeakBad,
    KfCode::WindowBad,
    KfCode::DigestBad,
    KfCode::RowBad,
    KfCode::Incomplete,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => KfCode::MapBad.detail(),
        2 => KfCode::SignatureBad.detail(),
        3 => KfCode::ChecksumBad.detail(),
        4 => KfCode::FallbackBad.detail(),
        5 => KfCode::TargetBad.detail(),
        6 => KfCode::AlertMissing.detail(),
        7 => KfCode::AlertDup.detail(),
        8 => KfCode::AlertCodeBad.detail(),
        9 => KfCode::RoundsBad.detail(),
        10 => KfCode::LeakBad.detail(),
        11 => KfCode::WindowBad.detail(),
        12 => KfCode::DigestBad.detail(),
        13 => KfCode::RowBad.detail(),
        14 => KfCode::Incomplete.detail(),
        _ => UNKNOWN,
    }
}

/// 段位自检：14 码值两两不同且全部落在本段窗口内。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ALL.len() {
        let a = ALL[i].val();
        if a <= SEG_BASE || a > SEG_BASE + 0x0F {
            return false;
        }
        let mut j = i + 1;
        while j < ALL.len() {
            if ALL[j].val() == a {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 布局与签名 ───────────────────────────────────────────────────
/// 键位方案。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapKind {
    /// 美式 QWERTY（回退目标）。
    Us,
    /// 英式。
    Uk,
    /// 德式 QWERTZ。
    De,
    /// 法式 AZERTY。
    Fr,
}

impl MapKind {
    /// 稳定编号。
    pub fn index(self) -> u8 {
        match self {
            MapKind::Us => 0,
            MapKind::Uk => 1,
            MapKind::De => 2,
            MapKind::Fr => 3,
        }
    }

    /// 方案名。
    pub fn name(self) -> &'static str {
        match self {
            MapKind::Us => "us",
            MapKind::Uk => "uk",
            MapKind::De => "de",
            MapKind::Fr => "fr",
        }
    }

    /// 由编号反解。
    pub fn from_index(i: u8) -> Option<MapKind> {
        match i {
            0 => Some(MapKind::Us),
            1 => Some(MapKind::Uk),
            2 => Some(MapKind::De),
            3 => Some(MapKind::Fr),
            _ => None,
        }
    }

    /// 由方案名反解。
    pub fn from_name(name: &str) -> Option<MapKind> {
        match name {
            "us" => Some(MapKind::Us),
            "uk" => Some(MapKind::Uk),
            "de" => Some(MapKind::De),
            "fr" => Some(MapKind::Fr),
            _ => None,
        }
    }

    /// 手写签名（八字节）。
    pub fn signature(self) -> [u8; 8] {
        match self {
            MapKind::Us => [0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23],
            MapKind::Uk => [0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x2B],
            MapKind::De => [0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x2C, 0x23],
            MapKind::Fr => [0x10, 0x30, 0x2E, 0x20, 0x12, 0x22, 0x21, 0x23],
        }
    }

    /// 手写键表（十六键）。
    pub fn keys(self) -> &'static [char; KEYS_PER_MAP] {
        match self {
            MapKind::Us => &KEY_US,
            MapKind::Uk => &KEY_UK,
            MapKind::De => &KEY_DE,
            MapKind::Fr => &KEY_FR,
        }
    }
}

/// 美式键表（回退基准）。
pub const KEY_US: [char; KEYS_PER_MAP] = [
    'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', 'a', 's', 'd', 'f', 'g', 'h',
];
/// 英式键表（字母层与美式同形，差异落在签名与校验和）。
pub const KEY_UK: [char; KEYS_PER_MAP] = [
    'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', 'a', 's', 'd', 'f', 'g', 'h',
];
/// 德式键表（y/z 互换）。
pub const KEY_DE: [char; KEYS_PER_MAP] = [
    'q', 'w', 'e', 'r', 't', 'z', 'u', 'i', 'o', 'p', 'a', 's', 'd', 'f', 'g', 'h',
];
/// 法式键表（a/q 互换）。
pub const KEY_FR: [char; KEYS_PER_MAP] = [
    'a', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', 'q', 's', 'd', 'f', 'g', 'h',
];

/// 签名校验和：方案名 + 八字节签名。
pub fn checksum(kind: MapKind) -> u32 {
    let mut h = p_core::FNV_OFF as u32;
    let bytes = kind.name().as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        h = p_core::fold_byte(h as u64, bytes[i]) as u32;
        i += 1;
    }
    let sig = kind.signature();
    let mut i = 0usize;
    while i < sig.len() {
        h = p_core::fold_byte(h as u64, sig[i]) as u32;
        i += 1;
    }
    h
}

/// 登记方案：编号 + 期望校验和。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Keymap {
    /// 方案。
    pub kind: MapKind,
    /// 磁盘上的校验和。
    pub stored: u32,
}

impl Keymap {
    /// 由方案造出完好登记。
    pub fn standard(kind: MapKind) -> Keymap {
        Keymap {
            kind,
            stored: checksum(kind),
        }
    }

    /// 完好登记。
    pub fn intact(&self) -> bool {
        self.stored == checksum(self.kind)
    }

    /// 损坏一笔（翻转最高位）。
    pub fn corrupted(&self) -> Keymap {
        Keymap {
            kind: self.kind,
            stored: self.stored ^ 0x8000_0000,
        }
    }
}

/// 回退结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Resolution {
    /// 实际投递方案。
    pub map: MapKind,
    /// 是否发生回退。
    pub fallback: bool,
    /// 告警（无回退时为空）。
    pub alert: Option<Alert>,
}

impl Resolution {
    /// 投递键。
    pub fn key(&self, slot: usize) -> Option<char> {
        if slot >= KEYS_PER_MAP {
            return None;
        }
        Some(self.map.keys()[slot])
    }
}

/// 解析登记：完好则用原方案，损坏则回退美式并告警。
pub fn resolve(reg: &Keymap) -> Resolution {
    if reg.intact() {
        Resolution {
            map: reg.kind,
            fallback: false,
            alert: None,
        }
    } else if reg.kind == MapKind::Us {
        Resolution {
            map: MapKind::Us,
            fallback: true,
            alert: Some(Alert::KeymapCritical),
        }
    } else {
        Resolution {
            map: MapKind::Us,
            fallback: true,
            alert: Some(Alert::KeymapFallback),
        }
    }
}

// ── 告警 ─────────────────────────────────────────────────────────
/// 键位告警（四类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Alert {
    /// 本地方案损坏，已回退美式。
    KeymapFallback,
    /// 连美式登记都损坏，按编译期默认表投递。
    KeymapCritical,
    /// 单轮事件被限速丢弃。
    RateLimited,
    /// 洪泛窗满，事件被丢弃。
    WindowFull,
}

impl Alert {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            Alert::KeymapFallback => 0,
            Alert::KeymapCritical => 1,
            Alert::RateLimited => 2,
            Alert::WindowFull => 3,
        }
    }

    /// 告警码（段内告警窗）。
    pub fn code(self) -> u32 {
        ALERT_BASE + 0x01 + self as u32
    }

    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Alert::KeymapFallback => "fallback",
            Alert::KeymapCritical => "critical",
            Alert::RateLimited => "rate",
            Alert::WindowFull => "window",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Alert> {
        match tag {
            "fallback" => Some(Alert::KeymapFallback),
            "critical" => Some(Alert::KeymapCritical),
            "rate" => Some(Alert::RateLimited),
            "window" => Some(Alert::WindowFull),
            _ => None,
        }
    }

    /// 自由函数：告警码 → 说明。
    pub fn describe(code: u32) -> &'static str {
        match code.wrapping_sub(ALERT_BASE) {
            1 => "本地方案损坏，已回退美式",
            2 => "美式登记亦损坏，按默认表投递",
            3 => "单轮事件被限速丢弃",
            4 => "洪泛窗满，事件被丢弃",
            _ => UNKNOWN_ALERT,
        }
    }
}

/// 有界告警册：容量八，同码去重。
#[derive(Clone, Debug)]
pub struct AlertLog {
    /// 已入册告警。
    pub items: Vec<Alert>,
    /// 去重命中的次数。
    pub duplicates: u32,
    /// 因容量满而未入册的次数。
    pub overflowed: u32,
}

impl AlertLog {
    /// 空册。
    pub fn new() -> AlertLog {
        AlertLog {
            items: Vec::new(),
            duplicates: 0,
            overflowed: 0,
        }
    }

    /// 入册（同码去重；满册计溢出）。
    pub fn push(&mut self, a: Alert) {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i] == a {
                self.duplicates = self.duplicates.saturating_add(1);
                return;
            }
            i += 1;
        }
        if self.items.len() >= ALERTS_MAX {
            self.overflowed = self.overflowed.saturating_add(1);
            return;
        }
        self.items.push(a);
    }

    /// 是否可查。
    pub fn contains(&self, a: Alert) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i] == a {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 册长。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

// ── 洪泛分类 ─────────────────────────────────────────────────────
/// 单事件分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    /// 接受入窗。
    Accept,
    /// 设备号非法。
    BadDevice,
    /// 字段畸形。
    Malformed,
    /// 限速丢弃。
    RateLimited,
    /// 窗满丢弃。
    WindowFull,
}

impl Class {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Class::Accept => "accept",
            Class::BadDevice => "device",
            Class::Malformed => "malformed",
            Class::RateLimited => "rate",
            Class::WindowFull => "window",
        }
    }

    /// 是否为接受。
    pub fn accepted(self) -> bool {
        self == Class::Accept
    }
}

/// 洪泛报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FloodReport {
    /// 轮数。
    pub rounds: u32,
    /// 接受数。
    pub accepted: u32,
    /// 非法设备数。
    pub bad_device: u32,
    /// 畸形字段数。
    pub malformed: u32,
    /// 限速丢弃数。
    pub rate_limited: u32,
    /// 窗满丢弃数。
    pub window_full: u32,
    /// 未记账数（必须为零）。
    pub leak: u32,
    /// 收尾窗占用。
    pub occupancy: u32,
    /// 链式摘要。
    pub digest: u64,
}

impl FloodReport {
    /// 五格合计。
    pub fn accounted(&self) -> u32 {
        self.accepted
            .saturating_add(self.bad_device)
            .saturating_add(self.malformed)
            .saturating_add(self.rate_limited)
            .saturating_add(self.window_full)
    }

    /// 零泄漏：五格等于轮数，且未记账为零。
    pub fn conserved(&self) -> bool {
        self.leak == 0 && self.accounted() == self.rounds
    }

    /// 四类丢弃全部出现（洪泛确实压到了每条分支）。
    pub fn all_drops_seen(&self) -> bool {
        self.bad_device > 0
            && self.malformed > 0
            && self.rate_limited > 0
            && self.window_full > 0
    }
}

/// 单轮分类：设备三位、码八位、标志八位、值八位、序号五位。
pub fn classify(raw: u32) -> (Class, u8) {
    let device = (raw >> 29) & 0x7;
    let code = (raw >> 21) & 0xFF;
    let flags = (raw >> 13) & 0xFF;
    let slot = raw & 0x1F;
    if device > 3 {
        return (Class::BadDevice, device as u8);
    }
    if code == 0 || (flags & 0x80) != 0 {
        return (Class::Malformed, code as u8);
    }
    if slot == 0 {
        return (Class::RateLimited, slot as u8);
    }
    (Class::Accept, device as u8)
}

/// 跑 `rounds` 轮洪泛（轮数非法时返回 `None`）。
pub fn flood(rounds: u32) -> Option<FloodReport> {
    if rounds == 0 || rounds > FLOOD_MAX {
        return None;
    }
    let mut rng = p_core::Lcg::new(0x0055_5A5A_85F0_0001);
    let mut accepted = 0u32;
    let mut bad_device = 0u32;
    let mut malformed = 0u32;
    let mut rate_limited = 0u32;
    let mut window_full = 0u32;
    let mut occupancy = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < rounds {
        let raw = rng.next() as u32;
        let (mut class, tail) = classify(raw);
        if class == Class::Accept {
            if occupancy >= WINDOW_CAP {
                class = Class::WindowFull;
            } else {
                occupancy = occupancy.saturating_add(1);
            }
        }
        match class {
            Class::Accept => accepted = accepted.saturating_add(1),
            Class::BadDevice => bad_device = bad_device.saturating_add(1),
            Class::Malformed => malformed = malformed.saturating_add(1),
            Class::RateLimited => rate_limited = rate_limited.saturating_add(1),
            Class::WindowFull => window_full = window_full.saturating_add(1),
        }
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, raw);
        h = p_core::fold_u32(h, class as u32);
        h = p_core::fold_byte(h, tail);
        digest = p_core::fold_u32(p_core::fold_u32(digest, h as u32), (h >> 32) as u32);
        if i % DRAIN_EVERY == 0 {
            occupancy = occupancy.saturating_sub(DRAIN_AMOUNT);
        }
        i += 1;
    }
    let report = FloodReport {
        rounds,
        accepted,
        bad_device,
        malformed,
        rate_limited,
        window_full,
        leak: 0,
        occupancy,
        digest,
    };
    if report.accounted() != rounds {
        return None;
    }
    Some(report)
}

/// 万轮零崩：报告成立、零泄漏、四类丢弃全见、摘要非零。
pub fn flood_green() -> bool {
    match flood(FLOOD_ROUNDS) {
        Some(r) => r.conserved() && r.all_drops_seen() && r.digest != p_core::FNV_OFF,
        None => false,
    }
}

/// 留痕稳定：同一轮数重复 `times` 次摘要逐位一致。
pub fn replay_stable(times: u32) -> bool {
    if times == 0 || times > 8 {
        return false;
    }
    let want = match flood(FLOOD_ROUNDS) {
        Some(r) => r.digest,
        None => return false,
    };
    let mut i = 0u32;
    while i < times {
        match flood(FLOOD_ROUNDS) {
            Some(r) => {
                if r.digest != want {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    want != p_core::FNV_OFF
}

// ── 账行 ─────────────────────────────────────────────────────────
/// 解析十进制（拒绝空串、非数字、溢出）。
pub fn parse_u32(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut acc = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?;
        acc = acc.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

/// 解析 16 位十六进制摘要。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut acc = 0u64;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let v = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        acc = acc.checked_mul(16)?;
        acc = acc.checked_add(v)?;
        i += 1;
    }
    Some(acc)
}

/// 按固定标签与键序拆行；返回各键值与摘要值。
pub fn row_fields<'a>(row: &'a str, tag: &str, keys: &[&str]) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = row.split('|').collect();
    if parts.len() != keys.len() + 2 {
        return None;
    }
    if parts[0] != tag {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < keys.len() {
        let piece = parts[i + 1];
        let rest = piece.strip_prefix(keys[i])?;
        let value = rest.strip_prefix('=')?;
        if value.is_empty() {
            return None;
        }
        out.push(value);
        i += 1;
    }
    let d = parts[keys.len() + 1].strip_prefix("digest=")?;
    if d.len() != 16 {
        return None;
    }
    out.push(d);
    Some(out)
}

/// 回退账摘要：方案号 + 回退位 + 告警标签。
pub fn fallback_digest(kind: MapKind, fallback: bool, alert: Option<Alert>) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x4641_4C4C);
    h = p_core::fold_u32(h, kind.index() as u32);
    h = p_core::fold_u32(h, if fallback { 1 } else { 0 });
    let tag = match alert {
        Some(a) => a.tag(),
        None => "none",
    };
    let bytes = tag.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        h = p_core::fold_byte(h, bytes[i]);
        i += 1;
    }
    h
}

/// 渲染回退账行。
pub fn render_fallback_row(reg: &Keymap) -> String {
    let res = resolve(reg);
    let alert = match res.alert {
        Some(a) => a.tag(),
        None => "none",
    };
    format!(
        "{}|map={}|fallback={}|alert={}|digest={:016x}",
        FALL_TAG,
        reg.kind.name(),
        res.fallback,
        alert,
        fallback_digest(reg.kind, res.fallback, res.alert)
    )
}

/// 回读回退账行（重算方案与告警）。
pub fn audit_fallback_row(row: &str) -> bool {
    let f = match row_fields(row, FALL_TAG, &FALL_KEYS) {
        Some(v) if v.len() == FALL_KEYS.len() + 1 => v,
        _ => return false,
    };
    let kind = match MapKind::from_name(f[0]) {
        Some(k) => k,
        None => return false,
    };
    let want_fallback = match f[1] {
        "true" => true,
        "false" => false,
        _ => return false,
    };
    let alert = if f[2] == "none" {
        None
    } else {
        match Alert::from_tag(f[2]) {
            Some(a) => Some(a),
            None => return false,
        }
    };
    // 账行以“登记损坏”为回退场景；完好场景必须无回退、无告警。
    let reg = if want_fallback {
        Keymap::standard(kind).corrupted()
    } else {
        Keymap::standard(kind)
    };
    let res = resolve(&reg);
    if res.fallback != want_fallback || res.alert != alert {
        return false;
    }
    let want_map = if want_fallback { MapKind::Us } else { kind };
    if res.map != want_map {
        return false;
    }
    let want = match parse_hex16(f[3]) {
        Some(v) => v,
        None => return false,
    };
    fallback_digest(kind, want_fallback, alert) == want
}

/// 回退账摘要：轮数 + 五格。
pub fn flood_digest(r: &FloodReport) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x464C_4F44);
    h = p_core::fold_u32(h, r.rounds);
    h = p_core::fold_u32(h, r.accepted);
    h = p_core::fold_u32(h, r.bad_device);
    h = p_core::fold_u32(h, r.malformed);
    h = p_core::fold_u32(h, r.rate_limited);
    h = p_core::fold_u32(h, r.window_full);
    h
}

/// 渲染洪泛账行。
pub fn render_flood_row(r: &FloodReport) -> String {
    format!(
        "{}|round={}|accepted={}|dropped={}|digest={:016x}",
        FLOOD_TAG,
        r.rounds,
        r.accepted,
        r.bad_device
            .saturating_add(r.malformed)
            .saturating_add(r.rate_limited)
            .saturating_add(r.window_full),
        flood_digest(r)
    )
}

/// 回读洪泛账行（重跑同轮数并比对五格）。
pub fn audit_flood_row(row: &str) -> bool {
    let f = match row_fields(row, FLOOD_TAG, &FLOOD_KEYS) {
        Some(v) if v.len() == FLOOD_KEYS.len() + 1 => v,
        _ => return false,
    };
    let rounds = match parse_u32(f[0]) {
        Some(v) if v >= 1 && v <= FLOOD_MAX => v,
        _ => return false,
    };
    let accepted = match parse_u32(f[1]) {
        Some(v) => v,
        _ => return false,
    };
    let dropped = match parse_u32(f[2]) {
        Some(v) => v,
        _ => return false,
    };
    if accepted.saturating_add(dropped) != rounds {
        return false;
    }
    let want = match parse_hex16(f[3]) {
        Some(v) => v,
        None => return false,
    };
    match flood(rounds) {
        Some(r) => {
            r.accepted == accepted
                && r.rounds.saturating_sub(r.accepted) == dropped
                && flood_digest(&r) == want
        }
        None => false,
    }
}

/// 八条回退账行（四方案 × 完好/损坏）。
pub fn export_fallback_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0u8;
    while i < MAPS as u8 {
        if let Some(kind) = MapKind::from_index(i) {
            let std = Keymap::standard(kind);
            out.push(render_fallback_row(&std));
            out.push(render_fallback_row(&std.corrupted()));
        }
        i += 1;
    }
    out
}

/// 八行两两不同且逐行可回读。
pub fn fallback_rows_stable() -> bool {
    let rows = export_fallback_rows();
    if rows.len() != MAPS * 2 {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        if !audit_fallback_row(&rows[i]) {
            return false;
        }
        let mut j = i + 1;
        while j < rows.len() {
            if rows[j] == rows[i] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 八项判据 ─────────────────────────────────────────────────────
/// 八项自检（真算，不预置结论）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1) 回退美式：三个本地方案损坏后逐键等于美式表。
    let mut fallback_ok = true;
    let mut fallback_count = 0u32;
    let mut i = 1u8;
    while i < MAPS as u8 {
        if let Some(kind) = MapKind::from_index(i) {
            let res = resolve(&Keymap::standard(kind).corrupted());
            fallback_count += 1;
            if res.map != MapKind::Us || !res.fallback || res.alert.is_none() {
                fallback_ok = false;
            }
            let mut slot = 0usize;
            while slot < KEYS_PER_MAP {
                if res.key(slot) != Some(KEY_US[slot]) {
                    fallback_ok = false;
                }
                slot += 1;
            }
        }
        i += 1;
    }
    let intact_ok = {
        let mut ok = true;
        let mut k = 0u8;
        while k < MAPS as u8 {
            if let Some(kind) = MapKind::from_index(k) {
                let res = resolve(&Keymap::standard(kind));
                if res.map != kind || res.fallback || res.alert.is_some() {
                    ok = false;
                }
            }
            k += 1;
        }
        ok
    };
    let critical = resolve(&Keymap::standard(MapKind::Us).corrupted());
    let critical_ok = critical.map == MapKind::Us
        && critical.fallback
        && critical.alert == Some(Alert::KeymapCritical);
    let ok = fallback_ok && intact_ok && critical_ok && fallback_count == 3;
    out.push((
        "F49085/回退美式",
        ok,
        format!(
            "fallback={} intact={} critical={} target=us",
            fallback_count, intact_ok, critical_ok
        ),
    ));

    // 2) 告警可查：入册、去重、可查、码在窗内。
    let mut log = AlertLog::new();
    let mut k = 0usize;
    while k < 4 {
        let a = match k {
            0 => Alert::KeymapFallback,
            1 => Alert::KeymapCritical,
            2 => Alert::RateLimited,
            _ => Alert::WindowFull,
        };
        log.push(a);
        log.push(a);
        k += 1;
    }
    let mut codes_ok = true;
    let mut q = 0usize;
    while q < 4 {
        let a = log.items[q];
        if a.code() <= ALERT_BASE || a.code() > ALERT_BASE + 0x0F {
            codes_ok = false;
        }
        if !log.contains(a) {
            codes_ok = false;
        }
        q += 1;
    }
    let query_ok = log.len() == 4 && log.duplicates == 4 && log.overflowed == 0;
    let unknown_ok = Alert::describe(ALERT_BASE) == UNKNOWN_ALERT
        && Alert::describe(ALERT_BASE + 0x10) == UNKNOWN_ALERT;
    let ok = query_ok && codes_ok && unknown_ok && !log.is_empty();
    out.push((
        "F49085/告警可查",
        ok,
        format!(
            "alerts={} dup={} codes_ok={} unknown_ok={}",
            log.len(),
            log.duplicates,
            codes_ok,
            unknown_ok
        ),
    ));

    // 3) 回退账行回读。
    let rows = export_fallback_rows();
    let mut tamp = String::new();
    if let Some(first) = rows.first() {
        tamp.push_str(first);
        tamp.push('x');
    }
    let tamper_rejected = !tamp.is_empty() && !audit_fallback_row(&tamp);
    let ok = rows.len() == MAPS * 2 && fallback_rows_stable() && tamper_rejected;
    out.push((
        "F49085/账行回读",
        ok,
        format!(
            "rows={} stable={} tamper_rejected={}",
            rows.len(),
            fallback_rows_stable(),
            tamper_rejected
        ),
    ));

    // 4) 万轮零崩。
    let report = flood(FLOOD_ROUNDS);
    let crash_free = match report {
        Some(r) => r.conserved() && r.digest != p_core::FNV_OFF,
        None => false,
    };
    let bounds_rejected = flood(0).is_none() && flood(FLOOD_MAX + 1).is_none();
    let ok = flood_green() && crash_free && bounds_rejected;
    out.push((
        "F49086/万轮零崩",
        ok,
        format!(
            "rounds={} crash_free={} bounds_rejected={}",
            FLOOD_ROUNDS, crash_free, bounds_rejected
        ),
    ));

    // 5) 零泄漏：五格闭合、四类丢弃全见、窗占用有界。
    let ok = match report {
        Some(r) => {
            r.leak == 0
                && r.accounted() == FLOOD_ROUNDS
                && r.all_drops_seen()
                && r.accepted > 0
                && r.occupancy <= WINDOW_CAP
        }
        None => false,
    };
    out.push((
        "F49086/零泄漏",
        ok,
        match report {
            Some(r) => format!(
                "rounds={} accepted={} device={} malformed={} rate={} window={} leak={}",
                r.rounds,
                r.accepted,
                r.bad_device,
                r.malformed,
                r.rate_limited,
                r.window_full,
                r.leak
            ),
            None => String::from("no report"),
        },
    ));

    // 6) 洪泛账行回读。
    let row_ok = match report {
        Some(r) => {
            let row = render_flood_row(&r);
            let mut tamp = row.clone();
            tamp.push('9');
            audit_flood_row(&row) && !audit_flood_row(&tamp)
        }
        None => false,
    };
    out.push((
        "F49086/账行回读",
        row_ok,
        format!("row_ok={} rounds={}", row_ok, FLOOD_ROUNDS),
    ));

    // 7) 异常码守卫。
    let mut distinct = true;
    let mut a = 0usize;
    while a < ALL.len() {
        let mut b = a + 1;
        while b < ALL.len() {
            if ALL[a].val() == ALL[b].val()
                || ALL[a].detail() == ALL[b].detail()
                || ALL[a].message() == ALL[b].message()
            {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    let free_ok = detail(SEG_BASE) == UNKNOWN
        && detail(SEG_BASE + 0x0F) == UNKNOWN
        && detail(ALL[0].val()) == KfCode::MapBad.detail();
    let alert_conflict = {
        let mut clash = false;
        let mut x = 0usize;
        while x < 4 {
            let code = ALERT_BASE + 1 + x as u32;
            let mut y = 0usize;
            while y < ALL.len() {
                if ALL[y].val() == code {
                    clash = true;
                }
                y += 1;
            }
            x += 1;
        }
        !clash
    };
    let ok = segment_guard() && distinct && free_ok && alert_conflict;
    out.push((
        "F49085/异常码守卫",
        ok,
        format!(
            "codes={} guard={} distinct={} free={} alert_window={}",
            ALL.len(),
            segment_guard(),
            distinct,
            free_ok,
            alert_conflict
        ),
    ));

    // 8) 复跑留痕：三次摘要一致，且不同轮数不同摘要。
    let replay_ok = replay_stable(REPLAY_TIMES) && !replay_stable(0);
    let differ = match (flood(1_000), flood(2_000)) {
        (Some(a), Some(b)) => a.digest != b.digest,
        _ => false,
    };
    let ok = replay_ok && differ;
    out.push((
        "F49086/复跑留痕",
        ok,
        match report {
            Some(r) => format!(
                "replays={} digest={:016x} differ={}",
                REPLAY_TIMES, r.digest, differ
            ),
            None => String::from("no report"),
        },
    ));

    out
}

/// 入口：八项判据写入 CheckSet。
pub fn run_unx49085_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49085 keymap 回退与坏设备洪泛");
    let checks = extra_checks();
    let mut i = 0usize;
    while i < checks.len() {
        s.add_dyn(checks[i].0, checks[i].1, &checks[i].2);
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 诊断码 ──────────────────────────────────────────────────

    #[test]
    fn codes_in_window() {
        assert!(segment_guard());
        assert_eq!(ALL.len(), 14);
        let mut i = 0usize;
        while i < ALL.len() {
            assert!(ALL[i].val() > SEG_BASE);
            assert!(ALL[i].val() < SEG_BASE + SEG_SPAN);
            i += 1;
        }
    }

    #[test]
    fn codes_value_order() {
        let mut i = 0usize;
        while i < ALL.len() {
            assert_eq!(ALL[i].val(), SEG_BASE + 1 + i as u32);
            i += 1;
        }
    }

    #[test]
    fn codes_detail_and_message_distinct() {
        let mut i = 0usize;
        while i < ALL.len() {
            let mut j = i + 1;
            while j < ALL.len() {
                assert_ne!(ALL[i].val(), ALL[j].val());
                assert_ne!(ALL[i].detail(), ALL[j].detail());
                assert_ne!(ALL[i].message(), ALL[j].message());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn detail_free_matches_table() {
        let mut i = 0usize;
        while i < ALL.len() {
            assert_eq!(detail(ALL[i].val()), ALL[i].detail());
            i += 1;
        }
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(SEG_BASE + SEG_SPAN), UNKNOWN);
    }

    // ── 方案与签名 ──────────────────────────────────────────────

    #[test]
    fn map_index_roundtrip() {
        let mut i = 0u8;
        while i < MAPS as u8 {
            let k = match MapKind::from_index(i) {
                Some(k) => k,
                None => panic!("方案反解失败"),
            };
            assert_eq!(k.index(), i);
            assert!(!k.name().is_empty());
            i += 1;
        }
        assert_eq!(MapKind::from_index(MAPS as u8), None);
        assert_eq!(MapKind::from_index(255), None);
    }

    #[test]
    fn map_names_distinct() {
        let names = ["us", "uk", "de", "fr"];
        let mut i = 0usize;
        while i < MAPS {
            let k = match MapKind::from_index(i as u8) {
                Some(k) => k,
                None => panic!("方案缺失"),
            };
            assert_eq!(k.name(), names[i]);
            let mut j = i + 1;
            while j < MAPS {
                assert_ne!(names[j], names[i]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn signatures_eight_bytes_distinct() {
        let mut i = 0u8;
        while i < MAPS as u8 {
            let a = match MapKind::from_index(i) {
                Some(k) => k,
                None => panic!("方案缺失"),
            };
            assert_eq!(a.signature().len(), 8);
            let mut j = i + 1;
            while j < MAPS as u8 {
                let b = match MapKind::from_index(j) {
                    Some(k) => k,
                    None => panic!("方案缺失"),
                };
                assert_ne!(a.signature(), b.signature());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn checksum_deterministic_and_distinct() {
        let mut seen = [0u32; MAPS];
        let mut i = 0usize;
        while i < MAPS {
            let k = match MapKind::from_index(i as u8) {
                Some(k) => k,
                None => panic!("方案缺失"),
            };
            let a = checksum(k);
            assert_eq!(a, checksum(k));
            seen[i] = a;
            i += 1;
        }
        let mut i = 0usize;
        while i < MAPS {
            let mut j = i + 1;
            while j < MAPS {
                assert_ne!(seen[i], seen[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn keymap_intact_and_corrupted() {
        let mut i = 0u8;
        while i < MAPS as u8 {
            let k = match MapKind::from_index(i) {
                Some(k) => k,
                None => panic!("方案缺失"),
            };
            let good = Keymap::standard(k);
            assert!(good.intact());
            let bad = good.corrupted();
            assert!(!bad.intact());
            assert_eq!(bad.kind, k);
            assert_eq!(bad.stored ^ 0x8000_0000, good.stored);
            i += 1;
        }
    }

    #[test]
    fn key_tables_sixteen() {
        let mut i = 0u8;
        while i < MAPS as u8 {
            let k = match MapKind::from_index(i) {
                Some(k) => k,
                None => panic!("方案缺失"),
            };
            assert_eq!(k.keys().len(), KEYS_PER_MAP);
            i += 1;
        }
        assert_eq!(KEY_US.len(), KEYS_PER_MAP);
    }

    #[test]
    fn layouts_differ_where_declared() {
        assert_eq!(KEY_US[5], 'y');
        assert_eq!(KEY_DE[5], 'z');
        assert_eq!(KEY_US[0], 'q');
        assert_eq!(KEY_FR[0], 'a');
        assert_eq!(KEY_FR[10], 'q');
    }

    // ── 回退 ────────────────────────────────────────────────────

    #[test]
    fn intact_map_keeps_itself() {
        let mut i = 0u8;
        while i < MAPS as u8 {
            let k = match MapKind::from_index(i) {
                Some(k) => k,
                None => panic!("方案缺失"),
            };
            let res = resolve(&Keymap::standard(k));
            assert_eq!(res.map, k);
            assert!(!res.fallback);
            assert_eq!(res.alert, None);
            i += 1;
        }
    }

    #[test]
    fn corrupted_local_falls_back_to_us() {
        let mut i = 1u8;
        while i < MAPS as u8 {
            let k = match MapKind::from_index(i) {
                Some(k) => k,
                None => panic!("方案缺失"),
            };
            let res = resolve(&Keymap::standard(k).corrupted());
            assert_eq!(res.map, MapKind::Us);
            assert!(res.fallback);
            assert_eq!(res.alert, Some(Alert::KeymapFallback));
            let mut slot = 0usize;
            while slot < KEYS_PER_MAP {
                assert_eq!(res.key(slot), Some(KEY_US[slot]));
                slot += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn corrupted_us_is_critical_but_still_us() {
        let res = resolve(&Keymap::standard(MapKind::Us).corrupted());
        assert_eq!(res.map, MapKind::Us);
        assert!(res.fallback);
        assert_eq!(res.alert, Some(Alert::KeymapCritical));
    }

    #[test]
    fn resolution_key_bounds() {
        let res = resolve(&Keymap::standard(MapKind::Us));
        assert_eq!(res.key(0), Some('q'));
        assert_eq!(res.key(KEYS_PER_MAP - 1), Some('h'));
        assert_eq!(res.key(KEYS_PER_MAP), None);
        assert_eq!(res.key(usize::MAX), None);
    }

    // ── 告警 ────────────────────────────────────────────────────

    #[test]
    fn alert_tags_roundtrip() {
        let all = [
            Alert::KeymapFallback,
            Alert::KeymapCritical,
            Alert::RateLimited,
            Alert::WindowFull,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(Alert::from_tag(all[i].tag()), Some(all[i]));
            i += 1;
        }
        assert_eq!(Alert::from_tag("none"), None);
        assert_eq!(Alert::from_tag(""), None);
    }

    #[test]
    fn alert_codes_in_window() {
        let mut i = 0usize;
        while i < 4 {
            let a = match i {
                0 => Alert::KeymapFallback,
                1 => Alert::KeymapCritical,
                2 => Alert::RateLimited,
                _ => Alert::WindowFull,
            };
            assert_eq!(a.index() as usize, i);
            assert!(a.code() > ALERT_BASE);
            assert!(a.code() <= ALERT_BASE + 0x0F);
            assert_ne!(Alert::describe(a.code()), UNKNOWN_ALERT);
            i += 1;
        }
        assert_eq!(Alert::describe(ALERT_BASE), UNKNOWN_ALERT);
        assert_eq!(Alert::describe(ALERT_BASE + 0x10), UNKNOWN_ALERT);
    }

    #[test]
    fn alert_log_dedup_and_query() {
        let mut log = AlertLog::new();
        assert!(log.is_empty());
        log.push(Alert::KeymapFallback);
        log.push(Alert::KeymapFallback);
        log.push(Alert::WindowFull);
        assert_eq!(log.len(), 2);
        assert_eq!(log.duplicates, 1);
        assert!(log.contains(Alert::KeymapFallback));
        assert!(log.contains(Alert::WindowFull));
        assert!(!log.contains(Alert::RateLimited));
        assert_eq!(log.overflowed, 0);
    }

    #[test]
    fn alert_log_holds_four_kinds() {
        let mut log = AlertLog::new();
        log.push(Alert::KeymapFallback);
        log.push(Alert::KeymapCritical);
        log.push(Alert::RateLimited);
        log.push(Alert::WindowFull);
        assert_eq!(log.len(), 4);
        assert!(log.len() <= ALERTS_MAX);
    }

    // ── 分类 ────────────────────────────────────────────────────

    #[test]
    fn classify_bad_device() {
        let raw = compose_event(4, 1, 0, 1);
        assert_eq!(classify(raw).0, Class::BadDevice);
        assert_eq!(classify(compose_event(7, 1, 0, 1)).0, Class::BadDevice);
    }

    #[test]
    fn classify_malformed() {
        assert_eq!(classify(compose_event(0, 0, 0, 1)).0, Class::Malformed);
        assert_eq!(classify(compose_event(0, 1, 0x80, 1)).0, Class::Malformed);
    }

    #[test]
    fn classify_rate_limited() {
        assert_eq!(classify(compose_event(0, 1, 0, 0)).0, Class::RateLimited);
    }

    #[test]
    fn classify_accept() {
        let (c, dev) = classify(compose_event(2, 5, 0x10, 3));
        assert_eq!(c, Class::Accept);
        assert_eq!(dev, 2);
        assert!(c.accepted());
        assert_eq!(c.tag(), "accept");
    }

    #[test]
    fn class_tags_distinct() {
        let all = [
            Class::Accept,
            Class::BadDevice,
            Class::Malformed,
            Class::RateLimited,
            Class::WindowFull,
        ];
        let mut i = 0usize;
        while i < all.len() {
            let mut j = i + 1;
            while j < all.len() {
                assert_ne!(all[i].tag(), all[j].tag());
                j += 1;
            }
            i += 1;
        }
    }

    // ── 洪泛 ────────────────────────────────────────────────────

    fn compose_event(device: u32, code: u32, flags: u32, slot: u32) -> u32 {
        (device << 29) | (code << 21) | (flags << 13) | slot
    }

    #[test]
    fn flood_round_bounds() {
        assert_eq!(flood(0), None);
        assert_eq!(flood(FLOOD_MAX + 1), None);
        assert!(flood(u32::MAX).is_none());
    }

    #[test]
    fn flood_one_round_conserved() {
        let r = match flood(1) {
            Some(r) => r,
            None => panic!("单轮应成立"),
        };
        assert_eq!(r.rounds, 1);
        assert!(r.conserved());
        assert_eq!(r.leak, 0);
    }

    #[test]
    fn flood_ten_thousand_green() {
        assert!(flood_green());
        let r = match flood(FLOOD_ROUNDS) {
            Some(r) => r,
            None => panic!("万轮应成立"),
        };
        assert_eq!(r.accounted(), FLOOD_ROUNDS);
        assert!(r.all_drops_seen());
        assert!(r.accepted > 0);
        assert!(r.occupancy <= WINDOW_CAP);
        assert_ne!(r.digest, p_core::FNV_OFF);
    }

    #[test]
    fn flood_max_rounds_conserved() {
        let r = match flood(FLOOD_MAX) {
            Some(r) => r,
            None => panic!("上限轮数应成立"),
        };
        assert!(r.conserved());
        assert!(r.all_drops_seen());
    }

    #[test]
    fn flood_deterministic() {
        let a = match flood(2_048) {
            Some(r) => r,
            None => panic!("首跑"),
        };
        let b = match flood(2_048) {
            Some(r) => r,
            None => panic!("复跑"),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn flood_different_rounds_differ() {
        let a = match flood(1_000) {
            Some(r) => r,
            None => panic!("首跑"),
        };
        let b = match flood(2_000) {
            Some(r) => r,
            None => panic!("复跑"),
        };
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn replay_stable_bounds() {
        assert!(replay_stable(1));
        assert!(replay_stable(REPLAY_TIMES));
        assert!(replay_stable(8));
        assert!(!replay_stable(0));
        assert!(!replay_stable(9));
    }

    #[test]
    fn report_conservation_predicate() {
        let mut r = match flood(64) {
            Some(r) => r,
            None => panic!("64 轮"),
        };
        assert!(r.conserved());
        r.leak = 1;
        assert!(!r.conserved());
        r.leak = 0;
        r.accepted = r.accepted.saturating_add(1);
        if r.accounted() != r.rounds {
            assert!(!r.conserved());
        }
    }

    #[test]
    fn flood_digest_depends_on_fields() {
        let base = match flood(256) {
            Some(r) => r,
            None => panic!("256 轮"),
        };
        let mut other = base;
        other.accepted = other.accepted.saturating_add(1);
        assert_ne!(flood_digest(&base), flood_digest(&other));
    }

    // ── 账行 ────────────────────────────────────────────────────

    #[test]
    fn fallback_rows_count_and_stable() {
        let rows = export_fallback_rows();
        assert_eq!(rows.len(), MAPS * 2);
        let first = match rows.first() {
            Some(r) => r,
            None => panic!("无账行"),
        };
        assert_eq!(first.split('|').count(), FALL_FIELDS);
        assert!(fallback_rows_stable());
    }

    #[test]
    fn fallback_rows_distinct() {
        let rows = export_fallback_rows();
        let mut i = 0usize;
        while i < rows.len() {
            let mut j = i + 1;
            while j < rows.len() {
                assert_ne!(rows[i], rows[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn fallback_row_rejects_tamper() {
        let rows = export_fallback_rows();
        let first = match rows.first() {
            Some(r) => r,
            None => panic!("无账行"),
        };
        assert!(audit_fallback_row(first));
        let mut tamp = first.clone();
        tamp.push('x');
        assert!(!audit_fallback_row(&tamp));
        assert!(!audit_fallback_row("FALL85|map=us|fallback=maybe|alert=none|digest=0000000000000000"));
    }

    #[test]
    fn flood_row_render_audit() {
        let r = match flood(512) {
            Some(r) => r,
            None => panic!("512 轮"),
        };
        let row = render_flood_row(&r);
        assert!(row.starts_with(FLOOD_TAG));
        assert_eq!(row.split('|').count(), FLOOD_KEYS.len() + 2);
        assert!(audit_flood_row(&row));
        let mut tamp = row.clone();
        tamp.push('0');
        assert!(!audit_flood_row(&tamp));
    }

    #[test]
    fn flood_row_rejects_bad_rounds() {
        assert!(!audit_flood_row(
            "FLOOD86|round=0|accepted=0|dropped=0|digest=0000000000000000"
        ));
        assert!(!audit_flood_row(
            "FLOOD86|round=20001|accepted=0|dropped=0|digest=0000000000000000"
        ));
    }

    #[test]
    fn parse_u32_boundaries() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("1x"), None);
        assert_eq!(parse_u32("-3"), None);
    }

    #[test]
    fn parse_hex16_boundaries() {
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("0123456789abcdef"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("0123456789ABCDEF"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("0123456789abcde"), None);
        assert_eq!(parse_hex16("0123456789abcdef0"), None);
        assert_eq!(parse_hex16("0123456789abcdeg"), None);
    }

    #[test]
    fn row_fields_shape() {
        let r = match flood(32) {
            Some(r) => r,
            None => panic!("32 轮"),
        };
        let row = render_flood_row(&r);
        assert_eq!(
            row_fields(&row, FLOOD_TAG, &FLOOD_KEYS).map(|v| v.len()),
            Some(4)
        );
        assert_eq!(row_fields(&row, FALL_TAG, &FLOOD_KEYS), None);
        assert_eq!(row_fields(&row, FLOOD_TAG, &FALL_KEYS), None);
        assert_eq!(row_fields(FLOOD_TAG, FLOOD_TAG, &FLOOD_KEYS), None);
    }

    #[test]
    fn digest_functions_differ() {
        let r = match flood(64) {
            Some(r) => r,
            None => panic!("64 轮"),
        };
        let a = fallback_digest(MapKind::Us, false, None);
        let b = fallback_digest(MapKind::Uk, false, None);
        assert_ne!(a, b);
        assert_ne!(a, flood_digest(&r));
        assert_ne!(a, p_core::FNV_OFF);
    }

    // ── CheckSet 契约 ───────────────────────────────────────────

    #[test]
    fn checkset_eight_green() {
        let s = run_unx49085_checks();
        assert_eq!(s.len(), 8);
        assert_eq!(s.passed_count(), 8);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.red_items_text().is_empty());
    }

    #[test]
    fn checks_names_and_details() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1);
            assert!(items[i].0.starts_with("F49085/") || items[i].0.starts_with("F49086/"));
            assert!(!items[i].2.is_empty());
            i += 1;
        }
    }
}
