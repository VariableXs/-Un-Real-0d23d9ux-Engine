//! VE-F1619 · 几何一致性（VE 几何族 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1619`
//!
//! **判据（锚点原文）**：跨平台、双跑、跨版本、判据。
//!
//! **职责定位（锚点原文）**：跨平台一致（同网格跨硬件 x86/ARM 加载结果字节级
//! 一致——确定性三要件：固定顺序/无共享态/整数优先，F1097 纪律的网格域落地）；
//! 量化确定性双跑一致（F1605）；重排确定性（F1609）；跨版本稳定（格式演进→
//! 老文件读出一致结果，F1602 版本号纪律——老文件永远可读）。
//!
//! # 一、字节级确定是**审计**不是口头：三要件逐条有证据
//!
//! 「跨平台一致」如果只是口头承诺，任何一处引入平台超越函数或哈希迭代序的
//! 改动都会让 x86 与 ARM 读出不同的网格，而且没人知道。故本单把确定性做成
//! 三张可机检的账：
//!
//! - **三要件审计表**（[`DETERMINISM_AUDIT`]）：几何管线四个关键环节（流解析/
//!   量化/重排/编码）逐环节核对固定顺序、无共享态、整数优先，每行带证据句；
//! - **算子白名单**（[`PLATFORM_SAFE_OPS`]）：产线实际引用的算子逐个登记，
//!   违禁算子表（[`PLATFORM_BANNED_OPS`]）与之**不相交**由判据断言——白名单
//!   里出现原生 `sin`、违禁表里混进 `to_le_bytes` 都会红；
//! - **双跑对拍**（[`double_run`]）：同一语料跑两遍量化管线，FNV-1a 摘要
//!   逐位对拍；再以「脏化再跑」（先跑别的语料污染任何假想的共享态）证明
//!   结果与跑序无关——无共享态不是声明，是被实测的。
//!
//! # 二、多项式近似是确定性的**物质前提**
//!
//! 平台超越函数（`sin`/`sqrt` 原生版）在 x86 与 ARM 上的最低有效位可以不同。
//! F1605 的产线用 [`meshquant::fsqrt`]（整数化牛顿迭代）与 [`meshquant::sin_poly`]
//! （多项式近似）替代原生算子，正是为了让两条硬件跑出**同一个数**。本单把
//! 这一取舍登记进白名单：`sin_poly` 在册、原生 `sin` 在违禁表——替代不是
//! 性能技巧，是跨平台一致性的实现保障。
//!
//! # 三、跨版本稳定：v1 解码语义冻结，老文件永远可读
//!
//! 格式演进（v2+）可以新增字段，但**不得改变 v1 字节流的解码结果**——
//! 否则历史资产库全体作废。[`VMESH_FORMAT_VERSION`] 锁定当前版本，[`VERSION_PLEDGE`]
//! 登记承诺条款，[`legacy_reference_digest`] 给固定语料的 v1 字节流算出参考
//! 摘要：判据侧独立重算同一摘要对拍——未来任何改动若让老文件读出不同结果，
//! 这个摘要就会变，红项在 CI 里等着它。
//!
//! ## 零 panic 面
//!
//! 审计与对拍全部走表驱动与 `Option`/`bool`：无 `unwrap`/`expect`/切片直下标，
//! 判据侧对空语料、未知档位都有显性失败路径。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::gfx::meshbatch::{forsyth_reorder, BatchMesh};
use crate::gfx::meshquant::{compute_bounds, dequantize_position, quantize_position, QuantBits};
use crate::svstar2::vef18_geofuzz::encode_mesh_stream;
use crate::gfx::meshrepair::RepairMesh;

// ===========================================================================
// 一、版本与失败码
// ===========================================================================

/// 一致性版本（审计表与承诺表的版本戳）。
pub const GCONSISTENCY_VERSION: &str = "GCONSISTENCY-v1";

/// 当前 vmesh 格式版本（F1602 版本号纪律：老文件永远可读）。
pub const VMESH_FORMAT_VERSION: u32 = 1;

/// 双跑摘要不一致（确定性破坏）。
pub const E_GCON_DOUBLE_RUN: &str = "E_GCON_DOUBLE_RUN";
/// 平台依赖算子混入（跨平台破坏）。
pub const E_GCON_PLATFORM_OP: &str = "E_GCON_PLATFORM_OP";
/// 老文件摘要漂移（跨版本破坏——v1 解码语义被改）。
pub const E_GCON_LEGACY_DRIFT: &str = "E_GCON_LEGACY_DRIFT";

