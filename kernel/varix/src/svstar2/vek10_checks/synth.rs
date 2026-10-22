//! VE-F2010 · 自检夹具与共享核算引擎（判据锚点见外壳 `vek10_checks.rs` 头注）
//!
//! 本子模块只承载**非判据**部分：
//! - 合成图构造器与像素放置/重排骨架（显性失败，不静默继续）；
//! - 反假变体 V1-V8（跑完即弃，不进生产代码）；
//! - 各判定组共享的纯函数核算核心与消重辅助骨架
//!   （`all_qualities` / `some2` / `some3`，参数化控制流）。
//!
//! 判据函数本体在 `verdicts1` / `verdicts2`；判据名与断言语义不变。
//! 零墙钟、零 IO，回归可复现。


// 内核浮点层：`x86_64-unknown-none` 下 core 不提供浮点 transcendentals，
// 本文件用到的浮点方法统一走 `FloatExt`（转发 libm，见 crate::float 文档）。
use crate::float::FloatExt;
use super::super::vek10_fxaa::*;

/// 合成图边长（够跑三档又不至于让自检变慢）。
pub(crate) const N: u32 = 12;

/// 转置/镜像不变性的比较容差。
/// 容差 1e-6 而非精确相等：f32 转置换了运算次序，纯舍入偏差实测约 1.5e-8，
/// 真缺陷（轴对调）偏差在 1e-2 量级——容差必须夹在中间。
pub(crate) const TRANSPOSE_TOL: f32 = 1.0e-6;

/// 三档预设参数上下文（收敛判据函数签名，避免逐参传递）。
pub(crate) struct PresetCtx {
    pub(crate) low: FxaaParams,
    pub(crate) med: FxaaParams,
    pub(crate) high: FxaaParams,
}

// ── 合成图构造器 ─────────────────────────────────────────────────────

/// 合成图构造骨架：分配 n×n 黑画布，逐像素按明暗函数着色。
/// 像素放置显性处理：坐标全由界内循环生成，`put` 失败（界逻辑被改坏）时
/// 返回空帧让下游判据显性转红——半涂的画布不能充当判据证据。
pub(crate) fn synth_with(n: u32, shade: impl Fn(u32, u32) -> f32) -> Frame {
    let mut f = Frame::filled(n, n, Rgb::new(0.0, 0.0, 0.0));
    for y in 0..n {
        for x in 0..n {
            let v = shade(x, y);
            let placed = f.put(x, y, Rgb::new(v, v, v));
            if !placed {
                return Frame::filled(0, 0, Rgb::black());
            }
        }
    }
    f
}

/// 构造合成阶梯图：左半暗、右半亮（垂直边缘）。
pub(crate) fn synth_vertical_edge(n: u32) -> Frame {
    synth_with(n, |x, _y| if x < n / 2 { 0.1 } else { 0.9 })
}

/// 构造合成阶梯图：上半暗、下半亮（水平边缘）。
pub(crate) fn synth_horizontal_edge(n: u32) -> Frame {
    synth_with(n, |_x, y| if y < n / 2 { 0.1 } else { 0.9 })
}

/// 构造合成对角边缘（NW-SE 方向）：`x + y` 的等值线。
pub(crate) fn synth_diagonal_edge(n: u32) -> Frame {
    synth_with(n, |x, y| if x + y < n { 0.1 } else { 0.9 })
}

/// 判据夹具的单点像素放置（显性处理，不再 `let _ =` 静默丢弃）。
/// 失败时调用方以 `false` 早退并把判据登记为红。
pub(crate) fn put_checked(f: &mut Frame, x: u32, y: u32, v: Rgb) -> bool {
    let placed = f.put(x, y, v);
    placed
}

/// 像素坐标重排骨架：按映射函数把输入像素散布到输出画布。
/// 源像素越界时 `get_clamped` 取边界值（与原实现一致）；`put` 失败返回空帧。
pub(crate) fn remap_pixels(f: &Frame, out_w: u32, out_h: u32, map: impl Fn(u32, u32) -> (u32, u32)) -> Frame {
    let mut out = Frame::filled(out_w, out_h, Rgb::black());
    for y in 0..f.h {
        for x in 0..f.w {
            if let Some(c) = f.get_clamped(x as i32, y as i32) {
                let (dx, dy) = map(x, y);
                let placed = out.put(dx, dy, c);
                if !placed {
                    return Frame::filled(0, 0, Rgb::black());
                }
            }
        }
    }
    out
}

/// 转置一幅图（宽高相同的方形图）。
pub(crate) fn transpose(f: &Frame) -> Frame {
    remap_pixels(f, f.h, f.w, |x, y| (y, x))
}

