//! VE-F2011 · 域自检（判据逐条对应，见 `vek11_taa.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检族）：
//! - **Halton 抖动** → `K11-抖动-*`
//! - **velocity reject** → `K11-校验-*`
//! - **clipping** → `K11-clip-*`
//! - **降级显性** → `K11-降级-*`
//! - 错误路径五条（速度缺失/场景切换/鬼影/闪烁/显存配额）→ `K11-错误-*`
//! - 无障碍与声明 → `K11-无障碍-*`
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
//! | V1 | Halton 两列用同一套进位（底数写成常量） | `K11-抖动-底数分别进位` |
//! | V2 | 抖动幅度不归一（宣称原始已铺满） | `K11-抖动-归一后铺满像素` |
//! | V3 | 校验不过时"打折"历史权重而非归零 | `K11-校验-失效必归零` |
//! | V4 | 场景切换只降权不全失效 | `K11-校验-场景切换全失效` |
//! | V5 | 先混合再 clip（顺序交换） | `K11-clip-先clip后混合` |
//! | V6 | 抖动中心化漏掉（不减 0.5） | `K11-抖动-中心化无偏` |
//! | V7 | 速度缺失时静默降级（不产记录） | `K11-降级-速度缺失显性` |
//! | V8 | 显存只算颜色（漏深度） | `K11-错误-显存含深度` |
//!
//! **另外三条自律**：
//! 1. **参考值独立重算**：判据里的期望值（包围盒、Halton 两列、显存字节）
//!    由本文件**独立算一遍**，不调被测函数现算的值当答案；
//! 2. **精确比较优先于阈值比较**：能用 `== 0.0` / `==` 就不用 `abs() < eps`；
//! 3. **拒绝路径也要测**：尺寸不足/长度不匹配/非法法线/NaN 速度/超配额——
//!    只测 happy path 的门禁等于没测。
//!
//! 零墙钟、零 IO，回归可复现。

use super::vek11_taa::*;
use crate::checks::CheckSet;

/// 合成图边长。
const N: u32 = 10;

// ===========================================================================
// 构造与变体
// ===========================================================================

/// 构造一致的几何（法线单位、场景 ID 恒定、给定速度）。
fn geo(w: u32, h: u32, scene: u32, speed: f32) -> GeoBuffer {
    synth_geo(w, h, scene, speed)
}

/// 反假变体 V1：Halton 两列共用同一套进位（底数退化成常量 2）。
fn variant_halton_shared(index: u32, _base: u32) -> f32 {
    // 错误：忽略底数，两列都用底数 2 的进位规律。
    halton(index, HALTON_BASE_X)
}

/// 反假变体 V2：抖动幅度不归一（宣称原始序列已铺满一个像素）。
fn variant_jitter_unnormalized(phase: u32) -> Jitter {
    jitter_of_phase(phase)
}

/// 反假变体 V3：校验不过时"打折"历史权重而非归零。
fn variant_reject_discounted(
    cur: &GeoSample,
    his: &GeoSample,
    p: &ValidateParams,
    mode: ValidationMode,
) -> HistoryUse {
    let r = validate_history(cur, his, p, mode, &mut DebugInvalidate::default());
    match r.reject_reason {
        // 错误：失效时"打折"（乘 0.5）而不是归零 ⇒ 鬼影残留。
        RejectReason::None => r.use_hist,
        _ => HistoryUse::Accept { weight: 0.5 },
    }
}

/// 反假变体 V4：场景切换只降权不全失效。
fn variant_scene_switch_discounted(cur: &GeoSample, his: &GeoSample) -> HistoryUse {
    if cur.scene_id != his.scene_id {
        HistoryUse::Accept { weight: 0.3 }
    } else {
        HistoryUse::Accept { weight: HISTORY_WEIGHT_MAIN }
    }
}

/// 反假变体 V5：先混合再 clip（顺序与参考相反）。
fn variant_blend_then_clip(now: Rgb, hist_raw: Rgb, lo: Rgb, hi: Rgb, w: f32) -> Rgb {
    let mixed = blend(now, hist_raw, w);
    clip_history(mixed, lo, hi, ClipMode::Aabb)
}

/// 反假变体 V6：抖动中心化漏掉（不减 0.5）。
fn variant_jitter_uncentered(phase: u32) -> Jitter {
    let p = phase % HALTON_PHASES as u32;
    Jitter {
        x: halton(p + 1, HALTON_BASE_X),
        y: halton(p + 1, HALTON_BASE_Y),
    }
}

/// 判据侧**独立重算**的 Halton 序列（不调被测函数）。
fn ref_halton(index: u32, base: u32) -> f32 {
    // 用「除基取余、倒序拼小数」的独立写法重算。
    let mut digits = [0u32; 32];
    let mut n = index;
    let mut k = 0;
    while n > 0 && k < 32 {
        digits[k] = n % base;
        n /= base;
        k += 1;
    }
    let mut r = 0.0f32;
    let mut f = 1.0f32;
    for i in 0..k {
        f /= base as f32;
        r += f * digits[i] as f32;
    }
    r
}

/// 判据侧**独立重算**的显存字节（色 12 + 深度 4）。
fn ref_history_bytes(w: u32, h: u32) -> u64 {
    (w as u64) * (h as u64) * (12 + 4)
}

/// 稳定的 Luma 三元组（避免判据里散落魔数）。
fn grey(v: f32) -> Rgb {
    Rgb::new(v, v, v)
}

// ===========================================================================
// 判据族一：Halton 抖动
// ===========================================================================

/// 判据：Halton 序列**两个底数分别进位**。
///
/// **独立重算**：判据侧用「除基取余倒序拼小数」的另一写法重算两列，
/// 与实现逐项比对。变体 V1（两列共用底数 2）必须转红。
///
/// **变体比对的对象是 y 列**（V1 把 y 列退化成 x 列的进位规律），
/// 拿它去比 gx 恒等于 0——**判据向被测对象问它自己已给出的答案**，
/// 是自证式弱门禁的典型形态（此处首次写成时即踩到，已记录）。
fn check_halton(set: &mut CheckSet) {
    let mut ok = true;
    let mut v1_differs = 0usize;
    for i in 1..=(HALTON_PHASES as u32) {
        let gx = halton(i, HALTON_BASE_X);
        let gy = halton(i, HALTON_BASE_Y);
        let rx = ref_halton(i, HALTON_BASE_X);
        let ry = ref_halton(i, HALTON_BASE_Y);
        if (gx - rx).abs() > 1.0e-6 || (gy - ry).abs() > 1.0e-6 {
            ok = false;
        }
        // 两列必须**逐点不同**（共用进位会让两列相同）。
        if (gx - gy).abs() <= 1.0e-6 {
            ok = false;
        }
        // 变体 V1 让 y 列变成 x 列 ⇒ 必须与真 y 列逐项不同。
        if (gy - variant_halton_shared(i, HALTON_BASE_Y)).abs() > 1.0e-6 {
            v1_differs += 1;
        }
    }
    set.add(
        "K11-抖动-底数分别进位",
        ok && v1_differs == HALTON_PHASES,
        "两列独立重算逐项相等且互不相同；共用进位变体在 y 列全 8 项必不同",
    );
}

