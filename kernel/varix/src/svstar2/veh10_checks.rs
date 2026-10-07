//! VE-F1410 · 域自检（判据逐条对应，见 `veh10_occlusion.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 两档判定（遮挡比例阈值分档）→ `H10-两档-*`
//! - 几何接口预留（一期球体代理 + 诚实标注）→ `H10-几何-*`
//! - 材质联动（F1431 系数输入 / 混凝土 vs 布帘）→ `H10-材质-*`
//! - 平滑过渡（50ms 渐变 / 不跳变）→ `H10-平滑-*`
//!
//! ## 弱门禁防线（本文件的判据为什么这样写）
//!
//! 1. **档位判据断"两档参数不等"是弱门禁**——一个"两档恰好用了同一组数"的
//!    实现能骗过它。故 `H10-两档-半挡不低通` 直接断**截止频率精确等于开路值**
//!    （单边等式，而非双边阈值），`H10-两档-全挡低通更严` 用**单边符号**
//!    `cutoff_full < cutoff_partial`，不用"差值 > 某阈值"的双边判据。
//!
//! 2. **阈值边界用夹逼对钉死**（`PARTIAL_ENTER ± TIER_EPS`、
//!    `FULL_ENTER ± TIER_EPS`）。只测"阈值之上"则闸门位置无人验证。
//!
//! 3. **遮挡比例的参考值由判据侧独立重算**，不调被测函数自证
//!    （`H10-材质-单球比例独立重算` 用闭式解 `2√(r²−d²)/L` 手算期望值）。
//!
//! 4. **重叠不双计**必须显式断（`H10-材质-重叠球不双计`）：一个"逐段相加"的
//!    实现会给出 2 倍比例，若判据侧顺手 `clamp` 到 1 则两者都"看起来对"。
//!    故此处断**两块完全重合球 == 一块球**的**相等**（非不等）。
//!
//! 5. **平滑判据断中点与端点的精确值**，非"发生了变化"：断 25ms 处恰为
//!    起止中点、49ms 未到位、50ms 已到位。"发生了变化"被"瞬间跳到目标再跳回"
//!    的实现全绿通过。
//!
//! 6. **过冲判据断"全程落在两端点之间"**（`H10-平滑-全程不越界`）：线性插值
//!    的结构性性质，用密集采样验证；一个用 `t` 未钳制的实现会越界。
//!
//! 7. **判据索引不随语料漂移**：所有夹逼对的中心值取自被测代码的公开常量
//!    （`PARTIAL_ENTER` / `FULL_ENTER` / `TRANSITION_MS`），改常量则判据跟着
//!    走，不会出现"判据还钉在旧常量上"的隐性弱门禁。
//!
//! 8. **变体反向验证**（`VARIANT_REGISTRY` 登记）：判据全绿只证明"当前实现合
//!    判据"。本轮用 10 个定向变异逐条反查，**10/10 全部被捕获**，证明判据不是恒真；
//!    其中 M3/M4/M8 各只转红 1–2 条是**符合预期的低冗余度**——三者对应"最通
//!    路径""未覆盖贡献 0""并集不双计"三条互不重叠语义，各有专属判据钉住。

use super::veh10_occlusion::*;
// `Cartesian` 在 veh10_occlusion 里是私有 import（`use ...` 非 `pub use`），
// 故 glob 不会把它带进来——判据侧须显式引一次，否则"两套坐标混用"的
// 纪律在编译期就漏了。
use super::veh09_spatial::Cartesian;
use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 构造一颗合法的遮挡代理球（材质越界时代替为混凝土，测试用）。
fn sphere(center: Cartesian, radius_m: f32) -> OccluderSphere {
    match OccluderSphere::new(center, radius_m, MAT_CONCRETE) {
        Ok(s) => s,
        Err(_) => OccluderSphere {
            center: Cartesian::ORIGIN,
            radius_m: 1.0,
            material: MAT_CONCRETE,
        },
    }
}

/// 声源在原点、听者在 +Z 方向 `L` 米处（视线的参数化基准）。
fn segment_z(len_m: f32) -> (Cartesian, Cartesian) {
    (Cartesian::ORIGIN, Cartesian::new(0.0, 0.0, len_m))
}

/// 独立重算：一条长 `L`、球心在垂直距`d`、半径 `r` 的球所截的**参数**长度，
/// 再乘该材质的遮挡量，得到材质加权后的期望遮挡比例。
///
/// 这是判据侧的参考值来源——**刻意不复用** `weighted_blocked_ratio`，
/// 也不调用 `segment_sphere_span`。若判据问被测函数要答案，则判据与实现
/// 同时错时仍全绿（自证式门禁）。
///
/// `material` 参与乘法是必需的：只重算几何弦长会把"材质加权"这一步漏掉，
/// 于是实现即使完全不查材质，本判据也仍绿（半覆盖混凝土：正确值
/// `0.5 × 0.92 = 0.46`，纯几何值 `0.5`）。
fn independent_blocked(
    len_m: f32,
    offset_m: f32,
    radius_m: f32,
    material: AcousticMaterial,
) -> f32 {
    if radius_m <= offset_m {
        return 0.0;
    }
    if len_m <= 0.0 {
        return 1.0;
    }
    // 弦长（球心垂足落在段中点的最简情形）→ 参数长度 = 弦长 / L。
    let half_chord = fsqrt(radius_m * radius_m - offset_m * offset_m);
    let chord = 2.0 * half_chord;
    let geometric = chord / len_m;
    let weighted = if geometric > 1.0 {
        material.block_of()
    } else {
        geometric * material.block_of()
    };
    if weighted > 1.0 {
        1.0
    } else {
        weighted
    }
}

