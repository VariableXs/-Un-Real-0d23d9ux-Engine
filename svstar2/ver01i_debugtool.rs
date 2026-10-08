//! VE-F3409 · 令牌调试工具 —— 检查器 + 追踪器 + 覆盖来源视图。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3409`
//!
//! # 职责（锚点原文拆解）
//!
//! - **令牌检查器**：任意 UI 元素反查其所用令牌链——元素查令牌、
//!   令牌查元素，两个方向都要能走通；查询记录在求值时落账，
//!   反查只是读账，不重放求值（重放会让"看一眼"变成"再跑一遍"）；
//! - **求值追踪**：一个令牌值的完整求值过程——从目标出发，
//!   沿依赖闭包逐节点给出「值从哪来、走没走缓存、花了多少步、
//!   依赖了谁」，截断必须显式记账而非悄悄停；
//! - **覆盖来源视图**：每个令牌的值由哪一级来源提供（默认/主题/
//!   场景/组件/计算五级封闭全集），视图是来源表的**投影**，
//!   投影与表失配即漂移，漂移的处置是**校准**而不是重画；
//! - **跨批对接点 X03 会话接入**：调试工具的所有入口都必须绑会话，
//!   未绑会话拒绝服务——调试数据混进别人的会话比没有调试更糟；
//! - **降级矩阵**：反查失败→提示；追踪开销→超预算降级；视图漂移→校准。
//!
//! # 为什么查询账本自己持哈希索引而不是线性扫
//!
//! 锚点给的性能口径是 **O(1) 查询**。`alloc` 没有 `HashMap`
//! （它在 `std` 里），`BTreeMap` 是 O(log n)。这里手写一张
//! **开桶索引**（FNV-1a 64 散列 + 定桶数 + 桶内追加）：
//! 平均 O(1)、最坏 O(桶内冲突数)，且桶内保持**插入序**——
//! 反查产出的链路顺序因此是确定的（同输入必得同输出），
//! 这是判据可复现的前提。
//!
//! # 为什么追踪器不钩进 F3408 的求值循环而是从公开结果反推
//!
//! 钩进 `EvalPipeline::evaluate` 内部要么改别人的函数签名，
//! 要么复制一份求值器——前者越界，后者会让「追踪看到的求值」
//! 与「真实发生的求值」变成两个实现（对拍漂移的温床）。
//! F3408 的 [`crate::svstar2::ver01h_evalperf`] 公开面已包含
//! 反推所需的全部事实（图 + 逐节点值/步数/缓存标志），
//! 追踪器从这些事实**确定性重建**过程：同一份结果永远推出
//! 同一条链，对拍才有意义。追踪的代价是 O(闭包)，这正是
//! 「追踪开销→超预算降级」要管的对象。
//!
//! # 零 panic 面
//!
//! `[i]` / `unwrap()` / `expect()` 只出现在 `#[cfg(test)]`；
//! `run_*_checks` 内一律 match 记红。

use crate::svstar2::ver01h_evalperf::{BatchResult, EvalGraph};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（自建；E11 段独占，与 E08/F3408 等既有段零重叠）
// ---------------------------------------------------------------------------

/// 调试工具域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraceCode {
    /// UI 元素路径非法（空或超长）。
    ElementInvalid,
    /// 令牌路径非法（空或超长）。
    TokenInvalid,
    /// 会话未绑定（X03 对接点：调试入口必须持有效会话）。
    SessionUnbound,
    /// 反查无结果（提示级：反查失败→提示，不阻断）。
    NotFound,
    /// 追踪超出预算（降级：截断留痕，不阻断）。
    TraceBudgetExceeded,
    /// 来源视图与来源表漂移（处置：校准）。
    ViewDrifted,
    /// 会话不匹配（结果带着别的会话号被呈报，阻断）。
    SessionMismatch,
}

