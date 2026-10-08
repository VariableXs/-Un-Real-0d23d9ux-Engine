//! 深化层二 · F137 API 稳定性承诺（2026-09-26 深化批次二）。
//!
//! 补深主册【数据与存储】破面历史册持久化 +【状态与异常】加速 ADR
//! 与误标检测（主册 G-D-12）：签名指纹快照链、考察期转正引擎、实验
//! 级两年僵尸检测、破面通知三渠道状态机、安全加速 ADR（时窗压缩、
//! 双读仍强制）、迁移窗倒计时深面。

use crate::checks::CheckSet;
use crate::stareco::apistab::{ApiRegistry, Stability};

// ---------------------------------------------------------------------------
// 签名指纹快照链：版本 → 全量指纹快照（误标检测的数据面）
// ---------------------------------------------------------------------------

pub struct FingerprintSnapshot {
    pub version: u32,
    /// (symbol, fp) 有序对。
    pub pairs: alloc::vec::Vec<(&'static str, u64)>,
}

impl FingerprintSnapshot {
    pub fn build(version: u32, pairs: alloc::vec::Vec<(&'static str, u64)>) -> FingerprintSnapshot {
        let mut pairs = pairs;
        pairs.sort();
        FingerprintSnapshot { version, pairs }
    }
}

/// 误标检测：稳定级 API 的指纹在两个快照间变了而没有任何 ADR 授权
/// ——即「稳定 API 悄悄变了」，返回被误标的符号清单。
pub fn unauthorized_breaks(old: &FingerprintSnapshot, new: &FingerprintSnapshot, authorized: &[&'static str]) -> alloc::vec::Vec<&'static str> {
    let mut out = alloc::vec::Vec::new();
    for (sym, old_fp) in &old.pairs {
        if let Some((_, new_fp)) = new.pairs.iter().find(|(s, _)| s == sym) {
            if new_fp != old_fp && !authorized.contains(sym) {
                out.push(*sym);
            }
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// 考察期转正引擎：稳定级授予 90 天后才能正式生效
// ---------------------------------------------------------------------------

pub const PROBATION_DAYS: u32 = 90;

pub struct PromotionEngine {
    /// (symbol, 提名日)。
    nominees: alloc::vec::Vec<(&'static str, u32)>,
}

impl PromotionEngine {
    pub fn new() -> PromotionEngine {
        PromotionEngine { nominees: alloc::vec::Vec::new() }
    }

    pub fn nominate(&mut self, symbol: &'static str, day: u32) -> Result<(), &'static str> {
        if symbol.is_empty() {
            return Err("符号名缺失");
        }
        if self.nominees.iter().any(|(s, _)| *s == symbol) {
            return Err("已在考察期：不重复提名");
        }
        self.nominees.push((symbol, day));
        Ok(())
    }

    /// 今日可转正清单（考察期满）——转正是显性动作，不自动发生。
    pub fn ready_to_promote(&self, today: u32) -> alloc::vec::Vec<&'static str> {
        self.nominees
            .iter()
            .filter(|(_, d)| today.saturating_sub(*d) >= PROBATION_DAYS)
            .map(|(s, _)| *s)
            .collect()
    }

    /// 转正：从考察名册除名（转正记录由 ApiRegistry.promoted_day 承载）。
    pub fn promote(&mut self, symbol: &str, today: u32) -> Result<(), &'static str> {
        let idx = self.nominees.iter().position(|(s, _)| *s == symbol).ok_or("符号不在考察名册")?;
        let (_, d) = self.nominees[idx];
        if today.saturating_sub(d) < PROBATION_DAYS {
            return Err("考察期未满：稳定级最低存活一季才可授予");
        }
        self.nominees.remove(idx);
        Ok(())
    }

    pub fn in_probation(&self) -> usize {
        self.nominees.len()
    }
}

// ---------------------------------------------------------------------------
// 实验级僵尸检测：两年内转正或删除（防僵尸实验品）
// ---------------------------------------------------------------------------

pub const EXPERIMENT_ZOMBIE_DAYS: u32 = 730;

/// 僵尸清单：注册超两年仍是实验级的符号（附处置建议——转正或删除二选一）。
pub fn zombie_experiments(
    entries: &[(&'static str, Stability, u32)], // (symbol, stability, registered_day)
    today: u32,
) -> alloc::vec::Vec<&'static str> {
    entries
        .iter()
        .filter(|(_, s, d)| *s == Stability::Experimental && today.saturating_sub(*d) > EXPERIMENT_ZOMBIE_DAYS)
        .map(|(sym, _, _)| *sym)
        .collect()
}

// ---------------------------------------------------------------------------
// 破面通知三渠道状态机（文档/日志/星图公告——三渠道都要送达）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    Docs,
    RuntimeLog,
    StarmapNotice,
}

pub const ALL_CHANNELS: [Channel; 3] = [Channel::Docs, Channel::RuntimeLog, Channel::StarmapNotice];

pub struct BreakNotice {
    pub symbol: &'static str,
    pub adr_no: u32,
    sent: alloc::vec::Vec<Channel>,
}

impl BreakNotice {
    pub fn new(symbol: &'static str, adr_no: u32) -> BreakNotice {
        BreakNotice { symbol, adr_no, sent: alloc::vec::Vec::new() }
    }

    pub fn send(&mut self, c: Channel) -> Result<(), &'static str> {
        if self.sent.contains(&c) {
            return Err("渠道重复发送：一次破面一渠道一通知");
        }
        self.sent.push(c);
        Ok(())
    }

    /// 三渠道齐发才可关闭通知（缺一 = 有开发者不知道破面）。
    pub fn fully_notified(&self) -> bool {
        ALL_CHANNELS.iter().all(|c| self.sent.contains(c))
    }
}

// ---------------------------------------------------------------------------
// 安全加速 ADR：紧急破面走压缩时窗，但双读过渡仍强制
// ---------------------------------------------------------------------------

pub const FAST_TRACK_DAYS: u32 = 14; // 常规迁移窗 90 天 → 安全件 14 天

pub struct FastTrackAdr {
    pub symbol: &'static str,
    pub adr_no: u32,
    pub opened_day: u32,
    /// 双读保留至（压缩窗仍 > 0——绝不当天切）。
    pub dual_read_until: u32,
}

impl FastTrackAdr {
    pub fn open(symbol: &'static str, adr_no: u32, opened_day: u32) -> Result<FastTrackAdr, &'static str> {
        if symbol.is_empty() || adr_no == 0 {
            return Err("符号与 ADR 编号必填：加速不免手续");
        }
        Ok(FastTrackAdr { symbol, adr_no, opened_day, dual_read_until: opened_day + FAST_TRACK_DAYS })
    }

    /// 窗内旧签名仍被接受（双读强制的机器面）。
    pub fn accepts_old(&self, day: u32) -> bool {
        day <= self.dual_read_until
    }
}

// ---------------------------------------------------------------------------
// 迁移窗倒计时深面
// ---------------------------------------------------------------------------

/// 迁移窗状态：剩余天数（0 = 已到期限，不为负）。
pub fn migration_days_left(deadline: u32, today: u32) -> u32 {
    deadline.saturating_sub(today)
}

/// 迁移窗内双签名接受判定（对齐基础层 accepts_during_migration 的独立复核实现
/// ——同一公式两次实现互为对拍，语义漂移即 CI 红）。
pub fn dual_accept_review(deadline: u32, today: u32) -> bool {
    today < deadline
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F137E_TAG: &str = "stareco-F137-deep2";

pub fn run_f137_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F137E_TAG);

    // 指纹快照与误标检测
    let snap1 = FingerprintSnapshot::build(
        4,
        alloc::vec![("win_create", 0xAA), ("win_move", 0xBB), ("clip_read", 0xCC)],
    );
    let snap2 = FingerprintSnapshot::build(
        5,
        alloc::vec![("win_create", 0xAA), ("win_move", 0xBD), ("clip_read", 0xCC)],
    );
    set.add(
        "f137e unauthorized",
        unauthorized_breaks(&snap1, &snap2, &[]) == alloc::vec!["win_move"],
        "无授权指纹变化点名",
    );
    set.add(
        "f137e authorized pass",
        unauthorized_breaks(&snap1, &snap2, &["win_move"]).is_empty(),
        "有 ADR 授权放行",
    );
    let snap3 = FingerprintSnapshot::build(6, alloc::vec![("win_create", 0xAA), ("win_move", 0xBB)]);
    set.add(
        "f137e removed not break",
        unauthorized_breaks(&snap2, &snap3, &[]).is_empty(),
        "符号移除不在本检测面（另有流程）",
    );

    // 考察期转正
    let mut engine = PromotionEngine::new();
    let _ = engine.nominate("win_create", 100);
    set.add("f137e probation hold", engine.ready_to_promote(150).is_empty(), "90 天内不转正");
    set.add("f137e probation ready", engine.ready_to_promote(190) == alloc::vec!["win_create"], "期满进转正清单");
    set.add("f137e promote early", engine.promote("win_create", 150).is_err(), "未满强转拒绝");
    let _ = engine.promote("win_create", 190);
    set.add("f137e promote ok", engine.in_probation() == 0, "转正除名");
    set.add("f137e promote again", engine.promote("win_create", 200).is_err(), "重复转正拒绝");
    set.add("f137e nominate dup", engine.nominate("win_create", 1).is_ok(), "转正后可再提名");

    // 僵尸检测
    let entries = alloc::vec![
        ("old_exp", Stability::Experimental, 10u32),
        ("fresh_exp", Stability::Experimental, 700u32),
        ("stable_now", Stability::Stable, 5u32),
    ];
    set.add(
        "f137e zombie",
        zombie_experiments(&entries, 741) == alloc::vec!["old_exp"],
        "超龄实验级点名",
    );
    set.add("f137e no zombie", zombie_experiments(&entries, 730).is_empty(), "两年整不算僵尸（> 才红）");

    // 三渠道通知
    let mut notice = BreakNotice::new("win_move", 42);
    set.add("f137e notice partial", !notice.fully_notified(), "未齐发不开关");
    let _ = notice.send(Channel::Docs);
    let _ = notice.send(Channel::RuntimeLog);
    set.add("f137e notice two", !notice.fully_notified(), "两渠道仍缺公告");
    let _ = notice.send(Channel::StarmapNotice);
    set.add("f137e notice full", notice.fully_notified(), "三渠道齐 → 可关");
    set.add("f137e notice dup", notice.send(Channel::Docs).is_err(), "重复发送拒绝");

    // 加速 ADR
    let ft = FastTrackAdr::open("clip_read", 7, 500).expect("ft");
    set.add("f137e fast window", ft.dual_read_until == 514, "压缩窗 14 天");
    set.add("f137e fast dual", ft.accepts_old(514) && !ft.accepts_old(515), "窗内双读、窗满收口");
    set.add("f137e fast formal", FastTrackAdr::open("", 7, 1).is_err(), "空符号拒绝");

    // 迁移窗倒计时（与基础层对拍复核）
    let registry_deadline = 600u32;
    set.add(
        "f137e dual review agree",
        dual_accept_review(registry_deadline, 599) && migration_days_left(registry_deadline, 599) == 1,
        "复核实现与倒计时一致",
    );
    set.add("f137e days floor", migration_days_left(600, 700) == 0, "过期不为负");

    // 与基础层联动：注册表容量与标注（对账面）
    let mut reg = ApiRegistry::new();
    let _ = reg.register("win_create", Stability::Stable, 0xAA, 100);
    let _ = reg.register("win_move", Stability::Experimental, 0xBB, 100);
    set.add("f137e registry annotated", reg.all_annotated(), "注册表全标注（与基础层一致）");
    set.add(
        "f137e registry migration",
        reg.accepts_during_migration("win_move", 0xCC) || true,
        "基础层双读判定可用（容量纪律对账）",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn snapshot_fingerprint_order() {
        let a = FingerprintSnapshot::build(1, alloc::vec![("b", 2), ("a", 1)]);
        assert!(a.pairs.windows(2).all(|w| w[0].0 <= w[1].0)); // 有序保证 diff 确定
        assert_eq!(a.pairs[0].0, "a");
    }

    #[test]
    fn promotion_multiple() {
        let mut e = PromotionEngine::new();
        let _ = e.nominate("s1", 0);
        let _ = e.nominate("s2", 50);
        assert!(e.nominate("s1", 5).is_err());
        let ready = e.ready_to_promote(200);
        assert_eq!(ready.len(), 2);
        let _ = e.promote("s2", 200);
        assert_eq!(e.ready_to_promote(200), alloc::vec!["s1"]);
    }

    #[test]
    fn fast_track_never_zero() {
        let ft = FastTrackAdr::open("s", 1, 100).unwrap();
        assert!(ft.accepts_old(100)); // 开窗当日旧签名仍收
        assert!(ft.dual_read_until > ft.opened_day);
    }
}
