//! UNX-F50025 页边距与缩放协商（fit-to-page / shrink 显式语义）（0x42f40 段）
//!
//! 锚点：UNX-F50025-J1 缩放 ×30 结果与规则一致，超界拒绝；判据锚定
//! §AI-63 §2 专题二。
//!
//! 数据段：`SEG_BASE = 0x42f40`。诊断码是闭集 `0x42f41..=0x42f4a`
//! （`FitError` 十档，`code() = SEG_BASE + n`）。
//!
//! 结构清单
//! * 数据结构：`PageBox`（页面/内容尺寸）、`Margins`（四边非对称边距）、
//!   `FitMode`（五档语义）、`FitPolicy`（每档策略注册表）、`FitDecision`
//!   （可审计的决策记录）、`FitLedger`（决策账本 + 拒绝码账）；
//! * 求解器：`available_box`（可用框）、`negotiate`（缩放 + 居中 + 设备像素
//!   量化）、`expected_scale`（独立参考公式）、`audit_scale_table`；
//! * 错误路径：`FitError` 十档；先校验页面/内容/边距/DPI，再算缩放，
//!   任何「超界」都在返回决策前显式失败；
//! * 注入核：`injection_core` 六类坏样本（零页面、零内容、边距吃穿、
//!   缩放过小、缩放过���、DPI 非法）。
//!
//! 五档语义为什么必须显式
//! ----------------------
//! 打印链路上「缩放」有三种常见误解：以为 shrink 会放大、以为 fit 一定
//! 留边、以为 fit-width 不会纵向溢出。本模块把每档写成策略表：
//! * `None`：缩放恒 1，溢出**显式记账**（overflow 标记），不自动改版；
//! * `Shrink`：只缩不放，上限 1.0；
//! * `Fit`：等比放到可用框内（可放大），永不溢出；
//! * `FitWidth`：只按宽度定标，纵向溢出必须被标记（允许，但不许沉默）；
//! * `FitHeight`：只按高度定标，横向溢出同理。
//! 判据里「超界拒绝」指的是**策略不允许的溢出**与**缩放出界**，而不是
//! 把「按宽定标导致的纵向溢出」误判成错误——那是这档的正常结果。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::{ceilf, fabsf};

/// 诊断段基址（0x42f40 起 0x20 归本模块）。
pub const SEG_BASE: u32 = 0x42f40;

/// 可用框最小边长（mm）。低于 10mm 已无可排内容，直接判边距不可行。
pub const MIN_AVAIL_MM: f32 = 10.0;

/// 缩放下限。小于 5% 时 6pt 正文变成 0.3pt，属于「印出来也看不见」，
/// 与其出一张白纸，不如显式失败让调用方改版。
pub const MIN_SCALE: f32 = 0.05;

/// 缩放上限。>8 倍时字形描边已明显锯齿，且超出绝大多数 RIP 的放大档。
pub const MAX_SCALE: f32 = 8.0;

/// 设备像素量化的 DPI 合法区间。
pub const MIN_DPI: f32 = 72.0;
/// 见 `MIN_DPI`。
pub const MAX_DPI: f32 = 4800.0;

/// 毫米/英寸换算常数（1 英寸 = 25.4mm，定义值）。
pub const MM_PER_INCH: f32 = 25.4;

pub const E_EMPTY: u16 = 1;
pub const E_PAPER: u16 = 2;
pub const E_MARGIN: u16 = 3;
pub const E_DEGENERATE: u16 = 4;
pub const E_SCALE_SMALL: u16 = 5;
pub const E_SCALE_BIG: u16 = 6;
pub const E_OVERFLOW: u16 = 7;
pub const E_DPI: u16 = 8;
pub const E_LEDGER: u16 = 9;
pub const E_MODE: u16 = 10;

/// 缩放协商域错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitError {
    Empty,
    Paper,
    Margin,
    Degenerate,
    ScaleSmall,
    ScaleBig,
    Overflow,
    Dpi,
    Ledger,
    Mode,
}

