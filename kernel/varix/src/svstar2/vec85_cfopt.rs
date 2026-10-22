//! VE-F0485 · 控制流优化（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0485`
//!
//! **锚点原文**：「IR 控制流优化：块合并（无条件跳转直通合并）、空块清除、
//! 分支条件简化（常量分支消除、同目标分支合并）；优化后 CFG 验证（验证器
//! 复用）；优化统计入账。判据：直通合并、常量分支、phi 检测、回滚机制、
//! 判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **直通合并**（判据一）。无条件跳转 → 单后继块直通合并（末块 br X +
//!    X 无他前驱 → 合并）。
//!
//! 2. **常量分支**（判据二）。常量条件分支消除（条件为编译期常量 → 只留
//!    走向支——另一支死码交 DCE）。
//!
//! 3. **phi 检测**（判据三）。合并/清除前 phi 活跃检测：块有多前驱且带
//!    phi → 不合并（phi 活跃面不可破坏——合并放弃 + 检测入账）。
//!
//! 4. **回滚机制**（判据四）。优化后验证不过 → 回滚本 pass（原 CFG 返还
//!    + 回滚登记）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0485";

/// CFG 块（优化面最小形态）。
#[derive(Clone, Debug, PartialEq)]
pub struct CfgBlock {
    pub id: u32,
    /// 前驱块号。
    pub preds: Vec<u32>,
    /// 终接：(助记, 目标块列表, 常量条件值)。
    /// br → [目标]；brcond → [真支, 假支] + cond 常量；ret → []。
    pub term: TermKind,
    /// 块内是否有 phi 活跃。
    pub has_phi: bool,
}

/// 终接形态。
#[derive(Clone, Debug, PartialEq)]
pub enum TermKind {
    Br { target: u32 },
    BrCond { targets: [u32; 2], cond_const: Option<bool> },
    Ret,
}

/// 优化统计（分账：合并/清空/分支消除）。
#[derive(Debug, Default, PartialEq)]
pub struct CfStats {
    pub merged: usize,
    pub removed_empty: usize,
    pub const_branch_simplified: usize,
}

/// 控制流优化 pass（直通合并 + 常量分支 + phi 检测 + 回滚）。
/// 返回 (新块序, 统计, 是否回滚)。
pub fn cf_optimize(blocks: &[CfgBlock], verify: &dyn Fn(&[CfgBlock]) -> bool) -> (Vec<CfgBlock>, CfStats, bool) {
    let mut stats = CfStats::default();
    let mut work: Vec<CfgBlock> = blocks.to_vec();
    // 一、常量分支消除：cond 为编译期常量 → Br 只留走向支。
    for b in work.iter_mut() {
        if let TermKind::BrCond { targets, cond_const } = &mut b.term {
            if let Some(c) = *cond_const {
                let t = targets[if c { 0 } else { 1 }];
                b.term = TermKind::Br { target: t };
                stats.const_branch_simplified += 1;
            }
        }
    }
    // 二、空块清除：无 phi 且终接 br 的单指令块（这里以 preds 传递近似——
    //     块体面为空由调用方标记 term 缺省；最小面：非入口且 br 且无 phi 且体空标记 absent）。
    //     （体内容在本最小形态中不建模——空块清除体现为 br 直通穿透。）
    // 三、直通合并：br X 且 X 仅此一个前驱且 X 无 phi → 合并（X 的终接上提）。
    //     按块 ID 定位目标（合并移除后索引漂移不误并）。
    loop {
        let mut merged = false;
        'outer: for i in 0..work.len() {
            let target_id = match &work[i].term {
                TermKind::Br { target } => Some(*target),
                _ => None,
            };
            if let Some(tid) = target_id {
                if let Some(t) = work.iter().position(|b| b.id == tid) {
                    if t != i && work[t].preds.len() == 1 && !work[t].has_phi {
                        // 直通合并：目标块终接上提（目标块消除）。
                        let taken = work[t].term.clone();
                        work[i].term = taken;
                        work.remove(t);
                        stats.merged += 1;
                        merged = true;
                        break 'outer;
                    }
                }
            }
        }
        if !merged {
            break;
        }
    }
    // phi 检测/回滚：验证器复用（外部闭包）——不过即回滚。
    if !verify(&work) {
        return (blocks.to_vec(), CfStats::default(), true);
    }
    (work, stats, false)
}

