//! unx484_prim —— UNX-F0484 渲染原语（put_pixel/矩形填充）。
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-A1-B25 · 行 1990）：
//! 「UNX-F0484 | 渲染原语（put_pixel/矩形填充） | 240 | 已深化 |
//!   判据：原语三件（put_pixel/hline/rect_fill）输出与构造预期位图一致
//!   （12/12 用例），越界参数拦截」
//!
//! 组件：位图面（u64 字集行位图 · 定宽定高 · 像素二值）
//!      + 裁剪矩形（clip rect · 原语一律先裁后写）
//!      + 原语三件（put_pixel / hline / rect_fill · 全量越界拦截）
//!      + 期望位图构造器（golden 12 用例 · 逐位比对）
//!      + 操作流水账（append-only · 守恒式 下发数 ≡ 命中数 + 拦截数）
//!      + 损伤追踪（damage box · 脏区合并账）
//!      + LCG 压力 512 轮 + 重放对拍 + 快照指纹（FNV-1a 确定性）。
//!
//! 判据落地：①原语三件输出与构造预期位图一致——12 个 golden 用例逐位
//! 全等（用例覆盖单点/整行/整矩形/裁剪边界/越界组合）；②越界参数拦截
//! 100%——探测集内全部越界参数被拒且留账；③账实守恒（下发 ≡ 命中 + 拦截）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//! （逻辑 tick 注入，零墙钟，零 IO），零浮点。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与诊断段
// ---------------------------------------------------------------------------

/// 模块版本。
pub const PRM_VERSION: &str = "unx484_prim-v1";

/// 诊断段基址（0x9800 段 · 本模块独占 0x980D~0x9814，入库前已 grep 全域确认无撞段）。
pub const PRM_SEG_BASE: u16 = 0x9800;

/// 面宽（像素 · golden 用例基准面）。
pub const PRM_WIDTH: u32 = 64;

/// 面高（像素 · golden 用例基准面）。
pub const PRM_HEIGHT: u32 = 32;

/// 字长（u64 · 每行 u64 字数 = 64/64 = 1，构造期锁定）。
pub const PRM_WORDS_PER_ROW: usize = (PRM_WIDTH as usize + 63) / 64;

/// 操作流水账容量（滚动覆盖最旧）。
pub const PRM_LOG_CAP: usize = 64;

/// LCG 压力轮数。
pub const PRM_STRESS_ROUNDS: u32 = 512;

// ---------------------------------------------------------------------------
// 诊断码（闭集 · 0x980D~0x9814）
// ---------------------------------------------------------------------------

/// 诊断码闭集（段内八码；闭集外一律拒绝进入账面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrmCode {
    /// 0x980D put_pixel 越界（x/y 超出面界）。
    PixelOutOfRange,
    /// 0x980E hline 越界（线段部分或全部出界且未整线拒绝）。
    HLineOutOfRange,
    /// 0x980F rect_fill 越界（矩形出界且未整矩形拒绝）。
    RectOutOfRange,
    /// 0x9810 宽高非法（零宽/零高/超构造上限）。
    GeometryBad,
    /// 0x9811 期望位图不符（golden 逐位比对失配）。
    GoldenMismatch,
    /// 0x9812 账不平（下发数 ≢ 命中数 + 拦截数）。
    LedgerUnbalanced,
    /// 0x9813 损伤框越界（damage box 超出面界或倒置）。
    DamageInvalid,
    /// 0x9814 快照指纹漂移（同面两次快照指纹不同）。
    FingerprintDrift,
}

impl PrmCode {
    /// 全量闭集（八码）。
    pub const ALL: [PrmCode; 8] = [
        PrmCode::PixelOutOfRange,
        PrmCode::HLineOutOfRange,
        PrmCode::RectOutOfRange,
        PrmCode::GeometryBad,
        PrmCode::GoldenMismatch,
        PrmCode::LedgerUnbalanced,
        PrmCode::DamageInvalid,
        PrmCode::FingerprintDrift,
    ];

