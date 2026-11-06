//! UNX-F48821 keymap 引擎与修饰键状态机总成（layout/variant/options 三层配置全语义） · 0x70280
//!
//! 覆盖卡：UNX-F48821（三层配置 ×10 组合解析一致）、UNX-F48822（XKB 语义
//! keymap 解析器；解析 ×10 布局零漂移 + 参照版本登记在册）。
//!
//! ── 闸门次序 ──
//! 解析入口 `parse_keymap` 串行七道闸，次序不可换：
//!   1) **头闸**：源里必须出现 `xkb_keymap` 声明行。先判头闸的理由：无头文本
//!      只可能是被截断的配置；若先做括号计数，截断点恰好落在成对括号之后时
//!      「括号平衡」会放行一张没有 keymap 声明的空表，后续层查表退化成
//!      「全键 NoSymbol」，用户只看到按键没反应——静默降级比报错更难查。
//!   2) **版本闸**：`// version 0x0106` 必须落在 `VERSION_LEDGER` 内。跳过它，
//!      未登记版本的三层语义（尤其 Grp 与 Mod5 的位序）会被按当前台账解释，
//!      跨版本 keymap 的组合终态漂移却不报错（F48822 的「版本登记在册」即此闸）。
//!   3) **括号闸**：`{`/`}` 深度中途不得为负、结束必须归零。跳过它，缺 `};`
//!      的 symbols 段会把下一段的 key 声明吞进本段，层号错位。
//!   4) **段闸**：只认 keycodes/types/compatibility/symbols 四段，其余
//!      `xkb_*` 段直接拒；放行等于把未来版本的语义段当注释吞掉。
//!   5) **名闸**：keyname 与 keysym 名必须在两张冻结表内。放行未知名等于把
//!      「拼错的 keysym」解释成 NoSymbol，键位静默失声。
//!   6) **层闸**：层数 ≤4；被取用的层不得是 NoSymbol（strict 取用，取空层报错）。
//!   7) **复现闸**：同一配置「生成→解析」两跑必须同摘要，防表数据被改而判据仍绿。
//!
//! ── 关键差异 ──
//! 与 B01（统一输入事件层，0x70000 段）的边界：B01 判「事件帧是否完整、时间戳
//! 是否单调、分发表是否命中」，只搬运 scancode/keysym，不解释 keysym 语义；
//! 本模块判「一个物理键码在有效修饰选择字 + 三层配置下产出哪个 keysym」。修饰位
//! 的**锁存**归 B02 的 meta 模块（0x702C0），本模块只接受调用方传入的本键有效
//! 修饰选择字，自身不持有跨键状态，因此锁存状态机出错时本模块仍绿——这是刻意
//! 的边界：谁判死谁放行，锁存判死归 meta，扫描码集归一归 B03（0x70500）。
//!
//! 设计纪律：no_std（core+alloc）；生产路径零 unwrap/expect/panic/assert；
//! 取值一律 get/get_mut/match/checked_*；直接下标仅用于已证 `i < len` 的循环。
//! 判据原文锚定：`三层配置 ×10 组合解析一致` / `解析 ×10 布局零漂移 + 版本登记在册`。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（PLAN.md B02 分配，禁止自改）。
pub const SEG_BASE: u32 = 0x70280;

/// keymap 文本内声明的参照版本（XKB spec rev，1.6 = 0x0106）。
pub const XKB_REF_REV: u16 = 0x0106;

/// 台账登记名（AI-94 参照版本备查）。
pub const XKB_REF_NAME: &str = "xkbcommon-1.6.0";

/// 每个 keyboard 配置的符号键位数（本模块冻结 16 键样本）。
pub const KEYS_PER_LAYOUT: usize = 16;

/// 展平层的层数上限（XKB 语义：group0 base/shift + group1 altgr/shift）。
pub const LEVELS_PER_KEY: usize = 4;

/// 十组合判据的轮数（卡面 ×10）。
pub const COMBO_ROUNDS: usize = 10;

/// 注入槽数（十类单故障）。
pub const INJECT_SLOTS: usize = 10;

/// FNV-1a 64 位偏移基。
const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位质数。
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

// ── 修饰选择字（本模块只消费，不锁存） ──────────────────────────
/// 选择字：Shift 位。
pub const SEL_SHIFT: u8 = 0x01;
/// 选择字：Lock（CapsLock 生效）位。
pub const SEL_LOCK: u8 = 0x02;
/// 选择字：AltGr（level 2/3）位。
pub const SEL_ALTGR: u8 = 0x04;
/// 选择字：Control 位（产出控制码，不产出字符）。
pub const SEL_CTRL: u8 = 0x08;

// ── 诊断码闭集（SEG_BASE+1 起连续，12 条） ──────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeymapCode {
    /// 缺 xkb_keymap 头（截断文件）
    MissingHeader,
    /// 花括号不配对（深度为负或非零收尾）
    UnbalancedBrace,
    /// 未知 xkb_* 段
    UnknownSection,
    /// 未知 keysym 名
    UnknownKeysym,
    /// 层号越界（>4 层）或 strict 取到 NoSymbol
    LevelOverflow,
    /// keyname / keycode 未绑定（符号段引用了未登记键名）
    NameUnbound,
    /// keycode 越 8 位
    KeycodeOverflow,
    /// layout/variant 未登记
    VariantUnknown,
    /// option 语法坏或未登记
    OptionMalformed,
    /// 版本不在台账
    VersionUnregistered,
    /// 版本与台账摘要不符
    VersionMismatch,
    /// 双跑/十组合漂移
    NotReproducible,
}

impl KeymapCode {
    /// 段内绝对码值（SEG_BASE + 枚举序）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文逐条文案。
    pub fn detail(self) -> &'static str {
        match self {
            KeymapCode::MissingHeader => "缺 xkb_keymap 头：截断文件不得当空表放行",
            KeymapCode::UnbalancedBrace => "花括号不配对：段边界丢失会吞下一段",
            KeymapCode::UnknownSection => "未知 xkb_* 段：新语义段不得当注释吞掉",
            KeymapCode::UnknownKeysym => "未知 keysym 名：拼错即静默失声",
            KeymapCode::LevelOverflow => "层号越界：>4 层或 strict 取到 NoSymbol",
            KeymapCode::NameUnbound => "键名/键码未绑定：符号段引用了未登记键",
            KeymapCode::KeycodeOverflow => "键码越 8 位：非法 evdev 键码",
            KeymapCode::VariantUnknown => "layout/variant 未登记：三层配置第二层失配",
            KeymapCode::OptionMalformed => "option 语法坏或未登记：三层配置第三层失配",
            KeymapCode::VersionUnregistered => "版本不在台账：未登记的语义不得按当前台账解释",
            KeymapCode::VersionMismatch => "版本与台账摘要不符：表被改写而判据仍绿",
            KeymapCode::NotReproducible => "双跑/十组合漂移：解析不幂等",
        }
    }

    /// 闭集全表（段位守卫与 detail(u32) 共用）。
    pub const ALL: [KeymapCode; 12] = [
        KeymapCode::MissingHeader,
        KeymapCode::UnbalancedBrace,
        KeymapCode::UnknownSection,
        KeymapCode::UnknownKeysym,
        KeymapCode::LevelOverflow,
        KeymapCode::NameUnbound,
        KeymapCode::KeycodeOverflow,
        KeymapCode::VariantUnknown,
        KeymapCode::OptionMalformed,
        KeymapCode::VersionUnregistered,
        KeymapCode::VersionMismatch,
        KeymapCode::NotReproducible,
    ];
}

/// 按绝对码值查文案；闭集外返回显式文案（不返回空串）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < KeymapCode::ALL.len() {
        if let Some(c) = KeymapCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70280 段 keymap 闭集内"
}

// ── 基础设施：FNV-1a 折叠 ───────────────────────────────────────
/// 折叠一个字节。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 折叠一个 32 位字（小端四字节）。
pub fn fold_word(h: u64, w: u32) -> u64 {
    let b = w.to_le_bytes();
    let mut i = 0usize;
    let mut acc = h;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            acc = fold_byte(acc, *v);
        }
        i += 1;
    }
    acc
}

/// 折叠一个字符串（UTF-8 字节序）。
pub fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc = fold_byte(acc, *b);
    }
    acc
}

/// 确定性 LCG（注入与 fuzz 用；种子写死保证复跑）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    /// 固定种子构造。
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed ^ 0x9E37_79B9 }
    }

    /// 下一状态。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }

    /// 取 [0, bound) 内值；bound=0 返回 0（不 panic、不取模零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 文本小工具（no_std，无 std::str） ───────────────────────────
/// 去首尾空格/制表。
pub fn trim(b: &[u8]) -> &[u8] {
    let mut s = 0usize;
    let mut e = b.len();
    while s < e {
        match b.get(s) {
            Some(b' ') | Some(b'\t') => s += 1,
            _ => break,
        }
    }
    while e > s {
        match e.checked_sub(1).and_then(|i| b.get(i)) {
            Some(b' ') | Some(b'\t') => e -= 1,
            _ => break,
        }
    }
    b.get(s..e).unwrap_or(&[])
}

/// 首个 '"' 与配对 '"' 之间的切片。
fn quoted(b: &[u8]) -> Option<&[u8]> {
    let mut i = 0usize;
    while i < b.len() {
        if let Some(b'"') = b.get(i) {
            let mut j = i + 1;
            while j < b.len() {
                if let Some(b'"') = b.get(j) {
                    return b.get(i + 1..j);
                }
                j += 1;
            }
            return None;
        }
        i += 1;
    }
    None
}

/// 单字节分隔的二分（左, 右）。
fn split_once(b: &[u8], sep: u8) -> Option<(&[u8], &[u8])> {
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            if *v == sep {
                let l = b.get(0..i)?;
                let r = b.get(i + 1..)?;
                return Some((l, r));
            }
        }
        i += 1;
    }
    None
}