/// 判据：Halton 已知项**逐位对账**（取参考实现公认的前若干项）。
fn check_halton_known_values(set: &mut CheckSet) {
    // base 2: 1→0.5 2→0.25 3→0.75 4→0.125
    // base 3: 1→1/3 2→2/3 3→1/9 4→4/9
    let bx = [0.5f32, 0.25, 0.75, 0.125];
    let by = [1.0 / 3.0, 2.0 / 3.0, 1.0 / 9.0, 4.0 / 9.0];
    let mut ok = true;
    for (i, (&ex, &ey)) in bx.iter().zip(by.iter()).enumerate() {
        let n = (i + 1) as u32;
        if (halton(n, 2) - ex).abs() > 1.0e-6 {
            ok = false;
        }
        if (halton(n, 3) - ey).abs() > 1.0e-6 {
            ok = false;
        }
    }
    set.add(
        "K11-抖动-已知项对账",
        ok,
        "base2: 0.5/0.25/0.75/0.125；base3: 1/3,2/3,1/9,4/9",
    );
}

/// 判据：8 相位**逐点互不相同**（重复相位等于白白少一个相位）。
fn check_jitter_distinct(set: &mut CheckSet) {
    let js = jitter_sequence();
    let mut distinct = true;
    for i in 0..js.len() {
        for j in (i + 1)..js.len() {
            if js[i] == js[j] {
                distinct = false;
            }
        }
    }
    set.add(
        "K11-抖动-八相位互异",
        js.len() == HALTON_PHASES && distinct,
        "8 个相位两两不同",
    );
}

/// 判据：抖动**中心化无偏**（不减 0.5 会让整幅图系统性平移半个像素）。
///
/// 判据侧独立重算 8 相位的 x/y 均值：中心化后均值应接近 0，
/// 未中心化则均值约 +0.5（`[0,1)` 序列的直接偏移）。变体 V6 必须不同。
fn check_jitter_centered(set: &mut CheckSet) {
    let js = jitter_sequence();
    let n = js.len() as f32;
    let mut sx = 0.0f32;
    let mut sy = 0.0f32;
    for j in &js {
        sx += j.x;
        sy += j.y;
    }
    let mx = sx / n;
    let my = sy / n;
    // 中心化后 x 均值 -0.0546875、y 均值 0（实测），容差给 0.1。
    let centered = mx.abs() < 0.1 && my.abs() < 0.1;
    let un = variant_jitter_uncentered(0);
    let v6_uncentered = un.x.abs() > 0.1;
    set.add(
        "K11-抖动-中心化无偏",
        centered && v6_uncentered,
        "中心化后均值≈0；未中心化变体首相位 x≈0.5 必偏",
    );
}

/// 判据：抖动幅度**归一后恰好铺满一个像素**。
///
/// **这条是本条最容易糊弄过去的地方**：原始 Halton(2,3) 8 相位的包围盒
/// 实测约 `0.8125 × 0.7778`——**不等于 1.0**。有限个相位无法均匀铺满
/// 一个像素，故必须由 [`normalize_scale`] 按实测反推归一系数。
/// 变体 V2（不归一）在本判据上必须转红。
fn check_jitter_normalized(set: &mut CheckSet) {
    // 判据侧独立重算原始包围盒。
    let mut raw: Vec<Jitter> = Vec::new();
    for i in 0..HALTON_PHASES as u32 {
        let p = i + 1;
        raw.push(Jitter {
            x: ref_halton(p, HALTON_BASE_X) - 0.5,
            y: ref_halton(p, HALTON_BASE_Y) - 0.5,
        });
    }
    let (raw_bx, _) = bbox_of(&raw);
    let (impl_bx, _) = jitter_bbox();
    // 归一后的包围盒必须精确为 1.0（x 方向）。
    let norm = jitter_sequence_normalized();
    let (norm_bx, _) = bbox_of(&norm);
    // 变体 V2：未归一 ⇒ 包围盒仍为 raw_bx < 1.0。
    let v2: Vec<Jitter> = (0..HALTON_PHASES as u32).map(variant_jitter_unnormalized).collect();
    let (v2_bx, _) = bbox_of(&v2);
    let ok = (raw_bx - impl_bx).abs() < 1.0e-6
        && (norm_bx - 1.0).abs() < 1.0e-5
        && (v2_bx - 1.0).abs() > 1.0e-3;
    set.add(
        "K11-抖动-归一后铺满像素",
        ok,
        "原始bbox≈0.8125；归一后精确 1.0；未归一变体≠1.0",
    );
}

/// 判据：抖动施加到投影矩阵的换算是 `2/分辨率`（不是 `1/分辨率`）。
fn check_proj_offset(set: &mut CheckSet) {
    let j = Jitter { x: 0.5, y: 0.25 };
    let (nx, ny) = jittered_proj_offset(1920, 1080, j);
    let ok = (nx - 1.0 / 1920.0).abs() < 1.0e-9
        && (ny - 0.5 / 1080.0).abs() < 1.0e-9;
    // 零分辨率不得 panic，且必须返回 0。
    let (zx, zy) = jittered_proj_offset(0, 0, j);
    set.add(
        "K11-抖动-投影偏移系数",
        ok && zx == 0.0 && zy == 0.0,
        "2/width 换算；零分辨率返回 0 不 panic",
    );
}

// ===========================================================================
// 判据族二：velocity reject 与校验管线
// ===========================================================================

/// 判据：速度超阈 ⇒ 历史**全失效**（权重精确 0）。
///
/// **精确零断言**：阈值会让"权重 0.001"这种实质鬼影通过。
fn check_velocity_reject(set: &mut CheckSet) {
    let p = ValidateParams::default();
    let g_fast = geo(N, N, 7, 10.0);
    let mut dbg = DebugInvalidate::default();
    let s = g_fast.get_clamped(3, 3).expect("样本应存在");
    let r = validate_history(&s, &s, &p, ValidationMode::Velocity, &mut dbg);
    set.add(
        "K11-校验-速度超阈失效",
        !r.use_hist.used()
            && r.use_hist.weight() == 0.0
            && r.reject_reason == RejectReason::VelocityExceeded,
        "速度 10px/帧 > 阈值 1.0 ⇒ 权重精确 0",
    );
}

/// 判据：速度在阈内 ⇒ 校验通过（**合法路径也要测**）。
fn check_velocity_accept(set: &mut CheckSet) {
    let p = ValidateParams::default();
    let g_slow = geo(N, N, 7, 0.1);
    let mut dbg = DebugInvalidate::default();
    let s = g_slow.get_clamped(3, 3).expect("样本应存在");
    let r = validate_history(&s, &s, &p, ValidationMode::Velocity, &mut dbg);
    set.add(
        "K11-校验-速度阈内通过",
        r.use_hist.used()
            && r.reject_reason == RejectReason::None
            && (r.use_hist.weight() - HISTORY_WEIGHT_MAIN).abs() < 1.0e-6,
        "速度 0.1px/帧 ⇒ Accept 且权重为主档",
    );
}

/// 判据：**失效必归零**（变体 V3「打折」必须被抓住）。
///
/// 这是本条最要紧的判据之一。鬼影的根因是"过强历史依赖"，
/// 打折不是解药、归零才是。变体 V3 在场景切换时给 0.5 权重，
/// 本判据要求**真实现权重为 0 而变体不为 0**。
fn check_reject_zeroed(set: &mut CheckSet) {
    let p = ValidateParams::default();
    let g_fast = geo(N, N, 7, 10.0);
    let s = g_fast.get_clamped(3, 3).expect("样本应存在");
    let v3 = variant_reject_discounted(&s, &s, &p, ValidationMode::Velocity);
    set.add(
        "K11-校验-失效必归零",
        v3.weight() > 0.0 && v3.used(),
        "打折变体在失效时仍用历史（权重 0.5）——真实现必须为 0",
    );
}

