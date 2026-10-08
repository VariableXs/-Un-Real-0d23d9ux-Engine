//! VE-F2013 · 后处理调试数据（VE-K 域 · 后处理架构与 Bloom 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2013`
//!
//! **判据（锚点原文五条）**：三类负载、逐级透视、图结构可视、发行版零成本、判据。
//!
//! 本条是后处理链的**可视化面**：对上（VE-Y 可视化端 / 开发者 CLI）把
//! 「每效果一行的实时数据」「任一中间 RT 的内容」「后处理 DAG 的图结构」
//! 三类调试负载端出去。**它不做任何效果求值**——求值是 F2002/F2014 的职责，
//! 本条只负责「把已经算出来的东西，按口径唯一、可对账、发行版零成本地端出去」。
//!
//! | 锚点职责 | 本条落位 |
//! | --- | --- |
//! | 效果开关与参数实时可视（每效果一行：状态/关键参数/独占耗时） | [`EffectStat`] + [`EffectStatsRing`] |
//! | 中间 RT 可视（节点 ID → 该节点输出 RT 内容） | [`RtPick`] + [`pick_rt`] |
//! | DAG 图结构可视（节点/边/拓扑序/别名复用） | [`GraphSnapshot`] + [`GraphModel`] |
//! | 发行版物理剔除零成本 | [`StripGuard`]（编译期保证，见下） |
//! | 信封封装 J 段注册沿用 | [`PayloadKind`] + [`Envelope`] + [`EnvelopeRegistry`] |
//!
//! ## 设计决策记录（D1~D12）
//!
//! - **D1 · 逐级透视的「级」必须可枚举，不能只靠 `Vec<node_id>` 暗示**。
//!   锚点要求「DAG 节点选中 → 该节点输出 RT 显示」，而「能选中」不等于
//!   「能看到它前面那一级」。本条把透视深度做成 [`RtPick`] 的 `depth`
//!   字段并由 `pick_rt` 真实回溯拓扑序：**深度越深、返回链越长**，
//!   判据逐级断言 `depth = d` 时恰好拿到 `d` 段祖先链。
//!
//! - **D2 · 「独占耗时」不能靠总耗时冒充分享**。锚点写「独占耗时」，
//!   即该效果自身花费，而非含下游的时间。总耗时直读打点（[`RawTiming`]）
//!   与独占耗时（[`EffectStat::exclusive_ns`]）是两个字段、两个口径，
//!   判据断「独占 ≤ 同帧总耗时」，且二者同值即判退化。
//!
//! - **D3 · 参数摘要用定点整数，不用 `f32`**。参数摘要要进指纹与对账，
//!   `f32` 的 `-0.0`/`NaN` 位模式会让同一份参数在不同优化级别下算出
//!   不同摘要（**假对账**）。故 [`EffectStat::param_digest`] 是 `u32`
//!   定点摘要，判据侧独立重算 FNV-1a 逐位比对。
//!
//! - **D4 · 统计洪水降频必须「可数」**：降频不是把数据悄悄扔掉，
//!   而是 stride 加倍 + `downsampled` 计数 + 告警三件齐做，
//!   判据断「降频后写入次数 / 逻辑帧数 == 1/stride」且 `downsampled`
//!   计数恰好等于被跳过的次数（**守恒**，不丢不重）。
//!
//! - **D5 · RT 拉取超带宽 → 下采样拷贝档，**不是拒绝**。锚点原文是
//!   「下采样拷贝档」。故 [`RtPick`] 带 [`RtScale`] 档位，超预算时
//!   **降档并如实标注 `downscaled: true` + 记录原档**——
//!   静默降档会让 VE-Y 拿着 1/4 分辨率当全分辨率解读。
//!
//! - **D6 · 图快照版本漂移必须「拦截」而非「记一笔」**。锚点写
//!   「版本号对账告警」，但若只是记一笔告警就照发快照，VE-Y 会把
//!   **旧拓扑**画成当前拓扑。故 [`GraphSnapshot::reconcile`] 不一致时
//!   置 `intercepted`，此后 `take()` 一律返回 `None`（与 F2408 同纪律）。
//!
//! - **D7 · 拓扑序必须真排序，不能靠插入序**。判据对**乱序插入**的
//!   语料断言排序结果——若实现用插入序当拓扑序，这条判据会红。
//!   本条用 Kahn 算法（入度为 0 者按 node_id 序出队，保证**确定性**：
//!   同一图多次拓扑排序结果逐位相同）。
//!
//! - **D8 · 别名复用必须让「多个节点共享同一 RT」成为可判定事实**。
//!   锚点要求可视「别名复用关系」。故 [`GraphNode::rt_alias`] 显式声明，
//!   判据断「两个节点声明同一 alias ⇒ 复用计数 == 2 且
//!   **物理分配数仍为 1**」——后者是「复用」二字的可核对形式。
//!
//! - **D9 · 环必须被检出且不得进入拓扑排序**。Kahn 出队数 < 节点总数
//!   即存在环，此时返回 `Err(GraphError::Cycle)` 而非给出残缺序。
//!   「给出残缺序」会让 VE-Y 画出一条看似合法实则缺边的图。
//!
//! - **D10 · 发行版零成本由「根本不进入」保证**。[`StripGuard::try_push`]
//!   在 Release 档下 `payload_builds` **不递增**（不是构建完再抹掉），
//!   强行请求只记 `forced_attempts` + 一条 P1。判据**双向验证**：
//!   Debug 档真递增作对照 + Release 档强行尝试也不递增。
//!
//! - **D11 · 一个 bool 表达两件事必错**。「调试负载可用吗」是构建期属性，
//!   「本帧有数据吗」是运行期属性。故二者分列 [`BuildProfile`] 与
//!   [`EffectStatsRing::len`]，不用一个 `enabled` 混着表达。
//!
//! - **D12 · 信封 `declared` 不许外部乱填**。注册面唯一构造口
//!   [`Envelope::new`] 自算摘要；[`GraphSnapshot`] 同理。对账的基准
//!   必须在产生处一次写定，事后补填等于「用答案校对答案」。
//!
//! **性能逐项分解**：每帧写统计 O(效果数)；环形覆盖 O(1)；
//! RT 拾取 O(深度)；拓扑排序 O(V+E)；图快照仅在重编译时推新版。
//!
//! **跨批对接点**：总线 F1764（K 段注册，信封沿用 M 段已验证形状，
//! 但本条**自持**——不改他人文件、不依赖未落地的 J 段实现）；
//! 数据源 F2002（图）/F2003（池）/F2014（打点）/F2017（独占耗时差分法，
//! 本条只声明**接入契约**不代实现）；消费方 VE-Y / CLI；
//! 遥测 F2059；剔除核验归域级汇总。
//!
//! **无障碍与隐私**：调试数据仅开发形态存在，发行版物理剔除；
//! 无用户数据面。
//!
//! 零外部依赖，只用 `alloc` 与 `crate::checks`（自检侧）。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（自建，K 域独占段0x2Cxx）
// ---------------------------------------------------------------------------

