//! F244 全局快捷键注册表与冲突审计 · 判据实装。
//!
//! **判据锚**：主册 F244「全局快捷键注册表与冲突审计」。
//!
//! **验收标准第一句（任务包原文）**：注册表条目完整性审计（系统快捷键
//! 全量入表）。
//!
//! **判据（主册原文摘录）**：所有系统级快捷键集中一处注册表（设置中心
//! 可查可改可禁用），注册时冲突即检——新快捷键与已有冲突时明确列出占用
//! 方让用户选（先到先得+手动改），禁止静默覆盖；第三方应用申请全局热键
//! 走同一注册表（vxapp 清单声明），冲突同样可见。注册表带「恢复默认」
//! 一键。验收：冲突检测用例（故意注册冲突 3 例）；用户改键持久化；
//! 第三方声明接入验证。
//!
//! **设计要点**：
//! - 键域：修饰键 4 位（Ctrl/Alt/Shift/Win）+ Windows VK 码域 u16
//!   （锚定 VK 原值，`0x41='A'`、`0x70=F1` 等）——键位编码一处一事实；
//! - 冲突即检哈希表：`(修饰键,键)` 32 位码 → 乘法散列 → 96 槽线性
//!   探测，注册/改键 O(1) 冲突判定；删除用**后退移位法**（Knuth 6.4
//!   Algorithm R）保持无墓碑探测链——派发面永不因删除产生盲区；
//! - 冲突语义：先到先得 + 显性拒绝（`Occupied` 携带占用方身份：
//!   系统条目给语义名、第三方给应用 ID），**无静默覆盖**；改键同样
//!   过冲突门（占用方照样列出）；
//! - 系统预置表 46 条全量入表（复制/粘贴/窗口切换/Win 系/F 键系，
//!   每条带说明与可禁用位），完整性审计逐条核对在册+回出厂态；
//! - 改键持久化：只导出「被用户改过/禁用」的差异条目（uid + 键位 +
//!   标志位，9 字节/条），导入时**冲突复核**（目标键位已被他人占用
//!   的条目跳过并如实计数）——round-trip 语义完整；
//! - 第三方接入：vxapp 清单声明行 `hotkey=ctrl+alt+f9|说明文本`，
//!   解析器逐 token 校验（未知 token/双键名/重复修饰键显性拒绝），
//!   注册走同一冲突门——第三方冲突同样可见；
//! - 恢复默认一键：全表回出厂键位+全部启用+清改键标记，版本推进、
//!   留痕；变更留痕环 64 条（改键/启停/恢复/注册/注销全留痕）；
//! - 零堆热路径：派发/冲突检测/审计全定长数组；变更环与导出缓冲
//!   由调用方给缓冲；时间一律注入（毫秒戳）。
//!
//! **依赖锚点**：F220（Ctrl+Shift+V 纯文本粘贴——预置表一条）、
//! F249（热角同源动作 Win+A/Win+N——预置表两条）、F081（任务视图
//! Win+Tab）、F235（虚拟桌面 Win+Ctrl+方向）。

use crate::checks::CheckSet;
use crate::star::sbase::RingLog;

use alloc::vec::Vec;

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
        const NAMES: [(u8, &str); 4] =
            [(MOD_CTRL, "Ctrl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift"), (MOD_WIN, "Win")];
        let mut k = 0;
        for (m, name) in NAMES.iter() {
            if self.mods & m != 0 && k < out.len() {
                out[k] = name;
                k += 1;
            }
        }
        k
    }
}

/// VK 码 → 键名（设置中心显示面；覆盖本域用到的全部码段）。
pub fn key_name(key: u16) -> Option<&'static str> {
    match key {
        VK_BACK => Some("Back"),
        VK_TAB => Some("Tab"),
        VK_RETURN => Some("Enter"),
        VK_ESCAPE => Some("Esc"),
        VK_SPACE => Some("Space"),
        VK_PRIOR => Some("PgUp"),
        VK_NEXT => Some("PgDn"),
        VK_END => Some("End"),
        VK_HOME => Some("Home"),
        VK_LEFT => Some("Left"),
        VK_UP => Some("Up"),
        VK_RIGHT => Some("Right"),
        VK_DOWN => Some("Down"),
        VK_SNAPSHOT => Some("PrtSc"),
        VK_INSERT => Some("Ins"),
        VK_DELETE => Some("Del"),
        VK_LWIN => Some("Win"),
        0x30..=0x39 => Some(match key {
            0x30 => "0",
            0x31 => "1",
            0x32 => "2",
            0x33 => "3",
            0x34 => "4",
            0x35 => "5",
            0x36 => "6",
            0x37 => "7",
            0x38 => "8",
            _ => "9",
        }),
        0x41..=0x5A => Some(match key {
            0x41 => "A",
            0x42 => "B",
            0x43 => "C",
            0x44 => "D",
            0x45 => "E",
            0x46 => "F",
            0x47 => "G",
            0x48 => "H",
            0x49 => "I",
            0x4A => "J",
            0x4B => "K",
            0x4C => "L",
            0x4D => "M",
            0x4E => "N",
            0x4F => "O",
            0x50 => "P",
            0x51 => "Q",
            0x52 => "R",
            0x53 => "S",
            0x54 => "T",
            0x55 => "U",
            0x56 => "V",
            0x57 => "W",
            0x58 => "X",
            0x59 => "Y",
            _ => "Z",
        }),
        0x70..=0x7B => Some(match key {
            0x70 => "F1",
            0x71 => "F2",
            0x72 => "F3",
            0x73 => "F4",
            0x74 => "F5",
            0x75 => "F6",
            0x76 => "F7",
            0x77 => "F8",
            0x78 => "F9",
            0x79 => "F10",
            0x7A => "F11",
            _ => "F12",
        }),
        0xBA => Some(";"),
        0xBB => Some("="),
        0xBC => Some(","),
        0xBD => Some("-"),
        0xBE => Some("."),
        0xBF => Some("/"),
        0xC0 => Some("`"),
        0xDB => Some("["),
        0xDC => Some("\\"),
        0xDD => Some("]"),
        0xDE => Some("'"),
        _ => None,
    }
}

/// 键名 → VK 码（清单声明解析的键名面；大小写不敏感）。
pub fn name_to_key(tok: &[u8]) -> Option<u16> {
    if tok.is_empty() {
        return None;
    }
    let mut buf = [0u8; 8];
    if tok.len() > buf.len() {
        return None;
    }
    for (i, &b) in tok.iter().enumerate() {
        buf[i] = ascii_lower(b);
    }
    let t = &buf[..tok.len()];
    match t {
        b"back" => Some(VK_BACK),
        b"tab" => Some(VK_TAB),
        b"enter" => Some(VK_RETURN),
        b"esc" => Some(VK_ESCAPE),
        b"space" => Some(VK_SPACE),
        b"pgup" => Some(VK_PRIOR),
        b"pgdn" => Some(VK_NEXT),
        b"end" => Some(VK_END),
        b"home" => Some(VK_HOME),
        b"left" => Some(VK_LEFT),
        b"up" => Some(VK_UP),
        b"right" => Some(VK_RIGHT),
        b"down" => Some(VK_DOWN),
        b"prtsc" => Some(VK_SNAPSHOT),
        b"ins" => Some(VK_INSERT),
        b"del" => Some(VK_DELETE),
        b"win" => Some(VK_LWIN),
        b"f1" => Some(VK_F1),
        b"f2" => Some(VK_F1 + 1),
        b"f3" => Some(VK_F1 + 2),
        b"f4" => Some(VK_F1 + 3),
        b"f5" => Some(VK_F1 + 4),
        b"f6" => Some(VK_F1 + 5),
        b"f7" => Some(VK_F1 + 6),
        b"f8" => Some(VK_F1 + 7),
        b"f9" => Some(VK_F1 + 8),
        b"f10" => Some(VK_F1 + 9),
        b"f11" => Some(VK_F1 + 10),
        b"f12" => Some(VK_F1 + 11),
        _ => {
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
    }
}

/// 修饰键 token → 位（清单声明解析用；大小写不敏感）。
fn mod_from_token(tok: &[u8]) -> Option<u8> {
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
pub fn combo_str(c: Combo, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    let mut put = |s: &str, out: &mut [u8], n: &mut usize| {
        for &b in s.as_bytes() {
            if *n < out.len() {
                out[*n] = b;
                *n += 1;
            }
        }
    };
    let mut wrote_mod = false;
    if c.mods & MOD_CTRL != 0 {
        if wrote_mod {
            put("+", out, &mut n);
        }
        put("Ctrl", out, &mut n);
        wrote_mod = true;
    }
    if c.mods & MOD_ALT != 0 {
        if wrote_mod {
            put("+", out, &mut n);
        }
        put("Alt", out, &mut n);
        wrote_mod = true;
    }
    if c.mods & MOD_SHIFT != 0 {
        if wrote_mod {
            put("+", out, &mut n);
        }
        put("Shift", out, &mut n);
        wrote_mod = true;
    }
    if c.mods & MOD_WIN != 0 {
        if wrote_mod {
            put("+", out, &mut n);
        }
        put("Win", out, &mut n);
        wrote_mod = true;
    }
    match key_name(c.key) {
        Some(kn) => {
            if wrote_mod {
                put("+", out, &mut n);
            }
            put(kn, out, &mut n);
        }
        None => put("?", out, &mut n),
    }
    n
}

// ---------------------------------------------------------------------------
// 条目与身份
// ---------------------------------------------------------------------------

/// 第三方应用身份（vxapp，16 字节定长）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppId {
    len: u8,
    bytes: [u8; 16],
}

impl AppId {
    pub const fn none() -> AppId {
        AppId { len: 0, bytes: [0u8; 16] }
    }

    pub fn from_bytes(src: &[u8]) -> AppId {
        let mut out = AppId::none();
        let n = src.len().min(16);
        out.bytes[..n].copy_from_slice(&src[..n]);
        out.len = n as u8;
        out
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// 说明文案缓存（设置中心「可查」面，48 字节定长）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DescBuf {
    len: u8,
    bytes: [u8; 48],
}

impl DescBuf {
    pub fn from_bytes(src: &[u8]) -> DescBuf {
        let mut out = DescBuf { len: 0, bytes: [0u8; 48] };
        let n = src.len().min(48);
        out.bytes[..n].copy_from_slice(&src[..n]);
        out.len = n as u8;
        out
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// 条目来源：系统预置 / 第三方声明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    System,
    App(AppId),
}

impl Source {
    pub fn is_system(&self) -> bool {
        matches!(self, Source::System)
    }

    pub fn app(&self) -> Option<AppId> {
        match self {
            Source::System => None,
            Source::App(a) => Some(*a),
        }
    }
}

/// 注册表条目。
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    /// 系统条目语义名（如 "sys.copy"）；第三方为空串。
    pub label: &'static str,
    /// 第三方声明序号（同应用多条声明去重 uid 用；系统条目恒 0）。
    pub seq: u8,
    pub desc: DescBuf,
    /// 当前键位（改键后 ≠ def）。
    pub combo: Combo,
    /// 出厂键位（恢复默认锚）。
    pub def: Combo,
    /// 可禁用位。
    pub enabled: bool,
    /// 用户改过键（持久化导出标记）。
    pub overridden: bool,
    pub source: Source,
}

impl Entry {
    /// 稳定身份：系统 = 语义名 FNV-1a；第三方 = 应用 ID FNV-1a 循环移位
    /// 异或声明序号（同应用多条声明互不冲突）。
    pub fn uid(&self) -> u32 {
        if self.source.is_system() {
            fnv1a(self.label.as_bytes())
        } else {
            fnv1a(self.source.app().unwrap_or(AppId::none()).as_bytes()).rotate_left(8)
                ^ (self.seq as u32)
        }
    }

    pub fn is_system(&self) -> bool {
        self.source.is_system()
    }

    /// 第三方应用 ID（系统条目返回空 ID）。
    pub fn app_id(&self) -> AppId {
        self.source.app().unwrap_or(AppId::none())
    }
}

/// 冲突占用方身份（判据「明确列出占用方」的承载）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occupant {
    /// 系统条目语义名（第三方占用时为空串）。
    pub label: &'static str,
    /// 第三方应用 ID（系统占用时为空 ID）。
    pub app: AppId,
}

impl Occupant {
    pub fn is_system(&self) -> bool {
        !self.label.is_empty()
    }
}

/// 注册结论（无静默：四种结局都可观测）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegOutcome {
    Registered,
    /// 表满显性拒绝。
    Full,
    /// 键位非法（无键/纯修饰键）。
    Malformed,
    /// 同 uid 已在册（幂等注册面由调用方处理）。
    Duplicate,
    /// 冲突显性拒绝，携带占用方身份（先到先得）。
    Occupied(Occupant),
}

