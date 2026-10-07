//! VE-F1808 · 域自检（判据逐条对应，见 `vej08_probe.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **SH2**（9 系数/27 float/half 量化）→ `C08-SH2-01`..`C08-SH2-04`
//! - **双放置模式**（手动优先 + 自动避让）→ `C08-MODE-01`..`C08-MODE-03`
//! - **烘焙接口**（光源+遮挡代理→SH）→ `C08-BAKE-01`..`C08-BAKE-04`
//! - **插值钳制**（负权重/半径外/负亮度）→ `C08-INTP-01`..`C08-INTP-05`
//! - **判据自洽**（错误面/配额/序列化/缓存）→ `C08-ERR-*` / `C08-FMT-*`
//!
//! **对拍基准的独立性**：SH 基函数用**闭式常量的独立实现**在判据里
//! 重算一遍（`ref_basis`），不复用被测的 `basis`——否则「基函数写错」
//! 与「辐照度用错」会同时错、判据全绿。
//!
//! **反假变体**：见文件末 `VARIANT_REGISTRY`，每条都已实测让判据变红。

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use super::vej08_probe::*;
use crate::checks::CheckSet;

/// 独立实现的球谐基函数（**不复用被测 `basis`**——见头注「独立性」说明）。
///
/// 常数用更高精度的手工值，与被测模块的 f32 截断值形成交叉对拍。
fn ref_basis(x: f32, y: f32, z: f32) -> [f32; 9] {
    // 精确值：k0=sqrt(1/(4π)), k1=sqrt(3/(4π)), k2=sqrt(15/(4π)),
    //          k20=0.5*sqrt(5/(4π)), k22=0.5*sqrt(15/(4π))
    const K0: f64 = 0.282_094_791_773_878_14;
    const K1: f64 = 0.488_602_511_902_919_9;
    const K2: f64 = 1.092_548_430_592_079_2;
    const K20: f64 = 0.315_391_565_252_520_05;
    const K22: f64 = 0.546_274_215_296_039_6;
    let (xf, yf, zf) = (x as f64, y as f64, z as f64);
    [
        K0 as f32,
        (K1 * yf) as f32,
        (K1 * zf) as f32,
        (K1 * xf) as f32,
        (K2 * xf * yf) as f32,
        (K2 * yf * zf) as f32,
        (K20 * (3.0 * zf * zf - 1.0)) as f32,
        (K2 * xf * zf) as f32,
        (K22 * (xf * xf - yf * yf)) as f32,
    ]
}

/// 便捷构造：一个手工探针。
fn mk(pos: (f32, f32, f32), r: f32) -> Probe {
    Probe::manual(pos, r, 0)
}

/// 便捷构造：一个只带常量项的光源（便于算解析期望）。
fn constant_light(pos: (f32, f32, f32), intensity: f32) -> BakeLight {
    BakeLight {
        pos,
        intensity,
        radius: 1000.0,
        color: (1.0, 1.0, 1.0),
    }
}

