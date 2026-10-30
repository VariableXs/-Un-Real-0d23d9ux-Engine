//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0487 · 清屏与配色原语 · 0x6C7C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2）：
//!   ① 纯功能代码 ≥200 行；
//!   ② UNX-F0487-J1 清屏（全屏单色填充）后位图零残留（回读全等），
//!     前景/背景色对账齐（16 色表内）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 16 色表闭集（VGA 文本色）：黑/蓝/绿/青/红/品红/棕/浅灰 +
//!      8 亮色（0x0~0xF），越表色拒；
//!    - 清屏原语：全屏单色填充（字符 0x20 + 属性色），回读全等判
//!      据（逐格 attr/char 对拍——零残留）；
//!    - 配色原语：前景/背景对账（fg 0xF 域、bg 0x7 域——背景不取
//!      亮色位，冻结规则）；属性字节 = (bg<<4)|fg；
//!    - 渲染污染检测：清屏前写入杂色，清屏后必须零残留；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C7C0，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C7C0;

// ── 16 色表闭集 ────────────────────────────────────────────────
pub const COLOR_NAMES: [&str; 16] = [
    "黑", "蓝", "绿", "青", "红", "品红", "棕", "浅灰",
    "深灰", "亮蓝", "亮绿", "亮青", "亮红", "亮品红", "黄", "白",
];
/// 背景色上界（0x7：背景不取亮色位，冻结规则）。
pub const BG_MAX: u8 = 0x7;
/// 屏幕规格（80x25 文本）。
pub const SCR_COLS: usize = 80;
pub const SCR_ROWS: usize = 25;
/// 空格字符。
pub const SPACE_CHAR: u8 = 0x20;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrCode {
    /// 色越 16 色表
    ColorOutOfRange,
    /// 背景取亮色位（冻结规则破坏）
    BgBrightForbidden,
    /// 色名空（16 色表齐备破坏）
    ColorNameBlank,
    /// 清屏残留（回读不全等）
    ClearResidue,
    /// 越屏写
    CellOutOfRange,
    /// 属性字节合成错（bg<<4|fg 对拍失败）
    AttrSynthMismatch,
    /// 清屏字符错（非空格）
    ClearCharWrong,
    /// 双跑不一致
    NotReproducible,
}

