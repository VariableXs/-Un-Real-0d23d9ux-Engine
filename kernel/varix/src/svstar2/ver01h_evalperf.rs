//! VE-F3408 · 令牌求值性能 —— 求值缓存与批量流水线。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3408`
//!
//! # 职责（锚点原文拆解）
//!
//! - **求值缓存（依赖图增量重算）**：令牌值变了，只重算它的**下游**，
//!   上游与其余令牌直接命中缓存；
//! - **批量求值流水线**：一次请求多个令牌，按拓扑序一次走完，
//!   中间结果在同一批内共享（同一令牌被多条路径依赖时只算一次）；
//! - **求值耗时 P95 承诺**：万级令牌毫秒级完成 ⇒ 单令牌预算有上限，
//!   超预算即进降级矩阵而不是拖垮整批；
//! - **降级矩阵**：缓存失效→重算；批超时→分帧；P95 劣化→告警。
//!
//! # 为什么自己持一份依赖边而不复用 F3403 的 [`DepGraph`]
//!
//! F3403 的图是**引用图**（谁引用谁），语义正确但它是别人的文件。
//! 本单要的是「令牌值变更 ⇒ 哪些令牌必须重算」，方向是**反向**的
//! （下游 = 谁依赖我），且需要按层分帧。把这份映射内联进来的代价是：
//! 一旦 F3403 改图语义，本单的缓存正确性会跟着静默漂移。
//!
//! 所以这里**显式声明**依赖边来源，并把「边集与图的一致性」做成**可验证的契约**
//! （[`EvalCache::verify_edges`]），而不是靠"我是照它抄的"这种口头保证。
//! 契约失配时判 [`PerfCode::GraphDiverged`] 并整体失效重算——宁可慢，不可错。
//!
//! # 零 panic 面
//!
//! `[i]` / `unwrap()` / `expect()` 只出现在 `run_*_checks()` 与 `#[cfg(test)]` 内。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（自建；下游封闭枚举无权加变体）
// ---------------------------------------------------------------------------

/// 求值性能域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfCode {
    /// 令牌路径非法（空或超长）。
    PathInvalid,
    /// 令牌值非法（空或超长）。
    ValueInvalid,
    /// 依赖图含环（求值顺序不存在）。
    CyclicGraph,
    /// 依赖图与缓存记录的边集不一致（契约失配）。
    GraphDiverged,
    /// 依赖边指向不存在的节点。
    EdgeDangling,
    /// 单令牌求值超出预算。
    BudgetExceeded,
    /// 单帧令牌数超出预算。
    FrameTooLarge,
    /// 令牌数超出上限。
    TokenLimit,
    /// 依赖边数超出上限。
    EdgeLimit,
    /// 依赖深度超出上限。
    DepthLimit,
    /// 缓存条目与令牌表长度不等（缓存已失配）。
    CacheShapeStale,
    /// P95 劣化超阈值（告警级，不阻断）。
    P95Degraded,
    /// 预算对齐失败（AD06 对接点）。
    BudgetUnaligned,
}

impl PerfCode {
    /// 全部码（判据据此核对无遗漏）。
    pub const ALL: [PerfCode; 13] = [
        PerfCode::PathInvalid,
        PerfCode::ValueInvalid,
        PerfCode::CyclicGraph,
        PerfCode::GraphDiverged,
        PerfCode::EdgeDangling,
        PerfCode::BudgetExceeded,
        PerfCode::FrameTooLarge,
        PerfCode::TokenLimit,
        PerfCode::EdgeLimit,
        PerfCode::DepthLimit,
        PerfCode::CacheShapeStale,
        PerfCode::P95Degraded,
        PerfCode::BudgetUnaligned,
    ];

    /// 线上短码。
    pub const fn code(self) -> &'static str {
        match self {
            PerfCode::PathInvalid => "E08-PATH-INVALID",
            PerfCode::ValueInvalid => "E08-VALUE-INVALID",
            PerfCode::CyclicGraph => "E08-CYCLIC",
            PerfCode::GraphDiverged => "E08-GRAPH-DIVERGED",
            PerfCode::EdgeDangling => "E08-EDGE-DANGLING",
            PerfCode::BudgetExceeded => "E08-BUDGET-EXCEEDED",
            PerfCode::FrameTooLarge => "E08-FRAME-TOO-LARGE",
            PerfCode::TokenLimit => "E08-TOKEN-LIMIT",
            PerfCode::EdgeLimit => "E08-EDGE-LIMIT",
            PerfCode::DepthLimit => "E08-DEPTH-LIMIT",
            PerfCode::CacheShapeStale => "E08-CACHE-STALE",
            PerfCode::P95Degraded => "E08-P95-DEGRADED",
            PerfCode::BudgetUnaligned => "E08-BUDGET-UNALIGNED",
        }
    }

    /// 是否阻断（告警级不阻断：P95 劣化只报警不停机）。
    pub const fn blocking(self) -> bool {
        !matches!(self, PerfCode::P95Degraded)
    }

    /// 是否属于**降级**类（走降级矩阵而非直接失败）。
    pub const fn degradable(self) -> bool {
        matches!(
            self,
            PerfCode::BudgetExceeded | PerfCode::FrameTooLarge | PerfCode::P95Degraded
        )
    }

    /// 读屏可达句子。
    pub fn spoken(self) -> String {
        let s = match self {
            PerfCode::PathInvalid => "令牌路径非法。",
            PerfCode::ValueInvalid => "令牌值非法。",
            PerfCode::CyclicGraph => "令牌依赖含环，求值顺序不存在。",
            PerfCode::GraphDiverged => "依赖图与缓存的边集不一致，已整体失效重算。",
            PerfCode::EdgeDangling => "依赖边指向不存在的令牌。",
            PerfCode::BudgetExceeded => "单个令牌求值超出预算，已降级。",
            PerfCode::FrameTooLarge => "单帧令牌数超出预算，已分帧。",
            PerfCode::TokenLimit => "令牌数超出上限。",
            PerfCode::EdgeLimit => "依赖边数超出上限。",
            PerfCode::DepthLimit => "依赖深度超出上限。",
            PerfCode::CacheShapeStale => "缓存与令牌表已失配，须整体重建。",
            PerfCode::P95Degraded => "求值耗时 P95 劣化，已告警。",
            PerfCode::BudgetUnaligned => "与AD06 的预算未对齐。",
        };
        format!("{}{}", s, self.code())
    }
}

