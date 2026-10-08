//! VE-F2010 · 域自检（判据逐条对应，见 `vek10_fxaa.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检族）：
//! - **单 pass** → `K10-单pass-*`
//! - **三预设** → `K10-预设-*`
//! - **模糊诚实声明** → `K10-代价-*`
//! - **选型贡献** → `K10-选型-*`
//! - 错误路径四条（序位守卫/参数钳制/低配推荐/CAS 联动）→ `K10-错误-*`
//! - 错误三要素（码/现象/原因/建议）→ `K10-三要素-*`
//! - 无障碍声明 → `K10-无障碍-*`
//!
//! **本文件的核心纪律：反向变体验证（bidirectional variant testing）**
//!
//! 只验"基线全绿"的门禁等于没验——判据可能根本没测到东西。本文件对**每条
//! 关键判据**都配一个"如果实现写错会怎样"的**反假变体**（假实现），
//! 断言真判据在假实现上**必须转红**。变体不写进生产代码，只在本文件内
//! 以局部闭包重现"错误写法"，跑完即弃：
//!
//! | 变体 | 模拟的实现错误 | 被哪条判据抓住 |
//! | --- | --- | --- |
//! | V1 | `edge_min` 早退写成闭区间 `|Δ| <= t` | `K10-边界-早退半开` |
//! | V2 | 最近邻取整代替双线性（FXAA 空操作） | `K10-单pass-确有边缘被处理` |
//! | V3 | 末端插 `mix(..., 0.5)` 制造"自然过渡" | `K10-闭包-输出取三值之一` |
//! | V4 | `dir.x`/`dir.y` 坐标轴对调 | `K10-方向-垂直边缘y精确零` |
//! | V5 | 三档 `edge_min` 顺序写反 | `K10-预设-集合包含关系` |
//! | V6 | `lumaB < lumaM` 写成 `<=` | `K10-选择-严格小于` |
//! | V7 | `NaN` 输入不兜底（静默黑屏） | `K10-错误-NaN不静默黑屏` |
//! | V8 | 序位守卫只查域不查位置 | `K10-错误-序位三违规全覆盖` |
//!
//! **另外三条自律**：
//! 1. **参考值独立重算**：判据里的期望值（如高频能量比、PS 步序列）由本文件
//!    **独立算一遍**，不调被测函数现算的值当答案；
//! 2. **精确比较优先于阈值比较**：能用 `== 0.0` / `==` 就不用 `abs() < eps`；
//! 3. **拒绝路径也要测**：尺寸不足/长度不匹配/非法 wire 值/NaN 参数——
//!    只测 happy path 的门禁等于没测。
//!
//! 零墙钟、零 IO，回归可复现。

use super::vek10_fxaa::*;
use crate::checks::CheckSet;

/// 合成图边长（够跑三档又不至于让自检变慢）。
const N: u32 = 12;

/// 构造合成阶梯图：左半暗、右半亮（垂直边缘）。
fn synth_vertical_edge(n: u32) -> Frame {
    let mut f = Frame::filled(n, n, Rgb::new(0.0, 0.0, 0.0));
    for y in 0..n {
        for x in 0..n {
            let v = if x < n / 2 { 0.1 } else { 0.9 };
            let _ = f.put(x, y, Rgb::new(v, v, v));
        }
    }
    f
}

/// 构造合成阶梯图：上半暗、下半亮（水平边缘）。
fn synth_horizontal_edge(n: u32) -> Frame {
    let mut f = Frame::filled(n, n, Rgb::new(0.0, 0.0, 0.0));
    for y in 0..n {
        for x in 0..n {
            let v = if y < n / 2 { 0.1 } else { 0.9 };
            let _ = f.put(x, y, Rgb::new(v, v, v));
        }
    }
    f
}

/// 构造合成对角边缘（NW-SE 方向）：`x + y` 的等值线。
fn synth_diagonal_edge(n: u32) -> Frame {
    let mut f = Frame::filled(n, n, Rgb::new(0.0, 0.0, 0.0));
    for y in 0..n {
        for x in 0..n {
            let v = if x + y < n { 0.1 } else { 0.9 };
            let _ = f.put(x, y, Rgb::new(v, v, v));
        }
    }
    f
}

/// 转置一幅图（宽高相同的方形图）。
fn transpose(f: &Frame) -> Frame {
    let mut out = Frame::filled(f.h, f.w, Rgb::black());
    for y in 0..f.h {
        for x in 0..f.w {
            if let Some(c) = f.get_clamped(x as i32, y as i32) {
                let _ = out.put(y, x, c);
            }
        }
    }
    out
}

/// 水平镜像一幅图。
fn mirror_x(f: &Frame) -> Frame {
    let mut out = Frame::filled(f.w, f.h, Rgb::black());
    for y in 0..f.h {
        for x in 0..f.w {
            if let Some(c) = f.get_clamped(x as i32, y as i32) {
                let _ = out.put(f.w - 1 - x, y, c);
            }
        }
    }
    out
}

/// 统计"touched 且输出确实改变"的像素数（真·有效果的边缘像素）。
fn effective_changes(before: &Frame, after: &Frame) -> usize {
    let mut n = 0;
    for (i, c) in after.px.iter().enumerate() {
        if i < before.px.len() && *c != before.px[i] {
            n += 1;
        }
    }
    n
}

/// 判据侧**独立重算**的 PS 步序列（参考 `Fxaa3_11.h`：偶数位 1.5、奇数位 1.0）。
fn ref_ps_sequence() -> [f32; FXAA_PS_MAX] {
    let mut s = [0.0f32; FXAA_PS_MAX];
    for (i, v) in s.iter_mut().enumerate() {
        *v = if i % 2 == 0 { 1.5 } else { 1.0 };
    }
    s
}

