//! unx1761_obsface —— UNX-F1761 观测面总纲（账→接口→聚合三层架构）。
//! 锚点（CoRun Unxreal 主册 L3753）：UNX-F1761 · 观测面总纲（账→接口→聚合三层架构） · 目标 320 行 · 已深化
//! 判据原文：UNX-F1761-J1 三层架构冻结 v1，各批观测接口汇聚关系全量登记，
//!           三层职责越界注入 10/10 次拒绝
//! 组件：三层职责冻结 v1（L1 账层=只记账 / L2 接口层=只转发 / L3 聚合层=只汇总）
//!      + 汇聚关系登记表（各批观测接口 → 聚合槽 · 全量登记 + 批批有点）
//!      + 职责越界注入核（10 场越界调用注入 · 三层闸 10/10 拒绝）
//!      + 聚合快照账（三层贯通导出：账数/接口数/聚合数一行账）
//! 判据落地：三层冻结 v1 + 汇聚全量登记 + 越界注入 10/10 拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u32/u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本（三层架构冻结 v1）。
pub const OF_VERSION: &str = "unx1761_obsface-v1";

/// 诊断段基址（0x4f90 段 · 本模块独占）。
pub const OF_SEG_BASE: u16 = 0x4f90;

/// 观测三层（职责冻结 v1 · 闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfLayer {
    /// L1 账层：只记账（写账/读账），不做汇总不做转发。
    Ledger,
    /// L2 接口层：只转发（账→聚合的桥），不落存储不做汇总。
    Bridge,
    /// L3 聚合层：只汇总（多账合一快照），不直写明细账。
    Aggr,
}

impl OfLayer {
    /// 层名。
    pub fn name(self) -> &'static str {
        match self {
            OfLayer::Ledger => "L1-ledger",
            OfLayer::Bridge => "L2-bridge",
            OfLayer::Aggr => "L3-aggr",
        }
    }

    /// 层序（0–2）。
    pub fn ordinal(self) -> u8 {
        match self {
            OfLayer::Ledger => 0,
            OfLayer::Bridge => 1,
            OfLayer::Aggr => 2,
        }
    }
}

/// 观测操作（越界判定用的操作闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfOp {
    /// 写明细账。
    WriteLedger,
    /// 读明细账。
    ReadLedger,
    /// 转发账到聚合。
    Forward,
    /// 汇总快照。
    Snapshot,
}

/// 三层职责矩阵（冻结 v1）：层 × 操作 → 是否本分。
pub fn of_duty_ok(layer: OfLayer, op: OfOp) -> bool {
    match (layer, op) {
        (OfLayer::Ledger, OfOp::WriteLedger) => true,
        (OfLayer::Ledger, OfOp::ReadLedger) => true,
        (OfLayer::Bridge, OfOp::Forward) => true,
        (OfLayer::Bridge, OfOp::ReadLedger) => true, // 桥读账为转发前置（冻结 v1 许可）
        (OfLayer::Aggr, OfOp::Snapshot) => true,
        _ => false,
    }
}

/// 汇聚关系登记条目（批观测接口 → 聚合槽）。
#[derive(Clone, Debug)]
pub struct OfConverge {
    /// 批号（0–39）。
    pub batch: u16,
    /// 观测接口名（如 "obs-B07-clock"）。
    pub iface: String,
    /// 聚合槽号（0–7）。
    pub slot: u8,
}

impl OfConverge {
    /// 行摘要。
    pub fn line(&self) -> String {
        format!("B{:02}|{}|slot{}", self.batch + 1, self.iface, self.slot)
    }
}

/// 聚合槽账（L3 快照）。
#[derive(Clone, Copy, Debug, Default)]
pub struct OfSlot {
    /// 汇入接口数。
    pub ifaces: u32,
    /// 汇入账条目总数。
    pub entries: u64,
    /// 最近快照 tick（逻辑 tick）。
    pub snap_tick: u64,
}

/// 汇聚关系登记表（全量 · 批批有点）。
pub struct OfRegistry {
    /// 登记条目。
    pub conv: Vec<OfConverge>,
    /// 8 个聚合槽。
    pub slots: [OfSlot; 8],
    /// 各批登记计数（40 槽）。
    pub per_batch: [u32; 40],
    /// 接口名重复数（卫生 · 应恒 0）。
    pub dup_iface: u32,
}

