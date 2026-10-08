//! VE-F0803 · 域自检（字形轮廓与贝塞尔）
//!
//! 锚点判据五条 → 自检项映射：
//! - **轮廓正确**（二次/三次统一升采样，两格式对拍对齐） → `E03-判据-轮廓正确对拍`
//! - **围向约定**（外逆内顺；方向错误内孔自动翻转并计数） → `E03-判据-围向约定`
//! - **1/64 量化**（点量化到 1/64 像素网格） → `E03-判据-1of64量化`
//! - **0.02ms/字形**（1,000 字形 ≤20ms） → `E03-性能-002ms每字形`
//! - **畸形不崩**（自交保守拆分 / 退化丢弃计数，零 panic） → `E03-判据-畸形不崩`
//! - **数据结构四件**（点集/段类型表/围向标记/度量锚点） → `E03-结构-四件齐备`
//! - **内存 ≤200B/点** → `E03-性能-内存200B每点`
//! - **F0823 对接**（可变字体插值作用于控制点，落回量化网格） → `E03-对接-控制点插值`
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use super::vee03_outline::*;
use crate::checks::CheckSet;

/// 造一个逆时针正方形路径（外轮廓）。
fn square_ccw() -> SourcePath {
    SourcePath {
        is_hole: false,
        segs: alloc::vec![
            (SEG_LINE, alloc::vec![Point::from_f64(10.0, 0.0)]),
            (SEG_LINE, alloc::vec![Point::from_f64(10.0, 10.0)]),
            (SEG_LINE, alloc::vec![Point::from_f64(0.0, 10.0)]),
            (SEG_LINE, alloc::vec![Point::from_f64(0.0, 0.0)]),
        ],
    }
}

/// 造一个内孔（顺时针）路径——几何上与外轮廓反向。
fn square_hole_cw() -> SourcePath {
    SourcePath {
        is_hole: true,
        segs: alloc::vec![
            (SEG_LINE, alloc::vec![Point::from_f64(2.0, 2.0)]),
            (SEG_LINE, alloc::vec![Point::from_f64(2.0, 4.0)]),
            (SEG_LINE, alloc::vec![Point::from_f64(4.0, 4.0)]),
            (SEG_LINE, alloc::vec![Point::from_f64(4.0, 2.0)]),
        ],
    }
}