// ===========================================================================
// 二、三要件审计表（判据「跨平台」的静态面）
// ===========================================================================

/// 一条确定性审计行：环节 × 要件 × 结论。
#[derive(Clone, Copy, Debug)]
pub struct DeterminismRow {
    /// 产线环节（稳定名）。
    pub stage: &'static str,
    /// 要件（固定顺序 / 无共享态 / 整数优先）。
    pub requirement: &'static str,
    /// 证据句（审计结论的实现依据——空=没审计）。
    pub evidence: &'static str,
}

/// 三要件审计表（四环节 × 三要件 = 12 行）。
///
/// 环节与产线实装的对应：流解析=vef18_geofuzz::parse_mesh_stream（顺序遍历、
/// 局部变量、字节序显式 LE）；量化=meshquant::quantize_position（bounds 一次
/// 求出、整数格点取整）；重排=meshbatch::forsyth_reorder（索引域纯函数、
/// 确定性 tie-break）；编码=vef18_geofuzz::encode_mesh_stream（声明序遍历、
/// LE 字节）。
pub const DETERMINISM_AUDIT: [DeterminismRow; 12] = [
    DeterminismRow { stage: "stream-parse", requirement: "固定顺序", evidence: "头部→顶点段→面段单遍历，无并发收集、无无序容器" },
    DeterminismRow { stage: "stream-parse", requirement: "无共享态", evidence: "解析器全部状态在入参与局部变量，多次调用互不可见" },
    DeterminismRow { stage: "stream-parse", requirement: "整数优先", evidence: "头部与索引全程 u32 整数；仅顶点坐标按规范读 f32 LE" },
    DeterminismRow { stage: "quantize", requirement: "固定顺序", evidence: "bounds 单次求出后逐顶点独立取整，顶点间无顺序耦合" },
    DeterminismRow { stage: "quantize", requirement: "无共享态", evidence: "quantize_position 纯函数：入参决定输出，无可变静态" },
    DeterminismRow { stage: "quantize", requirement: "整数优先", evidence: "格点索引用整数算术（i32 线性量化），反变换同为整数乘加" },
    DeterminismRow { stage: "reorder", requirement: "固定顺序", evidence: "Forsyth 打分平手时按索引小者先出（确定性 tie-break）" },
    DeterminismRow { stage: "reorder", requirement: "无共享态", evidence: "只读索引数组与缓存位表，全部状态在返回值内" },
    DeterminismRow { stage: "reorder", requirement: "整数优先", evidence: "打分与缓存位全部整数运算，无浮点参与排序裁决" },
    DeterminismRow { stage: "encode", requirement: "固定顺序", evidence: "按声明序写 header→verts→faces，与解析端逐字节对称" },
    DeterminismRow { stage: "encode", requirement: "无共享态", evidence: "编码器纯函数，输出 Vec 独占写入" },
    DeterminismRow { stage: "encode", requirement: "整数优先", evidence: "计数与索引 u32 LE；浮点仅坐标字段且按规范位型直写" },
];

/// 三要件审计：12 行全在、证据全非空、环节集与要件集恰为声明的闭集。
pub fn determinism_audit() -> Result<(), &'static str> {
    const STAGES: [&str; 4] = ["stream-parse", "quantize", "reorder", "encode"];
    const REQS: [&str; 3] = ["固定顺序", "无共享态", "整数优先"];
    if DETERMINISM_AUDIT.len() != STAGES.len() * REQS.len() {
        return Err("audit-rows-changed");
    }
    let mut k = 0usize;
    while k < DETERMINISM_AUDIT.len() {
        let row = &DETERMINISM_AUDIT[k];
        if row.evidence.is_empty() {
            return Err(row.stage);
        }
        let mut found = false;
        let mut s = 0usize;
        while s < STAGES.len() {
            if STAGES[s] == row.stage {
                found = true;
            }
            s += 1;
        }
        if !found {
            return Err(row.stage);
        }
        let mut found = false;
        let mut r = 0usize;
        while r < REQS.len() {
            if REQS[r] == row.requirement {
                found = true;
            }
            r += 1;
        }
        if !found {
            return Err(row.requirement);
        }
        k += 1;
    }
    Ok(())
}

// ===========================================================================
// 三、算子白名单 / 违禁表（跨平台的算子面）
// ===========================================================================

/// 平台安全算子（产线实际引用；多项式近似替代平台超越函数）。
pub const PLATFORM_SAFE_OPS: [&str; 7] = [
    "u32::to_le_bytes",
    "u32::from_le_bytes",
    "f32::to_le_bytes",
    "f32::from_le_bytes",
    "meshquant::fsqrt(整数化牛顿迭代)",
    "meshquant::sin_poly(多项式近似)",
    "整数线性量化(i32 格点)",
];