/// 水平镜像一幅图。
pub(crate) fn mirror_x(f: &Frame) -> Frame {
    remap_pixels(f, f.w, f.h, |x, y| (f.w - 1 - x, y))
}

/// 统计"touched 且输出确实改变"的像素数（真·有效果的边缘像素）。
pub(crate) fn effective_changes(before: &Frame, after: &Frame) -> usize {
    let mut n = 0;
    for (i, c) in after.px.iter().enumerate() {
        if i < before.px.len() && *c != before.px[i] {
            n += 1;
        }
    }
    n
}

/// 两帧逐像素最大通道偏差；长度不齐返回 `None`（不变性判据共用比较核心）。
pub(crate) fn frames_max_channel_diff(a: &Frame, b: &Frame) -> Option<f32> {
    if a.px.len() != b.px.len() {
        return None;
    }
    let mut worst = 0.0f32;
    for (p, q) in a.px.iter().zip(b.px.iter()) {
        let d = (p.r - q.r).abs().max((p.g - q.g).abs()).max((p.b - q.b).abs());
        if d > worst {
            worst = d;
        }
    }
    Some(worst)
}

/// 布尔集合包含：`a` 中为真的位在 `b` 中也必须为真（`S(a) ⊆ S(b)`）。
pub(crate) fn bool_set_subset(a: &[bool], b: &[bool]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| !*x || *y)
}

/// 布尔集合排他：`a` 中存在为真而 `b` 中为假的位（`S(a) ⊄ S(b)`）。
pub(crate) fn bool_set_has_exclusive(a: &[bool], b: &[bool]) -> bool {
    a.iter().zip(b.iter()).any(|(x, y)| *x && !*y)
}

/// 判据侧**独立重算**的 PS 步序列（参考 `Fxaa3_11.h`：偶数位 1.5、奇数位 1.0）。
pub(crate) fn ref_ps_sequence() -> [f32; FXAA_PS_MAX] {
    let mut s = [0.0f32; FXAA_PS_MAX];
    for (i, v) in s.iter_mut().enumerate() {
        *v = if i % 2 == 0 { 1.5 } else { 1.0 };
    }
    s
}

// ── 反假变体（V1-V8，跑完即弃） ─────────────────────────────────────

/// 反假变体 V4：坐标轴对调的方向解算（`dx`/`dy` 写反）。
pub(crate) fn variant_solve_direction_swapped(
    p4: &(f32, f32, f32, f32),
    p: &FxaaParams,
) -> EdgeDir {
    let real = solve_direction(p4, p);
    EdgeDir {
        x: real.y,
        y: real.x,
    }
}

/// 反假变体 V2：最近邻取整（把 FXAA 打成空操作）。
pub(crate) fn variant_resolve_nearest(f: &Frame, x: u32, y: u32, p: &FxaaParams) -> Option<Rgb> {
    let probe = detect_edge(f, x, y, p)?;
    if !probe.is_edge {
        return f.get_clamped(x as i32, y as i32);
    }
    let p4 = sample_luma_diagonal(f, x, y)?;
    let d = solve_direction(&p4, p);
    // 关键错误：把亚像素方向**取整**到最近 texel。
    let ix = if d.x >= 0.0 {
        (d.x + 0.5).m_floor() as i32
    } else {
        (d.x - 0.5).m_ceil() as i32
    };
    let iy = if d.y >= 0.0 {
        (d.y + 0.5).m_floor() as i32
    } else {
        (d.y - 0.5).m_ceil() as i32
    };
    let c1 = f.get_clamped(x as i32 + ix, y as i32 + iy)?;
    let c2 = f.get_clamped(x as i32 - ix, y as i32 - iy)?;
    Some(c1.mean2(c2))
}

/// 反假变体 V1：`edge_min` 早退写成闭区间。
pub(crate) fn variant_detect_edge_closed(f: &Frame, x: u32, y: u32, p: &FxaaParams) -> Option<EdgeProbe> {
    let probe = detect_edge(f, x, y, p)?;
    // 错误：闭区间——`range_l <= threshold` 也早退。
    Some(EdgeProbe {
        is_edge: probe.range_l > probe.threshold,
        ..probe
    })
}

/// 反假变体 V3：末端插 `mix(..., 0.5)` 制造"自然过渡"。
pub(crate) fn variant_blend_with_center(out: Rgb, center: Rgb) -> Rgb {
    out.mean2(center)
}

