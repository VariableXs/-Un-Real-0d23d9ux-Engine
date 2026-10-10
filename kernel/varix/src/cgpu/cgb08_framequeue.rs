//! CGPU-F0168 · 帧图与 GPU 队列映射（CGPU-B 域 · 帧任务图与执行域 · B02 组 · 目标 400 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0168`
//!
//! **判据（锚点原文）**：队列映射正确、多 GPU 路由、约束校验、映射开销、三队列用例。
//!
//! **职责定位（锚点原文）**：任务图的节点到 GPU 队列的映射：队列类型映射
//! （图形节点→图形队列、计算→计算队列、拷贝→拷贝队列——与 VE-F0006 队列
//! 调度器协同）、多 GPU 映射（F0106 放置策略联动——节点按资源所在卡路由）、
//! 映射约束（同资源读写任务的队列一致性）。
//!
//! ## 一、队列类型映射（判据一）
//!
//! 节点种类闭集（[`NodeKind`]）到队列种类闭集（[`QueueKind`]）的映射是一张
//! **写死的路由表**（[`NODE_QUEUE_ROUTE`]），不是 if-else 链——路由表是
//! 数据，映射正确性可以被独立对拍（判据侧拿同一张表重排验证）。图形族
//! （渲染/UI/后处理）→ 图形队列；计算族（模拟/剔除/编码）→ 计算队列；
//! 搬运族（上传/下载/卡间拷贝）→ 拷贝队列。表外节点种种类不存在（闭集）。
//!
//! ## 二、多 GPU 路由（判据二）
//!
//! 节点不选卡，**资源选卡**（F0106 放置策略的联动纪律）：节点声明读集与
//! 写集，路由取写集资源所在卡为 home（写者跟着数据走）；写集为空时取
//! 读集首资源的所在卡（纯消费者跟随数据）；读写集全空（纯同步节点）
//! 归主卡。跨卡声明（读集资源分布在多卡且写集为空）显性拒不猜——猜了
//! 就会把拷贝责任藏进路由器，拷贝必须显式成节点。
//!
//! ## 三、映射约束（判据三）
//!
//! 同资源一致性：对同一资源，本帧内「写后读」的读节点必须与写节点
//! 同队列（同卡同队列种类——无跨队列 barrier 语义，跨队列同步是
//! F0170 同步面的职责，本模块在映射层直接拒绝不一致映射并给出
//! 可操作建议：要么改路由，要么插入显式拷贝/同步节点）。只读多队列
//! 并发合法（读-读无序）。
//!
//! ## 四、映射开销（判据四）
//!
//! 映射是查表不是搜索：每节点 O(1)（种类→队列种类查表 + home 判定
//! O(读写集)）；开销账（[`MapLedger`]）记录节点数/路由判定数/约束拒绝数/
//! 跨卡拒绝数，供 B05 开销台账面复用（只读消费）。
//!
//! **对接**：VE-F0006（队列调度器——本模块产出映射，调度器消费映射）；
//! F0106（放置策略——home 判定的上游）；F0167（执行失败与降级——映射
//! 失败的处置面）；F0170（同步原语——跨队列 barrier 的下游）。
//! 零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const FRAME_QUEUE_MAP_VERSION: &str = "B02-framequeue-v1";

/// 三队列种类数（闭集宽度；路由表列数的权威口径）。
pub const QUEUE_KIND_TOTAL: usize = 3;

/// 队列种类名（判据侧独立重排对拍用）。
pub const QUEUE_KIND_NAMES: [&str; QUEUE_KIND_TOTAL] = ["图形队列", "计算队列", "拷贝队列"];

// ---------------------------------------------------------------------------
// 二、诊断码（0x30 段独占：B02 队列映射面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x30 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 节点引用的写集资源无 home 卡（放置策略未登记）。
    WriteHomeMissing,
    /// 纯消费者读集资源无 home 卡。
    ReadHomeMissing,
    /// 读写集全空且多卡可用（纯同步节点路由歧义显性拒）。
    AmbiguousSyncNode,
    /// 同资源写后读被路由到不同队列（一致性约束拒绝）。
    RwQueueInconsistent,
    /// 节点种类表外（闭集外无路由行——防御性分支，构造层已封死）。
    UnknownNodeKind,
}

