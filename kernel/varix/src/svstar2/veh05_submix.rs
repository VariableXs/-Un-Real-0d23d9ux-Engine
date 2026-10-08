//! VE-F1405 · 子混音与嵌套图（VE-H 域 · 音频引擎 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1405`
//!
//! **规格原文**：submix 子混音（一组节点打包为单节点复用——打包后的子图对外
//! 是单节点，端口即子图的出入口；鼓组三节点打包为一个鼓组 submix，总控一个
//! 推子）；嵌套层级（图内嵌套图——深度上限 8 层防失控，超限拒绝，上限声明
//! 式防护，F1122 纪律）；参数提升（嵌套图内部参数暴露到外层——封装与可控
//! 兼得，提升清单显性，提升与否是设计决策非默认）；子图计量（嵌套图 CPU
//! 归属统计——嵌套 CPU 计入宿主链，哪段子图最耗 CPU 可查）；模板库（房间
//! 修正/环绕下混/语言+音乐+音效三总线等常用预设）。
//! 判据：submix、八层上限、参数提升、计量、模板库、判据。
//!
//! **设计要点**：
//! - 封装即端口收缩：子图的出入口 = 打包节点的端口，宿主图只见一个节点
//!   一个推子——内部拓扑对外不可见（封装），但提升清单显性暴露选定参数
//!   （封装不黑盒）；
//! - 深度上限是声明式防护：打包时 depth+1，第 9 层直接拒绝——深层嵌套的
//!   调试与 CPU 归属复杂度是硬成本，上限写死不商量（F1122 纪律）；
//! - 参数提升是设计决策：没有默认提升——内部参数默认封装，逐条显性声明
//!   才可达（黑盒与可控的边界由设计者画）；
//! - 计量归属透明：子图内部 CPU 计入宿主链的打包节点，报表可下钻——
//!   哪段子图最耗 CPU 一查便知（资源透明，十二章纪律）；
//! - 模板即最佳实践的固化：游戏三总线（语言/音乐/音效）等预置图开箱即用，
//!   新项目从模板起步不从零建图。

use super::veh03_mixgraph::{Edge, MixGraph, NodeKind, Port};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、嵌套深度上限（声明式防护）
// ---------------------------------------------------------------------------

/// 嵌套深度上限（图内嵌套图最多 8 层）。
pub const MAX_NEST_DEPTH: u32 = 8;

/// 子混音（打包后的子图）。
#[derive(Clone, Debug)]
pub struct Submix {
    /// 在宿主图中的占位节点号。
    pub external_id: u32,
    pub name: String,
    /// 内部图（封装，宿主不可直接改）。
    pub inner: MixGraph,
    /// 内部图的入口节点（外部音频从这里进入）。
    pub entry_id: u32,
    /// 内部图的出口节点（音频从这里离开子图）。
    pub exit_id: u32,
    /// 提升清单：(内部节点号, 参数名, 对外参数名)。提升与否是设计决策。
    pub promoted: Vec<(u32, String, String)>,
    /// 本子图的嵌套深度（宿主图 = 第 0 层）。
    pub depth: u32,
}

/// 子混音错误（三要素）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmixError {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl SubmixError {
    fn new(code: &'static str, what: String, why: String, next: String) -> SubmixError {
        SubmixError { code, what, why, next }
    }
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 二、嵌套图宿主（打包 / 参数提升 / 封装纪律）
// ---------------------------------------------------------------------------

/// 嵌套图宿主：宿主图 + 子混音表。
pub struct NestedGraphHost {
    pub graph: MixGraph,
    pub submixes: Vec<Submix>,
    pub next_external: u32,
}

impl NestedGraphHost {
    pub fn new() -> NestedGraphHost {
        NestedGraphHost {
            graph: MixGraph::new(),
            submixes: Vec::new(),
            next_external: 1,
        }
    }

