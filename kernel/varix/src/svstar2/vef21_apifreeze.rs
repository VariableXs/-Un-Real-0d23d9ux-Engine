//! VE-F1618 · 几何 API 冻结 v1（VE 几何族 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1618`
//!
//! **判据（锚点原文）**：冻结、归一、承诺、判据。
//!
//! **职责定位（锚点原文）**：网格族冻结（加载/查询/统计/修复请求/简化请求
//! ——签名冻结版本化）；与资产管线 Q 域接口的关系（网格族是资产族的几何
//! 子集——命名归一）；十年承诺。
//!
//! # 一、签名冻结是**数据**不是注释：签名表可机检，改签名必红
//!
//! 「十年承诺」若只写在注释里，改一行签名没有任何机制拦住。故五签名写进
//! [`GRID_API_SPECS`] 数据表（与 Q 域 [`veq01_pipeline::STAGE_SPECS`] 同构
//! ——一表两域的「表」就是结构同构）：每签名的入参名、出参名、委托对端、
//! 摊销复杂度、排他性理由逐条登记。增删签名或改签名描述，判据侧的闭集
//! 断言与条数基线立刻红——**冻结因此是可验证状态，不是愿望**。
//!
//! # 二、薄层委托：冻结面不复制产线，只钉端口
//!
//! 五签名全部委托既有产线实现（F1615 流解析 / F1607 修复检测 / F1608 简化 /
//! F1613 统计体检），冻结面自身零算法。这是刻意的：委托意味着产线升级
//! 自动获益，冻结面不会变成第二套分叉实现；签名表里的 `delegates_to` 把
//! 委托关系写成数据，判据对每条非空断言——「委托」因此可机检。
//!
//! # 三、语义测试锁定 F1114 纪律：每签名配一条行为用例
//!
//! 签名冻结冻结的是**行为**不是函数名。每签名一条语义用例：加载 roundtrip
//! 逐字节、查询已知值、统计与网格实测相符、修复请求幂等、简化请求可取消。
//! 用例在本单判据侧（[`vef21_checks`]），行为变了签名就变了，冻结即破。
//!
//! # 四、十年承诺：条款闭集 + 版本戳，承诺本身可审计
//!
//! [`PLEDGE_TABLE`] 五条款闭集（签名不变/破坏走 ADR/废弃需超版/降级标注/
//! 委托对端不静默换），[`pledge_audit`] 逐条非空校验并输出版本指纹。
//! 承诺文本是契约的一部分：空条款=没承诺，判据红。
//!
//! ## 零 panic 面
//!
//! 冻结面全部走 `Option`/`Result`：无 `unwrap`/`expect`/切片直下标，
//! 越界查询返回 `None`（查询签名的失败语义），解析失败返回错误码。

use alloc::format;
use alloc::string::String;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::gfx::meshdecimate::{self, DecMesh, SimplifyOptions};
use crate::gfx::meshrepair::{self, DefectReport, RepairMesh};
use crate::svstar2::vef16_gridstat::{self, MeshStat, ThresholdTable};
use crate::svstar2::vef18_geofuzz::{parse_mesh_stream, MeshStreamOutcome};

// ===========================================================================
// 一、版本与失败码
// ===========================================================================

/// 冻结版本（签名集一经 v1 发布即受十年承诺约束）。
pub const GAFREEZE_VERSION: &str = "GAFREEZE-v1";

/// 加载失败（流解析三态中非 Ok 的统称——截断与拒绝都归此码，细节在字段）。
pub const E_GAF_LOAD_REJECT: &str = "E_GAF_LOAD_REJECT";
/// 查询越界（句柄不含所请求的元素——查询签名的失败语义）。
pub const E_GAF_QUERY_RANGE: &str = "E_GAF_QUERY_RANGE";
/// 简化请求被取消（「可取消」的显性失败语义，不是静默吞掉）。
pub const E_GAF_CANCELLED: &str = "E_GAF_CANCELLED";

// ===========================================================================
// 二、五签名闭集（判据「冻结」的数据载体）
// ===========================================================================

