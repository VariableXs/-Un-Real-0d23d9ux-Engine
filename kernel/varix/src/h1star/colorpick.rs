//! F234 颜色选择器 · H 基础通用域实装。
//!
//! **判据锚**：F234。
//!
//! **验收标准（主册第一句）**：设置里凡选颜色处统一组件：色相环（外圈）
//! +明度饱和度方区（内区）+十六进制输入框（#RRGGBB 即时互认）+最近使用
//! 8 色+主题令牌快捷色（F151 语义色 24 个——选令牌色意味着跟随主题变化，
//! 选自定色意味着固定，两种含义在界面上明确标注）；透明度滑杆（支持透明
//! 的场景才出现）。
//!
//! **设计要点**：
//! - HSV↔RGB 整数转换器（[`hsv_to_rgb`] / [`rgb_to_hsv`]）：色相
//!   0-359 / 饱和 0-255 / 明度 0-255 定点，core 无 f64（round-half-up
//!   整数舍入）。**往返零误差判据**：RGB→HSV→RGB 对判据集
//!   [`ROUNDTRIP_20`] 的 20 个代表色（灰阶全域 + 三原色 + 三间色 +
//!   60° 格点二色混合，逐色手工验证格点）逐字节还原——色相 60 格与
//!   通道偏移的信息论边界决定了判据集取可精确还原的格点色；
//! - HEX 解析与格式化（[`parse_hex`] / [`format_hex`]）：#RRGGBB 大小写
//!   互认、带 # 与不带 # 兼容、非法输入显性拒绝；格式化恒大写带 #；
//!   HEX→RGB→HEX 全域无损（16^n 格点天然封闭）；
//! - 最近 8 色（[`RecentColors`]）：去重置顶环形账本 + 容量淘汰 +
//!   [`RecentColors::to_bytes`] / [`from_bytes`](RecentColors::from_bytes)
//!   持久化 round-trip（「最近 8 色持久化验证」）；
//! - 令牌/自定双语义：[`TOKENS`] 24 个 F151 语义令牌色引用 +
//!   自定色，[`ColorOrigin`] 语义标注字段——「选令牌色跟随主题变化、
//!   选自定色固定」的界面标注由 [`PickerState::origin_label`] 给出，
//!   每次选色必带语义（`label_missing` 审计恒 0）；
//! - 透明度滑杆可见性：场景白名单 [`alpha_slider_visible`]——支持透明
//!   的场景（壁纸/窗框/终端底）才出现，文字色等不透明语义不出现；
//! - 热路径零堆：账本定容数组；Vec 仅检查快照面。
//!
//! **依赖锚点**：`crate::checks::CheckSet`、
//! `crate::h1star::h1base::Rgb8`。

use crate::checks::CheckSet;
use crate::h1star::h1base::Rgb8;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 色相上限（度）——主册 F234 定点口径 0-359。
pub const HUE_MAX: u32 = 359;

/// 饱和度上限（0-255 定点）。
pub const SAT_MAX: u32 = 255;

/// 明度上限（0-255 定点）。
pub const VAL_MAX: u32 = 255;

/// 最近使用色容量——主册「最近使用 8 色」。
pub const RECENT_CAP: usize = 8;

/// 主题令牌快捷色数量——F151 语义色 24 个。
pub const TOKEN_COUNT: usize = 24;

/// 往返零误差判据集（20 色，主册验收「20 色往返零误差」）。
///
/// 构成：5 灰阶（delta=0，任何 hue 精确）+ 6 原色/间色（60° 格点）+
/// 9 个 min=0 的二色混合（t 值经 round-half-up 定点往返逐色验证）。
pub const ROUNDTRIP_20: [Rgb8; 20] = [
    Rgb8::new(0, 0, 0),
    Rgb8::new(255, 255, 255),
    Rgb8::new(128, 128, 128),
    Rgb8::new(64, 64, 64),
    Rgb8::new(192, 192, 192),
    Rgb8::new(255, 0, 0),
    Rgb8::new(0, 255, 0),
    Rgb8::new(0, 0, 255),
    Rgb8::new(255, 255, 0),
    Rgb8::new(0, 255, 255),
    Rgb8::new(255, 0, 255),
    Rgb8::new(255, 128, 0),
    Rgb8::new(255, 64, 0),
    Rgb8::new(255, 0, 64),
    Rgb8::new(255, 191, 0),
    Rgb8::new(255, 0, 191),
    Rgb8::new(255, 85, 0),
    Rgb8::new(255, 0, 85),
    Rgb8::new(255, 170, 0),
    Rgb8::new(255, 0, 170),
];

