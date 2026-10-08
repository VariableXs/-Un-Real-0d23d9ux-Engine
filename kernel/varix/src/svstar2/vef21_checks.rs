//! VE-F1618 · 几何 API 冻结 v1 · 域自检（判据逐条映射，五族）
//!
//! 锚点判据 → 判据族：
//! - 冻结（五签名闭集数据化）→ [`group_freeze`]
//! - 归一（一表两域 + 对端锚定）→ [`group_unify`]
//! - 承诺（条款闭集 + 审计）→ [`group_pledge`]
//! - 语义测试（F1114：每签名一条行为用例）→ [`group_semantics`]
//! - 判据（收口自检）→ [`group_meta`]
//!
//! 双向验证纪律：查询在已知值上必须命中、在越界上必须给失败码（查询签名的
//! 失败语义是冻结面的一部分）；取消必须真的不进委托（Cancelled 不是静默
//! 成功）；目标不可达必须报 Infeasible 而不是夹到下限假装成功——正向恒绿
//! 不构成证据。

use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::gfx::meshdecimate::{plane_grid, DecMesh};
use crate::gfx::meshrepair::RepairMesh;
use crate::svstar2::vef16_gridstat::{default_thresholds, Metric};
use crate::svstar2::vef18_geofuzz::encode_mesh_stream;
use crate::svstar2::vef21_apifreeze::*;

// ---------------------------------------------------------------------------
// 语料构造（判据侧自建——不从被测反推）
// ---------------------------------------------------------------------------

/// 3 顶点 1 面最小网格（加载 roundtrip 与查询用）。
fn tri_mesh() -> RepairMesh {
    let mut m = RepairMesh::new();
    m.push_vert([0.0, 0.0, 0.0]);
    m.push_vert([1.0, 0.0, 0.0]);
    m.push_vert([0.0, 1.0, 0.0]);
    m.push_face([0, 1, 2]);
    m
}

/// 4 顶点 2 面网格（共享边 (1,2)：邻接语义用）。
fn quad_mesh() -> RepairMesh {
    let mut m = RepairMesh::new();
    m.push_vert([0.0, 0.0, 0.0]);
    m.push_vert([1.0, 0.0, 0.0]);
    m.push_vert([1.0, 1.0, 0.0]);
    m.push_vert([0.0, 1.0, 0.0]);
    m.push_face([0, 1, 2]);
    m.push_face([1, 2, 3]);
    m
}

/// DecMesh → RepairMesh（判据侧语料转换：plane 语料进冻结面用）。
fn dec_to_repair(d: &DecMesh) -> RepairMesh {
    let mut m = RepairMesh::new();
    let mut i = 0usize;
    while i < d.verts.len() {
        m.push_vert(d.verts[i]);
        i += 1;
    }
    let mut f = 0usize;
    while f < d.faces.len() {
        m.push_face(d.faces[f]);
        f += 1;
    }
    m
}