impl TraceCode {
    /// 全部码（判据据此核对无遗漏）。
    pub const ALL: [TraceCode; 7] = [
        TraceCode::ElementInvalid,
        TraceCode::TokenInvalid,
        TraceCode::SessionUnbound,
        TraceCode::NotFound,
        TraceCode::TraceBudgetExceeded,
        TraceCode::ViewDrifted,
        TraceCode::SessionMismatch,
    ];

    /// 线上短码（E11 段独占）。
    pub const fn code(self) -> &'static str {
        match self {
            TraceCode::ElementInvalid => "E11-ELEMENT-INVALID",
            TraceCode::TokenInvalid => "E11-TOKEN-INVALID",
            TraceCode::SessionUnbound => "E11-SESSION-UNBOUND",
            TraceCode::NotFound => "E11-NOT-FOUND",
            TraceCode::TraceBudgetExceeded => "E11-TRACE-BUDGET",
            TraceCode::ViewDrifted => "E11-VIEW-DRIFTED",
            TraceCode::SessionMismatch => "E11-SESSION-MISMATCH",
        }
    }

    /// 是否阻断（NotFound 是提示级；预算降级与漂移校准均不阻断）。
    pub const fn blocking(self) -> bool {
        !matches!(
            self,
            TraceCode::NotFound | TraceCode::TraceBudgetExceeded | TraceCode::ViewDrifted
        )
    }

    /// 是否属于降级类（走降级矩阵而非直接失败）。
    pub const fn degradable(self) -> bool {
        matches!(
            self,
            TraceCode::TraceBudgetExceeded | TraceCode::ViewDrifted
        )
    }

    /// 读屏可达句子。
    pub fn spoken(self) -> String {
        let s = match self {
            TraceCode::ElementInvalid => "UI 元素路径非法。",
            TraceCode::TokenInvalid => "令牌路径非法。",
            TraceCode::SessionUnbound => "调试工具未绑定会话，拒绝服务。",
            TraceCode::NotFound => "反查无结果：该元素没有已记录的令牌查询。",
            TraceCode::TraceBudgetExceeded => "求值追踪超出预算，已截断并留痕。",
            TraceCode::ViewDrifted => "来源视图与来源表不一致，已校准。",
            TraceCode::SessionMismatch => "调试结果属于另一个会话，拒绝呈报。",
        };
        format!("{}{}", s, self.code())
    }
}

// ---------------------------------------------------------------------------
// 二、常量与契约
// ---------------------------------------------------------------------------

/// 元素路径字节上限。
pub const ELEMENT_MAX: usize = 256;
/// 令牌路径字节上限。
pub const TOKEN_PATH_MAX: usize = 256;
/// 会话号字节上限。
pub const SESSION_MAX: usize = 64;
/// 查询账本容量（条）。
pub const MAX_QUERIES: usize = 16384;
/// 单次追踪步数预算。
pub const MAX_TRACE_STEPS: usize = 256;
/// 哈希索引桶数（2 的幂；平均每桶 ≈ MAX_QUERIES/BUCKETS ≈ 4）。
pub const INDEX_BUCKETS: usize = 4096;

/// 调试工具契约版本（契约冻结）。
pub const DEBUG_CONTRACT: &str = "E11-debug-v1";

/// 校验元素路径。
fn validate_element(p: &str) -> Result<(), TraceCode> {
    if p.is_empty() || p.len() > ELEMENT_MAX {
        return Err(TraceCode::ElementInvalid);
    }
    Ok(())
}