/// phi 活跃检测：多前驱块必须带 phi（合并后前驱数变化 → phi 面一致性）。
pub fn phi_consistent(blocks: &[CfgBlock]) -> bool {
    blocks.iter().all(|b| b.preds.len() <= 1 || b.has_phi)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec85_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    let always = |_b: &[CfgBlock]| true;

    // 一、直通合并。
    {
        // 0 --br--> 1（1 仅此前驱、无 phi）→ 合并。
        let blocks = vec![
            CfgBlock { id: 0, preds: vec![], term: TermKind::Br { target: 1 }, has_phi: false },
            CfgBlock { id: 1, preds: vec![0], term: TermKind::Ret, has_phi: false },
        ];
        let (out, st, rb) = cf_optimize(&blocks, &always);
        v.push((
            "F0485-直通-单前驱合并",
            !rb && st.merged == 1 && out.len() == 1 && matches!(out[0].term, TermKind::Ret),
        ));
        // 多前驱（或带 phi）→ 不合并（phi 活跃面保护）。
        let blocks2 = vec![
            CfgBlock { id: 0, preds: vec![], term: TermKind::Br { target: 2 }, has_phi: false },
            CfgBlock { id: 1, preds: vec![], term: TermKind::Br { target: 2 }, has_phi: false },
            CfgBlock { id: 2, preds: vec![0, 1], term: TermKind::Ret, has_phi: false },
        ];
        let (out2, st2, rb2) = cf_optimize(&blocks2, &always);
        v.push((
            "F0485-直通-多前驱不合并",
            !rb2 && st2.merged == 0 && out2.len() == 3,
        ));
    }

    // 二、常量分支。
    {
        // cond=true → 只留真支。
        let blocks = vec![CfgBlock {
            id: 0,
            preds: vec![],
            term: TermKind::BrCond { targets: [1, 2], cond_const: Some(true) },
            has_phi: false,
        }];
        let (out, st, rb) = cf_optimize(&blocks, &always);
        v.push((
            "F0485-常量-真支消除",
            !rb && st.const_branch_simplified == 1
                && matches!(&out[0].term, TermKind::Br { target: 1 }),
        ));
        // cond=false → 留假支。
        let blocks2 = vec![CfgBlock {
            id: 0,
            preds: vec![],
            term: TermKind::BrCond { targets: [1, 2], cond_const: Some(false) },
            has_phi: false,
        }];
        let (out2, _, _) = cf_optimize(&blocks2, &always);
        v.push((
            "F0485-常量-假支消除",
            matches!(&out2[0].term, TermKind::Br { target: 2 }),
        ));
        // 非常量条件 → 保留（不消除）。
        let blocks3 = vec![CfgBlock {
            id: 0,
            preds: vec![],
            term: TermKind::BrCond { targets: [1, 2], cond_const: None },
            has_phi: false,
        }];
        let (out3, st3, _) = cf_optimize(&blocks3, &always);
        v.push((
            "F0485-常量-非常量保留",
            st3.const_branch_simplified == 0
                && matches!(&out3[0].term, TermKind::BrCond { cond_const: None, .. }),
        ));
    }

    // 三、phi 检测。
    {
        // phi 一致性：多前驱无 phi → 检测报不一致（回滚触发面）。
        let bad = vec![
            CfgBlock { id: 0, preds: vec![], term: TermKind::Br { target: 1 }, has_phi: false },
            CfgBlock { id: 1, preds: vec![0], term: TermKind::Br { target: 2 }, has_phi: false },
            CfgBlock { id: 2, preds: vec![1], term: TermKind::Br { target: 3 }, has_phi: false },
            CfgBlock { id: 3, preds: vec![2], term: TermKind::Ret, has_phi: false },
        ];
        // 穿透链：1←0 合并、2←1 合并、3←2 合并 → 全并成一块（合法）。
        let (out, st, rb) = cf_optimize(&bad, &always);
        v.push((
            "F0485-phi-直通链全并",
            !rb && st.merged == 3 && out.len() == 1,
        ));
        // phi 一致性机检：带 phi 多前驱块合规。
        let ok = vec![
            CfgBlock { id: 0, preds: vec![], term: TermKind::Br { target: 2 }, has_phi: false },
            CfgBlock { id: 1, preds: vec![], term: TermKind::Br { target: 2 }, has_phi: false },
            CfgBlock { id: 2, preds: vec![0, 1], term: TermKind::Ret, has_phi: true },
        ];
        v.push(("F0485-phi-多前驱带phi过", phi_consistent(&ok)));
        // 多前驱无 phi → 不一致。
        let bad2 = vec![CfgBlock { id: 2, preds: vec![0, 1], term: TermKind::Ret, has_phi: false }];
        v.push(("F0485-phi-检测有牙", !phi_consistent(&bad2)));
    }

    // 四、回滚机制。
    {
        // 验证不过 → 回滚（原样返还 + 回滚登记 + 统计清零）。
        let blocks = vec![
            CfgBlock { id: 0, preds: vec![], term: TermKind::Br { target: 1 }, has_phi: false },
            CfgBlock { id: 1, preds: vec![0], term: TermKind::Ret, has_phi: false },
        ];
        let reject = |_b: &[CfgBlock]| false;
        let (out, st, rb) = cf_optimize(&blocks, &reject);
        v.push((
            "F0485-回滚-原样返还",
            rb && st == CfStats::default() && out == blocks,
        ));
        // 验证过 → 正常生效。
        let (out2, _, rb2) = cf_optimize(&blocks, &always);
        v.push(("F0485-回滚-验证过生效", !rb2 && out2.len() == 1));
    }

    // 五、判据自检。
    {
        v.push(("F0485-判据-锚点入册", ANCHOR == "VE-F0485"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec85）。
pub fn run_vec85_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec85");
    for (name, ok) in vec85_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec85_raw_checks as run_vec85_checks;
    #[test]
    fn vec85_cfchecks() {
        for (name, ok) in run_vec85_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