// ---------------------------------------------------------------------------
// 二、常量与预算
// ---------------------------------------------------------------------------

/// 令牌路径字节上限。
pub const PATH_MAX: usize = 256;
/// 令牌值字节上限。
pub const VALUE_MAX: usize = 512;
/// 令牌数上限（万级承诺的硬边界）。
pub const MAX_TOKENS: usize = 16384;
/// 依赖边数上限。
pub const MAX_EDGES: usize = 65536;
/// 依赖深度上限。
pub const MAX_DEPTH: usize = 32;
/// 单帧令牌数上限（分帧兜底的帧容量）。
pub const MAX_FRAME_TOKENS: usize = 256;
/// 单令牌求值预算（以"步"为单位的抽象机，非真实时间）。
pub const UNIT_BUDGET: u32 = 64;
/// P95 采样窗口。
pub const P95_WINDOW: usize = 64;
/// P95 劣化阈值（超出即告警，仍继续跑）。
pub const P95_DEGRADE_RATIO: u32 = 2;

/// 性能承诺版本（契约冻结）。
pub const PERF_CONTRACT: &str = "E08-perf-v1";

/// 依赖方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepDir {
    /// `a -> b`：a 引用 b（b 是 a 的依赖）。
    ///
    /// 名字取"谁指向谁"，避免 `Dependents`/`Dependencies` 这类
    /// 读起来要回去查定义的词。
    RefersTo,
}

impl DepDir {
    /// 全部方向（封闭全集）。
    pub const ALL: [DepDir; 1] = [DepDir::RefersTo];

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            DepDir::RefersTo => "引用",
        }
    }
}

/// 求值结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EvalValue {
    /// 最终值。
    pub value: String,
    /// 求值步数（抽象机计步，用于预算判定）。
    pub steps: u32,
    /// 是否来自缓存（`true` = 本次未重算）。
    pub cached: bool,
}

/// 性能画像（读屏可达的核心数据结构）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PerfProfile {
    /// 参与求值的令牌数。
    pub tokens: usize,
    /// 实际重算的令牌数（命中缓存的不计）。
    pub recomputed: usize,
    /// 缓存命中数。
    pub hits: usize,
    /// 批内共享命中数（同一令牌被多条路径依赖，只算一次）。
    pub shared: usize,
    /// 分帧数（1 = 未分帧）。
    pub frames: usize,
    /// 总步数。
    pub steps: u32,
    /// 单令牌最大步数。
    pub max_steps: u32,
    /// P95 步数。
    pub p95: u32,
    /// 是否触发降级。
    pub degraded: bool,
    /// 降级原因（令牌级或帧级，未降级为空）。
    ///
    /// 只装**降级**类原因（`BudgetExceeded` / `FrameTooLarge`），
    /// 不装批次级 SLA 告警 —— 后者走 [`Self::alert`]。
    /// 一个字段装两件事会让两者互相遮蔽，见 [`Self::alert`]。
    pub code: Option<PerfCode>,
    /// 批次级 SLA 告警（与 [`Self::code`] **正交**，不是它的子集）。
    ///
    /// # 为什么必须独立成字段
    ///
    /// 踩过的坑：P95 劣化原本也写 `code`，用 `if prof.code.is_none()`
    /// 兜底"先到先得"。于是 `P95Degraded` 成了**死码**——
    /// `P95Degraded` 的触发条件是 `p95 > UNIT_BUDGET * P95_DEGRADE_RATIO`
    /// (=128)，而 p95 是某个真实令牌的步数 ⇒ 那个令牌 `steps > 128 > 64`
    /// ⇒ 它在批内**必然先**触发 `BudgetExceeded` 占住 `code`
    /// ⇒ `P95Degraded` 这条分支永远进不去。
    ///
    /// 判据全绿而该码不可达，等于没有这条告警。
    ///
    /// 两条轴的语义本就不同，不该抢一个槽位：
    /// · [`Self::code`] = **令牌/帧级**：某个令牌超预算、某帧过大 ⇒ 降级；
    /// · `alert` = **批次级**：整批的 P95 越过承诺线 ⇒ 告警（不阻断）。
    /// 一个批可以既降级又告警，两件事都要能被读到。
    pub alert: Option<PerfCode>,
}

impl PerfProfile {
    /// 新建零值画像。
    pub fn new() -> PerfProfile {
        PerfProfile {
            tokens: 0,
            recomputed: 0,
            hits: 0,
            shared: 0,
            frames: 1,
            steps: 0,
            max_steps: 0,
            p95: 0,
            degraded: false,
            code: None,
            alert: None,
        }
    }

