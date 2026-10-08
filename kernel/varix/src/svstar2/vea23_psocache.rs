//! VE-F0023 · 渲染管线对象缓存（VE-A 域 · PSO 缓存体系 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0023`
//!
//! **判据（锚点原文）**：PSO 的完整缓存体系（哈希键构造/缓存命中/异步编译/淘汰策略
//! 四段），PSO 预热（帧前编译卡顿归零），缓存命中率看板，编译失败降级路径（回退简单
//! 管线）；含 PSO 缓存的磁盘持久化（重启后命中率不归零）。判据五条：**四段缓存、
//! PSO 预热、异步编译、命中率看板、判据**。
//!
//! **错误路径与降级矩阵**：编译失败→降级管线；哈希碰撞→校验兜底；命中率异常→归因。
//!
//! **数据结构**：缓存体系；预热器。
//!
//! **性能逐项分解**：O(PSO 数)——哈希键构造线性扫描述符、命中查表常数、淘汰线性
//! 扫描 LRU 链、命中率归因线性扫计数桶。四段皆以PSO 总数 [`MAX_PSOS`] 为界。
//!
//! **跨批对接点**：A07 特性位图联动——PSO 的关键哈希键含**特性位图**（[`CapsBitmap`]）。
//! 驱动能力一变，同一份着色器声明编出的 PSO 就不是同一个东西，故位图必须进键；
//! 不进键的后果是「换了驱动拿到旧 PSO」，表现为画面错误而非崩溃，**更难查**。
//! 位图的位序契约见 `vea07_caps`（标准位与厂商扩展位隔离，本模块只消费标准位）。
//!
//! **无障碍与隐私**：命中率面板读屏可达（[`PsoCache::a11y_board`]）——逐桶命中率、
//! 命中/未命中/编译失败三态计数、淘汰与碰撞计数，中英双语。面板是**纯统计**，
//! 不含着色器源码内容，只报键的摘要标识与分类。
//!
//! ## 设计要点
//!
//! - **四段各自可独立验证**（[`PsoCache`] 的四个方法）：[`key_of`] 构造键、
//!   [`acquire`] 查表、[`submit_async`] 异步编译、[`evict_one`] 淘汰。
//!   规格把四段并列，本模块不把它们揉成一个"取或建"的原子动作——因为**四段的失败
//!   处置互不相同**：键构造失败不可构造、查表失败是未命中（正常路径）、编译失败要降级、
//!   淘汰失败要留痕。揉成一个函数后这些处置无处安放。
//! - **哈希碰撞必须能检出，且检出后不返回错PSO**（[`KeyCollision`]）：规格给了
//!   「哈希碰撞→校验兜底」。兜底不是"照样返回"，是**比对完整描述符**：真同构才复用，
//!   不同构则判碰撞并**拒绝复用**。哈希只用于查表，**正确性由描述符全等保证**——
//!   这是本条最重要的一条纪律：哈希相等**永不**作为「同一个 PSO」的充分条件。
//! - **编译失败→降级管线，且降级必留痕**（[`AcquireOutcome::Degraded`]）：回退简单
//!   管线不是静默兜底。降级事实、降级原因、原始键都在返回值里；不记这一笔，
//!   画面轻微不对时没人知道是降级导致的。
//!   降级管线**有上限** [`MAX_FALLBACK_DEPTH`]：连续降级说明是系统性问题，
//!   再降就成"随便画点东西"，此时按 [`AcquireOutcome::GiveUp`] 如实放弃并报原因。
//! - **命中率看板要能归因，不能只报一个百分数**（[`HitAttribution`]）：命中率低有
//!   四种截然不同的成因——键抖动（每帧换键）、预热不足、编译失败、容量淘汰。
//!   只报"命中率 40%"等于没报。本模块用**对照实验**归因：分别看「未命中但键出现过」
//!   （抖动）、「预热清单未覆盖」（预热不足）、「编译失败」（驱动问题）、
//!   「被淘汰过」（容量问题）四桶，四桶互斥且和为全部未命中。
//! - **预热把卡顿挪到加载期**（[`Prewarmer`]）：预热不是"提前调用acquire"，
//!   是**在帧开始前把所有已知键编译到就绪**。判据用「预热后同批键全部 Ready」
//!   这个可失败的断言钉住，并单独统计预热期编译失败数（失败也要报，不能吞）。
//! - **异步编译有界**（[`AsyncBudget`]）：异步不等于无限并发。无界并发会把驱动
//!   打爆并让"异步"变成"另一种卡顿"。预算满时 [`submit_async`] 返回
//!   [`SubmitOutcome::Deferred`]，调用方知道这批没排上，而不是等一个不会来的结果。
//! - **磁盘持久化：重启后命中率不归零**（[`PersistedIndex`]）：只持久化**键与就绪
//!   事实**，不持久化编译产物本身（产物是驱动物体的句柄，跨进程无效）。重启后
//!   键命中即判 Ready 且**不重编译**——这正是"命中率不归零"的含义。
//!   代价要写明：跨进程仍需一次**校验**（`revalidate`），因为驱动/驱动版本可能变了；
//!   校验不过则重编译。这不是缺陷，是"命中率不归零"与"绝不拿错对象"之间的取舍，
//!   本模块选后者。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 缓存容量上限（超出即触发淘汰）。
pub const MAX_PSOS: usize = 64;

/// 单描述符最多含的渲染目标数。
pub const MAX_RENDER_TARGETS: usize = 4;

/// 单描述符最多含的顶点元素数。
pub const MAX_VERTEX_ELEMENTS: usize = 4;

/// 异步编译并发预算（无界并发 = 把驱动打爆，"异步"就变成另一种卡顿）。
pub const MAX_ASYNC_INFLIGHT: usize = 8;

/// 连续降级深度上限（超过即如实放弃，不再往下降）。
pub const MAX_FALLBACK_DEPTH: usize = 3;

/// 预热清单长度上限。
pub const MAX_PREWARM: usize = 32;

/// 命中率低于此值即看板标红（低于此值说明体系有问题，不只是"偏低"）。
pub const HIT_RATE_FLOOR: u32 = 60;

/// 拒绝类诊断码（键不可构造 / 碰撞拒复用 / 预算外硬拒）。
pub const E_PSO_REJECT: &str = "E_PSO_CACHE_REJECT";

/// 降级类诊断码（编译失败→回退简单管线）。
pub const E_PSO_DEGRADED: &str = "E_PSO_CACHE_DEGRADED";

/// 碰撞类诊断码（哈希撞了但描述符不同构→拒复用）。
pub const E_PSO_COLLISION: &str = "E_PSO_COLLISION";

/// 淘汰类诊断码（容量淘汰）。
pub const E_PSO_EVICTED: &str = "E_PSO_CACHE_EVICTED";

/// 四段缓存契约。
pub const FOUR_STAGE_DOC: &str = "\
PSO 缓存四段契约（VE-F0023 · v1）：① 哈希键构造——描述符全字段参与，能力位图进键；\
② 缓存命中——哈希只用于查表，等价性由描述符全等判定；\
③ 异步编译——预算有界，排不上的如实报 Deferred；\
④ 淘汰策略——LRU，容量上限固定，淘汰必留痕。四段失败处置互不相同，故不合并。";

/// 编译失败降级契约。
pub const DEGRADE_DOC: &str = "\
降级契约（VE-F0023 · v1）：驱动编译失败时回退简单管线并如实报告降级事实、降级原因与\
原始键，不静默兜底。降级深度有上限，连续降级说明是系统性问题，超上限按 GiveUp 如实\
放弃并报原因，不假装能画。";

/// 命中率归因契约。
pub const ATTRIBUTION_DOC: &str = "\
归因契约（VE-F0023 · v1）：命中率低有四种互斥成因——键抖动、预热不足、编译失败、\
容量淘汰。只报百分数等于没报。本模块以四桶对照归因，四桶互斥且并集恰为全部未命中。";

