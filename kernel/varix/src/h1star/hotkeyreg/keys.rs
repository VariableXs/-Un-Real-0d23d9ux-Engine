//! F244 键域与显示面：修饰键位、VK 码锚点、Combo 键位、键名互转与
//! 键位显示串（设置中心显示面唯一事实源）。
//!
//! 本文件是 [`super::registry`] 的键域承载：键位编码一处一事实——
//! 修饰键 4 位（Ctrl/Alt/Shift/Win）+ Windows VK 码域 u16
//! （锚定 VK 原值，`0x41='A'`、`0x70=F1` 等）。

// ---------------------------------------------------------------------------
// 修饰键位（判据键域的修饰半边）
// ---------------------------------------------------------------------------

/// Ctrl 修饰位。
pub const MOD_CTRL: u8 = 0x01;
/// Alt 修饰位。
pub const MOD_ALT: u8 = 0x02;
/// Shift 修饰位。
pub const MOD_SHIFT: u8 = 0x04;
/// Win 修饰位。
pub const MOD_WIN: u8 = 0x08;
/// 修饰位掩码（注册表只认 4 位修饰键）。
pub const MOD_MASK: u8 = 0x0F;

// ---------------------------------------------------------------------------
// VK 码锚点（Windows VK 原值——键域的事实源）
// ---------------------------------------------------------------------------

pub const VK_BACK: u16 = 0x08;
pub const VK_TAB: u16 = 0x09;
pub const VK_RETURN: u16 = 0x0D;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_SPACE: u16 = 0x20;
pub const VK_PRIOR: u16 = 0x21;
pub const VK_NEXT: u16 = 0x22;
pub const VK_END: u16 = 0x23;
pub const VK_HOME: u16 = 0x24;
pub const VK_LEFT: u16 = 0x25;
pub const VK_UP: u16 = 0x26;
pub const VK_RIGHT: u16 = 0x27;
pub const VK_DOWN: u16 = 0x28;
pub const VK_SNAPSHOT: u16 = 0x2C;
pub const VK_INSERT: u16 = 0x2D;
pub const VK_DELETE: u16 = 0x2E;
pub const VK_LWIN: u16 = 0x5B;
/// 字母键基码（'A'）——VK_A..VK_Z = 0x41..0x5A。
pub const VK_A: u16 = 0x41;
/// 数字键基码（'0'）——VK_0..VK_9 = 0x30..0x39。
pub const VK_0: u16 = 0x30;
/// F1 基码——VK_F1..VK_F12 = 0x70..0x7B。
pub const VK_F1: u16 = 0x70;

/// ASCII 小写化（core 无 format! 域的键名归一）。
fn ascii_lower(b: u8) -> u8 {
    if b.wrapping_sub(b'A') < 26 {
        b + 32
    } else {
        b
    }
}

// ---------------------------------------------------------------------------
// 键位（Combo）
// ---------------------------------------------------------------------------

/// 一条键位：修饰键 4 位 + VK 码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub mods: u8,
    pub key: u16,
}

impl Combo {
    /// 构造（修饰键越界位掩掉——键域收口）。
    pub const fn new(mods: u8, key: u16) -> Combo {
        Combo { mods: mods & MOD_MASK, key }
    }

    /// 空键位（留痕槽位用）。
    pub const fn none() -> Combo {
        Combo { mods: 0, key: 0 }
    }

    /// 32 位冲突判定码：修饰键高 16 位 + VK 低 16 位。
    pub const fn code(&self) -> u32 {
        ((self.mods as u32) << 16) | self.key as u32
    }

    pub const fn is_none(&self) -> bool {
        self.mods == 0 && self.key == 0
    }

    /// 键位合法性：必须有键（纯修饰键不可注册）。
    pub const fn sane(&self) -> bool {
        !self.is_none() && self.key != 0
    }

    /// 修饰键包含判定（mod ∈ CTRL/ALT/SHIFT/WIN 位或组合）。
    pub const fn has_mod(&self, m: u8) -> bool {
        self.mods & m == m
    }

    /// 是否纯修饰键（key == 0——sane 的反面，诊断文案用）。
    pub const fn is_bare_mod(&self) -> bool {
        self.key == 0
    }

