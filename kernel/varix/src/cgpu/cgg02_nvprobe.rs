//! CGPU-F0962 · NVIDIA 能力探测专项（CGPU-G 域 · 厂商深度适配域 · G02 组 · 目标 360 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0962`
//!
//! **判据（锚点原文）**：特性表、SKU 级、特有扩展、消费、判据。
//!
//! **职责定位（锚点原文）**：NVIDIA 特性表（RT Core 代际/Tensor 核心/
//! 显存规格/NVENC 代际——NVIDIA 特化探测，复用 F0803 体系）；SKU 级精度
//! （同代不同 SKU 细分——显存位宽/核心数差异）；特有扩展探测（DLSS 版本/
//! G-Sync Compatible 认证状态）；探测消费（能力表→降级矩阵/选路单点）。
//!
//! ## 一、NVIDIA 特性表（判据一）
//!
//! 特性表四元组（[`NvFeatureTable`]）：RT Core 代际（0=无 RT，1=Turing
//! 第一代，2=Ampere 第二代，3=Ada 第三代——代际是能力档不是型号名，
//! 型号归 SKU 面）；Tensor 核心数（0=无）；显存规格（字节数+位宽）；
//! NVENC 编码器代际（0=无硬编码）。探测结果**注入**而非本模块探测
//! （F0803 探测体系的分工：它产原始探测，本模块是 NVIDIA 特化建模面
//! ——零 IO、零墙钟、结果即数据）。
//!
//! ## 二、SKU 级精度（判据二）
//!
//! 同代不同 SKU 的细分表（[`SkuProfile`]）：显存位宽（bit）/核心数/
//! 加速频率——同一芯片代际下 SKU 差异决定实际能力（同代 50 级与 90 级
//! 不是一张能力表）。SKU 键 = 设备 id + PCI revision（探测侧给定）；
//! 未登记 SKU 走代际默认行（诚实降级，不虚报）。
//!
//! ## 三、特有扩展（判据三）
//!
//! DLSS 版本（0=不支持，>0=主版本号——超分可用性）；G-Sync Compatible
//! 认证（布尔——自适应同步可开启）。扩展是**可选项**不是承诺：消费方
//! 必须按 0/false 路径可用来设计（扩展缺席不破坏功能，只少增强）。
//!
//! ## 四、探测消费（判据四）
//!
//! 能力表 → 降级矩阵映射（[`degrade_hint`]）：RT 缺席→光线追踪路径
//! 整体关闭；Tensor 缺席→AI 增强全关；NVENC 缺席→编码走软编；显存
//! 低于阈值→贴图档位降级。选路单点：消费方只经 [`NvProbeReport`]
//! 一处取 NVIDIA 结论（单点取用纪律——散点读探测是漂移之源）。
//!
//! **对接**：F0961（G 域开工——本模块是其 NVIDIA 面）；F0803（探测
//! 体系——上游数据源）；F0804+（认证流程——消费本表判定）；D 域
//! 降级矩阵（degrade_hint 的下游）。零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const NV_PROBE_VERSION: &str = "G02-nvprobe-v1";

/// RT Core 代际上限（当前已知最高代；探测值超过按此钳制并标注）。
pub const RT_CORE_GEN_MAX: u8 = 3;

/// 显存降级阈值（字节；低于此值贴图档位降级——当前策略值，可调）。
pub const VRAM_DEGRADE_THRESHOLD: u64 = 4 * 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// 二、诊断码（0x32 段独占：G02 NVIDIA 探测面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x32 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 设备未登记（SKU 表无此键且无代际默认）。
    DeviceUnregistered,
    /// 显存规格非法（0 字节——探测未闭合就上报）。
    BadVramSpec,
    /// 代际越界（超过 RT_CORE_GEN_MAX 且未声明钳制）。
    GenOutOfRange,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::DeviceUnregistered => "设备未登记：SKU 表无此键，先在 F0961 厂商清单登记或提供代际默认行",
            XcCode::BadVramSpec => "显存规格非法：0 字节说明探测未闭合，拒绝建模并回查 F0803 上游",
            XcCode::GenOutOfRange => "代际越界：超过已知最高代，钳制到上限并在报告标注未知代",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、NVIDIA 特性表（判据一）