/// 反假变体 V7：`lumaB` 不做 `NaN` 兜底（保留 `NaN` 传播）。
pub(crate) fn variant_luma_no_guard(c: Rgb) -> f32 {
    c.r * 0.299 + c.g * 0.587 + c.b * 0.114
}

/// 变体 V4 反假验证的逐点核算：`Some(true)`=轴对调偏差超容差（变体被抓住）、
/// `Some(false)`=两轴皆非零但未超容差（按原语义本行扫描终止）、`None`=一轴为 0（行内继续采样）。
fn v4_point_verdict(t4: &(f32, f32, f32, f32), p: &FxaaParams, tol: f32) -> Option<bool> {
    let real = solve_direction(t4, p);
    if real.x == 0.0 || real.y == 0.0 {
        return None;
    }
    let swapped = variant_solve_direction_swapped(t4, p);
    let d = (real.x - swapped.x).abs() + (real.y - swapped.y).abs();
    Some(d > tol)
}

/// 变体 V4 反假验证的扫描核心：找「两轴皆非零」的采样点断言偏差超容差
///（纯垂直/水平边缘有一轴恰为 0，取样点不当时对调差值会漏判）。
pub(crate) fn v4_axis_swap_breaks(img: &Frame, p: &FxaaParams, tol: f32) -> bool {
    // 扫描找第一个两轴皆非零的采样点。
    'rows: for y in 1..(N - 1) {
        for x in 1..(N - 1) {
            if let Some(t4) = sample_luma_diagonal(img, x, y) {
                match v4_point_verdict(&t4, p, tol) {
                    // 两轴非零且偏差超容差：变体被抓住。
                    Some(true) => return true,
                    // 两轴非零点已找到但未超容差：跳到下一行（原语义）。
                    Some(false) => continue 'rows,
                    // 一轴为零：本点不可核算，行内继续。
                    None => {}
                }
            }
        }
    }
    false
}

/// 闭包判据（`K10-闭包-输出取三值之一`）的单像素核算。
/// `None`=采样链断裂（判据整体转红）；`Some((in_set, untouched_ok, v3_out))`
/// 为逐项结论：输出是否落在 {center, A, B} 内、未处理像素是否原样、
/// V3 mix 变体是否越界。
pub(crate) fn closure_pixel_verdict(
    img: &Frame,
    x: u32,
    y: u32,
    p: &FxaaParams,
) -> Option<(bool, bool, bool)> {
    let px = resolve_pixel(img, x, y, p)?;
    let center = img.get_clamped(x as i32, y as i32)?;
    let p4 = sample_luma_diagonal(img, x, y)?;
    let d = solve_direction(&p4, p);
    let a = sample_pair_a(img, x, y, d)?;
    let b = sample_pair_b(img, x, y, d, Rgb::black())?;
    // 闭包：逐位等于三者之一（touched 为假时必须等于 center）。
    let in_set = px.out == center || px.out == a || px.out == b;
    let untouched_ok = !px.touched || px.out == center;
    // 变体 V3 产出第四种值 ⇒ 同一判据在变体上不成立。
    let v3 = variant_blend_with_center(px.out, center);
    let v3_out = v3 != center && v3 != a && v3 != b;
    Some((in_set, untouched_ok, v3_out))
}

/// 「归一化后不二次钳制」判据的参考值独立重算：
/// 逐位等于 `clamp(d, ±span_max) * rcp`，**不带二次钳制**。
/// 返回 `(dx, dy, rx, ry)`。
pub(crate) fn reference_dir_components(
    nw: f32,
    ne: f32,
    sw: f32,
    se: f32,
    p: &FxaaParams,
) -> (f32, f32, f32, f32) {
    let luma_nwne = nw + ne;
    let luma_swse = sw + se;
    let dx = -(luma_nwne - luma_swse);
    let dy = (nw + sw) - (ne + se);
    let dir_reduce = {
        let v = (luma_nwne + luma_swse) * p.reduce_mul;
        if v > p.reduce_min {
            v
        } else {
            p.reduce_min
        }
    };
    let min_abs = if dx.abs() < dy.abs() { dx.abs() } else { dy.abs() };
    let denom = min_abs + dir_reduce;
    let rcp = if denom > 1.0e-12 { 1.0 / denom } else { 0.0 };
    let cx = clamp_range(dx, -p.span_max, p.span_max);
    let cy = clamp_range(dy, -p.span_max, p.span_max);
    // 参考值：钳一次，归一化，**不再钳**。
    (dx, dy, cx * rcp, cy * rcp)
}

// ── 判定组共享核算核心（纯函数，各判据独立调用，互不引用） ─────────────

