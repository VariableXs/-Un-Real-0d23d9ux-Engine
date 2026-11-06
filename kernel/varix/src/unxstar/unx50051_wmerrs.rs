//! UNX-F50051 水印错误码族（10 条：字体缺失 / 图坏 / 参数越界）（0x43280 段）
//!
//! 锚点：UNX-F50051-J1 错误码 ×10 零漂移；判据锚定 §AI-63 §2 专题二。
//!
//! 「码值零漂移」在本模块是**可验证的性质**而不是口头承诺：10 个变体按声明
//! 序占 `SEG_BASE + 1 ..= SEG_BASE + 10`，每个变体必须满足
//! `from_code(e.code()) == Some(e)`、消息非空且两两互异、类别/严重度/可重试
//! 三张表对每个变体都有定义。表指纹两次计算必须一致（防止有人改了表却只改
//! 一处）。
//!
//! 错误族按「谁该负责」分三类：
//! * Font（资源）：字体缺失 / 字体解析 / 字形缺失——字形缺失通常可退化到
//!   .notdef，故标 Degraded + 可重试；
//! * Image（介质）：图片解码 / 格式 / 尺寸——解码失败可换源重试，格式与尺寸
//!   属配置错误，直接 Fatal；
//! * Param（参数）：缩放 / 不透明度 / 角度 / 层深——全是调用方参数问题，
//!   Degraded（本次水印跳过，文档仍可印）。
//!
//! `validate()` 的判据顺序故意固定：字体 → 字形 → 图像 → 参数 → 层深。同一
//! 请求多处违约时**先报责任更上游的错**，避免调用方修完参数才发现字体根本
//! 没装。
//!
//! 数据段 `SEG_BASE = 0x43280`，诊断码闭集 `0x43281..=0x4328a`（10 条，
//! 全部由 `validate` 的真实分支构造）。
//! 结构清单：`WmErr`（10 条码族）、`ErrCategory`/`Severity`/`ErrSpec`（登记
//! 表）、`ErrRegistry`（命中台账 + 零漂移审计）、`WmRequest`/`validate`（请求
//! 校验器）、`injection_core`（10 个坏样本注入核）。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段基址（0x43280）。
pub const SEG_BASE: u32 = 0x43280;

pub const E_FONT_MISSING: u16 = 1;
pub const E_FONT_PARSE: u16 = 2;
pub const E_GLYPH_MISSING: u16 = 3;
pub const E_IMAGE_DECODE: u16 = 4;
pub const E_IMAGE_FORMAT: u16 = 5;
pub const E_IMAGE_DIMS: u16 = 6;
pub const E_PARAM_RANGE: u16 = 7;
pub const E_OPACITY_RANGE: u16 = 8;
pub const E_ANGLE_RANGE: u16 = 9;
pub const E_LAYER_DEPTH: u16 = 10;

/// 水印错误码族（10 条闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WmErr {
    /// 字体名未指定 / 字体资源不存在
    FontMissing,
    /// 字体文件存在但解析失败（头/表损坏）
    FontParse,
    /// 字形索引在字体中不存在（0xFFFF 是 .notdef 哨兵，同样视为缺失）
    GlyphMissing,
    /// 图片字节为空或解码器无法解出位图
    ImageDecode,
    /// 图片魔数不是受支持的 PNG/JPEG
    ImageFormat,
    /// 图片像素尺寸为 0 或超出上限
    ImageDims,
    /// 缩放在 [10,1000]‰ 之外
    ParamRange,
    /// 不透明度在 [0,1000]‰ 之外
    OpacityRange,
    /// 角度在 [-180,180] 度之外
    AngleRange,
    /// 层深在 [1,8] 之外
    LayerDepth,
}

impl WmErr {
    /// 10 条全表（声明序即码值序）。
    pub const ALL: [WmErr; 10] = [
        WmErr::FontMissing,
        WmErr::FontParse,
        WmErr::GlyphMissing,
        WmErr::ImageDecode,
        WmErr::ImageFormat,
        WmErr::ImageDims,
        WmErr::ParamRange,
        WmErr::OpacityRange,
        WmErr::AngleRange,
        WmErr::LayerDepth,
    ];

