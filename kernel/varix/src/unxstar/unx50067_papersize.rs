//! UNX-F50067 纸张尺寸归一（ISO/US/信封逐族映射表）（0x43480 段）
//!
//! 锚点：UNX-F50067-J1 映射表 ×40 纸型零漂移；判据锚定 §AI-63 §2 专题四。
//!
//! 数据段：`SEG_BASE = 0x43480`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 10`，
//! 逐项见 [`PaperError`]。新增码必须整段扩容，不得借位相邻卡
//! （0x43460 水印卡 / 0x434a0 自定义纸卡）。
//!
//! 纸张归一只有一个真问题：**作业里送来的一对毫米数，到底是哪张标准纸**。
//! 驱动、IPP、PDL 三路来源的写法互不相同（`A4` / `iso_a4` / `210x297` /
//! `8.5x11in`），尺寸又可能带扫描噪声或横放。本模块把三件事拆开处理：
//! * 族与码：40 条标准纸的**唯一真值表**（ISO A/B/C、US、ANSI、信封），
//!   宽度/高度以 0.1mm 整数存储——整数比较让「零漂移」可逐位判定，
//!   不引入任何浮点舍入；
//! * 归属：输入对按「短边/长边」归一后与真值表比较，漂移 = 两边绝对差之和，
//!   超出容差即 `NoMatch`；横放输入用 `Rotation::Cw90` 显式标注，绝不静默
//!   改写用户朝向；
//! * 解析：`parse_spec` 接受码、族前缀码、`WxH`（mm/in）三类写法，
//!   解析失败按原因分码（空、未知码、坏数、坏单位、超上限），不做就近猜测。
//!
//! 结构清单：
//! * [`PaperSize`] + [`PAPER_TABLE`]：40 条标准纸真值表；
//! * [`PaperFamily`]：六族闭集，族码非法显式拒绝；
//! * [`Dimensions`]：输入尺寸对（0.1mm 整数）；
//! * [`Registry`]：真值表注册表（码/族查询、最近邻归一、可扩容但有 64 条硬顶）；
//! * [`parse_spec`] / [`to_spec`]：文本 ↔ 尺寸的往返解析；
//! * [`injection_core`]：7 类坏样本注入核（逐类断言诊断码）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43480 独占 0x20）。
pub const SEG_BASE: u32 = 0x43480;

/// 0.1mm 计的最大边长：2000mm。再大属于大幅面绘图仪，不在本映射表职责内。
pub const MAX_DMM: u32 = 20000;

const E_EMPTY_NAME: u16 = 1;
const E_UNKNOWN_TOKEN: u16 = 2;
const E_BAD_DIMENSION: u16 = 3;
const E_UNIT: u16 = 4;
const E_TOO_LARGE: u16 = 5;
const E_NO_MATCH: u16 = 6;
const E_DUPLICATE_CODE: u16 = 7;
const E_REGISTRY_FULL: u16 = 8;
const E_BAD_ORIENTATION: u16 = 9;
const E_BAD_FAMILY: u16 = 10;

/// 纸张归一错误十型。`code()` 恒为 `SEG_BASE + n`，闭集内唯一。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperError {
    /// 文本为空（或族前缀后为空），没有可解析的主体。
    EmptyName,
    /// 码/别名不在真值表内。
    UnknownToken,
    /// 尺寸不是正数，或数字格式非法（多个小数点、缺整数部分等）。
    BadDimension,
    /// 单位不是 mm/in。
    UnsupportedUnit,
    /// 单边超过 2000mm 上限。
    TooLarge,
    /// 最近的候选纸也超出容差。
    NoMatch,
    /// 注册表插入的纸码已存在。
    DuplicateCode,
    /// 注册表达到 64 条硬顶。
    RegistryFull,
    /// 旋转码不在 0..=1 闭集。
    BadOrientation,
    /// 族码不在 0..=5 闭集。
    BadFamily,
}

impl PaperError {
    /// 段内序号（1 起），与 `code()` 一一对应。
    fn ordinal(self) -> u16 {
        match self {
            PaperError::EmptyName => E_EMPTY_NAME,
            PaperError::UnknownToken => E_UNKNOWN_TOKEN,
            PaperError::BadDimension => E_BAD_DIMENSION,
            PaperError::UnsupportedUnit => E_UNIT,
            PaperError::TooLarge => E_TOO_LARGE,
            PaperError::NoMatch => E_NO_MATCH,
            PaperError::DuplicateCode => E_DUPLICATE_CODE,
            PaperError::RegistryFull => E_REGISTRY_FULL,
            PaperError::BadOrientation => E_BAD_ORIENTATION,
            PaperError::BadFamily => E_BAD_FAMILY,
        }
    }

    /// 诊断码：段基址 + 序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    /// 一行诊断文本（驱动日志/联签回执用）。
    pub fn message(self) -> &'static str {
        match self {
            PaperError::EmptyName => "paper name empty",
            PaperError::UnknownToken => "paper code not in table",
            PaperError::BadDimension => "dimension not a positive decimal",
            PaperError::UnsupportedUnit => "unit is neither mm nor in",
            PaperError::TooLarge => "dimension above 2000mm cap",
            PaperError::NoMatch => "no standard paper within tolerance",
            PaperError::DuplicateCode => "paper code already registered",
            PaperError::RegistryFull => "paper registry at capacity",
            PaperError::BadOrientation => "orientation code invalid",
            PaperError::BadFamily => "paper family code invalid",
        }
    }
}

