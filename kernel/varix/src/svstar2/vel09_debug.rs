//! VE-F2209 · 粒子调试数据（VE-L 域 · 粒子与物理域 · 批次 L01 第 9 项 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2209`
//!
//! **判据（锚点原文五条）**：双层统计、gizmo 五形状、力场预留、发行版零成本、判据。
//! 逐条落位：
//! - **双层统计**：池级（[`PoolStatsSnapshot`]，**直读** F2208
//!   [`ParticlePool`] 计数器——`live/water_pct/level` 都是池已维护的量，
//!   本模块**零额外计算**）与发射器级（[`EmitterCounter`]，事件驱动计数）
//!   两层各自独立记账，在 [`DebugStatsCenter::second_tick`] 汇成
//!   [`SecondSample`] 进**每秒聚合环形缓冲**（[`StatsRing`]，定容——调试流
//!   的价值在「最近」不在「全量」，与 F2408 值流同纪律）。
//! - **gizmo 五形状**：[`emitter_wireframe`] 按 F2203
//!   [`ShapeParams`] 五型（点/线/球/锥/网格表面）各自生成线框顶点，
//!   **形状单源**：形状类型、参数校验（`validate_shape`）、圆周逼近
//!   （`cos_approx`/`sin_approx`）全部复用 F2203，不另造第二份几何——
//!   两份几何迟早漂移，漂移后 gizmo 画出来的形状与实际发射域不一致，
//!   调试数据本身在撒谎，比没有调试数据更糟。
//! - **力场预留**：[`request_field_stream`] 是**显性报错**的 STUB
//!   （F1871 语义）：调用即记 [`DiagCode::FIELD_STUB_CALLED`] 并指路
//!   F2230 对接位，**不静默返回空数据**——静默空数据会让消费端把
//!   「没实现」误读成「没有力场」。
//! - **发行版零成本**：[`StripGuard`] 双形态（Debug 构建负载 /
//!   Release 零成本剔除），**零成本是可证伪的**：Release 下
//!   `payload_builds` 恒为 0——**包括被强行尝试的那次**（强行请求只记
//!   `forced_attempts` 与一条 P1）。判据双向验证：Debug 档计数器必须
//!   真的递增（对照组证明计数器不是死的）。
//! - **判据**：`vel09_checks.rs` 逐条映射，双向验证（基线绿 + 变体红）。
//!
//! **降级矩阵（锚点原文五条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 统计洪水（千级发射器）→ TopN+聚合降档 | [`DebugStatsCenter::topn`]：发射器数超 [`FLOOD_EMITTER_THRESHOLD`] 时只交付按在用数排序的 [`TOP_N_EMITTERS`] 条，其余**聚合成一个 others 桶**（总数守恒可对账），并记 [`DiagCode::STATS_FLOOD`] |
//! | gizmo 顶点超限 → LOD 线框（远距离简化） | [`lod_segments`] 距离四档降密 + [`emitter_wireframe_with_budget`] 预算内二分减密，超限记 [`DiagCode::GIZMO_VERTEX_OVERFLOW`]，`requested_vertices` 如实记抽稀前顶点数 |
//! | 力场预留被调用 → 显性报错 | [`request_field_stream`] 记 [`DiagCode::FIELD_STUB_CALLED`] 并递增调用计数，指路 F2230 |
//! | 发行版剔除失败 → P1（家族纪律） | [`StripGuard::try_build`] Release 档强行请求记 P1 [`DiagCode::STRIP_FAILED`] |
//! | 信封 schema 漂移 → 对账拦截 | [`LEnvelope::drifted`] FNV-1a 重算摘要比对；[`LEnvelopeRegistry::reconcile`] 有漂移即置 `intercepted`，`take()` 一律 `None`——**漂移的后果是拦截不是记一笔**，只记不拦等于把红线降级成日志 |
//!
//! **信封 L 段注册（F1764 负载族扩展）**：两类型注册（[`LPayloadKind::
//! ParticleStats`] / [`LPayloadKind::EmitterGizmo`]，线上码 0x01/0x02），
//! 力场预留线上码 [`FIELD_RESERVE_WIRE`] = 0x03 **故意不注册**——注册表
//! 对它返回拒绝并记 [`DiagCode::ENVELOPE_UNKNOWN_PAYLOAD`]，「预留」的
//! 语义是「位置留着、实现未到」，不是「可以当已实现用」。
//!
//! **统计洪水为什么用 TopN 而不是全量降采样**：千级发射器全量推送时，
//! 消费端（编辑器面板）渲染成本 O(N) 且人类根本读不完一千行；TopN 交付
//! 「最值得看的 64 个」（按在用粒子数排序，同数按 id 升序保序稳定），
//! 其余**聚合不丢弃**——others 桶的 `others_live` + TopN 之和恒等于
//! 全体之和，这条**守恒式**让降档可对账：只发 TopN 丢掉其余的话，
//! 「总量对不上」会让排查者在统计面与池面之间来回怀疑。
//!
//! **LOD 为什么距离与预算两级**：距离档（[`lod_segments`]）解决「远处
//! 不需要细线框」的常规情形；预算档（`with_budget`）解决「同屏 gizmo
//! 太多、总顶点数超渲染预算」的聚合情形。两级独立触发，都记诊断——
//! 抽稀若不记账，绘制端会把简化线框当真实形状（比如把球画成了十二面体
//! 还以为画的是球）。
//!
//! **与 F2208 的分工**：F2208 决定槽位与水位，本模块**只读**它的计数器
//! 做展示，**不反写**池一个字节；调试旁路若能改模拟状态，「开调试与关
//! 调试双跑不一致」这类确定性红线立刻失守（F2270 隔离纪律同理）。
//!
//! **与 F2408（M 域动画调试数据）的同族关系**：信封注册/漂移对账拦截/
//! StripGuard 零成本剔除三件套与 F2408 同构（F1764 负载族纪律），但
//! **独立实现、独立码段**（诊断码独占 0x2Exx；F2408 占 0x2Bxx）——
//! 同构是架构上的，不是共享代码：M 段信封槽位是四类型，L 段是两类型
//! +力场预留位，槽位数不同，强行抽公共泛型只会造出两个都别扭的类型。
//!
//! 零 panic 面、零 IO、零墙钟（「每秒」以 `second_tick` 调用为秒边界，
//! 由调用方的节拍器驱动）；无全局可变状态（所有状态由调用方持有）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::vel03_emitter::{
    cos_approx, is_finite, length_v3, sin_approx, validate_shape, ShapeKind, ShapeParams, Vec3,
};
use super::vel08_pool::{ParticlePool, PressureLevel};

// ---------------------------------------------------------------------------
// 一、诊断码（独占 0x2Exx 码段；0x2Bxx 归 F2408，0x2Cxx 归 F0220，0x2Dxx 归 F2607）
// ---------------------------------------------------------------------------

