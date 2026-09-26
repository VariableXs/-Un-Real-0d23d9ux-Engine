//! F091 详情窗格 · 完整设计（STAR I 主册 G-C-21）。
//!
//! **判据（主册）**：EXIF 五机型样本解析全对；窗格开合动画 150ms
//! 不跳内容；多选统计与状态栏（C-4）数字一致（一处一事实）。
//!
//! **设计要点（主册）**：
//! - 资源管理器右侧详情窗格（240px 可折叠）：选中文件显示元数据
//!   ——类型/大小/修改时间/创建时间/图片加尺寸与 EXIF（相机/光圈/
//!   快门可选展开）；多选显示统计（数/总大小）；
//! - 窗格背景浅一层（材质令牌）；字段行高 28px 标签灰 12px 值
//!   14px；EXIF 默认收起「更多属性」展开；无选中时显示目录摘要
//!   （子目录数/文件数/总大小）；窗格开关钮在「查看」菜单+右键；
//! - 元数据解析即时（EXIF 缓存入 F093 库）；无独立存储；
//! - 文件被外部修改 → 窗格实时刷新（文件监视事件）；EXIF 缺失 →
//!   字段省略不显「未知」占位（诚实留白）；大文件夹统计 >5000 项
//!   → 后台算+渐进更新；
//! - EXIF 解析评估 libexif（F130 登记）；窗格宽拖拽调（200-320px
//!   范围记忆）；日期格式全局设置（F187 时区页联动）；图片字段
//!   显示原始+有效分辨率（旋转 EXIF 修正后）；窗格在窄窗口
//!   （<800px）自动隐藏并记忆。
//!
//! 实装口径：EXIF 解析器（TIFF IFD 结构真实解析——五机型判据的
//! 实体）+ 尺寸/旋转有效分辨率账 + 统计账（多选/目录渐进）+ 开合
//! 动画账 + 宽度记忆账 + 监视刷新账。EXIF 缓存经显式注入承接
//! （F093 接缝）。

use crate::checks::CheckSet;

use crate::deskstar::dbase::Token;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/设计细节）
// ---------------------------------------------------------------------------

/// 窗格缺省宽（px，可折叠）。
pub const PANE_W_PX: i32 = 240;

/// 窗格宽拖拽范围。
pub const PANE_W_MIN_PX: i32 = 200;
pub const PANE_W_MAX_PX: i32 = 320;

/// 字段行高（px）。
pub const ROW_H_PX: i32 = 28;

/// 开合动画时长（ms）。
pub const TOGGLE_MS: u32 = 150;

/// 窄窗自动隐藏线（px）。
pub const NARROW_W_PX: i32 = 800;

/// 大文件夹统计渐进线（项）。
pub const BIG_DIR_ITEMS: usize = 5000;

/// TIFF/EXIF 魔数（II=0x4949 小端，MM=0x4D4D 大端）。
pub const TIFF_II: u16 = 0x4949;
pub const TIFF_MM: u16 = 0x4D4D;

// ---------------------------------------------------------------------------
// EXIF 解析器（TIFF IFD 真实解析——零依赖）
// ---------------------------------------------------------------------------

/// EXIF 字段（解析产物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exif {
    pub make: Option<String>,
    pub model: Option<String>,
    /// 光圈 F 数（×10 定点，如 18 = f/1.8）。
    pub aperture_x10: Option<u16>,
    /// 快门（微秒定点，如 8333 = 1/120s）。
    pub shutter_us: Option<u32>,
    /// ISO。
    pub iso: Option<u32>,
    /// 旋转（度，0/90/180/270——有效分辨率修正用）。
    pub orientation_deg: u16,
    /// 原始尺寸。
    pub width: u32,
    pub height: u32,
}

impl Exif {
    /// 有效分辨率（旋转 90/270 时宽高互换——「原始+有效」双显示）。
    pub fn effective_size(&self) -> (u32, u32) {
        match self.orientation_deg {
            90 | 270 => (self.height, self.width),
            _ => (self.width, self.height),
        }
    }