/// 网格族五签名（闭集；顺序即语义，不可调换——与 Q 域六段同纪律）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridApiId {
    /// 加载：vmesh/glTF 字节流 → 网格对象。
    Load,
    /// 查询：属性/拓扑/邻接。
    Query,
    /// 统计：五项聚合量 + F1613 体检。
    Stat,
    /// 修复请求：缺陷检测（F1607 委托）。
    RepairRequest,
    /// 简化请求：目标面数简化（F1608 委托），可取消。
    SimplifyRequest,
}

impl GridApiId {
    /// 全集（顺序即 [`GRID_API_SPECS`] 的 ordinal 口径）。
    pub const ALL: [GridApiId; 5] = [
        GridApiId::Load,
        GridApiId::Query,
        GridApiId::Stat,
        GridApiId::RepairRequest,
        GridApiId::SimplifyRequest,
    ];

    /// 英文短码（归一声明与资产族命名对齐的载体）。
    pub const fn en(self) -> &'static str {
        match self {
            GridApiId::Load => "grid.load",
            GridApiId::Query => "grid.query",
            GridApiId::Stat => "grid.stat",
            GridApiId::RepairRequest => "grid.repair_request",
            GridApiId::SimplifyRequest => "grid.simplify_request",
        }
    }

    /// 中文名（诊断文案与读屏共用）。
    pub const fn zh(self) -> &'static str {
        match self {
            GridApiId::Load => "加载",
            GridApiId::Query => "查询",
            GridApiId::Stat => "统计",
            GridApiId::RepairRequest => "修复请求",
            GridApiId::SimplifyRequest => "简化请求",
        }
    }

    /// 由短码反查（未知短码 `None`——闭集外不存在第七签名）。
    pub fn from_en(s: &str) -> Option<GridApiId> {
        Some(match s {
            "grid.load" => GridApiId::Load,
            "grid.query" => GridApiId::Query,
            "grid.stat" => GridApiId::Stat,
            "grid.repair_request" => GridApiId::RepairRequest,
            "grid.simplify_request" => GridApiId::SimplifyRequest,
            _ => return None,
        })
    }
}

/// 一个签名的冻结规格（v1）。
#[derive(Clone, Copy, Debug)]
pub struct GridApiSpec {
    /// 签名标识。
    pub id: GridApiId,
    /// 序位（0..5，与 [`GridApiId::ALL`] 对齐）。
    pub ordinal: u8,
    /// 入参名（按序）。
    pub inputs: &'static [&'static str],
    /// 出参名（按序）。
    pub outputs: &'static [&'static str],
    /// 委托对端（产线实现路径——非空=委托关系是数据不是口头）。
    pub delegates_to: &'static str,
    /// 摊销复杂度声明。
    pub amortized: &'static str,
    /// 排他性理由（这一签名为什么单独存在）。
    pub rationale: &'static str,
}

/// 五签名冻结规格表（v1 唯一事实源）。
pub const GRID_API_SPECS: [GridApiSpec; 5] = [
    GridApiSpec {
        id: GridApiId::Load,
        ordinal: 0,
        inputs: &["bytes", "formatHint"],
        outputs: &["mesh", "loadStats"],
        delegates_to: "vef18_geofuzz::parse_mesh_stream",
        amortized: "O(bytes)",
        rationale: "字节进网格对象的唯一合法端口：容量护栏与三态失败语义在此一处，其余入口不得另开解析",
    },
    GridApiSpec {
        id: GridApiId::Query,
        ordinal: 1,
        inputs: &["mesh", "queryKind"],
        outputs: &["queryResult"],
        delegates_to: "gfx::meshrepair::RepairMesh 访问器",
        amortized: "O(1) 单点 / O(V+F) 邻接构建",
        rationale: "读路径与写路径分离：查询不改网格，越界返回空而不是panic——冻结的是失败语义",
    },
    GridApiSpec {
        id: GridApiId::Stat,
        ordinal: 2,
        inputs: &["mesh", "thresholds"],
        outputs: &["statReport"],
        delegates_to: "vef16_gridstat::inspect_mesh",
        amortized: "O(1) 聚合 + O(表) 体检",
        rationale: "统计口径单源 F1613：体检阈值与判定不进本面，网格族只负责把实测聚合量送进同一真相",
    },
    GridApiSpec {
        id: GridApiId::RepairRequest,
        ordinal: 3,
        inputs: &["mesh"],
        outputs: &["defectReport"],
        delegates_to: "gfx::meshrepair::detect_all",
        amortized: "O(V+F)",
        rationale: "只报告不修正（F1607 纪律）：修正动作在请求-确认之后，报告幂等是自动修复的安全前提",
    },
    GridApiSpec {
        id: GridApiId::SimplifyRequest,
        ordinal: 4,
        inputs: &["mesh", "targetRatio", "cancelFlag"],
        outputs: &["simplifiedMesh", "quality"],
        delegates_to: "gfx::meshdecimate::simplify",
        amortized: "O(V+F) 每轮收缩",
        rationale: "可取消是签名的一部分：长任务必须能在帧边界被叫停，取消是显性结果不是静默丢弃",
    },
];