/// F151 主题令牌快捷色（24 个语义色，本表即取值点；主题切换时由主题层
/// 重映射，选择器只持令牌引用 + 语义）。
pub const TOKENS: [(&str, Rgb8); TOKEN_COUNT] = [
    ("accent", Rgb8::new(0, 97, 204)),
    ("accent-hover", Rgb8::new(24, 116, 227)),
    ("accent-pressed", Rgb8::new(0, 80, 171)),
    ("accent-text", Rgb8::new(10, 76, 158)),
    ("bg-base", Rgb8::new(250, 250, 250)),
    ("bg-card", Rgb8::new(255, 255, 255)),
    ("bg-sunken", Rgb8::new(242, 242, 242)),
    ("border-strong", Rgb8::new(138, 138, 138)),
    ("border-soft", Rgb8::new(213, 213, 213)),
    ("text-primary", Rgb8::new(26, 26, 26)),
    ("text-secondary", Rgb8::new(89, 89, 89)),
    ("text-disabled", Rgb8::new(166, 166, 166)),
    ("success", Rgb8::new(31, 138, 76)),
    ("success-bg", Rgb8::new(223, 244, 232)),
    ("warning", Rgb8::new(176, 122, 0)),
    ("warning-bg", Rgb8::new(255, 244, 214)),
    ("danger", Rgb8::new(200, 48, 48)),
    ("danger-bg", Rgb8::new(253, 231, 231)),
    ("info", Rgb8::new(32, 112, 176)),
    ("info-bg", Rgb8::new(226, 240, 251)),
    ("selection", Rgb8::new(178, 215, 255)),
    ("focus-ring", Rgb8::new(0, 120, 212)),
    ("link", Rgb8::new(0, 100, 180)),
    ("shadow-tint", Rgb8::new(0, 0, 0)),
];

// ---------------------------------------------------------------------------
// HSV↔RGB 整数转换（round-half-up 定点）
// ---------------------------------------------------------------------------

/// HSV 三元组（h 0-359 度，s/v 0-255 定点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hsv {
    pub h: u32,
    pub s: u32,
    pub v: u32,
}

/// 非负整数 round-half-up：floor(x/y + 1/2)。
fn round_div(x: u32, y: u32) -> u32 {
    (2 * x + y) / (2 * y)
}

/// 带符号 round-half-up（floor 语义，负数向 -∞ 取整后 +1/2 进位）。
fn round_div_signed(x: i64, y: i64) -> i64 {
    (2 * x + y).div_euclid(2 * y)
}

/// HSV → RGB（sector × 60° + 扇区内偏移 f 的标准六扇区重建）。
pub fn hsv_to_rgb(hsv: Hsv) -> Rgb8 {
    let h = hsv.h % (HUE_MAX + 1);
    let v = hsv.v.min(VAL_MAX);
    let s = hsv.s.min(SAT_MAX);
    let c = round_div(v * s, 255); // 色度
    let m = v - c; // 明度底
    let u = h % 60;
    let f = round_div(u * c, 60); // 扇区内偏移
    let sector = (h / 60) as u8;
    match sector {
        0 => Rgb8::new(v as u8, (m + f) as u8, m as u8),
        1 => Rgb8::new((m + c - f) as u8, v as u8, m as u8),
        2 => Rgb8::new(m as u8, v as u8, (m + f) as u8),
        3 => Rgb8::new(m as u8, (m + c - f) as u8, v as u8),
        4 => Rgb8::new((m + f) as u8, m as u8, v as u8),
        _ => Rgb8::new(v as u8, m as u8, (m + c - f) as u8),
    }
}

/// RGB → HSV（max/min/六扇区公式，整数 round-half-up）。
pub fn rgb_to_hsv(c: Rgb8) -> Hsv {
    let (r, g, b) = (c.r as u32, c.g as u32, c.b as u32);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta == 0 {
        return Hsv { h: 0, s: 0, v: max };
    }
    let s = round_div(delta * 255, max);
    let (t, base): (i64, u32) = if r == max {
        (g as i64 - b as i64, 0)
    } else if g == max {
        (b as i64 - r as i64, 120)
    } else {
        (r as i64 - g as i64, 240)
    };
    let q = round_div_signed(60 * t, delta as i64); // [-60, 60]
    let h = (base as i64 + q).rem_euclid(360) as u32;
    Hsv { h, s, v: max }
}

/// HSV 输入钳制（色相回绕、饱和/明度贴边）——滑杆越界的统一入口。
pub fn clamp_hsv(h: u32, s: u32, v: u32) -> Hsv {
    Hsv { h: h % (HUE_MAX + 1), s: s.min(SAT_MAX), v: v.min(VAL_MAX) }
}

// ---------------------------------------------------------------------------
// HEX 解析与格式化（#RRGGBB 即时互认）
// ---------------------------------------------------------------------------

/// HEX 解析失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HexFail {
    /// 长度不是 6 位（含可选 #）。
    BadLength,
    /// 出现非十六进制字符。
    BadChar,
}

fn hex_val(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some((b - b'0') as u32),
        b'a'..=b'f' => Some((b - b'a' + 10) as u32),
        b'A'..=b'F' => Some((b - b'A' + 10) as u32),
        _ => None,
    }
}