/// 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagCode(pub u16);

impl DiagCode {
    /// 统计洪水：发射器数超阈值，已 TopN 降档交付。
    pub const STATS_FLOOD: DiagCode = DiagCode(0x2E01);
    /// 秒聚合环形缓冲已覆盖最旧样本。
    pub const STATS_RING_OVERWRITE: DiagCode = DiagCode(0x2E02);
    /// gizmo 顶点超限，已 LOD 简化。
    pub const GIZMO_VERTEX_OVERFLOW: DiagCode = DiagCode(0x2E03);
    /// gizmo 形状参数非法，被拒（复用 F2203 校验单源）。
    pub const GIZMO_PARAM_REJECTED: DiagCode = DiagCode(0x2E04);
    /// 力场预留被调用（显性报错，对接位归 F2230）。
    pub const FIELD_STUB_CALLED: DiagCode = DiagCode(0x2E05);
    /// 信封漂移：重算摘要与注册时声明不符。
    pub const ENVELOPE_DRIFT: DiagCode = DiagCode(0x2E06);
    /// 信封负载类型未注册（含力场预留码 0x03——预留不等于已注册）。
    pub const ENVELOPE_UNKNOWN_PAYLOAD: DiagCode = DiagCode(0x2E07);
    /// 发行版构建下强行请求调试负载（剔除失败，P1）。
    pub const STRIP_FAILED: DiagCode = DiagCode(0x2E08);

    /// 人话标签（读屏可达）。
    pub const fn label(self) -> &'static str {
        match self {
            DiagCode::STATS_FLOOD => "统计洪水，已 TopN 降档",
            DiagCode::STATS_RING_OVERWRITE => "秒聚合环形缓冲已覆盖最旧样本",
            DiagCode::GIZMO_VERTEX_OVERFLOW => "gizmo 顶点超限，已 LOD 简化",
            DiagCode::GIZMO_PARAM_REJECTED => "gizmo 形状参数非法，已拒绝",
            DiagCode::FIELD_STUB_CALLED => "力场可视化预留位被调用，尚未实现",
            DiagCode::ENVELOPE_DRIFT => "信封漂移，已对账拦截",
            DiagCode::ENVELOPE_UNKNOWN_PAYLOAD => "信封负载类型未注册",
            DiagCode::STRIP_FAILED => "发行版强行请求调试负载",
            other => {
                let _ = other;
                "未登记诊断码"
            }
        }
    }

    /// 全部码（供家族完整性判据）。
    pub const ALL: [DiagCode; 8] = [
        DiagCode::STATS_FLOOD,
        DiagCode::STATS_RING_OVERWRITE,
        DiagCode::GIZMO_VERTEX_OVERFLOW,
        DiagCode::GIZMO_PARAM_REJECTED,
        DiagCode::FIELD_STUB_CALLED,
        DiagCode::ENVELOPE_DRIFT,
        DiagCode::ENVELOPE_UNKNOWN_PAYLOAD,
        DiagCode::STRIP_FAILED,
    ];
}

/// 诊断严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 记账，不阻断。
    Minor,
    /// 显性告警。
    Major,
    /// 立案：需要人看一眼（剔除失败/漂移拦截进这里）。
    P1,
}

/// 一条诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub severity: Severity,
}

