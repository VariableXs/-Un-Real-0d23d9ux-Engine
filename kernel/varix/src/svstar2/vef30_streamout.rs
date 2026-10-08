//! VE-F1627 · 顶点流输出预留（transform feedback / GS 流输出接口）
//!
//! GPU 写回顶点数据——GPU 粒子（GPU 上模拟+渲染）与程序化生成的前置；
//! 对接 L 域粒子系统预留（跨域预留声明，深度实现随 L 域推进）。
//!
//! ## 要点一：流输出接口（GPU 写回）
//! transform feedback = GPU 把顶点着色/GS 处理后的顶点数据写进绑定缓冲，
//! CPU 零回读。接口三件：目标（`SoTarget`：缓冲/偏移/步幅/容量）、
//! 变量布局（`SoLayout`：语义槽+分量数）、管线绑定（`SoPipeline`：
//! 阶段二态 + 布局 + 目标组 + 重启语义）。目标偏移 4 字节对齐、步幅
//! 4 的倍数、目标区间互不重叠、目标数上限 [`MAX_SO_TARGETS`]——校验在
//! 绑定期一次完成，热路径零校验。
//!
//! ## 要点二：能力探测与表驱动降级（F1208 纪律范式）
//! [`SO_CAPS`] 静态能力矩阵：D3D 原生流输出、Vulkan 原生 transform
//! feedback 扩展、Metal 无原生（降级路径=顶点着色器写存储缓冲，
//! 表内 `alternate_path` 显性）——探测→能力表→降级→诚实标注。
//!
//! ## 要点三：对接预留（跨域声明）
//! L 域粒子系统的 GPU 路径消费流输出：[`particle_bridge_plan`] 产出一期
//! 绑定计划（位置+速度双变量布局）——只产计划不执行（深度实现随 L 域
//! 推进）；程序化生成消费路径同声明。
//!
//! ## 要点四：诚实标注与矩阵同源（F1625 纪律承接）
//! [`HONESTY_NOTICES`] 每后端一条 + 全局一条；[`verify_notes`] 逐条与
//! 能力矩阵对拍：非原生后端的标注必含降级声明，全局标注必含一期/L 域
//! 口径——标注与矩阵矛盾即 `ANNOTATION_MISMATCH` 拒（「矩阵驱动而非
//! 假设」机制化）。
//!
//! ## 要点五：流输出确定性
//! 相同输入同输出（确定性纪律延伸）：[`reference_emit`] 参考发射器按
//! 布局槽序逐顶点写出字节——固定顶点序、目标绑定序即写序、无原子竞争
//! 输出序（[`DETERMINISM_CONTRACT`] 字面量冻结）。全整数运算（i32 小端
//! 字节序），零浮点。
//!
//! ## 要点六：诊断码独占 0x45xx 段
//! 八码：目标错齐/形状非法/区间冲突/槽重复/分量越界/后端不支持/
//! 标注失配/确定性破坏。
//!
//! 判据（17 项）映射见 `vef30_streamout_checks.rs` 头注。

use alloc::string::String;
use alloc::vec::Vec;

/// F1627 版本溯源键。
pub const STREAMOUT_VERSION: &str = "F30-streamout-v1";

// ---------------------------------------------------------------------------
// 一、流输出接口（GPU 写回）
// ---------------------------------------------------------------------------

/// 流输出阶段二态封闭：顶点级 transform feedback / GS 流输出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoStage {
    /// 顶点阶段 transform feedback（GS 缺省时的写回路径）。
    VertexFeedback,
    /// 几何着色器流输出（多目标多份输出）。
    GeometryStream,
}

impl SoStage {
    pub fn say(self) -> &'static str {
        match self {
            SoStage::VertexFeedback => "顶点级 transform feedback 写回",
            SoStage::GeometryStream => "GS 流输出写回",
        }
    }
}

/// 流输出目标：一块被 GPU 写回的缓冲绑定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SoTarget {
    /// 目标缓冲标识。
    pub buffer_id: u32,
    /// 写入起始字节偏移（须 4 字节对齐）。
    pub offset_bytes: u32,
    /// 单顶点步幅字节（须 4 的倍数）。
    pub byte_stride: u32,
    /// 本目标可写顶点容量（≥1）。
    pub max_vertices: u32,
}

/// 目标字节数区间（checked 运算，溢出返回 None）。
fn target_span(t: &SoTarget) -> Option<u64> {
    if t.max_vertices == 0 {
        return None;
    }
    let stride = t.byte_stride as u64;
    let n = t.max_vertices as u64;
    let span = stride.checked_mul(n)?;
    let end = (t.offset_bytes as u64).checked_add(span)?;
    Some(end)
}

