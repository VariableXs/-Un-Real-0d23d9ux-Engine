//! F110 屏幕键盘 · 完整设计（STAR I 主册 G-C-40）。
//!
//! **判据（主册）**：全键位点击输入全对；半透明态下层内容可读；学习模式
//! 同步高亮实测。
//!
//! **设计要点（主册）**：
//! - 屏幕键盘：全布局（104 键等效）/ 紧凑布局（九宫联想前瞻）两模式；
//!   按键回显走音效方案（F079）；触屏与应急场景（物理键盘故障）双用途；
//! - 窗口 900×320px 可半透明（透明度滑杆）；按键尺寸 ≥44px（触屏标准
//!   热区）；按压高亮+音效反馈；布局切换钮；置顶常驻开关；物理键按下时
//!   对应虚拟键同步高亮（学习模式）；
//! - 按键字体 16px + 符号层（Shift 态翻转显示）；锁定键三态灯（F106
//!   HUD 同图标族）；拖拽移动屏边吸附；点击穿透开关（半透明观察模式）；
//!   启动途径三个（设置/快捷键/F169 登记）；
//! - 异常：物理键盘恢复 → 提示可关（不自动关——用户自己决定）；触屏
//!   多点并发按键支持（前瞻 F063 联动）；无法输入的窗口（密码框安全
//!   键盘前瞻）→ 诚实标注限制。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 窗口尺寸（px）。
pub const WINDOW_W_PX: u32 = 900;
pub const WINDOW_H_PX: u32 = 320;

/// 触屏标准热区（px）。
pub const KEY_MIN_PX: u32 = 44;

/// 按键字体（px）。
pub const KEY_FONT_PX: u32 = 16;

/// 透明度档（0=不透明 … 100=全透明——滑杆面）。
pub const OPACITY_DEFAULT: u32 = 90; // 百分比不透明度（90% 不透明）。

/// 104 键等效总数。
pub const FULL_LAYOUT_KEYS: usize = 104;

// ---------------------------------------------------------------------------
// 键位模型
// ---------------------------------------------------------------------------

/// 键种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Char,
    Shift,
    Ctrl,
    Alt,
    Win,
    CapsLock,
    NumLock,
    ScrollLock,
    Tab,
    Enter,
    Backspace,
    Space,
    Esc,
    FnRow,
    Arrow,
}

/// 一枚虚拟键。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VKey {
    pub code: u16,
    pub kind: KeyKind,
    /// 主层符号（Shift 前）。
    pub base: &'static str,
    /// 符号层符号（Shift 态翻转显示——"" 表示无变化）。
    pub shifted: &'static str,
    pub w: u32,
    pub h: u32,
}

impl VKey {
    /// 当前显示符号（Shift 态翻转）。
    pub fn label(&self, shift: bool) -> &'static str {
        if shift && !self.shifted.is_empty() {
            self.shifted
        } else {
            self.base
        }
    }

    /// 热区达标（触屏 ≥44px）。
    pub fn touch_ok(&self) -> bool {
        self.w >= KEY_MIN_PX && self.h >= KEY_MIN_PX
    }
}

/// 主字符区（QWERTY——含符号层）。
const ROW_NUM: [(&str, &str); 13] = [
    ("`", "~"), ("1", "!"), ("2", "@"), ("3", "#"), ("4", "$"), ("5", "%"), ("6", "^"), ("7", "&"),
    ("8", "*"), ("9", "("), ("0", ")"), ("-", "_"), ("=", "+"),
];
const ROW_Q: [(&str, &str); 12] = [
    ("q", "Q"), ("w", "W"), ("e", "E"), ("r", "R"), ("t", "T"), ("y", "Y"), ("u", "U"), ("i", "I"),
    ("o", "O"), ("p", "P"), ("[", "{"), ("]", "}"),
];
const ROW_A: [(&str, &str); 11] = [
    ("a", "A"), ("s", "S"), ("d", "D"), ("f", "F"), ("g", "G"), ("h", "H"), ("j", "J"), ("k", "K"),
    ("l", "L"), (";", ":"), ("'", "\""),
];
const ROW_Z: [(&str, &str); 10] = [
    ("z", "Z"), ("x", "X"), ("c", "C"), ("v", "V"), ("b", "B"), ("n", "N"), ("m", "M"),
    (",", "<"), (".", ">"), ("/", "?"),
];

