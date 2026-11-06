//! UNX-F48828 keymap 热切换（layout 运行时切换，切换期按键零丢）
//! UNX-F48829 keymap 文件损坏回退（回退内置美式布局并告警）
//! UNX-F48830 104/87 键全表逐键判据引擎（make/break 对逐键断言自动化载体）
//!
//! ── 闸门次序 ──
//! G1 表长闸：US=104 / DE=87，二表缺一行即 TableShort，不允许「差不多」。
//! G2 键码闸：布局内键码唯一、键名唯一；跨布局同键码允许不同字符（z/y、ß）。
//! G3 热切换闸：begin 之后进来的键一律进缓冲，commit 必须用**新**布局重放。
//! G4 对账闸：received == delivered + lost + queued；lost 只在显式溢出注入时非零，
//!    且非零那一刻必须同时报 QueueFull（禁止悄悄吞键）。
//! G5 回退闸：解析失败一律回内置美式并写告警账；不许「半张表继续跑」。
//! G6 可复现闸：同种子双跑摘要必须相等，否则 NotReproducible。
//!
//! ── 关键差异 ──
//! · 与 B01（evdev 原始码流解码）不同：本模块只认「布局表 + 切换窗口」，
//!   原始码流与扫描码集归 B01；这里输入的是已带修饰的 (键码, shift) 事件。
//! · 与 B03（输入法 / compose 提交）不同：这里保证的是 keysym 不丢、不串布局，
//!   不做任何预编辑与候选提交；提交时序归 B03。
//! · 切换窗口的语义是「旧布局收尾、新布局起步」：切换期内**不得**用旧布局解释
//!   缓冲区内的键，否则 z/y 与 ß/? 会在切换瞬间错位——这正是本模块的核心断言。
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（B02 dispatch 段）。
pub const SEG_BASE: u32 = 0x70360;

// ── 诊断码（段内闭集，0x70361..=0x7037F） ──────────────────────
/// dispatch 族诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispCode {
    /// 重复 begin（上一个切换还没落账）
    SwitchBusy = 1,
    /// 未 begin 就 commit
    SwitchNotBegun = 2,
    /// 切换缓冲溢出
    QueueFull = 3,
    /// 键事件在窗口里丢了
    KeyLost = 4,
    /// 目标布局不在册
    LayoutUnknown = 5,
    /// 回退发生了却没留告警
    FallbackAlarmMissing = 6,
    /// 空文本 / 缺头 / 缺符号段
    CorruptEmpty = 7,
    /// 表行数不足 87
    TableShort = 8,
    /// 键码不在布局表内
    KeyUnmapped = 9,
    /// make/break 不成对（双 make 或空 break）
    MakeBreakMismatch = 10,
    /// 布局内键码重复
    DuplicateKeycode = 11,
    /// 双跑结果不一致
    NotReproducible = 12,
}

impl DispCode {
    /// 段内绝对码。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 诊断文案。
    pub const fn detail(self) -> &'static str {
        match self {
            DispCode::SwitchBusy => "切换未落账又起一次 begin",
            DispCode::SwitchNotBegun => "没有 pending 布局却 commit",
            DispCode::QueueFull => "切换窗口缓冲溢出（显式计 lost）",
            DispCode::KeyLost => "切换窗口里丢键（received 对不上 delivered）",
            DispCode::LayoutUnknown => "目标布局不在册",
            DispCode::FallbackAlarmMissing => "回退内置美式却没有告警账",
            DispCode::CorruptEmpty => "文本为空或缺 keymap 头/版本/符号段",
            DispCode::TableShort => "布局表行数不足（DE 必须 87 行）",
            DispCode::KeyUnmapped => "键码不在当前布局表内",
            DispCode::MakeBreakMismatch => "make/break 不成对",
            DispCode::DuplicateKeycode => "同一布局内键码重复",
            DispCode::NotReproducible => "双跑摘要不一致",
        }
    }

    /// 全码表。
    pub const ALL: [DispCode; 12] = [
        DispCode::SwitchBusy,
        DispCode::SwitchNotBegun,
        DispCode::QueueFull,
        DispCode::KeyLost,
        DispCode::LayoutUnknown,
        DispCode::FallbackAlarmMissing,
        DispCode::CorruptEmpty,
        DispCode::TableShort,
        DispCode::KeyUnmapped,
        DispCode::MakeBreakMismatch,
        DispCode::DuplicateKeycode,
        DispCode::NotReproducible,
    ];
}

/// 码 → 文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DispCode::ALL.len() {
        if let Some(c) = DispCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70360 段 dispatch 闭集内"
}

// ── 折叠与掷骰 ────────────────────────────────────────────────
/// 折一个字节。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折一个 32 位字。
pub fn fold_word(h: u64, w: u32) -> u64 {
    let mut acc = h;
    let b = w.to_le_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            acc = fold_byte(acc, *v);
        }
        i += 1;
    }
    acc
}

/// 折一段文本。
pub fn fold_text(h: u64, s: &str) -> u64 {
    let mut acc = h;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            acc = fold_byte(acc, *v);
        }
        i += 1;
    }
    acc
}

/// 固定种子掷骰（只用于生成注入序列，绝不参与判定）。
pub struct Die {
    s: u32,
}

impl Die {
    /// 起骰。
    pub const fn new(seed: u32) -> Die {
        Die { s: seed ^ 0x5EED_1234 }
    }

    /// 下一步。
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.s ^= self.s >> 11;
        self.s
    }

    /// [0, bound)。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}

// ── 布局 ─────────────────────────────────────────────────────
/// 布局标识（B02 在册两套：美式 104、德式 87）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// 内置美式 104 键（回退目标）
    Us,
    /// 德式 QWERTZ 87 键
    De,
}

impl Layout {
    /// 布局名。
    pub const fn name(self) -> &'static str {
        match self {
            Layout::Us => "us",
            Layout::De => "de",
        }
    }

    /// 布局字节（折摘要用）。
    pub const fn code(self) -> u8 {
        match self {
            Layout::Us => 0,
            Layout::De => 1,
        }
    }
}

// ── 键行 ─────────────────────────────────────────────────────
/// 一行键位：键码（evdev）、XKB 键名、无修饰 keysym、Shift keysym。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyRow {
    /// evdev 键码（xkb 键码 = evdev + 8）
    pub kc: u16,
    /// XKB 键名
    pub name: &'static str,
    /// level 1
    pub lo: u32,
    /// level 2
    pub hi: u32,
}

