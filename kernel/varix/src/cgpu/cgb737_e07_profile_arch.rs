//! CGPU-F0737 · E07 开工与画像架构（目标 410 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥340 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0737，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E07 开工与画像架构——画像子系统（E07 组）的架构骨架：
//!    三层组件注册（采集/聚合/应用）、组件依赖 DAG、开工序
//!    （拓扑序）与架构完整性核查（层间依赖方向、无环、覆盖）。
//!
//! 设计纪律：
//! - 三层闭集：Collect(0)/Aggregate(1)/Apply(2)——依赖只许
//   从高序层指向低序层（应用依赖聚合，聚合依赖采集），
//   同层依赖与逆向依赖都拒绝。
// - 组件定容：COMP_CAP=24；组件名唯一；依赖按组件 id 记。
// - 开工序：Kahn 拓扑排序（确定性：同批按 id 升序出队）；
//   有环即 E_CYCLE（开工失败）。
// - 完整性：每组件依赖全部已注册；至少 1 采集、1 聚合、1 应用
//   才算骨架成立。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4a60 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4a60，批内 CGPU-F0737 独占）。
pub const SEG_BASE: u16 = 0x4a60;

/// 组件重名。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 组件名非法（空）。
pub const E_NAME: u16 = SEG_BASE + 2;
/// 组件账已满。
pub const E_FULL: u16 = SEG_BASE + 3;
/// 层间依赖方向非法。
pub const E_LAYER: u16 = SEG_BASE + 4;
/// 依赖未注册。
pub const E_DEP: u16 = SEG_BASE + 5;
/// 依赖成环（拓扑排序失败）。
pub const E_CYCLE: u16 = SEG_BASE + 6;
/// 骨架不完整（三层缺一）。
pub const E_SKELETON: u16 = SEG_BASE + 7;

/// 组件定容。
pub const COMP_CAP: usize = 24;

/// 架构层（闭集三层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// 采集层。
    Collect,
    /// 聚合层。
    Aggregate,
    /// 应用层。
    Apply,
}

impl Layer {
    /// 层序。
    pub fn ord(self) -> usize {
        match self {
            Layer::Collect => 0,
            Layer::Aggregate => 1,
            Layer::Apply => 2,
        }
    }
    /// 层名。
    pub fn name(self) -> &'static str {
        match self {
            Layer::Collect => "collect",
            Layer::Aggregate => "aggregate",
            Layer::Apply => "apply",
        }
    }
}

/// 架构组件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    /// 组件 id（1 起，注册序）。
    pub id: u32,
    /// 名称。
    pub name: &'static str,
    /// 所属层。
    pub layer: Layer,
    /// 依赖组件 id 表。
    pub deps: Vec<u32>,
}

/// 画像架构注册处。
#[derive(Clone, Debug, Default)]
pub struct ProfileArch {
    comps: Vec<Component>,
}