// ===========================================================================
// 三、归一声明（判据「归一」：网格族是资产族的几何子集——一表两域）
// ===========================================================================

/// 归一关系三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnifyRel {
    /// 网格签名是资产族对应段的前缀子集（语义收窄）。
    Subset,
    /// 平级对端（各自域内同构签名，数据可直转）。
    Peer,
    /// 下游消费（资产族产出经网格签名进入几何处理）。
    Downstream,
}

impl UnifyRel {
    /// 短码（表内登记与判据共用）。
    pub const fn wire(self) -> &'static str {
        match self {
            UnifyRel::Subset => "subset",
            UnifyRel::Peer => "peer",
            UnifyRel::Downstream => "downstream",
        }
    }
}

/// 归一声明行：网格签名 ↔ 资产族六段（Q 域 [`veq01_pipeline::StageId`]）。
#[derive(Clone, Copy, Debug)]
pub struct UnifyRow {
    /// 网格签名短码。
    pub grid_api: &'static str,
    /// 资产族对端段短码。
    pub asset_stage: &'static str,
    /// 关系。
    pub rel: UnifyRel,
    /// 归一说明（命名对齐点与差异——非空纪律）。
    pub note: &'static str,
}

/// 归一声明表（一表两域：本表同时是网格族与资产族的对接文档）。
pub const UNIFY_TABLE: [UnifyRow; 5] = [
    UnifyRow {
        grid_api: "grid.load",
        asset_stage: "load",
        rel: UnifyRel::Subset,
        note: "资产族加载段涵盖全部资源种类；grid.load 只收几何字节流，是同名的几何子集",
    },
    UnifyRow {
        grid_api: "grid.query",
        asset_stage: "verify",
        rel: UnifyRel::Peer,
        note: "资产族校验段产出校验诊断；网格查询只读不改——语义同构（读路径），数据可直转",
    },
    UnifyRow {
        grid_api: "grid.stat",
        asset_stage: "verify",
        rel: UnifyRel::Downstream,
        note: "统计体检消费加载产物：资产族 verify 通过是 grid.stat 有意义的前提",
    },
    UnifyRow {
        grid_api: "grid.repair_request",
        asset_stage: "request",
        rel: UnifyRel::Peer,
        note: "修复请求与资产请求同为「登记-裁决」形态：请求先行、动作在确认之后",
    },
    UnifyRow {
        grid_api: "grid.simplify_request",
        asset_stage: "schedule",
        rel: UnifyRel::Peer,
        note: "简化请求带取消语义，与资产调度段的可中断任务同构（帧边界叫停）",
    },
];

/// 归一锚定校验：每行的 asset_stage 必须指向 Q 域六段闭集内的真实段
/// （指向不存在段的归一声明是孤儿——「命名归一」若不锚定对端就是自说自话）。
pub fn unify_anchored() -> Result<(), &'static str> {
    for row in UNIFY_TABLE.iter() {
        if crate::svstar2::veq01_pipeline::StageId::from_en(row.asset_stage).is_none() {
            return Err(row.grid_api);
        }
        if GridApiId::from_en(row.grid_api).is_none() {
            return Err(row.asset_stage);
        }
    }
    Ok(())
}

// ===========================================================================
// 四、十年承诺（判据「承诺」：条款闭集 + 审计）
// ===========================================================================

/// 一条承诺条款。
#[derive(Clone, Copy, Debug)]
pub struct PledgeRow {
    /// 条款键（稳定名）。
    pub clause: &'static str,
    /// 条款文本（承诺内容——空条款=没承诺）。
    pub text: &'static str,
}

