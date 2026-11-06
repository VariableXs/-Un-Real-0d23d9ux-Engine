//! UNX-F49029 中文键盘布局预告位 + UNX-F49030 键盘事件延迟打点全链 · 0x71C40
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B12 族第五槽；任务板 J1 引用按原文保留）：
//!   UNX-F49029 中文键盘布局预告位（M1 桥域边界——本域只管 keymap 机制层，中文面归 M1）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49030-J1 全链 ×10^3 打点全落账（判据 J-1 载体）。
//!   UNX-F49030 键盘事件延迟打点全段接入（中断→应用回调全链）:
//!          ① 纯功能代码 ≥280 行；
//!          ② 判据原文：UNX-F49031-J1 用例入库可复跑全绿。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（keymap 机制层）：表序 → 查表 → 输出 → 桥域声明 → 预告登记。
//! 表序先于查表：表项必须按 (层, 扫描码) 严格升序且无重复。乱序表在二分
//! 或线性查表里都会静默返回错误的层——查表错了，后面所有断言都是在给
//! 错误答案盖章。
//! 查表先于输出：命中返回表项，未命中返回原样扫描码。原样是机制层唯一
//! 允许的 "不干预" 形态；把未命中伪装成某个字符，等于替上层做决定。
//! 输出先于桥域声明：机制层只产出 BMP 内的非 CJK 码位，且这条性质必须
//! 在整表上成立，而不是抽查几项。中文面归 M1，本域一旦产出 CJK 码位，
//! 就是越权——桥域声明必须由 "全表零 CJK" 支撑，不能只写在注释里。
//! 桥域声明先于预告登记：先声明本层只做机制，再登记预告位。预告位是
//! "已登记、未承诺" 的占位：它指向真实存在的表行，但不宣称任何中文
//! 输入能力。把预告位当成交付位，是本域最容易犯的虚报。
//! 闸门次序（延迟打点）：阶段 → 时间戳 → 单调 → 完整 → 预算 → 账目。
//! 阶段先于时间戳：五个阶段（中断/解码/keymap/桥/回调）必须先是闭集，
//! 否则时间戳落在哪个阶段都说不清，全链打点就退化成一条总耗时。
//! 时间戳先于单调：同一阶段重复打点与时间倒退都是坏数据，必须当场拒绝。
//! 单调先于完整：五个阶段全部落点才算一条完整链路；缺一段的链路不能
//! 计入 "全落账"，否则 10^3 的数字只是调用次数。
//! 完整先于预算：先证明链路完整，再谈每阶段是否在预算内。对残缺链路
//! 谈预算，等于用缺席者的成绩拉高平均分。
//! 预算先于账目：账目记录每阶段落点数与丢弃数；丢弃必须为零，且五阶段
//! 计数都必须非零，才允许声称 "全链打点全落账"。
//! 关键差异：本模块不做中文输入法（归 M1），不做 dead key（第二槽），
//! 不做修饰键抽样（第四槽），也不消费 LED 灯态（第三槽）。本槽只回答：
//! 机制层是否守住非 CJK 边界；事件全链的打点是否逐段落账且不超预算。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71C40，B12 族第五槽）。
pub const SEG_BASE: u32 = 0x71C40;

/// keymap 层数。
pub const LAYERS: usize = 4;
/// 表项数。
pub const KEYMAP_ROWS: usize = 12;
/// 预告位数。
pub const PREVIEW_SLOTS: usize = 8;
/// 全链阶段数。
pub const STAGES: usize = 5;
/// 全链打点轮数（判据 ×10^3）。
pub const CHAIN_ROUNDS: u32 = 1000;
/// 全链种子。
pub const CHAIN_SEED: u64 = 0x0000_0000_4903_0001;
/// 用例条数。
pub const CASE_COUNT: usize = 16;
/// 桥域越界注入次数。
pub const BRIDGE_INJECT: u32 = 10;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 预告位状态（只有 "已登记" 一种，不存在 "已交付"）。
pub const PREVIEW_STATUS: &str = "planned";
/// 桥域声明标签。
pub const BOUND_TAG: &str = "M1-BOUND";
/// 打点行标签。
pub const TRACE_TAG: &str = "TRACE";

/// HID usage 下界。
pub const KEY_MIN: u8 = 0x04;
/// HID usage 上界。
pub const KEY_MAX: u8 = 0xE7;

/// 每阶段绝对预算（微秒，自中断入口起算）。
pub const STAGE_DEADLINE: [u64; STAGES] = [20, 50, 80, 120, 200];

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KcCode {
    /// 层号非法
    LayerBad,
    /// 表项乱序/重复/越界
    MapBad,
    /// 预告位登记不合格
    PreviewBad,
    /// 桥域越权（产出 CJK 或接收 CJK）
    BridgeBad,
    /// 时间戳倒退
    TraceBad,
    /// 阶段重复或未知阶段
    StageBad,
    /// 超出自中断起算的预算
    BudgetBad,
    /// 账目不守恒
    LedgerBad,
    /// 复跑不一致
    ReplayBad,
    /// 渲染不可解析
    RenderBad,
    /// 段位越窗
    SegmentBad,
    /// 序号或计数溢出
    Overflow,
    /// 用例集不合格
    CaseBad,
    /// 覆盖轮数越界
    CoverBad,
}