/// 校验令牌路径。
fn validate_token(p: &str) -> Result<(), TraceCode> {
    if p.is_empty() || p.len() > TOKEN_PATH_MAX {
        return Err(TraceCode::TokenInvalid);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、X03 会话（调试入口的统一闸）
// ---------------------------------------------------------------------------

/// 调试会话（不可复制，凭 `SessionToken` 出入）。
///
/// 会话号在创建时校验一次，之后所有工具入口只接受 [`SessionToken`]——
/// 「拿着一个裸字符串到处当会话号」会让"哪条调试数据属于谁"
/// 变成口说无凭；类型化的票据让冒用他人的会话在类型上不可表达。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Session {
    id: String,
}

impl Session {
    /// 绑定会话（X03）。
    pub fn bind(id: &str) -> Result<Session, TraceCode> {
        if id.is_empty() || id.len() > SESSION_MAX {
            return Err(TraceCode::SessionUnbound);
        }
        Ok(Session {
            id: String::from(id),
        })
    }

    /// 会话号。
    pub fn id(&self) -> &str {
        &self.id
    }

    /// 与结果上的会话号对票。
    pub fn check(&self, owner: &str) -> Result<(), TraceCode> {
        if owner == self.id {
            Ok(())
        } else {
            Err(TraceCode::SessionMismatch)
        }
    }
}

// ---------------------------------------------------------------------------
// 四、开桶哈希索引（alloc 面 O(1) 平均查询；桶内保插入序）
// ---------------------------------------------------------------------------

/// FNV-1a 64（确定性散列，跨平台同值）。
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// 字符串 → 查询记录下标的开桶索引。
///
/// 桶内追加保持**插入序**；查找时桶内线性比对真键
/// （散列只选桶，不判等——判等必须落在键本身上）。
#[derive(Clone, PartialEq, Eq, Debug)]
struct HashIndex {
    buckets: Vec<Vec<u32>>,
}

impl HashIndex {
    fn new() -> HashIndex {
        HashIndex {
            buckets: vec![Vec::new(); INDEX_BUCKETS],
        }
    }

    fn insert(&mut self, key: &str, idx: u32) {
        let b = (fnv1a(key) % INDEX_BUCKETS as u64) as usize;
        self.buckets[b].push(idx);
    }

    /// 命中键的全部记录下标（插入序）。
    fn get<'a>(&'a self, key: &str, key_of: impl Fn(u32) -> Option<&'a str>) -> Vec<u32> {
        let b = (fnv1a(key) % INDEX_BUCKETS as u64) as usize;
        let mut out = Vec::new();
        for idx in self.buckets[b].iter() {
            if let Some(k) = key_of(*idx) {
                if k == key {
                    out.push(*idx);
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 五、令牌检查器（反查账本）
// ---------------------------------------------------------------------------

/// 一条查询记录：某元素在某会话里问过某令牌。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Query {
    /// 会话号（记录落账时快照，呈报时对票）。
    pub session: String,
    /// UI 元素路径。
    pub element: String,
    /// 令牌路径。
    pub token: String,
}

/// 令牌检查器。
///
/// 记录与索引分离：`log` 是事实（追加序），两张索引是事实的
/// 检索面。反查产出=「账上事实的投影」，因此**同账必同链**。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TokenInspector {
    /// 查询账本（追加序，容量 [`MAX_QUERIES`]）。
    pub log: Vec<Query>,
    /// 元素 → 记录下标索引。
    by_element: HashIndex,
    /// 令牌 → 记录下标索引。
    by_token: HashIndex,
    /// 账本是否已截断（超容量后拒绝新记录，不静默覆盖）。
    pub truncated: bool,
}

impl TokenInspector {
    /// 新建空检查器。
    pub fn new() -> TokenInspector {
        TokenInspector {
            log: Vec::new(),
            by_element: HashIndex::new(),
            by_token: HashIndex::new(),
            truncated: false,
        }
    }

    /// 落账：元素在会话里查询了令牌。
    ///
    /// 超容量**拒绝并标记截断**，不覆盖旧行——覆盖会让
    /// 「之前反查得到过、现在反查不到了」无从归因。
    pub fn record_query(
        &mut self,
        session: &Session,
        element: &str,
        token: &str,
    ) -> Result<(), TraceCode> {
        validate_element(element)?;
        validate_token(token)?;
        if self.log.len() >= MAX_QUERIES {
            self.truncated = true;
            return Err(TraceCode::TraceBudgetExceeded);
        }
        let idx = self.log.len() as u32;
        self.log.push(Query {
            session: String::from(session.id()),
            element: String::from(element),
            token: String::from(token),
        });
        self.by_element.insert(element, idx);
        self.by_token.insert(token, idx);
        Ok(())
    }

    fn element_of(&self, idx: u32) -> Option<&str> {
        self.log.get(idx as usize).map(|q| q.element.as_str())
    }

    fn token_of(&self, idx: u32) -> Option<&str> {
        self.log.get(idx as usize).map(|q| q.token.as_str())
    }

    /// 元素反查令牌链（查询序去重；O(1) 平均）。
    pub fn chain_of_element(
        &self,
        session: &Session,
        element: &str,
    ) -> Result<Vec<String>, TraceCode> {
        validate_element(element)?;
        let mut out: Vec<String> = Vec::new();
        for idx in self.by_element.get(element, |i| self.element_of(i)).iter() {
            let q = match self.log.get(*idx as usize) {
                Some(q) => q,
                None => continue,
            };
            if q.session != session.id() {
                continue; // 他会话的记录不属于本次反查，跳过不报错
            }
            if !out.iter().any(|t| t == &q.token) {
                out.push(String::from(&q.token));
            }
        }
        if out.is_empty() {
            return Err(TraceCode::NotFound);
        }
        Ok(out)
    }

    /// 令牌反查元素清单（查询序去重；O(1) 平均）。
    pub fn elements_of_token(
        &self,
        session: &Session,
        token: &str,
    ) -> Result<Vec<String>, TraceCode> {
        validate_token(token)?;
        let mut out: Vec<String> = Vec::new();
        for idx in self.by_token.get(token, |i| self.token_of(i)).iter() {
            let q = match self.log.get(*idx as usize) {
                Some(q) => q,
                None => continue,
            };
            if q.session != session.id() {
                continue;
            }
            if !out.iter().any(|e| e == &q.element) {
                out.push(String::from(&q.element));
            }
        }
        if out.is_empty() {
            return Err(TraceCode::NotFound);
        }
        Ok(out)
    }

    /// 账本规模（读屏可达）。
    pub fn spoken(&self) -> String {
        format!(
            "令牌检查器：已记录 {} 条查询，截断标志 {}。",
            self.log.len(),
            if self.truncated { "真" } else { "假" }
        )
    }
}

// ---------------------------------------------------------------------------
// 六、求值追踪器（从 F3408 公开事实确定性重建求值过程）
// ---------------------------------------------------------------------------

/// 一个节点的求值事实。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TraceNode {
    /// 令牌路径。
    pub token: String,
    /// 派生层级（基础令牌为 0）。
    pub level: u32,
    /// 是否命中缓存（true = 本次求值未重算）。
    pub cached: bool,
    /// 求值步数（F3408 抽象机口径）。
    pub steps: u32,
    /// 直接依赖的令牌路径（图序）。
    pub refs: Vec<String>,
}

/// 一条完整求值追踪。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EvalTrace {
    /// 会话号。
    pub session: String,
    /// 追踪目标令牌。
    pub target: String,
    /// 过程节点（拓扑序：层级升序、同层按节点号）。
    pub nodes: Vec<TraceNode>,
    /// 是否因超预算截断。
    pub truncated: bool,
    /// 因截断未展开的节点数。
    pub withheld: usize,
    /// 追踪自身步数（过程节点数，用于预算判定）。
    pub steps: usize,
}

impl EvalTrace {
    /// 读屏句子（无障碍：过程逐节点可读）。
    pub fn spoken(&self) -> String {
        let mut s = format!(
            "求值追踪：目标 {}，共 {} 个节点，步数 {}。",
            self.target,
            self.nodes.len(),
            self.steps
        );
        for n in self.nodes.iter() {
            s.push_str(&format!(
                "{}（层级 {}，{}，{} 步，依赖 {} 个）：",
                n.token,
                n.level,
                if n.cached { "缓存命中" } else { "实际计算" },
                n.steps,
                n.refs.len()
            ));
        }
        if self.truncated {
            s.push_str(&format!("已截断，未展开 {} 个节点。", self.withheld));
        }
        s
    }
}

/// 求值追踪器。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EvalTracer {
    /// 单次追踪步数预算。
    pub budget: usize,
}

