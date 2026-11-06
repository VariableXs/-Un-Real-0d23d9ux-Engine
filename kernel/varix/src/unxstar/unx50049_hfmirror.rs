//! UNX-F50049 页眉页脚与镜像页（小册子内侧边距镜像）（0x43240 段）
//!
//! 锚点：UNX-F50049-J1 镜像 ×10 位置逐页一致；判据锚定 §AI-63 §1 防重声明（B22 联签）。
//!
//! 小册子装订后，奇偶页的「内侧」在纸面上的物理位置相反：奇数页（recto）
//! 内侧在左，偶数页（verso）内侧在右。页眉页脚如果按「左边距固定」摆放，
//! 装订后偶数页的内容会贴上装订口。因此本模块把边距分成 inner/outer 两个
//! 语义量，再按页的奇偶与镜像策略解析成 left/right；槽位自身可声明
//! `mirror=true`，此时对齐方向在 verso 页翻转（Left↔Right，Center 天然中立）。
//!
//! 镜像的硬不变量（J1 判据核心）：同一槽位在相邻奇偶页上的水平位置必须满足
//! `x_odd + x_even + w == page_w`。这不是近似，而是「内外距互换」的代数结果：
//! 奇页 `x = inner`，偶页 `x = page_w - inner - w`。
//!
//! 另外本模块承担小册子拼版：padded 到 4 的倍数后按 (n, 1) (2, n-1) 配对，
//! 保证每页恰出现一次、空白补位显式落在 `None` 上（不伪造页号）。
//!
//! 数据段 `SEG_BASE = 0x43240`，诊断码闭集 `0x43241..=0x4324a`（10 条）。
//! 结构清单：`HfSlot`/`HfRegistry`（槽位注册表）、`PageMargins`/`MirrorPolicy`
//! （边距语义）、`PlacedBox`（落位）、`BookletSession`/`BindStage`（拼版状态机）、
//! `HfLedger`（防重台账）、`place_page`（几何求解）、`booklet_plan`（拼版）、
//! `injection_core`（10 类坏样本注入核）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x43240）。
pub const SEG_BASE: u32 = 0x43240;

pub const E_EMPTY: u16 = 1;
pub const E_SLOT_DUP: u16 = 2;
pub const E_SLOT_CAP: u16 = 3;
pub const E_ALIGN: u16 = 4;
pub const E_MARGIN: u16 = 5;
pub const E_MIRROR_UNSUP: u16 = 6;
pub const E_PAGE_ORDER: u16 = 7;
pub const E_BOUNDS: u16 = 8;
pub const E_IMPOSITION: u16 = 9;
pub const E_PARAM: u16 = 10;

/// 页眉页脚 / 拼版域错误。`code()` 落在 `SEG_BASE + n` 闭集内。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HfError {
    /// 槽位表为空或页数为 0
    Empty,
    /// 槽位 id 重复
    SlotDup,
    /// 槽位表容量满
    SlotCap,
    /// 对齐码非法
    Align,
    /// 边距为负
    MarginNeg,
    /// 槽位要求镜像但当前策略不支持
    MirrorUnsupported,
    /// 页号非法（本模块 1 基；0 页不存在）
    PageOrder,
    /// 槽位放不进内容区
    Bounds,
    /// 拼版超出张数上限
    Imposition,
    /// 尺寸/偏移参数非法
    Param,
}

impl HfError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            HfError::Empty => E_EMPTY,
            HfError::SlotDup => E_SLOT_DUP,
            HfError::SlotCap => E_SLOT_CAP,
            HfError::Align => E_ALIGN,
            HfError::MarginNeg => E_MARGIN,
            HfError::MirrorUnsupported => E_MIRROR_UNSUP,
            HfError::PageOrder => E_PAGE_ORDER,
            HfError::Bounds => E_BOUNDS,
            HfError::Imposition => E_IMPOSITION,
            HfError::Param => E_PARAM,
        };
        SEG_BASE + n as u32
    }

    /// 一句话诊断说明。
    pub fn message(self) -> &'static str {
        match self {
            HfError::Empty => "no slots registered",
            HfError::SlotDup => "slot id duplicated",
            HfError::SlotCap => "slot table full",
            HfError::Align => "alignment code invalid",
            HfError::MarginNeg => "negative margin",
            HfError::MirrorUnsupported => "mirror requested under none policy",
            HfError::PageOrder => "page number invalid",
            HfError::Bounds => "slot does not fit content box",
            HfError::Imposition => "booklet exceeds sheet budget",
            HfError::Param => "size or offset out of range",
        }
    }
}

/// 页眉 / 页脚。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Header,
    Footer,
}

/// 水平对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

impl HAlign {
    /// 对齐码闭集：0=左，1=中，2=右。
    pub fn from_u8(v: u8) -> Result<HAlign, HfError> {
        match v {
            0 => Ok(HAlign::Left),
            1 => Ok(HAlign::Center),
            2 => Ok(HAlign::Right),
            _ => Err(HfError::Align),
        }
    }