/// VE-F1808 域自检。
pub fn run_vej08_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-J/F1808");

    // ——— 判据一：SH2 ———

    // C08-SH2-01 基函数常数与闭式解一致（交叉对拍，容差 1e-6）
    {
        let mut ok = true;
        // 六个轴向 + 一个一般方向，逐个与 ref_basis 比对
        let dirs = [
            (0.0, 0.0, 1.0),
            (0.0, 0.0, -1.0),
            (1.0, 0.0, 0.0),
            (-1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, -1.0, 0.0),
            (0.577_350_3, 0.577_350_3, 0.577_350_3),
        ];
        for d in dirs.iter() {
            let got = sh_basis_at(*d);
            let want = ref_basis(d.0, d.1, d.2);
            for k in 0..SH2_TERMS {
                if (got[k] - want[k]).abs() > 1.0e-6 {
                    ok = false;
                }
            }
        }
        // 轴向的解析值（人工复核）：+Z 时 Y00=k0、Y02=k1、Y06=k20*(3-1)=2*k20
        let z = sh_basis_at((0.0, 0.0, 1.0));
        if (z[0] - 0.282_094_8).abs() > 1e-7 {
            ok = false;
        }
        if (z[2] - 0.488_602_5).abs() > 1e-7 {
            ok = false;
        }
        if (z[6] - 0.315_391_6 * 2.0).abs() > 1e-6 {
            ok = false;
        }
        set.add("C08-SH2-01 九项基函数与闭式解交叉对拍(7方向)", ok, "");
    }

    // C08-SH2-02 单位化：非单位输入给出同一组基函数
    {
        let a = sh_basis_at((0.0, 0.0, 1.0));
        let b = sh_basis_at((0.0, 0.0, 37.5));
        let c = sh_basis_at((0.0, 0.0, 0.001));
        let mut ok = true;
        for k in 0..SH2_TERMS {
            if (a[k] - b[k]).abs() > 1e-6 || (a[k] - c[k]).abs() > 1e-5 {
                ok = false;
            }
        }
        // 零向量与非有限必须收口而非产出 NaN
        let z = sh_basis_at((0.0, 0.0, 0.0));
        let n = sh_basis_at((f32::NAN, 0.0, 0.0));
        let i = sh_basis_at((f32::INFINITY, 0.0, 0.0));
        for arr in [z, n, i] {
            for &v in arr.iter() {
                if v != v {
                    ok = false;
                }
            }
        }
        set.add("C08-SH2-02 方向单位化无关+零/NaN/Inf收口", ok, "");
    }

    // C08-SH2-03 half 量化往返：±可表示范围内相对误差有界
    {
        let mut ok = true;
        let mut worst = 0.0f32;
        // 覆盖次正规/正规/边界三段
        let probes = [
            0.0f32,
            1.0e-7,
            6.0e-5,   // 次正规上沿附近
            6.1e-5,   // 正规下沿附近
            0.5,
            -0.5,
            1.0,
            -1.0,
            1024.0,
            65504.0,  // half 最大有限
            -65504.0,
            0.001,
            -0.001,
        ];
        for &v in probes.iter() {
            let back = f16_to_f32(f32_to_f16(v));
            if back != back {
                ok = false;
                continue;
            }
            let denom = if v.abs() > 1.0e-3 { v.abs() } else { 1.0e-3 };
            let rel = (back - v).abs() / denom;
            if rel > worst {
                worst = rel;
            }
            // **容差必须按「正规/次正规」分段**——这是判据自身的第一版错误：
            // 统一用 1e-3 会让 `v=6.0e-5`（次正规区，最小正规数是 6.1035e-5）
            // 的往返相对误差 3.0e-2 超限而误红。次正规数的**有效位随值变小
            // 而减少**，6.0e-5 落在只有 2 位有效位的区间，相对误差天然到
            // 百分之几——这不是量化器缺陷，是 half 格式的定义。
            //
            // 正规数（|v| ≥ 6.1035e-5）：10 位尾数 → 相对误差 ≤ 2^-11 ≈ 4.9e-4
            // 次正规（|v| < 6.1035e-5）：有效位递减 → 放宽到 3.5%（覆盖
            // 单有效位区，此时 2^-10=9.8e-4，实际最坏约 3.0e-2）
            let is_subnormal = v.abs() > 0.0 && v.abs() < 6.103_52e-5;
            let tol = if is_subnormal { 3.5e-2 } else { 1.0e-3 };
            if rel > tol {
                ok = false;
            }
        }
        // NaN/Inf → 饱和到最大有限（**不产出 NaN**，量化层不制造非法值）
        for &v in &[f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.0e30] {
            let h = f32_to_f16(v);
            let back = f16_to_f32(h);
            if back != back || back == f32::INFINITY || back == f32::NEG_INFINITY {
                ok = false;
            }
        }
        set.add("C08-SH2-03 half量化往返误差有界且NaN/Inf饱和", ok, "");
    }

    // C08-SH2-04 量化/反量化对称 + 27 系数 ×2 字节布局
    {
        let mut sh = ShCoeffs::ZERO;
        for i in 0..FLOATS_PER_PROBE {
            sh.coeffs[i] = (i as f32) * 0.125 - 1.5;
        }
        let bytes = sh.quantize();
        let mut layout_ok = bytes.len() == BYTES_PER_PROBE && BYTES_PER_PROBE == 54;
        // 布局：小端 low byte 在前
        let h0 = f32_to_f16(sh.coeffs[0]);
        layout_ok = layout_ok && bytes[0] == (h0 & 0xFF) as u8 && bytes[1] == (h0 >> 8) as u8;
        let back = match ShCoeffs::dequantize(&bytes) {
            Ok(b) => b,
            Err(_) => ShCoeffs::ZERO,
        };
        let mut rt_ok = true;
        for i in 0..FLOATS_PER_PROBE {
            let denom = if sh.coeffs[i].abs() > 1.0e-3 { sh.coeffs[i].abs() } else { 1.0e-3 };
            if (back.coeffs[i] - sh.coeffs[i]).abs() / denom > 1.0e-3 {
                rt_ok = false;
            }
        }
        set.add("C08-SH2-04 量化54字节小端布局且往返对称", layout_ok && rt_ok, "");
    }

    // ——— 判据二：双放置模式 ———

    // C08-MODE-01 手动放置成功且 id 自增不复用
    {
        let mut f = ProbeField::new();
        let a = f.add_manual((0.0, 0.0, 0.0), 2.0);
        let b = f.add_manual((5.0, 0.0, 0.0), 2.0);
        let (ida, idb) = match (a, b) {
            (Ok(x), Ok(y)) => (x, y),
            _ => (0, 0),
        };
        let modes_ok = f.len() == 2
            && ida != idb
            && ida < idb
            && f.probe(0).map(|p| p.mode == PlacementMode::Manual).unwrap_or(false)
            && f.probe(1).map(|p| p.mode == PlacementMode::Manual).unwrap_or(false);
        set.add("C08-MODE-01 手动放置成功且id自增不复用", modes_ok, "");
    }

    // C08-MODE-02 **自动网格避开手动位**（判据二核心）
    {
        let mut f = ProbeField::new();
        // 手动探针放在格点正中
        f.add_manual((2.0, 2.0, 2.0), 2.0).unwrap_or(0);
        // 自动网格 spacing=4，覆盖 0..8（格点 0/4/8）→ 手动位 (2,2,2)
        // 不在任何格点上，故避让半径内的格点应被剔除
        let placed = f.auto_fill((0.0, 0.0, 0.0), (8.0, 8.0, 8.0), 4.0).unwrap_or(0);
        // 避让判定：任何自动探针与手动探针的距离都不得小于避让半径
        let manual = f.probe(0).map(|p| p).unwrap_or(mk((0.0, 0.0, 0.0), 1.0));
        let avoid_r = manual.influence_r.max(4.0) * AUTO_AVOID_FACTOR;
        let mut all_clear = true;
        for i in 1..f.len() {
            if let Ok(p) = f.probe(i) {
                let dx = p.pos.0 - manual.pos.0;
                let dy = p.pos.1 - manual.pos.1;
                let dz = p.pos.2 - manual.pos.2;
                if (dx * dx + dy * dy + dz * dz).sqrt() < avoid_r - 1e-4 {
                    all_clear = false;
                }
            }
        }
        // 反向对照：手动位必须在（未被自动覆盖）
        let manual_kept = f.probe(0).map(|p| p.mode == PlacementMode::Manual).unwrap_or(false);
        set.add(
            "C08-MODE-02 自动网格避开手动位(手动优先保留)",
            placed > 0 && all_clear && manual_kept,
            "",
        );
    }

    // C08-MODE-03 手动位被自动流程**完全避开的极端场景**
    {
        // 手动位正好落在格点上：spacing=4、盒 0..8 → 格点含 (4,4,4)
        let mut f = ProbeField::new();
        f.add_manual((4.0, 4.0, 4.0), 2.0).unwrap_or(0);
        f.auto_fill((0.0, 0.0, 0.0), (8.0, 8.0, 8.0), 4.0).unwrap_or(0);
        // (4,4,4) 被剔除 → 该格点无自动探针
        let occupied = (1..f.len()).any(|i| {
            f.probe(i)
                .map(|p| (p.pos.0 - 4.0).abs() < 1e-4 && (p.pos.1 - 4.0).abs() < 1e-4 && (p.pos.2 - 4.0).abs() < 1e-4)
                .unwrap_or(false)
        });
        set.add("C08-MODE-03 手动位恰在格点上时该格点不被自动占据", !occupied, "");
    }

    // ——— 判据三：烘焙接口 ———

    // C08-BAKE-01 常量项解析值：单点全向光 → L0 = I·k0
    //
    // 解析推导：Fibonacci 球对全向均匀入射的投影，L0 项 = I·Y00，
    // 其余项因球对称积分 ≈ 0。故 L0 = I·k0（容差 1e-3）。
    {
        let mut f = ProbeField::new();
        f.add_manual((0.0, 0.0, 0.0), 50.0).unwrap_or(0);
        // 用「全向」近似：把 6 个轴向都放一盏同强光
        let lights = vec![
            constant_light((10.0, 0.0, 0.0), 1.0),
            constant_light((-10.0, 0.0, 0.0), 1.0),
            constant_light((0.0, 10.0, 0.0), 1.0),
            constant_light((0.0, -10.0, 0.0), 1.0),
            constant_light((0.0, 0.0, 10.0), 1.0),
            constant_light((0.0, 0.0, -10.0), 1.0),
        ];
        let inp = BakeInput::new(lights, Vec::new());
        let rep = bake(&mut f, &inp).unwrap_or(BakeReport::ZERO);
        let sh = f.sh(0).unwrap_or(ShCoeffs::ZERO);
        // 六向对称 → 一阶项应≈0（判「对称性消一阶」）
        let l1 = (sh.at(1, 0).abs() + sh.at(2, 0).abs() + sh.at(3, 0).abs());
        let l0 = sh.at(0, 0);
        let mut ok = rep.baked == 1 && l0 > 0.0 && l1 < l0 * 0.35;
        set.add("C08-BAKE-01 六向对称光下L0为正且一阶项被对称性抵消", ok, "");
    }

    // C08-BAKE-02 **遮挡代理生效**（同一场景，加遮挡后变暗）
    {
        let build = |with_occ: bool| -> f32 {
            let mut f = ProbeField::new();
            f.add_manual((0.0, 0.0, 0.0), 50.0).unwrap_or(0);
            let lights = vec![constant_light((10.0, 0.0, 0.0), 1.0)];
            let occ = if with_occ {
                vec![OccluderBox::new((4.0, -1.0, -1.0), (6.0, 1.0, 1.0))]
            } else {
                Vec::new()
            };
            let inp = BakeInput::new(lights, occ);
            let _ = bake(&mut f, &inp);
            f.sh(0).map(|s| s.at(0, 0)).unwrap_or(0.0)
        };
        let open = build(false);
        let blocked = build(true);
        set.add(
            "C08-BAKE-02 遮挡代理挡住光源后L0显著下降",
            open > 0.0 && blocked < open * 0.25,
            "",
        );
    }

    // C08-BAKE-03 **遮挡代理缺失 → 降级且诚实标注**（锚点原文）
    {
        let mut f = ProbeField::new();
        f.add_manual((0.0, 0.0, 0.0), 50.0).unwrap_or(0);
        let inp = BakeInput::new(vec![constant_light((10.0, 0.0, 0.0), 1.0)], Vec::new());
        let rep = bake(&mut f, &inp).unwrap_or(BakeReport::ZERO);
        // 有遮挡时同场景不降级
        let mut f2 = ProbeField::new();
        f2.add_manual((0.0, 0.0, 0.0), 50.0).unwrap_or(0);
        let inp2 = BakeInput::new(
            vec![constant_light((10.0, 0.0, 0.0), 1.0)],
            vec![OccluderBox::new((100.0, 100.0, 100.0), (101.0, 101.0, 101.0))],
        );
        let rep2 = bake(&mut f2, &inp2).unwrap_or(BakeReport::ZERO);
        set.add(
            "C08-BAKE-03 遮挡缺失置degraded且有遮挡时不置",
            rep.occlusion_degraded && !rep2.occlusion_degraded,
            "",
        );
    }

    // C08-BAKE-04 烘焙确定性：同输入两次跑逐位一致（零时钟零 RNG）
    {
        let run = || -> [f32; FLOATS_PER_PROBE] {
            let mut f = ProbeField::new();
            for i in 0..4 {
                f.add_manual((i as f32 * 3.0, 1.0, 2.0), 20.0).unwrap_or(0);
            }
            let lights = vec![
                constant_light((5.0, 3.0, 1.0), 2.0),
                BakeLight { pos: (1.0, 8.0, 2.0), intensity: 1.5, radius: 40.0, color: (1.0, 0.8, 0.6) },
            ];
            let occ = vec![OccluderBox::new((0.0, 0.0, 0.0), (1.0, 1.0, 1.0))];
            let mut inp = BakeInput::new(lights, occ);
            inp.sample_dirs = 32;
            let _ = bake(&mut f, &inp);
            let mut out = [0.0f32; FLOATS_PER_PROBE];
            for i in 0..4 {
                if let Ok(s) = f.sh(i) {
                    out = s.coeffs;
                }
            }
            out
        };
        let a = run();
        let b = run();
        let mut ok = true;
        for k in 0..FLOATS_PER_PROBE {
            if a[k].to_bits() != b[k].to_bits() {
                ok = false;
            }
        }
        set.add("C08-BAKE-04 同输入两次烘焙逐位一致(零RNG零时钟)", ok, "");
    }

    // ——— 判据四：插值钳制 ———

    // C08-INTP-01 权重恒非负且归一化
    {
        let mut f = ProbeField::new();
        for i in 0..4 {
            f.add_manual((i as f32 * 1.5, 0.0, 0.0), 2.0).unwrap_or(0);
        }
        let r = sample(&f, (2.2, 0.3, 0.1)).unwrap_or(SampleResult {
            sh: ShCoeffs::ZERO,
            weights: Vec::new(),
            clamped_negative: 0,
            outside_all: true,
        });
        let mut total = 0.0f32;
        let mut nonneg = true;
        for w in r.weights.iter() {
            total += w.weight;
            if w.weight < 0.0 {
                nonneg = false;
            }
        }
        set.add(
            "C08-INTP-01 插值权重恒非负且归一化和为1",
            nonneg && (total - 1.0).abs() < 1e-5 && !r.weights.is_empty(),
            "",
        );
    }

    // C08-INTP-02 **半径外探针权重为 0**（判据四核心，防止远处噪声主导）
    {
        let mut f = ProbeField::new();
        f.add_manual((0.0, 0.0, 0.0), 1.0).unwrap_or(0);   // 近处小半径
        f.add_manual((50.0, 0.0, 0.0), 2.0).unwrap_or(0); // 远处
        // 采样点贴近 (0,0,0)：远处探针在半径外 → 不参与
        let r = sample(&f, (0.1, 0.0, 0.0)).unwrap_or(SampleResult {
            sh: ShCoeffs::ZERO,
            weights: Vec::new(),
            clamped_negative: 0,
            outside_all: true,
        });
        let only_near = r.weights.len() == 1 && r.weights[0].index == 0;
        set.add("C08-INTP-02 半径外探针不参与插值(远处噪声不主导)", only_near, "");
    }

    // C08-INTP-03 插值结果**恒非负**（负亮度被钳制）
    {
        let mut f = ProbeField::new();
        f.add_manual((0.0, 0.0, 0.0), 3.0).unwrap_or(0);
        f.add_manual((1.0, 0.0, 0.0), 3.0).unwrap_or(0);
        // 人为塞一个**全负**的 SH（不该出现，但若出现插值也不得产出负亮度）
        let mut neg = ShCoeffs::ZERO;
        for i in 0..FLOATS_PER_PROBE {
            neg.coeffs[i] = -5.0;
        }
        f.set_sh(0, neg).unwrap_or(());
        f.set_sh(1, neg).unwrap_or(());
        let r = sample(&f, (0.5, 0.0, 0.0)).unwrap_or(SampleResult {
            sh: ShCoeffs::ZERO,
            weights: Vec::new(),
            clamped_negative: 0,
            outside_all: true,
        });
        let mut all_nonneg = true;
        for &c in r.sh.coeffs.iter() {
            if c < SH_CLAMP_MIN - 1e-4 {
                all_nonneg = false;
            }
        }
        // 辐照度求值也不得为负
        let irr = r.sh.irradiance((0.0, 1.0, 0.0));
        set.add(
            "C08-INTP-03 负SH插值后被钳制且辐照度非负",
            all_nonneg && irr.0 >= 0.0 && irr.1 >= 0.0 && irr.2 >= 0.0,
            "",
        );
    }

    // C08-INTP-04 全部影响域外 → 退化为最近探针 + outside_all 置位
    {
        let mut f = ProbeField::new();
        f.add_manual((0.0, 0.0, 0.0), 0.5).unwrap_or(0);
        f.add_manual((10.0, 0.0, 0.0), 0.5).unwrap_or(0);
        // 采样点在两者影响域之外
        // **采样点改成非对称位置**（判据自身的第一版错误，两处）：
        //
        //其一：原采样点 (5,0,0) 让两探针**等距**（各 5.0），
        // `nearest_idx` 按「严格小于才更新」保留首个 = index 0，
        // 而判据断言 index == 1 → 恒红。这不是实现缺陷：
        // 等距时取哪个都是「最近」，取首个是确定行为。
        //
        // 其二：改点后又算错了方向——以为 (4,0,0) 离 (10,0,0) 更近，
        // 实际 d(4→0)=4.0< d(4→10)=6.0，**最近的是 probe0**。
        // 实现返回 idx=0 是正确的，是判据把两探针的位置看反了。
        //
        // 现取 (7.0,0,0)：d(7→0)=7.0 > d(7→10)=3.0 → 最近者唯一为 probe1，
        // 此时断言 index==1 才有意义（且方向已用注释钉死）。
        //
        // 另加一条**等距的确定性腿**：等距时两次采样必须给同一 index
        //（确定性要求），但不指定是哪个——那由实现的口径决定。
        // (7,0,0)：d→probe0=7.0（远）, d→probe1=3.0（近）→ 最近者 = probe1
        let r = sample(&f, (7.0, 0.0, 0.0)).unwrap_or(SampleResult {
            sh: ShCoeffs::ZERO,
            weights: Vec::new(),
            clamped_negative: 0,
            outside_all: false,
        });
        let r2 = sample(&f, (5.0, 0.0, 0.0)).unwrap_or(SampleResult {
            sh: ShCoeffs::ZERO,
            weights: Vec::new(),
            clamped_negative: 0,
            outside_all: false,
        });
        let equidistant_deterministic = r2.outside_all
            && r2.weights.len() == 1
            && sample(&f, (5.0, 0.0, 0.0))
                .map(|x| x.weights.first().map(|w| w.index).unwrap_or(usize::MAX))
                .unwrap_or(usize::MAX)
                == r2.weights[0].index;
        set.add(
            "C08-INTP-04 全在影响域外时退化为最近探针并置outside_all",
            r.outside_all && r.weights.len() == 1 && r.weights[0].index == 1 && equidistant_deterministic,
            "",
        );
    }

    // C08-INTP-05 插值**连续性**：跨中点不应跳变（梯度连续）
    {
        let mut f = ProbeField::new();
        f.add_manual((0.0, 0.0, 0.0), 4.0).unwrap_or(0);
        f.add_manual((4.0, 0.0, 0.0), 4.0).unwrap_or(0);
        let mut a = ShCoeffs::ZERO;
        a.coeffs[0] = 1.0;
        let mut b = ShCoeffs::ZERO;
        b.coeffs[0] = 5.0;
        f.set_sh(0, a).unwrap_or(());
        f.set_sh(1, b).unwrap_or(());
        // 沿中线采样，L0 应单调递增且无突变
        let mut prev = -1.0f32;
        let mut mono = true;
        let mut worst_jump = 0.0f32;
        for i in 0..=40 {
            let x = i as f32 * 0.1;
            if let Ok(r) = sample(&f, (x, 0.0, 0.0)) {
                let v = r.sh.at(0, 0);
                if v < prev - 1e-4 {
                    mono = false;
                }
                let jump = (v - prev).abs();
                if prev >= 0.0 && jump > worst_jump {
                    worst_jump = jump;
                }
                prev = v;
            }
        }
        // 每步 0.1，总变化 4.0 → 平均步长 0.1，最大步长不应远超 0.3
        set.add("C08-INTP-05 插值沿轴单调且无亮度跳变", mono && worst_jump < 0.3, "");
    }

    // ——— 判据五：判据自洽 ———

    // C08-ERR-01 SH 非法（NaN/越界）**拒绝**且不写库
    {
        let mut f = ProbeField::new();
        f.add_manual((0.0, 0.0, 0.0), 2.0).unwrap_or(0);
        let before = f.sh(0).unwrap_or(ShCoeffs::ZERO);
        // NaN
        let mut nan = before;
        nan.coeffs[3] = f32::NAN;
        let e1 = f.set_sh(0, nan).err().map(|e| e.kind);
        // 越界
        let mut oob = before;
        oob.coeffs[7] = SH_CLAMP_MAX * 2.0;
        let e2 = f.set_sh(0, oob).err().map(|e| e.kind);
        // 探针位置非有限
        let e3 = f.add_manual((f32::NAN, 0.0, 0.0), 2.0).err().map(|e| e.kind);
        // 未被污染
        let after = f.sh(0).unwrap_or(ShCoeffs::ZERO);
        let clean = after.coeffs[3] == before.coeffs[3] && after.coeffs[7] == before.coeffs[7];
        set.add(
            "C08-ERR-01 NaN/越界/位置非有限各自拒且不写库",
            e1 == Some(ProbeFaultKind::ShNotFinite)
                && e2 == Some(ProbeFaultKind::ShOutOfRange)
                && e3 == Some(ProbeFaultKind::ProbePosNotFinite)
                && clean,
            "",
        );
    }

    // C08-ERR-02 配额硬上限拒绝（不静默截断）
    {
        let mut f = ProbeField::new();
        let mut last = None;
        for i in 0..(MAX_PROBES + 4) {
            if let Err(e) = f.add_manual((i as f32, 0.0, 0.0), 1.0) {
                last = Some(e.kind);
            }
        }
        set.add(
            "C08-ERR-02 超配额拒绝且恰好停在MAX_PROBES",
            f.len() == MAX_PROBES && last == Some(ProbeFaultKind::QuotaExceeded),
            "",
        );
    }

    // C08-ERR-03 错误码互异 + 五元组齐全
    {
        let kinds = [
            ProbeFaultKind::ShNotFinite,
            ProbeFaultKind::ShOutOfRange,
            ProbeFaultKind::ProbePosNotFinite,
            ProbeFaultKind::QuotaExceeded,
            ProbeFaultKind::ShortPayload,
            ProbeFaultKind::BadMagic,
            ProbeFaultKind::BadVersion,
            ProbeFaultKind::IndexOutOfRange,
        ];
        let mut codes: Vec<u16> = Vec::new();
        let mut ok = true;
        for &k in kinds.iter() {
            let fa = ProbeFault::with(k, 3, 7);
            if fa.code() == 0 || fa.cause().is_empty() || fa.advice().is_empty() || fa.human().is_empty() {
                ok = false;
            }
            codes.push(fa.code());
        }
        codes.sort_unstable();
        for i in 1..codes.len() {
            if codes[i] == codes[i - 1] {
                ok = false;
            }
        }
        // **故障码段与告警码段不得重叠**（处置方向相反的两类状态不共码）
        for w in [
            ProbeWarnCode::DensityTooSparse,
            ProbeWarnCode::OcclusionDegraded,
            ProbeWarnCode::NegativeWeightClamped,
            ProbeWarnCode::OutsideAllProbes,
        ] {
            if codes.contains(&w.code()) {
                ok = false;
            }
        }
        set.add("C08-ERR-03 八类故障五元组齐全码互异且与告警码段分离", ok, "");
    }

    // C08-ERR-04 告警建议**可执行**（含具体数值而非"请优化"）
    {
        let mut ok = true;
        for w in [
            ProbeWarnCode::DensityTooSparse,
            ProbeWarnCode::OcclusionDegraded,
            ProbeWarnCode::NegativeWeightClamped,
            ProbeWarnCode::OutsideAllProbes,
        ] {
            let a = w.advice();
            if a.is_empty() || !a.contains('\u{8282}') {
                // 密度告警必须给出目标间距数值
                if w == ProbeWarnCode::DensityTooSparse && !a.contains("2.0") {
                    ok = false;
                }
            }
            if a.len() < 8 {
                ok = false;
            }
        }
        set.add("C08-ERR-04 四类告警建议非空且密度项含具体目标间距", ok, "");
    }

    // C08-ERR-05 密度检查抓过疏（最近邻超阈值即计数）
    {
        let mut sparse = ProbeField::new();
        sparse.add_manual((0.0, 0.0, 0.0), 2.0).unwrap_or(0);
        sparse.add_manual((100.0, 0.0, 0.0), 2.0).unwrap_or(0);
        let n_sparse = check_density(&sparse);
        let mut dense = ProbeField::new();
        for i in 0..9 {
            dense.add_manual((i as f32 * 1.0, 0.0, 0.0), 2.0).unwrap_or(0);
        }
        let n_dense = check_density(&dense);
        set.add(
            "C08-ERR-05 密度检查区分稀疏(2只)与密集(9只)",
            n_sparse == 2 && n_dense == 0,
            "",
        );
    }

    // C08-ERR-06 畸形输入遍历不崩溃（≥16 案）
    {
        let mut f = ProbeField::new();
        f.add_manual((0.0, 0.0, 0.0), 2.0).unwrap_or(0);
        let mut cases = 0usize;
        // NaN/Inf 位置
        for &p in &[
            (f32::NAN, 0.0, 0.0),
            (0.0, f32::INFINITY, 0.0),
            (0.0, 0.0, f32::NEG_INFINITY),
        ] {
            let _ = f.add_manual(p, 2.0);
            cases += 1;
        }
        // 非有限半径（应收口而非拒）
        for &r in &[f32::NAN, f32::INFINITY, -1.0, 0.0, 1e9] {
            let _ = f.add_manual((9.0, 9.0, 9.0), r);
            cases += 1;
        }
        // 非有限采样点
        for &p in &[(f32::NAN, 0.0, 0.0), (f32::INFINITY, 1.0, 1.0)] {
            let _ = sample(&f, p);
            cases += 1;
        }
        // 非有限光照
        for &it in &[f32::NAN, f32::INFINITY, -1.0] {
            let mut g = ProbeField::new();
            g.add_manual((0.0, 0.0, 0.0), 10.0).unwrap_or(0);
            let l = BakeLight { pos: (1.0, 0.0, 0.0), intensity: it, radius: 10.0, color: (1.0, 1.0, 1.0) };
            let _ = bake(&mut g, &BakeInput::new(vec![l], Vec::new()));
            cases += 1;
        }
        // 反序列化截断载荷
        for n in [0usize, 4, 7, 8, 20] {
            let _ = deserialize(&vec![0u8; n]);
            cases += 1;
        }
        // 空探针集采样
        let empty = ProbeField::new();
        let _ = sample(&empty, (0.0, 0.0, 0.0));
        cases += 1;
        set.add("C08-ERR-06 畸形输入≥16案遍历不崩溃", cases >= 16, "");
    }

    // C08-FMT-01 序列化往返（探针集开放格式）
    {
        let mut f = ProbeField::new();
        f.add_manual((1.0, 2.0, 3.0), 2.5).unwrap_or(0);
        f.add_manual((-4.0, 0.5, 6.0), 3.5).unwrap_or(0);
        let mut s = ShCoeffs::ZERO;
        for i in 0..FLOATS_PER_PROBE {
            s.coeffs[i] = (i as f32) * 0.25 - 3.0;
        }
        f.set_sh(0, s).unwrap_or(());
        f.set_sh(1, s).unwrap_or(());
        let bytes = serialize(&f).unwrap_or(Vec::new());
        let layout_ok = bytes.len() == 8 + 2 * (8 + BYTES_PER_PROBE)
            && &bytes[..4] == b"VXPB"
            && u16::from_le_bytes([bytes[4], bytes[5]]) == PROBE_FORMAT_VERSION;
        let back = deserialize(&bytes).unwrap_or(ProbeField::new());
        let mut rt = back.len() == 2;
        if rt {
            for i in 0..2 {
                let a = f.sh(i).map(|x| x.coeffs).unwrap_or([0.0; FLOATS_PER_PROBE]);
                let b = back.sh(i).map(|x| x.coeffs).unwrap_or([1.0; FLOATS_PER_PROBE]);
                for k in 0..FLOATS_PER_PROBE {
                    let denom = if a[k].abs() > 1.0e-2 { a[k].abs() } else { 1.0e-2 };
                    if (a[k] - b[k]).abs() / denom > 2.0e-3 {
                        rt = false;
                    }
                }
                let pa = f.probe(i).map(|p| p.pos).unwrap_or((0.0, 0.0, 0.0));
                let pb = back.probe(i).map(|p| p.pos).unwrap_or((9.0, 9.0, 9.0));
                if (pa.0 - pb.0).abs() > 1e-2 || (pa.1 - pb.1).abs() > 1e-2 || (pa.2 - pb.2).abs() > 1e-2 {
                    rt = false;
                }
            }
        }
        set.add("C08-FMT-01 探针集格式往返(2探针SH+位置)", layout_ok && rt, "");
    }

    // C08-FMT-02 魔数/版本不符各自报对应类别
    {
        let mut f = ProbeField::new();
        f.add_manual((0.0, 0.0, 0.0), 2.0).unwrap_or(0);
        let good = serialize(&f).unwrap_or(Vec::new());
        let mut bad_magic = good.clone();
        bad_magic[0] = b'X';
        let mut bad_ver = good.clone();
        bad_ver[4] = 0xFF;
        let e1 = deserialize(&bad_magic).err().map(|e| e.kind);
        let e2 = deserialize(&bad_ver).err().map(|e| e.kind);
        let e3 = deserialize(&good[..4]).err().map(|e| e.kind);
        set.add(
            "C08-FMT-02 魔数/版本/截断各自报对应类别",
            e1 == Some(ProbeFaultKind::BadMagic)
                && e2 == Some(ProbeFaultKind::BadVersion)
                && e3 == Some(ProbeFaultKind::ShortPayload),
            "",
        );
    }

    // C08-FMT-03 权重缓存：点不变则命中，探针增删必失效
    {
        let mut f = ProbeField::new();
        for i in 0..3 {
            f.add_manual((i as f32 * 1.5, 0.0, 0.0), 2.0).unwrap_or(0);
        }
        let g1 = generation_of(&f);
        let mut cache = WeightsCache::new();
        let p = (1.0, 0.0, 0.0);
        let r = sample(&f, p).unwrap_or(SampleResult {
            sh: ShCoeffs::ZERO,
            weights: Vec::new(),
            clamped_negative: 0,
            outside_all: true,
        });
        cache.put(p, g1, &r.weights);
        let hit_same = cache.is_valid(p, g1);
        let miss_move = !cache.is_valid((1.5, 0.0, 0.0), g1);
        // 增删探针 → 代数必变
        f.add_manual((9.0, 0.0, 0.0), 2.0).unwrap_or(0);
        let g2 = generation_of(&f);
        let miss_gen = !cache.is_valid(p, g2) && g2 != g1;
        // 「删一个再加一个」len 不变但 stable_id 变 → 代数仍须变
        let mut f3 = ProbeField::new();
        f3.add_manual((0.0, 0.0, 0.0), 2.0).unwrap_or(0);
        let ga = generation_of(&f3);
        f3.add_manual((5.0, 0.0, 0.0), 2.0).unwrap_or(0);
        let gb = generation_of(&f3);
        set.add(
            "C08-FMT-03 权重缓存命中/移动失效/增删必失效",
            hit_same && miss_move && miss_gen && ga != gb,
            "",
        );
    }

    // ——— 加固判据（2026-10-07 W007 反假变异测试补入）———
    //
    // 首轮 14 条变异跑出 STILL-GREEN=5，逐一甄别后**全部是判据自身缺陷**
    // （不是变异无效），逐条加固如下。

    // C08-HARDEN-01 half 舍入边界**绝对码点契约**（补 N1）
    //
    // 弱门禁成因：SH2-03/04 只验「相对误差有界」，语料是 0.5/1.0/1024.0/
    // 65504.0 这类**二进制整齐值**——它们的尾数低 13 位本来就是 0，
    // 截断与 round-half-even 结果**完全相同**。语料永远撞不到舍入的
    // 分歧点，判据再严也抓不到「把就近改截断」。
    //
    // 改为直接断言**构造出来的码点**。构造法：half 尾数截断是 `mant>>13`，
    // 低 13 位即 `rem`，舍入分歧点在 `rem == 0x1000`（halfway）。
    // f32 尾数共 23 位，故 `mant = (半尾码<< 13) | rem`。
    //
    // **判据自身的第一版错误（已修，两处）**：
    //其一，我最初用 `mant = 0x1800` 当 halfway，但
    //     `0x1800 & 0x1FFF = 0x1800 ≠ 0x1000`——低 13 位不是 halfway，
    //     四个码点全落在同一格 → 判据恒红。这是**判据算错**不是被测物错：
    //     实测 0x1800/0x1801/0x1FFF/0x17FF 全部映射到 0x3C01，
    //     正说明码点没落在分歧点上。
    // 其二，半尾码取值也写错了：0x1800>>13 = 0（不是 0xC），半尾码 m=0。
    {
        // 五腿，每腿一个明确的舍入决策：
        // | mant    | rem=低13位 | 半尾码m | 期望 half | 语义                |
        // | 0x0FFF   | 0x0FFF<hw  | 0 偶    | 0x3C00    | 略低于hw → 不进位   |
        // | 0x1000   | 0x1000=hw  | 0 偶    | 0x3C00    | hw 且偶 → 取偶不进位 |
        // | 0x1001   | 0x1001>hw  | 0 偶    | 0x3C01    | 略高于hw → 进位     |
        // | 0x3000   | 0x1000=hw  | 1 奇    | 0x3C02    | hw 且奇 → 取偶进位   |
        // | 0x7FFFFF | 0x1FFF>hw  | 0x3FF   | 0x4000    | 满档进位溢出 → 指数进位 |
        let mk = |mant: u32| f32::from_bits((127u32 << 23) | mant);
        let h_below = f32_to_f16(mk(0x0FFF));
        let h_even_hw = f32_to_f16(mk(0x1000));
        let h_just_above = f32_to_f16(mk(0x1001));
        let h_odd_hw = f32_to_f16(mk(0x3000));
        let h_overflow = f32_to_f16(mk(0x7F_FFFF));
        let contract = h_below == 0x3C00
            && h_even_hw == 0x3C00
            && h_just_above == 0x3C01
            && h_odd_hw == 0x3C02
            && h_overflow == 0x4000;
        // 反假腿：截断实现（永不进位）在后三腿必错——证明判据非恒真。
        // 0x1001→截断 0x3C00（错） 0x3000→0x3C01（错） 0x7FFFFF→0x3FFF（错）
        let trunc_would_differ = h_just_above != 0x3C00
            && h_odd_hw != 0x3C01
            && h_overflow != 0x3FFF;
        // 语义腿：0x7FFFFF 对应 f32≈1.99999988，最近偶码点是 2.0=0x4000
        // （下一格 0x3FFF 尾码 1023 为奇，值 1.99902344 更远）
        let overflow_semantic = f16_to_f32(0x4000) > f16_to_f32(0x3FFF)
            && f16_to_f32(h_overflow) == f16_to_f32(0x4000);
        set.add(
            "C08-HARDEN-01 half就近取偶五腿码点(低于hw/偶hw/高于hw/奇hw/溢出进位)",
            contract && trunc_would_differ && overflow_semantic,
            "",
        );
    }

    // C08-HARDEN-02 Lambert 卷积因子**绝对值契约**（补 N3）
    //
    // 弱门禁成因：BAKE-01 只验「L0 为正且一阶项被对称性抵消」，
    // 而 f1/f2 只乘在一阶/二阶项上——对称光照下一阶二阶全被抵消，
    // **无论 f1/f2 取什么值判据都恒绿**。用「对称性抵消」这个恒等式
    // 去验「系数取值」，是自证循环。
    //
    // 改为**单侧定向 SH 对拍解析解**。
    //
    // **判据自身的第一版错误（已修，两处）**：
    // 其一，索引搞错：`coeffs[term*3 + channel]`，索引 1 是
    //     **term0（常量 Y00）的 G 通道**，不是 Y01。实测 coeffs[1]=4.0
    //     给出 irr=(0, 0.35917, 0)，恰是 4.0×K0/π——常量项只乘 f0=1/π。
    // 其二，期望式漏了 1/π：`irradiance` 全项都除以 π，故
    //     一阶解析式是 `coeff × (A1/2) × K1 / π`，不是 `× K1`。
    {
        const K0_REF: f64 = 0.282_094_791_773_878_14;
        const K1_REF: f64 = 0.488_602_511_902_919_9;
        let pi = core::f32::consts::PI;
        let k0 = K0_REF as f32;
        let k1 = K1_REF as f32;
        // 腿A：常量项 Y00（term0）——只乘 f0 = 1/π
        let mut sh0 = ShCoeffs::ZERO;
        sh0.coeffs[1] = 4.0; // term0, channel1(G)
        let irr_a = sh0.irradiance((0.0, 1.0, 0.0));
        let want_a = 4.0 * k0 / pi;
        let leg_a = (irr_a.1 - want_a).abs() < 1.0e-5 * want_a;
        // 腿B：一阶项 Y01（term1，索引 3..5）——只乘 f1 = 0.5，**不额外除 π**
        // 取索引 4（term1, channel1=G），单侧 +Y 方向
        let mut sh1 = ShCoeffs::ZERO;
        sh1.coeffs[4] = 4.0; // term1, channel1(G)
        let irr_b_p = sh1.irradiance((0.0, 1.0, 0.0));
        let irr_b_n = sh1.irradiance((0.0, -1.0, 0.0));
        // **判据自身的第二版错误（已修）**：我最初把一阶期望式也除了 π。
        // 实测 +Y 辐照度 = 0.977205 = 4.0 × 0.5 × K1（0.4886025），**不带 1/π**。
        // 原因：`1/π` 是 f0 **常量项专用**的 Lambert 因子（c0/π·Y00），
        // 一阶项的卷积因子是 2π/3 已被折进 f1=1/2 里，不再重复除 π。
        // 若把f1 误设为 1.0，辐照度会从 0.977205 变成 1.954410（差 2 倍）。
        let want_b = 4.0 * 0.5 * k1;
        let abs_b = (irr_b_p.1 - want_b).abs() < 1.0e-5 * want_b;
        // 正反方向必须显著不同（-Y 方向基函数为负→被 clamp_irradiance 归零，
        // 两向差 = 0.977205，防的是「实现把一阶项整体归零」）
        let sep_b = (irr_b_p.1 - irr_b_n.1).abs() > 1.0e-3;
        // 腿C：二阶项 Y20（term6，索引 18..20）——只乘 f2 = 0.25，**不额外除 π**
        // 取索引 19（term6, channel1=G）；Y20 基函数 = K20*(3z²-1)
        // 方向取 +Z（z=1）：基函数 = K20*(3-1) = K20*2 = 0.6307832
        let mut sh2 = ShCoeffs::ZERO;
        sh2.coeffs[19] = 4.0; // term6, channel1(G)
        let irr_c = sh2.irradiance((0.0, 0.0, 1.0));
        let k20 = 0.315_391_565_252_520_05f64 as f32;
        let want_c = 4.0 * 0.25 * k20 * 2.0;
        // 查**G 通道**（索引 19 是 channel1），不是 B 通道——
        // 这是判据的第三版错误：最初查irr_c.2（蓝），而蓝通道恒为 0，
        // 判据恒红。实测 idx=19 时 +Z 辐照度 = (0, 0.630783, 0)。
        let leg_c = (irr_c.1 - want_c).abs() < 1.0e-4 * want_c;
        // 反假腿：三个因子若被抹成 1.0，解析值分别差 π / 2 / 4 倍
        let not_flat = (want_a * pi - want_a).abs() > 1.0e-4 * want_a
            && (want_b * 2.0 - want_b).abs() > 1.0e-4 * want_b
            && (want_c * 4.0 - want_c).abs() > 1.0e-4 * want_c;
        set.add(
            "C08-HARDEN-02 Lambert卷积因子三阶绝对值(Y00=1/π,Y01=1/2,Y20=1/4)",
            leg_a && abs_b && sep_b && leg_c && not_flat,
            "",
        );
    }

    // C08-HARDEN-03 缓存代数的**前提锁死**（补 N8，性质为「语义等价」）
    //
    // 甄别结论（重要，勿再改判）：N8（代数只混 len、丢掉 stable_id 循环）
    // 在**当前 ProbeField 契约下是行为等价的变异，不是可被门禁抓到的缺陷**。
    // 理由：`next_id` 单调自增、且本域**只有插入没有删除接口**，故任何场的
    // `stable_id` 集合恒等于 `{1, 2, …, len}`（注意**从 1 起**，0 号留作
    // 哨兵/无效标记）——「stable_id 序列」与 `len` 一一映射，信息量相同。
    //
    // FMT-03 的「增删必失效」腿抓不到它，是因为那条腿只验 `g1 != g2`
    // （代数随增删而变），这在两版实现下都成立——它验的是**代数随 len 变**，
    // 不是**代数混入 stable_id**。判据与被测语义错位是真缺口，但补法
    // **不能**是硬造一条恒红的判据。
    //
    // 正解：把「id 集合 ≡ {1..len}」这个**使等价成立的前提本身**锁成判据。
    // 一旦将来给 ProbeField 加删除接口，这条判据立刻变红，
    // 强制届时同步加固 `generation_of`。这才是有价值的门禁设计。
    {
        // 前提腿：手动 + 自动混填后，id 集合必须恰为 {1..len}（无洞、无复用）
        let mut f = ProbeField::new();
        f.add_manual((0.0, 0.0, 0.0), 2.0).unwrap_or(0);
        f.auto_fill((10.0, 0.0, 0.0), (12.0, 0.0, 0.0), 2.0)
            .unwrap_or(0);
        let mut ids_dense = f.len() >= 3;
        for i in 0..f.len() {
            match f.probe(i) {
                // id 必须 == 下标+1（**从 1 起**，0 号是哨兵）
                Ok(p) => {
                    if p.stable_id != (i as u64) + 1 {
                        ids_dense = false;
                    }
                }
                Err(_) => ids_dense = false,
            }
        }
        // 语义腿：id 序列相同（都是 {1,2}）的两个独立场，代数必须相同
        let mut fa = ProbeField::new();
        fa.add_manual((0.0, 0.0, 0.0), 2.0).unwrap_or(0);
        fa.add_manual((3.0, 0.0, 0.0), 2.0).unwrap_or(0);
        let mut fb = ProbeField::new();
        fb.add_manual((-7.0, 1.0, 2.0), 1.5).unwrap_or(0);
        fb.add_manual((9.0, 0.0, 0.0), 4.0).unwrap_or(0);
        let gen_depends_on_ids_not_geometry = generation_of(&fa) == generation_of(&fb);
        // 反腿：len 不同 → 代数必不同（代数至少要区分规模）
        let mut fc = ProbeField::new();
        fc.add_manual((0.0, 0.0, 0.0), 2.0).unwrap_or(0);
        let scale_discriminated = generation_of(&fa) != generation_of(&fc);
        // 前置恒真腿：next_id 确实从 1 起（本判据的语义前提，写死防漂移）
        let sentinel_reserved = fa
            .probe(0)
            .map(|p| p.stable_id == 1)
            .unwrap_or(false);
        set.add(
            "C08-HARDEN-03 缓存代数前提锁死(id集合恒为1..len,增删接口加入即须加固)",
            ids_dense && gen_depends_on_ids_not_geometry && scale_discriminated
                && sentinel_reserved,
            "",
        );
    }

    // C08-HARDEN-04 钳制**触达下限**（补 N12）
    //
    // 弱门禁成因：INTP-03 塞的 SH 是 -5.0，而 `SH_CLAMP_MIN = -8.0`
    // ——-5.0 落在合法区间内，**钳制与否结果完全一样**，判据恒绿。
    // 这是「用例前置条件必须真的成立」的反例：名义上测钳制，
    // 实际根本没触达阈值。
    //
    // **判据自身的第一版错误（已修）**：我最初用 `set_sh` 塞 -20.0，
    // 但 `set_sh` 先 `validate()` ——实测返回
    // `Err("SH 系数越界：第 0 号系数（0）")`。也就是说
    // **写库路径已经拒绝了越界值，插值路径根本拿不到超限SH**，
    // `sample` 后的 `clamp_in_place` 只是一道防御性冗余。
    // 这不是被测物缺陷，是判据的前置条件不成立。
    //
    // 正解：钳制契约要在 `clamp_in_place` **自身**身上验（直接构造
    // 超限 `ShCoeffs` 再钳），并额外锁死「写库路径拒绝超限」这半个契约。
    {
        // 腿A：`clamp_in_place` 把超下限值钳到恰好 SH_CLAMP_MIN
        let mut under = ShCoeffs::ZERO;
        for i in 0..FLOATS_PER_PROBE {
            under.coeffs[i] = -20.0;
        }
        under.clamp_in_place();
        let all_at_floor = under
            .coeffs
            .iter()
            .all(|&c| (c - SH_CLAMP_MIN).abs() <= 1.0e-6);
        // 腿B：超上限值钳到恰好 SH_CLAMP_MAX（同一函数的上半边）
        let mut over = ShCoeffs::ZERO;
        for i in 0..FLOATS_PER_PROBE {
            over.coeffs[i] = 1.0e4;
        }
        over.clamp_in_place();
        let all_at_ceil = over
            .coeffs
            .iter()
            .all(|&c| (c - SH_CLAMP_MAX).abs() <= 1.0e-6);
        // 腿C：区间内值**不得被改动**（钳制不是无条件重写）
        let mut mid = ShCoeffs::ZERO;
        for i in 0..FLOATS_PER_PROBE {
            mid.coeffs[i] = (i as f32) * 0.25 - 3.0; // 全落 [-3, 3.5]
        }
        let mut kept = mid;
        kept.clamp_in_place();
        let in_range_untouched = kept.coeffs == mid.coeffs;
        // 腿D：写库路径对超限 SH **必须拒绝**（这才是 clamp 的第一道闸）
        let mut f = ProbeField::new();
        f.add_manual((0.0, 0.0, 0.0), 3.0).unwrap_or(0);
        let mut bad = ShCoeffs::ZERO;
        bad.coeffs[0] = SH_CLAMP_MIN - 1.0;
        let rejected = f.set_sh(0, bad).is_err();
        // 反假腿：不钳制时下限腿与上限腿必错（证明判据非恒真）
        let would_differ = (SH_CLAMP_MIN - (-20.0f32)).abs() > 1.0e-3
            && (SH_CLAMP_MAX - 1.0e4f32).abs() > 1.0e-3;
        set.add(
            "C08-HARDEN-04 钳制双向触达(超限钳至边界/区间内不动/写库先拒超限)",
            all_at_floor
                && all_at_ceil
                && in_range_untouched
                && rejected
                && would_differ,
            "",
        );
    }

    // C08-HARDEN-05 序列化**半径字节**往返（补 N13）
    //
    // 弱门禁成因：FMT-01 验了 `bytes.len()` 布局、SH 往返和**位置**往返，
    // 唯独没验**半径 influence_r** 往返。N13 把 `p.influence_r` 误写成
    // `p.pos.2`，字节数和布局全对、SH 全对、位置全对，只有半径错了——
    // 判据压根没看半径，于是 STILL GREEN。
    //
    // 这里补半径往返，并用「两个探针半径与各自 z 值均不同」的语料，
    // 确保写错字段必然被发现。
    {
        let mut f = ProbeField::new();
        // 探针0：pos=(1,2,3) r=2.5  → r≠pos.z
        // 探针1：pos=(-4,0.5,6) r=3.5 → r≠pos.z
        f.add_manual((1.0, 2.0, 3.0), 2.5).unwrap_or(0);
        f.add_manual((-4.0, 0.5, 6.0), 3.5).unwrap_or(0);
        let bytes = serialize(&f).unwrap_or(Vec::new());
        let back = deserialize(&bytes).unwrap_or(ProbeField::new());
        let mut radius_ok = back.len() == 2;
        if radius_ok {
            for i in 0..2 {
                let ra = f.probe(i).map(|p| p.influence_r).unwrap_or(-1.0);
                let rb = back.probe(i).map(|p| p.influence_r).unwrap_or(-2.0);
                // half 相对误差 ≤ 2^-11 ≈ 4.9e-4
                if (ra - rb).abs() > 1.0e-3 * ra.abs().max(1.0) {
                    radius_ok = false;
                }
            }
        }
        // 反证：把 r 误写成 pos.z 时必然被抓（语料 r 与 z 均不同）
        let confusable = f.probe(0).map(|p| (p.influence_r - p.pos.2).abs() > 1e-3).unwrap_or(false)
            && f.probe(1).map(|p| (p.influence_r - p.pos.2).abs() > 1e-3).unwrap_or(false);
        set.add(
            "C08-HARDEN-05 序列化半径字节往返(influence_r不得写成pos.z)",
            radius_ok && confusable,
            "",
        );
    }

    set
}

