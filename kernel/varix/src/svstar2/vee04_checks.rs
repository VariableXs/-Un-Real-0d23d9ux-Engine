//! VE-F0804 · 域自检（字形光栅化器）
//!
//! 锚点判据五条 → 自检项映射：
//! - **边缘平滑**（扫描线 + 精确面积累加，含中间灰阶、无锯齿台阶） → `E04-判据-边缘平滑`
//! - **Gamma 感知**（线性覆盖率 → sRGB 感知权重，整数表零浮点） → `E04-判据-Gamma感知`
//! - **四档粗细**（细/常规/半粗/粗，与 F0826 合成口径 0.02–0.04 对齐） → `E04-判据-四档粗细`
//! - **0.05ms/字形**（1,000 字形/帧 ≤50ms，超限走 F0806 缓存兜底） → `E04-性能-005ms每字形`
//! - **分块防越界**（字号 >256px 分块，单块 ≤256²） → `E04-防护-分块防越界`
//! - **退化轮廓**（零面积 → 空位图并计数） → `E04-防护-退化轮廓计数`
//! - **超限显性拒绝**（不静默截断） → `E04-防护-超限显性拒绝`
//! - **小字号可辨**（≤12px 保留中间灰阶） → `E04-质量-小字号可辨`
//! - **LCD 三通道独立覆盖** → `E04-可选-LCD三通道独立`
//! - **数据结构四件**（字形 ID/字号/变换/目标位图句柄） → `E04-结构-任务四件`
//! - **F0806 图集对接**（槽位落位 + 越界判定） → `E04-对接-图集槽位`
//! - **F0816 后端注册接口**（换后端不改调用面） → `E04-对接-后端注册接口`
//! - **上游 F0803 对接**（轮廓 → 轮廓集） → `E04-对接-上游轮廓适配`
//! - **非零环绕**（内孔被抵消） → `E04-判据-内孔环绕抵消`
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

// 上游 F0803 的点与围向类型须显式引入：本模块的 `use super::vee04_raster::*`
// 只能拿到 vee04 内的**公开**项，而 vee04 对 Point/Winding 的引入是私有的
// （私有 use 不参与 glob 再导出）——漏这一行会在 Point/Winding 上报 E0433。
use super::vee03_outline::{Point, Winding};
use super::vee04_raster::*;
use crate::checks::CheckSet;

/// 造一个 16×16 像素正方形轮廓（外轮廓，逆时针）。
fn square_16() -> ContourSet {
    let pts = [
        Point::from_f64(0.0, 0.0),
        Point::from_f64(16.0, 0.0),
        Point::from_f64(16.0, 16.0),
        Point::from_f64(0.0, 16.0),
    ];
    let mut cs = ContourSet::new();
    cs.contours
        .push(contour_from_polyline(&pts, Winding::Outer));
    cs
}

/// 造一个 16×16 方框（外逆内顺——验证非零环绕抵消）。
fn square_with_hole_16() -> ContourSet {
    let outer = [
        Point::from_f64(0.0, 0.0),
        Point::from_f64(16.0, 0.0),
        Point::from_f64(16.0, 16.0),
        Point::from_f64(0.0, 16.0),
    ];
    let inner = [
        Point::from_f64(4.0, 4.0),
        Point::from_f64(4.0, 12.0),
        Point::from_f64(12.0, 12.0),
        Point::from_f64(12.0, 4.0),
    ];
    let mut cs = ContourSet::new();
    cs.contours
        .push(contour_from_polyline(&outer, Winding::Outer));
    cs.contours
        .push(contour_from_polyline(&inner, Winding::Inner));
    cs
}

/// 造一个共线三点轮廓（零面积退化）。
fn degenerate_collinear() -> ContourSet {
    let pts = [
        Point::from_f64(0.0, 0.0),
        Point::from_f64(8.0, 0.0),
        Point::from_f64(16.0, 0.0),
    ];
    let mut cs = ContourSet::new();
    cs.contours
        .push(contour_from_polyline(&pts, Winding::Outer));
    cs
}