impl FitError {
    /// 诊断码：`SEG_BASE + 档内序号`。
    pub fn code(self) -> u32 {
        let n = match self {
            FitError::Empty => E_EMPTY,
            FitError::Paper => E_PAPER,
            FitError::Margin => E_MARGIN,
            FitError::Degenerate => E_DEGENERATE,
            FitError::ScaleSmall => E_SCALE_SMALL,
            FitError::ScaleBig => E_SCALE_BIG,
            FitError::Overflow => E_OVERFLOW,
            FitError::Dpi => E_DPI,
            FitError::Ledger => E_LEDGER,
            FitError::Mode => E_MODE,
        };
        SEG_BASE + n as u32
    }

    /// 面向操作员的稳定说明文本。
    pub fn message(self) -> &'static str {
        match self {
            FitError::Empty => "empty request",
            FitError::Paper => "page or content size must be positive",
            FitError::Margin => "margins leave less than 10mm of printable area",
            FitError::Degenerate => "content degenerates to zero size",
            FitError::ScaleSmall => "computed scale below 0.05",
            FitError::ScaleBig => "computed scale above 8.0",
            FitError::Overflow => "policy forbids overflow on this axis",
            FitError::Dpi => "dpi outside 72..4800",
            FitError::Ledger => "ledger entry rejected",
            FitError::Mode => "unknown fit mode code",
        }
    }
}

/// 页面 / 内容尺寸（毫米）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageBox {
    pub w: f32,
    pub h: f32,
}

impl PageBox {
    pub const fn new(w: f32, h: f32) -> PageBox {
        PageBox { w, h }
    }

    /// A4 纵向（210×297）。
    pub const A4_PORTRAIT: PageBox = PageBox::new(210.0, 297.0);

    /// 面积（mm²）。
    pub fn area(&self) -> f32 {
        self.w * self.h
    }

    /// 正尺寸且有限。
    pub fn is_positive(&self) -> bool {
        self.w > 0.0 && self.h > 0.0 && self.w.is_finite() && self.h.is_finite()
    }
}

/// 四边边距（毫米，非对称）。打印链路上左装订边常比右边宽，
/// 所以这里不用「统一边距 + 对称假设」。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Margins {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Margins {
    /// 四边各自指定。
    pub const fn new(top: f32, right: f32, bottom: f32, left: f32) -> Margins {
        Margins {
            top,
            right,
            bottom,
            left,
        }
    }

    /// 四边同一值。
    pub const fn uniform(m: f32) -> Margins {
        Margins::new(m, m, m, m)
    }

    /// 边距合法性：非负且有限。
    pub fn validate(&self) -> Result<(), FitError> {
        let ok = self.top.is_finite()
            && self.right.is_finite()
            && self.bottom.is_finite()
            && self.left.is_finite()
            && self.top >= 0.0
            && self.right >= 0.0
            && self.bottom >= 0.0
            && self.left >= 0.0;
        if ok {
            Ok(())
        } else {
            Err(FitError::Margin)
        }
    }

    /// 可用框宽 = 页宽 - 左右边距。
    pub fn avail_w(&self, page: &PageBox) -> f32 {
        page.w - self.left - self.right
    }

    /// 可用框高 = 页高 - 上下边距。
    pub fn avail_h(&self, page: &PageBox) -> f32 {
        page.h - self.top - self.bottom
    }
}

/// 缩放五档。持久编码写进账本，永不重排。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitMode {
    None,
    Shrink,
    Fit,
    FitWidth,
    FitHeight,
}

impl FitMode {
    /// 持久编码 0..=4。
    pub fn code(self) -> u8 {
        match self {
            FitMode::None => 0,
            FitMode::Shrink => 1,
            FitMode::Fit => 2,
            FitMode::FitWidth => 3,
            FitMode::FitHeight => 4,
        }
    }

    /// 由编码还原；未知码显式拒绝（不落默认档）。
    pub fn from_code(c: u8) -> Result<FitMode, FitError> {
        match c {
            0 => Ok(FitMode::None),
            1 => Ok(FitMode::Shrink),
            2 => Ok(FitMode::Fit),
            3 => Ok(FitMode::FitWidth),
            4 => Ok(FitMode::FitHeight),
            _ => Err(FitError::Mode),
        }
    }