pub fn run_veh10_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veh10");

    // ---- 判据：几何接口预留（一期球体代理 + 诚实标注）----

    {
        // 空场景 = 无遮挡：基线必须为开路（增益 1、截止开路、低通未开）。
        // 若空场景就带上了低通，则"半遮挡只衰减不低通"永无干净基线可比。
        let v = classify(&NullGeometry, Cartesian::ORIGIN, Cartesian::new(0.0, 0.0, 4.0));
        set.add(
            "H10-几何-空场景为开路基线",
            v.grade == OcclusionGrade::Clear
                && v.target_gain == 1.0
                && v.target_cutoff_hz == OPEN_CUTOFF_HZ
                && !v.lowpass_active()
                && v.blocked_ratio == 0.0,
            "",
        );
    }
    {
        // 一期简化标注须含三个关键成分：取舍性质、代价、二期路径。
        // 只查"非空"是弱门禁（任何字符串都非空）。
        let d = GEOMETRY_PHASE1_DECLARATION;
        set.add(
            "H10-几何-一期简化诚实标注",
            d.contains("工程取舍")
                && d.contains("球体代理")
                && d.contains("二期")
                && d.contains("OcclusionGeometry"),
            "",
        );
    }
    {
        // trait 接缝真实可替换：两个不同实现都能走通同一条判定链路。
        // 若trait 只是纸面声明（只有一个实现），本项无法构造两个实例。
        let mut g = SphereProxyGeometry::new();
        let _ = g.add(sphere(Cartesian::new(0.0, 0.0, 2.0), 3.0));
        let (a, b) = segment_z(4.0);
        let via_proxy = classify(&g, a, b);
        let via_null = classify(&NullGeometry, a, b);
        set.add(
            "H10-几何-接口两实现可替换",
            via_proxy.grade == OcclusionGrade::Full
                && via_null.grade == OcclusionGrade::Clear
                && via_proxy.blocked_ratio > via_null.blocked_ratio,
            "",
        );
    }
    {
        // 球心在段外（垂距 > 半径）→ 解析求交必须判为无交。
        // 若判别式写成 `> 0` 之外的形式（如漏掉 `< 0` 的 None 分支），
        // 负判别式会开方出 NaN，此处即红。
        let mut g = SphereProxyGeometry::new();
        let _ = g.add(sphere(Cartesian::new(0.0, 5.0, 2.0), 1.0));
        let (a, b) = segment_z(4.0);
        let v = classify(&g, a, b);
        set.add(
            "H10-几何-段外球判为无交",
            v.blocked_ratio == 0.0 && v.grade == OcclusionGrade::Clear,
            "",
        );
    }
    {
        // 零长连线（声源与听者重合）不得 panic，退化为点包含：球含该点 →
        // 区间 `[0,1]` 全覆盖；不含 → 无交。
        //
        // 期望值是 `1 −透声`（混凝土 0.92）而**不是 1.0**：全覆盖的连线
        // 仍要过材质加权（挡住 ≠ 完全不透声）。写成 1.0 会把"漏了材质
        // 加权"的实现判绿。
        let want_hit = MAT_CONCRETE.block_of();
        let mut hit = SphereProxyGeometry::new();
        let _ = hit.add(sphere(Cartesian::ORIGIN, 2.0));
        let o = Cartesian::ORIGIN;
        let v_hit = classify(&hit, o, o);
        let mut miss = SphereProxyGeometry::new();
        let _ = miss.add(sphere(Cartesian::new(10.0, 0.0, 0.0), 1.0));
        let v_miss = classify(&miss, o, o);
        set.add(
            "H10-几何-零长连线退化为点包含",
            (v_hit.blocked_ratio - want_hit).abs() < 1.0e-5
                && v_miss.blocked_ratio == 0.0
                && v_hit.target_gain.is_finite()
                && v_hit.grade == OcclusionGrade::Full
                && v_miss.grade == OcclusionGrade::Clear,
            "",
        );
    }
    {
        // **相切退化**（球面恰好切于视线，判别式 = 0 ⇒ t0 == t1）：不得产生
        // 任何遮挡量。
        //
        // 这条补的是一个**真实覆盖洞**：相切时 `t1 > t0` 不成立，该分支必须
        // 返回 `None`。若删掉它，零长区间会被推进积分——而后续的 `mid > a &&
        // mid < b` 判定对零长区间恒为假，**积分结果仍不变**，于是缺陷对
        // 所有外部判据完全隐形（变异实测：删掉该分支，43 项全绿）。
        //
        // 故此处断的是**结构性事实**而非最终数值：相切球的 span 数必须为 0。
        let mut g = SphereProxyGeometry::new();
        // 段沿 z 轴 0→4；球心 (0,1,2) 半径 1 ⇒ 球心到轴线距恰为 1 = r → 相切。
        let _ = g.add(sphere(Cartesian::new(0.0, 1.0, 2.0), 1.0));
        let (a, b) = segment_z(4.0);
        let spans = g.collect_spans(a, b);
        let v = classify(&g, a, b);
        set.add(
            "H10-几何-相切退化不产生遮挡",
            spans.is_empty() && v.blocked_ratio == 0.0 && v.grade == OcclusionGrade::Clear,
            "",
        );
    }
    {
        // 非法几何输入零 panic：非有限坐标不产出区间（不静默变成"部分遮挡"）。
        let mut g = SphereProxyGeometry::new();
        let _ = g.add(sphere(Cartesian::new(0.0, 0.0, 2.0), 1.0));
        let nan = Cartesian::new(f32::NAN, 0.0, 4.0);
        let v = classify(&g, nan, Cartesian::new(0.0, 0.0, 4.0));
        set.add(
            "H10-几何-非有限坐标安全降级",
            v.blocked_ratio.is_finite() && v.target_gain.is_finite(),
            "",
        );
    }
    {
        // 构造期校验：负半径 / 零半径 / 越界材质一律拒。
        let bad_r = OccluderSphere::new(Cartesian::ORIGIN, -1.0, MAT_CONCRETE).is_err();
        let zero_r = OccluderSphere::new(Cartesian::ORIGIN, 0.0, MAT_CONCRETE).is_err();
        let bad_m = OccluderSphere::new(
            Cartesian::ORIGIN,
            1.0,
            AcousticMaterial {
                id: 99,
                name: "越界材质",
                transmission: 1.7,
                absorption: 0.1,
                lowpass_hz: 700.0,
            },
        )
        .is_err();
        let nan_c = OccluderSphere::new(
            Cartesian::new(f32::INFINITY, 0.0, 0.0),
            1.0,
            MAT_CONCRETE,
        )
        .is_err();
        set.add(
            "H10-几何-构造期校验拒绝非法",
            bad_r && zero_r && bad_m && nan_c,
            "",
        );
    }

    // ---- 判据：材质联动（F1431 系数输入 / 混凝土 vs 布帘）----

    {
        // 判据侧独立重算参考值：不调被测函数，闭式解手算。
        // 三组几何（不同半径/垂距/段长）各取一点，避免"恰好一次对上"。
        let cases: [(f32, f32, f32); 3] = [(4.0, 0.0, 1.0), (6.0, 0.5, 2.0), (5.0, 0.0, 0.6)];
        let mut all = true;
        for (len, offset, radius) in cases.iter() {
            let mut g = SphereProxyGeometry::new();
            let _ = g.add(sphere(Cartesian::new(0.0, *offset, len / 2.0), *radius));
            let (a, b) = segment_z(*len);
            let got = classify(&g, a, b).blocked_ratio;
            let want = independent_blocked(*len, *offset, *radius, MAT_CONCRETE);
            // 单边符号：实测不得**超过**参考值（超出=求交解算错误）。
            // 不用双边阈值——双边阈值宽过正确实现的低估幅度。
            if got - want > 1.0e-4 {
                all = false;
            }
            // 另一侧单独断"低估不超过容差"，防"恒返回 0"骗过上式。
            if want - got > 1.0e-3 {
                all = false;
            }
        }
        set.add("H10-材质-单球比例独立重算", all, "");
    }
    {
        // 材质让遮挡"有质感"的核心判据：**同一几何、不同材质 → 不同结果**。
        // 若材质未参与（几何长度直接当比例），此判据红。
        //
        // 布帘（透声 0.72）盖满全段 → blocked = 0.28；混凝土（0.08）→ 0.92。
        // 二者分档亦不同：0.28 落半挡、0.92 落全挡。
        let mut curtain = SphereProxyGeometry::new();
        let _ = curtain.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 4.0, MAT_CURTAIN));
        let mut concrete = SphereProxyGeometry::new();
        let _ = concrete.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 4.0, MAT_CONCRETE));
        let (a, b) = segment_z(4.0);
        let vc = classify(&curtain, a, b);
        let vk = classify(&concrete, a, b);
        let geom_same = vc.grade != vk.grade;
        set.add(
            "H10-材质-混凝土与布帘分档不同",
            geom_same
                && vc.grade == OcclusionGrade::Partial
                && vk.grade == OcclusionGrade::Full,
            "",
        );
    }
    {
        // 布帘的遮挡比例须**小于**混凝土（单边符号）。
        // 透声系数反向（布帘更挡）时本判据红。
        let mut curtain = SphereProxyGeometry::new();
        let _ = curtain.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 4.0, MAT_CURTAIN));
        let mut concrete = SphereProxyGeometry::new();
        let _ = concrete.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 4.0, MAT_CONCRETE));
        let (a, b) = segment_z(4.0);
        set.add(
            "H10-材质-布帘遮挡小于混凝土",
            classify(&curtain, a, b).blocked_ratio
                < classify(&concrete, a, b).blocked_ratio,
            "",
        );
    }
    {
        // 全阻挡档的截止频率取**贡献主导**材质。
        //
        // 实测跨度（段长 4，z 轴）：玻璃 span len=0.75 → weight 0.75×0.65 =
        // 0.4875；混凝土 span len=0.25 → weight 0.25×0.92 = 0.2300。
        // 玻璃贡献更大 ⇒ 主导者是玻璃 ⇒ cutoff = 2600。
        //
        // 注意 span 长度**不等于**球直径：球超出段的部分在 t 归一化时被裁掉。
        // （早先按"直径=跨度"估算会得到相反结论。）
        //
        // 注：早先这里写的是"取最严材质"（两种材质并存时断700），
        // 该口径已被 `dominant_cutoff` 的加权理由取代（见 veh10_occlusion.rs
        // 的 `dominant_cutoff` 文档）——朴素取最小会让边缘材质一票否决主导材质。
        let mut g = SphereProxyGeometry::new();
        let _ = g.add(sphere_with(Cartesian::new(0.0, 0.0, 1.5), 1.5, MAT_GLASS));
        let _ = g.add(sphere_with(Cartesian::new(0.0, 0.0, 3.5), 0.5, MAT_CONCRETE));
        let (a, b) = segment_z(4.0);
        let v = classify(&g, a, b);
        set.add(
            "H10-材质-全挡取贡献主导低通",
            v.grade == OcclusionGrade::Full
                && (v.target_cutoff_hz - MAT_GLASS.lowpass_hz).abs() < CUTOFF_EPS_HZ,
            "",
        );
    }
    {
        // 重叠不双计（并集语义）：两块**完全重合**的球 == 一块球。
        // 断相等而非不等——"逐段相加"给2×比例，若判据顺手 clamp 到 1，
        // 两者都会看起来对，故此处必须断精确相等。
        let mut one = SphereProxyGeometry::new();
        let _ = one.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 1.0, MAT_CURTAIN));
        let mut two = SphereProxyGeometry::new();
        let _ = two.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 1.0, MAT_CURTAIN));
        let _ = two.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 1.0, MAT_CURTAIN));
        let (a, b) = segment_z(4.0);
        let r1 = classify(&one, a, b).blocked_ratio;
        let r2 = classify(&two, a, b).blocked_ratio;
        set.add(
            "H10-材质-重叠球不双计",
            (r1 - r2).abs() < 1.0e-5 && r1 < 1.0,
            "",
        );
    }
    {
        // 取最通路径：混凝土(透声0.08) 与 布帘(0.72) **完全重合**时，
        // 实际到达量由布帘决定 → blocked = 0.28，而非 0.92（取最严会被抓）。
        // 这是"并集取 max 透声"规则的专属判据——两条错路外部表现相同
        // （都是"全覆盖"），不给专属断言则删掉任一条仍全绿。
        let mut g = SphereProxyGeometry::new();
        let _ = g.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 3.0, MAT_CONCRETE));
        let _ = g.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 3.0, MAT_CURTAIN));
        let (a, b) = segment_z(4.0);
        let got = classify(&g, a, b).blocked_ratio;
        let want = 1.0 - MAT_CURTAIN.transmission;
        set.add(
            "H10-材质-并存取最通路径",
            (got - want).abs() < 1.0e-4,
            "",
        );
    }
    {
        // 相邻（不重合）两段各半：比例须**相加**（0.5×block_a + 0.5×block_b），
        // 而不是取 max 也不是取 min——这与上一条构成对偶，把"积分 vs 取极值"
        // 的实现错误双向夹住。
        let a_mat = MAT_CONCRETE; // block 0.92
        let b_mat = MAT_CURTAIN; // block 0.28
        let spans = vec![
            OcclusionSpan {
                t0: 0.0,
                t1: 0.5,
                material: a_mat,
            },
            OcclusionSpan {
                t0: 0.5,
                t1: 1.0,
                material: b_mat,
            },
        ];
        let got = weighted_blocked_ratio(spans.as_slice());
        let want = 0.5 * a_mat.block_of() + 0.5 * b_mat.block_of();
        set.add(
            "H10-材质-相邻两段按长度加权",
            (got - want).abs() < 1.0e-4,
            "",
        );
    }
    {
        // **贡献主导的截止频率**（本判据的存在源于一次实测缺陷）。
        //
        // 朴素实现"遍历全部 span 取最小 lowpass"在此转红：混凝土盖 90%
        // 视线（贡献 0.828）却输给出力 7.5%、几乎不挡声（贡献 0.00075）
        // 的材质 —— cutoff 被拉到 50Hz，比混凝土本身（700Hz）还闷。
        // 听感上等于"墙后声音比墙还厚"，是明确的过度低通。
        //
        // 幽灵材质用**显式构造**而非新增内置材质：它是本判据的专属对抗
        // 样本，不属于"混凝土/玻璃/木板/布帘"那套物理材质表。
        let ghost = AcousticMaterial {
            id: 77,
            name: "幽灵材质",
            transmission: 0.99,
            absorption: 0.01,
            lowpass_hz: 50.0,
        };
        let (a, b) = segment_z(4.0);
        let mut g = SphereProxyGeometry::new();
        let _ = g.add(sphere_with(Cartesian::new(0.0, 0.0, 1.8), 1.8, MAT_CONCRETE));
        let _ = g.add(sphere_with(Cartesian::new(0.0, 0.0, 3.8), 0.15, ghost));
        let v = classify(&g, a, b);
        set.add(
            "H10-材质-截止频率取贡献主导",
            v.grade == OcclusionGrade::Full
                // 主导者是混凝土：cutoff 须为 700Hz，且**严格大于**幽灵的 50Hz。
                && (v.target_cutoff_hz - MAT_CONCRETE.lowpass_hz).abs() < CUTOFF_EPS_HZ
                && v.target_cutoff_hz > ghost.lowpass_hz,
            "",
        );
    }
    {
        // 加权口径的**方向性**：同 blocked 量级下，谁挡得多听谁的。
        //
        // 关键：两种构造都必须落在 **Full 档**——若其中一个落到 Partial，
        // cutoff 按定义恒为开路值（半挡不低通），"主导翻转"就无从观察，
        // 判据会因档位差异而假绿。故用混凝土/玻璃配对（两者block 分别
        // 0.92/0.65，都能把 blocked 推过 0.55），只调长度比来翻转主导：
        //
        // - 玻璃段更长（weight 0.4875> 0.2300）⇒ 主导玻璃 ⇒ cutoff = 2600
        // - 混凝土段更长（weight 0.2300 vs 0.1625）⇒ 主导混凝土 ⇒ cutoff = 700
        //
        // (b) 是关键：只断 (a) 的话，"恒取最小"或"恒取最大"都能全绿。
        let (a, b) = segment_z(4.0);
        let mk = |clen: f32, glen: f32| {
            let mut g = SphereProxyGeometry::new();
            let _ = g.add(sphere_with(
                Cartesian::new(0.0, 0.0, clen / 2.0),
                clen / 2.0,
                MAT_CONCRETE,
            ));
            let _ = g.add(sphere_with(
                Cartesian::new(0.0, 0.0, clen + glen / 2.0),
                glen / 2.0,
                MAT_GLASS,
            ));
            classify(&g, a, b)
        };
        let concrete_heavy = mk(3.0, 1.0);
        let glass_heavy = mk(1.0, 3.0);
        set.add(
            "H10-材质-加权方向随主导翻转",
            concrete_heavy.grade == OcclusionGrade::Full
                && glass_heavy.grade == OcclusionGrade::Full
                && (concrete_heavy.target_cutoff_hz - MAT_CONCRETE.lowpass_hz).abs() < CUTOFF_EPS_HZ
                && (glass_heavy.target_cutoff_hz - MAT_GLASS.lowpass_hz).abs() < CUTOFF_EPS_HZ
                // 主导翻转 ⇒ cutoff 必须跟着翻转。
                && concrete_heavy.target_cutoff_hz < glass_heavy.target_cutoff_hz,
            "",
        );
    }
    {
        // **长度与密度解耦的主导判据**（补判据：源于变异验证 M7 漏过）。
        //
        // 既有两条 cutoff 判据（`H10-材质-截止频率取贡献主导` /
        // `H10-材质-加权方向随主导翻转`）都让"占线长度"与"材质密度"
        // **同向变化**——混凝土段既更长又更致密。于是权重口径
        // `weight = len × (1 − 透声)` 被悄悄换成 `weight = len`
        // （只看占线多长）时，两条判据**照样全绿**：更长者恒等于更致密者，
        // 两个口径给出同一个赢家。判据全绿而实现已错——这正是弱门禁。
        //
        // 本判据把两个因子**反向**拉开：让**更短但更致密**的段成为主导。
        // 此时 `len` 口径会选长段、`len×block` 口径会选短段，两者结论
        // 相反，故任何只按长度取主导的实现必转红。
        //
        // 幽灵材质用显式构造（不属于内置四材质）：transmission=0.02
        // （block=0.98，近乎不透），lowpass 取 120Hz——若实现误按长度
        // 选中了长玻璃段，cutoff 会落到 2600，与期望的 120 差一个数量级。
        //
        // 参数是**反解**出来的，不是随手写的：要同时满足
        //   ① `dl·0.98 > gl·0.65`（真权重口径下致密段胜出）且
        //   ② `dl < gl`（长度口径下致密段**落后**）——
        // 两式合起来要求 `bg <bd` 且 `1 < gl/dl < bd/bg`，
        // 即"短而致密"必须真的既短又致密，两个口径才给出**相反**赢家。
        //
        // 取 `gl = 0.70`、`dl = 0.50`（`gl/dl = 1.4`，落在开区间
        // `(1, 0.98/0.65≈1.508)` 内）：
        //   真权重 玻璃 0.4550 <致密 0.4900 ⇒ 致密段主导（120Hz）
        //   长度 玻璃 0.70 > 致密 0.50⇒ 玻璃段主导（2600Hz）
        // 二者结论相反，故任何只按长度取主导的实现必转红。
        // 且 `blocked = 0.945 ≥ 0.55` 落在 Full 档，
        // "cutoff 由主导材质决定"才可观察。
        //
        // （初版曾取 `gl=0.30/dl=0.55`——看似短致密段更重，实则
        //  `dl=0.55 > gl=0.30`，致密段反而**更长**，两口径同向，
        //  变异体照样全绿。此处记下这个反例以免后人重犯。）
        let ghost = AcousticMaterial {
            id: 78,
            name: "幽灵致密材质",
            transmission: 0.02,
            absorption: 0.01,
            lowpass_hz: 120.0,
        };
        let (a, b) = segment_z(4.0);
        let mut g = SphereProxyGeometry::new();
        // 玻璃段占参数长度 0.70（更长、更稀疏）：t=[0, 0.70]
        let _ = g.add(sphere_with(Cartesian::new(0.0, 0.0, 1.4), 1.4, MAT_GLASS));
        // ……致密段占参数长度 0.50（更短、更致密）：t=[0.50, 1.00]
        let _ = g.add(sphere_with(Cartesian::new(0.0, 0.0, 3.0), 1.0, ghost));
        let v = classify(&g, a, b);
        set.add(
            "H10-材质-短而致密段压过长而稀疏段",
            v.grade == OcclusionGrade::Full
                // 主导者须是短而致密的幽灵段：cutoff 精确等于它的 120Hz。
                && (v.target_cutoff_hz - ghost.lowpass_hz).abs() < CUTOFF_EPS_HZ
                // 单边符号：主导 cutoff 必须**严格低于**长玻璃段的 2600Hz。
                // 用单边而非双边阈值——双边阈值宽过正确实现的低估幅度时，
                // 高估型错误会从缝里钻过去。
                && v.target_cutoff_hz < MAT_GLASS.lowpass_hz,
            "",
        );
    }
    {
        // **越界材质的比例钳位**（补判据：源于变异验证 M6 漏过）。
        //
        // `weighted_blocked_ratio` 末尾的 `clamp01` 在**合法材质下是死代码**
        // ——每个元区间贡献 `seg × (1 − 透声) ≤ seg`，而 `sum(seg) = 1`，
        // 故结果天然落在 `[0,1]`。既有判据全部喂合法材质，于是删掉这个
        // 钳位后46 条判据**全绿**（实测确认）。
        //
        // 但 `AcousticMaterial` 的字段是 `pub`，`weighted_blocked_ratio`
        // 的入参是调用方给的 `&[OcclusionSpan]`——**调用方可以绕过
        // `AcousticMaterial::new` 的校验**（构造器拦不住字面量）。
        // 一个 `transmission = −10` 的越界材质会让单段贡献
        // `1 × (1 − (−10)) = 11`，未钳位时比例直冲11.0。
        //
        // 这个钳位正是 `pub`纯函数**不依赖调用方先钳制**的最后一道防线
        // （判据写错数条纪律第8 条），故必须有判据钉住它的可观察行为。
        let bad = AcousticMaterial {
            id: 999,
            name: "越界材质",
            transmission: -10.0,
            absorption: 0.5,
            lowpass_hz: 500.0,
        };
        let spans = [OcclusionSpan {
            t0: 0.0,
            t1: 1.0,
            material: bad,
        }];
        let r = weighted_blocked_ratio(spans.as_slice());
        set.add(
            "H10-材质-越界材质比例被钳在区间",
            // 上界：钳位生效时恰为 1.0，未钳位时会是 11.0。
            r == 1.0
                // 下界：负透声（transmission > 1）方向同样不得越界。
                && weighted_blocked_ratio(
                    [OcclusionSpan {
                        t0: 0.0,
                        t1: 1.0,
                        material: AcousticMaterial {
                            id: 998,
                            name: "越界材质2",
                            transmission: 5.0,
                            absorption: 0.5,
                            lowpass_hz: 500.0,
                        },
                    }]
                    .as_slice(),
                ) == 0.0,
            "",
        );
    }
    {
        // 材质库归属声明须点名 F1431（防止本模块被误当作权威系数库）。
        let d = MATERIAL_AUTHORITY_DECLARATION;
        set.add(
            "H10-材质-权威库归属F1431已声明",
            d.contains("F1431") && d.contains("输入结构"),
            "",
        );
    }
    {
        // 材质系数自洽 + 查表一致；内置表四项id 互异。
        let all_valid = BUILTIN_MATERIALS.iter().all(|m| m.is_valid());
        let lookup_ok = material_by_id(1) == Some(MAT_CONCRETE)
            && material_by_id(4) == Some(MAT_CURTAIN)
            && material_by_id(999).is_none();
        // 布帘须比混凝土更透、截止更高（材质梯度的物理方向）。
        let ordered = MAT_CURTAIN.transmission > MAT_CONCRETE.transmission
            && MAT_CURTAIN.lowpass_hz > MAT_CONCRETE.lowpass_hz;
        set.add("H10-材质-内置表自洽且可查", all_valid && lookup_ok && ordered, "");
    }

    // ---- 判据：两档判定（遮挡比例阈值分档）----

    {
        // 夹逼对钉死PARTIAL_ENTER 界位置。中心值取自被测公开常量——
        // 若判据写死 0.15 而代码改成 0.2，本判据仍绿=隐性弱门禁。
        let e = TIER_EPS;
        let below = OcclusionGrade::of(PARTIAL_ENTER - e);
        let at = OcclusionGrade::of(PARTIAL_ENTER);
        let above = OcclusionGrade::of(PARTIAL_ENTER + e);
        set.add(
            "H10-两档-半挡阈值夹逼",
            below == OcclusionGrade::Clear
                && at == OcclusionGrade::Partial
                && above == OcclusionGrade::Partial,
            "",
        );
    }
    {
        // 夹逼对钉死 FULL_ENTER 界位置。
        let e = TIER_EPS;
        let below = OcclusionGrade::of(FULL_ENTER - e);
        let at = OcclusionGrade::of(FULL_ENTER);
        let above = OcclusionGrade::of(FULL_ENTER + e);
        set.add(
            "H10-两档-全挡阈值夹逼",
            below == OcclusionGrade::Partial
                && at == OcclusionGrade::Full
                && above == OcclusionGrade::Full,
            "",
        );
    }
    {
        // **两档的唯一区分特征是低通**：半遮挡档截止频率须**精确等于**开路值。
        //
        // 这是本单最强的一条判据。若两档共用一组参数、或半挡也低通，
        // "两档参数不等"这类弱判据仍绿，本条红。
        // 用精确等式而非双边阈值：半挡截止"略低一点点"（如19999Hz）在
        // 双边阈值下会被放过，但它已是可闻的高频损失。
        //
        // 几何取**全覆盖**（r=3.0 ⊃ 段长 4）：半覆盖时布帘的加权比例只有
        // `0.5 × 0.28 = 0.14 < PARTIAL_ENTER`，落在无遮挡档——那样测的就不是
        // "半遮挡"了。可测性要求先把用例构造成目标档位，再断档位特征。
        let mut g = SphereProxyGeometry::new();
        let _ = g.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 3.0, MAT_CURTAIN));
        let (a, b) = segment_z(4.0);
        let partial = classify(&g, a, b);
        let exact_open = (partial.target_cutoff_hz - OPEN_CUTOFF_HZ).abs() < CUTOFF_EPS_HZ;
        set.add(
            "H10-两档-半挡只衰减不低通",
            partial.grade == OcclusionGrade::Partial
                && exact_open
                && !partial.lowpass_active()
                // 半挡须**确有衰减**——否则"只衰减不低通"会被"什么都不做"骗过。
                && partial.target_gain < 1.0,
            "",
        );
    }
    {
        // 全阻挡档的截止频率须**严格小于**半挡档（单边符号，不用双边阈值）。
        // "全挡比半挡低通更多"若写成 `|diff| > 阈值`，方向写反（负差）也过；
        // 单边 `cutoff_full < cutoff_partial` 才有方向性。
        let mut half = SphereProxyGeometry::new();
        let _ = half.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 3.0, MAT_CURTAIN));
        let mut full = SphereProxyGeometry::new();
        let _ = full.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 3.0, MAT_CONCRETE));
        let (a, b) = segment_z(4.0);
        let vp = classify(&half, a, b);
        let vf = classify(&full, a, b);
        set.add(
            "H10-两档-全挡低通严于半挡",
            vp.grade == OcclusionGrade::Partial
                && vf.grade == OcclusionGrade::Full
                && vf.target_cutoff_hz < vp.target_cutoff_hz,
            "",
        );
    }
    {
        // 增益衰减律跨档统一且随遮挡比例单调不增（单边符号）。
        // 若两档各写一套衰减系数且半挡比全挡还响，本判据红。
        let mut g = SphereProxyGeometry::new();
        let _ = g.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 4.0, MAT_CONCRETE));
        let (a, b) = segment_z(4.0);
        let full = classify(&g, a, b);
        let clear = classify(&NullGeometry, a, b);
        set.add(
            "H10-两档-增益随遮挡单调不增",
            clear.target_gain == 1.0
                && full.target_gain < clear.target_gain
                && (full.target_gain - (1.0 - full.blocked_ratio)).abs() < 1.0e-4,
            "",
        );
    }
    {
        // `lowpass_active` 须与截止频率**一致**（派生而非并行标志）。
        // 直接断字段之间的关系，不信任标志位本身。
        let mut g = SphereProxyGeometry::new();
        let _ = g.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 3.0, MAT_CONCRETE));
        let (a, b) = segment_z(4.0);
        let v = classify(&g, a, b);
        let derived = v.target_cutoff_hz < OPEN_CUTOFF_HZ - CUTOFF_EPS_HZ;
        let off_by_one = v.lowpass_active() != derived;
        let mut consistent = !off_by_one;
        // 再扫一遍 Clear/Partial 两档，确保不是只对Full 档自洽。
        for geo in [
            classify(&NullGeometry, a, b),
            classify(&g, Cartesian::ORIGIN, Cartesian::new(0.0, 0.0, 100.0)),
        ] {
            let d2 = geo.target_cutoff_hz < OPEN_CUTOFF_HZ - CUTOFF_EPS_HZ;
            if geo.lowpass_active() != d2 {
                consistent = false;
            }
        }
        set.add("H10-两档-低通标志与参数一致", consistent, "");
    }
    {
        // 档位 wire 名互异且语义正确（判据可读性的机检，防两档写同一个名）。
        set.add(
            "H10-两档-档位命名互异",
            OcclusionGrade::Clear.wire() == "clear"
                && OcclusionGrade::Partial.wire() == "obstruction"
                && OcclusionGrade::Full.wire() == "occlusion",
            "",
        );
    }
    {
        // 非有限比例不得静默落进"无遮挡"（异常零静默：故障不能听起来正常）。
        // 约定按最严处理——本判据把该约定固化，防后续改动悄悄改成 Clear。
        let nan = OcclusionGrade::of(f32::NAN);
        let inf = OcclusionGrade::of(f32::INFINITY);
        set.add(
            "H10-两档-非有限比例按最严处理",
            nan == OcclusionGrade::Full && inf == OcclusionGrade::Full,
            "",
        );
    }

    // ---- 判据：平滑过渡（50ms 渐变 / 不跳变）----

    {
        // 端点定点：0ms 处**恰为起值**，50ms 处**恰为目标值**。
        // 断精确值而非"变了"——"先跳到目标再跳回"的实现也能让"变了"为真。
        let mut r = Ramp::settled(1.0);
        r.retarget(0.5);
        let at0 = r.value();
        for _ in 0..25 {
            r.advance(TRANSITION_MS / 50.0);
        }
        let at25 = r.value();
        for _ in 0..24 {
            r.advance(TRANSITION_MS / 50.0);
        }
        let at49 = r.value();
        r.advance(TRANSITION_MS / 50.0);
        let at50 = r.value();
        set.add(
            "H10-平滑-端点精确0与50",
            (at0 - 1.0).abs() < 1.0e-6
                // 25ms 恰在中点（端点判据顺带钉住，避免此处只测两端）。
                && (at25 - 0.75).abs() < 1.0e-5
                // 49ms 仍未到位：断"未到位"而非断某个魔数阈值。
                && (at49 - 0.5).abs() > 1.0e-6
                && (at50 - 0.5).abs() < 1.0e-6
                && r.is_settled(),
            "",
        );
    }
    {
        // 中点定点：25ms 处**恰为起止中点**（线性斜坡的定义性质）。
        // 用等式而非区间——"在起止之间"被非线性缓动曲线全绿通过。
        let mut r = Ramp::settled(1.0);
        r.retarget(0.0);
        r.advance(TRANSITION_MS / 2.0);
        let mid = r.value();
        let want = 0.5;
        set.add("H10-平滑-25ms恰为中点", (mid - want).abs() < 1.0e-5, "");
    }
    {
        // 中途改目标**不跳变**：改目标的瞬间参数值须与改前**完全相同**。
        // 若`retarget` 从旧目标起坡（而非当前值），改目标瞬间即跳到旧终点。
        let mut r = Ramp::settled(1.0);
        r.retarget(0.0);
        r.advance(TRANSITION_MS / 4.0);
        let before = r.value();
        r.retarget(1.0);
        let after = r.value();
        set.add(
            "H10-平滑-中途改目标不跳变",
            (before - after).abs() < 1.0e-6 && after < 1.0 && after > 0.0,
            "",
        );
    }
    {
        // 中途改目标后仍从**当前值**起坡。算术：
        //   起坡 1.0 → 目标 0.0，12.5ms（1/4）后当前值 = 1.0 − 0.25 = 0.75；
        //   改目标为 1.0（从当前值 0.75 起坡），再 25ms（1/2）后
        //   = 0.75 + (1.0 − 0.75) × 0.5 = **0.875**。
        //
        // 这一条与上一条构成对偶：上一条抓"瞬间跳变"，本条抓"起坡点取错"
        // ——若从**旧目标** 0.0 起坡，25ms 后是 0.5 而非 0.875。
        let mut r = Ramp::settled(1.0);
        r.retarget(0.0);
        r.advance(TRANSITION_MS / 4.0);
        r.retarget(1.0);
        r.advance(TRANSITION_MS / 2.0);
        let v = r.value();
        set.add("H10-平滑-改目标后从当前值起坡", (v - 0.875).abs() < 1.0e-5, "");
    }
    {
        // 全程不越界：密集采样下当前值恒落在起止两端之间（凸组合性质）。
        // `t` 未钳制的实现会在 `dt > 50ms` 时越界。
        let mut r = Ramp::settled(0.2);
        r.retarget(0.9);
        let mut ok = true;
        for i in 0..200 {
            r.advance(1.0);
            let v = r.value();
            if !(0.2 - 1.0e-5<= v && v <= 0.9 + 1.0e-5) {
                ok = false;
            }
            let _ = i;
        }
        // 一次超大 dt（远超过渡时长）也不得越界。
        r.advance(10_000.0);
        let v = r.value();
        set.add(
            "H10-平滑-全程不越界",
            ok && (v - 0.9).abs() < 1.0e-5,
            "",
        );
    }
    {
        // 超大 dt 直接到位（钳到过渡时长，不按 dt 比例外插）。
        let mut r = Ramp::settled(1.0);
        r.retarget(0.3);
        r.advance(10_000.0);
        set.add(
            "H10-平滑-超大tick直达目标",
            r.is_settled() && (r.value() - 0.3).abs() < 1.0e-6,
            "",
        );
    }
    {
        // 零/负/非有限 tick 不推进（时间倒流与 NaN 注入不得让参数回退）。
        //
        // **只断"值没变"是不够的**——本条判据的初版就栽在这里：`elapsed_ms`
        // 被负 tick 拉成负数后，因 `clamp01(t)` 在 0 处截断、`from` 又恰等于
        // t=0 处的值，于是参数**看起来完全没动**，判据全绿；而真实后果是
        // **时间膨胀**——此后需要 150ms 的合法 tick 才能到位（多花 100ms，
        // 恰好等于被倒灌回去的量）。渐变时长是本单的红线，故此处必须断
        // "非法 tick 之后，仍能在 50ms 合法时间内到位"。
        let mut r = Ramp::settled(1.0);
        r.retarget(0.4);
        r.advance(0.0);
        let a = r.value();
        r.advance(-100.0);
        let b = r.value();
        r.advance(f32::NAN);
        let c = r.value();
        let no_backtrack = (a - 1.0).abs() < 1.0e-6
            && (b - a).abs() < 1.0e-6
            && (c - a).abs() < 1.0e-6;
        // 时间膨胀探测：非法 tick 之后，从当前状态起仍须 50ms 到位。
        let mut r2 = Ramp::settled(1.0);
        r2.retarget(0.4);
        r2.advance(-100.0);
        for _ in 0..50 {
            r2.advance(1.0);
        }
        let no_dilation = r2.is_settled() && (r2.value() - 0.4).abs() < 1.0e-6;
        set.add(
            "H10-平滑-非法tick不推进",
            no_backtrack && no_dilation,
            "",
        );
    }
    {
        // 目标未变时不重启斜坡（否则每次 apply 都会把进度清零，永远到不了位）。
        let mut r = Ramp::settled(1.0);
        r.retarget(0.5);
        r.advance(TRANSITION_MS / 2.0);
        let before = r.value();
        r.retarget(0.5);
        let after = r.value();
        r.advance(TRANSITION_MS / 2.0);
        set.add(
            "H10-平滑-同目标不重启",
            (before - after).abs() < 1.0e-6 && r.is_settled(),
            "",
        );
    }
    {
        // 非有限目标被拒（不被NaN 污染斜坡状态）。
        let mut r = Ramp::settled(1.0);
        r.retarget(f32::NAN);
        let v = r.value();
        set.add(
            "H10-平滑-非有限目标被拒",
            v.is_finite() && (v - 1.0).abs() < 1.0e-6 && r.target() == 1.0,
            "",
        );
    }
    {
        // 端到端：半遮挡→全遮挡的**切换**过程滤波参数连续（逐 tick 单调，
        //且任一 tick 的步进不超过 50ms 线性斜坡的解析期望）。
        //
        // 这里断的是"步进幅度有界"而非"总时长内到位"——若平滑被整体跳过，
        // 第一 tick 就会跳到目标，步进幅度超界。
        let mut sm = OcclusionSmoother::new_open();
        let mut half = SphereProxyGeometry::new();
        let _ = half.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 1.0, MAT_CURTAIN));
        let mut full = SphereProxyGeometry::new();
        let _ = full.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 3.0, MAT_CONCRETE));
        let (a, b) = segment_z(4.0);
        sm.apply(&classify(&half, a, b));
        for _ in 0..50 {
            sm.advance(1.0);
        }
        let settled_half = sm.current();
        let target_cut = classify(&full, a, b).target_cutoff_hz;
        sm.apply(&classify(&full, a, b));
        let first_tick = sm.current();
        let mut max_step: f32 = 0.0;
        let mut prev = first_tick.cutoff_hz;
        let mut monotone = true;
        for _ in 0..49 {
            sm.advance(1.0);
            let c = sm.current();
            if c.cutoff_hz > prev + 1.0e-4 {
                monotone = false;
            }
            let step = (c.cutoff_hz - prev).abs();
            if step > max_step {
                max_step = step;
            }
            prev = c.cutoff_hz;
        }
        // 1ms 步进的解析上限 = |目标−起值|/50。
        let analytic_step =
            (target_cut - settled_half.cutoff_hz).abs() / TRANSITION_MS;
        set.add(
            "H10-平滑-切换连续且步进有界",
            monotone
                && max_step <= analytic_step * 1.5 + 1.0e-3
                && !sm.is_settled()
                && (sm.current().cutoff_hz - target_cut).abs() > CUTOFF_EPS_HZ,
            "",
        );
    }
    {
        // 半挡稳态下低通**不得**生效（端到端复核平滑器路径，而非只看 verdict）。
        // 平滑器若把两档参数混用，此处 `lowpass_active` 会为真。
        // 几何取全覆盖（r=3.0）以确保真落在半挡档（见上一条判据的说明）。
        let mut sm = OcclusionSmoother::new_open();
        let mut half = SphereProxyGeometry::new();
        let _ = half.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 3.0, MAT_CURTAIN));
        let (a, b) = segment_z(4.0);
        sm.apply(&classify(&half, a, b));
        for _ in 0..100 {
            sm.advance(1.0);
        }
        let c = sm.current();
        set.add(
            "H10-平滑-半挡稳态不开低通",
            sm.is_settled()
                && !c.lowpass_active
                && (c.cutoff_hz - OPEN_CUTOFF_HZ).abs() < CUTOFF_EPS_HZ
                && c.gain < 1.0,
            "",
        );
    }
    {
        // 平滑器与 ramp 的关系须自洽（current().gain 恒等于 gain ramp 的值）。
        // 绕过聚合层直接断言——两个断言点若同源于一个已缓存的字段，
        // 改任一边都抓不到。
        let mut sm = OcclusionSmoother::new_open();
        sm.apply(&OcclusionVerdict {
            grade: OcclusionGrade::Full,
            blocked_ratio: 0.9,
            target_gain: 0.1,
            target_cutoff_hz: 700.0,
        });
        for _ in 0..25 {
            sm.advance(1.0);
        }
        let c = sm.current();
        let want = 1.0 + (0.1 - 1.0) * 0.5;
        set.add(
            "H10-平滑-参数插值自洽",
            (c.gain - want).abs() < 1.0e-5
                && (c.cutoff_hz - (20_000.0 + (700.0 - 20_000.0) * 0.5)).abs() < 1.0e-3,
            "",
        );
    }
    {
        // 开路稳态初值须为无遮挡参数（构造函数不是"随便置零"）。
        let sm = OcclusionSmoother::new_open();
        let c = sm.current();
        set.add(
            "H10-平滑-开路初值为无遮挡",
            sm.is_settled()
                && c.gain == 1.0
                && c.cutoff_hz == OPEN_CUTOFF_HZ
                && !c.lowpass_active,
            "",
        );
    }

    // ---- 判据：分层与能力自述 ----

    {
        // 分层声明须点名 F1329（滤波核不在本模块）。
        set.add(
            "H10-分层-滤波核归F1329已声明",
            LAYERING_DECLARATION.contains("F1329") && LAYERING_DECLARATION.contains("参数"),
            "",
        );
    }
    {
        // 能力自述须覆盖四段（几何/分档/平滑/材质），防"接口预留"只写在注释里。
        let d = capability_declaration();
        set.add(
            "H10-分层-能力自述覆盖四段",
            d.contains("OcclusionGeometry")
                && d.contains("F1431")
                && d.contains("F1329")
                && d.contains("50ms"),
            "",
        );
    }
    {
        // 模块 fsqrt 自足：对 0/负返回 0，对典型值收敛到真值。
        // 用独立参考（(1.0+9.0)/4.0=2.5）而非 `f32::sqrt`——后者在部分
        // no_std 目标上不可用，且用它会与被测实现同源。
        let ok0 = fsqrt(0.0) == 0.0;
        let okneg = fsqrt(-1.0) == 0.0;
        let ok25 = (fsqrt(6.25) - 2.5).abs() < 1.0e-4;
        let okodd = (fsqrt(2.0) - 1.41421356).abs() < 1.0e-4;
        set.add("H10-分层-开方自足且收敛", ok0 && okneg && ok25 && okodd, "");
    }
    {
        // 边界算术无 panic：非有限区间端点被跳过而非参与运算。
        let spans = vec![
            OcclusionSpan {
                t0: f32::NAN,
                t1: 1.0,
                material: MAT_CONCRETE,
            },
            OcclusionSpan {
                t0: 0.0,
                t1: f32::INFINITY,
                material: MAT_CURTAIN,
            },
        ];
        let r = weighted_blocked_ratio(spans.as_slice());
        set.add(
            "H10-分层-非法区间端点不致panic",
            r.is_finite() && (0.0..=1.0).contains(&r),
            "",
        );
    }
    {
        // 材质构造期校验（负透声/超吸声/零截止频率一律拒）。
        let neg = AcousticMaterial::new(90,"负透声", -0.1, 0.1, 700.0).is_err();
        let over = AcousticMaterial::new(91, "超吸声", 0.1, 1.5, 700.0).is_err();
        let zero_lp = AcousticMaterial::new(92, "零截止", 0.1, 0.1, 0.0).is_err();
        let ok_m = AcousticMaterial::new(93, "合法", 0.1, 0.1, 700.0).is_ok();
        set.add(
            "H10-分层-材质构造校验",
            neg && over && zero_lp && ok_m,
            "",
        );
    }
    {
        // 冗余但有价值的对账：`weighted_blocked_ratio` 与端到端 `classify`
        // 的比例字段必须一致（同源字段不得被聚合层改写）。
        let mut g = SphereProxyGeometry::new();
        let _ = g.add(sphere_with(Cartesian::new(0.0, 0.0, 2.0), 1.5, MAT_WOOD));
        let (a, b) = segment_z(4.0);
        let spans = g.collect_spans(a, b);
        let direct = weighted_blocked_ratio(spans.as_slice());
        let via = classify(&g, a, b).blocked_ratio;
        set.add(
            "H10-分层-端到端与直接加权一致",
            (direct - via).abs() < 1.0e-6,
            "",
        );
    }

    set
}

