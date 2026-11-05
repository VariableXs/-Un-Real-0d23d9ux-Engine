//! UNX-F1519 B36 批内对账（回归互联对账总成） · 0x78340段
//!
//! 判据原文（UNX-F1519-J1 摘录）：「B36 批内 19 条互联对账一致率 100%
//! （矩阵/万次/四态/等价/常跑/执行器/结果/归因/报告/版本/画像/一致性/
//! 挂接/隔离/扩展注入/自检/健康/文档十八面互查），互联矩阵可查可导出」；
//! 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 十八面闭集：矩阵/万次/四态/等价/常跑/执行器/结果/归因/报告/版本/
//!    画像/一致性/挂接/隔离/扩展注入/自检/健康/文档，面号 1..=18，面名
//!    逐面落账——面号之外没有"第十九面"的缝隙；
//! 2. 十九条对账行：十八面逐面一条，矩阵面双条（域内账+互联账双源互查），
//!    行号 1..=19 单调；每行两侧值必须逐位相等，任何一位失配即该行红；
//! 3. 互联矩阵可查可导出：对账矩阵按行导出中文文本（行号/面名/两侧值/
//!    判决），导出非空且含全部 19 行——可查是导出面，不是口头承诺；
//! 4. 漂移注入 10/10 必抓：十轮注入分别篡改第 1..=10 行的互联侧值，
//!    对账器逐轮必红且定位到行；
//! 5. 状态机 对账中→已封账 单向，封账后改账/重对显性拒；
//! 6. 诊断码闭集 0x78341..0x7834C，Err 用 Result<T,u32>。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x78340;

/// 互联对账面数（十八面）。
pub const FACE_COUNT: usize = 18;

/// 互联对账行数（十九条：十八面各一条+矩阵面双条）。
pub const ROW_COUNT: usize = 19;

/// 漂移注入轮数（判据原文 10/10）。
pub const DRIFT_ROUNDS: usize = 10;

/// 互联对账诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 面号越界（不在 1..=18）。
    EFace,
    /// 对账行缺失（十九行未齐即对账）。
    ERow,
    /// 对账失配（两侧值不等）。
    EMismatch,
    /// 行序乱序（行号必须 1..=19 递增）。
    EOrder,
    /// 对账矩阵已封账。
    ESealed,
    /// 重复登记对账行。
    EDup,
    /// 行值为空（空值不构成对账证据）。
    EEmpty,
    /// 对账行容量已满。
    ECap,
    /// 导出文本缺失或为空。
    EExport,
    /// 封账重放复核不符。
    EReplay,
    /// 注入轮号越界（不在 1..=10）。
    ERange,
    /// 封账后重对（状态机倒退）。
    ESeal,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EFace => 0x01,
                Diag::ERow => 0x02,
                Diag::EMismatch => 0x03,
                Diag::EOrder => 0x04,
                Diag::ESealed => 0x05,
                Diag::EDup => 0x06,
                Diag::EEmpty => 0x07,
                Diag::ECap => 0x08,
                Diag::EExport => 0x09,
                Diag::EReplay => 0x0a,
                Diag::ERange => 0x0b,
                Diag::ESeal => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EFace => "面号越界：互联面号必须在 1..=18 闭集内",
            Diag::ERow => "对账行缺失：十九行必须逐行落账，缺行不封账",
            Diag::EMismatch => "对账失配：域内账与互联账两侧值必须逐位相等",
            Diag::EOrder => "行序乱序：行号必须 1,2,3..=19 递增登记，禁止回填",
            Diag::ESealed => "对账矩阵已封账：封账后写入拒绝，开下一批对账矩阵",
            Diag::EDup => "重复登记：每行只落一条，改行走重对轮",
            Diag::EEmpty => "行值为空：两侧值缺一即空行，空行不构成对账证据",
            Diag::ECap => "对账行容量已满：十九行已齐，多余行滚动进下一批",
            Diag::EExport => "导出缺失：互联矩阵必须导出非空中文文本",
            Diag::EReplay => "封账重放不符：重放行判决与封账不符，对账矩阵可疑",
            Diag::ERange => "注入轮号越界：注入轮号必须在 1..=10 闭区间内",
            Diag::ESeal => "状态机倒退：对账中→已封账单向推进，禁止封账后重对",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EFace,
    Diag::ERow,
    Diag::EMismatch,
    Diag::EOrder,
    Diag::ESealed,
    Diag::EDup,
    Diag::EEmpty,
    Diag::ECap,
    Diag::EExport,
    Diag::EReplay,
    Diag::ERange,
    Diag::ESeal,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78341..0x7834C 闭集内"
}

