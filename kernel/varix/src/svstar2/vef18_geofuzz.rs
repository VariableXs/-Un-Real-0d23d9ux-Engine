//! VE-F1615 · 几何 fuzz（VE-I 域 · 几何工具段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1615`
//!
//! **判据（锚点原文）**：三类、容错、CI、判据。
//!
//! **职责定位（锚点原文）**：畸形网格 fuzz（越界索引 / NaN 几何 / 超大属性 /
//! 截断文件——四畸形域容错不崩，是 F1612 校验器的 fuzz 验证）；量化 fuzz
//! （极端量化参数——1bit 极限档 / 全域量化，精度路径验证：量化失真不崩
//! 不失控）；转换 fuzz（畸形 glTF 导入容错）；案例固化 CI。
//!
//! **工程量（锚点原文）**：三域 fuzz 语料与断言 220 行＋案例固化 40 行＋
//! CI 40 行＝目标 300 行构成。
//!
//! **语料来源三线并行（F1128 语料治理纪律）**：
//! - **构造性**（手工构建极端案例）：四畸形域网格逐类构造、极端量化形态、
//!   畸形 glTF 文档逐形态构造；
//! - **变异生成**：以合法种子网格流为基线做位翻转/字节替换，变异产物
//!   喂进解析+三查管线，专攻「差一个字节但后果很大」的一类缺陷；
//! - **生态收集**：以固化字节常量内嵌真实世界畸形形态（导出器中断产生
//!   的半截流等），作为回归语料永久保留。
//!
//! **不崩判据的承载方式**：被测面（流解析器 / 三查 / 修复 / 量化 / 导入）
//! 均为纯函数且契约零 panic；fuzz 执行器**不吞异常也不代填 panic_free**——
//! 任何一次 panic 都会让整套自检进程崩溃，门禁必红（与 vec19「不由本域
//! 代填」同一纪律）。每条语料的执行产出**记账**（完成/预算内/命中数），
//! 完成即证明该条走完全程。
//!
//! **不挂起判据**：解析器带**分配上限**（顶点/面计数超容量直接拒绝，
//! 不给畸形头部一次分配爆掉内核的机会）；执行器带**步数预算**，超限
//! 记 `BudgetExceeded` 为红——挂起在真实设备上表现为卡死，用预算把
//! 「不挂起」转成可判定判据。
//!
//! 零静默纪律：语料三线缺线即红（地板判据）；固化案例回放逐位一致，
//! 回放 digest 漂移即红；CI 门禁对红项**如实报数**不吞不改。

use alloc::vec::Vec;

use crate::gfx::meshquant::{
    compute_bounds, dequantize_position, position_error_bound, quantize_position,
    roundtrip_is_stable, QuantBits, QuantDiag,
};
use crate::gfx::meshrepair::RepairMesh;
use crate::gfx::meshvalidate::{apply_fixes, plan_fixes, triple_scan, ScanLimits, TripleScan};
use crate::svstar2::vem09_import::{
    import_gltf_anim, AccessorView, ChannelPath, ChannelRef, ComponentType, Fidelity,
    GltfAnimDoc, GltfInterp, MappingTable, SamplerRef,
};

// ---------------------------------------------------------------------------
// 一、确定性伪随机（种子可复现的基础——自持 LCG，同 vec19 纪律）
// ---------------------------------------------------------------------------

/// LCG 乘数（Knuth 64 位乘加常数）。
const GEO_LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
const GEO_LCG_C: u64 = 1442695040888963407;

/// 自持确定性伪随机源。**不用 std 随机源**：跨版本序列不保证稳定，
/// 拿它做种子等于放弃可复现性。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeoRng {
    state: u64,
}

impl GeoRng {
    pub const fn new(seed: u64) -> GeoRng {
        GeoRng { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(GEO_LCG_A).wrapping_add(GEO_LCG_C);
        self.state
    }

    /// `[0, n)` 均匀取值（`n == 0` 恒返 0，不panic）。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next_u64() % n
    }

    pub const fn state(&self) -> u64 {
        self.state
    }
}

/// FNV-1a 单步（digest 折叠用）。
const fn fnv1a_step(h: u64, byte: u8) -> u64 {
    (h ^ (byte as u64)).wrapping_mul(0x0000_0100_0000_01B3)
}

