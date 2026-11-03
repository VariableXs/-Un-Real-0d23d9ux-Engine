//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0490 · 渲染校验图案回读（视觉正确性） · 0x75D00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批5-K）：
//!   校验图案（棋盘/渐变/边框三图案闭集）回读逐位全等，图案账
//!   （图案数=校验数 100%）
//!   - 图案注入（翻转一位）必抓
//! 设计要点：
//!   1. 三图案闭集：棋盘（8×8 像素块双色交替）/ 渐变（x×8+y 取模
//!      256 的单色索引斜坡）/ 边框（2 像素框线包 0 底内域）三枚，
//!      ALL 数组闭集返回；
//!   2. 冻结口径：帧面 32×32 色索引位图，棋盘块 8×8，框线 2 像素；
//!      期望位图由生成器一次冻结成型，渲染/回读共用同一张期望；
//!   3. 回读逐位全等：帧面与期望逐格比对，首差异格即报
//!      （ReadbackMismatch），零截断零漂移；
//!   4. 图案账：渲染数 = 校验数（100%）守恒，校验数只在逐位全等
//!      时 +1，篡改一律不落账；
//!   5. 图案注入：LCG 定位翻转一位（色索引 +1），回读必报失配，
//!      10 轮 × 3 图案 10/10 必抓；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75D00，AI-VX7-1104 批5-K 独占槽）。
pub const SEG_BASE: u32 = 0x75D00;

// ── 冻结口径 ────────────────────────────────────────────────────
/// 帧面宽（32 像素）。
pub const PAT_W: usize = 32;
/// 帧面高（32 像素）。
pub const PAT_H: usize = 32;
/// 棋盘块边长（8×8 像素）。
pub const CHECK_CELL: usize = 8;
/// 边框线宽（2 像素）。
pub const BORDER_PX: usize = 2;
/// 棋盘色 A。
pub const CB_A: u8 = 0x01;
/// 棋盘色 B。
pub const CB_B: u8 = 0x02;
/// 边框色。
pub const BORDER_C: u8 = 0x03;
/// 边框内域底色。
pub const INNER_C: u8 = 0x00;
/// 渐变取模基数（色闭集 256）。
pub const GRAD_MOD: u32 = 256;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatCode {
    /// 图案尺寸越界（≠ 32×32）
    DimBreach,
    /// 未知图案（不在三图案闭集）
    KindUnknown,
    /// 回读失配（与期望位图不一致）
    ReadbackMismatch,
    /// 图案账守恒破坏（校验数 > 渲染数）
    LedgerBreach,
    /// 注入漏抓
    InjectLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl PatCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PatCode::DimBreach => "图案尺寸越界：仅 32×32，建议对齐冻结口径重新生成",
            PatCode::KindUnknown => "未知图案：不在三图案闭集，建议按闭集枚举取名",
            PatCode::ReadbackMismatch => "回读失配：与期望位图不一致，建议逐格对账定位首差异格位",
            PatCode::LedgerBreach => "图案账守恒破坏：校验数不得超过渲染数，建议先渲染后校验",
            PatCode::InjectLeaked => "注入漏抓：翻转样本未检出，建议对齐冻结期望位图复核",
            PatCode::NotReproducible => "双跑不一致：复现红线，建议固定种子后重跑两遍对账",
        }
    }
    pub const ALL: [PatCode; 6] = [
        PatCode::DimBreach,
        PatCode::KindUnknown,
        PatCode::ReadbackMismatch,
        PatCode::LedgerBreach,
        PatCode::InjectLeaked,
        PatCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回；段外码显性报未知）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PatCode::ALL.len() {
        if let Some(c) = PatCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75D00 段闭集内"
}

// ── 三图案闭集 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatKind {
    /// 棋盘（8×8 像素块双色交替）
    Checkerboard,
    /// 渐变（x×8+y 取模 256 斜坡）
    Gradient,
    /// 边框（2 像素框线包 0 底内域）
    Border,
}

impl PatKind {
    pub fn val(self) -> u32 {
        self as u32
    }
    pub const ALL: [PatKind; 3] = [PatKind::Checkerboard, PatKind::Gradient, PatKind::Border];
}