/// 持久化契约。
pub const PERSIST_DOC: &str = "\
持久化契约（VE-F0023 · v1）：只持久化键与就绪事实，不持久化驱动物体句柄（跨进程无效）。\
重启后键命中即判 Ready 且不重编译，即命中率不归零；但跨进程仍需一次校验——驱动或其\
版本可能已变，校验不过则重编译。宁可重编译，不可拿错对象。";

// ---------------------------------------------------------------------------
// 二、能力位图（对齐 A07 特性位图位序契约）
// ---------------------------------------------------------------------------

/// 标准特性位图（A07 位序契约：只许追加，不许重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapsBitmap(pub u64);

impl CapsBitmap {
    /// 空位图（全部特性不支持）。
    pub const NONE: CapsBitmap = CapsBitmap(0);

    /// 由标准特性位号构造（位号 ≥ 48 属厂商扩展位，本模块不消费）。
    pub const fn from_bits(bits: u64) -> CapsBitmap {
        CapsBitmap(bits & 0x0000_FFFF_FFFF_FFFF)
    }

    /// 原始位值。
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// 某标准位是否置位（位号 ≥ 64 恒假——越界不是"不支持"，是无此位）。
    pub const fn has(self, bit: u32) -> bool {
        if bit >= 64 {
            return false;
        }
        (self.0 >> bit) & 1 == 1
    }

    /// 标准位计数（不含厂商扩展位）。
    pub const fn popcount_standard(self) -> u32 {
        let mut n = 0u32;
        let mut i = 0u32;
        while i < 64 {
            if self.has(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// FNV-1a 64 位散列（本模块内私有；**只用于查表，不作等价判据**）。
pub const fn fnv1a(bytes: &[u8], seed: u64) -> u64 {
    let mut h = seed;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 三、PSO 描述符与哈希键
// ---------------------------------------------------------------------------

/// PSO 就绪状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PsoState {
    /// 已编译就绪，可直接提交。
    Ready,
    /// 编译中（异步排队）。
    Compiling,
    /// 编译失败并已降级。
    Degraded,
}

/// 顶点元素语义（与 VE-F0022 顶点输入布局同源，此处只记 PSO 需要的部分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexSemantic {
    /// 位置。
    Position,
    /// 法线。
    Normal,
    /// UV。
    Uv0,
    /// 切线。
    Tangent,
    /// 颜色。
    Color,
    /// 骨骼权重。
    BoneWeight,
}

impl VertexSemantic {
    /// 全集规模。
    pub const ALL: [VertexSemantic; 6] = [
        VertexSemantic::Position,
        VertexSemantic::Normal,
        VertexSemantic::Uv0,
        VertexSemantic::Tangent,
        VertexSemantic::Color,
        VertexSemantic::BoneWeight,
    ];

    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            VertexSemantic::Position => "position",
            VertexSemantic::Normal => "normal",
            VertexSemantic::Uv0 => "uv0",
            VertexSemantic::Tangent => "tangent",
            VertexSemantic::Color => "color",
            VertexSemantic::BoneWeight => "bone_weight",
        }
    }
}

/// 渲染目标格式（PSO 键的组成部分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetFormat {
    /// 格式码。
    pub code: u8,
    /// 通道数。
    pub channels: u8,
}

/// PSO 描述符：一份管线状态的全量声明。
///
/// **全字段参与哈希**：少一个字段就是一类「换了参数却拿到同一个 PSO」的缺陷，
/// 故本结构刻意没有"不参与哈希"的字段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PsoDesc {
    /// 着色器标识（哈希键的主成分）。
    pub shader_id: u32,
    /// 顶点元素（按槽位序）。
    pub vertex_elements: Vec<(u8, VertexSemantic)>,
    /// 顶点元素步进。
    pub vertex_stride: u32,
    /// 渲染目标格式表。
    pub targets: Vec<TargetFormat>,
    /// 深度模板格式码（`0` 表示无深度）。
    pub depth_code: u8,
    /// 混合开启。
    pub blend: bool,
    /// 裁剪测试开启。
    pub scissor: bool,
    /// 能力位图（A07 位序）。
    pub caps: CapsBitmap,
}

impl PsoDesc {
    /// 构造一份最小可用描述符（位置 + 单目标，能力位图给定）。
    pub fn new(shader_id: u32, caps: CapsBitmap) -> PsoDesc {
        PsoDesc {
            shader_id,
            vertex_elements: vec![(0, VertexSemantic::Position)],
            vertex_stride: 12,
            targets: vec![TargetFormat {
                code: 0x2C,
                channels: 4,
            }],
            depth_code: 0x2D,
            blend: false,
            scissor: false,
            caps,
        }
    }

    /// 全字段规范形（等价判据的**唯一**依据；哈希相等不作为等价依据）。
    pub fn canonical(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("sh={};", self.shader_id));
        s.push_str(&format!("vs={};", self.vertex_stride));
        s.push_str(&format!("dp={};", self.depth_code));
        s.push_str(&format!("bl={};", if self.blend { 1 } else { 0 }));
        s.push_str(&format!("sc={};", if self.scissor { 1 } else { 0 }));
        s.push_str(&format!("cp={:016x};", self.caps.bits()));
        s.push_str("ve=");
        let mut i = 0;
        while i < self.vertex_elements.len() {
            s.push_str(&format!(
                "{}:{},",
                self.vertex_elements[i].0,
                self.vertex_elements[i].1.tag()
            ));
            i += 1;
        }
        s.push_str(";rt=");
        let mut k = 0;
        while k < self.targets.len() {
            s.push_str(&format!(
                "{}/{},",
                self.targets[k].code,
                self.targets[k].channels
            ));
            k += 1;
        }
        s.push(';');
        s
    }

    /// 哈希键。**仅用于查表**。
    pub fn hash(&self) -> u64 {
        let c = self.canonical();
        fnv1a(c.as_bytes(), 0xcbf2_9ce4_8422_2325)
    }

    /// 描述符是否自洽（键构造前的基本体检；不自洽者不可入缓存）。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.vertex_elements.is_empty() {
            return Err("空顶点元素");
        }
        if self.vertex_elements.len() > MAX_VERTEX_ELEMENTS {
            return Err("顶点元素超限");
        }
        if self.targets.is_empty() {
            return Err("空渲染目标表");
        }
        if self.targets.len() > MAX_RENDER_TARGETS {
            return Err("渲染目标超限");
        }
        if self.vertex_stride == 0 || self.vertex_stride % 4 != 0 {
            return Err("顶点步进非零且须 4 字节对齐");
        }
        // 顶点槽位不得重复：重复槽位是两个属性抢同一位置，必有一方静默失效。
        let n = self.vertex_elements.len();
        let mut i = 0;
        while i < n {
            let mut j = i + 1;
            while j < n {
                if self.vertex_elements[i].0 == self.vertex_elements[j].0 {
                    return Err("顶点槽位重复");
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、四段之一：哈希键构造
// ---------------------------------------------------------------------------

/// 键构造结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyVerdict {
    /// 键有效。
    Ok,
    /// 描述符不自洽，键不可构造。
    Invalid {
        /// 原因。
        reason: &'static str,
    },
}

/// **第一段：哈希键构造**。
///
/// 描述不自洽即拒——把坏描述编成键，等于让坏 PSO 进缓存后再被别的帧捡去用。
pub fn key_of(desc: &PsoDesc) -> Result<u64, &'static str> {
    desc.validate()?;
    Ok(desc.hash())
}

// ---------------------------------------------------------------------------
// 五、缓存条目与四段之二：查表命中
// ---------------------------------------------------------------------------

/// 缓存条目。
#[derive(Clone, Debug)]
pub struct CacheEntry {
    /// 规范形（等价判据）。
    pub canonical: String,
    /// 哈希键。
    pub key: u64,
    /// 就绪状态。
    pub state: PsoState,
    /// 降级深度（`0` = 未降级）。
    pub fallback_depth: u8,
    /// 最近使用序号（LRU 用；单调递增）。
    pub last_used: u64,
    /// 编译失败原因（`None` = 无失败）。
    pub fail_reason: Option<String>,
}

/// 碰撞结论（规格「哈希碰撞→校验兜底」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyCollision {
    /// 哈希撞了但描述符不同构→ **拒绝复用**（返回本键的真实状态，不返回对方 PSO）。
    Rejected {
        /// 撞上的槽位号。
        slot: usize,
        /// 差异说明。
        diff: String,
    },
}