/// 诊断袋（P1 是一等公民，与 F2408 同纪律）。
#[derive(Clone, Debug, Default)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    #[allow(clippy::new_without_default)]
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    pub fn push(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::Minor });
    }

    pub fn push_major(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::Major });
    }

    pub fn push_p1(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::P1 });
    }

    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 某码出现次数（独立计数，不合并——合并会被别的码掩护）。
    pub fn count(&self, code: DiagCode) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(d) = self.items.get(i) {
                if d.code == code {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn has(&self, code: DiagCode) -> bool {
        self.count(code) > 0
    }

    /// P1 条数。
    pub fn p1_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(d) = self.items.get(i) {
                if d.severity == Severity::P1 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 人话渲染（非空即「诊断面不是哑巴」的可机检证据）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(d) = self.items.get(i) {
                let _ = s.push_str(&format!("[{:?}/{}] {}\n", d.severity, d.code.0, d.code.label()));
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 二、池级统计直读（F2208 计数器 · 零额外计算）
// ---------------------------------------------------------------------------

/// 池级统计快照：**直读** F2208 池已维护的量，本模块不做二次统计。
///
/// 「直读」是性能契约（锚点：统计直读池计数器零额外计算）：水位/档位
/// 在池里本来就每帧维护，调试面若自己再算一遍，两套数迟早漂移，且
/// 排查时不知道该信谁——单一事实来源，调试面只是取数端。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolStatsSnapshot {
    /// 池内总粒子数（在用槽位）。
    pub total_live: u32,
    /// 池容量（槽位数）。
    pub capacity: u32,
    /// 空闲槽位数。
    pub free: u32,
    /// 水位（百分位，池侧同口径整数）。
    pub water_pct: u64,
    /// 压力档位。
    pub level: PressureLevel,
}

impl PoolStatsSnapshot {
    /// 快照是否与池侧对得上（**守恒式**：live + free == capacity）。
    ///
    /// 这条恒等式是双层统计「池层数可信」的锚：调试面直读的三个数
    /// 若自相矛盾（比如 live+free ≠ capacity），说明取数时刻池正在
    /// 被并发改写，快照本身不可信——宁可重取也不带着矛盾数据画图。
    pub fn self_consistent(&self) -> bool {
        self.total_live as u64 + self.free as u64 == self.capacity as u64
    }
}

/// 直读一个池的统计快照。
pub fn pool_snapshot(pool: &ParticlePool) -> PoolStatsSnapshot {
    PoolStatsSnapshot {
        total_live: pool.live(),
        capacity: pool.capacity(),
        free: pool.free_count(),
        water_pct: pool.water_pct(),
        level: pool.level(),
    }
}

// ---------------------------------------------------------------------------
// 三、发射器级统计 + 每秒聚合环形缓冲（双层统计的另一层）
// ---------------------------------------------------------------------------

/// 单发射器计数器：事件驱动（发射/死亡发生时由调用方记录）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmitterCounter {
    /// 发射器 id（F2203 层级语义里的发射器标识）。
    pub emitter_id: u32,
    /// 当前在用粒子数。
    pub live: u32,
    /// 累计生成数。
    pub spawned_total: u64,
    /// 累计死亡数。
    pub died_total: u64,
}

impl EmitterCounter {
    pub fn new(emitter_id: u32) -> EmitterCounter {
        EmitterCounter { emitter_id, live: 0, spawned_total: 0, died_total: 0 }
    }

    /// 记一次生成（n 可 >1：一次爆发多粒子）。
    pub fn on_spawn(&mut self, n: u32) {
        self.live = self.live.saturating_add(n);
        self.spawned_total = self.spawned_total.saturating_add(n as u64);
    }

    /// 记一次死亡。
    pub fn on_death(&mut self, n: u32) {
        // live 饱和减：死亡数超过在用数是记账矛盾（重复记死），
        // live 钳到 0，矛盾交给守恒判据红——此处不 panic。
        self.live = self.live.saturating_sub(n);
        self.died_total = self.died_total.saturating_add(n as u64);
    }

    /// 在用数与累计数是否自洽（live == spawned - died，饱和口径下须
    /// `spawned >= died` 时成立；died > spawned 本身即记账矛盾）。
    pub fn self_consistent(&self) -> bool {
        self.died_total <= self.spawned_total
            && self.live as u64 == self.spawned_total - self.died_total
    }
}

/// 一秒聚合样本（环形缓冲的元素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecondSample {
    /// 秒序号（自 `second_tick` 调用次数，从 1 起）。
    pub seq: u64,
    /// 池级：总粒子数。
    pub pool_total: u32,
    /// 池级：水位百分位。
    pub pool_water_pct: u64,
    /// 发射器级：本秒发射器数。
    pub emitter_count: u32,
    /// 本秒生成速率（个/秒）。
    pub spawn_rate: u32,
    /// 本秒死亡速率（个/秒）。
    pub death_rate: u32,
}

/// 秒聚合环形缓冲默认容量（保留最近 64 秒）。
pub const STATS_RING_CAP: usize = 64;

/// 每秒聚合环形缓冲：定容，只保最近。
#[derive(Clone, Debug)]
pub struct StatsRing {
    buf: Vec<SecondSample>,
    head: usize,
    filled: usize,
    overwritten: u64,
}

impl StatsRing {
    /// 指定容量新建（一次分配，此后不增长）。
    pub fn with_capacity(cap: usize) -> StatsRing {
        let cap = if cap == 0 { 1 } else { cap };
        let mut buf: Vec<SecondSample> = Vec::new();
        let mut i = 0usize;
        while i < cap {
            buf.push(SecondSample {
                seq: 0,
                pool_total: 0,
                pool_water_pct: 0,
                emitter_count: 0,
                spawn_rate: 0,
                death_rate: 0,
            });
            i += 1;
        }
        StatsRing { buf, head: 0, filled: 0, overwritten: 0 }
    }

    /// 默认容量新建。
    #[allow(clippy::new_without_default)]
    pub fn new() -> StatsRing {
        StatsRing::with_capacity(STATS_RING_CAP)
    }

    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// 有效样本数。
    pub fn len(&self) -> usize {
        self.filled
    }

    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// 被覆盖的最旧样本累计数。
    pub fn overwritten(&self) -> u64 {
        self.overwritten
    }

    /// 推入一个样本。返回 `false` 表示覆盖了最旧样本（调用方记账）。
    pub fn push(&mut self, sample: SecondSample, bag: &mut DiagBag) -> bool {
        let cap = self.buf.len();
        let mut idx = self.head;
        if idx >= cap {
            idx = 0;
        }
        // 有界写入：idx 经取模守卫，且 cap ≥ 1 由构造保证。
        match self.buf.get_mut(idx) {
            Some(cell) => *cell = sample,
            None => return false,
        }
        self.head = idx + 1;
        if self.head >= cap {
            self.head = 0;
        }
        if self.filled < cap {
            self.filled += 1;
            true
        } else {
            self.overwritten += 1;
            bag.push(DiagCode::STATS_RING_OVERWRITE);
            false
        }
    }

    /// 最新样本（`None` 当且仅当流为空）。
    pub fn latest(&self) -> Option<&SecondSample> {
        if self.filled == 0 {
            return None;
        }
        let cap = self.buf.len();
        let idx = if self.head == 0 { cap - 1 } else { self.head - 1 };
        self.buf.get(idx)
    }

    /// 按时间倒序取第 i 新的样本（i=0 即最新）。
    pub fn nth_newest(&self, i: usize) -> Option<&SecondSample> {
        if i >= self.filled {
            return None;
        }
        let cap = self.buf.len();
        let idx = (self.head + cap - 1 - i) % cap;
        self.buf.get(idx)
    }
}

/// 统计洪水阈值：发射器数超过此值即 TopN 降档（锚点「千级发射器」）。
pub const FLOOD_EMITTER_THRESHOLD: usize = 1000;

/// TopN 交付条数（编辑器面板一屏读得完的量级）。
pub const TOP_N_EMITTERS: usize = 64;

/// TopN 条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopNEntry {
    pub emitter_id: u32,
    pub live: u32,
}

/// TopN 降档结果：TopN 条 + others 聚合桶（**守恒可对账**）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopNResult {
    /// 按在用数降序（同数按 id 升序，序稳定不依赖排序实现）的前 N 条。
    pub entries: Vec<TopNEntry>,
    /// 未入榜的发射器个数。
    pub others_count: usize,
    /// 未入榜发射器的在用粒子总数（与 entries 之和恒等于全体之和）。
    pub others_live: u64,
    /// 全体发射器个数（降档前的真实规模，**如实记账**）。
    pub total_emitters: usize,
    /// 是否触发降档。
    pub downgraded: bool,
}

impl TopNResult {
    /// 守恒式：TopN 之和 + others == 全体在用之和。
    ///
    /// 降档若丢数据（只发 TopN 不聚合其余），这条恒等式立刻破——
    /// 消费端拿 total 一对账就发现统计面对不上池面，降档本身在撒谎。
    pub fn conserves(&self, all_live_sum: u64) -> bool {
        let mut top_sum = 0u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                top_sum += e.live as u64;
            }
            i += 1;
        }
        top_sum + self.others_live == all_live_sum
    }
}

// ---------------------------------------------------------------------------
// 四、双层统计中心（池级 + 发射器级 → 秒聚合 → 环形缓冲 → TopN 降档）
// ---------------------------------------------------------------------------

/// 双层统计中心。
///
/// 状态全部由调用方持有（无全局可变态）；「每秒」以 `second_tick`
/// 调用为边界——内核 no_std 无墙钟，秒边界由调用方节拍器驱动，
/// 与 F2408 工作单元口径同一诚实纪律。
#[derive(Clone, Debug)]
pub struct DebugStatsCenter {
    /// 池级快照（最近一次 `attach_pool` 直读值）。
    pool: PoolStatsSnapshot,
    /// 是否已挂接池（未挂接时秒样本的池级字段记 0 并可查）。
    pool_attached: bool,
    /// 发射器计数器表（按记录顺序，TopN 时再排序）。
    emitters: Vec<EmitterCounter>,
    /// 秒聚合环形缓冲。
    ring: StatsRing,
    /// 本窗口生成累计。
    window_spawns: u64,
    /// 本窗口死亡累计。
    window_deaths: u64,
    /// 秒序号。
    seq: u64,
    /// 累计降档次数。
    floods: u32,
}