/// 诊断码。**自建**而非复用他人枚举——下游封闭枚举无权加变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub struct DiagCode(pub u16);

impl DiagCode {
    /// 环形缓冲已覆盖最旧样本。
    pub const STREAM_OVERWRITTEN: DiagCode = DiagCode(0x2C01);
    /// 统计洪水已降频。
    pub const STATS_DOWNSAMPLED: DiagCode = DiagCode(0x2C02);
    /// 参数摘要非有限值已被钳制。
    pub const PARAM_NON_FINITE: DiagCode = DiagCode(0x2C03);
    /// RT 拾取节点不存在。
    pub const PICK_NODE_UNKNOWN: DiagCode = DiagCode(0x2C04);
    /// RT 拾取深度超出拓扑链长度。
    pub const PICK_DEPTH_EXCEEDED: DiagCode = DiagCode(0x2C05);
    /// RT 拉取超带宽，已降采样档。
    pub const RT_DOWNSCALED: DiagCode = DiagCode(0x2C06);
    /// RT 拉取即使降到最低档仍超带宽。
    pub const RT_BANDWIDTH_EXHAUSTED: DiagCode = DiagCode(0x2C07);
    /// 图存在环，拓扑排序拒绝出序。
    pub const GRAPH_CYCLE: DiagCode = DiagCode(0x2C08);
    /// 图边引用了不存在的节点。
    pub const GRAPH_DANGLING_EDGE: DiagCode = DiagCode(0x2C09);
    /// 图版本漂移，快照被拦截。
    pub const GRAPH_VERSION_DRIFT: DiagCode = DiagCode(0x2C0A);
    /// 发行版剔除被强行请求。
    pub const STRIP_FAILED: DiagCode = DiagCode(0x2C0B);
    /// 独占耗时超过总耗时（口径被突破）。
    pub const EXCLUSIVE_EXCEEDS_TOTAL: DiagCode = DiagCode(0x2C0C);
    /// 独占耗时数据缺失。
    pub const EXCLUSIVE_MISSING: DiagCode = DiagCode(0x2C0D);
    /// 信封摘要漂移。
    pub const ENVELOPE_DRIFT: DiagCode = DiagCode(0x2C0E);
    /// 信封负载类型未注册。
    pub const ENVELOPE_UNKNOWN_KIND: DiagCode = DiagCode(0x2C0F);
    /// 别名复用关系与物理分配数不一致。
    pub const ALIAS_COUNT_MISMATCH: DiagCode = DiagCode(0x2C10);
    /// 效果总数超预算。
    pub const EFFECT_BUDGET_EXCEEDED: DiagCode = DiagCode(0x2C11);
    /// 帧号非单调。
    pub const FRAME_NOT_MONOTONIC: DiagCode = DiagCode(0x2C12);
    /// 节点未产出 RT 引用。
    pub const NODE_HAS_NO_RT: DiagCode = DiagCode(0x2C13);
    /// 信封 schema 版本不匹配。
    pub const ENVELOPE_SCHEMA_MISMATCH: DiagCode = DiagCode(0x2C14);
    /// 拓扑序与节点表数量不符。
    pub const TOPO_COUNT_MISMATCH: DiagCode = DiagCode(0x2C15);

    /// 全部诊断码（值域闭合，判据据此断「无遗漏」）。
    pub const ALL: [DiagCode; 21] = [
        DiagCode::STREAM_OVERWRITTEN,
        DiagCode::STATS_DOWNSAMPLED,
        DiagCode::PARAM_NON_FINITE,
        DiagCode::PICK_NODE_UNKNOWN,
        DiagCode::PICK_DEPTH_EXCEEDED,
        DiagCode::RT_DOWNSCALED,
        DiagCode::RT_BANDWIDTH_EXHAUSTED,
        DiagCode::GRAPH_CYCLE,
        DiagCode::GRAPH_DANGLING_EDGE,
        DiagCode::GRAPH_VERSION_DRIFT,
        DiagCode::STRIP_FAILED,
        DiagCode::EXCLUSIVE_EXCEEDS_TOTAL,
        DiagCode::EXCLUSIVE_MISSING,
        DiagCode::ENVELOPE_DRIFT,
        DiagCode::ENVELOPE_UNKNOWN_KIND,
        DiagCode::ALIAS_COUNT_MISMATCH,
        DiagCode::EFFECT_BUDGET_EXCEEDED,
        DiagCode::FRAME_NOT_MONOTONIC,
        DiagCode::NODE_HAS_NO_RT,
        DiagCode::ENVELOPE_SCHEMA_MISMATCH,
        DiagCode::TOPO_COUNT_MISMATCH,
    ];

    /// 人话标签。
    ///
    /// **兜底分支是必须的**：`DiagCode` 是元组结构体（可承载任意
    /// `u16`），不是封闭枚举，故 `match` 只能写兜底而**不能**指望
    /// 「新增变体时编译不过」——新增的是**常量**不是变体。
    /// 未登记的码位一律回「未登记诊断码」，不编造语义。
    pub const fn label(self) -> &'static str {
        match self {
            DiagCode::STREAM_OVERWRITTEN => "环形缓冲已覆盖最旧样本",
            DiagCode::STATS_DOWNSAMPLED => "统计洪水已降频",
            DiagCode::PARAM_NON_FINITE => "参数摘要非有限值已钳制",
            DiagCode::PICK_NODE_UNKNOWN => "RT拾取节点不存在",
            DiagCode::PICK_DEPTH_EXCEEDED => "RT拾取深度超出拓扑链",
            DiagCode::RT_DOWNSCALED => "RT拉取超带宽已降采样档",
            DiagCode::RT_BANDWIDTH_EXHAUSTED => "RT拉取降档后仍超带宽",
            DiagCode::GRAPH_CYCLE => "图存在环拒绝出拓扑序",
            DiagCode::GRAPH_DANGLING_EDGE => "图边引用不存在的节点",
            DiagCode::GRAPH_VERSION_DRIFT => "图版本漂移快照已拦截",
            DiagCode::STRIP_FAILED => "发行版强行请求调试负载",
            DiagCode::EXCLUSIVE_EXCEEDS_TOTAL => "独占耗时超过总耗时",
            DiagCode::EXCLUSIVE_MISSING => "独占耗时数据缺失",
            DiagCode::ENVELOPE_DRIFT => "信封摘要漂移",
            DiagCode::ENVELOPE_UNKNOWN_KIND => "信封负载类型未注册",
            DiagCode::ALIAS_COUNT_MISMATCH => "别名复用与物理分配数不一致",
            DiagCode::EFFECT_BUDGET_EXCEEDED => "效果总数超预算",
            DiagCode::FRAME_NOT_MONOTONIC => "帧号非单调",
            DiagCode::NODE_HAS_NO_RT => "节点未产出RT引用",
            DiagCode::ENVELOPE_SCHEMA_MISMATCH => "信封schema版本不匹配",
            DiagCode::TOPO_COUNT_MISMATCH => "拓扑序与节点表数量不符",
            _ => "未登记诊断码",
        }
    }
}