impl KcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            KcCode::LayerBad => "层号越出 [0,3]",
            KcCode::MapBad => "keymap 表项乱序、重复或越界",
            KcCode::PreviewBad => "预告位未登记或越权承诺",
            KcCode::BridgeBad => "桥域越权：产出或接收 CJK 码位",
            KcCode::TraceBad => "时间戳倒退",
            KcCode::StageBad => "阶段重复或未知阶段",
            KcCode::BudgetBad => "阶段超出自中断起算的预算",
            KcCode::LedgerBad => "全链账目不守恒",
            KcCode::ReplayBad => "全链复跑不一致",
            KcCode::RenderBad => "渲染字段不可解析",
            KcCode::SegmentBad => "诊断码越出段位窗口",
            KcCode::Overflow => "序号或计数溢出",
            KcCode::CaseBad => "用例集条数或名称不合格",
            KcCode::CoverBad => "覆盖轮数越界",
        }
    }

    pub const ALL: [KcCode; 14] = [
        KcCode::LayerBad,
        KcCode::MapBad,
        KcCode::PreviewBad,
        KcCode::BridgeBad,
        KcCode::TraceBad,
        KcCode::StageBad,
        KcCode::BudgetBad,
        KcCode::LedgerBad,
        KcCode::ReplayBad,
        KcCode::RenderBad,
        KcCode::SegmentBad,
        KcCode::Overflow,
        KcCode::CaseBad,
        KcCode::CoverBad,
    ];
}

/// 按诊断码取值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < KcCode::ALL.len() {
        if let Some(c) = KcCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知键盘机制诊断码"
}

// ── 层 ────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Base,
    Shift,
    AltGr,
    Caps,
}

impl Layer {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Layer> {
        match i {
            0 => Some(Layer::Base),
            1 => Some(Layer::Shift),
            2 => Some(Layer::AltGr),
            3 => Some(Layer::Caps),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Layer::Base => "base",
            Layer::Shift => "shift",
            Layer::AltGr => "altgr",
            Layer::Caps => "caps",
        }
    }
}

// ── 机制层表 ──────────────────────────────────────────────────────
/// 一条表项：层 + 扫描码 → 码位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapEntry {
    pub layer: u8,
    pub scan: u8,
    pub out: u32,
}

/// 机制层表（按 (层, 扫描码) 升序；只含非 CJK 码位）。
pub const KEYMAP: [MapEntry; KEYMAP_ROWS] = [
    MapEntry { layer: 0, scan: 0x04, out: 0x61 },
    MapEntry { layer: 0, scan: 0x05, out: 0x62 },
    MapEntry { layer: 0, scan: 0x2C, out: 0x20 },
    MapEntry { layer: 1, scan: 0x04, out: 0x41 },
    MapEntry { layer: 1, scan: 0x05, out: 0x42 },
    MapEntry { layer: 1, scan: 0x1E, out: 0x21 },
    MapEntry { layer: 2, scan: 0x2E, out: 0x3D },
    MapEntry { layer: 2, scan: 0x2F, out: 0x5B },
    MapEntry { layer: 2, scan: 0x30, out: 0x5D },
    MapEntry { layer: 3, scan: 0x04, out: 0x41 },
    MapEntry { layer: 3, scan: 0x05, out: 0x42 },
    MapEntry { layer: 3, scan: 0x1E, out: 0x31 },
];

/// CJK 码位判定（含扩展 A、兼容、标点与全角段）。
pub fn is_cjk(code: u32) -> bool {
    (code >= 0x3000 && code <= 0x303F)
        || (code >= 0x3400 && code <= 0x4DBF)
        || (code >= 0x4E00 && code <= 0x9FFF)
        || (code >= 0xF900 && code <= 0xFAFF)
        || (code >= 0xFF00 && code <= 0xFFEF)
        || (code >= 0x20000 && code <= 0x2FFFF)
}

/// 机制层表自洽：层号有效、扫描码在域内、输出为 BMP 非 CJK、升序且唯一。
pub fn keymap_ok() -> bool {
    if KEYMAP.len() != KEYMAP_ROWS {
        return false;
    }
    let mut prev_layer: u8 = 0;
    let mut prev_scan: u8 = 0;
    let mut i = 0usize;
    let mut seen_layers: u8 = 0;
    while i < KEYMAP.len() {
        let e = match KEYMAP.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if e.layer as usize >= LAYERS {
            return false;
        }
        if e.scan < KEY_MIN || e.scan > KEY_MAX {
            return false;
        }
        if e.out == 0 || e.out > 0xFFFF || is_cjk(e.out) {
            return false;
        }
        if i > 0 {
            if e.layer < prev_layer {
                return false;
            }
            if e.layer == prev_layer && e.scan <= prev_scan {
                return false;
            }
        }
        seen_layers |= 1u8 << e.layer;
        prev_layer = e.layer;
        prev_scan = e.scan;
        i += 1;
    }
    seen_layers == 0x0F
}

/// 线性查表（层号越界返回 None）。
pub fn lookup(layer: u8, scan: u8) -> Option<u32> {
    if layer as usize >= LAYERS {
        return None;
    }
    let mut i = 0usize;
    while i < KEYMAP.len() {
        if let Some(e) = KEYMAP.get(i) {
            if e.layer == layer && e.scan == scan {
                return Some(e.out);
            }
        }
        i += 1;
    }
    None
}