impl ScrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ScrCode::ColorOutOfRange => "色越 16 色表：0x0~0xF",
            ScrCode::BgBrightForbidden => "背景取亮色位：>0x7 拒",
            ScrCode::ColorNameBlank => "色名空：齐备破坏",
            ScrCode::ClearResidue => "清屏残留：回读不全等",
            ScrCode::CellOutOfRange => "越屏写",
            ScrCode::AttrSynthMismatch => "属性字节合成错",
            ScrCode::ClearCharWrong => "清屏字符错：非空格",
            ScrCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ScrCode; 8] = [
        ScrCode::ColorOutOfRange,
        ScrCode::BgBrightForbidden,
        ScrCode::ColorNameBlank,
        ScrCode::ClearResidue,
        ScrCode::CellOutOfRange,
        ScrCode::AttrSynthMismatch,
        ScrCode::ClearCharWrong,
        ScrCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ScrCode::ALL.len() {
        if ScrCode::ALL[i].val() == code {
            return ScrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C7C0 段闭集内"
}

// ── 配色原语 ───────────────────────────────────────────────────
/// 前景/背景合法核。
pub fn color_legal(fg: u8, bg: u8) -> Result<(), u32> {
    if fg > 0xF {
        return Err(ScrCode::ColorOutOfRange.val());
    }
    if bg > BG_MAX {
        return Err(ScrCode::BgBrightForbidden.val());
    }
    Ok(())
}

/// 属性字节合成：(bg<<4)|fg。
pub fn attr_of(fg: u8, bg: u8) -> Result<u8, u32> {
    color_legal(fg, bg)?;
    Ok((bg << 4) | fg)
}

// ── 文本屏 ─────────────────────────────────────────────────────
pub struct TextScreen {
    pub chars: Vec<u8>,
    pub attrs: Vec<u8>,
}

impl TextScreen {
    pub fn new() -> TextScreen {
        let n = SCR_COLS * SCR_ROWS;
        TextScreen { chars: vec![0u8; n], attrs: vec![0u8; n] }
    }

    /// 写格。
    pub fn put(&mut self, col: usize, row: usize, ch: u8, attr: u8) -> Result<(), u32> {
        if col >= SCR_COLS || row >= SCR_ROWS {
            return Err(ScrCode::CellOutOfRange.val());
        }
        let i = row * SCR_COLS + col;
        self.chars[i] = ch;
        self.attrs[i] = attr;
        Ok(())
    }

    /// 读格。
    pub fn get(&self, col: usize, row: usize) -> Result<(u8, u8), u32> {
        if col >= SCR_COLS || row >= SCR_ROWS {
            return Err(ScrCode::CellOutOfRange.val());
        }
        let i = row * SCR_COLS + col;
        Ok((self.chars[i], self.attrs[i]))
    }

    /// 清屏：全屏空格 + 单色属性。
    pub fn clear(&mut self, fg: u8, bg: u8) -> Result<(), u32> {
        let attr = attr_of(fg, bg)?;
        let mut r = 0;
        while r < SCR_ROWS {
            let mut c = 0;
            while c < SCR_COLS {
                self.put(c, r, SPACE_CHAR, attr)?;
                c += 1;
            }
            r += 1;
        }
        Ok(())
    }

    /// 零残留判据：逐格回读（字符=空格、属性=目标色）全等。
    pub fn clear_audit(&self, fg: u8, bg: u8) -> Result<usize, u32> {
        let attr = attr_of(fg, bg)?;
        let mut n = 0;
        let mut r = 0;
        while r < SCR_ROWS {
            let mut c = 0;
            while c < SCR_COLS {
                let (ch, at) = self.get(c, r)?;
                if ch != SPACE_CHAR {
                    return Err(ScrCode::ClearCharWrong.val());
                }
                if at != attr {
                    return Err(ScrCode::ClearResidue.val());
                }
                n += 1;
                c += 1;
            }
            r += 1;
        }
        Ok(n)
    }
}

// ── 16 色表齐备账 ──────────────────────────────────────────────
pub fn color_table_ready() -> bool {
    if COLOR_NAMES.len() != 16 {
        return false;
    }
    let mut i = 0;
    while i < 16 {
        if COLOR_NAMES[i].len() == 0 {
            return false;
        }
        i += 1;
    }
    true
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

/// 黄金全链：污染→清屏→零残留→配色对账（16 组 fg×8 组 bg）。
pub fn golden_run() -> Result<(usize, usize, u8), u32> {
    if !color_table_ready() {
        return Err(ScrCode::ColorNameBlank.val());
    }
    let mut scr = TextScreen::new();
    // 污染：随机杂色。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < SCR_ROWS {
        let mut c = 0;
        while c < SCR_COLS {
            scr.put(c, r, b'#', (lcg.below(256) as u8))?;
            c += 1;
        }
        r += 1;
    }
    // 清屏（白底黑字...账面 fg=浅灰 bg=黑）。
    scr.clear(0x7, 0x0)?;
    let cells = scr.clear_audit(0x7, 0x0)?;
    if cells != SCR_COLS * SCR_ROWS {
        return Err(ScrCode::ClearResidue.val());
    }
    // 配色对账：16 fg × 8 bg 全合成。
    let mut pairs = 0;
    let mut fg = 0u8;
    while fg <= 0xF {
        let mut bg = 0u8;
        while bg <= BG_MAX {
            let a = attr_of(fg, bg)?;
            if a != (bg << 4) | fg {
                return Err(ScrCode::AttrSynthMismatch.val());
            }
            pairs += 1;
            bg += 1;
        }
        fg += 1;
    }
    Ok((cells, pairs, BG_MAX))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut scr = TextScreen::new();
    scr.clear(0xF, 0x0)?;
    scr.clear_audit(0xF, 0x0).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 背景亮色 → 必拒。
    attr_of(0x7, 0x8).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 前景越表 → 必拒。
    attr_of(0x10, 0x0).map(|_| ())
}

/// 注入 10/10：背景亮色、前景越表、清屏残留、越屏写四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        led.record(0, color_legal(0x7, 0x8).is_err());
        led.record(1, color_legal(0x10, 0x0).is_err());
        // 清屏残留：清后污染一格。
        let mut scr = TextScreen::new();
        scr.clear(0x7, 0x0).ok();
        scr.put(0, 0, b'X', 0x0F).ok();
        led.record(2, scr.clear_audit(0x7, 0x0).is_err());
        // 越屏写。
        let mut scr2 = TextScreen::new();
        led.record(3, scr2.put(80, 0, b'A', 0x07).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ScrCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ScrCode::ALL.len() {
        let v = ScrCode::ALL[i].val();
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
    // 16 色表齐备。
    v.push((
        "J1-16色表齐备",
        color_table_ready() && COLOR_NAMES.len() == 16,
        format!("{:?}", COLOR_NAMES.len()),
    ));
    // 零残留。
    let mut scr = TextScreen::new();
    scr.clear(0x2, 0x0).ok();
    v.push((
        "J1-清屏零残留",
        scr.clear_audit(0x2, 0x0) == Ok(SCR_COLS * SCR_ROWS),
        format!("{} 格全等", SCR_COLS * SCR_ROWS),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C7C0 窗口")));
    v
}

pub fn run_unx0487_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0487 清屏与配色原语");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