/// 十进制解析（checked 乘加，溢出返回 None）。
fn parse_dec(b: &[u8]) -> Option<u32> {
    let mut acc: u32 = 0;
    if b.is_empty() {
        return None;
    }
    for c in b {
        if !c.is_ascii_digit() {
            return None;
        }
        let d = (*c as u32).checked_sub('0' as u32)?;
        acc = acc.checked_mul(10)?.checked_add(d)?;
    }
    Some(acc)
}

/// 十六进制解析（0x 前缀可选）。
fn parse_hex(b: &[u8]) -> Option<u32> {
    let body = if b.len() >= 2 && b.get(0) == Some(&b'0') && b.get(1) == Some(&b'x') {
        b.get(2..)?
    } else {
        b
    };
    if body.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    for c in body {
        let d = match c {
            b'0'..=b'9' => (*c as u32) - ('0' as u32),
            b'a'..=b'f' => (*c as u32) - ('a' as u32) + 10,
            b'A'..=b'F' => (*c as u32) - ('A' as u32) + 10,
            _ => return None,
        };
        acc = acc.checked_mul(16)?.checked_add(d)?;
    }
    Some(acc)
}

// ── keysym 名表（冻结；X11 keysymdef 子集） ─────────────────────
/// keysym 名 → 值。
pub const KSYM_NAMES: [(&str, u32); 61] = [
    ("NoSymbol", 0x0000),
    ("space", 0x0020),
    ("exclam", 0x0021),
    ("quotedbl", 0x0022),
    ("numbersign", 0x0023),
    ("dollar", 0x0024),
    ("percent", 0x0025),
    ("ampersand", 0x0026),
    ("apostrophe", 0x0027),
    ("parenleft", 0x0028),
    ("parenright", 0x0029),
    ("asterisk", 0x002a),
    ("plus", 0x002b),
    ("comma", 0x002c),
    ("minus", 0x002d),
    ("period", 0x002e),
    ("slash", 0x002f),
    ("colon", 0x003a),
    ("semicolon", 0x003b),
    ("less", 0x003c),
    ("equal", 0x003d),
    ("greater", 0x003e),
    ("question", 0x003f),
    ("at", 0x0040),
    ("A", 0x0041),
    ("D", 0x0044),
    ("E", 0x0045),
    ("F", 0x0046),
    ("G", 0x0047),
    ("H", 0x0048),
    ("I", 0x0049),
    ("O", 0x004f),
    ("P", 0x0050),
    ("Q", 0x0051),
    ("R", 0x0052),
    ("S", 0x0053),
    ("T", 0x0054),
    ("U", 0x0055),
    ("W", 0x0057),
    ("Y", 0x0059),
    ("Z", 0x005a),
    ("bracketleft", 0x005b),
    ("backslash", 0x005c),
    ("bracketright", 0x005d),
    ("asciicircum", 0x005e),
    ("underscore", 0x005f),
    ("grave", 0x0060),
    ("a", 0x0061),
    ("d", 0x0064),
    ("e", 0x0065),
    ("f", 0x0066),
    ("g", 0x0067),
    ("h", 0x0068),
    ("i", 0x0069),
    ("o", 0x006f),
    ("p", 0x0070),
    ("q", 0x0071),
    ("r", 0x0072),
    ("s", 0x0073),
    ("t", 0x0074),
    ("u", 0x0075),
];

/// keysym 名表续（字母尾部与拉丁补充/死键区）。
pub const KSYM_NAMES_2: [(&str, u32); 57] = [
    ("w", 0x0077),
    ("y", 0x0079),
    ("z", 0x007a),
    ("braceleft", 0x007b),
    ("bar", 0x007c),
    ("braceright", 0x007d),
    ("asciitilde", 0x007e),
    ("nbsp", 0x00a0),
    ("sterling", 0x00a3),
    ("section", 0x00a7),
    ("guillemotleft", 0x00ab),
    ("notsign", 0x00ac),
    ("registered", 0x00ae),
    ("degree", 0x00b0),
    ("twosuperior", 0x00b2),
    ("mu", 0x00b5),
    ("ccedilla", 0x00e7),
    ("Ccedilla", 0x00c7),
    ("eth", 0x00f0),
    ("ssharp", 0x00df),
    ("agrave", 0x00e0),
    ("aacute", 0x00e1),
    ("Aacute", 0x00c1),
    ("acircumflex", 0x00e2),
    ("atilde", 0x00e3),
    ("adiaeresis", 0x00e4),
    ("Adiaeresis", 0x00c4),
    ("aring", 0x00e5),
    ("Aring", 0x00c5),
    ("ae", 0x00e6),
    ("eacute", 0x00e9),
    ("Eacute", 0x00c9),
    ("ecircumflex", 0x00ea),
    ("igrave", 0x00ec),
    ("iacute", 0x00ed),
    ("ntilde", 0x00f1),
    ("oacute", 0x00f3),
    ("odiaeresis", 0x00f6),
    ("Odiaeresis", 0x00d6),
    ("oslash", 0x00f8),
    ("thorn", 0x00fe),
    ("uacute", 0x00fa),
    ("udiaeresis", 0x00fc),
    ("Udiaeresis", 0x00dc),
    ("EuroSign", 0x20ac),
    ("dead_grave", 0xfe50),
    ("dead_acute", 0xfe51),
    ("dead_circumflex", 0xfe52),
    ("dead_tilde", 0xfe53),
    ("dead_macron", 0xfe54),
    ("dead_breve", 0xfe55),
    ("dead_diaeresis", 0xfe57),
    ("dead_caron", 0xfe5a),
    ("dead_cedilla", 0xfe5b),
    ("dead_ogonek", 0xfe5c),
    ("dead_belowdot", 0xfe60),
    ("Multi_key", 0xff20),
];

/// 名 → keysym 值（未知返回 None，由名闸判死）。
pub fn ksym_value(name: &[u8]) -> Option<u32> {
    for (n, v) in KSYM_NAMES.iter() {
        if n.as_bytes() == name {
            return Some(*v);
        }
    }
    for (n, v) in KSYM_NAMES_2.iter() {
        if n.as_bytes() == name {
            return Some(*v);
        }
    }
    None
}

/// keysym 值 → 名（表内无此值返回空串，生成侧改走 0x 数字形）。
pub fn ksym_name(value: u32) -> &'static str {
    for (n, v) in KSYM_NAMES.iter() {
        if *v == value {
            return n;
        }
    }
    for (n, v) in KSYM_NAMES_2.iter() {
        if *v == value {
            return n;
        }
    }
    ""
}

// ── keyname 表（冻结；evdev+8 的 XKB 键码） ─────────────────────
/// 键名 → 缺省键码。
pub const KEYNAMES: [(&str, u16); 46] = [
    ("TLDE", 49),
    ("AE01", 10),
    ("AE02", 11),
    ("AE03", 12),
    ("AE04", 13),
    ("AE05", 14),
    ("AE06", 15),
    ("AE07", 16),
    ("AE08", 17),
    ("AE09", 18),
    ("AE10", 19),
    ("AE11", 20),
    ("AE12", 21),
    ("AD01", 24),
    ("AD02", 25),
    ("AD03", 26),
    ("AD04", 27),
    ("AD05", 28),
    ("AD06", 29),
    ("AD07", 30),
    ("AD08", 31),
    ("AD09", 32),
    ("AD10", 33),
    ("AD11", 34),
    ("AD12", 35),
    ("AC01", 38),
    ("AC02", 39),
    ("AC03", 40),
    ("AC04", 41),
    ("AC05", 42),
    ("AC06", 43),
    ("AC07", 44),
    ("AC08", 45),
    ("AC09", 46),
    ("AC10", 47),
    ("AC11", 48),
    ("BKSL", 51),
    ("LSGT", 94),
    ("SPCE", 65),
    ("RTRN", 36),
    ("LFSH", 50),
    ("RTSH", 62),
    ("CAPS", 66),
    ("LCTL", 37),
    ("RALT", 108),
    ("NMLK", 77),
];

/// 名 → 静态键名（未登记返回 None）。
pub fn canon_keyname(b: &[u8]) -> Option<&'static str> {
    for (n, _) in KEYNAMES.iter() {
        if n.as_bytes() == b {
            return Some(n);
        }
    }
    None
}

/// 名 → 缺省键码（仅供生成侧断言键名与键码一致）。
pub fn keycode_of(name: &str) -> Option<u16> {
    for (n, kc) in KEYNAMES.iter() {
        if *n == name {
            return Some(*kc);
        }
    }
    None
}

// ── 三层配置：layout / variant / options ────────────────────────
/// 已登记的 layout（第一层）。
pub const LAYOUTS: [&str; 4] = ["us", "de", "fr", "gb"];

/// 已登记的 (layout, variant) 对（第二层）。
pub const VARIANTS: [(&str, &str); 10] = [
    ("us", "basic"),
    ("us", "intl"),
    ("us", "dvorak"),
    ("us", "altgr-intl"),
    ("de", "basic"),
    ("de", "nodeadkeys"),
    ("fr", "basic"),
    ("fr", "oss"),
    ("gb", "basic"),
    ("gb", "intl"),
];

/// 已登记的 option 与位（第三层）。
pub const OPTIONS: [(&str, u32); 4] = [
    ("lv3:ralt_switch", 0x01),
    ("grp:alt_shift_toggle", 0x02),
    ("caps:escape", 0x04),
    ("compose:ralt", 0x08),
];

/// 十组合判据的规范配置串（F48821 卡面 ×10）。
pub const COMBO_SPECS: [&str; COMBO_ROUNDS] = [
    "us",
    "us+intl",
    "us+dvorak",
    "us+altgr-intl",
    "de",
    "de+nodeadkeys",
    "de+nodeadkeys+lv3:ralt_switch",
    "fr",
    "fr+oss+grp:alt_shift_toggle",
    "gb+intl",
];

/// version 台账：参照名、spec rev、该版本 keymap 文本摘要。
pub const VERSION_LEDGER: [(&str, u16, u64); 2] = [
    ("xkbcommon-1.5.0", 0x0105, 0x2b1f_6c4e_91a0_3357),
    ("xkbcommon-1.6.0", 0x0106, 0x71c3_88ea_4d02_b95f),
];