    /// 打包：内部图 + 出入口 → 宿主图上的单节点（占位），端口即出入口。
    ///
    /// 深度上限在打包时点强制：depth+1 > 8 直接拒绝（声明式防护）。
    pub fn package(
        &mut self,
        name: &str,
        inner: MixGraph,
        entry_id: u32,
        exit_id: u32,
        parent_depth: u32,
        in_port: Option<Port>,
        out_port: Option<Port>,
    ) -> Result<u32, SubmixError> {
        let depth = parent_depth + 1;
        if depth > MAX_NEST_DEPTH {
            return Err(SubmixError::new(
                "E_DEPTH_EXCEEDED",
                format!(
                    "嵌套深度 {} 超过上限 {} 层",
                    depth, MAX_NEST_DEPTH
                ),
                "深层嵌套的调试与 CPU 归属复杂度是硬成本——第 9 层不商量".to_string(),
                "展平一层子图，或把公共部分提为共享效果".to_string(),
            ));
        }
        // 出入口必须在内部图内且语义正确：入口有入口端口、出口有出口端口。
        let entry = inner.find(entry_id).ok_or_else(|| {
            SubmixError::new(
                "E_ENTRY_MISSING",
                format!("入口节点 {} 不在子图内", entry_id),
                "端口即子图的出入口：入口不存在则封装无从谈起".to_string(),
                "指定子图内部真实存在的入口节点".to_string(),
            )
        })?;
        if entry.in_port.is_none() {
            return Err(SubmixError::new(
                "E_ENTRY_MISSING",
                format!("入口节点 {} 没有入口端口", entry_id),
                "外部音频要进得来：入口节点必须有入口端口".to_string(),
                "给入口节点补入口端口".to_string(),
            ));
        }
        let exit = inner.find(exit_id).ok_or_else(|| {
            SubmixError::new(
                "E_EXIT_MISSING",
                format!("出口节点 {} 不在子图内", exit_id),
                "端口即子图的出入口：出口不存在则封装无从谈起".to_string(),
                "指定子图内部真实存在的出口节点".to_string(),
            )
        })?;
        if exit.out_port.is_none() {
            return Err(SubmixError::new(
                "E_EXIT_MISSING",
                format!("出口节点 {} 没有出口端口", exit_id),
                "子图音频要出得去：出口节点必须有出口端口".to_string(),
                "给出口节点补出口端口".to_string(),
            ));
        }
        // 占位节点号：跳过宿主图与子混音表已占用的号（模板起步宿主非空）。
        while self.graph.find(self.next_external).is_some()
            || self.submixes.iter().any(|s| s.external_id == self.next_external)
        {
            self.next_external += 1;
        }
        let external_id = self.next_external;
        self.next_external += 1;
        // 占位节点：宿主图上是一个处理节点（有入有出）。
        self.graph
            .add_node(
                external_id,
                NodeKind::Bus,
                &format!("submix:{}", name),
                in_port,
                out_port,
            )
            .map_err(|e| {
                SubmixError::new("E_PACK_HOST", e.what, e.why, e.next)
            })?;
        self.submixes.push(Submix {
            external_id,
            name: name.to_string(),
            inner,
            entry_id,
            exit_id,
            promoted: Vec::new(),
            depth,
        });
        Ok(external_id)
    }

    fn find_submix_mut(&mut self, external_id: u32) -> Option<&mut Submix> {
        self.submixes
            .iter_mut()
            .find(|s| s.external_id == external_id)
    }

    /// 参数提升：显性声明一条内部参数为子图对外参数（设计决策，无默认）。
    pub fn promote(
        &mut self,
        external_id: u32,
        inner_node: u32,
        inner_param: &str,
        external_name: &str,
    ) -> Result<(), SubmixError> {
        let s = self.find_submix_mut(external_id).ok_or_else(|| {
            SubmixError::new(
                "E_NO_SUBMIX",
                format!("子混音 {} 不存在", external_id),
                "只能对已打包的子图做参数提升".to_string(),
                "先 package 再 promote".to_string(),
            )
        })?;
        if s.inner.find(inner_node).is_none() {
            return Err(SubmixError::new(
                "E_PROMOTE_NO_NODE",
                format!("内部节点 {} 不在子图内", inner_node),
                "提升的是内部参数：节点不存在就是空挂".to_string(),
                "指定子图内部真实存在的节点".to_string(),
            ));
        }
        if s.promoted.iter().any(|(_, _, ext)| ext == external_name) {
            return Err(SubmixError::new(
                "E_PROMOTE_DUP",
                format!("对外参数名 {} 已被提升占用", external_name),
                "一物双名让控制面失去对象身份".to_string(),
                "换一个对外参数名".to_string(),
            ));
        }
        s.promoted.push((
            inner_node,
            inner_param.to_string(),
            external_name.to_string(),
        ));
        Ok(())
    }

