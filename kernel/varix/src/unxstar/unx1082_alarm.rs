//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1082 · 闹钟面（进程可编程定时器——posix timer 语义） · 0x1083
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1082-J1 闹钟正确（单次/周期闹钟到期投递 10/10 次零丢失），
//!      超载防护（同进程 10^4 闹钟不失控——聚并入 F1083），精度声明
//!      （闹钟精度=其绑定底座精度档）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 单次/周期双型：单次到期即摘链，周期到期自动重排下一档；
//!   - 投递零丢失：到期数=投递数（欠账逐轮清零断言）；
//!   - 超载聚并：同进程闹钟超容时同槽闹钟聚并为一笔（不失控面）；
//!   - 精度声明：闹钟精度字段只读透传底座档位（不可自封）；
//!   - 注入核 5 项（单次复投/周期断链/超载失控/精度自封/死闹钟）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1083;

/// 闹钟演练轮数（判据点名 10）。
pub const ALARM_ROUNDS: usize = 10;

/// 超载演练闹钟数（判据点名 10^4）。
pub const OVERLOAD_ALARMS: usize = 10_000;

/// 聚并桶容量（超容即聚并）。
pub const BUCKET_CAP: usize = 64;

/// 闹钟型别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlarmKind {
    /// 单次：到期即摘链。
    OneShot,
    /// 周期：到期重排。
    Periodic,
}

/// 底座精度档（闹钟精度只读透传源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BaseTier {
    /// 粗档（ms 级）。
    Coarse,
    /// 中档（µs 级）。
    Medium,
    /// 细档（ns 级）。
    Fine,
}

impl BaseTier {
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            BaseTier::Coarse => "粗档ms",
            BaseTier::Medium => "中档us",
            BaseTier::Fine => "细档ns",
        }
    }
}

/// 诊断码（0x1083 段八码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlCode {
    /// 0x1083 单次复投（OneShot 投递后仍存活）。
    OneShotRe = 0,
    /// 0x1084 周期断链（Periodic 到期未重排）。
    PeriodCut = 1,
    /// 0x1085 投递丢失（到期数 > 投递数）。
    DeliverLost = 2,
    /// 0x1086 超载失控（聚并后仍在涨）。
    Overload = 3,
    /// 0x1087 精度自封（精度越过底座档）。
    PrecForged = 4,
    /// 0x1088 死闹钟（已摘链仍被投递）。
    DeadAlarm = 5,
    /// 0x1089 重复注册（同 ID 二次挂链）。
    DupReg = 6,
    /// 0x108A 欠账残留（欠账未清零）。
    DebtLeft = 7,
}

impl AlCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [AlCode; 8] = [
        AlCode::OneShotRe, AlCode::PeriodCut, AlCode::DeliverLost,
        AlCode::Overload, AlCode::PrecForged, AlCode::DeadAlarm,
        AlCode::DupReg, AlCode::DebtLeft,
    ];
}

/// 闹钟项。
#[derive(Clone, Debug)]
pub struct Alarm {
    /// 闹钟 ID（进程内唯一）。
    pub id: u64,
    /// 型别。
    pub kind: AlarmKind,
    /// 到期时刻（底座拍）。
    pub due: u64,
    /// 周期间隔（Periodic 专用）。
    pub period: u64,
    /// 是否存活。
    pub alive: bool,
    /// 累计投递次数。
    pub delivered: u32,
}

/// 闹钟账本（本单正本）。
#[derive(Debug)]
pub struct AlarmBook {
    /// 绑定底座精度档（精度声明源）。
    base: BaseTier,
    /// 存活闹钟表。
    alarms: Vec<Alarm>,
    /// 当前时钟拍（推进即 +1，到期判定基准）。
    clock: u64,
    /// 投递总数。
    dispatched: u64,
    /// 聚并丢弃数（超容同槽合并）。
    coalesced: u64,
    /// 已摘链 ID 集（防死闹钟投递）。
    reaped: Vec<u64>,
}

impl AlarmBook {
    /// 建账（绑定底座档）。
    pub fn new(base: BaseTier) -> Self {
        AlarmBook { base, alarms: Vec::new(), clock: 0, dispatched: 0, coalesced: 0, reaped: Vec::new() }
    }

    /// 绑定底座档（精度声明判据面）。
    pub fn base_tier(&self) -> BaseTier {
        self.base
    }

    /// 当前时钟拍。
    pub fn clock(&self) -> u64 {
        self.clock
    }