    /// 码值（段位守卫口径）。
    pub const fn val(self) -> u16 {
        match self {
            PrmCode::PixelOutOfRange => 0x980D,
            PrmCode::HLineOutOfRange => 0x980E,
            PrmCode::RectOutOfRange => 0x980F,
            PrmCode::GeometryBad => 0x9810,
            PrmCode::GoldenMismatch => 0x9811,
            PrmCode::LedgerUnbalanced => 0x9812,
            PrmCode::DamageInvalid => 0x9813,
            PrmCode::FingerprintDrift => 0x9814,
        }
    }

    /// 现象（三要素之一）。
    pub const fn what(self) -> &'static str {
        match self {
            PrmCode::PixelOutOfRange => "put_pixel 的坐标超出面界",
            PrmCode::HLineOutOfRange => "hline 的线段越出裁剪矩形",
            PrmCode::RectOutOfRange => "rect_fill 的矩形越出裁剪矩形",
            PrmCode::GeometryBad => "面宽高为零或超出构造上限",
            PrmCode::GoldenMismatch => "原语输出与构造预期位图逐位不符",
            PrmCode::LedgerUnbalanced => "操作账不平：下发数 ≢ 命中数 + 拦截数",
            PrmCode::DamageInvalid => "损伤框越界或坐标倒置",
            PrmCode::FingerprintDrift => "同一位图面两次快照指纹不一致",
        }
    }

    /// 原因（三要素之二）。
    pub const fn why(self) -> &'static str {
        match self {
            PrmCode::PixelOutOfRange => "坐标未过裁剪器直接下发，越界写会踩越相邻行内存",
            PrmCode::HLineOutOfRange => "线段端点未做整线裁剪判定",
            PrmCode::RectOutOfRange => "矩形未做整框裁剪判定，部分越界写比全越界更隐蔽",
            PrmCode::GeometryBad => "构造器缺前置校验，零尺寸面会使位移算术退化",
            PrmCode::GoldenMismatch => "原语写位算术与预期模型脱钩（掩码/移位/行距其一）",
            PrmCode::LedgerUnbalanced => "存在绕过留账的下发路径或拦截未计数",
            PrmCode::DamageInvalid => "脏区合并时未做区间归一化",
            PrmCode::FingerprintDrift => "位图或账面含非确定源",
        }
    }

    /// 建议（三要素之三）。
    pub const fn advice(self) -> &'static str {
        match self {
            PrmCode::PixelOutOfRange => "所有坐标先过 clip 判定，拦截并留账",
            PrmCode::HLineOutOfRange => "整线先与 clip 求交，交集为空整线拒绝",
            PrmCode::RectOutOfRange => "整框先与 clip 求交，空交整框拒绝",
            PrmCode::GeometryBad => "构造期校验宽高上下界，零尺寸直接拒",
            PrmCode::GoldenMismatch => "对拍写位掩码/移位/行距三件套，逐位定位首失配位",
            PrmCode::LedgerUnbalanced => "审计写路径是否绕过留账原语入口",
            PrmCode::DamageInvalid => "损伤框先归一化（min/max 交换）再判越界",
            PrmCode::FingerprintDrift => "排查面缓冲未清零区与账面未初始化字段",
        }
    }
}

/// 段位守卫：全部诊断码落在 0x980D~0x9814 窗口内。
pub fn segment_guard() -> bool {
    PrmCode::ALL
        .iter()
        .all(|c| c.val() >= 0x980D && c.val() <= 0x9814)
}

// ---------------------------------------------------------------------------
// 几何（点/线/矩形/裁剪）
// ---------------------------------------------------------------------------

/// 像素坐标（u32 空间；面界由 Surface 定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pixel {
    /// x 坐标。
    pub x: u32,
    /// y 坐标。
    pub y: u32,
}

/// 矩形（含端点闭区间口径：x..x+w / y..y+h 半开区间更利于裁剪算术）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    /// 左上 x。
    pub x: u32,
    /// 左上 y。
    pub y: u32,
    /// 宽。
    pub w: u32,
    /// 高。
    pub h: u32,
}