    /// 镜像翻转：左↔右，中不变。
    pub fn flip(self) -> HAlign {
        match self {
            HAlign::Left => HAlign::Right,
            HAlign::Right => HAlign::Left,
            HAlign::Center => HAlign::Center,
        }
    }
}

/// 边距语义：内外分开，不是「左右」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageMargins {
    pub inner: i32,
    pub outer: i32,
    pub top: i32,
    pub bottom: i32,
}

/// 镜像策略两档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MirrorPolicy {
    /// 不镜像：所有页沿用「左=inner」的固定版面
    None,
    /// 内外互换：verso 页 left=outer / right=inner
    InnerOuter,
}

impl MirrorPolicy {
    /// 策略码闭集：0=None，1=InnerOuter。
    pub fn from_u8(v: u8) -> Result<MirrorPolicy, HfError> {
        match v {
            0 => Ok(MirrorPolicy::None),
            1 => Ok(MirrorPolicy::InnerOuter),
            _ => Err(HfError::Param),
        }
    }

    /// 策略码。
    pub fn as_u8(self) -> u8 {
        match self {
            MirrorPolicy::None => 0,
            MirrorPolicy::InnerOuter => 1,
        }
    }
}

/// 一个页眉/页脚槽位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HfSlot {
    pub id: u8,
    pub kind: SlotKind,
    pub align: HAlign,
    pub width: i32,
    pub height: i32,
    pub offset: i32,
    pub mirror: bool,
}

impl HfSlot {
    /// 构造槽位并做参数自检：宽高必须为正、偏移非负。
    pub fn new(
        id: u8,
        kind: SlotKind,
        align: HAlign,
        width: i32,
        height: i32,
        offset: i32,
        mirror: bool,
    ) -> Result<HfSlot, HfError> {
        if width <= 0 || height <= 0 || offset < 0 {
            return Err(HfError::Param);
        }
        Ok(HfSlot {
            id,
            kind,
            align,
            width,
            height,
            offset,
            mirror,
        })
    }
}

/// 槽位表容量（页眉 4 + 页脚 4 的典型上限）。
pub const SLOT_CAP: usize = 8;

/// 页眉页脚槽位注册表：定容数组 + id 去重。
pub struct HfRegistry {
    slots: [Option<HfSlot>; SLOT_CAP],
    len: usize,
}

impl HfRegistry {
    /// 空注册表。
    pub const fn new() -> HfRegistry {
        HfRegistry {
            slots: [None; SLOT_CAP],
            len: 0,
        }
    }

    /// 注册槽位；id 重复 → `SlotDup`，容量满 → `SlotCap`。
    pub fn add(&mut self, slot: HfSlot) -> Result<(), HfError> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.slots.get(i).and_then(|v| *v) {
                if e.id == slot.id {
                    return Err(HfError::SlotDup);
                }
            }
            i += 1;
        }
        if self.len >= SLOT_CAP {
            return Err(HfError::SlotCap);
        }
        if let Some(dst) = self.slots.get_mut(self.len) {
            *dst = Some(slot);
            self.len += 1;
        }
        Ok(())
    }

    /// 已注册槽位数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 按下标取槽位（越界返回 None）。
    pub fn at(&self, i: usize) -> Option<HfSlot> {
        self.slots.get(i).and_then(|v| *v)
    }

    /// 按 id 查槽位。
    pub fn find(&self, id: u8) -> Option<HfSlot> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.slots.get(i).and_then(|v| *v) {
                if e.id == id {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }
}

/// 一页上某个槽位的落位矩形。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacedBox {
    pub slot: u8,
    pub page: u16,
    pub kind: SlotKind,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// 解析本页的左右边距：verso 且策略为 InnerOuter 时内外互换。
pub fn margins_lr(m: PageMargins, verso: bool, policy: MirrorPolicy) -> (i32, i32) {
    let swap = verso && policy == MirrorPolicy::InnerOuter;
    if swap {
        (m.outer, m.inner)
    } else {
        (m.inner, m.outer)
    }
}

/// 槽位的水平起点。镜像槽在 verso 页把对齐方向翻转（Center 天然中立）。
pub fn slot_x(
    slot: &HfSlot,
    page: u16,
    page_w: i32,
    m: PageMargins,
    policy: MirrorPolicy,
) -> Result<i32, HfError> {
    if page == 0 {
        return Err(HfError::PageOrder);
    }
    if page_w <= 0 || slot.width <= 0 {
        return Err(HfError::Param);
    }
    if m.inner < 0 || m.outer < 0 || m.top < 0 || m.bottom < 0 {
        return Err(HfError::MarginNeg);
    }
    if slot.mirror && policy == MirrorPolicy::None {
        return Err(HfError::MirrorUnsupported);
    }
    let verso = page % 2 == 0;
    let (left, right) = margins_lr(m, verso, policy);
    let cw = page_w - left - right;
    if cw < slot.width {
        return Err(HfError::Bounds);
    }
    let align = if slot.mirror && verso {
        slot.align.flip()
    } else {
        slot.align
    };
    let x = match align {
        HAlign::Left => left,
        HAlign::Center => left + (cw - slot.width) / 2,
        HAlign::Right => left + cw - slot.width,
    };
    Ok(x)
}