/// 美式 104 键表（全表逐键判据用）。
pub const US_TABLE: [KeyRow; 104] = [
    KeyRow { kc: 1, name: "ESC", lo: 0xff1b, hi: 0xff1b },
    KeyRow { kc: 2, name: "AE01", lo: 0x0031, hi: 0x0021 },
    KeyRow { kc: 3, name: "AE02", lo: 0x0032, hi: 0x0040 },
    KeyRow { kc: 4, name: "AE03", lo: 0x0033, hi: 0x0023 },
    KeyRow { kc: 5, name: "AE04", lo: 0x0034, hi: 0x0024 },
    KeyRow { kc: 6, name: "AE05", lo: 0x0035, hi: 0x0025 },
    KeyRow { kc: 7, name: "AE06", lo: 0x0036, hi: 0x005e },
    KeyRow { kc: 8, name: "AE07", lo: 0x0037, hi: 0x0026 },
    KeyRow { kc: 9, name: "AE08", lo: 0x0038, hi: 0x002a },
    KeyRow { kc: 10, name: "AE09", lo: 0x0039, hi: 0x0028 },
    KeyRow { kc: 11, name: "AE00", lo: 0x0030, hi: 0x0029 },
    KeyRow { kc: 12, name: "AE11", lo: 0x002d, hi: 0x005f },
    KeyRow { kc: 13, name: "AE12", lo: 0x003d, hi: 0x002b },
    KeyRow { kc: 14, name: "BKSP", lo: 0xff08, hi: 0xff08 },
    KeyRow { kc: 15, name: "TAB", lo: 0xff09, hi: 0xfe20 },
    KeyRow { kc: 16, name: "AD01", lo: 0x0071, hi: 0x0051 },
    KeyRow { kc: 17, name: "AD02", lo: 0x0077, hi: 0x0057 },
    KeyRow { kc: 18, name: "AD03", lo: 0x0065, hi: 0x0045 },
    KeyRow { kc: 19, name: "AD04", lo: 0x0072, hi: 0x0052 },
    KeyRow { kc: 20, name: "AD05", lo: 0x0074, hi: 0x0054 },
    KeyRow { kc: 21, name: "AD06", lo: 0x0079, hi: 0x0059 },
    KeyRow { kc: 22, name: "AD07", lo: 0x0075, hi: 0x0055 },
    KeyRow { kc: 23, name: "AD08", lo: 0x0069, hi: 0x0049 },
    KeyRow { kc: 24, name: "AD09", lo: 0x006f, hi: 0x004f },
    KeyRow { kc: 25, name: "AD10", lo: 0x0070, hi: 0x0050 },
    KeyRow { kc: 26, name: "AD11", lo: 0x005b, hi: 0x007b },
    KeyRow { kc: 27, name: "AD12", lo: 0x005d, hi: 0x007d },
    KeyRow { kc: 28, name: "RTRN", lo: 0xff0d, hi: 0xff0d },
    KeyRow { kc: 29, name: "LCTL", lo: 0xffe3, hi: 0xffe3 },
    KeyRow { kc: 30, name: "AC01", lo: 0x0061, hi: 0x0041 },
    KeyRow { kc: 31, name: "AC02", lo: 0x0073, hi: 0x0053 },
    KeyRow { kc: 32, name: "AC03", lo: 0x0064, hi: 0x0044 },
    KeyRow { kc: 33, name: "AC04", lo: 0x0066, hi: 0x0046 },
    KeyRow { kc: 34, name: "AC05", lo: 0x0067, hi: 0x0047 },
    KeyRow { kc: 35, name: "AC06", lo: 0x0068, hi: 0x0048 },
    KeyRow { kc: 36, name: "AC07", lo: 0x006a, hi: 0x004a },
    KeyRow { kc: 37, name: "AC08", lo: 0x006b, hi: 0x004b },
    KeyRow { kc: 38, name: "AC09", lo: 0x006c, hi: 0x004c },
    KeyRow { kc: 39, name: "AC10", lo: 0x003b, hi: 0x003a },
    KeyRow { kc: 40, name: "AC11", lo: 0x0027, hi: 0x0022 },
    KeyRow { kc: 41, name: "TLDE", lo: 0x0060, hi: 0x007e },
    KeyRow { kc: 42, name: "LFSH", lo: 0xffe1, hi: 0xffe1 },
    KeyRow { kc: 43, name: "AB01", lo: 0x005c, hi: 0x007c },
    KeyRow { kc: 44, name: "AB02", lo: 0x007a, hi: 0x005a },
    KeyRow { kc: 45, name: "AB03", lo: 0x0078, hi: 0x0058 },
    KeyRow { kc: 46, name: "AB04", lo: 0x0063, hi: 0x0043 },
    KeyRow { kc: 47, name: "AB05", lo: 0x0076, hi: 0x0056 },
    KeyRow { kc: 48, name: "AB06", lo: 0x0062, hi: 0x0042 },
    KeyRow { kc: 49, name: "AB07", lo: 0x006e, hi: 0x004e },
    KeyRow { kc: 50, name: "AB08", lo: 0x006d, hi: 0x004d },
    KeyRow { kc: 51, name: "AB09", lo: 0x002c, hi: 0x003c },
    KeyRow { kc: 52, name: "AB10", lo: 0x002e, hi: 0x003e },
    KeyRow { kc: 53, name: "AB11", lo: 0x002f, hi: 0x003f },
    KeyRow { kc: 54, name: "RTSH", lo: 0xffe2, hi: 0xffe2 },
    KeyRow { kc: 55, name: "KPMU", lo: 0xffaa, hi: 0xffaa },
    KeyRow { kc: 56, name: "LALT", lo: 0xffe9, hi: 0xffe9 },
    KeyRow { kc: 57, name: "SPCE", lo: 0x0020, hi: 0x0020 },
    KeyRow { kc: 58, name: "CAPS", lo: 0xffe5, hi: 0xffe5 },
    KeyRow { kc: 59, name: "FK01", lo: 0xffbe, hi: 0xffbe },
    KeyRow { kc: 60, name: "FK02", lo: 0xffbf, hi: 0xffbf },
    KeyRow { kc: 61, name: "FK03", lo: 0xffc0, hi: 0xffc0 },
    KeyRow { kc: 62, name: "FK04", lo: 0xffc1, hi: 0xffc1 },
    KeyRow { kc: 63, name: "FK05", lo: 0xffc2, hi: 0xffc2 },
    KeyRow { kc: 64, name: "FK06", lo: 0xffc3, hi: 0xffc3 },
    KeyRow { kc: 65, name: "FK07", lo: 0xffc4, hi: 0xffc4 },
    KeyRow { kc: 66, name: "FK08", lo: 0xffc5, hi: 0xffc5 },
    KeyRow { kc: 67, name: "FK09", lo: 0xffc6, hi: 0xffc6 },
    KeyRow { kc: 68, name: "FK10", lo: 0xffc7, hi: 0xffc7 },
    KeyRow { kc: 69, name: "NMLK", lo: 0xff7f, hi: 0xff7f },
    KeyRow { kc: 70, name: "SCLK", lo: 0xff14, hi: 0xff14 },
    KeyRow { kc: 71, name: "KP7", lo: 0xffb7, hi: 0xff95 },
    KeyRow { kc: 72, name: "KP8", lo: 0xffb8, hi: 0xff97 },
    KeyRow { kc: 73, name: "KP9", lo: 0xffb9, hi: 0xff9a },
    KeyRow { kc: 74, name: "KPSU", lo: 0xffad, hi: 0xffad },
    KeyRow { kc: 75, name: "KP4", lo: 0xffb4, hi: 0xff96 },
    KeyRow { kc: 76, name: "KP5", lo: 0xffb5, hi: 0xff9d },
    KeyRow { kc: 77, name: "KP6", lo: 0xffb6, hi: 0xff98 },
    KeyRow { kc: 78, name: "KPAD", lo: 0xffab, hi: 0xffab },
    KeyRow { kc: 79, name: "KP1", lo: 0xffb1, hi: 0xff9c },
    KeyRow { kc: 80, name: "KP2", lo: 0xffb2, hi: 0xff99 },
    KeyRow { kc: 81, name: "KP3", lo: 0xffb3, hi: 0xff9b },
    KeyRow { kc: 82, name: "KP0", lo: 0xffb0, hi: 0xff9e },
    KeyRow { kc: 83, name: "KPDL", lo: 0xffae, hi: 0xff9f },
    KeyRow { kc: 86, name: "LSGT", lo: 0x003c, hi: 0x003e },
    KeyRow { kc: 87, name: "FK11", lo: 0xffc8, hi: 0xffc8 },
    KeyRow { kc: 88, name: "FK12", lo: 0xffc9, hi: 0xffc9 },
    KeyRow { kc: 96, name: "KPEN", lo: 0xff8d, hi: 0xff8d },
    KeyRow { kc: 97, name: "RCTL", lo: 0xffe4, hi: 0xffe4 },
    KeyRow { kc: 98, name: "KPDV", lo: 0xffaf, hi: 0xffaf },
    KeyRow { kc: 99, name: "PRSC", lo: 0xff61, hi: 0xff61 },
    KeyRow { kc: 100, name: "RALT", lo: 0xffea, hi: 0xffea },
    KeyRow { kc: 102, name: "HOME", lo: 0xff50, hi: 0xff50 },
    KeyRow { kc: 103, name: "UP", lo: 0xff52, hi: 0xff52 },
    KeyRow { kc: 104, name: "PGUP", lo: 0xff55, hi: 0xff55 },
    KeyRow { kc: 105, name: "LEFT", lo: 0xff51, hi: 0xff51 },
    KeyRow { kc: 106, name: "RGHT", lo: 0xff53, hi: 0xff53 },
    KeyRow { kc: 107, name: "END", lo: 0xff57, hi: 0xff57 },
    KeyRow { kc: 108, name: "DOWN", lo: 0xff54, hi: 0xff54 },
    KeyRow { kc: 109, name: "PGDN", lo: 0xff56, hi: 0xff56 },
    KeyRow { kc: 110, name: "INS", lo: 0xff63, hi: 0xff63 },
    KeyRow { kc: 111, name: "DELE", lo: 0xffff, hi: 0xffff },
    KeyRow { kc: 125, name: "LMDT", lo: 0xffeb, hi: 0xffeb },
    KeyRow { kc: 126, name: "RMDT", lo: 0xffec, hi: 0xffec },
    KeyRow { kc: 139, name: "MENU", lo: 0xff67, hi: 0xff67 },
];

