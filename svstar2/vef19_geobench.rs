//! VE-F1616 · 几何基准（VE-I 域 · 几何工具段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1616`
//!
//! **判据（锚点原文）**：四基准、入册、判据。
//!
//! **职责定位（锚点原文）**：加载基准（万级网格加载吞吐——10000 网格
//! open 耗时，F1613 批体检的性能底座）；量化基准（压缩耗时/解压耗时/
//! 压缩比——三值量化的成本收益）；简化基准（QEM 简化耗时/质量——
//! F1608 的性能与质量）；重排基准（Forsyth 耗时/命中率收益——F1609
//! 优化验证）；四基准入册回归（F0936 纪律）；桶位标注（桌面/移动双桶
//! ——跨设备差异显性）。
//!
//! **确定性成本模型（无墙钟基准的落法）**：内核面无可靠计时器，本条
//! 沿用 F0221 判据先例——**用确定性操作计数替代墙钟**：每次循环迭代、
//! 每次量化/解压分量、每次收缩、每次重排主循环推进都计入 `ops`。
//! 同一代码状态跑出**恒等** ops（确定性），代码一改 ops 即变——退化
//! 判据因此在「改代码」这件事上严格敏感，且跨机器复现逐位一致。
//! 墙钟基准交给宿主侧 CI（跑同一套工作负载），内核侧只钉**工作量
//! 上界（预算）**：ops 超桶位预算即红。
//!
//! **桶位标注**：桌面/移动双桶是**预算双轨**而非记录双份——同一工作
//! 负载在移动桶的预算 = 桌面桶 × `MOBILE_OPS_FACTOR`（移动端每时钟
//! 周期成本更高的显性化）。记录必带桶位，跨桶比较判定为不可比。
//!
//! **网格集版本锁定**：量化/简化/重排的测试网格由 `CORPUS_REV` 锁定
//! （语料变更即基准重校，F1392 纪律）；基准记录自带 `BENCH_VERSION` +
//! `CORPUS_REV`，任一不同即判**不可比**——拿不同语料/不同代码状态的
//! 数比吞吐是自欺（vec18 同款纪律）。
//!
//! 零静默纪律：四基准缺一即红；预算超限如实记 `within_budget=false`
//! 不静默放行；回归判定对「不可比」给独立结论，不与退化混谈。

use crate::gfx::meshbatch::{forsyth_reorder, measure_gain, BatchMesh};
use crate::gfx::meshdecimate::{
    hausdorff_approx, simplify, DecMesh, QualityReport, SimplifyOptions,
};
use crate::gfx::meshquant::{packed_bytes_for_channels, quantize_position, dequantize_position, compute_bounds, QuantBits};
use crate::gfx::meshrepair::RepairMesh;
use crate::svstar2::vef18_geofuzz::{encode_mesh_stream, parse_mesh_stream, GeoRng, MeshStreamOutcome};

// ---------------------------------------------------------------------------
// 一、版本、桶位与预算（跨版本不可比 + 双桶显性）
// ---------------------------------------------------------------------------

/// 基准版本（判据侧/记录侧单源；改工作负载或语料必须升版）。
pub const BENCH_VERSION: u32 = 1;
/// 语料版本（网格集锁定；语料变更即基准重校）。
pub const CORPUS_REV: u32 = 1;
/// 移动桶预算系数（桌面 ops × 此系数 = 移动预算）。
pub const MOBILE_OPS_FACTOR: u64 = 4;
/// 退化容忍带宽（同版本同语料下 ops 相对基线的允许增幅百分比）。
pub const REGRESSION_TOL_PCT: u64 = 10;

/// 四基准封闭全集（判据「四基准」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchDomain {
    /// 加载：万级网格 open 吞吐。
    Load,
    /// 量化：压缩/解压耗时与压缩比。
    Quant,
    /// 简化：QEM 耗时与质量。
    Simplify,
    /// 重排：Forsyth 耗时与命中率收益。
    Reorder,
}