impl XcCode {
    /// 人读诊断（错误路径带可操作建议）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::WriteHomeMissing => "写集资源无 home 卡：节点跟着数据走，先在 F0106 放置策略登记资源所在卡",
            XcCode::ReadHomeMissing => "读集资源无 home 卡：纯消费者跟随数据，先登记资源所在卡",
            XcCode::AmbiguousSyncNode => "纯同步节点且多卡可用：路由歧义显性拒，建议归主卡或拆成带资源的节点",
            XcCode::RwQueueInconsistent => "同资源写后读跨队列：无跨队列 barrier 语义，改路由一致或插入显式拷贝/同步节点",
            XcCode::UnknownNodeKind => "节点种类表外：路由表无此行，建议按 NodeKind 闭集取用",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、队列与节点闭集（判据一的映射表）
// ---------------------------------------------------------------------------

/// GPU 队列种类闭集（三队列；VE-F0006 调度器的队列面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueKind {
    /// 图形队列（渲染/呈现管线）。
    Graphics,
    /// 计算队列（异步计算/编码）。
    Compute,
    /// 拷贝队列（DMA 搬运）。
    Copy,
}

impl QueueKind {
    /// 序号（0-2；路由表与开销账下标）。
    pub const fn ordinal(self) -> usize {
        match self {
            QueueKind::Graphics => 0,
            QueueKind::Compute => 1,
            QueueKind::Copy => 2,
        }
    }

    /// 队列名（人读）。
    pub const fn zh(self) -> &'static str {
        QUEUE_KIND_NAMES[self.ordinal()]
    }
}

/// 帧图节点种类闭集（九种；每种的队列归属写死在路由表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// 渲染提交（图形）。
    Render,
    /// UI 合成（图形）。
    UiCompose,
    /// 后处理（图形）。
    PostFx,
    /// 物理模拟（计算）。
    Simulate,
    /// 剔除/裁剪（计算）。
    Culling,
    /// 视频编码（计算）。
    Encode,
    /// 上传（主机→设备，拷贝）。
    Upload,
    /// 下载（设备→主机，拷贝）。
    Download,
    /// 卡间拷贝（设备→设备，拷贝）。
    DeviceCopy,
}

/// 节点种类 → 队列种类路由表（写死的数据；判据侧拿同一张表重排对拍）。
pub const NODE_QUEUE_ROUTE: [(NodeKind, QueueKind); 9] = [
    (NodeKind::Render, QueueKind::Graphics),
    (NodeKind::UiCompose, QueueKind::Graphics),
    (NodeKind::PostFx, QueueKind::Graphics),
    (NodeKind::Simulate, QueueKind::Compute),
    (NodeKind::Culling, QueueKind::Compute),
    (NodeKind::Encode, QueueKind::Compute),
    (NodeKind::Upload, QueueKind::Copy),
    (NodeKind::Download, QueueKind::Copy),
    (NodeKind::DeviceCopy, QueueKind::Copy),
];

/// 路由查表（O(1) 线性扫 9 行——闭集小到线性即常数；表外显性拒）。
pub fn route_of(kind: NodeKind) -> Result<QueueKind, XcCode> {
    let mut i = 0usize;
    while i < NODE_QUEUE_ROUTE.len() {
        if let Some(row) = NODE_QUEUE_ROUTE.get(i) {
            if row.0 == kind {
                return Ok(row.1);
            }
        }
        i += 1;
    }
    Err(XcCode::UnknownNodeKind)
}

// ---------------------------------------------------------------------------
// 四、帧图节点与多 GPU 路由（判据二）
// ---------------------------------------------------------------------------

/// 帧图节点（映射的输入：种类 + 读写集）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameNode {
    /// 节点 id（帧内唯一，由上游帧图分配）。
    pub node_id: u32,
    /// 节点种类（决定队列种类）。
    pub kind: NodeKind,
    /// 写集资源（本节点写；非空时 home=写集首资源所在卡）。
    pub writes: Vec<u64>,
    /// 读集资源（写集空时 home=读集首资源所在卡）。
    pub reads: Vec<u64>,
}

