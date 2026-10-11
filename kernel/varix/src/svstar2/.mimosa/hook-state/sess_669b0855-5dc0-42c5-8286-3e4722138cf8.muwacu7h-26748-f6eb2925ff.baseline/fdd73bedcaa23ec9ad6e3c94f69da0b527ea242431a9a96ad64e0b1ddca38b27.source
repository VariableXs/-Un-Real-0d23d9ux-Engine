//! VE-F0007 · GPU 能力查询与特性位图（VE-A 域 · 内核图形抽象层 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0007`
//!
//! **判据（锚点原文）**：标准位图、分级降级、实测校准、缓存失效、判据；
//! 特性位图含供应商扩展位的隔离声明（厂商私有特性不混入标准位）；
//! 降级路径含效果损失量化（不支持的特性降级后画质差多少）；
//! 缓存失效含驱动更新联动（驱动一换位图重查）。
//!
//! **设计要点**：
//! - 标准位图：特性键 → 位号是**冻结表**（位序就是契约，位序漂移 =
//!   全部下游位图失义），64 位内前 48 位给标准特性，48..64 给厂商扩展；
//! - 厂商隔离：厂商位进独立的 `VendorExtensions`（带 vid 标签），
//!   `standard_bitmap()` 永不含厂商位——私有特性混入标准位会让
//!   跨厂商比较失真，这是隔离声明的实质；
//! - 实测校准：驱动自报位图 vs 实测位图交叉，"自报有实测无"判虚报，
//!   位图以实测为准修正并留审计（虚报位不清除 = 把假能力卖给上层）；
//! - 分级降级：每个特性都有预登记降级路径 + 效果损失量化（0.0~1.0），
//!   不支持的特性必须能回答"降级后画质差多少"——不量化就是空话；
//! - 缓存：查询结果按 (slot, driver_version) 缓存，驱动一换必失效重查
//!   （同卡不同驱动能力可能天差地别——VE-F0001 的指纹纪律同源）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、标准特性位图（冻结位序）
// ---------------------------------------------------------------------------

/// 标准特性键 → 位号的冻结表。位序即契约：只许追加，不许重排。
pub const FEATURE_KEYS: [&str; 12] = [
    "raster2d",      // 0
    "raster3d",      // 1
    "compute",       // 2
    "hdr",           // 3
    "async_compute", // 4
    "video_decode",  // 5
    "video_encode",  // 6
    "color_mgmt",    // 7
    "tessellation",  // 8
    "ray_trace",     // 9
    "mesh_shader",   // 10
    "tensor",        // 11
];

/// 标准特性位的位域上界（48..64 保留给厂商扩展）。
pub const STANDARD_BITS: u32 = 48;
/// 厂商扩展位域（48..64）。
pub const VENDOR_BITS_BASE: u32 = 48;

/// 按键查位号。未知键返回 None（不猜）。
pub fn bit_of(key: &str) -> Option<u32> {
    FEATURE_KEYS.iter().position(|k| *k == key).map(|i| i as u32)
}

/// 标准位图：只含标准特性位的封装。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct StandardBitmap(pub u64);

impl StandardBitmap {
    pub fn set(&mut self, key: &str, on: bool) -> bool {
        match bit_of(key) {
            Some(b) if b < STANDARD_BITS => {
                if on {
                    self.0 |= 1u64 << b;
                } else {
                    self.0 &= !(1u64 << b);
                }
                true
            }
            _ => false,
        }
    }

    pub fn get(&self, key: &str) -> Option<bool> {
        bit_of(key).filter(|b| *b < STANDARD_BITS).map(|b| self.0 & (1u64 << b) != 0)
    }

    /// 厂商位隔离断言（判据点名）：标准位图的高 16 位必须恒零。
    pub fn vendor_free(&self) -> bool {
        self.0 >> VENDOR_BITS_BASE == 0
    }

    /// 读屏可达：逐位列出（只列已支持位）。
    pub fn describe(&self) -> String {
        if self.0 == 0 {
            return "无标准特性支持".to_string();
        }
        let on: Vec<&str> = FEATURE_KEYS
            .iter()
            .filter(|k| self.get(k) == Some(true))
            .map(|k| *k)
            .collect();
        format!("支持 {} 项标准特性：{}", on.len(), on.join("、"))
    }
}