/// 全错误表：判据据此验证「码值 = 基址 + 序号」的零漂移性质。
pub const ALL_ERRORS: [PaperError; 10] = [
    PaperError::EmptyName,
    PaperError::UnknownToken,
    PaperError::BadDimension,
    PaperError::UnsupportedUnit,
    PaperError::TooLarge,
    PaperError::NoMatch,
    PaperError::DuplicateCode,
    PaperError::RegistryFull,
    PaperError::BadOrientation,
    PaperError::BadFamily,
];

/// 纸张族六类。族只用于展示与批量筛选，不参与尺寸匹配——匹配一律走尺寸，
/// 避免「族内最近邻」在跨族同尺寸（如 ANSI B 与 Ledger）上产生歧义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperFamily {
    IsoA,
    IsoB,
    IsoC,
    Us,
    Ansi,
    Envelope,
}

impl PaperFamily {
    /// 六族全表（判据遍历用）。
    pub const ALL: [PaperFamily; 6] = [
        PaperFamily::IsoA,
        PaperFamily::IsoB,
        PaperFamily::IsoC,
        PaperFamily::Us,
        PaperFamily::Ansi,
        PaperFamily::Envelope,
    ];

    /// 从外部族码解析；越界显式拒绝。
    pub fn from_code(v: u8) -> Result<PaperFamily, PaperError> {
        match v {
            0 => Ok(PaperFamily::IsoA),
            1 => Ok(PaperFamily::IsoB),
            2 => Ok(PaperFamily::IsoC),
            3 => Ok(PaperFamily::Us),
            4 => Ok(PaperFamily::Ansi),
            5 => Ok(PaperFamily::Envelope),
            _ => Err(PaperError::BadFamily),
        }
    }

    /// 对外族码。
    pub fn code(self) -> u8 {
        match self {
            PaperFamily::IsoA => 0,
            PaperFamily::IsoB => 1,
            PaperFamily::IsoC => 2,
            PaperFamily::Us => 3,
            PaperFamily::Ansi => 4,
            PaperFamily::Envelope => 5,
        }
    }

    /// 展示名。
    pub fn label(self) -> &'static str {
        match self {
            PaperFamily::IsoA => "ISO A",
            PaperFamily::IsoB => "ISO B",
            PaperFamily::IsoC => "ISO C",
            PaperFamily::Us => "US",
            PaperFamily::Ansi => "ANSI",
            PaperFamily::Envelope => "Envelope",
        }
    }
}

/// 一条标准纸。尺寸单位为 0.1mm，恒以「宽 ≤ 高」的肖像口径登记；
/// 横放只是使用朝向，不产生第二条真值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaperSize {
    pub code: &'static str,
    pub family: PaperFamily,
    pub width_dmm: u32,
    pub height_dmm: u32,
}

impl PaperSize {
    /// 常量构造（真值表在编译期落定，运行期零初始化）。
    pub const fn new(
        code: &'static str,
        family: PaperFamily,
        width_dmm: u32,
        height_dmm: u32,
    ) -> PaperSize {
        PaperSize {
            code,
            family,
            width_dmm,
            height_dmm,
        }
    }

    /// 面积（0.01mm² 单位 u64，避免 u32 溢出）。
    pub fn area_dmm2(&self) -> u64 {
        (self.width_dmm as u64) * (self.height_dmm as u64)
    }

    /// 是否肖像登记（真值表纪律，判据逐条核验）。
    pub fn is_portrait(&self) -> bool {
        self.width_dmm <= self.height_dmm
    }
}

/// 40 条标准纸真值表。顺序即最近邻的决胜顺序（同漂移取先登记者），
/// 因此顺序是契约的一部分：A0..A10、B0..B10、C0..C6、US 5、ANSI 3、信封 3。
/// 尺寸取自 ISO 216 / ISO 269 / ANSI Y14.1 / US 商用信封惯例。
pub const PAPER_TABLE: [PaperSize; 40] = [
    PaperSize::new("A0", PaperFamily::IsoA, 8410, 11890),
    PaperSize::new("A1", PaperFamily::IsoA, 5940, 8410),
    PaperSize::new("A2", PaperFamily::IsoA, 4200, 5940),
    PaperSize::new("A3", PaperFamily::IsoA, 2970, 4200),
    PaperSize::new("A4", PaperFamily::IsoA, 2100, 2970),
    PaperSize::new("A5", PaperFamily::IsoA, 1480, 2100),
    PaperSize::new("A6", PaperFamily::IsoA, 1050, 1480),
    PaperSize::new("A7", PaperFamily::IsoA, 740, 1050),
    PaperSize::new("A8", PaperFamily::IsoA, 520, 740),
    PaperSize::new("A9", PaperFamily::IsoA, 370, 520),
    PaperSize::new("A10", PaperFamily::IsoA, 260, 370),
    PaperSize::new("B0", PaperFamily::IsoB, 10000, 14140),
    PaperSize::new("B1", PaperFamily::IsoB, 7070, 10000),
    PaperSize::new("B2", PaperFamily::IsoB, 5000, 7070),
    PaperSize::new("B3", PaperFamily::IsoB, 3530, 5000),
    PaperSize::new("B4", PaperFamily::IsoB, 2500, 3530),
    PaperSize::new("B5", PaperFamily::IsoB, 1760, 2500),
    PaperSize::new("B6", PaperFamily::IsoB, 1250, 1760),
    PaperSize::new("B7", PaperFamily::IsoB, 880, 1250),
    PaperSize::new("B8", PaperFamily::IsoB, 620, 880),
    PaperSize::new("B9", PaperFamily::IsoB, 440, 620),
    PaperSize::new("B10", PaperFamily::IsoB, 310, 440),
    PaperSize::new("C0", PaperFamily::IsoC, 9170, 12970),
    PaperSize::new("C1", PaperFamily::IsoC, 6480, 9170),
    PaperSize::new("C2", PaperFamily::IsoC, 4580, 6480),
    PaperSize::new("C3", PaperFamily::IsoC, 3240, 4580),
    PaperSize::new("C4", PaperFamily::IsoC, 2290, 3240),
    PaperSize::new("C5", PaperFamily::IsoC, 1620, 2290),
    PaperSize::new("C6", PaperFamily::IsoC, 1140, 1620),
    PaperSize::new("Letter", PaperFamily::Us, 2159, 2794),
    PaperSize::new("Legal", PaperFamily::Us, 2159, 3556),
    PaperSize::new("Ledger", PaperFamily::Us, 2794, 4318),
    PaperSize::new("Executive", PaperFamily::Us, 1842, 2667),
    PaperSize::new("Statement", PaperFamily::Us, 1397, 2159),
    PaperSize::new("ANSI-C", PaperFamily::Ansi, 4318, 5588),
    PaperSize::new("ANSI-D", PaperFamily::Ansi, 5588, 8636),
    PaperSize::new("ANSI-E", PaperFamily::Ansi, 8636, 11176),
    PaperSize::new("Envelope-DL", PaperFamily::Envelope, 1100, 2200),
    PaperSize::new("Envelope-10", PaperFamily::Envelope, 1048, 2413),
    PaperSize::new("Envelope-C65", PaperFamily::Envelope, 1140, 2290),
];