/// spec rev → 台账项。
pub fn version_entry(rev: u16) -> Option<(&'static str, u16, u64)> {
    for e in VERSION_LEDGER.iter() {
        if e.1 == rev {
            return Some(*e);
        }
    }
    None
}

/// 版本台账（可写账）：登记 / 查询 / 摘要核对。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionBook {
    /// 账目：(名称, rev, 摘要)。
    pub rows: Vec<(&'static str, u16, u64)>,
}

impl VersionBook {
    /// 空账。
    pub fn new() -> VersionBook {
        VersionBook { rows: Vec::new() }
    }

    /// 以冻结台账为初值（AI-94 备查口径）。
    pub fn from_ledger() -> VersionBook {
        let mut b = VersionBook::new();
        for r in VERSION_LEDGER.iter() {
            b.rows.push(*r);
        }
        b
    }

    /// 登记；同名或同 rev 重复一律拒（账不得撞）。
    pub fn register(&mut self, name: &'static str, rev: u16, digest: u64) -> Result<(), u32> {
        if name.is_empty() || rev == 0 {
            return Err(KeymapCode::VersionUnregistered.val());
        }
        for r in self.rows.iter() {
            if r.0 == name || r.1 == rev {
                return Err(KeymapCode::VersionMismatch.val());
            }
        }
        if self.rows.len() >= 8 {
            return Err(KeymapCode::VersionMismatch.val());
        }
        self.rows.push((name, rev, digest));
        Ok(())
    }

    /// 按 rev 查询。
    pub fn query(&self, rev: u16) -> Option<(&'static str, u16, u64)> {
        for r in self.rows.iter() {
            if r.1 == rev {
                return Some(*r);
            }
        }
        None
    }

    /// 摘要核对：rev 在账且摘要一致才算「在册」。
    pub fn verify(&self, rev: u16, digest: u64) -> Result<(), u32> {
        match self.query(rev) {
            None => Err(KeymapCode::VersionUnregistered.val()),
            Some(r) => {
                if r.2 == digest {
                    Ok(())
                } else {
                    Err(KeymapCode::VersionMismatch.val())
                }
            }
        }
    }
}

// ── 符号表（十组 layout×variant，每组 16 键） ───────────────────
/// 一组符号：layout、variant、键位四层。
pub struct SymGroup {
    /// 第一层：布局名。
    pub layout: &'static str,
    /// 第二层：变体名。
    pub variant: &'static str,
    /// 键位：(键码, 键名, [base, shift, altgr, altgr+shift])。
    pub keys: &'static [(u16, &'static str, [u32; LEVELS_PER_KEY])],
}

/// 十组符号表（生成侧的唯一真源；解析侧只认文本，不读本表）。
pub const SYM_TABLE: [SymGroup; 10] = [
    SymGroup {
        layout: "us",
        variant: "basic",
        keys: &[
            (49, "TLDE", [0x0060, 0x007e, 0, 0]),
            (24, "AD01", [0x0071, 0x0051, 0, 0]),
            (25, "AD02", [0x0077, 0x0057, 0, 0]),
            (26, "AD03", [0x0065, 0x0045, 0, 0]),
            (27, "AD04", [0x0072, 0x0052, 0, 0]),
            (28, "AD05", [0x0074, 0x0054, 0, 0]),
            (29, "AD06", [0x0079, 0x0059, 0, 0]),
            (38, "AC01", [0x0061, 0x0041, 0, 0]),
            (39, "AC02", [0x0073, 0x0053, 0, 0]),
            (40, "AC03", [0x0064, 0x0044, 0, 0]),
            (41, "AC04", [0x0066, 0x0046, 0, 0]),
            (42, "AC05", [0x0067, 0x0047, 0, 0]),
            (43, "AC06", [0x0068, 0x0048, 0, 0]),
            (51, "BKSL", [0x005c, 0x007c, 0, 0]),
            (94, "LSGT", [0, 0, 0, 0]),
            (65, "SPCE", [0x0020, 0x0020, 0, 0]),
        ],
    },
    SymGroup {
        layout: "us",
        variant: "intl",
        keys: &[
            (49, "TLDE", [0x0060, 0x007e, 0xfe53, 0xfe57]),
            (24, "AD01", [0x0071, 0x0051, 0x00e4, 0x00c4]),
            (25, "AD02", [0x0077, 0x0057, 0x00e5, 0x00c5]),
            (26, "AD03", [0x0065, 0x0045, 0x00e9, 0x00c9]),
            (27, "AD04", [0x0072, 0x0052, 0x00ae, 0]),
            (28, "AD05", [0x0074, 0x0054, 0x00fe, 0]),
            (29, "AD06", [0x0079, 0x0059, 0x00fc, 0x00dc]),
            (38, "AC01", [0x0061, 0x0041, 0x00e1, 0x00c1]),
            (39, "AC02", [0x0073, 0x0053, 0x00df, 0]),
            (40, "AC03", [0x0064, 0x0044, 0x00f0, 0]),
            (41, "AC04", [0x0066, 0x0046, 0, 0]),
            (42, "AC05", [0x0067, 0x0047, 0, 0]),
            (43, "AC06", [0x0068, 0x0048, 0, 0]),
            (51, "BKSL", [0x005c, 0x007c, 0xfe51, 0]),
            (94, "LSGT", [0, 0, 0, 0]),
            (65, "SPCE", [0x0020, 0x0020, 0x00a0, 0]),
        ],
    },
    SymGroup {
        layout: "us",
        variant: "dvorak",
        keys: &[
            (49, "TLDE", [0x0060, 0x007e, 0, 0]),
            (24, "AD01", [0x0027, 0x0022, 0, 0]),
            (25, "AD02", [0x002c, 0x003c, 0, 0]),
            (26, "AD03", [0x002e, 0x003e, 0, 0]),
            (27, "AD04", [0x0070, 0x0050, 0, 0]),
            (28, "AD05", [0x0079, 0x0059, 0, 0]),
            (29, "AD06", [0x0066, 0x0046, 0, 0]),
            (38, "AC01", [0x0061, 0x0041, 0, 0]),
            (39, "AC02", [0x006f, 0x004f, 0, 0]),
            (40, "AC03", [0x0065, 0x0045, 0, 0]),
            (41, "AC04", [0x0075, 0x0055, 0, 0]),
            (42, "AC05", [0x0069, 0x0049, 0, 0]),
            (43, "AC06", [0x0064, 0x0044, 0, 0]),
            (51, "BKSL", [0x005c, 0x007c, 0, 0]),
            (94, "LSGT", [0, 0, 0, 0]),
            (65, "SPCE", [0x0020, 0x0020, 0, 0]),
        ],
    },
    SymGroup {
        layout: "us",
        variant: "altgr-intl",
        keys: &[
            (49, "TLDE", [0x0060, 0x007e, 0xfe50, 0xfe53]),
            (24, "AD01", [0x0071, 0x0051, 0x00e4, 0x00c4]),
            (25, "AD02", [0x0077, 0x0057, 0x00e5, 0x00c5]),
            (26, "AD03", [0x0065, 0x0045, 0x00e9, 0x00c9]),
            (27, "AD04", [0x0072, 0x0052, 0x00ae, 0]),
            (28, "AD05", [0x0074, 0x0054, 0x00fe, 0]),
            (29, "AD06", [0x0079, 0x0059, 0x00fc, 0x00dc]),
            (38, "AC01", [0x0061, 0x0041, 0x00e1, 0x00c1]),
            (39, "AC02", [0x0073, 0x0053, 0x00df, 0]),
            (40, "AC03", [0x0064, 0x0044, 0x00f0, 0]),
            (41, "AC04", [0x0066, 0x0046, 0, 0]),
            (42, "AC05", [0x0067, 0x0047, 0, 0]),
            (43, "AC06", [0x0068, 0x0048, 0, 0]),
            (51, "BKSL", [0x005c, 0x007c, 0xfe51, 0]),
            (94, "LSGT", [0, 0, 0, 0]),
            (65, "SPCE", [0x0020, 0x0020, 0x00a0, 0]),
        ],
    },
    SymGroup {
        layout: "de",
        variant: "basic",
        keys: &[
            (49, "TLDE", [0xfe52, 0x00b0, 0, 0]),
            (24, "AD01", [0x0071, 0x0051, 0x0040, 0]),
            (25, "AD02", [0x0077, 0x0057, 0, 0]),
            (26, "AD03", [0x0065, 0x0045, 0x20ac, 0]),
            (27, "AD04", [0x0072, 0x0052, 0, 0]),
            (28, "AD05", [0x0074, 0x0054, 0, 0]),
            (29, "AD06", [0x007a, 0x005a, 0, 0]),
            (38, "AC01", [0x0061, 0x0041, 0, 0]),
            (39, "AC02", [0x0073, 0x0053, 0, 0]),
            (40, "AC03", [0x0064, 0x0044, 0, 0]),
            (41, "AC04", [0x0066, 0x0046, 0, 0]),
            (42, "AC05", [0x0067, 0x0047, 0, 0]),
            (43, "AC06", [0x0068, 0x0048, 0, 0]),
            (51, "BKSL", [0x0023, 0x0027, 0, 0]),
            (94, "LSGT", [0x003c, 0x003e, 0x007c, 0]),
            (65, "SPCE", [0x0020, 0x0020, 0, 0]),
        ],
    },
    SymGroup {
        layout: "de",
        variant: "nodeadkeys",
        keys: &[
            (49, "TLDE", [0x005e, 0x00b0, 0, 0]),
            (24, "AD01", [0x0071, 0x0051, 0x0040, 0]),
            (25, "AD02", [0x0077, 0x0057, 0, 0]),
            (26, "AD03", [0x0065, 0x0045, 0x20ac, 0]),
            (27, "AD04", [0x0072, 0x0052, 0, 0]),
            (28, "AD05", [0x0074, 0x0054, 0, 0]),
            (29, "AD06", [0x007a, 0x005a, 0, 0]),
            (38, "AC01", [0x0061, 0x0041, 0, 0]),
            (39, "AC02", [0x0073, 0x0053, 0, 0]),
            (40, "AC03", [0x0064, 0x0044, 0, 0]),
            (41, "AC04", [0x0066, 0x0046, 0, 0]),
            (42, "AC05", [0x0067, 0x0047, 0, 0]),
            (43, "AC06", [0x0068, 0x0048, 0, 0]),
            (51, "BKSL", [0x0023, 0x0027, 0, 0]),
            (94, "LSGT", [0x003c, 0x003e, 0x007c, 0]),
            (65, "SPCE", [0x0020, 0x0020, 0, 0]),
        ],
    },
    SymGroup {
        layout: "fr",
        variant: "basic",
        keys: &[
            (49, "TLDE", [0x00b2, 0, 0, 0]),
            (24, "AD01", [0x0061, 0x0041, 0, 0]),
            (25, "AD02", [0x007a, 0x005a, 0, 0]),
            (26, "AD03", [0x0065, 0x0045, 0x20ac, 0]),
            (27, "AD04", [0x0072, 0x0052, 0, 0]),
            (28, "AD05", [0x0074, 0x0054, 0, 0]),
            (29, "AD06", [0x0079, 0x0059, 0, 0]),
            (38, "AC01", [0x0071, 0x0051, 0, 0]),
            (39, "AC02", [0x0073, 0x0053, 0, 0]),
            (40, "AC03", [0x0064, 0x0044, 0, 0]),
            (41, "AC04", [0x0066, 0x0046, 0, 0]),
            (42, "AC05", [0x0067, 0x0047, 0, 0]),
            (43, "AC06", [0x0068, 0x0048, 0, 0]),
            (51, "BKSL", [0x00b5, 0, 0, 0]),
            (94, "LSGT", [0x003c, 0x003e, 0, 0]),
            (65, "SPCE", [0x0020, 0x0020, 0, 0]),
        ],
    },
    SymGroup {
        layout: "fr",
        variant: "oss",
        keys: &[
            (49, "TLDE", [0x00b2, 0, 0xfe50, 0]),
            (24, "AD01", [0x0061, 0x0041, 0x00e6, 0]),
            (25, "AD02", [0x007a, 0x005a, 0x00ab, 0]),
            (26, "AD03", [0x0065, 0x0045, 0x20ac, 0]),
            (27, "AD04", [0x0072, 0x0052, 0, 0]),
            (28, "AD05", [0x0074, 0x0054, 0, 0]),
            (29, "AD06", [0x0079, 0x0059, 0x00a7, 0]),
            (38, "AC01", [0x0071, 0x0051, 0, 0]),
            (39, "AC02", [0x0073, 0x0053, 0, 0]),
            (40, "AC03", [0x0064, 0x0044, 0, 0]),
            (41, "AC04", [0x0066, 0x0046, 0, 0]),
            (42, "AC05", [0x0067, 0x0047, 0, 0]),
            (43, "AC06", [0x0068, 0x0048, 0, 0]),
            (51, "BKSL", [0x00b5, 0x00a3, 0, 0]),
            (94, "LSGT", [0x003c, 0x003e, 0, 0]),
            (65, "SPCE", [0x0020, 0x0020, 0, 0]),
        ],
    },
    SymGroup {
        layout: "gb",
        variant: "basic",
        keys: &[
            (49, "TLDE", [0x0060, 0x00ac, 0, 0]),
            (24, "AD01", [0x0071, 0x0051, 0, 0]),
            (25, "AD02", [0x0077, 0x0057, 0, 0]),
            (26, "AD03", [0x0065, 0x0045, 0, 0]),
            (27, "AD04", [0x0072, 0x0052, 0, 0]),
            (28, "AD05", [0x0074, 0x0054, 0, 0]),
            (29, "AD06", [0x0079, 0x0059, 0, 0]),
            (38, "AC01", [0x0061, 0x0041, 0, 0]),
            (39, "AC02", [0x0073, 0x0053, 0, 0]),
            (40, "AC03", [0x0064, 0x0044, 0, 0]),
            (41, "AC04", [0x0066, 0x0046, 0, 0]),
            (42, "AC05", [0x0067, 0x0047, 0, 0]),
            (43, "AC06", [0x0068, 0x0048, 0, 0]),
            (51, "BKSL", [0x0023, 0x007e, 0, 0]),
            (94, "LSGT", [0, 0, 0, 0]),
            (65, "SPCE", [0x0020, 0x0020, 0, 0]),
        ],
    },
    SymGroup {
        layout: "gb",
        variant: "intl",
        keys: &[
            (49, "TLDE", [0x0060, 0x00ac, 0xfe50, 0]),
            (24, "AD01", [0x0071, 0x0051, 0, 0]),
            (25, "AD02", [0x0077, 0x0057, 0, 0]),
            (26, "AD03", [0x0065, 0x0045, 0x20ac, 0]),
            (27, "AD04", [0x0072, 0x0052, 0, 0]),
            (28, "AD05", [0x0074, 0x0054, 0, 0]),
            (29, "AD06", [0x0079, 0x0059, 0, 0]),
            (38, "AC01", [0x0061, 0x0041, 0x00e1, 0]),
            (39, "AC02", [0x0073, 0x0053, 0, 0]),
            (40, "AC03", [0x0064, 0x0044, 0, 0]),
            (41, "AC04", [0x0066, 0x0046, 0, 0]),
            (42, "AC05", [0x0067, 0x0047, 0, 0]),
            (43, "AC06", [0x0068, 0x0048, 0, 0]),
            (51, "BKSL", [0x0023, 0x007e, 0xfe51, 0]),
            (94, "LSGT", [0, 0, 0, 0]),
            (65, "SPCE", [0x0020, 0x0020, 0x00a0, 0]),
        ],
    },
];

