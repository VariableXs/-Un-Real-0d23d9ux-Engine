//! CGPU-F0162 · 资源读写集自动依赖推导（CGPU-B 域 · 批次 B01 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0162`
//!
//! **判据（锚点原文）**：RAW/WAR/WAW 全测、子资源粒度验证、边合并正确、
//! 漏边零容忍注入测试、推导性能。
//!
//! **职责定位（锚点原文）**：任务依赖的自动推导：每任务声明读写资源集
//! （纹理/缓冲/表面），调度器按资源在任务间的访问序自动建边——写后读
//! （RAW）边、读后写（WAR）边、写后写（WAW）边，**读读不建边（可并行）**。
//! 资源粒度：**子资源级**（mip 层/数组层独立——同纹理不同 mip 可并行）。
//! 冲突消解：多资源多任务时的**边合并**（同对任务多资源冲突只建一条合并
//! 边）。
//!
//! # 一、为什么建边规则只有三条而「读读不建边」是第四条纪律
//!
//! RAW/WAR/WAW 是数据流的铁律（谁写我读、谁读我写、谁先写）；读读不建边
//! 则是并行的生命线——若两个只读任务被连上边，整条后续链都被串行化，
//! GPU 的并行性被一个「多此一举」的依赖吃掉。推导器（[`DepDeriver`]）的
//! 边来源**只有**写者与读者相遇，读后读不产生任何边——「可并行」不是
//! 优化，是推导规则的缺席（判据专门测两连读零边）。
//!
//! # 二、为什么粒度必须到「子资源」
//!
//! 纹理不是原子的：mip0 与 mip5 是不同的存储区域，数组层 0 与层 1 互不
//! 覆盖。若按整纹理建边，一份高分辨率烘焙（只写 mip 链）会与一份只读
//! mip5 的任务被错误串行——粒度粗一级，并行性丢一层。子资源键
//! （[`SubRes`]：base+mip+array_layer）是建边的最小单元：同 base 不同
//! mip 的两个任务**推导不出任何边**（判据专门验证）。
//!
//! # 三、为什么同对任务要「合并成一条边」且保留成因
//!
//! 两个任务间若有 5 个资源冲突，逐资源建边就是 5 条平行边——调度器只
//! 需要「A 在 B 前」这一个事实，多余的边是账面噪音。故同对任务合并为
//! 一条边（[`DerivedEdge`]），但**成因不丢**（`raw/war/waw` 三布尔逐项
//! 记录这次合并里发生过哪些冲突）——调试器与验证器能回答「这条边为什么
//! 在」，合并才不是信息损失。
//!
//! # 四、漏边为什么「零容忍」且用注入测试钉死
//!
//! 漏一条 RAW 边，渲染出的是上一帧的数据——错误是视觉的、偶发的、最难
//! 复现的。故推导的判据口径是**注入测试**：构造读写序完全已知的场景，
//! 手算期望边集，与推导输出**逐条对账**——不漏（期望边全在）也不滥
//! （无期望外边）。双向对账让「多建边」与「漏建边」同等级曝光。
//!
//! # 五、与相邻条的分工
//!
//! F0161 提供图数据结构（[`vcb01_framegraph`] 快照——本条的消费入口，
//! 扁平资源按子资源单层退化接入）；F0163+ 的发射调度消费推导产物
//! （[`DerivedGraph`]——只含边事实，不含执行序）。本条只管「**边从
//! 哪来、怎么合、漏没漏**」，不管怎么排、怎么发。
//!
//! **性能（锚点原文）**：推导按声明序单遍扫描——每任务每资源 O(打开读者
//! 数)，边合并按对去重；万节点链式场景判据压测覆盖。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::svstar2::vcb01_framegraph as fg;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const DEPDERIVE_VERSION: &str = "CB02-depderive-v1";

/// 任务未知（句柄越界）。
pub const E_TASK_UNKNOWN: &str = "E_TASK_UNKNOWN";

/// 依赖边落到 vcb01 构建器时的型别优先级（RAW > WAR > WAW——数据依赖
/// 优先于反序依赖；同对合并边落回单型边时按此取主因，钉死可对账）。
pub const EDGE_PRIORITY: [&str; 3] = ["raw", "war", "waw"];