/// 输入尺寸对（0.1mm 整数，允许横放）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dimensions {
    pub width_dmm: u32,
    pub height_dmm: u32,
}

impl Dimensions {
    /// 直接构造（不做校验，校验在 `Registry::normalize` 入口统一做）。
    pub const fn new(width_dmm: u32, height_dmm: u32) -> Dimensions {
        Dimensions {
            width_dmm,
            height_dmm,
        }
    }

    /// 短边/长边口径（横放与竖放归一到同一比较基准）。
    pub fn ordered(&self) -> (u32, u32) {
        if self.width_dmm <= self.height_dmm {
            (self.width_dmm, self.height_dmm)
        } else {
            (self.height_dmm, self.width_dmm)
        }
    }
}

/// 使用朝向。只描述「用户把标准纸横过来放」，不是第二种纸型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    None,
    Cw90,
}

impl Rotation {
    /// 从内外码解析（0=正放，1=横放），其余拒绝。
    pub fn from_code(v: u8) -> Result<Rotation, PaperError> {
        match v {
            0 => Ok(Rotation::None),
            1 => Ok(Rotation::Cw90),
            _ => Err(PaperError::BadOrientation),
        }
    }

    /// 对外码。
    pub fn code(self) -> u8 {
        match self {
            Rotation::None => 0,
            Rotation::Cw90 => 1,
        }
    }
}

/// 归一结果。`width_dmm/height_dmm` 是**真值表的登记尺寸**，不是输入回显；
/// `drift_dmm` 是短边差 + 长边差，零漂移即精确命中。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Normalized {
    pub code: &'static str,
    pub family: PaperFamily,
    pub width_dmm: u32,
    pub height_dmm: u32,
    pub rotation: Rotation,
    pub drift_dmm: u32,
}

impl Normalized {
    /// 是否精确命中（零漂移 + 未旋转以外无差）。
    pub fn is_exact(&self) -> bool {
        self.drift_dmm == 0
    }
}

/// 注册表容量：40 标准 + 24 扩展位。硬顶 64 让「表满」成为可测状态，
/// 而不是无限增长后把内存耗尽。
pub const REGISTRY_CAP: usize = 64;

/// 纸张注册表。标准表是默认内容，`insert` 只允许追加扩展纸（同码拒绝）。
#[derive(Clone, Debug)]
pub struct Registry {
    entries: Vec<PaperSize>,
}

impl Registry {
    /// 以 40 条标准表构造注册表。
    pub fn standard() -> Registry {
        let mut entries = Vec::with_capacity(REGISTRY_CAP);
        let mut i = 0usize;
        while i < PAPER_TABLE.len() {
            if let Some(p) = PAPER_TABLE.get(i) {
                entries.push(*p);
            }
            i += 1;
        }
        Registry { entries }
    }

    /// 当前条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空（空注册表的一律 `NoMatch`，不是隐含兜底）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 取第 idx 条（越界返回 None，供判据遍历）。
    pub fn at(&self, idx: usize) -> Option<PaperSize> {
        self.entries.get(idx).copied()
    }