impl ProfileArch {
    pub fn new() -> Self {
        Self { comps: Vec::new() }
    }
    /// 注册组件：名非空 + 不重名 + 不超容 + 依赖已注册 +
    /// 依赖层序严格低于本层（层间方向闭集）。
    pub fn register(&mut self, name: &'static str, layer: Layer, deps: &[u32]) -> Result<u32, u16> {
        if name.is_empty() {
            return Err(E_NAME);
        }
        if self.comps.len() >= COMP_CAP {
            return Err(E_FULL);
        }
        if self.comps.iter().any(|c| c.name == name) {
            return Err(E_DUP);
        }
        for d in deps {
            let dc = self.comps.iter().find(|c| c.id == *d).ok_or(E_DEP)?;
            if dc.layer.ord() >= layer.ord() {
                return Err(E_LAYER);
            }
        }
        let id = (self.comps.len() + 1) as u32;
        self.comps.push(Component {
            id,
            name,
            layer,
            deps: deps.to_vec(),
        });
        Ok(id)
    }
    /// 组件数。
    pub fn len(&self) -> usize {
        self.comps.len()
    }
    /// 查找。
    pub fn get(&self, id: u32) -> Option<&Component> {
        self.comps.iter().find(|c| c.id == id)
    }
    /// 骨架完整性：三层各 ≥1。
    pub fn skeleton_ok(&self) -> bool {
        for l in [Layer::Collect, Layer::Aggregate, Layer::Apply] {
            if !self.comps.iter().any(|c| c.layer == l) {
                return false;
            }
        }
        true
    }
    /// 开工序：Kahn 拓扑排序，同批按 id 升序（确定性）。
    /// 返回执行序（组件 id 列表）；有环返回 E_CYCLE。
    pub fn boot_order(&self) -> Result<Vec<u32>, u16> {
        let n = self.comps.len();
        let mut indeg = vec![0usize; n + 1];
        // 出边表：dep → 依赖者。
        let mut outs: Vec<Vec<u32>> = vec![Vec::new(); n + 1];
        for c in &self.comps {
            indeg[c.id as usize] = c.deps.len();
            for d in &c.deps {
                outs[*d as usize].push(c.id);
            }
        }
        let mut ready: Vec<u32> = (1..=n as u32)
            .filter(|i| indeg[*i as usize] == 0)
            .collect();
        let mut order = Vec::with_capacity(n);
        while !ready.is_empty() {
            // 取 id 最小（ready 保持升序：出队头）。
            ready.sort_unstable();
            let id = ready.remove(0);
            order.push(id);
            for &nxt in &outs[id as usize] {
                indeg[nxt as usize] -= 1;
                if indeg[nxt as usize] == 0 {
                    ready.push(nxt);
                }
            }
        }
        if order.len() != n {
            return Err(E_CYCLE);
        }
        Ok(order)
    }
    /// 依赖闭包完整性：每组件依赖全已注册（注册时保证，
    /// 此处复算防未来直改账）。
    pub fn deps_intact(&self) -> bool {
        for c in &self.comps {
            for d in &c.deps {
                if self.get(*d).is_none() {
                    return false;
                }
            }
        }
        true
    }
    /// 层方向复核：全账依赖层序严格递减。
    pub fn layer_dirs_ok(&self) -> bool {
        for c in &self.comps {
            for d in &c.deps {
                if let Some(dc) = self.get(*d) {
                    if dc.layer.ord() >= c.layer.ord() {
                        return false;
                    }
                } else {
                    return false;
                }
            }
        }
        true
    }
    /// 层计数视图。
    pub fn layer_counts(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for c in &self.comps {
            out[c.layer.ord()] += 1;
        }
        out
    }
}

/// 架构报告行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchRow {
    /// 组件 id。
    pub id: u32,
    /// 名称。
    pub name: &'static str,
    /// 层名。
    pub layer: &'static str,
    /// 依赖数。
    pub deps: usize,
}

/// 开工报告：序 + 每组件行（按开工序）。
pub fn boot_report(arch: &ProfileArch) -> Result<Vec<ArchRow>, u16> {
    if !arch.skeleton_ok() {
        return Err(E_SKELETON);
    }
    if !arch.deps_intact() || !arch.layer_dirs_ok() {
        return Err(E_LAYER);
    }
    let order = arch.boot_order()?;
    let mut rows = Vec::with_capacity(order.len());
    for id in order {
        let c = arch.get(id).ok_or(E_DEP)?;
        rows.push(ArchRow {
            id,
            name: c.name,
            layer: c.layer.name(),
            deps: c.deps.len(),
        });
    }
    Ok(rows)
}

/// 标准画像架构样例（5 采集 → 2 聚合 → 2 应用，依赖关系固定）。
pub fn standard_arch() -> Result<ProfileArch, u16> {
    let mut a = ProfileArch::new();
    let f1 = a.register("dom_events", Layer::Collect, &[])?;
    let f2 = a.register("style_ticks", Layer::Collect, &[])?;
    let f3 = a.register("net_timing", Layer::Collect, &[])?;
    let f4 = a.register("gpu_frames", Layer::Collect, &[])?;
    let f5 = a.register("script_costs", Layer::Collect, &[])?;
    let a1 = a.register("page_profile", Layer::Aggregate, &[f1, f2])?;
    let a2 = a.register("budget_model", Layer::Aggregate, &[f3, f4, f5])?;
    a.register("sched_driver", Layer::Apply, &[a1, a2])?;
    a.register("telemetry_out", Layer::Apply, &[a1])?;
    Ok(a)
}