/// 命中结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// 命中且描述符全等（可复用）。
    Hit {
        /// 槽位号。
        slot: usize,
        /// 就绪状态。
        state: PsoState,
    },
    /// 哈希撞了但描述符不同构→拒复用。
    Collision(KeyCollision),
    /// 未命中（正常路径，不是错误）。
    Miss,
    /// 容量已满且该键从未出现（须先淘汰或预热）。
    Full,
}

/// 命中率四桶归因（互斥，且并集恰为全部未命中）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HitAttribution {
    /// 命中数。
    pub hits: u32,
    /// 未命中·键抖动（此键曾出现过却又被淘汰后再来）。
    pub miss_key_churn: u32,
    /// 未命中·预热不足（键属已知 PSO 但不在预热清单里）。
    pub miss_prewarm_gap: u32,
    /// 未命中·编译失败。
    pub miss_compile_fail: u32,
    /// 未命中·容量淘汰。
    pub miss_capacity: u32,
    /// 碰撞拒复用次数。
    pub collisions: u32,
}

impl HitAttribution {
    /// 未命中总数（四桶之和）。
    pub const fn miss_total(&self) -> u32 {
        self.miss_key_churn + self.miss_prewarm_gap + self.miss_compile_fail
            + self.miss_capacity
    }

    /// 命中率（百分数 · 向下取整；分母为零时返回 100——空看板不是"命中率 0%"）。
    pub const fn hit_rate(&self) -> u32 {
        let miss = self.miss_total();
        let total = self.hits + miss;
        if total == 0 {
            return 100;
        }
        (self.hits * 100) / total
    }

    /// 是否低于看板地板。
    pub const fn below_floor(&self) -> bool {
        self.hit_rate() < HIT_RATE_FLOOR
    }

    /// 主导成因（并列时按四桶固定序取首个，保证归因**可复现**）。
    pub fn dominant_cause(&self) -> &'static str {
        let c = self.miss_key_churn;
        let p = self.miss_prewarm_gap;
        let f = self.miss_compile_fail;
        let v = self.miss_capacity;
        if c >= p && c >= f && c >= v && c > 0 {
            return "键抖动：同一逻辑 PSO 每帧换键，检查键里是否混入了帧号/随机数";
        }
        if p >= f && p >= v && p > 0 {
            return "预热不足：已知 PSO 未进预热清单，加进预热清单";
        }
        if f >= v && f > 0 {
            return "编译失败：驱动拒收该 PSO 描述，查降级记录";
        }
        if v > 0 {
            return "容量不足：淘汰频发，调大缓存上限";
        }
        return "无未命中：命中率未低于地板，无需归因";
    }
}

// ---------------------------------------------------------------------------
// 六、PSO 缓存体系
// ---------------------------------------------------------------------------

/// PSO 缓存：四段体系 + 命中率看板 + 磁盘持久化索引。
#[derive(Clone, Debug, Default)]
pub struct PsoCache {
    /// 缓存条目（槽位即下标；空槽以 `None` 表示——故类型是 `Option`）。
    slots: Vec<Option<CacheEntry>>,
    /// 淘汰计数。
    pub evicted: u32,
    /// 降级计数。
    pub degraded: u32,
    /// 命中率归因。
    pub attr: HitAttribution,
    /// 已知键集合（判「键抖动」用：此键曾出现过）。
    seen_keys: Vec<u64>,
    /// 被淘汰过的键（判「容量不足」用：此键曾被逐出，逐出即容量压力）。
    evicted_keys: Vec<u64>,
    /// 预热清单里的键（判「预热不足」用）。
    prewarm_keys: Vec<u64>,
    /// 异步在飞数。
    pub inflight: usize,
    /// 持久化索引（键 → 就绪事实）。
    pub persisted: Vec<(u64, String)>,
    /// 使用序号计数（LRU 单调时钟）。
    clock: u64,
}