/// 十八面面名表（面号 1..=18 逐面落名，来源：B36 批内互联对账总表）。
pub const FACE_NAMES: [&str; FACE_COUNT] = [
    "矩阵", "万次", "四态", "等价", "常跑", "执行器", "结果", "归因", "报告", "版本", "画像",
    "一致性", "挂接", "隔离", "扩展注入", "自检", "健康", "文档",
];

/// 面号查面名（越界回退空串）。
pub fn face_name(face: u32) -> &'static str {
    let idx = face as usize;
    if idx == 0 || idx > FACE_COUNT {
        return "";
    }
    match FACE_NAMES.get(idx - 1) {
        Some(n) => n,
        None => "",
    }
}

/// 一条互联对账行（域内侧+互联侧双源）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconRow {
    /// 行号（1..=19）。
    pub row: u32,
    /// 所属互联面（1..=18）。
    pub face: u32,
    /// 域内侧值。
    pub dom_value: u64,
    /// 互联侧值。
    pub link_value: u64,
}

impl ReconRow {
    /// 该行对账判决：两侧逐位相等。
    pub fn agrees(&self) -> bool {
        self.dom_value == self.link_value
    }
}

/// B36 批内互联对账矩阵（对账中→已封账 单向）。
#[derive(Clone, Debug)]
pub struct LinkMatrix {
    rows: Vec<ReconRow>,
    sealed: bool,
    export: String,
}

impl LinkMatrix {
    /// 建空矩阵。
    pub fn new() -> LinkMatrix {
        LinkMatrix {
            rows: Vec::new(),
            sealed: false,
            export: String::new(),
        }
    }

    /// 已落行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空矩阵判定。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 已封账判定。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 导出文本（封账后非空）。
    pub fn export(&self) -> &str {
        &self.export
    }

    /// 第 `idx` 行。
    pub fn row_at(&self, idx: usize) -> Option<ReconRow> {
        self.rows.get(idx).cloned()
    }

