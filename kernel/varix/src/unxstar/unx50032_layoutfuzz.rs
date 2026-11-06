//! UNX-F50032 布局 fuzz 面（极端页数/极端缩放注入不崩）（0x43020 段）
//!
//! 锚点：UNX-F50032-J1 fuzz ×10^3 零崩；判据锚定 §AI-63 §5。
//!
//! 数据段：`SEG_BASE = 0x43020`，诊断码闭集 `0x43020 + 1..=0x43020 + 10`。
//!
//! 结构清单：
//! - 几何：`PageBox`（包含/严格相交/面积），全部判据带 eps，避免浮点边界误报；
//! - 求解器：`LayoutSpec → LayoutPlan`。N-up 档决定网格 cols×rows，单张纸容量
//!   即 cols×rows；超出容量的页数落 `deferred` **显式记账**，不静默丢页——
//!   守恒式 `boxes.len() + deferred == pages` 就是 J1 的机器口径；
//! - fuzz 账：`Lcg`（确定性线性同余，常数取 Numerical Recipes 的
//!   6364136223846793005 / 1442695040888963407）驱动 `fuzz_campaign`：
//!   每轮从极端池抽取页数/媒体/边距/缩放，求解成功则复算四条不变量
//!   （盒在纸内、盒不重叠、页数守恒、摘要可复算）+ 一次重解比特一致；
//!   求解失败只登记错误码位图。「零崩」定义为：违例数 0 且
//!   `solved + rejected == iters`（拒绝也必须有闭集内的码，不许静默吞）；
//! - 注入核：`injection_core` 五类坏 spec 逐类断言具体诊断码。
//!
//! 为什么极端缩放走「钳制」而不是「拒绝」：缩放落到 1e-9 / 1e9 时内容盒会
//! 退化成点或溢出格，但排版语义仍可定义（退化到最小可印尺寸 / 贴满格）。
//! 钳制并显式置 `clamped` 能让管线继续消费；非法值（0 / NaN / Inf）仍是硬拒绝。
//! 这也是 fuzz 面与普通校验模块的分工：前者保「不崩」，后者保「值对」。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use libm::fabsf;

/// 诊断段基址（0x43020 段，独占 0x20）。
pub const SEG_BASE: u32 = 0x43020;

/// 单文档页数上限：65535 是 u16 页号的满量程，再大就必须换分册模型。
pub const MAX_PAGES: u32 = 65_535;
/// 媒体边长上限（pt）：1e7 ≈ 3.5km，超过它 f32 的有效位不足以分辨 1pt 格。
pub const MAX_MEDIA: f32 = 1.0e7;
/// 内容缩放有效区间：千分之一以下是点，超过 1 会越格；两端都钳制。
pub const MIN_SCALE: f32 = 1.0e-3;
pub const MAX_SCALE: f32 = 1.0;
/// 几何绝对容差（pt）；相对项另按跨度 1e-6 放大（见 `tolerance`）。
pub const GEOM_EPS: f32 = 1.0e-3;
/// fuzz 轮数上限：防单次自检把测试时间拖爆。
pub const MAX_FUZZ_ITERS: usize = 20_000;

const E_MEDIA: u16 = 1;
const E_MARGIN: u16 = 2;
const E_GUTTER: u16 = 3;
const E_NUP: u16 = 4;
const E_PAGES: u16 = 5;
const E_SCALE: u16 = 6;
const E_NOROOM: u16 = 7;
const E_BUDGET: u16 = 8;
const E_DRIFT: u16 = 9;
const E_RANGE: u16 = 10;

/// 布局 fuzz 域错误十型，`code()` 恒为 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzError {
    BadMedia,
    BadMargin,
    BadGutter,
    BadNup,
    TooManyPages,
    BadScale,
    NoRoom,
    Budget,
    DigestDrift,
    Range,
}

