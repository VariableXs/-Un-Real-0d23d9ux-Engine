//! F025 深化批次三 · DEVMODE 印刷参数与页范围边界（compatstar2/deep2 · G-A-25）。
//!
//! 主层 printpdf.rs 覆盖打印队列/PDF 流/单段页范围；本批补齐主册
//! 【功能定义】「全语义对齐」的执行/边界面：ISO/DIN 纸张尺寸表
//! （A3/A4/A5/B4/B5/Letter 六种，DEVMODE 0.1mm 单位真实值与 DMPAPER_*
//! 真实 ID）、取向/份数/双面协商模型（DMORIENT_*、份数 1-999 钳制、
//! DMDUP_* 长边/短边翻页语义）、分辨率档位匹配（300/600/1200dpi 与
//! 纸张面积预算模型——超预算降档并记账）、打印页范围解析器（"3-5,8,11-"
//! 三段式模型：起-闭区间/单页/开区间，定长 16 段，重叠合并、乱序拒绝）。
//!
//! 判据对账：主册 G-A-25【设计细节】打印参数段 + MS DEVMODE 文档语义
//! 对拍（dmPaperWidth/dmPaperLength 以 0.1mm 为单位、dmDup 取值）。
//!
//! 零堆纪律：定长纸张表/段表，无 Vec/String/Box/format!，拒绝一律
//! Err(&'static str) 或计数账面。

use crate::checks::CheckSet;

/// 纸张表容量。
pub const PAPER_SLOTS: usize = 6;
/// 份数下限/上限（MS DEVMODE dmCopies 语义钳制）。
pub const COPIES_MIN: u32 = 1;
pub const COPIES_MAX: u32 = 999;
/// 单页光栅预算（域内模型口径，128 MiB）。
pub const RASTER_BUDGET_BYTES: u64 = 128 << 20;
/// 页范围段定长（域内模型口径）。
pub const PAGE_SEG_SLOTS: usize = 16;
/// 开区间哨兵（"11-" 语义）。
pub const PAGE_OPEN_END: u32 = u32::MAX;

/// DEVMODE 取向（MS 真实值）。
pub const DMORIENT_PORTRAIT: u32 = 1;
pub const DMORIENT_LANDSCAPE: u32 = 2;
/// DEVMODE 双面（MS 真实值：SIMPLEX 单面；VERTICAL 沿竖直轴翻 = 长边
/// 翻页；HORIZONTAL 沿水平轴翻 = 短边翻页）。
pub const DMDUP_SIMPLEX: u32 = 1;
pub const DMDUP_VERTICAL: u32 = 2;
pub const DMDUP_HORIZONTAL: u32 = 3;

/// ISO/DIN 纸张（0.1mm 真实值 + MS DMPAPER_* 真实 ID）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PaperSize {
    pub name: &'static str,
    pub dm_id: u16,
    pub w_mm10: u16,
    pub h_mm10: u16,
}

pub const PAPER_TABLE: [PaperSize; PAPER_SLOTS] = [
    PaperSize { name: "A3", dm_id: 8, w_mm10: 2970, h_mm10: 4200 },
    PaperSize { name: "A4", dm_id: 9, w_mm10: 2100, h_mm10: 2970 },
    PaperSize { name: "A5", dm_id: 11, w_mm10: 1480, h_mm10: 2100 },
    PaperSize { name: "B4", dm_id: 12, w_mm10: 2570, h_mm10: 3640 },
    PaperSize { name: "B5", dm_id: 13, w_mm10: 1820, h_mm10: 2570 },
    PaperSize { name: "Letter", dm_id: 1, w_mm10: 2159, h_mm10: 2794 },
];

/// 按 DMPAPER_* ID 查表；未注册 ID 显性 None。
pub fn paper_by_id(dm_id: u16) -> Option<PaperSize> {
    PAPER_TABLE.iter().copied().find(|p| p.dm_id == dm_id)
}