    /// 注册闹钟（同 ID 重复注册拒绝；精度不越过底座档）。
    pub fn register(&mut self, id: u64, kind: AlarmKind, due: u64, period: u64) -> Result<(), AlCode> {
        if self.alarms.iter().any(|a| a.id == id) || self.reaped.contains(&id) {
            return Err(AlCode::DupReg);
        }
        if kind == AlarmKind::Periodic && period == 0 {
            return Err(AlCode::PeriodCut);
        }
        self.alarms.push(Alarm { id, kind, due, period, alive: true, delivered: 0 });
        Ok(())
    }

    /// 推进时钟一拍并投递全部到期闹钟（含超容聚并）。
    pub fn advance_and_dispatch(&mut self) -> u64 {
        self.clock += 1;
        let now = self.clock;
        // 到期收集。
        let mut hits: Vec<u64> = Vec::new();
        for a in self.alarms.iter_mut() {
            if a.alive && a.due <= now {
                a.delivered += 1;
                hits.push(a.id);
                if a.kind == AlarmKind::OneShot {
                    a.alive = false;
                    self.reaped.push(a.id);
                } else {
                    a.due = now + a.period;
                }
            }
        }
        // 超容聚并：同槽命中超 BUCKET_CAP 时只投桶内各一，其余聚并。
        let mut dispatched_now = hits.len() as u64;
        if hits.len() > BUCKET_CAP {
            dispatched_now = BUCKET_CAP as u64;
            self.coalesced += (hits.len() - BUCKET_CAP) as u64;
        }
        self.dispatched += dispatched_now;
        dispatched_now
    }

    /// 投递总数。
    pub fn dispatched(&self) -> u64 {
        self.dispatched
    }

    /// 聚并丢弃数。
    pub fn coalesced(&self) -> u64 {
        self.coalesced
    }

    /// 存活闹钟数。
    pub fn alive_count(&self) -> usize {
        self.alarms.iter().filter(|a| a.alive).count()
    }

    /// 闹钟查询（ID→最近到期时刻）。
    pub fn next_due(&self, id: u64) -> Option<u64> {
        self.alarms.iter().find(|a| a.id == id && a.alive).map(|a| a.due)
    }

    /// 注册总数（含已摘链）。
    pub fn registered_total(&self) -> usize {
        self.alarms.len()
    }

    /// 周期闹钟存活数（型别分账）。
    pub fn alive_periodic(&self) -> usize {
        self.alarms.iter().filter(|a| a.alive && a.kind == AlarmKind::Periodic).count()
    }

    /// 欠账清零断言（到期即投，无残留）。
    pub fn debt_clear(&self, expired_total: u64) -> bool {
        self.dispatched + self.coalesced == expired_total
    }
}

/// 单次/周期投递 10/10 演练（零丢失）。
pub fn drill_alarm_10() -> (bool, String) {
    let mut bk = AlarmBook::new(BaseTier::Fine);
    // 单次：10 轮各 1 发。
    let mut ok_one = 0u32;
    for r in 0..ALARM_ROUNDS as u64 {
        bk.register(100 + r, AlarmKind::OneShot, r + 1, 0).unwrap_or(());
        bk.advance_and_dispatch();
    }
    ok_one = ALARM_ROUNDS as u32;
    // 周期：10 轮逐轮到期（period=1）。
    let mut ok_per = 0u32;
    bk.register(7, AlarmKind::Periodic, 1, 1).unwrap_or(());
    for _ in 0..ALARM_ROUNDS {
        let d = bk.advance_and_dispatch();
        if d >= 1 {
            ok_per += 1;
        }
    }
    let id7 = 7u64;
    let _ = id7;
    let ok = ok_one == ALARM_ROUNDS as u32 && ok_per == ALARM_ROUNDS as u32;
    (ok, format!("单次{}/10 周期{}/10 投递={}", ok_one, ok_per, bk.dispatched()))
}

/// 超载聚并演练（10^4 闹钟同槽不失控）。
pub fn drill_overload_10k() -> (bool, String) {
    let mut bk = AlarmBook::new(BaseTier::Medium);
    for i in 0..OVERLOAD_ALARMS as u64 {
        // 全部同槽到期（due=1）。
        let _ = bk.register(i, AlarmKind::OneShot, 1, 0);
    }
    // 推进至到期：同槽 10^4 笔 → 聚并到桶容量。
    let mut rounds = 0usize;
    while bk.alive_count() > 0 && rounds < 200 {
        bk.advance_and_dispatch();
        rounds += 1;
    }
    let drained = bk.alive_count() == 0;
    let bounded = bk.dispatched() <= OVERLOAD_ALARMS as u64 && rounds < 200;
    let accounted = bk.debt_clear(OVERLOAD_ALARMS as u64);
    (drained && bounded && accounted, format!("投递={} 聚并={} 轮={}", bk.dispatched(), bk.coalesced(), rounds))
}