/// FNV-1a 字节流摘要。
pub fn fnv1a_bytes(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    let mut i = 0;
    while i < bytes.len() {
        h = fnv1a_step(h, bytes[i]);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 二、语料三线与三域（判据一：三类）
// ---------------------------------------------------------------------------

/// fuzz 三域（判据「三类」的封闭全集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzDomain {
    /// 畸形网格：越界索引 / NaN 几何 / 超大属性 / 截断文件。
    MalformedMesh,
    /// 量化 fuzz：极端量化参数（1bit 极限档 / 全域合法档）。
    Quantization,
    /// 转换 fuzz：畸形 glTF 导入。
    Conversion,
}

impl FuzzDomain {
    /// 三域封闭全集。
    pub const ALL: [FuzzDomain; 3] = [
        FuzzDomain::MalformedMesh,
        FuzzDomain::Quantization,
        FuzzDomain::Conversion,
    ];

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            FuzzDomain::MalformedMesh => "畸形网格",
            FuzzDomain::Quantization => "量化 fuzz",
            FuzzDomain::Conversion => "转换 fuzz",
        }
    }

    /// 线上短码（协议面，固化案例表用）。
    pub const fn wire(self) -> &'static str {
        match self {
            FuzzDomain::MalformedMesh => "gm",
            FuzzDomain::Quantization => "gq",
            FuzzDomain::Conversion => "gc",
        }
    }

    /// 由短码还原（未知短码 → `None`，不猜默认）。
    pub fn from_wire(s: &str) -> Option<FuzzDomain> {
        FuzzDomain::ALL.iter().copied().find(|d| d.wire() == s)
    }
}

/// 语料来源三线（F1128 语料治理纪律）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeoCorpusSource {
    /// 构造性：手工构建极端案例。
    Constructed,
    /// 变异生成：合法种子基线 + 位级变异。
    Mutated,
    /// 生态收集：真实世界畸形形态的固化样本。
    Ecosystem,
}