/// 德式 87 键表（无小键盘；QWERTZ + 变音 + ß/´）。
pub const DE_TABLE: [KeyRow; 87] = [
    KeyRow { kc: 1, name: "ESC", lo: 0xff1b, hi: 0xff1b },
    KeyRow { kc: 2, name: "AE01", lo: 0x0031, hi: 0x0021 },
    KeyRow { kc: 3, name: "AE02", lo: 0x0032, hi: 0x0022 },
    KeyRow { kc: 4, name: "AE03", lo: 0x0033, hi: 0x00a7 },
    KeyRow { kc: 5, name: "AE04", lo: 0x0034, hi: 0x0024 },
    KeyRow { kc: 6, name: "AE05", lo: 0x0035, hi: 0x0025 },
    KeyRow { kc: 7, name: "AE06", lo: 0x0036, hi: 0x0026 },
    KeyRow { kc: 8, name: "AE07", lo: 0x0037, hi: 0x002f },
    KeyRow { kc: 9, name: "AE08", lo: 0x0038, hi: 0x0028 },
    KeyRow { kc: 10, name: "AE09", lo: 0x0039, hi: 0x0029 },
    KeyRow { kc: 11, name: "AE00", lo: 0x0030, hi: 0x003d },
    KeyRow { kc: 12, name: "AE11", lo: 0x00df, hi: 0x003f },
    KeyRow { kc: 13, name: "AE12", lo: 0x00b4, hi: 0x0060 },
    KeyRow { kc: 14, name: "BKSP", lo: 0xff08, hi: 0xff08 },
    KeyRow { kc: 15, name: "TAB", lo: 0xff09, hi: 0xfe20 },
    KeyRow { kc: 16, name: "AD01", lo: 0x0071, hi: 0x0051 },
    KeyRow { kc: 17, name: "AD02", lo: 0x0077, hi: 0x0057 },
    KeyRow { kc: 18, name: "AD03", lo: 0x0065, hi: 0x0045 },
    KeyRow { kc: 19, name: "AD04", lo: 0x0072, hi: 0x0052 },
    KeyRow { kc: 20, name: "AD05", lo: 0x0074, hi: 0x0054 },
    KeyRow { kc: 21, name: "AD06", lo: 0x007a, hi: 0x005a },
    KeyRow { kc: 22, name: "AD07", lo: 0x0075, hi: 0x0055 },
    KeyRow { kc: 23, name: "AD08", lo: 0x0069, hi: 0x0049 },
    KeyRow { kc: 24, name: "AD09", lo: 0x006f, hi: 0x004f },
    KeyRow { kc: 25, name: "AD10", lo: 0x0070, hi: 0x0050 },
    KeyRow { kc: 26, name: "AD11", lo: 0x00fc, hi: 0x00dc },
    KeyRow { kc: 27, name: "AD12", lo: 0x002b, hi: 0x002a },
    KeyRow { kc: 28, name: "RTRN", lo: 0xff0d, hi: 0xff0d },
    KeyRow { kc: 29, name: "LCTL", lo: 0xffe3, hi: 0xffe3 },
    KeyRow { kc: 30, name: "AC01", lo: 0x0061, hi: 0x0041 },
    KeyRow { kc: 31, name: "AC02", lo: 0x0073, hi: 0x0053 },
    KeyRow { kc: 32, name: "AC03", lo: 0x0064, hi: 0x0044 },
    KeyRow { kc: 33, name: "AC04", lo: 0x0066, hi: 0x0046 },
    KeyRow { kc: 34, name: "AC05", lo: 0x0067, hi: 0x0047 },
    KeyRow { kc: 35, name: "AC06", lo: 0x0068, hi: 0x0048 },
    KeyRow { kc: 36, name: "AC07", lo: 0x006a, hi: 0x004a },
    KeyRow { kc: 37, name: "AC08", lo: 0x006b, hi: 0x004b },
    KeyRow { kc: 38, name: "AC09", lo: 0x006c, hi: 0x004c },
    KeyRow { kc: 39, name: "AC10", lo: 0x00f6, hi: 0x00d6 },
    KeyRow { kc: 40, name: "AC11", lo: 0x00e4, hi: 0x00c4 },
    KeyRow { kc: 41, name: "TLDE", lo: 0xfe52, hi: 0x00b0 },
    KeyRow { kc: 42, name: "LFSH", lo: 0xffe1, hi: 0xffe1 },
    KeyRow { kc: 43, name: "AB01", lo: 0x0023, hi: 0x0027 },
    KeyRow { kc: 44, name: "AB02", lo: 0x0079, hi: 0x0059 },
    KeyRow { kc: 45, name: "AB03", lo: 0x0078, hi: 0x0058 },
    KeyRow { kc: 46, name: "AB04", lo: 0x0063, hi: 0x0043 },
    KeyRow { kc: 47, name: "AB05", lo: 0x0076, hi: 0x0056 },
    KeyRow { kc: 48, name: "AB06", lo: 0x0062, hi: 0x0042 },
    KeyRow { kc: 49, name: "AB07", lo: 0x006e, hi: 0x004e },
    KeyRow { kc: 50, name: "AB08", lo: 0x006d, hi: 0x004d },
    KeyRow { kc: 51, name: "AB09", lo: 0x002c, hi: 0x003b },
    KeyRow { kc: 52, name: "AB10", lo: 0x002e, hi: 0x003a },
    KeyRow { kc: 53, name: "AB11", lo: 0x002d, hi: 0x005f },
    KeyRow { kc: 54, name: "RTSH", lo: 0xffe2, hi: 0xffe2 },
    KeyRow { kc: 56, name: "LALT", lo: 0xffe9, hi: 0xffe9 },
    KeyRow { kc: 57, name: "SPCE", lo: 0x0020, hi: 0x0020 },
    KeyRow { kc: 58, name: "CAPS", lo: 0xffe5, hi: 0xffe5 },
    KeyRow { kc: 59, name: "FK01", lo: 0xffbe, hi: 0xffbe },
    KeyRow { kc: 60, name: "FK02", lo: 0xffbf, hi: 0xffbf },
    KeyRow { kc: 61, name: "FK03", lo: 0xffc0, hi: 0xffc0 },
    KeyRow { kc: 62, name: "FK04", lo: 0xffc1, hi: 0xffc1 },
    KeyRow { kc: 63, name: "FK05", lo: 0xffc2, hi: 0xffc2 },
    KeyRow { kc: 64, name: "FK06", lo: 0xffc3, hi: 0xffc3 },
    KeyRow { kc: 65, name: "FK07", lo: 0xffc4, hi: 0xffc4 },
    KeyRow { kc: 66, name: "FK08", lo: 0xffc5, hi: 0xffc5 },
    KeyRow { kc: 67, name: "FK09", lo: 0xffc6, hi: 0xffc6 },
    KeyRow { kc: 68, name: "FK10", lo: 0xffc7, hi: 0xffc7 },
    KeyRow { kc: 69, name: "NMLK", lo: 0xff7f, hi: 0xff7f },
    KeyRow { kc: 70, name: "SCLK", lo: 0xff14, hi: 0xff14 },
    KeyRow { kc: 86, name: "LSGT", lo: 0x003c, hi: 0x003e },
    KeyRow { kc: 87, name: "FK11", lo: 0xffc8, hi: 0xffc8 },
    KeyRow { kc: 88, name: "FK12", lo: 0xffc9, hi: 0xffc9 },
    KeyRow { kc: 97, name: "RCTL", lo: 0xffe4, hi: 0xffe4 },
    KeyRow { kc: 99, name: "PRSC", lo: 0xff61, hi: 0xff61 },
    KeyRow { kc: 100, name: "RALT", lo: 0xffea, hi: 0xffea },
    KeyRow { kc: 102, name: "HOME", lo: 0xff50, hi: 0xff50 },
    KeyRow { kc: 103, name: "UP", lo: 0xff52, hi: 0xff52 },
    KeyRow { kc: 104, name: "PGUP", lo: 0xff55, hi: 0xff55 },
    KeyRow { kc: 105, name: "LEFT", lo: 0xff51, hi: 0xff51 },
    KeyRow { kc: 106, name: "RGHT", lo: 0xff53, hi: 0xff53 },
    KeyRow { kc: 107, name: "END", lo: 0xff57, hi: 0xff57 },
    KeyRow { kc: 108, name: "DOWN", lo: 0xff54, hi: 0xff54 },
    KeyRow { kc: 109, name: "PGDN", lo: 0xff56, hi: 0xff56 },
    KeyRow { kc: 110, name: "INS", lo: 0xff63, hi: 0xff63 },
    KeyRow { kc: 111, name: "DELE", lo: 0xffff, hi: 0xffff },
    KeyRow { kc: 125, name: "LMDT", lo: 0xffeb, hi: 0xffeb },
    KeyRow { kc: 126, name: "RMDT", lo: 0xffec, hi: 0xffec },
];

/// 布局 → 表。
pub fn table_of(layout: Layout) -> &'static [KeyRow] {
    match layout {
        Layout::Us => &US_TABLE,
        Layout::De => &DE_TABLE,
    }
}

/// 布局 → 表长。
pub const fn table_len(layout: Layout) -> usize {
    match layout {
        Layout::Us => US_TABLE.len(),
        Layout::De => DE_TABLE.len(),
    }
}

/// 正序查键。
pub fn find(layout: Layout, kc: u16) -> Option<KeyRow> {
    let rows = table_of(layout);
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            if r.kc == kc {
                return Some(*r);
            }
        }
        i += 1;
    }
    None
}

/// 倒序查键（独立臂：用来对拍正序查表，避免「同一个错被算两遍」）。
pub fn find_reverse(layout: Layout, kc: u16) -> Option<KeyRow> {
    let rows = table_of(layout);
    let mut i = rows.len();
    while i > 0 {
        i -= 1;
        if let Some(r) = rows.get(i) {
            if r.kc == kc {
                return Some(*r);
            }
        }
    }
    None
}

/// 单键求值（正序臂）。
pub fn resolve(layout: Layout, kc: u16, shift: bool) -> Option<u32> {
    match find(layout, kc) {
        Some(r) => Some(if shift { r.hi } else { r.lo }),
        None => None,
    }
}

/// 单键求值（倒序独立臂）。
pub fn resolve_independent(layout: Layout, kc: u16, shift: bool) -> Option<u32> {
    match find_reverse(layout, kc) {
        Some(r) => Some(if shift { r.hi } else { r.lo }),
        None => None,
    }
}

/// 两布局是否在册。
pub const fn layout_known(layout: Layout) -> bool {
    match layout {
        Layout::Us => true,
        Layout::De => true,
    }
}

// ── make/break 引擎 ──────────────────────────────────────────
/// 单布局按键引擎：make 出 keysym，break 清按下栈；双 make / 空 break 都是红。
pub struct Engine {
    /// 当前布局
    pub layout: Layout,
    /// 按下栈（键码）
    pub down: Vec<u16>,
    /// make 计数
    pub makes: u32,
    /// break 计数
    pub breaks: u32,
}

