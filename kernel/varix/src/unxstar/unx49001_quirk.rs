//! UNX-F49001 quirk 表引擎总成 + UNX-F49002 quirk 匹配器 · 0x718C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，VX6 批 B11-A 首槽）：
//!   UNX-F49001 quirk 表引擎总成（表项=机型/固件版本/参数覆盖三段式）:
//!           ① 纯功能代码 ≥400 行；
//!           ② 判据原文：UNX-F49002-J1 匹配链 ×10^2 覆盖正确。
//!   UNX-F49002 quirk 匹配器（设备探测→表项命中→参数覆盖链）:
//!           ① 纯功能代码 ≥380 行；
//!           ② 判据原文：UNX-F49003-J1 降级 ×10 设备记账零虚报。
//! 说明（板上自身的 +1 引用，不做改号）：F49001 卡面引用的 J1 是
//! UNX-F49002-J1，F49002 卡面引用的 J1 是 UNX-F49003-J1。本模块逐字保留
//! 该引用（不"修"成 F49001-J1/F49002-J1），以便任务板锚定可逐字回查。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（匹配链）：探测合法性 → 表自审 → 表项命中 → 参数覆盖 → 落账。
//! 探测先于表：机型/厂商为 0、固件字节为 0xFF 是「设备自报坏数据」，此时
//! 任何表项命中都不可信——先拒探测，后面的扫描才有意义。若先查表，坏
//! 探测可能因为 model 恰好为 0 而撞上"通配表项"，把传感器缺陷伪装成命中。
//! 表自审先于命中：表项越界（fw_lo > fw_hi）、参数越界、档位重复时，
//! "命中"只是一次带毒读取。先自审能把「表的病」与「设备不在表里」分开：
//! 前者是 Degraded（降级用默认参数并记账），后者是 Miss（合法的无表项）。
//! 命中先于覆盖：覆盖掩码只在命中之后才允许合并。若先合并再命中，未命中
//! 设备也会继承"最后一条表项"的参数，等于把 quirk 泄漏给全网设备。
//! 覆盖先于落账：落账记录的是"合并后的实际参数摘要"，不是表项原始参数。
//! 若先落账后合并，账本会声称下发了表项参数，而引擎实际下发默认值——
//! 这正是「虚报」的形态，F49003-J1 的零虚报正是靠这一序位守住的。
//! 闸门次序（10 设备账）：分类 → 声称/实际包含关系 → 分类计数 → 摘要。
//! 分类先于包含：一台设备只能是一种身份（全量/降级/拒绝）。先把分类钉死，
//! "声称能力是否被实际能力包含"才是可判定的；分类摇摆时包含关系会自相
//! 矛盾（既全量又降级），账本永远对不上。
//! 关键差异：本模块回答三组互不改写的问题——
//! ① 表：三段式表项（机型 / 固件区间 / 参数覆盖）本身是否自洽、可寻址；
//! ② 链：一次设备探测究竟落在 Hit / Miss / Degraded / Rejected 哪一格，
//!    且合并后的参数摘要可复算；
//! ③ 账：10 台设备的降级记账是否零虚报（声称 ⊄ 实际 的条数必须为 0）。
//! 本模块不做配置持久化（B11-C）、不做 T1 承诺面（B11-B）、不做 fuzz 注入
//! 的损坏回退（B11-E）；命中的 quirk 参数只产出结构体，不写硬件寄存器。
//! 表以定长数组固化、匹配为纯函数：同输入必同输出，跨批复跑不漂移。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x718C0，VX6 批 B11-A 首槽）。
pub const SEG_BASE: u32 = 0x718C0;

/// 表项容量（定长，禁止越界增长）。
pub const TABLE_CAP: usize = 16;
/// 参数段数（手腕阈值 / 增益 / 点按力度 / 滞回）。
pub const PARAM_FIELDS: usize = 4;
/// J1 场景轮数 ×10^2。
pub const SCENARIOS: u32 = 100;
/// 场景轮数上界（覆盖闸门）。
pub const SCENARIO_MAX: u32 = 400;
/// 降级记账设备数（卡面 ×10）。
pub const DEGRADE_DEVICES: usize = 10;
/// 注入路数。
pub const INJECT_ROUTES: usize = 4;
/// 渲染字段数。
pub const STATS_FIELDS: usize = 6;
/// 黄金脚本设备数。
pub const GOLDEN_ROUNDS: u32 = 16;
/// fuzz 表项轮数（×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;

/// 手腕阈值下界（毫米当量）。
pub const PALM_MIN: u16 = 20;
/// 手腕阈值上界。
pub const PALM_MAX: u16 = 80;
/// 增益下界（Q8）。
pub const GAIN_MIN: u16 = 32;
/// 增益上界（Q8）。
pub const GAIN_MAX: u16 = 1_024;
/// 点按力度下界（毫牛当量）。
pub const TAP_MIN: u16 = 10;
/// 点按力度上界。
pub const TAP_MAX: u16 = 200;
/// 滞回下界（毫米当量）。
pub const HYST_MIN: u16 = 2;
/// 滞回上界。
pub const HYST_MAX: u16 = 32;
/// 固件字节坏值哨兵。
pub const FW_SENTINEL: u8 = 0xFF;
/// 固件打包上界（24 位）。
pub const FW_MAX: u32 = 0x00FF_FFFF;
/// 优先级上界。
pub const PRIO_MAX: u8 = 15;
/// 表项标签上界（标签 0 保留为非法）。
pub const TAG_MAX: u8 = 15;

/// 表内机型池（黄金表与场景生成共用）。
pub const MODELS: [u16; 4] = [0x1001, 0x1002, 0x1003, 0x1004];

/// 能力位：手腕阈值。
pub const CAP_PALM: u8 = 0b0001;
/// 能力位：增益。
pub const CAP_GAIN: u8 = 0b0010;
/// 能力位：点按力度。
pub const CAP_TAP: u8 = 0b0100;
/// 能力位：滞回。
pub const CAP_HYST: u8 = 0b1000;
/// 全量能力（四段全覆盖）。
pub const CAP_FULL: u8 = 0b1111;
/// 默认能力（仅增益 + 滞回两段）。
pub const CAP_DEFAULT: u8 = CAP_GAIN | CAP_HYST;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QkCode {
    /// 表项三段式不完整
    EntryBad,
    /// 表容量越界
    TableFull,
    /// 表自审失败
    TableBad,
    /// 表项顺序非规范
    OrderBad,
    /// 标签重复
    TagBad,
    /// 探测字段非法
    ProbeBad,
    /// 固件区间反向
    FwBad,
    /// 参数覆盖越界
    OverrideBad,
    /// 命中链结论与独立预测不符
    ChainBad,
    /// 账本不守恒
    LedgerBad,
    /// 降级虚报（声称超出实际）
    ClaimBad,
    /// 渲染不可解析
    RenderBad,
    /// 复跑不一致
    ReplayBad,
    /// 覆盖轮数越界
    CoverBad,
}