impl FuzzError {
    pub fn code(self) -> u32 {
        let n = match self {
            FuzzError::BadMedia => E_MEDIA,
            FuzzError::BadMargin => E_MARGIN,
            FuzzError::BadGutter => E_GUTTER,
            FuzzError::BadNup => E_NUP,
            FuzzError::TooManyPages => E_PAGES,
            FuzzError::BadScale => E_SCALE,
            FuzzError::NoRoom => E_NOROOM,
            FuzzError::Budget => E_BUDGET,
            FuzzError::DigestDrift => E_DRIFT,
            FuzzError::Range => E_RANGE,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            FuzzError::BadMedia => "media box non-finite or out of range",
            FuzzError::BadMargin => "margin non-finite or negative",
            FuzzError::BadGutter => "gutter non-finite or negative",
            FuzzError::BadNup => "n-up level not in {1,2,4,6,9,16}",
            FuzzError::TooManyPages => "page count above 65535",
            FuzzError::BadScale => "content scale non-finite or non-positive",
            FuzzError::NoRoom => "margins/gutters leave no printable cell",
            FuzzError::Budget => "fuzz iteration budget invalid",
            FuzzError::DigestDrift => "plan digest does not recompute",
            FuzzError::Range => "argument outside accepted range",
        }
    }
}

/// 轴对齐矩形（pt，原点左上，y 向下）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl PageBox {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> PageBox {
        PageBox { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn area(&self) -> f32 {
        if self.w <= 0.0 || self.h <= 0.0 {
            0.0
        } else {
            self.w * self.h
        }
    }

    /// 自包含于 `m`（带 eps）。分母不参与比较，避免 NaN 传染式误判成 true。
    pub fn within(&self, m: &PageBox, eps: f32) -> bool {
        self.w >= 0.0
            && self.h >= 0.0
            && self.x >= m.x - eps
            && self.y >= m.y - eps
            && self.right() <= m.right() + eps
            && self.bottom() <= m.bottom() + eps
    }

    /// 严格相交：重叠宽高都大于 eps 才算——贴边相邻是合法排版，不算重叠。
    pub fn intersects(&self, o: &PageBox, eps: f32) -> bool {
        let ow = minf(self.right(), o.right()) - maxf(self.x, o.x);
        let oh = minf(self.bottom(), o.bottom()) - maxf(self.y, o.y);
        ow > eps && oh > eps
    }
}

fn minf(a: f32, b: f32) -> f32 {
    if a < b {
        a
    } else {
        b
    }
}

fn maxf(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}

fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// 容差随跨度放大：f32 在 1e6 上 ULP≈0.06，固定 1e-3 会误报越界。
pub fn tolerance(span: f32) -> f32 {
    GEOM_EPS + fabsf(span) * 1.0e-6
}

/// 一份待求解的版面规格（Copy，便于 fuzz 反复变形）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutSpec {
    pub pages: u32,
    pub nup: u8,
    pub media_w: f32,
    pub media_h: f32,
    pub margin: f32,
    pub gutter: f32,
    pub scale: f32,
}

impl LayoutSpec {
    pub const fn new(pages: u32, nup: u8, media_w: f32, media_h: f32) -> LayoutSpec {
        LayoutSpec {
            pages,
            nup,
            media_w,
            media_h,
            margin: 0.0,
            gutter: 0.0,
            scale: 1.0,
        }
    }
}

/// 求解结果。`deferred` 是「本张纸放不下、留待后续纸」的页数。
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutPlan {
    pub boxes: Vec<PageBox>,
    pub capacity: u16,
    pub deferred: u32,
    pub clamped: bool,
    pub digest: u64,
}

/// N-up 档 → (cols, rows)。只认印刷市场真实存在的六档，其余显式拒绝。
pub fn grid_of(nup: u8) -> Result<(u8, u8), FuzzError> {
    match nup {
        1 => Ok((1, 1)),
        2 => Ok((2, 1)),
        4 => Ok((2, 2)),
        6 => Ok((3, 2)),
        9 => Ok((3, 3)),
        16 => Ok((4, 4)),
        _ => Err(FuzzError::BadNup),
    }
}