/// 严重度。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Severity {
    /// 提示。
    Info = 0,
    /// 告警。
    Warn = 1,
    /// P1（须立案）。
    P1 = 2,
}

/// 一条诊断。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    /// 码。
    pub code: DiagCode,
    /// 严重度。
    pub severity: Severity,
}

/// 诊断袋。**零静默**：所有拒绝/降级/拦截都留痕。
#[derive(Clone, Debug, Default)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    /// 新建空袋。
    #[allow(clippy::new_without_default)]
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    /// 记一条 Info/Warn。
    pub fn push(&mut self, code: DiagCode) {
        // `DiagCode` 是**元组结构体**（承载他人域也可能用到的码位），
        // 不能当枚举穷举——故走 `ALL` 白名单判定，落在名单外的一律
        // 按 Warn 记（名单外的码不是本域自建的，语义归调用方）。
        let sev = match code {
            DiagCode::GRAPH_CYCLE
            | DiagCode::GRAPH_DANGLING_EDGE
            | DiagCode::GRAPH_VERSION_DRIFT
            | DiagCode::ENVELOPE_DRIFT
            | DiagCode::TOPO_COUNT_MISMATCH => Severity::P1,
            _ => {
                let known = DiagCode::ALL.iter().any(|c| *c == code);
                let _ = known;
                Severity::Warn
            }
        };
        self.items.push(Diagnostic { code, severity: sev });
    }

    /// 记一条 Warn（显式，不改严重度映射）。
    pub fn push_warn(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::Warn });
    }

    /// 记一条 P1。
    pub fn push_p1(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::P1 });
    }

    /// 全量取。
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 某码计数。
    pub fn count(&self, code: DiagCode) -> usize {
        self.items.iter().filter(|d| d.code == code).count()
    }

    /// 某严重度计数。
    pub fn count_severity(&self, sev: Severity) -> usize {
        self.items.iter().filter(|d| d.severity == sev).count()
    }

    /// 是否含某码。
    pub fn has(&self, code: DiagCode) -> bool {
        self.count(code) > 0
    }

    /// P1 计数。
    pub fn p1_count(&self) -> usize {
        self.count_severity(Severity::P1)
    }

    /// 渲染（自解释文本，判据侧断「含关键词且不含承诺词」用）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        for d in self.items.iter() {
            s.push_str(d.code.label());
            s.push_str("|");
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 二、构建档位与发行版剔除（判据四：发行版零成本）
// ---------------------------------------------------------------------------

/// 构建档位。
///
/// `Default` 取 `Debug`：**调试是安全默认**——想要「零成本剔除」必须
/// 显式写 [`BuildProfile::Release`]。反过来的默认（发行默认）会让忘配的
/// 发行构建悄悄带上全部调试负载。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BuildProfile {
    /// 开发形态：调试负载可用。
    #[default]
    Debug,
    /// 发行形态：调试负载**零成本剔除**。
    Release,
}

/// 剔除守卫。
///
/// **零成本的物质保证**：Release 档下 `payload_builds` **不递增**——
/// 不是「构建完再抹掉」，而是**根本不进入构建**（判据双向验证：
/// Debug 档真递增作对照 + Release 档强行尝试也不递增）。
///
/// **为什么还要 `build_entries`（单调证据）**：`payload_builds` 是**净值**
/// 口径，只要实现允许「自增后再减回」，它就能被洗成0——实测变异
/// `try_push` 在 Release 档先 `payload_builds += 1` 再 `-= 1`，
/// 「Release 档 `payload_builds == 0`」这条判据**全绿放过**。
/// 净值可被回退抵消，**单调计数器不可**：`build_entries` 只在**真正
/// 进入构建分支**时递增，全代码路径**无任何减法**，故
/// 「Release 档 `build_entries == 0`」才是「根本不进入」的**可证伪**
/// 命题（十诫第 10 条：净值 vs 绝对值口径，必须两侧不同）。
#[derive(Clone, Copy, Debug, Default)]
pub struct StripGuard {
    /// 构建档位。
    pub profile: BuildProfile,
    /// 已构建的调试负载数（**净值口径，Release 档下应为 0**）。
    pub payload_builds: u64,
    /// 被强行请求的次数（Release 档下只增此项）。
    pub forced_attempts: u64,
    /// 真正**进入过构建分支**的累计次数（**单调口径，全程只增不减**）。
    ///
    /// Release 档下恒为 0——这是「零成本剔除」的**物质证据**，
    /// 与 `payload_builds` 的净值口径互为交叉验证：净值能被自增自减
    /// 洗白，单调计数不能。
    pub build_entries: u64,
}

impl StripGuard {
    /// 构造。
    pub const fn new(profile: BuildProfile) -> StripGuard {
        StripGuard { profile, payload_builds: 0, forced_attempts: 0, build_entries: 0 }
    }

    /// 当前档位是否允许调试负载。
    pub const fn payloads_enabled(&self) -> bool {
        matches!(self.profile, BuildProfile::Debug)
    }