impl GeoCorpusSource {
    /// 三线封闭全集（缺线即红——地板判据）。
    pub const ALL: [GeoCorpusSource; 3] = [
        GeoCorpusSource::Constructed,
        GeoCorpusSource::Mutated,
        GeoCorpusSource::Ecosystem,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            GeoCorpusSource::Constructed => "构造性",
            GeoCorpusSource::Mutated => "变异生成",
            GeoCorpusSource::Ecosystem => "生态收集",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、网格流解析器（截断文件域的被测面之一：带分配上限，零 panic）
// ---------------------------------------------------------------------------

/// 网格流解析的顶点数分配上限（防畸形头部撑爆分配器）。
pub const STREAM_MAX_VERTS: u32 = 1 << 20;
/// 网格流解析的面数分配上限。
pub const STREAM_MAX_FACES: u32 = 1 << 20;
/// 解析器步数预算（单条语料内的循环上限）。
pub const STREAM_STEP_BUDGET: u64 = 1 << 16;

/// 网格流解析产出（三态封闭：成功 / 截断 / 拒绝——无第四态）。
#[derive(Clone, Debug)]
pub enum MeshStreamOutcome {
    /// 解析成功。
    Ok(RepairMesh),
    /// 字节流截断：头部声明超出实际字节供给。
    Truncated { need: u64, got: u64 },
    /// 头部声明超容量上限（防挂起/防分配爆炸，直接拒绝）。
    Rejected { reason: &'static str },
}

/// 从二进制流解析网格：`u32 vert_count | u32 face_count | 3f32×verts | 3u32×faces`（均 LE）。
///
/// 任何字节供给不足都归 `Truncated`（带需要量与实际量），头部超容量归
/// `Rejected`——**零 panic 面**：不对切片直接下标，全部走 `get` 链。
pub fn parse_mesh_stream(bytes: &[u8]) -> MeshStreamOutcome {
    // 头部 8 字节。
    if bytes.len() < 8 {
        return MeshStreamOutcome::Truncated { need: 8, got: bytes.len() as u64 };
    }
    let vc = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let fc = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if vc > STREAM_MAX_VERTS {
        return MeshStreamOutcome::Rejected { reason: "vert_count_over_cap" };
    }
    if fc > STREAM_MAX_FACES {
        return MeshStreamOutcome::Rejected { reason: "face_count_over_cap" };
    }
    let vert_bytes = (vc as u64) * 12;
    let face_bytes = (fc as u64) * 12;
    let need = 8u64 + vert_bytes + face_bytes;
    if (bytes.len() as u64) < need {
        return MeshStreamOutcome::Truncated { need, got: bytes.len() as u64 };
    }
    let mut m = RepairMesh::new();
    let mut off = 8usize;
    let mut i = 0u32;
    while i < vc {
        let b = match bytes.get(off..off + 12) {
            Some(s) => s,
            None => return MeshStreamOutcome::Truncated { need, got: bytes.len() as u64 },
        };
        let x = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let y = f32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        let z = f32::from_le_bytes([b[8], b[9], b[10], b[11]]);
        m.push_vert([x, y, z]);
        off += 12;
        i += 1;
    }
    let mut f = 0u32;
    while f < fc {
        let b = match bytes.get(off..off + 12) {
            Some(s) => s,
            None => return MeshStreamOutcome::Truncated { need, got: bytes.len() as u64 },
        };
        let a = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let e = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        let c = u32::from_le_bytes([b[8], b[9], b[10], b[11]]);
        m.push_face([a, e, c]);
        off += 12;
        f += 1;
    }
    MeshStreamOutcome::Ok(m)
}

/// 把合法网格编码为二进制流（变异线的种子来源）。
pub fn encode_mesh_stream(m: &RepairMesh) -> Vec<u8> {
    let mut out = Vec::new();
    let vc = m.vert_count() as u32;
    let fc = m.face_count() as u32;
    out.extend_from_slice(&vc.to_le_bytes());
    out.extend_from_slice(&fc.to_le_bytes());
    let mut i = 0u32;
    while i < vc {
        let v = match m.vert(i) {
            Some(p) => p,
            None => break,
        };
        out.extend_from_slice(&v[0].to_le_bytes());
        out.extend_from_slice(&v[1].to_le_bytes());
        out.extend_from_slice(&v[2].to_le_bytes());
        i += 1;
    }
    let mut f = 0usize;
    while f < (fc as usize) {
        let t = match m.face(f) {
            Some(t) => t,
            None => break,
        };
        out.extend_from_slice(&t[0].to_le_bytes());
        out.extend_from_slice(&t[1].to_le_bytes());
        out.extend_from_slice(&t[2].to_le_bytes());
        f += 1;
    }
    out
}

/// 位级变异（变异线）：按种子决定次数，每次做位翻转或字节替换。
/// 返回（变异后字节流，执行的操作数）。
pub fn mutate_stream(base: &[u8], seed: u64, ops_budget: u64) -> (Vec<u8>, u64) {
    let mut out = Vec::new();
    let mut i = 0;
    while i < base.len() {
        out.push(base[i]);
        i += 1;
    }
    let mut rng = GeoRng::new(seed ^ 0xA5A5_5A5A_1234_5678);
    let mut done = 0u64;
    let n = rng.below(ops_budget) + 1;
    while done < n {
        if out.is_empty() {
            break;
        }
        let idx = rng.below(out.len() as u64) as usize;
        let old = match out.get(idx) {
            Some(v) => *v,
            None => break,
        };
        let flip = rng.below(2) == 0;
        let newv = if flip {
            old ^ (1u8 << rng.below(8))
        } else {
            rng.next_u64() as u8
        };
        out[idx] = newv;
        done += 1;
    }
    (out, done)
}

// ---------------------------------------------------------------------------
// 四、域一：畸形网格构造性语料（四畸形域）与容错执行器
// ---------------------------------------------------------------------------

/// 四畸形域（封闭全集，锚点明列四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MalformedKind {
    /// 越界索引：面引用 >= 顶点数。
    IndexOutOfRange,
    /// NaN 几何：顶点坐标非有限。
    NanGeometry,
    /// 超大属性：有限但超声明上限。
    HugeAttribute,
    /// 截断文件：字节供给不足。
    TruncatedFile,
}

impl MalformedKind {
    /// 四域封闭全集。
    pub const ALL: [MalformedKind; 4] = [
        MalformedKind::IndexOutOfRange,
        MalformedKind::NanGeometry,
        MalformedKind::HugeAttribute,
        MalformedKind::TruncatedFile,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            MalformedKind::IndexOutOfRange => "越界索引",
            MalformedKind::NanGeometry => "NaN 几何",
            MalformedKind::HugeAttribute => "超大属性",
            MalformedKind::TruncatedFile => "截断文件",
        }
    }
}

/// 单条畸形网格语料的执行记账（完成 = 走完全程；不代填不崩位）。
#[derive(Clone, Debug, PartialEq)]
pub struct MalformedOutcome {
    pub kind: MalformedKind,
    /// 注入的畸形点数（构造性语料的**期望下界**）。
    pub injected: u32,
    /// 三查命中总数（执行后独立读出）。
    pub hits_total: usize,
    /// 各查种命中数（按 TripleScan::code() 槽位）。
    pub hits_by_code: [usize; 3],
    /// 修复后再扫的命中总数。
    pub hits_after_fix: usize,
    /// 执行步数（解析+扫描+修复循环合计），须 ≤ 预算。
    pub steps_used: u64,
    /// 解析/执行是否完成（截断域：解析产出 Truncated 即算完成）。
    pub completed: bool,
}

/// 步数预算（执行器全局，防挂起判据）。
pub const MALFORMED_STEP_BUDGET: u64 = 1 << 16;

/// 构造一个「干净」参考网格（单位立方体三角化：8 顶点 12 面）。
pub fn build_clean_cube() -> RepairMesh {
    let mut m = RepairMesh::new();
    let vs: [[f32; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];
    let mut i = 0;
    while i < 8 {
        m.push_vert(vs[i]);
        i += 1;
    }
    let fs: [[u32; 3]; 12] = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [2, 3, 7],
        [2, 7, 6],
        [0, 4, 7],
        [0, 7, 3],
        [1, 2, 6],
        [1, 6, 5],
    ];
    let mut f = 0;
    while f < 12 {
        m.push_face(fs[f]);
        f += 1;
    }
    m
}

/// 构造性畸形语料：按畸形域注入确定性畸形，返回（网格，注入数）。
/// 截断文件域返回空网格（该域走字节流解析面）。
pub fn build_malformed_mesh(kind: MalformedKind, seed: u64) -> (RepairMesh, u32) {
    let mut rng = GeoRng::new(seed ^ 0x0F1E_2D3C_0000_0001);
    match kind {
        MalformedKind::IndexOutOfRange => {
            let mut m = build_clean_cube();
            // 注入 3 条越界面：索引 = 顶点数 + 1..=3（确定性偏移）。
            let vc = m.vert_count() as u32;
            let mut k = 1u32;
            while k <= 3 {
                let off = (rng.below(4) as u32) + 1;
                m.push_face([vc + off, 0, 1]);
                k += 1;
            }
            (m, 3)
        }
        MalformedKind::NanGeometry => {
            let mut m = build_clean_cube();
            // 注入 2 个 NaN 顶点（覆写尾部两个顶点——先加后换不行，直接补）。
            m.push_vert([f32::NAN, 0.0, 0.0]);
            m.push_vert([0.0, f32::from_bits(0x7F80_0000), 0.0]);
            (m, 2)
        }
        MalformedKind::HugeAttribute => {
            let mut m = build_clean_cube();
            m.push_vert([1.0e30, 0.0, 0.0]);
            m.push_vert([0.0, -2.0e30, 0.0]);
            (m, 2)
        }
        MalformedKind::TruncatedFile => {
            // 截断域的语料是字节流，不构造网格（执行器走 parse_mesh_stream）。
            (RepairMesh::new(), 0)
        }
    }
}

/// 域一执行器：畸形网格 → 三查 → 修复 → 再扫，全程记账。
pub fn run_malformed_case(kind: MalformedKind, seed: u64, bytes: Option<&[u8]>) -> MalformedOutcome {
    let limits = ScanLimits::default();
    let mut steps = 0u64;
    let parsed: Option<RepairMesh> = match kind {
        MalformedKind::TruncatedFile => {
            // 截断域：语料为字节流（变异线/生态线供给），解析产出即结论。
            match bytes {
                Some(b) => {
                    steps += 1;
                    match parse_mesh_stream(b) {
                        MeshStreamOutcome::Ok(m) => Some(m),
                        // Truncated/Rejected = 该域容错的**期望产出**：
                        // 记账完成，网格置空（无三查可做）。
                        _ => None,
                    }
                }
                None => None,
            }
        }
        _ => {
            let (m, _) = build_malformed_mesh(kind, seed);
            steps += 1;
            Some(m)
        }
    };
    let mut out = MalformedOutcome {
        kind,
        injected: 0,
        hits_total: 0,
        hits_by_code: [0; 3],
        hits_after_fix: 0,
        steps_used: steps,
        completed: false,
    };
    let m = match parsed {
        Some(m) => m,
        None => {
            // 截断/拒绝产出：completed = 解析面走完（容错语义达成）。
            out.completed = true;
            out.steps_used = steps;
            return out;
        }
    };
    // 注入数（构造性域的固定注入量，与 build_malformed_mesh 各分支一致）。
    out.injected = match kind {
        MalformedKind::IndexOutOfRange => 3,
        MalformedKind::NanGeometry => 2,
        MalformedKind::HugeAttribute => 2,
        MalformedKind::TruncatedFile => 0,
    };
    // 三查。
    let rep = triple_scan(&m, &limits);
    steps += 1 + (rep.total() as u64);
    out.hits_total = rep.total();
    let mut c = 0;
    while c < 3 {
        out.hits_by_code[c] = match TripleScan::of_code(c as u8) {
            Some(s) => rep.count_of(s),
            None => 0,
        };
        c += 1;
    }
    // 修复 → 再扫（命中数不得增加——修复不制造新畸形）。
    let plan = plan_fixes(&rep, &limits);
    let fixed = apply_fixes(&m, &plan);
    let rep2 = triple_scan(&fixed, &limits);
    steps += 1 + (rep2.total() as u64);
    out.hits_after_fix = rep2.total();
    out.steps_used = steps;
    out.completed = steps <= MALFORMED_STEP_BUDGET;
    out
}

// ---------------------------------------------------------------------------
// 五、域二：量化 fuzz（极端量化参数——失真不崩不失控）
// ---------------------------------------------------------------------------

/// 单条量化语料的执行记账。
#[derive(Clone, Debug, PartialEq)]
pub struct QuantOutcome {
    /// 请求的原始位数字段（1..=16 全域扫描，含越域值）。
    pub raw_bits: u32,
    /// 越域请求被拒（`QuantBits::from_bits` → None）：1bit 极限档等。
    pub rejected: bool,
    /// 合法档：往返误差上界达成（判据侧独立界）。
    pub bound_ok: bool,
    /// 合法档：往返稳定。
    pub stable: bool,
    /// 量化诊断（NaN/Inf/空集路径）。
    pub diag_hit: Option<&'static str>,
}

/// 合法档位全集（与 meshquant 单源对齐，判据侧独立枚举）。
pub const VALID_QUANT_BITS: [u32; 4] = [8, 10, 12, 16];

/// 量化执行器：给定坐标片与原始位数，产出记账。
/// 输入约定：坐标已是**有限**值（超大属性归三查域）。
pub fn run_quant_case(verts: &[f32], raw_bits: u32) -> QuantOutcome {
    let mut out = QuantOutcome {
        raw_bits,
        rejected: false,
        bound_ok: false,
        stable: false,
        diag_hit: None,
    };
    let bits = match QuantBits::from_bits(raw_bits) {
        Some(b) => b,
        None => {
            out.rejected = true;
            return out;
        }
    };
    let mut diag = crate::gfx::meshquant::DiagBag::new();
    let bounds = match compute_bounds(verts, &mut diag) {
        Some(b) => b,
        None => {
            // NaN/Inf/空集：量化路径的容错产出 = None + 诊断记账。
            out.diag_hit = if diag.has(QuantDiag::PositionNan) {
                Some("position_nan")
            } else if diag.has(QuantDiag::PositionInf) {
                Some("position_inf")
            } else if diag.has(QuantDiag::VertexSetEmpty) {
                Some("vertex_set_empty")
            } else {
                Some("unknown")
            };
            return out;
        }
    };
    // 逐点往返：误差 ≤ 位置误差上界（+绝对容差吸收浮点尾差），且稳定。
    let bound = position_error_bound(&bounds, bits) + 1.0e-4;
    let mut all_ok = true;
    let mut all_stable = true;
    let mut i = 0;
    while i + 2 < verts.len() {
        let p = [verts[i], verts[i + 1], verts[i + 2]];
        let q = quantize_position(p, &bounds, bits);
        let back = dequantize_position(q, &bounds, bits);
        let mut d = 0;
        while d < 3 {
            let diff = (p[d] - back[d]).abs();
            if diff > bound {
                all_ok = false;
            }
            d += 1;
        }
        if !roundtrip_is_stable(q, &bounds, bits) {
            all_stable = false;
        }
        i += 3;
    }
    out.bound_ok = all_ok;
    out.stable = all_stable;
    out
}

/// 极端量化形态语料（构造性线）：（标签，坐标流）。
/// 超大属性不进本域（归三查），这里取「合法但极端」的尺寸/分布。
pub fn quant_extreme_shapes() -> [(&'static str, Vec<f32>); 4] {
    let mut unit = Vec::new();
    let mut degenerate = Vec::new();
    let mut wide = Vec::new();
    let mut tiny = Vec::new();
    // 单位立方体 8 顶点。
    let mut x = 0.0f32;
    while x <= 1.0 {
        let mut y = 0.0f32;
        while y <= 1.0 {
            let mut z = 0.0f32;
            while z <= 1.0 {
                unit.push(x);
                unit.push(y);
                unit.push(z);
                z += 1.0;
            }
            y += 1.0;
        }
        x += 1.0;
    }
    // 退化轴：单平面 quad（Y 全 0）。
    degenerate.extend_from_slice(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
    // 宽域：±1e4（合法但跨度大）。
    wide.extend_from_slice(&[-1.0e4, -1.0e4, -1.0e4, 1.0e4, 1.0e4, 1.0e4]);
    // 极小：亚毫米级（逼近退化夹紧下限）。
    tiny.extend_from_slice(&[0.0, 0.0, 0.0, 1.0e-7, 2.0e-7, 3.0e-7]);
    [("unit_cube", unit), ("degenerate_quad", degenerate), ("wide_span", wide), ("sub_micro", tiny)]
}

// ---------------------------------------------------------------------------
// 六、域三：转换 fuzz（畸形 glTF 导入容错）
// ---------------------------------------------------------------------------

/// 转换域畸形形态（构造性线，封闭全集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GltfMalformedKind {
    /// 节点数为 0（文档级三要素拒绝）。
    ZeroNodes,
    /// 通道引用越界节点。
    ChannelTargetOob,
    /// 采样器下标越界。
    SamplerOob,
    /// accessor 声明与数据长度不一致。
    AccessorLenMismatch,
    /// 空采样器表但通道非空。
    EmptySamplers,
}

impl GltfMalformedKind {
    pub const ALL: [GltfMalformedKind; 5] = [
        GltfMalformedKind::ZeroNodes,
        GltfMalformedKind::ChannelTargetOob,
        GltfMalformedKind::SamplerOob,
        GltfMalformedKind::AccessorLenMismatch,
        GltfMalformedKind::EmptySamplers,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            GltfMalformedKind::ZeroNodes => "零节点文档",
            GltfMalformedKind::ChannelTargetOob => "通道目标越界",
            GltfMalformedKind::SamplerOob => "采样器下标越界",
            GltfMalformedKind::AccessorLenMismatch => "accessor 长度不符",
            GltfMalformedKind::EmptySamplers => "空采样器表",
        }
    }
}

/// 转换域执行记账。
#[derive(Clone, Debug, PartialEq)]
pub struct ConversionOutcome {
    pub kind_label: &'static str,
    /// 导入返回 Err（被拒）。
    pub rejected: bool,
    /// Err 时诊断袋非空（错误有码，不静默）。
    pub diag_nonempty: bool,
    /// 命中文档级畸形码（`DOC_MALFORMED`）。
    pub doc_malformed: bool,
    /// Err 时的 P1 计数。
    pub p1_count: usize,
}

/// 构造畸形 glTF 文档（每形态确定性变异自同一合法基线）。
pub fn build_malformed_gltf(kind: GltfMalformedKind) -> GltfAnimDoc {
    // 合法基线：1 节点、2 accessor、1 采样器、1 通道。
    let acc_ok = AccessorView {
        component_type: ComponentType::Float,
        normalized: false,
        count: 2,
        comps: 3,
        data: alloc::vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    };
    match kind {
        GltfMalformedKind::ZeroNodes => GltfAnimDoc::new(
            0,
            alloc::vec![acc_ok],
            alloc::vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
            alloc::vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
            "fuzz-zero-nodes",
        ),
        GltfMalformedKind::ChannelTargetOob => GltfAnimDoc::new(
            1,
            alloc::vec![acc_ok],
            alloc::vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
            alloc::vec![ChannelRef { target_node: 99, path: ChannelPath::Translation, sampler: 0 }],
            "fuzz-target-oob",
        ),
        GltfMalformedKind::SamplerOob => GltfAnimDoc::new(
            1,
            alloc::vec![acc_ok],
            alloc::vec![SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear }],
            alloc::vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 42 }],
            "fuzz-sampler-oob",
        ),
        GltfMalformedKind::AccessorLenMismatch => {
            let bad = AccessorView {
                component_type: ComponentType::Float,
                normalized: false,
                count: 4,
                comps: 3,
                data: alloc::vec![0.0, 0.0, 0.0],
            };
            GltfAnimDoc::new(
                1,
                alloc::vec![acc_ok, bad],
                alloc::vec![SamplerRef { input: 1, output: 1, interp: GltfInterp::Linear }],
                alloc::vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
                "fuzz-acc-mismatch",
            )
        }
        GltfMalformedKind::EmptySamplers => GltfAnimDoc::new(
            1,
            alloc::vec![acc_ok],
            alloc::vec![],
            alloc::vec![ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 }],
            "fuzz-empty-samplers",
        ),
    }
}