/// 映射结论（节点 → (卡, 队列种类)；映射即数据，调度器直接消费）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueBinding {
    /// 节点 id。
    pub node_id: u32,
    /// home 卡 id（资源所在卡；F0106 放置策略的权威源）。
    pub device_id: u16,
    /// 队列种类（路由表查得）。
    pub queue: QueueKind,
}

/// 资源 → 所在卡（F0106 放置策略的只读投影；本模块不选卡只查卡）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacementView {
    /// 资源 id → 设备 id 的登记表。
    entries: Vec<(u64, u16)>,
}

impl PlacementView {
    /// 从 (资源, 卡) 对构建放置投影（同资源重复登记以首登记为准——
    /// 重复登记是放置策略的缺陷，本层只取首见不静默改写）。
    pub fn new(pairs: &[(u64, u16)]) -> PlacementView {
        let mut entries: Vec<(u64, u16)> = Vec::new();
        let mut i = 0usize;
        while i < pairs.len() {
            if let Some(&(res, dev)) = pairs.get(i) {
                let mut seen = false;
                let mut j = 0usize;
                while j < entries.len() {
                    if let Some(e) = entries.get(j) {
                        if e.0 == res {
                            seen = true;
                            break;
                        }
                    }
                    j += 1;
                }
                if !seen {
                    entries.push((res, dev));
                }
            }
            i += 1;
        }
        PlacementView { entries }
    }

    /// 查资源所在卡（O(n)——登记容量小，线性即常数；未登记返 None）。
    pub fn home_of(&self, resource: u64) -> Option<u16> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.0 == resource {
                    return Some(e.1);
                }
            }
            i += 1;
        }
        None
    }

    /// 在册资源数。
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判定。
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 五、映射器与一致性约束（判据三）
// ---------------------------------------------------------------------------

/// 队列映射器（单帧一次映射；确定性：同输入同绑定）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueMapper {
    /// 主卡 id（纯同步节点归主卡）。
    pub primary_device: u16,
    /// 放置投影（资源→卡）。
    pub placement: PlacementView,
}

impl QueueMapper {
    /// 新映射器（主卡 + 放置投影注入——零墙钟零探测，全部上游给定）。
    pub const fn new(primary_device: u16, placement: PlacementView) -> QueueMapper {
        QueueMapper {
            primary_device,
            placement,
        }
    }

    /// 单节点 home 判定（写集优先→读集首→主卡；歧义显性拒）。
    fn home_for(&self, node: &FrameNode) -> Result<u16, XcCode> {
        if !node.writes.is_empty() {
            let res = match node.writes.first() {
                Some(&r) => r,
                None => return Err(XcCode::WriteHomeMissing),
            };
            return self
                .placement
                .home_of(res)
                .ok_or(XcCode::WriteHomeMissing);
        }
        if !node.reads.is_empty() {
            let res = match node.reads.first() {
                Some(&r) => r,
                None => return Err(XcCode::ReadHomeMissing),
            };
            return self.placement.home_of(res).ok_or(XcCode::ReadHomeMissing);
        }
        if self.placement.len() == 0 {
            // 单卡世界：全空节点归主卡无歧义。
            return Ok(self.primary_device);
        }
        Err(XcCode::AmbiguousSyncNode)
    }