/// 取向/份数/双面协商结果（钳制事实显性回传，不静默）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Negotiated {
    pub orient: u32,
    pub copies: u32,
    pub dup: u32,
    pub copies_clamped: bool,
}

/// DEVMODE 协商：取向/双面必须取合法值，否则显性拒绝；份数钳制 1-999。
pub fn negotiate(orient: u32, copies: u32, dup: u32) -> Result<Negotiated, &'static str> {
    if orient != DMORIENT_PORTRAIT && orient != DMORIENT_LANDSCAPE {
        return Err("dmorient-invalid");
    }
    if dup != DMDUP_SIMPLEX && dup != DMDUP_VERTICAL && dup != DMDUP_HORIZONTAL {
        return Err("dmdup-invalid");
    }
    let clamped = copies < COPIES_MIN || copies > COPIES_MAX;
    Ok(Negotiated { orient, copies: copies.clamp(COPIES_MIN, COPIES_MAX), dup, copies_clamped: clamped })
}

/// 分辨率档位（dpi）。
pub const DPI_TIERS: [u32; 3] = [1200, 600, 300];

/// 一页 24bpp 光栅字节预算账（0.1mm → 英寸 ÷254）。
pub fn raster_bytes(w_mm10: u16, h_mm10: u16, dpi: u32) -> u64 {
    let pw = (w_mm10 as u64) * dpi as u64 / 254;
    let ph = (h_mm10 as u64) * dpi as u64 / 254;
    pw * ph * 3
}

/// 档位挑选结果：选中 dpi、降档次数、实际光栅字节、兜底超预算标志。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DpiPick {
    pub dpi: u32,
    pub downgrades: u32,
    pub raster_bytes: u64,
    pub over_budget: bool,
}

/// 面积预算匹配：自 want 对应档起降档至预算内；最低档仍超预算如实
/// 返回并打 over_budget 标（不静默吞）。
pub fn pick_dpi(w_mm10: u16, h_mm10: u16, want: u32) -> DpiPick {
    let mut tier = DPI_TIERS.iter().copied().find(|&d| d <= want).unwrap_or(300);
    let mut downgrades = 0u32;
    loop {
        let bytes = raster_bytes(w_mm10, h_mm10, tier);
        if bytes <= RASTER_BUDGET_BYTES {
            return DpiPick { dpi: tier, downgrades, raster_bytes: bytes, over_budget: false };
        }
        if tier == 300 {
            return DpiPick { dpi: 300, downgrades, raster_bytes: bytes, over_budget: true };
        }
        tier = match tier {
            1200 => 600,
            _ => 300,
        };
        downgrades += 1;
    }
}

/// 降档记账簿。
#[derive(Clone, Copy, Default)]
pub struct DpiGovernor {
    pub picks: u32,
    pub downgrades: u32,
}

impl DpiGovernor {
    pub fn pick(&mut self, w_mm10: u16, h_mm10: u16, want: u32) -> DpiPick {
        let p = pick_dpi(w_mm10, h_mm10, want);
        self.picks += 1;
        self.downgrades += p.downgrades;
        p
    }
}

/// 页范围段（from..to 闭区间；to == PAGE_OPEN_END 表示开区间）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageSeg {
    pub from: u32,
    pub to: u32,
}

fn parse_page_num(b: &[u8]) -> Result<u32, &'static str> {
    if b.is_empty() || b.len() > 7 {
        return Err("range-format");
    }
    let mut v: u32 = 0;
    for &c in b {
        if !c.is_ascii_digit() {
            return Err("range-format");
        }
        v = v * 10 + (c - b'0') as u32;
    }
    if v == 0 {
        return Err("page-zero");
    }
    Ok(v)
}