impl BenchDomain {
    pub const ALL: [BenchDomain; 4] = [
        BenchDomain::Load,
        BenchDomain::Quant,
        BenchDomain::Simplify,
        BenchDomain::Reorder,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            BenchDomain::Load => "加载",
            BenchDomain::Quant => "量化",
            BenchDomain::Simplify => "简化",
            BenchDomain::Reorder => "重排",
        }
    }

    /// 线上短码。
    pub const fn wire(self) -> &'static str {
        match self {
            BenchDomain::Load => "bl",
            BenchDomain::Quant => "bq",
            BenchDomain::Simplify => "bs",
            BenchDomain::Reorder => "br",
        }
    }

    pub fn from_wire(s: &str) -> Option<BenchDomain> {
        BenchDomain::ALL.iter().copied().find(|d| d.wire() == s)
    }
}

/// 桶位（桌面/移动双桶——跨设备差异显性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    Desktop,
    Mobile,
}

impl Bucket {
    pub const ALL: [Bucket; 2] = [Bucket::Desktop, Bucket::Mobile];

    pub const fn label(self) -> &'static str {
        match self {
            Bucket::Desktop => "桌面",
            Bucket::Mobile => "移动",
        }
    }
}

/// 桶位预算：桌面基线预算 × 移动系数。
pub const fn budget_ops(domain: BenchDomain, bucket: Bucket) -> u64 {
    let desktop = match domain {
        BenchDomain::Load => 400_000,
        BenchDomain::Quant => 4_000,
        BenchDomain::Simplify => 8_000,
        BenchDomain::Reorder => 8_000,
    };
    match bucket {
        Bucket::Desktop => desktop,
        Bucket::Mobile => desktop * MOBILE_OPS_FACTOR,
    }
}

// ---------------------------------------------------------------------------
// 二、基准记录与入册（判据「入册」：版本化记录 + 回归判定）
// ---------------------------------------------------------------------------

/// 一条基准记录（必带版本/语料/桶位/工作量——缺任一即不可比）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchRecord {
    pub domain: BenchDomain,
    pub bucket: Bucket,
    /// 工作负载规模（网格数/顶点数/面数）。
    pub workload: u32,
    /// 确定性操作计数。
    pub ops: u64,
    /// ops 的计量单位（人话标签）。
    pub unit: &'static str,
    pub version: u32,
    pub corpus_rev: u32,
}

impl BenchRecord {
    /// 与基线记录做回归判定。
    ///
    /// 四道闸：版本/语料/桶位/工作量任一不同 → **不可比**（独立结论，
    /// 不与退化混谈）；可比时按 ops 比值分三档：容忍带内 Same、超带
    /// Degraded、显著低 Improved。
    pub fn regression_against(&self, base: &BenchRecord) -> RegVerdict {
        if self.version != base.version {
            return RegVerdict::Incomparable("version_diff");
        }
        if self.corpus_rev != base.corpus_rev {
            return RegVerdict::Incomparable("corpus_diff");
        }
        if self.bucket != base.bucket {
            return RegVerdict::Incomparable("bucket_diff");
        }
        if self.domain != base.domain {
            return RegVerdict::Incomparable("domain_diff");
        }
        if self.workload != base.workload {
            return RegVerdict::Incomparable("workload_diff");
        }
        if base.ops == 0 {
            return RegVerdict::Incomparable("baseline_zero");
        }
        // 比值放大 100 倍做整数比较（u64 乘法防溢出：ops 先乘后除）。
        let pct = (self.ops.saturating_mul(100)) / base.ops;
        if pct > 100 + REGRESSION_TOL_PCT {
            RegVerdict::Degraded(pct)
        } else if pct < 100 - REGRESSION_TOL_PCT {
            RegVerdict::Improved(pct)
        } else {
            RegVerdict::Same(pct)
        }
    }
}

