//! NOVA-ASSETS · src 视觉资产体系 → 内核移植（第二批）。
//!
//! 覆盖 src 四个视觉资产子系统，令牌之外的"资产层"全部 1:1 在内核在位：
//! 1. iconpacks/registry.ts —— 图标包注册表（单包模型 / 回退计数 / 卸载还原）
//! 2. iconpacks/vicon.ts —— .vicon 清单校验（格式/版本/大小/键名/资源形态）
//! 3. desktop-design/ai11-icons.ts —— 图标栅格与密度 25 档（f0053 实测几何值）
//! 4. desktop-design/ai12-wallpaper.ts —— 壁纸引擎 25 档目录（f0056）
//! 5. start.md C-7 4K 资产管线 —— 母版尺寸/多档派生/禁放大红线
//!
//! 纯逻辑 + 固定容量：no_std / 仅 core，无分配、无 unsafe。

use crate::checks::CheckSet;

// ===========================================================================
// 1. .vicon 清单规范（iconpacks/vicon.ts 1:1）
// ===========================================================================
pub const VICON_FORMAT: &[u8] = b"vicon";
pub const VICON_VERSION: u32 = 1;
pub const VICON_MAX_BYTES: usize = 5 * 1024 * 1024;
pub const VICON_RESOURCE_MAX: usize = 512 * 1024;

/// 资源形态：内联 SVG（以 "<svg" 开头）或 PNG dataURL（data:image/ 开头）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResourceKind {
    InlineSvg,
    PngDataUrl,
    Invalid,
}

pub fn classify_resource(s: &[u8]) -> ResourceKind {
    if s.is_empty() || s.len() > VICON_RESOURCE_MAX {
        return ResourceKind::Invalid;
    }
    let t = trim_start_ascii(s);
    if t.starts_with(b"<svg") {
        ResourceKind::InlineSvg
    } else if t.starts_with(b"data:image/") {
        ResourceKind::PngDataUrl
    } else {
        ResourceKind::Invalid
    }
}

fn trim_start_ascii(s: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || s[i] == b'\t' || s[i] == b'\r' || s[i] == b'\n') {
        i += 1;
    }
    &s[i..]
}

/// 键规范校验：区域段 + 名称（首段可带点），ASCII 字母数字与 . _ - 空格。
/// 对应 VICON_KEY_RE = /^[a-z0-9][a-z0-9_.-]*:\.?[a-z0-9][a-z0-9_. -]*$/i。
pub fn valid_icon_key(key: &str) -> bool {
    let b = key.as_bytes();
    let colon = match b.iter().position(|&c| c == b':') {
        Some(i) => i,
        None => return false,
    };
    let (area, name) = (&b[..colon], &b[colon + 1..]);
    fn seg_ok(seg: &[u8], first_dot_ok: bool) -> bool {
        if seg.is_empty() {
            return false;
        }
        // 可选前导点（仅名称段允许 .md 形态）
        let mut rest = seg;
        if rest[0] == b'.' {
            if !first_dot_ok {
                return false;
            }
            rest = &rest[1..];
        }
        // 点后首字符必须字母数字，其余在允许集合内
        match rest.split_first() {
            Some((&f, tail)) => {
                f.is_ascii_alphanumeric()
                    && tail
                        .iter()
                        .all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-' | b' '))
            }
            None => false,
        }
    }
    seg_ok(area, false) && seg_ok(name, true)
}

// ===========================================================================
// 2. 图标包注册表（iconpacks/registry.ts：单包模型 + 回退计数 + 卸载还原）
// ===========================================================================
pub const MAX_ICONPACKS: usize = 4;
pub const MAX_ICONS_PER_PACK: usize = 16;
pub const MAX_KEY_LEN: usize = 48;
pub const MAX_RES_LEN: usize = 256; // 内核侧仅存元数据级资源引用（完整资产走 4K 图集文件，禁内联大资产）

#[derive(Clone, Copy)]
pub struct IconEntry {
    pub key: [u8; MAX_KEY_LEN],
    pub key_len: usize,
    pub res: [u8; MAX_RES_LEN],
    pub res_len: usize,
}

impl IconEntry {
    pub const EMPTY: IconEntry = IconEntry { key: [0; MAX_KEY_LEN], key_len: 0, res: [0; MAX_RES_LEN], res_len: 0 };
    pub fn key(&self) -> &[u8] {
        &self.key[..self.key_len]
    }
    pub fn res(&self) -> &[u8] {
        &self.res[..self.res_len]
    }
}