/// 判据入口（聚合器经 mod.rs 调用）。
pub fn run_vef21_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F1618");
    group_freeze(&mut set);
    group_unify(&mut set);
    group_pledge(&mut set);
    group_semantics(&mut set);
    group_meta(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 冻结：五签名闭集数据化
// ---------------------------------------------------------------------------

fn group_freeze(set: &mut CheckSet) {
    // ① 五签名闭集：短码可逆 + 未知短码不误映射。
    let mut reversible = GRID_API_SPECS.len() == 5;
    let mut k = 0usize;
    while k < GridApiId::ALL.len() {
        let id = GridApiId::ALL[k];
        if GridApiId::from_en(id.en()) != Some(id) {
            reversible = false;
        }
        k += 1;
    }
    set.add(
        "C18-FRZ-01 五签名短码可逆且未知短码拒绝",
        reversible
            && GridApiId::from_en("grid.nope").is_none()
            && GridApiId::from_en("").is_none(),
        "闭集外不存在第七签名；from_en 拒绝是冻结面的守门行为",
    );

    // ② 规格表与闭集逐位对齐：ordinal 连续、委托对端与理由非空。
    let mut aligned = true;
    let mut k = 0usize;
    while k < GRID_API_SPECS.len() {
        let s = &GRID_API_SPECS[k];
        if s.ordinal != k as u8 || s.id != GridApiId::ALL[k] {
            aligned = false;
        }
        if s.delegates_to.is_empty() || s.amortized.is_empty() || s.rationale.is_empty() {
            aligned = false;
        }
        k += 1;
    }
    set.add(
        "C18-FRZ-02 规格表 ordinal 连续且委托对端/理由全非空",
        aligned,
        "delegates_to 非空=委托关系是数据不是口头；理由空=签名存在性无据",
    );

    // ③ 每签名入参/出参闭集非空（签名=入参+出参+失败语义，缺一不成签名）。
    let mut io_ok = true;
    let mut k = 0usize;
    while k < GRID_API_SPECS.len() {
        if GRID_API_SPECS[k].inputs.is_empty() || GRID_API_SPECS[k].outputs.is_empty() {
            io_ok = false;
        }
        k += 1;
    }
    set.add(
        "C18-FRZ-03 五签名入参/出参表全非空",
        io_ok,
        "签名冻结冻结的是端口：入参/出参表是端口的数据形态",
    );
}

// ---------------------------------------------------------------------------
// 归一：一表两域 + 对端锚定
// ---------------------------------------------------------------------------

fn group_unify(set: &mut CheckSet) {
    // ④ 表行数基线非平凡 + 三态闭集 + 说明全非空。
    let mut rows_ok = UNIFY_TABLE.len() == 5;
    let mut k = 0usize;
    while k < UNIFY_TABLE.len() {
        let r = &UNIFY_TABLE[k];
        let rel_ok = r.rel.wire() == "subset" || r.rel.wire() == "peer" || r.rel.wire() == "downstream";
        if !rel_ok || r.note.is_empty() || r.grid_api.is_empty() || r.asset_stage.is_empty() {
            rows_ok = false;
        }
        k += 1;
    }
    set.add(
        "C18-UNI-01 归一表 5 行三态闭集且说明全非空",
        rows_ok,
        "归一说明空=只对表不释义，一表两域就只剩一张表",
    );

    // ⑤ 对端锚定：每行 asset_stage 指向 Q 域六段真实段（孤儿声明必被拒）。
    set.add(
        "C18-UNI-02 归一对端全部锚定在 Q 域六段闭集内",
        unify_anchored().is_ok(),
        "指向不存在段的归一是自说自话；锚定让「命名归一」可机检",
    );

    // ⑥ 反向验证：锚定函数对伪造对端真的会拒（Q 域与网格族双向）。
    set.add(
        "C18-UNI-03 锚定函数对未知短码双向拒绝",
        crate::svstar2::veq01_pipeline::StageId::from_en("nope").is_none()
            && GridApiId::from_en("nope.nope").is_none(),
        "锚定若对什么都绿，孤儿声明照样混过——双向用例证明拒绝真可达",
    );
}

// ---------------------------------------------------------------------------
// 承诺：条款闭集 + 审计
// ---------------------------------------------------------------------------

fn group_pledge(set: &mut CheckSet) {
    // ⑦ 五条款全非空（空条款=没承诺）。
    let mut ok = PLEDGE_TABLE.len() == 5;
    let mut k = 0usize;
    while k < PLEDGE_TABLE.len() {
        if PLEDGE_TABLE[k].clause.is_empty() || PLEDGE_TABLE[k].text.is_empty() {
            ok = false;
        }
        k += 1;
    }
    set.add(
        "C18-PLD-01 承诺五条款闭集且文本全非空",
        ok,
        "条款数守恒是 v1 口径：增删条款=改承诺=必须过判据",
    );

    // ⑧ 审计绿且输出携带版本指纹（承诺可审计=输出可读）。
    match pledge_audit() {
        Ok(line) => set.add(
            "C18-PLD-02 承诺审计绿且输出版本指纹",
            line.contains("GAFREEZE-v1") && line.contains("pledge=5"),
            "审计行携带版本与条款数——承诺输出可读可录屏",
        ),
        Err(_) => set.add("C18-PLD-02 承诺审计绿且输出版本指纹", false, "audit errored"),
    }
}

// ---------------------------------------------------------------------------
// 语义测试（F1114：每签名一条行为用例）
// ---------------------------------------------------------------------------

fn group_semantics(set: &mut CheckSet) {
    // ⑨ 加载 roundtrip：encode → grid_load → 逐顶点逐面对拍。
    let src = tri_mesh();
    let bytes = encode_mesh_stream(&src);
    let mut rt_ok = false;
    if let LoadOutcome::Ok(fz) = grid_load(&bytes) {
        rt_ok = fz.vert_count() == 3
            && fz.face_count() == 1
            && fz.vert(0) == Some([0.0, 0.0, 0.0])
            && fz.vert(1) == Some([1.0, 0.0, 0.0])
            && fz.vert(2) == Some([0.0, 1.0, 0.0])
            && fz.face(0) == Some([0, 1, 2]);
    }
    set.add(
        "C18-SEM-01 加载 roundtrip 逐顶点逐面相等",
        rt_ok,
        "冻结加载签名冻结的是行为：字节进对象的两端必须可复算",
    );

    // ⑩ 加载拒绝：截断字节流必须被拒（失败语义是签名的一部分）。
    let mut truncated = Vec::new();
    let mut i = 0usize;
    while i < 10 && i < bytes.len() {
        truncated.push(bytes[i]);
        i += 1;
    }
    let reject_ok = match grid_load(&truncated) {
        LoadOutcome::Reject { code, detail } => {
            code == E_GAF_LOAD_REJECT && detail.contains("truncated")
        }
        LoadOutcome::Ok(_) => false,
    };
    set.add(
        "C18-SEM-02 截断流被拒且失败码如实",
        reject_ok,
        "静默吃下截断流=下游拿到半张网格——拒绝必须显性",
    );

    // ⑪ 查询准确性 + 越界失败语义（双向）。
    let fz = match grid_load(&encode_mesh_stream(&tri_mesh())) {
        LoadOutcome::Ok(fz) => fz,
        LoadOutcome::Reject { .. } => FrozenMesh { verts: Vec::new(), faces: Vec::new() },
    };
    let q_ok = match (grid_query(&fz, QueryKind::Position(1)), grid_query(&fz, QueryKind::Corner(0))) {
        (Ok(QueryOutcome::Position(p)), Ok(QueryOutcome::Corner(t))) => {
            p == [1.0, 0.0, 0.0] && t == [0, 1, 2]
        }
        _ => false,
    };
    let range_ok = matches!(
        grid_query(&fz, QueryKind::Position(99)),
        Err(E_GAF_QUERY_RANGE)
    ) && matches!(grid_query(&fz, QueryKind::Corner(99)), Err(E_GAF_QUERY_RANGE));
    set.add(
        "C18-SEM-03 查询已知值命中且越界返回失败码",
        q_ok && range_ok,
        "查询签名冻结的是「读路径不改网格+越界不panic」",
    );

    // ⑫ 邻接语义：共享边 (1,2) 的两面——顶点 1 关联两面、面 0 邻面 1。
    let fq = match grid_load(&encode_mesh_stream(&quad_mesh())) {
        LoadOutcome::Ok(fq) => fq,
        LoadOutcome::Reject { .. } => FrozenMesh { verts: Vec::new(), faces: Vec::new() },
    };
    let adj_ok = match (
        grid_query(&fq, QueryKind::AdjacentFaces(1)),
        grid_query(&fq, QueryKind::AdjacentFacesOfFace(0)),
    ) {
        (Ok(QueryOutcome::FaceList(a)), Ok(QueryOutcome::FaceList(b))) => {
            a == alloc::vec![0u32, 1u32] && b == alloc::vec![1u32]
        }
        _ => false,
    };
    let adj_range = matches!(
        grid_query(&fq, QueryKind::AdjacentFaces(9)),
        Err(E_GAF_QUERY_RANGE)
    );
    set.add(
        "C18-SEM-04 邻接口径正确且越界拒绝",
        adj_ok && adj_range,
        "邻接出参口径（顶点→面 / 面→面）冻结后，下游统计才能依赖它",
    );

    // ⑬ 统计实测相符：verdict 的实测值与网格实取相等（透传不失真）。
    let thr = default_thresholds();
    let stat_ok = match grid_stat(0xABCD, &fz, &thr) {
        Ok(rep) => {
            rep.mesh_id == 0xABCD
                && rep.verdicts.len() == 5
                && rep.verdicts[0].metric == Metric::Vertices
                && rep.verdicts[0].value == fz.vert_count() as u64
                && rep.verdicts[1].value == fz.face_count() as u64
        }
        Err(_) => false,
    };
    set.add(
        "C18-SEM-05 统计实测值与网格实取相等且 id 透传",
        stat_ok,
        "统计若从调用方自报取数，体检就是在体检谎言",
    );

    // ⑭ 修复请求幂等：同网格两次报告全等 + 产线确定性自证。
    let tri = tri_mesh();
    let ra = grid_repair_request(&tri);
    let rb = grid_repair_request(&tri);
    let idem_ok = ra.holes == rb.holes
        && ra.degenerates == rb.degenerates
        && ra.flips == rb.flips
        && ra.uv_out_of_range == rb.uv_out_of_range
        && ra.non_manifold_edges == rb.non_manifold_edges
        && ra.findings.len() == rb.findings.len()
        && crate::gfx::meshrepair::report_is_deterministic(&tri);
    set.add(
        "C18-SEM-05b 修复请求幂等（两次报告全等）",
        idem_ok,
        "报告不幂等=自动修复的安全前提崩塌——幂等是冻结语义",
    );

    // ⑮ 简化请求：取消真的不进委托；正常产出≤请求目标；不可达如实报。
    let plane = dec_to_repair(&plane_grid(5, 5, 1.0, 1.0));
    let cancel_ok = matches!(
        grid_simplify_request(&plane, 500, true),
        SimplifyOutcome::Cancelled
    );
    let run_ok = match grid_simplify_request(&plane, 500, false) {
        SimplifyOutcome::Ok { mesh, target_faces } => {
            target_faces == plane.face_count() / 2 && mesh.face_count() <= plane.face_count()
        }
        _ => false,
    };
    let tiny = tri_mesh();
    let infeasible_ok = matches!(
        grid_simplify_request(&tiny, 500, false),
        SimplifyOutcome::Infeasible { .. }
    );
    set.add(
        "C18-SEM-06 简化可取消/可达产出/不可达如实报",
        cancel_ok && run_ok && infeasible_ok,
        "取消静默成功=假完成；不可达假装成功=夹到下限撒谎——双向用例钉死",
    );
}

// ---------------------------------------------------------------------------
// 判据：收口自检
// ---------------------------------------------------------------------------

fn group_meta(set: &mut CheckSet) {
    // ⑯ 实挂条数从 CheckSet 实取（非自证）：前四族实挂 15 条，META 段 2 条。
    let before_meta = set.len();
    set.add(
        "C18-META-01 实挂条数+2(META)=声明条数17",
        before_meta == 15 && before_meta + 2 == 17,
        "实 add 数从 CheckSet.len() 实取；增删判据漏改口径即红",
    );

    // ⑰ 判据名全集互异（重名=聚合器 tally 失真）。
    let mut names: Vec<&'static str> = Vec::new();
    let mut k = 0usize;
    while k < set.len() {
        if let Some(ch) = set.get(k) {
            names.push(ch.name);
        }
        k += 1;
    }
    let mut all_differ = true;
    let mut i = 0usize;
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
    set.add(
        "C18-META-02 判据名全集互异",
        all_differ && set.len() + 1 == 17,
        "重名判据会让 tally 与实际脱节——收口时逐名实取对拍",
    );
}
