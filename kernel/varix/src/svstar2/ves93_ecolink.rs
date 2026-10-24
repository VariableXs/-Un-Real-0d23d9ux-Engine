#![allow(dead_code)]
use alloc::vec;
// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves93_ecolink
// VE-F3693 · 跨生态联动（R05 组 · 生态运营组）
//
// 锚点：VE-F3693
// 判据原文：循环禁令、分叉断言、治理单源、互引越权→权限（复用）。
//
// 难点分析：
//   1. 跨生态互引必须单向无环——主题引用壁纸 OK，壁纸反向引用主题同一资产即环，依赖地狱起点。
//   2. 「同一资产跨生态信息一致」是元数据单源的必然推论：描述分叉即用户困惑，须在互引建立时断言。
//   3. 治理分歧归一走单源复述（F3307/F3684），不得在本模块另起仲裁逻辑。


extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x8D00 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x8D00;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C8DCode(pub u16);

impl C8DCode {
    pub const CYCLE_REJECTED: Self = Self(SEG_BASE);          // 循环引用拒绝
    pub const FORK_DETECTED: Self = Self(SEG_BASE + 1);       // 元数据分叉
    pub const GOV_SPLIT: Self = Self(SEG_BASE + 2);           // 治理分歧
    pub const REF_DENIED: Self = Self(SEG_BASE + 3);          // 互引越权
    pub const META_DIVERGED: Self = Self(SEG_BASE + 4);       // 元数据漂移
    pub const LINK_BROKEN: Self = Self(SEG_BASE + 5);         // 断链
    pub const GOV_SINGLE: Self = Self(SEG_BASE + 6);          // 治理单源

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::CYCLE_REJECTED => "跨生态循环引用已拒绝——单向依赖红线",
            Self::FORK_DETECTED => "跨生态元数据分叉已检出——单源修正",
            Self::GOV_SPLIT => "治理分歧已归一——治理单源复述",
            Self::REF_DENIED => "互引越权已拒绝——权限复用",
            Self::META_DIVERGED => "元数据漂移已回正——信息分叉红线",
            Self::LINK_BROKEN => "互引断链已登记——引用完整性",
            Self::GOV_SINGLE => "治理决议已单源下发——治理单源",
            _ => "未知诊断",
        }
    }
}

// ── 生态域 ────────────────────────────────────────────────────────

/// 生态域标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EcoDomain {
    /// 主题生态。
    Theme,
    /// 壁纸生态。
    Wallpaper,
    /// 组件生态。
    Widget,
}

impl EcoDomain {
    pub fn name(self) -> &'static str {
        match self {
            Self::Theme => "主题",
            Self::Wallpaper => "壁纸",
            Self::Widget => "组件",
        }
    }

    /// 域序号（用于邻接矩阵下标）。
    pub fn index(self) -> usize {
        match self {
            Self::Theme => 0,
            Self::Wallpaper => 1,
            Self::Widget => 2,
        }
    }
}

/// 互引方向：源域 → 目标域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CrossRef {
    pub src: EcoDomain,
    pub dst: EcoDomain,
}

impl CrossRef {
    pub fn new(src: EcoDomain, dst: EcoDomain) -> Option<Self> {
        if src == dst {
            return None; // 同域不算跨生态
        }
        Some(Self { src, dst })
    }

    /// 反向边（用于环检测）。
    pub fn reversed(self) -> Self {
        Self { src: self.dst, dst: self.src }
    }
}

// ── 元数据单源 ────────────────────────────────────────────────────

/// 资产元数据快照（跨生态一致性比对用）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MetaSnapshot {
    pub asset_id: u64,
    pub version: u32,
    pub desc_digest: u64,
    pub owner: u64,
}

impl MetaSnapshot {
    pub fn new(asset_id: u64, version: u32, desc_digest: u64, owner: u64) -> Self {
        Self { asset_id, version, desc_digest, owner }
    }

    /// 与另一快照是否同源一致（同资产跨生态描述不分叉）。
    pub fn agrees_with(&self, other: &Self) -> bool {
        self.asset_id == other.asset_id
            && self.version == other.version
            && self.desc_digest == other.desc_digest
            && self.owner == other.owner
    }
}

// ── 联动图 ────────────────────────────────────────────────────────

/// 跨生态互引图（三生态邻接 + 资产级引用边）。
pub struct EcoLinkGraph {
    /// 域邻接矩阵（3×3，true 表示存在 src→dst 互引边）。
    adj: [[bool; 3]; 3],
    /// 资产级引用边：(src_asset, dst_asset)。
    asset_edges: Vec<(u64, u64)>,
    /// 域边计数。
    edge_count: [u32; 3],
}