    /// 按码精确查找（ASCII 大小写不敏感）。
    pub fn find_code(&self, code: &str) -> Option<PaperSize> {
        let canon = canon_token(code);
        if canon.is_empty() {
            return None;
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if canon_token(e.code) == canon {
                    return Some(*e);
                }
            }
            i += 1;
        }
        None
    }

    /// 追加一条扩展纸。同码（忽略大小写与分隔符）拒绝；满 64 条拒绝。
    pub fn insert(&mut self, size: PaperSize) -> Result<(), PaperError> {
        if self.entries.len() >= REGISTRY_CAP {
            return Err(PaperError::RegistryFull);
        }
        let canon = canon_token(size.code);
        if canon.is_empty() || size.width_dmm == 0 || size.height_dmm == 0 {
            return Err(PaperError::BadDimension);
        }
        if size.width_dmm > MAX_DMM || size.height_dmm > MAX_DMM {
            return Err(PaperError::TooLarge);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if canon_token(e.code) == canon {
                    return Err(PaperError::DuplicateCode);
                }
            }
            i += 1;
        }
        self.entries.push(size);
        Ok(())
    }

    /// 最近邻归一：短边/长边比较，漂移 = 两边绝对差之和；同漂移取先登记者。
    ///
    /// 为什么用整数：J1 要求「40 纸型零漂移」，浮点比较会把 215.9mm 与
    /// 8.5in 的表示差算成 1e-5 的漂移，判据就永远说不清。0.1mm 整数
    /// 让精确命中恒为 `drift == 0`。
    pub fn normalize(&self, dims: Dimensions, tolerance_dmm: u32) -> Result<Normalized, PaperError> {
        if dims.width_dmm == 0 || dims.height_dmm == 0 {
            return Err(PaperError::BadDimension);
        }
        if dims.width_dmm > MAX_DMM || dims.height_dmm > MAX_DMM {
            return Err(PaperError::TooLarge);
        }
        let (in_short, in_long) = dims.ordered();
        let mut best: Option<(u32, PaperSize)> = None;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                let (es, el) = if e.width_dmm <= e.height_dmm {
                    (e.width_dmm, e.height_dmm)
                } else {
                    (e.height_dmm, e.width_dmm)
                };
                let drift = absdiff(in_short, es) + absdiff(in_long, el);
                if drift <= tolerance_dmm {
                    let better = match best {
                        Some((bd, _)) => drift < bd,
                        None => true,
                    };
                    if better {
                        best = Some((drift, *e));
                    }
                }
            }
            i += 1;
        }
        match best {
            Some((drift, e)) => Ok(Normalized {
                code: e.code,
                family: e.family,
                width_dmm: e.width_dmm,
                height_dmm: e.height_dmm,
                rotation: if dims.width_dmm > dims.height_dmm {
                    Rotation::Cw90
                } else {
                    Rotation::None
                },
                drift_dmm: drift,
            }),
            None => Err(PaperError::NoMatch),
        }
    }

    /// 批量归一：任一失败即整批失败（不做「跳过坏样本」的静默降级）。
    pub fn normalize_many(
        &self,
        items: &[Dimensions],
        tolerance_dmm: u32,
    ) -> Result<Vec<Normalized>, PaperError> {
        let mut out = Vec::with_capacity(items.len());
        let mut i = 0usize;
        while i < items.len() {
            match items.get(i) {
                Some(d) => out.push(self.normalize(*d, tolerance_dmm)?),
                None => return Err(PaperError::BadDimension),
            }
            i += 1;
        }
        Ok(out)
    }

    /// 族内条目数（族码非法显式拒绝）。
    pub fn count_family(&self, family: PaperFamily) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.family == family {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 绝对差（u32 无符号，先比大小再减，杜绝下溢回绕）。
fn absdiff(a: u32, b: u32) -> u32 {
    if a >= b {
        a - b
    } else {
        b - a
    }
}

/// 规范化 token：只保留 ASCII 字母数字并统一大写。
/// 这样 `iso_a4`、`ISO-A4`、`iso a4` 都落到同一个比较串。
pub fn canon_token(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(c) = bytes.get(i) {
            let b = *c;
            if b.is_ascii_alphanumeric() {
                out.push(b.to_ascii_uppercase() as char);
            }
        }
        i += 1;
    }
    out
}

/// 单位闭集。只认 mm/in——pt、cm、px 一律显式拒绝，避免静默换算引入
/// 第二套精度语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Mm,
    In,
}

/// 解析十进制数字，返回「千分之一单位」的整数（不做浮点）。
/// 最多三位小数；超过三位一律坏数，不四舍五入——四舍五入会让
/// `210.04` 与 `210.0` 产生相同的归一结果，判据就抓不住精度回退。
fn parse_dec_milli(s: &str) -> Result<u64, PaperError> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(PaperError::BadDimension);
    }
    let mut int_part: u64 = 0;
    let mut frac: u64 = 0;
    let mut frac_digits: u32 = 0;
    let mut seen_dot = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(PaperError::BadDimension),
        };
        if b == b'.' {
            if seen_dot || i == 0 {
                return Err(PaperError::BadDimension);
            }
            seen_dot = true;
        } else if b.is_ascii_digit() {
            let d = (b - b'0') as u64;
            if seen_dot {
                if frac_digits >= 3 {
                    return Err(PaperError::BadDimension);
                }
                frac = frac * 10 + d;
                frac_digits += 1;
            } else {
                int_part = match int_part.checked_mul(10) {
                    Some(v) => v,
                    None => return Err(PaperError::TooLarge),
                };
                int_part = match int_part.checked_add(d) {
                    Some(v) => v,
                    None => return Err(PaperError::TooLarge),
                };
                // 一千万（毫米）已远超本模块职责，提前截断防溢出。
                if int_part > 10_000_000 {
                    return Err(PaperError::TooLarge);
                }
            }
        } else {
            return Err(PaperError::BadDimension);
        }
        i += 1;
    }
    if seen_dot && frac_digits == 0 {
        return Err(PaperError::BadDimension);
    }
    while frac_digits < 3 {
        frac *= 10;
        frac_digits += 1;
    }
    let milli = match int_part.checked_mul(1000) {
        Some(v) => v,
        None => return Err(PaperError::TooLarge),
    };
    match milli.checked_add(frac) {
        Some(v) => Ok(v),
        None => Err(PaperError::TooLarge),
    }
}