    /// 命中率（千分比，避免浮点）。
    ///
    /// 分母为 0 时返回 1000（视为全命中）而不是 0——
    /// 空批不是"零命中"，把它报成 0% 会让面板显示"缓存完全无效"。
    pub fn hit_permille(&self) -> u32 {
        let total = self.recomputed + self.hits;
        if total == 0 {
            return 1000;
        }
        ((self.hits as usize * 1000) / total) as u32
    }

    /// 是否达成"万级令牌毫秒级"的量级承诺。
    ///
    /// 口径：**步数而非墙钟**。内核里没有可用的单调时钟（`no_std` 面
    /// 读时钟要走 MMIO，且在测试宿主上不可复现），所以承诺钉在
    /// 可复现的抽象步数上：万级令牌的总步数须 ≤ `MAX_TOKENS * UNIT_BUDGET / 64`。
    /// 这个口径的代价写在 [`PerfProfile::spoken`] 里——用户看到的是
    /// "相对承诺"，不是"绝对毫秒"。
    pub fn meets_sla(&self) -> bool {
        let cap = MAX_TOKENS as u32 * UNIT_BUDGET / 64;
        self.steps <= cap && self.p95 <= UNIT_BUDGET
    }

    /// 读屏句子。
    pub fn spoken(&self) -> String {
        let mut s = format!(
            "求值完成：{} 个令牌，重算 {} 个，缓存命中 {} 个，批内共享 {} 个。",
            self.tokens, self.recomputed, self.hits, self.shared
        );
        s.push_str(&format!("共 {} 步，单令牌最多 {} 步，P95 {} 步。", self.steps, self.max_steps, self.p95));
        s.push_str(&format!("分为 {} 帧。", self.frames));
        if self.degraded {
            match self.code {
                Some(c) => s.push_str(&format!("已降级：{} ", c.spoken())),
                None => s.push_str("已降级。"),
            }
        } else if self.meets_sla() {
            s.push_str("达成耗时承诺。");
        } else {
            s.push_str("未达耗时承诺，已告警。");
        }
        // 批次级告警与降级**正交**：一批可以既降级又告警，
        // 两句都要说出来——只报降级会把 P95 越线这件事吞掉。
        if let Some(a) = self.alert {
            s.push_str(&format!("另有告警：{} ", a.spoken()));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 三、依赖图（增量重算的数据基础）
// ---------------------------------------------------------------------------

/// 令牌依赖图（CSR 双向邻接，节点号即下标）。
///
/// 自持而非借用 F3403 的 [`DepGraph`]：本单要的是**反向**可达集
/// （"谁依赖我" = 值变了要重算谁），F3403 提供的是正向。
/// 两者的边若不一致，本单用 [`EvalCache::verify_edges`] 显式检出，
/// 而不是假定它们永远同步。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EvalGraph {
    /// 节点数。
    pub nodes: usize,
    /// 节点路径（与节点号一一对应）。
    pub paths: Vec<String>,
    /// 正向 CSR 行偏移，长度 `nodes + 1`（`RefersTo`）。
    pub out_start: Vec<u32>,
    /// 正向 CSR 目标列。
    pub out_target: Vec<u32>,
    /// 反向 CSR 行偏移，长度 `nodes + 1`（`Dependents`）。
    pub in_start: Vec<u32>,
    /// 反向 CSR 源列（谁依赖我）。
    pub in_from: Vec<u32>,
    /// 各节点派生层级（最长依赖链）。
    pub levels: Vec<u32>,
}

impl EvalGraph {
    /// 建图：入参为 `(路径列表, 边列表)`，边以**节点号**给出。
    ///
    /// **建图期即拒绝环**：求值顺序不存在时，越早失败越好——
    /// 带着环跑批会死循环，而死循环的表现是"卡住"，
    /// 没有任何线索指向"依赖图有环"。
    pub fn build(paths: Vec<String>, edges: &[(u32, u32)]) -> Result<EvalGraph, PerfCode> {
        let nodes = paths.len();
        if nodes > MAX_TOKENS {
            return Err(PerfCode::TokenLimit);
        }
        for p in paths.iter() {
            validate_path(p)?;
        }
        if edges.len() > MAX_EDGES {
            return Err(PerfCode::EdgeLimit);
        }
        for (a, b) in edges.iter() {
            if *a as usize >= nodes || *b as usize >= nodes {
                return Err(PerfCode::EdgeDangling);
            }
        }

        let mut out_deg = vec![0u32; nodes];
        let mut in_deg = vec![0u32; nodes];
        for (a, b) in edges.iter() {
            out_deg[*a as usize] += 1;
            in_deg[*b as usize] += 1;
        }
        let out_start = prefix(&out_deg);
        let in_start = prefix(&in_deg);
        // CSR 填充：用游标，天然保持与入参一致的**边序**⇒ 结果可复现。
        let mut out_cur = out_start.clone();
        let mut in_cur = in_start.clone();
        let mut out_target = vec![0u32; edges.len()];
        let mut in_from = vec![0u32; edges.len()];
        for (a, b) in edges.iter() {
            // 边序即写入序 ⇒ 同输入必得同输出（结果可复现）。
            out_target[out_cur[*a as usize] as usize] = *b;
            out_cur[*a as usize] += 1;
            in_from[in_cur[*b as usize] as usize] = *a;
            in_cur[*b as usize] += 1;
        }

        let mut g = EvalGraph {
            nodes,
            paths,
            out_start,
            out_target,
            in_start,
            in_from,
            levels: vec![0u32; nodes],
        };
        g.levels = g.compute_levels().ok_or(PerfCode::CyclicGraph)?;
        if g.levels.iter().any(|l| *l as usize > MAX_DEPTH) {
            return Err(PerfCode::DepthLimit);
        }
        Ok(g)
    }

    /// Kahn 拓扑排序求派生层级（`levels[n] = 最长依赖链长度`）。
    ///
    /// # 方向：出度不是入度
    ///
    /// 边 `a -> b` 的语义是「a 引用 b」⇒ **b 是 a 的依赖**，
    /// 所以 a 必须**后**算。而 `out_degree(a) = |refs(a)|` 是"我要算几个"。
    ///
    /// 踩过的坑：一开始按**入度**（`in_degree = |dependents|`）做 Kahn，
    /// 于是基础令牌（没人依赖它们、in_degree=0）被当成"就绪"先算，
    /// 派生令牌反而排后面 —— 层级算出 `[1,1,1,0,0,0]`，
    /// 拓扑序把 d0/d1/d2 排在 base 前面。**语义整个反过来。**
    ///
    /// 正确判据：就绪队列取**出度为 0** 的节点（无依赖的基础令牌），
    /// 松弛沿 `dependents()` 走（谁依赖我，我算完它就能算）。
    fn compute_levels(&self) -> Option<Vec<u32>> {
        let n = self.nodes;
        // 未满足的依赖计数 = 出度（我还欠着几个）。
        let mut pending = vec![0u32; n];
        for i in 0..self.nodes {
            pending[i] = self.refs(i as u32).len() as u32;
        }
        let mut levels = vec![0u32; n];
        // 就绪 = 出度 0。用栈而非队列：顺序不影响层级（层级由松弛决定），
        // 且最终排序是全序比较（层级升序 + 节点号升序），栈不引入不确定性。
        let mut ready: Vec<u32> = (0..n as u32)
            .filter(|i| pending[*i as usize] == 0)
            .collect();
        let mut settled = 0usize;
        while let Some(cur) = ready.pop() {
            settled += 1;
            for d in self.dependents(cur).iter() {
                if levels[*d as usize] < levels[cur as usize] + 1 {
                    levels[*d as usize] = levels[cur as usize] + 1;
                }
                pending[*d as usize] -= 1;
                if pending[*d as usize] == 0 {
                    ready.push(*d);
                }
            }
        }
        if settled == n {
            Some(levels)
        } else {
            None
        }
    }

    /// 拓扑序（`levels` 升序、同层按节点号升序 ⇒ 可复现）。
    pub fn eval_order(&self) -> Vec<u32> {
        let mut idx: Vec<u32> = (0..self.nodes as u32).collect();
        // 稳定排序不可用（`sort_unstable_by` 同键序不定），
        // 故用「先按层级、再按节点号」的**全序**比较键。
        idx.sort_unstable_by(|a, b| {
            let la = self.levels[*a as usize];
            let lb = self.levels[*b as usize];
            la.cmp(&lb).then(a.cmp(b))
        });
        idx
    }

    /// 谁依赖 `n`（反向邻居）。
    pub fn dependents(&self, n: u32) -> &[u32] {
        if n as usize >= self.nodes {
            return &[];
        }
        let s = self.in_start[n as usize] as usize;
        let t = self.in_start[n as usize + 1] as usize;
        &self.in_from[s..t]
    }

    /// `n` 依赖谁（正向邻居）。
    pub fn refs(&self, n: u32) -> &[u32] {
        if n as usize >= self.nodes {
            return &[];
        }
        let s = self.out_start[n as usize] as usize;
        let t = self.out_start[n as usize + 1] as usize;
        &self.out_target[s..t]
    }

    /// 节点号（按路径查）。
    pub fn node_of(&self, path: &str) -> Option<u32> {
        self.paths.iter().position(|p| p == path).map(|i| i as u32)
    }

    /// `n` 的**下游可达集**（含间接依赖者），按拓扑序升序去重。
    ///
    /// 增量重算的核心：值变了，只有这个集合里的令牌需要重算。
    ///
    /// 用**拓扑序**遍历而非 DFS：DFS 的访问顺序取决于邻接表顺序，
    /// 而下游集合要在同一份图上稳定复现（否则两次重算的顺序不同，
    /// 缓存写入顺序也跟着变，无法比对）。拓扑序天然稳定。
    pub fn downstream(&self, seeds: &[u32]) -> Vec<u32> {
        let mut mark = vec![false; self.nodes];
        let mut stack: Vec<u32> = Vec::new();
        for s in seeds.iter() {
            if (*s as usize) < self.nodes && !mark[*s as usize] {
                mark[*s as usize] = true;
                stack.push(*s);
            }
        }
        // 广度优先收集（不依赖递归深度 ⇒ 无栈溢出风险）
        let mut collected: Vec<u32> = Vec::new();
        let mut head = 0usize;
        while head < stack.len() {
            let cur = stack[head];
            head += 1;
            collected.push(cur);
            for d in self.dependents(cur) {
                if !mark[*d as usize] {
                    mark[*d as usize] = true;
                    stack.push(*d);
                }
            }
        }
        // 按拓扑序排序 ⇒ 稳定且与求值顺序一致
        collected.sort_unstable_by(|a, b| {
            let la = self.levels[*a as usize];
            let lb = self.levels[*b as usize];
            la.cmp(&lb).then(a.cmp(b))
        });
        collected
    }
}

/// 前缀和（CSR 行偏移构造）。
fn prefix(deg: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(deg.len() + 1);
    out.push(0u32);
    let mut acc = 0u32;
    for d in deg.iter() {
        acc += *d;
        out.push(acc);
    }
    out
}

/// 路径合法性。
fn validate_path(p: &str) -> Result<(), PerfCode> {
    if p.is_empty() || p.len() > PATH_MAX {
        return Err(PerfCode::PathInvalid);
    }
    Ok(())
}

/// 值合法性。
fn validate_value(v: &str) -> Result<(), PerfCode> {
    if v.is_empty() || v.len() > VALUE_MAX {
        return Err(PerfCode::ValueInvalid);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、求值缓存
// ---------------------------------------------------------------------------

/// 求值缓存（值 + 依赖指纹）。
///
/// **缓存有效性靠依赖指纹，不靠时间戳**：
/// 时间戳在 `no_std` 面拿不到可信值（要读 MMIO），而"这个值依赖谁"
/// 是纯数据。指纹 = 依赖节点号的有序集合，两次算出的指纹相同
/// ⇒ 依赖侧没变 ⇒ 值仍可用。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EvalCache {
    /// 与图同序的缓存值（`None` = 该节点未算过）。
    pub values: Vec<Option<EvalValue>>,
    /// 与图同序的依赖指纹。
    pub fingerprints: Vec<u64>,
    /// 建缓存时那张图的指纹（[`Self::graph_fingerprint`]）。
    ///
    /// **必须有它**：没有它就无法区分"这个节点的值变了"与
    /// "整张图换了、我这份缓存建立在另一张图上" —— 后者会让
    /// 按图算出的失效集彻底错，而缓存本身看不出任何异常。
    /// 这是"缓存形状对但内容属于另一版本"的唯一检出手段。
    pub graph_fp: u64,
}

impl EvalCache {
    /// 新建空缓存（与图同形）。
    pub fn new(g: &EvalGraph) -> EvalCache {
        EvalCache {
            values: vec![None; g.nodes],
            fingerprints: vec![0u64; g.nodes],
            graph_fp: Self::graph_fingerprint(g),
        }
    }

    /// 缓存形状是否仍与图一致。
    pub fn shape_ok(&self, g: &EvalGraph) -> bool {
        self.values.len() == g.nodes && self.fingerprints.len() == g.nodes
    }

    /// 边的指纹（与节点数无关的稳定哈希：FNV-1a 64）。
    ///
    /// **不依赖顺序**：两个节点依赖同一组节点号但顺序不同时，
    /// 语义上等价（依赖集合相同），指纹必须相同。
    /// 故先对邻居号做"无序归约"（xor + sum，两者合起来
    /// 比单纯 xor 更难碰撞：xor 对 `{1,2}` 与 `{3}` 这类会撞，
    /// sum 会把它们分开）。
    fn edge_fingerprint(g: &EvalGraph, n: u32) -> u64 {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut sum: u64 = 0;
        for d in g.refs(n).iter() {
            x ^= (*d as u64).wrapping_mul(0x100_0000_01B3);
            sum = sum.wrapping_add((*d as u64).wrapping_mul(0x9E37_79B1));
        }
        let count = g.refs(n).len() as u64;
        x ^ sum.rotate_left(17) ^ count.wrapping_mul(0xD6E8_FEB8_6659_FD93)
    }

    /// 全图指纹（边集指纹，用于检出"图变了但缓存不知道"）。
    pub fn graph_fingerprint(g: &EvalGraph) -> u64 {
        let mut acc: u64 = 0x243F_6A88_85A3_08D3;
        for n in 0..g.nodes as u32 {
            acc = acc.wrapping_mul(31).wrapping_add(Self::edge_fingerprint(g, n));
        }
        acc ^ (g.nodes as u64)
    }

    /// 缓存记录的图指纹。
    pub fn recorded_graph(&self) -> u64 {
        self.graph_fp
    }

    /// 契约核验：缓存记录的图指纹须与当前图一致。
    ///
    /// 不一致 ⇒ 缓存可能建立在**另一张图**上（有人在图改版后复用了旧缓存），
    /// 此时任何按图算出的失效集都是错的 ⇒ 整体重建。
    pub fn verify_edges(&self, g: &EvalGraph) -> Result<(), PerfCode> {
        if !self.shape_ok(g) {
            return Err(PerfCode::CacheShapeStale);
        }
        if self.graph_fp != Self::graph_fingerprint(g) {
            return Err(PerfCode::GraphDiverged);
        }
        Ok(())
    }

    /// 重设图指纹（重建后调用）。
    pub fn rebind(&mut self, g: &EvalGraph) {
        self.graph_fp = Self::graph_fingerprint(g);
    }

    /// **判据用窄缝**：只改写记录的图指纹，不动缓存形状与内容。
    ///
    /// 存在的理由：`verify_edges` 有**两个**失配出口
    /// （`shape_ok` 失配 → `CacheShapeStale`，`graph_fp` 失配 →
    /// `GraphDiverged`），但正常 API（`rebind` / `rebind_graph`）
    /// 总是同时改两者 ⇒ `GraphDiverged` 分支**不可达**，
    /// 于是「缓存建立在另一张图上」这条真实故障没有任何判据覆盖，
    /// 改坏它基线照样全绿。
    ///
    /// 这条缝让判据能构造**只错一个维度**的缓存失配。
    /// 它只改一个字段，不构成"绕过契约"的后门：
    /// 指纹失配后 [`Self::verify_edges`] 依然必拒。
    pub fn force_graph_fp(&mut self, fp: u64) {
        self.graph_fp = fp;
    }

    /// 读缓存。
    pub fn get(&self, n: u32) -> Option<&EvalValue> {
        if n as usize >= self.values.len() {
            return None;
        }
        self.values[n as usize].as_ref()
    }

    /// 写缓存（顺带记下依赖指纹）。
    pub fn put(&mut self, g: &EvalGraph, n: u32, v: EvalValue) {
        if n as usize >= self.values.len() {
            return;
        }
        self.fingerprints[n as usize] = Self::edge_fingerprint(g, n);
        self.values[n as usize] = Some(v);
    }

    /// 丢弃 `n` 的缓存（并递归丢其下游）。
    ///
    /// 递归用**显式栈**而非递归调用：依赖深度上限 32 虽浅，
    /// 但写成递归就等于把"深度"这件事交给调用栈上限，
    /// 而节点数可达万级（链式依赖下递归深度也可能是万级，
    /// 深度上限管的是**层级**不是**链长**——链上每层都只有一个节点）。
    pub fn invalidate(&mut self, g: &EvalGraph, seeds: &[u32]) -> Vec<u32> {
        let hit = self.downstream_cache(g, seeds);
        for n in hit.iter() {
            self.values[*n as usize] = None;
            self.fingerprints[*n as usize] = 0;
        }
        hit
    }

    /// 待失效集合 = `seeds` 的下游 ∩ 已缓存节点。
    fn downstream_cache(&self, g: &EvalGraph, seeds: &[u32]) -> Vec<u32> {
        let all = g.downstream(seeds);
        let mut out: Vec<u32> = Vec::new();
        for n in all.iter() {
            if (*n as usize) < self.values.len() && self.values[*n as usize].is_some() {
                out.push(*n);
            }
        }
        out
    }

    /// 缓存中已算过的节点数。
    pub fn live(&self) -> usize {
        self.values.iter().filter(|v| v.is_some()).count()
    }
}

// ---------------------------------------------------------------------------
// 五、批量求值流水线
// ---------------------------------------------------------------------------

/// 一批的输入：令牌号 + 基础值（无依赖节点的原始值）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BatchInput {
    /// 要产出值的节点号。
    pub targets: Vec<u32>,
    /// 基础令牌（无依赖）的值，与节点号一一对应。
    pub bases: Vec<(u32, String)>,
}

/// 批量求值结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BatchResult {
    /// 产出值（按 `targets` 顺序）。
    pub values: Vec<(u32, EvalValue)>,
    /// 画像。
    pub profile: PerfProfile,
    /// 分帧明细（每帧的令牌数）。
    pub frame_sizes: Vec<usize>,
}

/// 批量求值器。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EvalPipeline {
    /// 依赖图。
    pub graph: EvalGraph,
    /// 缓存。
    pub cache: EvalCache,
}

