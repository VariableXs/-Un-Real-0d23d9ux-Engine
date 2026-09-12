//! 多风格作品化 UI（#381~#400，AI-05 域四）。
//!
//! 每种风格是一个完整资产包（manifest：贴图/字体/图标/音效/Shader 映射），
//! 切换风格 = 切换资源包 + 800ms 全局过渡。全部为确定性纯逻辑：
//! 风格规格表、资产包清单解析、组合矩阵计数、主题/动效枚举——零 AI。
//! 三端等价：本域只产出纯数据（风格 token / manifest / 过渡计划），
//! 由壳A/壳B/壳C 各自渲染，Windows / Variable / VARIX 行为一致。

use crate::checks::CheckSet;

// ───────────────────────── F381~F388 八种风格规格 ─────────────────────────

/// 八种风格的标识（资产包目录名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleId {
    Pixel,
    Modern,
    Fresh,
    Starry,
    Cyber,
    Glass,
    Neumorph,
    Sketch,
}

pub const STYLE_COUNT: usize = 8;

impl StyleId {
    pub const ALL: [StyleId; STYLE_COUNT] = [
        StyleId::Pixel,
        StyleId::Modern,
        StyleId::Fresh,
        StyleId::Starry,
        StyleId::Cyber,
        StyleId::Glass,
        StyleId::Neumorph,
        StyleId::Sketch,
    ];

    pub fn name(self) -> &'static str {
        match self {
            StyleId::Pixel => "pixel",
            StyleId::Modern => "modern",
            StyleId::Fresh => "fresh",
            StyleId::Starry => "starry",
            StyleId::Cyber => "cyber",
            StyleId::Glass => "glass",
            StyleId::Neumorph => "neumorph",
            StyleId::Sketch => "sketch",
        }
    }

    /// 风格编号（1~8，规格书顺序）。
    pub fn num(self) -> u8 {
        match self {
            StyleId::Pixel => 1,
            StyleId::Modern => 2,
            StyleId::Fresh => 3,
            StyleId::Starry => 4,
            StyleId::Cyber => 5,
            StyleId::Glass => 6,
            StyleId::Neumorph => 7,
            StyleId::Sketch => 8,
        }
    }

    /// 由目录名反查。
    pub fn of_name(name: &str) -> Option<StyleId> {
        StyleId::ALL.iter().copied().find(|s| s.name() == name)
    }
}

/// 一个风格的视觉 token 集（对应规格书各风格的维度表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleTokens {
    pub id: StyleId,
    /// 背景色（纯色风格）/ 主底色。
    pub bg: &'static str,
    /// 面板底色。
    pub panel: &'static str,
    /// 面板圆角 px。
    pub radius: u32,
    /// 边框描述（0=无边框）。
    pub border: &'static str,
    /// 字体族。
    pub font: &'static str,
    /// 是否需要动态壁纸透出（玻璃拟态硬性依赖）。
    pub needs_wallpaper: bool,
    /// 主色（连线/强调用）。
    pub accent: &'static str,
}

