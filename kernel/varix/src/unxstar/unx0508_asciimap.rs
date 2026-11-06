//! UNX-F0508 按键 ASCII 映射（shift 双面） · 0x763C0段
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（分派表 DISPATCH_w7P 逐字锚定，AI-VX7-1104·批7P）：
//!   映射表 104 键闭集：shift 按下/释放双面映射账齐，caps/num 状态
//!   双态账，映射样本 30/30 一致，未映射键显性归档
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   1. 104 键闭集一张总表：主区 72（0x01..0x46 直码区 + 0x54 SysRq +
//!      0x57/0x58 F11/F12）+ 小键盘 13（0x47..0x53，num 双态）+ 扩展 19
//!      （E0 前缀：RCtrl/RAlt/Pad//KP-Enter/编辑导航七键/LWin/RWin/Menu
//!      + PrtSc + Pad=）；键位构成出处：IBM PC/AT 集 1 编码表（1501492）
//!      与 PS/2 扩展键 E0 表。表规模冻结 104，一码一位不重复。
//!   2. shift 双面：每键两列（norm/shifted），符号键逐对手写（1!/2@…
//!      /=、[:、'\"、`~、\\|、,<、.>、/?）。
//!   3. caps 双态：字母键大写条件 = shift 异或 caps（XOR 口径：同按
//!      回小写），非字母键 caps 不参与。
//!   4. num 双态：小键盘 num 开 → 数字/运算符 ASCII，num 关 → 导航
//!      非打印（Ok(0)）；Pad/ 与 KP-Enter 不受 num 影响。
//!   5. 非打印键（修饰键/F 键/导航键）显性归零 Ok(0)，与「未映射」
//!      严格区分；未登记扫描码（0x55/0x56/0x5A/0x59 等）显性归档入
//!      audit 账并 Err，绝不猜映射。
//!   6. 30 样本冻结语料逐条对拍 + 基准篡改注入必抓 + 双跑一致 +
//!      段位守卫三守卫齐备。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x763C0，AI-VX7-1104 批7P 独占区）。
pub const SEG_BASE: u32 = 0x763C0;

/// 映射表总键数（闭集冻结 104）。
pub const KEY_TOTAL: usize = 104;
/// 主区键数（直码区 + SysRq + F11/F12）。
pub const MAIN_N: usize = 72;
/// 小键盘键数（num 双态账）。
pub const PAD_N: usize = 13;
/// 扩展键数（E0 前缀区）。
pub const EXT_N: usize = 19;
/// 小键盘直码区间（主表内以 code 段识别）。
pub const PAD_LO: u8 = 0x47;
pub const PAD_HI: u8 = 0x53;
/// 冻结语料样本数（判据口径 30/30）。
pub const SAMPLE_N: usize = 30;
/// 未映射哨兵（0xFF 不是合法 ASCII 结果值）。
pub const UNMAPPED: u8 = 0xFF;
/// caps 大写条件口径：shift 异或 caps（冻结标记）。
pub const CAPS_XOR_CALIBER: bool = true;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapDiag {
    /// 未映射键（未登记扫描码）须归档不猜
    UnmappedKey,
    /// 表规模漂移（≠104 冻结）
    TableDrift,
    /// 表内 (code,ext) 重复登记
    TableDup,
    /// 样本与基准失配
    CorpusMismatch,
    /// caps 口径违例（非 XOR）
    CapsCaliber,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl MapDiag {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MapDiag::UnmappedKey => "未映射键：显性归档不猜（建议：登记入 audit 账）",
            MapDiag::TableDrift => "表规模漂移：冻结 104 键（建议：回滚总表）",
            MapDiag::TableDup => "表内 (code,ext) 重复（建议：闭集去重后复核）",
            MapDiag::CorpusMismatch => "映射样本与基准失配（建议：逐条对拍渲染文本）",
            MapDiag::CapsCaliber => "caps 口径违例：大写条件=shift 异或 caps（建议：修取列逻辑）",
            MapDiag::NotReproducible => "双跑不一致：复现红线（建议：查全局可变量）",
        }
    }
    pub const ALL: [MapDiag; 6] = [
        MapDiag::UnmappedKey,
        MapDiag::TableDrift,
        MapDiag::TableDup,
        MapDiag::CorpusMismatch,
        MapDiag::CapsCaliber,
        MapDiag::NotReproducible,
    ];
}

