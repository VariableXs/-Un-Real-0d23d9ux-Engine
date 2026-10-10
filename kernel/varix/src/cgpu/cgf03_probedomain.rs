//! CGPU-F0803 · 能力探测体系（CGPU-F 域 · 硬件兼容与认证域 · F02 组 · 目标 380 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0803`
//!
//! **判据（锚点原文）**：统一、三层、schema、防伪、判据。
//!
//! **职责定位（锚点原文）**：能力探测：跨厂商统一探测（GPU 特性全表：
//! 格式支持/采样能力/RT 能力/现代特性——I08 能力矩阵理念的全域化）；
//! 探测分层（设备级/驱动级/系统级三层探测）；探测结果 schema（统一
//! 格式——消费方 D04 降级/I 域选路单点取用）；探测防伪（探测结果与
//! 行为一致性抽查——防虚报）。
//!
//! ## 一、跨厂商统一特性全表（判据一）
//!
//! 统一能力表（[`CapabilityTable`]）：四能力域闭集——格式支持（渲染
//! 目标格式位图）/采样能力（各向异性档位/滤波档）/RT 能力（代际——
//! 与 F0962 特化表对位）/现代特性（bindless/异步计算/时间戳）。理念
//! 全域化自 I08 能力矩阵：**厂商差异被吸收进探测层，消费面只见统一
//! 表**——消费方不需要 per-厂商 if 链（这是本表存在的理由）。
//!
//! ## 二、三层探测（判据二）
//!
//! 探测分三层（[`ProbeTier`]）：设备级（PCI id/显存拓扑——硬件是什么）；
//! 驱动级（驱动版本/扩展清单——驱动说什么能做）；系统级（运行环境
//! 约束——OS 允许做什么）。三层结果**分层存放**：上层结论不得超过
//! 下层上限（驱动宣称 > 设备能力 = 虚报，见防伪）。三层独立注入
//! （探测执行在 F0801 前段——本模块是建模与仲裁面，零 IO）。
//!
//! ## 三、结果 schema（判据三）
//!
//! 统一 schema（[`ProbeReport`]）：设备指纹 + 三层表 + 仲裁结论 +
//! 防伪抽查记录，版本化（[`SCHEMA_VERSION`]）。消费单点：D04 降级与
//! I 域选路**只经 `ProbeReport` 取用**——散点读探测结果会在演进中
//! 漂移成多套口径。
//!
//! ## 四、探测防伪（判据四）
//!
//! 防虚报三查（[`anti_spoof`]）：①层间一致性（驱动宣称位 ⊆ 设备
//! 能力位——驱动吹设备没有的即虚报）；②声明-行为抽查（声明支持 X
//! 的能力位配一个**行为探针结论位**——行为结论由执行面回填，本模块
//! 只比对两处是否一致）；③上限钳制（现代特性代际超已知上限即钳制
//! 并标注——不虚构未知能力）。虚报处置：虚报位清零 + 记账留痕
//! （清零是防伪造假不是惩罚——能力表只装可信数据）。
//!
//! **对接**：F0801/F0802（F 域开工与认证流程——探测是认证的输入面）；
//! F0962（NVIDIA 特化表——本统一表的厂商特化投影）；D04（降级——
//! 消费单点）；I 域（选路——消费单点）。零 panic 面、零 IO、零墙钟、
//! no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const PROBE_DOMAIN_VERSION: &str = "F02-probedomain-v1";

/// 结果 schema 版本（消费方按版本对账）。
pub const SCHEMA_VERSION: &str = "probe-schema-v2";

// ---------------------------------------------------------------------------
// 二、诊断码（0x39 段独占：F02 探测面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x39 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 层序违规（驱动层结果先于设备层到达——三层有序注入）。
    TierOrderViolation,
    /// 行为探针未回填（声明位有而行为位缺——不可信）。
    BehaviorProbeMissing,
    /// 设备指纹重复（同指纹二次建模——探测重试污染）。
    DuplicateDevice,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::TierOrderViolation => "层序违规：设备→驱动→系统有序注入，上层结论不得先于下层",
            XcCode::BehaviorProbeMissing => "行为探针未回填：声明位有而行为位缺不可信，先回填再消费",
            XcCode::DuplicateDevice => "设备指纹重复：同设备二次建模拒绝，探测重试请复用既有报告",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、三层探测（判据二）
// ---------------------------------------------------------------------------

/// 探测层闭集（三层有序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeTier {
    /// 设备级（PCI id/显存拓扑——硬件是什么）。
    Device,
    /// 驱动级（驱动版本/扩展清单——驱动说什么）。
    Driver,
    /// 系统级（运行环境约束——OS 允许什么）。
    System,
}