/// 十年承诺条款表（v1；五条款闭集）。
pub const PLEDGE_TABLE: [PledgeRow; 5] = [
    PledgeRow {
        clause: "signature-freeze",
        text: "五签名的入参/出参/失败语义十年内不做破坏性变更",
    },
    PledgeRow {
        clause: "breaking-needs-adr",
        text: "任何破坏性变更必须走 ADR 与超版（v2+），不得就地改 v1",
    },
    PledgeRow {
        clause: "deprecation-superversion",
        text: "签名废弃只在超版发布后生效，v1 面内不出现静默废弃",
    },
    PledgeRow {
        clause: "fallback-labelled",
        text: "降级路径必须带标注（读屏可查），不允许静默降级",
    },
    PledgeRow {
        clause: "delegation-no-silent-swap",
        text: "委托对端实现可升级，失败语义不得静默更换——改语义即破坏冻结",
    },
];

/// 承诺审计：条款数守恒（v1 口径五条）+ 全条款非空。
/// 输出（版本指纹，面板/日志共用）。
pub fn pledge_audit() -> Result<String, &'static str> {
    if PLEDGE_TABLE.len() != 5 {
        return Err("pledge-rows-changed");
    }
    for row in PLEDGE_TABLE.iter() {
        if row.clause.is_empty() || row.text.is_empty() {
            return Err(row.clause);
        }
    }
    Ok(format!(
        "{} pledge=5 clauses ok",
        GAFREEZE_VERSION
    ))
}

// ===========================================================================
// 五、冻结签名实现（薄层委托——冻结面零算法）
// ===========================================================================

/// 网格对象句柄（冻结面的统一网格表示——加载签名的出参）。
///
/// 与 `RepairMesh` 同构但不持有其内部缓冲的所有权语义：冻结面的查询出参
/// 全部走 `Option`，调用方无须了解产线内部布局。
#[derive(Clone, Debug, PartialEq)]
pub struct FrozenMesh {
    /// 顶点（LE f32 三元组）。
    pub verts: Vec<[f32; 3]>,
    /// 三角面（顶点索引三元组）。
    pub faces: Vec<[u32; 3]>,
}

impl FrozenMesh {
    /// 顶点数。
    pub fn vert_count(&self) -> usize {
        self.verts.len()
    }

    /// 面数。
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// 单顶点（越界 `None`——查询签名的失败语义）。
    pub fn vert(&self, i: usize) -> Option<[f32; 3]> {
        self.verts.get(i).copied()
    }

    /// 单面（越界 `None`）。
    pub fn face(&self, f: usize) -> Option<[u32; 3]> {
        self.faces.get(f).copied()
    }
}

/// 加载结果（三态封闭：成功 / 拒绝——截断与超容统归拒绝，细节在消息）。
pub enum LoadOutcome {
    /// 成功。
    Ok(FrozenMesh),
    /// 拒绝（含原因）。
    Reject { code: &'static str, detail: String },
}

/// 签名一 grid.load：字节流 → 网格对象（委托 [`parse_mesh_stream`]）。
pub fn grid_load(bytes: &[u8]) -> LoadOutcome {
    match parse_mesh_stream(bytes) {
        MeshStreamOutcome::Ok(m) => {
            let mut verts = Vec::with_capacity(m.vert_count());
            let mut faces = Vec::with_capacity(m.face_count());
            let mut i = 0u32;
            while i < m.vert_count() as u32 {
                match m.vert(i) {
                    Some(v) => verts.push(v),
                    None => break,
                }
                i += 1;
            }
            let mut f = 0usize;
            while f < m.face_count() {
                match m.face(f) {
                    Some(t) => faces.push(t),
                    None => break,
                }
                f += 1;
            }
            LoadOutcome::Ok(FrozenMesh { verts, faces })
        }
        MeshStreamOutcome::Truncated { need, got } => LoadOutcome::Reject {
            code: E_GAF_LOAD_REJECT,
            detail: format!("truncated need={} got={}", need, got),
        },
        MeshStreamOutcome::Rejected { reason } => LoadOutcome::Reject {
            code: E_GAF_LOAD_REJECT,
            detail: format!("rejected: {}", reason),
        },
    }
}

/// 查询类别（查询签名的入参闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryKind {
    /// 顶点位置。
    Position(usize),
    /// 面索引。
    Corner(usize),
    /// 顶点关联面（邻接）。
    AdjacentFaces(usize),
    /// 面邻接面（共享边）。
    AdjacentFacesOfFace(usize),
}

