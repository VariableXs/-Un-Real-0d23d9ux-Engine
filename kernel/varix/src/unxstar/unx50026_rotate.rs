//! UNX-F50026 旋转语义（横纸张竖内容 90°/270° 旋转账）（0x42f60 段）
//!
//! 锚点：UNX-F50026-J1 旋转 ×2 方向输出逐像素镜像（模拟器档）；
//! 判据锚定 §AI-63 §2 专题二。
//!
//! 数据段：`SEG_BASE = 0x42f60`。诊断码是闭集 `0x42f61..=0x42f6a`
//! （`RotError` 十档，`code() = SEG_BASE + n`）。
//!
//! 结构清单
//! * 数据结构：`Rotation`（四档旋转群）、`Orientation`（纵/横）、
//!   `FeedEdge`（长边/短边进纸）、`Bitmap`（位图模拟器）、`RotEntry`
//!   （单页旋转决策）、`RotLedger`（旋转账本）；
//! * 求解器：`rotate_bitmap`（像素重映射）、`flip_horizontal` /
//!   `flip_vertical`（镜像）、`upright`（横纸张竖内容的旋转决策）、
//!   `compose`（旋转群合成）；
//! * 错误路径：`RotError` 十档；位图尺寸、像素数、角度码、账本项逐项校验；
//! * 注入核：`injection_core` 六类坏样本（空位图、像素数不符、非法角度、
//!   账本重复页、页号 0、账本乱序）。
//!
//! 像素映射为什么用「目标坐标反查源坐标」
//! --------------------------------------
//! 正向映射（遍历源像素写目标）会在 90° 时出现目标越界写，反查式
//! `dest(x', y') = src(f(x', y'))` 则天然落在目标尺寸内，且每个目标像素
//! 恰好写一次——双射由循环结构保证，再由 J1 的全量映射断言复核。
//! 四档的 `f`（源坐标）：
//! * `None`  ：(x', y')
//! * `Cw90`  ：(y', H-1-x')
//! * `Cw180` ：(W-1-x', H-1-y')
//! * `Cw270` ：(W-1-y', x')
//! 两方向（90/270）之间是**镜像共轭**关系，不是相等关系：
//! `flip_v(r90(flip_v(src))) == r270(src)`，J1 的「逐像素镜像」即按此断言。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x42f60 起 0x20 归本模块）。
pub const SEG_BASE: u32 = 0x42f60;

/// 位图边长上限（模拟器档）。超过 512 边的位图不再适合逐像素核对。
pub const MAX_SIDE: usize = 512;

/// 位图像素数上限（512×512）。
pub const MAX_PIXELS: usize = MAX_SIDE * MAX_SIDE;

pub const E_EMPTY: u16 = 1;
pub const E_DIMS: u16 = 2;
pub const E_ANGLE: u16 = 3;
pub const E_PAGE: u16 = 4;
pub const E_DUPLICATE: u16 = 5;
pub const E_ORDER: u16 = 6;
pub const E_LEDGER: u16 = 7;
pub const E_ORIENT: u16 = 8;
pub const E_PIXEL: u16 = 9;
pub const E_PLAN: u16 = 10;

/// 旋转域错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotError {
    Empty,
    Dims,
    Angle,
    Page,
    Duplicate,
    Order,
    Ledger,
    Orient,
    Pixel,
    Plan,
}

impl RotError {
    /// 诊断码：`SEG_BASE + 档内序号`。
    pub fn code(self) -> u32 {
        let n = match self {
            RotError::Empty => E_EMPTY,
            RotError::Dims => E_DIMS,
            RotError::Angle => E_ANGLE,
            RotError::Page => E_PAGE,
            RotError::Duplicate => E_DUPLICATE,
            RotError::Order => E_ORDER,
            RotError::Ledger => E_LEDGER,
            RotError::Orient => E_ORIENT,
            RotError::Pixel => E_PIXEL,
            RotError::Plan => E_PLAN,
        };
        SEG_BASE + n as u32
    }

