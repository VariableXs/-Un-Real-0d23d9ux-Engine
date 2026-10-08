//! VE-F1623 · 顶点拉取优化（VE-I 域 · I02 顶点流水线组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1623`
//!
//! **判据（锚点原文）**：命中优化、双模式、统计、度量、判据。
//!
//! **职责定位（锚点原文）**：顶点缓存命中（Forsyth 重排的运行时落地——
//! I01 F1609 的执行侧：重排后的网格进管线，命中率实测——GPU 顶点缓存的
//! 模拟验证）；实例数据布局（交错 AoS / 分离 SoA 两模式可配——场景自适应，
//! F1526 纪律）；拉取统计（命中率实时可见——F1630 调试数据消费）；
//! 收益度量（重排前后帧耗时对比入册）。
//!
//! # 一、重排的**运行时**落地：数据侧算法换成执行侧端口
//!
//! F1609 提供了 Forsyth 重排算法（数据侧），F1616 用它做过基准——但基准
//! 不等于运行时：管线的网格入口如果不调它，算法就只是账面上的存在。
//! [`prefetch_reorder`] 是执行侧端口：网格进管线前重排一次，之后每帧拉取
//! 都吃这份收益。端口**不复制算法**（委托 [`meshbatch::forsyth_reorder`]），
//! 与 F1618 冻结面的薄层纪律同源。
//!
//! # 二、命中率实测：LRU 模拟器是验收的量尺
//!
//! 「优化了」没有量尺就是自夸。[`simulate_hit_rate`] 用确定性 LRU 队列
//! 模拟 GPU 顶点缓存：按面序逐面请求 3 顶点，命中计一次、未命中计入并
//! 装载。模拟器纯函数、无共享态、整数运算——F1619 确定性纪律在量尺上的
//! 延续：量尺本身不确定，测量就是玄学。
//!
//! # 三、实例布局双模式：AoS/SoA 是**场景自适应**不是站队
//!
//! 交错（AoS）单实例全属性相邻——逐实例渲染局部性好；分离（SoA）按属性
//! 成列——批量按属性处理的流式访问友好（F1526 纪律）。两模式可配
//! （[`InstanceLayout`]），**同数据两布局语义等价**（判据逐字段对拍），
//! 切换布局不改内容——布局是拉取策略，不是数据变形。
//!
//! # 四、收益度量入册：操作计数口径（无墙钟确定性）
//!
//! 帧耗时无墙钟不可测，但重排前后命中率与拉取操作计数可测且确定
//! （F1616 模式）。[`gain_bench`] 跑 before/after 两遍同语料，命中率差
//! 与操作计数差入册（[`GainRecord`]）——收益是可复核的数字，不是感觉。
//!
//! ## 错误契约：独占 0x43 细分段（vef25 数学域占用 0x42）
//!
//! 零 panic 面：表驱动 + `Option`/`Result`，无 `unwrap`/`expect`。

use alloc::vec;
use alloc::vec::Vec;

use crate::gfx::meshbatch::{forsyth_reorder, BatchMesh};

// ===========================================================================
// 一、版本与诊断码（vef26 独占 0x43xx 段）
// ===========================================================================

/// 版本标识（家族格式）。
pub const VFETCH_VERSION: &str = "I02-vfetch-v1";

/// 顶点缓存模拟的默认缓存大小（GPU 顶点缓存常见规格的保守近似）。
pub const DEFAULT_CACHE_SIZE: usize = 16;

/// 拉取域诊断码（独占段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VfCode(pub u16);

impl VfCode {
    /// 布局未登记（查询了未声明的布局模式）。
    pub const LAYOUT_UNKNOWN: VfCode = VfCode(0x4301);
    /// 实例数据形状不符（AoS/SoA 数据长度不一致）。
    pub const SHAPE_MISMATCH: VfCode = VfCode(0x4302);
    /// 缓存大小非法（0 缓存无法模拟）。
    pub const CACHE_SIZE_INVALID: VfCode = VfCode(0x4303);

    /// 短码。
    pub const fn code(self) -> u16 {
        self.0
    }
}

// ===========================================================================
// 二、运行时落地：重排端口（判据一：命中优化）
// ===========================================================================