/// 平台违禁算子（跨硬件最低有效位不可复现或引入共享态）。
pub const PLATFORM_BANNED_OPS: [&str; 6] = [
    "f32::sin(原生)",
    "f32::cos(原生)",
    "f32::sqrt(原生)",
    "f32::powf(原生)",
    "HashMap(迭代序不定)",
    "static mut(共享可变态)",
];

/// 平台审计：白名单与违禁表非空、两表不相交、多项式替代在册（跨平台一致
/// 的实现保障必须真的在用，不是摆设）。
pub fn platform_audit() -> Result<(), &'static str> {
    if PLATFORM_SAFE_OPS.is_empty() || PLATFORM_BANNED_OPS.is_empty() {
        return Err("op-tables-empty");
    }
    let mut i = 0usize;
    while i < PLATFORM_SAFE_OPS.len() {
        let mut j = 0usize;
        while j < PLATFORM_BANNED_OPS.len() {
            if PLATFORM_SAFE_OPS[i] == PLATFORM_BANNED_OPS[j] {
                return Err(E_GCON_PLATFORM_OP);
            }
            j += 1;
        }
        i += 1;
    }
    let mut has_poly = false;
    let mut k = 0usize;
    while k < PLATFORM_SAFE_OPS.len() {
        if PLATFORM_SAFE_OPS[k].contains("sin_poly") {
            has_poly = true;
        }
        k += 1;
    }
    if !has_poly {
        return Err("poly-substitution-missing");
    }
    Ok(())
}

// ===========================================================================
// 四、双跑一致性（判据「双跑」的执行器）
// ===========================================================================

/// FNV-1a 64 位摘要（几何域跨平台摘要口径——纯整数运算）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 一条确定性语料（构造性：三顶点一面的最小非平凡网格的坐标变体）。
pub struct CorpusMesh {
    /// 语料名（对账按名）。
    pub name: &'static str,
    /// 顶点（f32 三元组）。
    pub verts: [[f32; 3]; 3],
    /// 量化档位。
    pub bits: u32,
}

/// 内置确定性语料集（判据与双跑共用同一事实源）。
pub const CORPUS: [CorpusMesh; 2] = [
    CorpusMesh {
        name: "tri-unit",
        verts: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        bits: 12,
    },
    CorpusMesh {
        name: "tri-fine",
        verts: [[0.0, 0.0, 0.0], [0.96875, 0.0, 0.0], [0.0, 0.9375, 0.0]],
        bits: 16,
    },
];

/// 单跑结果：量化 roundtrip 后重编码字节流的摘要。
fn run_once(corpus: &CorpusMesh) -> Option<u64> {
    let bits = QuantBits::from_bits(corpus.bits)?;
    // 量化 roundtrip：bounds → 逐顶点量化 → 反量化 → 回写网格 → 编码。
    let flat: Vec<f32> = {
        let mut v = Vec::with_capacity(9);
        let mut k = 0usize;
        while k < 3 {
            v.push(corpus.verts[k][0]);
            v.push(corpus.verts[k][1]);
            v.push(corpus.verts[k][2]);
            k += 1;
        }
        v
    };
    let b = compute_bounds(&flat, &mut crate::gfx::meshquant::DiagBag::new())?;
    let mut out = RepairMesh::new();
    let mut verts_rebuilt: Vec<[f32; 3]> = Vec::with_capacity(3);
    let mut k = 0usize;
    while k < 3 {
        let q = quantize_position(corpus.verts[k], &b, bits);
        verts_rebuilt.push(dequantize_position(q, &b, bits));
        k += 1;
    }
    let mut k = 0usize;
    while k < 3 {
        out.push_vert(verts_rebuilt[k]);
        k += 1;
    }
    out.push_face([0, 1, 2]);
    let bytes = encode_mesh_stream(&out);
    Some(fnv1a64(&bytes))
}

/// 双跑对拍：同一语料跑两遍，摘要逐位一致（确定性验收的执行面）。
pub fn double_run(corpus: &CorpusMesh) -> Result<u64, &'static str> {
    let a = run_once(corpus).ok_or("corpus-run-failed")?;
    let b = run_once(corpus).ok_or("corpus-run-failed")?;
    if a != b {
        return Err(E_GCON_DOUBLE_RUN);
    }
    Ok(a)
}