    /// 面向操作员的稳定说明文本。
    pub fn message(self) -> &'static str {
        match self {
            RotError::Empty => "empty bitmap",
            RotError::Dims => "bitmap dimensions or pixel count invalid",
            RotError::Angle => "rotation code not in 0..=3",
            RotError::Page => "page number must be positive",
            RotError::Duplicate => "page rotated twice",
            RotError::Order => "page appended out of order",
            RotError::Ledger => "ledger full or entry rejected",
            RotError::Orient => "orientation code invalid",
            RotError::Pixel => "pixel index outside bitmap",
            RotError::Plan => "rotation plan failed self-verification",
        }
    }
}

/// 四档旋转（顺时针为正）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    None,
    Cw90,
    Cw180,
    Cw270,
}

impl Rotation {
    /// 持久编码 0..=3。
    pub fn code(self) -> u8 {
        match self {
            Rotation::None => 0,
            Rotation::Cw90 => 1,
            Rotation::Cw180 => 2,
            Rotation::Cw270 => 3,
        }
    }

    /// 由编码还原；非 0..=3 显式拒绝（45° 这类角度在页面旋转里不存在）。
    pub fn from_code(c: u8) -> Result<Rotation, RotError> {
        match c {
            0 => Ok(Rotation::None),
            1 => Ok(Rotation::Cw90),
            2 => Ok(Rotation::Cw180),
            3 => Ok(Rotation::Cw270),
            _ => Err(RotError::Angle),
        }
    }

    /// 由角度（度）还原；只接受 0/90/180/270，其它一律拒绝。
    pub fn from_degrees(deg: i32) -> Result<Rotation, RotError> {
        match deg {
            0 => Ok(Rotation::None),
            90 => Ok(Rotation::Cw90),
            180 => Ok(Rotation::Cw180),
            270 | -90 => Ok(Rotation::Cw270),
            _ => Err(RotError::Angle),
        }
    }

    /// 写日志用的稳定标签。
    pub fn label(self) -> &'static str {
        match self {
            Rotation::None => "0",
            Rotation::Cw90 => "90",
            Rotation::Cw180 => "180",
            Rotation::Cw270 => "270",
        }
    }

    /// 顺时针四分之一圈数（0..=3）。
    pub fn quarter_turns(self) -> u8 {
        self.code()
    }

    /// 逆旋转。`r.inverse()` 与 `r` 组合必须是恒等。
    pub fn inverse(self) -> Rotation {
        match self {
            Rotation::None => Rotation::None,
            Rotation::Cw90 => Rotation::Cw270,
            Rotation::Cw180 => Rotation::Cw180,
            Rotation::Cw270 => Rotation::Cw90,
        }
    }

    /// 群合成：先 `self` 再 `other`（即 `other ∘ self`）。
    pub fn compose(self, other: Rotation) -> Rotation {
        let q = (self.quarter_turns() + other.quarter_turns()) % 4;
        match q {
            0 => Rotation::None,
            1 => Rotation::Cw90,
            2 => Rotation::Cw180,
            _ => Rotation::Cw270,
        }
    }

    /// 该旋转下的目标尺寸：90/270 交换宽高。
    pub fn dimensions(self, w: usize, h: usize) -> (usize, usize) {
        match self {
            Rotation::None | Rotation::Cw180 => (w, h),
            Rotation::Cw90 | Rotation::Cw270 => (h, w),
        }
    }

    /// 是否交换宽高。
    pub fn swaps_dims(self) -> bool {
        matches!(self, Rotation::Cw90 | Rotation::Cw270)
    }
}

/// 页面方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    /// 持久编码 0=纵、1=横。
    pub fn code(self) -> u8 {
        match self {
            Orientation::Portrait => 0,
            Orientation::Landscape => 1,
        }
    }

    /// 由编码还原。
    pub fn from_code(c: u8) -> Result<Orientation, RotError> {
        match c {
            0 => Ok(Orientation::Portrait),
            1 => Ok(Orientation::Landscape),
            _ => Err(RotError::Orient),
        }
    }

    /// 由尺寸推断方向（宽>高为横）。相等时按纵向处理（正方形无方向）。
    pub fn of(w: usize, h: usize) -> Orientation {
        if w > h {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        }
    }
}

/// 进纸边。长边进纸是双面打印机的默认走纸方式，短边进纸用于相册/卡片。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedEdge {
    Long,
    Short,
}