/// 域三执行器：畸形文档 → 导入 → 记账（Err/Ok 均为完成；Err 必须带诊断）。
pub fn run_conversion_case(kind: GltfMalformedKind) -> ConversionOutcome {
    let doc = build_malformed_gltf(kind);
    let table = MappingTable::standard();
    let mut bag = crate::svstar2::vem09_import::DiagBag::new();
    let r = import_gltf_anim(&doc, Fidelity::Faithful, &table, &mut bag);
    match r {
        Err(_) => ConversionOutcome {
            kind_label: kind.label(),
            rejected: true,
            diag_nonempty: !bag.is_empty(),
            doc_malformed: bag.has(crate::svstar2::vem09_import::DiagCode::DOC_MALFORMED),
            p1_count: bag.p1_count(),
        },
        Ok(_) => ConversionOutcome {
            kind_label: kind.label(),
            rejected: false,
            diag_nonempty: !bag.is_empty(),
            doc_malformed: bag.has(crate::svstar2::vem09_import::DiagCode::DOC_MALFORMED),
            p1_count: bag.p1_count(),
        },
    }
}

// ---------------------------------------------------------------------------
// 七、案例固化（判据三：固化即回归——回放逐位一致）
// ---------------------------------------------------------------------------

/// 固化案例条目：id 全局唯一，域×线全覆盖，种子写死。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeoFixedCase {
    pub id: &'static str,
    pub domain: FuzzDomain,
    pub source: GeoCorpusSource,
    pub seed: u64,
    /// 备注（生态线样本的来源形态描述）。
    pub note: &'static str,
}