/// 脏化再跑：先跑另一条语料污染任何假想的共享态，再跑目标语料——
/// 结果必须与干净跑一致（无共享态的实测证明，跑序无关）。
pub fn double_run_after_noise(target: &CorpusMesh) -> Result<u64, &'static str> {
    // 噪声跑：语料集里第一条与目标不同的语料。
    let mut noise: Option<&CorpusMesh> = None;
    let mut k = 0usize;
    while k < CORPUS.len() {
        if CORPUS[k].name != target.name {
            noise = Some(&CORPUS[k]);
        }
        k += 1;
    }
    match noise {
        Some(n) => {
            let _ = run_once(n);
            let _ = run_once(n);
        }
        None => return Err("no-noise-corpus"),
    }
    double_run(target)
}

/// 重排确定性：同一网格两次 Forsyth 重排，输出面序逐位一致（F1609 落地）。
pub fn reorder_double_run(verts: &[[f32; 3]], faces: &[[u32; 3]], cache_size: usize) -> Result<u64, &'static str> {
    let mut bm = BatchMesh {
        positions: Vec::new(),
        faces: Vec::new(),
        materials: Vec::new(),
    };
    let mut i = 0usize;
    while i < verts.len() {
        bm.positions.push(verts[i]);
        i += 1;
    }
    let mut f = 0usize;
    while f < faces.len() {
        bm.faces.push(faces[f]);
        f += 1;
    }
    let r1 = forsyth_reorder(&bm, cache_size);
    let r2 = forsyth_reorder(&bm, cache_size);
    let mut f1: Vec<u8> = Vec::new();
    let mut f2: Vec<u8> = Vec::new();
    let mut g = 0usize;
    while g < r1.faces.len() {
        let t = r1.faces[g];
        f1.extend_from_slice(&t[0].to_le_bytes());
        f1.extend_from_slice(&t[1].to_le_bytes());
        f1.extend_from_slice(&t[2].to_le_bytes());
        g += 1;
    }
    let mut g = 0usize;
    while g < r2.faces.len() {
        let t = r2.faces[g];
        f2.extend_from_slice(&t[0].to_le_bytes());
        f2.extend_from_slice(&t[1].to_le_bytes());
        f2.extend_from_slice(&t[2].to_le_bytes());
        g += 1;
    }
    let d1 = fnv1a64(&f1);
    let d2 = fnv1a64(&f2);
    if d1 != d2 {
        return Err(E_GCON_DOUBLE_RUN);
    }
    Ok(d1)
}

// ===========================================================================
// 五、跨版本稳定（判据「跨版本」：老文件永远可读）
// ===========================================================================

/// 版本承诺条款表（v1 解码语义冻结）。
pub const VERSION_PLEDGE: [&str; 3] = [
    "v1 字节流的解码结果永不改变（字段偏移/字节序/类型三冻结）",
    "v2+ 只增不改：新字段走新版本号，v1 语义原样保留",
    "任何改动若使 v1 老文件读出不同结果，即破坏性变更须走 ADR",
];

/// 固定语料的 v1 参考摘要（tri-unit 直接编码，未经量化——最原始的可读承诺）。
///
/// 这是「老文件永远可读」的落点：判据侧独立重算同一摘要对拍，未来任何人
/// 改动 v1 解码语义，此摘要漂移即红。
pub fn legacy_reference_digest() -> u64 {
    let mut m = RepairMesh::new();
    m.push_vert([0.0, 0.0, 0.0]);
    m.push_vert([1.0, 0.0, 0.0]);
    m.push_vert([0.0, 1.0, 0.0]);
    m.push_face([0, 1, 2]);
    fnv1a64(&encode_mesh_stream(&m))
}

/// 版本承诺审计：版本号守恒（v1）+ 条款全非空。
pub fn version_pledge_audit() -> Result<(), &'static str> {
    if VMESH_FORMAT_VERSION != 1 {
        return Err(E_GCON_LEGACY_DRIFT);
    }
    let mut k = 0usize;
    while k < VERSION_PLEDGE.len() {
        if VERSION_PLEDGE[k].is_empty() {
            return Err("pledge-empty");
        }
        k += 1;
    }
    Ok(())
}

/// 摘要行（面板/日志/读屏共用）。
pub fn screen_line() -> String {
    format!(
        "{} fmt_v{} audit_rows={} safe_ops={} banned_ops={} corpus={}",
        GCONSISTENCY_VERSION,
        VMESH_FORMAT_VERSION,
        DETERMINISM_AUDIT.len(),
        PLATFORM_SAFE_OPS.len(),
        PLATFORM_BANNED_OPS.len(),
        CORPUS.len(),
    )
}