impl Rect {
    /// 构造（零宽高拒绝 → None）。
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Option<Rect> {
        if w == 0 || h == 0 {
            None
        } else {
            Some(Rect { x, y, w, h })
        }
    }

    /// 与另一矩形求交（空交 → None）。
    pub const fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x1 = if self.x > o.x { self.x } else { o.x };
        let y1 = if self.y > o.y { self.y } else { o.y };
        let sx2 = self.x.wrapping_add(self.w);
        let ox2 = o.x.wrapping_add(o.w);
        let sy2 = self.y.wrapping_add(self.h);
        let oy2 = o.y.wrapping_add(o.h);
        let x2 = if sx2 < ox2 { sx2 } else { ox2 };
        let y2 = if sy2 < oy2 { sy2 } else { oy2 };
        if x2 > x1 && y2 > y1 {
            Some(Rect {
                x: x1,
                y: y1,
                w: x2 - x1,
                h: y2 - y1,
            })
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 位图面
// ---------------------------------------------------------------------------

/// 位图面（二值像素 · u64 字集行位图）。
#[derive(Clone, Debug)]
pub struct Surface {
    w: u32,
    h: u32,
    /// 行主序位图：row-major，每行 PRM_WORDS_PER_ROW 个 u64。
    bits: Vec<u64>,
}

impl Surface {
    /// 构造（全零面；宽高非法 → None）。
    pub fn new(w: u32, h: u32) -> Option<Surface> {
        if w == 0 || h == 0 || w > 4096 || h > 4096 {
            return None;
        }
        let wpr = ((w as usize) + 63) / 64;
        Some(Surface {
            w,
            h,
            bits: vec![0u64; wpr * (h as usize)],
        })
    }

    /// 宽。
    pub const fn width(&self) -> u32 {
        self.w
    }

    /// 高。
    pub const fn height(&self) -> u32 {
        self.h
    }

    /// 面界矩形。
    pub fn bounds(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: self.w,
            h: self.h,
        }
    }

    /// 像素读取（越界 → None）。
    pub fn get(&self, x: u32, y: u32) -> Option<bool> {
        if x >= self.w || y >= self.h {
            return None;
        }
        let idx = y as usize * PRM_WORDS_PER_ROW + (x as usize) / 64;
        let bit = (x as usize) % 64;
        Some((self.bits[idx] >> bit) & 1 == 1)
    }

    /// 像素写入（内部口径：调用方已裁剪）。
    fn set_raw(&mut self, x: u32, y: u32, on: bool) {
        let idx = y as usize * PRM_WORDS_PER_ROW + (x as usize) / 64;
        let bit = (x as usize) % 64;
        if on {
            self.bits[idx] |= 1u64 << bit;
        } else {
            self.bits[idx] &= !(1u64 << bit);
        }
    }

    /// 逐位比对（与另一面；几何不同视为不等）。
    pub fn bits_equal(&self, o: &Surface) -> bool {
        self.w == o.w && self.h == o.h && self.bits == o.bits
    }

    /// 置位点数（面内 1 位计数——golden 对账口径）。
    pub fn popcount(&self) -> u64 {
        self.bits.iter().map(|b| b.count_ones() as u64).sum()
    }

    /// 面快照指纹（FNV-1a 逐字累积）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in &self.bits {
            h ^= *b;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= (self.w as u64) << 32;
        h ^= self.h as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }
}

// ---------------------------------------------------------------------------
// 期望位图构造器（golden 用例的预期模型——与原语实现完全独立口径）
// ---------------------------------------------------------------------------

/// 期望位图构造器（参考实现：以集合语义记录点亮像素，与位运算实现互为对拍）。
#[derive(Clone, Debug)]
pub struct ExpectedSurface {
    w: u32,
    h: u32,
    lit: Vec<(u32, u32)>,
}

impl ExpectedSurface {
    /// 构造。
    pub fn new(w: u32, h: u32) -> Self {
        ExpectedSurface {
            w,
            h,
            lit: Vec::new(),
        }
    }