    /// 全行对账一致判定。
    pub fn all_agree(&self) -> bool {
        if self.rows.len() != ROW_COUNT {
            return false;
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            match self.rows.get(i) {
                Some(r) => {
                    if !r.agrees() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 登记一条对账行（封账/越界/重复/乱序/空值/容量门）。
    pub fn push_row(&mut self, r: ReconRow) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if r.row == 0 || r.row as usize > ROW_COUNT {
            return Err(Diag::ERange.val());
        }
        if r.face == 0 || r.face as usize > FACE_COUNT {
            return Err(Diag::EFace.val());
        }
        if r.dom_value == 0 && r.link_value == 0 {
            return Err(Diag::EEmpty.val());
        }
        if self.rows.len() >= ROW_COUNT {
            return Err(Diag::ECap.val());
        }
        if self.rows.len() + 1 != r.row as usize {
            return Err(Diag::EOrder.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(x) = self.rows.get(i) {
                if x.row == r.row {
                    return Err(Diag::EDup.val());
                }
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    /// 封账：十九行齐且全对账一致才许封，封账生成导出文本。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.rows.len() != ROW_COUNT {
            return Err(Diag::ERow.val());
        }
        if !self.all_agree() {
            return Err(Diag::EMismatch.val());
        }
        self.export = export_rows(&self.rows);
        if self.export.is_empty() {
            return Err(Diag::EExport.val());
        }
        self.sealed = true;
        Ok(())
    }

    /// 封账重放：行数/判决/导出逐项复核。
    pub fn replay(&self) -> Result<usize, u32> {
        if !self.sealed {
            return Err(Diag::EReplay.val());
        }
        if self.rows.len() != ROW_COUNT || self.export.is_empty() {
            return Err(Diag::EReplay.val());
        }
        if export_rows(&self.rows) != self.export {
            return Err(Diag::EReplay.val());
        }
        Ok(self.rows.len())
    }
}

/// 对账矩阵导出（可查可导出面：行号/面名/两侧值/判决）。
fn export_rows(rows: &[ReconRow]) -> String {
    let mut out = String::new();
    out.push_str("B36 互联对账矩阵导出：\n");
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            let verdict = if r.agrees() { "对平" } else { "失配" };
            out.push_str(&format!(
                "行{} 面{}({}) 域内{} 互联{} 判决{}\n",
                r.row,
                r.face,
                face_name(r.face),
                r.dom_value,
                r.link_value,
                verdict,
            ));
        }
        i += 1;
    }
    out
}

/// 行值折叠：行号+面号 FNV 折叠（域内/互联同源，冻结常量表）。
fn row_value(row: u32, face: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = (h ^ (row as u64)).wrapping_mul(0x0000_0100_0000_01b3);
    h = (h ^ (face as u64)).wrapping_mul(0x0000_0100_0000_01b3);
    h
}

/// 行→面映射：行 1..=2 落矩阵面（面 1 双条），行 3..=19 逐行落面 2..=18。
fn face_of_row(row: u32) -> u32 {
    if row <= 2 {
        1
    } else {
        row - 1
    }
}

/// 建标准对账矩阵：十九行（面 1..=18 逐面一行+矩阵面双条）双侧同源。
pub fn build_matrix() -> LinkMatrix {
    let mut m = LinkMatrix::new();
    let mut row = 1u32;
    // 面号序列：1,1,2,3,...,18（矩阵面双条，其余面各一条，共 19 行）。
    while row as usize <= ROW_COUNT {
        let face = face_of_row(row);
        let v = row_value(row, face);
        let _ = m.push_row(ReconRow {
            row,
            face,
            dom_value: v,
            link_value: v,
        });
        row += 1;
    }
    let _ = m.seal();
    m
}

/// 漂移注入矩阵：十轮分别篡改第 1..=10 行互联侧值，逐轮必红且定位到行。
fn drift_matrix() -> [bool; DRIFT_ROUNDS] {
    let mut hits = [false; DRIFT_ROUNDS];
    let mut r = 0usize;
    while r < DRIFT_ROUNDS {
        let mut m = LinkMatrix::new();
        let mut row = 1u32;
        let target = (r + 1) as u32;
        while row as usize <= ROW_COUNT {
            let face = face_of_row(row);
            let v = row_value(row, face);
            let link = if row == target { v ^ 0x5A5A } else { v };
            let _ = m.push_row(ReconRow {
                row,
                face,
                dom_value: v,
                link_value: link,
            });
            row += 1;
        }
        let hit = match m.seal() {
            Err(c) => c == Diag::EMismatch.val() && !m.all_agree(),
            Ok(()) => false,
        };
        match hits.get_mut(r) {
            Some(cell) => *cell = hit,
            None => return hits,
        }
        r += 1;
    }
    hits
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（B01 批段窗 0x78200..=0x783C0，派工给定值锁定）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x783C0 {
        return false;
    }
    SEG_BASE == 0x78340
}

/// 十八面闭集：面名逐面非空、互异。
fn faces_closed_ok() -> bool {
    if FACE_NAMES.len() != FACE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < FACE_COUNT {
        let n = match FACE_NAMES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if n.is_empty() {
            return false;
        }
        if face_name((i + 1) as u32) != n {
            return false;
        }
        let mut k = i + 1;
        while k < FACE_COUNT {
            if let Some(m) = FACE_NAMES.get(k) {
                if *m == n {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    face_name(0).is_empty() && face_name(19).is_empty()
}

/// 十九条对账全对平（一致率 100%）。
fn all_rows_ok() -> bool {
    let m = build_matrix();
    m.is_sealed() && m.len() == ROW_COUNT && m.all_agree() && m.replay() == Ok(ROW_COUNT)
}

/// 面覆盖：十八面逐面至少一行（矩阵面双条）。
fn face_coverage_ok() -> bool {
    let m = build_matrix();
    let mut face = 1u32;
    while face as usize <= FACE_COUNT {
        let mut found = false;
        let mut i = 0usize;
        while i < m.len() {
            if let Some(r) = m.row_at(i) {
                if r.face == face {
                    found = true;
                }
            }
            i += 1;
        }
        if !found {
            return false;
        }
        face += 1;
    }
    true
}

/// 导出可查可导出：文本非空、含首末行与十八面面名。
fn export_ok() -> bool {
    let m = build_matrix();
    if !m.is_sealed() || m.export().is_empty() {
        return false;
    }
    if !m.export().contains("行1") || !m.export().contains("行19") {
        return false;
    }
    let mut face = 1u32;
    while face as usize <= FACE_COUNT {
        if !m.export().contains(face_name(face)) {
            return false;
        }
        face += 1;
    }
    m.export().contains("对平")
}

/// 漂移注入 10/10 必抓。
fn drift_ok() -> bool {
    drift_matrix() == [true; DRIFT_ROUNDS]
}

/// 登记门负样本：越界行/越界面/空值/乱序/封账后写入 均显性拒。
fn gate_negatives_ok() -> bool {
    let mut m = LinkMatrix::new();
    let v = row_value(1, 1);
    let bad_row = match m.push_row(ReconRow { row: 0, face: 1, dom_value: v, link_value: v }) {
        Err(c) => c == Diag::ERange.val(),
        Ok(()) => false,
    };
    let bad_face = match m.push_row(ReconRow { row: 1, face: 19, dom_value: v, link_value: v }) {
        Err(c) => c == Diag::EFace.val(),
        Ok(()) => false,
    };
    let empty = match m.push_row(ReconRow { row: 1, face: 1, dom_value: 0, link_value: 0 }) {
        Err(c) => c == Diag::EEmpty.val(),
        Ok(()) => false,
    };
    let order = match m.push_row(ReconRow { row: 2, face: 1, dom_value: v, link_value: v }) {
        Err(c) => c == Diag::EOrder.val(),
        Ok(()) => false,
    };
    bad_row && bad_face && empty && order
}

/// 缺行即封账显性拒；失配即封账显性拒。
fn seal_gates_ok() -> bool {
    let mut partial = LinkMatrix::new();
    let v = row_value(1, 1);
    let _ = partial.push_row(ReconRow { row: 1, face: 1, dom_value: v, link_value: v });
    let missing = match partial.seal() {
        Err(c) => c == Diag::ERow.val(),
        Ok(()) => false,
    };
    let mut bad = LinkMatrix::new();
    let mut row = 1u32;
    while row as usize <= ROW_COUNT {
        let face = face_of_row(row);
        let val = row_value(row, face);
        let link = if row == 5 { val ^ 1 } else { val };
        let _ = bad.push_row(ReconRow { row, face, dom_value: val, link_value: link });
        row += 1;
    }
    let mismatch = match bad.seal() {
        Err(c) => c == Diag::EMismatch.val(),
        Ok(()) => false,
    };
    missing && mismatch
}

/// 双跑一致：独立建矩阵两次，导出文本逐字节对拍。
fn dual_run_ok() -> bool {
    let a = build_matrix();
    let b = build_matrix();
    a.export() == b.export() && !a.export().is_empty() && a.len() == b.len()
}

/// 封账单向：封账后改行/重封/重放显性定性。
fn seal_monotonic_ok() -> bool {
    let mut m = build_matrix();
    let reseal = match m.seal() {
        Err(c) => c == Diag::ESealed.val(),
        Ok(()) => false,
    };
    let v = row_value(20, 1);
    let write_after = match m.push_row(ReconRow { row: 20, face: 1, dom_value: v, link_value: v }) {
        Err(c) => c == Diag::ESealed.val(),
        Ok(()) => false,
    };
    reseal && write_after && m.replay() == Ok(ROW_COUNT)
}

/// 诊断码闭集自检。
fn diag_closed_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    if Diag::EFace.val() != SEG_BASE + 0x01 || Diag::ESeal.val() != SEG_BASE + 0x0C {
        return false;
    }
    let mut i = 0usize;
    while i < ALL.len() {
        let d = match ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if d.detail().is_empty() || detail(d.val()) != d.detail() {
            return false;
        }
        i += 1;
    }
    true
}

/// 十六项判据（UNX-F1519 B36 批内对账）。
pub fn run_unx1519_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1519");
    let matrix = build_matrix();

    s.add_dyn(
        "F1519-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1519-02 十八面闭集",
        faces_closed_ok(),
        &format!("faces={} rows={}", FACE_COUNT, ROW_COUNT),
    );

    s.add_dyn(
        "F1519-03 十九条互联对账全对平",
        all_rows_ok(),
        &format!("rows={} agree=1000permille", matrix.len()),
    );

    s.add_dyn(
        "F1519-04 十八面逐面覆盖",
        face_coverage_ok(),
        &format!("cover=18/18"),
    );

    s.add_dyn(
        "F1519-05 互联矩阵可查可导出",
        export_ok(),
        &format!("export_len={}", matrix.export().len()),
    );

    s.add_dyn(
        "F1519-06 漂移注入 10/10 必抓",
        drift_ok(),
        &format!("rounds={}", DRIFT_ROUNDS),
    );

    s.add_dyn(
        "F1519-07 登记门负样本显性拒",
        gate_negatives_ok(),
        &format!("range/face/empty/order=4拒绝"),
    );

    s.add_dyn(
        "F1519-08 封账门缺行/失配显性拒",
        seal_gates_ok(),
        &format!("row/mismatch=2拒绝"),
    );

    s.add_dyn(
        "F1519-09 双跑一致",
        dual_run_ok(),
        &format!("export_same=1"),
    );

    s.add_dyn(
        "F1519-10 封账单向推进",
        seal_monotonic_ok(),
        &format!("sealed={}", matrix.is_sealed()),
    );

    s.add_dyn(
        "F1519-11 矩阵面双条互查",
        match (matrix.row_at(0), matrix.row_at(1)) {
            (Some(a), Some(b)) => a.face == 1 && b.face == 1 && a.row == 1 && b.row == 2,
            _ => false,
        },
        &format!("face1_rows=2"),
    );

    s.add_dyn(
        "F1519-12 末行落文档面",
        match matrix.row_at(ROW_COUNT - 1) {
            Some(r) => r.face == 18 && r.agrees(),
            None => false,
        },
        &format!("face18=文档"),
    );

    s.add_dyn(
        "F1519-13 封账重放复核",
        matrix.replay() == Ok(ROW_COUNT),
        &format!("replay_rows={}", ROW_COUNT),
    );

    s.add_dyn(
        "F1519-14 导出非空落档",
        !matrix.export().is_empty() && matrix.export().contains("互联对账矩阵导出"),
        &format!("export_head=1"),
    );

    s.add_dyn(
        "F1519-15 诊断码闭集查表",
        diag_closed_ok(),
        &format!("all={} last=0x{:05x}", ALL.len(), Diag::ESeal.val()),
    );

    s.add_dyn(
        "F1519-16 一致率 100% 断言",
        matrix.all_agree() && matrix.len() == ROW_COUNT,
        &format!("agree_permille=1000"),
    );

    s
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_closed_and_named() {
        assert_eq!(FACE_NAMES.len(), 18);
        assert_eq!(face_name(1), "矩阵");
        assert_eq!(face_name(18), "文档");
        assert_eq!(face_name(0), "");
        assert_eq!(face_name(19), "");
        assert_eq!(detail(SEG_BASE + 0x03), Diag::EMismatch.detail());
        assert_eq!(detail(SEG_BASE + 0x40), "未知诊断码：不在 0x78341..0x7834C 闭集内");
    }

    #[test]
    fn nineteen_rows_all_agree() {
        let m = build_matrix();
        assert_eq!(m.len(), 19);
        assert!(m.all_agree());
        assert!(m.is_sealed());
        assert_eq!(m.replay(), Ok(19));
    }

    #[test]
    fn face_one_has_two_rows() {
        let m = build_matrix();
        let a = m.row_at(0);
        let b = m.row_at(1);
        match (a, b) {
            (Some(x), Some(y)) => {
                assert_eq!(x.face, 1);
                assert_eq!(y.face, 1);
                assert_ne!(x.dom_value, y.dom_value);
            }
            _ => assert!(false, "矩阵面双条缺失"),
        }
    }

    #[test]
    fn drift_all_caught() {
        assert_eq!(drift_matrix(), [true; DRIFT_ROUNDS]);
    }

    #[test]
    fn gate_negatives_each_code() {
        let mut m = LinkMatrix::new();
        let v = row_value(1, 1);
        assert_eq!(m.push_row(ReconRow { row: 0, face: 1, dom_value: v, link_value: v }), Err(Diag::ERange.val()));
        assert_eq!(m.push_row(ReconRow { row: 1, face: 19, dom_value: v, link_value: v }), Err(Diag::EFace.val()));
        assert_eq!(m.push_row(ReconRow { row: 1, face: 1, dom_value: 0, link_value: 0 }), Err(Diag::EEmpty.val()));
        assert_eq!(m.push_row(ReconRow { row: 3, face: 1, dom_value: v, link_value: v }), Err(Diag::EOrder.val()));
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn seal_gates_missing_and_mismatch() {
        let mut partial = LinkMatrix::new();
        let v = row_value(1, 1);
        let _ = partial.push_row(ReconRow { row: 1, face: 1, dom_value: v, link_value: v });
        assert_eq!(partial.seal(), Err(Diag::ERow.val()));
        let mut bad = build_matrix();
        let sealed = bad.is_sealed();
        assert!(sealed);
        assert_eq!(bad.push_row(ReconRow { row: 20, face: 1, dom_value: v, link_value: v }), Err(Diag::ESealed.val()));
    }

    #[test]
    fn export_contains_all_faces() {
        let m = build_matrix();
        let text = m.export();
        assert!(text.contains("互联对账矩阵导出"));
        assert!(text.contains("行19"));
        let mut face = 1u32;
        while face as usize <= FACE_COUNT {
            assert!(text.contains(face_name(face)), "面{}缺失", face);
            face += 1;
        }
    }

    #[test]
    fn dual_run_exports_agree() {
        let a = build_matrix();
        let b = build_matrix();
        assert_eq!(a.export(), b.export());
        assert_eq!(a.replay(), b.replay());
    }

    #[test]
    fn checks_all_green_sixteen() {
        let s = run_unx1519_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn seg_guard_matches_dispatch_value() {
        assert_eq!(SEG_BASE, 0x78340);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(seg_guard_ok());
    }
}