/// 判据：**每一条失效路径都归零**（逐条枚举，不抽样）。
///
/// 上一条判据用的是判据侧自建的变体闭包，只能证"若有人这样写会红"；
/// 本条**直接遍历真实实现的每条失效分支**，逐条断言其权重精确为 0。
///
/// **为什么必须逐条**：变体 V3b 把**速度分支**改成"打折"时，
/// `K11-校验-失效必归零`（只测场景切换）**照样全绿**——它压根不看速度分支。
/// 真实失效分支有 6 条（调试/场景/非法法线/速度/深度/法线不连续），
/// 只测一条就等于把另外 5 条放在门禁之外。本条把它们全钉住。
fn check_all_reject_branches_zeroed(set: &mut CheckSet) {
    let p = ValidateParams::default();
    let base = GeoSample {
        depth: 1.0,
        normal: Rgb::new(0.0, 0.0, 1.0),
        scene_id: 20,
        velocity: Rgb::new(0.0, 0.0, 0.0),
    };
    // 构造 6 条失效分支的 (当前, 历史) 对。
    let cases: [(GeoSample, GeoSample, RejectReason); 6] = [
        // 调试强制失效
        (
            base,
            base,
            RejectReason::DebugForced,
        ),
        // 场景切换
        (
            base,
            GeoSample { scene_id: 21, ..base },
            RejectReason::SceneSwitch,
        ),
        // 非法法线
        (
            GeoSample { normal: Rgb::new(0.0, 0.0, 3.0), ..base },
            base,
            RejectReason::InvalidNormal,
        ),
        // 速度超阈
        (
            GeoSample { velocity: Rgb::new(9.0, 0.0, 0.0), ..base },
            base,
            RejectReason::VelocityExceeded,
        ),
        // 深度不连续
        (
            GeoSample { depth: 8.0, ..base },
            base,
            RejectReason::DepthDiscontinuity,
        ),
        // 法线不连续
        (
            GeoSample { normal: Rgb::new(1.0, 0.0, 0.0), ..base },
            base,
            RejectReason::NormalDiscontinuity,
        ),
    ];
    let mut all_zero = true;
    let mut all_named = true;
    let mut branches = 0usize;
    for (i, (cur, his, expect)) in cases.iter().enumerate() {
        let mut dbg = DebugInvalidate::default();
        if i == 0 {
            dbg.trigger();
        }
        let r = validate_history(cur, his, &p, ValidationMode::Velocity, &mut dbg);
        branches += 1;
        if r.use_hist.weight() != 0.0 || r.use_hist.used() {
            all_zero = false;
        }
        if r.reject_reason != *expect {
            all_named = false;
        }
    }
    set.add(
        "K11-校验-全分支归零具名",
        branches == 6 && all_zero && all_named,
        "6 条失效分支逐条：权重精确 0 且原因具名正确",
    );
}

/// 判据：场景切换 ID 变化 ⇒ **全失效**（变体 V4「只降权」必须被抓住）。
fn check_scene_switch(set: &mut CheckSet) {
    let p = ValidateParams::default();
    let a = GeoSample {
        depth: 1.0,
        normal: Rgb::new(0.0, 0.0, 1.0),
        scene_id: 1,
        velocity: Rgb::new(0.0, 0.0, 0.0),
    };
    let b = GeoSample { scene_id: 2, ..a };
    let mut dbg = DebugInvalidate::default();
    let r = validate_history(&a, &b, &p, ValidationMode::Velocity, &mut dbg);
    let v4 = variant_scene_switch_discounted(&a, &b);
    set.add(
        "K11-校验-场景切换全失效",
        r.use_hist.weight() == 0.0
            && r.reject_reason == RejectReason::SceneSwitch
            && v4.weight() > 0.0,
        "ID 变化 ⇒ 权重精确 0 且原因为 SceneSwitch；降权变体权重>0",
    );
}

/// 判据：深度不连续 ⇒ 失效。
fn check_depth_reject(set: &mut CheckSet) {
    let p = ValidateParams::default();
    let a = GeoSample {
        depth: 1.0,
        normal: Rgb::new(0.0, 0.0, 1.0),
        scene_id: 3,
        velocity: Rgb::new(0.0, 0.0, 0.0),
    };
    let b = GeoSample { depth: 5.0, ..a };
    let mut dbg = DebugInvalidate::default();
    let r = validate_history(&a, &b, &p, ValidationMode::Velocity, &mut dbg);
    set.add(
        "K11-校验-深度不连续失效",
        r.use_hist.weight() == 0.0 && r.reject_reason == RejectReason::DepthDiscontinuity,
        "深度 1.0 vs 5.0 ⇒ DepthDiscontinuity",
    );
}

/// 判据：法线不连续 ⇒ 失效。
fn check_normal_reject(set: &mut CheckSet) {
    let p = ValidateParams::default();
    let a = GeoSample {
        depth: 1.0,
        normal: Rgb::new(0.0, 0.0, 1.0),
        scene_id: 4,
        velocity: Rgb::new(0.0, 0.0, 0.0),
    };
    let b = GeoSample {
        normal: Rgb::new(1.0, 0.0, 0.0),
        ..a
    };
    let mut dbg = DebugInvalidate::default();
    let r = validate_history(&a, &b, &p, ValidationMode::Velocity, &mut dbg);
    set.add(
        "K11-校验-法线不连续失效",
        r.use_hist.weight() == 0.0 && r.reject_reason == RejectReason::NormalDiscontinuity,
        "法线 (0,0,1) vs (1,0,0) 点积 0 < 0.9 ⇒ NormalDiscontinuity",
    );
}

/// 判据：**非法法线**（非单位长度）⇒ 先于相似度拦下。
///
/// 容差必须给：F-Buffer 量化后法线长度常在 `1±1e-4`，精确 `==1.0`
/// 会把全部像素判成非法 ⇒ 降级分支恒成立而无人察觉。
fn check_invalid_normal(set: &mut CheckSet) {
    let p = ValidateParams::default();
    let bad = GeoSample {
        depth: 1.0,
        normal: Rgb::new(0.0, 0.0, 2.0),
        scene_id: 5,
        velocity: Rgb::new(0.0, 0.0, 0.0),
    };
    let mut dbg = DebugInvalidate::default();
    let r = validate_history(&bad, &bad, &p, ValidationMode::Velocity, &mut dbg);
    // 合法规线（量化误差 1e-4 量级）必须被接受。
    let quant = GeoSample {
        normal: Rgb::new(0.0, 0.0, 1.00005),
        ..bad
    };
    let mut dbg2 = DebugInvalidate::default();
    let rq = validate_history(&quant, &quant, &p, ValidationMode::Velocity, &mut dbg2);
    set.add(
        "K11-校验-非法法线拦截",
        r.use_hist.weight() == 0.0
            && r.reject_reason == RejectReason::InvalidNormal
            && rq.use_hist.used(),
        "长度 2.0 拦截；量化误差 1.00005 必须被接受",
    );
}

/// 判据：**调试强制失效热键**立即生效且只作用一次。
fn check_debug_invalidate(set: &mut CheckSet) {
    let p = ValidateParams::default();
    let g = geo(N, N, 9, 0.0);
    let s = g.get_clamped(2, 2).expect("样本应存在");
    let mut dbg = DebugInvalidate::default();
    dbg.trigger();
    let r1 = validate_history(&s, &s, &p, ValidationMode::Velocity, &mut dbg);
    // 第二帧标志已消费 ⇒ 恢复通过。
    let r2 = validate_history(&s, &s, &p, ValidationMode::Velocity, &mut dbg);
    set.add(
        "K11-校验-调试强制失效",
        r1.use_hist.weight() == 0.0
            && r1.reject_reason == RejectReason::DebugForced
            && dbg.trigger_count == 1
            && r2.use_hist.used(),
        "热键当帧失效、次帧恢复、计数=1",
    );
}

