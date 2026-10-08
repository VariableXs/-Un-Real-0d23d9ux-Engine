//! VE-F1612 · 几何校验器域自检（判据逐条映射，40 项）
//!
//! **判据（锚点原文）**：schema、三查、容错、同标准、判据。
//!
//! # 判据侧的独立重算纪律
//!
//! 本文件**不复用被测的任何判定逻辑**来算期望值：
//! - 三查期望由判据侧自己遍历语料的索引/坐标数组独立算出，
//!   再与 [`triple_scan`] 的产出对账（不信 `count_of` 自述）。
//! - schema 期望由判据侧按声明表**手工列举**每个字段的违规数，
//!   不调用 [`validate_schema`] 反推。
//! - 修复后「三查应清零」由判据侧对修复产物**重跑一次独立三查**确认。
//!
//! # 弱门禁防护
//!
//! - 三查语料**三类各至少一条且互不遮蔽**：含NaN 的顶点与含超大值的顶点
//!   分属不同顶点，断言两类计数各自独立为正——若被测把两类合并成一条，
//!   两条断言会同时红。
//! - 范围检查的边界用**夹逼对**（`limit` / `limit+ε` / `limit-ε`），
//!   不只在明显越界处取样。
//! - 修复效果断言的是**修复后重扫的结果**，不是「修复函数被调用过」。

#![allow(dead_code)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::gfx::meshrepair::{Defect, DefectKind, RepairMesh};
use crate::gfx::meshvalidate::*;


/// 判据侧独立三查：自己遍历语料，返回 `(越界数, 非有限顶点数, 超限顶点数)`。
///
/// **刻意不调用被测的 `triple_scan`**——若调用则判据自证式
/// （问被测函数要答案）。这里重写一遍朴素遍历作为参考实现。
fn ref_triple(
    verts: &[f32],
    faces: &[u32],
    limit: f64,
) -> (usize, usize, usize) {
    let vcount = verts.len() / 3;
    let mut oob = 0usize;
    for &idx in faces {
        if idx as usize >= vcount {
            oob += 1;
        }
    }
    let mut nan = 0usize;
    let mut huge = 0usize;
    for v in 0..vcount {
        let mut is_nan = false;
        let mut is_huge = false;
        for c in 0..3 {
            let x = verts[v * 3 + c] as f64;
            if !x.is_finite() {
                is_nan = true;
            } else if x.abs() > limit {
                is_huge = true;
            }
        }
        // 与被测同口径：一个顶点只归一类（NaN 优先）。
        if is_nan {
            nan += 1;
        } else if is_huge {
            huge += 1;
        }
    }
    (oob, nan, huge)
}

/// 一个各面独立的干净网格（4 顶点 2 面），供 schema/漏斗判据复用。
fn quad() -> RepairMesh {
    let mut m = RepairMesh::new();
    m.push_vert([0.0, 0.0, 0.0]);
    m.push_vert([1.0, 0.0, 0.0]);
    m.push_vert([1.0, 1.0, 0.0]);
    m.push_vert([0.0, 1.0, 0.0]);
    m.push_face([0, 1, 2]);
    m.push_face([0, 2, 3]);
    m
}

/// 合法的 vmesh schema 实参（六个字段全对）。
fn good_values() -> Vec<(String, FieldValue)> {
    vec![
        ("version".to_string(), FieldValue::Uint(1)),
        ("vert_count".to_string(), FieldValue::Uint(4)),
        ("face_count".to_string(), FieldValue::Uint(2)),
        ("coord_limit".to_string(), FieldValue::Float(1.0e6)),
        ("index_bits".to_string(), FieldValue::Uint(32)),
        ("uv_components".to_string(), FieldValue::Uint(2)),
    ]
}

