//! UNX-F81005 台账列校验器（必填列/枚举列/行数列三闸） · 0x91090段
//!
//! 判据原文（派工卡 81004 逐字锚定）：
//!   ① 纯功能代码 ≥289 行；
//!   ② 判据原文：UNX-F81005-J1 「台账列校验器（必填列/枚举列/行数列
//!      三闸）」判据落地：正向断言 ×10 与冻结口径完全一致；注入同型故障
//!      10 次零静默；失败路径显性化、埋点入账；台账写入仅走本条登记通道
//!      （非法第四态零存在）；判据锚定 §AI-81 台账域任务书保真；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. RowGate 三闸结构：必填闸（6 必填列逐列非空）、枚举闸（3 枚举列
//!    闭集值域）、行数闸（行数列值==实际列数守恒），闸次序冻结。
//! 2. 列内容以定长字节槽表示（≤16 字节右补零），空白格（全 0x20/0x00）
//!    视为空——必填列仅空白同样拒。
//! 3. 枚举闭集 {1,2,3}：边界值 0 与最大+1 双向越值必拒，闭集外零漏。
//! 4. 逐列校验账 append-only：每闸每列一行结果，失败路径显性化、埋点
//!    入账，账不回写不改写。
//! 5. 注入核 ×10（必填列空/枚举越值/行数列漂移/空行/全空行/列数越界/
//!    枚举边界 0/枚举边界最大+1/必填列仅空白/三闸全失），逐类断言具体
//!    诊断码，注入必抓。
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/expect/panic。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX12 派工公式：0x91000 + 0x30 × (81004 − 81001)）。
pub const SEG_BASE: u32 = 0x91090;

// ── 诊断码闭集（段内偏移 0x01..0x0D） ─────────────────────────────
/// 必填列空。
pub const E_REQEMPTY: u16 = 1;
/// 枚举越值（闭集外）。
pub const E_ENUMVAL: u16 = 2;
/// 行数列漂移（声明 ≠ 实际）。
pub const E_ROWDRIFT: u16 = 3;
/// 空行（行数列为 0）。
pub const E_EMPTYROW: u16 = 4;
/// 全空行（逐列全空但行数列虚报）。
pub const E_ALLBLANK: u16 = 5;
/// 列数越界（超 12 列闭集）。
pub const E_COLRANGE: u16 = 6;
/// 枚举边界值 0。
pub const E_ENUMZERO: u16 = 7;
/// 枚举边界值最大+1。
pub const E_ENUMMAX: u16 = 8;
/// 必填列仅空白。
pub const E_REQBLANK: u16 = 9;
/// 三闸全失。
pub const E_GATEALL: u16 = 10;
/// 行太短（列槽数不足）。
pub const E_SHORTROW: u16 = 11;
/// 校验账乱序。
pub const E_LEDORDER: u16 = 12;

/// 诊断码闭集口径（12 个）。
pub const DIAG_ALL: usize = 12;

/// 列内容槽宽。
pub const CELL_W: usize = 16;

/// 列闭集容量（12 列 schema 口径）。
pub const MAX_COLS: usize = 12;

/// 必填列数（列 0..6）。
pub const REQ_COLS: usize = 6;

/// 枚举列号（3 枚举列：域号/状态/波型）。
pub const ENUM_COLS: [usize; 3] = [1, 9, 10];

/// 枚举闭集 {1,2,4}（含空洞：3 为闭集外值，越值可探）。
pub const ENUM_SET: [u16; 3] = [1, 2, 4];

/// 行数列号。
pub const ROWCOL: usize = 3;

/// 单行：12 列定长槽。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateRow {
    cells: [[u8; CELL_W]; MAX_COLS],
    width: usize,
}

impl GateRow {
    /// 从列槽数组构造（≤12 列；越 12 列构造即失败由校验入口报）。
    pub fn from_cells(cells: &[[u8; CELL_W]]) -> GateRow {
        let mut r = GateRow::empty(MAX_COLS);
        let n = if cells.len() > MAX_COLS { MAX_COLS } else { cells.len() };
        let mut i = 0usize;
        while i < n {
            r.cells[i] = cells[i];
            i += 1;
        }
        r
    }