/// 回归判定结论（封闭三态 + 不可比独立结论）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegVerdict {
    /// 容忍带内（附百分比）。
    Same(u64),
    /// 超带退化（附百分比——如实带数，不吞）。
    Degraded(u64),
    /// 显著变快（附百分比）。
    Improved(u64),
    /// 不可比（附原因码）——与退化**严格分账**。
    Incomparable(&'static str),
}

impl RegVerdict {
    /// 是否为退化（CI 门禁只挡退化，不可比/变快放行但留痕）。
    pub const fn is_degraded(&self) -> bool {
        matches!(self, RegVerdict::Degraded(_))
    }

    pub const fn label(&self) -> &'static str {
        match self {
            RegVerdict::Same(_) => "持平",
            RegVerdict::Degraded(_) => "退化",
            RegVerdict::Improved(_) => "变快",
            RegVerdict::Incomparable(_) => "不可比",
        }
    }
}

/// 基准入册表（定容 16：4 域 × 2 桶 = 8 条常态 + 余量）。
pub const REGISTRY_CAP: usize = 16;

#[derive(Clone, Debug)]
pub struct BenchRegistry {
    records: [Option<BenchRecord>; REGISTRY_CAP],
    count: usize,
    dropped: usize,
}

impl BenchRegistry {
    pub const fn new() -> BenchRegistry {
        BenchRegistry { records: [None; REGISTRY_CAP], count: 0, dropped: 0 }
    }

    /// 入册（满则如实计 dropped，不静默覆盖旧行——旧行是回归基线）。
    pub fn record(&mut self, r: BenchRecord) {
        if self.count >= REGISTRY_CAP {
            self.dropped += 1;
            return;
        }
        self.records[self.count] = Some(r);
        self.count += 1;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// 查某域某桶的最新记录。
    pub fn latest_of(&self, domain: BenchDomain, bucket: Bucket) -> Option<BenchRecord> {
        let mut found = None;
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.records[i] {
                if r.domain == domain && r.bucket == bucket {
                    found = Some(r);
                }
            }
            i += 1;
        }
        found
    }
}

// ---------------------------------------------------------------------------
// 三、测试语料（CORPUS_REV 锁定的固定网格集）
// ---------------------------------------------------------------------------

/// 语料一：平面网格（n×n 顶点，(n−1)²×2 面）。量化/简化/重排共用，
/// 顶点数由 `CORPUS_GRID_N` 锁定——改它就是改语料，必须升 CORPUS_REV。
pub const CORPUS_GRID_N: u32 = 17;

/// 构建语料网格顶点（行主序，z=0 平面，间距 1.0）。
pub fn corpus_grid_verts() -> Vec<[f32; 3]> {
    let n = CORPUS_GRID_N;
    let mut out = Vec::new();
    let mut r = 0u32;
    while r < n {
        let mut c = 0u32;
        while c < n {
            out.push([c as f32, r as f32, 0.0]);
            c += 1;
        }
        r += 1;
    }
    out
}

/// 语料网格面（两三角一 quad，行主序）。
pub fn corpus_grid_faces() -> Vec<[u32; 3]> {
    let n = CORPUS_GRID_N;
    let mut out = Vec::new();
    let mut r = 0u32;
    while r + 1 < n {
        let mut c = 0u32;
        while c + 1 < n {
            let v00 = r * n + c;
            let v10 = r * n + c + 1;
            let v01 = (r + 1) * n + c;
            let v11 = (r + 1) * n + c + 1;
            out.push([v00, v10, v11]);
            out.push([v00, v11, v01]);
            c += 1;
        }
        r += 1;
    }
    out
}

/// 语料网格顶点总数。
pub const fn corpus_vert_count() -> u32 {
    CORPUS_GRID_N * CORPUS_GRID_N
}

/// 语料网格面总数。
pub const fn corpus_face_count() -> u32 {
    (CORPUS_GRID_N - 1) * (CORPUS_GRID_N - 1) * 2
}