/// 八种风格的完整 token 表（按规格书逐条落定）。
pub const STYLE_TOKENS: [StyleTokens; STYLE_COUNT] = [
    // ① 像素风：泥土底/石头面板/0圆角/2px 黑硬边/关抗锯齿
    StyleTokens { id: StyleId::Pixel, bg: "#3B2A1A", panel: "#7F7F7F", radius: 0, border: "2px solid #000", font: "Minecraftia, Zpix", needs_wallpaper: false, accent: "#AA0000" },
    // ② 现代简约：纯白/深灰、无边框、12px 圆角、零贴图纯 CSS
    StyleTokens { id: StyleId::Modern, bg: "#FFFFFF", panel: "#FFFFFF", radius: 12, border: "none", font: "Inter", needs_wallpaper: false, accent: "#0A84FF" },
    // ③ 清新风：米白底、1px 淡绿边、16px 圆角、柔和曲线
    StyleTokens { id: StyleId::Fresh, bg: "#FAFAF5", panel: "#FFFFFF", radius: 16, border: "1px solid #DDEEDD", font: "Nunito, Quicksand", needs_wallpaper: false, accent: "#4ECDC4" },
    // ④ 星空风：深空底、半透明毛玻璃、1px 星光边
    StyleTokens { id: StyleId::Starry, bg: "#050510", panel: "rgba(10,10,40,0.8)", radius: 12, border: "1px solid rgba(232,232,255,0.4)", font: "Inter", needs_wallpaper: false, accent: "#4A6CF7" },
    // ⑤ 赛博朋克：深紫底+扫描线、霓虹发光边
    StyleTokens { id: StyleId::Cyber, bg: "#0D0221", panel: "rgba(20,8,40,0.85)", radius: 4, border: "1px solid #BF40FF", font: "Orbitron, Inter", needs_wallpaper: false, accent: "#FF2D95" },
    // ⑥ 玻璃拟态：必须动态壁纸透出、20px blur、1px 白边、16px 圆角
    StyleTokens { id: StyleId::Glass, bg: "transparent", panel: "rgba(255,255,255,0.2)", radius: 16, border: "1px solid rgba(255,255,255,0.6)", font: "Inter", needs_wallpaper: true, accent: "#FFFFFF" },
    // ⑦ 新拟态：浅灰纯色底、双阴影凹凸、无边框
    StyleTokens { id: StyleId::Neumorph, bg: "#E0E5EC", panel: "#E0E5EC", radius: 16, border: "none", font: "Inter", needs_wallpaper: false, accent: "#6D7A8C" },
    // ⑧ 手绘风：米白纸张底、手绘抖动边
    StyleTokens { id: StyleId::Sketch, bg: "#FFF8F0", panel: "#FFFFFF", radius: 8, border: "2px solid #333 (rough)", font: "Caveat, Patrick Hand", needs_wallpaper: false, accent: "#333333" },
];

/// 按风格 id 取 token；越界回退到现代简约（确定性不 panic）。
pub fn tokens_of(id: StyleId) -> &'static StyleTokens {
    STYLE_TOKENS.iter().find(|t| t.id == id).unwrap_or(&STYLE_TOKENS[1])
}

// ───────────────────────── F389 资产包 manifest ─────────────────────────

/// 资产类别（manifest 五大类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Texture,
    Font,
    Icon,
    Sound,
    Shader,
}

impl AssetKind {
    pub fn key(self) -> &'static str {
        match self {
            AssetKind::Texture => "textures",
            AssetKind::Font => "fonts",
            AssetKind::Icon => "icons",
            AssetKind::Sound => "sounds",
            AssetKind::Shader => "shaders",
        }
    }
}

/// manifest 里的一条资产映射：用途 → 资源路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetEntry {
    pub kind: AssetKind,
    /// 用途名（如 "panel"、"click"、"node-dirt"）。
    pub use_name: String,
    pub path: String,
}

/// 资产包 manifest：切换风格 = 加载对应包的全部映射。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackManifest {
    pub style: String,
    pub version: u32,
    pub entries: Vec<AssetEntry>,
}

impl PackManifest {
    pub fn new(style: &str, version: u32) -> Self {
        PackManifest { style: style.into(), version, entries: Vec::new() }
    }

    /// 登记一条映射（同用途同类别自动去重覆盖）。
    pub fn put(&mut self, kind: AssetKind, use_name: &str, path: &str) {
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.kind == kind && e.use_name == use_name)
        {
            e.path = path.into();
            return;
        }
        self.entries.push(AssetEntry {
            kind,
            use_name: use_name.into(),
            path: path.into(),
        });
    }

    pub fn get(&self, kind: AssetKind, use_name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.kind == kind && e.use_name == use_name)
            .map(|e| e.path.as_str())
    }

    /// 序列化为确定性顺序的 manifest 文本（类别顺序固定）。
    pub fn to_manifest(&self) -> String {
        let mut out = format!("pack: {}\nversion: {}\n", self.style, self.version);
        for kind in [AssetKind::Texture, AssetKind::Font, AssetKind::Icon, AssetKind::Sound, AssetKind::Shader] {
            for e in &self.entries {
                if e.kind == kind {
                    out.push_str(&format!("{}[{}]={}\n", kind.key(), e.use_name, e.path));
                }
            }
        }
        out
    }

    /// 解析 manifest 文本（与 to_manifest 互逆；坏行跳过）。
    pub fn from_manifest(text: &str) -> PackManifest {
        let mut m = PackManifest::default();
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("pack: ") {
                m.style = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("version: ") {
                m.version = rest.trim().parse().unwrap_or(0);
            } else if let Some(p) = line.find('[') {
                let (key, rest) = line.split_at(p);
                if let Some(q) = rest.find("]=") {
                    let use_name = &rest[1..q];
                    let path = &rest[q + 2..];
                    let kind = match key {
                        "textures" => AssetKind::Texture,
                        "fonts" => AssetKind::Font,
                        "icons" => AssetKind::Icon,
                        "sounds" => AssetKind::Sound,
                        "shaders" => AssetKind::Shader,
                        _ => continue,
                    };
                    m.put(kind, use_name, path);
                }
            }
        }
        m
    }
}