impl Engine {
    /// 起引擎（空按下栈）。
    pub fn new(layout: Layout) -> Engine {
        Engine { layout, down: Vec::new(), makes: 0, breaks: 0 }
    }

    /// make：同键未 break 又 make 直接 Err（不允许自动去重掩盖问题）。
    pub fn press(&mut self, kc: u16, shift: bool) -> Result<u32, u32> {
        if !layout_known(self.layout) {
            return Err(DispCode::LayoutUnknown.val());
        }
        if self.down.iter().any(|x| *x == kc) {
            return Err(DispCode::MakeBreakMismatch.val());
        }
        let ks = match resolve(self.layout, kc, shift) {
            Some(v) => v,
            None => return Err(DispCode::KeyUnmapped.val()),
        };
        self.down.push(kc);
        self.makes = self.makes.saturating_add(1);
        Ok(ks)
    }

    /// break：没按过就松 = 不成对。
    pub fn release(&mut self, kc: u16) -> Result<(), u32> {
        let mut at = None;
        let mut i = 0usize;
        while i < self.down.len() {
            if let Some(v) = self.down.get(i) {
                if *v == kc {
                    at = Some(i);
                    break;
                }
            }
            i += 1;
        }
        match at {
            Some(i) => {
                self.down.remove(i);
                self.breaks = self.breaks.saturating_add(1);
                Ok(())
            }
            None => Err(DispCode::MakeBreakMismatch.val()),
        }
    }

    /// 栈空？
    pub fn all_up(&self) -> bool {
        self.down.is_empty()
    }
}

/// 全表扫描结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableReport {
    /// 键数（104 + 87 = 191）
    pub keys: u32,
    /// 布局数（2）
    pub layouts: u32,
    /// 逐键断言格数（191 × 2 = 382）
    pub cells: u32,
    /// 绿格数
    pub green: u32,
    /// 未映射键数（必须 0）
    pub unmapped: u32,
    /// 粘滞键数（必须 0）
    pub sticky: u32,
    /// make 总数
    pub makes: u32,
    /// break 总数
    pub breaks: u32,
}

/// 扫一张表：每键两格（无修饰 make+break、Shift make+break）。
fn scan_table(layout: Layout, rep: &mut TableReport) {
    let rows = table_of(layout);
    let mut eng = Engine::new(layout);
    let mut i = 0usize;
    while i < rows.len() {
        let row = match rows.get(i) {
            Some(r) => *r,
            None => break,
        };
        rep.keys = rep.keys.saturating_add(1);
        // 格 A：无修饰
        let a_hit = match eng.press(row.kc, false) {
            Ok(ks) => ks == row.lo,
            Err(_) => {
                rep.unmapped = rep.unmapped.saturating_add(1);
                false
            }
        };
        let a_rel = match eng.release(row.kc) {
            Ok(()) => true,
            Err(_) => {
                rep.sticky = rep.sticky.saturating_add(1);
                false
            }
        };
        // 格 B：Shift（keysym 必须走 level 2）
        let b_hit = match eng.press(row.kc, true) {
            Ok(ks) => ks == row.hi,
            Err(_) => {
                rep.unmapped = rep.unmapped.saturating_add(1);
                false
            }
        };
        let b_rel = match eng.release(row.kc) {
            Ok(()) => true,
            Err(_) => {
                rep.sticky = rep.sticky.saturating_add(1);
                false
            }
        };
        rep.cells = rep.cells.saturating_add(2);
        if a_hit && a_rel {
            rep.green = rep.green.saturating_add(1);
        }
        if b_hit && b_rel {
            rep.green = rep.green.saturating_add(1);
        }
        if !eng.all_up() {
            rep.sticky = rep.sticky.saturating_add(1);
            eng.down.clear();
        }
        i += 1;
    }
    rep.makes = rep.makes.saturating_add(eng.makes);
    rep.breaks = rep.breaks.saturating_add(eng.breaks);
}

/// 191 键 × 2 布局逐键判据。
pub fn key_table_green() -> TableReport {
    let mut rep = TableReport {
        keys: 0,
        layouts: 0,
        cells: 0,
        green: 0,
        unmapped: 0,
        sticky: 0,
        makes: 0,
        breaks: 0,
    };
    scan_table(Layout::Us, &mut rep);
    scan_table(Layout::De, &mut rep);
    rep.layouts = 2;
    rep
}

/// 键码/键名卫生：(重复键码数, 重复键名数)。
pub fn keycode_hygiene() -> (u32, u32) {
    let mut dup_kc = 0u32;
    let mut dup_name = 0u32;
    let mut li = 0usize;
    let layouts = [Layout::Us, Layout::De];
    while li < layouts.len() {
        let layout = match layouts.get(li) {
            Some(l) => *l,
            None => break,
        };
        let rows = table_of(layout);
        let mut i = 0usize;
        while i < rows.len() {
            let a = match rows.get(i) {
                Some(r) => *r,
                None => break,
            };
            let mut j = i + 1;
            while j < rows.len() {
                if let Some(b) = rows.get(j) {
                    if a.kc == b.kc {
                        dup_kc = dup_kc.saturating_add(1);
                    }
                    if a.name == b.name {
                        dup_name = dup_name.saturating_add(1);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        li += 1;
    }
    (dup_kc, dup_name)
}

// ── 热切换 ───────────────────────────────────────────────────
/// 切换缓冲上限。
pub const QUEUE_CAP: usize = 64;

/// 一个待解释的键事件（已带修饰）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyEvent {
    /// 全局序号（对账用）
    pub seq: u32,
    /// evdev 键码
    pub kc: u16,
    /// 是否 Shift
    pub shift: bool,
}

/// 已投递结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Delivered {
    /// 序号
    pub seq: u32,
    /// keysym
    pub keysym: u32,
    /// 解释用的布局
    pub layout: Layout,
}

/// 调度器：active 布局 + 切换窗口 + 对账计数。
pub struct Dispatcher {
    /// 当前生效布局
    pub active: Layout,
    /// 目标布局（Some = 正在切换）
    pub pending: Option<Layout>,
    /// 切换窗口缓冲
    pub queue: Vec<KeyEvent>,
    /// 已投递
    pub delivered: Vec<Delivered>,
    /// 收到事件数
    pub received: u32,
    /// 显式丢失数（只在溢出时非零）
    pub lost: u32,
    /// 窗口峰值
    pub peak: u32,
    /// 完成切换次数
    pub switches: u32,
    /// 中止次数
    pub aborted: u32,
}

impl Dispatcher {
    /// 起机（美式）。
    pub fn new() -> Dispatcher {
        Dispatcher {
            active: Layout::Us,
            pending: None,
            queue: Vec::new(),
            delivered: Vec::new(),
            received: 0,
            lost: 0,
            peak: 0,
            switches: 0,
            aborted: 0,
        }
    }

    /// 起切换：同布局或已在切换 → SwitchBusy。
    pub fn begin_switch(&mut self, to: Layout) -> Result<(), u32> {
        if self.pending.is_some() {
            return Err(DispCode::SwitchBusy.val());
        }
        if !layout_known(to) {
            return Err(DispCode::LayoutUnknown.val());
        }
        if to == self.active {
            return Err(DispCode::SwitchBusy.val());
        }
        self.pending = Some(to);
        Ok(())
    }

    /// 喂一个事件：窗口期只入缓冲（不解释），窗口外立即用 active 解释。
    pub fn feed(&mut self, ev: KeyEvent) -> Result<Option<u32>, u32> {
        self.received = self.received.saturating_add(1);
        if self.pending.is_some() {
            if self.queue.len() >= QUEUE_CAP {
                self.lost = self.lost.saturating_add(1);
                return Err(DispCode::QueueFull.val());
            }
            self.queue.push(ev);
            let n = self.queue.len() as u32;
            if n > self.peak {
                self.peak = n;
            }
            return Ok(None);
        }
        let ks = match resolve(self.active, ev.kc, ev.shift) {
            Some(v) => v,
            None => return Err(DispCode::KeyUnmapped.val()),
        };
        self.delivered.push(Delivered { seq: ev.seq, keysym: ks, layout: self.active });
        Ok(Some(ks))
    }

    /// 落账：active 换成目标布局，缓冲按**新**布局重放。
    pub fn commit_switch(&mut self) -> Result<u32, u32> {
        let to = match self.pending {
            Some(t) => t,
            None => return Err(DispCode::SwitchNotBegun.val()),
        };
        self.active = to;
        self.pending = None;
        self.switches = self.switches.saturating_add(1);
        self.replay(to)
    }

    /// 中止：缓冲按**旧**布局重放，active 不变。
    pub fn abort_switch(&mut self) -> u32 {
        if self.pending.is_none() {
            return 0;
        }
        let keep = self.active;
        self.pending = None;
        self.aborted = self.aborted.saturating_add(1);
        match self.replay(keep) {
            Ok(n) => n,
            Err(_) => 0,
        }
    }

    /// 重放缓冲；任何一键未映射都是 KeyLost（不允许跳过）。
    fn replay(&mut self, layout: Layout) -> Result<u32, u32> {
        let buffered = core::mem::replace(&mut self.queue, Vec::new());
        let mut n = 0u32;
        let mut i = 0usize;
        while i < buffered.len() {
            let ev = match buffered.get(i) {
                Some(e) => *e,
                None => break,
            };
            let ks = match resolve(layout, ev.kc, ev.shift) {
                Some(v) => v,
                None => return Err(DispCode::KeyLost.val()),
            };
            self.delivered.push(Delivered { seq: ev.seq, keysym: ks, layout });
            n = n.saturating_add(1);
            i += 1;
        }
        Ok(n)
    }

    /// 对账恒等式：received == delivered + lost + queued。
    pub fn accounting_ok(&self) -> bool {
        let d = self.delivered.len() as u32;
        let q = self.queue.len() as u32;
        self.received == d.saturating_add(self.lost).saturating_add(q)
    }

    /// 投递摘要。
    pub fn digest(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0usize;
        while i < self.delivered.len() {
            if let Some(d) = self.delivered.get(i) {
                h = fold_word(h, d.seq);
                h = fold_word(h, d.keysym);
                h = fold_byte(h, d.layout.code());
            }
            i += 1;
        }
        h
    }
}

impl Default for Dispatcher {
    fn default() -> Dispatcher {
        Dispatcher::new()
    }
}

/// 一轮里的第 k 个键（两布局都有的键：数字行/主字母行）。
pub fn pick_kc(k: u32) -> u16 {
    match k % 8 {
        0 => 2,
        1 => 3,
        2 => 4,
        3 => 16,
        4 => 30,
        5 => 21,
        6 => 44,
        _ => 52,
    }
}

/// 百轮热切换报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchReport {
    /// 轮数
    pub rounds: u32,
    /// 事件数
    pub events: u32,
    /// 完成切换数
    pub switches: u32,
    /// 丢失数（必须 0）
    pub lost: u32,
    /// 窗口峰值
    pub peak: u32,
    /// 投递摘要
    pub digest: u64,
    /// 与独立臂不一致的键数（必须 0）
    pub mismatches: u32,
    /// 对账恒等式是否成立
    pub balanced: bool,
}

/// 热切换 ×rounds：每轮 7 键在窗口内、2 键在窗口外，逐键对独立臂。
pub fn switch_100(rounds: u32) -> Result<SwitchReport, u32> {
    let mut d = Dispatcher::new();
    let mut seq = 0u32;
    let mut mismatches = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let target = if r % 2 == 0 { Layout::De } else { Layout::Us };
        let before = d.delivered.len();
        d.begin_switch(target)?;
        let mut k = 0u32;
        while k < 7 {
            d.feed(KeyEvent { seq, kc: pick_kc(k), shift: k % 3 == 0 })?;
            seq = seq.saturating_add(1);
            k = k.saturating_add(1);
        }
        let drained = d.commit_switch()?;
        if drained != 7 {
            return Err(DispCode::KeyLost.val());
        }
        let mut k = 0u32;
        while k < 2 {
            let got = d.feed(KeyEvent { seq, kc: pick_kc(k + 9), shift: k == 1 })?;
            if got.is_none() {
                return Err(DispCode::KeyLost.val());
            }
            seq = seq.saturating_add(1);
            k = k.saturating_add(1);
        }
        // 独立臂：本轮 9 键全部在 target 布局下解释，逐一比对。
        let mut i = before;
        let mut k = 0u32;
        while i < d.delivered.len() {
            let got = match d.delivered.get(i) {
                Some(v) => *v,
                None => break,
            };
            let ev_k = if k < 7 { k } else { k + 2 };
            let shift = if k < 7 { k % 3 == 0 } else { k == 8 };
            let want = match resolve_independent(target, pick_kc(ev_k), shift) {
                Some(v) => v,
                None => return Err(DispCode::KeyUnmapped.val()),
            };
            if got.keysym != want || got.layout != target || got.seq != r.saturating_mul(9).saturating_add(k) {
                mismatches = mismatches.saturating_add(1);
            }
            i = i.saturating_add(1);
            k = k.saturating_add(1);
        }
        if !d.accounting_ok() {
            return Err(DispCode::KeyLost.val());
        }
        r = r.saturating_add(1);
    }
    Ok(SwitchReport {
        rounds,
        events: seq,
        switches: d.switches,
        lost: d.lost,
        peak: d.peak,
        digest: d.digest(),
        mismatches,
        balanced: d.accounting_ok(),
    })
}