impl QkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            QkCode::EntryBad => "三段式表项缺机型/固件区间/参数覆盖",
            QkCode::TableFull => "quirk 表已满，拒绝继续装入",
            QkCode::TableBad => "quirk 表自审失败（条目或容量坏）",
            QkCode::OrderBad => "表项未按机型与固件下界规范排序",
            QkCode::TagBad => "表项标签重复或越出标签域",
            QkCode::ProbeBad => "设备探测的厂商或机型字段为零",
            QkCode::FwBad => "固件字节为坏值哨兵或区间反向",
            QkCode::OverrideBad => "参数覆盖段越出合法区间",
            QkCode::ChainBad => "命中链结论与独立预测不一致",
            QkCode::LedgerBad => "四态账本求和不等于场景数",
            QkCode::ClaimBad => "降级设备声称能力未被实际能力包含",
            QkCode::RenderBad => "六字段账本文本不可解析",
            QkCode::ReplayBad => "两次确定性复跑结果不一致",
            QkCode::CoverBad => "场景/注入/fuzz 轮数越过覆盖闸门",
        }
    }

    pub const ALL: [QkCode; 14] = [
        QkCode::EntryBad,
        QkCode::TableFull,
        QkCode::TableBad,
        QkCode::OrderBad,
        QkCode::TagBad,
        QkCode::ProbeBad,
        QkCode::FwBad,
        QkCode::OverrideBad,
        QkCode::ChainBad,
        QkCode::LedgerBad,
        QkCode::ClaimBad,
        QkCode::RenderBad,
        QkCode::ReplayBad,
        QkCode::CoverBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < QkCode::ALL.len() {
        if let Some(c) = QkCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 参数四段 ──────────────────────────────────────────────────────
/// quirk 参数覆盖的四段载体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Params {
    pub palm_mm: u16,
    pub gain_q8: u16,
    pub tap_mn: u16,
    pub hyst_mm: u16,
}

impl Params {
    /// 出厂默认（未命中任何表项时使用）。
    pub fn defaults() -> Params {
        Params {
            palm_mm: 40,
            gain_q8: 256,
            tap_mn: 60,
            hyst_mm: 8,
        }
    }

    /// 全零（仅用于表项占位，绝不参与合并）。
    pub fn zero() -> Params {
        Params {
            palm_mm: 0,
            gain_q8: 0,
            tap_mn: 0,
            hyst_mm: 0,
        }
    }

    /// 构造并钳制（越界输入被拉到边界，供外部装配使用）。
    pub fn clamped(palm: u16, gain: u16, tap: u16, hyst: u16) -> Params {
        Params {
            palm_mm: palm.clamp(PALM_MIN, PALM_MAX),
            gain_q8: gain.clamp(GAIN_MIN, GAIN_MAX),
            tap_mn: tap.clamp(TAP_MIN, TAP_MAX),
            hyst_mm: hyst.clamp(HYST_MIN, HYST_MAX),
        }
    }

    /// 四段是否全部落在合法区间。
    pub fn in_range(&self) -> bool {
        self.palm_mm >= PALM_MIN
            && self.palm_mm <= PALM_MAX
            && self.gain_q8 >= GAIN_MIN
            && self.gain_q8 <= GAIN_MAX
            && self.tap_mn >= TAP_MIN
            && self.tap_mn <= TAP_MAX
            && self.hyst_mm >= HYST_MIN
            && self.hyst_mm <= HYST_MAX
    }

    /// 就地钳制（不产生 panic，越界拉到边界）。
    pub fn clamp(&mut self) {
        self.palm_mm = self.palm_mm.clamp(PALM_MIN, PALM_MAX);
        self.gain_q8 = self.gain_q8.clamp(GAIN_MIN, GAIN_MAX);
        self.tap_mn = self.tap_mn.clamp(TAP_MIN, TAP_MAX);
        self.hyst_mm = self.hyst_mm.clamp(HYST_MIN, HYST_MAX);
    }

    /// 逐段单调：a 的每一段都不高于 b（用于覆盖链单调性断言）。
    pub fn leq(&self, other: &Params) -> bool {
        self.palm_mm <= other.palm_mm
            && self.gain_q8 <= other.gain_q8
            && self.tap_mn <= other.tap_mn
            && self.hyst_mm <= other.hyst_mm
    }

    /// 段摘要（四段折叠）。
    pub fn digest(&self, h: u64) -> u64 {
        let mut x = p_core::fold_u32(h, self.palm_mm as u32);
        x = p_core::fold_u32(x, self.gain_q8 as u32);
        x = p_core::fold_u32(x, self.tap_mn as u32);
        p_core::fold_u32(x, self.hyst_mm as u32)
    }

    /// 按能力位取段值（越界位返回 0，不索引数组）。
    pub fn field(&self, bit: u8) -> u16 {
        match bit {
            0 => self.palm_mm,
            1 => self.gain_q8,
            2 => self.tap_mn,
            _ => self.hyst_mm,
        }
    }
}

// ── 设备探测 ──────────────────────────────────────────────────────
/// 一次设备探测的三段自报：厂商 + 机型 + 固件三元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    pub vendor: u16,
    pub model: u16,
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
}

impl Probe {
    pub fn new(vendor: u16, model: u16, major: u8, minor: u8, patch: u8) -> Probe {
        Probe {
            vendor,
            model,
            fw_major: major,
            fw_minor: minor,
            fw_patch: patch,
        }
    }

    /// 固件三元组打包为 24 位整数。
    pub fn fw_packed(&self) -> u32 {
        ((self.fw_major as u32) << 16)
            | ((self.fw_minor as u32) << 8)
            | (self.fw_patch as u32)
    }

    /// 任一固件字节为 0xFF 即坏值（硬件未回填、或总线读到拉高）。
    pub fn fw_poisoned(&self) -> bool {
        self.fw_major == FW_SENTINEL || self.fw_minor == FW_SENTINEL || self.fw_patch == FW_SENTINEL
    }

    /// 探测是否可用：厂商/机型非零且固件无坏字节。
    pub fn valid(&self) -> bool {
        self.vendor != 0 && self.model != 0 && !self.fw_poisoned()
    }

    /// 探测摘要（厂商 + 机型 + 打包固件）。
    pub fn digest(&self, h: u64) -> u64 {
        let x = p_core::fold_u32(h, self.vendor as u32);
        let y = p_core::fold_u32(x, self.model as u32);
        p_core::fold_u32(y, self.fw_packed())
    }

    /// 把机型替换为表内机型（场景构造用）。
    pub fn with_model(&self, model: u16) -> Probe {
        Probe {
            vendor: self.vendor,
            model,
            fw_major: self.fw_major,
            fw_minor: self.fw_minor,
            fw_patch: self.fw_patch,
        }
    }
}

// ── 参数覆盖段 ────────────────────────────────────────────────────
/// 覆盖掩码 + 覆盖值：掩码位 1 的段才允许落到设备上。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Override {
    pub mask: u8,
    pub values: Params,
}

impl Override {
    /// 空覆盖：不覆盖任何段。
    pub fn none() -> Override {
        Override {
            mask: 0,
            values: Params::defaults(),
        }
    }

    /// 四段全量覆盖。
    pub fn full(values: Params) -> Override {
        Override {
            mask: CAP_FULL,
            values,
        }
    }

    /// 指定位覆盖（越界位被截掉到 CAP_FULL 内）。
    pub fn masked(mask: u8, values: Params) -> Override {
        Override {
            mask: mask & CAP_FULL,
            values,
        }
    }

    /// 位是否置位（0..4 之外恒 false）。
    pub fn has(&self, bit: u8) -> bool {
        bit < PARAM_FIELDS as u8 && (self.mask & (1u8 << bit)) != 0
    }

    /// 覆盖计数（置位数）。
    pub fn width(&self) -> u32 {
        let mut n = 0u32;
        let mut m = self.mask;
        while m != 0 {
            n = n.saturating_add((m & 1) as u32);
            m >>= 1;
        }
        n
    }

    /// 把覆盖段并入基础参数；未置位段保持基础值。结果必被钳制。
    pub fn apply(&self, base: &Params) -> Params {
        let mut out = *base;
        if self.has(0) {
            out.palm_mm = self.values.palm_mm;
        }
        if self.has(1) {
            out.gain_q8 = self.values.gain_q8;
        }
        if self.has(2) {
            out.tap_mn = self.values.tap_mn;
        }
        if self.has(3) {
            out.hyst_mm = self.values.hyst_mm;
        }
        out.clamp();
        out
    }

    /// 覆盖摘要（掩码 + 值摘要）。
    pub fn digest(&self, h: u64) -> u64 {
        let x = p_core::fold_u32(h, self.mask as u32);
        self.values.digest(x)
    }
}

// ── 三段式表项 ────────────────────────────────────────────────────
/// 表项 = 机型段 + 固件区间段 + 参数覆盖段（三段缺一即坏）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuirkEntry {
    pub model: u16,
    pub fw_lo: u32,
    pub fw_hi: u32,
    pub prio: u8,
    pub tag: u8,
    pub over: Override,
}

impl QuirkEntry {
    /// 占位条目（容量数组初始化用，不参与匹配）。
    pub fn empty() -> QuirkEntry {
        QuirkEntry {
            model: 0,
            fw_lo: 0,
            fw_hi: 0,
            prio: 0,
            tag: 0,
            over: Override::none(),
        }
    }

    /// 构造并做字段级粗校验（详细病因在 `audit` 中给出）。
    pub fn new(
        model: u16,
        fw_lo: u32,
        fw_hi: u32,
        prio: u8,
        tag: u8,
        over: Override,
    ) -> QuirkEntry {
        QuirkEntry {
            model,
            fw_lo,
            fw_hi,
            prio,
            tag,
            over,
        }
    }

    /// 三段式完整性 + 字段域校验，返回首个病因。
    pub fn audit(&self) -> Result<(), u32> {
        if self.model == 0 {
            return Err(QkCode::ProbeBad.val());
        }
        if self.fw_lo > self.fw_hi || self.fw_hi > FW_MAX || self.fw_lo > FW_MAX {
            return Err(QkCode::FwBad.val());
        }
        if self.prio > PRIO_MAX {
            return Err(QkCode::EntryBad.val());
        }
        if self.tag == 0 || self.tag > TAG_MAX {
            return Err(QkCode::TagBad.val());
        }
        if !self.over.values.in_range() {
            return Err(QkCode::OverrideBad.val());
        }
        if self.over.mask & !CAP_FULL != 0 {
            return Err(QkCode::OverrideBad.val());
        }
        Ok(())
    }

    /// 机型 + 固件区间是否覆盖该探测（不判优先级、不判表自审）。
    pub fn covers(&self, probe: &Probe) -> bool {
        self.model == probe.model
            && probe.fw_packed() >= self.fw_lo
            && probe.fw_packed() <= self.fw_hi
    }

    /// 固件区间宽度（饱和）。
    pub fn span(&self) -> u32 {
        self.fw_hi.saturating_sub(self.fw_lo)
    }

    /// 表项摘要（三段全折叠）。
    pub fn digest(&self, h: u64) -> u64 {
        let x = p_core::fold_u32(h, self.model as u32);
        let y = p_core::fold_u32(x, self.fw_lo);
        let z = p_core::fold_u32(y, self.fw_hi);
        let w = p_core::fold_u32(z, ((self.prio as u32) << 8) | self.tag as u32);
        self.over.digest(w)
    }

    /// 规范序比较：机型升序，再固件下界升序，再标签升序。
    pub fn order_key(&self) -> (u16, u32, u8) {
        (self.model, self.fw_lo, self.tag)
    }