/// 求解单张纸版面。合法性逐项前置校验，任何失败都返回闭集内的诊断码。
pub fn solve(spec: &LayoutSpec) -> Result<LayoutPlan, FuzzError> {
    if !spec.media_w.is_finite() || !spec.media_h.is_finite() {
        return Err(FuzzError::BadMedia);
    }
    if spec.media_w <= 0.0 || spec.media_h <= 0.0 {
        return Err(FuzzError::BadMedia);
    }
    if spec.media_w > MAX_MEDIA || spec.media_h > MAX_MEDIA {
        return Err(FuzzError::BadMedia);
    }
    if !spec.margin.is_finite() || spec.margin < 0.0 {
        return Err(FuzzError::BadMargin);
    }
    if !spec.gutter.is_finite() || spec.gutter < 0.0 {
        return Err(FuzzError::BadGutter);
    }
    if !spec.scale.is_finite() || spec.scale <= 0.0 {
        return Err(FuzzError::BadScale);
    }
    if spec.pages > MAX_PAGES {
        return Err(FuzzError::TooManyPages);
    }
    let (cols, rows) = grid_of(spec.nup)?;
    let capacity = cols as u16 * rows as u16;
    let usable_w = spec.media_w - 2.0 * spec.margin;
    let usable_h = spec.media_h - 2.0 * spec.margin;
    if usable_w <= 0.0 || usable_h <= 0.0 {
        return Err(FuzzError::NoRoom);
    }
    let cell_w = (usable_w - spec.gutter * (cols as f32 - 1.0)) / cols as f32;
    let cell_h = (usable_h - spec.gutter * (rows as f32 - 1.0)) / rows as f32;
    if !cell_w.is_finite() || !cell_h.is_finite() || cell_w <= 0.0 || cell_h <= 0.0 {
        return Err(FuzzError::NoRoom);
    }

    // 缩放钳制：越界值仍有排版语义，钳到端点并显式记账（见文件头）。
    let clamped = spec.scale < MIN_SCALE || spec.scale > MAX_SCALE;
    let eff = clampf(spec.scale, MIN_SCALE, MAX_SCALE);
    let cw = cell_w * eff;
    let ch = cell_h * eff;

    let target = if spec.pages < capacity as u32 {
        spec.pages
    } else {
        capacity as u32
    };
    let mut boxes = Vec::with_capacity(target as usize);
    let mut n = 0u32;
    while n < target {
        let col = n % cols as u32;
        let row = n / cols as u32;
        let cx = spec.margin + col as f32 * (cell_w + spec.gutter);
        let cy = spec.margin + row as f32 * (cell_h + spec.gutter);
        boxes.push(PageBox::new(
            cx + (cell_w - cw) * 0.5,
            cy + (cell_h - ch) * 0.5,
            cw,
            ch,
        ));
        n += 1;
    }
    let deferred = spec.pages - target;
    let digest = digest_of(spec, &boxes);
    Ok(LayoutPlan {
        boxes,
        capacity,
        deferred,
        clamped,
        digest,
    })
}

const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut k = 0u32;
    while k < 4 {
        h ^= ((v >> (k * 8)) & 0xFF) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    h
}

fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut k = 0u32;
    while k < 8 {
        h ^= ((v >> (k * 8)) & 0xFF) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    h
}

fn fnv_f32(h: u64, v: f32) -> u64 {
    fnv_u32(h, v.to_bits())
}

/// 版面摘要（FNV-1a over spec + 逐盒坐标比特）。同输入必同摘要。
pub fn digest_of(spec: &LayoutSpec, boxes: &[PageBox]) -> u64 {
    let mut h = FNV_OFFSET;
    h = fnv_u32(h, spec.pages);
    h = fnv_u32(h, spec.nup as u32);
    h = fnv_f32(h, spec.media_w);
    h = fnv_f32(h, spec.media_h);
    h = fnv_f32(h, spec.margin);
    h = fnv_f32(h, spec.gutter);
    h = fnv_f32(h, spec.scale);
    h = fnv_u32(h, boxes.len() as u32);
    let mut i = 0usize;
    while i < boxes.len() {
        if let Some(b) = boxes.get(i) {
            h = fnv_f32(h, b.x);
            h = fnv_f32(h, b.y);
            h = fnv_f32(h, b.w);
            h = fnv_f32(h, b.h);
        }
        i += 1;
    }
    h
}

/// 复算摘要；`LayoutPlan.digest` 与重算不一致即 DigestDrift（防账目被篡改）。
pub fn verify_plan(spec: &LayoutSpec, plan: &LayoutPlan) -> Result<(), FuzzError> {
    if plan.digest != digest_of(spec, &plan.boxes) {
        return Err(FuzzError::DigestDrift);
    }
    Ok(())
}