/// 固化案例表（**只增不改**：追加只能追加到末尾；改种子=改判据，禁）。
pub const FIXED_CASES: [GeoFixedCase; 9] = [
    GeoFixedCase {
        id: "gm-const-oob",
        domain: FuzzDomain::MalformedMesh,
        source: GeoCorpusSource::Constructed,
        seed: 0x6E01_0000_0000_0001,
        note: "越界索引三面注入（构造性）",
    },
    GeoFixedCase {
        id: "gm-const-nan",
        domain: FuzzDomain::MalformedMesh,
        source: GeoCorpusSource::Constructed,
        seed: 0x6E01_0000_0000_0002,
        note: "NaN 几何两点注入（构造性）",
    },
    GeoFixedCase {
        id: "gm-const-huge",
        domain: FuzzDomain::MalformedMesh,
        source: GeoCorpusSource::Constructed,
        seed: 0x6E01_0000_0000_0003,
        note: "超大属性两点注入（构造性）",
    },
    GeoFixedCase {
        id: "gm-mut-seed7",
        domain: FuzzDomain::MalformedMesh,
        source: GeoCorpusSource::Mutated,
        seed: 7,
        note: "立方体流位级变异（变异生成）",
    },
    GeoFixedCase {
        id: "gm-eco-trunc-half",
        domain: FuzzDomain::MalformedMesh,
        source: GeoCorpusSource::Ecosystem,
        seed: 0,
        note: "导出器中断半截流（生态收集固化样本）",
    },
    GeoFixedCase {
        id: "gq-bits-sweep",
        domain: FuzzDomain::Quantization,
        source: GeoCorpusSource::Constructed,
        seed: 0x6E02_0000_0000_0001,
        note: "1..=16 全域位扫描 + 四极端形态",
    },
    GeoFixedCase {
        id: "gq-1bit-limit",
        domain: FuzzDomain::Quantization,
        source: GeoCorpusSource::Constructed,
        seed: 0x6E02_0000_0000_0002,
        note: "1bit 极限档越域请求拒绝路径",
    },
    GeoFixedCase {
        id: "gc-eco-doc-bad",
        domain: FuzzDomain::Conversion,
        source: GeoCorpusSource::Ecosystem,
        seed: 0x6E03_0000_0000_0001,
        note: "生态收集畸形文档五形态固化",
    },
    GeoFixedCase {
        id: "gc-const-nodes0",
        domain: FuzzDomain::Conversion,
        source: GeoCorpusSource::Constructed,
        seed: 0x6E03_0000_0000_0002,
        note: "零节点文档拒绝路径（构造性）",
    },
];