#[derive(Clone, Copy)]
pub struct IconPack {
    pub id: [u8; 32],
    pub id_len: usize,
    pub version: u32,
    pub icons: [IconEntry; MAX_ICONS_PER_PACK],
    pub icon_count: usize,
}

pub struct IconPackRegistry {
    packs: [Option<IconPack>; MAX_ICONPACKS],
    pack_count: usize,
    fallback_count: u32,
}

impl IconPackRegistry {
    pub const fn new() -> IconPackRegistry {
        IconPackRegistry { packs: [None; MAX_ICONPACKS], pack_count: 0, fallback_count: 0 }
    }

    /// 安装包：后装覆盖先装（单包生效模型与 src 一致——activePack 取末位）。
    pub fn install(&mut self, id: &[u8], version: u32, icons: &[IconEntry]) -> bool {
        if id.is_empty() || id.len() > 32 || icons.len() > MAX_ICONS_PER_PACK {
            return false;
        }
        // 同 id 覆盖
        let mut slot: Option<usize> = None;
        for i in 0..self.pack_count {
            if let Some(p) = &self.packs[i] {
                if &p.id[..p.id_len] == id {
                    slot = Some(i);
                    break;
                }
            }
        }
        let idx = match slot {
            Some(i) => i,
            None => {
                if self.pack_count >= MAX_ICONPACKS {
                    return false;
                }
                let i = self.pack_count;
                self.pack_count += 1;
                i
            }
        };
        let mut pack = IconPack {
            id: [0; 32],
            id_len: id.len(),
            version,
            icons: [IconEntry::EMPTY; MAX_ICONS_PER_PACK],
            icon_count: icons.len(),
        };
        pack.id[..id.len()].copy_from_slice(id);
        for (i, e) in icons.iter().enumerate() {
            pack.icons[i] = IconEntry { key: e.key, key_len: e.key_len, res: e.res, res_len: e.res_len };
        }
        self.packs[idx] = Some(pack);
        true
    }

    /// 卸载即全部还原（整体删除，无残留）。
    pub fn uninstall(&mut self, id: &[u8]) -> bool {
        for i in 0..self.pack_count {
            let hit = matches!(&self.packs[i], Some(p) if &p.id[..p.id_len] == id);
            if hit {
                self.packs[i] = None;
                // 压缩空洞，保持顺序
                let mut j = i;
                while j + 1 < self.pack_count {
                    self.packs[j] = self.packs[j + 1].take();
                    j += 1;
                }
                self.pack_count -= 1;
                return true;
            }
        }
        false
    }

    /// 当前生效包 = 最后安装（单包模型）。
    pub fn active_pack(&self) -> Option<&IconPack> {
        if self.pack_count == 0 {
            None
        } else {
            self.packs[self.pack_count - 1].as_ref()
        }
    }

    /// 查询：命中返回资源切片；未命中 null 并回退计数（回退纪律验收口径）。
    pub fn get_icon_override(&mut self, key: &[u8]) -> Option<&[u8]> {
        if let Some(p) = self.active_pack() {
            for i in 0..p.icon_count {
                if p.icons[i].key() == key {
                    let n = self.pack_count;
                    let _ = n;
                    // 借用拆分：先取索引再返回
                    let pack = self.packs[self.pack_count - 1].as_ref()?;
                    return Some(&pack.icons[i].res[..pack.icons[i].res_len]);
                }
            }
        }
        self.fallback_count += 1;
        None
    }

    pub fn fallback_count(&self) -> u32 {
        self.fallback_count
    }
    pub fn pack_count(&self) -> usize {
        self.pack_count
    }
}

// ===========================================================================
// 3. 图标栅格与密度（ai11-icons.ts f0053 实测几何值）
// ===========================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IconGrid {
    /// 格子宽/高（design px）
    pub tile_w: u32,
    pub tile_h: u32,
    /// 图标本体 px
    pub icon_px: u32,
    pub gap_x: u32,
    pub gap_y: u32,
    /// 瀑布模式（waterfall）
    pub waterfall: bool,
}

/// f0053 已命名档位（与 src entry 几何一一对应）。
pub const GRID_BASE24: IconGrid = IconGrid { tile_w: 84, tile_h: 96, icon_px: 24, gap_x: 12, gap_y: 12, waterfall: false };
pub const GRID_D4: IconGrid = IconGrid { tile_w: 84, tile_h: 98, icon_px: 32, gap_x: 12, gap_y: 12, waterfall: false };
pub const GRID_SUPERDENSE: IconGrid = IconGrid { tile_w: 72, tile_h: 84, icon_px: 20, gap_x: 8, gap_y: 8, waterfall: false };
pub const GRID_WALL: IconGrid = IconGrid { tile_w: 168, tile_h: 188, icon_px: 96, gap_x: 16, gap_y: 16, waterfall: false };
pub const GRID_WATERFALL: IconGrid = IconGrid { tile_w: 120, tile_h: 150, icon_px: 56, gap_x: 10, gap_y: 10, waterfall: true };