/// 反假变体登记表（变异测试实测结果，`run_veh10_checks` 不读它，仅作文档）。
///
/// **为什么必须登记**：判据全绿只证明"当前实现合判据"。若某条判据在原理上
/// 无法被某类缺陷转红，它就是弱门禁。本表记录每个变异**实际捕获的判据**，
/// 任何人改判据时可用同一变异集复核——若某变异改完仍全绿，说明该判据退化
/// 成了恒真。
///
/// 本轮实测（隔离探针，基线 48/48 绿）：**10/10 变异全部被捕获**。
///
/// | 变体 | 施加的缺陷 | 实际转红的判据 |
/// |---|---|---|
/// | M1 | 分档阈值判断倒置（`>=FULL` 与 `>=PARTIAL` 互换） | 两档阈值夹逼×2、半挡只衰减不低通、全挡低通严于半挡、半挡稳态不开低通、混凝土与布帘分档不同（6） |
/// | M2 | `applies_lowpass` 改 `!=Clear`（半挡也开低通） | 半挡只衰减不低通、半挡稳态不开低通（2） |
/// | M3 | 元区间取**最阻**路径（`transmission <`） | 并存取最通路径（1） |
/// | M4 | 未覆盖元区间贡献 `seg`（当全遮挡） | 单球比例独立重算（1） |
/// | M5 | 斜坡 `t` 恒 1（瞬时到位，无渐变） | 平滑 6 条（端点、中点、中途改目标、切换连续、参数插值、改目标起坡） |
/// | M6 | 到位判定放宽到半程 | 切换连续且步进有界（1） |
/// | M7 | 斜坡分母减半（速率翻倍） | 平滑 5 条 |
/// | M8 | 元区间**逐段相加**（重叠球双计） | 重叠球不双计、并存取最通路径（2） |
/// | M9 | 非有限比例静默落 `Clear` | 非有限比例按最严处理（1） |
/// | M10 | 阈值闭区间改开区间（`>` 替 `>=`） | 全挡阈值夹逼（1） |
///
/// **注意 M3/M4/M8 各只转红 1–2 条**——这是**符合预期的低冗余度**，不是弱门禁：
/// 三者分别对应"最通路径""未覆盖贡献 0""并集不双计"三条互不重叠的语义，
/// 每条都有专属判据钉住（十诫第 3 条：重合行为掩盖缺失分支 ⇒ 必须有专属错误码
/// 与专属判据）。若某天这三者中任一条转红数为 0，即说明专属判据失效。
pub const VARIANT_REGISTRY: [(&str, &str); 10] = [
    ("M1-tier-threshold-swapped", "H10-两档-半挡阈值夹逼/H10-两档-全挡阈值夹逼"),
    ("M2-lowpass-both-tiers", "H10-两档-半挡只衰减不低通"),
    ("M3-most-opaque-not-openest", "H10-材质-并存取最通路径"),
    ("M4-uncovered-counts-as-blocked", "H10-材质-单球比例独立重算"),
    ("M5-ramp-instant", "H10-平滑-25ms恰为中点"),
    ("M6-settled-at-half", "H10-平滑-切换连续且步进有界"),
    ("M7-ramp-rate-doubled", "H10-平滑-端点精确0与50"),
    ("M8-per-span-sum-double-count", "H10-材质-重叠球不双计"),
    ("M9-nonfinite-silently-clear", "H10-两档-非有限比例按最严处理"),
    ("M10-half-open-interval", "H10-两档-全挡阈值夹逼"),
];