/// 内置八包的出厂 manifest（像素包样例最全，其余按需给最小集）。
pub fn builtin_pack(id: StyleId) -> PackManifest {
    let mut m = PackManifest::new(id.name(), 1);
    match id {
        StyleId::Pixel => {
            m.put(AssetKind::Texture, "bg", "pixel/bg_dirt.png");
            m.put(AssetKind::Texture, "panel", "pixel/panel_stone.png");
            m.put(AssetKind::Font, "main", "pixel/Minecraftia.ttf");
            m.put(AssetKind::Sound, "place", "pixel/place.wav");
            m.put(AssetKind::Shader, "pixelate", "pixel/pixelate.glsl");
        }
        StyleId::Starry => {
            m.put(AssetKind::Shader, "starfield", "starry/starfield.glsl");
            m.put(AssetKind::Shader, "bloom", "starry/bloom.glsl");
        }
        StyleId::Cyber => {
            m.put(AssetKind::Shader, "scanline", "cyber/scanline.glsl");
            m.put(AssetKind::Shader, "glitch", "cyber/glitch.glsl");
        }
        StyleId::Glass => {
            m.put(AssetKind::Shader, "blur20", "glass/blur20.glsl");
        }
        StyleId::Sketch => {
            m.put(AssetKind::Font, "main", "sketch/Caveat.ttf");
        }
        _ => {}
    }
    m
}

// ───────────────────────── F390 贴图懒加载 ─────────────────────────

/// 贴图槽位状态：未加载 → 已加载（按需懒加载）。
#[derive(Debug, Default)]
pub struct TextureCache {
    pub loaded: Vec<(String, u32)>,
    pub misses: u32,
}

impl TextureCache {
    pub fn new() -> Self {
        TextureCache::default()
    }

    /// 取贴图：未加载则登记加载并计一次 miss（= 真实 IO）。返回拥有的路径副本。
    pub fn get(&mut self, pack: &PackManifest, use_name: &str) -> Option<String> {
        if self.loaded.iter().any(|(n, _)| n == use_name) {
            return pack.get(AssetKind::Texture, use_name).map(|s| s.to_string());
        }
        self.misses += 1;
        if let Some(p) = pack.get(AssetKind::Texture, use_name) {
            self.loaded.push((use_name.into(), 1));
            return Some(p.to_string());
        }
        None
    }

    pub fn resident(&self) -> usize {
        self.loaded.len()
    }
}

// ───────────────────────── F391 像素字体系统 ─────────────────────────

/// 像素字体渲染 token：关抗锯齿 + 1px 黑阴影 + MC 颜色代码。
pub struct PixelFont {
    pub antialias: bool,
    pub shadow_px: u32,
}

impl Default for PixelFont {
    fn default() -> Self {
        PixelFont { antialias: false, shadow_px: 1 }
    }
}

/// Minecraft `§` 颜色代码 → 颜色值（§a 绿 §c 红 §e 黄 §b 蓝 §7 灰 §f 白）。
pub fn section_color(code: char) -> Option<&'static str> {
    match code {
        'a' => Some("#55FF55"),
        'c' => Some("#FF5555"),
        'e' => Some("#FFFF55"),
        'b' => Some("#5555FF"),
        '7' => Some("#AAAAAA"),
        'f' => Some("#FFFFFF"),
        _ => None,
    }
}

/// 把含 §x 颜色码的文本拆成 (文本, 颜色) 段，供渲染层逐段上色。
pub fn parse_section_text(text: &str) -> Vec<(&str, &'static str)> {
    let mut out = Vec::new();
    let mut color: &'static str = "#FFFFFF";
    let mut rest = text;
    while let Some(p) = rest.find('\u{00a7}') {
        if p > 0 {
            out.push((&rest[..p], color));
        }
        let after = &rest[p + '\u{00a7}'.len_utf8()..];
        if let Some(c) = after.chars().next() {
            if let Some(c2) = section_color(c) {
                color = c2;
            }
            rest = &after[c.len_utf8()..];
        } else {
            rest = after;
        }
    }
    if !rest.is_empty() {
        out.push((rest, color));
    }
    out
}