/// 判据域自检。
pub fn run_vei12_checks() -> CheckSet {
    let mut set = CheckSet::new("gfx-vei12");
    let schema = MeshSchema::vmesh();
    let limits = ScanLimits::new(1.0e6);
    let decode = DecodeLimits::default();

    // =====================================================================
    // 判据族一：schema —— 字段类型 / 必填 / 范围（锚点原文三件）
    // =====================================================================

    // 全字段齐备且合法 ⇒ 零违规（合法侧反向对照，防「恒红」）。
    {
        let r = validate_schema(&schema, &good_values());
        set.add(
            "I1612-schema-合法输入零违规",
            r.is_clean() && r.faults.is_empty() && r.rejected_unknown.is_empty(),
            "",
        );
    }

    // 类型不匹配：version 由 Uint 改成 Text。
    {
        let mut v = good_values();
        v[0].1 = FieldValue::Text("1".to_string());
        let r = validate_schema(&schema, &v);
        // 判据侧独立期望：恰好 1 项 FieldType，且字段名为 version。
        let ft: Vec<&SchemaFault> = r
            .faults
            .iter()
            .filter(|f| f.kind == SchemaFaultKind::FieldType)
            .collect();
        set.add(
            "I1612-schema-类型不符检出",
            r.count_of(SchemaFaultKind::FieldType) == 1
                && ft.len() == 1
                && ft[0].field == "version"
                && ft[0].declared == FieldType::Uint.code()
                && ft[0].actual == FieldType::Text.code(),
            "",
        );
    }

    // 五种类型逐个错一遍——防止实现只认某一种类型错。
    {
        let mut ok = true;
        let cases: [(usize, FieldValue, FieldType); 5] = [
            (0, FieldValue::Int(1), FieldType::Uint),
            (3, FieldValue::Uint(1000), FieldType::Float),
            (4, FieldValue::Text("32".to_string()), FieldType::Uint),
            (5, FieldValue::Float(2.0), FieldType::Uint),
            (1, FieldValue::Bool(true), FieldType::Uint),
        ];
        for (i, bad, _want) in cases {
            let mut v = good_values();
            v[i].1 = bad;
            let r = validate_schema(&schema, &v);
            let n = r.count_of(SchemaFaultKind::FieldType);
            if n != 1 {
                ok = false;
            }
            // 类型不符时**不得**同时报范围错（否则一次错报两项，
            // 会让「恰好一项」的判据失去定位力）。
            if r.count_of(SchemaFaultKind::RangeViolated) != 0 {
                ok = false;
            }
        }
        set.add("I1612-schema-五种类型错各报一项", ok, "");
    }

    // 必填缺失：删掉 vert_count。
    {
        let mut v = good_values();
        v.retain(|(k, _)| k != "vert_count");
        let r = validate_schema(&schema, &v);
        let mf: Vec<&SchemaFault> = r
            .faults
            .iter()
            .filter(|f| f.kind == SchemaFaultKind::MissingField)
            .collect();
        set.add(
            "I1612-schema-必填缺失检出",
            r.count_of(SchemaFaultKind::MissingField) == 1
                && mf.len() == 1
                && mf[0].field == "vert_count"
                && mf[0].actual == u8::MAX,
            "",
        );
    }

    // 可选字段缺失**不得**报（uv_components 声明为可选）。
    {
        let mut v = good_values();
        v.retain(|(k, _)| k != "uv_components");
        let r = validate_schema(&schema, &v);
        set.add(
            "I1612-schema-可选字段缺失不报",
            r.count_of(SchemaFaultKind::MissingField) == 0 && r.is_clean(),
            "",
        );
    }

    // 范围越界：vert_count 超过 5000 万。
    {
        let mut v = good_values();
        v[1].1 = FieldValue::Uint(60_000_000);
        let r = validate_schema(&schema, &v);
        let rv: Vec<&SchemaFault> = r
            .faults
            .iter()
            .filter(|f| f.kind == SchemaFaultKind::RangeViolated)
            .collect();
        set.add(
            "I1612-schema-范围越界检出",
            r.count_of(SchemaFaultKind::RangeViolated) == 1
                && rv.len() == 1
                && rv[0].field == "vert_count"
                && rv[0].observed == 60_000_000.0,
            "",
        );
    }

    // 范围夹逼对：上界恰好命中（合法）/ 超出 1（非法）。
    // **两侧都要验**——只验越界侧则「上界判成开区间」这类错抓不到。
    {
        let spec_hi = 50_000_000.0f64;
        let mut at = good_values();
        at[1].1 = FieldValue::Uint(spec_hi as u64);
        let r_at = validate_schema(&schema, &at);
        let mut over = good_values();
        over[1].1 = FieldValue::Uint(spec_hi as u64 + 1);
        let r_over = validate_schema(&schema, &over);
        set.add(
            "I1612-schema-范围上界闭区间夹逼",
            r_at.count_of(SchemaFaultKind::RangeViolated) == 0
                && r_over.count_of(SchemaFaultKind::RangeViolated) == 1,
            "",
        );
    }

    // 范围夹逼对：下界恰好命中（合法）/ 低于 1（非法）。
    {
        let mut at = good_values();
        at[2].1 = FieldValue::Uint(0);
        let r_at = validate_schema(&schema, &at);
        let mut under = good_values();
        under[2].1 = FieldValue::Uint(0);
        under[0].1 = FieldValue::Uint(0); // version 下界 1 → 越界
        let r_under = validate_schema(&schema, &under);
        set.add(
            "I1612-schema-范围下界闭区间夹逼",
            r_at.count_of(SchemaFaultKind::RangeViolated) == 0
                && r_under.count_of(SchemaFaultKind::RangeViolated) == 1,
            "",
        );
    }

    // **NaN 作为实参值必须被判越界**——`n < low` 对 NaN 恒假，
    // 只做区间比较的实现会漏判这一类（弱门禁：不可比的值要显式判）。
    {
        let mut v = good_values();
        v[3].1 = FieldValue::Float(f64::NAN);
        let r = validate_schema(&schema, &v);
        let rv: Vec<&SchemaFault> = r
            .faults
            .iter()
            .filter(|f| f.kind == SchemaFaultKind::RangeViolated)
            .collect();
        set.add(
            "I1612-schema-NaN值判越界",
            rv.len() == 1 && rv[0].field == "coord_limit" && rv[0].observed.is_nan(),
            "",
        );
    }

    // 未知字段：Reject 策略下拦截并登记。
    {
        let mut v = good_values();
        v.push(("evil_field".to_string(), FieldValue::Uint(1)));
        let r = validate_schema(&schema, &v);
        set.add(
            "I1612-schema-未知字段按Reject拦截",
            r.rejected_unknown.len() == 1
                && r.rejected_unknown[0] == "evil_field"
                && !r.is_clean(),
            "",
        );
    }

    // 未知字段：Ignore 策略下忽略**但计数可查**（非静默）。
    {
        let mut s2 = schema.clone();
        s2.unknown = UnknownPolicy::Ignore;
        let mut v = good_values();
        v.push(("evil_field".to_string(), FieldValue::Uint(1)));
        let r = validate_schema(&s2, &v);
        set.add(
            "I1612-schema-未知字段按Ignore留痕",
            r.ignored_unknown.len() == 1
                && r.rejected_unknown.is_empty()
                && r.is_clean(),
            "",
        );
    }

    // 非数值字段不做范围检查（Text 超长不是范围违规）。
    {
        let s2 = MeshSchema {
            specs: vec![FieldSpec::new("name", FieldType::Text, true).range(0.0, 4.0)],
            unknown: UnknownPolicy::Reject,
        };
        let v = vec![("name".to_string(), FieldValue::Text("very long name".to_string()))];
        let r = validate_schema(&s2, &v);
        set.add(
            "I1612-schema-非数值字段免范围检查",
            r.count_of(SchemaFaultKind::RangeViolated) == 0 && r.is_clean(),
            "",
        );
    }

    // 声明自身区间倒置要被检出（声明不可信也是攻击面）。
    {
        let s2 = MeshSchema {
            specs: vec![FieldSpec::new("bad", FieldType::Uint, false).range(10.0, 1.0)],
            unknown: UnknownPolicy::Reject,
        };
        let r = validate_schema(&s2, &[]);
        set.add(
            "I1612-schema-声明区间倒置检出",
            r.count_of(SchemaFaultKind::RangeViolated) == 1,
            "",
        );
    }

    // 报告确定性：同一输入两次校验，产出逐条相等。
    {
        let mut v = good_values();
        v[0].1 = FieldValue::Text("x".to_string());
        v.push(("zzz".to_string(), FieldValue::Bool(true)));
        let a = validate_schema(&schema, &v);
        let b = validate_schema(&schema, &v);
        set.add("I1612-schema-报告确定性", a.faults == b.faults, "");
    }

    // 码位自洽：label/code/of_code 三者往返一致（含非法码位返回 None）。
    {
        let mut ok = true;
        for k in [
            SchemaFaultKind::FieldType,
            SchemaFaultKind::MissingField,
            SchemaFaultKind::RangeViolated,
        ] {
            if SchemaFaultKind::of_code(k.code()) != Some(k) || k.label().is_empty() {
                ok = false;
            }
        }
        if SchemaFaultKind::of_code(3).is_some() {
            ok = false;
        }
        set.add("I1612-schema-码位自洽", ok, "");
    }

    // =====================================================================
    // 判据族二：三查（越界索引 / NaN 几何 / 超大属性值）
    // =====================================================================

    // 干净网格三查全零（合法侧反向对照）。
    {
        let m = quad();
        let r = triple_scan(&m, &limits);
        let (eo, en, eh) = ref_triple(&m.verts, &m.faces, limits.coord_limit);
        set.add(
            "I1612-三查-干净网格零命中",
            r.is_clean() && r.total() == 0 && (eo, en, eh) == (0, 0, 0),
            "",
        );
    }

    // 查一：索引越界。**用夹逼对**：idx == vcount 合法、idx == vcount+1 越界。
    {
        let mut m = quad();
        m.push_face([0, 1, 4]); // 4 >= 4 越界
        let r = triple_scan(&m, &limits);
        let (eo, _, _) = ref_triple(&m.verts, &m.faces, limits.coord_limit);
        set.add(
            "I1612-三查-索引越界等于顶点数即拦",
            r.count_of(TripleScan::IndexOutOfRange) == 1 && eo == 1,
            "",
        );
    }
    {
        let mut m = quad();
        m.push_face([0, 1, 3]); // 3 < 4 合法
        let r = triple_scan(&m, &limits);
        set.add(
            "I1612-三查-索引等于末顶点不拦",
            r.count_of(TripleScan::IndexOutOfRange) == 0,
            "",
        );
    }

    // 查一多命中：一次越界三个索引全越界 ⇒ 计 3 条（不是 1 条面）。
    {
        let mut m = quad();
        m.push_face([7, 8, 9]);
        let r = triple_scan(&m, &limits);
        let (eo, _, _) = ref_triple(&m.verts, &m.faces, limits.coord_limit);
        set.add(
            "I1612-三查-越界按索引逐条计",
            r.count_of(TripleScan::IndexOutOfRange) == 3 && eo == 3,
            "",
        );
    }

    // 查二：NaN 几何。
    {
        let mut m = quad();
        m.verts[3] = f32::NAN; // 顶点 1 的 x 分量
        let r = triple_scan(&m, &limits);
        let (_, en, _) = ref_triple(&m.verts, &m.faces, limits.coord_limit);
        set.add(
            "I1612-三查-NaN几何检出",
            r.count_of(TripleScan::NanGeometry) == 1 && en == 1,
            "",
        );
    }

    // 查二含 Inf：Inf 也是非有限，须同样中查二。
    {
        let mut m = quad();
        m.verts[7] = f32::INFINITY;
        let r = triple_scan(&m, &limits);
        set.add(
            "I1612-三查-Inf同归NaN族",
            r.count_of(TripleScan::NanGeometry) == 1,
            "",
        );
    }

    // 查二含负 Inf。
    {
        let mut m = quad();
        m.verts[0] = f32::NEG_INFINITY;
        let r = triple_scan(&m, &limits);
        set.add(
            "I1612-三查-负Inf同归NaN族",
            r.count_of(TripleScan::NanGeometry) == 1,
            "",
        );
    }

    // 查三：超大属性值（1e30 坐标）。
    {
        let mut m = quad();
        m.verts[6] = 1.0e30; // 顶点 2 的 x
        let r = triple_scan(&m, &limits);
        let (_, _, eh) = ref_triple(&m.verts, &m.faces, limits.coord_limit);
        set.add(
            "I1612-三查-超大属性值检出",
            r.count_of(TripleScan::HugeAttribute) == 1 && eh == 1,
            "",
        );
    }

    // 查三夹逼对：恰等于上限不拦、超出 1 拦。
    {
        let lim = 1000.0f64;
        let l2 = ScanLimits::new(lim);
        let mut at = quad();
        at.verts[0] = lim as f32;
        let r_at = triple_scan(&at, &l2);
        let mut over = quad();
        over.verts[0] = (lim + 1.0) as f32;
        let r_over = triple_scan(&over, &l2);
        set.add(
            "I1612-三查-属性上限闭区间夹逼",
            r_at.count_of(TripleScan::HugeAttribute) == 0
                && r_over.count_of(TripleScan::HugeAttribute) == 1,
            "",
        );
    }

    // 查三含负超大值：绝对值语义，两侧都要拦。
    {
        let mut m = quad();
        m.verts[9] = -1.0e30;
        let r = triple_scan(&m, &limits);
        set.add(
            "I1612-三查-负超大值同样拦",
            r.count_of(TripleScan::HugeAttribute) == 1,
            "",
        );
    }

    // **三查互不遮蔽**：一个 NaN 顶点 + 一个超大顶点 + 一个越界索引同网格，
    // 三类计数必须**同时**为正（判据侧独立算出三个期望）。
    {
        let mut m = quad();
        m.verts[3] = f32::NAN; // 顶点 1 → NaN
        m.verts[6] = 1.0e30; // 顶点 2 → 超大
        m.push_face([0, 1, 9]); // 9 >= 4 → 越界
        let r = triple_scan(&m, &limits);
        let (eo, en, eh) = ref_triple(&m.verts, &m.faces, limits.coord_limit);
        set.add(
            "I1612-三查-三类同网格互不遮蔽",
            r.count_of(TripleScan::IndexOutOfRange) == eo
                && r.count_of(TripleScan::NanGeometry) == en
                && r.count_of(TripleScan::HugeAttribute) == eh
                && eo == 1
                && en == 1
                && eh == 1,
            "",
        );
    }

    // **NaN 顶点不得同时计入超大族**（一顶点只归一类，否则计数重复）。
    {
        let mut m = quad();
        m.verts[3] = f32::NAN;
        let r = triple_scan(&m, &limits);
        set.add(
            "I1612-三查-NaN不重复计入超大族",
            r.count_of(TripleScan::NanGeometry) == 1
                && r.count_of(TripleScan::HugeAttribute) == 0
                && r.total() == 1,
            "",
        );
    }

    // 分母口径：vertices_seen / indices_seen 独立登记。
    {
        let m = quad();
        let r = triple_scan(&m, &limits);
        set.add(
            "I1612-三查-分母口径登记",
            r.vertices_seen == 4 && r.indices_seen == 6,
            "",
        );
    }

    // 定位：越界命中的 vertex 字段记的是越界索引值本身。
    {
        let mut m = quad();
        m.push_face([0, 1, 42]);
        let r = triple_scan(&m, &limits);
        let hit = r
            .hits
            .iter()
            .find(|h| h.scan == TripleScan::IndexOutOfRange);
        set.add(
            "I1612-三查-越界定位记索引值",
            match hit {
                Some(h) => h.vertex == 42 && h.face == 2,
                None => false,
            },
            "",
        );
    }

    // **每顶点只记一条**：同一顶点在多个面上重复出现、或同一顶点多轴同时超限时，
// 命中数仍为 1——否则计数随面数膨胀，报告与分母口径全失真。
    //
    // 弱门禁防范：语料必须让`seen` 机制**真的被触发**——
    // ① 同一问题顶点被两个面引用（面数增加，命中数不得增加）；
    // ② 同一顶点的 x 与 y 两轴都超限（一顶点仍只记一条）。
    // 若语料里每个问题顶点只出现一次，去重逻辑改成什么样都全绿。
    {
        let mut m = RepairMesh::new();
        m.push_vert([0.0, 0.0, 0.0]);
        m.push_vert([1.0, 0.0, 0.0]);
        m.push_vert([0.0, 1.0, 0.0]);
        m.push_vert([0.0, 0.0, 1.0]);
        // 顶点 3 的 y 与 z 两轴都超限（1e30 与 1e31）
        m.verts[10] = 1.0e30;
        m.verts[11] = 1.0e31;
        // 顶点 3 被三个面引用
        m.push_face([0, 1, 3]);
        m.push_face([1, 2, 3]);
        m.push_face([2, 0, 3]);
        let r = triple_scan(&m, &limits);
        // 判据侧独立期望：只有顶点 3 超限，且只记 1 条。
        let (_, _, eh) = ref_triple(&m.verts, &m.faces, limits.coord_limit);
        set.add(
            "I1612-三查-每顶点只记一条命中",
            eh == 1
                && r.count_of(TripleScan::HugeAttribute) == 1
                && r.total() == 1,
            "",
        );
    }

    // 空网格：三查不panic 且零命中（空是合法资产）。
    {
        let m = RepairMesh::new();
        let r = triple_scan(&m, &limits);
        set.add(
            "I1612-三查-空网格零命中",
            r.is_clean() && r.vertices_seen == 0 && r.indices_seen == 0,
            "",
        );
    }

    // 码位自洽（三查三类）。
    {
        let mut ok = true;
        for s in [
            TripleScan::IndexOutOfRange,
            TripleScan::NanGeometry,
            TripleScan::HugeAttribute,
        ] {
            if TripleScan::of_code(s.code()) != Some(s) || s.label().is_empty() {
                ok = false;
            }
        }
        if TripleScan::of_code(3).is_some() {
            ok = false;
        }
        set.add("I1612-三查-码位自洽", ok, "");
    }

    // 三查确定性：同一网格两次扫描逐条相等。
    //
    // **按位比较而非 `==`**：语料含 NaN 顶点，而 `f64::NAN != f64::NAN`
    // 恒成立——用 `==` 比metric 会让本判据恒红，且红因与被测无关
    // （判据侧语料缺陷伪装成被测缺陷）。`to_bits` 让 NaN 按位模式可比。
    {
        fn hit_key(h: &ScanHit) -> (u8, u32, u32, u64) {
            (h.scan.code(), h.face, h.vertex, h.metric.to_bits())
        }
        let mut m = quad();
        m.verts[3] = f32::NAN;
        m.verts[6] = 1.0e30;
        m.push_face([0, 1, 9]);
        let a = triple_scan(&m, &limits);
        let b = triple_scan(&m, &limits);
        let ka: Vec<(u8, u32, u32, u64)> = a.hits.iter().map(hit_key).collect();
        let kb: Vec<(u8, u32, u32, u64)> = b.hits.iter().map(hit_key).collect();
        set.add("I1612-三查-结果确定性", ka == kb && ka.len() == 3, "");
    }

    // =====================================================================
    // 判据族三：容错策略（修复 / 拒绝二分 + 三要素）
    // =====================================================================

    // 三查三类**全部可修**（可修表独立重算，不信实现自述）。
    {
        let mut m = quad();
        m.verts[3] = f32::NAN;
        m.verts[6] = 1.0e30;
        m.push_face([0, 1, 9]);
        let r = triple_scan(&m, &limits);
        let plan = plan_fixes(&r, &limits);
        // 判据侧独立期望：三条命中 ⇒ 三条修复动作。
        let (eo, en, eh) = ref_triple(&m.verts, &m.faces, limits.coord_limit);
        set.add(
            "I1612-容错-三查全可修且动作数等于命中数",
            plan.rejections.is_empty()
                && plan.fixes.len() == eo + en + eh
                && plan.fixes.len() == 3,
            "",
        );
    }

    // 修复分类表自洽：六个类，可修/不可修的归属与判据侧硬编的表一致。
    {
        let want_fixable = [
            FixClass::IndexOutOfRange,
            FixClass::NanGeometry,
            FixClass::HugeAttribute,
        ];
        let want_reject = [
            FixClass::SchemaType,
            FixClass::SchemaMissing,
            FixClass::SchemaRange,
        ];
        let mut ok = true;
        for c in want_fixable {
            if !c.is_fixable() {
                ok = false;
            }
        }
        for c in want_reject {
            if c.is_fixable() {
                ok = false;
            }
        }
        set.add("I1612-容错-可修表独立重算一致", ok, "");
    }

    // 修复后**三查重扫清零**——判据侧独立重扫，不用被测的 count_of。
    {
        let mut m = quad();
        m.verts[3] = f32::NAN;
        m.verts[6] = 1.0e30;
        m.push_face([0, 1, 9]);
        let r = triple_scan(&m, &limits);
        let plan = plan_fixes(&r, &limits);
        let fixed = apply_fixes(&m, &plan);
        let after = ref_triple(&fixed.verts, &fixed.faces, limits.coord_limit);
        // 越界面被丢掉 ⇒ 索引越界清零；NaN 置零、超大钳制 ⇒ 后两族清零。
        set.add(
            "I1612-容错-修复后独立重扫清零",
            after == (0, 0, 0) && fixed.face_count() == 2,
            "",
        );
    }

    // 修复不动原网格（可撤销的前提）。
    {
        let mut m = quad();
        m.verts[3] = f32::NAN;
        let r = triple_scan(&m, &limits);
        let plan = plan_fixes(&r, &limits);
        let _fixed = apply_fixes(&m, &plan);
        set.add(
            "I1612-容错-修复不改原网格",
            m.verts[3].is_nan() && m.face_count() == 2,
            "",
        );
    }

    // 丢面幂等：两条修复动作指向同一面只丢一次。
    {
        let mut m = quad();
        m.push_face([0, 1, 9]);
        m.push_face([0, 2, 9]); // 同一面内含越界索引 9
        let r = triple_scan(&m, &limits);
        let plan = plan_fixes(&r, &limits);
        let fixed = apply_fixes(&m, &plan);
        set.add(
            "I1612-容错-丢面按面号去重",
            fixed.face_count() == 2 && r.count_of(TripleScan::IndexOutOfRange) == 2,
            "",
        );
    }

    // 钳制到**上限本身**（不是上限的一半）。
    {
        let lim = 500.0f64;
        let l2 = ScanLimits::new(lim);
        let mut m = quad();
        m.verts[0] = 1.0e30;
        let r = triple_scan(&m, &l2);
        let plan = plan_fixes(&r, &l2);
        let fixed = apply_fixes(&m, &plan);
        set.add(
            "I1612-容错-钳制到上限值",
            fixed.verts[0] == lim as f32,
            "",
        );
    }

    // 负超大值钳制到**负上限**（符号不得丢）。
    {
        let lim = 500.0f64;
        let l2 = ScanLimits::new(lim);
        let mut m = quad();
        m.verts[0] = -1.0e30;
        let r = triple_scan(&m, &l2);
        let plan = plan_fixes(&r, &l2);
        let fixed = apply_fixes(&m, &plan);
        set.add(
            "I1612-容错-负值钳制保号",
            fixed.verts[0] == -(lim as f32),
            "",
        );
    }

    // 修复动作标签非空（人话可读，不是空串）。
    {
        let mut m = quad();
        m.verts[3] = f32::NAN;
        m.verts[6] = 1.0e30;
        m.push_face([0, 1, 9]);
        let r = triple_scan(&m, &limits);
        let plan = plan_fixes(&r, &limits);
        set.add(
            "I1612-容错-修复动作标签非空",
            plan.fixes.iter().all(|f| !f.label().is_empty()),
            "",
        );
    }

    // **拒绝三要素齐备**：schema 类型错 ⇒ 一条拒绝，三要素全非空。
    {
        let mut v = good_values();
        v[0].1 = FieldValue::Text("x".to_string());
        let r = validate_schema(&schema, &v);
        let plan = plan_schema_rejections(&r);
        set.add(
            "I1612-容错-拒绝三要素齐备",
            plan.rejections.len() == 1 && plan.fixes.is_empty() && {
                match &plan.rejections[0].disposition {
                    Disposition::Reject(n) => {
                        n.is_complete()
                            && !n.exceeded.is_empty()
                            && !n.because.is_empty()
                            && !n.remedy.is_empty()
                    }
                    Disposition::Fix(_) => false,
                }
            },
            "",
        );
    }

    // 三类 schema 违规各产一条拒绝，且**都不可修**。
    {
        let mut v = good_values();
        v[0].1 = FieldValue::Text("x".to_string()); // 类型错
        v.retain(|(k, _)| k != "vert_count"); // 必填缺失
        v[1].1 = FieldValue::Uint(60_000_000); // 范围越界
        let r = validate_schema(&schema, &v);
        let plan = plan_schema_rejections(&r);
        set.add(
            "I1612-容错-三类schema违规各产拒绝",
            r.count_of(SchemaFaultKind::FieldType) == 1
                && r.count_of(SchemaFaultKind::MissingField) == 1
                && r.count_of(SchemaFaultKind::RangeViolated) == 1
                && plan.rejections.len() == 3
                && plan.fixes.is_empty(),
            "",
        );
    }

    // 拒绝告知的**分桶计数**与违规数对账（口径不许漂）。
    {
        let mut v = good_values();
        v[0].1 = FieldValue::Text("x".to_string());
        v.retain(|(k, _)| k != "index_bits");
        let r = validate_schema(&schema, &v);
        let plan = plan_schema_rejections(&r);
        let sum: u32 = plan.by_class.iter().map(|(_, n)| *n).sum();
        set.add(
            "I1612-容错-分桶计数与拒绝数对账",
            plan.rejections.len() == 2 && sum == 2,
            "",
        );
    }

    // **三要素缺一即不可交付**：造一条缺「怎么办」的告知，is_complete 须为 false。
    {
        let n1 = RejectNotice::new("超了", "因为", "怎么办");
        let n2 = RejectNotice::new("超了", "因为", "  ");
        let n3 = RejectNotice::new("", "因为", "怎么办");
        let n4 = RejectNotice::new("超了", "", "怎么办");
        set.add(
            "I1612-容错-三要素缺一判不可交付",
            n1.is_complete() && !n2.is_complete() && !n3.is_complete() && !n4.is_complete(),
            "",
        );
    }

    // 修复类标签与码位自洽。
    {
        let mut ok = true;
        for c in [
            FixClass::IndexOutOfRange,
            FixClass::NanGeometry,
            FixClass::HugeAttribute,
            FixClass::SchemaType,
            FixClass::SchemaMissing,
            FixClass::SchemaRange,
        ] {
            if c.label().is_empty() {
                ok = false;
            }
        }
        set.add("I1612-容错-分类标签非空", ok, "");
    }

    // =====================================================================
    // 判据族四：同标准对接（F1121 漏斗 / F1607 缺陷族）
    // =====================================================================

    // 四层齐备且顺序码为 0/1/2/3（不可跳、不可换序）。
    {
        let ords: Vec<u8> = FUNNEL_LAYERS.iter().map(|l| l.ordinal()).collect();
        set.add(
            "I1612-对接-四层漏斗顺序不可跳",
            FUNNEL_LAYERS.len() == 4
                && ords == vec![0, 1, 2, 3]
                && FUNNEL_LAYERS[0] == FunnelLayer::Probe
                && FUNNEL_LAYERS[3] == FunnelLayer::Content,
            "",
        );
    }

    // 层码位自洽。
    {
        let mut ok = true;
        for l in FUNNEL_LAYERS {
            if FunnelLayer::label(l).is_empty() || l.ordinal() > 3 {
                ok = false;
            }
        }
        set.add("I1612-对接-层码位自洽", ok, "");
    }

    // 干净网格：四层零拦截 + 放行。
    {
        let m = quad();
        let v = validate_mesh(&m, &schema, &good_values(), &limits, &decode);
        set.add(
            "I1612-对接-干净网格放行",
            v.accepted
                && v.funnel.len() == 4
                && v.layer_count(FunnelLayer::Probe) == 0
                && v.layer_count(FunnelLayer::Structure) == 0
                && v.layer_count(FunnelLayer::Decode) == 0
                && v.layer_count(FunnelLayer::Content) == 0
                && v.all_notices_complete(),
            "",
        );
    }

    // L1 探测：顶点数组长度不对齐 ⇒ 拦且不放行。
    {
        let mut m = quad();
        m.verts.push(0.0); // 长度不再被3 整除
        let v = validate_mesh(&m, &schema, &good_values(), &limits, &decode);
        set.add(
            "I1612-对接-L1探测拦不对齐数组",
            !v.accepted && v.layer_count(FunnelLayer::Probe) == 1,
            "",
        );
    }

    // L2 结构：schema 违规 ⇒ 该层计数 = 违规项数，且不放行。
    {
        let m = quad();
        let mut vals = good_values();
        vals[0].1 = FieldValue::Text("x".to_string());
        let v = validate_mesh(&m, &schema, &vals, &limits, &decode);
        set.add(
            "I1612-对接-L2结构计数等于违规数",
            !v.accepted
                && v.layer_count(FunnelLayer::Structure) == 1
                && v.rejections.len() == 1,
            "",
        );
    }

    // L3 解码：顶点数超上限 ⇒ 该层计数为 1，且不放行。
    {
        let m = quad();
        let small = DecodeLimits {
            max_verts: 2,
            max_faces: 100,
        };
        let v = validate_mesh(&m, &schema, &good_values(), &limits, &small);
        set.add(
            "I1612-对接-L3解码资源上限拦",
            !v.accepted && v.layer_count(FunnelLayer::Decode) == 1,
            "",
        );
    }

    // L3 夹逼对：顶点数**恰好等于**上限不拦、超 1 拦。
    {
        let m = quad(); // 4 顶点
        let at_limit = DecodeLimits {
            max_verts: 4,
            max_faces: 100,
        };
        let v_at = validate_mesh(&m, &schema, &good_values(), &limits, &at_limit);
        let over = DecodeLimits {
            max_verts: 3,
            max_faces: 100,
        };
        let v_over = validate_mesh(&m, &schema, &good_values(), &limits, &over);
        set.add(
            "I1612-对接-L3上限闭区间夹逼",
            v_at.layer_count(FunnelLayer::Decode) == 0
                && v_over.layer_count(FunnelLayer::Decode) == 1,
            "",
        );
    }

    // L4 内容：三查命中 ⇒ 该层计数 = 命中总数，且不放行（未经修复）。
    {
        let mut m = quad();
        m.verts[3] = f32::NAN;
        m.verts[6] = 1.0e30;
        m.push_face([0, 1, 9]);
        let (eo, en, eh) = ref_triple(&m.verts, &m.faces, limits.coord_limit);
        let expect = eo + en + eh;
        let v = validate_mesh(&m, &schema, &good_values(), &limits, &decode);
        set.add(
            "I1612-对接-L4内容计数等于三查命中",
            !v.accepted
                && v.layer_count(FunnelLayer::Content) == expect as u32
                && expect == 3,
            "",
        );
    }

    // F1607 缺陷族对接：NaN 映射退化族；越界与超大**如实不可映射**。
    {
        let nan_hit = ScanHit {
            scan: TripleScan::NanGeometry,
            face: 0,
            vertex: 1,
            metric: f64::NAN,
        };
        let oob_hit = ScanHit {
            scan: TripleScan::IndexOutOfRange,
            face: 0,
            vertex: 42,
            metric: 38.0,
        };
        let huge_hit = ScanHit {
            scan: TripleScan::HugeAttribute,
            face: 0,
            vertex: 2,
            metric: 1.0e30,
        };
        let d = defect_of(&nan_hit);
        set.add(
            "I1612-对接-F1607缺陷族映射",
            match d {
                Some(x) => x.kind == DefectKind::Degenerate && x.vertex == 1,
                None => false,
            } && defect_of(&oob_hit).is_none()
                && defect_of(&huge_hit).is_none(),
            "",
        );
    }

    // 顶层裁决的可交付性：一旦有拒绝项，**必不放行**且三要素齐备。
    {
        let m = quad();
        let mut vals = good_values();
        vals[0].1 = FieldValue::Int(9); // 类型错
        let v = validate_mesh(&m, &schema, &vals, &limits, &decode);
        set.add(
            "I1612-对接-有拒绝必不放行",
            !v.accepted && !v.rejections.is_empty() && v.all_notices_complete(),
            "",
        );
    }

    // 逐层拦截量可独立审计（F1121 通过率统计面）。
    {
        let mut vals = good_values();
        vals[0].1 = FieldValue::Int(9);
        let mut bad = quad();
        bad.push_face([0, 1, 99]);
        let v = validate_mesh(&bad, &schema, &vals, &limits, &decode);
        let total: u32 = v.funnel.iter().map(|h| h.count).sum();
        set.add(
            "I1612-对接-逐层拦截量可审计",
            v.funnel.len() == 4
                && total == v.layer_count(FunnelLayer::Structure)
                    + v.layer_count(FunnelLayer::Content)
                && v.layer_count(FunnelLayer::Structure) == 1
                && v.layer_count(FunnelLayer::Content) == 1,
            "",
        );
    }

    set
}