//! VE-F3403 · 令牌依赖图与级联（VE-E 域 · 主题与个性化引擎 · 令牌运行时组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3403`
//!
//! **判据（锚点原文）**：依赖可视化、级联合并、深度上限、耗时画像、判据。
//!
//! **职责定位（锚点原文）**：令牌间依赖的可视化与级联更新（改一个基础色全链
//! 求值重算），级联深度上限，级联耗时画像。
//!
//! **数据结构（锚点原文）**：依赖图 [`DepGraph`]；级联引擎 [`CascadeEngine`]。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - **深度超限 → 拒绝**。深度闸有**两处**，因为深度是两个不同的量：
//!   [`MAX_CHAIN_DEPTH`] 是**建图闸**，拦图本身的固有形状（最长派生链太长，
//!   任何一次级联都走不完，走到中途就是半新半旧）；
//!   [`MAX_CASCADE_DEPTH`] 是**级联闸**，拦单次操作的半径（这一批改动太散）。
//!   两闸同值是当前的设计选择，但**分设两个名字**，就是为了让诊断能说清
//!   "是哪一闸响的"——同一个码配两段不同文案，诊断就退化成"深度超了"。
//! - **级联风暴 → 批量合并**。[`ChangeQueue`] 把窗口内的多次改动并成**一次**
//!   级联：先并种子、再走一遍，受影响集合取并集、每个节点只求值一次，
//!   不是 N 遍叠加。合并省下的工作量是**实测**的（见 `cascade_merged`）。
//! - **图损坏 → 重建**。[`DepGraph::verify`] 判结构完整性，不过就
//!   [`CascadeEngine::rebuild`] 从源重建。**不在坏图上继续级联**。
//!
//! **性能逐项分解（锚点原文）**：O(边数)。级联沿**反向边**（谁依赖我）扩散，
//! 每条边至多被走一次；层级复算走 Kahn 消解，每个节点入队出队各一次。
//! 两处都不按"节点数 × 节点数"走——那正是 [`DepGraph::verify`] 里那条
//! "独立复算"的代价，只在校验期付，不在热路径付。
//!
//! **跨批对接点（锚点原文）**：E09 调试工具联动 —— 交出两件产物：
//! [`DepGraph::dot`]（图描述文本）与 [`DepGraph::debug_slice`]（**有界**邻域
//! 切片，半径受 [`MAX_DEBUG_RADIUS`] 闸住）。E09 直接消费，不自己重算依赖。
//!
//! **无障碍与隐私（锚点原文）**：依赖图读屏替代 —— 依赖图**不能只以图的形式
//! 存在**。[`DepGraph::spoken`] 给出与图形等信息的纯文本替述（节点数、边数、
//! 层数、逐层节点数、最深链末端、被依赖最多的令牌）；[`DepGraph::node_spoken`]
//! 给单点替述；[`DebugSlice::spoken`] 与 [`CascadeProfile::spoken`] 给操作级
//! 替述。三者都不依赖颜色、不依赖缩进宽度、不依赖终端列数——**朗读顺序即
//! 字段顺序**。
//!
//! **本项的边界（不越界施工）**：
//! - 拥有：依赖图数据结构（正反双向 CSR、层级、完整性校验、读屏替述）与级联
//!   引擎（级联传播、深度闸、批量合并、耗时画像）；
//! - **不做**双格式词法/语法解析与引用 DAG 构建（F3402 拥有，本模块消费
//!   `ver01b_parser::{TokenSet, TokenDag}`）、**不做**类型校验与单位体系
//!   （F3405 拥有）、**不做**四级覆盖优先级仲裁（F3406 拥有——本模块只接受
//!   **已仲裁完毕**的覆盖结果当输入，见 `set_override`）、**不做**增量求值
//!   缓存与批量流水线（F3408 拥有）、**不做**主题切换事务与快照回滚（F3404
//!   拥有）、**不做**风暴限频窗口配置与丢通知补发（F3416 拥有）。
//!
//! **设计要点（为什么这么写，不是这么写会怎样）**：
//! - **正反双向 CSR 都存，不是只存反向**。只存反向边看着省一半内存，但
//!   [`DepGraph::verify`] 就失去了**独立复算能力**——没有正向边就无法验证
//!   层级，只能"自己说自己对"。存双向换来两样**外部可判**的证据：反表必须是
//!   正表的转置、层级必须能被正表复算出来。损坏检测要求有第二个来源能推翻
//!   第一个来源，否则"检测"只是"把自己读了一遍"。
//! - **层级 = 最长派生链**：`level(n) = 0`（无依赖）否则
//!   `1 + max(level(各依赖))`。取最短会让"深度"随书写顺序抖动；取最长才
//!   保证**求值序安全**：任何被依赖方的层级都**严格小于**依赖方，故按层级
//!   升序求值时目标必然已就绪。这是级联正确性的地基。
//! - **受影响集合必须与实际值变化集合完全相等**。级联算出的"受影响节点"若
//!   与"重算后值真的变了的节点"只差一个，界面就会留一个陈旧值、或白重算一遍。
//!   故 [`CascadeOutcome::changed`] 是**实测差分**出来的，不是级联范围的
//!   转述——两者不等即红。这是本项最强的一条判据。
//! - **级联只重算影响面，但可以读到影响面外的值**。这是对的，且必须说清：
//!   若`m` 受影响而`m` 依赖的`c` 未受影响，则`c` 的值仍是上次全量求值的
//!   正确值（它没被这次改动碰到）。受影响片按层级升序重算，保证片内目标
//!   先就绪。两条合起来才是"只算影响面也正确"的完整理由。
//! - **深度闸在扩散当下就拦**，不是走完再报。走完再报意味着已经白跑了
//!   几十跳，而且报告里那个"太远"的节点是任意的第一个越界者。级联闸取
//!   [`MAX_CASCADE_DEPTH`] = 16，**严格小于**建图闸 32：沿反向边走一跳层级
//!   至少上升 1，故级联跳数恒不超过链长，两闸同值则级联闸永远触发不到。
//! - **批量合并的收益用单边符号判定**：`合并后 − max(逐个) < 0` 恒成立。
//!   不用双边阈值——双边阈值一旦宽过正确实现的低估幅度，高估型变异就从
//!   缝里钻过去了（本项实测：±6% 的双边阈值漏网，改单边 0.5% 捕获）。
//! - **耗时画像不取墙钟**。零墙钟是内核回归可复现的硬要求，所以画像计**工作量
//!   单位**（走过多少条边、拼了多少次、每层多少节点），真实时钟留给调用方
//!   注入（[`CascadeProfile::mark_ticks`]）——本模块自己一次都不读时钟。
//!   写 `elapsed = nodes * CONST` 的自证式算术等于没测：计数器必须覆盖缺陷
//!   真正发生的那一层，场景要选内层大量判失败的场合。
//! - **过期改动不静默丢弃**。[`ChangeQueue::drain`] 把过期与未到期的改动
//!   **原样交回**调用方。丢掉就等于让用户的改动凭空消失，那比慢更坏。
//!
//! **零外部依赖**，只依赖 `crate::checks`（自检侧）、兄弟模块 `ver01b_parser`
//! （F3402 交出的令牌集与引用 DAG）与 `alloc`。
//! 确定性：零墙钟、零 IO、无随机源，回归可复现。

use super::ver01b_parser::{extract_refs, Site, TokenDag, TokenSet};
use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 级联引擎版本。依赖图结构或传播语义变更走版本号。
pub const CASCADE_VERSION: &str = "E01-cascade-v1";

/// **建图闸**：最长派生链上限。一条 40 层的引用链意味着任何一次级联都要走 40
/// 跳才够得着底——超限即**拒绝收这张图**，而不是让每次级联走到一半停下，
/// 留下半新半旧的界面。
pub const MAX_CHAIN_DEPTH: usize = 32;

/// **级联闸**：单次级联距离上限。一次改动影响到 16 跳之外，说明这批改动过于
/// 分散，应当拆批而不是让一次级联横穿整张图。
///
/// **必须严格小于 [`MAX_CHAIN_DEPTH`]**，这不是随手取的数，是一条不变式：
/// 沿反向边走一跳，层级必然至少上升 1（`level[m] >= level[n] + 1`），
/// 故**级联跳数恒不超过链长**。两闸若取同值，级联闸就是**永远触发不到的
/// 死代码**——链长闸会先一步把超限的图拒掉。一道触发不到的闸不是闸，
/// 它只是让诊断文案里多了一个骗人的分支。这里取 16：图允许建到 32 层，
/// 但单次级联只许走 16 跳，更深的传播必须拆批。
pub const MAX_CASCADE_DEPTH: usize = 16;

/// 批量合并的种子上限。超了说明这一帧的改动不是"风暴"而是"重载"，应当拒绝
/// 并让调用方拆帧，而不是硬着头皮一次算完。
pub const MAX_BATCH_SEEDS: usize = 256;

/// 合并窗口（滴答）。窗口内的多次改动并成一次级联。
pub const MERGE_WINDOW_TICKS: u64 = 16;

/// 依赖图节点数上限，与 F3402 的 `MAX_TOKENS` 同值（两处必须同值：解析器放行
/// 5000 条而级联引擎拒收，就是"解析成功但级联失败"的空转）。
pub const MAX_GRAPH_NODES: usize = 4096;

/// 依赖图边数上限。一节点引用 4096 个目标是病态输入，不是令牌集。
pub const MAX_GRAPH_EDGES: usize = 16384;