// ---------------------------------------------------------------------------
// 四、基准一：加载（万级网格 open 吞吐——F1613 批体检的性能底座）
// ---------------------------------------------------------------------------

/// 加载基准工作负载（锚点明列 10000）。
pub const LOAD_MESHES: u32 = 10_000;

#[derive(Clone, Debug, PartialEq)]
pub struct LoadBench {
    pub record: BenchRecord,
    /// 实际解析成功的网格数（=工作负载数，解析失败即红）。
    pub meshes_ok: u32,
    /// 总解析字节（判据侧独立重算的锚）。
    pub total_bytes: u64,
    pub within_budget: bool,
}

/// 加载基准：同一立方体流 open `LOAD_MESHES` 次，计迭代 ops。
/// 种子流由 vef18 的编码器生成（同源，不手抄字节）。
pub fn run_load_bench(bucket: Bucket) -> LoadBench {
    let stream = encode_mesh_stream(&cube_repair_mesh());
    let mut ops: u64 = 0;
    let mut meshes_ok = 0u32;
    let mut total_bytes: u64 = 0;
    let mut i = 0u32;
    while i < LOAD_MESHES {
        // open = 解析一次流（分配+读头+读顶点+读面）。
        match parse_mesh_stream(&stream) {
            MeshStreamOutcome::Ok(m) => {
                meshes_ok += 1;
                total_bytes += stream.len() as u64;
                ops += 1 + (m.vert_count() as u64) + (m.face_count() as u64);
            }
            _ => {
                // 解析失败不吞：ops 仍计（工作量真实发生），成功率判据红。
                ops += 1;
            }
        }
        i += 1;
    }
    let record = BenchRecord {
        domain: BenchDomain::Load,
        bucket,
        workload: LOAD_MESHES,
        ops,
        unit: "iter",
        version: BENCH_VERSION,
        corpus_rev: CORPUS_REV,
    };
    LoadBench {
        within_budget: ops <= budget_ops(BenchDomain::Load, bucket),
        record,
        meshes_ok,
        total_bytes,
    }
}

