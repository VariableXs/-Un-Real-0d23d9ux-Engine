//! F031 深化批次二 · 卸载进度与残留差集面（compatstar2/deep · G-A-31）。
//!
//! 批次一深化覆盖 ARP 登记/静默开关/体积统计；本批补齐：卸载进度模型
//! （已清/总量——大沙盒不卡 UI 的进度数据源）、回收站还原路径账（还原 =
//! 原路径写回——「还原成功」判据的定位面）、蜂巢快照差集（卸载前后键集
//! 对比——「残留扫描」的判定依据）、强制卸载双确认（清单全空的最后手段，
//! 双确认 + 审计必记）。
//!
//! 零堆纪律：定长表，无 alloc。

use crate::checks::CheckSet;

/// 蜂巢快照容量（键指纹数）。
pub const HIVE_SNAPSHOT_SLOTS: usize = 16;
/// 强制卸载所需确认次数。
pub const FORCED_CONFIRMATIONS: u8 = 2;

/// 卸载进度（已清/总量 permille——进度条的数据源）。
pub struct UninstallProgress {
    pub total_items: usize,
    pub cleared_items: usize,
}

impl UninstallProgress {
    pub fn permille(&self) -> u32 {
        if self.total_items == 0 {
            return 0;
        }
        (self.cleared_items * 1000 / self.total_items) as u32
    }
    pub fn complete(&self) -> bool {
        self.cleared_items >= self.total_items && self.total_items > 0
    }
}

/// 回收站还原路径账：还原 = 原路径写回（「撤销还原成功」的定位面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecycleEntry {
    /// 原路径指纹（域内 8 字节口径；真实实现存完整路径）。
    pub orig_path_hash: [u8; 8],
    pub recycle_seq: u32,
    pub size_bytes: u64,
}

/// 还原：按序号取回原路径（路径账恒等——写回位置 = 删除位置）。
pub fn restore_target(entry: &RecycleEntry) -> [u8; 8] {
    entry.orig_path_hash
}

/// 蜂巢快照差集：卸载前后的键指纹集合对比。
/// 新增键（卸载后冒出）= 残留候选；消失键 = 已清（卸载器自己删的）。
pub struct HiveDiff {
    pub before: [[u8; 8]; HIVE_SNAPSHOT_SLOTS],
    pub before_n: usize,
    pub after: [[u8; 8]; HIVE_SNAPSHOT_SLOTS],
    pub after_n: usize,
}

impl HiveDiff {
    pub const fn new() -> Self {
        HiveDiff { before: [[0; 8]; HIVE_SNAPSHOT_SLOTS], before_n: 0, after: [[0; 8]; HIVE_SNAPSHOT_SLOTS], after_n: 0 }
    }
    pub fn snap_before(&mut self, keys: &[[u8; 8]]) {
        self.before_n = keys.len().min(HIVE_SNAPSHOT_SLOTS);
        self.before[..self.before_n].copy_from_slice(&keys[..self.before_n]);
    }
    pub fn snap_after(&mut self, keys: &[[u8; 8]]) {
        self.after_n = keys.len().min(HIVE_SNAPSHOT_SLOTS);
        self.after[..self.after_n].copy_from_slice(&keys[..self.after_n]);
    }
    fn contains(set: &[[u8; 8]], n: usize, key: &[u8; 8]) -> bool {
        set[..n].iter().any(|k| k == key)
    }
    /// 残留候选：卸载后仍在的键数（语义 = 清单外冒出的键）。
    pub fn residual_candidates(&self) -> usize {
        (0..self.after_n).filter(|&i| !Self::contains(&self.before, self.before_n, &self.after[i])).count()
    }
    /// 已清键数：卸载前有、卸载后无。
    pub fn cleared_count(&self) -> usize {
        (0..self.before_n).filter(|&i| !Self::contains(&self.after, self.after_n, &self.before[i])).count()
    }
}

/// 强制卸载闸：清单全空的最后手段——两次独立确认 + 审计必记。
pub struct ForcedUninstallGate {
    pub confirmations: u8,
    pub audit_entries: u32,
}

impl ForcedUninstallGate {
    pub const fn new() -> Self {
        ForcedUninstallGate { confirmations: 0, audit_entries: 0 }
    }
    /// 确认一次；达到两次 → 放行（审计入账；每次确认都留痕）。
    pub fn confirm(&mut self) -> bool {
        self.confirmations += 1;
        self.audit_entries += 1; // 每次确认都审计（不可抵赖）
        self.confirmations >= FORCED_CONFIRMATIONS
    }
}

/// 域自检（深化批次二）。
pub fn run_f031d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F031-uninstall-d2");
    // 1) 进度：93 项清 62 → 666‰；清完 = complete。
    let p = UninstallProgress { total_items: 93, cleared_items: 62 };
    let done = UninstallProgress { total_items: 93, cleared_items: 93 };
    cs.add("progress_permille", p.permille() == 666 && !p.complete() && done.complete(), "");
    // 2) 还原路径恒等：写回位置 = 删除位置（路径账对拍）。
    let e = RecycleEntry { orig_path_hash: [0xAB; 8], recycle_seq: 7, size_bytes: 86 << 20 };
    cs.add("restore_path_identity", restore_target(&e) == [0xAB; 8] && e.recycle_seq == 7, "");
    // 3) 蜂巢差集：卸载后冒出新键 = 残留候选 1；卸载器自删 1 键 = 已清 1。
    let mut hd = HiveDiff::new();
    hd.snap_before(&[[1; 8], [2; 8], [3; 8]]);
    hd.snap_after(&[[1; 8], [3; 8], [9; 8]]);
    cs.add("hive_diff", hd.residual_candidates() == 1 && hd.cleared_count() == 1, "");
    // 4) 强制卸载：一次确认不放行、两次放行、每次确认均审计。
    let mut f = ForcedUninstallGate::new();
    let once = f.confirm();
    let twice = f.confirm();
    cs.add("forced_double_confirm", !once && twice && f.confirmations == 2 && f.audit_entries == 2, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_all_cleared_zero_residual() {
        let mut hd = HiveDiff::new();
        hd.snap_before(&[[1; 8], [2; 8]]);
        hd.snap_after(&[]);
        assert_eq!(hd.residual_candidates(), 0);
        assert_eq!(hd.cleared_count(), 2, "勾选外零残留的干净卸载");
    }

    #[test]
    fn progress_boundary() {
        let zero = UninstallProgress { total_items: 0, cleared_items: 0 };
        assert_eq!(zero.permille(), 0);
        assert!(!zero.complete(), "空清单不算完成（强制卸载路径接管）");
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f031d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