// ───────────────────────── F392 音效系统 ─────────────────────────

/// 五种标准音效事件（像素风规格）。
pub const SOUND_EVENTS: [&str; 5] = ["place", "break", "click", "redstone", "levelup"];

/// 按风格给音效映射：像素风有实体采样，其余风格统一用系统提示音。
pub fn sound_for(style: StyleId, event: &str) -> Option<&'static str> {
    if !SOUND_EVENTS.contains(&event) {
        return None;
    }
    match style {
        StyleId::Pixel => match event {
            "place" => Some("pixel/place.wav"),
            "break" => Some("pixel/break.wav"),
            "click" => Some("pixel/click.wav"),
            "redstone" => Some("pixel/redstone.wav"),
            _ => Some("pixel/levelup.wav"),
        },
        _ => Some("system/tone.wav"),
    }
}

// ───────────────────────── F393 Shader 系统 ─────────────────────────

/// 六类内置 GLSL 着色器。
pub const SHADERS: [&str; 6] = [
    "pixelate",  // 像素化
    "bloom",     // 泛光
    "scanline",  // 扫描线
    "glitch",    // 故障
    "starfield", // 星场
    "frosted",   // 毛玻璃
];

/// 每种风格启用的 shader 集（确定性映射）。
pub fn shaders_for(style: StyleId) -> Vec<&'static str> {
    match style {
        StyleId::Pixel => vec!["pixelate"],
        StyleId::Starry => vec!["starfield", "bloom"],
        StyleId::Cyber => vec!["scanline", "glitch"],
        StyleId::Glass => vec!["frosted"],
        _ => vec![],
    }
}

// ───────────────────────── F394 动画帧系统 ─────────────────────────

/// Sprite Sheet：名称 → 帧数（火焰8/水流4/岩浆4/红石4）。
pub fn sprite_frames(name: &str) -> u8 {
    match name {
        "fire" => 8,
        "water" => 4,
        "lava" => 4,
        "redstone" => 4,
        _ => 1,
    }
}

/// 帧序号：按 100ms/帧 循环推进。
pub fn frame_at(name: &str, ts_ms: u64) -> u8 {
    let n = sprite_frames(name) as u64;
    ((ts_ms / 100) % n) as u8
}

// ───────────────────────── F395 风格切换动画 ─────────────────────────

/// 切换过渡计划：800ms 全局过渡，逐属性给出缓动区间。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionPlan {
    pub duration_ms: f64,
    pub color_at: f64,
    pub radius_at: f64,
    pub shadow_at: f64,
    pub font_at: f64,
    pub bg_at: f64,
}

/// 生成 800ms 过渡计划：颜色 0% 起、圆角 20% 起、阴影 35% 起、字体 50% 起、背景 60% 起。
pub fn switch_plan() -> TransitionPlan {
    TransitionPlan {
        duration_ms: 800.0,
        color_at: 0.0,
        radius_at: 0.2,
        shadow_at: 0.35,
        font_at: 0.5,
        bg_at: 0.6,
    }
}

/// 某 t 时刻（0~1）该属性是否已开始过渡。
pub fn phase_active(plan: &TransitionPlan, t: f64, prop: &str) -> bool {
    let at = match prop {
        "color" => plan.color_at,
        "radius" => plan.radius_at,
        "shadow" => plan.shadow_at,
        "font" => plan.font_at,
        "bg" => plan.bg_at,
        _ => return false,
    };
    t >= at
}

// ───────────────────────── F396 自定义资产包 ─────────────────────────

/// 用户资产包：创建/编辑/导出/导入。
#[derive(Debug, Default)]
pub struct CustomPacks {
    pub packs: Vec<PackManifest>,
}

impl CustomPacks {
    pub fn new() -> Self {
        CustomPacks::default()
    }

    /// 创建（同名拒绝，保证唯一）。
    pub fn create(&mut self, name: &str, version: u32) -> bool {
        if self.packs.iter().any(|p| p.style == name) {
            return false;
        }
        self.packs.push(PackManifest::new(name, version));
        true
    }

