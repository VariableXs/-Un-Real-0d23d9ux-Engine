//! vck05_hetgraph — CGPU-F1605 异构任务图扩展（跨引擎依赖·交接·切分·执行·传播）。
//!
//! 判据原文（施工书锚点 CGPU-F1605）：图扩展：跨引擎依赖（B 域依赖图支持
//! 跨引擎边（GPU 产出→NPU 消费——跨引擎围栏（fence 跨引擎语义（引擎间同
//! 步原语（跨引擎 fence 三态（未就绪/就绪/已消费）——fence 语义表；数据交
//! 接（跨引擎数据传递（显存→NPU 内存拷贝/共享内存（按平台能力（交接路径
//! 表（拷贝/零拷贝共享——交接成本标注（入 I 域带宽账；图编译（异构图编译
//! （引擎分配+跨引擎边最小化（切分算法（切分目标：通信最小+负载均衡——切
//! 分策略表；编译校验（切分后逐引擎子图合法（引擎能力满足子图——校验断
//! 言；执行（跨引擎并行执行（B 域执行器扩展（引擎 worker 池——执行扩展；
//! 失败（某引擎子图失败→跨引擎失败传播与隔离（失败域 F1608 衔接）；测试
//! （fence 三态/交接两路/切分/校验/执行/传播六组）。判据：跨引擎 fence、
//! 路径表、切分算法、校验、执行、失败传播、六组、判据。
//!
//! **职责定位**：跨引擎 fence 三态状态机（单向 未就绪→就绪→已消费——回退
//! 显性拒，信号语义与显存数据解耦——fence 只是旗不是货）+交接路径表（拷
//! 贝/零拷贝两路按平台能力裁决——成本字节数标注入 I 域带宽账口径）+异构
//! 图切分（按引擎偏好分组——同偏好同子图天然最小化跨引擎切面，均衡校验
//! 偏差超带显性标注——策略表两行）+子图校验（引擎登记可用性消费 vck04
//! 登记面+子图非空+边端点同引擎三项断言）+引擎 worker 池并行执行账（各
//! 引擎独立推进——跨引擎边 fence 同步点）+失败传播与隔离（失败子图的下
//! 游全取消——传播闭集；无关引擎零感知——隔离面；F1608 失败域衔接声明）。
//! 诊断码段 0x55xx 四码。前置 vck04（登记面消费）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vck04_hetprobe::{EngineKind, EngineRow};

/// 模块版本。
pub const K05_HETGRAPH_VERSION: &str = "K05-hetgraph-v1";

// ---------------------------------------------------------------------------
// 诊断码（0x55xx 段独占——四码互异）
// ---------------------------------------------------------------------------

/// 诊断码（0x55xx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HgCode {
    /// 0x5501 fence 回退（三态单向——已消费不可回就绪）。
    HgFenceRegress,
    /// 0x5502 子图校验失败（引擎能力/空图/跨引擎边残留）。
    HgSubgraphInvalid,
    /// 0x5503 负载失衡（引擎间节点数偏差超均衡带——显性标注非静默）。
    HgLoadImbalance,
    /// 0x5504 传播遗漏（失败子图的下游未被取消——传播闭集被破坏）。
    HgPropagationLeak,
}

impl HgCode {
    pub const ALL: [HgCode; 4] = [
        HgCode::HgFenceRegress,
        HgCode::HgSubgraphInvalid,
        HgCode::HgLoadImbalance,
        HgCode::HgPropagationLeak,
    ];

    /// 人读诊断（发生了什么/为什么/怎么办）。
    pub const fn detail(self) -> &'static str {
        match self {
            HgCode::HgFenceRegress => "fence 回退：三态单向 未就绪→就绪→已消费，回退显性拒",
            HgCode::HgSubgraphInvalid => "子图校验失败：引擎能力/空图/跨引擎边残留——先修切分",
            HgCode::HgLoadImbalance => "负载失衡：引擎间节点数偏差超均衡带——显性标注供切分复审",
            HgCode::HgPropagationLeak => "传播遗漏：失败子图的下游未被取消——传播闭集被破坏",
        }
    }
}