/// 槽位的垂直起点。页眉贴着上边距下沿，页脚自上边距反向计算。
pub fn slot_y(slot: &HfSlot, page_h: i32, m: PageMargins) -> Result<i32, HfError> {
    if page_h <= 0 || slot.height <= 0 || slot.offset < 0 {
        return Err(HfError::Param);
    }
    if m.inner < 0 || m.outer < 0 || m.top < 0 || m.bottom < 0 {
        return Err(HfError::MarginNeg);
    }
    match slot.kind {
        SlotKind::Header => {
            let y = m.top + slot.offset;
            if y + slot.height > page_h - m.bottom {
                return Err(HfError::Bounds);
            }
            Ok(y)
        }
        SlotKind::Footer => {
            let y = page_h - m.bottom - slot.offset - slot.height;
            if y < m.top {
                return Err(HfError::Bounds);
            }
            Ok(y)
        }
    }
}

/// 求解一页上全部槽位的落位；任一槽位越界即整页失败（不返回半页结果）。
pub fn place_page(
    reg: &HfRegistry,
    page: u16,
    page_w: i32,
    page_h: i32,
    m: PageMargins,
    policy: MirrorPolicy,
) -> Result<Vec<PlacedBox>, HfError> {
    if page == 0 {
        return Err(HfError::PageOrder);
    }
    if page_w <= 0 || page_h <= 0 {
        return Err(HfError::Param);
    }
    if reg.len() == 0 {
        return Err(HfError::Empty);
    }
    let mut out: Vec<PlacedBox> = Vec::with_capacity(reg.len());
    let mut i = 0usize;
    while i < reg.len() {
        let slot = match reg.at(i) {
            Some(v) => v,
            None => return Err(HfError::Empty),
        };
        let x = slot_x(&slot, page, page_w, m, policy)?;
        let y = slot_y(&slot, page_h, m)?;
        out.push(PlacedBox {
            slot: slot.id,
            page,
            kind: slot.kind,
            x,
            y,
            w: slot.width,
            h: slot.height,
        });
        i += 1;
    }
    Ok(out)
}

/// 落位指纹（确定性对平用）。
pub fn box_digest(boxes: &[PlacedBox]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < boxes.len() {
        if let Some(b) = boxes.get(i) {
            h = (h ^ b.slot as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ b.page as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ b.x as u32 as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ b.y as u32 as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
        i += 1;
    }
    h
}

/// 一页落位的摘要（日志用）。
pub fn page_summary(boxes: &[PlacedBox]) -> String {
    if boxes.is_empty() {
        return String::from("empty page");
    }
    match boxes.get(0) {
        Some(b) => format!("page={} boxes={}", b.page, boxes.len()),
        None => String::from("empty page"),
    }
}

/// 小册子拼版的一张纸（四面，`None` = 显式空白位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImposedSheet {
    pub sheet: u16,
    pub front_left: Option<u16>,
    pub front_right: Option<u16>,
    pub back_left: Option<u16>,
    pub back_right: Option<u16>,
}

fn opt_page(p: u16, n: u16) -> Option<u16> {
    if p <= n {
        Some(p)
    } else {
        None
    }
}

/// 小册子拼版：页数 padded 到 4 的倍数，第 k 张正面为 (padded-2k, 1+2k)、
/// 背面为 (2+2k, padded-1-2k)。超出真实页数的位置落 `None`。
pub fn booklet_plan(page_count: u16, max_sheets: u16) -> Result<Vec<ImposedSheet>, HfError> {
    if page_count == 0 {
        return Err(HfError::Empty);
    }
    if max_sheets == 0 {
        return Err(HfError::Imposition);
    }
    let padded = ((page_count + 3) / 4) * 4;
    let sheets = padded / 4;
    if sheets > max_sheets {
        return Err(HfError::Imposition);
    }
    let mut out: Vec<ImposedSheet> = Vec::with_capacity(sheets as usize);
    let mut k = 0u16;
    while k < sheets {
        let fl = padded - 2 * k;
        let fr = 1 + 2 * k;
        let bl = 2 + 2 * k;
        let br = padded - 1 - 2 * k;
        out.push(ImposedSheet {
            sheet: k,
            front_left: opt_page(fl, page_count),
            front_right: opt_page(fr, page_count),
            back_left: opt_page(bl, page_count),
            back_right: opt_page(br, page_count),
        });
        k += 1;
    }
    Ok(out)
}

/// 拼版阶段：只许前进或同阶段幂等。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindStage {
    Raw,
    Imposed,
    Paginated,
    Final,
}

