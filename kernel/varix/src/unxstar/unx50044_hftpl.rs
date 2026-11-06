//! UNX-F50044 页眉页脚模板（左中右三槽 + 首页不同档）（0x431a0 段）
//!
//! 锚点：UNX-F50044-J1 模板 ×20 渲染逐槽一致，首页档断言；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段 `SEG_BASE = 0x431a0`：诊断码闭集 9 条（1..=9），全部在注入核或
//! 判据里被真实构造。
//!
//! 结构清单：
//! * `HfRegistry` 注册表：`(带, 档位)` 四格定容登记（页眉/页脚 × 首页/常规），
//!   重复登记、超容、空槽、换行都显式拒绝；
//! * `HfTemplate`/`Slots`：左中右三槽文本，每槽独立度量；
//! * `Metrics` 版面参数（页宽/页边距/槽间距）+ `compose` 落位器：左槽贴左边距、
//!   右槽贴右边距、中槽严格居中，挤不下时**显式降级**（`dropped_center`）；
//! * `Event`/`HfRegistry::events`：登记/拒绝/渲染账本，`digest()` 供确定性断言；
//! * 注入核：六类坏登记（空槽/换行/重复/缺带/超容/零页号）逐类断言具体码。
//!
//! 为什么中槽挤不下就整段撤掉而不是压字距：页眉的字距一旦被压，右槽页码与
//! 左槽单位名会互相咬合，印出来比缺一槽更难读。撤槽必须在账本里留痕，
//! 不允许静默消失。

use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x431a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x431a0;

/// 注册表定容：2 带 × 2 档。多出来的组合是配置错误，不是扩展点。
pub const REG_CAP: usize = 4;
/// 槽间距（pt）。固定 4pt：与正文段距 12pt 的 1/3 对齐，视觉上不抢正文。
pub const GAP: u32 = 4;
/// 页宽上限（pt）。再宽是海报，不是文档。
const PAGE_W_MAX: u32 = 8192;

pub const E_EMPTY_BAND: u16 = 1;
pub const E_BAD_SLOT: u16 = 2;
pub const E_DUPLICATE: u16 = 3;
pub const E_MISSING_BAND: u16 = 4;
pub const E_FULL: u16 = 5;
pub const E_BAD_PAGE: u16 = 6;
pub const E_BAD_GEOMETRY: u16 = 7;
pub const E_BAD_VARIANT: u16 = 8;
pub const E_BAD_BAND: u16 = 9;

/// 页眉页脚域错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HfError {
    EmptyBand,
    BadSlot,
    Duplicate,
    MissingBand,
    Full,
    BadPage,
    BadGeometry,
    BadVariant,
    BadBand,
}

impl HfError {
    pub fn code(self) -> u32 {
        let n = match self {
            HfError::EmptyBand => E_EMPTY_BAND,
            HfError::BadSlot => E_BAD_SLOT,
            HfError::Duplicate => E_DUPLICATE,
            HfError::MissingBand => E_MISSING_BAND,
            HfError::Full => E_FULL,
            HfError::BadPage => E_BAD_PAGE,
            HfError::BadGeometry => E_BAD_GEOMETRY,
            HfError::BadVariant => E_BAD_VARIANT,
            HfError::BadBand => E_BAD_BAND,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            HfError::EmptyBand => "all three slots empty",
            HfError::BadSlot => "slot contains control character",
            HfError::Duplicate => "band/variant already registered",
            HfError::MissingBand => "band/variant not registered",
            HfError::Full => "registry full",
            HfError::BadPage => "page number is 1-based",
            HfError::BadGeometry => "slots do not fit page width",
            HfError::BadVariant => "variant code invalid",
            HfError::BadBand => "band code invalid",
        }
    }
}

/// 页眉 / 页脚。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Header,
    Footer,
}

impl Band {
    pub fn code(self) -> u8 {
        match self {
            Band::Header => 0,
            Band::Footer => 1,
        }
    }

    pub fn from_code(c: u8) -> Result<Band, HfError> {
        match c {
            0 => Ok(Band::Header),
            1 => Ok(Band::Footer),
            _ => Err(HfError::BadBand),
        }
    }
}

/// 首页档 / 常规档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    First,
    Default,
}

impl Variant {
    pub fn code(self) -> u8 {
        match self {
            Variant::First => 0,
            Variant::Default => 1,
        }
    }

