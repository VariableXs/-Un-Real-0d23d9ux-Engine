//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0487 · 清屏与配色原语 · 0x75BC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批4-J）：
//!   清屏全屏单色填充后位图零残留（回读全等），前景/背景 16 色表闭集
//!   对账
//! 设计要点：
//!   1. 位图账：25 行 × 80 列冻结口径，字符位图 + 配色位图双账；
//!   2. 清屏语义：全屏单色填充（属性字节 = 背景高半 <<4 | 前景低半），
//!      字符面统一补空格，填充后逐格回读全等（零残留红线）；
//!   3. 16 色表闭集：VGA 0x0~0xF 十六色中文命名冻结表，前景/背景
//!      256 组合全对账（16×16 闭集），越界色索引（≥16）显性拒；
//!   4. 越界样本 12/12：坐标（行/列）、色索引、字形索引三类共 12 个
//!      违约样本全部必须被拦截；
//!   5. 拦截计数生产 0：合规生产链（全合法参数）拦截计数恒为 0；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75BC0，AI-VX7-1104 批4-J 独占槽）。
pub const SEG_BASE: u32 = 0x75BC0;

// ── 冻结口径 ────────────────────────────────────────────────────
/// 可见行数上限（25 行）。
pub const ROWS: usize = 25;
/// 列宽上限（80 列）。
pub const COLS: usize = 80;
/// 16 色表宽（0x0~0xF 闭集）。
pub const PALETTE_W: usize = 16;
/// 空格字形（清屏字符面补白口径）。
pub const SPACE_GLYPH: u8 = 0x20;
/// 清屏底字形（零残留判定的期望字形）。
pub const CLEAR_GLYPH: u8 = SPACE_GLYPH;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 16 色表闭集（VGA 文本口径，中文命名冻结） ───────────────────
/// 十六色中文命名表（下标即色索引 0x0~0xF）。
pub const PALETTE_NAMES: [&str; 16] = [
    "黑", "蓝", "绿", "青", "红", "紫", "棕", "浅灰", "深灰", "亮蓝", "亮绿", "亮青", "亮红",
    "粉红", "亮黄", "白",
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 行号越界
    RowOver,
    /// 列号越界
    ColOver,
    /// 色索引越闭集（≥16）
    ColorOver,
    /// 字形越闭集（≥128）
    GlyphOver,
    /// 回读残留（清屏后位图非期望）
    ResidueFound,
    /// 色表对账失配
    PaletteMismatch,
    /// 注入漏抓
    InjectLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl ClCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClCode::RowOver => "行号越界：可见面仅 25 行，建议以 ROWS 为界先判后写",
            ClCode::ColOver => "列号越界：可见面仅 80 列，建议以 COLS 为界先判后写",
            ClCode::ColorOver => "色索引越闭集：仅 0x0~0xF 十六色，建议以 PALETTE_W 为界对色值先行过滤",
            ClCode::GlyphOver => "字形越闭集：仅 ASCII 0x00~0x7F，建议对非 ASCII 字形先行过滤",
            ClCode::ResidueFound => "回读残留：清屏后位图与期望不一致，建议逐格对账定位首残留格位",
            ClCode::PaletteMismatch => "色表对账失配：前景/背景组合账不平，建议按 16×16 闭集全对账",
            ClCode::InjectLeaked => "注入漏抓：违约样本未被拦截，建议复核守卫谓词方向",
            ClCode::NotReproducible => "双跑不一致：复现红线，建议排查非确定性来源（随机/墙钟/迭代序）",
        }
    }
    pub const ALL: [ClCode; 8] = [
        ClCode::RowOver,
        ClCode::ColOver,
        ClCode::ColorOver,
        ClCode::GlyphOver,
        ClCode::ResidueFound,
        ClCode::PaletteMismatch,
        ClCode::InjectLeaked,
        ClCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，未知码显性文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ClCode::ALL.len() {
        if let Some(c) = ClCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75BC0 段闭集内，建议核对诊断码表"
}

// ── 清屏与配色屏 ────────────────────────────────────────────────
/// 清屏配色屏：字符位图 + 配色位图 + 拦截计数（生产 0 口径）。
#[derive(Clone, Debug)]
pub struct ClearScreen {
    chars: Vec<u8>,
    attrs: Vec<u8>,
    /// 拦截计数（合规生产链必须为 0；注入面另账，不入此计数）。
    pub intercepts: u32,
    /// 清屏执行计数。
    pub clears: u64,
    /// 写格执行计数。
    pub puts: u64,
}

impl ClearScreen {
    pub fn new() -> ClearScreen {
        let total = match ROWS.checked_mul(COLS) {
            Some(t) => t,
            None => 0,
        };
        let mut chars = Vec::new();
        let mut attrs = Vec::new();
        let mut i = 0;
        while i < total {
            chars.push(CLEAR_GLYPH);
            attrs.push(0x07);
            i += 1;
        }
        ClearScreen { chars, attrs, intercepts: 0, clears: 0, puts: 0 }
    }

    /// 属性字节合成（前景/背景各 0x0~0xF，越闭集显性拒）。
    pub fn attr_of(fg: u32, bg: u32) -> Result<u8, u32> {
        if fg >= PALETTE_W as u32 {
            return Err(ClCode::ColorOver.val());
        }
        if bg >= PALETTE_W as u32 {
            return Err(ClCode::ColorOver.val());
        }
        Ok(((bg as u8) << 4) | (fg as u8))
    }

    /// 属性字节分解对账（fg/bg 均必落 16 色闭集）。
    pub fn split_attr(attr: u8) -> (u8, u8) {
        (attr & 0x0F, attr >> 4)
    }

    /// 格位寻址（checked 乘加，越界 None）。
    fn cell_at(row: usize, col: usize) -> Option<usize> {
        let base = row.checked_mul(COLS)?;
        base.checked_add(col)
    }

    /// 清屏原语：全屏单色填充（字符面补空格 + 配色面统一 attr），
    /// 填充后即席回读全等（零残留自证）。
    pub fn clear(&mut self, fg: u32, bg: u32) -> Result<(), u32> {
        let attr = match ClearScreen::attr_of(fg, bg) {
            Ok(a) => a,
            Err(e) => {
                self.intercepts += 1;
                return Err(e);
            }
        };
        let total = match ROWS.checked_mul(COLS) {
            Some(t) => t,
            None => return Err(ClCode::ResidueFound.val()),
        };
        let mut i = 0;
        while i < total {
            match (self.chars.get_mut(i), self.attrs.get_mut(i)) {
                (Some(c), Some(a)) => {
                    *c = CLEAR_GLYPH;
                    *a = attr;
                }
                _ => return Err(ClCode::ResidueFound.val()),
            }
            i += 1;
        }
        self.clears = match self.clears.checked_add(1) {
            Some(v) => v,
            None => self.clears,
        };
        // 即席零残留自证：回读全等。
        self.zero_residue(fg, bg)
    }

    /// 零残留回读：全屏字符 = CLEAR_GLYPH 且配色 = 期望 attr（逐格全等）。
    pub fn zero_residue(&self, fg: u32, bg: u32) -> Result<(), u32> {
        let want_attr = match ClearScreen::attr_of(fg, bg) {
            Ok(a) => a,
            Err(e) => return Err(e),
        };
        let total = self.chars.len();
        if total != self.attrs.len() || total != ROWS * COLS {
            return Err(ClCode::ResidueFound.val());
        }
        let mut i = 0;
        while i < total {
            let c = match self.chars.get(i) {
                Some(v) => *v,
                None => return Err(ClCode::ResidueFound.val()),
            };
            let a = match self.attrs.get(i) {
                Some(v) => *v,
                None => return Err(ClCode::ResidueFound.val()),
            };
            if c != CLEAR_GLYPH || a != want_attr {
                return Err(ClCode::ResidueFound.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 写格原语（行/列/字形三闸 + 色值两闸），违约计入拦截计数。
    pub fn put(&mut self, row: usize, col: usize, glyph: u8, fg: u32, bg: u32) -> Result<(), u32> {
        if row >= ROWS {
            self.intercepts += 1;
            return Err(ClCode::RowOver.val());
        }
        if col >= COLS {
            self.intercepts += 1;
            return Err(ClCode::ColOver.val());
        }
        if glyph as usize >= 128 {
            self.intercepts += 1;
            return Err(ClCode::GlyphOver.val());
        }
        let attr = match ClearScreen::attr_of(fg, bg) {
            Ok(a) => a,
            Err(e) => {
                self.intercepts += 1;
                return Err(e);
            }
        };
        let idx = match ClearScreen::cell_at(row, col) {
            Some(i) => i,
            None => {
                self.intercepts += 1;
                return Err(ClCode::RowOver.val());
            }
        };
        match (self.chars.get_mut(idx), self.attrs.get_mut(idx)) {
            (Some(c), Some(a)) => {
                *c = glyph;
                *a = attr;
            }
            _ => {
                self.intercepts += 1;
                return Err(ClCode::ColOver.val());
            }
        }
        self.puts = match self.puts.checked_add(1) {
            Some(v) => v,
            None => self.puts,
        };
        Ok(())
    }

    /// 整面回读（字符面）。
    pub fn readback_chars(&self) -> Vec<u8> {
        self.chars.clone()
    }

    /// 整面回读（配色面）。
    pub fn readback_attrs(&self) -> Vec<u8> {
        self.attrs.clone()
    }
}

impl Default for ClearScreen {
    fn default() -> ClearScreen {
        ClearScreen::new()
    }
}

// ── 16 色表闭集对账（16×16 = 256 组合全对账） ───────────────────
/// 色表对账：命名表 16 项齐备 + 256 组合合成/分解往返全等。
pub fn palette_reconcile() -> Result<(usize, usize), u32> {
    if PALETTE_NAMES.len() != PALETTE_W {
        return Err(ClCode::PaletteMismatch.val());
    }
    let mut i = 0;
    while i < PALETTE_NAMES.len() {
        match PALETTE_NAMES.get(i) {
            Some(n) => {
                if n.is_empty() {
                    return Err(ClCode::PaletteMismatch.val());
                }
            }
            None => return Err(ClCode::PaletteMismatch.val()),
        }
        i += 1;
    }
    let mut pairs = 0usize;
    let mut fg = 0u32;
    while fg < PALETTE_W as u32 {
        let mut bg = 0u32;
        while bg < PALETTE_W as u32 {
            let attr = match ClearScreen::attr_of(fg, bg) {
                Ok(a) => a,
                Err(e) => return Err(e),
            };
            let (fg2, bg2) = ClearScreen::split_attr(attr);
            if fg2 as u32 != fg || bg2 as u32 != bg {
                return Err(ClCode::PaletteMismatch.val());
            }
            pairs += 1;
            bg += 1;
        }
        fg += 1;
    }
    if pairs != PALETTE_W * PALETTE_W {
        return Err(ClCode::PaletteMismatch.val());
    }
    Ok((PALETTE_W, pairs))
}

// ── 越界样本 12/12（坐标 4 + 色索引 4 + 字形索引 4） ────────────
/// 三类 12 个违约样本，全部必须被拦截。
pub fn out_of_range_12() -> Result<u32, u32> {
    let mut caught = 0u32;
    // 坐标类 4 样本。
    let mut s = ClearScreen::new();
    let row_samples: [(usize, usize); 4] = [(ROWS, 0), (0, COLS), (99, 99), (ROWS, COLS)];
    let mut i = 0;
    while i < 4 {
        let (r, c) = match row_samples.get(i) {
            Some(v) => *v,
            None => break,
        };
        if s.put(r, c, b'X', 7, 0).is_err() {
            caught += 1;
        }
        i += 1;
    }
    // 色索引类 4 样本（清屏路 2 + 写格路 2）。
    let mut s2 = ClearScreen::new();
    if s2.clear(PALETTE_W as u32, 0).is_err() {
        caught += 1;
    }
    if s2.clear(0, 255).is_err() {
        caught += 1;
    }
    let mut s3 = ClearScreen::new();
    if s3.put(0, 0, b'X', 16, 0).is_err() {
        caught += 1;
    }
    if s3.put(0, 0, b'X', 0, 200).is_err() {
        caught += 1;
    }
    // 字形索引类 4 样本。
    let mut s4 = ClearScreen::new();
    let glyph_samples: [u8; 4] = [128, 200, 250, 255];
    let mut j = 0;
    while j < 4 {
        let g = match glyph_samples.get(j) {
            Some(v) => *v,
            None => break,
        };
        if s4.put(0, j, g, 7, 0).is_err() {
            caught += 1;
        }
        j += 1;
    }
    if caught != 12 {
        return Err(ClCode::InjectLeaked.val());
    }
    Ok(caught)
}

// ── 黄金全链：污染→清屏→零残留→写格→再清屏→生产拦截 0 ──────────
pub fn golden_run() -> Result<(u32, u64, u64), u32> {
    let mut s = ClearScreen::new();
    // 先污染：写一批可见字（黑底白字）。
    let mut r = 0;
    while r < 3 {
        let mut c = 0;
        while c < 40 {
            s.put(r, c, b'A', 15, 0)?;
            c += 1;
        }
        r += 1;
    }
    // 清屏（浅灰底黑字，VGA 常规 0x07）。
    s.clear(0, 7)?;
    // 零残留回读。
    s.zero_residue(0, 7)?;
    // 16 色逐色清屏对账（16 轮全零残留）。
    let mut color = 0u32;
    while color < PALETTE_W as u32 {
        s.clear(color, color)?;
        s.zero_residue(color, color)?;
        color += 1;
    }
    // 再清屏 0x07 后写合法格。
    s.clear(0, 7)?;
    s.put(24, 79, b'V', 15, 4)?;
    // 回读：仅末格为 'V'，其余全空格。
    let rb = s.readback_chars();
    let mut ok = true;
    let mut i = 0;
    while i < rb.len() {
        if let Some(g) = rb.get(i) {
            let expect = if i == ROWS * COLS - 1 { b'V' } else { CLEAR_GLYPH };
            if *g != expect {
                ok = false;
            }
        }
        i += 1;
    }
    if !ok {
        return Err(ClCode::ResidueFound.val());
    }
    // 合规生产链拦截计数必须为 0。
    if s.intercepts != 0 {
        return Err(ClCode::ResidueFound.val());
    }
    Ok((s.intercepts, s.clears, s.puts))
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 口径冻结谓词。
fn frozen_dims_ok() -> bool {
    ROWS == 25 && COLS == 80 && PALETTE_W == 16
}

/// 清屏幂等谓词：连清三遍零残留。
fn clear_idempotent_ok() -> bool {
    let mut s = ClearScreen::new();
    let mut k = 0;
    while k < 3 {
        if s.clear(9, 1).is_err() || s.zero_residue(9, 1).is_err() {
            return false;
        }
        k += 1;
    }
    true
}

/// 生产拦截 0 谓词：全合法链拦截计数恒 0。
fn production_zero_ok() -> bool {
    match golden_run() {
        Ok((intercepts, _, _)) => intercepts == 0,
        Err(_) => false,
    }
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A（正向）：清屏即零残留。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut s = ClearScreen::new();
    s.clear(15, 1)?;
    s.zero_residue(15, 1)
}

/// 序 B（越色必拒）：前景 16 出闭集。
pub fn probe_seq_b() -> Result<(), u32> {
    ClearScreen::attr_of(16, 0).map(|_| ())
}

/// 序 C（越行必拒）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut s = ClearScreen::new();
    s.put(ROWS, 0, b'X', 7, 0)
}

// ── LCG（种子写死 = SEG_BASE） ──────────────────────────────────
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

// ── 注入检出账（四槽：越行/越列/越色/越字形） ───────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 注入轮（LCG 驱动 rounds × 四槽） ────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 槽0：越行。
        let mut s0 = ClearScreen::new();
        led.record(0, s0.put(ROWS + 1, 0, b'A', 7, 0).is_err());
        // 槽1：越列。
        let mut s1 = ClearScreen::new();
        led.record(1, s1.put(0, COLS + 1, b'A', 7, 0).is_err());
        // 槽2：越色（前景/背景各一）。
        let mut s2 = ClearScreen::new();
        let fg_bad = s2.put(0, 0, b'A', 16, 0).is_err();
        let bg_bad = s2.put(0, 0, b'A', 0, 16).is_err();
        led.record(2, fg_bad && bg_bad);
        // 槽3：越字形。
        let mut s3 = ClearScreen::new();
        led.record(3, s3.put(0, 0, 129, 7, 0).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ClCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（窗口 + 对齐 + 区间三闸） ──────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < ClCode::ALL.len() {
        if let Some(c) = ClCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
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
        frozen_dims_ok(),
        format!("25×80 十六色表={}", PALETTE_W),
    ));
    // 2. 16 色表闭集对账。
    let pr = palette_reconcile();
    v.push((
        "J1-16色表闭集对账",
        match &pr {
            Ok((w, pairs)) => *w == 16 && *pairs == 256,
            Err(_) => false,
        },
        match &pr {
            Ok((w, pairs)) => format!("命名 {} 项 · fg/bg 组合 {} 全往返", w, pairs),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 3. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((intercepts, clears, puts)) => *intercepts == 0 && *clears == 18 && *puts == 121,
            Err(_) => false,
        },
        match &g {
            Ok((intercepts, clears, puts)) => format!("拦截={} 清屏={} 写格={}", intercepts, clears, puts),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 4. 清屏零残留（回读全等）。
    let residue_ok = {
        let mut s = ClearScreen::new();
        let _ = s.put(12, 40, b'X', 15, 0);
        let c1 = s.clear(7, 2).is_ok();
        let c2 = s.zero_residue(7, 2).is_ok();
        let attrs_all = {
            let rb = s.readback_attrs();
            let mut all = true;
            let mut i = 0;
            while i < rb.len() {
                if let Some(a) = rb.get(i) {
                    if *a != ClearScreen::attr_of(7, 2).unwrap_or(0xFF) {
                        all = false;
                    }
                }
                i += 1;
            }
            all
        };
        c1 && c2 && attrs_all
    };
    v.push((
        "J1-清屏零残留回读全等",
        residue_ok,
        String::from("污染后清屏 0x27 · 字符面/配色面逐格全等"),
    ));
    // 5. 清屏幂等。
    v.push((
        "J1-清屏幂等三连",
        clear_idempotent_ok(),
        String::from("连清三遍零残留"),
    ));
    // 6. 16 色逐色清屏全过。
    let color_ok = {
        let mut s = ClearScreen::new();
        let mut ok = true;
        let mut c = 0u32;
        while c < PALETTE_W as u32 {
            if s.clear(c, 15 - c).is_err() || s.zero_residue(c, 15 - c).is_err() {
                ok = false;
            }
            c += 1;
        }
        ok
    };
    v.push((
        "J1-16色逐色清屏全过",
        color_ok,
        String::from("前景 0x0~0xF × 背景反相 全零残留"),
    ));
    // 7. 越界样本 12/12。
    v.push((
        "J1-越界样本12/12",
        out_of_range_12() == Ok(12),
        format!("{}/12 检出（坐标4+色4+字形4）", match out_of_range_12() {
            Ok(n) => n,
            Err(_) => 0,
        }),
    ));
    // 8. 拦截计数生产 0。
    v.push((
        "J1-拦截计数生产0",
        production_zero_ok(),
        String::from("全合法生产链 intercepts == 0"),
    ));
    // 9. 注入四槽 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-注入四槽10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 10. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a正向={} b越色拒={} c越行拒={}", a, b, c),
    ));
    // 11. 诊断码闭集对账。
    let codes_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < ClCode::ALL.len() {
            if let Some(c) = ClCode::ALL.get(i) {
                if detail(c.val()) != c.detail() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok && detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
    };
    v.push((
        "J1-诊断码闭集对账",
        codes_ok,
        format!("{} 码全查表通过", ClCode::ALL.len()),
    ));
    // 12. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 13. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75BC0 窗口 0x40 对齐区间三闸"),
    ));
    // 14. LCG 确定性。
    let lcg_ok = {
        let mut x = Lcg::new(SEG_BASE as u64);
        let mut y = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 16 {
            if x.next() != y.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J2-LCG确定性", lcg_ok, String::from("同种子 16 步逐位一致")));
    v
}

pub fn run_unx0487_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0487 清屏与配色原语");
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
            Ok((intercepts, clears, puts)) => {
                assert_eq!(intercepts, 0);
                assert_eq!(clears, 18);
                assert_eq!(puts, 121);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn palette_closed_and_roundtrip() {
        assert!(frozen_dims_ok());
        assert_eq!(palette_reconcile(), Ok((16, 256)));
        assert_eq!(PALETTE_NAMES.len(), 16);
        let (fg, bg) = ClearScreen::split_attr(0x4F);
        assert_eq!(fg, 0x0F);
        assert_eq!(bg, 0x04);
    }

    #[test]
    fn color_over_caught() {
        match ClearScreen::attr_of(16, 0) {
            Err(e) => assert_eq!(e, ClCode::ColorOver.val()),
            Ok(_) => assert!(false, "前景越色未拒"),
        }
        match ClearScreen::attr_of(0, 17) {
            Err(e) => assert_eq!(e, ClCode::ColorOver.val()),
            Ok(_) => assert!(false, "背景越色未拒"),
        }
        let mut s = ClearScreen::new();
        match s.clear(99, 99) {
            Err(e) => assert_eq!(e, ClCode::ColorOver.val()),
            Ok(_) => assert!(false, "清屏越色未拒"),
        }
        assert_eq!(s.intercepts, 1, "越色应计入拦截计数");
    }

    #[test]
    fn row_col_glyph_caught() {
        let mut s = ClearScreen::new();
        match s.put(ROWS, 0, b'A', 7, 0) {
            Err(e) => assert_eq!(e, ClCode::RowOver.val()),
            Ok(_) => assert!(false, "越行未拒"),
        }
        match s.put(0, COLS, b'A', 7, 0) {
            Err(e) => assert_eq!(e, ClCode::ColOver.val()),
            Ok(_) => assert!(false, "越列未拒"),
        }
        match s.put(0, 0, 255, 7, 0) {
            Err(e) => assert_eq!(e, ClCode::GlyphOver.val()),
            Ok(_) => assert!(false, "越字形未拒"),
        }
        assert_eq!(s.intercepts, 3);
    }

    #[test]
    fn out_of_range_all_12_caught() {
        assert_eq!(out_of_range_12(), Ok(12));
    }

    #[test]
    fn zero_residue_after_pollution() {
        let mut s = ClearScreen::new();
        // 重污染：全屏写满。
        let mut r = 0;
        while r < ROWS {
            let mut c = 0;
            while c < COLS {
                assert!(s.put(r, c, b'#', 15, 1).is_ok());
                c += 1;
            }
            r += 1;
        }
        assert!(s.clear(3, 8).is_ok());
        assert!(s.zero_residue(3, 8).is_ok());
        // 篡改一格后零残留必失配。
        assert!(s.put(0, 0, b'Z', 7, 0).is_ok());
        match s.zero_residue(3, 8) {
            Err(e) => assert_eq!(e, ClCode::ResidueFound.val()),
            Ok(_) => assert!(false, "残留未检出"),
        }
    }

    #[test]
    fn production_intercepts_zero() {
        assert!(production_zero_ok());
        assert!(clear_idempotent_ok());
    }

    #[test]
    fn probe_sequences() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_err());
        assert!(probe_seq_c().is_err());
    }

    #[test]
    fn reproducible_two_runs() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn guard_and_codes() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(ClCode::ResidueFound.val()), ClCode::ResidueFound.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
        let mut x = Lcg::new(SEG_BASE as u64);
        let mut y = Lcg::new(SEG_BASE as u64);
        let mut i = 0;
        while i < 8 {
            assert_eq!(x.next(), y.next());
            i += 1;
        }
    }
}