/// 查询结果（越界即 `Err(E_GAF_QUERY_RANGE)`——失败语义是签名的一部分）。
pub enum QueryOutcome {
    /// 顶点位置。
    Position([f32; 3]),
    /// 面索引。
    Corner([u32; 3]),
    /// 关联面表。
    FaceList(Vec<u32>),
}

/// 顶点→关联面邻接表（O(V+F) 构建；冻结为查询签名的邻接出参口径）。
pub fn build_adjacency(m: &FrozenMesh) -> Vec<Vec<u32>> {
    let mut adj: Vec<Vec<u32>> = Vec::with_capacity(m.vert_count());
    let mut i = 0usize;
    while i < m.vert_count() {
        adj.push(Vec::new());
        i += 1;
    }
    let mut f = 0usize;
    while f < m.face_count() {
        if let Some(t) = m.face(f) {
            let mut k = 0usize;
            while k < 3 {
                if (t[k] as usize) < adj.len() {
                    adj[t[k] as usize].push(f as u32);
                }
                k += 1;
            }
        }
        f += 1;
    }
    adj
}

/// 面邻接面（共享一条边的面；结果升序去重——冻结口径）。
pub fn face_adjacency(m: &FrozenMesh, f: usize) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let t = match m.face(f) {
        Some(t) => t,
        None => return out,
    };
    // 边 → 出现次数：共享边即邻接。V/F 上限受加载护栏约束，线性扫描可承受。
    let mut g = 0usize;
    while g < m.face_count() {
        if g == f {
            g += 1;
            continue;
        }
        if let Some(u) = m.face(g) {
            let mut shared = 0usize;
            let mut a = 0usize;
            while a < 3 {
                let mut b = 0usize;
                while b < 3 {
                    if t[a] == u[b] {
                        shared += 1;
                    }
                    b += 1;
                }
                a += 1;
            }
            if shared >= 2 {
                out.push(g as u32);
            }
        }
        g += 1;
    }
    out
}

/// 签名二 grid.query：只读查询（越界返回失败码，不 panic）。
pub fn grid_query(m: &FrozenMesh, q: QueryKind) -> Result<QueryOutcome, &'static str> {
    match q {
        QueryKind::Position(i) => match m.vert(i) {
            Some(p) => Ok(QueryOutcome::Position(p)),
            None => Err(E_GAF_QUERY_RANGE),
        },
        QueryKind::Corner(f) => match m.face(f) {
            Some(t) => Ok(QueryOutcome::Corner(t)),
            None => Err(E_GAF_QUERY_RANGE),
        },
        QueryKind::AdjacentFaces(v) => {
            if v >= m.vert_count() {
                return Err(E_GAF_QUERY_RANGE);
            }
            let adj = build_adjacency(m);
            match adj.get(v) {
                Some(list) => Ok(QueryOutcome::FaceList(list.clone())),
                None => Err(E_GAF_QUERY_RANGE),
            }
        }
        QueryKind::AdjacentFacesOfFace(f) => {
            if f >= m.face_count() {
                return Err(E_GAF_QUERY_RANGE);
            }
            Ok(QueryOutcome::FaceList(face_adjacency(m, f)))
        }
    }
}

/// 签名三 grid.stat：统计聚合 + F1613 体检（委托 [`vef16_gridstat::inspect_mesh`]）。
///
/// 聚合量从网格**实测**（不是调用方自报）：vertices/faces 直取，lod/compression/
/// attribute 按冻结 v1 的缺省口径（单网格未量化未分级）。
pub fn grid_stat(
    mesh_id: u64,
    m: &FrozenMesh,
    t: &ThresholdTable,
) -> Result<crate::svstar2::vef16_gridstat::MeshReport, &'static str> {
    let stat = MeshStat {
        vertices: m.vert_count() as u64,
        faces: m.face_count() as u64,
        lod_levels: 1,
        compression_permille: 1000,
        attribute_bytes: (m.vert_count() * 12 + m.face_count() * 12) as u64,
    };
    if vef16_gridstat::validate_stat(&stat).is_err() {
        return Err("stat-invalid");
    }
    match vef16_gridstat::inspect_mesh(mesh_id, &stat, t) {
        Ok(rep) => Ok(rep),
        Err(_) => Err("stat-inspect-failed"),
    }
}