/// 改键结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebindOutcome {
    Done,
    NoSuchId,
    Malformed,
    Occupied(Occupant),
}

/// 变更种类（审计留痕面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Rebind,
    Enable,
    Disable,
    ResetOne,
    ResetAll,
    Register,
    Unregister,
}

/// 一条变更留痕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub ts: u64,
    pub kind: ChangeKind,
    pub uid: u32,
    pub old: Combo,
    pub new: Combo,
}

/// 注册表统计（设置中心概览面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegStats {
    pub total: usize,
    pub system: usize,
    pub app: usize,
    pub disabled: usize,
    pub overridden: usize,
}

// ---------------------------------------------------------------------------
// 序列键（chord）与派发留痕
// ---------------------------------------------------------------------------

/// 序列键表容量（两段式键的独立承载面，单键表之外）。
pub const CHORD_CAP: usize = 16;

/// 序列键第二段等待窗（ms）——第一段命中后仅在此窗内认第二段。
pub const CHORD_TIMEOUT_MS: u64 = 800;

/// 派发留痕环容量（运行面命中记录）。
pub const DISPATCH_CAP: usize = 32;

/// 序列键语义文档（判据「两段式序列键」的设置页说明锚）。
pub const CHORD_DOC: &str = "序列键：第一段按下后不立即触发，进入 800ms 等待窗；窗内按下第二段才执行。第一段键位不与任何单键冲突——冲突在注册时显性拒绝。";

/// 键位占用提示文档（设置页实时占位提示的文案锚）。
pub const CONFLICT_HINT_DOC: &str = "此键位已被占用：注册与改键一律过冲突门，先到先得；占用方身份（系统语义名或第三方应用）会明确列出。";

/// 一条序列键登记（两段均为合法单键位；段间冲突注册时拒绝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChordRec {
    /// 稳定语义名（如 "app.emacs.kill-region"）。
    pub label: &'static str,
    /// 第一段（命中即进入等待窗，不触发动作）。
    pub first: Combo,
    /// 第二段（等待窗内命中才触发）。
    pub second: Combo,
    /// 登记时刻（ms，注入式）。
    pub ts: u64,
    /// 段位被改过（差异面导出标记——与单键条目的 overridden 同纪律）。
    pub overridden: bool,
}

/// 序列键单步判定结论（运行面逐键调用 [`HotkeyReg::chord_key`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChordStep {
    /// 非序列键键位——调用方继续走普通单键派发。
    Idle,
    /// 第一段命中：进入等待窗，本击不派发任何动作。
    Armed,
    /// 第二段命中（等待窗内）：序列触发，返回语义名。
    Fired { label: &'static str },
}

/// 一次派发留痕（审计面：谁在何时被哪个键位命中）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispatchRec {
    pub ts: u64,
    pub uid: u32,
    pub combo: Combo,
    /// 命中条目当时是否启用（禁用条目留痕但不触发）。
    pub active: bool,
}

/// 预置条目分组（设置中心分类树的数据面；下标入 [`CATEGORY_NAMES`]）。
pub const CATEGORY_NAMES: [&str; 4] = ["edit", "win", "sys", "desktop"];

/// 46 条预置的分组归属（与 [`PRESETS`] 一一对应——按主册功能域划分）。
pub const PRESET_CAT: [u8; 46] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // copy..print：编辑域
    1, 1, 1, 2, 1, 1, 1, 2, 2, 3, // close-tab..show-desktop
    2, 2, 3, 1, 1, 1, 1, 3, 3, 3, // lock..min-all
    3, 2, 2, 2, 2, 2, 2, 0, 0, 2, // clip-hist..find-next
    2, 1, 2, 1, 2, 1, // menubar..cycle-pane
];

// ---------------------------------------------------------------------------
// 哈希与身份
// ---------------------------------------------------------------------------

/// FNV-1a 32 位（uid 稳定身份；与持久化导出的 uid 域一致）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 系统条目 uid 推导（调用方免持 Entry 也能定位）。
pub fn uid_of_label(label: &str) -> u32 {
    fnv1a(label.as_bytes())
}

/// 序列键 uid（语义名 FNV-1a——差异面持久化的稳定身份锚）。
pub fn chord_uid(c: &ChordRec) -> u32 {
    fnv1a(c.label.as_bytes())
}

// ---------------------------------------------------------------------------
// 注册表本体
// ---------------------------------------------------------------------------

/// 注册表容量——46 条预置 + 第三方余量（判据「全系统集中一处」）。
pub const HK_CAP: usize = 96;

/// 变更留痕环容量。
pub const CHANGE_CAP: usize = 64;

/// 冲突即检哈希：32 位码 × 黄金比例乘法散列 → 槽位。
fn slot_of(code: u32) -> usize {
    (code.wrapping_mul(0x9E37_79B1) % HK_CAP as u32) as usize
}

/// 全局快捷键注册表。
pub struct HotkeyReg {
    slots: [Option<Entry>; HK_CAP],
    pub count: usize,
    pub version: u32,
    changes: RingLog<Change, CHANGE_CAP>,
    /// 序列键表（与单键表独立；段位冲突注册时显性拒绝）。
    chords: [Option<ChordRec>; CHORD_CAP],
    /// 序列键等待窗状态：(表下标, 武装时刻)。None = 无武装。
    chord_armed: Option<(usize, u64)>,
    /// 派发留痕环（运行面命中记录）。
    dlog: RingLog<DispatchRec, DISPATCH_CAP>,
    /// 槽位级派发计数（热度画像；uid → find_uid_idx → hits[idx]）。
    hits: [u32; HK_CAP],
    /// 本会话安装的预置条目数（完整性审计面）。
    pub preset_installed: u32,
}

impl HotkeyReg {
    pub fn new() -> HotkeyReg {
        HotkeyReg {
            slots: [const { None }; HK_CAP],
            count: 0,
            version: 0,
            changes: RingLog::new(),
            chords: [const { None }; CHORD_CAP],
            chord_armed: None,
            dlog: RingLog::new(),
            hits: [0; HK_CAP],
            preset_installed: 0,
        }
    }

    // -- 表内核 ------------------------------------------------------------

    /// 线性探测：键位码 → 槽下标。删除走后退移位（无墓碑），探测链
    /// 不断裂，`None` 即可判终止。
    fn probe(&self, code: u32) -> Option<usize> {
        let start = slot_of(code);
        for k in 0..HK_CAP {
            let idx = (start + k) % HK_CAP;
            match self.slots[idx] {
                None => return None,
                Some(e) => {
                    if e.combo.code() == code {
                        return Some(idx);
                    }
                }
            }
        }
        None
    }

    /// 后退移位删除（Knuth TAOCP 6.4 Algorithm R，单调 j 版本）：
    /// 先腾空 `start`（保证环上存在空槽 → 必然终止），随后 j 单调前进，
    /// 「家址不在 (i, j] 环形区间」的条目前移填洞——线性探测链保持完整，
    /// 删除后全部条目仍可按探测规则命中（单测有专项验证）。
    /// 终止双保险：遇空槽即终；j 环绕一整圈回到洞位 i 也终（表曾被
    /// 100% 填满时首搬移会填掉唯一的空槽，靠回绕终止兜底——v2 批修复
    /// 的死循环缺陷，修复前 fuzz 满表删除会永久空转）。
    fn remove_at(&mut self, start: usize) {
        self.slots[start] = None;
        let mut i = start;
        let mut j = start;
        loop {
            j = (j + 1) % HK_CAP;
            if j == i {
                // 整表扫过一圈回到洞位：洞即终点（满表删除路径）。
                self.slots[i] = None;
                return;
            }
            match self.slots[j] {
                None => {
                    // 终止：清掉最后一个洞（物理残留来自最近一次搬移）。
                    self.slots[i] = None;
                    return;
                }
                Some(e) => {
                    let h = slot_of(e.combo.code());
                    let stays = if i < j {
                        h > i && h <= j
                    } else {
                        h > i || h <= j
                    };
                    if !stays {
                        self.slots[i] = Some(e);
                        i = j; // 洞前移（table[j] 物理残留由后续搬移覆盖或终局清洞）
                    }
                }
            }
        }
    }

    fn find_uid_idx(&self, uid: u32) -> Option<usize> {
        self.slots.iter().position(|s| matches!(s, Some(e) if e.uid() == uid))
    }

    fn push_change(&mut self, c: Change) {
        self.changes.push(c);
    }

    /// 变更留痕（新→旧，最多 64 条）。
    pub fn change_log(&self) -> [Option<Change>; CHANGE_CAP] {
        let mut out = [None; CHANGE_CAP];
        for (k, ev) in self.changes.newest_first().iter().enumerate() {
            out[k] = Some(*ev);
        }
        out
    }

    // -- 注册 / 冲突即检 ----------------------------------------------------

    /// 注册一条系统条目（冲突即检：占用即显性拒绝并列占用方）。
    pub fn register_system(&mut self, label: &'static str, combo: Combo, desc: &'static str, ts: u64) -> RegOutcome {
        let uid = uid_of_label(label);
        if self.find_uid_idx(uid).is_some() {
            return RegOutcome::Duplicate;
        }
        let e = Entry {
            label,
            seq: 0,
            desc: DescBuf::from_bytes(desc.as_bytes()),
            combo,
            def: combo,
            enabled: true,
            overridden: false,
            source: Source::System,
        };
        self.insert_entry(e, ts)
    }

    /// 注册一条第三方声明（与系统同门：同一冲突即检、同样可见）。
    pub fn register_app_decl(&mut self, app: AppId, decl: &ParsedDecl, seq: u8, ts: u64) -> RegOutcome {
        let e = Entry {
            label: "",
            seq,
            desc: decl.desc,
            combo: Combo::new(decl.mods, decl.key),
            def: Combo::new(decl.mods, decl.key),
            enabled: true,
            overridden: false,
            source: Source::App(app),
        };
        self.insert_entry(e, ts)
    }

    fn insert_entry(&mut self, e: Entry, ts: u64) -> RegOutcome {
        if !e.combo.sane() {
            return RegOutcome::Malformed;
        }
        if let Some(i) = self.probe(e.combo.code()) {
            let occ = self.slots[i].expect("hotkeyreg invariant");
            return RegOutcome::Occupied(Occupant { label: occ.label, app: occ.app_id() });
        }
        // 从家址起探测第一个空槽落位（线性探测插入语义——v2 批修复：
        // 修复前误用「物理顺序第一个空槽」，家址探测链会被无关空槽截断，
        // 注册成功却派发不到）。
        let start = slot_of(e.combo.code());
        for k in 0..HK_CAP {
            let idx = (start + k) % HK_CAP;
            if self.slots[idx].is_none() {
                self.slots[idx] = Some(e);
                self.count += 1;
                self.version += 1;
                self.push_change(Change { ts, kind: ChangeKind::Register, uid: e.uid(), old: Combo::none(), new: e.combo });
                return RegOutcome::Registered;
            }
        }
        RegOutcome::Full
    }

    /// 注销（第三方应用卸载时清理声明面）。
    pub fn unregister(&mut self, uid: u32, ts: u64) -> bool {
        if let Some(i) = self.find_uid_idx(uid) {
            let e = self.slots[i].expect("hotkeyreg invariant");
            self.remove_at(i);
            self.count -= 1;
            self.version += 1;
            self.push_change(Change { ts, kind: ChangeKind::Unregister, uid, old: e.combo, new: Combo::none() });
            true
        } else {
            false
        }
    }

    // -- 改键 / 禁用 / 恢复 --------------------------------------------------