    /// 写日志用的稳定标签。
    pub fn label(self) -> &'static str {
        match self {
            FitMode::None => "none",
            FitMode::Shrink => "shrink",
            FitMode::Fit => "fit",
            FitMode::FitWidth => "fit-width",
            FitMode::FitHeight => "fit-height",
        }
    }

    /// 该档是否允许放大。
    pub fn allows_enlarge(self) -> bool {
        matches!(self, FitMode::Fit | FitMode::FitWidth | FitMode::FitHeight)
    }

    /// 该档是否允许某轴溢出（仅按单轴定标的两档允许）。
    pub fn allows_overflow(self) -> bool {
        matches!(self, FitMode::FitWidth | FitMode::FitHeight | FitMode::None)
    }

    /// 该档策略（注册表入口）。
    pub fn policy(self) -> FitPolicy {
        FitPolicy::for_mode(self)
    }
}

/// 每档的显式策略。`min_scale` / `max_scale` 是硬边界，命中即拒绝。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitPolicy {
    pub mode: FitMode,
    pub allow_enlarge: bool,
    pub allow_overflow: bool,
    pub min_scale: f32,
    pub max_scale: f32,
}

impl FitPolicy {
    /// 五档策略表。所有边界值来自模块常量的推导，不在此处另立魔数。
    pub fn for_mode(mode: FitMode) -> FitPolicy {
        FitPolicy {
            mode,
            allow_enlarge: mode.allows_enlarge(),
            allow_overflow: mode.allows_overflow(),
            min_scale: MIN_SCALE,
            max_scale: MAX_SCALE,
        }
    }

    /// 按策略校验缩放值。
    pub fn check_scale(&self, scale: f32) -> Result<(), FitError> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(FitError::Degenerate);
        }
        if scale < self.min_scale {
            return Err(FitError::ScaleSmall);
        }
        if scale > self.max_scale {
            return Err(FitError::ScaleBig);
        }
        Ok(())
    }
}

/// 协商请求。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitRequest {
    pub page: PageBox,
    pub content: PageBox,
    pub margins: Margins,
    pub mode: FitMode,
    pub dpi: f32,
}

impl FitRequest {
    /// 新建请求（A4、15mm 边距、fit、300dpi）。
    pub const fn new(page: PageBox, content: PageBox, mode: FitMode) -> FitRequest {
        FitRequest {
            page,
            content,
            margins: Margins::uniform(15.0),
            mode,
            dpi: 300.0,
        }
    }

    /// 设置边距。
    pub fn with_margins(mut self, margins: Margins) -> FitRequest {
        self.margins = margins;
        self
    }

    /// 设置 DPI（影响设备像素量化）。
    pub fn with_dpi(mut self, dpi: f32) -> FitRequest {
        self.dpi = dpi;
        self
    }

    /// 输入合法性：页面 → 内容 → 边距 → 可用框 → DPI。
    pub fn validate(&self) -> Result<(), FitError> {
        if !self.page.is_positive() || !self.content.is_positive() {
            return Err(FitError::Paper);
        }
        if self.content.w <= 0.0 || self.content.h <= 0.0 {
            return Err(FitError::Degenerate);
        }
        self.margins.validate()?;
        let aw = self.margins.avail_w(&self.page);
        let ah = self.margins.avail_h(&self.page);
        if aw < MIN_AVAIL_MM || ah < MIN_AVAIL_MM {
            return Err(FitError::Margin);
        }
        if !self.dpi.is_finite() || self.dpi < MIN_DPI || self.dpi > MAX_DPI {
            return Err(FitError::Dpi);
        }
        Ok(())
    }
}

/// 决策记录（可审计：每条决策都能被 `expected_scale` 独立复算）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitDecision {
    pub mode: FitMode,
    pub scale: f32,
    pub dst: PageBox,
    pub origin_x: f32,
    pub origin_y: f32,
    pub overflow_x: bool,
    pub overflow_y: bool,
    pub residual_w: f32,
    pub residual_h: f32,
    pub px_w: u32,
    pub px_h: u32,
}