    /// 修饰键名集合（显示面辅助）：Ctrl/Alt/Shift/Win 依位拼出。
    pub fn mod_names(&self, out: &mut [&'static str]) -> usize {
        let mut k = 0;
        for (m, name) in MOD_TABLE.iter() {
            if self.mods & m != 0 && k < out.len() {
                out[k] = name;
                k += 1;
            }
        }
        k
    }
}

/// 修饰键规范序表（Ctrl→Alt→Shift→Win——显示序单一事实源；消重归并：
/// 原 mod_names / combo_str 各持一份同构表）。
const MOD_TABLE: [(u8, &str); 4] =
    [(MOD_CTRL, "Ctrl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift"), (MOD_WIN, "Win")];

/// 数字键名表（VK 0x30..=0x39 → "0".."9"——查表化：键名单一来源、
/// 分支收敛，替代逐键 match 的 10 分支嵌套）。
fn digit_name(key: u16) -> &'static str {
    const NAMES: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    NAMES[(key - 0x30) as usize]
}

/// 字母键名表（VK 0x41..=0x5A → "A".."Z"——查表化收分支）。
fn letter_name(key: u16) -> &'static str {
    const NAMES: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
        "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    NAMES[(key - 0x41) as usize]
}

/// F 键名表（VK_F1..VK_F12 → "F1".."F12"——查表化收分支）。
fn fkey_name(key: u16) -> &'static str {
    const NAMES: [&str; 12] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
    ];
    NAMES[(key - VK_F1) as usize]
}

/// 散码键名表（设置中心显示面）：VK 码 → 键名，码段（数字/字母/F 键）
/// 之外的全部映射。查表驱动——原 match 34 臂展平为数据表，码段映射一眼可核。
const KEY_NAMES: [(u16, &str); 28] = [
    (VK_BACK, "Back"),
    (VK_TAB, "Tab"),
    (VK_RETURN, "Enter"),
    (VK_ESCAPE, "Esc"),
    (VK_SPACE, "Space"),
    (VK_PRIOR, "PgUp"),
    (VK_NEXT, "PgDn"),
    (VK_END, "End"),
    (VK_HOME, "Home"),
    (VK_LEFT, "Left"),
    (VK_UP, "Up"),
    (VK_RIGHT, "Right"),
    (VK_DOWN, "Down"),
    (VK_SNAPSHOT, "PrtSc"),
    (VK_INSERT, "Ins"),
    (VK_DELETE, "Del"),
    (VK_LWIN, "Win"),
    (0xBA, ";"),
    (0xBB, "="),
    (0xBC, ","),
    (0xBD, "-"),
    (0xBE, "."),
    (0xBF, "/"),
    (0xC0, "`"),
    (0xDB, "["),
    (0xDC, "\\"),
    (0xDD, "]"),
    (0xDE, "'"),
];

/// VK 码 → 键名（设置中心显示面；覆盖本域用到的全部码段）。
/// 结构：数字/字母/F 键三段查表 + 散码表 [`KEY_NAMES`] 线性查找。
pub fn key_name(key: u16) -> Option<&'static str> {
    // 三段连续码段先判（0x30..0x39 数字、0x41..0x5A 字母、0x70..0x7B F 键）。
    if (0x30..=0x39).contains(&key) {
        return Some(digit_name(key));
    }
    if (0x41..=0x5A).contains(&key) {
        return Some(letter_name(key));
    }
    if (0x70..=0x7B).contains(&key) {
        return Some(fkey_name(key));
    }
    let mut i = 0usize;
    while i < KEY_NAMES.len() {
        if KEY_NAMES[i].0 == key {
            return Some(KEY_NAMES[i].1);
        }
        i += 1;
    }
    None
}

