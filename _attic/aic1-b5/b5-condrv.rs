
// ---------------------------------------------------------------------------
// F012 · 深化批次五：DECSC/DECRC 光标存取（VT 语义面补齐）
//
// 主册依据（G-A-12【设计细节】）：「VT 序列支持……」的光标状态组——DECSC
// （ESC 7 存光标）/ DECRC（ESC 8 取光标）是进度条与行内重绘的根基语义。
// ---------------------------------------------------------------------------

/// 光标存档（DECSC 保存的状态面：位置 + 可见性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorState {
    pub row: u32,
    pub col: u32,
    pub visible: bool,
}

/// 光标存取会话（单槽——Windows 控制台同语义：一次存取对）。
#[derive(Clone, Copy, Debug)]
pub struct CursorSaveRestore {
    saved: Option<CursorState>,
    /// 未取先存覆盖次数（诊断面——程序反复 DECSC 是正常模式，计数不告警）。
    pub saves: u32,
    /// 无存档取用次数（DECRC 先于 DECSC → 光标留在原位，如实计数）。
    pub restore_without_save: u32,
}

impl CursorSaveRestore {
    pub const fn new() -> CursorSaveRestore {
        CursorSaveRestore { saved: None, saves: 0, restore_without_save: 0 }
    }

    /// DECSC（ESC 7）。
    pub fn save(&mut self, st: CursorState) {
        self.saved = Some(st);
        self.saves += 1;
    }

    /// DECRC（ESC 8）：有存档 → 恢复；无存档 → 光标留原位（如实计数不崩）。
    pub fn restore(&mut self, current: CursorState) -> CursorState {
        match self.saved.take() {
            Some(st) => st,
            None => {
                self.restore_without_save += 1;
                current
            }
        }
    }
}

/// F012 深化批次五自检。
pub fn run_condrv_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F012-condrv-deep4");
    // 1) 标准对：存 (3,10) → 移到 (9,1) → 取 → 回 (3,10)。
    let mut csr = CursorSaveRestore::new();
    csr.save(CursorState { row: 3, col: 10, visible: true });
    let moved = csr.restore(CursorState { row: 9, col: 1, visible: true });
    cs.add(
        "decsc_decrc_roundtrip",
        moved == CursorState { row: 3, col: 10, visible: true } && csr.saves == 1,
        "",
    );
    // 2) 无存档取用：光标留原位 + 计数可见（不崩不猜）。
    let mut csr2 = CursorSaveRestore::new();
    let cur = CursorState { row: 0, col: 0, visible: true };
    let kept = csr2.restore(cur);
    cs.add(
        "decrc_without_save_honest",
        kept == cur && csr2.restore_without_save == 1,
        "",
    );
    // 3) 可见性随存档恢复（隐藏态也是状态面的一部分）。
    let mut csr3 = CursorSaveRestore::new();
    csr3.save(CursorState { row: 5, col: 5, visible: false });
    let got = csr3.restore(CursorState { row: 1, col: 1, visible: true });
    cs.add(
        "decsc_visibility_state",
        got.visible == false && got.row == 5,
        "",
    );
    cs
}