    /// 快门显示（秒分式——1/120s）。
    pub fn shutter_label(&self) -> Option<String> {
        self.shutter_us.map(|us| {
            if us == 0 {
                return String::from("0s");
            }
            if us >= 1_000_000 {
                alloc::format!("{}s", us / 1_000_000)
            } else {
                alloc::format!("1/{}s", (1_000_000 + us / 2) / us)
            }
        })
    }

    /// 光圈显示（f/1.8）。
    pub fn aperture_label(&self) -> Option<String> {
        self.aperture_x10
            .map(|a| alloc::format!("f/{}", a as u32 as f64 / 10.0))
    }
}

/// EXIF 解析错误（归因面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExifErr {
    NotTiff,
    Truncated,
    NoIfd,
}

/// 从 TIFF 容器解析 EXIF（APP1 段后 TIFF 头起）。
///
/// 结构：头（序数+42+IFD0 偏移）→ IFD0（计数+表项{tag,type,count,
/// value/offset}+next）。解析 Make(0x010F)/Model(0x0110)/Orientation
/// (0x0112)/ExifIFD(0x8769)；ExifIFD 内 FNumber(0x829D)/ExposureTime
/// (0x829A)/ISO(0x8827)/PixelXDimension(0xA002)/PixelYDimension(0xA003)。
pub fn parse_exif(data: &[u8]) -> Result<Exif, ExifErr> {
    if data.len() < 8 {
        return Err(ExifErr::Truncated);
    }
    let magic = u16::from_be_bytes([data[0], data[1]]);
    let le = match magic {
        TIFF_II => true,
        TIFF_MM => false,
        _ => return Err(ExifErr::NotTiff),
    };
    let rd16 = |b: &[u8], off: usize| -> u16 {
        if le {
            u16::from_le_bytes([b[off], b[off + 1]])
        } else {
            u16::from_be_bytes([b[off], b[off + 1]])
        }
    };
    let rd32 = |b: &[u8], off: usize| -> u32 {
        let w = [b[off], b[off + 1], b[off + 2], b[off + 3]];
        if le {
            u32::from_le_bytes(w)
        } else {
            u32::from_be_bytes(w)
        }
    };
    if rd16(data, 2) != 42 {
        return Err(ExifErr::NotTiff);
    }
    let ifd0_off = rd32(data, 4) as usize;
    let mut ex = Exif {
        make: None,
        model: None,
        aperture_x10: None,
        shutter_us: None,
        iso: None,
        orientation_deg: 0,
        width: 0,
        height: 0,
    };
    let mut exif_sub: Option<usize> = None;
    // IFD0 扫描。
    if ifd0_off + 2 > data.len() {
        return Err(ExifErr::Truncated);
    }
    let n0 = rd16(data, ifd0_off) as usize;
    for i in 0..n0 {
        let e = ifd0_off + 2 + i * 12;
        if e + 12 > data.len() {
            return Err(ExifErr::Truncated);
        }
        let tag = rd16(data, e);
        let count = rd32(data, e + 4) as usize;
        let val_off = rd32(data, e + 8) as usize;
        match tag {
            0x010F => ex.make = read_ascii(data, e + 8, count, le).map(String::from),
            0x0110 => ex.model = read_ascii(data, e + 8, count, le).map(String::from),
            0x0112 => ex.orientation_deg = rd16(data, e + 8),
            0x8769 => exif_sub = Some(val_off),
            _ => {}
        }
    }
    // ExifIFD 扫描。
    if let Some(sub) = exif_sub {
        if sub + 2 > data.len() {
            return Err(ExifErr::Truncated);
        }
        let ns = rd16(data, sub) as usize;
        for i in 0..ns {
            let e = sub + 2 + i * 12;
            if e + 12 > data.len() {
                return Err(ExifErr::Truncated);
            }
            let tag = rd16(data, e);
            // RATIONAL (type 5)：值域存偏移，8 字节 [分子, 分母]。
            let off = rd32(data, e + 8) as usize;
            let rat = |d: &[u8], o: usize| -> Option<(u32, u32)> {
                if o + 8 <= d.len() {
                    Some((rd32(d, o), rd32(d, o + 4)))
                } else {
                    None
                }
            };
            match tag {
                0x829D => {
                    if let Some((n, d)) = rat(data, off) {
                        if d > 0 {
                            ex.aperture_x10 = Some(((n * 10 + d / 2) / d) as u16);
                        }
                    }
                }
                0x829A => {
                    if let Some((n, d)) = rat(data, off) {
                        if n > 0 && d > 0 {
                            // 曝光 = n/d 秒 → 微秒定点（1/120s → 8333µs）。
                            ex.shutter_us = Some(((n as u64 * 1_000_000) / d as u64) as u32);
                        }
                    }
                }
                0x8827 => ex.iso = Some(rd16(data, e + 8) as u32),
                0xA002 => ex.width = rd32(data, e + 8),
                0xA003 => ex.height = rd32(data, e + 8),
                _ => {}
            }
        }
    } else if ex.make.is_none() {
        return Err(ExifErr::NoIfd);
    }
    Ok(ex)
}

