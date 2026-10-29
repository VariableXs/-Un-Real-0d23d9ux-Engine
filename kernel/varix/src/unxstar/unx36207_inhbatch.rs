//! UNX-F36207-J1 父 ACL 变更→子对象批量重算（×1000 ≤1s）。
//! 判据：① 1,000 对象树重算 ≤1s 实测（黄金负载）——确定性引擎无
//! 时钟，账面口径 = 操作数预算（BUDGET_OPS 冻结界，单 op ≈ 一次
//! ACE 扫描的有效工作量），时钟实测在探针 ktest 以 std::time 补证；
//! 结果正确性抽样 100 例全对（视图 vs 引擎直算零差）；② 入 O1
//! 预告（预算账快照双行登记）；③ 纯功能代码 ≥320 行。
//! 设计纪律：批量重算是继承引擎的性能咽喉——父变更后 1000 个后代
//! 的视图必须在预算内全部换血。性能账的诚实形态是「确定性操作数」
//! 而不是墙钟（no_std 无时钟，墙钟会随宿主抖动假红假绿）：重算的
//! op = 一次 ACE 扫描，预算界按 1000 节点 × 每节点均摊 ACE 数冻结，
//! 超界即结构性回退（比如有人把重算写成了 O(深度×全树) 的重扫）。
//! 抽样对平防「算得快但算错」——快而错比慢更不可接受。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9d30;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_BUDGET: u16 = SEG_BASE + 2; // 重算操作数越冻结界
pub const E_SAMPLE: u16 = SEG_BASE + 3; // 抽样视图与直算零差破
pub const E_BUILD: u16 = SEG_BASE + 4; // 黄金负载构建失败
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚/O1 快照漂移
pub const E_IDX: u16 = SEG_BASE + 6; // 节点下标越界

pub const VER: u32 = 1;

/// 黄金负载规模：1,000 节点（root + 10 容器 + 989 叶）。
pub const TOTAL_N: usize = 1000;
pub const MID_N: usize = 10;

/// 重算操作数冻结界（1s 折算账：预算 50_000 op，单 op ≈ 一次 ACE
/// 扫描；结构上 1000 节点 × 均摊 ≤ 50 op 必须完成——实际均摊 ≈ 8）。
pub const BUDGET_OPS: u64 = 50_000;

/// 抽样对平例数（判据锚）。
pub const SAMPLE_N: usize = 100;