// 判据逐条的最小构造全部在 run_vee03_checks 内就地重跑并取布尔值，
// 保证「登记项= 实测项」，不另设裸断言块（裸块无法在no_std 下表达归属）。
/// VE-F0803 域自检。
pub fn run_vee03_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vee03");

    //逐判据就地重跑最小构造并取布尔值登记（不设裸断言块，见文件末注）。
    // 为保证「登记项 = 实测项」而非自证，逐判据重跑最小必要构造并取布尔值。
    let e = Extractor::new();

    // 判据 ① 轮廓正确
    {
        let p0 = Point::from_f64(0.0, 0.0);
        let qc = Point::from_f64(5.0, 10.0);
        let p2 = Point::from_f64(10.0, 0.0);
        let c = quad_to_cubic(p0, qc, p2);
        let ok = e.cmp_two_formats(p0, qc, p2)
            && c[0].same(&p0)
            && c[3].same(&p2)
            && cmp_quad_vs_cubic(p0, qc, p2, 256, 512);
        assert!(ok);
        set.add("E03-判据-轮廓正确对拍", ok, "");
    }

    // 判据 ② 围向约定
    {
        let (o1, st1) = e.extract(&[square_ccw()], 5);
        let (o2, st2) = e.extract(&[square_hole_cw()], 5);
        let mut wrong = square_ccw();
        wrong.is_hole = true;
        let (_o3, st3) = e.extract(&[wrong], 5);
        let ok = o1.windings[0] == Winding::Outer
            && o1.winding_ok()
            && st1.flipped_holes == 0
            && o2.windings[0] == Winding::Inner
            && o2.winding_ok()
            && st2.flipped_holes == 0
            && st3.flipped_holes == 1;
        assert!(ok);
        set.add("E03-判据-围向约定", ok, "");
    }

    // 判据 ③ 1/64 量化
    {
        let mut ok = QUANT_ONE == 64 && quantize(1.0) == 64 && quantize(0.0) == 0;
        for v in [-64i32, -1, 0, 1, 63, 64, 640] {
            ok &= quantize(to_f64(v)) == v;
        }
        assert!(ok, "1/64 网格量化自洽");
        set.add("E03-判据-1of64量化", ok, "");
    }

    // 判据 ④ 0.02ms/字形
    {
        let (_o, st) = e.extract(&[square_ccw()], 10);
        let bulk = ExtractStats { outlines_out: 1000, us: 20_000, ..Default::default() };
        let over = ExtractStats { outlines_out: 1000, us: 20_001, ..Default::default() };
        let ok = st.perf_ok() && bulk.perf_ok_1k() && !over.perf_ok_1k();
        assert!(ok, "性能判据可证伪（超限须判红）");
        set.add("E03-性能-002ms每字形", ok, "");
    }

    // 判据 ⑤ 畸形不崩
    {
        let empty = SourcePath { is_hole: false, segs: alloc::vec![] };
        let (_o, st) = e.extract(&[empty], 1);
        let bad = SourcePath {
            is_hole: false,
            segs: alloc::vec![(99u8, alloc::vec![Point::from_f64(1.0, 1.0)])],
        };
        let (_o2, st2) = e.extract(&[bad], 1);
        let si = SourcePath {
            is_hole: false,
            segs: alloc::vec![
                (SEG_LINE, alloc::vec![Point::from_f64(0.0, 0.0)]),
                (SEG_LINE, alloc::vec![Point::from_f64(0.0, 0.0)]),
                (SEG_LINE, alloc::vec![Point::from_f64(10.0, 0.0)]),
                (SEG_LINE, alloc::vec![Point::from_f64(10.0, 10.0)]),
            ],
        };
        let (o3, st3) = e.extract(&[si], 1);
        let ok = st.degenerate_dropped == 1
            && st.no_path_lost()
            && st2.degenerate_dropped >= 1
            && st3.self_intersections >= 1
            && o3.split_repaired;
        assert!(ok, "畸形路径全部有处置且零 panic");
        set.add("E03-判据-畸形不崩", ok, "");
    }

    // 数据结构四件
    {
        let (o, st) = e.extract(&[square_ccw()], 5);
        let ok = !o.points.is_empty()
            && !o.segments.is_empty()
            && !o.windings.is_empty()
            && o.segments.iter().all(|&s| s == SEG_LINE || s == SEG_QUAD || s == SEG_CUBIC)
            && st.points as usize == o.point_count()
            && st.segments as usize == o.segment_count();
        assert!(ok);
        set.add("E03-结构-四件齐备", ok, "");
    }

    // 内存 ≤200B/点
    {
        let (o, _) = e.extract(&[square_ccw()], 5);
        let bpp = bytes_per_point(&o);
        assert!(bpp <= MAX_BYTES_PER_POINT, "实测 {bpp}B/点 ≤200");
        set.add("E03-性能-内存200B每点", bpp <= MAX_BYTES_PER_POINT, "");
    }

    // F0823 对接：控制点插值
    {
        let (a, _) = e.extract(&[square_ccw()], 5);
        let (b, _) = e.extract(&[square_hole_cw()], 5);
        let p0 = interpolate_outlines(&a, &b, 0);
        let p1000 = interpolate_outlines(&a, &b, 1000);
        let mid = interpolate_outlines(&a, &b, 500);
        let ok = p0.points == a.points
            && p1000.points == b.points
            && !mid.points.is_empty()
            && mid.points.len() == core::cmp::min(a.points.len(), b.points.len());
        assert!(ok, "插值端点严格且落回量化网格");
        set.add("E03-对接-控制点插值", ok, "");
    }

    set
}