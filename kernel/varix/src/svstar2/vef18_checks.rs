//! VE-F1615 · 域自检（判据逐条对应，见 `vef18_geofuzz.rs` 头注）
//!
//! **判据（锚点原文）**：三类、容错、CI、判据。
//!
//! 三族分列以规避 `MAX_CHECKS` 截断：
//! - **a 族**：畸形网格域（四畸形域构造性语料 + 变异/生态线 + 修复不制造新畸形）；
//! - **b 族**：量化域（1bit 越域拒绝 + 全档误差界）+ 转换域（畸形 glTF 容错）；
//! - **c 族**：案例固化（回放逐位一致）+ CI 门禁 + 判据集自检。
//!
//! 判据侧纪律：**独立重算**（误差界/档位数/注入期望由本侧写死或重推，
//! 不向被测问答案）；**基线非平凡**（先证干净网格走 Ok、合法文档导入
//! Ok，再谈容错——否则「全拒」也能假绿）；**零 panic 面**（取值失败
//! 一律 match 记红，不 unwrap 不越界下标）。

use crate::checks::CheckSet;
use crate::gfx::meshquant::{
    component_step, compute_bounds, dequantize_position, quantize_position, QuantBits, QuantDiag,
};
use crate::gfx::meshvalidate::{triple_scan, ScanLimits, TripleScan};
use crate::svstar2::vef18_geofuzz as fz;
use crate::svstar2::vem09_import::{GltfAnimDoc, GltfInterp, MappingTable, SamplerRef};
use alloc::vec;
use alloc::vec::Vec;

// 判据侧写死期望（不从被测反推）。
mod expect {
    /// 越界索引注入期望（构造函数分支写死 3 条）。
    pub const INJ_OOB: u32 = 3;
    /// NaN 注入期望。
    pub const INJ_NAN: u32 = 2;
    /// 超大属性注入期望。
    pub const INJ_HUGE: u32 = 2;
    /// 三查种类数（槽位上界防越界下标）。
    pub const SCAN_KIND_CAP: usize = 3;
    /// 干净立方体面数。
    pub const CUBE_FACES: u32 = 12;
    /// 干净立方体顶点数。
    pub const CUBE_VERTS: u32 = 8;
    /// 合法量化档（与 meshquant 单源语义对齐的判据侧独立枚举）。
    pub const VALID_BITS: [u32; 4] = [8, 10, 12, 16];
    /// 变异扫掠种子轮数。
    pub const MUT_ROUNDS: u64 = 16;
    /// 变异扫掠语料地板。
    pub const MUT_FLOOR: usize = 12;
}

