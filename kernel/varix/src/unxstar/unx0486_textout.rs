//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0486 · 文本输出原语（字符/字符串/滚动） · 0x75B00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批4-J）：
//!   字符/字符串/滚动三原语，滚动后行守恒（Σ行=行数上限），滚动丢弃
//!   行入历史账（历史账容量冻结 64）
//! 设计要点：
//!   1. 位图文本账：25 行 × 80 列冻结口径，格位 = 行×列宽（checked
//!      乘法寻址），字形闭集 ASCII 0x00~0x7F（128 枚），越界显性拒；
//!   2. 三原语同口径：putc 单格写入、puts 逐字符走 putc 同一闸
//!      （越宽显性拒、零截断静默）、scroll 顶行丢弃入历史账；
//!   3. 行守恒：滚动后可见行数恒 = 25（行数上限），每行列宽恒 80；
//!   4. 历史账：容量冻结 64，溢出丢最旧；kept + dropped ≡ pushed
//!      守恒式逐次滚动后复算，破坏即红线；
//!   5. 统一位图回读全等断言：三原语操作后回读整幅位图与期望逐格
//!      全等（零截断、零漂移）；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75B00，AI-VX7-1104 批4-J 独占槽）。
pub const SEG_BASE: u32 = 0x75B00;

// ── 冻结口径 ────────────────────────────────────────────────────
/// 可见行数上限（25 行，VGA 文本口径）。
pub const ROWS: usize = 25;
/// 列宽上限（80 列，VGA 文本口径）。
pub const COLS: usize = 80;
/// 历史账容量（冻结 64，溢出丢最旧）。
pub const HISTORY_CAP: usize = 64;
/// 字形闭集宽（ASCII 128 枚）。
pub const GLYPH_SET: usize = 128;
/// 空格字形（清行/末行补白口径）。
pub const SPACE_GLYPH: u8 = 0x20;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxCode {
    /// 行号越界
    RowOver,
    /// 列号越界
    ColOver,
    /// 字形越闭集（≥128）
    GlyphOver,
    /// 字符串写入越宽（越出列宽上限）
    WriteOverrun,
    /// 历史账守恒破坏（kept+dropped ≠ pushed）
    ConservationBreach,
    /// 行守恒破坏（Σ行 ≠ 行数上限）
    RowSumBreach,
    /// 回读全等失配
    ReadbackMismatch,
    /// 末行未补白
    BlankRowBreach,
    /// 注入漏抓
    InjectLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl TxCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TxCode::RowOver => "行号越界：可见面仅 25 行，建议以 ROWS 为界先判后写",
            TxCode::ColOver => "列号越界：可见面仅 80 列，建议以 COLS 为界先判后写",
            TxCode::GlyphOver => "字形越闭集：仅 ASCII 0x00~0x7F，建议对非 ASCII 入参先行过滤",
            TxCode::WriteOverrun => "字符串写入越宽：起点+长度超出列宽，建议分段写入或回退起点",
            TxCode::ConservationBreach => "历史账守恒破坏：kept+dropped ≠ pushed，建议逐次滚动后即席复算",
            TxCode::RowSumBreach => "行守恒破坏：滚动后可见行数 ≠ 25，建议检查移位循环边界",
            TxCode::ReadbackMismatch => "回读全等失配：位图与期望不一致，建议逐格对账定位首差异格位",
            TxCode::BlankRowBreach => "末行未补白：滚动后末行应为空格，建议滚动尾部统一补 SPACE_GLYPH",
            TxCode::InjectLeaked => "注入漏抓：违约样本未被拦截，建议复核守卫谓词方向",
            TxCode::NotReproducible => "双跑不一致：复现红线，建议排查非确定性来源（随机/墙钟/迭代序）",
        }
    }
    pub const ALL: [TxCode; 10] = [
        TxCode::RowOver,
        TxCode::ColOver,
        TxCode::GlyphOver,
        TxCode::WriteOverrun,
        TxCode::ConservationBreach,
        TxCode::RowSumBreach,
        TxCode::ReadbackMismatch,
        TxCode::BlankRowBreach,
        TxCode::InjectLeaked,
        TxCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，未知码显性文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TxCode::ALL.len() {
        if let Some(c) = TxCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75B00 段闭集内，建议核对诊断码表"
}

// ── 历史账（滚动丢弃行归档） ────────────────────────────────────
/// 历史账：容量 64 冻结，溢出丢最旧；kept + dropped ≡ pushed。
#[derive(Clone, Debug)]
pub struct HistoryLedger {
    pub kept: Vec<Vec<u8>>,
    /// 已因溢出被丢弃的最旧行数。
    pub dropped: u64,
    /// 累计入账行数（pushed）。
    pub pushed: u64,
}

impl HistoryLedger {
    pub fn new() -> HistoryLedger {
        HistoryLedger { kept: Vec::new(), dropped: 0, pushed: 0 }
    }

    /// 入账一行（容量满则丢最旧并记账）。
    pub fn push(&mut self, row: &[u8]) {
        if self.kept.len() >= HISTORY_CAP {
            // 丢最旧：移除队首并计入 dropped。
            let mut shifted = Vec::new();
            let mut i = 1;
            while i < self.kept.len() {
                if let Some(r) = self.kept.get(i) {
                    shifted.push(r.clone());
                }
                i += 1;
            }
            self.kept = shifted;
            self.dropped = match self.dropped.checked_add(1) {
                Some(v) => v,
                None => self.dropped,
            };
        }
        self.kept.push(row.to_vec());
        self.pushed = match self.pushed.checked_add(1) {
            Some(v) => v,
            None => self.pushed,
        };
    }

    /// 守恒复算：kept + dropped ≡ pushed。
    pub fn conservation_ok(&self) -> bool {
        let kept_n = self.kept.len() as u64;
        match kept_n.checked_add(self.dropped) {
            Some(sum) => sum == self.pushed,
            None => false,
        }
    }
}

impl Default for HistoryLedger {
    fn default() -> HistoryLedger {
        HistoryLedger::new()
    }
}

// ── 文本屏（25×80 位图账） ──────────────────────────────────────
/// 文本输出屏：字符位图 + 滚动历史账。
#[derive(Clone, Debug)]
pub struct TextScreen {
    cells: Vec<u8>,
    pub history: HistoryLedger,
    /// 原语调用计数（putc/puts/scroll）。
    pub putc_calls: u64,
    pub puts_calls: u64,
    pub scroll_calls: u64,
}

impl TextScreen {
    /// 新屏：全幅空格补白。
    pub fn new() -> TextScreen {
        let mut cells = Vec::new();
        let mut i = 0;
        let total = match ROWS.checked_mul(COLS) {
            Some(t) => t,
            None => 0,
        };
        while i < total {
            cells.push(SPACE_GLYPH);
            i += 1;
        }
        TextScreen {
            cells,
            history: HistoryLedger::new(),
            putc_calls: 0,
            puts_calls: 0,
            scroll_calls: 0,
        }
    }

    /// 格位寻址（checked 乘加，越界返回 None）。
    fn cell_at(row: usize, col: usize) -> Option<usize> {
        let base = row.checked_mul(COLS)?;
        base.checked_add(col)
    }

    /// 单格读（越界 None）。
    pub fn cell(&self, row: usize, col: usize) -> Option<u8> {
        let idx = match TextScreen::cell_at(row, col) {
            Some(i) => i,
            None => return None,
        };
        self.cells.get(idx).copied()
    }

    /// 单格写（越界 None）。
    fn set_cell(&mut self, row: usize, col: usize, glyph: u8) -> Option<()> {
        let idx = match TextScreen::cell_at(row, col) {
            Some(i) => i,
            None => return None,
        };
        match self.cells.get_mut(idx) {
            Some(slot) => {
                *slot = glyph;
                Some(())
            }
            None => None,
        }
    }

    /// 原语一：putc 字符写入（行/列/字形三闸）。
    pub fn putc(&mut self, row: usize, col: usize, glyph: u8) -> Result<(), u32> {
        if row >= ROWS {
            return Err(TxCode::RowOver.val());
        }
        if col >= COLS {
            return Err(TxCode::ColOver.val());
        }
        if glyph as usize >= GLYPH_SET {
            return Err(TxCode::GlyphOver.val());
        }
        match self.set_cell(row, col, glyph) {
            Some(()) => {
                self.putc_calls = match self.putc_calls.checked_add(1) {
                    Some(v) => v,
                    None => self.putc_calls,
                };
                Ok(())
            }
            None => Err(TxCode::RowOver.val()),
        }
    }

    /// 原语二：puts 字符串写入（逐字符走 putc 同口径，先验总宽）。
    pub fn puts(&mut self, row: usize, col: usize, s: &str) -> Result<usize, u32> {
        if row >= ROWS {
            return Err(TxCode::RowOver.val());
        }
        if col >= COLS {
            return Err(TxCode::ColOver.val());
        }
        let bytes = s.as_bytes();
        // 越宽先验：col+len ≤ COLS，零截断静默。
        let end = match col.checked_add(bytes.len()) {
            Some(e) => e,
            None => return Err(TxCode::WriteOverrun.val()),
        };
        if end > COLS {
            return Err(TxCode::WriteOverrun.val());
        }
        // 字形预检（全串 ASCII 闭集才开写，避免半串落账）。
        let mut k = 0;
        while k < bytes.len() {
            match bytes.get(k) {
                Some(b) => {
                    if *b as usize >= GLYPH_SET {
                        return Err(TxCode::GlyphOver.val());
                    }
                }
                None => return Err(TxCode::GlyphOver.val()),
            }
            k += 1;
        }
        let mut written = 0;
        let mut c = col;
        while c < end {
            let g = match bytes.get(written) {
                Some(b) => *b,
                None => break,
            };
            self.putc(row, c, g)?;
            written += 1;
            c += 1;
        }
        self.puts_calls = match self.puts_calls.checked_add(1) {
            Some(v) => v,
            None => self.puts_calls,
        };
        Ok(written)
    }

    /// 原语三：scroll 顶行丢弃入历史账，全屏上移，末行补白。
    pub fn scroll(&mut self) -> Result<(), u32> {
        // 顶行入历史账（先拷贝再移位）。
        let mut top = Vec::new();
        let mut c = 0;
        while c < COLS {
            match self.cell(0, c) {
                Some(g) => top.push(g),
                None => return Err(TxCode::RowSumBreach.val()),
            }
            c += 1;
        }
        self.history.push(&top);
        // 全屏上移一行。
        let mut r = 0;
        while r + 1 < ROWS {
            let mut c2 = 0;
            while c2 < COLS {
                let g = match self.cell(r + 1, c2) {
                    Some(v) => v,
                    None => return Err(TxCode::RowSumBreach.val()),
                };
                match self.set_cell(r, c2, g) {
                    Some(()) => {}
                    None => return Err(TxCode::RowSumBreach.val()),
                }
                c2 += 1;
            }
            r += 1;
        }
        // 末行补白。
        let mut c3 = 0;
        while c3 < COLS {
            match self.set_cell(ROWS - 1, c3, SPACE_GLYPH) {
                Some(()) => {}
                None => return Err(TxCode::BlankRowBreach.val()),
            }
            c3 += 1;
        }
        self.scroll_calls = match self.scroll_calls.checked_add(1) {
            Some(v) => v,
            None => self.scroll_calls,
        };
        Ok(())
    }

    /// 行守恒：可见行数 = 上限且每行列宽 = 冻结值。
    pub fn row_sum_ok(&self) -> bool {
        if self.cells.len() != ROWS * COLS {
            return false;
        }
        // 逐行抽样可寻址（首格/末格必在闭集内）。
        let mut r = 0;
        while r < ROWS {
            if self.cell(r, 0).is_none() || self.cell(r, COLS - 1).is_none() {
                return false;
            }
            r += 1;
        }
        self.history.conservation_ok()
    }

    /// 整幅回读（全等断言口径）。
    pub fn readback(&self) -> Vec<u8> {
        self.cells.clone()
    }

    /// 期望位图构造：全空格底 + 逐行写入语料（对拍基准）。
    pub fn expect_bitmap(rows_written: &[(usize, usize, &str)]) -> Vec<u8> {
        let mut want = Vec::new();
        let total = match ROWS.checked_mul(COLS) {
            Some(t) => t,
            None => 0,
        };
        let mut i = 0;
        while i < total {
            want.push(SPACE_GLYPH);
            i += 1;
        }
        let mut w = 0;
        while w < rows_written.len() {
            if let Some((r, c, s)) = rows_written.get(w) {
                let bytes = s.as_bytes();
                let mut k = 0;
                while k < bytes.len() {
                    if let Some(b) = bytes.get(k) {
                        if let Some(idx) = TextScreen::cell_at(*r, *c + k) {
                            if let Some(slot) = want.get_mut(idx) {
                                *slot = *b;
                            }
                        }
                    }
                    k += 1;
                }
            }
            w += 1;
        }
        want
    }

    /// 回读全等断言。
    pub fn readback_eq(&self, want: &[u8]) -> Result<(), u32> {
        if self.cells.len() != want.len() {
            return Err(TxCode::ReadbackMismatch.val());
        }
        let mut i = 0;
        while i < want.len() {
            let a = match self.cells.get(i) {
                Some(v) => *v,
                None => return Err(TxCode::ReadbackMismatch.val()),
            };
            let b = match want.get(i) {
                Some(v) => *v,
                None => return Err(TxCode::ReadbackMismatch.val()),
            };
            if a != b {
                return Err(TxCode::ReadbackMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }
}

impl Default for TextScreen {
    fn default() -> TextScreen {
        TextScreen::new()
    }
}

// ── 语料闭集（ASCII 确定性语料，列宽 80 内） ────────────────────
pub const CORPUS: [&str; 5] = [
    "VARIX TEXTOUT PUTCLINE-01 PRIMITIVE CHECK",
    "VARIX TEXTOUT PUTSLINE-02 PRIMITIVE CHECK",
    "VARIX TEXTOUT SCROLLLINE-03 PRIMITIVE CHECK",
    "VARIX TEXTOUT HISTORY-04 LEDGER CHECK",
    "VARIX TEXTOUT CONSERVE-05 LEDGER CHECK",
];

// ── 黄金全链：三原语→行守恒→历史账守恒→回读全等→容量溢出────────
pub fn golden_run() -> Result<(u64, u64, u64, u64), u32> {
    let mut s = TextScreen::new();
    // puts 五行语料。
    let mut k = 0;
    while k < CORPUS.len() {
        let line = match CORPUS.get(k) {
            Some(x) => *x,
            None => return Err(TxCode::ReadbackMismatch.val()),
        };
        s.puts(k + 3, 2, line)?;
        k += 1;
    }
    // 回读全等（写入后即拍）。
    let mut pairs: Vec<(usize, usize, &str)> = Vec::new();
    let mut k2 = 0;
    while k2 < CORPUS.len() {
        if let Some(line) = CORPUS.get(k2) {
            pairs.push((k2 + 3, 2, *line));
        }
        k2 += 1;
    }
    let want = TextScreen::expect_bitmap(&pairs);
    s.readback_eq(&want)?;
    // 滚动 70 次：历史账溢出（70 > 64），守恒仍须成立。
    let mut r = 0;
    while r < 70 {
        s.scroll()?;
        r += 1;
    }
    if !s.row_sum_ok() {
        return Err(TxCode::RowSumBreach.val());
    }
    if !s.history.conservation_ok() {
        return Err(TxCode::ConservationBreach.val());
    }
    // 守恒口径：pushed=70，dropped=70-64=6，kept=64。
    if s.history.pushed != 70 || s.history.dropped != 6 || s.history.kept.len() != HISTORY_CAP {
        return Err(TxCode::ConservationBreach.val());
    }
    // putc 抽写一格后回读比对。
    s.putc(0, 0, b'V')?;
    let mut want2 = s.readback();
    if let Some(slot) = want2.get_mut(0) {
        *slot = b'V';
    }
    s.readback_eq(&want2)?;
    Ok((s.history.pushed, s.history.dropped, s.putc_calls, s.scroll_calls))
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 口径冻结谓词。
fn frozen_dims_ok() -> bool {
    ROWS == 25 && COLS == 80 && HISTORY_CAP == 64 && GLYPH_SET == 128
}

/// 语料闭集谓词：全部 ASCII 且列宽内可写。
fn corpus_ok() -> bool {
    let mut i = 0;
    while i < CORPUS.len() {
        match CORPUS.get(i) {
            Some(s) => {
                if s.is_empty() || s.len() + 2 > COLS {
                    return false;
                }
                let bytes = s.as_bytes();
                let mut k = 0;
                while k < bytes.len() {
                    match bytes.get(k) {
                        Some(b) => {
                            if *b as usize >= GLYPH_SET {
                                return false;
                            }
                        }
                        None => return false,
                    }
                    k += 1;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 滚动序列守恒谓词：任意滚动次数后守恒式均成立。
fn scroll_conservation_ok() -> bool {
    let mut s = TextScreen::new();
    let mut r = 0;
    while r <= HISTORY_CAP + 10 {
        if s.scroll().is_err() {
            return false;
        }
        if !s.history.conservation_ok() || !s.row_sum_ok() {
            return false;
        }
        r += 1;
    }
    true
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A（正向）：putc + puts 同格对拍。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut a = TextScreen::new();
    let mut b = TextScreen::new();
    a.putc(0, 5, b'X')?;
    b.puts(0, 5, "X")?;
    if a.readback() != b.readback() {
        return Err(TxCode::ReadbackMismatch.val());
    }
    Ok(())
}

/// 序 B（越字形必拒）：字形 128 出闭集。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut s = TextScreen::new();
    s.putc(0, 0, 128)
}

/// 序 C（越宽必拒）：79 列起写 2 字节。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut s = TextScreen::new();
    s.puts(0, 79, "AB").map(|_| ())
}

// ── 注入轮（LCG 驱动 rounds × 四槽） ────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 槽0：越行。
        let mut s0 = TextScreen::new();
        led.record(0, s0.putc(ROWS, 0, b'A').is_err());
        // 槽1：越列。
        let mut s1 = TextScreen::new();
        led.record(1, s1.putc(0, COLS, b'A').is_err());
        // 槽2：越字形。
        let mut s2 = TextScreen::new();
        led.record(2, s2.putc(0, 0, 0xFF).is_err());
        // 槽3：越宽写入。
        let mut s3 = TextScreen::new();
        led.record(3, s3.puts(1, 78, "LONG-OVERRUN").is_err());
        r += 1;
    }
    Ok(led)
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

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TxCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（窗口 + 对齐 + 区间三闸） ──────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < TxCode::ALL.len() {
        if let Some(c) = TxCode::ALL.get(i) {
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
        format!("25×80 历史账={} 字形集={}", HISTORY_CAP, GLYPH_SET),
    ));
    // 2. 语料闭集。
    v.push((
        "J1-语料闭集",
        corpus_ok(),
        format!("{} 行 ASCII 语料全在列宽内", CORPUS.len()),
    ));
    // 3. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((pushed, dropped, _, _)) => *pushed == 70 && *dropped == 6,
            Err(_) => false,
        },
        match &g {
            Ok((pushed, dropped, pc, sc)) => {
                format!("pushed={} dropped={} putc={} scroll={}", pushed, dropped, pc, sc)
            }
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 4. 三原语同口径（putc/puts 同格对拍）。
    v.push((
        "J1-三原语同口径",
        probe_seq_a().is_ok(),
        String::from("putc 与 puts 同格逐位全等"),
    ));
    // 5. 滚动后行守恒。
    v.push((
        "J1-滚动行守恒",
        scroll_conservation_ok(),
        format!("滚动 0~{} 次全程 Σ行={} 守恒", HISTORY_CAP + 10, ROWS),
    ));
    // 6. 历史账容量冻结 + 溢出丢最旧。
    let hist_ok = {
        let mut s = TextScreen::new();
        let mut r = 0;
        while r < HISTORY_CAP + 6 {
            let _ = s.scroll();
            r += 1;
        }
        s.history.kept.len() == HISTORY_CAP
            && s.history.pushed == (HISTORY_CAP + 6) as u64
            && s.history.dropped == 6
    };
    v.push((
        "J1-历史账容量64溢出丢最旧",
        hist_ok,
        format!("kept={} dropped=6 pushed={}", HISTORY_CAP, HISTORY_CAP + 6),
    ));
    // 7. 回读全等（写入面）。
    let readback_ok = {
        let line3 = match CORPUS.get(3) {
            Some(x) => *x,
            None => "",
        };
        let mut s = TextScreen::new();
        let _ = s.puts(9, 9, line3);
        let want = TextScreen::expect_bitmap(&[(9, 9, line3)]);
        s.readback_eq(&want).is_ok()
    };
    v.push(("J1-回读全等", readback_ok, String::from("位图逐格全等（写入面）")));
    // 8. 滚动面回读全等（首行出屏、次行上移、末行补白）。
    let scroll_readback_ok = {
        let mut s = TextScreen::new();
        let line0 = match CORPUS.get(0) {
            Some(x) => *x,
            None => "",
        };
        let line1 = match CORPUS.get(1) {
            Some(x) => *x,
            None => "",
        };
        let _ = s.puts(0, 0, line0);
        let _ = s.puts(1, 2, line1);
        let _ = s.scroll();
        // 滚动后：行 0 = 旧行 1（line1 从列 2 起），行 1 = 空白，末行 = 空白。
        let line1_head = match line1.as_bytes().first() {
            Some(b) => *b,
            None => 0,
        };
        let shifted = s.cell(0, 2) == Some(line1_head);
        let old_gone = s.cell(1, 2) == Some(SPACE_GLYPH);
        let last_blank = {
            let mut blank = true;
            let mut c = 0;
            while c < COLS {
                if s.cell(ROWS - 1, c) != Some(SPACE_GLYPH) {
                    blank = false;
                }
                c += 1;
            }
            blank
        };
        shifted && old_gone && last_blank
    };
    v.push((
        "J1-滚动面回读全等",
        scroll_readback_ok,
        String::from("首行出屏 + 末行空格补白"),
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
        format!("a对拍={} b越字形拒={} c越宽拒={}", a, b, c),
    ));
    // 11. 诊断码闭集对账。
    let codes_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < TxCode::ALL.len() {
            if let Some(c) = TxCode::ALL.get(i) {
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
        format!("{} 码全查表通过", TxCode::ALL.len()),
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
        String::from("0x75B00 窗口 0x40 对齐区间三闸"),
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

pub fn run_unx0486_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0486 文本输出原语（字符/字符串/滚动）");
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
            Ok((pushed, dropped, _, _)) => {
                assert_eq!(pushed, 70);
                assert_eq!(dropped, 6);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn frozen_dims() {
        assert!(frozen_dims_ok());
        assert_eq!(ROWS, 25);
        assert_eq!(COLS, 80);
        assert_eq!(HISTORY_CAP, 64);
    }

    #[test]
    fn row_col_glyph_over_caught() {
        let mut s = TextScreen::new();
        match s.putc(ROWS, 0, b'A') {
            Err(e) => assert_eq!(e, TxCode::RowOver.val()),
            Ok(_) => assert!(false, "越行未拒"),
        }
        match s.putc(0, COLS, b'A') {
            Err(e) => assert_eq!(e, TxCode::ColOver.val()),
            Ok(_) => assert!(false, "越列未拒"),
        }
        match s.putc(0, 0, 200) {
            Err(e) => assert_eq!(e, TxCode::GlyphOver.val()),
            Ok(_) => assert!(false, "越字形未拒"),
        }
    }

    #[test]
    fn write_overrun_caught() {
        let mut s = TextScreen::new();
        match s.puts(0, 78, "ABC") {
            Err(e) => assert_eq!(e, TxCode::WriteOverrun.val()),
            Ok(_) => assert!(false, "越宽未拒"),
        }
        // 边界内恰好写满：79 列起写 1 字节合法。
        assert_eq!(s.puts(0, 79, "Z"), Ok(1));
    }

    #[test]
    fn puts_precheck_atomic() {
        // 半串不落账：字形预检失败则整串不写。
        let mut s = TextScreen::new();
        let bad = "OK\u{80}BAD";
        match s.puts(2, 0, bad) {
            Err(e) => assert_eq!(e, TxCode::GlyphOver.val()),
            Ok(_) => assert!(false, "非 ASCII 未拒"),
        }
        let want = TextScreen::expect_bitmap(&[]);
        assert!(s.readback_eq(&want).is_ok(), "预检失败后位图应无残留");
    }

    #[test]
    fn scroll_conservation_and_row_sum() {
        assert!(scroll_conservation_ok());
        let mut s = TextScreen::new();
        let mut r = 0;
        while r < 100 {
            assert!(s.scroll().is_ok());
            assert!(s.history.conservation_ok());
            assert!(s.row_sum_ok());
            r += 1;
        }
        assert_eq!(s.history.pushed, 100);
        assert_eq!(s.history.dropped, 36);
        assert_eq!(s.history.kept.len(), HISTORY_CAP);
    }

    #[test]
    fn readback_equality_both_faces() {
        // 写入面。
        let line1 = match CORPUS.get(1) {
            Some(x) => *x,
            None => "",
        };
        let mut s = TextScreen::new();
        let _ = s.puts(4, 4, line1);
        let want = TextScreen::expect_bitmap(&[(4, 4, line1)]);
        assert!(s.readback_eq(&want).is_ok());
        // 篡改一格必失配。
        let mut want2 = want.clone();
        if let Some(slot) = want2.get_mut(0) {
            *slot = b'Q';
        }
        match s.readback_eq(&want2) {
            Err(e) => assert_eq!(e, TxCode::ReadbackMismatch.val()),
            Ok(_) => assert!(false, "篡改格未检出"),
        }
    }

    #[test]
    fn history_overflow_drops_oldest() {
        let mut h = HistoryLedger::new();
        let mut i = 0u64;
        while i < (HISTORY_CAP + 3) as u64 {
            let row = vec![b'A' + (i % 26) as u8; COLS];
            h.push(&row);
            i += 1;
        }
        assert!(h.conservation_ok());
        assert_eq!(h.kept.len(), HISTORY_CAP);
        assert_eq!(h.dropped, 3);
        // 最旧三行（'A'..='C'）已丢，队首应为第 4 行（'D'）。
        match h.kept.first() {
            Some(r) => match r.first() {
                Some(g) => assert_eq!(*g, b'D'),
                None => assert!(false, "历史行空"),
            },
            None => assert!(false, "历史账空"),
        }
    }

    #[test]
    fn probe_sequences() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_err());
        assert!(probe_seq_c().is_err());
    }

    #[test]
    fn reproducible_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(TxCode::ConservationBreach.val()), TxCode::ConservationBreach.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }
}