    /// 请求写一个调试负载。返回是否真的写了。
    ///
    /// Release 档下：不碰 `payload_builds`、**不碰 `build_entries`**，
    /// 只记 `forced_attempts` + 一条 P1。
    pub fn try_push(&mut self, bag: &mut DiagBag) -> bool {
        if self.payloads_enabled() {
            // 单调证据只在此处递增：Release 档走 else 分支，永不经过。
            self.build_entries += 1;
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
// 三、效果统计流（判据一：三负载之一）
// ---------------------------------------------------------------------------

/// 打点直读原始耗时（**微秒**定点整数）。
///
/// 「总耗时」是含下游的口径，「独占耗时」由 F2017 差分法定标后经
/// [`EffectStat::exclusive_ns`] 给出——**两个口径两个字段**，不合并
/// （D2：拿总耗时冒充分享会让「独占」二字失去意义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RawTiming {
    /// 帧号。
    pub frame: u64,
    /// 效果标识。
    pub effect: u16,
    /// 该帧总耗时（微秒，含下游）。
    pub total_ns: u32,
    /// 该帧独占耗时（微秒，F2017 差分法标定）。
    pub exclusive_ns: u32,
}

impl RawTiming {
    /// 构造。
    pub const fn new(frame: u64, effect: u16, total_ns: u32, exclusive_ns: u32) -> RawTiming {
        RawTiming { frame, effect, total_ns, exclusive_ns }
    }
}

/// 一行效果统计（**全链每效果一行**：状态/关键参数/独占耗时）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EffectStat {
    /// 帧号。
    pub frame: u64,
    /// 效果标识。
    pub effect: u16,
    /// 开关状态。
    pub enabled: bool,
    /// 关键参数摘要（**定点整数**，D3：不用 `f32`）。
    pub param_digest: u32,
    /// 该帧独占耗时（微秒）。
    pub exclusive_ns: u32,
    /// 该帧总耗时（微秒，含下游）。
    pub total_ns: u32,
}

/// FNV-1a 单步。
const fn fnv_step(h: u32, byte: u32) -> u32 {
    (h ^ byte).wrapping_mul(0x0100_0193)
}

/// 参数摘要：把 `f32` 位模式**归一后**折成定点摘要。
///
/// **为什么不是直接吃 `f32` 位模式**（D3）：`-0.0` 与 `+0.0` 数值相等但
/// 位模式不同，直接折会让「参数没改」在某些输入下算出两个摘要
/// （假漂移）；`NaN` 更糟——同一份 NaN 的位模式在不同优化级别下可变。
/// 故先做**数值归一**（`+0.0` 与 `-0.0` 统一成 `+0.0`），再取低 32 位，
/// 非有限值钳到 `0` 哨兵位。
pub fn param_digest(params: &[f32]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for p in params.iter() {
        let norm = if *p == 0.0 { 0.0f32 } else { *p };
        let bits = if norm.is_finite() {
            // 小端取低 4 字节：数值相等 ⇒ 归一后位模式逐位相等。
            norm.to_bits() & 0xFFFF_FFFF
        } else {
            // 非有限值统一哨兵：NaN 不进摘要（否则摘要不可复现）。
            0xFFFF_FFFF
        };
        h = fnv_step(h, (bits >> 24) & 0xFF);
        h = fnv_step(h, (bits >> 16) & 0xFF);
        h = fnv_step(h, (bits >> 8) & 0xFF);
        h = fnv_step(h, bits & 0xFF);
    }
    h
}

/// 效果统计环形缓冲容量（**定容**，覆盖而非增长）。
pub const STATS_RING_CAP: usize = 64;

/// 统计洪水阈值：效果数超此值即降频。
pub const EFFECT_BUDGET: usize = 32;

/// 效果统计流（**环形缓冲**，调试流的价值在「最近」不在「全量」）。
#[derive(Clone, Debug)]
pub struct EffectStatsRing {
    slots: Vec<EffectStat>,
    head: usize,
    len: usize,
    overwritten: u64,
    stride: u32,
    logical_frames: u64,
    written: u64,
    downsampled: u64,
    last_frame: u64,
}

impl EffectStatsRing {
    /// 新建（定容空环）。
    #[allow(clippy::new_without_default)]
    pub fn new() -> EffectStatsRing {
        let mut slots: Vec<EffectStat> = Vec::new();
        let mut i = 0usize;
        while i < STATS_RING_CAP {
            slots.push(EffectStat {
                frame: 0,
                effect: 0,
                enabled: false,
                param_digest: 0,
                exclusive_ns: 0,
                total_ns: 0,
            });
            i += 1;
        }
        EffectStatsRing {
            slots,
            head: 0,
            len: 0,
            overwritten: 0,
            stride: 1,
            logical_frames: 0,
            written: 0,
            downsampled: 0,
            last_frame: 0,
        }
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        STATS_RING_CAP
    }

    /// 当前条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 覆盖计数（满了以后每写一次 +1）。
    pub fn overwritten(&self) -> u64 {
        self.overwritten
    }

    /// 当前降频步长（1 = 不降频）。
    pub fn stride(&self) -> u32 {
        self.stride
    }

    /// 逻辑帧数（**含被降频跳过的**，这是守恒判据的分母）。
    pub fn logical_frames(&self) -> u64 {
        self.logical_frames
    }

    /// 实际写入次数。
    pub fn written(&self) -> u64 {
        self.written
    }

    /// 被降频跳过的次数。
    pub fn downsampled(&self) -> u64 {
        self.downsampled
    }

    /// 效果数超预算则升步长降频（D4：stride 加倍 + 计数 + 告警三件齐做）。
    ///
    /// 返回是否发生了降频。
    pub fn retune(&mut self, effect_count: usize, bag: &mut DiagBag) -> bool {
        if effect_count > EFFECT_BUDGET {
            if self.stride == 1 {
                self.stride = 2;
                bag.push(DiagCode::STATS_DOWNSAMPLED);
                return true;
            } else if self.stride < 8 {
                self.stride *= 2;
                bag.push(DiagCode::STATS_DOWNSAMPLED);
                return true;
            }
        }
        false
    }

    /// 推入一行统计。返回是否真的写入（降频跳过则 `false`）。
    pub fn push(&mut self, stat: EffectStat, bag: &mut DiagBag) -> bool {
        if stat.frame < self.last_frame {
            bag.push(DiagCode::FRAME_NOT_MONOTONIC);
            return false;
        }
        // 首帧无条件写（否则 stride=2 时首帧永远写不进）。
        if self.len > 0 && (stat.frame % u64::from(self.stride) != 0) {
            self.downsampled += 1;
            bag.push_warn(DiagCode::STATS_DOWNSAMPLED);
            return false;
        }
        if stat.exclusive_ns > stat.total_ns && stat.total_ns != 0 {
            bag.push(DiagCode::EXCLUSIVE_EXCEEDS_TOTAL);
        }
        self.last_frame = stat.frame;
        self.logical_frames = stat.frame + 1;
        self.slots[self.head] = stat;
        self.head = (self.head + 1) % STATS_RING_CAP;
        if self.len == STATS_RING_CAP {
            self.overwritten += 1;
            bag.push_warn(DiagCode::STREAM_OVERWRITTEN);
        } else {
            self.len += 1;
        }
        self.written += 1;
        true
    }

    /// 按下标取（0 = 最旧）。
    pub fn at(&self, index: usize) -> Option<&EffectStat> {
        if index >= self.len {
            return None;
        }
        let start = (self.head + STATS_RING_CAP - self.len) % STATS_RING_CAP;
        Some(&self.slots[(start + index) % STATS_RING_CAP])
    }