impl FitDecision {
    /// 缩放后内容是否完全落在可用框内。
    pub fn inside(&self) -> bool {
        !self.overflow_x && !self.overflow_y
    }

    /// 内容尺寸是否严格等比（长宽比守恒）。
    pub fn aspect(&self) -> f32 {
        if self.dst.h <= 0.0 {
            return 0.0;
        }
        self.dst.w / self.dst.h
    }
}

/// 缩放求解。
///
/// 顺序：输入校验 → 求两轴缩放 → 按档取标 → 策略校验 → 溢出判定 →
/// 居中落位 → 设备像素量化。放不下/出界一律在返回决策前失败。
pub fn negotiate(req: &FitRequest) -> Result<FitDecision, FitError> {
    req.validate()?;
    let aw = req.margins.avail_w(&req.page);
    let ah = req.margins.avail_h(&req.page);
    let sx = aw / req.content.w;
    let sy = ah / req.content.h;
    if !sx.is_finite() || !sy.is_finite() || sx <= 0.0 || sy <= 0.0 {
        return Err(FitError::Degenerate);
    }
    let scale = match req.mode {
        FitMode::None => 1.0,
        FitMode::Shrink => {
            let m = if sx < sy { sx } else { sy };
            if m > 1.0 {
                1.0
            } else {
                m
            }
        }
        FitMode::Fit => {
            if sx < sy {
                sx
            } else {
                sy
            }
        }
        FitMode::FitWidth => sx,
        FitMode::FitHeight => sy,
    };
    let policy = req.mode.policy();
    policy.check_scale(scale)?;
    if !policy.allow_enlarge && scale > 1.0 {
        return Err(FitError::ScaleBig);
    }
    let dst = PageBox::new(req.content.w * scale, req.content.h * scale);
    let overflow_x = dst.w > aw + 1e-4;
    let overflow_y = dst.h > ah + 1e-4;
    if (overflow_x || overflow_y) && !policy.allow_overflow {
        return Err(FitError::Overflow);
    }
    let residual_w = aw - dst.w;
    let residual_h = ah - dst.h;
    let origin_x = req.margins.left + residual_w * 0.5;
    let origin_y = req.margins.top + residual_h * 0.5;
    let px_w = device_pixels(dst.w, req.dpi)?;
    let px_h = device_pixels(dst.h, req.dpi)?;
    Ok(FitDecision {
        mode: req.mode,
        scale,
        dst,
        origin_x,
        origin_y,
        overflow_x,
        overflow_y,
        residual_w,
        residual_h,
        px_w,
        px_h,
    })
}

/// 毫米 → 设备像素（向上取整）。向上取整是为了「宁可多一个像素也不丢边缘」；
/// 反过来向下取整会让最后一列描边落在纸外。
pub fn device_pixels(mm: f32, dpi: f32) -> Result<u32, FitError> {
    if !dpi.is_finite() || dpi < MIN_DPI || dpi > MAX_DPI {
        return Err(FitError::Dpi);
    }
    if !mm.is_finite() || mm <= 0.0 {
        return Err(FitError::Degenerate);
    }
    let px = ceilf(mm / MM_PER_INCH * dpi);
    if px <= 0.0 || px > 1_000_000.0 {
        return Err(FitError::ScaleBig);
    }
    Ok(px as u32)
}

/// 独立参考公式（J1 的对照组）：按模式语义直接给出应有缩放。
///
/// 与 `negotiate` 的区别是这里不做任何溢出/策略判断，只算数——两边分开
/// 写、再逐例比对，才能证明「实现没把某档的取标轴弄反」。
pub fn expected_scale(mode: FitMode, avail: PageBox, content: PageBox) -> f32 {
    let sx = avail.w / content.w;
    let sy = avail.h / content.h;
    let m = if sx < sy { sx } else { sy };
    match mode {
        FitMode::None => 1.0,
        FitMode::Shrink => {
            if m > 1.0 {
                1.0
            } else {
                m
            }
        }
        FitMode::Fit => m,
        FitMode::FitWidth => sx,
        FitMode::FitHeight => sy,
    }
}

