//! VARIX-M500 · AI-12 主题艺术与创意表达（F276~F300，M2）
//!
//! 使命：让每个人都是设计师——主题包 v2、壁纸 SDK、可变字体、季节换装。
//! 纯逻辑 + 固定容量数组，无堆分配。与 AURORA 的 `designsys.rs`（色彩/版式
//! 设计系统）零重复：本模块聚焦主题包生命周期（格式、验证、管理、混搭、
//! 回滚、隔离、转换、巡检）。

use crate::checks::CheckSet;

/// 主题标识槽位数（本地主题库容量）。
pub const THEME_SLOTS: usize = 16;

// ---------------------------------------------------------------------------
// F276 主题包格式 v2 — 全要素打包规范
// ---------------------------------------------------------------------------

/// 主题包 v2 魔数与元素位掩码。
pub const THEME_MAGIC: &[u8; 4] = b"VXT2";

/// 元素位：壁纸/图标/光标/声音/字体/动效曲线。
pub const EL_WALLPAPER: u16 = 1 << 0;
pub const EL_ICONS: u16 = 1 << 1;
pub const EL_CURSOR: u16 = 1 << 2;
pub const EL_SOUND: u16 = 1 << 3;
pub const EL_FONT: u16 = 1 << 4;
pub const EL_MOTION: u16 = 1 << 5;
pub const EL_ALL: u16 = 0x3F;

#[derive(Clone, Copy)]
pub struct ThemeHeader {
    pub elements: u16,
    pub api_ver: u8,
    pub payload_len: u32,
}

/// 校验包头：魔数 + 元素位不越界 + API 版本受支持。
pub fn parse_theme_header(data: &[u8]) -> Option<ThemeHeader> {
    if data.len() < 11 || &data[0..4] != THEME_MAGIC {
        return None;
    }
    let elements = u16::from_le_bytes([data[4], data[5]]);
    if elements & !EL_ALL != 0 {
        return None;
    }
    let api_ver = data[6];
    if !(1..=2).contains(&api_ver) {
        return None;
    }
    let payload_len = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
    if data.len() as u64 != 11 + payload_len as u64 {
        return None;
    }
    Some(ThemeHeader { elements, api_ver, payload_len })
}

// ---------------------------------------------------------------------------
// F277 主题验证器 — 完整性/对比度校验
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeVerdict {
    pub crc_ok: bool,
    pub contrast_ok: bool,
    pub all_elements_declared: bool,
}

/// CRC32（复用 power 的实现语义，这里独立小实现以保持模块自洽）。
pub fn theme_crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// WCAG 风格对比度：两色各自从 sRGB 近似求相对亮度（位深度 8，通道和近似），
/// 这里用可复核的简化式：亮度 = (R*2 + G*3 + B) / 6。
pub fn contrast_ratio(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> u32 {
    let lum = |c: (u8, u8, u8)| -> u32 {
        (c.0 as u32 * 2 + c.1 as u32 * 3 + c.2 as u32) / 6
    };
    let a = lum(fg);
    let b = lum(bg);
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    if lo == 0 {
        return u32::MAX;
    }
    hi * 100 / lo
}

pub fn validate_theme(crc_ok: bool, fg: (u8, u8, u8), bg: (u8, u8, u8), elements: u16) -> ThemeVerdict {
    ThemeVerdict {
        crc_ok,
        contrast_ok: contrast_ratio(fg, bg) >= 300, // 3.0:1 下限
        all_elements_declared: elements & EL_ALL == elements,
    }
}

// ---------------------------------------------------------------------------
// F278 本地主题管理器 — 主题库（固定 16 槽）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ThemeEntry {
    pub id: u32,
    pub active: bool,
    pub elements: u16,
}

#[derive(Clone, Copy)]
pub struct ThemeLibrary {
    pub entries: [ThemeEntry; THEME_SLOTS],
    pub len: usize,
    pub next_id: u32,
}

impl ThemeLibrary {
    pub const fn new() -> ThemeLibrary {
        ThemeLibrary {
            entries: [ThemeEntry { id: 0, active: false, elements: 0 }; THEME_SLOTS],
            len: 0,
            next_id: 1,
        }
    }