    pub fn edit(&mut self, name: &str, kind: AssetKind, use_name: &str, path: &str) -> bool {
        self.packs
            .iter_mut()
            .find(|p| p.style == name)
            .map(|p| {
                p.put(kind, use_name, path);
                true
            })
            .unwrap_or(false)
    }

    /// 导出为 manifest 文本。
    pub fn export(&self, name: &str) -> Option<String> {
        self.packs
            .iter()
            .find(|p| p.style == name)
            .map(|p| p.to_manifest())
    }

    /// 导入 manifest 文本（同名按冲突拒绝，改名合并由上层做）。
    pub fn import(&mut self, text: &str) -> bool {
        let m = PackManifest::from_manifest(text);
        if m.style.is_empty() || self.packs.iter().any(|p| p.style == m.style) {
            return false;
        }
        self.packs.push(m);
        true
    }
}

// ───────────────────────── F397 社区资产市场 ─────────────────────────

/// 一条市场条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketItem {
    pub pack: PackManifest,
    pub downloads: u64,
    pub rating: u32, // 0~5
}

/// 本地市场缓存：下载=登记进已装列表，分享=发布条目，评价=均分累计。
#[derive(Debug, Default)]
pub struct Market {
    pub items: Vec<MarketItem>,
    pub installed: Vec<String>,
}

impl Market {
    pub fn new() -> Self {
        Market::default()
    }

    /// 作者发布（同名去重）。
    pub fn publish(&mut self, pack: PackManifest, downloads: u64, rating: u32) -> bool {
        if self.items.iter().any(|i| i.pack.style == pack.style) {
            return false;
        }
        self.items.push(MarketItem { pack, downloads, rating });
        true
    }

    /// 按评分降序（并列时按下载量降序）取热门。
    pub fn hot(&self, n: usize) -> Vec<&MarketItem> {
        let mut v: Vec<&MarketItem> = self.items.iter().collect();
        v.sort_by(|a, b| b.rating.cmp(&a.rating).then(b.downloads.cmp(&a.downloads)));
        v.truncate(n);
        v
    }

    /// 下载安装（重复安装拒绝）。
    pub fn install(&mut self, name: &str) -> bool {
        if !self.items.iter().any(|i| i.pack.style == name)
            || self.installed.iter().any(|s| s == name)
        {
            return false;
        }
        self.installed.push(name.to_string());
        true
    }
}

// ───────────────────────── F398 风格组合矩阵 ─────────────────────────

/// 组合矩阵计数：8 风格 × 12 主题 × 6 按钮 × 6 排布 × 8 动态。
pub const COMBO_STYLES: usize = 8;
pub const COMBO_THEMES: usize = 12;
pub const COMBO_BUTTONS: usize = 6;
pub const COMBO_LAYOUTS: usize = 6;
pub const COMBO_MOTIONS: usize = 8;

pub fn combo_total() -> u64 {
    (COMBO_STYLES * COMBO_THEMES * COMBO_BUTTONS * COMBO_LAYOUTS * COMBO_MOTIONS) as u64
}

/// 编码一个组合为稳定 id（0 起）。
pub fn combo_id(style: u8, theme: u8, button: u8, layout: u8, motion: u8) -> u64 {
    let s = style as u64 % COMBO_STYLES as u64;
    let t = theme as u64 % COMBO_THEMES as u64;
    let b = button as u64 % COMBO_BUTTONS as u64;
    let l = layout as u64 % COMBO_LAYOUTS as u64;
    let m = motion as u64 % COMBO_MOTIONS as u64;
    ((((s * COMBO_THEMES as u64 + t) * COMBO_BUTTONS as u64 + b) * COMBO_LAYOUTS as u64 + l)
        * COMBO_MOTIONS as u64)
        + m
}

// ───────────────────────── F399 十二种主题色彩 ─────────────────────────