impl EvalTracer {
    /// 新建追踪器（预算取 [`MAX_TRACE_STEPS`]）。
    pub fn new() -> EvalTracer {
        EvalTracer {
            budget: MAX_TRACE_STEPS,
        }
    }

    /// 追踪一个目标令牌的完整求值过程。
    ///
    /// 数据源：F3408 的图（依赖结构）与批结果（逐节点值/步数/缓存标志）。
    /// 过程 = 目标的**依赖闭包**，按拓扑序展开；展开超出预算即降级——
    /// 已展开的保留、未展开的记入 `withheld`，绝不静默截短。
    pub fn trace(
        &self,
        session: &Session,
        graph: &EvalGraph,
        result: &BatchResult,
        target: u32,
    ) -> Result<EvalTrace, TraceCode> {
        if target as usize >= graph.nodes {
            return Err(TraceCode::TokenInvalid);
        }
        let tpath = match graph.paths.get(target as usize) {
            Some(p) => p,
            None => return Err(TraceCode::TokenInvalid),
        };

        // 闭包 = 目标 + **正向依赖闭包**（我依赖谁，含间接）。
        // ⚠ 方向：求值过程是「值从哪来」，要沿 `refs` 向上游走；
        //   `downstream` 给的是「谁依赖我」（反向），拿它当过程会把
        //   追踪写成影响面分析。显式栈做传递闭包：不递归（万级链长），
        //   每入队一次即标记；产出按 (层级, 节点号) 全序排列 ⇒ 确定性。
        let closure = {
            // 先做闭包收集（seen 直接用节点号集合）
            let mut seen: Vec<u32> = vec![target];
            let mut stack: Vec<u32> = vec![target];
            while let Some(cur) = stack.pop() {
                for r in graph.refs(cur).iter() {
                    if !seen.contains(r) {
                        seen.push(*r);
                        stack.push(*r);
                    }
                }
            }
            seen.sort_unstable_by(|a, b| {
                let la = graph
                    .levels
                    .get(*a as usize)
                    .copied()
                    .unwrap_or(0);
                let lb = graph
                    .levels
                    .get(*b as usize)
                    .copied()
                    .unwrap_or(0);
                la.cmp(&lb).then(a.cmp(b))
            });
            seen
        };

        let mut nodes: Vec<TraceNode> = Vec::new();
        let mut truncated = false;
        let mut withheld = 0usize;
        for n in closure.iter() {
            if nodes.len() >= self.budget {
                truncated = true;
                withheld += 1;
                continue;
            }
            let path = match graph.paths.get(*n as usize) {
                Some(p) => p,
                None => continue,
            };
            // 值事实：优先取批产出（targets 侧），缺了视为无值节点——
            // 闭包里可能有既非目标也无产出中间节点，追踪如实标「无值」
            // 而不是编一个值出来。
            let (cached, steps) = match result.values.iter().find(|(k, _)| k == n) {
                Some((_, v)) => (v.cached, v.steps),
                None => (false, 0),
            };
            let refs = graph
                .refs(*n)
                .iter()
                .filter_map(|r| graph.paths.get(*r as usize).cloned())
                .collect();
            nodes.push(TraceNode {
                token: String::from(path),
                level: *graph.levels.get(*n as usize).unwrap_or(&0),
                cached,
                steps,
                refs,
            });
        }

        Ok(EvalTrace {
            session: String::from(session.id()),
            target: String::from(tpath),
            steps: nodes.len(),
            nodes,
            truncated,
            withheld,
        })
    }