impl BindStage {
    /// 阶段序。
    pub fn rank(self) -> u8 {
        match self {
            BindStage::Raw => 0,
            BindStage::Imposed => 1,
            BindStage::Paginated => 2,
            BindStage::Final => 3,
        }
    }

    /// 迁移：同阶段幂等，前进一格允许，倒退/跳级拒。
    pub fn advance(self, to: BindStage) -> Result<BindStage, HfError> {
        let from = self.rank();
        let t = to.rank();
        if t == from {
            return Ok(self);
        }
        if t == from + 1 {
            return Ok(to);
        }
        Err(HfError::PageOrder)
    }
}

/// 拼版会话：把「拼版 → 分页 → 定稿」串成单调状态机，避免调用方乱序。
pub struct BookletSession {
    pages: u16,
    sheets: u16,
    stage: BindStage,
}

impl BookletSession {
    /// 以页数建会话（Draft 的 Raw 阶段）。
    pub fn new(pages: u16) -> BookletSession {
        BookletSession {
            pages,
            sheets: 0,
            stage: BindStage::Raw,
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> BindStage {
        self.stage
    }

    /// 已排定的张数。
    pub fn sheets(&self) -> u16 {
        self.sheets
    }

    /// 执行拼版；必须在 Raw 阶段，成功推进到 Imposed。
    pub fn impose(&mut self, max_sheets: u16) -> Result<Vec<ImposedSheet>, HfError> {
        if self.stage != BindStage::Raw {
            return Err(HfError::PageOrder);
        }
        let plan = booklet_plan(self.pages, max_sheets)?;
        self.sheets = plan.len() as u16;
        self.stage = BindStage::Imposed;
        Ok(plan)
    }

    /// 分页完成；必须已拼版。
    pub fn paginate(&mut self) -> Result<(), HfError> {
        if self.stage.rank() < BindStage::Imposed.rank() {
            return Err(HfError::PageOrder);
        }
        let next = self.stage.advance(BindStage::Paginated)?;
        self.stage = next;
        Ok(())
    }

    /// 定稿；必须已分页。
    pub fn finalize(&mut self) -> Result<(), HfError> {
        if self.stage.rank() < BindStage::Paginated.rank() {
            return Err(HfError::PageOrder);
        }
        let next = self.stage.advance(BindStage::Final)?;
        self.stage = next;
        Ok(())
    }
}

/// 落位台账容量（24 条 = 12 页 × 2 槽）。
pub const LEDGER_CAP: usize = 24;

/// 落位防重台账：同 (槽, 页) 只许记一次。
pub struct HfLedger {
    entries: [Option<PlacedBox>; LEDGER_CAP],
    len: usize,
    rejected: u16,
}

impl HfLedger {
    /// 空台账。
    pub const fn new() -> HfLedger {
        HfLedger {
            entries: [None; LEDGER_CAP],
            len: 0,
            rejected: 0,
        }
    }

    /// 入账；重复 → `SlotDup`（同键重登），容量满 → `SlotCap`。
    pub fn record(&mut self, b: PlacedBox) -> Result<(), HfError> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.entries.get(i).and_then(|v| *v) {
                if e.slot == b.slot && e.page == b.page {
                    self.rejected += 1;
                    return Err(HfError::SlotDup);
                }
            }
            i += 1;
        }
        if self.len >= LEDGER_CAP {
            self.rejected += 1;
            return Err(HfError::SlotCap);
        }
        if let Some(slot) = self.entries.get_mut(self.len) {
            *slot = Some(b);
            self.len += 1;
        }
        Ok(())
    }

    /// 已入账条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 被拒条数。
    pub fn rejected(&self) -> u16 {
        self.rejected
    }
}

// ---------------------------------------------------------------------------
// J1：10 页镜像位置逐页一致
// ---------------------------------------------------------------------------

/// 标准测试版面：一张 A4（595×842pt），内外边距 25/15，页眉左、页脚右，
/// 两者都声明镜像。
pub struct MirrorFixture {
    pub reg: HfRegistry,
    pub margins: PageMargins,
    pub page_w: i32,
    pub page_h: i32,
}

/// 构造镜像夹具；失败返回 None（常量参数不该失败，失败即真 bug）。
pub fn mirror_fixture() -> Option<MirrorFixture> {
    let mut reg = HfRegistry::new();
    let header = match HfSlot::new(
        1,
        SlotKind::Header,
        HAlign::Left,
        120_000,
        20_000,
        5_000,
        true,
    ) {
        Ok(v) => v,
        Err(_) => return None,
    };
    let footer = match HfSlot::new(
        2,
        SlotKind::Footer,
        HAlign::Right,
        80_000,
        18_000,
        4_000,
        true,
    ) {
        Ok(v) => v,
        Err(_) => return None,
    };
    if reg.add(header).is_err() || reg.add(footer).is_err() {
        return None;
    }
    Some(MirrorFixture {
        reg,
        margins: PageMargins {
            inner: 25_000,
            outer: 15_000,
            top: 20_000,
            bottom: 18_000,
        },
        page_w: 595_000,
        page_h: 842_000,
    })
}