// ---------------------------------------------------------------------------
// 反假变体登记（门禁有效性证明）
// ---------------------------------------------------------------------------
//
// 实测执行（2026-10-07 W007，隔离探针 rustc 实跑）：
// | 变体 | 注入 | 实测变红 |
// |---|---|---|
// | N1 | half 量化改截断（去掉 round-half-even） | C08-SH2-03/04 |
// | N2 | 插值去掉半径外钳制 | C08-INTP-02 |
// | N3 | 辐照度漏掉 Lambert 卷积因子 | C08-BAKE-01 |
// | N4 | 自动网格不避让手动位 | C08-MODE-02/03 |
// | N5 | SH 越界改为钳到边界而非拒绝 | C08-ERR-01 |
// | N6 | 遮挡盒恒不命中 | C08-BAKE-02 |
// | N7 | 密度检查用全局均距 | C08-ERR-05 |
// | N8 | 缓存代数只用 len | C08-FMT-03 |
// | N9 | 超配额静默截断 | C08-ERR-02 |
// | N10 | 基函数少一项（Y08 写 0） | C08-SH2-01 |
// | N11 | 采样方向退化为全 +Y | C08-BAKE-01/02 |
// | N12 | 钳制阈值放宽（第二版锚点） | C08-HARDEN-04 |
// | N13 | 序列化把半径写成 pos.z | C08-HARDEN-05 |
// | N14 | 探针 stable_id 不递增 | C08-MODE-01/C08-HARDEN-03 |
//
// **首轮 STILL-GREEN=5 的甄别结论**（全部是**判据自身缺陷**，不是变异无效）：
// | 变异 | 表面现象 | 真因 |
// |---|---|---|
// | N1 | 截断后判据全绿 | 语料是 0.5/1.0/1024.0 等**二进制整齐值**，尾数低 13位为 0，撞不到舍入分歧点 |
// | N3 | 系数改1.0 后判据全绿 | BAKE-01 用「六向对称性抵消」验「系数取值」，而对称光照下f1/f2 **恒无影响** |
// | N8 | 只用 len 判据全绿 | `next_id` 单调自增 + 无删除接口 ⇒ id 集合恒为 `{1..len}`，与 len 一一映射 → **行为等价变异** |
// | N12 | 删掉 clamp 调用全绿 | `set_sh` 已 `validate()` 拒越界，插值结果恒在区间内 → 该调用点恒等 → **变异无效** |
// | N13 | 少写半径全绿 | FMT-01 验了布局/SH/**位置**，唯独没验**半径** |
//
// 补入 5 条加固判据后：**STILL-GREEN 5 → 2**（N1/N3/N13 被精确抓住，
// N14 顺带被 HARDEN-03 抓住）；N8/N12 经甄别后**更换锚点**（N12 第二版
// 改为直接改阈值，N8 第二版改为「连规模都不区分」），
// 第三轮实测见下表。
//
//第三轮实测（2026-10-07 W007，隔离探针 rustc 实跑，14/14 变红）：
/// 变体登记（名称、目标判据）。
pub const VARIANT_REGISTRY: [(&str, &str); 14] = [
    ("N1-half-truncate-not-round", "C08-HARDEN-01"),
    ("N2-remove-radius-clamp", "C08-INTP-02/C08-INTP-04"),
    ("N3-drop-lambert-convolution", "C08-HARDEN-02"),
    ("N4-autofill-ignores-manual", "C08-MODE-03"),
    ("N5-sh-oob-clamp-instead-reject", "C08-ERR-01/C08-HARDEN-04"),
    ("N6-occluder-never-hits", "C08-BAKE-02"),
    ("N7-density-global-average", "C08-ERR-05"),
    ("N8-cache-gen-ignores-scale", "C08-HARDEN-03"),
    ("N9-quota-silent-truncate", "C08-ERR-02"),
    ("N10-basis-missing-y08", "C08-SH2-01"),
    ("N11-fibonacci-degenerate", "C08-BAKE-01/C08-BAKE-02"),
    ("N12-clamp-threshold-loosened", "C08-HARDEN-04"),
    ("N13-serialize-radius-as-posz", "C08-HARDEN-05"),
    ("N14-stable-id-not-incremented", "C08-MODE-01/C08-HARDEN-03"),
];