/// 千分之一单位 → 0.1mm。英寸按 1in = 254 × 0.1mm 精确换算。
fn to_dmm(milli: u64, unit: Unit) -> Result<u32, PaperError> {
    let dmm = match unit {
        Unit::Mm => milli / 100,
        Unit::In => {
            let scaled = match milli.checked_mul(254) {
                Some(v) => v,
                None => return Err(PaperError::TooLarge),
            };
            scaled / 1000
        }
    };
    if dmm == 0 {
        return Err(PaperError::BadDimension);
    }
    if dmm > MAX_DMM as u64 {
        return Err(PaperError::TooLarge);
    }
    Ok(dmm as u32)
}

/// 剥离尾部单位（mm/in，ASCII 大小写不敏感）。返回 (数字部分, 单位)；
/// 无单位时返回 None，由调用方决定默认值或拒绝。
fn strip_unit(s: &str) -> Option<(&str, Unit)> {
    let b = s.as_bytes();
    let n = b.len();
    if n < 2 {
        return None;
    }
    let c0 = *b.get(n - 2)?;
    let c1 = *b.get(n - 1)?;
    let is_m = c0 == b'm' || c0 == b'M';
    let is_m2 = c1 == b'm' || c1 == b'M';
    if is_m && is_m2 {
        // 末两字节是 ASCII 'm'，故 n-2 必在字符边界上。
        return Some((&s[..n - 2], Unit::Mm));
    }
    let is_i = c0 == b'i' || c0 == b'I';
    let is_n = c1 == b'n' || c1 == b'N';
    if is_i && is_n {
        return Some((&s[..n - 2], Unit::In));
    }
    None
}

/// 把「数值 + 单位」转成 0.1mm；无单位时用 `default`。
///
/// 为什么需要默认单位：`8.5x11in` 的单位只写在尾部，按排版惯例同时约束
/// 两个数。若左数永远默认毫米，8.5 会被读成 8.5mm——这是本模块首版实测
/// 过的真实缺陷（`85x2794`），因此单位归属必须在成对解析时统一决定。
fn dimension_with_default(s: &str, default: Unit) -> Result<u32, PaperError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(PaperError::BadDimension);
    }
    match strip_unit(trimmed) {
        Some((num, unit)) => {
            if num.is_empty() {
                return Err(PaperError::BadDimension);
            }
            let milli = parse_dec_milli(num)?;
            to_dmm(milli, unit)
        }
        None => {
            let last_alpha = match trimmed.as_bytes().last() {
                Some(c) => c.is_ascii_alphabetic(),
                None => false,
            };
            if last_alpha {
                return Err(PaperError::UnsupportedUnit);
            }
            let milli = parse_dec_milli(trimmed)?;
            to_dmm(milli, default)
        }
    }
}

/// 解析纸样文本。支持三类写法：
/// * 码：`A4` / `a4` / `iso_a4` / `na_letter`（族前缀剥离后比对）；
/// * 尺寸：`210x297` / `210x297mm` / `8.5x11in`（默认 mm）；
/// * 横放尺寸：`297x210`（归一结果带 `Cw90`）。
///
/// 失败按原因分码：空 → `EmptyName`，未知码 → `UnknownToken`，坏数 →
/// `BadDimension`，坏单位 → `UnsupportedUnit`，超限 → `TooLarge`。
pub fn parse_spec(reg: &Registry, text: &str) -> Result<Dimensions, PaperError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(PaperError::EmptyName);
    }
    // 先找尺寸分隔符 x/X。码名里不含 x（ANSI 用 '-'），所以找到 x 即尺寸式。
    let bytes = trimmed.as_bytes();
    let mut sep: Option<usize> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b == b'x' || *b == b'X' {
                if sep.is_some() {
                    return Err(PaperError::BadDimension);
                }
                sep = Some(i);
            }
        }
        i += 1;
    }
    if let Some(pos) = sep {
        let left = &trimmed[..pos];
        let right = &trimmed[pos + 1..];
        if left.is_empty() || right.is_empty() {
            return Err(PaperError::BadDimension);
        }
        // 右数先定单位：尾部 mm/in 显式声明的单位同时约束左数；
        // 右数无单位则整对默认毫米。
        let (rnum, runit) = match strip_unit(right) {
            Some((num, unit)) => {
                if num.is_empty() {
                    return Err(PaperError::BadDimension);
                }
                (num, unit)
            }
            None => {
                let last_alpha = match right.as_bytes().last() {
                    Some(c) => c.is_ascii_alphabetic(),
                    None => false,
                };
                if last_alpha {
                    return Err(PaperError::UnsupportedUnit);
                }
                (right, Unit::Mm)
            }
        };
        let w = dimension_with_default(left, runit)?;
        let h = to_dmm(parse_dec_milli(rnum)?, runit)?;
        return Ok(Dimensions::new(w, h));
    }
    let canon = canon_token(trimmed);
    if canon.is_empty() {
        return Err(PaperError::EmptyName);
    }
    // 精确码。
    let mut idx = 0usize;
    while idx < reg.len() {
        if let Some(e) = reg.at(idx) {
            if canon_token(e.code) == canon {
                return Ok(Dimensions::new(e.width_dmm, e.height_dmm));
            }
        }
        idx += 1;
    }
    // 族前缀：ISO / NA / US / JIS（只做一层剥离，不做递归猜码）。
    let prefixes = ["ISO", "NA", "US", "JIS"];
    let mut p = 0usize;
    while p < prefixes.len() {
        let pref = prefixes[p];
        if canon.len() > pref.len() && canon.starts_with(pref) {
            if let Some(rest) = canon.get(pref.len()..) {
                let mut k = 0usize;
                while k < reg.len() {
                    if let Some(e) = reg.at(k) {
                        if canon_token(e.code) == rest {
                            return Ok(Dimensions::new(e.width_dmm, e.height_dmm));
                        }
                    }
                    k += 1;
                }
            }
        }
        p += 1;
    }
    Err(PaperError::UnknownToken)
}