/// J1 矩阵：10 页逐页校验镜像位置，返回 (通过页数, 总页数)。
///
/// 每页要同时满足：① 页眉水平位置 = 奇页 inner / 偶页 page_w-inner-w；
/// ② 页脚对称相反；③ 垂直位置与页奇偶无关；④ 落位在内容区内。
pub fn j1_mirror_result() -> (usize, usize) {
    let fx = match mirror_fixture() {
        Some(v) => v,
        None => return (0, 10),
    };
    let mut ok = 0usize;
    let mut page = 1u16;
    while page <= 10 {
        let boxes = match place_page(
            &fx.reg,
            page,
            fx.page_w,
            fx.page_h,
            fx.margins,
            MirrorPolicy::InnerOuter,
        ) {
            Ok(v) => v,
            Err(_) => {
                page += 1;
                continue;
            }
        };
        let verso = page % 2 == 0;
        let want_header_x = if verso {
            fx.page_w - fx.margins.inner - 120_000
        } else {
            fx.margins.inner
        };
        // 页脚声明为右对齐：recto 贴外侧（右），verso 翻转后贴外侧（左）——
        // 「外侧」在两种页码下分别是 outer 边，故用 outer 而不是 inner。
        let want_footer_x = if verso {
            fx.margins.outer
        } else {
            fx.page_w - fx.margins.outer - 80_000
        };
        let mut good = boxes.len() == 2;
        let mut i = 0usize;
        while i < boxes.len() {
            if let Some(b) = boxes.get(i) {
                let (want_x, want_y) = match b.kind {
                    SlotKind::Header => (want_header_x, fx.margins.top + 5_000),
                    SlotKind::Footer => (
                        want_footer_x,
                        fx.page_h - fx.margins.bottom - 4_000 - 18_000,
                    ),
                };
                if b.x != want_x || b.y != want_y {
                    good = false;
                }
                // 内容区约束：奇页 left=inner/right=outer，偶页互换。
                let (l, r) = margins_lr(fx.margins, verso, MirrorPolicy::InnerOuter);
                if b.x < l || b.x + b.w > fx.page_w - r {
                    good = false;
                }
            }
            i += 1;
        }
        if good {
            ok += 1;
        }
        page += 1;
    }
    (ok, 10)
}

// ---------------------------------------------------------------------------
// 注入核：10 类坏样本逐类命中指定诊断码
// ---------------------------------------------------------------------------

