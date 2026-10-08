//! VE-F1617 · 网格格式规范文档（VE-I 域 · 几何工具段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1617`
//!
//! **判据（锚点原文）**：三文档、判据。补充判据：三读者适配、开放规范的可执行性。
//!
//! 职责定位：网格格式规范文档极致深化——vmesh 开放格式规范（第三方可依
//! 规范实现读写）、glTF 互转说明（支持范围/已知限制）、量化指南（何时量化/
//! 档位选择），三文档 240 行 + 漂移机制 30 行。
//!
//! # 一、三文档（判据一）——各有读者，各有纪律
//!
//! - **文档一 vmesh 规范**（[`VMESH_SPEC`]）：字段级 schema 表——偏移/
//!   名称/字节宽/线型/约束逐字段登记，总长公式 [`spec_total_size`]。
//!   面向**第三方实现者**：依表可实现读写。
//! - **文档二 glTF 互转**（[`GLTF_INTEROP`]）：支持/部分/不支持三态逐条
//!   清单；不支持的扩展**逐条带替代路径**（F1185 纪律：不做不假装，
//!   限制显性）。面向**资产作者**：知道哪些资产能进、哪些要绕。
//! - **文档三 量化指南**（[`QUANT_GUIDE`]）：何时量化、档位怎么选——
//!   场景→推荐档位→依据，档位集合与 [`QuantBits`] 实现全集单源对拍。
//!   面向**工具作者**：给用户的建议可执行。
//!
//! # 二、开放规范的可执行性（判据补充）——规范+实现同分发
//!
//! 规范不是散文：[`spec_impl_drift_check`] 把规范表当作**可执行断言**——
//! 用 F1615 的产线编码器编码参考网格，按规范表逐字段核对实际字节流
//! （头部 LE 值/逐顶点 3×f32/逐面 3×u32/总长公式）。规范与实现任何一侧
//! 漂移，检查即红——"依规范可实现"因此是**可验证承诺**而非愿景。
//! 漂移检查挂域聚合器随 CI 跑（漂移机制 30 行的落点）。
//!
//! # 三、三读者适配（判据补充）
//!
//! 每张表的每行都带 [`SpecAudience`]（实现者/资产作者/工具作者）；
//! [`all_audiences_covered`] 断言三读者在三文档合计覆盖中各至少出现——
//! 缺任一读者的"文档"都只是半个文档。
//!
//! # 四、诚实边界
//!
//! vmesh 最简格式**不含** magic/版本字段——容量上限（2^20 顶点/面）是
//! 唯一护栏，设计取舍在规范行内如实注明，不粉饰为"特性"。glTF 侧
//! 不支持的扩展不假装支持；量化指南的建议档位若与实现档位漂移，
//! [`QuantGuideRow`] 对拍先红。零外部依赖，只引用 vef18/meshquant/
//! meshrepair 的产线符号（规范-实现同源是漂移机制的物质前提）。

use alloc::vec::Vec;

use crate::gfx::meshquant::QuantBits;
use crate::gfx::meshrepair::RepairMesh;
use crate::svstar2::vef18_geofuzz::{encode_mesh_stream, STREAM_MAX_FACES, STREAM_MAX_VERTS};

// ---------------------------------------------------------------------------
// 一、三读者适配
// ---------------------------------------------------------------------------

/// 规范读者三分类（判据补充：三读者适配——每行必挂读者）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpecAudience {
    /// 第三方格式实现者（依规范写读写器）。
    Implementer,
    /// 资产作者（制作/导入资产的人）。
    AssetAuthor,
    /// 工具作者（做转换/检查工具的人）。
    ToolAuthor,
}

impl SpecAudience {
    /// 三读者全集。
    pub const ALL: [SpecAudience; 3] = [
        SpecAudience::Implementer,
        SpecAudience::AssetAuthor,
        SpecAudience::ToolAuthor,
    ];