/// 管线入口重排：网格进管线前做一次 Forsyth 重排（F1609 执行侧端口）。
///
/// 委托 [`forsyth_reorder`]，返回重映射后的网格（可直接进顶点管线）。
/// 重排是**一次性**成本：进管线的网格都已重排，每帧拉取吃现成收益。
pub fn prefetch_reorder(m: &BatchMesh, cache_size: usize) -> Option<BatchMesh> {
    if cache_size == 0 {
        return None;
    }
    let r = forsyth_reorder(m, cache_size);
    Some(BatchMesh {
        positions: r.positions,
        faces: r.faces,
        materials: m.materials.clone(),
    })
}

// ===========================================================================
// 三、LRU 命中率模拟（量尺——确定性纪律）
// ===========================================================================

/// 一次模拟的统计产物。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FetchStats {
    /// 顶点请求总数（3×面数）。
    pub requests: u64,
    /// 命中数。
    pub hits: u64,
    /// 未命中数（=装载数）。
    pub misses: u64,
}

impl FetchStats {
    /// 命中率（千分比；0 请求恒 0——防除零）。
    pub fn hit_rate_permille(&self) -> u32 {
        if self.requests == 0 {
            return 0;
        }
        ((self.hits * 1000) / self.requests) as u32
    }
}

/// LRU 顶点缓存命中率模拟（确定性：同输入同结果）。
///
/// 按面序逐面请求 3 顶点：命中计入 hits，未命中计入 misses 并装载；
/// 缓存满时淘汰最久未用项。纯整数运算，无共享态。
pub fn simulate_hit_rate(faces: &[[u32; 3]], cache_size: usize) -> Result<FetchStats, VfCode> {
    if cache_size == 0 {
        return Err(VfCode::CACHE_SIZE_INVALID);
    }
    let mut cache: Vec<u32> = Vec::with_capacity(cache_size);
    let mut stats = FetchStats { requests: 0, hits: 0, misses: 0 };
    let mut f = 0usize;
    while f < faces.len() {
        let mut k = 0usize;
        while k < 3 {
            let v = faces[f][k];
            stats.requests += 1;
            // 命中检查 + LRU 触碰（移到队尾=最近使用）。
            let mut hit_pos: Option<usize> = None;
            let mut i = 0usize;
            while i < cache.len() {
                if cache[i] == v {
                    hit_pos = Some(i);
                }
                i += 1;
            }
            match hit_pos {
                Some(pos) => {
                    stats.hits += 1;
                    let touched = cache.remove(pos);
                    cache.push(touched);
                }
                None => {
                    stats.misses += 1;
                    if cache.len() >= cache_size {
                        cache.remove(0);
                    }
                    cache.push(v);
                }
            }
            k += 1;
        }
        f += 1;
    }
    Ok(stats)
}

// ===========================================================================
// 四、实例数据布局双模式（判据二：AoS / SoA 可配）
// ===========================================================================

/// 实例布局模式（可配；语义等价，拉取策略不同）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstanceLayout {
    /// 交错：单实例全属性相邻（逐实例渲染局部性好）。
    Interleaved,
    /// 分离：属性成列（按属性批处理流式友好，F1526 纪律）。
    Separated,
}

/// 实例布局全集（判据防枚举被裁）。
pub const LAYOUT_ALL: [InstanceLayout; 2] = [InstanceLayout::Interleaved, InstanceLayout::Separated];

/// 每实例属性字节数（演示口径：pos 12B + normal 12B + color 4B）。
pub const INSTANCE_ATTR_BYTES: usize = 28;

/// 布局下每实例的**拉取跨度**（AoS=全属性一次跨度；SoA=逐属性三次跨度）。
pub fn fetch_spans(layout: InstanceLayout) -> usize {
    match layout {
        InstanceLayout::Interleaved => 1,
        InstanceLayout::Separated => 3,
    }
}

/// 双布局数据（同内容两种存储形态——语义等价是判据的钉点）。
pub struct InstancePair {
    /// 实例数。
    pub count: usize,
    /// AoS：每实例 [pos_x, pos_y, pos_z, normal_x, normal_y, normal_z, color]。
    pub interleaved: Vec<[f32; 7]>,
    /// SoA：pos / normal / color 三列。
    pub separated_pos: Vec<[f32; 3]>,
    pub separated_normal: Vec<[f32; 3]>,
    pub separated_color: Vec<f32>,
}

