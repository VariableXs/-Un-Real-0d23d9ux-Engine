//! VE-F1628 · 间接绘制预留（indirect draw 接口——GPU 驱动的绘制参数）
//!
//! 绘制参数存 GPU 缓冲、CPU 不回读——剔除/LOD 全 GPU 化（I09/I10 剔除组）
//! 的前置；对接 I 域剔除组预留。命令生成本体归 VE-F0029（VE-A 域），
//! 本域管**接口契约预留**：参数块布局、绑定期校验、能力矩阵、诚实标注。
//!
//! ## 要点一：接口契约（参数块 16 字节冻结）
//! [`IndirectArgs`] 四 u32（数量/实例/起始/实例基址）＝参数块
//! [`INDIRECT_ARGS_STRIDE`]=16 字节；命令（[`IndirectCommand`]）指向
//! 参数缓冲内偏移，偏移须 16 字节对齐；参数块区间互不重叠；批命令数
//! 上限 [`MAX_INDIRECT_PER_BATCH`]。校验在绑定期一次完成（热路径零校验
//! 纪律）；空绘制（数量 0）合法——被剔除的 draw 就是空绘制，这不是错误。
//!
//! ## 要点二：能力探测与表驱动降级（F1208 范式）
//! [`ID_CAPS`] 静态矩阵：D3D/Vulkan 原生间接绘制且索引变体原生；
//! Metal 原生非索引间接、**索引变体回退 CPU 组装**（域内冻结语料，
//! 标注显性）；命令数上限三后端各异——探测→能力表→降级→诚实标注。
//!
//! ## 要点三：对接预留（跨域声明）
//! I 域剔除组（I09/I10）的剔除/LOD 决策生产 indirect 参数缓冲——
//! 剔除决策在 GPU、零 CPU 回读（[`ZERO_CPU_READBACK`]）；一期只产桥接
//! 计划（[`cull_bridge_plan`]）不执行（深度实现随剔除组推进）。
//!
//! ## 要点四：诚实标注与矩阵同源（vef30 范式承接）
//! [`honesty_notices`] 每后端一条 + 全局一条；非原生 indexed 后端的
//! 标注必含「回退」，全局标注必含「一期」与「剔除组」口径——
//! [`verify_notes`] 矛盾即 `ANNOTATION_MISMATCH` 拒。
//!
//! ## 要点五：诊断码独占 0x4Dxx 段
//! 八码：偏移错齐/参数非法/批超限/参数块重叠/不支持/标注失配/
//! 桥计划非法/范围溢出。范围检查走 checked 运算（u32 溢出即拒）。
//!
//! 判据（21 项）映射见 `vef31_indirect_checks.rs` 头注。

use alloc::string::String;
use alloc::vec::Vec;

/// F1628 版本溯源键。
pub const INDIRECT_VERSION: &str = "F31-indirect-v1";

// ---------------------------------------------------------------------------
// 一、接口契约（GPU 驱动的绘制参数）
// ---------------------------------------------------------------------------

/// 间接绘制二态封闭：非索引 / 索引变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrawType {
    IndirectDraw,
    IndirectDrawIndexed,
}

impl DrawType {
    pub fn say(self) -> &'static str {
        match self {
            DrawType::IndirectDraw => "非索引间接绘制",
            DrawType::IndirectDrawIndexed => "索引间接绘制",
        }
    }
}

/// 间接绘制参数块步幅（四 u32，域内冻结）。
pub const INDIRECT_ARGS_STRIDE: u32 = 16;

/// 每批间接命令数上限（域内冻结，判据写死对拍）。
pub const MAX_INDIRECT_PER_BATCH: usize = 1024;

/// 间接绘制参数块（布局即 GPU 缓冲布局，小端）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IndirectArgs {
    /// 顶点数（非索引）或索引数（索引变体）；0=空绘制（被剔除，合法）。
    pub count: u32,
    /// 实例数；须 ≥1（0 实例非绘制语义）。
    pub instance_count: u32,
    /// 起始顶点/起始索引。
    pub first: u32,
    /// 实例基址（消减重复绑定用的实例偏移）。
    pub first_instance: u32,
}