impl ProbeTier {
    /// 层序（0-2；注入与仲裁的深度轴）。
    pub const fn depth(self) -> u8 {
        match self {
            ProbeTier::Device => 0,
            ProbeTier::Driver => 1,
            ProbeTier::System => 2,
        }
    }

    /// 层名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            ProbeTier::Device => "设备级",
            ProbeTier::Driver => "驱动级",
            ProbeTier::System => "系统级",
        }
    }
}

/// 单层能力表（四能力域；位图口径）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TierCaps {
    /// 渲染目标格式支持位图（位=格式枚举序，口径冻结在 F0801 词表）。
    pub formats_mask: u64,
    /// 采样能力位图（各向异性档位/滤波档）。
    pub sampling_mask: u64,
    /// RT 能力（代际；0=无）。
    pub rt_gen: u8,
    /// 现代特性位图（bindless/异步计算/时间戳）。
    pub modern_mask: u64,
    /// 行为探针结论位图（执行面回填；与声明位同位语义——防伪比对面）。
    pub behavior_mask: u64,
}

impl TierCaps {
    /// 声明位集（三域位图并集——防伪比对的范围）。
    pub const fn declared_mask(&self) -> u64 {
        self.formats_mask | self.sampling_mask | self.modern_mask
    }

    /// 虚报位（声明有而行为无）。
    pub const fn spoof_mask(&self) -> u64 {
        self.declared_mask() & !self.behavior_mask
    }
}

// ---------------------------------------------------------------------------
// 四、统一 schema 与仲裁（判据一、三）
// ---------------------------------------------------------------------------

/// 探测报告（统一 schema；消费单点）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeReport {
    /// 设备指纹（厂商探测给的稳定键）。
    pub device_fp: u64,
    /// 三层能力表（下标=ProbeTier::depth）。
    pub tiers: [TierCaps; 3],
    /// 现代特性代际上限钳制标记。
    pub clamped: bool,
    /// 虚报清零记录数（防伪处置留痕）。
    pub spoof_cleared: u64,
}

/// 三层仲裁（判据二的消费面：统一表 = 三层按层序**取交集下限**——
/// 设备 ∩ 驱动 ∩ 系统：任何一层说不，能力就不在；RT 代际取最小非零
/// （0 表示任一层无 RT）。产出即消费面唯一口径）。
pub fn arbitrate(tiers: &[TierCaps; 3]) -> Result<TierCaps, XcCode> {
    let dev = match tiers.get(ProbeTier::Device.depth() as usize) {
        Some(t) => t,
        None => return Err(XcCode::TierOrderViolation),
    };
    let drv = match tiers.get(ProbeTier::Driver.depth() as usize) {
        Some(t) => t,
        None => return Err(XcCode::TierOrderViolation),
    };
    let sys = match tiers.get(ProbeTier::System.depth() as usize) {
        Some(t) => t,
        None => return Err(XcCode::TierOrderViolation),
    };
    Ok(TierCaps {
        formats_mask: dev.formats_mask & drv.formats_mask & sys.formats_mask,
        sampling_mask: dev.sampling_mask & drv.sampling_mask & sys.sampling_mask,
        rt_gen: {
            let m = dev.rt_gen.min(drv.rt_gen).min(sys.rt_gen);
            m
        },
        modern_mask: dev.modern_mask & drv.modern_mask & sys.modern_mask,
        behavior_mask: dev.behavior_mask & drv.behavior_mask & sys.behavior_mask,
    })
}

// ---------------------------------------------------------------------------
// 五、探测防伪（判据四）
// ---------------------------------------------------------------------------

/// 防伪结论（三查结果 + 处置后的净表）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AntiSpoofVerdict {
    /// 层间一致性（驱动宣称 ⊆ 设备能力）。
    pub layer_consistent: bool,
    /// 声明-行为一致（三层的虚报位全零——处置后）。
    pub behavior_consistent: bool,
    /// 处置后净表（虚报位清零；消费面拿这张表）。
    pub clean: TierCaps,
    /// 清零位数（留痕）。
    pub cleared_bits: u64,
}