/// 厂商扩展位（隔离域：带 vid 标签，永不混入标准位图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct VendorExtensions {
    /// 厂商 ID（0 = 无）
    pub vid: u32,
    /// 厂商私有位（语义只有厂商自己知道）
    pub bits: u16,
}

impl VendorExtensions {
    /// 厂商位装入隔离域（位号 ≥16 的私有位装不进 u16，天然拒收）。
    pub fn pack(vid: u32, bits: u16) -> VendorExtensions {
        VendorExtensions { vid, bits }
    }
}

// ---------------------------------------------------------------------------
// 二、查询输入与实测校准
// ---------------------------------------------------------------------------

/// 一次能力查询的原始输入：驱动自报 + 实测。
#[derive(Clone, Debug)]
pub struct CapQueryInput {
    pub slot: String,
    pub driver_version: String,
    /// 驱动自报的标准位图（未校准）
    pub claimed: StandardBitmap,
    /// 实测位图（探测微基准的结果）
    pub measured: StandardBitmap,
    /// 厂商扩展（隔离域）
    pub vendor: VendorExtensions,
}

/// 校准结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calibration {
    /// 修正后的标准位图（以实测为准）
    pub corrected: StandardBitmap,
    /// 虚报位（自报有、实测无）
    pub inflated: Vec<&'static str>,
    /// 漏报位（自报无、实测有——驱动谦虚）
    pub underreported: Vec<&'static str>,
    /// 审计记录（每条带三要素）
    pub audit: Vec<String>,
}

/// 实测校准（判据：误报支持→实测校准）。
///
/// 修正纪律：位图以实测为准——自报有实测无的位清除（虚报不放行），
/// 自报无实测有的位保留（实测是硬证据）。每条修正带审计三要素。
pub fn calibrate(input: &CapQueryInput) -> Calibration {
    let mut c = StandardBitmap::default();
    let mut inflated: Vec<&'static str> = Vec::new();
    let mut under: Vec<&'static str> = Vec::new();
    let mut audit: Vec<String> = Vec::new();
    for key in FEATURE_KEYS.iter() {
        let claimed = input.claimed.get(key);
        let measured = input.measured.get(key);
        let final_on = measured.unwrap_or(false);
        if final_on {
            let _ = c.set(key, true);
        }
        if claimed == Some(true) && measured == Some(false) {
            inflated.push(key);
            audit.push(format!(
                "特性 {} 自报支持实测不支持：判虚报，已按实测清除。\
虚报会把假能力卖给上层，上层一用就翻车；下一步：驱动版本指纹入册，\
同版本不再信自报",
                key
            ));
        } else if claimed == Some(false) && measured == Some(true) {
            under.push(key);
            audit.push(format!(
                "特性 {} 自报不支持实测支持：按实测保留（实测是硬证据）；\
下一步：以实测位图驱动分级降级表",
                key
            ));
        }
    }
    Calibration {
        corrected: c,
        inflated,
        underreported: under,
        audit,
    }
}

// ---------------------------------------------------------------------------
// 三、能力分级与降级路径（判据：分级降级 + 效果损失量化）
// ---------------------------------------------------------------------------

/// 支持档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    /// 完整支持
    Full,
    /// 部分支持（降级路径生效）
    Partial,
    /// 不支持（降级路径必走）
    None,
}

/// 单个特性的分级条目：档位 + 预登记降级路径 + 效果损失量化。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grading {
    pub level: Support,
    /// 降级路径（None 档必填；None 值 = 登记缺失，构建期缺陷）
    pub degrade_path: Option<&'static str>,
    /// 效果损失量化 0.0（无损失）~1.0（效果全失）——不支持的特性
    /// 必须能回答"降级后画质差多少"
    pub effect_loss: f32,
}

impl Grading {
    pub fn entry_valid(&self) -> bool {
        match self.level {
            Support::Full => self.degrade_path.is_none() && self.effect_loss == 0.0,
            Support::Partial | Support::None => {
                self.degrade_path.is_some() && self.effect_loss >= 0.0 && self.effect_loss <= 1.0
            }
        }
    }
}