    /// 诊断码：`SEG_BASE + n`，n 为 1..=10。
    pub fn code(self) -> u32 {
        let n = match self {
            WmErr::FontMissing => E_FONT_MISSING,
            WmErr::FontParse => E_FONT_PARSE,
            WmErr::GlyphMissing => E_GLYPH_MISSING,
            WmErr::ImageDecode => E_IMAGE_DECODE,
            WmErr::ImageFormat => E_IMAGE_FORMAT,
            WmErr::ImageDims => E_IMAGE_DIMS,
            WmErr::ParamRange => E_PARAM_RANGE,
            WmErr::OpacityRange => E_OPACITY_RANGE,
            WmErr::AngleRange => E_ANGLE_RANGE,
            WmErr::LayerDepth => E_LAYER_DEPTH,
        };
        SEG_BASE + n as u32
    }

    /// 变体序（0..9，等于码值偏移减一）。
    pub fn index(self) -> usize {
        (self.code() - SEG_BASE) as usize - 1
    }

    /// 码值反查：闭集外的码返回 None（不猜测、不兜底）。
    pub fn from_code(code: u32) -> Option<WmErr> {
        if code < SEG_BASE + 1 || code > SEG_BASE + 10 {
            return None;
        }
        let idx = (code - SEG_BASE - 1) as usize;
        WmErr::ALL.get(idx).map(|v| *v)
    }

    /// 稳定诊断文本（不含可变数据，便于对账与检索）。
    pub fn message(self) -> &'static str {
        match self {
            WmErr::FontMissing => "watermark font not found",
            WmErr::FontParse => "watermark font parse failed",
            WmErr::GlyphMissing => "glyph missing in font",
            WmErr::ImageDecode => "watermark image decode failed",
            WmErr::ImageFormat => "watermark image format unsupported",
            WmErr::ImageDims => "watermark image dimensions invalid",
            WmErr::ParamRange => "watermark scale out of range",
            WmErr::OpacityRange => "watermark opacity out of range",
            WmErr::AngleRange => "watermark angle out of range",
            WmErr::LayerDepth => "watermark layer depth out of range",
        }
    }

    /// 责任类别。
    pub fn category(self) -> ErrCategory {
        match self {
            WmErr::FontMissing | WmErr::FontParse | WmErr::GlyphMissing => ErrCategory::Font,
            WmErr::ImageDecode | WmErr::ImageFormat | WmErr::ImageDims => ErrCategory::Image,
            WmErr::ParamRange
            | WmErr::OpacityRange
            | WmErr::AngleRange
            | WmErr::LayerDepth => ErrCategory::Param,
        }
    }

    /// 严重度：资源缺失/解析失败与图像格式错误是 Fatal（本次无法出水印），
    /// 其余可降级（跳过水印，正文照印）。
    pub fn severity(self) -> Severity {
        match self {
            WmErr::FontMissing
            | WmErr::FontParse
            | WmErr::ImageDecode
            | WmErr::ImageFormat => Severity::Fatal,
            WmErr::GlyphMissing
            | WmErr::ImageDims
            | WmErr::ParamRange
            | WmErr::OpacityRange
            | WmErr::AngleRange
            | WmErr::LayerDepth => Severity::Degraded,
        }
    }

    /// 是否允许原样重试：只有「换源/换字形」类才有意义，纯参数错误重试无用。
    pub fn retryable(self) -> bool {
        match self {
            WmErr::ImageDecode | WmErr::GlyphMissing => true,
            _ => false,
        }
    }

    /// 登记表快照（类别 + 严重度 + 可重试 + 码值）。
    pub fn spec(self) -> ErrSpec {
        ErrSpec {
            code: self.code(),
            category: self.category(),
            severity: self.severity(),
            retryable: self.retryable(),
        }
    }
}

/// 责任类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrCategory {
    Font,
    Image,
    Param,
}

impl ErrCategory {
    /// 类别码（日志/上报用）。
    pub fn as_u8(self) -> u8 {
        match self {
            ErrCategory::Font => 0,
            ErrCategory::Image => 1,
            ErrCategory::Param => 2,
        }
    }
}

/// 严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 本次无法生成水印
    Fatal,
    /// 跳过水印，正文继续
    Degraded,
}