impl FeedEdge {
    /// 持久编码 0=长边、1=短边。
    pub fn code(self) -> u8 {
        match self {
            FeedEdge::Long => 0,
            FeedEdge::Short => 1,
        }
    }

    /// 由编码还原。
    pub fn from_code(c: u8) -> Result<FeedEdge, RotError> {
        match c {
            0 => Ok(FeedEdge::Long),
            1 => Ok(FeedEdge::Short),
            _ => Err(RotError::Orient),
        }
    }
}

/// 位图模拟器：`px` 按行主序存放，值即内容。
#[derive(Clone, Debug, PartialEq)]
pub struct Bitmap {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Bitmap {
    /// 用给定像素建立位图；像素数必须等于 w×h，否则 `Dims`。
    pub fn from_pixels(w: usize, h: usize, px: Vec<u8>) -> Result<Bitmap, RotError> {
        if w == 0 || h == 0 {
            return Err(RotError::Empty);
        }
        if w > MAX_SIDE || h > MAX_SIDE || w.saturating_mul(h) > MAX_PIXELS {
            return Err(RotError::Dims);
        }
        if px.len() != w * h {
            return Err(RotError::Dims);
        }
        Ok(Bitmap { w, h, px })
    }

    /// 索引位图：每个像素存自己的源序号 `y*w+x`，用于验证映射无损。
    /// 需要 w*h ≤ 256（一个像素一个 u8 值）。
    pub fn indexed(w: usize, h: usize) -> Result<Bitmap, RotError> {
        if w == 0 || h == 0 {
            return Err(RotError::Empty);
        }
        if w * h > 256 {
            return Err(RotError::Dims);
        }
        let mut px = Vec::with_capacity(w * h);
        let mut y = 0usize;
        while y < h {
            let mut x = 0usize;
            while x < w {
                px.push((y * w + x) as u8);
                x += 1;
            }
            y += 1;
        }
        Bitmap::from_pixels(w, h, px)
    }

    /// 花纹位图：像素值 = (x*7 + y*31 + 5) mod 251，相邻格几乎不等，
    /// 镜像比对时任何错位都会露馅。
    pub fn patterned(w: usize, h: usize) -> Result<Bitmap, RotError> {
        if w == 0 || h == 0 {
            return Err(RotError::Empty);
        }
        let mut px = Vec::with_capacity(w * h);
        let mut y = 0usize;
        while y < h {
            let mut x = 0usize;
            while x < w {
                px.push(((x * 7 + y * 31 + 5) % 251) as u8);
                x += 1;
            }
            y += 1;
        }
        Bitmap::from_pixels(w, h, px)
    }

    /// 像素数。
    pub fn len(&self) -> usize {
        self.px.len()
    }

    /// 是否为空位图（构造期已禁止，保留查询口径）。
    pub fn is_empty(&self) -> bool {
        self.px.is_empty()
    }

    /// 取像素；越界返回 `None`（内核路径零 panic 的基石）。
    pub fn get(&self, x: usize, y: usize) -> Option<u8> {
        if x >= self.w || y >= self.h {
            return None;
        }
        self.px.get(y * self.w + x).copied()
    }

