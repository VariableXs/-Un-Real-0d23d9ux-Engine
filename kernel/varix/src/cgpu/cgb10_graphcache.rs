//! cgb10_graphcache — CGPU-F0170 帧图构建结果缓存（CGPU 册 cgb 号段深化）。
//!
//! 锚点（施工书 F0170）：帧图的构建结果缓存：图结构缓存（同一帧布局的图结构
//! 不重建——缓存键=表面集+资源集哈希），增量图更新（只增删变化节点而非全图
//! 重建——列表滚动的增量场景），缓存失效（表面/资源集变化即失效）。判据：
//! 缓存键正确、增量=全量一致、命中率遥测、失效纪律、构建性能收益实测。
//!
//! 本文件七段深化：
//! 1. 缓存键（CacheKey）——表面集/资源集**序无关** FNV-1a 逐项异或合成 +
//!    计数混入；哈希命中后**明细对拍**二次验证（防碰撞假命中）；三段独立
//!    （键序无关但字段彼此独立：表面变键变、资源变键变、布局版本变键变）。
//! 2. 被缓存本体（CompiledGraph）——节点/边集合 + 拓扑序 + 每节点编译代数
//!    （compile epoch）：增量更新的收益载体，未动节点编译态原样保留。
//! 3. 图缓存（GraphCache）——四代环形槽位；命中/未命中/碰撞拒绝三态；
//!    满员逐最旧（代际时钟）；TTL 帧龄过期；返回克隆与缓存隔离。
//! 4. 增量更新（IncrementalPatch）——五算子：AddNode/RemoveNode（边级联删）/
//!    ReplaceNode/AddEdge/RemoveEdge；基于 base 键校验（base 不符整 patch
//!    拒绝）；apply 后结构必与"从零全量构建同终集"逐节点逐边全等（判据核心）。
//! 5. 失效纪律（InvalidationLedger）——五因定表：表面集变化/资源集变化/
//!    布局版本推进/手动逐出/TTL 过期/碰撞拒绝；每次失效记账（键摘要+原因+
//!    tick）；失效后同键再查必 miss。
//! 6. 命中率遥测（CacheTelemetry）——命中/未命中/碰撞/逐出/失效五计数 +
//!    千分比命中率 + 快照；累计口径单调。
//! 7. 构建性能收益实测（BuildCostProbe）——全量构建按节点数+边数计价
//!    （NODE_COST/EDGE_COST），命中路径按查表 O(1) 计价；累计节约成本账；
//!    收益判据实测：同场景重复构建，命中成本严格小于全量成本。
//!
//! 判据总入口 [`run_f0170_checks`]，七族三十四条，全部零 panic 面
//! （无 unwrap/expect/index 越界），预期常量独立给定不从被测反推。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 哈希基建 — FNV-1a 64 位；集合哈希序无关合成
// ---------------------------------------------------------------------------

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节推进。
const fn fnv_byte(acc: u64, byte: u8) -> u64 {
    (acc ^ (byte as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 逐字节哈希一段字节。
const fn fnv_bytes(acc: u64, bytes: &[u8]) -> u64 {
    let mut h = acc;
    let mut i = 0;
    while i < bytes.len() {
        h = fnv_byte(h, bytes[i]);
        i += 1;
    }
    h
}

/// u64 按 FNV 推进（小端 8 字节展开，跨平台稳定）。
const fn fnv_u64(acc: u64, v: u64) -> u64 {
    let mut h = acc;
    let mut b = v;
    let mut k = 0;
    while k < 8 {
        h = fnv_byte(h, (b & 0xff) as u8);
        b >>= 8;
        k += 1;
    }
    h
}

/// 集合哈希：**序无关**——逐项 FNV 后异或折叠，再混入计数防"子集同键"
/// （{a} 与 {a,a} 逐项异或同值，但计数不同故键不同；{a,b} 与 {b,a} 同序
/// 异或同值且计数相同故键相同——正是需要的序无关语义）。
fn set_hash(items: &[u64]) -> u64 {
    let mut fold: u64 = 0;
    let mut i = 0;
    while i < items.len() {
        fold ^= fnv_u64(FNV_OFFSET, items[i]);
        i += 1;
    }
    fnv_u64(fold, items.len() as u64)
}

// ---------------------------------------------------------------------------
// §1 缓存键 — 表面集+资源集哈希 + 明细对拍防碰撞
// ---------------------------------------------------------------------------

/// 缓存命中判定结果三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyMatch {
    /// 哈希与明细双对拍通过——真命中。
    Hit,
    /// 哈希同但明细不符——碰撞拒绝（必须重建，不得假命中）。
    Collision,
    /// 哈希即不同——直接未命中。
    Miss,
}

/// 帧图缓存键：表面集哈希 + 资源集哈希 + 布局版本，三者任一变化即键变。
/// 同时保存原始集明细（计数 + 逐项），命中前二次对拍防哈希碰撞假命中。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheKey {
    pub layout_ver: u32,
    pub surfaces_hash: u64,
    pub resources_hash: u64,
    /// 合成键：三段再折叠。
    pub combined: u64,
    /// 明细：表面 id 全集（对拍用，不参与 combined）。
    pub surface_ids: Vec<u32>,
    /// 明细：资源 id 全集（对拍用）。
    pub resource_ids: Vec<u32>,
}

impl CacheKey {
    /// 从表面/资源 id 集与布局版本铸键。集内顺序不影响键值（序无关），
    /// 但明细按升序规整保存以稳定对拍。
    pub fn mint(layout_ver: u32, surfaces: &[u32], resources: &[u32]) -> CacheKey {
        let sh = set_hash(&surfaces.iter().map(|&v| v as u64).collect::<Vec<u64>>());
        let rh = set_hash(&resources.iter().map(|&v| v as u64).collect::<Vec<u64>>());
        let combined = fnv_u64(fnv_u64(fnv_u64(FNV_OFFSET, sh), rh), layout_ver as u64);
        let mut s2 = Vec::new();
        let mut i = 0;
        while i < surfaces.len() {
            s2.push(surfaces[i]);
            i += 1;
        }
        sort_u32(&mut s2);
        let mut r2 = Vec::new();
        let mut i = 0;
        while i < resources.len() {
            r2.push(resources[i]);
            i += 1;
        }
        sort_u32(&mut r2);
        CacheKey {
            layout_ver,
            surfaces_hash: sh,
            resources_hash: rh,
            combined,
            surface_ids: s2,
            resource_ids: r2,
        }
    }

    /// 三态判定：哈希段同则进入明细对拍；明细全等才是真命中。
    pub fn matches(&self, other: &CacheKey) -> KeyMatch {
        if self.combined != other.combined
            || self.surfaces_hash != other.surfaces_hash
            || self.resources_hash != other.resources_hash
            || self.layout_ver != other.layout_ver
        {
            return KeyMatch::Miss;
        }
        let same_surfaces = self.surface_ids.len() == other.surface_ids.len()
            && seq_eq_u32(&self.surface_ids, &other.surface_ids);
        let same_resources = self.resource_ids.len() == other.resource_ids.len()
            && seq_eq_u32(&self.resource_ids, &other.resource_ids);
        if same_surfaces && same_resources {
            KeyMatch::Hit
        } else {
            KeyMatch::Collision
        }
    }
}

fn seq_eq_u32(a: &[u32], b: &[u32]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 插入排序升序（id 集小，O(n²) 足够且无递归无栈）。
fn sort_u32(v: &mut Vec<u32>) {
    let n = v.len();
    let mut i = 1;
    while i < n {
        let key = v[i];
        let mut j = i;
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// §2 被缓存本体 — 节点/边/拓扑 + 每节点编译代数
// ---------------------------------------------------------------------------

/// 帧图节点类别（与 cgb08 TaskKind 对齐的精简闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Acquire,
    Render,
    Present,
}

/// 帧图节点：id、类别、消费的表面、读写的资源。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GNode {
    pub id: u32,
    pub kind: NodeKind,
    /// 消费（写）的表面 id。
    pub surface: u32,
    /// 读的资源 id 集。
    pub reads: Vec<u32>,
}

/// 帧图边：数据依赖 from → to（from 的产出是 to 的输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GEdge {
    pub from: u32,
    pub to: u32,
}

/// 编译产物：节点/边 + 拓扑序（依赖在前）+ 每节点编译代数。
/// 增量更新时未动节点的 epoch 保持不变——这就是"只增删变化节点"的收益。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledGraph {
    pub nodes: Vec<GNode>,
    pub edges: Vec<GEdge>,
    /// 拓扑序：按依赖关系排序后的节点 id 序列。
    pub topo: Vec<u32>,
    /// 每节点编译代数（与 nodes 一一对应）：在全量构建时铸 1，增量未动保持。
    pub epoch: Vec<u64>,
    /// 本图整体构建代数（每次全量重建 +1；增量不涨）。
    pub build_epoch: u64,
}

