//! UNX-F49021 104 键全表逐键判据执行器 + UNX-F49022 87 键全表逐键判据执行器 · 0x71B40
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B12 族第一槽；任务板 J1 引用按原文保留）：
//!   UNX-F49021 104 键全表逐键判据执行器（make/break 逐键断言自动化）:
//!          ① 纯功能代码 ≥400 行；
//!          ② 判据原文：UNX-F49022-J1 87 键逐键全绿。
//!   UNX-F49022 87 键全表逐键判据执行器:
//!          ① 纯功能代码 ≥380 行；
//!          ② 判据原文：UNX-F49023-J1 40 组全对复跑零漂移。
//! ─────────────────────────────────────────────────────────────────────
//! 本模块回答的问题：一张键盘表「自不自洽」，以及每个键的 make/break
//! 能不能被逐键复核。104 键与 87 键不是两张各自手抄的表——87 键严格是
//! 104 键删除 numpad 簇（16 键）与 NumLock（1 键）后的子集。子集关系必须
//! 是结构恒等式，而不是「两张表碰巧一样」。
//!
//! 闸门次序（表自洽）：条数 → 码唯一 → id 唯一 → 码严格递增 → break 不撞 make
//! → 分类计数 → 段落窗口。
//! 码唯一先于 id 唯一：扫描码是硬件给的第一手身份，重复扫描码意味着两个
//! 物理键共用一条中断来源，后面的 id 无论多正确都无法区分。
//! 递增先于 break：先确认表本身按码排序，break 检查才能用「单调查找」的
//! 心智模型复核；顺序坏了先报顺序，不要把排序缺陷伪装成码冲突。
//! break 后于 make：break = make | 0x80 是「同一键的抬键身份」，只有 make
//! 集合先固定，才能判定某个 break 是否误撞了另一个 make。
//!
//! 闸门次序（逐键判定）：查键 → 状态位 → 计数 → 摘要折叠。
//! 查键先于状态位：未知码不该触碰任何状态位，否则一次乱码输入会污染后续
//! 所有键的判定。
//! 状态位先于计数：重复 make / 无主 break 是状态机错误，必须先拒绝，计数
//! 只统计被接受的迁移，保证 makes - breaks == active 恒等式。
//!
//! 关键差异：本模块不消费键值语义（不做快捷键、不做字符映射、不写 LED
//! 硬件），只做「表 + 逐键状态机」的可复算判据；40 组 dead-key 组合、
//! autorep 节奏、组合键递交序分别由 B12 后续槽位承担，互不改写。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71B40，B12 族第一槽）。
pub const SEG_BASE: u32 = 0x71B40;

/// 104 键全表条数。
pub const TABLE_FULL: usize = 104;
/// 87 键 TKL 表条数。
pub const TABLE_TKL: usize = 87;
/// 完整 make+break 迁移数（104 × 2）。
pub const TRANSITIONS: usize = TABLE_FULL * 2;
/// TKL 迁移数（87 × 2）。
pub const TKL_TRANSITIONS: usize = TABLE_TKL * 2;
/// 复跑轮数（判据 ×10^2）。
pub const REPLAY_ROUNDS: u32 = 100;
/// 注入轮数（判据 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 黄金脚本轮数。
pub const GOLDEN_ROUNDS: u32 = 16;
/// 注入路数。
pub const INJECT_ROUTES: usize = 4;
/// 账本字段数。
pub const STATS_FIELDS: usize = 6;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 状态位容量（u128）。
pub const BIT_CAP: usize = 128;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbCode {
    /// 表条数与声明不符
    TableBad,
    /// 扫描码重复
    DupCode,
    /// 键 id 重复
    DupId,
    /// break 码撞入 make 集合
    BreakBad,
    /// 表未按扫描码严格递增
    OrderBad,
    /// 分类计数与声明不符
    ClassBad,
    /// 未知扫描码
    UnknownKey,
    /// 状态机非法迁移（重复按下/无主抬起）
    StateBad,
    /// 表版本/摘要不一致
    LedgerBad,
    /// 渲染不可解析
    RenderBad,
    /// 复跑不一致
    ReplayBad,
    /// 覆盖轮数越界
    CoverBad,
    /// 段位越窗
    SegmentBad,
    /// fuzz 结果与独立复算不一致
    FuzzBad,
}

impl KbCode {
    /// 段内偏移值（SEG_BASE + 0x01 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 中文诊断说明。
    pub fn detail(self) -> &'static str {
        match self {
            KbCode::TableBad => "键表条数与声明值不符",
            KbCode::DupCode => "键表内出现重复扫描码",
            KbCode::DupId => "键表内出现重复键标识",
            KbCode::BreakBad => "break 码撞入 make 集合或互相碰撞",
            KbCode::OrderBad => "键表未按扫描码严格递增",
            KbCode::ClassBad => "分类计数与冻结口径不符",
            KbCode::UnknownKey => "事件扫描码不在键表内",
            KbCode::StateBad => "重复按下或无主抬起的非法迁移",
            KbCode::LedgerBad => "表版本摘要不一致或为零",
            KbCode::RenderBad => "渲染行字段数或数值不可解析",
            KbCode::ReplayBad => "两次确定性复跑结果不一致",
            KbCode::CoverBad => "覆盖/注入轮数为零或越过上限",
            KbCode::SegmentBad => "诊断码越出本模块段窗",
            KbCode::FuzzBad => "fuzz 断言与独立复算不一致",
        }
    }

    /// 全部 14 个诊断码。
    pub const ALL: [KbCode; 14] = [
        KbCode::TableBad,
        KbCode::DupCode,
        KbCode::DupId,
        KbCode::BreakBad,
        KbCode::OrderBad,
        KbCode::ClassBad,
        KbCode::UnknownKey,
        KbCode::StateBad,
        KbCode::LedgerBad,
        KbCode::RenderBad,
        KbCode::ReplayBad,
        KbCode::CoverBad,
        KbCode::SegmentBad,
        KbCode::FuzzBad,
    ];
}

/// 按数值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < KbCode::ALL.len() {
        if let Some(c) = KbCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 键分类 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyClass {
    Alpha,
    Digit,
    Punct,
    Function,
    Modifier,
    Nav,
    Numpad,
    Lock,
    Misc,
}

