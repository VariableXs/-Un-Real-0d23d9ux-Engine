//! VE-F4027 · RTL 与表格
//!
//! 表格 RTL：表格列序镜像/列对齐/排序图标方向——表格镜像规则（列序
//! 镜像+对齐镜像——表格断言：列序与表头一致——表头一致红线：列序镜
//! 像表头未镜像=数据错读——一致断言）；单元格内混合方向（复用
//! F4023——单源）；排序交互（排序点击方向语义——复用控件表）。
//! 数据结构：一致断言；单元格复用；排序复述；规则复述。
//! 错误路径：表头错位→P0（数据错读级）；单元格错→复用；排序语义反
//! →P1；漏镜像→覆盖。
//! 性能：一致 O(断言)；单元格 O(复用)；排序 O(复述)；镜像 O(规则)。
//! 跨批对接：F4024 控件单源复用声明；N 域表格对端；F4023 混合复用。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x6B40）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x6B40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T33TCode(pub u16);

impl T33TCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const HEADER_MISS: Self = Self(SEG_BASE + 1);
    pub const SORT_FLIP: Self = Self(SEG_BASE + 2);
    pub const MIRROR_MISS: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::HEADER_MISS => "表头错位→P0（数据错读）",
            Self::SORT_FLIP => "排序语义反→P1",
            Self::MIRROR_MISS => "漏镜像→覆盖",
            _ => "未知",
        }
    }
}

// ── 表格模型 ────────────────────────────────────────────────────────

/// 列：表头 + 数据 + 对齐。
#[derive(Debug, Clone)]
pub struct Column {
    pub header: String,
    /// 对齐（true = 左对齐，false = 右对齐）。
    pub align_left: bool,
}

/// 表格（LTR 声明态）。
#[derive(Debug, Clone)]
pub struct Table {
    pub cols: Vec<Column>,
    /// RTL 态列序已镜像。
    pub cols_mirrored: bool,
    /// RTL 态表头已镜像。
    pub header_mirrored: bool,
    /// RTL 态对齐已镜像。
    pub align_mirrored: bool,
}

impl Table {
    pub fn new(headers: &[&str], aligns: &[bool]) -> Self {
        Self {
            cols: headers.iter().zip(aligns.iter())
                .map(|(h, a)| Column { header: String::from(*h), align_left: *a })
                .collect(),
            cols_mirrored: false,
            header_mirrored: false,
            align_mirrored: false,
        }
    }

    /// 列数。
    pub fn n(&self) -> usize {
        self.cols.len()
    }
}

/// 一致断言：三镜像同开或同关（列序镜像了表头必须镜像——数据错读
/// 红线：列序镜像表头未镜像 = P0）。
pub fn header_consistency(t: &Table) -> T33TCode {
    if t.cols_mirrored && !t.header_mirrored {
        return T33TCode::HEADER_MISS;
    }
    if t.align_mirrored && !t.cols_mirrored {
        return T33TCode::HEADER_MISS;
    }
    T33TCode::OK
}

// ── 排序交互（方向语义）─────────────────────────────────────────────

/// 排序方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// 排序图标语义：升序在 RTL 中图标方向镜像但语义（A→Z）不变。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortSemantics {
    /// 语义升序保持。
    pub asc_logical: bool,
    /// 图标已镜像。
    pub icon_mirrored: bool,
}

/// 排序断言：语义与图标解耦——镜像图标不反转语义。
pub fn sort_assert(sem: SortSemantics) -> T33TCode {
    if !sem.asc_logical {
        T33TCode::SORT_FLIP
    } else {
        T33TCode::OK
    }
}

// ── 单元格混合方向（复用 F4023 口径）───────────────────────────────

/// 单元格内容类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellMix {
    /// 纯本地语言。
    PureLocal,
    /// 含数字/URL（需 isolate）。
    HasEmbed,
    /// isolate 已包裹。
    Isolated,
}

/// 单元格断言（复用 F4023 isolate 红线：HasEmbed 未隔离 = P0 级错误）。
pub fn cell_assert(mix: CellMix) -> T33TCode {
    match mix {
        CellMix::PureLocal | CellMix::Isolated => T33TCode::OK,
        CellMix::HasEmbed => T33TCode::HEADER_MISS, // 复用 P0 级通道
    }
}

// ── 覆盖扫描 ────────────────────────────────────────────────────────

/// 表格覆盖扫描器。
#[derive(Debug, Clone)]
pub struct TableScanner {
    pub tables: Vec<(String, Table)>,
}

impl TableScanner {
    pub fn new() -> Self {
        Self { tables: Vec::new() }
    }

    pub fn put(&mut self, name: &str, t: Table) {
        self.tables.push((String::from(name), t));
    }

    /// RTL 态漏镜像检出：RTL 表格三项全须镜像。
    pub fn missing(&self) -> Vec<String> {
        self.tables.iter()
            .filter(|(_, t)| !(t.cols_mirrored && t.header_mirrored && t.align_mirrored))
            .map(|(n, _)| n.clone())
            .collect()
    }

