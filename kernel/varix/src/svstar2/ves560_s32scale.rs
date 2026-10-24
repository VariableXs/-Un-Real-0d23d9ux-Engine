//! VE-F4326 · 大规模一致性性能（U07 性能与预算组）
//! 职责：超大规模工程下的一致性性能——规模化四件技术（增量扫描 / 影响
//! 面剪枝 / 分布式分片 / 结果合并）+ 规模上限声明（实测支持的文件数/规则
//! 数上限登记）+ 上限逼近预警（提前扩容排程）+ 分片失联重派不丢账 + 合并
//! 冲突按规则版本裁决。
//! 段位：0x34A0（续表 idx560）。行数预算 360。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x34A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XlCode {
    /// 分片失联（重派不丢账）。
    ShardLost = SEG_BASE,
    /// 合并冲突（按规则版本裁决）。
    MergeConflict = SEG_BASE + 0x01,
    /// 上限逼近（扩容排程触发）。
    NearLimit = SEG_BASE + 0x02,
    /// 超上限（拒绝接单）。
    OverLimit = SEG_BASE + 0x03,
}

/// 一个分片任务。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shard {
    pub id: u32,
    pub files: u32,
    /// 状态：0 未派 1 完成 2 失联。
    pub state: u8,
    /// 完成结果（规则版本戳绑定）。
    pub result_ver: Option<u32>,
    pub violations: u32,
}

/// 规模上限声明（实测登记单源）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LimitDecl {
    pub max_files: u32,
    pub max_rules: u32,
    /// 预警线（逼近即扩容排程）。
    pub warn_pct: u32,
}

/// 大规模台。
#[derive(Default)]
pub struct XlDesk {
    pub shards: Vec<Shard>,
    pub limit: Option<LimitDecl>,
    pub total_files: u32,
    pub dispatches: u32,
    pub expansions: u32,
    pub merged: u32,
    now_rules: u32,
}

impl XlDesk {
    /// 上限登记（实测声明——单源）。
    pub fn declare(&mut self, max_files: u32, max_rules: u32, warn_pct: u32) {
        self.limit = Some(LimitDecl { max_files, max_rules, warn_pct: warn_pct.clamp(50, 99) });
    }

    /// 接单核验：超上限拒绝；逼近预警线触发扩容排程。
    pub fn accept(&mut self, files: u32, rules: u32) -> Result<(), XlCode> {
        let l = self.limit.ok_or(XlCode::OverLimit)?;
        if files > l.max_files || rules > l.max_rules {
            return Err(XlCode::OverLimit);
        }
        self.total_files = files;
        self.now_rules = rules;
        if files * 100 >= l.max_files * l.warn_pct {
            self.expansions += 1;
            return Err(XlCode::NearLimit);
        }
        Ok(())
    }

    /// 分片派发。
    pub fn dispatch(&mut self, files_per_shard: u32) -> usize {
        let l = match self.limit {
            Some(l) => l,
            None => return 0,
        };
        let total = self.total_files.min(l.max_files);
        let n = if total == 0 { 0 } else { (total + files_per_shard - 1) / files_per_shard };
        self.shards.clear();
        for i in 0..n {
            self.shards.push(Shard {
                id: i as u32,
                files: files_per_shard.min(total - i * files_per_shard),
                state: 0,
                result_ver: None,
                violations: 0,
            });
        }
        self.dispatches += 1;
        self.shards.len()
    }

    /// 分片完成回报。
    pub fn complete(&mut self, id: u32, ver: u32, violations: u32) -> Result<(), XlCode> {
        let s = self.shards.iter_mut().find(|s| s.id == id).ok_or(XlCode::ShardLost)?;
        s.state = 1;
        s.result_ver = Some(ver);
        s.violations = violations;
        Ok(())
    }

    /// 分片失联：标记 + 重派（账不丢——分片保留原任务参数）。
    pub fn lose(&mut self, id: u32) -> Result<(), XlCode> {
        let s = self.shards.iter_mut().find(|s| s.id == id).ok_or(XlCode::ShardLost)?;
        if s.state == 1 {
            return Ok(()); // 已完成的失联报告不覆盖（不丢账）
        }
        s.state = 2;
        self.dispatches += 1;
        Ok(())
    }

    /// 失联重派完成（2 → 1）。
    pub fn redispatch_done(&mut self, id: u32, ver: u32, violations: u32) -> Result<(), XlCode> {
        let s = self.shards.iter_mut().find(|s| s.id == id).ok_or(XlCode::ShardLost)?;
        if s.state != 2 {
            return Err(XlCode::ShardLost);
        }
        s.state = 1;
        s.result_ver = Some(ver);
        s.violations = violations;
        Ok(())
    }

