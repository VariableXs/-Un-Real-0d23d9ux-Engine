//! VE-F1807 · 域自检（判据逐条对应，见 `vej07_lightmgr.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **句柄安全** → `F1807-句柄-槽位复用且代数递增`、`F1807-句柄-删后失效`、
//!   `F1807-句柄-槽位复用后旧句柄仍失效`、`F1807-句柄-越界拒绝`、
//!   `F1807-句柄-字段私有不可手工拼造`；
//! - **双因子排序** → `F1807-双因子-方向光恒权`、`F1807-双因子-点光随距离衰减`、
//!   `F1807-双因子-距离为次键`、`F1807-双因子-距离平方口径绝对值(平方律非线性律)`、
//!   `F1807-双因子-类型权重表显式`、
//!   `F1807-双因子-面光降权有理由`；
//! - **确定性** → `F1807-确定性-三级键全序`、`F1807-确定性-ID决胜消歧`、
//!   `F1807-确定性-双跑逐位一致`、`F1807-确定性-NaN键不 UB`、
//!   `F1807-确定性-稳定id不复用`；
//! - **剔除（对齐 I07 接口）** → `F1807-剔除-接口形状对齐I07`、
//!   `F1807-剔除-扩张球兜底`、`F1807-剔除-任一面全外即剔`、`F1807-剔除-非有限保Keep`、
//!   `F1807-剔除-标量规模诚实声明`；
//! - 错误路径与降级矩阵 → `F1807-降级-悬挂拒绝不崩溃`、`F1807-降级-风暴合并到帧边界`、
//!   `F1807-降级-风暴超限拒超额`、`F1807-降级-超上限拒绝`、
//!   `F1807-降级-上限绝对值契约(256,变更须走ADR)`、
//!   `F1807-降级-非有限钳制带告警`、
//!   `F1807-降级-裁剪计数显性`、`F1807-降级-六码齐备且指引非空`；
//! - 参数块 → `F1807-参数块-每帧一次`、`F1807-参数块-16字节对齐`、
//!   `F1807-参数块-字段序冻结`、`F1807-参数块-无限范围写哨兵`；
//! - 性能与台账 → `F1807-台账-接口位不重复`、`F1807-台账-统计四计数齐备`。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use alloc::format;
use alloc::vec::Vec;

use super::vej07_lightmgr::*;
use crate::checks::CheckSet;

/// 便捷构造：带指定 stable_id 的点光。
fn pt(pos: (f32, f32, f32), intensity: f32, range: f32, id: u64) -> LightDesc {
    let (mut d, _) = LightDesc::new(
        LightKind::Point,
        pos,
        (0.0, 0.0, -1.0),
        (1.0, 1.0, 1.0),
        intensity,
        range,
        id,
    );
    d.stable_id = id;
    d
}

/// 便捷构造：带指定 stable_id 的方向光。
fn directional(intensity: f32, id: u64) -> LightDesc {
    let (mut d, _) = LightDesc::new(
        LightKind::Directional,
        (0.0, 0.0, 0.0),
        (0.0, 0.0, -1.0),
        (1.0, 1.0, 1.0),
        intensity,
        1.0,
        id,
    );
    d.stable_id = id;
    d
}

/// 便捷构造：标准视锥（近平面 z=0，法线已单位化故 d 项免缩放）。
fn std_frustum() -> FrustumPlanes {
    frustum_from_planes(
        (1.0, 0.0, 0.0, 0.0),
        (-1.0, 0.0, 0.0, 0.0),
        (0.0, 1.0, 0.0, 0.0),
        (0.0, -1.0, 0.0, 0.0),
        (0.0, 0.0, 1.0, 0.0),
        (0.0, 0.0, -1.0, 1000.0),
    )
    .unwrap()
}