    pub fn coverage_ok(&self) -> bool {
        self.missing().is_empty()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves267_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves267_s12table");

    // C4027-01 诊断码段不越窗。
    chk(&mut set, "C4027-01",
        T33TCode::MIRROR_MISS.0 < SEG_BASE + 16 && SEG_BASE == 0x6B40, "err");

    // C4027-02 表格构建：列数与表头一致。
    let t = Table::new(&["名称", "数量", "日期"], &[true, false, false]);
    chk(&mut set, "C4027-02", t.n() == 3 && t.cols[0].align_left, "err");

    // C4027-03 表头一致断言：LTR 全不镜像 → 过。
    chk(&mut set, "C4027-03", header_consistency(&t) == T33TCode::OK, "err");

    // C4027-04 表头错位 → P0（列序镜像表头未镜像——数据错读红线）。
    let mut t2 = Table::new(&["A", "B"], &[true, true]);
    t2.cols_mirrored = true;
    chk(&mut set, "C4027-04", header_consistency(&t2) == T33TCode::HEADER_MISS, "err");

    // C4027-05 补表头镜像 → 过。
    t2.header_mirrored = true;
    chk(&mut set, "C4027-05", header_consistency(&t2) == T33TCode::OK, "err");

    // C4027-06 对齐镜像领先列序镜像 → 拦。
    let mut t3 = Table::new(&["A"], &[true]);
    t3.align_mirrored = true;
    chk(&mut set, "C4027-06", header_consistency(&t3) == T33TCode::HEADER_MISS, "err");

    // C4027-07 全镜像 → 过。
    t3.cols_mirrored = true;
    t3.header_mirrored = true;
    chk(&mut set, "C4027-07", header_consistency(&t3) == T33TCode::OK, "err");

    // C4027-08 排序断言：语义升序 + 图标镜像 → 过。
    let s1 = SortSemantics { asc_logical: true, icon_mirrored: true };
    chk(&mut set, "C4027-08", sort_assert(s1) == T33TCode::OK, "err");

    // C4027-09 排序语义反 → P1。
    let s2 = SortSemantics { asc_logical: false, icon_mirrored: false };
    chk(&mut set, "C4027-09", sort_assert(s2) == T33TCode::SORT_FLIP, "err");

    // C4027-10 排序枚举互异。
    chk(&mut set, "C4027-10", SortDir::Asc != SortDir::Desc, "err");

    // C4027-11 单元格断言：纯本地过。
    chk(&mut set, "C4027-11", cell_assert(CellMix::PureLocal) == T33TCode::OK, "err");

    // C4027-12 单元格断言：数字未隔离 → P0 通道。
    chk(&mut set, "C4027-12", cell_assert(CellMix::HasEmbed) != T33TCode::OK, "err");

    // C4027-13 单元格断言：isolate 后过。
    chk(&mut set, "C4027-13", cell_assert(CellMix::Isolated) == T33TCode::OK, "err");

    // C4027-14 覆盖扫描：全镜像过。
    let mut sc = TableScanner::new();
    let mut full = Table::new(&["A", "B"], &[true, false]);
    full.cols_mirrored = true;
    full.header_mirrored = true;
    full.align_mirrored = true;
    sc.put("订单表", full);
    chk(&mut set, "C4027-14", sc.coverage_ok(), "err");

    // C4027-15 漏镜像检出。
    let partial = Table::new(&["A"], &[true]);
    sc.put("库存表", partial);
    chk(&mut set, "C4027-15",
        sc.missing() == vec![String::from("库存表")] && !sc.coverage_ok(), "err");

    // C4027-16 补齐后过。
    if let Some((_, t)) = sc.tables.iter_mut().find(|(n, _)| n == "库存表") {
        t.cols_mirrored = true;
        t.header_mirrored = true;
        t.align_mirrored = true;
    }
    chk(&mut set, "C4027-16", sc.coverage_ok(), "err");

    // C4027-17 空扫描过（零常态）。
    chk(&mut set, "C4027-17", TableScanner::new().coverage_ok(), "err");

    // C4027-18 表头错位码语义（数据错读级 P0）。
    chk(&mut set, "C4027-18",
        T33TCode::HEADER_MISS.explain().contains("数据错读"), "err");

    // C4027-19 排序码语义。
    chk(&mut set, "C4027-19", T33TCode::SORT_FLIP.explain().contains("P1"), "err");

    // C4027-20 错误码互异。
    let codes = [T33TCode::OK.0, T33TCode::HEADER_MISS.0, T33TCode::SORT_FLIP.0, T33TCode::MIRROR_MISS.0];
    chk(&mut set, "C4027-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_mirror_consistency() {
        let mut t = Table::new(&["A", "B"], &[true, true]);
        t.cols_mirrored = true;
        assert_eq!(header_consistency(&t), T33TCode::HEADER_MISS);
        t.header_mirrored = true;
        assert_eq!(header_consistency(&t), T33TCode::OK);
    }

    #[test]
    fn sort_semantics_decoupled() {
        assert_eq!(sort_assert(SortSemantics { asc_logical: true, icon_mirrored: true }), T33TCode::OK);
        assert_eq!(sort_assert(SortSemantics { asc_logical: false, icon_mirrored: true }), T33TCode::SORT_FLIP);
    }
}