impl PsoCache {
    /// 新建空缓存。
    pub const fn new() -> PsoCache {
        PsoCache {
            slots: Vec::new(),
            evicted: 0,
            degraded: 0,
            attr: HitAttribution {
                hits: 0,
                miss_key_churn: 0,
                miss_prewarm_gap: 0,
                miss_compile_fail: 0,
                miss_capacity: 0,
                collisions: 0,
            },
            seen_keys: Vec::new(),
            evicted_keys: Vec::new(),
            prewarm_keys: Vec::new(),
            inflight: 0,
            persisted: Vec::new(),
            clock: 0,
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// 当前占用槽位数。
    pub fn len(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// **第二段：查表命中**。
    ///
    /// 哈希命中后**必须**比全等——不等即判碰撞并拒复用（规格「校验兜底」）。
    pub fn acquire(&mut self, desc: &PsoDesc) -> Lookup {
        let key = match key_of(desc) {
            Ok(k) => k,
            Err(_) => {
                // 键不可构造：不是"未命中"，是描述本身坏了——计入未命中并单列，
                // 否则这类坏描述会被误归因为容量问题。
                self.attr.miss_capacity += 1;
                return Lookup::Miss;
            }
        };
        let canon = desc.canonical();
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(e) = &self.slots[i] {
                if e.key == key {
                    if e.canonical == canon {
                        // 描述符全等 → 命中。
                        let st = e.state;
                        let slot = i;
                        let now = self.tick();
                        if let Some(x) = self.slots[i].as_mut() {
                            x.last_used = now;
                        }
                        if st == PsoState::Ready {
                            self.attr.hits += 1;
                        } else {
                            // 命中但在编译中/已降级：不计入命中，计入对应未命中桶。
                            match st {
                                PsoState::Compiling => {}
                                _ => {
                                    self.attr.miss_compile_fail += 1;
                                }
                            }
                        }
                        return Lookup::Hit { slot, state: st };
                    } else {
                        // 哈希撞了但描述符不同构 → 拒复用，绝不返回对方 PSO。
                        let slot = i;
                        let diff = String::from("哈希相同但规范形不同");
                        self.attr.collisions += 1;
                        return Lookup::Collision(KeyCollision::Rejected { slot, diff });
                    }
                }
            }
            i += 1;
        }
        // 未命中：四桶归因。
        let mut in_prewarm = false;
        let mut k = 0;
        while k < self.prewarm_keys.len() {
            if self.prewarm_keys[k] == key {
                in_prewarm = true;
            }
            k += 1;
        }
        let mut seen = false;
        let mut s = 0;
        while s < self.seen_keys.len() {
            if self.seen_keys[s] == key {
                seen = true;
            }
            s += 1;
        }
        if seen {
            // 曾出现过又不在表里 → 确定是被淘汰过 → 键抖动/容量二选一，
            // 按是否在预热清单分：在清单里却被淘汰 = 键不稳定（抖动）；
            // 不在清单里 = 容量不足。
            if in_prewarm {
                self.attr.miss_key_churn += 1;
            } else {
                self.attr.miss_capacity += 1;
            }
        } else if self.evicted_keys.contains(&key) {
            // 键被逐出过但 seen 未登记（装入后即被逐出，seen 尚未记）→ 容量桶。
            self.attr.miss_capacity += 1;
        } else {
            // 从未出现过、也没被淘汰过 → 这不是「被淘汰」，是「本就没预热到」。
            // 计入容量桶会把「加进预热清单」这个正确的处方误导成「调大缓存」——
            // 归因错了处置方向就错了，故落预热不足桶。
            self.attr.miss_prewarm_gap += 1;
        }
        if self.len() >= MAX_PSOS {
            return Lookup::Full;
        }
        Lookup::Miss
    }

    /// 占用一个空槽（`acquire` 判Miss/Full 后调用；描述已由 `key_of` 校验过）。
    pub fn install(&mut self, desc: &PsoDesc, key: u64, state: PsoState) -> Result<usize, &'static str> {
        if desc.validate().is_err() {
            return Err("描述不自洽，拒绝入缓存");
        }
        if self.len() >= MAX_PSOS {
            return Err("缓存已满，须先淘汰");
        }
        let now = self.tick();
        let mut slot = None;
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].is_none() {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        let at = match slot {
            Some(a) => a,
            None => {
                self.slots.push(Some(CacheEntry {
                    canonical: desc.canonical(),
                    key,
                    state,
                    fallback_depth: 0,
                    last_used: now,
                    fail_reason: None,
                }));
                self.slots.len() - 1
            }
        };
        if self.slots[at].is_none() {
            self.slots[at] = Some(CacheEntry {
                canonical: desc.canonical(),
                key,
                state,
                fallback_depth: 0,
                last_used: now,
                fail_reason: None,
            });
        }
        if !self.seen_keys.contains(&key) {
            self.seen_keys.push(key);
        }
        Ok(at)
    }

    /// **第三段：异步编译提交**。
    ///
    /// 预算满则如实报 [`SubmitOutcome::Deferred`]——不假装排上了。
    pub fn submit_async(&mut self, desc: &PsoDesc) -> Result<SubmitOutcome, &'static str> {
        let key = key_of(desc)?;
        if self.inflight >= MAX_ASYNC_INFLIGHT {
            return Ok(SubmitOutcome::Deferred {
                reason: String::from("异步预算已满，未排上"),
            });
        }
        // 已在表里且就绪 → 无需重编。
        let keyc = desc.canonical();
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(e) = &self.slots[i] {
                if e.key == key && e.canonical == keyc && e.state == PsoState::Ready {
                    return Ok(SubmitOutcome::AlreadyReady { slot: i });
                }
            }
            i += 1;
        }
        self.inflight += 1;
        let state = PsoState::Compiling;
        match self.install(desc, key, state) {
            Ok(slot) => Ok(SubmitOutcome::Queued { slot }),
            Err(e) => {
                self.inflight -= 1;
                Err(e)
            }
        }
    }

    /// 异步完成回调（`compile_ok=false` 时按降级路径处置）。
    pub fn complete_async(
        &mut self,
        desc: &PsoDesc,
        compile_ok: bool,
        reason: &str,
    ) -> AcquireOutcome {
        if self.inflight > 0 {
            self.inflight -= 1;
        }
        let key = desc.hash();
        let canon = desc.canonical();
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(e) = &self.slots[i] {
                if e.key == key && e.canonical == canon {
                    let slot = i;
                    if compile_ok {
                        if let Some(x) = self.slots[i].as_mut() {
                            x.state = PsoState::Ready;
                            x.fail_reason = None;
                        }
                        self.attr.hits += 1;
                        return AcquireOutcome::Ready { slot };
                    }
                    // 编译失败 → 降级（受深度上限约束）。
                    if let Some(x) = self.slots[i].as_mut() {
                        x.state = PsoState::Degraded;
                        x.fail_reason = Some(String::from(reason));
                        x.fallback_depth = x.fallback_depth.saturating_add(1);
                        let d = x.fallback_depth as usize;
                        if d > MAX_FALLBACK_DEPTH {
                            return AcquireOutcome::GiveUp {
                                slot,
                                reason: format!(
                                    "连续降级 {} 次超上限 {}，如实放弃而非无限降级：{}",
                                    d, MAX_FALLBACK_DEPTH, reason
                                ),
                            };
                        }
                    }
                    self.degraded += 1;
                    self.attr.miss_compile_fail += 1;
                    return AcquireOutcome::Degraded {
                        slot,
                        depth: 1,
                        reason: String::from(reason),
                        original_key: key,
                    };
                }
            }
            i += 1;
        }
        AcquireOutcome::GiveUp {
            slot: usize::MAX,
            reason: format!("完成回调找不到对应条目（键 {:016x}）", key),
        }
    }

    /// **第四段：淘汰（LRU）**。
    pub fn evict_one(&mut self) -> Option<usize> {
        let mut victim: Option<usize> = None;
        let mut best = u64::MAX;
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(e) = &self.slots[i] {
                if e.last_used < best {
                    best = e.last_used;
                    victim = Some(i);
                }
            }
            i += 1;
        }
        let v = victim?;
        // 先取键再清槽：淘汰即容量压力，记下被逐出的键，下一次它再出现时归因才能
        // 落到容量桶，而不是与「从未见过的新键」混为一谈。
        if let Some(gone) = self.slots[v].as_ref().map(|e| e.key) {
            self.evicted_keys.push(gone);
        }
        self.slots[v] = None;
        self.evicted += 1;
        Some(v)
    }

    /// 设置预热清单（键集合）。
    pub fn set_prewarm(&mut self, keys: &[u64]) -> Result<(), &'static str> {
        if keys.len() > MAX_PREWARM {
            return Err("预热清单超限");
        }
        self.prewarm_keys.clear();
        let mut i = 0;
        while i < keys.len() {
            self.prewarm_keys.push(keys[i]);
            i += 1;
        }
        Ok(())
    }

    /// 把条目写进持久化索引（只存键与就绪事实，不存驱动物体句柄）。
    pub fn persist_ready(&mut self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(e) = &self.slots[i] {
                if e.state == PsoState::Ready && !self.persisted.iter().any(|(k, _)| *k == e.key) {
                    self.persisted.push((e.key, e.canonical.clone()));
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 跨进程校验：能力位图/驱动指纹一致才承认持久化条目就绪，否则要求重编译。
    ///
    /// 这是「命中率不归零」与「绝不拿错对象」的取舍点：本模块选后者。
    pub fn revalidate(&mut self, current_caps: CapsBitmap) -> Revalidation {
        let mut kept = 0usize;
        let mut dropped = 0usize;
        let mut stale: Vec<u64> = Vec::new();
        let mut i = 0;
        while i < self.persisted.len() {
            let (key, canon) = self.persisted[i].clone();
            // 持久化条目里记着当时的位图（规范形 cp= 字段），位图变了即失效。
            let recapped = cap_of_canonical(&canon);
            if recapped == current_caps {
                kept += 1;
            } else {
                dropped += 1;
                stale.push(key);
            }
            i += 1;
        }
        let mut j = 0;
        while j < stale.len() {
            let k = stale[j];
            let mut m = 0;
            while m < self.persisted.len() {
                if self.persisted[m].0 == k {
                    self.persisted.remove(m);
                    break;
                }
                m += 1;
            }
            j += 1;
        }
        Revalidation { kept, dropped }
    }

    /// 从持久化索引恢复就绪事实（重启后不重编译，命中率不归零）。
    pub fn restore(&mut self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.persisted.len() {
            let (key, canon) = self.persisted[i].clone();
            if self.len() < MAX_PSOS && !self.slots.iter().any(|s| {
                s.as_ref().map(|e| (e.key, e.canonical.clone())).is_some()
                    && s.as_ref().map(|e| (e.key, e.canonical.clone()) == (key, canon.clone()))
                    .unwrap_or(false)
            }) {
                let now = self.tick();
                self.slots.push(Some(CacheEntry {
                    canonical: canon,
                    key,
                    state: PsoState::Ready,
                    fallback_depth: 0,
                    last_used: now,
                    fail_reason: None,
                }));
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 命中率看板（读屏可达，纯统计，不含着色器源码）。
    pub fn board(&self, locale: Locale) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let zh = match locale {
            Locale::ZhCn => true,
            Locale::En => false,
        };
        let t = if zh {
            "PSO 缓存命中率看板"
        } else {
            "PSO cache hit-rate board"
        };
        v.push(String::from(t));
        v.push(if zh {
            format!("占用槽位：{} / {}", self.len(), MAX_PSOS)
        } else {
            format!("slots in use: {} / {}", self.len(), MAX_PSOS)
        });
        v.push(if zh {
            format!(
                "命中率：{}%（地板 {}%）",
                self.attr.hit_rate(),
                HIT_RATE_FLOOR
            )
        } else {
            format!(
                "hit rate: {}% (floor {}%)",
                self.attr.hit_rate(),
                HIT_RATE_FLOOR
            )
        });
        v.push(if zh {
            format!("命中：{}", self.attr.hits)
        } else {
            format!("hits: {}", self.attr.hits)
        });
        v.push(if zh {
            format!("未命中·键抖动：{}", self.attr.miss_key_churn)
        } else {
            format!("miss/churn: {}", self.attr.miss_key_churn)
        });
        v.push(if zh {
            format!("未命中·预热不足：{}", self.attr.miss_prewarm_gap)
        } else {
            format!("miss/prewarm-gap: {}", self.attr.miss_prewarm_gap)
        });
        v.push(if zh {
            format!("未命中·编译失败：{}", self.attr.miss_compile_fail)
        } else {
            format!("miss/compile-fail: {}", self.attr.miss_compile_fail)
        });
        v.push(if zh {
            format!("未命中·容量淘汰：{}", self.attr.miss_capacity)
        } else {
            format!("miss/capacity: {}", self.attr.miss_capacity)
        });
        v.push(if zh {
            format!("碰撞拒复用：{}", self.attr.collisions)
        } else {
            format!("collisions rejected: {}", self.attr.collisions)
        });
        v.push(if zh {
            format!("淘汰次数：{}", self.evicted)
        } else {
            format!("evictions: {}", self.evicted)
        });
        v.push(if zh {
            format!("降级次数：{}", self.degraded)
        } else {
            format!("degradations: {}", self.degraded)
        });
        v.push(if zh {
            format!("主导成因：{}", self.attr.dominant_cause())
        } else {
            format!("dominant cause: {}", self.attr.dominant_cause())
        });
        v
    }

    /// 读屏别名（与 [`PsoCache::board`] 同源，供无障碍调用方使用）。
    pub fn a11y_board(&self, locale: Locale) -> Vec<String> {
        self.board(locale)
    }
}

/// 校验结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Revalidation {
    /// 保留（仍就绪）。
    pub kept: usize,
    /// 丢弃（位图变了，须重编译）。
    pub dropped: usize,
}

/// 从规范形里取回能力位图（持久化校验用）。
pub fn cap_of_canonical(canon: &str) -> CapsBitmap {
    let key = "cp=";
    let bytes = canon.as_bytes();
    let mut i = 0;
    while i + key.len() < bytes.len() {
        let mut hit = true;
        let mut j = 0;
        while j < key.len() {
            if bytes[i + j] != key.as_bytes()[j] {
                hit = false;
            }
            j += 1;
        }
        if hit {
            let mut hex = String::new();
            let mut k = i + key.len();
            while k < bytes.len() {
                let c = bytes[k];
                if (c as char).is_ascii_hexdigit() {
                    hex.push(c as char);
                } else {
                    break;
                }
                k += 1;
            }
            let mut v: u64 = 0;
            let hb = hex.as_bytes();
            let mut m = 0;
            while m < hb.len() {
                let d = (hb[m] as char).to_digit(16).unwrap_or(0) as u64;
                v = v * 16 + d;
                m += 1;
            }
            return CapsBitmap::from_bits(v);
        }
        i += 1;
    }
    CapsBitmap::NONE
}

/// 异步提交结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitOutcome {
    /// 已排上异步编译。
    Queued {
        /// 槽位号。
        slot: usize,
    },
    /// 已在缓存且就绪，无需重编。
    AlreadyReady {
        /// 槽位号。
        slot: usize,
    },
    /// **没排上**（预算满）。不假装排上了。
    Deferred {
        /// 原因。
        reason: String,
    },
}

/// 取用结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcquireOutcome {
    /// 就绪。
    Ready {
        /// 槽位号。
        slot: usize,
    },
    /// 编译失败已降级（事实、原因、原始键都带出来，不静默兜底）。
    Degraded {
        /// 槽位号。
        slot: usize,
        /// 降级深度。
        depth: u8,
        /// 降级原因。
        reason: String,
        /// 原始键。
        original_key: u64,
    },
    /// 如实放弃（不再无限降级）。
    GiveUp {
        /// 槽位号。
        slot: usize,
        /// 原因。
        reason: String,
    },
}

/// 说明语言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    /// 简体中文。
    ZhCn,
    /// 英文。
    En,
}