/// 自例程：主路径（标准架构→开工序→报告）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 主路径：标准架构。
    let a = standard_arch()?;
    if a.len() != 9 {
        return Err(E_SKELETON);
    }
    if !a.skeleton_ok() || !a.deps_intact() || !a.layer_dirs_ok() {
        return Err(E_SKELETON);
    }
    if a.layer_counts() != [5, 2, 2] {
        return Err(E_SKELETON);
    }
    let order = a.boot_order()?;
    if order.len() != 9 {
        return Err(E_CYCLE);
    }
    // 开工序约束：任何组件在其全部依赖之后。
    for (pos, id) in order.iter().enumerate() {
        let c = a.get(*id).ok_or(E_DEP)?;
        for d in &c.deps {
            let dpos = order.iter().position(|x| x == d).ok_or(E_DEP)?;
            if dpos >= pos {
                return Err(E_CYCLE);
            }
        }
    }
    // 同批 id 升序（前 5 应为 1..=5 采集层）。
    if order[0..5] != [1, 2, 3, 4, 5] {
        return Err(E_CYCLE);
    }
    let rows = boot_report(&a)?;
    if rows.len() != 9 || rows[0].layer != "collect" {
        return Err(E_SKELETON);
    }
    // 错误路径 0：重名。
    let mut bad = ProfileArch::new();
    bad.register("x", Layer::Collect, &[])?;
    if bad.register("x", Layer::Collect, &[]) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 错误路径 1：空名。
    if bad.register("", Layer::Collect, &[]) != Err(E_NAME) {
        return Err(E_NAME);
    }
    // 错误路径 2：依赖未注册。
    if bad.register("y", Layer::Aggregate, &[99]) != Err(E_DEP) {
        return Err(E_DEP);
    }
    // 错误路径 3：逆向依赖（Aggregate 依赖 Apply）。
    let app = bad.register("a1", Layer::Apply, &[])?;
    // Apply 无依赖可注册（deps 空）；再注册 Aggregate 依赖该 Apply。
    if bad.register("g1", Layer::Aggregate, &[app]) != Err(E_LAYER) {
        return Err(E_LAYER);
    }
    // 错误路径 4：同层依赖。
    let c1 = bad.register("c1", Layer::Collect, &[])?;
    if bad.register("c2", Layer::Collect, &[c1]) != Err(E_LAYER) {
        return Err(E_LAYER);
    }
    // 错误路径 5：成环——构造环需要绕过层方向约束，
    // 环在同层内不可能（同层依赖被拒），环必经逆向依赖（也被拒）；
    // 因此本架构约束下环不可构造——以 boot_order 对手工账检验：
    // 直接构造带环的注册处不可行（register 拦截），
    // 退而对空依赖闭包（自依赖 id 自指）做防御性检验：
    let mut cyc = ProfileArch::new();
    let id = cyc.register("self_dep", Layer::Collect, &[])?;
    // 手工注入自依赖（模拟账面被直改的防御路径）。
    if let Some(c) = cyc.comps.iter_mut().find(|c| c.id == id) {
        c.deps.push(id);
    }
    if cyc.boot_order() != Err(E_CYCLE) {
        return Err(E_CYCLE);
    }
    if !cyc.deps_intact() {
        // 自依赖仍指向已注册 id → intact 为 true（符合定义）。
    }
    // 错误路径 6：骨架不完整（只有采集层）。
    let mut skel = ProfileArch::new();
    skel.register("only_collect", Layer::Collect, &[])?;
    if boot_report(&skel) != Err(E_SKELETON) {
        return Err(E_SKELETON);
    }
    Ok(0)
}