impl EcoLinkGraph {
    pub fn new() -> Self {
        Self {
            adj: [[false; 3]; 3],
            asset_edges: Vec::new(),
            edge_count: [0; 3],
        }
    }

    /// 建立域级互引边。若会形成二环（dst→src 已存在）则拒绝。
    pub fn link_domain(&mut self, r: CrossRef) -> Result<(), C8DCode> {
        let (si, di) = (r.src.index(), r.dst.index());
        if self.adj[di][si] {
            return Err(C8DCode::CYCLE_REJECTED);
        }
        if !self.adj[si][di] {
            self.adj[si][di] = true;
            self.edge_count[si] = self.edge_count[si].saturating_add(1);
        }
        Ok(())
    }

    /// 建立资产级引用边，带环检测（DFS）。
    pub fn link_asset(&mut self, src: u64, dst: u64) -> Result<(), C8DCode> {
        if src == dst {
            return Err(C8DCode::CYCLE_REJECTED);
        }
        // 已存在 src→dst 则幂等放行
        for &(s, d) in &self.asset_edges {
            if s == src && d == dst {
                return Ok(());
            }
        }
        // 若 dst 可达 src，则加边成环
        if self.reachable(dst, src) {
            return Err(C8DCode::CYCLE_REJECTED);
        }
        self.asset_edges.push((src, dst));
        Ok(())
    }

    /// 从 start 出发能否到达 goal（DFS）。
    pub fn reachable(&self, start: u64, goal: u64) -> bool {
        if start == goal {
            return true;
        }
        let mut stack = vec![start];
        let mut visited: Vec<u64> = Vec::new();
        while let Some(node) = stack.pop() {
            if visited.contains(&node) {
                continue;
            }
            visited.push(node);
            for &(s, d) in &self.asset_edges {
                if s == node {
                    if d == goal {
                        return true;
                    }
                    if !visited.contains(&d) {
                        stack.push(d);
                    }
                }
            }
        }
        false
    }

    /// 域二环检测（adj 中是否存在互反边）。
    pub fn has_domain_cycle(&self) -> bool {
        for i in 0..3 {
            for j in (i + 1)..3 {
                if self.adj[i][j] && self.adj[j][i] {
                    return true;
                }
            }
        }
        false
    }

    pub fn asset_edge_count(&self) -> usize {
        self.asset_edges.len()
    }

    pub fn domain_edges_of(&self, d: EcoDomain) -> u32 {
        self.edge_count[d.index()]
    }
}

impl Default for EcoLinkGraph {
    fn default() -> Self {
        Self::new()
    }
}

// ── 分叉断言 ──────────────────────────────────────────────────────

/// 跨生态元数据一致性登记表。
pub struct MetaRegistry {
    /// (asset_id, first_seen_snapshot) —— 先到先得为单源。
    entries: Vec<(u64, MetaSnapshot)>,
    /// 分叉检出次数。
    forks_caught: u32,
}

impl MetaRegistry {
    pub fn new() -> Self {
        Self { entries: Vec::new(), forks_caught: 0 }
    }

    /// 登记一份快照。与既有单源不一致时拒绝并计数分叉。
    pub fn register(&mut self, snap: &MetaSnapshot) -> Result<(), C8DCode> {
        for (id, canonical) in &self.entries {
            if *id == snap.asset_id {
                if canonical.agrees_with(snap) {
                    return Ok(());
                }
                self.forks_caught = self.forks_caught.saturating_add(1);
                return Err(C8DCode::FORK_DETECTED);
            }
        }
        self.entries.push((snap.asset_id, snap.clone()));
        Ok(())
    }

    /// 取单源快照。
    pub fn lookup(&self, asset_id: u64) -> Option<&MetaSnapshot> {
        for (id, snap) in &self.entries {
            if *id == asset_id {
                return Some(snap);
            }
        }
        None
    }

    /// 单源修正：以权威快照覆盖（治理单源下发后的回正动作）。
    pub fn reconcile(&mut self, snap: &MetaSnapshot) -> Result<(), C8DCode> {
        for (id, canonical) in self.entries.iter_mut() {
            if *id == snap.asset_id {
                *canonical = snap.clone();
                return Ok(());
            }
        }
        self.entries.push((snap.asset_id, snap.clone()));
        Ok(())
    }

    pub fn forks_caught(&self) -> u32 {
        self.forks_caught
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for MetaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── 治理单源 ──────────────────────────────────────────────────────

/// 治理决议来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GovSource {
    /// 本域治理。
    Local,
    /// F3307/F3684 单源复述。
    Single,
    /// 联合治理（跨域）。
    Joint,
}

impl GovSource {
    /// 是否具备单源权威（Local 不算单源，须经 Single 归一）。
    pub fn is_authoritative(self) -> bool {
        matches!(self, Self::Single)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Local => "本域治理",
            Self::Single => "治理单源",
            Self::Joint => "联合治理",
        }
    }
}