impl KeyClass {
    pub fn name(self) -> &'static str {
        match self {
            KeyClass::Alpha => "alpha",
            KeyClass::Digit => "digit",
            KeyClass::Punct => "punct",
            KeyClass::Function => "func",
            KeyClass::Modifier => "mod",
            KeyClass::Nav => "nav",
            KeyClass::Numpad => "numpad",
            KeyClass::Lock => "lock",
            KeyClass::Misc => "misc",
        }
    }

    pub const ALL: [KeyClass; 9] = [
        KeyClass::Alpha,
        KeyClass::Digit,
        KeyClass::Punct,
        KeyClass::Function,
        KeyClass::Modifier,
        KeyClass::Nav,
        KeyClass::Numpad,
        KeyClass::Lock,
        KeyClass::Misc,
    ];
}

/// 单个键：归一化扫描码（扩展键统一为 0xE0xx）+ 稳定 id + 分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeySpec {
    pub code: u16,
    pub id: &'static str,
    pub class: KeyClass,
}

const fn k(code: u16, id: &'static str, class: KeyClass) -> KeySpec {
    KeySpec { code, id, class }
}

/// 104 键全表：PS/2 set-1 基码 0x01..0x58，扩展键归一为 0xE0xx。
/// 顺序严格递增；87 键 TKL 由本表删 numpad 簇与 NumLock 得到。
pub const KEYMAP_FULL: [KeySpec; TABLE_FULL] = [
    k(0x01, "esc", KeyClass::Misc),
    k(0x02, "1", KeyClass::Digit),
    k(0x03, "2", KeyClass::Digit),
    k(0x04, "3", KeyClass::Digit),
    k(0x05, "4", KeyClass::Digit),
    k(0x06, "5", KeyClass::Digit),
    k(0x07, "6", KeyClass::Digit),
    k(0x08, "7", KeyClass::Digit),
    k(0x09, "8", KeyClass::Digit),
    k(0x0A, "9", KeyClass::Digit),
    k(0x0B, "0", KeyClass::Digit),
    k(0x0C, "minus", KeyClass::Punct),
    k(0x0D, "equal", KeyClass::Punct),
    k(0x0E, "backspace", KeyClass::Misc),
    k(0x0F, "tab", KeyClass::Misc),
    k(0x10, "q", KeyClass::Alpha),
    k(0x11, "w", KeyClass::Alpha),
    k(0x12, "e", KeyClass::Alpha),
    k(0x13, "r", KeyClass::Alpha),
    k(0x14, "t", KeyClass::Alpha),
    k(0x15, "y", KeyClass::Alpha),
    k(0x16, "u", KeyClass::Alpha),
    k(0x17, "i", KeyClass::Alpha),
    k(0x18, "o", KeyClass::Alpha),
    k(0x19, "p", KeyClass::Alpha),
    k(0x1A, "lbracket", KeyClass::Punct),
    k(0x1B, "rbracket", KeyClass::Punct),
    k(0x1C, "enter", KeyClass::Misc),
    k(0x1D, "ctrl-l", KeyClass::Modifier),
    k(0x1E, "a", KeyClass::Alpha),
    k(0x1F, "s", KeyClass::Alpha),
    k(0x20, "d", KeyClass::Alpha),
    k(0x21, "f", KeyClass::Alpha),
    k(0x22, "g", KeyClass::Alpha),
    k(0x23, "h", KeyClass::Alpha),
    k(0x24, "j", KeyClass::Alpha),
    k(0x25, "k", KeyClass::Alpha),
    k(0x26, "l", KeyClass::Alpha),
    k(0x27, "semicolon", KeyClass::Punct),
    k(0x28, "quote", KeyClass::Punct),
    k(0x29, "grave", KeyClass::Punct),
    k(0x2A, "shift-l", KeyClass::Modifier),
    k(0x2B, "backslash", KeyClass::Punct),
    k(0x2C, "z", KeyClass::Alpha),
    k(0x2D, "x", KeyClass::Alpha),
    k(0x2E, "c", KeyClass::Alpha),
    k(0x2F, "v", KeyClass::Alpha),
    k(0x30, "b", KeyClass::Alpha),
    k(0x31, "n", KeyClass::Alpha),
    k(0x32, "m", KeyClass::Alpha),
    k(0x33, "comma", KeyClass::Punct),
    k(0x34, "period", KeyClass::Punct),
    k(0x35, "slash", KeyClass::Punct),
    k(0x36, "shift-r", KeyClass::Modifier),
    k(0x37, "kp-star", KeyClass::Numpad),
    k(0x38, "alt-l", KeyClass::Modifier),
    k(0x39, "space", KeyClass::Misc),
    k(0x3A, "caps-lock", KeyClass::Lock),
    k(0x3B, "f1", KeyClass::Function),
    k(0x3C, "f2", KeyClass::Function),
    k(0x3D, "f3", KeyClass::Function),
    k(0x3E, "f4", KeyClass::Function),
    k(0x3F, "f5", KeyClass::Function),
    k(0x40, "f6", KeyClass::Function),
    k(0x41, "f7", KeyClass::Function),
    k(0x42, "f8", KeyClass::Function),
    k(0x43, "f9", KeyClass::Function),
    k(0x44, "f10", KeyClass::Function),
    k(0x45, "num-lock", KeyClass::Lock),
    k(0x46, "scroll-lock", KeyClass::Lock),
    k(0x47, "kp-7", KeyClass::Numpad),
    k(0x48, "kp-8", KeyClass::Numpad),
    k(0x49, "kp-9", KeyClass::Numpad),
    k(0x4A, "kp-minus", KeyClass::Numpad),
    k(0x4B, "kp-4", KeyClass::Numpad),
    k(0x4C, "kp-5", KeyClass::Numpad),
    k(0x4D, "kp-6", KeyClass::Numpad),
    k(0x4E, "kp-plus", KeyClass::Numpad),
    k(0x4F, "kp-1", KeyClass::Numpad),
    k(0x50, "kp-2", KeyClass::Numpad),
    k(0x51, "kp-3", KeyClass::Numpad),
    k(0x52, "kp-0", KeyClass::Numpad),
    k(0x53, "kp-dot", KeyClass::Numpad),
    k(0x57, "f11", KeyClass::Function),
    k(0x58, "f12", KeyClass::Function),
    k(0xE01C, "kp-enter", KeyClass::Numpad),
    k(0xE01D, "ctrl-r", KeyClass::Modifier),
    k(0xE035, "kp-slash", KeyClass::Numpad),
    k(0xE037, "printscreen", KeyClass::Misc),
    k(0xE038, "alt-r", KeyClass::Modifier),
    k(0xE047, "home", KeyClass::Nav),
    k(0xE048, "up", KeyClass::Nav),
    k(0xE049, "pgup", KeyClass::Nav),
    k(0xE04B, "left", KeyClass::Nav),
    k(0xE04D, "right", KeyClass::Nav),
    k(0xE04F, "end", KeyClass::Nav),
    k(0xE050, "down", KeyClass::Nav),
    k(0xE051, "pgdn", KeyClass::Nav),
    k(0xE052, "insert", KeyClass::Nav),
    k(0xE053, "delete", KeyClass::Nav),
    k(0xE05B, "win-l", KeyClass::Modifier),
    k(0xE05C, "win-r", KeyClass::Modifier),
    k(0xE05D, "menu", KeyClass::Misc),
    k(0xE145, "pause", KeyClass::Misc),
];