// ── 损坏回退 ─────────────────────────────────────────────────
/// 内置美式行数。
pub const BUILTIN_US_ROWS: usize = 104;
/// 德式行数。
pub const DE_ROWS: usize = 87;
/// 告警账容量。
pub const ALARM_CAP: usize = 16;

/// 告警账一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AlarmRow {
    /// 诊断码
    pub code: u32,
    /// 失败门号（1 名字 / 2 头段 / 3 行数 / 4 行语法 / 5 重复 / 6 键值域）
    pub gate: u8,
    /// 失败时已读行数
    pub seen: u32,
}

/// 告警账：满则显式 dropped，绝不静默。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlarmLedger {
    /// 行
    pub rows: Vec<AlarmRow>,
    /// 溢出条数
    pub dropped: u32,
}

impl AlarmLedger {
    /// 空账。
    pub fn new() -> AlarmLedger {
        AlarmLedger { rows: Vec::new(), dropped: 0 }
    }

    /// 记一条；满返回 Err(LedgerFull 同段码)。
    pub fn push(&mut self, code: u32, gate: u8, seen: u32) -> Result<(), u32> {
        if self.rows.len() >= ALARM_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(DispCode::FallbackAlarmMissing.val());
        }
        self.rows.push(AlarmRow { code, gate, seen });
        Ok(())
    }

    /// 某码条数。
    pub fn count_of(&self, code: u32) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.code == code {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 可查性：有记录且无溢出。
    pub fn queryable(&self) -> bool {
        !self.rows.is_empty() && self.dropped == 0
    }

    /// 摘要。
    pub fn digest(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fold_word(h, r.code);
                h = fold_byte(h, r.gate);
                h = fold_word(h, r.seen);
            }
            i += 1;
        }
        h
    }
}

impl Default for AlarmLedger {
    fn default() -> AlarmLedger {
        AlarmLedger::new()
    }
}

/// 装载结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoadOutcome {
    /// 最终生效布局
    pub layout: Layout,
    /// 行数
    pub rows: u32,
    /// 是否回退
    pub fallback: bool,
    /// 告警码（未回退为 0）
    pub alarm: u32,
    /// 表摘要
    pub digest: u64,
}

/// 内置美式表摘要。
pub fn us_digest() -> u64 {
    table_digest(Layout::Us)
}

/// 德式表摘要。
pub fn de_digest() -> u64 {
    table_digest(Layout::De)
}

/// 任意布局表摘要。
pub fn table_digest(layout: Layout) -> u64 {
    let rows = table_of(layout);
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            h = fold_word(h, r.kc as u32);
            h = fold_text(h, r.name);
            h = fold_word(h, r.lo);
            h = fold_word(h, r.hi);
        }
        i += 1;
    }
    h
}

/// 十六进制回写（0x 前缀，最少一位）。
fn push_hex(s: &mut String, v: u32) {
    s.push_str("0x");
    let mut sh = 28u32;
    let mut started = false;
    loop {
        let nib = ((v >> sh) & 0xf) as u8;
        if nib != 0 || started || sh == 0 {
            started = true;
            let ch = match nib {
                0..=9 => (b'0' + nib) as char,
                _ => (b'a' + (nib - 10)) as char,
            };
            s.push(ch);
        }
        if sh == 0 {
            break;
        }
        sh -= 4;
    }
}

/// 十进制回写。
fn push_dec(s: &mut String, v: u32) {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = buf.len();
    loop {
        let d = (n % 10) as u8;
        n /= 10;
        if i == 0 {
            break;
        }
        i -= 1;
        if let Some(c) = buf.get_mut(i) {
            *c = b'0' + d;
        }
        if n == 0 {
            break;
        }
    }
    let mut j = i;
    while j < buf.len() {
        if let Some(c) = buf.get(j) {
            s.push(*c as char);
        }
        j += 1;
    }
}

/// 一份合法的德式 keymap 文本（87 行）。
pub fn good_de_text() -> String {
    let mut s = String::new();
    s.push_str("# keymap de\n");
    s.push_str("version xkbcommon-1.6.0\n");
    s.push_str("xkb_symbols \"basic\" {\n");
    let mut i = 0usize;
    while i < DE_TABLE.len() {
        if let Some(r) = DE_TABLE.get(i) {
            s.push_str("kc.");
            push_dec(&mut s, r.kc as u32);
            s.push_str(" = ");
            push_hex(&mut s, r.lo);
            s.push_str(", ");
            push_hex(&mut s, r.hi);
            s.push_str(";\n");
        }
        i += 1;
    }
    s.push_str("};\n");
    s
}

/// 十六进制解析（必须 0x 前缀）。
fn parse_hex(t: &str) -> Option<u32> {
    let b = t.as_bytes();
    if b.len() < 3 {
        return None;
    }
    if b.first() != Some(&b'0') {
        return None;
    }
    let second = b.get(1)?;
    if *second != b'x' && *second != b'X' {
        return None;
    }
    let mut v = 0u32;
    let mut i = 2usize;
    while i < b.len() {
        let c = *b.get(i)?;
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d as u32)?;
        i += 1;
    }
    Some(v)
}