impl IndirectArgs {
    /// 参数域校验：实例 ≥1；first+count 须不溢出 u32。
    pub fn validate(&self) -> Result<(), IdCode> {
        if self.instance_count == 0 {
            return Err(IdCode::ARGS_INVALID);
        }
        let sum = (self.first as u64).checked_add(self.count as u64);
        match sum {
            Some(s) if s <= u32::MAX as u64 => Ok(()),
            _ => Err(IdCode::RANGE_INVALID),
        }
    }

    /// 序列化为 16 字节参数块（小端，确定性布局）。
    pub fn to_bytes(self) -> [u8; 16] {
        let words: [u32; 4] = [
            self.count,
            self.instance_count,
            self.first,
            self.first_instance,
        ];
        let mut out = [0u8; 16];
        let mut k = 0usize;
        while k < 4 {
            let v = words[k];
            let base = k * 4;
            out[base] = (v & 0xFF) as u8;
            out[base + 1] = ((v >> 8) & 0xFF) as u8;
            out[base + 2] = ((v >> 16) & 0xFF) as u8;
            out[base + 3] = ((v >> 24) & 0xFF) as u8;
            k += 1;
        }
        out
    }
}

/// 一条间接绘制命令：指向参数缓冲内的偏移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IndirectCommand {
    pub draw_type: DrawType,
    /// 参数缓冲标识。
    pub buffer_id: u32,
    /// 参数块字节偏移（须 16 字节对齐）。
    pub offset_bytes: u32,
}

/// 单命令校验：对齐 + 参数域。
pub fn validate_command(cmd: &IndirectCommand, args: &IndirectArgs) -> Result<(), IdCode> {
    if cmd.offset_bytes % INDIRECT_ARGS_STRIDE != 0 {
        return Err(IdCode::OFFSET_MISALIGNED);
    }
    args.validate()?;
    match cmd.draw_type {
        DrawType::IndirectDraw | DrawType::IndirectDrawIndexed => Ok(()),
    }
}

/// 间接绘制批：同缓冲多命令（剔除组一次产出一批可见 draw）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IndirectBatch {
    pub buffer_id: u32,
    /// 命令序列（绑定序即执行序）。
    pub commands: Vec<IndirectCommand>,
}