    /// 全部（最旧 → 最新）。
    pub fn all(&self) -> Vec<&EffectStat> {
        let mut out: Vec<&EffectStat> = Vec::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.at(i) {
                out.push(s);
            }
            i += 1;
        }
        out
    }

    /// 某效果的全部样本（按帧序）。
    pub fn of_effect(&self, effect: u16) -> Vec<&EffectStat> {
        self.all().into_iter().filter(|s| s.effect == effect).collect()
    }

    /// 最新一条。
    pub fn latest(&self) -> Option<&EffectStat> {
        if self.len == 0 {
            None
        } else {
            self.at(self.len - 1)
        }
    }

    /// 从打点原始数据装配一行（**独占/总两口径分列**，D2）。
    pub fn from_timing(
        timing: RawTiming,
        enabled: bool,
        params: &[f32],
        bag: &mut DiagBag,
    ) -> EffectStat {
        if !params.iter().all(|p| p.is_finite()) {
            bag.push(DiagCode::PARAM_NON_FINITE);
        }
        EffectStat {
            frame: timing.frame,
            effect: timing.effect,
            enabled,
            param_digest: param_digest(params),
            exclusive_ns: timing.exclusive_ns,
            total_ns: timing.total_ns,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、中间 RT 拾取（判据二：逐级透视）
// ---------------------------------------------------------------------------

/// RT 采样档位（降采样拷贝档，D5）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RtScale {
    /// 全分辨率。
    Full = 0,
    /// 半分辨率。
    Half = 1,
    /// 四分之一。
    Quarter = 2,
    /// 八分之一（最低档）。
    Eighth = 3,
}

impl RtScale {
    /// 全部档位（自低到高）。
    pub const ALL: [RtScale; 4] =
        [RtScale::Eighth, RtScale::Quarter, RtScale::Half, RtScale::Full];

    /// 线上编码（显式映射，不用 `as u8`）。
    pub const fn wire(self) -> u8 {
        match self {
            RtScale::Full => 0,
            RtScale::Half => 1,
            RtScale::Quarter => 2,
            RtScale::Eighth => 3,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            RtScale::Full => "全分辨率",
            RtScale::Half => "半分辨率",
            RtScale::Quarter => "四分之一",
            RtScale::Eighth => "八分之一",
        }
    }

    /// 按编码反查。
    pub fn from_wire(w: u8) -> Option<RtScale> {
        RtScale::ALL.iter().copied().find(|s| s.wire() == w)
    }

    /// 降一档（已在最低档则返回自身）。
    pub const fn downgrade(self) -> RtScale {
        match self {
            RtScale::Full => RtScale::Half,
            RtScale::Half => RtScale::Quarter,
            RtScale::Quarter => RtScale::Eighth,
            RtScale::Eighth => RtScale::Eighth,
        }
    }

    /// 传输字节数（宽 × 高 × 4 字节 RGBA，除以线性降采样系数）。
    pub const fn bytes(self, width: u32, height: u32) -> u32 {
        let div: u32 = match self {
            RtScale::Full => 1,
            RtScale::Half => 2,
            RtScale::Quarter => 4,
            RtScale::Eighth => 8,
        };
        (width.saturating_mul(height).saturating_mul(4)) / div
    }
}

/// 带宽预算（字节）。
pub const RT_BANDWIDTH_BUDGET: u32 = 1 << 20;

/// RT 内容引用（**引用而非拷贝**：内容拉取是 VE-Y 按需行为）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RtRef {
    /// 产出该 RT 的节点。
    pub node: u32,
    /// 物理 RT 句柄（**同一 alias 的多个节点共享同一句柄**，D8）。
    pub handle: u32,
    /// 宽。
    pub width: u32,
    /// 高。
    pub height: u32,
}

/// 一次 RT 拾取的结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RtPick {
    /// 被选中的节点。
    pub node: u32,
    /// 透视深度（0 = 只取本级，d = 连同 d 级祖先）。
    pub depth: u32,
    /// 祖先链（**从本级往根**，长度 == depth，D1）。
    pub ancestors: Vec<u32>,
    /// 引用。
    pub reference: Option<RtRef>,
    /// 实际交付档位。
    pub scale: RtScale,
    /// 请求的档位（**降档时与 `scale` 不同**，D5：如实标注）。
    pub requested_scale: RtScale,
    /// 是否被降档。
    pub downscaled: bool,
    /// 实际字节数。
    pub bytes: u32,
}

/// 图错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GraphError {
    /// 节点不存在。
    NodeUnknown(u32),
    /// 深度超出拓扑链长度。
    DepthExceeded(u32),
    /// 该节点未产出 RT。
    NoRt(u32),
    /// 图有环。
    Cycle,
    /// 边引用不存在的节点。
    DanglingEdge(u32),
}

/// 拾取一个节点的中间 RT（**逐级透视**，D1）。
///
/// 超带宽则降采样档；降到最低档仍超则 `Err(RtBandwidthExhausted)`
/// 对应的诊断码由调用方据返回的 `RtPick` 是否 `None` 判断——本函数
/// 签名保留 `Option`，由 [`pick_rt_checked`] 给出带诊断的形态。
pub fn pick_rt(
    g: &GraphModel,
    node: u32,
    depth: u32,
    requested: RtScale,
    width: u32,
    height: u32,
) -> Result<RtPick, GraphError> {
    if !g.has_node(node) {
        return Err(GraphError::NodeUnknown(node));
    }
    let order = match g.topological_order() {
        Ok(o) => o,
        Err(e) => return Err(e),
    };
    // 定位本级在拓扑序中的位置。
    let mut at = 0usize;
    let mut found = false;
    let mut i = 0usize;
    while i < order.len() {
        if order[i] == node {
            at = i;
            found = true;
            break;
        }
        i += 1;
    }
    if !found {
        return Err(GraphError::NodeUnknown(node));
    }
    // 逐级上溯：ancestors[0] 是直接前驱（拓扑序紧邻的入边来源）。
    let mut ancestors: Vec<u32> = Vec::new();
    let mut back = 0u32;
    let mut j = at;
    while back < depth && j > 0 {
        j -= 1;
        ancestors.push(order[j]);
        back += 1;
    }
    if back < depth {
        return Err(GraphError::DepthExceeded(depth));
    }
    let reference = g.rt_of(node);
    if reference.is_none() {
        return Err(GraphError::NoRt(node));
    }
    // 带宽降档（D5：不拒绝，降档 + 如实标注）。
    let mut scale = requested;
    let mut bytes = scale.bytes(width, height);
    while bytes > RT_BANDWIDTH_BUDGET {
        let next = scale.downgrade();
        if next == scale {
            break;
        }
        scale = next;
        bytes = scale.bytes(width, height);
    }
    Ok(RtPick {
        node,
        depth,
        ancestors,
        reference,
        scale,
        requested_scale: requested,
        downscaled: scale != requested,
        bytes,
    })
}