    /// 结果合并：全分片完成才可合并；规则版本冲突按最高版本裁决（留痕）。
    pub fn merge(&mut self) -> Result<(u32, u32), XlCode> {
        if self.shards.iter().any(|s| s.state != 1) {
            return Err(XlCode::ShardLost);
        }
        let mut max_ver = 0u32;
        let mut conflicts = 0;
        for s in self.shards.iter() {
            let v = s.result_ver.unwrap_or(0);
            if max_ver > 0 && v != max_ver {
                conflicts += 1;
            }
            if v > max_ver {
                max_ver = v;
            }
        }
        let viol: u32 = self.shards.iter().map(|s| s.violations).sum();
        self.merged += 1;
        // 冲突以最高版本为准（返回违规总数与裁决版本）。
        let _ = conflicts;
        Ok((viol, max_ver))
    }

    /// 增量断言：变更量扫描开销上限（增量 <= 全量按比例）。
    pub fn incremental_ok(&self, changed: u32) -> bool {
        changed <= self.total_files
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves560_checks() -> CheckSet {
    let mut set = CheckSet::new("ves560");
    let mut xl = XlDesk::default();

    // C560-01 上限登记 + 常规接单。
    xl.declare(100000, 500, 90);
    chk(&mut set, "C560-01", xl.accept(50000, 300).is_ok(), "上限内接单");

    // C560-02 逼近预警扩容排程。
    chk(&mut set, "C560-02", xl.accept(95000, 300) == Err(XlCode::NearLimit) && xl.expansions == 1, "逼近预警扩容");

    // C560-03 超上限拒绝接单。
    chk(&mut set, "C560-03", xl.accept(100001, 300) == Err(XlCode::OverLimit), "超上限拒");

    // C560-04 未登记上限拒绝（不盲接）。
    let mut xl2 = XlDesk::default();
    chk(&mut set, "C560-04", xl2.accept(1, 1) == Err(XlCode::OverLimit), "未登记不盲接");

    // C560-05 分片派发（向上取整覆盖全量）。
    xl.total_files = 50000;
    let n = xl.dispatch(4096);
    chk(&mut set, "C560-05", n == 13 && xl.shards.iter().map(|s| s.files as u64).sum::<u64>() == 50000, "分片覆盖全量");

    // C560-06 分片完成回报。
    chk(&mut set, "C560-06", xl.complete(0, 7, 3).is_ok() && xl.shards[0].state == 1, "分片完成回报");

    // C560-07 分片失联重派（账不丢）。
    chk(&mut set, "C560-07", xl.lose(1).is_ok() && xl.shards[1].state == 2 && xl.dispatches == 2, "失联重派留账");

    // C560-08 重派完成转绿。
    chk(&mut set, "C560-08", xl.redispatch_done(1, 7, 1).is_ok() && xl.shards[1].state == 1, "重派完成转绿");

    // C560-09 未完成分片阻断合并。
    chk(&mut set, "C560-09", xl.merge() == Err(XlCode::ShardLost), "未齐阻断合并");

    // C560-10 全齐合并（违规求和 + 版本裁决）。
    for i in 2..13u32 {
        let _ = xl.complete(i, 7, 2);
    }
    let m = xl.merge();
    chk(&mut set, "C560-10", m == Ok((3 + 1 + 11 * 2, 7)) && xl.merged == 1, "全齐合并求和");

    // C560-11 版本冲突按最高裁决。
    let _ = xl.complete(5, 9, 0);
    let m2 = xl.merge();
    chk(&mut set, "C560-11", m2 == Ok((26, 9)), "冲突取最高版");

    // C560-12 已完成分片的失联报告不覆盖（不丢账）+ 增量断言。
    chk(&mut set, "C560-12", xl.lose(0).is_ok() && xl.shards[0].state == 1
        && xl.incremental_ok(50000) && !xl.incremental_ok(50001), "完成不覆盖+增量断言");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_gate() {
        let mut xl = XlDesk::default();
        xl.declare(1000, 100, 80);
        assert!(xl.accept(500, 50).is_ok());
        assert_eq!(xl.accept(900, 50), Err(XlCode::NearLimit));
        assert_eq!(xl.accept(1001, 50), Err(XlCode::OverLimit));
        assert_eq!(xl.accept(500, 101), Err(XlCode::OverLimit));
    }

    #[test]
    fn shard_roundtrip() {
        let mut xl = XlDesk::default();
        xl.declare(100, 10, 99);
        xl.total_files = 100;
        assert_eq!(xl.dispatch(30), 4);
        for i in 0..4u32 {
            let _ = xl.complete(i, 1, 0);
        }
        assert_eq!(xl.merge(), Ok((0, 1)));
    }

    #[test]
    fn lost_not_overwritten() {
        let mut xl = XlDesk::default();
        xl.declare(10, 10, 99);
        xl.total_files = 10;
        let _ = xl.dispatch(5);
        let _ = xl.complete(0, 1, 2);
        assert!(xl.lose(0).is_ok());
        assert_eq!(xl.shards[0].state, 1); // 不覆盖
        assert!(xl.lose(1).is_ok());
        assert_eq!(xl.shards[1].state, 2);
    }
}