/// 构造 10 类坏样本并逐类断言诊断码，返回 (命中数, 总数)。
pub fn injection_core() -> (usize, usize) {
    let total = 10usize;
    let mut caught = 0usize;
    let m = PageMargins {
        inner: 25_000,
        outer: 15_000,
        top: 20_000,
        bottom: 18_000,
    };
    let good_slot = HfSlot::new(1, SlotKind::Header, HAlign::Left, 120_000, 20_000, 5_000, true);

    // ① 空槽位表：放页必拒
    let empty_reg = HfRegistry::new();
    if place_page(
        &empty_reg,
        1,
        595_000,
        842_000,
        m,
        MirrorPolicy::InnerOuter,
    )
    .err()
        == Some(HfError::Empty)
    {
        caught += 1;
    }
    // ② 槽位 id 重复
    if let Ok(sl) = good_slot {
        let mut reg = HfRegistry::new();
        let first = reg.add(sl);
        if first.is_ok() && reg.add(sl).err() == Some(HfError::SlotDup) {
            caught += 1;
        }
    }
    // ③ 槽位容量满（第 9 个）
    {
        let mut reg = HfRegistry::new();
        let mut k = 0u8;
        let mut cap_hit = false;
        while k < 10 {
            let sl = HfSlot {
                id: k,
                kind: SlotKind::Header,
                align: HAlign::Left,
                width: 10_000,
                height: 5_000,
                offset: 0,
                mirror: false,
            };
            match reg.add(sl) {
                Err(HfError::SlotCap) => {
                    cap_hit = true;
                    break;
                }
                _ => {}
            }
            k += 1;
        }
        if cap_hit && reg.len() == SLOT_CAP {
            caught += 1;
        }
    }
    // ④ 对齐码非法
    if HAlign::from_u8(7).err() == Some(HfError::Align) {
        caught += 1;
    }
    // ⑤ 负边距
    if let (Ok(sl),) = (good_slot,) {
        let bad = PageMargins {
            inner: -1,
            outer: 15_000,
            top: 20_000,
            bottom: 18_000,
        };
        if slot_x(&sl, 1, 595_000, bad, MirrorPolicy::InnerOuter).err()
            == Some(HfError::MarginNeg)
        {
            caught += 1;
        }
    }
    // ⑥ 镜像槽 + 不镜像策略
    if let Ok(sl) = good_slot {
        if slot_x(&sl, 1, 595_000, m, MirrorPolicy::None).err()
            == Some(HfError::MirrorUnsupported)
        {
            caught += 1;
        }
    }
    // ⑦ 页号 0
    if let Ok(sl) = good_slot {
        if slot_x(&sl, 0, 595_000, m, MirrorPolicy::InnerOuter).err() == Some(HfError::PageOrder) {
            caught += 1;
        }
    }
    // ⑧ 槽位过宽越界
    if let Ok(sl) = HfSlot::new(9, SlotKind::Header, HAlign::Left, 900_000, 20_000, 0, false) {
        if slot_x(&sl, 1, 595_000, m, MirrorPolicy::InnerOuter).err() == Some(HfError::Bounds) {
            caught += 1;
        }
    }
    // ⑨ 页宽非正
    if let Ok(sl) = good_slot {
        if slot_x(&sl, 1, 0, m, MirrorPolicy::InnerOuter).err() == Some(HfError::Param) {
            caught += 1;
        }
    }
    // ⑩ 拼版超张数上限
    if booklet_plan(10, 2).err() == Some(HfError::Imposition) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50049 的 15 条判据。
pub fn run_unx50049_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50049");
    let fx = match mirror_fixture() {
        Some(v) => v,
        None => {
            s.add("50049-夹具", false, "标准版面构造失败");
            return s;
        }
    };

    // J1-1 槽位表容量 8 + 第 9 个拒
    let mut reg = HfRegistry::new();
    let mut k = 0u8;
    let mut add_ok = 0usize;
    let mut cap_err = false;
    while k < 10 {
        let sl = HfSlot {
            id: k,
            kind: SlotKind::Header,
            align: HAlign::Left,
            width: 10_000,
            height: 5_000,
            offset: 0,
            mirror: false,
        };
        match reg.add(sl) {
            Ok(()) => add_ok += 1,
            Err(HfError::SlotCap) => {
                cap_err = true;
                break;
            }
            Err(_) => break,
        }
        k += 1;
    }
    s.add(
        "50049-槽表容量",
        add_ok == SLOT_CAP && cap_err && reg.len() == SLOT_CAP,
        "槽表容量或溢出码漂移",
    );

    // J1-2 槽位 id 去重
    let dup_ok = match HfSlot::new(7, SlotKind::Footer, HAlign::Right, 10_000, 4_000, 0, false) {
        Ok(sl) => {
            let mut r = HfRegistry::new();
            r.add(sl).is_ok() && r.add(sl).err() == Some(HfError::SlotDup)
        }
        Err(_) => false,
    };
    s.add("50049-槽位去重", dup_ok, "重复 id 未被拒");

    // J1-3 对齐闭集
    let align_ok = HAlign::from_u8(0) == Ok(HAlign::Left)
        && HAlign::from_u8(1) == Ok(HAlign::Center)
        && HAlign::from_u8(2) == Ok(HAlign::Right)
        && HAlign::from_u8(3).err() == Some(HfError::Align)
        && HAlign::Left.flip() == HAlign::Right
        && HAlign::Center.flip() == HAlign::Center;
    s.add("50049-对齐闭集", align_ok, "对齐翻转或闭集漂移");

    // J1-4 页号下界拒
    let page0_ok = place_page(
        &fx.reg,
        0,
        fx.page_w,
        fx.page_h,
        fx.margins,
        MirrorPolicy::InnerOuter,
    )
    .err()
        == Some(HfError::PageOrder);
    s.add("50049-页号下界", page0_ok, "页号 0 未被拒");

    // J1-5 边距镜像：奇页 left=inner，偶页 left=outer
    let margin_ok = margins_lr(fx.margins, false, MirrorPolicy::InnerOuter)
        == (fx.margins.inner, fx.margins.outer)
        && margins_lr(fx.margins, true, MirrorPolicy::InnerOuter)
            == (fx.margins.outer, fx.margins.inner)
        && margins_lr(fx.margins, true, MirrorPolicy::None)
            == (fx.margins.inner, fx.margins.outer);
    s.add("50049-边距互换", margin_ok, "内外边距未互换");

    // J1-6 镜像 ×10 位置逐页一致
    let (ok, total) = j1_mirror_result();
    let det = format!("{}/{}", ok, total);
    s.add_dyn("50049-J1-镜像逐页", ok == 10 && total == 10, &det);

    // J1-7 相邻奇偶页镜像互补：x_odd + x_even + w == page_w
    let complement_ok = match (1u16, 2u16) {
        (a, b) => {
            let ra = place_page(
                &fx.reg,
                a,
                fx.page_w,
                fx.page_h,
                fx.margins,
                MirrorPolicy::InnerOuter,
            );
            let rb = place_page(
                &fx.reg,
                b,
                fx.page_w,
                fx.page_h,
                fx.margins,
                MirrorPolicy::InnerOuter,
            );
            match (ra, rb) {
                (Ok(ba), Ok(bb)) => {
                    let mut good = true;
                    let mut i = 0usize;
                    while i < ba.len() {
                        if let (Some(x), Some(y)) = (ba.get(i), bb.get(i)) {
                            if x.slot == y.slot && x.x + y.x + x.w != fx.page_w {
                                good = false;
                            }
                        }
                        i += 1;
                    }
                    good && !ba.is_empty()
                }
                _ => false,
            }
        }
    };
    s.add("50049-镜像互补", complement_ok, "相邻页位置不互补");

    // J1-8 页眉/页脚垂直位置与页奇偶无关
    let mut y_stable = true;
    let mut prev_h: Option<i32> = None;
    let mut prev_f: Option<i32> = None;
    let mut page = 1u16;
    while page <= 10 {
        if let Ok(boxes) = place_page(
            &fx.reg,
            page,
            fx.page_w,
            fx.page_h,
            fx.margins,
            MirrorPolicy::InnerOuter,
        ) {
            let mut i = 0usize;
            while i < boxes.len() {
                if let Some(b) = boxes.get(i) {
                    match b.kind {
                        SlotKind::Header => match prev_h {
                            None => prev_h = Some(b.y),
                            Some(v) => {
                                if v != b.y {
                                    y_stable = false;
                                }
                            }
                        },
                        SlotKind::Footer => match prev_f {
                            None => prev_f = Some(b.y),
                            Some(v) => {
                                if v != b.y {
                                    y_stable = false;
                                }
                            }
                        },
                    }
                }
                i += 1;
            }
        } else {
            y_stable = false;
        }
        page += 1;
    }
    s.add("50049-垂直稳定", y_stable, "页眉/页脚垂直位置随页漂移");

    // J1-9 居中槽：内外边距对称时奇偶页 x 相同；不对称时位移恰为
    // outer-inner（居中跟随的是文字块中心，不是纸面中心——这是真实版面语义，
    // 不能想当然断言「镜像不影响居中」）。
    let center_ok = match HfSlot::new(3, SlotKind::Header, HAlign::Center, 100_000, 20_000, 0, true)
    {
        Ok(sl) => {
            let sym = PageMargins {
                inner: 20_000,
                outer: 20_000,
                top: fx.margins.top,
                bottom: fx.margins.bottom,
            };
            let sa = slot_x(&sl, 1, fx.page_w, sym, MirrorPolicy::InnerOuter);
            let sb = slot_x(&sl, 2, fx.page_w, sym, MirrorPolicy::InnerOuter);
            let aa = slot_x(&sl, 1, fx.page_w, fx.margins, MirrorPolicy::InnerOuter);
            let ab = slot_x(&sl, 2, fx.page_w, fx.margins, MirrorPolicy::InnerOuter);
            match (sa, sb, aa, ab) {
                (Ok(x1), Ok(x2), Ok(y1), Ok(y2)) => {
                    x1 == x2 && y2 - y1 == fx.margins.outer - fx.margins.inner
                }
                _ => false,
            }
        }
        Err(_) => false,
    };
    s.add("50049-居中跟随文字块", center_ok, "居中槽位移不等于内外距差");

    // J1-10 不支持镜像的策略必拒
    let unsup_ok = match fx.reg.find(1) {
        Some(sl) => {
            slot_x(&sl, 1, fx.page_w, fx.margins, MirrorPolicy::None).err()
                == Some(HfError::MirrorUnsupported)
        }
        None => false,
    };
    s.add("50049-镜像策略错配", unsup_ok, "None 策略未拒镜像槽");

    // J1-11 越界槽必拒（页脚太矮 / 槽过宽）
    let bounds_ok = match (
        HfSlot::new(4, SlotKind::Header, HAlign::Left, 900_000, 20_000, 0, false),
        HfSlot::new(5, SlotKind::Footer, HAlign::Left, 100_000, 900_000, 0, false),
    ) {
        (Ok(wide), Ok(tall)) => {
            slot_x(&wide, 1, fx.page_w, fx.margins, MirrorPolicy::InnerOuter).err()
                == Some(HfError::Bounds)
                && slot_y(&tall, fx.page_h, fx.margins).err() == Some(HfError::Bounds)
        }
        _ => false,
    };
    s.add("50049-越界必拒", bounds_ok, "越界槽未被拒");

    // J1-12 小册子拼版：10 页 → 3 张，1..10 每页恰一次
    let book_ok = match booklet_plan(10, 8) {
        Ok(plan) => {
            let mut seen: Vec<u16> = Vec::with_capacity(12);
            let mut dup = false;
            let mut i = 0usize;
            let mut blanks = 0usize;
            while i < plan.len() {
                if let Some(sh) = plan.get(i) {
                    let sides = [sh.front_left, sh.front_right, sh.back_left, sh.back_right];
                    let mut k = 0usize;
                    while k < sides.len() {
                        match sides.get(k).and_then(|v| *v) {
                            Some(p) => {
                                if seen.contains(&p) {
                                    dup = true;
                                }
                                seen.push(p);
                            }
                            None => blanks += 1,
                        }
                        k += 1;
                    }
                }
                i += 1;
            }
            plan.len() == 3 && !dup && seen.len() == 10 && blanks == 2
        }
        Err(_) => false,
    };
    let det = match booklet_plan(10, 8) {
        Ok(p) => format!("sheets={}", p.len()),
        Err(_) => String::from("impose failed"),
    };
    s.add_dyn("50049-拼版覆盖", book_ok, &det);

    // J1-13 拼版上限拒 + 空作业拒
    let impose_ok = booklet_plan(10, 2).err() == Some(HfError::Imposition)
        && booklet_plan(0, 8).err() == Some(HfError::Empty);
    s.add("50049-拼版边界", impose_ok, "拼版上限/空作业未拒");

    // J1-14 拼版状态机：乱序拒 + 单调推进
    let mut sess = BookletSession::new(10);
    let premature = sess.paginate().err() == Some(HfError::PageOrder);
    let imposed = sess.impose(8).is_ok() && sess.stage() == BindStage::Imposed && sess.sheets() == 3;
    let twice = sess.impose(8).err() == Some(HfError::PageOrder);
    let pag = sess.paginate().is_ok() && sess.stage() == BindStage::Paginated;
    let fin = sess.finalize().is_ok() && sess.stage() == BindStage::Final;
    let rewind = BindStage::Final.advance(BindStage::Imposed).err() == Some(HfError::PageOrder);
    s.add(
        "50049-拼版状态机",
        premature && imposed && twice && pag && fin && rewind,
        "拼版状态机允许乱序",
    );

    // J1-15 台账去重 + 容量
    let ledger_ok = match place_page(
        &fx.reg,
        1,
        fx.page_w,
        fx.page_h,
        fx.margins,
        MirrorPolicy::InnerOuter,
    ) {
        Ok(boxes) => match boxes.get(0) {
            Some(first) => {
                let mut led = HfLedger::new();
                let r1 = led.record(*first);
                let r2 = led.record(*first);
                let mut cap_hit = false;
                let mut p = 1u16;
                while p <= 30 {
                    let b = PlacedBox {
                        slot: 9,
                        page: p,
                        kind: SlotKind::Header,
                        x: 0,
                        y: 0,
                        w: 1,
                        h: 1,
                    };
                    match led.record(b) {
                        Err(HfError::SlotCap) => {
                            cap_hit = true;
                            break;
                        }
                        _ => {}
                    }
                    p += 1;
                }
                r1.is_ok()
                    && r2.err() == Some(HfError::SlotDup)
                    && cap_hit
                    && led.len() == LEDGER_CAP
                    && led.rejected() == 2
            }
            None => false,
        },
        Err(_) => false,
    };
    s.add("50049-台账纪律", ledger_ok, "台账去重或容量失效");

    // J1-16 确定性
    let d1 = match place_page(&fx.reg, 7, fx.page_w, fx.page_h, fx.margins, MirrorPolicy::InnerOuter)
    {
        Ok(v) => box_digest(&v),
        Err(_) => 0,
    };
    let d2 = match place_page(&fx.reg, 7, fx.page_w, fx.page_h, fx.margins, MirrorPolicy::InnerOuter)
    {
        Ok(v) => box_digest(&v),
        Err(_) => 1,
    };
    s.add("50049-确定性", d1 == d2 && d1 != 0, "两次落位不一致");

    // J1-17 注入核 10/10
    let (caught, total) = injection_core();
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50049-注入必抓", caught == total, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50049_checks_green() {
        let set = run_unx50049_checks();
        assert!(set.all_passed(), "UNX-F50049 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50049_mirror_ten_pages() {
        let (ok, total) = j1_mirror_result();
        assert_eq!(ok, 10);
        assert_eq!(total, 10);
    }

    #[test]
    fn unx50049_booklet_covers_each_page_once() {
        let plan = match booklet_plan(10, 8) {
            Ok(v) => v,
            Err(_) => return,
        };
        let mut seen: Vec<u16> = Vec::new();
        let mut i = 0usize;
        while i < plan.len() {
            if let Some(sh) = plan.get(i) {
                let sides = [sh.front_left, sh.front_right, sh.back_left, sh.back_right];
                for s in sides.iter() {
                    if let Some(p) = *s {
                        assert!(!seen.contains(&p));
                        seen.push(p);
                    }
                }
            }
            i += 1;
        }
        assert_eq!(seen.len(), 10);
    }
}