// ── 域模型：校验图案（期望位图） ────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    pub kind: PatKind,
    pub w: usize,
    pub h: usize,
    /// 色索引位图（行主序，w×h 格）。
    pub pixels: Vec<u8>,
}

impl Pattern {
    /// 棋盘块坐标取色：(bx+by) 偶 ⇒ A，奇 ⇒ B。
    pub fn checker_cell(x: usize, y: usize) -> u8 {
        let bx = x / CHECK_CELL;
        let by = y / CHECK_CELL;
        if (bx + by) % 2 == 0 {
            CB_A
        } else {
            CB_B
        }
    }

    /// 渐变斜坡取色：(x×8+y) 取模 256。
    pub fn gradient_cell(x: usize, y: usize) -> u8 {
        ((x as u32 * 8 + y as u32) % GRAD_MOD) as u8
    }

    /// 边框取色：距边 < 2 像素 ⇒ 框色，否则内域底色。
    pub fn border_cell(x: usize, y: usize) -> u8 {
        if x < BORDER_PX || y < BORDER_PX || x >= PAT_W - BORDER_PX || y >= PAT_H - BORDER_PX {
            BORDER_C
        } else {
            INNER_C
        }
    }
}

/// 生成期望位图（三图案闭集；尺寸冻结 32×32）。
pub fn generate(kind: PatKind) -> Result<Pattern, u32> {
    let mut pixels: Vec<u8> = Vec::new();
    let mut y = 0;
    while y < PAT_H {
        let mut x = 0;
        while x < PAT_W {
            let v = match kind {
                PatKind::Checkerboard => Pattern::checker_cell(x, y),
                PatKind::Gradient => Pattern::gradient_cell(x, y),
                PatKind::Border => Pattern::border_cell(x, y),
            };
            pixels.push(v);
            x += 1;
        }
        y += 1;
    }
    Ok(Pattern { kind, w: PAT_W, h: PAT_H, pixels })
}

/// 空帧面（全 0 底色）。
pub fn blank_fb() -> Vec<u8> {
    let mut fb: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < PAT_W * PAT_H {
        fb.push(0);
        i += 1;
    }
    fb
}

/// 渲染（blit 整幅位图；尺寸不符显性拒）。
pub fn render(pat: &Pattern, fb: &mut Vec<u8>) -> Result<(), u32> {
    if pat.w != PAT_W || pat.h != PAT_H || pat.pixels.len() != PAT_W * PAT_H {
        return Err(PatCode::DimBreach.val());
    }
    if fb.len() != PAT_W * PAT_H {
        return Err(PatCode::DimBreach.val());
    }
    let mut i = 0;
    while i < pat.pixels.len() {
        let v = match pat.pixels.get(i) {
            Some(x) => *x,
            None => return Err(PatCode::DimBreach.val()),
        };
        if let Some(cell) = fb.get_mut(i) {
            *cell = v;
        }
        i += 1;
    }
    Ok(())
}

/// 回读逐位全等（首差异格即报）。
pub fn readback_eq(fb: &Vec<u8>, pat: &Pattern) -> Result<(), u32> {
    if pat.pixels.len() != fb.len() {
        return Err(PatCode::DimBreach.val());
    }
    let mut i = 0;
    while i < fb.len() {
        let got = match fb.get(i) {
            Some(x) => *x,
            None => return Err(PatCode::DimBreach.val()),
        };
        let want = match pat.pixels.get(i) {
            Some(x) => *x,
            None => return Err(PatCode::DimBreach.val()),
        };
        if got != want {
            return Err(PatCode::ReadbackMismatch.val());
        }
        i += 1;
    }
    Ok(())
}

/// 图案注入：翻转一位（色索引 +1）。返回是否翻转成功。
pub fn flip_one(fb: &mut Vec<u8>, pos: usize) -> bool {
    match fb.get_mut(pos) {
        Some(cell) => {
            *cell = (*cell).wrapping_add(1);
            true
        }
        None => false,
    }
}

