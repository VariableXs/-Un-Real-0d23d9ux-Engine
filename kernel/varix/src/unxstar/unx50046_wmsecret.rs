//! UNX-F50046 机密级水印预设（草稿/机密/绝密三档文案与样式）（0x431e0 段）
//!
//! 锚点：UNX-F50046-J1 预设 ×3 渲染一致且文案零漂移；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段 `SEG_BASE = 0x431e0`：诊断码闭集 8 条（1..=8），全部由注入核或判据
//! 真实构造。
//!
//! 结构清单：
//! * `Level` 三档枚举（Draft/Confidential/TopSecret）：rank 1..=3、标准文案
//!   「草稿/机密/绝密」、内置样式；三档的顺序是打印/阅卷语义的硬约束；
//! * `Style` + `Override`：样式四元组（不透明度/角度/密度/字号）与逐项覆盖，
//!   覆盖只改样式、**不碰文案**，且每项都有显式闭区间；
//! * `Preset`：等级 × 文案 × 样式的组合体，唯一入口 `custom` 会校验「文案必须
//!   逐字节等于等级标准文案」，这是「文案零漂移」的实现级保证；
//! * `TileGrid`：由样式反推的铺贴网格（密度越大步距越小、格数越多）；
//! * `Snapshot` + `RenderLedger`：快照含文案字节数/量化角/网格几何与 FNV
//!   摘要，账本记录每次渲染并区分成功/拒绝，「×3 渲染一致」可复算；
//! * 注入核：八类坏样本（篡改文案/未知等级码/不透明度越界/角度越界/密度
//!   为零/字号越界/空文案/零画布）逐类断言具体诊断码。
//!
//! 为什么文案要在 `Preset` 里存一份而不是每次问 `Level`：水印是企业对外文
//! 书，文案是**法律责任**。存一份再对齐校验，篡改（哪怕是尾部空格）会在
//! 建预设时就失败，而不是等到印出来才发现「机密 」多了个空格。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x431e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x431e0;

/// 不透明度下限（%）。再低在浅底上就看不见了。
pub const OPACITY_MIN: u8 = 16;
/// 不透明度上限（%）。再高会盖住正文，与「水印」定位相悖。
pub const OPACITY_MAX: u8 = 64;
/// 角度绝对值上限（毫度 ±45°）：斜排是水印惯例，超过 45° 阅读方向反了。
pub const ANGLE_LIMIT: i32 = 45_000;
/// 角度量化步长（毫度）。0.5° 的粒度足够，且让同一角度反复覆盖结果稳定。
pub const ANGLE_STEP: i32 = 500;
/// 密度上限（每平方区域的重复次数档位）。
pub const DENSITY_MAX: u8 = 8;
/// 字号下限（pt）。
pub const FONT_MIN: u8 = 8;
/// 字号上限（pt）。
pub const FONT_MAX: u8 = 48;

pub const E_COPY_DRIFT: u16 = 1;
pub const E_UNKNOWN_LEVEL: u16 = 2;
pub const E_RANGE_OPACITY: u16 = 3;
pub const E_RANGE_ANGLE: u16 = 4;
pub const E_DENSITY: u16 = 5;
pub const E_RANGE_FONT: u16 = 6;
pub const E_EMPTY_COPY: u16 = 7;
pub const E_ZERO_CANVAS: u16 = 8;

/// 预设域错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretError {
    CopyDrift,
    UnknownLevel,
    RangeOpacity,
    RangeAngle,
    DensityZero,
    RangeFont,
    EmptyCopy,
    ZeroCanvas,
}