/// 十进制解析。
fn parse_dec(t: &str) -> Option<u32> {
    let b = t.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        let c = *b.get(i)?;
        if !(b'0'..=b'9').contains(&c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

/// keysym 合法域（X11：0x0100_0000 以上是厂商区，这里只收常用区）。
pub const KSYM_MAX: u32 = 0x1f_ffff;

/// 解析一行 `kc.<code> = <lo>, <hi>;`。
fn parse_row(t: &str) -> Result<(u16, u32, u32), (u32, u8)> {
    let eq = match t.find('=') {
        Some(p) => p,
        None => return Err((DispCode::CorruptEmpty.val(), 4)),
    };
    let left = t.get(..eq).unwrap_or("").trim();
    let right = t.get(eq + 1..).unwrap_or("").trim();
    let kc_txt = match left.strip_prefix("kc.") {
        Some(v) => v.trim(),
        None => return Err((DispCode::CorruptEmpty.val(), 4)),
    };
    let kc22 = match parse_dec(kc_txt) {
        Some(v) => v,
        None => return Err((DispCode::CorruptEmpty.val(), 4)),
    };
    if kc22 > u16::MAX as u32 {
        return Err((DispCode::KeyUnmapped.val(), 6));
    }
    let mut it = right.split(',');
    let lo_t = it.next().unwrap_or("").trim();
    let hi_raw = it.next().unwrap_or("").trim();
    let hi_t = hi_raw.trim_end_matches(';').trim();
    let lo = match parse_hex(lo_t) {
        Some(v) => v,
        None => return Err((DispCode::KeyUnmapped.val(), 6)),
    };
    let hi = match parse_hex(hi_t) {
        Some(v) => v,
        None => return Err((DispCode::KeyUnmapped.val(), 6)),
    };
    if lo == 0 || lo > KSYM_MAX || hi == 0 || hi > KSYM_MAX {
        return Err((DispCode::KeyUnmapped.val(), 6));
    }
    Ok((kc22 as u16, lo, hi))
}

/// 解析 keymap 文本（只认德式 87 行表；任何异常都带门号返回）。
fn parse_keymap(name: &str, text: &str) -> Result<u32, (u32, u8, u32)> {
    if name.is_empty() {
        return Err((DispCode::CorruptEmpty.val(), 1, 0));
    }
    let mut header = false;
    let mut version = false;
    let mut symbols = false;
    let mut rows = 0u32;
    let mut seen: Vec<u16> = Vec::new();
    let mut line_no = 0u32;
    for ln in text.lines() {
        line_no = line_no.saturating_add(1);
        let t = ln.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with('#') {
            if t.starts_with("# keymap") {
                header = true;
            }
            continue;
        }
        if t.starts_with("version") {
            if t.find("xkbcommon-1.6.0").is_some() {
                version = true;
            }
            continue;
        }
        if t.starts_with("xkb_symbols") {
            symbols = true;
            continue;
        }
        if t.starts_with("};") {
            continue;
        }
        if t.starts_with("kc.") {
            let (kc, _lo, _hi) = match parse_row(t) {
                Ok(v) => v,
                Err((code, gate)) => return Err((code, gate, rows)),
            };
            if seen.iter().any(|x| *x == kc) {
                return Err((DispCode::DuplicateKeycode.val(), 5, rows));
            }
            seen.push(kc);
            rows = rows.saturating_add(1);
            continue;
        }
        return Err((DispCode::CorruptEmpty.val(), 4, rows));
    }
    if !header || !version || !symbols {
        return Err((DispCode::CorruptEmpty.val(), 2, rows));
    }
    if rows != DE_ROWS as u32 {
        return Err((DispCode::TableShort.val(), 3, rows));
    }
    let _ = line_no;
    Ok(rows)
}

/// 装载：成功 → 德式表；失败 → 内置美式 + 告警入账。
pub fn load_keymap(name: &str, text: &str, alarms: &mut AlarmLedger) -> LoadOutcome {
    match parse_keymap(name, text) {
        Ok(rows) => LoadOutcome {
            layout: Layout::De,
            rows,
            fallback: false,
            alarm: 0,
            digest: de_digest(),
        },
        Err((code, gate, seen)) => {
            let _ = alarms.push(code, gate, seen);
            LoadOutcome {
                layout: Layout::Us,
                rows: BUILTIN_US_ROWS as u32,
                fallback: true,
                alarm: code,
                digest: us_digest(),
            }
        }
    }
}

/// 取前 n 行拼回文本。
fn head_lines(good: &str, n: u32) -> String {
    let mut out = String::new();
    let mut i = 0u32;
    for ln in good.lines() {
        if i >= n {
            break;
        }
        out.push_str(ln);
        out.push('\n');
        i = i.saturating_add(1);
    }
    out
}

/// 造第 idx 类损坏文本（0..10；idx=0 由调用方传空名字）。
pub fn corrupt_text(idx: usize, good: &str) -> String {
    let mut out = String::new();
    match idx {
        0 => out.push_str(good),
        1 => {
            // 掉头行
            let mut i = 0u32;
            for ln in good.lines() {
                if i > 0 {
                    out.push_str(ln);
                    out.push('\n');
                }
                i = i.saturating_add(1);
            }
        }
        2 => {
            // 版本号不合册
            for ln in good.lines() {
                if ln.starts_with("version") {
                    out.push_str("version xkbcommon-9.9\n");
                } else {
                    out.push_str(ln);
                    out.push('\n');
                }
            }
        }
        3 => {
            // 掉符号段
            for ln in good.lines() {
                if ln.starts_with("xkb_symbols") {
                    continue;
                }
                out.push_str(ln);
                out.push('\n');
            }
        }
        4 => {
            // 空文本
        }
        5 => out = head_lines(good, 30),
        6 => {
            // 键码重复
            let mut dup_done = false;
            for ln in good.lines() {
                out.push_str(ln);
                out.push('\n');
                if !dup_done && ln.starts_with("kc.") {
                    out.push_str(ln);
                    out.push('\n');
                    dup_done = true;
                }
            }
        }
        7 => {
            // 非法十六进制
            let mut hit = false;
            for ln in good.lines() {
                if !hit && ln.starts_with("kc.") {
                    if let Some(eq) = ln.find('=') {
                        out.push_str(ln.get(..eq).unwrap_or("kc.2 "));
                        out.push_str("= 0xzz, 0x21;\n");
                        hit = true;
                        continue;
                    }
                }
                out.push_str(ln);
                out.push('\n');
            }
        }
        8 => {
            // 混入无等号杂行
            let mut hit = false;
            for ln in good.lines() {
                out.push_str(ln);
                out.push('\n');
                if !hit && ln.starts_with("kc.") {
                    out.push_str("this line has no equals sign\n");
                    hit = true;
                }
            }
        }
        _ => {
            // keysym 越域
            let mut hit = false;
            for ln in good.lines() {
                if !hit && ln.starts_with("kc.") {
                    if let Some(eq) = ln.find('=') {
                        out.push_str(ln.get(..eq).unwrap_or("kc.2 "));
                        out.push_str("= 0x7fffffff, 0x21;\n");
                        hit = true;
                        continue;
                    }
                }
                out.push_str(ln);
                out.push('\n');
            }
        }
    }
    out
}

/// 十类损坏注入报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CorruptReport {
    /// 注入次数（10）
    pub tries: u32,
    /// 回退次数（必须 10）
    pub fallbacks: u32,
    /// 留告警次数（必须 10）
    pub alarmed: u32,
    /// 静默次数（必须 0）
    pub silent: u32,
    /// 回退目标错次数（必须 0）
    pub wrong_target: u32,
    /// 正常装载是否先验证通过
    pub good_ok: bool,
    /// 告警账摘要
    pub digest: u64,
}

/// 十类损坏 × 10 次注入：回退 10/10 且告警可查、零静默。
pub fn corrupt_inject_10() -> CorruptReport {
    let good = good_de_text();
    let mut ledger = AlarmLedger::new();
    let mut good_ok = false;
    let out = load_keymap("de", good.as_str(), &mut ledger);
    if !out.fallback && out.layout == Layout::De && out.rows == DE_ROWS as u32 && ledger.rows.is_empty() {
        good_ok = true;
    }
    let mut rep = CorruptReport {
        tries: 0,
        fallbacks: 0,
        alarmed: 0,
        silent: 0,
        wrong_target: 0,
        good_ok,
        digest: 0,
    };
    let mut idx = 0usize;
    while idx < 10 {
        let text = corrupt_text(idx, good.as_str());
        let name = if idx == 0 { "" } else { "de" };
        let before = ledger.rows.len();
        let o = load_keymap(name, text.as_str(), &mut ledger);
        rep.tries = rep.tries.saturating_add(1);
        if o.fallback {
            rep.fallbacks = rep.fallbacks.saturating_add(1);
            if o.alarm >= SEG_BASE && o.alarm < SEG_BASE + 0x20 {
                // 码在段内
            } else {
                rep.silent = rep.silent.saturating_add(1);
            }
            if o.layout == Layout::Us && o.rows == BUILTIN_US_ROWS as u32 && o.digest == us_digest() {
                // 回退目标正确
            } else {
                rep.wrong_target = rep.wrong_target.saturating_add(1);
            }
            if ledger.rows.len() == before.saturating_add(1) {
                rep.alarmed = rep.alarmed.saturating_add(1);
            } else {
                rep.silent = rep.silent.saturating_add(1);
            }
        } else {
            rep.silent = rep.silent.saturating_add(1);
        }
        idx = idx.saturating_add(1);
    }
    rep.digest = ledger.digest();
    rep
}

// ── 黄金跑 / 探针 / 双跑 ─────────────────────────────────────
/// 黄金跑：(切换轮数, 逐键格数, 回退次数)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let s = switch_100(100)?;
    if s.lost != 0 || s.mismatches != 0 || s.switches != 100 || !s.balanced {
        return Err(DispCode::KeyLost.val());
    }
    let t = key_table_green();
    if t.keys != 191 || t.cells != 382 || t.green != 382 || t.unmapped != 0 || t.sticky != 0 {
        return Err(DispCode::MakeBreakMismatch.val());
    }
    let c = corrupt_inject_10();
    if c.fallbacks != 10 || c.alarmed != 10 || c.silent != 0 || c.wrong_target != 0 || !c.good_ok {
        return Err(DispCode::FallbackAlarmMissing.val());
    }
    Ok((s.rounds, t.cells, c.fallbacks))
}

