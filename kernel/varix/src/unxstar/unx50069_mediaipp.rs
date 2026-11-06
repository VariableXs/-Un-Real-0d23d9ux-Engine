//! UNX-F50069 纸张与 IPP media 求交联签（B17 求交器单源）（0x434c0 段）
//!
//! 锚点：UNX-F50069-J1 联签登记，B17 条目零改写；判据锚定 §AI-63 §1 防重声明。
//!
//! 数据段：`SEG_BASE = 0x434c0`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 13`，
//! 逐项见 [`MediaError`]。不得借位相邻卡（0x434a0 自定义纸卡 / 0x434e0
//! 双面页序卡）。
//!
//! 「求交器单源」的含义：作业请求的 `media` 名与设备支持的 media 表求交，
//! 结果只能由本模块的 [`intersect`] 产生一次；B17 的条目是这次求交的
//! **只读快照**（键 + 摘要）。联签只引用快照摘要，绝不回写快照：
//! * 解析：IPP media 名（`iso_a4_210x297mm` / `na_letter_8.5x11in`）按
//!   最后一个含 `x` 的段解析；单位必须显式（mm/in），不猜；
//! * 求交：尺寸对按短边/长边归一，漂移 = 两边绝对差之和；容差内全部入选，
//!   按 (漂移, 设备 ID) 升序稳定排列——排序键保证同输入逐位可复算；
//! * 联签：`B17Table` 登记 (键, 摘要)；同键同摘要幂等、同键异摘要或冻结后
//!   写入一律 `EntryFrozen`；`SignLedger` 对同一键只签一次；
//! * 注入核：6 类坏样本（空名、无尺寸段、零边、坏单位、超上限、空设备）
//!   逐类断言诊断码。
//!
//! 本模块自包含：不引用 B17 的源文件，只用「键 + 摘要」把单源事实钉在
//! 联签记录里，因此 B17 条目是否被改写可以直接用摘要相等来判。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x434c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x434c0;

/// 单边上限 2000mm，与标准纸卡同一口径。
pub const MAX_DMM: u32 = 20000;
/// 设备 media 表硬顶。
pub const DEVICE_CAP: usize = 24;
/// B17 快照表硬顶。
pub const B17_CAP: usize = 16;
/// 联签账本硬顶。
pub const SIGN_CAP: usize = 16;

/// 联签盐：把「摘要」升级为「摘要 + 盐」的封签，防止摘要被重算附会。
const B17_SALT: u64 = 0xB17A_11CE_0000_0069;

const E_EMPTY: u16 = 1;
const E_BAD_TOKEN: u16 = 2;
const E_BAD_DIM: u16 = 3;
const E_UNIT: u16 = 4;
const E_TOO_LARGE: u16 = 5;
const E_NO_MATCH: u16 = 6;
const E_TABLE_FULL: u16 = 7;
const E_DUPLICATE: u16 = 8;
const E_FROZEN: u16 = 9;
const E_UNKNOWN_ENTRY: u16 = 10;
const E_DUPLICATE_SIGN: u16 = 11;
const E_SIGN: u16 = 12;
const E_LEDGER_FULL: u16 = 13;

/// IPP 求交/联签错误十三型。`code()` 恒为 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaError {
    /// media 名为空。
    EmptyMedia,
    /// 名称里没有可解析的 `WxH` 段。
    BadToken,
    /// 尺寸为 0 或数字格式非法。
    BadDimension,
    /// 单位不是 mm/in。
    UnsupportedUnit,
    /// 单边超过 2000mm。
    TooLarge,
    /// 设备表里没有容差内的 media。
    NoMatch,
    /// 设备表或 B17 表已满。
    TableFull,
    /// media 名重复登记。
    DuplicateMedia,
    /// B17 条目已冻结，拒绝改写。
    EntryFrozen,
    /// B17 条目不存在。
    UnknownEntry,
    /// 同一键重复联签。
    DuplicateSign,
    /// 摘要/封签不符。
    SignMismatch,
    /// 联签账本已满。
    LedgerFull,
}

impl MediaError {
    /// 段内序号（1 起），与 `code()` 一一对应。
    fn ordinal(self) -> u16 {
        match self {
            MediaError::EmptyMedia => E_EMPTY,
            MediaError::BadToken => E_BAD_TOKEN,
            MediaError::BadDimension => E_BAD_DIM,
            MediaError::UnsupportedUnit => E_UNIT,
            MediaError::TooLarge => E_TOO_LARGE,
            MediaError::NoMatch => E_NO_MATCH,
            MediaError::TableFull => E_TABLE_FULL,
            MediaError::DuplicateMedia => E_DUPLICATE,
            MediaError::EntryFrozen => E_FROZEN,
            MediaError::UnknownEntry => E_UNKNOWN_ENTRY,
            MediaError::DuplicateSign => E_DUPLICATE_SIGN,
            MediaError::SignMismatch => E_SIGN,
            MediaError::LedgerFull => E_LEDGER_FULL,
        }
    }

