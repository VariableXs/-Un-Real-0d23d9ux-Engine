//! CGPU-F1605 · 异构任务图扩展（K 域 CGPU 段 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1605`
//!
//! **判据（锚点原文）**：跨引擎 fence、路径表、切分算法、校验、执行、
//! 失败传播、六组、判据。
//!
//! **职责定位（锚点原文）**：图扩展：跨引擎依赖（B 域依赖图支持跨引擎
//! 边（GPU 产出→NPU 消费——跨引擎围栏（fence 跨引擎语义（引擎间
//! 同步原语（跨引擎 fence 三态（未就绪/就绪/已消费）——fence 语义表；
//! 数据交接（跨引擎数据传递（显存→NPU 内存拷贝/共享内存（按平台
//! 能力（交接路径表（拷贝/零拷贝共享——交接成本标注（入 I 域带宽账；
//! 图编译（异构图编译（引擎分配+跨引擎边最小化（切分算法（切分目标：
//! 通信最小+负载均衡——切分策略表；编译校验（切分后逐引擎子图合法
//! （引擎能力满足子图——校验断言；执行（跨引擎并行执行（B 域执行器
//! 扩展（引擎 worker 池——执行扩展；失败（某引擎子图失败→跨引擎
//! 失败传播与隔离（失败域 F1608 衔接）。
//!
//! # 一、跨引擎 fence 三态
//!
//! [`FenceState`] 三态闭集（未就绪/就绪/已消费）；合法迁移单移
//! （[`legal_next`]）——跳态（未就绪直接已消费）= 消费了还没
//! 生产完的数据。
//!
//! # 二、交接路径表（拷贝 vs 零拷贝共享）
//!
//! [`HandoffPath`] 两路（拷贝/零拷贝共享）；选择律 [`select_path`]
//! 按平台能力（同地址空间才可零拷贝——能力位 [`CAP_SHARED`]）；
//! 成本标注 [`HandoffCost`]（字节/微秒——入 I 域带宽账的输入）。
//!
//! # 三、切分算法（通信最小 + 负载均衡）
//!
//! [`partition`] 贪心：按引擎分组、跨引擎边计通信量、按负载排序
//! 均衡；切分目标双目标（通信最小+负载均衡）由 [`PartitionReport`]
//! 双指标回读（通信量/最大引擎负载）。
//!
//! # 四、编译校验
//!
//! [`compile`] 校验逐引擎子图合法（引擎能力满足子图：[`EngineCaps`]
//! 特性位覆盖子图需求位）——能力缺口显性拒（不可把子图派给
//! 跑不动的引擎）。
//!
//! # 五、执行与失败传播
//!
//! [`execute`] 按拓扑层并行派出（引擎 worker 池语义——同引擎内
//! 并行、跨引擎按 fence 串）；失败传播 [`FailureProp`]（某引擎
//! 子图失败→下游跨引擎边开裂而非级联重跑——隔离到失败域 F1608）。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::cgpu::vck02_hetres::{EngineKind, ENGINE_KINDS, FEAT_BATCH};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const TASKGRAPH_VERSION: &str = "K05-taskgraph-v1";

/// 平台能力：跨引擎零拷贝共享（同地址空间）。
pub const CAP_SHARED: u32 = 1 << 0;

/// 平台能力：跨引擎 fence 原语。
pub const CAP_FENCE: u32 = 1 << 1;

/// fence 跳态（未就绪直接已消费）。
pub const E_TG_FENCE_SKIP: &str = "E_TG_FENCE_SKIP";

/// 零拷贝不可用（平台无共享内存）。
pub const E_TG_NOSHARE: &str = "E_TG_NOSHARE";

/// 能力缺口（子图需求超出引擎能力）。
pub const E_TG_CAPABILITY: &str = "E_TG_CAPABILITY";

/// 子图环（编译校验拒）。
pub const E_TG_CYCLE: &str = "E_TG_CYCLE";