/// 确定性线性同余发生器（常数固定，跨平台同种子同序列）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg {
            state: seed ^ 0xA5A5_5A5A_4302_0000,
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }

    /// 取 [0, n) 的索引；n 为 0 时返回 0（调用方保证池非空）。
    pub fn pick(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next_u32() % n
        }
    }
}

/// fuzz 轮次账。`codes` 是错误码位图（第 n 位对应 `SEG_BASE + n + 1`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzReport {
    pub iters: usize,
    pub solved: usize,
    pub rejected: usize,
    pub violations: usize,
    pub codes: u16,
    pub seal: u64,
}

const PAGES_POOL: [u32; 10] = [0, 1, 2, 3, 7, 16, 17, 999, 4096, 65_535];
const NUP_POOL: [u8; 8] = [1, 2, 4, 6, 9, 16, 3, 5];
const MEDIA_POOL: [f32; 6] = [0.0, 0.0005, 1.0, 595.0, 1.0e6, f32::NAN];
const MARGIN_POOL: [f32; 6] = [-1.0, 0.0, 2.0, 30.0, 500.0, f32::INFINITY];
const GUTTER_POOL: [f32; 5] = [-0.5, 0.0, 3.0, 50.0, f32::NAN];
const SCALE_POOL: [f32; 8] = [0.0, -2.0, 1.0e-9, 0.5, 1.0, 4.0, 1.0e9, f32::NAN];

fn pool_u32(pool: &[u32], rng: &mut Lcg) -> u32 {
    match pool.get(rng.pick(pool.len() as u32) as usize) {
        Some(v) => *v,
        None => 0,
    }
}

fn pool_u8(pool: &[u8], rng: &mut Lcg) -> u8 {
    match pool.get(rng.pick(pool.len() as u32) as usize) {
        Some(v) => *v,
        None => 1,
    }
}

fn pool_f32(pool: &[f32], rng: &mut Lcg) -> f32 {
    match pool.get(rng.pick(pool.len() as u32) as usize) {
        Some(v) => *v,
        None => 0.0,
    }
}

/// 跑 `iters` 轮极端 fuzz。违例计数为 0 即 J1 的「零崩」。
pub fn fuzz_campaign(iters: usize) -> Result<FuzzReport, FuzzError> {
    if iters == 0 {
        return Err(FuzzError::Budget);
    }
    if iters > MAX_FUZZ_ITERS {
        return Err(FuzzError::Range);
    }
    let mut rng = Lcg::new(0x4302_0000_0000_0007);
    let mut solved = 0usize;
    let mut rejected = 0usize;
    let mut violations = 0usize;
    let mut codes = 0u16;
    let mut seal = FNV_OFFSET;

    let mut round = 0usize;
    while round < iters {
        let spec = LayoutSpec {
            pages: pool_u32(&PAGES_POOL, &mut rng),
            nup: pool_u8(&NUP_POOL, &mut rng),
            media_w: pool_f32(&MEDIA_POOL, &mut rng),
            media_h: pool_f32(&MEDIA_POOL, &mut rng),
            margin: pool_f32(&MARGIN_POOL, &mut rng),
            gutter: pool_f32(&GUTTER_POOL, &mut rng),
            scale: pool_f32(&SCALE_POOL, &mut rng),
        };
        seal = fnv_f32(seal, spec.media_w);
        seal = fnv_f32(seal, spec.scale);
        match solve(&spec) {
            Ok(plan) => {
                solved += 1;
                seal = fnv_u64(seal, plan.digest);
                let eps = maxf(tolerance(spec.media_w), tolerance(spec.media_h));
                let media = PageBox::new(0.0, 0.0, spec.media_w, spec.media_h);
                if plan.boxes.len() as u32 + plan.deferred != spec.pages {
                    violations += 1;
                }
                if plan.digest != digest_of(&spec, &plan.boxes) {
                    violations += 1;
                }
                let mut a = 0usize;
                while a < plan.boxes.len() {
                    if let Some(ba) = plan.boxes.get(a) {
                        if !ba.within(&media, eps) {
                            violations += 1;
                        }
                        let mut b = a + 1;
                        while b < plan.boxes.len() {
                            if let Some(bb) = plan.boxes.get(b) {
                                if ba.intersects(bb, eps) {
                                    violations += 1;
                                }
                            }
                            b += 1;
                        }
                    }
                    a += 1;
                }
                match solve(&spec) {
                    Ok(again) => {
                        if again != plan {
                            violations += 1;
                        }
                    }
                    Err(_) => violations += 1,
                }
            }
            Err(e) => {
                rejected += 1;
                let bit = e.code() - SEG_BASE;
                if bit >= 1 && bit <= 15 {
                    codes |= 1u16 << (bit - 1);
                }
                seal = fnv_u32(seal, e.code());
            }
        }
        round += 1;
    }
    Ok(FuzzReport {
        iters,
        solved,
        rejected,
        violations,
        codes,
        seal,
    })
}