/// 治理分歧消解器——一切分歧归一到 Single 单源。
pub struct GovUnifier {
    /// 归一动作计数。
    unify_count: u32,
    /// 分歧未决队列（等待单源下发）。
    pending: Vec<u64>,
}

impl GovUnifier {
    pub fn new() -> Self {
        Self { unify_count: 0, pending: Vec::new() }
    }

    /// 提交一个带来源的治理议题。Local 来源会挂起等待单源；Single 直接生效。
    pub fn submit(&mut self, issue: u64, src: GovSource) -> Result<(), C8DCode> {
        if src.is_authoritative() {
            self.unify_count = self.unify_count.saturating_add(1);
            self.pending.retain(|&x| x != issue);
            return Ok(());
        }
        if src == GovSource::Local {
            if !self.pending.contains(&issue) {
                self.pending.push(issue);
            }
            return Err(C8DCode::GOV_SPLIT);
        }
        // Joint 也须单源归一后才权威
        if !self.pending.contains(&issue) {
            self.pending.push(issue);
        }
        Err(C8DCode::GOV_SPLIT)
    }

    /// 单源下发——消解 pending 中的议题。
    pub fn unify(&mut self, issue: u64) -> Result<(), C8DCode> {
        self.pending.retain(|&x| x != issue);
        self.unify_count = self.unify_count.saturating_add(1);
        Ok(())
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn unify_count(&self) -> u32 {
        self.unify_count
    }
}

impl Default for GovUnifier {
    fn default() -> Self {
        Self::new()
    }
}

// ── 互引权限 ──────────────────────────────────────────────────────

/// 互引权限表（谁可以引谁）。
pub struct RefPolicy {
    /// 域级许可矩阵。
    allow: [[bool; 3]; 3],
}

impl RefPolicy {
    /// 默认策略：任意跨域方向单向可引，但须过环检测。
    pub fn standard() -> Self {
        let mut allow = [[false; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                if i != j {
                    allow[i][j] = true;
                }
            }
        }
        Self { allow }
    }

    /// 收紧：禁止某方向。
    pub fn deny(&mut self, r: CrossRef) {
        self.allow[r.src.index()][r.dst.index()] = false;
    }

    /// 许可查询。
    pub fn permits(&self, r: CrossRef) -> bool {
        self.allow[r.src.index()][r.dst.index()]
    }

    /// 越权检查：不允许即拒绝。
    pub fn check(&self, r: CrossRef) -> Result<(), C8DCode> {
        if self.permits(r) {
            Ok(())
        } else {
            Err(C8DCode::REF_DENIED)
        }
    }
}

// ── 联动总线 ──────────────────────────────────────────────────────

/// 跨生态联动总线——组合图 + 元数据 + 治理 + 权限。
pub struct EcoLinkBus {
    pub graph: EcoLinkGraph,
    pub meta: MetaRegistry,
    pub gov: GovUnifier,
    pub policy: RefPolicy,
}

impl EcoLinkBus {
    pub fn new() -> Self {
        Self {
            graph: EcoLinkGraph::new(),
            meta: MetaRegistry::new(),
            gov: GovUnifier::new(),
            policy: RefPolicy::standard(),
        }
    }

    /// 完整互引流程：权限 → 环检 → 落边。
    pub fn cross_link(&mut self, r: CrossRef, src_asset: u64, dst_asset: u64) -> Result<(), C8DCode> {
        self.policy.check(r)?;
        self.graph.link_domain(r)?;
        self.graph.link_asset(src_asset, dst_asset)
    }