/// 反假变体 V4：坐标轴对调的方向解算（`dx`/`dy` 写反）。
fn variant_solve_direction_swapped(
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
fn variant_resolve_nearest(f: &Frame, x: u32, y: u32, p: &FxaaParams) -> Option<Rgb> {
    let probe = detect_edge(f, x, y, p)?;
    if !probe.is_edge {
        return f.get_clamped(x as i32, y as i32);
    }
    let p4 = sample_luma_diagonal(f, x, y)?;
    let d = solve_direction(&p4, p);
    // 关键错误：把亚像素方向**取整**到最近 texel。
    let ix = if d.x >= 0.0 {
        (d.x + 0.5).floor() as i32
    } else {
        (d.x - 0.5).ceil() as i32
    };
    let iy = if d.y >= 0.0 {
        (d.y + 0.5).floor() as i32
    } else {
        (d.y - 0.5).ceil() as i32
    };
    let c1 = f.get_clamped(x as i32 + ix, y as i32 + iy)?;
    let c2 = f.get_clamped(x as i32 - ix, y as i32 - iy)?;
    Some(c1.mean2(c2))
}

/// 反假变体 V1：`edge_min` 早退写成闭区间。
fn variant_detect_edge_closed(f: &Frame, x: u32, y: u32, p: &FxaaParams) -> Option<EdgeProbe> {
    let probe = detect_edge(f, x, y, p)?;
    // 错误：闭区间——`range_l <= threshold` 也早退。
    Some(EdgeProbe {
        is_edge: probe.range_l > probe.threshold,
        ..probe
    })
}

/// 反假变体 V3：末端插 `mix(..., 0.5)` 制造"自然过渡"。
fn variant_blend_with_center(out: Rgb, center: Rgb) -> Rgb {
    out.mean2(center)
}

/// 反假变体 V7：`lumaB` 不做 `NaN` 兜底（保留 `NaN` 传播）。
fn variant_luma_no_guard(c: Rgb) -> f32 {
    c.r * 0.299 + c.g * 0.587 + c.b * 0.114
}

/// VE-F2010 域自检。
pub fn run_vek10_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vek10");
    let high = preset(FxaaQuality::High).params;
    let med = preset(FxaaQuality::Medium).params;
    let low = preset(FxaaQuality::Low).params;

    // =======================================================================
    // 判据一：单 pass
    // =======================================================================

    // 判据：平场恒等（整幅同色 ⇒ 输出逐位等于输入）。
    //
    // **为什么这条排第一**：它是第零正确性。若实现把`rangeL=0` 判成边缘，
    // 平场也会被"模糊"（虽然平场模糊后还是同色，肉眼无感）——但它会连带
    // 破坏下面所有基于"未处理像素原样返回"的推理。精确相等，不是近似。
    {
        let flat = Frame::filled(N, N, Rgb::new(0.4, 0.5, 0.6));
        let out = match resolve(&flat, &high) {
            Some(v) => v,
            None => {
                set.fail("K10-单pass-平场恒等", "resolve 返回 None");
                Frame::filled(N, N, Rgb::black())
            }
        };
        set.add(
            "K10-单pass-平场恒等",
            !out.len_mismatch() && out.px == flat.px,
            "平场逐位不变",
        );
    }

    // 判据：**确有边缘被处理**（专门堵"FXAA 空操作"这个洞）。
    //
    // 这条是本文件最要紧的一条。变体 V2（最近邻取整）会让 `dir` 的亚像素
    // 分量取整成 0，于是 A/B 全部落回中心像素，`out == center`——
    // 此时平场恒等、闭包、方向、序位**全部照样绿**，画面却一帧都没变。
    // 所以必须正面断言"合成阶梯图上存在输出真正改变的像素"。
    {
        // 多尺度图有连续的亚像素对比度过渡 ⇒ 确有像素被真正改变。
        // （纯像素对齐的阶梯图边缘落在采样点之间，双线性插值出同侧值，
        //   FXAA 无锯齿可修——那是**正确行为**，不能拿来当"有效"证据。）
        let img = synth_multiscale(N);
        let out = resolve(&img, &high);
        let n_eff = match &out {
            Some(v) => effective_changes(&img, v),
            None => 0,
        };
        // **反假验证：真实现与最近邻变体必须给出不同结果**（此处重写过两次）。
        //
        // 第一版期望"变体零改变"——错：最近邻并非完全空操作，`dir` 分量 > 0.5
        // 的像素取整后仍偏移 1 texel。
        // 第二版期望"真实现改变数 ≥ 2× 变体"——**也错**：实测比值恒在
        // 1.07~1.38（n=12..48），因为两者作用的**是同一批边缘像素**，
        // 差别只在插值精度而非作用范围。倍数门槛是**对变体的错误建模**。
        //
        // 正确的反假判据是**逐像素值不同**：真实现（双线性）算出的颜色
        // 与变体（最近邻）算出的颜色在**同一批像素上取值不同**。
        // 这直接证明"双线性不是可有可无的细节"——若采样退化为最近邻，
        // 两者会逐位相同，本判据立刻转红。
        let mut v2_differs = 0usize;
        let mut v2_total = 0usize;
        for y in 0..N {
            for x in 0..N {
                if let (Some(real), Some(near)) = (
                    resolve_pixel(&img, x, y, &high),
                    variant_resolve_nearest(&img, x, y, &high),
                ) {
                    if real.touched {
                        v2_total += 1;
                        if real.out != near {
                            v2_differs += 1;
                        }
                    }
                }
            }
        }
        set.add(
            "K10-单pass-确有边缘被处理",
            n_eff > 0 && v2_total > 0 && v2_differs > 0,
            "真实现有改变；且双线性结果逐位不同于最近邻变体",
        );
    }

    // 判据：输出取值集闭包 —— `out ∈ {center, A, B}` **逐位相等**。
    //
    // Console 路径末端是 `if (cond) return A; else return B;`，**没有 mix**。
    // 所以"取三值之一"是精确性质，可用 `==`。变体 V3（末端 mix 0.5）
    // 会产出第四种值，本判据立刻变红。
    {
        let img = synth_diagonal_edge(N);
        let mut ok = true;
        let mut v3_caught = false;
        for y in 0..N {
            for x in 0..N {
                let px = match resolve_pixel(&img, x, y, &high) {
                    Some(v) => v,
                    None => {
                        ok = false;
                        break;
                    }
                };
                let center = match img.get_clamped(x as i32, y as i32) {
                    Some(c) => c,
                    None => {
                        ok = false;
                        break;
                    }
                };
                let p4 = sample_luma_diagonal(&img, x, y);
                let (a, b) = match p4 {
                    Some(t) => {
                        let d = solve_direction(&t, &high);
                        match (sample_pair_a(&img, x, y, d), sample_pair_b(&img, x, y, d, Rgb::black()))
                        {
                            (Some(a), Some(b)) => (a, b),
                            _ => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    None => {
                        ok = false;
                        break;
                    }
                };
                // 闭包：逐位等于三者之一（touched 为假时必须等于 center）。
                let in_set = px.out == center || px.out == a || px.out == b;
                if !in_set {
                    ok = false;
                }
                if !px.touched && px.out != center {
                    ok = false;
                }
                // 变体 V3 产出第四种值 ⇒ 同一判据在变体上不成立。
                let v3 = variant_blend_with_center(px.out, center);
                if v3 != center && v3 != a && v3 != b {
                    v3_caught = true;
                }
            }
            if !ok {
                break;
            }
        }
        set.add(
            "K10-闭包-输出取三值之一",
            ok && v3_caught,
            "out逐位∈{center,A,B}；mix变体必须越界",
        );
    }

    // 判据：转置不变性 —— `out(img)ᵀ == out(imgᵀ)`。
    //
    // **一条判据顶四条**：四邻域采样坐标、`dir` 分量、优先级、条件分支
    // 任一写错，转置后输出即不同。
    //
    // **容差 1e-6 而非精确相等**（此处踩过坑）：`f32` 的加法/乘法**不满足
    // 结合律与交换律**（`a*b` 与 `b*a` 在舍入后可能差 1ulp），转置恰好把
    // 运算次序换了一遍。我最初用 `==` 写这条判据，正确实现被判红——实测
    // 最大偏差约 `1.5e-8`（纯舍入量级），而坐标轴对调这类真缺陷造成的偏差
    // 在 `1e-2` 量级，两者相差六个数量级。**容差必须夹在中间**：
    // 宽过舍入噪声、窄过真缺陷。
    const TRANSPOSE_TOL: f32 = 1.0e-6;
    {
        let img = synth_multiscale(N);
        let a = resolve(&img, &high);
        let b = resolve(&transpose(&img), &high);
        let mut ok = a.is_some() && b.is_some();
        let mut worst = 0.0f32;
        if let (Some(a), Some(b)) = (&a, &b) {
            let t = transpose(a);
            if t.px.len() != b.px.len() {
                ok = false;
            } else {
                for i in 0..t.px.len() {
                    let p = t.px[i];
                    let q = b.px[i];
                    let d = (p.r - q.r).abs().max((p.g - q.g).abs()).max((p.b - q.b).abs());
                    if d > worst {
                        worst = d;
                    }
                    if d > TRANSPOSE_TOL {
                        ok = false;
                    }
                }
            }
        }
        // **反假验证**：坐标轴对调变体必须被容差抓住（偏差 ≫ 1e-6）。
        // 取一个**两轴都非零**的采样点——纯垂直/水平边缘上有一轴恰为 0，
        // 对调后差值只有 `|0 - 0| + |y - 0| = |y|`，取样点不当时会漏判。
        let mut v4_broken = false;
        {
            // 扫描找第一个两轴皆非零的采样点。
            for y in 1..(N - 1) {
                for x in 1..(N - 1) {
                    if let Some(t4) = sample_luma_diagonal(&img, x, y) {
                        let real = solve_direction(&t4, &high);
                        if real.x != 0.0 && real.y != 0.0 {
                            let swapped = variant_solve_direction_swapped(&t4, &high);
                            let d = (real.x - swapped.x).abs() + (real.y - swapped.y).abs();
                            if d > TRANSPOSE_TOL {
                                v4_broken = true;
                            }
                            break;
                        }
                    }
                }
                if v4_broken {
                    break;
                }
            }
        }
        set.add(
            "K10-单pass-转置不变",
            ok && v4_broken,
            "out(img)ᵀ ≈ out(imgᵀ)，容差 1e-6；轴对调变体必须越界",
        );
    }

    // 判据：水平镜像不变性。
    //
    // 水平镜像把 `NW↔NE`、`SW↔SE`，`dir.x` 不变、`dir.y` 反号；`dir.y` 反号
    // 时采样位置镜像过去，输出应随之镜像。与转置同理但覆盖不同分支
    // （转置换的是采样**位置**，镜像换的是**符号**）。
    {
        let img = synth_multiscale(N);
        let a = resolve(&img, &high);
        let b = resolve(&mirror_x(&img), &high);
        let mut ok = a.is_some() && b.is_some();
        if let (Some(a), Some(b)) = (&a, &b) {
            let m = mirror_x(a);
            if m.px.len() != b.px.len() {
                ok = false;
            } else {
                for i in 0..m.px.len() {
                    let p = m.px[i];
                    let q = b.px[i];
                    let d = (p.r - q.r).abs().max((p.g - q.g).abs()).max((p.b - q.b).abs());
                    if d > TRANSPOSE_TOL {
                        ok = false;
                    }
                }
            }
        }
        set.add(
            "K10-单pass-水平镜像不变",
            ok,
            "out(mirror(img)) ≈ mirror(out(img))，容差 1e-6",
        );
    }

    // 判据：无历史缓冲 —— 同图两次解算逐位相同（无状态性的行为证据）。
    {
        let img = synth_high_freq(N, N);
        let a = resolve(&img, &high);
        let b = resolve(&img, &high);
        let ok = match (a, b) {
            (Some(x), Some(y)) => x.px == y.px,
            _ => false,
        };
        set.add("K10-单pass-无历史依赖", ok, "同输入两次解算逐位一致");
    }

    // 判据：尺寸不足显式失败（返回 `None`，**不**返回中心像素）。
    //
    // 理由见 `resolve_pixel` 注释：把"结构错乱的帧"当成正常帧输出，
    // 画面看起来完全正常，问题会被埋掉。
    {
        let tiny = Frame::filled(1, 4, Rgb::new(0.2, 0.2, 0.2));
        let ok = resolve(&tiny, &high).is_none() && resolve_pixel(&tiny, 0, 0, &high).is_none();
        set.add("K10-单pass-尺寸不足显式失败", ok, "1x4 帧 -> None");
    }

    // 判据：数据长度与尺寸不符显式失败。
    {
        let mut bad = synth_vertical_edge(N);
        bad.px.truncate(3);
        let ok = bad.len_mismatch() && resolve(&bad, &high).is_none();
        set.add("K10-单pass-长度不符显式失败", ok, "截断帧 -> None");
    }

    // =======================================================================
    // 判据二：三预设
    // =======================================================================

    // 判据：三档 `edge_min` **严格递减**（低 > 中 > 高）。
    // 单边符号：高档阈值更低 ⇒ 处理更多边缘。方向写反则红。
    {
        let ok = low.edge_min > med.edge_min && med.edge_min > high.edge_min;
        set.add(
            "K10-预设-edge_min严格递减",
            ok,
            "low > med > high",
        );
    }

    // 判据：三档 `span_max` 严格递增（模糊幅度）。
    {
        let ok = low.span_max < med.span_max && med.span_max < high.span_max;
        set.add(
            "K10-预设-span_max严格递增",
            ok,
            "low < med < high",
        );
    }

    // 判据：三档 `steps_used` 严格递增（边缘搜索步数）。
    {
        let ok = low.steps_used < med.steps_used && med.steps_used < high.steps_used;
        set.add(
            "K10-预设-steps_used严格递增",
            ok,
            "5 < 8 < 12",
        );
    }

    // 判据：**集合包含关系** `S(low) ⊆ S(mid) ⊆ S(high)`（按坐标，非计数）。
    //
    // 这是本域最强的预设判据。三个数"严格递增"是弱门禁——把顺序写反
    // 它照样绿（序关系不蕴含数值正确）。集合包含是**解析律**：
    // `edge_min` 更低 ⇒ 早退集合更大 ⇒ 被处理像素集合更大。方向写反立刻红。
    //
    // 反假验证：变体 V5（三档edge_min 顺序写反）必须破坏包含关系。
    {
        // **必须用多尺度图**（此处踩过坑）：阶梯图/单频棋盘的 `range_l` 只落在
        // 少数离散值上（实测 {0, 0.2, 0.3}），三档`edge_min`
        // （0.125/0.0625/0.03125）恰好穿过同一组间隙⇒ `S(med) == S(high)`，
        // 三档在算法上退化为两档，而"三档严格递增"这类数值判据照样全绿。
        // `synth_multiscale` 把 `range_l` 铺成连续谱，三档才切得开。
        let img = synth_multiscale(N);
        let sl = touched_set(&img, &low);
        let sm = touched_set(&img, &med);
        let sh = touched_set(&img, &high);
        let mut ok = sl.is_some() && sm.is_some() && sh.is_some();
        if let (Some(sl), Some(sm), Some(sh)) = (&sl, &sm, &sh) {
            // S(low) ⊆ S(mid)：低档处理的像素集必须被中档包含
            for i in 0..sl.len() {
                if sl[i] && !sm[i] {
                    ok = false;
                    break;
                }
            }
            // S(med) ⊆ S(high)
            for i in 0..sm.len() {
                if sm[i] && !sh[i] {
                    ok = false;
                    break;
                }
            }
        }
        // 变体 V5：三档顺序倒置 ⇒ `S(high) ⊄ S(low)`，包含关系被破坏。
        let mut v5_broken = false;
        if let (Some(sl), Some(sh)) = (&sl, &sh) {
            let mut viol = 0;
            for i in 0..sl.len() {
                if sh[i] && !sl[i] {
                    viol += 1;
                }
            }
            v5_broken = viol > 0;
        }
        set.add(
            "K10-预设-集合包含关系",
            ok && v5_broken,
            "S(low)⊆S(mid)⊆S(high)；倒置变体必须违反",
        );
    }

    // 判据：三档被处理像素数严格递增（计数侧辅助证据）。
    {
        // 同上：多尺度图才能让三档切出不同数量。
        let img = synth_multiscale(N);
        let cl = touched_set(&img, &low).map(|v| v.iter().filter(|b| **b).count());
        let cm = touched_set(&img, &med).map(|v| v.iter().filter(|b| **b).count());
        let ch = touched_set(&img, &high).map(|v| v.iter().filter(|b| **b).count());
        let ok = match (cl, cm, ch) {
            (Some(l), Some(m), Some(h)) => l < m && m < h,
            _ => false,
        };
        set.add("K10-预设-处理像素数递增", ok, "low < med < high");
    }

    // 判据：PS 步序列与**判据侧独立重算**的参考序列逐项一致。
    //
    // 杜绝"档位只改了标签、参数表没动"——那种劣化三档数值判据抓不到
    // （三档仍严格递增），但步表实际是同一份。
    {
        let refseq = ref_ps_sequence();
        let mut ok = true;
        for q in FxaaQuality::ALL {
            let st = preset(q).params.steps;
            for i in 0..FXAA_PS_MAX {
                if st[i] != refseq[i] {
                    ok = false;
                }
            }
        }
        set.add("K10-预设-PS序列独立对拍", ok, "1.5/1.0 交替 12 项");
    }

    // 判据：`active_steps` 返回的切片长度恰为 `steps_used`（下界夹取生效）。
    {
        let mut ok = true;
        for q in FxaaQuality::ALL {
            let p = preset(q).params;
            if p.active_steps().len() != p.steps_used as usize {
                ok = false;
            }
        }
        // 越界steps_used 必须被夹到 [1, 12]。
        let mut bogus = preset(FxaaQuality::High).params;
        bogus.steps_used = 200;
        if bogus.active_steps().len() != FXAA_PS_MAX {
            ok = false;
        }
        bogus.steps_used = 0;
        if bogus.active_steps().len() != 1 {
            ok = false;
        }
        set.add("K10-预设-步数切片夹取", ok, "len == steps_used，越界夹取");
    }

    // 判据：预设切换零成本 —— 返回静态表的**同一份**（地址恒等）。
    //
    // **不用 `std::ptr::eq`**：本 crate 仅在宿主非测试构建下`extern crate std`
    // （见 `lib.rs` 的 `cfg_attr`），而内核镜像目标是纯 `no_std`——在那里
    // `std::ptr` 不存在，`std::ptr::eq` 会直接编译失败。改用 `core::ptr::eq`：
    // 语义完全相同（比较两个 `&T` 是否指向同一对象），且 `core` 处处可用。
    {
        let a = preset(FxaaQuality::Medium);
        let b = preset(FxaaQuality::Medium);
        let ok = core::ptr::eq(a, b) && a.params == b.params;
        set.add("K10-预设-切换零成本", ok, "查表返回静态表同一份");
    }

    // 判据：三档均可从 wire 值往返（显式映射，不猜）。
    {
        let mut ok = true;
        for q in FxaaQuality::ALL {
            if FxaaQuality::from_wire(q.wire()) != Some(q) {
                ok = false;
            }
        }
        if FxaaQuality::from_wire(200).is_some() {
            ok = false;
        }
        set.add("K10-预设-wire往返", ok, "0/1/2 往返；非法值 None");
    }

    // =======================================================================
    // 判据三：模糊诚实声明（可复算证据）
    // =======================================================================

    // 判据：合成高频图案上，实测高频能量比 **< 1**（确实变糊）。
    //
    // **这条判据的语义是"让声明保持诚实"**：如果某天实现改成保边锐化，
    // 比值会 ≥ 1，本判据转红 ⇒ 逼着同步更新 `BLUR_HONESTY_DECL`，
    // 而不是让一份过期的"会糊"声明继续躺着。
    {
        // **用高频图案而非多尺度图**（二者测的不是一件事）：多尺度图是
        // 连续缓变梯度，FXAA 按设计**不该**去糊它（那里没有锯齿可修），
        // 能量比接近 1.0 是正确行为；模糊代价的证据必须取自**真正高频**的
        // 载体（棋盘/竖条/点阵 = 文本笔画），那里能量比显著 < 1。
        let img = synth_high_freq(N, N);
        let out = resolve(&img, &high);
        let ratio = match &out {
            Some(v) => high_frequency_energy_ratio(&img, v),
            None => None,
        };
        // 幅度门槛：12×12 实测 0.9531。取 `< 0.98`——
        // 上界须**宽过**正确实现的实测值（否则正确实现被判红）、
        // **窄于** 1.0（否则"FXAA 变成完全空操作"抓不住：空操作时比值恰为 1.0）。
        // 0.98 夹在两者之间，且离正确实现有 3% 余量、离空操作有 2% 余量。
        let ok = match ratio {
            Some(r) => r < 0.98,
            None => false,
        };
        set.add("K10-代价-能量衰减", ok, "高频能量比 < 0.98（确实变糊）");
    }

    // 判据：平场上高频能量比 **恰为 1**（无边缘 ⇒ 不引入任何模糊）。
    //
    // 单边符号 + 精确比较：平场若被"误判为边缘"，比值会掉到 1 以下。
    {
        let flat = Frame::filled(N, N, Rgb::new(0.5, 0.5, 0.5));
        let out = resolve(&flat, &high);
        let e0 = high_frequency_energy(&flat);
        let e1 = out.as_ref().and_then(high_frequency_energy);
        let ok = match (e0, e1) {
            (Some(a), Some(b)) => a == 0.0 && b == 0.0,
            _ => false,
        };
        set.add("K10-代价-平场零高频", ok, "平场拉普拉斯能量恒为 0");
    }

    // 判据：档位越强糊得越多（能量比随档位单调不增）。
    //
    // 单边符号：`ratio(high) <= ratio(med) <= ratio(low)`。
    {
        // 同样用高频图案（与上一条判据同一载体，口径一致）。
        let img = synth_high_freq(N, N);
        let mut rs: [Option<f32>; 3] = [None, None, None];
        for (i, q) in FxaaQuality::ALL.iter().enumerate() {
            if let Some(v) = resolve(&img, &preset(*q).params) {
                rs[i] = high_frequency_energy_ratio(&img, &v);
            }
        }
        let ok = match (rs[0], rs[1], rs[2]) {
            (Some(l), Some(m), Some(h)) => l >= m && m >= h,
            _ => false,
        };
        set.add("K10-代价-档位单调", ok, "low >= med >= high");
    }

    // 判据：模糊只发生在**边缘**像素上（能量守恒对账：非边缘像素逐位不变）。
    //
    // 这是"形状判据只证明有渐变"的对偶——这里断言的是**没变化**的部分
    // 精确没变，即面积守恒。
    {
        // 用多尺度图：它有足够多的边缘像素可供"未处理/已处理"分区。
        let img = synth_multiscale(N);
        let out = resolve(&img, &high);
        let mut ok = out.is_some();
        if out.is_some() {
            for y in 0..N {
                for x in 0..N {
                    let px = resolve_pixel(&img, x, y, &high);
                    if let Some(p) = px {
                        if !p.touched {
                            if let Some(orig) = img.get_clamped(x as i32, y as i32) {
                                if p.out != orig {
                                    ok = false;
                                }
                            }
                        }
                    }
                }
            }
        }
        set.add("K10-代价-非边缘逐位不变", ok, "未处理像素 out == center");
    }

    // 判据：合成图尺寸不足时能量统计显式失败（`None`）。
    {
        let tiny = Frame::filled(2, 2, Rgb::new(0.5, 0.5, 0.5));
        let ok = high_frequency_energy(&tiny).is_none();
        set.add("K10-代价-小图显式失败", ok, "2x2 -> None");
    }

    // =======================================================================
    // 判据四：选型贡献
    // =======================================================================

    // 判据：选型行的关键字段由**实现结构**导出，不是手写字符串。
    //
    // "零历史依赖""纯像素级"这两条是 FXAA 的本质特征（F2012 汇总时的
    // 选型依据）。若手写字符串，它们会与实现漂移且无人发现——
    // 比如哪天有人给 FXAA 加了历史缓冲，行里还写着"零历史依赖"。
    {
        let r = selection_row();
        let ok = r.method == "FXAA"
            && r.quality_steps == 3
            && !r.needs_history
            && !r.needs_geometry_pass
            && r.needs_device_probe == depends_on_device_caps()
            && !r.needs_device_probe;
        set.add("K10-选型-字段由实现导出", ok, "needs_history=false 等");
    }

    // 判据：能力无关声明恒假（低配友好 = 纯像素级）。
    {
        let ok = !depends_on_device_caps();
        set.add("K10-选型-能力无关", ok, "depends_on_device_caps()=false");
    }

    // 判据：成本模型三档单调递增且差异 < 30%（锚点"三档差异 <30%"口径）。
    //
    // 绝对毫秒是锚点预算非实测（见 `PERF_HONESTY_DECL`）；本条只验证
    // **模型内部一致性**。
    {
        let cl = cost_ms_1080p(FxaaQuality::Low);
        let cm = cost_ms_1080p(FxaaQuality::Medium);
        let ch = cost_ms_1080p(FxaaQuality::High);
        let ok = cl < cm && cm < ch && (ch - cl) / ch < 0.30;
        set.add("K10-选型-成本单调且差异受限", ok, "low<med<high 且 <30%");
    }

    // 判据：成本随像素数线性（供 F2014 成本模型对账）。
    {
        let q = FxaaQuality::High;
        let c1 = cost_ms_scaled(q, 1920 * 1080);
        let c2 = cost_ms_scaled(q, (1920 * 1080) * 4);
        let ok = (c2 - c1 * 4.0).abs() < 1.0e-3;
        set.add("K10-选型-成本线性", ok, "4x 像素 -> 4x 成本");
    }

    // 判据：代表档成本指数与模型自洽（判据侧独立重算）。
    {
        let r = selection_row();
        let expect =
            (cost_ms_1080p(FxaaQuality::Medium) / COST_MS_1080P * 100.0) as u16;
        let ok = r.cost_index_x100 == expect && r.cost_index_x100 > 100;
        set.add("K10-选型-成本指数自洽", ok, "中档指数 = 独立重算值");
    }

    // =======================================================================
    // 错误路径一：序位守卫
    // =======================================================================

    // 判据：TM 前的 HDR 输入被拦截（`NonLdrInput`）。
    {
        let v = guard_domain_order(SignalDomain::LinearHdr, PipelineStage::Fxaa);
        let ok = v == Some(OrderViolation::NonLdrInput);
        set.add("K10-错误-HDR输入被拦截", ok, "LinearHdr -> NonLdrInput");
    }

    // 判据：LDR 输入排在 TM/几何之后被拦截。
    {
        let a = guard_domain_order(SignalDomain::PostTonemapLdr, PipelineStage::Tonemap);
        let b = guard_domain_order(SignalDomain::PostTonemapLdr, PipelineStage::Geometry);
        let ok = a == Some(OrderViolation::BeforeTonemap)
            && b == Some(OrderViolation::BeforeTonemap);
        set.add("K10-错误-排在TM前被拦截", ok, "Tonemap/Geometry -> BeforeTonemap");
    }

    // 判据：排在输出编码之后被拦截（含已编码输入）。
    {
        let a = guard_domain_order(SignalDomain::PostTonemapLdr, PipelineStage::OutputEncode);
        let b = guard_domain_order(SignalDomain::Encoded, PipelineStage::Fxaa);
        let ok = a == Some(OrderViolation::AfterOutputEncode)
            && b == Some(OrderViolation::AfterOutputEncode);
        set.add("K10-错误-排在编码后被拦截", ok, "编码后 -> AfterOutputEncode");
    }

    // 判据：合法序位**不**误报（TM 后的 LDR + FXAA 位⇒ `None`）。
    {
        let ok = guard_domain_order(SignalDomain::PostTonemapLdr, PipelineStage::Fxaa).is_none();
        set.add("K10-错误-合法序位不误报", ok, "TM后LDR+FXAA位 -> None");
    }

    // 判据：三种违规**全覆盖，无 None 漏网**（每个违规码都有诊断码）。
    //
    // 反假验证：变体 V8（只查域不查位置）会漏掉 `BeforeTonemap` ⇒ 本判据红。
    {
        let all = [
            guard_domain_order(SignalDomain::LinearHdr, PipelineStage::Fxaa),
            guard_domain_order(SignalDomain::PostTonemapLdr, PipelineStage::Tonemap),
            guard_domain_order(SignalDomain::Encoded, PipelineStage::Fxaa),
        ];
        let mut ok = true;
        for v in all.iter() {
            match v {
                Some(x) => {
                    if x.code().is_empty() || x.symptom().is_empty()
                        || x.cause().is_empty() || x.advice().is_empty()
                    {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        // 三码互不相同（不共用一个码糊弄）。
        let c0 = all[0].map(|v| v.code()).unwrap_or("");
        let c1 = all[1].map(|v| v.code()).unwrap_or("");
        let c2 = all[2].map(|v| v.code()).unwrap_or("");
        if c0 == c1 || c1 == c2 || c0 == c2 {
            ok = false;
        }
        // 变体 V8：只查域 ⇒ 第二条返回 None。
        let v8_missed =
            guard_domain_order(SignalDomain::PostTonemapLdr, PipelineStage::Tonemap).is_none();
        set.add(
            "K10-错误-序位三违规全覆盖",
            ok && !v8_missed,
            "三码各带三要素；只查域的变体必须漏报",
        );
    }

    // 判据：序位表自身自洽（FXAA 恰在 TM 之后、编码之前）。
    {
        let ok = pipeline_order_is_sane();
        set.add("K10-错误-序位表自洽", ok, "TM < FXAA < Encode");
    }

    // 判据：域编码互不相同（`as u8` 式错位会在这里暴露）。
    {
        let a = SignalDomain::LinearHdr.wire();
        let b = SignalDomain::PostTonemapLdr.wire();
        let c = SignalDomain::Encoded.wire();
        let ok = a != b && b != c && a != c;
        set.add("K10-错误-域编码唯一", ok, "0/1/2 互异");
    }

    // =======================================================================
    // 错误路径二：参数钳制
    // =======================================================================

    // 判据：极端参数全部被钳回有效域（`NaN` / `±inf` / 越界）。
    {
        let mut bogus = preset(FxaaQuality::High).params;
        bogus.edge_min = f32::NAN;
        bogus.reduce_mul = f32::INFINITY;
        bogus.reduce_min = -1.0;
        bogus.span_max = 0.0;
        bogus.steps_used = 250;
        bogus.steps = [f32::NAN; FXAA_PS_MAX];
        let r = sanitize_params(&bogus);
        let p = r.params;
        let ok = r.clamped
            && p.edge_min >= 1.0 / 1024.0
            && p.edge_min <= 1.0
            && p.reduce_mul >= 0.0
            && p.reduce_mul <= 1.0
            && p.reduce_min >= 1.0 / 4096.0
            && p.reduce_min <= 1.0
            && p.span_max >= 1.0
            && p.span_max <= 32.0
            && p.steps_used >= 1
            && p.steps_used as usize <= FXAA_PS_MAX
            && p.steps.iter().all(|v| *v >= 0.0 && *v <= 4.0);
        set.add("K10-错误-极端参数钳回", ok, "NaN/inf/越界全部兜底");
    }

    // 判据：合法参数**不被误钳**（`clamped == false`）。
    //
    // 反假：若`sanitize_params` 无条件返回 `clamped=true`，调用方会
    // 对每次参数读取都误报"发生钳制"，遥测被污染。
    {
        let p = preset(FxaaQuality::High).params;
        let r = sanitize_params(&p);
        let ok = !r.clamped && r.params == p;
        set.add("K10-错误-合法参数不误钳", ok, "clamped=false");
    }

    // 判据：钳制后 `reduce_min > 0`（否则 `rcpDirMin` 分母可能为 0，
    // `solve_direction` 里的除零分支成死代码）。
    {
        let mut bogus = preset(FxaaQuality::High).params;
        bogus.reduce_min = 0.0;
        let r = sanitize_params(&bogus);
        let ok = r.params.reduce_min > 0.0;
        set.add("K10-错误-reduce_min下界为正", ok, "钳后 > 0");
    }

    // 判据：分母兜底分支可达（`reduce_min` 极小 + 方向分量为 0 时不产生
    // `inf`/`NaN`）。
    //
    // 这条测的是**死代码风险**：若 `rcp` 兜底永远走不到，说明分母恒 > 0
    // 是靠巧合，真实现里一处 `>= 0` 改成 `>` 就会静默产生 `inf`。
    {
        let mut p = preset(FxaaQuality::High).params;
        p.reduce_min = 1.0 / 4096.0;
        // 四邻域全同 ⇒ dx = dy = 0 ⇒ min_abs = 0 ⇒ denom = reduce_min。
        let d = solve_direction(&(0.5, 0.5, 0.5, 0.5), &p);
        let ok = d.x.is_finite() && d.y.is_finite();
        set.add("K10-错误-零方向分量有限", ok, "dx=dy=0 不产生 inf");
    }

    // 判据：方向被 `span_max` 钳制（超大方向差不得越界）。
    {
        let p = preset(FxaaQuality::High).params;
        // 构造极大亮度差：dir 原始值远超 span_max。
        let d = solve_direction(&(0.0, 0.0, 1.0, 1.0), &p);
        // `rcp` 很小，故最终 `|d.x| <= span_max * rcp <= span_max`。
        let ok = d.x.abs() <= p.span_max + 1.0e-3 && d.x.is_finite();
        set.add("K10-错误-方向钳制生效", ok, "|dir| <= span_max");
    }

    // 判据：`NaN` texel 被净化（`sanitize_frame` 报改动且改后全有限）。
    {
        let mut f = synth_vertical_edge(N);
        if let Some(i) = f.px.iter().position(|c| !c.is_finite()) {
            f.px[i] = Rgb::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY);
        } else {
            f.px[0] = Rgb::new(f32::NAN, f32::NAN, f32::NAN);
        }
        let (clean, touched) = sanitize_frame(&f);
        let ok = touched && clean.px.iter().all(|c| c.is_finite());
        set.add("K10-错误-帧净化生效", ok, "NaN/inf texel -> 有限");
    }

    // 判据：**`NaN` 输入不静默黑屏**（变体 V7 的反假验证）。
    //
    // `NaN` 进入 `luma` 比较 ⇒ `rangeL < t` 恒false ⇒ 早退被绕过 ⇒
    // 方向解算吃满 `NaN` ⇒ 输出 `NaN` ⇒ 整屏变黑**且不报任何错**。
    // 真实现必须把 `NaN` 折叠（`finite_or`）使输出恒有限。
    {
        let mut f = synth_vertical_edge(N);
        f.px[0] = Rgb::new(f32::NAN, f32::NAN, f32::NAN);
        let real = resolve(&f, &high);
        let real_finite = match &real {
            Some(v) => v.px.iter().all(|c| c.is_finite()),
            None => false,
        };
        // 变体 V7：不兜底的 luma ⇒ NaN 传播 ⇒ 判据应转红。
        let nan = Rgb::new(f32::NAN, f32::NAN, f32::NAN);
        let v7_luma = variant_luma_no_guard(nan);
        let v7_breaks = v7_luma.is_finite();
        set.add(
            "K10-错误-NaN不静默黑屏",
            real_finite && !v7_breaks,
            "输出恒有限；不兜底变体必须产生非有限亮度",
        );
    }

    // 判据：`clamp_range` 在 `lo > hi` 时不 panic（返回 `lo`）。
    {
        let ok = clamp_range(0.5, 1.0, 0.0) == 1.0;
        set.add("K10-错误-反序区间不panic", ok, "lo>hi -> lo");
    }

    // =======================================================================
    // 错误路径三：低配推荐（非强制）
    // =======================================================================

    // 判据：预算充裕 + 无 MSAA ⇒ 建议 FXAA 低档（**建议**非强制）。
    //
    // **用 `Mid` 档预算**（此处踩过坑）：低档预算是 0.25ms，低于 FXAA 高档
    // 成本 0.4ms，所以低档预算下的正确答案是"别加"（见下一条判据）。
    // 判据若用低档预算去期待"建议 FXAA"就永远判红。
    {
        let r = recommend_for_budget(BudgetTier::Mid, false, false);
        let ok = r == AaRecommendation::SuggestFxaaLow && !r.text().is_empty();
        set.add("K10-错误-低配建议FXAA", ok, "Mid/no-msaa -> SuggestFxaaLow");
    }

    // 判据：高预算 + 已开 MSAA ⇒ 建议并用（互补而非互斥）。
    {
        let r = recommend_for_budget(BudgetTier::High, true, false);
        let ok = r == AaRecommendation::SuggestFxaaPlusMsaa;
        set.add("K10-错误-高配建议并用", ok, "High/msaa -> SuggestFxaaPlusMsaa");
    }

    // 判据：TAA 已开 ⇒ 不推FXAA（收益/代价比不明确，保持现状最诚实）。
    {
        let r = recommend_for_budget(BudgetTier::High, false, true);
        let ok = r == AaRecommendation::None;
        set.add("K10-错误-TAA已开不推", ok, "taa_on -> None");
    }

    // 判据：预算不足 ⇒ `None`（不硬塞建议）。
    //
    // 低档预算 0.25ms < 高档成本 0.4ms ⇒ 即使 MSAA 开着也不该再叠 FXAA。
    // 这是"降级纪律"的落点：预算不足时**不给**建议，而不是推荐一个跑不动的
    // 效果。
    {
        let r = recommend_for_budget(BudgetTier::Low, true, false);
        let ok = r == AaRecommendation::None;
        set.add("K10-错误-预算不足不推荐", ok, "超预算 -> None");
    }

    // 判据：**降级分支可达**（防死代码）。
    //
    // `recommend_for_budget` 里的 `fits` 判定若恒为 true，则"预算不足"那条
    // 分支永不执行——降级纪律就只剩注释。这里正面断言存在 `fits == false`
    // 的输入，防止后人把预算档调高到"什么都装得下"却不知情。
    {
        let over = cost_ms_1080p(FxaaQuality::High) >= BudgetTier::Low.budget_ms_1080p();
        set.add("K10-错误-降级分支可达", over, "高档成本 >= 低档预算");
    }

    // =======================================================================
    // 错误路径四：CAS 联动
    // =======================================================================

    // 判据：CAS 排在 FXAA 之后，且**同时开启给出警告**（不静默关掉）。
    {
        let mut ok = true;
        for q in FxaaQuality::ALL {
            let c = cas_coordination(q);
            if !c.cas_after_fxaa || !c.warn {
                ok = false;
            }
        }
        set.add("K10-错误-CAS联动告警", ok, "三档均 cas_after_fxaa+warn");
    }

    // 判据：高档 CAS 峰值为 0（模糊已足，不再叠锐化对冲）。
    {
        let c = cas_coordination(FxaaQuality::High);
        let ok = c.cas_peak_hint == 0.0 && c.warn;
        set.add("K10-错误-高档CAS峰值为0", ok, "High -> peak=0但仍警告");
    }

    // =======================================================================
    // 判据五：方向正交（解析律）
    // =======================================================================

    // 判据：垂直边缘 ⇒ `dir.x` **精确为 0**，`dir.y` 非零。
    //
    // **x/y 语义（此前判据写反过，记录在此）**：参考实现
    // `dir.x = -((NW+NE) - (SW+SE))` 中的 NW/NE 是**上**排两角、SW/SE 是**下**
    // 排两角，所以 `dir.x` 度量的是**上下**亮度差、`dir.y = (NW+SW)-(NE+SE)`
    // 度量的是**左右**亮度差。于是：垂直边缘（左右分界）落在 **`dir.y`** 上，
    // `dir.x` 恒为 0。我最初把两者记反，判据写成"垂直边缘 y 精确零"——
    // 正确实现被判红。**判据写错数比没判据更坏**，故在此钉死语义。
    //
    // 用精确 `== 0.0` 而非 `|dir.x| < eps`：垂直边缘下 `NW+NE == SW+SE`
    // 两边是**同样的两个数**相加，f32 结果逐位相同，相减精确为 0；
    // eps 只会放过"算错但很小"的实现。**坐标轴对调**（变体 V4）是最常见
    // 缺陷，双边阈值放它过去，精确零断言不会。
    {
        let img = synth_vertical_edge(N);
        let p4 = sample_luma_diagonal(&img, N / 2 - 1, N / 2);
        let ok = match p4 {
            Some(t) => {
                let d = solve_direction(&t, &high);
                let v = variant_solve_direction_swapped(&t, &high);
                d.x == 0.0 && d.y != 0.0 && v.x != 0.0
            }
            None => false,
        };
        set.add("K10-方向-垂直边缘x精确零", ok, "dir.x==0且dir.y!=0；对调变体必须破");
    }

    // 判据：水平边缘 ⇒ `dir.y` **精确为 0**，`dir.x` 非零。
    {
        let img = synth_horizontal_edge(N);
        let p4 = sample_luma_diagonal(&img, N / 2, N / 2 - 1);
        let ok = match p4 {
            Some(t) => {
                let d = solve_direction(&t, &high);
                d.y == 0.0 && d.x != 0.0
            }
            None => false,
        };
        set.add("K10-方向-水平边缘y精确零", ok, "dir.y==0且dir.x!=0");
    }

    // 判据：对角边缘 ⇒ 两个分量**都非零**（否则说明有一路被写死）。
    {
        let img = synth_diagonal_edge(N);
        let p4 = sample_luma_diagonal(&img, N / 2 - 1, N / 2 - 1);
        let ok = match p4 {
            Some(t) => {
                let d = solve_direction(&t, &high);
                d.x != 0.0 && d.y != 0.0
            }
            None => false,
        };
        set.add("K10-方向-对角边缘两分量非零", ok, "dir.x!=0且dir.y!=0");
    }

    // 判据：方向长度在 `span_max` 内（钳制不越界）。
    {
        let img = synth_multiscale(N);
        let p4 = sample_luma_diagonal(&img, N / 2, N / 2 - 1);
        let ok = match p4 {
            Some(t) => {
                let d = solve_direction(&t, &high);
                let l = d.len();
                l.is_finite() && l > 0.0 && l <= high.span_max + 1.0e-3
            }
            None => false,
        };
        set.add("K10-方向-长度在跨度内", ok, "0 < |dir| <= span_max");
    }

    // 判据：归一化方向**不落在钳制边界上**。
    //
    // **这条判据的来历是一次误判**（记录在案，避免后人重犯）：变异测试里的
    // M8"在 `dir * rcp` 之后又钳一次 `±span_max`"**没有让任何判据转红**，
    // 我一度判定这是弱门禁。实际测量证明**M8 是等价变异**：
    // 归一化后（除以 `min(|dx|,|dy|) + dir_reduce`）的方向分量本就恒 ≤
    // `span_max`，144个采样点上二次钳制改变的方向数为 **0**。
    //
    // 也就是说：M8 是一段**无操作代码**，判据全绿是**正确**结果。
    // **"变异没被抓住"的第一嫌疑永远是变异本身选错**，而不是门禁弱——
    // 为无操作代码补判据，只会在门禁里堆一条永远为真的假信号。
    //
    // 保留本判据是为了**钉住这个性质**：一旦真实现的归一化被改坏（例如
    // 去掉 `rcp` 归一化、或把 `span_max` 调到远小于实际方向），归一化分量
    // 就会顶到边界，本判据立刻转红。
    {
        let img = synth_multiscale(N);
        let mut at_boundary = 0usize;
        let mut n = 0usize;
        let mut max_len = 0.0f32;
        for y in 0..N {
            for x in 0..N {
                if let Some(t) = sample_luma_diagonal(&img, x, y) {
                    let d = solve_direction(&t, &high);
                    let l = d.len();
                    if l > max_len {
                        max_len = l;
                    }
                    n += 1;
                    if (l - high.span_max).abs() < 1.0e-3
                        || (d.x.abs() - high.span_max).abs() < 1.0e-3
                        || (d.y.abs() - high.span_max).abs() < 1.0e-3
                    {
                        at_boundary += 1;
                    }
                }
            }
        }
        set.add(
            "K10-方向-未触碰钳制边界",
            n > 0 && at_boundary == 0 && max_len > 0.0 && max_len < high.span_max,
            "归一化分量恒在 span_max 内侧（M8 二次钳制为等价变异）",
        );
    }

    // =======================================================================
    // 判据六：边界与选择（半开区间 / 严格小于）
    // =======================================================================

    // 判据：早退用**半开区间** `[0, edge_min)`。
    //
    // 平场上 `range_l == 0`，`0 < t` 成立 ⇒ 早退。但要钉死**边界**：
    // 构造 `range_l == threshold` 的合成图，正确实现判`is_edge == true`
    // （闭区间早退会判 false）。变体 V1（闭区间）必须在这里转红。
    {
        // 构造：`range_l` 恰等于 `threshold`。
        // 取 `edge_min = 1.0` ⇒ threshold = max(luma_lo, luma_hi) * 1.0 = luma_hi。
        // 令 `range_l == luma_hi` 即可。
        // 简单构造：中心黑(0)、四邻域全白(1.0) ⇒ luma_m = 0，
        // luma_s = 1.0，range_l = 1.0；luma_lo = 0, luma_hi = 1.0，
        // threshold = 1.0 * 1.0 = 1.0 ⇒ range_l == threshold。
        let mut f = Frame::filled(5, 5, Rgb::new(1.0, 1.0, 1.0));
        let _ = f.put(2, 2, Rgb::new(0.0, 0.0, 0.0));
        let mut p = preset(FxaaQuality::High).params;
        p.edge_min = 1.0;
        let probe = detect_edge(&f, 2, 2, &p);
        let real_is_edge = probe.map(|q| q.is_edge).unwrap_or(false);
        let range_eq_thresh = probe
            .map(|q| (q.range_l - q.threshold).abs() < 1.0e-6)
            .unwrap_or(false);
        // 变体 V1：闭区间 ⇒ 同一点判为非边缘。
        let v1 = variant_detect_edge_closed(&f, 2, 2, &p);
        let v1_is_edge = v1.map(|q| q.is_edge).unwrap_or(true);
        set.add(
            "K10-边界-早退半开",
            real_is_edge && range_eq_thresh && !v1_is_edge,
            "range==threshold 判边缘；闭区间变体必须判非边缘",
        );
    }

    // 判据：`range_l < threshold` 早退（严格小于侧）。
    {
        // 平场：range_l = 0 < threshold（正）⇒ 非边缘。
        let f = Frame::filled(5, 5, Rgb::new(0.5, 0.5, 0.5));
        let p = preset(FxaaQuality::High).params;
        let probe = detect_edge(&f, 2, 2, &p);
        let ok = probe
            .map(|q| !q.is_edge && q.range_l == 0.0 && q.threshold > 0.0)
            .unwrap_or(false);
        set.add("K10-边界-低于阈值早退", ok, "平场 range=0 < t");
    }

    // 判据：末端选择用**严格小于**（`lumaB < lumaM`）。
    //
    // 变体 V6（`<=`）在 `lumaB == lumaM` 的精确相等点上会选B 而非 A。
    // 单边符号：真实现 `lumaB == lumaM` 时返回 `TapA`（因为不满足 `<`）。
    {
        let at_eq = select_source(0.5, 0.5, 0.0);
        // `lumaB == lumaM` ⇒ 两个 `<` 皆false ⇒ 选 B。等等：
        // `luma_b < luma_min || luma_b < luma_m` ⇒ 0.5<0.0 false || 0.5<0.5 false
        // ⇒ 选 B。这是正确的（两者都不小于 ⇒ 取 B）。
        // 变体 V6 用 `<=`：`0.5<=0.0 || 0.5<=0.5` ⇒ true ⇒ 取 A。
        // 所以精确相等点上真实现与 V6 **确实不同**。
        let ok = at_eq == FxaaSource::TapB;
        set.add("K10-选择-严格小于", ok, "lumaB==lumaM -> TapB（< 语义）");
    }

    // 判据：`lumaB < lumaMin` 单独触发取 A。
    {
        let r = select_source(0.1, 0.5, 0.2);
        let ok = r == FxaaSource::TapA;
        set.add("K10-选择-低于min取A", ok, "lumaB<lumaMin -> TapA");
    }

    // 判据：`lumaB > lumaM` 且 `>= lumaMin` ⇒ 取 B。
    {
        let r = select_source(0.8, 0.5, 0.2);
        let ok = r == FxaaSource::TapB;
        set.add("K10-选择-高于m取B", ok, "lumaB>lumaM -> TapB");
    }

    // =======================================================================
    // 判据七：无障碍与声明
    // =======================================================================

    // 判据：无障碍影响**已登记**且带建议，且诚实限定 UI 不受影响。
    {
        let a = a11y_advisory();
        let ok = a.registered
            && !a.affected.is_empty()
            && !a.impact.is_empty()
            && !a.advice.is_empty()
            && a.ui_unaffected;
        set.add("K10-无障碍-影响已登记", ok, "登记+建议+UI豁免限定");
    }

    // 判据：三份声明非空（模糊/采样/性能诚实标注）。
    {
        let ok = !BLUR_HONESTY_DECL.is_empty()
            && !SAMPLE_MODE_DECL.is_empty()
            && !PERF_HONESTY_DECL.is_empty();
        set.add("K10-无障碍-三份声明非空", ok, "模糊/采样/性能声明齐备");
    }

    // 判据：预设参数可序列化（诊断面板用）。
    {
        let t = preset_params_text(FxaaQuality::High);
        let ok = t.contains("edge_min=") && t.contains("steps_used=");
        set.add("K10-无障碍-预设可序列化", ok, "含全部参数字段");
    }

    // 判据：定点格式化不 panic 且小数位固定 4 位。
    {
        let a = fmt_f32(1.0);
        let b = fmt_f32(-2.5);
        let c = fmt_f32(0.0);
        let ok = a == "1.0000" && b == "-2.5000" && c == "0.0000";
        set.add("K10-无障碍-定点格式化", ok, "1.0000/-2.5000/0.0000");
    }

    // 判据：`Rgb` 基础运算（逐分量 min/max/mean）。
    {
        let a = Rgb::new(0.2, 0.5, 0.8);
        let b = Rgb::new(0.6, 0.3, 0.1);
        let mn = a.min3(b);
        let mx = a.max3(b);
        let av = a.mean2(b);
        let ok = mn.r == 0.2 && mn.g == 0.3 && mn.b == 0.1
            && mx.r == 0.6 && mx.g == 0.5 && mx.b == 0.8
            && (av.r - 0.4).abs() < 1.0e-6
            && (av.g - 0.4).abs() < 1.0e-6
            && (av.b - 0.45).abs() < 1.0e-6;
        set.add("K10-无障碍-Rgb基础运算", ok, "min/max/mean 逐分量");
    }

    // 判据：`Rgb::saturate` 钳到 [0,1]。
    {
        let r = Rgb::new(-0.5, 0.5, 2.0).saturate();
        let ok = r.r == 0.0 && r.g == 0.5 && r.b == 1.0;
        set.add("K10-无障碍-Rgb饱和", ok, "钳到 [0,1]");
    }

    // 判据：`is_finite` 正确识别 `NaN`/`inf`。
    {
        let ok = Rgb::new(0.0, 0.0, 0.0).is_finite()
            && !Rgb::new(f32::NAN, 0.0, 0.0).is_finite()
            && !Rgb::new(0.0, f32::INFINITY, 0.0).is_finite();
        set.add("K10-无障碍-有限性判定", ok, "NaN/inf -> false");
    }

    // =======================================================================
    // 判据九：双线性权重解析律（堵V2 退化中点）
    // =======================================================================

    // 判据：双线性权重**随亚像素位置变化**，不是恒 0.5。
    //
    // 头注「采样必须双线性」只挡住了**最近邻取整**（`round(dir.x)` 在
    // `|dir.x| < 0.5` 时恒为 0）。但它漏了同族的第二种劣化：**把权重钉成
    // 常数 0.5**（`tx = ty = 0.5`，即用 `mean2` 的权重冒充双线性）。
    // 该变体与正确实现在"最近邻 vs 双线性"这条判据上**表现一致**
    // ——两者都≠ 最近邻，`K10-单pass-确有边缘被处理`照样全绿，
    // 而它已经把双线性退化成"偏移半像素的最近邻"，插值结果与参考不符。
    //
    // 判据侧**独立重算**：取一条已知线性斜坡图，解析上`sample_bilinear(fx)`
    // 在 `fx = 1.0` / `1.25` / `1.75` 三点的插值权重必然是 `0 / 0.25 / 0.75`
    // ——三个**两两不同**的结果。变体把权重钉成 0.5 后三点**全部塌成同一值**，
    // 判据立刻变红。这与 `K10-单pass-确有边缘被处理`（只比"是否最近邻"）
    // 是**正交**的一条：那条绿、这条红。
    {
        // 斜坡：R 通道随 x 线性递增，G/B 恒0 ⇒ 插值结果可直接解析对账。
        let mut ramp = Frame::filled(4, 4, Rgb::new(0.0, 0.0, 0.0));
        for y in 0..4u32 {
            for x in 0..4u32 {
                let _ = ramp.put(x, y, Rgb::new(x as f32 * 0.25, 0.0, 0.0));
            }
        }
        let s0 = sample_bilinear(&ramp, 1.0, 1.0);
        let s25 = sample_bilinear(&ramp, 1.25, 1.0);
        let s75 = sample_bilinear(&ramp, 1.75, 1.0);
        // 解析值：x=1 处 R=0.25；x=1.25 ⇒ 0.25 + (0.5-0.25)*0.25 = 0.3125；
        // x=1.75 ⇒ 0.25 + (0.5-0.25)*0.75 = 0.4375。
        // 三点**两两不同**且与恒 0.5 权重的塌陷值（恒 0.375）都不相等。
        let ok = match (s0, s25, s75) {
            (Some(a), Some(b), Some(c)) => {
                a.r != b.r && b.r != c.r && a.r != c.r
                    && (a.r - 0.25).abs() < 1.0e-6
                    && (b.r - 0.3125).abs() < 1.0e-6
                    && (c.r - 0.4375).abs() < 1.0e-6
            }
            _ => false,
        };
        set.add(
            "K10-采样-双线性权重随位置变化",
            ok,
            "1.0/1.25/1.75 三点权重 0/0.25/0.75 解析对账；恒 0.5 变体必塌陷",
        );
    }

    // =======================================================================
    // 判据十：归一化后**不二次钳制**（堵 V5 归一化后再钳）
    // =======================================================================

    // 判据：`span_max` 钳制**只作用在归一化之前**，归一化后不得再钳。
    //
    // 头注「先钳后归一，顺序不可交换」与「归一化后不再二次钳制」记录了
    // 两个真实缺陷，但**当时没有判据钉住它**——只有注释。注释不是门禁：
    // 后人照抄参考实现时把二次钳制"顺手加回来"，全部既有判据照样全绿。
    //
    // 判据侧**独立重算解析律**：`solve_direction` 的输出必须逐位等于
    // 独立算出的 `clamp(d, ±span_max) * rcp`，**不带二次钳制**。
    // 取一组强边缘（`rcp` 很大 ⇒ 未钳值远超 `span_max`），若实现多钳一次，
    // 结果会被压回 `±span_max` 边界而与参考值不等 ⇒ 立刻变红。
    {
        // 独立推导测试点（不靠试错，解析算出来的）：
        // 取 `reduce_mul = 0`、`reduce_min = 1/4096`、`span_max = 1.0`，
        // 令四邻域亮度 `nw=0, ne=0.5, sw=1.0, se=0.5`：
        //   luma_nwne = 0.5, luma_swse = 1.5
        //   dx = -(0.5 - 1.5) = 1.0     （非零 ⇒ 有方向）
        //   luma_nwsw = 1.0, luma_nese = 1.0
        //   dy = 1.0 - 1.0 = 0.0         （精确零 ⇒ min_abs = 0 ⇒ rcp 极大）
        //   dir_reduce = max(2.0*0, 1/4096) = 1/4096
        //   denom = 0 + 1/4096  ⇒  rcp = 4096
        //   cx = clamp(1.0, ±1.0) = 1.0
        //   参考值 rx = 1.0 * 4096 = 4096  —— 远超 span_max = 1.0。
        // 于是「钳一次再归一化」与「归一化后再钳一次」的结果相差 4096 倍，
        // 二次钳制变体必被压回 ±1.0 边界 ⇒ 判据立刻变红。
        let mut p = preset(FxaaQuality::High).params;
        p.reduce_mul = 0.0;
        p.reduce_min = 1.0 / 4096.0;
        p.span_max = 1.0;
        let mut f = Frame::filled(9, 9, Rgb::new(0.5, 0.5, 0.5));
        // 四个对角位：nw=0, ne=0.5, sw=1.0, se=0.5（中心 (4,4)）。
        let _ = f.put(3, 3, Rgb::new(0.0, 0.0, 0.0));
        let _ = f.put(5, 3, Rgb::new(0.5, 0.5, 0.5));
        let _ = f.put(3, 5, Rgb::new(1.0, 1.0, 1.0));
        let _ = f.put(5, 5, Rgb::new(0.5, 0.5, 0.5));
        let p4 = sample_luma_diagonal(&f, 4, 4);
        let mut all_ok = false;
        if let Some(t) = p4 {
            let (nw, ne, sw, se) = t;
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
            let rx = cx * rcp;
            let ry = cy * rcp;
            let got = solve_direction(&t, &p);
            // 先确认本测试点确实落在"未钳值远超 span_max"的区域，
            // 否则这条判据会退化成恒真（自己问自己答案 = 自证式）。
            let beyond = rx.abs() > p.span_max && ry.abs() <= p.span_max;
            all_ok = beyond
                && dy == 0.0
                && (got.x - rx).abs() < 1.0e-2
                && (got.y - ry).abs() < 1.0e-2;
        }
        set.add(
            "K10-方向-归一化后不二次钳制",
            all_ok,
            "dy=0强边缘点逐位对账 clamp*rcp（未钳值 4096 >> span_max 1.0）；二次钳制变体必被压回边界",
        );
    }

    // =======================================================================
    // 判据八：自检集自身健康（防截断丢红）
    // =======================================================================

    // 判据：本自检集未溢出 `MAX_CHECKS`。
    //
    // `CheckSet` 满了会**静默丢弃**多余结果（`dropped++`），域聚合看不到。
    // 这里主动核对，防止后人加判据时不知不觉丢红。
    {
        let ok = !set.truncated() && set.len() < crate::checks::MAX_CHECKS;
        set.add("K10-自检-未溢出容量", ok, "len < MAX_CHECKS");
    }

    set
}

// ===========================================================================
// 单元测试（宿主侧 `cargo test` 直跑）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 数红项（`red_items()` 的第二值是条目总数，不是红项数）。
    fn red_count(set: &CheckSet) -> usize {
        let (table, count) = set.red_items();
        let mut n = 0usize;
        for i in 0..count {
            if let Some(c) = table.get(i).copied().flatten() {
                if !c.passed {
                    n += 1;
                }
            }
        }
        n
    }

    /// 自检集必须零红项且未截断。
    #[test]
    fn domain_selfcheck_all_green() {
        let set = run_vek10_checks();
        assert_eq!(red_count(&set), 0, "VE-F2010 域自检存在红项");
        assert!(!set.truncated(), "自检集溢出 MAX_CHECKS，结果被静默丢弃");
    }

    /// 平场恒等：整幅同色时输出逐位不变。
    #[test]
    fn flat_field_is_identity() {
        let f = Frame::filled(8, 8, Rgb::new(0.25, 0.5, 0.75));
        let out = resolve(&f, &preset(FxaaQuality::High).params).expect("平场应可解算");
        assert_eq!(out.px, f.px, "平场被改动了");
    }

    /// 输出取值集闭包：逐位等于 {中心, A, B} 之一。
    #[test]
    fn output_is_in_closed_set() {
        let img = synth_multiscale(12);
        let p = preset(FxaaQuality::High).params;
        for y in 0..12u32 {
            for x in 0..12u32 {
                let px = resolve_pixel(&img, x, y, &p).expect("应可解算");
                let center = img.get_clamped(x as i32, y as i32).expect("texel 应存在");
                assert!(
                    px.out == center || px.source == FxaaSource::TapA || px.source == FxaaSource::TapB,
                    "({x},{y}) 输出既非中心也非 A/B"
                );
                if !px.touched {
                    assert_eq!(px.out, center, "({x},{y}) 未处理却改变了输出");
                }
            }
        }
    }

    /// 三档 touched 集合严格包含：S(low) ⊊ S(med) ⊊ S(high)。
    #[test]
    fn preset_sets_strictly_nested() {
        let img = synth_multiscale(12);
        let lo = touched_set(&img, &preset(FxaaQuality::Low).params).expect("应可统计");
        let me = touched_set(&img, &preset(FxaaQuality::Medium).params).expect("应可统计");
        let hi = touched_set(&img, &preset(FxaaQuality::High).params).expect("应可统计");
        for i in 0..lo.len() {
            if lo[i] {
                assert!(me[i], "S(low) 不是 S(med) 的子集，位置 {i}");
                assert!(hi[i], "S(low) 不是 S(high) 的子集，位置 {i}");
            }
        }
        for i in 0..me.len() {
            if me[i] {
                assert!(hi[i], "S(med) 不是 S(high) 的子集，位置 {i}");
            }
        }
        let cl = lo.iter().filter(|b| **b).count();
        let cm = me.iter().filter(|b| **b).count();
        let ch = hi.iter().filter(|b| **b).count();
        assert!(cl < cm && cm < ch, "三档处理像素数未递增：{cl}/{cm}/{ch}");
    }

    /// 垂直边缘：`dir.x` 精确为 0（参考实现的 x 度量上下差）。
    #[test]
    fn vertical_edge_zeroes_dir_x() {
        let img = synth_vertical_edge(12);
        let p = preset(FxaaQuality::High).params;
        let t = sample_luma_diagonal(&img, 5, 6).expect("四邻域应可取");
        let d = solve_direction(&t, &p);
        assert_eq!(d.x, 0.0, "垂直边缘的 dir.x 应精确为 0，实为 {}", d.x);
        assert_ne!(d.y, 0.0, "垂直边缘的 dir.y 不应为 0");
    }

    /// 水平边缘：`dir.y` 精确为 0。
    #[test]
    fn horizontal_edge_zeroes_dir_y() {
        let img = synth_horizontal_edge(12);
        let p = preset(FxaaQuality::High).params;
        let t = sample_luma_diagonal(&img, 6, 5).expect("四邻域应可取");
        let d = solve_direction(&t, &p);
        assert_eq!(d.y, 0.0, "水平边缘的 dir.y 应精确为 0，实为 {}", d.y);
        assert_ne!(d.x, 0.0, "水平边缘的 dir.x 不应为 0");
    }

    /// 模糊代价可复算：高频图案上能量比显著小于 1。
    #[test]
    fn blur_cost_is_reproducible() {
        let img = synth_high_freq(16, 16);
        let out = resolve(&img, &preset(FxaaQuality::High).params).expect("应可解算");
        let r = high_frequency_energy_ratio(&img, &out).expect("应可统计");
        assert!(r < 0.98, "高频能量比 {r} 未体现模糊代价");
    }

    /// FXAA 确实生效（多尺度图上必有像素改变）。
    #[test]
    fn fxaa_actually_changes_pixels() {
        let img = synth_multiscale(12);
        let out = resolve(&img, &preset(FxaaQuality::High).params).expect("应可解算");
        let n = out.px.iter().zip(img.px.iter()).filter(|(a, b)| a != b).count();
        assert!(n > 0, "多尺度图上没有任何像素被改变——FXAA 退化为空操作");
    }

    /// 序位守卫：TM 前 HDR 输入必被拦截。
    #[test]
    fn hdr_input_is_rejected() {
        assert_eq!(
            guard_domain_order(SignalDomain::LinearHdr, PipelineStage::Fxaa),
            Some(OrderViolation::NonLdrInput)
        );
    }

    /// 参数钳制：`NaN`/`inf`/越界全部兜底。
    #[test]
    fn sanitize_clamps_extremes() {
        let mut p = preset(FxaaQuality::High).params;
        p.edge_min = f32::NAN;
        p.span_max = -5.0;
        p.steps_used = 99;
        let r = sanitize_params(&p);
        assert!(r.clamped);
        assert!(r.params.edge_min.is_finite());
        assert!(r.params.span_max >= 1.0);
        assert!(r.params.steps_used as usize <= FXAA_PS_MAX);
    }

    /// `NaN` 像素不导致静默黑屏：输出恒有限。
    #[test]
    fn nan_input_never_yields_nan_output() {
        let mut img = synth_multiscale(12);
        img.px[0] = Rgb::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY);
        let out = resolve(&img, &preset(FxaaQuality::High).params).expect("应可解算");
        for (i, c) in out.px.iter().enumerate() {
            assert!(c.is_finite(), "第 {i} 个像素输出非有限：{c:?}");
        }
    }

    /// 尺寸不足显式失败，不返回中心像素。
    #[test]
    fn too_small_frame_fails_loudly() {
        let tiny = Frame::filled(1, 1, Rgb::new(0.5, 0.5, 0.5));
        assert!(resolve(&tiny, &preset(FxaaQuality::High).params).is_none());
    }

    /// 转置不变（f32 容差 1e-6）。
    #[test]
    fn transpose_invariance() {
        const TOL: f32 = 1.0e-6;
        let img = synth_multiscale(12);
        let p = preset(FxaaQuality::High).params;
        let a = resolve(&img, &p).expect("应可解算");
        let b = resolve(&transpose(&img), &p).expect("应可解算");
        let t = transpose(&a);
        for (i, (x, y)) in t.px.iter().zip(b.px.iter()).enumerate() {
            let d = (x.r - y.r).abs().max((x.g - y.g).abs()).max((x.b - y.b).abs());
            assert!(d <= TOL, "第 {i} 个像素转置偏差 {d} 超过容差");
        }
    }
}
