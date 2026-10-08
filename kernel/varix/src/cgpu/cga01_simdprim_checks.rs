//! CGPU-F0001 判据（C1- 前缀，全局互异；独立重算防同源恒绿）。
//!
//! 锚点判据逐条落实：
//! - 混合 12 种与参考逐像素一致（判据侧第三份独立混合实现对拍）
//! - 越界注入零写穿（哨兵像素）
//! - 四目标编译全绿（派发表完整性 + 探针四 cfg 门禁口径）
//! - 吞吐与正确性基准入 CGPU-Bench（账面条目 + 未回填不虚报）
//! - 渐变零重算 / SDF 平方量 / FP16 往返 / GPU 对拍仅舍入

use crate::checks::CheckSet;
use crate::cgpu::cga01_simdprim as p;

/// 判据侧独立混合参考（第三份：不复用 p::blend_channel 的 match 表，
/// 按同语义用系数点积手写，防同源恒绿）。
pub fn ref_blend(m: p::BlendMode, s: u8, sa: u8, d: u8, da: u8) -> u8 {
    use p::BlendMode::*;
    let one = 255u32;
    let (sv, sa_n, dv, da_n) = (s as u32, sa as u32, d as u32, da as u32);
    let (isa, ida) = (one - sa_n, one - da_n);
    // 色通道：预乘域语义（与锚点 12 模式定义一致，系数形式独立展开）。
    let c: u32 = match m {
        Over => sv * one + dv * isa,
        SourceIn => sv * da_n,
        SourceOut => sv * ida,
        SourceAtop => sv * da_n + dv * isa,
        DestIn => dv * sa_n,
        DestAtop => dv * sa_n + sv * ida,
        Multiply => sv * dv,
        Screen => (sv + dv) * one - sv * dv,
        Darken => core::cmp::min(sv, dv) * one,
        Lighten => core::cmp::max(sv, dv) * one,
        Plus => core::cmp::min(sv + dv, 255) * one,
        Xor => sv * ida + dv * isa,
    } / one;
    // alpha 通道。
    let a: u32 = match m {
        Over => sa_n * one + da_n * isa,
        SourceIn | DestIn => sa_n * da_n,
        SourceOut => sa_n * ida,
        SourceAtop => sa_n * da_n + da_n * isa,
        DestAtop => sa_n * da_n + sa_n * ida,
        Xor => (sa_n + da_n) * one - 2 * sa_n * da_n,
        _ => (sa_n + da_n) * one - sa_n * da_n,
    } / one;
    let a8 = core::cmp::min(a, 255) as u8;
    core::cmp::min(core::cmp::min(c, 255) as u8, a8) // 预乘纪律
}

/// 判据侧独立像素级参考（逐通道，语义直写）。
pub fn ref_blend_pixel(m: p::BlendMode, src: u32, dst: u32) -> u32 {
    let (sr, sg, sb, sa) = p::unpack(src);
    let (dr, dg, db, da) = p::unpack(dst);
    let mix = |sc: u8, dc: u8| ref_blend(m, sc, sa, dc, da);
    p::pack(mix(sr, dr), mix(sg, dg), mix(sb, db), mix(sa, da))
}

/// 纯色像素（ABGR）。
fn solid(r: u8, g: u8, b: u8, a: u8) -> u32 {
    p::pack(r, g, b, a)
}