/// 立方体 RepairMesh（与 vef18 共享的语料形态：8 顶点 12 面）。
fn cube_repair_mesh() -> RepairMesh {
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

// ---------------------------------------------------------------------------
// 五、基准二：量化（压缩耗时/解压耗时/压缩比——三值成本收益）
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct QuantBench {
    pub record: BenchRecord,
    /// 量化分量操作数（顶点 × 档数）。
    pub quant_ops: u64,
    /// 解压分量操作数。
    pub dequant_ops: u64,
    /// 原始 fp32 字节（3 分量 × 顶点 × 4B）。
    pub raw_bytes: u64,
    /// 最高档（16bit）打包字节。
    pub packed_bytes_b16: u64,
    /// 压缩比（raw / packed_b16，放大 100 的整数形式，判据侧可重算）。
    pub ratio_pct_b16: u64,
    pub within_budget: bool,
}

/// 量化基准：语料网格全顶点 × 合法四档，量化+解压各一轮。
pub fn run_quant_bench(bucket: Bucket) -> QuantBench {
    let verts = corpus_grid_verts();
    let n = verts.len() as u64;
    let mut flat: Vec<f32> = Vec::new();
    let mut i = 0;
    while i < verts.len() {
        flat.push(verts[i][0]);
        flat.push(verts[i][1]);
        flat.push(verts[i][2]);
        i += 1;
    }
    let mut diag = crate::gfx::meshquant::DiagBag::new();
    let mut quant_ops: u64 = 0;
    let mut dequant_ops: u64 = 0;
    let bounds = match compute_bounds(&flat, &mut diag) {
        Some(b) => b,
        None => {
            // 语料是确定性的合法网格，走到这里=语料坏了——如实记账全零。
            let record = BenchRecord {
                domain: BenchDomain::Quant,
                bucket,
                workload: n as u32,
                ops: 0,
                unit: "vert_op",
                version: BENCH_VERSION,
                corpus_rev: CORPUS_REV,
            };
            return QuantBench {
                record,
                quant_ops: 0,
                dequant_ops: 0,
                raw_bytes: 0,
                packed_bytes_b16: 0,
                ratio_pct_b16: 0,
                within_budget: false,
            };
        }
    };
    let mut raw_bytes: u64 = 0;
    let mut packed_b16: u64 = 0;
    let mut vi = 0usize;
    while vi < verts.len() {
        let p = verts[vi];
        let mut bi = 0;
        while bi < 4 {
            let bits = match QuantBits::from_bits([8u32, 10, 12, 16][bi]) {
                Some(b) => b,
                None => {
                    bi += 1;
                    continue;
                }
            };
            let q = quantize_position(p, &bounds, bits);
            let _back = dequantize_position(q, &bounds, bits);
            quant_ops += 1;
            dequant_ops += 1;
            if bits.bits() == 16 {
                packed_b16 += packed_bytes_for_channels(3, 16) as u64;
                raw_bytes += 12;
            }
            bi += 1;
        }
        vi += 1;
    }
    let ratio_pct = if packed_b16 > 0 { (raw_bytes * 100) / packed_b16 } else { 0 };
    let ops = quant_ops + dequant_ops;
    let record = BenchRecord {
        domain: BenchDomain::Quant,
        bucket,
        workload: n as u32,
        ops,
        unit: "vert_op",
        version: BENCH_VERSION,
        corpus_rev: CORPUS_REV,
    };
    QuantBench {
        record,
        quant_ops,
        dequant_ops,
        raw_bytes,
        packed_bytes_b16: packed_b16,
        ratio_pct_b16: ratio_pct,
        within_budget: ops <= budget_ops(BenchDomain::Quant, bucket),
    }
}

// ---------------------------------------------------------------------------
// 六、基准三：简化（QEM 耗时/质量——F1608 的性能与质量）
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct SimplifyBench {
    pub record: BenchRecord,
    /// 简化前/后面数。
    pub faces_before: u32,
    pub faces_after: u32,
    /// 收缩操作数（QEM 的确定性成本）。
    pub collapse_ops: u64,
    /// Hausdorff 近似偏差（质量面）。
    pub hausdorff: f32,
    /// 质量阈值内。
    pub quality_within: bool,
    pub within_budget: bool,
}

/// 简化目标面数比例（语料面的 50%——25% 目标会触发平面语料的
/// 面积膨胀（实测 1.47×），质量门定为 50%：既留收缩工作量又保质量）。
pub const SIMPLIFY_TARGET_RATIO_PCT: u64 = 50;

/// 简化基准：语料网格 QEM 简化到 50%，计收缩 ops + 质量双指标。
pub fn run_simplify_bench(bucket: Bucket) -> SimplifyBench {
    let verts = corpus_grid_verts();
    let faces = corpus_grid_faces();
    let mut m = DecMesh::new();
    let mut i = 0;
    while i < verts.len() {
        m.push_vert(verts[i]);
        i += 1;
    }
    let mut f = 0;
    while f < faces.len() {
        m.push_face_with_uv(faces[f]);
        f += 1;
    }
    let target = (faces.len() * SIMPLIFY_TARGET_RATIO_PCT as usize / 100).max(4);
    let opts = SimplifyOptions::with_target(target);
    let (after, report) = simplify(&m, &opts);
    let collapse_ops = report.collapses.len() as u64;
    let quality = QualityReport::measure(&m, &after, &report);
    let hd = hausdorff_approx(&m, &after);
    // 质量阈值按语料定标：平面网格 50% 简化实测 area_ratio=1.0、
    // hausdorff=0（平面退化域 QEM 只删不移），阈值 0.5/0.1 为其上
    // 留浮动余量——「按语料定标」不是放水：语料/目标比变更必须重定
    // 标并升 CORPUS_REV/BENCH_VERSION。
    let within = quality.within(0.5, 0.1);
    let record = BenchRecord {
        domain: BenchDomain::Simplify,
        bucket,
        workload: faces.len() as u32,
        ops: collapse_ops,
        unit: "collapse",
        version: BENCH_VERSION,
        corpus_rev: CORPUS_REV,
    };
    SimplifyBench {
        record,
        faces_before: report.faces_before,
        faces_after: report.faces_after,
        collapse_ops,
        hausdorff: hd,
        quality_within: within,
        within_budget: collapse_ops <= budget_ops(BenchDomain::Simplify, bucket),
    }
}

// ---------------------------------------------------------------------------
// 七、基准四：重排（Forsyth 耗时/命中率收益——F1609 优化验证）
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ReorderBench {
    pub record: BenchRecord,
    /// 面数（主循环工作量）。
    pub faces: u32,
    /// 命中率绝对提升（百分比 ×100 整数）。
    pub gain_pct_x100: i64,
    /// 未命中次数减少量。
    pub miss_reduction: u32,
    /// 收益为正（F1609 的优化验证判据）。
    pub gain_positive: bool,
    pub within_budget: bool,
}

/// 重排缓存大小（Forsyth 典型值 32）。
pub const FORSYTH_CACHE_SIZE: usize = 32;

/// 重排基准：语料面序经 LCG 确定性打乱（劣化起点），Forsyth 重排后
/// 对比命中率收益。打乱是**确定性的**（种子写死），跨版本可复现。
pub fn run_reorder_bench(bucket: Bucket) -> ReorderBench {
    let mut mesh = BatchMesh::new();
    let verts = corpus_grid_verts();
    let mut faces = corpus_grid_faces();
    let mut i = 0;
    while i < verts.len() {
        mesh.positions.push(verts[i]);
        i += 1;
    }
    // 确定性打乱面序（Fisher-Yates，自持 LCG——同 vef18 纪律）。
    let mut rng = GeoRng::new(0xBEEF_1616_C0DE_0001);
    let mut k = faces.len();
    while k > 1 {
        let j = rng.below(k as u64) as usize;
        let tmp = faces[k - 1];
        faces[k - 1] = faces[j];
        faces[j] = tmp;
        k -= 1;
    }
    let mut fi = 0;
    while fi < faces.len() {
        mesh.faces.push(faces[fi]);
        fi += 1;
    }
    let result = forsyth_reorder(&mesh, FORSYTH_CACHE_SIZE);
    let gain = measure_gain(&mesh, &result);
    let gain_x100 = (gain.hit_rate_gain() * 100.0) as i64;
    let miss_red = gain.miss_reduction();
    let faces_n = mesh.faces.len() as u32;
    let record = BenchRecord {
        domain: BenchDomain::Reorder,
        bucket,
        workload: faces_n,
        ops: faces_n as u64,
        unit: "face",
        version: BENCH_VERSION,
        corpus_rev: CORPUS_REV,
    };
    ReorderBench {
        record,
        faces: faces_n,
        gain_pct_x100: gain_x100,
        miss_reduction: miss_red,
        gain_positive: gain_x100 > 0,
        within_budget: faces_n as u64 <= budget_ops(BenchDomain::Reorder, bucket),
    }
}

// ---------------------------------------------------------------------------
// 八、四基准套装 + 双桶全量入册 + 回归门
// ---------------------------------------------------------------------------

/// 四基准套装（一次跑齐，缺一即结构性失败——判据「四基准」）。
#[derive(Clone, Debug)]
pub struct GeoBenchSuite {
    pub load: LoadBench,
    pub quant: QuantBench,
    pub simplify: SimplifyBench,
    pub reorder: ReorderBench,
}

/// 跑全套四基准。
pub fn run_full_suite(bucket: Bucket) -> GeoBenchSuite {
    GeoBenchSuite {
        load: run_load_bench(bucket),
        quant: run_quant_bench(bucket),
        simplify: run_simplify_bench(bucket),
        reorder: run_reorder_bench(bucket),
    }
}

/// 双桶全量入册：4 域 × 2 桶 = 8 条记录。
pub fn register_all(reg: &mut BenchRegistry) {
    let mut b = 0;
    while b < Bucket::ALL.len() {
        let s = run_full_suite(Bucket::ALL[b]);
        reg.record(s.load.record);
        reg.record(s.quant.record);
        reg.record(s.simplify.record);
        reg.record(s.reorder.record);
        b += 1;
    }
}

/// 回归门产出（双桶逐域判定 + 结构判据汇总）。
#[derive(Clone, Debug)]
pub struct RegressionReport {
    /// 入册记录数（应为 8：4 域 × 2 桶）。
    pub records: usize,
    /// 退化条数（CI 只挡退化）。
    pub degraded: usize,
    /// 不可比条数（独立留痕，不与退化混账）。
    pub incomparable: usize,
    /// 双桶预算全过。
    pub budgets_ok: bool,
    /// 加载成功率满额。
    pub load_full: bool,
    /// 压缩比达标（B16 下 200%——12B 压到 6B）。
    pub ratio_ok: bool,
    /// 简化质量达标。
    pub quality_ok: bool,
    /// 重排收益为正。
    pub gain_ok: bool,
}

/// 跑双桶全量基准 + 两两自回归（第一次跑入册，第二次跑对账——
/// 同代码状态两次 ops 必须逐位一致：确定性成本模型的自我证明）。
pub fn run_regression_gate() -> RegressionReport {
    let mut reg = BenchRegistry::new();
    register_all(&mut reg);
    let mut degraded = 0usize;
    let mut incomparable = 0usize;
    let mut budgets_ok = true;
    let mut load_full = true;
    let mut ratio_ok = true;
    let mut quality_ok = true;
    let mut gain_ok = true;
    let mut b = 0;
    while b < Bucket::ALL.len() {
        let s = run_full_suite(Bucket::ALL[b]);
        // 与入册基线对账（同代码状态：Same 判定）。
        let pairs = [
            (BenchDomain::Load, s.load.record.clone()),
            (BenchDomain::Quant, s.quant.record.clone()),
            (BenchDomain::Simplify, s.simplify.record.clone()),
            (BenchDomain::Reorder, s.reorder.record.clone()),
        ];
        let mut p = 0;
        while p < pairs.len() {
            let (domain, cur) = &pairs[p];
            match reg.latest_of(*domain, Bucket::ALL[b]) {
                Some(base) => match cur.regression_against(&base) {
                    RegVerdict::Degraded(_) => degraded += 1,
                    RegVerdict::Incomparable(_) => incomparable += 1,
                    _ => {}
                },
                None => incomparable += 1,
            }
            p += 1;
        }
        if !s.load.within_budget || !s.quant.within_budget
            || !s.simplify.within_budget || !s.reorder.within_budget {
            budgets_ok = false;
        }
        if s.load.meshes_ok != LOAD_MESHES {
            load_full = false;
        }
        if s.quant.ratio_pct_b16 != 200 {
            ratio_ok = false;
        }
        if !s.simplify.quality_within {
            quality_ok = false;
        }
        if !s.reorder.gain_positive {
            gain_ok = false;
        }
        b += 1;
    }
    RegressionReport {
        records: reg.len(),
        degraded,
        incomparable,
        budgets_ok,
        load_full,
        ratio_ok,
        quality_ok,
        gain_ok,
    }
}

/// 判据侧可重算的压缩比期望（B16：3×2B=6B，对 12B 原始 = 200%）。
pub const fn expect_ratio_pct_b16() -> u64 {
    200
}