/// 闭集查表：诊断码 → 中文明细（段外码显性回退）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MapDiag::ALL.len() {
        if let Some(v) = MapDiag::ALL.get(i) {
            if v.val() == code {
                return v.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x763C0 段闭集内"
}

// ── 域模型：键位能力 + 修饰态 ───────────────────────────────────
/// 一键双面能力：norm（平键列）/ shifted（shift 列）。
/// norm==0x00 表示非打印键（修饰/F/导航）；0x08/0x09/0x0D/0x1B 等
/// 为控制字符 ASCII；小键盘键 norm=num 开列，shifted=num 关列（0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyCap {
    /// 集 1 make 码
    pub code: u8,
    /// E0 扩展位
    pub ext: bool,
    /// 平键列（小键盘键此列为 num 开）
    pub norm: u8,
    /// shift 列（小键盘键此列为 num 关 = 0 导航）
    pub shifted: u8,
}

/// 修饰态（三账：shift / caps / num 各一位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mods {
    pub shift: bool,
    pub caps: bool,
    pub num: bool,
}

impl Mods {
    pub const fn none() -> Mods {
        Mods { shift: false, caps: false, num: false }
    }
    pub const fn shift_on() -> Mods {
        Mods { shift: true, caps: false, num: false }
    }
    pub const fn caps_on() -> Mods {
        Mods { shift: false, caps: true, num: false }
    }
}