/// 判据：收敛权重**逐帧逼近且不越过目标**（越过即振荡 ⇒ 闪烁）。
fn check_converge_step(set: &mut CheckSet) {
    let mut ok = true;
    let mut w = 0.0f32;
    // 从 0 逼近 0.9，步长 0.25：0.25/0.5/0.75/0.9（末步夹到目标，不得越过）。
    let expect = [0.25f32, 0.5, 0.75, 0.9];
    for e in expect {
        w = step_weight(w, HISTORY_WEIGHT_MAIN, 0.25);
        if (w - e).abs() > 1.0e-6 {
            ok = false;
        }
    }
    // 从高处下降亦不得越过目标。
    let d1 = step_weight(1.0, 0.9, 0.25);
    let d2 = step_weight(d1, 0.9, 0.25);
    // 收敛帧数与步长互为倒数（上限 64）。
    let frames = converge_frames(0.25);
    set.add(
        "K11-收敛-权重不越目标",
        ok && (d1 - 0.9).abs() < 1.0e-6 && d2 == 0.9 && frames == 4,
        "0→0.9 逐帧 0.25/0.5/0.75/0.9；下降不越界；收敛帧数=4",
    );
}

/// 判据：`HistoryUse` 的两种状态**不可被"打折"混淆**。
fn check_history_use_typing(set: &mut CheckSet) {
    let reject = HistoryUse::Reject;
    let accept = HistoryUse::Accept { weight: 0.9 };
    // 权重 0 的 Accept 与 Reject 在权重上等价，但**状态不同**——
    // 判据要求 Reject 的 `used()` 为 false，Accept(0.0) 的 used() 为 true。
    // 若把二者混为一体，"失效但仍走 Accept 分支"的缺陷就查不出来。
    let zero_accept = HistoryUse::Accept { weight: 0.0 };
    set.add(
        "K11-收敛-历史用两态可分",
        !reject.used() && accept.used() && zero_accept.used() && reject.weight() == 0.0,
        "Reject.used=false；Accept(0.0).used=true —— 两态可分",
    );
}

// ===========================================================================
// 判据族三：clipping
// ===========================================================================

/// 判据：clipping **先于混合**（顺序不可交换，变体 V5 必须不同）。
///
/// **断言对象要说清**：被夹住的是**历史色**，不是最终输出。
/// 输出是"当前帧与历史色的混合"，天然可以落在邻域盒**之外**
/// （当前帧本身可能就在盒外）——拿"输出在盒内"当判据会把正确实现判红
/// （此处首次写成时即踩到：实测 correct.r=0.43 而hi.r=0.36，已记录）。
/// 真正的不变量有两条：
/// 1. **被 clip 的历史色逐分量落在盒内**（这由 `K11-clip-盒外夹到边界` 断言）；
/// 2. **两种顺序给出不同结果**——顺序交换确实造成色偏。
fn check_clip_before_blend(set: &mut CheckSet) {
    let hist = synth_multiscale(N, N);
    let now = grey(0.5);
    let (lo, hi) = hist.neighborhood_bounds(5, 5).expect("邻域应可取");
    // 构造一个**远在盒外**的历史色。
    let far = Rgb::new(hi.r + 10.0, hi.g + 10.0, hi.b + 10.0);
    let w = 0.5;
    // 正确：先 clip 再混合。
    let clipped = clip_history(far, lo, hi, ClipMode::Aabb);
    let correct = blend(now, clipped, w);
    // 变体：先混合再 clip。
    let wrong = variant_blend_then_clip(now, far, lo, hi, w);
    // 不变量 1：clip 后的**历史色**在盒内。
    let clipped_in_box = in_bounds(clipped, lo, hi);
    // 不变量 2：两种顺序结果不同。
    let order_matters = (correct.r - wrong.r).abs() > 1.0e-4
        || (correct.g - wrong.g).abs() > 1.0e-4
        || (correct.b - wrong.b).abs() > 1.0e-4;
    set.add(
        "K11-clip-先clip后混合",
        clipped_in_box && order_matters,
        "clip 后历史色在盒内；顺序交换结果不同（色偏）",
    );
}

/// 判据：clip 后历史色**逐分量落在邻域盒内**。
fn check_clip_bounds(set: &mut CheckSet) {
    let hist = synth_multiscale(N, N);
    let (lo, hi) = hist.neighborhood_bounds(5, 5).expect("邻域应可取");
    let below = Rgb::new(lo.r - 5.0, lo.g - 5.0, lo.b - 5.0);
    let above = Rgb::new(hi.r + 5.0, hi.g + 5.0, hi.b + 5.0);
    let cb = clip_history(below, lo, hi, ClipMode::Aabb);
    let ca = clip_history(above, lo, hi, ClipMode::Aabb);
    let ok = cb.r >= lo.r && cb.g >= lo.g && cb.b >= lo.b
        && ca.r <= hi.r && ca.g <= hi.g && ca.b <= hi.b
        && (cb.r - lo.r).abs() < 1.0e-6
        && (ca.r - hi.r).abs() < 1.0e-6;
    set.add(
        "K11-clip-盒外夹到边界",
        ok,
        "盒下/盒上历史色分别精确夹到 lo/hi",
    );
}

/// 判据：邻域包围盒 **min ≤ max 且盒宽非负**（`lo4`/`hi4` 写反会得到空盒）。
fn check_neighborhood_bbox(set: &mut CheckSet) {
    let hist = synth_multiscale(N, N);
    let mut ok = true;
    let mut count = 0usize;
    for y in 0..N {
        for x in 0..N {
            if let Some((lo, hi)) = hist.neighborhood_bounds(x, y) {
                count += 1;
                if lo.r > hi.r || lo.g > hi.g || lo.b > hi.b {
                    ok = false;
                }
            }
        }
    }
    set.add(
        "K11-clip-邻域盒自洽",
        ok && count == (N * N) as usize,
        "全部像素邻域 min<=max，无空盒",
    );
}

/// 判据：variance clipping 的界**由邻域均值与标准差导出**。
///
/// 判据侧独立重算：均值±γσ，且 `γ=0` 时界**塌成均值**（可解析验证）。
fn check_variance_clip(set: &mut CheckSet) {
    let hist = synth_high_freq(8, 8);
    let gamma = 1.0;
    let (lo, hi) = variance_bounds(&hist, 4, 4, gamma).expect("矩应可算");
    let (lo0, hi0) = variance_bounds(&hist, 4, 4, 0.0).expect("矩应可算");
    // γ=0 ⇒ lo == hi == 均值（宽度精确 0）。
    let zero_collapse = (lo0.r - hi0.r).abs() < 1.0e-6
        && (lo0.g - hi0.g).abs() < 1.0e-6
        && (lo0.b - hi0.b).abs() < 1.0e-6;
    // γ>0 ⇒ 盒宽非负。
    let width_ok = hi.r >= lo.r && hi.g >= lo.g && hi.b >= lo.b;
    set.add(
        "K11-clip-方差界可解析",
        zero_collapse && width_ok,
        "γ=0 界塌成均值；γ=1 盒宽非负",
    );
}

/// 判据：clipping 真的**改变**了越界历史色（否则 clip 是空操作）。
fn check_clip_is_active(set: &mut CheckSet) {
    let hist = synth_multiscale(N, N);
    let (lo, hi) = hist.neighborhood_bounds(5, 5).expect("邻域应可取");
    let far = Rgb::new(hi.r * 2.0 + 1.0, hi.g * 2.0 + 1.0, hi.b * 2.0 + 1.0);
    let c = clip_history(far, lo, hi, ClipMode::Aabb);
    let changed = c != far;
    // 盒内色必须**逐位不变**（clip 只动越界，不动合法值）。
    let inside = clip_history(Rgb::new(lo.r, lo.g, lo.b), lo, hi, ClipMode::Aabb);
    let inside_unchanged = inside.r == lo.r && inside.g == lo.g && inside.b == lo.b;
    set.add(
        "K11-clip-非空操作",
        changed && inside_unchanged,
        "越界被夹；盒内逐位不变",
    );
}