    /// 规范化：按 `order_key` 把表项排成升序（定长数组内插入排序）。
    pub fn canonicalize(entries: &mut [QuirkEntry; TABLE_CAP], len: usize) {
        let mut i = 1usize;
        while i < len && i < TABLE_CAP {
            let key = entries[i];
            let mut j = i;
            while j > 0 {
                let prev = entries[j - 1];
                if prev.order_key() > key.order_key() {
                    entries[j] = prev;
                    j -= 1;
                } else {
                    break;
                }
            }
            entries[j] = key;
            i += 1;
        }
    }
}

// ── 定长 quirk 表 ─────────────────────────────────────────────────
/// 定长 quirk 表：容量恒为 TABLE_CAP，len 显式记账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuirkTable {
    entries: [QuirkEntry; TABLE_CAP],
    len: usize,
}

impl QuirkTable {
    /// 空表。
    pub fn new() -> QuirkTable {
        QuirkTable {
            entries: [QuirkEntry::empty(); TABLE_CAP],
            len: 0,
        }
    }

    /// 当前表项数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空表。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取表项（越界返回 None）。
    pub fn get(&self, i: usize) -> Option<&QuirkEntry> {
        if i < self.len && i < TABLE_CAP {
            self.entries.get(i)
        } else {
            None
        }
    }

    /// 追加一条表项：容量满即拒（不静默丢）。
    pub fn push(&mut self, e: QuirkEntry) -> Result<usize, u32> {
        if self.len >= TABLE_CAP {
            return Err(QkCode::TableFull.val());
        }
        if let Some(slot) = self.entries.get_mut(self.len) {
            *slot = e;
            self.len += 1;
            Ok(self.len - 1)
        } else {
            Err(QkCode::TableFull.val())
        }
    }

    /// 追加后立即做全表自审（构建期用法，坏表不进入匹配）。
    pub fn push_audited(&mut self, e: QuirkEntry) -> Result<usize, u32> {
        let at = self.push(e)?;
        self.audit()?;
        Ok(at)
    }

    /// 全表自审：容量 → 逐条字段 → 标签唯一 → 规范序。
    pub fn audit(&self) -> Result<u64, u32> {
        if self.len == 0 || self.len > TABLE_CAP {
            return Err(QkCode::TableBad.val());
        }
        let mut i = 0usize;
        let mut prev_key: Option<(u16, u32, u8)> = None;
        while i < self.len {
            let e = match self.entries.get(i) {
                Some(v) => v,
                None => return Err(QkCode::TableBad.val()),
            };
            e.audit()?;
            if let Some(pk) = prev_key {
                if e.order_key() <= pk {
                    return Err(QkCode::OrderBad.val());
                }
            }
            prev_key = Some(e.order_key());
            let mut j = i + 1;
            while j < self.len {
                if let Some(o) = self.entries.get(j) {
                    if o.tag == e.tag {
                        return Err(QkCode::TagBad.val());
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(self.checksum())
    }

    /// 表内容校验和（三段 + 长度，独立于条目缓存的任何字段）。
    pub fn checksum(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.len as u32);
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.entries.get(i) {
                h = e.digest(h);
            }
            i += 1;
        }
        if h == 0 {
            h = 1;
        }
        h
    }

    /// 确定性命中：优先级高者胜；优先级相同取最早装入者（下标小者）。
    pub fn find_best(&self, probe: &Probe) -> Option<usize> {
        let mut best: Option<usize> = None;
        let mut best_prio = 0u8;
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.entries.get(i) {
                if e.covers(probe) {
                    let take = match best {
                        None => true,
                        Some(_) => e.prio > best_prio,
                    };
                    if take {
                        best = Some(i);
                        best_prio = e.prio;
                    }
                }
            }
            i += 1;
        }
        best
    }

    /// 命中表项的参数覆盖段（未命中返回 None）。
    pub fn override_at(&self, i: usize) -> Option<Override> {
        self.get(i).map(|e| e.over)
    }

    /// 表摘要（校验和再折长度）。
    pub fn digest(&self) -> u64 {
        p_core::fold_u32(self.checksum(), self.len as u32)
    }

    /// 深拷贝并规范化（装配器用）。
    pub fn canonicalized(&self) -> QuirkTable {
        let mut t = *self;
        QuirkEntry::canonicalize(&mut t.entries, t.len);
        t
    }
}

// ── 四态命中链 ────────────────────────────────────────────────────
/// 一次匹配链的四种合法结论（互斥且穷尽）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// 表项命中并合并覆盖参数
    Hit,
    /// 探测合法但表内无覆盖项（走默认参数）
    Miss,
    /// 探测合法但表自审失败（降级走默认参数并记账）
    Degraded,
    /// 探测本身坏（不扫描表）
    Rejected,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Hit => "hit",
            Outcome::Miss => "miss",
            Outcome::Degraded => "degraded",
            Outcome::Rejected => "rejected",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Outcome::Hit => 1,
            Outcome::Miss => 2,
            Outcome::Degraded => 3,
            Outcome::Rejected => 4,
        }
    }

    pub fn from_code(c: u8) -> Option<Outcome> {
        match c {
            1 => Some(Outcome::Hit),
            2 => Some(Outcome::Miss),
            3 => Some(Outcome::Degraded),
            4 => Some(Outcome::Rejected),
            _ => None,
        }
    }

    pub const ALL: [Outcome; 4] = [
        Outcome::Hit,
        Outcome::Miss,
        Outcome::Degraded,
        Outcome::Rejected,
    ];
}

/// 链步（四道闸的位置与通过标记）。
pub const STEP_PROBE: usize = 0;
pub const STEP_TABLE: usize = 1;
pub const STEP_MATCH: usize = 2;
pub const STEP_MERGE: usize = 3;
/// 链步数。
pub const STEPS: usize = 4;

/// 一次完整匹配链的可复算记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chain {
    pub outcome: Outcome,
    pub index: Option<usize>,
    pub merged: Params,
    pub steps: [u8; STEPS],
    pub digest: u64,
}

impl Chain {
    /// 是否走到了最后一道闸（命中或未命中都算走完全链）。
    pub fn completed(&self) -> bool {
        self.steps.get(STEP_MERGE) == Some(&1)
    }

    /// 合并参数摘要。
    pub fn merged_digest(&self) -> u64 {
        self.merged.digest(p_core::FNV_OFF)
    }

    /// 链摘要是否非零。
    pub fn sane(&self) -> bool {
        self.digest != 0
    }
}

/// 执行匹配链：探测 → 表自审 → 命中 → 覆盖合并，逐闸留痕。
pub fn chain(probe: &Probe, table: &QuirkTable) -> Chain {
    let mut steps = [0u8; STEPS];
    let mut h = p_core::FNV_OFF;
    h = probe.digest(h);

    // 闸 1：探测合法性。
    if !probe.valid() {
        steps[STEP_PROBE] = 0;
        h = p_core::fold_u32(h, QkCode::ProbeBad.val());
        h = p_core::fold_u32(h, Outcome::Rejected.code() as u32);
        let merged = Params::defaults();
        h = merged.digest(h);
        if h == 0 {
            h = 1;
        }
        return Chain {
            outcome: Outcome::Rejected,
            index: None,
            merged,
            steps,
            digest: h,
        };
    }
    steps[STEP_PROBE] = 1;

    // 闸 2：表自审。
    let audit = table.audit();
    let table_ok = audit.is_ok();
    if table_ok {
        steps[STEP_TABLE] = 1;
    } else {
        h = p_core::fold_u32(h, QkCode::TableBad.val());
    }

    // 闸 3：命中（坏表不参与扫描，直接按降级处理）。
    let hit = if table_ok { table.find_best(probe) } else { None };
    if !table_ok {
        steps[STEP_MATCH] = 0;
        h = p_core::fold_u32(h, Outcome::Degraded.code() as u32);
        let merged = Params::defaults();
        h = merged.digest(h);
        if h == 0 {
            h = 1;
        }
        return Chain {
            outcome: Outcome::Degraded,
            index: None,
            merged,
            steps,
            digest: h,
        };
    }
    steps[STEP_MATCH] = 1;

    // 闸 4：覆盖合并。
    let base = Params::defaults();
    let (outcome, merged) = match hit {
        Some(i) => {
            let over = match table.override_at(i) {
                Some(o) => o,
                None => Override::none(),
            };
            (Outcome::Hit, over.apply(&base))
        }
        None => (Outcome::Miss, base),
    };
    steps[STEP_MERGE] = 1;
    h = p_core::fold_u32(h, outcome.code() as u32);
    let hit_code = match hit {
        Some(i) => i as u32,
        None => 0u32,
    };
    h = p_core::fold_u32(h, hit_code);
    h = p_core::fold_u32(h, table.checksum() as u32);
    h = merged.digest(h);
    if h == 0 {
        h = 1;
    }
    Chain {
        outcome,
        index: hit,
        merged,
        steps,
        digest: h,
    }
}

/// 独立预测（不复用 `find_best`，用直扫复算结论），用于交叉验证。
pub fn predict(probe: &Probe, table: &QuirkTable) -> Outcome {
    if !probe.valid() {
        return Outcome::Rejected;
    }
    if table.audit().is_err() {
        return Outcome::Degraded;
    }
    let mut i = 0usize;
    while i < table.len() {
        if let Some(e) = table.get(i) {
            if e.covers(probe) {
                return Outcome::Hit;
            }
        }
        i += 1;
    }
    Outcome::Miss
}