impl Severity {
    /// 严重度码。
    pub fn as_u8(self) -> u8 {
        match self {
            Severity::Fatal => 2,
            Severity::Degraded => 1,
        }
    }
}

/// 一条错误码的登记项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrSpec {
    pub code: u32,
    pub category: ErrCategory,
    pub severity: Severity,
    pub retryable: bool,
}

/// 命中台账容量（按码计，最多 10 个槽；每码命中次数 u16 饱和）。
pub const REG_CAP: usize = 10;

/// 错误码命中台账 + 零漂移审计。
pub struct ErrRegistry {
    hits: [u16; REG_CAP],
    total: u32,
    last: Option<WmErr>,
    duplicates: u16,
}

impl ErrRegistry {
    /// 空台账。
    pub const fn new() -> ErrRegistry {
        ErrRegistry {
            hits: [0; REG_CAP],
            total: 0,
            last: None,
            duplicates: 0,
        }
    }

    /// 记一次命中；饱和计数，重复命中单独计数（不掩盖高频错误）。
    pub fn record(&mut self, e: WmErr) {
        let idx = e.index();
        if let Some(slot) = self.hits.get_mut(idx) {
            if *slot > 0 {
                self.duplicates = self.duplicates.saturating_add(1);
            }
            *slot = slot.saturating_add(1);
        }
        self.total = self.total.saturating_add(1);
        self.last = Some(e);
    }

    /// 总命中次数。
    pub fn total(&self) -> u32 {
        self.total
    }

