//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0488 · 渲染参数越界拦截 · 0x75C00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批5-K）：
//!   越界样本 12/12（坐标/宽高/色索引/字形索引四类）检出，拦截计数
//!   账齐（生产 0），负数与回绕（下溢回绕成大数）双口径必抓
//! 设计要点：
//!   1. 四类参数闸：坐标（x,y）/宽高（w,h）/色索引/字形索引全部先判
//!      后写；帧面 640×480 冻结口径，色闭集 256、字形闭集 128；
//!   2. 双口径分闸：有符号口径（i32 负数显性拒 NegativeParam）与无
//!      符号回绕口径（u32 ≥ 0x8000_0000 判为下溢回绕像 WrapParam）
//!      两套入参两种闸，一张 12 样本冻结表对账；
//!   3. 摆位闸用 checked_add：x+w > 640 或 y+h > 480 显性拒，加法
//!      溢出同样拒（回绕无处遁形）；零尺寸（w/h = 0）单独设闸；
//!   4. 拦截计数账：生产拦截 / 注入拦截 / 总账三本，守恒式
//!      总尝试 = 总拦截 + 总放行 逐次复算，生产拦截恒 0；
//!   5. 12 样本冻结表：四类各 3 枚（越界 1 + 回绕 1 + 负数 1），
//!      期望诊断码一并冻结，跑表对账 12/12 一枚不漏；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75C00，AI-VX7-1104 批5-K 独占槽）。
pub const SEG_BASE: u32 = 0x75C00;

// ── 冻结口径 ────────────────────────────────────────────────────
/// 帧面宽（640，VGA 图形口径）。
pub const FB_W: u32 = 640;
/// 帧面高（480，VGA 图形口径）。
pub const FB_H: u32 = 480;
/// 色索引闭集（256 色，索引 0~255）。
pub const COLOR_SET: u32 = 256;
/// 字形索引闭集（128 枚）。
pub const GLYPH_SET: u32 = 128;
/// 有符号/无符号口径分界：u32 ≥ 0x8000_0000 判为负数下溢回绕像。
pub const NEG_SIGN_BIT: u32 = 0x8000_0000;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoundCode {
    /// 坐标越界（x ≥ 640 或 y ≥ 480）
    CoordOver,
    /// 摆位越界（x+w 超 640 或 y+h 超 480）
    DimOver,
    /// 零尺寸（w 或 h 为 0）
    DimZero,
    /// 色索引越闭集（≥256）
    ColorOver,
    /// 字形索引越闭集（≥128）
    GlyphOver,
    /// 负数入参（有符号口径显性拒）
    NegativeParam,
    /// 下溢回绕（u32 ≥ 0x8000_0000 的回绕像）
    WrapParam,
    /// 拦截计数账守恒破坏
    CounterBreach,
    /// 注入漏抓
    InjectLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl BoundCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BoundCode::CoordOver => "坐标越界：x≥640 或 y≥480，建议以 FB_W/FB_H 为界先判后写",
            BoundCode::DimOver => "摆位越界：x+w 或 y+h 超出帧面，建议回退起点或裁剪尺寸",
            BoundCode::DimZero => "零尺寸：w/h 为 0 不构成面，建议给最小 1×1 或显性拒绝",
            BoundCode::ColorOver => "色索引越闭集：仅 0~255，建议对调色板外索引先行过滤",
            BoundCode::GlyphOver => "字形索引越闭集：仅 0~127，建议对越界字形先行过滤",
            BoundCode::NegativeParam => "负数入参：有符号口径不允许负值，建议先判负再折算",
            BoundCode::WrapParam => "下溢回绕：u32 ≥0x8000_0000 为回绕像，建议上游改用有符号口径",
            BoundCode::CounterBreach => "拦截计数账守恒破坏：总尝试≠总拦截+总放行，建议逐次记账即席复算",
            BoundCode::InjectLeaked => "注入漏抓：违约样本未拦截，建议对齐冻结样本表逐枚复核",
            BoundCode::NotReproducible => "双跑不一致：复现红线，建议固定种子后重跑两遍对账",
        }
    }
    pub const ALL: [BoundCode; 10] = [
        BoundCode::CoordOver,
        BoundCode::DimOver,
        BoundCode::DimZero,
        BoundCode::ColorOver,
        BoundCode::GlyphOver,
        BoundCode::NegativeParam,
        BoundCode::WrapParam,
        BoundCode::CounterBreach,
        BoundCode::InjectLeaked,
        BoundCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回；段外码显性报未知）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BoundCode::ALL.len() {
        if let Some(c) = BoundCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75C00 段闭集内"
}

// ── 域模型：渲染参数（无符号口径） ──────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderParams {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub color: u32,
    pub glyph: u32,
}