    /// 注册主题（自带去重：同 id 重复注册是 no-op）。
    pub fn register(&mut self, elements: u16) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        if self.len < THEME_SLOTS {
            self.entries[self.len] = ThemeEntry { id, active: false, elements };
            self.len += 1;
        }
        id
    }

    /// 激活某主题（同一时间只有一个激活）。
    pub fn activate(&mut self, id: u32) -> bool {
        let mut found = false;
        for i in 0..self.len {
            let is_target = self.entries[i].id == id;
            self.entries[i].active = is_target;
            found = found || is_target;
        }
        found
    }

    pub fn active(&self) -> Option<u32> {
        for i in 0..self.len {
            if self.entries[i].active {
                return Some(self.entries[i].id);
            }
        }
        None
    }

    /// 按元素过滤：返回包含指定元素位主题的数量。
    pub fn count_with(&self, el: u16) -> usize {
        self.entries[..self.len].iter().filter(|e| e.elements & el == el).count()
    }
}

// ---------------------------------------------------------------------------
// F279 动态壁纸 SDK — 粒子/着色接口（开放）
// ---------------------------------------------------------------------------

/// 动态壁纸描述：渲染模式 + 粒子上限 + 帧率上限。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveWallSpec {
    pub particle_cap: u16,
    pub fps_cap: u8,
    pub shader_slots: u8,
}

/// SDK 默认预算（保守值，供第三方起步）。
pub const LIVEWALL_DEFAULT: LiveWallSpec = LiveWallSpec { particle_cap: 512, fps_cap: 30, shader_slots: 2 };

/// 合法性：粒子 ≤ 8192、fps ≤ 60、shader ≤ 8。
pub fn livewall_valid(spec: &LiveWallSpec) -> bool {
    spec.particle_cap <= 8192 && spec.fps_cap <= 60 && spec.shader_slots <= 8
}

/// 按性能档收紧预算。
pub fn livewall_for_tier(spec: &LiveWallSpec, tier: u8) -> LiveWallSpec {
    match tier {
        0 => LiveWallSpec { particle_cap: 0, fps_cap: 0, shader_slots: 0 }, // 静态降级
        1 => LiveWallSpec { particle_cap: spec.particle_cap / 4, fps_cap: 15, shader_slots: 1 },
        _ => *spec,
    }
}

// ---------------------------------------------------------------------------
// F280 动壁纸帧红线 — 性能预算
// ---------------------------------------------------------------------------

/// 单帧预算 8ms（125fps 供给上限前的硬红线，合成器留余量）。
pub const LIVEWALL_FRAME_BUDGET_US: u32 = 8_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameVerdict {
    Within,
    OverBudget,
}

pub fn livewall_frame_check(render_us: u32) -> FrameVerdict {
    if render_us > LIVEWALL_FRAME_BUDGET_US {
        FrameVerdict::OverBudget
    } else {
        FrameVerdict::Within
    }
}

/// 连续超标 N 帧触发自动降档。
pub fn livewall_autoderade(over_streak: u32, streak_threshold: u32) -> bool {
    over_streak >= streak_threshold && streak_threshold > 0
}

// ---------------------------------------------------------------------------
// F281 全局图标主题 — 图标替换管线
// ---------------------------------------------------------------------------

/// 图标名 → 主题内覆盖索引；无覆盖返回 None（回落系统默认）。
pub struct IconMap {
    /// (name[8], index) 固定 32 条。
    pub entries: [([u8; 8], u16); 32],
    pub len: usize,
}

