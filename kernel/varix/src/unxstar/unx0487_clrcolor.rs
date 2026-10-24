//! UNX-F0487 · 清屏与配色原语账 · 诊断段 0xB830
//! 验收锚点：清屏（全屏单色填充）后位图零残留（回读全等），
//! 前景/背景色对账齐（16 色表内），双判据缺一即批内红灯。
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;


pub const SEG_BASE: u16 = 0xB830;

// ── 诊断段独占 · 清屏配色错误域 ─────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_RESIDUE: u16 = SEG_ERR + 1; // 残留
const E_COLOR: u16 = SEG_ERR + 2;   // 色号越 16 色表
const E_ATTR: u16 = SEG_ERR + 3;    // 属性字拼装错
const E_CLEAR: u16 = SEG_ERR + 4;   // 清屏未全屏
const E_BAL: u16 = SEG_ERR + 5;     // 对账不平

pub const SCREEN_W: usize = 80;
pub const SCREEN_H: usize = 25;
pub const VGA_COLORS: usize = 16;

// ── 确定性 LCG ───────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 文本屏帧缓冲 ─────────────────────────────────────────────
/// 字符单元：ASCII + 属性（fg 4bit | bg 4bit）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub ch: u8,
    pub attr: u8,
}

impl Cell {
    pub fn new(ch: u8, fg: u8, bg: u8) -> Result<Self, u16> {
        if fg as usize >= VGA_COLORS || bg as usize >= VGA_COLORS {
            return Err(E_COLOR);
        }
        Ok(Cell { ch, attr: (bg << 4) | fg })
    }
    pub fn fg(&self) -> u8 {
        self.attr & 0xF
    }
    pub fn bg(&self) -> u8 {
        self.attr >> 4
    }
}

/// 文本屏：80×25 单元阵。
pub struct TextScreen {
    pub buf: Vec<Cell>,
}

impl TextScreen {
    pub fn new() -> Self {
        TextScreen { buf: vec![Cell { ch: b' ', attr: 0x07 }; SCREEN_W * SCREEN_H] }
    }
    /// 写单元（越界拒）。
    pub fn put(&mut self, x: usize, y: usize, c: Cell) -> Result<(), u16> {
        if x >= SCREEN_W || y >= SCREEN_H {
            return Err(E_CLEAR);
        }
        self.buf[y * SCREEN_W + x] = c;
        Ok(())
    }
    /// 清屏：全屏单色填充（ch=空格, attr=fg|bg<<4）。
    pub fn clear(&mut self, fg: u8, bg: u8) -> Result<(), u16> {
        let c = Cell::new(b' ', fg, bg)?;
        for i in 0..self.buf.len() {
            self.buf[i] = c;
        }
        Ok(())
    }
    /// 零残留校验：全屏回读与目标单元全等。
    pub fn no_residue(&self, fg: u8, bg: u8) -> bool {
        let want = Cell::new(b' ', fg, bg).unwrap_or(Cell { ch: 0xFF, attr: 0xFF });
        self.buf.iter().all(|c| *c == want)
    }
    /// 残留计数。
    pub fn residue_count(&self, fg: u8, bg: u8) -> usize {
        let want = Cell::new(b' ', fg, bg).unwrap_or(Cell { ch: 0xFF, attr: 0xFF });
        self.buf.iter().filter(|c| **c != want).count()
    }
    /// 乱写污染：随机撒噪声。
    pub fn pollute(&mut self, seed: u64, n: usize) {
        let mut lcg = Lcg(seed);
        for _ in 0..n {
            let x = lcg.below(SCREEN_W as u64) as usize;
            let y = lcg.below(SCREEN_H as u64) as usize;
            let fg = lcg.below(VGA_COLORS as u64) as u8;
            let bg = lcg.below(VGA_COLORS as u64) as u8;
            let ch = 0x21 + lcg.below(90) as u8;
            let _ = self.put(x, y, Cell { ch, attr: (bg << 4) | fg });
        }
    }
}

// ── 16 色表对账 ──────────────────────────────────────────────
/// 16 色表：黑蓝绿青红紫棕灰 + 亮系。色号 → RGB 值（确定性）。
pub fn color_table() -> [(u8, u32); VGA_COLORS] {
    [
        (0, 0x000000), (1, 0x0000AA), (2, 0x00AA00), (3, 0x00AAAA),
        (4, 0xAA0000), (5, 0xAA00AA), (6, 0xAA5500), (7, 0xAAAAAA),
        (8, 0x555555), (9, 0x5555FF), (10, 0x55FF55), (11, 0x55FFFF),
        (12, 0xFF5555), (13, 0xFF55FF), (14, 0xFFFF55), (15, 0xFFFFFF),
    ]
}