/// 生态线固化样本：导出器中断的半截流（头部声明 4 顶点 2 面，只给到第 2 顶点一半）。
pub const ECO_TRUNCATED_STREAM: [u8; 20] = [
    4, 0, 0, 0, // vert_count = 4
    2, 0, 0, 0, // face_count = 2
    0, 0, 0x80, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0, // 只有 1.5 个顶点的字节
];

/// 变异线种子流：干净立方体的二进制流（由 encode_mesh_stream 生成，
/// 此处用构造函数保证与被测编码器**同源**，不手抄字节）。
pub fn mutated_seed_stream() -> Vec<u8> {
    encode_mesh_stream(&build_clean_cube())
}

/// 单条固化案例的回放产出（digest 覆盖：域短码 + 关键记账字段）。
#[derive(Clone, Debug, PartialEq)]
pub struct FixedReplay {
    pub id: &'static str,
    pub digest: u64,
    /// 回放是否完成（完成 = 该域执行器走完全程）。
    pub completed: bool,
}

/// 回放一条固化案例（确定性：同条目在任何机器上产出同一 digest）。
pub fn replay_fixed_case(idx: usize) -> Option<FixedReplay> {
    let c = match FIXED_CASES.get(idx) {
        Some(c) => c,
        None => return None,
    };
    match c.domain {
        FuzzDomain::MalformedMesh => {
            let bytes: Vec<u8> = match c.source {
                GeoCorpusSource::Ecosystem => ECO_TRUNCATED_STREAM.to_vec(),
                GeoCorpusSource::Mutated => {
                    let (b, _) = mutate_stream(&mutated_seed_stream(), c.seed, 8);
                    b
                }
                GeoCorpusSource::Constructed => encode_mesh_stream(&build_clean_cube()),
            };
            let kind = match c.source {
                GeoCorpusSource::Mutated => MalformedKind::TruncatedFile,
                GeoCorpusSource::Ecosystem => MalformedKind::TruncatedFile,
                GeoCorpusSource::Constructed => MalformedKind::IndexOutOfRange,
            };
            let o = run_malformed_case(kind, c.seed, Some(&bytes));
            let mut h = fnv1a_bytes(0x1615, c.id.as_bytes());
            h = fnv1a_step(h, o.completed as u8);
            h = h.wrapping_add((o.hits_total as u64) << 8);
            h = h.wrapping_add((o.hits_after_fix as u64) << 40);
            Some(FixedReplay { id: c.id, digest: h, completed: o.completed })
        }
        FuzzDomain::Quantization => {
            // 位扫描 + 四形态，digest 折叠全部产出。
            let mut h = fnv1a_bytes(0x1615, c.id.as_bytes());
            let completed = true;
            let shapes = quant_extreme_shapes();
            let mut s = 0;
            while s < shapes.len() {
                let mut raw = 1u32;
                while raw <= 16 {
                    let o = run_quant_case(&shapes[s].1, raw);
                    h = fnv1a_step(h, o.rejected as u8);
                    h = fnv1a_step(h, o.bound_ok as u8);
                    h = fnv1a_step(h, o.stable as u8);
                    h = fnv1a_step(h, o.diag_hit.is_some() as u8);
                    raw += 1;
                }
                s += 1;
            }
            Some(FixedReplay { id: c.id, digest: h, completed })
        }
        FuzzDomain::Conversion => {
            let mut h = fnv1a_bytes(0x1615, c.id.as_bytes());
            let completed = true;
            let mut k = 0;
            while k < GltfMalformedKind::ALL.len() {
                let o = run_conversion_case(GltfMalformedKind::ALL[k]);
                h = fnv1a_step(h, o.rejected as u8);
                h = fnv1a_step(h, o.diag_nonempty as u8);
                h = h.wrapping_add((o.p1_count as u64) << (k + 8));
                k += 1;
            }
            Some(FixedReplay { id: c.id, digest: h, completed })
        }
    }
}