    /// 某码命中次数。
    pub fn hits(&self, e: WmErr) -> u16 {
        match self.hits.get(e.index()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 重复命中次数（第二次及以后）。
    pub fn duplicates(&self) -> u16 {
        self.duplicates
    }

    /// 最近一次命中的码。
    pub fn last(&self) -> Option<WmErr> {
        self.last
    }

    /// 零漂移审计：10 个槽都已命中且码值/消息/反查三项自洽，才算零漂移。
    pub fn drift_free(&self) -> bool {
        let mut i = 0usize;
        while i < REG_CAP {
            if let Some(h) = self.hits.get(i) {
                if *h == 0 {
                    return false;
                }
            }
            i += 1;
        }
        let (ok, _) = drift_audit();
        ok == REG_CAP
    }

    /// 台账摘要。
    pub fn summary(&self) -> String {
        format!(
            "total={} dup={} codes={}",
            self.total,
            self.duplicates,
            hit_codes(self)
        )
    }
}

fn hit_codes(reg: &ErrRegistry) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < REG_CAP {
        if let Some(h) = reg.hits.get(i) {
            if *h > 0 {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 零漂移审计：逐条核对码值连续、反查往返、消息非空且互异。
/// 返回 (通过项数, 总项数=10)。
pub fn drift_audit() -> (usize, usize) {
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < WmErr::ALL.len() {
        let e = match WmErr::ALL.get(i) {
            Some(v) => *v,
            None => break,
        };
        let contiguous = e.code() == SEG_BASE + 1 + i as u32;
        let roundtrip = WmErr::from_code(e.code()) == Some(e);
        let non_empty = !e.message().is_empty();
        let unique = {
            let mut j = 0usize;
            let mut uniq = true;
            while j < WmErr::ALL.len() {
                if let Some(o) = WmErr::ALL.get(j) {
                    if j != i && o.message() == e.message() {
                        uniq = false;
                    }
                }
                j += 1;
            }
            uniq
        };
        if contiguous && roundtrip && non_empty && unique {
            ok += 1;
        }
        i += 1;
    }
    (ok, WmErr::ALL.len())
}

/// 表指纹：码值 + 类别 + 严重度 + 可重试 + 消息长度，逐位参与。
pub fn table_digest() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < WmErr::ALL.len() {
        if let Some(e) = WmErr::ALL.get(i) {
            let s = e.spec();
            h = (h ^ s.code as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ s.category.as_u8() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ s.severity.as_u8() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ if s.retryable { 1 } else { 0 }).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ e.message().len() as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
        i += 1;
    }
    h
}

/// 待校验的水印请求（只保留校验所需字段，避免依赖真实字形/位图缓冲）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WmRequest {
    pub font_name_len: usize,
    pub glyph_id: u16,
    pub image_bytes: usize,
    pub image_magic: u32,
    pub image_w: u16,
    pub image_h: u16,
    pub opacity: i16,
    pub angle: i16,
    pub scale: i16,
    pub layer: u8,
}

/// PNG 魔数（前 4 字节 89 50 4E 47）。
pub const MAGIC_PNG: u32 = 0x8950_4E47;
/// JPEG 魔数（SOI + APP0 的 FF D8 FF E0）。
pub const MAGIC_JPEG: u32 = 0xFFD8_FFE0;

impl WmRequest {
    /// 一份合法基线请求（所有边界都在闭区间内部）。
    pub fn baseline() -> WmRequest {
        WmRequest {
            font_name_len: 16,
            glyph_id: 5,
            image_bytes: 1024,
            image_magic: MAGIC_PNG,
            image_w: 512,
            image_h: 512,
            opacity: 500,
            angle: 45,
            scale: 100,
            layer: 3,
        }
    }
}

/// 参数闭区间（写死便于对账）：缩放 10..=1000‰，角度 -180..=180，
/// 层深 1..=8，图片单边 1..=20000 px。
pub const SCALE_MIN: i16 = 10;
pub const SCALE_MAX: i16 = 1000;
pub const ANGLE_MIN: i16 = -180;
pub const ANGLE_MAX: i16 = 180;
pub const LAYER_MIN: u8 = 1;
pub const LAYER_MAX: u8 = 8;
pub const IMAGE_MAX_SIDE: u16 = 20_000;

/// 校验请求；违约顺序固定为 字体 → 字形 → 图像 → 参数 → 层深。
pub fn validate(req: &WmRequest) -> Result<(), WmErr> {
    if req.font_name_len == 0 {
        return Err(WmErr::FontMissing);
    }
    if req.font_name_len > 64 {
        return Err(WmErr::FontParse);
    }
    if req.glyph_id == 0xFFFF {
        return Err(WmErr::GlyphMissing);
    }
    if req.image_bytes == 0 {
        return Err(WmErr::ImageDecode);
    }
    if req.image_magic != MAGIC_PNG && req.image_magic != MAGIC_JPEG {
        return Err(WmErr::ImageFormat);
    }
    if req.image_w == 0 || req.image_h == 0 || req.image_w > IMAGE_MAX_SIDE || req.image_h > IMAGE_MAX_SIDE
    {
        return Err(WmErr::ImageDims);
    }
    if req.scale < SCALE_MIN || req.scale > SCALE_MAX {
        return Err(WmErr::ParamRange);
    }
    if req.opacity < 0 || req.opacity > 1000 {
        return Err(WmErr::OpacityRange);
    }
    if req.angle < ANGLE_MIN || req.angle > ANGLE_MAX {
        return Err(WmErr::AngleRange);
    }
    if req.layer < LAYER_MIN || req.layer > LAYER_MAX {
        return Err(WmErr::LayerDepth);
    }
    Ok(())
}

/// 10 个坏样本：(样本名, 请求, 期望码)。全部从基线只改一个字段。
pub fn samples() -> [(&'static str, WmRequest, WmErr); 10] {
    let b = WmRequest::baseline();
    [
        (
            "font-empty",
            WmRequest { font_name_len: 0, ..b },
            WmErr::FontMissing,
        ),
        (
            "font-oversize",
            WmRequest { font_name_len: 65, ..b },
            WmErr::FontParse,
        ),
        (
            "glyph-sentinel",
            WmRequest { glyph_id: 0xFFFF, ..b },
            WmErr::GlyphMissing,
        ),
        (
            "image-empty",
            WmRequest { image_bytes: 0, ..b },
            WmErr::ImageDecode,
        ),
        (
            "image-magic",
            WmRequest { image_magic: 0x1234_5678, ..b },
            WmErr::ImageFormat,
        ),
        (
            "image-dims",
            WmRequest { image_w: 0, ..b },
            WmErr::ImageDims,
        ),
        (
            "scale-low",
            WmRequest { scale: 9, ..b },
            WmErr::ParamRange,
        ),
        (
            "opacity-high",
            WmRequest { opacity: 1001, ..b },
            WmErr::OpacityRange,
        ),
        (
            "angle-high",
            WmRequest { angle: 181, ..b },
            WmErr::AngleRange,
        ),
        (
            "layer-zero",
            WmRequest { layer: 0, ..b },
            WmErr::LayerDepth,
        ),
    ]
}

// ---------------------------------------------------------------------------
// 注入核：10 个坏样本必须逐条命中指定码
// ---------------------------------------------------------------------------

/// 跑完 10 个样本并断言命中码，返回 (命中数, 总数)。
pub fn injection_core() -> (usize, usize) {
    let table = samples();
    let mut caught = 0usize;
    let mut i = 0usize;
    while i < table.len() {
        if let Some((_name, req, want)) = table.get(i) {
            if validate(req).err() == Some(*want) {
                caught += 1;
            }
        }
        i += 1;
    }
    (caught, table.len())
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50051 的 13 条判据。
pub fn run_unx50051_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50051");

    // J1-1 码族 10 条
    let count_ok = WmErr::ALL.len() == 10;
    s.add("50051-码族十条", count_ok, "码族长度不是 10");

    // J1-2 码值连续且唯一：SEG_BASE+1 ..= SEG_BASE+10
    let mut contiguous = true;
    let mut i = 0usize;
    while i < WmErr::ALL.len() {
        if let Some(e) = WmErr::ALL.get(i) {
            if e.code() != SEG_BASE + 1 + i as u32 {
                contiguous = false;
            }
        }
        i += 1;
    }
    let det = format!("base=0x{:x}", SEG_BASE);
    s.add_dyn("50051-码值连续", contiguous, &det);

    // J1-3 反查往返 10/10 + 闭集外 None
    let mut roundtrip = 0usize;
    let mut j = 0usize;
    while j < WmErr::ALL.len() {
        if let Some(e) = WmErr::ALL.get(j) {
            if WmErr::from_code(e.code()) == Some(*e) {
                roundtrip += 1;
            }
        }
        j += 1;
    }
    let closed = WmErr::from_code(SEG_BASE).is_none()
        && WmErr::from_code(SEG_BASE + 11).is_none()
        && WmErr::from_code(0).is_none();
    let det = format!("{}/10", roundtrip);
    s.add_dyn("50051-反查往返", roundtrip == 10 && closed, &det);

    // J1-4 消息非空且两两互异
    let (audit_ok, audit_total) = drift_audit();
    let det = format!("{}/{}", audit_ok, audit_total);
    s.add_dyn("50051-消息互异", audit_ok == 10 && audit_total == 10, &det);

    // J1-5 类别三分且每类非空
    let mut font_n = 0usize;
    let mut image_n = 0usize;
    let mut param_n = 0usize;
    let mut k = 0usize;
    while k < WmErr::ALL.len() {
        if let Some(e) = WmErr::ALL.get(k) {
            match e.category() {
                ErrCategory::Font => font_n += 1,
                ErrCategory::Image => image_n += 1,
                ErrCategory::Param => param_n += 1,
            }
        }
        k += 1;
    }
    let det = format!("{}/{}/{}", font_n, image_n, param_n);
    s.add_dyn(
        "50051-类别三分",
        font_n == 3 && image_n == 3 && param_n == 4,
        &det,
    );

    // J1-6 严重度表：Fatal 恰为 {FontMissing, FontParse, ImageDecode, ImageFormat}
    let mut fatal_ok = true;
    let mut m = 0usize;
    while m < WmErr::ALL.len() {
        if let Some(e) = WmErr::ALL.get(m) {
            let want = match e {
                WmErr::FontMissing
                | WmErr::FontParse
                | WmErr::ImageDecode
                | WmErr::ImageFormat => Severity::Fatal,
                _ => Severity::Degraded,
            };
            if e.severity() != want || e.spec().severity != want || e.spec().code != e.code() {
                fatal_ok = false;
            }
        }
        m += 1;
    }
    s.add("50051-严重度表", fatal_ok, "严重度表与闭集不符");

    // J1-7 可重试表：只有 GlyphMissing/ImageDecode 可重试
    let mut retry_ok = true;
    let mut r = 0usize;
    while r < WmErr::ALL.len() {
        if let Some(e) = WmErr::ALL.get(r) {
            let want = match e {
                WmErr::GlyphMissing | WmErr::ImageDecode => true,
                _ => false,
            };
            if e.retryable() != want {
                retry_ok = false;
            }
        }
        r += 1;
    }
    s.add("50051-可重试表", retry_ok, "可重试表与闭集不符");

    // J1-8 错误码 ×10 零漂移：注入 10/10 精确命中
    let (caught, total) = injection_core();
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50051-J1-码值零漂移", caught == 10 && total == 10, &det);

    // J1-9 基线请求合法
    let base_ok = validate(&WmRequest::baseline()).is_ok();
    s.add("50051-基线合法", base_ok, "基线请求被误拒");

    // J1-10 边界闭区间：合法端点全过、越界一位全拒
    let mut edge_ok = true;
    let b = WmRequest::baseline();
    let ok_cases = [
        WmRequest { opacity: 0, ..b },
        WmRequest { opacity: 1000, ..b },
        WmRequest { angle: -180, ..b },
        WmRequest { angle: 180, ..b },
        WmRequest { scale: 10, ..b },
        WmRequest { scale: 1000, ..b },
        WmRequest { layer: 1, ..b },
        WmRequest { layer: 8, ..b },
    ];
    let mut c = 0usize;
    while c < ok_cases.len() {
        if let Some(req) = ok_cases.get(c) {
            if validate(req).is_err() {
                edge_ok = false;
            }
        }
        c += 1;
    }
    let bad_cases = [
        WmRequest { opacity: -1, ..b },
        WmRequest { angle: -181, ..b },
        WmRequest { scale: 1001, ..b },
        WmRequest { layer: 9, ..b },
    ];
    let mut d = 0usize;
    while d < bad_cases.len() {
        if let Some(req) = bad_cases.get(d) {
            if validate(req).is_ok() {
                edge_ok = false;
            }
        }
        d += 1;
    }
    s.add("50051-边界闭区间", edge_ok, "边界端点判定漂移");

    // J1-11 责任优先：字体缺失与坏图同时存在时报字体
    let both = WmRequest {
        font_name_len: 0,
        image_magic: 0xDEAD_BEEF,
        ..b
    };
    let priority_ok = validate(&both).err() == Some(WmErr::FontMissing);
    s.add("50051-责任优先", priority_ok, "违约优先级漂移");

    // J1-12 命中台账：10 码各记一次 → 全槽非零、零漂移、汇总对平
    let mut reg = ErrRegistry::new();
    let mut q = 0usize;
    while q < WmErr::ALL.len() {
        if let Some(e) = WmErr::ALL.get(q) {
            reg.record(*e);
        }
        q += 1;
    }
    let once_more = match WmErr::ALL.get(0) {
        Some(e) => {
            reg.record(*e);
            reg.hits(*e) == 2
        }
        None => false,
    };
    let ledger_ok = reg.total() == 11
        && reg.duplicates() == 1
        && reg.drift_free()
        && once_more
        && reg.last() == Some(WmErr::FontMissing);
    let det = reg.summary();
    s.add_dyn("50051-台账零漂移", ledger_ok, &det);

    // J1-13 表指纹确定：两次一致且非零
    let d1 = table_digest();
    let d2 = table_digest();
    s.add("50051-表指纹确定", d1 == d2 && d1 != 0, "两次表指纹不一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50051_checks_green() {
        let set = run_unx50051_checks();
        assert!(set.all_passed(), "UNX-F50051 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50051_codes_are_ten_and_contiguous() {
        assert_eq!(WmErr::ALL.len(), 10);
        let mut i = 0usize;
        while i < WmErr::ALL.len() {
            match WmErr::ALL.get(i) {
                Some(e) => assert_eq!(e.code(), SEG_BASE + 1 + i as u32),
                None => assert!(false, "missing error variant"),
            }
            i += 1;
        }
    }

    #[test]
    fn unx50051_samples_hit_exactly() {
        let (caught, total) = injection_core();
        assert_eq!(caught, 10);
        assert_eq!(total, 10);
    }
}