// ---------------------------------------------------------------------------
// 二、子资源键（mip 层/数组层独立——粒度红线）
// ---------------------------------------------------------------------------

/// 子资源键：base 资源 + mip 层 + 数组层（建边的最小粒度单元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SubRes {
    /// 资源基 id（纹理/缓冲的 ID 索引——A 域资源表口径）。
    pub base: fg::ResId,
    /// mip 层（同 base 不同 mip 相互独立）。
    pub mip: u16,
    /// 数组层（同 base 不同数组层相互独立）。
    pub array_layer: u16,
}

impl SubRes {
    /// 整资源键（全 mip 全数组层——粗粒度声明入口，如深度缓冲整体清空）。
    pub const fn whole(base: fg::ResId) -> SubRes {
        SubRes { base, mip: u16::MAX, array_layer: u16::MAX }
    }

    /// 扁平 ResId → 子资源键（vcb01 扁平资源的单层退化：mip/层归 0）。
    pub const fn from_flat(r: fg::ResId) -> SubRes {
        SubRes { base: r, mip: 0, array_layer: 0 }
    }

    /// 是否与键 other 属同一 base（粒度对账辅助）。
    pub const fn same_base(&self, other: &SubRes) -> bool {
        self.base == other.base
    }
}

// ---------------------------------------------------------------------------
// 三、任务表（推导输入：句柄 + 子资源读写集）
// ---------------------------------------------------------------------------

/// 推导输入任务（子资源级读写集）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskAccess {
    /// 任务句柄（对齐 vcb01 的 NodeHandle——声明序即池序）。
    pub handle: u32,
    /// 读子资源集。
    pub reads: Vec<SubRes>,
    /// 写子资源集。
    pub writes: Vec<SubRes>,
}

// ---------------------------------------------------------------------------
// 四、合并边与推导产物
// ---------------------------------------------------------------------------

/// 推导依赖类型（三成因布尔——合并边保留完整成因，合并不是信息损失）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepReason {
    /// 写后读成因存在。
    pub raw: bool,
    /// 读后写成因存在。
    pub war: bool,
    /// 写后写成因存在。
    pub waw: bool,
}

impl DepReason {
    /// 空成因（不可能出现在真实边上——判据用来断言边必带因）。
    pub const NONE: DepReason = DepReason { raw: false, war: false, waw: false };

    /// 是否至少含一个成因（真实边恒真）。
    pub const fn any(&self) -> bool {
        self.raw || self.war || self.waw
    }

    /// 主因短码（落回 vcb01 单型边时的优先级映射：RAW > WAR > WAW）。
    pub const fn primary(&self) -> &'static str {
        if self.raw {
            "raw"
        } else if self.war {
            "war"
        } else {
            "waw"
        }
    }

    /// 成因中文名（读屏可达：列出全部命中的成因）。
    pub fn zh(&self) -> &'static str {
        match (self.raw, self.war, self.waw) {
            (true, false, false) => "写后读",
            (false, true, false) => "读后写",
            (false, false, true) => "写后写",
            (true, true, false) => "写后读+读后写",
            (true, false, true) => "写后读+写后写",
            (false, true, true) => "读后写+写后写",
            (true, true, true) => "写后读+读后写+写后写",
            (false, false, false) => "无成因",
        }
    }

    /// 从 vcb01 单型边反推成因（衔接闭环：手工建边与自动推导同口径）。
    pub const fn from_kind(kind: fg::EdgeKind) -> DepReason {
        match kind {
            fg::EdgeKind::ReadAfterWrite => DepReason { raw: true, war: false, waw: false },
            fg::EdgeKind::WriteAfterRead => DepReason { raw: false, war: true, waw: false },
            fg::EdgeKind::WriteAfterWrite => DepReason { raw: false, war: false, waw: true },
            // 用户显式边不属三类数据成因——记为全假（any()=false，
            // 调用方须自行判定显式边不进推导对账）。
            fg::EdgeKind::Explicit => DepReason::NONE,
        }
    }
}

/// 合并依赖边（同对任务一条边——成因逐项保留）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedEdge {
    /// 前驱（先执行）。
    pub from: u32,
    /// 后继（后执行）。
    pub to: u32,
    /// 成因（至少一项为真——无因边=推导器 bug，判据拦截）。
    pub reason: DepReason,
}