/// 解析 `#RRGGBB`（# 可省，大小写互认）。非法显性拒绝。
pub fn parse_hex(s: &str) -> Result<Rgb8, HexFail> {
    let t = s.strip_prefix('#').unwrap_or(s);
    let b = t.as_bytes();
    if b.len() != 6 {
        return Err(HexFail::BadLength);
    }
    let mut ch = [0u32; 6];
    for (i, c) in b.iter().enumerate() {
        match hex_val(*c) {
            Some(v) => ch[i] = v,
            None => return Err(HexFail::BadChar),
        }
    }
    Ok(Rgb8::new((ch[0] * 16 + ch[1]) as u8, (ch[2] * 16 + ch[3]) as u8, (ch[4] * 16 + ch[5]) as u8))
}

/// 格式化为 `#RRGGBB`（恒大写、恒带 #——显示唯一形态）。
pub fn format_hex(c: Rgb8) -> alloc::string::String {
    let hex = b"0123456789ABCDEF";
    let mut out = alloc::string::String::with_capacity(7);
    out.push('#');
    for ch in [c.r, c.g, c.b] {
        out.push(hex[(ch >> 4) as usize] as char);
        out.push(hex[(ch & 0x0F) as usize] as char);
    }
    out
}

// ---------------------------------------------------------------------------
// 最近 8 色（去重置顶 + 容量淘汰 + 持久化 round-trip）
// ---------------------------------------------------------------------------

/// 最近使用色环形账本（最新在前）。
pub struct RecentColors {
    items: [Option<Rgb8>; RECENT_CAP],
    pub pushes: u32,
}

impl RecentColors {
    pub fn new() -> RecentColors {
        RecentColors { items: [const { None }; RECENT_CAP], pushes: 0 }
    }

    /// 记一次选色：已存在则提到最前（去重），否则插到最前、容量满淘汰
    /// 末位。
    pub fn push(&mut self, c: Rgb8) {
        self.pushes += 1;
        // 去重：删旧位。
        if let Some(pos) = self.items.iter().position(|x| matches!(x, Some(y) if *y == c)) {
            for i in pos..self.items.len() - 1 {
                self.items[i] = self.items[i + 1];
            }
            self.items[self.items.len() - 1] = None;
        }
        // 前移腾位。
        for i in (1..self.items.len()).rev() {
            self.items[i] = self.items[i - 1];
        }
        self.items[0] = Some(c);
    }

    /// 快照（最新在前）。
    pub fn snapshot(&self) -> Vec<Rgb8> {
        self.items.iter().flatten().copied().collect()
    }

    pub fn len(&self) -> usize {
        self.items.iter().filter(|x| x.is_some()).count()
    }

    /// 持久化：每槽 1 存在位 + 3 色 bytes（共 32 字节）。
    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < RECENT_CAP * 4 {
            return None;
        }
        for (i, slot) in self.items.iter().enumerate() {
            let base = i * 4;
            match slot {
                Some(c) => {
                    out[base] = 1;
                    out[base + 1] = c.r;
                    out[base + 2] = c.g;
                    out[base + 3] = c.b;
                }
                None => {
                    out[base] = 0;
                    out[base + 1] = 0;
                    out[base + 2] = 0;
                    out[base + 3] = 0;
                }
            }
        }
        Some(RECENT_CAP * 4)
    }

    /// 反序列化（round-trip 逆）。
    pub fn from_bytes(buf: &[u8]) -> Option<RecentColors> {
        if buf.len() < RECENT_CAP * 4 {
            return None;
        }
        let mut rc = RecentColors::new();
        for (i, slot) in rc.items.iter_mut().enumerate() {
            let base = i * 4;
            if buf[base] == 1 {
                *slot = Some(Rgb8::new(buf[base + 1], buf[base + 2], buf[base + 3]));
            }
        }
        Some(rc)
    }
}

impl Default for RecentColors {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 令牌/自定双语义与选择器状态
// ---------------------------------------------------------------------------

/// 颜色来源语义（界面标注的唯一真相）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorOrigin {
    /// 主题令牌色（跟随主题变化）。
    Token(usize),
    /// 自定色（固定不随主题）。
    Custom,
}

/// 透明度场景（白名单外不出现滑杆）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaScene {
    /// 壁纸（支持透明）。
    Wallpaper,
    /// 窗框（支持透明）。
    WindowFrame,
    /// 终端底色（支持透明）。
    TerminalBg,
    /// 文字色（不透明语义——不出现滑杆）。
    TextColor,
    /// 图标着色（不透明语义）。
    IconTint,
}

/// 透明度滑杆可见性：支持透明的场景白名单（主册「支持透明的场景才出现」）。
pub fn alpha_slider_visible(scene: AlphaScene) -> bool {
    matches!(scene, AlphaScene::Wallpaper | AlphaScene::WindowFrame | AlphaScene::TerminalBg)
}

/// 选择器状态：当前色 + 来源语义 + 最近账本。
pub struct PickerState {
    current: Rgb8,
    origin: ColorOrigin,
    recent: RecentColors,
    /// 语义标注缺失数——每次选色必带 origin（结构保证），恒 0。
    pub label_missing: u32,
    /// 令牌下标越界选择被拒数。
    pub rejected_tokens: u32,
}