/// 签名四 grid.repair_request：缺陷检测（委托 [`meshrepair::detect_all`]）。
/// 报告幂等由产线 `report_is_deterministic` 与判据侧双重承载。
pub fn grid_repair_request(m: &RepairMesh) -> DefectReport {
    let mut bag = crate::gfx::meshrepair::MeshDiagBag::new();
    meshrepair::detect_all(m, &mut bag)
}

/// 简化请求结果（取消是显性失败，不是静默丢弃）。
pub enum SimplifyOutcome {
    /// 成功产出。
    Ok {
        /// 简化后网格。
        mesh: DecMesh,
        /// 目标面数（请求口径，判据与日志用）。
        target_faces: usize,
    },
    /// 被取消（帧边界叫停——签名承诺的取消语义）。
    Cancelled,
    /// 目标面数不可达（低于产线下限——如实报错，不静默夹到下限再假装成功）。
    Infeasible { detail: String },
}

/// 签名五 grid.simplify_request：可取消的简化请求（委托
/// [`meshdecimate::simplify`]）。
///
/// `cancel` 为真时**不进入委托**——取消语义发生在请求层，产线不被打扰；
/// 这与「简化请求可取消」的锚点判据直接对应：取消是签名行为，不是调用方
/// 自行 try 的事。
pub fn grid_simplify_request(
    m: &RepairMesh,
    target_ratio_permille: u32,
    cancel: bool,
) -> SimplifyOutcome {
    if cancel {
        return SimplifyOutcome::Cancelled;
    }
    let src = repair_to_dec(m);
    let target = (src.face_count() as u64 * target_ratio_permille as u64 / 1000).max(1) as usize;
    if target < meshdecimate::MIN_FACES {
        return SimplifyOutcome::Infeasible {
            detail: format!("target {} below MIN_FACES {}", target, meshdecimate::MIN_FACES),
        };
    }
    let opts = SimplifyOptions::with_target(target);
    let (out, _rep) = meshdecimate::simplify(&src, &opts);
    SimplifyOutcome::Ok { mesh: out, target_faces: target }
}

/// RepairMesh → DecMesh 转换（冻结面的委托胶水；边界标记实算——开放边=
/// 只被一个面使用的无向边，错误的全 false 会误导产线的收缩跳过纪律）。
pub fn repair_to_dec(m: &RepairMesh) -> DecMesh {
    let mut d = DecMesh::new();
    let mut i = 0u32;
    while i < m.vert_count() as u32 {
        match m.vert(i) {
            Some(v) => {
                d.verts.push(v);
                d.boundary.push(false);
            }
            None => break,
        }
        i += 1;
    }
    let mut f = 0usize;
    while f < m.face_count() {
        match m.face(f) {
            Some(t) => d.faces.push(t),
            None => break,
        }
        f += 1;
    }
    // 开放边界标记：每条无向边数出现次数（BTreeMap，O(F log F)），
    // 恰一次者为边界边——错误的全 false 会误导产线的收缩跳过纪律。
    let mut edge_count: BTreeMap<(u32, u32), u32> = BTreeMap::new();
    let mut e = 0usize;
    while e < d.faces.len() {
        let t = d.faces[e];
        let mut k = 0usize;
        while k < 3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let key = if a < b { (a, b) } else { (b, a) };
            *edge_count.entry(key).or_insert(0) += 1;
            k += 1;
        }
        e += 1;
    }
    for (&(a, b), &occ) in edge_count.iter() {
        if occ == 1 {
            if (a as usize) < d.boundary.len() {
                d.boundary[a as usize] = true;
            }
            if (b as usize) < d.boundary.len() {
                d.boundary[b as usize] = true;
            }
        }
    }
    d
}

/// 摘要行（面板/日志/读屏共用同一事实源）。
pub fn screen_line() -> String {
    format!(
        "{} apis={} unify_rows={} pledges={}",
        GAFREEZE_VERSION,
        GRID_API_SPECS.len(),
        UNIFY_TABLE.len(),
        PLEDGE_TABLE.len(),
    )
}