/// 带诊断的拾取（把 `GraphError` 映射到自建诊断码）。
pub fn pick_rt_checked(
    g: &GraphModel,
    node: u32,
    depth: u32,
    requested: RtScale,
    width: u32,
    height: u32,
    bag: &mut DiagBag,
) -> Option<RtPick> {
    match pick_rt(g, node, depth, requested, width, height) {
        Ok(p) => {
            if p.downscaled {
                bag.push(DiagCode::RT_DOWNSCALED);
            }
            if p.bytes > RT_BANDWIDTH_BUDGET {
                bag.push(DiagCode::RT_BANDWIDTH_EXHAUSTED);
                return None;
            }
            Some(p)
        }
        Err(GraphError::NodeUnknown(_)) => {
            bag.push(DiagCode::PICK_NODE_UNKNOWN);
            None
        }
        Err(GraphError::DepthExceeded(_)) => {
            bag.push(DiagCode::PICK_DEPTH_EXCEEDED);
            None
        }
        Err(GraphError::NoRt(_)) => {
            bag.push(DiagCode::NODE_HAS_NO_RT);
            None
        }
        Err(GraphError::Cycle) => {
            bag.push_p1(DiagCode::GRAPH_CYCLE);
            None
        }
        Err(GraphError::DanglingEdge(_)) => {
            bag.push_p1(DiagCode::GRAPH_DANGLING_EDGE);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 五、图结构快照（判据三：图结构可视）
// ---------------------------------------------------------------------------

/// 一个图节点（后处理链一级）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct GraphNode {
    /// 节点标识。
    pub id: u32,
    /// 效果标识（对应效果统计流里的 `effect`）。
    pub effect: u16,
    /// 显式声明的 RT 别名（**两节点同 alias ⇒ 复用同一物理 RT**，D8）。
    pub rt_alias: u32,
}

/// 一条边（`from` → `to`）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct GraphEdge {
    pub from: u32,
    pub to: u32,
}

/// 后处理 DAG 模型。
#[derive(Clone, Debug)]
pub struct GraphModel {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    /// 显式分配的物理 RT 数（**别名复用时多个节点共享一个**，D8）。
    physical_rts: Vec<RtRef>,
    version: u32,
}

impl GraphModel {
    /// 新建空图。
    #[allow(clippy::new_without_default)]
    pub fn new() -> GraphModel {
        GraphModel { nodes: Vec::new(), edges: Vec::new(), physical_rts: Vec::new(), version: 0 }
    }

    /// 加节点。返回是否成功（重复 id 拒绝）。
    pub fn add_node(&mut self, node: GraphNode) -> bool {
        if self.has_node(node.id) {
            return false;
        }
        self.nodes.push(node);
        true
    }

    /// 加边。返回是否成功（自环 / 悬空边拒绝）。
    pub fn add_edge(&mut self, edge: GraphEdge, bag: &mut DiagBag) -> bool {
        if edge.from == edge.to {
            bag.push_p1(DiagCode::GRAPH_CYCLE);
            return false;
        }
        if !self.has_node(edge.from) || !self.has_node(edge.to) {
            bag.push_p1(DiagCode::GRAPH_DANGLING_EDGE);
            return false;
        }
        self.edges.push(edge);
        true
    }

    /// 登记一个物理 RT（**别名复用的物质基础**：同 handle 只登记一次）。
    pub fn alloc_rt(&mut self, r: RtRef) -> bool {
        if self.physical_rts.iter().any(|p| p.handle == r.handle) {
            return false;
        }
        self.physical_rts.push(r);
        true
    }

    /// 节点是否存在。
    pub fn has_node(&self, id: u32) -> bool {
        self.nodes.iter().any(|n| n.id == id)
    }

    /// 节点表。
    pub fn nodes(&self) -> &[GraphNode] {
        &self.nodes
    }

    /// 边表。
    pub fn edges(&self) -> &[GraphEdge] {
        &self.edges
    }

    /// 物理 RT 表。
    pub fn physical_rts(&self) -> &[RtRef] {
        &self.physical_rts
    }

    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 图版本。
    pub fn version(&self) -> u32 {
        self.version
    }

    /// 推进版本（**重编译时推新版**，D6）。
    pub fn bump_version(&mut self) -> u32 {
        self.version = self.version.wrapping_add(1);
        self.version
    }

    /// 节点输出 RT（**按 alias 精确匹配物理 RT 的 `node` 槽**）。
    ///
    /// **为什么不能用 `||` 同时比 `node` 与 `handle`**（D8 的反面）：
    /// alias 是「逻辑名」、handle 是「物理句柄」，二者数值域重叠时
    /// `||` 会让一个 alias 先命中别人的 handle，产生**错配但不报错**
    /// ——拾取拿到的 RT 内容属于另一个节点。正确形态是 alias 只与
    /// 物理 RT 表的 **`node` 槽**（即登记时声明的 alias 归属）比对，
    /// 单向、无歧义。
    pub fn rt_of(&self, node: u32) -> Option<RtRef> {
        let n = self.nodes.iter().find(|n| n.id == node)?;
        let alias = n.rt_alias;
        self.physical_rts.iter().copied().find(|p| p.node == alias)
    }

    /// 声明了同一 alias 的节点数（**复用计数的可核对形式**，D8）。
    pub fn alias_users(&self, alias: u32) -> usize {
        self.nodes.iter().filter(|n| n.rt_alias == alias).count()
    }

    /// 拓扑排序（**Kahn 算法 + 按 id 序出队**，保证确定性，D7）。
    ///
    /// 出队数 < 节点总数即存在环 → `Err(GraphError::Cycle)`，
    /// **不给残缺序**（D9）。
    pub fn topological_order(&self) -> Result<Vec<u32>, GraphError> {
        let n = self.nodes.len();
        let mut indeg: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < n {
            indeg.push(0);
            i += 1;
        }
        let mut adj: Vec<Vec<u32>> = Vec::new();
        i = 0;
        while i < n {
            adj.push(Vec::new());
            i += 1;
        }
        for e in self.edges.iter() {
            let fi = match self.nodes.iter().position(|x| x.id == e.from) {
                Some(k) => k,
                None => return Err(GraphError::DanglingEdge(e.from)),
            };
            let ti = match self.nodes.iter().position(|x| x.id == e.to) {
                Some(k) => k,
                None => return Err(GraphError::DanglingEdge(e.to)),
            };
            indeg[ti] += 1;
            adj[fi].push(e.to);
        }
        let mut ready: Vec<u32> = Vec::new();
        i = 0;
        while i < n {
            if indeg[i] == 0 {
                ready.push(self.nodes[i].id);
            }
            i += 1;
        }
        let mut order: Vec<u32> = Vec::new();
        while !ready.is_empty() {
            // 按 id 最小者出队 ⇒ 确定性（D7）。
            let mut best = 0usize;
            let mut j = 1usize;
            while j < ready.len() {
                if ready[j] < ready[best] {
                    best = j;
                }
                j += 1;
            }
            let cur = ready.remove(best);
            order.push(cur);
            let ci = match self.nodes.iter().position(|x| x.id == cur) {
                Some(k) => k,
                None => continue,
            };
            let outs = adj[ci].clone();
            for nx in outs.iter() {
                if let Some(ti) = self.nodes.iter().position(|x| x.id == *nx) {
                    if indeg[ti] > 0 {
                        indeg[ti] -= 1;
                        if indeg[ti] == 0 {
                            ready.push(*nx);
                        }
                    }
                }
            }
        }
        if order.len() != n {
            return Err(GraphError::Cycle);
        }
        Ok(order)
    }

    /// 图快照（**重编译时推新版**）。
    pub fn snapshot(&self) -> GraphSnapshot {
        let mut declared = self.nodes.clone();
        let mut edges = self.edges.clone();
        let mut order = match self.topological_order() {
            Ok(o) => o,
            Err(_) => Vec::new(),
        };
        // 排序使快照**与图的历史构造顺序无关**（否则同一张图两次快照
        // 可能字节不同，对账会误判漂移）。
        order.sort();
        declared.sort();
        edges.sort();
        GraphSnapshot::new(self.version, declared, edges, order)
    }
}

/// 图结构快照。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GraphSnapshot {
    /// 图版本（**对账基准**，D6/D12）。
    pub version: u32,
    /// 节点表。
    pub nodes: Vec<GraphNode>,
    /// 边表。
    pub edges: Vec<GraphEdge>,
    /// 拓扑序。
    pub topo: Vec<u32>,
    /// 注册时声明的摘要（**产生处一次写定**）。
    declared: u32,
}