impl SecretError {
    pub fn code(self) -> u32 {
        let n = match self {
            SecretError::CopyDrift => E_COPY_DRIFT,
            SecretError::UnknownLevel => E_UNKNOWN_LEVEL,
            SecretError::RangeOpacity => E_RANGE_OPACITY,
            SecretError::RangeAngle => E_RANGE_ANGLE,
            SecretError::DensityZero => E_DENSITY,
            SecretError::RangeFont => E_RANGE_FONT,
            SecretError::EmptyCopy => E_EMPTY_COPY,
            SecretError::ZeroCanvas => E_ZERO_CANVAS,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            SecretError::CopyDrift => "copy text drifted from level canon",
            SecretError::UnknownLevel => "level code outside 0..=2",
            SecretError::RangeOpacity => "opacity outside 16..=64",
            SecretError::RangeAngle => "angle outside -45000..=45000 mdeg",
            SecretError::DensityZero => "density outside 1..=8",
            SecretError::RangeFont => "font size outside 8..=48 pt",
            SecretError::EmptyCopy => "copy text is empty",
            SecretError::ZeroCanvas => "canvas width/height is zero",
        }
    }
}

/// 三档机密等级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Draft,
    Confidential,
    TopSecret,
}

impl Level {
    /// 等级码（0/1/2），用于序列化与摘要。
    pub fn code(self) -> u8 {
        match self {
            Level::Draft => 0,
            Level::Confidential => 1,
            Level::TopSecret => 2,
        }
    }

    /// 由码反解等级；越界一律 UnknownLevel（不猜、不夹取）。
    pub fn from_code(c: u8) -> Result<Level, SecretError> {
        match c {
            0 => Ok(Level::Draft),
            1 => Ok(Level::Confidential),
            2 => Ok(Level::TopSecret),
            _ => Err(SecretError::UnknownLevel),
        }
    }

    /// 等级秩序：1 草稿 < 2 机密 < 3 绝密。
    pub fn rank(self) -> u8 {
        match self {
            Level::Draft => 1,
            Level::Confidential => 2,
            Level::TopSecret => 3,
        }
    }

    /// 标准文案。这三串是法务口径，任何改动都必须走版本变更而非现场输入。
    pub fn copy(self) -> &'static str {
        match self {
            Level::Draft => "草稿",
            Level::Confidential => "机密",
            Level::TopSecret => "绝密",
        }
    }

    /// 内置样式：等级越高，越不透明、角度越陡、密度越大、字号越大。
    pub fn canonical_style(self) -> Style {
        match self {
            Level::Draft => Style {
                opacity: 32,
                angle_mdeg: -15_000,
                density: 2,
                font_pt: 14,
            },
            Level::Confidential => Style {
                opacity: 40,
                angle_mdeg: -30_000,
                density: 3,
                font_pt: 16,
            },
            Level::TopSecret => Style {
                opacity: 48,
                angle_mdeg: -45_000,
                density: 4,
                font_pt: 18,
            },
        }
    }
}

/// 样式四元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub opacity: u8,
    pub angle_mdeg: i32,
    pub density: u8,
    pub font_pt: u8,
}

impl Style {
    /// 逐项校验四个字段，返回第一个越界项对应的诊断码。
    pub fn validate(&self) -> Result<(), SecretError> {
        if self.opacity < OPACITY_MIN || self.opacity > OPACITY_MAX {
            return Err(SecretError::RangeOpacity);
        }
        if self.angle_mdeg < -ANGLE_LIMIT || self.angle_mdeg > ANGLE_LIMIT {
            return Err(SecretError::RangeAngle);
        }
        if self.density == 0 || self.density > DENSITY_MAX {
            return Err(SecretError::DensityZero);
        }
        if self.font_pt < FONT_MIN || self.font_pt > FONT_MAX {
            return Err(SecretError::RangeFont);
        }
        Ok(())
    }

    /// 量化后的角度（毫度）：展示与摘要都用它，保证「同角度同结果」。
    pub fn angle_q(&self) -> i32 {
        quantize_angle(self.angle_mdeg)
    }
}

/// 把角度按 0.5° 就近量化（半格向外舍入），i32 全量程不溢出。
pub fn quantize_angle(mdeg: i32) -> i32 {
    let half: i64 = if mdeg >= 0 {
        (ANGLE_STEP / 2) as i64
    } else {
        -((ANGLE_STEP / 2) as i64)
    };
    // 先升 i64 再算，避免 mdeg 接近 i32 边界时加 half 溢出。
    let q = ((mdeg as i64 + half) / ANGLE_STEP as i64) * ANGLE_STEP as i64;
    q as i32
}