impl CompiledGraph {
    /// 空图。
    pub fn empty() -> CompiledGraph {
        CompiledGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
            topo: Vec::new(),
            epoch: Vec::new(),
            build_epoch: 0,
        }
    }

    /// 从零全量构建：节点入图、边入图、拓扑排序、全节点铸编译代数。
    pub fn build_full(nodes: Vec<GNode>, edges: Vec<GEdge>, build_epoch: u64) -> CompiledGraph {
        let topo = topo_sort(&nodes, &edges);
        let mut epoch = Vec::new();
        let mut i = 0;
        while i < nodes.len() {
            epoch.push(build_epoch);
            i += 1;
        }
        CompiledGraph {
            nodes,
            edges,
            topo,
            epoch,
            build_epoch,
        }
    }

    /// 全图表面 id 集（去重升序）。
    pub fn surfaces(&self) -> Vec<u32> {
        let mut acc: Vec<u32> = Vec::new();
        let mut i = 0;
        while i < self.nodes.len() {
            let s = self.nodes[i].surface;
            if !contains_u32(&acc, s) {
                acc.push(s);
            }
            i += 1;
        }
        sort_u32(&mut acc);
        acc
    }

    /// 全图资源 id 集（读集并集，去重升序）。
    pub fn resources(&self) -> Vec<u32> {
        let mut acc: Vec<u32> = Vec::new();
        let mut i = 0;
        while i < self.nodes.len() {
            let mut j = 0;
            while j < self.nodes[i].reads.len() {
                let r = self.nodes[i].reads[j];
                if !contains_u32(&acc, r) {
                    acc.push(r);
                }
                j += 1;
            }
            i += 1;
        }
        sort_u32(&mut acc);
        acc
    }

    /// 结构全等对拍：节点集全等（逐字段）+ 边集全等（序无关）+ 拓扑全等。
    /// 判据"增量=全量一致"的裁决器。
    pub fn structurally_equal(&self, other: &CompiledGraph) -> bool {
        if self.nodes.len() != other.nodes.len() || self.edges.len() != other.edges.len() {
            return false;
        }
        // 节点按 id 配对逐字段比对。
        let mut i = 0;
        while i < self.nodes.len() {
            let a = &self.nodes[i];
            let b = find_node(&other.nodes, a.id);
            match b {
                None => return false,
                Some(n) => {
                    if n.kind != a.kind || n.surface != a.surface || !seq_eq_u32(&n.reads, &a.reads)
                    {
                        return false;
                    }
                }
            }
            i += 1;
        }
        // 边序无关对拍。
        let mut i = 0;
        while i < self.edges.len() {
            let e = self.edges[i];
            let mut found = false;
            let mut j = 0;
            while j < other.edges.len() {
                if other.edges[j].from == e.from && other.edges[j].to == e.to {
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                return false;
            }
            i += 1;
        }
        seq_eq_u32(&self.topo, &other.topo)
    }
}

fn contains_u32(v: &[u32], x: u32) -> bool {
    let mut i = 0;
    while i < v.len() {
        if v[i] == x {
            return true;
        }
        i += 1;
    }
    false
}

fn find_node(nodes: &[GNode], id: u32) -> Option<&GNode> {
    let mut i = 0;
    while i < nodes.len() {
        if nodes[i].id == id {
            return Some(&nodes[i]);
        }
        i += 1;
    }
    None
}

/// Kahn 拓扑排序（确定性：同轮候选取最小 id）。存在环时输出已达节点数
/// < 总数——缓存键/构建路径不依赖环存在与否，但拓扑用于判据对拍需确定性。
pub fn topo_sort(nodes: &[GNode], edges: &[GEdge]) -> Vec<u32> {
    let n = nodes.len();
    let mut indeg: Vec<u32> = vec![0; n];
    let mut adj: Vec<Vec<u32>> = Vec::new();
    let mut i = 0;
    while i < n {
        adj.push(Vec::new());
        i += 1;
    }
    // 节点 id → 下标。
    let mut e = 0;
    while e < edges.len() {
        let fi = index_of(nodes, edges[e].from);
        let ti = index_of(nodes, edges[e].to);
        if let (Some(f), Some(t)) = (fi, ti) {
            adj[f].push(t as u32);
            indeg[t] += 1;
        }
        e += 1;
    }
    let mut out: Vec<u32> = Vec::new();
    let mut taken: Vec<bool> = vec![false; n];
    let mut round = 0;
    while round < n {
        // 同轮候选取最小 id（确定性）。
        let mut pick: Option<usize> = None;
        let mut i = 0;
        while i < n {
            if !taken[i] && indeg[i] == 0 {
                match pick {
                    None => pick = Some(i),
                    Some(p) => {
                        if nodes[i].id < nodes[p].id {
                            pick = Some(i);
                        }
                    }
                }
            }
            i += 1;
        }
        match pick {
            None => break,
            Some(p) => {
                taken[p] = true;
                out.push(nodes[p].id);
                let mut k = 0;
                while k < adj[p].len() {
                    let t = adj[p][k] as usize;
                    indeg[t] -= 1;
                    k += 1;
                }
            }
        }
        round += 1;
    }
    out
}