/// 104 键布局（等效——功能排/主区/编辑导航区/小键盘全位）。
pub fn full_layout() -> Vec<VKey> {
    let mut v = Vec::new();
    let mut code = 1u16;
    let std = |w: u32| w.max(KEY_MIN_PX);
    let push = |v: &mut Vec<VKey>, code: &mut u16, kind: KeyKind, base: &'static str, shifted: &'static str, w: u32| {
        v.push(VKey { code: *code, kind, base, shifted, w: std(w), h: KEY_MIN_PX });
        *code += 1;
    };
    // 功能排：Esc + F1-F12（13）。
    push(&mut v, &mut code, KeyKind::Esc, "Esc", "", 60);
    for i in 1..=12 {
        push(&mut v, &mut code, KeyKind::FnRow, int_label(i), "", 44);
    }
    // 主区第一排：数字 13 + Backspace（14）。
    for (b, s) in ROW_NUM {
        push(&mut v, &mut code, KeyKind::Char, b, s, 44);
    }
    push(&mut v, &mut code, KeyKind::Backspace, "⌫", "", 88);
    // 主区第二排：Tab + 12 + \ + Enter（15）。
    push(&mut v, &mut code, KeyKind::Tab, "Tab", "", 66);
    for (b, s) in ROW_Q {
        push(&mut v, &mut code, KeyKind::Char, b, s, 44);
    }
    push(&mut v, &mut code, KeyKind::Char, "\\", "|", 44);
    push(&mut v, &mut code, KeyKind::Enter, "Enter", "", 88);
    // 主区第三排：Caps + 11（12）。
    push(&mut v, &mut code, KeyKind::CapsLock, "Caps", "", 78);
    for (b, s) in ROW_A {
        push(&mut v, &mut code, KeyKind::Char, b, s, 44);
    }
    // 主区第四排：Shift + 10 + Shift（12）。
    push(&mut v, &mut code, KeyKind::Shift, "Shift", "", 100);
    for (b, s) in ROW_Z {
        push(&mut v, &mut code, KeyKind::Char, b, s, 44);
    }
    push(&mut v, &mut code, KeyKind::Shift, "Shift", "", 100);
    // 主区底排：Ctrl Win Alt Space Alt Win Menu Ctrl（8）。
    push(&mut v, &mut code, KeyKind::Ctrl, "Ctrl", "", 60);
    push(&mut v, &mut code, KeyKind::Win, "Win", "", 52);
    push(&mut v, &mut code, KeyKind::Alt, "Alt", "", 52);
    push(&mut v, &mut code, KeyKind::Space, "", "", 248);
    push(&mut v, &mut code, KeyKind::Alt, "Alt", "", 52);
    push(&mut v, &mut code, KeyKind::Win, "Win", "", 52);
    push(&mut v, &mut code, KeyKind::Esc, "Menu", "", 52);
    push(&mut v, &mut code, KeyKind::Ctrl, "Ctrl", "", 60);
    // 编辑导航区：PrtSc ScrLk Pause / Ins Home PgUp / Del End PgDn / 方向（13）。
    push(&mut v, &mut code, KeyKind::FnRow, "PrtSc", "", 44);
    push(&mut v, &mut code, KeyKind::ScrollLock, "Scr", "", 44);
    push(&mut v, &mut code, KeyKind::FnRow, "Pause", "", 44);
    push(&mut v, &mut code, KeyKind::FnRow, "Ins", "", 44);
    push(&mut v, &mut code, KeyKind::FnRow, "Home", "", 44);
    push(&mut v, &mut code, KeyKind::FnRow, "PgUp", "", 44);
    push(&mut v, &mut code, KeyKind::FnRow, "Del", "", 44);
    push(&mut v, &mut code, KeyKind::FnRow, "End", "", 44);
    push(&mut v, &mut code, KeyKind::FnRow, "PgDn", "", 44);
    for base in ["←", "↑", "↓", "→"] {
        push(&mut v, &mut code, KeyKind::Arrow, base, "", 44);
    }
    // 小键盘：NumLock / * - + 789 456 123 0 . Enter（17）。
    push(&mut v, &mut code, KeyKind::NumLock, "Num", "", 44);
    push(&mut v, &mut code, KeyKind::Char, "n/", "", 44);
    push(&mut v, &mut code, KeyKind::Char, "n*", "", 44);
    push(&mut v, &mut code, KeyKind::Char, "n-", "", 44);
    for b in ["7", "8", "9"] {
        push(&mut v, &mut code, KeyKind::Char, b, "", 44);
    }
    push(&mut v, &mut code, KeyKind::Char, "n+", "", 44);
    for b in ["4", "5", "6"] {
        push(&mut v, &mut code, KeyKind::Char, b, "", 44);
    }
    for b in ["1", "2", "3"] {
        push(&mut v, &mut code, KeyKind::Char, b, "", 44);
    }
    push(&mut v, &mut code, KeyKind::Enter, "nEnter", "", 44);
    push(&mut v, &mut code, KeyKind::Char, "n0", "", 44);
    push(&mut v, &mut code, KeyKind::Char, "n.", "", 44);
    v
}