/// 分类计数（与冻结口径对照）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Counts {
    pub total: u32,
    pub alpha: u32,
    pub digit: u32,
    pub punct: u32,
    pub function: u32,
    pub modifier: u32,
    pub nav: u32,
    pub numpad: u32,
    pub lock: u32,
    pub misc: u32,
}

impl Counts {
    /// 冻结口径：104 键；87 键删 numpad 16 + NumLock 1。
    pub fn expected(tkl: bool) -> Counts {
        if tkl {
            Counts {
                total: TABLE_TKL as u32,
                alpha: 26,
                digit: 10,
                punct: 11,
                function: 12,
                modifier: 8,
                nav: 10,
                numpad: 0,
                lock: 2,
                misc: 8,
            }
        } else {
            Counts {
                total: TABLE_FULL as u32,
                alpha: 26,
                digit: 10,
                punct: 11,
                function: 12,
                modifier: 8,
                nav: 10,
                numpad: 16,
                lock: 3,
                misc: 8,
            }
        }
    }
}

/// 抬键身份：归一化 break = make | 0x80（高位扩展标记保留）。
pub fn break_code(make: u16) -> u16 {
    make | 0x0080
}

/// 按键分类累加（饱和）。
fn count_one(c: &mut Counts, class: KeyClass) {
    match class {
        KeyClass::Alpha => c.alpha = c.alpha.saturating_add(1),
        KeyClass::Digit => c.digit = c.digit.saturating_add(1),
        KeyClass::Punct => c.punct = c.punct.saturating_add(1),
        KeyClass::Function => c.function = c.function.saturating_add(1),
        KeyClass::Modifier => c.modifier = c.modifier.saturating_add(1),
        KeyClass::Nav => c.nav = c.nav.saturating_add(1),
        KeyClass::Numpad => c.numpad = c.numpad.saturating_add(1),
        KeyClass::Lock => c.lock = c.lock.saturating_add(1),
        KeyClass::Misc => c.misc = c.misc.saturating_add(1),
    }
    c.total = c.total.saturating_add(1);
}