impl RenderParams {
    /// 基准合法参数（0,0 起 64×32，色 15，字形 65）。
    pub fn base() -> RenderParams {
        RenderParams { x: 0, y: 0, w: 64, h: 32, color: 15, glyph: 65 }
    }
    pub fn with_x(mut self, x: u32) -> RenderParams {
        self.x = x;
        self
    }
    pub fn with_y(mut self, y: u32) -> RenderParams {
        self.y = y;
        self
    }
    pub fn with_w(mut self, w: u32) -> RenderParams {
        self.w = w;
        self
    }
    pub fn with_h(mut self, h: u32) -> RenderParams {
        self.h = h;
        self
    }
    pub fn with_color(mut self, color: u32) -> RenderParams {
        self.color = color;
        self
    }
    pub fn with_glyph(mut self, glyph: u32) -> RenderParams {
        self.glyph = glyph;
        self
    }

    /// 无符号口径闸：回绕 → 零尺寸 → 坐标 → 摆位 → 色 → 字形。
    /// 闸序冻结：回绕闸最先（回绕像必须按回绕口径报，不落到普通越界）。
    pub fn validate_wire(&self) -> Result<(), u32> {
        if self.x >= NEG_SIGN_BIT
            || self.y >= NEG_SIGN_BIT
            || self.w >= NEG_SIGN_BIT
            || self.h >= NEG_SIGN_BIT
            || self.color >= NEG_SIGN_BIT
            || self.glyph >= NEG_SIGN_BIT
        {
            return Err(BoundCode::WrapParam.val());
        }
        if self.w == 0 || self.h == 0 {
            return Err(BoundCode::DimZero.val());
        }
        if self.x >= FB_W || self.y >= FB_H {
            return Err(BoundCode::CoordOver.val());
        }
        match self.x.checked_add(self.w) {
            Some(sum) => {
                if sum > FB_W {
                    return Err(BoundCode::DimOver.val());
                }
            }
            None => return Err(BoundCode::DimOver.val()),
        }
        match self.y.checked_add(self.h) {
            Some(sum) => {
                if sum > FB_H {
                    return Err(BoundCode::DimOver.val());
                }
            }
            None => return Err(BoundCode::DimOver.val()),
        }
        if self.color >= COLOR_SET {
            return Err(BoundCode::ColorOver.val());
        }
        if self.glyph >= GLYPH_SET {
            return Err(BoundCode::GlyphOver.val());
        }
        Ok(())
    }
}

// ── 域模型：渲染参数（有符号口径） ──────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignedParams {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub color: i32,
    pub glyph: i32,
}

impl SignedParams {
    /// 基准合法参数（与无符号基准同值）。
    pub fn base() -> SignedParams {
        SignedParams { x: 0, y: 0, w: 64, h: 32, color: 15, glyph: 65 }
    }
    pub fn with_x(mut self, x: i32) -> SignedParams {
        self.x = x;
        self
    }
    pub fn with_y(mut self, y: i32) -> SignedParams {
        self.y = y;
        self
    }
    pub fn with_w(mut self, w: i32) -> SignedParams {
        self.w = w;
        self
    }
    pub fn with_h(mut self, h: i32) -> SignedParams {
        self.h = h;
        self
    }
    pub fn with_color(mut self, color: i32) -> SignedParams {
        self.color = color;
        self
    }
    pub fn with_glyph(mut self, glyph: i32) -> SignedParams {
        self.glyph = glyph;
        self
    }

    /// 有符号口径闸：任一字段为负先显性拒（NegativeParam），
    /// 随后折算无符号口径复闸（边界闸复用同一套闭集）。
    pub fn validate_signed(&self) -> Result<(), u32> {
        if self.x < 0 || self.y < 0 || self.w < 0 || self.h < 0 || self.color < 0 || self.glyph < 0 {
            return Err(BoundCode::NegativeParam.val());
        }
        let wire = RenderParams {
            x: self.x as u32,
            y: self.y as u32,
            w: self.w as u32,
            h: self.h as u32,
            color: self.color as u32,
            glyph: self.glyph as u32,
        };
        wire.validate_wire()
    }
}