// ---------------------------------------------------------------------------

/// NVIDIA 特性表四元组（探测结果的建模面；结果即数据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NvFeatureTable {
    /// RT Core 代际（0=无；1=Turing/2=Ampere/3=Ada）。
    pub rt_core_gen: u8,
    /// Tensor 核心数（0=无）。
    pub tensor_cores: u32,
    /// 显存字节数。
    pub vram_bytes: u64,
    /// 显存位宽（bit）。
    pub vram_bus_bits: u16,
    /// NVENC 编码器代际（0=无硬编码）。
    pub nvenc_gen: u8,
}

impl NvFeatureTable {
    /// 基础合法性（显存必须非零——探测未闭合不算数据）。
    pub const fn validate(&self) -> Result<(), XcCode> {
        if self.vram_bytes == 0 {
            return Err(XcCode::BadVramSpec);
        }
        Ok(())
    }

    /// 代际钳制（超上限按上限建模并报告未知代——诚实：不虚构新代能力）。
    pub fn clamp_report(&self) -> (NvFeatureTable, bool) {
        if self.rt_core_gen > RT_CORE_GEN_MAX {
            let mut clamped = *self;
            clamped.rt_core_gen = RT_CORE_GEN_MAX;
            (clamped, true)
        } else {
            (*self, false)
        }
    }
}

// ---------------------------------------------------------------------------
// 四、SKU 级精度（判据二）
// ---------------------------------------------------------------------------

/// SKU 细分行（同代不同 SKU 的差异面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkuProfile {
    /// 设备 id（厂商探测给的稳定键）。
    pub device_id: u32,
    /// PCI revision（同设备多 SKU 的二级键）。
    pub pci_rev: u8,
    /// CUDA/流处理器核心数。
    pub cores: u32,
    /// 加速频率（MHz）。
    pub boost_mhz: u32,
    /// 本 SKU 有效显存字节数（与代际表可能不同——定制卡）。
    pub vram_bytes: u64,
}

/// SKU 表（device_id+pci_rev → 细分行；未命中走代际默认）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkuRegistry {
    rows: Vec<SkuProfile>,
}

impl SkuRegistry {
    /// 登记一行（同键重复登记以首见为准——防探测重试污染）。
    pub fn register(&mut self, row: SkuProfile) {
        let mut hit = false;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.device_id == row.device_id && r.pci_rev == row.pci_rev {
                    hit = true;
                    break;
                }
            }
            i += 1;
        }
        if !hit {
            self.rows.push(row);
        }
    }

    /// 精确查 SKU（O(n) 线性——SKU 行容量小，线性即常数）。
    pub fn lookup(&self, device_id: u32, pci_rev: u8) -> Option<SkuProfile> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.device_id == device_id && r.pci_rev == pci_rev {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    /// 在册行数。
    pub const fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空判定。
    pub const fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 五、特有扩展（判据三）
// ---------------------------------------------------------------------------

/// NVIDIA 特有扩展（可选项；缺席不破坏功能只少增强）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NvExtensions {
    /// DLSS 主版本（0=不支持；超分是增强不是依赖）。
    pub dlss_version: u32,
    /// G-Sync Compatible 认证（自适应同步可开启）。
    pub gsync_compatible: bool,
}

// ---------------------------------------------------------------------------
// 六、探测消费（判据四：降级矩阵 + 选路单点）
// ---------------------------------------------------------------------------