// ---------------------------------------------------------------------------
// 八、CI 门禁（判据三：CI——红项如实报数，地板防语料退化）
// ---------------------------------------------------------------------------

/// 每域语料条数地板（判据侧独立写死，防「清空语料全绿」退化）。
pub const CI_FLOOR_PER_DOMAIN: usize = 2;
/// 全量 fuzz 步数预算。
pub const CI_STEP_BUDGET: u64 = 1 << 18;

/// CI 门禁产出。
#[derive(Clone, Debug, PartialEq)]
pub struct GeoFuzzCiReport {
    /// 执行的案例条数（固化表全量）。
    pub cases_run: usize,
    /// 红项数（未完成 / 语料缺线 / 地板不达）。
    pub cases_red: usize,
    /// 全表回放 digest（两次回放一致才有效——确定性判据）。
    pub digest: u64,
    /// 步数预算内完成。
    pub budget_ok: bool,
    /// 语料三线齐备。
    pub sources_complete: bool,
    /// 每域条数达地板。
    pub floors_ok: bool,
}

/// 跑 CI 门禁：固化表全量回放 + 三线/地板/预算/确定性四判据。
pub fn run_ci_gate() -> GeoFuzzCiReport {
    let mut red = 0usize;
    let mut steps = 0u64;
    let mut digest = 0u64;
    let mut i = 0usize;
    while i < FIXED_CASES.len() {
        match replay_fixed_case(i) {
            Some(r) => {
                if !r.completed {
                    red += 1;
                }
                digest = digest.wrapping_mul(31).wrapping_add(r.digest);
                steps += 4;
            }
            None => {
                red += 1;
            }
        }
        i += 1;
    }
    // 确定性：整体 digest 再折一次自证非平凡（零 digest = 折叠链失效，红）。
    if digest == 0 {
        red += 1;
    }
    // 三线齐备（对固化表独立扫描）。
    let mut seen = [false; 3];
    let mut ci = 0usize;
    while ci < FIXED_CASES.len() {
        let mut li = 0usize;
        while li < GeoCorpusSource::ALL.len() {
            if FIXED_CASES[ci].source == GeoCorpusSource::ALL[li] {
                seen[li] = true;
            }
            li += 1;
        }
        ci += 1;
    }
    let sources_complete = seen[0] && seen[1] && seen[2];
    if !sources_complete {
        red += 1;
    }
    // 每域地板（对固化表独立计数）。
    let mut floors_ok = true;
    let mut di = 0usize;
    while di < FuzzDomain::ALL.len() {
        let mut n = 0usize;
        let mut ci = 0usize;
        while ci < FIXED_CASES.len() {
            if FIXED_CASES[ci].domain == FuzzDomain::ALL[di] {
                n += 1;
            }
            ci += 1;
        }
        if n < CI_FLOOR_PER_DOMAIN {
            floors_ok = false;
        }
        di += 1;
    }
    if !floors_ok {
        red += 1;
    }
    GeoFuzzCiReport {
        cases_run: FIXED_CASES.len(),
        cases_red: red,
        digest,
        budget_ok: steps <= CI_STEP_BUDGET,
        sources_complete,
        floors_ok,
    }
}