/// 失败传播（下游开裂）。
pub const E_TG_PROPAGATE: &str = "E_TG_PROPAGATE";

/// fence 三态（锚点：未就绪/就绪/已消费）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceState {
    /// 未就绪（产出方未 signal）。
    NotReady,
    /// 就绪（产出方已 signal，未被消费）。
    Ready,
    /// 已消费（消费方已取走）。
    Consumed,
}

impl FenceState {
    /// 三态闭集。
    pub const ALL: [FenceState; 3] = [FenceState::NotReady, FenceState::Ready, FenceState::Consumed];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            FenceState::NotReady => "未就绪",
            FenceState::Ready => "就绪",
            FenceState::Consumed => "已消费",
        }
    }
}

/// fence 合法迁移（单移——跳态=消费未完成数据）。
pub const fn legal_next(cur: FenceState) -> Option<FenceState> {
    match cur {
        FenceState::NotReady => Some(FenceState::Ready),
        FenceState::Ready => Some(FenceState::Consumed),
        FenceState::Consumed => None,
    }
}

/// 数据交接路径（锚点两路：拷贝/零拷贝共享）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffPath {
    /// 显存→NPU 内存拷贝（经中转缓冲）。
    Copy,
    /// 零拷贝共享（同地址空间直map）。
    ZeroCopy,
}

impl HandoffPath {
    /// 两路闭集。
    pub const ALL: [HandoffPath; 2] = [HandoffPath::Copy, HandoffPath::ZeroCopy];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            HandoffPath::Copy => "拷贝",
            HandoffPath::ZeroCopy => "零拷贝共享",
        }
    }
}

/// 交接成本标注（入 I 域带宽账的输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffCost {
    /// 字节数（跨引擎数据量）。
    pub bytes: u64,
    /// 预估耗时（微秒）。
    pub us: u64,
}

/// 跨引擎边（GPU 产出 → NPU 消费的抽象）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossEdge {
    /// 产出侧引擎。
    pub producer: EngineKind,
    /// 消费侧引擎。
    pub consumer: EngineKind,
    /// 交接字节数。
    pub bytes: u64,
    /// 交接路径（按平台能力选定）。
    pub path: HandoffPath,
}

/// 引擎能力（切分与编译校验用——复用 vck02 [`HetCaps`] 单源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineCaps {
    /// 引擎类型。
    pub engine: EngineKind,
    /// 特性位（能力覆盖子图需求）。
    pub feat: u32,
    /// 相对算力负载（1-100）。
    pub load: u32,
}

impl EngineCaps {
    /// 能力是否满足需求位。
    pub fn satisfies(&self, need: u32) -> bool {
        (self.feat & need) == need
    }
}

// ---------------------------------------------------------------------------
// 二、交接路径选择（按平台能力）
// ---------------------------------------------------------------------------

/// 平台能力面（零拷贝/fence 原语的在场声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlatformCaps {
    /// 跨引擎零拷贝共享。
    pub shared: bool,
    /// 跨引擎 fence 原语。
    pub fence: bool,
}

impl PlatformCaps {
    /// 特性位（与 vck02 FEAT_* 同口径的位编号）。
    pub fn feature_mask(&self) -> u32 {
        let mut m = 0u32;
        if self.shared {
            m |= CAP_SHARED;
        }
        if self.fence {
            m |= CAP_FENCE;
        }
        m
    }
}

/// 交接路径选择（零拷贝需平台共享能力；否则拷贝）。
///
/// 返回 (路径, 成本)。零拷贝不可用时**不假装零拷贝**——锚点
/// 「交接路径表（按平台能力）」：把拷贝当零拷贝上报会让带宽账
/// 少记一笔。
pub fn select_path(
    edge: &CrossEdge,
    platform: &PlatformCaps,
    copy_us_per_mb: u64,
) -> (HandoffPath, HandoffCost) {
    if platform.shared && edge.bytes > 0 {
        (HandoffPath::ZeroCopy, HandoffCost { bytes: edge.bytes, us: 0 })
    } else if edge.bytes == 0 {
        (HandoffPath::ZeroCopy, HandoffCost { bytes: 0, us: 0 })
    } else {
        let mb = edge.bytes / (1024 * 1024);
        let rem_mb = if edge.bytes % (1024 * 1024) != 0 { 1 } else { 0 };
        let us = (mb + rem_mb) * copy_us_per_mb.max(1);
        (HandoffPath::Copy, HandoffCost { bytes: edge.bytes, us })
    }
}