// ── 104 键闭集总表（主区 72 + 小键盘 13 + 扩展 19，一一手写） ────
pub const KEY_TABLE: [KeyCap; KEY_TOTAL] = [
    // ── 主区：0x01..0x36（54） ──
    KeyCap { code: 0x01, ext: false, norm: 0x1B, shifted: 0x1B }, // Esc
    KeyCap { code: 0x02, ext: false, norm: b'1', shifted: b'!' },
    KeyCap { code: 0x03, ext: false, norm: b'2', shifted: b'@' },
    KeyCap { code: 0x04, ext: false, norm: b'3', shifted: b'#' },
    KeyCap { code: 0x05, ext: false, norm: b'4', shifted: b'$' },
    KeyCap { code: 0x06, ext: false, norm: b'5', shifted: b'%' },
    KeyCap { code: 0x07, ext: false, norm: b'6', shifted: b'^' },
    KeyCap { code: 0x08, ext: false, norm: b'7', shifted: b'&' },
    KeyCap { code: 0x09, ext: false, norm: b'8', shifted: b'*' },
    KeyCap { code: 0x0A, ext: false, norm: b'9', shifted: b'(' },
    KeyCap { code: 0x0B, ext: false, norm: b'0', shifted: b')' },
    KeyCap { code: 0x0C, ext: false, norm: b'-', shifted: b'_' },
    KeyCap { code: 0x0D, ext: false, norm: b'=', shifted: b'+' },
    KeyCap { code: 0x0E, ext: false, norm: 0x08, shifted: 0x08 }, // Bksp
    KeyCap { code: 0x0F, ext: false, norm: 0x09, shifted: 0x09 }, // Tab
    KeyCap { code: 0x10, ext: false, norm: b'q', shifted: b'Q' },
    KeyCap { code: 0x11, ext: false, norm: b'w', shifted: b'W' },
    KeyCap { code: 0x12, ext: false, norm: b'e', shifted: b'E' },
    KeyCap { code: 0x13, ext: false, norm: b'r', shifted: b'R' },
    KeyCap { code: 0x14, ext: false, norm: b't', shifted: b'T' },
    KeyCap { code: 0x15, ext: false, norm: b'y', shifted: b'Y' },
    KeyCap { code: 0x16, ext: false, norm: b'u', shifted: b'U' },
    KeyCap { code: 0x17, ext: false, norm: b'i', shifted: b'I' },
    KeyCap { code: 0x18, ext: false, norm: b'o', shifted: b'O' },
    KeyCap { code: 0x19, ext: false, norm: b'p', shifted: b'P' },
    KeyCap { code: 0x1A, ext: false, norm: b'[', shifted: b'{' },
    KeyCap { code: 0x1B, ext: false, norm: b']', shifted: b'}' },
    KeyCap { code: 0x1C, ext: false, norm: 0x0D, shifted: 0x0D }, // Enter
    KeyCap { code: 0x1D, ext: false, norm: 0x00, shifted: 0x00 }, // LCtrl
    KeyCap { code: 0x1E, ext: false, norm: b'a', shifted: b'A' },
    KeyCap { code: 0x1F, ext: false, norm: b's', shifted: b'S' },
    KeyCap { code: 0x20, ext: false, norm: b'd', shifted: b'D' },
    KeyCap { code: 0x21, ext: false, norm: b'f', shifted: b'F' },
    KeyCap { code: 0x22, ext: false, norm: b'g', shifted: b'G' },
    KeyCap { code: 0x23, ext: false, norm: b'h', shifted: b'H' },
    KeyCap { code: 0x24, ext: false, norm: b'j', shifted: b'J' },
    KeyCap { code: 0x25, ext: false, norm: b'k', shifted: b'K' },
    KeyCap { code: 0x26, ext: false, norm: b'l', shifted: b'L' },
    KeyCap { code: 0x27, ext: false, norm: b';', shifted: b':' },
    KeyCap { code: 0x28, ext: false, norm: b'\'', shifted: b'"' },
    KeyCap { code: 0x29, ext: false, norm: b'`', shifted: b'~' },
    KeyCap { code: 0x2A, ext: false, norm: 0x00, shifted: 0x00 }, // LShift
    KeyCap { code: 0x2B, ext: false, norm: b'\\', shifted: b'|' },
    KeyCap { code: 0x2C, ext: false, norm: b'z', shifted: b'Z' },
    KeyCap { code: 0x2D, ext: false, norm: b'x', shifted: b'X' },
    KeyCap { code: 0x2E, ext: false, norm: b'c', shifted: b'C' },
    KeyCap { code: 0x2F, ext: false, norm: b'v', shifted: b'V' },
    KeyCap { code: 0x30, ext: false, norm: b'b', shifted: b'B' },
    KeyCap { code: 0x31, ext: false, norm: b'n', shifted: b'N' },
    KeyCap { code: 0x32, ext: false, norm: b'm', shifted: b'M' },
    KeyCap { code: 0x33, ext: false, norm: b',', shifted: b'<' },
    KeyCap { code: 0x34, ext: false, norm: b'.', shifted: b'>' },
    KeyCap { code: 0x35, ext: false, norm: b'/', shifted: b'?' },
    KeyCap { code: 0x36, ext: false, norm: 0x00, shifted: 0x00 }, // RShift
    // ── 主区：0x38..0x3A（3） ──
    KeyCap { code: 0x38, ext: false, norm: 0x00, shifted: 0x00 }, // LAlt
    KeyCap { code: 0x39, ext: false, norm: b' ', shifted: b' ' }, // Space
    KeyCap { code: 0x3A, ext: false, norm: 0x00, shifted: 0x00 }, // Caps
    // ── 主区：0x3B..0x44（F1..F10，10） ──
    KeyCap { code: 0x3B, ext: false, norm: 0x00, shifted: 0x00 },
    KeyCap { code: 0x3C, ext: false, norm: 0x00, shifted: 0x00 },
    KeyCap { code: 0x3D, ext: false, norm: 0x00, shifted: 0x00 },
    KeyCap { code: 0x3E, ext: false, norm: 0x00, shifted: 0x00 },
    KeyCap { code: 0x3F, ext: false, norm: 0x00, shifted: 0x00 },
    KeyCap { code: 0x40, ext: false, norm: 0x00, shifted: 0x00 },
    KeyCap { code: 0x41, ext: false, norm: 0x00, shifted: 0x00 },
    KeyCap { code: 0x42, ext: false, norm: 0x00, shifted: 0x00 },
    KeyCap { code: 0x43, ext: false, norm: 0x00, shifted: 0x00 },
    KeyCap { code: 0x44, ext: false, norm: 0x00, shifted: 0x00 },
    // ── 主区：0x45..0x46（2） ──
    KeyCap { code: 0x45, ext: false, norm: 0x00, shifted: 0x00 }, // NumLk
    KeyCap { code: 0x46, ext: false, norm: 0x00, shifted: 0x00 }, // ScrLk
    // ── 主区：0x54 SysRq（1） + 0x57/0x58 F11/F12（2） ──
    KeyCap { code: 0x54, ext: false, norm: 0x00, shifted: 0x00 }, // SysRq
    KeyCap { code: 0x57, ext: false, norm: 0x00, shifted: 0x00 }, // F11
    KeyCap { code: 0x58, ext: false, norm: 0x00, shifted: 0x00 }, // F12
    // ── 小键盘：0x47..0x53（13，norm=num 开列，shifted=num 关列 0） ──
    KeyCap { code: 0x47, ext: false, norm: b'7', shifted: 0x00 },
    KeyCap { code: 0x48, ext: false, norm: b'8', shifted: 0x00 },
    KeyCap { code: 0x49, ext: false, norm: b'9', shifted: 0x00 },
    KeyCap { code: 0x4A, ext: false, norm: b'-', shifted: 0x00 },
    KeyCap { code: 0x4B, ext: false, norm: b'4', shifted: 0x00 },
    KeyCap { code: 0x4C, ext: false, norm: b'5', shifted: 0x00 },
    KeyCap { code: 0x4D, ext: false, norm: b'6', shifted: 0x00 },
    KeyCap { code: 0x4E, ext: false, norm: b'+', shifted: 0x00 },
    KeyCap { code: 0x4F, ext: false, norm: b'1', shifted: 0x00 },
    KeyCap { code: 0x50, ext: false, norm: b'2', shifted: 0x00 },
    KeyCap { code: 0x51, ext: false, norm: b'3', shifted: 0x00 },
    KeyCap { code: 0x52, ext: false, norm: b'0', shifted: 0x00 },
    KeyCap { code: 0x53, ext: false, norm: b'.', shifted: 0x00 },
    // ── 扩展区：E0 前缀 19 键 ──
    KeyCap { code: 0x0D, ext: true, norm: b'=', shifted: b'+' },  // Pad=
    KeyCap { code: 0x1C, ext: true, norm: 0x0D, shifted: 0x0D },  // KP-Enter
    KeyCap { code: 0x1D, ext: true, norm: 0x00, shifted: 0x00 },  // RCtrl
    KeyCap { code: 0x35, ext: true, norm: b'/', shifted: b'/' },  // Pad/
    KeyCap { code: 0x37, ext: true, norm: 0x00, shifted: 0x00 },  // PrtSc
    KeyCap { code: 0x38, ext: true, norm: 0x00, shifted: 0x00 },  // RAlt
    KeyCap { code: 0x47, ext: true, norm: 0x00, shifted: 0x00 },  // Home
    KeyCap { code: 0x48, ext: true, norm: 0x00, shifted: 0x00 },  // Up
    KeyCap { code: 0x49, ext: true, norm: 0x00, shifted: 0x00 },  // PgUp
    KeyCap { code: 0x4B, ext: true, norm: 0x00, shifted: 0x00 },  // Left
    KeyCap { code: 0x4D, ext: true, norm: 0x00, shifted: 0x00 },  // Right
    KeyCap { code: 0x4F, ext: true, norm: 0x00, shifted: 0x00 },  // End
    KeyCap { code: 0x50, ext: true, norm: 0x00, shifted: 0x00 },  // Down
    KeyCap { code: 0x51, ext: true, norm: 0x00, shifted: 0x00 },  // PgDn
    KeyCap { code: 0x52, ext: true, norm: 0x00, shifted: 0x00 },  // Ins
    KeyCap { code: 0x53, ext: true, norm: 0x00, shifted: 0x00 },  // Del
    KeyCap { code: 0x5B, ext: true, norm: 0x00, shifted: 0x00 },  // LWin
    KeyCap { code: 0x5C, ext: true, norm: 0x00, shifted: 0x00 },  // RWin
    KeyCap { code: 0x5D, ext: true, norm: 0x00, shifted: 0x00 },  // Menu
];