    /// 经由提升清单设置参数：未提升的内部参数不可达（封装纪律）。
    pub fn set_param(
        &mut self,
        external_id: u32,
        external_name: &str,
        value: f64,
    ) -> Result<(), SubmixError> {
        let s = self.find_submix_mut(external_id).ok_or_else(|| {
            SubmixError::new(
                "E_NO_SUBMIX",
                format!("子混音 {} 不存在", external_id),
                "只能对已打包的子图设参".to_string(),
                "先 package".to_string(),
            )
        })?;
        let hit = s
            .promoted
            .iter()
            .find(|(_, _, ext)| ext == external_name)
            .ok_or_else(|| {
                SubmixError::new(
                    "E_PARAM_NOT_EXPOSED",
                    format!("参数 {} 未提升——封装的内部参数外部不可达", external_name),
                    "提升与否是设计决策：没提升就不许绕过封装直改内部".to_string(),
                    "先 promote 该参数（设计决策留痕），或用已提升参数".to_string(),
                )
            })?
            .clone();
        // 参数落点记入内部图的节点名账（本模型参数账面：节点名后缀记账）。
        if let Some(n) = s.inner.find_port_mut(hit.0) {
            n.name = format!("{}@{}={:.3}", n.name.split('@').next().unwrap_or("n"), hit.1, value);
        }
        Ok(())
    }

    /// 读屏摘要。
    pub fn a11y_summary(&self) -> String {
        format!(
            "嵌套图宿主：宿主节点 {}、子混音 {} 个（最深第 {} 层 / 上限 {}）",
            self.graph.nodes.len(),
            self.submixes.len(),
            self.submixes.iter().map(|s| s.depth).max().unwrap_or(0),
            MAX_NEST_DEPTH
        )
    }
}

// ---------------------------------------------------------------------------
// 三、子图计量（CPU 归属：嵌套计入宿主链，最耗可查）
// ---------------------------------------------------------------------------

/// 一条计量账：(归属路径, CPU 开销 µs)。
#[derive(Clone, Debug, PartialEq)]
pub struct MeterEntry {
    /// 归属路径（如 "host > 鼓组submix > 压缩器"）。
    pub path: String,
    pub cost_us: u64,
}

/// CPU 计量器：逐节点记账 + 子图开销上卷到宿主占位节点。
#[derive(Debug, Default)]
pub struct CpuMeter {
    pub entries: Vec<MeterEntry>,
}

impl CpuMeter {
    pub fn new() -> CpuMeter {
        CpuMeter::default()
    }

    /// 记宿主图直接节点的开销。
    pub fn account_host(&mut self, node_name: &str, cost_us: u64) {
        self.entries.push(MeterEntry {
            path: format!("host > {}", node_name),
            cost_us,
        });
    }

    /// 记子图内部节点开销：归属路径带子图名（归属透明的凭证）。
    pub fn account_submix(&mut self, submix_name: &str, node_name: &str, cost_us: u64) {
        self.entries.push(MeterEntry {
            path: format!("host > {} > {}", submix_name, node_name),
            cost_us,
        });
    }

    /// 归属报表：按路径聚合、开销降序——哪段子图最耗 CPU 一查便知。
    pub fn report(&self) -> Vec<(String, u64)> {
        let mut agg: Vec<(String, u64)> = Vec::new();
        for e in self.entries.iter() {
            match agg.iter_mut().find(|(p, _)| *p == e.path) {
                Some((_, c)) => *c += e.cost_us,
                None => agg.push((e.path.clone(), e.cost_us)),
            }
        }
        agg.sort_by(|a, b| b.1.cmp(&a.1));
        agg
    }

    /// 子图聚合视图：全部子图内部开销上卷后的降序清单。
    pub fn report_by_top_level(&self) -> Vec<(String, u64)> {
        let mut agg: Vec<(String, u64)> = Vec::new();
        for e in self.entries.iter() {
            let top = e.path.split('>').nth(1).map(|s| s.trim().to_string()).unwrap_or_default();
            match agg.iter_mut().find(|(p, _)| *p == top) {
                Some((_, c)) => *c += e.cost_us,
                None => agg.push((top, e.cost_us)),
            }
        }
        agg.sort_by(|a, b| b.1.cmp(&a.1));
        agg
    }
}