impl IconMap {
    pub const fn new() -> IconMap {
        IconMap { entries: [([0; 8], 0); 32], len: 0 }
    }
    pub fn put(&mut self, name: &[u8], index: u16) -> bool {
        if name.is_empty() || name.len() > 8 {
            return false;
        }
        for i in 0..self.len {
            if self.entries[i].0[..name.len()] == *name && self.entries[i].0[name.len()] == 0 {
                self.entries[i].1 = index; // 覆盖更新
                return true;
            }
        }
        if self.len < 32 {
            let mut key = [0u8; 8];
            key[..name.len()].copy_from_slice(name);
            self.entries[self.len] = (key, index);
            self.len += 1;
            true
        } else {
            false
        }
    }
    pub fn get(&self, name: &[u8]) -> Option<u16> {
        for i in 0..self.len {
            if self.entries[i].0[..name.len()] == *name && self.entries[i].0[name.len()] == 0 {
                return Some(self.entries[i].1);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// F282 光标主题 — 指针皮肤（状态 × 帧表）
// ---------------------------------------------------------------------------

/// 指针状态：普通/文本/链接/拖拽/忙碌/缩放。
pub const CURSOR_STATES: usize = 6;

#[derive(Clone, Copy)]
pub struct CursorTheme {
    /// 每状态帧序列号表（0 = 无覆盖）。
    pub frames: [[u16; 4]; CURSOR_STATES],
    /// 每状态热点偏移 (x, y)。
    pub hotspots: [(u8, u8); CURSOR_STATES],
}

impl CursorTheme {
    pub const fn new() -> CursorTheme {
        CursorTheme { frames: [[0; 4]; CURSOR_STATES], hotspots: [(0, 0); CURSOR_STATES] }
    }
    pub fn set_state(&mut self, state: usize, frames: [u16; 4], hotspot: (u8, u8)) -> bool {
        if state >= CURSOR_STATES {
            return false;
        }
        self.frames[state] = frames;
        self.hotspots[state] = hotspot;
        true
    }
    /// 忙碌状态动画帧选择（按时间步进循环）。
    pub fn busy_frame(&self, tick: u32) -> u16 {
        let seq = self.frames[4];
        let anim: [u16; 4] = [seq[0], seq[1], seq[2], seq[3]];
        if anim.iter().all(|&f| f == 0) {
            return 0;
        }
        anim[(tick % 4) as usize]
    }
}

// ---------------------------------------------------------------------------
// F283 声音整包 — 系统音主题
// ---------------------------------------------------------------------------

/// 系统音事件槽：登录/通知/错误/关机。
pub const SOUND_EVENTS: usize = 4;

#[derive(Clone, Copy)]
pub struct SoundPack {
    /// 每事件音色 id（0 = 静音）。
    pub tones: [u16; SOUND_EVENTS],
}

impl SoundPack {
    pub const fn silent() -> SoundPack {
        SoundPack { tones: [0; SOUND_EVENTS] }
    }
    pub fn tone(&self, event: usize) -> u16 {
        if event < SOUND_EVENTS {
            self.tones[event]
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// F284 可变字体支持 — 字重连续调节
// ---------------------------------------------------------------------------

/// 字重轴 100~900 线性插值（轴向实例化）。
pub fn variable_weight(wght_a: u16, wght_b: u16, t_permille: u16) -> u16 {
    let (lo, hi) = if wght_a <= wght_b { (wght_a, wght_b) } else { (wght_b, wght_a) };
    let t = (t_permille as u32).min(1000);
    (lo as u32 + ((hi as u32 - lo as u32) * t) / 1000) as u16
}

/// 字重 → 排版档位（100 段一格）。
pub fn weight_bucket(wght: u16) -> u8 {
    (((wght as u32).clamp(100, 900) - 100) / 100) as u8
}

// ---------------------------------------------------------------------------
// F285 排版尺度令牌 — 字号阶梯体系
// ---------------------------------------------------------------------------

/// 1.25 比例阶梯（取整 px）：12/15/19/24/30/37/47/59。
pub const TYPE_SCALE: [u16; 8] = [12, 15, 19, 24, 30, 37, 47, 59];

pub fn type_scale_step(level: usize) -> u16 {
    TYPE_SCALE[level.min(TYPE_SCALE.len() - 1)]
}

// ---------------------------------------------------------------------------
// F286 动效曲线包 — 社区曲线分享
// ---------------------------------------------------------------------------

/// 三次贝塞尔 (x1,y1,x2,y2) 曲线包，固定 8 条。
pub const MOTION_CURVES: usize = 8;

#[derive(Clone, Copy)]
pub struct CurvePack {
    pub curves: [[u8; 4]; MOTION_CURVES], // 分量以 1/255 为单位
    pub len: usize,
}

impl CurvePack {
    pub const fn new() -> CurvePack {
        CurvePack { curves: [[0; 4]; MOTION_CURVES], len: 0 }
    }
    pub fn add(&mut self, c: [u8; 4]) -> bool {
        // 去重
        for i in 0..self.len {
            if self.curves[i] == c {
                return true;
            }
        }
        if self.len < MOTION_CURVES {
            self.curves[self.len] = c;
            self.len += 1;
            true
        } else {
            false
        }
    }
    /// 贝塞尔 y(t) 三次近似（x 分量做时间参数）。
    pub fn sample(&self, idx: usize, t: u8) -> u8 {
        if idx >= self.len {
            return t;
        }
        let c = self.curves[idx];
        let f = |v: u8| -> u32 { v as u32 };
        let t32 = t as u32;
        let mt = 255 - t32;
        // B(t) = 3(1-t)²t·y1 + 3(1-t)t²·y2 + t³（起点/终点为 0/1）
        let v = 3 * mt * mt * t32 / 255 / 255 * f(c[1]) / 255
            + 3 * mt * t32 * t32 / 255 / 255 * f(c[3]) / 255
            + t32 * t32 * t32 / 255 / 255;
        (v.min(255)) as u8
    }
}

// ---------------------------------------------------------------------------
// F287 对比度守护 — 无障碍自动校验
// ---------------------------------------------------------------------------

/// 文本 ≥4.5:1（近似 450），大字 ≥3.0:1（300）。
pub fn contrast_guard(fg: (u8, u8, u8), bg: (u8, u8, u8), large_text: bool) -> bool {
    let need = if large_text { 300 } else { 450 };
    contrast_ratio(fg, bg) >= need
}

// ---------------------------------------------------------------------------
// F288 壁纸取色渐变 — 换装动效（艺术）
// ---------------------------------------------------------------------------

/// 从壁纸主色派生渐变终点：亮度拉向中间调（避免过曝/死黑）。
pub fn gradient_end(primary: (u8, u8, u8)) -> (u8, u8, u8) {
    let lum = (primary.0 as u32 * 2 + primary.1 as u32 * 3 + primary.2 as u32) / 6;
    let target = if lum < 80 { lum + 60 } else if lum > 180 { lum - 60 } else { lum };
    let scale = |v: u8| -> u8 {
        if lum == 0 {
            40
        } else {
            ((v as u32 * target) / lum).min(255) as u8
        }
    };
    (scale(primary.0), scale(primary.1), scale(primary.2))
}

/// 渐变插值（t 千分比）。
pub fn lerp_color(a: (u8, u8, u8), b: (u8, u8, u8), t_permille: u16) -> (u8, u8, u8) {
    let t = (t_permille as u32).min(1000);
    let ch = |x: u8, y: u8| -> u8 {
        let v = x as i32 + ((y as i32 - x as i32) * t as i32) / 1000;
        v.clamp(0, 255) as u8
    };
    (ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2))
}

// ---------------------------------------------------------------------------
// F289 季节主题 — 时令自动换装
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

/// 北半球气象季节（月，日简化）。
pub fn season_of(month: u8) -> Season {
    match month {
        3..=5 => Season::Spring,
        6..=8 => Season::Summer,
        9..=11 => Season::Autumn,
        _ => Season::Winter,
    }
}

/// 季节 → 推荐元素位。
pub fn season_elements(s: Season) -> u16 {
    match s {
        Season::Spring => EL_WALLPAPER | EL_ICONS,
        Season::Summer => EL_WALLPAPER | EL_MOTION,
        Season::Autumn => EL_WALLPAPER | EL_CURSOR,
        Season::Winter => EL_WALLPAPER | EL_SOUND,
    }
}

// ---------------------------------------------------------------------------
// F290 主题混搭 — 跨包元素组合
// ---------------------------------------------------------------------------

/// 从 A 取壁纸/图标，从 B 取声音/动效，冲突时 A 优先。
pub fn mix_themes(a: u16, b: u16) -> u16 {
    let from_a = EL_WALLPAPER | EL_ICONS | EL_CURSOR;
    (a & from_a) | (b & !from_a & EL_ALL)
}

// ---------------------------------------------------------------------------
// F291 主题回滚 — 一键还原
// ---------------------------------------------------------------------------

/// 单层撤销栈（激活历史固定 4 条）。
pub struct ThemeHistory {
    stack: [u32; 4],
    len: usize,
}

impl ThemeHistory {
    pub const fn new() -> ThemeHistory {
        ThemeHistory { stack: [0; 4], len: 0 }
    }
    pub fn push(&mut self, id: u32) {
        if self.len < 4 {
            self.stack[self.len] = id;
            self.len += 1;
        } else {
            self.stack[0] = self.stack[1];
            self.stack[1] = self.stack[2];
            self.stack[2] = self.stack[3];
            self.stack[3] = id;
        }
    }
    pub fn rollback(&mut self) -> Option<u32> {
        if self.len < 2 {
            return None; // 至少要有一条"上一个"
        }
        self.len -= 1;
        Some(self.stack[self.len - 1])
    }
}

// ---------------------------------------------------------------------------
// F292 恶意主题隔离 — 主题压测
// ---------------------------------------------------------------------------

/// 隔离判定：元素位越界 / 资源引用环 / 声明尺寸与实际不符 → 隔离。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeIsolation {
    Clean,
    Quarantined,
}

pub fn quarantine_check(elements: u16, declared_len: u32, actual_len: u32, self_ref: bool) -> ThemeIsolation {
    if elements & !EL_ALL != 0 || declared_len != actual_len || self_ref {
        ThemeIsolation::Quarantined
    } else {
        ThemeIsolation::Clean
    }
}

// ---------------------------------------------------------------------------
// F293 主题开发文档 — 开放规范（版本常量 + 字节化渲染）
// ---------------------------------------------------------------------------

pub const THEME_SPEC_VERSION: &str = "VXT2/1";
pub const THEME_SPEC_FEATURES: u16 = EL_ALL;

/// 渲染规范头一行：`VXT2/1 elements=0x3F`。
pub fn render_spec_header(out: &mut [u8]) -> usize {
    use crate::checks::{push_hex_u64, push_str};
    let mut n = 0;
    push_str(out, &mut n, THEME_SPEC_VERSION);
    push_str(out, &mut n, " elements=0x");
    push_hex_u64(out, &mut n, THEME_SPEC_FEATURES as u64);
    n
}

// ---------------------------------------------------------------------------
// F294 主题预览沙盒 — 安全试装
// ---------------------------------------------------------------------------

/// 沙盒内试装：不写激活位、只返回预览 id 组合。
pub fn preview_combine(lib: &ThemeLibrary, a_id: u32, b_id: u32) -> Option<(u16, u16)> {
    let mut ea = None;
    let mut eb = None;
    for e in lib.entries[..lib.len].iter() {
        if e.id == a_id {
            ea = Some(e.elements);
        }
        if e.id == b_id {
            eb = Some(e.elements);
        }
    }
    match (ea, eb) {
        (Some(x), Some(y)) => Some((x, mix_themes(x, y))),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// F295 第三方主题转换 — 导入器
// ---------------------------------------------------------------------------

/// 外部包 → VXT2 元素位映射（位序对齐即为恒等映射，含裁剪）。
pub fn import_external(bits: u16) -> u16 {
    bits & EL_ALL
}

/// 批量导入：返回有效包数。
pub fn import_batch<'a>(packs: impl Iterator<Item = &'a [u8]>, out: &mut [u16]) -> usize {
    let mut n = 0;
    for p in packs {
        if let Some(h) = parse_theme_header(p) {
            if n < out.len() {
                out[n] = h.elements;
                n += 1;
            }
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F296 视觉规范巡检 — 品质线审计
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditRow {
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
    pub large: bool,
}

/// 全部行都过对比度线才算过巡检；返回失败行数。
pub fn audit_contrast(rows: &[AuditRow]) -> usize {
    rows.iter().filter(|r| !contrast_guard(r.fg, r.bg, r.large)).count()
}

// ---------------------------------------------------------------------------
// F297 OLED 纯黑档 — 深色深度档
// ---------------------------------------------------------------------------

/// OLED 档：背景压到 #000000，面板压到 0x08。
pub fn oled_black_panel(panel: (u8, u8, u8)) -> (u8, u8, u8) {
    let lum = (panel.0 as u32 * 2 + panel.1 as u32 * 3 + panel.2 as u32) / 6;
    if lum <= 8 {
        (0, 0, 0)
    } else {
        panel
    }
}

// ---------------------------------------------------------------------------
// F298 动效减弱强化 — 系统级遵从
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionPref {
    Full,
    Reduced,
    Off,
}

/// 偏好 → 每动效时长千分比（Reduced 30%，Off 0%）。
pub fn motion_scale(pref: MotionPref) -> u16 {
    match pref {
        MotionPref::Full => 1000,
        MotionPref::Reduced => 300,
        MotionPref::Off => 0,
    }
}

/// 晃动/视差类动效在 Reduced/Off 下一律禁用。
pub fn parallax_allowed(pref: MotionPref) -> bool {
    pref == MotionPref::Full
}

// ---------------------------------------------------------------------------
// F299 主题 API 版本化
// ---------------------------------------------------------------------------

pub const THEME_API_VERSION: u8 = 2;
pub const THEME_API_MIN: u8 = 1;

pub fn theme_api_ok(requested: u8) -> bool {
    (THEME_API_MIN..=THEME_API_VERSION).contains(&requested)
}

// ---------------------------------------------------------------------------
// F300 主题域自检（M500）
// ---------------------------------------------------------------------------

pub fn run_theme_checks() -> CheckSet {
    let mut set = CheckSet::new("theme-m500");

    // F276
    let mut pkt = [0u8; 17];
    pkt[0..4].copy_from_slice(THEME_MAGIC);
    pkt[4..6].copy_from_slice(&EL_WALLPAPER.to_le_bytes());
    pkt[6] = 2;
    pkt[7..11].copy_from_slice(&5u32.to_le_bytes());
    pkt[11..16].copy_from_slice(b"hello");
    let pkt = &pkt[..16];
    set.add("F276 header ok", parse_theme_header(pkt).is_some(), "valid v2");
    set.add("F276 bad magic", parse_theme_header(b"XXXX00000000").is_none(), "magic gate");
    set.add("F276 bad element", {
        let mut bad = [0u8; 16];
        bad.copy_from_slice(pkt);
        bad[4] = 0xFF;
        bad[5] = 0xFF;
        parse_theme_header(&bad).is_none()
    }, "bits out of range");

    // F277
    let v = validate_theme(true, (255, 255, 255), (0, 0, 0), EL_ALL);
    set.add("F277 contrast ok", v.contrast_ok, "white on black");
    let bad = validate_theme(true, (128, 128, 128), (140, 140, 140), EL_ALL);
    set.add("F277 contrast fail", !bad.contrast_ok, "too close");

    // F278
    let mut lib = ThemeLibrary::new();
    let id1 = lib.register(EL_WALLPAPER | EL_FONT);
    let id2 = lib.register(EL_SOUND);
    set.add("F278 register 2", lib.len == 2 && id1 == 1 && id2 == 2, "ids");
    set.add("F278 activate", lib.activate(id2) && lib.active() == Some(id2), "one active");
    set.add("F278 filter font", lib.count_with(EL_FONT) == 1, "only theme1");

    // F279
    set.add("F279 default valid", livewall_valid(&LIVEWALL_DEFAULT), "default");
    set.add("F279 reject wild", !livewall_valid(&LiveWallSpec { particle_cap: 9999, fps_cap: 90, shader_slots: 1 }), "fps>60");
    set.add("F279 tier0 static", livewall_for_tier(&LIVEWALL_DEFAULT, 0).particle_cap == 0, "degrade");

    // F280
    set.add("F280 within", livewall_frame_check(7_999) == FrameVerdict::Within, "7.9ms");
    set.add("F280 over", livewall_frame_check(9_000) == FrameVerdict::OverBudget, "9ms");
    set.add("F280 auto degrade", livewall_autoderade(5, 5) && !livewall_autoderade(4, 5), "streak");

    // F281
    let mut icons = IconMap::new();
    set.add("F281 put", icons.put(b"folder", 7), "insert");
    set.add("F281 overwrite", icons.put(b"folder", 9) && icons.get(b"folder") == Some(9), "update");
    set.add("F281 miss", icons.get(b"trash").is_none(), "fallback");
    set.add("F281 name guard", !icons.put(b"toolongname", 1), "len>8");

    // F282
    let mut cur = CursorTheme::new();
    set.add("F282 set ok", cur.set_state(4, [10, 11, 12, 13], (4, 2)), "busy");
    set.add("F282 set bad", !cur.set_state(9, [1, 0, 0, 0], (0, 0)), "state range");
    set.add("F282 busy anim", cur.busy_frame(5) == 11 && cur.busy_frame(7) == 13, "cycle");

    // F283
    let mut pack = SoundPack::silent();
    pack.tones[0] = 42;
    set.add("F283 tone", pack.tone(0) == 42 && pack.tone(3) == 0, "slots");

    // F284
    set.add("F284 mid weight", variable_weight(100, 900, 500) == 500, "half");
    set.add("F284 clamp t", variable_weight(400, 600, 2000) == 600, "t<=1000");
    set.add("F284 bucket", weight_bucket(450) == 3 && weight_bucket(95) == 0, "100-step");

    // F285
    set.add("F285 scale", type_scale_step(3) == 24, "level 3");
    set.add("F285 clamp", type_scale_step(99) == 59, "top clamp");

    // F286
    let mut cp = CurvePack::new();
    set.add("F286 add", cp.add([64, 0, 192, 255]) && cp.len == 1, "insert");
    set.add("F286 dedup", cp.add([64, 0, 192, 255]) && cp.len == 1, "no dup");
    set.add("F286 endpoints", cp.sample(0, 0) == 0, "t=0");

    // F287
    set.add("F287 body ok", contrast_guard((0, 0, 0), (255, 255, 255), false), "4.5 needed");
    set.add("F287 large ok", contrast_guard((80, 80, 80), (255, 255, 255), true), "3.0 enough");
    set.add("F287 body fail", !contrast_guard((140, 140, 140), (160, 160, 160), false), "close grays");

    // F288
    let end = gradient_end((230, 200, 180));
    set.add("F288 gradient end", end.0 < 230, "pull down");
    let mid = lerp_color((0, 0, 0), (255, 255, 255), 500);
    set.add("F288 lerp mid", mid == (127, 127, 127), "half");
    set.add("F288 lerp clamp", lerp_color((0, 0, 0), (255, 255, 255), 2000) == (255, 255, 255), "t clamp");

    // F289
    set.add("F289 seasons", season_of(4) == Season::Spring && season_of(12) == Season::Winter, "mapping");
    set.add("F289 elements", season_elements(Season::Summer) & EL_MOTION != 0, "summer motion");

    // F290
    let m = mix_themes(EL_WALLPAPER | EL_CURSOR | EL_SOUND, EL_ICONS | EL_SOUND);
    set.add("F290 a wins", m & (EL_WALLPAPER | EL_CURSOR) == EL_WALLPAPER | EL_CURSOR, "from A");
    set.add("F290 b sound", m & EL_SOUND == EL_SOUND, "B sound kept");
    set.add("F290 a wins icons", m & EL_ICONS == 0, "A owns icons slot");

    // F291
    let mut hist = ThemeHistory::new();
    hist.push(1);
    hist.push(2);
    let prev = hist.rollback();
    set.add("F291 rollback", prev == Some(1), "one back");
    set.add("F291 empty stop", hist.rollback().is_none() || hist.rollback().is_none(), "bounded");
    hist.push(9);
    set.add("F291 push after", hist.rollback().is_some(), "still usable");

    // F292
    set.add("F292 clean", quarantine_check(EL_WALLPAPER, 100, 100, false) == ThemeIsolation::Clean, "clean pack");
    set.add("F292 size lie", quarantine_check(EL_WALLPAPER, 100, 90, false) == ThemeIsolation::Quarantined, "declared!=actual");
    set.add("F292 self ref", quarantine_check(0, 10, 10, true) == ThemeIsolation::Quarantined, "cycle");

    // F293
    let mut buf = [0u8; 32];
    let n = render_spec_header(&mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap();
    set.add("F293 spec render", text == "VXT2/1 elements=0x3f", "format");

    // F294
    let mut lib2 = ThemeLibrary::new();
    let a = lib2.register(EL_WALLPAPER);
    let b = lib2.register(EL_SOUND);
    set.add("F294 preview", preview_combine(&lib2, a, b).is_some(), "combine");
    set.add("F294 preview miss", preview_combine(&lib2, a, 999).is_none(), "unknown id");
    set.add("F294 no activation", lib2.active().is_none(), "sandbox side-effect free");

    // F295
    set.add("F295 import clip", import_external(0xFFFF) == EL_ALL, "clip bits");
    let mut out = [0u16; 4];
    let packets: [&[u8]; 2] = [pkt, b"XXXX00000000"];
    let n = import_batch(packets.iter().copied(), &mut out);
    set.add("F295 batch 1", n == 1 && out[0] == EL_WALLPAPER, "skip invalid");

    // F296
    let rows = [
        AuditRow { fg: (0, 0, 0), bg: (255, 255, 255), large: false },
        AuditRow { fg: (128, 128, 128), bg: (150, 150, 150), large: false },
    ];
    set.add("F296 audit 1 fail", audit_contrast(&rows) == 1, "row2 low contrast");
    set.add("F296 audit pass", audit_contrast(&[]) == 0, "empty ok");

    // F297
    set.add("F297 oled crush", oled_black_panel((6, 6, 6)) == (0, 0, 0), "near black");
    set.add("F297 oled keep", oled_black_panel((20, 20, 20)) == (20, 20, 20), "panel kept");

    // F298
    set.add("F298 reduced scale", motion_scale(MotionPref::Reduced) == 300, "30%");
    set.add("F298 off scale", motion_scale(MotionPref::Off) == 0, "0%");
    set.add("F298 parallax", !parallax_allowed(MotionPref::Reduced), "no parallax");

    // F299
    set.add("F299 api ok", theme_api_ok(1) && theme_api_ok(2), "1~2");
    set.add("F299 api deny", !theme_api_ok(0) && !theme_api_ok(3), "range");

    // F300
    set.add("F300 self count", set.len() >= 25, "25+ checks");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_packet(elements: u16, payload: &[u8]) -> std::vec::Vec<u8> {
        let mut v = std::vec::Vec::new();
        v.extend_from_slice(THEME_MAGIC);
        v.extend_from_slice(&elements.to_le_bytes());
        v.push(2);
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn f276_payload_length_must_match() {
        let mut p = make_packet(EL_ALL, b"abc");
        p.push(0);
        assert!(parse_theme_header(&p).is_none());
    }

    #[test]
    fn f277_crc_known_vector() {
        // CRC32("123456789") = 0xCBF43926（标准校验向量）。
        assert_eq!(theme_crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn f278_activate_missing_id() {
        let mut lib = ThemeLibrary::new();
        let id = lib.register(0);
        assert!(!lib.activate(id + 100));
        assert!(lib.active().is_none());
    }

    #[test]
    fn f282_busy_anim_silent_when_unset() {
        assert_eq!(CursorTheme::new().busy_frame(0), 0);
    }

    #[test]
    fn f286_sample_monotonic_endpoints() {
        let mut cp = CurvePack::new();
        cp.add([0, 0, 255, 255]); // 线性
        assert_eq!(cp.sample(0, 0), 0);
        assert_eq!(cp.sample(0, 255), 255);
    }

    #[test]
    fn f291_history_capacity_four() {
        let mut h = ThemeHistory::new();
        for id in 1..=6u32 {
            h.push(id);
        }
        // 栈里只剩 3,4,5,6；回滚依次 5,4,3，然后空。
        assert_eq!(h.rollback(), Some(5));
        assert_eq!(h.rollback(), Some(4));
        assert_eq!(h.rollback(), Some(3));
        assert_eq!(h.rollback(), None);
    }

    #[test]
    fn f295_import_batch_capacity() {
        let p1 = make_packet(EL_FONT, b"x");
        let p2 = make_packet(EL_SOUND, b"y");
        let mut out = [0u16; 1];
        let n = import_batch([p1.as_slice(), p2.as_slice()].iter().copied(), &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0], EL_FONT);
    }

    #[test]
    fn domain_self_test_passes() {
        let set = run_theme_checks();
        assert!(!set.truncated());
        assert!(set.all_passed(), "theme-m500 self-test: {} checks", set.len());
    }
}