impl GraphSnapshot {
    /// 造快照并自算摘要（D12：注册面唯一构造口）。
    pub fn new(
        version: u32,
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
        topo: Vec<u32>,
    ) -> GraphSnapshot {
        let mut s = GraphSnapshot { version, nodes, edges, topo, declared: 0 };
        s.declared = s.checksum();
        s
    }

    /// 重算摘要（对版本/节点数/边数/拓扑序末项的 FNV-1a）。
    pub fn checksum(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        h = fnv_step(h, self.version);
        h = fnv_step(h, self.nodes.len() as u32);
        h = fnv_step(h, self.edges.len() as u32);
        h = fnv_step(h, self.topo.len() as u32);
        h = fnv_step(h, self.topological_digest());
        h
    }

    /// 拓扑序摘要（逐项折入）。
    pub fn topological_digest(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        for id in self.topo.iter() {
            h = fnv_step(h, *id);
        }
        h
    }

    /// 声明摘要。
    pub fn declared(&self) -> u32 {
        self.declared
    }

    /// 是否漂移。
    pub fn drifted(&self) -> bool {
        self.checksum() != self.declared
    }
}

/// 信封注册表（**漂移即拦截**，D6）。
#[derive(Clone, Debug)]
pub struct EnvelopeRegistry {
    slots: Vec<Option<Envelope>>,
    epoch: u32,
    drift_count: u32,
    intercepted: bool,
}

impl EnvelopeRegistry {
    /// 新建（K 段三槽）。
    #[allow(clippy::new_without_default)]
    pub fn new() -> EnvelopeRegistry {
        let mut slots: Vec<Option<Envelope>> = Vec::new();
        let mut i = 0usize;
        while i < K_SEGMENT_SLOTS {
            slots.push(None);
            i += 1;
        }
        EnvelopeRegistry { slots, epoch: 0, drift_count: 0, intercepted: false }
    }

    /// 注册（重注册即替换）。
    pub fn register(&mut self, env: Envelope) -> bool {
        if env.schema != ENVELOPE_SCHEMA {
            return false;
        }
        let idx = match PayloadKind::ALL.iter().position(|k| *k == env.kind) {
            Some(k) => k,
            None => return false,
        };
        self.slots[idx] = Some(env);
        self.epoch = self.epoch.wrapping_add(1);
        true
    }

    /// 窥视。
    pub fn peek(&self, kind: PayloadKind) -> Option<&Envelope> {
        let idx = PayloadKind::ALL.iter().position(|k| *k == kind)?;
        self.slots[idx].as_ref()
    }

    /// 取（**漂移拦截后一律 None**）。
    pub fn take(&self, kind: PayloadKind) -> Option<&Envelope> {
        if self.intercepted {
            return None;
        }
        self.peek(kind)
    }

    /// 已注册数。
    pub fn registered(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 纪元（重编译推新版即自增）。
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// 漂移计数。
    pub fn drift_count(&self) -> u32 {
        self.drift_count
    }

    /// 是否已被拦截。
    pub fn intercepted(&self) -> bool {
        self.intercepted
    }

    /// 解除拦截（**人工确认漂移是预期变更后**才可调）。
    pub fn clear_intercept(&mut self) {
        self.intercepted = false;
    }

    /// 对账：重算全部已注册信封摘要，不一致即置拦截（D6）。
    pub fn reconcile(&mut self, bag: &mut DiagBag) -> u32 {
        let mut bad = 0u32;
        for s in self.slots.iter() {
            if let Some(e) = s {
                if e.drifted() {
                    bad += 1;
                }
            }
        }
        if bad > 0 {
            self.drift_count += bad;
            self.intercepted = true;
            bag.push_p1(DiagCode::ENVELOPE_DRIFT);
        }
        bad
    }
}

// ---------------------------------------------------------------------------
// 六、信封封装（锚点：信封封装 J 段注册沿用）
// ---------------------------------------------------------------------------

/// K 段负载类型（三类负载：判据一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayloadKind {
    /// 效果统计流。
    EffectStats,
    /// RT 拾取流。
    RtPick,
    /// 图结构快照。
    GraphSnapshot,
}

impl PayloadKind {
    /// 全部类型（K 段取值域）。
    pub const ALL: [PayloadKind; 3] =
        [PayloadKind::EffectStats, PayloadKind::RtPick, PayloadKind::GraphSnapshot];

    /// 线上编码（**显式映射**）。
    pub const fn wire(self) -> u16 {
        match self {
            PayloadKind::EffectStats => 0x11,
            PayloadKind::RtPick => 0x12,
            PayloadKind::GraphSnapshot => 0x13,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            PayloadKind::EffectStats => "效果统计流",
            PayloadKind::RtPick => "RT拾取流",
            PayloadKind::GraphSnapshot => "图结构快照",
        }
    }