/// 三段式页范围解析器："3-5"（起-闭）、"8"（单页）、"11-"（开区间）。
/// 乱序/零页/空段/超容如实拒绝；相邻或重叠段（新 from ≥ 旧 from 且
/// from ≤ 旧 to+1）合并——乱序非相邻（如 5-7,3-4）拒绝。
pub fn parse_page_ranges(spec: &str) -> Result<([PageSeg; PAGE_SEG_SLOTS], usize), &'static str> {
    let mut segs = [PageSeg { from: 0, to: 0 }; PAGE_SEG_SLOTS];
    let mut n = 0usize;
    for part in spec.split(',') {
        let b = part.as_bytes();
        if b.is_empty() {
            return Err("range-format");
        }
        let (from, to) = if let Some(hy) = b.iter().position(|&c| c == b'-') {
            let (a, r) = (&b[..hy], &b[hy + 1..]);
            let a_v = parse_page_num(a)?;
            if r.is_empty() {
                (a_v, PAGE_OPEN_END)
            } else {
                let b_v = parse_page_num(r)?;
                if a_v > b_v {
                    return Err("range-disorder");
                }
                (a_v, b_v)
            }
        } else {
            let v = parse_page_num(b)?;
            (v, v)
        };
        if n > 0 {
            let prev = segs[n - 1];
            if prev.to == PAGE_OPEN_END {
                return Err("range-after-open");
            }
            if from < prev.from {
                return Err("range-disorder");
            }
            if from <= prev.to.saturating_add(1) {
                if to > prev.to {
                    segs[n - 1].to = to;
                }
                continue;
            }
        }
        if n >= PAGE_SEG_SLOTS {
            return Err("page-seg-full");
        }
        segs[n] = PageSeg { from, to };
        n += 1;
    }
    if n == 0 {
        return Err("range-format");
    }
    Ok((segs, n))
}