/// 0.1mm → 毫米文本（整数省掉 `.0`，非整数保留一位）。
fn fmt_mm(dmm: u32) -> String {
    let whole = dmm / 10;
    let frac = dmm % 10;
    if frac == 0 {
        format!("{}", whole)
    } else {
        format!("{}.{}", whole, frac)
    }
}

/// 尺寸 → 规范文本（`WxHmm`，按给定朝向）。与 `parse_spec` 构成往返。
pub fn to_spec(size: &PaperSize, rotation: Rotation) -> String {
    let (w, h) = match rotation {
        Rotation::None => (size.width_dmm, size.height_dmm),
        Rotation::Cw90 => (size.height_dmm, size.width_dmm),
    };
    let mut s = fmt_mm(w);
    s.push('x');
    s.push_str(&fmt_mm(h));
    s.push_str("mm");
    s
}

/// 全表零漂移审计：40 条 × 正放/横放两种朝向，逐条要求命中自身且 drift=0。
/// 返回 (核验次数, 不合格次数)。
pub fn audit_table_exact(reg: &Registry) -> (usize, usize) {
    let mut checked = 0usize;
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < reg.len() {
        if let Some(e) = reg.at(i) {
            let cases = [
                Dimensions::new(e.width_dmm, e.height_dmm),
                Dimensions::new(e.height_dmm, e.width_dmm),
            ];
            let mut k = 0usize;
            while k < cases.len() {
                checked += 1;
                match reg.normalize(cases[k], 0) {
                    Ok(n) => {
                        if n.code != e.code || n.drift_dmm != 0 {
                            bad += 1;
                        }
                        if k == 0 && n.rotation != Rotation::None {
                            bad += 1;
                        }
                        if k == 1 && n.rotation != Rotation::Cw90 {
                            bad += 1;
                        }
                    }
                    Err(_) => bad += 1,
                }
                k += 1;
            }
        }
        i += 1;
    }
    (checked, bad)
}

// ---------------------------------------------------------------------------
// 注入核：七类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 构造 7 类坏样本并断言诊断码。返回 (抓到, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;
    let mut reg = Registry::standard();

    // 1. 空文本：不是「默认 A4」，必须显式拒绝。
    if parse_spec(&reg, "   ").err() == Some(PaperError::EmptyName) {
        caught += 1;
    }
    // 2. 未知码：不得就近猜成 A4。
    if parse_spec(&reg, "A99").err() == Some(PaperError::UnknownToken) {
        caught += 1;
    }
    // 3. 坏数字：0 边是退化纸。
    if parse_spec(&reg, "0x297").err() == Some(PaperError::BadDimension) {
        caught += 1;
    }
    // 4. 坏单位：pt 不在闭集内。
    if parse_spec(&reg, "210x297pt").err() == Some(PaperError::UnsupportedUnit) {
        caught += 1;
    }
    // 5. 超上限：99999mm 远超 2000mm。
    if parse_spec(&reg, "99999x297").err() == Some(PaperError::TooLarge) {
        caught += 1;
    }
    // 6. 无匹配：5×5mm 与任何标准纸的漂移都超过 20。
    if reg
        .normalize(Dimensions::new(50, 50), 20)
        .err()
        == Some(PaperError::NoMatch)
    {
        caught += 1;
    }
    // 7. 重复登记：同码扩展纸必须被拒，而不是覆盖标准条目。
    let dup = PaperSize::new("A4", PaperFamily::IsoA, 2100, 2970);
    if reg.insert(dup).err() == Some(PaperError::DuplicateCode) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 判据用扩展码：24 条合法扩展纸，用于把注册表填到 64 条硬顶。
fn ext_codes() -> [&'static str; 24] {
    [
        "X1", "X2", "X3", "X4", "X5", "X6", "X7", "X8", "X9", "X10", "X11", "X12",
        "X13", "X14", "X15", "X16", "X17", "X18", "X19", "X20", "X21", "X22", "X23",
        "X24",
    ]
}