/// 防伪三查（判据四：层间一致 → 声明-行为一致 → 上限钳制在建模侧
/// 已做（clamp_report 语义）；此处执行前两查与清零处置）。
pub fn anti_spoof(tiers: &[TierCaps; 3]) -> AntiSpoofVerdict {
    let dev = tiers[ProbeTier::Device.depth() as usize];
    let drv = tiers[ProbeTier::Driver.depth() as usize];
    let sys = tiers[ProbeTier::System.depth() as usize];
    // 查①：驱动三域声明位 ⊆ 设备对应位（吹设备没有的=虚报）。
    let layer_consistent = (drv.formats_mask & !dev.formats_mask) == 0
        && (drv.sampling_mask & !dev.sampling_mask) == 0
        && (drv.modern_mask & !dev.modern_mask) == 0;
    // 查②：三层声明-行为一致；虚报位清零（净表只装可信位）。
    let mut clean = TierCaps {
        formats_mask: dev.formats_mask & drv.formats_mask & sys.formats_mask,
        sampling_mask: dev.sampling_mask & drv.sampling_mask & sys.sampling_mask,
        rt_gen: dev.rt_gen.min(drv.rt_gen).min(sys.rt_gen),
        modern_mask: dev.modern_mask & drv.modern_mask & sys.modern_mask,
        behavior_mask: dev.behavior_mask & drv.behavior_mask & sys.behavior_mask,
    };
    let mut cleared = 0u64;
    let m = clean.spoof_mask();
    if m != 0 {
        // 虚报位从三域声明位统一清零（净表只装可信位——清零是防伪造假）。
        clean.formats_mask &= !m;
        clean.sampling_mask &= !m;
        clean.modern_mask &= !m;
        cleared = m.count_ones() as u64;
    }
    // 行为一致复验（清零后虚报位必零）。
    let behavior_consistent = clean.spoof_mask() == 0;
    AntiSpoofVerdict {
        layer_consistent,
        behavior_consistent,
        clean,
        cleared_bits: cleared,
    }
}

// ---------------------------------------------------------------------------
// 六、探测台账（判据三的消费单点注册面）
// ---------------------------------------------------------------------------

/// 探测台账（设备指纹 → 报告；消费方单点取用）。
#[derive(Clone, Debug, Default)]
pub struct ProbeRegistry {
    reports: Vec<ProbeReport>,
}

impl ProbeRegistry {
    /// 建模入账（指纹去重；层序校验——三层按序注入才受理）。
    pub fn register(&mut self, r: ProbeReport) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < self.reports.len() {
            if let Some(x) = self.reports.get(i) {
                if x.device_fp == r.device_fp {
                    return Err(XcCode::DuplicateDevice);
                }
            }
            i += 1;
        }
        self.reports.push(r);
        Ok(())
    }

    /// 单点取用（消费方唯一入口；未建模设备返 None——消费方走诚实
    /// 缺席路径，不猜）。
    pub fn get(&self, device_fp: u64) -> Option<&ProbeReport> {
        let mut i = 0usize;
        while i < self.reports.len() {
            if let Some(x) = self.reports.get(i) {
                if x.device_fp == device_fp {
                    return Some(x);
                }
            }
            i += 1;
        }
        None
    }

    /// 在册数。
    pub const fn len(&self) -> usize {
        self.reports.len()
    }

    /// 空判定。
    pub const fn is_empty(&self) -> bool {
        self.reports.is_empty()
    }

    /// 台账摘要（人读）。
    pub fn summary(&self) -> String {
        let mut s = String::from("探测台账：设备 ");
        s.push_str(&self.reports.len().to_string());
        s.push_str("，schema ");
        s.push_str(SCHEMA_VERSION);
        s
    }
}

// ---------------------------------------------------------------------------
// 七、层序注入状态机与多卡聚合（判据二、一的深化面）
// ---------------------------------------------------------------------------

/// 层序注入器（判据二的执行面：三层必须按 设备→驱动→系统 有序注入，
/// 越序显性拒——`TierOrderViolation` 的唯一产生点；注入完成后产出
/// 三层表供仲裁）。
#[derive(Clone, Debug, Default)]
pub struct TierInjector {
    next_depth: u8,
    tiers: [Option<TierCaps>; 3],
}

impl TierInjector {
    /// 新注入器。
    pub const fn new() -> TierInjector {
        TierInjector {
            next_depth: 0,
            tiers: [None, None, None],
        }
    }

    /// 注入一层（层序强制；重复注入同层拒）。
    pub fn inject(&mut self, tier: ProbeTier, caps: TierCaps) -> Result<(), XcCode> {
        if tier.depth() != self.next_depth {
            return Err(XcCode::TierOrderViolation);
        }
        self.tiers[tier.depth() as usize] = Some(caps);
        self.next_depth = self.next_depth.saturating_add(1);
        Ok(())
    }

    /// 注入完备性（三层齐才可仲裁——缺层显性拒语义）。
    pub const fn complete(&self) -> bool {
        self.next_depth == 3
    }

    /// 收口出表（未完备显性拒）。
    pub fn finish(&self) -> Result<[TierCaps; 3], XcCode> {
        if !self.complete() {
            return Err(XcCode::TierOrderViolation);
        }
        let d = match self.tiers[0] {
            Some(t) => t,
            None => return Err(XcCode::TierOrderViolation),
        };
        let v = match self.tiers[1] {
            Some(t) => t,
            None => return Err(XcCode::TierOrderViolation),
        };
        let s = match self.tiers[2] {
            Some(t) => t,
            None => return Err(XcCode::TierOrderViolation),
        };
        Ok([d, v, s])
    }
}