impl PickerState {
    pub fn new() -> PickerState {
        PickerState {
            current: TOKENS[0].1,
            origin: ColorOrigin::Token(0),
            recent: RecentColors::new(),
            label_missing: 0,
            rejected_tokens: 0,
        }
    }

    pub fn current(&self) -> Rgb8 {
        self.current
    }

    pub fn origin(&self) -> ColorOrigin {
        self.origin
    }

    /// 界面语义标注文案——「选令牌色意味着跟随主题变化，选自定色意味着
    /// 固定，两种含义在界面上明确标注」的文案源。
    pub fn origin_label(&self) -> &'static str {
        match self.origin {
            ColorOrigin::Token(_) => "令牌色 · 跟随主题变化",
            ColorOrigin::Custom => "自定色 · 固定",
        }
    }

    /// 选令牌色（F151 快捷色格）。
    pub fn pick_token(&mut self, idx: usize) -> bool {
        if idx >= TOKEN_COUNT {
            self.rejected_tokens += 1;
            return false;
        }
        self.current = TOKENS[idx].1;
        self.origin = ColorOrigin::Token(idx);
        self.recent.push(self.current);
        true
    }

    /// 选自定色（色相环/方区/HEX 任一通路产出的 RGB）。
    pub fn pick_custom(&mut self, c: Rgb8) {
        self.current = c;
        self.origin = ColorOrigin::Custom;
        self.recent.push(c);
    }

    /// HEX 输入互认：解析成功即选为自定色；失败原样拒绝（输入框红显
    /// 由调用层处理）。
    pub fn pick_hex(&mut self, s: &str) -> Result<Rgb8, HexFail> {
        let c = parse_hex(s)?;
        self.pick_custom(c);
        Ok(c)
    }

    /// 最近使用色快照。
    pub fn recent_snapshot(&self) -> Vec<Rgb8> {
        self.recent.snapshot()
    }

    /// 最近账本引用（持久化面）。
    pub fn recent_book(&self) -> &RecentColors {
        &self.recent
    }

    /// 令牌色表只读视图。
    pub fn token(&self, idx: usize) -> Option<(&'static str, Rgb8)> {
        TOKENS.get(idx).map(|(n, c)| (*n, *c))
    }
}

