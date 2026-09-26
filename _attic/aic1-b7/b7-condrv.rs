
// ---------------------------------------------------------------------------
// F012 · 深化批次六续：DEC 自动换行语义（pending 换行标志——VT 光标推进核）
//
// 主册依据（G-A-12【设计细节】）：「VT 序列支持……」的 DECAWM（自动换行）
// 语义：光标到最右列时**不立即换行**，置 pending 标志；下一个可打印字符才
// 换到下行首列（Windows 控制台同语义——防止「恰好写满一行」误换行）。
// ---------------------------------------------------------------------------

/// 自动换行光标（DECAWM 开启语义）。
#[derive(Clone, Copy, Debug)]
pub struct AutowrapCursor {
    pub col: u32,
    pub row: u32,
    pub pending: bool,
    cols: u32,
    rows: u32,
}

impl AutowrapCursor {
    pub fn new(cols: u32, rows: u32) -> AutowrapCursor {
        AutowrapCursor { col: 0, row: 0, pending: false, cols, rows }
    }

    /// 打印一个字符（宽 1——宽字符折算由调用面承担）。
    pub fn print_char(&mut self) {
        if self.pending {
            self.pending = false;
            self.col = 0;
            if self.row + 1 < self.rows {
                self.row += 1;
            }
        }
        if self.col + 1 >= self.cols {
            self.col = self.cols - 1;
            self.pending = true;
        } else {
            self.col += 1;
        }
    }

    /// 换行（LF）：清 pending（显式换行不叠加自动换行），行 +1（底行钳制——
    /// 滚动归缓冲面）。
    pub fn linefeed(&mut self) {
        self.pending = false;
        if self.row + 1 < self.rows {
            self.row += 1;
        }
    }

    /// 回车（CR）：列归零。
    pub fn carriage_return(&mut self) {
        self.col = 0;
    }
}

/// F012 深化批次七自检。
pub fn run_condrv_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F012-condrv-deep6");
    // 1) 写满一行 → pending 置位（不立即换行）；下一字符换行首列。
    let mut c = AutowrapCursor::new(4, 3);
    for _ in 0..4 {
        c.print_char();
    }
    let at_end = c.col == 3 && c.pending && c.row == 0;
    c.print_char();
    cs.add(
        "autowrap_pending_then_wrap",
        at_end && c.col == 0 && c.row == 1 && !c.pending,
        "",
    );
    // 2) 行尾后 LF：清 pending + 行推进（不双跳）。
    let mut c2 = AutowrapCursor::new(4, 3);
    for _ in 0..4 {
        c2.print_char();
    }
    c2.linefeed();
    cs.add(
        "linefeed_clears_pending",
        c2.row == 1 && c2.col == 3 && !c2.pending,
        "",
    );
    // 3) 底行钳制：滚到最后一行后 LF 不再下移（滚动归缓冲面——光标停在底行）。
    let mut c3 = AutowrapCursor::new(4, 2);
    c3.linefeed();
    c3.linefeed();
    c3.linefeed();
    cs.add(
        "autowrap_bottom_clamped",
        c3.row == 1 && c3.cols == 4,
        "",
    );
    cs
}