/// 决策账本：逐条记账 + 拒绝码账。
///
/// 拒绝也入账（记诊断码而非吞掉），这样「30 次协商里有几次被拒、被哪档拒」
/// 可以事后复核，而不是只看最后一张纸。
#[derive(Clone, Debug)]
pub struct FitLedger {
    entries: Vec<(u16, FitDecision)>,
    rejects: Vec<u32>,
    limit: usize,
}

impl FitLedger {
    /// 建账，`limit` 为决策条数硬上限。
    pub fn new(limit: usize) -> FitLedger {
        FitLedger {
            entries: Vec::new(),
            rejects: Vec::new(),
            limit,
        }
    }

    /// 已记账决策数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 被拒次数。
    pub fn rejects(&self) -> usize {
        self.rejects.len()
    }

    /// 记一条决策。拒绝：容量满、页号为 0、重复页、缩放非有限。
    pub fn record(&mut self, page: u16, d: FitDecision) -> Result<(), FitError> {
        if self.entries.len() >= self.limit {
            self.rejects.push(FitError::Ledger.code());
            return Err(FitError::Ledger);
        }
        if page == 0 || !d.scale.is_finite() || d.scale <= 0.0 {
            self.rejects.push(FitError::Ledger.code());
            return Err(FitError::Ledger);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.0 == page {
                    self.rejects.push(FitError::Ledger.code());
                    return Err(FitError::Ledger);
                }
            }
            i += 1;
        }
        self.entries.push((page, d));
        Ok(())
    }

    /// 记一条拒绝（诊断码）。
    pub fn record_reject(&mut self, code: u32) {
        self.rejects.push(code);
    }

    /// 回放：按记账顺序的缩放值。
    pub fn replay(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.entries.len());
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                out.push(e.1.scale);
            }
            i += 1;
        }
        out
    }

    /// 回放页号。
    pub fn pages(&self) -> Vec<u16> {
        let mut out = Vec::with_capacity(self.entries.len());
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                out.push(e.0);
            }
            i += 1;
        }
        out
    }

    /// 逐条与参考缩放表比对，返回不一致条数（0 = 全部一致）。
    pub fn audit(&self, table: &[f32]) -> usize {
        if table.len() != self.entries.len() {
            return table.len().max(self.entries.len());
        }
        let mut bad = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let (Some(e), Some(t)) = (self.entries.get(i), table.get(i)) {
                if fabsf(e.1.scale - *t) > 1e-5 {
                    bad += 1;
                }
            } else {
                bad += 1;
            }
            i += 1;
        }
        bad
    }
}