// ── 图案账（渲染数 = 校验数 100%） ──────────────────────────────
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PatLedger {
    pub rendered: u32,
    pub verified: u32,
}

impl PatLedger {
    pub fn new() -> PatLedger {
        PatLedger::default()
    }
    pub fn record_render(&mut self) {
        self.rendered = self.rendered.saturating_add(1);
    }
    /// 只在逐位全等时计校验数。
    pub fn record_verify(&mut self, ok: bool) {
        if ok {
            self.verified = self.verified.saturating_add(1);
        }
    }
    /// 守恒：校验数 ≤ 渲染数；100% 即两者相等。
    pub fn conservation_ok(&self) -> bool {
        self.verified <= self.rendered
    }
    pub fn full_match(&self) -> bool {
        self.rendered > 0 && self.rendered == self.verified
    }
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

// ── 黄金全链：三图案渲染→回读全等→账 100%→注入 10 轮×3 图案全抓 ─
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = PatLedger::new();
    let mut verified = 0u32;
    let mut k = 0;
    while k < PatKind::ALL.len() {
        let kind = match PatKind::ALL.get(k) {
            Some(x) => *x,
            None => return Err(PatCode::KindUnknown.val()),
        };
        let pat = generate(kind)?;
        let mut fb = blank_fb();
        render(&pat, &mut fb)?;
        led.record_render();
        let ok = readback_eq(&fb, &pat).is_ok();
        led.record_verify(ok);
        if ok {
            verified = verified.saturating_add(1);
        }
        k += 1;
    }
    if !led.conservation_ok() || !led.full_match() {
        return Err(PatCode::LedgerBreach.val());
    }
    // 注入：10 轮 × 3 图案，每轮翻转一位必报失配。
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut caught = 0u32;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut k = 0;
        while k < PatKind::ALL.len() {
            let kind = match PatKind::ALL.get(k) {
                Some(x) => *x,
                None => return Err(PatCode::KindUnknown.val()),
            };
            let pat = generate(kind)?;
            let mut fb = blank_fb();
            render(&pat, &mut fb)?;
            let pos = rng.below((PAT_W * PAT_H) as u64) as usize;
            if !flip_one(&mut fb, pos) {
                return Err(PatCode::InjectLeaked.val());
            }
            match readback_eq(&fb, &pat) {
                Err(e) if e == PatCode::ReadbackMismatch.val() => {
                    caught = caught.saturating_add(1);
                }
                _ => return Err(PatCode::InjectLeaked.val()),
            }
            k += 1;
        }
        r += 1;
    }
    if caught != JUDGE_ROUNDS * (PatKind::ALL.len() as u32) {
        return Err(PatCode::InjectLeaked.val());
    }
    Ok((verified, caught, led.rendered))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（3/3 校验 + 注入 30/30 全抓）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((3, 30, 3)) => Ok(()),
        _ => Err(PatCode::NotReproducible.val()),
    }
}

/// 序 B：棋盘与渐变锚点（块同域同色 / 斜坡端点）。
pub fn probe_seq_b() -> Result<(), u32> {
    let c00 = Pattern::checker_cell(0, 0);
    let c77 = Pattern::checker_cell(7, 7);
    let c80 = Pattern::checker_cell(8, 0);
    let g00 = Pattern::gradient_cell(0, 0);
    let g3131 = Pattern::gradient_cell(31, 31);
    match (c00, c77, c80, g00, g3131) {
        (CB_A, CB_A, CB_B, 0, 23) => Ok(()),
        _ => Err(PatCode::NotReproducible.val()),
    }
}