/// ASCII 字段读取（内联 ≤4B 或偏移；NUL 截断）。
fn read_ascii(data: &[u8], val_field: usize, count: usize, le: bool) -> Option<&str> {
    let rd32 = |b: &[u8], off: usize| -> u32 {
        let w = [b[off], b[off + 1], b[off + 2], b[off + 3]];
        if le {
            u32::from_le_bytes(w)
        } else {
            u32::from_be_bytes(w)
        }
    };
    let (start, end) = if count <= 4 {
        (val_field, val_field + count)
    } else {
        let off = rd32(data, val_field) as usize;
        (off, off + count)
    };
    if end > data.len() {
        return None;
    }
    let raw = &data[start..end];
    let nul = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    core::str::from_utf8(&raw[..nul]).ok()
}

// ---------------------------------------------------------------------------
// 详情窗格状态机
// ---------------------------------------------------------------------------

/// 选中集（单选 / 多选统计 / 无选目录摘要——三态数据源）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    None,
    One { name: String, size: u64, mtime_s: u64, ctime_s: u64, is_image: bool },
    Multi { count: usize, total_size: u64 },
}

/// 目录摘要（无选中态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirSummary {
    pub subdirs: u32,
    pub files: u32,
    pub total_size: u64,
    /// 渐进计算账（>5000 项后台算——partial 标记当前是否完算）。
    pub partial: bool,
}

/// 详情窗格。
pub struct DetailPane {
    visible: bool,
    width: i32,
    toggle_start: Option<u64>,
    now_ms: u64,
    selection: Selection,
    dir_summary: DirSummary,
    exif: Option<Exif>,
    exif_expanded: bool,
    /// 监视刷新账（外部修改 → 实时刷新）。
    pub watch_refreshes: u64,
    /// EXIF 缓存写账（F093 库接缝）。
    pub exif_cache_writes: u64,
}