impl DebugStatsCenter {
    #[allow(clippy::new_without_default)]
    pub fn new() -> DebugStatsCenter {
        DebugStatsCenter {
            pool: PoolStatsSnapshot {
                total_live: 0,
                capacity: 0,
                free: 0,
                water_pct: 0,
                level: PressureLevel::Normal,
            },
            pool_attached: false,
            emitters: Vec::new(),
            ring: StatsRing::new(),
            window_spawns: 0,
            window_deaths: 0,
            seq: 0,
            floods: 0,
        }
    }

    /// 直读挂接池（每帧一次；调试面取数端，不反写池）。
    pub fn attach_pool(&mut self, pool: &ParticlePool) {
        self.pool = pool_snapshot(pool);
        self.pool_attached = true;
    }

    /// 池级快照。
    pub fn pool(&self) -> &PoolStatsSnapshot {
        &self.pool
    }

    /// 是否已挂接池。
    pub fn pool_attached(&self) -> bool {
        self.pool_attached
    }

    /// 发射器计数器表（只读）。
    pub fn emitters(&self) -> &[EmitterCounter] {
        &self.emitters
    }

    /// 环形缓冲（只读）。
    pub fn ring(&self) -> &StatsRing {
        &self.ring
    }

    /// 累计降档次数。
    pub fn floods(&self) -> u32 {
        self.floods
    }

    /// 记一次生成（发射器不存在则建档）。
    pub fn record_spawn(&mut self, emitter_id: u32, n: u32) {
        let mut found = false;
        let mut i = 0usize;
        while i < self.emitters.len() {
            if let Some(e) = self.emitters.get_mut(i) {
                if e.emitter_id == emitter_id {
                    e.on_spawn(n);
                    found = true;
                    break;
                }
            }
            i += 1;
        }
        if !found {
            let mut e = EmitterCounter::new(emitter_id);
            e.on_spawn(n);
            self.emitters.push(e);
        }
        self.window_spawns = self.window_spawns.saturating_add(n as u64);
    }

    /// 记一次死亡。
    pub fn record_death(&mut self, emitter_id: u32, n: u32) {
        let mut i = 0usize;
        while i < self.emitters.len() {
            if let Some(e) = self.emitters.get_mut(i) {
                if e.emitter_id == emitter_id {
                    e.on_death(n);
                    break;
                }
            }
            i += 1;
        }
        // 死亡未建档：只在窗口累计（发射器全死亡后建档只剩 0 值条目，
        // 没有信息量——但窗口死亡率必须如实反映）。
        self.window_deaths = self.window_deaths.saturating_add(n as u64);
    }

    /// 秒边界：聚合本窗口 → 推环形缓冲 → 清窗。
    pub fn second_tick(&mut self, bag: &mut DiagBag) -> SecondSample {
        self.seq += 1;
        let sample = SecondSample {
            seq: self.seq,
            pool_total: self.pool.total_live,
            pool_water_pct: self.pool.water_pct,
            emitter_count: self.emitters.len() as u32,
            spawn_rate: self.window_spawns.min(u32::MAX as u64) as u32,
            death_rate: self.window_deaths.min(u32::MAX as u64) as u32,
        };
        self.ring.push(sample, bag);
        self.window_spawns = 0;
        self.window_deaths = 0;
        sample
    }

    /// TopN 降档交付（**守恒式可对账**，见 [`TopNResult::conserves`]）。
    ///
    /// 排序键：在用数降序、同数 id 升序——**序稳定不依赖排序实现**：
    /// 若序依赖排序算法的稳定性，换个 sort 实现榜单顺序就变，编辑器
    /// 上同一帧刷新两次榜单跳位，比不排还难受。
    pub fn topn(&mut self, bag: &mut DiagBag) -> TopNResult {
        let total = self.emitters.len();
        // 全体和对账基准由 [`DebugStatsCenter::all_live_sum`] 提供，
        // 此处不重算——判据侧也不信任本方法的中间量（弱门禁纪律：
        // 累加与被测同源时守恒式恒真）。
        if total <= FLOOD_EMITTER_THRESHOLD {
            let mut entries: Vec<TopNEntry> = Vec::new();
            let mut j = 0usize;
            while j < total {
                if let Some(e) = self.emitters.get(j) {
                    entries.push(TopNEntry { emitter_id: e.emitter_id, live: e.live });
                }
                j += 1;
            }
            return TopNResult {
                entries,
                others_count: 0,
                others_live: 0,
                total_emitters: total,
                downgraded: false,
            };
        }
        // 洪水：按 (live 降序, id 升序) 排序后取前 N，其余聚合。
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        let mut j = 0usize;
        while j < total {
            if let Some(e) = self.emitters.get(j) {
                pairs.push((e.emitter_id, e.live));
            }
            j += 1;
        }
        pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut entries: Vec<TopNEntry> = Vec::new();
        let mut others_count = 0usize;
        let mut others_live = 0u64;
        let mut k = 0usize;
        while k < pairs.len() {
            if let Some(&(id, live)) = pairs.get(k) {
                if k < TOP_N_EMITTERS {
                    entries.push(TopNEntry { emitter_id: id, live });
                } else {
                    others_count += 1;
                    others_live += live as u64;
                }
            }
            k += 1;
        }
        self.floods = self.floods.saturating_add(1);
        bag.push_major(DiagCode::STATS_FLOOD);
        TopNResult {
            entries,
            others_count,
            others_live,
            total_emitters: total,
            downgraded: true,
        }
    }