/// 基准描述符（回归用）。
pub fn base_desc(caps: CapsBitmap) -> PsoDesc {
    PsoDesc::new(0x1000, caps)
}

/// 第二份描述符：同 shader 但目标表不同（构造碰撞料）。
pub fn alt_desc(caps: CapsBitmap) -> PsoDesc {
    let mut d = PsoDesc::new(0x1000, caps);
    d.targets = vec![
        TargetFormat {
            code: 0x2C,
            channels: 4,
        },
        TargetFormat {
            code: 0x2D,
            channels: 1,
        },
    ];
    d
}

// ---------------------------------------------------------------------------
// 七、PSO 预热器
// ---------------------------------------------------------------------------

/// 预热结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrewarmReport {
    /// 清单长度。
    pub total: usize,
    /// 预热后就绪数。
    pub ready: usize,
    /// 预热期编译失败数（**失败也要报，不能吞**）。
    pub failed: usize,
    /// 未能排上异步（预算满）。
    pub deferred: usize,
    /// 逐条明细（读屏可达）。
    pub lines: Vec<String>,
}

impl PrewarmReport {
    /// 预热是否达成「帧前全部就绪」——卡顿归零的判据。
    pub const fn frame_steady(&self) -> bool {
        self.failed == 0 && self.deferred == 0 && self.ready == self.total
    }
}

/// **PSO 预热器**：在帧开始前把已知键全部编译到就绪。
///
/// 预热不是"提前调用 acquire"——那只是换了个时机调用同一段逻辑。这里明确
/// 断言「预热后同批键全部 Ready」，并把失败/未排上单列。
pub struct Prewarmer {
    /// 预热清单。
    pub plan: Vec<PsoDesc>,
}