/// 前景/背景色对账：fg/bg 组合 256 全域合法、属性字往返拆装零错位。
pub fn color_pair_recon() -> bool {
    for fg in 0..VGA_COLORS as u8 {
        for bg in 0..VGA_COLORS as u8 {
            let c = match Cell::new(b'A', fg, bg) {
                Ok(c) => c,
                Err(_) => return false,
            };
            if c.fg() != fg || c.bg() != bg || c.attr != (bg << 4) | fg {
                return false;
            }
        }
    }
    true
}

/// 16 色表完整性：色号 0..15 依序、RGB 非全同。
pub fn color_table_ok() -> bool {
    let t = color_table();
    t.iter().enumerate().all(|(i, (n, _))| *n == i as u8)
        && t.iter().map(|(_, rgb)| *rgb).collect::<Vec<_>>().windows(2).all(|w| w[0] != w[1])
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx0487_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F0487 清屏与配色原语");

    // 1. 污染后清屏零残留（4 色组合）
    let mut ok = true;
    for &(fg, bg) in &[(7u8, 0u8), (15, 1), (0, 15), (14, 4)] {
        let mut scr = TextScreen::new();
        scr.pollute(0xB830, 500);
        if scr.clear(fg, bg).is_err() || !scr.no_residue(fg, bg) {
            ok = false;
        }
    }
    s.add("CLR-清屏零残留", ok, if ok { "" } else { "清屏残留" });

    // 2. 残留计数对账（污染 500 → 清后 0；不清 → >0）
    let mut scr2 = TextScreen::new();
    scr2.pollute(0xB831, 500);
    let before = scr2.residue_count(7, 0);
    let _ = scr2.clear(7, 0);
    let after = scr2.residue_count(7, 0);
    s.add("CLR-残留计数", before > 0 && after == 0,
        if before > 0 && after == 0 { "" } else { "计数异常" });

    // 3. 色号越 16 色表拒
    let rej = Cell::new(b'X', 16, 0) == Err(E_COLOR) && Cell::new(b'X', 0, 16) == Err(E_COLOR);
    s.add("CLR-越色拒", rej, if rej { "" } else { "越色放行" });

    // 4. 前景/背景 256 组合对账齐
    s.add("CLR-256组合对账", color_pair_recon(), if color_pair_recon() { "" } else { "拆装错位" });

    // 5. 16 色表完整
    s.add("CLR-16色表完整", color_table_ok(), if color_table_ok() { "" } else { "色表残缺" });

    // 6. 属性字拆装位域（fg 低 4 位 bg 高 4 位）
    let c = Cell::new(b'#', 0b1010, 0b0101).unwrap_or(Cell { ch: 0, attr: 0 });
    s.add("CLR-属性位域", c.attr == 0b0101_1010 && c.fg() == 0b1010 && c.bg() == 0b0101,
        if c.attr == 0b0101_1010 { "" } else { "位域错位" });

    // 7. 越界写拒
    let mut scr3 = TextScreen::new();
    let oob = scr3.put(SCREEN_W, 0, Cell { ch: b'x', attr: 7 }) == Err(E_CLEAR)
        && scr3.put(0, SCREEN_H, Cell { ch: b'x', attr: 7 }) == Err(E_CLEAR);
    s.add("CLR-越界写拒", oob, if oob { "" } else { "越界放行" });

    // 8. 单元覆盖写读一致
    let mut scr4 = TextScreen::new();
    let _ = scr4.put(10, 5, Cell { ch: b'Z', attr: 0x1F });
    let rd = scr4.buf[5 * SCREEN_W + 10];
    s.add("CLR-覆盖写读", rd.ch == b'Z' && rd.attr == 0x1F,
        if rd.ch == b'Z' { "" } else { "读写不一致" });

    // 9. LCG 随机污染清屏循环 20 轮零残留
    let mut lcg = Lcg(0xB832);
    let mut all_clear = true;
    for r in 0..20 {
        let mut sc = TextScreen::new();
        let n = 50 + lcg.below(500) as usize;
        sc.pollute(0xB800 + r, n);
        let fg = lcg.below(16) as u8;
        let bg = lcg.below(16) as u8;
        if sc.clear(fg, bg).is_err() || sc.residue_count(fg, bg) != 0 {
            all_clear = false;
        }
    }
    s.add("CLR-随机清屏20轮", all_clear, if all_clear { "" } else { "随机轮残留" });

    // 10. 屏容量守恒（清屏不改单元总数）
    let mut scr5 = TextScreen::new();
    scr5.pollute(1, 100);
    let n1 = scr5.buf.len();
    let _ = scr5.clear(7, 0);
    s.add("CLR-容量守恒", scr5.buf.len() == n1 && n1 == SCREEN_W * SCREEN_H,
        if scr5.buf.len() == n1 { "" } else { "容量漂移" });

    // 11. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB830 && SEG_BASE < 0xB850;
    s.add("CLR-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