    /// 诊断码：段基址 + 序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            MediaError::EmptyMedia => "ipp media name empty",
            MediaError::BadToken => "no WxH token in media name",
            MediaError::BadDimension => "media dimension not positive",
            MediaError::UnsupportedUnit => "media unit is neither mm nor in",
            MediaError::TooLarge => "media dimension above 2000mm",
            MediaError::NoMatch => "no device media within tolerance",
            MediaError::TableFull => "device/b17 table at capacity",
            MediaError::DuplicateMedia => "device media name already present",
            MediaError::EntryFrozen => "b17 entry frozen by joint sign",
            MediaError::UnknownEntry => "b17 entry not registered",
            MediaError::DuplicateSign => "joint sign already recorded",
            MediaError::SignMismatch => "entry digest / seal mismatch",
            MediaError::LedgerFull => "joint sign ledger full",
        }
    }
}

/// 全错误表：判据验证码值闭集与「基址 + 序号」零漂移。
pub const ALL_ERRORS: [MediaError; 13] = [
    MediaError::EmptyMedia,
    MediaError::BadToken,
    MediaError::BadDimension,
    MediaError::UnsupportedUnit,
    MediaError::TooLarge,
    MediaError::NoMatch,
    MediaError::TableFull,
    MediaError::DuplicateMedia,
    MediaError::EntryFrozen,
    MediaError::UnknownEntry,
    MediaError::DuplicateSign,
    MediaError::SignMismatch,
    MediaError::LedgerFull,
];

/// 尺寸对（0.1mm 整数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaDims {
    pub width_dmm: u32,
    pub height_dmm: u32,
}

impl MediaDims {
    /// 常量构造。
    pub const fn new(width_dmm: u32, height_dmm: u32) -> MediaDims {
        MediaDims {
            width_dmm,
            height_dmm,
        }
    }

    /// 短边/长边口径。
    pub fn ordered(&self) -> (u32, u32) {
        if self.width_dmm <= self.height_dmm {
            (self.width_dmm, self.height_dmm)
        } else {
            (self.height_dmm, self.width_dmm)
        }
    }
}

/// 解析后的 IPP media。`name` 保留原始写法（联签键取它的字节），
/// `dims` 是解析出的尺寸——名称与尺寸必须同时留存，缺一不可复算。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IppMedia {
    pub name: String,
    pub dims: MediaDims,
}

/// 设备 media 一条（ID 由表分配）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaOffer {
    pub id: u16,
    pub media: IppMedia,
}

/// 单位闭集：IPP 的 media 尺寸后缀只认 mm/in。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    Mm,
    In,
}

/// 十进制 → 千分之一单位（整数解析，不做浮点）。
fn parse_milli(s: &str) -> Result<u64, MediaError> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(MediaError::BadDimension);
    }
    let mut int_part: u64 = 0;
    let mut frac: u64 = 0;
    let mut frac_digits: u32 = 0;
    let mut dot = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(MediaError::BadDimension),
        };
        if b == b'.' {
            if dot || i == 0 {
                return Err(MediaError::BadDimension);
            }
            dot = true;
        } else if b.is_ascii_digit() {
            let d = (b - b'0') as u64;
            if dot {
                if frac_digits >= 3 {
                    return Err(MediaError::BadDimension);
                }
                frac = frac * 10 + d;
                frac_digits += 1;
            } else {
                int_part = match int_part.checked_mul(10) {
                    Some(v) => v,
                    None => return Err(MediaError::TooLarge),
                };
                int_part = match int_part.checked_add(d) {
                    Some(v) => v,
                    None => return Err(MediaError::TooLarge),
                };
                if int_part > 10_000_000 {
                    return Err(MediaError::TooLarge);
                }
            }
        } else {
            return Err(MediaError::BadDimension);
        }
        i += 1;
    }
    if dot && frac_digits == 0 {
        return Err(MediaError::BadDimension);
    }
    while frac_digits < 3 {
        frac *= 10;
        frac_digits += 1;
    }
    let milli = match int_part.checked_mul(1000) {
        Some(v) => v,
        None => return Err(MediaError::TooLarge),
    };
    match milli.checked_add(frac) {
        Some(v) => Ok(v),
        None => Err(MediaError::TooLarge),
    }
}

/// 千分之一单位 → 0.1mm。英寸按 1in = 254 × 0.1mm 精确换算。
fn to_dmm(milli: u64, unit: Unit) -> Result<u32, MediaError> {
    let dmm = match unit {
        Unit::Mm => milli / 100,
        Unit::In => {
            let scaled = match milli.checked_mul(254) {
                Some(v) => v,
                None => return Err(MediaError::TooLarge),
            };
            scaled / 1000
        }
    };
    if dmm == 0 {
        return Err(MediaError::BadDimension);
    }
    if dmm > MAX_DMM as u64 {
        return Err(MediaError::TooLarge);
    }
    Ok(dmm as u32)
}