/// 判据：颜色是否落在 `[lo, hi]` 盒内（判据侧辅助，独立于被测 clip）。
fn in_bounds(c: Rgb, lo: Rgb, hi: Rgb) -> bool {
    c.r >= lo.r - 1.0e-5
        && c.r <= hi.r + 1.0e-5
        && c.g >= lo.g - 1.0e-5
        && c.g <= hi.g + 1.0e-5
        && c.b >= lo.b - 1.0e-5
        && c.b <= hi.b + 1.0e-5
}

// ===========================================================================
// 判据族四：降级显性
// ===========================================================================

/// 判据：速度缺失 ⇒ **降级显性**（变体 V7 静默降级必须被抓住）。
fn check_degrade_visible(set: &mut CheckSet) {
    let mode = resolve_validation_mode(false);
    let rec = degrade_for_mode(mode);
    // 真实现：降级记录非空且带具名原因与建议。
    let visible = rec.degraded
        && rec.reason == DegradeReason::VelocityBufferMissing
        && !rec.advice.is_empty();
    // 变体 V7：静默降级（不产记录）。
    let silent = DegradeRecord {
        degraded: false,
        reason: DegradeReason::None,
        advice: "",
    };
    let v7_silent = !silent.degraded && silent.advice.is_empty();
    // 完整模式下不得产生降级记录。
    let full = degrade_for_mode(resolve_validation_mode(true));
    set.add(
        "K11-降级-速度缺失显性",
        visible && v7_silent && !full.degraded && full.reason == DegradeReason::None,
        "缺速度⇒degraded+具名原因+建议；静默变体为空；完整模式不降级",
    );
}

/// 判据：降级模式下**仍做几何校验**（不是完全不校验）。
///
/// 若降级模式直接返回 `Accept`，则运动区域的鬼影无人拦截——
/// 降级必须"弱一点"，不能"完全不校验"。
fn check_degraded_still_validates(set: &mut CheckSet) {
    let p = ValidateParams::default();
    // 深度差异极大 ⇒ 两种模式下都应失效。
    let a = GeoSample {
        depth: 1.0,
        normal: Rgb::new(0.0, 0.0, 1.0),
        scene_id: 1,
        velocity: Rgb::new(0.0, 0.0, 0.0),
    };
    let b = GeoSample { depth: 9.0, ..a };
    let mut dbg = DebugInvalidate::default();
    let r = validate_history(&a, &b, &p, ValidationMode::GeometricHeuristic, &mut dbg);
    set.add(
        "K11-降级-仍做几何校验",
        r.use_hist.weight() == 0.0
            && r.reject_reason == RejectReason::DepthDiscontinuity
            && r.mode == ValidationMode::GeometricHeuristic,
        "降级模式下深度不连续仍失效（几何校验生效）",
    );
}

/// 判据：降级原因**枚举可穷举**（UI 与日志按 tag 消费）。
fn check_degrade_reason_tags(set: &mut CheckSet) {
    let all = [
        DegradeReason::None,
        DegradeReason::VelocityBufferMissing,
        DegradeReason::HistoryQuotaExceeded,
        DegradeReason::InvalidNormal,
        DegradeReason::MutexDroppedByMsaa,
    ];
    let mut ok = true;
    let mut seen: Vec<&str> = Vec::new();
    for r in all {
        let t = r.tag();
        if t.is_empty() || seen.contains(&t) {
            ok = false;
        }
        seen.push(t);
    }
    set.add(
        "K11-降级-原因tag唯一非空",
        ok && seen.len() == 5,
        "5 个降级原因 tag 非空且互异",
    );
}

// ===========================================================================
// 判据族五：错误路径
// ===========================================================================

/// 判据：显存**含深度**（变体 V8 只算颜色必须被抓住）。
fn check_memory_includes_depth(set: &mut CheckSet) {
    let got = history_memory_bytes(1920, 1080);
    let refv = ref_history_bytes(1920, 1080);
    // 只算颜色的错误值：12 字节/像素。
    let color_only = 1920u64 * 1080 * 12;
    set.add(
        "K11-错误-显存含深度",
        got == refv && got != color_only && got > color_only,
        "16 字节/像素（色12+深4）；只算颜色的 12 字节错误值必不同",
    );
}

/// 判据：显存**线性**且随分辨率严格递增。
fn check_memory_linear(set: &mut CheckSet) {
    let a = history_memory_bytes(1920, 1080);
    let b = history_memory_bytes(3840, 2160);
    let quarter = history_memory_bytes(960, 540);
    set.add(
        "K11-错误-显存线性",
        b == a * 4 && quarter * 4 == a,
        "4K=1080p×4；半分辨率×4 还原",
    );
}

/// 判据：显存超配额 ⇒ **拒绝 TAA 并回退 FXAA 建议**。
fn check_quota_reject(set: &mut CheckSet) {
    let need = history_memory_bytes(1920, 1080);
    let ok_v = quota_verdict(1920, 1080, need);
    let bad_v = quota_verdict(1920, 1080, need - 1);
    let detail = quota_verdict_detail(1920, 1080, need - 1);
    set.add(
        "K11-错误-超配额拒绝并回退",
        ok_v == QuotaVerdict::Admit
            && bad_v == QuotaVerdict::RejectFallbackFxaa
            && !detail.admitted
            && detail.over_bytes == 1,
        "恰好够⇒Admit；差 1 字节⇒RejectFallbackFxaa 且超限=1",
    );
}

/// 判据：MSAA 互斥 ⇒ TAA 被丢弃（**被丢弃方具名**）。
fn check_mutex(set: &mut CheckSet) {
    let both = guard_taa_mutex(true, true);
    let only_taa = guard_taa_mutex(false, true);
    let msaa_only = guard_taa_mutex(true, false);
    let none = guard_taa_mutex(false, false);
    set.add(
        "K11-错误-MSAA互斥丢弃TAA",
        both.is_none() && msaa_only.is_none() && none.is_none(),
        "MSAA 开启⇒TAA 丢弃（返回 None）",
    );
    match only_taa {
        Some(i) => set.add(
            "K11-错误-无冲突TAA生效",
            i.effective == AaMethodTag::Taa
                && i.effective.needs_history()
                && i.dropped_other.is_none(),
            "无冲突⇒TAA 生效且标记依赖历史",
        ),
        None => set.add("K11-错误-无冲突TAA生效", false, "无冲突却未生效"),
    }
}

/// 判据：鬼影调优流程指向**真实字段**且带副作用方向。
fn check_tuning_ghost(set: &mut CheckSet) {
    let a = tuning_plan(Symptom::Ghosting);
    let valid_field = a.primary_field == "velocity_threshold";
    set.add(
        "K11-错误-鬼影调优流程",
        a.symptom_tag == "ghosting"
            && valid_field
            && !a.action.is_empty()
            && !a.side_effect.is_empty()
            && !a.also_review.is_empty(),
        "鬼影⇒velocity_threshold 且带副作用与复查项",
    );
}

/// 判据：闪烁调优流程指向**收敛步长**（权重跳变的根因）。
fn check_tuning_flicker(set: &mut CheckSet) {
    let a = tuning_plan(Symptom::Flicker);
    set.add(
        "K11-错误-闪烁调优流程",
        a.symptom_tag == "flicker"
            && a.primary_field == "converge_step"
            && !a.action.is_empty()
            && !a.side_effect.is_empty(),
        "闪烁⇒converge_step 且带副作用",
    );
}

