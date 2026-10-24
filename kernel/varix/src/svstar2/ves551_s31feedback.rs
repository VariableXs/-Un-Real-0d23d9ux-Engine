//! VE-F4317 · 场景应用反馈闭环（U06 场景应用组）
//! 职责：场景使用反馈闭环——反馈采集（场景内一键反馈 + 体验日志挫败信号
//! 联动：rage click / dead click 自动归集）+ 反馈分级分诊（体验问题入优化
//! 池 / 判定争议入仲裁；误分诊人工复核改道）+ 闭环度量（反馈到改进周期
//! 指标；劣化预警联动）+ 恶意反馈频控 + 反馈入口键盘读屏可达（域本色）+
//! 反馈内容脱敏存储。
//! 段位：0x3260（续表 idx551）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3260;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FbCode {
    /// 恶意反馈（频控拦截）。
    FloodBlocked = SEG_BASE,
    /// 反馈入口不可达（键盘/读屏缺失——域色硬线）。
    EntryUnreachable = SEG_BASE + 0x01,
    /// 误分诊（人工复核改道）。
    MisRouted = SEG_BASE + 0x02,
    /// 内容未脱敏（拒收）。
    Unsanitized = SEG_BASE + 0x03,
}

/// 反馈类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FbKind {
    /// 体验问题（入优化池）。
    Experience,
    /// 判定争议（入仲裁）。
    VerdictDispute,
    /// 挫败信号自动归集。
    RageSignal,
}

/// 一条反馈。
#[derive(Clone, Debug, PartialEq)]
pub struct Feedback {
    pub id: u32,
    pub scene: u8,
    pub kind: FbKind,
    /// 脱敏标记（强制）。
    pub sanitized: bool,
    /// 键盘/读屏可达入口产生（域色）。
    pub reachable: bool,
    /// 分诊去向：true=优化池 false=仲裁。
    pub routed_pool: Option<bool>,
    /// 改进闭环时间戳（0 = 未闭环）。
    pub closed_at: u64,
}

/// 反馈闭环台。
#[derive(Default)]
pub struct FeedbackDesk {
    pub items: Vec<Feedback>,
    /// 频控：同来源窗口内计数（此处以场景号窗口简化）。
    pub flood: Vec<(u8, u32)>,
    /// 频控阈值。
    pub flood_limit: u32,
    now: u64,
    pub improved: u32,
}

const FLOOD_LIMIT: u32 = 5;

impl FeedbackDesk {
    pub fn new() -> Self {
        FeedbackDesk { flood_limit: FLOOD_LIMIT, ..Default::default() }
    }

    /// 反馈采集：入口可达（域色）+ 内容脱敏双前置；频控窗口拦截。
    pub fn collect(&mut self, id: u32, scene: u8, kind: FbKind, sanitized: bool, reachable: bool) -> Result<(), FbCode> {
        if !reachable {
            return Err(FbCode::EntryUnreachable);
        }
        if !sanitized {
            return Err(FbCode::Unsanitized);
        }
        let cnt = self.flood.iter_mut().find(|f| f.0 == scene);
        match cnt {
            Some(f) if f.1 >= self.flood_limit => return Err(FbCode::FloodBlocked),
            Some(f) => f.1 += 1,
            None => {
                self.flood.push((scene, 1));
            }
        }
        self.items.push(Feedback { id, scene, kind, sanitized, reachable, routed_pool: None, closed_at: 0 });
        Ok(())
    }

    /// 挫败信号自动归集（rage click / dead click ——体验日志联动）。
    pub fn rage_signal(&mut self, id: u32, scene: u8) -> Result<(), FbCode> {
        self.collect(id, scene, FbKind::RageSignal, true, true)
    }

    /// 分诊：体验/挫败 → 优化池；判定争议 → 仲裁。
    pub fn triage(&mut self, id: u32) -> Result<bool, FbCode> {
        let f = self.items.iter_mut().find(|f| f.id == id).ok_or(FbCode::MisRouted)?;
        let to_pool = !matches!(f.kind, FbKind::VerdictDispute);
        f.routed_pool = Some(to_pool);
        Ok(to_pool)
    }

    /// 误分诊复核改道（人工复核翻转去向）。
    pub fn reroute(&mut self, id: u32) -> Result<bool, FbCode> {
        let f = self.items.iter_mut().find(|f| f.id == id).ok_or(FbCode::MisRouted)?;
        match f.routed_pool {
            Some(p) => {
                f.routed_pool = Some(!p);
                Ok(!p)
            }
            None => Err(FbCode::MisRouted),
        }
    }

    /// 改进闭环登记（周期度量样本）。
    pub fn close_loop(&mut self, id: u32) -> Result<u64, FbCode> {
        let now = self.now;
        let f = self.items.iter_mut().find(|f| f.id == id).ok_or(FbCode::MisRouted)?;
        if f.closed_at == 0 {
            f.closed_at = now;
            self.improved += 1;
        }
        Ok(f.closed_at)
    }