/// 页面级样式覆盖。`None` 表示沿用预设值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Override {
    pub opacity: Option<u8>,
    pub angle_mdeg: Option<i32>,
    pub density: Option<u8>,
    pub font_pt: Option<u8>,
}

impl Override {
    /// 空覆盖：什么都不改。
    pub fn none() -> Override {
        Override {
            opacity: None,
            angle_mdeg: None,
            density: None,
            font_pt: None,
        }
    }

    /// 设置不透明度（不校验，`apply` 时统一验）。
    pub fn opacity(mut self, v: u8) -> Override {
        self.opacity = Some(v);
        self
    }

    /// 设置角度毫度（不校验，`apply` 时统一验）。
    pub fn angle(mut self, v: i32) -> Override {
        self.angle_mdeg = Some(v);
        self
    }

    /// 设置密度（不校验，`apply` 时统一验）。
    pub fn density(mut self, v: u8) -> Override {
        self.density = Some(v);
        self
    }

    /// 设置字号（不校验，`apply` 时统一验）。
    pub fn font(mut self, v: u8) -> Override {
        self.font_pt = Some(v);
        self
    }

    /// 应用到基线样式：只换样式字段，角度写回前先量化。
    pub fn apply(self, base: Style) -> Result<Style, SecretError> {
        let mut s = base;
        if let Some(o) = self.opacity {
            if o < OPACITY_MIN || o > OPACITY_MAX {
                return Err(SecretError::RangeOpacity);
            }
            s.opacity = o;
        }
        if let Some(a) = self.angle_mdeg {
            if a < -ANGLE_LIMIT || a > ANGLE_LIMIT {
                return Err(SecretError::RangeAngle);
            }
            s.angle_mdeg = quantize_angle(a);
        }
        if let Some(d) = self.density {
            if d == 0 || d > DENSITY_MAX {
                return Err(SecretError::DensityZero);
            }
            s.density = d;
        }
        if let Some(f) = self.font_pt {
            if f < FONT_MIN || f > FONT_MAX {
                return Err(SecretError::RangeFont);
            }
            s.font_pt = f;
        }
        Ok(s)
    }
}

/// 一条预设：等级 + 文案 + 样式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    pub level: Level,
    pub copy: &'static str,
    pub style: Style,
}

impl Preset {
    /// 内置三档预设。常量表可信，因此不做运行期校验。
    pub fn builtin(level: Level) -> Preset {
        Preset {
            level,
            copy: level.copy(),
            style: level.canonical_style(),
        }
    }

    /// 外部输入入口：文案必须逐字节等于等级标准文案，样式必须整组合法。
    pub fn custom(level: Level, copy: &'static str, style: Style) -> Result<Preset, SecretError> {
        if copy.is_empty() {
            return Err(SecretError::EmptyCopy);
        }
        if copy != level.copy() {
            // 注意 `!=` 是逐字节比较：尾部空格、全半角差异都会在这里被拦下。
            return Err(SecretError::CopyDrift);
        }
        style.validate()?;
        Ok(Preset { level, copy, style })
    }

    /// 仅换样式、保留文案与等级（页面覆盖走这条路，文案天然不动）。
    pub fn restyle(&self, style: Style) -> Result<Preset, SecretError> {
        style.validate()?;
        Ok(Preset {
            level: self.level,
            copy: self.copy,
            style,
        })
    }
}

/// 铺贴网格：由样式与画布尺寸反推。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileGrid {
    pub cols: u16,
    pub rows: u16,
    pub step_x: u16,
    pub step_y: u16,
}