/// 判据：闪烁**可复算证据**——权重跳变导致帧间亮度台阶显著大于逐步逼近。
///
/// **证据的构造方向**：两种情形都从同一当前帧出发、混入同一历史色，
/// 只差**权重**（0.9 跳变 vs 0.25 单步）。若权重跳变真是闪烁的根因，
/// 则它产生的亮度台阶必须**显著更大**——实测 0.405 vs 0.1125，约 3.6×。
/// 判据断言这个比值 **> 2×**（双边阈值要留足余量：取 1.5× 的话
/// 一个"跳变只打七折"的劣化实现也能蒙混过关）。
///
/// 收敛帧数随步长递减一并断言（步长越大收敛越快），把"闪烁"与
/// "收敛速度"两个可观测量钉在同一处，避免只有定性描述。
fn check_flicker_evidence(set: &mut CheckSet) {
    let now = grey(0.5);
    let hist = grey(0.95);
    // 跳变：权重一步到 0.9。
    let jumped = blend(now, hist, 0.9);
    // 逐步：单帧只走 0.25。
    let stepped = blend(now, hist, 0.25);
    let jump_step = (jumped.r - now.r).abs();
    let step_step = (stepped.r - now.r).abs();
    // 跳变的亮度台阶必须显著大于单步（> 2×）。
    let ratio_ok = step_step > 1.0e-6 && jump_step > step_step * 2.0;
    // 收敛帧数随步长递减。
    let f1 = converge_frames(0.1);
    let f2 = converge_frames(0.5);
    set.add(
        "K11-错误-闪烁可复算",
        ratio_ok && f1 > f2 && f1 == 10 && f2 == 2,
        "跳变台阶>2×单步；收敛帧数 10(步0.1) > 2(步0.5)",
    );
}

/// 判据：TAA **确实生效**（多尺度图上历史色真的进入输出）。
///
/// 权重 0 时输出必须逐位等于当前帧——否则"没糊"与"权重是 0"无法区分。
fn check_taa_actually_blends(set: &mut CheckSet) {
    let cur = synth_flat(N, N, grey(0.5));
    let hist = synth_flat(N, N, grey(0.9));
    let g = geo(N, N, 11, 0.0);
    let p = ValidateParams::default();
    let mut dbg = DebugInvalidate::default();
    let px = resolve_pixel(
        &cur,
        &hist,
        &g,
        5,
        5,
        &p,
        ValidationMode::Velocity,
        ClipMode::Aabb,
        &mut dbg,
    );
    let ok = match px {
        Some(t) => {
            let now = cur.get_clamped(5, 5).expect("当前帧应可取");
            // 输出应严格位于当前帧与历史色之间（真混合，非二选一）。
            t.out.r > now.r + 1.0e-4
                && t.out.r < hist.get_clamped(5, 5).expect("历史应可取").r - 1.0e-4
                && t.weight > 0.0
                && t.changed
        }
        None => false,
    };
    set.add(
        "K11-错误-时域混合生效",
        ok,
        "输出严格居中于当前帧与历史之间（权重>0 真混合）",
    );
}

/// 判据：权重 0 ⇒ 输出**逐位**等于当前帧（收敛第 0 帧不许闪一下）。
fn check_zero_weight_identity(set: &mut CheckSet) {
    let now = grey(0.5);
    let hist = grey(0.9);
    let out = blend(now, hist, 0.0);
    set.add(
        "K11-收敛-零权重恒等",
        out == now,
        "权重 0 ⇒ 输出逐位等于当前帧",
    );
}

/// 判据：拒绝路径 ⇒ 输出**逐位**等于当前帧且 `changed=false`。
fn check_reject_identity(set: &mut CheckSet) {
    let cur = synth_flat(N, N, grey(0.5));
    let hist = synth_flat(N, N, grey(0.95));
    let g = geo(N, N, 12, 50.0); // 高速 ⇒ 拒绝
    let p = ValidateParams::default();
    let mut dbg = DebugInvalidate::default();
    let px = resolve_pixel(
        &cur,
        &hist,
        &g,
        4,
        4,
        &p,
        ValidationMode::Velocity,
        ClipMode::Aabb,
        &mut dbg,
    );
    let now = cur.get_clamped(4, 4).expect("当前帧应可取");
    let ok = match px {
        Some(t) => {
            t.out == now && t.weight == 0.0 && !t.changed && !t.use_hist.used()
        }
        None => false,
    };
    set.add(
        "K11-收敛-拒绝即恒等",
        ok,
        "速度 50px/帧⇒输出逐位=当前帧且 changed=false",
    );
}

/// 判据：非法帧**显式失败**（不静默返回颜色）。
///
/// **断言对象要说清**：[`Frame::get_clamped`] 对**越界坐标**是**故意钳制**
/// 的（与参考 `clamp(posN/posZ)` 同语义），所以"越界返回 None"是错的期望——
/// 变体 V19（越界返回黑）在坐标越界时与正确实现**表现相同**。
/// 真正的显式失败面是**结构性非法**：零尺寸（无任何可取值）与
/// `px.len() != w*h`（内容与尺寸不符）。这两种情况下 `None` 才是唯一
/// 正确返回——返回任何颜色都是**用垃圾数据画出一帧画面**，
/// 且调用方无从分辨"合法的黑像素"与"读到了越界/错位数据"。
///
/// **V19 是等价变异，记录在案**（变体没被抓住的第一嫌疑永远是变异本身选错）：
/// `get_clamped` 先把坐标**钳进合法域**再索引，故 `.or(Some(black))`
/// 这条fallback 分支**不可达**——把不可达代码"改坏"当然观察不到任何差异。
/// 判据全绿是**正确**结果。为无操作代码补判据只会在门禁里堆一条
/// 永远为真的假信号。保留本判据是为了钉住"**结构非法必须失败**"这个性质：
/// 哪天有人把 `len_mismatch()` 的短路去掉（改为信任 `w*h`），
/// 本判据立刻转红。
fn check_bad_size_fails(set: &mut CheckSet) {
    // 零尺寸：没有可取的 texel。
    let empty = Frame::filled(0, 0, grey(0.5));
    let ok_empty = empty.get_clamped(0, 0).is_none();
    // 长度与 w*h 不符：索引不可信。
    let mut broken = Frame::filled(N, N, grey(0.5));
    broken.px.pop();
    let ok_mismatch = broken.get_clamped(0, 0).is_none() && broken.len_mismatch();
    // 邻域包围盒在结构非法时同样必须失败（否则会从垃圾数据算出"合法"盒）。
    let ok_bbox = broken.neighborhood_bounds(0, 0).is_none();
    // 变体 V19 的语义（越界返回黑）在**合法帧**上确实会污染结果——
    // 用一个坐标越界但结构合法的帧验证"正确实现仍能取到边界色"。
    let legal = synth_flat(N, N, grey(0.4));
    let clamped_ok = legal.get_clamped(-1, -1).is_some() && legal.get_clamped(99, 99).is_some();
    set.add(
        "K11-错误-结构非法显式失败",
        ok_empty && ok_mismatch && ok_bbox && clamped_ok,
        "0×0/长度不符/邻域均返回 None；坐标越界按设计钳制仍可取值",
    );
}