/// 反假 V2 的逐点核算：`None`=任一路解算失败（该点不可核算）；
/// `Some((touched, differs))`=该点是否被真实现触碰、输出是否与变体逐位不同。
pub(crate) fn v2_pixel_verdict(img: &Frame, x: u32, y: u32, p: &FxaaParams) -> Option<(bool, bool)> {
    let real = resolve_pixel(img, x, y, p)?;
    let near = variant_resolve_nearest(img, x, y, p)?;
    let differs = real.touched && real.out != near;
    Some((real.touched, differs))
}

/// 「非边缘逐位不变」判据的逐点核算：`None`=该点不可核算（与原实现一样
/// 跳过不计）；`Some(pass)`=未触碰像素输出是否逐位等于原像素。
pub(crate) fn untouched_pixel_intact(img: &Frame, x: u32, y: u32, p: &FxaaParams) -> Option<bool> {
    let px = resolve_pixel(img, x, y, p)?;
    if px.touched {
        // 已处理像素不在本判据范围（能量守恒对账只看"没变化"的部分）。
        return Some(true);
    }
    let orig = img.get_clamped(x as i32, y as i32)?;
    Some(px.out == orig)
}

/// 变换不变性判据的共享核算核心（转置/镜像两条判据各自调用，互不引用）：
/// `resolve(transform(img))` 与 `transform(resolve(img))` 的最大通道偏差
/// 是否落在容差内。任一路 resolve 失败、长度不齐或偏差超容差 ⇒ `false`。
pub(crate) fn invariance_within_tol(
    img: &Frame,
    transform: impl Fn(&Frame) -> Frame,
    tol: f32,
    p: &FxaaParams,
) -> bool {
    let a = resolve(img, p);
    let b = resolve(&transform(img), p);
    match (a, b) {
        (Some(a), Some(b)) => match frames_max_channel_diff(&transform(&a), &b) {
            Some(worst) => worst <= tol,
            None => false,
        },
        _ => false,
    }
}

/// 方向判据的共享核算核心（各判据独立调用，互不引用）：
/// 取 `(x,y)` 处四邻域亮度样本交给判据自己的断言闭包；采样失败 ⇒ `false`。
pub(crate) fn dir_verdict(
    img: &Frame,
    x: u32,
    y: u32,
    judge: impl Fn(&(f32, f32, f32, f32)) -> bool,
) -> bool {
    match sample_luma_diagonal(img, x, y) {
        Some(t) => judge(&t),
        None => false,
    }
}

/// 三档预设判据的共享核算核心（各判据独立调用，互不引用）：
/// 按字段抽取函数验证三档**严格递增**。
pub(crate) fn preset_field_strictly_increasing<T: PartialOrd>(
    ctx: &PresetCtx,
    get: impl Fn(&FxaaParams) -> T,
) -> bool {
    let lo = get(&ctx.low);
    let mid = get(&ctx.med);
    let hi = get(&ctx.high);
    lo < mid && mid < hi
}

/// 「归一化后不二次钳制」判据的解析推导测试点参数：
/// `reduce_mul = 0`、`reduce_min = 1/4096`、`span_max = 1.0`（推导见 verdicts2）。
pub(crate) fn second_clamp_params() -> FxaaParams {
    let mut p = preset(FxaaQuality::High).params;
    p.reduce_mul = 0.0;
    p.reduce_min = 1.0 / 4096.0;
    p.span_max = 1.0;
    p
}

// ── 消重辅助骨架（参数化控制流；判据独立调用各自的断言闭包） ─────────────

/// 三档全量判定骨架：对每个档位调用 `judge`，全部 `true` 才通过。
/// 消除重复的 `for q in ALL` 控制流；断言语义仍由各判据闭包自带。
pub(crate) fn all_qualities(mut judge: impl FnMut(FxaaQuality) -> bool) -> bool {
    FxaaQuality::ALL.iter().all(|&q| judge(q))
}

/// 双参考对拍骨架：两个可选值**都存在**且断言通过；任一 `None` ⇒ 红。
pub(crate) fn some2<A, B>(a: Option<A>, b: Option<B>, f: impl FnOnce(A, B) -> bool) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => f(x, y),
        _ => false,
    }
}

/// 三参考对拍骨架：三个可选值**都存在**且断言通过；任一 `None` ⇒ 红。
pub(crate) fn some3<A, B, C>(
    a: Option<A>,
    b: Option<B>,
    c: Option<C>,
    f: impl FnOnce(A, B, C) -> bool,
) -> bool {
    match (a, b, c) {
        (Some(x), Some(y), Some(z)) => f(x, y, z),
        _ => false,
    }
}