/// 推导产物：边表 + 三型计数账。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DerivedGraph {
    edges: Vec<DerivedEdge>,
    /// RAW 边发生次数（按冲突计，非合并后边数）。
    pub raw_hits: u32,
    /// WAR 边发生次数。
    pub war_hits: u32,
    /// WAW 边发生次数。
    pub waw_hits: u32,
}

impl DerivedGraph {
    /// 空产物。
    pub fn new() -> DerivedGraph {
        DerivedGraph { edges: Vec::new(), raw_hits: 0, war_hits: 0, waw_hits: 0 }
    }

    /// 内部插入：同对任务存在则**合并成因**，否则新边（边合并红线）。
    fn upsert(&mut self, from: u32, to: u32, r: DepReason) {
        for e in self.edges.iter_mut() {
            if e.from == from && e.to == to {
                e.reason.raw = e.reason.raw || r.raw;
                e.reason.war = e.reason.war || r.war;
                e.reason.waw = e.reason.waw || r.waw;
                return;
            }
        }
        self.edges.push(DerivedEdge { from, to, reason: r });
    }

    /// 边数（合并后）。
    pub fn len(&self) -> usize {
        self.edges.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// 查某对任务之间的合并边。
    pub fn edge_between(&self, from: u32, to: u32) -> Option<&DerivedEdge> {
        self.edges.iter().find(|e| e.from == from && e.to == to)
    }

    /// 某任务的前驱数（发射调度消费面）。
    pub fn in_degree(&self, to: u32) -> usize {
        self.edges.iter().filter(|e| e.to == to).count()
    }

    /// 边表只读视图（按插入序=声明序推导序）。
    pub fn edges(&self) -> &[DerivedEdge] {
        self.edges.as_slice()
    }

    /// 无环验证（推导产物的结构性承诺：声明序扫描天然无环——from 恒小于
    /// to 时必无环；存在 from >= to 即推导器被误用/被污染，判据拦截）。
    ///
    /// 返回 true = 结构无环（全部边 from < to）。
    pub fn is_acyclic(&self) -> bool {
        self.edges.iter().all(|e| e.from < e.to)
    }
}

// ---------------------------------------------------------------------------
// 五、推导器（声明序单遍扫描——资源状态机建边）
// ---------------------------------------------------------------------------

/// 单个资源键的访问状态（last writer + 打开读者集——读读不建边的载体）。
#[derive(Clone, Debug, Default)]
struct ResState {
    last_writer: Option<u32>,
    open_readers: Vec<u32>,
}

/// 依赖推导器。
#[derive(Clone, Debug, Default)]
pub struct DepDeriver {
    graph: DerivedGraph,
    states: Vec<(SubRes, ResState)>,
}

impl DepDeriver {
    /// 空推导器。
    pub fn new() -> DepDeriver {
        DepDeriver { graph: DerivedGraph::new(), states: Vec::new() }
    }

    /// 按键取状态（键有序插入——二分定位 O(log n)）。
    fn state_of(&mut self, key: SubRes) -> &mut ResState {
        match self.states.binary_search_by(|(k, _)| k.cmp(&key)) {
            Ok(i) => &mut self.states[i].1,
            Err(pos) => {
                self.states.insert(pos, (key, ResState::default()));
                &mut self.states[pos].1
            }
        }
    }