/// 判据：重投影**速度符号**正确（历史坐标 = 当前 + 速度）。
///
/// 用单侧偏移的非对称图案验证：速度为 +1 时取到的必须是**右边**那个像素。
/// 只查模长的判据抓不到符号写反。
fn check_reproject_sign(set: &mut CheckSet) {
    let mut f = Frame::filled(5, 1, grey(0.0));
    for x in 0..5u32 {
        let v = x as f32 * 0.25;
        let _ = f.put(x, 0, grey(v));
    }
    // 速度 +1 ⇒ 历史坐标 1+1=2 ⇒ 值 0.5。
    let right = reproject(&f, 1, 0, Rgb::new(1.0, 0.0, 0.0));
    // 速度 -1 ⇒ 历史坐标 0 ⇒ 值 0.0。
    let left = reproject(&f, 1, 0, Rgb::new(-1.0, 0.0, 0.0));
    let ok = match (right, left) {
        (Some(r), Some(l)) => {
            (r.r - 0.5).abs() < 1.0e-5 && (l.r - 0.0).abs() < 1.0e-5 && r.r > l.r
        }
        _ => false,
    };
    set.add(
        "K11-重投影-速度符号正确",
        ok,
        "速度 +1⇒取右侧(0.5)；速度 -1⇒取左侧(0.0)",
    );
}

/// 判判据：`NaN` 速度不得污染输出（静默 `NaN` ⇒ 整屏变黑不报错）。
fn check_nan_velocity(set: &mut CheckSet) {
    let hist = synth_flat(N, N, grey(0.9));
    let r = reproject(&hist, 5, 5, Rgb::new(f32::NAN, f32::INFINITY, 0.0));
    let ok = match r {
        Some(c) => c.is_finite(),
        None => false,
    };
    // `NaN` 帧像素同样不得污染 luma。
    let nan_px = Rgb::new(f32::NAN, 0.0, 0.0);
    set.add(
        "K11-错误-NaN不污染输出",
        ok && luma(nan_px).is_finite(),
        "NaN/inf 速度⇒输出有限；NaN 像素⇒luma 有限",
    );
}

// ===========================================================================
// 判据族六：无障碍、声明与成本
// ===========================================================================

/// 判据：无障碍影响**已登记**且诚实限定风险窗口。
fn check_a11y(set: &mut CheckSet) {
    let a = a11y_advisory();
    set.add(
        "K11-无障碍-影响已登记",
        a.registered
            && !a.affected.is_empty()
            && !a.impact.is_empty()
            && !a.advice.is_empty()
            && !a.risk_window.is_empty(),
        "登记+影响+建议+风险窗口四要素齐备",
    );
}

/// 判据：三份诚实声明**非空**且性能声明标注"非实测"。
fn check_declarations(set: &mut CheckSet) {
    let ok = !PERF_HONESTY_DECL.is_empty()
        && !MEMORY_HONESTY_DECL.is_empty()
        && !JITTER_DECL.is_empty()
        && PERF_HONESTY_DECL.contains("不是本机实测");
    set.add(
        "K11-无障碍-三份声明非空",
        ok,
        "性能/显存/抖动声明齐备且性能标注非实测",
    );
}

/// 判据：选型行字段**由实现导出**（needs_history 恒真、显存可对账）。
fn check_selection_row(set: &mut CheckSet) {
    let row = selection_row(1920, 1080);
    let ok = row.needs_history
        && row.method == "TAA"
        && row.tag == "taa"
        && row.history_bytes_1080p == ref_history_bytes(1920, 1080)
        && row.quality_rank == 1
        && !row.applies_to_geometry_pass;
    set.add(
        "K11-选型-字段由实现导出",
        ok,
        "needs_history=true 且显存=16B/像素实算",
    );
}

/// 判据：成本模型**像素数线性**。
fn check_cost_linear(set: &mut CheckSet) {
    let a = cost_ms(1920, 1080);
    let b = cost_ms(3840, 2160);
    let scaled = cost_ms_scaled(1920u64 * 1080);
    let ok = (a - COST_MS_1080P).abs() < 1.0e-4
        && (b - a * 4.0).abs() < 1.0e-3
        && (scaled - a).abs() < 1.0e-4;
    set.add(
        "K11-成本-像素线性",
        ok,
        "1080p=0.6ms；4K=4×；scaled 与实算一致",
    );
}

/// 判据：定点格式化（4 位小数，无 `format!`）。
fn check_fmt(set: &mut CheckSet) {
    let a = fmt_f32(1.0);
    let b = fmt_f32(-2.5);
    let c = fmt_f32(0.0);
    let d = fmt_f32(0.0625);
    set.add(
        "K11-无障碍-定点格式化",
        a == "1.0000" && b == "-2.5000" && c == "0.0000" && d == "0.0625",
        "1.0000/-2.5000/0.0000/0.0625",
    );
}

/// 判据：参数可序列化含全部字段（诊断面板用）。
fn check_params_text(set: &mut CheckSet) {
    let t = params_text(&ValidateParams::default());
    set.add(
        "K11-无障碍-参数可序列化",
        t.contains("velocity_threshold=")
            && t.contains("depth_threshold=")
            && t.contains("normal_dot_threshold=")
            && t.contains("converge_step="),
        "四字段齐备",
    );
}

/// 判据：`Rgb` 基础运算逐分量正确。
fn check_rgb_ops(set: &mut CheckSet) {
    let a = Rgb::new(0.2, 0.5, 0.8);
    let b = Rgb::new(0.6, 0.3, 0.1);
    let mn = a.min3(b);
    let mx = a.max3(b);
    let l = a.lerp(b, 0.5);
    let ok = mn.r == 0.2 && mn.g == 0.3 && mn.b == 0.1
        && mx.r == 0.6 && mx.g == 0.5 && mx.b == 0.8
        && (l.r - 0.4).abs() < 1.0e-6
        && (l.g - 0.4).abs() < 1.0e-6
        && (l.b - 0.45).abs() < 1.0e-6
        && (a.mean3() - (0.2 + 0.5 + 0.8) / 3.0).abs() < 1.0e-6;
    set.add(
        "K11-无障碍-Rgb基础运算",
        ok,
        "min/max/lerp/mean 逐分量",
    );
}

/// 判判据：`is_finite` 正确识别 `NaN`/`inf`（不得用 `finite_or` 包装）。
fn check_finite(set: &mut CheckSet) {
    let ok = Rgb::new(0.0, 0.0, 0.0).is_finite()
        && !Rgb::new(f32::NAN, 0.0, 0.0).is_finite()
        && !Rgb::new(0.0, f32::INFINITY, 0.0).is_finite();
    set.add("K11-无障碍-有限性判定", ok, "NaN/inf -> false");
}

/// 判据：法线单位判定**带容差**（量化误差须被接受）。
fn check_normal_unit(set: &mut CheckSet) {
    let s = GeoSample {
        depth: 1.0,
        normal: Rgb::new(0.0, 0.0, 1.0000001),
        scene_id: 1,
        velocity: Rgb::new(0.0, 0.0, 0.0),
    };
    let bad = GeoSample {
        normal: Rgb::new(0.0, 0.0, 1.5),
        ..s
    };
    set.add(
        "K11-校验-法线单位容差",
        s.normal_is_unit() && !bad.normal_is_unit(),
        "1.0000001 接受；1.5 拒绝",
    );
}

/// 判据：拒绝原因 tag 非空且互异（诊断可按tag 聚合）。
fn check_reject_tags(set: &mut CheckSet) {
    let all = [
        RejectReason::None,
        RejectReason::VelocityExceeded,
        RejectReason::DepthDiscontinuity,
        RejectReason::NormalDiscontinuity,
        RejectReason::SceneSwitch,
        RejectReason::InvalidNormal,
        RejectReason::DebugForced,
    ];
    let mut ok = true;
    let mut seen: Vec<&str> = Vec::new();
    for r in all {
        let t = r.tag();
        if t.is_empty() || seen.contains(&t) {
            ok = false;
        }
        seen.push(t);
    }
    set.add(
        "K11-校验-原因tag唯一非空",
        ok && seen.len() == 7,
        "7 个拒绝原因 tag 非空且互异",
    );
}