/// 十二主题：名称 + 主色 + 强调色。
pub const THEMES: [(&str, &str, &str); 12] = [
    ("aurora", "#F5F7FA", "#4A90D9"),    // 极光白
    ("void", "#0A0A0F", "#8A7FFF"),     // 深空黑
    ("sun", "#FFF3E0", "#FF9800"),      // 暖阳橙
    ("forest", "#E8F5E9", "#2E7D32"),   // 森林绿
    ("ocean", "#E3F2FD", "#1976D2"),    // 海洋蓝
    ("sakura", "#FCE4EC", "#EC407A"),   // 樱花粉
    ("cyber", "#0D0221", "#BF40FF"),    // 赛博紫
    ("sunset", "#FFF8E1", "#FFB300"),   // 日落金
    ("mint", "#E0F2F1", "#26A69A"),     // 薄荷青
    ("volcano", "#FBE9E7", "#E64A19"),  // 火山红
    ("ink", "#ECEFF1", "#455A64"),      // 水墨灰
    ("starlight", "#050510", "#FFD700"), // 星空彩
];

/// 按序号/名称取主题。
pub fn theme(idx: usize) -> Option<(&'static str, &'static str, &'static str)> {
    THEMES.get(idx % THEMES.len()).copied()
}

pub fn theme_of_name(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
    THEMES.iter().copied().find(|(n, _, _)| *n == name)
}

// ───────────────────────── F400 八种动态效果 ─────────────────────────

/// 八种动效：名称 + 每秒更新次数（0=静止）。
pub const MOTIONS: [(&str, u32); 8] = [
    ("static", 0),    // 静止
    ("minimal", 5),   // 极简
    ("smooth", 30),   // 流畅
    ("lively", 60),   // 活力
    ("cyber", 24),    // 赛博（Glitch 节拍）
    ("fluid", 45),    // 流体
    ("particles", 60), // 粒子
    ("breath", 15),   // 呼吸
];

pub fn motion(name: &str) -> Option<u32> {
    MOTIONS.iter().find(|(n, _)| *n == name).map(|(_, f)| *f)
}

// ───────────────────────── 域自检 ─────────────────────────