impl EvalPipeline {
    /// 新建流水线（缓存与图同形）。
    pub fn new(graph: EvalGraph) -> EvalPipeline {
        let mut cache = EvalCache::new(&graph);
        cache.rebind(&graph);
        EvalPipeline { graph, cache }
    }

    /// 换图（换图即换缓存：旧缓存的指纹对不上新图）。
    pub fn rebind_graph(&mut self, graph: EvalGraph) {
        self.cache = EvalCache::new(&graph);
        self.cache.rebind(&graph);
        self.graph = graph;
    }

    /// **AD06 跨批对接点**：核对上游声明的单令牌预算与本模块口径是否一致。
    ///
    /// 跨批拼接时，上游（AD06）按自己的预算表切分批次并预声明
    /// 「每个令牌至多 N 步」。本模块的预算闸是 [`UNIT_BUDGET`]。
    /// 两者不一致时**必须拒**，不能按上游的 N 放行也不能按本模块的
    /// [`UNIT_BUDGET`] 静默收紧：
    /// · 按上游放行 ⇒ 本模块的降级闸（`st > UNIT_BUDGET`）形同虚设，
    ///   上游以为有预算约束、实际没有，P95 承诺失去上界；
    /// · 按本模块静默收紧 ⇒ 上游按 N 排的批次在这里被截断，
    ///   分帧明细与上游对不上，跨批对账失去依据。
    ///
    /// 两种错法都会让「预算」这个词在两个模块间指两件事，
    /// 所以口径不一致时报 [`PerfCode::BudgetUnaligned`]，
    /// 由上游改口径或走显式协商，不在这里猜。
    ///
    /// 口径一致时返回本模块实际使用的闸值，供上游记账
    /// （返回的不是入参，避免调用方把「声明值」当成「已对齐值」记下来）。
    pub fn align_budget(&self, declared_unit_budget: u32) -> Result<u32, PerfCode> {
        if declared_unit_budget != UNIT_BUDGET {
            return Err(PerfCode::BudgetUnaligned);
        }
        Ok(UNIT_BUDGET)
    }