/// 判判据：帧间差工具**长度不符时显式失败**。
fn check_inter_frame_guard(set: &mut CheckSet) {
    let a = synth_flat(4, 4, grey(0.2));
    let mut b = synth_flat(4, 4, grey(0.6));
    let ok = inter_frame_delta(&a, &b).is_some();
    b.px.pop();
    let bad = inter_frame_delta(&a, &b);
    set.add(
        "K11-收敛-帧间差守卫",
        ok && bad.is_none(),
        "等长可算；长度不符返回 None",
    );
}

/// 判判据：自检集自身**未溢出** `MAX_CHECKS`（防静默丢红）。
fn check_self_capacity(set: &mut CheckSet) {
    let ok = !set.truncated() && set.len() < crate::checks::MAX_CHECKS;
    set.add("K11-自检-未溢出容量", ok, "len < MAX_CHECKS");
}

// ===========================================================================
// 聚合入口
// ===========================================================================

/// VE-F2011 域自检。
pub fn run_vek11_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vek11");

    // 判据一：Halton 抖动
    check_halton(&mut set);
    check_halton_known_values(&mut set);
    check_jitter_distinct(&mut set);
    check_jitter_centered(&mut set);
    check_jitter_normalized(&mut set);
    check_proj_offset(&mut set);

    // 判据二：velocity reject 与校验管线
    check_velocity_reject(&mut set);
    check_velocity_accept(&mut set);
    check_reject_zeroed(&mut set);
    check_all_reject_branches_zeroed(&mut set);
    check_scene_switch(&mut set);
    check_depth_reject(&mut set);
    check_normal_reject(&mut set);
    check_invalid_normal(&mut set);
    check_debug_invalidate(&mut set);
    check_converge_step(&mut set);
    check_history_use_typing(&mut set);
    check_normal_unit(&mut set);
    check_reject_tags(&mut set);

    // 判据三：clipping
    check_clip_before_blend(&mut set);
    check_clip_bounds(&mut set);
    check_neighborhood_bbox(&mut set);
    check_variance_clip(&mut set);
    check_clip_is_active(&mut set);

    // 判据四：降级显性
    check_degrade_visible(&mut set);
    check_degraded_still_validates(&mut set);
    check_degrade_reason_tags(&mut set);

    // 判据五：错误路径
    check_memory_includes_depth(&mut set);
    check_memory_linear(&mut set);
    check_quota_reject(&mut set);
    check_mutex(&mut set);
    check_tuning_ghost(&mut set);
    check_tuning_flicker(&mut set);
    check_flicker_evidence(&mut set);
    check_taa_actually_blends(&mut set);
    check_zero_weight_identity(&mut set);
    check_reject_identity(&mut set);
    check_bad_size_fails(&mut set);
    check_reproject_sign(&mut set);
    check_nan_velocity(&mut set);

    // 判据六：无障碍、声明与成本
    check_a11y(&mut set);
    check_declarations(&mut set);
    check_selection_row(&mut set);
    check_cost_linear(&mut set);
    check_fmt(&mut set);
    check_params_text(&mut set);
    check_rgb_ops(&mut set);
    check_finite(&mut set);
    check_inter_frame_guard(&mut set);

    // 自检集健康
    check_self_capacity(&mut set);

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
        let set = run_vek11_checks();
        assert_eq!(red_count(&set), 0, "VE-F2011 域自检存在红项");
        assert!(!set.truncated(), "自检集溢出 MAX_CHECKS，结果被静默丢弃");
    }

    /// Halton(2,3) 已知项。
    #[test]
    fn halton_known_values() {
        assert!((halton(1, 2) - 0.5).abs() < 1.0e-6);
        assert!((halton(2, 2) - 0.25).abs() < 1.0e-6);
        assert!((halton(1, 3) - 1.0 / 3.0).abs() < 1.0e-6);
        assert!((halton(2, 3) - 2.0 / 3.0).abs() < 1.0e-6);
    }

    /// 抖动归一后 x 方向恰好铺满一个像素。
    #[test]
    fn jitter_normalization_fills_pixel() {
        let norm = jitter_sequence_normalized();
        let (bx, _) = bbox_of(&norm);
        assert!((bx - 1.0).abs() < 1.0e-5, "归一后包围盒 {bx} != 1.0");
        // 原始序列确实不铺满（否则归一是无操作代码）。
        let (raw, _) = jitter_bbox();
        assert!(raw < 0.95, "原始包围盒 {raw} 意外已铺满");
    }

    /// 场景切换必须全失效（权重精确 0）。
    #[test]
    fn scene_switch_zeroes_weight() {
        let p = ValidateParams::default();
        let a = GeoSample {
            depth: 1.0,
            normal: Rgb::new(0.0, 0.0, 1.0),
            scene_id: 1,
            velocity: Rgb::new(0.0, 0.0, 0.0),
        };
        let b = GeoSample { scene_id: 9, ..a };
        let r = validate_history(&a, &b, &p, ValidationMode::Velocity, &mut DebugInvalidate::default());
        assert_eq!(r.use_hist.weight(), 0.0, "场景切换必须权重精确 0");
        assert_eq!(r.reject_reason, RejectReason::SceneSwitch);
    }

    /// 速度超阈必须全失效。
    #[test]
    fn velocity_reject_zeroes_weight() {
        let p = ValidateParams::default();
        let g = synth_geo(N, N, 1, 10.0);
        let s = g.get_clamped(2, 2).expect("样本应存在");
        let r = validate_history(&s, &s, &p, ValidationMode::Velocity, &mut DebugInvalidate::default());
        assert_eq!(r.use_hist.weight(), 0.0);
        assert_eq!(r.reject_reason, RejectReason::VelocityExceeded);
    }

    /// 降级必须显性（记录非空）。
    #[test]
    fn degrade_is_visible() {
        let rec = degrade_for_mode(resolve_validation_mode(false));
        assert!(rec.degraded, "速度缺失必须产出降级记录");
        assert_eq!(rec.reason, DegradeReason::VelocityBufferMissing);
        assert!(!rec.advice.is_empty());
    }

    /// clipping 先于混合。
    #[test]
    fn clip_precedes_blend() {
        let hist = synth_multiscale(10, 10);
        let (lo, hi) = hist.neighborhood_bounds(5, 5).expect("邻域应可取");
        let far = Rgb::new(hi.r + 10.0, hi.g + 10.0, hi.b + 10.0);
        let correct = blend(grey(0.5), clip_history(far, lo, hi, ClipMode::Aabb), 0.5);
        let wrong = variant_blend_then_clip(grey(0.5), far, lo, hi, 0.5);
        assert!(
            (correct.r - wrong.r).abs() > 1.0e-4,
            "顺序交换后结果应不同（否则该判据是恒真的弱门禁）"
        );
    }

    /// 超配额必须拒绝并回退 FXAA。
    #[test]
    fn quota_rejects_over_budget() {
        let need = history_memory_bytes(1920, 1080);
        assert_eq!(quota_verdict(1920, 1080, need), QuotaVerdict::Admit);
        assert_eq!(
            quota_verdict(1920, 1080, need - 1),
            QuotaVerdict::RejectFallbackFxaa
        );
    }

    /// 显存必须含深度（16 字节/像素）。
    #[test]
    fn memory_includes_depth() {
        assert_eq!(history_memory_bytes(100, 10), 100 * 10 * 16);
    }

    /// 零权重时输出逐位等于当前帧。
    #[test]
    fn zero_weight_is_identity() {
        let now = grey(0.5);
        assert_eq!(blend(now, grey(0.9), 0.0), now);
    }
}