/// 剥离尾部单位（ASCII 大小写不敏感）。
fn strip_unit(s: &str) -> Option<(&str, Unit)> {
    let b = s.as_bytes();
    let n = b.len();
    if n < 2 {
        return None;
    }
    let c0 = *b.get(n - 2)?;
    let c1 = *b.get(n - 1)?;
    if (c0 == b'm' || c0 == b'M') && (c1 == b'm' || c1 == b'M') {
        return Some((&s[..n - 2], Unit::Mm));
    }
    if (c0 == b'i' || c0 == b'I') && (c1 == b'n' || c1 == b'N') {
        return Some((&s[..n - 2], Unit::In));
    }
    None
}

/// 单段数值（可带单位，缺省用 `default`）。
fn side_value(s: &str, default: Unit) -> Result<u32, MediaError> {
    let t = s.trim();
    if t.is_empty() {
        return Err(MediaError::BadDimension);
    }
    match strip_unit(t) {
        Some((num, unit)) => {
            if num.is_empty() {
                return Err(MediaError::BadDimension);
            }
            to_dmm(parse_milli(num)?, unit)
        }
        None => {
            let last_alpha = match t.as_bytes().last() {
                Some(c) => c.is_ascii_alphabetic(),
                None => false,
            };
            if last_alpha {
                return Err(MediaError::UnsupportedUnit);
            }
            to_dmm(parse_milli(t)?, default)
        }
    }
}

/// 解析 `WxH<unit>` 尺寸段（单位必须显式，IPP 惯例）。
fn parse_size_token(token: &str) -> Result<MediaDims, MediaError> {
    let bytes = token.as_bytes();
    let mut sep: Option<usize> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b == b'x' || *b == b'X' {
                if sep.is_some() {
                    return Err(MediaError::BadDimension);
                }
                sep = Some(i);
            }
        }
        i += 1;
    }
    let pos = match sep {
        Some(p) => p,
        None => return Err(MediaError::BadToken),
    };
    let left = &token[..pos];
    let right = &token[pos + 1..];
    if left.is_empty() || right.is_empty() {
        return Err(MediaError::BadDimension);
    }
    let (rnum, runit) = match strip_unit(right) {
        Some((num, unit)) => {
            if num.is_empty() {
                return Err(MediaError::BadDimension);
            }
            (num, unit)
        }
        None => return Err(MediaError::UnsupportedUnit),
    };
    let w = side_value(left, runit)?;
    let h = to_dmm(parse_milli(rnum)?, runit)?;
    Ok(MediaDims::new(w, h))
}

/// 解析 IPP media 名。取**最后一个**含 `x` 的下划线段作为尺寸段：
/// IPP 名称的尺寸段恒在尾部（`iso_a4_210x297mm`），前缀段可能含 `A4`
/// 这类伪 `x` 风险极低，但「取最后」让语义与 IPP 规范一致。
pub fn parse_ipp_media(text: &str) -> Result<IppMedia, MediaError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(MediaError::EmptyMedia);
    }
    let mut candidate: Option<&str> = None;
    let mut parts = trimmed.split('_');
    while let Some(part) = parts.next() {
        if part.is_empty() {
            continue;
        }
        let bytes = part.as_bytes();
        let mut i = 0usize;
        let mut has_x = false;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                if *b == b'x' || *b == b'X' {
                    has_x = true;
                }
            }
            i += 1;
        }
        if has_x {
            candidate = Some(part);
        }
    }
    let token = match candidate {
        Some(t) => t,
        None => return Err(MediaError::BadToken),
    };
    let dims = parse_size_token(token)?;
    Ok(IppMedia {
        name: String::from(trimmed),
        dims,
    })
}

/// 设备 media 表。ID 单调分配且不回收。
#[derive(Clone, Debug)]
pub struct DeviceTable {
    offers: Vec<MediaOffer>,
    next_id: u16,
}

impl DeviceTable {
    /// 空表，首个 ID = 1。
    pub fn new() -> DeviceTable {
        DeviceTable {
            offers: Vec::with_capacity(DEVICE_CAP),
            next_id: 1,
        }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.offers.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.offers.is_empty()
    }

    /// 按下标取条目。
    pub fn at(&self, idx: usize) -> Option<&MediaOffer> {
        self.offers.get(idx)
    }

    /// 按 ID 取条目。
    pub fn get(&self, id: u16) -> Option<&MediaOffer> {
        let mut i = 0usize;
        while i < self.offers.len() {
            if let Some(o) = self.offers.get(i) {
                if o.id == id {
                    return Some(o);
                }
            }
            i += 1;
        }
        None
    }