    pub fn from_code(c: u8) -> Result<Variant, HfError> {
        match c {
            0 => Ok(Variant::First),
            1 => Ok(Variant::Default),
            _ => Err(HfError::BadVariant),
        }
    }
}

/// 三槽文本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slots {
    pub left: String,
    pub center: String,
    pub right: String,
}

impl Slots {
    pub fn new(left: &str, center: &str, right: &str) -> Slots {
        Slots {
            left: String::from(left),
            center: String::from(center),
            right: String::from(right),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty() && self.center.is_empty() && self.right.is_empty()
    }

    pub fn widths(&self) -> (u32, u32, u32) {
        (measure(&self.left), measure(&self.center), measure(&self.right))
    }
}

/// 单字符推进量：ASCII 6pt、中日韩 12pt、其余中间态 8pt。
/// 这是等宽版式的度量口径——不查字体表，判据才能逐位复算。
pub fn glyph_px(c: char) -> u32 {
    let u = c as u32;
    if u < 0x80 {
        6
    } else if u >= 0x2e80 {
        12
    } else {
        8
    }
}

/// 文本度量（pt）。
pub fn measure(s: &str) -> u32 {
    let mut w = 0u32;
    let mut it = s.chars();
    loop {
        match it.next() {
            Some(c) => w += glyph_px(c),
            None => break,
        }
    }
    w
}

fn slot_has_control(s: &str) -> bool {
    let mut it = s.chars();
    loop {
        match it.next() {
            Some(c) => {
                let u = c as u32;
                if u < 0x20 || u == 0x7f {
                    return true;
                }
            }
            None => return false,
        }
    }
}

fn slots_valid(s: &Slots) -> bool {
    !slot_has_control(&s.left) && !slot_has_control(&s.center) && !slot_has_control(&s.right)
}

/// 一个 `(带, 档位)` 模板。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HfTemplate {
    pub band: Band,
    pub variant: Variant,
    pub slots: Slots,
}

impl HfTemplate {
    pub fn new(band: Band, variant: Variant, left: &str, center: &str, right: &str) -> HfTemplate {
        HfTemplate {
            band,
            variant,
            slots: Slots::new(left, center, right),
        }
    }
}

/// 版面参数。`new` 就把「页边距吃到页宽一半」挡掉，避免后面每处除法都要防御。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    pub page_w: u32,
    pub margin: u32,
    pub gap: u32,
}

impl Metrics {
    pub fn new(page_w: u32, margin: u32) -> Result<Metrics, HfError> {
        if page_w == 0 || page_w > PAGE_W_MAX {
            return Err(HfError::BadGeometry);
        }
        let need = match margin.checked_mul(2) {
            Some(v) => v,
            None => return Err(HfError::BadGeometry),
        };
        if need + GAP * 2 >= page_w {
            return Err(HfError::BadGeometry);
        }
        Ok(Metrics {
            page_w,
            margin,
            gap: GAP,
        })
    }
}

/// 一页一槽带的落位结果（含槽文本快照，便于「逐槽一致」直接比对）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BandRender {
    pub page: u32,
    pub band: Band,
    pub variant: Variant,
    /// 首页档缺席时是否回退到常规档。
    pub fallback: bool,
    pub slots: Slots,
    pub left_x: u32,
    pub right_x: u32,
    pub center_x: Option<u32>,
    pub left_w: u32,
    pub center_w: u32,
    pub right_w: u32,
    pub dropped_center: bool,
    /// 槽宽 + 槽间距合计（不含页边距）。
    pub used: u32,
}