impl IndirectBatch {
    /// 批校验：非空且 ≤ 上限、偏移对齐、参数块区间互不重叠。
    pub fn validate(&self, args_of: &dyn Fn(&IndirectCommand) -> Option<IndirectArgs>) -> Result<(), IdCode> {
        if self.commands.is_empty() || self.commands.len() > MAX_INDIRECT_PER_BATCH {
            return Err(IdCode::BATCH_OVERFLOW);
        }
        for i in 0..self.commands.len() {
            let c = match self.commands.get(i) {
                Some(c) => c,
                None => return Err(IdCode::BATCH_OVERFLOW),
            };
            if c.buffer_id != self.buffer_id {
                return Err(IdCode::ARGS_INVALID);
            }
            if c.offset_bytes % INDIRECT_ARGS_STRIDE != 0 {
                return Err(IdCode::OFFSET_MISALIGNED);
            }
            let a = match args_of(c) {
                Some(a) => a,
                None => return Err(IdCode::ARGS_INVALID),
            };
            a.validate()?;
            // 参数块区间 [offset, offset+16) 两两不重叠
            for j in (i + 1)..self.commands.len() {
                let d = match self.commands.get(j) {
                    Some(d) => d,
                    None => continue,
                };
                if d.offset_bytes == c.offset_bytes {
                    return Err(IdCode::ARGS_OVERLAP);
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
pub enum IdBackend {
    D3D,
    Vulkan,
    Metal,
}

impl IdBackend {
    pub fn say(self) -> &'static str {
        match self {
            IdBackend::D3D => "D3D",
            IdBackend::Vulkan => "Vulkan",
            IdBackend::Metal => "Metal",
        }
    }
}

/// 后端间接绘制能力。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdCaps {
    pub backend: IdBackend,
    /// 原生间接绘制（非索引）。
    pub native_draw: bool,
    /// 索引变体是否原生（Metal 回退 CPU 组装）。
    pub native_indexed: bool,
    /// 每批命令数上限（域内冻结语料）。
    pub max_command_count: u32,
    /// 降级/差异标注（全原生时为 None）。
    pub note: Option<&'static str>,
}

/// 静态能力矩阵。
pub const ID_CAPS: [IdCaps; 3] = [
    IdCaps {
        backend: IdBackend::D3D,
        native_draw: true,
        native_indexed: true,
        max_command_count: 65_536,
        note: None,
    },
    IdCaps {
        backend: IdBackend::Vulkan,
        native_draw: true,
        native_indexed: true,
        max_command_count: 65_536,
        note: None,
    },
    IdCaps {
        backend: IdBackend::Metal,
        native_draw: true,
        native_indexed: false,
        max_command_count: 16_384,
        note: Some("索引变体回退 CPU 组装"),
    },
];

/// 能力查询。
pub fn caps_for(b: IdBackend) -> Option<&'static IdCaps> {
    for c in ID_CAPS.iter() {
        if c.backend == b {
            return Some(c);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 三、对接预留（I 域剔除组，深度实现随剔除组推进）
// ---------------------------------------------------------------------------

/// I 域剔除组对接预留声明（字面量冻结）。
pub const I_CULL_RESERVATION: &str =
    "I09/I10 剔除组的剔除/LOD 决策生产 indirect 参数缓冲——GPU 驱动全 GPU 化前置（接口预留）";

/// CPU 不回读纪律声明。
pub const ZERO_CPU_READBACK: &str =
    "绘制参数驻留 GPU 缓冲，CPU 不回读——剔除决策在 GPU，回读即破坏全 GPU 化前提";

/// VE-F0029 上游衔接声明（命令生成本体归 VE-A 域）。
pub const VEA29_UPSTREAM_NOTICE: &str =
    "间接命令生成与调试回放归 VE-F0029（VE-A 域）——本域管接口契约预留，衔接不重叠";

/// 一期剔除桥计划：剔除组产出一批可见 draw 参数块（只产计划不执行）。
pub fn cull_bridge_plan() -> Option<IndirectBatch> {
    let mut commands: Vec<IndirectCommand> = Vec::new();
    let mut k: u32 = 0;
    while k < 2 {
        commands.push(IndirectCommand {
            draw_type: DrawType::IndirectDraw,
            buffer_id: 900,
            offset_bytes: k * INDIRECT_ARGS_STRIDE,
        });
        k += 1;
    }
    let batch = IndirectBatch {
        buffer_id: 900,
        commands,
    };
    if batch.validate(&|_c| {
        Some(IndirectArgs {
            count: 36,
            instance_count: 1,
            first: 0,
            first_instance: 0,
        })
    })
    .is_err()
    {
        return None;
    }
    Some(batch)
}

// ---------------------------------------------------------------------------
// 四、诚实标注（与能力矩阵同源）
// ---------------------------------------------------------------------------

/// 诚实标注：后端为 None 表示全局口径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HonestyNote {
    pub backend: Option<IdBackend>,
    pub phase: &'static str,
    pub text: &'static str,
}

/// 诚实标注全集：每后端一条 + 全局一条。
pub fn honesty_notices() -> Vec<HonestyNote> {
    alloc::vec![
        HonestyNote {
            backend: Some(IdBackend::D3D),
            phase: "一期",
            text: "D3D 原生间接绘制（非索引+索引）：一期接口，GPU 驱动剔除随剔除组推进",
        },
        HonestyNote {
            backend: Some(IdBackend::Vulkan),
            phase: "一期",
            text: "Vulkan 原生间接绘制（非索引+索引）：一期接口，GPU 驱动剔除随剔除组推进",
        },
        HonestyNote {
            backend: Some(IdBackend::Metal),
            phase: "一期",
            text: "Metal 原生非索引间接，索引变体回退 CPU 组装——差异在案，随剔除组推进",
        },
        HonestyNote {
            backend: None,
            phase: "全局",
            text: "一期接口：GPU 驱动剔除随剔除组推进；参数驻留 GPU 零 CPU 回读；命令生成衔接 VE-F0029",
        },
    ]
}

/// 标注与能力矩阵对拍：非原生 indexed 后端标注必含「回退」；
/// 全局标注必含「一期」与「剔除组」。矛盾即 `ANNOTATION_MISMATCH`。
pub fn verify_notes(notes: &[HonestyNote]) -> Result<(), IdCode> {
    for n in notes {
        match n.backend {
            Some(b) => {
                let caps = match caps_for(b) {
                    Some(c) => c,
                    None => return Err(IdCode::ANNOTATION_MISMATCH),
                };
                if caps.native_indexed && n.text.contains("回退") {
                    return Err(IdCode::ANNOTATION_MISMATCH);
                }
                if !caps.native_indexed && !n.text.contains("回退") {
                    return Err(IdCode::ANNOTATION_MISMATCH);
                }
            }
            None => {
                if !n.text.contains("一期") || !n.text.contains("剔除组") {
                    return Err(IdCode::ANNOTATION_MISMATCH);
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、诊断码（独占 0x4Dxx 段）
// ---------------------------------------------------------------------------

/// vef31 诊断码。独占 `0x4Dxx` 段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdCode(pub u16);

impl IdCode {
    pub const OFFSET_MISALIGNED: IdCode = IdCode(0x4D01);
    pub const ARGS_INVALID: IdCode = IdCode(0x4D02);
    pub const BATCH_OVERFLOW: IdCode = IdCode(0x4D03);
    pub const ARGS_OVERLAP: IdCode = IdCode(0x4D04);
    pub const NOT_SUPPORTED: IdCode = IdCode(0x4D05);
    pub const ANNOTATION_MISMATCH: IdCode = IdCode(0x4D06);
    pub const BRIDGE_INVALID: IdCode = IdCode(0x4D07);
    pub const RANGE_INVALID: IdCode = IdCode(0x4D08);

    /// 全部在案码。
    pub fn all() -> [IdCode; 8] {
        [
            IdCode::OFFSET_MISALIGNED,
            IdCode::ARGS_INVALID,
            IdCode::BATCH_OVERFLOW,
            IdCode::ARGS_OVERLAP,
            IdCode::NOT_SUPPORTED,
            IdCode::ANNOTATION_MISMATCH,
            IdCode::BRIDGE_INVALID,
            IdCode::RANGE_INVALID,
        ]
    }
}

impl IdCode {
    pub fn say(self) -> String {
        let name = match self {
            IdCode::OFFSET_MISALIGNED => "参数块偏移未 16 字节对齐",
            IdCode::ARGS_INVALID => "绘制参数域非法（实例数 0 等）",
            IdCode::BATCH_OVERFLOW => "批命令数越界（空或超上限）",
            IdCode::ARGS_OVERLAP => "参数块区间重叠",
            IdCode::NOT_SUPPORTED => "后端不支持该间接绘制变体",
            IdCode::ANNOTATION_MISMATCH => "诚实标注与能力矩阵矛盾",
            IdCode::BRIDGE_INVALID => "剔除桥计划非法",
            IdCode::RANGE_INVALID => "first+count 范围溢出",
            IdCode(_) => "vef31 未在案码",
        };
        alloc::format!("0x{:04X} {}", self.0, name)
    }
}

/// 后端接入体：三事实（能力在案/变体受理/声明非空）。
#[derive(Clone, Debug)]
pub struct IdBackendFace {
    pub backend: IdBackend,
    /// 索引变体是否原生受理。
    pub indexed_accepted: bool,
    pub declare_line: String,
}

/// 三后端接入体。
pub fn backend_faces() -> Vec<IdBackendFace> {
    let mut faces: Vec<IdBackendFace> = Vec::new();
    for c in ID_CAPS.iter() {
        let line = alloc::format!(
            "后端 {} 间接绘制声明：非索引原生={} 索引原生={} 批上限={} 差异={}",
            c.backend.say(),
            c.native_draw,
            c.native_indexed,
            c.max_command_count,
            c.note.unwrap_or("无")
        );
        faces.push(IdBackendFace {
            backend: c.backend,
            indexed_accepted: c.native_indexed,
            declare_line: line,
        });
    }
    faces
}