    /// 追加一条设备 media：名称重复显式拒绝（同尺寸不同名允许，求交按漂移排序）。
    pub fn add(&mut self, name: &str) -> Result<u16, MediaError> {
        if self.offers.len() >= DEVICE_CAP {
            return Err(MediaError::TableFull);
        }
        let parsed = parse_ipp_media(name)?;
        let mut i = 0usize;
        while i < self.offers.len() {
            if let Some(o) = self.offers.get(i) {
                if o.media.name.eq_ignore_ascii_case(&parsed.name) {
                    return Err(MediaError::DuplicateMedia);
                }
            }
            i += 1;
        }
        if self.next_id == 0 || self.next_id == u16::MAX {
            return Err(MediaError::TableFull);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.offers.push(MediaOffer {
            id,
            media: parsed,
        });
        Ok(id)
    }
}

/// 求交命中的一条：设备 ID、漂移、请求是否相对设备肖像方向旋转 90°。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chosen {
    pub id: u16,
    pub drift_dmm: u32,
    pub rotated: bool,
}

/// 一次求交的完整结果（B17 快照的输入）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intersection {
    pub req_key: u32,
    pub chosen: Vec<Chosen>,
    pub best_drift: u32,
}

/// 绝对差（先比大小，杜绝无符号下溢）。
fn absdiff(a: u32, b: u32) -> u32 {
    if a >= b {
        a - b
    } else {
        b - a
    }
}

/// 请求键：名称字节 + 尺寸的 FNV 低 32 位。
fn request_key(m: &IppMedia) -> u32 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let bytes = m.name.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = mix(h, *b as u64);
        }
        i += 1;
    }
    h = mix(h, m.dims.width_dmm as u64);
    h = mix(h, m.dims.height_dmm as u64);
    (h & 0xffff_ffff) as u32
}

/// 稳定排序：按 (漂移, ID) 升序；插入排序在 ≤24 条上比通用排序更可控，
/// 且不依赖 `sort_by` 的比较器是否稳定——顺序本身就是判据的一部分。
fn sort_chosen(v: &mut Vec<Chosen>) {
    let mut i = 1usize;
    while i < v.len() {
        let mut j = i;
        while j > 0 {
            let a = match v.get(j - 1) {
                Some(x) => *x,
                None => break,
            };
            let b = match v.get(j) {
                Some(x) => *x,
                None => break,
            };
            let a_first = a.drift_dmm < b.drift_dmm
                || (a.drift_dmm == b.drift_dmm && a.id <= b.id);
            if a_first {
                break;
            }
            v.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }
}

/// 求交主入口：容差内全部入选并按 (漂移, ID) 排序；空集 `NoMatch`。
///
/// 「全部入选」而不是「只取最近」是刻意的：B17 的条目要能回答
/// 「这台设备还有哪些纸能承接该请求」，只留最近一条会丢掉次优候选，
/// 后续降级/换纸就没有依据。
pub fn intersect(req: &IppMedia, table: &DeviceTable, tolerance_dmm: u32) -> Result<Intersection, MediaError> {
    if table.is_empty() {
        return Err(MediaError::NoMatch);
    }
    let (rs, rl) = req.dims.ordered();
    let mut chosen: Vec<Chosen> = Vec::with_capacity(table.len());
    let mut i = 0usize;
    while i < table.len() {
        if let Some(o) = table.at(i) {
            let (ds, dl) = o.media.dims.ordered();
            let drift = absdiff(rs, ds) + absdiff(rl, dl);
            if drift <= tolerance_dmm {
                chosen.push(Chosen {
                    id: o.id,
                    drift_dmm: drift,
                    rotated: req.dims.width_dmm > req.dims.height_dmm,
                });
            }
        }
        i += 1;
    }
    if chosen.is_empty() {
        return Err(MediaError::NoMatch);
    }
    sort_chosen(&mut chosen);
    let best = match chosen.first() {
        Some(c) => c.drift_dmm,
        None => return Err(MediaError::NoMatch),
    };
    Ok(Intersection {
        req_key: request_key(req),
        chosen,
        best_drift: best,
    })
}

/// FNV-1a 混合原语。
fn mix(h: u64, v: u64) -> u64 {
    let x = h ^ v;
    x.wrapping_mul(0x100_0000_01b3)
}

/// 求交结果摘要（B17 条目的内容）。只覆盖请求键与入选集，
/// 不覆盖设备表其余条目——这样「同一请求重算」的摘要必须相同。
pub fn intersection_digest(inter: &Intersection) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = mix(h, inter.req_key as u64);
    h = mix(h, inter.chosen.len() as u64);
    let mut i = 0usize;
    while i < inter.chosen.len() {
        if let Some(c) = inter.chosen.get(i) {
            h = mix(h, c.id as u64);
            h = mix(h, c.drift_dmm as u64);
            h = mix(h, if c.rotated { 1 } else { 0 });
        }
        i += 1;
    }
    h
}