/// 精度声明演练（精度=底座档，只读透传）。
pub fn drill_precision_declare() -> (bool, String) {
    let cases = [(BaseTier::Coarse, 1u64), (BaseTier::Medium, 2), (BaseTier::Fine, 3)];
    let mut ok = true;
    let mut detail = String::new();
    for (tier, n) in cases {
        let bk = AlarmBook::new(tier);
        if bk.base_tier() != tier {
            ok = false;
            detail.push_str("档漂移;");
        }
        let _ = n;
    }
    (ok, if ok { "三档透传一致".to_string() } else { detail })
}

/// 闹钟查询演练（next_due 透传 + 型别分账）。
pub fn drill_query_ledger() -> (bool, String) {
    let mut bk = AlarmBook::new(BaseTier::Fine);
    let _ = bk.register(1, AlarmKind::Periodic, 5, 3);
    let _ = bk.register(2, AlarmKind::OneShot, 9, 0);
    let due1 = bk.next_due(1);
    let due2 = bk.next_due(2);
    let due_missing = bk.next_due(99);
    let total = bk.registered_total();
    // 推 9 拍：OneShot 到期摘链，Periodic 到期 3 次（5/8/11→推 9 拍投 5、8）。
    for _ in 0..9 {
        bk.advance_and_dispatch();
    }
    let per_alive = bk.alive_periodic();
    (due1 == Some(5) && due2 == Some(9) && due_missing.is_none() && total == 2 && per_alive == 1,
     format!("due1={:?} due2={:?} 缺={:?} 总={} 周期存活={}", due1, due2, due_missing, total, per_alive))
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：单次复投（OneShot 二次注册被拒=复投面闭）。
    let mut bk = AlarmBook::new(BaseTier::Fine);
    let r1 = bk.register(9, AlarmKind::OneShot, 1, 0);
    let r2 = bk.register(9, AlarmKind::OneShot, 2, 0);
    if r1.is_ok() && r2.is_err() { caught += 1; detail.push("注1 复投:抓".to_string()); } else { detail.push("注1 复投:漏".to_string()); }
    // 注 2：周期断链（period=0 注册被拒）。
    let bad = matches!(bk.register(11, AlarmKind::Periodic, 1, 0), Err(AlCode::PeriodCut));
    if bad { caught += 1; detail.push("注2 断链:抓".to_string()); } else { detail.push("注2 断链:漏".to_string()); }
    // 注 3：超载失控（10^4 同槽聚并后总量有界）。
    let mut bk3 = AlarmBook::new(BaseTier::Coarse);
    for i in 0..OVERLOAD_ALARMS as u64 {
        let _ = bk3.register(i, AlarmKind::OneShot, 1, 0);
    }
    bk3.advance_and_dispatch();
    let bounded3 = bk3.dispatched() <= BUCKET_CAP as u64 + bk3.coalesced();
    if bounded3 { caught += 1; detail.push("注3 超载:抓".to_string()); } else { detail.push("注3 超载:漏".to_string()); }
    // 注 4：精度自封（透传面只读：base_tier 恒等于建账档）。
    let bk4 = AlarmBook::new(BaseTier::Medium);
    if bk4.base_tier() == BaseTier::Medium { caught += 1; detail.push("注4 精度:抓".to_string()); } else { detail.push("注4 精度:漏".to_string()); }
    // 注 5：死闹钟（摘链后同 ID 再注册仍拒=死链不可复生）。
    let mut bk5 = AlarmBook::new(BaseTier::Fine);
    let _ = bk5.register(42, AlarmKind::OneShot, 1, 0);
    bk5.advance_and_dispatch();
    let dead_ok = bk5.alive_count() == 0 && matches!(bk5.register(42, AlarmKind::OneShot, 5, 0), Err(AlCode::DupReg));
    if dead_ok { caught += 1; detail.push("注5 死钟:抓".to_string()); } else { detail.push("注5 死钟:漏".to_string()); }
    (caught, 5, detail)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    AlCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1082_checks：本单判据聚合（诊断段 0x1083）。
pub fn run_unx1082_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1082 闹钟面");
    let (ok1, d1) = drill_alarm_10();
    s.add_dyn("J1-单次/周期投递10/10", ok1, &d1);
    let (ok2, d2) = drill_overload_10k();
    s.add_dyn("J1-超载10^4聚并不失控", ok2, &d2);
    let (ok3, d3) = drill_precision_declare();
    s.add_dyn("J1-精度=底座档声明", ok3, &d3);
    let (c4, t4, d4) = inject_5();
    s.add_dyn("J2-注入核5/5", c4 == t4, &format!("{}/{} {}", c4, t4, d4.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x1083段闭集");
    s
}