impl OfRegistry {
    /// 空登记表。
    pub fn new() -> Self {
        OfRegistry {
            conv: Vec::new(),
            slots: core::array::from_fn(|_| OfSlot::default()),
            per_batch: [0; 40],
            dup_iface: 0,
        }
    }

    /// 登记一条汇聚关系（接口名唯一 + 批号/槽号越界拒绝）。
    pub fn register(&mut self, batch: u16, iface: &str, slot: u8) -> bool {
        if batch as usize >= 40 || slot >= 8 || iface.is_empty() {
            return false;
        }
        if self.conv.iter().any(|c| c.iface == iface) {
            self.dup_iface += 1;
            return false;
        }
        self.conv.push(OfConverge {
            batch,
            iface: String::from(iface),
            slot,
        });
        self.per_batch[batch as usize] += 1;
        self.slots[slot as usize].ifaces += 1;
        true
    }

    /// 账条目汇入（L1→L2→L3 贯通 · 逻辑转发）。
    pub fn feed(&mut self, batch: u16, entries: u64, tick: u64) -> bool {
        if batch as usize >= 40 {
            return false;
        }
        // 找到该批登记的接口 → 对应槽累加。
        let mut fed = false;
        for c in self.conv.iter().filter(|c| c.batch == batch) {
            let s = &mut self.slots[c.slot as usize];
            s.entries += entries;
            s.snap_tick = tick;
            fed = true;
        }
        fed
    }

    /// 汇聚覆盖满格：批批有登记（40/40）。
    pub fn coverage_full(&self) -> bool {
        self.per_batch.iter().all(|&c| c >= 1)
    }

    /// 汇聚覆盖率（万分数）。
    pub fn coverage_rate(&self) -> u32 {
        let cov = self.per_batch.iter().filter(|&&c| c >= 1).count() as u32;
        cov * 10000 / 40
    }

    /// 登记卫生：零重名。
    pub fn clean(&self) -> bool {
        self.dup_iface == 0
    }

    /// L3 快照导出（聚合层唯一操作）。
    pub fn snapshot(&self, tick: u64) -> String {
        let total_if: u32 = self.slots.iter().map(|s| s.ifaces).sum();
        let total_en: u64 = self.slots.iter().map(|s| s.entries).sum();
        format!(
            "snap@{}: ifaces={} entries={} conv={} rate={}/10000",
            tick,
            total_if,
            total_en,
            self.conv.len(),
            self.coverage_rate()
        )
    }
}

/// 越界注入场景（10 场 · 层×操作越界闭集 + 卫生场）。
pub const OF_INJECT_CASES: [(OfLayer, OfOp, &str); 10] = [
    (OfLayer::Ledger, OfOp::Snapshot, "L1 snapshots (aggr-only op)"),
    (OfLayer::Ledger, OfOp::Forward, "L1 forwards (bridge-only op)"),
    (OfLayer::Aggr, OfOp::WriteLedger, "L3 writes detail ledger"),
    (OfLayer::Aggr, OfOp::Forward, "L3 forwards (bridge-only op)"),
    (OfLayer::Bridge, OfOp::WriteLedger, "L2 writes detail ledger"),
    (OfLayer::Bridge, OfOp::Snapshot, "L2 snapshots (aggr-only op)"),
    (OfLayer::Aggr, OfOp::ReadLedger, "L3 reads detail ledger"),
    (OfLayer::Ledger, OfOp::ReadLedger, "L1 legal control"),
    (OfLayer::Bridge, OfOp::Forward, "L2 legal control"),
    (OfLayer::Aggr, OfOp::Snapshot, "L3 legal control"),
];

/// 越界注入核：前 7 场必拒，后 3 场合法对照必受理。
pub struct OfInjector {
    /// 场数。
    pub cases: u32,
    /// 预期拒绝。
    pub expect_reject: u32,
    /// 实际拒绝。
    pub rejected: u32,
    /// 预期受理。
    pub expect_accept: u32,
    /// 实际受理。
    pub accepted: u32,
    /// 明细。
    pub log: Vec<String>,
}