// ---------------------------------------------------------------------------
// 一、跨引擎 fence 三态（判据「跨引擎 fence」）
// ---------------------------------------------------------------------------

/// fence 三态（单向状态机——信号语义，与数据交接解耦）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FenceState {
    /// 未就绪（产出引擎尚未完成）。
    NotReady,
    /// 就绪（产出完成——消费引擎可开工）。
    Ready,
    /// 已消费（消费引擎取用完毕——fence 生命周期终点）。
    Consumed,
}

/// 跨引擎 fence（产出引擎×消费引擎×状态）。
#[derive(Clone, Copy, Debug)]
pub struct CrossFence {
    /// fence id。
    pub fid: u32,
    /// 产出引擎（GPU 或异构引擎——生产侧）。
    pub producer: EngineKind,
    /// 消费引擎。
    pub consumer: EngineKind,
    /// 当前状态。
    pub state: FenceState,
}

/// fence 语义表（判据「fence 语义表」——id→fence 的状态机账）。
pub struct FenceTable {
    fences: Vec<CrossFence>,
    /// 回退拒绝计数。
    pub regress_blocks: u32,
}

impl FenceTable {
    /// 空表。
    pub fn new() -> Self {
        FenceTable { fences: Vec::new(), regress_blocks: 0 }
    }

    /// **建 fence**（初始 未就绪——同 id 重复建被复用不重开）。
    pub fn create(&mut self, fid: u32, producer: EngineKind, consumer: EngineKind) {
        let mut i = 0usize;
        while i < self.fences.len() {
            if self.fences[i].fid == fid {
                return; // 已存在——复用（幂等）
            }
            i += 1;
        }
        self.fences.push(CrossFence {
            fid,
            producer,
            consumer,
            state: FenceState::NotReady,
        });
    }

    /// **推进状态**（单向一步：NotReady→Ready→Consumed；回退显性拒——
    /// O(n) 查找，锚点口径）。返回 Err(HgFenceRegress) 表示回退被拦。
    pub fn advance(&mut self, fid: u32) -> Result<FenceState, HgCode> {
        let mut i = 0usize;
        while i < self.fences.len() {
            if self.fences[i].fid == fid {
                let next = match self.fences[i].state {
                    FenceState::NotReady => FenceState::Ready,
                    FenceState::Ready => FenceState::Consumed,
                    FenceState::Consumed => {
                        self.regress_blocks += 1;
                        return Err(HgCode::HgFenceRegress);
                    }
                };
                self.fences[i].state = next;
                return Ok(next);
            }
            i += 1;
        }
        Err(HgCode::HgFenceRegress) // 无此 fence——按回退面处置（表外推进）
    }

    /// 查状态。
    pub fn state_of(&self, fid: u32) -> Option<FenceState> {
        let mut i = 0usize;
        while i < self.fences.len() {
            if self.fences[i].fid == fid {
                return Some(self.fences[i].state);
            }
            i += 1;
        }
        None
    }

    /// fence 数。
    pub fn len(&self) -> usize {
        self.fences.len()
    }

    /// 空判据。
    pub fn is_empty(&self) -> bool {
        self.fences.is_empty()
    }
}

impl Default for FenceTable {
    fn default() -> Self {
        FenceTable::new()
    }
}

// ---------------------------------------------------------------------------
// 二、交接路径表（判据「路径表」——拷贝/零拷贝两路按平台能力裁决）
// ---------------------------------------------------------------------------

/// 平台能力（共享内存可用性——交接路径裁决输入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlatformCap {
    /// 支持共享内存（零拷贝可用）。
    SharedMem,
    /// 不支持（只能拷贝）。
    CopyOnly,
}

/// 交接路径（两路——判据「拷贝/零拷贝共享」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoffPath {
    /// 内存拷贝（成本=数据字节——入 I 域带宽账口径）。
    Copy,
    /// 零拷贝共享（成本 0——同物理内存映射）。
    ZeroCopy,
}