    /// 全体发射器在用粒子之和（守恒式判据的对账基准）。
    pub fn all_live_sum(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.emitters.len() {
            if let Some(e) = self.emitters.get(i) {
                s += e.live as u64;
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 五、gizmo 流（五形状线框 + 双级 LOD）
// ---------------------------------------------------------------------------

/// gizmo 单顶点（发射器局部坐标即世界坐标——发射器形状参数本身就是
/// 世界系，见 F2203 `ShapeParams`）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GizmoVertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl GizmoVertex {
    pub fn of(v: Vec3) -> GizmoVertex {
        GizmoVertex { x: v.x, y: v.y, z: v.z }
    }

    pub fn is_finite(&self) -> bool {
        is_finite(self.x) && is_finite(self.y) && is_finite(self.z)
    }
}

/// 默认顶点预算（单 gizmo 一次交付的顶点上限）。
pub const GIZMO_VERTEX_BUDGET: usize = 512;

/// 基础细分段数（LOD 距离档的满档值；圆周逼近复用 F2203 cos/sin）。
pub const GIZMO_BASE_SEGS: usize = 32;

/// LOD 最低段数（圆周至少三边才是圆；低于此值线框形状失真）。
pub const GIZMO_MIN_SEGS: usize = 3;

/// gizmo 请求。
#[derive(Clone, Debug)]
pub struct GizmoRequest {
    /// 发射器 id。
    pub emitter_id: u32,
    /// 形状类型（F2203 单源）。
    pub shape: ShapeKind,
    /// 形状参数（F2203 单源，含校验）。
    pub params: ShapeParams,
    /// 相机距离（世界单位；LOD 距离档依据）。
    pub distance: f32,
}

/// LOD 距离档：距离越远细分越疏（四档：1/1、1/2、1/4、1/8）。
///
/// **返回值下限 [`GIZMO_MIN_SEGS`]**：段数除到 0 会让圆周退化成一个点
/// （gizmo 消失）——「远处看不见」应该是 LOD 的渐进效果而不是突然消失。
/// 非有限/负距离按近距离满档处理（相机数据异常时不该拿 gizmo 陪葬）。
pub fn lod_segments(distance: f32, base: usize) -> usize {
    if !is_finite(distance) || distance < 0.0 {
        return if base < GIZMO_MIN_SEGS { GIZMO_MIN_SEGS } else { base };
    }
    let div: usize = if distance < 10.0 {
        1
    } else if distance < 50.0 {
        2
    } else if distance < 200.0 {
        4
    } else {
        8
    };
    let seg = base / div;
    if seg < GIZMO_MIN_SEGS {
        GIZMO_MIN_SEGS
    } else {
        seg
    }
}

/// 一个 gizmo 交付帧。
#[derive(Clone, Debug, PartialEq)]
pub struct GizmoFrame {
    /// 发射器 id。
    pub emitter_id: u32,
    /// 形状类型。
    pub shape: ShapeKind,
    /// 线框顶点（**成对即线段**：顶点序列按段组织，2k 与 2k+1 是一段）。
    pub vertices: Vec<GizmoVertex>,
    /// 是否触发了 LOD 减密。
    pub lod_applied: bool,
    /// 减密**前**的顶点数（抽稀如实记账，绘制端才知道拿到的是简化视图）。
    pub requested_vertices: usize,
    /// LOD 后段数（点/线/网格形状无圆周，记 0）。
    pub segs: usize,
}

impl GizmoFrame {
    /// 线段数（顶点成对）。
    pub fn segments(&self) -> usize {
        self.vertices.len() / 2
    }

    /// 顶点是否全部有限（NaN 绝不进绘制缓冲——一条 NaN 污染整条线）。
    pub fn all_finite(&self) -> bool {
        let mut i = 0usize;
        while i < self.vertices.len() {
            match self.vertices.get(i) {
                Some(v) if !v.is_finite() => return false,
                None => return false,
                _ => {}
            }
            i += 1;
        }
        true
    }
}

/// 生成一个圆周（圆心 center、正交基 u/v、半径 r、段数 segs）。
///
/// 段数 segs 产出 segs 条边 = 2×segs 个顶点（首尾闭合由末段回连首点实现，
/// 不重复顶点）。
fn circle_wire(center: Vec3, u: Vec3, v: Vec3, r: f32, segs: usize, out: &mut Vec<GizmoVertex>) {
    if segs < GIZMO_MIN_SEGS || !(r > 0.0) || !is_finite(r) {
        return;
    }
    let tau = core::f32::consts::TAU;
    let mut k = 0usize;
    while k < segs {
        let a0 = tau * (k as f32) / (segs as f32);
        let a1 = tau * ((k + 1) as f32) / (segs as f32);
        let p0 = super::vel03_emitter::add_v3(
            center,
            super::vel03_emitter::add_v3(
                super::vel03_emitter::scale_v3(u, cos_approx(a0) * r),
                super::vel03_emitter::scale_v3(v, sin_approx(a0) * r),
            ),
        );
        let p1 = super::vel03_emitter::add_v3(
            center,
            super::vel03_emitter::add_v3(
                super::vel03_emitter::scale_v3(u, cos_approx(a1) * r),
                super::vel03_emitter::scale_v3(v, sin_approx(a1) * r),
            ),
        );
        out.push(GizmoVertex::of(p0));
        out.push(GizmoVertex::of(p1));
        k += 1;
    }
}

/// 点形状线框：过原点的三轴短十字（3 段 = 6 顶点），边长 `arm`。
fn point_wire(origin: Vec3, arm: f32, out: &mut Vec<GizmoVertex>) {
    let axes = [
        Vec3::new(arm, 0.0, 0.0),
        Vec3::new(0.0, arm, 0.0),
        Vec3::new(0.0, 0.0, arm),
    ];
    let mut i = 0usize;
    while i < axes.len() {
        if let Some(a) = axes.get(i) {
            let p0 = super::vel03_emitter::add_v3(origin, super::vel03_emitter::scale_v3(*a, -1.0));
            let p1 = super::vel03_emitter::add_v3(origin, *a);
            out.push(GizmoVertex::of(p0));
            out.push(GizmoVertex::of(p1));
        }
        i += 1;
    }
}

/// 球形状线框：三个正交大圆（XY/YZ/ZX 平面），各 segs 段。
fn sphere_wire(center: Vec3, radius: f32, segs: usize, out: &mut Vec<GizmoVertex>) {
    // XY 平面大圆：u=x 轴，v=y 轴。
    circle_wire(center, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), radius, segs, out);
    // YZ 平面大圆：u=y 轴，v=z 轴。
    circle_wire(center, Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0), radius, segs, out);
    // ZX 平面大圆：u=z 轴，v=x 轴。
    circle_wire(center, Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0), radius, segs, out);
}

/// 锥形状线框：底面圆环（segs 段）+ 从顶点到底面圆周的 segs 条辐条。
fn cone_wire(apex: Vec3, axis: Vec3, half_angle: f32, length: f32, segs: usize, out: &mut Vec<GizmoVertex>) {
    let ax = super::vel03_emitter::normalize_v3(axis);
    if length_v3(ax) <= super::vel03_emitter::DEGENERATE_EPS {
        return;
    }
    // 构造与轴正交的基（取任意不平行轴的辅助向量做叉积）。
    let helper = if ax.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) };
    let u = super::vel03_emitter::normalize_v3(super::vel03_emitter::cross_v3(ax, helper));
    let v = super::vel03_emitter::normalize_v3(super::vel03_emitter::cross_v3(ax, u));
    // tan = sin/cos，同承 F2203 内核替身单源（cos_approx/sin_approx）：
    // 裸机面（x86_64-unknown-none）无 libm 的 f32::tan；且两份几何逼近
    // 并存迟早漂移——gizmo 底环半径与发射采样域不一致等于调试数据撒谎。
    // 校验单源（F2203 validate_shape）保证 half_angle ∈ (0, π/2) 开区间，
    // cos > 0 不除零。
    let base_r = length * (sin_approx(half_angle) / cos_approx(half_angle));
    let base_c = super::vel03_emitter::add_v3(apex, super::vel03_emitter::scale_v3(ax, length));
    // 底面圆环。
    circle_wire(base_c, u, v, base_r, segs, out);
    // 辐条：顶点 → 圆周点（segs 条 = 2×segs 顶点）。
    let tau = core::f32::consts::TAU;
    let mut k = 0usize;
    while k < segs {
        let a0 = tau * (k as f32) / (segs as f32);
        let p = super::vel03_emitter::add_v3(
            base_c,
            super::vel03_emitter::add_v3(
                super::vel03_emitter::scale_v3(u, cos_approx(a0) * base_r),
                super::vel03_emitter::scale_v3(v, sin_approx(a0) * base_r),
            ),
        );
        out.push(GizmoVertex::of(apex));
        out.push(GizmoVertex::of(p));
        k += 1;
    }
}