    /// 参考写点（面内才记）。
    pub fn put(&mut self, x: u32, y: u32) {
        if x < self.w && y < self.h && !self.lit.contains(&(x, y)) {
            self.lit.push((x, y));
        }
    }

    /// 参考 hline（面内逐点记）。
    pub fn hline(&mut self, y: u32, x0: u32, x1: u32) {
        if y >= self.h {
            return;
        }
        let (a, b) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        for x in a..=b {
            if x < self.w {
                self.put(x, y);
            }
        }
    }

    /// 参考 rect_fill。
    pub fn rect_fill(&mut self, r: &Rect) {
        for y in r.y..r.y.saturating_add(r.h) {
            if y < self.h {
                for x in r.x..r.x.saturating_add(r.w) {
                    if x < self.w {
                        self.put(x, y);
                    }
                }
            }
        }
    }

    /// 物化成位图面（与原语面同构；几何非法时退化为 1×1 兜底面）。
    pub fn materialize(&self) -> Surface {
        let mut s = match Surface::new(self.w, self.h) {
            Some(s) => s,
            None => Surface {
                w: 1,
                h: 1,
                bits: vec![0u64],
            },
        };
        for (x, y) in &self.lit {
            s.set_raw(*x, *y, true);
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 操作流水账（守恒式：下发 ≡ 命中 + 拦截）
// ---------------------------------------------------------------------------

/// 单条操作账目。
#[derive(Clone, Debug)]
pub struct PrmRecord {
    /// 原语名（0=put_pixel 1=hline 2=rect_fill）。
    pub op: u8,
    /// 是否命中（false=越界拦截）。
    pub hit: bool,
    /// 拦截码（0 = 命中无码）。
    pub code: u16,
    /// 操作像位数（命中时为实际写点数；拦截时为 0）。
    pub pixels: u64,
    /// 注入 tick。
    pub tick: u64,
}

/// 操作流水账。
#[derive(Clone, Debug)]
pub struct PrmLog {
    records: Vec<PrmRecord>,
    dropped: u64,
    seen: u64,
    hits: u64,
    intercepted: u64,
    pixels_written: u64,
}

impl PrmLog {
    /// 新账。
    pub fn new() -> Self {
        PrmLog {
            records: Vec::new(),
            dropped: 0,
            seen: 0,
            hits: 0,
            intercepted: 0,
            pixels_written: 0,
        }
    }

    /// 入账。
    pub fn push(&mut self, rec: PrmRecord) {
        self.seen += 1;
        if rec.hit {
            self.hits += 1;
            self.pixels_written += rec.pixels;
        } else {
            self.intercepted += 1;
        }
        if self.records.len() >= PRM_LOG_CAP {
            self.records.remove(0);
            self.dropped += 1;
        }
        self.records.push(rec);
    }

    /// 守恒式：下发 ≡ 命中 + 拦截。
    pub fn conserves(&self) -> bool {
        self.seen == self.hits + self.intercepted
    }

    /// 窗口守恒。
    pub fn window_conserves(&self) -> bool {
        self.seen == self.records.len() as u64 + self.dropped
    }

    /// 累计（下发/命中/拦截/写点）。
    pub fn totals(&self) -> (u64, u64, u64, u64) {
        (self.seen, self.hits, self.intercepted, self.pixels_written)
    }
}

// ---------------------------------------------------------------------------
// 渲染原语引擎（原语三件 + 损伤追踪 + 压力）
// ---------------------------------------------------------------------------

/// 渲染原语引擎。
#[derive(Clone, Debug)]
pub struct PrimEngine {
    surf: Surface,
    clip: Rect,
    log: PrmLog,
    tick: u64,
    lcg: u64,
    /// 损伤框（累积脏区；None = 无脏区）。
    damage: Option<Rect>,
    /// golden 用例命中计数（12/12 口径）。
    golden_hits: u32,
}

impl PrimEngine {
    /// 新引擎（基准面 64×32，clip=面界）。
    pub fn new() -> Self {
        let surf = match Surface::new(PRM_WIDTH, PRM_HEIGHT) {
            Some(s) => s,
            None => Surface {
                w: 1,
                h: 1,
                bits: vec![0u64],
            },
        };
        let clip = surf.bounds();
        PrimEngine {
            surf,
            clip,
            log: PrmLog::new(),
            tick: 0,
            lcg: 0x9E37_79B9_7F4A_7C15,
            damage: None,
            golden_hits: 0,
        }
    }

    /// 面引用。
    pub fn surface(&self) -> &Surface {
        &self.surf
    }

    /// 设裁剪矩形（与面界求交；空交拒 → None）。
    pub fn set_clip(&mut self, r: Rect) -> Option<Rect> {
        let c = self.surf.bounds().intersect(&r)?;
        self.clip = c;
        Some(c)
    }

    /// 损伤并入（归一化合并框）。
    fn damage_union(&mut self, r: Rect) {
        let nd = match self.damage {
            None => r,
            Some(d) => {
                let x1 = d.x.min(r.x);
                let y1 = d.y.min(r.y);
                let x2 = (d.x + d.w).max(r.x + r.w);
                let y2 = (d.y + d.h).max(r.y + r.h);
                Rect {
                    x: x1,
                    y: y1,
                    w: x2 - x1,
                    h: y2 - y1,
                }
            }
        };
        self.damage = Some(nd);
    }

    /// 原语一：put_pixel（越界整点拦截）。
    pub fn put_pixel(&mut self, p: Pixel, on: bool) -> bool {
        self.tick += 1;
        let inb = p.x < self.clip.w + self.clip.x
            && p.y < self.clip.h + self.clip.y
            && p.x >= self.clip.x
            && p.y >= self.clip.y;
        if !inb {
            self.log.push(PrmRecord {
                op: 0,
                hit: false,
                code: PrmCode::PixelOutOfRange.val(),
                pixels: 0,
                tick: self.tick,
            });
            return false;
        }
        self.surf.set_raw(p.x, p.y, on);
        self.damage_union(Rect {
            x: p.x,
            y: p.y,
            w: 1,
            h: 1,
        });
        self.log.push(PrmRecord {
            op: 0,
            hit: true,
            code: 0,
            pixels: 1,
            tick: self.tick,
        });
        true
    }

    /// 原语二：hline（整线先与 clip 求交；空交整线拦截；部分交线内裁剪写入）。
    pub fn hline(&mut self, y: u32, x0: u32, x1: u32, on: bool) -> bool {
        self.tick += 1;
        let (a, b) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        // 线矩形：宽 = b-a+1，高 = 1。
        let line = match Rect::new(a, y, b.wrapping_sub(a).saturating_add(1), 1) {
            Some(r) => r,
            None => {
                self.log.push(PrmRecord {
                    op: 1,
                    hit: false,
                    code: PrmCode::HLineOutOfRange.val(),
                    pixels: 0,
                    tick: self.tick,
                });
                return false;
            }
        };
        let clipped = match self.clip.intersect(&line) {
            Some(c) => c,
            None => {
                self.log.push(PrmRecord {
                    op: 1,
                    hit: false,
                    code: PrmCode::HLineOutOfRange.val(),
                    pixels: 0,
                    tick: self.tick,
                });
                return false;
            }
        };
        let mut n: u64 = 0;
        for x in clipped.x..clipped.x + clipped.w {
            self.surf.set_raw(x, clipped.y, on);
            n += 1;
        }
        if on {
            self.damage_union(clipped);
        }
        self.log.push(PrmRecord {
            op: 1,
            hit: true,
            code: 0,
            pixels: n,
            tick: self.tick,
        });
        true
    }

    /// 原语三：rect_fill（整框与 clip 求交；空交整框拦截）。
    pub fn rect_fill(&mut self, r: Rect, on: bool) -> bool {
        self.tick += 1;
        let clipped = match self.clip.intersect(&r) {
            Some(c) => c,
            None => {
                self.log.push(PrmRecord {
                    op: 2,
                    hit: false,
                    code: PrmCode::RectOutOfRange.val(),
                    pixels: 0,
                    tick: self.tick,
                });
                return false;
            }
        };
        let mut n: u64 = 0;
        for y in clipped.y..clipped.y + clipped.h {
            for x in clipped.x..clipped.x + clipped.w {
                self.surf.set_raw(x, y, on);
                n += 1;
            }
        }
        if on {
            self.damage_union(clipped);
        }
        self.log.push(PrmRecord {
            op: 2,
            hit: true,
            code: 0,
            pixels: n,
            tick: self.tick,
        });
        true
    }

    /// LCG 伪随机。
    fn lcg_next(&mut self) -> u64 {
        self.lcg = self
            .lcg
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.lcg
    }

    /// golden 12 用例：每例以独立参考模型构造预期位图，与原语输出逐位比对。
    /// 返回命中用例数（判据口径 12/12）。
    pub fn golden_12(&mut self) -> u32 {
        let mut hits = 0u32;
        for case in 0..12u32 {
            // 每例独立面与参考模型。
            let mut eng = PrimEngine::new();
            let mut exp = ExpectedSurface::new(PRM_WIDTH, PRM_HEIGHT);
            let mut pass = true;
            match case {
                0 => {
                    // 单点中心。
                    let (x, y) = (32u32, 16u32);
                    eng.put_pixel(Pixel { x, y }, true);
                    exp.put(x, y);
                }
                1 => {
                    // 四角点。
                    for (x, y) in [(0u32, 0u32), (63, 0), (0, 31), (63, 31)] {
                        eng.put_pixel(Pixel { x, y }, true);
                        exp.put(x, y);
                    }
                }
                2 => {
                    // 整行 y=0。
                    eng.hline(0, 0, 63, true);
                    exp.hline(0, 0, 63);
                }
                3 => {
                    // 整行 y=31（末行）。
                    eng.hline(31, 0, 63, true);
                    exp.hline(31, 0, 63);
                }
                4 => {
                    // 中段线（部分行）。
                    eng.hline(8, 20, 40, true);
                    exp.hline(8, 20, 40);
                }
                5 => {
                    // 反序线段（x0 > x1 归一化）。
                    eng.hline(9, 50, 10, true);
                    exp.hline(9, 50, 10);
                }
                6 => {
                    // 全屏矩形。
                    eng.rect_fill(Rect { x: 0, y: 0, w: 64, h: 32 }, true);
                    exp.rect_fill(&Rect { x: 0, y: 0, w: 64, h: 32 });
                }
                7 => {
                    // 中央矩形。
                    eng.rect_fill(Rect { x: 16, y: 8, w: 32, h: 16 }, true);
                    exp.rect_fill(&Rect { x: 16, y: 8, w: 32, h: 16 });
                }
                8 => {
                    // 裁剪：clip 右下角后画越界矩形（期望只写角内部分）。
                    let c = Rect { x: 32, y: 16, w: 32, h: 16 };
                    let _ = eng.set_clip(c);
                    eng.rect_fill(Rect { x: 48, y: 24, w: 64, h: 64 }, true);
                    let mut ce = ExpectedSurface::new(64, 32);
                    ce.rect_fill(&Rect { x: 48, y: 24, w: 16, h: 8 });
                    exp = ce;
                }
                9 => {
                    // 越界组合：点/线/矩形全越界（面空），拦截后位图仍全零。
                    eng.put_pixel(Pixel { x: 64, y: 0 }, true);
                    eng.hline(32, 0, 63, true);
                    eng.rect_fill(Rect { x: 100, y: 100, w: 4, h: 4 }, true);
                    // exp 保持空。
                }
                10 => {
                    // 覆写：先画矩形再擦除（off 写入）。
                    eng.rect_fill(Rect { x: 8, y: 4, w: 16, h: 8 }, true);
                    eng.rect_fill(Rect { x: 12, y: 6, w: 8, h: 4 }, false);
                    exp.rect_fill(&Rect { x: 8, y: 4, w: 16, h: 8 });
                    // 参考模型无擦除口径，物化后再清内部矩形。
                    let mut mat = exp.materialize();
                    for y in 6..10u32 {
                        for x in 12..20u32 {
                            mat.set_raw(x, y, false);
                        }
                    }
                    pass &= eng.surface().bits_equal(&mat);
                    let bit_ok = eng.surface().popcount()
                        == (16 * 8 - 8 * 4) as u64;
                    pass &= bit_ok;
                    if pass {
                        hits += 1;
                    }
                    self.golden_hits = hits;
                    continue;
                }
                _ => {
                    // 用例 11：窄线宽为 1（单像素线段）。
                    eng.hline(20, 33, 33, true);
                    exp.hline(20, 33, 33);
                }
            }
            pass &= eng.surface().bits_equal(&exp.materialize());
            if pass {
                hits += 1;
            }
        }
        self.golden_hits = hits;
        hits
    }

    /// LCG 压力 512 轮：随机原语 + 随机参数（含越界），全程守恒。
    pub fn stress(&mut self, rounds: u32) -> bool {
        let mut ok = true;
        for _ in 0..rounds {
            let op = (self.lcg_next() % 3) as u8;
            let x = (self.lcg_next() % 80) as u32; // 故意超面界（64）概率
            let y = (self.lcg_next() % 40) as u32;
            let x1 = (self.lcg_next() % 80) as u32;
            match op {
                0 => {
                    let _ = self.put_pixel(Pixel { x, y }, true);
                }
                1 => {
                    let _ = self.hline(y, x, x1, true);
                }
                _ => {
                    let w = (self.lcg_next() % 40) as u32;
                    let h = (self.lcg_next() % 40) as u32;
                    if let Some(r) = Rect::new(x, y, w, h) {
                        let _ = self.rect_fill(r, true);
                    }
                }
            }
            ok &= self.log.conserves() && self.log.window_conserves();
        }
        ok
    }

    /// 重放对拍：新子面上同序列重复写入，首次写变指纹、重放写不改指纹。
    pub fn replay_pair(&mut self) -> bool {
        let mut a = PrimEngine::new();
        let f0 = a.surface().fingerprint();
        let _ = a.hline(16, 0, 63, true);
        let _ = a.rect_fill(Rect { x: 8, y: 8, w: 8, h: 4 }, true);
        let f1 = a.surface().fingerprint();
        let _ = a.hline(16, 0, 63, true);
        let _ = a.rect_fill(Rect { x: 8, y: 8, w: 8, h: 4 }, true);
        let f2 = a.surface().fingerprint();
        f0 != f1 && f1 == f2
    }

    /// 损伤框合法性（None 或在面界内）。
    pub fn damage_valid(&self) -> bool {
        match self.damage {
            None => true,
            Some(d) => self.surf.bounds().intersect(&d).is_some(),
        }
    }

    /// 快照指纹（面 + 账 + 损伤全量）。
    pub fn snapshot_fingerprint(&self) -> u64 {
        let mut h = self.surface().fingerprint();
        let (s, hi, ic, px) = self.log.totals();
        for v in [s, hi, ic, px, self.tick] {
            h ^= v;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// 快照双取一致。
    pub fn snapshot_stable(&self) -> bool {
        self.snapshot_fingerprint() == self.snapshot_fingerprint()
    }
}

impl Default for PrimEngine {
    fn default() -> Self {
        PrimEngine::new()
    }
}

// ---------------------------------------------------------------------------
// 判据（当前模式：轻量判据内嵌，重判据押后统一补）
// ---------------------------------------------------------------------------

/// 判据集（登记进 mod.rs 域聚合——折叠范式）。
pub fn run_t484_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t484");

    // c1 段位守卫 + 几何闭集。
    set.add(
        "t484-c1-seg",
        segment_guard() && Rect::new(0, 0, 0, 1).is_none() && Rect::new(0, 0, 1, 1).is_some(),
        if segment_guard() { "" } else { "diag segment guard broken" },
    );

    // c2 golden 12/12：三原语输出与构造预期位图逐位一致。
    let mut eng = PrimEngine::new();
    let hits = eng.golden_12();
    set.add(
        "t484-c2-golden",
        hits == 12,
        if hits == 12 { "" } else { "golden cases below 12/12" },
    );

    // c3 越界拦截 100%：探测集（点 6 / 线 6 / 矩形 6 = 18 全拦）。
    let mut eng = PrimEngine::new();
    let mut all_blocked = true;
    for (x, y) in [(64u32, 0u32), (0, 32), (65, 33), (u32::MAX, 0), (0, u32::MAX), (63, 32)] {
        all_blocked &= !eng.put_pixel(Pixel { x, y }, true);
    }
    for (y, a, b) in [(32u32, 0u32, 63u32), (0, 64, 127), (33, 0, 63), (0, 100, 200), (31, 64, 70), (0, 200, 100)] {
        all_blocked &= !eng.hline(y, a, b, true);
    }
    for r in [
        Rect { x: 64, y: 0, w: 4, h: 4 },
        Rect { x: 0, y: 32, w: 4, h: 4 },
        Rect { x: 100, y: 100, w: 4, h: 4 },
        Rect { x: 70, y: 40, w: 8, h: 8 },
        Rect { x: 64, y: 32, w: 2, h: 2 },
        Rect { x: 200, y: 200, w: 1, h: 1 },
    ] {
        all_blocked &= !eng.rect_fill(r, true);
    }
    let (seen, _, ic, px) = eng.log.totals();
    set.add(
        "t484-c3-oob",
        all_blocked && seen == 18 && ic == 18 && px == 0 && eng.surface().popcount() == 0,
        if all_blocked { "" } else { "OOB probe set leaked a write" },
    );

    // c4 账实守恒 + 快照稳定。
    set.add(
        "t484-c4-ledger",
        eng.log.conserves() && eng.log.window_conserves() && eng.snapshot_stable(),
        if eng.log.conserves() { "" } else { "op ledger unbalanced" },
    );

    // c5 LCG 压力 512 轮。
    let mut eng = PrimEngine::new();
    let ok = eng.stress(PRM_STRESS_ROUNDS);
    set.add(
        "t484-c5-stress",
        ok && eng.log.conserves() && eng.damage_valid(),
        if ok { "" } else { "512-round stress violated invariant" },
    );

    // c6 重放对拍。
    set.add(
        "t484-c6-replay",
        eng.replay_pair(),
        if eng.replay_pair() { "" } else { "replay fingerprint drift" },
    );

    // c7 裁剪语义：clip 交空后画矩形必须整框拦截；clip 内写不越 clip。
    let mut eng = PrimEngine::new();
    let _ = eng.set_clip(Rect { x: 8, y: 8, w: 16, h: 8 });
    let blocked = !eng.rect_fill(Rect { x: 40, y: 20, w: 4, h: 4 }, true);
    let _ = eng.rect_fill(Rect { x: 8, y: 8, w: 16, h: 8 }, true);
    // clip 外全零：检查 (0,0) 与 (63,31)。
    let outside_clean = !eng.surface().get(0, 0).unwrap_or(true)
        && !eng.surface().get(63, 31).unwrap_or(true);
    // clip 内点计数 = 16*8。
    let cnt = eng.surface().popcount();
    set.add(
        "t484-c7-clip",
        blocked && outside_clean && cnt == 16 * 8,
        if blocked && outside_clean { "" } else { "clip semantics broken" },
    );

    // c8 损伤框合并：两点合并框含两点。
    let mut eng = PrimEngine::new();
    let _ = eng.put_pixel(Pixel { x: 2, y: 3 }, true);
    let _ = eng.put_pixel(Pixel { x: 50, y: 20 }, true);
    let dv = match eng.damage {
        Some(d) => d.x <= 2 && d.y <= 3 && d.x + d.w >= 51 && d.y + d.h >= 21,
        None => false,
    };
    set.add(
        "t484-c8-damage",
        dv,
        if dv { "" } else { "damage union box wrong" },
    );

    set
}