/// 序 C：边框内外分区（角与边为框色，内心为底色）。
pub fn probe_seq_c() -> Result<(), u32> {
    let corner = Pattern::border_cell(0, 0);
    let edge = Pattern::border_cell(31, 16);
    let inner = Pattern::border_cell(16, 16);
    let inner_edge = Pattern::border_cell(BORDER_PX, BORDER_PX);
    match (corner, edge, inner, inner_edge) {
        (BORDER_C, BORDER_C, INNER_C, INNER_C) => Ok(()),
        _ => Err(PatCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 3 图案翻转） ───────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0 }
    }
    pub fn record(&mut self, hit: bool) {
        self.tries = self.tries.saturating_add(1);
        if hit {
            self.caught = self.caught.saturating_add(1);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let mut k = 0;
        while k < PatKind::ALL.len() {
            let kind = match PatKind::ALL.get(k) {
                Some(x) => *x,
                None => return Err(PatCode::KindUnknown.val()),
            };
            let pat = generate(kind)?;
            let mut fb = blank_fb();
            render(&pat, &mut fb)?;
            let pos = rng.below((PAT_W * PAT_H) as u64) as usize;
            let flipped = flip_one(&mut fb, pos);
            led.record(
                flipped && match readback_eq(&fb, &pat) {
                    Err(e) => e == PatCode::ReadbackMismatch.val(),
                    Ok(()) => false,
                },
            );
            k += 1;
        }
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PatCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在 0x75000~0x781C0 的 0x40 对齐窗口） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < PatCode::ALL.len() {
        if let Some(c) = PatCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 诊断码闭集对账 ─────────────────────────────────────────────
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < PatCode::ALL.len() {
        if let Some(c) = PatCode::ALL.get(i) {
            if detail(c.val()) != c.detail() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 三图案闭集对账（ALL 数组 3 枚无重复） ───────────────────────
fn kind_closure_ok() -> bool {
    if PatKind::ALL.len() != 3 {
        return false;
    }
    let mut i = 0;
    while i < PatKind::ALL.len() {
        let mut j = i + 1;
        while j < PatKind::ALL.len() {
            if let (Some(a), Some(b)) = (PatKind::ALL.get(i), PatKind::ALL.get(j)) {
                if a == b {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 逐位全等独立复核（生成器 vs 锚点谓词逐格重算） ──────────────
fn pixelwise_recheck_ok() -> bool {
    let mut k = 0;
    while k < PatKind::ALL.len() {
        let kind = match PatKind::ALL.get(k) {
            Some(x) => *x,
            None => return false,
        };
        let pat = match generate(kind) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let mut y = 0;
        while y < PAT_H {
            let mut x = 0;
            while x < PAT_W {
                let idx = y * PAT_W + x;
                let got = match pat.pixels.get(idx) {
                    Some(v) => *v,
                    None => return false,
                };
                let want = match kind {
                    PatKind::Checkerboard => Pattern::checker_cell(x, y),
                    PatKind::Gradient => Pattern::gradient_cell(x, y),
                    PatKind::Border => Pattern::border_cell(x, y),
                };
                if got != want {
                    return false;
                }
                x += 1;
            }
            y += 1;
        }
        k += 1;
    }
    true
}

// ── LCG 确定性 ─────────────────────────────────────────────────
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
        PAT_W == 32 && PAT_H == 32 && CHECK_CELL == 8 && BORDER_PX == 2 && GRAD_MOD == 256,
        format!("32×32 块8 框2 色256"),
    ));
    // 2. 三图案闭集。
    v.push((
        "J1-三图案闭集",
        kind_closure_ok(),
        format!("棋盘/渐变/边框 {} 枚无重复", PatKind::ALL.len()),
    ));
    // 3. 黄金全链（三图案渲染+回读+账 100%）。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((ver, caught, rend)) => *ver == 3 && *rend == 3 && *caught == 30,
            Err(_) => false,
        },
        match &g {
            Ok((ver, caught, rend)) => format!("ver={} rend={} inject={}", ver, rend, caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 4. 棋盘逐位全等。
    let cb_ok = {
        let pat = generate(PatKind::Checkerboard);
        match &pat {
            Ok(p) => {
                let mut fb = blank_fb();
                let _ = render(p, &mut fb);
                readback_eq(&fb, p).is_ok()
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-棋盘逐位全等",
        cb_ok,
        String::from("8×8 块双色交替 1024 格全等"),
    ));
    // 5. 渐变逐位全等。
    let gr_ok = {
        let pat = generate(PatKind::Gradient);
        match &pat {
            Ok(p) => {
                let mut fb = blank_fb();
                let _ = render(p, &mut fb);
                readback_eq(&fb, p).is_ok()
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-渐变逐位全等",
        gr_ok,
        String::from("x×8+y 取模斜坡 1024 格全等"),
    ));
    // 6. 边框逐位全等。
    let bd_ok = {
        let pat = generate(PatKind::Border);
        match &pat {
            Ok(p) => {
                let mut fb = blank_fb();
                let _ = render(p, &mut fb);
                readback_eq(&fb, p).is_ok()
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-边框逐位全等",
        bd_ok,
        String::from("2 像素框线包 0 底内域 1024 格全等"),
    ));
    // 7. 图案账 100%。
    let ledger_ok = {
        let mut led = PatLedger::new();
        let mut k = 0;
        while k < PatKind::ALL.len() {
            if let Some(kind) = PatKind::ALL.get(k) {
                let pat = generate(*kind);
                led.record_render();
                let ok = match &pat {
                    Ok(p) => {
                        let mut fb = blank_fb();
                        let _ = render(p, &mut fb);
                        readback_eq(&fb, p).is_ok()
                    }
                    Err(_) => false,
                };
                led.record_verify(ok);
            }
            k += 1;
        }
        led.conservation_ok() && led.full_match() && led.rendered == PatKind::ALL.len() as u32
    };
    v.push((
        "J1-图案账100%",
        ledger_ok,
        format!("图案数=校验数={}", PatKind::ALL.len()),
    ));
    // 8. 注入翻转一位 10 轮 × 3 图案全抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-翻转注入30/30",
        match &inj {
            Ok(l) => l.all_caught() && l.tries == JUDGE_ROUNDS * 3,
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("t={} c={}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 9. 逐位独立复核（锚点谓词重算）。
    v.push((
        "J1-逐位独立复核",
        pixelwise_recheck_ok(),
        String::from("三图案 1024 格逐格重算全等"),
    ));
    // 10. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b锚点={} c边框分区={}", a, b, c),
    ));
    // 11. 诊断码闭集对账。
    v.push((
        "J1-诊断码闭集对账",
        code_closure_ok(),
        format!("{} 码全查表通过", PatCode::ALL.len()),
    ));
    // 12. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 13. LCG 确定性。
    v.push((
        "J2-LCG确定性",
        lcg_deterministic_ok(),
        String::from("同种子 16 步逐位一致"),
    ));
    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75D00 窗口 0x40 对齐区间三闸"),
    ));
    v
}

pub fn run_unx0490_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0490 渲染校验图案回读（视觉正确性）");
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
            Ok((ver, caught, rend)) => {
                assert_eq!(ver, 3);
                assert_eq!(rend, 3);
                assert_eq!(caught, 30);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn frozen_consts() {
        assert_eq!(PAT_W, 32);
        assert_eq!(PAT_H, 32);
        assert_eq!(CHECK_CELL, 8);
        assert_eq!(BORDER_PX, 2);
        assert_eq!(GRAD_MOD, 256);
        assert_eq!(PatKind::ALL.len(), 3);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn checkerboard_block_property() {
        // 同块同色：块内任意两格同色。
        assert_eq!(Pattern::checker_cell(0, 0), Pattern::checker_cell(7, 7));
        assert_eq!(Pattern::checker_cell(8, 8), Pattern::checker_cell(15, 15));
        // 邻块异色。
        assert_ne!(Pattern::checker_cell(0, 0), Pattern::checker_cell(8, 0));
        assert_ne!(Pattern::checker_cell(0, 0), Pattern::checker_cell(0, 8));
        // 对角同色（32×32 恰 4×4 块，偶数块）。
        assert_eq!(Pattern::checker_cell(0, 0), Pattern::checker_cell(31, 31));
        // 仅两色闭集。
        let pat = generate(PatKind::Checkerboard).ok().unwrap_or_else(|| {
            Pattern { kind: PatKind::Checkerboard, w: PAT_W, h: PAT_H, pixels: Vec::new() }
        });
        let mut i = 0;
        while i < pat.pixels.len() {
            if let Some(v) = pat.pixels.get(i) {
                assert!(*v == CB_A || *v == CB_B, "棋盘出闭集色 0x{:x}", v);
            }
            i += 1;
        }
    }

    #[test]
    fn gradient_anchors() {
        assert_eq!(Pattern::gradient_cell(0, 0), 0);
        assert_eq!(Pattern::gradient_cell(1, 0), 8);
        assert_eq!(Pattern::gradient_cell(0, 1), 1);
        assert_eq!(Pattern::gradient_cell(31, 31), 23);
        // 斜坡单调（沿 x 每 +8，取模回卷前）。
        assert_eq!(Pattern::gradient_cell(2, 0), 16);
    }

    #[test]
    fn border_zone_property() {
        // 四角与四边为框色。
        assert_eq!(Pattern::border_cell(0, 0), BORDER_C);
        assert_eq!(Pattern::border_cell(31, 0), BORDER_C);
        assert_eq!(Pattern::border_cell(0, 31), BORDER_C);
        assert_eq!(Pattern::border_cell(16, 0), BORDER_C);
        assert_eq!(Pattern::border_cell(16, 31), BORDER_C);
        // 内心为底色。
        assert_eq!(Pattern::border_cell(16, 16), INNER_C);
        assert_eq!(Pattern::border_cell(2, 2), INNER_C);
        assert_eq!(Pattern::border_cell(29, 29), INNER_C);
    }

    #[test]
    fn flip_one_caught_everywhere() {
        // 全帧面逐格翻转必报失配（注入无死角）。
        let pat = generate(PatKind::Gradient).ok().unwrap_or_else(|| {
            Pattern { kind: PatKind::Gradient, w: PAT_W, h: PAT_H, pixels: Vec::new() }
        });
        let mut pos = 0;
        while pos < PAT_W * PAT_H {
            let mut fb = blank_fb();
            let _ = render(&pat, &mut fb);
            assert!(flip_one(&mut fb, pos));
            match readback_eq(&fb, &pat) {
                Err(e) => assert_eq!(e, PatCode::ReadbackMismatch.val()),
                Ok(()) => assert!(false, "翻转位 {} 未检出", pos),
            }
            pos += 1;
        }
    }

    #[test]
    fn ledger_conservation() {
        let mut led = PatLedger::new();
        led.record_render();
        led.record_verify(true);
        assert!(led.conservation_ok() && led.full_match());
        // 校验数不得超渲染数。
        led.record_verify(true);
        assert!(!led.conservation_ok(), "校验数超渲染数未抓");
        // 失配不落校验账。
        let mut led2 = PatLedger::new();
        led2.record_render();
        led2.record_verify(false);
        assert_eq!(led2.verified, 0);
        assert!(led2.conservation_ok() && !led2.full_match());
    }

    #[test]
    fn dim_breach_caught() {
        // 尺寸不符的期望位图渲染/回读显性拒。
        let pat = generate(PatKind::Border).ok().unwrap_or_else(|| {
            Pattern { kind: PatKind::Border, w: PAT_W, h: PAT_H, pixels: Vec::new() }
        });
        let mut short = pat.clone();
        let _ = short.pixels.pop();
        let mut fb = blank_fb();
        match render(&short, &mut fb) {
            Err(e) => assert_eq!(e, PatCode::DimBreach.val()),
            Ok(()) => assert!(false, "缺格位图未拒"),
        }
        match readback_eq(&fb, &short) {
            Err(e) => assert_eq!(e, PatCode::DimBreach.val()),
            Ok(()) => assert!(false, "长度失配回读未拒"),
        }
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
        assert!(led.all_caught());
        assert_eq!(led.tries, JUDGE_ROUNDS * 3);
        assert_eq!(led.caught, JUDGE_ROUNDS * 3);
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
        assert!(kind_closure_ok());
        assert!(pixelwise_recheck_ok());
        assert!(lcg_deterministic_ok());
        assert_eq!(detail(PatCode::ReadbackMismatch.val()), PatCode::ReadbackMismatch.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }
}