/// (layout, variant) → 符号组。
pub fn sym_group(layout: &str, variant: &str) -> Option<&'static SymGroup> {
    for g in SYM_TABLE.iter() {
        if g.layout == layout && g.variant == variant {
            return Some(g);
        }
    }
    None
}

// ── 三层配置解析 ────────────────────────────────────────────────
/// 三层配置的解析结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerSpec {
    /// 第一层：layout。
    pub layout: &'static str,
    /// 第二层：variant。
    pub variant: &'static str,
    /// 第三层：option 位图。
    pub opts: u32,
}

/// 配置串 → 三层。语法：`layout[+variant|+option[:arg]]...`。
pub fn parse_spec(spec: &str) -> Result<LayerSpec, u32> {
    let bytes = spec.as_bytes();
    let (head, tail) = match split_once(bytes, b'+') {
        Some((h, t)) => (h, Some(t)),
        None => (bytes, None),
    };
    let mut layout: Option<&'static str> = None;
    for l in LAYOUTS.iter() {
        if l.as_bytes() == head {
            layout = Some(l);
        }
    }
    let lay = layout.ok_or(KeymapCode::VariantUnknown.val())?;
    let mut variant: &'static str = "";
    let mut opts: u32 = 0;
    if let Some(mut rest) = tail {
        loop {
            let (tok, next) = match split_once(rest, b'+') {
                Some((a, b)) => (a, Some(b)),
                None => (rest, None),
            };
            if tok.is_empty() {
                return Err(KeymapCode::OptionMalformed.val());
            }
            if split_once(tok, b':').is_some() {
                let mut bit = 0u32;
                for (name, b) in OPTIONS.iter() {
                    if name.as_bytes() == tok {
                        bit = *b;
                    }
                }
                if bit == 0 {
                    return Err(KeymapCode::OptionMalformed.val());
                }
                if (opts & bit) != 0 {
                    return Err(KeymapCode::OptionMalformed.val());
                }
                opts |= bit;
            } else {
                let mut found = false;
                for (l, v) in VARIANTS.iter() {
                    if *l == lay && v.as_bytes() == tok {
                        variant = v;
                        found = true;
                    }
                }
                if !found {
                    return Err(KeymapCode::VariantUnknown.val());
                }
                if !variant.is_empty() && variant.as_bytes() != tok {
                    return Err(KeymapCode::VariantUnknown.val());
                }
            }
            match next {
                Some(n) => rest = n,
                None => break,
            }
        }
    }
    if variant.is_empty() {
        variant = "basic";
        let mut ok = false;
        for (l, v) in VARIANTS.iter() {
            if *l == lay && *v == variant {
                ok = true;
            }
        }
        if !ok {
            return Err(KeymapCode::VariantUnknown.val());
        }
    }
    Ok(LayerSpec { layout: lay, variant, opts })
}

// ── 生成侧：三层配置 → XKB 语义文本 ────────────────────────────
/// 生成 keymap 文本（解析侧的唯一输入源；options 以注释行承载）。
pub fn generate_keymap_text(ls: &LayerSpec) -> Result<String, u32> {
    let g = sym_group(ls.layout, ls.variant).ok_or(KeymapCode::VariantUnknown.val())?;
    let mut s = String::new();
    s.push_str("xkb_keymap \"");
    s.push_str(ls.layout);
    s.push('/');
    s.push_str(ls.variant);
    s.push_str("\" {\n");
    s.push_str("// version 0x");
    let hex = format!("{:04x}", XKB_REF_REV);
    s.push_str(hex.as_str());
    s.push('\n');
    for (name, bit) in OPTIONS.iter() {
        if (ls.opts & *bit) != 0 {
            s.push_str("// options ");
            s.push_str(name);
            s.push('\n');
        }
    }
    s.push_str("  xkb_keycodes \"evdev+vx\" {\n");
    for (kc, name, _) in g.keys.iter() {
        s.push_str("    <");
        s.push_str(name);
        s.push_str("> = ");
        let d = format!("{}", kc);
        s.push_str(d.as_str());
        s.push_str(";\n");
    }
    s.push_str("  };\n");
    s.push_str("  xkb_types \"vx\" {\n");
    s.push_str("    type \"ALPHABETIC\" { modifiers = Shift+Lock; };\n");
    s.push_str("  };\n");
    s.push_str("  xkb_compatibility \"vx\" {\n");
    s.push_str("    interpret Shift_L { action = SetMods(modifiers=Shift); };\n");
    s.push_str("  };\n");
    s.push_str("  xkb_symbols \"");
    s.push_str(ls.variant);
    s.push_str("\" {\n");
    for (_, name, levels) in g.keys.iter() {
        s.push_str("    key <");
        s.push_str(name);
        s.push_str("> { [ ");
        let mut i = 0usize;
        while i < LEVELS_PER_KEY {
            if i > 0 {
                s.push_str(", ");
            }
            let v = levels.get(i).copied().unwrap_or(0);
            let n = ksym_name(v);
            if n.is_empty() {
                let h = format!("0x{:04x}", v);
                s.push_str(h.as_str());
            } else {
                s.push_str(n);
            }
            i += 1;
        }
        s.push_str(" ] };\n");
    }
    s.push_str("  };\n");
    s.push_str("};\n");
    Ok(s)
}