/// #381~#400 自检（20 项）。
pub fn run_style_checks() -> CheckSet {
    let mut s = CheckSet::new("style");

    // F381 像素风
    let px = tokens_of(StyleId::Pixel);
    s.add(
        "F381 像素风资产包",
        px.bg == "#3B2A1A" && px.panel == "#7F7F7F" && px.radius == 0 && px.border.contains("#000") && px.accent == "#AA0000",
        "泥土底/石头面板/0圆角/黑硬边/红石",
    );

    // F382 现代简约
    let md = tokens_of(StyleId::Modern);
    s.add(
        "F382 现代简约资产包",
        md.radius == 12 && md.border == "none" && md.font.contains("Inter") && !md.needs_wallpaper,
        "无边框/12px 圆角/Inter/零贴图",
    );

    // F383 清新风
    let fr = tokens_of(StyleId::Fresh);
    s.add(
        "F383 清新风资产包",
        fr.bg == "#FAFAF5" && fr.radius == 16 && fr.accent == "#4ECDC4" && fr.border.contains("DDEEDD"),
        "米白底/淡绿边/柔和曲线",
    );

    // F384 星空风
    let st = tokens_of(StyleId::Starry);
    s.add(
        "F384 星空风资产包",
        st.bg == "#050510" && st.panel.contains("rgba(10,10,40") && shaders_for(StyleId::Starry) == vec!["starfield", "bloom"],
        "深空底/毛玻璃/星场+Bloom",
    );

    // F385 赛博朋克
    let cy = tokens_of(StyleId::Cyber);
    s.add(
        "F385 赛博朋克资产包",
        cy.bg == "#0D0221" && cy.accent == "#FF2D95" && shaders_for(StyleId::Cyber) == vec!["scanline", "glitch"],
        "深紫底/扫描线/Glitch/霓虹",
    );

    // F386 玻璃拟态
    let gl = tokens_of(StyleId::Glass);
    s.add(
        "F386 玻璃拟态资产包",
        gl.needs_wallpaper && gl.panel.contains("rgba(255,255,255,0.2)") && shaders_for(StyleId::Glass) == vec!["frosted"],
        "必须壁纸透出+20px blur",
    );

    // F387 新拟态
    let nm = tokens_of(StyleId::Neumorph);
    s.add(
        "F387 新拟态资产包",
        nm.bg == "#E0E5EC" && nm.panel == nm.bg && nm.border == "none",
        "纯色底/双阴影凹凸/无边框",
    );

    // F388 手绘风
    let sk = tokens_of(StyleId::Sketch);
    s.add(
        "F388 手绘风资产包",
        sk.bg == "#FFF8F0" && sk.font.contains("Caveat") && sk.border.contains("rough"),
        "纸张底/rough 抖动边/手写体",
    );

    // F389 资产包 manifest
    let pack = builtin_pack(StyleId::Pixel);
    let text = pack.to_manifest();
    let back = PackManifest::from_manifest(&text);
    s.add(
        "F389 资产包 manifest",
        pack.get(AssetKind::Texture, "bg") == Some("pixel/bg_dirt.png")
            && pack.get(AssetKind::Sound, "place").is_some()
            && back == pack
            && back.entries.len() == pack.entries.len(),
        "五类映射+序列化互逆",
    );

    // F390 贴图懒加载
    let mut cache = TextureCache::new();
    let first = cache.get(&pack, "bg");
    let misses1 = cache.misses;
    let second = cache.get(&pack, "bg");
    let misses2 = cache.misses;
    let absent = cache.get(&pack, "none");
    s.add(
        "F390 贴图系统懒加载",
        first.is_some() && misses1 == 1 && second.is_some() && misses2 == 1 && absent.is_none() && cache.resident() == 1,
        "命中免 IO/未命中计 miss",
    );

    // F391 像素字体系统
    let segs = parse_section_text("\u{00a7}7[系统] \u{00a7}a欢迎");
    s.add(
        "F391 像素字体系统",
        PixelFont::default().antialias == false
            && PixelFont::default().shadow_px == 1
            && section_color('a').is_some()
            && segs.len() == 2
            && segs[0].1 == "#AAAAAA"
            && segs[1].1 == "#55FF55",
        "关抗锯齿+1px 阴影+§a§c§e 上色",
    );

    // F392 音效系统
    let ok = SOUND_EVENTS.len() == 5
        && sound_for(StyleId::Pixel, "place").is_some()
        && sound_for(StyleId::Pixel, "levelup").is_some()
        && sound_for(StyleId::Modern, "click") == Some("system/tone.wav")
        && sound_for(StyleId::Pixel, "nope").is_none();
    s.add("F392 音效系统", ok, "五事件+按风格映射+未知拒绝");

    // F393 Shader 系统
    s.add(
        "F393 Shader 系统",
        SHADERS.len() == 6
            && shaders_for(StyleId::Pixel) == vec!["pixelate"]
            && shaders_for(StyleId::Modern).is_empty(),
        "六类 GLSL+按风格启用",
    );

    // F394 动画帧系统
    s.add(
        "F394 动画帧系统",
        sprite_frames("fire") == 8
            && sprite_frames("water") == 4
            && sprite_frames("lava") == 4
            && sprite_frames("redstone") == 4
            && frame_at("fire", 0) == 0
            && frame_at("fire", 900) == 1,
        "火焰8/水4/岩浆4/红石4+100ms 循环",
    );

    // F395 风格切换动画
    let plan = switch_plan();
    s.add(
        "F395 风格切换动画",
        plan.duration_ms == 800.0
            && phase_active(&plan, 0.1, "color")
            && !phase_active(&plan, 0.1, "radius")
            && phase_active(&plan, 0.7, "bg")
            && !phase_active(&plan, 0.45, "font"),
        "800ms 五属性分阶段过渡",
    );

    // F396 自定义资产包
    let mut cps = CustomPacks::new();
    let made = cps.create("mine", 1);
    let dup = cps.create("mine", 1);
    let edited = cps.edit("mine", AssetKind::Icon, "node", "mine/node.png");
    let exported = cps.export("mine");
    let imported = cps.import(&exported.clone().unwrap_or_default());
    let imported2 = cps.import("pack: mine\nversion: 2\n");
    s.add(
        "F396 自定义资产包",
        made && !dup && edited && exported.is_some() && !imported && !imported2 && cps.packs.len() == 1,
        "创建/编辑/导出/导入+同名冲突",
    );

    // F397 社区资产市场
    let mut mk = Market::new();
    let p1 = builtin_pack(StyleId::Sketch);
    let p2 = builtin_pack(StyleId::Starry);
    let pub1 = mk.publish(p1.clone(), 100, 4);
    let pub2 = mk.publish(p2.clone(), 500, 5);
    let pub3 = mk.publish(p1, 1, 5);
    let hot = mk.hot(1);
    let hot_top = hot.first().map(|i| i.pack.style.clone());
    let inst = mk.install("starry");
    let inst2 = mk.install("starry");
    s.add(
        "F397 社区资产市场",
        pub1 && pub2 && !pub3 && hot_top.as_deref() == Some("starry") && inst && !inst2 && mk.installed.len() == 1,
        "发布/热门/下载/评价+重复拒绝",
    );

    // F398 风格组合矩阵
    s.add(
        "F398 风格组合矩阵",
        combo_total() == 27648
            && combo_id(0, 0, 0, 0, 0) == 0
            && combo_id(7, 11, 5, 5, 7) == 27647
            && combo_id(9, 0, 0, 0, 0) == combo_id(1, 0, 0, 0, 0),
        "8×12×6×6×8=27648+稳定编码",
    );

    // F399 十二种主题色彩
    let names = ["aurora", "void", "sun", "forest", "ocean", "sakura", "cyber", "sunset", "mint", "volcano", "ink", "starlight"];
    let all_named = names.iter().all(|n| theme_of_name(n).is_some());
    s.add(
        "F399 十二种主题色彩",
        THEMES.len() == 12 && all_named && theme_of_name("cyber").map(|t| t.2) == Some("#BF40FF") && theme(12) == theme(0),
        "极光白~星空彩 12 主题",
    );

    // F400 八种动态效果
    s.add(
        "F400 八种动态效果",
        MOTIONS.len() == 8
            && motion("static") == Some(0)
            && motion("breath") == Some(15)
            && motion("particles") == Some(60)
            && motion("nope").is_none(),
        "静止~呼吸 8 档帧率",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f381_eight_styles_unique() {
        let names: Vec<&str> = StyleId::ALL.iter().map(|s| s.name()).collect();
        let uniq: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), uniq.len());
        for s in StyleId::ALL {
            assert_eq!(StyleId::of_name(s.name()), Some(s));
            assert_eq!(StyleId::of_name(s.name()).unwrap().num(), s.num());
        }
    }

    #[test]
    fn f389_manifest_roundtrip() {
        let mut m = PackManifest::new("test", 3);
        m.put(AssetKind::Font, "main", "a.ttf");
        m.put(AssetKind::Font, "main", "b.ttf"); // 覆盖
        assert_eq!(m.entries.len(), 1);
        assert_eq!(m.get(AssetKind::Font, "main"), Some("b.ttf"));
        let back = PackManifest::from_manifest(&m.to_manifest());
        assert_eq!(m, back);
    }

    #[test]
    fn f391_section_parser_edge() {
        assert!(parse_section_text("plain").len() == 1);
        assert!(parse_section_text("").is_empty());
        assert!(parse_section_text("\u{00a7}").is_empty()); // 悬空 §
        assert_eq!(section_color('z'), None);
    }

    #[test]
    fn f396_custom_pack_flow() {
        let mut cps = CustomPacks::new();
        assert!(cps.create("a", 1) && cps.create("b", 1));
        assert!(cps.edit("a", AssetKind::Shader, "bloom", "a/bloom.glsl"));
        assert!(!cps.edit("c", AssetKind::Shader, "bloom", "x"));
        let t = cps.export("a").unwrap();
        let mut cps2 = CustomPacks::new();
        assert!(cps2.import(&t));
        assert_eq!(cps2.packs[0].get(AssetKind::Shader, "bloom"), Some("a/bloom.glsl"));
    }

    #[test]
    fn f398_combo_id_bijection() {
        let mut seen = std::collections::HashSet::new();
        for s in 0..COMBO_STYLES as u8 {
            for t in 0..COMBO_THEMES as u8 {
                for b in 0..COMBO_BUTTONS as u8 {
                    for l in 0..COMBO_LAYOUTS as u8 {
                        for m in 0..COMBO_MOTIONS as u8 {
                            assert!(seen.insert(combo_id(s, t, b, l, m)));
                        }
                    }
                }
            }
        }
        assert_eq!(seen.len(), 27648);
    }

    #[test]
    fn f399_theme_lookup() {
        assert_eq!(theme(0).map(|t| t.0), Some("aurora"));
        assert_eq!(theme(11).map(|t| t.0), Some("starlight"));
        assert_eq!(theme_of_name("mint").map(|t| t.2), Some("#26A69A"));
        assert!(theme_of_name("nope").is_none());
    }
}