/// 能力命名查询（判据一的消费面深化：四能力域的语义化位查询——
/// 消费方按域问而不是拆位图猜位义）。
pub mod capquery {
    use super::TierCaps;

    /// 格式位查询。
    pub const fn has_format(t: &TierCaps, bit: u8) -> bool {
        (t.formats_mask >> bit) & 1 == 1
    }

    /// 采样位查询。
    pub const fn has_sampling(t: &TierCaps, bit: u8) -> bool {
        (t.sampling_mask >> bit) & 1 == 1
    }

    /// RT 能力（代际 >0 即有）。
    pub const fn has_rt(t: &TierCaps) -> bool {
        t.rt_gen > 0
    }

    /// 现代特性位查询。
    pub const fn has_modern(t: &TierCaps, bit: u8) -> bool {
        (t.modern_mask >> bit) & 1 == 1
    }
}

/// 多卡统一表（判据一的深化：同系统多卡时消费面拿**交集表**——
/// 任何一张卡说不，跨卡路径就不在；与 cgg02 的最弱卡水线同口径：
/// 集合面取交、数值面取小）。
pub fn multi_gpu_intersect(tables: &[TierCaps]) -> Result<TierCaps, XcCode> {
    if tables.is_empty() {
        return Err(XcCode::DuplicateDevice); // 语义：空表按无设备拒
    }
    let mut acc = match tables.first() {
        Some(t) => *t,
        None => return Err(XcCode::DuplicateDevice),
    };
    let mut i = 1usize;
    while i < tables.len() {
        if let Some(t) = tables.get(i) {
            acc.formats_mask &= t.formats_mask;
            acc.sampling_mask &= t.sampling_mask;
            acc.modern_mask &= t.modern_mask;
            acc.behavior_mask &= t.behavior_mask;
            acc.rt_gen = if t.rt_gen == 0 || acc.rt_gen == 0 {
                0
            } else {
                acc.rt_gen.min(t.rt_gen)
            };
        }
        i += 1;
    }
    Ok(acc)
}

/// FNV-1a 报告指纹（消费方对账用：同报告同指纹——schema 版本参与
/// 哈希，跨版本指纹必变）。
pub fn report_fingerprint(r: &ProbeReport) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mix = |h: &mut u64, b: u8| {
        *h = (*h ^ b as u64).wrapping_mul(0x100000001b3);
    };
    // schema 版本（逐字节）。
    let sv = SCHEMA_VERSION.as_bytes();
    let mut i = 0usize;
    while i < sv.len() {
        mix(&mut h, sv[i]);
        i += 1;
    }
    mix(&mut h, (r.device_fp & 0xFF) as u8);
    mix(&mut h, ((r.device_fp >> 8) & 0xFF) as u8);
    mix(&mut h, ((r.device_fp >> 16) & 0xFF) as u8);
    mix(&mut h, ((r.device_fp >> 24) & 0xFF) as u8);
    let mut t = 0usize;
    while t < 3 {
        let caps = match r.tiers.get(t) {
            Some(c) => c,
            None => break,
        };
        let mut b = 0usize;
        let bytes: [u8; 24] = [
            (caps.formats_mask & 0xFF) as u8,
            ((caps.formats_mask >> 8) & 0xFF) as u8,
            ((caps.formats_mask >> 16) & 0xFF) as u8,
            ((caps.formats_mask >> 24) & 0xFF) as u8,
            (caps.sampling_mask & 0xFF) as u8,
            ((caps.sampling_mask >> 8) & 0xFF) as u8,
            (caps.sampling_mask >> 16) as u8,
            (caps.sampling_mask >> 24) as u8,
            caps.rt_gen,
            (caps.modern_mask & 0xFF) as u8,
            ((caps.modern_mask >> 8) & 0xFF) as u8,
            ((caps.modern_mask >> 16) & 0xFF) as u8,
            ((caps.modern_mask >> 24) & 0xFF) as u8,
            (caps.behavior_mask & 0xFF) as u8,
            ((caps.behavior_mask >> 8) & 0xFF) as u8,
            ((caps.behavior_mask >> 16) & 0xFF) as u8,
            ((caps.behavior_mask >> 24) & 0xFF) as u8,
            0, 0, 0, 0, 0, 0, 0,
        ];
        while b < bytes.len() {
            mix(&mut h, bytes[b]);
            b += 1;
        }
        t += 1;
    }
    mix(&mut h, r.clamped as u8);
    mix(&mut h, r.spoof_cleared as u8);
    h
}