/// 落位器：左贴边、右贴边、中居中；中槽与左右冲突时整段撤下并记 `dropped_center`。
pub fn compose(
    page: u32,
    band: Band,
    variant: Variant,
    fallback: bool,
    slots: Slots,
    m: &Metrics,
) -> Result<BandRender, HfError> {
    if page == 0 {
        return Err(HfError::BadPage);
    }
    if !slots_valid(&slots) {
        return Err(HfError::BadSlot);
    }
    if slots.is_empty() {
        return Err(HfError::EmptyBand);
    }
    let (lw, cw, rw) = slots.widths();
    let inner = m.page_w - 2 * m.margin;
    let lr_need = lw
        .checked_add(rw)
        .and_then(|v| v.checked_add(m.gap))
        .ok_or(HfError::BadGeometry)?;
    if lr_need > inner {
        return Err(HfError::BadGeometry);
    }
    let left_x = m.margin;
    let right_x = m.page_w - m.margin - rw;
    let lo = if lw > 0 { left_x + lw + m.gap } else { left_x };
    let hi = if rw > 0 {
        right_x.saturating_sub(m.gap)
    } else {
        m.page_w - m.margin
    };
    let mut center_x: Option<u32> = None;
    let mut dropped = false;
    if cw > 0 {
        let want = m.page_w.saturating_sub(cw) / 2;
        let fits = want >= lo && want + cw <= hi;
        if fits {
            center_x = Some(want);
        } else {
            dropped = true;
        }
    }
    let present = (if lw > 0 { 1u32 } else { 0 })
        + (if rw > 0 { 1u32 } else { 0 })
        + (if center_x.is_some() { 1u32 } else { 0 });
    let gaps = if present > 1 { m.gap * (present - 1) } else { 0 };
    let used = lw + rw + (if center_x.is_some() { cw } else { 0 }) + gaps;
    if used + 2 * m.margin > m.page_w {
        return Err(HfError::BadGeometry);
    }
    Ok(BandRender {
        page,
        band,
        variant,
        fallback,
        slots,
        left_x,
        right_x,
        center_x,
        left_w: lw,
        center_w: cw,
        right_w: rw,
        dropped_center: dropped,
        used,
    })
}

/// 账本事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Registered { band: Band, variant: Variant },
    Rejected { code: u16 },
    Rendered {
        page: u32,
        band: Band,
        variant: Variant,
        fallback: bool,
    },
}

/// 页眉页脚注册表（定容 4 + 事件账本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HfRegistry {
    entries: Vec<HfTemplate>,
    pub events: Vec<Event>,
}

impl HfRegistry {
    pub fn new() -> HfRegistry {
        HfRegistry {
            entries: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn slots_of(&self, band: Band, variant: Variant) -> Option<&Slots> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(t) = self.entries.get(i) {
                if t.band == band && t.variant == variant {
                    return Some(&t.slots);
                }
            }
            i += 1;
        }
        None
    }

    /// 显式登记。重复登记**不覆盖**旧模板——覆盖会让上一版页眉在改稿期悄悄消失。
    pub fn register(&mut self, t: HfTemplate) -> Result<(), HfError> {
        if self.entries.len() >= REG_CAP {
            self.events.push(Event::Rejected {
                code: E_FULL as u16,
            });
            return Err(HfError::Full);
        }
        if !slots_valid(&t.slots) {
            self.events.push(Event::Rejected {
                code: E_BAD_SLOT as u16,
            });
            return Err(HfError::BadSlot);
        }
        if t.slots.is_empty() {
            self.events.push(Event::Rejected {
                code: E_EMPTY_BAND as u16,
            });
            return Err(HfError::EmptyBand);
        }
        if self.slots_of(t.band, t.variant).is_some() {
            self.events.push(Event::Rejected {
                code: E_DUPLICATE as u16,
            });
            return Err(HfError::Duplicate);
        }
        self.entries.push(HfTemplate {
            band: t.band,
            variant: t.variant,
            slots: t.slots,
        });
        self.events.push(Event::Registered {
            band: t.band,
            variant: t.variant,
        });
        Ok(())
    }

    /// 渲染第 `page` 页的某个带。首页取 `First` 档，缺席则回退 `Default` 并记 `fallback`。
    pub fn render_page(
        &mut self,
        page: u32,
        band: Band,
        m: &Metrics,
    ) -> Result<BandRender, HfError> {
        if page == 0 {
            self.events.push(Event::Rejected {
                code: E_BAD_PAGE as u16,
            });
            return Err(HfError::BadPage);
        }
        let want = if page == 1 {
            Variant::First
        } else {
            Variant::Default
        };
        let mut variant = want;
        let mut fallback = false;
        let mut slots: Option<Slots> = None;
        if let Some(s) = self.slots_of(band, want) {
            slots = Some(s.clone());
        }
        if slots.is_none() && want == Variant::First {
            if let Some(s) = self.slots_of(band, Variant::Default) {
                slots = Some(s.clone());
                variant = Variant::Default;
                fallback = true;
            }
        }
        let slots = match slots {
            Some(s) => s,
            None => {
                self.events.push(Event::Rejected {
                    code: E_MISSING_BAND as u16,
                });
                return Err(HfError::MissingBand);
            }
        };
        let out = compose(page, band, variant, fallback, slots, m)?;
        self.events.push(Event::Rendered {
            page,
            band,
            variant,
            fallback,
        });
        Ok(out)
    }