/// 键名 → VK 码表（清单声明解析的键名面；全小写，与输入 lower 后比对）。
const NAME_KEYS: [(&[u8], u16); 29] = [
    (b"back", VK_BACK),
    (b"tab", VK_TAB),
    (b"enter", VK_RETURN),
    (b"esc", VK_ESCAPE),
    (b"space", VK_SPACE),
    (b"pgup", VK_PRIOR),
    (b"pgdn", VK_NEXT),
    (b"end", VK_END),
    (b"home", VK_HOME),
    (b"left", VK_LEFT),
    (b"up", VK_UP),
    (b"right", VK_RIGHT),
    (b"down", VK_DOWN),
    (b"prtsc", VK_SNAPSHOT),
    (b"ins", VK_INSERT),
    (b"del", VK_DELETE),
    (b"win", VK_LWIN),
    (b"f1", VK_F1),
    (b"f2", VK_F1 + 1),
    (b"f3", VK_F1 + 2),
    (b"f4", VK_F1 + 3),
    (b"f5", VK_F1 + 4),
    (b"f6", VK_F1 + 5),
    (b"f7", VK_F1 + 6),
    (b"f8", VK_F1 + 7),
    (b"f9", VK_F1 + 8),
    (b"f10", VK_F1 + 9),
    (b"f11", VK_F1 + 10),
    (b"f12", VK_F1 + 11),
];

/// 键名 → VK 码（清单声明解析的键名面；大小写不敏感）。
/// 结构：键名表 [`NAME_KEYS`] 线性查找 + 单字符字母/数字直接映射。
pub fn name_to_key(tok: &[u8]) -> Option<u16> {
    if tok.is_empty() {
        return None;
    }
    let mut buf = [0u8; 8];
    if tok.len() > buf.len() {
        return None;
    }
    let mut i = 0usize;
    while i < tok.len() {
        buf[i] = ascii_lower(tok[i]);
        i += 1;
    }
    let t = &buf[..tok.len()];
    let mut j = 0usize;
    while j < NAME_KEYS.len() {
        if NAME_KEYS[j].0 == t {
            return Some(NAME_KEYS[j].1);
        }
        j += 1;
    }
    // 单字符：字母/数字直接映射 VK 码。
    if t.len() == 1 {
        let c = t[0];
        if c.wrapping_sub(b'a') < 26 {
            return Some(VK_A + (c - b'a') as u16);
        }
        if c.wrapping_sub(b'0') < 10 {
            return Some(VK_0 + (c - b'0') as u16);
        }
    }
    None
}

/// 修饰键 token → 位（清单声明解析用；大小写不敏感）。
/// crate 内可见（`super::decl` 的 parse_decl 复用——修饰键语义一处一事实）。
pub(crate) fn mod_from_token(tok: &[u8]) -> Option<u8> {
    let mut buf = [0u8; 6];
    if tok.is_empty() || tok.len() > buf.len() {
        return None;
    }
    for (i, &b) in tok.iter().enumerate() {
        buf[i] = ascii_lower(b);
    }
    match &buf[..tok.len()] {
        b"ctrl" => Some(MOD_CTRL),
        b"alt" => Some(MOD_ALT),
        b"shift" => Some(MOD_SHIFT),
        b"win" => Some(MOD_WIN),
        _ => None,
    }
}

/// 键位显示串（设置中心显示面）：修饰键规范序 + 键名。
/// 返回写入字节数；未知 VK 写 '?'（不静默吞键位）。
/// 修饰键段查表化：四段重复 if 结构归并为规范序表循环——显示序单一
/// 事实源、分支收敛（重构留痕：原实现四段 if 逐段复制粘贴）。
pub fn combo_str(c: Combo, out: &mut [u8]) -> usize {
    /// 追加一段字面量（缓冲截断纪律：越界即丢，不计入长度）。
    /// （命名为 `emit`：追加是确定性操作、无失败路径——截断即目标缓冲
    /// 容量不足这一事实本身，由调用方缓冲约定承担。）
    fn emit(s: &str, out: &mut [u8], n: &mut usize) {
        for &b in s.as_bytes() {
            if *n < out.len() {
                out[*n] = b;
                *n += 1;
            }
        }
    }
    let mut n = 0usize;
    let mut wrote_mod = false;
    for (m, name) in MOD_TABLE.iter() {
        if c.mods & m != 0 {
            if wrote_mod {
                emit("+", out, &mut n);
            }
            emit(name, out, &mut n);
            wrote_mod = true;
        }
    }
    match key_name(c.key) {
        Some(kn) => {
            if wrote_mod {
                emit("+", out, &mut n);
            }
            emit(kn, out, &mut n);
        }
        None => emit("?", out, &mut n),
    }
    n
}