// ── 解析侧：XKB 语义文本 → ParsedLayout ────────────────────────
/// 单键符号项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEntry {
    /// evdev+8 键码。
    pub kc: u16,
    /// 静态键名。
    pub name: &'static str,
    /// 四层 keysym。
    pub levels: [u32; LEVELS_PER_KEY],
}

/// 解析产物。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedLayout {
    /// 第一层。
    pub layout: &'static str,
    /// 第二层。
    pub variant: &'static str,
    /// 第三层位图。
    pub opts: u32,
    /// 文本声明的 version。
    pub version: u16,
    /// types 段声明的修饰位。
    pub type_mods: u32,
    /// 键位表（按文本出现序）。
    pub keys: Vec<KeyEntry>,
    /// 全文摘要。
    pub digest: u64,
}

/// 段状态（解析机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sec {
    /// 结构层（keymap 外层）
    Top,
    /// keycodes 段
    Keycodes,
    /// types 段
    Types,
    /// compatibility 段
    Compat,
    /// symbols 段
    Symbols,
}

/// 修饰名 → 位（types 段用）。
const MOD_NAMES: [(&str, u32); 6] = [
    ("Shift", 0x01),
    ("Lock", 0x02),
    ("Control", 0x04),
    ("Mod1", 0x08),
    ("Mod5", 0x20),
    ("Any", 0x7f),
];

/// layout 名 → 静态名。
fn canon_layout(b: &[u8]) -> Option<&'static str> {
    for l in LAYOUTS.iter() {
        if l.as_bytes() == b {
            return Some(l);
        }
    }
    None
}

/// variant 名 → 静态名（跨 layout 全集）。
fn canon_variant(b: &[u8]) -> Option<&'static str> {
    for (_, v) in VARIANTS.iter() {
        if v.as_bytes() == b {
            return Some(v);
        }
    }
    None
}

/// option 名 → 位。
fn option_bit(b: &[u8]) -> Option<u32> {
    for (n, bit) in OPTIONS.iter() {
        if n.as_bytes() == b {
            return Some(*bit);
        }
    }
    None
}