    /// 自检：像素数必须与尺寸一致。
    pub fn verify(&self) -> Result<(), RotError> {
        if self.w == 0 || self.h == 0 {
            return Err(RotError::Empty);
        }
        if self.w > MAX_SIDE || self.h > MAX_SIDE || self.w * self.h > MAX_PIXELS {
            return Err(RotError::Dims);
        }
        if self.px.len() != self.w * self.h {
            return Err(RotError::Pixel);
        }
        Ok(())
    }
}

/// 顺时针旋转。目标坐标反查源坐标，逐像素写一次。
pub fn rotate_bitmap(src: &Bitmap, rot: Rotation) -> Result<Bitmap, RotError> {
    src.verify()?;
    let (dw, dh) = rot.dimensions(src.w, src.h);
    if dw == 0 || dh == 0 {
        return Err(RotError::Dims);
    }
    let mut out = Vec::with_capacity(dw * dh);
    let mut dy = 0usize;
    while dy < dh {
        let mut dx = 0usize;
        while dx < dw {
            let (sx, sy) = match rot {
                Rotation::None => (dx, dy),
                Rotation::Cw90 => (dy, src.h - 1 - dx),
                Rotation::Cw180 => (src.w - 1 - dx, src.h - 1 - dy),
                Rotation::Cw270 => (src.w - 1 - dy, dx),
            };
            match src.get(sx, sy) {
                Some(v) => out.push(v),
                None => return Err(RotError::Pixel),
            }
            dx += 1;
        }
        dy += 1;
    }
    Bitmap::from_pixels(dw, dh, out)
}

/// 水平镜像（左右翻）：`out(x,y) = src(W-1-x, y)`。
pub fn flip_horizontal(src: &Bitmap) -> Result<Bitmap, RotError> {
    src.verify()?;
    let mut out = Vec::with_capacity(src.w * src.h);
    let mut y = 0usize;
    while y < src.h {
        let mut x = 0usize;
        while x < src.w {
            match src.get(src.w - 1 - x, y) {
                Some(v) => out.push(v),
                None => return Err(RotError::Pixel),
            }
            x += 1;
        }
        y += 1;
    }
    Bitmap::from_pixels(src.w, src.h, out)
}

/// 垂直镜像（上下翻）：`out(x,y) = src(x, H-1-y)`。
pub fn flip_vertical(src: &Bitmap) -> Result<Bitmap, RotError> {
    src.verify()?;
    let mut out = Vec::with_capacity(src.w * src.h);
    let mut y = 0usize;
    while y < src.h {
        let mut x = 0usize;
        while x < src.w {
            match src.get(x, src.h - 1 - y) {
                Some(v) => out.push(v),
                None => return Err(RotError::Pixel),
            }
            x += 1;
        }
        y += 1;
    }
    Bitmap::from_pixels(src.w, src.h, out)
}

/// 横纸张 / 竖内容的旋转决策。
///
/// 语义：纸张与内容方向一致时不动；不一致时必须转 90 或 270，方向由进纸边
/// 决定——长边进纸按顺时针 90°（读第一行的方向与走纸方向一致），
/// 短边进纸按逆时针 90°（等价 270°）。两档互为逆，绝不会"看起来差不多"。
pub fn upright(media: Orientation, content: Orientation, feed: FeedEdge) -> Rotation {
    if media == content {
        return Rotation::None;
    }
    match (media, feed) {
        (Orientation::Landscape, FeedEdge::Long) => Rotation::Cw90,
        (Orientation::Landscape, FeedEdge::Short) => Rotation::Cw270,
        (Orientation::Portrait, FeedEdge::Long) => Rotation::Cw270,
        (Orientation::Portrait, FeedEdge::Short) => Rotation::Cw90,
    }
}

/// 单页旋转决策。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RotEntry {
    pub page: u16,
    pub media: Orientation,
    pub content: Orientation,
    pub feed: FeedEdge,
    pub rotation: Rotation,
}

impl RotEntry {
    /// 决策是否自洽：旋转量必须等于 `upright` 的结论，且方向真的被转正。
    pub fn consistent(&self) -> bool {
        if self.rotation != upright(self.media, self.content, self.feed) {
            return false;
        }
        if self.media == self.content {
            return self.rotation == Rotation::None;
        }
        // 方向不同时必须确实交换宽高。
        self.rotation.swaps_dims()
    }
}

/// 旋转账本：逐页记决策，拒绝重复页与乱序页。
#[derive(Clone, Debug)]
pub struct RotLedger {
    entries: Vec<RotEntry>,
    limit: usize,
}

impl RotLedger {
    /// 建账，`limit` 为页数硬上限。
    pub fn new(limit: usize) -> RotLedger {
        RotLedger {
            entries: Vec::new(),
            limit,
        }
    }