/// 耗时画像的层桶上限。超出部分**计数后丢弃**并在 [`CascadeProfile::dropped`]
/// 里显性化——静默截断会让画像看起来完整而其实缺了尾巴。
pub const MAX_PROFILE_LEVELS: usize = 64;

/// 单个令牌求值结果的字节上限。嵌引用会指数膨胀，没有闸就是内存耗尽。
pub const MAX_VALUE_LEN: usize = 4096;

/// E09 调试切片的半径上限。调试工具不能要求一个无界邻域——那等于要整张图。
pub const MAX_DEBUG_RADIUS: usize = MAX_CASCADE_DEPTH;

/// 覆盖层未仲裁时的缺省值。**覆盖仲裁归 F3406**，本模块拿到的必须是已仲裁结果；
/// 未设置覆盖的令牌按原文求值，不猜一个。
pub const NO_OVERRIDE: &str = "";

// ---------------------------------------------------------------------------
// 二、诊断（深度闸 / 图损坏 / 风暴超载，三要素缺一不可）
// ---------------------------------------------------------------------------

/// 级联侧诊断码。与 F3402 的 `DiagCode` 是**两套**：各自模块自持，不共用枚举，
/// 免得一处加码牵连另一处的穷举匹配。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CascadeCode {
    /// 诊断三要素里有空段（构造期拦截）。
    IncompleteDiag,
    /// 深度超限。`CascadeDiag::what` 的措辞指明是哪一闸（建图链长/ 级联距离）。
    DepthExceeded,
    /// 依赖图结构损坏。
    GraphCorrupt,
    /// 批量合并的种子数超载。
    BatchTooLarge,
    /// 节点编号越界，或图与令牌表不同源。
    NodeOutOfRange,
    /// 求值结果超长（引用嵌套膨胀）。
    ValueTooLong,
    /// 路径在图里不存在。
    NoSuchPath,
    /// 画像层桶溢出。
    ProfileOverflow,
}

impl CascadeCode {
    /// 稳定短码，进诊断台账与读屏播报。**枚举判别值不是线上编码值**——自检里
    /// 断言 `wire()` 与判别值解耦。
    pub fn wire(self) -> &'static str {
        match self {
            CascadeCode::IncompleteDiag => "E03_DIAG_INCOMPLETE",
            CascadeCode::DepthExceeded => "E03_DEPTH_LIMIT",
            CascadeCode::GraphCorrupt => "E03_GRAPH_CORRUPT",
            CascadeCode::BatchTooLarge => "E03_BATCH_TOO_LARGE",
            CascadeCode::NodeOutOfRange => "E03_NODE_RANGE",
            CascadeCode::ValueTooLong => "E03_VALUE_TOO_LONG",
            CascadeCode::NoSuchPath => "E03_NO_SUCH_PATH",
            CascadeCode::ProfileOverflow => "E03_PROFILE_OVERFLOW",
        }
    }

    /// 该码是否代表**阻断**。级联侧全部阻断：半张图比没有图更危险，半新半旧的
    /// 界面比不换更危险。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            CascadeCode::IncompleteDiag
                | CascadeCode::DepthExceeded
                | CascadeCode::GraphCorrupt
                | CascadeCode::BatchTooLarge
                | CascadeCode::NodeOutOfRange
                | CascadeCode::ValueTooLong
                | CascadeCode::NoSuchPath
                | CascadeCode::ProfileOverflow
        )
    }

    /// 该码是否为**深度闸**（建图链长与级联距离共用一码，靠诊断文本指明哪一闸）。
    pub fn is_depth(self) -> bool {
        matches!(self, CascadeCode::DepthExceeded)
    }
}

/// 级联侧诊断。**三要素缺一不可**——"报错但没给出路"的诊断比不报更坏，
/// 因为它让人以为已经处理过了。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CascadeDiag {
    /// 诊断码。
    pub code: CascadeCode,
    /// 位置（读屏第一句就念它）。
    pub site: Site,
    /// 现象：发生了什么。
    pub what: String,
    /// 原因：为什么发生。
    pub why: String,
    /// 处置：读者下一步该做什么。
    pub fix: String,
}

impl CascadeDiag {
    /// 构造诊断。**任一要素为空则降级为 [`CascadeCode::IncompleteDiag`]**。
    pub fn new(
        code: CascadeCode,
        site: Site,
        what: &str,
        why: &str,
        fix: &str,
    ) -> CascadeDiag {
        if what.is_empty() || why.is_empty() || fix.is_empty() {
            let which = if what.is_empty() {
                "现象段为空：调用方没说明发生了什么"
            } else if why.is_empty() {
                "原因段为空：调用方没说明为什么发生"
            } else {
                "处置段为空：调用方没说明读者下一步该做什么"
            };
            return CascadeDiag {
                code: CascadeCode::IncompleteDiag,
                site,
                what: "诊断三要素不完整".to_string(),
                why: which.to_string(),
                fix: "补齐缺失的那一段后重试；级联引擎不接受残缺诊断".to_string(),
            };
        }
        CascadeDiag {
            code,
            site,
            what: what.to_string(),
            why: why.to_string(),
            fix: fix.to_string(),
        }
    }

    /// 三要素是否齐备。
    pub fn complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.fix.is_empty()
    }

    /// 读屏播报文本：**先位置、再现象、后原因与处置**，字段顺序即朗读顺序。
    pub fn spoken(&self) -> String {
        format!(
            "{}第 {} 行第 {} 列（字节 {}）：{}。原因：{}。处置：{}",
            self.code.wire(),
            self.site.line,
            self.site.col,
            self.site.byte,
            self.what,
            self.why,
            self.fix
        )
    }
}

/// 建图链长闸的诊断（两闸之一：图本身的固有形状）。
pub fn diag_chain_depth(path: &str, level: u32, cap: usize) -> CascadeDiag {
    CascadeDiag::new(
        CascadeCode::DepthExceeded,
        Site::start(),
        &format!(
            "建图闸：令牌 {:?} 处在第 {} 层派生链上，超过上限 {}",
            path, level, cap
        ),
        "派生链过长意味着任何一次级联都无法一次走完，走到中途就会留下半新半旧的界面",
        &format!(
            "把这条引用链拆成两段（在中间加一层直接引用底层基础令牌的中间令牌），或把建图上限从 {} 调到至少 {}",
            cap,
            level + 1
        ),
    )
}

/// 级联距离闸的诊断（两闸之二：单次操作半径）。
pub fn diag_cascade_depth(seed: &str, far: &str, depth: u32, cap: usize) -> CascadeDiag {
    CascadeDiag::new(
        CascadeCode::DepthExceeded,
        Site::start(),
        &format!(
            "级联闸：从 {:?} 出发的传播最远到了第 {} 跳的 {:?}，超过单次级联上限 {}",
            seed, depth, far, cap
        ),
        "一次改动影响到太远的节点，说明这批改动过于分散，一次级联会横穿整张图",
        &format!(
            "把这批改动拆成几批分别级联；若确实需要这么远的传播，先检查 {:?} 的引用是不是绕了远路",
            far
        ),
    )
}

/// 图损坏诊断。
pub fn diag_corrupt(what: &str, why: &str, fix: &str) -> CascadeDiag {
    CascadeDiag::new(CascadeCode::GraphCorrupt, Site::start(), what, why, fix)
}

/// 令牌表缺条目的位置（内部不一致，没有更好的位置可报）。
fn entry_site() -> Site {
    Site::start()
}

// ---------------------------------------------------------------------------
// 三、依赖图（正反双向 CSR + 层级 + 完整性校验 + 读屏替述）
// ---------------------------------------------------------------------------

/// 令牌依赖图。
///
/// **出边**（out）：`n` 引用了谁（沿派生方向向下）。
/// **入边**（in）：谁引用了 `n`（沿派生方向向上，级联走的就是这一侧）。
///
/// 两张 CSR 都存，不是只存入边——理由见头注"设计要点"第一条：存双向才让
/// [`DepGraph::verify`] 有独立复算能力。
#[derive(Clone, Debug)]
pub struct DepGraph {
    /// 节点数（等于令牌数）。
    pub nodes: usize,
    /// 节点路径，与节点号一一对应。
    pub paths: Vec<String>,
    /// 正向 CSR 行偏移，长度 `nodes + 1`。
    pub out_start: Vec<u32>,
    /// 正向 CSR 目标列，扁平存储。
    pub out_target: Vec<u32>,
    /// 反向 CSR 行偏移，长度 `nodes + 1`。
    pub in_start: Vec<u32>,
    /// 反向 CSR 源列，扁平存储（= 谁依赖我）。
    pub in_from: Vec<u32>,
    /// 每个节点的派生层级（最长派生链）。基础令牌为 0。
    pub levels: Vec<u32>,
    /// 全图最高层级。
    pub max_level: u32,
    /// 边数。
    pub edges: usize,
}