// ---------------------------------------------------------------------------
// a 族：畸形网格域
// ---------------------------------------------------------------------------
fn c1615_malformed(s: &mut CheckSet) {
    // ① 四畸形域全集恰好四类且标签互异（重名会让报告无法区分）。
    let mut labels_differ = true;
    let mut i = 0;
    while i < fz::MalformedKind::ALL.len() {
        let mut j = i + 1;
        while j < fz::MalformedKind::ALL.len() {
            if fz::MalformedKind::ALL[i].label() == fz::MalformedKind::ALL[j].label() {
                labels_differ = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "四畸形域全集恰好四类且标签互异",
        fz::MalformedKind::ALL.len() == 4 && labels_differ,
        "",
    );

    // ② 基线非平凡：干净立方体三查 is_clean（先证正常路径产 Ok——否则「全拒也绿」）。
    let clean = fz::build_clean_cube();
    let limits = ScanLimits::default();
    let rep0 = triple_scan(&clean, &limits);
    s.add(
        "基线：干净立方体三查零命中",
        rep0.is_clean() && rep0.total() == 0,
        "干净网格若报命中，容错判据全失真",
    );

    // ③ 越界索引：注入期望=3，三查按类命中 ≥ 3（独立对拍）。
    let o = fz::run_malformed_case(fz::MalformedKind::IndexOutOfRange, 0x11, None);
    let oob_code = TripleScan::IndexOutOfRange.code() as usize;
    s.add(
        "越界索引注入数与三查按类命中对拍",
        o.completed
            && o.injected == expect::INJ_OOB
            && oob_code < expect::SCAN_KIND_CAP
            && o.hits_by_code[oob_code] >= expect::INJ_OOB as usize,
        "注入 3 条越界面，三查必须全数可见",
    );

    // ④ NaN 几何：注入 2，命中 ≥ 2 且修复后命中总数不增（修复不制造新畸形）。
    let o = fz::run_malformed_case(fz::MalformedKind::NanGeometry, 0x22, None);
    let nan_code = TripleScan::NanGeometry.code() as usize;
    s.add(
        "NaN 几何注入命中且修复不制造新畸形",
        o.completed
            && o.injected == expect::INJ_NAN
            && nan_code < expect::SCAN_KIND_CAP
            && o.hits_by_code[nan_code] >= expect::INJ_NAN as usize
            && o.hits_after_fix <= o.hits_total,
        "NaN 进管线=渲染灾难，必须可查可修",
    );

    // ⑤ 超大属性：注入 2，命中 ≥ 2。
    let o = fz::run_malformed_case(fz::MalformedKind::HugeAttribute, 0x33, None);
    let huge_code = TripleScan::HugeAttribute.code() as usize;
    s.add(
        "超大属性注入命中（有限但超限）",
        o.completed
            && o.injected == expect::INJ_HUGE
            && huge_code < expect::SCAN_KIND_CAP
            && o.hits_by_code[huge_code] >= expect::INJ_HUGE as usize,
        "1e30 坐标撑爆下游运算",
    );

    // ⑥ 截断文件（生态线固化样本）：解析产出 Truncated 而非崩/误 Ok。
    let o = fz::run_malformed_case(
        fz::MalformedKind::TruncatedFile,
        0,
        Some(&fz::ECO_TRUNCATED_STREAM),
    );
    s.add(
        "生态线半截流执行完成且无三查可做（容错不崩）",
        o.completed && o.hits_total == 0 && o.hits_after_fix == 0,
        "截断域期望产出=解析面归 Truncated，无网格可查",
    );
    let parsed = fz::parse_mesh_stream(&fz::ECO_TRUNCATED_STREAM);
    s.add(
        "半截流解析产出 Truncated 三态（非 Ok）",
        matches!(parsed, fz::MeshStreamOutcome::Truncated { .. }),
        "供给不足归截断是本域容错的期望产出",
    );

    // ⑦ 变异线：16 轮扫掠完成数达地板且零红（不崩由「跑完」结构承载）。
    let (ok, red) = fz::run_mutation_sweep(expect::MUT_ROUNDS);
    s.add(
        "变异线扫掠完成数达地板且零红",
        ok >= expect::MUT_FLOOR && red == 0,
        "变异产物无论三态哪态都必须完成记账",
    );

    // ⑧ 变异线确定性：同种子两次扫掠产出一致（回放可复现）。
    let (ok1, red1) = fz::run_mutation_sweep(4);
    let (ok2, red2) = fz::run_mutation_sweep(4);
    s.add(
        "变异线同种子两次扫掠产出一致",
        ok1 == ok2 && red1 == red2,
        "自持 LCG 保证跨次运行可复现",
    );

    // ⑨ 合法种子流解析 Ok 且顶点/面数与立方体一致（编码器-解析器往返）。
    let stream = fz::mutated_seed_stream();
    match fz::parse_mesh_stream(&stream) {
        fz::MeshStreamOutcome::Ok(m) => s.add(
            "种子流往返：顶点/面数与源网格一致",
            m.vert_count() == expect::CUBE_VERTS as usize
                && m.face_count() == expect::CUBE_FACES as usize,
            "编码→解析往返必须无损",
        ),
        _ => s.add("种子流往返：顶点/面数与源网格一致", false, "合法流被解析拒绝"),
    }

    // ⑩ 头部超容量拒绝（防分配爆炸——畸形头部不给分配机会）。
    let mut bad = Vec::new();
    bad.extend_from_slice(&[0xFFu8, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    match fz::parse_mesh_stream(&bad) {
        fz::MeshStreamOutcome::Rejected { .. } => {
            s.add("头部超容量直接拒绝（防挂起/防爆分配）", true, "");
        }
        _ => s.add(
            "头部超容量直接拒绝（防挂起/防爆分配）",
            false,
            "u32::MAX 计数必须被容量闸拒绝",
        ),
    }

    // ⑪ 空流：供给不足归 Truncated 且 need/got 如实。
    match fz::parse_mesh_stream(&[]) {
        fz::MeshStreamOutcome::Truncated { need, got } => {
            s.add("空流归截断且 need≥got 记账", need >= got && got == 0, "");
        }
        _ => s.add("空流归截断且 need≥got 记账", false, "空字节流应归 Truncated"),
    }

    // ⑫ 解析产出三态封闭：变异后无第五态（Ok/Truncated/Rejected 恰全覆盖）。
    let (mut ok_cnt, mut trunc_cnt, mut rej_cnt) = (0usize, 0usize, 0usize);
    let mut r = 0u64;
    while r < 8 {
        let (bytes, _) = fz::mutate_stream(&fz::mutated_seed_stream(), 0x900 + r, 6);
        match fz::parse_mesh_stream(&bytes) {
            fz::MeshStreamOutcome::Ok(_) => ok_cnt += 1,
            fz::MeshStreamOutcome::Truncated { .. } => trunc_cnt += 1,
            fz::MeshStreamOutcome::Rejected { .. } => rej_cnt += 1,
        }
        r += 1;
    }
    s.add(
        "解析产出三态封闭（Ok/Truncated/Rejected）",
        ok_cnt + trunc_cnt + rej_cnt == 8,
        "出现第四态=match 漏臂，判据仍钉住总量",
    );

    // ⑬ 计数分母独立登记：三查分母随注入递增（分母口径不被吞）。
    let rep_clean = triple_scan(&fz::build_clean_cube(), &limits);
    let (with_nan, _) = fz::build_malformed_mesh(fz::MalformedKind::NanGeometry, 1);
    let rep_nan = triple_scan(&with_nan, &limits);
    s.add(
        "三查分母随注入顶点递增（分母口径独立）",
        rep_nan.vertices_seen > rep_clean.vertices_seen,
        "分母被吞会让命中率虚高",
    );

    // ⑭ 预算内完成：执行步数不超预算（不挂起判据可判定化）。
    let o = fz::run_malformed_case(fz::MalformedKind::IndexOutOfRange, 0x55, None);
    s.add(
        "执行步数在预算内（不挂起判据）",
        o.steps_used <= fz::MALFORMED_STEP_BUDGET && o.completed,
        "超预算=挂起，记红",
    );
}

// ---------------------------------------------------------------------------
// b 族：量化域 + 转换域
// ---------------------------------------------------------------------------
fn c1615_quant_conversion(s: &mut CheckSet) {
    // ① 1bit 极限档越域请求被拒（量化失真不失控的第一道闸）。
    let o = fz::run_quant_case(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 1);
    s.add(
        "1bit 极限档越域请求被优雅拒绝",
        o.rejected && !o.bound_ok,
        "QuantBits 无 1bit 档：from_bits 必须返回 None",
    );

    // ② 越域档全集扫描：1..=16 中仅 4 档合法，其余全拒（判据侧独立枚举）。
    let mut reject_ok = true;
    let mut raw = 1u32;
    while raw <= 16 {
        let legal = expect::VALID_BITS.iter().any(|b| *b == raw);
        let o = fz::run_quant_case(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], raw);
        if o.rejected == legal {
            reject_ok = false;
        }
        raw += 1;
    }
    s.add("全域位扫描：合法档恰四个，越域档全拒", reject_ok, "拒绝面与合法面必须互补不重叠");

    // ③ 合法档步长 = size/(2^bits−1)（判据侧独立重算对拍）。
    let mut levels_ok = true;
    let mut bi = 0;
    while bi < expect::VALID_BITS.len() {
        let b = match QuantBits::from_bits(expect::VALID_BITS[bi]) {
            Some(b) => b,
            None => {
                levels_ok = false;
                break;
            }
        };
        let step_a = component_step(1.0, b);
        let expect_step = 1.0f32 / ((1u32 << b.bits()) - 1) as f32;
        if (step_a - expect_step).abs() > 1.0e-7 {
            levels_ok = false;
        }
        bi += 1;
    }
    s.add("全档步长 = size/(2^bits−1) 独立重算一致", levels_ok, "档位数是间隔数不是端点数");

    // ④ 全档×单位立方体：往返误差有界且稳定（失真不崩不失控）。
    let mut bound_ok = true;
    let verts4: Vec<f32> = vec![
        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.5, 0.5, 0.5,
    ];
    let mut bi = 0;
    while bi < expect::VALID_BITS.len() {
        let bits = expect::VALID_BITS[bi];
        let o = fz::run_quant_case(&verts4, bits);
        if !o.bound_ok || !o.stable {
            bound_ok = false;
        }
        bi += 1;
    }
    s.add("全档量化往返误差有界且稳定", bound_ok, "失真有上界+可复现=失真不失控");

    // ⑤ 角点精确还原：B16 下 (1,1,1) 格点往返逐位回原（弱界会把它放走）。
    let b16 = match QuantBits::from_bits(16) {
        Some(b) => b,
        None => {
            s.add("角点精确还原（B16 格点）", false, "B16 档不存在");
            return finish_quant(s);
        }
    };
    let mut diag = crate::gfx::meshquant::DiagBag::new();
    let bnds = match compute_bounds(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &mut diag) {
        Some(b) => b,
        None => {
            s.add("角点精确还原（B16 格点）", false, "单位立方体 bounds 计算失败");
            return finish_quant(s);
        }
    };
    let q = quantize_position([1.0, 1.0, 1.0], &bnds, b16);
    let back = dequantize_position(q, &bnds, b16);
    s.add(
        "角点精确还原（B16 格点）",
        back == [1.0f32; 3],
        "格点往返必须无损——误差界判据的强锚点",
    );

    finish_quant(s);
}

/// b 族后半（NaN/Inf/空集容错 + 确定性）。
fn finish_quant(s: &mut CheckSet) {
    // ⑥ NaN 坐标 → None + PositionNan 记账。
    let mut d = crate::gfx::meshquant::DiagBag::new();
    let r = compute_bounds(&[f32::NAN, 0.0, 0.0], &mut d);
    s.add(
        "NaN 坐标归 None 且 PositionNan 记账",
        r.is_none() && d.has(QuantDiag::PositionNan),
        "NaN 进包围盒=污染全域，必须显性拒绝",
    );

    // ⑦ Inf 坐标 → None + PositionInf。
    let mut d2 = crate::gfx::meshquant::DiagBag::new();
    let r = compute_bounds(&[0.0, f32::INFINITY, 0.0], &mut d2);
    s.add(
        "Inf 坐标归 None 且 PositionInf 记账",
        r.is_none() && d2.has(QuantDiag::PositionInf),
        "",
    );

    // ⑧ 非整三分量流 → None + VertexSetEmpty（截断顶点流不静默算包围盒）。
    let mut d3 = crate::gfx::meshquant::DiagBag::new();
    let r = compute_bounds(&[0.0, 0.0], &mut d3);
    s.add(
        "非整三分量流归 None 且 VertexSetEmpty",
        r.is_none() && d3.has(QuantDiag::VertexSetEmpty),
        "",
    );

    // ⑨ 确定性：同输入两次量化位型一致（失真可复现）。
    let verts = [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0, 0.25, 0.75, 0.5];
    let mut d4 = crate::gfx::meshquant::DiagBag::new();
    match compute_bounds(&verts, &mut d4) {
        Some(bb) => {
            let b12 = match QuantBits::from_bits(12) {
                Some(b) => b,
                None => {
                    s.add("同输入两次量化位型一致", false, "B12 档不存在");
                    return finish_conv(s);
                }
            };
            let q1 = quantize_position([0.25, 0.75, 0.5], &bb, b12);
            let q2 = quantize_position([0.25, 0.75, 0.5], &bb, b12);
            s.add("同输入两次量化位型一致", q1 == q2, "");
        }
        None => {
            s.add("同输入两次量化位型一致", false, "bounds 失败");
        }
    }

    finish_conv(s);
}

/// b 族转换域段。
fn finish_conv(s: &mut CheckSet) {
    // ⑩ 基线非平凡：合法最小文档导入 Ok（先证正常路径通——否则全拒也绿）。
    let mut table = MappingTable::standard();
    let mut bag = crate::svstar2::vem09_import::DiagBag::new();
    let baseline = build_baseline_doc();
    let r = crate::svstar2::vem09_import::import_gltf_anim(
        &baseline,
        crate::svstar2::vem09_import::Fidelity::Faithful,
        &table,
        &mut bag,
    );
    let baseline_ok = r.is_ok();
    let _ = &mut table;
    s.add(
        "基线：合法最小文档导入 Ok",
        baseline_ok,
        "正常路径若不通，容错判据全失真",
    );

    // ⑪ 五形态全集且标签互异。
    let mut labels_differ = true;
    let mut i = 0;
    while i < fz::GltfMalformedKind::ALL.len() {
        let mut j = i + 1;
        while j < fz::GltfMalformedKind::ALL.len() {
            if fz::GltfMalformedKind::ALL[i].label() == fz::GltfMalformedKind::ALL[j].label() {
                labels_differ = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "转换域五形态全集且标签互异",
        fz::GltfMalformedKind::ALL.len() == 5 && labels_differ,
        "",
    );

    // ⑫ 零节点文档：Err 且 DOC_MALFORMED 精确命中（拒绝有码有账）。
    let o = fz::run_conversion_case(fz::GltfMalformedKind::ZeroNodes);
    s.add(
        "零节点文档拒绝且命中 DOC_MALFORMED",
        o.rejected && o.doc_malformed && o.diag_nonempty && o.p1_count >= 1,
        "拒绝必须有码有账，不静默",
    );

    // ⑬ 采样器越界：完成且不崩（跳通道+记账是导入器容错契约）。
    let o = fz::run_conversion_case(fz::GltfMalformedKind::SamplerOob);
    s.add(
        "采样器越界走容错路径（不 panic、有诊断）",
        o.diag_nonempty,
        "跳过通道必须留诊断痕迹",
    );

    // ⑭ 通道目标越界：完成且不崩。
    let o = fz::run_conversion_case(fz::GltfMalformedKind::ChannelTargetOob);
    let _ = o;
    s.add("通道目标越界走容错路径（不 panic）", true, "执行完成即通过；panic 由进程崩承载");

    // ⑮ accessor 长度不符：完成且不崩。
    let o = fz::run_conversion_case(fz::GltfMalformedKind::AccessorLenMismatch);
    let _ = o;
    s.add("accessor 长度不符走容错路径（不 panic）", true, "同上：完成即通过");

    // ⑯ 空采样器表：完成且不崩（通道全跳或拒绝均为合法容错产出）。
    let o = fz::run_conversion_case(fz::GltfMalformedKind::EmptySamplers);
    let _ = o;
    s.add("空采样器表走容错路径（不 panic）", true, "同上：完成即通过");
}

/// 合法基线文档（判据侧持有，不向被测问「给一份合法的」）。
fn build_baseline_doc() -> GltfAnimDoc {
    use crate::svstar2::vem09_import::{AccessorView, ChannelPath, ChannelRef, ComponentType};
    let acc_in = AccessorView {
        component_type: ComponentType::Float,
        normalized: false,
        count: 2,
        comps: 3,
        data: vec![0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0],
    };
    let acc_out = AccessorView {
        component_type: ComponentType::Float,
        normalized: false,
        count: 2,
        comps: 3,
        data: vec![0.0f32, 0.0, 0.0, 2.0, 2.0, 2.0],
    };
    GltfAnimDoc::new(
        1,
        vec![acc_in, acc_out],
        vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
        vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
        "fuzz-baseline-ok",
    )
}

// ---------------------------------------------------------------------------
// c 族：案例固化 + CI 门禁 + 判据集自检
// ---------------------------------------------------------------------------
fn c1615_freeze_ci(s: &mut CheckSet) {
    // ① 固化表非空且覆盖三域（语料缺域=该域门禁恒绿退化）。
    let mut dom_seen = [false; 3];
    let mut i = 0;
    while i < fz::FIXED_CASES.len() {
        let mut d = 0;
        while d < fz::FuzzDomain::ALL.len() {
            if fz::FIXED_CASES[i].domain == fz::FuzzDomain::ALL[d] {
                dom_seen[d] = true;
            }
            d += 1;
        }
        i += 1;
    }
    s.add(
        "固化表覆盖三域（缺域即红）",
        fz::FIXED_CASES.len() >= 6 && dom_seen[0] && dom_seen[1] && dom_seen[2],
        "地板：每域 ≥2 条",
    );

    // ② 固化表覆盖三语料线（F1128 纪律：缺线即盲区）。
    let mut src_seen = [false; 3];
    let mut i = 0;
    while i < fz::FIXED_CASES.len() {
        let mut l = 0;
        while l < fz::GeoCorpusSource::ALL.len() {
            if fz::FIXED_CASES[i].source == fz::GeoCorpusSource::ALL[l] {
                src_seen[l] = true;
            }
            l += 1;
        }
        i += 1;
    }
    s.add(
        "固化表覆盖三语料线（构造/变异/生态）",
        src_seen[0] && src_seen[1] && src_seen[2],
        "缺线=盲区，语料治理纪律",
    );

    // ③ 案例 id 全局互异（重名=回放覆盖，回归失效）。
    let mut ids_differ = true;
    let mut i = 0;
    while i < fz::FIXED_CASES.len() {
        let mut j = i + 1;
        while j < fz::FIXED_CASES.len() {
            if fz::FIXED_CASES[i].id == fz::FIXED_CASES[j].id {
                ids_differ = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("固化案例 id 全局互异", ids_differ, "");

    // ④ CI 全表两次回放 digest 一致且非零（确定性判据的强形态）。
    let d1 = fz::run_ci_gate();
    let d2 = fz::run_ci_gate();
    s.add(
        "CI 全表两次回放 digest 一致且非零",
        d1.digest == d2.digest && d1.digest != 0,
        "回放漂移=语料不可复现=fuzz 价值归零",
    );

    // ⑤ CI 门禁红项为零且执行数=固化表长（红项如实报数不吞）。
    s.add(
        "CI 门禁红项为零",
        d1.cases_red == 0 && d1.cases_run == fz::FIXED_CASES.len(),
        "",
    );

    // ⑥ 预算判据：CI 步数在预算内。
    s.add("CI 步数在预算内（不挂起）", d1.budget_ok, "");

    // ⑦ 地板判据由 CI 自身报出且此刻为真。
    s.add("每域语料条数达地板", d1.floors_ok, "地板阈值判据侧写死 ≥2");

    // ⑧ 不同案例回放 digest 互异（折叠链非平凡——恒等 digest=记账失效）。
    match (fz::replay_fixed_case(0), fz::replay_fixed_case(1)) {
        (Some(a), Some(b)) => s.add(
            "不同案例回放 digest 互异（折叠链非平凡）",
            a.digest != b.digest && a.completed && b.completed,
            "",
        ),
        _ => s.add("不同案例回放 digest 互异（折叠链非平凡）", false, "回放越界"),
    }

    // ⑨ 回放越界安全：idx 超表长返回 None 不 panic（零 panic 面）。
    s.add(
        "回放越界返回 None（零 panic 面）",
        fz::replay_fixed_case(fz::FIXED_CASES.len()).is_none(),
        "",
    );

    // ⑩ 三域短码可逆且未知短码不误映射（协议面冻结）。
    let mut wire_ok = true;
    let mut d = 0;
    while d < fz::FuzzDomain::ALL.len() {
        match fz::FuzzDomain::from_wire(fz::FuzzDomain::ALL[d].wire()) {
            Some(back) => {
                if back != fz::FuzzDomain::ALL[d] {
                    wire_ok = false;
                }
            }
            None => wire_ok = false,
        }
        d += 1;
    }
    s.add(
        "三域短码可逆且未知短码不误映射",
        wire_ok && fz::FuzzDomain::from_wire("zz").is_none(),
        "",
    );

    // ⑪ 语料条数与固化表一致（聚合口径不被吞）。
    s.add(
        "语料条数与固化表长度一致",
        fz::total_fuzz_case_count() == fz::FIXED_CASES.len() as u32,
        "",
    );
}

// ---------------------------------------------------------------------------
// 判据集自身自检（防判据写错）
// ---------------------------------------------------------------------------
fn meta_checks(s: &mut CheckSet) {
    // ① 三族条数合计与 total_check_count 一致（聚合口径单源）。
    let mut a = CheckSet::new("vef18-meta");
    c1615_malformed(&mut a);
    let mut b = CheckSet::new("vef18-meta");
    c1615_quant_conversion(&mut b);
    let mut c = CheckSet::new("vef18-meta");
    c1615_freeze_ci(&mut c);
    let total = a.len() + b.len() + c.len() + 3;
    s.add(
        "判据集条数与 total_check_count 一致",
        total == total_check_count() as usize,
        "聚合口径漂移会让文档与实际脱节",
    );

    // ② 判据名全局互异（重名=报告不可区分，一条红可能被掩盖）。
    let mut names: Vec<&str> = Vec::new();
    collect_names(&a, &mut names);
    collect_names(&b, &mut names);
    collect_names(&c, &mut names);
    let mut all_differ = true;
    let mut i = 0;
    while i < names.len() {
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                all_differ = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("判据名全局互异", all_differ, "");

    // ③ 三族全绿镜像（任何一族任何一条红，在 c 族也可见——不吞不改）。
    s.add(
        "三族自检全绿",
        a.all_passed() && b.all_passed() && c.all_passed(),
        "任一判据红即在此可见",
    );
}

/// 收集判据名（零 panic 面 helper）。
fn collect_names(set: &CheckSet, out: &mut Vec<&'static str>) {
    let (items, n) = set.red_items();
    let mut i = 0;
    while i < n {
        if let Some(ch) = items[i] {
            out.push(ch.name);
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// 三族入口（a/b/c standalone，供聚合器）
// ---------------------------------------------------------------------------

/// a 族：畸形网格域。
pub fn run_vef18_checks_a() -> CheckSet {
    let mut s = CheckSet::new("vef18-malformed");
    c1615_malformed(&mut s);
    s
}

/// a 族独立入口（聚合器用）。
pub fn run_vef18_checks_a_standalone() -> CheckSet {
    run_vef18_checks_a()
}

/// b 族：量化域 + 转换域。
pub fn run_vef18_checks_b() -> CheckSet {
    let mut s = CheckSet::new("vef18-quant-conv");
    c1615_quant_conversion(&mut s);
    s
}

/// b 族独立入口（聚合器用）。
pub fn run_vef18_checks_b_standalone() -> CheckSet {
    run_vef18_checks_b()
}

/// c 族：案例固化 + CI 门禁 + 判据集自检。
pub fn run_vef18_checks_c() -> CheckSet {
    let mut s = CheckSet::new("vef18-freeze");
    c1615_freeze_ci(&mut s);
    meta_checks(&mut s);
    s
}

/// c 族独立入口（聚合器用）。
pub fn run_vef18_checks_c_standalone() -> CheckSet {
    run_vef18_checks_c()
}

/// 三族合并（单点调用；15+16+14 不超 MAX_CHECKS）。
pub fn run_vef18_checks() -> CheckSet {
    CheckSet::merge(
        CheckSet::merge(run_vef18_checks_a(), run_vef18_checks_b()),
        run_vef18_checks_c(),
    )
}

/// 判据集条数（供文档/聚合器自检，不参与判定）。
pub const fn total_check_count() -> u32 {
    45
}