    /// 访问一个任务：按声明序处理其读写集并建边（推导核心，三规则）。
    ///
    /// 1. 读 r：r 的 last_writer → 本任务 RAW 边；本任务加入 open_readers。
    /// 2. 写 r：last_writer → WAW；open_readers 全体 → WAR；
    ///    然后 last_writer=本任务、open_readers 清空。
    /// 3. 读读：只入 open_readers，不建边（可并行的结构性来源）。
    pub fn visit(&mut self, t: &TaskAccess) -> Result<(), &'static str> {
        // 读集：先全部处理（RAW），再入打开读者。
        for r in t.reads.iter() {
            let w = self.state_of(*r).last_writer;
            if let Some(from) = w {
                if from != t.handle {
                    self.graph.raw_hits += 1;
                    self.graph.upsert(from, t.handle, DepReason { raw: true, war: false, waw: false });
                }
            }
        }
        for r in t.reads.iter() {
            let st = self.state_of(*r);
            if !st.open_readers.contains(&t.handle) {
                st.open_readers.push(t.handle);
            }
        }
        // 写集：WAW + WAR，然后收口状态。
        for w in t.writes.iter() {
            let prev_writer = self.state_of(*w).last_writer;
            if let Some(from) = prev_writer {
                if from != t.handle {
                    self.graph.waw_hits += 1;
                    self.graph.upsert(from, t.handle, DepReason { raw: false, war: false, waw: true });
                }
            }
            let readers = {
                let st = self.state_of(*w);
                core::mem::take(&mut st.open_readers)
            };
            for rd in readers.iter() {
                if *rd != t.handle {
                    self.graph.war_hits += 1;
                    self.graph.upsert(*rd, t.handle, DepReason { raw: false, war: true, waw: false });
                }
            }
            // 写者接管：last_writer=本任务、open_readers 保持清空——
            // 后续读者改从本任务建 RAW（旧读者已全部经 WAR 收口）。
            let st = self.state_of(*w);
            st.last_writer = Some(t.handle);
        }
        Ok(())
    }

    /// 推导产物（只读引用——多消费者安全）。
    pub fn result(&self) -> &DerivedGraph {
        &self.graph
    }

    /// 批量推导入口：按声明序扫描任务表（visit 的多任务收口）。
    ///
    /// 返回推导产物；任一任务处理失败即显性报错（不静默跳过——漏边
    /// 零容忍在入口层同样生效）。
    pub fn derive_all(&mut self, tasks: &[TaskAccess]) -> Result<&DerivedGraph, &'static str> {
        for t in tasks.iter() {
            self.visit(t)?;
        }
        Ok(&self.graph)
    }
}

// ---------------------------------------------------------------------------
// 六、场景入口（漏边零容忍的注入测试台 + vcb01 衔接）
// ---------------------------------------------------------------------------

/// 从 vcb01 快照推导（扁平资源单层退化接入——F0161 消费面）。
///
/// 每个节点的 ResId 经 [`SubRes::from_flat`] 升为子资源键，声明序=池序。
pub fn from_frame_graph(g: &fg::FrameGraph) -> Result<DerivedGraph, &'static str> {
    let mut d = DepDeriver::new();
    for h in 0..g.node_count() as u32 {
        let reads: Vec<SubRes> = g
            .reads_of(h)
            .unwrap_or(&[])
            .iter()
            .map(|r| SubRes::from_flat(*r))
            .collect();
        let writes: Vec<SubRes> = g
            .writes_of(h)
            .unwrap_or(&[])
            .iter()
            .map(|r| SubRes::from_flat(*r))
            .collect();
        d.visit(&TaskAccess { handle: h, reads, writes })?;
    }
    Ok(DerivedGraph {
        edges: d.graph.edges.clone(),
        raw_hits: d.graph.raw_hits,
        war_hits: d.graph.war_hits,
        waw_hits: d.graph.waw_hits,
    })
}

/// 合并边 → vcb01 边表（按 [`EDGE_PRIORITY`] 主因落回单型边——跨条衔接
/// 的映射规则钉死，判据对账）。
pub fn to_builder_edges(dg: &DerivedGraph) -> Result<Vec<fg::Edge>, &'static str> {
    let mut out = Vec::new();
    for e in dg.edges().iter() {
        if !e.reason.any() {
            return Err(E_TASK_UNKNOWN);
        }
        let kind = match e.reason.primary() {
            "raw" => fg::EdgeKind::ReadAfterWrite,
            "war" => fg::EdgeKind::WriteAfterRead,
            _ => fg::EdgeKind::WriteAfterWrite,
        };
        out.push(fg::Edge { from: e.from, to: e.to, kind });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 七、读屏替述
// ---------------------------------------------------------------------------

/// 推导结果读屏单行（边数+三型计数；无资源明细）。
pub fn screen_line_derive(dg: &DerivedGraph) -> String {
    format!(
        "依赖推导：合并后 {} 条边（RAW {} 次 / WAR {} 次 / WAW {} 次）",
        dg.len(),
        dg.raw_hits,
        dg.war_hits,
        dg.waw_hits
    )
}