impl TileGrid {
    /// 由样式与画布尺寸构造网格；画布任一维为 0 即 ZeroCanvas。
    pub fn new(style: &Style, width: u16, height: u16) -> Result<TileGrid, SecretError> {
        style.validate()?;
        if width == 0 || height == 0 {
            return Err(SecretError::ZeroCanvas);
        }
        let font = style.font_pt as u32;
        let dens = style.density as u32;
        // 密度语义是「重复次数」而不是「间距」：密度越大步距越小。
        // 字号项保证字不压字（步距 ≥ 2 倍字号的下限由 40 - 4×8 = 8 兜住）。
        let step_x = (font * 2 + 40 - dens * 4) as u16;
        let step_y = (font + 30 - dens * 3) as u16;
        // 宽高非 0 且步距非 0，向上取整后至少 1 列 / 1 行。
        let cols = ((width as u32 + step_x as u32 - 1) / step_x as u32) as u16;
        let rows = ((height as u32 + step_y as u32 - 1) / step_y as u32) as u16;
        Ok(TileGrid {
            cols,
            rows,
            step_x,
            step_y,
        })
    }

    /// 总格数（列 × 行），用于密度单调性判据。
    pub fn tiles(&self) -> u32 {
        self.cols as u32 * self.rows as u32
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 按 4 字节小端把 u32 混入 FNV-1a 摘要。
fn fnv_step(h: &mut u64, v: u32) {
    let mut i = 0u32;
    while i < 4 {
        *h ^= ((v >> (i * 8)) & 0xff) as u64;
        *h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
}

/// 一次渲染的确定性快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub level: Level,
    pub copy_len: usize,
    pub opacity: u8,
    pub angle_q: i32,
    pub density: u8,
    pub font_pt: u8,
    pub cols: u16,
    pub rows: u16,
    pub step_x: u16,
    pub step_y: u16,
    pub digest: u64,
}

impl Snapshot {
    /// 由预设与画布生成快照；所有输入在这里二次校验（防止绕过 `custom` 手搓结构体）。
    pub fn of(p: &Preset, width: u16, height: u16) -> Result<Snapshot, SecretError> {
        p.style.validate()?;
        if p.copy.is_empty() {
            return Err(SecretError::EmptyCopy);
        }
        if p.copy != p.level.copy() {
            return Err(SecretError::CopyDrift);
        }
        let grid = TileGrid::new(&p.style, width, height)?;
        let bytes = p.copy.as_bytes();
        let mut d = FNV_OFFSET;
        fnv_step(&mut d, p.level.code() as u32);
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                fnv_step(&mut d, *b as u32);
            }
            i += 1;
        }
        fnv_step(&mut d, p.style.opacity as u32);
        fnv_step(&mut d, p.style.angle_mdeg as u32);
        fnv_step(&mut d, p.style.density as u32);
        fnv_step(&mut d, p.style.font_pt as u32);
        fnv_step(&mut d, grid.cols as u32);
        fnv_step(&mut d, grid.rows as u32);
        fnv_step(&mut d, grid.step_x as u32);
        fnv_step(&mut d, grid.step_y as u32);
        Ok(Snapshot {
            level: p.level,
            copy_len: bytes.len(),
            opacity: p.style.opacity,
            angle_q: p.style.angle_q(),
            density: p.style.density,
            font_pt: p.style.font_pt,
            cols: grid.cols,
            rows: grid.rows,
            step_x: grid.step_x,
            step_y: grid.step_y,
            digest: d,
        })
    }
}

/// 账本里的一条渲染记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderNote {
    pub level: Level,
    pub page: u16,
    pub digest: u64,
}

/// 渲染账本：成功落记录，失败只增拒绝计数（失败原因由返回值携带）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderLedger {
    pub notes: Vec<RenderNote>,
    pub rejected: u16,
}

impl RenderLedger {
    /// 空账本。
    pub fn new() -> RenderLedger {
        RenderLedger {
            notes: Vec::new(),
            rejected: 0,
        }
    }

    /// 记录一次渲染；快照失败时计入 rejected 并原样返回错误。
    pub fn record(
        &mut self,
        p: &Preset,
        page: u16,
        width: u16,
        height: u16,
    ) -> Result<Snapshot, SecretError> {
        match Snapshot::of(p, width, height) {
            Ok(s) => {
                self.notes.push(RenderNote {
                    level: p.level,
                    page,
                    digest: s.digest,
                });
                Ok(s)
            }
            Err(e) => {
                self.rejected = self.rejected.saturating_add(1);
                Err(e)
            }
        }
    }

