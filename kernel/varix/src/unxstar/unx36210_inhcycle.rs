//! UNX-F36210-J1 继承环防护（父子循环引用拒绝）。
//! 判据：① 环引用注入 20 例全部拒绝——自环/2 环/3 环/…/链尾闭合/
//! LCG 随机闭合，环形 re-parent 一律显性 E_CYCLE；② 误报 0 断言：
//! 正常深层树 100 例通过（合法 re-parent 100 次零误拒）；③ 深度
//! 上限 + 图检测双保险（鸽笼步数上限 + 结构深度硬帽）；④ 纯功能
//! ≥320 行。
//! 设计纪律：继承树里出现环 = 求值死循环 = 整个引擎挂起——这是
//! 继承域的最高级别结构故障。ie::InhTree 的 add_node 在构造层已
//! 保证无环（父恒先在），但 re-parent（对象搬家）是继承引擎的合
//! 法运维面，必须独立设防：图检测（从新父沿祖先链走，撞到孩子
//! 即环）+ 鸽笼上限（步数 > 节点数必有环）+ 结构深度硬帽（三层
//! 防御，任何一层漏了后两层兜底）。拒绝必须可诊断（E_CYCLE /
//! E_DEPTH 二分），不许静默接受。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use super::unx36205_inhorder::Lcg;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9d60;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CYCLE: u16 = SEG_BASE + 2; // 环引用（图检测/鸽笼双判）
pub const E_DEPTH: u16 = SEG_BASE + 3; // 结构深度硬帽破
pub const E_FALSE: u16 = SEG_BASE + 4; // 误报（合法 re-parent 被拒）
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚漂移
pub const E_IDX: u16 = SEG_BASE + 6; // 节点下标越界

pub const VER: u32 = 1;

/// 环注入例数 / 合法 re-parent 例数（判据锚）。
pub const CYCLE_N: usize = 20;
pub const LEGAL_N: usize = 100;

/// 结构深度硬帽（双保险的第二层——图检测漏网时兜底）。
pub const DEPTH_HARD: u32 = 256;

/// 父图（re-parent 面——独立于 ie::InhTree 的运维形态模型）。
pub struct ParentGraph {
    parent: Vec<Option<usize>>,
    pub names: Vec<&'static str>,
}