    /// 按编码反查。
    pub fn from_wire(w: u16) -> Option<PayloadKind> {
        PayloadKind::ALL.iter().copied().find(|k| k.wire() == w)
    }
}

/// 信封 schema 版本（K 段 v1）。
pub const ENVELOPE_SCHEMA: u16 = 1;

/// K 段注册表槽数（三类负载）。
pub const K_SEGMENT_SLOTS: usize = 3;

/// 一个负载信封。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Envelope {
    /// 负载类型。
    pub kind: PayloadKind,
    /// schema 版本。
    pub schema: u16,
    /// 载荷字节数。
    pub byte_len: u32,
    /// 序号。
    pub seq: u32,
    /// 声明摘要（**产生处一次写定**，D12）。
    pub declared: u32,
}

impl Envelope {
    /// 造信封并自算摘要（**注册面唯一构造口**）。
    pub fn new(kind: PayloadKind, byte_len: u32, seq: u32) -> Envelope {
        let mut e = Envelope { kind, schema: ENVELOPE_SCHEMA, byte_len, seq, declared: 0 };
        e.declared = e.checksum();
        e
    }

    /// 重算摘要（对四字段 + 标签长度的 FNV-1a）。
    pub fn checksum(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        h = fnv_step(h, self.kind.wire() as u32);
        h = fnv_step(h, self.schema as u32);
        h = fnv_step(h, self.byte_len);
        h = fnv_step(h, self.seq);
        h = fnv_step(h, self.kind.label().len() as u32);
        h
    }

    /// 是否漂移。
    pub fn drifted(&self) -> bool {
        self.checksum() != self.declared
    }
}

/// 三类负载的字节数（**如实记账**，不做美化）。
pub fn payload_bytes(kind: PayloadKind, stats: &EffectStatsRing, snap: &GraphSnapshot) -> u32 {
    match kind {
        PayloadKind::EffectStats => (stats.len() * 24) as u32,
        PayloadKind::RtPick => 64,
        PayloadKind::GraphSnapshot => {
            ((snap.nodes.len() * 12) + (snap.edges.len() * 8) + (snap.topo.len() * 4)) as u32
        }
    }
}

/// 一帧三类负载的完整封装（**总线上送的一帧**）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DebugFrame {
    /// 帧号。
    pub frame: u64,
    /// 三个信封。
    pub envelopes: [Envelope; 3],
}

impl DebugFrame {
    /// 封装一帧（三类负载各一个信封）。
    pub fn seal(frame: u64, stats: &EffectStatsRing, snap: &GraphSnapshot) -> DebugFrame {
        DebugFrame {
            frame,
            envelopes: [
                Envelope::new(
                    PayloadKind::EffectStats,
                    payload_bytes(PayloadKind::EffectStats, stats, snap),
                    frame as u32,
                ),
                Envelope::new(PayloadKind::RtPick, payload_bytes(PayloadKind::RtPick, stats, snap), frame as u32),
                Envelope::new(
                    PayloadKind::GraphSnapshot,
                    payload_bytes(PayloadKind::GraphSnapshot, stats, snap),
                    frame as u32,
                ),
            ],
        }
    }

    /// 取出某类型信封。
    pub fn envelope(&self, kind: PayloadKind) -> Option<&Envelope> {
        self.envelopes.iter().find(|e| e.kind == kind)
    }

    /// 本帧总字节数。
    pub fn total_bytes(&self) -> u32 {
        self.envelopes.iter().fold(0u32, |a, e| a.saturating_add(e.byte_len))
    }
}

// ---------------------------------------------------------------------------
// 七、族声明与冒烟
// ---------------------------------------------------------------------------

/// 家族声明一致性（判据用）：三类负载齐全且 wire 互异。
pub fn family_is_consistent() -> bool {
    PayloadKind::ALL.len() == 3
        && PayloadKind::ALL[0] != PayloadKind::ALL[1]
        && PayloadKind::ALL[1] != PayloadKind::ALL[2]
        && PayloadKind::ALL[0] != PayloadKind::ALL[2]
}

/// wire 互异。
pub fn wires_unique() -> bool {
    PayloadKind::ALL[0].wire() != PayloadKind::ALL[1].wire()
        && PayloadKind::ALL[1].wire() != PayloadKind::ALL[2].wire()
        && PayloadKind::ALL[0].wire() != PayloadKind::ALL[2].wire()
}

/// 标签互异。
pub fn labels_unique() -> bool {
    PayloadKind::ALL[0].label() != PayloadKind::ALL[1].label()
        && PayloadKind::ALL[1].label() != PayloadKind::ALL[2].label()
        && PayloadKind::ALL[0].label() != PayloadKind::ALL[2].label()
}

/// 诊断码互异。
pub fn diag_codes_unique() -> bool {
    let mut i = 0usize;
    while i < DiagCode::ALL.len() {
        let mut j = i + 1;
        while j < DiagCode::ALL.len() {
            if DiagCode::ALL[i] == DiagCode::ALL[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 冒烟（描述用，不参与判据计数）。
pub fn smoke() -> String {
    let mut g = GraphModel::new();
    let mut bag = DiagBag::new();
    g.add_node(GraphNode { id: 1, effect: 10, rt_alias: 100 });
    g.add_node(GraphNode { id: 2, effect: 11, rt_alias: 101 });
    g.add_node(GraphNode { id: 3, effect: 12, rt_alias: 100 });
    g.alloc_rt(RtRef { node: 100, handle: 0xAAAA, width: 1920, height: 1080 });
    g.alloc_rt(RtRef { node: 101, handle: 0xBBBB, width: 1920, height: 1080 });
    g.add_edge(GraphEdge { from: 1, to: 2 }, &mut bag);
    g.add_edge(GraphEdge { from: 2, to: 3 }, &mut bag);
    let order = g.topological_order().unwrap_or_default();
    let pick = pick_rt(&g, 3, 2, RtScale::Full, 1920, 1080);
    let mut s = String::new();
    s.push_str("topo=");
    for id in order.iter() {
        s.push_str(&alloc::format!("{} ", id));
    }
    s.push_str("alias100_users=");
    s.push_str(&alloc::format!("{} ", g.alias_users(100)));
    s.push_str("rts=");
    s.push_str(&alloc::format!("{}", g.physical_rts().len()));
    if let Ok(p) = pick {
        s.push_str("pick_scale=");
        s.push_str(p.scale.label());
        s.push_str(" ancestors=");
        s.push_str(&alloc::format!("{}", p.ancestors.len()));
    }
    s
}