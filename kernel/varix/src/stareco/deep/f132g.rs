//! 深化层四 · F132 差异表公开（2026-09-27 深化批次四 · g 层）。
//!
//! API 迁移映射表（旧→新调用对）、迁移计划器（按调用量排序分批）、
//! 破坏面热力矩阵（族×影响计数）、季度积压曲线、签名快照对比。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 迁移映射表：old → new 调用对；被移除 API 必须有映射（零静默移除）
// ---------------------------------------------------------------------------

pub struct MigrationMap {
    pairs: alloc::vec::Vec<(&'static str, &'static str)>,
}

impl MigrationMap {
    pub fn new() -> MigrationMap {
        MigrationMap { pairs: alloc::vec::Vec::new() }
    }

    pub fn map(&mut self, old: &'static str, new: &'static str) -> Result<(), &'static str> {
        if old.is_empty() || new.is_empty() {
            return Err("映射两端必填");
        }
        if old == new {
            return Err("自映射无意义");
        }
        if self.pairs.iter().any(|(o, _)| *o == old) {
            return Err("旧 API 重复映射：迁移路径必须唯一");
        }
        self.pairs.push((old, new));
        Ok(())
    }

    pub fn target(&self, old: &str) -> Option<&'static str> {
        self.pairs.iter().find(|(o, _)| *o == old).map(|(_, n)| *n)
    }

    /// 零静默移除审计：removed 名单里每个 API 都必须有映射。
    pub fn removals_covered(&self, removed: &[&'static str]) -> alloc::vec::Vec<&'static str> {
        removed.iter().filter(|r| self.target(r).is_none()).copied().collect()
    }
}

// ---------------------------------------------------------------------------
// 迁移计划器：消费方按调用量降序分批（每批 ≤N 家）
// ---------------------------------------------------------------------------

pub struct Migratee {
    pub name: &'static str,
    pub call_sites: u32,
}

/// 分批：call_sites 降序，切批每批 cap 家。返回批号→名单。
pub fn plan_batches(migratees: &[Migratee], cap: usize) -> alloc::vec::Vec<alloc::vec::Vec<&'static str>> {
    let mut sorted: alloc::vec::Vec<&Migratee> = migratees.iter().collect();
    // 插入序按调用量降序，平局按名字节序（确定性）。
    for i in 1..sorted.len() {
        let k = sorted[i];
        let mut j = i;
        while j > 0
            && (sorted[j - 1].call_sites < k.call_sites
                || (sorted[j - 1].call_sites == k.call_sites && sorted[j - 1].name > k.name))
        {
            sorted[j] = sorted[j - 1];
            j -= 1;
        }
        sorted[j] = k;
    }
    let mut out: alloc::vec::Vec<alloc::vec::Vec<&'static str>> = alloc::vec::Vec::new();
    for m in sorted {
        match out.last_mut() {
            Some(b) if b.len() < cap => b.push(m.name),
            _ => out.push(alloc::vec![m.name]),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 破坏面热力矩阵：族 × 影响计数 → 族风险分（致命×3 + 降级×1）
// ---------------------------------------------------------------------------

pub struct HeatCell {
    pub family: u8,
    pub lethal: u32,
    pub degraded: u32,
}

pub fn family_risk(cells: &[HeatCell]) -> alloc::vec::Vec<(u8, u32)> {
    cells
        .iter()
        .map(|c| (c.family, c.lethal * 3 + c.degraded))
        .collect()
}

/// 风险最高族（平局取族号小者）。
pub fn hottest_family(cells: &[HeatCell]) -> Option<u8> {
    let scored = family_risk(cells);
    let mut best: Option<(u8, u32)> = None;
    for (f, r) in scored {
        match best {
            Some((bf, br)) if !(r > br || (r == br && f < bf)) => {}
            _ => best = Some((f, r)),
        }
    }
    best.map(|(f, _)| f)
}

// ---------------------------------------------------------------------------
// 季度积压曲线：open/resolved 季度序列 → 净积压差分
// ---------------------------------------------------------------------------

pub fn backlog_curve(opened: &[u32], resolved: &[u32]) -> Result<alloc::vec::Vec<i64>, &'static str> {
    if opened.len() != resolved.len() {
        return Err("开/解序列长度不一致：账目矛盾");
    }
    Ok(opened
        .iter()
        .zip(resolved.iter())
        .map(|(o, r)| *o as i64 - *r as i64)
        .collect())
}

/// 净积压连续为负（ resolving 快于新增）→ 达标。
pub fn shrinking(curve: &[i64]) -> bool {
    curve.iter().all(|d| *d < 0)
}

// ---------------------------------------------------------------------------
// 签名快照对比：两代公共 API 指纹集 → 增/删
// ---------------------------------------------------------------------------

pub fn snapshot_compare(old: &[u64], new: &[u64]) -> (alloc::vec::Vec<u64>, alloc::vec::Vec<u64>) {
    let added: alloc::vec::Vec<u64> =
        new.iter().filter(|n| !old.contains(n)).copied().collect();
    let removed: alloc::vec::Vec<u64> =
        old.iter().filter(|o| !new.contains(o)).copied().collect();
    (added, removed)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F132G_TAG: &str = "stareco-F132-deep4";

pub fn run_f132_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F132G_TAG);

    // 映射表
    let mut mm = MigrationMap::new();
    set.add("f132g map self", mm.map("a", "a").is_err(), "自映射拒绝");
    let _ = mm.map("old_open", "open2");
    set.add("f132g map dup", mm.map("old_open", "open3").is_err(), "重复映射拒绝");
    set.add("f132g map lookup", mm.target("old_open") == Some("open2"), "查新路径");
    set.add(
        "f132g removals covered",
        mm.removals_covered(&["old_open", "old_zap"]) == alloc::vec!["old_zap"],
        "未映射移除点名",
    );

    // 分批
    let ms = [
        Migratee { name: "big", call_sites: 40 },
        Migratee { name: "tiny", call_sites: 1 },
        Migratee { name: "mid", call_sites: 10 },
        Migratee { name: "mid2", call_sites: 10 },
    ];
    let batches = plan_batches(&ms, 2);
    set.add(
        "f132g batch order",
        batches[0] == alloc::vec!["big", "mid"] && batches[1] == alloc::vec!["mid2", "tiny"],
        "调用量降序+平局名字序",
    );
    set.add("f132g batch cap", batches.len() == 2 && batches.iter().all(|b| b.len() <= 2), "批容量守恒");

    // 热力矩阵
    let cells = [
        HeatCell { family: 1, lethal: 1, degraded: 2 },
        HeatCell { family: 2, lethal: 0, degraded: 9 },
    ];
    set.add("f132g risk", family_risk(&cells) == alloc::vec![(1, 5), (2, 9)], "族风险分");
    set.add("f132g hottest", hottest_family(&cells) == Some(2), "最热族");
    set.add("f132g tie", hottest_family(&[
        HeatCell { family: 3, lethal: 1, degraded: 0 },
        HeatCell { family: 1, lethal: 1, degraded: 0 },
    ]) == Some(1), "平局取族号小");

    // 积压曲线
    set.add(
        "f132g curve mismatch",
        backlog_curve(&[1, 2], &[1]).is_err(),
        "长度矛盾拒绝",
    );
    let curve = backlog_curve(&[5, 3, 2], &[6, 4, 3]).expect("ok");
    set.add(
        "f132g curve shrinking",
        curve == alloc::vec![-1, -1, -1] && shrinking(&curve),
        "净积压全负=收敛",
    );
    set.add("f132g curve grow", !shrinking(&[1, -1]), "有正差分即未收敛");

    // 快照对比
    let (added, removed) = snapshot_compare(&[1, 2, 3], &[2, 3, 9]);
    set.add(
        "f132g snapshot",
        added == alloc::vec![9] && removed == alloc::vec![1],
        "增删对拍",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn batch_empty_and_cap_one() {
        assert!(plan_batches(&[], 3).is_empty());
        let one = [Migratee { name: "a", call_sites: 5 }];
        let b = plan_batches(&one, 1);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0], alloc::vec!["a"]);
    }

    #[test]
    fn map_empty_removals() {
        let mm = MigrationMap::new();
        assert!(mm.removals_covered(&[]).is_empty());
        assert_eq!(mm.removals_covered(&["x"]), alloc::vec!["x"]);
    }
}