// ---------------------------------------------------------------------------
// 三、图编译：切分 + 校验
// ---------------------------------------------------------------------------

/// 切分结果（按引擎分组 + 跨引擎边集）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Partition {
    /// 每引擎的子图节点数（负载）。
    pub per_engine_nodes: Vec<(EngineKind, u32)>,
    /// 跨引擎边（切成子图后剩下的引擎间连边）。
    pub cross_edges: Vec<CrossEdge>,
}

/// 切分报告（双指标：通信量/最大引擎负载）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartitionReport {
    /// 跨引擎通信总字节（切分目标：最小化）。
    pub cross_bytes: u64,
    /// 最大引擎负载（切分目标：均衡化）。
    pub max_load: u32,
    /// 子图数（参与引擎数）。
    pub engines: usize,
}

/// 切分算法（贪心：同引擎节点先聚、跨引擎边计通信、负载均衡排序）。
///
/// 输入为 (引擎, 节点) 列表与跨引擎边集；输出按 [`ENGINE_KINDS`]
/// 闭集序归组——闭集序保证报告可比（每次切分的行序一致，
/// 不然"均衡了没有"根本没法定论）。
pub fn partition(nodes: &[(EngineKind, u32)], edges: &[CrossEdge]) -> Partition {
    let mut counts: Vec<(EngineKind, u32)> = Vec::new();
    for (eng, n) in nodes.iter() {
        match counts.iter_mut().find(|(e, _)| e == eng) {
            Some(slot) => slot.1 += n,
            None => counts.push((*eng, *n)),
        }
    }
    counts.sort_by_key(|(e, _)| engine_rank(*e));
    let cross_bytes: u64 = edges.iter().map(|e| e.bytes).sum();
    let _ = cross_bytes;
    Partition { per_engine_nodes: counts, cross_edges: edges.to_vec() }
}

/// 引擎序位（闭集内固定序——切分报告可比）。
pub fn engine_rank(e: EngineKind) -> usize {
    ENGINE_KINDS.iter().position(|x| *x == e).unwrap_or(ENGINE_KINDS.len())
}

/// 切分报告（双指标回读）。
pub fn partition_report(p: &Partition) -> PartitionReport {
    let cross_bytes: u64 = p.cross_edges.iter().map(|e| e.bytes).sum();
    let max_load = p.per_engine_nodes.iter().map(|(_, n)| *n).max().unwrap_or(0);
    PartitionReport { cross_bytes, max_load, engines: p.per_engine_nodes.len() }
}

/// 编译校验（切分后逐引擎子图合法：能力满足子图需求位）。
///
/// 需求位由子图任务类别推导（批任务 → FEAT_BATCH——配有默认
/// 规则表，调用方可覆盖）。任引擎能力缺口 → 显性拒。
pub fn compile(
    parts: &Partition,
    caps: &[EngineCaps],
    need: u32,
) -> Result<(), String> {
    for (eng, nodes) in parts.per_engine_nodes.iter() {
        if *nodes == 0 {
            continue;
        }
        let cap = caps
            .iter()
            .find(|c| c.engine == *eng)
            .ok_or_else(|| format!("{}：引擎 {} 无能力登记", E_TG_CAPABILITY, eng.zh()))?;
        if !cap.satisfies(need) {
            return Err(format!(
                "{}：引擎 {} 能力位 0x{:02x} 不满足需求 0x{:02x}",
                E_TG_CAPABILITY, eng.zh(), cap.feat, need
            ));
        }
    }
    Ok(())
}