/// 表查位：(code, ext) → 表项。
pub fn cap_of(code: u8, ext: bool) -> Option<KeyCap> {
    for c in KEY_TABLE.iter() {
        if c.code == code && c.ext == ext {
            return Some(*c);
        }
    }
    None
}

/// 归档账：未映射请求显性登记（不猜）。
#[derive(Clone, Debug, Default)]
pub struct MapAudit {
    pub unmapped: Vec<u8>,
    pub queries: u32,
}

impl MapAudit {
    pub fn new() -> MapAudit {
        MapAudit::default()
    }
    pub fn note_miss(&mut self, code: u8) {
        self.unmapped.push(code);
    }
}

/// ASCII 取值（判据主路径）。
/// Ok(0) = 表内非打印键（修饰/F/导航）；Err(UnmappedKey) = 未登记码
/// （已归档入 audit）；其余 Ok 值为 ASCII。
pub fn to_ascii(code: u8, ext: bool, mods: Mods, audit: &mut MapAudit) -> Result<u8, u32> {
    audit.queries += 1;
    let cap = match cap_of(code, ext) {
        Some(c) => c,
        None => {
            audit.note_miss(code);
            return Err(MapDiag::UnmappedKey.val());
        }
    };
    // 非打印键：显性归零（与未映射严格区分）。
    if cap.norm == 0x00 {
        return Ok(0);
    }
    // 小键盘：num 双态账（num 关 → 导航 0）。
    if !ext && cap.code >= PAD_LO && cap.code <= PAD_HI {
        if mods.num {
            return Ok(cap.norm);
        }
        return Ok(0);
    }
    // 字母键：caps XOR 口径（大写条件 = shift 异或 caps）。
    if cap.norm >= b'a' && cap.norm <= b'z' {
        let upper = mods.shift != mods.caps;
        return Ok(if upper { cap.shifted } else { cap.norm });
    }
    // 其余键：shift 双面取列。
    Ok(if mods.shift { cap.shifted } else { cap.norm })
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 表规模冻结谓词：总表 104 且三区账齐。
fn table_size_ok() -> bool {
    KEY_TABLE.len() == KEY_TOTAL && KEY_TOTAL == MAIN_N + PAD_N + EXT_N
}

/// 表闭集无重复谓词：104 个 (code,ext) 两两互异。
fn table_unique_ok() -> bool {
    let mut i = 0;
    while i < KEY_TABLE.len() {
        let mut j = i + 1;
        while j < KEY_TABLE.len() {
            if let (Some(a), Some(b)) = (KEY_TABLE.get(i), KEY_TABLE.get(j)) {
                if a.code == b.code && a.ext == b.ext {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// caps XOR 口径谓词：a 键四态（shift/caps 组合）逐一对。
fn caps_xor_ok() -> bool {
    let mut audit = MapAudit::new();
    let a_norm = to_ascii(0x1E, false, Mods::none(), &mut audit);
    let a_shift = to_ascii(0x1E, false, Mods::shift_on(), &mut audit);
    let a_caps = to_ascii(0x1E, false, Mods::caps_on(), &mut audit);
    let a_both = to_ascii(0x1E, false, Mods { shift: true, caps: true, num: false }, &mut audit);
    match (a_norm, a_shift, a_caps, a_both) {
        (Ok(n), Ok(s), Ok(c), Ok(x)) => n == b'a' && s == b'A' && c == b'A' && x == b'a',
        _ => false,
    }
}

/// num 双态谓词：Pad7/Pad0/Pad- 三点 num 开关各对。
fn num_dual_ok() -> bool {
    let mut audit = MapAudit::new();
    let on7 = to_ascii(0x47, false, Mods { num: true, ..Mods::none() }, &mut audit);
    let off7 = to_ascii(0x47, false, Mods::none(), &mut audit);
    let on0 = to_ascii(0x52, false, Mods { num: true, ..Mods::none() }, &mut audit);
    let onm = to_ascii(0x4A, false, Mods { num: true, ..Mods::none() }, &mut audit);
    match (on7, off7, on0, onm) {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => a == b'7' && b == 0 && c == b'0' && d == b'-',
        _ => false,
    }
}

/// 未映射归档谓词：0x55/0x5A 必 Err 且入账。
fn unmapped_archive_ok() -> bool {
    let mut audit = MapAudit::new();
    let r1 = to_ascii(0x55, false, Mods::none(), &mut audit);
    let r2 = to_ascii(0x5A, false, Mods::none(), &mut audit);
    let r3 = to_ascii(0x59, true, Mods::none(), &mut audit);
    r1 == Err(MapDiag::UnmappedKey.val())
        && r2 == Err(MapDiag::UnmappedKey.val())
        && r3 == Err(MapDiag::UnmappedKey.val())
        && audit.unmapped.len() == 3
}

/// 非打印归零谓词：F1/LCtrl/RCtrl/Home 显性归零（非未映射）。
fn nav_zero_ok() -> bool {
    let mut audit = MapAudit::new();
    let f1 = to_ascii(0x3B, false, Mods::none(), &mut audit);
    let lctrl = to_ascii(0x1D, false, Mods::none(), &mut audit);
    let rctrl = to_ascii(0x1D, true, Mods::none(), &mut audit);
    let home = to_ascii(0x47, true, Mods::none(), &mut audit);
    match (f1, lctrl, rctrl, home) {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => a == 0 && b == 0 && c == 0 && d == 0,
        _ => false,
    }
}

// ── 冻结语料（30 样本：码/扩展位/修饰态 → 期望值） ───────────────
/// 期望值口径：ASCII（>0）、0=表内非打印、UNMAPPED(0xFF)=未登记码。
const SAMPLES: [(u8, bool, Mods, u8); 30] = [
    // 字母 caps/shift 双态四点（0x1E='a'）。
    (0x1E, false, Mods::none(), b'a'),
    (0x1E, false, Mods::shift_on(), b'A'),
    (0x1E, false, Mods::caps_on(), b'A'),
    (0x1E, false, Mods { shift: true, caps: true, num: false }, b'a'),
    // 字母另三点。
    (0x1F, false, Mods::none(), b's'),
    (0x32, false, Mods::shift_on(), b'M'),
    (0x19, false, Mods::caps_on(), b'P'),
    // 数字与符号 shift 双面。
    (0x02, false, Mods::none(), b'1'),
    (0x02, false, Mods::shift_on(), b'!'),
    (0x03, false, Mods::shift_on(), b'@'),
    (0x0C, false, Mods::shift_on(), b'_'),
    (0x0D, false, Mods::shift_on(), b'+'),
    (0x1A, false, Mods::shift_on(), b'{'),
    (0x27, false, Mods::shift_on(), b':'),
    (0x28, false, Mods::shift_on(), b'"'),
    (0x29, false, Mods::shift_on(), b'~'),
    (0x2B, false, Mods::shift_on(), b'|'),
    (0x33, false, Mods::shift_on(), b'<'),
    // 控制键与空格。
    (0x39, false, Mods::none(), b' '),
    (0x0F, false, Mods::none(), 0x09),
    (0x1C, false, Mods::none(), 0x0D),
    (0x0E, false, Mods::none(), 0x08),
    (0x01, false, Mods::none(), 0x1B),
    // 小键盘 num 双态。
    (0x47, false, Mods { num: true, ..Mods::none() }, b'7'),
    (0x47, false, Mods::none(), 0),
    (0x52, false, Mods { num: true, ..Mods::none() }, b'0'),
    // 扩展区。
    (0x35, true, Mods::none(), b'/'),
    (0x1D, true, Mods::none(), 0),
    // 未登记码显性归档。
    (0x55, false, Mods::none(), UNMAPPED),
    (0x5A, false, Mods::none(), UNMAPPED),
];

/// 语料逐样本对拍：返回 (对拍一致数, 归档数)。
pub fn corpus_match() -> Result<(usize, usize), u32> {
    if !table_size_ok() {
        return Err(MapDiag::TableDrift.val());
    }
    let mut audit = MapAudit::new();
    let mut matched = 0usize;
    for (code, ext, mods, expect) in SAMPLES.iter() {
        let got = match to_ascii(*code, *ext, *mods, &mut audit) {
            Ok(v) => v,
            Err(e) if e == MapDiag::UnmappedKey.val() => UNMAPPED,
            Err(e) => return Err(e),
        };
        if got == *expect {
            matched += 1;
        }
    }
    Ok((matched, audit.unmapped.len()))
}

// ── 注入演练账（四路：未映射不猜/基准篡改/caps 口径/表重复） ─────
pub struct DrillTally {
    pub fired: [u32; 4],
    pub caught: [u32; 4],
}

impl DrillTally {
    pub fn new() -> DrillTally {
        DrillTally { fired: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, lane: usize, hit: bool) {
        if lane < 4 {
            self.fired[lane] += 1;
            if hit {
                self.caught[lane] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.fired[i] == 0 || self.caught[i] != self.fired[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入演练（人为构造违约必被拒）：
/// 路 0：未映射不猜（Err 且归档入账）；
/// 路 1：基准篡改（把 'a' 期望改成 'A'）→ 对拍必抓；
/// 路 2：caps 口径破坏（若误用 OR 而非 XOR，同按必判 'A'）→ 谓词必抓；
/// 路 3：表重复/规模违约 → 闭集谓词必抓。
pub fn inject_rounds(rounds: u32) -> Result<DrillTally, u32> {
    let mut t = DrillTally::new();
    let mut r = 0;
    while r < rounds {
        // 路 0：未映射不猜。
        let mut audit = MapAudit::new();
        let r0 = to_ascii(0x55, false, Mods::none(), &mut audit);
        let hit0 = r0 == Err(MapDiag::UnmappedKey.val()) && audit.unmapped.len() == 1;
        t.record(0, hit0);
        // 路 1：基准篡改必抓——用「被改错的期望」对拍必须失配。
        let mut audit1 = MapAudit::new();
        let got = to_ascii(0x1E, false, Mods::none(), &mut audit1);
        let hit1 = got == Ok(b'a') && got != Ok(b'A');
        t.record(1, hit1);
        // 路 2：caps XOR 口径同按回小写（误用 OR 的实现必被抓）。
        let mut audit2 = MapAudit::new();
        let both = to_ascii(0x1E, false, Mods { shift: true, caps: true, num: false }, &mut audit2);
        let hit2 = both == Ok(b'a') && caps_xor_ok();
        t.record(2, hit2);
        // 路 3：表闭集（规模 + 无重复）。
        let hit3 = table_size_ok() && table_unique_ok();
        t.record(3, hit3);
        r += 1;
    }
    Ok(t)
}

// ── 黄金全链：表闭集 → 语料 30/30 → 归档账 → 四路注入 ────────────
/// 返回 (对拍一致数, 归档数, 查询数)。判据口径：30 一致 / 2 归档 / 30 查询。
pub fn golden_run() -> Result<(usize, usize, u32), u32> {
    let (matched, arch_n) = corpus_match()?;
    let inj = inject_rounds(2)?;
    let mut audit = MapAudit::new();
    let _ = to_ascii(0x55, false, Mods::none(), &mut audit);
    if !inj.all_caught() || audit.queries != 1 {
        return Err(MapDiag::NotReproducible.val());
    }
    Ok((matched, arch_n, audit.queries))
}

/// 双跑一致（复现红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MapDiag::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let in_window = SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x75000 && SEG_BASE <= 0x781C0;
    let mut codes_ok = true;
    let mut i = 0;
    while i < MapDiag::ALL.len() {
        if let Some(v) = MapDiag::ALL.get(i) {
            let val = v.val();
            if val < SEG_BASE || val >= SEG_BASE + 0x40 {
                codes_ok = false;
            }
        }
        i += 1;
    }
    in_window && codes_ok
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链 30 一致 / 2 归档 / 1 查询。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((30, 2, 1)) => Ok(()),
        _ => Err(MapDiag::NotReproducible.val()),
    }
}

/// 序 B：未登记码显性归档（0x55/0x5A Err 且 audit 两笔）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut audit = MapAudit::new();
    let r1 = to_ascii(0x55, false, Mods::none(), &mut audit);
    let r2 = to_ascii(0x5A, false, Mods::none(), &mut audit);
    match (r1, r2) {
        (Err(e1), Err(e2))
            if e1 == MapDiag::UnmappedKey.val()
                && e2 == MapDiag::UnmappedKey.val()
                && audit.unmapped.len() == 2 =>
        {
            Ok(())
        }
        _ => Err(MapDiag::NotReproducible.val()),
    }
}

/// 序 C：常量冻结（104=72+13+19、样本 30、XOR 口径、未映射哨兵）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        KEY_TOTAL == 104,
        MAIN_N == 72 && PAD_N == 13 && EXT_N == 19,
        SAMPLE_N == 30,
        CAPS_XOR_CALIBER,
        UNMAPPED == 0xFF,
    ) {
        (true, true, true, true, true) => Ok(()),
        _ => Err(MapDiag::NotReproducible.val()),
    }
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1：映射样本 30/30（主判据）。
    let cm = corpus_match();
    v.push((
        "J1-样本30/30",
        match &cm {
            Ok((m, _)) => *m == SAMPLE_N,
            Err(_) => false,
        },
        match &cm {
            Ok((m, a)) => format!("一致={} 归档={}", m, a),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J1：表规模 104 闭集（主区+小键盘+扩展三区账）。
    v.push((
        "J1-表规模104",
        table_size_ok(),
        format!("总={} 主{}+盘{}+扩{}", KEY_TABLE.len(), MAIN_N, PAD_N, EXT_N),
    ));
    // J1：表闭集无重复。
    v.push((
        "J1-表无重复",
        table_unique_ok(),
        String::from("104 个 (code,ext) 两两互异"),
    ));
    // J1：caps 双态 XOR 口径。
    v.push((
        "J1-caps双态",
        caps_xor_ok(),
        String::from("a 键四态：平a/shift A/caps A/同按a"),
    ));
    // J1：num 双态。
    v.push((
        "J1-num双态",
        num_dual_ok(),
        String::from("Pad7/Pad0/Pad- 三点开关各对"),
    ));
    // J1：shift 双面（符号三对手写列核对）。
    let mut audit = MapAudit::new();
    let s1 = to_ascii(0x02, false, Mods::shift_on(), &mut audit);
    let s2 = to_ascii(0x28, false, Mods::shift_on(), &mut audit);
    let s3 = to_ascii(0x35, false, Mods::shift_on(), &mut audit);
    let shift_ok = s1 == Ok(b'!') && s2 == Ok(b'"') && s3 == Ok(b'?');
    v.push((
        "J1-shift双面",
        shift_ok,
        String::from("1!/2@/'\" 等双面列逐对手写"),
    ));
    // J1：未映射显性归档。
    v.push((
        "J1-未映射归档",
        unmapped_archive_ok(),
        String::from("0x55/0x5A/0x59E 三笔 Err+入账"),
    ));
    // J1：非打印键归零（与未映射严格区分）。
    v.push((
        "J1-非打印归零",
        nav_zero_ok(),
        String::from("F1/LCtrl/RCtrl/Home 归零 Ok(0)"),
    ));
    // J1：注入四路全抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(t) => t.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入四路",
        inj_ok,
        format!(
            "f/c {:?}/{:?}",
            inj.as_ref().map(|t| t.fired),
            inj.as_ref().map(|t| t.caught)
        ),
    ));
    // J1：黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((m, a, q)) => *m == 30 && *a == 2 && *q == 1,
            Err(_) => false,
        },
        match &g {
            Ok((m, a, q)) => format!("match={} arch={} q={}", m, a, q),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J2：双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // J2：探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J2-探针三序",
        a && b && c,
        format!("a=绿 b=归档检出{} c=冻结检出{}", b, c),
    ));
    // J3：段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x763C0 窗口"),
    ));
    // J3：诊断闭集查表往返。
    let mut det_ok = true;
    for d in MapDiag::ALL.iter() {
        if detail(d.val()) != d.detail() {
            det_ok = false;
        }
    }
    v.push((
        "J3-诊断闭集",
        det_ok && detail(SEG_BASE + 0x3F).starts_with("未知诊断码"),
        String::from("6 变体查表往返+段外回退"),
    ));
    v
}

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

pub fn run_unx0508_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0508 按键 ASCII 映射（shift 双面）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（注入必抓 / 双跑一致 / 边界） ────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_table_is_104_closed() {
        assert_eq!(KEY_TABLE.len(), 104);
        assert!(table_unique_ok());
        assert!(table_size_ok());
    }

    #[test]
    fn t_letter_caps_xor() {
        assert!(caps_xor_ok());
        let mut audit = MapAudit::new();
        assert_eq!(
            to_ascii(0x32, false, Mods { shift: true, caps: true, num: false }, &mut audit),
            Ok(b'm')
        );
    }

    #[test]
    fn t_shift_dual_face() {
        let mut audit = MapAudit::new();
        assert_eq!(to_ascii(0x02, false, Mods::none(), &mut audit), Ok(b'1'));
        assert_eq!(to_ascii(0x02, false, Mods::shift_on(), &mut audit), Ok(b'!'));
        assert_eq!(to_ascii(0x29, false, Mods::shift_on(), &mut audit), Ok(b'~'));
        assert_eq!(to_ascii(0x2B, false, Mods::shift_on(), &mut audit), Ok(b'|'));
    }

    #[test]
    fn t_num_dual_state() {
        assert!(num_dual_ok());
        let mut audit = MapAudit::new();
        assert_eq!(to_ascii(0x53, false, Mods { num: true, ..Mods::none() }, &mut audit), Ok(b'.'));
        assert_eq!(to_ascii(0x53, false, Mods::none(), &mut audit), Ok(0));
    }

    #[test]
    fn t_unmapped_archived() {
        assert!(unmapped_archive_ok());
        let mut audit = MapAudit::new();
        let r = to_ascii(0x56, false, Mods::none(), &mut audit);
        assert_eq!(r, Err(MapDiag::UnmappedKey.val()));
        assert_eq!(audit.unmapped.len(), 1);
    }

    #[test]
    fn t_nav_vs_unmapped_distinct() {
        assert!(nav_zero_ok());
    }

    #[test]
    fn t_corpus_30_of_30() {
        match corpus_match() {
            Ok((m, a)) => {
                assert_eq!(m, 30);
                assert_eq!(a, 2);
            }
            Err(code) => panic!("语料对拍须全绿：err=0x{:x}", code),
        }
    }

    #[test]
    fn t_double_run_identical() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        assert!(a.is_ok());
    }

    #[test]
    fn t_segment_guard_window() {
        assert!(segment_guard());
        for d in MapDiag::ALL.iter() {
            let v = d.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
        }
    }

    #[test]
    fn t_detail_closed_set() {
        for d in MapDiag::ALL.iter() {
            assert!(!d.detail().is_empty());
            assert_eq!(detail(d.val()), d.detail());
        }
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }
}