    /// 更新基础值（令牌值被改）。
    ///
    /// 返回**被失效的缓存节点数**——调用方据此决定要不要立刻重算。
    /// 值没变的节点不失效：否则"打开面板时顺手重写一遍相同值"
    /// 会让整棵下游缓存全丢，增量重算退化成全量。
    pub fn set_base(&mut self, n: u32, value: &str) -> Result<usize, PerfCode> {
        if n as usize >= self.graph.nodes {
            return Err(PerfCode::EdgeDangling);
        }
        validate_value(value)?;
        let changed = match self.cache.get(n) {
            Some(old) => old.value != value,
            None => true,
        };
        if !changed {
            return Ok(0);
        }
        let hit = self.cache.invalidate(&self.graph, &[n]);
        Ok(hit.len())
    }

    /// 批量求值。
    ///
    /// 流程：拓扑序 → 命中缓存的直接取 → 未命中的按依赖求值 →
    /// 超出预算则**分帧**（把本帧剩下的挪到下一帧，不丢令牌）→
    /// 统计步数与 P95 → 判定 SLA。
    pub fn evaluate(&mut self, input: &BatchInput) -> Result<BatchResult, PerfCode> {
        self.cache.verify_edges(&self.graph)?;
        for (n, v) in input.bases.iter() {
            if *n as usize >= self.graph.nodes {
                return Err(PerfCode::EdgeDangling);
            }
            validate_value(v)?;
        }
        for t in input.targets.iter() {
            if *t as usize >= self.graph.nodes {
                return Err(PerfCode::EdgeDangling);
            }
        }

        let base_map = base_lookup(input);
        // 本批真正要算的集合 = 目标的**依赖闭包**（含目标自身及其全部依赖）。
        //
        // ⚠ 只取 `downstream` 是不够的：`downstream` 给的是"谁依赖我"，
        // 而求值需要的是"我依赖谁 + 我自己"。上一版只并入 targets，
        // 于是宽扇出图（40 个节点都依赖同一个 root）里root 不在集合内，
        // 每个叶节点都找不到它的依赖 ⇒ 步数恒 2、预算闸形同虚设。
        let mut need = self.graph.downstream(&input.targets);
        for t in input.targets.iter() {
            if !need.contains(t) {
                need.push(*t);
            }
        }
        // 补上 targets 的**完整正向依赖闭包**（基础令牌，含间接）。
        // 用显式栈做传递闭包：不递归（节点数可达万级），
        // 且每入队一次即标记，避免重复入队。
        {
            let mut extra: Vec<u32> = Vec::new();
            let mut stack: Vec<u32> = input.targets.clone();
            let mut seen = need.clone();
            while let Some(cur) = stack.pop() {
                for r in self.graph.refs(cur).iter() {
                    if !seen.contains(r) {
                        seen.push(*r);
                        extra.push(*r);
                        stack.push(*r);
                    }
                }
            }
            for e in extra.iter() {
                if !need.contains(e) {
                    need.push(*e);
                }
            }
        }
        need.sort_unstable_by(|a, b| {
            let la = self.graph.levels[*a as usize];
            let lb = self.graph.levels[*b as usize];
            la.cmp(&lb).then(a.cmp(b))
        });

        let mut prof = PerfProfile::new();
        prof.tokens = input.targets.len();
        let mut frame_sizes: Vec<usize> = Vec::new();
        let mut frame_len = 0usize;
        let mut steps: Vec<u32> = Vec::new();
        let mut values: Vec<(u32, EvalValue)> = Vec::new();

        // **批内共享的计数口径**：一个令牌被本批**多个令牌引用** ⇒
        // 没有共享机制时它会被反复求值。
        //
        // ⚠ 不能靠"批内列表里第二次遇到同一节点"来计：`need` 是去重后的
        // 拓扑序列表，每个节点只出现一次，那个口径恒为 0（判据会恒红）。
        // 正确口径是**引用次数**：被 k 个节点引用 ⇒ 贡献 k-1 次共享。
        //
        // 这也是"批内共享"真正的收益来源：菱形里 base0 被 d0、d1 引用，
        // 第二次引用时值已在手，不必重算。
        let mut ref_count: Vec<u32> = vec![0u32; self.graph.nodes];
        for n in need.iter() {
            for d in self.graph.refs(*n).iter() {
                ref_count[*d as usize] += 1;
            }
        }
        let expect_shared: usize = ref_count
            .iter()
            .map(|k| if *k > 1 { (*k - 1) as usize } else { 0 })
            .sum();

        for n in need.iter() {
            // 1) 缓存命中：不重算。
            // 共享数已由ref_count 统一计，此处不再判"批内第二次"。
            if let Some(hit) = self.cache.get(*n) {
                prof.hits += 1;
                values.push((*n, hit.clone()));
                steps.push(hit.steps);
                frame_len += 1;
                if frame_len >= MAX_FRAME_TOKENS {
                    frame_sizes.push(frame_len);
                    frame_len = 0;
                }
                continue;
            }
            // 2) 未命中：先算它的依赖（拓扑序保证依赖已算/已命中）。
            let refs = self.graph.refs(*n);
            let mut acc: Option<String> = None;
            let mut st: u32 = 1;
            for r in refs.iter() {
                let (rv, rs) = match self.cache.get(*r) {
                    Some(c) => (c.value.clone(), c.steps),
                    None => match values.iter().find(|(k, _)| k == r) {
                        Some((_, c)) => (c.value.clone(), c.steps),
                        // 依赖既不在缓存也不在本批产出里 ⇒ 基础值缺失。
                        None => match base_map.lookup(*r) {
                            Some(b) => (b.clone(), 0),
                            None => return Err(PerfCode::GraphDiverged),
                        },
                    },
                };
                st = st.saturating_add(rs).saturating_add(rv.len() as u32);
                acc = Some(match acc {
                    None => rv,
                    Some(prev) => {
                        if prev.len() + rv.len() > VALUE_MAX {
                            return Err(PerfCode::ValueInvalid);
                        }
                        format!("{}{}", prev, rv)
                    }
                });
            }
            // 3) 预算闸：单令牌步数超上限 ⇒ 降级（不静默慢）。
            if st > UNIT_BUDGET {
                prof.degraded = true;
                if prof.code.is_none() {
                    prof.code = Some(PerfCode::BudgetExceeded);
                }
                // 降级仍产出（取基础值兜底），但**不写缓存**——
                // 写进去等于把一个超预算的结果固化下来。
                let fb = base_map.lookup(*n).unwrap_or_else(|| String::from("0"));
                values.push((
                    *n,
                    EvalValue {
                        value: fb,
                        steps: st,
                        cached: false,
                    },
                ));
                steps.push(st);
                prof.recomputed += 1;
            } else {
                let v = EvalValue {
                    value: match acc {
                        Some(v) => v,
                        None => base_map.lookup(*n).unwrap_or_else(|| String::from("0")),
                    },
                    steps: st,
                    cached: false,
                };
                self.cache.put(&self.graph, *n, v.clone());
                values.push((*n, v));
                steps.push(st);
                prof.recomputed += 1;
            }
            frame_len += 1;
            if frame_len >= MAX_FRAME_TOKENS {
                frame_sizes.push(frame_len);
                frame_len = 0;
            }
        }
        if frame_len > 0 {
            frame_sizes.push(frame_len);
        }
        if frame_sizes.len() > 1 {
            prof.frames = frame_sizes.len();
        }
        // 共享数按引用次数口径（见上面的ref_count 推导）。
        prof.shared = expect_shared;

        prof.steps = steps.iter().fold(0u32, |a, b| a.saturating_add(*b));
        prof.max_steps = steps.iter().fold(0u32, |a, b| if *b > a { *b } else { a });
        prof.p95 = percentile95(&steps);
        // P95 劣化是**批次级告警**，走独立字段 `alert` ——
        // 不与令牌级降级抢 `code`（原因见 `PerfProfile::alert` 的说明）。
        // 触发条件 `p95 > UNIT_BUDGET * P95_DEGRADE_RATIO` 意味着
        // 第 95 百分位那个令牌自己就超了单令牌预算，因此
        // `code` 几乎必然已是 `BudgetExceeded` —— 共用槽位则此码永不可达。
        if prof.p95 > UNIT_BUDGET * P95_DEGRADE_RATIO {
            prof.degraded = true;
            prof.alert = Some(PerfCode::P95Degraded);
        }
        if prof.frames > 1 && prof.code.is_none() {
            prof.degraded = true;
            prof.code = Some(PerfCode::FrameTooLarge);
        }

        // 产出按 targets 顺序（调用方要的是"我点的那个令牌"）。
        let mut out: Vec<(u32, EvalValue)> = Vec::new();
        for t in input.targets.iter() {
            match values.iter().find(|(k, _)| k == t) {
                Some((_, v)) => out.push((*t, v.clone())),
                None => match base_map.lookup(*t) {
                    Some(b) => out.push((
                        *t,
                        EvalValue {
                            value: b,
                            steps: 1,
                            cached: false,
                        },
                    )),
                    None => return Err(PerfCode::GraphDiverged),
                },
            }
        }

        Ok(BatchResult {
            values: out,
            profile: prof,
            frame_sizes,
        })
    }