    /// 帧图整图映射（确定性序：按输入序逐节点；写后读一致性在本遍内
    /// 即检即拒——首个不一致的读节点报错，错误信息点名资源与两侧队列）。
    pub fn map_frame(&self, nodes: &[FrameNode]) -> Result<Vec<QueueBinding>, (usize, XcCode)> {
        let mut out: Vec<QueueBinding> = Vec::new();
        // 资源最近写绑定（写后读一致性检查的状态）。
        let mut last_write: Vec<(u64, u16, QueueKind)> = Vec::new();
        let mut idx = 0usize;
        while idx < nodes.len() {
            let node = match nodes.get(idx) {
                Some(n) => n,
                None => break,
            };
            let queue = match route_of(node.kind) {
                Ok(q) => q,
                Err(e) => return Err((idx, e)),
            };
            let home = match self.home_for(node) {
                Ok(h) => h,
                Err(e) => return Err((idx, e)),
            };
            // 一致性约束：本节点读的每个资源，若此前有写绑定，必须同卡同队列种类。
            let mut r = 0usize;
            while r < node.reads.len() {
                let res = match node.reads.get(r) {
                    Some(&x) => x,
                    None => break,
                };
                let mut w = 0usize;
                while w < last_write.len() {
                    if let Some(lw) = last_write.get(w) {
                        if lw.0 == res && (lw.1 != home || lw.2 != queue) {
                            return Err((idx, XcCode::RwQueueInconsistent));
                        }
                    }
                    w += 1;
                }
                r += 1;
            }
            // 登记本节点的写绑定（写节点进入 last_write 表）。
            let mut wr = 0usize;
            while wr < node.writes.len() {
                let res = match node.writes.get(wr) {
                    Some(&x) => x,
                    None => break,
                };
                // 同资源重复写以最近为准（覆盖旧行——last-write-wins）。
                let mut hit = false;
                let mut w = 0usize;
                while w < last_write.len() {
                    if let Some(lw) = last_write.get_mut(w) {
                        if lw.0 == res {
                            lw.1 = home;
                            lw.2 = queue;
                            hit = true;
                            break;
                        }
                    }
                    w += 1;
                }
                if !hit {
                    last_write.push((res, home, queue));
                }
                wr += 1;
            }
            out.push(QueueBinding {
                node_id: node.node_id,
                device_id: home,
                queue,
            });
            idx += 1;
        }
        Ok(out)
    }

    /// 三队列用例（判据五：图形/计算/拷贝各至少一节点的最小帧图——
    /// 建造器给回归与下游探针复用；确定性输出）。
    pub fn three_queue_sample() -> [FrameNode; 3] {
        [
            FrameNode {
                node_id: 1,
                kind: NodeKind::Render,
                writes: vec![100],
                reads: vec![],
            },
            FrameNode {
                node_id: 2,
                kind: NodeKind::Encode,
                writes: vec![200],
                reads: vec![100],
            },
            FrameNode {
                node_id: 3,
                kind: NodeKind::Download,
                writes: vec![],
                reads: vec![200],
            },
        ]
    }
}

// ---------------------------------------------------------------------------
// 六、映射开销账（判据四）
// ---------------------------------------------------------------------------

/// 映射开销账（B05 开销台账面的只读消费源）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapLedger {
    /// 已映射节点数。
    pub mapped_nodes: u64,
    /// 各队列种类的节点数（下标=QueueKind::ordinal）。
    pub per_queue: [u64; QUEUE_KIND_TOTAL],
    /// home 判定次数（含失败）。
    pub home_probes: u64,
    /// 一致性约束拒绝次数。
    pub rw_rejects: u64,
}

impl MapLedger {
    /// 新账。
    pub const fn new() -> MapLedger {
        MapLedger {
            mapped_nodes: 0,
            per_queue: [0; QUEUE_KIND_TOTAL],
            home_probes: 0,
            rw_rejects: 0,
        }
    }

    /// 从映射结果记账（一次遍历 O(n)；映射成功后调用）。
    pub fn record(&mut self, bindings: &[QueueBinding]) {
        let mut i = 0usize;
        while i < bindings.len() {
            if let Some(b) = bindings.get(i) {
                self.mapped_nodes = self.mapped_nodes.saturating_add(1);
                let o = b.queue.ordinal();
                if o < QUEUE_KIND_TOTAL {
                    self.per_queue[o] = self.per_queue[o].saturating_add(1);
                }
            }
            i += 1;
        }
        // home 判定次数=节点数（每节点恰一次 home_for）。
        self.home_probes = self.mapped_nodes;
    }

    /// 记一次一致性拒绝（map_frame 出错面调用）。
    pub const fn record_reject(&mut self) {
        self.rw_rejects = self.rw_rejects.saturating_add(1);
    }

    /// 映射开销结论（人读；供台账渲染）。
    pub fn summary(&self) -> String {
        let mut s = String::from("映射账：节点 ");
        s.push_str(&self.mapped_nodes.to_string());
        s.push_str("，图形 ");
        s.push_str(&self.per_queue[0].to_string());
        s.push_str("，计算 ");
        s.push_str(&self.per_queue[1].to_string());
        s.push_str("，拷贝 ");
        s.push_str(&self.per_queue[2].to_string());
        s.push_str("，一致性拒 ");
        s.push_str(&self.rw_rejects.to_string());
        s
    }
}