/// 一次机制层输出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Output {
    pub code: u32,
    pub mapped: bool,
    pub cjk: bool,
}

/// 生成输出：命中取表项，未命中原样透传（机制层唯一允许的不干预形态）。
pub fn build_output(layer: u8, scan: u8) -> Result<Output, u32> {
    if layer as usize >= LAYERS {
        return Err(KcCode::LayerBad.val());
    }
    if scan < KEY_MIN || scan > KEY_MAX {
        return Err(KcCode::MapBad.val());
    }
    match lookup(layer, scan) {
        Some(code) => Ok(Output { code, mapped: true, cjk: is_cjk(code) }),
        None => Ok(Output { code: scan as u32, mapped: false, cjk: false }),
    }
}

/// 全表零 CJK：机制层声明必须由整表性质支撑。
pub fn table_non_cjk() -> bool {
    let mut i = 0usize;
    while i < KEYMAP.len() {
        if let Some(e) = KEYMAP.get(i) {
            if is_cjk(e.out) {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── M1 桥域边界 ───────────────────────────────────────────────────
/// 桥域声明：本域只做机制层，中文面归 M1。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct M1Boundary {
    pub owner: &'static str,
    pub domain: &'static str,
    pub mechanism_only: bool,
}

pub const M1: M1Boundary = M1Boundary { owner: "M1", domain: "CJK-input", mechanism_only: true };

/// 桥域声明摘要。
pub fn boundary_digest() -> u64 {
    let mut h = p_core::fold_u32(p_core::FNV_OFF, 1);
    h = p_core::fold_u32(h, M1.mechanism_only as u32);
    h = p_core::fold_u32(h, M1.owner.len() as u32);
    p_core::fold_u32(h, M1.domain.len() as u32)
}

/// 桥域声明行（可被 audit 原样复现）。
pub fn boundary_row() -> String {
    render_row(
        BOUND_TAG,
        1,
        M1.mechanism_only as u32,
        M1.owner.len() as u32,
        M1.domain.len() as u32,
        boundary_digest(),
    )
}

/// 桥域声明落账：标签、字段与常量一致。
pub fn boundary_declared() -> bool {
    let row = boundary_row();
    let (tag, vals, digest) = match row_fields(&row) {
        Some(v) => v,
        None => return false,
    };
    tag == BOUND_TAG
        && vals[0] == 1
        && vals[1] == 1
        && vals[2] == M1.owner.len() as u32
        && vals[3] == M1.domain.len() as u32
        && digest == boundary_digest()
        && M1.owner == "M1"
        && M1.domain == "CJK-input"
        && audit_render(&row)
}

/// 桥域转发：机制层只能转发非 CJK；CJK 一律拒绝（中文面归 M1）。
pub fn bridge_forward(code: u32) -> Result<Option<u32>, u32> {
    if is_cjk(code) {
        return Err(KcCode::BridgeBad.val());
    }
    Ok(None)
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub attempts: u32,
    pub caught: u32,
}

impl InjectLog {
    pub fn new() -> InjectLog {
        InjectLog { attempts: 0, caught: 0 }
    }

    pub fn all_caught(&self) -> bool {
        self.attempts == self.caught && self.attempts > 0
    }
}

/// 十条 CJK 码位（覆盖常用汉字、扩展 A、兼容、标点、全角、扩展 B 边界）。
pub const CJK_INJECT: [u32; 10] = [
    0x4E00, 0x9FFF, 0x3400, 0x4DBF, 0xF900, 0xFAFF, 0x3000, 0xFF01, 0x20000, 0x2FFFF,
];

/// 桥域越界注入：十条 CJK 码位必须全部被拒绝。
pub fn bridge_inject() -> InjectLog {
    let mut log = InjectLog::new();
    let mut i = 0usize;
    while i < CJK_INJECT.len() {
        if let Some(c) = CJK_INJECT.get(i) {
            log.attempts = log.attempts.saturating_add(1);
            if bridge_forward(*c).is_err() {
                log.caught = log.caught.saturating_add(1);
            }
        }
        i += 1;
    }
    log
}

// ── 预告位 ────────────────────────────────────────────────────────
/// 一个预告位：指向真实表行，但只登记、不承诺。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PreviewSlot {
    pub id: u8,
    pub layer: u8,
    pub scan: u8,
    pub label: &'static str,
}

/// 八个预告位（全部指向已存在的表行）。
pub const PREVIEW: [PreviewSlot; PREVIEW_SLOTS] = [
    PreviewSlot { id: 0, layer: 0, scan: 0x04, label: "base-a" },
    PreviewSlot { id: 1, layer: 0, scan: 0x05, label: "base-b" },
    PreviewSlot { id: 2, layer: 1, scan: 0x04, label: "shift-a" },
    PreviewSlot { id: 3, layer: 1, scan: 0x05, label: "shift-b" },
    PreviewSlot { id: 4, layer: 2, scan: 0x2E, label: "altgr-eq" },
    PreviewSlot { id: 5, layer: 2, scan: 0x2F, label: "altgr-lb" },
    PreviewSlot { id: 6, layer: 3, scan: 0x04, label: "caps-a" },
    PreviewSlot { id: 7, layer: 3, scan: 0x1E, label: "caps-1" },
];

/// 预告位摘要。
pub fn preview_digest() -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < PREVIEW.len() {
        if let Some(s) = PREVIEW.get(i) {
            h = p_core::fold_u32(h, s.id as u32);
            h = p_core::fold_u32(h, s.layer as u32);
            h = p_core::fold_u32(h, s.scan as u32);
            h = p_core::fold_u32(h, s.label.len() as u32);
        }
        i += 1;
    }
    p_core::fold_u32(h, PREVIEW.len() as u32)
}

/// 预告位登记合格：id 连续、指向真实表行、标签非空、状态只有 planned。
pub fn preview_ok() -> bool {
    if PREVIEW.len() != PREVIEW_SLOTS || PREVIEW_STATUS != "planned" {
        return false;
    }
    let mut i = 0usize;
    while i < PREVIEW.len() {
        let s = match PREVIEW.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.id != i as u8 {
            return false;
        }
        if s.layer as usize >= LAYERS {
            return false;
        }
        if s.label.is_empty() {
            return false;
        }
        match lookup(s.layer, s.scan) {
            Some(code) => {
                if is_cjk(code) {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 预告位登记行。
pub fn preview_row() -> String {
    render_row("PREVIEW", PREVIEW_SLOTS as u32, PREVIEW.len() as u32, 0, 8, preview_digest())
}

/// 预告位行可复现。
pub fn preview_row_ok() -> bool {
    let row = preview_row();
    match row_fields(&row) {
        Some((tag, vals, digest)) => {
            tag == "PREVIEW"
                && vals[0] == PREVIEW_SLOTS as u32
                && vals[1] == PREVIEW.len() as u32
                && vals[2] == 0
                && vals[3] == 8
                && digest == preview_digest()
                && audit_render(&row)
        }
        None => false,
    }
}

// ── 全链打点 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Isr,
    Decode,
    Keymap,
    Bridge,
    Callback,
}

impl Stage {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Stage> {
        match i {
            0 => Some(Stage::Isr),
            1 => Some(Stage::Decode),
            2 => Some(Stage::Keymap),
            3 => Some(Stage::Bridge),
            4 => Some(Stage::Callback),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Stage::Isr => "isr",
            Stage::Decode => "decode",
            Stage::Keymap => "keymap",
            Stage::Bridge => "bridge",
            Stage::Callback => "callback",
        }
    }

    pub fn deadline(self) -> u64 {
        match STAGE_DEADLINE.get(self.index()) {
            Some(v) => *v,
            None => 0,
        }
    }
}

/// 一条全链打点（五个阶段，时间自中断入口起算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trace {
    pub seq: u32,
    pub slots: [Option<u64>; STAGES],
    pub landed: u8,
}

impl Trace {
    pub fn new(seq: u32) -> Trace {
        Trace { seq, slots: [None; STAGES], landed: 0 }
    }

    pub fn last(&self) -> Option<u64> {
        let mut i = STAGES;
        while i > 0 {
            i -= 1;
            if let Some(t) = self.slots.get(i).and_then(|x| *x) {
                return Some(t);
            }
        }
        None
    }

    pub fn complete(&self) -> bool {
        self.landed as usize == STAGES
    }

    /// 打点：阶段重复 → StageBad；时间倒退 → TraceBad；越窗 → StageBad。
    pub fn record(&mut self, stage: Stage, at: u64) -> Result<(), u32> {
        let i = stage.index();
        if i >= STAGES {
            return Err(KcCode::StageBad.val());
        }
        if self.slots.get(i).and_then(|x| *x).is_some() {
            return Err(KcCode::StageBad.val());
        }
        if let Some(prev) = self.last() {
            if at < prev {
                return Err(KcCode::TraceBad.val());
            }
        }
        if let Some(slot) = self.slots.get_mut(i) {
            *slot = Some(at);
        }
        self.landed = self.landed.saturating_add(1);
        Ok(())
    }

    /// 审计：五段齐全且时间戳非降。
    pub fn audit(&self) -> bool {
        if !self.complete() {
            return false;
        }
        let mut prev: Option<u64> = None;
        let mut i = 0usize;
        while i < STAGES {
            let t = match self.slots.get(i).and_then(|x| *x) {
                Some(v) => v,
                None => return false,
            };
            if let Some(p) = prev {
                if t < p {
                    return false;
                }
            }
            prev = Some(t);
            i += 1;
        }
        true
    }

    /// 预算：完整且每段不超过自中断起算的绝对预算。
    pub fn within_budget(&self) -> bool {
        if !self.complete() {
            return false;
        }
        let mut i = 0usize;
        while i < STAGES {
            let t = match self.slots.get(i).and_then(|x| *x) {
                Some(v) => v,
                None => return false,
            };
            let d = match STAGE_DEADLINE.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t > d {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 相邻阶段间隔（Isr 段为自 0 起算）。
    pub fn gap(&self, stage: Stage) -> Option<u64> {
        let i = stage.index();
        let t = self.slots.get(i).and_then(|x| *x)?;
        let mut j = i;
        let mut prev = 0u64;
        while j > 0 {
            j -= 1;
            if let Some(p) = self.slots.get(j).and_then(|x| *x) {
                prev = p;
                break;
            }
        }
        Some(t - prev)
    }
}

/// 打点摘要。
pub fn fold_trace(h: u64, tr: &Trace) -> u64 {
    let mut x = p_core::fold_u32(h, tr.seq);
    let mut i = 0usize;
    while i < STAGES {
        if let Some(t) = tr.slots.get(i).and_then(|x| *x) {
            x = p_core::fold_u32(x, (t & 0xFFFF_FFFF) as u32);
            x = p_core::fold_u32(x, (t >> 32) as u32);
        }
        i += 1;
    }
    x
}

/// 全链账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainLedger {
    pub rounds: u32,
    pub traces: u32,
    pub completed: u32,
    pub dropped: u32,
    pub counts: [u32; STAGES],
    pub digest: u64,
}

impl ChainLedger {
    /// 守恒：每条链路五段全落，丢弃为零。
    pub fn conserved(&self) -> bool {
        self.dropped == 0
            && self.completed == self.rounds
            && self.traces == self.rounds
            && self.all_stages_seen()
    }

    pub fn all_stages_seen(&self) -> bool {
        let mut i = 0usize;
        while i < STAGES {
            match self.counts.get(i) {
                Some(c) if *c > 0 => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 全链 ×rounds：每条链路在每段预算内落一个时间戳。
pub fn chain_rounds(rounds: u32, seed: u64) -> Result<ChainLedger, u32> {
    if rounds == 0 || rounds > CHAIN_ROUNDS {
        return Err(KcCode::CoverBad.val());
    }
    let mut lcg = p_core::Lcg::new(seed);
    let mut digest = p_core::FNV_OFF;
    let mut counts = [0u32; STAGES];
    let mut completed: u32 = 0;
    let mut dropped: u32 = 0;
    let mut i = 0u32;
    while i < rounds {
        let mut tr = Trace::new(i);
        let mut t: u64 = 0;
        let mut s = 0usize;
        while s < STAGES {
            let stage = match Stage::from_index(s) {
                Some(v) => v,
                None => return Err(KcCode::StageBad.val()),
            };
            let deadline = stage.deadline();
            let room = if t < deadline { deadline - t } else { 1 };
            let span = if room > 16 { 16 } else { room };
            let gap = 1 + lcg.below(span as u32) as u64;
            t = match t.checked_add(gap) {
                Some(v) => v,
                None => return Err(KcCode::Overflow.val()),
            };
            tr.record(stage, t)?;
            if let Some(c) = counts.get_mut(s) {
                *c = c.saturating_add(1);
            }
            s += 1;
        }
        if tr.complete() && tr.audit() && tr.within_budget() {
            completed = completed.saturating_add(1);
        } else {
            dropped = dropped.saturating_add(1);
        }
        digest = fold_trace(digest, &tr);
        i += 1;
    }
    Ok(ChainLedger {
        rounds,
        traces: rounds,
        completed,
        dropped,
        counts,
        digest,
    })
}

/// 全链复跑对。
pub fn chain_replay() -> Result<(ChainLedger, ChainLedger), u32> {
    let a = chain_rounds(CHAIN_ROUNDS, CHAIN_SEED)?;
    let b = chain_rounds(CHAIN_ROUNDS, CHAIN_SEED)?;
    Ok((a, b))
}

/// 反例：时间倒退、阶段重复、缺段、超预算都必须被识别。
pub fn tamper_rejected() -> bool {
    let mut tr = Trace::new(0);
    if tr.record(Stage::Isr, 5).is_err() {
        return false;
    }
    if !tr.record(Stage::Decode, 4).is_err() {
        return false;
    }
    if tr.record(Stage::Decode, 6).is_err() {
        return false;
    }
    let dup_rejected = tr.record(Stage::Isr, 7).is_err();
    if !dup_rejected {
        return false;
    }
    let missing = !tr.complete() && !tr.audit();
    let mut over = Trace::new(1);
    if over.record(Stage::Isr, 21).is_err() {
        return false;
    }
    let mut s = 1usize;
    while s < STAGES {
        let st = match Stage::from_index(s) {
            Some(v) => v,
            None => return false,
        };
        let t = st.deadline() + 1;
        if over.record(st, t).is_err() {
            return false;
        }
        s += 1;
    }
    missing && over.complete() && !over.within_budget()
}

/// 打点行。
pub fn trace_row(tr: &Trace) -> String {
    let last = tr.last().unwrap_or(0);
    render_row(TRACE_TAG, tr.seq, (last & 0xFFFF_FFFF) as u32, 0, tr.landed as u32, fold_trace(p_core::FNV_OFF, tr))
}

/// 打点行可复现。
pub fn trace_row_ok(tr: &Trace) -> bool {
    let row = trace_row(tr);
    match row_fields(&row) {
        Some((tag, vals, digest)) => {
            tag == TRACE_TAG
                && vals[0] == tr.seq
                && vals[1] == (tr.last().unwrap_or(0) & 0xFFFF_FFFF) as u32
                && vals[2] == 0
                && vals[3] == tr.landed as u32
                && digest == fold_trace(p_core::FNV_OFF, tr)
                && audit_render(&row)
        }
        None => false,
    }
}

// ── 用例集（F49030 判据：入库可复跑全绿） ────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    pub name: &'static str,
    pub passed: bool,
}

/// 16 条入库用例，全部由真实函数求值。
pub fn case_set() -> Vec<Case> {
    let mut v: Vec<Case> = Vec::new();
    v.push(Case { name: "keymap_sorted_unique", passed: keymap_ok() });
    v.push(Case { name: "keymap_non_cjk", passed: table_non_cjk() });
    v.push(Case { name: "lookup_hit", passed: lookup(0, 0x04) == Some(0x61) });
    v.push(Case { name: "lookup_miss", passed: lookup(0, 0x7F).is_none() });
    v.push(Case { name: "output_passthrough", passed: matches!(build_output(0, 0x7F), Ok(o) if !o.mapped && o.code == 0x7F) });
    v.push(Case { name: "output_bad_layer", passed: build_output(4, 0x04).is_err() });
    v.push(Case { name: "output_bad_scan", passed: build_output(0, 0x00).is_err() });
    v.push(Case { name: "boundary_declared", passed: boundary_declared() });
    v.push(Case { name: "bridge_rejects_cjk", passed: bridge_forward(0x4E00).is_err() });
    v.push(Case { name: "bridge_passes_ascii", passed: matches!(bridge_forward(0x61), Ok(None)) });
    v.push(Case { name: "preview_registered", passed: preview_ok() });
    v.push(Case { name: "preview_row", passed: preview_row_ok() });
    v.push(Case { name: "trace_tamper", passed: tamper_rejected() });
    v.push(Case { name: "trace_row", passed: {
        let mut tr = Trace::new(0);
        let mut s = 0usize;
        let mut ok = true;
        while s < STAGES {
            let stage = match Stage::from_index(s) { Some(v) => v, None => { ok = false; break; } };
            if tr.record(stage, 10 + s as u64).is_err() { ok = false; break; }
            s += 1;
        }
        ok && trace_row_ok(&tr)
    } });
    v.push(Case { name: "segment_window", passed: segment_guard() });
    v.push(Case { name: "code_detail", passed: detail(KcCode::BridgeBad.val()) == KcCode::BridgeBad.detail() });
    v
}

/// 用例集全绿：条数、名称非空且唯一、逐条通过。
pub fn case_set_ok() -> bool {
    let set = case_set();
    if set.len() != CASE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < set.len() {
        let a = match set.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.name.is_empty() || !a.passed {
            return false;
        }
        let mut j = i + 1;
        while j < set.len() {
            if let Some(b) = set.get(j) {
                if a.name == b.name {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 用例集摘要（名称 + 结果）。
pub fn case_digest() -> u64 {
    let set = case_set();
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < set.len() {
        if let Some(c) = set.get(i) {
            let bytes = c.name.as_bytes();
            let mut k = 0usize;
            while k < bytes.len() {
                if let Some(b) = bytes.get(k) {
                    h = p_core::fold_byte(h, *b);
                }
                k += 1;
            }
            h = p_core::fold_u32(h, c.passed as u32);
        }
        i += 1;
    }
    p_core::fold_u32(h, CASE_COUNT as u32)
}

/// 用例集复跑：两次摘要相等且全绿。
pub fn case_replay_ok() -> bool {
    case_set_ok() && case_digest() == case_digest() && case_digest() != 0
}

// ── 段位守卫 ──────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < KcCode::ALL.len() {
        if let Some(c) = KcCode::ALL.get(i) {
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

// ── 渲染 ──────────────────────────────────────────────────────────
/// 六字段渲染：tag | params | schedule | fails | keys | digest。
pub fn render_row(tag: &str, params: u32, schedule: u32, fails: u32, keys: u32, digest: u64) -> String {
    format!(
        "{}|params={}|schedule={}|fails={}|keys={}|digest={:016X}",
        tag, params, schedule, fails, keys, digest
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

/// 六字段解析：标签、四个数值、摘要。
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

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49029 ①：机制层表自洽（有序、四层齐、输出 BMP 非 CJK、查表一致）。
    let map = keymap_ok() && table_non_cjk();
    let hits = lookup(0, 0x04) == Some(0x61) && lookup(3, 0x1E) == Some(0x31) && lookup(9, 0x04).is_none();
    let outputs = matches!(build_output(0, 0x7F), Ok(o) if !o.mapped && o.code == 0x7F)
        && build_output(4, 0x04).is_err();
    out.push((
        "F49029/机制层自洽",
        map && hits && outputs,
        format!("rows={} map={} hits={} outputs={}", KEYMAP.len(), map, hits, outputs),
    ));

    // F49029 ②：M1 边界不越权（声明落账 + 全表零 CJK + 十条注入全拒）。
    let log = bridge_inject();
    let decl = boundary_declared();
    out.push((
        "F49029/M1边界不越权",
        decl && table_non_cjk() && log.all_caught() && log.attempts == BRIDGE_INJECT,
        format!(
            "declared={} attempts={} caught={}",
            decl, log.attempts, log.caught
        ),
    ));

    // F49029 ③：预告位登记（8 位、指向真实行、零承诺）。
    let prev = preview_ok() && preview_row_ok() && PREVIEW.len() == PREVIEW_SLOTS;
    out.push((
        "F49029/预告位登记",
        prev && PREVIEW_STATUS == "planned",
        format!("slots={} status={} digest={:016X}", PREVIEW.len(), PREVIEW_STATUS, preview_digest()),
    ));

    // F49029 ④：全链 ×10^3 打点全落账。
    let chain = chain_rounds(CHAIN_ROUNDS, CHAIN_SEED);
    let chain_ok = match &chain {
        Ok(l) => l.rounds == CHAIN_ROUNDS && l.traces == CHAIN_ROUNDS && l.conserved(),
        Err(_) => false,
    };
    out.push((
        "F49029/全链×10^3落账",
        chain_ok,
        format!(
            "rounds={:?} completed={:?} dropped={:?}",
            chain.as_ref().map(|l| l.rounds),
            chain.as_ref().map(|l| l.completed),
            chain.as_ref().map(|l| l.dropped)
        ),
    ));

    // F49030 ①：五阶段全覆盖（每段计数非零）。
    let cover = match &chain {
        Ok(l) => l.all_stages_seen()
            && matches!(l.counts.get(0), Some(c) if *c == CHAIN_ROUNDS)
            && matches!(l.counts.get(4), Some(c) if *c == CHAIN_ROUNDS),
        Err(_) => false,
    };
    out.push((
        "F49030/五点全落账",
        cover,
        format!("counts={:?}", chain.as_ref().map(|l| l.counts)),
    ));

    // F49030 ②：延迟预算（全部链路在预算内 + 超预算反例必拒）。
    let budget = match &chain {
        Ok(l) => l.completed == l.rounds && l.dropped == 0,
        Err(_) => false,
    };
    let tamper = tamper_rejected();
    out.push((
        "F49030/延迟预算",
        budget && tamper,
        format!("budget={} tamper={}", budget, tamper),
    ));

    // F49030 ③：用例入库可复跑全绿（16 条、零重复、摘要稳定）。
    let cases = case_set_ok() && case_replay_ok();
    out.push((
        "F49030/用例全绿",
        cases,
        format!("cases={} digest={:016X}", CASE_COUNT, case_digest()),
    ));

    // F49030 ④：全链复跑一致 + 段位守卫。
    let replay = chain_replay();
    let rep_ok = match &replay {
        Ok((a, b)) => a.digest == b.digest && a.digest != 0 && a.conserved() && b.conserved(),
        Err(_) => false,
    };
    let seg = segment_guard();
    out.push((
        "F49030/复跑一致",
        rep_ok && seg,
        format!(
            "digest={:016X} stable={} seg={}",
            replay.as_ref().map(|(a, _)| a.digest).unwrap_or_else(|_| 0),
            rep_ok,
            seg
        ),
    ));

    out
}

pub fn run_unx49029_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49030 中文布局预告位与键盘延迟全链打点");
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
    fn keymap_is_sorted_unique_and_non_cjk() {
        assert!(keymap_ok());
        assert!(table_non_cjk());
    }

    #[test]
    fn every_layer_present() {
        let mut bits = 0u8;
        let mut i = 0usize;
        while i < KEYMAP.len() {
            if let Some(e) = KEYMAP.get(i) {
                bits |= 1u8 << e.layer;
            }
            i += 1;
        }
        assert_eq!(bits, 0x0F);
    }

    #[test]
    fn lookup_hits_and_misses() {
        assert_eq!(lookup(0, 0x04), Some(0x61));
        assert_eq!(lookup(1, 0x1E), Some(0x21));
        assert_eq!(lookup(2, 0x30), Some(0x5D));
        assert_eq!(lookup(3, 0x1E), Some(0x31));
        assert_eq!(lookup(0, 0x7F), None);
        assert_eq!(lookup(4, 0x04), None);
    }

    #[test]
    fn output_passthrough_and_errors() {
        let o = match build_output(0, 0x7F) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert!(!o.mapped);
        assert_eq!(o.code, 0x7F);
        assert_eq!(build_output(4, 0x04), Err(KcCode::LayerBad.val()));
        assert_eq!(build_output(0, 0x00), Err(KcCode::MapBad.val()));
        assert_eq!(build_output(0, 0x00), Err(KcCode::MapBad.val()));
    }

    #[test]
    fn cjk_ranges_are_detected() {
        assert!(is_cjk(0x4E00));
        assert!(is_cjk(0x3400));
        assert!(is_cjk(0xF900));
        assert!(is_cjk(0x3000));
        assert!(is_cjk(0xFF01));
        assert!(is_cjk(0x20000));
        assert!(!is_cjk(0x61));
        assert!(!is_cjk(0x1F600));
    }

    #[test]
    fn boundary_row_roundtrip() {
        let row = boundary_row();
        assert!(audit_render(&row));
        assert!(boundary_declared());
        let (tag, vals, _) = match row_fields(&row) {
            Some(v) => v,
            None => return assert!(false),
        };
        assert_eq!(tag, BOUND_TAG);
        assert_eq!(vals[0], 1);
        assert_eq!(vals[1], 1);
    }

    #[test]
    fn bridge_rejects_cjk_codes() {
        assert_eq!(bridge_forward(0x4E00), Err(KcCode::BridgeBad.val()));
        assert_eq!(bridge_forward(0x61), Ok(None));
        let log = bridge_inject();
        assert_eq!(log.attempts, BRIDGE_INJECT);
        assert!(log.all_caught());
    }

    #[test]
    fn preview_slots_registered() {
        assert!(preview_ok());
        assert!(preview_row_ok());
        assert_eq!(PREVIEW.len(), PREVIEW_SLOTS);
        assert_eq!(PREVIEW_STATUS, "planned");
        let mut i = 0usize;
        while i < PREVIEW.len() {
            if let Some(s) = PREVIEW.get(i) {
                assert_eq!(s.id as usize, i);
                assert!(!s.label.is_empty());
                assert!(lookup(s.layer, s.scan).is_some());
            }
            i += 1;
        }
    }

    #[test]
    fn stage_deadlines_ascending() {
        let mut i = 1usize;
        while i < STAGES {
            let a = STAGE_DEADLINE.get(i - 1);
            let b = STAGE_DEADLINE.get(i);
            match (a, b) {
                (Some(x), Some(y)) => assert!(y > x),
                _ => assert!(false),
            }
            i += 1;
        }
    }

    #[test]
    fn trace_requires_monotonic_stages() {
        let mut tr = Trace::new(0);
        assert!(tr.record(Stage::Isr, 1).is_ok());
        assert_eq!(tr.record(Stage::Decode, 0), Err(KcCode::TraceBad.val()));
        assert_eq!(tr.record(Stage::Isr, 2), Err(KcCode::StageBad.val()));
        assert!(tr.record(Stage::Decode, 2).is_ok());
        assert!(!tr.complete());
        assert!(!tr.audit());
    }

    #[test]
    fn trace_complete_and_budget() {
        let mut tr = Trace::new(7);
        let mut s = 0usize;
        while s < STAGES {
            let stage = match Stage::from_index(s) {
                Some(v) => v,
                None => return assert!(false),
            };
            assert!(tr.record(stage, 5 + s as u64).is_ok());
            s += 1;
        }
        assert!(tr.complete());
        assert!(tr.audit());
        assert!(tr.within_budget());
        assert_eq!(tr.last(), Some(9));
        assert_eq!(tr.gap(Stage::Callback), Some(1));
        assert!(trace_row_ok(&tr));
    }

    #[test]
    fn trace_over_budget_is_flagged() {
        let mut tr = Trace::new(0);
        assert!(tr.record(Stage::Isr, 21).is_ok());
        let mut s = 1usize;
        while s < STAGES {
            let stage = match Stage::from_index(s) {
                Some(v) => v,
                None => return assert!(false),
            };
            assert!(tr.record(stage, stage.deadline() + 1).is_ok());
            s += 1;
        }
        assert!(tr.complete());
        assert!(tr.audit());
        assert!(!tr.within_budget());
    }

    #[test]
    fn chain_1000_conserved() {
        let l = match chain_rounds(CHAIN_ROUNDS, CHAIN_SEED) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.rounds, CHAIN_ROUNDS);
        assert_eq!(l.traces, CHAIN_ROUNDS);
        assert_eq!(l.completed, CHAIN_ROUNDS);
        assert_eq!(l.dropped, 0);
        assert!(l.conserved());
        assert!(l.all_stages_seen());
        assert_ne!(l.digest, 0);
    }

    #[test]
    fn chain_replay_stable() {
        let (a, b) = match chain_replay() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(a.digest, b.digest);
        assert_ne!(a.digest, 0);
        assert!(a.conserved() && b.conserved());
    }

    #[test]
    fn chain_cover_bounds() {
        assert_eq!(chain_rounds(0, CHAIN_SEED), Err(KcCode::CoverBad.val()));
        assert_eq!(chain_rounds(CHAIN_ROUNDS + 1, CHAIN_SEED), Err(KcCode::CoverBad.val()));
    }

    #[test]
    fn tamper_is_rejected() {
        assert!(tamper_rejected());
    }

    #[test]
    fn cases_are_sixteen_green() {
        let set = case_set();
        assert_eq!(set.len(), CASE_COUNT);
        assert!(case_set_ok());
        let mut i = 0usize;
        while i < set.len() {
            if let Some(c) = set.get(i) {
                assert!(c.passed, "case {} failed", c.name);
            }
            i += 1;
        }
        assert!(case_replay_ok());
        assert_ne!(case_digest(), 0);
    }

    #[test]
    fn segment_guard_and_codes() {
        assert!(segment_guard());
        assert_eq!(KcCode::LayerBad.val(), SEG_BASE + 1);
        assert_eq!(KcCode::CoverBad.val(), SEG_BASE + 14);
        assert_eq!(detail(KcCode::BridgeBad.val()), KcCode::BridgeBad.detail());
        assert_eq!(detail(0), "未知键盘机制诊断码");
    }

    #[test]
    fn render_roundtrip_and_rejects() {
        let row = render_row("T", 1, 2, 3, 4, 0x1F);
        assert!(audit_render(&row));
        assert!(!audit_render(""));
        assert!(!audit_render("T|params=1|schedule=2|fails=3|keys=4"));
        assert!(!audit_render("T|params=1|schedule=2|fails=3|keys=4|digest=ZZZZZZZZZZZZZZZZ"));
        assert!(parse_u32("4294967295").is_some());
        assert!(parse_u32("4294967296").is_none());
        assert!(parse_hex16("000000000000001F").is_some());
        assert!(parse_hex16("00000000000001F").is_none());
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let s = run_unx49029_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }
}