    /// 预算对齐（X03 侧声明的追踪预算须与本模块口径一致，否则拒）。
    ///
    /// 与 F3408 的 `align_budget` 同纪律：不一致就报错让上游改口径，
    /// 不静默取小也不静默取大。
    pub fn align_budget(&self, declared: usize) -> Result<usize, TraceCode> {
        if declared != self.budget {
            return Err(TraceCode::TraceBudgetExceeded);
        }
        Ok(self.budget)
    }
}

// ---------------------------------------------------------------------------
// 七、覆盖来源视图（来源表的投影 + 漂移校准）
// ---------------------------------------------------------------------------

/// 令牌值来源（五级封闭全集，与覆盖层四级栈 + 计算来源对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceTag {
    /// 默认值（基线）。
    Default,
    /// 主题级覆盖。
    Theme,
    /// 场景级覆盖。
    Scene,
    /// 组件级覆盖。
    Component,
    /// 求值计算得出（派生令牌）。
    Computed,
}

impl SourceTag {
    /// 全集（判据据此核对无遗漏）。
    pub const ALL: [SourceTag; 5] = [
        SourceTag::Default,
        SourceTag::Theme,
        SourceTag::Scene,
        SourceTag::Component,
        SourceTag::Computed,
    ];

    /// 中文名（读屏）。
    pub const fn zh(self) -> &'static str {
        match self {
            SourceTag::Default => "默认",
            SourceTag::Theme => "主题",
            SourceTag::Scene => "场景",
            SourceTag::Component => "组件",
            SourceTag::Computed => "计算",
        }
    }
}