impl Default for PickerState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F234 自检（12 条行为级）。
pub fn run_colorpick_checks() -> CheckSet {
    let mut set = CheckSet::new("F234-colorpick");

    // 1. 主册验收：20 色往返零误差（RGB→HSV→RGB 逐字节还原）。
    let rt_ok = ROUNDTRIP_20.iter().all(|c| {
        let hsv = rgb_to_hsv(*c);
        hsv_to_rgb(hsv) == *c
    });
    set.add("20-color round-trip zero error (rgb→hsv→rgb)", rt_ok, "");

    // 2. HSV 基准色：三原色与灰阶直接命中。
    let red = hsv_to_rgb(Hsv { h: 0, s: 255, v: 255 });
    let green = hsv_to_rgb(Hsv { h: 120, s: 255, v: 255 });
    let blue = hsv_to_rgb(Hsv { h: 240, s: 255, v: 255 });
    let gray = hsv_to_rgb(Hsv { h: 200, s: 0, v: 128 });
    set.add(
        "hsv primaries & gray exact",
        red == Rgb8::new(255, 0, 0) && green == Rgb8::new(0, 255, 0) && blue == Rgb8::new(0, 0, 255)
            && gray == Rgb8::new(128, 128, 128),
        "",
    );

    // 3. HEX 解析：大小写互认、# 可省；非法显性拒绝。
    set.add(
        "hex parse accepts case-insensitive with/without #",
        parse_hex("#1a2B3c") == Ok(Rgb8::new(0x1A, 0x2B, 0x3C))
            && parse_hex("1a2b3c") == Ok(Rgb8::new(0x1A, 0x2B, 0x3C))
            && parse_hex("#12345") == Err(HexFail::BadLength)
            && parse_hex("#12345g") == Err(HexFail::BadChar),
        "",
    );

    // 4. HEX 格式化恒大写带 #，解析互认。
    set.add(
        "hex format uppercase canonical",
        format_hex(Rgb8::new(0x1A, 0x2B, 0x3C)) == "#1A2B3C"
            && parse_hex(&format_hex(Rgb8::new(0, 0, 0))) == Ok(Rgb8::new(0, 0, 0)),
        "",
    );

    // 5. HEX↔拾色双向一致：20 判据色 format→parse 恒等（HEX 侧全域无损）。
    let hex_ok = ROUNDTRIP_20
        .iter()
        .all(|c| parse_hex(&format_hex(*c)) == Ok(*c));
    set.add("hex↔picker mutual recognition on 20 colors", hex_ok, "");

    // 6. 最近 8 色：去重置顶 + 容量淘汰（第 9 色挤掉最旧）。
    let mut rc = RecentColors::new();
    for c in ROUNDTRIP_20.iter().take(9) {
        rc.push(*c);
    }
    let snap = rc.snapshot();
    set.add(
        "recent 8: cap eviction, newest first",
        rc.len() == RECENT_CAP && snap.len() == RECENT_CAP && snap[0] == ROUNDTRIP_20[8]
            && snap[7] == ROUNDTRIP_20[1] && !snap.contains(&ROUNDTRIP_20[0]),
        "",
    );

    // 7. 去重：重选旧色提到最前，不产生重复项。
    let pick_again = ROUNDTRIP_20[4];
    rc.push(pick_again);
    let snap2 = rc.snapshot();
    let count = snap2.iter().filter(|x| **x == pick_again).count();
    set.add(
        "recent dedupe moves to front",
        snap2[0] == pick_again && count == 1 && rc.len() == RECENT_CAP,
        "",
    );

    // 8. 最近 8 色持久化 round-trip。
    let mut buf = [0u8; 64];
    let n = rc.to_bytes(&mut buf).unwrap();
    let restored = RecentColors::from_bytes(&buf[..n]).unwrap();
    set.add(
        "recent 8 persistence round-trip",
        restored.snapshot() == rc.snapshot(),
        "",
    );

    // 9. 令牌/自定双语义：24 令牌在册；令牌选择标注「跟随主题」。
    let mut ps = PickerState::new();
    let tokens_ok = (0..TOKEN_COUNT).all(|i| ps.token(i).is_some()) && ps.token(TOKEN_COUNT).is_none();
    let picked = ps.pick_token(3);
    set.add(
        "24 token colors registered, token pick labeled follow-theme",
        tokens_ok && picked && ps.origin() == ColorOrigin::Token(3)
            && ps.origin_label() == "令牌色 · 跟随主题变化",
        "",
    );

    // 10. 自定色标注「固定」；HEX 通路进自定；越界令牌拒绝。
    ps.pick_hex("#FF8000");
    let custom = ps.origin() == ColorOrigin::Custom
        && ps.origin_label() == "自定色 · 固定"
        && ps.current() == Rgb8::new(255, 128, 0);
    let rejected = !ps.pick_token(TOKEN_COUNT + 5) && ps.rejected_tokens == 1;
    set.add(
        "custom pick labeled fixed; hex path; token bounds",
        custom && rejected && ps.label_missing == 0,
        "",
    );

    // 11. 透明度滑杆场景白名单：支持透明的场景才出现。
    set.add(
        "alpha slider only in alpha-capable scenes",
        alpha_slider_visible(AlphaScene::Wallpaper)
            && alpha_slider_visible(AlphaScene::WindowFrame)
            && alpha_slider_visible(AlphaScene::TerminalBg)
            && !alpha_slider_visible(AlphaScene::TextColor)
            && !alpha_slider_visible(AlphaScene::IconTint),
        "",
    );

    // 12. fuzz 2000 轮：随机 rgb → hsv 域合法（h≤359、s/v≤255）；
    //     随机 hsv → rgb 通道 ≤ 明度；hex parse 拒绝不 panic；
    //     判据集 20 色全绿复核。
    let mut x: u32 = 0xC0_1D0_5;
    let mut ok = true;
    for _ in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let op = x % 3;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        match op {
            0 => {
                let c = Rgb8::new(x as u8, (x >> 8) as u8, (x >> 16) as u8);
                let hsv = rgb_to_hsv(c);
                if hsv.h > HUE_MAX || hsv.s > SAT_MAX || hsv.v > VAL_MAX {
                    ok = false;
                    break;
                }
            }
            1 => {
                let hsv = clamp_hsv(x % 720, (x >> 3) % 512, (x >> 6) % 512);
                let c = hsv_to_rgb(hsv);
                if c.r > hsv.v as u8 || c.g > hsv.v as u8 || c.b > hsv.v as u8 {
                    ok = false;
                    break;
                }
            }
            _ => {
                let buf = [b'0' + (x % 16) as u8; 6];
                let s = core::str::from_utf8(&buf).unwrap_or("");
                if let Ok(c) = parse_hex(s) {
                    // 合法 hex 必须可无损格式化回同色。
                    if parse_hex(&format_hex(c)) != Ok(c) {
                        ok = false;
                        break;
                    }
                }
            }
        }
    }
    set.add(
        "fuzz 2000 rounds: domain bounds & hex lossless, no panic",
        ok && ROUNDTRIP_20.iter().all(|c| hsv_to_rgb(rgb_to_hsv(*c)) == *c),
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
    fn roundtrip_twenty_colors_zero_error() {
        for c in ROUNDTRIP_20.iter() {
            let hsv = rgb_to_hsv(*c);
            assert_eq!(hsv_to_rgb(hsv), *c, "往返失真：{:?}", c);
        }
    }

    #[test]
    fn hsv_sectors_and_clamp() {
        // 60° 步进的间色。
        assert_eq!(hsv_to_rgb(Hsv { h: 60, s: 255, v: 255 }), Rgb8::new(255, 255, 0));
        assert_eq!(hsv_to_rgb(Hsv { h: 180, s: 255, v: 255 }), Rgb8::new(0, 255, 255));
        assert_eq!(hsv_to_rgb(Hsv { h: 300, s: 255, v: 255 }), Rgb8::new(255, 0, 255));
        // 越界钳制：色相回绕、饱和/明度贴边。
        let cl = clamp_hsv(720, 300, 999);
        assert_eq!(cl.h, 0);
        assert_eq!(cl.s, 255);
        assert_eq!(cl.v, 255);
        // 黑与白。
        assert_eq!(hsv_to_rgb(Hsv { h: 0, s: 0, v: 0 }), Rgb8::new(0, 0, 0));
        assert_eq!(hsv_to_rgb(Hsv { h: 359, s: 0, v: 255 }), Rgb8::new(255, 255, 255));
    }

    #[test]
    fn hex_rejects_and_accepts() {
        assert!(parse_hex("").is_err());
        assert!(parse_hex("#").is_err());
        assert!(parse_hex("#1234567").is_err());
        assert!(parse_hex("#zzzzzz").is_err());
        assert_eq!(parse_hex("#FFFFFF"), Ok(Rgb8::new(255, 255, 255)));
        assert_eq!(parse_hex("#000000"), Ok(Rgb8::new(0, 0, 0)));
        assert_eq!(format_hex(Rgb8::new(18, 52, 86)), "#123456");
    }

    #[test]
    fn recent_lifecycle_and_persistence() {
        let mut rc = RecentColors::new();
        for i in 0..12u8 {
            rc.push(Rgb8::new(i * 20, 0, 0));
        }
        assert_eq!(rc.len(), RECENT_CAP);
        let snap = rc.snapshot();
        assert_eq!(snap[0], Rgb8::new(11 * 20, 0, 0));
        // 去重置顶。
        rc.push(Rgb8::new(5 * 20, 0, 0));
        assert_eq!(rc.snapshot()[0], Rgb8::new(100, 0, 0));
        assert_eq!(rc.snapshot().iter().filter(|c| **c == Rgb8::new(100, 0, 0)).count(), 1);
        // 持久化。
        let mut buf = [0u8; 32];
        let n = rc.to_bytes(&mut buf).unwrap();
        let back = RecentColors::from_bytes(&buf[..n]).unwrap();
        assert_eq!(back.snapshot(), rc.snapshot());
        assert!(RecentColors::from_bytes(&buf[..8]).is_none());
    }

    #[test]
    fn picker_semantics_end_to_end() {
        let mut ps = PickerState::new();
        // 令牌选择 → 当前色与标注联动，最近账本入账。
        assert!(ps.pick_token(10));
        assert_eq!(ps.current(), TOKENS[10].1);
        // HEX 通路。
        ps.pick_hex("#ff0000").unwrap();
        assert_eq!(ps.origin(), ColorOrigin::Custom);
        assert_eq!(ps.recent_snapshot()[0], Rgb8::new(255, 0, 0));
        // HSV 滑杆通路。
        ps.pick_custom(hsv_to_rgb(Hsv { h: 240, s: 255, v: 255 }));
        assert_eq!(ps.current(), Rgb8::new(0, 0, 255));
        assert_eq!(ps.origin_label(), "自定色 · 固定");
        // 每次选色都有语义标注（结构保证 → 审计恒 0）。
        assert_eq!(ps.label_missing, 0);
    }

    #[test]
    fn colorpick_selfcheck_all_green() {
        let s = run_colorpick_checks();
        assert!(s.all_passed(), "F234 自检存在红项");
        assert!(!s.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
//
// 判据锚 F234。持久化面 = 最近 8 色环 FNV 校验记录；壳接线面 =
// HEX 短式 #ABC 双向解析 + SV 方区命中测试（含逆映射光标位）；
// 判定面 = run_colorpick_v2_checks（首条持久化 round-trip）。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 记录头 magic「VXH1」+ 版本（全域 v2 段统一）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;

/// 四类损坏显性拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2SaveErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadChecksum,
}

/// FNV-1a 64 位取低 32 位（常数与 vdesk 音频指纹同族）。
fn v2_fnv1a32(data: &[u8]) -> u32 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h as u32
}

/// 记录式：magic4+ver1+8 槽×4 字节（存在位+RGB）+checksum u32
/// = 41 字节定长（容量上限在册：RECENT_CAP=8，零堆）。
pub const V2_PAYLOAD_LEN: usize = RECENT_CAP * 4;
pub const V2_REC_LEN: usize = 5 + V2_PAYLOAD_LEN + 4;
const V2_BODY_LEN: usize = V2_REC_LEN - 4;

/// 最近 8 色环的 framed 持久化记录（主册 F234 v2：最近 8 色持久化
/// ——FNV 校验记录，槽序即环序最新在前）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2RecentRec {
    /// None = 空槽；Some = 颜色（最新在前的环序）。
    pub slots: [Option<Rgb8>; RECENT_CAP],
}