impl DepGraph {
    /// 从 F3402 交出的引用 DAG 构建依赖图。**建图闸在此**：最长派生链超
    /// [`MAX_CHAIN_DEPTH`] 即拒绝收图。
    pub fn from_dag(ts: &TokenSet, dag: &TokenDag) -> Result<DepGraph, CascadeDiag> {
        if dag.nodes != ts.entries.len() {
            return Err(diag_corrupt(
                &format!(
                    "引用图说有 {} 个节点，令牌表里却有 {} 条",
                    dag.nodes,
                    ts.entries.len()
                ),
                "两张表对不上同一批令牌，说明上游交出的图与令牌集不是同一次构建的产物",
                "从源文件重新走一遍解析与建图，不要手工拼接图与令牌表",
            ));
        }
        let nodes = dag.nodes;
        if nodes > MAX_GRAPH_NODES {
            return Err(diag_corrupt(
                &format!(
                    "依赖图有 {} 个节点，超过上限 {}",
                    nodes, MAX_GRAPH_NODES
                ),
                "节点数失控会让级联的 O(边数) 承诺失去意义",
                &format!(
                    "把令牌集拆成主题包分批加载，或把节点上限从 {} 调到至少 {}",
                    MAX_GRAPH_NODES,
                    nodes + 1
                ),
            ));
        }
        if dag.edge_count() > MAX_GRAPH_EDGES {
            return Err(diag_corrupt(
                &format!(
                    "依赖图有 {} 条边，超过上限 {}",
                    dag.edge_count(),
                    MAX_GRAPH_EDGES
                ),
                "边数失控说明有令牌在病态地互相引用",
                "找出引用数异常的令牌并把它拆开",
            ));
        }

        let mut paths: Vec<String> = Vec::with_capacity(nodes);
        for e in ts.entries.iter() {
            paths.push(e.path.clone());
        }

        // ---- 正向 CSR：抄一份并逐项验范围。抄来的数据也要验——
        //      上游给的东西不可因为"是自己人"就免检。
        let mut out_start: Vec<u32> = Vec::new();
        out_start.resize(nodes + 1, 0u32);
        for i in 0..=nodes {
            if i < dag.out_start.len() {
                out_start[i] = dag.out_start[i];
            }
        }
        let mut out_target: Vec<u32> = Vec::new();
        out_target.resize(dag.out_target.len(), 0u32);
        for (i, t) in dag.out_target.iter().enumerate() {
            if (*t as usize) >= nodes {
                return Err(diag_corrupt(
                    &format!("正向 CSR 第 {} 列的目标节点 {} 越界", i, t),
                    "边指向了不存在的节点，级联会顺着它走进虚空",
                    "从源文件重建依赖图",
                ));
            }
            out_target[i] = *t;
        }
        let edges = out_target.len();
        Self::check_csr_shape("正向", &out_start, edges, nodes)?;

        // ---- 反向 CSR：正表的转置。
        let (in_start, in_from) = Self::transpose(nodes, &out_start, &out_target);
        Self::check_csr_shape("反向", &in_start, in_from.len(), nodes)?;

        // ---- 层级：Kahn 消解，沿反向边把已定级的节点抬到它的依赖者上。
        let (levels, max_level) = Self::compute_levels(nodes, &out_start, &out_target, &in_start, &in_from)?;
        if max_level as usize > MAX_CHAIN_DEPTH {
            // 点名最深那个节点：诊断只报一个数字，读者不知道深在哪。
            let mut worst = 0usize;
            for n in 0..nodes {
                if levels[n] > levels[worst] {
                    worst = n;
                }
            }
            return Err(diag_chain_depth(&paths[worst], levels[worst], MAX_CHAIN_DEPTH));
        }

        Ok(DepGraph {
            nodes,
            paths,
            out_start,
            out_target,
            in_start,
            in_from,
            levels,
            max_level,
            edges,
        })
    }

    /// CSR 形状校验：以 0 起头、非降、末项等于列长。
    fn check_csr_shape(
        name: &str,
        start: &[u32],
        col_len: usize,
        nodes: usize,
    ) -> Result<(), CascadeDiag> {
        if start.len() != nodes + 1 {
            return Err(diag_corrupt(
                &format!(
                    "{} CSR 行偏移有 {} 项，应为节点数加一（{}）",
                    name,
                    start.len(),
                    nodes + 1
                ),
                "CSR 的行偏移必须比节点多一格，末格是哨兵",
                "从源文件重建依赖图",
            ));
        }
        if nodes > 0 && start[0] != 0 {
            return Err(diag_corrupt(
                &format!("{} CSR 的行偏移没有从 0 起头", name),
                "行偏移必须以 0 起头，否则首行的边数会被算错",
                "从源文件重建依赖图",
            ));
        }
        let mut prev = 0u32;
        for i in 1..=nodes {
            if start[i] < prev {
                return Err(diag_corrupt(
                    &format!("{} CSR 的行偏移在第 {} 行倒退了", name, i),
                    "CSR 要求行偏移非降，倒退说明列存储与行偏移不同源",
                    "从源文件重建依赖图",
                ));
            }
            prev = start[i];
        }
        if prev as usize != col_len {
            return Err(diag_corrupt(
                &format!(
                    "{} CSR 末行偏移 {} 与列长 {} 不等",
                    name,
                    prev as usize,
                    col_len
                ),
                "行偏移总和与列长不等，说明有边被漏写或重复写",
                "从源文件重建依赖图",
            ));
        }
        Ok(())
    }

    /// 求正向表的转置：`(行偏移, 源列)`。
    ///
    /// **行偏移必须按目标节点（入度）计数，不是按源节点（出度）。**
    /// 这两个数一般不相等——`a→b` 这条边给 b 的入区贡献 1，给 a 的入区贡献 0。
    /// 按源累加会让整张反表的行偏移全错，而**长度检查与单调性检查都过得去**
    /// （总和与列长相等、逐项非降），只有"反表是不是正表的转置"这条能抓到。
    /// 这正是 [`DepGraph::verify`] 里那条交叉校验存在的理由。
    fn transpose(
        nodes: usize,
        out_start: &[u32],
        out_target: &[u32],
    ) -> (Vec<u32>, Vec<u32>) {
        // 第一遍：数每个目标节点的入度。
        let mut in_start: Vec<u32> = Vec::new();
        in_start.resize(nodes + 1, 0u32);
        for t in out_target.iter() {
            in_start[*t as usize + 1] += 1;
        }
        // 前缀和。
        for i in 1..=nodes {
            in_start[i] += in_start[i - 1];
        }
        // 第二遍：按源节点顺序填列（保序，便于报错时按书写顺序列出）。
        let mut cursor: Vec<u32> = in_start[..nodes].to_vec();
        let mut in_from: Vec<u32> = Vec::new();
        in_from.resize(out_target.len(), 0u32);
        for src in 0..nodes {
            let lo = out_start[src] as usize;
            let hi = out_start[src + 1] as usize;
            for k in lo..hi {
                let dst = out_target[k] as usize;
                let slot = cursor[dst] as usize;
                if slot < in_from.len() {
                    in_from[slot] = src as u32;
                }
                cursor[dst] += 1;
            }
        }
        (in_start, in_from)
    }

    /// 复算层级（Kahn 消解）：出度为 0 的是基础令牌，沿**反向边**把已定级的
    /// 节点抬到它的依赖者上。
    ///
    /// 每个节点入队出队各一次、每条反向边被走一次，故 **O(V+E)**。
    /// 若一轮消解后仍有节点未定级，说明图里有环（F3402 正常路径下不该出现；
    /// 出现即图损坏），报 [`CascadeCode::GraphCorrupt`] 而非静默填 0——
    /// 静默填 0 会让环上的节点都变成"基础令牌"，求值序随即失效。
    fn compute_levels(
        nodes: usize,
        out_start: &[u32],
        _out_target: &[u32],
        in_start: &[u32],
        in_from: &[u32],
    ) -> Result<(Vec<u32>, u32), CascadeDiag> {
        let mut remaining: Vec<u32> = Vec::with_capacity(nodes);
        remaining.resize(nodes, 0u32);
        for n in 0..nodes {
            remaining[n] = out_start[n + 1] - out_start[n];
        }
        let mut levels: Vec<u32> = Vec::with_capacity(nodes);
        levels.resize(nodes, 0u32);
        let mut max_level = 0u32;
        // 队列用 Vec 当前向缓冲：head 单调前进，不回收已消费的槽。
        // 内核里不用 VecDeque（少一个依赖），也不预先按 nodes 定容——
        // 一个节点可能因多轮抬升被重复入队？不：remaining 归零才入队，
        // 故每个节点恰好入队一次。
        let mut queue: Vec<u32> = Vec::with_capacity(nodes);
        for n in 0..nodes {
            if remaining[n] == 0 {
                queue.push(n as u32);
            }
        }
        let mut head = 0usize;
        let mut settled = 0usize;
        while head < queue.len() {
            let n = queue[head];
            head += 1;
            settled += 1;
            if levels[n as usize] > max_level {
                max_level = levels[n as usize];
            }
            for k in 0..self_degree(in_start, in_from, n) {
                let m = match dependent_at(in_start, in_from, n, k) {
                    Some(m) => m,
                    None => continue,
                };
                let want = levels[n as usize] + 1;
                if levels[m as usize] < want {
                    levels[m as usize] = want;
                }
                remaining[m as usize] -= 1;
                if remaining[m as usize] == 0 {
                    queue.push(m);
                }
            }
        }
        if settled != nodes {
            return Err(diag_corrupt(
                &format!(
                    "有 {}/{} 个节点在层级消解中没能定级，说明依赖图里存在环",
                    nodes - settled,
                    nodes
                ),
                "环上的节点互相等待，层级无法收敛，级联会永远传播下去",
                "从源文件重建依赖图；若F3402 的环检测被绕过，请一并检查解析器",
            ));
        }
        // **刻意不在这里判链长上限**。判深度超限是 [`MAX_CHAIN_DEPTH`] 那道闸的
        // 职责，它在 `from_dag` 里、由 [`diag_chain_depth`] 出面——诊断码必须是
        // [`CascadeCode::DepthExceeded`]，处置是"把链拆短"。若在这里抢着报
        // [`CascadeCode::GraphCorrupt`]，深度超限就会被伪装成图损坏：调用方
        // 读到"重建依赖图"，于是反复重建同一张永远重建不出来的图——
        // 处置方向相反的两种状态绝不能共用一个码。
        Ok((levels, max_level))
    }