/// VE-F0804 域自检。
pub fn run_vee04_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vee04");

    // 判据 ① 边缘平滑：轴对齐半像素偏移（须有中间灰阶、无硬跳变）
    //           + 斜边渐变（相邻灰阶须递减，不得出现台阶）
    //           + **面积守恒**（本项为判据①的**可证伪核心**）。
    //
    // 为什么必须加面积守恒（变异测试实证）：只判「有中间灰阶 + 无硬跳变」时，
    // 把 `add_span` 的亚像素摊分**整段删掉**（跨度两端一律记满格 `amount`）
    // 仍然全绿——因为「是否有中间灰阶」只证明纵向子扫描线在分层，与横向
    // 覆盖率**摊得准不准**无关。实测该变异把 16px 方块面积从 255.87 抬到
    // 263.90（+3.1%）、左缘覆盖从 63 错成 127，而旧判据一声不响。
    // 面积守恒对**任何**横向摊分误差都敏感（真AA 误差实测仅 −0.05%），
    // 故以此为判据①的硬核；中间灰阶/硬跳变降为辅助形态判据。
    {
        let cs = square_16();
        let (bmp, st) = scanline::rasterize(&cs, 32, 0, RasterOpts::plain());
        let s = smoothness(&bmp);

        // 面积守恒：半像素偏移的 16×16 方块解析面积恒为 256 px²（平移不改
        // 面积）。用 gamma=false 拿**线性**覆盖率，才能与解析值直接对账
        // （过 Gamma 后面积被系统性抬高，非同一口径，不能混用）。
        // 阈值取 ±1%：实测正确实现 −0.05%，而「整像素置位」型变异是
        // +3.1%，两者相差 60 倍余量——阈值不贴着实测值，也不放宽到变异
        // 能逃逸，故判据可证伪。
        const AREA_TOL: f64 = 0.01;
        let (lin, _lst) =
            scanline::rasterize(&cs, 32, 0, RasterOpts { half_width_q: 0, gamma: false });
        let area_px = lin.area() as f64 / 255.0;
        let area_rel = (area_px - 256.0) / 256.0;
        let area_ok = area_rel.abs() <= AREA_TOL;

        // 左缘覆盖率须**恰为半格**：右移 32/64=0.5px ⇒ 首列覆盖 0.5 ⇒
        // 线性值 ≈ 127.5（量化到 127 或 128）。这条比面积更敏感：面积是
        // 全图积分，单点错摊可被别处补偿；边缘剖面是逐像素对账。
        let edge_cov = lin.cov(lin.left.max(0) as u16, 0);
        let edge_ok = edge_cov >= 126 && edge_cov <= 129;

        let ok = !bmp.is_empty()
            && s.mid_pixels > 0
            && s.hard_jumps == 0
            && edge_smooth(&bmp)
            && st.sub_scanlines > 0
            && st.spans > 0
            && area_ok
            && edge_ok;
        assert!(
            ok,
            "边缘平滑失败：中间灰阶 {} 硬跳 {} 面积 {:.3}(解析256, 相对误差{:+.3}%) 左缘覆盖 {}",
            s.mid_pixels,
            s.hard_jumps,
            area_px,
            area_rel * 100.0,
            edge_cov
        );
        set.add("E04-判据-边缘平滑", ok, "");
    }

    // 判据 ①之二：斜边渐变——边缘剖面须出现≥3 档递减灰阶（真抗锯齿的斜面
    // 是渐变而非台阶）。这是比「有无中间灰阶」更强的判据。
    {
        let wedge = {
            // 斜边三角形：左竖边 + 两条斜边。
            let pts = [
                Point::from_f64(0.0, 0.0),
                Point::from_f64(24.0, 12.0),
                Point::from_f64(0.0, 24.0),
            ];
            let mut cs = ContourSet::new();
            cs.contours
                .push(contour_from_polyline(&pts, Winding::Outer));
            cs
        };
        let (bmp, _st) = scanline::rasterize(&wedge, 0, 0, RasterOpts::plain());
        let s = smoothness(&bmp);
        // 取中间一行，找右缘向内的连续非零像素，统计其灰阶层数。
        let y = bmp.h / 2;
        let mut ramp: Vec<u8> = Vec::new();
        for x in (0..bmp.w).rev() {
            let c = bmp.cov(x, y);
            if c != 0 {
                ramp.push(c);
            } else {
                break;
            }
        }
        let mut uniq: Vec<u8> = Vec::new();
        for v in ramp.iter() {
            if !uniq.contains(v) {
                uniq.push(*v);
            }
        }
        let ok = s.hard_jumps == 0 && ramp.len() >= 2 && uniq.len() >= 3 && s.mid_pixels > 0;
        assert!(
            ok,
            "斜边渐变失败：剖面 {:?} 去重 {} 档硬跳 {}",
            ramp,
            uniq.len(),
            s.hard_jumps
        );
        set.add("E04-判据-斜边渐变无台阶", ok, "");
    }

    // 判据 ①之一：内孔被非零环绕抵消（外逆内顺约定在光栅化侧生效）。
    {
        let cs = square_with_hole_16();
        let (bmp, _st) = scanline::rasterize(&cs, 0, 0, RasterOpts::plain());
        let ok = bmp.cov(8, 8) == 0 && bmp.cov(8, 2) > 200;
        assert!(ok, "内孔未抵消：中心 {} 上边 {}", bmp.cov(8, 8), bmp.cov(8, 2));
        set.add("E04-判据-内孔环绕抵消", ok, "");
    }

    // 判据 ② Gamma 感知：整数表形状 + 中间调被提亮 + 应用后确实改变数据。
    //
    // 抽查**全表分位**（不只高索引）：Gamma 感知的关键在**低/中覆盖**被显著
    // 提亮，若只抽查 LUT[128]/LUT[224]，把 LUT[0..64] 整段改坏仍会全绿——
    // 那是判据采样不足，不是实现正确。故对多个分位点做**数值对照**
    // （`round(255·(i/255)^(1/2.2))`，容差 ±2/255），任一处对不上即判红。
    {
        fn gamma_ref(i: u32) -> f64 {
            255.0 * (i as f64 / 255.0).powf(1.0 / 2.2)
        }
        let mut mono = true;
        for i in 1..256usize {
            if GAMMA_LUT[i] < GAMMA_LUT[i - 1] {
                mono = false;
            }
        }
        // 分位点数值对照（含低区——最易被改坏且最影响观感）。
        let mut table_ok = true;
        let mut worst = 0i32;
        for i in [0u32, 1, 4, 16, 32, 48, 64, 96, 128, 160, 192, 224, 250, 255] {
            let expect = gamma_ref(i).round() as i32;
            let d = GAMMA_LUT[i as usize] as i32 - expect;
            if d.abs() > 2 {
                table_ok = false;
            }
            if d.abs() > worst {
                worst = d.abs();
            }
        }
        let lift_low = GAMMA_LUT[64] as i32 - 64;
        let lift_high = GAMMA_LUT[224] as i32 - 224;
        let cs = square_16();
        let (plain, _s1) = scanline::rasterize(&cs, 32, 0, RasterOpts { half_width_q: 0, gamma: false });
        let (gammed, _s2) = scanline::rasterize(&cs, 32, 0, RasterOpts::plain());
        let changed = plain.data != gammed.data;
        let ok = GAMMA_LUT[0] == 0
            && GAMMA_LUT[255] == 255
            && GAMMA_LUT[128] > 128
            && mono
            && table_ok
            && lift_low > lift_high
            && changed
            && gammed.area() > plain.area();
        assert!(
            ok,
            "Gamma 感知失败：单调 {mono} 表对照 {table_ok}(最大偏差 {worst}) 低提升 {lift_low} 高提升 {lift_high} 变化 {changed}"
        );
        set.add("E04-判据-Gamma感知", ok, "");
    }

    // 判据 ③ 四档粗细：em 比例对齐 F0826 + 半宽递增 + 面积严格递增
    //           + **膨胀面积上界律**（本项为判据③的**可证伪核心**）。
    //
    // 为什么必须有面积律（变异测试实证）：只判「四档面积严格递增」时，把
    // 覆盖率归一化分母在 hw>0 时改回 `SUB_SAMPLES`（加粗行不再按**实际
    // 参与**的子扫描线条数归一）**仍然全绿**——四档照样单调递增，但每档
    // 被系统性高估，档间**比例**失真。「递增」只约束序关系，不约束数值
    // 正确性，是弱门禁。
    //
    // 解析律：加粗 = 与边长 d=2hw 的方形做闵可夫斯基和 ⇒ 矩形面积
    // A' = A + d·(w+h) + d²。
    //
    // 判据取**单边上界**而非双边区间，理由是实测（10 字号 × 4 档 = 40 组）：
    // 正确实现的偏差**恒为负**（−1.86% ~ −3.93%，源于 hw 的 F26Dot6 量化与
    // 亚像素栅格离散化使膨胀量略小于名义 d），全域**无一次高估**；而归一化
    // 分母改错型的偏差**恒为正**（最高 +3.22%）。符号本身就是判别式——
    // 比「卡一个双边阈值」更稳：双边阈值一旦取得比正确实现的低估幅度还宽，
    // 高估型变异就会从缝里钻过去（初版取 ±6% 即漏网，实测已复现）。
    // 上界留 0.5% 余量给量化噪声，正确实现余量约 2.4 个百分点。
    {
        const AREA_OVER_TOL: f64 = 0.005;
        let aligned = Weight::aligned_with_f0826();
        let mono = Weight::half_width_monotonic(16);
        let cs = square_16();
        let mut areas: Vec<u64> = Vec::new();
        let mut law_ok = true;
        let mut worst_over: f64 = -1e9;
        let mut all_unsaturated = true;
        for w in Weight::ALL.iter() {
            let opts = RasterOpts::for_weight(16, *w, false);
            let (bmp, _st) = scanline::rasterize(&cs, 0, 0, opts);
            areas.push(bmp.area());
            // 加粗后仍须留有中间灰阶：四档若全撞 255 封顶，档间差异就被抹平。
            if bmp.data.iter().all(|v| *v == 255 || *v == 0) {
                all_unsaturated = false;
            }
            let d = 2.0 * opts.half_width_q as f64 / 64.0;
            let expect = 256.0 + d * 32.0 + d * d;
            let got = bmp.area() as f64 / 255.0;
            let over = (got - expect) / expect;
            if over > worst_over {
                worst_over = over;
            }
            if over > AREA_OVER_TOL {
                law_ok = false;
            }
        }
        let mut strictly = true;
        for i in 1..areas.len() {
            if areas[i] <= areas[i - 1] {
                strictly = false;
            }
        }
        let names_ok = Weight::ALL[0].name() == "细"
            && Weight::ALL[1].name() == "常规"
            && Weight::ALL[2].name() == "半粗"
            && Weight::ALL[3].name() == "粗";
        let ok = aligned && mono && strictly && names_ok && law_ok && all_unsaturated;
        assert!(
            ok,
            "四档粗细失败：对齐 {aligned} 半宽 {mono} 严格 {strictly} 面积 {areas:?} 面积上界 {law_ok}(最大高估{:+.2}%) 未饱和 {all_unsaturated}",
            worst_over * 100.0
        );
        set.add("E04-判据-四档粗细", ok, "");
    }

    // 判据 ④ 0.05ms/字形 + 1,000 字形 ≤50ms + 超限触发 F0806 兜底。
    //
    // 判据**不复用被测常量**做预期值：若把 `PERF_MAX_US_PER_GLYPH` 同时当
    // 「阈值」与「压线预期」，则该断言自证（阈值改成任何值都恒绿）——是空断言。
    // 故此处用锚点原文的**字面量** 50µs / 50,000µs 作预期，并另行断言常量
    // 等于这两个字面量：阈值被改宽时，「压线须达标」与「超限须判红」两端
    // 同时变红，判据因此可证伪。
    const ANCHOR_US_PER_GLYPH: u32 = 50; // 锚点原文「≤0.05ms/字形」
    const ANCHOR_US_1K: u32 = 50_000; // 锚点原文「1,000 字形/帧 ≤50ms」
    {
        let mut one = RasterStats {
            us: ANCHOR_US_PER_GLYPH,
            ..Default::default()
        };
        let edge_ok = one.perf_ok();
        one.us += 1;
        let over_red = !one.perf_ok();
        let mut batch = RasterStats {
            us: ANCHOR_US_1K,
            ..Default::default()
        };
        let batch_edge = batch.perf_ok_1k();
        batch.us += 1;
        let batch_red = !batch.perf_ok_1k();
        // 常量须与锚点字面量一致（防止有人悄悄放宽阈值）。
        let const_aligned = PERF_MAX_US_PER_GLYPH == ANCHOR_US_PER_GLYPH
            && PERF_MAX_US_1K == ANCHOR_US_1K;

        // 实跑：64 字形 × 10µs 累计 640µs，应在预算内且不触发兜底。
        let cs = square_16();
        let mut r = Rasterizer::new();
        let mut jobs: Vec<(ContourSet, RasterTask)> = Vec::new();
        for i in 0..64u32 {
            jobs.push((
                cs.clone(),
                RasterTask::new(
                    i,
                    16,
                    RasterTransform::new(0, 0, Weight::Regular),
                    BitmapHandle { page: 0, slot: i as u16 },
                ),
            ));
        }
        let (bmps, agg) = rasterize_frame(&mut r, &jobs, 10);
        let run_ok = bmps.len() == 64 && agg.us == 640 && agg.perf_ok_1k() && !agg.atlas_fallback;
        // 实跑超限：单帧 1,000µs × 60 = 60ms ⇒ 必须触发兜底标记。
        let (_b, agg_over) = rasterize_frame(&mut r, &jobs, 1000);
        let fallback_ok = agg_over.atlas_fallback && !agg_over.perf_ok_1k();
        let ok = edge_ok && over_red && batch_edge && batch_red && const_aligned && run_ok && fallback_ok;
        assert!(
            ok,
            "性能判据失败：压线 {edge_ok} 可证伪 {over_red} 批量 {batch_edge}/{batch_red} 常量对齐 {const_aligned} 实跑 {run_ok} 兜底 {fallback_ok}"
        );
        set.add("E04-性能-005ms每字形", ok, "");
    }

    // 判据 ⑤ 分块防越界：1024px → 4×4 块，单块仍 256²=64KB。
    // 期望值用**字面量** 256 / 65536（锚点原文「>256px 分块」），
    // 不用 MAX_TILE_DIM 自身——否则阈值被抬宽时断言恒绿（空断言）。
    const ANCHOR_TILE_DIM: u32 = 256;
    const ANCHOR_TILE_BYTES: u32 = 65_536;
    {
        let big = {
            let s = 1024.0f64;
            let pts = [
                Point::from_f64(0.0, 0.0),
                Point::from_f64(s, 0.0),
                Point::from_f64(s, s),
                Point::from_f64(0.0, s),
            ];
            let mut cs = ContourSet::new();
            cs.contours
                .push(contour_from_polyline(&pts, Winding::Outer));
            cs
        };
        let need = TiledBitmap::needs_tiling(1024, 1024) && !TiledBitmap::needs_tiling(64, 64);
        let (tb, st) = tiled::rasterize_tiled(&big, 0, 0, RasterOpts::plain());
        let const_aligned = MAX_TILE_DIM == ANCHOR_TILE_DIM && MAX_TILE_BYTES == ANCHOR_TILE_BYTES;
        let ok = need
            && const_aligned
            && st.tiles == 16
            && tb.within_tile_limit()
            && tb.tile_bytes() == ANCHOR_TILE_BYTES
            && tb.tiles.len() == 16
            // 单块边长不得超过锚点上限（用字面量卡死，不依赖常量）。
            && tb.tile_w as u32 <= ANCHOR_TILE_DIM
            && tb.tile_h as u32 <= ANCHOR_TILE_DIM;
        assert!(
            ok,
            "分块失败：需分块 {need} 常量对齐 {const_aligned} 块数 {} 单块 {}B 上限 {ANCHOR_TILE_DIM}",
            st.tiles,
            tb.tile_bytes()
        );
        set.add("E04-防护-分块防越界", ok, "");
    }

    // 错误路径：退化轮廓（零面积）→ 空位图并计数。
    {
        let cs = degenerate_collinear();
        let flagged = cs.is_degenerate();
        let (bmp, st) = scanline::rasterize(&cs, 0, 0, RasterOpts::plain());
        let empty_ok = ContourSet::new();
        let (b2, s2) = scanline::rasterize(&empty_ok, 0, 0, RasterOpts::plain());
        let ok = flagged && bmp.is_empty() && st.degenerate == 1 && b2.is_empty() && s2.degenerate == 1;
        assert!(ok, "退化处置失败：标记 {flagged} 空 {} 计数 {} 空集 {} 计数 {}", bmp.is_empty(), st.degenerate, b2.is_empty(), s2.degenerate);
        set.add("E04-防护-退化轮廓计数", ok, "");
    }

    // 错误路径：边长超绝对上限 → 显性拒绝（不静默截断）。
    {
        let huge = {
            let s = 5000.0f64;
            let pts = [
                Point::from_f64(0.0, 0.0),
                Point::from_f64(s, 0.0),
                Point::from_f64(s, s),
                Point::from_f64(0.0, s),
            ];
            let mut cs = ContourSet::new();
            cs.contours
                .push(contour_from_polyline(&pts, Winding::Outer));
            cs
        };
        let (b, st) = scanline::rasterize(&huge, 0, 0, RasterOpts::plain());
        let (tb, st2) = tiled::rasterize_tiled(&huge, 0, 0, RasterOpts::plain());
        let ok = st.rejected == 1
            && b.is_empty()
            && st2.rejected == 1
            && tb.tiles.is_empty();
        assert!(ok, "超限拒绝失败：单块 {} 分块 {}", st.rejected, st2.rejected);
        set.add("E04-防护-超限显性拒绝", ok, "");
    }

    // 质量约定：小字号（≤12px）可辨——保留中间灰阶、无硬跳变、主干实心。
    //
    // 造型用**四分之一像素错位**的方块（边落在 x.25 处）：真实小字号字体在
    // ≤12px 时笔画边几乎必然落在像素之间，若取整数对齐的矩形，二值是**几何
    // 使然**，判据会误判光栅器为「不可辨」。故令被测几何自带小数边。
    {
        let small = {
            let pts = [
                Point::from_f64(2.25, 2.25),
                Point::from_f64(9.75, 2.25),
                Point::from_f64(9.75, 9.75),
                Point::from_f64(2.25, 9.75),
            ];
            let mut cs = ContourSet::new();
            cs.contours
                .push(contour_from_polyline(&pts, Winding::Outer));
            cs
        };
        let (bmp, st) = scanline::rasterize(&small, 0, 0, RasterOpts::plain());
        let s = smoothness(&bmp);
        // 位图须落在 ≤12px 量级（小字号场景）。
        let small_enough = bmp.w <= SMALL_PX_MAX as u16 && bmp.h <= SMALL_PX_MAX as u16;
        let ok = SMALL_PX_MAX == 12
            && small_enough
            && s.mid_pixels > 0
            && s.hard_jumps == 0
            && bmp.peak() >= 200
            && st.spans > 0;
        assert!(
            ok,
            "小字号可辨失败：{}x{} 中间灰阶 {} 硬跳 {} 峰值 {}",
            bmp.w, bmp.h, s.mid_pixels, s.hard_jumps, bmp.peak()
        );
        set.add("E04-质量-小字号可辨", ok, "");
    }

    // 可选模式：LCD 亚像素三通道**独立覆盖**（红通道左偏 ⇒ 左缘 R > B）。
    {
        let cs = square_16();
        let (lcd, st) = lcd::rasterize_lcd(&cs, 0, 0, RasterOpts::plain());
        let mut left_edge = false;
        for y in 0..lcd.h {
            for x in 0..lcd.w {
                let r = lcd.cov(0, x, y);
                let b = lcd.cov(2, x, y);
                if r > 0 && b > 0 && r > b {
                    left_edge = true;
                }
            }
        }
        // 通道偏移（±1/3 像素 = ±21/64）使并集画布比单趟宽 1–2 像素
        // （三趟各17/16/17 ⇒ 并集 18）。这是几何使然，不该判红；
        // 真正的机检点是**三通道长度与并集画布一致**（否则按 y·w+x 读越界）。
        let union_ok = lcd.w >= 17 && lcd.w <= 19 && lcd.h == 16;
        let ok = st.lcd == 1
            && lcd.has_channel_divergence()
            && left_edge
            && lcd.channels_consistent()
            && union_ok;
        assert!(
            ok,
            "LCD 失败：标记 {} 分歧 {} 左缘 {left_edge} 一致 {} 并集 {union_ok} 尺寸 {}x{}",
            st.lcd,
            lcd.has_channel_divergence(),
            lcd.channels_consistent(),
            lcd.w,
            lcd.h
        );
        set.add("E04-可选-LCD三通道独立", ok, "");
    }

    // 数据结构四件：字形 ID / 字号 / 变换 / 目标位图句柄。
    {
        let t = RasterTask::new(
            42,
            24,
            RasterTransform::new(32, -16, Weight::SemiBold),
            BitmapHandle { page: 3, slot: 5 },
        );
        let cs = square_16();
        let mut r = Rasterizer::new();
        let (out, tiled_out) = r.rasterize(&cs, &t, 10);
        let ok = t.glyph_id == 42
            && t.px_size == 24
            && t.transform.weight == Weight::SemiBold
            && t.transform.dx == 32
            && out.handle == t.target
            && out.handle.page == 3
            && out.handle.slot == 5
            && !out.bitmap.is_empty()
            && tiled_out.is_none();
        assert!(ok, "任务四件失败：{:?}", out.handle);
        set.add("E04-结构-任务四件", ok, "");
    }

    // F0806 对接：图集槽位落位 + 越界判定。
    {
        let cs = square_16();
        let (bmp, _st) = scanline::rasterize(&cs, 0, 0, RasterOpts::plain());
        let inside = bmp.atlas_slot(BitmapHandle { page: 2, slot: 0 }, 100, 200);
        let outside = bmp.atlas_slot(BitmapHandle { page: 2, slot: 0 }, 2040, 2040);
        let ok = inside.page == 2
            && inside.w == bmp.w
            && inside.h == bmp.h
            && inside.fits_page(2048, 2048)
            && !outside.fits_page(2048, 2048);
        assert!(ok, "图集槽位失败：内 {}x{}@({},{})", inside.w, inside.h, inside.x, inside.y);
        set.add("E04-对接-图集槽位", ok, "");
    }

    // F0816 对接：后端注册接口——换后端不改调用面，越界 id 被拒。
    {
        let mut r = Rasterizer::new();
        let n0 = r.backend_count();
        let default_name = r.active_name();
        let ok = n0 == 1 && default_name == "scanline" && !r.set_active(99);
        assert!(ok, "后端注册基线失败：{n0} {default_name}");
        set.add("E04-对接-后端注册接口", ok, "");
    }

    // 上游 F0803 对接：Outline → ContourSet 适配面。
    {
        let path = super::vee03_outline::SourcePath {
            is_hole: false,
            segs: alloc::vec![
                (super::vee03_outline::SEG_LINE, alloc::vec![Point::from_f64(10.0, 0.0)]),
                (super::vee03_outline::SEG_LINE, alloc::vec![Point::from_f64(10.0, 10.0)]),
                (super::vee03_outline::SEG_LINE, alloc::vec![Point::from_f64(0.0, 10.0)]),
                (super::vee03_outline::SEG_LINE, alloc::vec![Point::from_f64(0.0, 0.0)]),
            ],
        };
        let (o, _st) = super::vee03_outline::Extractor::new().extract(&[path], 1);
        let cs = ContourSet::from_outline(&o, 0, 0);
        let ok = cs.edge_count() > 0 && !cs.is_degenerate();
        assert!(ok, "上游适配失败：边数 {}", cs.edge_count());
        set.add("E04-对接-上游轮廓适配", ok, "");
    }

    // 三次段扁平化无几何漂移（首尾端点重合 + 无水平边残留）。
    {
        let p = [
            Point::from_f64(0.0, 0.0),
            Point::from_f64(5.0, 10.0),
            Point::from_f64(10.0, -5.0),
            Point::from_f64(15.0, 0.0),
        ];
        let edges = flatten_cubic(p);
        let last = edges[CUBIC_FLATTEN_STEPS - 1];
        let no_h = edges.iter().all(|e| e.y0 != e.y1);
        let ok = edges.len() == CUBIC_FLATTEN_STEPS
            && edges[0].x0 == p[0].x
            && edges[0].y0 == p[0].y
            && last.x1 == p[3].x
            && last.y1 == p[3].y
            && no_h;
        assert!(ok, "扁平化失败：{} 段", edges.len());
        set.add("E04-几何-三次扁平化", ok, "");
    }

    set
}