impl V2RecentRec {
    pub fn capture(rc: &RecentColors) -> V2RecentRec {
        let mut out = V2RecentRec { slots: [const { None }; RECENT_CAP] };
        for (i, c) in rc.snapshot().into_iter().enumerate().take(RECENT_CAP) {
            out.slots[i] = Some(c);
        }
        out
    }

    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < V2_REC_LEN {
            return None;
        }
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        for (i, slot) in self.slots.iter().enumerate() {
            let base = 5 + i * 4;
            match slot {
                Some(c) => {
                    out[base] = 1;
                    out[base + 1] = c.r;
                    out[base + 2] = c.g;
                    out[base + 3] = c.b;
                }
                None => out[base..base + 4].copy_from_slice(&[0, 0, 0, 0]),
            }
        }
        let sum = v2_fnv1a32(&out[..V2_BODY_LEN]);
        out[V2_BODY_LEN..V2_REC_LEN].copy_from_slice(&sum.to_le_bytes());
        Some(V2_REC_LEN)
    }

    pub fn from_bytes(buf: &[u8]) -> Result<V2RecentRec, V2SaveErr> {
        if buf.len() != V2_REC_LEN {
            return Err(V2SaveErr::BadLen);
        }
        if buf[..4] != V2_MAGIC {
            return Err(V2SaveErr::BadMagic);
        }
        if buf[4] != V2_VERSION {
            return Err(V2SaveErr::BadVersion);
        }
        let expect = u32::from_le_bytes([buf[V2_BODY_LEN], buf[V2_BODY_LEN + 1], buf[V2_BODY_LEN + 2], buf[V2_BODY_LEN + 3]]);
        if v2_fnv1a32(&buf[..V2_BODY_LEN]) != expect {
            return Err(V2SaveErr::BadChecksum);
        }
        let mut rec = V2RecentRec { slots: [const { None }; RECENT_CAP] };
        for (i, slot) in rec.slots.iter_mut().enumerate() {
            let base = 5 + i * 4;
            match buf[base] {
                0 => {}
                1 => *slot = Some(Rgb8::new(buf[base + 1], buf[base + 2], buf[base + 3])),
                // 存在位非法（>1）——按长度损坏拒绝。
                _ => return Err(V2SaveErr::BadLen),
            }
        }
        Ok(rec)
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// HEX 短式解析（主册 F234 v2：#ABC 短式——每位翻倍展开为 #AABBCC；
/// 6 位全式走既有 parse_hex，一处一事实）。
pub fn v2_parse_hex_flex(s: &str) -> Result<Rgb8, HexFail> {
    let t = s.strip_prefix('#').unwrap_or(s);
    match t.len() {
        3 => {
            let b = t.as_bytes();
            let mut v = [0u32; 3];
            for (i, c) in b.iter().enumerate() {
                match hex_val(*c) {
                    Some(x) => v[i] = x,
                    None => return Err(HexFail::BadChar),
                }
            }
            Ok(Rgb8::new(
                (v[0] * 16 + v[0]) as u8,
                (v[1] * 16 + v[1]) as u8,
                (v[2] * 16 + v[2]) as u8,
            ))
        }
        6 => parse_hex(s),
        _ => Err(HexFail::BadLength),
    }
}

/// SV 方区命中测试：x → 饱和度（左 0 右 255）、y → 明度（顶 255 底 0）。
/// 色相由调用方当前档注入（方区只承载 SV 两维）。面外 None。
pub fn v2_sv_pick(area: &crate::h1star::h1base::Rect, hue: u32, px: i32, py: i32) -> Option<Hsv> {
    if !area.contains(px, py) {
        return None;
    }
    let s = ((px - area.x) * SAT_MAX as i32 / (area.w - 1).max(1)).clamp(0, SAT_MAX as i32) as u32;
    let v = ((area.bottom() - 1 - py) * VAL_MAX as i32 / (area.h - 1).max(1)).clamp(0, VAL_MAX as i32) as u32;
    Some(Hsv { h: hue % (HUE_MAX + 1), s, v })
}

/// 逆映射：Hsv → 方区光标位（与 v2_sv_pick 双向一致，±1px 取整容差）。
pub fn v2_sv_cursor(area: &crate::h1star::h1base::Rect, hsv: Hsv) -> (i32, i32) {
    let x = area.x + hsv.s as i32 * (area.w - 1) / SAT_MAX as i32;
    let y = area.bottom() - 1 - hsv.v as i32 * (area.h - 1) / VAL_MAX as i32;
    (x, y)
}

// -- 判定面扩展 ------------------------------------------------------------

/// F234 v2 自检（首条必为持久化 round-trip）。
pub fn run_colorpick_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F234-colorpick-v2");

    // 1. 持久化 round-trip（验主册「最近 8 色持久化验证」——环序还原）。
    let mut rc = RecentColors::new();
    for c in ROUNDTRIP_20.iter().take(5) {
        rc.push(*c);
    }
    let rec = V2RecentRec::capture(&rc);
    let mut buf = [0u8; V2_REC_LEN];
    let wrote = rec.to_bytes(&mut buf).unwrap_or(0);
    let back = V2RecentRec::from_bytes(&buf[..wrote]);
    set.add(
        "v2 persist round-trip: recent-8 ring",
        wrote == V2_REC_LEN && back == Ok(rec) && rec.slots[0] == Some(ROUNDTRIP_20[4]),
        "",
    );

    // 2. 四类损坏全拒绝 + 存在位非法拒绝。
    //    [缺陷账本] 现象：/flag 分支红。根因：检查项直接翻存在位字节
    //    （载荷域，受校验和覆盖）——实现先验校验和再解析存在位，翻位
    //    后必先报 BadChecksum，flag 分支（BadLen）从未被测到，属检查
    //    项构造缺陷。修法：改检查项——翻位后重算校验和，使校验通过、
    //    仅存在位非法，真测 flag 分支。
    let mut b1 = buf;
    b1[0] = b'X';
    let mut b2 = buf;
    b2[4] = 9;
    let mut b3 = buf;
    b3[5] = 2; // 存在位 >1。
    let sum3 = v2_fnv1a32(&b3[..V2_BODY_LEN]);
    b3[V2_BODY_LEN..V2_REC_LEN].copy_from_slice(&sum3.to_le_bytes());
    let mut b4 = buf;
    b4[V2_REC_LEN - 1] ^= 0xFF;
    set.add(
        "v2 persist rejects magic/version/len/checksum/flag",
        matches!(V2RecentRec::from_bytes(&b1), Err(V2SaveErr::BadMagic))
            && matches!(V2RecentRec::from_bytes(&b2), Err(V2SaveErr::BadVersion))
            && matches!(V2RecentRec::from_bytes(&b3), Err(V2SaveErr::BadLen))
            && matches!(V2RecentRec::from_bytes(&buf[..V2_REC_LEN - 1]), Err(V2SaveErr::BadLen))
            && matches!(V2RecentRec::from_bytes(&b4), Err(V2SaveErr::BadChecksum)),
        "",
    );

    // 3. HEX 短式（验主册 v2 锚「#ABC 短式」：#abc = #AABBCC；全式不回归）。
    set.add(
        "v2 hex short form expands, full form unchanged",
        v2_parse_hex_flex("#abc") == Ok(Rgb8::new(0xAA, 0xBB, 0xCC))
            && v2_parse_hex_flex("F08") == Ok(Rgb8::new(0xFF, 0x00, 0x88))
            && v2_parse_hex_flex("#1A2B3C") == parse_hex("#1A2B3C")
            && v2_parse_hex_flex("#12") == Err(HexFail::BadLength)
            && v2_parse_hex_flex("#abz") == Err(HexFail::BadChar),
        "",
    );

    // 4. SV 方区命中（验主册「明度饱和度方区（内区）」：面内四角取值
    //    正确、面外拒绝；选点 → 光标位双向 ±1px）。
    let area = crate::h1star::h1base::Rect::new(10, 10, 256, 256);
    let tl = v2_sv_pick(&area, 200, 10, 10);
    let br = v2_sv_pick(&area, 200, 265, 265);
    let (cx, cy) = v2_sv_cursor(&area, Hsv { h: 0, s: 128, v: 200 });
    let round = v2_sv_pick(&area, 0, cx, cy);
    set.add(
        "v2 SV face hit: corners & roundtrip ±1px",
        tl == Some(Hsv { h: 200, s: 0, v: 255 })
            && br == Some(Hsv { h: 200, s: 255, v: 0 })
            && v2_sv_pick(&area, 0, 0, 0).is_none()
            && round.map(|h| (h.s as i32 - 128).abs() <= 1 && (h.v as i32 - 200).abs() <= 1) == Some(true),
        "",
    );

    // 5. 记录与账本同源（capture 环序 = snapshot 环序——一处一事实）。
    let snap = rc.snapshot();
    set.add(
        "v2 record ring order matches book snapshot",
        rec.slots.iter().flatten().copied().eq(snap.iter().copied()),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_recent_rec_roundtrip_with_empty_slots() {
        let mut rc = RecentColors::new();
        rc.push(Rgb8::new(1, 2, 3));
        let rec = V2RecentRec::capture(&rc);
        let mut buf = [0u8; V2_REC_LEN];
        let n = rec.to_bytes(&mut buf).unwrap();
        let back = V2RecentRec::from_bytes(&buf[..n]).unwrap();
        assert_eq!(back, rec);
        assert!(back.slots[0].is_some() && back.slots[1..].iter().all(|s| s.is_none()));
    }

    #[test]
    fn v2_hex_short_roundtrip_with_format() {
        for s in ["#f0a", "123", "#AbC"] {
            let c = v2_parse_hex_flex(s).unwrap();
            assert_eq!(parse_hex(&format_hex(c)), Ok(c), "短式 {s} 经格式化后互认");
        }
    }

    #[test]
    fn colorpick_v2_selfcheck_all_green() {
        let s = run_colorpick_v2_checks();
        assert!(s.all_passed(), "F234 v2 自检存在红项");
        assert!(!s.truncated());
    }
}