/// 自适应间距（auto-gap：base 12，每 10 枚 -1，下限 6）。
pub const AUTO_GAP_BASE: u32 = 12;
pub const AUTO_GAP_PER_TEN: u32 = 1;
pub const AUTO_GAP_MIN: u32 = 6;

pub fn auto_gap(count: u32) -> u32 {
    let g = AUTO_GAP_BASE.saturating_sub(count / 10 * AUTO_GAP_PER_TEN);
    if g < AUTO_GAP_MIN {
        AUTO_GAP_MIN
    } else {
        g
    }
}

/// 4K 换算：栅格几何乘缩放因子（四舍五入）。
pub const fn grid_dp(g: IconGrid, scale_milli: u32) -> IconGrid {
    IconGrid {
        tile_w: (g.tile_w * scale_milli + 500) / 1000,
        tile_h: (g.tile_h * scale_milli + 500) / 1000,
        icon_px: (g.icon_px * scale_milli + 500) / 1000,
        gap_x: (g.gap_x * scale_milli + 500) / 1000,
        gap_y: (g.gap_y * scale_milli + 500) / 1000,
        waterfall: g.waterfall,
    }
}

// ===========================================================================
// 4. 壁纸引擎目录（ai12-wallpaper.ts f0056 · 25 档引擎）
// ===========================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WallpaperEngine {
    StaticImage,
    VideoLoop,
    WeatherScene,
    Procedural,
    Particles,
    Shader,
    SandboxedWeb,
    Slideshow,
    Pano360,
    Parallax,
    DualScreenSpan,
    DaypartAuto,
    SolarTerm,
    CalendarBlend,
    GiantClock,
    NowPlayingLyrics,
    CodeRain,
    ConwayLife,
    Fractal,
    FluidSim,
    Starry,
    Ocean,
    CityDayNight,
    HandDrawnLoop,
    SolidSaver,
}

pub const WALLPAPER_ENGINES: [WallpaperEngine; 25] = [
    WallpaperEngine::StaticImage,
    WallpaperEngine::VideoLoop,
    WallpaperEngine::WeatherScene,
    WallpaperEngine::Procedural,
    WallpaperEngine::Particles,
    WallpaperEngine::Shader,
    WallpaperEngine::SandboxedWeb,
    WallpaperEngine::Slideshow,
    WallpaperEngine::Pano360,
    WallpaperEngine::Parallax,
    WallpaperEngine::DualScreenSpan,
    WallpaperEngine::DaypartAuto,
    WallpaperEngine::SolarTerm,
    WallpaperEngine::CalendarBlend,
    WallpaperEngine::GiantClock,
    WallpaperEngine::NowPlayingLyrics,
    WallpaperEngine::CodeRain,
    WallpaperEngine::ConwayLife,
    WallpaperEngine::Fractal,
    WallpaperEngine::FluidSim,
    WallpaperEngine::Starry,
    WallpaperEngine::Ocean,
    WallpaperEngine::CityDayNight,
    WallpaperEngine::HandDrawnLoop,
    WallpaperEngine::SolidSaver,
];

impl WallpaperEngine {
    pub fn name(self) -> &'static str {
        match self {
            WallpaperEngine::StaticImage => "static",
            WallpaperEngine::VideoLoop => "video",
            WallpaperEngine::WeatherScene => "scene",
            WallpaperEngine::Procedural => "generative",
            WallpaperEngine::Particles => "particles",
            WallpaperEngine::Shader => "shader",
            WallpaperEngine::SandboxedWeb => "web",
            WallpaperEngine::Slideshow => "slideshow",
            WallpaperEngine::Pano360 => "pano360",
            WallpaperEngine::Parallax => "parallax",
            WallpaperEngine::DualScreenSpan => "dual-screen",
            WallpaperEngine::DaypartAuto => "daypart",
            WallpaperEngine::SolarTerm => "solar",
            WallpaperEngine::CalendarBlend => "calendar",
            WallpaperEngine::GiantClock => "clock",
            WallpaperEngine::NowPlayingLyrics => "lyrics",
            WallpaperEngine::CodeRain => "code-rain",
            WallpaperEngine::ConwayLife => "game-of-life",
            WallpaperEngine::Fractal => "fractal",
            WallpaperEngine::FluidSim => "fluid",
            WallpaperEngine::Starry => "starry",
            WallpaperEngine::Ocean => "ocean",
            WallpaperEngine::CityDayNight => "city",
            WallpaperEngine::HandDrawnLoop => "hand-drawn",
            WallpaperEngine::SolidSaver => "solid-saver",
        }
    }
}