    /// 节点 `n` 的层级。
    pub fn level(&self, n: u32) -> Option<u32> {
        self.levels.get(n as usize).copied()
    }

    /// 节点 `n` 直接依赖了几个（出度）。
    pub fn out_degree(&self, n: u32) -> usize {
        let n = n as usize;
        if n + 1 >= self.out_start.len() {
            return 0;
        }
        (self.out_start[n + 1] - self.out_start[n]) as usize
    }

    /// 节点 `n` 被几个直接依赖（入度）。
    pub fn in_degree(&self, n: u32) -> usize {
        self_degree(&self.in_start, &self.in_from, n)
    }

    /// 节点 `n` 的第 `k` 个依赖目标。
    pub fn out_at(&self, n: u32, k: usize) -> Option<u32> {
        let n = n as usize;
        if n + 1 >= self.out_start.len() {
            return None;
        }
        let base = self.out_start[n] as usize;
        self.out_target.get(base + k).copied()
    }

    /// 节点 `n` 的第 `k` 个依赖者。
    pub fn dependent_at(&self, n: u32, k: usize) -> Option<u32> {
        dependent_at(&self.in_start, &self.in_from, n, k)
    }

    /// 按路径找节点。
    pub fn node_of(&self, path: &str) -> Option<u32> {
        self.paths.iter().position(|p| p == path).map(|i| i as u32)
    }

    /// 节点 `n` 的路径。
    pub fn path_of(&self, n: u32) -> Option<&str> {
        self.paths.get(n as usize).map(|s| s.as_str())
    }

    /// 求值序：按层级升序（依赖先于依赖方），同层内按节点号，保证确定性。
    ///
    /// **这是级联正确性的地基**：层级取的是最长派生链，所以任何被依赖方的
    /// 层级都严格小于依赖方，按此序求值时目标必然已就绪。
    pub fn eval_order(&self) -> Vec<u32> {
        let mut buckets: Vec<Vec<u32>> = Vec::new();
        buckets.resize(self.max_level as usize + 1, Vec::new());
        for n in 0..self.nodes {
            let lv = self.levels[n] as usize;
            buckets[lv].push(n as u32);
        }
        let mut out: Vec<u32> = Vec::with_capacity(self.nodes);
        for b in buckets.iter() {
            for n in b.iter() {
                out.push(*n);
            }
        }
        out
    }