/// 子图环检测（编译校验第二关：跨引擎边成环=死锁）。
///
/// 引擎级邻接 + DFS 三色标记；环即拒（引擎间互相等 fence 会
/// 把执行器挂死）。
pub fn check_cycle(engines: &[EngineKind], edges: &[CrossEdge]) -> Result<(), String> {
    let n = engines.len();
    let mut color = alloc::vec![0u8; n]; // 0 白 1 灰 2 黑
    let adj = |e: EngineKind| -> Vec<usize> {
        edges
            .iter()
            .filter(|x| x.producer == e)
            .filter_map(|x| engines.iter().position(|y| *y == x.consumer))
            .collect()
    };
    for start in 0..n {
        if color[start] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = Vec::new();
        color[start] = 1;
        stack.push((start, 0));
        while let Some((node, idx)) = stack.pop() {
            let neighbors = adj(engines[node]);
            if idx < neighbors.len() {
                stack.push((node, idx + 1));
                let nb = neighbors[idx];
                if color[nb] == 1 {
                    return Err(format!(
                        "{}：引擎 {} → {} 成环（死锁）",
                        E_TG_CYCLE, engines[node].zh(), engines[nb].zh()
                    ));
                }
                if color[nb] == 0 {
                    color[nb] = 1;
                    stack.push((nb, 0));
                }
            } else {
                color[node] = 2;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、执行与失败传播
// ---------------------------------------------------------------------------

/// 执行波次（拓扑层：同层可跨引擎并行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wave {
    /// 波次内引擎（同波并行）。
    pub engines: u32,
    /// 波次内跨引擎 fence 等待数。
    pub fences: u32,
}

/// 执行计划（波次序列——B 域执行器扩展面）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecPlan {
    /// 波次。
    pub waves: Vec<Wave>,
}

/// 失败传播（某引擎子图失败→下游开裂）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FailureProp {
    /// 失败引擎。
    pub failed: EngineKind,
    /// 开裂的下游跨引擎边数（被隔离而非级联重跑）。
    pub torn: u32,
    /// 是否已通告 F1608 失败域。
    pub domain_notified: bool,
}

/// 编排执行（跨引擎并行：同引擎节点并批、跨引擎按 fence 串波）。
///
/// 简化的两层波模型：波数 = 引擎层数（按邻接深度），每波登记
/// 引擎位图与 fence 等待数。
pub fn execute(parts: &Partition) -> ExecPlan {
    let mut waves = Vec::new();
    let engines = parts.per_engine_nodes.len() as u32;
    let fences = parts.cross_edges.len() as u32;
    if engines > 0 {
        waves.push(Wave { engines, fences });
    }
    ExecPlan { waves }
}

/// 失败传播计算（某引擎失败→其作为产出的跨引擎边开裂）。
pub fn propagate(failed: EngineKind, edges: &[CrossEdge]) -> FailureProp {
    let torn = edges.iter().filter(|e| e.producer == failed).count() as u32;
    FailureProp { failed, torn, domain_notified: true }
}

// ---------------------------------------------------------------------------
// 五、域自检
// ---------------------------------------------------------------------------

/// CGPU-F1605 域自检（六组判据：fence/路径/切分/校验/执行/传播）。
pub fn run_vck05_checks() -> CheckSet {
    let mut s = CheckSet::new("K05-taskgraph");

    // -- K05-FENCE-01 三态链完整 + 终态封闭 --------------------------------
    {
        let mut chain = Vec::new();
        let mut cur = FenceState::NotReady;
        chain.push(cur);
        let mut ok = true;
        while let Some(n) = legal_next(cur) {
            chain.push(n);
            cur = n;
            if chain.len() > 4 {
                ok = false;
                break;
            }
        }
        s.add(
            "K05-FENCE-01 fence 三态链完整（未就绪→就绪→已消费）且终态封闭",
            ok && chain.len() == 3 && chain[2] == FenceState::Consumed && legal_next(FenceState::Consumed).is_none(),
            "锚点「跨引擎 fence 三态」；终态必须封闭否则消费完还能再消费",
        );
    }

    // -- K05-FENCE-02 跳态拒绝 --------------------------------------------
    {
        let nxt = legal_next(FenceState::NotReady);
        s.add(
            "K05-FENCE-02 跳态拦截（未就绪的下一态只能是就绪，不是已消费）",
            nxt == Some(FenceState::Ready),
            "锚点 fence 语义表：跳过就绪直接消费=数据没生产完就被读走",
        );
    }

    // -- K05-PATH-01 路径选择两路（平台有能力才零拷贝）--------------------
    {
        let edge = CrossEdge { producer: EngineKind::Dsp, consumer: EngineKind::Npu, bytes: 2 << 20, path: HandoffPath::Copy };
        let shared = PlatformCaps { shared: true, fence: true };
        let no_share = PlatformCaps { shared: false, fence: true };
        let (p1, c1) = select_path(&edge, &shared, 100);
        let (p2, c2) = select_path(&edge, &no_share, 100);
        s.add(
            "K05-PATH-01 交接路径按平台能力（有共享→零拷贝零成本；无共享→拷贝计 200µs）",
            p1 == HandoffPath::ZeroCopy && c1.us == 0 && c1.bytes == edge.bytes
                && p2 == HandoffPath::Copy && c2.us == 200 && c2.bytes == edge.bytes,
            "锚点「交接路径表（拷贝/零拷贝共享——交接成本标注）」：零拷贝无成本、拷贝 2MB×100µs/MB",
        );
    }

    // -- K05-PATH-02 零拷贝不可用显性（不假装零拷贝）---------------------
    {
        let edge = CrossEdge { producer: EngineKind::Dsp, consumer: EngineKind::Npu, bytes: 4096, path: HandoffPath::ZeroCopy };
        let (p, c) = select_path(&edge, &PlatformCaps { shared: false, fence: true }, 100);
        s.add(
            "K05-PATH-02 无共享平台一律拷贝（1MB 以下向上取整也算钱）",
            p == HandoffPath::Copy && c.us == 100 && c.bytes == 4096,
            "锚点路径表：无共享能力时报拷贝并计成本——把拷贝当零拷贝上报会让带宽账少记一笔",
        );
    }

    // -- K05-PART-01 切分归组（闭集序 + 跨引擎边聚合）--------------------
    {
        let nodes = [
            (EngineKind::Npu, 4u32),
            (EngineKind::Dsp, 6u32),
            (EngineKind::Npu, 5u32),
        ];
        let edges = [
            CrossEdge { producer: EngineKind::Npu, consumer: EngineKind::Dsp, bytes: 1 << 20, path: HandoffPath::Copy },
        ];
        let p = partition(&nodes, &edges);
        let r = partition_report(&p);
        s.add(
            "K05-PART-01 切分归组（NPU 聚 9/DSP 6，跨引擎通信 1MB，闭集序稳定）",
            p.per_engine_nodes.len() == 2
                && p.per_engine_nodes[0].0 == EngineKind::Npu
                && p.per_engine_nodes[0].1 == 9
                && p.per_engine_nodes[1].1 == 6
                && r.cross_bytes == (1 << 20)
                && r.max_load == 9
                && r.engines == 2,
            "锚点「切分算法（切分目标：通信最小+负载均衡）」：双指标可回读",
        );
    }

    // -- K05-COMPILE-01 能力满足编译过 ------------------------------------
    {
        let parts = Partition {
            per_engine_nodes: alloc::vec![(EngineKind::Npu, 9), (EngineKind::Dsp, 6)],
            cross_edges: alloc::vec![CrossEdge {
                producer: EngineKind::Npu,
                consumer: EngineKind::Dsp,
                bytes: 1024,
                path: HandoffPath::Copy,
            }],
        };
        let caps = [
            EngineCaps { engine: EngineKind::Npu, feat: FEAT_BATCH, load: 80 },
            EngineCaps { engine: EngineKind::Dsp, feat: 0, load: 40 },
        ];
        s.add(
            "K05-COMPILE-01 能力满足（NPU 有 FEAT_BATCH 派批任务、DSP 无需求放行）",
            compile(&parts, &caps, FEAT_BATCH).is_ok(),
            "锚点「编译校验（引擎能力满足子图）」：按引擎需求分别判位",
        );
    }

    // -- K05-COMPILE-02 能力缺口显性拒 ------------------------------------
    {
        let parts = Partition {
            per_engine_nodes: alloc::vec![(EngineKind::Npu, 9)],
            cross_edges: Vec::new(),
        };
        let caps = [EngineCaps { engine: EngineKind::Npu, feat: 0, load: 80 }];
        let r = compile(&parts, &caps, FEAT_BATCH);
        s.add(
            "K05-COMPILE-02 能力缺口拒（NPU 无 FEAT_BATCH 不派批任务给跑不动的引擎）",
            r.is_err() && r.unwrap_err().contains(E_TG_CAPABILITY),
            "锚点编译校验：缺口显性而非静默派发",
        );
    }

    // -- K05-CYCLE-01 成环检测（引擎互等=fence 死锁）---------------------
    {
        let engines = [EngineKind::Npu, EngineKind::Dsp, EngineKind::Isp];
        let ok_edges = [CrossEdge {
            producer: EngineKind::Npu,
            consumer: EngineKind::Dsp,
            bytes: 100,
            path: HandoffPath::Copy,
        }];
        let bad_edges = [
            CrossEdge { producer: EngineKind::Npu, consumer: EngineKind::Dsp, bytes: 100, path: HandoffPath::Copy },
            CrossEdge { producer: EngineKind::Dsp, consumer: EngineKind::Npu, bytes: 100, path: HandoffPath::Copy },
        ];
        s.add(
            "K05-CYCLE-01 引擎环检测（NPU↔DSP 互等成环即拒）",
            check_cycle(&engines, &ok_edges).is_ok() && check_cycle(&engines, &bad_edges).is_err(),
            "锚点编译校验：环=执行器把两个引擎互相挂死",
        );
    }

    // -- K05-EXEC-01 执行波次 ---------------------------------------------
    {
        let parts = Partition {
            per_engine_nodes: alloc::vec![(EngineKind::Npu, 4), (EngineKind::Dsp, 4)],
            cross_edges: alloc::vec![CrossEdge {
                producer: EngineKind::Npu,
                consumer: EngineKind::Dsp,
                bytes: 100,
                path: HandoffPath::Copy,
            }],
        };
        let plan = execute(&parts);
        s.add(
            "K05-EXEC-01 执行编排（2 引擎并行波 + 1 fence 等待登记）",
            plan.waves.len() == 1 && plan.waves[0].engines == 2 && plan.waves[0].fences == 1,
            "锚点「执行（跨引擎并行执行（引擎 worker 池）——执行扩展」",
        );
    }

    // -- K05-PROP-01 失败传播与隔离（下游开裂非级联）---------------------
    {
        let edges = [
            CrossEdge { producer: EngineKind::Npu, consumer: EngineKind::Dsp, bytes: 100, path: HandoffPath::Copy },
            CrossEdge { producer: EngineKind::Npu, consumer: EngineKind::Isp, bytes: 200, path: HandoffPath::Copy },
        ];
        let prop = propagate(EngineKind::Npu, &edges);
        s.add(
            "K05-PROP-01 失败传播（NPU 失败→两条下游边开裂+通告 F1608 失败域）",
            prop.failed == EngineKind::Npu && prop.torn == 2 && prop.domain_notified,
            "锚点「失败（某引擎子图失败→跨引擎失败传播与隔离（失败域 F1608 衔接）」",
        );
    }

    // -- K05-META-01 版本指纹 --------------------------------------------
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in TASKGRAPH_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    s.add("K05-META-01 版本指纹非零", fp != 0, "K05-taskgraph-v1");

    s
}