impl InstancePair {
    /// 构造双布局同内容语料。
    pub fn build(count: usize) -> InstancePair {
        let mut p = InstancePair {
            count,
            interleaved: Vec::with_capacity(count),
            separated_pos: Vec::with_capacity(count),
            separated_normal: Vec::with_capacity(count),
            separated_color: Vec::with_capacity(count),
        };
        let mut i = 0usize;
        while i < count {
            let fi = i as f32;
            let pos = [fi, fi * 2.0, fi * 3.0];
            let nor = [0.0, 1.0, 0.0];
            let col = fi;
            p.interleaved.push([pos[0], pos[1], pos[2], nor[0], nor[1], nor[2], col]);
            p.separated_pos.push(pos);
            p.separated_normal.push(nor);
            p.separated_color.push(col);
            i += 1;
        }
        p
    }

    /// 语义等价检查：两布局逐实例逐属性相等（布局切换不改内容）。
    pub fn semantically_equal(&self) -> bool {
        if self.interleaved.len() != self.count
            || self.separated_pos.len() != self.count
            || self.separated_normal.len() != self.count
            || self.separated_color.len() != self.count
        {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            let a = self.interleaved[i];
            let p = self.separated_pos[i];
            let n = self.separated_normal[i];
            if a[0] != p[0] || a[1] != p[1] || a[2] != p[2] {
                return false;
            }
            if a[3] != n[0] || a[4] != n[1] || a[5] != n[2] {
                return false;
            }
            if a[6] != self.separated_color[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ===========================================================================
// 五、拉取统计实时快照（判据三：F1630 调试数据消费声明）
// ===========================================================================

/// 拉取统计账本（管线每帧累加；快照供调试面板实时可见）。
pub struct FetchLedger {
    stats: FetchStats,
}

impl FetchLedger {
    /// 空账本。
    pub fn new() -> FetchLedger {
        FetchLedger { stats: FetchStats { requests: 0, hits: 0, misses: 0 } }
    }

    /// 累加一次模拟/实测的统计。
    pub fn accumulate(&mut self, s: &FetchStats) {
        self.stats.requests += s.requests;
        self.stats.hits += s.hits;
        self.stats.misses += s.misses;
    }

    /// 实时快照（F1630 调试数据消费点：面板逐帧取此快照显示）。
    pub fn snapshot(&self) -> FetchStats {
        self.stats
    }
}

// ===========================================================================
// 六、收益度量入册（判据四：重排前后对比，操作计数口径）
// ===========================================================================

/// 收益记录（重排前后同语料同缓存口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GainRecord {
    /// 重排前命中率（千分比）。
    pub before_permille: u32,
    /// 重排后命中率（千分比）。
    pub after_permille: u32,
    /// 重排前拉取跨度操作计数（AoS 口径：未命中×每实例拉取跨度）。
    pub ops_before: u64,
    /// 重排后拉取跨度操作计数。
    pub ops_after: u64,
    /// 缓存大小（口径声明——跨缓存大小不可直比，F1616 教训）。
    pub cache_size: usize,
}

/// 收益基准：同语料重排前后各跑一遍 LRU 模拟，命中率与操作计数入册。
///
/// 操作口径：每次未命中 = 一次缓存行装载 ≈ 1 次拉取跨度（AoS）。
/// 无墙钟环境下用**可测的确定性量**（命中率/未命中数）承载收益，
/// 不伪造毫秒数。
pub fn gain_bench(m: &BatchMesh, cache_size: usize) -> Result<GainRecord, VfCode> {
    if cache_size == 0 {
        return Err(VfCode::CACHE_SIZE_INVALID);
    }
    let before = simulate_hit_rate(&m.faces, cache_size)?;
    let reordered = prefetch_reorder(m, cache_size).ok_or(VfCode::CACHE_SIZE_INVALID)?;
    let after = simulate_hit_rate(&reordered.faces, cache_size)?;
    Ok(GainRecord {
        before_permille: before.hit_rate_permille(),
        after_permille: after.hit_rate_permille(),
        ops_before: before.misses,
        ops_after: after.misses,
        cache_size,
    })
}

/// 摘要行（面板/日志/读屏共用；F1630 调试数据同源）。
pub fn screen_line() -> String {
    format!(
        "{} cache={} layouts={} attr_bytes={}",
        VFETCH_VERSION,
        DEFAULT_CACHE_SIZE,
        LAYOUT_ALL.len(),
        INSTANCE_ATTR_BYTES,
    )
}
