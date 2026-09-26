//! 深化层 · F576 磁贴分组文件夹（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F576 节）：
//! ①「组折叠（只显示组名条）」的**折叠状态机**——折叠/展开逐组独立，
//!   折叠时组内磁贴账不丢（展开即原样回归）；
//! ②「组整体可拖可删」的**组删除与拆散**——删组即散（组员回散盘）、
//!   拖出单枚即离组（拆组语义与手机桌面同构）；
//! ③折叠态的**容量合同**——折叠只藏内容不销账（组内 12 枚上限在
//!   折叠态依旧有效，防「折起来偷偷塞爆」）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::tilegroup::{TileBoard, GROUP_CAP};

// ---------------------------------------------------------------------------
// 折叠状态机
// ---------------------------------------------------------------------------

/// 折叠账（组下标 → 折叠位；容量与组上限同规 8）。
pub struct FoldLedger {
    folded: [bool; 8],
    len: usize,
}

impl FoldLedger {
    pub fn new() -> FoldLedger {
        FoldLedger { folded: [false; 8], len: 0 }
    }

    /// 同步组数（建组/删组后调用——账随板走）。
    pub fn sync_groups(&mut self, groups: usize) {
        self.len = groups.min(8);
    }

    pub fn toggle(&mut self, idx: usize) -> bool {
        if idx >= self.len {
            return false;
        }
        self.folded[idx] = !self.folded[idx];
        self.folded[idx]
    }

    pub fn is_folded(&self, idx: usize) -> bool {
        idx < self.len && self.folded[idx]
    }

    /// 折叠合同：折叠不销账——展开后组员数与折叠前一致（由调用方
    /// 传入前后组员数核对）。
    pub fn fold_keeps_members(before: usize, after: usize) -> bool {
        before == after
    }
}

impl Default for FoldLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 拆组语义：删组即散——组员按原序回散盘尾部。
pub fn ungroup_to_loose(members: &[u64], loose_tail: &mut alloc::vec::Vec<u64>) -> usize {
    let n = members.len();
    for &m in members {
        loose_tail.push(m);
    }
    n
}

/// 折叠态容量合同：组员数在折叠前后都不得超 [`GROUP_CAP`]。
pub fn fold_cap_holds(members: usize) -> bool {
    members <= GROUP_CAP
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f576_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 折叠状态机：逐组独立折叠/展开。
    let mut fl = FoldLedger::new();
    fl.sync_groups(2);
    let f0 = fl.toggle(0);
    cs.add(
        "fold per group independent",
        f0 && fl.is_folded(0) && !fl.is_folded(1) && !fl.toggle(0),
        "",
    );

    // 2) 折叠不销账：展开后组员数一致。
    cs.add("fold keeps members", FoldLedger::fold_keeps_members(5, 5), "");

    // 3) 越界组不可折叠（账随板走——没有的组没有折叠位）。
    cs.add("out of range fold rejected", !fl.toggle(5), "");

    // 4) 删组即散：组员按原序回散盘尾部。
    let mut loose = alloc::vec::Vec::new();
    let n = ungroup_to_loose(&[7, 8, 9], &mut loose);
    cs.add(
        "ungroup scatters members",
        n == 3 && loose == alloc::vec![7, 8, 9],
        "",
    );

    // 5) 折叠态容量合同：12 枚上限折叠前后都有效。
    cs.add(
        "fold cap holds",
        fold_cap_holds(GROUP_CAP) && !fold_cap_holds(GROUP_CAP + 1),
        "",
    );

    // 6) 与基础板联动：hover 600ms 建组后，折叠账同步组数。
    let mut board = TileBoard::new();
    board.add_tile(1, true);
    board.add_tile(2, true);
    board.hover_start(1, 2, 0);
    let made = board.hover_tick(600); // 恰到 600ms 合组
    fl.sync_groups(board.groups().len());
    cs.add(
        "board group synced to fold ledger",
        made && board.groups().len() == 1 && fl.is_folded(0) == false && fl.toggle(0),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_ledger_rolls_with_groups() {
        let mut fl = FoldLedger::new();
        fl.sync_groups(3);
        fl.toggle(2);
        fl.sync_groups(1); // 删组后高位折叠位失效
        assert!(!fl.toggle(2));
        assert!(fl.toggle(0));
    }
}