// ── 拦截计数账（生产 / 注入两本，守恒式逐次复算） ────────────────
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GateLedger {
    /// 生产流尝试次数（合法渲染请求）。
    pub prod_tries: u32,
    /// 生产流拦截数（正常流应恒 0）。
    pub prod_intercepts: u32,
    /// 注入尝试次数（人为构造的违约样本）。
    pub inj_tries: u32,
    /// 注入拦截数（= 注入数才叫账齐）。
    pub inj_intercepts: u32,
}

impl GateLedger {
    pub fn new() -> GateLedger {
        GateLedger::default()
    }

    /// 记一笔生产流尝试（rejected = 该次被闸拦下）。
    pub fn record_prod(&mut self, rejected: bool) {
        self.prod_tries = self.prod_tries.saturating_add(1);
        if rejected {
            self.prod_intercepts = self.prod_intercepts.saturating_add(1);
        }
    }

    /// 记一笔注入尝试。
    pub fn record_inj(&mut self, rejected: bool) {
        self.inj_tries = self.inj_tries.saturating_add(1);
        if rejected {
            self.inj_intercepts = self.inj_intercepts.saturating_add(1);
        }
    }

    /// 守恒式：总尝试 = 总拦截 + 总放行（两通道各自成立 + 合账成立）；
    /// 拦截数不得超过尝试数（账面越界即破坏）。
    pub fn conservation_ok(&self) -> bool {
        if self.prod_intercepts > self.prod_tries || self.inj_intercepts > self.inj_tries {
            return false;
        }
        let prod_pass = self.prod_tries.saturating_sub(self.prod_intercepts);
        let inj_pass = self.inj_tries.saturating_sub(self.inj_intercepts);
        let total_try = self.prod_tries.saturating_add(self.inj_tries);
        let total_hit = self.prod_intercepts.saturating_add(self.inj_intercepts);
        let total_pass = prod_pass.saturating_add(inj_pass);
        total_try == total_hit.saturating_add(total_pass)
    }

    /// 账齐：守恒成立 + 生产拦截为 0 + 注入拦截 = 注入尝试。
    pub fn all_tallied(&self) -> bool {
        self.conservation_ok() && self.prod_intercepts == 0 && self.inj_intercepts == self.inj_tries
    }
}

// ── 12 样本冻结表（四类 × 越界/回绕/负数三口径） ─────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamCat {
    /// 坐标类（x/y）
    Coord,
    /// 宽高类（w/h 及摆位）
    Dim,
    /// 色索引类
    Color,
    /// 字形索引类
    Glyph,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleKind {
    /// 普通越界（无符号口径超闭集/摆位超帧面）
    Over,
    /// 下溢回绕（u32 大数像）
    Wrap,
    /// 负数（有符号口径）
    Neg,
}

#[derive(Clone, Debug)]
pub struct BoundSample {
    pub id: u32,
    pub cat: ParamCat,
    pub kind: SampleKind,
    /// 无符号口径入参（Neg 样本此处填回绕像，仅作账面对照）。
    pub wire: RenderParams,
    /// 有符号口径入参（仅 Neg 样本非空）。
    pub signed: Option<SignedParams>,
    /// 期望诊断码（冻结）。
    pub expect: u32,
}

/// 跑一枚样本：按口径走闸，返回（是否拦下、实际码）。
pub fn run_sample(s: &BoundSample) -> (bool, Option<u32>) {
    let r = match s.signed {
        Some(sp) => sp.validate_signed(),
        None => s.wire.validate_wire(),
    };
    match r {
        Err(e) => (e == s.expect, Some(e)),
        Ok(()) => (false, None),
    }
}