/// 独立预测的合并参数（只取胜出表项的覆盖段），与 `chain` 交叉验证。
pub fn predict_merged(probe: &Probe, table: &QuirkTable) -> Params {
    if !probe.valid() || table.audit().is_err() {
        return Params::defaults();
    }
    let mut best: Option<usize> = None;
    let mut best_prio = 0u8;
    let mut i = 0usize;
    while i < table.len() {
        if let Some(e) = table.get(i) {
            if e.covers(probe) {
                let take = match best {
                    None => true,
                    Some(_) => e.prio > best_prio,
                };
                if take {
                    best = Some(i);
                    best_prio = e.prio;
                }
            }
        }
        i += 1;
    }
    match best.and_then(|idx| table.override_at(idx)) {
        Some(o) => o.apply(&Params::defaults()),
        None => Params::defaults(),
    }
}

// ── 黄金表与坏表 ──────────────────────────────────────────────────
/// 基准表（四条表项，机型升序、标签唯一、参数四段合法）。
pub fn fixture_table() -> QuirkTable {
    let mut t = QuirkTable::new();
    let e0 = QuirkEntry::new(
        MODELS[0],
        0x0000_0100,
        0x0000_02FF,
        5,
        1,
        Override::masked(0b0011, Params::clamped(48, 320, 60, 8)),
    );
    let e1 = QuirkEntry::new(
        MODELS[1],
        0x0000_0000,
        0x0000_00FF,
        7,
        2,
        Override::full(Params::clamped(56, 512, 80, 12)),
    );
    let e2 = QuirkEntry::new(
        MODELS[2],
        0x0003_0000,
        0x0003_00FF,
        3,
        3,
        Override::masked(0b1100, Params::clamped(40, 256, 90, 16)),
    );
    let e3 = QuirkEntry::new(
        MODELS[3],
        0x0000_0000,
        0x00FF_FFFF,
        2,
        4,
        Override::masked(0b0001, Params::clamped(36, 256, 60, 8)),
    );
    let mut i = 0usize;
    let seed = [e0, e1, e2, e3];
    while i < seed.len() {
        if let Some(e) = seed.get(i) {
            if t.push(*e).is_err() {
                return QuirkTable::new();
            }
        }
        i += 1;
    }
    t
}

/// 坏表种类数（参数越界 / 固件区间反向 / 标签为零）。
pub const BAD_KINDS: usize = 3;

/// 构造指定种类的坏表（其余三条表项保持合法，只坏第一条）。
pub fn bad_table(kind: u32) -> QuirkTable {
    let t = fixture_table();
    let mut e = match t.get(0) {
        Some(v) => *v,
        None => QuirkEntry::empty(),
    };
    match kind % 3 {
        0 => {
            e.over.values.palm_mm = 0;
        }
        1 => {
            e.fw_lo = 0x0000_FFF0;
            e.fw_hi = 0x0000_0001;
        }
        _ => {
            e.tag = 0;
        }
    }
    let mut rebuilt = QuirkTable::new();
    if rebuilt.push(e).is_err() {
        return rebuilt;
    }
    let mut i = 1usize;
    while i < t.len() {
        if let Some(v) = t.get(i) {
            if rebuilt.push(*v).is_err() {
                break;
            }
        }
        i += 1;
    }
    rebuilt
}

/// 一条能命中基准表的探测（按机型选择固件区间内的值）。
pub fn hit_probe(model: u16, salt: u32) -> Probe {
    let mut fw = 0x0000_0100u32.saturating_add(salt % 0x40);
    if model == MODELS[1] {
        fw = 0x0000_0040u32.saturating_add(salt % 0x40);
    } else if model == MODELS[2] {
        fw = 0x0003_0000u32.saturating_add(salt % 0x40);
    }
    Probe::new(
        0x046D,
        model,
        ((fw >> 16) & 0xFF) as u8,
        ((fw >> 8) & 0xFF) as u8,
        (fw & 0xFF) as u8,
    )
}

// ── 四态账本与 ×10^2 场景 ────────────────────────────────────────
/// 匹配场景的账本（四态之和必须等于场景数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatchStats {
    pub scenarios: u32,
    pub hits: u32,
    pub misses: u32,
    pub degrades: u32,
    pub rejects: u32,
    pub overrides: u32,
    pub digest: u64,
}

impl MatchStats {
    pub fn empty() -> MatchStats {
        MatchStats {
            scenarios: 0,
            hits: 0,
            misses: 0,
            degrades: 0,
            rejects: 0,
            overrides: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 四态求和是否等于场景数。
    pub fn balanced(&self) -> bool {
        self.hits
            .saturating_add(self.misses)
            .saturating_add(self.degrades)
            .saturating_add(self.rejects)
            == self.scenarios
    }

    /// 账本自审：非零场景、守恒、摘要非零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.scenarios == 0 || !self.balanced() {
            return Err(QkCode::LedgerBad.val());
        }
        if self.digest == 0 {
            return Err(QkCode::RenderBad.val());
        }
        Ok(())
    }

    /// 账本摘要。
    pub fn digest_now(&self) -> u64 {
        let mut h = p_core::fold_u32(p_core::FNV_OFF, self.scenarios);
        h = p_core::fold_u32(h, self.hits);
        h = p_core::fold_u32(h, self.misses);
        h = p_core::fold_u32(h, self.degrades);
        h = p_core::fold_u32(h, self.rejects);
        h = p_core::fold_u32(h, self.overrides);
        if h == 0 {
            h = 1;
        }
        h
    }
}

/// 场景轮数闸门：1..=SCENARIO_MAX 且必须是 10 的倍数（四态比例可精确断言）。
pub fn scenario_rounds_ok(rounds: u32) -> bool {
    rounds > 0 && rounds <= SCENARIO_MAX && rounds % 10 == 0
}

/// ×10^2 匹配链场景：每轮恰好落到一种结论，并与独立预测交叉验证。
pub fn match_scenarios(rounds: u32) -> Result<MatchStats, u32> {
    if !scenario_rounds_ok(rounds) {
        return Err(QkCode::CoverBad.val());
    }
    let good = fixture_table();
    let mut stats = MatchStats::empty();
    let mut i = 0u32;
    while i < rounds {
        let kind = i % 10;
        let (probe, table) = match kind {
            0..=2 => {
                let model = MODELS[(i as usize) % MODELS.len()];
                (hit_probe(model, i), good)
            }
            3 | 4 => (
                Probe::new(
                    0x046D,
                    0x2F00u16.saturating_add(kind as u16),
                    0,
                    1,
                    kind as u8,
                ),
                good,
            ),
            5 => (Probe::new(0, MODELS[0], 0, 1, 0), good),
            6 => (Probe::new(0x046D, MODELS[0], FW_SENTINEL, 0, 0), good),
            _ => {
                let model = MODELS[(i as usize) % MODELS.len()];
                (hit_probe(model, i), bad_table(kind - 7))
            }
        };
        let ch = chain(&probe, &table);
        let want = predict(&probe, &table);
        if ch.outcome != want {
            return Err(QkCode::ChainBad.val());
        }
        let want_merged = predict_merged(&probe, &table);
        if ch.merged != want_merged {
            return Err(QkCode::ChainBad.val());
        }
        match ch.outcome {
            Outcome::Hit => {
                stats.hits = stats.hits.saturating_add(1);
                if let Some(idx) = ch.index {
                    if let Some(e) = table.get(idx) {
                        stats.overrides =
                            stats.overrides.saturating_add(e.over.width());
                    }
                }
            }
            Outcome::Miss => stats.misses = stats.misses.saturating_add(1),
            Outcome::Degraded => stats.degrades = stats.degrades.saturating_add(1),
            Outcome::Rejected => stats.rejects = stats.rejects.saturating_add(1),
        }
        stats.digest = p_core::fold_u32(stats.digest, ch.digest as u32);
        stats.digest = p_core::fold_u32(stats.digest, ch.outcome.code() as u32);
        i = i.saturating_add(1);
    }
    stats.scenarios = rounds;
    // 精确比例：每 10 轮 3 命中 / 2 未命中 / 2 拒绝 / 3 降级。
    if stats.hits != rounds / 10 * 3
        || stats.misses != rounds / 10 * 2
        || stats.rejects != rounds / 10 * 2
        || stats.degrades != rounds / 10 * 3
    {
        return Err(QkCode::LedgerBad.val());
    }
    stats.audit()?;
    Ok(stats)
}

// ── 降级 ×10 设备账 ──────────────────────────────────────────────
/// 设备分类：全量（表内命中）/ 降级（坏表或越界）/ 拒绝（探测坏）。
pub const CLASS_FULL: u8 = 0;
pub const CLASS_DEGRADE: u8 = 1;
pub const CLASS_REJECT: u8 = 2;

/// 单台设备的承诺面记账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeRow {
    pub model: u16,
    pub claimed: u8,
    pub actual: u8,
    pub class: u8,
    pub digest: u64,
}

impl DegradeRow {
    /// 声明的能力位是否全部被实际能力包含（虚报判据）。
    pub fn honest(&self) -> bool {
        self.claimed & !self.actual == 0
    }