    /// 成功记录条数。
    pub fn len(&self) -> usize {
        self.notes.len()
    }

    /// 某档成功渲染次数。
    pub fn count(&self, level: Level) -> u16 {
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.notes.len() {
            if let Some(x) = self.notes.get(i) {
                if x.level == level {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 同一 (等级, 页码) 的所有记录摘要是否一致（渲染幂等的账本级判据）。
    pub fn stable(&self) -> bool {
        let mut i = 0usize;
        while i < self.notes.len() {
            if let Some(a) = self.notes.get(i) {
                let mut j = i + 1;
                while j < self.notes.len() {
                    if let Some(b) = self.notes.get(j) {
                        if a.level == b.level && a.page == b.page && a.digest != b.digest {
                            return false;
                        }
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        true
    }

    /// 账本摘要（含拒绝计数），用于「账本可对账」。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.notes.len() {
            if let Some(n) = self.notes.get(i) {
                fnv_step(&mut h, n.level.code() as u32);
                fnv_step(&mut h, n.page as u32);
                fnv_step(&mut h, (n.digest & 0xffff_ffff) as u32);
                fnv_step(&mut h, (n.digest >> 32) as u32);
            }
            i += 1;
        }
        fnv_step(&mut h, self.rejected as u32);
        h
    }
}

/// 注入核：八类坏样本逐类断言具体诊断码，返回 (命中数, 总数)。
pub fn injection_core() -> (u32, u32) {
    let mut caught = 0u32;
    let mut total = 0u32;
    let base = Style {
        opacity: 32,
        angle_mdeg: 0,
        density: 2,
        font_pt: 14,
    };

    total += 1;
    if Preset::custom(Level::Confidential, "机密 ", base).err() == Some(SecretError::CopyDrift) {
        caught += 1;
    }

    total += 1;
    if Level::from_code(9).err() == Some(SecretError::UnknownLevel) {
        caught += 1;
    }

    total += 1;
    if Override::none().opacity(0).apply(base).err() == Some(SecretError::RangeOpacity) {
        caught += 1;
    }

    total += 1;
    if Override::none().angle(60_000).apply(base).err() == Some(SecretError::RangeAngle) {
        caught += 1;
    }

    total += 1;
    if Override::none().density(0).apply(base).err() == Some(SecretError::DensityZero) {
        caught += 1;
    }

    total += 1;
    if Override::none().font(4).apply(base).err() == Some(SecretError::RangeFont) {
        caught += 1;
    }

    total += 1;
    if Preset::custom(Level::Draft, "", base).err() == Some(SecretError::EmptyCopy) {
        caught += 1;
    }

    total += 1;
    if TileGrid::new(&base, 0, 600).err() == Some(SecretError::ZeroCanvas) {
        caught += 1;
    }

    (caught, total)
}

/// 取一次快照摘要；失败返回 None（判据里只关心「是否一致」）。
fn snapshot_digest(p: &Preset, w: u16, h: u16) -> Option<u64> {
    match Snapshot::of(p, w, h) {
        Ok(s) => Some(s.digest),
        Err(_) => None,
    }
}

/// F50046 判据集：预设 ×3 渲染一致且文案零漂移。
pub fn run_unx50046_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50046");

    let draft = Preset::builtin(Level::Draft);
    let conf = Preset::builtin(Level::Confidential);
    let top = Preset::builtin(Level::TopSecret);

    // 1 文案零漂移：三档文案与等级常量逐字节一致，重复取值也不变
    let copy_ok = draft.copy == "草稿"
        && conf.copy == "机密"
        && top.copy == "绝密"
        && draft.copy == Level::Draft.copy()
        && conf.copy == Level::Confidential.copy()
        && top.copy == Level::TopSecret.copy()
        && draft.copy.len() == 6
        && conf.copy.len() == 6
        && top.copy.len() == 6;
    s.add(
        "50046-J1-文案零漂移",
        copy_ok,
        if copy_ok { "" } else { "文案偏移" },
    );

    // 2 等级秩序：草稿 < 机密 < 绝密，且与码值同序
    let rank_ok = Level::Draft.rank() == 1
        && Level::Confidential.rank() == 2
        && Level::TopSecret.rank() == 3
        && Level::Draft.code() < Level::Confidential.code()
        && Level::Confidential.code() < Level::TopSecret.code();
    s.add(
        "50046-J1-等级秩单调",
        rank_ok,
        if rank_ok { "" } else { "等级秩乱序" },
    );

    // 3 默认样式随等级递增：不透明度/密度/字号单调不减，角度非增（更陡）
    let style_ok = draft.style.opacity < conf.style.opacity
        && conf.style.opacity < top.style.opacity
        && draft.style.density <= conf.style.density
        && conf.style.density <= top.style.density
        && draft.style.font_pt < conf.style.font_pt
        && conf.style.font_pt < top.style.font_pt
        && draft.style.angle_mdeg > conf.style.angle_mdeg
        && conf.style.angle_mdeg > top.style.angle_mdeg;
    s.add(
        "50046-J1-默认样式递增",
        style_ok,
        if style_ok { "" } else { "样式未随等级递增" },
    );

    // 4 三档渲染一致：每档重复渲染两次，摘要逐位相同
    let d1 = snapshot_digest(&draft, 800, 600);
    let c1 = snapshot_digest(&conf, 800, 600);
    let t1 = snapshot_digest(&top, 800, 600);
    let render_ok = d1.is_some()
        && d1 == snapshot_digest(&draft, 800, 600)
        && c1.is_some()
        && c1 == snapshot_digest(&conf, 800, 600)
        && t1.is_some()
        && t1 == snapshot_digest(&top, 800, 600);
    // 明细里用 0 表示「本档取不到快照」，避免任何可能 panic 的取值。
    let dv = match d1 {
        Some(v) => v,
        None => 0,
    };
    let cv = match c1 {
        Some(v) => v,
        None => 0,
    };
    let tv = match t1 {
        Some(v) => v,
        None => 0,
    };
    let det = format!("d={:08x} c={:08x} t={:08x}", dv, cv, tv);
    s.add_dyn("50046-J1-三档渲染确定", render_ok, &det);

    // 5 三档摘要互异：任意两档摘要不同（否则「三档」名存实亡）
    let distinct_ok = match (d1, c1, t1) {
        (Some(a), Some(b), Some(c)) => a != b && b != c && a != c,
        _ => false,
    };
    s.add(
        "50046-J1-三档摘要互异",
        distinct_ok,
        if distinct_ok { "" } else { "两档摘要撞车" },
    );

    // 6 越权文案必拒：尾部空格也视为漂移
    let tamper_bad = Preset::custom(Level::Confidential, "机密 ", conf.style)
        .err()
        == Some(SecretError::CopyDrift);
    let tamper_ok = Preset::custom(Level::Confidential, "机密", conf.style).is_ok();
    let tamper_ok = tamper_ok && tamper_bad;
    s.add(
        "50046-J1-越权文案必拒",
        tamper_ok,
        if tamper_ok { "" } else { "篡改文案漏过" },
    );

    // 7 未知等级码：3 与 255 都必须被拒
    let unknown_ok = Level::from_code(3).err() == Some(SecretError::UnknownLevel)
        && Level::from_code(255).err() == Some(SecretError::UnknownLevel)
        && Level::from_code(0).is_ok()
        && Level::from_code(2).is_ok();
    s.add(
        "50046-J1-未知等级码",
        unknown_ok,
        if unknown_ok { "" } else { "越界等级码被接受" },
    );

    // 8 不透明度区间：16/64 收，15/65 拒（闭区间两端都要测）
    let base = draft.style;
    let op_ok = Override::none().opacity(16).apply(base).is_ok()
        && Override::none().opacity(64).apply(base).is_ok()
        && Override::none().opacity(15).apply(base).err() == Some(SecretError::RangeOpacity)
        && Override::none().opacity(65).apply(base).err() == Some(SecretError::RangeOpacity);
    s.add(
        "50046-J1-不透明度区间",
        op_ok,
        if op_ok { "" } else { "不透明度区间失守" },
    );

    // 9 角度区间：±45000 收，±45001 拒
    let ang_ok = Override::none().angle(45_000).apply(base).is_ok()
        && Override::none().angle(-45_000).apply(base).is_ok()
        && Override::none().angle(45_001).apply(base).err() == Some(SecretError::RangeAngle)
        && Override::none().angle(-45_001).apply(base).err() == Some(SecretError::RangeAngle);
    s.add(
        "50046-J1-角度区间",
        ang_ok,
        if ang_ok { "" } else { "角度区间失守" },
    );

    // 10 角度量化幂等：量化结果必为 500 的整数倍，再量化不再变化
    let mut q_ok = true;
    let cases: [i32; 7] = [0, 250, -250, 1_499, -1_499, 45_000, -45_000];
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(x) = cases.get(i) {
            let q = quantize_angle(*x);
            if q % ANGLE_STEP != 0 || quantize_angle(q) != q {
                q_ok = false;
            }
        }
        i += 1;
    }
    // 半格向外舍入的两个定点：+250 → +500，-250 → -500。
    let q_ok = q_ok
        && quantize_angle(250) == 500
        && quantize_angle(-250) == -500
        && quantize_angle(1_499) == 1_500
        && quantize_angle(-1_499) == -1_500;
    s.add(
        "50046-J1-角度量化幂等",
        q_ok,
        if q_ok { "" } else { "量化不幂等" },
    );

    // 11 密度格数单调：密度 1..=8，步距非增、格数非减
    let mut dens_ok = true;
    let mut prev_step = u16::MAX;
    let mut prev_tiles = 0u32;
    let mut d = 1u8;
    while d <= DENSITY_MAX {
        let st = Style {
            opacity: 32,
            angle_mdeg: 0,
            density: d,
            font_pt: 16,
        };
        match TileGrid::new(&st, 800, 600) {
            Ok(g) => {
                if g.step_x > prev_step || g.tiles() < prev_tiles {
                    dens_ok = false;
                }
                prev_step = g.step_x;
                prev_tiles = g.tiles();
            }
            Err(_) => dens_ok = false,
        }
        d += 1;
    }
    s.add(
        "50046-J1-密度格数单调",
        dens_ok,
        if dens_ok { "" } else { "密度单调性断裂" },
    );

    // 12 零画布必拒：宽为 0 或高为 0 都不能出网格
    let zero_ok = TileGrid::new(&base, 0, 600).err() == Some(SecretError::ZeroCanvas)
        && TileGrid::new(&base, 800, 0).err() == Some(SecretError::ZeroCanvas);
    s.add(
        "50046-J1-零画布必拒",
        zero_ok,
        if zero_ok { "" } else { "零画布出网格" },
    );

    // 13 字号区间：8/48 收，7/49 拒
    let font_ok = Override::none().font(8).apply(base).is_ok()
        && Override::none().font(48).apply(base).is_ok()
        && Override::none().font(7).apply(base).err() == Some(SecretError::RangeFont)
        && Override::none().font(49).apply(base).err() == Some(SecretError::RangeFont);
    s.add(
        "50046-J1-字号区间",
        font_ok,
        if font_ok { "" } else { "字号区间失守" },
    );

    // 14 覆盖不动文案：restyle 后文案与等级原样保留
    let restyled = draft.restyle(Style {
        opacity: 20,
        angle_mdeg: -7_000,
        density: 1,
        font_pt: 10,
    });
    let restyle_ok = match restyled {
        Ok(p) => {
            p.copy == draft.copy
                && p.level == draft.level
                && p.style.opacity == 20
                && p.style.angle_mdeg == -7_000
                && Snapshot::of(&p, 640, 480).is_ok()
        }
        Err(_) => false,
    };
    s.add(
        "50046-J1-覆盖不动文案",
        restyle_ok,
        if restyle_ok { "" } else { "覆盖污染文案" },
    );

    // 15 账本守恒：9 次成功 + 2 次失败 = 11 次尝试，账本可对账
    let mut led = RenderLedger::new();
    let _ = led.record(&draft, 1, 800, 600);
    let _ = led.record(&draft, 2, 800, 600);
    let _ = led.record(&draft, 3, 800, 600);
    let _ = led.record(&conf, 1, 800, 600);
    let _ = led.record(&conf, 2, 800, 600);
    let _ = led.record(&conf, 3, 800, 600);
    let _ = led.record(&top, 1, 800, 600);
    let _ = led.record(&top, 2, 800, 600);
    let _ = led.record(&top, 3, 800, 600);
    let _ = led.record(&draft, 4, 0, 600);
    let _ = led.record(&top, 4, 800, 0);
    let ledger_ok = led.len() == 9
        && led.rejected == 2
        && led.count(Level::Draft) == 3
        && led.count(Level::Confidential) == 3
        && led.count(Level::TopSecret) == 3
        && led.digest() != 0;
    s.add(
        "50046-J1-账本守恒",
        ledger_ok,
        if ledger_ok { "" } else { "账本对不上" },
    );

    // 16 同页幂等落账：同档同页重复渲染，摘要不漂
    let mut led2 = RenderLedger::new();
    let a = led2.record(&conf, 7, 800, 600).is_ok();
    let b = led2.record(&conf, 7, 800, 600).is_ok();
    let c = led2.record(&top, 7, 800, 600).is_ok();
    let stable_ok = a && b && c && led2.stable() && led2.len() == 3;
    s.add(
        "50046-J1-同页幂等落账",
        stable_ok,
        if stable_ok { "" } else { "同页摘要漂移" },
    );

    // 17 快照字段对齐：快照的几何/样式字段与来源逐项相等，摘要可复算
    let field_ok = match Snapshot::of(&conf, 800, 600) {
        Ok(snap) => match TileGrid::new(&conf.style, 800, 600) {
            Ok(g) => {
                let again = snapshot_digest(&conf, 800, 600) == Some(snap.digest);
                snap.level == Level::Confidential
                    && snap.copy_len == conf.copy.len()
                    && snap.opacity == conf.style.opacity
                    && snap.angle_q == quantize_angle(conf.style.angle_mdeg)
                    && snap.density == conf.style.density
                    && snap.font_pt == conf.style.font_pt
                    && snap.cols == g.cols
                    && snap.rows == g.rows
                    && snap.step_x == g.step_x
                    && snap.step_y == g.step_y
                    && again
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add(
        "50046-J1-快照字段对齐",
        field_ok,
        if field_ok { "" } else { "快照字段错位" },
    );

    // 18 注入必抓：八类坏样本逐类断言具体码
    let (caught, total) = injection_core();
    let inj_ok = caught == total && total >= 8;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50046-J1-注入必抓", inj_ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50046_checks_green() {
        let set = run_unx50046_checks();
        assert!(set.all_passed(), "UNX-F50046 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50046_copy_stays_canonical() {
        let levels: [Level; 3] = [Level::Draft, Level::Confidential, Level::TopSecret];
        let mut i = 0usize;
        while i < levels.len() {
            if let Some(lv) = levels.get(i) {
                let p = Preset::builtin(*lv);
                assert_eq!(p.copy, lv.copy());
                assert!(Preset::custom(*lv, "草稿x", p.style).is_err());
                let snap = Snapshot::of(&p, 320, 240);
                assert!(snap.is_ok());
                if let Ok(snap) = snap {
                    assert_eq!(snap.copy_len, p.copy.len());
                }
            }
            i += 1;
        }
    }

    #[test]
    fn unx50046_quantize_and_ledger_stable() {
        let mut x = -50_000i32;
        while x <= 50_000 {
            let q = quantize_angle(x);
            assert_eq!(q % ANGLE_STEP, 0);
            assert_eq!(quantize_angle(q), q);
            x += 137;
        }
        let mut led = RenderLedger::new();
        let p = Preset::builtin(Level::TopSecret);
        assert!(led.record(&p, 1, 800, 600).is_ok());
        assert!(led.record(&p, 1, 800, 600).is_ok());
        assert!(led.stable());
        assert_eq!(led.rejected, 0);
        assert_eq!(led.count(Level::TopSecret), 2);
    }
}