    /// 耗时面板（读屏可达）。
    pub fn panel(&mut self, input: &BatchInput) -> Result<String, PerfCode> {
        let r = self.evaluate(input)?;
        let mut s = r.profile.spoken();
        s.push_str("分帧明细：");
        for (i, n) in r.frame_sizes.iter().enumerate() {
            s.push_str(&format!("第{} 帧 {} 个；", fmt_num(i + 1), fmt_num(*n)));
        }
        s.push_str(&format!("命中率 {} 成。", fmt_num(r.profile.hit_permille() as usize / 10)));
        s.push_str(&format!("契约 {}{}。", PERF_CONTRACT, "已冻结"));
        Ok(s)
    }
}

/// 基础值查表（小规模线性查：基础令牌数量级远小于目标数，
/// 且线性查**不依赖排序**，省一次 clone）。
struct BaseLookup {
    items: Vec<(u32, String)>,
}

impl BaseLookup {
    fn lookup(&self, n: u32) -> Option<String> {
        for (k, v) in self.items.iter() {
            if *k == n {
                return Some(v.clone());
            }
        }
        None
    }
}

fn base_lookup(input: &BatchInput) -> BaseLookup {
    BaseLookup {
        items: input.bases.clone(),
    }
}

/// P95（近秩法：`ceil(0.95 * n) - 1` 位，样本量小时比插值法稳）。
fn percentile95(samples: &[u32]) -> u32 {
    if samples.is_empty() {
        return 0;
    }
    let mut v = samples.to_vec();
    v.sort_unstable();
    // ceil(0.95n) 用整数算：`(95 * n + 99) / 100`
    let rank = (95 * v.len() + 99) / 100;
    let idx = if rank == 0 { 0 } else { rank - 1 };
    v[idx.min(v.len() - 1)]
}

/// 数量读屏（逐位）。
pub fn fmt_num(n: usize) -> String {
    if n == 0 {
        return String::from("零");
    }
    let digits = [
        "零", "一", "二", "三", "四", "五", "六", "七", "八", "九",
    ];
    let mut s = String::new();
    let mut started = false;
    for ch in format!("{}", n).chars() {
        let d = (ch as u8) - b'0';
        if d == 0 {
            if started {
                s.push('0');
            }
        } else {
            if started {
                s.push(' ');
            }
            s.push_str(digits[d as usize]);
            started = true;
        }
    }
    s
}