/// 注入核：五类坏规格必须全部落到各自诊断码（不是统一兜底码）。
pub fn injection_core() -> (usize, usize) {
    let base = LayoutSpec {
        pages: 4,
        nup: 4,
        media_w: 595.0,
        media_h: 842.0,
        margin: 20.0,
        gutter: 5.0,
        scale: 1.0,
    };
    let total = 5usize;
    let mut caught = 0usize;

    let mut media = base;
    media.media_w = 0.0;
    if solve(&media).err() == Some(FuzzError::BadMedia) {
        caught += 1;
    }
    let mut gutter = base;
    gutter.gutter = f32::NAN;
    if solve(&gutter).err() == Some(FuzzError::BadGutter) {
        caught += 1;
    }
    let mut nup = base;
    nup.nup = 3;
    if solve(&nup).err() == Some(FuzzError::BadNup) {
        caught += 1;
    }
    let mut room = base;
    room.margin = 400.0;
    if solve(&room).err() == Some(FuzzError::NoRoom) {
        caught += 1;
    }
    let mut scale = base;
    scale.scale = 0.0;
    if solve(&scale).err() == Some(FuzzError::BadScale) {
        caught += 1;
    }
    (caught, total)
}

/// 布局 fuzz 域自检（J1：fuzz ×10^3 零崩 + 极端输入全部显式落码）。
pub fn run_unx50032_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50032");

    // J1-1 千轮 fuzz：违例 0、记账守恒、错误码位图覆盖主要拒绝类。
    let (ok, det) = match fuzz_campaign(1000) {
        Ok(r) => {
            let good = r.iters == 1000
                && r.violations == 0
                && r.solved + r.rejected == r.iters
                && (r.codes & 0x00FF) != 0;
            (
                good,
                format!(
                    "solved={} rej={} viol={} codes={:#06x}",
                    r.solved, r.rejected, r.violations, r.codes
                ),
            )
        }
        Err(e) => (false, format!("campaign={}", e.code())),
    };
    s.add_dyn("50032-J1-fuzz千轮零崩", ok, &det);

    // J1-2 极端页数：0 页给空计划；65535 页只有本张纸的格数，其余显式 deferred。
    let zero = LayoutSpec::new(0, 16, 595.0, 842.0);
    let huge = LayoutSpec::new(MAX_PAGES, 16, 595.0, 842.0);
    let mut maybe = 0usize;
    if let (Ok(p0), Ok(ph)) = (solve(&zero), solve(&huge)) {
        if p0.boxes.is_empty() && p0.deferred == 0 && ph.boxes.len() == 16 {
            maybe += 1;
        }
        if ph.deferred == MAX_PAGES - 16 && ph.capacity == 16 {
            maybe += 1;
        }
    }
    let ok = maybe == 2;
    s.add("50032-J1-极端页数", ok, if ok { "" } else { "极端页数未按账处理" });

    // J1-3 盒必在纸内（A4 595×842、9-up、边距 20、缝 8、满缩）。
    let spec = LayoutSpec {
        pages: 9,
        nup: 9,
        media_w: 595.0,
        media_h: 842.0,
        margin: 20.0,
        gutter: 8.0,
        scale: 1.0,
    };
    let inside = match solve(&spec) {
        Ok(p) => {
            let media = PageBox::new(0.0, 0.0, 595.0, 842.0);
            let mut bad = 0usize;
            let mut i = 0usize;
            while i < p.boxes.len() {
                if let Some(b) = p.boxes.get(i) {
                    if !b.within(&media, tolerance(595.0)) {
                        bad += 1;
                    }
                }
                i += 1;
            }
            (bad == 0 && p.boxes.len() == 9, bad)
        }
        Err(_) => (false, 99),
    };
    let det = format!("bad={}", inside.1);
    s.add_dyn("50032-J1-盒在纸内", inside.0, &det);

    // J1-4 盒互不重叠（9 格 36 对，贴边相邻不算重叠）。
    let overlap = match solve(&spec) {
        Ok(p) => {
            let eps = tolerance(595.0);
            let mut hits = 0usize;
            let mut a = 0usize;
            while a < p.boxes.len() {
                if let Some(ba) = p.boxes.get(a) {
                    let mut b = a + 1;
                    while b < p.boxes.len() {
                        if let Some(bb) = p.boxes.get(b) {
                            if ba.intersects(bb, eps) {
                                hits += 1;
                            }
                        }
                        b += 1;
                    }
                }
                a += 1;
            }
            (hits == 0, hits)
        }
        Err(_) => (false, 99),
    };
    let det = format!("overlap_pairs={}", overlap.1);
    s.add_dyn("50032-J1-盒不重叠", overlap.0, &det);

    // J1-5 页数守恒：boxes + deferred == pages（跨容量边界 5/16/17）。
    let mut conserved = 0usize;
    for pages in [5u32, 16, 17].iter() {
        let sp = LayoutSpec::new(*pages, 16, 595.0, 842.0);
        if let Ok(p) = solve(&sp) {
            if p.boxes.len() as u32 + p.deferred == *pages {
                conserved += 1;
            }
        }
    }
    let ok = conserved == 3;
    let det = format!("conserved={}/3", conserved);
    s.add_dyn("50032-J1-页数守恒", ok, &det);

    // J1-6 求解确定性：同 spec 两次逐位一致（含摘要）。
    let det_ok = match (solve(&spec), solve(&spec)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    s.add("50032-J1-求解确定", det_ok, if det_ok { "" } else { "两次求解不一致" });

    // J1-7 极端缩放：上下越界都钳制、标记 clamped、仍在纸内且摘要可复算。
    let mut scale_ok = 0usize;
    let mut scale_detail = 0.0f32;
    for sc in [1.0e9f32, 1.0e-9].iter() {
        let mut sp = spec;
        sp.scale = *sc;
        if let Ok(p) = solve(&sp) {
            let media = PageBox::new(0.0, 0.0, 595.0, 842.0);
            let mut all_in = true;
            let mut i = 0usize;
            while i < p.boxes.len() {
                if let Some(b) = p.boxes.get(i) {
                    if !b.within(&media, tolerance(595.0)) || b.w <= 0.0 || b.h <= 0.0 {
                        all_in = false;
                    }
                }
                i += 1;
            }
            if p.clamped && all_in && verify_plan(&sp, &p).is_ok() {
                scale_ok += 1;
                if let Some(b) = p.boxes.first() {
                    scale_detail = b.w;
                }
            }
        }
    }
    let ok = scale_ok == 2;
    let det = format!("clamped_ok={}/2 w={:.4}", scale_ok, scale_detail);
    s.add_dyn("50032-J1-缩放钳制", ok, &det);

    // J1-8 非法媒体必拒（0 / NaN / 超上限）。
    let mut bad_media = 0usize;
    for (w, h) in [(0.0f32, 842.0f32), (f32::NAN, 842.0), (595.0, MAX_MEDIA * 2.0)].iter() {
        let sp = LayoutSpec::new(4, 4, *w, *h);
        if solve(&sp).err() == Some(FuzzError::BadMedia) {
            bad_media += 1;
        }
    }
    let ok = bad_media == 3;
    let det = format!("bad_media={}/3", bad_media);
    s.add_dyn("50032-J1-媒体必拒", ok, &det);

    // J1-9 非法网格/边距/缝必拒。
    let mut m = spec;
    m.nup = 5;
    let nup_bad = solve(&m).err() == Some(FuzzError::BadNup);
    m = spec;
    m.margin = -1.0;
    let margin_bad = solve(&m).err() == Some(FuzzError::BadMargin);
    m = spec;
    m.gutter = -0.5;
    let gutter_bad = solve(&m).err() == Some(FuzzError::BadGutter);
    m = spec;
    m.margin = 400.0;
    let room_bad = solve(&m).err() == Some(FuzzError::NoRoom);
    let ok = nup_bad && margin_bad && gutter_bad && room_bad;
    let det = format!(
        "nup={} margin={} gutter={} room={}",
        nup_bad, margin_bad, gutter_bad, room_bad
    );
    s.add_dyn("50032-J1-网格边距必拒", ok, &det);

    // J1-10 页数上限与 fuzz 预算：各自独立码，不给通用兜底。
    let over = LayoutSpec::new(MAX_PAGES + 1, 4, 595.0, 842.0);
    let pages_bad = solve(&over).err() == Some(FuzzError::TooManyPages);
    let budget_bad = fuzz_campaign(0).err() == Some(FuzzError::Budget);
    let range_bad = fuzz_campaign(MAX_FUZZ_ITERS + 1).err() == Some(FuzzError::Range);
    let ok = pages_bad && budget_bad && range_bad;
    let det = format!("pages={} budget={} range={}", pages_bad, budget_bad, range_bad);
    s.add_dyn("50032-J1-页数预算必拒", ok, &det);

    // J1-11 摘要漂移：手工改一位摘要必须被 verify_plan 抓出。
    let drift = match solve(&spec) {
        Ok(mut p) => {
            p.digest ^= 1;
            verify_plan(&spec, &p).err() == Some(FuzzError::DigestDrift)
        }
        Err(_) => false,
    };
    s.add("50032-J1-摘要漂移必抓", drift, if drift { "" } else { "摘要篡改未被发现" });

    // J1-12 五类注入全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50032-J1-注入五类", ok, &det);

    // J1-13 错误码闭集：所有码落在 SEG_BASE+1..=SEG_BASE+10。
    let all = [
        FuzzError::BadMedia,
        FuzzError::BadMargin,
        FuzzError::BadGutter,
        FuzzError::BadNup,
        FuzzError::TooManyPages,
        FuzzError::BadScale,
        FuzzError::NoRoom,
        FuzzError::Budget,
        FuzzError::DigestDrift,
        FuzzError::Range,
    ];
    let mut in_range = 0usize;
    let mut i = 0usize;
    while i < all.len() {
        if let Some(e) = all.get(i) {
            let c = e.code();
            if c > SEG_BASE && c <= SEG_BASE + 10 && !e.message().is_empty() {
                in_range += 1;
            }
        }
        i += 1;
    }
    let ok = in_range == all.len();
    let det = format!("closed={}/{}", in_range, all.len());
    s.add_dyn("50032-J1-码集闭包", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50032_checks_green() {
        let set = run_unx50032_checks();
        assert!(set.all_passed(), "UNX-F50032 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50032_fuzz_thousand_no_violation() {
        match fuzz_campaign(1000) {
            Ok(r) => {
                assert_eq!(r.violations, 0);
                assert_eq!(r.solved + r.rejected, 1000);
                assert_eq!(r.iters, 1000);
            }
            Err(e) => panic!("campaign rejected: {}", e.code()),
        }
    }

    #[test]
    fn unx50032_extreme_scale_contained() {
        let mut sp = LayoutSpec::new(9, 9, 595.0, 842.0);
        sp.margin = 20.0;
        sp.gutter = 8.0;
        sp.scale = 1.0e9;
        match solve(&sp) {
            Ok(p) => {
                assert!(p.clamped);
                let media = PageBox::new(0.0, 0.0, 595.0, 842.0);
                let mut i = 0usize;
                while i < p.boxes.len() {
                    if let Some(b) = p.boxes.get(i) {
                        assert!(b.within(&media, tolerance(595.0)));
                    }
                    i += 1;
                }
                assert!(verify_plan(&sp, &p).is_ok());
            }
            Err(e) => panic!("unexpected {}", e.code()),
        }
    }
}