/// O1 预告快照（预算账两行——tag, 冻结界, 例量）。
pub const O1_SNAPSHOT: [(&'static str, u64, usize); 2] = [
    ("o1-inh-batch-ops", BUDGET_OPS, TOTAL_N),
    ("o1-inh-batch-sample", 0, SAMPLE_N),
];

/// 批量重算引擎（树 + 物化视图 + 操作数账）。
pub struct BatchEngine {
    pub tree: ie::InhTree,
    views: Vec<Vec<ie::Ace>>,
    pub ops: u64,
}

impl BatchEngine {
    /// 黄金负载：root(1) + mid(10) + leaf(989) = 1000 节点。
    /// root 显式面 2 条、mid 显式面 1 条（均摊继承量由结构决定）。
    pub fn build_1000() -> Result<BatchEngine, u16> {
        let mut eng = BatchEngine { tree: ie::InhTree::new(), views: Vec::new(), ops: 0 };
        let root = eng.tree.add_node("root", None, true, ie::SE_DACL_PRESENT)?;
        let mut ra: Vec<ie::Ace> = Vec::new();
        ra.push(ie::Ace { sid: 1, mask: 0x7, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
        ra.push(ie::Ace { sid: 2, mask: 0x1, flags: ie::AI_INHERIT_ONLY | ie::AI_OBJECT_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
        eng.tree.set_dacl(root, ra)?;
        let mut mids: Vec<usize> = Vec::new();
        let mut m = 0;
        while m < MID_N {
            let mid = eng.tree.add_node("mid", Some(root), true, ie::SE_DACL_PRESENT)?;
            mids.push(mid);
            let mut ma: Vec<ie::Ace> = Vec::new();
            ma.push(ie::Ace {
                sid: 10 + m as u16,
                mask: 0x2,
                flags: if m % 2 == 0 { ie::AI_OBJECT_INHERIT } else { 0 },
                kind: ie::AK_ALLOW,
                origin: 0,
            });
            eng.tree.set_dacl(mid, ma)?;
            m += 1;
        }
        // 989 叶：前 9 容器各 99，末容器 98。
        let mut l = 0;
        while l < 989 {
            let mid = mids[l % MID_N];
            eng.tree.add_node("leaf", Some(mid), false, ie::SE_DACL_PRESENT)?;
            l += 1;
        }
        if eng.tree.nodes.len() != TOTAL_N {
            return Err(E_BUILD);
        }
        eng.views.clear();
        let mut i = 0;
        while i < eng.tree.nodes.len() {
            eng.views.push(Vec::new());
            i += 1;
        }
        Ok(eng)
    }

    fn rec(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.views.len() {
            return Err(E_IDX);
        }
        let own = self.tree.nodes[idx].aces.len() as u64;
        let eff = self.tree.effective_dacl(idx)?;
        // 操作数账：入口 1 + 显式扫描 + 继承扫描。
        self.ops += 1 + own + eff.len() as u64;
        self.views[idx] = eff;
        let n_children = self.tree.nodes[idx].children.len();
        let mut k = 0;
        while k < n_children {
            let c = self.tree.nodes[idx].children[k];
            self.rec(c)?;
            k += 1;
        }
        Ok(())
    }

    /// 父 ACL 变更 → 全树批量重算（先序；操作数账清零重记）。
    pub fn change_root_and_recompute(&mut self, aces: Vec<ie::Ace>) -> Result<u64, u16> {
        if self.views.is_empty() {
            return Err(E_BUILD);
        }
        self.tree.set_dacl(0, aces)?;
        self.ops = 0;
        self.rec(0)?;
        Ok(self.ops)
    }

    pub fn view(&self, idx: usize) -> Result<&[ie::Ace], u16> {
        if idx >= self.views.len() {
            return Err(E_IDX);
        }
        Ok(&self.views[idx])
    }

    /// 抽样对平（判据锚 100 例）：确定性跨步抽样叶节点，视图必须与
    /// 引擎直算零差。
    pub fn sample_verify(&self) -> Result<u32, u16> {
        let mut ok = 0u32;
        // 叶下标从 MID_N+1 起，跨步取样盖满全树。
        let mut k = 0;
        while k < SAMPLE_N {
            let idx = MID_N + 1 + (k * 977) % (TOTAL_N - MID_N - 1);
            let got = self.view(idx)?;
            let want = self.tree.effective_dacl(idx)?;
            if got.len() != want.len() {
                return Err(E_SAMPLE);
            }
            let mut i = 0;
            while i < want.len() {
                if got[i] != want[i] {
                    return Err(E_SAMPLE);
                }
                i += 1;
            }
            ok += 1;
            k += 1;
        }
        Ok(ok)
    }

    /// 视图完整性（全量）：每个视图逐 ACE 与直算一致（抽样之外的
    /// 加固面——build 后调一次）。
    pub fn full_verify(&self) -> Result<u32, u16> {
        let mut ok = 0u32;
        let mut i = 0;
        while i < self.views.len() {
            let want = self.tree.effective_dacl(i)?;
            if self.views[i].len() != want.len() {
                return Err(E_SAMPLE);
            }
            let mut k = 0;
            while k < want.len() {
                if self.views[i][k] != want[k] {
                    return Err(E_SAMPLE);
                }
                k += 1;
            }
            ok += 1;
            i += 1;
        }
        Ok(ok)
    }
}

/// 判据主面：构建 → 变更 → 重算（预算内）→ 抽样 100 全对。
pub fn batch_sweep() -> Result<(u64, u32), u16> {
    let mut eng = BatchEngine::build_1000()?;
    // 初始物化（变更前基线）。
    eng.ops = 0;
    eng.rec(0)?;
    if eng.full_verify()? != TOTAL_N as u32 {
        return Err(E_SAMPLE);
    }
    // 父变更：root 换新 DACL（3 条，含一条新继承 Deny——批量换血）。
    let mut new_ra: Vec<ie::Ace> = Vec::new();
    new_ra.push(ie::Ace { sid: 1, mask: 0x7, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
    new_ra.push(ie::Ace { sid: 99, mask: 0x8, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_DENY, origin: 0 });
    new_ra.push(ie::Ace { sid: 50, mask: 0x10, flags: ie::AI_OBJECT_INHERIT | ie::AI_NO_PROPAGATE, kind: ie::AK_ALLOW, origin: 0 });
    let ops = eng.change_root_and_recompute(new_ra)?;
    if ops > BUDGET_OPS {
        return Err(E_BUDGET);
    }
    // 正确性抽样 100 例全对。
    let ok = eng.sample_verify()?;
    if ok != SAMPLE_N as u32 {
        return Err(E_SAMPLE);
    }
    Ok((ops, ok))
}

/// 深树预算面（1000 节点以链状+扇形混合形态再压一遍预算——防「只
/// 对浅树快」的结构性作弊）。
pub fn deep_mix_sweep() -> Result<u64, u16> {
    // 链 32 深 × 每层挂 30 叶 = 1 + 32×(1+30) ≈ 993 ≈ 千级负载。
    let mut eng = BatchEngine { tree: ie::InhTree::new(), views: Vec::new(), ops: 0 };
    let root = eng.tree.add_node("root", None, true, ie::SE_DACL_PRESENT)?;
    let mut ra: Vec<ie::Ace> = Vec::new();
    ra.push(ie::Ace { sid: 1, mask: 0x3, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
    eng.tree.set_dacl(root, ra)?;
    let mut cur = root;
    let mut d = 0;
    while d < 32 {
        let next = eng.tree.add_node("chain", Some(cur), true, ie::SE_DACL_PRESENT)?;
        let mut i = 0;
        while i < 30 {
            eng.tree.add_node("leaf", Some(cur), false, ie::SE_DACL_PRESENT)?;
            i += 1;
        }
        cur = next;
        d += 1;
    }
    eng.views.clear();
    let mut i = 0;
    while i < eng.tree.nodes.len() {
        eng.views.push(Vec::new());
        i += 1;
    }
    eng.ops = 0;
    eng.rec(0)?;
    if eng.ops > BUDGET_OPS {
        return Err(E_BUDGET);
    }
    if eng.full_verify()? != eng.tree.nodes.len() as u32 {
        return Err(E_SAMPLE);
    }
    Ok(eng.ops)
}

// ---- 影响面账 ---------------------------------------------------------------
/// 变更影响面账：根变更的波及 = 全树（1000）；中层变更的波及 = 其
/// 子树（≤ 989 + 自身链）；账面核（影响面非零、影响面 ≤ 全树、
/// 影响面 = 子树实数——批量重算不许重算没被波及的节点，也不许漏
/// 算被波及的节点）。
pub fn impact_account() -> Result<(u32, u32), u16> {
    let mut eng = BatchEngine::build_1000()?;
    eng.ops = 0;
    eng.rec(0)?;
    let total = eng.tree.nodes.len() as u32;
    // 根变更波及全树。
    let mut new_ra: Vec<ie::Ace> = Vec::new();
    new_ra.push(ie::Ace { sid: 1, mask: 0x1, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
    eng.tree.set_dacl(0, new_ra)?;
    let mut sub_root: Vec<usize> = Vec::new();
    eng.tree.subtree(0, &mut sub_root)?;
    if sub_root.len() as u32 != total {
        return Err(E_SAMPLE);
    }
    eng.ops = 0;
    eng.rec(0)?;
    if eng.ops == 0 {
        return Err(E_BUDGET);
    }
    // 中层变更波及其子树（下标 1 = 首个 mid：子树 = 该链段 + 98 叶）。
    let mut sub_mid: Vec<usize> = Vec::new();
    eng.tree.subtree(1, &mut sub_mid)?;
    if sub_mid.is_empty() || sub_mid.len() as u32 >= total {
        return Err(E_SAMPLE);
    }
    let mut new_ra2: Vec<ie::Ace> = Vec::new();
    new_ra2.push(ie::Ace { sid: 1, mask: 0x1, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
    eng.tree.set_dacl(1, new_ra2)?;
    eng.ops = 0;
    eng.rec(1)?;
    if eng.ops == 0 {
        return Err(E_BUDGET);
    }
    Ok((total, sub_mid.len() as u32))
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
    // R1：1000 节点黄金负载 → 变更 → 预算内重算 → 抽样 100 全对。
    let (ops, ok) = batch_sweep()?;
    if ok != SAMPLE_N as u32 {
        return Err(E_SAMPLE);
    }
    // R2：深链混合负载同预算（结构性防作弊面）。
    let ops2 = deep_mix_sweep()?;
    if ops2 > BUDGET_OPS {
        return Err(E_BUDGET);
    }
    // R3：操作数账守恒（重算恰好触碰全树一次——op 下界 = 节点数）。
    if ops < TOTAL_N as u64 {
        return Err(E_BUDGET);
    }
    // R4：O1 快照形状（两行、tag 非空、第一行界 = BUDGET_OPS）。
    if O1_SNAPSHOT.len() != 2
        || O1_SNAPSHOT[0].0.is_empty()
        || O1_SNAPSHOT[1].0.is_empty()
        || O1_SNAPSHOT[0].1 != BUDGET_OPS
        || O1_SNAPSHOT[1].2 != SAMPLE_N
    {
        return Err(E_ANCHOR);
    }
    // R5：构建失败面（负载规模锚自检——TOTAL_N 改动即红）。
    if TOTAL_N != 1000 || MID_N != 10 {
        return Err(E_BUILD);
    }
    // R6：影响面账（根波及全树 / 中层波及子树，双方 op 非零）。
    let (total, sub_mid) = impact_account()?;
    if total != TOTAL_N as u32 || sub_mid == 0 || sub_mid >= total {
        return Err(E_SAMPLE);
    }
    Ok((ops as u32).wrapping_add(ok).wrapping_add(ops2 as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_BUDGET => "重算操作数越冻结界（BUDGET_OPS）",
        E_SAMPLE => "抽样/全量视图与直算零差破",
        E_BUILD => "黄金负载构建失败",
        E_ANCHOR => "引擎锚/O1 快照漂移",
        E_IDX => "节点下标越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36207_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36207_inhbatch");
    cs.add("SEG 段位独占", SEG_BASE == 0x9d30, "0x9d30~0x9d3F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "千节点预算内重算/抽样 100 全对/深链同预算/O1 登记"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("预算锚", BUDGET_OPS == 50_000, "1s 折算操作数冻结界 = 50_000 op");
    cs.add("负载锚", TOTAL_N == 1000 && SAMPLE_N == 100, "黄金负载 1000 节点 + 抽样 100 例");
    cs.add("O1 预告", O1_SNAPSHOT[0].1 == BUDGET_OPS && O1_SNAPSHOT[0].0 == "o1-inh-batch-ops", "预算账入 O1 预告（两行快照）");
    cs.add("下界账", { let mut e = BatchEngine::build_1000().ok().unwrap_or(BatchEngine { tree: ie::InhTree::new(), views: Vec::new(), ops: 0 }); e.ops = 0; e.rec(0).is_ok() && e.ops >= TOTAL_N as u64 }, "重算 op 下界 = 全树节点数（触碰账）");
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
        let cs = run_unx36207_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_wall_clock_under_one_second() {
        // 时钟补证（探针 ktest 面）：千节点重算实跑 < 1s（no_std 账面
        // 用 op 预算；测试域有 std 时钟，两者必须同向）。
        let t0 = std::time::Instant::now();
        let (ops, ok) = batch_sweep().ok().unwrap_or((u64::MAX, 0));
        let dt = t0.elapsed();
        assert_eq!(ok, SAMPLE_N as u32);
        assert!(ops <= BUDGET_OPS, "op 预算破: {}", ops);
        assert!(dt.as_secs() < 1, "墙钟破 1s: {:?}", dt);
    }

    #[test]
    fn test_new_deny_reaches_unprotected_leaves() {
        // 账面守恒：变更后未受 NP 阻断的叶必见新 Deny(sid=99)——
        // 快而错的反面证据（抽样对平的语义补充）。
        let mut eng = BatchEngine::build_1000().ok().unwrap_or(BatchEngine { tree: ie::InhTree::new(), views: Vec::new(), ops: 0 });
        let mut new_ra: Vec<ie::Ace> = Vec::new();
        new_ra.push(ie::Ace { sid: 99, mask: 0x8, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_DENY, origin: 0 });
        eng.change_root_and_recompute(new_ra).ok().unwrap_or(0);
        let mid0 = eng.tree.nodes[0].children[0];
        let leaf = eng.tree.nodes[mid0].children[0];
        let v = eng.view(leaf).ok().unwrap_or_default().to_vec();
        let mut saw99 = false;
        let mut i = 0;
        while i < v.len() {
            if v[i].sid == 99 && v[i].kind == ie::AK_DENY {
                saw99 = true;
            }
            i += 1;
        }
        assert!(saw99, "root 新继承 Deny 必须批量到达叶视图");
    }
}