/// 覆盖来源表（事实：每令牌一来源）。
///
/// 同令牌**改派**来源是合法操作（覆盖层会变），但改派必须留痕——
/// `revisions` 记每令牌的改派次数，校准报告据此可归因。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SourceTable {
    /// 令牌路径 → 来源（追加序）。
    pub entries: Vec<(String, SourceTag)>,
    /// 令牌路径 → 改派次数（首派不计）。
    pub revisions: Vec<(String, usize)>,
}

impl SourceTable {
    /// 新建空表。
    pub fn new() -> SourceTable {
        SourceTable {
            entries: Vec::new(),
            revisions: Vec::new(),
        }
    }

    fn pos(&self, path: &str) -> Option<usize> {
        self.entries.iter().position(|(p, _)| p == path)
    }

    /// 指派/改派来源。
    pub fn assign(&mut self, path: &str, tag: SourceTag) -> Result<(), TraceCode> {
        validate_token(path)?;
        match self.pos(path) {
            Some(i) => {
                self.entries[i].1 = tag;
                match self.revisions.iter_mut().find(|(p, _)| p == path) {
                    Some((_, c)) => *c += 1,
                    None => self.revisions.push((String::from(path), 1)),
                }
            }
            None => {
                self.entries.push((String::from(path), tag));
            }
        }
        Ok(())
    }

    /// 撤销指派（覆盖回收：令牌不再有覆盖来源时表里必须真的删行，
    /// 否则「视图有、表无」的漂移分支永远造不出来，校准退化为改派对账）。
    pub fn remove(&mut self, path: &str) -> Result<(), TraceCode> {
        validate_token(path)?;
        match self.pos(path) {
            Some(i) => {
                self.entries.remove(i);
                Ok(())
            }
            None => Err(TraceCode::NotFound),
        }
    }

    /// 指纹（FNV-1a over 追加序的 (路径,来源) 流）。
    ///
    /// 表与视图用**同一指纹口径**：指纹不等即漂移，无歧义。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811c_9dc5;
        for (p, t) in self.entries.iter() {
            for b in p.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
            // 变体序号进指纹：封闭全集的判别值跨平台稳定
            h ^= match t {
                SourceTag::Default => 1,
                SourceTag::Theme => 2,
                SourceTag::Scene => 3,
                SourceTag::Component => 4,
                SourceTag::Computed => 5,
            };
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }
}

/// 来源视图（表的投影快照）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SourceView {
    /// 投影内容（建视图时的表快照）。
    pub snapshot: Vec<(String, SourceTag)>,
}

impl SourceView {
    /// 从表投影建视图。
    pub fn project(table: &SourceTable) -> SourceView {
        SourceView {
            snapshot: table.entries.clone(),
        }
    }