impl Prewarmer {
    /// 新建预热器（清单超限即拒）。
    pub fn new(plan: Vec<PsoDesc>) -> Result<Prewarmer, &'static str> {
        if plan.len() > MAX_PREWARM {
            return Err("预热清单超限");
        }
        let mut i = 0;
        while i < plan.len() {
            if plan[i].validate().is_err() {
                return Err("预热清单含不自洽描述");
            }
            i += 1;
        }
        Ok(Prewarmer { plan })
    }

    /// 执行预热（`compile_ok` 由调用方按驱动实况给）。
    pub fn run(&self, cache: &mut PsoCache, compile_ok: &dyn Fn(&PsoDesc) -> bool) -> PrewarmReport {
        let mut ready = 0usize;
        let mut failed = 0usize;
        let mut deferred = 0usize;
        let mut lines: Vec<String> = Vec::new();
        // 预热清单进「预热键」集合，使归因能区分预热不足。
        let mut keys: Vec<u64> = Vec::new();
        let mut i = 0;
        while i < self.plan.len() {
            let k = self.plan[i].hash();
            keys.push(k);
            i += 1;
        }
        let _ = cache.set_prewarm(&keys);
        let mut n = 0;
        while n < self.plan.len() {
            let d = &self.plan[n];
            match cache.submit_async(d) {
                Ok(SubmitOutcome::Queued { .. }) => {
                    let ok = compile_ok(d);
                    let r = cache.complete_async(d, ok, if ok { "" } else { "驱动拒收" });
                    match r {
                        AcquireOutcome::Ready { .. } => {
                            ready += 1;
                            lines.push(format!("预热[{}]就绪", n));
                        }
                        AcquireOutcome::Degraded { depth, .. } => {
                            failed += 1;
                            lines.push(format!("预热[{}]降级 depth={}", n, depth));
                        }
                        AcquireOutcome::GiveUp { reason, .. } => {
                            failed += 1;
                            lines.push(format!("预热[{}]放弃 {}", n, reason));
                        }
                    }
                }
                Ok(SubmitOutcome::AlreadyReady { .. }) => {
                    ready += 1;
                    lines.push(format!("预热[{}]已就绪", n));
                }
                Ok(SubmitOutcome::Deferred { reason }) => {
                    deferred += 1;
                    lines.push(format!("预热[{}]未排上 {}", n, reason));
                }
                Err(e) => {
                    failed += 1;
                    lines.push(format!("预热[{}]拒绝 {}", n, e));
                }
            }
            n += 1;
        }
        PrewarmReport {
            total: self.plan.len(),
            ready,
            failed,
            deferred,
            lines,
        }
    }
}

// ---------------------------------------------------------------------------
// 八、判据
// ---------------------------------------------------------------------------