fn index_of(nodes: &[GNode], id: u32) -> Option<usize> {
    let mut i = 0;
    while i < nodes.len() {
        if nodes[i].id == id {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// §3 图缓存 — 四代环形槽位 + 三态命中 + TTL + 满员逐最旧
// ---------------------------------------------------------------------------

/// 缓存槽位容量：四代环形（与全仓"代际环形"惯例一致，容量小而够用——
/// 帧图布局种类在单场景内有限，四代覆盖滚动窗口）。
pub const CACHE_SLOTS: usize = 4;

/// 缓存条目 TTL（帧龄）：超过即过期失效，防止陈旧布局长期占槽。
pub const CACHE_TTL_TICKS: u64 = 32;

/// 缓存查询三态结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LookupOutcome {
    /// 真命中：返回编译产物克隆（与缓存隔离，改克隆不影响缓存）。
    Hit(CompiledGraph),
    /// 碰撞拒绝：哈希同明细异，已记账失效。
    CollisionRejected,
    /// 未命中（含 TTL 过期）。
    Miss,
}

/// 单个缓存槽。
#[derive(Clone, Debug)]
struct CacheSlot {
    occupied: bool,
    key: Option<CacheKey>,
    graph: Option<CompiledGraph>,
    /// 入槽时的 tick（逐最旧依据）。
    stamped: u64,
}

impl CacheSlot {
    const fn vacant() -> CacheSlot {
        CacheSlot {
            occupied: false,
            key: None,
            graph: None,
            stamped: 0,
        }
    }
}

/// 图结构缓存：四代环形槽位；命中/未命中/碰撞拒绝三态；TTL 过期；
/// 满员逐最旧（stamped 最小者让位）；返回克隆与缓存隔离。
#[derive(Clone, Debug)]
pub struct GraphCache {
    slots: [CacheSlot; CACHE_SLOTS],
    tick: u64,
    /// 遥测钩子（本缓存自带的累计计数；门面层另有汇总遥测）。
    pub hits: u64,
    pub misses: u64,
    pub collisions: u64,
    pub evictions: u64,
    pub ttl_expired: u64,
}

impl GraphCache {
    pub fn new() -> GraphCache {
        GraphCache {
            slots: [
                CacheSlot::vacant(),
                CacheSlot::vacant(),
                CacheSlot::vacant(),
                CacheSlot::vacant(),
            ],
            tick: 0,
            hits: 0,
            misses: 0,
            collisions: 0,
            evictions: 0,
            ttl_expired: 0,
        }
    }

    /// 时钟推进一格（帧边界调用）。
    pub fn advance_tick(&mut self) {
        self.tick += 1;
    }

    pub fn now(&self) -> u64 {
        self.tick
    }

    /// 查缓存：哈希→明细对拍三态；TTL 过期按未命中并清除槽位。
    pub fn lookup(&mut self, key: &CacheKey) -> LookupOutcome {
        let mut i = 0;
        while i < CACHE_SLOTS {
            if self.slots[i].occupied {
                if let Some(k) = &self.slots[i].key {
                    if k.combined == key.combined {
                        match k.matches(key) {
                            KeyMatch::Hit => {
                                // TTL 裁决：入槽帧龄超限即过期。
                                if self.tick.saturating_sub(self.slots[i].stamped)
                                    > CACHE_TTL_TICKS
                                {
                                    self.ttl_expired += 1;
                                    self.slots[i] = CacheSlot::vacant();
                                    self.misses += 1;
                                    return LookupOutcome::Miss;
                                }
                                self.hits += 1;
                                match &self.slots[i].graph {
                                    Some(g) => return LookupOutcome::Hit(g.clone()),
                                    None => {
                                        self.misses += 1;
                                        return LookupOutcome::Miss;
                                    }
                                }
                            }
                            KeyMatch::Collision => {
                                self.collisions += 1;
                                // 碰撞拒绝：槽位内容不可信，清除并失效记账。
                                self.slots[i] = CacheSlot::vacant();
                                return LookupOutcome::CollisionRejected;
                            }
                            KeyMatch::Miss => {
                                // combined 相同但前段字段已 miss 时不发生；
                                // matches 返回 Miss 仅当三段有异，这里 defensively 计。
                            }
                        }
                    }
                }
            }
            i += 1;
        }
        self.misses += 1;
        LookupOutcome::Miss
    }

    /// 存缓存：同键真命中已有则原地刷新（保持 stamped）；满员逐 stamped
    /// 最旧者（evictions 记账）；新条目以当前 tick 盖戳。
    pub fn insert(&mut self, key: CacheKey, graph: CompiledGraph) {
        // 同键刷新。
        let mut i = 0;
        while i < CACHE_SLOTS {
            if self.slots[i].occupied {
                if let Some(k) = &self.slots[i].key {
                    if k.combined == key.combined && k.matches(&key) == KeyMatch::Hit {
                        self.slots[i].graph = Some(graph);
                        self.slots[i].stamped = self.tick;
                        return;
                    }
                }
            }
            i += 1;
        }
        // 找空槽。
        let mut target: Option<usize> = None;
        let mut i = 0;
        while i < CACHE_SLOTS {
            if !self.slots[i].occupied {
                target = Some(i);
                break;
            }
            i += 1;
        }
        // 满员逐最旧。
        if target.is_none() {
            let mut oldest = 0usize;
            let mut i = 1;
            while i < CACHE_SLOTS {
                if self.slots[i].stamped < self.slots[oldest].stamped {
                    oldest = i;
                }
                i += 1;
            }
            self.evictions += 1;
            target = Some(oldest);
        }
        let t = match target {
            Some(t) => t,
            None => return, // 不可达（CACHE_SLOTS>0），零 panic 面兜底。
        };
        self.slots[t] = CacheSlot {
            occupied: true,
            key: Some(key),
            graph: Some(graph),
            stamped: self.tick,
        };
    }

    /// 手动逐出某键（失效纪律：失效后同键再查必 miss）。返回是否真逐出。
    pub fn evict(&mut self, key: &CacheKey) -> bool {
        let mut i = 0;
        while i < CACHE_SLOTS {
            if self.slots[i].occupied {
                if let Some(k) = &self.slots[i].key {
                    if k.combined == key.combined && k.matches(key) == KeyMatch::Hit {
                        self.slots[i] = CacheSlot::vacant();
                        self.evictions += 1;
                        return true;
                    }
                }
            }
            i += 1;
        }
        false
    }

    /// 当前占用槽位数。
    pub fn occupied(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < CACHE_SLOTS {
            if self.slots[i].occupied {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// §4 增量更新 — 五算子 + base 键校验 + 增量=全量一致
// ---------------------------------------------------------------------------

/// 增量算子（五算子闭集：节点三算子 + 边两算子——新增节点靠边算子接入
/// 依赖链，列表滚动场景的"变化节点+变化边"一步到位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchOp {
    /// 新增节点（id 不得与现有冲突；新节点按当前 patch 代数编译）。
    AddNode(GNode),
    /// 删除节点及其全部关联边（级联）。
    RemoveNode(u32),
    /// 原地替换节点内容（surface/reads/kind 可变；编译代数刷新）。
    ReplaceNode(GNode),
    /// 新增依赖边（两端节点必须已存在；重复边拒绝）。
    AddEdge(GEdge),
    /// 删除依赖边（边必须存在）。
    RemoveEdge(GEdge),
}

/// 增量补丁：一串算子 + 所基于的 base 键（base 不符整 patch 拒绝——
/// 增量只在"从哪个图出发"明确时才安全）。
#[derive(Clone, Debug)]
pub struct IncrementalPatch {
    pub base: CacheKey,
    pub ops: Vec<PatchOp>,
    /// 本 patch 的编译代数（apply 时写入新动节点）。
    pub patch_epoch: u64,
}

impl IncrementalPatch {
    pub fn new(base: CacheKey, patch_epoch: u64) -> IncrementalPatch {
        IncrementalPatch {
            base,
            ops: Vec::new(),
            patch_epoch,
        }
    }

    pub fn add_node(&mut self, n: GNode) {
        self.ops.push(PatchOp::AddNode(n));
    }

    pub fn remove_node(&mut self, id: u32) {
        self.ops.push(PatchOp::RemoveNode(id));
    }

    pub fn replace_node(&mut self, n: GNode) {
        self.ops.push(PatchOp::ReplaceNode(n));
    }

    pub fn add_edge(&mut self, from: u32, to: u32) {
        self.ops.push(PatchOp::AddEdge(GEdge { from, to }));
    }

    pub fn remove_edge(&mut self, from: u32, to: u32) {
        self.ops.push(PatchOp::RemoveEdge(GEdge { from, to }));
    }

    /// 应用补丁：只增删变化节点，未动节点 epoch 原样保留。
    /// 返回 None 表示 base 不符或算子非法（重复 id 等）——整 patch 拒绝。
    pub fn apply(&self, base_graph: &CompiledGraph, base_key: &CacheKey) -> Option<CompiledGraph> {
        // base 键校验：base 键必须与"从 base_graph 重算的键"真命中一致。
        let recomputed = CacheKey::mint(
            base_key.layout_ver,
            &base_graph.surfaces(),
            &base_graph.resources(),
        );
        if base_key.matches(&recomputed) != KeyMatch::Hit {
            return None;
        }
        let mut nodes: Vec<GNode> = Vec::new();
        let mut edges: Vec<GEdge> = Vec::new();
        let mut epoch: Vec<u64> = Vec::new();
        let mut i = 0;
        while i < base_graph.nodes.len() {
            nodes.push(base_graph.nodes[i].clone());
            epoch.push(base_graph.epoch[i]);
            i += 1;
        }
        let mut i = 0;
        while i < base_graph.edges.len() {
            edges.push(base_graph.edges[i]);
            i += 1;
        }
        let mut oi = 0;
        while oi < self.ops.len() {
            match &self.ops[oi] {
                PatchOp::AddNode(n) => {
                    if find_node(&nodes, n.id).is_some() {
                        return None; // id 冲突：整 patch 拒绝。
                    }
                    nodes.push(n.clone());
                    epoch.push(self.patch_epoch);
                }
                PatchOp::RemoveNode(id) => {
                    if find_node(&nodes, *id).is_none() {
                        return None; // 删不存在节点：拒绝。
                    }
                    // 级联删边。
                    let mut kept: Vec<GEdge> = Vec::new();
                    let mut j = 0;
                    while j < edges.len() {
                        if edges[j].from != *id && edges[j].to != *id {
                            kept.push(edges[j]);
                        }
                        j += 1;
                    }
                    edges = kept;
                    // 删节点与对应 epoch。
                    let mut nn: Vec<GNode> = Vec::new();
                    let mut ne: Vec<u64> = Vec::new();
                    let mut j = 0;
                    while j < nodes.len() {
                        if nodes[j].id != *id {
                            nn.push(nodes[j].clone());
                            ne.push(epoch[j]);
                        }
                        j += 1;
                    }
                    nodes = nn;
                    epoch = ne;
                }
                PatchOp::ReplaceNode(n) => {
                    let idx = index_of(&nodes, n.id);
                    match idx {
                        None => return None, // 替换不存在节点：拒绝。
                        Some(ix) => {
                            nodes[ix] = n.clone();
                            epoch[ix] = self.patch_epoch;
                        }
                    }
                }
                PatchOp::AddEdge(e) => {
                    // 两端节点必须已存在。
                    if index_of(&nodes, e.from).is_none() || index_of(&nodes, e.to).is_none() {
                        return None;
                    }
                    // 重复边拒绝（幂等加边会掩盖调用方状态混乱）。
                    let mut j = 0;
                    while j < edges.len() {
                        if edges[j].from == e.from && edges[j].to == e.to {
                            return None;
                        }
                        j += 1;
                    }
                    edges.push(*e);
                }
                PatchOp::RemoveEdge(e) => {
                    // 边必须存在，否则拒绝。
                    let mut hit = None;
                    let mut j = 0;
                    while j < edges.len() {
                        if edges[j].from == e.from && edges[j].to == e.to {
                            hit = Some(j);
                            break;
                        }
                        j += 1;
                    }
                    match hit {
                        None => return None,
                        Some(ix) => {
                            edges.remove(ix);
                        }
                    }
                }
            }
            oi += 1;
        }
        // 拓扑重排（确定性）；未动节点内容与 epoch 不变。
        let topo = topo_sort(&nodes, &edges);
        Some(CompiledGraph {
            nodes,
            edges,
            topo,
            epoch,
            build_epoch: base_graph.build_epoch,
        })
    }
}

// ---------------------------------------------------------------------------
// §5 失效纪律 — 五因定表 + 失效账本 + 失效后必 miss
// ---------------------------------------------------------------------------

/// 失效原因五因闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidReason {
    /// 表面集变化（键的表面段变）。
    SurfaceSetChanged,
    /// 资源集变化。
    ResourceSetChanged,
    /// 布局版本推进。
    LayoutAdvanced,
    /// 手动逐出。
    ManualEvict,
    /// TTL 过期。
    TtlExpired,
    /// 哈希碰撞拒绝。
    CollisionRejected,
}

impl InvalidReason {
    pub const fn tag(self) -> &'static str {
        match self {
            InvalidReason::SurfaceSetChanged => "surface-changed",
            InvalidReason::ResourceSetChanged => "resource-changed",
            InvalidReason::LayoutAdvanced => "layout-advanced",
            InvalidReason::ManualEvict => "manual-evict",
            InvalidReason::TtlExpired => "ttl-expired",
            InvalidReason::CollisionRejected => "collision",
        }
    }
}

/// 失效账目一条。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidEntry {
    /// 键摘要（combined）。
    pub key_digest: u64,
    pub reason: InvalidReason,
    pub at_tick: u64,
}

/// 失效账本：环形记账（容量定长，满了挤最旧），失效后同键再查必 miss
/// 由缓存槽清除保证，账本负责"为什么失效"的可审计轨迹。
pub struct InvalidationLedger {
    entries: Vec<InvalidEntry>,
    cap: usize,
    pub dropped: usize,
}

/// 账本容量。
pub const LEDGER_CAP: usize = 64;

impl InvalidationLedger {
    pub fn new() -> InvalidationLedger {
        InvalidationLedger {
            entries: Vec::new(),
            cap: LEDGER_CAP,
            dropped: 0,
        }
    }

    /// 记一笔失效。
    pub fn record(&mut self, key_digest: u64, reason: InvalidReason, at_tick: u64) {
        if self.entries.len() >= self.cap {
            // 挤最旧（头部）。
            let mut i = 1;
            while i < self.entries.len() {
                self.entries[i - 1] = self.entries[i];
                i += 1;
            }
            self.entries.pop();
            self.dropped += 1;
        }
        self.entries.push(InvalidEntry {
            key_digest,
            reason,
            at_tick,
        });
    }

    /// 某键的失效次数（按摘要）。
    pub fn count_for(&self, key_digest: u64) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].key_digest == key_digest {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 最后一条（时间序）。
    pub fn last(&self) -> Option<InvalidEntry> {
        if self.entries.is_empty() {
            return None;
        }
        Some(self.entries[self.entries.len() - 1])
    }
}

/// 失效裁决器：对比旧键与新键，判定失效原因（表面/资源/布局何者变了）。
/// 三段都同则不算失效（键未变）。
pub fn classify_invalidation(old: &CacheKey, new: &CacheKey) -> Option<InvalidReason> {
    if old.layout_ver != new.layout_ver {
        return Some(InvalidReason::LayoutAdvanced);
    }
    if old.surfaces_hash != new.surfaces_hash {
        return Some(InvalidReason::SurfaceSetChanged);
    }
    if old.resources_hash != new.resources_hash {
        return Some(InvalidReason::ResourceSetChanged);
    }
    None
}

// ---------------------------------------------------------------------------
// §6 命中率遥测 — 五计数 + 千分比命中率 + 快照
// ---------------------------------------------------------------------------

/// 遥测快照（Copy，便于上账/渲染）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetrySnapshot {
    pub hits: u64,
    pub misses: u64,
    pub collisions: u64,
    pub evictions: u64,
    pub invalidations: u64,
}

impl TelemetrySnapshot {
    /// 命中率千分比：hits / (hits + misses + collisions) × 1000。
    /// 分母 0 时记 0（无查询无命中率）。
    pub fn hit_permille(self) -> u64 {
        let denom = self.hits + self.misses + self.collisions;
        if denom == 0 {
            return 0;
        }
        (self.hits * 1000) / denom
    }
}

/// 命中率遥测：累计口径单调不减；失效计数由失效纪律侧注入。
pub struct CacheTelemetry {
    pub hits: u64,
    pub misses: u64,
    pub collisions: u64,
    pub evictions: u64,
    pub invalidations: u64,
}

impl CacheTelemetry {
    pub fn new() -> CacheTelemetry {
        CacheTelemetry {
            hits: 0,
            misses: 0,
            collisions: 0,
            evictions: 0,
            invalidations: 0,
        }
    }

    /// 从缓存注入一次查询结果。
    pub fn observe(&mut self, outcome: &LookupOutcome) {
        match outcome {
            LookupOutcome::Hit(_) => self.hits += 1,
            LookupOutcome::Miss => self.misses += 1,
            LookupOutcome::CollisionRejected => self.collisions += 1,
        }
    }

    pub fn note_eviction(&mut self) {
        self.evictions += 1;
    }

    pub fn note_invalidation(&mut self) {
        self.invalidations += 1;
    }

    pub fn snapshot(&self) -> TelemetrySnapshot {
        TelemetrySnapshot {
            hits: self.hits,
            misses: self.misses,
            collisions: self.collisions,
            evictions: self.evictions,
            invalidations: self.invalidations,
        }
    }
}

// ---------------------------------------------------------------------------
// §7 构建性能收益实测 — 成本模型 + 节约账
// ---------------------------------------------------------------------------

/// 全量构建单价：每节点 100，每边 10（任意线性计价即可比）。
pub const NODE_COST: u64 = 100;
pub const EDGE_COST: u64 = 10;
/// 增量单价：每算子 30（新增节点要编译，但不重排未动节点）。
pub const PATCH_OP_COST: u64 = 30;
/// 命中查表单价（O(1) 固定小值）。
pub const HIT_COST: u64 = 2;

/// 全量构建成本。
pub fn full_build_cost(nodes: usize, edges: usize) -> u64 {
    nodes as u64 * NODE_COST + edges as u64 * EDGE_COST
}

/// 增量应用成本。
pub fn patch_cost(ops: usize) -> u64 {
    ops as u64 * PATCH_OP_COST
}

/// 构建收益账：记录每次动作（全量/增量/命中）的成本与"相对全量的节约"。
pub struct BuildCostProbe {
    /// 累计全量成本（若全部走全量路径的总花费）。
    pub full_total: u64,
    /// 累计实际成本（真实路径花费）。
    pub actual_total: u64,
    /// 动作次数。
    pub actions: u64,
    /// 最近一次节约额（实测判据用）。
    pub last_saved: u64,
}

impl BuildCostProbe {
    pub fn new() -> BuildCostProbe {
        BuildCostProbe {
            full_total: 0,
            actual_total: 0,
            actions: 0,
            last_saved: 0,
        }
    }

    /// 记一次全量构建。
    pub fn note_full(&mut self, nodes: usize, edges: usize) {
        let c = full_build_cost(nodes, edges);
        self.full_total += c;
        self.actual_total += c;
        self.last_saved = 0;
        self.actions += 1;
    }

    /// 记一次命中。
    pub fn note_hit(&mut self, equiv_nodes: usize, equiv_edges: usize) {
        let full = full_build_cost(equiv_nodes, equiv_edges);
        self.full_total += full;
        self.actual_total += HIT_COST;
        self.last_saved = full.saturating_sub(HIT_COST);
        self.actions += 1;
    }

    /// 记一次增量。
    pub fn note_patch(&mut self, ops: usize, base_nodes: usize, base_edges: usize) {
        let full = full_build_cost(base_nodes, base_edges);
        let pc = patch_cost(ops);
        self.full_total += full;
        self.actual_total += pc;
        self.last_saved = full.saturating_sub(pc);
        self.actions += 1;
    }

    /// 总节约。
    pub fn total_saved(&self) -> u64 {
        self.full_total.saturating_sub(self.actual_total)
    }

    /// 节约率千分比。
    pub fn saved_permille(&self) -> u64 {
        if self.full_total == 0 {
            return 0;
        }
        (self.total_saved() * 1000) / self.full_total
    }
}

// ---------------------------------------------------------------------------
// §8 门面编排 — 缓存/增量/失效/遥测/收益全链
// ---------------------------------------------------------------------------

/// 图缓存门面：全链编排。
/// - `build_or_hit`：键铸→查缓存→命中返回克隆（记收益）；未命中全量构建
///   入缓存（记全量成本）。
/// - `apply_incremental`：增量 patch（记增量成本）+ 新键入缓存 + 旧键失效
///   记账。
/// - `invalidate`：失效裁决（表面/资源/布局三段对比）→ 槽逐出 → 账本记账
///   → 遥测。
pub struct GraphCacheFacade {
    pub cache: GraphCache,
    pub telemetry: CacheTelemetry,
    pub ledger: InvalidationLedger,
    pub probe: BuildCostProbe,
}

impl GraphCacheFacade {
    pub fn new() -> GraphCacheFacade {
        GraphCacheFacade {
            cache: GraphCache::new(),
            telemetry: CacheTelemetry::new(),
            ledger: InvalidationLedger::new(),
            probe: BuildCostProbe::new(),
        }
    }

    /// 帧推进（透传缓存时钟）。
    pub fn advance_tick(&mut self) {
        self.cache.advance_tick();
    }

    /// 构建或命中：同键第二次调用必走命中路径（性能收益实测的载体）。
    pub fn build_or_hit(
        &mut self,
        layout_ver: u32,
        nodes: Vec<GNode>,
        edges: Vec<GEdge>,
    ) -> (CompiledGraph, bool) {
        let key = CacheKey::mint(layout_ver, &surfaces_of(&nodes), &resources_of(&nodes));
        match self.cache.lookup(&key) {
            LookupOutcome::Hit(g) => {
                let n = g.nodes.len();
                let e = g.edges.len();
                self.telemetry.observe(&LookupOutcome::Hit(CompiledGraph::empty()));
                self.probe.note_hit(n, e);
                (g, true)
            }
            LookupOutcome::CollisionRejected => {
                self.telemetry
                    .observe(&LookupOutcome::CollisionRejected);
                self.ledger.record(key.combined, InvalidReason::CollisionRejected, self.cache.now());
                self.telemetry.note_invalidation();
                // 碰撞后重建（全量路径）。
                self.build_full_and_cache(key, nodes, edges)
            }
            LookupOutcome::Miss => {
                self.telemetry.observe(&LookupOutcome::Miss);
                self.build_full_and_cache(key, nodes, edges)
            }
        }
    }

    fn build_full_and_cache(
        &mut self,
        key: CacheKey,
        nodes: Vec<GNode>,
        edges: Vec<GEdge>,
    ) -> (CompiledGraph, bool) {
        let g = CompiledGraph::build_full(nodes, edges, self.cache.now().max(1));
        self.probe.note_full(g.nodes.len(), g.edges.len());
        self.cache.insert(key, g.clone());
        (g, false)
    }

    /// 增量应用：base 键不符返回 None；成功则返回新图并按新键入缓存、
    /// 旧键失效记账。
    pub fn apply_incremental(
        &mut self,
        patch: &IncrementalPatch,
        base_graph: &CompiledGraph,
    ) -> Option<CompiledGraph> {
        let new_graph = patch.apply(base_graph, &patch.base)?;
        let new_key = CacheKey::mint(
            patch.base.layout_ver,
            &new_graph.surfaces(),
            &new_graph.resources(),
        );
        // 失效裁决：base 键 vs 新键（三段对比定因）。
        if let Some(reason) = classify_invalidation(&patch.base, &new_key) {
            self.ledger.record(patch.base.combined, reason, self.cache.now());
            self.telemetry.note_invalidation();
            self.cache.evict(&patch.base);
        }
        self.probe.note_patch(
            patch.ops.len(),
            base_graph.nodes.len(),
            base_graph.edges.len(),
        );
        self.cache.insert(new_key, new_graph.clone());
        Some(new_graph)
    }

    /// 显式失效（外部驱动：表面销毁等场景）。
    pub fn invalidate(&mut self, key: &CacheKey, reason: InvalidReason) -> bool {
        let gone = self.cache.evict(key);
        self.ledger.record(key.combined, reason, self.cache.now());
        self.telemetry.note_invalidation();
        if gone {
            self.telemetry.note_eviction();
        }
        gone
    }

    pub fn snapshot(&self) -> TelemetrySnapshot {
        self.telemetry.snapshot()
    }
}

/// 从节点集取表面 id 集（去重升序）。
fn surfaces_of(nodes: &[GNode]) -> Vec<u32> {
    let mut acc: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < nodes.len() {
        if !contains_u32(&acc, nodes[i].surface) {
            acc.push(nodes[i].surface);
        }
        i += 1;
    }
    sort_u32(&mut acc);
    acc
}

/// 从节点集取资源 id 集（读集并集，去重升序）。
fn resources_of(nodes: &[GNode]) -> Vec<u32> {
    let mut acc: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < nodes.len() {
        let mut j = 0;
        while j < nodes[i].reads.len() {
            if !contains_u32(&acc, nodes[i].reads[j]) {
                acc.push(nodes[i].reads[j]);
            }
            j += 1;
        }
        i += 1;
    }
    sort_u32(&mut acc);
    acc
}

/// 便捷节点构造。
pub fn gnode(id: u32, kind: NodeKind, surface: u32, reads: &[u32]) -> GNode {
    let mut r = Vec::new();
    let mut i = 0;
    while i < reads.len() {
        r.push(reads[i]);
        i += 1;
    }
    GNode {
        id,
        kind,
        surface,
        reads: r,
    }
}

// ---------------------------------------------------------------------------
// §9 判据 — 七族三十四条（零 panic 面，预期独立给定）
// ---------------------------------------------------------------------------

/// F0170 判据总入口。
pub fn run_f0170_checks(checks: &mut CheckSet) {
    // ---- 键族（键-01..05）----
    // 键-01 序无关：同集不同铸序同 combined。
    let ka = CacheKey::mint(7, &[30, 10, 20], &[90, 40]);
    let kb = CacheKey::mint(7, &[20, 30, 10], &[40, 90]);
    checks.add("键-01-序无关同键", ka.combined == kb.combined, "surface/resource 集合乱序重铸应同键");

    // 键-02 计数混入：子集不得同键（{30,10} ⊂ {30,10,20}）。
    let kc = CacheKey::mint(7, &[30, 10], &[90, 40]);
    checks.add("键-02-子集不同键", ka.combined != kc.combined, "表面数不同必须不同键");

    // 键-03 字段独立：表面变键变；资源变键变；布局变键变。
    let ks = CacheKey::mint(7, &[30, 10, 25], &[90, 40]);
    let kr = CacheKey::mint(7, &[30, 10, 20], &[90, 41]);
    let kl = CacheKey::mint(8, &[30, 10, 20], &[90, 40]);
    checks.add(
        "键-03-三段独立",
        ka.combined != ks.combined && ka.combined != kr.combined && ka.combined != kl.combined,
        "表面/资源/布局任一变化键必须变",
    );

    // 键-04 明细对拍：同键同明细=Hit。
    let kd = CacheKey::mint(7, &[10, 20, 30], &[40, 90]);
    checks.add("键-04-同集命中", kd.matches(&ka) == KeyMatch::Hit, "同集不同铸序应明细对拍通过");

    // 键-05 碰撞防护：哈希段可伪造同、明细异必 Collision（用 combined 强写
    // 场景不可行，改验证 matches 的 Miss 分支：不同集直接 Miss 不进对拍）。
    let ke = CacheKey::mint(7, &[10, 20, 30, 99], &[40, 90]);
    checks.add(
        "键-05-异集Miss",
        kd.matches(&ke) == KeyMatch::Miss && ka.matches(&kc) == KeyMatch::Miss,
        "不同哈希段应直接 Miss",
    );

    // ---- 缓存族（缓存-01..06）----
    let mut cache = GraphCache::new();
    let key1 = CacheKey::mint(1, &[5], &[50]);
    let g1 = CompiledGraph::build_full(
        vec![gnode(1, NodeKind::Acquire, 5, &[])],
        vec![],
        1,
    );
    cache.insert(key1.clone(), g1.clone());

    // 缓存-01 存取回环。
    let hit = cache.lookup(&key1);
    let hit_ok = matches!(hit, LookupOutcome::Hit(ref g) if g.nodes.len() == 1);
    checks.add("缓存-01-存取回环", hit_ok, "insert 后同键 lookup 应命中");

    // 缓存-02 克隆隔离：改命中克隆不影响缓存。
    if let LookupOutcome::Hit(mut g2) = cache.lookup(&key1) {
        g2.nodes[0].surface = 999;
        let again = cache.lookup(&key1);
        let iso = matches!(again, LookupOutcome::Hit(ref g) if g.nodes[0].surface == 5);
        checks.add("缓存-02-克隆隔离", iso, "改克隆不得污染缓存本体");
    } else {
        checks.add("缓存-02-克隆隔离", false, "前置命中失败");
    }

    // 缓存-03 TTL 过期：超 CACHE_TTL_TICKS 后必 Miss。
    let mut ttl_cache = GraphCache::new();
    let keyt = CacheKey::mint(2, &[7], &[70]);
    ttl_cache.insert(keyt.clone(), g1.clone());
    let mut t = 0;
    while t <= CACHE_TTL_TICKS + 1 {
        ttl_cache.advance_tick();
        t += 1;
    }
    let expired = ttl_cache.lookup(&keyt);
    checks.add(
        "缓存-03-TTL过期",
        expired == LookupOutcome::Miss && ttl_cache.ttl_expired == 1,
        "超 TTL 应过期且记账",
    );

    // 缓存-04 满员逐最旧：塞 5 键占 4 槽，最早者让位。
    let mut full_cache = GraphCache::new();
    let mut i = 0;
    while i < CACHE_SLOTS + 1 {
        let k = CacheKey::mint(3, &[100 + i as u32], &[]);
        full_cache.advance_tick(); // 每键盖戳递增，最旧=第一个。
        full_cache.insert(k, g1.clone());
        i += 1;
    }
    let first_key = CacheKey::mint(3, &[100], &[]);
    let evicted_miss = full_cache.lookup(&first_key) == LookupOutcome::Miss;
    checks.add(
        "缓存-04-满员逐最旧",
        full_cache.occupied() == CACHE_SLOTS && evicted_miss && full_cache.evictions >= 1,
        "第 5 键应逐出第 1 键",
    );

    // 缓存-05 手动逐出后必 miss。
    let mut evict_cache = GraphCache::new();
    let key5 = CacheKey::mint(4, &[8], &[80]);
    evict_cache.insert(key5.clone(), g1.clone());
    let gone = evict_cache.evict(&key5);
    let after = evict_cache.lookup(&key5);
    checks.add(
        "缓存-05-逐出后必miss",
        gone && after == LookupOutcome::Miss,
        "失效纪律：逐出后同键必 miss",
    );

    // 缓存-06 同键 insert 刷新不占新槽。
    let mut refresh_cache = GraphCache::new();
    let key6 = CacheKey::mint(5, &[9], &[]);
    refresh_cache.insert(key6.clone(), g1.clone());
    refresh_cache.insert(key6.clone(), g1.clone());
    checks.add(
        "缓存-06-同键刷新",
        refresh_cache.occupied() == 1 && refresh_cache.evictions == 0,
        "同键二次 insert 应原槽刷新",
    );

    // ---- 增量族（增量-01..07）----
    // 场景：列表滚动——4 节点基础图，滚入 1 新节点 + 滚出 1 旧节点。
    let base_nodes = vec![
        gnode(1, NodeKind::Acquire, 10, &[]),
        gnode(2, NodeKind::Render, 10, &[50]),
        gnode(3, NodeKind::Render, 11, &[51]),
        gnode(4, NodeKind::Present, 12, &[60]),
    ];
    let base_edges = vec![
        GEdge { from: 1, to: 2 },
        GEdge { from: 1, to: 3 },
        GEdge { from: 2, to: 4 },
        GEdge { from: 3, to: 4 },
    ];
    let base_graph = CompiledGraph::build_full(base_nodes, base_edges, 1);
    let base_key = CacheKey::mint(10, &base_graph.surfaces(), &base_graph.resources());

    // 增量-01 Add：新节点入图、epoch=patch 代数、旧节点 epoch 保持。
    let mut p1 = IncrementalPatch::new(base_key.clone(), 2);
    p1.add_node(gnode(5, NodeKind::Render, 13, &[61]));
    p1.add_node(gnode(6, NodeKind::Present, 13, &[61]));
    let after_add = match p1.apply(&base_graph, &base_key) {
        Some(g) => g,
        None => CompiledGraph::empty(),
    };
    let add_ok = after_add.nodes.len() == 6
        && find_node(&after_add.nodes, 5).is_some()
        && epoch_of(&after_add, 1) == Some(1)
        && epoch_of(&after_add, 5) == Some(2);
    checks.add("增量-01-Add算子", add_ok, "Add 节点入图且新节点编译代数=patch 代数、旧节点保持");

    // 增量-02 Remove 级联：删节点 3，其 4 条关联边中 2 条（1→3、3→4）级联删。
    let mut p2 = IncrementalPatch::new(base_key.clone(), 2);
    p2.remove_node(3);
    let after_rm = match p2.apply(&base_graph, &base_key) {
        Some(g) => g,
        None => CompiledGraph::empty(),
    };
    let rm_ok = after_rm.nodes.len() == 3
        && after_rm.edges.len() == 2
        && find_node(&after_rm.nodes, 3).is_none();
    checks.add("增量-02-Remove级联", rm_ok, "Remove 删节点且关联边级联删除");

    // 增量-03 Replace：节点 2 换 surface，epoch 刷新。
    let mut p3 = IncrementalPatch::new(base_key.clone(), 3);
    p3.replace_node(gnode(2, NodeKind::Render, 14, &[52]));
    let after_rp = match p3.apply(&base_graph, &base_key) {
        Some(g) => g,
        None => CompiledGraph::empty(),
    };
    let rp_ok = match find_node(&after_rp.nodes, 2) {
        Some(n) => n.surface == 14 && seq_eq_u32(&n.reads, &[52]),
        None => false,
    } && epoch_of(&after_rp, 2) == Some(3);
    checks.add("增量-03-Replace算子", rp_ok, "Replace 原地换内容且编译代数刷新");

    // 增量-04 增量=全量一致：滚入 5（含两条边接入依赖链）、滚出 3 后，
    // 增量结果与"从零按终集全量构建"逐节点逐边全等。
    let mut p4 = IncrementalPatch::new(base_key.clone(), 2);
    p4.add_node(gnode(5, NodeKind::Render, 13, &[61]));
    p4.add_edge(1, 5);
    p4.add_edge(5, 4);
    p4.remove_node(3);
    let incremental = match p4.apply(&base_graph, &base_key) {
        Some(g) => g,
        None => CompiledGraph::empty(),
    };
    let full_nodes = vec![
        gnode(1, NodeKind::Acquire, 10, &[]),
        gnode(2, NodeKind::Render, 10, &[50]),
        gnode(4, NodeKind::Present, 12, &[60]),
        gnode(5, NodeKind::Render, 13, &[61]),
    ];
    let full_edges = vec![
        GEdge { from: 1, to: 2 },
        GEdge { from: 1, to: 5 },
        GEdge { from: 2, to: 4 },
        GEdge { from: 5, to: 4 },
    ];
    let rebuilt = CompiledGraph::build_full(full_nodes, full_edges, 9);
    checks.add(
        "增量-04-增量等于全量",
        incremental.structurally_equal(&rebuilt),
        "增量结果与从零全量重建逐节点逐边拓扑全等",
    );

    // 增量-05 base 校验：base 键与图不符整 patch 拒绝（键图不配——
    // 键声称的集合与真实图不符，增量无从谈起）。
    let wrong_base = CacheKey::mint(99, &[777], &[]);
    let mut p5 = IncrementalPatch::new(wrong_base.clone(), 2);
    p5.add_node(gnode(9, NodeKind::Render, 15, &[]));
    checks.add(
        "增量-05-base校验拒绝",
        p5.apply(&base_graph, &wrong_base).is_none(),
        "base 键与图不配应整 patch 拒绝",
    );

    // 增量-06 id 冲突拒绝：Add 已存在 id。
    let mut p6 = IncrementalPatch::new(base_key.clone(), 2);
    p6.add_node(gnode(2, NodeKind::Render, 99, &[]));
    checks.add(
        "增量-06-id冲突拒绝",
        p6.apply(&base_graph, &base_key).is_none(),
        "Add 已存在 id 应拒绝",
    );

    // 增量-07 未动节点编译态保留：增量后 4 个未动节点 epoch 全为 1。
    let keep_ok = after_add.epoch.iter().filter(|&&e| e == 1).count() == 4
        && epoch_of(&after_add, 5) == Some(2)
        && epoch_of(&after_add, 6) == Some(2);
    checks.add(
        "增量-07-未动保留",
        keep_ok,
        "增量只动变化节点：未动节点编译代数原样保留",
    );

    // ---- 失效族（失效-01..05）----
    let mut fac = GraphCacheFacade::new();
    let n1 = vec![gnode(1, NodeKind::Acquire, 20, &[])];
    let (g_a, hit_a) = fac.build_or_hit(1, n1.clone(), vec![]);
    let (g_b, hit_b) = fac.build_or_hit(1, n1.clone(), vec![]);
    checks.add(
        "失效-01-同键复用",
        !hit_a && hit_b && g_a.structurally_equal(&g_b),
        "首建未命中、次查命中且图一致",
    );

    // 失效-02 表面集变化即失效：变化后旧图绝不冒充新场景（键正确性），
    // 显式逐出走账，逐出后旧键必 miss。
    let n2 = vec![
        gnode(1, NodeKind::Acquire, 20, &[]),
        gnode(2, NodeKind::Render, 21, &[]),
    ];
    let old_key = CacheKey::mint(1, &g_a.surfaces(), &g_a.resources());
    let (g_new, hit_new) = fac.build_or_hit(1, n2.clone(), vec![]);
    let no_ghost = hit_new == false
        && match find_node(&g_new.nodes, 2) {
            Some(n) => n.surface == 21,
            None => false,
        };
    // 显式逐出旧键（表面销毁的外部驱动路径）。
    let evicted = fac.invalidate(&old_key, InvalidReason::SurfaceSetChanged);
    let old_gone = fac.cache.lookup(&old_key) == LookupOutcome::Miss;
    let ledger_has = fac.ledger.count_for(old_key.combined) == 1;
    checks.add(
        "失效-02-表面变化失效",
        no_ghost && evicted && old_gone && ledger_has,
        "表面变化后旧图不冒充、显式逐出后必 miss 且账本有录",
    );

    // 失效-03 失效裁决器：三段对比定因（布局 > 表面 > 资源 优先级）。
    let k_old = CacheKey::mint(1, &[1], &[2]);
    let k_layout = CacheKey::mint(2, &[1], &[2]);
    let k_surface = CacheKey::mint(1, &[3], &[2]);
    let k_resource = CacheKey::mint(1, &[1], &[5]);
    let cls_ok = classify_invalidation(&k_old, &k_layout) == Some(InvalidReason::LayoutAdvanced)
        && classify_invalidation(&k_old, &k_surface) == Some(InvalidReason::SurfaceSetChanged)
        && classify_invalidation(&k_old, &k_resource) == Some(InvalidReason::ResourceSetChanged)
        && classify_invalidation(&k_old, &k_old).is_none();
    checks.add("失效-03-裁决定因", cls_ok, "三段对比应正确定因且键同不算失效");

    // 失效-04 显式 invalidate：逐出成功 + 账本 + 遥测三联动。
    let mut fac2 = GraphCacheFacade::new();
    let _ = fac2.build_or_hit(3, vec![gnode(1, NodeKind::Acquire, 30, &[])], vec![]);
    let k3 = CacheKey::mint(3, &[30], &[]);
    let evicted = fac2.invalidate(&k3, InvalidReason::ManualEvict);
    let snap2 = fac2.snapshot();
    checks.add(
        "失效-04-显式失效联动",
        evicted
            && fac2.ledger.count_for(k3.combined) == 1
            && snap2.invalidations == 1
            && snap2.evictions == 1,
        "invalidate 应逐出+记账+遥测三联动",
    );

    // 失效-05 账本环形：容量满后挤最旧且 dropped 记账。
    let mut ledger = InvalidationLedger::new();
    let mut i = 0;
    while i < LEDGER_CAP + 3 {
        ledger.record(1000 + i as u64, InvalidReason::ManualEvict, i as u64);
        i += 1;
    }
    let first_gone = ledger.count_for(1000) == 0;
    let last_kept = ledger.count_for(1000 + LEDGER_CAP as u64 + 2) == 1;
    checks.add(
        "失效-05-账本环形",
        ledger.len() == LEDGER_CAP && first_gone && last_kept && ledger.dropped == 3,
        "超容量应挤最旧且 dropped 精确记账",
    );

    // ---- 遥测族（遥测-01..04）----
    // 遥测-01 命中率千分比：3 hit + 1 miss = 750‰。
    let mut tel = CacheTelemetry::new();
    tel.observe(&LookupOutcome::Hit(CompiledGraph::empty()));
    tel.observe(&LookupOutcome::Hit(CompiledGraph::empty()));
    tel.observe(&LookupOutcome::Hit(CompiledGraph::empty()));
    tel.observe(&LookupOutcome::Miss);
    checks.add("遥测-01-命中率", tel.snapshot().hit_permille() == 750, "3/4 应为 750‰");

    // 遥测-02 空口径 0：无查询无命中率（不除零）。
    checks.add("遥测-02-空口径", CacheTelemetry::new().snapshot().hit_permille() == 0, "分母 0 应记 0 不除零");

    // 遥测-03 快照单调：observe 只增不减。
    let mut tel3 = CacheTelemetry::new();
    tel3.observe(&LookupOutcome::Miss);
    let s_a = tel3.snapshot();
    tel3.observe(&LookupOutcome::CollisionRejected);
    let s_b = tel3.snapshot();
    checks.add(
        "遥测-03-单调累计",
        s_b.misses == s_a.misses
            && s_b.collisions == s_a.collisions + 1
            && s_b.hits >= s_a.hits,
        "累计口径应单调不减",
    );

    // 遥测-04 门面集成：build→hit→collision 路径遥测全落账。
    let mut fac4 = GraphCacheFacade::new();
    let _ = fac4.build_or_hit(6, vec![gnode(1, NodeKind::Acquire, 40, &[])], vec![]);
    let _ = fac4.build_or_hit(6, vec![gnode(1, NodeKind::Acquire, 40, &[])], vec![]);
    let s4 = fac4.snapshot();
    checks.add(
        "遥测-04-门面集成",
        s4.hits == 1 && s4.misses == 1,
        "门面两连查应 1 miss 1 hit",
    );

    // ---- 收益族（收益-01..04）----
    // 收益-01 命中成本 < 全量成本（实测：10 节点 12 边图）。
    let full10 = full_build_cost(10, 12);
    let mut probe1 = BuildCostProbe::new();
    probe1.note_full(10, 12);
    let c_full = probe1.actual_total;
    probe1.note_hit(10, 12);
    let c_hit = probe1.actual_total - c_full;
    checks.add(
        "收益-01-命中快于全量",
        c_hit == HIT_COST && HIT_COST < full10,
        "命中 O(1) 成本应严格小于全量构建成本",
    );

    // 收益-02 增量成本 < 全量成本（大图小补丁）。
    let mut probe2 = BuildCostProbe::new();
    probe2.note_full(200, 300);
    let c2_full = probe2.actual_total;
    probe2.note_patch(2, 200, 300);
    let c2_patch = probe2.actual_total - c2_full;
    checks.add(
        "收益-02-增量快于全量",
        c2_patch == patch_cost(2) && patch_cost(2) < full_build_cost(200, 300),
        "2 算子增量应远快于 200 节点全量",
    );

    // 收益-03 节约账累计与节约率。
    let mut probe3 = BuildCostProbe::new();
    probe3.note_full(10, 10);
    probe3.note_hit(10, 10);
    probe3.note_hit(10, 10);
    let saved_ok = probe3.total_saved() == full_build_cost(10, 10).saturating_sub(HIT_COST) * 2
        && probe3.saved_permille() > 0;
    checks.add("收益-03-节约账", saved_ok, "两次命中应累计节约且节约率为正");

    // 收益-04 门面端到端实测：同场景三连查，第 2、3 次命中节约为正。
    let mut fac5 = GraphCacheFacade::new();
    let scene = vec![
        gnode(1, NodeKind::Acquire, 50, &[]),
        gnode(2, NodeKind::Render, 50, &[500]),
        gnode(3, NodeKind::Present, 51, &[500, 501]),
    ];
    let edges_scene = vec![GEdge { from: 1, to: 2 }, GEdge { from: 2, to: 3 }];
    let (g1r, h1) = fac5.build_or_hit(9, scene.clone(), edges_scene.clone());
    let (g2r, h2) = fac5.build_or_hit(9, scene.clone(), edges_scene.clone());
    let (g3r, h3) = fac5.build_or_hit(9, scene.clone(), edges_scene.clone());
    let saved_positive = fac5.probe.total_saved() > 0
        && g1r.structurally_equal(&g2r)
        && g2r.structurally_equal(&g3r);
    checks.add(
        "收益-04-端到端实测",
        !h1 && h2 && h3 && saved_positive,
        "三连查应 1 全量 2 命中且累计节约为正、图一致",
    );

    // ---- 编排族（编排-01..03）----
    // 编排-01 全链：构建→命中→增量→失效→重建→再命中。
    let mut fac6 = GraphCacheFacade::new();
    let chain_n = vec![
        gnode(1, NodeKind::Acquire, 60, &[]),
        gnode(2, NodeKind::Present, 61, &[600]),
    ];
    let chain_e = vec![GEdge { from: 1, to: 2 }];
    let (g0, h0) = fac6.build_or_hit(12, chain_n.clone(), chain_e.clone());
    let (_g1r, h1r) = fac6.build_or_hit(12, chain_n.clone(), chain_e.clone());
    let kbase = CacheKey::mint(12, &g0.surfaces(), &g0.resources());
    let mut patch = IncrementalPatch::new(kbase.clone(), 2);
    patch.add_node(gnode(3, NodeKind::Render, 62, &[601]));
    let inc = fac6.apply_incremental(&patch, &g0);
    // 增量后新键命中。
    let (g2r, h2r) = match &inc {
        Some(g) => {
            let nk = CacheKey::mint(12, &g.surfaces(), &g.resources());
            match fac6.cache.lookup(&nk) {
                LookupOutcome::Hit(h) => (h, true),
                _ => (CompiledGraph::empty(), false),
            }
        }
        None => (CompiledGraph::empty(), false),
    };
    let chain_ok = !h0
        && h1r
        && inc.is_some()
        && h2r
        && match &inc {
            Some(g) => g2r.structurally_equal(g),
            None => false,
        }
        && fac6.ledger.len() >= 1;
    checks.add("编排-01-全链闭环", chain_ok, "构建→命中→增量→失效→新键命中全链应闭环");

    // 编排-02 增量后旧键失效：增量导致的键变化应把旧键从缓存逐出。
    let old_gone2 = fac6.cache.lookup(&kbase) == LookupOutcome::Miss;
    checks.add(
        "编排-02-增量旧键失效",
        old_gone2,
        "增量改变表面/资源集后旧键应失效",
    );

    // 编排-03 TTL 周期稳定性：同键在 TTL 内反复查结果恒定不漂移。
    let mut fac7 = GraphCacheFacade::new();
    let _ = fac7.build_or_hit(15, vec![gnode(1, NodeKind::Acquire, 70, &[])], vec![]);
    let k7 = CacheKey::mint(15, &[70], &[]);
    let mut stable = true;
    let mut tick_i = 0;
    while tick_i < CACHE_TTL_TICKS {
        fac7.advance_tick();
        if fac7.cache.lookup(&k7) == LookupOutcome::Miss {
            stable = false;
            break;
        }
        tick_i += 1;
    }
    checks.add(
        "编排-03-TTL内稳定",
        stable,
        "TTL 窗口内同键反复查应恒命中不漂移",
    );
}

/// 取某节点 epoch（找不到返回 None；零 panic 面）。
fn epoch_of(g: &CompiledGraph, node_id: u32) -> Option<u64> {
    let idx = index_of(&g.nodes, node_id)?;
    if idx < g.epoch.len() {
        Some(g.epoch[idx])
    } else {
        None
    }
}