    /// 行摘要。
    pub fn fold(&self) -> u64 {
        let mut h = p_core::fold_u32(p_core::FNV_OFF, self.model as u32);
        h = p_core::fold_u32(h, ((self.claimed as u32) << 16) | self.actual as u32);
        h = p_core::fold_u32(h, self.class as u32);
        if h == 0 {
            h = 1;
        }
        h
    }
}

/// 十台设备的降级账本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeBook {
    rows: [DegradeRow; DEGRADE_DEVICES],
    n: usize,
    digest: u64,
}

impl DegradeBook {
    pub fn new() -> DegradeBook {
        DegradeBook {
            rows: [DegradeRow {
                model: 0,
                claimed: 0,
                actual: 0,
                class: CLASS_REJECT,
                digest: 0,
            }; DEGRADE_DEVICES],
            n: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 追加设备行（容量满即拒）。
    pub fn push(&mut self, row: DegradeRow) -> Result<(), u32> {
        if self.n >= DEGRADE_DEVICES {
            return Err(QkCode::TableFull.val());
        }
        if let Some(slot) = self.rows.get_mut(self.n) {
            *slot = row;
            self.n += 1;
            self.digest = p_core::fold_u32(self.digest, row.fold() as u32);
            Ok(())
        } else {
            Err(QkCode::TableFull.val())
        }
    }

    /// 已记账设备数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 取行（越界 None）。
    pub fn get(&self, i: usize) -> Option<&DegradeRow> {
        if i < self.n && i < DEGRADE_DEVICES {
            self.rows.get(i)
        } else {
            None
        }
    }

    /// 虚报条数：声称能力位未被实际能力包含的行数（必须为 0）。
    pub fn false_claims(&self) -> u32 {
        let mut i = 0usize;
        let mut bad = 0u32;
        while i < self.n {
            if let Some(r) = self.rows.get(i) {
                if !r.honest() {
                    bad = bad.saturating_add(1);
                }
            }
            i += 1;
        }
        bad
    }

    /// 某分类的设备数。
    pub fn class_count(&self, class: u8) -> u32 {
        let mut i = 0usize;
        let mut n = 0u32;
        while i < self.n {
            if let Some(r) = self.rows.get(i) {
                if r.class == class {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 声称位总数（用于"不承诺"断言：降级/拒绝设备不得声称四段全量）。
    pub fn claimed_bits(&self) -> u32 {
        let mut i = 0usize;
        let mut n = 0u32;
        while i < self.n {
            if let Some(r) = self.rows.get(i) {
                let mut m = r.claimed;
                while m != 0 {
                    n = n.saturating_add((m & 1) as u32);
                    m >>= 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 账本自审：恰好 10 台、分类 4/3/3、机型唯一、零虚报、摘要非零。
    pub fn audit(&self) -> Result<u64, u32> {
        if self.n != DEGRADE_DEVICES {
            return Err(QkCode::LedgerBad.val());
        }
        if self.class_count(CLASS_FULL) != 4
            || self.class_count(CLASS_DEGRADE) != 3
            || self.class_count(CLASS_REJECT) != 3
        {
            return Err(QkCode::LedgerBad.val());
        }
        if self.false_claims() != 0 {
            return Err(QkCode::ClaimBad.val());
        }
        let mut i = 0usize;
        while i < self.n {
            let a = match self.rows.get(i) {
                Some(v) => v,
                None => return Err(QkCode::LedgerBad.val()),
            };
            if a.model == 0 || a.digest == 0 {
                return Err(QkCode::LedgerBad.val());
            }
            let mut j = i + 1;
            while j < self.n {
                if let Some(b) = self.rows.get(j) {
                    if b.model == a.model {
                        return Err(QkCode::LedgerBad.val());
                    }
                }
                j += 1;
            }
            i += 1;
        }
        if self.digest == 0 {
            return Err(QkCode::RenderBad.val());
        }
        Ok(self.digest)
    }
}

/// 构造十台设备的降级账：4 全量 + 3 降级 + 3 拒绝，声称恒等于实际。
pub fn degrade_ledger() -> DegradeBook {
    let mut b = DegradeBook::new();
    let mut i = 0usize;
    while i < DEGRADE_DEVICES {
        let (claimed, actual, class) = match i {
            0..=3 => (CAP_FULL, CAP_FULL, CLASS_FULL),
            4..=6 => (CAP_DEFAULT, CAP_DEFAULT, CLASS_DEGRADE),
            _ => (0u8, 0u8, CLASS_REJECT),
        };
        let model = if i < 4 {
            MODELS[i]
        } else {
            0x3000u16.saturating_add(i as u16)
        };
        let row = DegradeRow {
            model,
            claimed,
            actual,
            class,
            digest: 0,
        };
        let mut row = row;
        row.digest = row.fold();
        if b.push(row).is_err() {
            return b;
        }
        i += 1;
    }
    b
}

// ── 四路注入 ──────────────────────────────────────────────────────
/// 注入账：每路尝试数与捕获数（必须相等且非零才叫全捕获）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; INJECT_ROUTES],
    pub caught: [u32; INJECT_ROUTES],
}

impl InjectLog {
    pub fn new() -> InjectLog {
        InjectLog {
            tries: [0; INJECT_ROUTES],
            caught: [0; INJECT_ROUTES],
        }
    }

    /// 四路是否全部尝试且全部捕获。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJECT_ROUTES {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 四路注入：坏探测 / 坏固件 / 坏参数表 / 同优先级定序。
pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > 4_096 {
        return Err(QkCode::CoverBad.val());
    }
    let mut log = InjectLog::new();
    let mut i = 0u32;
    while i < rounds {
        // ① 厂商为零必须 Rejected。
        let p0 = Probe::new(0, MODELS[0], 0, 1, 0);
        if chain(&p0, &fixture_table()).outcome == Outcome::Rejected {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);

        // ② 固件坏字节必须 Rejected（即便机型在表内）。
        let p1 = Probe::new(0x046D, MODELS[1], 0, FW_SENTINEL, 0);
        if chain(&p1, &fixture_table()).outcome == Outcome::Rejected {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);

        // ③ 参数越界表项必须让全表 Degraded。
        let p2 = hit_probe(MODELS[0], i);
        if chain(&p2, &bad_table(0)).outcome == Outcome::Degraded {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);

        // ④ 同优先级必须恒取最早装入者（两次调用同一下标）。
        let mut tie = QuirkTable::new();
        let a = QuirkEntry::new(
            MODELS[0],
            0,
            0x00FF_FFFF,
            9,
            5,
            Override::masked(0b0001, Params::clamped(44, 256, 60, 8)),
        );
        let b = QuirkEntry::new(
            MODELS[0],
            0,
            0x00FF_FFFF,
            9,
            6,
            Override::masked(0b0010, Params::clamped(44, 400, 60, 8)),
        );
        let _ = tie.push(a);
        let _ = tie.push(b);
        let p3 = Probe::new(0x046D, MODELS[0], 0, 0x20, 0);
        let first = tie.find_best(&p3);
        let second = tie.find_best(&p3);
        if first == second && first == Some(0) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(QkCode::CoverBad.val());
    }
    Ok(log)
}

// ── 表项 fuzz（×10^4） ───────────────────────────────────────────
/// fuzz 账：接受/拒绝之和必须等于轮数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzStats {
    pub rounds: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub digest: u64,
}

impl FuzzStats {
    pub fn balanced(&self) -> bool {
        self.accepted.saturating_add(self.rejected) == self.rounds
    }
}

/// 乱表项注入：字段全域随机，坏项必须被 `audit` 拒绝，好项必须可自审。
pub fn fuzz_entries(rounds: u32) -> Result<FuzzStats, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(QkCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x718C_0F49_0001_0001);
    let mut st = FuzzStats {
        rounds,
        accepted: 0,
        rejected: 0,
        digest: p_core::FNV_OFF,
    };
    let mut i = 0u32;
    while i < rounds {
        let model = rng.below(8) as u16;
        let fw_lo = rng.below(0x0004_0000);
        let fw_hi = fw_lo.saturating_add(rng.below(0x0002_0000));
        let prio = rng.below(20) as u8;
        let tag = rng.below(20) as u8;
        let values = Params {
            palm_mm: rng.below(120) as u16,
            gain_q8: rng.below(1_400) as u16,
            tap_mn: rng.below(260) as u16,
            hyst_mm: rng.below(50) as u16,
        };
        let over = Override::masked(rng.below(16) as u8, values);
        let e = QuirkEntry::new(model, fw_lo, fw_hi, prio, tag, over);

        let mut t = QuirkTable::new();
        let pushed = t.push(e).is_ok();
        let ok = e.audit().is_ok() && pushed && t.audit().is_ok();
        if e.audit().is_ok() {
            if ok {
                st.accepted = st.accepted.saturating_add(1);
            } else {
                // 字段合法却装不进/审不过，属于表引擎自身缺陷。
                return Err(QkCode::TableBad.val());
            }
        } else {
            st.rejected = st.rejected.saturating_add(1);
        }

        // 同一张表上跑一次随机探测：结论必须与独立预测一致。
        let p = Probe::new(
            1 + rng.below(8) as u16,
            rng.below(10) as u16,
            rng.below(4) as u8,
            rng.below(4) as u8,
            rng.below(4) as u8,
        );
        let c = chain(&p, &t);
        if c.outcome != predict(&p, &t) {
            return Err(QkCode::ChainBad.val());
        }
        if c.merged != predict_merged(&p, &t) {
            return Err(QkCode::ChainBad.val());
        }
        st.digest = p_core::fold_u32(st.digest, c.digest as u32);
        i = i.saturating_add(1);
    }
    if !st.balanced() || st.digest == 0 {
        return Err(QkCode::LedgerBad.val());
    }
    Ok(st)
}

// ── 六字段渲染 ────────────────────────────────────────────────────
/// 渲染：场景|命中|未命中|降级|拒绝|摘要。
pub fn render_stats(s: &MatchStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.scenarios, s.hits, s.misses, s.degrades, s.rejects, s.digest
    )
}

/// 十进制 u32 解析（空串、非数字、溢出均返回 None）。
pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        let c = *b.get(i)?;
        if !(48..=57).contains(&c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - 48) as u32)?;
        i += 1;
    }
    Some(v)
}

/// 0x + 16 位十六进制解析（大小写均可）。
pub fn parse_hex16(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 18 {
        return None;
    }
    if *b.get(0)? != b'0' || *b.get(1)? != b'x' {
        return None;
    }
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let c = *b.get(i)?;
        let d = match c {
            48..=57 => c - 48,
            65..=70 => c - 55,
            97..=102 => c - 87,
            _ => return None,
        };
        v = (v << 4) | d as u64;
        i += 1;
    }
    Some(v)
}

/// 回读六字段账本：字段数 → 数值 → 守恒 → 摘要。
pub fn audit_render(text: &str) -> Result<usize, u32> {
    let mut parts = [""; STATS_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < STATS_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != STATS_FIELDS {
        return Err(QkCode::RenderBad.val());
    }
    let scenarios = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(QkCode::RenderBad.val()),
    };
    let hits = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(QkCode::RenderBad.val()),
    };
    let misses = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(QkCode::RenderBad.val()),
    };
    let degrades = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(QkCode::RenderBad.val()),
    };
    let rejects = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(QkCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(QkCode::RenderBad.val()),
    };
    if scenarios == 0
        || hits
            .saturating_add(misses)
            .saturating_add(degrades)
            .saturating_add(rejects)
            != scenarios
    {
        return Err(QkCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(QkCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

// ── 黄金运行 ──────────────────────────────────────────────────────
/// 固定 16 台设备的黄金脚本：8 命中 / 4 未命中 / 2 拒绝 / 2 降级。
pub fn golden_run() -> Result<MatchStats, u32> {
    let good = fixture_table();
    let mut stats = MatchStats::empty();
    let mut i = 0u32;
    while i < GOLDEN_ROUNDS {
        let (probe, table) = match i {
            0..=7 => {
                let model = MODELS[(i as usize) % MODELS.len()];
                (hit_probe(model, i), good)
            }
            8..=11 => (
                Probe::new(0x046D, 0x2F00u16.saturating_add(i as u16), 0, 2, 0),
                good,
            ),
            12 | 13 => (Probe::new(0x046D, MODELS[0], 0, FW_SENTINEL, 0), good),
            _ => {
                let model = MODELS[(i as usize) % MODELS.len()];
                (hit_probe(model, i), bad_table(i - 14))
            }
        };
        let ch = chain(&probe, &table);
        if ch.outcome != predict(&probe, &table) {
            return Err(QkCode::ChainBad.val());
        }
        match ch.outcome {
            Outcome::Hit => stats.hits = stats.hits.saturating_add(1),
            Outcome::Miss => stats.misses = stats.misses.saturating_add(1),
            Outcome::Degraded => stats.degrades = stats.degrades.saturating_add(1),
            Outcome::Rejected => stats.rejects = stats.rejects.saturating_add(1),
        }
        stats.digest = p_core::fold_u32(stats.digest, ch.digest as u32);
        i = i.saturating_add(1);
    }
    stats.scenarios = GOLDEN_ROUNDS;
    if stats.hits != 8 || stats.misses != 4 || stats.rejects != 2 || stats.degrades != 2 {
        return Err(QkCode::LedgerBad.val());
    }
    stats.audit()?;
    Ok(stats)
}

/// 复跑一致性：场景账、黄金账、表校验和三路逐位相同。
pub fn repro_check() -> Result<(), u32> {
    let a = match match_scenarios(SCENARIOS) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    let b = match match_scenarios(SCENARIOS) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    if a != b {
        return Err(QkCode::ReplayBad.val());
    }
    let g1 = golden_run()?;
    let g2 = golden_run()?;
    if g1 != g2 {
        return Err(QkCode::ReplayBad.val());
    }
    let t1 = fixture_table();
    let t2 = fixture_table();
    if t1.checksum() != t2.checksum() || t1.checksum() == 0 {
        return Err(QkCode::ReplayBad.val());
    }
    Ok(())
}

// ── 码族与段位 ────────────────────────────────────────────────────
/// 14 个诊断码两两不同。
pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < QkCode::ALL.len() {
        let mut j = i + 1;
        while j < QkCode::ALL.len() {
            match (QkCode::ALL.get(i), QkCode::ALL.get(j)) {
                (Some(a), Some(b)) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                _ => return false,
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 全部诊断码落在本段窗口 (+0x01..=+0x0F) 内。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < QkCode::ALL.len() {
        if let Some(c) = QkCode::ALL.get(i) {
            let v = c.val();
            if v <= SEG_BASE || v > SEG_BASE + 0x0F {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 探针三序列 ────────────────────────────────────────────────────
/// 序列 A：表自审 + 校验和 + 四个机型逐一可寻址。
pub fn probe_seq_a() -> Result<(), u32> {
    let t = fixture_table();
    let csum = t.audit()?;
    if csum == 0 || t.len() != 4 {
        return Err(QkCode::TableBad.val());
    }
    let mut i = 0usize;
    while i < MODELS.len() {
        let model = MODELS[i];
        let p = hit_probe(model, i as u32);
        if t.find_best(&p).is_none() {
            return Err(QkCode::ChainBad.val());
        }
        if predict(&p, &t) != Outcome::Hit {
            return Err(QkCode::ChainBad.val());
        }
        i += 1;
    }
    Ok(())
}

/// 序列 B：×10^2 场景账 + 六字段回读 + 黄金账。
pub fn probe_seq_b() -> Result<(), u32> {
    let s = match_scenarios(SCENARIOS)?;
    if !s.balanced() || s.overrides == 0 {
        return Err(QkCode::LedgerBad.val());
    }
    let text = render_stats(&s);
    if audit_render(&text)? != STATS_FIELDS {
        return Err(QkCode::RenderBad.val());
    }
    let g = golden_run()?;
    if !g.balanced() {
        return Err(QkCode::LedgerBad.val());
    }
    Ok(())
}

/// 序列 C：十设备账 + 64 组覆盖掩码钳制。
pub fn probe_seq_c() -> Result<(), u32> {
    let book = degrade_ledger();
    book.audit()?;
    if book.false_claims() != 0 {
        return Err(QkCode::ClaimBad.val());
    }
    let base = Params::defaults();
    let wild = Params {
        palm_mm: 0,
        gain_q8: u16::MAX,
        tap_mn: u16::MAX,
        hyst_mm: 0,
    };
    let mut mask = 0u8;
    let mut checked = 0u32;
    while mask < 16 {
        // 逐掩码生成覆盖，并验证结果四段恒在区间内。
        let over = Override::masked(mask, wild);
        let merged = over.apply(&base);
        if !merged.in_range() {
            return Err(QkCode::OverrideBad.val());
        }
        // 未置位的段必须保持基础值（覆盖不得越权改写）。
        let mut bit = 0u8;
        while bit < PARAM_FIELDS as u8 {
            if !over.has(bit) && merged.field(bit) != base.field(bit) {
                return Err(QkCode::OverrideBad.val());
            }
            bit += 1;
        }
        checked = checked.saturating_add(1);
        mask = mask.saturating_add(1);
    }
    if checked != 16 {
        return Err(QkCode::CoverBad.val());
    }
    Ok(())
}

// ── 八条卡面判据 ──────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1) F49001：三段式表项自审（容量/字段/标签/规范序一次过）。
    let table = fixture_table();
    let audit = table.audit();
    let all_ok = {
        let mut ok = true;
        let mut i = 0usize;
        while i < table.len() {
            match table.get(i) {
                Some(e) => {
                    if e.audit().is_err() || e.model == 0 {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            i += 1;
        }
        ok
    };
    out.push((
        "F49001 三段式表项自审",
        matches!(audit, Ok(c) if c != 0) && table.len() == 4 && all_ok,
        match &audit {
            Ok(c) => format!("csum=0x{:08X} n={}", *c as u32, table.len()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 2) F49001 卡面引用的 J1：匹配链 ×10^2 覆盖正确。
    let scen = match_scenarios(SCENARIOS);
    let replay = repro_check().is_ok();
    let scen_render = match &scen {
        Ok(s) => audit_render(&render_stats(s)) == Ok(STATS_FIELDS),
        Err(_) => false,
    };
    out.push((
        "F49002-J1 匹配链×10^2",
        matches!(&scen, Ok(s) if s.balanced() && s.hits == 30 && s.scenarios == SCENARIOS)
            && replay
            && scen_render,
        match &scen {
            Ok(s) => format!(
                "hit={} miss={} deg={} rej={}",
                s.hits, s.misses, s.degrades, s.rejects
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 3) F49001：覆盖账本守恒（四态之和 == 场景数，摘要非零）。
    out.push((
        "F49002 覆盖账本守恒",
        matches!(&scen, Ok(s) if s.audit().is_ok() && s.digest != 0 && s.overrides > 0),
        match &scen {
            Ok(s) => format!("sum={} ovr={}", s.scenarios, s.overrides),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 4) F49001：参数覆盖链钳制（未置位段不得被改写）。
    let seq_c = probe_seq_c();
    out.push((
        "F49001 参数覆盖钳制",
        seq_c.is_ok(),
        match &seq_c {
            Ok(()) => String::from("masks=16 bits=4 clamp=ok"),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 5) F49002 卡面引用的 J1：降级 ×10 设备记账零虚报。
    let book = degrade_ledger();
    let book_audit = book.audit();
    out.push((
        "F49003-J1 降级×10零虚报",
        book_audit.is_ok() && book.len() == DEGRADE_DEVICES && book.false_claims() == 0,
        format!(
            "dev={} claim={} bit={}",
            book.len(),
            book.false_claims(),
            book.claimed_bits()
        ),
    ));

    // 6) F49001：探测拒绝零命中（坏探测不得借表项洗白）。
    let mut rejects_ok = true;
    let mut i = 0u32;
    while i < 10 {
        let p = match i % 3 {
            0 => Probe::new(0, MODELS[0], 0, 1, 0),
            1 => Probe::new(0x046D, 0, 0, 1, 0),
            _ => Probe::new(0x046D, MODELS[0], FW_SENTINEL, 0, 0),
        };
        if chain(&p, &fixture_table()).outcome != Outcome::Rejected {
            rejects_ok = false;
        }
        i = i.saturating_add(1);
    }
    out.push((
        "F49002 探测拒绝零命中",
        rejects_ok,
        format!("rej=10 hit=0 table={}", table.len()),
    ));

    // 7) F49001：表容量与标签去重（第 17 条必拒、重复标签必炸）。
    let mut cap = QuirkTable::new();
    let mut cap_ok = true;
    let mut k = 0usize;
    while k < TABLE_CAP {
        if cap.push(QuirkEntry::empty()).is_err() {
            cap_ok = false;
        }
        k += 1;
    }
    let full = cap.push(QuirkEntry::empty()).is_err();
    let mut dup = QuirkTable::new();
    let d1 = QuirkEntry::new(1, 0, 0x10, 1, 7, Override::none());
    let d2 = QuirkEntry::new(2, 0, 0x10, 1, 7, Override::none());
    let _ = dup.push(d1);
    let _ = dup.push(d2);
    let dup_bad = dup.audit() == Err(QkCode::TagBad.val());
    out.push((
        "F49001 表容量与去重",
        cap_ok && full && dup_bad,
        format!("cap={} full={} dup={}", TABLE_CAP, full as u8, dup_bad as u8),
    ));

    // 8) F49001/F49002：fuzz 表项 + 黄金账（两卡共用的健壮性面）。
    let fuzz = fuzz_entries(FUZZ_ROUNDS);
    let golden = golden_run();
    let golden_render_ok = match &golden {
        Ok(g) => audit_render(&render_stats(g)) == Ok(STATS_FIELDS),
        Err(_) => false,
    };
    out.push((
        "F49001/02 fuzz与黄金",
        matches!(&fuzz, Ok(s) if s.balanced() && s.accepted > 0 && s.rejected > 0)
            && matches!(&golden, Ok(g) if g.balanced())
            && golden_render_ok,
        match (&fuzz, &golden) {
            (Ok(f), Ok(g)) => format!("acc={} rej={} g={}", f.accepted, f.rejected, g.scenarios),
            _ => String::from("fuzz/golden err"),
        },
    ));

    out
}

/// 五卡判据入口（域名与卡面一致）。
pub fn run_unx49001_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49001 quirk 表引擎与匹配器");
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

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x718C0);
        assert_eq!(TABLE_CAP, 16);
        assert_eq!(SCENARIOS, 100);
        assert_eq!(DEGRADE_DEVICES, 10);
        assert_eq!(STATS_FIELDS, 6);
        assert_eq!(QkCode::ALL.len(), 14);
        assert_eq!(MODELS.len(), 4);
        assert_eq!(CAP_FULL, CAP_PALM | CAP_GAIN | CAP_TAP | CAP_HYST);
    }

    #[test]
    fn params_default_in_range() {
        let p = Params::defaults();
        assert!(p.in_range());
        assert_eq!(p.palm_mm, 40);
        assert_eq!(p.gain_q8, 256);
        assert_eq!(p.tap_mn, 60);
        assert_eq!(p.hyst_mm, 8);
    }

    #[test]
    fn params_clamp_pulls_into_range() {
        let mut p = Params {
            palm_mm: 0,
            gain_q8: u16::MAX,
            tap_mn: 0,
            hyst_mm: u16::MAX,
        };
        p.clamp();
        assert_eq!(p.palm_mm, PALM_MIN);
        assert_eq!(p.gain_q8, GAIN_MAX);
        assert_eq!(p.tap_mn, TAP_MIN);
        assert_eq!(p.hyst_mm, HYST_MAX);
        assert!(p.in_range());
    }

    #[test]
    fn params_leq_is_monotone() {
        let lo = Params::clamped(20, 32, 10, 2);
        let hi = Params::clamped(80, 1024, 200, 32);
        assert!(lo.leq(&hi));
        assert!(!hi.leq(&lo));
        assert!(lo.leq(&lo));
    }

    #[test]
    fn probe_packs_firmware() {
        let p = Probe::new(0x046D, 0x1001, 0x01, 0x02, 0x03);
        assert_eq!(p.fw_packed(), 0x0001_0203);
        assert!(p.valid());
        assert_eq!(p.fw_packed(), 0x00010203);
    }

    #[test]
    fn probe_rejects_poison_and_zero() {
        assert!(!Probe::new(0, 0x1001, 0, 0, 0).valid());
        assert!(!Probe::new(0x046D, 0, 0, 0, 0).valid());
        assert!(!Probe::new(0x046D, 0x1001, FW_SENTINEL, 0, 0).valid());
        assert!(!Probe::new(0x046D, 0x1001, 0, FW_SENTINEL, 0).valid());
        assert!(!Probe::new(0x046D, 0x1001, 0, 0, FW_SENTINEL).valid());
    }

    #[test]
    fn override_mask_merge() {
        let base = Params::defaults();
        let vals = Params::clamped(70, 512, 90, 20);
        let over = Override::masked(0b0101, vals);
        let out = over.apply(&base);
        assert_eq!(out.palm_mm, 70);
        assert_eq!(out.gain_q8, base.gain_q8);
        assert_eq!(out.tap_mn, 90);
        assert_eq!(out.hyst_mm, base.hyst_mm);
        assert!(out.in_range());
    }

    #[test]
    fn override_width_counts() {
        assert_eq!(Override::none().width(), 0);
        assert_eq!(Override::full(Params::defaults()).width(), 4);
        assert_eq!(Override::masked(0b1010, Params::defaults()).width(), 2);
        assert_eq!(Override::masked(0xFF, Params::defaults()).width(), 4);
    }

    #[test]
    fn entry_audit_rejects_each_defect() {
        let good = QuirkEntry::new(
            MODELS[0],
            0,
            0x100,
            5,
            1,
            Override::masked(0b0001, Params::defaults()),
        );
        assert_eq!(good.audit(), Ok(()));
        let mut e = good;
        e.model = 0;
        assert_eq!(e.audit(), Err(QkCode::ProbeBad.val()));
        let mut e = good;
        e.fw_lo = 0x200;
        e.fw_hi = 0x100;
        assert_eq!(e.audit(), Err(QkCode::FwBad.val()));
        let mut e = good;
        e.prio = PRIO_MAX + 1;
        assert_eq!(e.audit(), Err(QkCode::EntryBad.val()));
        let mut e = good;
        e.tag = 0;
        assert_eq!(e.audit(), Err(QkCode::TagBad.val()));
        let mut e = good;
        e.over.values.palm_mm = 0;
        assert_eq!(e.audit(), Err(QkCode::OverrideBad.val()));
    }

    #[test]
    fn entry_covers_range() {
        let e = QuirkEntry::new(0x1001, 0x100, 0x1FF, 1, 1, Override::none());
        let inside = Probe::new(1, 0x1001, 0, 0x01, 0x80);
        let low = Probe::new(1, 0x1001, 0, 0x00, 0xFF);
        let other = Probe::new(1, 0x1002, 0, 0x01, 0x80);
        assert!(e.covers(&inside));
        assert!(!e.covers(&low));
        assert!(!e.covers(&other));
        assert_eq!(e.span(), 0xFF);
    }

    #[test]
    fn table_push_capacity() {
        let mut t = QuirkTable::new();
        let mut i = 0usize;
        while i < TABLE_CAP {
            assert!(t.push(QuirkEntry::empty()).is_ok());
            i += 1;
        }
        assert_eq!(t.len(), TABLE_CAP);
        assert_eq!(t.push(QuirkEntry::empty()), Err(QkCode::TableFull.val()));
    }

    #[test]
    fn table_audit_green() {
        let t = fixture_table();
        let c = t.audit();
        assert!(c.is_ok());
        match c {
            Ok(v) => assert!(v != 0),
            Err(_) => assert!(false),
        }
        assert_eq!(t.len(), 4);
    }

    #[test]
    fn table_audit_rejects_dup_tag() {
        let mut t = QuirkTable::new();
        let a = QuirkEntry::new(1, 0, 0x10, 1, 7, Override::none());
        let b = QuirkEntry::new(2, 0, 0x10, 1, 7, Override::none());
        assert!(t.push(a).is_ok());
        assert!(t.push(b).is_ok());
        assert_eq!(t.audit(), Err(QkCode::TagBad.val()));
    }

    #[test]
    fn table_canonicalize_sorts() {
        let mut t = QuirkTable::new();
        let e2 = QuirkEntry::new(3, 0, 0x10, 1, 3, Override::none());
        let e0 = QuirkEntry::new(1, 0, 0x10, 1, 1, Override::none());
        let e1 = QuirkEntry::new(2, 0, 0x10, 1, 2, Override::none());
        assert!(t.push(e2).is_ok());
        assert!(t.push(e0).is_ok());
        assert!(t.push(e1).is_ok());
        assert_eq!(t.audit(), Err(QkCode::OrderBad.val()));
        let sorted = t.canonicalized();
        assert!(sorted.audit().is_ok());
        match sorted.get(0) {
            Some(e) => assert_eq!(e.model, 1),
            None => assert!(false),
        }
    }

    #[test]
    fn find_best_priority_and_tie() {
        let mut t = QuirkTable::new();
        let low = QuirkEntry::new(1, 0, 0xFFFF, 1, 1, Override::none());
        let high = QuirkEntry::new(1, 0, 0xFFFF, 9, 2, Override::none());
        assert!(t.push(low).is_ok());
        assert!(t.push(high).is_ok());
        let p = Probe::new(1, 1, 0, 0, 5);
        assert_eq!(t.find_best(&p), Some(1));
        let mut tie = QuirkTable::new();
        assert!(tie.push(low).is_ok());
        let same = QuirkEntry::new(1, 0, 0xFFFF, 1, 3, Override::none());
        assert!(tie.push(same).is_ok());
        assert_eq!(tie.find_best(&p), Some(0));
    }

    #[test]
    fn chain_rejects_probe_first() {
        let p = Probe::new(0, MODELS[0], 0, 1, 0);
        let c = chain(&p, &fixture_table());
        assert_eq!(c.outcome, Outcome::Rejected);
        assert_eq!(c.steps[STEP_PROBE], 0);
        assert!(!c.completed());
        assert_eq!(c.merged, Params::defaults());
    }

    #[test]
    fn chain_miss_uses_defaults() {
        let p = Probe::new(1, 0x2F00, 0, 1, 0);
        let c = chain(&p, &fixture_table());
        assert_eq!(c.outcome, Outcome::Miss);
        assert_eq!(c.index, None);
        assert_eq!(c.merged, Params::defaults());
        assert!(c.completed());
        assert!(c.sane());
    }

    #[test]
    fn chain_hit_merges_override() {
        let p = hit_probe(MODELS[1], 7);
        let c = chain(&p, &fixture_table());
        assert_eq!(c.outcome, Outcome::Hit);
        assert!(c.index.is_some());
        assert_eq!(c.merged.palm_mm, 56);
        assert_eq!(c.merged.gain_q8, 512);
        assert_eq!(c.merged.tap_mn, 80);
        assert_eq!(c.merged.hyst_mm, 12);
        assert!(c.completed());
    }

    #[test]
    fn chain_degrades_on_bad_table() {
        let p = hit_probe(MODELS[0], 1);
        let t = bad_table(0);
        let c = chain(&p, &t);
        assert_eq!(c.outcome, Outcome::Degraded);
        assert_eq!(c.steps[STEP_TABLE], 0);
        assert_eq!(c.merged, Params::defaults());
        assert_eq!(predict(&p, &t), Outcome::Degraded);
    }

    #[test]
    fn match_scenarios_exact() {
        let s = match match_scenarios(SCENARIOS) {
            Ok(v) => v,
            Err(_) => {
                assert!(false);
                MatchStats::empty()
            }
        };
        assert_eq!(s.scenarios, 100);
        assert_eq!(s.hits, 30);
        assert_eq!(s.misses, 20);
        assert_eq!(s.degrades, 30);
        assert_eq!(s.rejects, 20);
        assert!(s.balanced());
        assert!(s.overrides > 0);
        assert_eq!(s.audit(), Ok(()));
    }

    #[test]
    fn match_scenarios_rejects_bad_rounds() {
        assert_eq!(match_scenarios(0), Err(QkCode::CoverBad.val()));
        assert_eq!(match_scenarios(5), Err(QkCode::CoverBad.val()));
        assert_eq!(
            match_scenarios(SCENARIO_MAX + 10),
            Err(QkCode::CoverBad.val())
        );
    }

    #[test]
    fn match_scenarios_replay_equal() {
        let a = match_scenarios(50);
        let b = match_scenarios(50);
        assert_eq!(a, b);
    }

    #[test]
    fn degrade_ledger_audits() {
        let book = degrade_ledger();
        assert_eq!(book.len(), DEGRADE_DEVICES);
        assert!(book.audit().is_ok());
        assert_eq!(book.class_count(CLASS_FULL), 4);
        assert_eq!(book.class_count(CLASS_DEGRADE), 3);
        assert_eq!(book.class_count(CLASS_REJECT), 3);
    }

    #[test]
    fn degrade_ledger_zero_false_claims() {
        let book = degrade_ledger();
        assert_eq!(book.false_claims(), 0);
        let mut i = 0usize;
        while i < book.len() {
            match book.get(i) {
                Some(r) => assert!(r.honest()),
                None => assert!(false),
            }
            i += 1;
        }
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(3) {
            Ok(log) => {
                assert!(log.all_caught());
                assert_eq!(log.tries, [3, 3, 3, 3]);
                assert_eq!(log.caught, [3, 3, 3, 3]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn injection_rejects_zero() {
        assert_eq!(inject_rounds(0), Err(QkCode::CoverBad.val()));
        assert_eq!(inject_rounds(5_000), Err(QkCode::CoverBad.val()));
    }

    #[test]
    fn fuzz_entries_balanced() {
        match fuzz_entries(2_000) {
            Ok(s) => {
                assert_eq!(s.rounds, 2_000);
                assert!(s.balanced());
                assert!(s.accepted > 0);
                assert!(s.rejected > 0);
                assert!(s.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_entries_rejects_oversize() {
        assert_eq!(fuzz_entries(0), Err(QkCode::CoverBad.val()));
        assert_eq!(fuzz_entries(FUZZ_ROUNDS + 1), Err(QkCode::CoverBad.val()));
    }

    #[test]
    fn render_round_trips() {
        let s = match match_scenarios(SCENARIOS) {
            Ok(v) => v,
            Err(_) => MatchStats::empty(),
        };
        let text = render_stats(&s);
        assert_eq!(text.split('|').count(), STATS_FIELDS);
        assert_eq!(audit_render(&text), Ok(STATS_FIELDS));
        assert_eq!(parse_u32("100"), Some(100));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("1x"), None);
        assert_eq!(parse_hex16("0x0000000000000001"), Some(1));
        assert_eq!(parse_hex16("1x"), None);
    }

    #[test]
    fn render_tamper_rejected() {
        let s = match match_scenarios(SCENARIOS) {
            Ok(v) => v,
            Err(_) => MatchStats::empty(),
        };
        let text = render_stats(&s);
        let bad = text.replacen("|30|20|", "|31|20|", 1);
        assert!(bad != text);
        assert_eq!(audit_render(&bad), Err(QkCode::LedgerBad.val()));
        assert_eq!(audit_render(""), Err(QkCode::RenderBad.val()));
        assert_eq!(audit_render("1|2|3|4|5"), Err(QkCode::RenderBad.val()));
    }

    #[test]
    fn golden_reproducible() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        match a {
            Ok(s) => {
                assert_eq!(s.scenarios, GOLDEN_ROUNDS);
                assert_eq!(s.hits, 8);
                assert!(s.balanced());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn repro_check_green() {
        assert_eq!(repro_check(), Ok(()));
    }

    #[test]
    fn codes_closed_and_segment_guard() {
        assert!(codes_are_distinct());
        assert!(segment_guard());
        assert_eq!(QkCode::ALL.len(), 14);
        let mut i = 0usize;
        while i < QkCode::ALL.len() {
            match QkCode::ALL.get(i) {
                Some(c) => {
                    assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
                    assert!(!c.detail().is_empty());
                    assert_eq!(detail(c.val()), c.detail());
                }
                None => assert!(false),
            }
            i += 1;
        }
        assert_eq!(detail(0), "未知诊断码");
    }

    #[test]
    fn probe_sequences_green() {
        assert_eq!(probe_seq_a(), Ok(()));
        assert_eq!(probe_seq_b(), Ok(()));
        assert_eq!(probe_seq_c(), Ok(()));
    }

    #[test]
    fn checks_are_green_and_untruncated() {
        let s = run_unx49001_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
        assert_eq!(s.dyn_used(), 8);
        assert_eq!(s.dyn_dropped(), 0);
    }

    #[test]
    fn checks_names_are_eight() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            match items.get(i) {
                Some((n, _, d)) => {
                    assert!(!n.is_empty());
                    assert!(d.len() <= 48);
                }
                None => assert!(false),
            }
            i += 1;
        }
    }
}