/// 网格表面线框：三角形集合的包围盒（12 条棱 = 24 顶点）。
///
/// 不画每个三角形（网格动辄万面，调试面画全量本身就是洪水）——
/// 包围盒给出「这个发射器的发射域大概占多大」的调试信息，正是
/// gizmo 的职责量级。
fn mesh_wire(tris: &[super::vel03_emitter::Triangle], out: &mut Vec<GizmoVertex>) {
    if tris.is_empty() {
        return;
    }
    let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
    let mut i = 0usize;
    while i < tris.len() {
        if let Some(t) = tris.get(i) {
            let vs = [t.a, t.b, t.c];
            let mut j = 0usize;
            while j < vs.len() {
                if let Some(p) = vs.get(j) {
                    if p.x < min.x {
                        min.x = p.x;
                    }
                    if p.y < min.y {
                        min.y = p.y;
                    }
                    if p.z < min.z {
                        min.z = p.z;
                    }
                    if p.x > max.x {
                        max.x = p.x;
                    }
                    if p.y > max.y {
                        max.y = p.y;
                    }
                    if p.z > max.z {
                        max.z = p.z;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    // 八个角点。
    let c000 = Vec3::new(min.x, min.y, min.z);
    let c100 = Vec3::new(max.x, min.y, min.z);
    let c010 = Vec3::new(min.x, max.y, min.z);
    let c110 = Vec3::new(max.x, max.y, min.z);
    let c001 = Vec3::new(min.x, min.y, max.z);
    let c101 = Vec3::new(max.x, min.y, max.z);
    let c011 = Vec3::new(min.x, max.y, max.z);
    let c111 = Vec3::new(max.x, max.y, max.z);
    // 12 条棱：底面 4 + 顶面 4 + 竖边 4。
    let edges = [
        (c000, c100), (c100, c110), (c110, c010), (c010, c000),
        (c001, c101), (c101, c111), (c111, c011), (c011, c001),
        (c000, c001), (c100, c101), (c110, c111), (c010, c011),
    ];
    let mut e = 0usize;
    while e < edges.len() {
        if let Some((a, b)) = edges.get(e) {
            out.push(GizmoVertex::of(*a));
            out.push(GizmoVertex::of(*b));
        }
        e += 1;
    }
}

/// 生成发射器线框（**带预算**，双级 LOD 的预算级入口）。
///
/// 流程：参数校验（F2203 单源）→ 距离档定段数 → 生成 → 超预算则
/// 折半减密重生成（每轮记一次溢出）→ 仍超则**截断交付**（截断量
/// 如实记账）。参数非法拒绝**显性**（记 [`DiagCode::GIZMO_PARAM_REJECTED`]，
/// 返回空帧而非静默空帧——两者调用方拿到的东西一样，但诊断面能分清
/// 「没有形状」和「形状被拒了」）。
pub fn emitter_wireframe_with_budget(
    req: &GizmoRequest,
    budget: usize,
    bag: &mut DiagBag,
) -> GizmoFrame {
    let mut frame = GizmoFrame {
        emitter_id: req.emitter_id,
        shape: req.shape,
        vertices: Vec::new(),
        lod_applied: false,
        requested_vertices: 0,
        segs: 0,
    };
    // 参数校验单源：F2203 validate_shape。非法即显性拒绝。
    if validate_shape(&req.params).is_err() {
        bag.push_major(DiagCode::GIZMO_PARAM_REJECTED);
        return frame;
    }
    let budget = if budget == 0 { 1 } else { budget };
    let mut segs = lod_segments(req.distance, GIZMO_BASE_SEGS);
    build_shape_wire(&req.params, segs, &mut frame.vertices);
    frame.requested_vertices = frame.vertices.len();
    // 预算超限：折半减密（LOD 级），每轮记溢出。
    while frame.vertices.len() > budget && segs > GIZMO_MIN_SEGS {
        segs /= 2;
        if segs < GIZMO_MIN_SEGS {
            segs = GIZMO_MIN_SEGS;
        }
        frame.vertices.clear();
        build_shape_wire(&req.params, segs, &mut frame.vertices);
        frame.lod_applied = true;
        bag.push(DiagCode::GIZMO_VERTEX_OVERFLOW);
    }
    // 减密到下限仍超：截断交付（顶点成对截，不撕半段线）。
    if frame.vertices.len() > budget {
        let keep = if budget % 2 == 0 { budget } else { budget - 1 };
        frame.vertices.truncate(keep);
        frame.lod_applied = true;
        bag.push(DiagCode::GIZMO_VERTEX_OVERFLOW);
    }
    frame.segs = segs;
    frame
}

/// 默认预算入口。
pub fn emitter_wireframe(req: &GizmoRequest, bag: &mut DiagBag) -> GizmoFrame {
    emitter_wireframe_with_budget(req, GIZMO_VERTEX_BUDGET, bag)
}

/// 按形状分发线框生成（五形状各自线框，判据「gizmo 五形状」直译）。
fn build_shape_wire(params: &ShapeParams, segs: usize, out: &mut Vec<GizmoVertex>) {
    match params {
        ShapeParams::Point { origin } => point_wire(*origin, 0.25, out),
        ShapeParams::Line { from, to } => {
            out.push(GizmoVertex::of(*from));
            out.push(GizmoVertex::of(*to));
        }
        ShapeParams::Sphere { center, radius, .. } => sphere_wire(*center, *radius, segs, out),
        ShapeParams::Cone { apex, axis, half_angle, length, .. } => {
            cone_wire(*apex, *axis, *half_angle, *length, segs, out)
        }
        ShapeParams::Mesh { surface } => mesh_wire(&surface.triangles, out),
    }
}

// ---------------------------------------------------------------------------
// 六、力场预留位（STUB · 显性报错 · 对接位 F2230）
// ---------------------------------------------------------------------------

/// 力场预留线上码（**故意不注册**进信封表——预留 ≠ 已实现）。
pub const FIELD_RESERVE_WIRE: u16 = 0x03;

/// 显性报错的指路文案（读屏可达）。
pub const FIELD_STUB_HINT: &str =
    "力场矢量场可视化尚未实现：预留对接位归 F2230（力场 gizmo 兑现）；\
     当前请用力场参数面板查看恒定力场，或等待 F2230 落地后改走信封注册负载";

/// 力场预留存根。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldGizmoStub {
    /// 被调用累计次数（调试面自己的可见性：预留被反复调用说明有
    /// 消费端在等 F2230，这个数是立项依据之一）。
    pub calls: u64,
}

impl FieldGizmoStub {
    pub fn new() -> FieldGizmoStub {
        FieldGizmoStub { calls: 0 }
    }

    /// 请求力场矢量场流：**显性报错**（F1871 STUB 语义）。
    ///
    /// 返回 `Err(())` 且记 [`DiagCode::FIELD_STUB_CALLED`]——不静默返回
    /// 空数据（消费端会把「没实现」误读成「没有力场」，调试数据本身
    /// 在撒谎），也不 panic（调试旁路崩溃会连坐主循环）。
    pub fn request(&mut self, bag: &mut DiagBag) -> Result<(), ()> {
        self.calls += 1;
        bag.push_major(DiagCode::FIELD_STUB_CALLED);
        Err(())
    }
}

// ---------------------------------------------------------------------------
// 七、信封 L 段注册（F1764 负载族扩展 · 两类型 + 漂移对账拦截）
// ---------------------------------------------------------------------------

/// L 段负载类型（**两类型**注册：粒子统计 / 发射器 gizmo）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LPayloadKind {
    /// 粒子统计负载（双层统计流）。
    ParticleStats,
    /// 发射器 gizmo 负载（五形状线框流）。
    EmitterGizmo,
}

impl LPayloadKind {
    /// 全部类型（L 段取值域——**恰好两型**，力场预留不算：见
    /// [`FIELD_RESERVE_WIRE`] 注）。
    pub const ALL: [LPayloadKind; 2] = [LPayloadKind::ParticleStats, LPayloadKind::EmitterGizmo];

    /// 线上编码（**显式映射**，不用 `as u16`——判别值不是线上值）。
    pub const fn wire(self) -> u16 {
        match self {
            LPayloadKind::ParticleStats => 0x01,
            LPayloadKind::EmitterGizmo => 0x02,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            LPayloadKind::ParticleStats => "粒子统计",
            LPayloadKind::EmitterGizmo => "发射器 gizmo",
        }
    }

    /// 按线上编码反查（0x03 力场预留码返回 `None`——未注册）。
    pub fn from_wire(w: u16) -> Option<LPayloadKind> {
        LPayloadKind::ALL.iter().copied().find(|k| k.wire() == w)
    }
}

/// 信封 schema 版本（L 段 v1）。
pub const L_ENVELOPE_SCHEMA: u16 = 1;

/// L 段注册表容量（两类型）。
pub const L_SEGMENT_SLOTS: usize = 2;

/// 一个负载信封。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LEnvelope {
    /// 负载类型。
    pub kind: LPayloadKind,
    /// schema 版本。
    pub schema: u16,
    /// 载荷字节数。
    pub byte_len: u32,
    /// 序号。
    pub seq: u32,
    /// 注册时声明的摘要（对账基准）。
    pub declared: u32,
}

/// FNV-1a 单步。
const fn fnv_step(h: u32, byte: u32) -> u32 {
    (h ^ byte).wrapping_mul(0x0100_0193)
}

impl LEnvelope {
    /// 造信封并写入自摘要（注册面唯一构造口）。
    pub fn new(kind: LPayloadKind, byte_len: u32, seq: u32) -> LEnvelope {
        let mut e = LEnvelope { kind, schema: L_ENVELOPE_SCHEMA, byte_len, seq, declared: 0 };
        e.declared = e.checksum();
        e
    }

    /// 重算摘要（对五字段 + 标签长度的 FNV-1a）。
    pub fn checksum(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        h = fnv_step(h, self.kind.wire() as u32);
        h = fnv_step(h, self.schema as u32);
        h = fnv_step(h, self.byte_len);
        h = fnv_step(h, self.seq);
        h = fnv_step(h, self.kind.label().len() as u32);
        h
    }

    /// 是否漂移（重算 ≠ 声明）。
    pub fn drifted(&self) -> bool {
        self.checksum() != self.declared
    }
}

/// L 段信封注册表。
#[derive(Clone, Debug)]
pub struct LEnvelopeRegistry {
    slots: Vec<Option<LEnvelope>>,
    epoch: u32,
    drift_count: u32,
    intercepted: bool,
}

impl LEnvelopeRegistry {
    /// 新建（两槽空表）。
    #[allow(clippy::new_without_default)]
    pub fn new() -> LEnvelopeRegistry {
        let mut slots: Vec<Option<LEnvelope>> = Vec::new();
        let mut i = 0usize;
        while i < L_SEGMENT_SLOTS {
            slots.push(None);
            i += 1;
        }
        LEnvelopeRegistry { slots, epoch: 0, drift_count: 0, intercepted: false }
    }

    /// 注册一个信封（重注册即替换）。**未注册码（含力场预留 0x03）返回
    /// `false` 并记账**——拒绝必须显性，静默 false 会让调用方以为是成功。
    pub fn register(&mut self, env: LEnvelope, bag: &mut DiagBag) -> bool {
        let wire = env.kind.wire();
        if wire == 0 || (wire as usize) > L_SEGMENT_SLOTS {
            bag.push_major(DiagCode::ENVELOPE_UNKNOWN_PAYLOAD);
            return false;
        }
        self.epoch = self.epoch.wrapping_add(1);
        match self.slots.get_mut((wire - 1) as usize) {
            Some(cell) => {
                *cell = Some(env);
                true
            }
            None => {
                bag.push_major(DiagCode::ENVELOPE_UNKNOWN_PAYLOAD);
                false
            }
        }
    }

    /// 只读查表（不受拦截影响——对账自身要能看到信封）。
    pub fn peek(&self, kind: LPayloadKind) -> Option<&LEnvelope> {
        let wire = kind.wire();
        if wire == 0 || (wire as usize) > L_SEGMENT_SLOTS {
            return None;
        }
        self.slots.get((wire - 1) as usize).and_then(|c| c.as_ref())
    }

    /// 消费端取用（**拦截态下一律 `None`**——漂移信封不得被消费）。
    pub fn take(&self, kind: LPayloadKind) -> Option<&LEnvelope> {
        if self.intercepted {
            return None;
        }
        self.peek(kind)
    }

    /// 已注册类型数。
    pub fn registered(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(c) = self.slots.get(i) {
                if c.is_some() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 注册代数。
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// 累计漂移数。
    pub fn drift_count(&self) -> u32 {
        self.drift_count
    }

    /// 是否拦截态。
    pub fn intercepted(&self) -> bool {
        self.intercepted
    }

    /// 显式解除拦截（须由人确认后调用，不随注册自动清除）。
    pub fn clear_intercept(&mut self) {
        self.intercepted = false;
    }

    /// 对账：重算全部信封摘要。返回漂移数；**有漂移即置拦截**（P1）。
    pub fn reconcile(&mut self, bag: &mut DiagBag) -> u32 {
        let mut drifted = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            let bad = match self.slots.get(i).and_then(|c| c.as_ref()) {
                Some(e) => e.drifted(),
                None => false,
            };
            if bad {
                drifted += 1;
            }
            i += 1;
        }
        if drifted > 0 {
            self.drift_count = self.drift_count.saturating_add(drifted);
            self.intercepted = true;
            bag.push_p1(DiagCode::ENVELOPE_DRIFT);
        }
        drifted
    }

    /// 注册表是否齐备（两类型全注册）。
    pub fn complete(&self) -> bool {
        self.registered() == L_SEGMENT_SLOTS
    }
}

// ---------------------------------------------------------------------------
// 八、发行版剔除零成本（双形态 · 可证伪）
// ---------------------------------------------------------------------------

/// 构建档位。`Default` 取 `Debug`：**调试是安全默认**——想要零成本剔除
/// 必须显式写 `Release`；反过来（发行默认）会让忘配的发行构建悄悄带上
/// 全部调试负载。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BuildProfile {
    /// 开发形态：调试负载可用。
    #[default]
    Debug,
    /// 发行形态：调试负载零成本剔除。
    Release,
}

/// 剔除守卫。
#[derive(Clone, Copy, Debug, Default)]
pub struct StripGuard {
    /// 构建档位。
    pub profile: BuildProfile,
    /// 已构建的调试负载数（Release 档下恒为 0，**含强行尝试路径**）。
    pub payload_builds: u64,
    /// 被强行请求的次数（Release 档下只增此项）。
    pub forced_attempts: u64,
}

impl StripGuard {
    pub const fn new(profile: BuildProfile) -> StripGuard {
        StripGuard { profile, payload_builds: 0, forced_attempts: 0 }
    }

    /// 当前档位是否允许调试负载。
    pub const fn payloads_enabled(&self) -> bool {
        matches!(self.profile, BuildProfile::Debug)
    }

    /// 请求构建一个调试负载。返回是否真的构建。
    ///
    /// 零成本的物质保证：Release 档下 `payload_builds` **不递增**——
    /// 不是「构建完再抹掉」，而是根本不进入构建。强行请求只记
    /// `forced_attempts` 与一条 P1（[`DiagCode::STRIP_FAILED`]）。
    pub fn try_build(&mut self, bag: &mut DiagBag) -> bool {
        if self.payloads_enabled() {
            self.payload_builds += 1;
            true
        } else {
            self.forced_attempts += 1;
            bag.push_p1(DiagCode::STRIP_FAILED);
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 九、族声明与冒烟
// ---------------------------------------------------------------------------

/// 家族声明一致性（判据用）。
pub fn family_is_consistent() -> bool {
    LPayloadKind::ALL.len() == L_SEGMENT_SLOTS && DiagCode::ALL.len() == 8
}

/// 两类型线上码互异（wire 显式映射的机检）。
pub fn wires_unique() -> bool {
    let mut seen: Vec<u16> = Vec::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < LPayloadKind::ALL.len() {
        let w = LPayloadKind::ALL[i].wire();
        if seen.contains(&w) {
            ok = false;
        } else {
            seen.push(w);
        }
        i += 1;
    }
    // 力场预留码不得与已注册码撞车。
    let mut j = 0usize;
    while j < LPayloadKind::ALL.len() {
        if let Some(k) = LPayloadKind::ALL.get(j) {
            if k.wire() == FIELD_RESERVE_WIRE {
                ok = false;
            }
        }
        j += 1;
    }
    ok
}

/// 码标签互异（两码共用一句人话是最难查的一类缺陷）。
pub fn labels_unique() -> bool {
    let mut i = 0usize;
    while i < DiagCode::ALL.len() {
        let mut j = i + 1;
        while j < DiagCode::ALL.len() {
            let a = match DiagCode::ALL.get(i) {
                Some(c) => *c,
                None => return true,
            };
            let b = match DiagCode::ALL.get(j) {
                Some(c) => *c,
                None => return true,
            };
            if a.label() == b.label() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 描述。
pub fn describe() -> String {
    let mut s = String::new();
    let _ = s.push_str("粒子调试数据（VE-F2209）：\n");
    let _ = s.push_str("· 双层统计：池级直读 F2208 计数器（零额外计算）+ 发射器级事件计数，每秒聚合进定容环形缓冲。\n");
    let _ = s.push_str("· 统计洪水防线：千级发射器 TopN 降档交付，其余聚合不丢弃，守恒式可对账。\n");
    let _ = s.push_str("· gizmo 五形状线框：形状/校验/圆周逼近全部复用 F2203 单源，距离四档 + 预算折半双级 LOD。\n");
    let _ = s.push_str("· 力场预留位：STUB 显性报错指路 F2230，不静默给空数据。\n");
    let _ = s.push_str("· 信封 L 段两类型注册：FNV 对账漂移即拦截，消费端一律取不到。\n");
    let _ = s.push_str("· 发行版剔除零成本：Release 档连强行尝试都不递增构建计数。\n");
    s
}

/// 冒烟：双层统计一轮 + 一个球 gizmo + 一次预留调用。
pub fn smoke() -> String {
    use super::vel03_emitter::DEGENERATE_EPS;
    let mut bag = DiagBag::new();
    let mut center = DebugStatsCenter::new();
    center.record_spawn(1, 100);
    center.record_spawn(2, 50);
    center.record_death(1, 30);
    let s1 = center.second_tick(&mut bag);
    // 球 gizmo。
    let req = GizmoRequest {
        emitter_id: 1,
        shape: ShapeKind::Sphere,
        params: ShapeParams::Sphere {
            center: Vec3::new(0.0, 1.0, 0.0),
            radius: 2.0,
            mode: super::vel03_emitter::SphereMode::Surface,
        },
        distance: 5.0,
    };
    let g = emitter_wireframe(&req, &mut bag);
    // 预留调用。
    let mut stub = FieldGizmoStub::new();
    let _ = stub.request(&mut bag);
    let mut s = String::new();
    let _ = s.push_str(&format!(
        "统计 池总={} 发射器={} 生成率={} 死亡率={} 环深={}\n",
        s1.pool_total,
        s1.emitter_count,
        s1.spawn_rate,
        s1.death_rate,
        center.ring().len()
    ));
    let _ = s.push_str(&format!(
        "gizmo 球 顶点={} 段数={} LOD={} 全有限={}（半径阈值 {}）\n",
        g.vertices.len(),
        g.segs,
        g.lod_applied,
        g.all_finite(),
        DEGENERATE_EPS
    ));
    let _ = s.push_str(&format!("预留 调用={} 诊断={}\n", stub.calls, bag.len()));
    s
}