/// 按校准后位图生成分级表（每个标准特性一条，缺一即缺陷）。
pub fn grade(bitmap: &StandardBitmap) -> Vec<(&'static str, Grading)> {
    FEATURE_KEYS
        .iter()
        .map(|key| {
            let on = bitmap.get(key).unwrap_or(false);
            let (level, path, loss): (Support, Option<&'static str>, f32) = match *key {
                "raster2d" => (
                    Support::Full,
                    None,
                    0.0,
                ),
                "raster3d" => (Support::Full, None, 0.0),
                "compute" => (Support::Full, None, 0.0),
                "hdr" => (
                    if on { Support::Full } else { Support::None },
                    if on {
                        None
                    } else {
                        Some("SDR 色域映射（PQ/HLG 转 sRGB）")
                    },
                    if on { 0.0 } else { 0.6 }, // HDR→SDR：亮度/色域损失六成
                ),
                "async_compute" => (
                    if on { Support::Full } else { Support::None },
                    if on {
                        None
                    } else {
                        Some("同步串行执行（异步任务排队等图形管线空闲）")
                    },
                    if on { 0.0 } else { 0.3 }, // 并行度损失：延迟类效果打七折
                ),
                "video_decode" => (
                    if on { Support::Full } else { Support::None },
                    if on {
                        None
                    } else {
                        Some("CPU 软解回退（联动 CGPU-A 域 SIMD 解码）")
                    },
                    if on { 0.0 } else { 0.1 }, // 画质无损，功耗/占用损失
                ),
                "video_encode" => (
                    if on { Support::Full } else { Support::None },
                    if on {
                        None
                    } else {
                        Some("CPU 软编回退（推流/录屏降档）")
                    },
                    if on { 0.0 } else { 0.2 },
                ),
                "color_mgmt" => (
                    if on { Support::Full } else { Support::None },
                    if on {
                        None
                    } else {
                        Some("sRGB 直通（P3/Rec2020 压缩到 sRGB）")
                    },
                    if on { 0.0 } else { 0.5 }, // 广色域压缩损失
                ),
                "tessellation" => (
                    if on { Support::Full } else { Support::None },
                    if on {
                        None
                    } else {
                        Some("LOD 网格近似（细分曲面转静态网格）")
                    },
                    if on { 0.0 } else { 0.4 },
                ),
                "ray_trace" => (
                    if on { Support::Full } else { Support::None },
                    if on {
                        None
                    } else {
                        Some("屏幕空间反射/探针近似（光线追踪效果转栅格化）")
                    },
                    if on { 0.0 } else { 0.7 }, // RT→SSR：反射保真损失七成
                ),
                "mesh_shader" => (
                    if on { Support::Full } else { Support::None },
                    if on {
                        None
                    } else {
                        Some("传统顶点管线回退")
                    },
                    if on { 0.0 } else { 0.15 },
                ),
                "tensor" => (
                    if on { Support::Full } else { Support::None },
                    if on {
                        None
                    } else {
                        Some("上行采样/降噪走计算着色器近似")
                    },
                    if on { 0.0 } else { 0.35 },
                ),
                _ => (Support::None, Some("未知特性缺省降级"), 1.0),
            };
            (*key, Grading {
                level,
                degrade_path: path,
                effect_loss: loss,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 四、查询缓存与驱动更新联动（判据：缓存失效）
// ---------------------------------------------------------------------------

/// 缓存条目。
#[derive(Clone, Debug, PartialEq)]
pub struct CacheEntry {
    pub slot: String,
    pub driver_version: String,
    pub bitmap: StandardBitmap,
    /// 命中计数（可观测）
    pub hits: u64,
}

/// 查询缓存：按 (slot, driver_version) 缓存校准后位图。
///
/// 失效纪律：驱动一换位图重查——缓存键含驱动版本，版本不同即 miss；
/// 另有显式失效口（探测引擎重跑时全清）。
pub struct CapCache {
    entries: Vec<CacheEntry>,
    pub misses: u64,
    pub hits_total: u64,
    pub invalidations: u64,
}

impl CapCache {
    pub fn new() -> CapCache {
        CapCache {
            entries: Vec::new(),
            misses: 0,
            hits_total: 0,
            invalidations: 0,
        }
    }

    /// 查缓存。驱动版本不同 ⇒ miss（驱动更新联动失效，判据点名）。
    pub fn lookup(&mut self, slot: &str, driver_version: &str) -> Option<StandardBitmap> {
        match self
            .entries
            .iter_mut()
            .find(|e| e.slot == slot && e.driver_version == driver_version)
        {
            Some(e) => {
                e.hits += 1;
                self.hits_total += 1;
                Some(e.bitmap)
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// 写缓存（同 slot 旧驱动版本条目保留——别的版本还在用就别删）。
    pub fn store(&mut self, slot: &str, driver_version: &str, bitmap: StandardBitmap) {
        if self
            .entries
            .iter()
            .any(|e| e.slot == slot && e.driver_version == driver_version)
        {
            return; // 幂等
        }
        self.entries.push(CacheEntry {
            slot: slot.to_string(),
            driver_version: driver_version.to_string(),
            bitmap,
            hits: 0,
        });
    }

    /// 显式失效（探测引擎重跑/设备热插时全清）。
    pub fn invalidate_all(&mut self) {
        self.invalidations += self.entries.len() as u64;
        self.entries.clear();
    }

    /// 驱动更新联动失效：换驱动版本后旧条目仍在（其他版本可查），
    /// 但新版本查询必 miss——此处断言语义成立的机检入口。
    pub fn driver_update_requery_semantics(&mut self, slot: &str, old: &str, new: &str) -> bool {
        let old_hit = self.lookup(slot, old).is_some();
        let new_miss = self.lookup(slot, new).is_none();
        old_hit && new_miss
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 五、统一出口：查询全流程（自报+实测 → 校准 → 缓存 → 分级）
// ---------------------------------------------------------------------------

/// 查询结论（V01 能力协商复用的对接口）。
#[derive(Clone, Debug)]
pub struct CapReport {
    /// 校准后的标准位图（缓存与分级的唯一事实源）
    pub bitmap: StandardBitmap,
    /// 厂商扩展（隔离透传）
    pub vendor: VendorExtensions,
    pub calibration: Calibration,
    /// 分级降级表（每标准特性一条）
    pub grading: Vec<(&'static str, Grading)>,
    /// 本次是否缓存命中
    pub cache_hit: bool,
    /// 读屏可达摘要
    pub a11y: String,
}

/// 能力查询全流程。缓存命中直接回位图（带命中标记）；miss 走校准并存缓存。
pub fn query(cache: &mut CapCache, input: &CapQueryInput) -> CapReport {
    let (bitmap, cache_hit) = match cache.lookup(&input.slot, &input.driver_version) {
        Some(b) => (b, true),
        None => {
            let cal = calibrate(input);
            cache.store(&input.slot, &input.driver_version, cal.corrected);
            (cal.corrected, false)
        }
    };
    let calibration = if cache_hit {
        // 命中时校准审计不重算（缓存的就是校准后的），给空审计
        Calibration {
            corrected: bitmap,
            inflated: Vec::new(),
            underreported: Vec::new(),
            audit: Vec::new(),
        }
    } else {
        calibrate(input)
    };
    let grading = grade(&bitmap);
    let a11y = format!(
        "GPU 能力查询（{}，驱动 {}）：{}。厂商扩展隔离在位 {}..64（vid 0x{:04X}）。\
降级路径全部预登记，效果损失已量化",
        input.slot,
        input.driver_version,
        bitmap.describe(),
        VENDOR_BITS_BASE,
        input.vendor.vid
    );
    CapReport {
        bitmap,
        vendor: input.vendor,
        calibration,
        grading,
        cache_hit,
        a11y,
    }
}

/// 位图 → V01/VE-F0001 能力表（(&str, f32) 对，0.0/1.0 离散）。
/// 跨批对接点：V01 能力协商与 AdapterProbeInput.caps 直接消费本表。
pub fn to_v01_caps(bitmap: &StandardBitmap) -> Vec<(&'static str, f32)> {
    FEATURE_KEYS
        .iter()
        .map(|k| (*k, if bitmap.get(k) == Some(true) { 1.0 } else { 0.0 }))
        .collect()
}