    /// 用户改键（判据「可改」）：过冲突门 → 更新键位 → 置改键标记。
    pub fn rebind(&mut self, uid: u32, new_combo: Combo, ts: u64) -> RebindOutcome {
        let Some(idx) = self.find_uid_idx(uid) else {
            return RebindOutcome::NoSuchId;
        };
        if !new_combo.sane() {
            return RebindOutcome::Malformed;
        }
        if let Some(j) = self.probe(new_combo.code()) {
            if j != idx {
                let occ = self.slots[j].expect("hotkeyreg invariant");
                return RebindOutcome::Occupied(Occupant { label: occ.label, app: occ.app_id() });
            }
        }
        let mut e = self.slots[idx].expect("hotkeyreg invariant");
        let old = e.combo;
        if old != new_combo {
            // 改键 = 从旧键位探测链摘除 + 按新键位家址重插（v2 批修复：
            // 修复前原地改写槽位，新键位的家址探测链够不着该条目，
            // 改键后派发丢失）。摘除后表内必有空槽 → 重插必成功；
            // 万一不成立的兜底是显性回滚并留痕，不静默。
            let was_overridden = e.overridden;
            self.remove_at(idx);
            e.combo = new_combo;
            e.overridden = true;
            let start = slot_of(new_combo.code());
            let mut placed = false;
            for k in 0..HK_CAP {
                let p = (start + k) % HK_CAP;
                if self.slots[p].is_none() {
                    self.slots[p] = Some(e);
                    placed = true;
                    break;
                }
            }
            if !placed {
                // 理论不可达（摘一插一）：显性回滚旧态并留痕，不静默。
                e.combo = old;
                e.overridden = was_overridden;
                let _ = self.insert_entry(e, ts);
            }
            self.version += 1;
            self.push_change(Change { ts, kind: ChangeKind::Rebind, uid, old, new: new_combo });
        }
        RebindOutcome::Done
    }

    /// 禁用/启用（判据「可禁用」）：禁用后活跃派发不命中、查询仍可见。
    pub fn set_enabled(&mut self, uid: u32, on: bool, ts: u64) -> bool {
        let Some(idx) = self.find_uid_idx(uid) else {
            return false;
        };
        let mut e = self.slots[idx].expect("hotkeyreg invariant");
        if e.enabled == on {
            return true; // 幂等
        }
        let old = e.combo;
        e.enabled = on;
        self.slots[idx] = Some(e);
        self.version += 1;
        let kind = if on { ChangeKind::Enable } else { ChangeKind::Disable };
        self.push_change(Change { ts, kind, uid, old, new: e.combo });
        true
    }

    /// 单条恢复默认。
    pub fn reset_one(&mut self, uid: u32, ts: u64) -> bool {
        let Some(idx) = self.find_uid_idx(uid) else {
            return false;
        };
        let mut e = self.slots[idx].expect("hotkeyreg invariant");
        if !e.overridden && e.enabled {
            return true;
        }
        let old = e.combo;
        e.combo = e.def;
        e.enabled = true;
        e.overridden = false;
        self.slots[idx] = Some(e);
        self.version += 1;
        self.push_change(Change { ts, kind: ChangeKind::ResetOne, uid, old, new: e.def });
        true
    }

    /// 恢复默认一键（判据「注册表带恢复默认一键」）：全表回出厂键位、
    /// 全部启用、清改键标记；返回实际恢复条数。
    pub fn reset_all(&mut self, ts: u64) -> usize {
        let mut n = 0;
        for idx in 0..HK_CAP {
            let Some(e) = self.slots[idx] else { continue };
            if e.overridden || !e.enabled || e.combo != e.def {
                let old = e.combo;
                let mut e2 = e;
                e2.combo = e2.def;
                e2.enabled = true;
                e2.overridden = false;
                self.slots[idx] = Some(e2);
                self.push_change(Change { ts, kind: ChangeKind::ResetOne, uid: e.uid(), old, new: e2.def });
                n += 1;
            }
        }
        if n > 0 {
            self.version += 1;
            self.push_change(Change { ts, kind: ChangeKind::ResetAll, uid: 0, old: Combo::none(), new: Combo::none() });
        }
        n
    }

    // -- 查询 / 派发 / 审计 ---------------------------------------------------

    /// 派发：键位 → 条目（禁用条目仍返回，由调用方按 enabled 处理——
    /// 设置中心「可查」与运行面「不触发」分离）。
    pub fn dispatch(&self, combo: Combo) -> Option<Entry> {
        self.probe(combo.code()).and_then(|i| self.slots[i])
    }

    /// 活跃派发：只命中启用条目（运行面用这个）。
    pub fn dispatch_active(&self, combo: Combo) -> Option<Entry> {
        self.dispatch(combo).filter(|e| e.enabled)
    }

    pub fn entry_by_uid(&self, uid: u32) -> Option<Entry> {
        self.find_uid_idx(uid).and_then(|i| self.slots[i])
    }

    /// 槽位直读（fuzz/审计面用；越界 None）。
    pub fn slot_at(&self, i: usize) -> Option<Entry> {
        if i < HK_CAP {
            self.slots[i]
        } else {
            None
        }
    }

    /// 全表快照（槽序，诊断面）。
    pub fn list(&self, out: &mut [Entry]) -> usize {
        let mut k = 0;
        for e in self.slots.iter().flatten() {
            if k < out.len() {
                out[k] = *e;
                k += 1;
            }
        }
        k
    }

    /// 系统条目数 / 第三方条目数等（设置中心概览）。
    pub fn stats(&self) -> RegStats {
        let mut s = RegStats { total: 0, system: 0, app: 0, disabled: 0, overridden: 0 };
        for e in self.slots.iter().flatten() {
            s.total += 1;
            if e.is_system() {
                s.system += 1;
            } else {
                s.app += 1;
            }
            if !e.enabled {
                s.disabled += 1;
            }
            if e.overridden {
                s.overridden += 1;
            }
        }
        s
    }

    /// 去重列出已接入的第三方应用。
    pub fn apps(&self, out: &mut [AppId]) -> usize {
        let mut k = 0;
        for e in self.slots.iter().flatten() {
            if let Source::App(a) = e.source {
                if k < out.len() && !out[..k].contains(&a) {
                    out[k] = a;
                    k += 1;
                }
            }
        }
        k
    }

    fn overridden_count(&self) -> usize {
        self.slots.iter().flatten().filter(|e| e.overridden || !e.enabled).count()
    }

    /// 预置完整性审计（判据本体）：46 条全量在册、键位=出厂、全启用、
    /// 无改键残留——「系统快捷键全量入表」的机器判定。
    pub fn audit_presets(&self) -> bool {
        if self.count < PRESETS.len() {
            return false;
        }
        PRESETS.iter().all(|(label, combo, _)| {
            self.entry_by_uid(uid_of_label(label))
                .map_or(false, |e| e.is_system() && e.combo == *combo && e.enabled && !e.overridden)
        })
    }

    // -- 冲突预检 / 派发留痕 ---------------------------------------------------

    /// 键位占用预检（设置页「此键位已被占用」实时提示的数据面——
    /// register/rebind 冲突门的只读版）。
    pub fn conflict_of(&self, combo: Combo) -> Option<Entry> {
        self.probe(combo.code()).and_then(|i| self.slots[i])
    }

    /// 带留痕派发（运行面唯一入口）：命中即写派发环并按槽位计热。
    /// 禁用条目同样留痕（active=false）但不触发——可查与可触发分离。
    pub fn dispatch_logged(&mut self, combo: Combo, ts: u64) -> Option<Entry> {
        let hit = self.dispatch(combo);
        if let Some(e) = hit {
            let uid = e.uid();
            if let Some(i) = self.probe(combo.code()) {
                self.hits[i] = self.hits[i].wrapping_add(1);
            }
            self.dlog.push(DispatchRec { ts, uid, combo, active: e.enabled });
        }
        hit
    }

    /// 派发留痕（新→旧，最多 32 条）。
    pub fn dispatch_log(&self) -> [Option<DispatchRec>; DISPATCH_CAP] {
        let mut out = [const { None }; DISPATCH_CAP];
        for (k, rec) in self.dlog.newest_first().iter().enumerate() {
            out[k] = Some(*rec);
        }
        out
    }

    /// 派发环条数（容量有界性核对）。
    pub fn dispatch_log_len(&self) -> usize {
        self.dlog.len()
    }

    /// 某 uid 的累计派发次数（热度画像面；未命中过为 0）。
    pub fn hits_for(&self, uid: u32) -> u32 {
        self.find_uid_idx(uid).map_or(0, |i| self.hits[i])
    }

    /// 派发总次数（= 全槽计数之和；画像对账面）。
    pub fn total_hits(&self) -> u32 {
        self.hits.iter().fold(0u32, |a, &h| a.wrapping_add(h))
    }

    /// 某条 uid 的变更链（旧→新——设置页条目详情的历史页签）。
    pub fn changes_for_uid(&self, uid: u32) -> Vec<Change> {
        let mut out: Vec<Change> = self
            .changes
            .newest_first()
            .iter()
            .copied()
            .filter(|c| c.uid == uid)
            .collect();
        out.reverse();
        out
    }

    /// 变更环条数。
    pub fn change_log_len(&self) -> usize {
        self.changes.len()
    }

    // -- 序列键（chord） -------------------------------------------------------

    /// 序列键登记：两段都不得与单键表任何键位冲突（否则单键被序列
    /// 「吞掉」——结构性排除），第一段在序列表内必须唯一（命中即武装，
    /// 二义性不允许）。表满显性拒绝。
    pub fn register_chord(&mut self, label: &'static str, first: Combo, second: Combo, ts: u64) -> bool {
        if !(first.sane() && second.sane()) {
            return false;
        }
        if self.probe(first.code()).is_some() || self.probe(second.code()).is_some() {
            return false;
        }
        if self.chords.iter().flatten().any(|c| c.first == first) {
            return false;
        }
        for slot in self.chords.iter_mut() {
            if slot.is_none() {
                *slot = Some(ChordRec { label, first, second, ts, overridden: false });
                self.version = self.version.wrapping_add(1);
                return true;
            }
        }
        false
    }

    /// 序列键注销（按语义名；未在册如实返回 false）。
    pub fn unregister_chord(&mut self, label: &str) -> bool {
        for slot in self.chords.iter_mut() {
            if let Some(c) = slot {
                if c.label == label {
                    *slot = None;
                    self.version = self.version.wrapping_add(1);
                    return true;
                }
            }
        }
        false
    }

    /// 序列键改段（label 在册才可改；新段位重过冲突门）。
    pub fn chord_rebind(&mut self, label: &str, first: Combo, second: Combo) -> bool {
        if !(first.sane() && second.sane()) {
            return false;
        }
        let exists = self.chords.iter().flatten().any(|c| c.label == label);
        if !exists {
            return false;
        }
        if self.chord_segment_conflicts(label, first) || self.chord_segment_conflicts(label, second) {
            return false;
        }
        for slot in self.chords.iter_mut() {
            if let Some(c) = slot {
                if c.label == label {
                    c.first = first;
                    c.second = second;
                    c.overridden = true;
                    self.version = self.version.wrapping_add(1);
                    return true;
                }
            }
        }
        false
    }

    /// 段位冲突复核：该段码不得等于任何「其他序列键的段」或「单键表
    /// 键位」（自身旧段除外——改段前的旧值不算冲突）。
    fn chord_segment_conflicts(&self, label: &str, seg: Combo) -> bool {
        if self.probe(seg.code()).is_some() {
            return true;
        }
        self.chords
            .iter()
            .flatten()
            .any(|c| c.label != label && (c.first == seg || c.second == seg))
    }

    /// 运行面逐键判定：Idle（继续普通派发）/ Armed（吞键等待）/ Fired。
    /// 等待窗超时自动回落（时间注入，模块不持时钟）。
    pub fn chord_key(&mut self, combo: Combo, ts: u64) -> ChordStep {
        // 1. 武装态优先：窗内认第二段；超时/不匹配回落。
        if let Some((idx, t0)) = self.chord_armed {
            if ts.saturating_sub(t0) <= CHORD_TIMEOUT_MS {
                if let Some(c) = self.chords[idx] {
                    if c.second == combo {
                        self.chord_armed = None;
                        return ChordStep::Fired { label: c.label };
                    }
                }
            }
            self.chord_armed = None;
        }
        // 2. 第一段命中 → 武装（本击吞掉，不派发动作）。
        for (i, slot) in self.chords.iter().enumerate() {
            if let Some(c) = slot {
                if c.first == combo {
                    self.chord_armed = Some((i, ts));
                    return ChordStep::Armed;
                }
            }
        }
        ChordStep::Idle
    }