/// 单目标校验：对齐/步幅/容量。
pub fn validate_target(t: &SoTarget) -> Result<(), SoCode> {
    if t.offset_bytes % 4 != 0 {
        return Err(SoCode::TARGET_MISALIGNED);
    }
    if t.byte_stride == 0 || t.byte_stride % 4 != 0 || t.max_vertices == 0 {
        return Err(SoCode::TARGET_SHAPE_INVALID);
    }
    if target_span(t).is_none() {
        return Err(SoCode::TARGET_SHAPE_INVALID);
    }
    Ok(())
}

/// 流输出变量：语义槽 + 分量数（1..=4）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SoVar {
    pub name: &'static str,
    pub slot: u8,
    pub components: u8,
}

/// 流输出变量布局：槽序即写出序。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SoLayout {
    pub vars: Vec<SoVar>,
}

impl SoLayout {
    /// 步幅字节 = Σ 分量×4。
    pub fn stride_bytes(&self) -> u32 {
        let mut sum: u32 = 0;
        for v in &self.vars {
            sum += v.components as u32 * 4;
        }
        sum
    }

    /// 布局校验：分量域 1..=4、槽唯一、名唯一。
    pub fn validate(&self) -> Result<(), SoCode> {
        for v in &self.vars {
            if v.components == 0 || v.components > 4 {
                return Err(SoCode::COMPONENT_RANGE);
            }
        }
        for i in 0..self.vars.len() {
            for j in (i + 1)..self.vars.len() {
                let a = match self.vars.get(i) {
                    Some(x) => x,
                    None => continue,
                };
                let b = match self.vars.get(j) {
                    Some(x) => x,
                    None => continue,
                };
                if a.slot == b.slot {
                    return Err(SoCode::VAR_SLOT_DUP);
                }
                if a.name == b.name {
                    return Err(SoCode::VAR_SLOT_DUP);
                }
            }
        }
        Ok(())
    }
}

/// 每管线流输出目标数上限（业界通行 4）。
pub const MAX_SO_TARGETS: usize = 4;

/// 流输出管线绑定：阶段 + 布局 + 目标组 + 重启语义。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SoPipeline {
    pub stage: SoStage,
    pub layout: SoLayout,
    pub targets: Vec<SoTarget>,
    /// 顶点流重启语义（条带分割）是否开启。
    pub restart_enabled: bool,
}