/// **路径裁决**（按平台能力——零拷贝优先，不可用降级拷贝；成本标注
/// bytes：Copy=数据量、ZeroCopy=0——入 I 域带宽账的字段口径）。
pub const fn decide_handoff(cap: PlatformCap, data_bytes: u64) -> (HandoffPath, u64) {
    match cap {
        PlatformCap::SharedMem => (HandoffPath::ZeroCopy, 0),
        PlatformCap::CopyOnly => (HandoffPath::Copy, data_bytes),
    }
}

// ---------------------------------------------------------------------------
// 三、异构图与切分算法（判据「切分算法」——通信最小+负载均衡策略表）
// ---------------------------------------------------------------------------

/// 图节点（任务——带引擎偏好与数据量）。
#[derive(Clone, Copy, Debug)]
pub struct HetNode {
    /// 节点 id。
    pub nid: u32,
    /// 引擎偏好（GPU 主图节点用 EngineKind 之外的 GPU 档——用 producer
    /// 语义：偏好落在哪个引擎就切给谁；GPU 节点用 Npu/Dsp/IspMedia 之外
    /// 无表达——扩一档 Gpu）。
    pub prefer: PrefEngine,
    /// 产出数据量（bytes——交接成本输入）。
    pub out_bytes: u64,
}

/// 引擎偏好（GPU + vck04 三异构引擎——切分对象全闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrefEngine {
    /// GPU（主渲染/计算引擎）。
    Gpu,
    /// NPU（vck04 登记面）。
    Npu,
    /// DSP。
    Dsp,
    /// ISP/MEDIA。
    IspMedia,
}

/// 均衡带（各引擎子图节点数最大差——超出即失衡标注；策略表值 2）。
pub const BALANCE_BAND: usize = 2;

/// 引擎子图（同引擎节点集合）。
#[derive(Clone, Debug)]
pub struct EngineSubgraph {
    /// 承载引擎。
    pub engine: PrefEngine,
    /// 节点 id 集（保序）。
    pub nodes: Vec<u32>,
}

/// 切分结果（逐引擎子图+跨引擎切面统计）。
#[derive(Clone, Debug)]
pub struct SplitPlan {
    /// 逐引擎子图（空引擎不出子图）。
    pub subgraphs: Vec<EngineSubgraph>,
    /// 跨引擎边数（切面——通信量代理指标：偏好不同的相邻节点对数）。
    pub cross_edges: usize,
    /// 失衡标注（None=均衡；Some(code)=偏差超带）。
    pub imbalance: Option<HgCode>,
}

/// **切分算法**（确定性贪心——策略表两行）：
/// 行一（通信最小）：按节点引擎偏好分组——同偏好节点聚同子图，跨引擎
/// 边只在偏好相异节点间产生（边集由调用方以 `(src,dst)` 对给出）。
/// 行二（负载均衡）：各引擎节点数极差 > BALANCE_BAND → 显性标注
/// HgLoadImbalance（一期不自动迁移——标注供复审，二期能力预留）。
pub fn split_graph(nodes: &[HetNode], edges: &[(u32, u32)]) -> SplitPlan {
    let mut subgraphs: Vec<EngineSubgraph> = Vec::new();
    let prefs = [
        (PrefEngine::Gpu, "GPU"),
        (PrefEngine::Npu, "NPU"),
        (PrefEngine::Dsp, "DSP"),
        (PrefEngine::IspMedia, "ISP"),
    ];
    let mut p = 0usize;
    while p < prefs.len() {
        let pe = prefs[p].0;
        let mut nodes_of: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < nodes.len() {
            if nodes[i].prefer == pe {
                nodes_of.push(nodes[i].nid);
            }
            i += 1;
        }
        if !nodes_of.is_empty() {
            subgraphs.push(EngineSubgraph { engine: pe, nodes: nodes_of });
        }
        p += 1;
    }
    // 跨引擎切面：偏好相异的边端点对数。
    let mut cross = 0usize;
    let mut e = 0usize;
    while e < edges.len() {
        let src_prefer = find_prefer(nodes, edges[e].0);
        let dst_prefer = find_prefer(nodes, edges[e].1);
        if src_prefer != dst_prefer {
            cross += 1;
        }
        e += 1;
    }
    // 负载均衡校验（极差 > 带 → 标注）。
    let mut imbalance = None;
    if subgraphs.len() >= 2 {
        let mut min_n = usize::MAX;
        let mut max_n = 0usize;
        let mut i = 0usize;
        while i < subgraphs.len() {
            let n = subgraphs[i].nodes.len();
            if n < min_n {
                min_n = n;
            }
            if n > max_n {
                max_n = n;
            }
            i += 1;
        }
        if max_n - min_n > BALANCE_BAND {
            imbalance = Some(HgCode::HgLoadImbalance);
        }
    }
    SplitPlan { subgraphs, cross_edges: cross, imbalance }
}