    /// 元数据登记（分叉即拒）。
    pub fn sync_meta(&mut self, snap: &MetaSnapshot) -> Result<(), C8DCode> {
        self.meta.register(snap)
    }
}

impl Default for EcoLinkBus {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves93_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves93_ecolink");
    {
        // 01 循环禁令：二环边拒绝
        let mut g = EcoLinkGraph::new();
        let a = CrossRef::new(EcoDomain::Theme, EcoDomain::Wallpaper).unwrap();
        let b = CrossRef::new(EcoDomain::Wallpaper, EcoDomain::Theme).unwrap();
        let r1 = g.link_domain(a);
        let r2 = g.link_domain(b);
        chk(
            &mut set,
            "C38D-01",
            r1.is_ok() && r2.is_err() && r2.err() == Some(C8DCode::CYCLE_REJECTED),
            "二环互引须拒绝（单向依赖红线）",
        );
    }
    {
        // 02 资产环拒绝
        let mut g = EcoLinkGraph::new();
        assert!(g.link_asset(1, 2).is_ok());
        assert!(g.link_asset(2, 3).is_ok());
        let r = g.link_asset(3, 1);
        chk(
            &mut set,
            "C38D-02",
            r.is_err() && r.err() == Some(C8DCode::CYCLE_REJECTED),
            "资产级三节点环须在加边前拒绝",
        );
    }
    {
        // 03 自引拒绝
        let mut g = EcoLinkGraph::new();
        let r = g.link_asset(7, 7);
        chk(&mut set, "C38D-03", r.is_err(), "自引即环，须拒绝");
    }
    {
        // 04 分叉断言
        let mut reg = MetaRegistry::new();
        let s1 = MetaSnapshot::new(10, 1, 0xAA, 100);
        let s2 = MetaSnapshot::new(10, 1, 0xBB, 100); // 描述分叉
        assert!(reg.register(&s1).is_ok());
        let r = reg.register(&s2);
        chk(
            &mut set,
            "C38D-04",
            r.is_err() && r.err() == Some(C8DCode::FORK_DETECTED),
            "同资产跨生态描述分叉须检出（信息分叉红线）",
        );
    }
    {
        // 05 单源一致放行
        let mut reg = MetaRegistry::new();
        let s1 = MetaSnapshot::new(11, 2, 0xCC, 200);
        let r = reg.register(&s1);
        let r2 = reg.register(&s1);
        chk(&mut set, "C38D-05", r.is_ok() && r2.is_ok(), "同源快照幂等放行");
    }
    {
        // 06 治理单源：Local 挂起，Single 生效
        let mut gu = GovUnifier::new();
        let r_local = gu.submit(1, GovSource::Local);
        let r_single = gu.submit(1, GovSource::Single);
        chk(
            &mut set,
            "C38D-06",
            r_local.is_err() && r_single.is_ok() && gu.pending_count() == 0,
            "治理分歧须挂起待单源，单源下发即归一",
        );
    }
    {
        // 07 互引越权
        let mut pol = RefPolicy::standard();
        pol.deny(CrossRef::new(EcoDomain::Widget, EcoDomain::Theme).unwrap());
        let r = pol.check(CrossRef::new(EcoDomain::Widget, EcoDomain::Theme).unwrap());
        chk(
            &mut set,
            "C38D-07",
            r.is_err() && r.err() == Some(C8DCode::REF_DENIED),
            "互引越权须拒绝（权限复用）",
        );
    }
    {
        // 08 同域引用不算跨生态
        let r = CrossRef::new(EcoDomain::Theme, EcoDomain::Theme);
        chk(&mut set, "C38D-08", r.is_none(), "同域 CrossRef 不构造");
    }
    {
        // 09 可达性查询正确
        let mut g = EcoLinkGraph::new();
        assert!(g.link_asset(1, 2).is_ok());
        assert!(g.link_asset(2, 3).is_ok());
        chk(
            &mut set,
            "C38D-09",
            g.reachable(1, 3) && !g.reachable(3, 1),
            "可达性须单向（1→2→3 可达 3，反向不可）",
        );
    }
    {
        // 10 域二环检测
        let mut g = EcoLinkGraph::new();
        // 绕过 link_domain 的拒绝，直接构造二环验证检测器
        g.adj[0][1] = true;
        g.adj[1][0] = true;
        chk(&mut set, "C38D-10", g.has_domain_cycle(), "域邻接二环须被检测器捕获");
    }
    {
        // 11 单源修正覆盖分叉
        let mut reg = MetaRegistry::new();
        let s1 = MetaSnapshot::new(20, 1, 0x11, 1);
        assert!(reg.register(&s1).is_ok());
        let authoritative = MetaSnapshot::new(20, 2, 0x22, 1);
        assert!(reg.reconcile(&authoritative).is_ok());
        let got = reg.lookup(20);
        chk(
            &mut set,
            "C38D-11",
            got.is_some() && got.unwrap().version == 2,
            "单源修正须覆盖既有快照（归一动作）",
        );
    }
    {
        // 12 总线完整流程
        let mut bus = EcoLinkBus::new();
        let r = bus.cross_link(
            CrossRef::new(EcoDomain::Theme, EcoDomain::Widget).unwrap(),
            100,
            200,
        );
        let snap = MetaSnapshot::new(100, 1, 0x55, 9);
        let r2 = bus.sync_meta(&snap);
        chk(
            &mut set,
            "C38D-12",
            r.is_ok() && r2.is_ok() && bus.graph.asset_edge_count() == 1,
            "总线互引+元数据登记全通",
        );
    }
    set
}