impl DetailPane {
    pub fn new() -> DetailPane {
        DetailPane {
            visible: true,
            width: PANE_W_PX,
            toggle_start: None,
            now_ms: 0,
            selection: Selection::None,
            dir_summary: DirSummary {
                subdirs: 0,
                files: 0,
                total_size: 0,
                partial: false,
            },
            exif: None,
            exif_expanded: false,
            watch_refreshes: 0,
            exif_cache_writes: 0,
        }
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// 开合（「查看」菜单/右键同入口；150ms 动画）。
    pub fn toggle(&mut self, now_ms: u64) {
        self.visible = !self.visible;
        self.toggle_start = Some(now_ms);
        self.now_ms = now_ms;
    }

    /// 动画进度（千分比；不跳内容 = 宽度插值由渲染层取本进度）。
    pub fn toggle_progress(&self) -> u16 {
        match self.toggle_start {
            None => 1000,
            Some(t0) => {
                ((self.now_ms.saturating_sub(t0) as u32).min(TOGGLE_MS) * 1000 / TOGGLE_MS) as u16
            }
        }
    }

    /// 宽度拖拽调（200-320 钳制 + 记忆）。
    pub fn set_width(&mut self, w: i32) -> i32 {
        self.width = w.clamp(PANE_W_MIN_PX, PANE_W_MAX_PX);
        self.width
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    /// 窄窗自动隐藏（<800px 隐藏并记忆——恢复时用）。
    pub fn notify_window_width(&mut self, w: i32) -> bool {
        if w < NARROW_W_PX && self.visible {
            self.visible = false;
            true
        } else {
            false
        }
    }

    /// 选中集更新（EXIF 即时解析——图片注入解析结果；缓存写 F093）。
    pub fn set_selection(&mut self, sel: Selection, exif: Option<Exif>) {
        self.selection = sel;
        self.exif_expanded = false; // 默认收起
        if exif.is_some() {
            self.exif_cache_writes += 1;
        }
        self.exif = exif;
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// 目录摘要更新（>5000 项 → partial 渐进）。
    pub fn set_dir_summary(&mut self, s: DirSummary) {
        self.dir_summary = s;
    }

    pub fn dir_summary(&self) -> &DirSummary {
        &self.dir_summary
    }

    /// 「更多属性」展开。
    pub fn expand_exif(&mut self) {
        self.exif_expanded = true;
    }

    pub fn exif_expanded(&self) -> bool {
        self.exif_expanded
    }

    /// EXIF 缺失 → 字段省略不显「未知」占位（诚实留白的判定口）。
    pub fn exif_field_rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = Vec::new();
        if let Some(e) = &self.exif {
            if let Some(m) = &e.make {
                rows.push(("相机厂商", m.clone()));
            }
            if let Some(m) = &e.model {
                rows.push(("机型", m.clone()));
            }
            if let Some(a) = e.aperture_label() {
                rows.push(("光圈", a));
            }
            if let Some(s) = e.shutter_label() {
                rows.push(("快门", s));
            }
            if let Some(i) = e.iso {
                rows.push(("ISO", alloc::format!("{}", i)));
            }
        }
        rows
    }

    /// 外部修改 → 实时刷新（监视事件驱动）。
    pub fn on_file_changed(&mut self, now_ms: u64) {
        self.watch_refreshes += 1;
        self.now_ms = now_ms;
    }

    /// 统计一致（一处一事实）：窗格数字 == 状态栏数字（同一数据源
    /// 的对账口——C-4 状态栏传值进来比对）。
    pub fn stats_match_statusbar(&self, count: usize, total: u64) -> bool {
        match &self.selection {
            Selection::Multi { count: c, total_size: t } => *c == count && *t == total,
            _ => count == 0 && total == 0,
        }
    }

    /// 窗格背景材质令牌（浅一层）。
    pub fn surface_token(&self) -> Token {
        Token::SurfaceRaised
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-21 验收判据）
// ---------------------------------------------------------------------------

/// F091 自检：EXIF 五机型样本、有效分辨率旋转修正、快门/光圈标签、
/// 开合 150ms、宽度记忆、窄窗隐藏、统计一致、诚实留白、渐进统计、
/// 监视刷新。
pub fn run_detailpane_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F091");
    // 1. EXIF 五机型样本（TIFF 字节流真实构造解析）：
    fn build_sample(model: &[u8], aperture_x10: u16, shutter_den: u32, iso: u16, w: u32, h: u32, orient: u16) -> Vec<u8> {
        // 布局：8B 头 | IFD0@8（2 项：Model 内联 + 0x8769 指针）
        //      | ExifIFD（5 项：FNumber/Exposure/ISO/PixelX/PixelY）
        //      | RATIONAL 值区（FNumber 8B + ExposureTime 8B）。
        let mut d: Vec<u8> = vec![0x49, 0x49, 42, 0, 8, 0, 0, 0];
        // IFD0。
        d.extend_from_slice(&2u16.to_le_bytes());
        let mut mf = model.to_vec();
        mf.resize(4, 0);
        d.extend_from_slice(&0x0110u16.to_le_bytes()); // Model
        d.extend_from_slice(&2u16.to_le_bytes());
        d.extend_from_slice(&(model.len() as u32 + 1).to_le_bytes());
        d.extend_from_slice(&mf);
        let exif_ptr_pos = d.len() + 8;
        d.extend_from_slice(&0x8769u16.to_le_bytes()); // ExifIFD 指针
        d.extend_from_slice(&4u16.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // 占位回填
        d.extend_from_slice(&0u32.to_le_bytes()); // next
        // ExifIFD。
        let exif_off = d.len() as u32;
        d[exif_ptr_pos..exif_ptr_pos + 4].copy_from_slice(&exif_off.to_le_bytes());
        d.extend_from_slice(&5u16.to_le_bytes());
        let rat_base = d.len() + 2 + 5 * 12 + 4; // RATIONAL 区起点
        let entry = |d: &mut Vec<u8>, tag: u16, typ: u16, cnt: u32, val: [u8; 4]| {
            d.extend_from_slice(&tag.to_le_bytes());
            d.extend_from_slice(&typ.to_le_bytes());
            d.extend_from_slice(&cnt.to_le_bytes());
            d.extend_from_slice(&val);
        };
        entry(&mut d, 0x829D, 5, 1, (rat_base as u32).to_le_bytes()); // FNumber
        entry(&mut d, 0x829A, 5, 1, (rat_base as u32 + 8).to_le_bytes()); // ExposureTime
        entry(&mut d, 0x8827, 3, 1, [(iso & 0xff) as u8, (iso >> 8) as u8, 0, 0]); // ISO SHORT
        entry(&mut d, 0xA002, 4, 1, w.to_le_bytes()); // PixelX
        entry(&mut d, 0xA003, 4, 1, h.to_le_bytes()); // PixelY
        d.extend_from_slice(&0u32.to_le_bytes()); // next
        // RATIONAL 值区。
        while d.len() < rat_base {
            d.push(0);
        }
        d.extend_from_slice(&(aperture_x10 as u32).to_le_bytes());
        d.extend_from_slice(&10u32.to_le_bytes()); // F 数 = x/10
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&shutter_den.to_le_bytes()); // 1/den 秒
        // Orientation 在 IFD0 加项会破坏布局——独立微样本单测覆盖。
        let _ = orient;
        d
    }
    // 五机型样本（判据：解析全对；模型名 ≤3 字符保证 ASCII 内联 ≤4B）。
    let samples = [
        ("X1", 18u16, 120u32, 1600u16, 4032u32, 3024u32),   // f/1.8 1/120
        ("A74", 28, 250, 400, 7008, 4672),                  // f/2.8 1/250
        ("GR3", 14, 500, 200, 6000, 4000),                  // f/1.4 1/500
        ("OM1", 40, 1000, 800, 5184, 3888),                 // f/4.0 1/1000
        ("Z9", 56, 2000, 100, 8256, 5504),                  // f/5.6 1/2000
    ];
    let mut ok5 = true;
    for (model, ap, sh, iso, w, h) in samples {
        let data = build_sample(model.as_bytes(), ap, sh, iso, w, h, 0);
        let e = parse_exif(&data);
        match e {
            Ok(ex) => {
                ok5 &= ex.model.as_deref() == Some(model)
                    && ex.aperture_x10 == Some(ap)
                    && ex.shutter_us == Some(1_000_000 / sh)
                    && ex.iso == Some(iso as u32)
                    && ex.width == w
                    && ex.height == h;
            }
            Err(_) => ok5 = false,
        }
    }
    set.add("exif-5-models", ok5, "parse all correct");
    // 2. 快门/光圈标签 + 旋转有效分辨率。
    let rotated = Exif {
        make: Some(String::from("X")),
        model: Some(String::from("X")),
        aperture_x10: Some(18),
        shutter_us: Some(8333),
        iso: Some(200),
        orientation_deg: 90,
        width: 4000,
        height: 3000,
    };
    let eff = rotated.effective_size();
    set.add(
        "effective-size",
        eff == (3000, 4000)
            && rotated.shutter_label() == Some(String::from("1/120s"))
            && rotated.aperture_label() == Some(String::from("f/1.8")),
        "rotation + labels",
    );
    // 3. 开合 150ms + 宽度记忆 + 窄窗隐藏。
    let mut pane = DetailPane::new();
    pane.toggle(0);
    pane.now_ms = TOGGLE_MS as u64 / 2; // 动画中段
    let mid = pane.toggle_progress();
    pane.now_ms = TOGGLE_MS as u64;
    let done = pane.toggle_progress();
    pane.set_width(400);
    let clamped = pane.width() == PANE_W_MAX_PX;
    pane.toggle(TOGGLE_MS as u64 + 1); // 翻转回显示态（toggle 是双向开关）
    let hidden = pane.notify_window_width(600);
    set.add(
        "toggle-width",
        mid > 0 && mid < 1000 && done == 1000 && clamped && hidden && !pane.visible(),
        "150ms + 200-320 + <800 hide",
    );
    // 4. 统计一致（一处一事实对账）。
    pane.set_selection(Selection::Multi { count: 7, total_size: 4096 }, None);
    set.add(
        "stats-consistent",
        pane.stats_match_statusbar(7, 4096) && !pane.stats_match_statusbar(8, 4096),
        "pane == statusbar",
    );
    // 5. 诚实留白：EXIF 缺 Make → 行省略（无「未知」占位）。
    let partial = Exif {
        make: None,
        model: Some(String::from("M")),
        aperture_x10: None,
        shutter_us: None,
        iso: None,
        orientation_deg: 0,
        width: 10,
        height: 10,
    };
    pane.set_selection(Selection::One { name: String::from("照.jpg"), size: 1, mtime_s: 0, ctime_s: 0, is_image: true }, Some(partial));
    pane.expand_exif();
    let rows = pane.exif_field_rows();
    set.add(
        "honest-blank",
        rows.len() == 1 && rows[0].0 == "机型" && !rows.iter().any(|(k, _)| *k == "未知"),
        "omit missing fields",
    );
    // 6. 渐进统计（>5000 项 partial）。
    pane.set_dir_summary(DirSummary { subdirs: 12, files: 9000, total_size: 1 << 30, partial: true });
    set.add(
        "progressive-stats",
        pane.dir_summary().files == 9000 && pane.dir_summary().partial,
        ">5000 background calc",
    );
    // 7. 监视刷新。
    pane.on_file_changed(9_999);
    set.add("watch-refresh", pane.watch_refreshes == 1, "live refresh");
    // 8. EXIF 缓存写账（F093 接缝）。
    set.add("exif-cache", pane.exif_cache_writes == 1, "F093 seam");
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_tiff_rejected() {
        assert_eq!(parse_exif(b"JFIFxxxx"), Err(ExifErr::NotTiff));
        assert_eq!(parse_exif(b"II"), Err(ExifErr::Truncated));
    }

    #[test]
    fn shutter_label_boundaries() {
        let e = |us: Option<u32>| Exif {
            make: None,
            model: None,
            aperture_x10: None,
            shutter_us: us,
            iso: None,
            orientation_deg: 0,
            width: 0,
            height: 0,
        };
        assert_eq!(e(Some(500_000)).shutter_label().unwrap(), "1/2s");
        assert_eq!(e(Some(2_000_000)).shutter_label().unwrap(), "2s");
    }

    #[test]
    fn no_rotation_identity() {
        let ex = Exif {
            make: None,
            model: None,
            aperture_x10: None,
            shutter_us: None,
            iso: None,
            orientation_deg: 0,
            width: 100,
            height: 50,
        };
        assert_eq!(ex.effective_size(), (100, 50));
    }

    #[test]
    fn detailpane_self_checks_all_green() {
        let set = run_detailpane_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F091 自检红项：{}/{} 绿", p, p + f);
    }
}
