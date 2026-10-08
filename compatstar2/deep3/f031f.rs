//! F031 深化批次四 · 卸载顺序依赖面（compatstar2/deep3 · G-A-31）。
//!
//! 批次一/二/三已覆盖 F031 的三清协议面、执行治理面与残留扫描启发式面；
//! 本批补主册【功能定义】「全语义对齐」的序列化/账本/容错面：组件依赖图
//! 卸载定序（定长 12 节点 × 3 依赖边，逆拓扑序输出——依赖者先卸、被依赖者后卸；
//! 环检出显性 Err）、共享组件引用计数账（归零移除非零保留——MS SharedDLLs 语义）、
//! 占用检出与重启挂起账（定长 8 去重）。
//!
//! 判据对账：主册【状态与异常】「卸载中程序正在运行 → 先请求关闭，拒关
//! 则列明进程让用户选」、【设计细节】「清扫项给用户勾选」未落地面为源，
//! 一处一事实（MS 安装器组件引用计数、PendingFileRenameOperations 语义）。
//!
//! 零堆纪律：定长依赖表/引用账/占用表/挂起集，无 alloc。

use crate::checks::CheckSet;

/// 依赖图节点容量（本批定长口径）。
pub const MAX_NODES: usize = 12;
/// 每节点依赖边容量。
pub const MAX_DEPS: usize = 3;
/// 依赖边空槽哨兵。
pub const NO_DEP: u8 = 0xFF;
/// 共享组件引用账容量。
pub const MAX_SHARED: usize = 8;
/// 在跑进程名表容量（占用检出模型）。
pub const MAX_PROCS: usize = 8;
/// 延迟移除清单容量。
pub const MAX_DEFERRED: usize = 8;
/// 重启挂起标记集容量。
pub const MAX_PENDING: usize = 8;
/// 重启挂起判据种子路径（判例对拍用）。
pub const PENDING_SEEDS: [&'static str; MAX_PENDING] = ["drv/old_a.sys", "drv/old_b.sys", "drv/old_c.sys", "drv/old_d.sys", "drv/old_e.sys", "drv/old_f.sys", "drv/old_g.sys", "drv/old_h.sys"];

/// 卸载组件依赖图：12 节点 × 每节点至多 3 条依赖边（本组件依赖谁）。
pub struct DepGraph {
    present: [bool; MAX_NODES],
    deps: [[u8; MAX_DEPS]; MAX_NODES],
    order: [usize; MAX_NODES],
    /// 已定序节点数。
    pub order_len: usize,
}

impl DepGraph {
    pub const fn new() -> Self {
        DepGraph { present: [false; MAX_NODES], deps: [[NO_DEP; MAX_DEPS]; MAX_NODES], order: [0; MAX_NODES], order_len: 0 }
    }
    /// 登记组件节点；重复登记拒绝。
    pub fn add_node(&mut self, id: usize) -> bool {
        if id >= MAX_NODES || self.present[id] { return false; }
        self.present[id] = true;
        true
    }
    /// 登记依赖边：id 依赖 dep（dep 必须在 id 之后卸载）。重复边拒绝。
    pub fn add_dep(&mut self, id: usize, dep: usize) -> bool {
        if id >= MAX_NODES || dep >= MAX_NODES || id == dep
            || !self.present[id] || !self.present[dep] || self.depends_on(id, dep) {
            return false;
        }
        if let Some(slot) = self.deps[id].iter().position(|&d| d == NO_DEP) {
            self.deps[id][slot] = dep as u8;
            return true;
        }
        false
    }
    /// 查询依赖边存在性（定序校验用）。
    pub fn depends_on(&self, id: usize, dep: usize) -> bool {
        id < MAX_NODES && dep < MAX_NODES && self.deps[id].contains(&(dep as u8))
    }
    /// 逆拓扑卸载定序：每轮摘除「所有依赖它的现存节点都已卸走」的节点；
    /// 一整轮无进展 → 依赖环，显性 Err（零静默——MSI 组件图容错出口）。
    pub fn uninstall_order(&mut self) -> Result<(), &'static str> {
        let mut removed = 0usize;
        while removed < MAX_NODES {
            let mut progressed = false;
            for n in 0..MAX_NODES {
                if !self.present[n] { continue; }
                let blocked = (0..MAX_NODES).any(|m| m != n && self.present[m] && self.deps[m].contains(&(n as u8)));
                if blocked { continue; }
                self.present[n] = false;
                self.order[self.order_len] = n;
                self.order_len += 1;
                removed += 1;
                progressed = true;
            }
            if !progressed { return Err("dep-cycle"); }
        }
        Ok(())
    }
    /// 定序结果视图。
    pub fn order(&self) -> &[usize] { &self.order[..self.order_len] }
}