pub fn run_unx50067_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50067");
    let reg = Registry::standard();

    // J1-1 主判据：40 纸型 × 2 朝向，零漂移且码/朝向一致。
    let (checked, bad) = audit_table_exact(&reg);
    let ok = checked == 80 && bad == 0;
    let det = format!("checked={} bad={}", checked, bad);
    s.add_dyn("50067-J1-四十纸型零漂移", ok, &det);

    // J1-2 真值表规模与肖像登记纪律。
    let mut portrait_bad = 0usize;
    let mut i = 0usize;
    while i < PAPER_TABLE.len() {
        if let Some(e) = PAPER_TABLE.get(i) {
            if !e.is_portrait() || e.width_dmm == 0 || e.height_dmm == 0 {
                portrait_bad += 1;
            }
        }
        i += 1;
    }
    let ok = PAPER_TABLE.len() == 40 && portrait_bad == 0;
    let det = format!("n={} portrait_bad={}", PAPER_TABLE.len(), portrait_bad);
    s.add_dyn("50067-J1-表规模肖像", ok, &det);

    // J1-3 码唯一：40 条码规范化后两两不同（重复码会让最近邻决胜失序）。
    let mut dup_codes = 0usize;
    let mut a = 0usize;
    while a < PAPER_TABLE.len() {
        if let Some(ea) = PAPER_TABLE.get(a) {
            let ca = canon_token(ea.code);
            let mut b = a + 1;
            while b < PAPER_TABLE.len() {
                if let Some(eb) = PAPER_TABLE.get(b) {
                    if canon_token(eb.code) == ca {
                        dup_codes += 1;
                    }
                }
                b += 1;
            }
        }
        a += 1;
    }
    let ok = dup_codes == 0;
    s.add("50067-J1-码值唯一", ok, if ok { "" } else { "码名重复" });

    // J1-4 族覆盖：六族各自非空，且族计数之和 = 40。
    let mut fam_bad = 0usize;
    let mut fam_sum = 0usize;
    let mut f = 0usize;
    while f < PaperFamily::ALL.len() {
        if let Some(fam) = PaperFamily::ALL.get(f) {
            let n = reg.count_family(*fam);
            if n == 0 {
                fam_bad += 1;
            }
            fam_sum += n;
        }
        f += 1;
    }
    let ok = fam_bad == 0 && fam_sum == 40;
    let det = format!("empty_fam={} sum={}", fam_bad, fam_sum);
    s.add_dyn("50067-J1-族覆盖", ok, &det);

    // J1-5 ISO A 面积减半：A0..A10 相邻面积比应为 2（允许 2% 的取整误差）。
    let mut area_bad = 0usize;
    let mut a_idx = 0usize;
    while a_idx < 10 {
        if let (Some(e0), Some(e1)) = (PAPER_TABLE.get(a_idx), PAPER_TABLE.get(a_idx + 1)) {
            let cur = e0.area_dmm2() as i64;
            let next = e1.area_dmm2() as i64;
            let diff = (2 * next - cur).abs();
            // 2% 容差：ISO 216 的毫米取整在 A9/A10 上最大偏差约 0.4%。
            if diff * 50 > cur {
                area_bad += 1;
            }
        } else {
            area_bad += 1;
        }
        a_idx += 1;
    }
    let ok = area_bad == 0;
    let det = format!("area_bad={}", area_bad);
    s.add_dyn("50067-J1-A系面积减半", ok, &det);

    // J1-6 最近邻 + 漂移量：210.0→211.0mm 仍归 A4，漂移恰为 1.0mm（10 dmm）。
    let near = reg.normalize(Dimensions::new(2110, 2970), 20);
    let ok = match near {
        Ok(n) => n.code == "A4" && n.drift_dmm == 10 && n.rotation == Rotation::None,
        Err(_) => false,
    };
    let det = match near {
        Ok(n) => format!("code={} drift={}", n.code, n.drift_dmm),
        Err(_) => String::from("err"),
    };
    s.add_dyn("50067-J1-最近邻漂移", ok, &det);

    // J1-7 容差边界：同一输入容差 10 命中、9 拒绝（边界含等号）。
    let in_tol = reg.normalize(Dimensions::new(2110, 2970), 10).is_ok();
    let out_tol = reg
        .normalize(Dimensions::new(2110, 2970), 9)
        .err()
        == Some(PaperError::NoMatch);
    let ok = in_tol && out_tol;
    s.add("50067-J1-容差边界", ok, if ok { "" } else { "容差边界不闭合" });

    // J1-8 横放显式标注：297×210 命中 A4 且带 Cw90，尺寸不回显为 297×210。
    let land = reg.normalize(Dimensions::new(2970, 2100), 0);
    let ok = match land {
        Ok(n) => n.code == "A4" && n.rotation == Rotation::Cw90 && n.width_dmm == 2100,
        Err(_) => false,
    };
    let det = match land {
        Ok(n) => format!("code={} rot={}", n.code, n.rotation.code()),
        Err(_) => String::from("err"),
    };
    s.add_dyn("50067-J1-横放标注", ok, &det);

    // J1-9 解析往返：40 条真值表 → 文本 → 解析，尺寸逐位一致。
    let mut rt_bad = 0usize;
    let mut k = 0usize;
    while k < reg.len() {
        if let Some(e) = reg.at(k) {
            let text = to_spec(&e, Rotation::None);
            match parse_spec(&reg, &text) {
                Ok(d) => {
                    if d.width_dmm != e.width_dmm || d.height_dmm != e.height_dmm {
                        rt_bad += 1;
                    }
                }
                Err(_) => rt_bad += 1,
            }
            let ltext = to_spec(&e, Rotation::Cw90);
            match parse_spec(&reg, &ltext) {
                Ok(d) => {
                    if d.width_dmm != e.height_dmm || d.height_dmm != e.width_dmm {
                        rt_bad += 1;
                    }
                }
                Err(_) => rt_bad += 1,
            }
        }
        k += 1;
    }
    let ok = rt_bad == 0;
    let det = format!("rt_bad={}", rt_bad);
    s.add_dyn("50067-J1-文本往返", ok, &det);

    // J1-10 三类文本写法等价：A4 / iso_a4 / 210x297mm 全部落到 2100×2970。
    let by_code = parse_spec(&reg, "A4");
    let by_alias = parse_spec(&reg, "iso_a4");
    let by_dim = parse_spec(&reg, "210x297mm");
    let ok = by_code == Ok(Dimensions::new(2100, 2970))
        && by_alias == Ok(Dimensions::new(2100, 2970))
        && by_dim == Ok(Dimensions::new(2100, 2970));
    s.add("50067-J1-三写法等价", ok, if ok { "" } else { "别名解析分叉" });

    // J1-11 英寸精确换算：8.5×11in = 2159×2794 dmm（1in = 254 dmm）。
    let inch = parse_spec(&reg, "8.5x11in");
    let ok = inch == Ok(Dimensions::new(2159, 2794));
    let det = match inch {
        Ok(d) => format!("{}x{}", d.width_dmm, d.height_dmm),
        Err(_) => String::from("err"),
    };
    s.add_dyn("50067-J1-英寸换算", ok, &det);

    // J1-12 确定性：同一输入两次归一逐字段一致。
    let n1 = reg.normalize(Dimensions::new(2975, 4205), 10);
    let n2 = reg.normalize(Dimensions::new(2975, 4205), 10);
    let ok = n1 == n2 && matches!(n1, Ok(_));
    s.add("50067-J1-归一确定", ok, if ok { "" } else { "两次归一不一致" });

    // J1-13 批量归一：40 条全过，长度守恒；空批返回空表（不是错误）。
    let mut dims = Vec::with_capacity(40);
    let mut q = 0usize;
    while q < PAPER_TABLE.len() {
        if let Some(e) = PAPER_TABLE.get(q) {
            dims.push(Dimensions::new(e.width_dmm, e.height_dmm));
        }
        q += 1;
    }
    let many = reg.normalize_many(&dims, 0);
    let empty = reg.normalize_many(&[], 10);
    let ok = match (&many, &empty) {
        (Ok(v), Ok(e)) => v.len() == 40 && e.is_empty(),
        _ => false,
    };
    let det = match many {
        Ok(v) => format!("n={}", v.len()),
        Err(_) => String::from("err"),
    };
    s.add_dyn("50067-J1-批量守恒", ok, &det);

    // J1-14 注册表硬顶：40+24 填满 64，第 65 条 RegistryFull；重复码单独分码。
    let mut top = Registry::standard();
    let codes = ext_codes();
    let mut filled = 0usize;
    let mut t = 0usize;
    while t < codes.len() {
        let c = codes[t];
        let p = PaperSize::new(c, PaperFamily::Us, 1000 + (t as u32) * 10, 1500 + (t as u32) * 10);
        if top.insert(p).is_ok() {
            filled += 1;
        }
        t += 1;
    }
    let overflow = top.insert(PaperSize::new("X99", PaperFamily::Us, 1200, 1600));
    let ok = filled == 24
        && top.len() == 64
        && overflow.err() == Some(PaperError::RegistryFull);
    let det = format!("filled={} len={}", filled, top.len());
    s.add_dyn("50067-J1-表满拒绝", ok, &det);

    // J1-15 输入错误分码：空/未知/坏数/坏单位/超限 五条各命中自身。
    let ok = parse_spec(&reg, "").err() == Some(PaperError::EmptyName)
        && parse_spec(&reg, "ZZ").err() == Some(PaperError::UnknownToken)
        && parse_spec(&reg, "210x").err() == Some(PaperError::BadDimension)
        && parse_spec(&reg, "210x297cm").err() == Some(PaperError::UnsupportedUnit)
        && parse_spec(&reg, "21000x297").err() == Some(PaperError::TooLarge);
    s.add("50067-J1-输入分码", ok, if ok { "" } else { "输入错误混码" });

    // J1-16 码/朝向闭集：族码 0..=5、朝向码 0..=1，越界显式拒绝。
    let mut closed = true;
    let mut fc = 0u8;
    while fc < 6 {
        if PaperFamily::from_code(fc).err().is_some() {
            closed = false;
        }
        fc += 1;
    }
    let ok = closed
        && PaperFamily::from_code(6).err() == Some(PaperError::BadFamily)
        && Rotation::from_code(0).is_ok()
        && Rotation::from_code(1).is_ok()
        && Rotation::from_code(2).err() == Some(PaperError::BadOrientation);
    s.add("50067-J1-闭集解析", ok, if ok { "" } else { "闭集越界未拒" });

    // J1-17 错误码闭集：10 条，码值 = SEG_BASE + 1..=10 且两两不同。
    let mut codes_ok = ALL_ERRORS.len() == 10;
    let mut marks = [false; 11];
    let mut e = 0usize;
    while e < ALL_ERRORS.len() {
        if let Some(err) = ALL_ERRORS.get(e) {
            if err.code() != SEG_BASE + (e as u32) + 1 {
                codes_ok = false;
            }
            let off = (err.code() - SEG_BASE) as usize;
            match marks.get_mut(off) {
                Some(m) => {
                    if *m {
                        codes_ok = false;
                    }
                    *m = true;
                }
                None => codes_ok = false,
            }
        }
        e += 1;
    }
    s.add("50067-J1-错误码闭集", codes_ok, if codes_ok { "" } else { "码值漂移" });

    // J1-18 注入核 7 类全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50067-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50067_checks_green() {
        let set = run_unx50067_checks();
        assert!(set.all_passed(), "UNX-F50067 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50067_exact_roundtrip() {
        let reg = Registry::standard();
        let mut i = 0usize;
        while i < PAPER_TABLE.len() {
            if let Some(e) = PAPER_TABLE.get(i) {
                let text = to_spec(e, Rotation::None);
                let d = parse_spec(&reg, &text);
                assert_eq!(d, Ok(Dimensions::new(e.width_dmm, e.height_dmm)));
            }
            i += 1;
        }
    }

    #[test]
    fn unx50067_nearest_is_deterministic() {
        let reg = Registry::standard();
        let a = reg.normalize(Dimensions::new(2160, 2795), 30);
        let b = reg.normalize(Dimensions::new(2160, 2795), 30);
        assert_eq!(a, b);
    }
}