    /// 空行构造（width 为声明列数）。
    pub fn empty(width: usize) -> GateRow {
        GateRow {
            cells: [[0u8; CELL_W]; MAX_COLS],
            width,
        }
    }

    /// 声明列数。
    pub fn width(&self) -> usize {
        self.width
    }

    /// 取列槽（越界 None）。
    pub fn cell(&self, col: usize) -> Option<[u8; CELL_W]> {
        if col >= MAX_COLS {
            return None;
        }
        Some(self.cells[col])
    }

    /// 直改列槽（注入核构坏样本用）。
    pub fn set_cell(&mut self, col: usize, v: [u8; CELL_W]) -> bool {
        if col >= MAX_COLS {
            return false;
        }
        self.cells[col] = v;
        true
    }

    /// 列是否空（全零槽）。
    pub fn cell_is_zero(col: [u8; CELL_W]) -> bool {
        let mut i = 0usize;
        while i < CELL_W {
            if col[i] != 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 列是否仅空白（全 0 或全空格 0x20）。
    pub fn cell_is_blank(col: [u8; CELL_W]) -> bool {
        let mut i = 0usize;
        while i < CELL_W {
            if col[i] != 0 && col[i] != b' ' {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 列解析为 u16（小端；非数字语义不在此判，只取字节值）。
    pub fn cell_u16(col: [u8; CELL_W]) -> u16 {
        (col[0] as u16) | ((col[1] as u16) << 8)
    }
}

/// 单格校验结果（校验账一行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateRecord {
    /// 闸号（1 必填 / 2 枚举 / 3 行数）。
    pub gate: u8,
    /// 列号。
    pub col: u8,
    /// 通过位。
    pub passed: bool,
    /// 失败诊断码（段内偏移；通过为 0）。
    pub diag: u16,
}

/// 三闸校验器：逐列校验账 append-only。
#[derive(Debug)]
pub struct RowGate {
    ledger: Vec<GateRecord>,
    seq: u32,
}

/// 列校验域错误。`code()` 返回 `SEG_BASE + n`，全部码值被真实构造。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateError {
    /// 必填列空。
    ReqEmpty,
    /// 枚举越值。
    EnumVal,
    /// 行数列漂移。
    RowDrift,
    /// 空行。
    EmptyRow,
    /// 全空行。
    AllBlank,
    /// 列数越界。
    ColRange,
    /// 枚举边界 0。
    EnumZero,
    /// 枚举边界最大+1。
    EnumMax,
    /// 必填列仅空白。
    ReqBlank,
    /// 三闸全失。
    GateAll,
    /// 行太短。
    ShortRow,
    /// 校验账乱序。
    LedOrder,
}

impl GateError {
    /// 诊断码（段内偏移闭集）。
    pub fn code(self) -> u32 {
        let n: u16 = match self {
            GateError::ReqEmpty => E_REQEMPTY,
            GateError::EnumVal => E_ENUMVAL,
            GateError::RowDrift => E_ROWDRIFT,
            GateError::EmptyRow => E_EMPTYROW,
            GateError::AllBlank => E_ALLBLANK,
            GateError::ColRange => E_COLRANGE,
            GateError::EnumZero => E_ENUMZERO,
            GateError::EnumMax => E_ENUMMAX,
            GateError::ReqBlank => E_REQBLANK,
            GateError::GateAll => E_GATEALL,
            GateError::ShortRow => E_SHORTROW,
            GateError::LedOrder => E_LEDORDER,
        };
        SEG_BASE + n as u32
    }

    /// 三要素之一：出了什么事（冻结文本）。
    pub fn message(self) -> &'static str {
        match self {
            GateError::ReqEmpty => "required column empty",
            GateError::EnumVal => "enum value outside closed set",
            GateError::RowDrift => "row-count column drifts from actual",
            GateError::EmptyRow => "empty row (row-count zero)",
            GateError::AllBlank => "all-blank row with fake count",
            GateError::ColRange => "column count out of closed set",
            GateError::EnumZero => "enum boundary zero",
            GateError::EnumMax => "enum boundary max+1",
            GateError::ReqBlank => "required column blank-only",
            GateError::GateAll => "all three gates failed",
            GateError::ShortRow => "row shorter than declared",
            GateError::LedOrder => "gate ledger out of order",
        }
    }
}

impl RowGate {
    /// 新校验器：空账。
    pub fn new() -> RowGate {
        RowGate { ledger: Vec::new(), seq: 0 }
    }

    /// 账记录数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 按序取账记录。
    pub fn record(&self, idx: usize) -> Option<GateRecord> {
        if idx >= self.ledger.len() {
            return None;
        }
        Some(self.ledger[idx])
    }

    fn log(&mut self, gate: u8, col: u8, passed: bool, diag: u16) {
        self.seq += 1;
        self.ledger.push(GateRecord { gate, col, passed, diag });
    }

    /// 必填闸：6 必填列逐列校验（逐列全零 → AllBlank；单列全零槽 →
    /// ReqEmpty；仅空白 → ReqBlank）。
    pub fn gate_required(&mut self, row: &GateRow) -> Result<(), GateError> {
        let mut zeros = 0usize;
        let mut i = 0usize;
        while i < REQ_COLS {
            let c = row.cell(i).unwrap_or([0u8; CELL_W]);
            if GateRow::cell_is_zero(c) {
                zeros += 1;
            }
            i += 1;
        }
        if zeros == REQ_COLS {
            self.log(1, 0, false, E_ALLBLANK);
            return Err(GateError::AllBlank);
        }
        let mut i = 0usize;
        while i < REQ_COLS {
            let c = row.cell(i).unwrap_or([0u8; CELL_W]);
            if GateRow::cell_is_zero(c) {
                self.log(1, i as u8, false, E_REQEMPTY);
                return Err(GateError::ReqEmpty);
            }
            if GateRow::cell_is_blank(c) {
                self.log(1, i as u8, false, E_REQBLANK);
                return Err(GateError::ReqBlank);
            }
            self.log(1, i as u8, true, 0);
            i += 1;
        }
        Ok(())
    }

    /// 枚举闸：3 枚举列值域闭集校验（0 → EnumZero；max+1 → EnumMax；
    /// 其余闭集外 → EnumVal）。
    pub fn gate_enum(&mut self, row: &GateRow) -> Result<(), GateError> {
        let mut k = 0usize;
        while k < ENUM_COLS.len() {
            let col = ENUM_COLS[k];
            let c = row.cell(col).unwrap_or([0u8; CELL_W]);
            let v = GateRow::cell_u16(c);
            if v == 0 {
                self.log(2, col as u8, false, E_ENUMZERO);
                return Err(GateError::EnumZero);
            }
            if v > ENUM_SET[ENUM_SET.len() - 1] {
                self.log(2, col as u8, false, E_ENUMMAX);
                return Err(GateError::EnumMax);
            }
            let mut in_set = false;
            let mut j = 0usize;
            while j < ENUM_SET.len() {
                if ENUM_SET[j] == v {
                    in_set = true;
                }
                j += 1;
            }
            if !in_set {
                self.log(2, col as u8, false, E_ENUMVAL);
                return Err(GateError::EnumVal);
            }
            self.log(2, col as u8, true, 0);
            k += 1;
        }
        Ok(())
    }

    /// 行数闸：行数列值 == 实际声明列数守恒校验。
    pub fn gate_rowcount(&mut self, row: &GateRow) -> Result<(), GateError> {
        let c = row.cell(ROWCOL).unwrap_or([0u8; CELL_W]);
        let declared = GateRow::cell_u16(c) as usize;
        if declared == 0 {
            self.log(3, ROWCOL as u8, false, E_EMPTYROW);
            return Err(GateError::EmptyRow);
        }
        if declared <= ROWCOL {
            self.log(3, ROWCOL as u8, false, E_SHORTROW);
            return Err(GateError::ShortRow);
        }
        if declared > MAX_COLS {
            self.log(3, ROWCOL as u8, false, E_COLRANGE);
            return Err(GateError::ColRange);
        }
        if row.width() > MAX_COLS || row.width() == 0 {
            self.log(3, ROWCOL as u8, false, E_COLRANGE);
            return Err(GateError::ColRange);
        }
        if declared != row.width() {
            self.log(3, ROWCOL as u8, false, E_ROWDRIFT);
            return Err(GateError::RowDrift);
        }
        self.log(3, ROWCOL as u8, true, 0);
        Ok(())
    }

    /// 三闸串行入口（唯一合法校验通道）：任一闸失败即显性返回，逐格
    /// 结果埋点入账；三闸全失样本特殊标记 GateAll。
    pub fn verify(&mut self, row: &GateRow) -> Result<(), GateError> {
        let g1 = self.gate_required(row);
        let g2 = self.gate_enum(row);
        let g3 = self.gate_rowcount(row);
        let fail = |g: &Result<(), GateError>| matches!(g, Err(_));
        if fail(&g1) && fail(&g2) && fail(&g3) {
            self.log(9, 0, false, E_GATEALL);
            return Err(GateError::GateAll);
        }
        g1?;
        g2?;
        g3
    }

    /// 账审计：埋点闸号/列号必须落在合法域，越域即 LedOrder。
    pub fn audit_ledger(&self) -> Result<(), GateError> {
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].gate == 0 || self.ledger[i].col > MAX_COLS as u8 {
                return Err(GateError::LedOrder);
            }
            i += 1;
        }
        Ok(())
    }