/// 12 样本冻结表（期望码与闸序一并冻结）。
pub fn sample_table() -> Vec<BoundSample> {
    let mut v: Vec<BoundSample> = Vec::new();
    // 坐标类 3 枚：越界 / 回绕 / 负数。
    v.push(BoundSample {
        id: 1,
        cat: ParamCat::Coord,
        kind: SampleKind::Over,
        wire: RenderParams::base().with_x(FB_W),
        signed: None,
        expect: BoundCode::CoordOver.val(),
    });
    v.push(BoundSample {
        id: 2,
        cat: ParamCat::Coord,
        kind: SampleKind::Wrap,
        wire: RenderParams::base().with_x(u32::MAX),
        signed: None,
        expect: BoundCode::WrapParam.val(),
    });
    v.push(BoundSample {
        id: 3,
        cat: ParamCat::Coord,
        kind: SampleKind::Neg,
        wire: RenderParams::base().with_y(u32::MAX),
        signed: Some(SignedParams::base().with_y(-1)),
        expect: BoundCode::NegativeParam.val(),
    });
    // 宽高类 3 枚：回绕 / 摆位越界 / 负数。
    v.push(BoundSample {
        id: 4,
        cat: ParamCat::Dim,
        kind: SampleKind::Wrap,
        wire: RenderParams::base().with_w(u32::MAX),
        signed: None,
        expect: BoundCode::WrapParam.val(),
    });
    v.push(BoundSample {
        id: 5,
        cat: ParamCat::Dim,
        kind: SampleKind::Over,
        wire: RenderParams::base().with_x(600).with_w(41),
        signed: None,
        expect: BoundCode::DimOver.val(),
    });
    v.push(BoundSample {
        id: 6,
        cat: ParamCat::Dim,
        kind: SampleKind::Neg,
        wire: RenderParams::base().with_h(u32::MAX),
        signed: Some(SignedParams::base().with_h(-1)),
        expect: BoundCode::NegativeParam.val(),
    });
    // 色索引类 3 枚：越界 / 回绕 / 负数。
    v.push(BoundSample {
        id: 7,
        cat: ParamCat::Color,
        kind: SampleKind::Over,
        wire: RenderParams::base().with_color(COLOR_SET),
        signed: None,
        expect: BoundCode::ColorOver.val(),
    });
    v.push(BoundSample {
        id: 8,
        cat: ParamCat::Color,
        kind: SampleKind::Wrap,
        wire: RenderParams::base().with_color(u32::MAX),
        signed: None,
        expect: BoundCode::WrapParam.val(),
    });
    v.push(BoundSample {
        id: 9,
        cat: ParamCat::Color,
        kind: SampleKind::Neg,
        wire: RenderParams::base().with_color(u32::MAX),
        signed: Some(SignedParams::base().with_color(-1)),
        expect: BoundCode::NegativeParam.val(),
    });
    // 字形索引类 3 枚：越界 / 回绕 / 负数。
    v.push(BoundSample {
        id: 10,
        cat: ParamCat::Glyph,
        kind: SampleKind::Over,
        wire: RenderParams::base().with_glyph(GLYPH_SET),
        signed: None,
        expect: BoundCode::GlyphOver.val(),
    });
    v.push(BoundSample {
        id: 11,
        cat: ParamCat::Glyph,
        kind: SampleKind::Wrap,
        wire: RenderParams::base().with_glyph(u32::MAX),
        signed: None,
        expect: BoundCode::WrapParam.val(),
    });
    v.push(BoundSample {
        id: 12,
        cat: ParamCat::Glyph,
        kind: SampleKind::Neg,
        wire: RenderParams::base().with_glyph(u32::MAX),
        signed: Some(SignedParams::base().with_glyph(-1)),
        expect: BoundCode::NegativeParam.val(),
    });
    v
}