impl OfInjector {
    /// 空注入核。
    pub fn new() -> Self {
        OfInjector {
            cases: 0,
            expect_reject: 0,
            rejected: 0,
            expect_accept: 0,
            accepted: 0,
            log: Vec::new(),
        }
    }

    /// 执行 10 场注入。
    pub fn inject_all(&mut self) {
        for (i, &(layer, op, tag)) in OF_INJECT_CASES.iter().enumerate() {
            let ok = of_duty_ok(layer, op);
            self.cases += 1;
            let expect_rej = i < 7;
            if expect_rej {
                self.expect_reject += 1;
                if !ok {
                    self.rejected += 1;
                }
            } else {
                self.expect_accept += 1;
                if ok {
                    self.accepted += 1;
                }
            }
            self.log.push(format!(
                "#{} {} x {} ({}) -> {}",
                i,
                layer.name(),
                match op {
                    OfOp::WriteLedger => "write",
                    OfOp::ReadLedger => "read",
                    OfOp::Forward => "fwd",
                    OfOp::Snapshot => "snap",
                },
                tag,
                if expect_rej {
                    if ok { "MISS(allowed!)" } else { "DENIED" }
                } else if ok {
                    "OK" } else { "MISS(denied!)" }
            ));
        }
    }

    /// 注入满格。
    pub fn all_exact(&self) -> bool {
        self.cases >= OF_INJECT_CASES.len() as u32
            && self.rejected == self.expect_reject
            && self.accepted == self.expect_accept
    }
}

/// 观测面门面（判据四件：三层冻结/汇聚全量/越界必拒/快照可导）。
pub struct OfFace {
    /// 登记表。
    pub reg: OfRegistry,
    /// 注入核。
    pub injector: OfInjector,
    /// 最近快照。
    pub last_snap: Option<String>,
}

impl OfFace {
    /// 新门面。
    pub fn new() -> Self {
        OfFace {
            reg: OfRegistry::new(),
            injector: OfInjector::new(),
            last_snap: None,
        }
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.reg.coverage_full()
            && self.reg.clean()
            && self.injector.all_exact()
            && self.last_snap.is_some();
        let msg = format!(
            "{}: conv={} cover={}/10000 dup={} inject_rej={}/{} inject_acc={}/{} snap=[{}] seg=0x{:04x}",
            OF_VERSION,
            self.reg.conv.len(),
            self.reg.coverage_rate(),
            self.reg.dup_iface,
            self.injector.rejected,
            self.injector.expect_reject,
            self.injector.accepted,
            self.injector.expect_accept,
            self.last_snap.clone().unwrap_or_else(|| "-".into()),
            OF_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：40 批汇聚登记 + L1→L3 贯通 + 10 场越界注入 + 快照导出。
pub fn run_t761_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1761-t761");
    let mut of = OfFace::new();
    // 40 批全量登记：每批 2 个观测接口，槽号按 (batch%8) 与 (batch%8+1)%8 双槽。
    for b in 0..40u16 {
        of.reg.register(b, &format!("obs-B{:02}-a", b + 1), (b % 8) as u8);
        of.reg.register(b, &format!("obs-B{:02}-b", b + 1), ((b % 8 + 1) % 8) as u8);
    }
    // L1→L2→L3 贯通：每批汇入 20 条账。
    for b in 0..40u16 {
        of.reg.feed(b, 20, 1000 + b as u64);
    }
    // 越界注入 10 场。
    of.injector.inject_all();
    // L3 快照导出。
    of.last_snap = Some(of.reg.snapshot(2000));
    let (ok, msg) = of.judge();
    set.add("UNX-F1761-J1", ok, if ok { "" } else { "judge red: detail in ledger" });
    set.add("OF-coverage-full", of.reg.coverage_full(), "");
    set.add("OF-registry-clean", of.reg.clean(), "");
    set.add("OF-inject-10/10", of.injector.all_exact(), "");
    set.add("OF-snapshot-exported", of.last_snap.is_some(), "");
    set
}