/// B17 只读快照表。条目是求交的登记事实，联签只能引用、不能改写。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct B17Entry {
    pub key: u32,
    pub digest: u64,
}

/// B17 表（登记 + 冻结）。冻结后 `register/amend` 一律 `EntryFrozen`。
#[derive(Clone, Debug)]
pub struct B17Table {
    entries: Vec<B17Entry>,
    frozen: bool,
}

impl B17Table {
    /// 空表。
    pub fn new() -> B17Table {
        B17Table {
            entries: Vec::with_capacity(B17_CAP),
            frozen: false,
        }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 按键取摘要。
    pub fn get(&self, key: u32) -> Option<u64> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == key {
                    return Some(e.digest);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记 (键, 摘要)。同键同摘要幂等；同键异摘要说明求交单源被破坏，
    /// 必须 `EntryFrozen` 而不是覆盖——这正是「B17 条目零改写」的执行点。
    pub fn register(&mut self, key: u32, digest: u64) -> Result<u64, MediaError> {
        if self.frozen {
            return Err(MediaError::EntryFrozen);
        }
        if let Some(existing) = self.get(key) {
            if existing == digest {
                return Ok(existing);
            }
            return Err(MediaError::EntryFrozen);
        }
        if self.entries.len() >= B17_CAP {
            return Err(MediaError::TableFull);
        }
        self.entries.push(B17Entry { key, digest });
        Ok(digest)
    }

    /// 显式改写条目（仅供冻结前的迁移路径；冻结后拒绝）。
    pub fn amend(&mut self, key: u32, digest: u64) -> Result<(), MediaError> {
        if self.frozen {
            return Err(MediaError::EntryFrozen);
        }
        let mut pos: Option<usize> = None;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == key {
                    pos = Some(i);
                    break;
                }
            }
            i += 1;
        }
        match pos {
            Some(p) => match self.entries.get_mut(p) {
                Some(e) => {
                    e.digest = digest;
                    Ok(())
                }
                None => Err(MediaError::UnknownEntry),
            },
            None => Err(MediaError::UnknownEntry),
        }
    }

    /// 冻结后再无写入路径（联签完成后必须调用）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 表级内容摘要（条目键与摘要），冻结状态不计入。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = mix(h, self.entries.len() as u64);
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = mix(h, e.key as u64);
                h = mix(h, e.digest);
            }
            i += 1;
        }
        h
    }
}

/// 一条联签记录：对 B17 条目摘要的签名，不复制条目本体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignRecord {
    pub idx: u16,
    pub key: u32,
    pub entry_digest: u64,
    pub signer: u32,
    pub seal: u64,
}

/// 联签账本。
#[derive(Clone, Debug)]
pub struct SignLedger {
    records: Vec<SignRecord>,
}

impl SignLedger {
    /// 空账本。
    pub fn new() -> SignLedger {
        SignLedger {
            records: Vec::with_capacity(SIGN_CAP),
        }
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 按下标取记录。
    pub fn at(&self, idx: usize) -> Option<SignRecord> {
        self.records.get(idx).copied()
    }

    /// 同一 B17 键只签一次：重复联签返回 `DuplicateSign`，防止同一事实
    /// 出现两份可能互相矛盾的签署记录。
    pub fn sign(&mut self, key: u32, entry_digest: u64, signer: u32) -> Result<SignRecord, MediaError> {
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.key == key {
                    return Err(MediaError::DuplicateSign);
                }
            }
            i += 1;
        }
        if self.records.len() >= SIGN_CAP {
            return Err(MediaError::LedgerFull);
        }
        let seal = mix(
            entry_digest ^ (signer as u64),
            B17_SALT ^ (key as u64),
        );
        let rec = SignRecord {
            idx: self.records.len() as u16,
            key,
            entry_digest,
            signer,
            seal,
        };
        self.records.push(rec);
        Ok(rec)
    }
}