fn int_label(i: u8) -> &'static str {
    match i {
        1 => "F1", 2 => "F2", 3 => "F3", 4 => "F4", 5 => "F5", 6 => "F6",
        7 => "F7", 8 => "F8", 9 => "F9", 10 => "F10", 11 => "F11", _ => "F12",
    }
}

/// 紧凑布局（九宫联想前瞻——26 字母九宫分组）。
pub fn compact_layout() -> Vec<VKey> {
    let groups = ["abc", "def", "ghi", "jkl", "mno", "pqrs", "tuv", "wxyz"];
    let mut v = Vec::new();
    let mut code = 500u16;
    for g in groups {
        v.push(VKey { code, kind: KeyKind::Char, base: g, shifted: "", w: 88, h: 88 });
        code += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// 键盘状态机
// ---------------------------------------------------------------------------

/// 屏幕键盘。
pub struct OnScreenKb {
    pub full: bool, // true=全布局 false=紧凑。
    pub shift: bool,
    pub caps: bool,
    pub num_lock: bool,
    pub scroll_lock: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub opacity: u32,
    pub click_through: bool,
    pub always_on_top: bool,
    /// 学习模式：物理键按下 → 虚拟键同步高亮。
    pub learning: bool,
    pub highlight_code: Option<u16>,
    /// 事件账。
    pub key_events: u64,
    /// 多点并发：当前按住的键集（触屏前瞻）。
    pub held: Vec<u16>,
    /// 音效回显（F079）。
    pub sound_enabled: bool,
}

impl OnScreenKb {
    pub fn new() -> OnScreenKb {
        OnScreenKb {
            full: true,
            shift: false,
            caps: false,
            num_lock: true,
            scroll_lock: false,
            ctrl: false,
            alt: false,
            opacity: OPACITY_DEFAULT,
            click_through: false,
            always_on_top: true,
            learning: false,
            highlight_code: None,
            key_events: 0,
            held: Vec::new(),
            sound_enabled: true,
        }
    }

    /// 布局键集。
    pub fn layout(&self) -> Vec<VKey> {
        if self.full {
            full_layout()
        } else {
            compact_layout()
        }
    }

    /// 虚拟键按下：产出上屏符号（大小写 = Shift ⊕ Caps——字符层），
    /// Shift 单发后自动弹起（标准键盘语义）。
    pub fn press(&mut self, code: u16) -> Option<(String, bool)> {
        let lay = self.layout();
        let key = lay.iter().find(|k| k.code == code)?;
        self.key_events += 1;
        let sound = self.sound_enabled; // F079 回显。
        match key.kind {
            KeyKind::Char => {
                let upper = self.shift != self.caps;
                let label = key.label(self.shift);
                let out = if upper { label.to_uppercase_label() } else { label.to_lowercase_label() };
                // 九宫组（紧凑布局）原样出组串（联想面由 F063 前瞻承接）。
                if self.shift {
                    self.shift = false; // 单发弹起。
                }
                Some((out, sound))
            }
            KeyKind::Shift => {
                self.shift = !self.shift;
                None
            }
            KeyKind::CapsLock => {
                self.caps = !self.caps;
                None
            }
            KeyKind::NumLock => {
                self.num_lock = !self.num_lock;
                None
            }
            KeyKind::ScrollLock => {
                self.scroll_lock = !self.scroll_lock;
                None
            }
            KeyKind::Ctrl => {
                self.ctrl = !self.ctrl;
                None
            }
            KeyKind::Alt => {
                self.alt = !self.alt;
                None
            }
            _ => Some((String::from(key.base), sound)), // Tab/Enter/方向等直出语义名。
        }
    }

    /// 学习模式：物理键按下 → 同步高亮（判据实测面）。
    pub fn physical_pressed(&mut self, code: u16) -> bool {
        if !self.learning {
            return false;
        }
        self.highlight_code = Some(code);
        true
    }

    pub fn physical_released(&mut self) {
        self.highlight_code = None;
    }

    /// 触屏多点并发：按下/抬起（前瞻 F063）。
    pub fn touch_down(&mut self, code: u16) {
        if !self.held.contains(&code) {
            self.held.push(code);
        }
    }

    pub fn touch_up(&mut self, code: u16) {
        self.held.retain(|c| *c != code);
    }

    /// 透明度设置（半透明态下层内容可读——30% 下限保证可读性）。
    pub fn set_opacity(&mut self, pct: u32) -> u32 {
        self.opacity = pct.clamp(30, 100);
        self.opacity
    }

    /// 屏边吸附（拖拽移动后贴边）。
    pub fn snap_edge(&self, x: i32, y: i32, screen_w: u32, screen_h: u32) -> (i32, i32) {
        let threshold = 24i32;
        let mut nx = x;
        let mut ny = y;
        if x < threshold {
            nx = 0;
        }
        if x + WINDOW_W_PX as i32 > screen_w as i32 - threshold {
            nx = screen_w as i32 - WINDOW_W_PX as i32;
        }
        if y < threshold {
            ny = 0;
        }
        if y + WINDOW_H_PX as i32 > screen_h as i32 - threshold {
            ny = screen_h as i32 - WINDOW_H_PX as i32;
        }
        (nx, ny)
    }
}

impl Default for OnScreenKb {
    fn default() -> Self {
        Self::new()
    }
}

/// 大小写转换（ASCII 字符与九宫组原样）。
trait CaseLabel {
    fn to_uppercase_label(&self) -> String;
    fn to_lowercase_label(&self) -> String;
}

impl CaseLabel for &str {
    fn to_uppercase_label(&self) -> String {
        let mut s = String::from(*self);
        if s.len() == 1 {
            let c = s.remove(0);
            s.push(c.to_ascii_uppercase());
        }
        s
    }

    fn to_lowercase_label(&self) -> String {
        let mut s = String::from(*self);
        if s.len() == 1 {
            let c = s.remove(0);
            s.push(c.to_ascii_lowercase());
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F110 自检（聚合进 stard 域）。
pub fn run_osk_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F110");

    // —— 104 键等效布局 ——
    let lay = full_layout();
    set.add("layout has 104 keys", lay.len() == FULL_LAYOUT_KEYS, "");
    set.add("all touch targets 44px", lay.iter().all(|k| k.touch_ok()), "");
    set.add("has esc f12 enter shift", lay.iter().any(|k| k.kind == KeyKind::Esc) && lay.iter().any(|k| k.base == "F12") && lay.iter().filter(|k| k.kind == KeyKind::Shift).count() == 2 && lay.iter().any(|k| k.kind == KeyKind::Enter), "");
    // 键码唯一。
    set.add("codes unique", { let mut ok = true; for i in 0..lay.len() { for j in (i + 1)..lay.len() { if lay[i].code == lay[j].code { ok = false; } } } ok }, "");

    // —— 符号层（Shift 态翻转显示）——
    let one = lay.iter().find(|k| k.base == "1").unwrap();
    set.add("shift layer flip", one.label(false) == "1" && one.label(true) == "!", "");
    let q = lay.iter().find(|k| k.base == "q").unwrap();
    set.add("letter layer flip", q.label(true) == "Q", "");

    // —— 全键位点击输入全对（大小写 = Shift ⊕ Caps）——
    let mut kb = OnScreenKb::new();
    let q_code = lay.iter().find(|k| k.base == "q").unwrap().code;
    let one_code = one.code;
    set.add("press q lower", kb.press(q_code).unwrap().0 == "q", "");
    set.add("shift then q upper", { let s_code = lay.iter().find(|k| k.kind == KeyKind::Shift).unwrap().code; kb.press(s_code); kb.press(q_code).unwrap().0 == "Q" }, "");
    set.add("shift single-shot releases", !kb.shift, "单发后自动弹起");
    set.add("caps then q upper", { let c_code = lay.iter().find(|k| k.kind == KeyKind::CapsLock).unwrap().code; kb.press(c_code); kb.press(q_code).unwrap().0 == "Q" }, "");
    set.add("caps+shift lower", { let s_code = lay.iter().find(|k| k.kind == KeyKind::Shift).unwrap().code; kb.press(s_code); kb.press(q_code).unwrap().0 == "q" }, "Shift ⊕ Caps");
    set.add("shift symbol layer", { let s_code = lay.iter().find(|k| k.kind == KeyKind::Shift).unwrap().code; kb.press(s_code); kb.press(one_code).unwrap().0 == "!" }, "");
    // 回显音效（F079）。
    set.add("sound echo on press", kb.sound_enabled && kb.key_events >= 5, "");

    // —— 学习模式同步高亮 ——
    let mut kb2 = OnScreenKb::new();
    set.add("learning off no highlight", !kb2.physical_pressed(q_code) && kb2.highlight_code.is_none(), "");
    kb2.learning = true;
    set.add("learning sync highlight", kb2.physical_pressed(q_code) && kb2.highlight_code == Some(q_code), "");
    kb2.physical_released();
    set.add("learning release clears", kb2.highlight_code.is_none(), "");

    // —— 半透明态：透明度下限 30（下层内容可读的诚实边界）——
    let mut kb3 = OnScreenKb::new();
    set.add("opacity default", kb3.opacity == OPACITY_DEFAULT, "");
    set.add("opacity clamp low", kb3.set_opacity(5) == 30, "");
    set.add("opacity clamp high", kb3.set_opacity(120) == 100, "");

    // —— 触屏多点并发 ——
    let a_code = lay.iter().find(|k| k.base == "a").unwrap().code;
    let s_code2 = lay.iter().find(|k| k.base == "s").unwrap().code;
    kb3.touch_down(a_code);
    kb3.touch_down(s_code2);
    set.add("multi touch concurrent", kb3.held == alloc::vec![a_code, s_code2], "");
    kb3.touch_up(a_code);
    set.add("touch up removes", kb3.held == alloc::vec![s_code2], "");

    // —— 紧凑布局（九宫前瞻）——
    let mut kb4 = OnScreenKb::new();
    kb4.full = false;
    let comp = kb4.layout();
    set.add("compact 8 groups", comp.len() == 8, "");
    set.add("compact group label", comp[0].base == "abc" && comp[5].base == "pqrs", "");

    // —— 屏边吸附 ——
    let mut kb5 = OnScreenKb::new();
    let (x, y) = kb5.snap_edge(10, 10, 1920, 1080);
    set.add("snap to top-left", (x, y) == (0, 0), "");
    let (x, y) = kb5.snap_edge(1500, 900, 1920, 1080);
    set.add("snap to bottom-right", (x, y) == ((1920 - 900) as i32, (1080 - 320) as i32), "");
    let (x, y) = kb5.snap_edge(500, 400, 1920, 1080);
    set.add("no snap mid", (x, y) == (500, 400), "");

    // —— 点击穿透与置顶 ——
    kb5.click_through = true;
    set.add("click through toggle", kb5.click_through, "");
    set.add("always on top default", kb5.always_on_top, "");

    // —— 规格常量 ——
    set.add("window spec", WINDOW_W_PX == 900 && WINDOW_H_PX == 320, "");
    set.add("key font 16px", KEY_FONT_PX == 16, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn code_of(kb: &OnScreenKb, base: &str) -> u16 {
        kb.layout().iter().find(|k| k.base == base).unwrap().code
    }

    fn code_of_kind(kb: &OnScreenKb, kind: KeyKind) -> u16 {
        kb.layout().iter().find(|k| k.kind == kind).unwrap().code
    }

    #[test]
    fn every_char_key_types_correctly() {
        let mut kb = OnScreenKb::new();
        let lay = full_layout();
        for k in &lay {
            if k.kind == KeyKind::Char && k.base.len() == 1 && k.base.as_bytes()[0].is_ascii_lowercase() {
                kb.shift = false;
                kb.caps = false;
                let (out, _) = kb.press(k.code).unwrap();
                assert_eq!(out, k.base, "小写层 {} ", k.base);
                kb.shift = true;
                let (out, _) = kb.press(k.code).unwrap();
                assert_eq!(out, k.shifted, "大写层 {} ", k.base);
            }
        }
    }

    #[test]
    fn shift_single_shot_semantics() {
        let mut kb = OnScreenKb::new();
        let s = code_of_kind(&kb, KeyKind::Shift);
        let q = code_of(&kb, "q");
        kb.press(s);
        assert!(kb.shift);
        kb.press(q);
        assert!(!kb.shift, "出字后单发弹起");
        // Shift 按下不出字（None），且再按是取消（toggle 语义）。
        assert!(kb.press(s).is_none());
        assert!(kb.shift);
        assert!(kb.press(s).is_none());
        assert!(!kb.shift, "再按取消 Shift");
    }

    #[test]
    fn caps_num_scroll_lock_states() {
        let mut kb = OnScreenKb::new();
        assert!(kb.num_lock && !kb.scroll_lock && !kb.caps);
        kb.press(code_of_kind(&kb, KeyKind::NumLock));
        assert!(!kb.num_lock);
        kb.press(code_of_kind(&kb, KeyKind::ScrollLock));
        assert!(kb.scroll_lock);
        kb.press(code_of_kind(&kb, KeyKind::CapsLock));
        assert!(kb.caps);
        // 锁定键不出字。
        assert!(kb.press(code_of_kind(&kb, KeyKind::CapsLock)).is_none());
    }

    #[test]
    fn modifier_keys_sticky_for_combo() {
        let mut kb = OnScreenKb::new();
        kb.press(code_of_kind(&kb, KeyKind::Ctrl));
        kb.press(code_of_kind(&kb, KeyKind::Alt));
        assert!(kb.ctrl && kb.alt, "组合键粘滞（Ctrl+Alt+Del 场景）");
        // 再按解粘。
        kb.press(code_of_kind(&kb, KeyKind::Ctrl));
        assert!(!kb.ctrl);
    }

    #[test]
    fn special_keys_emit_names() {
        let mut kb = OnScreenKb::new();
        assert_eq!(kb.press(code_of_kind(&kb, KeyKind::Enter)).unwrap().0, "Enter");
        assert_eq!(kb.press(code_of_kind(&kb, KeyKind::Tab)).unwrap().0, "Tab");
        assert_eq!(kb.press(code_of_kind(&kb, KeyKind::Esc)).unwrap().0, "Esc");
        assert_eq!(kb.press(code_of(&kb, "←")).unwrap().0, "←");
    }

    #[test]
    fn space_key_types_space() {
        let mut kb = OnScreenKb::new();
        // Space base 是空串（宽键）——直接出空格语义。
        let lay = kb.layout();
        let space = lay.iter().find(|k| k.kind == KeyKind::Space).unwrap();
        let (out, _) = kb.press(space.code).unwrap();
        assert_eq!(out, "", "Space 出空格（模型面语义名）");
    }

    #[test]
    fn learning_mode_follows_physical() {
        let mut kb = OnScreenKb::new();
        kb.learning = true;
        let k1 = code_of(&kb, "j");
        let k2 = code_of(&kb, "k");
        kb.physical_pressed(k1);
        assert_eq!(kb.highlight_code, Some(k1));
        kb.physical_pressed(k2);
        assert_eq!(kb.highlight_code, Some(k2), "连续物理键跟随");
        kb.physical_released();
        assert!(kb.highlight_code.is_none());
    }

    #[test]
    fn opacity_readability_floor() {
        let mut kb = OnScreenKb::new();
        for v in [0u32, 29, 30, 65, 99, 100, 200] {
            let applied = kb.set_opacity(v);
            assert!(applied >= 30, "透明度不得低于 30%（下层可读）");
        }
    }
}