// ===========================================================================
// 5. 4K 资产管线（start.md C-7 硬标准）
// ===========================================================================
/// 图标母版尺寸：512px 母版，派生各档位——禁止位图放大。
pub const ICON_MASTER_PX: u32 = 512;
/// 内核运行时用档位（design px 基准；16/20/24 UI 档 + 32/48/64/96 栅格档）。
pub const ATLAS_TIERS: [u32; 7] = [16, 20, 24, 32, 48, 64, 96];
/// 4K 下各档物理尺寸 = tier × 2（16→32 … 96→192），母版 512 覆盖全部档位无放大。
pub const WALLPAPER_MASTER_W: u32 = 3840;
pub const WALLPAPER_MASTER_H: u32 = 2160;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtlasFit {
    /// 有 ≥ 目标档的母版派生档——矢量或高分辨率缩放，合法。
    Exact,
    /// 无足够母版、需要放大——C-7 红线，拒绝。
    UpscaleBanned,
}

/// 资产档位判定：master_px >= tier×scale 即可无损派生；否则放大违红线。
pub const fn atlas_fit(master_px: u32, tier: u32, scale_milli: u32) -> AtlasFit {
    let need = tier * scale_milli / 1000;
    if master_px >= need {
        AtlasFit::Exact
    } else {
        AtlasFit::UpscaleBanned
    }
}

// ===========================================================================
// 6. CheckSet 自检
// ===========================================================================
pub fn checks(cs: &mut CheckSet) {
    fn g(cs: &mut CheckSet, n: &str, ok: bool) {
        cs.check(n, ok);
    }

    // vicon 规范
    g(cs, "novaassets-vicon-const", VICON_VERSION == 1 && VICON_MAX_BYTES == 5 * 1024 * 1024);
    g(cs, "novaassets-res-svg", classify_resource(b"  <svg xmlns=..>") == ResourceKind::InlineSvg);
    g(cs, "novaassets-res-png", classify_resource(b"data:image/png;base64,xx") == ResourceKind::PngDataUrl);
    g(cs, "novaassets-res-bad", classify_resource(b"<script>") == ResourceKind::Invalid);
    g(cs, "novaassets-res-oversize", classify_resource(&[b'x'; VICON_RESOURCE_MAX + 1]) == ResourceKind::Invalid);
    g(cs, "novaassets-key-ok", valid_icon_key("desktop:sys-recycle") && valid_icon_key("file:.md"));
    // src 正则 /i 大小写不敏感：AREA:x 合法；无冒号/空区域段非法
    g(cs, "novaassets-key-bad", !valid_icon_key("no-colon") && !valid_icon_key(":name") && valid_icon_key("AREA:x"));

    // 注册表：单包模型 / 覆盖 / 回退计数 / 卸载还原
    let mut reg = IconPackRegistry::new();
    let mut mk = |key: &[u8]| -> IconEntry {
        let mut e = IconEntry::EMPTY;
        e.key[..key.len()].copy_from_slice(key);
        e.key_len = key.len();
        e.res[..4].copy_from_slice(b"SVG!");
        e.res_len = 4;
        e
    };
    let entries = [mk(b"desktop:sys-recycle"), mk(b"start:app-write")];
    let ok_install = reg.install(b"pack-a", 1, &entries);
    g(cs, "novaassets-reg-install", ok_install && reg.pack_count() == 1);
    // 空注册表时查询会计回退
    let mut empty = IconPackRegistry::new();
    let _ = empty.get_icon_override(b"desktop:sys-recycle");
    g(cs, "novaassets-reg-fallback", empty.fallback_count() == 1);
    let hit = reg.get_icon_override(b"desktop:sys-recycle");
    g(cs, "novaassets-reg-hit", hit.is_some());
    let miss = reg.get_icon_override(b"start:app-mind");
    g(cs, "novaassets-reg-miss-counts", miss.is_none() && reg.fallback_count() == 1);
    // 后装覆盖（activePack 取末位）
    let ok2 = reg.install(b"pack-b", 2, &entries);
    g(cs, "novaassets-reg-single-active", ok2 && reg.pack_count() == 2 && reg.active_pack().unwrap().version == 2);
    // 同 id 覆盖
    let ok3 = reg.install(b"pack-b", 3, &entries);
    g(cs, "novaassets-reg-overwrite", ok3 && reg.pack_count() == 2 && reg.active_pack().unwrap().version == 3);
    // 卸载还原
    let ok4 = reg.uninstall(b"pack-b");
    g(cs, "novaassets-reg-uninstall", ok4 && reg.pack_count() == 1 && reg.active_pack().unwrap().version == 1);

    // 栅格档位与 src entry 几何一致
    g(cs, "novaassets-grid-base24", GRID_BASE24.tile_w == 84 && GRID_BASE24.icon_px == 24);
    g(cs, "novaassets-grid-superdense", GRID_SUPERDENSE.tile_w == 72 && GRID_SUPERDENSE.icon_px == 20 && GRID_SUPERDENSE.gap_x == 8);
    g(cs, "novaassets-grid-wall", GRID_WALL.icon_px == 96 && GRID_WALL.tile_w == 168);
    g(cs, "novaassets-grid-waterfall", GRID_WATERFALL.waterfall && GRID_WATERFALL.icon_px == 56);
    g(cs, "novaassets-autogap", auto_gap(5) == 12 && auto_gap(25) == 10 && auto_gap(100) == 6);
    let g4k = grid_dp(GRID_BASE24, 2000);
    g(cs, "novaassets-grid-4k", g4k.tile_w == 168 && g4k.icon_px == 48 && g4k.gap_x == 24);

    // 壁纸引擎 25 档目录完整
    g(cs, "novaassets-wp-25", WALLPAPER_ENGINES.len() == 25);
    g(cs, "novaassets-wp-unique", {
        let mut uniq = true;
        for i in 0..WALLPAPER_ENGINES.len() {
            for j in (i + 1)..WALLPAPER_ENGINES.len() {
                if WALLPAPER_ENGINES[i] == WALLPAPER_ENGINES[j] {
                    uniq = false;
                }
            }
        }
        uniq
    });
    g(cs, "novaassets-wp-names", WallpaperEngine::StaticImage.name() == "static" && WallpaperEngine::SolidSaver.name() == "solid-saver");

    // 4K 资产管线红线
    g(cs, "novaassets-atlas-master", ICON_MASTER_PX == 512 && WALLPAPER_MASTER_W == 3840);
    g(cs, "novaassets-atlas-exact", atlas_fit(512, 96, 2000) == AtlasFit::Exact);
    g(cs, "novaassets-atlas-ban", atlas_fit(24, 96, 2000) == AtlasFit::UpscaleBanned);
    g(cs, "novaassets-atlas-4k-96", atlas_fit(ICON_MASTER_PX, 96, 2000) == AtlasFit::Exact); // 96×2=192 ≤ 512
}