    /// 已记账页数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 追加一页。页号 0、重复页、页号倒退、容量满都显式拒绝。
    pub fn push(&mut self, page: u16, media: Orientation, content: Orientation, feed: FeedEdge) -> Result<Rotation, RotError> {
        if page == 0 {
            return Err(RotError::Page);
        }
        if self.entries.len() >= self.limit {
            return Err(RotError::Ledger);
        }
        if let Some(last) = self.entries.last() {
            if page < last.page {
                return Err(RotError::Order);
            }
            if page == last.page {
                return Err(RotError::Duplicate);
            }
        }
        let rotation = upright(media, content, feed);
        let e = RotEntry {
            page,
            media,
            content,
            feed,
            rotation,
        };
        if !e.consistent() {
            return Err(RotError::Plan);
        }
        self.entries.push(e);
        Ok(rotation)
    }

    /// 回放旋转序列。
    pub fn replay(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.entries.len());
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                out.push(e.rotation.code());
            }
            i += 1;
        }
        out
    }

    /// 逐条重算决策，返回不自洽条数（0 = 账本可信）。
    pub fn audit(&self) -> usize {
        let mut bad = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if !e.consistent() || e.rotation != upright(e.media, e.content, e.feed) {
                    bad += 1;
                }
            } else {
                bad += 1;
            }
            i += 1;
        }
        bad
    }

    /// 统计某个旋转量的页数（便于判据核对方向分布）。
    pub fn count_of(&self, rot: Rotation) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.rotation == rot {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

/// 六类坏样本：空位图、像素数不符、非法角度、重复页、页号 0、账本乱序。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    if Bitmap::from_pixels(0, 4, Vec::new()).err() == Some(RotError::Empty) {
        caught += 1;
    }
    // 尺寸说 4×4 但只给 15 个像素
    let mut short = Vec::new();
    let mut i = 0usize;
    while i < 15 {
        short.push(i as u8);
        i += 1;
    }
    if Bitmap::from_pixels(4, 4, short).err() == Some(RotError::Dims) {
        caught += 1;
    }
    if Rotation::from_degrees(45).err() == Some(RotError::Angle) {
        caught += 1;
    }
    let mut led = RotLedger::new(16);
    let med = Orientation::Landscape;
    let con = Orientation::Portrait;
    if led.push(1, med, con, FeedEdge::Long).is_err() {
        return (caught, total);
    }
    if led.push(1, med, con, FeedEdge::Short).err() == Some(RotError::Duplicate) {
        caught += 1;
    }
    if led.push(0, med, con, FeedEdge::Long).err() == Some(RotError::Page) {
        caught += 1;
    }
    // 先推一页到 2，再回退到 1（页号倒退）
    if led.push(2, con, con, FeedEdge::Long).is_ok() {
        if led.push(1, med, con, FeedEdge::Long).err() == Some(RotError::Order) {
            caught += 1;
        }
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50026 判据集（16 条）。
pub fn run_unx50026_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50026");

    // 8×6 索引位图：像素值 = 源序号，映射正确性可直接读值验证
    let idx = match Bitmap::indexed(8, 6) {
        Ok(b) => b,
        Err(_) => {
            s.add("50026-J1-尺寸互换", false, "索引位图构造失败");
            return s;
        }
    };

    // J1-1 90/270 交换宽高，180/0 不交换
    let r90 = rotate_bitmap(&idx, Rotation::Cw90);
    let r270 = rotate_bitmap(&idx, Rotation::Cw270);
    let ok = match (&r90, &r270) {
        (Ok(a), Ok(b)) => {
            a.w == 6
                && a.h == 8
                && b.w == 6
                && b.h == 8
                && Rotation::Cw180.dimensions(8, 6) == (8, 6)
                && Rotation::None.dimensions(8, 6) == (8, 6)
        }
        _ => false,
    };
    s.add("50026-J1-尺寸互换", ok, if ok { "" } else { "尺寸未交换" });

    // J1-2 全量映射：dest(x',y') 的值 == 源序号公式（四档逐像素）
    let mut ok = true;
    let mut checked = 0usize;
    let mut rot_i = 0u8;
    while rot_i < 4 {
        let rot = match Rotation::from_code(rot_i) {
            Ok(v) => v,
            Err(_) => Rotation::None,
        };
        match rotate_bitmap(&idx, rot) {
            Ok(dst) => {
                let (dw, dh) = rot.dimensions(idx.w, idx.h);
                let mut y = 0usize;
                while y < dh {
                    let mut x = 0usize;
                    while x < dw {
                        let (sx, sy) = match rot {
                            Rotation::None => (x, y),
                            Rotation::Cw90 => (y, idx.h - 1 - x),
                            Rotation::Cw180 => (idx.w - 1 - x, idx.h - 1 - y),
                            Rotation::Cw270 => (idx.w - 1 - y, x),
                        };
                        let want = (sy * idx.w + sx) as u8;
                        if dst.get(x, y) != Some(want) {
                            ok = false;
                        }
                        checked += 1;
                        x += 1;
                    }
                    y += 1;
                }
            }
            Err(_) => ok = false,
        }
        rot_i += 1;
    }
    let ok = ok && checked == 48 * 4;
    let det = format!("checked={}", checked);
    s.add_dyn("50026-J1-全量映射", ok, &det);

    // J1-3 双射：90 旋转后每个源序号恰好出现一次
    let mut seen = [0u8; 48];
    let mut ok = true;
    let mut y = 0usize;
    if let Ok(dst) = &r90 {
        while y < dst.h {
            let mut x = 0usize;
            while x < dst.w {
                match dst.get(x, y) {
                    Some(v) => {
                        let at = v as usize;
                        if at >= seen.len() {
                            ok = false;
                        } else if let Some(c) = seen.get_mut(at) {
                            *c += 1;
                        }
                    }
                    None => ok = false,
                }
                x += 1;
            }
            y += 1;
        }
    } else {
        ok = false;
    }
    let mut k = 0usize;
    while k < 48 {
        if seen.get(k) != Some(&1u8) {
            ok = false;
        }
        k += 1;
    }
    s.add("50026-J1-双射", ok, if ok { "" } else { "映射非双射" });

    // J1-4 90 四次 = 恒等
    let mut acc = idx.clone();
    let mut i = 0usize;
    let mut good = true;
    while i < 4 {
        acc = match rotate_bitmap(&acc, Rotation::Cw90) {
            Ok(v) => v,
            Err(_) => {
                good = false;
                break;
            }
        };
        i += 1;
    }
    let ok = good && acc == idx;
    s.add("50026-J1-90四次恒等", ok, if ok { "" } else { "90° 循环不闭合" });

    // J1-5 180 两次 = 恒等
    let twice = match rotate_bitmap(&idx, Rotation::Cw180) {
        Ok(a) => match rotate_bitmap(&a, Rotation::Cw180) {
            Ok(b) => Some(b),
            Err(_) => None,
        },
        Err(_) => None,
    };
    let ok = match &twice {
        Some(b) => *b == idx,
        None => false,
    };
    s.add("50026-J1-180两次恒等", ok, if ok { "" } else { "180° 不闭合" });

    // J1-6 90 + 270 = 恒等（两方向互为逆）
    let there_and_back = match &r90 {
        Ok(a) => match rotate_bitmap(a, Rotation::Cw270) {
            Ok(b) => Some(b),
            Err(_) => None,
        },
        Err(_) => None,
    };
    let ok = match &there_and_back {
        Some(b) => *b == idx,
        None => false,
    };
    s.add("50026-J1-两向互逆", ok, if ok { "" } else { "90/270 不互逆" });

    // J1-7 两方向逐像素镜像：flip_v(r90(flip_v(src))) == r270(src)
    // 以及 flip_h(r90(src)) == r270(flip_h(src))；两条都走全量像素。
    let pat = match Bitmap::patterned(9, 5) {
        Ok(b) => b,
        Err(_) => idx.clone(),
    };
    let mut ok = true;
    let mut diff = 0usize;
    let mut samples = 0usize;
    // 恒等式一：flip_v(r90(flip_v(src))) == r270(src)
    let fv = flip_vertical(&pat);
    let inner = match &fv {
        Ok(v) => rotate_bitmap(v, Rotation::Cw90).ok(),
        Err(_) => None,
    };
    let lhs = match &inner {
        Some(v) => flip_vertical(v).ok(),
        None => None,
    };
    let rhs = rotate_bitmap(&pat, Rotation::Cw270).ok();
    match (&lhs, &rhs) {
        (Some(a), Some(b)) if a.w == b.w && a.h == b.h => {
            let mut y = 0usize;
            while y < a.h {
                let mut x = 0usize;
                while x < a.w {
                    samples += 1;
                    if a.get(x, y) != b.get(x, y) {
                        diff += 1;
                    }
                    x += 1;
                }
                y += 1;
            }
        }
        _ => ok = false,
    }
    // 恒等式二：flip_h(r90(src)) == r270(flip_h(src))
    let fh = flip_horizontal(&pat);
    let lhs2 = match &fh {
        Ok(v) => rotate_bitmap(v, Rotation::Cw270).ok(),
        Err(_) => None,
    };
    let rhs2 = match &rhs {
        Some(_) => match rotate_bitmap(&pat, Rotation::Cw90) {
            Ok(v) => flip_horizontal(&v).ok(),
            Err(_) => None,
        },
        None => None,
    };
    match (&lhs2, &rhs2) {
        (Some(a), Some(b)) if a.w == b.w && a.h == b.h => {
            let mut y = 0usize;
            while y < a.h {
                let mut x = 0usize;
                while x < a.w {
                    samples += 1;
                    if a.get(x, y) != b.get(x, y) {
                        diff += 1;
                    }
                    x += 1;
                }
                y += 1;
            }
        }
        _ => ok = false,
    }
    let ok = ok && diff == 0 && samples == 2 * (pat.h * pat.w);
    let det = format!("samples={} diff={}", samples, diff);
    s.add_dyn("50026-J1-两向镜像", ok, &det);

    // J1-8 群封闭：16 个组合的 compose 与逐次旋转结果一致
    let mut ok = true;
    let mut pairs = 0usize;
    let mut a_i = 0u8;
    while a_i < 4 {
        let a = match Rotation::from_code(a_i) {
            Ok(v) => v,
            Err(_) => Rotation::None,
        };
        let mut b_i = 0u8;
        while b_i < 4 {
            let b = match Rotation::from_code(b_i) {
            Ok(v) => v,
            Err(_) => Rotation::None,
        };
            let c = a.compose(b);
            let step1 = rotate_bitmap(&idx, a);
            let step2 = match &step1 {
                Ok(v) => rotate_bitmap(v, b),
                Err(_) => Err(RotError::Plan),
            };
            let direct = rotate_bitmap(&idx, c);
            match (&step2, &direct) {
                (Ok(u), Ok(v)) => {
                    if u != v {
                        ok = false;
                    }
                }
                _ => ok = false,
            }
            pairs += 1;
            b_i += 1;
        }
        a_i += 1;
    }
    let ok = ok && pairs == 16;
    let det = format!("pairs={}", pairs);
    s.add_dyn("50026-J1-群封闭", ok, &det);

    // J1-9 横纸张竖内容：长边进纸 90、短边进纸 270，且两者互为逆
    let med = Orientation::Landscape;
    let con = Orientation::Portrait;
    let l = upright(med, con, FeedEdge::Long);
    let sh = upright(med, con, FeedEdge::Short);
    let ok = l == Rotation::Cw90
        && sh == Rotation::Cw270
        && l.compose(sh) == Rotation::None
        && l.swaps_dims()
        && upright(con, con, FeedEdge::Long) == Rotation::None
        && upright(con, med, FeedEdge::Long) == Rotation::Cw270;
    let det = format!("long={} short={}", l.label(), sh.label());
    s.add_dyn("50026-J1-横纸竖内容", ok, &det);

    // J1-10 账本 12 条：audit=0、方向分布正确、回放长度一致
    let mut led = RotLedger::new(64);
    let mut recorded = 0usize;
    let mut page = 1u16;
    while page <= 12 {
        let media = if page % 2 == 0 { Orientation::Portrait } else { Orientation::Landscape };
        let content = if page % 3 == 0 { Orientation::Landscape } else { Orientation::Portrait };
        let feed = if page % 2 == 0 { FeedEdge::Short } else { FeedEdge::Long };
        if led.push(page, media, content, feed).is_ok() {
            recorded += 1;
        }
        page += 1;
    }
    let ok = recorded == 12
        && led.len() == 12
        && led.audit() == 0
        && led.replay().len() == 12
        && led.count_of(Rotation::None) + led.count_of(Rotation::Cw90) + led.count_of(Rotation::Cw270)
            == 12;
    let det = format!("rec={} none={} cw90={} cw270={}", recorded, led.count_of(Rotation::None), led.count_of(Rotation::Cw90), led.count_of(Rotation::Cw270));
    s.add_dyn("50026-J1-账本十二", ok, &det);

    // J1-11 重复页拒
    let mut led2 = RotLedger::new(8);
    let mut ok = false;
    if led2.push(3, med, con, FeedEdge::Long).is_ok() {
        ok = led2.push(3, med, con, FeedEdge::Short).err() == Some(RotError::Duplicate);
    }
    s.add("50026-J1-重复页拒", ok, if ok { "" } else { "重复页未拒" });

    // J1-12 乱序拒（页号倒退）
    let mut led3 = RotLedger::new(8);
    let mut ok = false;
    if led3.push(5, med, con, FeedEdge::Long).is_ok() {
        ok = led3.push(4, med, con, FeedEdge::Long).err() == Some(RotError::Order);
    }
    s.add("50026-J1-乱序拒", ok, if ok { "" } else { "乱序未拒" });

    // J1-13 空位图 / 像素数不符 / 尺寸越界
    let e1 = Bitmap::from_pixels(0, 3, Vec::new()).err() == Some(RotError::Empty);
    let mut px = Vec::new();
    px.push(1u8);
    let e2 = Bitmap::from_pixels(3, 3, px).err() == Some(RotError::Dims);
    let e3 = Bitmap::from_pixels(MAX_SIDE + 1, 4, Vec::new()).err() == Some(RotError::Dims);
    let ok = e1 && e2 && e3;
    s.add("50026-J1-位图非法", ok, if ok { "" } else { "非法位图未拒" });

    // J1-14 角度码：45/91 拒绝，0/90/180/270 接受
    let ok = Rotation::from_degrees(45).err() == Some(RotError::Angle)
        && Rotation::from_degrees(91).err() == Some(RotError::Angle)
        && Rotation::from_code(4).err() == Some(RotError::Angle)
        && Rotation::from_degrees(270) == Ok(Rotation::Cw270)
        && Orientation::from_code(2).err() == Some(RotError::Orient);
    s.add("50026-J1-角度码", ok, if ok { "" } else { "角度码校验失效" });

    // J1-15 确定性：同一输入两次旋转逐像素一致
    let a = rotate_bitmap(&pat, Rotation::Cw90);
    let b = rotate_bitmap(&pat, Rotation::Cw90);
    let ok = match (&a, &b) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    };
    s.add("50026-J1-确定性", ok, if ok { "" } else { "两次旋转不一致" });

    // J1-16 注入核
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50026-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50026_checks_green() {
        let set = run_unx50026_checks();
        assert!(set.all_passed(), "UNX-F50026 red: {}", set.red_items_text());
    }

    /// 任意尺寸：4 次 90° 之后逐像素回到原图。
    #[test]
    fn unx50026_four_quarter_turns() {
        let src = match Bitmap::patterned(7, 3) {
            Ok(b) => b,
            Err(_) => return,
        };
        let mut cur = src.clone();
        let mut i = 0usize;
        while i < 4 {
            cur = match rotate_bitmap(&cur, Rotation::Cw90) {
                Ok(v) => v,
                Err(_) => return,
            };
            i += 1;
        }
        assert_eq!(cur, src);
    }

    /// 270 的逆是 90，且维度交换两次回到原尺寸。
    #[test]
    fn unx50026_inverse_dims() {
        let src = match Bitmap::patterned(4, 9) {
            Ok(b) => b,
            Err(_) => return,
        };
        let a = rotate_bitmap(&src, Rotation::Cw270);
        assert!(a.is_ok());
        if let Ok(a) = a {
            assert_eq!((a.w, a.h), (9, 4));
            let back = rotate_bitmap(&a, Rotation::Cw270.inverse());
            assert!(back.is_ok());
            if let Ok(back) = back {
                assert_eq!(back, src);
            }
        }
    }
}