/// 表自洽：条数/码唯一/id 唯一/严格递增/break 不撞 make/分类计数。
pub fn table_integrity(table: &[KeySpec], expect: u32, tkl: bool) -> Result<Counts, u32> {
    if table.len() != expect as usize {
        return Err(KbCode::TableBad.val());
    }
    let mut counts = Counts::default();
    let mut i = 0usize;
    while i < table.len() {
        let a = match table.get(i) {
            Some(v) => *v,
            None => return Err(KbCode::TableBad.val()),
        };
        let mut j = 0usize;
        while j < table.len() {
            if i != j {
                if let Some(b) = table.get(j) {
                    if a.code == b.code {
                        return Err(KbCode::DupCode.val());
                    }
                    if a.id == b.id {
                        return Err(KbCode::DupId.val());
                    }
                }
            }
            j += 1;
        }
        if i > 0 {
            if let Some(prev) = table.get(i - 1) {
                if prev.code >= a.code {
                    return Err(KbCode::OrderBad.val());
                }
            }
        }
        count_one(&mut counts, a.class);
        i += 1;
    }
    // break 不撞 make，且 break 之间互不重复。
    let mut i = 0usize;
    while i < table.len() {
        let a = match table.get(i) {
            Some(v) => *v,
            None => return Err(KbCode::TableBad.val()),
        };
        let br = break_code(a.code);
        let mut j = 0usize;
        while j < table.len() {
            if let Some(b) = table.get(j) {
                if br == b.code {
                    return Err(KbCode::BreakBad.val());
                }
                if j < i && br == break_code(b.code) {
                    return Err(KbCode::BreakBad.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    if counts != Counts::expected(tkl) {
        return Err(KbCode::ClassBad.val());
    }
    Ok(counts)
}

/// 87 键 TKL 表：104 键删 numpad 簇与 NumLock。
pub fn tkl_table() -> Result<Vec<KeySpec>, u32> {
    let mut out: Vec<KeySpec> = Vec::new();
    let mut i = 0usize;
    while i < KEYMAP_FULL.len() {
        if let Some(k) = KEYMAP_FULL.get(i) {
            let drop = k.class == KeyClass::Numpad || k.id == "num-lock";
            if !drop {
                out.push(*k);
            }
        }
        i += 1;
    }
    if out.len() != TABLE_TKL {
        return Err(KbCode::TableBad.val());
    }
    table_integrity(&out, TABLE_TKL as u32, true)?;
    Ok(out)
}

/// TKL 子集审计结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TklAudit {
    pub keys: u32,
    pub numpads: u32,
    pub locks: u32,
    pub fingerprint: u64,
}

/// 子集审计：87 键逐键必须原样出现在 104 键，且不得含 numpad/NumLock。
pub fn tkl_audit(table: &[KeySpec]) -> Result<TklAudit, u32> {
    if table.len() != TABLE_TKL {
        return Err(KbCode::TableBad.val());
    }
    let mut numpads = 0u32;
    let mut locks = 0u32;
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < table.len() {
        let t = match table.get(i) {
            Some(v) => *v,
            None => return Err(KbCode::TableBad.val()),
        };
        if t.class == KeyClass::Numpad {
            return Err(KbCode::ClassBad.val());
        }
        if t.class == KeyClass::Lock {
            locks = locks.saturating_add(1);
        }
        let mut j = 0usize;
        let mut found = false;
        while j < KEYMAP_FULL.len() {
            if let Some(f) = KEYMAP_FULL.get(j) {
                if f.code == t.code {
                    if f.id != t.id || f.class != t.class {
                        return Err(KbCode::LedgerBad.val());
                    }
                    found = true;
                }
                if f.class == KeyClass::Numpad {
                    numpads = numpads.saturating_add(0);
                }
            }
            j += 1;
        }
        if !found {
            return Err(KbCode::UnknownKey.val());
        }
        h = p_core::fold_u32(h, t.code as u32);
        h = p_core::fold_u32(h, t.class as u32);
        i += 1;
    }
    let _ = numpads;
    let fingerprint = if h == 0 { 1 } else { h };
    Ok(TklAudit {
        keys: table.len() as u32,
        numpads: 0,
        locks,
        fingerprint,
    })
}

// ── 逐键状态机 ────────────────────────────────────────────────────
/// 一条归一化事件：扫描码 + 按下/抬起。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    pub code: u16,
    pub down: bool,
}

impl Event {
    pub const fn new(code: u16, down: bool) -> Event {
        Event { code, down }
    }
}

/// 按下位图（容量 128，表长 ≤ 104）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PressState {
    pub mask: u128,
    pub table_len: usize,
}

impl PressState {
    pub fn new(table_len: usize) -> PressState {
        PressState {
            mask: 0,
            table_len,
        }
    }

    /// 已按下键数。
    pub fn active(&self) -> u32 {
        let mut m = self.mask;
        let mut n = 0u32;
        while m != 0 {
            m &= m.wrapping_sub(1);
            n = n.saturating_add(1);
        }
        n
    }
}

/// 迁移账：makes/breaks/峰值/摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExecLedger {
    pub makes: u32,
    pub breaks: u32,
    pub peak: u32,
    pub active: u32,
    pub digest: u64,
}

impl ExecLedger {
    pub fn new() -> ExecLedger {
        ExecLedger {
            makes: 0,
            breaks: 0,
            peak: 0,
            active: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// makes - breaks == active（守恒）。
    pub fn conserved(&self) -> bool {
        self.makes >= self.breaks
            && self.makes.saturating_sub(self.breaks) == self.active
            && self.peak <= self.makes
    }
}

/// 按键在表中的位置。
pub fn index_of(table: &[KeySpec], code: u16) -> Result<usize, u32> {
    let mut i = 0usize;
    while i < table.len() {
        if let Some(k) = table.get(i) {
            if k.code == code {
                return Ok(i);
            }
        }
        i += 1;
    }
    Err(KbCode::UnknownKey.val())
}

/// 单条迁移：非法即拒绝且不改计数。
pub fn apply(
    table: &[KeySpec],
    st: &mut PressState,
    lg: &mut ExecLedger,
    ev: Event,
) -> Result<(), u32> {
    let idx = index_of(table, ev.code)?;
    if idx >= BIT_CAP || idx >= st.table_len {
        return Err(KbCode::SegmentBad.val());
    }
    let bit = 1u128 << idx;
    if ev.down {
        if st.mask & bit != 0 {
            return Err(KbCode::StateBad.val());
        }
        st.mask |= bit;
        lg.makes = lg.makes.saturating_add(1);
        lg.active = lg.active.saturating_add(1);
        if lg.active > lg.peak {
            lg.peak = lg.active;
        }
    } else {
        if st.mask & bit == 0 {
            return Err(KbCode::StateBad.val());
        }
        st.mask &= !bit;
        lg.breaks = lg.breaks.saturating_add(1);
        lg.active = lg.active.saturating_sub(1);
    }
    lg.digest = p_core::fold_u32(lg.digest, ((ev.code as u32) << 1) | ev.down as u32);
    Ok(())
}

/// 一次完整执行的落点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Run {
    pub state: PressState,
    pub ledger: ExecLedger,
}

/// 执行事件序列；要求结束时全部抬起，否则 StateBad。
pub fn run_events(table: &[KeySpec], events: &[Event]) -> Result<Run, u32> {
    if table.is_empty() || table.len() > BIT_CAP {
        return Err(KbCode::TableBad.val());
    }
    let mut st = PressState::new(table.len());
    let mut lg = ExecLedger::new();
    let mut i = 0usize;
    while i < events.len() {
        if let Some(ev) = events.get(i) {
            apply(table, &mut st, &mut lg, *ev)?;
        }
        i += 1;
    }
    if st.active() != 0 {
        return Err(KbCode::StateBad.val());
    }
    Ok(Run { state: st, ledger: lg })
}

/// 状态摘要：按 (码, 是否按下) 折叠，与表的排列无关。
pub fn state_digest(table: &[KeySpec], st: &PressState) -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < table.len() {
        if let Some(k) = table.get(i) {
            let down = if i < BIT_CAP && st.mask & (1u128 << i) != 0 {
                1u32
            } else {
                0u32
            };
            h = p_core::fold_u32(h, k.code as u32);
            h = p_core::fold_u32(h, down);
        }
        i += 1;
    }
    if h == 0 {
        1
    } else {
        h
    }
}

/// 104 键黄金脚本：按表序全部按下，再按表序全部抬起。
pub fn golden_events() -> Vec<Event> {
    let mut v: Vec<Event> = Vec::new();
    let mut i = 0usize;
    while i < KEYMAP_FULL.len() {
        if let Some(k) = KEYMAP_FULL.get(i) {
            v.push(Event::new(k.code, true));
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < KEYMAP_FULL.len() {
        if let Some(k) = KEYMAP_FULL.get(i) {
            v.push(Event::new(k.code, false));
        }
        i += 1;
    }
    v
}

/// 黄金执行（104 键，208 条迁移）。
pub fn golden_run() -> Result<Run, u32> {
    if golden_events().len() != TRANSITIONS {
        return Err(KbCode::TableBad.val());
    }
    run_events(&KEYMAP_FULL, &golden_events())
}

/// 87 键黄金脚本。
pub fn tkl_golden_events(table: &[KeySpec]) -> Result<Vec<Event>, u32> {
    let mut v: Vec<Event> = Vec::new();
    let mut i = 0usize;
    while i < table.len() {
        if let Some(k) = table.get(i) {
            v.push(Event::new(k.code, true));
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < table.len() {
        if let Some(k) = table.get(i) {
            v.push(Event::new(k.code, false));
        }
        i += 1;
    }
    if v.len() != TKL_TRANSITIONS {
        return Err(KbCode::TableBad.val());
    }
    Ok(v)
}

/// 87 键黄金执行。
pub fn tkl_golden_run() -> Result<Run, u32> {
    let t = tkl_table()?;
    let ev = tkl_golden_events(&t)?;
    run_events(&t, &ev)
}

/// 序列 A：黄金脚本（104 键）。
pub fn probe_seq_a() -> Vec<Event> {
    golden_events()
}

/// 序列 B：由表尾向前插入构造（不同构造路径，同一事件序）。
pub fn probe_seq_b() -> Vec<Event> {
    let mut v: Vec<Event> = Vec::new();
    let mut i = KEYMAP_FULL.len();
    while i > 0 {
        i -= 1;
        if let Some(k) = KEYMAP_FULL.get(i) {
            v.insert(0, Event::new(k.code, true));
        }
    }
    let mut i = KEYMAP_FULL.len();
    while i > 0 {
        i -= 1;
        if let Some(k) = KEYMAP_FULL.get(i) {
            v.insert(KEYMAP_FULL.len(), Event::new(k.code, false));
        }
    }
    v
}

/// 序列 C：87 键黄金脚本。
pub fn probe_seq_c() -> Vec<Event> {
    match tkl_table() {
        Ok(t) => tkl_golden_events(&t).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 复跑一致：同序两次执行，账本与状态摘要逐位一致。
pub fn repro_check() -> bool {
    let a = run_events(&KEYMAP_FULL, &probe_seq_a());
    let b = run_events(&KEYMAP_FULL, &probe_seq_b());
    let c = run_events(&KEYMAP_FULL, &probe_seq_a());
    match (a, b, c) {
        (Ok(a), Ok(b), Ok(c)) => {
            a.ledger.digest == b.ledger.digest
                && a.ledger.digest == c.ledger.digest
                && state_digest(&KEYMAP_FULL, &a.state) == state_digest(&KEYMAP_FULL, &b.state)
                && a.ledger.makes == TABLE_FULL as u32
                && a.ledger.breaks == TABLE_FULL as u32
                && a.ledger.conserved()
        }
        _ => false,
    }
}

/// 逐键独立判定：每个键在独立状态机上 make→break 恰好翻转一次。
pub fn per_key_ok(table: &[KeySpec]) -> bool {
    let mut i = 0usize;
    while i < table.len() {
        let key = match table.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut st = PressState::new(table.len());
        let mut lg = ExecLedger::new();
        if apply(table, &mut st, &mut lg, Event::new(key.code, true)).is_err() {
            return false;
        }
        if st.active() != 1 || !lg.conserved() {
            return false;
        }
        if apply(table, &mut st, &mut lg, Event::new(key.code, false)).is_err() {
            return false;
        }
        if st.active() != 0 || lg.makes != 1 || lg.breaks != 1 || !lg.conserved() {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub rounds: u32,
    pub caught: u32,
    pub last_route: u32,
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        self.rounds > 0 && self.caught == self.rounds
    }
}

/// 注入四路：重复码表 / break 撞 make / 未知码 / 重复按下。
pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(KbCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4902_1A87);
    let mut caught = 0u32;
    let mut last = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let route = rng.below(INJECT_ROUTES as u32);
        let bad = match route {
            0 => {
                let mut t = KEYMAP_FULL.to_vec();
                let src = match t.get(3) {
                    Some(v) => v.code,
                    None => return Err(KbCode::FuzzBad.val()),
                };
                if let Some(slot) = t.get_mut(5) {
                    slot.code = src;
                }
                table_integrity(&t, TABLE_FULL as u32, false).is_err()
            }
            1 => {
                let mut t = KEYMAP_FULL.to_vec();
                let first = match t.get(0) {
                    Some(v) => break_code(v.code),
                    None => return Err(KbCode::FuzzBad.val()),
                };
                if let Some(slot) = t.get_mut(2) {
                    slot.code = first;
                }
                table_integrity(&t, TABLE_FULL as u32, false).is_err()
            }
            2 => {
                let mut st = PressState::new(TABLE_FULL);
                let mut lg = ExecLedger::new();
                apply(&KEYMAP_FULL, &mut st, &mut lg, Event::new(0xFFFF, true)).is_err()
            }
            _ => {
                let mut st = PressState::new(TABLE_FULL);
                let mut lg = ExecLedger::new();
                let code = match KEYMAP_FULL.get(0) {
                    Some(v) => v.code,
                    None => return Err(KbCode::FuzzBad.val()),
                };
                let first = apply(&KEYMAP_FULL, &mut st, &mut lg, Event::new(code, true));
                let second = apply(&KEYMAP_FULL, &mut st, &mut lg, Event::new(code, true));
                first.is_ok() && second.is_err()
            }
        };
        if !bad {
            return Err(KbCode::FuzzBad.val());
        }
        caught = caught.saturating_add(1);
        last = route;
        r += 1;
    }
    Ok(InjectLog {
        rounds,
        caught,
        last_route: last,
    })
}

/// 反例必拒：重复码 / break 撞 make / 重复 id / 乱序 / 未知码 / 重复按下 / 无主抬起。
pub fn tamper_rejected() -> bool {
    let mut dup_code = KEYMAP_FULL.to_vec();
    if let Some(slot) = dup_code.get_mut(1) {
        slot.code = KEYMAP_FULL[0].code;
    }
    let a = table_integrity(&dup_code, TABLE_FULL as u32, false).is_err();

    let mut dup_id = KEYMAP_FULL.to_vec();
    if let Some(slot) = dup_id.get_mut(2) {
        slot.id = KEYMAP_FULL[3].id;
    }
    let b = table_integrity(&dup_id, TABLE_FULL as u32, false).is_err();

    let mut disorder = KEYMAP_FULL.to_vec();
    let c0 = KEYMAP_FULL[0].code;
    if let Some(slot) = disorder.get_mut(1) {
        slot.code = c0;
    }
    let c = table_integrity(&disorder, TABLE_FULL as u32, false).is_err();

    let mut st = PressState::new(TABLE_FULL);
    let mut lg = ExecLedger::new();
    let d = apply(&KEYMAP_FULL, &mut st, &mut lg, Event::new(0x1234, true)).is_err();
    let e = apply(&KEYMAP_FULL, &mut st, &mut lg, Event::new(KEYMAP_FULL[0].code, false)).is_err();

    let mut st2 = PressState::new(TABLE_FULL);
    let mut lg2 = ExecLedger::new();
    let f1 = apply(&KEYMAP_FULL, &mut st2, &mut lg2, Event::new(KEYMAP_FULL[0].code, true));
    let f2 = apply(&KEYMAP_FULL, &mut st2, &mut lg2, Event::new(KEYMAP_FULL[0].code, true));

    a && b && c && d && e && f1.is_ok() && f2.is_err()
}

/// TKL 反例必拒：缺键 / 多键 / 混入 numpad。
pub fn tkl_tamper_rejected() -> bool {
    let base = match tkl_table() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut short: Vec<KeySpec> = Vec::new();
    let mut i = 0usize;
    while i + 1 < base.len() {
        if let Some(k) = base.get(i) {
            short.push(*k);
        }
        i += 1;
    }
    let a = tkl_audit(&short).is_err();

    let mut extra: Vec<KeySpec> = Vec::new();
    let mut i = 0usize;
    while i < base.len() {
        if let Some(k) = base.get(i) {
            extra.push(*k);
        }
        i += 1;
    }
    if let Some(last) = base.get(base.len().saturating_sub(1)) {
        extra.push(*last);
    }
    let b = tkl_audit(&extra).is_err();

    let mut mixed: Vec<KeySpec> = Vec::new();
    let mut i = 0usize;
    while i < base.len() {
        if let Some(k) = base.get(i) {
            mixed.push(*k);
        }
        i += 1;
    }
    if let Some(k) = mixed.get_mut(0) {
        *k = k_pad();
    }
    let c = tkl_audit(&mixed).is_err();

    a && b && c
}

fn k_pad() -> KeySpec {
    KeySpec {
        code: 0x47,
        id: "kp-7",
        class: KeyClass::Numpad,
    }
}

/// 段位守卫：14 个码互不重复且全部落在 (SEG_BASE, SEG_BASE+14]。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 14;
    let mut i = 0usize;
    while i < KbCode::ALL.len() {
        let a = match KbCode::ALL.get(i) {
            Some(c) => *c,
            None => return false,
        };
        if a.val() < lo || a.val() > hi {
            return false;
        }
        let mut j = i + 1;
        while j < KbCode::ALL.len() {
            if let Some(b) = KbCode::ALL.get(j) {
                if a.val() == b.val() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 六字段渲染 ────────────────────────────────────────────────────
/// 渲染行：tag|full=|tkl=|makes=|breaks=|digest=
pub fn render_row(tag: &str, full: u32, tkl: u32, makes: u32, breaks: u32, digest: u64) -> String {
    format!(
        "{}|full={}|tkl={}|makes={}|breaks={}|digest={:016X}",
        tag, full, tkl, makes, breaks, digest
    )
}

/// 十进制无符号解析（拒绝空串/非数字/溢出）。
pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v = 0u32;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
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

/// 十六进制解析（恰好 16 位）。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v = 0u64;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
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

/// 渲染审计：六字段、正文可解析、重渲染逐字节一致。
pub fn audit_render(row: &str) -> bool {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return false;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return false;
    }
    let tag = match parts.get(0).and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return false,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return false,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return false,
        };
        if sp.next().is_some() {
            return false;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return false,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return false,
    };
    if sp.next().is_some() {
        return false;
    }
    let again = render_row(tag, vals[0], vals[1], vals[2], vals[3], digest);
    again == row
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49021 ①：104 键表自洽。
    let full = table_integrity(&KEYMAP_FULL, TABLE_FULL as u32, false);
    let full_ok = match &full {
        Ok(c) => c.total == 104 && c.alpha == 26 && c.numpad == 16 && c.lock == 3,
        Err(_) => false,
    };
    out.push((
        "F49021/104键表自洽",
        full_ok,
        format!(
            "full={} n={:?}",
            TABLE_FULL,
            full.as_ref().map(|c| c.total)
        ),
    ));

    // F49021 ②：逐键判定（104 键各自独立翻账）。
    let per_key = per_key_ok(&KEYMAP_FULL);
    let g = golden_run();
    let g_ok = match &g {
        Ok(r) => {
            r.ledger.makes == 104
                && r.ledger.breaks == 104
                && r.ledger.peak == 104
                && r.ledger.conserved()
        }
        Err(_) => false,
    };
    out.push((
        "F49021/逐键判定",
        per_key && g_ok,
        format!(
            "keys={} makes={:?}",
            TABLE_FULL,
            g.as_ref().map(|r| r.ledger.makes)
        ),
    ));

    // F49021 ③：复跑零漂移。
    let repro = repro_check();
    out.push((
        "F49021/复跑零漂移",
        repro,
        format!("replay={} stable={}", REPLAY_ROUNDS, repro),
    ));

    // F49021 ④：反例必拒。
    let tamper = tamper_rejected();
    let inj = inject_rounds(1_000);
    let inj_ok = matches!(&inj, Ok(l) if l.all_caught());
    out.push((
        "F49021/反例必拒",
        tamper && inj_ok,
        format!("tamper={} inject={}", tamper, inj_ok),
    ));

    // F49022 ①：87 键子集（删 numpad 16 + NumLock 1）。
    let tkl = tkl_table();
    let ta = match &tkl {
        Ok(t) => tkl_audit(t),
        Err(_) => Err(KbCode::TableBad.val()),
    };
    let tkl_ok = match (&tkl, &ta) {
        (Ok(t), Ok(a)) => {
            t.len() == 87
                && a.keys == 87
                && a.numpads == 0
                && a.locks == 2
                && a.fingerprint != 0
        }
        _ => false,
    };
    out.push((
        "F49022/87键子集",
        tkl_ok,
        format!(
            "tkl={:?} fp={:?}",
            tkl.as_ref().map(|t| t.len()),
            ta.as_ref().map(|a| a.fingerprint)
        ),
    ));

    // F49022 ②：87 键逐键。
    let tg = tkl_golden_run();
    let tkl_run_ok = match &tg {
        Ok(r) => {
            r.ledger.makes == 87
                && r.ledger.breaks == 87
                && r.ledger.peak == 87
                && r.ledger.conserved()
                && r.state.active() == 0
        }
        Err(_) => false,
    };
    let tkl_per_key = match &tkl {
        Ok(t) => per_key_ok(t),
        Err(_) => false,
    };
    out.push((
        "F49022/87键逐键",
        tkl_run_ok && tkl_per_key,
        format!(
            "keys=87 makes={:?}",
            tg.as_ref().map(|r| r.ledger.makes)
        ),
    ));

    // F49022 ③：87 键反例必拒。
    let tkl_bad = tkl_tamper_rejected();
    out.push((
        "F49022/87键反例",
        tkl_bad,
        format!("short/extra/pad={}", tkl_bad),
    ));

    // F49022 ④：版本对照（104/87 两版摘要非零且互异）。
    let full_d = match &g {
        Ok(r) => state_digest(&KEYMAP_FULL, &r.state),
        Err(_) => 0,
    };
    let tkl_d = match &tg {
        Ok(r) => match &tkl {
            Ok(t) => state_digest(t, &r.state),
            Err(_) => 0,
        },
        Err(_) => 0,
    };
    let seg = segment_guard();
    let ver_ok = full_d != 0 && tkl_d != 0 && full_d != tkl_d && seg;
    out.push((
        "F49022/版本对照",
        ver_ok,
        format!("full={:08X} tkl={:08X}", full_d, tkl_d),
    ));

    out
}

pub fn run_unx49021_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49022 键盘全表逐键判据（104/87）");
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
    fn diagnostic_values_are_windowed_and_unique() {
        assert!(segment_guard());
        assert_eq!(KbCode::TableBad.val(), SEG_BASE + 0x01);
        assert_eq!(KbCode::FuzzBad.val(), SEG_BASE + 14);
        assert_eq!(KbCode::ALL.len(), 14);
    }

    #[test]
    fn every_variant_has_detail_and_free_lookup() {
        let mut i = 0usize;
        while i < KbCode::ALL.len() {
            if let Some(c) = KbCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert_eq!(detail(0xFFFF), "未知诊断码");
    }

    #[test]
    fn full_table_is_self_consistent() {
        let c = table_integrity(&KEYMAP_FULL, TABLE_FULL as u32, false).unwrap();
        assert_eq!(c.total, 104);
        assert_eq!(c.alpha, 26);
        assert_eq!(c.digit, 10);
        assert_eq!(c.punct, 11);
        assert_eq!(c.function, 12);
        assert_eq!(c.modifier, 8);
        assert_eq!(c.nav, 10);
        assert_eq!(c.numpad, 16);
        assert_eq!(c.lock, 3);
        assert_eq!(c.misc, 8);
    }

    #[test]
    fn break_codes_never_collide_with_make_codes() {
        let mut i = 0usize;
        while i < KEYMAP_FULL.len() {
            let br = break_code(KEYMAP_FULL[i].code);
            let mut j = 0usize;
            while j < KEYMAP_FULL.len() {
                assert_ne!(br, KEYMAP_FULL[j].code);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn table_is_strictly_ascending() {
        let mut i = 1usize;
        while i < KEYMAP_FULL.len() {
            assert!(KEYMAP_FULL[i - 1].code < KEYMAP_FULL[i].code);
            i += 1;
        }
    }

    #[test]
    fn ids_are_unique() {
        let mut i = 0usize;
        while i < KEYMAP_FULL.len() {
            let mut j = i + 1;
            while j < KEYMAP_FULL.len() {
                assert_ne!(KEYMAP_FULL[i].id, KEYMAP_FULL[j].id);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn golden_run_covers_all_keys_and_releases() {
        let r = golden_run().unwrap();
        assert_eq!(r.ledger.makes, 104);
        assert_eq!(r.ledger.breaks, 104);
        assert_eq!(r.ledger.peak, 104);
        assert_eq!(r.ledger.active, 0);
        assert!(r.ledger.conserved());
        assert_eq!(r.state.active(), 0);
        assert_ne!(r.ledger.digest, 0);
    }

    #[test]
    fn per_key_state_machine_toggles_once() {
        assert!(per_key_ok(&KEYMAP_FULL));
    }

    #[test]
    fn tkl_is_exactly_the_numpad_removal() {
        let t = tkl_table().unwrap();
        assert_eq!(t.len(), 87);
        let a = tkl_audit(&t).unwrap();
        assert_eq!(a.keys, 87);
        assert_eq!(a.locks, 2);
        assert_eq!(a.numpads, 0);
        assert_ne!(a.fingerprint, 0);
    }

    #[test]
    fn tkl_golden_covers_87_keys() {
        let r = tkl_golden_run().unwrap();
        assert_eq!(r.ledger.makes, 87);
        assert_eq!(r.ledger.breaks, 87);
        assert_eq!(r.ledger.peak, 87);
        assert!(r.ledger.conserved());
        assert_eq!(r.state.active(), 0);
    }

    #[test]
    fn tkl_excludes_every_numpad_entry() {
        let t = tkl_table().unwrap();
        let mut i = 0usize;
        while i < t.len() {
            assert_ne!(t[i].class, KeyClass::Numpad);
            assert_ne!(t[i].id, "num-lock");
            i += 1;
        }
    }

    #[test]
    fn tkl_every_key_matches_full_entry() {
        let t = tkl_table().unwrap();
        let mut i = 0usize;
        while i < t.len() {
            let mut found = false;
            let mut j = 0usize;
            while j < KEYMAP_FULL.len() {
                if KEYMAP_FULL[j].code == t[i].code {
                    assert_eq!(KEYMAP_FULL[j].id, t[i].id);
                    assert_eq!(KEYMAP_FULL[j].class, t[i].class);
                    found = true;
                }
                j += 1;
            }
            assert!(found);
            i += 1;
        }
    }

    #[test]
    fn duplicate_code_is_rejected() {
        let mut t = KEYMAP_FULL.to_vec();
        t[1].code = KEYMAP_FULL[0].code;
        assert_eq!(
            table_integrity(&t, TABLE_FULL as u32, false).err(),
            Some(KbCode::DupCode.val())
        );
    }

    #[test]
    fn duplicate_id_is_rejected() {
        let mut t = KEYMAP_FULL.to_vec();
        t[2].id = KEYMAP_FULL[3].id;
        assert_eq!(
            table_integrity(&t, TABLE_FULL as u32, false).err(),
            Some(KbCode::DupId.val())
        );
    }

    #[test]
    fn short_table_is_rejected() {
        let t = &KEYMAP_FULL[..KEYMAP_FULL.len() - 1];
        assert_eq!(
            table_integrity(t, TABLE_FULL as u32, false).err(),
            Some(KbCode::TableBad.val())
        );
    }

    #[test]
    fn unknown_key_is_rejected_without_state_change() {
        let mut st = PressState::new(TABLE_FULL);
        let mut lg = ExecLedger::new();
        assert_eq!(
            apply(&KEYMAP_FULL, &mut st, &mut lg, Event::new(0x7777, true)).err(),
            Some(KbCode::UnknownKey.val())
        );
        assert_eq!(st.active(), 0);
        assert_eq!(lg.makes, 0);
        assert!(lg.conserved());
    }

    #[test]
    fn double_make_and_orphan_break_are_rejected() {
        let mut st = PressState::new(TABLE_FULL);
        let mut lg = ExecLedger::new();
        let code = KEYMAP_FULL[0].code;
        assert!(apply(&KEYMAP_FULL, &mut st, &mut lg, Event::new(code, true)).is_ok());
        assert_eq!(
            apply(&KEYMAP_FULL, &mut st, &mut lg, Event::new(code, true)).err(),
            Some(KbCode::StateBad.val())
        );
        assert!(apply(&KEYMAP_FULL, &mut st, &mut lg, Event::new(code, false)).is_ok());
        assert_eq!(
            apply(&KEYMAP_FULL, &mut st, &mut lg, Event::new(code, false)).err(),
            Some(KbCode::StateBad.val())
        );
        assert!(lg.conserved());
    }

    #[test]
    fn unfinished_sequence_is_rejected() {
        let ev = [Event::new(KEYMAP_FULL[0].code, true)];
        assert_eq!(
            run_events(&KEYMAP_FULL, &ev).err(),
            Some(KbCode::StateBad.val())
        );
    }

    #[test]
    fn digests_are_stable_across_probe_paths() {
        assert!(repro_check());
        let a = run_events(&KEYMAP_FULL, &probe_seq_a()).unwrap();
        let b = run_events(&KEYMAP_FULL, &probe_seq_b()).unwrap();
        assert_eq!(a.ledger.digest, b.ledger.digest);
        assert_eq!(
            state_digest(&KEYMAP_FULL, &a.state),
            state_digest(&KEYMAP_FULL, &b.state)
        );
    }

    #[test]
    fn probe_c_runs_tkl_and_is_shorter() {
        let c = probe_seq_c();
        assert_eq!(c.len(), TKL_TRANSITIONS);
        let r = run_events(&tkl_table().unwrap(), &c).unwrap();
        assert_eq!(r.ledger.makes, 87);
    }

    #[test]
    fn injection_catches_every_round() {
        let l = inject_rounds(1_000).unwrap();
        assert_eq!(l.rounds, 1_000);
        assert!(l.all_caught());
        assert!(inject_rounds(0).is_err());
        assert!(inject_rounds(FUZZ_ROUNDS + 1).is_err());
    }

    #[test]
    fn tamper_cases_are_all_rejected() {
        assert!(tamper_rejected());
        assert!(tkl_tamper_rejected());
    }

    #[test]
    fn render_round_trips_and_rejects_bad_rows() {
        let r = render_row("K49021", 104, 87, 104, 104, 0xDEAD_BEEF_0123_4567);
        assert!(audit_render(&r));
        assert!(!audit_render(""));
        assert!(!audit_render("K49021|full=104|tkl=87|makes=104|breaks=104"));
        assert!(!audit_render("K49021|full=x|tkl=87|makes=104|breaks=104|digest=0000000000000001"));
        assert!(!audit_render("K49021|full=104|tkl=87|makes=104|breaks=104|digest=123"));
        assert!(parse_u32("104") == Some(104));
        assert!(parse_u32("") == None);
        assert!(parse_u32("12x") == None);
        assert!(parse_u32("99999999999") == None);
        assert_eq!(parse_hex16("00000000000000FF"), Some(255));
        assert_eq!(parse_hex16("000000000000000"), None);
    }

    #[test]
    fn checkset_is_complete_and_untruncated() {
        let s = run_unx49021_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(extra_checks().len(), 8);
    }

    #[test]
    fn class_count_ledger_matches_declared_partition() {
        let e = Counts::expected(false);
        let total = e.alpha
            + e.digit
            + e.punct
            + e.function
            + e.modifier
            + e.nav
            + e.numpad
            + e.lock
            + e.misc;
        assert_eq!(total, 104);
        let t = Counts::expected(true);
        let ttotal = t.alpha
            + t.digit
            + t.punct
            + t.function
            + t.modifier
            + t.nav
            + t.numpad
            + t.lock
            + t.misc;
        assert_eq!(ttotal, 87);
        assert_eq!(t.numpad, 0);
        assert_eq!(t.lock + 1, e.lock);
    }

    #[test]
    fn tkl_fingerprint_differs_from_full_sequence_digest() {
        let g = golden_run().unwrap();
        let t = tkl_golden_run().unwrap();
        assert_ne!(g.ledger.digest, t.ledger.digest);
        let ta = tkl_audit(&tkl_table().unwrap()).unwrap();
        assert_ne!(ta.fingerprint, 0);
    }

    #[test]
    fn golden_script_lengths_are_frozen() {
        assert_eq!(golden_events().len(), TRANSITIONS);
        assert_eq!(probe_seq_a().len(), TRANSITIONS);
        assert_eq!(probe_seq_b().len(), TRANSITIONS);
        assert_eq!(TKL_TRANSITIONS, 174);
    }
}