/// 域自检（深化批次三）。
pub fn run_f025e_checks() -> crate::checks::CheckSet {
    let mut cs = CheckSet::new("F025-printpdf-d3");
    // 1) 纸张表真实值（DEVMODE 0.1mm + DMPAPER_* ID）。
    let a4 = paper_by_id(9).expect("A4 必在表");
    let letter = paper_by_id(1).expect("Letter 必在表");
    cs.add(
        "paper_table_real",
        a4 == PaperSize { name: "A4", dm_id: 9, w_mm10: 2100, h_mm10: 2970 }
            && letter == PaperSize { name: "Letter", dm_id: 1, w_mm10: 2159, h_mm10: 2794 }
            && paper_by_id(13) == Some(PAPER_TABLE[4])
            && PAPER_TABLE[4].w_mm10 == 1820,
        "",
    );
    // 2) 未注册 DMPAPER ID 显性 None。
    cs.add("paper_unknown_id", paper_by_id(999).is_none(), "");
    // 3) 份数钳制：5000 → 999（记 clamped）；0 → 1。
    let hi = negotiate(DMORIENT_PORTRAIT, 5000, DMDUP_SIMPLEX).expect("取向合法必成");
    let lo = negotiate(DMORIENT_PORTRAIT, 0, DMDUP_SIMPLEX).expect("取向合法必成");
    cs.add(
        "copies_clamp",
        hi.copies == COPIES_MAX && hi.copies_clamped && lo.copies == COPIES_MIN && lo.copies_clamped,
        "",
    );
    // 4) 取向/双面非法值显性拒绝；合法三档双面放行。
    cs.add(
        "orient_dup_reject",
        negotiate(7, 1, DMDUP_SIMPLEX) == Err("dmorient-invalid")
            && negotiate(DMORIENT_PORTRAIT, 1, 4) == Err("dmdup-invalid")
            && negotiate(DMORIENT_LANDSCAPE, 1, DMDUP_VERTICAL).is_ok()
            && negotiate(DMORIENT_LANDSCAPE, 1, DMDUP_HORIZONTAL).is_ok(),
        "",
    );
    // 5) A4@1200 超 128MiB 预算 → 降档 600 且预算内（约 104MB）。
    let p1 = pick_dpi(2100, 2970, 1200);
    cs.add(
        "dpi_budget_a4",
        p1.dpi == 600 && p1.downgrades == 1 && !p1.over_budget && p1.raster_bytes <= RASTER_BUDGET_BYTES,
        "",
    );
    // 6) A3@1200 → 连降两档至 300（约 52MB）。
    let p2 = pick_dpi(2970, 4200, 1200);
    cs.add("dpi_budget_a3", p2.dpi == 300 && p2.downgrades == 2 && !p2.over_budget, "");
    // 7) A5@300 一次选中，零降档。
    let p3 = pick_dpi(1480, 2100, 300);
    cs.add("dpi_no_downgrade", p3.dpi == 300 && p3.downgrades == 0 && p3.raster_bytes <= RASTER_BUDGET_BYTES, "");
    // 8) 记账簿：两次降档挑选累计 downgrades = 3。
    let mut g = DpiGovernor::default();
    let _ = g.pick(2100, 2970, 1200);
    let _ = g.pick(2970, 4200, 1200);
    cs.add("dpi_governor_ledger", g.picks == 2 && g.downgrades == 3, "");
    // 9) 三段式解析："3-5,8,11-" → 闭区间/单页/开区间三段。
    let (segs, n) = parse_page_ranges("3-5,8,11-").expect("合法 spec 必成");
    cs.add(
        "range_three_forms",
        n == 3
            && segs[0] == PageSeg { from: 3, to: 5 }
            && segs[1] == PageSeg { from: 8, to: 8 }
            && segs[2] == PageSeg { from: 11, to: PAGE_OPEN_END },
        "",
    );
    // 10) 相邻与重叠合并："3-5,6-8" → 3-8；"1,1-3" → 1-3。
    let (m1, n1) = parse_page_ranges("3-5,6-8").expect("合法 spec 必成");
    let (m2, n2) = parse_page_ranges("1,1-3").expect("合法 spec 必成");
    cs.add(
        "range_merge",
        n1 == 1 && m1[0] == PageSeg { from: 3, to: 8 } && n2 == 1 && m2[0] == PageSeg { from: 1, to: 3 },
        "",
    );
    // 11) 乱序/开区间后继显性拒绝。
    cs.add(
        "range_disorder_reject",
        parse_page_ranges("5-7,3-4") == Err("range-disorder")
            && parse_page_ranges("11-,12") == Err("range-after-open"),
        "",
    );
    // 12) 畸形段显性拒绝：双连字符/非数字/零页。
    cs.add(
        "range_malformed_reject",
        parse_page_ranges("3--5") == Err("range-format")
            && parse_page_ranges("abc") == Err("range-format")
            && parse_page_ranges("0") == Err("page-zero"),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_table_monotone_area() {
        // A3 > A4 > A5 面积单调（0.1mm² 口径）。
        let area = |p: PaperSize| p.w_mm10 as u64 * p.h_mm10 as u64;
        assert!(area(PAPER_TABLE[0]) > area(PAPER_TABLE[1]));
        assert!(area(PAPER_TABLE[1]) > area(PAPER_TABLE[2]));
        assert_eq!(area(PAPER_TABLE[1]), 2100 * 2970);
    }

    #[test]
    fn range_merge_adjacent_and_overlap() {
        let (segs, n) = parse_page_ranges("3-5,4-9,10-12").expect("合法 spec 必成");
        assert_eq!(n, 1);
        assert_eq!(segs[0], PageSeg { from: 3, to: 12 });
    }

    #[test]
    fn copies_within_bounds_pass_through() {
        let ok = negotiate(DMORIENT_LANDSCAPE, 42, DMDUP_VERTICAL).expect("合法必成");
        assert_eq!(ok.copies, 42);
        assert!(!ok.copies_clamped);
        assert_eq!(ok.dup, DMDUP_VERTICAL, "VERTICAL = 长边翻页语义保留");
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f025e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