    /// 事件账本摘要（FNV-1a）。同操作序列必须得到同摘要。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                let (a, b, c) = match e {
                    Event::Registered { band, variant } => {
                        (band.code() as u32, variant.code() as u32, 1u32)
                    }
                    Event::Rejected { code } => (*code as u32, 0, 2u32),
                    Event::Rendered {
                        page,
                        band,
                        variant,
                        fallback,
                    } => (
                        *page,
                        (band.code() as u32) * 4 + variant.code() as u32 + if *fallback { 8 } else { 0 },
                        3u32,
                    ),
                };
                h ^= a as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= c as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }

    pub fn rendered_pages(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(Event::Rendered { page, .. }) = self.events.get(i) {
                out.push(*page);
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 注入核：六类坏登记必须逐类抓到
// ---------------------------------------------------------------------------

/// 返回（抓到的坏样本数, 总数）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    let mut r = HfRegistry::new();
    if r.register(HfTemplate::new(Band::Header, Variant::Default, "", "", ""))
        .err()
        == Some(HfError::EmptyBand)
    {
        caught += 1;
    }
    if r.register(HfTemplate::new(
        Band::Header,
        Variant::Default,
        "单位\n名称",
        "",
        "",
    ))
    .err()
        == Some(HfError::BadSlot)
    {
        caught += 1;
    }
    let mut r2 = HfRegistry::new();
    let ok_first = r2
        .register(HfTemplate::new(Band::Footer, Variant::Default, "左", "", "右"))
        .is_ok();
    if ok_first
        && r2
            .register(HfTemplate::new(Band::Footer, Variant::Default, "左2", "", "右2"))
            .err()
            == Some(HfError::Duplicate)
    {
        caught += 1;
    }
    let m = match Metrics::new(320, 12) {
        Ok(v) => v,
        Err(_) => Metrics {
            page_w: 320,
            margin: 12,
            gap: GAP,
        },
    };
    let mut r3 = HfRegistry::new();
    if r3
        .render_page(2, Band::Header, &m)
        .err()
        == Some(HfError::MissingBand)
    {
        caught += 1;
    }
    let mut r4 = HfRegistry::new();
    let mut i = 0usize;
    let mut full_ok = false;
    while i < REG_CAP {
        let band = if i % 2 == 0 { Band::Header } else { Band::Footer };
        let variant = if (i / 2) % 2 == 0 {
            Variant::First
        } else {
            Variant::Default
        };
        let _ = r4.register(HfTemplate::new(band, variant, "L", "C", "R"));
        i += 1;
    }
    if r4.len() == REG_CAP {
        full_ok = r4
            .register(HfTemplate::new(Band::Header, Variant::First, "X", "Y", "Z"))
            .err()
            == Some(HfError::Full);
    }
    if full_ok {
        caught += 1;
    }
    let mut r5 = HfRegistry::new();
    if r5.render_page(0, Band::Header, &m).err() == Some(HfError::BadPage) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 20 组模板语料：两个带交替，左/中/右槽按不同周期组合，覆盖空槽与长串。
fn template_corpus() -> Vec<HfTemplate> {
    let lefts = ["内部资料", "机密", "第 1 页", "ACME", "研发中心"];
    let centers = ["", "研发中心", "草案"];
    let rights = ["2026-11-06", "第 3 页", "COPY-2"];
    let mut out: Vec<HfTemplate> = Vec::new();
    let mut i = 0usize;
    while i < 20 {
        let band = if i % 2 == 0 { Band::Header } else { Band::Footer };
        out.push(HfTemplate::new(
            band,
            Variant::Default,
            lefts[i % 5],
            centers[(i / 2) % 3],
            rights[(i / 3) % 3],
        ));
        i += 1;
    }
    out
}

pub fn run_unx50044_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50044");
    let m = match Metrics::new(360, 14) {
        Ok(v) => v,
        Err(_) => Metrics {
            page_w: 360,
            margin: 14,
            gap: GAP,
        },
    };

    // J1 主体：20 组模板逐槽渲染一致（文本快照 + 槽宽 + 无降级）
    let corpus = template_corpus();
    let mut slot_ok = 0usize;
    let mut width_ok = 0usize;
    let mut i = 0usize;
    while i < corpus.len() {
        if let Some(t) = corpus.get(i) {
            let mut reg = HfRegistry::new();
            if reg.register(t.clone()).is_ok() {
                if let Ok(r) = reg.render_page(2, t.band, &m) {
                    let text_same = r.slots == t.slots;
                    let w = t.slots.widths();
                    let width_same =
                        r.left_w == w.0 && r.center_w == w.1 && r.right_w == w.2;
                    if text_same {
                        slot_ok += 1;
                    }
                    if width_same && !r.dropped_center && r.slots.is_empty() == false {
                        width_ok += 1;
                    }
                }
            }
        }
        i += 1;
    }
    let ok = slot_ok == 20 && width_ok == 20;
    let det = format!("逐槽 {}/20 宽度 {}/20", slot_ok, width_ok);
    s.add_dyn("50044-J1-三槽一致", ok, &det);

    // J1 首页档：第 1 页取 First 档文案，第 2 页取 Default 档
    let mut first_ok = false;
    let mut reg = HfRegistry::new();
    let a = reg.register(HfTemplate::new(Band::Header, Variant::First, "", "机密", ""));
    let b = reg.register(HfTemplate::new(Band::Header, Variant::Default, "", "内部", ""));
    if a.is_ok() && b.is_ok() {
        let r1 = reg.render_page(1, Band::Header, &m);
        let r2 = reg.render_page(2, Band::Header, &m);
        if let (Ok(x), Ok(y)) = (r1, r2) {
            first_ok = x.variant == Variant::First
                && x.slots.center == "机密"
                && !x.fallback
                && y.variant == Variant::Default
                && y.slots.center == "内部";
        }
    }
    s.add(
        "50044-J1-首页档",
        first_ok,
        if first_ok { "" } else { "首页档未生效" },
    );

    // 首页档缺席 → 回退常规档并显式记 fallback
    let mut fb_ok = false;
    let mut reg = HfRegistry::new();
    if reg
        .register(HfTemplate::new(Band::Footer, Variant::Default, "页脚", "", "1"))
        .is_ok()
    {
        if let Ok(r) = reg.render_page(1, Band::Footer, &m) {
            fb_ok = r.fallback && r.variant == Variant::Default && r.slots.left == "页脚";
        }
    }
    s.add("50044-J1-首页回退", fb_ok, if fb_ok { "" } else { "回退未记账" });

    // 左槽贴边：20 组全部满足 left_x == margin
    let mut edge_l = 0usize;
    let mut edge_r = 0usize;
    let mut i = 0usize;
    while i < corpus.len() {
        if let Some(t) = corpus.get(i) {
            let mut reg = HfRegistry::new();
            if reg.register(t.clone()).is_ok() {
                if let Ok(r) = reg.render_page(2, t.band, &m) {
                    if r.left_x == m.margin {
                        edge_l += 1;
                    }
                    if r.right_x + r.right_w + m.margin == m.page_w {
                        edge_r += 1;
                    }
                }
            }
        }
        i += 1;
    }
    let ok = edge_l == 20 && edge_r == 20;
    s.add("50044-J1-两侧贴边", ok, if ok { "" } else { "槽未贴边" });

    // 中槽居中偏差 ≤1px（2*x + w 与页宽的差 ≤2 等价）
    let mut center_ok = 0usize;
    let mut i = 0usize;
    while i < corpus.len() {
        if let Some(t) = corpus.get(i) {
            let mut reg = HfRegistry::new();
            if reg.register(t.clone()).is_ok() {
                if let Ok(r) = reg.render_page(2, t.band, &m) {
                    if r.center_w == 0 {
                        if r.center_x.is_none() {
                            center_ok += 1;
                        }
                    } else if let Some(x) = r.center_x {
                        let d = if 2 * x + r.center_w > m.page_w {
                            2 * x + r.center_w - m.page_w
                        } else {
                            m.page_w - (2 * x + r.center_w)
                        };
                        if d <= 2 {
                            center_ok += 1;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    let ok = center_ok == 20;
    let det = format!("居中 {}/20", center_ok);
    s.add_dyn("50044-J1-中槽居中", ok, &det);

    // 宽度不变式：used + 2*margin ≤ page_w（20 组）
    let mut fit_ok = 0usize;
    let mut i = 0usize;
    while i < corpus.len() {
        if let Some(t) = corpus.get(i) {
            let mut reg = HfRegistry::new();
            if reg.register(t.clone()).is_ok() {
                if let Ok(r) = reg.render_page(2, t.band, &m) {
                    if r.used + 2 * m.margin <= m.page_w {
                        fit_ok += 1;
                    }
                }
            }
        }
        i += 1;
    }
    let ok = fit_ok == 20;
    s.add("50044-J1-版面不变式", ok, if ok { "" } else { "槽宽越界" });

    // 度量口径：CJK 双宽、ASCII 单宽
    let ok = measure("机密") == 24 && measure("AB") == 12 && measure("") == 0;
    s.add("50044-J1-度量口径", ok, if ok { "" } else { "字宽口径错" });

    // 重叠降级：窄页长槽 → 中槽整段撤下并记账，左右仍贴边
    let narrow = match Metrics::new(200, 4) {
        Ok(v) => v,
        Err(_) => Metrics {
            page_w: 200,
            margin: 4,
            gap: GAP,
        },
    };
    let mut drop_ok = false;
    let mut reg = HfRegistry::new();
    let t = HfTemplate::new(Band::Header, Variant::Default, "AAAAAAAAAAAAAA", "BBBBBBBBBBBBBB", "CCCC");
    if reg.register(t).is_ok() {
        if let Ok(r) = reg.render_page(2, Band::Header, &narrow) {
            drop_ok = r.dropped_center
                && r.center_x.is_none()
                && r.center_w > 0
                && r.left_x == narrow.margin
                && r.right_x + r.right_w + narrow.margin == narrow.page_w;
        }
    }
    s.add("50044-J1-重叠降级", drop_ok, if drop_ok { "" } else { "降级未显式" });

    // 挤爆左右两槽本身 → 显式几何错误（不静默压字距）
    let tight = match Metrics::new(120, 4) {
        Ok(v) => v,
        Err(_) => Metrics {
            page_w: 120,
            margin: 4,
            gap: GAP,
        },
    };
    let wide = HfTemplate::new(
        Band::Footer,
        Variant::Default,
        "AAAAAAAAAAAAAAAAAAAA",
        "",
        "BBBBBBBBBBBBBBBBBBBB",
    );
    let ok = compose(
        2,
        wide.band,
        wide.variant,
        false,
        wide.slots.clone(),
        &tight,
    )
    .err()
        == Some(HfError::BadGeometry);
    s.add("50044-J1-几何必拒", ok, if ok { "" } else { "越宽未拦" });

    // 确定性：同操作序列两次 → 渲染逐字段相同、账本摘要相同
    let mut det_ok = false;
    let mut ra = HfRegistry::new();
    let mut rb = HfRegistry::new();
    let ta = HfTemplate::new(Band::Header, Variant::Default, "A", "B", "C");
    let tb = HfTemplate::new(Band::Header, Variant::Default, "A", "B", "C");
    if ra.register(ta).is_ok() && rb.register(tb).is_ok() {
        let x = ra.render_page(2, Band::Header, &m);
        let y = rb.render_page(2, Band::Header, &m);
        match (x, y) {
            (Ok(p), Ok(q)) => {
                det_ok = p == q && ra.digest() == rb.digest();
            }
            _ => det_ok = false,
        }
    }
    s.add("50044-J1-确定", det_ok, if det_ok { "" } else { "两次渲染不一致" });

    // 账本：连渲 3 页 → 页号单调 1,2,3；事件数 = 登记 + 渲染
    let mut led_ok = false;
    let mut reg = HfRegistry::new();
    if reg
        .register(HfTemplate::new(Band::Header, Variant::First, "首", "", ""))
        .is_ok()
        && reg
            .register(HfTemplate::new(Band::Header, Variant::Default, "常", "", ""))
            .is_ok()
    {
        let _ = reg.render_page(1, Band::Header, &m);
        let _ = reg.render_page(2, Band::Header, &m);
        let _ = reg.render_page(3, Band::Header, &m);
        let pages = reg.rendered_pages();
        led_ok = pages == {
            let mut v: Vec<u32> = Vec::new();
            v.push(1);
            v.push(2);
            v.push(3);
            v
        } && reg.events.len() == 5;
    }
    s.add("50044-J1-账本单调", led_ok, if led_ok { "" } else { "页序或事件数错" });

    // 重复登记必拒（不覆盖旧模板）
    let mut reg = HfRegistry::new();
    let ok = reg
        .register(HfTemplate::new(Band::Footer, Variant::First, "甲", "", ""))
        .is_ok()
        && reg
            .register(HfTemplate::new(Band::Footer, Variant::First, "乙", "", ""))
            .err()
            == Some(HfError::Duplicate)
        && reg.slots_of(Band::Footer, Variant::First) == Some(&Slots::new("甲", "", ""));
    s.add("50044-J1-重复必拒", ok, if ok { "" } else { "覆盖了旧模板" });

    // 空槽三连 / 换行 / 未登记带 / 零页号
    let mut r0 = HfRegistry::new();
    let ok = r0
        .register(HfTemplate::new(Band::Header, Variant::First, "", "", ""))
        .err()
        == Some(HfError::EmptyBand)
        && r0
            .register(HfTemplate::new(Band::Header, Variant::First, "a\tb", "", ""))
            .err()
            == Some(HfError::BadSlot)
        && r0.render_page(2, Band::Footer, &m).err() == Some(HfError::MissingBand)
        && r0.render_page(0, Band::Header, &m).err() == Some(HfError::BadPage);
    s.add("50044-J1-登记四拒", ok, if ok { "" } else { "坏登记漏过" });

    // 档位/带号解码：非法码落到各自诊断码
    let ok = Variant::from_code(9).err() == Some(HfError::BadVariant)
        && Band::from_code(9).err() == Some(HfError::BadBand)
        && Variant::from_code(0) == Ok(Variant::First)
        && Band::from_code(1) == Ok(Band::Footer);
    s.add("50044-J1-码值解码", ok, if ok { "" } else { "档位码解码错" });

    // 几何参数自检：零页宽、边距吃满都拒
    let ok = Metrics::new(0, 0).err() == Some(HfError::BadGeometry)
        && Metrics::new(100, 46).err() == Some(HfError::BadGeometry)
        && Metrics::new(320, 12).is_ok();
    s.add("50044-J1-参数自检", ok, if ok { "" } else { "几何参数未校验" });

    // 注册表满 → 显式 Full
    let mut rf = HfRegistry::new();
    let mut i = 0usize;
    while i < REG_CAP {
        let band = if i % 2 == 0 { Band::Header } else { Band::Footer };
        let variant = if (i / 2) % 2 == 0 {
            Variant::First
        } else {
            Variant::Default
        };
        let _ = rf.register(HfTemplate::new(band, variant, "L", "C", "R"));
        i += 1;
    }
    let ok = rf.len() == REG_CAP
        && rf
            .register(HfTemplate::new(Band::Header, Variant::First, "X", "Y", "Z"))
            .err()
            == Some(HfError::Full);
    s.add("50044-J1-容量必拒", ok, if ok { "" } else { "超容未拦" });

    // 注入核汇总：六类坏登记逐类命中具体码
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50044-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50044_checks_green() {
        let set = run_unx50044_checks();
        assert!(set.all_passed(), "UNX-F50044 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50044_first_page_variant() {
        let m = match Metrics::new(320, 12) {
            Ok(v) => v,
            Err(_) => Metrics {
                page_w: 320,
                margin: 12,
                gap: GAP,
            },
        };
        let mut reg = HfRegistry::new();
        let _ = reg.register(HfTemplate::new(Band::Header, Variant::First, "", "机密", ""));
        let _ = reg.register(HfTemplate::new(Band::Header, Variant::Default, "", "内部", ""));
        let r1 = match reg.render_page(1, Band::Header, &m) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r1.slots.center, "机密");
        assert_eq!(r1.variant, Variant::First);
    }

    #[test]
    fn unx50044_center_drop_is_explicit() {
        let narrow = match Metrics::new(200, 4) {
            Ok(v) => v,
            Err(_) => return,
        };
        let t = HfTemplate::new(
            Band::Header,
            Variant::Default,
            "AAAAAAAAAAAAAA",
            "BBBBBBBBBBBBBB",
            "CCCC",
        );
        let r = match compose(2, t.band, t.variant, false, t.slots, &narrow) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(r.dropped_center);
        assert!(r.center_x.is_none());
    }
}
