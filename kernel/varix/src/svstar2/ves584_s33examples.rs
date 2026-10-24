//! VE-F4350 · 一致性示例库汇总（U08 测试与资产组）
//! 职责：一致性示例库总汇总——示例六源归集（U02 引擎 / U03 验证 / U04
//! 治理 / U05 集成 / U06 场景 / U07 性能）+ 汇总纪律（单源维护双链引用
//! ——不允许复制粘贴分叉：哈希比对告警）+ 示例健康度（日验证 + 版本戳
//! ——断流阻断发布）+ 失效下架 + 源修复。
//! 段位：0x4260（续表 idx584）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4260;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExCode {
    /// 分叉检出（哈希比对告警）。
    Forked = SEG_BASE,
    /// 示例失效（下架 + 源修复）。
    Stale = SEG_BASE + 0x01,
    /// 健康度断流（阻断发布）。
    HealthGap = SEG_BASE + 0x02,
    /// 源号越界。
    SrcRange = SEG_BASE + 0x03,
}

/// 六源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Src {
    Engine,
    Verify,
    Gov,
    Integ,
    Scene,
    Perf,
}

/// 示例条目（单源双链引用）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Example {
    pub src: Src,
    pub id: u32,
    /// 内容哈希（单源正本——分叉检测键）。
    pub hash: u32,
    /// 版本戳。
    pub ver: u32,
    /// 健康度：最近日验证日序。
    pub last_check: u32,
    pub retired: bool,
}

/// 汇总台。
#[derive(Default)]
pub struct ExampleHub {
    pub items: Vec<Example>,
    /// 日验证今日序。
    pub today: u32,
    pub forks: u32,
    pub health_blocks: u32,
}

/// 日验证间隔（天）。
pub const HEALTH_SLACK: u32 = 2;

fn fnv(x: u32) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let mut v = x;
    for _ in 0..4 {
        h ^= v & 0xFF;
        h = h.wrapping_mul(0x01000193);
        v >>= 8;
    }
    h
}

impl ExampleHub {
    pub fn new(today: u32) -> Self {
        ExampleHub { today, ..Default::default() }
    }

    /// 示例归集（源校验在类型层；同 id 覆盖）。
    pub fn ingest(&mut self, src: Src, id: u32, content: u32, ver: u32) {
        let hash = fnv(content);
        if let Some(x) = self.items.iter_mut().find(|x| x.id == id) {
            x.src = src;
            x.hash = hash;
            x.ver = ver;
            x.last_check = self.today;
        } else {
            self.items.push(Example { src, id, hash, ver, last_check: self.today, retired: false });
        }
    }

    /// 分叉检测：同 id 双登记内容哈希不一致 = 复制粘贴分叉 → 告警（以先登
    /// 记的单源正本为准，后者拒绝）。
    pub fn check_fork(&mut self, id: u32, content: u32) -> Result<(), ExCode> {
        let x = self.items.iter().find(|x| x.id == id).ok_or(ExCode::Stale)?;
        if x.hash != fnv(content) {
            self.forks += 1;
            return Err(ExCode::Forked);
        }
        Ok(())
    }

    /// 健康度扫描：超过日验证间隔的示例清单（阻断发布对象）。
    pub fn health_scan(&mut self) -> Vec<u32> {
        let today = self.today;
        let stale: Vec<u32> = self
            .items
            .iter()
            .filter(|x| !x.retired && today > x.last_check + HEALTH_SLACK)
            .map(|x| x.id)
            .collect();
        if !stale.is_empty() {
            self.health_blocks += 1;
        }
        stale
    }

    /// 日验证执行：刷新健康度。
    pub fn daily_check(&mut self, id: u32) -> Result<(), ExCode> {
        let x = self.items.iter_mut().find(|x| x.id == id).ok_or(ExCode::Stale)?;
        x.last_check = self.today;
        Ok(())
    }

    /// 失效下架（源修复前不复活）。
    pub fn retire(&mut self, id: u32) -> Result<(), ExCode> {
        let x = self.items.iter_mut().find(|x| x.id == id).ok_or(ExCode::Stale)?;
        x.retired = true;
        Ok(())
    }