    /// 当前武装中的序列键语义名（设置页「等待第二段」提示面）。
    pub fn chord_armed_label(&self) -> Option<&'static str> {
        self.chord_armed
            .and_then(|(i, _)| self.chords[i].as_ref())
            .map(|c| c.label)
    }

    /// 显性超时清除（设置页测试面的主动复位；返回清除前是否武装）。
    pub fn expire_chord(&mut self) -> bool {
        let was = self.chord_armed.is_some();
        self.chord_armed = None;
        was
    }

    /// 序列键条数。
    pub fn chord_count(&self) -> usize {
        self.chords.iter().flatten().count()
    }

    /// 按语义名查序列键。
    pub fn chord_by_label(&self, label: &str) -> Option<ChordRec> {
        self.chords.iter().flatten().find(|c| c.label == label).copied()
    }

    /// 序列键语义名清单（去重列出；写入 out 返回条数）。
    pub fn chord_labels(&self, out: &mut [&'static str]) -> usize {
        let mut k = 0;
        for c in self.chords.iter().flatten() {
            if k < out.len() {
                out[k] = c.label;
                k += 1;
            }
        }
        k
    }

    /// 键位是否为某序列键的第一段（派发路由的预判面）。
    pub fn is_chord_first(&self, combo: Combo) -> bool {
        self.chords.iter().flatten().any(|c| c.first == combo)
    }

    /// 序列键段位审计：全部段位不与单键表冲突、第一段互异——注册门
    /// 的独立复核面（结构被未来改动破坏时先红）。
    pub fn audit_chord_no_collision(&self) -> bool {
        let recs: Vec<ChordRec> = self.chords.iter().flatten().copied().collect();
        for c in recs.iter() {
            if self.probe(c.first.code()).is_some() || self.probe(c.second.code()).is_some() {
                return false;
            }
        }
        for i in 0..recs.len() {
            for j in (i + 1)..recs.len() {
                if recs[i].first == recs[j].first {
                    return false;
                }
            }
        }
        true
    }

    /// 单键表两两键位码互异审计（线性探测表的结构性保证——独立复核
    /// 面，与探测/删除单测互补）。
    pub fn audit_conflict_free(&self) -> bool {
        let entries: Vec<Entry> = self.slots.iter().flatten().copied().collect();
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                if entries[i].combo.code() == entries[j].combo.code() {
                    return false;
                }
            }
        }
        true
    }

    // -- 批量查询 / 应用注销 ---------------------------------------------------

    /// 某应用的全部条目（注册序；写入 out 返回条数）。
    pub fn entries_of_app(&self, app: &AppId, out: &mut [Entry]) -> usize {
        let mut k = 0;
        for e in self.slots.iter().flatten() {
            if e.source == Source::App(*app) && k < out.len() {
                out[k] = *e;
                k += 1;
            }
        }
        k
    }

    /// 某应用的全部 uid（注销对账面）。
    pub fn app_uids(&self, app: &AppId, out: &mut [u32]) -> usize {
        let mut k = 0;
        for e in self.slots.iter().flatten() {
            if e.source == Source::App(*app) && k < out.len() {
                out[k] = e.uid();
                k += 1;
            }
        }
        k
    }

    /// 系统条目清单（预置走查面）。
    pub fn system_entries(&self, out: &mut [Entry]) -> usize {
        let mut k = 0;
        for e in self.slots.iter().flatten() {
            if e.is_system() && k < out.len() {
                out[k] = *e;
                k += 1;
            }
        }
        k
    }

    /// 全表语义名清单（诊断导出面）。
    pub fn entry_labels(&self, out: &mut [&'static str]) -> usize {
        let mut k = 0;
        for e in self.slots.iter().flatten() {
            if k < out.len() {
                out[k] = e.label;
                k += 1;
            }
        }
        k
    }

    /// 应用批量注销（卸载清理）：逐条走后退移位删除，留痕 Unregister。
    /// 反复「找第一条 → 删」直至无——回避迭代中探测链搬移的遍历陷阱。
    pub fn unregister_app(&mut self, app: AppId, ts: u64) -> usize {
        let mut n = 0;
        while let Some(idx) = self
            .slots
            .iter()
            .position(|s| matches!(s, Some(e) if e.source == Source::App(app)))
        {
            let e = self.slots[idx].expect("hotkeyreg invariant");
            let uid = e.uid();
            self.remove_at(idx);
            self.count -= 1;
            self.push_change(Change { ts, kind: ChangeKind::Unregister, uid, old: e.combo, new: Combo::none() });
            n += 1;
        }
        if n > 0 {
            self.version += 1;
        }
        n
    }

    // -- 预置分组 --------------------------------------------------------------

    /// 预置分组计数（设置中心分类树概览）：只统计**已在册**的预置条目
    /// ——安装半途/被注销时计数如实反映缺额，与 [`PRESET_CAT`] 对应。
    pub fn preset_category_counts(&self) -> [u32; 4] {
        let mut counts = [0u32; 4];
        for (i, (label, _, _)) in PRESETS.iter().enumerate() {
            if self.find_uid_idx(uid_of_label(label)).is_some() {
                counts[PRESET_CAT[i] as usize] += 1;
            }
        }
        counts
    }

    // -- 运行面专用派发 --------------------------------------------------------

    /// 活跃派发带留痕（运行面用这个）：只认启用条目，禁用命中留痕但
    /// 返回 None——「可查」与「可触发」在一个入口内分流。
    pub fn dispatch_active_logged(&mut self, combo: Combo, ts: u64) -> Option<Entry> {
        let hit = self.dispatch_logged(combo, ts).filter(|e| e.enabled);
        hit
    }

    // -- 批量改键 / 占用查询 / 检索 -------------------------------------------

    /// 批量改键（配置装配面）：逐条过冲突门，返回成功数——部分成功
    /// 不是静默：调用方可用 rebind 逐条复核失败项。
    pub fn rebind_many(&mut self, pairs: &[(u32, Combo)], ts: u64) -> usize {
        let mut ok = 0;
        for &(uid, combo) in pairs.iter() {
            if self.rebind(uid, combo, ts) == RebindOutcome::Done {
                ok += 1;
            }
        }
        ok
    }

    /// 批量占用查询（设置中心「导入前预检」面）：对申请键位清单逐个
    /// 给占用方（无占用为 None），数量与输入对齐。
    pub fn find_occupants(&self, combos: &[Combo], out: &mut [Option<Occupant>]) -> usize {
        let n = combos.len().min(out.len());
        for (o, &c) in out[..n].iter_mut().zip(combos.iter()) {
            *o = self.probe(c.code()).and_then(|i| self.slots[i]).map(|e| Occupant {
                label: e.label,
                app: e.app_id(),
            });
        }
        n
    }

    /// 按语义名前缀检索（设置中心搜索框数据面；系统条目 label 前缀）。
    pub fn entries_by_prefix(&self, prefix: &str, out: &mut [Entry]) -> usize {
        let pb = prefix.as_bytes();
        let mut k = 0;
        for e in self.slots.iter().flatten() {
            if e.label.len() >= pb.len() && &e.label.as_bytes()[..pb.len()] == pb && k < out.len() {
                out[k] = *e;
                k += 1;
            }
        }
        k
    }

    /// 分类叶面：某预置分组的语义名清单（设置中心分类树展开）。
    pub fn preset_labels_by_cat(&self, cat: usize, out: &mut [&'static str]) -> usize {
        if cat >= CATEGORY_NAMES.len() {
            return 0;
        }
        let mut k = 0;
        for (i, (label, _, _)) in PRESETS.iter().enumerate() {
            if PRESET_CAT[i] as usize == cat && k < out.len() {
                out[k] = *label;
                k += 1;
            }
        }
        k
    }

    // -- 序列键差异面持久化 -----------------------------------------------------

    /// 导出序列键差异（被改过段的条目）：`VXHC` 头 + 条数 u16LE + 每条
    /// 12 字节（uid u32LE、第一段 mods+key u16LE、第二段 mods+key）。
    /// label 是 'static 语义名不入差异面——导入按 uid 改已在册条目段位。
    pub fn export_chords(&self, out: &mut [u8]) -> usize {
        let n = self.chords.iter().flatten().filter(|c| c.overridden).count();
        let need = 6 + n * 12;
        if out.len() < need {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXHC");
        out[4..6].copy_from_slice(&(n as u16).to_le_bytes());
        let mut p = 6;
        for c in self.chords.iter().flatten() {
            if c.overridden {
                out[p..p + 4].copy_from_slice(&chord_uid(c).to_le_bytes());
                out[p + 4] = c.first.mods;
                out[p + 5..p + 7].copy_from_slice(&c.first.key.to_le_bytes());
                out[p + 7] = c.second.mods;
                out[p + 9..p + 11].copy_from_slice(&c.second.key.to_le_bytes());
                p += 12;
            }
        }
        need
    }

    /// 导入序列键差异：逐条按 uid 定位在册序列键 → 过冲突门 → 改段。
    /// 返回成功数；未在册 uid/冲突/越界段跳过并如实计数（缓冲截断面）。
    pub fn import_chords(&mut self, buf: &[u8]) -> usize {
        if buf.len() < 6 || &buf[0..4] != b"VXHC" {
            return 0;
        }
        let n = u16::from_le_bytes([buf[4], buf[5]]) as usize;
        let mut applied = 0;
        for k in 0..n {
            let base = 6 + k * 12;
            if base + 12 > buf.len() {
                break;
            }
            let uid = u32::from_le_bytes([buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]);
            let first = Combo::new(buf[base + 4], u16::from_le_bytes([buf[base + 5], buf[base + 6]]));
            let second = Combo::new(buf[base + 7], u16::from_le_bytes([buf[base + 9], buf[base + 10]]));
            let label = self
                .chords
                .iter()
                .flatten()
                .find(|c| chord_uid(c) == uid)
                .map(|c| c.label);
            if let Some(label) = label {
                if self.chord_rebind(label, first, second) {
                    applied += 1;
                }
            }
        }
        applied
    }

    /// 槽位热度向量全量导出（与槽序一一对应——设置中心热度排序与
    /// 诊断对账面；派发计数的成批读出，免逐 uid 查表）。
    pub fn hits_snapshot(&self, out: &mut [u32]) -> usize {
        let n = HK_CAP.min(out.len());
        out[..n].copy_from_slice(&self.hits[..n]);
        n
    }

    /// 序列键全量导出（诊断面；表序写入 out 返回条数）。
    pub fn chords_all(&self, out: &mut [ChordRec]) -> usize {
        let mut k = 0;
        for c in self.chords.iter().flatten() {
            if k < out.len() {
                out[k] = *c;
                k += 1;
            }
        }
        k
    }

    // -- 持久化（用户改键差异面） ---------------------------------------------

    /// 导出用户差异（改键/禁用条目）：`VXHK` 头 + 条数 u16LE +
    /// 每条 9 字节（uid u32LE、mods、key u16LE、flags）。缓冲不足返回 0。
    pub fn export_overrides(&self, out: &mut [u8]) -> usize {
        let n = self.overridden_count();
        let need = 6 + n * 9;
        if out.len() < need || need > 0xFFFF + 6 {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXHK");
        out[4..6].copy_from_slice(&(n as u16).to_le_bytes());
        let mut p = 6;
        for e in self.slots.iter().flatten() {
            if e.overridden || !e.enabled {
                out[p..p + 4].copy_from_slice(&e.uid().to_le_bytes());
                out[p + 4] = e.combo.mods;
                out[p + 5..p + 7].copy_from_slice(&e.combo.key.to_le_bytes());
                out[p + 7] = e.enabled as u8 | ((e.overridden as u8) << 1);
                p += 9;
            }
        }
        p
    }

    /// 导入用户差异（启动装载面）：逐条冲突复核——目标键位已被其他
    /// 条目占用的差异跳过（先到先得同样适用于装载序）；返回生效条数。
    pub fn import_overrides(&mut self, buf: &[u8], ts: u64) -> usize {
        if buf.len() < 6 || buf[0..4] != *b"VXHK" {
            return 0;
        }
        let cnt = u16::from_le_bytes([buf[4], buf[5]]) as usize;
        if buf.len() < 6 + cnt * 9 {
            return 0;
        }
        let mut applied = 0;
        let mut p = 6;
        for _ in 0..cnt {
            let uid = u32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]);
            let mods = buf[p + 4] & MOD_MASK;
            let key = u16::from_le_bytes([buf[p + 5], buf[p + 6]]);
            let flags = buf[p + 7];
            p += 9;
            let target = Combo::new(mods, key);
            if !target.sane() {
                continue;
            }
            let Some(idx) = self.find_uid_idx(uid) else { continue };
            if self.probe(target.code()).map_or(false, |j| j != idx) {
                continue;
            }
            let mut e = self.slots[idx].expect("hotkeyreg invariant");
            let old = e.combo;
            if old != target {
                // 修障登记（与 rebind 同病同修）：改键 = 从旧键位探测链摘除
                // + 按新键位家址重插。原地改写槽位会让新键位的家址探测链
                // 够不着条目，装载后派发丢失。摘一插一必有空槽；不成立的
                // 兜底是显性回滚并留痕，不静默。
                let was_overridden = e.overridden;
                let was_enabled = e.enabled;
                self.remove_at(idx);
                e.combo = target;
                e.overridden = flags & 2 != 0;
                e.enabled = flags & 1 == 1;
                let start = slot_of(target.code());
                let mut placed = false;
                for k in 0..HK_CAP {
                    let p = (start + k) % HK_CAP;
                    if self.slots[p].is_none() {
                        self.slots[p] = Some(e);
                        placed = true;
                        break;
                    }
                }
                if !placed {
                    // 理论不可达（摘一插一）：显性回滚旧态并留痕，不静默。
                    e.combo = old;
                    e.overridden = was_overridden;
                    e.enabled = was_enabled;
                    let _ = self.insert_entry(e, ts);
                }
            } else {
                e.overridden = flags & 2 != 0;
                e.enabled = flags & 1 == 1;
                self.slots[idx] = Some(e);
            }
            self.push_change(Change { ts, kind: ChangeKind::Rebind, uid, old, new: target });
            applied += 1;
        }
        if applied > 0 {
            self.version += 1;
        }
        applied
    }
}

impl Default for HotkeyReg {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 系统预置表（46 条全量——判据「系统快捷键全量入表」的数据面）
// ---------------------------------------------------------------------------

/// 系统级默认键位表：语义名 / 键位 / 说明。数值为主册判据与 F220/F249/
/// F081/F235 锚点的并集，每条可通过设置中心改键/禁用。
pub const PRESETS: [(&str, Combo, &str); 46] = [
    ("sys.copy", Combo::new(MOD_CTRL, 0x43), "复制选中内容"),
    ("sys.paste", Combo::new(MOD_CTRL, 0x56), "粘贴"),
    ("sys.cut", Combo::new(MOD_CTRL, 0x58), "剪切"),
    ("sys.undo", Combo::new(MOD_CTRL, 0x5A), "撤销"),
    ("sys.redo", Combo::new(MOD_CTRL, 0x59), "重做"),
    ("sys.find", Combo::new(MOD_CTRL, 0x46), "查找"),
    ("sys.paste-plain", Combo::new(MOD_CTRL | MOD_SHIFT, 0x56), "纯文本粘贴（F220）"),
    ("sys.select-all", Combo::new(MOD_CTRL, 0x41), "全选"),
    ("sys.save", Combo::new(MOD_CTRL, 0x53), "保存"),
    ("sys.print", Combo::new(MOD_CTRL, 0x50), "打印"),
    ("sys.close-tab", Combo::new(MOD_CTRL, 0x57), "关闭标签页"),
    ("sys.new-tab", Combo::new(MOD_CTRL, 0x54), "新建标签页"),
    ("sys.new-window", Combo::new(MOD_CTRL, 0x4E), "新建窗口"),
    ("sys.taskmgr", Combo::new(MOD_CTRL | MOD_SHIFT, VK_ESCAPE), "任务管理器"),
    ("sys.alt-tab", Combo::new(MOD_ALT, VK_TAB), "窗口切换"),
    ("sys.close-win", Combo::new(MOD_ALT, VK_F1 + 3), "关闭窗口"),
    ("sys.props", Combo::new(MOD_ALT, VK_RETURN), "属性"),
    ("sys.start-menu", Combo::new(0, VK_LWIN), "呼出开始菜单"),
    ("sys.explorer", Combo::new(MOD_WIN, 0x45), "打开资源管理器"),
    ("sys.show-desktop", Combo::new(MOD_WIN, 0x44), "显示桌面"),
    ("sys.lock", Combo::new(MOD_WIN, 0x4C), "锁定屏幕"),
    ("sys.run", Combo::new(MOD_WIN, 0x52), "运行"),
    ("sys.task-view", Combo::new(MOD_WIN, VK_TAB), "任务视图（F081）"),
    ("sys.snap-left", Combo::new(MOD_WIN, VK_LEFT), "窗口贴靠左半屏"),
    ("sys.snap-right", Combo::new(MOD_WIN, VK_RIGHT), "窗口贴靠右半屏"),
    ("sys.snap-max", Combo::new(MOD_WIN, VK_UP), "窗口最大化"),
    ("sys.snap-min", Combo::new(MOD_WIN, VK_DOWN), "窗口最小化"),
    ("sys.desk-prev", Combo::new(MOD_WIN | MOD_CTRL, VK_LEFT), "虚拟桌面左移（F235）"),
    ("sys.desk-next", Combo::new(MOD_WIN | MOD_CTRL, VK_RIGHT), "虚拟桌面右移（F235）"),
    ("sys.desk-new", Combo::new(MOD_WIN | MOD_CTRL, 0x44), "新建虚拟桌面"),
    ("sys.min-all", Combo::new(MOD_WIN, 0x4D), "最小化全部窗口"),
    ("sys.clip-hist", Combo::new(MOD_WIN, 0x56), "剪贴板历史"),
    ("sys.project", Combo::new(MOD_WIN, 0x50), "投影切换"),
    ("sys.settings", Combo::new(MOD_WIN, 0x49), "打开设置中心"),
    ("sys.quick-settings", Combo::new(MOD_WIN, 0x41), "快速设置（F249 右下热角同源）"),
    ("sys.notif-center", Combo::new(MOD_WIN, 0x4E), "通知中心（F249 右上热角同源）"),
    ("sys.help", Combo::new(0, VK_F1), "帮助"),
    ("sys.rename", Combo::new(0, VK_F1 + 1), "重命名"),
    ("sys.find-next", Combo::new(0, VK_F1 + 2), "查找下一个"),
    ("sys.refresh", Combo::new(0, VK_F1 + 4), "刷新"),
    ("sys.menubar", Combo::new(0, VK_F1 + 9), "菜单栏"),
    ("sys.fullscreen", Combo::new(0, VK_F1 + 10), "全屏切换"),
    ("sys.devtools", Combo::new(0, VK_F1 + 11), "开发者工具"),
    ("sys.address", Combo::new(MOD_ALT, 0x44), "聚焦地址栏"),
    ("sys.screenshot", Combo::new(0, VK_SNAPSHOT), "截屏"),
    ("sys.cycle-pane", Combo::new(0, VK_F1 + 5), "窗格循环聚焦"),
];

impl HotkeyReg {
    /// 安装全部系统预置（已存在的 uid 跳过——幂等重装）；返回新装条数。
    pub fn preset_all(&mut self, ts: u64) -> usize {
        let mut n = 0;
        for (label, combo, desc) in PRESETS.iter() {
            // *label/*desc：内层本就是 'static——不能靠 &&str 自动解引用
            // 强转 &'static str（外层借用的是 const 具象化临时值）。
            if self.register_system(*label, *combo, *desc, ts) == RegOutcome::Registered {
                n += 1;
            }
        }
        self.preset_installed += n as u32;
        n
    }
}

// ---------------------------------------------------------------------------
// vxapp 清单声明解析
// ---------------------------------------------------------------------------

/// 清单声明行格式：`hotkey=ctrl+alt+f9|说明文本`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedDecl {
    pub mods: u8,
    pub key: u16,
    pub desc: DescBuf,
}

const DECL_PREFIX: &[u8] = b"hotkey=";

/// 解析一条声明行：前缀校验 + 逐 token 校验（未知 token、双键名、
/// 重复修饰键、缺键名显性拒绝——清单错误不静默）。
pub fn parse_decl(line: &[u8]) -> Option<ParsedDecl> {
    if line.len() < DECL_PREFIX.len() + 3 || &line[..DECL_PREFIX.len()] != DECL_PREFIX {
        return None;
    }
    let rest = &line[DECL_PREFIX.len()..];
    let bar = rest.iter().position(|&b| b == b'|')?;
    let combo_part = &rest[..bar];
    let desc_part = &rest[bar + 1..];
    let mut mods = 0u8;
    let mut key: Option<u16> = None;
    let mut start = 0usize;
    for i in 0..=combo_part.len() {
        if i == combo_part.len() || combo_part[i] == b'+' {
            let tok = &combo_part[start..i];
            if tok.is_empty() {
                return None;
            }
            match mod_from_token(tok) {
                Some(m) => {
                    if key.is_some() || mods & m != 0 {
                        return None; // 键名出现在键位之前 / 重复修饰键
                    }
                    mods |= m;
                }
                None => {
                    if key.is_some() {
                        return None; // 双键名
                    }
                    key = Some(name_to_key(tok)?);
                }
            }
            start = i + 1;
        }
    }
    let key = key?;
    if !Combo::new(mods, key).sane() {
        return None;
    }
    Some(ParsedDecl { mods, key, desc: DescBuf::from_bytes(desc_part) })
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F244 自检（判据：完整性审计、冲突 3 例、改键持久化、第三方接入；
/// 含 xors32 fuzz）。
pub fn run_hotkeyreg_checks() -> CheckSet {
    let mut set = CheckSet::new("F244-hotkeyreg");

    // 1. 预置表全量安装（判据本体）：46 条全量入表。
    let mut reg = HotkeyReg::new();
    let installed = reg.preset_all(1000);
    set.add(
        "preset table fully installed",
        installed == PRESETS.len() && reg.count == PRESETS.len() && reg.preset_installed == PRESETS.len() as u32,
        "",
    );

    // 2. 完整性审计 + 数值抽查：三条代表键位与主册一致。
    let cc = reg.entry_by_uid(uid_of_label("sys.copy"));
    let at = reg.dispatch(Combo::new(MOD_ALT, VK_TAB));
    let wl = reg.dispatch(Combo::new(MOD_WIN, 0x4C));
    set.add(
        "preset audit + spot checks",
        reg.audit_presets()
            && cc.map_or(false, |e| e.combo == Combo::new(MOD_CTRL, 0x43) && e.enabled)
            && at.map_or(false, |e| e.label == "sys.alt-tab")
            && wl.map_or(false, |e| e.label == "sys.lock"),
        "",
    );

    // 3. 冲突即检例 1（系统 vs 系统）：注册 Ctrl+C → Occupied 且列占用方。
    let o1 = reg.register_system("sys.my-copy", Combo::new(MOD_CTRL, 0x43), "", 1100);
    set.add(
        "conflict case 1: lists system occupant",
        matches!(o1, RegOutcome::Occupied(ref occ) if occ.is_system() && occ.label == "sys.copy"),
        "",
    );

    // 4. 冲突即检例 2（第三方 vs 系统）：声明 Win+E → 占用可见、原条目
    //    键位未被覆盖（先到先得，无静默覆盖）。
    let decl = parse_decl("hotkey=win+e|打开资源管理器（第三方）".as_bytes()).expect("hotkeyreg invariant");
    let o2 = reg.register_app_decl(AppId::from_bytes(b"vxapp-demo"), &decl, 0, 1200);
    set.add(
        "conflict case 2: third-party sees system occupant",
        matches!(o2, RegOutcome::Occupied(ref occ) if occ.is_system() && occ.label == "sys.explorer")
            && reg.dispatch(Combo::new(MOD_WIN, 0x45)).map_or(false, |e| e.label == "sys.explorer"),
        "",
    );

    // 5. 冲突即检例 3（改键撞第三方）：先让第三方成功注册 ctrl+alt+f9，
    //    再把 sys.copy 改到该键位 → Occupied（第三方占用方可见）。
    let decl_ok = parse_decl("hotkey=ctrl+alt+f9|全局截屏增强".as_bytes()).expect("hotkeyreg invariant");
    let app2 = AppId::from_bytes(b"vxapp-shot");
    let o3a = reg.register_app_decl(app2, &decl_ok, 0, 1300);
    let copy_uid = uid_of_label("sys.copy");
    let o3b = reg.rebind(copy_uid, Combo::new(MOD_CTRL | MOD_ALT, VK_F1 + 8), 1400);
    set.add(
        "conflict case 3: rebind hits third-party occupant",
        o3a == RegOutcome::Registered
            && matches!(o3b, RebindOutcome::Occupied(ref occ) if !occ.is_system() && occ.app.as_bytes() == b"vxapp-shot"),
        "",
    );

    // 6. 用户改键成功路径：sys.copy → Ctrl+Shift+C；新旧派发面翻转。
    let r6 = reg.rebind(copy_uid, Combo::new(MOD_CTRL | MOD_SHIFT, 0x43), 1500);
    set.add(
        "user rebind flips dispatch",
        r6 == RebindOutcome::Done
            && reg.dispatch(Combo::new(MOD_CTRL | MOD_SHIFT, 0x43)).map_or(false, |e| e.uid() == copy_uid && e.overridden)
            && reg.entry_by_uid(copy_uid).map_or(true, |e| e.combo != Combo::new(MOD_CTRL, 0x43))
            && reg.version >= 5,
        "",
    );

    // 7. 改键持久化 round-trip：导出差异 → 新表装载 → 状态一致。
    let _ = reg.set_enabled(uid_of_label("sys.undo"), false, 1600);
    let mut blob = [0u8; 256];
    let n = reg.export_overrides(&mut blob);
    // 修障登记（check 7 红）：applied 是生效条数、n 是导出字节长度
    // （6 + 条数×9），二者结构性不等——对拍对象改为 blob 头部声明的
    // 差异条数（判据本体：每条导出差异都须生效）。
    let want = u16::from_le_bytes([blob[4], blob[5]]) as usize;
    let mut reg2 = HotkeyReg::new();
    let _ = reg2.preset_all(2000);
    let applied = reg2.import_overrides(&blob[..n], 2100);
    let e_src = reg.entry_by_uid(copy_uid).expect("hotkeyreg invariant");
    let e_dst = reg2.entry_by_uid(copy_uid).expect("hotkeyreg invariant");
    set.add(
        "rebind persistence round-trip",
        n > 0
            && applied == want
            && e_dst.combo == e_src.combo
            && e_dst.overridden == e_src.overridden
            && reg2.entry_by_uid(uid_of_label("sys.undo")).map_or(false, |e| !e.enabled),
        "",
    );

    // 8. 禁用位：禁用后活跃派发不命中、普通派发仍可查（设置中心可查可禁用）。
    let undo_uid = uid_of_label("sys.undo");
    set.add(
        "disabled entry visible but not active",
        reg.dispatch(Combo::new(MOD_CTRL, 0x5A)).map_or(false, |e| e.uid() == undo_uid)
            && reg.dispatch_active(Combo::new(MOD_CTRL, 0x5A)).is_none()
            && reg2.dispatch_active(Combo::new(MOD_CTRL, 0x5A)).is_none(),
        "",
    );

    // 9. 恢复默认一键：reset_all 后全表回出厂、审计转绿、版本推进。
    let v_before = reg.version;
    let restored = reg.reset_all(3000);
    set.add(
        "reset all restores factory",
        restored >= 2
            && reg.audit_presets()
            && reg.version > v_before
            && reg.entry_by_uid(copy_uid).map_or(false, |e| e.combo == Combo::new(MOD_CTRL, 0x43)),
        "",
    );

    // 10. 清单声明解析：合法行逐项正确；三类非法行显性拒绝。
    let d1 = parse_decl("hotkey=ctrl+shift+f5|快速收集".as_bytes());
    let bad1 = parse_decl(b"hotkey=ctrl|no key");
    let bad2 = parse_decl(b"hotkey=ctrl+f10+f11|double key");
    let bad3 = parse_decl(b"hotkey=hyper+f1|unknown mod");
    set.add(
        "manifest parse valid & explicit rejects",
        d1.map_or(false, |d| d.mods == (MOD_CTRL | MOD_SHIFT) && d.key == VK_F1 + 4 && d.desc.as_bytes() == "快速收集".as_bytes())
            && bad1.is_none()
            && bad2.is_none()
            && bad3.is_none(),
        "",
    );

    // 11. 第三方接入验证：两条声明注册、apps() 去重列出、stats() 计数。
    let mut reg3 = HotkeyReg::new();
    let _ = reg3.preset_all(4000);
    let d_a = parse_decl("hotkey=ctrl+alt+7|应用 A 快捷动作一".as_bytes()).expect("hotkeyreg invariant");
    let d_b = parse_decl("hotkey=ctrl+alt+8|应用 A 快捷动作二".as_bytes()).expect("hotkeyreg invariant");
    let appa = AppId::from_bytes(b"vxapp-a");
    let ra1 = reg3.register_app_decl(appa, &d_a, 0, 4100);
    let ra2 = reg3.register_app_decl(appa, &d_b, 1, 4200);
    let mut app_list = [AppId::none(); 8];
    let napps = reg3.apps(&mut app_list);
    let st = reg3.stats();
    set.add(
        "third-party intake & stats",
        ra1 == RegOutcome::Registered
            && ra2 == RegOutcome::Registered
            && napps == 1
            && app_list[0].as_bytes() == b"vxapp-a"
            && st.app == 2
            && st.total == PRESETS.len() + 2,
        "",
    );

    // 12. 键位显示串（设置中心显示面）：四条参考例逐字核对。
    //     各用独立缓冲——s1..s4 借用各自数组，避免借用冲突（零堆纪律不变）。
    let mut b1 = [0u8; 24];
    let mut b2 = [0u8; 24];
    let mut b3 = [0u8; 24];
    let mut b4 = [0u8; 24];
    let n1 = combo_str(Combo::new(MOD_CTRL | MOD_SHIFT, 0x56), &mut b1);
    let s1 = core::str::from_utf8(&b1[..n1]).unwrap_or("?");
    let n2 = combo_str(Combo::new(MOD_ALT, VK_TAB), &mut b2);
    let s2 = core::str::from_utf8(&b2[..n2]).unwrap_or("?");
    let n3 = combo_str(Combo::new(0, VK_LWIN), &mut b3);
    let s3 = core::str::from_utf8(&b3[..n3]).unwrap_or("?");
    let n4 = combo_str(Combo::new(0, VK_F1 + 4), &mut b4);
    let s4 = core::str::from_utf8(&b4[..n4]).unwrap_or("?");
    set.add(
        "combo display strings",
        s1 == "Ctrl+Shift+V" && s2 == "Alt+Tab" && s3 == "Win" && s4 == "F5",
        "",
    );

    // 13. xors32 fuzz 2000 轮：注册/注销/改键混合流——注册成功必可派发、
    //     冲突必显性拒绝且原条目键位不变、表容量有界、无 panic。
    let mut x: u32 = 0x853C_49E7;
    let mut fz = HotkeyReg::new();
    let mut survived = true;
    let mut ts: u64 = 0;
    for i in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        ts += (x % 7 + 1) as u64;
        match x % 10 {
            0..=4 => {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let combo = Combo::new((x % 16) as u8, VK_A + (x % 26) as u16);
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let mut appb = [0u8; 8];
                appb[0] = b'a';
                appb[1..5].copy_from_slice(&i.to_le_bytes());
                let decl = ParsedDecl { mods: combo.mods, key: combo.key, desc: DescBuf::from_bytes(b"fuzz") };
                let before = fz.dispatch(combo);
                match fz.register_app_decl(AppId::from_bytes(&appb), &decl, 0, ts) {
                    RegOutcome::Registered => {
                        let want_uid = fnv1a(&appb).rotate_left(8);
                        if before.is_some() || fz.dispatch(combo).map_or(true, |e| e.uid() != want_uid) {
                            survived = false;
                        }
                    }
                    RegOutcome::Occupied(_) => {
                        if before.is_none() {
                            survived = false;
                        }
                    }
                    RegOutcome::Full => {
                        if fz.count < HK_CAP {
                            survived = false;
                        }
                    }
                    _ => {}
                }
            }
            5..=6 => {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let combo = Combo::new((x % 16) as u8, VK_A + (x % 26) as u16);
                if let Some(e) = fz.dispatch(combo) {
                    let uid = e.uid();
                    let removed = fz.unregister(uid, ts);
                    if removed && fz.dispatch(combo).is_some() {
                        survived = false;
                    }
                }
            }
            _ => {
                // 改键：随便挑一个在册条目改到随机键位。
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let pick = (x as usize) % HK_CAP;
                if let Some(e) = fz.slot_at(pick) {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    let new_combo = Combo::new((x % 16) as u8, VK_A + (x % 26) as u16);
                    let old = e.combo;
                    match fz.rebind(e.uid(), new_combo, ts) {
                        RebindOutcome::Done => {
                            if new_combo != old && fz.dispatch(old).map_or(false, |e2| e2.uid() == e.uid()) {
                                survived = false; // 旧键位不应再命中自己
                            }
                        }
                        RebindOutcome::Occupied(_) => {
                            if fz.dispatch(new_combo).is_none() {
                                survived = false;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    set.add(
        "fuzz 2000 registry ops invariants hold",
        survived && fz.count <= HK_CAP,
        "",
    );

    // 14. 序列键（chord）：登记门（段位撞单键拒绝/第一段互异）→ 逐键
    //     状态机（Armed 吞键/窗内第二段 Fired/超时回落）→ 派发留痕与
    //     热度计数对账 → 应用批量注销 → 预置分组计数 → 两级审计全绿。
    let mut g = HotkeyReg::new();
    let _ = g.preset_all(100);
    let f1 = Combo::new(MOD_CTRL, 0x58); // Ctrl+X
    let f2 = Combo::new(MOD_CTRL | MOD_ALT, 0x4B); // Ctrl+Alt+K
    let s2 = Combo::new(MOD_CTRL | MOD_ALT, 0x4C); // Ctrl+Alt+L
    let ok1 = g.register_chord("app.emacs.kill", f2, s2, 200);
    // 第一段撞单键（Ctrl+X 与 sys.cut 冲突）→ 显性拒绝。
    let bad1 = g.register_chord("app.bad.chord", f1, s2, 210) == false;
    let dup_first = g.register_chord("app.dup.chord", f2, Combo::new(MOD_ALT, 0x4A), 220) == false;
    let chord_step = g.chord_key(f2, 300);
    let armed_hint = g.chord_armed_label();
    let fired = g.chord_key(s2, 400);
    let idle_after = g.chord_armed_label().is_none();
    // 普通单键派发带留痕 + 热度计数。
    let hit = g.dispatch_logged(Combo::new(MOD_CTRL, 0x43), 500); // sys.copy
    let copy_uid = uid_of_label("sys.copy");
    let _ = g.dispatch_logged(Combo::new(MOD_CTRL, 0x43), 510);
    let dlog = g.dispatch_log();
    let cats = g.preset_category_counts();
    let app3 = AppId::from_bytes(b"vxapp-zap");
    // 修障登记（check 14 红）：哈希表以键位码为主键，同一组合键（Alt+J）
    // 注册两条必撞冲突门（第二条 Occupied、不入表），`removed == 2`
    // 结构性不可达——两条声明改为互异键位（Alt+J / Alt+K），批量注销
    // 判据恢复可满足，并以注册成功前置断言补强。
    let d3 = ParsedDecl { mods: MOD_ALT, key: 0x4A, desc: DescBuf::from_bytes(b"zap") };
    let d4 = ParsedDecl { mods: MOD_ALT, key: 0x4B, desc: DescBuf::from_bytes(b"zap2") };
    let r3a = g.register_app_decl(app3, &d3, 0, 600);
    let r3b = g.register_app_decl(app3, &d4, 1, 610);
    let removed = g.unregister_app(app3, 620);
    let inv = g.audit_conflict_free() && g.audit_chord_no_collision();
    set.add(
        "chord state machine & dispatch log & app purge",
        ok1 && bad1 && dup_first
            && chord_step == ChordStep::Armed
            && armed_hint == Some("app.emacs.kill")
            && fired == ChordStep::Fired { label: "app.emacs.kill" }
            && idle_after
            && hit.map_or(false, |e| e.uid() == copy_uid)
            && g.dispatch_log_len() == 2
            && dlog[0].map_or(false, |r| r.uid == copy_uid && r.active && r.ts == 510)
            && g.hits_for(copy_uid) == 2
            && g.total_hits() == 2
            && g.chord_count() == 1
            && g.chord_by_label("app.emacs.kill").map_or(false, |c| c.first == f2 && c.second == s2)
            && cats == [12, 13, 15, 6]
            && r3a == RegOutcome::Registered
            && r3b == RegOutcome::Registered
            && removed == 2
            && g.entry_by_uid(fnv1a(b"vxapp-zap").rotate_left(8)).is_none()
            && !g.changes_for_uid(copy_uid).is_empty()
            && g.changes_for_uid(copy_uid)[0].kind == ChangeKind::Register
            && g.change_log_len() >= 2
            && inv
            && CHORD_DOC.contains("800ms")
            && CONFLICT_HINT_DOC.contains("占用方"),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_table_has_no_internal_collision() {
        let mut reg = HotkeyReg::new();
        for (label, combo, desc) in PRESETS.iter() {
            assert_eq!(
                reg.register_system(*label, *combo, *desc, 1),
                RegOutcome::Registered,
                "预置表内部冲突：{label}"
            );
        }
        assert_eq!(reg.count, PRESETS.len());
    }

    #[test]
    fn backward_shift_keeps_probe_chain_intact() {
        // 密集注册 → 从中段抽删 → 全部幸存条目必须仍可派发（后退移位
        // 正确性的直接验证，线性探测删除的经典坑）。
        let mut reg = HotkeyReg::new();
        let mut labels: [&'static str; 40] = [""; 40];
        for i in 0..40usize {
            // 每条不同的键位码（key = A+i, mods 交错制造同槽聚集）。
            let label: &'static str = LABELS[i];
            labels[i] = label;
            let combo = Combo::new((i % 4) as u8 * MOD_CTRL, VK_A + i as u16);
            assert_eq!(reg.register_system(label, combo, "", 1), RegOutcome::Registered);
        }
        // 删除全部偶数下标条目。
        for i in (0..40usize).step_by(2) {
            assert!(reg.unregister(uid_of_label(labels[i]), 2));
        }
        for i in (1..40usize).step_by(2) {
            let combo = Combo::new((i % 4) as u8 * MOD_CTRL, VK_A + i as u16);
            assert_eq!(
                reg.dispatch(combo).map(|e| e.uid()),
                Some(uid_of_label(labels[i])),
                "后退移位后探测链断裂 i={i}"
            );
        }
        assert_eq!(reg.count, 20);
    }

    const LABELS: [&str; 40] = [
        "t00", "t01", "t02", "t03", "t04", "t05", "t06", "t07", "t08", "t09", "t10", "t11",
        "t12", "t13", "t14", "t15", "t16", "t17", "t18", "t19", "t20", "t21", "t22", "t23",
        "t24", "t25", "t26", "t27", "t28", "t29", "t30", "t31", "t32", "t33", "t34", "t35",
        "t36", "t37", "t38", "t39",
    ];

    #[test]
    fn combo_str_bare_win_and_unknown() {
        let mut buf = [0u8; 24];
        let n = combo_str(Combo::new(0, VK_LWIN), &mut buf);
        assert_eq!(core::str::from_utf8(&buf[..n]), Ok("Win"));
        let n2 = combo_str(Combo::new(MOD_WIN | MOD_CTRL, VK_F1 + 9), &mut buf);
        assert_eq!(core::str::from_utf8(&buf[..n2]), Ok("Ctrl+Win+F10"));
        let n3 = combo_str(Combo::new(0, 0xFF), &mut buf);
        assert_eq!(core::str::from_utf8(&buf[..n3]), Ok("?"));
    }

    #[test]
    fn import_rejects_malformed_buffers() {
        let mut reg = HotkeyReg::new();
        assert_eq!(reg.import_overrides(&[], 1), 0);
        assert_eq!(reg.import_overrides(b"VXHK", 1), 0);
        assert_eq!(reg.import_overrides(b"NOPE\x00\x00", 1), 0);
        // 声明条数与实际字节数不符。
        assert_eq!(reg.import_overrides(b"VXHK\x03\x00", 1), 0);
    }

    #[test]
    fn rebind_idempotent_and_none_such() {
        let mut reg = HotkeyReg::new();
        let _ = reg.preset_all(1);
        let uid = uid_of_label("sys.refresh");
        // 不存在的 uid。
        assert_eq!(reg.rebind(0xDEAD_BEEF, Combo::new(MOD_CTRL, 0x42), 2), RebindOutcome::NoSuchId);
        // 同键位重改 = 幂等 Done，不推进版本、不新增留痕（环内既有的
        // 出厂 Register 不算新增——对账口径以 change_log_len 零增量为准）。
        let v0 = reg.version;
        let len0 = reg.change_log_len();
        assert_eq!(reg.rebind(uid, Combo::new(0, VK_F1 + 4), 3), RebindOutcome::Done);
        assert_eq!(reg.version, v0);
        assert_eq!(reg.change_log_len(), len0, "同值改键不得新增留痕");
        // 纯修饰键（无键）→ Malformed。
        assert_eq!(reg.rebind(uid, Combo::new(MOD_CTRL, 0), 4), RebindOutcome::Malformed);
    }

    #[test]
    fn chord_timeout_and_fallthrough() {
        let mut g = HotkeyReg::new();
        let _ = g.preset_all(0);
        let f = Combo::new(MOD_CTRL | MOD_ALT, 0x4B);
        let s = Combo::new(MOD_CTRL | MOD_ALT, 0x4C);
        assert!(g.register_chord("app.demo", f, s, 0));
        // 窗内第二段 → Fired。
        assert_eq!(g.chord_key(f, 100), ChordStep::Armed);
        assert_eq!(g.chord_key(s, 500), ChordStep::Fired { label: "app.demo" });
        // 超时回落：窗外第二段不触发，回落为 Idle。
        assert_eq!(g.chord_key(f, 1000), ChordStep::Armed);
        assert_eq!(g.chord_key(s, 1000 + CHORD_TIMEOUT_MS + 1), ChordStep::Idle);
        assert!(g.chord_armed_label().is_none());
        // 回落后的第二段若是单键 → 正常派发（序列键不吞单键）。
        let copy_uid = uid_of_label("sys.copy");
        assert!(g.dispatch_logged(Combo::new(MOD_CTRL, 0x43), 2000).map_or(false, |e| e.uid() == copy_uid));
        // 主动复位面。
        let _ = g.chord_key(f, 3000);
        assert!(g.expire_chord());
        assert!(!g.expire_chord());
        assert!(g.audit_chord_no_collision());
    }

    #[test]
    fn unregister_app_purges_all_and_rehash_holds() {
        let mut g = HotkeyReg::new();
        let app = AppId::from_bytes(b"vxapp-many");
        // 同应用注册 6 条不同键位声明。
        for seq in 0..6u8 {
            let d = ParsedDecl {
                mods: MOD_ALT,
                key: 0x61 + (seq as u16) * 3, // Alt+J,Alt+M,...（避开预置键域）
                desc: DescBuf::from_bytes(b"bulk"),
            };
            assert_eq!(g.register_app_decl(app, &d, seq, seq as u64), RegOutcome::Registered);
        }
        assert_eq!(g.preset_all(10), PRESETS.len());
        let mut uids = [0u32; 8];
        let un = g.app_uids(&app, &mut uids);
        assert_eq!(un, 6);
        // 批量注销后：全部不可派发、表计数正确、审计双绿。
        assert_eq!(g.unregister_app(app, 20), 6);
        for &u in uids[..un].iter() {
            assert!(g.entry_by_uid(u).is_none());
        }
        assert_eq!(g.count, PRESETS.len());
        assert!(g.audit_conflict_free());
        assert!(g.audit_presets());
        // 注销面留痕可查（每条 uid 都有 Unregister 事件）。
        for &u in uids[..un].iter() {
            let chain = g.changes_for_uid(u);
            assert!(chain.iter().any(|c| c.kind == ChangeKind::Unregister));
        }
        // 再注册同应用同键位 → 重新占位成功（注销彻底、无残留墓碑）。
        let d = ParsedDecl { mods: MOD_ALT, key: 0x61, desc: DescBuf::from_bytes(b"again") };
        assert_eq!(g.register_app_decl(app, &d, 0, 30), RegOutcome::Registered);
    }

    #[test]
    fn hotkeyreg_selfcheck_all_green() {
        let set = run_hotkeyreg_checks();
        assert!(set.all_passed(), "F244 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 主册锚 F244（全局快捷键集中注册表）。v2 三件事：
// 1) 持久化 I/O：用户差异定长快照册（改键/禁用条目 → uid+键位+启用位）
//    v2 容器序列化——magic b"VXH1" + 版本 1 + 定长 payload + FNV-1a 校验
//    和，四类损坏显性拒绝（与既有 VXHK 变长差异面并存于追加段）；
// 2) UI 壳接线：设置中心快捷键页行清单（combo_str 显示串 + 启用/改键
//    徽标位）+ 行命中测试 + 键盘遍历；
// 3) 判定面扩展：run_hotkeyreg_v2_checks，首条即持久化 round-trip。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// 差异快照册容量（改键/禁用条目定容 12）。
pub const VX2_HK_SNAP_CAP: usize = 12;
/// v2 容器 payload 定长：条数 u32 + 12 槽×8B（uid u32 + mods + key u16 + flags）。
pub const VX2_HK_PAYLOAD: usize = 4 + VX2_HK_SNAP_CAP * 8;
/// v2 容器全长 = magic 4 + version 1 + payload + checksum 4。
pub const VX2_HK_BLOB: usize = 9 + VX2_HK_PAYLOAD;

/// v2 损坏分类（显性拒绝面——各归其名，不静默回默认）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vx2Error {
    BadMagic,
    BadVersion,
    /// 总长 ≠ 定长容器，或条数越出 payload 容量。
    BadLength,
    BadChecksum,
}

/// FNV-1a 32 位校验和（offset 0x811C9DC5、素数 0x01000193）。
fn vx2_fnv(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 一条差异快照（改键/禁用条目；uid 与既有持久化域一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HkSnap {
    pub uid: u32,
    pub mods: u8,
    pub key: u16,
    pub enabled: bool,
}

/// 用户差异定长快照册（启动装载面的 v2 承载）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HkSnapBook {
    pub entries: [Option<HkSnap>; VX2_HK_SNAP_CAP],
}

impl HkSnapBook {
    pub const fn new() -> HkSnapBook {
        HkSnapBook { entries: [None; VX2_HK_SNAP_CAP] }
    }

    /// 从注册表读出差异（overridden 或 disabled 的条目；超出容量显性
    /// 拒绝返回 None——不静默截断）。
    pub fn snapshot(reg: &HotkeyReg) -> Option<HkSnapBook> {
        let mut book = HkSnapBook::new();
        let mut n = 0usize;
        for i in 0..HK_CAP {
            if let Some(e) = reg.slot_at(i) {
                if e.overridden || !e.enabled {
                    if n >= VX2_HK_SNAP_CAP {
                        return None;
                    }
                    book.entries[n] = Some(HkSnap { uid: e.uid(), mods: e.combo.mods, key: e.combo.key, enabled: e.enabled });
                    n += 1;
                }
            }
        }
        Some(book)
    }

    /// 推到注册表：逐条走既有 rebind/set_enabled 正规门（冲突/未知 uid
    /// 如实跳过——返回实际生效条数，不静默吞差异）。
    pub fn apply_to(&self, reg: &mut HotkeyReg, ts: u64) -> usize {
        let mut applied = 0;
        for s in self.entries.iter().flatten() {
            let combo = Combo::new(s.mods, s.key);
            if !combo.sane() {
                continue;
            }
            if reg.rebind(s.uid, combo, ts) == RebindOutcome::Done && reg.set_enabled(s.uid, s.enabled, ts) {
                applied += 1;
            }
        }
        applied
    }

    /// 序列化：b"VXH1" + 版本 1 + 定长 payload + FNV-1a。缓冲不足返回 0。
    pub fn to_bytes(&self, out: &mut [u8]) -> usize {
        if out.len() < VX2_HK_BLOB {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXH1");
        out[4] = 1;
        let mut n = 0usize;
        for s in self.entries.iter().flatten() {
            let p = 9 + n * 8;
            out[p..p + 4].copy_from_slice(&s.uid.to_le_bytes());
            out[p + 4] = s.mods;
            out[p + 5..p + 7].copy_from_slice(&s.key.to_le_bytes());
            // 修障登记（F244v2 check 1 红）：flags 必须落在 p+7——此前误写
            // p+6，与 key u16 的高位字节重叠，enabled=1 会把 key=0x43 解码
            // 成 0x143（round-trip 字段不等，装载面改键到幽灵键位）。
            out[p + 7] = s.enabled as u8;
            out[p + 6] = 0;
            n += 1;
        }
        out[5..9].copy_from_slice(&(n as u32).to_le_bytes());
        let end = 9 + VX2_HK_PAYLOAD;
        let crc = vx2_fnv(&out[..end - 4]);
        out[end - 4..end].copy_from_slice(&crc.to_le_bytes());
        VX2_HK_BLOB
    }

    /// 反序列化：四类损坏显性拒绝。
    pub fn from_bytes(blob: &[u8]) -> Result<HkSnapBook, Vx2Error> {
        if blob.len() != VX2_HK_BLOB {
            return Err(Vx2Error::BadLength);
        }
        if blob[0..4] != *b"VXH1" {
            return Err(Vx2Error::BadMagic);
        }
        if blob[4] != 1 {
            return Err(Vx2Error::BadVersion);
        }
        let end = 9 + VX2_HK_PAYLOAD;
        let crc = u32::from_le_bytes([blob[end - 4], blob[end - 3], blob[end - 2], blob[end - 1]]);
        if vx2_fnv(&blob[..end - 4]) != crc {
            return Err(Vx2Error::BadChecksum);
        }
        let n = u32::from_le_bytes([blob[5], blob[6], blob[7], blob[8]]) as usize;
        if n > VX2_HK_SNAP_CAP {
            return Err(Vx2Error::BadLength);
        }
        let mut book = HkSnapBook::new();
        for k in 0..n {
            let p = 9 + k * 8;
            book.entries[k] = Some(HkSnap {
                uid: u32::from_le_bytes([blob[p], blob[p + 1], blob[p + 2], blob[p + 3]]),
                mods: blob[p + 4],
                key: u16::from_le_bytes([blob[p + 5], blob[p + 6]]),
                enabled: blob[p + 7] & 1 == 1,
            });
        }
        Ok(book)
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 快捷键页行高（px）——v2 布局常量：F244 设置页行 28px。
pub const VX2_ROW_H_PX: i32 = 28;
/// 显示串缓冲（与 combo_str 截断纪律一致）。
pub const VX2_TEXT_CAP: usize = 24;
/// 键盘遍历键码（VK_UP/VK_DOWN/VK_RETURN 同码）。
pub const VX2_KEY_UP: u8 = 0x26;
pub const VX2_KEY_DOWN: u8 = 0x27;
pub const VX2_KEY_ENTER: u8 = 0x0D;

/// 快捷键页行绘制条目：显示串 + 行矩形 + 启用/改键徽标位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HkRow {
    pub uid: u32,
    pub label: &'static str,
    pub y: i32,
    pub h: i32,
    pub enabled: bool,
    pub overridden: bool,
    pub text_len: usize,
    pub text: [u8; VX2_TEXT_CAP],
}

/// 生成快捷键页行清单（槽序 = 表序；显示串直取 combo_str——显示面
/// 唯一事实源，不另拼键名）。
pub fn hk_rows(reg: &HotkeyReg, out: &mut [HkRow]) -> usize {
    let mut k = 0usize;
    for i in 0..HK_CAP {
        if k >= out.len() {
            break;
        }
        if let Some(e) = reg.slot_at(i) {
            let mut row = HkRow {
                uid: e.uid(),
                label: e.label,
                y: k as i32 * VX2_ROW_H_PX,
                h: VX2_ROW_H_PX,
                enabled: e.enabled,
                overridden: e.overridden,
                text_len: 0,
                text: [0u8; VX2_TEXT_CAP],
            };
            row.text_len = combo_str(e.combo, &mut row.text);
            out[k] = row;
            k += 1;
        }
    }
    k
}

/// 行命中测试（列表内坐标；x ∈ [0, w) 且落在行内）。
pub fn hk_row_hit(rows: &[HkRow], n: usize, px: i32, py: i32, w: i32) -> Option<usize> {
    (0..n.min(rows.len())).find(|&k| px >= 0 && px < w && py >= rows[k].y && py < rows[k].y + rows[k].h)
}

/// 键盘遍历结论：不动 / 移高亮（夹取）/ 激活当前行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListNav {
    Stay,
    Moved(usize),
    Activate(usize),
}

/// 键盘遍历：Up/Down 移高亮（首末行夹取）、Enter 激活。
pub fn hk_list_nav(hl: usize, n: usize, key: u8) -> ListNav {
    if n == 0 {
        return ListNav::Stay;
    }
    match key {
        VX2_KEY_UP => ListNav::Moved(hl.saturating_sub(1)),
        VX2_KEY_DOWN => ListNav::Moved((hl + 1).min(n - 1)),
        VX2_KEY_ENTER => ListNav::Activate(hl.min(n - 1)),
        _ => ListNav::Stay,
    }
}

// -- 判定面扩展 ------------------------------------------------------------

/// F244 v2 自检（锚注见各条注释；首条 = 持久化 round-trip）。
pub fn run_hotkeyreg_v2_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("F244-hotkeyreg-v2");

    // 1. 持久化 round-trip：差异快照编→解→推新表→改键与禁用逐条一致。
    let mut src = HotkeyReg::new();
    let _ = src.preset_all(100);
    let _ = src.rebind(uid_of_label("sys.copy"), Combo::new(MOD_CTRL | MOD_SHIFT, 0x43), 200);
    let _ = src.set_enabled(uid_of_label("sys.undo"), false, 300);
    let book = match HkSnapBook::snapshot(&src) {
        Some(b) => b,
        None => {
            set.add("v2 persistence round-trip", false, "");
            set.add("v2 corruption explicitly rejected", false, "");
            set.add("v2 apply respects conflict gate", false, "");
            set.add("v2 rows display & nav", false, "");
            set.add("v2 fuzz 500 round-trips & checksum", false, "");
            return set;
        }
    };
    let mut buf = [0u8; VX2_HK_BLOB];
    let len = book.to_bytes(&mut buf);
    let mut dst = HotkeyReg::new();
    let _ = dst.preset_all(400);
    match HkSnapBook::from_bytes(&buf[..len]) {
        Ok(b2) => {
            let applied = b2.apply_to(&mut dst, 500);
            let copy_ok = dst
                .entry_by_uid(uid_of_label("sys.copy"))
                .map_or(false, |e| e.combo == Combo::new(MOD_CTRL | MOD_SHIFT, 0x43) && e.overridden);
            let undo_ok = dst.entry_by_uid(uid_of_label("sys.undo")).map_or(false, |e| !e.enabled);
            set.add(
                "v2 persistence round-trip",
                b2 == book && applied == 2 && copy_ok && undo_ok && dst.audit_conflict_free(),
                "",
            );
        }
        Err(_) => set.add("v2 persistence round-trip", false, ""),
    }

    // 2. 四类损坏显性拒绝（截断 / magic / 版本 / payload 翻位）。
    let mut m = buf;
    m[0] = b'X';
    let mut v = buf;
    v[4] = 9;
    let mut c = buf;
    c[12] ^= 0xFF;
    set.add(
        "v2 corruption explicitly rejected",
        HkSnapBook::from_bytes(&buf[..len - 1]) == Err(Vx2Error::BadLength)
            && HkSnapBook::from_bytes(&m) == Err(Vx2Error::BadMagic)
            && HkSnapBook::from_bytes(&v) == Err(Vx2Error::BadVersion)
            && HkSnapBook::from_bytes(&c) == Err(Vx2Error::BadChecksum),
        "",
    );

    // 3. 冲突门复核：快照键位撞已占用条目时 apply 显性跳过（先到先得
    //    同样适用于装载序——与 import_overrides 同纪律）。
    let mut hostile = HotkeyReg::new();
    let _ = hostile.preset_all(600);
    let _ = hostile.rebind(uid_of_label("sys.paste"), Combo::new(MOD_CTRL | MOD_SHIFT, 0x43), 700);
    let skipped = book.apply_to(&mut hostile, 800);
    set.add(
        "v2 apply respects conflict gate",
        skipped == 1
            && hostile.entry_by_uid(uid_of_label("sys.copy")).map_or(false, |e| e.combo == Combo::new(MOD_CTRL, 0x43))
            && hostile.audit_conflict_free(),
        "",
    );

    // 4. 页面行清单：全量行清单（46 预置；hk_rows 槽序=表序是文档契约，
    //    行内容不依赖槽位次序）——sys.copy 行显示串 "Ctrl+Shift+C"
    //    （改键后）+ 命中测试 + 键盘遍历夹取。
    let mut rows = [HkRow { uid: 0, label: "", y: 0, h: 0, enabled: false, overridden: false, text_len: 0, text: [0u8; VX2_TEXT_CAP] }; 48];
    let rn = hk_rows(&src, &mut rows);
    // 修障登记（v2 check 4 红）：hk_rows 按哈希槽序输出（表序=槽序是
    // 文档明示的契约），sys.copy 未必落首槽、甚至未必落在草稿的 8 行
    // 视窗内——行清单扩到全量并按 label 定位目标行；显示串、徽标、
    // 命中、遍历与首末夹取判据全部保留。
    let copy_row = (0..rn).find(|&k| rows[k].label == "sys.copy");
    let t0 = copy_row.map(|k| core::str::from_utf8(&rows[k].text[..rows[k].text_len]).unwrap_or("?"));
    let mut hl = 0usize;
    for _ in 0..9 {
        if let ListNav::Moved(k) = hk_list_nav(hl, rn, VX2_KEY_DOWN) {
            hl = k;
        }
    }
    set.add(
        "v2 rows display & nav",
        rn == 46
            && copy_row.is_some()
            && t0 == Some("Ctrl+Shift+C")
            && rows[copy_row.unwrap_or(0)].overridden
            && hk_row_hit(&rows, rn, 40, VX2_ROW_H_PX + 5, 600) == Some(1)
            && hk_row_hit(&rows, rn, 40, -3, 600).is_none()
            && hl == 9
            && hk_list_nav(rn - 1, rn, VX2_KEY_DOWN) == ListNav::Moved(rn - 1)
            && hk_list_nav(0, 1, VX2_KEY_ENTER) == ListNav::Activate(0),
        "",
    );

    // 5. xors32 fuzz 500 轮：随机差异册 round-trip 逐字段相等、payload
    //    任一字节翻位必被校验和捕获。
    let mut x: u32 = 0x244A_11CC;
    let mut ok = true;
    for _ in 0..500u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let mut b = HkSnapBook::new();
        for k in 0..((x >> 3) % 13) {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            b.entries[k as usize] = Some(HkSnap {
                uid: x,
                mods: (x % 16) as u8,
                key: (x % 0x100) as u16,
                enabled: x & 1 == 1,
            });
        }
        let mut tbuf = [0u8; VX2_HK_BLOB];
        ok &= b.to_bytes(&mut tbuf) == VX2_HK_BLOB && HkSnapBook::from_bytes(&tbuf) == Ok(b);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        tbuf[5 + (x as usize) % VX2_HK_PAYLOAD] ^= 0x20;
        ok &= HkSnapBook::from_bytes(&tbuf) == Err(Vx2Error::BadChecksum);
    }
    set.add("v2 fuzz 500 round-trips & checksum", ok, "");

    set
}

// ---------------------------------------------------------------------------
// v2 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_snapshot_roundtrip_and_reject() {
        let mut reg = HotkeyReg::new();
        let _ = reg.preset_all(1);
        let _ = reg.rebind(uid_of_label("sys.find"), Combo::new(MOD_ALT, 0x46), 2);
        let b = HkSnapBook::snapshot(&reg).expect("diff under cap");
        let mut buf = [0u8; VX2_HK_BLOB];
        assert_eq!(b.to_bytes(&mut buf), VX2_HK_BLOB);
        assert_eq!(HkSnapBook::from_bytes(&buf), Ok(b));
        let mut bad = buf;
        bad[9] ^= 0x01;
        assert_eq!(HkSnapBook::from_bytes(&bad), Err(Vx2Error::BadChecksum));
        assert_eq!(HkSnapBook::from_bytes(&buf[..9]), Err(Vx2Error::BadLength));
    }

    #[test]
    fn v2_rows_text_uses_combo_str() {
        let mut reg = HotkeyReg::new();
        let _ = reg.preset_all(1);
        // 修障登记：hk_rows 按哈希槽序输出，rows[0] 未必是 sys.copy、
        // sys.copy 也未必落在 4 行视窗内——缓冲扩到全量后按 label 定位
        // 再核对显示串（与 v2 check 4 同病同修）。
        let mut rows = [HkRow { uid: 0, label: "", y: 0, h: 0, enabled: false, overridden: false, text_len: 0, text: [0u8; VX2_TEXT_CAP] }; 48];
        let n = hk_rows(&reg, &mut rows);
        assert_eq!(n, PRESETS.len());
        let pos = (0..n).find(|&k| rows[k].label == "sys.copy").expect("sys.copy in rows");
        let s = core::str::from_utf8(&rows[pos].text[..rows[pos].text_len]).expect("combo str");
        assert_eq!(s, "Ctrl+C");
        for k in 1..n {
            assert!(rows[k].y >= rows[k - 1].y + rows[k - 1].h, "行矩形不得重叠");
        }
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_hotkeyreg_v2_checks();
        assert!(set.all_passed(), "F244 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