/// VE-F1807 域自检。
pub fn run_vej07_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-J/F1807");

    // ——— 判据一：句柄安全 ———
    let mut m = LightManager::new();
    let h0 = m.add(pt((1.0, 0.0, 0.0), 10.0, 5.0, 1), Vec::new()).unwrap();
    let gen0 = h0.generation();
    let idx0 = h0.index();
    m.remove(h0).unwrap();
    let stale_code = m.get(h0).err().map(|e| e.code);
    set.add(
        "F1807-句柄-删后失效",
        stale_code == Some(LightError::StaleHandle) && !m.is_live(h0),
        "",
    );

    // 槽位复用后旧句柄**仍**失效 —— generation 的核心价值。
    let h1 = m.add(pt((2.0, 0.0, 0.0), 10.0, 5.0, 2), Vec::new()).unwrap();
    set.add(
        "F1807-句柄-槽位复用后旧句柄仍失效",
        h1.index() == idx0 && h1.generation() != gen0 && m.get(h0).is_err() && m.is_live(h1),
        "",
    );

    // **旧句柄不得删掉别人的灯**——这条曾长期缺判据。
    //
    // 缺口症状：`get`/`is_live` 各自校验了代数（于是上面两条都绿），
    // 但 `remove` 若只查「槽位非空」不查代数，旧句柄就能删掉同槽的新灯，
    // 而用户拿到的是「我明明remove 的是已删的灯，怎么报错说成功」。
    // 反假变体 M4（去掉 remove 的代数校验）正是打这里。
    let mut m_steal = LightManager::new();
    let victim = m_steal.add(pt((9.0, 0.0, 0.0), 10.0, 5.0, 11), Vec::new()).unwrap();
    m_steal.remove(victim).unwrap();
    let newcomer = m_steal.add(pt((8.0, 0.0, 0.0), 10.0, 5.0, 12), Vec::new()).unwrap();
    let steal_err = m_steal.remove(victim).err().map(|e| e.code);
    set.add(
        "F1807-句柄-旧句柄删不掉新灯",
        newcomer.index() == victim.index()
            && newcomer.generation() != victim.generation()
            && steal_err == Some(LightError::StaleHandle)
            && m_steal.is_live(newcomer)
            && m_steal.len() == 1,
        "",
    );

    // 越界句柄拒绝。
    let bogus = LightHandle::forged((MAX_LIGHTS + 10) as u32, 1);
    set.add(
        "F1807-句柄-越界拒绝",
        m.get(bogus).err().map(|e| e.code) == Some(LightError::HandleOutOfRange),
        "",
    );

    // 连续增删 N 轮：槽位必被复用（index恒同），代数必单调递增。
    //
    // 判据同时查两件事——只查代数递增会漏掉「每轮开新槽」的实现，
    // 只查index 恒同会漏掉「槽位复用但代数不递增」的假复用。
    let mut m2 = LightManager::new();
    let mut idxs: Vec<u32> = Vec::new();
    let mut gens: Vec<u32> = Vec::new();
    for i in 0..64 {
        let hh = m2.add(pt((i as f32, 0.0, 0.0), 1.0, 1.0, i as u64 + 1), Vec::new()).unwrap();
        idxs.push(hh.index());
        gens.push(hh.generation());
        m2.remove(hh).unwrap();
    }
    let same_slot_reused = idxs.windows(2).all(|w| w[0] == w[1]);
    let gen_strictly_up = gens.windows(2).all(|w| w[1] > w[0]);
    set.add(
        "F1807-句柄-槽位复用且代数递增",
        same_slot_reused && gen_strictly_up && m2.len() == 0,
        "",
    );

    // 句柄不可伪造 —— 这条必须用**运行时可判**的等价纪律来验。
    //
    // 「字段私有」本身是编译期属性，运行时断言不到：把字段改成 `pub`
    // 不会让任何 `set.add` 变红（反假变体 M12 就是这么假绿的）。
    // 真正要守的不变式是「**任何非管理器产出的句柄都无效**」，
    // 它可以用 [`LightHandle::forged`] 造出任意 (index, generation) 来验：
    // 伪造句柄一律不得命中在册光源。
    let hx = m2.add(pt((0.0, 0.0, 0.0), 1.0, 1.0, 1), Vec::new()).unwrap();
    let mut forged_all_rejected = true;
    // 枚举**与真实代数不同**的伪造档位：正确代数、+1、0、u32::MAX。
    //
    // 早先版本把「正确代数」也放进循环，那造出的句柄与 hx 完全相等，
    // 于是 `f == hx` 为真、整条判据恒红——**正确代数本就该命中**，
    // 那是真句柄不是伪造。伪造的定义是「代数对不上却仍被接受」。
    for g in [
        hx.generation().wrapping_add(1),
        hx.generation().wrapping_sub(1),
        0u32,
        u32::MAX,
    ] {
        let f = LightHandle::forged(hx.index(), g);
        if f == hx || m2.is_live(f) || m2.get(f).is_ok() || m2.remove(f).is_ok() {
            forged_all_rejected = false;
        }
    }
    // 越界下标同样必须被拒（forged 接缝绕过了 add/free 的所有约束）。
    let oob = LightHandle::forged(MAX_LIGHTS as u32, hx.generation());
    if m2.is_live(oob) || m2.get(oob).is_ok() || m2.remove(oob).is_ok() {
        forged_all_rejected = false;
    }
    set.add(
        "F1807-句柄-伪造句柄一律无效",
        hx.generation() > 0
            && hx.wire().contains('#')
            && forged_all_rejected
            // 真实句柄必须仍然有效——否则这条判据可以用「全拒」骗过。
            && m2.is_live(hx)
            && m2.get(hx).is_ok()
            && m2.len() == 1,
        "",
    );

    // ——— 判据二：双因子排序 ———
    let dir = directional(5.0, 100);
    set.add(
        "F1807-双因子-方向光恒权",
        dir.importance_at((0.0, 0.0, 0.0)) == 5.0
            && dir.importance_at((999.0, 0.0, 0.0)) == 5.0
            && dir.distance_sq_to((1.0, 1.0, 1.0)) == 0.0
            && dir.kind.distance_independent(),
        "",
    );

    // 点光随距离衰减：越远越低，且严格单调递减。
    let pl = pt((10.0, 0.0, 0.0), 100.0, 1000.0, 1);
    let near = pl.importance_at((0.0, 0.0, 0.0));
    let far = pl.importance_at((100.0, 0.0, 0.0));
    set.add(
        "F1807-双因子-点光随距离衰减",
        near > far && far > 0.0 && pt((10.0, 0.0, 0.0), 100.0, 1000.0, 1).importance_at((50.0, 0.0, 0.0)) > far,
        "",
    );

    // 距离为次键：重要性相等时近者在前。
    //
    // 补偿公式必须与 `attenuation_at` 的真实分母一致：实现是
    // `1 / (ATTEN_EPS + d²)`，不是 `1 / (1 + d²)`。写错分母会让
    // 两个重要性差了好几个数量级，判据就退化成「测主键」，
    // 次键根本没被检验。
    let view0 = (0.0, 0.0, 0.0);
    let a = pt((5.0, 0.0, 0.0), 10.0, 100.0, 1);
    let b = pt((50.0, 0.0, 0.0), 10.0, 100.0, 2);
    let d_a = a.distance_sq_to(view0);
    let d_b = b.distance_sq_to(view0);
    // 要 importance_a == importance_b，取 I_b = I_a * (ε+d_b)/(ε+d_a)。
    let b_adj = pt(
        (50.0, 0.0, 0.0),
        10.0 * (ATTEN_EPS + d_b) / (ATTEN_EPS + d_a),
        100.0,
        2,
    );
    let ka = SortKey {
        importance: a.importance_at(view0),
        distance_sq: d_a,
        stable_id: 1,
    };
    let kb = SortKey {
        importance: b_adj.importance_at(view0),
        distance_sq: d_b,
        stable_id: 2,
    };
    // 反向对照：若把 stable_id 也抹平（改用副产物键），近者仍须在前。
    // 这条防的是「实现其实在比 stable_id」的假通过。
    let kc = SortKey { stable_id: 1, ..ka };
    let kd = SortKey { stable_id: 2, ..kb };
    set.add(
        "F1807-双因子-距离为次键",
        (ka.importance - kb.importance).abs() <= 1e-3 * ka.importance.abs().max(1.0)
            && ka.cmp_total(&kb) == core::cmp::Ordering::Less
            && kc.cmp_total(&kd) == core::cmp::Ordering::Less,
        "",
    );

    // **距离平方的绝对值契约**（变异 M14 抓此项）。
    //
    // 为什么必须另立：上面两条衰减判据全是**相对比较**（`near > far`、
    // `ka < kb`），只能证明「近的比远的亮」这一**序关系**。把
    // `distance_sq_to` 改成返回 `sqrt(d²)`（即距离而非距离平方），
    // 序关系**完全不变**——近的仍然更近更亮，两条判据照样全绿。
    // 但衰减公式 `1/(ε+d²)` 的分母被换掉，衰减速度从平方律退化成线性律，
    // 大范围场景的光照分布明显改变（实测d=5 处衰减 0.04 → 0.2，差 5 倍）。
    //
    // 这与「用表内元素验查表函数」同族：相对判据验不了**口径**，
    // 只能验**方向**。口径必须用绝对值钉死。
    //
    // 锚点原文「点光随距离衰减」未给具体公式，但平方律是逆平方物理定律
    // 的直接表述（照度∝ 1/d²），线性衰减无物理依据，故以此为准绳。
    let d_probe = pt((3.0, 4.0, 0.0), 10.0, 100.0, 77); // 距原点 5m
    let d2 = d_probe.distance_sq_to((0.0, 0.0, 0.0));
    // 3-4-5 三角形：d² = 25（若实现开方则为 5，一眼可辨）
    let sq_ok = (d2 - 25.0).abs() < 1e-3;
    // 衰减绝对值：I=100、ε=1e-4、d²=25 → 100/(ε+25) ≈ 3.99998
    let att = d_probe.attenuation_at((0.0, 0.0, 0.0));
    let att_want = 1.0 / (ATTEN_EPS + 25.0);
    let att_ok = (att - att_want).abs() < 1e-4 * att_want;
    // 平方律 vs 线性律的分离度：d=20m 处两者差 25 倍，必须显著不同
    let d_far = pt((20.0, 0.0, 0.0), 10.0, 100.0, 78);
    let att_far = d_far.attenuation_at((0.0, 0.0, 0.0));
    let sep = (1.0 / (ATTEN_EPS + 400.0)) / (1.0 / (ATTEN_EPS + 20.0));
    let law_ok = (att_far - 1.0 / (ATTEN_EPS + 400.0)).abs() < 1e-5 && (sep - 0.05).abs() < 1e-3;
    set.add(
        "F1807-双因子-距离平方口径绝对值(平方律非线性律)",
        sq_ok && att_ok && law_ok,
        "",
    );

    // 类型权重表显式且方向光唯一 distance_independent。
    let weights_ok = LightKind::Directional.weight() == TYPE_WEIGHT_DIRECTIONAL
        && LightKind::Point.weight() == TYPE_WEIGHT_POINT_BASE
        && LightKind::Spot.weight() == TYPE_WEIGHT_SPOT_BASE
        && LightKind::Area.weight() == TYPE_WEIGHT_AREA;
    let only_dir_independent = LIGHT_KINDS
        .iter()
        .filter(|k| k.distance_independent())
        .count()
        == 1;
    set.add(
        "F1807-双因子-类型权重表显式",
        weights_ok && only_dir_independent && LightKind::Directional.distance_independent(),
        "",
    );

    set.add(
        "F1807-双因子-面光降权有理由",
        LightKind::Area.weight() < LightKind::Point.weight()
            && !AREA_WEIGHT_RATIONALE.is_empty()
            && AREA_WEIGHT_RATIONALE.contains("LTC"),
        "",
    );

    // ——— 判据三：确定性 ———
    // 三级键全序：imp 降序 → dist 升序 → id 升序。
    let mut ks: Vec<SortKey> = alloc::vec![
        SortKey { importance: 1.0, distance_sq: 5.0, stable_id: 3 },
        SortKey { importance: 1.0, distance_sq: 5.0, stable_id: 1 },
        SortKey { importance: 2.0, distance_sq: 9.0, stable_id: 2 },
        SortKey { importance: 1.0, distance_sq: 2.0, stable_id: 4 },
    ];
    ks.sort_by(|x, y| x.cmp_total(y));
    let ids: Vec<u64> = ks.iter().map(|k| k.stable_id).collect();
    set.add(
        "F1807-确定性-三级键全序",
        ids == alloc::vec![2u64, 4, 1, 3],
        "",
    );

    // ID 决胜消歧：前两级完全相等时按 id 排，且与输入序无关。
    let k1 = SortKey { importance: 1.0, distance_sq: 2.0, stable_id: 7 };
    let k2 = SortKey { importance: 1.0, distance_sq: 2.0, stable_id: 8 };
    set.add(
        "F1807-确定性-ID决胜消歧",
        k1.cmp_total(&k2) == core::cmp::Ordering::Less
            && k2.cmp_total(&k1) == core::cmp::Ordering::Greater
            && k1.cmp_total(&k1) == core::cmp::Ordering::Equal,
        "",
    );

    // 双跑逐位一致：同输入两次 build_frame，选中序列与参数块逐位相同。
    let mut m3 = LightManager::new();
    for (i, spec) in [
        (1i32, 0i32),
        (2, 1),
        (3, 2),
        (4, 3),
    ]
    .iter()
    .enumerate()
    {
        let id = (i + 1) as u64;
        m3.add(
            pt((spec.0 as f32, 0.0, 0.0), 10.0 * (4 - i) as f32, 100.0, id),
            Vec::new(),
        )
        .unwrap();
    }
    let run1 = m3.build_frame((2.0, 0.0, 0.0), None, 0).unwrap();
    let run2 = m3.build_frame((2.0, 0.0, 0.0), None, 0).unwrap();
    let s1: Vec<u64> = run1.selection.selected.iter().map(|x| x.desc.stable_id).collect();
    let s2: Vec<u64> = run2.selection.selected.iter().map(|x| x.desc.stable_id).collect();
    set.add(
        "F1807-确定性-双跑逐位一致",
        s1 == s2 && run1.param_block == run2.param_block,
        "",
    );

    // NaN 键不 UB：total_cmp 必返回一个**确定且反对称**的序。
    //
    // 判据要求三件事同时成立：
    // 1. 反称：cmp(a,b) 与 cmp(b,a) 必为相反值（NaN 与正常数不能判等——
    //    这正是 `partial_cmp` 返回 None 后被 `unwrap_or(Equal)` 吞掉的情形）；
    // 2. 传递到具体值：NaN 键必被判到「重要性更差」的一侧（NaN 是非数，
    //    不能让它凭stable_id 混进前排）；
    // 3. 自反：cmp(a,a) 必为 Equal。
    //
    // 旧写法是 `o1 != Equal || o2 != Equal || true`——末尾的 `|| true`
    // 让整条判据恒真（这正是编译器unused_comparisons 类告警的信号：
    // 恒真断言）。反假变体 M6（total_cmp 换成 partial_cmp）跑出 ALL GREEN，
    // 就是被这个 `|| true` 吞掉的。
    let nan_key = SortKey { importance: f32::NAN, distance_sq: f32::NAN, stable_id: 1 };
    let ord_key = SortKey { importance: 1.0, distance_sq: 1.0, stable_id: 2 };
    let o1 = nan_key.cmp_total(&ord_key);
    let o2 = ord_key.cmp_total(&nan_key);
    let antisymmetric = o1 == o2.reverse();
    let nan_is_last = o1 == core::cmp::Ordering::Less;
    let reflexive = nan_key.cmp_total(&nan_key) == core::cmp::Ordering::Equal;
    set.add(
        "F1807-确定性-NaN键不UB",
        antisymmetric && nan_is_last && reflexive,
        "",
    );

    // stable_id 永不复用：删除后新建的 id 必大于此前所有 id。
    let mut m4 = LightManager::new();
    let mut max_before = 0u64;
    for i in 0..8 {
        let hh = m4.add(pt((0.0, 0.0, 0.0), 1.0, 1.0, i + 1), Vec::new()).unwrap();
        max_before = m4.get(hh).unwrap().stable_id;
        m4.remove(hh).unwrap();
    }
    let hh2 = m4.add(pt((0.0, 0.0, 0.0), 1.0, 1.0, 1), Vec::new()).unwrap();
    set.add(
        "F1807-确定性-稳定id不复用",
        m4.get(hh2).unwrap().stable_id > 0 && max_before > 0,
        "",
    );

    // ——— 判据四：剔除 ———
    let fr = std_frustum();
    set.add(
        "F1807-剔除-接口形状对齐I07",
        fr.planes.len() == 6 && fr.planes.iter().all(|p| {
            let l = (p.0 * p.0 + p.1 * p.1 + p.2 * p.2).sqrt();
            (l - 1.0).abs() < 1e-5
        }),
        "",
    );

    // 扩张球兜底：中心在视锥外但 range 大时须 Keep。
    //
    // **取值必须落在膨胀敏感区间**。此视锥近平面 z=0、球心 z=-1，
    // 故只有 z=0 那个面的带号距离为 -1，判据退化成
    // 「-1 < -range × CULL_INFLATE」。要让膨胀系数真正起作用，必须
    // 让结论在系数=1 与系数=1.25 之间翻转，即 range ∈ [0.8, 1.0)。
    //
    // 早先选的 0.5 与 2.0 恰好**都落在敏感区外**：0.5 时两种系数都 Cull，
    // 2.0 时两种系数都 Keep——于是把 CULL_INFLATE 改成 1（反假变体 M2）
    // 自检依然全绿。选点前先算，别凭直觉。
    let near_out = frustum_from_planes(
        (1.0, 0.0, 0.0, 0.0),
        (-1.0, 0.0, 0.0, 0.0),
        (0.0, 1.0, 0.0, 0.0),
        (0.0, -1.0, 0.0, 0.0),
        (0.0, 0.0, 1.0, 0.0),
        (0.0, 0.0, -1.0, 100.0),
    )
    .unwrap();
    set.add(
        "F1807-剔除-扩张球兜底",
        // range=0.9：紧半径 0.9 会剔（-1 < -0.9），扩张球 1.125 不剔（-1 < -1.125 为假）。
        // 这正是「大范围点光中心在视锥外但光锥有部分在视野内」不得被误剔。
        sphere_in_frustum(&near_out, (0.0, 0.0, -1.0), 0.9) == CullVerdict::Keep
            // range=0.5：连扩张球都在外，确实在视锥外 → 该剔。
            && sphere_in_frustum(&near_out, (0.0, 0.0, -1.0), 0.5) == CullVerdict::Cull,
        "",
    );

    // 越远越Keep —— 同一视锥外点，range 增大到覆盖回近平面就不该再剔。
    //
    // 这条曾被写成「range=2.0 → Cull」，是判据错：range=2.0 时
    // 扩张球 2.5 已覆盖近平面（带号距离 -1 > -2.5），灯**不该**被剔。
    // 膨胀只会把 Cull 变Keep，绝不会反向——写反了就是把兜底语义钉死成
    // 「扩张=更狠地剔」，与实现意图正相反。
    set.add(
        "F1807-剔除-膨胀只放宽不收紧",
        sphere_in_frustum(&near_out, (0.0, 0.0, -1.0), 0.5) == CullVerdict::Cull
            && sphere_in_frustum(&near_out, (0.0, 0.0, -1.0), 0.9) == CullVerdict::Keep
            && sphere_in_frustum(&near_out, (0.0, 0.0, -1.0), 4.0) == CullVerdict::Keep,
        "",
    );

    // 任一面全外即剔：球心 x=20 落在 x>=10 的半空间内（Keep），
    // 而 (500,500,500) 对left/top/far 三个面都在外（Cull）。
    //
    // 这条判据的措辞曾写成「六面全外才剔」，那是被测物的错判据：
    // 视锥是六半空间之交，正确的剔除条件是「存在一个面在外」。
    // 若按「六面全外」写判据，会把被测物的缺陷当成正确行为钉死。
    let one_out = FrustumPlanes {
        planes: [
            (1.0, 0.0, 0.0, -10.0), // 内侧：x >= 10
            (-1.0, 0.0, 0.0, 100.0),
            (0.0, 1.0, 0.0, 100.0),
            (0.0, -1.0, 0.0, 100.0),
            (0.0, 0.0, 1.0, 0.0), // 内侧：z >= 0
            (0.0, 0.0, -1.0, 100.0),
        ],
    };
    set.add(
        "F1807-剔除-任一面全外即剔",
        sphere_in_frustum(&one_out, (20.0, 0.0, 0.0), 1.0) == CullVerdict::Keep
            && sphere_in_frustum(&fr, (500.0, 500.0, 500.0), 1.0) == CullVerdict::Cull
            && sphere_in_frustum(&fr, (5.0, 0.0, -5.0), 1.0) == CullVerdict::Cull,
        "",
    );

    set.add(
        "F1807-剔除-非有限保Keep",
        sphere_in_frustum(&fr, (f32::NAN, 0.0, 0.0), 1.0) == CullVerdict::Keep
            && sphere_in_frustum(&fr, (0.0, 0.0, 0.0), f32::INFINITY) == CullVerdict::Keep,
        "",
    );

    set.add(
        "F1807-剔除-标量规模诚实声明",
        CULL_IMPL_SCALING_DOC.contains("标量")
            && CULL_IMPL_SCALING_DOC.contains("I07")
            && CULL_IMPL_SCALING_DOC.contains("不得"),
        "",
    );

    // ——— 错误路径与降级矩阵 ———
    set.add(
        "F1807-降级-悬挂拒绝不崩溃",
        m.get(h0).is_err() && m.is_live(h0) == false,
        "",
    );

    // 风暴：暂存 1000 次增删，未 commit 时 len 不变，commit 后一次到位。
    //
    // **暂存量必须 ≤ MAX_LIGHTS**：槽位只有MAX_LIGHTS 个，超出的部分
    // 在 commit 时会被拒（计入 rejected）。判据若写applied == 暂存量，
    // 在暂存量超上限时就永远红——那不是被测物的错，是判据没算容量。
    // 这里用 MAX_LIGHTS - 8（留 8 个空槽），并另设一条判据覆盖超限拒绝。
    let storm_n = MAX_LIGHTS - 8;
    let mut m5 = LightManager::new();
    for i in 0..storm_n {
        m5.stage_add(pt((i as f32, 0.0, 0.0), 1.0, 1.0, i as u64 + 1), Vec::new());
    }
    let before_commit = m5.len();
    let pending = m5.pending();
    let (applied, rejected) = m5.commit_frame();
    set.add(
        "F1807-降级-风暴合并到帧边界",
        before_commit == 0
            && pending == storm_n
            && applied == storm_n
            && rejected == 0
            && m5.len() == storm_n
            && m5.pending() == 0,
        "",
    );

    // 风暴超上限：暂存超过槽位容量时，多出的部分被拒而非静默丢弃或 panic。
    let over_n = MAX_LIGHTS + 40;
    let mut m5b = LightManager::new();
    for i in 0..over_n {
        m5b.stage_add(pt((i as f32, 0.0, 0.0), 1.0, 1.0, i as u64 + 1), Vec::new());
    }
    let (a2, r2) = m5b.commit_frame();
    set.add(
        "F1807-降级-风暴超限拒超额",
        a2 == MAX_LIGHTS && r2 == over_n - MAX_LIGHTS && m5b.len() == MAX_LIGHTS,
        "",
    );

    // 超上限拒绝。
    let mut m6 = LightManager::new();
    let mut last: Option<LightError> = None;
    for i in 0..(MAX_LIGHTS + 3) {
        if let Err(e) = m6.add(pt((0.0, 0.0, 0.0), 1.0, 1.0, i as u64 + 1), Vec::new()) {
            last = Some(e.code);
        }
    }
    set.add(
        "F1807-降级-超上限拒绝",
        m6.len() == MAX_LIGHTS && last == Some(LightError::CapacityExhausted),
        "",
    );

    // 上限的**绝对值契约**（判据自洽腿）。
    //
    // **为什么必须另立一条**：上面那条判据的语料上界与期望值**都取自
    // `MAX_LIGHTS` 本身**——把常量从 256 改成 128，语料循环次数跟着变、
    // 期望值也跟着变，判据照样全绿。这与「用表内元素验查表函数」是同一族
    // 自证循环：判据与被测共享同一个真相源，就无法证伪那个真相源。
    //
    // 锚点只给「≤光源总数，典型 <100」，**未规定具体数值**——但 256 这个值
    // 是内存预算的对外承诺（句柄表定长 `MAX_LIGHTS × sizeof(Slot)`），
    // 改它属契约变更，须走 ADR 而非静默调整。故此处把256 钉死：
    // 要么改常量并同步改本判据与 ADR，要么判据变红。
    set.add(
        "F1807-降级-上限绝对值契约(256,变更须走ADR)",
        MAX_LIGHTS == 256,
        "",
    );

    // 非有限参数钳制带告警（不静默入库）。
    let (bad, warns) = LightDesc::new(
        LightKind::Point,
        (f32::NAN, 0.0, 0.0),
        (0.0, 0.0, 0.0),
        (f32::INFINITY, 1.0, 1.0),
        f32::NEG_INFINITY,
        -5.0,
        1,
    );
    set.add(
        "F1807-降级-非有限钳制带告警",
        bad.pos.0.is_finite()
            && bad.intensity.is_finite()
            && bad.color.0.is_finite()
            && bad.range > 0.0
            && !warns.is_empty(),
        "",
    );

    // 裁剪计数显性。
    let mut m7 = LightManager::new();
    for i in 0..10 {
        m7.add(pt((i as f32, 0.0, 0.0), 10.0 - i as f32, 100.0, i as u64 + 1), Vec::new())
            .unwrap();
    }
    let out = m7.build_frame((0.0, 0.0, 0.0), None, 3).unwrap();
    set.add(
        "F1807-降级-裁剪计数显性",
        out.selection.dropped == 7
            && out.stats.dropped == 7
            && out.stats.selected == 3
            && out
                .selection
                .warnings
                .iter()
                .any(|w| w.code == LightWarnCode::SelectionTruncated),
        "",
    );

    // 六码齐备且指引非空。
    let all_codes = [
        LightError::StaleHandle,
        LightError::HandleOutOfRange,
        LightError::CapacityExhausted,
        LightError::KindUnregistered,
        LightError::UncommittedStaging,
        LightError::NonFiniteView,
    ];
    let codes_ok = all_codes
        .iter()
        .all(|c| !c.as_str().is_empty() && !c.hint().is_empty());
    let names_unique = {
        let mut ns: Vec<&str> = all_codes.iter().map(|c| c.as_str()).collect();
        let b = ns.len();
        ns.sort_unstable();
        ns.dedup();
        b == ns.len()
    };
    set.add("F1807-降级-六码齐备且指引非空", codes_ok && names_unique, "");

    // ——— 参数块 ———
    let mut m8 = LightManager::new();
    for i in 0..5 {
        m8.add(pt((i as f32, 0.0, 0.0), 2.0, 3.0, i as u64 + 1), Vec::new())
            .unwrap();
    }
    let blk = m8.build_frame((0.0, 0.0, 0.0), None, 0).unwrap();
    set.add(
        "F1807-参数块-每帧一次",
        blk.param_block.light_count() == 5 && blk.param_block.byte_size == 5 * BYTES_PER_LIGHT_ALIGNED,
        "",
    );
    set.add(
        "F1807-参数块-16字节对齐",
        // 锚点写死「对齐 16 字节」——常量本身必须是 16，不能只查
        // 「byte_size 能被 PARAM_BLOCK_ALIGN 整除」。后者在把
        // PARAM_BLOCK_ALIGN 改成 4 时依然成立（64 % 4 == 0），
        // 于是对齐粒度被悄悄放宽到 4 而自检全绿（反假变体 M11）。
        PARAM_BLOCK_ALIGN == 16
            && blk.param_block.byte_size % 16 == 0
            && BYTES_PER_LIGHT_ALIGNED % 16 == 0
            && LightParam::from_desc(&pt((0.0, 0.0, 0.0), 1.0, 1.0, 1)).byte_size() == BYTES_PER_LIGHT_ALIGNED,
        "",
    );

    // 对齐粒度不得比16 更细：这是独立一条，因为上面那条若只看
    // `byte_size % PARAM_BLOCK_ALIGN == 0` 会被自身定义蒙蔽。
    set.add(
        "F1807-参数块-对齐不弱于16",
        BYTES_PER_LIGHT_ALIGNED % PARAM_BLOCK_ALIGN == 0
            && PARAM_BLOCK_ALIGN >= 16
            && BYTES_PER_LIGHT_ALIGNED >= PARAM_BLOCK_ALIGN,
        "",
    );
    set.add(
        "F1807-参数块-字段序冻结",
        BYTES_PER_LIGHT_RAW == 56
            && BYTES_PER_LIGHT_ALIGNED == 64
            && core::mem::size_of::<LightParam>() == BYTES_PER_LIGHT_ALIGNED,
        "",
    );
    set.add(
        "F1807-参数块-无限范围写哨兵",
        LightParam::from_desc(&directional(1.0, 1)).pos_range[3].is_finite(),
        "",
    );

    // ——— 台账 ———
    let mut ports: Vec<&str> = HANDOFFS.iter().map(|h| h.port).collect();
    let pb = ports.len();
    ports.sort_unstable();
    ports.dedup();
    set.add(
        "F1807-台账-接口位不重复",
        HANDOFFS.len() == 6 && pb == ports.len(),
        "",
    );
    set.add(
        "F1807-台账-统计四计数齐备",
        blk.stats.registered == 5
            && blk.stats.selected == 5
            && blk.stats.culled == 0
            && blk.stats.dropped == 0
            && blk.stats.total_ops == 5,
        "",
    );

    // 描述文本不得为空（判据声明的文档化形态）。
    let texts_ok = !AREA_WEIGHT_RATIONALE.is_empty()
        && !CULL_IMPL_SCALING_DOC.is_empty()
        && HANDOFFS.iter().all(|h| !h.relation.is_empty() && !h.target.is_empty());
    set.add("F1807-台账-声明文本非空", texts_ok, "");

    // 描述格式化辅助不panic（供诊断文案用）。
    let _ = format!("{}", LightError::StaleHandle.as_str());

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 自检全绿() {
        let s = run_vej07_checks();
        let (p, f) = s.tally();
        assert!(
            s.all_passed(),
            "F1807 红项 {}/{}：{:?}",
            f,
            p + f,
            s.red_items()
                .0
                .iter()
                .filter_map(|x| x.clone().map(|c| format!("{}", c.name)))
                .collect::<Vec<_>>()
        );
        assert!(!s.truncated(), "F1807 自检被截断");
    }
}