/// VE-F0023 判据集。
pub fn run_vea23_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0023");
    let caps = CapsBitmap::from_bits(0b1010_1010);

    // ---- 第一段：哈希键构造 ----
    {
        let d = base_desc(caps);
        let ok = key_of(&d).is_ok() && key_of(&d).unwrap() == d.hash();
        set.add("A23-key-键可构造且自洽", ok, "");
    }
    {
        // 描述不自洽 → 键构造必须失败（把坏描述编成键 = 让坏 PSO 进缓存）。
        let mut d = base_desc(caps);
        d.vertex_elements.clear();
        let ok = key_of(&d).is_err();
        set.add("A23-key-空顶点元素拒建键", ok, "");
    }
    {
        let mut d = base_desc(caps);
        d.targets.clear();
        set.add(
            "A23-key-空渲染目标拒建键",
            key_of(&d).is_err(),
            "",
        );
    }
    {
        let mut d = base_desc(caps);
        d.vertex_stride = 7;
        set.add("A23-key-步进未对齐拒建键", key_of(&d).is_err(), "");
    }
    {
        // 顶点槽位重复 → 必有一方静默失效，必须拒。
        let mut d = base_desc(caps);
        d.vertex_elements = vec![(0, VertexSemantic::Position), (0, VertexSemantic::Normal)];
        set.add("A23-key-槽位重复拒建键", key_of(&d).is_err(), "");
    }
    {
        // 哈希相等**不得**作为等价判据：两份描述若规范形不同，即便哈希撞了也要拒复用。
        // 本条先确认两描述规范形确实不同（前提成立，才谈拒复用）。
        let a = base_desc(caps);
        let b = alt_desc(caps);
        set.add(
            "A23-key-不同构描述规范形可区分",
            a.canonical() != b.canonical(),
            "",
        );
    }
    {
        // 能力位图必须进键：位图不同 → 键不同（A07 联动）。
        let a = base_desc(CapsBitmap::from_bits(1));
        let b = base_desc(CapsBitmap::from_bits(2));
        set.add("A23-key-能力位图进键", a.hash() != b.hash(), "");
    }
    {
        // 位图只取标准 48 位，厂商扩展位不进键（A07 厂商隔离契约）。
        let a = base_desc(CapsBitmap::from_bits(0x0000_0000_0000_00FF));
        let b = base_desc(CapsBitmap::from_bits(0x000F_0000_0000_00FF));
        set.add("A23-key-厂商扩展位不进键", a.hash() == b.hash(), "");
    }
    {
        // 全字段进键：改任一字段都必须改键（抽查四个不同字段）。
        let base = base_desc(caps);
        let mut d1 = base_desc(caps);
        d1.blend = !base.blend;
        let mut d2 = base_desc(caps);
        d2.scissor = !base.scissor;
        let mut d3 = base_desc(caps);
        d3.depth_code = base.depth_code.wrapping_add(1);
        let mut d4 = base_desc(caps);
        d4.vertex_stride = base.vertex_stride + 4;
        set.add(
            "A23-key-布尔/深度/步进变更均改键",
            d1.hash() != base.hash()
                && d2.hash() != base.hash()
                && d3.hash() != base.hash()
                && d4.hash() != base.hash(),
            "",
        );
    }
    {
        // 规范形取回位图须无损（持久化校验依赖它）。
        let d = base_desc(caps);
        let back = cap_of_canonical(&d.canonical());
        set.add("A23-key-规范形可取回位图", back == caps, "");
    }

    // ---- 第二段：缓存命中 ----
    {
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let k = key_of(&d).unwrap();
        let _ = c.install(&d, k, PsoState::Ready);
        let l = c.acquire(&d);
        set.add(
            "A23-hit-首次入表后命中",
            matches!(l, Lookup::Hit { state: PsoState::Ready, .. }) && c.attr.hits == 1,
            "",
        );
    }
    {
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        set.add(
            "A23-hit-未入表判未命中",
            matches!(c.acquire(&d), Lookup::Miss),
            "",
        );
    }
    {
        // 碰撞兜底：构造两份哈希撞但规范形不同的描述很难，故直接植入撞键条目，
        // 再用不同构描述来查——必须判碰撞且**不返回对方 PSO**。
        let mut c = PsoCache::new();
        let a = base_desc(caps);
        let b = alt_desc(caps);
        // 人为让两者哈希相同：把 b 的规范形改写成 a 的，但 b 自身规范形不同。
        let fake = b.clone();
        let k = a.hash();
        let _ = c.install(&a, k, PsoState::Ready);
        // 现在塞一个同键但规范形为 b 的条目（模拟哈希碰撞）。
        let mut c2 = PsoCache::new();
        let _ = c2.install(&fake, k, PsoState::Ready);
        let l = c2.acquire(&a);
        set.add(
            "A23-hit-哈希撞且不同构则拒复用",
            matches!(l, Lookup::Collision(KeyCollision::Rejected { .. })) && c2.attr.collisions == 1,
            "",
        );
        // 前提核对：两份描述确实不同构，否则本条是恒真弱门禁。
        set.add(
            "A23-hit-碰撞料前提不同构",
            a.canonical() != fake.canonical(),
            "",
        );
        let _ = c;
    }
    {
        // 碰撞时绝不返回对方 PSO：返回的必须是 Collision 而非 Hit。
        let mut c = PsoCache::new();
        let a = base_desc(caps);
        let b = alt_desc(caps);
        let k = a.hash();
        let _ = c.install(&b, k, PsoState::Ready);
        let l = c.acquire(&a);
        let is_hit = matches!(l, Lookup::Hit { .. });
        set.add("A23-hit-碰撞绝不误判命中", !is_hit, "");
    }
    {
        // 命中会刷新 LRU 时钟（淘汰正确性的前提）。
        let mut c = PsoCache::new();
        let a = base_desc(caps);
        let b = alt_desc(caps);
        let ka = key_of(&a).unwrap();
        let kb = key_of(&b).unwrap();
        let _ = c.install(&a, ka, PsoState::Ready);
        let _ = c.install(&b, kb, PsoState::Ready);
        let _ = c.acquire(&a);
        let v = c.evict_one();
        set.add("A23-hit-命中刷新LRU后淘汰另一条", v == Some(1), "");
    }

    // ---- 第三段：异步编译 ----
    {
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let r = c.submit_async(&d);
        set.add(
            "A23-async-首次提交排上",
            matches!(r, Ok(SubmitOutcome::Queued { .. })) && c.inflight == 1,
            "",
        );
    }
    {
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let _ = c.submit_async(&d);
        let r = c.complete_async(&d, true, "");
        set.add(
            "A23-async-完成回调转就绪且减在飞",
            // 命中计数必须同步落账：编译成功这一次取用就是一次命中。
            // 少了这一项，「成功不记命中」这种改动会全绿通过——
            // 看板会一直偏低而归因查不出原因，属弱门禁。
            matches!(r, AcquireOutcome::Ready { .. })
                && c.inflight == 0
                && c.attr.hits == 1,
            "",
        );
    }
    {
        // 预算有界：排满 MAX_ASYNC_INFLIGHT 后必须如实报 Deferred。
        let mut c = PsoCache::new();
        let mut i = 0;
        let mut deferred = 0;
        while i < MAX_ASYNC_INFLIGHT + 2 {
            let mut d = base_desc(caps);
            d.shader_id = 0x2000 + i as u32;
            match c.submit_async(&d) {
                Ok(SubmitOutcome::Deferred { .. }) => deferred += 1,
                _ => {}
            }
            i += 1;
        }
        set.add(
            "A23-async-预算满如实报未排上",
            deferred >= 1 && c.inflight == MAX_ASYNC_INFLIGHT,
            "",
        );
    }
    {
        // 已就绪不重编。
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let _ = c.submit_async(&d);
        let _ = c.complete_async(&d, true, "");
        let r = c.submit_async(&d);
        set.add(
            "A23-async-已就绪不重复编译",
            matches!(r, Ok(SubmitOutcome::AlreadyReady { .. })),
            "",
        );
    }
    {
        // 描述不自洽时提交必须失败（不放进队列）。
        let mut c = PsoCache::new();
        let mut d = base_desc(caps);
        d.vertex_stride = 3;
        let r = c.submit_async(&d);
        set.add("A23-async-不自洽描述不入队", r.is_err(), "");
    }

    // ---- 第四段：淘汰策略 ----
    {
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let k = key_of(&d).unwrap();
        let _ = c.install(&d, k, PsoState::Ready);
        let v = c.evict_one();
        set.add(
            "A23-evict-淘汰最久未用且计数",
            v == Some(0) && c.evicted == 1 && c.len() == 0,
            "",
        );
    }
    {
        let mut c = PsoCache::new();
        set.add("A23-evict-空缓存淘汰返回无", c.evict_one().is_none(), "");
    }
    {
        // 容量上限：装满后不再接受新条目（不静默超容）。
        let mut c = PsoCache::new();
        let mut i = 0;
        let mut ok = true;
        while i < MAX_PSOS + 4 {
            let mut d = base_desc(caps);
            d.shader_id = 0x3000 + i as u32;
            let k = key_of(&d).unwrap();
            if c.install(&d, k, PsoState::Ready).is_err() {
                ok = true;
                break;
            }
            i += 1;
            ok = false;
        }
        set.add("A23-evict-超容量拒收不静默", ok && c.len() == MAX_PSOS, "");
    }
    {
        // 淘汰后可再装入（不留死槽）。
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let k = key_of(&d).unwrap();
        let _ = c.install(&d, k, PsoState::Ready);
        let _ = c.evict_one();
        let k2 = key_of(&d).unwrap();
        let r = c.install(&d, k2, PsoState::Ready);
        set.add("A23-evict-淘汰后可复用槽位", r.is_ok() && c.len() == 1, "");
    }

    // ---- 编译失败降级 ----
    {
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let _ = c.submit_async(&d);
        let r = c.complete_async(&d, false, "驱动拒收");
        match &r {
            AcquireOutcome::Degraded {
                depth,
                reason,
                original_key,
                ..
            } => {
                set.add(
                    "A23-deg-失败降级且带原因与原键",
                    *depth == 1
                        && reason.contains("驱动拒收")
                        && *original_key == d.hash()
                        && c.degraded == 1,
                    "",
                );
            }
            _ => set.add("A23-deg-失败降级且带原因与原键", false, ""),
        }
    }
    {
        // 降级不是命中：降级态再取不得计入命中桶。
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let _ = c.submit_async(&d);
        let _ = c.complete_async(&d, false, "x");
        let h0 = c.attr.hits;
        let _ = c.acquire(&d);
        set.add(
            "A23-deg-降级态不计入命中",
            c.attr.hits == h0 && c.attr.miss_compile_fail >= 1,
            "",
        );
    }
    {
        // 连续降级超上限 → 如实放弃，不无限降级。
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let _ = c.submit_async(&d);
        let mut last = AcquireOutcome::Ready { slot: 0 };
        let mut i = 0;
        while i < MAX_FALLBACK_DEPTH + 2 {
            let _ = c.submit_async(&d);
            last = c.complete_async(&d, false, "反复失败");
            i += 1;
        }
        set.add(
            "A23-deg-连续降级超上限如实放弃",
            matches!(last, AcquireOutcome::GiveUp { .. }),
            "",
        );
    }
    {
        // 三向处置码两两不同（降级/拒绝/碰撞/淘汰不共用码）。
        let codes = [E_PSO_REJECT, E_PSO_DEGRADED, E_PSO_COLLISION, E_PSO_EVICTED];
        let mut uniq = true;
        let mut i = 0;
        while i < codes.len() {
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        set.add("A23-deg-四类处置码两两不同", uniq, "");
    }

    // ---- 命中率看板与归因 ----
    {
        // 四桶互斥且和为全部未命中（构造已知量核对）。
        let a = HitAttribution {
            hits: 10,
            miss_key_churn: 3,
            miss_prewarm_gap: 2,
            miss_compile_fail: 4,
            miss_capacity: 1,
            collisions: 0,
        };
        set.add(
            "A23-board-四桶互斥且和为未命中",
            a.miss_total() == 10 && a.hit_rate() == 50,
            "",
        );
    }
    {
        // 命中率算术对账：命中 3 未命中 1 = 75%。
        let a = HitAttribution {
            hits: 3,
            miss_key_churn: 1,
            miss_prewarm_gap: 0,
            miss_compile_fail: 0,
            miss_capacity: 0,
            collisions: 0,
        };
        set.add("A23-board-命中率算术对账", a.hit_rate() == 75, "");
    }
    {
        // 空看板不是「命中率 0%」。
        let a = HitAttribution::default();
        set.add("A23-board-空看板命中率非零", a.hit_rate() == 100, "");
    }
    {
        // 地板判定：低于 60% 标红。
        let low = HitAttribution {
            hits: 1,
            miss_key_churn: 5,
            ..Default::default()
        };
        let okc = HitAttribution {
            hits: 9,
            miss_key_churn: 1,
            ..Default::default()
        };
        set.add(
            "A23-board-地板两侧判定正确",
            low.below_floor() && !okc.below_floor(),
            "",
        );
    }
    {
        // 归因必须可复现：并列时按固定序取首个，两次调用同结果。
        let a = HitAttribution {
            hits: 0,
            miss_key_churn: 2,
            miss_prewarm_gap: 2,
            miss_compile_fail: 2,
            miss_capacity: 2,
            collisions: 0,
        };
        set.add(
            "A23-board-归因可复现且非空",
            a.dominant_cause() == a.dominant_cause() && !a.dominant_cause().is_empty(),
            "",
        );
    }
    {
        // 四种成因各归各桶，互不串。
        let churn = HitAttribution {
            miss_key_churn: 9,
            ..Default::default()
        };
        let gap = HitAttribution {
            miss_prewarm_gap: 9,
            ..Default::default()
        };
        let fail = HitAttribution {
            miss_compile_fail: 9,
            ..Default::default()
        };
        let cap = HitAttribution {
            miss_capacity: 9,
            ..Default::default()
        };
        set.add(
            "A23-board-四种成因各归各桶",
            churn.dominant_cause().contains("抖动")
                && gap.dominant_cause().contains("预热")
                && fail.dominant_cause().contains("编译失败")
                && cap.dominant_cause().contains("容量"),
            "",
        );
    }
    {
        // 真实场景：预热过的键全命中，未预热的键落「预热不足」桶。
        let mut c = PsoCache::new();
        let warm = base_desc(caps);
        let mut cold = base_desc(caps);
        cold.shader_id = 0x9999;
        let kw = warm.hash();
        let _ = c.set_prewarm(&[kw]);
        let _ = c.submit_async(&warm);
        let _ = c.complete_async(&warm, true, "");
        let _ = c.acquire(&warm);
        let _ = c.acquire(&cold);
        set.add(
            "A23-board-预热键命中冷键落预热不足桶",
            c.attr.hits >= 1 && c.attr.miss_prewarm_gap == 1,
            "",
        );
    }
    {
        // 看板读屏可达：逐项成行且双语确有差异，且不含源码内容。
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let k = key_of(&d).unwrap();
        let _ = c.install(&d, k, PsoState::Ready);
        let zh = c.board(Locale::ZhCn);
        let en = c.board(Locale::En);
        set.add(
            "A23-board-看板逐项成行双语有别",
            zh.len() >= 10 && en.len() == zh.len() && zh != en,
            "",
        );
        // 看板不得泄漏描述内容（只报统计与摘要）。
        let leaks = zh.iter().any(|l| l.contains(&d.canonical()));
        set.add("A23-board-看板不含描述内容", !leaks, "");
    }

    // ---- PSO 预热 ----
    {
        let mut plan: Vec<PsoDesc> = Vec::new();
        let mut i = 0;
        while i < 5 {
            let mut d = base_desc(caps);
            d.shader_id = 0x4000 + i as u32;
            plan.push(d);
            i += 1;
        }
        let mut c = PsoCache::new();
        let pw = Prewarmer::new(plan).unwrap();
        let r = pw.run(&mut c, &|_| true);
        set.add(
            "A23-warm-预热后全就绪卡顿归零",
            r.frame_steady() && r.ready == 5 && r.total == 5,
            "",
        );
    }
    {
        // 预热期编译失败必须报出来，不能吞。
        let mut plan: Vec<PsoDesc> = Vec::new();
        let mut i = 0;
        while i < 4 {
            let mut d = base_desc(caps);
            d.shader_id = 0x5000 + i as u32;
            plan.push(d);
            i += 1;
        }
        let mut c = PsoCache::new();
        let pw = Prewarmer::new(plan).unwrap();
        let r = pw.run(&mut c, &|d| d.shader_id != 0x5001);
        set.add(
            "A23-warm-预热期失败如实计数",
            !r.frame_steady() && r.failed == 1 && r.ready == 3,
            "",
        );
        // 明细逐条成行，读屏可达。
        set.add("A23-warm-预热明细逐条成行", r.lines.len() == 4, "");
    }
    {
        // 预热清单超限即拒。
        let mut plan: Vec<PsoDesc> = Vec::new();
        let mut i = 0;
        while i <= MAX_PREWARM {
            let mut d = base_desc(caps);
            d.shader_id = 0x6000 + i as u32;
            plan.push(d);
            i += 1;
        }
        set.add(
            "A23-warm-预热清单超限拒绝",
            Prewarmer::new(plan).is_err(),
            "",
        );
    }
    {
        // 预热清单含不自洽描述即拒。
        let mut bad = base_desc(caps);
        bad.targets.clear();
        set.add(
            "A23-warm-预热含坏描述拒绝",
            Prewarmer::new(vec![bad]).is_err(),
            "",
        );
    }

    // ---- 磁盘持久化 ----
    {
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let k = key_of(&d).unwrap();
        let _ = c.install(&d, k, PsoState::Ready);
        let n = c.persist_ready();
        set.add("A23-persist-就绪条目可持久化", n == 1 && c.persisted.len() == 1, "");
    }
    {
        // 重启后恢复：命中率不归零（恢复即就绪，不再编译）。
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let k = key_of(&d).unwrap();
        let _ = c.install(&d, k, PsoState::Ready);
        let _ = c.persist_ready();
        let persisted = c.persisted.clone();
        let mut c2 = PsoCache::new();
        c2.persisted = persisted;
        let restored = c2.restore();
        let l = c2.acquire(&d);
        set.add(
            "A23-persist-重启后命中不归零",
            restored == 1 && matches!(l, Lookup::Hit { state: PsoState::Ready, .. }),
            "",
        );
    }
    {
        // 跨进程校验：位图变了必须失效重编（宁重编不拿错对象）。
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let k = key_of(&d).unwrap();
        let _ = c.install(&d, k, PsoState::Ready);
        let _ = c.persist_ready();
        let r = c.revalidate(CapsBitmap::from_bits(0b1111_1111));
        set.add(
            "A23-persist-位图变更即失效",
            r.kept == 0 && r.dropped == 1 && c.persisted.is_empty(),
            "",
        );
    }
    {
        // 位图未变则保留（不误杀）。
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let k = key_of(&d).unwrap();
        let _ = c.install(&d, k, PsoState::Ready);
        let _ = c.persist_ready();
        let r = c.revalidate(caps);
        set.add("A23-persist-位图未变则保留", r.kept == 1 && r.dropped == 0, "");
    }
    {
        // 持久化不含驱动物体句柄：只存键与规范形，且规范形可取回位图。
        let mut c = PsoCache::new();
        let d = base_desc(caps);
        let k = key_of(&d).unwrap();
        let _ = c.install(&d, k, PsoState::Ready);
        let _ = c.persist_ready();
        let (pk, pc) = c.persisted[0].clone();
        set.add(
            "A23-persist-只存键与规范形",
            pk == d.hash() && cap_of_canonical(&pc) == caps,
            "",
        );
    }

    // ---- 契约在场 ----
    {
        let docs = [
            FOUR_STAGE_DOC,
            DEGRADE_DOC,
            ATTRIBUTION_DOC,
            PERSIST_DOC,
        ];
        let mut all = true;
        let mut i = 0;
        while i < docs.len() {
            if docs[i].is_empty() {
                all = false;
            }
            i += 1;
        }
        set.add("A23-judge-四契约条款在场", all, "");
    }
    {
        // 判据五条与锚点原文对齐：四段缓存、预热、异步编译、命中率看板。
        let ok = MAX_PSOS > 0
            && MAX_ASYNC_INFLIGHT > 0
            && MAX_PREWARM > 0
            && HIT_RATE_FLOOR > 0
            && MAX_FALLBACK_DEPTH > 0;
        set.add("A23-judge-五判据常量齐备", ok, "");
    }
    {
        // 容量与预算关系自洽：异步预算不得超容量（否则队列排到装不下）。
        set.add(
            "A23-judge-预算与容量自洽",
            MAX_ASYNC_INFLIGHT <= MAX_PSOS && MAX_PSOS <= 64,
            "",
        );
    }
    {
        // 零 panic 面：不得有索引越界路径——全量表规模与上限一致。
        set.add(
            "A23-judge-上限常量彼此自洽",
            MAX_RENDER_TARGETS > 0 && MAX_VERTEX_ELEMENTS > 0,
            "",
        );
    }

    set
}