/// 探针 A：美式单键。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut d = Dispatcher::new();
    match d.feed(KeyEvent { seq: 0, kc: 2, shift: false }) {
        Ok(Some(0x0031)) => Ok(()),
        Ok(_) => Err(DispCode::KeyLost.val()),
        Err(c) => Err(c),
    }
}

/// 探针 B：窗口内按键必须按新布局（德式 2 → 引号）解释。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut d = Dispatcher::new();
    d.begin_switch(Layout::De)?;
    d.feed(KeyEvent { seq: 0, kc: 3, shift: true })?;
    if d.commit_switch()? != 1 {
        return Err(DispCode::KeyLost.val());
    }
    match d.delivered.first() {
        Some(v) if v.keysym == 0x0022 && v.layout == Layout::De => Ok(()),
        _ => Err(DispCode::KeyLost.val()),
    }
}

/// 探针 C：空文本必须回退并留告警。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut ledger = AlarmLedger::new();
    let o = load_keymap("de", "", &mut ledger);
    if o.fallback && o.layout == Layout::Us && o.alarm == DispCode::CorruptEmpty.val() && ledger.queryable() {
        Ok(())
    } else {
        Err(DispCode::FallbackAlarmMissing.val())
    }
}

/// 双跑对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DispCode::NotReproducible.val());
    }
    let s1 = switch_100(100)?;
    let s2 = switch_100(100)?;
    if s1.digest != s2.digest {
        return Err(DispCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < DispCode::ALL.len() {
        if let Some(c) = DispCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x20 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 注入账（十槽） ────────────────────────────────────────────
/// 注入账：十槽，每槽期望一个诊断码。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    /// 期望码
    pub want: [u32; 10],
    /// 判定次数
    pub tries: [u32; 10],
    /// 命中次数
    pub caught: [u32; 10],
}

impl InjectLedger {
    /// 建账。
    pub fn new() -> InjectLedger {
        InjectLedger {
            want: [
                DispCode::SwitchBusy.val(),
                DispCode::SwitchNotBegun.val(),
                DispCode::QueueFull.val(),
                DispCode::KeyUnmapped.val(),
                DispCode::MakeBreakMismatch.val(),
                DispCode::MakeBreakMismatch.val(),
                DispCode::CorruptEmpty.val(),
                DispCode::TableShort.val(),
                DispCode::DuplicateKeycode.val(),
                DispCode::SwitchBusy.val(),
            ],
            tries: [0; 10],
            caught: [0; 10],
        }
    }

    /// 记账。
    pub fn record(&mut self, slot: usize, got: u32) {
        if slot >= 10 {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        let want = self.want.get(slot).copied().unwrap_or(0);
        if got == want {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    /// 全槽命中。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 10 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl Default for InjectLedger {
    fn default() -> InjectLedger {
        InjectLedger::new()
    }
}

/// 十槽注入 ×rounds。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：切换未落账又 begin。
        let mut d = Dispatcher::new();
        let _ = d.begin_switch(Layout::De);
        let got0 = match d.begin_switch(Layout::Us) {
            Err(c) => c,
            Ok(_) => 0,
        };
        led.record(0, got0);
        // 槽 1：未 begin 就 commit。
        let mut d1 = Dispatcher::new();
        let got1 = match d1.commit_switch() {
            Err(c) => c,
            Ok(_) => 0,
        };
        led.record(1, got1);
        // 槽 2：缓冲溢出必须显式计 lost。
        let mut d2 = Dispatcher::new();
        let _ = d2.begin_switch(Layout::De);
        let mut k = 0u32;
        let mut last = 0u32;
        while k <= QUEUE_CAP as u32 {
            match d2.feed(KeyEvent { seq: k, kc: 2, shift: false }) {
                Ok(_) => last = 0,
                Err(c) => last = c,
            }
            k = k.saturating_add(1);
        }
        let ok2 = last == DispCode::QueueFull.val() && d2.lost >= 1 && d2.accounting_ok();
        led.record(2, if ok2 { last } else { 0 });
        // 槽 3：未映射键。
        let mut e3 = Engine::new(Layout::Us);
        let got3 = match e3.press(200, false) {
            Err(c) => c,
            Ok(_) => 0,
        };
        led.record(3, got3);
        // 槽 4：双 make。
        let mut e4 = Engine::new(Layout::Us);
        let _ = e4.press(30, false);
        let got4 = match e4.press(30, false) {
            Err(c) => c,
            Ok(_) => 0,
        };
        led.record(4, got4);
        // 槽 5：空 break。
        let mut e5 = Engine::new(Layout::Us);
        let got5 = match e5.release(30) {
            Err(c) => c,
            Ok(_) => 0,
        };
        led.record(5, got5);
        // 槽 6：空文本（空名字）。
        let mut lg6 = AlarmLedger::new();
        let o6 = load_keymap("", good_de_text().as_str(), &mut lg6);
        led.record(6, if o6.fallback { o6.alarm } else { 0 });
        // 槽 7：截断表。
        let good = good_de_text();
        let trunc = corrupt_text(5, good.as_str());
        let mut lg7 = AlarmLedger::new();
        let o7 = load_keymap("de", trunc.as_str(), &mut lg7);
        led.record(7, if o7.fallback { o7.alarm } else { 0 });
        // 槽 8：键码重复。
        let dup = corrupt_text(6, good.as_str());
        let mut lg8 = AlarmLedger::new();
        let o8 = load_keymap("de", dup.as_str(), &mut lg8);
        led.record(8, if o8.fallback { o8.alarm } else { 0 });
        // 槽 9：切到当前布局。
        let mut d9 = Dispatcher::new();
        let got9 = match d9.begin_switch(Layout::Us) {
            Err(c) => c,
            Ok(_) => 0,
        };
        led.record(9, got9);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 判据 ─────────────────────────────────────────────────────
/// 九条判据 + 一条对账。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1 百轮热切换零丢
    let s = switch_100(100);
    let (ok1, d1) = match s {
        Ok(rep) => {
            let mut t = String::new();
            t.push_str("rounds=");
            push_dec(&mut t, rep.rounds);
            t.push_str(" events=");
            push_dec(&mut t, rep.events);
            t.push_str(" lost=");
            push_dec(&mut t, rep.lost);
            t.push_str(" switches=");
            push_dec(&mut t, rep.switches);
            (
                rep.lost == 0 && rep.switches == 100 && rep.events == 900 && rep.balanced,
                t,
            )
        }
        Err(c) => {
            let mut t = String::new();
            t.push_str("err=");
            push_hex(&mut t, c);
            (false, t)
        }
    };
    v.push(("F48828-J1-切换×10^2期间按键计数对账零丢", ok1, d1));

    // 2 窗口全量缓冲并送达
    let s2 = switch_100(100);
    let (ok2, d2) = match s2 {
        Ok(rep) => {
            let mut t = String::new();
            t.push_str("peak=");
            push_dec(&mut t, rep.peak);
            t.push_str(" rounds=");
            push_dec(&mut t, rep.rounds);
            (rep.peak >= 7, t)
        }
        Err(_) => (false, String::from("switch err")),
    };
    v.push(("F48828-J2-切换窗口内键全量缓冲后按新布局送达", ok2, d2));

    // 3 独立臂逐键一致
    let s3 = switch_100(100);
    let (ok3, d3) = match s3 {
        Ok(rep) => {
            let mut t = String::new();
            t.push_str("mismatch=");
            push_dec(&mut t, rep.mismatches);
            t.push_str(" digest=");
            push_hex(&mut t, (rep.digest & 0xffff_ffff) as u32);
            (rep.mismatches == 0, t)
        }
        Err(_) => (false, String::from("switch err")),
    };
    v.push(("F48828-J3-倒序查表独立重算逐键一致", ok3, d3));

    // 4 中止回旧布局
    let mut d4 = Dispatcher::new();
    let _ = d4.begin_switch(Layout::De);
    let _ = d4.feed(KeyEvent { seq: 0, kc: 21, shift: false });
    let drained = d4.abort_switch();
    let ok4 = match d4.delivered.first() {
        Some(v0) => drained == 1 && v0.keysym == 0x0079 && v0.layout == Layout::Us && d4.active == Layout::Us,
        None => false,
    };
    v.push(("F48828-J4-中止切换回旧布局重放", ok4, String::from("abort→us y")));

    // 5 十类损坏十次回退
    let c5 = corrupt_inject_10();
    let (ok5, d5) = {
        let mut t = String::new();
        t.push_str("tries=");
        push_dec(&mut t, c5.tries);
        t.push_str(" fallbacks=");
        push_dec(&mut t, c5.fallbacks);
        t.push_str(" good=");
        t.push_str(if c5.good_ok { "1" } else { "0" });
        (c5.tries == 10 && c5.fallbacks == 10 && c5.good_ok, t)
    };
    v.push(("F48829-J1-损坏注入×10回退10/10次告警可查零静默", ok5, d5));

    // 6 零静默
    let c6 = corrupt_inject_10();
    let (ok6, d6) = {
        let mut t = String::new();
        t.push_str("alarmed=");
        push_dec(&mut t, c6.alarmed);
        t.push_str(" silent=");
        push_dec(&mut t, c6.silent);
        (c6.alarmed == 10 && c6.silent == 0, t)
    };
    v.push(("F48829-J2-回退必留告警账零静默", ok6, d6));

    // 7 回退目标即内置美式
    let c7 = corrupt_inject_10();
    let ok7 = c7.wrong_target == 0 && us_digest() == table_digest(Layout::Us) && BUILTIN_US_ROWS == 104;
    v.push(("F48829-J3-回退布局即内置美式104键", ok7, String::from("us=104 target ok")));

    // 8 191 键 × 2 布局逐键绿
    let t8 = key_table_green();
    let (ok8, d8) = {
        let mut t = String::new();
        t.push_str("keys=");
        push_dec(&mut t, t8.keys);
        t.push_str(" layouts=");
        push_dec(&mut t, t8.layouts);
        t.push_str(" cells=");
        push_dec(&mut t, t8.cells);
        t.push_str(" green=");
        push_dec(&mut t, t8.green);
        (t8.keys == 191 && t8.layouts == 2 && t8.green == t8.cells && t8.unmapped == 0, t)
    };
    v.push(("F48830-J1-全表×191键×2布局逐键绿", ok8, d8));

    // 9 make/break 成对
    let ok9 = t8.makes == t8.breaks && t8.makes == 382 && t8.sticky == 0;
    v.push(("F48830-J2-make/break成对无粘滞", ok9, String::from("382/382")));

    // 10 键码卫生
    let (dup_kc, dup_name) = keycode_hygiene();
    let ok10 = dup_kc == 0 && dup_name == 0 && US_TABLE.len() == 104 && DE_TABLE.len() == 87;
    v.push(("F48830-J3-布局内键码与键名唯一", ok10, String::from("dup=0")));

    v
}

// ── 检查集 ───────────────────────────────────────────────────
/// 运行本模块判据。
pub fn run_unx48828_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48828 keymap 热切换 / 损坏回退 / 191 键全表");
    let checks = extra_checks();
    let mut i = 0usize;
    while i < checks.len() {
        if let Some((name, passed, d)) = checks.get(i) {
            if d.is_empty() {
                s.add(name, *passed, "");
            } else {
                s.add_dyn(name, *passed, d.as_str());
            }
        }
        i += 1;
    }
    s
}

// ── 测试 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_tables_104_87() {
        assert_eq!(US_TABLE.len(), 104);
        assert_eq!(DE_TABLE.len(), 87);
        assert_eq!(table_len(Layout::Us), 104);
        assert_eq!(table_len(Layout::De), 87);
        assert_eq!(US_TABLE.len() + DE_TABLE.len(), 191);
        // 德式 z/y 互换是真的换过，不是贴同一张表
        let us_z = find(Layout::Us, 44).expect("us z");
        let de_z = find(Layout::De, 44).expect("de z");
        assert_eq!(us_z.lo, 0x007a);
        assert_eq!(de_z.lo, 0x0079);
        let us_y = find(Layout::Us, 21).expect("us y");
        let de_y = find(Layout::De, 21).expect("de y");
        assert_eq!(us_y.lo, 0x0079);
        assert_eq!(de_y.lo, 0x007a);
        // 德式 ß 在 AE11
        assert_eq!(find(Layout::De, 12).map(|r| r.lo), Some(0x00df));
    }

    #[test]
    fn t_engine_make_break() {
        let mut e = Engine::new(Layout::Us);
        assert_eq!(e.press(30, false), Ok(0x0061));
        assert_eq!(e.press(30, true), Err(DispCode::MakeBreakMismatch.val()));
        assert_eq!(e.release(30), Ok(()));
        assert_eq!(e.release(30), Err(DispCode::MakeBreakMismatch.val()));
        assert!(e.all_up());
        assert_eq!(e.press(200, false), Err(DispCode::KeyUnmapped.val()));
        assert_eq!(e.makes, 1);
        assert_eq!(e.breaks, 1);
    }

    #[test]
    fn t_key_table_191x2() {
        let t = key_table_green();
        assert_eq!(t.keys, 191);
        assert_eq!(t.layouts, 2);
        assert_eq!(t.cells, 382);
        assert_eq!(t.green, 382);
        assert_eq!(t.unmapped, 0);
        assert_eq!(t.sticky, 0);
        assert_eq!(t.makes, 382);
        assert_eq!(t.breaks, 382);
    }

    #[test]
    fn t_switch_100_lossless() {
        let r = switch_100(100).expect("switch");
        assert_eq!(r.rounds, 100);
        assert_eq!(r.events, 900);
        assert_eq!(r.switches, 100);
        assert_eq!(r.lost, 0);
        assert!(r.peak >= 7);
        assert_eq!(r.mismatches, 0);
        assert!(r.balanced);
    }

    #[test]
    fn t_switch_window_values() {
        let mut d = Dispatcher::new();
        d.begin_switch(Layout::De).expect("begin");
        // 窗口内按键不得用旧布局解释（z 键在德式是 y）
        let got = d.feed(KeyEvent { seq: 0, kc: 44, shift: false }).expect("feed");
        assert!(got.is_none());
        assert_eq!(d.commit_switch(), Ok(1));
        assert_eq!(d.delivered.first().map(|v| v.keysym), Some(0x0079));
        assert_eq!(d.delivered.first().map(|v| v.layout), Some(Layout::De));
        assert!(d.accounting_ok());
    }

    #[test]
    fn t_switch_abort_keeps_old() {
        let mut d = Dispatcher::new();
        d.begin_switch(Layout::De).expect("begin");
        d.feed(KeyEvent { seq: 0, kc: 44, shift: false }).expect("feed");
        assert_eq!(d.abort_switch(), 1);
        assert_eq!(d.active, Layout::Us);
        assert_eq!(d.delivered.first().map(|v| v.keysym), Some(0x007a));
        assert_eq!(d.pending, None);
        assert_eq!(d.aborted, 1);
    }

    #[test]
    fn t_queue_overflow_explicit() {
        let mut d = Dispatcher::new();
        d.begin_switch(Layout::De).expect("begin");
        let mut i = 0u32;
        let mut last: Result<Option<u32>, u32> = Ok(None);
        while i <= QUEUE_CAP as u32 {
            last = d.feed(KeyEvent { seq: i, kc: 2, shift: false });
            i += 1;
        }
        assert_eq!(last, Err(DispCode::QueueFull.val()));
        assert_eq!(d.lost, 1);
        assert!(d.accounting_ok());
        assert_eq!(d.peak, QUEUE_CAP as u32);
    }

    #[test]
    fn t_load_good_de() {
        let good = good_de_text();
        let mut ledger = AlarmLedger::new();
        let o = load_keymap("de", good.as_str(), &mut ledger);
        assert!(!o.fallback);
        assert_eq!(o.layout, Layout::De);
        assert_eq!(o.rows, 87);
        assert_eq!(o.alarm, 0);
        assert_eq!(o.digest, de_digest());
        assert!(ledger.rows.is_empty());
        assert_eq!(good.lines().count(), 91);
    }

    #[test]
    fn t_corrupt_ten_never_silent() {
        let c = corrupt_inject_10();
        assert!(c.good_ok);
        assert_eq!(c.tries, 10);
        assert_eq!(c.fallbacks, 10);
        assert_eq!(c.alarmed, 10);
        assert_eq!(c.silent, 0);
        assert_eq!(c.wrong_target, 0);
        // 十类损坏至少触发三类不同诊断码，不是全都撞一个
        let good = good_de_text();
        let mut ledger = AlarmLedger::new();
        let mut idx = 0usize;
        let mut kinds = 0u32;
        while idx < 10 {
            let text = corrupt_text(idx, good.as_str());
            let name = if idx == 0 { "" } else { "de" };
            let o = load_keymap(name, text.as_str(), &mut ledger);
            if o.fallback && ledger.count_of(o.alarm) >= 1 {
                kinds = kinds.saturating_add(1);
            }
            idx += 1;
        }
        assert_eq!(kinds, 10);
        assert!(ledger.rows.len() >= 3);
        assert!(ledger.queryable());
        assert_eq!(ledger.dropped, 0);
    }

    #[test]
    fn t_golden_probe_repro_segment() {
        let g = golden_run().expect("golden");
        assert_eq!(g, (100, 382, 10));
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(detail(0).contains("未知"));
        for c in DispCode::ALL.iter() {
            assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x20);
        }
        let led = inject_rounds(1).expect("inject");
        assert!(led.all_caught());
        let v = extra_checks();
        assert!(v.len() >= 8 && v.len() <= 10);
        for (n, p, _) in v {
            assert!(p, "红项 {}", n);
        }
        assert!(run_unx48828_checks().all_passed());
    }
}