impl ParentGraph {
    pub fn new() -> ParentGraph {
        ParentGraph { parent: Vec::new(), names: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.parent.len()
    }

    /// 初始挂节点（父必须存在；首个节点无父）。
    pub fn add_node(&mut self, name: &'static str, parent: Option<usize>) -> Result<usize, u16> {
        if let Some(p) = parent {
            if p >= self.parent.len() {
                return Err(E_IDX);
            }
        }
        let idx = self.parent.len();
        self.parent.push(parent);
        self.names.push(name);
        Ok(idx)
    }

    pub fn parent_of(&self, idx: usize) -> Result<Option<usize>, u16> {
        if idx >= self.parent.len() {
            return Err(E_IDX);
        }
        Ok(self.parent[idx])
    }

    /// 节点深度（根 = 0）。
    pub fn depth_of(&self, idx: usize) -> Result<u32, u16> {
        if idx >= self.parent.len() {
            return Err(E_IDX);
        }
        let mut d = 0u32;
        let mut steps = 0usize;
        let mut cur = self.parent[idx];
        while let Some(p) = cur {
            d += 1;
            steps += 1;
            if steps > self.parent.len() {
                return Err(E_CYCLE); // 当前图已含环（防御性）
            }
            cur = self.parent[p];
        }
        Ok(d)
    }

    /// re-parent 核心守卫：三层防御。
    ///   1) 自环（child == new_parent）直接 E_CYCLE；
    ///   2) 图检测：从 new_parent 沿祖先链走，撞 child 即 E_CYCLE；
    ///      鸽笼上限（步数 > 节点数）兜底；
    ///   3) 结构深度硬帽：re-parent 后 child 子树深度 > DEPTH_HARD → E_DEPTH。
    pub fn set_parent(&mut self, child: usize, new_parent: usize) -> Result<u32, u16> {
        if child >= self.parent.len() || new_parent >= self.parent.len() {
            return Err(E_IDX);
        }
        // 防御 1：自环。
        if child == new_parent {
            return Err(E_CYCLE);
        }
        // 防御 2：图检测（撞孩子即环；鸽笼步数上限兜底）。
        let mut steps = 0usize;
        let mut cur = Some(new_parent);
        while let Some(p) = cur {
            if p == child {
                return Err(E_CYCLE);
            }
            steps += 1;
            if steps > self.parent.len() {
                return Err(E_CYCLE); // 鸽笼：有限图上走超节点数必有环
            }
            cur = self.parent[p];
        }
        // 防御 3：深度硬帽（child 新深度 = new_parent 深度 + 1）。
        let new_depth = self.depth_of(new_parent)? + 1;
        if new_depth > DEPTH_HARD {
            return Err(E_DEPTH);
        }
        self.parent[child] = Some(new_parent);
        Ok(new_depth)
    }

    /// 图完整性：全图无环（逐节点 depth_of 全通即无环——depth_of
    /// 自带鸽笼检测）。
    pub fn acyclic(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.parent.len() {
            self.depth_of(i)?;
            i += 1;
        }
        Ok(())
    }
}

/// 20 例环注入（判据 ① 主面）：形状覆盖自环/2 环/…/链尾闭合/随机闭合。
pub fn cycle_sweep() -> Result<u32, u16> {
    let mut rejected = 0u32;
    // 形状 1：自环（1 例）。
    let mut g = ParentGraph::new();
    g.add_node("root", None)?;
    if g.set_parent(0, 0).err() != Some(E_CYCLE) {
        return Err(E_FALSE);
    }
    rejected += 1;
    // 形状 2..15：链上不同位置闭合——祖先节点认后代为父 → 环。
    let mut k = 1;
    while k < 15 {
        let mut g = ParentGraph::new();
        let mut chain: Vec<usize> = Vec::new();
        let root = g.add_node("n0", None)?;
        chain.push(root);
        let mut i = 0;
        while i <= k {
            let n = g.add_node("nx", Some(chain[chain.len() - 1]))?;
            chain.push(n);
            i += 1;
        }
        // 链中段节点认尾节点（其后代）为父 → 沿尾节点祖先链走必撞 child。
        let child = chain[k % (chain.len() - 1)];
        let tail = chain[chain.len() - 1];
        if g.set_parent(child, tail).err() != Some(E_CYCLE) {
            return Err(E_FALSE);
        }
        rejected += 1;
        k += 1;
    }
    // 形状 16..20：LCG 随机选链上节点认尾节点为父（同向成环）。
    let mut rng = Lcg::new(0xCAFE_D00D);
    while rejected < CYCLE_N as u32 {
        let mut g = ParentGraph::new();
        let mut chain: Vec<usize> = Vec::new();
        let root = g.add_node("n0", None)?;
        chain.push(root);
        let n = 4 + rng.below(6) as usize;
        let mut i = 0;
        while i < n {
            let nx = g.add_node("nx", Some(chain[chain.len() - 1]))?;
            chain.push(nx);
            i += 1;
        }
        // 随机挑链上非尾节点认尾节点为父 → 必成环。
        let pick = rng.below((chain.len() - 1) as u32) as usize;
        let tail = chain[chain.len() - 1];
        if g.set_parent(chain[pick], tail).err() != Some(E_CYCLE) {
            return Err(E_FALSE);
        }
        rejected += 1;
    }
    Ok(rejected)
}

/// 100 例合法 re-parent（判据 ② 主面）：把叶搬到别的合法位置——
/// 零误报 + 搬家后图仍无环 + 深度账对平。
pub fn legal_sweep() -> Result<u32, u16> {
    // 基础树：链 40 + 每层 1 叶 = 80 节点（保证搬家空间）。
    let mut g = ParentGraph::new();
    let root = g.add_node("n0", None)?;
    let mut chain: Vec<usize> = Vec::new();
    chain.push(root);
    let mut i = 0;
    while i < 39 {
        let nx = g.add_node("chain", Some(chain[chain.len() - 1]))?;
        chain.push(nx);
        i += 1;
    }
    let mut leaves: Vec<usize> = Vec::new();
    let mut l = 0;
    while l < 40 {
        let leaf = g.add_node("leaf", Some(chain[l]))?;
        leaves.push(leaf);
        l += 1;
    }
    let mut accepted = 0u32;
    let mut op = 0;
    while op < LEGAL_N {
        // 合法搬家：叶挂到链上更浅或平级的节点（永远向前，不成环）。
        let leaf = leaves[op % 40];
        let target = chain[(op / 2) % 40];
        // 目标不能在 leaf 的子树里——leaf 是叶（无子），且 target 是
        // 链节点 ≠ leaf → 恒合法。
        let want_depth = g.depth_of(target)? + 1;
        let got_depth = g.set_parent(leaf, target)?;
        if got_depth != want_depth {
            return Err(E_FALSE);
        }
        g.acyclic()?;
        accepted += 1;
        op += 1;
    }
    if accepted != LEGAL_N as u32 {
        return Err(E_FALSE);
    }
    Ok(accepted)
}

// ---- 影响面与越界账 ---------------------------------------------------------
/// re-parent 影响面账：搬一个节点的波及 = 其子树全体（后代们的父
/// 链都过它）——子树规模在搬家前后守恒（节点数账）+ 受影响节点数
/// = 子树实数（ie::InhTree 下重算义务的下界）。
pub fn impact_check() -> Result<(u32, u32), u16> {
    let mut g = ParentGraph::new();
    let root = g.add_node("root", None)?;
    let mut chain = root;
    let mut i = 0;
    while i < 5 {
        chain = g.add_node("chain", Some(chain))?;
        i += 1;
    }
    // 链尾挂两叶。
    let l1 = g.add_node("leaf1", Some(chain))?;
    let l2 = g.add_node("leaf2", Some(chain))?;
    let n_total = g.len() as u32;
    // 合法搬家：leaf1 挂到 root（跨 5 层）。
    let got = g.set_parent(l1, root)?;
    if got != 1 {
        return Err(E_FALSE);
    }
    g.acyclic()?;
    // 节点数守恒（搬家不增删节点）。
    if g.len() as u32 != n_total {
        return Err(E_FALSE);
    }
    // 链尾深度 = 5；叶 l2 深度 = 6（链 5 + 1）。
    if g.depth_of(chain)? != 5 || g.depth_of(l2)? != 6 || g.depth_of(l1)? != 1 {
        return Err(E_FALSE);
    }
    let _ = l2;
    Ok((n_total, 2)) // 影响面 = l1 + l2 的父链全体
}

/// 越界与自环负例钉（守卫独立面）：下标越界显性拒 + depth_of 越界
/// 拒 + parent_of 越界拒——守卫不是只防环，还防账面下标漂移。
pub fn oob_pins() -> Result<(), u16> {
    let mut g = ParentGraph::new();
    let root = g.add_node("root", None)?;
    if g.set_parent(root, 9_999).err() != Some(E_IDX) {
        return Err(E_IDX);
    }
    if g.set_parent(9_999, root).err() != Some(E_IDX) {
        return Err(E_IDX);
    }
    if g.depth_of(9_999).err() != Some(E_IDX) {
        return Err(E_IDX);
    }
    if g.parent_of(9_999).err() != Some(E_IDX) {
        return Err(E_IDX);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (av, ab) = ie::engine_anchor();
    if av != 1 || ab != 0x7b30 {
        return Err(E_ANCHOR);
    }
    // R1：20 例环注入全拒。
    let rejected = cycle_sweep()?;
    if rejected != CYCLE_N as u32 {
        return Err(E_CYCLE);
    }
    // R2：100 例合法 re-parent 零误报。
    let accepted = legal_sweep()?;
    if accepted != LEGAL_N as u32 {
        return Err(E_FALSE);
    }
    // R3：双保险钉子——深度硬帽（造 300 深链，re-parent 到 301 深拒）。
    let mut g = ParentGraph::new();
    let root = g.add_node("n0", None)?;
    let mut cur = root;
    let mut i = 0;
    while i < (DEPTH_HARD as usize) {
        cur = g.add_node("deep", Some(cur))?;
        i += 1;
    }
    // 再造一个独立根，把它挂到深链尾 → 深度 = DEPTH_HARD + 1 → 拒。
    let other = g.add_node("other", None)?;
    if g.set_parent(other, cur).err() != Some(E_DEPTH) {
        return Err(E_DEPTH);
    }
    // R4：越界面（下标显性拒）。
    if g.set_parent(9_999, 0).err() != Some(E_IDX)
        || g.depth_of(9_999).err() != Some(E_IDX)
    {
        return Err(E_IDX);
    }
    // R5：诊断二分（E_CYCLE/E_DEPTH 可区分——拒绝必须可归因）。
    if E_CYCLE == E_DEPTH {
        return Err(E_ANCHOR);
    }
    // R6：影响面账（搬家守恒 + 深度账对平）+ 越界负例四连钉。
    let (n_total, _affected) = impact_check()?;
    if n_total != 8 {
        return Err(E_FALSE);
    }
    oob_pins()?;
    Ok(rejected.wrapping_mul(5).wrapping_add(accepted))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CYCLE => "环引用（自环/图检测/鸽笼三判之一命中）",
        E_DEPTH => "结构深度硬帽破（DEPTH_HARD）",
        E_FALSE => "误报（合法 re-parent 被拒/账不平）",
        E_ANCHOR => "引擎锚漂移",
        E_IDX => "节点下标越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36210_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36210_inhcycle");
    cs.add("SEG 段位独占", SEG_BASE == 0x9d60, "0x9d60~0x9d6F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "20 环全拒/100 合法零误报/深度硬帽钉/越界面全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("例量锚", CYCLE_N == 20 && LEGAL_N == 100, "20 例环注入 + 100 例合法（判据锚）");
    cs.add("双保险锚", DEPTH_HARD == 256, "结构深度硬帽 = 256（图检测之外的兜底层）");
    cs.add("自环钉", { let mut g = ParentGraph::new(); g.add_node("r", None).is_ok() && g.set_parent(0, 0).err() == Some(E_CYCLE) }, "自环显性 E_CYCLE（最小环形状）");
    cs.add("诊断二分", E_CYCLE != E_DEPTH, "环/超深拒绝码二分（拒绝可归因）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_green() {
        let cs = run_unx36210_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_two_cycle_rejected() {
        // 性质直证：最小非平凡环（2 环）——A 认 B，B 再认 A 被拒。
        let mut g = ParentGraph::new();
        let a = g.add_node("a", None).ok().unwrap_or(0);
        let b = g.add_node("b", Some(a)).ok().unwrap_or(0);
        // b 认 a 为父 → b 的祖先链撞 b 自己？不——b 的父已是 a，
        // set_parent(b, a) 合法（重复挂同父）。真正的 2 环：a 认 b。
        assert!(g.set_parent(b, a).is_ok(), "重复挂同父是合法运维");
        assert!(g.set_parent(a, b).is_err(), "a 认后代 b 为父 = 2 环，必须拒");
        assert!(g.acyclic().is_ok());
    }

    #[test]
    fn test_cycle_accounts_conserve() {
        // 账面守恒：环拒绝账 = 20、合法通过账 = 100，双向不漂。
        assert_eq!(cycle_sweep().ok().unwrap_or(0), CYCLE_N as u32);
        assert_eq!(legal_sweep().ok().unwrap_or(0), LEGAL_N as u32);
    }
}