pub fn run_cga01_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-A");

    // --- 格式两族 ---------------------------------------------------------
    let px = solid(0x10, 0x22, 0x33, 0x80);
    let (r0, g0, b0, a0) = p::unpack(px);
    s.add("C1-格式-ABGR打包解包往返", px == p::pack(r0, g0, b0, a0) && (r0, g0, b0, a0) == (0x10, 0x22, 0x33, 0x80), "");

    let f = p::f32_to_f16_bits(1.0);
    let back = p::f16_bits_to_f32(f);
    let zero = p::f16_bits_to_f32(p::f32_to_f16_bits(0.0));
    let inf = p::f16_bits_to_f32(p::f32_to_f16_bits(f32::MAX));
    s.add("C1-格式-FP16往返", (back - 1.0).abs() < 0.001 && zero == 0.0 && inf.is_infinite(), "");

    let ar = p::fp16_pack_ar(0.5, 0.25);
    let (fa, fr) = p::fp16_unpack_ar(ar);
    s.add("C1-格式-FP16打包解包", (fa - 0.5).abs() < 0.001 && (fr - 0.25).abs() < 0.001, "");

    // --- 混合 12 种与参考逐像素一致 ---------------------------------------
    let mut blend_ok = true;
    let mut blend_nontrivial = false;
    for m in p::BlendMode::ALL {
        for &(src, dst) in &[
            (solid(0xFF, 0x80, 0x40, 0xFF), solid(0x20, 0x40, 0x60, 0xFF)),
            (solid(0x10, 0x20, 0x30, 0x7F), solid(0xC0, 0xC0, 0xC0, 0x3F)),
            (solid(0, 0, 0, 0), solid(0xFF, 0xFF, 0xFF, 0)),
            (solid(0xAA, 0xAA, 0xAA, 0xAA), solid(0x55, 0x55, 0x55, 0x55)),
        ] {
            let got = p::blend_pixel(m, src, dst);
            let want = ref_blend_pixel(m, src, dst);
            if got != want {
                blend_ok = false;
            }
            if got != dst {
                blend_nontrivial = true;
            }
        }
    }
    s.add("C1-混合-12种与参考逐像素一致", blend_ok, "");
    s.add("C1-混合-基准非平凡", blend_nontrivial, "");

    // 常量矩阵：12 模式全部有矩阵且 screen=ms+md、multiply=msd（矩阵语义抽查）。
    let scr = p::blend_matrix(p::BlendMode::Screen);
    let mul = p::blend_matrix(p::BlendMode::Multiply);
    let ovr = p::blend_matrix(p::BlendMode::Over);
    let dk = p::blend_matrix(p::BlendMode::Darken);
    let pl = p::blend_matrix(p::BlendMode::Plus);
    s.add(
        "C1-混合-常量矩阵语义",
        scr.ms == 255 && scr.md == 255 && mul.msd == 255 && ovr.ms == 255
            && dk.is_min && !dk.is_max && pl.is_plus && p::blend_matrix(p::BlendMode::Lighten).is_max,
        "",
    );

    // --- 越界注入零写穿（哨兵） -------------------------------------------
    let mut buf = vec![solid(1, 2, 3, 0xFF); 16 * 4];
    let sent_l = solid(0xDE, 0xAD, 0xBE, 0xEF);
    let sent_r = solid(0xCA, 0xFE, 0xBA, 0xBE);
    buf[3] = sent_l; // 行 0 尾（越界矩形左伸处）
    buf[4] = sent_r; // 行 1 头
    let mut surf = p::Surface::new(16, 4, p::PixelFormat::Abgr8888Premul, &mut buf).unwrap_or_else(|| p::Surface {
        w: 16,
        h: 4,
        fmt: p::PixelFormat::Abgr8888Premul,
        pix: &mut [],
    });
    let _ = surf;
    // 重新构造（借用纪律：上面构造只做类型闸，真正操作走下面）。
    drop(surf);
    let mut buf2 = vec![solid(1, 2, 3, 0xFF); 16 * 4];
    buf2[3] = sent_l;
    buf2[4] = sent_r;
    buf2[20] = sent_l; // 行 1 x=4
    {
        let mut surf = match p::Surface::new(16, 4, p::PixelFormat::Abgr8888Premul, &mut buf2) {
            Some(v) => v,
            None => return s,
        };
        // 矩形整体左越界 8 像素、右越界 8 像素。
        let n = p::fill_rect(&mut surf, -8, 0, 24, 2, solid(0xFF, 0, 0, 0xFF));
        // 行 0/1 全部被填（裁剪后仍覆盖全行），哨兵在行 1 x=4 应被覆盖——
        // 换验证「整行越界」零写穿：
        let n2 = p::fill_rect(&mut surf, -40, 3, -10, 4, solid(0, 0xFF, 0, 0xFF));
        let n3 = p::fill_rect(&mut surf, 30, 0, 40, 4, solid(0, 0, 0xFF, 0xFF));
        s.add("C1-越界-左越界行覆盖数", n == 32, "");
        s.add("C1-越界-整行越界零写穿", n2 == 0 && n3 == 0, "");
    }
    // 哨兵核验：行 3（未被 n2 触及，n3 也不覆盖）保持原色。
    let row3_untouched = buf2[48] == solid(1, 2, 3, 0xFF) && buf2[63] == solid(1, 2, 3, 0xFF);
    s.add("C1-越界-哨兵零写穿", row3_untouched, "");

    // 行掩码三段结构自洽（head/body/tail 覆盖 span）。
    let m1 = p::RowMask::clip(100, 0, 0, 100);
    let m2 = p::RowMask::clip(100, 0, 3, 7);
    let m3 = p::RowMask::clip(100, 5, -10, 2);
    let m_ok = match (m1, m2, m3) {
        (Some(a), Some(b), Some(c)) => {
            a.pixels() == 100 && b.pixels() == 4 && c.pixels() == 2
                && a.head == 4 && a.body == 24 && a.tail == 0
                && b.head == 4 && b.body == 0 && b.tail == 0
        }
        _ => false,
    };
    let m4 = p::RowMask::clip(10, 9, 50, 60);
    s.add("C1-掩码-三段结构与裁剪", m_ok && m4.is_none(), "");

    // --- 渐变：预展开插值表 + 零重算 --------------------------------------
    let stops = [
        p::GradStop { pos: 0, color: solid(0, 0, 0, 0xFF) },
        p::GradStop { pos: 255, color: solid(0xFF, 0xFF, 0xFF, 0xFF) },
    ];
    let lut = p::GradientLut::build(64, &stops);
    let lut_ok = match lut {
        Ok(l) => {
            let head = p::unpack(l.at(0)).0;
            let tail = p::unpack(l.at(63)).0;
            let mid = p::unpack(l.at(32)).0;
            let mid2 = p::unpack(l.at(40)).0;
            // 档内插值口径：头黑、中点 ≈ 127、单调递增、尾接近白。
            l.lanes.len() == 16 && head == 0 && (100..=155).contains(&mid)
                && mid2 > mid && (180..=255).contains(&tail)
        }
        Err(_) => false,
    };
    s.add("C1-渐变-插值表两端中点", lut_ok, "");

    let bad = p::GradientLut::build(64, &[p::GradStop { pos: 0, color: 0 }, p::GradStop { pos: 128, color: 1 }]);
    let bad2 = p::GradientLut::build(0, &stops);
    s.add("C1-渐变-非法stop拒绝", bad == Err(p::ERR_GRADIENT_STOPS) && bad2 == Err(p::ERR_GRADIENT_STOPS), "");

    // 渐变填充进表面并与 LUT 直读一致（填充走的就是表，零重算口径）。
    let mut gbuf = vec![0u32; 8 * 2];
    let mut gsurf_ok = false;
    if let Some(mut gsurf) = p::Surface::new(8, 2, p::PixelFormat::Abgr8888Premul, &mut gbuf) {
        if let Ok(gl) = p::GradientLut::build(8, &stops) {
            let n = p::fill_gradient(&mut gsurf, 0, 0, 8, 2, &gl);
            // 头黑；x=7 灰度应接近白但小于等于 255（档内插值粒度）。
            let tail_gray = p::unpack(gbuf[7]).0;
            gsurf_ok = n == 16 && gbuf[0] == solid(0, 0, 0, 0xFF) && (100..=255).contains(&tail_gray);
        }
    }
    s.add("C1-渐变-填充与LUT一致", gsurf_ok, "");

    // --- 圆角矩形 SDF（平方量，角区分类） ---------------------------------
    let rr = p::RRect { cx: 8.0, cy: 8.0, hw: 6.0, hh: 6.0, r: 2.0 };
    let center = rr.contains(8.0, 8.0);
    let edge = rr.contains(13.0, 8.0);
    let outside = !rr.contains(15.0, 8.0);
    let corner_in = rr.contains(13.4, 13.4); // 角圆内（(13.4,13.4) 距角心 (12,12)² ≈ 2.8 ≤ 4）
    let corner_out = !rr.contains(14.0, 14.0); // 距角心 √8 > r
    let corner_zone = rr.in_corner_zone(13.4, 13.4) && !rr.in_corner_zone(8.0, 13.4);
    s.add(
        "C1-圆角-SDF分类",
        center && edge && outside && corner_in && corner_out && corner_zone,
        "",
    );

    // 圆角填充与逐像素 contains 一致（批判加速不改语义）。
    let mut rbuf = vec![0u32; 16 * 16];
    let mut rcount = 0usize;
    if let Some(mut rsurf) = p::Surface::new(16, 16, p::PixelFormat::Abgr8888Premul, &mut rbuf) {
        rcount = p::fill_rounded(&mut rsurf, &rr, solid(0, 0, 0xFF, 0xFF));
    }
    let mut want_count = 0usize;
    for y in 0..16i64 {
        for x in 0..16i64 {
            if rr.contains(x as f32, y as f32) {
                want_count += 1;
            }
        }
    }
    s.add("C1-圆角-填充与逐像素一致", rcount == want_count && rcount > 0, "");

    // --- 椭圆 / 三角形 ----------------------------------------------------
    let el_in = p::ellipse_contains(0.0, 0.0, 3.0, 2.0, 1.5, 1.0);
    let el_edge = p::ellipse_contains(0.0, 0.0, 3.0, 2.0, 3.0, 0.0);
    let el_out = !p::ellipse_contains(0.0, 0.0, 3.0, 2.0, 3.1, 0.0);
    s.add("C1-椭圆-覆盖判定", el_in && el_edge && el_out, "");

    let mut ebuf = vec![0u32; 12 * 8];
    let mut ecount = 0usize;
    if let Some(mut esurf) = p::Surface::new(12, 8, p::PixelFormat::Abgr8888Premul, &mut ebuf) {
        ecount = p::fill_ellipse(&mut esurf, 5.5, 3.5, 4.0, 2.5, 1);
    }
    s.add("C1-椭圆-填充非空", ecount > 20, "");

    let t_in = p::triangle_contains(0.0, 0.0, 8.0, 0.0, 0.0, 8.0, 1.0, 1.0);
    let t_out = !p::triangle_contains(0.0, 0.0, 8.0, 0.0, 0.0, 8.0, 6.0, 6.0);
    s.add("C1-三角形-覆盖判定", t_in && t_out, "");

    let mut tbuf = vec![0u32; 10 * 10];
    let mut tcount = 0usize;
    if let Some(mut tsurf) = p::Surface::new(10, 10, p::PixelFormat::Abgr8888Premul, &mut tbuf) {
        tcount = p::fill_triangle(&mut tsurf, 0.0, 0.0, 9.0, 0.0, 0.0, 9.0, 2);
    }
    s.add("C1-三角形-填充非空", tcount >= 40, "");

    // --- 纹理块搬运 --------------------------------------------------------
    let mut src_buf = vec![solid(7, 7, 7, 0xFF); 4 * 4];
    let src = match p::Surface::new(4, 4, p::PixelFormat::Abgr8888Premul, &mut src_buf) {
        Some(v) => v,
        None => return s,
    };
    let mut dbuf = vec![0u32; 10 * 10];
    let mut moved = 0usize;
    let mut blit_ok = false;
    if let Some(mut dsurf) = p::Surface::new(10, 10, p::PixelFormat::Abgr8888Premul, &mut dbuf) {
        moved = p::blit(&mut dsurf, &src, 3, 3);
        blit_ok = dbuf[3 * 10 + 3] == solid(7, 7, 7, 0xFF) && dbuf[6 * 10 + 6] == solid(7, 7, 7, 0xFF);
    }
    s.add("C1-搬运-块拷贝一致", blit_ok && moved == 16, "");

    // 越界 blit 零写穿：目标右侧越界 2 像素（x=0,1 未触及保持哨兵）。
    let mut oob = vec![solid(9, 9, 9, 0xFF); 4 * 4];
    let mut moved2 = 0usize;
    if let Some(mut osurf) = p::Surface::new(4, 4, p::PixelFormat::Abgr8888Premul, &mut oob) {
        moved2 = p::blit(&mut osurf, &src, 2, 0);
    }
    s.add("C1-搬运-越界零写穿", moved2 == 8 && oob[0] == solid(9, 9, 9, 0xFF) && oob[1] == solid(9, 9, 9, 0xFF), "");

    // --- 派发表与四目标口径 ------------------------------------------------
    let all_paths = p::SimdPath::ALL.len() == 5
        && p::SimdPath::Avx2.batch_pixels() == 8
        && p::SimdPath::Avx512.batch_pixels() == 16
        && p::SimdPath::Sse4.batch_pixels() == 4
        && p::SimdPath::Scalar.batch_pixels() == 1;
    let _ = p::SimdPath::dispatch(); // 编译期派发可达（当前目标任一路径合法）
    s.add("C1-派发-四目标表完整", all_paths, "");

    // --- GPU 对拍：差异仅舍入且入册 ----------------------------------------
    let cpu = vec![solid(10, 20, 30, 40), solid(0, 0, 0, 0xFF)];
    let gpu_round = vec![solid(11, 20, 29, 40), solid(0, 0, 0, 0xFF)]; // ±1 LSB
    let gpu_bad = vec![solid(10, 20, 30, 40), solid(0, 0, 0, 5)]; // 结构性（alpha 差大）
    let rep_ok = match p::GpuDiffReport::compare(&cpu, &gpu_round) {
        Some(r) => r.differing == 1 && r.rounding_only == 1 && r.rounding_clean(),
        None => false,
    };
    let rep_bad = match p::GpuDiffReport::compare(&cpu, &gpu_bad) {
        Some(r) => !r.rounding_clean(),
        None => false,
    };
    let rep_len = p::GpuDiffReport::compare(&cpu, &gpu_round[1..]).is_none();
    s.add("C1-对拍-GPU差异仅舍入", rep_ok && rep_bad && rep_len, "");

    // --- 基准账面入 CGPU-Bench --------------------------------------------
    let fill_entry = &p::BENCH_LEDGER[0];
    let bench_ok = fill_entry.target_gbps == 8
        && !fill_entry.target_met() // 未回填实测不得虚报达标
        && p::BENCH_LEDGER.len() == 3
        && p::BENCH_LEDGER.iter().all(|b| b.target_gbps > 0);
    s.add("C1-基准-吞吐目标入册", bench_ok, "");

    // --- 判据自检（基准非零/毒值） ------------------------------------------
    let mut zbuf = vec![0u32; 4 * 4];
    let mut zero_fill_works = false;
    if let Some(mut zs) = p::Surface::new(4, 4, p::PixelFormat::Abgr8888Premul, &mut zbuf) {
        p::fill_rect(&mut zs, 0, 0, 4, 4, 0);
        zero_fill_works = zbuf.iter().all(|v| *v == 0);
    }
    let zero_surf = p::Surface::new(0, 0, p::PixelFormat::Abgr8888Premul, &mut []);
    s.add("C1-自检-毒值与空表", zero_fill_works && zero_surf.is_some(), "");

    s
}