// ---------------------------------------------------------------------------
// 四、模板库（预置图 = 最佳实践的固化）
// ---------------------------------------------------------------------------

fn p(channels: u8, rate: u32) -> Port {
    Port {
        channels,
        sample_rate: rate,
        resample_declared: false,
    }
}

/// 模板一：游戏三总线（语言 / 音乐 / 音效 → 主总线 → 监听）。
pub fn template_game_three_bus() -> Result<MixGraph, super::veh03_mixgraph::GraphError> {
    let mut g = MixGraph::new();
    g.add_node(1, NodeKind::Source, "语言", None, Some(p(1, 48_000)))?;
    g.add_node(2, NodeKind::Source, "音乐", None, Some(p(2, 48_000)))?;
    g.add_node(3, NodeKind::Source, "音效", None, Some(p(2, 48_000)))?;
    g.add_node(4, NodeKind::Bus, "语言总线", Some(p(1, 48_000)), Some(p(1, 48_000)))?;
    g.add_node(5, NodeKind::Bus, "音乐总线", Some(p(2, 48_000)), Some(p(2, 48_000)))?;
    g.add_node(6, NodeKind::Bus, "音效总线", Some(p(2, 48_000)), Some(p(2, 48_000)))?;
    g.add_node(7, NodeKind::Bus, "主总线", Some(p(2, 48_000)), Some(p(2, 48_000)))?;
    g.add_node(8, NodeKind::Listener, "监听", Some(p(2, 48_000)), None)?;
    // 语言单声道 → 总线声明转换上混到主总线
    g.find_port_mut(4).map(|n| n.out_port = Some(p(1, 48_000)));
    g.find_port_mut(7).map(|n| n.in_port = Some(p(1, 48_000)));
    g.find_port_mut(7).map(|n| {
        if let Some(ip) = n.in_port.as_mut() {
            ip.resample_declared = true; // 下混转换声明
        }
        if let Some(op) = n.out_port.as_mut() {
            op.resample_declared = true;
        }
    });
    g.connect(Edge { from: 1, to: 4 })?;
    g.connect(Edge { from: 2, to: 5 })?;
    g.connect(Edge { from: 3, to: 6 })?;
    g.connect(Edge { from: 4, to: 7 })?;
    g.connect(Edge { from: 5, to: 7 })?;
    g.connect(Edge { from: 6, to: 7 })?;
    g.connect(Edge { from: 7, to: 8 })?;
    g.validate_all()?;
    Ok(g)
}

/// 模板二：房间修正链（源 → 测量 EQ → 房间总线 → 监听）。
pub fn template_room_correction() -> Result<MixGraph, super::veh03_mixgraph::GraphError> {
    let mut g = MixGraph::new();
    g.add_node(1, NodeKind::Source, "扬声器测试信号", None, Some(p(2, 48_000)))?;
    g.add_node(2, NodeKind::Processor, "房间修正EQ", Some(p(2, 48_000)), Some(p(2, 48_000)))?;
    g.add_node(3, NodeKind::Bus, "房间总线", Some(p(2, 48_000)), Some(p(2, 48_000)))?;
    g.add_node(4, NodeKind::Listener, "监听", Some(p(2, 48_000)), None)?;
    g.connect(Edge { from: 1, to: 2 })?;
    g.connect(Edge { from: 2, to: 3 })?;
    g.connect(Edge { from: 3, to: 4 })?;
    g.validate_all()?;
    Ok(g)
}

/// 模板三：环绕下混（5.1 源 → 下混声明 → 立体声监听）。
pub fn template_surround_downmix() -> Result<MixGraph, super::veh03_mixgraph::GraphError> {
    let mut g = MixGraph::new();
    g.add_node(1, NodeKind::Source, "5.1源", None, Some(p(6, 48_000)))?;
    g.add_node(2, NodeKind::Processor, "下混矩阵", Some(p(6, 48_000)), Some(p(2, 48_000)))?;
    g.add_node(3, NodeKind::Listener, "立体声监听", Some(p(2, 48_000)), None)?;
    g.connect(Edge { from: 1, to: 2 })?;
    g.connect(Edge { from: 2, to: 3 })?;
    g.validate_all()?;
    Ok(g)
}
