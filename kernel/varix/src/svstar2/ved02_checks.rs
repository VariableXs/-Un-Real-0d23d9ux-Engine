//! VE-F0602 · 域自检（判据逐条对应，见 `ved02_xform.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 变换原点三参照系（自身中心/父原点/显式锚点）→ `D02-参照系-*`
//! - 级联契约（世界矩阵=父级联乘自身，顺序即视觉结果）→ `D02-级联-*`
//! - 分解与重组（可逆分解，供 F0608 插值）→ `D02-分解-*`
//! - 退化防护（奇异检测、降级恒等加告警、分量钳制）→ `D02-退化-*`
//! - 逆矩阵缓存（命中 O(1)、断链重算）→ `D02-逆缓存-*`
//! - 3D 仿射与投影 → `D02-三维-*`
//!
//! 浮点断言统一 1e-4 容差；零墙钟、零 IO，回归可复现。

use super::ved02_xform::*;
use crate::checks::CheckSet;

/// 浮点近似相等（1e-4 容差）。
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// 点近似相等。
fn pt_close(p: (f32, f32), q: (f32, f32)) -> bool {
    close(p.0, q.0) && close(p.1, q.1)
}

/// VE-F0602 域自检。
pub fn run_ved02_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ved02");

    // ---- 三参照系 ----

    // 判据：三参照系解析语义唯一（自身中心/父原点/显式锚点）。
    {
        let oc = OriginFrame::OwnCenter.resolve((100.0, 50.0), (7.0, 7.0));
        let po = OriginFrame::ParentOrigin.resolve((100.0, 50.0), (7.0, 7.0));
        let ea = OriginFrame::ExplicitAnchor.resolve((100.0, 50.0), (7.0, 7.0));
        set.add(
            "D02-参照系-三系解析",
            pt_close(oc, (50.0, 25.0)) && pt_close(po, (0.0, 0.0)) && pt_close(ea, (7.0, 7.0)),
            "",
        );
    }

    // 判据：自身中心参照系生效（绕自身中心旋转时中心点不动）。
    {
        let m = Mat2D::rotation(core::f32::consts::FRAC_PI_2);
        let sized = m.with_origin(OriginFrame::OwnCenter, (100.0, 50.0), (0.0, 0.0));
        let center = sized.apply(50.0, 25.0);
        // 角点 (0,0) 绕中心转 90° → (75, -25)（相对中心 (−50,−25) 旋转）。
        let corner = sized.apply(0.0, 0.0);
        set.add(
            "D02-参照系-自中心旋转中心不动",
            pt_close(center, (50.0, 25.0)) && pt_close(corner, (75.0, -25.0)),
            "",
        );
    }

    // 判据：显式锚点参照系生效（绕锚点旋转时锚点不动）。
    {
        let m = Mat2D::rotation(core::f32::consts::FRAC_PI_2);
        let anchored = m.with_origin(OriginFrame::ExplicitAnchor, (0.0, 0.0), (10.0, 0.0));
        let anchor_pt = anchored.apply(10.0, 0.0);
        set.add("D02-参照系-锚点不动", pt_close(anchor_pt, (10.0, 0.0)), "");
    }

    // ---- 级联契约 ----

    // 判据：世界矩阵=父级联乘自身（先 local 后 parent，顺序即视觉结果）。
    // 用缩放做 local（纯平移可交换，体现不出顺序）。
    {
        let parent = Mat2D::translation(10.0, 0.0);
        let local = Mat2D::scaling(2.0, 2.0);
        let world = cascade(&parent, &local);
        let p = world.apply(1.0, 1.0);
        // local 先行：(1,1)→(2,2)；parent 后行：→(12,2)。
        // 反序会得 (22,2)——顺序即视觉结果，不容两义。
        let swapped = local.mul(&parent).apply(1.0, 1.0);
        set.add(
            "D02-级联-父乘自顺序契约",
            pt_close(p, (12.0, 2.0)) && pt_close(swapped, (22.0, 2.0)) && !pt_close(p, swapped),
            "",
        );
    }

    // 判据：三层级联链（world = P·G·L 逐层成立，O(深度) 沿树）。
    {
        let l1 = Mat2D::translation(1.0, 0.0);
        let l2 = Mat2D::scaling(2.0, 2.0);
        let l3 = Mat2D::translation(0.0, 1.0);
        let w1 = cascade(&Mat2D::IDENTITY, &l1);
        let w2 = cascade(&w1, &l2);
        let w3 = cascade(&w2, &l3);
        let p = w3.apply(1.0, 1.0);
        // l3 先：(1,1)→(1,2)；l2 次之：→(2,4)；l1 最后：→(3,4)。
        set.add("D02-级联-三层链", pt_close(p, (3.0, 4.0)), "");
    }

    // ---- 分解与重组 ----

    // 判据：可逆分解（T·R·K·S 构造 → 分解四分量与构造值一致）。
    {
        let (tx, ty, rot, skx, sx, sy) = (3.0, -2.0, 0.5, 0.2, 2.0, 3.0);
        let m = Mat2D::recompose(tx, ty, rot, skx, sx, sy);
        let d = m.decompose().expect("非奇异");
        let ok = close(d.0, tx) && close(d.1, ty) && close(d.2, rot)
            && close(d.3, skx) && close(d.4, sx) && close(d.5, sy);
        set.add("D02-分解-分量还原", ok, "");
    }

    // 判据：分解/重组数学互逆（任意非奇异仿射 roundtrip 逐点一致）。
    {
        let m = Mat2D::translation(5.0, 7.0)
            .mul(&Mat2D::rotation(1.1))
            .mul(&Mat2D::skewing(0.3, 0.0))
            .mul(&Mat2D::scaling(1.5, 2.5));
        let (tx, ty, rot, skx, sx, sy) = m.decompose().expect("非奇异");
        let back = Mat2D::recompose(tx, ty, rot, skx, sx, sy);
        let p0 = m.apply(13.0, -4.0);
        let p1 = back.apply(13.0, -4.0);
        set.add("D02-分解-重组互逆", pt_close(p0, p1), "");
    }

    // ---- 退化防护 ----

    // 判据：奇异检测与降级恒等加告警（|det| < 阈值 → 恒等 + 告警入账）。
    {
        let singular = Mat2D::scaling(0.0, 0.0);
        let mut led = WarningLedger::default();
        let inv = WarningLedger::invert_or_identity(&mut led, &singular);
        let ok = inv == Mat2D::IDENTITY
            && led.count == 1
            && led.items[0].expect("告警在账").action == 0
            && singular.invert().is_none();
        set.add("D02-退化-奇异降级加告警", ok, "");
    }

    // 判据：插值分量钳制（奇异矩阵 decompose_clamped 不返回 None，
    // 缩放钳到 MIN_SCALE_CLAMP——插值穿越奇异→分量钳制）。
    {
        let singular = Mat2D::scaling(0.0, 5.0);
        let d = singular.decompose_clamped();
        let ok = d.4 == MIN_SCALE_CLAMP && d.5 >= MIN_SCALE_CLAMP;
        set.add("D02-退化-分量钳制", ok, "");
    }

    // 判据：正常矩阵求逆精确（M·M⁻¹ ≈ I，逐点 roundtrip）。
    {
        let m = Mat2D::translation(9.0, -3.0).mul(&Mat2D::rotation(0.7)).mul(&Mat2D::scaling(2.0, 0.5));
        let inv = m.invert().expect("非奇异");
        let p = m.apply(7.0, 7.0);
        let q = inv.apply(p.0, p.1);
        set.add("D02-退化-求逆精确", pt_close(q, (7.0, 7.0)), "");
    }

    // ---- 逆矩阵缓存 ----

    // 判据：逆缓存命中 O(1)（同矩阵二次查询命中；换矩阵未命中；断链清空）。
    {
        let m1 = Mat2D::translation(3.0, 4.0).mul(&Mat2D::scaling(2.0, 2.0));
        let m2 = Mat2D::translation(1.0, 1.0);
        let mut cache = InvCache::new();
        let i1 = cache.invert_cached(&m1).expect("非奇异");
        let i1b = cache.invert_cached(&m1).expect("非奇异");
        let hit = cache.hits == 1 && cache.misses == 1 && i1 == i1b;
        let _ = cache.invert_cached(&m2).expect("非奇异");
        let miss_after_new = cache.misses == 2;
        cache.clear();
        let _ = cache.invert_cached(&m1).expect("非奇异");
        let recount = cache.misses == 3;
        // 逆的正确性：m1 · inv(m1) ≈ I（逐点 roundtrip）。
        let p = i1.apply(m1.apply(4.0, 6.0).0, m1.apply(4.0, 6.0).1);
        set.add(
            "D02-逆缓存-命中与断链重算",
            hit && miss_after_new && recount && pt_close(p, (4.0, 6.0)),
            "",
        );
    }

    // ---- 3D 仿射与投影 ----

    // 判据：3D 旋转保长度（rotation_x 单位向量旋转后仍单位长度）。
    {
        let m = Mat4::rotation_x(0.9);
        let (x, y, z) = m.apply_point(0.0, 1.0, 0.0);
        let len = (x * x + y * y + z * z).sqrt();
        set.add("D02-三维-旋转保长度", close(len, 1.0), "");
    }

    // 判据：透视投影（近平面 → NDC z=0，远平面 → NDC z=1）。
    {
        let proj = Mat4::perspective(core::f32::consts::FRAC_PI_2, 16.0 / 9.0, 0.1, 100.0);
        let near_pt = proj.apply_point(0.0, 0.0, -0.1);
        let far_pt = proj.apply_point(0.0, 0.0, -100.0);
        set.add(
            "D02-三维-透视近平远平",
            close(near_pt.2, 0.0) && close(far_pt.2, 1.0),
            "",
        );
    }

    // 判据：3D 级联顺序契约（T·R 与 R·T 结果不同——顺序即视觉结果）。
    {
        let t = Mat4::translation(5.0, 0.0, 0.0);
        let r = Mat4::rotation_z(core::f32::consts::FRAC_PI_2);
        let a = t.mul(&r).apply_point(1.0, 0.0, 0.0);
        let b = r.mul(&t).apply_point(1.0, 0.0, 0.0);
        set.add(
            "D02-三维-顺序契约",
            !pt_close((a.0, a.1), (b.0, b.1)),
            "",
        );
    }

    // ---- 边界防护 ----

    // 判据：非有限输入零容忍（NaN/Inf 在入口拦住，不进级联链）。
    {
        let bad1 = Mat2D::validated(1.0, 0.0, 0.0, 1.0, f32::NAN, 0.0);
        let bad2 = Mat2D::validated(1.0, 0.0, 0.0, 1.0, 0.0, f32::INFINITY);
        let good = Mat2D::validated(1.0, 0.0, 0.0, 1.0, 5.0, 5.0);
        let mut m4 = Mat4::IDENTITY;
        m4.m[2][2] = f32::NAN;
        set.add(
            "D02-防护-非有限拦截",
            bad1.is_none() && bad2.is_none() && good.is_some() && !m4.is_finite(),
            "",
        );
    }

    // 判据：分量插值（F0608 入口：分量级 lerp，旋转半程不塌缩；
    // t 越界钳制到端点）。
    {
        let from = Mat2D::translation(0.0, 0.0);
        let to = Mat2D::translation(10.0, 0.0).mul(&Mat2D::rotation(core::f32::consts::FRAC_PI_2));
        let half = from.lerp_components(&to, 0.5);
        let (tx, _ty, r, _k, _sx, _sy) = half.decompose().expect("非奇异");
        // 半程：平移 5、旋转 45°——分量各自插值，元素级 lerp 早已破相。
        let over = from.lerp_components(&to, 7.0);
        let (tx_over, _ty_over, r_over, _k2, _sx2, _sy2) = over.decompose().expect("非奇异");
        set.add(
            "D02-插值-分量插值与越界钳制",
            close(tx, 5.0) && close(r, core::f32::consts::FRAC_PI_4)
                && close(tx_over, 10.0) && close(r_over, core::f32::consts::FRAC_PI_2),
            "",
        );
    }

    // 判据：正交投影（近远平面映射到 0..1；边界点映射保持线性）。
    {
        let ortho = Mat4::orthographic(0.0, 100.0, 0.0, 100.0, 1.0, 11.0);
        let mid = ortho.apply_point(50.0, 50.0, -6.0);
        set.add(
            "D02-三维-正交投影",
            close(mid.0, 0.0) && close(mid.1, 0.0) && close(mid.2, 0.5),
            "",
        );
    }

    // ---- 读屏 ----

    {
        let s = screen_text(&Mat2D::IDENTITY);
        set.add("D02-读屏-矩阵摘要可播", s.contains("变换矩阵"), "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ved02_all_judgements_green() {
        let set = run_ved02_checks();
        let (passed, failed) = set.tally();
        let (reds, n) = set.red_items();
        let names: Vec<&'static str> = (0..n)
            .filter_map(|i| reds[i].as_ref().filter(|c| !c.passed).map(|c| c.name))
            .collect();
        assert!(
            set.all_passed(),
            "VE-F0602 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            names
        );
    }

    #[test]
    fn ved02_identity_is_involutive() {
        // 恒等的逆是恒等；I·M = M（级联幺元）。
        let m = Mat2D::translation(2.0, 3.0);
        assert_eq!(Mat2D::IDENTITY.mul(&m), m);
        assert_eq!(Mat2D::IDENTITY.invert(), Some(Mat2D::IDENTITY));
    }
}