/// 按类目对账：返回（尝试数，拦截数）。
pub fn count_by_cat(cat: &ParamCat) -> (u32, u32) {
    let table = sample_table();
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut i = 0;
    while i < table.len() {
        if let Some(s) = table.get(i) {
            if s.cat == *cat {
                let (hit, _) = run_sample(s);
                tries = tries.saturating_add(1);
                if hit {
                    caught = caught.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    (tries, caught)
}

/// 按口径对账：返回（尝试数，拦截数）。
pub fn count_by_kind(kind: &SampleKind) -> (u32, u32) {
    let table = sample_table();
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut i = 0;
    while i < table.len() {
        if let Some(s) = table.get(i) {
            if s.kind == *kind {
                let (hit, _) = run_sample(s);
                tries = tries.saturating_add(1);
                if hit {
                    caught = caught.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    (tries, caught)
}

/// 全表 12/12 检出判据谓词。
fn samples_all_caught_ok() -> bool {
    let table = sample_table();
    if table.len() != 12 {
        return false;
    }
    let mut i = 0;
    while i < table.len() {
        if let Some(s) = table.get(i) {
            let (hit, _) = run_sample(s);
            if !hit {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── LCG（种子写死 = SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 生产流合法参数生成（LCG 抖动，全程落在帧面内）。
pub fn legal_param(rng: &mut Lcg) -> RenderParams {
    let x = rng.below((FB_W - 128) as u64) as u32;
    let y = rng.below((FB_H - 64) as u64) as u32;
    RenderParams::base().with_x(x).with_y(y)
}

// ── 黄金全链：生产 8 连放行（生产拦截 0）→ 12 样本全拦 → 账齐
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = GateLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut legal_ok = 0u32;
    let mut r = 0;
    while r < 8 {
        let p = legal_param(&mut rng);
        let rejected = p.validate_wire().is_err();
        led.record_prod(rejected);
        if !rejected {
            legal_ok = legal_ok.saturating_add(1);
        }
        r += 1;
    }
    // 注入面：12 样本冻结表全拦。
    let table = sample_table();
    let mut inj_caught = 0u32;
    let mut i = 0;
    while i < table.len() {
        if let Some(s) = table.get(i) {
            let (hit, _) = run_sample(s);
            led.record_inj(hit);
            if hit {
                inj_caught = inj_caught.saturating_add(1);
            }
        }
        i += 1;
    }
    if !led.conservation_ok() {
        return Err(BoundCode::CounterBreach.val());
    }
    if led.prod_intercepts != 0 {
        return Err(BoundCode::InjectLeaked.val());
    }
    if inj_caught != 12 {
        return Err(BoundCode::InjectLeaked.val());
    }
    Ok((legal_ok, inj_caught, led.prod_intercepts))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（生产 8 放行 + 注入 12 全拦 + 生产拦截 0）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((8, 12, 0)) => Ok(()),
        _ => Err(BoundCode::NotReproducible.val()),
    }
}

/// 序 B：摆位边界（恰好贴边合法，多出 1 像素即拒）。
pub fn probe_seq_b() -> Result<(), u32> {
    let fit = RenderParams::base().with_x(FB_W - 64).with_y(FB_H - 32).validate_wire();
    let one_over = RenderParams::base().with_x(FB_W - 63).with_y(FB_H - 32).validate_wire();
    match (fit, one_over) {
        (Ok(()), Err(e)) if e == BoundCode::DimOver.val() => Ok(()),
        _ => Err(BoundCode::NotReproducible.val()),
    }
}

/// 序 C：零尺寸闸与色闭集边界（255 合法 / 256 拒）。
pub fn probe_seq_c() -> Result<(), u32> {
    let zero = RenderParams::base().with_w(0).validate_wire();
    let edge_ok = RenderParams::base().with_color(COLOR_SET - 1).validate_wire();
    let edge_over = RenderParams::base().with_color(COLOR_SET).validate_wire();
    match (zero, edge_ok, edge_over) {
        (Err(e1), Ok(()), Err(e2)) if e1 == BoundCode::DimZero.val() && e2 == BoundCode::ColorOver.val() => Ok(()),
        _ => Err(BoundCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 12 样本） ─────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<GateLedger, u32> {
    let mut led = GateLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        // 生产面：每轮一枚合法请求放行（生产拦截保持 0）。
        let p = legal_param(&mut rng);
        led.record_prod(p.validate_wire().is_err());
        // 注入面：12 样本逐枚过闸。
        let table = sample_table();
        let mut i = 0;
        while i < table.len() {
            if let Some(s) = table.get(i) {
                let (hit, _) = run_sample(s);
                led.record_inj(hit);
            }
            i += 1;
        }
        r += 1;
    }
    if !led.conservation_ok() {
        return Err(BoundCode::CounterBreach.val());
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BoundCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在 0x75000~0x781C0 的 0x40 对齐窗口） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < BoundCode::ALL.len() {
        if let Some(c) = BoundCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 诊断码闭集对账（val/detail 查表一致 + 段外码报未知） ────────
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < BoundCode::ALL.len() {
        if let Some(c) = BoundCode::ALL.get(i) {
            if detail(c.val()) != c.detail() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── LCG 确定性（同种子两串逐位一致） ────────────────────────────
fn lcg_deterministic_ok() -> bool {
    let mut x = Lcg::new(SEG_BASE as u64);
    let mut y = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < 16 {
        if x.next() != y.next() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：≥12 条 add_dyn） ──────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 口径冻结。
    v.push((
        "J1-口径冻结",
        FB_W == 640 && FB_H == 480 && COLOR_SET == 256 && GLYPH_SET == 128 && NEG_SIGN_BIT == 0x8000_0000,
        String::from("640×480 / 色256 / 字形128 / 符号位分界"),
    ));
    // 2. 合法参数放行（生产拦截 0）。
    let g = golden_run();
    v.push((
        "J1-黄金全链生产拦截0",
        match &g {
            Ok((legal, inj, prod)) => *legal == 8 && *inj == 12 && *prod == 0,
            Err(_) => false,
        },
        match &g {
            Ok((legal, inj, prod)) => format!("legal={} inj={} prod拦截={}", legal, inj, prod),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 3. 越界样本 12/12。
    v.push((
        "J1-越界样本12/12",
        samples_all_caught_ok(),
        String::from("四类冻结样本表逐枚全拦"),
    ));
    // 4. 坐标类 3/3。
    let (ct, cc) = count_by_cat(&ParamCat::Coord);
    v.push((
        "J1-坐标类3/3",
        ct == 3 && cc == 3,
        format!("tries={} caught={}", ct, cc),
    ));
    // 5. 宽高类 3/3。
    let (dt, dc) = count_by_cat(&ParamCat::Dim);
    v.push((
        "J1-宽高类3/3",
        dt == 3 && dc == 3,
        format!("tries={} caught={}", dt, dc),
    ));
    // 6. 色索引类 3/3。
    let (ot, oc) = count_by_cat(&ParamCat::Color);
    v.push((
        "J1-色索引类3/3",
        ot == 3 && oc == 3,
        format!("tries={} caught={}", ot, oc),
    ));
    // 7. 字形索引类 3/3。
    let (gt, gc) = count_by_cat(&ParamCat::Glyph);
    v.push((
        "J1-字形类3/3",
        gt == 3 && gc == 3,
        format!("tries={} caught={}", gt, gc),
    ));
    // 8. 负数口径必抓。
    let (nt, nc) = count_by_kind(&SampleKind::Neg);
    v.push((
        "J1-负数口径必抓",
        nt == 4 && nc == 4,
        format!("neg tries={} caught={}", nt, nc),
    ));
    // 9. 回绕口径必抓。
    let (wt, wc) = count_by_kind(&SampleKind::Wrap);
    v.push((
        "J1-回绕口径必抓",
        wt == 4 && wc == 4,
        format!("wrap tries={} caught={}", wt, wc),
    ));
    // 10. 注入 10 轮账齐（注入拦截=注入数）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-注入10轮账齐",
        match &inj {
            Ok(l) => l.all_tallied() && l.inj_intercepts == JUDGE_ROUNDS * 12 && l.prod_tries == JUDGE_ROUNDS,
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!(
                "inj={}/{} prod={}/{}",
                l.inj_intercepts, l.inj_tries, l.prod_intercepts, l.prod_tries
            ),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 11. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b贴边闸={} c零尺寸+色边界={}", a, b, c),
    ));
    // 12. 诊断码闭集对账。
    v.push((
        "J1-诊断码闭集对账",
        code_closure_ok(),
        format!("{} 码全查表通过", BoundCode::ALL.len()),
    ));
    // 13. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 14. LCG 确定性。
    v.push((
        "J2-LCG确定性",
        lcg_deterministic_ok(),
        String::from("同种子 16 步逐位一致"),
    ));
    // 15. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75C00 窗口 0x40 对齐区间三闸"),
    ));
    v
}

pub fn run_unx0488_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0488 渲染参数越界拦截");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_run_green() {
        match golden_run() {
            Ok((legal, inj, prod)) => {
                assert_eq!(legal, 8);
                assert_eq!(inj, 12);
                assert_eq!(prod, 0);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn frozen_consts() {
        assert_eq!(FB_W, 640);
        assert_eq!(FB_H, 480);
        assert_eq!(COLOR_SET, 256);
        assert_eq!(GLYPH_SET, 128);
        assert_eq!(NEG_SIGN_BIT, 0x8000_0000);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn sample_table_12_of_12() {
        assert_eq!(sample_table().len(), 12);
        assert!(samples_all_caught_ok());
        let (a, b) = count_by_cat(&ParamCat::Coord);
        assert_eq!((a, b), (3, 3));
        let (a, b) = count_by_cat(&ParamCat::Dim);
        assert_eq!((a, b), (3, 3));
        let (a, b) = count_by_cat(&ParamCat::Color);
        assert_eq!((a, b), (3, 3));
        let (a, b) = count_by_cat(&ParamCat::Glyph);
        assert_eq!((a, b), (3, 3));
    }

    #[test]
    fn negative_caliber_caught() {
        match SignedParams::base().with_x(-1).validate_signed() {
            Err(e) => assert_eq!(e, BoundCode::NegativeParam.val()),
            Ok(()) => assert!(false, "负 x 未拒"),
        }
        match SignedParams::base().with_h(-1).validate_signed() {
            Err(e) => assert_eq!(e, BoundCode::NegativeParam.val()),
            Ok(()) => assert!(false, "负 h 未拒"),
        }
        match SignedParams::base().with_glyph(-1).validate_signed() {
            Err(e) => assert_eq!(e, BoundCode::NegativeParam.val()),
            Ok(()) => assert!(false, "负字形未拒"),
        }
    }

    #[test]
    fn wrap_caliber_caught() {
        match RenderParams::base().with_x(u32::MAX).validate_wire() {
            Err(e) => assert_eq!(e, BoundCode::WrapParam.val()),
            Ok(()) => assert!(false, "回绕 x 未拒"),
        }
        match RenderParams::base().with_color(NEG_SIGN_BIT).validate_wire() {
            Err(e) => assert_eq!(e, BoundCode::WrapParam.val()),
            Ok(()) => assert!(false, "回绕色未拒"),
        }
    }

    #[test]
    fn placement_boundary() {
        // 恰好贴边：合法。
        assert!(RenderParams::base().with_x(FB_W - 64).with_y(FB_H - 32).validate_wire().is_ok());
        // 多出 1 像素：摆位越界。
        match RenderParams::base().with_x(FB_W - 63).validate_wire() {
            Err(e) => assert_eq!(e, BoundCode::DimOver.val()),
            Ok(()) => assert!(false, "摆位越界未拒"),
        }
        // 回绕闸序：w 极大（≥0x8000_0000）先按回绕口径拒（回绕闸在摆位闸之前）。
        match RenderParams::base().with_x(600).with_w(u32::MAX - 500).validate_wire() {
            Err(e) => assert_eq!(e, BoundCode::WrapParam.val()),
            Ok(()) => assert!(false, "回绕像未拒"),
        }
    }

    #[test]
    fn dim_zero_and_coord_over() {
        match RenderParams::base().with_w(0).validate_wire() {
            Err(e) => assert_eq!(e, BoundCode::DimZero.val()),
            Ok(()) => assert!(false, "零宽未拒"),
        }
        match RenderParams::base().with_y(FB_H).validate_wire() {
            Err(e) => assert_eq!(e, BoundCode::CoordOver.val()),
            Ok(()) => assert!(false, "越行未拒"),
        }
        match RenderParams::base().with_glyph(GLYPH_SET).validate_wire() {
            Err(e) => assert_eq!(e, BoundCode::GlyphOver.val()),
            Ok(()) => assert!(false, "越字形未拒"),
        }
    }

    #[test]
    fn ledger_conservation() {
        let mut led = GateLedger::new();
        led.record_prod(false);
        led.record_prod(false);
        led.record_inj(true);
        led.record_inj(true);
        led.record_inj(false);
        assert!(led.conservation_ok());
        // 破坏守恒：拦截数越过尝试数必被复算抓出。
        let mut bad = led.clone();
        bad.inj_intercepts = bad.inj_intercepts.saturating_add(2);
        assert!(bad.inj_intercepts > bad.inj_tries);
        assert!(!bad.conservation_ok());
        assert!(led.all_tallied() == false, "放行的注入不该叫账齐");
    }

    #[test]
    fn inject_rounds_tally() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(e) => {
                assert!(false, "inject err=0x{:x}", e);
                return;
            }
        };
        assert_eq!(led.inj_tries, JUDGE_ROUNDS * 12);
        assert_eq!(led.inj_intercepts, JUDGE_ROUNDS * 12);
        assert_eq!(led.prod_tries, JUDGE_ROUNDS);
        assert_eq!(led.prod_intercepts, 0);
        assert!(led.all_tallied());
    }

    #[test]
    fn probe_sequences() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn reproducible_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(code_closure_ok());
        assert!(lcg_deterministic_ok());
        assert_eq!(detail(BoundCode::WrapParam.val()), BoundCode::WrapParam.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }
}