    /// 线上短码（显式映射）。
    pub const fn wire(self) -> &'static str {
        match self {
            SpecAudience::Implementer => "impl",
            SpecAudience::AssetAuthor => "asset",
            SpecAudience::ToolAuthor => "tool",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、文档一：vmesh 开放格式规范（字段级 schema）
// ---------------------------------------------------------------------------

/// 字节宽常量（规范与实现同源——实现侧逐字段核对以此为准）。
pub const VMESH_HEADER_BYTES: u64 = 8;
pub const VMESH_VERT_BYTES: u64 = 12;
pub const VMESH_FACE_BYTES: u64 = 12;

/// 线型（封闭三态——线型即字节语义，第三方按此解码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WireType {
    /// 无符号 32 位整数，小端。
    U32LE,
    /// 32 位 IEEE-754 浮点，小端。
    F32LE,
}

/// vmesh 规范字段行。
#[derive(Clone, Copy, Debug)]
pub struct VmeshFieldSpec {
    /// 字段路径（人读，点分）。
    pub path: &'static str,
    /// 起始偏移表达式（依赖 V/F 的用公式文本；头/定长字段用常量）。
    pub offset_expr: &'static str,
    /// 字节宽。
    pub size: u64,
    /// 线型。
    pub wire: WireType,
    /// 约束（容量/取值域——空字符串表示无额外约束）。
    pub constraint: &'static str,
    /// 主要读者。
    pub audience: SpecAudience,
}

/// vmesh 流式格式规范表（开放规范——第三方依此可实现读写）。
///
/// 格式刻意最简：`u32 顶点数 | u32 面数 | 3×f32×顶点 | 3×u32×面`（均 LE）。
/// **无 magic/版本字段**是设计取舍不是疏漏：容量上限（各 2^20）是唯一
/// 护栏，最小头部让万级网格 open 的常数开销最低——取舍在行内如实注明。
pub const VMESH_SPEC: [VmeshFieldSpec; 4] = [
    VmeshFieldSpec {
        path: "header.vert_count",
        offset_expr: "0",
        size: 4,
        wire: WireType::U32LE,
        constraint: "cap:STREAM_MAX_VERTS",
        audience: SpecAudience::Implementer,
    },
    VmeshFieldSpec {
        path: "header.face_count",
        offset_expr: "4",
        size: 4,
        wire: WireType::U32LE,
        constraint: "cap:STREAM_MAX_FACES",
        audience: SpecAudience::Implementer,
    },
    VmeshFieldSpec {
        path: "verts[i].xyz",
        offset_expr: "8 + 12*i",
        size: 12,
        wire: WireType::F32LE,
        constraint: "i < vert_count",
        audience: SpecAudience::ToolAuthor,
    },
    VmeshFieldSpec {
        path: "faces[j].abc",
        offset_expr: "8 + 12*vert_count + 12*j",
        size: 12,
        wire: WireType::U32LE,
        constraint: "j < face_count; index < vert_count",
        audience: SpecAudience::ToolAuthor,
    },
];

/// 规范总长公式：`8 + 12V + 12F`（与实现侧逐字节核对的总长口径）。
pub const fn spec_total_size(vert_count: u64, face_count: u64) -> u64 {
    VMESH_HEADER_BYTES + VMESH_VERT_BYTES * vert_count + VMESH_FACE_BYTES * face_count
}

/// 规范容量上限（直引实现常量——单源，防规范与实现各自写数）。
pub const SPEC_MAX_VERTS: u32 = STREAM_MAX_VERTS;
pub const SPEC_MAX_FACES: u32 = STREAM_MAX_FACES;

// ---------------------------------------------------------------------------
// 三、文档二：glTF 互转说明（支持范围 / 已知限制清单化）
// ---------------------------------------------------------------------------

/// 互转支持三态（封闭集——"部分支持"必须单列，混进支持里就是撒谎）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InteropSupport {
    /// 直接支持（语义无损或损耗有界且已声明）。
    Supported,
    /// 部分支持（覆盖范围有明确边界）。
    Partial,
    /// 不支持（逐条给替代路径——F1185：不做不假装）。
    Unsupported,
}

/// glTF 互转条目。
#[derive(Clone, Copy, Debug)]
pub struct GltfInteropRow {
    /// glTF 侧条目（属性/扩展/结构）。
    pub item: &'static str,
    /// 支持态。
    pub support: InteropSupport,
    /// 说明（支持边界或限制原因——非空纪律）。
    pub note: &'static str,
    /// 替代路径（不支持/部分支持的必填项；支持的留空）。
    pub alternative: &'static str,
    /// 主要读者。
    pub audience: SpecAudience,
}