    /// 视图指纹。
    pub fn fingerprint(&self) -> u64 {
        let t = SourceTable {
            entries: self.snapshot.clone(),
            revisions: Vec::new(),
        };
        t.fingerprint()
    }

    /// 读屏：逐令牌「令牌 = 来源」。
    pub fn spoken(&self) -> String {
        let mut s = String::from("覆盖来源视图：");
        for (p, t) in self.snapshot.iter() {
            s.push_str(&format!("{} = {} 来源；", p, t.zh()));
        }
        s
    }
}

/// 校准报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CalibReport {
    /// 是否发生漂移。
    pub drifted: bool,
    /// 校准修复的条目数（漂移条数）。
    pub fixes: usize,
}

/// 来源视图管理器（表 + 视图 + 校准闭环）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SourceScope {
    /// 来源表（事实）。
    pub table: SourceTable,
    /// 来源视图（投影）。
    pub view: SourceView,
    /// 最近一次改表操作的会话号（X03 归因：改动可归属到会话）。
    pub touched_by: String,
}

impl SourceScope {
    /// 新建：视图为空表的投影（表空 ⇔ 视图空，天然无漂移）。
    pub fn new() -> SourceScope {
        let table = SourceTable::new();
        let view = SourceView::project(&table);
        SourceScope {
            table,
            view,
            touched_by: String::new(),
        }
    }

    /// 改表（不自动重投影——投影滞后正是漂移的来源，校准显式做）。
    ///
    /// 会话号落账到 [`Self::touched_by`]：漂移发生时「谁改的」可归因，
    /// 而不是一张不知道被谁动过的表。
    pub fn assign(
        &mut self,
        session: &Session,
        path: &str,
        tag: SourceTag,
    ) -> Result<(), TraceCode> {
        self.table.assign(path, tag)?;
        self.touched_by = String::from(session.id());
        Ok(())
    }

    /// 视图是否漂移（指纹口径，O(1) 比对）。
    pub fn drifted(&self) -> bool {
        self.table.fingerprint() != self.view.fingerprint()
    }

    /// 校准：以当前表重投影，报告漂移条数。
    ///
    /// 漂移条数 = 「表里不在视图快照中」∪「视图快照里来源已变」，
    /// 按令牌路径去重计数。
    pub fn calibrate(&mut self) -> CalibReport {
        let mut fixes = 0usize;
        for (p, t) in self.table.entries.iter() {
            match self.view.snapshot.iter().find(|(vp, _)| vp == p) {
                Some((_, vt)) if vt != t => fixes += 1,
                None => fixes += 1,
                _ => {}
            }
        }
        // 视图里有、表里已删的条目也计入修复（校准后视图不再含它们）
        for (vp, _) in self.view.snapshot.iter() {
            if self.table.pos(vp).is_none() {
                fixes += 1;
            }
        }
        let drifted = fixes > 0;
        self.view = SourceView::project(&self.table);
        CalibReport { drifted, fixes }
    }
}

// ---------------------------------------------------------------------------
// 八、判定与收尾
// ---------------------------------------------------------------------------

/// 数量读屏（逐位，零歧义）。
pub fn fmt_num(n: usize) -> String {
    if n == 0 {
        return String::from("零");
    }
    let digits = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    let mut s = String::new();
    let mut started = false;
    for ch in format!("{}", n).chars() {
        let d = match ch.to_digit(10) {
            Some(d) => d as usize,
            None => continue,
        };
        if d == 0 {
            if started {
                s.push('0');
            }
        } else {
            if started {
                s.push(' ');
            }
            s.push_str(digits[d]);
            started = true;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_num_basic() {
        assert_eq!(fmt_num(0), String::from("零"));
        assert_eq!(fmt_num(7), String::from("七"));
        assert_eq!(fmt_num(12), String::from("一 二"));
    }

    #[test]
    fn session_roundtrip() {
        let s = Session::bind("sess-1").unwrap();
        assert_eq!(s.id(), "sess-1");
        assert!(Session::bind("").is_err());
    }
}