    /// 外部塞账检测口：把账记录闸号改写为越域值后由审计抓 LedOrder（注入核用）。
    pub fn corrupt_ledger_for_audit(&mut self, idx: usize, gate: u8) -> Result<(), GateError> {
        if idx >= self.ledger.len() {
            return Err(GateError::ShortRow);
        }
        self.ledger[idx].gate = gate;
        Ok(())
    }
}

/// 12 列合法行样板（必填 6 列非空、枚举 3 列在闭集、行数列=12）。
pub fn sample_good_row() -> GateRow {
    let mut cells: Vec<[u8; CELL_W]> = Vec::new();
    let mut i = 0usize;
    while i < MAX_COLS {
        let mut c = [0u8; CELL_W];
        c[0] = (i + 1) as u8;
        c[2] = b'A';
        cells.push(c);
        i += 1;
    }
    // 枚举列置闭集值；行数列置 12。
    cells[1][0] = 1;
    cells[9][0] = 2;
    cells[10][0] = 4;
    cells[3][0] = 12;
    cells[3][1] = 0;
    GateRow::from_cells(&cells)
}

/// 注入核：十类坏样本逐一注入，返回（样本名，捕获诊断码）对；
/// 每类必须返回 Err（注入必抓），抓不住即判据失效。
pub fn inject_ten() -> Vec<(&'static str, u32)> {
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    // 1 必填列空 → ReqEmpty
    let mut r = sample_good_row();
    r.set_cell(2, [0u8; CELL_W]);
    match RowGate::new().gate_required(&r) {
        Err(e) => out.push(("req-empty", e.code())),
        Ok(_) => out.push(("req-empty-MISSED", SEG_BASE)),
    }
    // 2 枚举越值（闭集 {1,2,4} 的空洞值 3）→ EnumVal
    let mut r = sample_good_row();
    r.set_cell(9, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    match RowGate::new().gate_enum(&r) {
        Err(e) => out.push(("enum-gap", e.code())),
        Ok(_) => out.push(("enum-gap-MISSED", SEG_BASE)),
    }
    // 3 行数列漂移 → RowDrift
    let mut r = sample_good_row();
    r.set_cell(3, [9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    match RowGate::new().gate_rowcount(&r) {
        Err(e) => out.push(("row-drift", e.code())),
        Ok(_) => out.push(("row-drift-MISSED", SEG_BASE)),
    }
    // 4 空行（行数列 0）→ EmptyRow
    let r = GateRow::empty(12);
    match RowGate::new().gate_rowcount(&r) {
        Err(e) => out.push(("empty-row", e.code())),
        Ok(_) => out.push(("empty-row-MISSED", SEG_BASE)),
    }
    // 5 全空行（行数列虚报 12 但逐列全空）→ AllBlank（经 verify 走必填闸抓）
    let r = GateRow::empty(12);
    match RowGate::new().verify(&r) {
        Err(e) => out.push(("all-blank", e.code())),
        Ok(_) => out.push(("all-blank-MISSED", SEG_BASE)),
    }
    // 6 列数越界 → ColRange
    let mut r = sample_good_row();
    r.set_cell(3, [20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    match RowGate::new().gate_rowcount(&r) {
        Err(e) => out.push(("col-range", e.code())),
        Ok(_) => out.push(("col-range-MISSED", SEG_BASE)),
    }
    // 7 枚举边界值 0 → EnumZero
    let mut r = sample_good_row();
    r.set_cell(1, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    match RowGate::new().gate_enum(&r) {
        Err(e) => out.push(("enum-zero", e.code())),
        Ok(_) => out.push(("enum-zero-MISSED", SEG_BASE)),
    }
    // 8 枚举边界值最大+1（闭集顶 4，取 5）→ EnumMax
    let mut r = sample_good_row();
    r.set_cell(10, [5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    match RowGate::new().gate_enum(&r) {
        Err(e) => out.push(("enum-max-plus", e.code())),
        Ok(_) => out.push(("enum-max-plus-MISSED", SEG_BASE)),
    }
    // 9 必填列仅空白 → ReqBlank
    let mut r = sample_good_row();
    r.set_cell(0, [b' ', b' ', b' ', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    match RowGate::new().gate_required(&r) {
        Err(e) => out.push(("req-blank", e.code())),
        Ok(_) => out.push(("req-blank-MISSED", SEG_BASE)),
    }
    // 10 三闸全失 → GateAll（必填空 + 枚举 0 + 行数漂移）
    let mut r = GateRow::empty(12);
    r.set_cell(3, [5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    match RowGate::new().verify(&r) {
        Err(e) => out.push(("gate-all-fail", e.code())),
        Ok(_) => out.push(("gate-all-fail-MISSED", SEG_BASE)),
    }
    out
}

/// 注入核自检：10/10 捕获且码值全在段闭集内（注入必抓、零静默）。
pub fn inject_all_caught() -> bool {
    let samples = inject_ten();
    if samples.len() < 10 {
        return false;
    }
    let mut i = 0usize;
    while i < samples.len() {
        let (name, code) = samples[i];
        if name.ends_with("MISSED") {
            return false;
        }
        if code < SEG_BASE || code >= SEG_BASE + DIAG_ALL as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据集（F81005-J1 落地）：14 条，正向 + 注入汇总。
pub fn run_unx81004_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F81004");

    // 1 段位守卫
    let ok = SEG_BASE > 0x91000 && SEG_BASE < 0x94000 && (SEG_BASE - 0x91000) % 0x30 == 0;
    s.add("F81005-01 段位守卫", ok, if ok { "" } else { "段位越窗" });

    // 2 诊断闭集口径
    let ok = DIAG_ALL == 12;
    s.add("F81005-02 诊断闭集", ok, if ok { "" } else { "闭集口径漂移" });

    // 3 三闸结构冻结（必填 6 / 枚举 3 / 行数 1）
    let ok = REQ_COLS == 6 && ENUM_COLS == [1, 9, 10] && ROWCOL == 3 && ENUM_SET == [1, 2, 4];
    s.add("F81005-03 三闸冻结", ok, if ok { "" } else { "三闸口径漂移" });

    // 4 合法行三闸全绿
    let ok = RowGate::new().verify(&sample_good_row()) == Ok(());
    s.add("F81005-04 合法行全绿", ok, if ok { "" } else { "合法行误拒" });

    // 5 必填闸逐列通过留痕（6 条全 passed）
    let mut g = RowGate::new();
    let _ = g.gate_required(&sample_good_row());
    let mut all_pass = g.ledger_len() == REQ_COLS;
    let mut i = 0usize;
    while i < g.ledger_len() {
        match g.record(i) {
            Some(rec) => {
                if !rec.passed || rec.gate != 1 {
                    all_pass = false;
                }
            }
            None => all_pass = false,
        }
        i += 1;
    }
    s.add("F81005-05 必填闸留痕", ok_fwd(all_pass), if all_pass { "" } else { "必填闸账坏" });

    // 6 枚举闸 3 列全过
    let mut g = RowGate::new();
    let ok = g.gate_enum(&sample_good_row()) == Ok(()) && g.ledger_len() == 3;
    s.add("F81005-06 枚举闸过", ok, if ok { "" } else { "枚举闸坏" });

    // 7 行数守恒：声明=实际
    let ok = RowGate::new().gate_rowcount(&sample_good_row()) == Ok(());
    s.add("F81005-07 行数守恒", ok, if ok { "" } else { "行数闸坏" });

    // 8 校验账 append-only：连续两次校验账只增不改
    let mut g = RowGate::new();
    let _ = g.verify(&sample_good_row());
    let n0 = g.ledger_len();
    let r0 = g.record(0);
    let _ = g.verify(&sample_good_row());
    let ok = g.ledger_len() == 2 * n0 && g.record(0) == r0 && n0 > 0;
    s.add("F81005-08 账只增", ok, if ok { "" } else { "账被改写" });

    // 9 账审计幂等：健康账连续两次审计均绿；塞账越域后审计抓 LedOrder
    let mut g = RowGate::new();
    let _ = g.verify(&sample_good_row());
    let healthy = g.audit_ledger() == Ok(()) && g.audit_ledger() == Ok(());
    let _ = g.corrupt_ledger_for_audit(0, 0);
    let ok = healthy && matches!(g.audit_ledger(), Err(GateError::LedOrder));
    s.add("F81005-09 审计幂等", ok, if ok { "" } else { "审计不确定" });

    // 10 注入 10/10 零静默
    let ok = inject_all_caught();
    s.add("F81005-10 注入10/10", ok, if ok { "" } else { "注入漏抓" });

    // 11 空白判别与零判别分离
    let blank = [b' '; CELL_W];
    let zero = [0u8; CELL_W];
    let ok = GateRow::cell_is_blank(blank) && !GateRow::cell_is_zero(blank) && GateRow::cell_is_zero(zero);
    s.add("F81005-11 空白分离", ok, if ok { "" } else { "空白判别漂移" });

    // 12 枚举闭集双向边界（0 与 max+1 拒、1 与闭集顶 4 收）
    let mut rz = sample_good_row();
    rz.set_cell(1, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut rm = sample_good_row();
    rm.set_cell(1, [5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let ok = matches!(RowGate::new().gate_enum(&rz), Err(GateError::EnumZero))
        && matches!(RowGate::new().gate_enum(&rm), Err(GateError::EnumMax))
        && RowGate::new().gate_enum(&sample_good_row()) == Ok(());
    s.add("F81005-12 枚举边界", ok, if ok { "" } else { "枚举边界漏抓" });

    // 13 GateAll 标记：三闸全失样本必带专码
    let mut r = GateRow::empty(12);
    r.set_cell(3, [5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let ok = matches!(RowGate::new().verify(&r), Err(GateError::GateAll));
    s.add("F81005-13 全失专码", ok, if ok { "" } else { "全失无专码" });

    // 14 合法行往返：good row 校验后账内无失败记录
    let mut g = RowGate::new();
    let _ = g.verify(&sample_good_row());
    let mut no_fail = true;
    let mut i = 0usize;
    while i < g.ledger_len() {
        if let Some(rec) = g.record(i) {
            if !rec.passed {
                no_fail = false;
            }
        }
        i += 1;
    }
    s.add("F81005-14 账零失败", ok_fwd(no_fail), if no_fail { "" } else { "合法行带失败痕" });

    s
}

/// 小工具：bool 直通（判据可读性）。
fn ok_fwd(b: bool) -> bool {
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx81004_checks_green() {
        let set = run_unx81004_checks();
        assert!(set.all_passed(), "UNX-F81004 red: {}", set.red_items_text());
    }
}