// ===========================================================================
// 7. 单元测试（宿主机 std 下运行）
// ===========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vicon_rules_match_src() {
        assert_eq!(VICON_VERSION, 1);
        assert!(valid_icon_key("desktop:sys-recycle"));
        assert!(valid_icon_key("file:.md"));
        assert!(valid_icon_key("start:app-write"));
        assert!(!valid_icon_key("desktop:"));
        assert!(!valid_icon_key(".md"));
        assert_eq!(classify_resource(b"<svg/>"), ResourceKind::InlineSvg);
        assert_eq!(classify_resource(b"data:image/webp,"), ResourceKind::PngDataUrl);
    }

    #[test]
    fn registry_single_pack_model() {
        let mut reg = IconPackRegistry::new();
        let mut e = IconEntry::EMPTY;
        let k = b"desktop:sys-recycle";
        e.key[..k.len()].copy_from_slice(k);
        e.key_len = k.len();
        assert!(reg.install(b"a", 1, &[e]));
        assert!(reg.get_icon_override(k).is_some());
        assert!(reg.get_icon_override(b"none").is_none());
        assert_eq!(reg.fallback_count(), 1);
        assert!(reg.uninstall(b"a"));
        assert!(reg.active_pack().is_none());
    }

    #[test]
    fn grid_and_wallpaper_match_src_entries() {
        assert_eq!(GRID_SUPERDENSE, IconGrid { tile_w: 72, tile_h: 84, icon_px: 20, gap_x: 8, gap_y: 8, waterfall: false });
        assert_eq!(WALLPAPER_ENGINES.len(), 25);
        assert_eq!(WALLPAPER_ENGINES[0], WallpaperEngine::StaticImage);
        assert_eq!(WALLPAPER_ENGINES[24], WallpaperEngine::SolidSaver);
        // 4K：超密档 20px 图标 → 40px 物理，母版 512 恒可派生
        let g = grid_dp(GRID_SUPERDENSE, 2000);
        assert_eq!(g.icon_px, 40);
        assert_eq!(atlas_fit(512, 20, 2000), AtlasFit::Exact);
    }
}
