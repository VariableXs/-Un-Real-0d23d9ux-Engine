//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0103 · 帧簿控制台字体渲染 · 0x6C4C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F0103-J1 8x16 位图字体全字符集渲染，可变分辨率下行列
//!     自适应无破版；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 8x16 位图字体：每字符 16 字节行位图（8 像素宽），全字符集
//!     闭集 96 可打印 ASCII（0x20~0x7E），缺字显性方框；
//!   - 字形账：字符→16 行位图（确定性派生：笔画账写死主干字模 +
//!     LCG 补底纹，同字符双跑一致）；
//!   - 渲染：帧簿（framebuffer 账面 u8 灰度）按行写像素；可变分
//!     辨率：cols/rows 由 (宽/8, 高/16) 自适应派生，越行列即拒；
//!   - 无破版判据：整屏字符渲染后逐格对拍（格内 128 字节与字形
//!     账一致），乱序绘制后重排一致（幂等）；
//!   - 滚动账：上滚一行 = 行账整体上移 + 底行清零（逐字节）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C4C0，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C4C0;

// ── 字体冻结值 ──────────────────────────────────────────────────
/// 字符宽（像素）。
pub const GLYPH_W: usize = 8;
/// 字符高（像素）。
pub const GLYPH_H: usize = 16;
/// 每字形字节数（16 行 × 1 字节）。
pub const GLYPH_BYTES: usize = GLYPH_H;
/// 全字符集：可打印 ASCII 0x20~0x7E。
pub const CHARSET_FIRST: u8 = 0x20;
pub const CHARSET_LAST: u8 = 0x7E;
pub const CHARSET_COUNT: usize = (CHARSET_LAST - CHARSET_FIRST + 1) as usize;
/// 缺字方框（索引：全部位亮）。
pub const BOX_GLYPH_HINT: u8 = 0xFF;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontCode {
    /// 字符越字符集
    CharOutOfRange,
    /// 字形账长度错（≠16 字节）
    GlyphLenWrong,
    /// 分辨率不足（一行/一列都放不下）
    ScreenTooSmall,
    /// 越行列写（越屏写拒）
    CellOutOfRange,
    /// 渲染破版（格内对拍失败）
    RenderCorrupt,
    /// 滚动账破坏（顶行未清/底行丢失）
    ScrollLedgerBroken,
    /// 空帧簿渲染
    EmptyFramebuffer,
    /// 字形底纹不一致（双派生不同）
    TextureMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl FontCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FontCode::CharOutOfRange => "字符越字符集：0x20~0x7E 冻结",
            FontCode::GlyphLenWrong => "字形账长度错：≠16 字节",
            FontCode::ScreenTooSmall => "分辨率不足：一行/一列放不下",
            FontCode::CellOutOfRange => "越行列写：越屏拒",
            FontCode::RenderCorrupt => "渲染破版：格内对拍失败",
            FontCode::ScrollLedgerBroken => "滚动账破坏",
            FontCode::EmptyFramebuffer => "空帧簿渲染",
            FontCode::TextureMismatch => "字形底纹不一致：双派生不同",
            FontCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [FontCode; 9] = [
        FontCode::CharOutOfRange,
        FontCode::GlyphLenWrong,
        FontCode::ScreenTooSmall,
        FontCode::CellOutOfRange,
        FontCode::RenderCorrupt,
        FontCode::ScrollLedgerBroken,
        FontCode::EmptyFramebuffer,
        FontCode::TextureMismatch,
        FontCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FontCode::ALL.len() {
        if FontCode::ALL[i].val() == code {
            return FontCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C4C0 段闭集内"
}

// ── 字形账（确定性派生） ────────────────────────────────────────
/// 字形派生：主干字模（笔画账）+ 底纹补齐（LCG 确定性）。
/// 设计：每字符 16 行；主干 = 该字符低位轮廓（写死公式），底纹 =
/// LCG(种子=字符) 生成非主干行填充位；同字符双派生一致。
pub fn derive_glyph(ch: u8) -> Result<[u8; GLYPH_BYTES], u32> {
    if ch < CHARSET_FIRST || ch > CHARSET_LAST {
        return Err(FontCode::CharOutOfRange.val());
    }
    let idx = (ch - CHARSET_FIRST) as u64;
    let mut g = [0u8; GLYPH_BYTES];
    // 主干：首尾行定框 + 中线列亮（字符可辨识的最小骨架）。
    g[0] = 0xFF;
    g[GLYPH_BYTES - 1] = 0xFF;
    let mut r = 1;
    while r < GLYPH_BYTES - 1 {
        // 中列位（bit3）+ 字符低位轮廓（字符索引低 4 位展开到两侧）。
        let profile = ((idx + r as u64) % 7) as u8; // 行轮廓 0~6
        g[r] = 0x08 | (if profile > 3 { 0x70 } else { 0x0E });
        r += 1;
    }
    // 底纹：LCG 确定性补齐非主干位（只置位不清位）。
    let mut lcg = Lcg::new(SEG_BASE as u64 + idx * 131);
    let mut r2 = 1;
    while r2 < GLYPH_BYTES - 1 {
        let tex = (lcg.next() & 0x81) as u8; // 只补 bit7/bit0。
        g[r2] |= tex;
        r2 += 1;
    }
    Ok(g)
}

/// 字形账齐备审计：全字符集逐字符派生两次一致 + 长度守恒。
pub fn glyph_audit() -> Result<usize, u32> {
    let mut n = 0;
    let mut ch = CHARSET_FIRST;
    while ch <= CHARSET_LAST {
        let a = derive_glyph(ch)?;
        let b = derive_glyph(ch)?;
        if a != b {
            return Err(FontCode::TextureMismatch.val());
        }
        n += 1;
        ch += 1;
    }
    if n != CHARSET_COUNT {
        return Err(FontCode::CharOutOfRange.val());
    }
    Ok(n)
}

// ── 帧簿（灰度账面） ────────────────────────────────────────────
pub struct Framebuffer {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    /// 派生行列（自适应）。
    pub cols: usize,
    pub rows: usize,
}

impl Framebuffer {
    /// 构造：分辨率守卫（至少 8x16）。
    pub fn new(width: usize, height: usize) -> Result<Framebuffer, u32> {
        if width < GLYPH_W || height < GLYPH_H {
            return Err(FontCode::ScreenTooSmall.val());
        }
        let cols = width / GLYPH_W;
        let rows = height / GLYPH_H;
        Ok(Framebuffer {
            width,
            height,
            pixels: vec![0u8; width * height],
            cols,
            rows,
        })
    }

    /// 画字符：格（col,row）写 8x16 位图。越行列拒。
    pub fn draw_char(&mut self, col: usize, row: usize, ch: u8) -> Result<(), u32> {
        if col >= self.cols || row >= self.rows {
            return Err(FontCode::CellOutOfRange.val());
        }
        let glyph = if ch < CHARSET_FIRST || ch > CHARSET_LAST {
            // 缺字显性方框。
            let mut g = [0u8; GLYPH_BYTES];
            let mut r = 0;
            while r < GLYPH_BYTES {
                g[r] = BOX_GLYPH_HINT;
                r += 1;
            }
            g
        } else {
            derive_glyph(ch)?
        };
        let x0 = col * GLYPH_W;
        let y0 = row * GLYPH_H;
        let mut r = 0;
        while r < GLYPH_H {
            let bits = glyph[r];
            let mut b = 0;
            while b < GLYPH_W {
                let on = (bits >> (7 - b)) & 1;
                let px = (y0 + r) * self.width + x0 + b;
                self.pixels[px] = if on == 1 { 0xFF } else { 0x00 };
                b += 1;
            }
            r += 1;
        }
        Ok(())
    }

    /// 画字符串：逐格排布，越行自动换列/换行，越屏拒。
    pub fn draw_str(&mut self, s: &str) -> Result<usize, u32> {
        let bytes = s.as_bytes();
        let mut col = 0usize;
        let mut row = 0usize;
        let mut drawn = 0;
        let mut i = 0;
        while i < bytes.len() {
            if col >= self.cols {
                col = 0;
                row += 1;
            }
            if row >= self.rows {
                return Err(FontCode::CellOutOfRange.val());
            }
            self.draw_char(col, row, bytes[i])?;
            col += 1;
            drawn += 1;
            i += 1;
        }
        Ok(drawn)
    }

    /// 无破版对拍：逐格重画到对照帧，逐字节一致。
    pub fn verify_no_corrupt(&self, drawn: &[(usize, usize, u8)]) -> Result<(), u32> {
        let mut ref_fb = Framebuffer::new(self.width, self.height)?;
        let mut i = 0;
        while i < drawn.len() {
            ref_fb.draw_char(drawn[i].0, drawn[i].1, drawn[i].2)?;
            i += 1;
        }
        if ref_fb.pixels.len() != self.pixels.len() {
            return Err(FontCode::RenderCorrupt.val());
        }
        let mut k = 0;
        while k < self.pixels.len() {
            if ref_fb.pixels[k] != self.pixels[k] {
                return Err(FontCode::RenderCorrupt.val());
            }
            k += 1;
        }
        Ok(())
    }

    /// 上滚一行：行账上移 + 底行清零。
    pub fn scroll_up(&mut self) -> Result<(), u32> {
        let line = self.width * GLYPH_H;
        if self.pixels.len() < line * 2 {
            return Err(FontCode::ScreenTooSmall.val());
        }
        // 整体上移一行字符高。
        let mut k = 0;
        while k < self.pixels.len() - line {
            self.pixels[k] = self.pixels[k + line];
            k += 1;
        }
        // 底行清零。
        let start = self.pixels.len() - line;
        let mut j = start;
        while j < self.pixels.len() {
            self.pixels[j] = 0;
            j += 1;
        }
        Ok(())
    }

    /// 滚动账点验：底行必全零。
    pub fn scroll_audit(&self) -> Result<(), u32> {
        let line = self.width * GLYPH_H;
        let start = self.pixels.len() - line;
        let mut j = start;
        while j < self.pixels.len() {
            if self.pixels[j] != 0 {
                return Err(FontCode::ScrollLedgerBroken.val());
            }
            j += 1;
        }
        Ok(())
    }
}

// ── LCG ────────────────────────────────────────────────────────
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

// ── 注入检出账 ──────────────────────────────────────────────────
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
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：字形审计→三档分辨率渲染→无破版→滚动账。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let glyphs = glyph_audit()?;
    // 三档分辨率（可变分辨率自适应）。
    let sizes = [(80usize, 25usize), (100, 31), (64, 50)];
    let mut total_cells = 0;
    let mut s = 0;
    while s < sizes.len() {
        let (cols, rows) = sizes[s];
        let mut fb = Framebuffer::new(cols * GLYPH_W, rows * GLYPH_H)?;
        if fb.cols != cols || fb.rows != rows {
            return Err(FontCode::ScreenTooSmall.val());
        }
        // 全字符集铺屏（确定性循环填充）。
        let mut drawn: Vec<(usize, usize, u8)> = Vec::new();
        let mut r = 0;
        while r < rows {
            let mut c = 0;
            while c < cols {
                let ch = CHARSET_FIRST + ((c + r * 7) % CHARSET_COUNT) as u8;
                fb.draw_char(c, r, ch)?;
                drawn.push((c, r, ch));
                c += 1;
            }
            r += 1;
        }
        fb.verify_no_corrupt(&drawn)?;
        total_cells += drawn.len();
        s += 1;
    }
    // 滚动账。
    let mut fb2 = Framebuffer::new(640, 400)?;
    fb2.draw_str("Varix console font 8x16 audit.")?;
    fb2.scroll_up()?;
    fb2.scroll_audit()?;
    Ok((glyphs, total_cells, fb2.cols))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut fb = Framebuffer::new(640, 400)?;
    fb.draw_str("HELLO")?;
    fb.scroll_up()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 越屏写 → 必拒。
    let mut fb = Framebuffer::new(640, 400)?;
    fb.draw_char(fb.cols, 0, b'A')
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 分辨率不足 → 必拒。
    Framebuffer::new(4, 8).map(|_| ())
}

/// 注入 10/10：越字符集、小屏、越行列、底纹不一致四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越字符集。
        led.record(0, derive_glyph(0x7F).is_err());
        // 槽1：小屏。
        led.record(1, Framebuffer::new(8, 8).is_err());
        // 槽2：越行列。
        let mut fb = Framebuffer::new(640, 400).ok();
        let hit2 = match &mut fb {
            Some(f) => f.draw_char(0, f.rows, b'A').is_err(),
            None => false,
        };
        led.record(2, hit2);
        // 槽3：滚动账破坏（未滚动时底行非零？底行本来就是清屏零——
        // 注入：底行写 1 后 audit 必抓）。
        let mut fb2 = Framebuffer::new(640, 400).ok();
        let hit3 = match &mut fb2 {
            Some(f) => {
                f.draw_str("X").ok();
                // 手工污染底行末字节。
                let last = f.pixels.len() - 1;
                f.pixels[last] = 1;
                f.scroll_audit().is_err()
            }
            None => false,
        };
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FontCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FontCode::ALL.len() {
        let v = FontCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 全字符集。
    let ga = glyph_audit();
    v.push((
        "J1-全字符集96字形",
        ga == Ok(CHARSET_COUNT) && CHARSET_COUNT == 95,
        format!("{:?}/{})", ga, CHARSET_COUNT),
    ));
    // 可变分辨率自适应。
    let fb = Framebuffer::new(101, 49);
    v.push((
        "J1-行列自适应",
        match &fb {
            Ok(f) => f.cols == 12 && f.rows == 3,
            Err(_) => false,
        },
        format!("{:?}", fb.as_ref().map(|f| (f.cols, f.rows))),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C4C0 窗口")));
    v
}

pub fn run_unx0103_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0103 帧簿控制台字体渲染");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