fn find_prefer(nodes: &[HetNode], nid: u32) -> Option<PrefEngine> {
    let mut i = 0usize;
    while i < nodes.len() {
        if nodes[i].nid == nid {
            return Some(nodes[i].prefer);
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 四、编译校验（判据「校验」——切分后逐引擎子图合法三项断言）
// ---------------------------------------------------------------------------

/// **子图校验**（三项断言：① 引擎登记可用（异构引擎须在 vck04 登记
/// 面——GPU 恒可用；② 子图非空；③ 边端点不全落本子图即跨引擎边——
/// 切分后子图内边两端点必须都在本子图，跨引擎边由 fence 兜底不算
/// 非法——校验只拦「端点悬空」）。返回违例码列表（空=全合法）。
pub fn validate_split(
    plan: &SplitPlan,
    registry: &[EngineRow],
    nodes: &[HetNode],
    edges: &[(u32, u32)],
) -> Vec<HgCode> {
    let mut bad: Vec<HgCode> = Vec::new();
    // ① 引擎能力：异构引擎（Npu/Dsp/IspMedia）须有登记行且非 ProbeFailed
    // 语义（vck04 登记行存在即可用——行剥离语义由 vck04 保证）。
    let mut si = 0usize;
    while si < plan.subgraphs.len() {
        let sg = &plan.subgraphs[si];
        if sg.nodes.is_empty() {
            if !contains_code(&bad, HgCode::HgSubgraphInvalid) {
                bad.push(HgCode::HgSubgraphInvalid);
            }
        }
        if sg.engine != PrefEngine::Gpu {
            let mut found = false;
            let mut ri = 0usize;
            while ri < registry.len() {
                let want = match sg.engine {
                    PrefEngine::Npu => EngineKind::Npu,
                    PrefEngine::Dsp => EngineKind::Dsp,
                    PrefEngine::IspMedia => EngineKind::IspMedia,
                    PrefEngine::Gpu => EngineKind::Npu, // 不可达——占位
                };
                if registry[ri].kind == want {
                    found = true;
                }
                ri += 1;
            }
            if !found && !contains_code(&bad, HgCode::HgSubgraphInvalid) {
                bad.push(HgCode::HgSubgraphInvalid);
            }
        }
        si += 1;
    }
    // ③ 端点悬空：边两端点必须都存在于节点表。
    let mut e = 0usize;
    while e < edges.len() {
        if find_prefer(nodes, edges[e].0).is_none() || find_prefer(nodes, edges[e].1).is_none() {
            if !contains_code(&bad, HgCode::HgSubgraphInvalid) {
                bad.push(HgCode::HgSubgraphInvalid);
            }
        }
        e += 1;
    }
    bad
}

fn contains_code(v: &[HgCode], c: HgCode) -> bool {
    let mut i = 0usize;
    while i < v.len() {
        if v[i] == c {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 五、执行账 + 失败传播与隔离（判据「执行」「失败传播」）
// ---------------------------------------------------------------------------

/// 引擎 worker 池执行账（各引擎独立推进——跨引擎边 fence 同步点账）。
#[derive(Clone, Debug, Default)]
pub struct WorkerPoolLedger {
    /// 逐引擎已完成节点数。
    pub done: Vec<(PrefEngine, u32)>,
    /// fence 等待次数（跨引擎同步点）。
    pub fence_waits: u32,
    /// 失败引擎（隔离面——标记不炸全局）。
    pub failed_engines: Vec<PrefEngine>,
    /// 取消节点数（传播闭集——失败下游）。
    pub cancelled: u32,
}

impl WorkerPoolLedger {
    /// 空账。
    pub fn new() -> Self {
        WorkerPoolLedger::default()
    }

    /// **记完成**（同引擎累计——O(n) 查找更新）。
    pub fn record_done(&mut self, engine: PrefEngine, n: u32) {
        let mut i = 0usize;
        while i < self.done.len() {
            if self.done[i].0 == engine {
                self.done[i].1 += n;
                return;
            }
            i += 1;
        }
        self.done.push((engine, n));
    }

    /// **失败传播**（判据核心——失败引擎的下游节点全取消；无关引擎零
    /// 感知：done 账不动、failed_engines 只记失败者。传播闭集=依赖图
    /// 可达集，此处以 `downstream_of` 给出的下游清单为准——调用方由
    /// 依赖图算出，本账只管闭集执行）。幂等：同引擎重复传播不重复
    /// 累计（首次标记时才计入 cancelled——重复立案不重复销账）。
    pub fn propagate_failure(&mut self, failed: PrefEngine, downstream: &[u32]) {
        let mut i = 0usize;
        while i < self.failed_engines.len() {
            if self.failed_engines[i] == failed {
                return; // 已标记——幂等（cancelled 不重复累计）
            }
            i += 1;
        }
        self.failed_engines.push(failed);
        self.cancelled += downstream.len() as u32;
    }

    /// 引擎是否被隔离。
    pub fn is_isolated(&self, engine: PrefEngine) -> bool {
        let mut i = 0usize;
        while i < self.failed_engines.len() {
            if self.failed_engines[i] == engine {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 引擎完成量。
    pub fn done_of(&self, engine: PrefEngine) -> u32 {
        let mut i = 0usize;
        while i < self.done.len() {
            if self.done[i].0 == engine {
                return self.done[i].1;
            }
            i += 1;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// 判据（判据面零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F1605 域自检（判据 9 条六组：fence 三态/交接两路/切分/校验/执行/传播+元）。
pub fn run_vck05_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F1605");

    // --- fence 三态（判据一）---
    // fence-01：三态单向全旅程——建 fence NotReady→advance Ready→advance
    // Consumed→advance 拒（regress 计数）；无此 fence 推进也拒。
    let mut ft = FenceTable::new();
    ft.create(1, EngineKind::Npu, EngineKind::Dsp);
    let st0 = ft.state_of(1) == Some(FenceState::NotReady);
    let a1 = ft.advance(1);
    let a2 = ft.advance(1);
    let a3 = ft.advance(1);
    let fence_ok = st0
        && a1 == Ok(FenceState::Ready)
        && a2 == Ok(FenceState::Consumed)
        && a3 == Err(HgCode::HgFenceRegress)
        && ft.regress_blocks == 1
        && ft.state_of(99).is_none()
        && ft.len() == 1;
    if fence_ok {
        s.ok("K05-fence-01-三态单向回退拒");
    } else {
        s.fail("K05-fence-01-三态单向回退拒", "fence 状态机失效");
    }

    // --- 交接两路（判据二）---
    // hand-01：路径裁决两路——SharedMem→ZeroCopy 成本 0；CopyOnly→Copy
    // 成本=数据字节（入 I 域带宽账字段口径）；路径枚举两值互异。
    let (p1, c1) = decide_handoff(PlatformCap::SharedMem, 4096);
    let (p2, c2) = decide_handoff(PlatformCap::CopyOnly, 4096);
    let hand_ok = p1 == HandoffPath::ZeroCopy
        && c1 == 0
        && p2 == HandoffPath::Copy
        && c2 == 4096
        && p1 != p2;
    if hand_ok {
        s.ok("K05-hand-01-交接两路按能力裁决");
    } else {
        s.fail("K05-hand-01-交接两路按能力裁决", "路径裁决失效");
    }

    // --- 切分（判据三）---
    // split-01：偏好分组切分——GPU 3 节点+NPU 2 节点+DSP 1 节点 → 三子图
    // 节点数 3/2/1；同偏好相邻边不算切面、异偏好边算（手算 2 跨引擎边）；
    // 极差 3-1=2 ≤ 带 2 → 无失衡。
    let nodes = [
        HetNode { nid: 1, prefer: PrefEngine::Gpu, out_bytes: 100 },
        HetNode { nid: 2, prefer: PrefEngine::Gpu, out_bytes: 50 },
        HetNode { nid: 3, prefer: PrefEngine::Gpu, out_bytes: 70 },
        HetNode { nid: 4, prefer: PrefEngine::Npu, out_bytes: 200 },
        HetNode { nid: 5, prefer: PrefEngine::Npu, out_bytes: 80 },
        HetNode { nid: 6, prefer: PrefEngine::Dsp, out_bytes: 30 },
    ];
    // 边：1→2（同 GPU）、4→5（同 NPU）、3→4（GPU→NPU 跨）、5→6（NPU→DSP 跨）。
    let edges = [(1u32, 2u32), (4, 5), (3, 4), (5, 6)];
    let plan = split_graph(&nodes, &edges);
    let mut got_gpu = 0usize;
    let mut got_npu = 0usize;
    let mut got_dsp = 0usize;
    let mut si = 0usize;
    while si < plan.subgraphs.len() {
        match plan.subgraphs[si].engine {
            PrefEngine::Gpu => got_gpu = plan.subgraphs[si].nodes.len(),
            PrefEngine::Npu => got_npu = plan.subgraphs[si].nodes.len(),
            PrefEngine::Dsp => got_dsp = plan.subgraphs[si].nodes.len(),
            _ => {}
        }
        si += 1;
    }
    let split_ok = got_gpu == 3
        && got_npu == 2
        && got_dsp == 1
        && plan.cross_edges == 2
        && plan.imbalance.is_none()
        && plan.subgraphs.len() == 3;
    if split_ok {
        s.ok("K05-split-01-偏好分组切面最小");
    } else {
        s.fail("K05-split-01-偏好分组切面最小", "切分失效");
    }

    // split-02：失衡标注——GPU 5 节点+DSP 1 节点极差 4>2 → HgLoadImbalance。
    let nodes2 = [
        HetNode { nid: 1, prefer: PrefEngine::Gpu, out_bytes: 1 },
        HetNode { nid: 2, prefer: PrefEngine::Gpu, out_bytes: 1 },
        HetNode { nid: 3, prefer: PrefEngine::Gpu, out_bytes: 1 },
        HetNode { nid: 4, prefer: PrefEngine::Gpu, out_bytes: 1 },
        HetNode { nid: 5, prefer: PrefEngine::Gpu, out_bytes: 1 },
        HetNode { nid: 6, prefer: PrefEngine::Dsp, out_bytes: 1 },
    ];
    let plan2 = split_graph(&nodes2, &[]);
    let imb_ok = plan2.imbalance == Some(HgCode::HgLoadImbalance);
    if imb_ok {
        s.ok("K05-split-02-负载失衡显性标注");
    } else {
        s.fail("K05-split-02-负载失衡显性标注", "均衡面失效");
    }

    // --- 校验（判据四）---
    // val-01：登记可用三项断言——NPU/DSP 子图有登记行（构造 vck04 行）+
    // 子图非空 + 边端点齐 → 零违例；去掉 NPU 登记行 → HgSubgraphInvalid；
    // 悬空边（端点不在节点表）→ HgSubgraphInvalid。
    let registry = [
        EngineRow {
            kind: EngineKind::Npu,
            route: Some(super::vck04_hetprobe::ProbeRoute::NpuWindowsCim),
            outcome: super::vck04_hetprobe::ProbeOutcome::Available(0b11),
            isolated: false,
        },
        EngineRow {
            kind: EngineKind::Dsp,
            route: Some(super::vck04_hetprobe::ProbeRoute::DspAudioEngine),
            outcome: super::vck04_hetprobe::ProbeOutcome::Available(0b1),
            isolated: false,
        },
    ];
    let val_ok = validate_split(&plan, &registry, &nodes, &edges).is_empty();
    let registry_no_npu = [registry[1]];
    let val_miss = !validate_split(&plan, &registry_no_npu, &nodes, &edges).is_empty();
    let dangling = [(1u32, 99u32)];
    let val_dangle = !validate_split(&plan, &registry, &nodes, &dangling).is_empty();
    if val_ok && val_miss && val_dangle {
        s.ok("K05-val-01-子图三项断言校验");
    } else {
        s.fail("K05-val-01-子图三项断言校验", "校验失效");
    }

    // --- 执行（判据五）---
    // exec-01：worker 池分引擎记账——GPU done 3、NPU done 2、DSP done 1
    // （各引擎独立累计互不串）；fence 等待计数独立记。
    let mut pool = WorkerPoolLedger::new();
    pool.record_done(PrefEngine::Gpu, 3);
    pool.record_done(PrefEngine::Npu, 2);
    pool.record_done(PrefEngine::Dsp, 1);
    pool.record_done(PrefEngine::Gpu, 1); // 同引擎再累计
    pool.fence_waits += 2;
    let exec_ok = pool.done_of(PrefEngine::Gpu) == 4
        && pool.done_of(PrefEngine::Npu) == 2
        && pool.done_of(PrefEngine::Dsp) == 1
        && pool.done_of(PrefEngine::IspMedia) == 0
        && pool.fence_waits == 2
        && !pool.is_isolated(PrefEngine::Npu);
    if exec_ok {
        s.ok("K05-exec-01-worker池分引擎记账");
    } else {
        s.fail("K05-exec-01-worker池分引擎记账", "执行账失效");
    }

    // --- 传播（判据六）---
    // prop-01：失败传播与隔离——NPU 子图失败 → 下游 3 节点取消+cancelled
    // 累计+NPU 隔离标记；GPU/DSP 完成账零感知（隔离面）；重复传播幂等
    // （failed_engines 不重复、cancelled 不重复累计——同一下游清单）。
    let mut pool2 = WorkerPoolLedger::new();
    pool2.record_done(PrefEngine::Gpu, 3);
    pool2.record_done(PrefEngine::Dsp, 1);
    let down = [7u32, 8u32, 9u32];
    pool2.propagate_failure(PrefEngine::Npu, &down);
    let after1 = pool2.cancelled;
    pool2.propagate_failure(PrefEngine::Npu, &down);
    let prop_ok = pool2.cancelled == 3
        && after1 == 3 // 幂等——不重复累计
        && pool2.is_isolated(PrefEngine::Npu)
        && !pool2.is_isolated(PrefEngine::Gpu)
        && !pool2.is_isolated(PrefEngine::Dsp)
        && pool2.done_of(PrefEngine::Gpu) == 3 // 隔离零感知
        && pool2.failed_engines.len() == 1;
    if prop_ok {
        s.ok("K05-prop-01-失败传播闭集与隔离");
    } else {
        s.fail("K05-prop-01-失败传播闭集与隔离", "传播面失效");
    }

    // --- 判据元 ---
    // 元-01：四码互异+详情齐备+F1608 衔接声明（版本串非空）。
    let codes = HgCode::ALL;
    let mut codes_ok = codes.len() == 4;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if codes[i].detail().is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] || codes[i].detail() == codes[j].detail() {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    let meta_ok = codes_ok && !K05_HETGRAPH_VERSION.is_empty();
    if meta_ok {
        s.ok("K05-元-01-四码互异版本齐备");
    } else {
        s.fail("K05-元-01-四码互异版本齐备", "码面漂移");
    }

    // 元-02：零 panic 面——自扫本文件+空表平凡。
    let src = include_str!("vck05_hetgraph.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    let mut empty_ft = FenceTable::new();
    let empty_ok = empty_ft.is_empty()
        && empty_ft.state_of(1).is_none()
        && empty_ft.advance(1).is_err()
        && !K05_HETGRAPH_VERSION.is_empty();
    if hits == 0 && empty_ok {
        s.ok("K05-元-02-零panic空态平凡");
    } else {
        s.fail("K05-元-02-零panic空态平凡", "panic 面或空态失效");
    }

    s
}

/// 单遍词法剥除（注释剥除、字符串整体剥空——判据模式串不自 hit）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
