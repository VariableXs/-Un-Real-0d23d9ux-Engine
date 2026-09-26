
// ---------------------------------------------------------------------------
// F012 · 深化批次六：控制台缓冲区尺寸调整（列/行变化语义）
//
// 主册依据（G-A-12【数据与存储】）：「控制台缓冲区（回看 10 万行）在终端
// 进程内存」——终端尺寸调整语义：回看内容保留、光标位置钳制在新界内
// （Windows 控制台同语义）。
// ---------------------------------------------------------------------------

/// 缓冲区尺寸状态（列/行 + 光标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleSize {
    pub cols: u32,
    pub rows: u32,
    pub cursor_col: u32,
    pub cursor_row: u32,
}

impl ConsoleSize {
    /// 尺寸调整：回看保留（本面只管几何——内容面归既有缓冲）、光标钳制。
    pub fn resize(&mut self, new_cols: u32, new_rows: u32) {
        if new_cols == 0 || new_rows == 0 {
            return; // 零尺寸如实拒（不产生不可见的控制台）
        }
        self.cols = new_cols;
        self.rows = new_rows;
        self.cursor_col = self.cursor_col.min(new_cols.saturating_sub(1));
        self.cursor_row = self.cursor_row.min(new_rows.saturating_sub(1));
    }
}

/// F012 深化批次六自检。
pub fn run_condrv_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F012-condrv-deep5");
    // 1) 缩小钳制：80×25 光标 (70,20) → 60×20 → 光标钳 (59,19)。
    let mut sz = ConsoleSize { cols: 80, rows: 25, cursor_col: 70, cursor_row: 20 };
    sz.resize(60, 20);
    cs.add(
        "console_resize_cursor_clamped",
        sz.cols == 60 && sz.rows == 20 && sz.cursor_col == 59 && sz.cursor_row == 19,
        "",
    );
    // 2) 放大不动光标（钳制不上移——用户视点保留）。
    let mut sz2 = ConsoleSize { cols: 60, rows: 20, cursor_col: 10, cursor_row: 5 };
    sz2.resize(100, 30);
    cs.add(
        "console_resize_grow_keeps_cursor",
        sz2.cursor_col == 10 && sz2.cursor_row == 5,
        "",
    );
    // 3) 零尺寸如实拒（几何不变——不产生不可见控制台）。
    let mut sz3 = ConsoleSize { cols: 80, rows: 25, cursor_col: 1, cursor_row: 1 };
    sz3.resize(0, 25);
    cs.add("console_resize_zero_rejected", sz3.cols == 80 && sz3.cursor_col == 1, "");
    cs
}