    /// 周期指标：平均反馈→改进周期（0 周期不计入）。
    pub fn avg_cycle(&self) -> u64 {
        let n = self.items.iter().filter(|f| f.closed_at > 0).count() as u64;
        if n == 0 {
            return 0;
        }
        let sum: u64 = self.items.iter().filter(|f| f.closed_at > 0).map(|f| f.closed_at).sum();
        sum / n
    }

    /// 周期劣化判定（阈值比较——预警联动对象）。
    pub fn cycle_degraded(&self, budget: u64) -> bool {
        let c = self.avg_cycle();
        c > 0 && c > budget
    }

    pub fn advance(&mut self, dt: u64) {
        self.now += dt;
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves551_checks() -> CheckSet {
    let mut set = CheckSet::new("ves551");
    let mut fd = FeedbackDesk::new();

    // C551-01 一键反馈采集（可达 + 脱敏双前置）。
    chk(&mut set, "C551-01", fd.collect(1, 1, FbKind::Experience, true, true).is_ok() && fd.items.len() == 1, "反馈采集入册");

    // C551-02 入口不可达拒收（域色硬线）。
    chk(&mut set, "C551-02", fd.collect(2, 1, FbKind::Experience, true, false) == Err(FbCode::EntryUnreachable), "不可达拒收");

    // C551-03 未脱敏拒收。
    chk(&mut set, "C551-03", fd.collect(3, 1, FbKind::Experience, false, true) == Err(FbCode::Unsanitized), "未脱敏拒收");

    // C551-04 挫败信号自动归集（rage click 联动）。
    chk(&mut set, "C551-04", fd.rage_signal(4, 2).is_ok() && fd.items[1].kind == FbKind::RageSignal, "挫败信号归集");

    // C551-05 分诊：体验入优化池 / 争议入仲裁。
    let r1 = fd.triage(1);
    chk(&mut set, "C551-05", r1 == Ok(true) && fd.items[0].routed_pool == Some(true), "体验入优化池");
    let _ = fd.collect(5, 3, FbKind::VerdictDispute, true, true);
    chk(&mut set, "C551-05b", fd.triage(5) == Ok(false), "争议入仲裁");

    // C551-06 误分诊复核改道。
    let r2 = fd.reroute(5);
    chk(&mut set, "C551-06", r2 == Ok(true) && fd.items[2].routed_pool == Some(true), "复核改道");

    // C551-07 频控：窗口内超阈拦截。
    let mut fd2 = FeedbackDesk::new();
    let mut blocked = false;
    for i in 0..7u32 {
        if fd2.collect(100 + i, 4, FbKind::Experience, true, true) == Err(FbCode::FloodBlocked) {
            blocked = true;
        }
    }
    chk(&mut set, "C551-07", blocked && fd2.items.len() == 5, "频控超阈拦截");

    // C551-08 改进闭环登记（周期样本）。
    fd.advance(48);
    chk(&mut set, "C551-08", fd.close_loop(1) == Ok(48) && fd.improved == 1, "闭环登记");

    // C551-09 周期指标与劣化判定。
    fd.advance(20);
    let _ = fd.close_loop(4); // 周期 68
    let avg = fd.avg_cycle();
    chk(&mut set, "C551-09", avg == 58 && !fd.cycle_degraded(100) && fd.cycle_degraded(50), "周期指标劣化判定");

    // C551-10 未分诊改道拒绝。
    let _ = fd.collect(6, 5, FbKind::Experience, true, true);
    chk(&mut set, "C551-10", fd.reroute(6) == Err(FbCode::MisRouted), "未分诊改道拒");

    // C551-11 闭环幂等（重复登记不重复计）。
    let c2 = fd.close_loop(1);
    chk(&mut set, "C551-11", c2 == Ok(48) && fd.improved == 1, "闭环幂等");

    // C551-12 无闭环样本周期恒零（不臆测）。
    let fd3 = FeedbackDesk::new();
    chk(&mut set, "C551-12", fd3.avg_cycle() == 0 && !fd3.cycle_degraded(0), "无样本不臆测");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flood_control() {
        let mut fd = FeedbackDesk::new();
        for i in 0..5u32 {
            assert!(fd.collect(i, 9, FbKind::Experience, true, true).is_ok());
        }
        assert_eq!(fd.collect(9, 9, FbKind::Experience, true, true), Err(FbCode::FloodBlocked));
    }

    #[test]
    fn triage_and_reroute() {
        let mut fd = FeedbackDesk::new();
        let _ = fd.collect(1, 1, FbKind::VerdictDispute, true, true);
        assert_eq!(fd.triage(1), Ok(false));
        assert_eq!(fd.reroute(1), Ok(true));
        assert!(fd.reroute(2).is_err());
    }

    #[test]
    fn entry_hardline() {
        let mut fd = FeedbackDesk::new();
        assert_eq!(fd.collect(1, 1, FbKind::Experience, true, false), Err(FbCode::EntryUnreachable));
        assert_eq!(fd.collect(1, 1, FbKind::Experience, false, true), Err(FbCode::Unsanitized));
    }
}