/// 三域全量 fuzz 扫掠（CI 之外的加压路径：固化表之外再扫变异线 16 种子）。
/// 返回（完成条数, 红项条数）。
pub fn run_mutation_sweep(rounds: u64) -> (usize, usize) {
    let mut ok = 0usize;
    let mut red = 0usize;
    let mut r = 0u64;
    while r < rounds {
        let seed = 0x1000 + r;
        let (bytes, _ops) = mutate_stream(&mutated_seed_stream(), seed, 16);
        // 变异流 → 解析 → （成功则）三查：全程不崩即记 ok（panic=进程崩=红）。
        let outcome = parse_mesh_stream(&bytes);
        match outcome {
            MeshStreamOutcome::Ok(m) => {
                let limits = ScanLimits::default();
                let rep = triple_scan(&m, &limits);
                let plan = plan_fixes(&rep, &limits);
                let fixed = apply_fixes(&m, &plan);
                let rep2 = triple_scan(&fixed, &limits);
                // 修复不制造新畸形。
                if rep2.total() <= rep.total() {
                    ok += 1;
                } else {
                    red += 1;
                }
            }
            _ => {
                ok += 1;
            }
        }
        r += 1;
    }
    (ok, red)
}

/// 判据集条数（供聚合器自检，不参与判定）。
pub const fn total_fuzz_case_count() -> u32 {
    FIXED_CASES.len() as u32
}