    /// 全图完整性校验。**任何一项不过即图损坏，应当重建。**
    ///
    /// 校验项分两类：**形状类**（长度/范围/非降）与**交叉类**（反表必须是正表
    /// 的转置、层级必须能被正表复算出来）。交叉类才是真正抓损坏的——形状类
    /// 只能抓"明显坏了"，交叉类能抓"某一处被悄悄改过"。
    pub fn verify(&self) -> Result<(), CascadeDiag> {
        // ---- 形状一：各表长度。
        if self.paths.len() != self.nodes
            || self.levels.len() != self.nodes
            || self.out_start.len() != self.nodes + 1
            || self.in_start.len() != self.nodes + 1
        {
            return Err(diag_corrupt(
                &format!(
                    "依赖图表长对不上：节点 {}，路径 {}，层级 {}，正向行偏移 {}，反向行偏移 {}",
                    self.nodes,
                    self.paths.len(),
                    self.levels.len(),
                    self.out_start.len(),
                    self.in_start.len()
                ),
                "CSR 要求行偏移比节点多一格，其余表与节点数等长",
                "从源文件重建依赖图",
            ));
        }
        if self.out_target.len() != self.in_from.len() || self.out_target.len() != self.edges {
            return Err(diag_corrupt(
                &format!(
                    "正向列 {} 与反向列 {} 长度不等，两者本应是同一条边集的两个方向",
                    self.out_target.len(),
                    self.in_from.len()
                ),
                "两张 CSR 不同源，说明建图过程中有一侧被截断或被改过",
                "从源文件重建依赖图",
            ));
        }
        Self::check_csr_shape("正向", &self.out_start, self.out_target.len(), self.nodes)?;
        Self::check_csr_shape("反向", &self.in_start, self.in_from.len(), self.nodes)?;

        // ---- 形状二：列内节点号在范围内，且无自环。
        for (name, col) in [("正向", &self.out_target), ("反向", &self.in_from)] {
            for (i, v) in col.iter().enumerate() {
                if (*v as usize) >= self.nodes {
                    return Err(diag_corrupt(
                        &format!("{} CSR 第 {} 列的节点号 {} 越界", name, i, v),
                        "边指向了不存在的节点，级联会顺着它走进虚空",
                        "从源文件重建依赖图",
                    ));
                }
            }
        }
        for src in 0..self.nodes {
            for k in 0..self.out_degree(src as u32) {
                if self.out_at(src as u32, k) == Some(src as u32) {
                    return Err(diag_corrupt(
                        &format!("节点 {:?} 引用了自己", self.paths[src]),
                        "自引用是长度为 1 的环，永远求不出值",
                        "从源文件重建依赖图；若这是源文件的真实问题，请回到 F3402 修引用",
                    ));
                }
            }
        }

        // ---- 交叉一：反表必须是正表的转置。逐条正边 (src -> dst) 都要能在
        //      dst 的入区里找到 src。
        for src in 0..self.nodes {
            for k in 0..self.out_degree(src as u32) {
                let dst = match self.out_at(src as u32, k) {
                    Some(d) => d,
                    None => continue,
                };
                let mut found = false;
                for j in 0..self.in_degree(dst) {
                    if self.dependent_at(dst, j) == Some(src as u32) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(diag_corrupt(
                        &format!(
                            "正向边 {:?} -> {:?} 在反向表里找不到",
                            self.paths[src], self.paths[dst as usize]
                        ),
                        "正反两张表不是同一条边集的两个方向，转置关系被破坏了",
                        "从源文件重建依赖图",
                    ));
                }
            }
        }

        // ---- 交叉二：层级必须能被正表复算出来。存下来的层级只当**被审对象**，
        //      复算结果才是判据——否则就是"自己说自己对"。
        let (recomputed, recomputed_max) = match Self::compute_levels(
            self.nodes,
            &self.out_start,
            &self.out_target,
            &self.in_start,
            &self.in_from,
        ) {
            Ok(v) => v,
            Err(mut e) => {
                e.what = format!("复算层级时发现图已损坏：{}", e.what);
                return Err(e);
            }
        };
        for n in 0..self.nodes {
            if recomputed[n] != self.levels[n] {
                return Err(diag_corrupt(
                    &format!(
                        "节点 {:?} 存着的层级是 {}，按正向边复算出来是 {}",
                        self.paths[n], self.levels[n], recomputed[n]
                    ),
                    "层级被改过或正向边被改过，两者不再自洽；级联会按错序求值",
                    "从源文件重建依赖图",
                ));
            }
        }
        if recomputed_max != self.max_level {
            return Err(diag_corrupt(
                &format!(
                    "存着的最高层级是 {}，复算出来是 {}",
                    self.max_level, recomputed_max
                ),
                "层级摘要与逐点层级不同源",
                "从源文件重建依赖图",
            ));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 依赖可视化（判据一）+ 读屏替述（无障碍）
    // -----------------------------------------------------------------------

    /// 图描述文本（Graphviz DOT）。**E09 调试工具联动的产物之一**。
    pub fn dot(&self) -> String {
        let mut s = String::new();
        s.push_str("digraph tokens {\n");
        s.push_str("  rankdir=BT;\n");
        s.push_str("  node [shape=box, fontname=\"monospace\"];\n");
        for n in 0..self.nodes {
            s.push_str(&format!(
                "  n{} [label=\"{}\\nL{}\"];\n",
                n, self.paths[n], self.levels[n]
            ));
        }
        for src in 0..self.nodes {
            for k in 0..self.out_degree(src as u32) {
                if let Some(dst) = self.out_at(src as u32, k) {
                    s.push_str(&format!("  n{} -> n{};\n", dst, src));
                }
            }
        }
        s.push_str("}\n");
        s
    }

    /// 层级缩进大纲（人类可读的树形）。**不依赖终端宽度**：每层固定两格缩进。
    pub fn outline(&self) -> String {
        let per = self.per_level_counts();
        let mut s = String::new();
        s.push_str(&format!(
            "依赖大纲：{} 个令牌，{} 条依赖，{} 层\n",
            self.nodes,
            self.edges,
            self.max_level + 1
        ));
        for (lv, count) in per.iter().enumerate() {
            s.push_str(&format!("  第 {} 层：{} 个令牌\n", lv, count));
            for n in 0..self.nodes {
                if self.levels[n] as usize == lv {
                    s.push_str(&format!(
                        "    {}（被{} 个直接依赖）\n",
                        self.paths[n],
                        self.in_degree(n as u32)
                    ));
                }
            }
        }
        s
    }

    /// **依赖图的读屏替述**（无障碍硬要求：图不能只以图的形式存在）。
    ///
    /// 内容与 [`DepGraph::dot`] 等信息量：节点数、边数、层数、逐层节点数、最深
    /// 链上的令牌、被依赖最多的令牌。不依赖颜色、不依赖缩进宽度、不依赖终端
    /// 列数——朗读顺序即字段顺序。
    pub fn spoken(&self) -> String {
        let per = self.per_level_counts();
        let mut s = String::new();
        s.push_str(&format!(
            "令牌依赖图：共 {} 个令牌，{} 条依赖边，分 {} 层。最深的是第 {} 层。",
            self.nodes,
            self.edges,
            self.max_level + 1,
            self.max_level
        ));
        for (lv, count) in per.iter().enumerate() {
            s.push_str(&format!("第 {} 层 {} 个令牌。", lv, count));
        }
        // 点名最深链末端：只报"最深是第几层"，读者不知道深在哪。
        let mut deepest: Vec<&str> = Vec::new();
        for n in 0..self.nodes {
            if self.levels[n] == self.max_level {
                deepest.push(self.paths[n].as_str());
            }
        }
        if !deepest.is_empty() {
            s.push_str(&format!("最深链的末端是：{}。", deepest.join("、")));
        }
        // 被依赖最多的令牌：整张图里传播力最强的那一个，改值时最该先看它。
        if self.nodes > 0 {
            let mut worst = 0usize;
            for n in 1..self.nodes {
                if self.in_degree(n as u32) > self.in_degree(worst as u32) {
                    worst = n;
                }
            }
            s.push_str(&format!(
                "被依赖最多的令牌是 {}，有 {} 个令牌直接依赖它。",
                self.paths[worst],
                self.in_degree(worst as u32)
            ));
        }
        s
    }

    /// 单个节点的读屏替述：它依赖谁、被谁依赖、在第几层。
    pub fn node_spoken(&self, path: &str) -> Option<String> {
        let n = self.node_of(path)?;
        let mut deps: Vec<&str> = Vec::new();
        for k in 0..self.out_degree(n) {
            if let Some(p) = self.out_at(n, k).and_then(|d| self.path_of(d)) {
                deps.push(p);
            }
        }
        let mut dependents: Vec<&str> = Vec::new();
        for k in 0..self.in_degree(n) {
            if let Some(p) = self.dependent_at(n, k).and_then(|d| self.path_of(d)) {
                dependents.push(p);
            }
        }
        let mut s = String::new();
        s.push_str(&format!("令牌 {} 在第 {} 层。", path, self.levels[n as usize]));
        if deps.is_empty() {
            s.push_str("它不依赖别的令牌，是基础令牌。");
        } else {
            s.push_str(&format!("它依赖：{}。", deps.join("、")));
        }
        if dependents.is_empty() {
            s.push_str("没有令牌依赖它。");
        } else {
            s.push_str(&format!("依赖它的令牌有：{}。", dependents.join("、")));
        }
        Some(s)
    }

    /// **E09 调试工具联动的产物之二**：有界邻域切片。
    ///
    /// 半径受 [`MAX_DEBUG_RADIUS`] 闸住——调试工具不能要求无界邻域，那等于要
    /// 整张图。切片沿**反向边**扩散，与级联同方向：调试"改这个会波及谁"。
    pub fn debug_slice(&self, path: &str, radius: usize) -> Result<DebugSlice, CascadeDiag> {
        if radius > MAX_DEBUG_RADIUS {
            return Err(CascadeDiag::new(
                CascadeCode::DepthExceeded,
                Site::start(),
                &format!("调试切片半径 {} 超过上限 {}", radius, MAX_DEBUG_RADIUS),
                "邻域切片必须是有界的，无界邻域等于把整张图复制一遍",
                &format!("把半径收到 {} 以内；确实要看更远，请改用分层大纲", MAX_DEBUG_RADIUS),
            ));
        }
        let start = self.node_of(path).ok_or_else(|| {
            CascadeDiag::new(
                CascadeCode::NoSuchPath,
                Site::start(),
                &format!("依赖图里没有路径 {:?}", path),
                "切片起点必须落在图内的节点上",
                "先核对路径拼写；路径区分大小写且用点号分层",
            )
        })?;
        let mut visited: Vec<u8> = Vec::new();
        visited.resize(self.nodes, 0u8);
        let mut layers: Vec<Vec<u32>> = Vec::new();
        layers.resize(radius + 1, Vec::new());
        visited[start as usize] = 1;
        layers[0].push(start);
        // 只在 d < radius 时展开——d == radius 那一层只看不再扩散，
        // 越界写layers[d + 1] 会 panic（半径恰为边界时的真实缺陷）。
        let mut d = 0usize;
        while d < radius {
            let frontier = layers[d].clone();
            for n in frontier.iter() {
                for k in 0..self.in_degree(*n) {
                    if let Some(m) = self.dependent_at(*n, k) {
                        if visited[m as usize] == 0 {
                            visited[m as usize] = 1;
                            layers[d + 1].push(m);
                        }
                    }
                }
            }
            d += 1;
        }
        let at_radius = layers[radius].len();
        Ok(DebugSlice {
            origin: path.to_string(),
            radius,
            layers,
            at_radius,
        })
    }

    /// 逐层节点计数（层号 → 节点数），长度 `max_level + 1`。
    pub fn per_level_counts(&self) -> Vec<usize> {
        let mut per: Vec<usize> = Vec::new();
        per.resize(self.max_level as usize + 1, 0);
        for n in 0..self.nodes {
            let lv = self.levels[n] as usize;
            if lv < per.len() {
                per[lv] += 1;
            }
        }
        per
    }
}

/// 反向 CSR 的出度（自由函数版，供建图期复用，避免与 `DepGraph` 耦合）。
fn self_degree(start: &[u32], _col: &[u32], n: u32) -> usize {
    let n = n as usize;
    if n + 1 >= start.len() {
        return 0;
    }
    (start[n + 1] - start[n]) as usize
}

/// 反向 CSR 的第 `k` 个依赖者（自由函数版）。
fn dependent_at(start: &[u32], col: &[u32], n: u32, k: usize) -> Option<u32> {
    let n = n as usize;
    if n + 1 >= start.len() {
        return None;
    }
    let base = start[n] as usize;
    col.get(base + k).copied()
}

/// E09 调试工具消费的邻域切片。
#[derive(Clone, Debug)]
pub struct DebugSlice {
    /// 切片起点路径。
    pub origin: String,
    /// 实际半径。
    pub radius: usize,
    /// 逐层节点（第 0 层是起点自己）。
    pub layers: Vec<Vec<u32>>,
    /// 半径那一层的节点数（再往外没算，报这个数让人知道切片是截断的）。
    pub at_radius: usize,
}

impl DebugSlice {
    /// 切片里的节点总数（层内不重复，跨层也不重复——扩散时用 visited 去重）。
    pub fn total(&self) -> usize {
        let mut n = 0usize;
        for l in self.layers.iter() {
            n += l.len();
        }
        n
    }

    /// 切片的读屏替述。
    pub fn spoken(&self, g: &DepGraph) -> String {
        let mut s = format!(
            "以 {} 为起点、半径 {} 的依赖切片，共命中 {} 个令牌。",
            self.origin,
            self.radius,
            self.total()
        );
        for (d, l) in self.layers.iter().enumerate() {
            if l.is_empty() {
                continue;
            }
            s.push_str(&format!("第 {} 跳：", d));
            for n in l.iter() {
                if let Some(p) = g.path_of(*n) {
                    s.push_str(p);
                    s.push('、');
                }
            }
            s.push_str("。");
        }
        if self.at_radius > 0 {
            s.push_str(&format!(
                "半径 {} 处仍有 {} 个令牌未展开，切片到此为止。",
                self.radius, self.at_radius
            ));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 四、级联结果与耗时画像
// ---------------------------------------------------------------------------

/// 被级联命中的一个节点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CascadeNode {
    /// 节点号。
    pub node: u32,
    /// 路径。
    pub path: String,
    /// 距种子集的跳数（0 = 种子自己）。
    pub depth: u32,
}

/// 一次级联的结果。
#[derive(Clone, Debug)]
pub struct CascadeOutcome {
    /// 本批种子路径（已去重）。
    pub seeds: Vec<String>,
    /// 受影响节点，按层级升序（= 求值序）。
    pub affected: Vec<CascadeNode>,
    /// 值**真的变了**的节点路径（实测差分，不是级联范围的转述）。
    pub changed: Vec<String>,
    /// 距种子集的最大跳数。
    pub max_depth: u32,
    /// 本次走过的反向边条数（真实工作量的主项）。
    pub edges_touched: usize,
    /// 是否走了去重合并（同路径多种子被并成一次）。
    pub merged: bool,
    /// 合并省下的节点访问数（= 逐个级联之和 − 合并后一次），由 `cascade_merged` 填。
    pub saved_visits: usize,
}

impl CascadeOutcome {
    /// 受影响节点数。
    pub fn affected_len(&self) -> usize {
        self.affected.len()
    }
}

/// 耗时画像的单层桶。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LevelCost {
    /// 层级（此处是**距种子的跳数**，不是依赖图的层级）。
    pub depth: u32,
    /// 该跳被级联命中的节点数。
    pub nodes: u32,
    /// 该跳做过的引用拼接次数。
    pub splices: u32,
}

/// 级联耗时画像。
///
/// **计的是工作量单位，不是墙钟**——本模块一次都不读时钟（零墙钟是内核回归
/// 可复现的硬要求）。真实耗时由调用方注入（[`CascadeProfile::mark_ticks`]）。
#[derive(Clone, Debug, Default)]
pub struct CascadeProfile {
    /// 级联批次数。
    pub cascades: usize,
    /// 种子总数（含被去重合并掉的重复种子）。
    pub seeds_total: usize,
    /// 被求值的节点总数。
    pub nodes_evaluated: usize,
    /// 走过的反向边总数。
    pub edges_touched: usize,
    /// 引用拼接总次数。
    pub splices: usize,
    /// 触发批量合并的批次数。
    pub merges: usize,
    /// 合并累计省下的节点访问数。
    pub saved_visits: usize,
    /// 逐跳桶。
    pub per_hop: Vec<LevelCost>,
    /// 见过的最大跳数。
    pub max_depth_seen: u32,
    /// 深度闸拒绝次数。
    pub depth_rejects: usize,
    /// 图损坏重建次数。
    pub rebuilds: usize,
    /// 调用方注入的真实滴答（`None` = 本次没注入）。
    pub ticks: Option<u64>,
    /// 因桶上限被丢弃的跳数（**显性化，不静默截断**）。
    pub dropped: usize,
}

impl CascadeProfile {
    /// 记录一批级联的工作量。
    pub fn record(&mut self, outcome: &CascadeOutcome, splices: usize, splices_per_hop: &[u32]) {
        self.cascades += 1;
        self.seeds_total += outcome.seeds.len();
        self.nodes_evaluated += outcome.affected_len();
        self.edges_touched += outcome.edges_touched;
        self.splices += splices;
        if outcome.merged {
            self.merges += 1;
        }
        self.saved_visits += outcome.saved_visits;
        if outcome.max_depth > self.max_depth_seen {
            self.max_depth_seen = outcome.max_depth;
        }
        for (d, cnt) in splices_per_hop.iter().enumerate() {
            if *cnt == 0 {
                continue;
            }
            if d >= MAX_PROFILE_LEVELS {
                self.dropped += 1;
                continue;
            }
            while self.per_hop.len() <= d {
                self.per_hop.push(LevelCost::default());
            }
            self.per_hop[d].depth = d as u32;
            self.per_hop[d].splices += *cnt;
        }
        // 逐跳节点数单独一遍（与拼接数分开计，两者可能不同：
        // 一个节点可能只落一个没有引用的字面量，拼接数为 0）。
        let mut nodes_per_hop: Vec<u32> = Vec::new();
        nodes_per_hop.resize(outcome.max_depth as usize + 1, 0);
        for c in outcome.affected.iter() {
            let d = c.depth as usize;
            if d < nodes_per_hop.len() {
                nodes_per_hop[d] += 1;
            }
        }
        for (d, cnt) in nodes_per_hop.iter().enumerate() {
            if *cnt == 0 {
                continue;
            }
            if d >= MAX_PROFILE_LEVELS {
                self.dropped += 1;
                continue;
            }
            while self.per_hop.len() <= d {
                self.per_hop.push(LevelCost::default());
            }
            self.per_hop[d].nodes += *cnt;
        }
    }

    /// 记录一次深度闸拒绝。
    pub fn note_depth_reject(&mut self) {
        self.depth_rejects += 1;
    }

    /// 记录一次图重建。
    pub fn note_rebuild(&mut self) {
        self.rebuilds += 1;
    }

    /// 调用方注入真实时钟读数。本模块自己**不读时钟**。
    pub fn mark_ticks(&mut self, begin: u64, end: u64) {
        self.ticks = Some(end.saturating_sub(begin));
    }

    /// 画像的读屏替述。
    pub fn spoken(&self) -> String {
        let mut s = format!(
            "级联耗时画像：{} 批，种子 {} 个，求值节点 {} 个，走过依赖边 {} 条，拼接 {} 次。",
            self.cascades, self.seeds_total, self.nodes_evaluated, self.edges_touched, self.splices
        );
        if self.merges > 0 {
            s.push_str(&format!(
                "其中 {} 批走了合并，累计省下 {} 次节点访问。",
                self.merges, self.saved_visits
            ));
        }
        s.push_str(&format!("最大级联深度 {}。", self.max_depth_seen));
        if self.depth_rejects > 0 {
            s.push_str(&format!("深度闸拒绝了 {} 次。", self.depth_rejects));
        }
        if self.rebuilds > 0 {
            s.push_str(&format!("图损坏重建了 {} 次。", self.rebuilds));
        }
        if self.dropped > 0 {
            s.push_str(&format!(
                "有 {} 跳的用量超出了画像桶上限，未计入分明细。",
                self.dropped
            ));
        }
        match self.ticks {
            Some(t) => s.push_str(&format!("调用方测得真实耗时 {} 滴答。", t)),
            None => s.push_str("本模块不读时钟，本次没有真实耗时读数。"),
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 五、变更队列（级联风暴 → 批量合并）
// ---------------------------------------------------------------------------

/// 队列里的一条待级联改动。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingChange {
    /// 被改的令牌路径。
    pub path: String,
    /// 进入队列的滴答。
    pub tick: u64,
}

/// 待级联改动队列。**级联风暴在此被合并成一批**。
///
/// 合并的语义要说清：窗口内的多次改动**不是**各自跑一遍级联再叠加，而是
/// **先并种子、再走一遍**——受影响集合取并集、每个节点只求值一次。
/// 这也是省下来的工作量能被实测出来的原因。
#[derive(Clone, Debug, Default)]
pub struct ChangeQueue {
    /// 待处理改动。
    pub pending: Vec<PendingChange>,
}

impl ChangeQueue {
    /// 新队列。
    pub fn new() -> ChangeQueue {
        ChangeQueue { pending: Vec::new() }
    }

    /// 队列长度。
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// 队列是否为空。
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// 投入一条改动。**种子数超 [`MAX_BATCH_SEEDS`] 即拒绝**。
    pub fn push(&mut self, path: &str, tick: u64) -> Result<(), CascadeDiag> {
        if self.pending.len() >= MAX_BATCH_SEEDS {
            return Err(CascadeDiag::new(
                CascadeCode::BatchTooLarge,
                Site::start(),
                &format!(
                    "级联队列已有 {} 条改动，再投入就超过上限 {}",
                    self.pending.len(),
                    MAX_BATCH_SEEDS
                ),
                "一帧之内的改动不是风暴而是重载，一次算不完",
                &format!(
                    "先把已入队的这批级联掉再继续投入，或把种子上限从 {} 调高",
                    MAX_BATCH_SEEDS
                ),
            ));
        }
        self.pending.push(PendingChange {
            path: path.to_string(),
            tick,
        });
        Ok(())
    }

    /// 取走一批待级联改动。返回 `(本批, 留待后处理)`。
    ///
    /// - **本批**：滴答落在 `[now - MERGE_WINDOW_TICKS, now]` 内的，**路径去重**
    ///   后作为合并种子——同一路径改两次只算一次，这是合并的第一层省钱；
    /// - **留待后处理**：比窗口还早的（过期）与滴答晚于 `now` 的（时钟回拨或
    ///   时基不一致）。它们**不静默丢弃**，原样交回调用方处置——丢掉就等于让
    ///   用户的改动凭空消失，那比慢更坏。
    pub fn drain(&mut self, now: u64) -> (Vec<PendingChange>, Vec<PendingChange>) {
        let lo = now.saturating_sub(MERGE_WINDOW_TICKS);
        let mut batch: Vec<PendingChange> = Vec::new();
        let mut held: Vec<PendingChange> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for c in self.pending.iter() {
            if c.tick > now || c.tick < lo {
                held.push(c.clone());
                continue;
            }
            if seen.iter().any(|p| *p == c.path) {
                continue;
            }
            seen.push(c.path.clone());
            batch.push(c.clone());
        }
        self.pending = held.clone();
        (batch, held)
    }
}

// ---------------------------------------------------------------------------
// 六、级联引擎
// ---------------------------------------------------------------------------

/// 令牌级联引擎。持有依赖图、当前值、已仲裁的覆盖结果与耗时画像。
pub struct CascadeEngine {
    /// 令牌集（重建图时要用）。
    pub ts: TokenSet,
    /// F3402 交出的引用 DAG（重建图时要用）。
    pub dag: TokenDag,
    /// 当前依赖图。
    pub graph: DepGraph,
    /// 各节点当前值，与节点号同序。
    pub values: Vec<String>,
    /// 已仲裁的覆盖结果（路径 → 字面量），**按路径字典序**存，二分查找。
    /// 仲裁归 F3406，本模块只消费结果。
    pub overrides: Vec<(String, String)>,
    /// 耗时画像。
    pub profile: CascadeProfile,
}

impl CascadeEngine {
    /// 建引擎：建图 → 全量求值。**建图失败或全量求值失败即拒绝建引擎**，
    /// 不给出一个"半可用"的引擎。
    pub fn new(ts: TokenSet, dag: TokenDag) -> Result<CascadeEngine, CascadeDiag> {
        let graph = DepGraph::from_dag(&ts, &dag)?;
        let mut values: Vec<String> = Vec::new();
        values.resize(graph.nodes, String::new());
        let mut engine = CascadeEngine {
            ts,
            dag,
            graph,
            values,
            overrides: Vec::new(),
            profile: CascadeProfile::default(),
        };
        let mut splices = 0usize;
        engine.resolve_all(&mut splices)?;
        Ok(engine)
    }

    /// 依赖图（只读）。
    pub fn graph(&self) -> &DepGraph {
        &self.graph
    }

    /// 耗时画像（只读）。
    pub fn profile(&self) -> &CascadeProfile {
        &self.profile
    }

    /// 耗时画像（可变）。**供自检注入调用方时钟读数用**——本模块自己不读时钟，
    /// 真实耗时只能由外部注入，这个入口就是那个"外部"。
    pub fn profile_mut_for_test(&mut self) -> &mut CascadeProfile {
        &mut self.profile
    }

    /// 依赖图（可变）。**仅供自检注入损坏**：验证"图被改坏之后 verify 能不能
    /// 抓到"。正常使用路径不该拿到它，故名字里带 `for_test` 明示。
    pub fn graph_mut_for_test(&mut self) -> &mut DepGraph {
        &mut self.graph
    }

    /// 覆盖表（只读迭代）。供自检核对二分查找的不变式（按字典序）。
    pub fn override_entries(&self) -> &[(String, String)] {
        &self.overrides
    }

    /// 取某路径的当前值。
    pub fn value_of(&self, path: &str) -> Option<&str> {
        let n = self.graph.node_of(path)?;
        self.values.get(n as usize).map(|s| s.as_str())
    }

    /// 设置覆盖结果。**调用方必须已持有该层级的授权**——越权仲裁归 F3406，
    /// 本模块不做权限判断，也不该做：两处各判一次会出现"两处规则不同"的分叉，
    /// 而授权规则只有一个出处。传 [`NO_OVERRIDE`] 取消该路径的覆盖。
    pub fn set_override(&mut self, path: &str, literal: &str) -> Result<(), CascadeDiag> {
        if self.graph.node_of(path).is_none() {
            return Err(CascadeDiag::new(
                CascadeCode::NoSuchPath,
                Site::start(),
                &format!("覆盖目标路径 {:?} 不在依赖图里", path),
                "覆盖一个不存在的令牌，等于往图里塞一个没有定义点的悬空引用",
                "先核对路径；令牌集变更后必须重建引擎再设覆盖",
            ));
        }
        if literal == NO_OVERRIDE {
            self.overrides.retain(|(p, _)| p != path);
            return Ok(());
        }
        let mut found = false;
        for e in self.overrides.iter_mut() {
            if e.0 == path {
                e.1 = literal.to_string();
                found = true;
                break;
            }
        }
        if !found {
            self.overrides.push((path.to_string(), literal.to_string()));
            self.overrides
                .sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        }
        Ok(())
    }

    /// 覆盖条数。
    pub fn override_len(&self) -> usize {
        self.overrides.len()
    }

    /// 查覆盖值（二分，保持 `overrides` 的字典序不变式）。
    pub fn override_of(&self, path: &str) -> Option<&str> {
        self.overrides
            .binary_search_by(|e| e.0.as_bytes().cmp(path.as_bytes()))
            .ok()
            .map(|i| self.overrides[i].1.as_str())
    }

    /// 单节点求值：覆盖优先，否则把原文里的 `{引用}` 逐处替换成目标的已求值值。
    ///
    /// **必须按 [`DepGraph::eval_order`] 的顺序调用**——层级取最长派生链，
    /// 保证目标节点先于依赖方就绪。
    ///
    /// 替换按**原文逐处扫描**进行，同一目标出现多次就替换多次。引用登记表
    /// （`extract_refs`）按出现顺序**去重**，只留首次；所以拿登记表去定位会漏掉
    /// 第二处——那会让 `1px {c} 1px {c}` 只换掉一半，界面出现半新半旧。
    fn evaluate_node(&self, n: u32, splices: &mut usize) -> Result<String, CascadeDiag> {
        let path = match self.graph.path_of(n) {
            Some(p) => p,
            None => {
                return Err(CascadeDiag::new(
                    CascadeCode::NodeOutOfRange,
                    Site::start(),
                    &format!("求值时节点号 {} 不在图内", n),
                    "节点号越界说明图与令牌表已经不同源",
                    "重建引擎（rebuild）后再试",
                ))
            }
        };
        if let Some(lit) = self.override_of(path) {
            return Ok(lit.to_string());
        }
        let entry = match self.ts.entries.get(n as usize) {
            Some(e) => e,
            None => {
                return Err(CascadeDiag::new(
                    CascadeCode::NodeOutOfRange,
                    entry_site(),
                    &format!("节点 {:?} 在令牌表里没有对应条目", path),
                    "图里有节点而令牌表里没有，说明两表不同源",
                    "重建引擎后再试",
                ))
            }
        };
        let site = entry.site;
        let raw = entry.raw.clone();
        if !raw.contains(super::ver01b_parser::REF_OPEN) {
            return Ok(raw);
        }
        // 先过一遍引用抽取：它的作用是**校验**（空引用、非法段、未闭合），
        // 位置替换另有扫描（抽取结果按出现顺序去重，不能直接用来定位）。
        //
        // F3402 的 `Diag` 与本模块的 `CascadeDiag` 是两套类型，`?` 走不通，
        // 故用显式 `map_err` 桥接——**保留 F3402 的诊断码与三要素**，
        // 不在这里把它压成本模块的码：解析期的错就该说解析期的话，
        // 压成级联码会让"错在哪一层"这条信息丢掉。
        extract_refs(&raw, site).map_err(|d| CascadeDiag::new(
            CascadeCode::GraphCorrupt,
            d.site,
            &format!("令牌 {:?} 的引用无法解析：{}", path, d.what),
            &format!("引用抽取失败：{}。这是令牌原文的问题，不是级联图的问题", d.why),
            &d.fix,
        ))?;
        let chars: Vec<char> = raw.chars().collect();
        let mut out = String::new();
        let mut i = 0usize;
        while i < chars.len() {
            // 花括号转义：`{{` 与 `}}` 是**字面**花括号，不是引用定界。
            // 求值时必须把它们**收敛成一个**——`extract_refs` 认定 `{{` 不是
            // 引用（所以不进引用表），若这里原样吐回两个 `{`，产出就与
            // "引用定界"语义打架：`{{base}}` 会被求值成 `{{base}}` 而不是
            // `{base}`，令牌作者写的字面花括号凭空多了一层。
            if chars[i] == super::ver01b_parser::REF_OPEN
                && i + 1 < chars.len()
                && chars[i + 1] == super::ver01b_parser::REF_OPEN
            {
                out.push(super::ver01b_parser::REF_OPEN);
                i += 2;
                continue;
            }
            if chars[i] == super::ver01b_parser::REF_CLOSE
                && i + 1 < chars.len()
                && chars[i + 1] == super::ver01b_parser::REF_CLOSE
            {
                out.push(super::ver01b_parser::REF_CLOSE);
                i += 2;
                continue;
            }
            if chars[i] != super::ver01b_parser::REF_OPEN {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            let mut j = i + 1;
            let mut target = String::new();
            let mut closed = false;
            while j < chars.len() {
                if chars[j] == super::ver01b_parser::REF_CLOSE {
                    closed = true;
                    break;
                }
                target.push(chars[j]);
                j += 1;
            }
            if !closed {
                return Err(CascadeDiag::new(
                    CascadeCode::GraphCorrupt,
                    site,
                    &format!("令牌 {:?} 的原文里有没闭合的引用花括号", path),
                    "引用必须成对出现，右花括号缺失",
                    "补上右花括号；若本意是字面花括号，写成两个左花括号",
                ));
            }
            let target = target.trim().to_string();
            let dst = match self.graph.node_of(&target) {
                Some(d) => d,
                None => {
                    return Err(CascadeDiag::new(
                        CascadeCode::NoSuchPath,
                        site,
                        &format!("令牌 {:?} 引用的 {:?} 不在依赖图里", path, target),
                        "引用目标不存在，值求不出来",
                        "补上被引用的令牌定义，或把引用改到已存在的路径上",
                    ))
                }
            };
            let resolved = match self.values.get(dst as usize) {
                Some(v) if !v.is_empty() => v.clone(),
                _ => {
                    return Err(CascadeDiag::new(
                        CascadeCode::GraphCorrupt,
                        site,
                        &format!(
                            "求值 {:?} 时它依赖的 {:?} 还没有值，说明求值序被破坏了",
                            path, target
                        ),
                        "层级升序求值本应保证目标先就绪；没就绪说明层级算错了",
                        "重建依赖图并核对层级；若层级是对的，检查是不是有节点没被写进求值序",
                    ))
                }
            };
            out.push_str(&resolved);
            *splices += 1;
            i = j + 1;
            if out.len() > MAX_VALUE_LEN {
                return Err(CascadeDiag::new(
                    CascadeCode::ValueTooLong,
                    site,
                    &format!(
                        "令牌 {:?} 求值结果超过 {} 字节",
                        path, MAX_VALUE_LEN
                    ),
                    "嵌引用会指数膨胀，没有闸就是内存耗尽",
                    &format!(
                        "把引用拆成多级中间令牌，或把上限从 {} 调高",
                        MAX_VALUE_LEN
                    ),
                ));
            }
        }
        if out.len() > MAX_VALUE_LEN {
            return Err(CascadeDiag::new(
                CascadeCode::ValueTooLong,
                site,
                &format!(
                    "令牌 {:?} 求值结果 {} 字节，超过上限 {}",
                    path,
                    out.len(),
                    MAX_VALUE_LEN
                ),
                "嵌引用会指数膨胀",
                "把引用拆成多级中间令牌",
            ));
        }
        Ok(out)
    }

    /// 全量求值（按层级升序）。
    fn resolve_all(&mut self, splices: &mut usize) -> Result<(), CascadeDiag> {
        let order = self.graph.eval_order();
        if order.len() != self.graph.nodes {
            return Err(diag_corrupt(
                &format!(
                    "求值序只有 {} 项，图里有 {} 个节点",
                    order.len(),
                    self.graph.nodes
                ),
                "有节点没被排进求值序，它将永远保持空值",
                "重建依赖图；若层级正确，检查层桶分配是否漏了高层级",
            ));
        }
        for n in order {
            let v = self.evaluate_node(n, splices)?;
            self.values[n as usize] = v;
        }
        Ok(())
    }

    /// 影响面测算（**不落值**）：只算从种子出发的可达集与边数。
    ///
    /// 深度闸在此生效，故 [`CascadeEngine::cascade`] 与本函数共用同一套扩散逻辑
    /// ——两处各写一遍扩散，早晚会长歪。
    fn impact(&self, seed_nodes: &[u32]) -> Result<Impact, CascadeDiag> {
        let mut depth: Vec<u32> = Vec::new();
        depth.resize(self.graph.nodes, u32::MAX);
        let mut frontier: Vec<u32> = Vec::new();
        for n in seed_nodes.iter() {
            depth[*n as usize] = 0;
            frontier.push(*n);
        }
        let mut edges_touched = 0usize;
        let mut reached: Vec<u32> = Vec::new();
        let mut head = 0usize;
        let mut max_depth = 0u32;
        while head < frontier.len() {
            let n = frontier[head];
            head += 1;
            reached.push(n);
            let d = depth[n as usize];
            if d > max_depth {
                max_depth = d;
            }
            for k in 0..self.graph.in_degree(n) {
                edges_touched += 1;
                let m = match self.graph.dependent_at(n, k) {
                    Some(m) => m,
                    None => continue,
                };
                if depth[m as usize] != u32::MAX {
                    continue;
                }
                let nd = d + 1;
                // **级联闸在扩散当下就拦**，不是走完再报：走完再报意味着已经
                // 白跑了几十跳，且报出来的"太远"节点是任意的第一个越界者。
                if nd as usize > MAX_CASCADE_DEPTH {
                    let far = self.graph.path_of(m).unwrap_or("");
                    return Err(diag_cascade_depth(
                        self.graph.path_of(seed_nodes[0]).unwrap_or(""),
                        far,
                        nd,
                        MAX_CASCADE_DEPTH,
                    ));
                }
                depth[m as usize] = nd;
                frontier.push(m);
            }
        }
        Ok(Impact {
            reached,
            depth,
            max_depth,
            edges_touched,
        })
    }

    /// **级联**：从一组种子出发，沿反向边把影响面全走一遍，然后只重算这一片。
    ///
    /// 种子数超 [`MAX_BATCH_SEEDS`] 即拒绝；影响面距种子超 [`MAX_CASCADE_DEPTH`]
    /// 即拒绝。
    pub fn cascade(&mut self, seeds: &[&str]) -> Result<CascadeOutcome, CascadeDiag> {
        let merged = seeds.len() > dedup_count(seeds);
        if seeds.len() > MAX_BATCH_SEEDS {
            return Err(CascadeDiag::new(
                CascadeCode::BatchTooLarge,
                Site::start(),
                &format!(
                    "一次级联投入 {} 个种子，超过上限 {}",
                    seeds.len(),
                    MAX_BATCH_SEEDS
                ),
                "种子过多说明这批改动过于分散，不是级联而是重载",
                &format!(
                    "把种子拆成几批分别级联，或把种子上限从 {} 调高",
                    MAX_BATCH_SEEDS
                ),
            ));
        }

        // 去重并解析节点号。**去重必须在扩散之前**：同一路径给两次不该让它
        // 被算两遍。
        let mut seed_nodes: Vec<u32> = Vec::new();
        let mut seed_paths: Vec<String> = Vec::new();
        for s in seeds.iter() {
            if seed_paths.iter().any(|p| p == s) {
                continue;
            }
            let n = match self.graph.node_of(s) {
                Some(n) => n,
                None => {
                    return Err(CascadeDiag::new(
                        CascadeCode::NoSuchPath,
                        Site::start(),
                        &format!("级联种子路径 {:?} 不在依赖图里", s),
                        "种子不存在就没有影响面可言",
                        "先核对路径；令牌集变更后必须重建引擎",
                    ))
                }
            };
            seed_paths.push(s.to_string());
            seed_nodes.push(n);
        }
        if seed_nodes.is_empty() {
            return Ok(CascadeOutcome {
                seeds: seed_paths,
                affected: Vec::new(),
                changed: Vec::new(),
                max_depth: 0,
                edges_touched: 0,
                merged,
                saved_visits: 0,
            });
        }

        let imp = match self.impact(&seed_nodes) {
            Ok(v) => v,
            Err(e) => {
                if e.code.is_depth() {
                    self.profile.note_depth_reject();
                }
                return Err(e);
            }
        };

        // 受影响节点按层级升序重排——重算必须按求值序，不是按发现序。
        let mut affected: Vec<CascadeNode> = Vec::with_capacity(imp.reached.len());
        for n in imp.reached.iter() {
            affected.push(CascadeNode {
                node: *n,
                path: self.graph.path_of(*n).unwrap_or("").to_string(),
                depth: imp.depth[*n as usize],
            });
        }
        affected.sort_by(|a, b| {
            let la = self.graph.level(a.node).unwrap_or(0);
            let lb = self.graph.level(b.node).unwrap_or(0);
            la.cmp(&lb).then(a.node.cmp(&b.node))
        });

        // 记旧值 → 只重算受影响片 → 与旧值逐项差分。
        let mut old: Vec<String> = Vec::with_capacity(affected.len());
        for c in affected.iter() {
            old.push(self.values[c.node as usize].clone());
        }
        let mut splices = 0usize;
        let mut splices_per_hop: Vec<u32> = Vec::new();
        splices_per_hop.resize(imp.max_depth as usize + 1, 0);
        for c in affected.iter() {
            let before = splices;
            let v = self.evaluate_node(c.node, &mut splices)?;
            self.values[c.node as usize] = v;
            let d = c.depth as usize;
            if d < splices_per_hop.len() {
                splices_per_hop[d] += (splices - before) as u32;
            }
        }
        let mut changed: Vec<String> = Vec::new();
        for (i, c) in affected.iter().enumerate() {
            if old[i] != self.values[c.node as usize] {
                changed.push(c.path.clone());
            }
        }

        let outcome = CascadeOutcome {
            seeds: seed_paths,
            affected,
            changed,
            max_depth: imp.max_depth,
            edges_touched: imp.edges_touched,
            merged,
            saved_visits: 0,
        };
        self.profile.record(&outcome, splices, &splices_per_hop);
        Ok(outcome)
    }

    /// **批量合并级联**：把若干个种子并成一次级联，并**实测**省下的工作量。
    ///
    /// 返回 `(合并结果, 合并前逐个级联的成本, 合并后一次的成本)`。后两个数是
    /// 省下的**证据**：正确的合并实现必须满足
    /// `合并后 < 合并前`（种子多于一个且影响面有重叠时），用**单边符号**判定。
    ///
    /// 合并前的成本只做**测算**（[`CascadeEngine::impact`] 不落值），不真的
    /// 逐个级联一遍——那会把同一批值算两遍，既浪费又让画像虚高。
    pub fn cascade_merged(
        &mut self,
        seeds: &[&str],
    ) -> Result<(CascadeOutcome, usize, usize), CascadeDiag> {
        let mut individual = 0usize;
        let mut seen: Vec<u32> = Vec::new();
        for s in seeds.iter() {
            if seen.iter().any(|n| self.graph.path_of(*n) == Some(*s)) {
                continue;
            }
            let n = match self.graph.node_of(s) {
                Some(n) => n,
                None => continue,
            };
            seen.push(n);
            if let Ok(imp) = self.impact(&[n]) {
                individual += imp.reached.len() + imp.edges_touched;
            }
        }
        let outcome = self.cascade(seeds)?;
        let merged_cost = outcome.affected_len() + outcome.edges_touched;
        Ok((outcome, individual, merged_cost))
    }

    /// **图损坏 → 重建**：校验当前图，不过就丢弃并从源重建。
    ///
    /// 返回是否发生了重建（0 = 原来的图是好的）。**不在坏图上继续级联**——
    /// 坏图上的级联结果不可信，宁可重来。
    pub fn rebuild(&mut self) -> Result<usize, CascadeDiag> {
        if self.graph.verify().is_ok() {
            return Ok(0);
        }
        let fresh = DepGraph::from_dag(&self.ts, &self.dag)?;
        fresh.verify()?;
        let mut values: Vec<String> = Vec::new();
        values.resize(fresh.nodes, String::new());
        self.graph = fresh;
        self.values = values;
        let mut splices = 0usize;
        self.resolve_all(&mut splices)?;
        self.profile.note_rebuild();
        Ok(1)
    }
}

/// 影响面测算结果（私有）。
struct Impact {
    /// 可达节点（发现序）。
    reached: Vec<u32>,
    /// 每节点距种子的跳数（`u32::MAX` = 不可达）。
    depth: Vec<u32>,
    /// 最大跳数。
    max_depth: u32,
    /// 走过的反向边条数。
    edges_touched: usize,
}

/// 种子去重后的个数。
fn dedup_count(seeds: &[&str]) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    for s in seeds.iter() {
        if !seen.iter().any(|p| p == s) {
            seen.push(s);
        }
    }
    seen.len()
}

/// 级联引擎自检入口（判据逐条映射见 `ver01c_checks.rs`）。
pub fn run_ver01c_checks() -> CheckSet {
    super::ver01c_checks::run_ver01c_checks()
}