impl SoPipeline {
    /// 绑定期一次校验（热路径零校验纪律）：
    /// 布局合法、目标 1..=MAX、单目标合法、目标区间互不重叠。
    pub fn validate(&self) -> Result<(), SoCode> {
        self.layout.validate()?;
        if self.targets.is_empty() || self.targets.len() > MAX_SO_TARGETS {
            return Err(SoCode::TARGET_OVERFLOW);
        }
        for t in &self.targets {
            validate_target(t)?;
        }
        // 两两区间重叠检测（checked 运算，判据区零 panic 面）
        for i in 0..self.targets.len() {
            for j in (i + 1)..self.targets.len() {
                let a = match self.targets.get(i) {
                    Some(x) => x,
                    None => continue,
                };
                let b = match self.targets.get(j) {
                    Some(x) => x,
                    None => continue,
                };
                let a_end = match target_span(a) {
                    Some(e) => e,
                    None => return Err(SoCode::TARGET_SHAPE_INVALID),
                };
                let b_end = match target_span(b) {
                    Some(e) => e,
                    None => return Err(SoCode::TARGET_SHAPE_INVALID),
                };
                let a_off = a.offset_bytes as u64;
                let b_off = b.offset_bytes as u64;
                let overlapping = if a_off <= b_off {
                    b_off < a_end
                } else {
                    a_off < b_end
                };
                if overlapping {
                    return Err(SoCode::TARGET_OVERFLOW);
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、能力探测与表驱动降级
// ---------------------------------------------------------------------------

/// 后端三态封闭。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoBackend {
    D3D,
    Vulkan,
    Metal,
}

impl SoBackend {
    pub fn say(self) -> &'static str {
        match self {
            SoBackend::D3D => "D3D",
            SoBackend::Vulkan => "Vulkan",
            SoBackend::Metal => "Metal",
        }
    }
}

/// 后端流输出能力。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SoCaps {
    pub backend: SoBackend,
    /// 原生流输出支持。
    pub native: bool,
    /// 原生路径目标数上限。
    pub max_targets: u32,
    /// 降级替代路径（原生支持时为 None）。
    pub alternate_path: Option<&'static str>,
}

/// 静态能力矩阵（探测→能力表→降级→诚实标注）。
pub const SO_CAPS: [SoCaps; 3] = [
    SoCaps {
        backend: SoBackend::D3D,
        native: true,
        max_targets: 4,
        alternate_path: None,
    },
    SoCaps {
        backend: SoBackend::Vulkan,
        native: true,
        max_targets: 4,
        alternate_path: None,
    },
    SoCaps {
        backend: SoBackend::Metal,
        native: false,
        max_targets: 0,
        alternate_path: Some("顶点着色器写存储缓冲替代路径"),
    },
];

/// 能力查询：域外返回 None。
pub fn caps_for(b: SoBackend) -> Option<&'static SoCaps> {
    for c in SO_CAPS.iter() {
        if c.backend == b {
            return Some(c);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 三、对接预留（跨域声明，深度实现随 L 域推进）
// ---------------------------------------------------------------------------

/// L 域粒子系统对接预留声明（字面量冻结，判据对拍）。
pub const L_PARTICLE_RESERVATION: &str =
    "L 域粒子系统 GPU 路径经 ParticleBridge 消费流输出目标——跨域预留声明（深度实现随 L 域推进）";

/// 程序化生成消费路径声明。
pub const PROCEDURAL_NOTICE: &str =
    "程序化生成（植被/碎裂/轨迹重放）为流输出第二消费面——一期接口预留";

/// 一期粒子桥计划：位置+速度双变量布局，只产计划不执行（诚实预留）。
pub fn particle_bridge_plan() -> Option<(SoLayout, SoStage)> {
    let layout = SoLayout {
        vars: alloc::vec![
            SoVar {
                name: "particle_pos",
                slot: 0,
                components: 4,
            },
            SoVar {
                name: "particle_vel",
                slot: 1,
                components: 3,
            },
        ],
    };
    if layout.validate().is_err() {
        return None;
    }
    Some((layout, SoStage::VertexFeedback))
}

// ---------------------------------------------------------------------------
// 四、诚实标注（与能力矩阵同源，F1625 纪律承接）
// ---------------------------------------------------------------------------

/// 诚实标注：后端为 None 表示全局口径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HonestyNote {
    pub backend: Option<SoBackend>,
    pub phase: &'static str,
    pub text: &'static str,
}

/// 诚实标注全集：每后端一条 + 全局一条（条数同源判据）。
pub fn honesty_notices() -> Vec<HonestyNote> {
    alloc::vec![
        HonestyNote {
            backend: Some(SoBackend::D3D),
            phase: "一期",
            text: "D3D 原生流输出：一期接口+能力探测，深度实现随 L 域推进",
        },
        HonestyNote {
            backend: Some(SoBackend::Vulkan),
            phase: "一期",
            text: "Vulkan transform feedback 扩展：一期接口+能力探测，深度实现随 L 域推进",
        },
        HonestyNote {
            backend: Some(SoBackend::Metal),
            phase: "一期",
            text: "Metal 无原生流输出——顶点着色器写存储缓冲降级替代路径，能力探测在案",
        },
        HonestyNote {
            backend: None,
            phase: "全局",
            text: "一期接口+能力探测，深度实现随 L 域推进；确定性契约见 DETERMINISM_CONTRACT",
        },
    ]
}

/// 标注与能力矩阵对拍（「矩阵驱动而非假设」机制化）：
/// 每后端标注须对应矩阵在案行；非原生后端标注必含「降级」；
/// 全局标注必含一期与 L 域口径。矛盾即 `ANNOTATION_MISMATCH`。
pub fn verify_notes(notes: &[HonestyNote]) -> Result<(), SoCode> {
    for n in notes {
        match n.backend {
            Some(b) => {
                let caps = match caps_for(b) {
                    Some(c) => c,
                    None => return Err(SoCode::ANNOTATION_MISMATCH),
                };
                if !caps.native && !n.text.contains("降级") {
                    return Err(SoCode::ANNOTATION_MISMATCH);
                }
                if caps.native && caps.alternate_path.is_some() {
                    return Err(SoCode::ANNOTATION_MISMATCH);
                }
            }
            None => {
                if !n.text.contains("一期") || !n.text.contains("L 域") {
                    return Err(SoCode::ANNOTATION_MISMATCH);
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、流输出确定性（相同输入同输出）
// ---------------------------------------------------------------------------

/// 确定性契约声明（字面量冻结，判据对拍）。
pub const DETERMINISM_CONTRACT: &str =
    "流输出确定性：相同输入同输出——固定顶点序、目标绑定序即写序、无原子竞争输出序";

/// 参考发射器：按布局槽序逐顶点写出字节（i32 小端，全整数零浮点）。
/// 顶点提供 4 分量，变量声明用前 `components` 个，其余忽略。
pub fn reference_emit(layout: &SoLayout, vertices: &[[i32; 4]], out: &mut Vec<u8>) {
    out.clear();
    let ordered: Vec<&SoVar> = {
        let mut v: Vec<&SoVar> = layout.vars.iter().collect();
        v.sort_by_key(|x| x.slot);
        v
    };
    for vert in vertices.iter() {
        for var in ordered.iter() {
            let comp = var.components as usize;
            for k in 0..comp {
                let val = if k < 4 { vert[k] } else { 0 };
                out.push((val & 0xFF) as u8);
                out.push(((val >> 8) & 0xFF) as u8);
                out.push(((val >> 16) & 0xFF) as u8);
                out.push(((val >> 24) & 0xFF) as u8);
            }
        }
    }
}

/// 确定性对拍：两次发射逐字节一致。
pub fn emit_twice_equal(layout: &SoLayout, vertices: &[[i32; 4]]) -> bool {
    let mut a: Vec<u8> = Vec::new();
    let mut b: Vec<u8> = Vec::new();
    reference_emit(layout, vertices, &mut a);
    reference_emit(layout, vertices, &mut b);
    a == b
}

// ---------------------------------------------------------------------------
// 六、诊断码（独占 0x45xx 段）
// ---------------------------------------------------------------------------

/// vef30 诊断码。独占 `0x45xx` 段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SoCode(pub u16);

impl SoCode {
    pub const TARGET_MISALIGNED: SoCode = SoCode(0x4501);
    pub const TARGET_SHAPE_INVALID: SoCode = SoCode(0x4502);
    pub const TARGET_OVERFLOW: SoCode = SoCode(0x4503);
    pub const VAR_SLOT_DUP: SoCode = SoCode(0x4504);
    pub const COMPONENT_RANGE: SoCode = SoCode(0x4505);
    pub const NOT_SUPPORTED: SoCode = SoCode(0x4506);
    pub const ANNOTATION_MISMATCH: SoCode = SoCode(0x4507);
    pub const DETERMINISM_BROKEN: SoCode = SoCode(0x4508);

    /// 全部在案码（判据条数与互异对账用）。
    pub fn all() -> [SoCode; 8] {
        [
            SoCode::TARGET_MISALIGNED,
            SoCode::TARGET_SHAPE_INVALID,
            SoCode::TARGET_OVERFLOW,
            SoCode::VAR_SLOT_DUP,
            SoCode::COMPONENT_RANGE,
            SoCode::NOT_SUPPORTED,
            SoCode::ANNOTATION_MISMATCH,
            SoCode::DETERMINISM_BROKEN,
        ]
    }
}

impl SoCode {
    pub fn say(self) -> String {
        let name = match self {
            SoCode::TARGET_MISALIGNED => "目标偏移未 4 字节对齐",
            SoCode::TARGET_SHAPE_INVALID => "目标形状非法（步幅/容量）",
            SoCode::TARGET_OVERFLOW => "目标区间冲突或超上限",
            SoCode::VAR_SLOT_DUP => "布局槽或名称重复",
            SoCode::COMPONENT_RANGE => "分量数越域（须 1..=4）",
            SoCode::NOT_SUPPORTED => "后端不支持原生流输出",
            SoCode::ANNOTATION_MISMATCH => "诚实标注与能力矩阵矛盾",
            SoCode::DETERMINISM_BROKEN => "流输出确定性破坏",
            SoCode(_) => "vef30 未在案码",
        };
        alloc::format!("0x{:04X} {}", self.0, name)
    }
}

/// 后端接入体：三事实（能力在案/管线受理/声明非空）。
#[derive(Clone, Debug)]
pub struct SoBackendFace {
    pub backend: SoBackend,
    /// 该后端一条示例管线是否可受理（Metal 降级路径不受理原生绑定）。
    pub native_bind_accepted: bool,
    pub declare_line: String,
}

/// 三后端接入体（探测→受理→声明三件齐备）。
pub fn backend_faces() -> Vec<SoBackendFace> {
    let mut faces: Vec<SoBackendFace> = Vec::new();
    for c in SO_CAPS.iter() {
        let accepted = c.native;
        let line = alloc::format!(
            "后端 {} 流输出声明：原生={} 目标上限={} 替代路径={}",
            c.backend.say(),
            c.native,
            c.max_targets,
            c.alternate_path.unwrap_or("无（原生路径）")
        );
        faces.push(SoBackendFace {
            backend: c.backend,
            native_bind_accepted: accepted,
            declare_line: line,
        });
    }
    faces
}