/// 构造带指定材质的合法遮挡球（材质非法时退回混凝土，仅供判据组装）。
fn sphere_with(center: Cartesian, radius_m: f32, material: AcousticMaterial) -> OccluderSphere {
    match OccluderSphere::new(center, radius_m, material) {
        Ok(s) => s,
        Err(_) => OccluderSphere {
            center,
            radius_m: if radius_m > 0.0 { radius_m } else { 1.0 },
            material: MAT_CONCRETE,
        },
    }
}

/// 变体测试辅助：把一段 `Vec<OcclusionSpan>` 的端点按比例缩放（生成镜像变体）。
#[allow(dead_code)]
fn scaled_spans(spans: &[OcclusionSpan], k: f32) -> Vec<OcclusionSpan> {
    let mut out: Vec<OcclusionSpan> = Vec::new();
    for s in spans.iter() {
        out.push(OcclusionSpan {
            t0: clamp_unit(s.t0 * k),
            t1: clamp_unit(s.t1 * k),
            material: s.material,
        });
    }
    out
}

fn clamp_unit(v: f32) -> f32 {
    if !(v.is_finite()) {
        return 0.0;
    }
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

/// 供变异验证使用的格式化诊断（红项定位用，聚合器不调用）。
#[allow(dead_code)]
fn describe(geo: &SphereProxyGeometry, a: Cartesian, b: Cartesian) -> String {
    format!(
        "spans={} blocked={:.6} grade={:?}",
        geo.len(),
        classify(geo, a, b).blocked_ratio,
        classify(geo, a, b).grade
    )
}