/// 共享组件引用计数账：归零移除、非零保留记账（MS SharedDLLs：RefCount
/// 减到 0 才物理删除）。
pub struct RefCountLedger {
    names: [&'static str; MAX_SHARED],
    counts: [u32; MAX_SHARED],
    live: [bool; MAX_SHARED],
    /// 归零移除 / 非零保留 / 未知组件释放检出（零静默）三账。
    pub removed: u32,
    pub kept: u32,
    kept_log: [&'static str; MAX_SHARED],
    pub kept_n: usize,
    pub unknown_releases: u32,
}

impl RefCountLedger {
    pub const fn new() -> Self {
        RefCountLedger {
            names: [""; MAX_SHARED], counts: [0; MAX_SHARED], live: [false; MAX_SHARED],
            removed: 0, kept: 0, kept_log: [""; MAX_SHARED], kept_n: 0, unknown_releases: 0,
        }
    }
    /// 登记共享组件并累加初始引用数；零引用登记拒绝。
    pub fn register(&mut self, name: &'static str, refs: u32) -> bool {
        if refs == 0 { return false; }
        for i in 0..MAX_SHARED {
            if self.live[i] && self.names[i] == name { self.counts[i] += refs; return true; }
        }
        for i in 0..MAX_SHARED {
            if !self.live[i] {
                self.names[i] = name;
                self.counts[i] = refs;
                self.live[i] = true;
                return true;
            }
        }
        false
    }
    /// 释放一次引用：归零 → 物理移除（true）；非零 → 保留并记账（false）。
    pub fn release(&mut self, name: &'static str) -> bool {
        for i in 0..MAX_SHARED {
            if self.live[i] && self.names[i] == name {
                if self.counts[i] > 1 {
                    self.counts[i] -= 1;
                    self.kept += 1;
                    if self.kept_n < MAX_SHARED && !self.kept_log[..self.kept_n].contains(&name) {
                        self.kept_log[self.kept_n] = name;
                        self.kept_n += 1;
                    }
                    return false;
                }
                self.live[i] = false;
                self.counts[i] = 0;
                self.removed += 1;
                return true;
            }
        }
        self.unknown_releases += 1;
        false
    }
    /// 当前引用数（未登记为 0）。
    pub fn refcount(&self, name: &'static str) -> u32 {
        for i in 0..MAX_SHARED {
            if self.live[i] && self.names[i] == name { return self.counts[i]; }
        }
        0
    }
    /// 保留记账视图。
    pub fn kept_log(&self) -> &[&'static str] {
        &self.kept_log[..self.kept_n]
    }
}

/// 占用闸：文件路径命中在跑进程名 → 入延迟移除清单；未命中 → 即时移除。
pub struct OccupancyGate {
    procs: [&'static str; MAX_PROCS],
    proc_n: usize,
    deferred: [&'static str; MAX_DEFERRED],
    /// 延迟移除条数 / 即时移除计数。
    pub deferred_n: usize,
    pub removed_files: u32,
}

impl OccupancyGate {
    pub const fn new() -> Self {
        OccupancyGate { procs: [""; MAX_PROCS], proc_n: 0, deferred: [""; MAX_DEFERRED], deferred_n: 0, removed_files: 0 }
    }
    /// 登记在跑进程名（卸载器清单缺失时由进程枚举补齐）。
    pub fn register_proc(&mut self, name: &'static str) -> bool {
        if self.proc_n >= MAX_PROCS { return false; }
        self.procs[self.proc_n] = name;
        self.proc_n += 1;
        true
    }
    /// 卸载检出：命中占用 → 延迟（false）；未命中 → 即时移除（true）。
    pub fn remove_file(&mut self, path: &'static str) -> bool {
        for i in 0..self.proc_n {
            if path.contains(self.procs[i]) {
                if self.deferred_n < MAX_DEFERRED { self.deferred[self.deferred_n] = path; self.deferred_n += 1; }
                return false;
            }
        }
        self.removed_files += 1;
        true
    }
    /// 延迟移除清单视图。
    pub fn deferred_list(&self) -> &[&'static str] {
        &self.deferred[..self.deferred_n]
    }
}

/// 重启挂起标记集：标记/去重/满拒全显性记账（PendingFileRenameOperations：
/// 同路径重复出现不追加）。
pub struct RebootPending {
    marks: [&'static str; MAX_PENDING],
    /// 已挂起条数 / 重复去重计数 / 满容拒绝计数。
    pub n: usize,
    pub dup_marks: u32,
    pub full_rejects: u32,
}

impl RebootPending {
    pub const fn new() -> Self {
        RebootPending { marks: [""; MAX_PENDING], n: 0, dup_marks: 0, full_rejects: 0 }
    }
    /// 标记重启待删；重复路径去重（不追加、计 dup 账）。
    pub fn mark(&mut self, path: &'static str) -> bool {
        for i in 0..self.n {
            if self.marks[i] == path { self.dup_marks += 1; return false; }
        }
        if self.n >= MAX_PENDING { self.full_rejects += 1; return false; }
        self.marks[self.n] = path;
        self.n += 1;
        true
    }
}

/// 域自检（深化批次四）。
pub fn run_f031f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F031-uninstall-order-d4");
    // 1) 12 节点全登记；重复登记拒绝。
    let mut g = DepGraph::new();
    let mut all_added = true;
    for i in 0..MAX_NODES {
        all_added &= g.add_node(i);
    }
    cs.add("graph_full_registration", all_added && !g.add_node(3), "");
    // 2) 链式依赖 i←i+1 的逆拓扑定序恰为 0..12 依序卸载。
    for i in 0..MAX_NODES - 1 {
        g.add_dep(i, i + 1);
    }
    let mut chain_ok = g.uninstall_order().is_ok() && g.order_len == MAX_NODES;
    for p in 0..g.order_len {
        chain_ok &= g.order()[p] == p;
    }
    cs.add("reverse_topo_chain_order", chain_ok, "");
    // 3) 定序尊重依赖边：扇出图中每个节点出现在其全部依赖之后。
    let mut g2 = DepGraph::new();
    for i in 0..MAX_NODES {
        g2.add_node(i);
    }
    g2.add_dep(0, 6);
    g2.add_dep(0, 11);
    g2.add_dep(3, 9);
    g2.add_dep(5, 2);
    g2.add_dep(5, 7);
    g2.add_dep(5, 11);
    let mut respect = g2.uninstall_order().is_ok();
    for p in 0..g2.order_len {
        for d in 0..MAX_NODES {
            if g2.depends_on(g2.order()[p], d) {
                respect &= (p + 1..g2.order_len).any(|q| g2.order()[q] == d);
            }
        }
    }
    cs.add("order_respects_dependencies", respect, "");
    // 4) 依赖环检出 → 显性 Err 且零产出。
    let mut g3 = DepGraph::new();
    for i in 0..3 {
        g3.add_node(i);
    }
    g3.add_dep(0, 1);
    g3.add_dep(1, 2);
    g3.add_dep(2, 0);
    cs.add("dep_cycle_explicit_err", g3.uninstall_order() == Err("dep-cycle") && g3.order_len == 0, "");
    // 5) 引用计数归零 → 移除（MS SharedDLLs 语义）。
    let mut r = RefCountLedger::new();
    r.register("msvcp140.dll", 2);
    let first = r.release("msvcp140.dll");
    cs.add("refcount_zero_removal", !first && r.refcount("msvcp140.dll") == 1 && r.release("msvcp140.dll") && r.removed == 1, "");
    // 6) 引用未归零 → 保留并记账。
    let mut r2 = RefCountLedger::new();
    r2.register("d3dx9_43.dll", 3);
    let _ = r2.release("d3dx9_43.dll");
    let _ = r2.release("d3dx9_43.dll");
    cs.add("refcount_nonzero_kept_ledger", r2.refcount("d3dx9_43.dll") == 1 && r2.kept == 2 && r2.kept_log() == ["d3dx9_43.dll"], "");
    // 7) 未知组件释放检出（零静默）。
    cs.add("refcount_unknown_release_counted", !r2.release("ghost.dll") && r2.unknown_releases == 1, "");
    // 8) 占用检出：命中在跑进程 → 延迟；未命中 → 即时移除。
    let mut o = OccupancyGate::new();
    o.register_proc("game.exe");
    o.register_proc("launcher.exe");
    let kept = o.remove_file("bin/launcher.exe");
    let gone = o.remove_file("logs/inst.log");
    cs.add("occupancy_hit_defers_miss_removes", !kept && gone && o.deferred_n == 1 && o.removed_files == 1, "");
    // 9) 延迟移除清单内容如实。
    let _ = o.remove_file("bin/game.exe");
    cs.add("deferred_list_contents", o.deferred_list() == ["bin/launcher.exe", "bin/game.exe"], "");
    // 10) 重启挂起：重复标记去重 + 满容显性拒绝。
    let mut p = RebootPending::new();
    let m1 = p.mark(PENDING_SEEDS[0]);
    let dup = p.mark(PENDING_SEEDS[0]);
    let mut fills = true;
    for i in 1..MAX_PENDING {
        fills &= p.mark(PENDING_SEEDS[i]);
    }
    cs.add("reboot_pending_dedup_and_full", m1 && !dup && p.dup_marks == 1 && fills && p.n == MAX_PENDING && !p.mark("drv/overflow.sys") && p.full_rejects == 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fanout_order_respects_edges() {
        let mut g = DepGraph::new();
        for i in 0..MAX_NODES { assert!(g.add_node(i)); }
        assert!(g.add_dep(4, 0) && g.add_dep(4, 9) && g.add_dep(9, 2));
        assert!(g.uninstall_order().is_ok());
        let order = g.order();
        let pos = |x: usize| order.iter().position(|&v| v == x).unwrap();
        assert!(pos(4) < pos(0) && pos(4) < pos(9) && pos(9) < pos(2), "依赖者先卸、被依赖者后卸");
    }
    #[test]
    fn refcount_ledger_is_exact() {
        let mut r = RefCountLedger::new();
        assert!(r.register("sh.dll", 1));
        assert!(!r.register("sh.dll", 0), "零引用登记拒绝");
        assert!(r.release("sh.dll"), "归零即移除");
        assert_eq!(r.refcount("sh.dll"), 0);
        assert_eq!(r.removed, 1); assert_eq!(r.kept, 0);
    }
    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f031f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