/// NVIDIA 探测报告（选路单点：消费方只经此取 NVIDIA 结论）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NvProbeReport {
    /// 代际特性表（已钳制）。
    pub features: NvFeatureTable,
    /// SKU 细分（None=未登记，走代际默认——诚实降级）。
    pub sku: Option<SkuProfile>,
    /// 特有扩展。
    pub ext: NvExtensions,
    /// 代际越界钳制标记（true=探测值超已知代，按上限建模）。
    pub gen_clamped: bool,
}

/// 降级提示矩阵（能力表→降级建议；消费方按建议路由，本模块不执行降级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegradeHints {
    /// 光线追踪路径整体关闭（RT 缺席）。
    pub rt_disabled: bool,
    /// AI 增强全关（Tensor 缺席）。
    pub ai_enhance_disabled: bool,
    /// 视频编码走软编（NVENC 缺席）。
    pub encode_fallback_sw: bool,
    /// 贴图档位降级（显存低于阈值）。
    pub texture_downgrade: bool,
    /// 超分不可用（DLSS=0）。
    pub upscaler_off: bool,
    /// 自适应同步不可开（G-Sync 未认证）。
    pub adaptive_sync_off: bool,
}

impl DegradeHints {
    /// 从探测报告推导降级矩阵（纯函数、确定性）。
    pub const fn derive(r: &NvProbeReport) -> DegradeHints {
        DegradeHints {
            rt_disabled: r.features.rt_core_gen == 0,
            ai_enhance_disabled: r.features.tensor_cores == 0,
            encode_fallback_sw: r.features.nvenc_gen == 0,
            texture_downgrade: r.features.vram_bytes < VRAM_DEGRADE_THRESHOLD,
            upscaler_off: r.ext.dlss_version == 0,
            adaptive_sync_off: !r.ext.gsync_compatible,
        }
    }

    /// 任一降级生效判定（快速门）。
    pub const fn any(&self) -> bool {
        self.rt_disabled
            || self.ai_enhance_disabled
            || self.encode_fallback_sw
            || self.texture_downgrade
            || self.upscaler_off
            || self.adaptive_sync_off
    }
}

/// 探测消费入口（单点）：特性表 + SKU 注册 + 扩展 → 报告（含钳制与校验）。
pub fn build_report(
    device_id: u32,
    pci_rev: u8,
    raw: NvFeatureTable,
    ext: NvExtensions,
    registry: &SkuRegistry,
) -> Result<NvProbeReport, XcCode> {
    raw.validate()?;
    let (features, clamped) = raw.clamp_report();
    let sku = registry.lookup(device_id, pci_rev);
    Ok(NvProbeReport {
        features,
        sku,
        ext,
        gen_clamped: clamped,
    })
}

/// 报告摘要（人读；台账/遥测渲染面）。
pub fn report_summary(r: &NvProbeReport) -> String {
    let mut s = String::from("NV 报告：RT 代 ");
    s.push_str(&r.features.rt_core_gen.to_string());
    s.push_str("，Tensor ");
    s.push_str(&r.features.tensor_cores.to_string());
    s.push_str("，显存 ");
    s.push_str(&(r.features.vram_bytes / (1024 * 1024)).to_string());
    s.push_str("MiB/");
    s.push_str(&r.features.vram_bus_bits.to_string());
    s.push_str("bit，NVENC 代 ");
    s.push_str(&r.features.nvenc_gen.to_string());
    s.push_str("，DLSS ");
    s.push_str(&r.ext.dlss_version.to_string());
    if r.ext.gsync_compatible {
        s.push_str("，GSync 认证");
    }
    if r.gen_clamped {
        s.push_str("，代际越界已钳制");
    }
    s
}

// ---------------------------------------------------------------------------
// 七、多卡聚合与认证对照（探测消费的消费面深化）
// ---------------------------------------------------------------------------