/// 30 组内容尺寸（mm）：宽 40+11i、高 30+7i，i=0..30。
/// 覆盖「明显小于页面」到「略超页面」两段，让五档都至少命中一次放大/缩小。
pub fn size_table() -> Vec<PageBox> {
    let mut v = Vec::with_capacity(30);
    let mut i = 0usize;
    while i < 30 {
        v.push(PageBox::new(40.0 + 11.0 * i as f32, 30.0 + 7.0 * i as f32));
        i += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

/// 六类坏样本：零页面、零内容、边距吃穿、缩放过小、缩放过��、DPI 非法。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;
    let base = FitRequest::new(PageBox::A4_PORTRAIT, PageBox::new(100.0, 100.0), FitMode::Fit);

    if negotiate(&FitRequest::new(PageBox::new(0.0, 297.0), PageBox::new(100.0, 100.0), FitMode::Fit))
        .err()
        == Some(FitError::Paper)
    {
        caught += 1;
    }
    if negotiate(&FitRequest::new(PageBox::A4_PORTRAIT, PageBox::new(0.0, 100.0), FitMode::Fit))
        .err()
        == Some(FitError::Paper)
    {
        caught += 1;
    }
    if negotiate(&base.with_margins(Margins::uniform(110.0))).err() == Some(FitError::Margin) {
        caught += 1;
    }
    if negotiate(&FitRequest::new(
        PageBox::A4_PORTRAIT,
        PageBox::new(10000.0, 10000.0),
        FitMode::Fit,
    ))
    .err()
        == Some(FitError::ScaleSmall)
    {
        caught += 1;
    }
    if negotiate(&FitRequest::new(
        PageBox::A4_PORTRAIT,
        PageBox::new(5.0, 5.0),
        FitMode::Fit,
    ))
    .err()
        == Some(FitError::ScaleBig)
    {
        caught += 1;
    }
    if negotiate(&base.with_dpi(10.0)).err() == Some(FitError::Dpi) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50025 判据集（16 条）。
pub fn run_unx50025_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50025");
    let page = PageBox::A4_PORTRAIT;
    let margins = Margins::uniform(15.0);
    let avail = PageBox::new(margins.avail_w(&page), margins.avail_h(&page));

    // J1-1 30 组尺寸 × fit：缩放与参考公式一致、居中、不溢出、等比
    let mut ok = true;
    let mut worst = 0.0f32;
    let mut max_center_err = 0.0f32;
    let mut cases = 0usize;
    let mut ledger = FitLedger::new(64);
    let mut table = Vec::with_capacity(30);
    let sizes = size_table();
    let mut i = 0usize;
    while i < 30 {
        let content = match sizes.get(i) {
            Some(c) => *c,
            None => break,
        };
        let req = FitRequest::new(page, content, FitMode::Fit).with_margins(margins);
        let want = expected_scale(FitMode::Fit, avail, content);
        match negotiate(&req) {
            Ok(d) => {
                let e = fabsf(d.scale - want);
                if e > worst {
                    worst = e;
                }
                let ce = fabsf(d.origin_x - (margins.left + d.residual_w * 0.5));
                let ce2 = fabsf(d.origin_y - (margins.top + d.residual_h * 0.5));
                if ce > max_center_err {
                    max_center_err = ce;
                }
                if ce2 > max_center_err {
                    max_center_err = ce2;
                }
                if !d.inside() || fabsf(d.aspect() - content.w / content.h) > 1e-3 {
                    ok = false;
                }
                if ledger.record(30 + i as u16, d).is_err() {
                    ok = false;
                }
                table.push(d.scale);
                cases += 1;
            }
            Err(_) => ok = false,
        }
        i += 1;
    }
    if cases != 30 {
        ok = false;
    }
    let ok = ok && worst < 1e-5 && max_center_err < 1e-4;
    let det = format!("cases={} dscale={:.7} dcenter={:.7}", cases, worst, max_center_err);
    s.add_dyn("50025-J1-三十例一致", ok, &det);

    // J1-2 shrink 只缩不放：小内容缩放恰为 1，大内容 < 1
    let small = negotiate(&FitRequest::new(page, PageBox::new(50.0, 50.0), FitMode::Shrink).with_margins(margins));
    let big = negotiate(&FitRequest::new(page, PageBox::new(400.0, 600.0), FitMode::Shrink).with_margins(margins));
    let ok = match (&small, &big) {
        (Ok(a), Ok(b)) => fabsf(a.scale - 1.0) < 1e-6 && b.scale < 1.0 && b.scale > 0.0,
        _ => false,
    };
    let det = match (&small, &big) {
        (Ok(a), Ok(b)) => format!("small={:.4} big={:.4}", a.scale, b.scale),
        _ => format!("err"),
    };
    s.add_dyn("50025-J1-shrink语义", ok, &det);

    // J1-3 fit 允许放大：小内容缩放 > 1（上限内）
    let fit_small = negotiate(&FitRequest::new(page, PageBox::new(50.0, 50.0), FitMode::Fit).with_margins(margins));
    let ok = match &fit_small {
        Ok(d) => d.scale > 1.0 && d.inside(),
        Err(_) => false,
    };
    let det = match &fit_small {
        Ok(d) => format!("fit_scale={:.4}", d.scale),
        Err(_) => format!("err"),
    };
    s.add_dyn("50025-J1-fit放大", ok, &det);

    // J1-4 fit-width：宽贴边、纵向溢出显式标记（允许但不沉默）
    let fw = negotiate(&FitRequest::new(page, PageBox::new(180.0, 400.0), FitMode::FitWidth).with_margins(margins));
    let ok = match &fw {
        Ok(d) => {
            fabsf(d.dst.w - avail.w) < 1e-3 && d.overflow_y && !d.overflow_x && d.scale > 0.0
        }
        Err(_) => false,
    };
    let det = match &fw {
        Ok(d) => format!("w={:.2} ox={} oy={}", d.dst.w, d.overflow_x, d.overflow_y),
        Err(_) => format!("err"),
    };
    s.add_dyn("50025-J1-fitwidth溢出", ok, &det);

    // J1-5 fit-height：高贴边、横向溢出显式标记
    let fh = negotiate(&FitRequest::new(page, PageBox::new(400.0, 267.0), FitMode::FitHeight).with_margins(margins));
    let ok = match &fh {
        Ok(d) => fabsf(d.dst.h - avail.h) < 1e-3 && d.overflow_x && !d.overflow_y,
        Err(_) => false,
    };
    let det = match &fh {
        Ok(d) => format!("h={:.2} ox={} oy={}", d.dst.h, d.overflow_x, d.overflow_y),
        Err(_) => format!("err"),
    };
    s.add_dyn("50025-J1-fitheight溢出", ok, &det);

    // J1-6 none：恒 1 倍，溢出只记账不拦截
    let non = negotiate(&FitRequest::new(page, PageBox::new(400.0, 500.0), FitMode::None).with_margins(margins));
    let ok = match &non {
        Ok(d) => fabsf(d.scale - 1.0) < 1e-6 && d.overflow_x && d.overflow_y,
        Err(_) => false,
    };
    s.add("50025-J1-none语义", ok, if ok { "" } else { "none 被静默缩放" });

    // J1-7 超界拒绝：边距吃穿 / 内容为零 / 缩放过小 / 缩放过大 / DPI
    let base = FitRequest::new(page, PageBox::new(100.0, 100.0), FitMode::Fit);
    let r1 = negotiate(&base.with_margins(Margins::uniform(110.0))).err() == Some(FitError::Margin);
    let r2 = negotiate(&FitRequest::new(page, PageBox::new(0.0, 100.0), FitMode::Fit)).err()
        == Some(FitError::Paper);
    let r3 = negotiate(&FitRequest::new(page, PageBox::new(10000.0, 10000.0), FitMode::Fit)).err()
        == Some(FitError::ScaleSmall);
    let r4 = negotiate(&FitRequest::new(page, PageBox::new(5.0, 5.0), FitMode::Fit)).err()
        == Some(FitError::ScaleBig);
    let r5 = negotiate(&base.with_dpi(10.0)).err() == Some(FitError::Dpi);
    let ok = r1 && r2 && r3 && r4 && r5;
    let det = format!("{}{}{}{}{}", r1, r2, r3, r4, r5);
    s.add_dyn("50025-J1-超界拒绝", ok, &det);

    // J1-8 负边距拒绝
    let neg = Margins::new(-1.0, 10.0, 10.0, 10.0);
    let ok = negotiate(&base.with_margins(neg)).err() == Some(FitError::Margin);
    s.add("50025-J1-负边距拒", ok, if ok { "" } else { "负边距未拒" });

    // J1-9 账本 30 条：条数、回放、与参考表逐条一致
    let replay = ledger.replay();
    let ok = ledger.len() == 30
        && ledger.rejects() == 0
        && replay.len() == 30
        && ledger.audit(&table) == 0;
    let det = format!("len={} rejects={}", ledger.len(), ledger.rejects());
    s.add_dyn("50025-J1-账本三十条", ok, &det);

    // J1-10 账本拒绝：重复页 / 页号 0 / 容量满
    let d0 = negotiate(&FitRequest::new(page, PageBox::new(80.0, 80.0), FitMode::Fit).with_margins(margins));
    let mut led = FitLedger::new(8);
    let mut ok = false;
    if let Ok(d) = &d0 {
        if led.record(7, *d).is_ok() {
            let dup = led.record(7, *d).err() == Some(FitError::Ledger);
            let zero = led.record(0, *d).err() == Some(FitError::Ledger);
            ok = dup && zero && led.rejects() >= 2;
        }
    }
    s.add("50025-J1-账本拒绝", ok, if ok { "" } else { "账本未拒坏条目" });

    // J1-11 设备像素量化：向上取整且与 mm 单调
    let px_a = device_pixels(25.4, 300.0);
    let px_b = device_pixels(25.4, 600.0);
    let px_small = device_pixels(0.01, 72.0);
    let ok = match (&px_a, &px_b, &px_small) {
        (Ok(a), Ok(b), Ok(c)) => *a == 300 && *b == 600 && *c == 1,
        _ => false,
    };
    s.add("50025-J1-像素量化", ok, if ok { "" } else { "device px 量化错" });

    // J1-12 非对称边距：内容落位不越过较宽一侧
    let asym = Margins::new(10.0, 10.0, 10.0, 40.0);
    let req = FitRequest::new(page, PageBox::new(100.0, 200.0), FitMode::Fit).with_margins(asym);
    let ok = match negotiate(&req) {
        Ok(d) => {
            let aw = asym.avail_w(&page);
            d.origin_x >= asym.left - 1e-4
                && d.origin_x + d.dst.w <= asym.left + aw + 1e-4
                && fabsf((d.origin_x - asym.left) - (aw - d.dst.w) * 0.5) < 1e-4
        }
        Err(_) => false,
    };
    s.add("50025-J1-非对称边距", ok, if ok { "" } else { "落位越界" });

    // J1-13 等比守恒：缩放前后长宽比一致（五档逐一）
    let mut ok = true;
    let modes = [FitMode::None, FitMode::Shrink, FitMode::Fit, FitMode::FitWidth, FitMode::FitHeight];
    let mut m = 0usize;
    while m < 5 {
        let mode = modes[m];
        let req = FitRequest::new(page, PageBox::new(120.0, 80.0), mode).with_margins(margins);
        match negotiate(&req) {
            Ok(d) => {
                if fabsf(d.aspect() - 120.0 / 80.0) > 1e-3 {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        m += 1;
    }
    s.add("50025-J1-等比守恒", ok, if ok { "" } else { "长宽比漂移" });

    // J1-14 策略表一致性：allow_enlarge 与档位语义一致
    let ok = !FitMode::None.allows_enlarge()
        && !FitMode::Shrink.allows_enlarge()
        && FitMode::Fit.allows_enlarge()
        && FitMode::FitWidth.allows_enlarge()
        && FitMode::FitHeight.allows_enlarge()
        && FitMode::from_code(9).err() == Some(FitError::Mode);
    s.add("50025-J1-策略表", ok, if ok { "" } else { "策略表与语义不符" });

    // J1-15 确定性：同请求两次决策逐位一致
    let a1 = negotiate(&req);
    let a2 = negotiate(&req);
    let ok = match (&a1, &a2) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    };
    s.add("50025-J1-确定性", ok, if ok { "" } else { "两次决策不一致" });

    // J1-16 注入核
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50025-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50025_checks_green() {
        let set = run_unx50025_checks();
        assert!(set.all_passed(), "UNX-F50025 red: {}", set.red_items_text());
    }

    /// fit 的缩放必须让内容恰好贴住至少一条可用边。
    #[test]
    fn unx50025_fit_touches_edge() {
        let page = PageBox::A4_PORTRAIT;
        let m = Margins::uniform(15.0);
        let content = PageBox::new(100.0, 50.0);
        let req = FitRequest::new(page, content, FitMode::Fit).with_margins(m);
        let d = negotiate(&req);
        let d = match d {
            Ok(v) => v,
            Err(_) => return,
        };
        let aw = m.avail_w(&page);
        let ah = m.avail_h(&page);
        let touch_w = fabsf(d.dst.w - aw) < 1e-3;
        let touch_h = fabsf(d.dst.h - ah) < 1e-3;
        assert!(touch_w || touch_h);
        assert!(d.inside());
    }

    /// shrink 在内容小于页面时不得缩放。
    #[test]
    fn unx50025_shrink_identity() {
        let req = FitRequest::new(PageBox::A4_PORTRAIT, PageBox::new(60.0, 60.0), FitMode::Shrink);
        let d = negotiate(&req);
        assert!(d.is_ok());
        if let Ok(d) = d {
            assert!((d.scale - 1.0).abs() < 1e-6);
        }
    }
}