    /// 源修复后重新归集（解除下架 + 版本戳刷新）。
    pub fn revive_from_src(&mut self, id: u32, content: u32, ver: u32) -> Result<(), ExCode> {
        let today = self.today;
        let hash = fnv(content);
        let x = self.items.iter_mut().find(|x| x.id == id).ok_or(ExCode::Stale)?;
        x.hash = hash;
        x.ver = ver;
        x.retired = false;
        x.last_check = today;
        Ok(())
    }

    /// 六源齐备断言（去下架口径）。
    pub fn all_srcs(&self) -> bool {
        (0..6).all(|i| self.items.iter().any(|x| !x.retired && x.src as usize == i))
    }

    /// 发布闸：六源齐 + 无失效健康度。
    pub fn publish_gate(&mut self) -> bool {
        self.all_srcs() && self.health_scan().is_empty()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves584_checks() -> CheckSet {
    let mut set = CheckSet::new("ves584");
    let mut eh = ExampleHub::new(100);

    // C584-01 六源示例归集。
    let srcs = [Src::Engine, Src::Verify, Src::Gov, Src::Integ, Src::Scene, Src::Perf];
    for (i, s) in srcs.iter().enumerate() {
        eh.ingest(*s, i as u32 + 1, 1000 + i as u32, 1);
    }
    chk(&mut set, "C584-01", eh.all_srcs() && eh.items.len() == 6, "六源归集齐备");

    // C584-02 单源双链：同 id 同内容通过（引用合法）。
    chk(&mut set, "C584-02", eh.check_fork(1, 1000).is_ok(), "双链引用合法");

    // C584-03 分叉检出（哈希不一致告警）。
    chk(&mut set, "C584-03", eh.check_fork(1, 9999) == Err(ExCode::Forked) && eh.forks == 1, "复制分叉告警");

    // C584-04 健康度断流检出（隔 3 天未验）。
    eh.today = 104;
    let stale = eh.health_scan();
    chk(&mut set, "C584-04", stale.len() == 6 && eh.health_blocks == 1, "断流全量检出");

    // C584-05 日验证刷新后健康。
    for id in 1..=6u32 {
        let _ = eh.daily_check(id);
    }
    chk(&mut set, "C584-05", eh.health_scan().is_empty(), "日验后健康");

    // C584-06 边界恰隔 2 天不误报。
    eh.today = 106;
    chk(&mut set, "C584-06", eh.health_scan().is_empty(), "间隔边界不误报");

    // C584-07 失效下架。
    eh.today = 200;
    let _ = eh.retire(3);
    chk(&mut set, "C584-07", !eh.all_srcs(), "下架后六源缺");

    // C584-08 源修复重新归集（解除下架 + 版本刷新）。
    let _ = eh.revive_from_src(3, 1002, 2);
    chk(&mut set, "C584-08", eh.all_srcs() && eh.items[2].ver == 2, "源修复复活");

    // C584-09 同 id 覆盖归集（版本戳刷新）。
    eh.today = 201;
    eh.ingest(Src::Perf, 6, 1005, 3);
    chk(&mut set, "C584-09", eh.items[5].ver == 3 && eh.items[5].last_check == 201, "覆盖归集刷新");

    // C584-10 发布闸全过。
    chk(&mut set, "C584-10", eh.publish_gate(), "发布闸放行");

    // C584-11 未知示例验证/下架拒。
    chk(&mut set, "C584-11", eh.daily_check(99).is_err() && eh.retire(99).is_err(), "未知示例拒");

    // C584-12 空库发布闸恒假（六源缺）。
    let mut eh2 = ExampleHub::new(0);
    chk(&mut set, "C584-12", !eh2.publish_gate(), "空库不放行");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_detection() {
        let mut eh = ExampleHub::new(0);
        eh.ingest(Src::Engine, 1, 42, 1);
        assert!(eh.check_fork(1, 42).is_ok());
        assert_eq!(eh.check_fork(1, 43), Err(ExCode::Forked));
        assert_eq!(eh.forks, 1);
    }

    #[test]
    fn health_slack_boundary() {
        let mut eh = ExampleHub::new(10);
        eh.ingest(Src::Gov, 1, 1, 1);
        eh.today = 12; // 恰 = last_check + 2
        assert!(eh.health_scan().is_empty());
        eh.today = 13;
        assert_eq!(eh.health_scan(), [1]);
    }
}