/// 多卡水线聚合（判据一的消费面深化：同系统多 NVIDIA 卡时，能力表
/// 取**最弱卡**（木桶口径）——水线=最弱卡是调度能承诺的上界，取
/// 最强卡的表会承诺调度不了的路径）。
pub fn aggregate_weakest(reports: &[NvProbeReport]) -> Result<NvProbeReport, XcCode> {
    if reports.is_empty() {
        return Err(XcCode::DeviceUnregistered);
    }
    let mut best: Option<NvProbeReport> = None;
    let mut i = 0usize;
    while i < reports.len() {
        let cur = match reports.get(i) {
            Some(r) => r,
            None => break,
        };
        best = match best {
            None => Some(*cur),
            Some(b) => {
                // 逐维取弱：RT 代/Tensor/NVENC 取小；显存取小；位宽取小。
                let weaker = NvProbeReport {
                    features: NvFeatureTable {
                        rt_core_gen: b.features.rt_core_gen.min(cur.features.rt_core_gen),
                        tensor_cores: b.features.tensor_cores.min(cur.features.tensor_cores),
                        vram_bytes: b.features.vram_bytes.min(cur.features.vram_bytes),
                        vram_bus_bits: b.features.vram_bus_bits.min(cur.features.vram_bus_bits),
                        nvenc_gen: b.features.nvenc_gen.min(cur.features.nvenc_gen),
                    },
                    sku: None, // 聚合面无单卡 SKU 语义
                    ext: NvExtensions {
                        dlss_version: b.ext.dlss_version.min(cur.ext.dlss_version),
                        gsync_compatible: b.ext.gsync_compatible && cur.ext.gsync_compatible,
                    },
                    gen_clamped: b.gen_clamped || cur.gen_clamped,
                };
                Some(weaker)
            }
        };
        i += 1;
    }
    best.ok_or(XcCode::DeviceUnregistered)
}

/// 最低规格行（认证对照基准；F0804 认证流程的判定输入——字面量冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinSpec {
    /// RT 代下界（0=无要求）。
    pub rt_min: u8,
    /// Tensor 下界（0=无要求）。
    pub tensor_min: u32,
    /// 显存下界（字节）。
    pub vram_min: u64,
    /// NVENC 代下界（0=无要求）。
    pub nvenc_min: u8,
    /// DLSS 版本下界（0=无要求）。
    pub dlss_min: u32,
}

/// 认证对照（判据四的判定面：报告 vs 最低规格——逐维给出达标性；
/// 对照是查询，认证授予是 F0804 的职责）。
pub const fn certify_check(r: &NvProbeReport, spec: &MinSpec) -> (bool, bool, bool, bool, bool) {
    (
        r.features.rt_core_gen >= spec.rt_min,
        r.features.tensor_cores >= spec.tensor_min,
        r.features.vram_bytes >= spec.vram_min,
        r.features.nvenc_gen >= spec.nvenc_min,
        r.ext.dlss_version >= spec.dlss_min,
    )
}

/// SKU 显存覆写（判据二的精度面：SKU 行显存与代际表冲突时以 SKU
/// 为准——定制卡显存与公版不同是常态；覆写留痕返回是否覆写）。
pub fn apply_sku_vram(r: &mut NvProbeReport) -> bool {
    if let Some(sku) = r.sku {
        if sku.vram_bytes != 0 && sku.vram_bytes != r.features.vram_bytes {
            r.features.vram_bytes = sku.vram_bytes;
            return true;
        }
    }
    false
}

/// 显存带宽估算（MB/s：位宽 × 等效频率不在表内——此处给位宽分档
/// 带宽档位而非精确值：x128/x192/x256/x320/x384 五档；精确带宽走
/// I 域传输面实测）。返回档位序（0-4；越界按最大档）。
pub const fn vram_bus_tier(bus_bits: u16) -> usize {
    if bus_bits >= 384 {
        4
    } else if bus_bits >= 320 {
        3
    } else if bus_bits >= 256 {
        2
    } else if bus_bits >= 192 {
        1
    } else {
        0
    }
}