/// 解析主入口：七道闸按序判死（见模块头）。
pub fn parse_keymap(src: &[u8]) -> Result<ParsedLayout, u32> {
    let mut has_header = false;
    let mut head_layout: Option<&'static str> = None;
    let mut head_variant: Option<&'static str> = None;
    let mut version: Option<u16> = None;
    let mut opts: u32 = 0;
    let mut type_mods: u32 = 0;
    let mut sym_section: Option<&'static str> = None;
    let mut depth: i32 = 0;
    let mut brace_err: Option<u32> = None;
    let mut sec_err: Option<u32> = None;
    let mut name_err: Option<u32> = None;
    let mut lvl_err: Option<u32> = None;
    let mut codes: Vec<(&'static str, u16)> = Vec::new();
    let mut keys: Vec<KeyEntry> = Vec::new();
    let mut sec = Sec::Top;

    let mut pos = 0usize;
    while pos < src.len() {
        let start = pos;
        let mut end = start;
        while end < src.len() && src.get(end).copied() != Some(b'\n') {
            end += 1;
        }
        let mut stop = end;
        while stop > start && src.get(stop.wrapping_sub(1)).copied() == Some(b'\r') {
            stop -= 1;
        }
        let raw = src.get(start..stop).unwrap_or(&[]);
        pos = if end < src.len() { end + 1 } else { end };
        let t = trim(raw);
        if t.is_empty() {
            continue;
        }
        // 花括号记账（先记，后判内容）。
        for c in t.iter() {
            if *c == b'{' {
                depth += 1;
            } else if *c == b'}' {
                depth -= 1;
                if depth < 0 && brace_err.is_none() {
                    brace_err = Some(KeymapCode::UnbalancedBrace.val());
                }
                if depth <= 1 {
                    sec = Sec::Top;
                }
            }
        }
        // 注释行：版本 / options 承载。
        if t.starts_with(b"//") {
            if t.starts_with(b"// version ") {
                if let Some(v) = t.get(11..) {
                    match parse_hex(trim(v)) {
                        Some(rev) if rev <= 0xffff => version = Some(rev as u16),
                        _ => {
                            if sec_err.is_none() {
                                sec_err = Some(KeymapCode::VersionUnregistered.val());
                            }
                        }
                    }
                }
            } else if t.starts_with(b"// options ") {
                if let Some(v) = t.get(11..) {
                    match option_bit(trim(v)) {
                        Some(b) => opts |= b,
                        None => {
                            if sec_err.is_none() {
                                sec_err = Some(KeymapCode::OptionMalformed.val());
                            }
                        }
                    }
                }
            }
            continue;
        }
        // 段头。
        if t.starts_with(b"xkb_keymap") {
            has_header = true;
            if let Some(q) = quoted(t) {
                if let Some((l, v)) = split_once(q, b'/') {
                    head_layout = canon_layout(l);
                    head_variant = canon_variant(v);
                }
            }
            sec = Sec::Top;
            continue;
        }
        if t.starts_with(b"xkb_keycodes") {
            sec = Sec::Keycodes;
            continue;
        }
        if t.starts_with(b"xkb_types") {
            sec = Sec::Types;
            continue;
        }
        if t.starts_with(b"xkb_compatibility") {
            sec = Sec::Compat;
            continue;
        }
        if t.starts_with(b"xkb_symbols") {
            sec = Sec::Symbols;
            if let Some(q) = quoted(t) {
                match canon_variant(q) {
                    Some(v) => sym_section = Some(v),
                    None => {
                        if name_err.is_none() {
                            name_err = Some(KeymapCode::VariantUnknown.val());
                        }
                    }
                }
            }
            continue;
        }
        if t.starts_with(b"xkb_") {
            if sec_err.is_none() {
                sec_err = Some(KeymapCode::UnknownSection.val());
            }
            continue;
        }
        if t.starts_with(b"type ") {
            // type "ALPHABETIC" { modifiers = Shift+Lock; };
            match parse_mod_list(t) {
                Some(bits) => type_mods |= bits,
                None => {
                    if name_err.is_none() {
                        name_err = Some(KeymapCode::UnknownKeysym.val());
                    }
                }
            }
            continue;
        }
        match sec {
            Sec::Keycodes => {
                if t.starts_with(b"<") && t.contains(&b'=') {
                    let (name, kc) = match parse_keycode_line(t) {
                        Some(p) => p,
                        None => {
                            if name_err.is_none() {
                                name_err = Some(KeymapCode::NameUnbound.val());
                            }
                            continue;
                        }
                    };
                    if kc > 0xff {
                        if name_err.is_none() {
                            name_err = Some(KeymapCode::KeycodeOverflow.val());
                        }
                        continue;
                    }
                    codes.push((name, kc));
                }
            }
            Sec::Symbols => {
                if t.starts_with(b"key ") {
                    match parse_symbol_line(t) {
                        Ok(mut e) => {
                            let mut kc = e.kc;
                            if kc == 0 {
                                let mut found = 0u16;
                                for (n, k) in codes.iter() {
                                    if *n == e.name {
                                        found = *k;
                                    }
                                }
                                if found == 0 {
                                    if name_err.is_none() {
                                        name_err = Some(KeymapCode::NameUnbound.val());
                                    }
                                    continue;
                                }
                                kc = found;
                            }
                            e.kc = kc;
                            keys.push(e);
                        }
                        Err(code) => match code {
                            x if x == KeymapCode::LevelOverflow.val() => {
                                if lvl_err.is_none() {
                                    lvl_err = Some(x);
                                }
                            }
                            x => {
                                if name_err.is_none() {
                                    name_err = Some(x);
                                }
                            }
                        },
                    }
                }
            }
            _ => {}
        }
    }

    // 闸 1：头闸。
    if !has_header {
        return Err(KeymapCode::MissingHeader.val());
    }
    // 闸 2：版本闸。
    let rev = match version {
        Some(v) => v,
        None => return Err(KeymapCode::VersionUnregistered.val()),
    };
    if version_entry(rev).is_none() {
        return Err(KeymapCode::VersionUnregistered.val());
    }
    // 闸 3：括号闸。
    if depth != 0 && brace_err.is_none() {
        brace_err = Some(KeymapCode::UnbalancedBrace.val());
    }
    if let Some(e) = brace_err {
        return Err(e);
    }
    // 闸 4：段闸。
    if let Some(e) = sec_err {
        return Err(e);
    }
    // 闸 5：名闸。
    if let Some(e) = name_err {
        return Err(e);
    }
    // 闸 6：层闸。
    if let Some(e) = lvl_err {
        return Err(e);
    }
    // symbols 段名必须与头声明一致（variant 层失配）。
    match (head_variant, sym_section) {
        (Some(a), Some(b)) => {
            if a != b {
                return Err(KeymapCode::VariantUnknown.val());
            }
        }
        _ => return Err(KeymapCode::VariantUnknown.val()),
    }
    let lay = match head_layout {
        Some(l) => l,
        None => return Err(KeymapCode::VariantUnknown.val()),
    };
    if keys.is_empty() {
        return Err(KeymapCode::NameUnbound.val());
    }
    // 闸 7：复现闸由调用方双跑（此处给出本跑摘要）。
    let mut digest = fold_str(FNV_BASIS, lay);
    if let Some(v) = head_variant {
        digest = fold_str(digest, v);
    }
    digest = fold_word(digest, opts);
    digest = fold_word(digest, rev as u32);
    digest = fold_word(digest, type_mods);
    for k in keys.iter() {
        digest = fold_word(digest, k.kc as u32);
        let mut i = 0usize;
        while i < LEVELS_PER_KEY {
            digest = fold_word(digest, k.levels.get(i).copied().unwrap_or(0));
            i += 1;
        }
    }
    Ok(ParsedLayout {
        layout: lay,
        variant: head_variant.unwrap_or("basic"),
        opts,
        version: rev,
        type_mods,
        keys,
        digest,
    })
}

/// 在切片中找 token 起始位置。
fn find_token(b: &[u8], tok: &[u8]) -> Option<usize> {
    if tok.is_empty() || b.len() < tok.len() {
        return None;
    }
    let last = b.len() - tok.len();
    let mut i = 0usize;
    while i <= last {
        let mut ok = true;
        let mut j = 0usize;
        while j < tok.len() {
            match (b.get(i + j), tok.get(j)) {
                (Some(a), Some(c)) => {
                    if a != c {
                        ok = false;
                        break;
                    }
                }
                _ => {
                    ok = false;
                    break;
                }
            }
            j += 1;
        }
        if ok {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 解析 types 段的 `modifiers = Shift+Lock;` 列表（未知修饰名返回 None）。
fn parse_mod_list(t: &[u8]) -> Option<u32> {
    let mi = find_token(t, b"modifiers")?;
    let after = t.get(mi..)?;
    let eq = find_token(after, b"=")?;
    let rhs = after.get(eq + 1..)?;
    let mut body = trim(rhs);
    if let Some(semi) = find_token(body, b";") {
        body = trim(body.get(0..semi)?);
    }
    if body.is_empty() {
        return None;
    }
    let mut bits = 0u32;
    let mut rest = body;
    loop {
        let (tok, next) = match split_once(rest, b'+') {
            Some((a, b)) => (a, Some(b)),
            None => (rest, None),
        };
        let name = trim(tok);
        let mut bit = 0u32;
        for (n, b) in MOD_NAMES.iter() {
            if n.as_bytes() == name {
                bit = *b;
            }
        }
        if bit == 0 {
            return None;
        }
        bits |= bit;
        match next {
            Some(x) => rest = x,
            None => break,
        }
    }
    Some(bits)
}

/// 解析 `<NAME> = 24;` 行。
fn parse_keycode_line(t: &[u8]) -> Option<(&'static str, u16)> {
    let lt = find_token(t, b"<")?;
    let rest = t.get(lt + 1..)?;
    let gt = find_token(rest, b">")?;
    let name = canon_keyname(rest.get(0..gt)?)?;
    let after = rest.get(gt + 1..)?;
    let eq = find_token(after, b"=")?;
    let tail = trim(after.get(eq + 1..)?);
    let semi = find_token(tail, b";").unwrap_or(tail.len());
    let num = parse_dec(trim(tail.get(0..semi)?))?;
    if num > 0xffff {
        return None;
    }
    Some((name, num as u16))
}

/// 解析 `key <NAME> { [ a, b, c, d ] };` 行。
fn parse_symbol_line(t: &[u8]) -> Result<KeyEntry, u32> {
    let lt = find_token(t, b"<").ok_or(KeymapCode::NameUnbound.val())?;
    let rest = t.get(lt + 1..).ok_or(KeymapCode::NameUnbound.val())?;
    let gt = find_token(rest, b">").ok_or(KeymapCode::NameUnbound.val())?;
    let name = canon_keyname(rest.get(0..gt).ok_or(KeymapCode::NameUnbound.val())?)
        .ok_or(KeymapCode::NameUnbound.val())?;
    let after = rest.get(gt + 1..).ok_or(KeymapCode::NameUnbound.val())?;
    let lb = find_token(after, b"[").ok_or(KeymapCode::NameUnbound.val())?;
    let inner = after.get(lb + 1..).ok_or(KeymapCode::NameUnbound.val())?;
    let rb = find_token(inner, b"]").ok_or(KeymapCode::NameUnbound.val())?;
    let body = trim(inner.get(0..rb).ok_or(KeymapCode::NameUnbound.val())?);
    let mut levels: [u32; LEVELS_PER_KEY] = [0; LEVELS_PER_KEY];
    let mut n = 0usize;
    let mut rest2 = body;
    loop {
        let (tok, next) = match split_once(rest2, b',') {
            Some((a, b)) => (a, Some(b)),
            None => (rest2, None),
        };
        let nm = trim(tok);
        if nm.is_empty() {
            return Err(KeymapCode::UnknownKeysym.val());
        }
        // 只有 0x/0X 前缀才按数字形解析：单字母 keysym（a/b/c/d/e/f）本身
        // 也是合法十六进制字符，若「先试数字」会把 a 解析成 10（层语义全错）。
        let numeric = nm.len() >= 2
            && nm.get(0) == Some(&b'0')
            && (nm.get(1) == Some(&b'x') || nm.get(1) == Some(&b'X'));
        let val = if numeric {
            parse_hex(nm).ok_or(KeymapCode::UnknownKeysym.val())?
        } else {
            ksym_value(nm).ok_or(KeymapCode::UnknownKeysym.val())?
        };
        if n >= LEVELS_PER_KEY {
            return Err(KeymapCode::LevelOverflow.val());
        }
        if let Some(slot) = levels.get_mut(n) {
            *slot = val;
        }
        n += 1;
        match next {
            Some(x) => rest2 = x,
            None => break,
        }
        if n > LEVELS_PER_KEY {
            return Err(KeymapCode::LevelOverflow.val());
        }
    }
    let kc = keycode_of(name).unwrap_or(0);
    Ok(KeyEntry { kc, name, levels })
}

// ── 层取用（本模块只消费选择字） ────────────────────────────────
/// 小写字母判定（Lock 只翻转字母层，不翻转符号层）。
pub fn is_lower_alpha(ks: u32) -> bool {
    if (0x61..=0x7a).contains(&ks) {
        return true;
    }
    if (0x00e0..=0x00ff).contains(&ks) {
        if ks == 0x00f7 {
            return false;
        }
        return true;
    }
    false
}

/// Control 位语义：字母→C0 控制码，符号→对应 C0，其余报层越界。
fn ctrl_of(sym: u32) -> Result<u32, u32> {
    let v = match sym {
        0x61..=0x7a => sym - 0x60,
        0x40 => 0x00,
        0x5b => 0x1b,
        0x5c => 0x1c,
        0x5d => 0x1d,
        0x5e => 0x1e,
        0x5f => 0x1f,
        _ => return Err(KeymapCode::LevelOverflow.val()),
    };
    Ok(v)
}

/// 取用某键在给定选择字下的 keysym（strict：空层报 LevelOverflow）。
pub fn resolve_level(keys: &[KeyEntry], kc: u16, sel: u8) -> Result<u32, u32> {
    let mut hit: Option<KeyEntry> = None;
    for k in keys.iter() {
        if k.kc == kc {
            hit = Some(*k);
            break;
        }
    }
    let k = hit.ok_or(KeymapCode::NameUnbound.val())?;
    let mut lvl = if (sel & SEL_ALTGR) != 0 { 2 } else { 0 };
    let mut shift = (sel & SEL_SHIFT) != 0;
    if (sel & SEL_LOCK) != 0 && (sel & SEL_ALTGR) == 0 {
        let base = k.levels.get(lvl).copied().unwrap_or(0);
        if is_lower_alpha(base) {
            shift = !shift;
        }
    }
    if shift {
        lvl += 1;
    }
    let sym = k.levels.get(lvl).copied().unwrap_or(0);
    if sym == 0 {
        return Err(KeymapCode::LevelOverflow.val());
    }
    if (sel & SEL_CTRL) != 0 {
        return ctrl_of(sym);
    }
    Ok(sym)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：十组合生成→解析→双跑→样本取用。
/// 返回 (组合数, 键位总数, 摘要异或)。
pub fn golden_run() -> Result<(usize, usize, u64), u32> {
    let mut total = 0usize;
    let mut mix = 0u64;
    let mut last: Option<ParsedLayout> = None;
    for spec in COMBO_SPECS.iter() {
        let ls = parse_spec(spec)?;
        let text = generate_keymap_text(&ls)?;
        let a = parse_keymap(text.as_bytes())?;
        let b = parse_keymap(text.as_bytes())?;
        if a.keys.len() != KEYS_PER_LAYOUT {
            return Err(KeymapCode::NotReproducible.val());
        }
        if a.digest != b.digest || a.version != b.version {
            return Err(KeymapCode::NotReproducible.val());
        }
        if a.layout != ls.layout || a.variant != ls.variant || a.opts != ls.opts {
            return Err(KeymapCode::VariantUnknown.val());
        }
        total += a.keys.len();
        mix ^= a.digest;
        last = Some(a);
    }
    let us = match last {
        Some(v) => v,
        None => return Err(KeymapCode::NotReproducible.val()),
    };
    // 样本：gb/intl 的 AC01 四层必须与表一致。
    let e = match us.keys.iter().find(|k| k.kc == 38) {
        Some(k) => *k,
        None => return Err(KeymapCode::NameUnbound.val()),
    };
    if e.levels.get(0).copied().unwrap_or(0) != 0x0061 {
        return Err(KeymapCode::NotReproducible.val());
    }
    Ok((COMBO_SPECS.len(), total, mix))
}

/// 探针 a：十组合全部可解析。
pub fn probe_seq_a() -> Result<(), u32> {
    for spec in COMBO_SPECS.iter() {
        let ls = parse_spec(spec)?;
        let text = generate_keymap_text(&ls)?;
        parse_keymap(text.as_bytes())?;
    }
    Ok(())
}

/// 探针 b：未登记版本必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let text = generate_keymap_text(&parse_spec("us")?)?;
    let bad = replace_token(text.as_bytes(), b"// version 0x0106", b"// version 0x0001");
    match parse_keymap(bad.as_slice()) {
        Err(c) if c == KeymapCode::VersionUnregistered.val() => Ok(()),
        _ => Err(KeymapCode::VersionUnregistered.val()),
    }
}

/// 探针 c：未知 keysym 必拒（de 的 TLDE 首层是 dead_circumflex）。
pub fn probe_seq_c() -> Result<(), u32> {
    let text = generate_keymap_text(&parse_spec("de")?)?;
    let bad = replace_token(text.as_bytes(), b"dead_circumflex", b"NoSuchSymbol");
    match parse_keymap(bad.as_slice()) {
        Err(c) if c == KeymapCode::UnknownKeysym.val() => Ok(()),
        _ => Err(KeymapCode::UnknownKeysym.val()),
    }
}

/// 一次性替换首个 token。
pub fn replace_token(src: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = vec![0u8; 0];
    let mut hit = false;
    let mut i = 0usize;
    while i < src.len() {
        if !hit && i + from.len() <= src.len() {
            let mut eq = true;
            let mut j = 0usize;
            while j < from.len() {
                match (src.get(i + j), from.get(j)) {
                    (Some(a), Some(b)) => {
                        if a != b {
                            eq = false;
                            break;
                        }
                    }
                    _ => {
                        eq = false;
                        break;
                    }
                }
                j += 1;
            }
            if eq {
                for b in to.iter() {
                    out.push(*b);
                }
                i += from.len();
                hit = true;
                continue;
            }
        }
        if let Some(b) = src.get(i) {
            out.push(*b);
        }
        i += 1;
    }
    out
}

/// 删除含指定 token 的首行。
pub fn drop_line(src: &[u8], needle: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = vec![0u8; 0];
    let mut pos = 0usize;
    let mut dropped = false;
    while pos < src.len() {
        let start = pos;
        let mut end = start;
        while end < src.len() && src.get(end).copied() != Some(b'\n') {
            end += 1;
        }
        let line = src.get(start..end).unwrap_or(&[]);
        let hit = !dropped && find_token(line, needle).is_some();
        if hit {
            dropped = true;
        } else {
            for b in line.iter() {
                out.push(*b);
            }
            out.push(b'\n');
        }
        pos = if end < src.len() { end + 1 } else { end };
    }
    out
}

/// 注入账：每槽 tries/caught/期望码。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    /// 各槽期望的诊断码。
    pub want: [u32; INJECT_SLOTS],
    /// 各槽判定次数。
    pub tries: [u32; INJECT_SLOTS],
    /// 各槽命中次数（返回码与期望一致才计）。
    pub caught: [u32; INJECT_SLOTS],
}

impl InjectLedger {
    /// 建账（期望码写死，与注入槽一一对应）。
    pub fn new() -> InjectLedger {
        InjectLedger {
            want: [
                KeymapCode::MissingHeader.val(),
                KeymapCode::VersionUnregistered.val(),
                KeymapCode::VersionUnregistered.val(),
                KeymapCode::UnbalancedBrace.val(),
                KeymapCode::UnbalancedBrace.val(),
                KeymapCode::UnknownSection.val(),
                KeymapCode::UnknownKeysym.val(),
                KeymapCode::NameUnbound.val(),
                KeymapCode::LevelOverflow.val(),
                KeymapCode::VariantUnknown.val(),
            ],
            tries: [0; INJECT_SLOTS],
            caught: [0; INJECT_SLOTS],
        }
    }

    /// 记账：槽位越界直接丢弃（不 panic）。
    pub fn record(&mut self, slot: usize, got: Result<ParsedLayout, u32>) {
        if slot >= INJECT_SLOTS {
            return;
        }
        let want = self.want.get(slot).copied().unwrap_or(0);
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
        }
        let ok = match got {
            Ok(_) => false,
            Err(c) => c == want,
        };
        if ok {
            if let Some(c) = self.caught.get_mut(slot) {
                *c += 1;
            }
        }
    }

    /// 全槽至少一次且全命中。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJECT_SLOTS {
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

/// 注入 10 轮 × 10 槽（LCG 打乱槽序，账按槽号归集）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let base = generate_keymap_text(&parse_spec("us")?)?;
    let mut led = InjectLedger::new();
    let mut lcg = Lcg::new(SEG_BASE);
    let mut r = 0u32;
    while r < rounds {
        // 每轮按 LCG 打乱槽序（Fisher-Yates），保证十槽各判一次不重不漏。
        let mut order: [usize; INJECT_SLOTS] = [0; INJECT_SLOTS];
        let mut i = 0usize;
        while i < INJECT_SLOTS {
            if let Some(s) = order.get_mut(i) {
                *s = i;
            }
            i += 1;
        }
        let mut j = INJECT_SLOTS;
        while j > 1 {
            let pick = lcg.below(j as u32) as usize;
            let hi = j - 1;
            if pick < INJECT_SLOTS && hi < INJECT_SLOTS {
                order.swap(hi, pick);
            }
            j -= 1;
        }
        let mut k = 0usize;
        while k < INJECT_SLOTS {
            let slot = order.get(k).copied().unwrap_or(0);
            let text: Vec<u8> = match slot {
                0 => replace_token(base.as_bytes(), b"xkb_keymap", b"keymap_off"),
                1 => drop_line(base.as_bytes(), b"// version"),
                2 => replace_token(base.as_bytes(), b"0x0106", b"0x0001"),
                3 => replace_token(base.as_bytes(), b"};\n", b"};\n{\n"),
                4 => {
                    let mut v: Vec<u8> = vec![b'}', b'\n'];
                    for b in base.as_bytes().iter() {
                        v.push(*b);
                    }
                    v
                }
                5 => replace_token(
                    base.as_bytes(),
                    b"xkb_symbols",
                    b"xkb_geometry \"vx\" { };\n  xkb_symbols",
                ),
                6 => replace_token(base.as_bytes(), b"grave", b"NoSuchSymbol"),
                7 => replace_token(base.as_bytes(), b"<TLDE>", b"<ZZ99>"),
                8 => replace_token(base.as_bytes(), b"NoSymbol", b"backslash, grave"),
                _ => replace_token(base.as_bytes(), b"xkb_symbols \"basic\"", b"xkb_symbols \"intl\""),
            };
            led.record(slot, parse_keymap(text.as_slice()));
            k += 1;
        }
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(KeymapCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫：全部码落在 [SEG_BASE, SEG_BASE+0x20)。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < KeymapCode::ALL.len() {
        if let Some(c) = KeymapCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x20 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 九条判据（8~10 条；动态明细恰好 8 条，不越 CheckSet 动态侧表）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // J1-1 十组合解析一致。
    let g = golden_run();
    let combos_ok = match &g {
        Ok((n, k, _)) => *n == COMBO_ROUNDS && *k == COMBO_ROUNDS * KEYS_PER_LAYOUT,
        Err(_) => false,
    };
    v.push((
        "F48821-J1-三层十组合解析一致",
        combos_ok,
        format!("{:?}", g.as_ref().map(|(n, k, _)| (*n, *k))),
    ));

    // J1-2 variant 叠加必须改变摘要（同 layout 不同 variant 不得同摘要）。
    let mut vari_ok = true;
    let mut vari_note = String::new();
    for (lay, a, b) in [
        ("us", "basic", "intl"),
        ("de", "basic", "nodeadkeys"),
        ("fr", "basic", "oss"),
    ] {
        let da = dig_of(lay, a);
        let db = dig_of(lay, b);
        match (da, db) {
            (Ok(x), Ok(y)) => {
                if x == y {
                    vari_ok = false;
                }
                vari_note.push_str(a);
                vari_note.push('|');
                vari_note.push_str(b);
                vari_note.push(' ');
            }
            _ => vari_ok = false,
        }
    }
    v.push((
        "F48821-J1-variant叠加生效",
        vari_ok,
        vari_note,
    ));

    // J1-3 options 位图：三重 option 组合位必须全置位且互不串。
    let (opt_ok, opt_bits) = match parse_spec(
        "de+nodeadkeys+lv3:ralt_switch+grp:alt_shift_toggle+caps:escape",
    ) {
        Ok(ls) => (
            ls.opts == 0x07
                && ls.layout == "de"
                && ls.variant == "nodeadkeys"
                && parse_spec("us+bogus:opt").is_err()
                && parse_spec("de+nodeadkeys+lv3:ralt_switch+lv3:ralt_switch").is_err(),
            ls.opts,
        ),
        Err(_) => (false, 0u32),
    };
    v.push((
        "F48821-J1-options位图",
        opt_ok,
        format!("bits=0x{:02x}", opt_bits),
    ));

    // J1-4 闸门次序：单故障文本必须返回「该闸」的码，不得串码。
    let led = inject_rounds(1);
    let gate_ok = match &led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "F48821-J1-闸门次序七道",
        gate_ok,
        match &led {
            Ok(l) => format!("caught={:?}", l.caught),
            Err(c) => format!("err=0x{:X}", c),
        },
    ));

    // J1-5 十布局零漂移（双跑摘要逐组合一致）。
    let mut drift_ok = true;
    for spec in COMBO_SPECS.iter() {
        match double_digest(spec) {
            Ok((x, y)) => {
                if x != y || x == 0 {
                    drift_ok = false;
                }
            }
            Err(_) => drift_ok = false,
        }
    }
    v.push((
        "F48822-J1-十布局零漂移",
        drift_ok,
        format!("combos={}", COMBO_SPECS.len()),
    ));

    // J1-6 版本登记在册。
    let book = VersionBook::from_ledger();
    let ver_ok = book.query(XKB_REF_REV).is_some()
        && version_entry(XKB_REF_REV).map(|e| e.0) == Some(XKB_REF_NAME)
        && match book.query(XKB_REF_REV) {
            Some(e) => book.verify(XKB_REF_REV, e.2).is_ok(),
            None => false,
        };
    v.push((
        "F48822-J1-版本登记在册",
        ver_ok,
        format!("rev=0x{:04x}", XKB_REF_REV),
    ));

    // J1-7 keysym 名表冻结：表内每条都可反向查到。
    let mut ksym_ok = true;
    for (n, val) in KSYM_NAMES.iter() {
        if ksym_name(*val) == "" || ksym_value(n.as_bytes()) != Some(*val) {
            ksym_ok = false;
        }
    }
    for (n, val) in KSYM_NAMES_2.iter() {
        if ksym_name(*val) == "" || ksym_value(n.as_bytes()) != Some(*val) {
            ksym_ok = false;
        }
    }
    v.push(("F48822-J1-keysym名表冻结", ksym_ok, String::new()));

    // J1-8 层取用严格：US basic 的第三层必须报 LevelOverflow，不得静默 0。
    let strict_ok = match parse_spec("us").and_then(|ls| generate_keymap_text(&ls)) {
        Ok(t) => match parse_keymap(t.as_bytes()) {
            Ok(p) => {
                let base = resolve_level(&p.keys, 24, 0);
                let shift = resolve_level(&p.keys, 24, SEL_SHIFT);
                let alt = resolve_level(&p.keys, 24, SEL_ALTGR);
                let ctrl = resolve_level(&p.keys, 24, SEL_CTRL);
                base == Ok(0x71)
                    && shift == Ok(0x51)
                    && alt.is_err()
                    && ctrl == Ok(0x11)
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    v.push(("F48821-J1-层取用严格", strict_ok, String::new()));

    // J2 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "F48821-J2-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));

    // J3 段位闭集。
    v.push(("F48821-J3-段位闭集", segment_guard(), String::new()));

    v
}

/// 单配置摘要。
fn dig_of(layout: &str, variant: &str) -> Result<u64, u32> {
    let ls = LayerSpec { layout: layout_static(layout)?, variant: variant_static(variant)?, opts: 0 };
    let t = generate_keymap_text(&ls)?;
    Ok(parse_keymap(t.as_bytes())?.digest)
}

/// 两跑摘要。
fn double_digest(spec: &str) -> Result<(u64, u64), u32> {
    let ls = parse_spec(spec)?;
    let t = generate_keymap_text(&ls)?;
    let a = parse_keymap(t.as_bytes())?;
    let b = parse_keymap(t.as_bytes())?;
    Ok((a.digest, b.digest))
}

/// 静态 layout 名。
fn layout_static(n: &str) -> Result<&'static str, u32> {
    for l in LAYOUTS.iter() {
        if *l == n {
            return Ok(l);
        }
    }
    Err(KeymapCode::VariantUnknown.val())
}

/// 静态 variant 名。
fn variant_static(n: &str) -> Result<&'static str, u32> {
    for (_, v) in VARIANTS.iter() {
        if *v == n {
            return Ok(v);
        }
    }
    Err(KeymapCode::VariantUnknown.val())
}

/// 本模块自检入口（名与文件名前 8 字符一致）。
pub fn run_unx48821_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48821 keymap 三层配置与 XKB 解析");
    for (n, p, d) in extra_checks() {
        if d.is_empty() {
            s.add(n, p, "");
        } else {
            s.add_dyn(n, p, d.as_str());
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(spec: &str) -> String {
        let ls = parse_spec(spec).expect("spec");
        generate_keymap_text(&ls).expect("gen")
    }

    #[test]
    fn t_header_only_rejected() {
        // 只有头没有 symbols 段：variant 层无法核对（VariantUnknown），不得因
        // 「头在、括号平衡、版本在册」就把空表当合法 keymap 放行。
        let src = b"xkb_keymap \"us/basic\" {\n// version 0x0106\n};\n";
        assert_eq!(parse_keymap(src).err(), Some(KeymapCode::VariantUnknown.val()));
        // 头 + symbols 段但零键位：名闸报 NameUnbound。
        let empty =
            b"xkb_keymap \"us/basic\" {\n// version 0x0106\n  xkb_symbols \"basic\" {\n  };\n};\n";
        assert_eq!(parse_keymap(empty).err(), Some(KeymapCode::NameUnbound.val()));
    }

    #[test]
    fn t_missing_header_rejected() {
        let src = text_of("us").replace("xkb_keymap", "keymap_off");
        assert_eq!(
            parse_keymap(src.as_bytes()).err(),
            Some(KeymapCode::MissingHeader.val())
        );
    }

    #[test]
    fn t_version_gate() {
        let src = text_of("us").replace("0x0106", "0x0007");
        assert_eq!(
            parse_keymap(src.as_bytes()).err(),
            Some(KeymapCode::VersionUnregistered.val())
        );
        // 缺版本行同样判死（不得默认按当前台账）。
        let nv = drop_line(text_of("us").as_bytes(), b"// version");
        assert_eq!(
            parse_keymap(nv.as_slice()).err(),
            Some(KeymapCode::VersionUnregistered.val())
        );
    }

    #[test]
    fn t_unbalanced_brace() {
        let mut v: Vec<u8> = Vec::new();
        v.push(b'}');
        v.push(b'\n');
        for b in text_of("us").as_bytes() {
            v.push(*b);
        }
        assert_eq!(
            parse_keymap(v.as_slice()).err(),
            Some(KeymapCode::UnbalancedBrace.val())
        );
    }

    #[test]
    fn t_unknown_section_and_keysym() {
        let sec = text_of("us").replace("xkb_symbols", "xkb_geometry \"vx\" { };\n  xkb_symbols");
        assert_eq!(
            parse_keymap(sec.as_bytes()).err(),
            Some(KeymapCode::UnknownSection.val())
        );
        let ks = text_of("us").replace("grave", "NoSuchSymbol");
        assert_eq!(
            parse_keymap(ks.as_bytes()).err(),
            Some(KeymapCode::UnknownKeysym.val())
        );
    }

    #[test]
    fn t_level_overflow() {
        let over = text_of("us").replace("NoSymbol", "backslash, grave");
        assert_eq!(
            parse_keymap(over.as_bytes()).err(),
            Some(KeymapCode::LevelOverflow.val())
        );
    }

    #[test]
    fn t_spec_three_layers() {
        let ls = parse_spec("de+nodeadkeys+lv3:ralt_switch").expect("spec");
        assert_eq!(ls.layout, "de");
        assert_eq!(ls.variant, "nodeadkeys");
        assert_eq!(ls.opts, 0x01);
        assert_eq!(parse_spec("us").map(|s| s.variant), Ok("basic"));
        assert!(parse_spec("xx").is_err());
        assert!(parse_spec("us+nope:opt").is_err());
        assert!(parse_spec("us+interstellar").is_err());
    }

    #[test]
    fn t_ten_combos_stable() {
        let g = golden_run().expect("golden");
        assert_eq!(g.0, COMBO_ROUNDS);
        assert_eq!(g.1, COMBO_ROUNDS * KEYS_PER_LAYOUT);
        assert!(g.2 != 0);
        assert!(repro_check().is_ok());
    }

    #[test]
    fn t_variant_changes_digest() {
        let a = dig_of("de", "basic").expect("basic");
        let b = dig_of("de", "nodeadkeys").expect("nodeadkeys");
        assert!(a != b);
        // nodeadkeys 的 TLDE 首层从死键换成 ASCII 抑扬符。
        let ls = parse_spec("de+nodeadkeys").expect("spec");
        let p = parse_keymap(text_of("de+nodeadkeys").as_bytes()).expect("parse");
        assert_eq!(ls.variant, p.variant);
        let e = p.keys.iter().find(|k| k.kc == 49).expect("tlde");
        assert_eq!(e.levels[0], 0x005e);
    }

    #[test]
    fn t_options_carried_into_parse() {
        let p = parse_keymap(text_of("fr+oss+grp:alt_shift_toggle").as_bytes()).expect("parse");
        assert_eq!(p.opts, 0x02);
        assert_eq!(p.variant, "oss");
        // 同 variant 不同 option → 摘要必须不同（第三层不得被解析吞掉）。
        let q = parse_keymap(text_of("fr+oss").as_bytes()).expect("parse");
        assert!(p.digest != q.digest);
    }

    #[test]
    fn t_resolve_caps_and_ctrl() {
        let p = parse_keymap(text_of("us").as_bytes()).expect("parse");
        assert_eq!(resolve_level(&p.keys, 38, 0), Ok(0x61));
        assert_eq!(resolve_level(&p.keys, 38, SEL_SHIFT), Ok(0x41));
        assert_eq!(resolve_level(&p.keys, 38, SEL_LOCK), Ok(0x41));
        assert_eq!(resolve_level(&p.keys, 38, SEL_LOCK | SEL_SHIFT), Ok(0x61));
        assert_eq!(resolve_level(&p.keys, 38, SEL_CTRL), Ok(0x01));
        assert_eq!(resolve_level(&p.keys, 49, SEL_CTRL), Err(KeymapCode::LevelOverflow.val()));
        assert_eq!(resolve_level(&p.keys, 94, SEL_SHIFT), Err(KeymapCode::LevelOverflow.val()));
        assert_eq!(resolve_level(&p.keys, 999, 0), Err(KeymapCode::NameUnbound.val()));
    }

    #[test]
    fn t_inject_ten_slots_caught() {
        let led = inject_rounds(1).expect("inject");
        assert!(led.all_caught());
        assert_eq!(led.tries.iter().filter(|t| **t > 0).count(), INJECT_SLOTS);
    }

    #[test]
    fn t_segment_window() {
        assert!(segment_guard());
        for c in KeymapCode::ALL.iter() {
            assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x20);
            assert!(!c.detail().is_empty());
        }
        assert_eq!(detail(SEG_BASE + 11).is_empty(), false);
        assert!(detail(0x7777).contains("未知"));
    }

    #[test]
    fn t_extra_checks_all_green() {
        let v = extra_checks();
        assert!(v.len() >= 8 && v.len() <= 10);
        for (n, p, _) in v {
            assert!(p, "红项 {}", n);
            assert!(n.starts_with("F488"));
        }
        assert!(run_unx48821_checks().all_passed());
    }
}