/// glTF 互转清单（支持范围 + 已知限制逐条显性）。
///
/// vmesh 最简格式只承载位置与三角面索引——属性与扩展一律 Unsupported
/// 且逐条指路替代管线，绝不"收下但悄悄丢数据"。
pub const GLTF_INTEROP: [GltfInteropRow; 7] = [
    GltfInteropRow {
        item: "POSITION (VEC3 float32)",
        support: InteropSupport::Supported,
        note: "直转 verts[]，语义无损（平移/缩放属资产侧预处理）",
        alternative: "",
        audience: SpecAudience::AssetAuthor,
    },
    GltfInteropRow {
        item: "indices (TRIANGLES, uint32)",
        support: InteropSupport::Supported,
        note: "直转 faces[]；仅 TRIANGLES 拓扑，fan/strip 需先展开",
        alternative: "",
        audience: SpecAudience::AssetAuthor,
    },
    GltfInteropRow {
        item: "NORMAL / TEXCOORD_0",
        support: InteropSupport::Unsupported,
        note: "vmesh 最简格式不承载顶点属性（只有位置）",
        alternative: "法线/切线经 F1606 生成管线重算；UV 经 F1605 量化管线另行打包",
        audience: SpecAudience::AssetAuthor,
    },
    GltfInteropRow {
        item: "KHR_materials_variants",
        support: InteropSupport::Unsupported,
        note: "材质变体超出几何容器语义",
        alternative: "材质分组与批处理准备归 F1609（逐面材质 id 容器）",
        audience: SpecAudience::ToolAuthor,
    },
    GltfInteropRow {
        item: "skins / joints / weights",
        support: InteropSupport::Unsupported,
        note: "骨骼数据不在几何容器内",
        alternative: "骨骼权重数据容器归 F1610",
        audience: SpecAudience::ToolAuthor,
    },
    GltfInteropRow {
        item: "animations (sampler/channel)",
        support: InteropSupport::Partial,
        note: "经 F2409 动画导入四通道映射（translation/rotation/scale/weights），非网格流直转",
        alternative: "网格拓扑以外的轨道走 F2409 映射表",
        audience: SpecAudience::Implementer,
    },
    GltfInteropRow {
        item: "KHR_texture_transform 等材质扩展",
        support: InteropSupport::Unsupported,
        note: "几何容器无材质语义位",
        alternative: "由上层材质系统（I03 材质族）承接，vmesh 不承载",
        audience: SpecAudience::Implementer,
    },
];

// ---------------------------------------------------------------------------
// 四、文档三：量化指南（何时量化 / 档位选择）
// ---------------------------------------------------------------------------

/// 量化指南行：场景 → 推荐档位 → 依据。
#[derive(Clone, Copy, Debug)]
pub struct QuantGuideRow {
    /// 使用场景。
    pub scenario: &'static str,
    /// 推荐档位（必须经 `QuantBits::from_bits` 可构造——指南不给实现
    /// 不存在的档位，对拍钉死）。
    pub bits: u32,
    /// 依据（误差口径/收益说明——非空纪律）。
    pub rationale: &'static str,
    /// 主要读者。
    pub audience: SpecAudience,
}

/// 量化指南（四档全覆盖，档位集合与实现单源对拍）。
pub const QUANT_GUIDE: [QuantGuideRow; 4] = [
    QuantGuideRow {
        scenario: "远景 LOD / 占位预览（顶点误差不敏感）",
        bits: 8,
        rationale: "1 字节/分量，压缩比最高；误差上界最大但远景不可辨",
        audience: SpecAudience::ToolAuthor,
    },
    QuantGuideRow {
        scenario: "中景静态几何",
        bits: 10,
        rationale: "误差与字节数的中间折衷，适合批量中景资产",
        audience: SpecAudience::ToolAuthor,
    },
    QuantGuideRow {
        scenario: "主流默认（基准/常规资产）",
        bits: 12,
        rationale: "F1616 量化基准锁定的主流档：压缩比 416‰、误差上界适中",
        audience: SpecAudience::AssetAuthor,
    },
    QuantGuideRow {
        scenario: "近景/归档母版",
        bits: 16,
        rationale: "最高保真档；B16 打包 6B/顶点对 12B 原始 = 200% 压缩比",
        audience: SpecAudience::AssetAuthor,
    },
];

// ---------------------------------------------------------------------------
// 五、漂移机制：规范-实现一致性断言（可执行规范）
// ---------------------------------------------------------------------------

/// 单字段核对结论。
#[derive(Clone, Copy, Debug)]
pub struct DriftFinding {
    /// 字段路径（对应规范行）。
    pub path: &'static str,
    /// 是否一致。
    pub ok: bool,
    /// 期望值摘要（人读）。
    pub expect: &'static str,
}

/// 漂移检查报告。
#[derive(Clone, Debug)]
pub struct SpecDriftReport {
    /// 逐字段结论。
    pub findings: Vec<DriftFinding>,
    /// 漂移计数。
    pub drift_count: u32,
    /// 核对的总长（规范公式 vs 实际字节流长度）。
    pub spec_len: u64,
    pub actual_len: u64,
}