/// 复核联签：B17 条目必须存在、摘要必须相同、封签必须可复算。
pub fn verify_sign(rec: &SignRecord, b17: &B17Table) -> Result<(), MediaError> {
    let digest = match b17.get(rec.key) {
        Some(d) => d,
        None => return Err(MediaError::UnknownEntry),
    };
    if digest != rec.entry_digest {
        return Err(MediaError::SignMismatch);
    }
    let want = mix(
        rec.entry_digest ^ (rec.signer as u64),
        B17_SALT ^ (rec.key as u64),
    );
    if want != rec.seal {
        return Err(MediaError::SignMismatch);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 注入核：六类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 构造 6 类坏样本并断言诊断码。返回 (抓到, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    // 1. 空名。
    if parse_ipp_media("  ").err() == Some(MediaError::EmptyMedia) {
        caught += 1;
    }
    // 2. 无尺寸段：iso_a4 只有纸名，没有 WxH。
    if parse_ipp_media("iso_a4").err() == Some(MediaError::BadToken) {
        caught += 1;
    }
    // 3. 零边：0mm 不是可用 media。
    if parse_ipp_media("custom_0x150mm").err() == Some(MediaError::BadDimension) {
        caught += 1;
    }
    // 4. 坏单位：pt 不在闭集。
    if parse_ipp_media("custom_100x150pt").err() == Some(MediaError::UnsupportedUnit) {
        caught += 1;
    }
    // 5. 超上限：9999mm > 2000mm。
    if parse_ipp_media("custom_9999x100mm").err() == Some(MediaError::TooLarge) {
        caught += 1;
    }
    // 6. 空设备表求交：不得凭空返回一个候选。
    let empty = DeviceTable::new();
    if let Ok(req) = parse_ipp_media("iso_a4_210x297mm") {
        if intersect(&req, &empty, 20).err() == Some(MediaError::NoMatch) {
            caught += 1;
        }
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 审计设备表：5 条常见纸。
fn audit_device() -> DeviceTable {
    let mut t = DeviceTable::new();
    let names = [
        "iso_a4_210x297mm",
        "iso_a5_148x210mm",
        "na_letter_8.5x11in",
        "custom_100x150mm",
        "iso_a3_297x420mm",
    ];
    let mut i = 0usize;
    while i < names.len() {
        if let Some(n) = names.get(i) {
            let _ = t.add(n);
        }
        i += 1;
    }
    t
}

/// 审计请求：12 条，含精确、横放、英寸、20dmm 边界漂移。
fn audit_requests() -> [&'static str; 12] {
    [
        "iso_a4_210x297mm",
        "iso_a4_297x210mm",
        "iso_a5_148x210mm",
        "na_letter_8.5x11in",
        "custom_100x150mm",
        "iso_a3_297x420mm",
        "custom_210x298mm",
        "iso_a5_210x148mm",
        "custom_149x211mm",
        "custom_209x296mm",
        "ISO_A4_210x297MM",
        "custom_101x151mm",
    ]
}

/// J1 主审计：12 条请求逐条求交 → 登记 B17 → 联签；随后复核
/// (登录数, 总数, B17 条目数, 表摘要是否全程不变, 冻结后改写是否被拒)。
pub fn audit_joint_sign() -> (usize, usize, usize, bool, bool) {
    let device = audit_device();
    let requests = audit_requests();
    let mut b17 = B17Table::new();
    let mut ledger = SignLedger::new();
    let mut signed = 0usize;
    let mut total = 0usize;

    // 先登记全部条目并冻结，再联签——顺序对应真实流程：
    // B17 求交结果先落快照，联签只是对快照的引用。
    let mut digests: Vec<(u32, u64)> = Vec::with_capacity(requests.len());
    let mut i = 0usize;
    while i < requests.len() {
        total += 1;
        if let Some(name) = requests.get(i) {
            if let Ok(req) = parse_ipp_media(name) {
                if let Ok(inter) = intersect(&req, &device, 20) {
                    let d = intersection_digest(&inter);
                    if b17.register(inter.req_key, d).is_ok() {
                        digests.push((inter.req_key, d));
                    }
                }
            }
        }
        i += 1;
    }
    let before = b17.digest();
    b17.freeze();
    let b17_len = b17.len();

    let mut k = 0usize;
    while k < digests.len() {
        if let Some((key, d)) = digests.get(k) {
            if ledger.sign(*key, *d, 69).is_ok() {
                signed += 1;
            }
        }
        k += 1;
    }
    let stable = b17.digest() == before;
    // 冻结后改写必须被拒；同时尝试登记新键也必须被拒。
    let frozen_denied = match digests.first() {
        Some((key, d)) => {
            b17.amend(*key, *d ^ 0x1).err() == Some(MediaError::EntryFrozen)
                && b17.register(0xdead_beef, 0x1).err() == Some(MediaError::EntryFrozen)
        }
        None => false,
    };
    (signed, total, b17_len, stable, frozen_denied)
}

pub fn run_unx50069_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50069");

    // J1-1 主判据：12 条联签全部登记，B17 摘要全程不变，冻结后改写被拒。
    let (signed, total, b17_len, stable, frozen_denied) = audit_joint_sign();
    let ok = signed == 12 && total == 12 && b17_len == 12 && stable && frozen_denied;
    let det = format!(
        "signed={}/{} entries={} stable={} frozen={}",
        signed, total, b17_len, stable, frozen_denied
    );
    s.add_dyn("50069-J1-联签零改写", ok, &det);

    // J1-2 解析：5 条常见 media 的尺寸逐位正确。
    let cases = [
        ("iso_a4_210x297mm", 2100u32, 2970u32),
        ("iso_a5_148x210mm", 1480, 2100),
        ("na_letter_8.5x11in", 2159, 2794),
        ("custom_100x150mm", 1000, 1500),
        ("iso_a3_297x420mm", 2970, 4200),
    ];
    let mut parse_bad = 0usize;
    let mut i = 0usize;
    while i < cases.len() {
        if let Some((name, w, h)) = cases.get(i) {
            match parse_ipp_media(name) {
                Ok(m) => {
                    if m.dims.width_dmm != *w || m.dims.height_dmm != *h {
                        parse_bad += 1;
                    }
                }
                Err(_) => parse_bad += 1,
            }
        }
        i += 1;
    }
    let ok = parse_bad == 0;
    let det = format!("parse_bad={}", parse_bad);
    s.add_dyn("50069-J1-media解析", ok, &det);

    // J1-3 横放：297×210 命中 A4 且 rotated=true，设备尺寸仍是肖像口径。
    let device = audit_device();
    let land = parse_ipp_media("iso_a4_297x210mm");
    let ok = match land {
        Ok(req) => match intersect(&req, &device, 20) {
            Ok(inter) => match inter.chosen.first() {
                Some(c) => {
                    c.rotated
                        && c.drift_dmm == 0
                        && c.id == 1
                        && inter.best_drift == 0
                }
                None => false,
            },
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("50069-J1-横放求交", ok, if ok { "" } else { "横放未显式标注" });

    // J1-4 容差边界：211×297 对 A4 漂移 10，容差 10 命中、9 拒绝。
    let near = parse_ipp_media("custom_211x297mm");
    let ok = match near {
        Ok(req) => {
            intersect(&req, &device, 10).is_ok()
                && intersect(&req, &device, 9).err() == Some(MediaError::NoMatch)
        }
        Err(_) => false,
    };
    s.add("50069-J1-容差边界", ok, if ok { "" } else { "容差边界不闭合" });

    // J1-5 候选排序：(漂移, ID) 升序，最近者恒在首位。
    let mut pair = DeviceTable::new();
    let _ = pair.add("iso_a4_210x297mm");
    let _ = pair.add("custom_210x298mm");
    let ok = match parse_ipp_media("iso_a4_211x297mm") {
        Ok(req) => match intersect(&req, &pair, 20) {
            Ok(inter) => match (inter.chosen.first(), inter.chosen.get(1)) {
                (Some(a), Some(b)) => a.drift_dmm <= b.drift_dmm && a.id < b.id,
                _ => false,
            },
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("50069-J1-候选排序", ok, if ok { "" } else { "候选未按漂移排序" });

    // J1-6 设备名重复：同尺寸不同名允许，同名拒绝。
    let mut dup = DeviceTable::new();
    let ok = dup.add("iso_a4_210x297mm").is_ok()
        && dup.add("iso_a4_210x297mm").err() == Some(MediaError::DuplicateMedia)
        && dup.add("custom_210x297mm").is_ok()
        && dup.len() == 2;
    s.add("50069-J1-设备去重", ok, if ok { "" } else { "设备重名未拒" });

    // J1-7 设备表硬顶：24 条满，第 25 条 TableFull。
    let mut full = DeviceTable::new();
    let mut filled = 0usize;
    let mut k = 0usize;
    while k < DEVICE_CAP {
        let name = format!("custom_{}x{}mm", 100 + k, 150 + k);
        if full.add(&name).is_ok() {
            filled += 1;
        }
        k += 1;
    }
    let ok = filled == 24
        && full.len() == 24
        && full.add("custom_200x250mm").err() == Some(MediaError::TableFull);
    let det = format!("filled={} len={}", filled, full.len());
    s.add_dyn("50069-J1-设备表满", ok, &det);

    // J1-8 B17 登记语义：同键同摘要幂等；同键异摘要拒绝；未知键 amend 拒绝。
    let mut b17 = B17Table::new();
    let r1 = b17.register(7, 0xAA);
    let r2 = b17.register(7, 0xAA);
    let r3 = b17.register(7, 0xBB);
    let amend_unknown = b17.amend(8, 0xCC);
    let ok = r1 == Ok(0xAA)
        && r2 == Ok(0xAA)
        && r3.err() == Some(MediaError::EntryFrozen)
        && amend_unknown.err() == Some(MediaError::UnknownEntry)
        && b17.len() == 1;
    s.add("50069-J1-B17登记语义", ok, if ok { "" } else { "B17 登记语义漂移" });

    // J1-9 B17 冻结：冻结后 register/amend 全部 EntryFrozen，摘要不变。
    let mut fz = B17Table::new();
    let _ = fz.register(1, 0x11);
    let before = fz.digest();
    fz.freeze();
    let ok = fz.register(2, 0x22).err() == Some(MediaError::EntryFrozen)
        && fz.amend(1, 0x12).err() == Some(MediaError::EntryFrozen)
        && fz.digest() == before
        && fz.get(1) == Some(0x11);
    s.add("50069-J1-B17冻结", ok, if ok { "" } else { "冻结后仍可写" });

    // J1-10 联签：正常签名可复核；重复键拒绝；篡改摘要/未知键分码。
    let mut ledger = SignLedger::new();
    let mut refs = B17Table::new();
    let _ = refs.register(11, 0x1234);
    let rec = ledger.sign(11, 0x1234, 5);
    let ok = match rec {
        Ok(r) => {
            let mut bad = r;
            bad.entry_digest ^= 0x1;
            let mut bad_seal = r;
            bad_seal.seal ^= 0x2;
            let unknown = match ledger.sign(99, 0x1, 5) {
                Ok(x) => x,
                Err(_) => SignRecord {
                    idx: 0,
                    key: 99,
                    entry_digest: 1,
                    signer: 5,
                    seal: 0,
                },
            };
            verify_sign(&r, &refs).is_ok()
                && verify_sign(&bad, &refs).err() == Some(MediaError::SignMismatch)
                && verify_sign(&bad_seal, &refs).err() == Some(MediaError::SignMismatch)
                && ledger.sign(11, 0x1234, 5).err() == Some(MediaError::DuplicateSign)
                && verify_sign(&unknown, &refs).err() == Some(MediaError::UnknownEntry)
        }
        Err(_) => false,
    };
    s.add("50069-J1-联签复核", ok, if ok { "" } else { "联签复核失守" });

    // J1-11 联签账本硬顶：16 条满，第 17 条 LedgerFull。
    let mut lf = SignLedger::new();
    let mut n = 0usize;
    while n < SIGN_CAP {
        if lf.sign(100 + n as u32, 0x100 + n as u64, 1).is_ok() {
            n += 1;
        } else {
            break;
        }
    }
    let ok = lf.len() == 16 && lf.sign(999, 0x999, 1).err() == Some(MediaError::LedgerFull);
    let det = format!("len={}", lf.len());
    s.add_dyn("50069-J1-联签账满", ok, &det);

    // J1-12 摘要确定性与敏感性：同请求同摘要；尺寸变/设备变即变。
    let ok = match (
        parse_ipp_media("iso_a4_210x297mm"),
        parse_ipp_media("iso_a4_297x210mm"),
    ) {
        (Ok(a), Ok(b)) => {
            let da = match intersect(&a, &device, 20) {
                Ok(x) => Some(intersection_digest(&x)),
                Err(_) => None,
            };
            let da2 = match intersect(&a, &device, 20) {
                Ok(x) => Some(intersection_digest(&x)),
                Err(_) => None,
            };
            let db = match intersect(&b, &device, 20) {
                Ok(x) => Some(intersection_digest(&x)),
                Err(_) => None,
            };
            match (da, da2, db) {
                (Some(x), Some(y), Some(z)) => x == y && x != z,
                _ => false,
            }
        }
        _ => false,
    };
    s.add("50069-J1-摘要敏感", ok, if ok { "" } else { "摘要不随输入变化" });

    // J1-13 注入核 6 类全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50069-J1-注入必抓", ok, &det);

    // J1-14 错误码闭集：13 条，码值 = SEG_BASE + 1..=13 且两两不同。
    let mut codes_ok = ALL_ERRORS.len() == 13;
    let mut marks = [false; 14];
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
    s.add("50069-J1-错误码闭集", codes_ok, if codes_ok { "" } else { "码值漂移" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50069_checks_green() {
        let set = run_unx50069_checks();
        assert!(set.all_passed(), "UNX-F50069 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50069_ipp_roundtrip_dims() {
        let m = parse_ipp_media("na_letter_8.5x11in");
        assert_eq!(m.map(|x| x.dims), Ok(MediaDims::new(2159, 2794)));
        let a = parse_ipp_media("iso_a4_210x297mm");
        assert_eq!(a.map(|x| x.dims), Ok(MediaDims::new(2100, 2970)));
    }

    #[test]
    fn unx50069_b17_is_write_once() {
        let mut b = B17Table::new();
        assert_eq!(b.register(1, 10), Ok(10));
        assert_eq!(b.register(1, 10), Ok(10));
        assert_eq!(b.register(1, 11).err(), Some(MediaError::EntryFrozen));
        b.freeze();
        assert_eq!(b.register(2, 20).err(), Some(MediaError::EntryFrozen));
    }
}