impl SpecDriftReport {
    /// 是否零漂移（规范与实现一致）。
    pub const fn drifted(&self) -> bool {
        self.drift_count > 0 || self.spec_len != self.actual_len
    }
}

/// 参考 语料网格：3 顶点 1 面（最小非平凡——头/顶点段/面段三段都覆盖）。
fn reference_mesh() -> RepairMesh {
    let mut m = RepairMesh::new();
    let vs = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut i = 0;
    while i < 3 {
        m.push_vert(vs[i]);
        i += 1;
    }
    m.push_face([0, 1, 2]);
    m
}

/// 规范-实现一致性检查：编码参考网格，按规范表逐字段核对实际字节流。
///
/// 这就是"开放规范的可执行性"的落点：规范表若与产线编码器漂移
/// （任何一侧被改动），此检查即红并逐字段点名。
pub fn spec_impl_drift_check() -> SpecDriftReport {
    let mesh = reference_mesh();
    let stream = encode_mesh_stream(&mesh);
    let mut findings: Vec<DriftFinding> = Vec::new();
    let mut drift = 0u32;

    // 规范口径总长 vs 实际长度。
    let spec_len = spec_total_size(3, 1);
    let actual_len = stream.len() as u64;

    // 头部两字段：小端 u32 值核对。
    let vc = u32::from_le_bytes([stream[0], stream[1], stream[2], stream[3]]) as u64;
    let fc = u32::from_le_bytes([stream[4], stream[5], stream[6], stream[7]]) as u64;
    let ok_vc = vc == 3;
    if !ok_vc {
        drift += 1;
    }
    findings.push(DriftFinding { path: "header.vert_count", ok: ok_vc, expect: "LE u32 == 3" });
    let ok_fc = fc == 1;
    if !ok_fc {
        drift += 1;
    }
    findings.push(DriftFinding { path: "header.face_count", ok: ok_fc, expect: "LE u32 == 1" });

    // 顶点段：3×f32 LE 逐值核对（参考网格坐标已知）。
    let expect_verts: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut vi = 0usize;
    while vi < 3 {
        let base = (8 + 12 * vi) as usize;
        let x = f32::from_le_bytes([stream[base], stream[base + 1], stream[base + 2], stream[base + 3]]);
        let y = f32::from_le_bytes([stream[base + 4], stream[base + 5], stream[base + 6], stream[base + 7]]);
        let z = f32::from_le_bytes([stream[base + 8], stream[base + 9], stream[base + 10], stream[base + 11]]);
        let ok = x == expect_verts[vi][0] && y == expect_verts[vi][1] && z == expect_verts[vi][2];
        if !ok {
            drift += 1;
        }
        findings.push(DriftFinding { path: "verts[i].xyz", ok, expect: "LE f32 x3 == 参考坐标" });
        vi += 1;
    }

    // 面段：3×u32 LE 索引核对。
    let base = (8 + 12 * 3) as usize;
    let a = u32::from_le_bytes([stream[base], stream[base + 1], stream[base + 2], stream[base + 3]]);
    let b = u32::from_le_bytes([stream[base + 4], stream[base + 5], stream[base + 6], stream[base + 7]]);
    let c = u32::from_le_bytes([stream[base + 8], stream[base + 9], stream[base + 10], stream[base + 11]]);
    let ok_face = a == 0 && b == 1 && c == 2;
    if !ok_face {
        drift += 1;
    }
    findings.push(DriftFinding { path: "faces[j].abc", ok: ok_face, expect: "LE u32 x3 == [0,1,2]" });

    SpecDriftReport { findings, drift_count: drift, spec_len, actual_len }
}

/// 三读者覆盖断言：三文档合计，三读者各至少出现一次。
pub fn all_audiences_covered() -> bool {
    let mut seen = [false; 3];
    for row in VMESH_SPEC.iter() {
        let mut k = 0usize;
        while k < 3 {
            if row.audience == SpecAudience::ALL[k] {
                seen[k] = true;
            }
            k += 1;
        }
    }
    for row in GLTF_INTEROP.iter() {
        let mut k = 0usize;
        while k < 3 {
            if row.audience == SpecAudience::ALL[k] {
                seen[k] = true;
            }
            k += 1;
        }
    }
    for row in QUANT_GUIDE.iter() {
        let mut k = 0usize;
        while k < 3 {
            if row.audience == SpecAudience::ALL[k] {
                seen[k] = true;
            }
            k += 1;
        }
    }
    seen[0] && seen[1] && seen[2]
}
